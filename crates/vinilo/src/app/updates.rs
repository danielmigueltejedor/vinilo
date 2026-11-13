// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! In-app updates: a look at GitHub Releases, a question, and a verified install.
//!
//! The work is `nodalia-update`'s, shared with the other Nodalia apps. This file is only what is
//! Vinilo's own: where its releases live, how it words things, and the two windows. Nothing is
//! installed without a click, a package someone else manages (Flatpak, `.deb`, the AUR, a dev
//! build) is only pointed at, and a failed check on startup stays in the log rather than becoming
//! a toast nobody asked for.

use std::time::{SystemTime, UNIX_EPOCH};

use nodalia_gtk as ndl;
use nodalia_update as nu;
use relm4::ComponentSender;
use relm4::gtk;
use vinilo_core::i18n::{self, Language};

use super::{AppModel, AppMsg, CommandMsg};

pub(super) const APP: nu::App = nu::App {
    name: "Vinilo",
    repo: "getnodalia/vinilo",
    bundle: "Vinilo Nodalia.app",
    version: env!("CARGO_PKG_VERSION"),
    // On macOS the release carries two disk images, one per edition; this one takes its own.
    edition: Some("nodalia"),
};

/// An automatic check is skipped if one ran less than this long ago.
const EVERY: u64 = 24 * 60 * 60;

/// How long after start-up the automatic check waits, so it never competes with the first
/// paint or the library load.
pub(super) const STARTUP_DELAY: std::time::Duration = std::time::Duration::from_secs(20);

/// What the update windows hold between messages.
#[derive(Default)]
pub(super) struct UpdateUi {
    /// A check or an install is running: a second click does nothing.
    busy: bool,
    /// The release the open question is about.
    release: Option<nu::Release>,
    /// The download window, while it is up.
    progress: Option<(ndl::AlertDialog, gtk::ProgressBar)>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Whether an automatic check is due.
fn due(last: u64, now: u64) -> bool {
    now.saturating_sub(last) >= EVERY
}

impl AppModel {
    /// After start-up: say why the last macOS update failed, and check if it is time.
    pub(super) fn startup_update_checks(&mut self, sender: &ComponentSender<Self>) {
        if let Some(failure) = nu::take_last_failure(&APP) {
            let reason = match failure {
                nu::Failure::StillRunning => i18n::update::reason_still_running(),
                nu::Failure::Mount => i18n::update::reason_mount(),
                nu::Failure::Contents => i18n::update::reason_contents(),
                nu::Failure::Copy => i18n::update::reason_copy(),
                nu::Failure::Replace => i18n::update::reason_replace(),
                // `Install` and `Cancelled` only come from the Windows installer.
                _ => i18n::update::reason_other(),
            };
            self.toast(&i18n::update::last_failed(&reason));
        }
        // Only an install that can replace itself is worth interrupting: for the rest the
        // package manager already says when there is something new.
        if self.settings.auto_update_check
            && nu::install_kind(&APP).can_apply_in_place()
            && due(self.settings.last_update_check, now())
        {
            self.check_for_updates(sender, false);
        }
    }

    /// Look for a release. `user` is true for the menu item, which answers every outcome; the
    /// automatic check says something only when there is an update.
    pub(super) fn check_for_updates(&mut self, sender: &ComponentSender<Self>, user: bool) {
        if self.updates.busy {
            return;
        }
        self.updates.busy = true;
        sender.oneshot_command(async move {
            let result = tokio::task::spawn_blocking(|| nu::check(&APP))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()));
            CommandMsg::UpdateChecked { user, result }
        });
    }

    pub(super) fn update_checked(
        &mut self,
        sender: &ComponentSender<Self>,
        root: &ndl::ApplicationWindow,
        user: bool,
        result: Result<nu::Check, String>,
    ) {
        self.updates.busy = false;
        if !user {
            self.settings.last_update_check = now();
            self.settings.save();
        }
        match result {
            Ok(nu::Check::UpToDate) if user => {
                self.toast(&i18n::update::up_to_date(APP.version));
            }
            Ok(nu::Check::UpToDate) => {}
            Ok(nu::Check::Available(release)) => self.offer_update(sender, root, release),
            Err(why) if user => self.toast(&i18n::update::check_failed(&why)),
            Err(why) => tracing::info!(%why, "automatic update check failed"),
        }
    }

    fn offer_update(
        &mut self,
        sender: &ComponentSender<Self>,
        root: &ndl::ApplicationWindow,
        release: nu::Release,
    ) {
        let in_place = nu::install_kind(&APP).can_apply_in_place();
        let mut body = release_notes(&release.notes, i18n::current());
        if body.is_empty() {
            body = i18n::update::available_body_fallback();
        }
        if !in_place {
            body = format!("{body}\n\n{}", i18n::update::managed_hint());
        }
        let dialog = ndl::AlertDialog::new(
            Some(&i18n::update::available_title(&release.version)),
            Some(&body),
        );
        dialog.add_response("later", &i18n::update::later());
        if in_place {
            dialog.add_response("install", &i18n::update::install());
            dialog.set_response_appearance("install", ndl::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("install"));
        } else {
            dialog.add_response("page", &i18n::update::open_page());
            dialog.set_response_appearance("page", ndl::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("page"));
        }
        dialog.set_close_response("later");
        self.updates.release = Some(release);

        let sender = sender.clone();
        dialog.connect_response(None, move |_, response| match response {
            "install" => sender.input(AppMsg::InstallUpdate),
            "page" => sender.input(AppMsg::OpenReleasePage),
            _ => {}
        });
        dialog.present(Some(root));
    }

    pub(super) fn open_release_page(&self, root: &ndl::ApplicationWindow) {
        let Some(release) = &self.updates.release else {
            return;
        };
        // The portal inside a Flatpak, the browser otherwise: `GtkUriLauncher` is the same call
        // in both.
        gtk::UriLauncher::new(&release.page).launch(
            Some(root),
            gtk::gio::Cancellable::NONE,
            |result| {
                if let Err(err) = result {
                    tracing::warn!(?err, "could not open the release page");
                }
            },
        );
    }

    /// Download, verify and install the release the question was about, then restart.
    pub(super) fn install_update(
        &mut self,
        sender: &ComponentSender<Self>,
        root: &ndl::ApplicationWindow,
    ) {
        let Some(release) = self.updates.release.clone() else {
            return;
        };
        if self.updates.busy {
            return;
        }
        self.updates.busy = true;

        let bar = gtk::ProgressBar::builder().margin_top(12).build();
        bar.pulse();
        let dialog =
            ndl::AlertDialog::new(Some(&i18n::update::downloading(&release.version)), None);
        dialog.set_extra_child(Some(&bar));
        dialog.present(Some(root));
        self.updates.progress = Some((dialog, bar));

        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    let progress = out.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        let mut last = u64::MAX;
                        let package = nu::download(&APP, &release, |received, total| {
                            // Whole percent when the size is known, whole MiB when not: the
                            // window does not need 5,000 updates for one download.
                            let step = match total {
                                Some(total) if total > 0 => received.saturating_mul(100) / total,
                                _ => received >> 20,
                            };
                            if step != last {
                                last = step;
                                let _ =
                                    progress.send(CommandMsg::UpdateProgress { received, total });
                            }
                        })?;
                        nu::apply(&APP, &package, || Ok(()))
                    })
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e| e.to_string()));
                    let _ = out.send(CommandMsg::UpdateInstalled(result));
                })
                .drop_on_shutdown()
        });
    }

    pub(super) fn update_progress(&self, received: u64, total: Option<u64>) {
        let Some((_, bar)) = &self.updates.progress else {
            return;
        };
        match total {
            Some(total) if total > 0 => bar.set_fraction((received as f64 / total as f64).min(1.0)),
            _ => bar.pulse(),
        }
    }

    pub(super) fn update_installed(
        &mut self,
        sender: &ComponentSender<Self>,
        result: Result<(), String>,
    ) {
        self.updates.busy = false;
        match result {
            Ok(()) => {
                if let Some((dialog, _)) = &self.updates.progress {
                    let version = self
                        .updates
                        .release
                        .as_ref()
                        .map_or("", |r| r.version.as_str())
                        .to_owned();
                    dialog.set_heading(Some(&i18n::update::restarting(&version)));
                }
                // The new copy is already starting (the AppImage) or a helper is waiting for
                // this process to end (macOS). Quit through the daemon first so the old player
                // stops with the old window, and do not rely on the main loop alone to end.
                sender.input(AppMsg::Quit);
                gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(5), || {
                    std::process::exit(0);
                });
            }
            Err(why) => {
                if let Some((dialog, _)) = self.updates.progress.take() {
                    dialog.dialog().force_close();
                }
                self.toast(&i18n::update::install_failed(&why));
            }
        }
    }
}

/// The part of a release's notes in `language`, as plain text and short enough for a question.
///
/// Releases are written bilingually, with `**Español**` and `**English**` headings; a release
/// that is not falls through whole.
fn release_notes(notes: &str, language: Language) -> String {
    const ES: &str = "**Español**";
    const EN: &str = "**English**";
    let section = |start: &str, other: &str| -> Option<String> {
        let from = notes.find(start)? + start.len();
        let rest = &notes[from..];
        let end = rest.find(other).unwrap_or(rest.len());
        Some(rest[..end].to_owned())
    };
    let picked = match language {
        Language::Spanish => section(ES, EN),
        Language::English => section(EN, ES),
    }
    .unwrap_or_else(|| notes.to_owned());

    let plain: Vec<String> = picked
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches('#')
                .replace("**", "")
                .replace('`', "")
                .trim()
                .to_owned()
        })
        .filter(|line| !line.is_empty())
        .collect();
    let text = plain.join("\n");

    const LIMIT: usize = 700;
    if text.chars().count() <= LIMIT {
        return text;
    }
    let cut: String = text.chars().take(LIMIT).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The release workflow's package names, checked against what the updater looks for. A
    /// rename there that the updater does not know about would strand people on the old version.
    #[test]
    fn the_released_packages_are_the_ones_the_updater_installs() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let read = |path: &str| std::fs::read_to_string(format!("{root}/{path}")).unwrap();
        let release = read(".github/workflows/release.yml");
        assert!(release.contains(
            r#""dist/macos/Vinilo Nodalia.app" "dist/Vinilo-${VERSION}-nodalia-macos-arm64.dmg""#
        ));
        assert_eq!(APP.bundle, "Vinilo Nodalia.app");
        assert_eq!(APP.edition, Some("nodalia"));
        assert_eq!(APP.repo, "getnodalia/vinilo");
        let appimage = read("packaging/release/appimage.sh");
        assert!(appimage.contains(r#"OUTPUT="$OUT_DIR/Vinilo-${VERSION}-${ARCH}.AppImage""#));
        for (package, system) in [
            ("Vinilo-1.0.0-nodalia-macos-arm64.dmg", ("macos", "aarch64")),
            ("Vinilo-1.0.0-x86_64.AppImage", ("linux", "x86_64")),
        ] {
            assert!(
                nu::systems_for_package(package).contains(&system),
                "{package} for {system:?}"
            );
        }
    }

    const NOTES: &str = "## Vinilo 2.0.1\n\n**Español**\n\n- Arreglo uno.\n- Arreglo `dos`.\n\n**English**\n\n- Fix one.\n- Fix **two**.\n";

    #[test]
    fn the_notes_follow_the_language() {
        assert_eq!(
            release_notes(NOTES, Language::English),
            "- Fix one.\n- Fix two."
        );
        assert_eq!(
            release_notes(NOTES, Language::Spanish),
            "- Arreglo uno.\n- Arreglo dos."
        );
    }

    #[test]
    fn notes_without_language_headings_pass_through_as_plain_text() {
        assert_eq!(
            release_notes("## Title\n\n**Bold** text", Language::English),
            "Title\nBold text"
        );
    }

    #[test]
    fn long_notes_are_cut() {
        let long = "word ".repeat(400);
        let cut = release_notes(&long, Language::English);
        assert!(cut.chars().count() <= 701);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn a_check_is_due_once_a_day() {
        assert!(due(0, 1_000_000));
        assert!(!due(1_000_000, 1_000_000 + EVERY - 1));
        assert!(due(1_000_000, 1_000_000 + EVERY));
        assert!(
            !due(5, 1),
            "a clock that went backwards waits rather than checking"
        );
    }
}
