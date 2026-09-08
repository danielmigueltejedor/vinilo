// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Spotify catalogue via the web-player partner API.
//!
//! Playback goes through librespot when a Premium OAuth session is available
//! (same idea as Sonora). The cookie login here is still the catalogue path:
//! search, liked songs, playlists, albums, and Listen Now. The partner token
//! from `/api/token` cannot stream; librespot needs its own OAuth with the
//! `streaming` scope. Without Premium, the daemon falls back to `yt-dlp`.
//!
//! The cookie login mints an **api-partner** token (`/api/token` + TOTP). That
//! token does not work on `api.spotify.com/v1/me/*` (401/403). The web player
//! loads the library through Pathfinder GraphQL, so we do the same: a
//! client-token, then `libraryV3` / `fetchLibraryTracks` / `searchDesktop`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::discover::Discover;
use crate::entry::Entry;
use crate::i18n::{self, Key};
use crate::ipc::CatalogFilter;
use crate::music::client::SearchResults;
use crate::music::types::{Album, Artist, Artwork, Playlist, Track, TrackId};
use crate::provider::Provider;
use crate::streams::{self, StreamHit};

mod partner;
mod totp;
mod write;

pub use write::{apply as write, created_playlist};

const API: &str = "https://api.spotify.com/v1";
const PLAYER_TOKEN: &str = "https://open.spotify.com/api/token";

struct CachedToken {
    session: partner::Session,
    expires: Instant,
}

static TOKEN: Mutex<Option<CachedToken>> = Mutex::new(None);
static COOLDOWN: Mutex<Option<Instant>> = Mutex::new(None);
static FETCH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// What a `sp:` id refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ref {
    Track(String),
    Album(String),
    Artist(String),
    Playlist(String),
    Liked,
    Top,
}

impl Ref {
    pub fn parse(id: &str) -> Option<Self> {
        let rest = id.strip_prefix("sp:")?;
        match rest {
            "liked" => Some(Self::Liked),
            "top" => Some(Self::Top),
            other => {
                if let Some(p) = other.strip_prefix("playlist:") {
                    return Some(Self::Playlist(p.to_owned()));
                }
                if let Some(a) = other.strip_prefix("album:") {
                    return Some(Self::Album(a.to_owned()));
                }
                if let Some(a) = other.strip_prefix("artist:") {
                    return Some(Self::Artist(a.to_owned()));
                }
                if let Some(t) = other.strip_prefix("track:") {
                    return (!t.is_empty() && !t.contains(':')).then(|| Self::Track(t.to_owned()));
                }
                if other.is_empty() || other.contains(':') {
                    return None;
                }
                Some(Self::Track(other.to_owned()))
            }
        }
    }

    pub fn is_track(&self) -> bool {
        matches!(self, Self::Track(_))
    }
}

pub struct SearchPage {
    pub hits: Vec<StreamHit>,
    pub results: SearchResults,
}

pub struct Library {
    pub songs: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
}

/// True while a 429 cooldown is in effect. Callers should paint a homemade
/// page instead of waiting on another request that will fail the same way.
pub fn cooling_down() -> bool {
    cooldown_left().is_some()
}

/// Search tracks, albums, artists and playlists. Never falls back to YouTube.
pub async fn search(http: &reqwest::Client, query: &str) -> Result<SearchPage> {
    let session = session(http).await?;
    if let Ok(page) = search_partner(http, &session, query).await {
        return Ok(page);
    }
    let url = format!(
        "{API}/search?q={}&type=track,album,artist,playlist&limit=20",
        streams::urlencoding(query)
    );
    let value = api_get(http, &session.access, &url).await?;
    Ok(search_page_from_rest(&value))
}

pub fn catalog_entries(page: SearchPage, filter: CatalogFilter) -> (Vec<StreamHit>, Vec<Entry>) {
    let SearchPage { hits, results } = page;
    let (entries, _) = crate::catalog::catalog_rows(filter, results, true);
    (hits, entries)
}

/// Mint (or reuse) a partner session. Library refresh calls this first so a
/// slow TOTP round-trip is not counted as an empty catalogue.
pub async fn warm_session(http: &reqwest::Client) -> Result<()> {
    session(http).await.map(|_| ())
}

/// Line-synced lyrics from `color-lyrics`, the same endpoint the web player
/// uses. 404 means this track has none, not that the session is dead.
pub async fn lyrics(http: &reqwest::Client, id: &str) -> Result<crate::ipc::Lyrics> {
    let track = match Ref::parse(id) {
        Some(Ref::Track(t)) => t,
        _ => anyhow::bail!("{}", i18n::t(Key::LyricsMissing)),
    };
    let session = session(http).await?;
    let url = format!(
        "https://spclient.wg.spotify.com/color-lyrics/v2/track/{track}?format=json&vocalRemoval=false"
    );
    let mut req = http
        .get(&url)
        .bearer_auth(&session.access)
        .header("Accept", "application/json")
        .header("App-Platform", "WebPlayer")
        .header("Spotify-App-Version", &session.client_version);
    if let Some(token) = &session.client_token {
        req = req.header("client-token", token);
    }
    let res = req.send().await.context("spotify lyrics")?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if status.as_u16() == 404 {
        anyhow::bail!("{}", i18n::t(Key::LyricsMissing));
    }
    if !status.is_success() {
        anyhow::bail!("Spotify lyrics {status}");
    }
    let value: Value = serde_json::from_str(&text).context("spotify lyrics json")?;
    lyrics_from_color(&value).ok_or_else(|| anyhow::anyhow!("{}", i18n::t(Key::LyricsMissing)))
}

fn lyrics_from_color(value: &Value) -> Option<crate::ipc::Lyrics> {
    let lyrics = value.get("lyrics")?;
    let sync = lyrics.get("syncType").and_then(Value::as_str).unwrap_or("");
    let lines = lyrics.get("lines").and_then(Value::as_array)?;
    let lines: Vec<crate::ipc::LyricLine> = lines
        .iter()
        .filter_map(|line| {
            let text = line
                .get("words")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != "♪")?;
            let start_ms = line
                .get("startTimeMs")
                .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
                .unwrap_or(0);
            Some(crate::ipc::LyricLine {
                start_ms,
                text: text.to_owned(),
            })
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    Some(crate::ipc::Lyrics {
        synced: sync.eq_ignore_ascii_case("LINE_SYNCED"),
        lines,
    })
}

pub async fn library(http: &reqwest::Client) -> Result<Library> {
    library_progress(http, |_, _, _, _| {}).await
}

/// Fetch one section at a time so a 429 on liked songs does not cancel
/// playlists that already came back. `progress` runs after each non-empty
/// section so the window can draw before the last page lands.
pub async fn library_progress(
    http: &reqwest::Client,
    mut progress: impl FnMut(&[Track], &[Album], &[Artist], &[Playlist]),
) -> Result<Library> {
    let session = session(http).await?;
    let mut playlists =
        library_section("spotify playlists", library_playlists(http, &session, 100)).await;
    let mut songs = Vec::new();
    let mut albums = Vec::new();
    let mut artists = Vec::new();

    if !playlists.is_empty() {
        ensure_liked_playlist(&mut playlists, &songs);
        progress(&songs, &albums, &artists, &playlists);
    }
    if cooling_down() {
        return library_from_parts(songs, albums, artists, playlists);
    }

    songs = library_section("spotify liked songs", library_tracks(http, &session, 200)).await;
    if !songs.is_empty() {
        ensure_liked_playlist(&mut playlists, &songs);
        progress(&songs, &albums, &artists, &playlists);
    }
    if cooling_down() {
        return library_from_parts(songs, albums, artists, playlists);
    }

    albums = library_section("spotify saved albums", library_albums(http, &session, 100)).await;
    if !albums.is_empty() {
        progress(&songs, &albums, &artists, &playlists);
    }
    if cooling_down() {
        return library_from_parts(songs, albums, artists, playlists);
    }

    artists = library_section(
        "spotify followed artists",
        library_artists(http, &session, 50),
    )
    .await;
    library_from_parts(songs, albums, artists, playlists)
}

async fn library_section<T: Default>(
    label: &'static str,
    fut: impl std::future::Future<Output = Result<T>>,
) -> T {
    match fut.await {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(?err, "{label}");
            T::default()
        }
    }
}

fn library_from_parts(
    songs: Vec<Track>,
    albums: Vec<Album>,
    artists: Vec<Artist>,
    mut playlists: Vec<Playlist>,
) -> Result<Library> {
    if songs.is_empty() && albums.is_empty() && artists.is_empty() && playlists.is_empty() {
        anyhow::bail!("Spotify would not return a library for this session");
    }
    ensure_liked_playlist(&mut playlists, &songs);
    Ok(Library {
        songs,
        albums,
        artists,
        playlists,
    })
}

pub async fn discover(http: &reqwest::Client, library: &Library) -> Result<Discover> {
    let session = session(http).await?;
    let home = home_feed(http, &session).await.unwrap_or_else(|err| {
        tracing::warn!(?err, "spotify home feed");
        HomeFeed::default()
    });
    let charts = if home.albums.is_empty() {
        whats_new_albums(http, &session, 16)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(Entry::Album)
            .collect()
    } else {
        home.albums.into_iter().take(16).map(Entry::Album).collect()
    };
    let mut recommended_playlists: Vec<Entry> = home
        .playlists
        .into_iter()
        .take(16)
        .map(Entry::Playlist)
        .collect();
    let mixes: Vec<Entry> = library
        .playlists
        .iter()
        .filter(|p| looks_made_for_you(&p.name) || p.id == "sp:liked" || p.id == "sp:top")
        .cloned()
        .map(Entry::Playlist)
        .collect();
    if recommended_playlists.is_empty() {
        recommended_playlists = mixes;
    } else {
        for mix in mixes.into_iter().rev() {
            recommended_playlists.insert(0, mix);
        }
    }
    let recommended_songs = if home.songs.is_empty() {
        library.songs.iter().take(16).cloned().collect()
    } else {
        home.songs
    };
    let recently_played: Vec<Entry> = if home.recent.is_empty() {
        recommended_songs
            .iter()
            .take(8)
            .cloned()
            .map(Entry::Song)
            .collect()
    } else {
        home.recent.into_iter().map(Entry::Song).collect()
    };
    let recently_added: Vec<Entry> = library
        .playlists
        .iter()
        .filter(|p| p.id != "sp:liked" && p.id != "sp:top" && !looks_made_for_you(&p.name))
        .cloned()
        .map(Entry::Playlist)
        .take(16)
        .collect();
    Ok(Discover::shelves(
        recently_played,
        recommended_playlists,
        recommended_songs,
        recently_added,
        charts,
    ))
}

pub async fn open(
    http: &reqwest::Client,
    _kind: crate::ipc::PageKind,
    id: &str,
) -> Result<(Entry, Vec<Entry>)> {
    let parsed = Ref::parse(id).context("not a Spotify id")?;
    let session = session(http).await?;
    match parsed {
        Ref::Album(album_id) => {
            let (album, tracks) = open_album(http, &session, &album_id).await?;
            Ok((
                Entry::Album(album),
                tracks.into_iter().map(Entry::Song).collect(),
            ))
        }
        Ref::Playlist(playlist_id) => {
            let (list, tracks) = open_playlist(http, &session, &playlist_id).await?;
            Ok((
                Entry::Playlist(list),
                tracks.into_iter().map(Entry::Song).collect(),
            ))
        }
        Ref::Liked => {
            let tracks = library_tracks(http, &session, 200).await?;
            Ok((
                Entry::Playlist(liked_playlist(&tracks)),
                tracks.into_iter().map(Entry::Song).collect(),
            ))
        }
        Ref::Top => {
            let tracks = library_tracks(http, &session, 50).await?;
            Ok((
                Entry::Playlist(top_playlist(&tracks)),
                tracks.into_iter().map(Entry::Song).collect(),
            ))
        }
        Ref::Artist(artist_id) => {
            let (artist, albums) = open_artist(http, &session, &artist_id).await?;
            Ok((
                Entry::Artist(artist),
                albums.into_iter().map(Entry::Album).collect(),
            ))
        }
        Ref::Track(_) => anyhow::bail!("a track does not open a page"),
    }
}

pub async fn track_hit(http: &reqwest::Client, id: &str) -> Result<StreamHit> {
    let session = session(http).await?;
    if let Ok(Some(hit)) = track_from_partner(http, &session, id).await {
        return Ok(hit);
    }
    let value = api_get(http, &session.access, &format!("{API}/tracks/{id}")).await?;
    hit_from_track(&value).context("spotify track")
}

pub fn hits_from_songs(songs: &[Track]) -> Vec<StreamHit> {
    songs.iter().filter_map(StreamHit::from_song).collect()
}

pub fn tracks_from_search(value: &Value) -> Vec<StreamHit> {
    let Some(items) = value.pointer("/tracks/items").and_then(Value::as_array) else {
        return Vec::new();
    };
    items.iter().filter_map(hit_from_track).collect()
}

pub(super) async fn session(http: &reqwest::Client) -> Result<partner::Session> {
    if let Some(cached) = cached_session() {
        return Ok(cached);
    }
    if let Some(wait) = cooldown_left() {
        tracing::warn!(secs = wait.as_secs(), "spotify still rate-limited");
        anyhow::bail!("{}", i18n::t(Key::SpotifyRateLimited));
    }
    let _fetch = FETCH.lock().await;
    if let Some(cached) = cached_session() {
        return Ok(cached);
    }
    if cooldown_left().is_some() {
        anyhow::bail!("{}", i18n::t(Key::SpotifyRateLimited));
    }
    let Some(cookie) = crate::setup::session_cookie(Provider::Spotify) else {
        anyhow::bail!("{}", i18n::t(Key::SpotifyNotSignedIn));
    };
    match fetch_totp_token(http, &cookie).await {
        Ok((session, ttl)) => {
            store_session(session.clone(), ttl);
            Ok(session)
        }
        Err(_) if cooldown_left().is_some() => {
            anyhow::bail!("{}", i18n::t(Key::SpotifyRateLimited));
        }
        Err(err) => Err(err).context(i18n::t(Key::SpotifyTokenRefused)),
    }
}

fn cached_session() -> Option<partner::Session> {
    let guard = TOKEN.lock().ok()?;
    let cached = guard.as_ref()?;
    (cached.expires > Instant::now()).then(|| cached.session.clone())
}

pub fn cooldown_left() -> Option<Duration> {
    let until = *COOLDOWN.lock().ok()?;
    let until = until?;
    let now = Instant::now();
    (until > now).then(|| until.saturating_duration_since(now))
}

pub(super) fn note_rate_limit(wait: Duration) {
    tracing::warn!(secs = wait.as_secs(), "spotify 429 — backing off");
    if let Ok(mut guard) = COOLDOWN.lock() {
        let until = Instant::now() + wait;
        if guard.is_none_or(|was| until > was) {
            *guard = Some(until);
        }
    }
}

pub(super) fn retry_after(headers: &reqwest::header::HeaderMap) -> Duration {
    const DEFAULT: Duration = Duration::from_secs(60);
    let Some(raw) = headers.get("retry-after").and_then(|h| h.to_str().ok()) else {
        return DEFAULT;
    };
    match raw.trim().parse::<u64>() {
        Ok(secs) => Duration::from_secs(secs.clamp(15, 15 * 60)),
        Err(_) => DEFAULT,
    }
}

fn store_session(session: partner::Session, ttl: Duration) {
    tracing::info!(
        client_id = %session.client_id,
        has_client_token = session.client_token.is_some(),
        "spotify partner session ready"
    );
    if let Ok(mut guard) = TOKEN.lock() {
        *guard = Some(CachedToken {
            session,
            expires: Instant::now() + ttl,
        });
    }
}

fn drop_session() {
    if let Ok(mut guard) = TOKEN.lock() {
        *guard = None;
    }
}

async fn fetch_totp_token(
    http: &reqwest::Client,
    cookie: &str,
) -> Result<(partner::Session, Duration)> {
    let secret = totp::current_secret(http).await;
    let server_time = totp::server_time(http).await;
    let otp = totp::totp_from_cipher(&secret.cipher, server_time);
    tracing::info!(ver = secret.version, "requesting spotify web-player token");
    let mut last_err = anyhow::anyhow!("spotify /api/token: no accessToken");
    for reason in ["transport", "init"] {
        if cooldown_left().is_some() {
            anyhow::bail!("{}", i18n::t(Key::SpotifyRateLimited));
        }
        match fetch_player_token(http, cookie, reason, &otp, secret.version, server_time).await {
            Ok((access, false, ttl, client_id)) => {
                let client_version = partner::CLIENT_VERSION.to_owned();
                let client_token = match partner::mint_client_token(
                    http,
                    cookie,
                    &client_id,
                    &client_version,
                )
                .await
                {
                    Ok(token) => Some(token),
                    Err(err) => {
                        tracing::warn!(%err, "spotify client-token failed; Pathfinder may reject requests");
                        None
                    }
                };
                return Ok((
                    partner::Session {
                        access,
                        client_token,
                        client_id,
                        client_version,
                    },
                    ttl,
                ));
            }
            Ok((_, true, _, _)) => {
                last_err = anyhow::anyhow!("spotify /api/token returned an anonymous token");
                tracing::warn!(reason, "spotify totp token was anonymous");
            }
            Err(err) => {
                tracing::warn!(reason, %err, "spotify totp token failed");
                last_err = err;
                if cooldown_left().is_some() {
                    break;
                }
            }
        }
    }
    Err(last_err)
}

async fn fetch_player_token(
    http: &reqwest::Client,
    cookie: &str,
    reason: &str,
    otp: &str,
    totp_ver: u32,
    server_time: u64,
) -> Result<(String, bool, Duration, String)> {
    let ver = totp_ver.to_string();
    let mut pairs: Vec<(&str, String)> = vec![
        ("reason", reason.to_owned()),
        ("productType", "web-player".into()),
        ("totp", otp.to_owned()),
        ("totpServer", otp.to_owned()),
        ("totpVer", ver),
    ];
    if totp_ver < 10 {
        let date = totp::utc_ymd(server_time);
        let client_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let build_ver = format!("web-player_{date}_{}_{:x}", server_time * 1000, client_ms);
        pairs.push(("sTime", server_time.to_string()));
        pairs.push(("cTime", client_ms.to_string()));
        pairs.push(("buildDate", date));
        pairs.push(("buildVer", build_ver));
    }
    send_token(player_token_request(http, PLAYER_TOKEN, cookie).query(&pairs)).await
}

fn player_token_request(
    http: &reqwest::Client,
    url: &str,
    cookie: &str,
) -> reqwest::RequestBuilder {
    http.get(url)
        .header("Accept", "application/json")
        .header("App-Platform", "WebPlayer")
        .header("Referer", "https://open.spotify.com/")
        .header("Origin", "https://open.spotify.com")
        .header("Cookie", cookie)
}

async fn send_token(req: reqwest::RequestBuilder) -> Result<(String, bool, Duration, String)> {
    let res = req.send().await.context("spotify token")?;
    let status = res.status();
    if status.as_u16() == 429 {
        note_rate_limit(retry_after(res.headers()));
        anyhow::bail!("{}", i18n::t(Key::SpotifyRateLimited));
    }
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!(
            %status,
            body = %clip_body(&body),
            "spotify token http error"
        );
        anyhow::bail!("Spotify token {status}");
    }
    let value: Value = serde_json::from_str(&body).context("spotify token json")?;
    let access = value
        .get("accessToken")
        .or_else(|| value.get("access_token"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .context("Spotify token missing accessToken")?;
    let anonymous = value
        .get("isAnonymous")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let expires_ms = value
        .get("accessTokenExpirationTimestampMs")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let ttl = if expires_ms > 0 {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Duration::from_millis(
            expires_ms
                .saturating_sub(now_ms)
                .saturating_sub(60_000)
                .max(30_000),
        )
    } else {
        Duration::from_secs(50 * 60)
    };
    let client_id = value
        .get("clientId")
        .or_else(|| value.get("client_id"))
        .and_then(Value::as_str)
        .unwrap_or(partner::WEB_PLAYER_CLIENT_ID)
        .to_owned();
    Ok((access, anonymous, ttl, client_id))
}

pub(super) fn clip_body(body: &str) -> String {
    const MAX: usize = 400;
    let trimmed = body.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_owned()
    } else {
        let clipped: String = trimmed.chars().take(MAX).collect();
        format!("{clipped}…")
    }
}

async fn api_get(http: &reqwest::Client, token: &str, url: &str) -> Result<Value> {
    let res = http
        .get(url)
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await
        .with_context(|| format!("spotify GET {url}"))?;
    let status = res.status();
    if status.as_u16() == 429 {
        note_rate_limit(retry_after(res.headers()));
        anyhow::bail!("{}", i18n::t(Key::SpotifyRateLimited));
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        tracing::debug!(%status, url, "spotify REST refused this partner token (expected)");
        anyhow::bail!("Spotify REST {status} for {url}");
    }
    if !status.is_success() {
        anyhow::bail!("Spotify {status} for {url}");
    }
    res.json().await.context("spotify json")
}

pub(super) async fn partner_query(
    http: &reqwest::Client,
    session: &partner::Session,
    operation: &str,
    hashes: &[&str],
    variables: Value,
) -> Result<Value> {
    match partner::query(http, session, operation, hashes, variables).await {
        Ok(value) => Ok(value),
        Err(err) => {
            if err.to_string().contains("refused the partner session") {
                drop_session();
            }
            Err(err)
        }
    }
}

async fn library_tracks(
    http: &reqwest::Client,
    session: &partner::Session,
    max: usize,
) -> Result<Vec<Track>> {
    match liked_tracks_partner(http, session, max).await {
        Ok(songs) if !songs.is_empty() => Ok(songs),
        Ok(empty) => {
            if let Ok(rest) = saved_tracks(http, &session.access, max).await
                && !rest.is_empty()
            {
                return Ok(rest);
            }
            Ok(empty)
        }
        Err(err) => {
            tracing::warn!(%err, "spotify fetchLibraryTracks");
            saved_tracks(http, &session.access, max).await
        }
    }
}

async fn liked_tracks_partner(
    http: &reqwest::Client,
    session: &partner::Session,
    max: usize,
) -> Result<Vec<Track>> {
    let mut songs = Vec::new();
    let mut offset = 0usize;
    let limit = partner::page_limit();
    while songs.len() < max {
        let value = partner_query(
            http,
            session,
            "fetchLibraryTracks",
            partner::FETCH_LIBRARY_TRACKS,
            serde_json::json!({ "offset": offset, "limit": limit }),
        )
        .await?;
        let Some(tracks) = value.pointer("/data/me/library/tracks") else {
            anyhow::bail!("fetchLibraryTracks missing me.library.tracks");
        };
        let page = tracks
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if page.is_empty() {
            break;
        }
        for item in &page {
            if let Some(song) = song_from_library_track(item) {
                songs.push(song);
            }
        }
        offset += limit;
        let total = tracks
            .get("totalCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        if offset >= total || page.len() < limit {
            break;
        }
    }
    songs.truncate(max);
    Ok(songs)
}

async fn library_playlists(
    http: &reqwest::Client,
    session: &partner::Session,
    max: usize,
) -> Result<Vec<Playlist>> {
    let mut lists = match library_v3_playlists(http, session, max).await {
        Ok(lists) => lists,
        Err(err) => {
            tracing::warn!(%err, "spotify libraryV3 playlists");
            Vec::new()
        }
    };
    // libraryV3 often returns only Liked Songs / editorial mixes. The
    // rootlist is what the web player actually pins — the user's own lists.
    match write::playlists_in_rootlist(http, session, max).await {
        Ok(root) => merge_playlists(&mut lists, root),
        Err(err) => tracing::debug!(%err, "spotify rootlist playlists"),
    }
    if lists.iter().all(|list| !is_user_playlist(list))
        && let Ok(rest) = user_playlists(http, &session.access, max).await
    {
        merge_playlists(&mut lists, rest);
    }
    lists.truncate(max);
    Ok(lists)
}

fn merge_playlists(into: &mut Vec<Playlist>, extra: Vec<Playlist>) {
    for list in extra {
        if into.iter().all(|have| have.id != list.id) {
            into.push(list);
        }
    }
}

fn is_user_playlist(list: &Playlist) -> bool {
    list.id.starts_with("sp:playlist:") && !list.id.contains("sp:playlist:37i9")
}

async fn library_v3_playlists(
    http: &reqwest::Client,
    session: &partner::Session,
    max: usize,
) -> Result<Vec<Playlist>> {
    let mut lists = library_v3_playlists_with(http, session, max, "Playlists").await?;
    if lists.is_empty() {
        lists = library_v3_playlists_with(http, session, max, "").await?;
    }
    Ok(lists)
}

async fn library_v3_playlists_with(
    http: &reqwest::Client,
    session: &partner::Session,
    max: usize,
    filter: &str,
) -> Result<Vec<Playlist>> {
    let mut lists = Vec::new();
    let mut offset = 0usize;
    let limit = partner::page_limit();
    while lists.len() < max {
        let value = partner_query(
            http,
            session,
            "libraryV3",
            partner::LIBRARY_V3,
            partner::library_v3_vars(filter, offset, limit, true),
        )
        .await?;
        let lib = value
            .pointer("/data/me/libraryV3")
            .cloned()
            .context("libraryV3 missing")?;
        if lib.get("__typename").and_then(Value::as_str) == Some("LibraryInvalidFilterIdError") {
            anyhow::bail!("libraryV3 rejected the Playlists filter");
        }
        let page = lib
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if page.is_empty() {
            break;
        }
        for item in &page {
            if let Some(list) = playlist_from_library_item(item) {
                lists.push(list);
            }
        }
        offset += limit;
        let total = lib.get("totalCount").and_then(Value::as_u64).unwrap_or(0) as usize;
        if offset >= total || page.len() < limit {
            break;
        }
    }
    lists.truncate(max);
    Ok(lists)
}

async fn library_albums(
    http: &reqwest::Client,
    session: &partner::Session,
    max: usize,
) -> Result<Vec<Album>> {
    match library_v3_albums(http, session, max).await {
        Ok(albums) if !albums.is_empty() => Ok(albums),
        Ok(empty) => {
            if let Ok(rest) = saved_albums(http, &session.access, max).await
                && !rest.is_empty()
            {
                return Ok(rest);
            }
            Ok(empty)
        }
        Err(err) => {
            tracing::warn!(%err, "spotify libraryV3 albums");
            saved_albums(http, &session.access, max).await
        }
    }
}

async fn library_v3_albums(
    http: &reqwest::Client,
    session: &partner::Session,
    max: usize,
) -> Result<Vec<Album>> {
    let mut albums = Vec::new();
    let mut offset = 0usize;
    let limit = partner::page_limit();
    while albums.len() < max {
        let value = partner_query(
            http,
            session,
            "libraryV3",
            partner::LIBRARY_V3,
            partner::library_v3_vars("Albums", offset, limit, false),
        )
        .await?;
        let lib = value
            .pointer("/data/me/libraryV3")
            .cloned()
            .context("libraryV3 missing")?;
        if lib.get("__typename").and_then(Value::as_str) == Some("LibraryInvalidFilterIdError") {
            anyhow::bail!("libraryV3 rejected the Albums filter");
        }
        let page = lib
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if page.is_empty() {
            break;
        }
        for item in &page {
            if let Some(album) = album_from_library_item(item) {
                albums.push(album);
            }
        }
        offset += limit;
        let total = lib.get("totalCount").and_then(Value::as_u64).unwrap_or(0) as usize;
        if offset >= total || page.len() < limit {
            break;
        }
    }
    albums.truncate(max);
    Ok(albums)
}

async fn library_artists(
    http: &reqwest::Client,
    session: &partner::Session,
    max: usize,
) -> Result<Vec<Artist>> {
    match library_v3_artists(http, session, max).await {
        Ok(artists) if !artists.is_empty() => Ok(artists),
        Ok(empty) => {
            if let Ok(rest) = followed_artists(http, &session.access, max).await
                && !rest.is_empty()
            {
                return Ok(rest);
            }
            Ok(empty)
        }
        Err(err) => {
            tracing::warn!(%err, "spotify libraryV3 artists");
            followed_artists(http, &session.access, max).await
        }
    }
}

async fn library_v3_artists(
    http: &reqwest::Client,
    session: &partner::Session,
    max: usize,
) -> Result<Vec<Artist>> {
    let mut artists = Vec::new();
    let mut offset = 0usize;
    let limit = partner::page_limit();
    while artists.len() < max {
        let value = partner_query(
            http,
            session,
            "libraryV3",
            partner::LIBRARY_V3,
            partner::library_v3_vars("Artists", offset, limit, false),
        )
        .await?;
        let lib = value
            .pointer("/data/me/libraryV3")
            .cloned()
            .context("libraryV3 missing")?;
        if lib.get("__typename").and_then(Value::as_str) == Some("LibraryInvalidFilterIdError") {
            anyhow::bail!("libraryV3 rejected the Artists filter");
        }
        let page = lib
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if page.is_empty() {
            break;
        }
        for item in &page {
            if let Some(artist) = artist_from_library_item(item) {
                artists.push(artist);
            }
        }
        offset += limit;
        let total = lib.get("totalCount").and_then(Value::as_u64).unwrap_or(0) as usize;
        if offset >= total || page.len() < limit {
            break;
        }
    }
    artists.truncate(max);
    Ok(artists)
}

async fn search_partner(
    http: &reqwest::Client,
    session: &partner::Session,
    query: &str,
) -> Result<SearchPage> {
    let value = partner_query(
        http,
        session,
        "searchDesktop",
        partner::SEARCH_DESKTOP,
        serde_json::json!({
            "searchTerm": query,
            "offset": 0,
            "limit": 20,
            "numberOfTopResults": 5,
            "includeAudiobooks": false,
            "includeArtistHasConcertsField": false,
            "includePreReleases": false,
            "includeLocalConcertsField": false,
            "includeAuthors": false
        }),
    )
    .await?;
    let search = value
        .pointer("/data/searchV2")
        .cloned()
        .unwrap_or(Value::Null);
    let hits = tracks_from_search_v2(&search);
    if hits.is_empty()
        && search
            .pointer("/tracksV2/items")
            .and_then(Value::as_array)
            .is_none()
    {
        anyhow::bail!("searchDesktop returned no tracksV2");
    }
    let songs: Vec<Track> = hits.iter().filter_map(hit_to_song).collect();
    Ok(SearchPage {
        results: SearchResults {
            songs,
            albums: albums_from_search_v2(&search),
            artists: artists_from_search_v2(&search),
            playlists: playlists_from_search_v2(&search),
        },
        hits,
    })
}

fn search_page_from_rest(value: &Value) -> SearchPage {
    let hits = tracks_from_search(value);
    let songs: Vec<Track> = hits.iter().filter_map(hit_to_song).collect();
    SearchPage {
        results: SearchResults {
            songs,
            albums: albums_from_search(value),
            artists: artists_from_search(value),
            playlists: playlists_from_search(value),
        },
        hits,
    }
}

async fn open_playlist(
    http: &reqwest::Client,
    session: &partner::Session,
    id: &str,
) -> Result<(Playlist, Vec<Track>)> {
    match playlist_partner(http, session, id).await {
        Ok(ok) => Ok(ok),
        Err(err) => {
            tracing::warn!(%err, id, "spotify fetchPlaylist");
            playlist(http, &session.access, id).await
        }
    }
}

async fn playlist_partner(
    http: &reqwest::Client,
    session: &partner::Session,
    id: &str,
) -> Result<(Playlist, Vec<Track>)> {
    let mut tracks = Vec::new();
    let mut list = None;
    let mut offset = 0usize;
    let limit = 100usize;
    loop {
        let value = partner_query(
            http,
            session,
            "fetchPlaylist",
            partner::FETCH_PLAYLIST,
            serde_json::json!({
                "uri": format!("spotify:playlist:{id}"),
                "offset": offset,
                "limit": limit,
                "enableWatchFeedEntrypoint": offset == 0
            }),
        )
        .await?;
        let playlist = value
            .pointer("/data/playlistV2")
            .cloned()
            .context("spotify fetchPlaylist missing playlistV2")?;
        if list.is_none() {
            list = playlist_from_gql(&playlist, id);
        }
        let content = playlist.get("content").cloned().unwrap_or(Value::Null);
        let page = content
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if page.is_empty() {
            break;
        }
        for item in &page {
            if let Some(song) = song_from_playlist_item(item) {
                tracks.push(song);
            }
        }
        offset += limit;
        let total = content
            .get("totalCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        if offset >= total || page.len() < limit || tracks.len() >= 500 {
            break;
        }
    }
    Ok((list.context("spotify playlist")?, tracks))
}

async fn open_album(
    http: &reqwest::Client,
    session: &partner::Session,
    id: &str,
) -> Result<(Album, Vec<Track>)> {
    match album_partner(http, session, id).await {
        Ok(ok) => Ok(ok),
        Err(err) => {
            tracing::warn!(%err, id, "spotify getAlbum");
            album(http, &session.access, id).await
        }
    }
}

async fn album_partner(
    http: &reqwest::Client,
    session: &partner::Session,
    id: &str,
) -> Result<(Album, Vec<Track>)> {
    let value = partner_query(
        http,
        session,
        "getAlbum",
        partner::GET_ALBUM,
        serde_json::json!({
            "uri": format!("spotify:album:{id}"),
            "locale": "",
            "offset": 0,
            "limit": 300
        }),
    )
    .await?;
    let union = value
        .pointer("/data/albumUnion")
        .cloned()
        .context("spotify getAlbum missing albumUnion")?;
    let album = album_from_gql(&union, id).context("spotify album")?;
    let mut tracks = Vec::new();
    if let Some(items) = union.pointer("/tracksV2/items").and_then(Value::as_array) {
        for (i, item) in items.iter().enumerate() {
            let track = item.get("track").unwrap_or(item);
            if let Some(mut song) = song_from_gql(track, String::new(), false, false) {
                if song.track_number == 0 {
                    song.track_number = (i as u32) + 1;
                }
                if song.album.is_empty() {
                    song.album = album.name.clone();
                }
                if song.artwork.is_none() {
                    song.artwork = album.artwork.clone();
                }
                tracks.push(song);
            }
        }
    }
    Ok((album, tracks))
}

async fn open_artist(
    http: &reqwest::Client,
    session: &partner::Session,
    id: &str,
) -> Result<(Artist, Vec<Album>)> {
    match artist_partner(http, session, id).await {
        Ok(ok) => Ok(ok),
        Err(err) => {
            tracing::warn!(%err, id, "spotify queryArtistOverview");
            artist_albums(http, &session.access, id).await
        }
    }
}

async fn artist_partner(
    http: &reqwest::Client,
    session: &partner::Session,
    id: &str,
) -> Result<(Artist, Vec<Album>)> {
    let value = partner_query(
        http,
        session,
        "queryArtistOverview",
        partner::ARTIST_OVERVIEW,
        serde_json::json!({
            "uri": format!("spotify:artist:{id}"),
            "locale": "",
            "preReleaseV2": false
        }),
    )
    .await?;
    let union = value
        .pointer("/data/artistUnion")
        .cloned()
        .context("spotify queryArtistOverview missing artistUnion")?;
    let artist = artist_from_gql(&union, id).context("spotify artist")?;
    Ok((artist, albums_from_artist_union(&union)))
}

async fn track_from_partner(
    http: &reqwest::Client,
    session: &partner::Session,
    id: &str,
) -> Result<Option<StreamHit>> {
    let value = partner_query(
        http,
        session,
        "getTrack",
        partner::GET_TRACK,
        serde_json::json!({ "uri": format!("spotify:track:{id}") }),
    )
    .await?;
    let track = value
        .pointer("/data/trackUnion")
        .or_else(|| value.pointer("/data/track"))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(hit_from_gql_track(&track))
}

#[derive(Default)]
struct HomeFeed {
    playlists: Vec<Playlist>,
    albums: Vec<Album>,
    songs: Vec<Track>,
    recent: Vec<Track>,
}

async fn home_feed(http: &reqwest::Client, session: &partner::Session) -> Result<HomeFeed> {
    let value = partner_query(
        http,
        session,
        "home",
        partner::HOME,
        serde_json::json!({
            "homeEndUserIntegration": "INTEGRATION_WEB_PLAYER",
            "timeZone": "Europe/Madrid",
            "sp_t": "",
            "facet": "",
            "sectionItemsLimit": 10,
            "includeEpisodeContentRatingsV2": false
        }),
    )
    .await?;
    let sections = value
        .pointer("/data/home/sectionContainer/sections/items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut feed = HomeFeed::default();
    for section in sections {
        let title = section
            .pointer("/data/title/transformedLabel")
            .or_else(|| section.pointer("/data/title/text"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        let items = section
            .pointer("/sectionItems/items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for item in items {
            let content = item.get("content").unwrap_or(&item);
            let typename = content
                .get("__typename")
                .and_then(Value::as_str)
                .unwrap_or("");
            let data = content.get("data").unwrap_or(content);
            if typename.contains("Playlist")
                || typename.contains("Radio")
                || typename.contains("Station")
                || typename.contains("Blend")
            {
                if let Some(list) = playlist_from_gql(data, "") {
                    feed.playlists.push(list);
                }
            } else if typename.contains("Album") {
                if let Some(album) = album_from_gql(data, "") {
                    feed.albums.push(album);
                }
            } else if typename.contains("Track")
                && let Some(song) = song_from_gql(data, String::new(), false, false)
            {
                if title.contains("jump") || title.contains("recent") || title.contains("vuelve") {
                    feed.recent.push(song);
                } else {
                    feed.songs.push(song);
                }
            }
        }
    }
    Ok(feed)
}

async fn whats_new_albums(
    http: &reqwest::Client,
    session: &partner::Session,
    max: usize,
) -> Result<Vec<Album>> {
    let value = partner_query(
        http,
        session,
        "queryWhatsNewFeed",
        partner::WHATS_NEW,
        serde_json::json!({
            "offset": 0,
            "limit": max,
            "onlyUnPlayedItems": false,
            "includedContentTypes": ["ALBUM"]
        }),
    )
    .await?;
    let items = value
        .pointer("/data/whatsNewFeedItems/items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(items
        .iter()
        .filter_map(|item| {
            let content = item.get("content")?;
            album_from_gql(content.get("data").unwrap_or(content), "")
        })
        .take(max)
        .collect())
}

fn song_from_library_track(item: &Value) -> Option<Track> {
    let wrapper = item.get("track").or_else(|| item.get("item"))?;
    let data = wrapper.get("data").unwrap_or(wrapper);
    let added = item
        .pointer("/addedAt/isoString")
        .or_else(|| item.get("addedAt"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    song_from_gql(data, added, true, true)
}

fn song_from_playlist_item(item: &Value) -> Option<Track> {
    let wrapper = item
        .get("itemV2")
        .or_else(|| item.get("item"))
        .unwrap_or(item);
    if let Some(typename) = wrapper.get("__typename").and_then(Value::as_str)
        && !typename.contains("Track")
        && typename != "TrackResponseWrapper"
    {
        return None;
    }
    let data = wrapper.get("data").unwrap_or(wrapper);
    if data.get("__typename").and_then(Value::as_str) == Some("NotFound") {
        return None;
    }
    let added = item
        .pointer("/addedAt/isoString")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    song_from_gql(data, added, false, false)
}

fn playlist_from_library_item(item: &Value) -> Option<Playlist> {
    let mut cursor = item;
    for _ in 0..5 {
        if let Some(list) = playlist_from_library_node(cursor) {
            return Some(list);
        }
        cursor = cursor.get("item")?;
    }
    None
}

fn playlist_from_library_node(node: &Value) -> Option<Playlist> {
    let wrapper = node.get("item").unwrap_or(node);
    let data = wrapper.get("data").unwrap_or(wrapper);
    if data.get("__typename").and_then(Value::as_str) == Some("NotFound") {
        return None;
    }
    let uri = wrapper
        .get("_uri")
        .or_else(|| wrapper.get("uri"))
        .or_else(|| data.get("uri"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if uri.contains(":folder:") {
        return None;
    }
    if uri.contains(":collection:tracks") {
        return Some(liked_playlist(&[]));
    }
    if uri.contains(":collection:") {
        return None;
    }
    let typename = format!(
        "{}{}",
        wrapper
            .get("__typename")
            .and_then(Value::as_str)
            .unwrap_or(""),
        data.get("__typename").and_then(Value::as_str).unwrap_or("")
    );
    if !typename.contains("Playlist") && !uri.contains(":playlist:") {
        return None;
    }
    let id = uri_tail(uri);
    playlist_from_gql(data, &id).map(|mut list| {
        list.library = true;
        list
    })
}

fn album_from_library_item(item: &Value) -> Option<Album> {
    let wrapper = item.get("item").unwrap_or(item);
    let typename = wrapper
        .get("__typename")
        .and_then(Value::as_str)
        .unwrap_or("");
    let data = wrapper.get("data").unwrap_or(wrapper);
    if !typename.contains("Album")
        && !data
            .get("__typename")
            .and_then(Value::as_str)
            .is_some_and(|t| t.contains("Album"))
    {
        return None;
    }
    album_from_gql(data, "").map(|mut album| {
        album.library = true;
        album.date_added = item
            .pointer("/addedAt/isoString")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        album
    })
}

fn artist_from_library_item(item: &Value) -> Option<Artist> {
    let wrapper = item.get("item")?;
    let typename = wrapper
        .get("__typename")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !typename.contains("Artist") {
        return None;
    }
    artist_from_gql(wrapper.get("data").unwrap_or(wrapper), "")
}

fn tracks_from_search_v2(search: &Value) -> Vec<StreamHit> {
    search
        .pointer("/tracksV2/items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let wrapper = item.get("item").unwrap_or(item);
                    let data = wrapper.get("data").unwrap_or(wrapper);
                    hit_from_gql_track(data)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn albums_from_search_v2(search: &Value) -> Vec<Album> {
    let items = search
        .pointer("/albumsV2/items")
        .or_else(|| search.pointer("/albums/items"))
        .and_then(Value::as_array);
    items
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let data = item.get("data").unwrap_or(item);
                    album_from_gql(data, "")
                })
                .collect()
        })
        .unwrap_or_default()
}

fn artists_from_search_v2(search: &Value) -> Vec<Artist> {
    search
        .pointer("/artists/items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let data = item.get("data").unwrap_or(item);
                    artist_from_gql(data, "")
                })
                .collect()
        })
        .unwrap_or_default()
}

fn playlists_from_search_v2(search: &Value) -> Vec<Playlist> {
    search
        .pointer("/playlists/items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let data = item.get("data").unwrap_or(item);
                    playlist_from_gql(data, "")
                })
                .collect()
        })
        .unwrap_or_default()
}

fn song_from_gql(
    data: &Value,
    date_added: String,
    favorite: bool,
    in_library: bool,
) -> Option<Track> {
    let hit = hit_from_gql_track(data)?;
    let mut song = hit_to_song(&hit)?;
    song.date_added = date_added;
    song.favorite = favorite;
    song.in_library = in_library;
    song.track_number = data
        .get("trackNumber")
        .or_else(|| data.get("track_number"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    song.year = year_of(&gql_date(data));
    Some(song)
}

fn hit_from_gql_track(data: &Value) -> Option<StreamHit> {
    if data.is_null() {
        return None;
    }
    let typename = data.get("__typename").and_then(Value::as_str).unwrap_or("");
    if typename == "NotFound" || typename.contains("Episode") {
        return None;
    }
    let uri = data
        .get("uri")
        .or_else(|| data.get("_uri"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let id = if uri.is_empty() {
        data.get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    } else {
        uri_tail(uri)
    };
    if id.is_empty() || id.contains(':') {
        return None;
    }
    let title = text(data, "name");
    if title.is_empty() {
        return None;
    }
    Some(StreamHit {
        play_query: format!("https://open.spotify.com/track/{id}"),
        artwork: gql_cover(data),
        duration_ms: gql_duration_ms(data),
        album: gql_album_name(data),
        artist: gql_artists_line(data),
        title,
        id: format!("sp:{id}"),
    })
}

fn playlist_from_gql(data: &Value, fallback_id: &str) -> Option<Playlist> {
    if data.is_null() {
        return None;
    }
    let uri = data.get("uri").and_then(Value::as_str).unwrap_or("");
    let mut id = uri_tail(uri);
    if id.is_empty() {
        id = fallback_id.to_owned();
    }
    if id.is_empty() {
        return None;
    }
    let name = gql_label(data, "name");
    let name = if name.is_empty() {
        gql_label(data, "title")
    } else {
        name
    };
    if name.is_empty() {
        return None;
    }
    Some(Playlist {
        id: format!("sp:playlist:{id}"),
        date_added: String::new(),
        last_modified: String::new(),
        name,
        curator: data
            .pointer("/ownerV2/data/name")
            .or_else(|| data.pointer("/owner/display_name"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        description: text(data, "description"),
        artwork: gql_playlist_cover(data)
            .or_else(|| gql_cover(data))
            .map(Artwork::new),
        library: false,
    })
}

fn album_from_gql(data: &Value, fallback_id: &str) -> Option<Album> {
    if data.is_null() {
        return None;
    }
    let uri = data.get("uri").and_then(Value::as_str).unwrap_or("");
    let mut id = uri_tail(uri);
    if id.is_empty() {
        id = fallback_id.to_owned();
    }
    if id.is_empty() {
        id = data
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
    }
    if id.is_empty() {
        return None;
    }
    let name = text(data, "name");
    if name.is_empty() {
        return None;
    }
    Some(Album {
        id: format!("sp:album:{id}"),
        date_added: String::new(),
        name,
        artist: gql_artists_line(data),
        artwork: gql_cover(data).map(Artwork::new),
        year: year_of(&gql_date(data)),
        track_count: data
            .pointer("/tracksV2/totalCount")
            .or_else(|| data.get("total_tracks"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        library: false,
    })
}

fn artist_from_gql(data: &Value, fallback_id: &str) -> Option<Artist> {
    if data.is_null() {
        return None;
    }
    let uri = data.get("uri").and_then(Value::as_str).unwrap_or("");
    let mut id = uri_tail(uri);
    if id.is_empty() {
        id = fallback_id.to_owned();
    }
    if id.is_empty() {
        id = data
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
    }
    let name = data
        .pointer("/profile/name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| text(data, "name"));
    if id.is_empty() || name.is_empty() {
        return None;
    }
    Some(Artist {
        id: format!("sp:artist:{id}"),
        name,
        artwork: gql_cover(data).map(Artwork::new),
        genres: String::new(),
        library: true,
    })
}

fn albums_from_artist_union(union: &Value) -> Vec<Album> {
    let mut albums = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let pointers = [
        "/discography/popularReleasesAlbums/items",
        "/discography/albums/items",
        "/discography/singles/items",
        "/discography/topAlbums/items",
    ];
    for pointer in pointers {
        let Some(items) = union.pointer(pointer).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let releases = item
                .pointer("/releases/items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_else(|| vec![item.clone()]);
            for release in releases {
                let data = release.get("data").unwrap_or(&release);
                if let Some(album) = album_from_gql(data, "")
                    && seen.insert(album.id.clone())
                {
                    albums.push(album);
                }
            }
        }
    }
    albums
}

fn gql_artists_line(data: &Value) -> String {
    if let Some(items) = data.pointer("/artists/items").and_then(Value::as_array) {
        let names: Vec<&str> = items
            .iter()
            .filter_map(|a| {
                a.pointer("/profile/name")
                    .or_else(|| a.get("name"))
                    .and_then(Value::as_str)
            })
            .collect();
        if !names.is_empty() {
            return names.join(", ");
        }
    }
    artists_line(data)
}

fn gql_album_name(data: &Value) -> String {
    data.pointer("/albumOfTrack/name")
        .or_else(|| data.pointer("/album/name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn gql_duration_ms(data: &Value) -> u64 {
    data.pointer("/duration/totalMilliseconds")
        .or_else(|| data.pointer("/trackDuration/totalMilliseconds"))
        .or_else(|| data.get("durationMs"))
        .or_else(|| data.get("duration_ms"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn gql_cover(data: &Value) -> Option<String> {
    cover_from_sources(data.pointer("/albumOfTrack/coverArt/sources"))
        .or_else(|| cover_from_sources(data.pointer("/coverArt/sources")))
        .or_else(|| cover_from_sources(data.pointer("/visuals/avatarImage/sources")))
        .or_else(|| image_url(data.pointer("/album/images")))
        .or_else(|| image_url(data.get("images")))
}

fn gql_playlist_cover(data: &Value) -> Option<String> {
    data.pointer("/images/items")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| cover_from_sources(item.get("sources")))
}

fn cover_from_sources(sources: Option<&Value>) -> Option<String> {
    let arr = sources.and_then(Value::as_array)?;
    arr.iter()
        .max_by_key(|s| s.get("height").and_then(Value::as_u64).unwrap_or(0))
        .and_then(|s| s.get("url"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            arr.first()
                .and_then(|s| s.get("url"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

fn gql_date(data: &Value) -> String {
    data.pointer("/albumOfTrack/date/isoString")
        .or_else(|| data.pointer("/date/isoString"))
        .or_else(|| data.pointer("/album/release_date"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            data.pointer("/date/year")
                .and_then(Value::as_u64)
                .map(|y| y.to_string())
        })
        .unwrap_or_default()
}

pub(super) fn uri_tail(uri: &str) -> String {
    uri.rsplit(':').next().unwrap_or("").to_owned()
}

async fn saved_tracks(http: &reqwest::Client, token: &str, max: usize) -> Result<Vec<Track>> {
    let items = collect_items(http, token, &format!("{API}/me/tracks?limit=50"), max).await?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let added = text(item, "added_at");
            let track = item.get("track")?;
            song_from_track(track, added, true, true)
        })
        .collect())
}

async fn saved_albums(http: &reqwest::Client, token: &str, max: usize) -> Result<Vec<Album>> {
    let items = collect_items(http, token, &format!("{API}/me/albums?limit=50"), max).await?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let added = text(item, "added_at");
            let album = item.get("album")?;
            album_from_object(album, added, true)
        })
        .collect())
}

async fn user_playlists(http: &reqwest::Client, token: &str, max: usize) -> Result<Vec<Playlist>> {
    let items = collect_items(http, token, &format!("{API}/me/playlists?limit=50"), max).await?;
    Ok(items
        .iter()
        .filter_map(|item| playlist_from_object(item, true))
        .collect())
}

async fn followed_artists(http: &reqwest::Client, token: &str, max: usize) -> Result<Vec<Artist>> {
    let mut url = format!("{API}/me/following?type=artist&limit=50");
    let mut artists = Vec::new();
    while artists.len() < max && !url.is_empty() {
        let value = api_get(http, token, &url).await?;
        let page = value.pointer("/artists/items").and_then(Value::as_array);
        let Some(page) = page else {
            break;
        };
        if page.is_empty() {
            break;
        }
        artists.extend(page.iter().filter_map(artist_from_object));
        url = value
            .pointer("/artists/next")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
    }
    artists.truncate(max);
    Ok(artists)
}

async fn album(http: &reqwest::Client, token: &str, id: &str) -> Result<(Album, Vec<Track>)> {
    let value = api_get(http, token, &format!("{API}/albums/{id}?market=from_token")).await?;
    let album = album_from_object(&value, String::new(), false).context("spotify album")?;
    let mut tracks = Vec::new();
    if let Some(items) = value.pointer("/tracks/items").and_then(Value::as_array) {
        for (i, item) in items.iter().enumerate() {
            if let Some(mut song) = song_from_track_on_album(item, &value) {
                if song.track_number == 0 {
                    song.track_number = (i as u32) + 1;
                }
                tracks.push(song);
            }
        }
    }
    let mut next = value
        .pointer("/tracks/next")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    while !next.is_empty() && tracks.len() < 500 {
        let page = api_get(http, token, &next).await?;
        if let Some(items) = page.get("items").and_then(Value::as_array) {
            for item in items {
                if let Some(song) = song_from_track_on_album(item, &value) {
                    tracks.push(song);
                }
            }
        }
        next = page
            .get("next")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
    }
    Ok((album, tracks))
}

async fn playlist(http: &reqwest::Client, token: &str, id: &str) -> Result<(Playlist, Vec<Track>)> {
    let value = api_get(
        http,
        token,
        &format!("{API}/playlists/{id}?market=from_token"),
    )
    .await?;
    let list = playlist_from_object(&value, false).context("spotify playlist")?;
    let mut tracks = Vec::new();
    if let Some(items) = value.pointer("/tracks/items").and_then(Value::as_array) {
        for item in items {
            if let Some(track) = item.get("track").or_else(|| item.get("item"))
                && let Some(song) = song_from_track(track, text(item, "added_at"), false, true)
            {
                tracks.push(song);
            }
        }
    }
    let mut next = value
        .pointer("/tracks/next")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    while !next.is_empty() && tracks.len() < 500 {
        let page = api_get(http, token, &next).await?;
        if let Some(items) = page.get("items").and_then(Value::as_array) {
            for item in items {
                if let Some(track) = item.get("track").or_else(|| item.get("item"))
                    && let Some(song) = song_from_track(track, text(item, "added_at"), false, true)
                {
                    tracks.push(song);
                }
            }
        }
        next = page
            .get("next")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
    }
    Ok((list, tracks))
}

async fn artist_albums(
    http: &reqwest::Client,
    token: &str,
    id: &str,
) -> Result<(Artist, Vec<Album>)> {
    let artist_json = api_get(http, token, &format!("{API}/artists/{id}")).await?;
    let artist = artist_from_object(&artist_json).context("spotify artist")?;
    let albums_json = api_get(
        http,
        token,
        &format!("{API}/artists/{id}/albums?include_groups=album,single&limit=50"),
    )
    .await?;
    let albums = albums_json
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| album_from_object(item, String::new(), false))
                .collect()
        })
        .unwrap_or_default();
    Ok((artist, albums))
}

async fn collect_items(
    http: &reqwest::Client,
    token: &str,
    start: &str,
    max: usize,
) -> Result<Vec<Value>> {
    let mut url = start.to_owned();
    let mut items = Vec::new();
    while items.len() < max && !url.is_empty() {
        let value = api_get(http, token, &url).await?;
        let Some(page) = value.get("items").and_then(Value::as_array) else {
            break;
        };
        if page.is_empty() {
            break;
        }
        items.extend(page.iter().cloned());
        url = value
            .get("next")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
    }
    items.truncate(max);
    Ok(items)
}

fn hit_from_track(item: &Value) -> Option<StreamHit> {
    if item.get("is_local").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let id = item.get("id")?.as_str()?.to_owned();
    if id.is_empty() {
        return None;
    }
    let title = text(item, "name");
    if title.is_empty() {
        return None;
    }
    Some(StreamHit {
        play_query: format!("https://open.spotify.com/track/{id}"),
        artwork: image_url(item.pointer("/album/images")).or_else(|| image_url(item.get("images"))),
        duration_ms: item.get("duration_ms").and_then(Value::as_u64).unwrap_or(0),
        album: item
            .pointer("/album/name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        artist: artists_line(item),
        title,
        id: format!("sp:{id}"),
    })
}

fn hit_to_song(hit: &StreamHit) -> Option<Track> {
    Some(Track {
        date_added: String::new(),
        year: String::new(),
        favorite: false,
        in_library: false,
        library_id: Some(hit.id.clone()),
        id: TrackId(hit.id.clone()),
        catalog_id: Some(hit.id.clone()),
        title: hit.title.clone(),
        artist: hit.artist.clone(),
        album: hit.album.clone(),
        duration_ms: hit.duration_ms,
        track_number: 0,
        artwork: hit.artwork.clone().map(Artwork::new),
    })
}

fn song_from_track(
    item: &Value,
    date_added: String,
    favorite: bool,
    in_library: bool,
) -> Option<Track> {
    let hit = hit_from_track(item)?;
    let mut song = hit_to_song(&hit)?;
    song.date_added = date_added;
    song.favorite = favorite;
    song.in_library = in_library;
    song.track_number = item
        .get("track_number")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    song.year = item
        .pointer("/album/release_date")
        .and_then(Value::as_str)
        .map(year_of)
        .unwrap_or_default();
    Some(song)
}

fn song_from_track_on_album(item: &Value, album: &Value) -> Option<Track> {
    let mut merged = item.clone();
    if merged.get("album").is_none()
        && let Some(obj) = merged.as_object_mut()
    {
        obj.insert("album".into(), album.clone());
    }
    song_from_track(&merged, String::new(), false, false)
}

fn album_from_object(item: &Value, date_added: String, library: bool) -> Option<Album> {
    if item.is_null() {
        return None;
    }
    let id = item.get("id")?.as_str()?.to_owned();
    Some(Album {
        id: format!("sp:album:{id}"),
        date_added,
        name: text(item, "name"),
        artist: artists_line(item),
        artwork: image_url(item.get("images")).map(Artwork::new),
        year: year_of(&text(item, "release_date")),
        track_count: item
            .get("total_tracks")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        library,
    })
}

fn playlist_from_object(item: &Value, library: bool) -> Option<Playlist> {
    if item.is_null() {
        return None;
    }
    let id = item.get("id")?.as_str()?.to_owned();
    Some(Playlist {
        id: format!("sp:playlist:{id}"),
        date_added: String::new(),
        last_modified: text(item, "snapshot_id"),
        name: text(item, "name"),
        curator: item
            .pointer("/owner/display_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        description: text(item, "description"),
        artwork: image_url(item.get("images")).map(Artwork::new),
        library,
    })
}

fn artist_from_object(item: &Value) -> Option<Artist> {
    if item.is_null() {
        return None;
    }
    let id = item.get("id")?.as_str()?.to_owned();
    let genres = item
        .get("genres")
        .and_then(Value::as_array)
        .map(|g| {
            g.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    Some(Artist {
        id: format!("sp:artist:{id}"),
        name: text(item, "name"),
        artwork: image_url(item.get("images")).map(Artwork::new),
        genres,
        library: true,
    })
}

fn albums_from_search(value: &Value) -> Vec<Album> {
    value
        .pointer("/albums/items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| album_from_object(item, String::new(), false))
                .collect()
        })
        .unwrap_or_default()
}

fn artists_from_search(value: &Value) -> Vec<Artist> {
    value
        .pointer("/artists/items")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(artist_from_object).collect())
        .unwrap_or_default()
}

fn playlists_from_search(value: &Value) -> Vec<Playlist> {
    value
        .pointer("/playlists/items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| playlist_from_object(item, false))
                .collect()
        })
        .unwrap_or_default()
}

fn liked_playlist(songs: &[Track]) -> Playlist {
    Playlist {
        id: "sp:liked".into(),
        date_added: String::new(),
        last_modified: String::new(),
        name: i18n::t(Key::LikedSongs).to_owned(),
        curator: String::new(),
        description: String::new(),
        artwork: songs.first().and_then(|s| s.artwork.clone()),
        library: true,
    }
}

fn ensure_liked_playlist(playlists: &mut Vec<Playlist>, songs: &[Track]) {
    if let Some(pos) = playlists.iter().position(|p| p.id == "sp:liked") {
        if playlists[pos].name.is_empty() {
            playlists[pos].name = i18n::t(Key::LikedSongs).to_owned();
        }
        if playlists[pos].artwork.is_none() {
            playlists[pos].artwork = songs.first().and_then(|s| s.artwork.clone());
        }
        playlists[pos].library = true;
        if pos != 0 {
            let liked = playlists.remove(pos);
            playlists.insert(0, liked);
        }
        return;
    }
    playlists.insert(0, liked_playlist(songs));
}

fn top_playlist(songs: &[Track]) -> Playlist {
    Playlist {
        id: "sp:top".into(),
        date_added: String::new(),
        last_modified: String::new(),
        name: i18n::t(Key::YourTopTracks).to_owned(),
        curator: String::new(),
        description: String::new(),
        artwork: songs.first().and_then(|s| s.artwork.clone()),
        library: true,
    }
}

fn looks_made_for_you(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("discover weekly")
        || n.contains("release radar")
        || n.contains("daily mix")
        || n.contains("on repeat")
        || n.contains("repeat rewind")
        || n.contains("tu mezcla")
        || n.contains("radar de novedades")
        || n.contains("descubrir")
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn gql_label(value: &Value, key: &str) -> String {
    let Some(field) = value.get(key) else {
        return String::new();
    };
    if let Some(s) = field.as_str() {
        return s.to_owned();
    }
    field
        .get("transformedLabel")
        .or_else(|| field.get("text"))
        .or_else(|| field.get("transformedName"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn artists_line(item: &Value) -> String {
    item.get("artists")
        .and_then(Value::as_array)
        .map(|artists| {
            artists
                .iter()
                .filter_map(|a| a.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn image_url(images: Option<&Value>) -> Option<String> {
    images
        .and_then(Value::as_array)
        .and_then(|imgs| imgs.first())
        .and_then(|img| img.get("url"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn year_of(date: &str) -> String {
    date.get(..4).unwrap_or("").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_album_and_liked_ids_round_trip() {
        assert_eq!(Ref::parse("sp:abc"), Some(Ref::Track("abc".into())));
        assert_eq!(Ref::parse("sp:track:abc"), Some(Ref::Track("abc".into())));
        assert_eq!(Ref::parse("sp:album:x"), Some(Ref::Album("x".into())));
        assert_eq!(Ref::parse("sp:playlist:y"), Some(Ref::Playlist("y".into())));
        assert_eq!(Ref::parse("sp:liked"), Some(Ref::Liked));
        assert_eq!(Ref::parse("yt:abc"), None);
        assert!(!Ref::parse("sp:playlist:y").unwrap().is_track());
    }

    #[test]
    fn a_fresh_process_is_not_rate_limited() {
        assert!(!cooling_down());
    }

    #[test]
    fn search_json_keeps_songs_and_playlists() {
        let json = serde_json::json!({
            "tracks": {"items": [{
                "id": "abc",
                "name": "Pa Mal",
                "duration_ms": 180000,
                "artists": [{"name": "Aitana"}],
                "album": {
                    "name": "Alpha",
                    "images": [{"url": "https://i.scdn.co/image/x"}]
                }
            }]},
            "playlists": {"items": [{
                "id": "pl1",
                "name": "Gym",
                "owner": {"display_name": "Aitana"},
                "images": [{"url": "https://i.scdn.co/image/p"}]
            }]}
        });
        let hits = tracks_from_search(&json);
        assert_eq!(hits[0].id, "sp:abc");
        let lists = playlists_from_search(&json);
        assert_eq!(lists[0].id, "sp:playlist:pl1");
        assert_eq!(lists[0].curator, "Aitana");
    }

    #[test]
    fn saved_track_wrapper_becomes_a_library_song() {
        let item = serde_json::json!({
            "added_at": "2024-01-02T00:00:00Z",
            "track": {
                "id": "t1",
                "name": "Superestrella",
                "duration_ms": 200000,
                "track_number": 3,
                "artists": [{"name": "Aitana"}],
                "album": {"name": "Alpha", "release_date": "2023-01-01", "images": []}
            }
        });
        let song = song_from_track(
            item.get("track").unwrap(),
            text(&item, "added_at"),
            true,
            true,
        )
        .unwrap();
        assert_eq!(song.catalog_id.as_deref(), Some("sp:t1"));
        assert_eq!(song.title, "Superestrella");
        assert!(song.favorite);
        assert_eq!(song.track_number, 3);
        assert_eq!(song.year, "2023");
    }

    #[test]
    fn retry_after_reads_seconds_and_clamps() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "30".parse().unwrap());
        assert_eq!(retry_after(&headers), Duration::from_secs(30));
        headers.insert("retry-after", "5".parse().unwrap());
        assert_eq!(retry_after(&headers), Duration::from_secs(15));
        headers.insert("retry-after", "99999".parse().unwrap());
        assert_eq!(retry_after(&headers), Duration::from_secs(15 * 60));
        headers.clear();
        assert_eq!(retry_after(&headers), Duration::from_secs(60));
    }

    #[test]
    fn pathfinder_liked_song_becomes_a_library_track() {
        let item = serde_json::json!({
            "addedAt": {"isoString": "2026-01-02T00:00:00Z"},
            "track": {
                "_uri": "spotify:track:t1",
                "data": {
                    "__typename": "Track",
                    "uri": "spotify:track:t1",
                    "name": "Superestrella",
                    "trackDuration": {"totalMilliseconds": 200000},
                    "artists": {"items": [{"profile": {"name": "Aitana"}}]},
                    "albumOfTrack": {
                        "name": "Alpha",
                        "date": {"isoString": "2023-01-01"},
                        "coverArt": {"sources": [{"url": "https://i.scdn.co/image/x", "height": 300}]}
                    }
                }
            }
        });
        let song = song_from_library_track(&item).unwrap();
        assert_eq!(song.catalog_id.as_deref(), Some("sp:t1"));
        assert_eq!(song.title, "Superestrella");
        assert_eq!(song.artist, "Aitana");
        assert_eq!(song.album, "Alpha");
        assert_eq!(song.duration_ms, 200000);
        assert!(song.favorite);
        assert_eq!(song.year, "2023");
        assert_eq!(
            song.artwork.as_ref().unwrap().url(300),
            "https://i.scdn.co/image/x"
        );
    }

    #[test]
    fn pathfinder_library_playlists_skip_collection_uris() {
        let liked = serde_json::json!({
            "item": {
                "__typename": "PlaylistResponseWrapper",
                "_uri": "spotify:user:me:collection:tracks",
                "data": { "__typename": "Playlist", "name": "Liked Songs", "uri": "spotify:user:me:collection:tracks" }
            }
        });
        let liked = playlist_from_library_item(&liked).unwrap();
        assert_eq!(liked.id, "sp:liked");
        assert!(liked.library);
        let item = serde_json::json!({
            "item": {
                "__typename": "PlaylistResponseWrapper",
                "_uri": "spotify:playlist:pl1",
                "data": {
                    "__typename": "Playlist",
                    "uri": "spotify:playlist:pl1",
                    "name": "Gym",
                    "ownerV2": {"data": {"name": "Aitana"}},
                    "images": {"items": [{"sources": [{"url": "https://i.scdn.co/image/p", "height": 64}]}]}
                }
            }
        });
        let list = playlist_from_library_item(&item).unwrap();
        assert_eq!(list.id, "sp:playlist:pl1");
        assert_eq!(list.curator, "Aitana");
        assert!(list.library);
        assert_eq!(
            list.artwork.as_ref().unwrap().url(64),
            "https://i.scdn.co/image/p"
        );
        let flat = serde_json::json!({
            "__typename": "PlaylistResponseWrapper",
            "uri": "spotify:playlist:pl2",
            "data": {
                "__typename": "Playlist",
                "uri": "spotify:playlist:pl2",
                "name": "Drive"
            }
        });
        let list = playlist_from_library_item(&flat).unwrap();
        assert_eq!(list.id, "sp:playlist:pl2");
        assert!(list.library);
        let nested = serde_json::json!({
            "item": {
                "__typename": "LibraryPaginatableItem",
                "item": {
                    "__typename": "PlaylistResponseWrapper",
                    "_uri": "spotify:playlist:pl3",
                    "data": {
                        "__typename": "Playlist",
                        "uri": "spotify:playlist:pl3",
                        "name": "Noche"
                    }
                }
            }
        });
        let list = playlist_from_library_item(&nested).unwrap();
        assert_eq!(list.id, "sp:playlist:pl3");
        assert!(list.library);
    }

    #[test]
    fn editorial_playlist_names_can_be_a_transformed_label() {
        let json = serde_json::json!({
            "uri": "spotify:playlist:37i9dQZF1DX0XUs1WfzixN",
            "name": { "transformedLabel": "Beele Radio" }
        });
        let list = playlist_from_gql(&json, "").unwrap();
        assert_eq!(list.id, "sp:playlist:37i9dQZF1DX0XUs1WfzixN");
        assert_eq!(list.name, "Beele Radio");
    }

    #[test]
    fn color_lyrics_map_synced_lines() {
        let json = serde_json::json!({
            "lyrics": {
                "syncType": "LINE_SYNCED",
                "lines": [
                    { "startTimeMs": "1200", "words": "Hola" },
                    { "startTimeMs": "2400", "words": "♪" },
                    { "startTimeMs": "3600", "words": "Noche" }
                ]
            }
        });
        let lyrics = lyrics_from_color(&json).unwrap();
        assert!(lyrics.synced);
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.lines[0].text, "Hola");
        assert_eq!(lyrics.lines[0].start_ms, 1200);
    }

    #[test]
    fn pathfinder_search_maps_tracks_albums_and_playlists() {
        let search = serde_json::json!({
            "tracksV2": {"items": [{
                "item": {
                    "__typename": "TrackResponseWrapper",
                    "data": {
                        "__typename": "Track",
                        "id": "abc",
                        "uri": "spotify:track:abc",
                        "name": "Pa Mal",
                        "duration": {"totalMilliseconds": 180000},
                        "artists": {"items": [{"profile": {"name": "Aitana"}}]},
                        "albumOfTrack": {
                            "name": "Alpha",
                            "coverArt": {"sources": [{"url": "https://i.scdn.co/image/x"}]}
                        }
                    }
                }
            }]},
            "playlists": {"items": [{
                "__typename": "PlaylistResponseWrapper",
                "data": {
                    "__typename": "Playlist",
                    "uri": "spotify:playlist:pl1",
                    "name": "Gym",
                    "ownerV2": {"data": {"name": "Aitana"}}
                }
            }]},
            "albumsV2": {"items": [{
                "__typename": "AlbumResponseWrapper",
                "data": {
                    "__typename": "Album",
                    "uri": "spotify:album:al1",
                    "name": "Alpha",
                    "artists": {"items": [{"profile": {"name": "Aitana"}}]},
                    "date": {"year": 2023}
                }
            }]}
        });
        let hits = tracks_from_search_v2(&search);
        assert_eq!(hits[0].id, "sp:abc");
        assert_eq!(hits[0].artist, "Aitana");
        let lists = playlists_from_search_v2(&search);
        assert_eq!(lists[0].id, "sp:playlist:pl1");
        assert!(!lists[0].library);
        let albums = albums_from_search_v2(&search);
        assert_eq!(albums[0].id, "sp:album:al1");
        assert_eq!(albums[0].year, "2023");
    }

    #[test]
    fn pathfinder_playlist_item_v2_becomes_a_song() {
        let item = serde_json::json!({
            "uid": "row1",
            "itemV2": {
                "__typename": "TrackResponseWrapper",
                "data": {
                    "__typename": "Track",
                    "uri": "spotify:track:abc",
                    "name": "Pa Mal",
                    "trackDuration": {"totalMilliseconds": 180000},
                    "artists": {"items": [{"profile": {"name": "Aitana"}}]},
                    "albumOfTrack": {
                        "name": "Alpha",
                        "coverArt": {"sources": [{"url": "https://i.scdn.co/image/x", "height": 64}]}
                    }
                }
            }
        });
        let song = song_from_playlist_item(&item).unwrap();
        assert_eq!(song.id.0, "sp:abc");
        assert_eq!(song.artist, "Aitana");
        assert_eq!(song.duration_ms, 180000);
    }
}
