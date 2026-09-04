// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Spotify web-player TOTP.
//!
//! `open.spotify.com/get_access_token` now answers 400 without a rotating
//! one-time password. The cipher bytes live in the web player's JS; community
//! scrapers republish them as a versioned dict. We download the current dict,
//! XOR it the same way the player does, and mint a 6-digit SHA-1 TOTP.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use serde_json::Value;
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

const SECRET_URLS: &[&str] = &[
    "https://raw.githubusercontent.com/xyloflake/spot-secrets-go/main/secrets/secretDict.json",
    "https://cdn.jsdelivr.net/gh/xyloflake/spot-secrets-go@main/secrets/secretDict.json",
];

/// Last known working dict, used when GitHub is unreachable.
const BUNDLED_SECRETS: &str = r#"{"59":[123,105,79,70,110,59,52,125,60,49,80,70,89,75,80,86,63,53,123,37,117,49,52,93,77,62,47,86,48,104,68,72],"60":[79,109,69,123,90,65,46,74,94,34,58,48,70,71,92,85,122,63,91,64,87,87],"61":[44,55,47,42,70,40,34,114,76,74,50,111,120,97,75,76,94,102,43,69,49,120,118,80,64,78]}"#;

const MEMORY_TTL: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone)]
pub struct Secret {
    pub version: u32,
    pub cipher: Vec<u8>,
}

static CACHED: Mutex<Option<(Instant, Secret)>> = Mutex::new(None);

pub fn transform_cipher(cipher: &[u8]) -> Vec<u8> {
    cipher
        .iter()
        .enumerate()
        .map(|(i, &byte)| byte ^ ((i % 33) as u8 + 9))
        .collect()
}

/// HMAC key: XOR the cipher, stringify each byte, UTF-8.
pub fn hmac_key(cipher: &[u8]) -> Vec<u8> {
    transform_cipher(cipher)
        .into_iter()
        .map(|n| n.to_string())
        .collect::<String>()
        .into_bytes()
}

pub fn totp_from_cipher(cipher: &[u8], unix_time: u64) -> String {
    totp_at(&hmac_key(cipher), unix_time, 6)
}

pub fn totp_at(key: &[u8], unix_time: u64, digits: u32) -> String {
    hotp(key, unix_time / 30, digits)
}

fn hotp(key: &[u8], counter: u64, digits: u32) -> String {
    let mut mac = HmacSha1::new_from_slice(key).expect("HMAC-SHA1 accepts any key length");
    mac.update(&counter.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = (hash[19] & 0x0f) as usize;
    let bin = (u32::from(hash[offset]) & 0x7f) << 24
        | u32::from(hash[offset + 1]) << 16
        | u32::from(hash[offset + 2]) << 8
        | u32::from(hash[offset + 3]);
    let otp = bin % 10u32.pow(digits);
    format!("{otp:0width$}", width = digits as usize)
}

pub async fn current_secret(http: &reqwest::Client) -> Secret {
    if let Ok(guard) = CACHED.lock()
        && let Some((at, secret)) = guard.as_ref()
        && at.elapsed() < MEMORY_TTL
    {
        return secret.clone();
    }
    // Prefer a live dict so a stale ~/.cache/vinilo/spotify-totp.json cannot
    // pin us to an expired TOTP version. GitHub is optional: waiting on it
    // is how Listen Now spun forever on a machine that cannot reach it.
    let downloaded = tokio::time::timeout(Duration::from_secs(3), download_secret(http)).await;
    if let Ok(Some(secret)) = downloaded {
        remember(secret.clone());
        tracing::info!(ver = secret.version, "spotify totp secrets downloaded");
        return secret;
    }
    if let Some(secret) = load_disk() {
        remember(secret.clone());
        tracing::warn!(ver = secret.version, "spotify totp secrets from disk cache");
        return secret;
    }
    let secret = bundled();
    remember(secret.clone());
    tracing::warn!(
        ver = secret.version,
        "spotify totp secrets from the bundled copy"
    );
    secret
}

pub async fn server_time(http: &reqwest::Client) -> u64 {
    if let Ok(res) = http
        .get("https://open.spotify.com/api/server-time")
        .header("Accept", "application/json")
        .send()
        .await
        && let Ok(value) = res.json::<Value>().await
        && let Some(t) = json_u64(&value, "serverTime")
    {
        return t;
    }
    if let Ok(res) = http.head("https://open.spotify.com/").send().await
        && let Some(date) = res.headers().get("date").and_then(|h| h.to_str().ok())
        && let Some(t) = parse_http_date(date)
    {
        return t;
    }
    now_unix()
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn utc_ymd(unix: u64) -> String {
    let (y, m, d) = civil_from_days((unix / 86400) as i32);
    format!("{y:04}-{m:02}-{d:02}")
}

async fn download_secret(http: &reqwest::Client) -> Option<Secret> {
    for url in SECRET_URLS {
        match http.get(*url).send().await {
            Ok(res) if res.status().is_success() => match res.text().await {
                Ok(text) => match parse_dict(&text) {
                    Some(secret) => {
                        if let Some(path) = dict_path() {
                            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
                            let _ = std::fs::write(path, text);
                        }
                        return Some(secret);
                    }
                    None => tracing::warn!(url, "spotify totp dict was not a versioned cipher"),
                },
                Err(err) => tracing::warn!(url, %err, "spotify totp dict body"),
            },
            Ok(res) => tracing::warn!(url, status = %res.status(), "spotify totp dict http"),
            Err(err) => tracing::warn!(url, %err, "spotify totp dict fetch"),
        }
    }
    None
}

fn load_disk() -> Option<Secret> {
    let text = std::fs::read_to_string(dict_path()?).ok()?;
    parse_dict(&text)
}

fn bundled() -> Secret {
    parse_dict(BUNDLED_SECRETS).expect("bundled Spotify TOTP dict")
}

fn remember(secret: Secret) {
    if let Ok(mut guard) = CACHED.lock() {
        *guard = Some((Instant::now(), secret));
    }
}

fn dict_path() -> Option<std::path::PathBuf> {
    Some(crate::paths::cache_dir()?.join("spotify-totp.json"))
}

fn parse_dict(text: &str) -> Option<Secret> {
    let value: Value = serde_json::from_str(text).ok()?;
    let obj = value.as_object()?;
    let mut best: Option<Secret> = None;
    for (key, cipher) in obj {
        let Ok(version) = key.parse::<u32>() else {
            continue;
        };
        let Some(arr) = cipher.as_array() else {
            continue;
        };
        let bytes: Vec<u8> = arr
            .iter()
            .filter_map(|n| n.as_u64().and_then(|x| u8::try_from(x).ok()))
            .collect();
        if bytes.is_empty() {
            continue;
        }
        if best.as_ref().is_none_or(|s| version > s.version) {
            best = Some(Secret {
                version,
                cipher: bytes,
            });
        }
    }
    best
}

fn json_u64(value: &Value, key: &str) -> Option<u64> {
    let n = value.get(key)?;
    n.as_u64()
        .or_else(|| n.as_i64().and_then(|i| u64::try_from(i).ok()))
        .or_else(|| n.as_str()?.parse().ok())
}

fn parse_http_date(s: &str) -> Option<u64> {
    let rest = s.trim().split_once(", ")?.1;
    let mut parts = rest.split_whitespace();
    let day: u32 = parts.next()?.parse().ok()?;
    let month = month_num(parts.next()?)?;
    let year: i32 = parts.next()?.parse().ok()?;
    let mut hms = parts.next()?.split(':');
    let hour: u64 = hms.next()?.parse().ok()?;
    let minute: u64 = hms.next()?.parse().ok()?;
    let second: u64 = hms.next()?.parse().ok()?;
    let days = days_from_civil(year, month, day)?;
    Some(days * 86400 + hour * 3600 + minute * 60 + second)
}

fn month_num(name: &str) -> Option<u32> {
    Some(match name {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    })
}

/// Howard Hinnant, days since 1970-01-01.
fn days_from_civil(year: i32, month: u32, day: u32) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u32;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let z = era * 146097 + doe as i32 - 719468;
    u64::try_from(z).ok()
}

fn civil_from_days(days: i32) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i32 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_sha1_vectors() {
        let key = b"12345678901234567890";
        assert_eq!(totp_at(key, 59, 8), "94287082");
        assert_eq!(totp_at(key, 59, 6), "287082");
        assert_eq!(totp_at(key, 1_111_111_109, 8), "07081804");
    }

    #[test]
    fn spotify_cipher_becomes_the_known_hmac_key() {
        let cipher: [u8; 26] = [
            44, 55, 47, 42, 70, 40, 34, 114, 76, 74, 50, 111, 120, 97, 75, 76, 94, 102, 43, 69, 49,
            120, 118, 80, 64, 78,
        ];
        let key = String::from_utf8(hmac_key(&cipher)).unwrap();
        assert_eq!(
            key,
            "376136387538459893883312310911992847112448894410210511297108"
        );
        assert_eq!(totp_from_cipher(&cipher, 1_750_000_000), "848996");
        assert_eq!(totp_from_cipher(&cipher, 0), "204513");
    }

    #[test]
    fn bundled_dict_picks_the_highest_version() {
        let secret = bundled();
        assert_eq!(secret.version, 61);
        assert_eq!(secret.cipher.len(), 26);
    }

    #[test]
    fn http_date_header_is_unix_seconds() {
        assert_eq!(
            parse_http_date("Fri, 04 Sep 2026 18:24:24 GMT"),
            Some(1_788_546_264)
        );
    }

    #[test]
    fn utc_date_round_trips_the_http_date() {
        assert_eq!(utc_ymd(1_788_546_264), "2026-09-04");
        assert_eq!(utc_ymd(0), "1970-01-01");
    }
}
