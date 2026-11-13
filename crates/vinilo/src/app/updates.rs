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
        let dialog =
            ndl::AlertDialog::new(Some(&i18n::update::available_title(&release.version)), None);
        // Wider than a plain question: the notes read as a list, not a sentence.
        dialog.dialog().set_content_width(440);
        dialog.set_extra_child(Some(&offer_body(&release.notes, in_place)));
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

/// The release notes in the reader's language, left-aligned and scrolling on their own so the
/// heading and the buttons never leave the window, then the hint for a managed install.
fn offer_body(notes: &str, in_place: bool) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let mut markup = nu::notes_markup(notes, notes_language(i18n::current()));
    if markup.is_empty() {
        markup = gtk::glib::markup_escape_text(&i18n::update::available_body_fallback()).into();
    }
    let label = gtk::Label::builder()
        .use_markup(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .yalign(0.0)
        .halign(gtk::Align::Fill)
        .build();
    label.set_markup(&markup);
    label.add_css_class("body");
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(NOTES_HEIGHT)
        .child(&label)
        .build();
    column.append(&scroller);
    if !in_place {
        let hint = gtk::Label::builder()
            .label(i18n::update::managed_hint())
            .wrap(true)
            .xalign(0.0)
            .build();
        hint.add_css_class("dim-label");
        column.append(&hint);
    }
    column
}

/// The notes scroll past this height.
const NOTES_HEIGHT: i32 = 260;

fn notes_language(language: Language) -> nu::Language {
    match language {
        Language::English => nu::Language::English,
        Language::Spanish => nu::Language::Spanish,
    }
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

    /// The body Vinilo 0.2.3 shipped with: both languages, then a downloads table. The update
    /// window showed all of it as raw Markdown and lost its buttons under it.
    const NOTES: &str = include_str!("../../../../docs/release-notes/v0.2.3.md");

    #[test]
    fn the_offer_shows_only_the_readers_language_without_markdown() {
        let es = nu::notes_markup(NOTES, notes_language(Language::Spanish));
        assert!(
            es.starts_with("<b>Correcciones en Vinilo Native</b>"),
            "{es}"
        );
        assert!(es.contains("<b>Instalar y reiniciar</b>"));
        let en = nu::notes_markup(NOTES, notes_language(Language::English));
        assert!(en.starts_with("<b>Fixes in Vinilo Native</b>"), "{en}");
        for text in [&es, &en] {
            assert!(!text.contains("**") && !text.contains("##"), "{text}");
            assert!(!text.contains("AppImage"), "no downloads table: {text}");
        }
        assert!(!en.contains("Correcciones") && !es.contains("Fixes in"));
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
