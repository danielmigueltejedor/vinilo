// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Catalog search for Spotify, YouTube Music and Tidal.
//!
//! These services do not expose a Linux-legal full-quality stream the way
//! MusicKit does. Search talks to each catalogue; playback is a separate
//! `yt-dlp` fetch in the daemon (YouTube audio of the same recording). The
//! ids we mint (`yt:`, `sp:`, `td:`) keep that split honest: MusicKit never
//! sees them.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entry::Entry;
use crate::music::types::{Artwork, Track, TrackId};

const WEB: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";

/// A hit from a non-Apple catalogue, ready to become a row and later a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamHit {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub artwork: Option<String>,
    /// What `yt-dlp` should type to get audio when the id is not a YouTube id.
    pub play_query: String,
}

impl StreamHit {
    pub fn into_entry(self) -> Entry {
        Entry::Song(Track {
            date_added: String::new(),
            year: String::new(),
            favorite: false,
            in_library: false,
            library_id: None,
            id: TrackId(self.id.clone()),
            catalog_id: Some(self.id),
            title: self.title,
            artist: self.artist,
            album: self.album,
            duration_ms: self.duration_ms,
            track_number: 0,
            artwork: self.artwork.map(Artwork::new),
        })
    }

    pub fn is_stream_id(id: &str) -> bool {
        id.starts_with("yt:") || id.starts_with("sp:") || id.starts_with("td:")
    }

    /// What to type into yt-dlp if the service URL itself cannot be extracted.
    pub fn youtube_search_spec(&self) -> String {
        let q = format!("{} {}", self.artist, self.title);
        let q = q.trim();
        if q.is_empty() {
            format!("ytsearch1:{}", self.play_query)
        } else {
            format!("ytsearch1:{q}")
        }
    }

    /// Rebuild a playable hit from a library or playlist song we already drew.
    pub fn from_song(track: &Track) -> Option<Self> {
        let id = track.catalog_id.clone()?;
        if !Self::is_stream_id(&id) {
            return None;
        }
        let play_query = if let Some(sp) = id.strip_prefix("sp:") {
            if sp.contains(':') {
                return None;
            }
            format!("https://open.spotify.com/track/{sp}")
        } else if let Some(yt) = id.strip_prefix("yt:") {
            format!("https://www.youtube.com/watch?v={yt}")
        } else if let Some(td) = id.strip_prefix("td:") {
            format!("https://tidal.com/browse/track/{td}")
        } else {
            return None;
        };
        Some(Self {
            play_query,
            artwork: track.artwork.as_ref().map(|art| art.url(300)),
            duration_ms: track.duration_ms,
            album: track.album.clone(),
            artist: track.artist.clone(),
            title: track.title.clone(),
            id,
        })
    }

    /// True when the title is just the raw id (a reconstructed hit after restart).
    pub fn title_is_placeholder(&self) -> bool {
        title_is_raw_id(&self.id, &self.title)
    }
}

/// Empty titles, or ones that are only the catalogue id / video id.
pub fn title_is_raw_id(id: &str, title: &str) -> bool {
    let title = title.trim();
    if title.is_empty() {
        return true;
    }
    let rest = id.split_once(':').map(|(_, rest)| rest).unwrap_or(id);
    title == rest || title == id
}

pub fn track_title_is_placeholder(track: &Track) -> bool {
    let id = track.catalog_id.as_deref().unwrap_or(track.id.0.as_str());
    title_is_raw_id(id, &track.title)
}

pub fn youtube_thumb(video: &str) -> String {
    format!("https://i.ytimg.com/vi/{video}/hqdefault.jpg")
}

/// Sidecar JSON next to a downloaded file, so the now-playing bar keeps the
/// catalogue title after a restart instead of the `yt_…` / `sp_…` filename.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Option<String>,
    #[serde(default)]
    pub duration_ms: u64,
}

impl FileMeta {
    pub fn from_hit(hit: &StreamHit) -> Self {
        Self {
            id: hit.id.clone(),
            title: hit.title.clone(),
            artist: hit.artist.clone(),
            album: hit.album.clone(),
            artwork: hit.artwork.clone(),
            duration_ms: hit.duration_ms,
        }
    }
}

fn sidecar_path(audio: &std::path::Path) -> Option<std::path::PathBuf> {
    let stem = audio.file_stem()?.to_str()?;
    Some(audio.parent()?.join(format!("{stem}.json")))
}

pub fn write_sidecar(audio: &std::path::Path, hit: &StreamHit) {
    if hit.title_is_placeholder() {
        return;
    }
    let Some(path) = sidecar_path(audio) else {
        return;
    };
    let Ok(json) = serde_json::to_string(&FileMeta::from_hit(hit)) else {
        return;
    };
    let _ = std::fs::write(path, json);
}

pub fn read_sidecar(audio: &std::path::Path) -> Option<FileMeta> {
    let path = sidecar_path(audio)?;
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn hits_cache_path() -> Option<std::path::PathBuf> {
    Some(crate::paths::cache_dir()?.join("stream-hits.json"))
}

/// Hits remembered so Play after a daemon restart still has titles.
pub fn load_hits() -> std::collections::HashMap<String, StreamHit> {
    let Some(path) = hits_cache_path() else {
        return std::collections::HashMap::new();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return std::collections::HashMap::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_hits(hits: &std::collections::HashMap<String, StreamHit>) {
    let Some(path) = hits_cache_path() else {
        return;
    };
    let Some(dir) = path.parent() else {
        return;
    };
    let clipped: std::collections::HashMap<String, StreamHit> = hits
        .iter()
        .take(2_000)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let Ok(json) = serde_json::to_string(&clipped) else {
        return;
    };
    let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(path, json));
}

pub fn http() -> reqwest::Client {
    http_with_timeout(Duration::from_secs(12))
}

/// Longer budget for a first library load: TOTP + client-token + several
/// Pathfinder pages often exceed the search timeout.
pub fn http_long() -> reqwest::Client {
    http_with_timeout(Duration::from_secs(25))
}

fn http_with_timeout(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(WEB)
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_else(|_| {
            reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
}

/// Parse one `yt-dlp -j --flat-playlist` line.
pub fn youtube_hit_from_json(value: &Value) -> Option<StreamHit> {
    let id = value.get("id")?.as_str()?.to_owned();
    if id.is_empty() {
        return None;
    }
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    let title = if title.is_empty() || title == id || title == "NA" {
        String::new()
    } else {
        title
    };
    let artist = value
        .get("uploader")
        .or_else(|| value.get("channel"))
        .or_else(|| value.get("artist"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let duration_ms = value
        .get("duration")
        .and_then(Value::as_f64)
        .map(|s| (s * 1000.0) as u64)
        .unwrap_or(0);
    let artwork = value
        .get("thumbnails")
        .and_then(Value::as_array)
        .and_then(|thumbs| {
            thumbs
                .iter()
                .max_by_key(|t| t.get("width").and_then(Value::as_u64).unwrap_or(0))
        })
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| Some(youtube_thumb(&id)));
    Some(StreamHit {
        play_query: format!("https://www.youtube.com/watch?v={id}"),
        id: format!("yt:{id}"),
        title,
        artist,
        album: String::new(),
        duration_ms,
        artwork,
    })
}

pub async fn search_spotify(http: &reqwest::Client, query: &str) -> Result<Vec<StreamHit>> {
    Ok(crate::spotify::search(http, query).await?.hits)
}

pub async fn search_tidal(http: &reqwest::Client, query: &str) -> Result<Vec<StreamHit>> {
    let url = format!(
        "https://listen.tidal.com/v1/search/top-hits?query={}&limit=20&offset=0&types=TRACKS&countryCode=US",
        urlencoding(query)
    );
    let res = http.get(url).send().await.context("tidal search")?;
    if !res.status().is_success() {
        anyhow::bail!("Tidal search {}", res.status());
    }
    let value: Value = res.json().await.context("tidal search json")?;
    let hits = tidal_tracks(&value);
    if hits.is_empty() {
        anyhow::bail!("Tidal search returned no tracks");
    }
    Ok(hits)
}

fn tidal_tracks(value: &Value) -> Vec<StreamHit> {
    let items = value
        .pointer("/tracks/items")
        .or_else(|| value.pointer("/items"))
        .and_then(Value::as_array);
    let Some(items) = items else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let track = item.get("resource").unwrap_or(item);
            let id = track.get("id").and_then(|v| {
                v.as_u64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_str().map(str::to_owned))
            })?;
            let title = track
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Unknown")
                .to_owned();
            let artist = track
                .pointer("/artist/name")
                .or_else(|| track.pointer("/artists/0/name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let album = track
                .pointer("/album/title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let duration_ms = track
                .get("duration")
                .and_then(Value::as_u64)
                .map(|s| s * 1000)
                .unwrap_or(0);
            let artwork = track
                .pointer("/album/cover")
                .and_then(Value::as_str)
                .map(|uuid| {
                    format!(
                        "https://resources.tidal.com/images/{}/320x320.jpg",
                        uuid.replace('-', "/")
                    )
                });
            Some(StreamHit {
                play_query: format!("https://tidal.com/browse/track/{id}"),
                id: format!("td:{id}"),
                title,
                artist,
                album,
                duration_ms,
                artwork,
            })
        })
        .collect()
}

pub(crate) fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_yt_dlp_flat_line_becomes_a_playable_hit() {
        let json = serde_json::json!({
            "id": "dQw4w9WgXcQ",
            "title": "Never Gonna Give You Up",
            "channel": "Rick Astley",
            "duration": 213.0,
            "thumbnails": [{"url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/default.jpg"}]
        });
        let hit = youtube_hit_from_json(&json).unwrap();
        assert_eq!(hit.id, "yt:dQw4w9WgXcQ");
        assert!(StreamHit::is_stream_id(&hit.id));
        assert!(!hit.title_is_placeholder());
        let Entry::Song(track) = hit.into_entry() else {
            panic!("song");
        };
        assert!(track.playable());
        assert_eq!(track.title, "Never Gonna Give You Up");
    }

    #[test]
    fn a_reconstructed_id_title_is_a_placeholder() {
        assert!(title_is_raw_id("yt:vrY1THC_NQE", "vrY1THC_NQE"));
        assert!(title_is_raw_id("yt:vrY1THC_NQE", ""));
        assert!(!title_is_raw_id("yt:vrY1THC_NQE", "Pa Mal"));
    }

    #[test]
    fn a_flat_line_without_a_title_is_a_placeholder() {
        let json = serde_json::json!({ "id": "vrY1THC_NQE" });
        let hit = youtube_hit_from_json(&json).unwrap();
        assert!(hit.title_is_placeholder());
        assert!(
            hit.artwork
                .as_deref()
                .is_some_and(|u| u.contains("vrY1THC_NQE"))
        );
    }

    #[test]
    fn spotify_search_json_keeps_artist_and_cover() {
        let json = serde_json::json!({
            "tracks": {"items": [{
                "id": "abc",
                "name": "Pa Mal",
                "duration_ms": 180000,
                "artists": [{"name": "Aitana"}],
                "album": {
                    "name": "Alpha",
                    "images": [{"url": "https://i.scdn.co/image/x"}]
                }
            }]}
        });
        let hits = crate::spotify::tracks_from_search(&json);
        assert_eq!(hits[0].id, "sp:abc");
        assert_eq!(hits[0].artist, "Aitana");
        assert_eq!(hits[0].play_query, "https://open.spotify.com/track/abc");
        assert_eq!(hits[0].youtube_search_spec(), "ytsearch1:Aitana Pa Mal");
        assert!(!hits[0].title_is_placeholder());
    }
}
