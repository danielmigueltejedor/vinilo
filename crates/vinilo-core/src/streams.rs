// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Catalog search for Spotify, YouTube Music and Tidal.
//!
//! These services do not expose a Linux-legal full-quality stream the way
//! MusicKit does. Search talks to each catalogue; playback is a separate
//! `yt-dlp` fetch in the daemon (YouTube audio of the same recording). The
//! ids we mint (`yt:`, `sp:`, `td:`) keep that split honest: MusicKit never
//! sees them.

use anyhow::{Context, Result};
use serde_json::Value;

use crate::entry::Entry;
use crate::music::types::{Artwork, Track, TrackId};

const WEB: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";

/// A hit from a non-Apple catalogue, ready to become a row and later a file.
#[derive(Debug, Clone)]
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
}

pub fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(WEB)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
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
        .unwrap_or("Unknown")
        .to_owned();
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
        .and_then(|thumbs| thumbs.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .map(str::to_owned);
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
    let token = spotify_token(http).await?;
    let url = format!(
        "https://api.spotify.com/v1/search?q={}&type=track&limit=20",
        urlencoding(query)
    );
    let res = http
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .context("spotify search")?;
    if !res.status().is_success() {
        anyhow::bail!("Spotify search {}", res.status());
    }
    let value: Value = res.json().await.context("spotify search json")?;
    Ok(spotify_tracks(&value))
}

fn spotify_tracks(value: &Value) -> Vec<StreamHit> {
    let Some(items) = value
        .pointer("/tracks/items")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_owned();
            let title = item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Unknown")
                .to_owned();
            let artist = item
                .pointer("/artists/0/name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let album = item
                .pointer("/album/name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let duration_ms = item
                .get("duration_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let artwork = item
                .pointer("/album/images/0/url")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Some(StreamHit {
                play_query: format!("{artist} {title}"),
                id: format!("sp:{id}"),
                title,
                artist,
                album,
                duration_ms,
                artwork,
            })
        })
        .collect()
}

async fn spotify_token(http: &reqwest::Client) -> Result<String> {
    let res = http
        .get("https://open.spotify.com/get_access_token?reason=transport&productType=web_player")
        .header("Accept", "application/json")
        .send()
        .await
        .context("spotify token")?;
    if !res.status().is_success() {
        anyhow::bail!("Spotify token {}", res.status());
    }
    let value: Value = res.json().await.context("spotify token json")?;
    value
        .get("accessToken")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .context("Spotify token missing accessToken")
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
            let id = track
                .get("id")
                .and_then(|v| v.as_u64().map(|n| n.to_string()).or_else(|| v.as_str().map(str::to_owned)))?;
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
                play_query: format!("{artist} {title}"),
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

fn urlencoding(s: &str) -> String {
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
        let Entry::Song(track) = hit.into_entry() else {
            panic!("song");
        };
        assert!(track.playable());
        assert_eq!(track.title, "Never Gonna Give You Up");
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
        let hits = spotify_tracks(&json);
        assert_eq!(hits[0].id, "sp:abc");
        assert_eq!(hits[0].artist, "Aitana");
        assert_eq!(hits[0].play_query, "Aitana Pa Mal");
    }
}
