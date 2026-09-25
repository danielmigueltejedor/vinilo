// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Optional Last.fm scrobbling for online listening stats.
//!
//! Credentials live in `~/.config/vinilo/lastfm.json`. The GTK client writes
//! them from Preferences; the daemon reads them when a track starts. Session
//! keys are never logged.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use md5::{Digest, Md5};

const API_ROOT: &str = "https://ws.audioscrobbler.com/2.0/";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_secret: String,
    #[serde(default)]
    pub session_key: String,
    #[serde(default)]
    pub username: String,
}

impl Config {
    pub fn ready(&self) -> bool {
        self.enabled
            && !self.api_key.is_empty()
            && !self.api_secret.is_empty()
            && !self.session_key.is_empty()
    }

    pub fn can_auth(&self) -> bool {
        !self.api_key.is_empty() && !self.api_secret.is_empty()
    }
}

fn path() -> Option<std::path::PathBuf> {
    Some(crate::paths::config_dir()?.join("lastfm.json"))
}

pub fn load() -> Config {
    let Some(path) = path() else {
        return Config::default();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Config::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(cfg: &Config) {
    let Some(path) = path() else {
        return;
    };
    let Some(dir) = path.parent() else {
        return;
    };
    let Ok(json) = serde_json::to_string_pretty(cfg) else {
        return;
    };
    let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(path, json));
}

fn md5_hex(input: &str) -> String {
    let dig = Md5::digest(input.as_bytes());
    dig.iter().map(|b| format!("{b:02x}")).collect()
}

/// Last.fm `api_sig`: sorted `namevalue` pairs + secret, then MD5.
pub fn sign(params: &BTreeMap<String, String>, secret: &str) -> String {
    let mut flat = String::new();
    for (k, v) in params {
        if k == "format" || k == "callback" {
            continue;
        }
        flat.push_str(k);
        flat.push_str(v);
    }
    flat.push_str(secret);
    md5_hex(&flat)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|err| format!("last.fm http: {err}"))
}

async fn call_json(
    method: &str,
    mut params: BTreeMap<String, String>,
    secret: &str,
    post: bool,
) -> Result<serde_json::Value, String> {
    params.insert("method".into(), method.into());
    params.insert("format".into(), "json".into());
    let sig = sign(&params, secret);
    params.insert("api_sig".into(), sig);

    let http = client()?;
    let response = if post {
        http.post(API_ROOT)
            .form(&params)
            .send()
            .await
            .map_err(|err| format!("last.fm: {err}"))?
    } else {
        http.get(API_ROOT)
            .query(&params)
            .send()
            .await
            .map_err(|err| format!("last.fm: {err}"))?
    };
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("last.fm body: {err}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&body).map_err(|err| format!("last.fm json: {err}"))?;
    if let Some(code) = value.get("error").and_then(|e| e.as_i64()) {
        let msg = value
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("request failed");
        return Err(format!("last.fm error {code}: {msg}"));
    }
    if !status.is_success() {
        return Err(format!("last.fm HTTP {status}"));
    }
    Ok(value)
}

/// Start desktop auth. Open [`auth_url`] in a browser, then [`complete_auth`].
pub async fn begin_auth(cfg: &Config) -> Result<(String, String), String> {
    if !cfg.can_auth() {
        return Err("enter a Last.fm API key and shared secret first".into());
    }
    let mut params = BTreeMap::new();
    params.insert("api_key".into(), cfg.api_key.clone());
    let value = call_json("auth.getToken", params, &cfg.api_secret, false).await?;
    let token = value
        .get("token")
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "last.fm returned no auth token".to_string())?
        .to_owned();
    let auth_url = format!(
        "https://www.last.fm/api/auth/?api_key={}&token={}",
        urlencoding_lightweight(&cfg.api_key),
        urlencoding_lightweight(&token)
    );
    Ok((token, auth_url))
}

/// Exchange an authorized token for a long-lived session key.
pub async fn complete_auth(cfg: &Config, token: &str) -> Result<Config, String> {
    if !cfg.can_auth() {
        return Err("enter a Last.fm API key and shared secret first".into());
    }
    let mut params = BTreeMap::new();
    params.insert("api_key".into(), cfg.api_key.clone());
    params.insert("token".into(), token.to_owned());
    let value = call_json("auth.getSession", params, &cfg.api_secret, false).await?;
    let session = value
        .get("session")
        .ok_or_else(|| "last.fm: no session (authorize in the browser first)".to_string())?;
    let key = session
        .get("key")
        .and_then(|k| k.as_str())
        .filter(|k| !k.is_empty())
        .ok_or_else(|| "last.fm: empty session key".to_string())?
        .to_owned();
    let name = session
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_owned();
    let mut next = cfg.clone();
    next.session_key = key;
    next.username = name;
    next.enabled = true;
    save(&next);
    Ok(next)
}

pub async fn update_now_playing(
    cfg: &Config,
    artist: &str,
    title: &str,
    album: &str,
) -> Result<(), String> {
    if !cfg.ready() || artist.is_empty() || title.is_empty() {
        return Ok(());
    }
    let mut params = BTreeMap::new();
    params.insert("api_key".into(), cfg.api_key.clone());
    params.insert("sk".into(), cfg.session_key.clone());
    params.insert("artist".into(), artist.to_owned());
    params.insert("track".into(), title.to_owned());
    if !album.is_empty() {
        params.insert("album".into(), album.to_owned());
    }
    let _ = call_json("track.updateNowPlaying", params, &cfg.api_secret, true).await?;
    Ok(())
}

pub async fn scrobble(
    cfg: &Config,
    artist: &str,
    title: &str,
    album: &str,
    started_unix: u64,
) -> Result<(), String> {
    if !cfg.ready() || artist.is_empty() || title.is_empty() {
        return Ok(());
    }
    let mut params = BTreeMap::new();
    params.insert("api_key".into(), cfg.api_key.clone());
    params.insert("sk".into(), cfg.session_key.clone());
    params.insert("artist[0]".into(), artist.to_owned());
    params.insert("track[0]".into(), title.to_owned());
    params.insert("timestamp[0]".into(), started_unix.to_string());
    if !album.is_empty() {
        params.insert("album[0]".into(), album.to_owned());
    }
    let _ = call_json("track.scrobble", params, &cfg.api_secret, true).await?;
    Ok(())
}

/// Last.fm eligibility: ≥30s, or half the duration when shorter than 60s.
pub fn is_scrobble_eligible(heard_ms: u64, duration_ms: u64) -> bool {
    let heard_s = heard_ms / 1000;
    if duration_ms > 0 && duration_ms < 60_000 {
        return heard_ms * 2 >= duration_ms;
    }
    heard_s >= 30
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn urlencoding_lightweight(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_matches_lastfm_auth_example_shape() {
        // Documented construction: sorted name+value + secret → md5.
        let mut params = BTreeMap::new();
        params.insert("api_key".into(), "xxxxxxxx".into());
        params.insert("method".into(), "auth.getSession".into());
        params.insert("token".into(), "xxxxxxx".into());
        let sig = sign(&params, "mysecret");
        assert_eq!(
            sig,
            md5_hex("api_keyxxxxxxxxmethodauth.getSessiontokenxxxxxxxmysecret")
        );
    }

    #[test]
    fn short_tracks_need_half_duration() {
        assert!(is_scrobble_eligible(20_000, 40_000));
        assert!(!is_scrobble_eligible(10_000, 40_000));
    }

    #[test]
    fn long_tracks_need_thirty_seconds() {
        assert!(is_scrobble_eligible(30_000, 240_000));
        assert!(!is_scrobble_eligible(29_000, 240_000));
    }
}
