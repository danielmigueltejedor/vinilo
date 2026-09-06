// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Listen Now / Discover shelves: recently played, recommendations, charts.
//!
//! Apple's own endpoints are tried first. Anything they leave empty is filled
//! from the library already on disk and from the local listen history, so the
//! page still has something to show on a storefront that 403s recommendations.

use serde::{Deserialize, Serialize};

use crate::entry::Entry;
use crate::music::types::{Album, Playlist, Track};

const VERSION: u32 = 2;
const SHELF: usize = 16;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Discover {
    #[serde(default)]
    version: u32,
    /// Which source wrote this cache, so a Spotify session does not open on
    /// Apple's last Listen Now page.
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub recently_played: Vec<Entry>,
    /// Mixes and albums Apple put in "Made for You". `Entry` rather than
    /// `Playlist` so an album recommendation is a tile, not a dropped row.
    #[serde(default)]
    pub recommended_playlists: Vec<Entry>,
    #[serde(default)]
    pub recommended_songs: Vec<Track>,
    #[serde(default)]
    pub recently_added: Vec<Entry>,
    #[serde(default)]
    pub charts: Vec<Entry>,
}

impl Discover {
    pub fn shelves(
        recently_played: Vec<Entry>,
        recommended_playlists: Vec<Entry>,
        recommended_songs: Vec<Track>,
        recently_added: Vec<Entry>,
        charts: Vec<Entry>,
    ) -> Self {
        Self {
            version: VERSION,
            provider: crate::provider::load()
                .unwrap_or_default()
                .as_str()
                .to_owned(),
            recently_played,
            recommended_playlists,
            recommended_songs,
            recently_added,
            charts,
        }
    }

    /// Drop songs (and tiles) whose title is just a YouTube video id, left in
    /// the on-disk cache before we stopped minting those rows.
    pub fn drop_placeholder_titles(&mut self) {
        self.recently_played.retain(|e| !entry_title_is_raw_id(e));
        self.recommended_playlists
            .retain(|e| !entry_title_is_raw_id(e));
        self.recommended_songs
            .retain(|t| !crate::streams::track_title_is_placeholder(t));
        self.recently_added.retain(|e| !entry_title_is_raw_id(e));
        self.charts.retain(|e| !entry_title_is_raw_id(e));
    }

    pub fn is_empty(&self) -> bool {
        self.recently_played.is_empty()
            && self.recommended_playlists.is_empty()
            && self.recommended_songs.is_empty()
            && self.recently_added.is_empty()
            && self.charts.is_empty()
    }

    /// Whether two pages would paint the same tiles. Artwork URLs can churn
    /// between a cache hit and a refresh; rebuilding for that is the flicker
    /// when you reopen Listen Now.
    pub fn same_tiles(&self, other: &Discover) -> bool {
        fn entries(a: &[Entry], b: &[Entry]) -> bool {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(left, right)| left.id() == right.id() && left.title() == right.title())
        }
        fn songs(a: &[Track], b: &[Track]) -> bool {
            a.len() == b.len()
                && a.iter().zip(b).all(|(left, right)| {
                    song_key(left) == song_key(right) && left.title == right.title
                })
        }
        self.provider == other.provider
            && entries(&self.recently_played, &other.recently_played)
            && entries(&self.recommended_playlists, &other.recommended_playlists)
            && songs(&self.recommended_songs, &other.recommended_songs)
            && entries(&self.recently_added, &other.recently_added)
            && entries(&self.charts, &other.charts)
    }

    /// Fill any shelf Apple left empty, without replacing one that already has
    /// editorial content.
    pub fn fill_gaps(&mut self, homemade: Discover) {
        if self.recently_played.is_empty() {
            self.recently_played = homemade.recently_played;
        }
        if self.recommended_playlists.is_empty() {
            self.recommended_playlists = homemade.recommended_playlists;
        }
        if self.recommended_songs.is_empty() {
            self.recommended_songs = homemade.recommended_songs;
        }
        if self.recently_added.is_empty() {
            self.recently_added = homemade.recently_added;
        }
        if self.charts.is_empty() {
            self.charts = homemade.charts;
        }
    }
}

fn song_key(track: &Track) -> &str {
    track.catalog_id.as_deref().unwrap_or(track.id.0.as_str())
}

fn entry_title_is_raw_id(entry: &Entry) -> bool {
    crate::streams::title_is_raw_id(entry.id(), entry.title())
}

fn cache_file() -> Option<std::path::PathBuf> {
    crate::paths::cache_file("discover")
}

fn rescue_foreign_legacy() {
    let Some(legacy) = crate::paths::legacy_cache_file("discover") else {
        return;
    };
    let Ok(raw) = std::fs::read_to_string(&legacy) else {
        return;
    };
    let Ok(cache) = serde_json::from_str::<Discover>(&raw) else {
        return;
    };
    if cache.is_empty() {
        return;
    }
    let Some(stem) = crate::provider::Provider::parse(&cache.provider)
        .and_then(|p| p.cache_stem())
        .or_else(|| infer_discover_stem(&cache))
    else {
        return;
    };
    let Some(dest) = crate::paths::cache_dir().map(|d| d.join(format!("discover-{stem}.json")))
    else {
        return;
    };
    if dest.exists() {
        return;
    }
    let _ = std::fs::rename(&legacy, dest);
}

pub fn load() -> Discover {
    rescue_foreign_legacy();
    let Some(path) = cache_file() else {
        return Discover::default();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Discover::default();
    };
    match serde_json::from_str::<Discover>(&raw) {
        Ok(mut cache) if cache.version == VERSION && fits_current_source(&cache) => {
            cache.drop_placeholder_titles();
            cache
        }
        _ => Discover::default(),
    }
}

fn infer_discover_stem(cache: &Discover) -> Option<&'static str> {
    for entry in cache
        .recently_played
        .iter()
        .chain(&cache.recommended_playlists)
        .chain(&cache.recently_added)
        .chain(&cache.charts)
    {
        let id = entry.id();
        if id.starts_with("sp:") {
            return Some("spotify");
        }
        if id.starts_with("yt:") {
            return Some("youtube-music");
        }
        if id.starts_with("td:") {
            return Some("tidal");
        }
    }
    cache.recommended_songs.iter().find_map(|t| {
        let id = t.catalog_id.as_deref().unwrap_or(t.id.0.as_str());
        if id.starts_with("sp:") {
            Some("spotify")
        } else if id.starts_with("yt:") {
            Some("youtube-music")
        } else if id.starts_with("td:") {
            Some("tidal")
        } else {
            None
        }
    })
}

fn fits_current_source(cache: &Discover) -> bool {
    let current = crate::provider::load().unwrap_or_default();
    let stamped = crate::provider::Provider::parse(&cache.provider);
    let streams = infer_discover_stem(cache).is_some();
    match current {
        crate::provider::Provider::AppleMusic => {
            stamped.is_none_or(|p| p == crate::provider::Provider::AppleMusic) && !streams
        }
        crate::provider::Provider::Local => false,
        other => stamped == Some(other) || (stamped.is_none() && streams),
    }
}

pub fn save(discover: &Discover) {
    if discover.is_empty() {
        return;
    }
    let Some(path) = cache_file() else { return };
    let Some(dir) = path.parent() else { return };
    let mut writing = discover.clone();
    writing.version = VERSION;
    if writing.provider.is_empty() {
        writing.provider = crate::provider::load()
            .unwrap_or_default()
            .as_str()
            .to_owned();
    }
    let Ok(json) = serde_json::to_string(&writing) else {
        return;
    };
    let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(path, json));
}

pub fn clear() {
    if let Some(path) = cache_file() {
        let _ = std::fs::remove_file(path);
    }
}

/// What we can put on Discover without asking Apple again.
///
/// Recently played comes from the local listen history. Recently added comes
/// from the library cache. "Made for You" and charts stay empty — dumping
/// the user's own playlists into Hecho para ti, or inventing a top-songs
/// list from one library, would be a lie. Favourite songs can fill Canciones
/// para ti when Apple sent none.
pub fn homemade(
    songs: &[Track],
    albums: &[Album],
    playlists: &[Playlist],
    history: &[Track],
) -> Discover {
    let mut recently_added: Vec<Entry> = albums
        .iter()
        .filter(|a| !a.date_added.is_empty())
        .cloned()
        .map(Entry::Album)
        .chain(
            playlists
                .iter()
                .filter(|p| !p.date_added.is_empty())
                .cloned()
                .map(Entry::Playlist),
        )
        .collect();
    recently_added.sort_by(|a, b| date_of(b).cmp(&date_of(a)));
    recently_added.truncate(SHELF);
    // YouTube Music (and some other catalogs) often omit date_added. Still
    // show the user's playlists on Discover so the page is not an empty
    // "Made for You". They stay out of recommended_playlists on purpose.
    if recently_added.is_empty() {
        recently_added = playlists
            .iter()
            .cloned()
            .map(Entry::Playlist)
            .chain(albums.iter().cloned().map(Entry::Album))
            .take(SHELF)
            .collect();
    }

    let mut recommended_songs: Vec<Track> = songs.iter().filter(|t| t.favorite).cloned().collect();
    if recommended_songs.is_empty() {
        recommended_songs = songs.iter().take(SHELF).cloned().collect();
    } else {
        recommended_songs.truncate(SHELF);
    }

    Discover::shelves(
        history
            .iter()
            .cloned()
            .map(Entry::Song)
            .take(SHELF)
            .collect(),
        Vec::new(),
        recommended_songs,
        recently_added,
        Vec::new(),
    )
}

fn date_of(entry: &Entry) -> &str {
    match entry {
        Entry::Album(a) => a.date_added.as_str(),
        Entry::Playlist(p) => p.date_added.as_str(),
        Entry::Song(t) => t.date_added.as_str(),
        Entry::Artist(_) => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::types::{Artwork, TrackId};

    fn song(title: &str, favorite: bool, added: &str) -> Track {
        Track {
            id: TrackId(format!("i.{title}")),
            catalog_id: Some("1".into()),
            favorite,
            in_library: true,
            library_id: None,
            date_added: added.into(),
            year: String::new(),
            title: title.into(),
            artist: "Aitana".into(),
            album: "Superestrella".into(),
            duration_ms: 200_000,
            track_number: 1,
            artwork: Some(Artwork::new("https://x/{w}x{h}bb.jpg")),
        }
    }

    #[test]
    fn homemade_prefers_favourites_for_recommended_songs() {
        let songs = vec![
            song("plain", false, "2024-01-01T00:00:00Z"),
            song("star", true, "2023-01-01T00:00:00Z"),
        ];
        let made = homemade(&songs, &[], &[], &[]);
        assert_eq!(made.recommended_songs.len(), 1);
        assert_eq!(made.recommended_songs[0].title, "star");
    }

    #[test]
    fn same_tiles_ignores_artwork_url_churn() {
        let mut a = Discover::shelves(
            vec![Entry::Song(song("played", false, ""))],
            Vec::new(),
            vec![song("star", true, "")],
            Vec::new(),
            Vec::new(),
        );
        let mut b = a.clone();
        if let Entry::Song(track) = &mut a.recently_played[0] {
            track.artwork = Some(Artwork::new("https://cdn/old/{w}x{h}.jpg"));
        }
        if let Entry::Song(track) = &mut b.recently_played[0] {
            track.artwork = Some(Artwork::new("https://cdn/new/{w}x{h}.jpg"));
        }
        assert!(a.same_tiles(&b));
        b.recommended_songs[0].title = "other".into();
        assert!(!a.same_tiles(&b));
    }

    #[test]
    fn fill_gaps_does_not_replace_apple_content() {
        let mut apple = Discover {
            recommended_songs: vec![song("from-apple", false, "")],
            ..Discover::default()
        };
        apple.fill_gaps(Discover {
            recommended_songs: vec![song("homemade", true, "")],
            recently_played: vec![Entry::Song(song("played", false, ""))],
            ..Discover::default()
        });
        assert_eq!(apple.recommended_songs[0].title, "from-apple");
        assert_eq!(apple.recently_played.len(), 1);
    }

    fn playlist(name: &str) -> Playlist {
        Playlist {
            id: format!("p.{name}"),
            date_added: "2024-01-01T00:00:00Z".into(),
            last_modified: "2024-06-01T00:00:00Z".into(),
            name: name.into(),
            curator: String::new(),
            description: String::new(),
            artwork: None,
            library: true,
        }
    }

    #[test]
    fn homemade_does_not_pass_library_playlists_off_as_made_for_you() {
        let made = homemade(&[], &[], &[playlist("Gym"), playlist("Drive")], &[]);
        assert!(made.recommended_playlists.is_empty());
        assert!(made.charts.is_empty());
    }

    fn playlist_undated(name: &str) -> Playlist {
        Playlist {
            date_added: String::new(),
            ..playlist(name)
        }
    }

    #[test]
    fn homemade_lists_undated_playlists_when_nothing_is_dated() {
        let made = homemade(
            &[],
            &[],
            &[playlist_undated("Gym"), playlist_undated("Drive")],
            &[],
        );
        assert_eq!(made.recently_added.len(), 2);
        assert!(made.recommended_playlists.is_empty());
    }
}
