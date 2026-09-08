// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-License-Identifier: GPL-3.0-or-later

//! The daemon contract, in one file (rule 9's discipline, a second time).
//!
//! Newline-delimited JSON over a Unix socket, the same shape as the sidecar's
//! protocol and for the same reason: one file holds the whole surface, so a
//! request and its handler cannot drift apart across a release.
//!
//! **Field names are snake_case, tag values are camelCase.** The fields match
//! the domain types `Entry` carries verbatim — and those are also
//! `library.json`'s format, so renaming them would silently orphan every
//! cached library on disk. Tags are names rather than fields: `playPause`.
//!
//! **The client sends intent, never state.** `vinilod` owns the sidecar and
//! the mirror; a frontend asks for things and is told what happened. That is
//! rule 3 one hop further out — MusicKit owns the queue, the daemon mirrors it,
//! and clients mirror the daemon.

use serde::{Deserialize, Serialize};

use crate::sort::SortBy;

use crate::entry::Entry;
use crate::player::protocol::{Item, RepeatMode};
use crate::queue::Start;

fn runtime_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|d| !d.is_empty())?;
    Some(std::path::PathBuf::from(dir))
}

/// Where the daemon listens. `$XDG_RUNTIME_DIR` is per-user and cleared on
/// logout, which is what a socket wants — a stale one in `~` outlives the
/// process that made it.
pub fn socket_path() -> Option<std::path::PathBuf> {
    Some(runtime_dir()?.join("vinilo.sock"))
}

fn pid_path() -> Option<std::path::PathBuf> {
    Some(runtime_dir()?.join("vinilo.pid"))
}

fn source_stamp_path() -> Option<std::path::PathBuf> {
    Some(runtime_dir()?.join("vinilo.source"))
}

/// Record which source this process is serving, so a later client can tell a
/// Spotify daemon from an Apple Music one without asking it.
pub fn write_runtime_identity(source: crate::provider::Provider) {
    if let Some(path) = pid_path() {
        let _ = std::fs::write(path, std::process::id().to_string());
    }
    if let Some(path) = source_stamp_path() {
        let _ = std::fs::write(path, source.as_str());
    }
}

/// Forget the stamp. Called on a clean shutdown so a leftover pid file cannot
/// SIGKILL whatever reused that number.
pub fn clear_runtime_identity() {
    if let Some(path) = pid_path() {
        let _ = std::fs::remove_file(path);
    }
    if let Some(path) = source_stamp_path() {
        let _ = std::fs::remove_file(path);
    }
}

fn running_source() -> Option<crate::provider::Provider> {
    let text = std::fs::read_to_string(source_stamp_path()?).ok()?;
    crate::provider::Provider::parse(&text)
}

fn running_pid() -> Option<u32> {
    let text = std::fs::read_to_string(pid_path()?).ok()?;
    text.trim().parse().ok()
}

fn pid_is_alive(pid: u32) -> bool {
    if pid == 0 || pid == 1 {
        return false;
    }
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn pid_is_vinilod(pid: u32) -> bool {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
    comm.trim() == "vinilod"
}

fn signal_pid(pid: u32, sig: i32) {
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

/// True when the live daemon was booted for a different source.
///
/// A missing stamp is not a reason to kill anyone: the replacement we just
/// started has not written it yet, and killing it is how switching sources
/// spun forever.
pub fn source_needs_replace(
    running: Option<crate::provider::Provider>,
    wanted: crate::provider::Provider,
) -> bool {
    running.is_some_and(|source| source != wanted)
}

fn ask_daemon_to_quit(path: &std::path::Path) {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    let Ok(mut stream) = UnixStream::connect(path) else {
        return;
    };
    let _ = stream.write_all(b"{\"req\":\"quit\"}\n");
    let _ = stream.flush();
}

/// Drop the socket and stamp only if this process still owns them.
///
/// A dying Spotify daemon must not unlink the socket a new Apple Music daemon
/// has already bound, or every client reconnects into a loop.
pub fn release_runtime_if_ours() {
    if running_pid() != Some(std::process::id()) {
        return;
    }
    if let Some(path) = socket_path() {
        let _ = std::fs::remove_file(path);
    }
    clear_runtime_identity();
}

struct SpawnLock(Option<std::fs::File>);

impl SpawnLock {
    fn acquire() -> Self {
        use std::os::fd::AsRawFd;

        let Some(path) = runtime_dir().map(|dir| dir.join("vinilo.lock")) else {
            return Self(None);
        };
        let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
        else {
            return Self(None);
        };
        unsafe {
            libc::flock(file.as_raw_fd(), libc::LOCK_EX);
        }
        Self(Some(file))
    }
}

impl Drop for SpawnLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;

        if let Some(file) = &self.0 {
            unsafe {
                libc::flock(file.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}

/// Stop a live `vinilod` so the next connect starts one for the current source.
///
/// Preferences rewrite the provider file and send Quit, but a daemon stuck in
/// a Spotify request never reads that line — and even when it does, the next
/// `connect()` can attach to the dying process. Switching Spotify → Apple
/// Music then talks to a sidecar-less daemon, which is why Play did nothing.
pub fn stop_running_daemon() {
    let target = running_pid();
    let path = socket_path();
    if let Some(path) = &path {
        ask_daemon_to_quit(path);
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
    while std::time::Instant::now() < deadline {
        let socket_gone = path.as_ref().is_none_or(|p| !p.exists());
        let pid_gone = target.is_none_or(|pid| !pid_is_alive(pid));
        if socket_gone && pid_gone {
            if running_pid() == target {
                clear_runtime_identity();
            }
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    if let Some(pid) = target
        && running_pid() == Some(pid)
        && pid != std::process::id()
        && pid_is_vinilod(pid)
        && pid_is_alive(pid)
    {
        tracing::warn!(pid, "vinilod ignored Quit — stopping it");
        signal_pid(pid, libc::SIGTERM);
        std::thread::sleep(std::time::Duration::from_millis(400));
        if running_pid() == Some(pid) && pid_is_alive(pid) {
            signal_pid(pid, libc::SIGKILL);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    if running_pid() != target && running_pid().is_some() {
        // A replacement already claimed the runtime files.
        return;
    }
    if let Some(path) = path {
        let _ = std::fs::remove_file(path);
    }
    if running_pid() == target {
        clear_runtime_identity();
    }
}

/// Connect to the daemon, starting it if it is not there.
///
/// **Vinilo is an app you open, not a service you enable.** Nobody should have
/// to run `systemctl --user enable` before music works, so the first client to
/// arrive starts the daemon and the rest find it already running. A unit file
/// ships for anyone who wants playback to survive closing every window, but it
/// is an option rather than a step.
///
/// The race is benign: two clients starting at once both spawn, and the second
/// daemon exits on its own — binding the socket is what settles who owns it.
///
/// A daemon booted for a *different* source is not reused. It is stopped, then
/// a new process reads the provider file.
pub fn connect_or_spawn(exe: &std::path::Path) -> std::io::Result<std::os::unix::net::UnixStream> {
    use std::os::unix::net::UnixStream;

    let path = socket_path().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "XDG_RUNTIME_DIR is not set, so there is nowhere to put the socket",
        )
    })?;

    let _lock = SpawnLock::acquire();

    let wanted = crate::provider::load().unwrap_or_default();
    if let Ok(stream) = UnixStream::connect(&path) {
        if !source_needs_replace(running_source(), wanted) {
            return Ok(stream);
        }
        drop(stream);
        tracing::info!(
            running = ?running_source(),
            ?wanted,
            "replacing vinilod — music source changed"
        );
        stop_running_daemon();
    }

    tracing::info!(daemon = %exe.display(), "no daemon listening — starting one");
    let mut command = std::process::Command::new(exe);
    command.stdin(std::process::Stdio::null());
    // Default: the daemon is a session of its own, so it must not keep the
    // client's pipes. When `RUST_LOG` is set we keep stderr, so one terminal
    // running `vinilo` shows catalogue writes (create playlist, likes).
    if std::env::var_os("RUST_LOG").is_none() {
        command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    }

    // **Its own session, or the terminal takes it with it.** A plain child
    // inherits the client's process group *and* its session, and so does the
    // sidecar one level down — so closing the terminal SIGHUPs all three.
    // Chromium traps on SIGHUP and dumps core, the daemon dies before its
    // SIGTERM handler can save the session or clear the socket, and the music
    // stops. Measured: closing the window left a 7.8 MB Electron core dump and
    // an orphaned socket every time.
    //
    // `setsid` is the whole fix. It is the one thing that has to happen between
    // `fork` and `exec`, which is what `pre_exec` is for — and why it is
    // `unsafe`: only async-signal-safe calls are legal in that window, and
    // `setsid` is one.
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn()?;

    // **Reaped, or it becomes a zombie.** Dropping a `Child` neither kills nor
    // waits, so a daemon that dies while this client is still running would sit
    // in the process table until the client exited — and a daemon that keeps
    // crashing would leave one behind each time. The thread costs nothing: it
    // blocks for the daemon's whole life and then ends.
    let mut child = child;
    std::thread::Builder::new()
        .name("vinilod-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        })?;

    // It binds the socket before it does anything else, so this is the daemon
    // coming up rather than the sidecar — a fraction of a second, not the
    // seconds Chromium takes.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
        if let Ok(stream) = UnixStream::connect(&path) {
            return Ok(stream);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "the daemon did not start listening",
    ))
}

/// Client → daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "req")]
pub enum Request {
    /// Drive playback. The daemon translates to the sidecar; nothing here says
    /// what the resulting state will be, because only MusicKit decides that.
    #[serde(rename = "transport")]
    Transport(Transport),

    /// Move within the queue the daemon already holds. **Not** a way to build
    /// one — that would cost the gapless buffer (rule 3).
    #[serde(rename = "jumpTo")]
    JumpTo { index: usize },

    /// Everything the daemon is holding, once. Sent on connect by a client that
    /// needs to draw before anything changes.
    #[serde(rename = "snapshot")]
    Snapshot,

    /// The queue as the daemon mirrors it.
    #[serde(rename = "queue")]
    Queue,

    /// Where the daemon stands right now.
    ///
    /// **Asked for, because [`Event::Stage`] only fires on a change.** A client
    /// attaching to a daemon that has been ready for an hour would otherwise
    /// never be told it was ready, and would draw its startup screen forever.
    #[serde(rename = "stage")]
    Stage,

    /// Start receiving [`Event`]s on this connection. Idempotent.
    #[serde(rename = "subscribe")]
    Subscribe,

    /// Show Apple's own sign-in.
    ///
    /// **The one thing that needs a window**, and the daemon has none of its
    /// own — it hands this to the sidecar, whose hidden Chromium is shown for
    /// exactly this and hidden again afterwards.
    #[serde(rename = "signIn")]
    SignIn,

    /// End the Apple session. Clears the cookies too, which only MusicKit's own
    /// context can do.
    #[serde(rename = "signOut")]
    SignOut,

    /// A page of the library. `query` filters, `offset`/`limit` window it —
    /// a client draws a screenful, not 535 rows.
    #[serde(rename = "browse")]
    Browse {
        view: View,
        #[serde(default)]
        query: String,
        #[serde(default)]
        offset: usize,
        /// Zero means "the rest".
        #[serde(default)]
        limit: usize,
        /// What to order by, and which way round.
        ///
        /// **The daemon sorts, not the client**, because the sort has to happen
        /// before the window is taken — ordering one page of a library is not
        /// ordering the library, and a client asking for rows 0..40 of an
        /// unsorted list gets the wrong forty.
        #[serde(default)]
        sort: SortBy,
        #[serde(default)]
        reverse: bool,
    },

    /// Search all of Apple Music, not just the library.
    ///
    /// **Here rather than in a client, because this is what needs the tokens**,
    /// and rule 7 keeps those in one process. `offset` pages the same way the
    /// catalog does — a client asks for more as somebody scrolls.
    #[serde(rename = "search")]
    Search {
        query: String,
        #[serde(default)]
        filter: CatalogFilter,
        #[serde(default)]
        offset: usize,
    },

    /// Open an album, artist or playlist. Fetched from Apple if it is not
    /// already known, which is why the answer arrives as an event rather than
    /// a return value.
    #[serde(rename = "open")]
    Open { kind: PageKind, id: String },

    /// Listen Now / Discover: recently played, recommendations, charts.
    #[serde(rename = "discover")]
    Discover,

    /// Grow the queue MusicKit already holds, without rebuilding it.
    ///
    /// **Not a `Play`**, and that is the point: rebuilding a queue to add a
    /// track restarts playback and discards the gapless buffer (rule 3).
    #[serde(rename = "enqueue")]
    Enqueue {
        ids: Vec<String>,
        /// Right after the current track, rather than at the end.
        #[serde(default)]
        next: bool,
    },

    /// Drop one track from the loaded queue, by its position.
    #[serde(rename = "removeFromQueue")]
    RemoveFromQueue { index: usize },

    /// Move one track within the loaded queue. `to` is where it lands *after*
    /// it has been taken out, which is what a drag naturally means.
    #[serde(rename = "moveInQueue")]
    MoveInQueue { from: usize, to: usize },

    /// Empty the queue and stop.
    #[serde(rename = "clearQueue")]
    ClearQueue,

    /// Change what the signed-in catalogue holds for this account.
    ///
    /// `id` is the track (or the playlist, for removals). `playlist_id` and
    /// `name` are only set for playlist writes.
    #[serde(rename = "write")]
    Write {
        action: WriteAction,
        id: String,
        #[serde(default)]
        playlist_id: Option<String>,
        #[serde(default)]
        name: Option<String>,
    },

    /// Re-read the library from Apple. Happens on its own once tokens arrive;
    /// this is for a client offering a reload button.
    #[serde(rename = "refresh")]
    Refresh,

    /// Build a queue from these ids and start playing.
    ///
    /// **Ids, not indices into something the daemon remembers.** The client
    /// already holds the rows it drew; sending them back is what keeps the
    /// daemon from having to mirror every client's scroll position.
    #[serde(rename = "play")]
    Play {
        ids: Vec<String>,
        /// Which of `ids` to open on. Ignored when `start` is `shuffled` —
        /// MusicKit reorders as it loads, so the row we name is not the row it
        /// opens on (#152).
        #[serde(default)]
        index: usize,
        #[serde(default)]
        start: PlayMode,
    },

    /// Play files (or folders of files) from this computer.
    ///
    /// **Not catalog ids.** MusicKit cannot open a path, so the daemon's own
    /// decoder handles these. A second `playFiles` replaces the local queue
    /// the way `play` replaces the MusicKit one.
    #[serde(rename = "playFiles")]
    PlayFiles {
        paths: Vec<String>,
        #[serde(default)]
        index: usize,
    },

    /// Lyrics for the track currently in the expanded player.
    ///
    /// Fetched here rather than in a client because it needs the same partner
    /// token / InnerTube cookie the catalogue already holds (rule 8).
    #[serde(rename = "lyrics")]
    Lyrics { id: String },

    /// Stop the daemon: stop playing, save the session, exit.
    ///
    /// **Refused while another client is attached.** Quitting takes the player
    /// from every window on this machine, so a client can only ask for it when
    /// it is the last one holding the socket — otherwise the answer is an
    /// [`Event::Error`] and the client should just leave. That is also why
    /// MPRIS `Quit` is ignored outright: a media key must never mean this.
    #[serde(rename = "quit")]
    Quit,
}

/// Which kinds a catalog search should answer for.
///
/// Apple answers only for the kinds named, and leaves a key out entirely rather
/// than returning it empty — so this is a filter on the request, not on what
/// comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogFilter {
    #[default]
    All,
    Songs,
    Albums,
    Artists,
    Playlists,
}

impl CatalogFilter {
    /// The `types=` value Apple wants.
    /// Every filter, in the order a client should step through them.
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Songs,
        Self::Albums,
        Self::Artists,
        Self::Playlists,
    ];

    pub fn label(self) -> &'static str {
        crate::i18n::t(match self {
            Self::All => crate::i18n::Key::FilterEverything,
            Self::Songs => crate::i18n::Key::Songs,
            Self::Albums => crate::i18n::Key::Albums,
            Self::Artists => crate::i18n::Key::Artists,
            Self::Playlists => crate::i18n::Key::Playlists,
        })
    }

    pub fn types(self) -> &'static str {
        match self {
            Self::All => "songs,albums,artists,playlists",
            Self::Songs => "songs",
            Self::Albums => "albums",
            Self::Artists => "artists",
            Self::Playlists => "playlists",
        }
    }
}

/// Something we can ask the signed-in catalogue to do to this account.
///
/// The daemon picks the route: MusicKit for Apple Music, Pathfinder for
/// Spotify, InnerTube for YouTube Music, Tidal's own REST. A client does not
/// need to know which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WriteAction {
    Favorite,
    Unfavorite,
    AddToLibrary,
    RemoveFromLibrary,
    CreatePlaylist,
    AddToPlaylist,
    RemoveFromPlaylist,
}

/// Which library section to browse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum View {
    Songs,
    Albums,
    Artists,
    Playlists,
}

/// What a [`Request::Open`] is opening.
///
/// Catalog and library are separate variants because **the two id spaces are
/// not interchangeable** — a catalog id 404s against `/me/library` and back
/// again — and because an artist page is their *albums* rather than their
/// tracks, which is a different call entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageKind {
    Album,
    Artist,
    Playlist,
    LibraryAlbum,
    LibraryArtist,
    LibraryPlaylist,
}

/// The mode a queue is *created* in, on the wire.
///
/// Mirrors [`Start`], which is the type the arithmetic uses. Two enums because
/// this one is a contract with clients and that one is internal — and because
/// `Start::Clicked` needs a name a client can understand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayMode {
    /// A row click: this list, from here, in order.
    #[default]
    Clicked,
    /// A Play button: in order, whatever mode was on before.
    InOrder,
    /// A Shuffle button.
    Shuffled,
}

impl From<PlayMode> for Start {
    fn from(mode: PlayMode) -> Self {
        match mode {
            PlayMode::Clicked => Start::Clicked,
            PlayMode::InOrder => Start::InOrder,
            PlayMode::Shuffled => Start::Shuffled,
        }
    }
}

/// The transport verbs, kept separate from [`Request`] so a client can pass one
/// around without carrying the rest of the protocol.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(tag = "cmd", rename_all = "camelCase")]
pub enum Transport {
    Play,
    Pause,
    PlayPause,
    Next,
    Previous,
    /// Absolute, in milliseconds.
    Seek {
        position_ms: u64,
    },
    SetVolume {
        volume: f64,
    },
    SetShuffle {
        shuffle: bool,
    },
    SetRepeat {
        mode: RepeatMode,
    },
}

/// Daemon → client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum Event {
    /// What is playing and how. The one a bar redraws from.
    #[serde(rename = "snapshot")]
    Snapshot(Snapshot),

    /// The queue changed — reordered, replaced, or emptied. Carries the whole
    /// list: it is bounded by what MusicKit will hold, and a diff protocol here
    /// would be a second reconciliation to keep honest.
    #[serde(rename = "queue")]
    Queue {
        items: Vec<QueueItem>,
        position: usize,
    },

    /// Where the daemon is in its own startup, or why it is not playable.
    #[serde(rename = "stage")]
    Stage(Stage),

    /// A window of the library, answering a [`Request::Browse`].
    #[serde(rename = "rows")]
    Rows {
        view: View,
        entries: Vec<Entry>,
        /// How many matched before `offset`/`limit`, so a client can show a
        /// scrollbar without asking for everything.
        total: usize,
    },

    /// Catalog results, answering a [`Request::Search`].
    ///
    /// Carries the query it answers: a client types faster than Apple replies,
    /// and a result for two keystrokes ago must not replace one for the word
    /// that is actually in the box.
    #[serde(rename = "results")]
    Results {
        query: String,
        entries: Vec<Entry>,
        offset: usize,
        /// Whether asking again at a higher offset is worth it.
        more: bool,
    },

    /// An opened album, artist or playlist.
    ///
    /// `header` is the thing itself — the album with its artwork and year, the
    /// artist with their portrait — because a page draws that above its rows
    /// and asking for it separately would be a second round trip for one object
    /// the fetch already had.
    #[serde(rename = "page")]
    Page {
        kind: PageKind,
        id: String,
        header: Entry,
        entries: Vec<Entry>,
    },

    /// Listen Now shelves, answering a [`Request::Discover`].
    #[serde(rename = "discover")]
    Discover(crate::discover::Discover),

    /// The library changed under a client — a refresh landed, or a write
    /// settled. **An invalidation, not the rows**: a client asks for the page
    /// it is drawing rather than having 535 pushed at it.
    #[serde(rename = "libraryChanged")]
    LibraryChanged,

    /// Whether the daemon is replacing the complete library cache.
    #[serde(rename = "libraryRefreshing")]
    LibraryRefreshing { refreshing: bool },

    /// Lyrics for a track, answering a [`Request::Lyrics`].
    #[serde(rename = "lyrics")]
    Lyrics {
        id: String,
        lyrics: Option<Lyrics>,
        #[serde(default)]
        error: Option<String>,
    },

    /// Something went wrong that a person should see.
    #[serde(rename = "error")]
    Error { detail: String },
}

/// Timed or plain lyrics for one track.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Lyrics {
    /// True when `lines` carry `start_ms` that tracks playback.
    pub synced: bool,
    pub lines: Vec<LyricLine>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LyricLine {
    pub start_ms: u64,
    pub text: String,
}

/// One row of the queue.
///
/// Not `protocol::Item`, whose doc says it stays inside `player/` — and it
/// should: that is MusicKit's shape, carrying two id spaces and an artwork
/// template a client has no business resolving. This is what a row needs to
/// draw itself and to be jumped to.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct QueueItem {
    /// Whichever id the daemon can act on — catalog if there is one, else the
    /// library id. Opaque to the client; it comes back in `JumpTo`.
    pub id: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
}

impl From<&Item> for QueueItem {
    fn from(item: &Item) -> Self {
        Self {
            id: item.catalog_id.clone().or_else(|| item.id.clone()),
            title: item.title.clone(),
            artist: item.artist.clone(),
            album: item.album.clone(),
            duration_ms: item.duration_ms,
        }
    }
}

/// What is playing, flattened for a client that only draws.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    /// Which track this is, so a client can tell one snapshot from the next
    /// without consulting the queue. **They arrive as separate events**, and a
    /// client that asks the queue "what is playing" while holding a snapshot
    /// from before the change gets one answer from each.
    pub track_id: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// A local file, already fetched. Clients do not talk to Apple for art.
    pub art_path: Option<String>,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub playing: bool,
    /// Still working towards audio. A client should not read this as paused.
    pub busy: bool,
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub can_next: bool,
    pub can_previous: bool,
}

/// How far along the daemon is. A client draws something different for each.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "stage", rename_all = "camelCase")]
pub enum Stage {
    /// The sidecar is up but MusicKit has not attached yet. Also what a client
    /// assumes before it has asked.
    #[default]
    Connecting,
    /// Ready to play.
    Ready,
    /// No Apple session. A client cannot fix this — sign-in needs a window.
    SignedOut,
    /// Broken in a way a restart will not fix.
    Broken { detail: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire names are the contract. A rename here is a breaking change for
    /// every client, so it fails a test rather than a user's bar.
    #[test]
    fn requests_carry_the_names_clients_send() {
        let seek =
            serde_json::to_string(&Request::Transport(Transport::Seek { position_ms: 4200 }))
                .unwrap();
        assert_eq!(
            seek,
            r#"{"req":"transport","cmd":"seek","position_ms":4200}"#
        );

        let sub = serde_json::to_string(&Request::Subscribe).unwrap();
        assert_eq!(sub, r#"{"req":"subscribe"}"#);

        let files = serde_json::to_string(&Request::PlayFiles {
            paths: vec!["/tmp/a.mp3".into()],
            index: 0,
        })
        .unwrap();
        assert_eq!(
            files,
            r#"{"req":"playFiles","paths":["/tmp/a.mp3"],"index":0}"#
        );
    }

    #[test]
    fn a_request_round_trips_through_its_own_wire_form() {
        let sent = Request::Transport(Transport::SetRepeat {
            mode: RepeatMode::All,
        });
        let line = serde_json::to_string(&sent).unwrap();
        let back: Request = serde_json::from_str(&line).unwrap();
        assert!(matches!(
            back,
            Request::Transport(Transport::SetRepeat {
                mode: RepeatMode::All
            })
        ));
    }

    #[test]
    fn library_refresh_state_is_explicit_on_the_wire() {
        let started =
            serde_json::to_string(&Event::LibraryRefreshing { refreshing: true }).unwrap();
        let finished =
            serde_json::to_string(&Event::LibraryRefreshing { refreshing: false }).unwrap();

        assert_eq!(
            started,
            r#"{"event":"libraryRefreshing","refreshing":true}"#
        );
        assert_eq!(
            finished,
            r#"{"event":"libraryRefreshing","refreshing":false}"#
        );
    }

    #[test]
    fn an_older_write_without_playlist_fields_still_parses() {
        let req: Request =
            serde_json::from_str(r#"{"req":"write","action":"favorite","id":"sp:abc"}"#).unwrap();
        match req {
            Request::Write {
                action: WriteAction::Favorite,
                id,
                playlist_id,
                name,
            } => {
                assert_eq!(id, "sp:abc");
                assert!(playlist_id.is_none());
                assert!(name.is_none());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_new_playlist_write_carries_the_name() {
        let line = serde_json::to_string(&Request::Write {
            action: WriteAction::CreatePlaylist,
            id: String::new(),
            playlist_id: None,
            name: Some("Noche".into()),
        })
        .unwrap();
        assert!(line.contains("createPlaylist"));
        assert!(line.contains("Noche"));
    }

    #[test]
    fn an_unknown_request_is_an_error_rather_than_a_default() {
        // A client from a newer version must fail loudly here, not be silently
        // read as something else — which is what an untagged enum would do.
        let bad = serde_json::from_str::<Request>(r#"{"req":"teleport"}"#);
        assert!(bad.is_err());
    }

    #[test]
    fn a_daemon_for_another_source_is_replaced() {
        use crate::provider::Provider;
        assert!(!source_needs_replace(None, Provider::AppleMusic));
        assert!(source_needs_replace(
            Some(Provider::Spotify),
            Provider::AppleMusic
        ));
        assert!(!source_needs_replace(
            Some(Provider::AppleMusic),
            Provider::AppleMusic
        ));
        assert!(source_needs_replace(
            Some(Provider::AppleMusic),
            Provider::Spotify
        ));
    }

    #[test]
    fn the_socket_lives_under_the_runtime_dir() {
        // Not `~`: a socket that outlives the session is a socket that answers
        // for a daemon which is not running.
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000") };
        assert_eq!(
            socket_path().unwrap(),
            std::path::PathBuf::from("/run/user/1000/vinilo.sock")
        );
    }
}
