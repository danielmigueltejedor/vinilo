// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lyrics for the expanded player. Each catalogue has its own endpoint; this
//! file is the door the daemon uses so the GTK client never talks HTTP.

use anyhow::{Result, bail};

use crate::i18n::{self, Key};
use crate::ipc::Lyrics;

pub async fn for_id(http: &reqwest::Client, id: &str) -> Result<Lyrics> {
    if id.starts_with("sp:") {
        crate::spotify::lyrics(http, id).await
    } else if id.starts_with("yt:") {
        crate::ytmusic::lyrics(http, id).await
    } else {
        bail!("{}", i18n::t(Key::LyricsMissing))
    }
}
