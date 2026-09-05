// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Library writes for every catalogue Vinilo can sign into.
//!
//! The GTK client sends the same [`WriteAction`] for a star or a new playlist.
//! This module picks the service from the id prefix (and the current provider)
//! so Apple Music, Spotify, YouTube Music and Tidal all go through one door.

use anyhow::{Result, bail};

use crate::i18n::{self, Key};
use crate::ipc::WriteAction;
use crate::provider::Provider;

pub async fn apply(
    http: &reqwest::Client,
    provider: Provider,
    action: WriteAction,
    id: &str,
    playlist_id: Option<&str>,
    name: Option<&str>,
) -> Result<Option<String>> {
    reject_foreign(provider, id)?;
    if let Some(list) = playlist_id {
        reject_foreign(provider, list)?;
    }
    match provider {
        Provider::Spotify => crate::spotify::write(http, action, id, playlist_id, name).await,
        Provider::YoutubeMusic => crate::ytmusic::apply(http, action, id, playlist_id, name).await,
        Provider::Tidal => crate::tidal::apply(http, action, id, playlist_id, name).await,
        Provider::AppleMusic | Provider::Local => {
            bail!("{}", i18n::t(Key::WriteWrongSource))
        }
    }
}

fn reject_foreign(provider: Provider, id: &str) -> Result<(), anyhow::Error> {
    if id.is_empty() {
        return Ok(());
    }
    let ok = match provider {
        Provider::Spotify => id.starts_with("sp:"),
        Provider::YoutubeMusic => id.starts_with("yt:"),
        Provider::Tidal => id.starts_with("td:"),
        Provider::AppleMusic => !crate::streams::StreamHit::is_stream_id(id),
        Provider::Local => false,
    };
    if ok {
        Ok(())
    } else {
        bail!("{}", i18n::t(Key::WriteWrongSource))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotify_rejects_an_apple_library_id() {
        let err = reject_foreign(Provider::Spotify, "p.rmUMW1EMxo").unwrap_err();
        assert!(
            err.to_string().contains("another music source")
                || err.to_string().contains("otra fuente")
        );
    }

    #[test]
    fn empty_id_is_allowed_when_creating_a_playlist() {
        assert!(reject_foreign(Provider::Spotify, "").is_ok());
    }
}
