// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's furniture: the primary menu's actions and accelerators, and the
//! first-run dialogs behind them.
//!
//! All built imperatively rather than in `view!`, because they are presented on
//! demand and own no state of their own — every change one of them makes goes
//! straight back through an `AppMsg`, so the reducer stays the only writer.

use std::cell::Cell;
use std::rc::Rc;

use relm4::adw::prelude::*;
use relm4::gtk::prelude::WidgetExt;
use relm4::{ComponentSender, adw, gtk};

use super::{AppModel, AppMsg};
use crate::style::Accent;
use vinilo_core::i18n::{self, Key, Language};

fn t(key: Key) -> &'static str {
    i18n::t(key)
}

impl super::AppModel {
    /// Point the sort popover at the section now showing.
    ///
    /// Both halves are needed. The **menu** changes because the keys differ per
    /// section, and the **action states** change because each section remembers
    /// its own choice — without the second, the radio dot would sit on whatever
    /// the last section chose and lie about the list underneath it.
    ///
    /// Artists get no key list at all: a library artist carries only a name, so
    /// there is nothing to choose between and the popover is just the direction
    /// toggle.
    pub(super) fn sync_sort_menu(&self, button: &gtk::MenuButton) {
        use gtk::prelude::ToVariant;
        let sort = self.sorts.get(self.view);

        let menu = gtk::gio::Menu::new();
        let direction = gtk::gio::Menu::new();
        direction.append(Some(t(Key::ReverseOrder)), Some("sort.reverse"));
        menu.append_section(None, &direction);
        if super::SortBy::for_view(self.view.sortable()).len() > 1 {
            menu.prepend_section(None, &sort_keys_menu(self.view));
        }
        button.set_menu_model(Some(&menu));

        if let Some((by, reverse)) = &self.sort_actions {
            by.set_state(&sort.by.id().to_variant());
            reverse.set_state(&sort.reversed.to_variant());
        }
    }
}

/// The radio list in the sort popover, for whatever section is showing.
///
/// Rebuilt on every section change rather than filtered from one fixed list,
/// because the keys are not a subset of each other: a playlist has no artist,
/// an album has a date added and a song does not. See `SortBy::for_view` for
/// which are honest where — it is a measurement, not a preference.
pub(super) fn sort_keys_menu(view: super::View) -> gtk::gio::Menu {
    use gtk::prelude::ToVariant;
    let keys = gtk::gio::Menu::new();
    for option in super::SortBy::for_view(view.sortable()) {
        let item = gtk::gio::MenuItem::new(Some(option.label()), None);
        item.set_action_and_target_value(Some("sort.by"), Some(&option.id().to_variant()));
        keys.append_item(&item);
    }
    keys
}

// The primary menu's action group. GTK menu items invoke `GAction`s by name;
// each of these bridges to an `AppMsg` so the reducer stays the only place
// state changes.
relm4::new_action_group!(AppMenuActionGroup, "win");
relm4::new_stateless_action!(PreferencesAction, AppMenuActionGroup, "preferences");
relm4::new_stateless_action!(ShortcutsAction, AppMenuActionGroup, "shortcuts");
relm4::new_stateless_action!(AboutAction, AppMenuActionGroup, "about");
relm4::new_stateless_action!(PlayPauseAction, AppMenuActionGroup, "play-pause");
relm4::new_stateless_action!(NextAction, AppMenuActionGroup, "next");
relm4::new_stateless_action!(PreviousAction, AppMenuActionGroup, "previous");
relm4::new_stateless_action!(VolumeUpAction, AppMenuActionGroup, "volume-up");
relm4::new_stateless_action!(VolumeDownAction, AppMenuActionGroup, "volume-down");
relm4::new_stateless_action!(CloseWindowAction, AppMenuActionGroup, "close-window");
relm4::new_stateless_action!(ToggleQueueAction, AppMenuActionGroup, "toggle-queue");
relm4::new_stateless_action!(ToggleSidebarAction, AppMenuActionGroup, "toggle-sidebar");
relm4::new_stateless_action!(SignOutAction, AppMenuActionGroup, "sign-out");
relm4::new_stateless_action!(FocusSearchAction, AppMenuActionGroup, "focus-search");
relm4::new_stateless_action!(SupportAction, AppMenuActionGroup, "support");

/// Wire the primary menu's actions to messages, with their accelerators.
pub(super) fn register_actions(
    window: &adw::ApplicationWindow,
    sender: &ComponentSender<AppModel>,
) {
    use relm4::actions::{AccelsPlus, RelmAction, RelmActionGroup};

    let mut group = RelmActionGroup::<AppMenuActionGroup>::new();

    let s = sender.clone();
    group.add_action(RelmAction::<PreferencesAction>::new_stateless(move |_| {
        s.input(AppMsg::ShowPreferences)
    }));
    let s = sender.clone();
    group.add_action(RelmAction::<ShortcutsAction>::new_stateless(move |_| {
        s.input(AppMsg::ShowShortcuts)
    }));
    let s = sender.clone();
    group.add_action(RelmAction::<SignOutAction>::new_stateless(move |_| {
        s.input(AppMsg::SignOut)
    }));
    let s = sender.clone();
    group.add_action(RelmAction::<AboutAction>::new_stateless(move |_| {
        s.input(AppMsg::ShowAbout)
    }));
    // **Application-scoped, not window-scoped.** A `win.` action resolves
    // through whatever currently holds focus, and the first-run gate is an
    // `adw::Dialog` presented into the window's own dialog host — so the one
    // moment a user most needs a way out is the moment that scope is least
    // certain. `app.quit` is reachable from any focus scope, and is the GNOME
    // convention besides.
    //
    // It matters more than it looks: an `adw::Dialog` with `can_close(false)`
    // also blocks the window's close request, so while the gate is up the title
    // bar button does nothing either. Between that and Quit missing from the
    // primary menu, a signed-out app had no visible way to exit at all.
    let app = relm4::main_application();
    let quit = gtk::gio::SimpleAction::new("quit", None);
    let s = sender.clone();
    quit.connect_activate(move |_, _| s.input(AppMsg::Quit));
    app.add_action(&quit);

    // Transport, so the app answers the keyboard even when the bar does not
    // have focus. Media keys already arrive over MPRIS; these are the
    // in-window equivalents.
    let s = sender.clone();
    group.add_action(RelmAction::<PlayPauseAction>::new_stateless(move |_| {
        s.input(AppMsg::PlayPause)
    }));
    let s = sender.clone();
    group.add_action(RelmAction::<NextAction>::new_stateless(move |_| {
        s.input(AppMsg::Next)
    }));
    let s = sender.clone();
    group.add_action(RelmAction::<PreviousAction>::new_stateless(move |_| {
        s.input(AppMsg::Previous)
    }));
    let s = sender.clone();
    group.add_action(RelmAction::<VolumeUpAction>::new_stateless(move |_| {
        s.input(AppMsg::VolumeUp)
    }));
    let s = sender.clone();
    group.add_action(RelmAction::<VolumeDownAction>::new_stateless(move |_| {
        s.input(AppMsg::VolumeDown)
    }));
    // The keyboard equivalent of the close button, and deliberately the *same*
    // message — so it inherits the same two meanings: hide and keep playing when
    // something is loaded, quit when nothing is. A `Ctrl`+`W` that quit outright
    // while the close button did not would be the worse kind of surprise.
    let s = sender.clone();
    group.add_action(RelmAction::<CloseWindowAction>::new_stateless(move |_| {
        s.input(AppMsg::WindowCloseRequested)
    }));

    let s = sender.clone();
    group.add_action(RelmAction::<ToggleQueueAction>::new_stateless(move |_| {
        s.input(AppMsg::ToggleQueue)
    }));
    let s = sender.clone();
    group.add_action(RelmAction::<ToggleSidebarAction>::new_stateless(
        move |_| s.input(AppMsg::ToggleSidebar),
    ));
    let s = sender.clone();
    group.add_action(RelmAction::<FocusSearchAction>::new_stateless(move |_| {
        s.input(AppMsg::FocusSearch)
    }));
    let s = sender.clone();
    group.add_action(RelmAction::<SupportAction>::new_stateless(move |_| {
        s.input(AppMsg::OpenSupport)
    }));

    app.set_accelerators_for_action::<PreferencesAction>(&["<Control>comma"]);
    app.set_accelerators_for_action::<ShortcutsAction>(&["<Control>question"]);
    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accelerators_for_action::<CloseWindowAction>(&["<Control>w"]);
    app.set_accelerators_for_action::<PlayPauseAction>(&["<Control>k"]);
    app.set_accelerators_for_action::<NextAction>(&["<Control>Right"]);
    app.set_accelerators_for_action::<PreviousAction>(&["<Control>Left"]);
    app.set_accelerators_for_action::<VolumeUpAction>(&["<Control>Up"]);
    app.set_accelerators_for_action::<VolumeDownAction>(&["<Control>Down"]);
    app.set_accelerators_for_action::<ToggleQueueAction>(&["<Control>u"]);
    // F9 is the GNOME convention for showing and hiding a sidebar.
    app.set_accelerators_for_action::<ToggleSidebarAction>(&["F9"]);
    app.set_accelerators_for_action::<FocusSearchAction>(&["<Control>f"]);

    group.register_for_widget(window);
}

/// Check an icon name against the theme, falling back if it is missing.
///
/// A name that does not exist renders as nothing at all — silently, with no
/// warning — which is how `music-note-single-symbolic` shipped as an invisible
/// icon. Colloid's Bold pack is the *symbolic* 1.5px set; colour app icons
/// have no bold cut, so this keeps the `-symbolic` suffix on purpose.
pub(super) fn icon(name: &'static str) -> &'static str {
    if crate::nodalix::has_icon(name) {
        name
    } else {
        tracing::warn!(icon = name, "icon missing from the theme; falling back");
        "audio-x-generic-symbolic"
    }
}

pub(super) fn show_about(parent: &adw::ApplicationWindow) {
    let about = adw::AboutDialog::builder()
        .application_name(t(Key::AppName))
        .application_icon(crate::APP_ID)
        .developer_name("Daniel Miguel Tejedor")
        .version(env!("CARGO_PKG_VERSION"))
        .license_type(gtk::License::Gpl30)
        .comments(t(Key::AboutComments))
        .build();
    about.present(Some(parent));
}

/// Where to say thank you.
pub(super) const SUPPORT_URL: &str = "https://ko-fi.com/miguelrincon";

/// Open the support page in the user's browser.
///
/// `GtkUriLauncher` rather than `gio::AppInfo`: inside a Flatpak it goes
/// through the OpenURI portal, which is the only route out of the sandbox —
/// and it is the same call whether sandboxed or not, so there is nothing to
/// branch on.
pub(super) fn open_support(parent: &adw::ApplicationWindow) {
    gtk::UriLauncher::new(SUPPORT_URL).launch(
        Some(parent),
        gtk::gio::Cancellable::NONE,
        |result| {
            // Nothing to recover: if no browser answered, a toast telling them
            // so would be one more thing that cannot open a browser either.
            if let Err(err) = result {
                tracing::warn!(?err, "could not open the support page");
            }
        },
    );
}

pub(super) fn show_shortcuts(parent: &adw::ApplicationWindow) {
    // Built by hand rather than from a .ui file: it is a dozen lines either
    // way, and this keeps the strings next to the code that implements them.
    let dialog = adw::ShortcutsDialog::new();

    let playback = adw::ShortcutsSection::new(Some(t(Key::ShortcutsPlayback)));
    for (title, accel) in [
        (t(Key::ShortcutPlayPause), "<Control>k"),
        (t(Key::ShortcutNext), "<Control>Right"),
        (t(Key::ShortcutPrevious), "<Control>Left"),
        (t(Key::ShortcutVolumeUp), "<Control>Up"),
        (t(Key::ShortcutVolumeDown), "<Control>Down"),
    ] {
        playback.add(adw::ShortcutsItem::new(title, accel));
    }

    let general = adw::ShortcutsSection::new(Some(t(Key::ShortcutsGeneral)));
    for (title, accel) in [
        (t(Key::ShortcutSearch), "<Control>f"),
        (t(Key::ShortcutCloseWindow), "<Control>w"),
        (t(Key::ShortcutToggleSidebar), "F9"),
        (t(Key::ShortcutToggleQueue), "<Control>u"),
        (t(Key::ShortcutPreferences), "<Control>comma"),
        (t(Key::ShortcutShortcuts), "<Control>question"),
        (t(Key::ShortcutQuit), "<Control>q"),
    ] {
        general.add(adw::ShortcutsItem::new(title, accel));
    }

    dialog.add(playback);
    dialog.add(general);
    dialog.present(Some(parent));
}

impl AppModel {
    /// The first-run gate.
    ///
    /// A modal that cannot be dismissed, rather than a page behind a usable
    /// window. Signed out, every control in the app is a control that cannot
    /// work: the sidebar sections fire library loads, the search box queries a
    /// catalog that answers 403, and the transport talks to a player with no
    /// session. Leaving them reachable meant a 403 per second against Apple —
    /// blocking is not a nicety here, it is the correct behaviour.
    ///
    /// Dismissed from `update` the moment the sidecar reports an authorized
    /// session, never by the user.
    pub(super) fn present_onboarding(
        &self,
        sender: &ComponentSender<Self>,
        parent: &adw::ApplicationWindow,
    ) -> adw::Dialog {
        let page = adw::StatusPage::builder()
            .icon_name(crate::APP_ID)
            .title(t(Key::WelcomeTitle))
            .description(t(Key::WelcomeBody))
            .build();

        let button = gtk::Button::builder()
            .label(t(Key::SignIn))
            .halign(gtk::Align::Center)
            .css_classes(["suggested-action", "pill"])
            .build();
        {
            let sender = sender.clone();
            button.connect_clicked(move |_| sender.input(AppMsg::SignIn));
        }

        // Said before the button is pressed, not after. A browser window
        // opening out of a native app is alarming when it is a surprise, and
        // this is the one moment Vinilo cannot hide the web engine.
        let note = gtk::Label::builder()
            .label(t(Key::SignInNote))
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
        page.set_child(Some(&column));

        // A way out of the app, on the one screen that otherwise has none:
        // `can_close(false)` stops the window's own close button too, so
        // without this the gate is a dead end for anyone who does not want to
        // sign in right now.
        //
        // In the corner rather than under the call to action. Below Sign In it
        // sat in the reading order as if it were the second step, and it is not
        // a step at all — it is the way out. Flat, and not destructive:
        // quitting is ordinary, and red would imply it discards something.
        let quit = gtk::Button::builder()
            .label(i18n::quit_button())
            .css_classes(["flat"])
            .build();
        {
            let sender = sender.clone();
            quit.connect_clicked(move |_| sender.input(AppMsg::Quit));
        }

        // The bar exists only to hold that button — the dialog cannot be
        // closed, so there are no window controls to show and no title to
        // repeat above the status page's own.
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .css_classes(["flat"])
            .build();
        header.set_title_widget(Some(&gtk::Label::new(None)));
        header.pack_end(&quit);

        let view = adw::ToolbarView::builder().content(&page).build();
        view.add_top_bar(&header);

        // **Width only.** A fixed `content_height` was what made this scroll:
        // `adw::StatusPage` puts its content in a scrolled window, so any
        // height smaller than the natural one produces a scrollbar — and 420
        // was smaller, on a dialog with a heading, two short paragraphs and a
        // button. Left unset, the dialog takes the height its content asks for
        // and there is nothing to scroll.
        let dialog = adw::Dialog::builder()
            .child(&view)
            .content_width(480)
            // No escape, no click-outside: there is nothing behind this worth
            // reaching until there is a session.
            .can_close(false)
            .build();

        // Ctrl+Q, again, because the gate swallows the application one.
        //
        // Moving the action from `win.quit` to `app.quit` was not enough:
        // tested by hand with the gate up, the Quit button works — so
        // `main_application().quit()` is fine — while the accelerator never
        // arrives. A modal `adw::Dialog` holds the focus, and the application
        // shortcut does not survive that, whatever the scope of the action
        // behind it.
        //
        // So the dialog carries its own, local to it and its children, which is
        // exactly where the key is going. A `CallbackAction` rather than a
        // `NamedAction`: nothing to resolve by name, so there is no second
        // lookup that can fail the same quiet way the first one did.
        // **Capture, not bubble.** A bubbling controller runs on the way back
        // up, which is after anything nearer the focus has had its chance to
        // stop the event — and something is stopping it, or the application
        // accelerator would have worked. Capture runs on the way *down* from
        // the dialog, before its own children see the key, so nothing gets to
        // swallow it first. Safe here only because the gate holds no text
        // input: on a dialog with an entry, capturing Ctrl+Q would take it away
        // from the entry, which is why this is not the default anywhere else.
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

    /// Ask before signing out.
    ///
    /// Destructive and not obviously reversible from the user's side: it drops
    /// Apple's session, so getting back in means the login window again, with
    /// whatever two-factor prompt that involves. Worth a question.
    pub(super) fn confirm_sign_out(
        &self,
        sender: &ComponentSender<Self>,
        parent: &adw::ApplicationWindow,
    ) {
        let catalog = self.settings.provider.is_catalog();
        let dialog = adw::AlertDialog::new(
            Some(t(if catalog {
                Key::CatalogSignOutTitle
            } else {
                Key::SignOutTitle
            })),
            Some(t(if catalog {
                Key::CatalogSignOutBody
            } else {
                Key::SignOutBody
            })),
        );
        dialog.add_response("cancel", t(Key::Cancel));
        dialog.add_response("sign-out", i18n::sign_out_button());
        dialog.set_response_appearance("sign-out", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let sender = sender.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "sign-out" {
                sender.input(AppMsg::SignOutConfirmed);
            }
        });
        dialog.present(Some(parent));
    }

    /// Name a new playlist. Optional `track_id` is added after it exists.
    pub(super) fn prompt_new_playlist(
        &self,
        sender: &ComponentSender<Self>,
        parent: &adw::ApplicationWindow,
        track_id: Option<String>,
    ) {
        let dialog = adw::AlertDialog::new(Some(t(Key::NewPlaylistTitle)), None);
        let entry = gtk::Entry::builder()
            .placeholder_text(t(Key::NewPlaylistPlaceholder))
            .activates_default(true)
            .hexpand(true)
            .build();
        dialog.set_extra_child(Some(&entry));
        dialog.add_response("cancel", t(Key::Cancel));
        dialog.add_response("create", t(Key::Create));
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("create"));
        dialog.set_close_response("cancel");

        // Hold the entry ourselves. libadwaita unparents `extra_child` as the
        // dialog closes, often *before* this callback, which made Create a
        // silent no-op — type a name, click, nothing happens.
        let sender = sender.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "create" {
                return;
            }
            let name = entry.text().trim().to_string();
            if name.is_empty() {
                return;
            }
            tracing::info!(%name, has_track = track_id.is_some(), "creating playlist");
            sender.input(AppMsg::CreatePlaylist {
                name,
                track_id: track_id.clone(),
            });
        });
        dialog.present(Some(parent));
    }

    /// Preferences: appearance, and the track-change notification.
    ///
    /// Built imperatively rather than in `view!` because it is presented on
    /// demand and owns no state of its own — every change goes straight back
    /// through `AppMsg` so the reducer stays the only writer.
    pub(super) fn show_preferences(
        &self,
        sender: &ComponentSender<Self>,
        parent: &adw::ApplicationWindow,
    ) {
        let dialog = adw::PreferencesDialog::new();
        let page = adw::PreferencesPage::new();

        let appearance = adw::PreferencesGroup::builder()
            .title(t(Key::Appearance))
            .build();
        let theme = adw::ComboRow::builder()
            .title(t(Key::Theme))
            .model(&gtk::StringList::new(&[
                t(Key::ThemeSystem),
                t(Key::ThemeLight),
                t(Key::ThemeDark),
            ]))
            .selected(self.settings.theme.index())
            .build();
        {
            let sender = sender.clone();
            theme.connect_selected_notify(move |row| {
                sender.input(AppMsg::SetTheme(row.selected()));
            });
        }
        appearance.add(&theme);

        let names: Vec<&str> = Accent::ALL.iter().map(|a| a.label()).collect();
        let accent = adw::ComboRow::builder()
            .title(t(Key::AccentColour))
            .model(&gtk::StringList::new(&names))
            .selected(Accent::parse(&self.settings.accent).index())
            .build();
        {
            let sender = sender.clone();
            accent.connect_selected_notify(move |row| {
                sender.input(AppMsg::SetAccent(Accent::from_index(row.selected())));
            });
        }
        appearance.add(&accent);

        let source_tint = adw::SwitchRow::builder()
            .title(t(Key::SourceTint))
            .subtitle(t(Key::SourceTintSub))
            .active(self.settings.source_tint)
            .build();
        accent.set_sensitive(!self.settings.source_tint);
        {
            let sender = sender.clone();
            let accent = accent.clone();
            let armed = Rc::new(Cell::new(false));
            source_tint.connect_active_notify({
                let armed = armed.clone();
                move |row| {
                    if !armed.get() {
                        return;
                    }
                    accent.set_sensitive(!row.is_active());
                    sender.input(AppMsg::SetSourceTint(row.is_active()));
                }
            });
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(0), move || {
                armed.set(true);
            });
        }
        appearance.add(&source_tint);

        // #145: a colour field behind small type is still a preference, and
        // libadwaita's own surfaces are plain. Off, the two surfaces fall
        // back to the bar and sheet backgrounds they had before the wash
        // existed — there is nothing to draw instead.
        let backdrop = adw::SwitchRow::builder()
            .title(t(Key::AlbumArtBackdrop))
            .subtitle(t(Key::AlbumArtBackdropSub))
            .active(self.settings.player_backdrop)
            .build();
        {
            let sender = sender.clone();
            backdrop.connect_active_notify(move |row| {
                sender.input(AppMsg::SetPlayerBackdrop(row.is_active()));
            });
        }
        appearance.add(&backdrop);

        let language = adw::PreferencesGroup::builder()
            .title(t(Key::Language))
            .build();
        let language_row = adw::ComboRow::builder()
            .title(t(Key::Language))
            .subtitle(t(Key::LanguageSub))
            .model(&gtk::StringList::new(&[
                Language::English.native_name(),
                Language::Spanish.native_name(),
            ]))
            .selected(self.settings.language.index())
            .build();
        {
            let sender = sender.clone();
            let armed = Rc::new(Cell::new(false));
            language_row.connect_selected_notify({
                let armed = armed.clone();
                move |row| {
                    if !armed.get() {
                        return;
                    }
                    sender.input(AppMsg::SetLanguage(row.selected()));
                }
            });
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(0), move || {
                armed.set(true);
            });
        }
        language.add(&language_row);

        let source = adw::PreferencesGroup::builder()
            .title(t(Key::MusicSource))
            .build();
        let source_row = adw::ComboRow::builder()
            .title(t(Key::MusicSource))
            .subtitle(t(Key::MusicSourceSub))
            .model(&gtk::StringList::new(&[
                t(Key::ProviderApple),
                t(Key::ProviderLocal),
                t(Key::ProviderSpotify),
                t(Key::ProviderYoutube),
                t(Key::ProviderTidal),
            ]))
            .selected(self.settings.provider.index())
            .build();
        {
            let sender = sender.clone();
            let armed = Rc::new(Cell::new(false));
            source_row.connect_selected_notify({
                let armed = armed.clone();
                move |row| {
                    if !armed.get() {
                        return;
                    }
                    sender.input(AppMsg::SetProvider(row.selected()));
                }
            });
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(0), move || {
                armed.set(true);
            });
        }
        source.add(&source_row);

        // No group description. It carried a caveat about notifications needing
        // the app to be installed, which is a **developer's** problem — anyone
        // who has Preferences open from a Flatpak or `make install` is already
        // past it, and pointing at the README from inside a settings dialog is
        // not something a preferences pane should do.
        let notifications = adw::PreferencesGroup::builder()
            .title(t(Key::Notifications))
            .build();
        let notify = adw::SwitchRow::builder()
            .title(t(Key::NotifyTrackChange))
            .subtitle(t(Key::NotifyTrackChangeSub))
            .active(self.settings.notify_track_change)
            .build();
        {
            let sender = sender.clone();
            notify.connect_active_notify(move |row| {
                sender.input(AppMsg::SetNotifyTrackChange(row.is_active()));
            });
        }
        notifications.add(&notify);

        page.add(&language);
        page.add(&source);
        page.add(&appearance);
        page.add(&notifications);
        dialog.add(&page);
        dialog.present(Some(parent));
    }

    /// First-run language gate. Bilingual on purpose: nothing has been chosen
    /// yet, so the screen has to make sense in both languages at once.
    pub(super) fn present_language_picker(
        &self,
        sender: &ComponentSender<Self>,
        parent: &adw::ApplicationWindow,
    ) -> adw::Dialog {
        let page = adw::StatusPage::builder()
            .icon_name(crate::APP_ID)
            .title("Vinilo")
            .description("Choose your language / Elige tu idioma")
            .build();

        let english = gtk::Button::builder()
            .label("English")
            .halign(gtk::Align::Fill)
            .width_request(220)
            .css_classes(["pill"])
            .build();
        {
            let sender = sender.clone();
            english.connect_clicked(move |_| {
                sender.input(AppMsg::ChooseLanguage(Language::English));
            });
        }

        let spanish = gtk::Button::builder()
            .label("Español")
            .halign(gtk::Align::Fill)
            .width_request(220)
            .css_classes(["suggested-action", "pill"])
            .build();
        {
            let sender = sender.clone();
            spanish.connect_clicked(move |_| {
                sender.input(AppMsg::ChooseLanguage(Language::Spanish));
            });
        }

        let note = gtk::Label::builder()
            .label(
                "You can change this later in Preferences.\n\
                 Puedes cambiarlo más tarde en Preferencias.",
            )
            .justify(gtk::Justification::Center)
            .wrap(true)
            .css_classes(["caption", "dim-label"])
            .build();

        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Center)
            .spacing(10)
            .build();
        column.append(&english);
        column.append(&spanish);
        column.append(&note);
        page.set_child(Some(&column));

        let view = adw::ToolbarView::builder().content(&page).build();
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .css_classes(["flat"])
            .build();
        header.set_title_widget(Some(&gtk::Label::new(None)));
        view.add_top_bar(&header);

        let dialog = adw::Dialog::builder()
            .child(&view)
            .content_width(480)
            .can_close(false)
            .build();
        dialog.present(Some(parent));
        dialog
    }

    /// First-run music source. Every catalogue is a real choice.
    pub(super) fn present_provider_picker(
        &self,
        sender: &ComponentSender<Self>,
        parent: &adw::ApplicationWindow,
    ) -> adw::Dialog {
        use vinilo_core::provider::Provider;

        let page = adw::StatusPage::builder()
            .icon_name(crate::APP_ID)
            .title(t(Key::ProviderTitle))
            .description(t(Key::ProviderBody))
            .build();

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .halign(gtk::Align::Center)
            .width_request(380)
            .build();

        let apple = adw::ActionRow::builder()
            .title(t(Key::ProviderApple))
            .subtitle(t(Key::ProviderAppleSub))
            .activatable(true)
            .build();
        apple.add_prefix(&gtk::Image::from_icon_name("folder-music-symbolic"));
        apple.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        {
            let sender = sender.clone();
            apple.connect_activated(move |_| {
                sender.input(AppMsg::ChooseProvider(Provider::AppleMusic));
            });
        }
        list.append(&apple);

        let local = adw::ActionRow::builder()
            .title(t(Key::ProviderLocal))
            .subtitle(t(Key::ProviderLocalSub))
            .activatable(true)
            .build();
        local.add_prefix(&gtk::Image::from_icon_name("drive-harddisk-symbolic"));
        local.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        {
            let sender = sender.clone();
            local.connect_activated(move |_| {
                sender.input(AppMsg::ChooseProvider(Provider::Local));
            });
        }
        list.append(&local);

        let spotify = adw::ActionRow::builder()
            .title(t(Key::ProviderSpotify))
            .subtitle(t(Key::ProviderSpotifySub))
            .activatable(true)
            .build();
        spotify.add_prefix(&gtk::Image::from_icon_name("audio-headphones-symbolic"));
        spotify.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        {
            let sender = sender.clone();
            spotify.connect_activated(move |_| {
                sender.input(AppMsg::ChooseProvider(Provider::Spotify));
            });
        }
        list.append(&spotify);

        let youtube = adw::ActionRow::builder()
            .title(t(Key::ProviderYoutube))
            .subtitle(t(Key::ProviderYoutubeSub))
            .activatable(true)
            .build();
        youtube.add_prefix(&gtk::Image::from_icon_name("video-display-symbolic"));
        youtube.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        {
            let sender = sender.clone();
            youtube.connect_activated(move |_| {
                sender.input(AppMsg::ChooseProvider(Provider::YoutubeMusic));
            });
        }
        list.append(&youtube);

        let tidal = adw::ActionRow::builder()
            .title(t(Key::ProviderTidal))
            .subtitle(t(Key::ProviderTidalSub))
            .activatable(true)
            .build();
        tidal.add_prefix(&gtk::Image::from_icon_name("audio-x-generic-symbolic"));
        tidal.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        {
            let sender = sender.clone();
            tidal.connect_activated(move |_| {
                sender.input(AppMsg::ChooseProvider(Provider::Tidal));
            });
        }
        list.append(&tidal);

        let note = gtk::Label::builder()
            .label(t(Key::ProviderNote))
            .justify(gtk::Justification::Center)
            .wrap(true)
            .max_width_chars(48)
            .css_classes(["caption", "dim-label"])
            .build();

        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Center)
            .spacing(18)
            .build();
        column.append(&list);
        column.append(&note);
        page.set_child(Some(&column));

        let view = adw::ToolbarView::builder().content(&page).build();
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .css_classes(["flat"])
            .build();
        header.set_title_widget(Some(&gtk::Label::new(None)));
        view.add_top_bar(&header);

        let dialog = adw::Dialog::builder()
            .child(&view)
            .content_width(520)
            .can_close(false)
            .build();
        dialog.present(Some(parent));
        dialog
    }

    pub(super) fn fill_primary_menu(
        menu: &gtk::gio::Menu,
        provider: vinilo_core::provider::Provider,
    ) {
        menu.remove_all();
        let section = gtk::gio::Menu::new();
        section.append(Some(t(Key::Preferences)), Some("win.preferences"));
        section.append(Some(t(Key::KeyboardShortcuts)), Some("win.shortcuts"));
        section.append(Some(t(Key::About)), Some("win.about"));
        menu.append_section(None, &section);

        if provider.needs_apple()
            || (provider.is_catalog() && vinilo_core::setup::is_configured(provider))
        {
            let account = gtk::gio::Menu::new();
            account.append(Some(t(Key::SignOut)), Some("win.sign-out"));
            menu.append_section(None, &account);
        }

        let quit = gtk::gio::Menu::new();
        quit.append(Some(t(Key::Quit)), Some("app.quit"));
        menu.append_section(None, &quit);
    }
}
