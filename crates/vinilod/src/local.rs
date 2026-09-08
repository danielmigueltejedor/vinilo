// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Playback of files on this computer.
//!
//! MusicKit cannot open a path, so files never go through the sidecar. This
//! owns a rodio sink on the daemon thread and reports the same [`Item`]
//! shape the rest of the daemon already mirrors — one now-playing bar, two
//! sources.

use std::fs::File;
use std::io::BufReader;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use vinilo_core::local_files::expand_audio_paths;
use vinilo_core::player::protocol::Event as PlayerEvent;
use vinilo_core::player::protocol::{Item, PlaybackState, Queue, RepeatMode};
use vinilo_core::streams::{self, StreamHit};

use lofty::prelude::*;

/// One file on a local queue.
#[derive(Debug, Clone)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub art_path: Option<PathBuf>,
    pub catalog_id: Option<String>,
    pub artwork_template: Option<String>,
}

impl Track {
    fn as_item(&self) -> Item {
        Item {
            occurrence_id: self.path.to_string_lossy().into_owned(),
            id: self
                .catalog_id
                .clone()
                .or_else(|| Some(self.path.to_string_lossy().into_owned())),
            catalog_id: self.catalog_id.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            duration_ms: self.duration_ms,
            track_number: 0,
            artwork_template: self.artwork_template.clone(),
        }
    }
}

/// The local decoder. Empty until somebody opens a file.
pub struct Player {
    stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    sink: Option<Sink>,
    queue: Vec<Track>,
    index: usize,
    volume: f64,
    repeat: RepeatMode,
    shuffle: bool,
    /// Queue order before shuffle was turned on, so turning it off can restore.
    unshuffled: Option<Vec<Track>>,
    /// True while this, not MusicKit, owns the queue.
    active: bool,
}

impl Player {
    pub fn new() -> Self {
        Self {
            stream: None,
            handle: None,
            sink: None,
            queue: Vec::new(),
            index: 0,
            volume: 1.0,
            repeat: RepeatMode::None,
            shuffle: false,
            unshuffled: None,
            active: false,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    #[cfg(test)]
    pub(crate) fn hold_for_test(&mut self, title: &str, artist: &str, art: Option<PathBuf>) {
        self.active = true;
        self.queue = vec![Track {
            path: PathBuf::from("/tmp/local-file.flac"),
            title: title.to_owned(),
            artist: artist.to_owned(),
            album: "Local album".into(),
            duration_ms: 1_000,
            art_path: art,
            catalog_id: None,
            artwork_template: None,
        }];
        self.index = 0;
    }

    pub fn stop(&mut self) {
        if let Some(sink) = self.sink.as_ref() {
            sink.stop();
        }
        self.active = false;
        self.queue.clear();
        self.unshuffled = None;
        self.index = 0;
    }

    /// Replace the queue with these paths and start at `index`.
    pub fn play_paths(&mut self, paths: Vec<PathBuf>, index: usize) -> Result<(), String> {
        let files = expand_audio_paths(paths);
        if files.is_empty() {
            return Err("Nothing here is an audio file".into());
        }
        let queue: Vec<Track> = files.iter().map(|p| read_track(p)).collect();
        let index = index.min(queue.len().saturating_sub(1));
        self.ensure_output()?;
        self.take_queue(queue, index)?;
        Ok(())
    }

    /// Play downloaded catalogue audio, using the search/library title rather
    /// than the `yt_…` filename yt-dlp wrote.
    pub fn play_hits(
        &mut self,
        files: Vec<(PathBuf, StreamHit)>,
        index: usize,
    ) -> Result<(), String> {
        if files.is_empty() {
            return Err("Nothing here is an audio file".into());
        }
        let queue: Vec<Track> = files
            .iter()
            .map(|(path, hit)| read_track_labeled(path, Some(hit)))
            .collect();
        let index = index.min(queue.len().saturating_sub(1));
        self.ensure_output()?;
        self.take_queue(queue, index)?;
        Ok(())
    }

    fn take_queue(&mut self, mut queue: Vec<Track>, index: usize) -> Result<(), String> {
        let index = index.min(queue.len().saturating_sub(1));
        if self.shuffle && queue.len() > 1 {
            queue.swap(0, index);
            shuffle_tail(&mut queue, 0);
            self.unshuffled = None;
            self.queue = queue;
            self.index = 0;
        } else {
            self.unshuffled = None;
            self.queue = queue;
            self.index = index;
        }
        self.start_current()?;
        self.active = true;
        Ok(())
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    /// Add a file to a queue that is already playing, without restarting.
    #[allow(dead_code)]
    pub fn append_path(&mut self, path: PathBuf) {
        if !self.active {
            return;
        }
        self.queue.push(read_track(&path));
    }

    pub fn append_hit(&mut self, path: PathBuf, hit: StreamHit) {
        if !self.active {
            return;
        }
        if self
            .queue
            .iter()
            .any(|t| t.catalog_id.as_deref() == Some(hit.id.as_str()))
        {
            return;
        }
        let track = read_track_labeled(&path, Some(&hit));
        if let Some(original) = self.unshuffled.as_mut() {
            original.push(track.clone());
        }
        self.queue.push(track);
    }

    pub fn play(&mut self) {
        // An empty sink is a finished file, not a paused one. `Sink::play`
        // on it is silence; the file has to be opened again.
        if self.should_reload() {
            let _ = self.start_current();
            return;
        }
        if let Some(sink) = self.sink.as_ref() {
            sink.play();
        }
    }

    pub fn pause(&mut self) {
        if let Some(sink) = self.sink.as_ref() {
            sink.pause();
        }
    }

    pub fn play_pause(&mut self) {
        if self.should_reload() {
            let _ = self.start_current();
            return;
        }
        let Some(sink) = self.sink.as_ref() else {
            return;
        };
        if sink.is_paused() {
            sink.play();
        } else {
            sink.pause();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.sink.as_ref().is_none_or(Sink::is_paused)
    }

    pub fn is_playing(&self) -> bool {
        self.active
            && self
                .sink
                .as_ref()
                .is_some_and(|s| !s.is_paused() && !s.empty())
    }

    /// True when the current decoder has run out, so the caller should advance.
    pub fn ended(&self) -> bool {
        self.active
            && self
                .sink
                .as_ref()
                .is_some_and(|s| s.empty() && !s.is_paused())
    }

    /// Queue still here, nothing on the decoder — Play must open the file again.
    pub(crate) fn should_reload(&self) -> bool {
        self.active && !self.queue.is_empty() && self.sink.as_ref().is_none_or(Sink::empty)
    }

    #[cfg(test)]
    pub(crate) fn volume(&self) -> f64 {
        self.volume
    }

    /// User skip: always leave the current track, wrapping when Repeat is All.
    pub fn next(&mut self) -> Result<bool, String> {
        if self.queue.is_empty() {
            return Ok(false);
        }
        if self.index + 1 < self.queue.len() {
            self.index += 1;
            self.start_current()?;
            return Ok(true);
        }
        if self.repeat == RepeatMode::All {
            self.index = 0;
            self.start_current()?;
            return Ok(true);
        }
        self.pause();
        Ok(false)
    }

    /// The decoder ran out: honour Repeat One on this track, else skip.
    pub fn advance_ended(&mut self) -> Result<bool, String> {
        if self.queue.is_empty() {
            return Ok(false);
        }
        if self.repeat == RepeatMode::One {
            self.start_current()?;
            return Ok(true);
        }
        self.next()
    }

    pub fn previous(&mut self) -> Result<bool, String> {
        if self.queue.is_empty() {
            return Ok(false);
        }
        if self.position_ms() > 3_000 {
            self.seek(0);
            return Ok(true);
        }
        if self.index == 0 {
            match self.repeat {
                RepeatMode::All => self.index = self.queue.len() - 1,
                _ => {
                    self.seek(0);
                    return Ok(true);
                }
            }
        } else {
            self.index -= 1;
        }
        self.start_current()?;
        Ok(true)
    }

    pub fn jump(&mut self, index: usize) -> Result<(), String> {
        if index >= self.queue.len() {
            return Err("That track is not on the local queue".into());
        }
        self.index = index;
        self.start_current()
    }

    pub fn seek(&mut self, position_ms: u64) {
        let Some(sink) = self.sink.as_mut() else {
            return;
        };
        let _ = sink.try_seek(Duration::from_millis(position_ms));
    }

    pub fn position_ms(&self) -> u64 {
        self.sink
            .as_ref()
            .map(|s| s.get_pos().as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn duration_ms(&self) -> u64 {
        self.queue
            .get(self.index)
            .map(|t| t.duration_ms)
            .unwrap_or(0)
    }

    pub fn art_path(&self) -> Option<PathBuf> {
        self.queue.get(self.index).and_then(|t| t.art_path.clone())
    }

    pub fn set_volume(&mut self, volume: f64) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(sink) = self.sink.as_ref() {
            sink.set_volume(self.volume as f32);
        }
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        if self.shuffle == shuffle {
            return;
        }
        self.shuffle = shuffle;
        if !self.active || self.queue.len() < 2 {
            return;
        }
        let current = track_key(&self.queue[self.index]);
        if shuffle {
            if self.unshuffled.is_none() {
                self.unshuffled = Some(self.queue.clone());
            }
            shuffle_tail(&mut self.queue, self.index);
            return;
        }
        let Some(original) = self.unshuffled.take() else {
            return;
        };
        self.queue = original;
        self.index = self
            .queue
            .iter()
            .position(|t| track_key(t) == current)
            .unwrap_or(self.index.min(self.queue.len().saturating_sub(1)));
    }

    pub fn remove(&mut self, index: usize) -> Result<(), String> {
        if index >= self.queue.len() {
            return Err("That track is not on the local queue".into());
        }
        if index == self.index {
            return Err("Can't remove the track that's playing".into());
        }
        self.queue.remove(index);
        if index < self.index {
            self.index -= 1;
        }
        Ok(())
    }

    pub fn move_item(&mut self, from: usize, to: usize) -> Result<(), String> {
        if from >= self.queue.len() {
            return Err("That track is not on the local queue".into());
        }
        let item = self.queue.remove(from);
        let to = to.min(self.queue.len());
        self.queue.insert(to, item);
        if self.index == from {
            self.index = to;
        } else if from < self.index && to >= self.index {
            self.index -= 1;
        } else if from > self.index && to <= self.index {
            self.index += 1;
        }
        Ok(())
    }

    pub fn now_playing_event(&self) -> PlayerEvent {
        let items: Vec<Item> = self.queue.iter().map(Track::as_item).collect();
        let item = items.get(self.index).cloned();
        PlayerEvent::NowPlaying {
            item,
            queue: Queue {
                position: self.index as i64,
                items,
                ..Default::default()
            },
        }
    }

    pub fn playback_event(&self) -> PlayerEvent {
        let state = if !self.active {
            PlaybackState::None
        } else if self.is_paused() || self.sink.as_ref().is_some_and(Sink::empty) {
            // Finished files stay on the bar as paused, so Play can open them
            // again instead of looking like they are still going.
            PlaybackState::Paused
        } else {
            PlaybackState::Playing
        };
        PlayerEvent::PlaybackState { state }
    }

    pub fn position_event(&self) -> PlayerEvent {
        PlayerEvent::Position {
            position_ms: self.position_ms(),
            duration_ms: self.duration_ms(),
        }
    }

    fn ensure_output(&mut self) -> Result<(), String> {
        if self.handle.is_some() {
            return Ok(());
        }
        // So the desktop mixer labels this stream the same way it labels the
        // sidecar — `mixer.rs` looks up `application.name = Vinilo`.
        unsafe {
            std::env::set_var("PULSE_PROP_application.name", "Vinilo");
            std::env::set_var("PULSE_PROP_application.icon_name", vinilo_core::APP_ID);
            std::env::set_var("PULSE_PROP_media.role", "music");
        }
        let (stream, handle) =
            OutputStream::try_default().map_err(|err| format!("no audio output: {err}"))?;
        self.stream = Some(stream);
        self.handle = Some(handle);
        Ok(())
    }

    fn start_current(&mut self) -> Result<(), String> {
        let Some(track) = self.queue.get(self.index) else {
            return Err("the local queue is empty".into());
        };
        let mut path = track.path.clone();
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| "no audio output".to_string())?;
        let decoder = match open_decoder(&path) {
            Ok(decoder) => decoder,
            Err(first) => {
                // rodio 0.19 panics on some webm/opus instead of returning Err.
                tracing::warn!(
                    path = %path.display(),
                    err = %first,
                    "native decode failed; transcoding"
                );
                let converted = crate::ytdlp::ensure_native(path.clone());
                if converted == path {
                    return Err(first);
                }
                let decoder = open_decoder(&converted)
                    .map_err(|err| format!("{first}; after transcode: {err}"))?;
                path = converted;
                decoder
            }
        };
        if let Some(track) = self.queue.get_mut(self.index) {
            track.path = path;
        }
        let sink = Sink::try_new(handle).map_err(|err| format!("audio sink: {err}"))?;
        sink.set_volume(self.volume as f32);
        sink.append(decoder);
        sink.play();
        self.sink = Some(sink);
        Ok(())
    }
}

fn open_decoder(path: &Path) -> Result<Decoder<BufReader<File>>, String> {
    let file = File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    catch_unwind(AssertUnwindSafe(|| Decoder::new(BufReader::new(file))))
        .map_err(|_| {
            format!(
                "{}: decoder panicked (unsupported or truncated audio)",
                path.display()
            )
        })?
        .map_err(|err| format!("{}: {err}", path.display()))
}

fn read_track(path: &Path) -> Track {
    read_track_labeled(path, None)
}

fn read_track_labeled(path: &Path, hit: Option<&StreamHit>) -> Track {
    let filename = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Audio")
        .to_owned();
    let sidecar = streams::read_sidecar(path);
    let folder_art = folder_cover(path);
    let mut track = match lofty::read_from_path(path) {
        Ok(tagged) => {
            let duration_ms = tagged.properties().duration().as_millis() as u64;
            let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
            let title = tag
                .and_then(|t| t.title().map(|s: std::borrow::Cow<'_, str>| s.into_owned()))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| filename.clone());
            let artist = tag
                .and_then(|t| {
                    t.artist()
                        .map(|s: std::borrow::Cow<'_, str>| s.into_owned())
                })
                .unwrap_or_default();
            let album = tag
                .and_then(|t| t.album().map(|s: std::borrow::Cow<'_, str>| s.into_owned()))
                .unwrap_or_default();
            let art_path = tagged.tags().iter().find_map(write_picture).or(folder_art);
            Track {
                path: path.to_path_buf(),
                title,
                artist,
                album,
                duration_ms,
                art_path,
                catalog_id: None,
                artwork_template: None,
            }
        }
        Err(_) => Track {
            path: path.to_path_buf(),
            title: filename.clone(),
            artist: String::new(),
            album: String::new(),
            duration_ms: 0,
            art_path: folder_art,
            catalog_id: None,
            artwork_template: None,
        },
    };
    if let Some(meta) = sidecar.as_ref() {
        apply_meta(&mut track, meta);
    }
    if let Some(hit) = hit {
        if !hit.title.is_empty() && !hit.title_is_placeholder() {
            track.title = hit.title.clone();
        }
        if !hit.artist.is_empty() {
            track.artist = hit.artist.clone();
        }
        if !hit.album.is_empty() {
            track.album = hit.album.clone();
        }
        if hit.duration_ms > 0 && track.duration_ms == 0 {
            track.duration_ms = hit.duration_ms;
        }
        track.catalog_id = Some(hit.id.clone());
        track.artwork_template = hit.artwork.clone();
        streams::write_sidecar(path, hit);
    } else if (track.title == filename
        || track.title.starts_with("yt_")
        || track.title.starts_with("sp_"))
        && let Some(meta) = sidecar
        && !meta.title.is_empty()
    {
        track.title = meta.title;
    }
    track
}

fn apply_meta(track: &mut Track, meta: &streams::FileMeta) {
    if !meta.title.is_empty() {
        track.title = meta.title.clone();
    }
    if !meta.artist.is_empty() {
        track.artist = meta.artist.clone();
    }
    if !meta.album.is_empty() {
        track.album = meta.album.clone();
    }
    if track.catalog_id.is_none() && !meta.id.is_empty() {
        track.catalog_id = Some(meta.id.clone());
    }
    if track.artwork_template.is_none() {
        track.artwork_template = meta.artwork.clone();
    }
    if track.duration_ms == 0 && meta.duration_ms > 0 {
        track.duration_ms = meta.duration_ms;
    }
}

fn track_key(track: &Track) -> String {
    track
        .catalog_id
        .clone()
        .unwrap_or_else(|| track.path.to_string_lossy().into_owned())
}

/// Fisher–Yates on everything after `keep`, so the playing track stays put.
fn shuffle_tail<T>(items: &mut [T], keep: usize) {
    if items.len() <= keep + 1 {
        return;
    }
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    if seed == 0 {
        seed = 1;
    }
    for i in ((keep + 1)..items.len()).rev() {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let span = i - keep;
        let j = keep + 1 + (seed as usize % span);
        items.swap(i, j);
    }
}

/// `cover.jpg` / `folder.png` beside the file, the convention every local
/// player follows when the tags have no picture.
fn folder_cover(audio: &Path) -> Option<PathBuf> {
    let dir = audio.parent()?;
    const NAMES: &[&str] = &[
        "cover.jpg",
        "cover.jpeg",
        "cover.png",
        "cover.webp",
        "folder.jpg",
        "folder.jpeg",
        "folder.png",
        "front.jpg",
        "front.png",
        "albumart.jpg",
        "album.jpg",
    ];
    let entries = std::fs::read_dir(dir).ok()?;
    let mut found: Vec<(usize, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name()?.to_str()?.to_ascii_lowercase();
        if let Some(rank) = NAMES.iter().position(|n| *n == name) {
            found.push((rank, path));
        }
    }
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().next().map(|(_, path)| path)
}

fn write_picture(tag: &lofty::tag::Tag) -> Option<PathBuf> {
    let picture = tag.pictures().first()?;
    let dir = vinilo_core::paths::artwork_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let ext = match picture.mime_type().map(|m| m.to_string()) {
        Some(m) if m.contains("png") => "png",
        _ => "jpg",
    };
    let name = format!(
        "local-{}-{}.{ext}",
        tag.album().unwrap_or_default().replace('/', "_"),
        tag.title().unwrap_or_default().replace('/', "_"),
    );
    let path = dir.join(name);
    if !path.is_file() {
        std::fs::write(&path, picture.data()).ok()?;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cover_jpg_beside_the_file_is_used_when_tags_have_no_picture() {
        let dir = std::env::temp_dir().join(format!("vinilo-local-art-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("track.mp3");
        std::fs::write(&audio, b"not a real mp3").unwrap();
        let cover = dir.join("cover.jpg");
        std::fs::write(&cover, b"jpeg").unwrap();
        let track = read_track(&audio);
        assert_eq!(track.art_path.as_deref(), Some(cover.as_path()));
        assert_eq!(track.title, "track");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_beats_the_yt_dlp_filename() {
        let dir = std::env::temp_dir().join(format!("vinilo-local-meta-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("yt_fhabc123.mp3");
        std::fs::write(&audio, b"not a real mp3").unwrap();
        let hit = StreamHit {
            id: "sp:abc".into(),
            title: "Pa Mal".into(),
            artist: "Aitana".into(),
            album: "Alpha".into(),
            duration_ms: 180_000,
            artwork: Some("https://i.scdn.co/image/x".into()),
            play_query: "https://open.spotify.com/track/abc".into(),
        };
        streams::write_sidecar(&audio, &hit);
        let track = read_track(&audio);
        assert_eq!(track.title, "Pa Mal");
        assert_eq!(track.artist, "Aitana");
        assert_eq!(track.catalog_id.as_deref(), Some("sp:abc"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_finished_file_is_paused_and_play_must_reload_it() {
        let mut player = Player::new();
        player.hold_for_test("Local", "Artist", None);
        assert!(
            player.should_reload(),
            "no decoder yet is the same hole as a file that has run out"
        );
        match player.playback_event() {
            PlayerEvent::PlaybackState {
                state: PlaybackState::Paused,
            } => {}
            other => panic!("expected paused so the bar offers Play, got {other:?}"),
        }
        player.set_volume(0.4);
        assert!((player.volume() - 0.4).abs() < f64::EPSILON);
        player.set_volume(3.0);
        assert!((player.volume() - 1.0).abs() < f64::EPSILON);
    }

    fn named_track(name: &str) -> Track {
        Track {
            path: PathBuf::from(format!("/tmp/{name}")),
            title: name.to_owned(),
            artist: "x".into(),
            album: String::new(),
            duration_ms: 1_000,
            art_path: None,
            catalog_id: Some(name.to_owned()),
            artwork_template: None,
        }
    }

    #[test]
    fn shuffle_tail_leaves_the_playing_track_in_place() {
        let mut items: Vec<u8> = (0..8).collect();
        shuffle_tail(&mut items, 2);
        assert_eq!(items[2], 2);
        let mut sorted = items.clone();
        sorted.sort();
        assert_eq!(sorted, (0..8).collect::<Vec<_>>());
    }

    #[test]
    fn turning_shuffle_off_restores_the_original_order() {
        let mut player = Player::new();
        player.active = true;
        player.queue = ["a", "b", "c", "d"].into_iter().map(named_track).collect();
        player.index = 0;
        player.set_shuffle(true);
        assert_eq!(player.queue[0].title, "a");
        player.set_shuffle(false);
        let titles: Vec<_> = player.queue.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["a", "b", "c", "d"]);
    }
}
