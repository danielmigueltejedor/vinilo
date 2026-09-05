// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! The row context menu: what it offers, and what each item sends.
//!
//! Built imperatively rather than in `view!` because it is per-row and
//! transient — a `ListView` recycles the widget underneath it while it is open,
//! so the popover is parented, shown, and unparented per click rather than
//! living in the tree.
//!
//! Each item appears only when it would do something. Offering "Add to Library"
//! for a track already saved, or "Favourite" for one already starred, is a menu
//! that lies about the state of things.

use relm4::gtk;
use relm4::gtk::prelude::*;

use super::{AppModel, AppMsg, LibraryAction};
use crate::components::track_row::RowMenuRequest;
use vinilo_core::i18n::{self, Key};

impl AppModel {
    /// Playlists the user can add a song to. Liked-songs shortcuts and
    /// editorial mixes are left out: those are not writable lists.
    pub(super) fn writable_playlists(&self) -> Vec<(String, String)> {
        self.playlists
            .iter()
            .filter(|list| {
                list.library
                    && list.id != "sp:liked"
                    && list.id != "sp:top"
                    && list.id != "yt:liked"
                    && list.id != "td:liked"
                    && !list.id.contains(":collection:")
            })
            .map(|list| (list.id.clone(), list.name.clone()))
            .collect()
    }

    /// Show a row's context menu where it was clicked.
    ///
    /// Built fresh each time and parented to the row: a single long-lived
    /// popover would have to be re-parented on every click anyway, and a
    /// `ListView` recycles the widget under it while it is open.
    pub(super) fn show_row_menu(&self, req: RowMenuRequest) {
        let menu = gtk::gio::Menu::new();

        let queue = gtk::gio::Menu::new();
        queue.append(Some(i18n::t(Key::PlayNext)), Some("row.play-next"));
        queue.append(Some(i18n::t(Key::AddToQueue)), Some("row.play-later"));
        menu.append_section(None, &queue);

        let account = gtk::gio::Menu::new();
        if req.in_library {
            account.append(
                Some(i18n::t(Key::RemoveFromLibrary)),
                Some("row.remove-from-library"),
            );
        } else {
            account.append(Some(i18n::t(Key::AddToLibrary)), Some("row.add-to-library"));
        }
        if req.favorite {
            account.append(Some(i18n::t(Key::RemoveFavourite)), Some("row.unfavorite"));
        } else {
            account.append(Some(i18n::t(Key::Favourite)), Some("row.favorite"));
        }
        if account.n_items() > 0 {
            menu.append_section(None, &account);
        }

        let lists = gtk::gio::Menu::new();
        lists.append(Some(i18n::t(Key::NewPlaylist)), Some("row.new-playlist"));
        for (id, name) in self.writable_playlists().into_iter().take(24) {
            lists.append(Some(&name), Some(&format!("row.add-to-playlist::{id}")));
        }
        menu.append_section(None, &{
            let section = gtk::gio::Menu::new();
            section.append_submenu(Some(i18n::t(Key::AddToPlaylist)), &lists);
            section
        });

        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(req.at.0, req.at.1, 1, 1)));
        popover.set_parent(&req.over);

        let actions = gtk::gio::SimpleActionGroup::new();
        for (name, next) in [("play-next", true), ("play-later", false)] {
            let action = gtk::gio::SimpleAction::new(name, None);
            let id = req.catalog_id.clone();
            let sender = self.menu_sender.clone();
            action.connect_activate(move |_, _| {
                sender.input(AppMsg::Enqueue {
                    catalog_id: id.clone(),
                    next,
                });
            });
            actions.add_action(&action);
        }

        {
            let action = gtk::gio::SimpleAction::new("remove-from-library", None);
            let library_id = req
                .library_id
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| req.catalog_id.clone());
            let catalog_id = req.catalog_id.clone();
            let sender = self.menu_sender.clone();
            action.connect_activate(move |_, _| {
                sender.input(AppMsg::RemoveFromLibrary {
                    library_id: library_id.clone(),
                    catalog_id: catalog_id.clone(),
                });
            });
            actions.add_action(&action);

            let action = gtk::gio::SimpleAction::new("unfavorite", None);
            let catalog_id = req.catalog_id.clone();
            let sender = self.menu_sender.clone();
            action.connect_activate(move |_, _| {
                sender.input(AppMsg::Unfavorite {
                    catalog_id: catalog_id.clone(),
                });
            });
            actions.add_action(&action);
        }

        for (name, what) in [
            ("add-to-library", LibraryAction::AddToLibrary),
            ("favorite", LibraryAction::Favorite),
        ] {
            let action = gtk::gio::SimpleAction::new(name, None);
            let id = req.catalog_id.clone();
            let sender = self.menu_sender.clone();
            action.connect_activate(move |_, _| {
                sender.input(AppMsg::LibraryWrite {
                    catalog_id: id.clone(),
                    action: what,
                });
            });
            actions.add_action(&action);
        }

        {
            let action = gtk::gio::SimpleAction::new("new-playlist", None);
            let track_id = req.catalog_id.clone();
            let sender = self.menu_sender.clone();
            action.connect_activate(move |_, _| {
                sender.input(AppMsg::PromptNewPlaylist {
                    track_id: Some(track_id.clone()),
                });
            });
            actions.add_action(&action);

            let action =
                gtk::gio::SimpleAction::new("add-to-playlist", Some(gtk::glib::VariantTy::STRING));
            let track_id = req.catalog_id.clone();
            let sender = self.menu_sender.clone();
            action.connect_activate(move |_, param| {
                let Some(playlist_id) = param.and_then(|v| v.str().map(str::to_owned)) else {
                    return;
                };
                sender.input(AppMsg::AddToPlaylist {
                    playlist_id,
                    track_id: track_id.clone(),
                });
            });
            actions.add_action(&action);
        }
        popover.insert_action_group("row", Some(&actions));

        popover.connect_closed(|p| {
            let p = p.clone();
            gtk::glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    }
}
