// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Spotify web-player partner API (Pathfinder GraphQL + client-token).
//!
//! The token from `open.spotify.com/api/token` is an **api-partner** token.
//! Replaying it against `api.spotify.com/v1/me/*` returns 401/403. The web
//! player loads the library through Pathfinder:
//! `POST https://api-partner.spotify.com/pathfinder/v2/query`
//! with `Authorization: Bearer` **and** `Client-Token`.

use std::io::Read;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::{clip_body, note_rate_limit, retry_after};
use crate::i18n::{self, Key};
use crate::provider::Provider;

pub const PATHFINDER: &str = "https://api-partner.spotify.com/pathfinder/v2/query";
pub const CLIENT_TOKEN_URL: &str = "https://clienttoken.spotify.com/v1/clienttoken";

/// Fallback web-player client id when `/api/token` omits `clientId`.
pub const WEB_PLAYER_CLIENT_ID: &str = "d8a5ed958d274c2e8ee717e6a4b0971d";
pub const CLIENT_VERSION: &str = "1.2.91.72.g5337566e";

/// Persisted-query hashes captured from the web player. First hash is current;
/// the rest are previous hashes Spotify still accepts for a while after a
/// rotation. Order matters: we stop at the first that is not
/// `PersistedQueryNotFound`.
pub const LIBRARY_V3: &[&str] = &[
    "390c78e5b951029bad359785e69b07b536a509c581cbcd0aded5e5067f187455",
    "973e511ca44261fda7eebac8b653155e7caee3675abb4fb110cc1b8c78b091c3",
];
pub const FETCH_LIBRARY_TRACKS: &[&str] =
    &["087278b20b743578a6262c2b0b4bcd20d879c503cc359a2285baf083ef944240"];
pub const SEARCH_DESKTOP: &[&str] = &[
    "db61238974d27839a136c9dc02bfdbe3fab7635f21cf85976ebff9a1ee281345",
    "4801118d4a100f756e833d33984436a3899cff359c532f8fd3aaf174b60b3b49",
    "3c9d3f60dac5dea3876b6db3f534192b1c1d90032c4233c1bbaba526db41eb31",
];
pub const FETCH_PLAYLIST: &[&str] = &[
    "86dde7b9d9356e2369414647cf6950cfed96e778e129cfdfc99aea6c1613b3b0",
    "e4b2953f160e58e38ac025d79b5a9b3aceee5c4c716598e9830bfceb69faff5f",
    "bb67e0af06e8d6f52b531f97468ee4acd44cd0f82b988e15c2ea47b1148efc77",
    "346811f856fb0b7e4f6c59f8ebea78dd081c6e2fb01b77c954b26259d5fc6763",
];
pub const GET_ALBUM: &[&str] =
    &["b9bfabef66ed756e5e13f68a942deb60bd4125ec1f1be8cc42769dc0259b4b10"];
pub const GET_TRACK: &[&str] =
    &["612585ae06ba435ad26369870deaae23b5c8800a256cd8a57e08eddc25a37294"];
pub const ARTIST_OVERVIEW: &[&str] = &[
    "7bdc7185c219898c7a2b659cfff2f8ce066dd2d9a97f8b7c4bde92ccfec28310",
    "ae0e2958a4ab645b35ca19ac04d0495ae12d9c5d7b7286217674801a9aab281a",
    "5b9e64f43843fa3a9b6a98543600299b0a2cbbbccfdcdcef2402eb9c1017ca4c",
];
pub const HOME: &[&str] = &[
    "76243c78b0e20ecdbe41b794dec8cbe73f75e585b0a7201b8d2e84578412847a",
    "5366cbf1f73f8c813dd0f1addc6934950f0dd529cec907107c85851e645c2d16",
];
pub const WHATS_NEW: &[&str] = &[
    "d889c8c936ab192af8ced595427f5ba2acdf63478fdc0a181c8d477f8322630e",
    "3b53dede3c6054e8b7c962dd280eb6761c5d1c82b06b039f4110d76a62b4966b",
];

#[derive(Debug, Clone)]
pub struct Session {
    pub access: String,
    pub client_token: Option<String>,
    pub client_id: String,
    pub client_version: String,
}

pub async fn mint_client_token(
    http: &reqwest::Client,
    cookie: &str,
    client_id: &str,
    client_version: &str,
) -> Result<String> {
    let body = json!({
        "client_data": {
            "client_version": client_version,
            "client_id": client_id,
            "js_sdk_data": {
                "device_brand": "unknown",
                "device_model": "unknown",
                "os": "linux",
                "os_version": "unknown",
                "device_id": device_id(),
                "device_type": "computer"
            }
        }
    });
    let res = http
        .post(CLIENT_TOKEN_URL)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("Origin", "https://open.spotify.com")
        .header("Referer", "https://open.spotify.com/")
        .header("Cookie", cookie)
        .json(&body)
        .send()
        .await
        .context("spotify client-token")?;
    let status = res.status();
    if status.as_u16() == 429 {
        note_rate_limit(retry_after(res.headers()));
        anyhow::bail!("{}", i18n::t(Key::SpotifyRateLimited));
    }
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!(
            %status,
            body = %clip_body(&text),
            "spotify client-token http error"
        );
        anyhow::bail!("Spotify client-token {status}");
    }
    let value: Value = serde_json::from_str(&text).context("spotify client-token json")?;
    value
        .pointer("/granted_token/token")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .context("Spotify client-token missing granted_token.token")
}

pub async fn query(
    http: &reqwest::Client,
    session: &Session,
    operation: &str,
    hashes: &[&str],
    variables: Value,
) -> Result<Value> {
    let mut last_not_found = false;
    for (i, hash) in hashes.iter().enumerate() {
        match query_once(http, session, operation, hash, &variables).await {
            Ok(value) => return Ok(value),
            Err(err) if is_persisted_query_missing(&err) => {
                last_not_found = true;
                tracing::warn!(
                    operation,
                    hash = &hash[..hash.len().min(12)],
                    remaining = hashes.len() - i - 1,
                    "spotify persisted query hash rejected"
                );
            }
            Err(err) => return Err(err),
        }
    }
    if last_not_found {
        anyhow::bail!("{}", i18n::t(Key::SpotifyCatalogueChanged));
    }
    anyhow::bail!("spotify pathfinder {operation}: no hash candidates")
}

async fn query_once(
    http: &reqwest::Client,
    session: &Session,
    operation: &str,
    hash: &str,
    variables: &Value,
) -> Result<Value> {
    let body = json!({
        "variables": variables,
        "operationName": operation,
        "extensions": {
            "persistedQuery": {
                "version": 1,
                "sha256Hash": hash
            }
        }
    });
    let mut req = http
        .post(PATHFINDER)
        .bearer_auth(&session.access)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("App-Platform", "WebPlayer")
        .header("Spotify-App-Version", &session.client_version)
        .header("Origin", "https://open.spotify.com")
        .header("Referer", "https://open.spotify.com/");
    if let Some(client_token) = &session.client_token {
        req = req.header("client-token", client_token);
    }
    if let Some(cookie) = crate::setup::session_cookie(Provider::Spotify) {
        req = req.header("Cookie", cookie);
    }
    let res = req.json(&body).send().await.context("spotify pathfinder")?;
    let status = res.status();
    if status.as_u16() == 429 {
        note_rate_limit(retry_after(res.headers()));
        anyhow::bail!("{}", i18n::t(Key::SpotifyRateLimited));
    }
    if status.as_u16() == 412 {
        anyhow::bail!("PersistedQueryNotFound");
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        anyhow::bail!(
            "Spotify refused the partner session ({status}). Sign in again from the menu."
        );
    }
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!(
            operation,
            %status,
            body = %clip_body(&text),
            "spotify pathfinder http error"
        );
        anyhow::bail!("Spotify pathfinder {status} for {operation}");
    }
    let value: Value = serde_json::from_str(&text).context("spotify pathfinder json")?;
    pathfinder_payload(value, operation)
}

/// GraphQL often answers `{ "data": null, "errors": [...] }`. Treating that as
/// an empty library would overwrite songs we already had.
pub(super) fn pathfinder_payload(value: Value, operation: &str) -> Result<Value> {
    if persisted_query_missing_in_body(&value) {
        anyhow::bail!("PersistedQueryNotFound");
    }
    match value.get("data") {
        None | Some(Value::Null) => {
            let msg = first_graphql_error(&value).unwrap_or("empty data");
            anyhow::bail!("spotify pathfinder {operation}: {msg}");
        }
        Some(_) => Ok(value),
    }
}

fn first_graphql_error(value: &Value) -> Option<&str> {
    value
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|errors| errors.first())
        .and_then(|err| err.get("message"))
        .and_then(Value::as_str)
}

fn persisted_query_missing_in_body(value: &Value) -> bool {
    value
        .get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| {
            errors.iter().any(|err| {
                err.get("message")
                    .and_then(Value::as_str)
                    .is_some_and(|m| m.contains("PersistedQueryNotFound"))
            })
        })
}

fn is_persisted_query_missing(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|e| e.to_string().contains("PersistedQueryNotFound"))
}

pub fn library_v3_vars(filter: &str, offset: usize, limit: usize, flatten: bool) -> Value {
    json!({
        "filters": [filter],
        "order": null,
        "textFilter": "",
        "features": [
            "LIKED_SONGS",
            "YOUR_EPISODES_V2",
            "PRERELEASES",
            "PRERELEASES_V2",
            "CLIPS",
            "EVENTS"
        ],
        "limit": limit,
        "offset": offset,
        "flatten": flatten,
        "expandedFolders": [],
        "folderUri": null,
        "includeFoldersWhenFlattening": !flatten
    })
}

fn device_id() -> String {
    if let Some(path) = crate::paths::cache_dir().map(|d| d.join("spotify-device-id")) {
        if let Ok(existing) = std::fs::read_to_string(&path) {
            let trimmed = existing.trim();
            if trimmed.len() >= 16 {
                return trimmed.to_owned();
            }
        }
        let id = random_hex(32);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, &id);
        return id;
    }
    random_hex(32)
}

fn random_hex(n: usize) -> String {
    let mut buf = vec![0u8; n.div_ceil(2)];
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        let _ = file.read_exact(&mut buf);
    } else {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(1);
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = ((t >> ((i % 16) * 8)) ^ (i as u128 * 17)) as u8;
        }
    }
    buf.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .chars()
        .take(n)
        .collect()
}

pub fn page_limit() -> usize {
    50
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn null_data_is_not_an_empty_library() {
        let value = json!({"data": null, "errors": [{"message": "Unauthorized"}]});
        let err = pathfinder_payload(value, "libraryV3").unwrap_err();
        assert!(err.to_string().contains("Unauthorized"));
    }

    #[test]
    fn persisted_query_errors_are_distinct() {
        let value = json!({"errors": [{"message": "PersistedQueryNotFound"}]});
        let err = pathfinder_payload(value, "libraryV3").unwrap_err();
        assert!(err.to_string().contains("PersistedQueryNotFound"));
    }

    #[test]
    fn a_payload_with_data_is_kept() {
        let value = json!({"data": {"me": {"libraryV3": {"items": []}}}});
        assert!(pathfinder_payload(value, "libraryV3").is_ok());
    }
}
