// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Files this process was asked to play, before the window exists to hear it.
//!
//! GApplication delivers `open` either instead of `activate` (first launch
//! with a file) or onto an already-running instance (the second `vinilo
//! song.mp3`). The component is not alive for the first of those, so the
//! paths sit here until `init` drains them. After that, every later `open`
//! is a message.
//!
//! The live sender is a thread-local, not a `static Mutex`. `AppMsg` carries
//! GTK widgets (the row context menu), which gtk4 0.11 no longer marks `Send`.
//! `open` and `init` both run on the GTK thread, so a thread-local is the
//! right place.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Mutex;

use relm4::Sender;

use crate::app::AppMsg;

static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

thread_local! {
    static SENDER: RefCell<Option<Sender<AppMsg>>> = const { RefCell::new(None) };
}

/// Bind the live window so a later `open` can reach it.
pub fn bind(sender: Sender<AppMsg>) {
    let pending = take();
    SENDER.with(|slot| *slot.borrow_mut() = Some(sender.clone()));
    if !pending.is_empty() {
        sender.emit(AppMsg::PlayFiles(pending));
    }
}

/// Paths that arrived before the window existed.
pub fn take() -> Vec<PathBuf> {
    PENDING
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default()
}

/// A GApplication `open`. Stash, or deliver, never drop.
pub fn receive(paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    SENDER.with(|slot| {
        if let Some(sender) = slot.borrow().as_ref() {
            sender.emit(AppMsg::PlayFiles(paths));
        } else if let Ok(mut pending) = PENDING.lock() {
            pending.extend(paths);
        }
    });
}
