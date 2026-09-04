// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mixed Apple Music JSON: recently played, recommendations, charts.
//!
//! Those endpoints return several resource types in one array. The rest of the
//! client parses one kind at a time; this is the one place a `type` field
//! decides which of our types to build. Recommendation `contents` are often
//! `{ id, type }` stubs — `stub_ids` collects them so the client can hydrate
//! them from the catalog instead of dropping the whole shelf.

use serde::Deserialize;

use crate::entry::Entry;
use crate::music::types::{
    Album, AlbumAttributes, Artist, ArtistAttributes, Playlist, PlaylistAttributes, Resource,
    SongAttributes, Track,
};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TypedResource {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub attributes: Option<serde_json::Value>,
    #[serde(default)]
    pub relationships: Option<serde_json::Value>,
}

impl TypedResource {
    pub(crate) fn into_entry(self) -> Option<Entry> {
        let attributes = self.attributes?;
        match self.kind.as_str() {
            "songs" | "library-songs" => {
                let attrs: SongAttributes = serde_json::from_value(attributes).ok()?;
                let library = self.kind.starts_with("library-");
                let mut track = Track::from(Resource {
                    id: self.id.clone(),
                    attributes: Some(attrs),
                    relationships: None,
                });
                if library {
                    track.in_library = true;
                    if track.library_id.is_none() {
                        track.library_id = Some(self.id);
                    }
                }
                Some(Entry::Song(track))
            }
            "albums" | "library-albums" => {
                let attrs: AlbumAttributes = serde_json::from_value(attributes).ok()?;
                let library = self.kind.starts_with("library-");
                let mut album = Album::from(Resource {
                    id: self.id,
                    attributes: Some(attrs),
                    relationships: None,
                });
                album.library = library;
                Some(Entry::Album(album))
            }
            "playlists" | "library-playlists" => {
                let attrs: PlaylistAttributes = serde_json::from_value(attributes).ok()?;
                let library = self.kind.starts_with("library-");
                let mut playlist = Playlist::from(Resource {
                    id: self.id,
                    attributes: Some(attrs),
                    relationships: None,
                });
                playlist.library = library;
                Some(Entry::Playlist(playlist))
            }
            "artists" | "library-artists" => {
                let attrs: ArtistAttributes = serde_json::from_value(attributes).ok()?;
                let library = self.kind.starts_with("library-");
                let mut artist = Artist::from(Resource {
                    id: self.id,
                    attributes: Some(attrs),
                    relationships: None,
                });
                artist.library = library;
                Some(Entry::Artist(artist))
            }
            _ => None,
        }
    }
}

pub(crate) fn entries_from_list(list: Vec<TypedResource>) -> Vec<Entry> {
    list.into_iter().filter_map(TypedResource::into_entry).collect()
}

/// Ids Apple sent without attributes, grouped by catalog collection.
///
/// Recommendation `contents` often arrives as `{ id, type }` stubs. Those
/// cannot become a tile until we fetch the real resource.
pub(crate) fn stub_ids(list: &[TypedResource]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut songs = Vec::new();
    let mut albums = Vec::new();
    let mut playlists = Vec::new();
    for resource in list {
        if resource.attributes.is_some() {
            continue;
        }
        match resource.kind.as_str() {
            "songs" | "library-songs" => songs.push(resource.id.clone()),
            "albums" | "library-albums" => albums.push(resource.id.clone()),
            "playlists" | "library-playlists" => playlists.push(resource.id.clone()),
            _ => {}
        }
    }
    (songs, albums, playlists)
}

/// `relationships.contents.data` on a personal-recommendation resource.
pub(crate) fn contents_resources(resource: &TypedResource) -> Vec<TypedResource> {
    let Some(relationships) = &resource.relationships else {
        return Vec::new();
    };
    let Some(contents) = relationships.get("contents") else {
        return Vec::new();
    };
    let Some(data) = contents.get("data") else {
        return Vec::new();
    };
    serde_json::from_value::<Vec<TypedResource>>(data.clone()).unwrap_or_default()
}

pub(crate) fn contents_of(resource: &TypedResource) -> Vec<Entry> {
    entries_from_list(contents_resources(resource))
}

/// Apple's `next` on a relationship, stripped to a path `Client::get` can use.
pub(crate) fn next_path(resource: &TypedResource) -> Option<String> {
    let href = resource
        .relationships
        .as_ref()?
        .get("contents")?
        .get("next")?
        .as_str()?;
    Some(api_path(href))
}

pub(crate) fn api_path(href: &str) -> String {
    href.strip_prefix("https://api.music.apple.com/v1")
        .or_else(|| href.strip_prefix("/v1"))
        .unwrap_or(href)
        .to_owned()
}

/// `relationships.contents.href`, stripped to a path `Client::get` can use.
pub(crate) fn contents_href(resource: &TypedResource) -> Option<String> {
    let href = resource
        .relationships
        .as_ref()?
        .get("contents")?
        .get("href")?
        .as_str()?;
    Some(api_path(href))
}

/// Charts live under `results.{songs,albums,playlists}`, each either a list of
/// chart objects with a `data` array, or (rarely) one object. Treating only
/// arrays as valid is how an otherwise-good charts response became an empty
/// Éxitos shelf.
pub(crate) fn chart_resources(results: &serde_json::Value, key: &str) -> Vec<TypedResource> {
    let Some(node) = results.get(key) else {
        return Vec::new();
    };
    let charts: Vec<&serde_json::Value> = if let Some(array) = node.as_array() {
        array.iter().collect()
    } else {
        vec![node]
    };
    let mut out = Vec::new();
    for chart in charts {
        let Some(data) = chart.get("data") else {
            continue;
        };
        if let Ok(list) = serde_json::from_value::<Vec<TypedResource>>(data.clone()) {
            out.extend(list);
        }
    }
    out
}

/// Library resources use `i.…` / `l.…` / `p.…`. Catalog playlists use `pl.…`,
/// which must not be treated as a library playlist id.
pub(crate) fn looks_library_id(id: &str) -> bool {
    let Some((prefix, rest)) = id.split_once('.') else {
        return false;
    };
    matches!(prefix, "i" | "l" | "p") && !rest.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catalog_song_becomes_a_playable_entry() {
        let json = serde_json::json!({
            "id": "1440857781",
            "type": "songs",
            "attributes": {
                "name": "Bloom",
                "artistName": "Radiohead",
                "albumName": "The King of Limbs",
                "durationInMillis": 328000,
                "playParams": { "id": "1440857781", "kind": "song" }
            }
        });
        let resource: TypedResource = serde_json::from_value(json).unwrap();
        let Entry::Song(track) = resource.into_entry().unwrap() else {
            panic!("expected a song");
        };
        assert_eq!(track.title, "Bloom");
        assert_eq!(track.catalog_id.as_deref(), Some("1440857781"));
        assert!(track.playable());
    }

    #[test]
    fn a_library_playlist_keeps_its_library_flag() {
        let json = serde_json::json!({
            "id": "p.abc",
            "type": "library-playlists",
            "attributes": { "name": "Game on" }
        });
        let resource: TypedResource = serde_json::from_value(json).unwrap();
        let Entry::Playlist(list) = resource.into_entry().unwrap() else {
            panic!("expected a playlist");
        };
        assert!(list.library);
        assert_eq!(list.id, "p.abc");
    }

    #[test]
    fn a_station_is_skipped() {
        let json = serde_json::json!({
            "id": "ra.1",
            "type": "stations",
            "attributes": { "name": "Radio" }
        });
        let resource: TypedResource = serde_json::from_value(json).unwrap();
        assert!(resource.into_entry().is_none());
    }

    #[test]
    fn a_recommendation_group_yields_its_contents() {
        let json = serde_json::json!({
            "id": "rec.1",
            "type": "personal-recommendation",
            "relationships": {
                "contents": {
                    "data": [
                        {
                            "id": "pl.abc",
                            "type": "playlists",
                            "attributes": { "name": "New Music Mix" }
                        },
                        {
                            "id": "1440857781",
                            "type": "songs",
                            "attributes": {
                                "name": "Bloom",
                                "artistName": "Radiohead",
                                "playParams": { "id": "1440857781", "kind": "song" }
                            }
                        }
                    ]
                }
            }
        });
        let resource: TypedResource = serde_json::from_value(json).unwrap();
        let contents = contents_of(&resource);
        assert_eq!(contents.len(), 2);
        assert!(matches!(&contents[0], Entry::Playlist(p) if p.name == "New Music Mix"));
        assert!(matches!(&contents[1], Entry::Song(t) if t.title == "Bloom"));
    }

    #[test]
    fn stubs_without_attributes_are_collected_not_turned_into_tiles() {
        let json = serde_json::json!({
            "id": "rec.1",
            "type": "personal-recommendation",
            "relationships": {
                "contents": {
                    "href": "/v1/me/recommendations/rec.1/contents",
                    "data": [
                        { "id": "pl.u-mix", "type": "playlists" },
                        { "id": "1440857781", "type": "songs" },
                        { "id": "l.abc", "type": "library-albums" }
                    ]
                }
            }
        });
        let resource: TypedResource = serde_json::from_value(json).unwrap();
        assert!(contents_of(&resource).is_empty());
        let (songs, albums, playlists) = stub_ids(&contents_resources(&resource));
        assert_eq!(songs, vec!["1440857781"]);
        assert_eq!(albums, vec!["l.abc"]);
        assert_eq!(playlists, vec!["pl.u-mix"]);
        assert_eq!(
            contents_href(&resource).as_deref(),
            Some("/me/recommendations/rec.1/contents")
        );
        assert!(looks_library_id("l.abc"));
        assert!(!looks_library_id("pl.u-mix"));
        assert!(!looks_library_id("1440857781"));
    }

    #[test]
    fn charts_accept_an_array_or_a_single_object() {
        let results = serde_json::json!({
            "songs": [{
                "chart": "most-played",
                "data": [{
                    "id": "1",
                    "type": "songs",
                    "attributes": {
                        "name": "One",
                        "artistName": "Aitana",
                        "playParams": { "id": "1", "kind": "song" }
                    }
                }]
            }],
            "albums": {
                "chart": "most-played",
                "data": [{
                    "id": "2",
                    "type": "albums",
                    "attributes": { "name": "Alpha", "artistName": "Aitana" }
                }]
            }
        });
        let songs = entries_from_list(chart_resources(&results, "songs"));
        let albums = entries_from_list(chart_resources(&results, "albums"));
        assert!(matches!(&songs[0], Entry::Song(t) if t.title == "One"));
        assert!(matches!(&albums[0], Entry::Album(a) if a.name == "Alpha"));
        assert!(chart_resources(&results, "playlists").is_empty());
    }

    #[test]
    fn api_paths_drop_the_v1_prefix() {
        assert_eq!(
            api_path("https://api.music.apple.com/v1/me/recommendations"),
            "/me/recommendations"
        );
        assert_eq!(api_path("/v1/catalog/es/charts"), "/catalog/es/charts");
        assert_eq!(api_path("/catalog/es/songs?ids=1"), "/catalog/es/songs?ids=1");
    }
}
