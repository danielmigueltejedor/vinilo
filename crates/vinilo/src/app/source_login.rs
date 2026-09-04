// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Catalogue sign-in, the same shape as Apple Music's login window.
//!
//! Apple opens MusicKit's own Chromium page. Spotify / YouTube Music / Tidal
//! have no equivalent sidecar, so this window *is* the login: a WebKit view
//! of the service, cookies dumped to a Netscape file, then `yt-dlp --cookies`
//! for playback. The first-run gate (`can_close(false)`) stays up until that
//! dump exists, which is why picking an unconfigured source feels like Apple's
//! signed-out screen rather than a toast you can dismiss.

use std::cell::Cell;
use std::rc::Rc;

use relm4::adw::prelude::*;
use relm4::{ComponentSender, adw, gtk};
use vinilo_core::i18n::{self, Key};
use vinilo_core::provider::Provider;
use vinilo_core::setup::{self, NetscapeCookie};
use webkit6::prelude::*;
use webkit6::{CookieAcceptPolicy, LoadEvent, NetworkSession, WebView};

use super::{AppModel, AppMsg};

fn t(key: Key) -> &'static str {
    i18n::t(key)
}

const CHROME_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";

/// The login window plus the cookie manager we still need after it closes.
#[derive(Clone)]
pub(super) struct CatalogLogin {
    pub window: adw::Window,
    pub cookies: webkit6::CookieManager,
    pub provider: Provider,
}

impl AppModel {
    /// Blocking gate, copied from Apple's onboarding: Sign In, a note, Quit.
    pub(super) fn present_catalog_onboarding(
        &self,
        sender: &ComponentSender<Self>,
        parent: &adw::ApplicationWindow,
        provider: Provider,
    ) -> adw::Dialog {
        let ytdlp = std::process::Command::new("yt-dlp")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        let page = adw::StatusPage::builder()
            .icon_name(crate::APP_ID)
            .title(i18n::catalog_heading(provider))
            .description(t(Key::CatalogSetupBody))
            .build();

        let button = gtk::Button::builder()
            .label(i18n::catalog_sign_in(provider))
            .halign(gtk::Align::Center)
            .css_classes(["suggested-action", "pill"])
            .build();
        {
            let sender = sender.clone();
            button.connect_clicked(move |_| sender.input(AppMsg::CatalogSignIn));
        }

        let note = gtk::Label::builder()
            .label(t(Key::CatalogSignInNote))
            .justify(gtk::Justification::Center)
            .wrap(true)
            .max_width_chars(46)
            .css_classes(["caption", "dim-label"])
            .build();

        let ytdlp_note = gtk::Label::builder()
            .label(t(if ytdlp {
                Key::CatalogSetupYtOk
            } else {
                Key::CatalogSetupYtMissing
            }))
            .justify(gtk::Justification::Center)
            .wrap(true)
            .max_width_chars(46)
            .css_classes(["caption", "dim-label"])
            .build();

        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Center)
            .spacing(18)
            .build();
        column.append(&button);
        column.append(&note);
        column.append(&ytdlp_note);
        page.set_child(Some(&column));

        let quit = gtk::Button::builder()
            .label(i18n::quit_button())
            .css_classes(["flat"])
            .build();
        {
            let sender = sender.clone();
            quit.connect_clicked(move |_| sender.input(AppMsg::Quit));
        }

        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .css_classes(["flat"])
            .build();
        header.set_title_widget(Some(&gtk::Label::new(None)));
        header.pack_end(&quit);

        let view = adw::ToolbarView::builder().content(&page).build();
        view.add_top_bar(&header);

        let dialog = adw::Dialog::builder()
            .child(&view)
            .content_width(480)
            .can_close(false)
            .build();

        let shortcuts = gtk::ShortcutController::new();
        shortcuts.set_scope(gtk::ShortcutScope::Local);
        shortcuts.set_propagation_phase(gtk::PropagationPhase::Capture);
        let sender = sender.clone();
        shortcuts.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>q"),
            Some(gtk::CallbackAction::new(move |_, _| {
                sender.input(AppMsg::Quit);
                gtk::glib::Propagation::Stop
            })),
        ));
        dialog.add_controller(shortcuts);

        dialog.present(Some(parent));
        dialog
    }

    /// Separate window with the service's own sign-in page, like MusicKit.
    pub(super) fn present_catalog_login(
        &mut self,
        sender: &ComponentSender<Self>,
        parent: &adw::ApplicationWindow,
        provider: Provider,
    ) {
        if let Some(existing) = &self.catalog_login {
            existing.window.present();
            return;
        }

        let data_dir = setup::webkit_data_dir(provider);
        let cache_dir = setup::webkit_cache_dir(provider);
        if let Some(dir) = &data_dir {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Some(dir) = &cache_dir {
            let _ = std::fs::create_dir_all(dir);
        }

        let session = NetworkSession::new(
            data_dir.as_deref().and_then(|p| p.to_str()),
            cache_dir.as_deref().and_then(|p| p.to_str()),
        );
        let cookies = match session.cookie_manager() {
            Some(manager) => manager,
            None => {
                tracing::error!("webkit session has no cookie manager");
                return;
            }
        };
        cookies.set_accept_policy(CookieAcceptPolicy::Always);

        let webview = WebView::builder().network_session(&session).build();
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
            settings.set_user_agent(Some(CHROME_UA));
        }
        webview.load_uri(setup::login_url(provider));

        let finished = Rc::new(Cell::new(false));
        {
            let sender = sender.clone();
            let finished = finished.clone();
            webview.connect_load_changed(move |view, event| {
                if event != LoadEvent::Finished || finished.get() {
                    return;
                }
                let uri = view.uri().unwrap_or_default();
                if setup::uri_looks_signed_in(provider, &uri) {
                    finished.set(true);
                    sender.input(AppMsg::CatalogLoginFinished);
                }
            });
        }

        let done = gtk::Button::builder()
            .label(t(Key::CatalogLoginDone))
            .css_classes(["suggested-action", "pill"])
            .build();
        {
            let sender = sender.clone();
            done.connect_clicked(move |_| sender.input(AppMsg::CatalogLoginFinished));
        }

        let browser = gtk::Button::builder()
            .label(t(Key::CatalogOpenBrowser))
            .css_classes(["flat"])
            .build();
        {
            let url = setup::login_url(provider).to_owned();
            browser.connect_clicked(move |_| {
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            });
        }

        let header = adw::HeaderBar::builder().css_classes(["flat"]).build();
        header.set_title_widget(Some(&gtk::Label::new(Some(&i18n::catalog_sign_in(
            provider,
        )))));
        header.pack_start(&browser);
        header.pack_end(&done);

        let view = adw::ToolbarView::builder().content(&webview).build();
        view.add_top_bar(&header);

        let window = adw::Window::builder()
            .title(i18n::catalog_sign_in(provider))
            .default_width(720)
            .default_height(780)
            .transient_for(parent)
            .modal(true)
            .content(&view)
            .build();

        {
            let sender = sender.clone();
            window.connect_close_request(move |_| {
                sender.input(AppMsg::CatalogLoginClosed);
                gtk::glib::Propagation::Proceed
            });
        }

        window.present();
        self.catalog_login = Some(CatalogLogin {
            window,
            cookies,
            provider,
        });
    }

    pub(super) fn close_catalog_login(&mut self) {
        if let Some(login) = self.catalog_login.take() {
            login.window.close();
        }
    }

    /// Dump cookies (including session ones), then mark the source configured.
    pub(super) fn finish_catalog_login(&mut self, sender: &ComponentSender<Self>) {
        if self.catalog_login_busy {
            return;
        }
        let Some(login) = self.catalog_login.clone() else {
            return;
        };
        self.catalog_login_busy = true;
        let provider = login.provider;
        let cookies = login.cookies.clone();
        let sender = sender.clone();
        gtk::glib::spawn_future_local(async move {
            let dumped = dump_cookies(&cookies, provider).await;
            if !dumped {
                std::thread::spawn(move || import_browser_cookies(provider));
            }
            sender.input(AppMsg::CatalogSignedIn);
        });
    }
}

async fn dump_cookies(manager: &webkit6::CookieManager, provider: Provider) -> bool {
    let Some(path) = setup::cookies_path(provider) else {
        return false;
    };
    let mut jar: Vec<NetscapeCookie> = Vec::new();
    for uri in setup::cookie_uris(provider) {
        match manager.cookies_future(uri).await {
            Ok(list) => {
                for mut cookie in list {
                    jar.push(from_soup(&mut cookie));
                }
            }
            Err(err) => tracing::warn!(%uri, ?err, "could not read login cookies"),
        }
    }
    jar.sort_by(|a, b| a.name.cmp(&b.name).then(a.domain.cmp(&b.domain)));
    jar.dedup_by(|a, b| a.name == b.name && a.domain == b.domain && a.path == b.path);
    if let Err(err) = setup::write_netscape(&path, &jar) {
        tracing::warn!(?err, "could not write cookie jar");
        return false;
    }
    let names: Vec<String> = jar.into_iter().map(|c| c.name).collect();
    setup::looks_signed_in(provider, &names)
}

fn from_soup(cookie: &mut webkit6::soup::Cookie) -> NetscapeCookie {
    let domain = cookie.domain().unwrap_or_default().to_string();
    let host_only = !domain.starts_with('.');
    let expires = cookie.expires().map(|date| date.to_unix()).unwrap_or(0);
    NetscapeCookie {
        domain,
        host_only,
        path: cookie.path().unwrap_or_default().to_string(),
        secure: cookie.is_secure(),
        http_only: cookie.is_http_only(),
        expires,
        name: cookie.name().unwrap_or_default().to_string(),
        value: cookie.value().unwrap_or_default().to_string(),
    }
}

/// If the WebView session is empty (Google blocking WebKit, user chose
/// the system browser), copy cookies out of a desktop browser via yt-dlp.
fn import_browser_cookies(provider: Provider) {
    let Some(dest) = setup::cookies_path(provider) else {
        return;
    };
    if setup::looks_signed_in(provider, &setup::cookie_names(provider)) {
        return;
    }
    for browser in ["firefox", "chromium", "google-chrome", "chrome", "brave"] {
        let status = std::process::Command::new("yt-dlp")
            .args([
                "--cookies-from-browser",
                browser,
                "--cookies",
                &dest.to_string_lossy(),
                "--skip-download",
                "--no-warnings",
                setup::login_url(provider),
            ])
            .status();
        if status.map(|s| s.success()).unwrap_or(false) && dest.is_file() {
            tracing::info!(browser, "imported cookies from the desktop browser");
            return;
        }
    }
}
