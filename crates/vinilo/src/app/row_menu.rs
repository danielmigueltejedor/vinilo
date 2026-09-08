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
use relm4::gtk::prelude::ToVariant;
use relm4::gtk::prelude::*;

use super::{AppModel, AppMsg, LibraryAction};
use crate::components::overridden;
use crate::components::track_row::{Entry, RowMenuRequest};
use vinilo_core::i18n::{self, Key};
use vinilo_core::music::types::{Playlist, Track};

fn is_writable_playlist(list: &Playlist) -> bool {
    let id = list.id.as_str();
    if matches!(id, "sp:liked" | "sp:top" | "yt:liked" | "td:liked") {
        return false;
    }
    if id.contains(":collection:") {
        return false;
    }
    // Spotify's editorial mixes (Discover Weekly, Daily Mix) are not yours.
    if id.contains("sp:playlist:37i9") {
        return false;
    }
    if id.starts_with("sp:playlist:") || id.starts_with("yt:playlist:") {
        return true;
    }
    list.library
}

impl AppModel {
    /// Playlists the user can add a song to. Liked-songs shortcuts and
    /// editorial mixes are left out: those are not writable lists.
    pub(super) fn writable_playlists(&self) -> Vec<(String, String)> {
        self.playlists
            .iter()
            .filter(|list| is_writable_playlist(list))
            .map(|list| (list.id.clone(), list.name.clone()))
            .take(48)
            .collect()
    }

    fn track_for(&self, catalog_id: &str) -> Option<&Track> {
        self.all_tracks
            .iter()
            .find(|track| {
                track.catalog_id.as_deref() == Some(catalog_id) || track.id.0 == catalog_id
            })
            .or_else(|| {
                self.catalog.iter().find_map(|entry| match entry {
                    Entry::Song(track)
                        if track.catalog_id.as_deref() == Some(catalog_id)
                            || track.id.0 == catalog_id =>
                    {
                        Some(track)
                    }
                    _ => None,
                })
            })
            .or_else(|| self.discover.track(catalog_id))
    }

    /// Same menu a library row uses, for whatever is in the player.
    pub(super) fn show_now_playing_menu(&self, at: (i32, i32), over: gtk::Widget) {
        let Some(catalog_id) = self.playing_catalog_id() else {
            return;
        };
        let fetched = self.track_for(&catalog_id);
        let (favorite, in_library) = overridden(
            &self.row_overrides,
            Some(&catalog_id),
            fetched.is_some_and(|track| track.favorite),
            fetched.is_some_and(|track| track.in_library),
        );
        self.show_row_menu(RowMenuRequest {
            catalog_id,
            library_id: fetched.and_then(|track| track.library_id.clone()),
            in_library,
            favorite,
            at,
            over,
        });
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
            // Spotify/YTM ids contain colons (`sp:playlist:…`). GLib parses
            // `action::target` as a GVariant print, so a detailed name never
            // activates. Set the target as a real string variant instead.
            let item = gtk::gio::MenuItem::new(Some(&name), None);
            item.set_action_and_target_value(Some("row.add-to-playlist"), Some(&id.to_variant()));
            lists.append_item(&item);
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

    /// Right-click on a Listen Now cover: songs get the row menu, playlists
    /// can be pinned or added to one of yours.
    pub(super) fn show_discover_menu(&self, entry: Entry, at: (i32, i32), over: gtk::Widget) {
        match entry {
            Entry::Song(track) => {
                let catalog_id = track
                    .catalog_id
                    .clone()
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| track.id.0.clone());
                let (favorite, in_library) = overridden(
                    &self.row_overrides,
                    Some(&catalog_id),
                    track.favorite,
                    track.in_library,
                );
                self.show_row_menu(RowMenuRequest {
                    catalog_id,
                    library_id: track.library_id.clone(),
                    in_library,
                    favorite,
                    at,
                    over,
                });
            }
            Entry::Playlist(list) => self.show_playlist_add_menu(list.id, at, over),
            Entry::Album(_) | Entry::Artist(_) => {}
        }
    }

    fn show_playlist_add_menu(&self, playlist_id: String, at: (i32, i32), over: gtk::Widget) {
        let pinned = self.settings.pinned_playlists.contains(&playlist_id);
        let menu = gtk::gio::Menu::new();
        menu.append(
            Some(if pinned {
                i18n::t(Key::RemoveFromSidebar)
            } else {
                i18n::t(Key::AddToSidebar)
            }),
            Some("tile.toggle-pin"),
        );

        let lists = gtk::gio::Menu::new();
        lists.append(Some(i18n::t(Key::NewPlaylist)), Some("tile.new-playlist"));
        for (id, name) in self.writable_playlists().into_iter().take(48) {
            if id == playlist_id {
                continue;
            }
            let item = gtk::gio::MenuItem::new(Some(&name), None);
            item.set_action_and_target_value(Some("tile.add-to-playlist"), Some(&id.to_variant()));
            lists.append_item(&item);
        }
        menu.append_section(None, &{
            let section = gtk::gio::Menu::new();
            section.append_submenu(Some(i18n::t(Key::AddToPlaylist)), &lists);
            section
        });

        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(at.0, at.1, 1, 1)));
        popover.set_parent(&over);

        let actions = gtk::gio::SimpleActionGroup::new();
        {
            let action = gtk::gio::SimpleAction::new("toggle-pin", None);
            let id = playlist_id.clone();
            let sender = self.menu_sender.clone();
            action.connect_activate(move |_, _| {
                sender.input(AppMsg::SetPinned {
                    id: id.clone(),
                    pinned: !pinned,
                });
            });
            actions.add_action(&action);
        }
        {
            let action = gtk::gio::SimpleAction::new("new-playlist", None);
            let id = playlist_id.clone();
            let sender = self.menu_sender.clone();
            action.connect_activate(move |_, _| {
                sender.input(AppMsg::PromptNewPlaylist {
                    track_id: Some(id.clone()),
                });
            });
            actions.add_action(&action);

            let action =
                gtk::gio::SimpleAction::new("add-to-playlist", Some(gtk::glib::VariantTy::STRING));
            let source = playlist_id;
            let sender = self.menu_sender.clone();
            action.connect_activate(move |_, param| {
                let Some(dest) = param.and_then(|v| v.str().map(str::to_owned)) else {
                    return;
                };
                sender.input(AppMsg::AddToPlaylist {
                    playlist_id: dest,
                    track_id: source.clone(),
                });
            });
            actions.add_action(&action);
        }
        popover.insert_action_group("tile", Some(&actions));
        popover.connect_closed(|p| {
            let p = p.clone();
            gtk::glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    }
}
