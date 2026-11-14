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
#[cfg(target_os = "linux")]
mod mixer;
#[cfg(not(target_os = "linux"))]
#[path = "mixer_none.rs"]
mod mixer;
mod serve;
mod spotify_play;
mod state;
mod tls;
mod url_play;
mod vocal;
mod watchdog;
mod ytdlp;

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    init_logging();
    tls::install_provider();

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

fn filter() -> EnvFilter {
    EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("vinilod=info,vinilo_core=info"))
}

/// On macOS the app starts the daemon with its output thrown away, so the log goes to
/// `~/Library/Logs/Vinilo/`, where Console finds it. Elsewhere it stays on stderr.
fn init_logging() {
    #[cfg(target_os = "macos")]
    if let Some(file) = macos_log_file() {
        tracing_subscriber::fmt()
            .with_env_filter(filter())
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .init();
        log_panics();
        return;
    }
    tracing_subscriber::fmt().with_env_filter(filter()).init();
}

/// One file per edition, started fresh each launch so it never grows without bound.
#[cfg(target_os = "macos")]
fn macos_log_file() -> Option<std::fs::File> {
    let dir = std::path::PathBuf::from(std::env::var_os("HOME")?).join("Library/Logs/Vinilo");
    std::fs::create_dir_all(&dir).ok()?;
    let name = format!("{}.log", vinilo_core::edition::dir_name());
    std::fs::File::create(dir.join(name)).ok()
}

/// A panic's message goes to stderr, which nobody reads here; put it in the log too.
#[cfg(target_os = "macos")]
fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(%info, "panic");
        previous(info);
    }));
}
