// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mixed Apple Music JSON: recently played, recommendations, charts.
//!
//! Those endpoints return several resource types in one array. The rest of the
//! client parses one kind at a time; this is the one place a `type` field
//! decides which of our types to build. Items without attributes (id-only
//! stubs inside a recommendation group) are skipped rather than fetched again.

use serde::Deserialize;

use crate::entry::Entry;
use crate::music::types::{
    Album, AlbumAttributes, Artist, ArtistAttributes, Playlist, PlaylistAttributes, Resource,
    SongAttributes, Track,
};

#[derive(Debug, Deserialize)]
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

/// `relationships.contents.data` on a personal-recommendation resource.
pub(crate) fn contents_of(resource: &TypedResource) -> Vec<Entry> {
    let Some(relationships) = &resource.relationships else {
        return Vec::new();
    };
    let Some(contents) = relationships.get("contents") else {
        return Vec::new();
    };
    let Some(data) = contents.get("data") else {
        return Vec::new();
    };
    let Ok(list) = serde_json::from_value::<Vec<TypedResource>>(data.clone()) else {
        return Vec::new();
    };
    entries_from_list(list)
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
}
