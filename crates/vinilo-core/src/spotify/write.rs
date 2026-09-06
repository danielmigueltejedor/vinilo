// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Spotify library writes through Pathfinder and the playlist/v2 service.
//!
//! The web-player token cannot call `api.spotify.com/v1/me/*`. Likes and
//! playlist edits go the same way the player does: GraphQL mutations plus a
//! protobuf-JSON POST to create an empty playlist.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use super::partner::{self, Session};
use super::{Ref, partner_query, session, uri_tail};
use crate::ipc::WriteAction;
use crate::music::types::Playlist;

pub async fn apply(
    http: &reqwest::Client,
    action: WriteAction,
    id: &str,
    playlist_id: Option<&str>,
    name: Option<&str>,
) -> Result<Option<String>> {
    let session = session(http).await?;
    match action {
        WriteAction::Favorite | WriteAction::AddToLibrary => {
            library_mutate(http, &session, "addToLibrary", id).await?;
            Ok(None)
        }
        WriteAction::Unfavorite | WriteAction::RemoveFromLibrary => {
            library_mutate(http, &session, "removeFromLibrary", id).await?;
            Ok(None)
        }
        WriteAction::CreatePlaylist => {
            let title = name
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("New playlist");
            let created = create_playlist(http, &session, title).await?;
            if let Some(track) = id_if_track(id) {
                add_to_playlist(http, &session, &created, &track).await?;
            }
            Ok(Some(created))
        }
        WriteAction::AddToPlaylist => {
            let playlist = playlist_id.context("missing playlist id")?;
            add_to_playlist(http, &session, playlist, id).await?;
            Ok(None)
        }
        WriteAction::RemoveFromPlaylist => {
            anyhow::bail!(
                "Removing a song from a Spotify playlist needs a row uid this client does not hold yet"
            )
        }
    }
}

fn id_if_track(id: &str) -> Option<String> {
    matches!(Ref::parse(id), Some(Ref::Track(_))).then(|| id.to_owned())
}

fn library_item_uri(id: &str) -> Result<String> {
    match Ref::parse(id) {
        Some(Ref::Track(t)) => Ok(format!("spotify:track:{t}")),
        Some(Ref::Album(a)) => Ok(format!("spotify:album:{a}")),
        Some(Ref::Artist(a)) => Ok(format!("spotify:artist:{a}")),
        Some(Ref::Playlist(p)) => Ok(format!("spotify:playlist:{p}")),
        Some(Ref::Liked | Ref::Top) => bail!("liked songs cannot be written that way"),
        None => bail!("not a Spotify id"),
    }
}

fn playlist_uri(id: &str) -> Result<String> {
    match Ref::parse(id) {
        Some(Ref::Playlist(p)) => Ok(format!("spotify:playlist:{p}")),
        _ => bail!("not a Spotify playlist id"),
    }
}

async fn library_mutate(
    http: &reqwest::Client,
    session: &Session,
    operation: &str,
    id: &str,
) -> Result<()> {
    let uri = library_item_uri(id)?;
    let shapes = [
        json!({ "libraryItemUris": [uri] }),
        json!({ "uris": [uri] }),
    ];
    let mut last = anyhow::anyhow!("spotify {operation} failed");
    for variables in shapes {
        match partner_query(http, session, operation, partner::ADD_TO_LIBRARY, variables).await {
            Ok(_) => return Ok(()),
            Err(err) => last = err,
        }
    }
    Err(last).context(format!("spotify {operation}"))
}

async fn add_to_playlist(
    http: &reqwest::Client,
    session: &Session,
    playlist_id: &str,
    track_id: &str,
) -> Result<()> {
    let playlist_uri = playlist_uri(playlist_id)?;
    let track_uri = library_item_uri(track_id)?;
    let shapes = [
        json!({
            "playlistUri": playlist_uri,
            "playlistItemUris": [track_uri],
            "newPosition": {
                "moveType": "BOTTOM_OF_PLAYLIST",
                "fromUid": null
            }
        }),
        json!({
            "playlistUri": playlist_uri,
            "uris": [track_uri],
            "newPosition": { "moveType": "BOTTOM" }
        }),
    ];
    let mut last = anyhow::anyhow!("spotify addToPlaylist failed");
    for variables in shapes {
        match partner_query(
            http,
            session,
            "addToPlaylist",
            partner::ADD_TO_PLAYLIST,
            variables,
        )
        .await
        {
            Ok(_) => return Ok(()),
            Err(err) => last = err,
        }
    }
    match add_to_playlist_v2(http, session, playlist_id, &track_uri).await {
        Ok(()) => Ok(()),
        Err(err) => Err(last).context(format!("spotify addToPlaylist ({err})")),
    }
}

fn add_items_body(track_uri: &str) -> Value {
    json!({
        "ops": [{
            "kind": "ADD",
            "add": {
                "fromIndex": 0,
                "items": [{ "uri": track_uri }],
                "addFirst": false,
                "addLast": true
            }
        }],
        "info": { "source": { "client": "WEBPLAYER" } }
    })
}

/// Pathfinder hashes rotate. The same playlist/v2 Cosmic POST that creates a
/// list can append a track without a persisted query.
async fn add_to_playlist_v2(
    http: &reqwest::Client,
    session: &Session,
    playlist_id: &str,
    track_uri: &str,
) -> Result<()> {
    let id = match Ref::parse(playlist_id) {
        Some(Ref::Playlist(id)) => id,
        _ => bail!("not a Spotify playlist id"),
    };
    let url = format!("{}/playlist/{id}", partner::PLAYLIST_V2);
    let mut req = http
        .post(&url)
        .bearer_auth(&session.access)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("Origin", "https://open.spotify.com")
        .header("Referer", "https://open.spotify.com/")
        .header("Spotify-App-Version", &session.client_version)
        .header("App-Platform", "WebPlayer");
    if let Some(token) = &session.client_token {
        req = req.header("client-token", token);
    }
    if let Some(cookie) = crate::setup::session_cookie(crate::provider::Provider::Spotify) {
        req = req.header("Cookie", cookie);
    }
    let res = req
        .json(&add_items_body(track_uri))
        .send()
        .await
        .context("spotify playlist add")?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!(
            %status,
            body = %super::clip_body(&text),
            "spotify playlist add http error"
        );
        bail!("Spotify would not add the track ({status})");
    }
    Ok(())
}

fn create_playlist_body(name: &str) -> Value {
    json!({
        "ops": [{
            "kind": "UPDATE_LIST_ATTRIBUTES",
            "updateListAttributes": {
                "newAttributes": {
                    "values": {
                        "name": name,
                        "formatAttributes": [],
                        "pictureSize": []
                    },
                    "noValue": []
                }
            }
        }],
        "info": { "source": { "client": "WEBPLAYER" } }
    })
}

async fn create_playlist(http: &reqwest::Client, session: &Session, name: &str) -> Result<String> {
    let url = format!("{}/playlist", partner::PLAYLIST_V2);
    let mut req = http
        .post(&url)
        .bearer_auth(&session.access)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("Origin", "https://open.spotify.com")
        .header("Referer", "https://open.spotify.com/")
        .header("Spotify-App-Version", &session.client_version)
        .header("App-Platform", "WebPlayer");
    if let Some(token) = &session.client_token {
        req = req.header("client-token", token);
    }
    if let Some(cookie) = crate::setup::session_cookie(crate::provider::Provider::Spotify) {
        req = req.header("Cookie", cookie);
    }
    let res = req
        .json(&create_playlist_body(name))
        .send()
        .await
        .context("spotify create playlist")?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!(
            %status,
            body = %super::clip_body(&text),
            "spotify create playlist http error"
        );
        bail!("Spotify would not create the playlist ({status})");
    }
    let value: Value = serde_json::from_str(&text).context("spotify create playlist json")?;
    let uri = value
        .get("uri")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/playlist/uri").and_then(Value::as_str))
        .context("Spotify create playlist did not return a uri")?;
    let id = uri_tail(uri);
    if id.is_empty() {
        bail!("Spotify create playlist uri was empty");
    }
    Ok(format!("sp:playlist:{id}"))
}

pub fn created_playlist(id: String, name: &str) -> Playlist {
    Playlist {
        id,
        date_added: String::new(),
        last_modified: String::new(),
        name: name.to_owned(),
        curator: String::new(),
        description: String::new(),
        artwork: None,
        library: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_ids_become_spotify_uris() {
        assert_eq!(
            library_item_uri("sp:4uLU6hMCjMI75M1A2tKUQC").unwrap(),
            "spotify:track:4uLU6hMCjMI75M1A2tKUQC"
        );
        assert_eq!(
            playlist_uri("sp:playlist:37i9dQZF1DXcBWIGoYBM5M").unwrap(),
            "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"
        );
    }

    #[test]
    fn liked_songs_are_not_a_writable_uri() {
        assert!(library_item_uri("sp:liked").is_err());
    }

    #[test]
    fn create_playlist_body_names_the_list() {
        let body = create_playlist_body("Noche");
        assert_eq!(
            body.pointer("/ops/0/updateListAttributes/newAttributes/values/name")
                .and_then(Value::as_str),
            Some("Noche")
        );
        assert_eq!(
            body.pointer("/info/source/client").and_then(Value::as_str),
            Some("WEBPLAYER")
        );
    }

    #[test]
    fn add_items_body_appends_the_track() {
        let body = add_items_body("spotify:track:4uLU6hMCjMI75M1A2tKUQC");
        assert_eq!(
            body.pointer("/ops/0/kind").and_then(Value::as_str),
            Some("ADD")
        );
        assert_eq!(
            body.pointer("/ops/0/add/items/0/uri")
                .and_then(Value::as_str),
            Some("spotify:track:4uLU6hMCjMI75M1A2tKUQC")
        );
        assert_eq!(
            body.pointer("/ops/0/add/addLast").and_then(Value::as_bool),
            Some(true)
        );
    }
}
