// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tidal catalogue writes and a thin library read, using the listen.tidal.com
//! session cookie. Playback is still `yt-dlp`.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::i18n::{self, Key};
use crate::ipc::WriteAction;
use crate::library_cache::Library;
use crate::music::types::{Album, Artist, Artwork, Playlist, Track, TrackId};
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

async fn authed_session(
    http: &reqwest::Client,
    session: &Session,
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
    if !session.session_id.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", session.session_id));
    }
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

fn b64_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    while i + 1 < bytes.len() {
        let a = val(bytes[i])?;
        let b = val(bytes[i + 1])?;
        out.push((a << 2) | (b >> 4));
        if i + 2 >= bytes.len() {
            break;
        }
        let c = val(bytes[i + 2])?;
        out.push((b << 4) | (c >> 2));
        if i + 3 >= bytes.len() {
            break;
        }
        let d = val(bytes[i + 3])?;
        out.push((c << 6) | d);
        i += 4;
    }
    Some(out)
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
    session_id: String,
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
    let session_id = value
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    Ok(Session {
        user_id,
        country,
        session_id,
    })
}

/// Cookie plus the account country, so search is not stuck on US.
pub async fn locale(http: &reqwest::Client) -> (String, String) {
    let cookie = cookie().unwrap_or_default();
    let country = session(http)
        .await
        .map(|s| s.country)
        .unwrap_or_else(|_| "US".into());
    (cookie, country)
}

/// A direct audio URL for this session, when Tidal hands one back.
///
/// Encrypted DASH manifests are refused: those need the web player, the same
/// ceiling as Apple Music. A plain URL is streamed like YouTube Music.
pub async fn stream_url(http: &reqwest::Client, id: &str) -> Result<String> {
    let session = session(http).await?;
    let track = track_id(id)?;
    let url = format!(
        "{API}/tracks/{track}/playbackinfopostpaywall?playbackmode=STREAM&assetpresentation=FULL&audioquality=HIGH&countryCode={}",
        session.country
    );
    let value =
        authed_session(http, &session, reqwest::Method::POST, &url, Some(json!({}))).await?;
    let mime = value
        .get("manifestMimeType")
        .and_then(Value::as_str)
        .unwrap_or("");
    if mime.contains("dash") {
        bail!("Tidal returned an encrypted stream");
    }
    let manifest = value
        .get("manifest")
        .and_then(Value::as_str)
        .context("Tidal playback had no manifest")?;
    let raw = b64_decode(manifest).context("Tidal manifest was not base64")?;
    let text = String::from_utf8(raw).context("Tidal manifest was not text")?;
    if text.contains("<MPD") {
        bail!("Tidal returned an encrypted stream");
    }
    let parsed: Value = serde_json::from_str(&text).context("Tidal manifest json")?;
    if parsed
        .get("encryptionType")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty() && !s.eq_ignore_ascii_case("NONE"))
    {
        bail!("Tidal returned an encrypted stream");
    }
    parsed
        .pointer("/urls/0")
        .and_then(Value::as_str)
        .filter(|s| s.starts_with("http"))
        .map(str::to_owned)
        .context("Tidal manifest had no audio url")
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
    let songs = tracks_from_items(
        &collect_pages(
            http,
            &format!(
                "{API}/users/{}/favorites/tracks?countryCode={}",
                session.user_id, session.country
            ),
        )
        .await,
    );
    let albums = albums_from_items(
        &collect_pages(
            http,
            &format!(
                "{API}/users/{}/favorites/albums?countryCode={}",
                session.user_id, session.country
            ),
        )
        .await,
    );
    let artists = artists_from_items(
        &collect_pages(
            http,
            &format!(
                "{API}/users/{}/favorites/artists?countryCode={}",
                session.user_id, session.country
            ),
        )
        .await,
    );
    let mut playlists = playlists_from_items(
        &collect_pages(
            http,
            &format!(
                "{API}/users/{}/playlists?countryCode={}",
                session.user_id, session.country
            ),
        )
        .await,
    );
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
    Ok(Library::from_parts(songs, albums, artists, playlists))
}

async fn collect_pages(http: &reqwest::Client, base: &str) -> Vec<Value> {
    let mut all = Vec::new();
    let mut offset = 0u32;
    loop {
        let join = if base.contains('?') { '&' } else { '?' };
        let url = format!("{base}{join}limit=50&offset={offset}");
        let Ok(value) = authed(http, reqwest::Method::GET, &url, None).await else {
            break;
        };
        let items = value
            .pointer("/items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let n = items.len();
        all.extend(items);
        if n < 50 || offset >= 2_000 {
            break;
        }
        offset += 50;
    }
    all
}

fn tracks_from_items(items: &[Value]) -> Vec<Track> {
    items
        .iter()
        .filter_map(|item| {
            let track = item.get("item").unwrap_or(item);
            track_from(track)
        })
        .collect()
}

fn track_from(track: &Value) -> Option<Track> {
    let id = track.get("id").and_then(|v| {
        v.as_u64()
            .map(|n| n.to_string())
            .or_else(|| v.as_str().map(str::to_owned))
    })?;
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
    let artwork = cover_url(track.pointer("/album/cover").and_then(Value::as_str));
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
}

fn albums_from_items(items: &[Value]) -> Vec<Album> {
    items
        .iter()
        .filter_map(|item| {
            let album = item.get("item").unwrap_or(item);
            let id = album.get("id").and_then(|v| {
                v.as_u64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_str().map(str::to_owned))
            })?;
            let name = album
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Album")
                .to_owned();
            let artist = album
                .pointer("/artist/name")
                .or_else(|| album.pointer("/artists/0/name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            Some(Album {
                id: format!("td:album:{id}"),
                date_added: String::new(),
                name,
                artist,
                artwork: cover_url(album.get("cover").and_then(Value::as_str)).map(Artwork::new),
                year: String::new(),
                track_count: album
                    .get("numberOfTracks")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                library: true,
            })
        })
        .collect()
}

fn artists_from_items(items: &[Value]) -> Vec<Artist> {
    items
        .iter()
        .filter_map(|item| {
            let artist = item.get("item").unwrap_or(item);
            let id = artist.get("id").and_then(|v| {
                v.as_u64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_str().map(str::to_owned))
            })?;
            let name = artist.get("name").and_then(Value::as_str)?.to_owned();
            Some(Artist {
                id: format!("td:artist:{id}"),
                name,
                artwork: cover_url(artist.get("picture").and_then(Value::as_str)).map(Artwork::new),
                genres: String::new(),
                library: true,
            })
        })
        .collect()
}

fn cover_url(uuid: Option<&str>) -> Option<String> {
    let uuid = uuid?;
    Some(format!(
        "https://resources.tidal.com/images/{}/320x320.jpg",
        uuid.replace('-', "/")
    ))
}

fn playlists_from_items(items: &[Value]) -> Vec<Playlist> {
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
