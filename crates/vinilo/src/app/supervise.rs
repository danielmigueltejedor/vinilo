// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-License-Identifier: GPL-3.0-or-later

//! Staying connected to the daemon, and folding in what it says.
//!
//! This file used to supervise a child process. It does not any more: the
//! daemon owns the sidecar, because the Chromium profile lock says exactly one
//! process may. What is left is a socket that can go away — which is a smaller
//! problem, since reconnecting costs a connect rather than a Widevine boot.

use relm4::ComponentSender;

use super::{AppModel, CommandMsg, Stage};
use crate::daemon;
use vinilo_core::ipc::{Event, Request, Stage as DaemonStage, Transport};

/// How long to wait before dialling again, per consecutive failure.
///
/// Shorter than the sidecar's backoff was, because there is no Chromium to boot
/// — a daemon that is coming up is listening within a fraction of a second, and
/// one that will not start is not helped by waiting longer.
fn redial_delay(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis(200 * (1 << attempt.min(5)) as u64)
}

fn clears_account_state(stage: &DaemonStage) -> bool {
    matches!(stage, DaemonStage::SignedOut)
}

pub(super) fn reconnect(
    sender: &ComponentSender<AppModel>,
    delay: std::time::Duration,
    session: u64,
) {
    sender.command(move |out, shutdown| {
        shutdown
            .register(async move {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                tokio::spawn(daemon::connect(tx, session));
                while let Some(message) = rx.recv().await {
                    if out.send(CommandMsg::Daemon(message)).is_err() {
                        break; // the component is gone
                    }
                }
            })
            .drop_on_shutdown()
    });
}

impl AppModel {
    /// Start one connection attempt. Each call bumps `daemon_session` so a
    /// `Lost` from the previous socket cannot wipe the new handle or redial.
    pub(super) fn dial(&mut self, sender: &ComponentSender<Self>, delay: std::time::Duration) {
        self.daemon_session = self.daemon_session.wrapping_add(1);
        reconnect(sender, delay, self.daemon_session);
    }

    /// Ask the daemon for something.
    ///
    /// Fire-and-forget, and deliberately: the answer comes back as an event
    /// like any other, so a button can never draw a state by asking for it.
    pub(super) fn ask(&self, request: Request) {
        match &self.daemon {
            Some(handle) => handle.send(request),
            // The window is up before the connection is, which is a fraction of
            // a second and nothing has been drawn to click yet.
            None => tracing::debug!(?request, "dropped: not connected yet"),
        }
    }

    /// The transport verbs, which are most of what this app asks for.
    pub(super) fn transport(&self, verb: Transport) {
        self.ask(Request::Transport(verb));
    }

    pub(super) fn on_daemon(&mut self, message: daemon::Incoming, sender: &ComponentSender<Self>) {
        match message {
            daemon::Incoming::Connected { handle, session } => {
                if session != self.daemon_session {
                    tracing::debug!(
                        session,
                        current = self.daemon_session,
                        "ignored a stale daemon connection"
                    );
                    return;
                }
                tracing::info!("connected to vinilod");
                self.switching_source = false;
                self.redials = 0;
                self.daemon = Some(handle);
                // Everything at once, so the window is right before the first
                // change rather than after it. **The stage especially**: it is
                // only broadcast when it changes, so a client attaching to a
                // daemon that has been ready for an hour has to ask — otherwise
                // it draws its startup screen with music playing behind it.
                self.ask(Request::Stage);
                self.ask(Request::Snapshot);
                self.ask(Request::Queue);
                // The daemon may have finished a Spotify fetch while we were
                // dialling. Read the cache now, then ask for a refresh so a
                // first empty/failed load is retried without a manual Reload.
                self.reload_from_cache(sender);
                if self.settings.provider.is_catalog()
                    && self.all_tracks.is_empty()
                    && self.playlists.is_empty()
                {
                    self.set_library_refreshing(true);
                    self.loading_discover = true;
                }
                self.ask(Request::Refresh);
                self.refresh_discover();
                if !self.pending_files.is_empty() {
                    let paths = std::mem::take(&mut self.pending_files);
                    self.play_files(paths);
                }
            }
            daemon::Incoming::Event { event, session } => {
                if session != self.daemon_session {
                    tracing::debug!(
                        session,
                        current = self.daemon_session,
                        "ignored a stale daemon event"
                    );
                    return;
                }
                self.on_event(*event, sender);
            }
            daemon::Incoming::Unparsed { line, session } => {
                if session != self.daemon_session {
                    return;
                }
                // Loudly, per rule 4: this is `ipc.rs` and this build
                // disagreeing, which a restart will not fix and silence hides.
                tracing::warn!(%line, "daemon sent something this build cannot read");
            }
            daemon::Incoming::Lost { why, session } => {
                if session != self.daemon_session {
                    tracing::debug!(
                        session,
                        current = self.daemon_session,
                        "ignored a stale daemon loss"
                    );
                    return;
                }
                tracing::warn!(%why, "lost the daemon");
                self.daemon = None;
                if self.switching_source {
                    tracing::info!("lost vinilod while switching music source — dialling again");
                }
                if self.settings.provider.needs_apple() {
                    self.stage = Stage::Connecting;
                }
                let attempt = self.redials;
                self.redials += 1;
                self.dial(sender, redial_delay(attempt));
            }
        }
    }

    fn on_event(&mut self, event: Event, sender: &ComponentSender<Self>) {
        match event {
            Event::Snapshot(snap) => {
                self.mirror.snap = snap;
                self.sync_tick(sender);
                self.push_snapshot();
                // **Here, not on the queue event.** The cover belongs to the
                // snapshot that carries its path, and the two events arrive
                // separately — syncing on the queue read whichever snapshot
                // happened to be current, which is the one *before* the track
                // changed. `art_for` makes the repeat calls free.
                let in_flight = self.sync_artwork(sender);
                // **Here, with the title.** `maybe_notify` decides from the id
                // and draws from the title; taking them from different events
                // is what announced the previous song. `notified_for` is what
                // keeps a twice-a-second snapshot from notifying twice.
                self.maybe_notify(in_flight);
            }
            Event::Queue { items, position } => {
                self.mirror.queue = items;
                self.mirror.queue_position = position;
                self.push_snapshot();
                self.mark_now_playing();
            }
            Event::Stage(stage) => {
                let clear = clears_account_state(&stage) && self.settings.provider.needs_apple();
                self.mirror.stage = Some(stage.clone());
                if !self.settings.provider.needs_apple()
                    && matches!(stage, DaemonStage::Connecting | DaemonStage::SignedOut)
                {
                    self.stage = Stage::Ready;
                    return;
                }
                self.stage = match stage {
                    DaemonStage::Connecting => Stage::Connecting,
                    DaemonStage::Ready => Stage::Ready,
                    DaemonStage::SignedOut => Stage::SignedOut,
                    // The loud failure rule 4 demands.
                    DaemonStage::Broken { detail } => Stage::Broken(detail),
                };
                if clear {
                    self.forget_session(sender);
                }
            }
            Event::LibraryChanged => {
                tracing::info!("the daemon refreshed the library");
                self.set_library_refreshing(false);
                self.reload_from_cache(sender);
                self.refresh_discover();
            }
            Event::Discover(page) => {
                self.loading_discover = false;
                if page.is_empty() && !self.discover.is_empty() {
                    return;
                }
                let mut page = page;
                page.fill_gaps(self.discover.snapshot());
                self.discover.fill(page);
            }
            Event::LibraryRefreshing { refreshing } => {
                self.set_library_refreshing(refreshing);
            }
            Event::Page {
                id,
                header,
                entries,
                ..
            } => self.fill_page(&id, header, entries, sender),
            Event::Results {
                query,
                entries,
                offset,
                more,
            } => self.fill_catalog(&query, entries, offset, more),
            Event::Rows { .. } => {
                // Not asked for: this client reads the cache the daemon writes
                // and does its own filtering, sorting and grids, which is
                // presentation rather than something to ask across a socket.
            }
            Event::Error { detail } => {
                self.loading_discover = false;
                self.searching_catalog = false;
                self.set_library_refreshing(false);
                self.toast(&detail);
            }
        }
    }

    pub(super) fn set_library_refreshing(&mut self, refreshing: bool) {
        tracing::debug!(refreshing, "library refresh spinner");
        self.loading_library = refreshing;
        self.loading_albums = refreshing;
        self.loading_artists = refreshing;
        self.loading_playlists = refreshing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_confirmed_signed_out_stage_clears_account_state() {
        assert!(clears_account_state(&DaemonStage::SignedOut));
        assert!(!clears_account_state(&DaemonStage::Connecting));
        assert!(!clears_account_state(&DaemonStage::Ready));
        assert!(!clears_account_state(&DaemonStage::Broken {
            detail: "sign-out failed".into(),
        }));
    }
}
