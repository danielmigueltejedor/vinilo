// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! YouTube Music InnerTube writes and a thin library read.
//!
//! Playback is still `yt-dlp`. This is the signed-in catalogue: likes,
//! playlists, and the lists that should show up in the sidebar after a write.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};

use crate::i18n::{self, Key};
use crate::ipc::WriteAction;
use crate::library_cache::Library;
use crate::music::types::{Artwork, Playlist, Track};
use crate::provider::Provider;
use crate::setup;
use crate::streams::StreamHit;

const ORIGIN: &str = "https://music.youtube.com";
const API: &str = "https://music.youtube.com/youtubei/v1";
const KEY: &str = "AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";

fn context() -> Value {
    json!({
        "client": {
            "clientName": "WEB_REMIX",
            "clientVersion": "1.20250903.01.00",
            "hl": "en",
            "gl": "US"
        }
    })
}

fn cookie_header() -> Result<String> {
    setup::session_cookie(Provider::YoutubeMusic)
        .context(i18n::t(Key::SpotifyNotSignedIn).replace("Spotify", "YouTube Music"))
}

fn sapisid_from_cookie(cookie: &str) -> Option<String> {
    for part in cookie.split(';') {
        let part = part.trim();
        let (name, value) = part.split_once('=')?;
        if name.eq_ignore_ascii_case("SAPISID")
            || name.eq_ignore_ascii_case("__Secure-1PSAPISID")
            || name.eq_ignore_ascii_case("__Secure-3PSAPISID")
        {
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn sapisidhash(sapisid: &str) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut hasher = Sha1::new();
    hasher.update(format!("{ts} {sapisid} {ORIGIN}").as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{ts}_{hex}")
}

async fn innertube(http: &reqwest::Client, endpoint: &str, extras: Value) -> Result<Value> {
    let cookie = cookie_header()?;
    let mut body = extras;
    if let Some(obj) = body.as_object_mut() {
        obj.insert("context".into(), context());
    }
    let mut req = http
        .post(format!("{API}/{endpoint}?alt=json&key={KEY}"))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("Origin", ORIGIN)
        .header("Referer", format!("{ORIGIN}/"))
        .header("X-Origin", ORIGIN)
        .header("Cookie", &cookie);
    if let Some(sapisid) = sapisid_from_cookie(&cookie) {
        req = req.header(
            "Authorization",
            format!("SAPISIDHASH {}", sapisidhash(&sapisid)),
        );
    }
    let res = req.json(&body).send().await.context("youtube music")?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!(endpoint, %status, body = %clip(&text), "youtube music http error");
        bail!("YouTube Music {status} for {endpoint}");
    }
    serde_json::from_str(&text).context("youtube music json")
}

fn clip(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= 400 {
        trimmed.to_owned()
    } else {
        format!("{}…", trimmed.chars().take(400).collect::<String>())
    }
}

fn video_id(id: &str) -> Result<String> {
    id.strip_prefix("yt:")
        .filter(|s| !s.is_empty() && !s.contains(':'))
        .map(str::to_owned)
        .context("not a YouTube Music track id")
}

fn playlist_key(id: &str) -> Result<String> {
    let rest = id.strip_prefix("yt:playlist:").unwrap_or(id);
    let rest = rest.strip_prefix("yt:").unwrap_or(rest);
    if rest.is_empty() {
        bail!("not a YouTube Music playlist id");
    }
    Ok(rest.to_owned())
}

pub async fn apply(
    http: &reqwest::Client,
    action: WriteAction,
    id: &str,
    playlist_id: Option<&str>,
    name: Option<&str>,
) -> Result<Option<String>> {
    match action {
        WriteAction::Favorite | WriteAction::AddToLibrary => {
            let video = video_id(id)?;
            innertube(http, "like/like", json!({ "target": { "videoId": video } })).await?;
            Ok(None)
        }
        WriteAction::Unfavorite | WriteAction::RemoveFromLibrary => {
            let video = video_id(id)?;
            innertube(
                http,
                "like/removelike",
                json!({ "target": { "videoId": video } }),
            )
            .await?;
            Ok(None)
        }
        WriteAction::CreatePlaylist => {
            let title = name
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("New playlist");
            let mut extras = json!({ "title": title, "privacyStatus": "PRIVATE" });
            if let Ok(video) = video_id(id) {
                extras
                    .as_object_mut()
                    .unwrap()
                    .insert("videoIds".into(), json!([video]));
            }
            let value = innertube(http, "playlist/create", extras).await?;
            let created = value
                .get("playlistId")
                .and_then(Value::as_str)
                .context("YouTube Music did not return a playlist id")?;
            Ok(Some(format!("yt:playlist:{created}")))
        }
        WriteAction::AddToPlaylist => {
            let list = playlist_id.context("missing playlist id")?;
            let video = video_id(id)?;
            innertube(
                http,
                "browse/edit_playlist",
                json!({
                    "playlistId": playlist_key(list)?,
                    "actions": [{ "action": "ACTION_ADD_VIDEO", "addedVideoId": video }]
                }),
            )
            .await?;
            Ok(None)
        }
        WriteAction::RemoveFromPlaylist => {
            let list = playlist_id.context("missing playlist id")?;
            let video = video_id(id)?;
            innertube(
                http,
                "browse/edit_playlist",
                json!({
                    "playlistId": playlist_key(list)?,
                    "actions": [{ "action": "ACTION_REMOVE_VIDEO", "removedVideoId": video }]
                }),
            )
            .await?;
            Ok(None)
        }
    }
}

pub async fn library(http: &reqwest::Client) -> Result<Library> {
    let liked = browse(http, "FEmusic_liked_videos")
        .await
        .unwrap_or_else(|err| {
            tracing::warn!(?err, "youtube music liked videos");
            Value::Null
        });
    let lists = browse(http, "FEmusic_liked_playlists")
        .await
        .unwrap_or_else(|err| {
            tracing::warn!(?err, "youtube music playlists");
            Value::Null
        });
    let songs = tracks_from_browse(&liked);
    let mut playlists = playlists_from_browse(&lists);
    if !songs.is_empty() {
        playlists.insert(
            0,
            Playlist {
                id: "yt:liked".into(),
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

async fn browse(http: &reqwest::Client, browse_id: &str) -> Result<Value> {
    innertube(http, "browse", json!({ "browseId": browse_id })).await
}

fn tracks_from_browse(value: &Value) -> Vec<Track> {
    let mut songs = Vec::new();
    walk(value, &mut |node| {
        let Some(obj) = node.as_object() else {
            return;
        };
        let video = obj.get("videoId").and_then(Value::as_str).or_else(|| {
            obj.get("navigationEndpoint")
                .and_then(|e| e.pointer("/watchEndpoint/videoId"))
                .and_then(Value::as_str)
        });
        let Some(video) = video.filter(|s| !s.is_empty()) else {
            return;
        };
        let id = format!("yt:{video}");
        if songs
            .iter()
            .any(|t: &Track| t.catalog_id.as_deref() == Some(id.as_str()))
        {
            return;
        }
        let title = first_text(node).unwrap_or_else(|| video.to_owned());
        let artist = flex_column_text(node, 1).unwrap_or_default();
        let hit = StreamHit {
            id: id.clone(),
            title,
            artist,
            album: String::new(),
            duration_ms: 0,
            artwork: thumbnail(node),
            play_query: format!("https://www.youtube.com/watch?v={video}"),
        };
        if let Some(mut song) = hit.into_entry().into_song() {
            song.favorite = true;
            song.in_library = true;
            song.library_id = Some(id);
            songs.push(song);
        }
    });
    songs
}

fn playlists_from_browse(value: &Value) -> Vec<Playlist> {
    let mut lists = Vec::new();
    walk(value, &mut |node| {
        let Some(obj) = node.as_object() else {
            return;
        };
        let playlist = obj.get("playlistId").and_then(Value::as_str).or_else(|| {
            obj.get("navigationEndpoint")
                .and_then(|e| e.pointer("/browseEndpoint/browseId"))
                .and_then(Value::as_str)
                .and_then(|b| b.strip_prefix("VL"))
        });
        let Some(playlist) = playlist.filter(|s| s.starts_with("PL") || s.starts_with("LM")) else {
            return;
        };
        let id = format!("yt:playlist:{playlist}");
        if lists.iter().any(|p: &Playlist| p.id == id) {
            return;
        }
        let name = first_text(node).unwrap_or_else(|| playlist.to_owned());
        lists.push(Playlist {
            id,
            date_added: String::new(),
            last_modified: String::new(),
            name,
            curator: String::new(),
            description: String::new(),
            artwork: thumbnail(node).map(Artwork::new),
            library: true,
        });
    });
    lists
}

fn walk<'a>(value: &'a Value, visit: &mut impl FnMut(&'a Value)) {
    visit(value);
    match value {
        Value::Array(items) => {
            for item in items {
                walk(item, visit);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                walk(item, visit);
            }
        }
        _ => {}
    }
}

fn first_text(value: &Value) -> Option<String> {
    value
        .pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")
        .or_else(|| value.pointer("/title/runs/0/text"))
        .or_else(|| value.pointer("/flexColumns/0/text/runs/0/text"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn flex_column_text(value: &Value, index: usize) -> Option<String> {
    value
        .pointer(&format!(
            "/flexColumns/{index}/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text"
        ))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn thumbnail(value: &Value) -> Option<String> {
    value
        .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
        .or_else(|| value.pointer("/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails"))
        .or_else(|| value.pointer("/thumbnail/thumbnails"))
        .and_then(Value::as_array)
        .and_then(|thumbs| thumbs.last())
        .and_then(|thumb| thumb.get("url"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

trait IntoSong {
    fn into_song(self) -> Option<Track>;
}

impl IntoSong for crate::entry::Entry {
    fn into_song(self) -> Option<Track> {
        match self {
            crate::entry::Entry::Song(track) => Some(track),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_ids_strip_the_prefix() {
        assert_eq!(video_id("yt:dQw4w9WgXcQ").unwrap(), "dQw4w9WgXcQ");
        assert!(video_id("sp:abc").is_err());
    }

    #[test]
    fn sapisid_is_read_from_the_cookie_header() {
        let cookie = "SID=x; SAPISID=secret; HSID=y";
        assert_eq!(sapisid_from_cookie(cookie).as_deref(), Some("secret"));
    }
}
