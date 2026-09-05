// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-License-Identifier: GPL-3.0-or-later

//! The socket, the sidecar, and the loop that keeps them agreeing.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use vinilo_core::artwork;
use vinilo_core::catalog;
use vinilo_core::entry::Entry;
use vinilo_core::ipc::{
    self, CatalogFilter, Event, PageKind, Request, Stage, Transport, WriteAction,
};
use vinilo_core::music::client::Client;
use vinilo_core::music::types::Artwork;
use vinilo_core::player::protocol::{Command, Event as PlayerEvent, PlaybackState};
use vinilo_core::player::{Incoming, sidecar};
use vinilo_core::queue::{Start, queue_from_ids, start_index, unresolvable_ids};

use crate::bus;
use crate::heal;
use crate::state::Model;
use crate::watchdog;

/// How many events a slow client may fall behind before it is dropped.
///
/// A client that cannot keep up with a 500ms tick is not going to catch up, and
/// holding the backlog for it would make every other client's memory its
/// problem. `broadcast` tells it how far it lagged; it can ask for a snapshot.
const BACKLOG: usize = 64;

/// Position ticks while playing. The same cadence the GTK client uses.
const TICK_MS: u64 = 500;

/// How long with nobody listening and nothing playing before the sidecar goes.
///
/// It costs **393 MB of the daemon's 404** and near-zero CPU, so this is about
/// resident memory, not battery. Long enough that closing a window and
/// reopening it does not pay for a restart; short enough that a machine left
/// alone gets its memory back.
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// How often to ask whether the sidecar is still wanted.
const IDLE_CHECK: std::time::Duration = std::time::Duration::from_secs(30);

pub struct Daemon {
    pub model: RefCell<Model>,
    /// Replaced on every respawn, so everything holding an `Rc<Daemon>` keeps
    /// talking to whichever sidecar is alive now.
    pub sidecar: RefCell<Option<sidecar::Handle>>,
    pub events: broadcast::Sender<Event>,
    /// Whether the session has been put back since the current sidecar started.
    pub restored: std::cell::Cell<bool>,
    /// Consecutive failed starts, for the backoff. Reset once MusicKit attaches.
    pub restarts: std::cell::Cell<u32>,
    /// The last queue we asked for, and the track we aimed at. Kept for the
    /// dead-track retry, which has to rebuild it without the ids Apple refused.
    pub last_queue: RefCell<Option<(Vec<String>, Option<String>)>>,
    /// The track a new queue was meant to open on, until `verify_start` has
    /// confirmed it. `None` for a shuffled queue, which has no wrong answer.
    pub pending_start: RefCell<Option<String>>,
    /// Whether a non-playing `play` has already been healed once. A second
    /// attempt that also does nothing is a real failure.
    pub healed: std::cell::Cell<bool>,
    /// A position to restore once the reloaded track is current.
    pub resume_at: std::cell::Cell<Option<u64>>,
    /// An occurrence selected through MPRIS, restarted when that exact item arrives.
    pub restart_at: RefCell<Option<String>>,
    /// A finished command whose ending state is worth judging, once the mirror
    /// has caught up with it.
    pub after_apply: std::cell::Cell<Option<String>>,
    /// The bus. Filled in after the daemon exists, because it needs one.
    pub mpris: RefCell<Option<vinilo_core::mpris::Mpris>>,
    /// Whether a library fetch is in flight, so a reload button cannot stack
    /// four of them. The generation lets a new account refresh without waiting
    /// for the old account's request to finish.
    pub refreshing: std::cell::Cell<Option<u64>>,
    /// Changes whenever authorization crosses an account boundary.
    pub authorization_generation: std::cell::Cell<u64>,
    /// The artwork template already fetched, so a 500ms tick does not ask again
    /// for a file that is on disk.
    pub art_for: RefCell<Option<String>>,
    /// Clients currently connected. The sidecar is only worth holding while
    /// somebody is listening, or something is playing for them to come back to.
    pub clients: std::cell::Cell<usize>,
    /// Set when the sidecar was stopped on purpose, so the supervisor waits to
    /// be asked rather than treating it as a crash.
    pub idle: std::cell::Cell<bool>,
    /// Rung when a client turns up and the sidecar is not running.
    pub wake: tokio::sync::Notify,
    /// The desktop's own volume for this application, or `None` where there is
    /// no audio server to talk to.
    pub mixer: Option<crate::mixer::Mixer>,
    /// Files on this computer. Empty until a client asks to play some.
    pub local: RefCell<crate::local::Player>,
    /// Rung by [`Request::Quit`], and answered where SIGTERM is — so leaving on
    /// purpose and being stopped by a service manager take the same path.
    pub quitting: tokio::sync::Notify,
    /// Last catalog id written to the listen history, so a 500ms tick does not
    /// rewrite the same song.
    pub last_listen: RefCell<Option<String>>,
    /// Search hits for Spotify / YouTube Music / Tidal, keyed by the id we
    /// minted (`yt:`, `sp:`, `td:`), so a later Play can fetch audio.
    pub stream_hits: RefCell<HashMap<String, vinilo_core::streams::StreamHit>>,
    /// The source this process booted with. Preferences may rewrite the
    /// provider file before we quit; the queue we save still belongs to this.
    pub source: vinilo_core::provider::Provider,
    /// A Refresh arrived while a fetch was already running. Run another when
    /// the current one finishes, so the window connecting at boot is not a no-op.
    pub refresh_again: std::cell::Cell<bool>,
    /// A library write landed while a fetch was already running. Always run
    /// another when that fetch finishes, even if it succeeded — otherwise a
    /// new playlist never appears until the next launch.
    pub refresh_dirty: std::cell::Cell<bool>,
}

impl Daemon {
    pub fn send(&self, cmd: Command) {
        match self.sidecar.borrow().as_ref() {
            Some(handle) => handle.send(cmd),
            // Only reachable in the moment between a client connecting and the
            // sidecar coming back — under three seconds, and nothing has been
            // drawn yet for anyone to click.
            None => tracing::debug!(cmd = cmd.name(), "dropped: the sidecar is asleep"),
        }
    }

    /// Whether the sidecar is worth holding: somebody is listening, or
    /// something is playing for them to come back to.
    fn wanted(&self) -> bool {
        self.clients.get() > 0 || self.model.borrow().player.state.is_playing()
    }

    pub fn publish(&self, event: Event) {
        // An error here means nobody is listening, which is the ordinary state
        // of a daemon with no client attached.
        let _ = self.events.send(event);
    }

    /// A client for Apple's API, if the tokens for one have arrived.
    fn client(&self) -> Option<Client> {
        let model = self.model.borrow();
        let tokens = model.tokens.as_ref()?;
        Some(Client::new(
            tokens.developer_token.clone(),
            tokens.music_user_token.clone(),
            tokens.storefront.clone(),
        ))
    }

    fn publish_snapshot(&self) {
        self.record_listen();
        self.publish(Event::Snapshot(self.model.borrow().snapshot()));
        if let Some(mpris) = self.mpris.borrow().as_ref() {
            mpris.update(bus::state(self));
        }
    }

    fn record_listen(&self) {
        let model = self.model.borrow();
        if !model.player.state.is_playing() {
            return;
        }
        let Some(item) = model.player.now_playing.as_ref() else {
            return;
        };
        let Some(id) = item.catalog_id.clone().or_else(|| item.id.clone()) else {
            return;
        };
        if self.last_listen.borrow().as_deref() == Some(id.as_str()) {
            return;
        }
        let track = vinilo_core::music::types::Track {
            id: vinilo_core::music::types::TrackId(id.clone()),
            catalog_id: Some(id.clone()),
            favorite: false,
            in_library: false,
            library_id: None,
            date_added: String::new(),
            year: String::new(),
            title: item.title.clone(),
            artist: item.artist.clone(),
            album: item.album.clone(),
            duration_ms: item.duration_ms,
            track_number: item.track_number,
            artwork: item.artwork_template.clone().map(Artwork::new),
        };
        drop(model);
        *self.last_listen.borrow_mut() = Some(id);
        vinilo_core::listen_history::record(track);
    }
}

pub async fn run() -> Result<()> {
    let path = ipc::socket_path().context("XDG_RUNTIME_DIR is not set")?;

    // **Ask before removing.** A socket file outlives the process that made it,
    // so a crash leaves one behind that accepts nothing — but unlinking it and
    // binding a fresh inode would let a *second* daemon start beside a live
    // one. Both would spawn a sidecar, and the Chromium profile lock is the
    // thing this whole design exists to respect. So: if something answers, it
    // is not ours to replace.
    if path.exists() {
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(_) => anyhow::bail!("a daemon is already listening on {}", path.display()),
            // Nobody home — the file is a leftover.
            Err(_) => {
                tracing::info!(socket = %path.display(), "removing a stale socket");
                std::fs::remove_file(&path).with_context(|| format!("removing {path:?}"))?;
            }
        }
    }
    let source = vinilo_core::provider::load().unwrap_or_default();
    let listener = UnixListener::bind(&path).with_context(|| format!("binding {path:?}"))?;
    vinilo_core::ipc::write_runtime_identity(source);
    tracing::info!(socket = %path.display(), source = %source.as_str(), "listening");
    let local_only = !source.needs_sidecar();

    let (sidecar_handle, incoming) = if local_only {
        tracing::info!("music source does not need MusicKit — not starting the sidecar");
        (None, None)
    } else {
        let (handle, incoming) = sidecar::spawn().context("spawning the sidecar")?;
        (Some(handle), Some(incoming))
    };
    let (events, _) = broadcast::channel(BACKLOG);
    let daemon = Rc::new(Daemon {
        model: RefCell::new(Model::new()),
        sidecar: RefCell::new(sidecar_handle),
        local: RefCell::new(crate::local::Player::new()),
        events,
        restored: std::cell::Cell::new(false),
        restarts: std::cell::Cell::new(0),
        last_queue: RefCell::new(None),
        pending_start: RefCell::new(None),
        healed: std::cell::Cell::new(false),
        resume_at: std::cell::Cell::new(None),
        restart_at: RefCell::new(None),
        after_apply: std::cell::Cell::new(None),
        mpris: RefCell::new(None),
        refreshing: std::cell::Cell::new(None),
        authorization_generation: std::cell::Cell::new(0),
        art_for: RefCell::new(None),
        clients: std::cell::Cell::new(0),
        idle: std::cell::Cell::new(false),
        wake: tokio::sync::Notify::new(),
        quitting: tokio::sync::Notify::new(),
        mixer: crate::mixer::Mixer::start(),
        last_listen: RefCell::new(None),
        stream_hits: RefCell::new(vinilo_core::streams::load_hits()),
        source,
        refresh_again: std::cell::Cell::new(false),
        refresh_dirty: std::cell::Cell::new(false),
    });

    // After the `Rc` exists: MPRIS holds one so a button on a bar can reach the
    // sidecar.
    *daemon.mpris.borrow_mut() = Some(bus::start(&daemon));

    // Accept loop: one task per client, each holding a handle to the daemon.
    let accepting = daemon.clone();
    tokio::task::spawn_local(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let daemon = accepting.clone();
                    tokio::task::spawn_local(async move {
                        // Counted around the connection rather than inside it,
                        // so an error on the way out still releases the sidecar.
                        daemon.clients.set(daemon.clients.get() + 1);
                        // Somebody is here: if the sidecar was put down for
                        // being unwanted, it is wanted again. Woken on connect
                        // rather than on the first command, so it is ready by
                        // the time anything has been drawn to click.
                        if daemon.sidecar.borrow().is_none() {
                            daemon.wake.notify_one();
                        }
                        if let Err(err) = client(stream, daemon.clone()).await {
                            tracing::debug!(?err, "client gone");
                        }
                        daemon.clients.set(daemon.clients.get().saturating_sub(1));
                        tracing::debug!(clients = daemon.clients.get(), "client left");
                    });
                }
                Err(err) => {
                    tracing::error!(?err, "accept failed");
                    return;
                }
            }
        }
    });

    // Position ticks. Only while playing — a paused player's position is a
    // fact, not something to extrapolate from.
    let ticking = daemon.clone();
    tokio::task::spawn_local(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(TICK_MS));
        let mut watch = watchdog::Watch::default();
        loop {
            tick.tick().await;
            // **Whoever moved it, everyone hears.** The desktop's audio panel
            // can change this without Vinilo being asked, and a client still
            // showing the old number is the disagreement this whole thing
            // moved to fix.
            let local_active = ticking.local.borrow().is_active();
            // The Pulse mixer looks up `application.name = Vinilo`. While a
            // file is playing that name still matches the paused sidecar, so
            // reading it would snap the slider back to Apple Music's gain.
            let outside = if local_active {
                None
            } else {
                ticking.mixer.as_ref().and_then(|m| m.current())
            };
            let moved = outside.is_some_and(|v| (v - ticking.model.borrow().volume).abs() > 0.005);
            if moved {
                ticking.model.borrow_mut().volume = outside.unwrap_or_default();
            }
            if local_active {
                if ticking.local.borrow().ended() {
                    let _ = ticking.local.borrow_mut().advance_ended();
                    publish_local(&ticking);
                } else if ticking.local.borrow().is_playing() {
                    let event = ticking.local.borrow().position_event();
                    ticking.model.borrow_mut().player.apply(&event);
                    ticking.publish_snapshot();
                } else if moved {
                    ticking.publish_snapshot();
                }
                watchdog::check(&ticking, &mut watch);
                continue;
            }
            if moved || ticking.model.borrow().player.state.is_playing() {
                ticking.publish_snapshot();
            }
            // On every tick, not only the playing ones: a stall is measured
            // against a position that has stopped moving, and the state that
            // says so is exactly the one that stops publishing.
            watchdog::check(&ticking, &mut watch);
        }
    });

    // **Give the memory back when nobody wants it.** The sidecar is a hidden
    // Chromium: 393 MB of the daemon's 404, and measured at 0.22% of a core
    // doing nothing. Holding it while no client is connected and nothing is
    // playing buys nothing at all — and starting it again costs 0.6s warm,
    // 2.4s cold, which is cheaper than the memory it was holding.
    let idling = daemon.clone();
    tokio::task::spawn_local(async move {
        let mut unwanted_since: Option<std::time::Instant> = None;
        loop {
            tokio::time::sleep(IDLE_CHECK).await;
            if idling.sidecar.borrow().is_none() {
                unwanted_since = None;
                continue;
            }
            if idling.wanted() {
                unwanted_since = None;
                continue;
            }
            let since = *unwanted_since.get_or_insert_with(std::time::Instant::now);
            if since.elapsed() < IDLE_TIMEOUT {
                continue;
            }
            unwanted_since = None;

            tracing::info!("no clients and nothing playing — stopping the sidecar");
            // Written first: after this the mirror is the only record of what
            // was loaded, and it is about to be emptied.
            save_session(&idling);
            idling.idle.set(true);
            // Dropping the handle closes the child's stdin, which is the
            // shutdown signal `main.js` waits on; `kill_on_drop` is the backstop.
            idling.sidecar.borrow_mut().take();
            idling.model.borrow_mut().stage = Stage::Connecting;
            idling.publish(Event::Stage(Stage::Connecting));
        }
    });

    // **Leaving is a thing to do properly.** A service manager stops this with
    // SIGTERM, and that is the last moment the position is accurate — after it,
    // the process is gone and the session file still says where the last track
    // change left off.
    let leaving = daemon.clone();
    tokio::task::spawn_local(async move {
        let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        else {
            return;
        };
        let mut interrupt =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()) {
                Ok(sig) => sig,
                Err(_) => return,
            };
        let why = tokio::select! {
            _ = term.recv() => "SIGTERM",
            _ = interrupt.recv() => "SIGINT",
            _ = leaving.quitting.notified() => "a client quit",
        };
        tracing::info!(why, "shutting down");
        save_session(&leaving);
        // Only if we still own them. Unlinking after a replacement has bound
        // the path is how switching Spotify → Apple Music never finished.
        vinilo_core::ipc::release_runtime_if_ours();
        std::process::exit(0);
    });

    if local_only {
        if let Some(language) = vinilo_core::i18n::load() {
            vinilo_core::i18n::set_current(language);
        }
        daemon.model.borrow_mut().stage = Stage::Ready;
        daemon.publish(Event::Stage(Stage::Ready));
        if source.is_catalog() {
            refresh_library(&daemon);
        }
        // The sidecar loop below is what keeps `run` from returning. Without
        // one, wait here: clients still attach, files still play, and SIGTERM
        // still takes the process out through the task above.
        std::future::pending::<()>().await;
    }

    let mut incoming = incoming.expect("MusicKit sidecar");

    // **The daemon outlives its sidecar.** A child that dies takes the queue
    // with it, but not the process every client is connected to — so this
    // respawns rather than returning, and the session it saves on the way down
    // is what puts playback back where it was.
    loop {
        while let Some(message) = incoming.recv().await {
            match message {
                Incoming::Event(event) => on_event(&daemon, event),
                Incoming::Unparsed(line) => {
                    tracing::warn!(%line, "sidecar sent a line we could not parse")
                }
                Incoming::Died(why) => {
                    tracing::error!(%why, "sidecar died");
                    daemon.publish(Event::Stage(Stage::Broken { detail: why }));
                    break;
                }
            }
        }

        // **The player is gone, so nothing is playing.** Carrying the last
        // state across the gap reports `playing` over a sidecar that no longer
        // exists — the lie rule 6 exists to prevent — and it is load-bearing
        // for the watchdog: a restored queue loads *without* starting, so its
        // position is legitimately frozen, and a stale `playing` beside it read
        // as a wedge. That restarted a healthy replacement every twelve
        // seconds, forever. The queue and position are untouched; only the
        // claim to be playing goes.
        daemon.model.borrow_mut().player.state = PlaybackState::None;
        daemon.restart_at.borrow_mut().take();
        daemon.publish_snapshot();

        // Written from the live mirror, not from what was on disk: a crash
        // mid-track should come back to that track, not to last night's.
        save_session(&daemon);

        // Put down on purpose: wait to be asked rather than backing off, and do
        // not count it as a failure.
        if daemon.idle.replace(false) {
            tracing::info!("sidecar stopped; waiting for a client");
            daemon.wake.notified().await;
            tracing::info!("a client turned up — starting the sidecar");
            daemon.restarts.set(0);
        }

        let attempt = daemon.restarts.get();
        daemon.restarts.set(attempt + 1);
        if attempt > 0 {
            let delay = sidecar::restart_delay(attempt - 1);
            tracing::warn!(attempt, ?delay, "restarting the sidecar");
            tokio::time::sleep(delay).await;
        }
        daemon.publish(Event::Stage(Stage::Connecting));

        match sidecar::spawn() {
            Ok((handle, rx)) => {
                *daemon.sidecar.borrow_mut() = Some(handle);
                // The new child holds nothing, so the session goes back in once
                // MusicKit attaches to it.
                daemon.restored.set(false);
                daemon.model.borrow_mut().stage = Stage::Connecting;
                incoming = rx;
            }
            Err(err) => {
                // Reported down the same path as a crash, so there is one
                // recovery route rather than two.
                tracing::error!(?err, "could not respawn the sidecar");
                daemon.publish(Event::Stage(Stage::Broken {
                    detail: err.to_string(),
                }));
            }
        }
    }
}

/// Handle MusicKit's all-or-nothing `NOT_FOUND` by dropping the ids it named
/// and trying again.
///
/// Returns whether it took ownership of the error, so the caller does not also
/// tell a client about something it cannot act on.
fn retry_without_dead_tracks(daemon: &Daemon, detail: &str) -> bool {
    let dead = unresolvable_ids(detail);
    if dead.is_empty() {
        return false;
    }
    let Some((songs, wanted)) = daemon.last_queue.borrow_mut().take() else {
        return false;
    };

    let newly_dead = {
        let mut model = daemon.model.borrow_mut();
        let before = model.dead_ids.len();
        model.dead_ids.extend(dead);
        model.dead_ids.len() - before
    };

    // Nothing new: the retry already happened and failed again. Stop, or we
    // loop for ever on an error we cannot parse our way out of.
    if newly_dead == 0 {
        tracing::warn!("queue still unresolvable after dropping known-dead ids");
        return false;
    }
    vinilo_core::unplayable::save(&daemon.model.borrow().dead_ids);

    let dead_ids = daemon.model.borrow().dead_ids.clone();
    let retry: Vec<String> = songs
        .into_iter()
        .filter(|id| !dead_ids.contains(id))
        .collect();
    if retry.is_empty() {
        daemon.publish(Event::Error {
            detail: "None of these tracks are available to stream".into(),
        });
        return true;
    }

    let position = start_index(&retry, wanted.as_ref());
    tracing::info!(
        dropped = newly_dead,
        queue = retry.len(),
        position,
        "retrying queue without unresolvable tracks"
    );
    *daemon.pending_start.borrow_mut() = wanted.clone();
    *daemon.last_queue.borrow_mut() = Some((retry.clone(), wanted));
    daemon.send(Command::SetQueue {
        songs: retry,
        start_position: position,
        start_playing: true,
        start_time_ms: 0,
    });
    true
}

/// Remember the queue and where we are in it.
///
/// Called on every track change *and* when the sidecar dies. The second is the
/// one that matters: a crash is exactly when nobody gets to save anything, so
/// the worst case is coming back to the start of the right track rather than to
/// nothing at all.
fn save_session(daemon: &Daemon) {
    let model = daemon.model.borrow();
    let songs: Vec<String> = model
        .player
        .queue
        .iter()
        .filter_map(|item| item.catalog_id.clone().or_else(|| item.id.clone()))
        .collect();

    if songs.is_empty() {
        vinilo_core::session::clear();
        return;
    }

    vinilo_core::session::save(&vinilo_core::session::Session {
        start: model
            .player
            .queue_position
            .min(songs.len().saturating_sub(1)),
        position_ms: model.player.position_ms,
        provider: daemon.source.as_str().to_owned(),
        songs,
    });
}

/// Playback-shaped MusicKit events. Tokens, library writes and hook
/// lifecycle still have to flow while a file is playing.
fn is_musickit_playback(event: &PlayerEvent) -> bool {
    matches!(
        event,
        PlayerEvent::NowPlaying { .. }
            | PlayerEvent::PlaybackState { .. }
            | PlayerEvent::Position { .. }
            | PlayerEvent::Queue(_)
            | PlayerEvent::Modes { .. }
    )
}

fn on_event(daemon: &Rc<Daemon>, event: PlayerEvent) {
    // The name, not the payload: a `NowPlaying` carries the whole queue, and
    // 112 items per line is not observability.
    tracing::debug!(event = event_name(&event), "sidecar event");
    // Stage transitions before the mirror moves, so a client is told the daemon
    // is ready by the same pass that makes it so.
    let signed_out = matches!(
        &event,
        PlayerEvent::HookReady {
            authorized: false,
            ..
        } | PlayerEvent::Authorization { authorized: false }
            | PlayerEvent::SignedOut
    );
    match &event {
        PlayerEvent::HookReady { authorized, .. } => {
            daemon.restarts.set(0);
            set_authorization(daemon, *authorized);
        }
        PlayerEvent::Authorization { authorized } => set_authorization(daemon, *authorized),
        PlayerEvent::SignedOut => set_authorization(daemon, false),
        PlayerEvent::HookFailed { detail } => {
            let stage = Stage::Broken {
                detail: detail.clone(),
            };
            daemon.model.borrow_mut().stage = stage.clone();
            daemon.publish(Event::Stage(stage));
        }
        PlayerEvent::Error { detail, .. } => {
            // A refused queue is worth healing rather than reporting: `setQueue`
            // is all-or-nothing, so one delisted track makes a whole playlist
            // unplayable and the person can do nothing about it.
            if !retry_without_dead_tracks(daemon, detail) {
                daemon.publish(Event::Error {
                    detail: detail.clone(),
                });
            }
        }
        // Older installed sidecars still report MusicKit's persisted gain.
        // The desktop stream owns volume now, so accepting this would let a
        // hidden second gain overwrite the number every client displays.
        PlayerEvent::Volume { .. } => {}
        // The sidecar's half of a library write. Removing and un-favouriting
        // can only be done by MusicKit itself, so they settle here rather than
        // where the REST writes do — and the library is stale until a refresh
        // says otherwise.
        PlayerEvent::LibraryWrite {
            kind,
            id,
            ok,
            detail,
            ..
        } => {
            if *ok {
                tracing::info!(%kind, %id, "library write settled");
                refresh_library(daemon);
            } else {
                tracing::warn!(%kind, %id, %detail, "library write refused");
                daemon.publish(Event::Error {
                    detail: detail.clone(),
                });
            }
        }
        PlayerEvent::CmdDone { cmd, .. } => {
            // Checked after the mirror moves, below — the state this reports is
            // the one the command ended in.
            let cmd = cmd.clone();
            daemon.after_apply.set(Some(cmd));
        }
        PlayerEvent::Tokens(tokens) => {
            // Never the token itself (rule 7) — only whether we have the one
            // that matters. A developer token alone gets catalog search but not
            // playback, and the difference is otherwise invisible.
            tracing::info!(
                storefront = %tokens.storefront,
                authorized = tokens.authorized,
                has_user_token = tokens.music_user_token.is_some(),
                "tokens harvested"
            );
            let first_usable = {
                let mut model = daemon.model.borrow_mut();
                let first_usable = model
                    .tokens
                    .as_ref()
                    .and_then(|old| old.music_user_token.as_ref())
                    .is_none()
                    && tokens.authorized
                    && tokens.music_user_token.is_some();
                model.tokens = Some(tokens.clone());
                first_usable
            };
            // Fresh sign-in can report authorization before its tokens. The
            // first refresh then has no client, so usable tokens retry it.
            if first_usable {
                refresh_library(daemon);
            }
        }
        _ => {}
    }

    if signed_out {
        return;
    }

    // Files on this computer own the now-playing bar. MusicKit stays paused
    // with its last item, and every pause/nowPlaying/queue echo would put
    // that Apple track back on the bar — title, artist and cover — while
    // rodio is already playing the file.
    if daemon.local.borrow().is_active() && is_musickit_playback(&event) {
        tracing::debug!(
            event = event_name(&event),
            "local files own the player — ignoring MusicKit playback"
        );
        return;
    }

    let queue_changed = matches!(
        event,
        PlayerEvent::Queue(_) | PlayerEvent::NowPlaying { .. }
    );
    daemon.model.borrow_mut().player.apply(&event);

    if queue_changed {
        let (items, position) = daemon.model.borrow().queue();
        tracing::debug!(len = items.len(), position, "queue changed");
        daemon.publish(Event::Queue { items, position });
        // On every track change, because shutdown is the moment that might not
        // run — a SIGKILL, a session ending badly.
        save_session(daemon);
    }
    // The position goes back once there is an item to seek within, which is
    // what `nowPlayingItemDidChange` means.
    if matches!(event, PlayerEvent::NowPlaying { .. }) {
        heal::resume_position(daemon);
        if let Some(command) = restart_selected_occurrence(daemon) {
            daemon.send(command);
        }
        fetch_artwork(daemon);
    }
    if let Some(cmd) = daemon.after_apply.take() {
        heal::play_did_nothing(daemon, &cmd);
    }
    // After the mirror has the new queue, confirm MusicKit put us on the track
    // that was actually asked for.
    heal::verify_start(daemon);

    daemon.publish_snapshot();
}

fn set_authorization(daemon: &Rc<Daemon>, authorized: bool) {
    let stage = if authorized {
        Stage::Ready
    } else {
        Stage::SignedOut
    };
    if daemon.model.borrow().stage != stage {
        daemon
            .authorization_generation
            .set(daemon.authorization_generation.get().saturating_add(1));
    }

    if !authorized {
        clear_account_state(daemon);
        return;
    }

    let restore = !daemon.restored.replace(true);
    daemon.model.borrow_mut().stage = Stage::Ready;
    daemon.publish(Event::Stage(Stage::Ready));
    daemon.send(Command::Hide);
    // Once per run: the hook re-attaches on every navigation, and a second
    // restore would throw away whatever is playing by then.
    if restore {
        restore_session(daemon);
    }
    refresh_library(daemon);
}

fn clear_account_state(daemon: &Daemon) {
    let mut model = daemon.model.borrow_mut();
    model.clear_account_state();
    model.stage = Stage::SignedOut;
    drop(model);

    daemon.last_queue.borrow_mut().take();
    daemon.pending_start.borrow_mut().take();
    daemon.healed.set(false);
    daemon.resume_at.set(None);
    daemon.restart_at.borrow_mut().take();
    daemon.after_apply.take();
    daemon.art_for.borrow_mut().take();
    vinilo_core::library_cache::clear();
    vinilo_core::page_cache::clear();
    vinilo_core::discover::clear();
    vinilo_core::listen_history::clear();
    vinilo_core::session::clear();

    daemon.publish(Event::Stage(Stage::SignedOut));
    daemon.publish(Event::Queue {
        items: Vec::new(),
        position: 0,
    });
    daemon.publish_snapshot();
    daemon.publish(Event::LibraryChanged);
}

/// Is there a queue with nothing open in it?
///
/// The signature of a restore: tracks loaded, `startPlaying: false`, so
/// MusicKit holds a queue with no now-playing item to press play on.
fn needs_opening(daemon: &Daemon) -> bool {
    let model = daemon.model.borrow();
    model.player.now_playing.is_none() && !model.player.queue.is_empty()
}

async fn client(stream: UnixStream, daemon: Rc<Daemon>) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let mut feed: Option<broadcast::Receiver<Event>> = None;

    loop {
        // Either the client says something, or — once subscribed — the daemon
        // does. `biased` so a request is never starved by a busy event stream.
        let outgoing = tokio::select! {
            biased;
            line = lines.next_line() => match line? {
                Some(line) => answer(&line, &daemon, &mut feed),
                None => return Ok(()), // client hung up
            },
            event = recv(&mut feed) => Some(event?),
        };

        if let Some(event) = outgoing {
            let mut line = serde_json::to_vec(&event)?;
            line.push(b'\n');
            write.write_all(&line).await?;
        }
    }
}

/// Wait on the event feed, or forever if this client has not subscribed.
///
/// `select!` needs both arms to be futures; a client that never subscribes
/// simply has one that never completes.
async fn recv(feed: &mut Option<broadcast::Receiver<Event>>) -> Result<Event> {
    match feed {
        Some(rx) => Ok(rx.recv().await?),
        None => std::future::pending().await,
    }
}

fn answer(
    line: &str,
    daemon: &Rc<Daemon>,
    feed: &mut Option<broadcast::Receiver<Event>>,
) -> Option<Event> {
    let request: Request = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(err) => {
            // Loudly, per rule 4: a client sending something we cannot read is
            // a version mismatch, and silence would make it look like a hang.
            tracing::warn!(%line, ?err, "unreadable request");
            return Some(Event::Error {
                detail: format!("unreadable request: {err}"),
            });
        }
    };

    match request {
        Request::Subscribe => {
            if feed.is_none() {
                *feed = Some(daemon.events.subscribe());
            }
            if !daemon.model.borrow().library.is_empty() {
                daemon.publish(Event::LibraryChanged);
            }
            // The first thing a subscriber gets is where things stand, so it
            // never has to draw an empty bar waiting for a change.
            Some(Event::Snapshot(daemon.model.borrow().snapshot()))
        }
        Request::Snapshot => Some(Event::Snapshot(daemon.model.borrow().snapshot())),
        Request::Stage => Some(Event::Stage(daemon.model.borrow().stage.clone())),
        Request::Queue => {
            let (items, position) = daemon.model.borrow().queue();
            Some(Event::Queue { items, position })
        }
        Request::JumpTo { index } => {
            if daemon.local.borrow().is_active() {
                return match daemon.local.borrow_mut().jump(index) {
                    Ok(()) => {
                        publish_local(daemon);
                        None
                    }
                    Err(detail) => Some(Event::Error { detail }),
                };
            }
            // Nothing pending: there is no new queue to verify against.
            *daemon.pending_start.borrow_mut() = None;
            daemon.restart_at.borrow_mut().take();
            daemon.send(Command::ChangeToIndex { index });
            // Clicking a row is a request to *play* it, and
            // `changeToMediaAtIndex` only moves the cursor.
            if !daemon.model.borrow().player.state.is_playing() {
                daemon.send(Command::Play);
            }
            None
        }
        Request::Browse {
            view,
            query,
            offset,
            limit,
            sort,
            reverse,
        } => {
            let model = daemon.model.borrow();
            let (entries, total) = model
                .library
                .browse(view, &query, offset, limit, sort, reverse);
            Some(Event::Rows {
                view,
                entries,
                total,
            })
        }
        Request::SignIn => {
            daemon.send(Command::ShowLogin);
            None
        }
        Request::SignOut => {
            daemon.send(Command::SignOut);
            None
        }
        Request::Search {
            query,
            filter,
            offset,
        } => {
            search(daemon, query, filter, offset);
            None
        }
        Request::Open { kind, id } => {
            // Answered off this task: opening a page is a network round trip,
            // and a client waiting on one must not stop the daemon answering
            // everyone else.
            open_page(daemon, kind, id);
            None
        }
        Request::Discover => {
            discover(daemon);
            None
        }
        Request::Quit => {
            // Counted around the connection, so this client is in the total.
            // Anyone else attached is a window that would lose its player.
            if daemon.clients.get() > 1 {
                return Some(Event::Error {
                    detail: "Another Vinilo client is still open".into(),
                });
            }
            tracing::info!("a client asked to quit");
            // **Through the same door a service manager uses.** Saving the
            // session and clearing the socket are already done properly on
            // SIGTERM; a second copy of that here is a second one to keep
            // right. Raising it on ourselves also lets this return first, so
            // the client is not writing into a socket that is already gone.
            daemon.quitting.notify_one();
            None
        }
        Request::Play { ids, index, start } => {
            let provider = daemon.source;
            if provider.is_catalog() {
                play_streams(daemon, ids, index, start.into());
                None
            } else {
                let songs: Vec<String> = ids
                    .into_iter()
                    .filter(|id| !vinilo_core::streams::StreamHit::is_stream_id(id))
                    .collect();
                if songs.is_empty() {
                    return Some(Event::Error {
                        detail: "Those tracks belong to another music source".into(),
                    });
                }
                if provider.needs_apple() && daemon.sidecar.borrow().is_none() {
                    daemon.wake.notify_one();
                    return Some(Event::Error {
                        detail: vinilo_core::i18n::t(vinilo_core::i18n::Key::Connecting).into(),
                    });
                }
                stop_local(daemon);
                play(daemon, &songs, index, start.into());
                None
            }
        }
        Request::PlayFiles { paths, index } => play_files(daemon, paths, index),
        Request::Enqueue { ids, next } => {
            let provider = daemon.source;
            if provider.is_catalog() {
                if daemon.local.borrow().is_active() {
                    enqueue_streams(daemon, ids);
                    return None;
                }
                play_streams(daemon, ids, 0, Start::InOrder);
                return None;
            }
            if daemon.local.borrow().is_active() {
                return Some(Event::Error {
                    detail: "Can't add Apple Music tracks to a file that's playing".into(),
                });
            }
            // Filtered, because a dead id rejects the whole insert the same way
            // it rejects a whole queue.
            let dead = daemon.model.borrow().dead_ids.clone();
            let songs: Vec<String> = ids
                .into_iter()
                .filter(|id| {
                    !dead.contains(id) && !vinilo_core::streams::StreamHit::is_stream_id(id)
                })
                .collect();
            if songs.is_empty() {
                return Some(Event::Error {
                    detail: "Nothing here can be streamed".into(),
                });
            }
            tracing::info!(count = songs.len(), next, "enqueueing");
            daemon.send(if next {
                Command::PlayNext { songs }
            } else {
                Command::PlayLater { songs }
            });
            None
        }
        Request::RemoveFromQueue { index } => {
            if daemon.local.borrow().is_active() {
                return match daemon.local.borrow_mut().remove(index) {
                    Ok(()) => {
                        publish_local(daemon);
                        None
                    }
                    Err(detail) => Some(Event::Error { detail }),
                };
            }
            // **MusicKit will not remove the track it is playing**, and says
            // nothing when asked to: `queue.remove(current)` returns, fires no
            // event, and leaves the queue as it was. Measured on a five-track
            // queue — index 3 removed, index 0 did not, no error either time.
            // Silence would look like the click missed.
            let model = daemon.model.borrow();
            if index == model.player.queue_position && !model.player.queue.is_empty() {
                return Some(Event::Error {
                    detail: "Can't remove the track that's playing".into(),
                });
            }
            drop(model);
            daemon.send(Command::RemoveFromQueue { index });
            None
        }
        Request::MoveInQueue { from, to } => {
            if daemon.local.borrow().is_active() {
                return match daemon.local.borrow_mut().move_item(from, to) {
                    Ok(()) => {
                        publish_local(daemon);
                        None
                    }
                    Err(detail) => Some(Event::Error { detail }),
                };
            }
            // Optimistic, like the GTK client's drag: the mirror moves now and
            // MusicKit's echo confirms it, so a client redrawing from the next
            // snapshot does not see the row spring back.
            if daemon.model.borrow_mut().player.move_item(from, to) {
                daemon.send(Command::MoveInQueue { from, to });
                let (items, position) = daemon.model.borrow().queue();
                return Some(Event::Queue { items, position });
            }
            None
        }
        Request::ClearQueue => {
            if daemon.local.borrow().is_active() {
                stop_local(daemon);
                daemon.publish(Event::Queue {
                    items: Vec::new(),
                    position: 0,
                });
                daemon.publish_snapshot();
                return None;
            }
            tracing::info!("clearing the queue");
            daemon.send(Command::ClearQueue);
            // The mirror follows the sidecar's own event as always (rule 3);
            // this is the half MusicKit cannot know about.
            *daemon.last_queue.borrow_mut() = None;
            *daemon.pending_start.borrow_mut() = None;
            vinilo_core::session::clear();
            None
        }
        Request::Write {
            action,
            id,
            playlist_id,
            name,
        } => {
            write(daemon, action, id, playlist_id, name);
            None
        }
        Request::Refresh => {
            if daemon.refreshing.get().is_some() {
                daemon.refresh_again.set(true);
                return None;
            }
            refresh_library(daemon);
            None
        }
        Request::Transport(transport) => {
            route_transport(daemon, transport);
            None
        }
    }
}

fn stop_local(daemon: &Daemon) {
    if !daemon.local.borrow().is_active() {
        return;
    }
    daemon.local.borrow_mut().stop();
    daemon.model.borrow_mut().player.state = PlaybackState::None;
    daemon.model.borrow_mut().art_path = None;
}

fn publish_local(daemon: &Rc<Daemon>) {
    let local = daemon.local.borrow();
    if !local.is_active() {
        return;
    }
    let now = local.now_playing_event();
    let playback = local.playback_event();
    let position = local.position_event();
    let art = local.art_path();
    drop(local);
    {
        let mut model = daemon.model.borrow_mut();
        model.player.apply(&now);
        model.player.apply(&playback);
        model.player.apply(&position);
        model.player.shuffle = daemon.local.borrow().shuffle();
        model.player.repeat = daemon.local.borrow().repeat();
        if art.is_some() {
            model.art_path = art;
        }
    }
    let (items, position) = daemon.model.borrow().queue();
    daemon.publish(Event::Queue { items, position });
    daemon.publish_snapshot();
    if daemon
        .model
        .borrow()
        .player
        .now_playing
        .as_ref()
        .and_then(|item| item.artwork_template.as_ref())
        .is_some()
    {
        fetch_artwork(daemon);
    }
}

fn play_files(daemon: &Rc<Daemon>, paths: Vec<String>, index: usize) -> Option<Event> {
    let paths: Vec<std::path::PathBuf> = paths.into_iter().map(std::path::PathBuf::from).collect();
    if daemon.sidecar.borrow().is_some() {
        daemon.send(Command::Pause);
    }
    let volume = daemon.model.borrow().volume;
    daemon.local.borrow_mut().set_volume(volume);
    match daemon.local.borrow_mut().play_paths(paths, index) {
        Ok(()) => {
            tracing::info!("playing files from this computer");
            *daemon.art_for.borrow_mut() = None;
            publish_local(daemon);
            None
        }
        Err(detail) => Some(Event::Error { detail }),
    }
}

fn route_local_transport(daemon: &Rc<Daemon>, transport: Transport) {
    match transport {
        Transport::Play => daemon.local.borrow_mut().play(),
        Transport::Pause => daemon.local.borrow_mut().pause(),
        Transport::PlayPause => daemon.local.borrow_mut().play_pause(),
        Transport::Next => {
            let _ = daemon.local.borrow_mut().next();
        }
        Transport::Previous => {
            let _ = daemon.local.borrow_mut().previous();
        }
        Transport::Seek { position_ms } => {
            daemon.local.borrow_mut().seek(position_ms);
            daemon.model.borrow_mut().player.seeked_to(position_ms);
        }
        Transport::SetVolume { volume } => {
            let volume = volume.clamp(0.0, 1.0);
            // Rodio's gain, not the Pulse stream named Vinilo — that stream is
            // still the paused sidecar, so driving it leaves the file loud.
            daemon.local.borrow_mut().set_volume(volume);
            daemon.model.borrow_mut().volume = volume;
        }
        Transport::SetShuffle { shuffle } => {
            daemon.local.borrow_mut().set_shuffle(shuffle);
            daemon.model.borrow_mut().player.shuffle = shuffle;
        }
        Transport::SetRepeat { mode } => {
            daemon.local.borrow_mut().set_repeat(mode);
            daemon.model.borrow_mut().player.repeat = mode;
        }
    }
    if daemon.local.borrow().is_active() {
        publish_local(daemon);
    } else {
        daemon.publish_snapshot();
    }
}

pub(crate) fn route_transport(daemon: &Rc<Daemon>, transport: Transport) {
    // Spotify / YouTube Music / Tidal play through the local decoder, so
    // shuffle and repeat have to land here even before the first file exists.
    if daemon.local.borrow().is_active() || daemon.source.is_catalog() {
        route_local_transport(daemon, transport);
        return;
    }
    if matches!(
        transport,
        Transport::Next | Transport::Previous | Transport::Seek { .. }
    ) {
        daemon.restart_at.borrow_mut().take();
    }
    // A restored queue has no current item, so MusicKit's `play` does nothing
    // until the current queue entry has been opened.
    if matches!(transport, Transport::Play | Transport::PlayPause) && needs_opening(daemon) {
        let index = daemon.model.borrow().player.queue_position;
        tracing::info!(index, "nothing is open yet — opening the queue first");
        daemon.send(Command::ChangeToIndex { index });
    }
    if let Some(command) = command_for(transport) {
        tracing::debug!(cmd = command.name(), "transport");
        daemon.send(command);
    }
    match transport {
        Transport::SetVolume { volume } => {
            let volume = volume.clamp(0.0, 1.0);
            if let Some(mixer) = &daemon.mixer {
                mixer.set(volume);
            }
            // Keep the requested value while no stream exists, and publish it
            // at once so paused clients can step from the new value.
            daemon.model.borrow_mut().volume = volume;
            daemon.publish_snapshot();
        }
        // Adopt a seek before MusicKit confirms it so the next position tick
        // cannot pull a client's slider back to the old position.
        Transport::Seek { position_ms } => {
            daemon.model.borrow_mut().player.seeked_to(position_ms);
            daemon.publish_snapshot();
        }
        _ => {}
    }
}

/// Fetch the current track's cover, if we do not already have it.
///
/// **At most once per template.** A snapshot goes out twice a second and the
/// cover changes at most once a track; refetching on every one would be a
/// request per tick for a file already on disk.
fn fetch_artwork(daemon: &Rc<Daemon>) {
    let template = daemon
        .model
        .borrow()
        .player
        .now_playing
        .as_ref()
        .and_then(|item| item.artwork_template.clone());

    if template == *daemon.art_for.borrow() {
        return;
    }
    *daemon.art_for.borrow_mut() = template.clone();

    let Some(template) = template else {
        daemon.model.borrow_mut().art_path = None;
        return;
    };

    let art = Artwork::new(template);
    // Already on disk from an earlier play, or from the GTK client — the cache
    // is shared, which is the point of it living in core.
    if let Some(path) = artwork::cache_path(&art, artwork::ART_SIZE)
        && path.is_file()
    {
        daemon.model.borrow_mut().art_path = Some(path);
        return;
    }

    // Cleared first: a stale cover under a new track is worse than none, and
    // this is a network round trip.
    daemon.model.borrow_mut().art_path = None;
    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        match artwork::fetch(art, artwork::ART_SIZE).await {
            Ok(path) => {
                daemon.model.borrow_mut().art_path = Some(path);
                daemon.publish_snapshot();
            }
            // Cosmetic: a missing cover is not worth telling anyone about.
            Err(err) => tracing::warn!(?err, "artwork not fetched"),
        }
    });
}

/// Change what the signed-in catalogue holds for this account.
///
/// Apple Music still splits: adding and starring go over REST, removing goes
/// through MusicKit. Spotify / YouTube Music / Tidal stay in this process.
fn write(
    daemon: &Rc<Daemon>,
    action: WriteAction,
    id: String,
    playlist_id: Option<String>,
    name: Option<String>,
) {
    let provider = daemon.source;
    if provider.is_catalog() {
        write_catalog(daemon, provider, action, id, playlist_id, name);
        return;
    }

    match action {
        WriteAction::RemoveFromLibrary => {
            daemon.send(Command::RemoveFromLibrary { id });
            return;
        }
        WriteAction::Unfavorite => {
            daemon.send(Command::Unfavorite { id });
            return;
        }
        WriteAction::CreatePlaylist
        | WriteAction::AddToPlaylist
        | WriteAction::RemoveFromPlaylist => {
            write_apple_playlist(daemon, action, id, playlist_id, name);
            return;
        }
        _ => {}
    }

    let Some(client) = daemon.client() else {
        daemon.publish(Event::Error {
            detail: "Not signed in yet".into(),
        });
        return;
    };
    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        let result = match action {
            WriteAction::Favorite => client.add_to_favorites("songs", &id).await,
            WriteAction::AddToLibrary => client.add_to_library("songs", &id).await,
            _ => unreachable!("handled above"),
        };
        match result {
            Ok(()) => {
                tracing::info!(?action, %id, "library write sent");
                request_library_refresh(&daemon);
            }
            Err(err) => {
                tracing::warn!(?err, ?action, %id, "library write failed");
                daemon.publish(Event::Error {
                    detail: format!("{err}"),
                });
            }
        }
    });
}

fn write_apple_playlist(
    daemon: &Rc<Daemon>,
    action: WriteAction,
    id: String,
    playlist_id: Option<String>,
    name: Option<String>,
) {
    let Some(client) = daemon.client() else {
        daemon.publish(Event::Error {
            detail: "Not signed in yet".into(),
        });
        return;
    };
    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        let result = match action {
            WriteAction::CreatePlaylist => {
                let title = name.as_deref().unwrap_or("New playlist");
                let seed = if id.is_empty() {
                    None
                } else {
                    Some(id.as_str())
                };
                client.create_playlist(title, seed).await.map(Some)
            }
            WriteAction::AddToPlaylist => {
                let Some(list) = playlist_id.as_deref() else {
                    daemon.publish(Event::Error {
                        detail: "missing playlist id".into(),
                    });
                    return;
                };
                client.add_to_playlist(list, &id).await.map(|()| None)
            }
            WriteAction::RemoveFromPlaylist => Err(anyhow::anyhow!(
                "Removing a song from an Apple Music playlist is not wired yet"
            )),
            _ => unreachable!(),
        };
        match result {
            Ok(created) => {
                tracing::info!(?action, created = ?created, "playlist write sent");
                if let Some(list_id) = created {
                    insert_created_playlist(&daemon, list_id, name.clone());
                }
                request_library_refresh(&daemon);
            }
            Err(err) => {
                tracing::warn!(?err, ?action, "playlist write failed");
                daemon.publish(Event::Error {
                    detail: format!("{err}"),
                });
            }
        }
    });
}

fn write_catalog(
    daemon: &Rc<Daemon>,
    provider: vinilo_core::provider::Provider,
    action: WriteAction,
    id: String,
    playlist_id: Option<String>,
    name: Option<String>,
) {
    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        let http = vinilo_core::streams::http_long();
        match vinilo_core::catalog_write::apply(
            &http,
            provider,
            action,
            &id,
            playlist_id.as_deref(),
            name.as_deref(),
        )
        .await
        {
            Ok(created) => {
                tracing::info!(?action, %id, created = ?created, "catalogue write sent");
                if let Some(list_id) = created {
                    insert_created_playlist(&daemon, list_id, name);
                }
                request_library_refresh(&daemon);
            }
            Err(err) => {
                tracing::warn!(?err, ?action, %id, "catalogue write failed");
                daemon.publish(Event::Error {
                    detail: format!("{err}"),
                });
            }
        }
    });
}

fn insert_created_playlist(daemon: &Daemon, list_id: String, name: Option<String>) {
    if list_id.is_empty() {
        return;
    }
    let title = name.unwrap_or_else(|| "New playlist".into());
    let list = vinilo_core::music::types::Playlist {
        id: list_id,
        date_added: String::new(),
        last_modified: String::new(),
        name: title,
        curator: String::new(),
        description: String::new(),
        artwork: None,
        library: true,
    };
    daemon.model.borrow_mut().library.playlists.insert(0, list);
    let model = daemon.model.borrow();
    vinilo_core::library_cache::save(
        &model.library.tracks,
        &model.library.albums,
        &model.library.artists,
        &model.library.playlists,
    );
    drop(model);
    daemon.publish(Event::LibraryChanged);
}

/// Refresh now, or remember to do it when the in-flight fetch finishes.
///
/// A write during a fetch used to be a no-op, so a new playlist waited until
/// the next launch. `refresh_again` is only for a connect-time Refresh that
/// can be skipped after a successful load; writes always need another pass.
fn request_library_refresh(daemon: &Rc<Daemon>) {
    if daemon.refreshing.get().is_some() {
        daemon.refresh_dirty.set(true);
        return;
    }
    refresh_library(daemon);
}

/// Re-read the library from Apple and tell clients it moved.
///
/// Spawned, and at most one at a time: a client hammering a reload button must
/// not queue up four full library fetches behind it.
fn refresh_library(daemon: &Rc<Daemon>) {
    let provider = daemon.source;
    if provider.is_catalog() {
        refresh_catalog_library(daemon, provider);
        return;
    }
    let Some(generation) = begin_library_refresh(daemon) else {
        tracing::debug!("library refresh already running");
        return;
    };
    let Some(client) = daemon.client() else {
        finish_and_maybe_chain(daemon, generation, true);
        return;
    };

    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        const MAX: usize = 10_000;
        let fetched = tokio::join!(
            client.all_library_songs(MAX),
            client.all_library_albums(MAX),
            client.all_library_artists(MAX),
            client.all_library_playlists(MAX),
        );
        if daemon.authorization_generation.get() != generation {
            finish_and_maybe_chain(&daemon, generation, false);
            tracing::debug!(generation, "discarded stale library refresh");
            return;
        }

        match fetched {
            (Ok(songs), Ok(albums), Ok(artists), Ok(playlists)) => {
                tracing::info!(
                    songs = songs.len(),
                    albums = albums.len(),
                    artists = artists.len(),
                    playlists = playlists.len(),
                    "library refreshed"
                );
                vinilo_core::library_cache::save(&songs, &albums, &artists, &playlists);
                let mut model = daemon.model.borrow_mut();
                model.library.tracks = songs;
                model.library.albums = albums;
                model.library.artists = artists;
                model.library.playlists = playlists;
                drop(model);
                daemon.publish(Event::LibraryChanged);
                finish_and_maybe_chain(&daemon, generation, true);
            }
            _ => {
                tracing::warn!("library refresh failed; keeping what was cached");
                finish_and_maybe_chain(&daemon, generation, false);
            }
        }
    });
}

fn begin_library_refresh(daemon: &Daemon) -> Option<u64> {
    let generation = daemon.authorization_generation.get();
    if daemon.refreshing.get() == Some(generation) {
        return None;
    }
    daemon.refreshing.set(Some(generation));
    daemon.publish(Event::LibraryRefreshing { refreshing: true });
    Some(generation)
}

fn finish_library_refresh(daemon: &Daemon, generation: u64) -> bool {
    let current = daemon.authorization_generation.get() == generation;
    if daemon.refreshing.get() == Some(generation) {
        daemon.refreshing.set(None);
        if current {
            daemon.publish(Event::LibraryRefreshing { refreshing: false });
        }
    }
    current
}

/// A window connecting at boot always sends Refresh. If that landed while a
/// fetch was already running, run it now — but only when the one that just
/// finished did not already put a library on disk. A second full fetch after
/// a success is how Pathfinder starts returning 429s.
fn finish_and_maybe_chain(daemon: &Rc<Daemon>, generation: u64, applied: bool) {
    finish_library_refresh(daemon, generation);
    let dirty = daemon.refresh_dirty.replace(false);
    let again = daemon.refresh_again.replace(false);
    if daemon.refreshing.get().is_some() {
        return;
    }
    if dirty {
        tracing::info!("refreshing the library after a write that landed during a fetch");
        refresh_library(daemon);
        return;
    }
    if !again || applied {
        return;
    }
    tracing::info!("running the library refresh that arrived while one was already in flight");
    refresh_library(daemon);
}

fn refresh_catalog_library(daemon: &Rc<Daemon>, provider: vinilo_core::provider::Provider) {
    let Some(generation) = begin_library_refresh(daemon) else {
        tracing::debug!("library refresh already running");
        return;
    };
    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        match provider {
            vinilo_core::provider::Provider::Spotify => {
                refresh_spotify_library(&daemon, generation).await;
            }
            vinilo_core::provider::Provider::YoutubeMusic => {
                let http = vinilo_core::streams::http_long();
                refresh_named_catalog(
                    &daemon,
                    generation,
                    "youtube music",
                    vinilo_core::ytmusic::library(&http),
                )
                .await;
            }
            vinilo_core::provider::Provider::Tidal => {
                let http = vinilo_core::streams::http_long();
                refresh_named_catalog(
                    &daemon,
                    generation,
                    "tidal",
                    vinilo_core::tidal::library(&http),
                )
                .await;
            }
            _ => {
                finish_and_maybe_chain(&daemon, generation, true);
            }
        }
    });
}

fn library_is_empty(library: &vinilo_core::spotify::Library) -> bool {
    library.songs.is_empty()
        && library.albums.is_empty()
        && library.artists.is_empty()
        && library.playlists.is_empty()
}

fn cache_library_is_empty(library: &vinilo_core::library_cache::Library) -> bool {
    library.is_empty()
}

async fn refresh_spotify_library(daemon: &Rc<Daemon>, generation: u64) {
    let http = vinilo_core::streams::http_long();
    match tokio::time::timeout(
        std::time::Duration::from_secs(20),
        vinilo_core::spotify::warm_session(&http),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            tracing::warn!(?err, "spotify session warm-up failed");
            let detail = format!("{err}");
            if should_surface_spotify_error(&detail) {
                if daemon.model.borrow().library.is_empty()
                    && vinilo_core::library_cache::load().is_empty()
                {
                    daemon.publish(Event::Error { detail });
                }
                finish_and_maybe_chain(daemon, generation, false);
                return;
            }
        }
        Err(_) => {
            tracing::warn!("spotify session warm-up timed out");
        }
    }

    const BACKOFF_SECS: [u64; 3] = [0, 4, 12];
    // Per-request HTTP already waits 25s. The old 25s cap around the *whole*
    // library cancelled pagination mid-flight and left the cache empty.
    const LIBRARY_DEADLINE: std::time::Duration = std::time::Duration::from_secs(180);
    for (attempt, wait) in BACKOFF_SECS.iter().enumerate() {
        if daemon.authorization_generation.get() != generation {
            finish_and_maybe_chain(daemon, generation, false);
            return;
        }
        if let Some(cool) = vinilo_core::spotify::cooldown_left() {
            let wait_for = cool.min(std::time::Duration::from_secs(90));
            tracing::info!(
                secs = wait_for.as_secs(),
                "waiting out Spotify rate limit before a library fetch"
            );
            tokio::time::sleep(wait_for).await;
        } else if *wait > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(*wait)).await;
        }
        if daemon.authorization_generation.get() != generation {
            finish_and_maybe_chain(daemon, generation, false);
            return;
        }
        if vinilo_core::spotify::cooling_down() {
            tracing::warn!(
                attempt = attempt + 1,
                "spotify still rate-limited; not spending a library fetch"
            );
            continue;
        }
        let daemon_for_progress = daemon.clone();
        let fetch = vinilo_core::spotify::library_progress(
            &http,
            move |songs, albums, artists, playlists| {
                apply_partial_catalog_library(
                    &daemon_for_progress,
                    generation,
                    songs,
                    albums,
                    artists,
                    playlists,
                );
            },
        );
        match tokio::time::timeout(LIBRARY_DEADLINE, fetch).await {
            Ok(Ok(library)) if !library_is_empty(&library) => {
                apply_catalog_library(
                    daemon,
                    generation,
                    library.songs,
                    library.albums,
                    library.artists,
                    library.playlists,
                );
                return;
            }
            Ok(Ok(_)) => {
                tracing::warn!(
                    attempt = attempt + 1,
                    "spotify library fetch was empty — retrying"
                );
            }
            Ok(Err(err)) => {
                tracing::warn!(
                    ?err,
                    attempt = attempt + 1,
                    "spotify library refresh failed"
                );
                let detail = format!("{err}");
                if should_surface_spotify_error(&detail) {
                    if daemon.model.borrow().library.is_empty()
                        && vinilo_core::library_cache::load().is_empty()
                    {
                        daemon.publish(Event::Error { detail });
                    }
                    finish_and_maybe_chain(daemon, generation, false);
                    return;
                }
            }
            Err(_) => {
                tracing::warn!(
                    attempt = attempt + 1,
                    "spotify library refresh timed out — retrying"
                );
            }
        }
        if !vinilo_core::library_cache::load().is_empty() {
            tracing::info!("keeping the cached Spotify library after this attempt");
            finish_and_maybe_chain(daemon, generation, true);
            return;
        }
        if attempt == 0 {
            // Keep retrying off the spinner: an empty first fetch used to hold
            // the window on "loading" until every backoff finished.
            daemon.publish(Event::LibraryRefreshing { refreshing: false });
        }
    }
    tracing::warn!("spotify library still empty after retries — keeping what was cached");
    finish_and_maybe_chain(daemon, generation, false);
}

async fn refresh_named_catalog<Fut>(
    daemon: &Rc<Daemon>,
    generation: u64,
    label: &'static str,
    fetch: Fut,
) where
    Fut: std::future::Future<Output = anyhow::Result<vinilo_core::library_cache::Library>>,
{
    match tokio::time::timeout(std::time::Duration::from_secs(60), fetch).await {
        Ok(Ok(library)) if !cache_library_is_empty(&library) => {
            apply_catalog_library(
                daemon,
                generation,
                library.songs,
                library.albums,
                library.artists,
                library.playlists,
            );
        }
        Ok(Ok(_)) => {
            tracing::warn!("{label} library fetch was empty — keeping what was cached");
            finish_and_maybe_chain(daemon, generation, false);
        }
        Ok(Err(err)) => {
            tracing::warn!(?err, "{label} library refresh failed");
            finish_and_maybe_chain(daemon, generation, false);
        }
        Err(_) => {
            tracing::warn!("{label} library refresh timed out — keeping what was cached");
            finish_and_maybe_chain(daemon, generation, false);
        }
    }
}

fn apply_partial_catalog_library(
    daemon: &Daemon,
    generation: u64,
    songs: &[vinilo_core::music::types::Track],
    albums: &[vinilo_core::music::types::Album],
    artists: &[vinilo_core::music::types::Artist],
    playlists: &[vinilo_core::music::types::Playlist],
) {
    if daemon.authorization_generation.get() != generation {
        return;
    }
    if songs.is_empty() && albums.is_empty() && artists.is_empty() && playlists.is_empty() {
        return;
    }
    // A good cache already painted the window. Do not replace it with
    // playlists-only while the rest of the fetch is still in flight.
    if !vinilo_core::library_cache::load().is_empty() {
        return;
    }
    remember_songs(daemon, songs);
    vinilo_core::library_cache::save(songs, albums, artists, playlists);
    let mut model = daemon.model.borrow_mut();
    if !songs.is_empty() {
        model.library.tracks = songs.to_vec();
    }
    if !albums.is_empty() {
        model.library.albums = albums.to_vec();
    }
    if !artists.is_empty() {
        model.library.artists = artists.to_vec();
    }
    if !playlists.is_empty() {
        model.library.playlists = playlists.to_vec();
    }
    drop(model);
    daemon.publish(Event::LibraryChanged);
}

fn apply_catalog_library(
    daemon: &Rc<Daemon>,
    generation: u64,
    songs: Vec<vinilo_core::music::types::Track>,
    albums: Vec<vinilo_core::music::types::Album>,
    artists: Vec<vinilo_core::music::types::Artist>,
    playlists: Vec<vinilo_core::music::types::Playlist>,
) {
    if daemon.authorization_generation.get() != generation {
        finish_and_maybe_chain(daemon, generation, false);
        return;
    }
    tracing::info!(
        songs = songs.len(),
        albums = albums.len(),
        artists = artists.len(),
        playlists = playlists.len(),
        "catalogue library refreshed"
    );
    remember_songs(daemon, &songs);
    vinilo_core::library_cache::save(&songs, &albums, &artists, &playlists);
    let mut model = daemon.model.borrow_mut();
    model.library.tracks = songs;
    model.library.albums = albums;
    model.library.artists = artists;
    model.library.playlists = playlists;
    drop(model);
    daemon.publish(Event::LibraryChanged);
    finish_and_maybe_chain(daemon, generation, true);
}

fn should_surface_spotify_error(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    lower.contains("sign in") || lower.contains("iniciar sesión")
}

fn discover_spotify(daemon: &Rc<Daemon>) {
    let cached = vinilo_core::discover::load();
    if !cached.is_empty() {
        daemon.publish(Event::Discover(cached));
    }
    let homemade = {
        let library = &daemon.model.borrow().library;
        vinilo_core::discover::homemade(
            &library.tracks,
            &library.albums,
            &library.playlists,
            &vinilo_core::listen_history::load(),
        )
    };
    if vinilo_core::spotify::cooling_down() {
        vinilo_core::discover::save(&homemade);
        daemon.publish(Event::Discover(homemade));
        return;
    }
    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        let http = vinilo_core::streams::http();
        let snapshot = {
            let library = &daemon.model.borrow().library;
            vinilo_core::spotify::Library {
                songs: library.tracks.clone(),
                albums: library.albums.clone(),
                artists: library.artists.clone(),
                playlists: library.playlists.clone(),
            }
        };
        let fetch = vinilo_core::spotify::discover(&http, &snapshot);
        match tokio::time::timeout(std::time::Duration::from_secs(20), fetch).await {
            Ok(Ok(mut page)) => {
                page.fill_gaps(homemade);
                remember_songs(
                    &daemon,
                    &page
                        .recommended_songs
                        .iter()
                        .cloned()
                        .chain(page.recently_played.iter().filter_map(|e| match e {
                            Entry::Song(t) => Some(t.clone()),
                            _ => None,
                        }))
                        .collect::<Vec<_>>(),
                );
                tracing::info!(
                    recently_played = page.recently_played.len(),
                    made_for_you = page.recommended_playlists.len(),
                    recommended_songs = page.recommended_songs.len(),
                    recently_added = page.recently_added.len(),
                    charts = page.charts.len(),
                    "spotify listen now shelves"
                );
                vinilo_core::discover::save(&page);
                daemon.publish(Event::Discover(page));
            }
            Ok(Err(err)) => {
                tracing::warn!(?err, "spotify discover failed");
                vinilo_core::discover::save(&homemade);
                daemon.publish(Event::Discover(homemade));
            }
            Err(_) => {
                tracing::warn!("spotify discover timed out — keeping homemade shelves");
                vinilo_core::discover::save(&homemade);
                daemon.publish(Event::Discover(homemade));
            }
        }
    });
}

fn open_spotify_page(daemon: &Rc<Daemon>, kind: PageKind, id: String) {
    if let Some(cached) = vinilo_core::page_cache::load(kind, &id) {
        remember_songs(
            daemon,
            &cached
                .entries
                .iter()
                .filter_map(|e| match e {
                    Entry::Song(t) => Some(t.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
        );
        daemon.publish(Event::Page {
            kind,
            id: id.clone(),
            header: cached.header,
            entries: cached.entries,
        });
    }
    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        let http = vinilo_core::streams::http();
        match vinilo_core::spotify::open(&http, kind, &id).await {
            Ok((header, entries)) => {
                let songs: Vec<_> = entries
                    .iter()
                    .filter_map(|e| match e {
                        Entry::Song(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect();
                remember_songs(&daemon, &songs);
                let unchanged = vinilo_core::page_cache::load(kind, &id).is_some_and(|cached| {
                    vinilo_core::page_cache::same(&cached, &header, &entries)
                });
                vinilo_core::page_cache::save(kind, &id, &header, &entries);
                if !unchanged {
                    daemon.publish(Event::Page {
                        kind,
                        id,
                        header,
                        entries,
                    });
                }
            }
            Err(err) => {
                tracing::warn!(?err, %id, "opening a Spotify page failed");
                if vinilo_core::page_cache::load(kind, &id).is_none() {
                    daemon.publish(Event::Error {
                        detail: format!("{err}"),
                    });
                }
            }
        }
    });
}

/// Tracks as rows.
fn songs(tracks: Vec<vinilo_core::music::types::Track>) -> Vec<Entry> {
    tracks.into_iter().map(Entry::Song).collect()
}

/// Search the catalog and announce what came back.
///
/// Spawned like `open_page`, and for the same reason: this is a network round
/// trip, and a client waiting on one must not stop the daemon answering anyone
/// else. The answer carries the query it belongs to, because somebody types
/// faster than Apple replies.
fn search(daemon: &Rc<Daemon>, query: String, filter: CatalogFilter, offset: usize) {
    let provider = daemon.source;
    if provider.is_catalog() {
        search_catalog(daemon, provider, query, filter, offset);
        return;
    }
    let Some(client) = daemon.client() else {
        daemon.publish(Event::Error {
            detail: "Not signed in yet".into(),
        });
        return;
    };

    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        let found = client
            .search(&query, filter.types(), catalog::CATALOG_LIMIT, offset)
            .await;
        match found {
            Ok(results) => {
                let (entries, paged) = catalog::catalog_rows(filter, results, offset == 0);
                tracing::info!(%query, ?filter, offset, rows = entries.len(), "searched");
                daemon.publish(Event::Results {
                    query,
                    entries,
                    offset,
                    more: paged >= catalog::CATALOG_LIMIT as usize,
                });
            }
            Err(err) => {
                tracing::warn!(?err, %query, "search failed");
                daemon.publish(Event::Error {
                    detail: format!("{err}"),
                });
            }
        }
    });
}

fn search_catalog(
    daemon: &Rc<Daemon>,
    provider: vinilo_core::provider::Provider,
    query: String,
    filter: CatalogFilter,
    offset: usize,
) {
    if offset > 0 {
        daemon.publish(Event::Results {
            query,
            entries: Vec::new(),
            offset,
            more: false,
        });
        return;
    }
    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        let (hits, entries) = match provider {
            vinilo_core::provider::Provider::YoutubeMusic => {
                let q = query.clone();
                match tokio::task::spawn_blocking(move || crate::ytdlp::search(&q)).await {
                    Ok(Ok(hits)) => {
                        let entries: Vec<Entry> = hits
                            .iter()
                            .cloned()
                            .map(vinilo_core::streams::StreamHit::into_entry)
                            .collect();
                        (hits, entries)
                    }
                    Ok(Err(detail)) => {
                        daemon.publish(Event::Error { detail });
                        return;
                    }
                    Err(err) => {
                        daemon.publish(Event::Error {
                            detail: format!("{err}"),
                        });
                        return;
                    }
                }
            }
            vinilo_core::provider::Provider::Spotify => {
                let http = vinilo_core::streams::http();
                let fetch = vinilo_core::spotify::search(&http, &query);
                match tokio::time::timeout(std::time::Duration::from_secs(25), fetch).await {
                    Ok(Ok(page)) => vinilo_core::spotify::catalog_entries(page, filter),
                    Ok(Err(err)) => {
                        tracing::warn!(?err, "spotify search failed");
                        daemon.publish(Event::Error {
                            detail: format!("{err}"),
                        });
                        daemon.publish(Event::Results {
                            query,
                            entries: Vec::new(),
                            offset: 0,
                            more: false,
                        });
                        return;
                    }
                    Err(_) => {
                        tracing::warn!("spotify search timed out");
                        daemon.publish(Event::Error {
                            detail: vinilo_core::i18n::t(vinilo_core::i18n::Key::SpotifyTimeout)
                                .into(),
                        });
                        daemon.publish(Event::Results {
                            query,
                            entries: Vec::new(),
                            offset: 0,
                            more: false,
                        });
                        return;
                    }
                }
            }
            vinilo_core::provider::Provider::Tidal => {
                let http = vinilo_core::streams::http();
                match vinilo_core::streams::search_tidal(&http, &query).await {
                    Ok(hits) => {
                        let entries: Vec<Entry> = hits
                            .iter()
                            .cloned()
                            .map(vinilo_core::streams::StreamHit::into_entry)
                            .collect();
                        (hits, entries)
                    }
                    Err(err) => {
                        tracing::warn!(?err, "tidal search failed");
                        daemon.publish(Event::Error {
                            detail: format!("{err}"),
                        });
                        return;
                    }
                }
            }
            vinilo_core::provider::Provider::AppleMusic
            | vinilo_core::provider::Provider::Local => (Vec::new(), Vec::new()),
        };
        remember_hits(&daemon, hits);
        tracing::info!(%query, rows = entries.len(), ?provider, "searched catalogue");
        daemon.publish(Event::Results {
            query,
            entries,
            offset: 0,
            more: false,
        });
    });
}

fn remember_hits(daemon: &Daemon, hits: Vec<vinilo_core::streams::StreamHit>) {
    if hits.is_empty() {
        return;
    }
    {
        let mut map = daemon.stream_hits.borrow_mut();
        for hit in hits {
            map.insert(hit.id.clone(), hit);
        }
        vinilo_core::streams::save_hits(&map);
    }
}

fn remember_songs(daemon: &Daemon, songs: &[vinilo_core::music::types::Track]) {
    remember_hits(daemon, vinilo_core::spotify::hits_from_songs(songs));
}

fn play_streams(daemon: &Rc<Daemon>, ids: Vec<String>, index: usize, start: Start) {
    let shuffled = start.reorders();
    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        let mut hits = Vec::new();
        for id in &ids {
            if let Some(hit) = resolve_stream_hit(&daemon, id).await {
                hits.push(hit);
            }
        }
        if hits.is_empty() {
            daemon.publish(Event::Error {
                detail: "Nothing here can be streamed".into(),
            });
            return;
        }
        let start_at = if shuffled {
            random_row(hits.len())
        } else {
            index.min(hits.len().saturating_sub(1))
        };
        let first = hits.remove(start_at);
        if shuffled {
            shuffle_hits(&mut hits);
        }
        let rest = hits;
        let Some(dir) = vinilo_core::paths::cache_dir().map(|p| p.join("streams")) else {
            daemon.publish(Event::Error {
                detail: "No cache directory".into(),
            });
            return;
        };
        let dir_first = dir.clone();
        let download = first.clone();
        let path = match tokio::task::spawn_blocking(move || {
            crate::ytdlp::download(&download, &dir_first)
        })
        .await
        {
            Ok(Ok(path)) => path,
            Ok(Err(detail)) => {
                daemon.publish(Event::Error { detail });
                return;
            }
            Err(err) => {
                daemon.publish(Event::Error {
                    detail: format!("{err}"),
                });
                return;
            }
        };
        if daemon.sidecar.borrow().is_some() {
            daemon.send(Command::Pause);
        }
        *daemon.art_for.borrow_mut() = None;
        let volume = daemon.model.borrow().volume;
        let repeat = daemon.model.borrow().player.repeat;
        {
            let mut local = daemon.local.borrow_mut();
            local.set_volume(volume);
            local.set_repeat(repeat);
            local.set_shuffle(shuffled);
        }
        daemon.model.borrow_mut().player.shuffle = shuffled;
        if let Err(detail) = daemon.local.borrow_mut().play_hits(vec![(path, first)], 0) {
            daemon.publish(Event::Error { detail });
            return;
        }
        publish_local(&daemon);
        for hit in rest {
            let dir = dir.clone();
            let download = hit.clone();
            match tokio::task::spawn_blocking(move || crate::ytdlp::download(&download, &dir)).await
            {
                Ok(Ok(path)) => daemon.local.borrow_mut().append_hit(path, hit),
                Ok(Err(err)) => tracing::warn!(%err, "skipping a catalogue track"),
                Err(err) => tracing::warn!(?err, "skipping a catalogue track"),
            }
        }
        publish_local(&daemon);
    });
}

fn enqueue_streams(daemon: &Rc<Daemon>, ids: Vec<String>) {
    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        let Some(dir) = vinilo_core::paths::cache_dir().map(|p| p.join("streams")) else {
            return;
        };
        for id in ids {
            let Some(hit) = resolve_stream_hit(&daemon, &id).await else {
                continue;
            };
            let dir = dir.clone();
            let download = hit.clone();
            match tokio::task::spawn_blocking(move || crate::ytdlp::download(&download, &dir)).await
            {
                Ok(Ok(path)) => daemon.local.borrow_mut().append_hit(path, hit),
                Ok(Err(err)) => tracing::warn!(%err, "could not add catalogue track"),
                Err(err) => tracing::warn!(?err, "could not add catalogue track"),
            }
        }
        publish_local(&daemon);
    });
}

async fn resolve_stream_hit(daemon: &Daemon, id: &str) -> Option<vinilo_core::streams::StreamHit> {
    if let Some(hit) = daemon.stream_hits.borrow().get(id).cloned()
        && !hit.title_is_placeholder()
    {
        return Some(hit);
    }
    if let Some(vinilo_core::spotify::Ref::Track(sp)) = vinilo_core::spotify::Ref::parse(id) {
        let http = vinilo_core::streams::http();
        if let Ok(hit) = vinilo_core::spotify::track_hit(&http, &sp).await {
            remember_hits(daemon, vec![hit.clone()]);
            return Some(hit);
        }
    }
    stream_hit(daemon, id)
}

fn stream_hit(daemon: &Daemon, id: &str) -> Option<vinilo_core::streams::StreamHit> {
    if let Some(hit) = daemon.stream_hits.borrow().get(id).cloned() {
        return Some(hit);
    }
    // After a daemon restart the search cache is gone. Reconstruct a
    // playable target from the id so a click still reaches yt-dlp.
    if let Some(video) = id.strip_prefix("yt:") {
        return Some(vinilo_core::streams::StreamHit {
            id: id.to_owned(),
            title: video.to_owned(),
            artist: String::new(),
            album: String::new(),
            duration_ms: 0,
            artwork: None,
            play_query: format!("https://www.youtube.com/watch?v={video}"),
        });
    }
    if let Some(vinilo_core::spotify::Ref::Track(track)) = vinilo_core::spotify::Ref::parse(id) {
        return Some(vinilo_core::streams::StreamHit {
            id: id.to_owned(),
            title: track.clone(),
            artist: String::new(),
            album: String::new(),
            duration_ms: 0,
            artwork: None,
            play_query: format!("https://open.spotify.com/track/{track}"),
        });
    }
    if let Some(track) = id.strip_prefix("td:") {
        return Some(vinilo_core::streams::StreamHit {
            id: id.to_owned(),
            title: track.to_owned(),
            artist: String::new(),
            album: String::new(),
            duration_ms: 0,
            artwork: None,
            play_query: format!("https://tidal.com/browse/track/{track}"),
        });
    }
    None
}

/// Fetch an album, artist or playlist and announce it.
///
/// Spawned rather than awaited: this is a network round trip, and a client
/// waiting on one must not stop the daemon answering anybody else. The answer
/// arrives as an [`Event::Page`] on every subscriber, which is also what lets a
/// second client show a page the first one opened.
fn open_page(daemon: &Rc<Daemon>, kind: PageKind, id: String) {
    let provider = daemon.source;
    if provider == vinilo_core::provider::Provider::Spotify
        && vinilo_core::spotify::Ref::parse(&id).is_some()
    {
        open_spotify_page(daemon, kind, id);
        return;
    }
    if let Some(cached) = vinilo_core::page_cache::load(kind, &id) {
        daemon.publish(Event::Page {
            kind,
            id: id.clone(),
            header: cached.header,
            entries: cached.entries,
        });
    }

    let Some(client) = daemon.client() else {
        if vinilo_core::page_cache::load(kind, &id).is_none() {
            daemon.publish(Event::Error {
                detail: "Not signed in yet".into(),
            });
        }
        return;
    };

    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        // **An artist page is their albums, not their tracks**, which is why
        // this is not one call with a different id. Catalog and library are
        // separate for the other reason: the two id spaces are not
        // interchangeable, and a catalog id 404s against `/me/library`.
        let fetched = match kind {
            PageKind::Album => client
                .album(&id)
                .await
                .map(|(album, tracks)| (Entry::Album(album), songs(tracks))),
            PageKind::LibraryAlbum => client
                .library_album(&id)
                .await
                .map(|(album, tracks)| (Entry::Album(album), songs(tracks))),
            PageKind::Playlist => client
                .playlist(&id)
                .await
                .map(|(list, tracks)| (Entry::Playlist(list), songs(tracks))),
            PageKind::LibraryPlaylist => client
                .library_playlist(&id)
                .await
                .map(|(list, tracks)| (Entry::Playlist(list), songs(tracks))),
            PageKind::Artist => client.artist_albums(&id).await.map(|(artist, albums)| {
                (
                    Entry::Artist(artist),
                    albums.into_iter().map(Entry::Album).collect(),
                )
            }),
            PageKind::LibraryArtist => {
                client
                    .library_artist_albums(&id)
                    .await
                    .map(|(artist, albums)| {
                        (
                            Entry::Artist(artist),
                            albums.into_iter().map(Entry::Album).collect(),
                        )
                    })
            }
        };

        match fetched {
            Ok((header, entries)) => {
                let unchanged = vinilo_core::page_cache::load(kind, &id).is_some_and(|cached| {
                    vinilo_core::page_cache::same(&cached, &header, &entries)
                });
                vinilo_core::page_cache::save(kind, &id, &header, &entries);
                if !unchanged {
                    daemon.publish(Event::Page {
                        kind,
                        id,
                        header,
                        entries,
                    });
                }
            }
            Err(err) => {
                tracing::warn!(?err, %id, "opening a page failed");
                if vinilo_core::page_cache::load(kind, &id).is_none() {
                    daemon.publish(Event::Error {
                        detail: format!("{err}"),
                    });
                }
            }
        }
    });
}

fn discover(daemon: &Rc<Daemon>) {
    let provider = daemon.source;
    if provider == vinilo_core::provider::Provider::Spotify {
        discover_spotify(daemon);
        return;
    }

    let cached = vinilo_core::discover::load();
    if !cached.is_empty() {
        daemon.publish(Event::Discover(cached));
    }

    let homemade = {
        let library = &daemon.model.borrow().library;
        vinilo_core::discover::homemade(
            &library.tracks,
            &library.albums,
            &library.playlists,
            &vinilo_core::listen_history::load(),
        )
    };

    if !provider.needs_apple() {
        daemon.publish(Event::Discover(homemade));
        return;
    }

    let Some(client) = daemon.client() else {
        daemon.publish(Event::Discover(homemade));
        return;
    };

    let daemon = daemon.clone();
    tokio::task::spawn_local(async move {
        let mut page = client.discover().await;
        page.fill_gaps(homemade);
        tracing::info!(
            recently_played = page.recently_played.len(),
            made_for_you = page.recommended_playlists.len(),
            recommended_songs = page.recommended_songs.len(),
            recently_added = page.recently_added.len(),
            charts = page.charts.len(),
            "listen now shelves"
        );
        vinilo_core::discover::save(&page);
        daemon.publish(Event::Discover(page));
    });
}

/// Build a queue from ids a client drew, and start it.
///
/// The arithmetic is `vinilo_core::queue`'s, the same the GTK client uses —
/// which is the point of it living there. Two implementations would be two
/// answers to "which song did they click".
fn play(daemon: &Daemon, ids: &[String], index: usize, start: Start) {
    let visible: Vec<Option<String>> = ids.iter().cloned().map(Some).collect();
    let row = if start.reorders() {
        // Shuffle: the entry point is the random part (#147).
        random_row(ids.len())
    } else {
        index
    };

    let dead = daemon.model.borrow().dead_ids.clone();
    let (songs, start_id) = queue_from_ids(&visible, row, &dead);
    if songs.is_empty() {
        daemon.publish(Event::Error {
            detail: "Nothing here can be streamed".into(),
        });
        return;
    }

    let position = start_index(&songs, start_id.as_ref());
    tracing::info!(queue = songs.len(), position, ?start, "enqueuing");
    // Nothing to verify when the order is not ours: MusicKit reorders as it
    // loads, so no track is the *wrong* one to open on (#152).
    *daemon.pending_start.borrow_mut() = if start.reorders() {
        None
    } else {
        start_id.clone()
    };
    *daemon.last_queue.borrow_mut() = Some((songs.clone(), start_id));
    if let Some(shuffle) = start.mode(true) {
        daemon.send(Command::SetShuffle { shuffle });
    }
    daemon.send(Command::SetQueue {
        songs,
        start_position: position,
        start_playing: true,
        start_time_ms: 0,
    });
}

/// A row to open a shuffle on.
///
/// Not `rand`: one number, once per shuffled play, and a dependency for it
/// would be the largest thing in this binary's tree.
fn random_row(len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    nanos % len
}

fn shuffle_hits(hits: &mut [vinilo_core::streams::StreamHit]) {
    if hits.len() < 2 {
        return;
    }
    for i in (1..hits.len()).rev() {
        let j = random_row(i + 1);
        hits.swap(i, j);
    }
}

/// Put back what was playing when Vinilo last closed.
///
/// **Loaded, not started.** A daemon that makes noise because it was started is
/// worse than an app that does — nobody asked, and it may have been started by
/// a login script.
fn restore_session(daemon: &Daemon) {
    let Some(session) = vinilo_core::session::load() else {
        return;
    };
    if !vinilo_core::session::playable_on_current_source(&session) {
        tracing::info!(
            provider = %session.provider,
            tracks = session.songs.len(),
            "saved session is for another music source — not restoring"
        );
        return;
    }
    let start = session.start.min(session.songs.len().saturating_sub(1));
    tracing::info!(
        tracks = session.songs.len(),
        start,
        position_ms = session.position_ms,
        "restoring the last session"
    );
    // Sequential: the saved order *is* what was playing, and `start` indexes
    // into it (#152).
    daemon.send(Command::SetShuffle { shuffle: false });
    let wanted = session.songs.get(start).cloned();
    *daemon.pending_start.borrow_mut() = wanted.clone();
    *daemon.last_queue.borrow_mut() = Some((session.songs.clone(), wanted));
    daemon.send(Command::SetQueue {
        songs: session.songs,
        start_position: start,
        start_playing: false,
        start_time_ms: session.position_ms,
    });
}

/// The event's kind, for a log line that stays one line.
fn event_name(event: &PlayerEvent) -> &'static str {
    match event {
        PlayerEvent::NowPlaying { .. } => "nowPlaying",
        PlayerEvent::Queue(_) => "queue",
        PlayerEvent::PlaybackState { .. } => "playbackState",
        PlayerEvent::CmdRecv { .. } => "cmdRecv",
        PlayerEvent::CmdDone { .. } => "cmdDone",
        PlayerEvent::Tokens(_) => "tokens",
        PlayerEvent::Error { .. } => "error",
        _ => "other",
    }
}

fn restart_selected_occurrence(daemon: &Daemon) -> Option<Command> {
    let target = daemon.restart_at.borrow().clone()?;
    let model = daemon.model.borrow();
    let arrived = model
        .player
        .now_playing
        .as_ref()
        .is_some_and(|item| item.occurrence_id == target);
    let still_queued = model
        .player
        .queue
        .iter()
        .any(|item| item.occurrence_id == target);
    drop(model);

    if arrived {
        daemon.restart_at.borrow_mut().take();
        return Some(Command::Seek { position_ms: 0 });
    }
    if !still_queued {
        daemon.restart_at.borrow_mut().take();
    }
    None
}

/// What the sidecar is told, if anything.
///
/// **Volume is `None` on purpose.** It is applied to the audio stream instead
/// (see `mixer`), and sending it here as well would put two gains in series on
/// one player — which is the bug that moved it in the first place, not a
/// belt-and-braces.
fn command_for(transport: Transport) -> Option<Command> {
    Some(match transport {
        Transport::Play => Command::Play,
        Transport::Pause => Command::Pause,
        Transport::PlayPause => Command::PlayPause,
        Transport::Next => Command::Next,
        Transport::Previous => Command::Previous,
        Transport::Seek { position_ms } => Command::Seek { position_ms },
        Transport::SetVolume { .. } => return None,
        Transport::SetShuffle { shuffle } => Command::SetShuffle { shuffle },
        Transport::SetRepeat { mode } => Command::SetRepeat { mode },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vinilo_core::music::types::{Track, TrackId};
    use vinilo_core::player::protocol::{Item, RepeatMode, Tokens};

    fn daemon() -> Rc<Daemon> {
        let (events, _) = broadcast::channel(8);
        Rc::new(Daemon {
            model: RefCell::new(Model::new()),
            sidecar: RefCell::new(None),
            events,
            restored: std::cell::Cell::new(false),
            restarts: std::cell::Cell::new(0),
            last_queue: RefCell::new(None),
            pending_start: RefCell::new(None),
            healed: std::cell::Cell::new(false),
            resume_at: std::cell::Cell::new(None),
            restart_at: RefCell::new(None),
            after_apply: std::cell::Cell::new(None),
            mpris: RefCell::new(None),
            refreshing: std::cell::Cell::new(None),
            authorization_generation: std::cell::Cell::new(0),
            art_for: RefCell::new(None),
            clients: std::cell::Cell::new(0),
            idle: std::cell::Cell::new(false),
            wake: tokio::sync::Notify::new(),
            mixer: None,
            local: RefCell::new(crate::local::Player::new()),
            last_listen: RefCell::new(None),
            quitting: tokio::sync::Notify::new(),
            stream_hits: RefCell::new(HashMap::new()),
            source: vinilo_core::provider::Provider::AppleMusic,
            refresh_again: std::cell::Cell::new(false),
            refresh_dirty: std::cell::Cell::new(false),
        })
    }

    #[test]
    fn a_legacy_musickit_volume_event_cannot_replace_stream_volume() {
        let daemon = daemon();
        daemon.model.borrow_mut().volume = 0.75;

        on_event(&daemon, PlayerEvent::Volume { volume: 0.0 });

        assert_eq!(daemon.model.borrow().volume, 0.75);
    }

    #[test]
    fn authorization_completion_makes_the_daemon_ready() {
        let daemon = daemon();
        daemon.model.borrow_mut().stage = Stage::SignedOut;

        on_event(&daemon, PlayerEvent::Authorization { authorized: true });

        assert_eq!(daemon.model.borrow().stage, Stage::Ready);
    }

    #[test]
    fn sign_in_tokens_retry_the_library_refresh() {
        let daemon = daemon();
        daemon.model.borrow_mut().stage = Stage::Ready;
        let local = tokio::task::LocalSet::new();
        let _entered = local.enter();

        on_event(
            &daemon,
            PlayerEvent::Tokens(Tokens {
                developer_token: "developer".into(),
                music_user_token: Some("user".into()),
                storefront: "us".into(),
                authorized: true,
            }),
        );

        assert_eq!(daemon.refreshing.get(), Some(0));
    }

    #[test]
    fn stale_refresh_cannot_finish_or_release_the_current_guard() {
        let daemon = daemon();
        assert_eq!(daemon.authorization_generation.get(), 0);

        on_event(&daemon, PlayerEvent::Authorization { authorized: false });
        let signed_out = daemon.authorization_generation.get();
        on_event(&daemon, PlayerEvent::SignedOut);
        assert_eq!(daemon.authorization_generation.get(), signed_out);

        on_event(&daemon, PlayerEvent::Authorization { authorized: true });
        let current = daemon.authorization_generation.get();
        assert!(current > signed_out);
        on_event(&daemon, PlayerEvent::Authorization { authorized: true });
        assert_eq!(daemon.authorization_generation.get(), current);

        let mut events = daemon.events.subscribe();
        daemon.refreshing.set(Some(signed_out));
        assert_eq!(begin_library_refresh(&daemon), Some(current));
        assert!(matches!(
            events.try_recv(),
            Ok(Event::LibraryRefreshing { refreshing: true })
        ));
        assert_eq!(begin_library_refresh(&daemon), None);
        assert!(!finish_library_refresh(&daemon, signed_out));
        assert!(events.try_recv().is_err());
        assert_eq!(daemon.refreshing.get(), Some(current));
        assert!(finish_library_refresh(&daemon, current));
        assert!(matches!(
            events.try_recv(),
            Ok(Event::LibraryRefreshing { refreshing: false })
        ));
        assert_eq!(daemon.refreshing.get(), None);
    }

    #[test]
    fn refresh_during_a_fetch_is_queued_instead_of_dropping() {
        let daemon = daemon();
        daemon.refreshing.set(Some(0));
        let mut events = daemon.events.subscribe();
        let mut feed = None;
        assert!(answer(r#"{"req":"refresh"}"#, &daemon, &mut feed).is_none());
        assert!(daemon.refresh_again.get());
        assert!(events.try_recv().is_err());
        assert_eq!(daemon.refreshing.get(), Some(0));
    }

    #[test]
    fn a_failed_refresh_runs_the_one_that_arrived_while_it_was_busy() {
        let daemon = daemon();
        daemon.refreshing.set(Some(0));
        daemon.refresh_again.set(true);
        finish_and_maybe_chain(&daemon, 0, false);
        assert!(!daemon.refresh_again.get());
        assert_eq!(daemon.refreshing.get(), None);
    }

    #[test]
    fn a_write_during_a_fetch_refreshes_again_even_after_success() {
        let daemon = daemon();
        daemon.refreshing.set(Some(0));
        request_library_refresh(&daemon);
        assert!(daemon.refresh_dirty.get());
        finish_and_maybe_chain(&daemon, 0, true);
        assert!(!daemon.refresh_dirty.get());
        assert_eq!(daemon.refreshing.get(), None);
    }

    fn populate_account_state(daemon: &Daemon) {
        let item = Item {
            occurrence_id: "old:1".into(),
            id: Some("old-library-id".into()),
            catalog_id: Some("old-catalog-id".into()),
            title: "Old song".into(),
            ..Default::default()
        };
        let mut model = daemon.model.borrow_mut();
        model.volume = 0.75;
        model.dead_ids.insert("globally-dead".into());
        model.tokens = Some(Tokens {
            developer_token: "developer".into(),
            music_user_token: Some("user".into()),
            storefront: "us".into(),
            authorized: true,
        });
        model.library.tracks.push(Track {
            date_added: String::new(),
            year: String::new(),
            favorite: false,
            in_library: true,
            library_id: Some("old-library-id".into()),
            id: TrackId("old-library-id".into()),
            catalog_id: Some("old-catalog-id".into()),
            title: "Old song".into(),
            artist: "Old artist".into(),
            album: "Old album".into(),
            duration_ms: 180_000,
            track_number: 1,
            artwork: None,
        });
        model.player.state = PlaybackState::Playing;
        model.player.now_playing = Some(item.clone());
        model.player.queue = vec![item];
        model.player.queue_position = 4;
        model.player.position_disagrees = true;
        model.player.position_ms = 42_000;
        model.player.duration_ms = 180_000;
        model.player.shuffle = true;
        model.player.repeat = RepeatMode::All;
        model.art_path = Some("/tmp/old-art".into());
        drop(model);

        daemon
            .last_queue
            .replace(Some((vec!["old-catalog-id".into()], None)));
        daemon.pending_start.replace(Some("old-catalog-id".into()));
        daemon.healed.set(true);
        daemon.resume_at.set(Some(42_000));
        daemon.restart_at.replace(Some("old:1".into()));
        daemon.after_apply.set(Some("play".into()));
        daemon.art_for.replace(Some("old-art-template".into()));
    }

    fn assert_account_state_is_empty(daemon: &Daemon) {
        let model = daemon.model.borrow();
        assert_eq!(model.stage, Stage::SignedOut);
        assert!(model.tokens.is_none());
        assert!(model.library.tracks.is_empty());
        assert!(model.player.queue.is_empty());
        assert!(model.player.now_playing.is_none());
        assert_eq!(model.player.state, PlaybackState::None);
        assert_eq!(model.player.queue_position, 0);
        assert!(!model.player.position_disagrees);
        assert_eq!(model.player.position_ms, 0);
        assert_eq!(model.player.duration_ms, 0);
        assert!(!model.player.shuffle);
        assert_eq!(model.player.repeat, RepeatMode::None);
        assert!(model.art_path.is_none());
        assert_eq!(model.volume, 0.75);
        assert!(model.dead_ids.contains("globally-dead"));
        drop(model);

        assert!(daemon.last_queue.borrow().is_none());
        assert!(daemon.pending_start.borrow().is_none());
        assert!(!daemon.healed.get());
        assert!(daemon.resume_at.get().is_none());
        assert!(daemon.restart_at.borrow().is_none());
        assert!(daemon.after_apply.take().is_none());
        assert!(daemon.art_for.borrow().is_none());
    }

    fn assert_signed_out_events(events: &mut broadcast::Receiver<Event>) {
        assert!(matches!(
            events.try_recv(),
            Ok(Event::Stage(Stage::SignedOut))
        ));
        assert!(matches!(
            events.try_recv(),
            Ok(Event::Queue { items, position }) if items.is_empty() && position == 0
        ));
        assert!(matches!(
            events.try_recv(),
            Ok(Event::Snapshot(snapshot))
                if snapshot.track_id.is_none()
                    && snapshot.title.is_empty()
                    && !snapshot.playing
        ));
        assert!(matches!(events.try_recv(), Ok(Event::LibraryChanged)));
    }

    #[test]
    fn sign_out_clears_account_state_and_notifies_clients() {
        let daemon = daemon();
        let mut events = daemon.events.subscribe();
        populate_account_state(&daemon);

        on_event(&daemon, PlayerEvent::Authorization { authorized: false });

        assert_account_state_is_empty(&daemon);
        assert_signed_out_events(&mut events);

        on_event(&daemon, PlayerEvent::SignedOut);

        assert_account_state_is_empty(&daemon);
        assert_signed_out_events(&mut events);
    }

    #[test]
    fn sign_out_confirmation_is_a_cleanup_backstop() {
        let daemon = daemon();
        let mut events = daemon.events.subscribe();
        populate_account_state(&daemon);

        on_event(&daemon, PlayerEvent::SignedOut);

        assert_account_state_is_empty(&daemon);
        assert_signed_out_events(&mut events);
    }

    #[test]
    fn sign_out_removes_account_persistence() {
        const CHILD: &str = "VINILO_SIGN_OUT_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let daemon = daemon();
            populate_account_state(&daemon);
            let model = daemon.model.borrow();
            vinilo_core::library_cache::save(&model.library.tracks, &[], &[], &[]);
            drop(model);
            vinilo_core::session::save(&vinilo_core::session::Session {
                songs: vec!["old-catalog-id".into()],
                start: 0,
                position_ms: 42_000,
                provider: String::new(),
            });
            let library = vinilo_core::paths::cache_dir()
                .expect("test cache directory")
                .join("library.json");
            let session = vinilo_core::paths::state_dir()
                .expect("test state directory")
                .join("session.json");
            assert!(library.exists());
            assert!(session.exists());

            on_event(&daemon, PlayerEvent::SignedOut);

            assert!(!library.exists());
            assert!(!session.exists());
            return;
        }

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("vinilo-sign-out-{}-{unique}", std::process::id()));
        let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "serve::tests::sign_out_removes_account_persistence",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("HOME", root.join("home"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("XDG_STATE_HOME", root.join("state"))
            .status()
            .expect("run isolated sign-out test");
        let _ = std::fs::remove_dir_all(root);
        assert!(status.success());
    }

    #[test]
    fn every_transport_verb_has_a_sidecar_command() {
        // The two enums are deliberately separate — the wire is a contract with
        // clients, `Command` is one with the sidecar — so this is what stops
        // them drifting into disagreement.
        let verbs = [
            Transport::Play,
            Transport::Pause,
            Transport::PlayPause,
            Transport::Next,
            Transport::Previous,
            Transport::Seek { position_ms: 1 },
            Transport::SetVolume { volume: 0.5 },
            Transport::SetShuffle { shuffle: true },
            Transport::SetRepeat {
                mode: vinilo_core::player::protocol::RepeatMode::All,
            },
        ];
        for verb in verbs {
            // Exhaustive by construction; the point is that it compiles and
            // runs. Volume is the one that deliberately maps to nothing —
            // asserted rather than left implied, because a future edit adding
            // it back would silently put two gains in series again.
            let sent = command_for(verb);
            assert_eq!(
                sent.is_none(),
                matches!(verb, Transport::SetVolume { .. }),
                "{verb:?} mapped to {sent:?}"
            );
        }
    }

    #[test]
    fn selected_occurrence_is_restarted_only_after_it_arrives() {
        let daemon = daemon();
        let first = vinilo_core::player::protocol::Item {
            occurrence_id: "run:1".into(),
            ..Default::default()
        };
        let second = vinilo_core::player::protocol::Item {
            occurrence_id: "run:2".into(),
            ..Default::default()
        };
        daemon.model.borrow_mut().player.queue = vec![first.clone(), second.clone()];
        daemon.restart_at.replace(Some("run:2".into()));
        daemon.model.borrow_mut().player.now_playing = Some(first);

        assert!(restart_selected_occurrence(&daemon).is_none());
        assert_eq!(daemon.restart_at.borrow().as_deref(), Some("run:2"));

        daemon.model.borrow_mut().player.now_playing = Some(second);
        assert!(matches!(
            restart_selected_occurrence(&daemon),
            Some(Command::Seek { position_ms: 0 })
        ));
        assert!(daemon.restart_at.borrow().is_none());
    }

    #[test]
    fn musickit_cannot_put_the_last_apple_track_over_a_local_file() {
        let daemon = daemon();
        populate_account_state(&daemon);
        daemon.local.borrow_mut().hold_for_test(
            "Local title",
            "Local artist",
            Some("/tmp/local-cover.jpg".into()),
        );
        publish_local(&daemon);

        {
            let model = daemon.model.borrow();
            let item = model.player.now_playing.as_ref().expect("local item");
            assert_eq!(item.title, "Local title");
            assert_eq!(item.artist, "Local artist");
            assert_eq!(
                model.art_path.as_deref(),
                Some(std::path::Path::new("/tmp/local-cover.jpg"))
            );
        }

        // What MusicKit does after we Pause it: it still holds the last Apple
        // track and echoes it. That used to overwrite the bar.
        on_event(
            &daemon,
            PlayerEvent::NowPlaying {
                item: Some(Item {
                    title: "Apple title".into(),
                    artist: "Apple artist".into(),
                    artwork_template: Some(
                        "https://is1-ssl.mzstatic.com/image/{w}x{h}bb.jpg".into(),
                    ),
                    ..Default::default()
                }),
                queue: Default::default(),
            },
        );
        on_event(
            &daemon,
            PlayerEvent::PlaybackState {
                state: PlaybackState::Paused,
            },
        );

        let model = daemon.model.borrow();
        let item = model.player.now_playing.as_ref().expect("local item");
        assert_eq!(item.title, "Local title");
        assert_eq!(item.artist, "Local artist");
        assert_eq!(
            model.art_path.as_deref(),
            Some(std::path::Path::new("/tmp/local-cover.jpg"))
        );
    }

    #[test]
    fn local_volume_moves_the_rodio_gain() {
        let daemon = daemon();
        daemon.model.borrow_mut().volume = 1.0;
        daemon
            .local
            .borrow_mut()
            .hold_for_test("Local title", "Local artist", None);

        route_local_transport(&daemon, Transport::SetVolume { volume: 0.25 });

        assert!((daemon.local.borrow().volume() - 0.25).abs() < f64::EPSILON);
        assert!((daemon.model.borrow().volume - 0.25).abs() < f64::EPSILON);
    }
}
