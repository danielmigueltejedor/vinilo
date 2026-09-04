// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-License-Identifier: GPL-3.0-or-later

//! Everything a client of Apple Music needs that is not drawing.
//!
//! No toolkit reaches in here — that is the point of the split. A frontend that
//! draws nothing at all still needs all of it: the sidecar contract, the child's
//! lifetime, our mirror of its state, and the catalog client.

pub mod artwork;
pub mod catalog;
pub mod discover;
pub mod entry;
pub mod i18n;
pub mod ipc;
pub mod library_cache;
pub mod listen_history;
pub mod local_files;
pub mod mpris;
pub mod music;
pub mod page_cache;
pub mod paths;
pub mod player;
pub mod provider;
pub mod queue;
pub mod session;
pub mod setup;
pub mod sort;
pub mod streams;
pub mod unplayable;

/// The application id. It must match the `.desktop` file name, the GResource
/// prefix, `RelmApp::new()` and the MPRIS bus name suffix.
pub const APP_ID: &str = "dev.danielmiguelt.Vinilo";
