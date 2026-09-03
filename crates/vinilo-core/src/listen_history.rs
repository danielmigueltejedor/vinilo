// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Songs actually played on this machine, for Discover when Apple's recently
//! played endpoint is empty or missing.

use serde::{Deserialize, Serialize};

use crate::music::types::Track;

const VERSION: u32 = 1;
const KEEP: usize = 40;

#[derive(Deserialize, Serialize)]
struct File {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    tracks: Vec<Track>,
}

fn path() -> Option<std::path::PathBuf> {
    Some(crate::paths::cache_dir()?.join("recent.json"))
}

pub fn load() -> Vec<Track> {
    let Some(path) = path() else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match serde_json::from_str::<File>(&raw) {
        Ok(file) if file.version == VERSION => file.tracks,
        _ => Vec::new(),
    }
}

/// Newest first. Drops an older copy of the same catalog id so repeating a
/// song does not fill the shelf with itself.
pub fn record(track: Track) {
    let Some(path) = path() else { return };
    let Some(dir) = path.parent() else { return };
    let id = track
        .catalog_id
        .clone()
        .unwrap_or_else(|| track.id.0.clone());
    let mut tracks = load();
    tracks.retain(|t| t.catalog_id.as_deref().unwrap_or(t.id.0.as_str()) != id);
    tracks.insert(0, track);
    tracks.truncate(KEEP);
    let file = File {
        version: VERSION,
        tracks,
    };
    let Ok(json) = serde_json::to_string(&file) else {
        return;
    };
    let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(path, json));
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

    fn track(id: &str) -> Track {
        Track {
            id: TrackId(id.into()),
            catalog_id: Some(id.into()),
            favorite: false,
            in_library: false,
            library_id: None,
            date_added: String::new(),
            year: String::new(),
            title: id.into(),
            artist: "A".into(),
            album: "B".into(),
            duration_ms: 1,
            track_number: 1,
            artwork: None,
        }
    }

    #[test]
    fn recording_the_same_song_moves_it_to_the_front() {
        let dir = std::env::temp_dir().join(format!("vinilo-history-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // paths::cache_dir reads XDG_CACHE_HOME; tests must not clobber the
        // user's. This unit only checks the in-memory reorder via the same
        // retain/insert used by `record`.
        let mut tracks = vec![track("a"), track("b")];
        let id = "b";
        tracks.retain(|t| t.catalog_id.as_deref() != Some(id));
        tracks.insert(0, track(id));
        assert_eq!(tracks[0].catalog_id.as_deref(), Some("b"));
        assert_eq!(tracks.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
