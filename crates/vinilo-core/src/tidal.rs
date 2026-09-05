// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tidal catalogue writes and a thin library read, using the listen.tidal.com
//! session cookie. Playback is still `yt-dlp`.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::i18n::{self, Key};
use crate::ipc::WriteAction;
use crate::library_cache::Library;
use crate::music::types::{Artwork, Playlist, Track, TrackId};
use crate::provider::Provider;
use crate::setup;

const LISTEN: &str = "https://listen.tidal.com";
const API: &str = "https://api.tidal.com/v1";

fn cookie() -> Result<String> {
    setup::session_cookie(Provider::Tidal)
        .context(i18n::t(Key::SpotifyNotSignedIn).replace("Spotify", "Tidal"))
}

fn track_id(id: &str) -> Result<String> {
    id.strip_prefix("td:")
        .filter(|s| !s.is_empty() && !s.contains(':'))
        .map(str::to_owned)
        .context("not a Tidal track id")
}

fn playlist_uuid(id: &str) -> Result<String> {
    let rest = id.strip_prefix("td:playlist:").unwrap_or(id);
    let rest = rest.strip_prefix("td:").unwrap_or(rest);
    if rest.is_empty() {
        bail!("not a Tidal playlist id");
    }
    Ok(rest.to_owned())
}

async fn authed(
    http: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    body: Option<Value>,
) -> Result<Value> {
    let cookie = cookie()?;
    let mut req = http
        .request(method, url)
        .header("Accept", "application/json")
        .header("Origin", LISTEN)
        .header("Referer", format!("{LISTEN}/"))
        .header("Cookie", cookie);
    if let Some(body) = body {
        req = req.header("Content-Type", "application/json").json(&body);
    }
    let res = req.send().await.context("tidal")?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!(%status, body = %clip(&text), url, "tidal http error");
        bail!("Tidal {status}");
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).context("tidal json")
}

fn clip(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= 300 {
        trimmed.to_owned()
    } else {
        format!("{}…", trimmed.chars().take(300).collect::<String>())
    }
}

struct Session {
    user_id: String,
    country: String,
}

async fn session(http: &reqwest::Client) -> Result<Session> {
    let value = match authed(
        http,
        reqwest::Method::GET,
        &format!("{LISTEN}/v1/sessions"),
        None,
    )
    .await
    {
        Ok(value) => value,
        Err(_) => authed(http, reqwest::Method::GET, &format!("{API}/sessions"), None).await?,
    };
    let user_id = value
        .pointer("/userId")
        .or_else(|| value.pointer("/user/id"))
        .and_then(|v| {
            v.as_u64()
                .map(|n| n.to_string())
                .or_else(|| v.as_str().map(str::to_owned))
        })
        .context("Tidal session missing userId")?;
    let country = value
        .get("countryCode")
        .and_then(Value::as_str)
        .unwrap_or("US")
        .to_owned();
    Ok(Session { user_id, country })
}

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
            let track = track_id(id)?;
            authed(
                http,
                reqwest::Method::POST,
                &format!(
                    "{API}/users/{}/favorites/tracks?countryCode={}",
                    session.user_id, session.country
                ),
                Some(json!({ "trackId": track })),
            )
            .await?;
            Ok(None)
        }
        WriteAction::Unfavorite | WriteAction::RemoveFromLibrary => {
            let track = track_id(id)?;
            authed(
                http,
                reqwest::Method::DELETE,
                &format!(
                    "{API}/users/{}/favorites/tracks/{track}?countryCode={}",
                    session.user_id, session.country
                ),
                None,
            )
            .await?;
            Ok(None)
        }
        WriteAction::CreatePlaylist => {
            let title = name
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("New playlist");
            let value = authed(
                http,
                reqwest::Method::POST,
                &format!(
                    "{API}/users/{}/playlists?countryCode={}",
                    session.user_id, session.country
                ),
                Some(json!({ "title": title })),
            )
            .await?;
            let uuid = value
                .get("uuid")
                .or_else(|| value.get("id"))
                .and_then(Value::as_str)
                .context("Tidal did not return a playlist id")?;
            let created = format!("td:playlist:{uuid}");
            if track_id(id).is_ok() {
                add_to_playlist(http, &session, &created, id).await?;
            }
            Ok(Some(created))
        }
        WriteAction::AddToPlaylist => {
            let list = playlist_id.context("missing playlist id")?;
            add_to_playlist(http, &session, list, id).await?;
            Ok(None)
        }
        WriteAction::RemoveFromPlaylist => {
            anyhow::bail!("Tidal playlist item removal is not wired yet")
        }
    }
}

async fn add_to_playlist(
    http: &reqwest::Client,
    session: &Session,
    playlist_id: &str,
    track: &str,
) -> Result<()> {
    let uuid = playlist_uuid(playlist_id)?;
    let track = track_id(track)?;
    authed(
        http,
        reqwest::Method::POST,
        &format!(
            "{API}/playlists/{uuid}/items?countryCode={}",
            session.country
        ),
        Some(json!({ "trackIds": track })),
    )
    .await?;
    Ok(())
}

pub async fn library(http: &reqwest::Client) -> Result<Library> {
    let session = session(http).await?;
    let fav = authed(
        http,
        reqwest::Method::GET,
        &format!(
            "{API}/users/{}/favorites/tracks?limit=100&countryCode={}",
            session.user_id, session.country
        ),
        None,
    )
    .await
    .unwrap_or(Value::Null);
    let lists = authed(
        http,
        reqwest::Method::GET,
        &format!(
            "{API}/users/{}/playlists?limit=50&countryCode={}",
            session.user_id, session.country
        ),
        None,
    )
    .await
    .unwrap_or(Value::Null);
    let songs = tracks_from_favorites(&fav);
    let mut playlists = playlists_from(&lists);
    if !songs.is_empty() {
        playlists.insert(
            0,
            Playlist {
                id: "td:liked".into(),
                date_added: String::new(),
                last_modified: String::new(),
                name: i18n::t(Key::LikedSongs).to_owned(),
                curator: String::new(),
                description: String::new(),
                artwork: songs.first().and_then(|s| s.artwork.clone()),
                library: true,
            },
        );
    }
    Ok(Library::from_parts(
        songs,
        Vec::new(),
        Vec::new(),
        playlists,
    ))
}

fn tracks_from_favorites(value: &Value) -> Vec<Track> {
    let items = value
        .pointer("/items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    items
        .iter()
        .filter_map(|item| {
            let track = item.get("item").unwrap_or(item);
            let id = track.get("id")?.as_u64()?.to_string();
            let title = track.get("title")?.as_str()?.to_owned();
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
            let artwork = track
                .pointer("/album/cover")
                .and_then(Value::as_str)
                .map(|cover| {
                    format!(
                        "https://resources.tidal.com/images/{}/320x320.jpg",
                        cover.replace('-', "/")
                    )
                });
            Some(Track {
                date_added: String::new(),
                year: String::new(),
                favorite: true,
                in_library: true,
                library_id: Some(format!("td:{id}")),
                id: TrackId(format!("td:{id}")),
                catalog_id: Some(format!("td:{id}")),
                title,
                artist,
                album,
                duration_ms: track
                    .get("duration")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .saturating_mul(1000),
                track_number: 0,
                artwork: artwork.map(Artwork::new),
            })
        })
        .collect()
}

fn playlists_from(value: &Value) -> Vec<Playlist> {
    let items = value
        .pointer("/items")
        .or_else(|| value.get("playlists").and_then(|p| p.get("items")))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    items
        .iter()
        .filter_map(|item| {
            let list = item.get("playlist").unwrap_or(item);
            let uuid = list.get("uuid").or_else(|| list.get("id"))?.as_str()?;
            Some(Playlist {
                id: format!("td:playlist:{uuid}"),
                date_added: String::new(),
                last_modified: String::new(),
                name: list.get("title")?.as_str()?.to_owned(),
                curator: String::new(),
                description: String::new(),
                artwork: None,
                library: true,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidal_ids_strip_the_prefix() {
        assert_eq!(track_id("td:12345").unwrap(), "12345");
        assert_eq!(playlist_uuid("td:playlist:abc-def").unwrap(), "abc-def");
    }
}
