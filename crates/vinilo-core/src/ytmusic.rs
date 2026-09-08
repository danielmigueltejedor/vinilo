// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! YouTube Music InnerTube reads and writes, the same surface ytmusicapi and
//! Music Assistant use: signed WEB_REMIX `browse` calls.
//!
//! Playback prefers a direct InnerTube audio URL (the same idea Sonora's
//! `ytmusic-rs` uses for guest VisionOS streams). Ciphered formats still fall
//! through to `yt-dlp` in the daemon. This module is also the signed-in
//! catalogue: likes, library albums, artists, playlists, and the lists that
//! should show up in the sidebar after a write.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};

use crate::discover::Discover;
use crate::entry::Entry;
use crate::i18n::{self, Key};
use crate::ipc::{PageKind, WriteAction};
use crate::library_cache::Library;
use crate::music::types::{Album, Artist, Artwork, Playlist, Track};
use crate::provider::Provider;
use crate::setup;
use crate::streams::StreamHit;

const ORIGIN: &str = "https://music.youtube.com";
const API: &str = "https://music.youtube.com/youtubei/v1";
const KEY: &str = "AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";
/// Same string ytmusicapi sends. InnerTube 403s a lot of library browses
/// when this looks like a raw reqwest client.
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0";

static VISITOR: Mutex<Option<String>> = Mutex::new(None);

fn cached_visitor() -> Option<String> {
    VISITOR.lock().ok().and_then(|g| g.clone())
}

fn store_visitor(id: String) {
    if let Ok(mut g) = VISITOR.lock() {
        *g = Some(id);
    }
}

fn visitor_from_html(html: &str) -> Option<String> {
    for key in ["\"VISITOR_DATA\":\"", "\"visitorData\":\""] {
        if let Some(rest) = html.split(key).nth(1) {
            let id = rest.split('"').next().unwrap_or("");
            if !id.is_empty() && id.len() < 200 {
                return Some(id.to_owned());
            }
        }
    }
    None
}

async fn ensure_visitor(http: &reqwest::Client, cookie: &str) {
    if cached_visitor().is_some() {
        return;
    }
    let res = http
        .get(ORIGIN)
        .header("User-Agent", USER_AGENT)
        .header("Cookie", cookie)
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await;
    let Ok(res) = res else {
        return;
    };
    let Ok(html) = res.text().await else {
        return;
    };
    if let Some(id) = visitor_from_html(&html) {
        tracing::debug!(visitor = %id, "youtube music visitor id");
        store_visitor(id);
    }
}

fn context() -> Value {
    let mut client = json!({
        "clientName": "WEB_REMIX",
        "clientVersion": "1.20250903.01.00",
        "hl": "en",
        "gl": "US"
    });
    if let Some(visitor) = cached_visitor()
        && let Some(obj) = client.as_object_mut()
    {
        obj.insert("visitorData".into(), json!(visitor));
    }
    json!({
        "client": client,
        "user": {}
    })
}

fn cookie_header() -> Result<String> {
    setup::session_cookie(Provider::YoutubeMusic)
        .context(i18n::t(Key::SpotifyNotSignedIn).replace("Spotify", "YouTube Music"))
}

fn sapisid_from_cookie(cookie: &str) -> Option<String> {
    // ytmusicapi hashes `__Secure-3PAPISID`. Prefer that over the older
    // `SAPISID` name, which can be present and stale in the same jar.
    let mut sapisid = None;
    let mut secure1 = None;
    let mut secure3 = None;
    for part in cookie.split(';') {
        let part = part.trim();
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        if name.eq_ignore_ascii_case("__Secure-3PAPISID") {
            secure3 = Some(value.to_owned());
        } else if name.eq_ignore_ascii_case("__Secure-1PSAPISID") {
            secure1 = Some(value.to_owned());
        } else if name.eq_ignore_ascii_case("SAPISID") {
            sapisid = Some(value.to_owned());
        }
    }
    secure3.or(secure1).or(sapisid)
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
    ensure_visitor(http, &cookie).await;
    let mut body = extras;
    if let Some(obj) = body.as_object_mut() {
        obj.insert("context".into(), context());
    }
    let mut req = http
        .post(format!("{API}/{endpoint}?alt=json&key={KEY}"))
        .header("User-Agent", USER_AGENT)
        .header("Accept", "*/*")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Content-Type", "application/json")
        .header("Origin", ORIGIN)
        .header("Referer", format!("{ORIGIN}/"))
        .header("X-Origin", ORIGIN)
        .header("X-Goog-AuthUser", "0")
        .header("Cookie", &cookie);
    if let Some(visitor) = cached_visitor() {
        req = req.header("X-Goog-Visitor-Id", visitor);
    }
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
    if id == "yt:liked" {
        return Ok("LM".into());
    }
    let rest = id.strip_prefix("yt:playlist:").unwrap_or(id);
    let rest = rest.strip_prefix("yt:").unwrap_or(rest);
    let rest = rest.strip_prefix("VL").unwrap_or(rest);
    if rest.is_empty() {
        bail!("not a YouTube Music playlist id");
    }
    Ok(rest.to_owned())
}

fn album_key(id: &str) -> Result<String> {
    let rest = id.strip_prefix("yt:album:").unwrap_or(id);
    let rest = rest.strip_prefix("yt:").unwrap_or(rest);
    if rest.starts_with("MPREb_") || rest.starts_with("OLAK") {
        Ok(rest.to_owned())
    } else {
        bail!("not a YouTube Music album id")
    }
}

fn artist_key(id: &str) -> Result<String> {
    let rest = id.strip_prefix("yt:artist:").unwrap_or(id);
    let rest = rest.strip_prefix("yt:").unwrap_or(rest);
    let rest = rest.strip_prefix("MPLA").unwrap_or(rest);
    if rest.starts_with("UC") {
        Ok(rest.to_owned())
    } else {
        bail!("not a YouTube Music artist id")
    }
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

/// The same four library tabs ytmusicapi reads: liked songs, playlists,
/// saved albums, and artists whose songs are in the library.
pub async fn library(http: &reqwest::Client) -> Result<Library> {
    let (liked, lists, albums, artists) = tokio::join!(
        browse(http, "FEmusic_liked_videos"),
        browse(http, "FEmusic_liked_playlists"),
        browse(http, "FEmusic_liked_albums"),
        browse(http, "FEmusic_library_corpus_track_artists"),
    );
    let liked = liked.unwrap_or_else(|err| {
        tracing::warn!(?err, "youtube music liked videos");
        Value::Null
    });
    let lists = lists.unwrap_or_else(|err| {
        tracing::warn!(?err, "youtube music playlists");
        Value::Null
    });
    let albums_json = albums.unwrap_or_else(|err| {
        tracing::warn!(?err, "youtube music albums");
        Value::Null
    });
    let artists_json = artists.unwrap_or_else(|err| {
        tracing::warn!(?err, "youtube music artists");
        Value::Null
    });

    let mut songs = tracks_from_browse(&liked);
    if songs.is_empty() {
        // ytmusicapi's liked-songs playlist is `LM`, not the liked-videos tab.
        match browse(http, "VLLM").await {
            Ok(liked_playlist) => songs = tracks_from_browse(&liked_playlist),
            Err(err) => tracing::warn!(?err, "youtube music liked playlist"),
        }
    }
    let albums = albums_from_browse(&albums_json);
    let artists = artists_from_browse(&artists_json);
    let mut playlists = playlists_from_browse(&lists);
    if !songs.is_empty()
        && !playlists
            .iter()
            .any(|p| p.id == "yt:liked" || p.id == "yt:playlist:LM")
    {
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
    tracing::info!(
        songs = songs.len(),
        albums = albums.len(),
        artists = artists.len(),
        playlists = playlists.len(),
        "youtube music library"
    );
    Ok(Library::from_parts(songs, albums, artists, playlists))
}

/// YouTube Music's own home page (`FEmusic_home`), the same browse ytmusicapi
/// `get_home` uses. This is what Listen Now should show when the library is
/// empty — mixes, listen-again, charts — not a blank status page.
pub async fn discover(http: &reqwest::Client) -> Result<Discover> {
    let home = browse(http, "FEmusic_home").await?;
    let mut page = discover_from_home(&home);
    if page.charts.is_empty() || page.recommended_playlists.is_empty() {
        match browse(http, "FEmusic_explore").await {
            Ok(explore) => page.fill_gaps(discover_from_home(&explore)),
            Err(err) => tracing::warn!(?err, "youtube music explore"),
        }
    }
    tracing::info!(
        recently_played = page.recently_played.len(),
        made_for_you = page.recommended_playlists.len(),
        recommended_songs = page.recommended_songs.len(),
        recently_added = page.recently_added.len(),
        charts = page.charts.len(),
        "youtube music home"
    );
    Ok(page)
}

const SHELF: usize = 16;

#[derive(Clone, Copy)]
enum HomeShelf {
    ListenAgain,
    Charts,
    Picks,
    Other,
}

fn classify_carousel(title: &str) -> HomeShelf {
    let t = title.to_ascii_lowercase();
    if t.contains("listen again")
        || t.contains("forgotten")
        || t.contains("jump back")
        || t.contains("recientes")
        || t.contains("escuchar")
        || t.contains("volver a")
    {
        HomeShelf::ListenAgain
    } else if t.contains("chart")
        || t.contains("trending")
        || t.contains("top 100")
        || t.contains("top songs")
        || t.contains("éxito")
        || t.contains("exito")
    {
        HomeShelf::Charts
    } else if t.contains("quick pick")
        || t.contains("mixed for you")
        || t.contains("songs for you")
        || t.contains("canciones para")
    {
        HomeShelf::Picks
    } else {
        HomeShelf::Other
    }
}

fn discover_from_home(value: &Value) -> Discover {
    let mut recently_played = Vec::new();
    let mut recommended_playlists = Vec::new();
    let mut recommended_songs = Vec::new();
    let mut recently_added = Vec::new();
    let mut charts = Vec::new();

    walk(value, &mut |node| {
        let Some(carousel) = node.get("musicCarouselShelfRenderer") else {
            return;
        };
        let title = carousel
            .pointer("/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text")
            .or_else(|| {
                carousel
                    .pointer("/header/musicCarouselShelfBasicHeaderRenderer/strapline/runs/0/text")
            })
            .and_then(Value::as_str)
            .unwrap_or("");
        let kind = classify_carousel(title);
        let songs = tracks_from_browse(carousel);
        let albums = albums_from_browse(carousel);
        let lists = playlists_from_browse(carousel);
        match kind {
            HomeShelf::ListenAgain => {
                recently_played.extend(songs.into_iter().map(Entry::Song));
                recently_played.extend(albums.into_iter().map(Entry::Album));
                recently_played.extend(lists.into_iter().map(Entry::Playlist));
            }
            HomeShelf::Charts => {
                charts.extend(songs.into_iter().map(Entry::Song));
                charts.extend(albums.into_iter().map(Entry::Album));
                charts.extend(lists.into_iter().map(Entry::Playlist));
            }
            HomeShelf::Picks => {
                recommended_songs.extend(songs);
                recently_added.extend(albums.into_iter().map(Entry::Album));
                recommended_playlists.extend(lists.into_iter().map(Entry::Playlist));
            }
            HomeShelf::Other => {
                recommended_songs.extend(songs);
                recently_added.extend(albums.into_iter().map(Entry::Album));
                recommended_playlists.extend(lists.into_iter().map(Entry::Playlist));
            }
        }
    });

    recently_played.truncate(SHELF);
    recommended_playlists.truncate(SHELF);
    recommended_songs.truncate(SHELF);
    recently_added.truncate(SHELF);
    charts.truncate(SHELF);
    let mut page = Discover::shelves(
        recently_played,
        recommended_playlists,
        recommended_songs,
        recently_added,
        charts,
    );
    page.drop_placeholder_titles();
    page
}

pub async fn open(http: &reqwest::Client, kind: PageKind, id: &str) -> Result<(Entry, Vec<Entry>)> {
    match kind {
        PageKind::Playlist | PageKind::LibraryPlaylist => {
            let key = playlist_key(id)?;
            let browse_id = if key.starts_with("VL") {
                key.clone()
            } else {
                format!("VL{key}")
            };
            let value = browse(http, &browse_id).await?;
            let songs = tracks_from_browse(&value);
            let header = Playlist {
                id: if id.starts_with("yt:") {
                    id.to_owned()
                } else {
                    format!("yt:playlist:{key}")
                },
                date_added: String::new(),
                last_modified: String::new(),
                name: page_title(&value).unwrap_or_else(|| key.clone()),
                curator: String::new(),
                description: String::new(),
                artwork: songs.first().and_then(|s| s.artwork.clone()),
                library: true,
            };
            Ok((
                Entry::Playlist(header),
                songs.into_iter().map(Entry::Song).collect(),
            ))
        }
        PageKind::Album | PageKind::LibraryAlbum => {
            let key = album_key(id)?;
            let value = browse(http, &key).await?;
            let mut songs = tracks_from_browse(&value);
            if songs.is_empty()
                && let Some(audio) = first_playlist_id(&value).filter(|p| p.starts_with("OLAK"))
                && let Ok(extra) = browse(http, &format!("VL{audio}")).await
            {
                songs = tracks_from_browse(&extra);
            }
            let albums = albums_from_browse(&value);
            let mut header = albums.into_iter().next().unwrap_or(Album {
                id: format!("yt:album:{key}"),
                date_added: String::new(),
                name: page_title(&value).unwrap_or_else(|| key.clone()),
                artist: String::new(),
                artwork: songs.first().and_then(|s| s.artwork.clone()),
                year: String::new(),
                track_count: songs.len() as u32,
                library: true,
            });
            header.track_count = songs.len() as u32;
            header.library = true;
            Ok((
                Entry::Album(header),
                songs.into_iter().map(Entry::Song).collect(),
            ))
        }
        PageKind::Artist | PageKind::LibraryArtist => {
            let key = artist_key(id)?;
            let value = browse(http, &key).await?;
            let albums = albums_from_browse(&value);
            let songs = tracks_from_browse(&value);
            let header = artists_from_browse(&value)
                .into_iter()
                .next()
                .unwrap_or(Artist {
                    id: format!("yt:artist:{key}"),
                    name: page_title(&value).unwrap_or_else(|| key.clone()),
                    artwork: songs.first().and_then(|s| s.artwork.clone()),
                    genres: String::new(),
                    library: true,
                });
            let entries = if albums.is_empty() {
                songs.into_iter().map(Entry::Song).collect()
            } else {
                albums.into_iter().map(Entry::Album).collect()
            };
            Ok((Entry::Artist(header), entries))
        }
    }
}

async fn browse(http: &reqwest::Client, browse_id: &str) -> Result<Value> {
    innertube(http, "browse", json!({ "browseId": browse_id })).await
}

/// Title, artist and cover for one video. Used when a reconstructed `yt:` hit
/// would otherwise play under the raw id (`vrY1THC_NQE`) with no artwork.
pub async fn video_hit(http: &reqwest::Client, id: &str) -> Result<StreamHit> {
    let video = video_id(id)?;
    let value = innertube(http, "player", json!({ "videoId": video })).await?;
    hit_from_player(&video, &value)
        .or_else(|| {
            tracks_from_browse(&value)
                .into_iter()
                .next()
                .and_then(|track| StreamHit::from_song(&track))
        })
        .context("YouTube Music player had no title")
}

/// Fetch audio bytes for a `yt:` id into `dir`, without shelling out to yt-dlp.
///
/// Tries the VisionOS player first (Sonora's guest path), which often returns
/// plain googlevideo URLs. Then the signed WEB_REMIX player. Ciphered formats
/// are refused here so the daemon can fall back to yt-dlp.
pub async fn download_audio(http: &reqwest::Client, id: &str, dir: &Path) -> Result<PathBuf> {
    let video = video_id(id)?;
    let stem = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();
    let _ = std::fs::create_dir_all(dir);
    if let Some(existing) = find_cached_audio(dir, &stem) {
        return Ok(existing);
    }

    let stream = match direct_stream(http, &video).await {
        Ok(stream) => stream,
        Err(err) => {
            tracing::debug!(%video, ?err, "innertube had no direct audio url");
            bail!("no direct InnerTube audio for {video}");
        }
    };

    let path = dir.join(format!("{stem}.{}", stream.ext));
    let tmp = dir.join(format!("{stem}.part"));
    let bytes = http
        .get(&stream.url)
        .header("User-Agent", stream.user_agent)
        .header("Accept-Encoding", "identity")
        .header("Origin", "https://www.youtube.com")
        .header("Referer", "https://www.youtube.com/")
        .send()
        .await
        .context("youtube stream")?
        .error_for_status()
        .context("youtube stream status")?
        .bytes()
        .await
        .context("youtube stream body")?;
    if bytes.len() < 1024 {
        bail!("youtube stream was empty");
    }
    std::fs::write(&tmp, &bytes).context("write youtube audio")?;
    std::fs::rename(&tmp, &path).context("rename youtube audio")?;
    Ok(path)
}

struct DirectStream {
    url: String,
    ext: &'static str,
    user_agent: &'static str,
}

async fn direct_stream(http: &reqwest::Client, video: &str) -> Result<DirectStream> {
    if let Ok(value) = player_visionos(http, video).await
        && let Some(stream) = pick_direct_audio(&value, VISION_UA)
    {
        return Ok(stream);
    }
    let value = innertube(
        http,
        "player",
        json!({
            "videoId": video,
            "contentCheckOk": true,
            "racyCheckOk": true,
        }),
    )
    .await?;
    pick_direct_audio(&value, USER_AGENT).context("player had only ciphered audio")
}

const VISION_UA: &str = "com.google.ios.youtube/";

async fn player_visionos(http: &reqwest::Client, video: &str) -> Result<Value> {
    let body = json!({
        "context": {
            "client": {
                "clientName": "VISIONOS",
                "clientVersion": "0.1",
                "deviceModel": "Apple Vision Pro",
                "osName": "visionOS",
                "osVersion": "1.0.0",
                "hl": "en",
                "gl": "US",
            },
            "user": {}
        },
        "videoId": video,
        "contentCheckOk": true,
        "racyCheckOk": true,
        "playbackContext": {
            "contentPlaybackContext": { "html5Preference": "HTML5_PREF_WANTS" }
        }
    });
    let res = http
        .post(format!("{API}/player?alt=json&key={KEY}"))
        .header("User-Agent", VISION_UA)
        .header("Content-Type", "application/json")
        .header("X-YouTube-Client-Name", "101")
        .header("X-YouTube-Client-Version", "0.1")
        .json(&body)
        .send()
        .await
        .context("visionos player")?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("VISIONOS player {status}");
    }
    serde_json::from_str(&text).context("visionos player json")
}

fn pick_direct_audio(value: &Value, user_agent: &'static str) -> Option<DirectStream> {
    let formats = value
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            value
                .pointer("/streamingData/formats")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        );
    let mut best: Option<(u64, DirectStream)> = None;
    for format in formats {
        let Some(mime) = format.get("mimeType").and_then(Value::as_str) else {
            continue;
        };
        if !mime.starts_with("audio/") {
            continue;
        }
        if format.get("signatureCipher").is_some() {
            continue;
        }
        let Some(url) = format.get("url").and_then(Value::as_str) else {
            continue;
        };
        if url.is_empty() {
            continue;
        }
        let bitrate = format.get("bitrate").and_then(Value::as_u64).unwrap_or(0);
        let ext = if mime.contains("mp4") || mime.contains("mp4a") {
            "m4a"
        } else if mime.contains("webm") {
            "webm"
        } else if mime.contains("mp3") {
            "mp3"
        } else {
            "m4a"
        };
        let candidate = DirectStream {
            url: url.to_owned(),
            ext,
            user_agent,
        };
        if best.as_ref().is_none_or(|(b, _)| bitrate > *b) {
            best = Some((bitrate, candidate));
        }
    }
    best.map(|(_, stream)| stream)
}

fn find_cached_audio(dir: &Path, stem: &str) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name()?.to_string_lossy();
        if name.starts_with(stem)
            && !name.ends_with(".part")
            && !name.ends_with(".json")
            && path.is_file()
            && entry.metadata().map(|m| m.len() > 1024).unwrap_or(false)
        {
            return Some(path);
        }
    }
    None
}

fn hit_from_player(video: &str, value: &Value) -> Option<StreamHit> {
    let details = value.get("videoDetails")?;
    let title = details
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty() && *t != video)?;
    let artist = details
        .get("author")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let duration_ms = details
        .get("lengthSeconds")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(0)
        .saturating_mul(1000);
    let artwork = thumbnail(details)
        .or_else(|| last_thumb(details.pointer("/thumbnail/thumbnails")))
        .or_else(|| Some(crate::streams::youtube_thumb(video)));
    Some(StreamHit {
        id: format!("yt:{video}"),
        title: title.to_owned(),
        artist,
        album: String::new(),
        duration_ms,
        artwork,
        play_query: format!("https://www.youtube.com/watch?v={video}"),
    })
}

fn tracks_from_browse(value: &Value) -> Vec<Track> {
    let mut songs = Vec::new();
    walk(value, &mut |node| {
        let Some(obj) = node.as_object() else {
            return;
        };
        if obj.contains_key("browseEndpoint") && !obj.contains_key("watchEndpoint") {
            return;
        }
        let video = video_id_from_node(node);
        let Some(video) = video.filter(|s| !s.is_empty() && !s.contains(':') && s.len() >= 11)
        else {
            return;
        };
        let id = format!("yt:{video}");
        if songs
            .iter()
            .any(|t: &Track| t.catalog_id.as_deref() == Some(id.as_str()))
        {
            return;
        }
        // Overlay/watchEndpoint nodes carry a videoId and nothing else.
        // Using that as the title is how Listen Now showed `vrY1THC_NQE`.
        let Some(title) = song_title(node, video) else {
            return;
        };
        let artist = flex_column_text(node, 1)
            .or_else(|| subtitle_text(node, 0))
            .filter(|s| s != video && !crate::streams::title_is_raw_id(&id, s))
            .unwrap_or_default();
        let album = flex_column_text(node, 2).unwrap_or_default();
        let artwork = thumbnail(node).or_else(|| Some(crate::streams::youtube_thumb(video)));
        let hit = StreamHit {
            id: id.clone(),
            title,
            artist,
            album,
            duration_ms: 0,
            artwork,
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
        if video_id_from_node(node).is_some() {
            return;
        }
        let Some(playlist) = playlist_id_from_node(node) else {
            return;
        };
        if playlist == "LM" {
            return;
        }
        let id = format!("yt:playlist:{playlist}");
        if lists.iter().any(|p: &Playlist| p.id == id) {
            return;
        }
        let Some(name) = first_text(node).filter(|n| n != &playlist) else {
            return;
        };
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

fn albums_from_browse(value: &Value) -> Vec<Album> {
    let mut albums = Vec::new();
    walk(value, &mut |node| {
        if video_id_from_node(node).is_some() {
            return;
        }
        let Some(album_id) = album_id_from_node(node) else {
            return;
        };
        let id = format!("yt:album:{album_id}");
        if albums.iter().any(|a: &Album| a.id == id) {
            return;
        }
        let Some(name) = first_text(node).filter(|n| n != &album_id) else {
            return;
        };
        let artist = subtitle_text(node, 0)
            .or_else(|| flex_column_text(node, 1))
            .unwrap_or_default();
        let year = subtitle_text(node, 1)
            .filter(|s| s.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or_default();
        albums.push(Album {
            id,
            date_added: String::new(),
            name,
            artist,
            artwork: thumbnail(node).map(Artwork::new),
            year,
            track_count: 0,
            library: true,
        });
    });
    albums
}

fn artists_from_browse(value: &Value) -> Vec<Artist> {
    let mut artists = Vec::new();
    walk(value, &mut |node| {
        if video_id_from_node(node).is_some() {
            return;
        }
        let Some(channel) = artist_id_from_node(node) else {
            return;
        };
        let id = format!("yt:artist:{channel}");
        if artists.iter().any(|a: &Artist| a.id == id) {
            return;
        }
        let Some(name) = first_text(node).filter(|n| n != &channel) else {
            return;
        };
        artists.push(Artist {
            id,
            name,
            artwork: thumbnail(node).map(Artwork::new),
            genres: String::new(),
            library: true,
        });
    });
    artists
}

fn video_id_from_node(node: &Value) -> Option<&str> {
    node.get("videoId")
        .and_then(Value::as_str)
        .or_else(|| {
            node.pointer("/playlistItemData/videoId")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            node.pointer("/navigationEndpoint/watchEndpoint/videoId")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            node.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId")
                .and_then(Value::as_str)
        })
        .filter(|s| !s.is_empty())
}

fn playlist_id_from_node(node: &Value) -> Option<String> {
    let raw = node
        .get("playlistId")
        .and_then(Value::as_str)
        .or_else(|| {
            node.pointer("/navigationEndpoint/browseEndpoint/browseId")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            node.pointer("/title/runs/0/navigationEndpoint/browseEndpoint/browseId")
                .and_then(Value::as_str)
        })?;
    normalize_playlist_id(raw)
}

fn album_id_from_node(node: &Value) -> Option<String> {
    let raw = node
        .pointer("/navigationEndpoint/browseEndpoint/browseId")
        .and_then(Value::as_str)
        .or_else(|| {
            node.pointer("/title/runs/0/navigationEndpoint/browseEndpoint/browseId")
                .and_then(Value::as_str)
        })?;
    if raw.starts_with("MPREb_") {
        Some(raw.to_owned())
    } else {
        None
    }
}

fn artist_id_from_node(node: &Value) -> Option<String> {
    let raw = node
        .pointer("/navigationEndpoint/browseEndpoint/browseId")
        .and_then(Value::as_str)
        .or_else(|| {
            node.pointer("/title/runs/0/navigationEndpoint/browseEndpoint/browseId")
                .and_then(Value::as_str)
        })?;
    let channel = raw.strip_prefix("MPLA").unwrap_or(raw);
    if channel.starts_with("UC") {
        Some(channel.to_owned())
    } else {
        None
    }
}

/// InnerTube wraps playlists as `VL` + id. ytmusicapi strips those two
/// characters; we do the same, then drop feature pages (`FE…`), albums
/// (`MPRE…` / `OLAK…`) and radios (`RD…`).
fn normalize_playlist_id(raw: &str) -> Option<String> {
    let id = raw.strip_prefix("VL").unwrap_or(raw);
    if id.is_empty()
        || id.starts_with("FE")
        || id.starts_with("MP")
        || id.starts_with("UC")
        || id.starts_with("RD")
        || id.starts_with("OLAK")
        || id.starts_with("RDEM")
    {
        return None;
    }
    if id.starts_with("PL")
        || id.starts_with("LM")
        || id.starts_with("WL")
        || id.starts_with("LL")
        || id.starts_with("OLA")
        || id.len() >= 11
    {
        Some(id.to_owned())
    } else {
        None
    }
}

fn first_playlist_id(value: &Value) -> Option<String> {
    let mut found = None;
    walk(value, &mut |node| {
        if found.is_some() {
            return;
        }
        if let Some(id) = node.get("playlistId").and_then(Value::as_str)
            && !id.is_empty()
        {
            found = Some(id.to_owned());
        }
    });
    found
}

fn page_title(value: &Value) -> Option<String> {
    const PATHS: &[&str] = &[
        "/header/musicDetailHeaderRenderer/title/runs/0/text",
        "/header/musicEditablePlaylistDetailHeaderRenderer/header/musicDetailHeaderRenderer/title/runs/0/text",
        "/header/musicVisualHeaderRenderer/title/runs/0/text",
        "/header/musicImmersiveHeaderRenderer/title/runs/0/text",
        "/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicResponsiveHeaderRenderer/title/runs/0/text",
    ];
    for path in PATHS {
        if let Some(text) = value.pointer(path).and_then(Value::as_str)
            && !text.is_empty()
        {
            return Some(text.to_owned());
        }
    }
    None
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

fn song_title(node: &Value, video: &str) -> Option<String> {
    first_text(node).filter(|t| {
        let t = t.trim();
        !t.is_empty() && t != video && t != format!("yt:{video}")
    })
}

fn first_text(value: &Value) -> Option<String> {
    runs_text(
        value,
        "/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs",
    )
    .or_else(|| runs_text(value, "/title/runs"))
    .or_else(|| runs_text(value, "/headline/runs"))
    .or_else(|| runs_text(value, "/flexColumns/0/text/runs"))
    .or_else(|| {
        value
            .pointer("/title/simpleText")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    })
    .or_else(|| {
        value
            .pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/simpleText")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    })
}

fn runs_text(value: &Value, pointer: &str) -> Option<String> {
    let runs = value.pointer(pointer)?.as_array()?;
    let mut out = String::new();
    for run in runs {
        if let Some(text) = run.get("text").and_then(Value::as_str) {
            out.push_str(text);
        }
    }
    let out = out.trim();
    if out.is_empty() {
        None
    } else {
        Some(out.to_owned())
    }
}

fn flex_column_text(value: &Value, index: usize) -> Option<String> {
    value
        .pointer(&format!(
            "/flexColumns/{index}/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text"
        ))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn subtitle_text(value: &Value, index: usize) -> Option<String> {
    value
        .pointer(&format!("/subtitle/runs/{index}/text"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn thumbnail(value: &Value) -> Option<String> {
    const PATHS: &[&str] = &[
        "/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails",
        "/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails",
        "/thumbnail/thumbnails",
        "/musicThumbnailRenderer/thumbnail/thumbnails",
        "/croppedSquareThumbnailRenderer/thumbnail/thumbnails",
    ];
    for path in PATHS {
        if let Some(url) = last_thumb(value.pointer(path)) {
            return Some(url);
        }
    }
    let mut found = None;
    walk(value, &mut |node| {
        if found.is_some() {
            return;
        }
        if let Some(url) = last_thumb(node.get("thumbnails"))
            && (url.contains("ytimg") || url.contains("googleusercontent") || url.contains("ggpht"))
        {
            found = Some(url);
        }
    });
    found
}

fn last_thumb(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_array).and_then(|thumbs| {
        thumbs
            .iter()
            .max_by_key(|thumb| thumb.get("width").and_then(Value::as_u64).unwrap_or(0))
            .or_else(|| thumbs.last())
            .and_then(|thumb| thumb.get("url"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    })
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
        assert!(video_id("yt:playlist:PLabc").is_err());
    }

    #[test]
    fn sapisid_prefers_the_secure_cookie_ytmusicapi_hashes() {
        let cookie = "SID=x; SAPISID=stale; __Secure-3PAPISID=fresh; HSID=y";
        assert_eq!(sapisid_from_cookie(cookie).as_deref(), Some("fresh"));
    }

    #[test]
    fn sapisid_falls_back_when_the_secure_name_is_missing() {
        let cookie = "SID=x; SAPISID=secret; HSID=y";
        assert_eq!(sapisid_from_cookie(cookie).as_deref(), Some("secret"));
    }

    #[test]
    fn playlist_ids_accept_innertube_vl_wrappers() {
        assert_eq!(
            normalize_playlist_id("VLPLQwVIlKxHM6rz0fDJVv_0UlXGEWf-bFys").as_deref(),
            Some("PLQwVIlKxHM6rz0fDJVv_0UlXGEWf-bFys")
        );
        assert_eq!(
            normalize_playlist_id("PLabcdefghijk").as_deref(),
            Some("PLabcdefghijk")
        );
        assert!(normalize_playlist_id("FEmusic_liked_playlists").is_none());
        assert!(normalize_playlist_id("MPREb_G8AiyN7RvFg").is_none());
        assert!(normalize_playlist_id("RDAMVMHLCsfOykA94").is_none());
    }

    #[test]
    fn library_tiles_parse_from_innertube_two_row_items() {
        let json = json!({
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "gridRenderer": {
                                            "items": [
                                                {
                                                    "musicTwoRowItemRenderer": {
                                                        "title": { "runs": [{ "text": "Gym" }] },
                                                        "navigationEndpoint": {
                                                            "browseEndpoint": {
                                                                "browseId": "VLPLQwVIlKxHM6rz0fDJVv_0UlXGEWf-bFys"
                                                            }
                                                        },
                                                        "thumbnailRenderer": {
                                                            "musicThumbnailRenderer": {
                                                                "thumbnail": {
                                                                    "thumbnails": [
                                                                        { "url": "https://i.ytimg.com/vi/x/hqdefault.jpg" }
                                                                    ]
                                                                }
                                                            }
                                                        }
                                                    }
                                                },
                                                {
                                                    "musicTwoRowItemRenderer": {
                                                        "title": { "runs": [{ "text": "Beautiful" }] },
                                                        "subtitle": { "runs": [
                                                            { "text": "Project 46" },
                                                            { "text": " · " },
                                                            { "text": "2015" }
                                                        ]},
                                                        "navigationEndpoint": {
                                                            "browseEndpoint": {
                                                                "browseId": "MPREb_G8AiyN7RvFg"
                                                            }
                                                        }
                                                    }
                                                },
                                                {
                                                    "musicTwoRowItemRenderer": {
                                                        "title": { "runs": [{ "text": "Aitana" }] },
                                                        "navigationEndpoint": {
                                                            "browseEndpoint": {
                                                                "browseId": "UCxEqaQWosMHaTih-tgzDqug"
                                                            }
                                                        }
                                                    }
                                                }
                                            ]
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        });
        let lists = playlists_from_browse(&json);
        assert_eq!(lists.len(), 1);
        assert_eq!(
            lists[0].id,
            "yt:playlist:PLQwVIlKxHM6rz0fDJVv_0UlXGEWf-bFys"
        );
        assert_eq!(lists[0].name, "Gym");

        let albums = albums_from_browse(&json);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].id, "yt:album:MPREb_G8AiyN7RvFg");
        assert_eq!(albums[0].name, "Beautiful");
        assert_eq!(albums[0].artist, "Project 46");

        let artists = artists_from_browse(&json);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].id, "yt:artist:UCxEqaQWosMHaTih-tgzDqug");
        assert_eq!(artists[0].name, "Aitana");
    }

    #[test]
    fn playlist_key_accepts_the_synthetic_liked_id() {
        assert_eq!(playlist_key("yt:liked").unwrap(), "LM");
        assert_eq!(playlist_key("yt:playlist:PLabc").unwrap(), "PLabc");
        assert_eq!(playlist_key("yt:playlist:VLPLabc").unwrap(), "PLabc");
    }

    #[test]
    fn visitor_id_is_read_from_ytcfg_html() {
        let html = r#"ytcfg.set({"VISITOR_DATA":"Cgtabc123xyz"});"#;
        assert_eq!(visitor_from_html(html).as_deref(), Some("Cgtabc123xyz"));
    }

    #[test]
    fn home_carousels_fill_listen_now_shelves() {
        let json = json!({
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": { "runs": [{ "text": "Listen again" }] }
                                                    }
                                                },
                                                "contents": [{
                                                    "musicTwoRowItemRenderer": {
                                                        "title": { "runs": [{ "text": "Gym" }] },
                                                        "navigationEndpoint": {
                                                            "browseEndpoint": {
                                                                "browseId": "VLPLQwVIlKxHM6rz0fDJVv_0UlXGEWf-bFys"
                                                            }
                                                        }
                                                    }
                                                }]
                                            }
                                        },
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": { "runs": [{ "text": "Mixed for you" }] }
                                                    }
                                                },
                                                "contents": [{
                                                    "musicTwoRowItemRenderer": {
                                                        "title": { "runs": [{ "text": "Your mix" }] },
                                                        "navigationEndpoint": {
                                                            "browseEndpoint": {
                                                                "browseId": "VLPLQwVIlKxHM6aaaaaaaaaaaaaaaaaaa"
                                                            }
                                                        }
                                                    }
                                                }]
                                            }
                                        }
                                    ]
                                }
                            }
                        }
                    }]
                }
            }
        });
        let page = discover_from_home(&json);
        assert_eq!(page.recently_played.len(), 1);
        assert_eq!(page.recommended_playlists.len(), 1);
        assert_eq!(page.recommended_playlists[0].title(), "Your mix");
    }

    #[test]
    fn a_bare_watch_endpoint_is_not_a_song() {
        let json = json!({
            "navigationEndpoint": {
                "watchEndpoint": { "videoId": "vrY1THC_NQE" }
            }
        });
        assert!(tracks_from_browse(&json).is_empty());
    }

    #[test]
    fn a_titled_row_keeps_its_name_and_gets_a_cover() {
        let json = json!({
            "musicTwoRowItemRenderer": {
                "title": { "runs": [{ "text": "Pa Mal" }] },
                "subtitle": { "runs": [{ "text": "Aitana" }] },
                "navigationEndpoint": {
                    "watchEndpoint": { "videoId": "vrY1THC_NQE" }
                }
            }
        });
        let songs = tracks_from_browse(&json);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].title, "Pa Mal");
        assert_ne!(songs[0].title, "vrY1THC_NQE");
        let art = songs[0]
            .artwork
            .as_ref()
            .map(|a| a.url(300))
            .expect("cover");
        assert!(art.contains("vrY1THC_NQE"), "{art}");
    }

    #[test]
    fn an_accessibility_label_that_is_the_video_id_is_not_a_title() {
        let json = json!({
            "accessibility": {
                "accessibilityData": { "label": "vrY1THC_NQE" }
            },
            "navigationEndpoint": {
                "watchEndpoint": { "videoId": "vrY1THC_NQE" }
            }
        });
        assert!(tracks_from_browse(&json).is_empty());
    }

    #[test]
    fn player_details_become_a_named_hit_with_cover() {
        let json = json!({
            "videoDetails": {
                "videoId": "vrY1THC_NQE",
                "title": "Pa Mal",
                "author": "Aitana",
                "lengthSeconds": "180",
                "thumbnail": {
                    "thumbnails": [
                        { "url": "https://i.ytimg.com/vi/vrY1THC_NQE/default.jpg", "width": 120 },
                        { "url": "https://i.ytimg.com/vi/vrY1THC_NQE/hqdefault.jpg", "width": 480 }
                    ]
                }
            }
        });
        let hit = hit_from_player("vrY1THC_NQE", &json).unwrap();
        assert_eq!(hit.title, "Pa Mal");
        assert_eq!(hit.artist, "Aitana");
        assert_eq!(hit.duration_ms, 180_000);
        assert!(
            hit.artwork
                .as_deref()
                .is_some_and(|u| u.contains("hqdefault")),
            "{:?}",
            hit.artwork
        );
    }

    #[test]
    fn plain_player_formats_become_a_direct_stream() {
        let json = json!({
            "streamingData": {
                "adaptiveFormats": [
                    {
                        "itag": 140,
                        "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                        "bitrate": 128000,
                        "url": "https://googlevideo.com/audio.m4a"
                    },
                    {
                        "itag": 251,
                        "mimeType": "audio/webm; codecs=\"opus\"",
                        "bitrate": 160000,
                        "signatureCipher": "s=abc&sp=sig&url=https%3A%2F%2Fciphered"
                    }
                ]
            }
        });
        let stream = pick_direct_audio(&json, "ua").unwrap();
        assert_eq!(stream.ext, "m4a");
        assert!(stream.url.contains("googlevideo"));
    }

    #[test]
    fn ciphered_only_formats_are_refused() {
        let json = json!({
            "streamingData": {
                "adaptiveFormats": [{
                    "itag": 251,
                    "mimeType": "audio/webm",
                    "bitrate": 160000,
                    "signatureCipher": "s=abc&url=https%3A%2F%2Fx"
                }]
            }
        });
        assert!(pick_direct_audio(&json, "ua").is_none());
    }
}
