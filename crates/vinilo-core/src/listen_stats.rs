// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Local listening totals: play counts and time, persisted under the cache dir.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::music::types::Track;

const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackStat {
    pub id: String,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub year: String,
    /// Artwork template or https URL when we have one.
    #[serde(default)]
    pub artwork: Option<String>,
    /// Provider label derived from the id (`yt:`, `sp:`, …) or "Apple Music" / "This computer".
    #[serde(default)]
    pub source: String,
    /// Public play/view count from the catalogue when the API exposes one.
    #[serde(default)]
    pub public_plays: Option<u64>,
    pub play_count: u64,
    pub total_ms: u64,
    pub last_played: u64,
}

#[derive(Deserialize, Serialize, Default)]
struct File {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    tracks: HashMap<String, TrackStat>,
    #[serde(default)]
    total_ms: u64,
}

fn path() -> Option<std::path::PathBuf> {
    crate::paths::cache_file("listen-stats")
}

fn load_file() -> File {
    let Some(path) = path() else {
        return File::default();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return File::default();
    };
    match serde_json::from_str::<File>(&raw) {
        Ok(file) if file.version == VERSION => file,
        // Older shapes still parse via serde defaults on new fields.
        Ok(mut file) => {
            file.version = VERSION;
            file
        }
        _ => File::default(),
    }
}

fn save_file(file: &File) {
    let Some(path) = path() else {
        return;
    };
    let Some(dir) = path.parent() else {
        return;
    };
    let Ok(json) = serde_json::to_string(file) else {
        return;
    };
    let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(path, json));
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn source_label(id: &str) -> String {
    if id.starts_with("yt:") {
        "YouTube Music".into()
    } else if id.starts_with("sp:") {
        "Spotify".into()
    } else if id.starts_with("td:") {
        "Tidal".into()
    } else if id.starts_with('/') || id.contains(".mp3") || id.contains(".flac") {
        "This computer".into()
    } else {
        "Apple Music".into()
    }
}

/// Compact public counts: 950, 12.4K, 1.2M, 2.1B.
pub fn format_count(n: u64) -> String {
    const K: f64 = 1_000.0;
    const M: f64 = 1_000_000.0;
    const B: f64 = 1_000_000_000.0;
    let x = n as f64;
    if x < K {
        format!("{n}")
    } else if x < M {
        let v = x / K;
        if v >= 100.0 {
            format!("{v:.0}K")
        } else {
            format!("{v:.1}K")
        }
    } else if x < B {
        let v = x / M;
        if v >= 100.0 {
            format!("{v:.0}M")
        } else {
            format!("{v:.1}M")
        }
    } else {
        let v = x / B;
        if v >= 100.0 {
            format!("{v:.0}B")
        } else {
            format!("{v:.1}B")
        }
    }
}

/// Record one listen. `heard_ms` is how long this play counted (track length
/// when we only know the song changed).
pub fn record(track: &Track, heard_ms: u64) {
    record_with_public(track, heard_ms, None);
}

pub fn record_with_public(track: &Track, heard_ms: u64, public_plays: Option<u64>) {
    let id = track
        .catalog_id
        .clone()
        .unwrap_or_else(|| track.id.0.clone());
    if id.is_empty() {
        return;
    }
    let mut file = load_file();
    file.version = VERSION;
    let entry = file.tracks.entry(id.clone()).or_insert_with(|| TrackStat {
        id: id.clone(),
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        year: track.year.clone(),
        artwork: track.artwork.as_ref().map(|a| a.url(300)),
        source: source_label(&id),
        public_plays: None,
        play_count: 0,
        total_ms: 0,
        last_played: 0,
    });
    entry.title = track.title.clone();
    entry.artist = track.artist.clone();
    if !track.album.is_empty() {
        entry.album = track.album.clone();
    }
    if !track.year.is_empty() {
        entry.year = track.year.clone();
    }
    if let Some(art) = track.artwork.as_ref().map(|a| a.url(300)) {
        entry.artwork = Some(art);
    }
    entry.source = source_label(&id);
    if public_plays.is_some() {
        entry.public_plays = public_plays;
    }
    entry.play_count = entry.play_count.saturating_add(1);
    entry.total_ms = entry.total_ms.saturating_add(heard_ms);
    entry.last_played = now_secs();
    file.total_ms = file.total_ms.saturating_add(heard_ms);
    save_file(&file);
}

pub fn total_ms() -> u64 {
    load_file().total_ms
}

pub fn top_tracks(limit: usize) -> Vec<TrackStat> {
    let mut tracks: Vec<_> = load_file().tracks.into_values().collect();
    tracks.sort_by_key(|t| {
        (
            std::cmp::Reverse(t.play_count),
            std::cmp::Reverse(t.total_ms),
        )
    });
    tracks.truncate(limit);
    tracks
}

/// Prefer a local cover path once artwork has been cached for the bar.
pub fn set_artwork_file(id: &str, file: &std::path::Path) {
    if id.is_empty() {
        return;
    }
    let mut file_store = load_file();
    let Some(entry) = file_store.tracks.get_mut(id) else {
        return;
    };
    entry.artwork = Some(file.display().to_string());
    save_file(&file_store);
}

/// Attach a catalogue play/view count once a background hydrate finds one.
pub fn set_public_plays(id: &str, plays: u64) {
    if id.is_empty() || plays == 0 {
        return;
    }
    let mut file = load_file();
    let Some(entry) = file.tracks.get_mut(id) else {
        return;
    };
    entry.public_plays = Some(plays);
    save_file(&file);
}

pub fn clear() {
    if let Some(path) = path() {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::types::TrackId;

    #[test]
    fn top_orders_by_play_count() {
        let mut file = File {
            version: VERSION,
            tracks: HashMap::new(),
            total_ms: 0,
        };
        file.tracks.insert(
            "a".into(),
            TrackStat {
                id: "a".into(),
                title: "A".into(),
                artist: "x".into(),
                album: String::new(),
                year: String::new(),
                artwork: None,
                source: String::new(),
                public_plays: None,
                play_count: 1,
                total_ms: 100,
                last_played: 1,
            },
        );
        file.tracks.insert(
            "b".into(),
            TrackStat {
                id: "b".into(),
                title: "B".into(),
                artist: "x".into(),
                album: String::new(),
                year: String::new(),
                artwork: None,
                source: String::new(),
                public_plays: None,
                play_count: 5,
                total_ms: 50,
                last_played: 2,
            },
        );
        let mut tracks: Vec<_> = file.tracks.into_values().collect();
        tracks.sort_by_key(|t| std::cmp::Reverse(t.play_count));
        assert_eq!(tracks[0].id, "b");
        let _ = TrackId("x".into());
    }

    #[test]
    fn compact_counts() {
        assert_eq!(format_count(950), "950");
        assert_eq!(format_count(12_400), "12.4K");
        assert_eq!(format_count(1_200_000), "1.2M");
        assert_eq!(format_count(2_100_000_000), "2.1B");
    }
}
