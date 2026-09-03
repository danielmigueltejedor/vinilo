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
use std::path::{Path, PathBuf};
use std::time::Duration;

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use vinilo_core::local_files::expand_audio_paths;
use vinilo_core::player::protocol::{Item, PlaybackState, Queue, RepeatMode};
use vinilo_core::player::protocol::Event as PlayerEvent;

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
}

impl Track {
    fn as_item(&self) -> Item {
        Item {
            occurrence_id: self.path.to_string_lossy().into_owned(),
            id: Some(self.path.to_string_lossy().into_owned()),
            catalog_id: None,
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            duration_ms: self.duration_ms,
            track_number: 0,
            artwork_template: None,
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
            active: false,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn stop(&mut self) {
        if let Some(sink) = self.sink.as_ref() {
            sink.stop();
        }
        self.active = false;
        self.queue.clear();
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
        self.queue = queue;
        self.index = index;
        self.active = true;
        self.start_current()
    }

    pub fn play(&mut self) {
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
        self.active && self.sink.as_ref().is_some_and(|s| !s.is_paused() && !s.empty())
    }

    /// True when the current decoder has run out, so the caller should advance.
    pub fn ended(&self) -> bool {
        self.active && self.sink.as_ref().is_some_and(|s| s.empty() && !s.is_paused())
    }

    pub fn next(&mut self) -> Result<bool, String> {
        if self.queue.is_empty() {
            return Ok(false);
        }
        if self.index + 1 < self.queue.len() {
            self.index += 1;
            self.start_current()?;
            return Ok(true);
        }
        match self.repeat {
            RepeatMode::All => {
                self.index = 0;
                self.start_current()?;
                Ok(true)
            }
            RepeatMode::One => {
                self.start_current()?;
                Ok(true)
            }
            RepeatMode::None => {
                self.pause();
                Ok(false)
            }
        }
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
        self.queue.get(self.index).map(|t| t.duration_ms).unwrap_or(0)
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
        } else if self.is_paused() {
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
            std::env::set_var(
                "PULSE_PROP_application.icon_name",
                vinilo_core::APP_ID,
            );
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
        let path = track.path.clone();
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| "no audio output".to_string())?;
        let file = File::open(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        let decoder = Decoder::new(BufReader::new(file))
            .map_err(|err| format!("{}: {err}", path.display()))?;
        let sink = Sink::try_new(handle).map_err(|err| format!("audio sink: {err}"))?;
        sink.set_volume(self.volume as f32);
        sink.append(decoder);
        sink.play();
        self.sink = Some(sink);
        Ok(())
    }
}

fn read_track(path: &Path) -> Track {
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Audio")
        .to_owned();
    match lofty::read_from_path(path) {
        Ok(tagged) => {
            let duration_ms = tagged.properties().duration().as_millis() as u64;
            let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
            let title = tag
                .and_then(|t| t.title().map(|s: std::borrow::Cow<'_, str>| s.into_owned()))
                .filter(|s| !s.is_empty())
                .unwrap_or(title);
            let artist = tag
                .and_then(|t| t.artist().map(|s: std::borrow::Cow<'_, str>| s.into_owned()))
                .unwrap_or_default();
            let album = tag
                .and_then(|t| t.album().map(|s: std::borrow::Cow<'_, str>| s.into_owned()))
                .unwrap_or_default();
            let art_path = tag.and_then(write_picture);
            Track {
                path: path.to_path_buf(),
                title,
                artist,
                album,
                duration_ms,
                art_path,
            }
        }
        Err(_) => Track {
            path: path.to_path_buf(),
            title,
            artist: String::new(),
            album: String::new(),
            duration_ms: 0,
            art_path: None,
        },
    }
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
        tag.album()
            .unwrap_or_default()
            .replace('/', "_"),
        tag.title()
            .unwrap_or_default()
            .replace('/', "_"),
    );
    let path = dir.join(name);
    if !path.is_file() {
        std::fs::write(&path, picture.data()).ok()?;
    }
    Some(path)
}
