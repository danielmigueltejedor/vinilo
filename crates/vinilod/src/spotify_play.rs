// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Spotify Premium playback through librespot, the same path Sonora uses.
//!
//! Cookie login in the GTK window is only the catalogue. Streaming needs an
//! OAuth token with the `streaming` scope, cached under
//! `~/.cache/vinilo/librespot/`. Without Premium, callers fall back to yt-dlp.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use librespot_core::authentication::Credentials;
use librespot_core::cache::Cache;
use librespot_core::{Session, SessionConfig, SpotifyUri};
use librespot_oauth::OAuthClientBuilder;
use librespot_playback::audio_backend::{Sink, SinkError, SinkResult};
use librespot_playback::config::{Bitrate, PlayerConfig};
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::NoOpVolume;
use librespot_playback::player::{Player, PlayerEvent};
use librespot_playback::{NUM_CHANNELS, SAMPLE_RATE};
use rodio::{OutputStream, Sink as RodioSink};
use tokio::sync::mpsc::UnboundedReceiver;
use vinilo_core::player::protocol::RepeatMode;
use vinilo_core::streams::StreamHit;

/// Spotify's own desktop client id — the same one Sonora and librespot demos use.
const CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const REDIRECT: &str = "http://127.0.0.1:8989/login";
const SCOPES: &[&str] = &[
    "playlist-read-collaborative",
    "playlist-read-private",
    "streaming",
    "user-follow-read",
    "user-library-read",
    "user-read-email",
    "user-read-playback-state",
    "user-read-private",
    "user-read-recently-played",
    "user-top-read",
];

fn cache_dir() -> Option<PathBuf> {
    Some(vinilo_core::paths::cache_dir()?.join("librespot"))
}

/// Open a browser for Spotify Premium OAuth and cache the session.
async fn login() -> Result<()> {
    let dir = cache_dir().context("no cache directory")?;
    std::fs::create_dir_all(&dir).context("librespot cache")?;
    let token = tokio::task::spawn_blocking(|| {
        OAuthClientBuilder::new(CLIENT_ID, REDIRECT, SCOPES.to_vec())
            .open_in_browser()
            .build()?
            .get_access_token()
    })
    .await
    .context("oauth task")?
    .context("spotify oauth")?;
    let session = session(&dir)?;
    session
        .connect(Credentials::with_access_token(token.access_token), true)
        .await
        .context("spotify connect")?;
    require_premium(&session).await?;
    Ok(())
}

async fn restore() -> Result<Option<Session>> {
    let Some(dir) = cache_dir() else {
        return Ok(None);
    };
    if !dir.exists() {
        return Ok(None);
    }
    let session = session(&dir)?;
    let Some(credentials) = session.cache().and_then(|cache| cache.credentials()) else {
        return Ok(None);
    };
    session
        .connect(credentials, true)
        .await
        .context("spotify restore")?;
    require_premium(&session).await?;
    Ok(Some(session))
}

fn session(dir: &std::path::Path) -> Result<Session> {
    let cache = Cache::new(Some(dir), None, None, None)
        .with_context(|| format!("librespot cache at {}", dir.display()))?;
    let config = SessionConfig {
        client_id: CLIENT_ID.to_owned(),
        ..Default::default()
    };
    Ok(Session::new(config, Some(cache)))
}

async fn require_premium(session: &Session) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(kind) = session.user_data().attributes.get("type") {
            if kind == "premium" {
                return Ok(());
            }
            session.shutdown();
            bail!("Spotify Premium is required for native playback");
        }
        if tokio::time::Instant::now() >= deadline {
            // Spotify sometimes answers late; try playing rather than blocking.
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn track_id(hit: &StreamHit) -> Result<String> {
    let rest = hit
        .id
        .strip_prefix("sp:")
        .context("not a Spotify track id")?;
    let id = rest.strip_prefix("track:").unwrap_or(rest);
    if id.is_empty() || id.contains(':') {
        bail!("not a Spotify track id");
    }
    Ok(id.to_owned())
}

/// Shared gain for the rodio sink, the same idea as Sonora's `Volume`.
#[derive(Clone)]
struct Gain(Arc<AtomicU32>);

impl Gain {
    fn new(volume: f64) -> Self {
        let gain = Self(Arc::new(AtomicU32::new(0)));
        gain.set(volume);
        gain
    }

    fn set(&self, volume: f64) {
        let volume = volume.clamp(0.0, 1.0) as f32;
        self.0.store(volume.to_bits(), Ordering::Relaxed);
    }

    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// A librespot session plus a rodio sink that owns the speakers for Spotify.
pub struct Engine {
    player: Arc<Player>,
    events: UnboundedReceiver<PlayerEvent>,
    queue: Vec<StreamHit>,
    index: usize,
    playing: bool,
    position_ms: u64,
    ended: Arc<AtomicBool>,
    active: bool,
    gain: Gain,
    repeat: RepeatMode,
}

impl Engine {
    /// Restore or open OAuth, then start the first track.
    pub async fn start(queue: Vec<StreamHit>, index: usize, volume: f64) -> Result<Self> {
        if queue.is_empty() {
            bail!("empty Spotify queue");
        }
        let session = match restore().await? {
            Some(session) => session,
            None => {
                login().await?;
                restore()
                    .await?
                    .context("Spotify signed in but no session was stored")?
            }
        };
        let ended = Arc::new(AtomicBool::new(false));
        let gain = Gain::new(volume);
        let player_config = PlayerConfig {
            bitrate: Bitrate::Bitrate320,
            gapless: true,
            position_update_interval: Some(Duration::from_millis(500)),
            ..Default::default()
        };
        let sink_gain = gain.clone();
        let player = Player::new(player_config, session, Box::new(NoOpVolume), move || {
            RodioBackend::boxed(sink_gain.clone())
        });
        let events = player.get_player_event_channel();
        let index = index.min(queue.len().saturating_sub(1));
        let engine = Self {
            player,
            events,
            queue,
            index,
            playing: true,
            position_ms: 0,
            ended,
            active: true,
            gain,
            repeat: RepeatMode::None,
        };
        engine.load_current(true)?;
        Ok(engine)
    }

    fn load_current(&self, play: bool) -> Result<()> {
        let hit = self
            .queue
            .get(self.index)
            .context("Spotify queue is empty")?;
        let id = track_id(hit)?;
        let uri = SpotifyUri::from_uri(&format!("spotify:track:{id}"))
            .with_context(|| format!("{id} is not a track id"))?;
        self.ended.store(false, Ordering::Relaxed);
        self.player.load(uri, play, 0);
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn position_ms(&self) -> u64 {
        self.position_ms
    }

    pub fn stop(&mut self) {
        self.player.stop();
        self.playing = false;
        self.active = false;
    }

    pub fn play(&mut self) {
        self.player.play();
        self.playing = true;
    }

    pub fn pause(&mut self) {
        self.player.pause();
        self.playing = false;
    }

    pub fn seek(&mut self, position_ms: u64) {
        let position_ms = position_ms.min(u32::MAX as u64);
        self.player.seek(position_ms as u32);
        self.position_ms = position_ms;
    }

    pub fn set_volume(&mut self, volume: f64) {
        self.gain.set(volume);
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    pub fn next(&mut self) -> Result<bool> {
        self.advance(false)
    }

    /// End of track: honour Repeat One / All. User skip always leaves the track.
    pub fn advance_ended(&mut self) -> Result<bool> {
        self.advance(true)
    }

    fn advance(&mut self, from_end: bool) -> Result<bool> {
        if self.queue.is_empty() {
            self.pause();
            return Ok(false);
        }
        if from_end && self.repeat == RepeatMode::One {
            self.position_ms = 0;
            self.playing = true;
            self.load_current(true)?;
            return Ok(true);
        }
        if self.index + 1 < self.queue.len() {
            self.index += 1;
            self.position_ms = 0;
            self.playing = true;
            self.load_current(true)?;
            return Ok(true);
        }
        if self.repeat == RepeatMode::All {
            self.index = 0;
            self.position_ms = 0;
            self.playing = true;
            self.load_current(true)?;
            return Ok(true);
        }
        self.pause();
        Ok(false)
    }

    pub fn append(&mut self, hits: Vec<StreamHit>) {
        self.insert_at(self.queue.len(), hits);
    }

    pub fn insert_next(&mut self, hits: Vec<StreamHit>) {
        self.insert_at(self.index + 1, hits);
    }

    fn insert_at(&mut self, at: usize, hits: Vec<StreamHit>) {
        let mut at = at.min(self.queue.len());
        for hit in hits {
            if self.queue.iter().any(|existing| existing.id == hit.id) {
                continue;
            }
            self.queue.insert(at, hit);
            at += 1;
        }
    }

    pub fn remove(&mut self, index: usize) -> Result<()> {
        if index >= self.queue.len() {
            bail!("That track is not on the Spotify queue");
        }
        if index == self.index {
            bail!("Can't remove the track that's playing");
        }
        self.queue.remove(index);
        if index < self.index {
            self.index -= 1;
        }
        Ok(())
    }

    pub fn move_item(&mut self, from: usize, to: usize) -> Result<()> {
        if from >= self.queue.len() {
            bail!("That track is not on the Spotify queue");
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

    pub fn previous(&mut self) -> Result<bool> {
        if self.index == 0 {
            self.seek(0);
            return Ok(true);
        }
        self.index -= 1;
        self.position_ms = 0;
        self.playing = true;
        self.load_current(true)?;
        Ok(true)
    }

    pub fn jump(&mut self, index: usize) -> Result<()> {
        if index >= self.queue.len() {
            bail!("That track is not on the Spotify queue");
        }
        self.index = index;
        self.position_ms = 0;
        self.playing = true;
        self.load_current(true)
    }

    pub fn queue(&self) -> &[StreamHit] {
        &self.queue
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// Drain librespot events. Returns true when the current track ended.
    pub fn poll_ended(&mut self) -> bool {
        while let Ok(event) = self.events.try_recv() {
            match event {
                PlayerEvent::EndOfTrack { .. } => {
                    self.ended.store(true, Ordering::Relaxed);
                }
                PlayerEvent::Unavailable { .. } => {
                    self.ended.store(true, Ordering::Relaxed);
                    self.playing = false;
                }
                PlayerEvent::Playing { position_ms, .. } => {
                    self.playing = true;
                    self.position_ms = u64::from(position_ms);
                }
                PlayerEvent::Paused { position_ms, .. } => {
                    self.playing = false;
                    self.position_ms = u64::from(position_ms);
                }
                PlayerEvent::Seeked { position_ms, .. }
                | PlayerEvent::PositionChanged { position_ms, .. }
                | PlayerEvent::PositionCorrection { position_ms, .. } => {
                    self.position_ms = u64::from(position_ms);
                }
                _ => {}
            }
        }
        self.ended.swap(false, Ordering::Relaxed)
    }
}

struct RodioBackend {
    _stream: OutputStream,
    sink: RodioSink,
    gain: Gain,
}

impl RodioBackend {
    fn open(gain: Gain) -> Result<Self, SinkError> {
        unsafe {
            std::env::set_var("PULSE_PROP_application.name", "Vinilo");
            std::env::set_var("PULSE_PROP_application.icon_name", vinilo_core::APP_ID);
            std::env::set_var("PULSE_PROP_media.role", "music");
        }
        let (stream, handle) = OutputStream::try_default()
            .map_err(|err| SinkError::ConnectionRefused(err.to_string()))?;
        let sink = RodioSink::try_new(&handle)
            .map_err(|err| SinkError::ConnectionRefused(err.to_string()))?;
        sink.set_volume(gain.get());
        sink.play();
        Ok(Self {
            _stream: stream,
            sink,
            gain,
        })
    }

    fn boxed(gain: Gain) -> Box<dyn Sink> {
        match Self::open(gain) {
            Ok(sink) => Box::new(sink),
            Err(err) => {
                tracing::error!(%err, "spotify: cannot open audio output");
                Box::new(Silence)
            }
        }
    }
}

impl Sink for RodioBackend {
    fn start(&mut self) -> SinkResult<()> {
        self.sink.set_volume(self.gain.get());
        self.sink.play();
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        self.sink.pause();
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        self.sink.set_volume(self.gain.get());
        let samples = packet
            .samples()
            .map_err(|err| SinkError::OnWrite(err.to_string()))?;
        let samples = converter.f64_to_f32(samples);
        self.sink.append(rodio::buffer::SamplesBuffer::new(
            NUM_CHANNELS as u16,
            SAMPLE_RATE,
            samples.to_vec(),
        ));
        while self.sink.len() > 32 {
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}

struct Silence;

impl Sink for Silence {
    fn write(&mut self, _packet: AudioPacket, _converter: &mut Converter) -> SinkResult<()> {
        Ok(())
    }
}

/// Strip `sp:` ids from a mixed queue so callers know whether librespot can own it.
pub fn all_spotify(hits: &[StreamHit]) -> bool {
    !hits.is_empty() && hits.iter().all(|hit| hit.id.starts_with("sp:"))
}
