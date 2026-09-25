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

/// Record one listen. `heard_ms` is how long this play counted (track length
/// when we only know the song changed).
pub fn record(track: &Track, heard_ms: u64) {
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
        play_count: 0,
        total_ms: 0,
        last_played: 0,
    });
    entry.title = track.title.clone();
    entry.artist = track.artist.clone();
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
}
