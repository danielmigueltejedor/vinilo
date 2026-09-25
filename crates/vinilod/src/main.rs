// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-License-Identifier: GPL-3.0-or-later

//! `vinilod` — the half of Vinilo that owns the sidecar.
//!
//! **A daemon because the sidecar is a singleton, not because daemons are
//! nice.** One Widevine CDM, one `persist:vinilo` partition, one Chromium
//! profile lock: two processes cannot each run one. So if a terminal client and
//! a GTK window are ever to coexist, exactly one process holds the sidecar and
//! the rest are clients of it.
//!
//! Single-threaded on purpose. `mpris_server::Player` is `!Send`, the sidecar's
//! stdout reader is a local stream, and there is nothing here worth a thread
//! pool — this process waits on I/O, it does not compute.

mod bus;
mod heal;
mod library;
mod local;
mod mixer;
mod serve;
mod spotify_play;
mod state;
mod url_play;
mod vocal;
mod watchdog;
mod ytdlp;

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("vinilod=info,vinilo_core=info")),
        )
        .init();

    if let Some(language) = vinilo_core::i18n::load() {
        vinilo_core::i18n::set_current(language);
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building the runtime")?;

    // A `LocalSet`, because the things this process holds cannot be moved
    // between threads and there is no reason to want them to be.
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, serve::run())
}
