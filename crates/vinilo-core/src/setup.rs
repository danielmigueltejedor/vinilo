// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Whether a catalogue source has been set up, and where its cookies live.
//!
//! Apple Music keeps its session inside the sidecar. Spotify, YouTube Music
//! and Tidal have no equivalent Linux playback API, so Vinilo opens a login
//! window of its own and stores the resulting cookies here. The daemon then
//! hands that file to `yt-dlp`. Tokens still never live in settings.ini.

use std::path::{Path, PathBuf};

use crate::paths;
use crate::provider::Provider;

fn configured_path() -> Option<PathBuf> {
    Some(paths::config_dir()?.join("configured"))
}

/// Netscape cookie jar `yt-dlp --cookies` reads. One file per catalogue.
pub fn cookies_path(provider: Provider) -> Option<PathBuf> {
    if !provider.is_catalog() {
        return None;
    }
    Some(paths::config_dir()?.join(format!("cookies-{}.txt", provider.as_str())))
}

/// WebKit profile for that source's login window.
pub fn webkit_data_dir(provider: Provider) -> Option<PathBuf> {
    if !provider.is_catalog() {
        return None;
    }
    Some(paths::config_dir()?.join(format!("webkit-{}", provider.as_str())))
}

pub fn webkit_cache_dir(provider: Provider) -> Option<PathBuf> {
    if !provider.is_catalog() {
        return None;
    }
    Some(paths::cache_dir()?.join(format!("webkit-{}", provider.as_str())))
}

/// Sign-in page opened in the setup window.
pub fn login_url(provider: Provider) -> &'static str {
    match provider {
        Provider::Spotify => {
            "https://accounts.spotify.com/en/login?continue=https%3A%2F%2Fopen.spotify.com%2F"
        }
        Provider::YoutubeMusic => {
            "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F"
        }
        Provider::Tidal => "https://listen.tidal.com/login",
        Provider::AppleMusic | Provider::Local => "",
    }
}

/// URIs whose cookies we dump after login. Session cookies never hit
/// WebKit's on-disk jar, so we ask for these explicitly.
pub fn cookie_uris(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Spotify => &[
            "https://open.spotify.com/",
            "https://accounts.spotify.com/",
            "https://www.spotify.com/",
            "https://spotify.com/",
        ],
        Provider::YoutubeMusic => &[
            "https://music.youtube.com/",
            "https://www.youtube.com/",
            "https://accounts.google.com/",
            "https://www.google.com/",
        ],
        Provider::Tidal => &[
            "https://listen.tidal.com/",
            "https://tidal.com/",
            "https://login.tidal.com/",
            "https://auth.tidal.com/",
            "https://accounts.tidal.com/",
            "https://api.tidal.com/",
        ],
        Provider::AppleMusic | Provider::Local => &[],
    }
}

/// True once the login window has landed on the service's own site.
pub fn uri_looks_signed_in(provider: Provider, uri: &str) -> bool {
    let uri = uri.to_ascii_lowercase();
    match provider {
        Provider::Spotify => {
            uri.contains("open.spotify.com")
                && !uri.contains("accounts.spotify.com")
                && !uri.contains("/login")
        }
        Provider::YoutubeMusic => {
            uri.contains("music.youtube.com")
                && !uri.contains("accounts.google")
                && !uri.contains("servicelogin")
                && !uri.contains("/signin")
        }
        Provider::Tidal => {
            (uri.contains("listen.tidal.com") || uri.contains("tidal.com/browse"))
                && !uri.contains("/login")
                && !uri.contains("login.tidal")
                && !uri.contains("accounts.tidal")
        }
        Provider::AppleMusic | Provider::Local => false,
    }
}

/// Cookie names that mean a real session, not an anonymous visit.
pub fn looks_signed_in(provider: Provider, names: &[String]) -> bool {
    let has = |want: &[&str]| {
        names
            .iter()
            .any(|name| want.iter().any(|n| name.eq_ignore_ascii_case(n)))
    };
    match provider {
        // `sp_key` alone is an anonymous visit. The web-player token needs `sp_dc`.
        Provider::Spotify => has(&["sp_dc"]),
        Provider::YoutubeMusic => has(&[
            "SID",
            "SAPISID",
            "__Secure-1PSID",
            "__Secure-3PSID",
            "LOGIN_INFO",
        ]),
        Provider::Tidal => has(&[
            "sid",
            "token",
            "_token",
            "refresh_token",
            "tidal_sid",
            "access_token",
            "id_token",
            "authorization",
            "refresh",
            "sessionid",
            "userid",
            "user_id",
            "playback",
        ]) || names.iter().any(|name| looks_like_tidal_session_cookie(name)),
        Provider::AppleMusic | Provider::Local => false,
    }
}

fn looks_like_tidal_session_cookie(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    if name.contains("csrf") || name.contains("xsrf") || name.starts_with("_ga") {
        return false;
    }
    (name.contains("tidal")
        && (name.contains("sid") || name.contains("token") || name.contains("auth")))
        || name.ends_with("_token")
        || name.ends_with("-token")
}

pub fn is_configured(provider: Provider) -> bool {
    if !provider.is_catalog() {
        return true;
    }
    let Some(text) = configured_path().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return false;
    };
    text.lines()
        .any(|line| Provider::parse(line) == Some(provider))
}

pub fn mark_configured(provider: Provider) {
    if !provider.is_catalog() {
        return;
    }
    let Some(path) = configured_path() else {
        return;
    };
    if let Some(dir) = path.parent()
        && let Err(err) = std::fs::create_dir_all(dir)
    {
        tracing::warn!(?err, "could not create config directory");
        return;
    }
    let mut ids: Vec<String> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter_map(Provider::parse)
        .filter(|p| p.is_catalog())
        .map(|p| p.as_str().to_owned())
        .collect();
    let id = provider.as_str();
    if !ids.iter().any(|seen| seen == id) {
        ids.push(id.to_owned());
    }
    ids.sort();
    ids.dedup();
    if let Err(err) = std::fs::write(&path, format!("{}\n", ids.join("\n"))) {
        tracing::warn!(?err, "could not save catalogue setup");
    }
}

/// Forget the session: cookies, WebKit profile, and the configured flag.
pub fn clear(provider: Provider) {
    if !provider.is_catalog() {
        return;
    }
    if let Some(path) = cookies_path(provider) {
        let _ = std::fs::remove_file(path);
    }
    if let Some(dir) = webkit_data_dir(provider) {
        let _ = std::fs::remove_dir_all(dir);
    }
    if let Some(dir) = webkit_cache_dir(provider) {
        let _ = std::fs::remove_dir_all(dir);
    }
    let Some(path) = configured_path() else {
        return;
    };
    let ids: Vec<String> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter_map(Provider::parse)
        .filter(|p| *p != provider && p.is_catalog())
        .map(|p| p.as_str().to_owned())
        .collect();
    let _ = std::fs::write(&path, format!("{}\n", ids.join("\n")));
}

/// One cookie in the Netscape format `yt-dlp` and curl agree on.
#[derive(Debug, Clone)]
pub struct NetscapeCookie {
    pub domain: String,
    pub host_only: bool,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub expires: i64,
    pub name: String,
    pub value: String,
}

pub fn write_netscape(path: &Path, cookies: &[NetscapeCookie]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut out =
        String::from("# Netscape HTTP Cookie File\n# This file was generated by Vinilo.\n");
    for cookie in cookies {
        if cookie.name.is_empty() {
            continue;
        }
        let mut domain = cookie.domain.clone();
        if domain.is_empty() {
            continue;
        }
        if !cookie.host_only && !domain.starts_with('.') {
            domain.insert(0, '.');
        }
        let flag = if cookie.host_only { "FALSE" } else { "TRUE" };
        let path_s = if cookie.path.is_empty() {
            "/"
        } else {
            cookie.path.as_str()
        };
        let secure = if cookie.secure { "TRUE" } else { "FALSE" };
        if cookie.http_only {
            out.push_str("#HttpOnly_");
        }
        out.push_str(&format!(
            "{domain}\t{flag}\t{path_s}\t{secure}\t{}\t{}\t{}\n",
            cookie.expires, cookie.name, cookie.value
        ));
    }
    std::fs::write(path, out)
}

/// `Cookie` header for catalogue HTTP search, from the login dump.
pub fn cookie_header(provider: Provider) -> Option<String> {
    let path = cookies_path(provider)?;
    let text = std::fs::read_to_string(path).ok()?;
    let mut parts = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') && !line.starts_with("#HttpOnly_") {
            continue;
        }
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 7 {
            continue;
        }
        let name = cols[5].trim();
        let value = cols[6].trim();
        if !name.is_empty() {
            parts.push(format!("{name}={value}"));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

/// Cookies the web-player token endpoint actually needs. Sending the whole
/// jar (including leftover YouTube cookies from a bad import) makes Spotify
/// answer 403 and search used to fall back to YouTube.
pub fn session_cookie(provider: Provider) -> Option<String> {
    match provider {
        Provider::Spotify => named_cookies(provider, &["sp_dc", "sp_key"]),
        Provider::YoutubeMusic => ranked_cookie_header(
            provider,
            &["music.youtube.com", "youtube.com", "google.com"],
        ),
        _ => cookie_header(provider),
    }
}

/// One Cookie header, keeping the value from the most specific domain when
/// the Netscape jar repeats a name (Google dumps `SID` for both `.google.com`
/// and `.youtube.com`; InnerTube wants the YouTube one).
pub fn ranked_cookie_header(provider: Provider, prefer: &[&str]) -> Option<String> {
    let path = cookies_path(provider)?;
    let text = std::fs::read_to_string(path).ok()?;
    ranked_cookie_header_from(&text, prefer)
}

pub fn ranked_cookie_header_from(text: &str, prefer: &[&str]) -> Option<String> {
    struct Line<'a> {
        name: &'a str,
        value: &'a str,
        rank: usize,
    }
    let mut lines = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || (line.starts_with('#') && !line.starts_with("#HttpOnly_")) {
            continue;
        }
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 7 {
            continue;
        }
        let domain = cols[0].trim();
        let name = cols[5].trim();
        let value = cols[6].trim();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        let rank = prefer
            .iter()
            .position(|d| domain.contains(d))
            .unwrap_or(prefer.len());
        lines.push(Line { name, value, rank });
    }
    lines.sort_by_key(|l| (l.rank, l.name));
    let mut seen = std::collections::HashSet::new();
    let mut parts = Vec::new();
    for line in lines {
        let key = line.name.to_ascii_lowercase();
        if seen.insert(key) {
            parts.push(format!("{}={}", line.name, line.value));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

fn named_cookies(provider: Provider, names: &[&str]) -> Option<String> {
    let path = cookies_path(provider)?;
    let text = std::fs::read_to_string(path).ok()?;
    let mut found: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || (line.starts_with('#') && !line.starts_with("#HttpOnly_")) {
            continue;
        }
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 7 {
            continue;
        }
        let domain = cols[0].trim();
        if provider == Provider::Spotify && !domain.contains("spotify.com") {
            continue;
        }
        let name = cols[5].trim();
        let value = cols[6].trim();
        if names.iter().any(|want| name.eq_ignore_ascii_case(want)) && !value.is_empty() {
            if !found.iter().any(|(n, _)| n == name) {
                found.push((name.to_owned(), value.to_owned()));
            }
        }
    }
    if found.iter().any(|(n, _)| n == "sp_dc") || provider != Provider::Spotify {
        if found.is_empty() {
            None
        } else {
            Some(
                found
                    .into_iter()
                    .map(|(n, v)| format!("{n}={v}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        }
    } else {
        None
    }
}

pub fn cookie_names(provider: Provider) -> Vec<String> {
    let Some(path) = cookies_path(provider) else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || (line.starts_with('#') && !line.starts_with("#HttpOnly_")) {
            continue;
        }
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() >= 6 {
            let name = cols[5].trim();
            if !name.is_empty() {
                names.push(name.to_owned());
            }
        }
    }
    names
}

/// `--cookies PATH` when the login dump exists.
pub fn ytdlp_cookie_args(provider: Provider) -> Vec<String> {
    let Some(path) = cookies_path(provider) else {
        return Vec::new();
    };
    if path.is_file() {
        vec!["--cookies".into(), path.to_string_lossy().into_owned()]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotify_home_counts_as_signed_in() {
        assert!(uri_looks_signed_in(
            Provider::Spotify,
            "https://open.spotify.com/intl-es"
        ));
        assert!(!uri_looks_signed_in(
            Provider::Spotify,
            "https://accounts.spotify.com/en/login"
        ));
    }

    #[test]
    fn youtube_music_home_counts_as_signed_in() {
        assert!(uri_looks_signed_in(
            Provider::YoutubeMusic,
            "https://music.youtube.com/"
        ));
        assert!(!uri_looks_signed_in(
            Provider::YoutubeMusic,
            "https://accounts.google.com/ServiceLogin?service=youtube"
        ));
        assert!(
            !uri_looks_signed_in(Provider::YoutubeMusic, "https://www.youtube.com/"),
            "plain youtube.com is not the Music session"
        );
    }

    #[test]
    fn tidal_home_counts_as_signed_in() {
        assert!(uri_looks_signed_in(
            Provider::Tidal,
            "https://listen.tidal.com/"
        ));
        assert!(uri_looks_signed_in(
            Provider::Tidal,
            "https://tidal.com/browse"
        ));
        assert!(!uri_looks_signed_in(
            Provider::Tidal,
            "https://listen.tidal.com/login"
        ));
        assert!(!uri_looks_signed_in(
            Provider::Tidal,
            "https://login.tidal.com/authorize"
        ));
    }

    #[test]
    fn tidal_session_cookies_are_not_analytics() {
        assert!(looks_signed_in(
            Provider::Tidal,
            &["SID".into(), "_ga".into()]
        ));
        assert!(looks_signed_in(
            Provider::Tidal,
            &["tidal_access_token".into()]
        ));
        assert!(!looks_signed_in(
            Provider::Tidal,
            &["_ga".into(), "__cf_bm".into(), "XSRF-TOKEN".into()]
        ));
    }

    #[test]
    fn sp_dc_means_a_spotify_session() {
        assert!(looks_signed_in(
            Provider::Spotify,
            &["sp_dc".into(), "sp_t".into()]
        ));
        assert!(!looks_signed_in(Provider::Spotify, &["sp_landing".into()]));
        assert!(
            !looks_signed_in(Provider::Spotify, &["sp_key".into()]),
            "sp_key without sp_dc is not a logged-in session"
        );
    }

    #[test]
    fn netscape_round_trip_keeps_httponly() {
        let dir = std::env::temp_dir().join(format!(
            "vinilo-setup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cookies.txt");
        write_netscape(
            &path,
            &[NetscapeCookie {
                domain: "spotify.com".into(),
                host_only: false,
                path: "/".into(),
                secure: true,
                http_only: true,
                expires: 2000000000,
                name: "sp_dc".into(),
                value: "abc".into(),
            }],
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("#HttpOnly_.spotify.com"));
        assert!(text.contains("sp_dc"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn youtube_cookie_header_prefers_music_over_google() {
        let text = "\
.google.com	TRUE	/	TRUE	0	SID	google-sid
.youtube.com	TRUE	/	TRUE	0	SID	youtube-sid
.music.youtube.com	TRUE	/	TRUE	0	SID	music-sid
.google.com	TRUE	/	TRUE	0	SAPISID	google-sapi
";
        let header =
            ranked_cookie_header_from(text, &["music.youtube.com", "youtube.com", "google.com"])
                .unwrap();
        assert!(header.contains("SID=music-sid"));
        assert!(!header.contains("google-sid"));
        assert!(!header.contains("youtube-sid"));
        assert!(header.contains("SAPISID=google-sapi"));
    }
}
