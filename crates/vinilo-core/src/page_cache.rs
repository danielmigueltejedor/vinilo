// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tracks inside a playlist, album or artist page, kept between visits.
//!
//! Opening a playlist used to hit Apple every time — details plus a paginated
//! track walk, then GTK rebuilt every row. The library cache only stores the
//! playlist *tile*. This stores the page itself, so going back in is a disk
//! read, with a background revalidation behind it.

use serde::{Deserialize, Serialize};

use crate::entry::Entry;
use crate::ipc::PageKind;

const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPage {
    #[serde(default)]
    version: u32,
    pub header: Entry,
    pub entries: Vec<Entry>,
}

fn dir() -> Option<std::path::PathBuf> {
    Some(crate::paths::cache_dir()?.join("pages"))
}

fn file_name(kind: PageKind, id: &str) -> String {
    let kind = match kind {
        PageKind::Album => "album",
        PageKind::Artist => "artist",
        PageKind::Playlist => "playlist",
        PageKind::LibraryAlbum => "library-album",
        PageKind::LibraryArtist => "library-artist",
        PageKind::LibraryPlaylist => "library-playlist",
    };
    let id: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    format!("{kind}-{id}.json")
}

fn path(kind: PageKind, id: &str) -> Option<std::path::PathBuf> {
    Some(dir()?.join(file_name(kind, id)))
}

pub fn load(kind: PageKind, id: &str) -> Option<CachedPage> {
    let raw = std::fs::read_to_string(path(kind, id)?).ok()?;
    let page: CachedPage = serde_json::from_str(&raw).ok()?;
    (page.version == VERSION).then_some(page)
}

pub fn save(kind: PageKind, id: &str, header: &Entry, entries: &[Entry]) {
    let Some(path) = path(kind, id) else { return };
    let Some(dir) = path.parent() else { return };
    let page = CachedPage {
        version: VERSION,
        header: header.clone(),
        entries: entries.to_vec(),
    };
    let Ok(json) = serde_json::to_string(&page) else {
        return;
    };
    if let Err(err) = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(path, json)) {
        tracing::debug!(?err, "could not cache the page");
    }
}

pub fn forget(kind: PageKind, id: &str) {
    if let Some(path) = path(kind, id) {
        let _ = std::fs::remove_file(path);
    }
}

pub fn clear() {
    let Some(dir) = dir() else { return };
    let _ = std::fs::remove_dir_all(dir);
}

/// Same music, same order — used to skip a redraw when revalidation agrees.
pub fn same(cached: &CachedPage, header: &Entry, entries: &[Entry]) -> bool {
    cached.header.id() == header.id()
        && cached.header.title() == header.title()
        && art_key(&cached.header) == art_key(header)
        && cached.entries.len() == entries.len()
        && cached
            .entries
            .iter()
            .zip(entries)
            .all(|(a, b)| a.id() == b.id() && a.title() == b.title())
}

fn art_key(entry: &Entry) -> Option<String> {
    match entry {
        Entry::Song(t) => t.artwork.as_ref().map(|a| a.cache_key()),
        Entry::Album(a) => a.artwork.as_ref().map(|a| a.cache_key()),
        Entry::Artist(a) => a.artwork.as_ref().map(|a| a.cache_key()),
        Entry::Playlist(p) => p.artwork.as_ref().map(|a| a.cache_key()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::types::{Playlist, Track, TrackId};

    fn playlist() -> Entry {
        Entry::Playlist(Playlist {
            id: "p.abc".into(),
            date_added: String::new(),
            last_modified: String::new(),
            name: "Game on".into(),
            curator: String::new(),
            description: String::new(),
            artwork: None,
            library: true,
        })
    }

    fn song(title: &str) -> Entry {
        Entry::Song(Track {
            id: TrackId(title.into()),
            catalog_id: Some(title.into()),
            favorite: false,
            in_library: true,
            library_id: None,
            date_added: String::new(),
            year: String::new(),
            title: title.into(),
            artist: "A".into(),
            album: "B".into(),
            duration_ms: 1,
            track_number: 1,
            artwork: None,
        })
    }

    #[test]
    fn same_page_matches_on_ids() {
        let header = playlist();
        let entries = vec![song("one"), song("two")];
        let cached = CachedPage {
            version: VERSION,
            header: header.clone(),
            entries: entries.clone(),
        };
        assert!(same(&cached, &header, &entries));
        assert!(!same(&cached, &header, &[song("one")]));
    }

    #[test]
    fn playlist_ids_cannot_escape_the_cache_directory() {
        let name = file_name(PageKind::LibraryPlaylist, "p.a/b?c");
        assert!(!name.contains('/'));
        assert!(!name.contains('?'));
        assert!(name.starts_with("library-playlist-"));
        assert!(name.ends_with(".json"));
    }
}
