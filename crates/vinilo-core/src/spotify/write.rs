// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Spotify library writes through Pathfinder and the playlist/v2 service.
//!
//! The web-player token cannot call `api.spotify.com/v1/me/*`. Likes go
//! through Pathfinder. Creating a playlist is the Cosmic `playlist/v2` POST
//! the web player uses, then a rootlist pin so the new uri actually appears
//! in the library.

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
            pin_in_library(http, &session, &created).await?;
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
    let hashes = if operation == "removeFromLibrary" {
        partner::REMOVE_FROM_LIBRARY
    } else {
        partner::ADD_TO_LIBRARY
    };
    let shapes = [
        json!({ "uris": [uri] }),
        json!({ "libraryItemUris": [uri] }),
    ];
    let mut last = anyhow::anyhow!("spotify {operation} failed");
    for variables in shapes {
        match partner_query(http, session, operation, hashes, variables).await {
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
    // Match the web player's Cosmic JSON: empty arrays and `info` are omitted
    // by their protobuf toJSON, and sending them is what 400'd create.
    json!({
        "ops": [{
            "kind": "ADD",
            "add": {
                "addLast": true,
                "items": [{
                    "uri": track_uri,
                    "attributes": { "timestamp": unix_ms() }
                }]
            }
        }]
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
    playlist_v2_send(
        http,
        session,
        &url,
        &add_items_body(track_uri),
        "add the track",
    )
    .await?;
    Ok(())
}

fn create_playlist_body(name: &str) -> Value {
    json!({
        "ops": [{
            "kind": "UPDATE_LIST_ATTRIBUTES",
            "updateListAttributes": {
                "newAttributes": {
                    "values": { "name": name }
                }
            }
        }]
    })
}

fn unix_ms() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

fn rootlist_pin_body(playlist_uri: &str) -> Value {
    json!({
        "deltas": [{
            "ops": [{
                "kind": "ADD",
                "add": {
                    "addFirst": true,
                    "items": [{
                        "uri": playlist_uri,
                        "attributes": { "timestamp": unix_ms() }
                    }]
                }
            }]
        }]
    })
}

fn playlist_v2_post(
    http: &reqwest::Client,
    session: &Session,
    url: &str,
) -> reqwest::RequestBuilder {
    let mut req = http
        .post(url)
        .bearer_auth(&session.access)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json;charset=UTF-8")
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
    req
}

async fn playlist_v2_send(
    http: &reqwest::Client,
    session: &Session,
    url: &str,
    body: &Value,
    what: &str,
) -> Result<String> {
    let res = playlist_v2_post(http, session, url)
        .json(body)
        .send()
        .await
        .with_context(|| format!("spotify {what}"))?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!(
            %status,
            url,
            what,
            body = %super::clip_body(&text),
            "spotify playlist/v2 http error"
        );
        bail!("Spotify would not {what} ({status})");
    }
    Ok(text)
}

async fn create_playlist(http: &reqwest::Client, session: &Session, name: &str) -> Result<String> {
    let url = format!("{}/playlist", partner::PLAYLIST_V2);
    let text = playlist_v2_send(
        http,
        session,
        &url,
        &create_playlist_body(name),
        "create playlist",
    )
    .await?;
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

/// A Cosmic create does not put the list in the library. The web player then
/// POSTs the new uri onto the user's rootlist. Without that, Spotify has the
/// list and Vinilo never sees it.
async fn pin_in_library(
    http: &reqwest::Client,
    session: &Session,
    playlist_id: &str,
) -> Result<()> {
    match pin_in_rootlist(http, session, playlist_id).await {
        Ok(()) => return Ok(()),
        Err(err) => tracing::warn!(%err, "spotify rootlist pin failed; trying addToLibrary"),
    }
    library_mutate(http, session, "addToLibrary", playlist_id)
        .await
        .context("spotify pin playlist in library")
}

async fn pin_in_rootlist(
    http: &reqwest::Client,
    session: &Session,
    playlist_id: &str,
) -> Result<()> {
    let user = username(http, session).await?;
    let uri = playlist_uri(playlist_id)?;
    let url = format!(
        "{}/user/{}/rootlist/changes",
        partner::PLAYLIST_V2,
        crate::streams::urlencoding(&user)
    );
    playlist_v2_send(
        http,
        session,
        &url,
        &rootlist_pin_body(&uri),
        "pin playlist",
    )
    .await?;
    Ok(())
}

async fn username(http: &reqwest::Client, session: &Session) -> Result<String> {
    let value = partner_query(
        http,
        session,
        "profileAttributes",
        partner::PROFILE_ATTRIBUTES,
        json!({}),
    )
    .await
    .context("spotify profileAttributes")?;
    value
        .pointer("/data/me/profile/username")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .context("Spotify profile had no username")
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
        // The web player's protobuf toJSON drops empty arrays and `info`.
        // Sending them is the 400 we kept hitting.
        assert!(body.get("info").is_none(), "{body}");
        assert!(
            body.pointer("/ops/0/updateListAttributes/newAttributes/values/formatAttributes")
                .is_none(),
            "{body}"
        );
        assert!(
            body.pointer("/ops/0/updateListAttributes/newAttributes/noValue")
                .is_none(),
            "{body}"
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
        assert!(body.get("info").is_none(), "{body}");
    }

    #[test]
    fn rootlist_pin_puts_the_list_first() {
        let body = rootlist_pin_body("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M");
        assert_eq!(
            body.pointer("/deltas/0/ops/0/kind").and_then(Value::as_str),
            Some("ADD")
        );
        assert_eq!(
            body.pointer("/deltas/0/ops/0/add/addFirst")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            body.pointer("/deltas/0/ops/0/add/items/0/uri")
                .and_then(Value::as_str),
            Some("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M")
        );
    }
}
