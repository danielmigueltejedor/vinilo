// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Spotify Web API, using the web-player token from the login cookies.
//!
//! Playback is still `yt-dlp` — Spotify does not offer a legal Linux stream.
//! This module is the catalogue: search, liked songs, playlists, albums, and
//! the Listen Now shelves. Tokens never leave this process.
//!
//! The web player no longer hands out a token from `get_access_token` alone.
//! We mint the same TOTP the site uses, then call `/api/token`.

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

mod totp;

const API: &str = "https://api.spotify.com/v1";
const PLAYER_TOKEN: &str = "https://open.spotify.com/api/token";

struct CachedToken {
    access: String,
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
    let token = token(http).await?;
    let url = format!(
        "{API}/search?q={}&type=track,album,artist,playlist&limit=20",
        streams::urlencoding(query)
    );
    let value = api_get(http, &token, &url).await?;
    let hits = tracks_from_search(&value);
    let songs: Vec<Track> = hits.iter().filter_map(hit_to_song).collect();
    let albums = albums_from_search(&value);
    let artists = artists_from_search(&value);
    let playlists = playlists_from_search(&value);
    Ok(SearchPage {
        results: SearchResults {
            songs,
            albums,
            artists,
            playlists,
        },
        hits,
    })
}

pub fn catalog_entries(page: SearchPage, filter: CatalogFilter) -> (Vec<StreamHit>, Vec<Entry>) {
    let SearchPage { hits, results } = page;
    let (entries, _) = crate::catalog::catalog_rows(filter, results, true);
    (hits, entries)
}

pub async fn library(http: &reqwest::Client) -> Result<Library> {
    let token = token(http).await?;
    let (liked, albums, artists, playlists, top) = tokio::join!(
        saved_tracks(http, &token, 200),
        saved_albums(http, &token, 100),
        followed_artists(http, &token, 50),
        user_playlists(http, &token, 100),
        top_tracks(http, &token, 50),
    );
    if liked.is_err() && albums.is_err() && artists.is_err() && playlists.is_err() {
        return Err(liked.err().unwrap_or_else(|| {
            anyhow::anyhow!("Spotify would not return a library for this session")
        }));
    }
    let songs = liked.unwrap_or_else(|err| {
        tracing::warn!(?err, "spotify liked songs");
        Vec::new()
    });
    let albums = albums.unwrap_or_else(|err| {
        tracing::warn!(?err, "spotify saved albums");
        Vec::new()
    });
    let artists = artists.unwrap_or_else(|err| {
        tracing::warn!(?err, "spotify followed artists");
        Vec::new()
    });
    let mut playlists = playlists.unwrap_or_else(|err| {
        tracing::warn!(?err, "spotify playlists");
        Vec::new()
    });
    if !songs.is_empty() {
        playlists.insert(0, liked_playlist(&songs));
    }
    if let Ok(top) = top
        && !top.is_empty()
    {
        playlists.insert(if songs.is_empty() { 0 } else { 1 }, top_playlist(&top));
    }
    Ok(Library {
        songs,
        albums,
        artists,
        playlists,
    })
}

pub async fn discover(http: &reqwest::Client, library: &Library) -> Result<Discover> {
    let token = token(http).await?;
    let (recent, new_releases, featured) = tokio::join!(
        recently_played(http, &token, 16),
        new_releases(http, &token, 16),
        featured_playlists(http, &token, 16),
    );
    let recently_played = recent
        .unwrap_or_default()
        .into_iter()
        .map(Entry::Song)
        .collect();
    let charts = new_releases
        .unwrap_or_default()
        .into_iter()
        .map(Entry::Album)
        .collect();
    let mut recommended_playlists: Vec<Entry> = featured
        .unwrap_or_default()
        .into_iter()
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
    let recommended_songs = if library.songs.is_empty() {
        Vec::new()
    } else {
        library.songs.iter().take(16).cloned().collect()
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
    let token = token(http).await?;
    match parsed {
        Ref::Album(album_id) => {
            let (album, tracks) = album(http, &token, &album_id).await?;
            Ok((
                Entry::Album(album),
                tracks.into_iter().map(Entry::Song).collect(),
            ))
        }
        Ref::Playlist(playlist_id) => {
            let (list, tracks) = playlist(http, &token, &playlist_id).await?;
            Ok((
                Entry::Playlist(list),
                tracks.into_iter().map(Entry::Song).collect(),
            ))
        }
        Ref::Liked => {
            let tracks = saved_tracks(http, &token, 200).await?;
            Ok((
                Entry::Playlist(liked_playlist(&tracks)),
                tracks.into_iter().map(Entry::Song).collect(),
            ))
        }
        Ref::Top => {
            let tracks = top_tracks(http, &token, 50).await?;
            Ok((
                Entry::Playlist(top_playlist(&tracks)),
                tracks.into_iter().map(Entry::Song).collect(),
            ))
        }
        Ref::Artist(artist_id) => {
            let (artist, albums) = artist_albums(http, &token, &artist_id).await?;
            Ok((
                Entry::Artist(artist),
                albums.into_iter().map(Entry::Album).collect(),
            ))
        }
        Ref::Track(_) => anyhow::bail!("a track does not open a page"),
    }
}

pub async fn track_hit(http: &reqwest::Client, id: &str) -> Result<StreamHit> {
    let token = token(http).await?;
    let value = api_get(http, &token, &format!("{API}/tracks/{id}")).await?;
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

async fn token(http: &reqwest::Client) -> Result<String> {
    if let Some(cached) = cached_token() {
        return Ok(cached);
    }
    if let Some(wait) = cooldown_left() {
        tracing::warn!(secs = wait.as_secs(), "spotify still rate-limited");
        anyhow::bail!("{}", i18n::t(Key::SpotifyRateLimited));
    }
    let _fetch = FETCH.lock().await;
    if let Some(cached) = cached_token() {
        return Ok(cached);
    }
    if cooldown_left().is_some() {
        anyhow::bail!("{}", i18n::t(Key::SpotifyRateLimited));
    }
    let Some(cookie) = crate::setup::session_cookie(Provider::Spotify) else {
        anyhow::bail!("{}", i18n::t(Key::SpotifyNotSignedIn));
    };
    match fetch_totp_token(http, &cookie).await {
        Ok((access, ttl)) => {
            store_token(&access, ttl);
            Ok(access)
        }
        Err(_) if cooldown_left().is_some() => {
            anyhow::bail!("{}", i18n::t(Key::SpotifyRateLimited));
        }
        Err(err) => Err(err).context(i18n::t(Key::SpotifyTokenRefused)),
    }
}

fn cached_token() -> Option<String> {
    let guard = TOKEN.lock().ok()?;
    let cached = guard.as_ref()?;
    (cached.expires > Instant::now()).then(|| cached.access.clone())
}

fn cooldown_left() -> Option<Duration> {
    let until = *COOLDOWN.lock().ok()?;
    let until = until?;
    let now = Instant::now();
    (until > now).then(|| until.saturating_duration_since(now))
}

fn note_rate_limit(wait: Duration) {
    tracing::warn!(secs = wait.as_secs(), "spotify 429 — backing off");
    if let Ok(mut guard) = COOLDOWN.lock() {
        let until = Instant::now() + wait;
        if guard.is_none_or(|was| until > was) {
            *guard = Some(until);
        }
    }
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Duration {
    const DEFAULT: Duration = Duration::from_secs(60);
    let Some(raw) = headers.get("retry-after").and_then(|h| h.to_str().ok()) else {
        return DEFAULT;
    };
    match raw.trim().parse::<u64>() {
        Ok(secs) => Duration::from_secs(secs.clamp(15, 15 * 60)),
        Err(_) => DEFAULT,
    }
}

fn store_token(access: &str, ttl: Duration) {
    if let Ok(mut guard) = TOKEN.lock() {
        *guard = Some(CachedToken {
            access: access.to_owned(),
            expires: Instant::now() + ttl,
        });
    }
}

async fn fetch_totp_token(http: &reqwest::Client, cookie: &str) -> Result<(String, Duration)> {
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
            Ok((access, false, ttl)) => return Ok((access, ttl)),
            Ok((_, true, _)) => {
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
) -> Result<(String, bool, Duration)> {
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

async fn send_token(req: reqwest::RequestBuilder) -> Result<(String, bool, Duration)> {
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
    Ok((access, anonymous, ttl))
}

fn clip_body(body: &str) -> String {
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
        if let Ok(mut guard) = TOKEN.lock() {
            *guard = None;
        }
        anyhow::bail!("Spotify refused the session ({status}). Sign in again from the menu.");
    }
    if !status.is_success() {
        anyhow::bail!("Spotify {status} for {url}");
    }
    res.json().await.context("spotify json")
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

async fn top_tracks(http: &reqwest::Client, token: &str, max: usize) -> Result<Vec<Track>> {
    let value = match api_get(
        http,
        token,
        &format!("{API}/me/top/tracks?time_range=short_term&limit={max}"),
    )
    .await
    {
        Ok(value) => value,
        Err(_) => {
            api_get(
                http,
                token,
                &format!("{API}/me/top/tracks?time_range=medium_term&limit={max}"),
            )
            .await?
        }
    };
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    Ok(items
        .iter()
        .filter_map(|track| song_from_track(track, String::new(), false, true))
        .collect())
}

async fn recently_played(http: &reqwest::Client, token: &str, max: usize) -> Result<Vec<Track>> {
    let value = api_get(
        http,
        token,
        &format!("{API}/me/player/recently-played?limit={max}"),
    )
    .await?;
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut songs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in items {
        let Some(track) = item.get("track") else {
            continue;
        };
        let Some(song) = song_from_track(track, text(item, "played_at"), false, true) else {
            continue;
        };
        if seen.insert(song.id.0.clone()) {
            songs.push(song);
        }
    }
    Ok(songs)
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

async fn new_releases(http: &reqwest::Client, token: &str, max: usize) -> Result<Vec<Album>> {
    let value = api_get(
        http,
        token,
        &format!("{API}/browse/new-releases?limit={max}"),
    )
    .await?;
    let Some(items) = value.pointer("/albums/items").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    Ok(items
        .iter()
        .filter_map(|item| album_from_object(item, String::new(), false))
        .collect())
}

async fn featured_playlists(
    http: &reqwest::Client,
    token: &str,
    max: usize,
) -> Result<Vec<Playlist>> {
    let featured = api_get(
        http,
        token,
        &format!("{API}/browse/featured-playlists?limit={max}"),
    )
    .await;
    if let Ok(value) = featured
        && let Some(items) = value.pointer("/playlists/items").and_then(Value::as_array)
    {
        let lists: Vec<Playlist> = items
            .iter()
            .filter_map(|item| playlist_from_object(item, false))
            .collect();
        if !lists.is_empty() {
            return Ok(lists);
        }
    }
    let categories = api_get(http, token, &format!("{API}/browse/categories?limit=6")).await;
    let Ok(value) = categories else {
        return Ok(Vec::new());
    };
    let Some(cats) = value.pointer("/categories/items").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut lists = Vec::new();
    for cat in cats.iter().take(4) {
        let Some(id) = cat.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Ok(page) = api_get(
            http,
            token,
            &format!("{API}/browse/categories/{id}/playlists?limit=6"),
        )
        .await
        else {
            continue;
        };
        if let Some(items) = page.pointer("/playlists/items").and_then(Value::as_array) {
            lists.extend(
                items
                    .iter()
                    .filter_map(|item| playlist_from_object(item, false)),
            );
        }
        if lists.len() >= max {
            break;
        }
    }
    lists.truncate(max);
    Ok(lists)
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
            if let Some(track) = item.get("track")
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
                if let Some(track) = item.get("track")
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
        library_id: None,
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
    if merged.get("album").is_none() {
        if let Some(obj) = merged.as_object_mut() {
            obj.insert("album".into(), album.clone());
        }
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
}
