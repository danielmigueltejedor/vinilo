// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Listen Now: recently played, recommendations, charts.
//!
//! A scrolled column of horizontal shelves rather than a grid or a list,
//! because those are three different questions (what you played, what Apple
//! made for you, what is popular) and stacking them as one TypedGridView would
//! flatten the headings. Tiles reuse the same cover pipeline as the album grid.

use std::rc::Rc;

use relm4::gtk::prelude::*;
use relm4::{adw, gtk};

use crate::components::cover::Cover;
use crate::components::grid_item::{ArtRegistry, ArtRequest, TILE_PX, register};
use vinilo_core::discover::Discover;
use vinilo_core::entry::Entry;
use vinilo_core::i18n::{self, Key};
use vinilo_core::music::types::{Artwork, Track};

/// What a click on a Discover tile is asking for.
#[derive(Debug)]
pub enum DiscoverAction {
    Open(Entry),
    PlaySongs { songs: Vec<Track>, index: usize },
}

pub type DiscoverHandler = Rc<dyn Fn(DiscoverAction)>;

pub struct DiscoverView {
    /// The stack the app wraps in a `ScrolledWindow`. Built here rather than
    /// in `view!` because the shelves are rebuilt in place; `#[local_ref]`
    /// after `add_named =` is not valid relm4 syntax (the parser wants an
    /// identifier, which is how the grids are attached).
    pub stack: gtk::Stack,
    body: gtk::Box,
    empty: adw::StatusPage,
    pub(crate) registry: ArtRegistry,
    request: ArtRequest,
    on_activate: DiscoverHandler,
    data: Discover,
}

impl DiscoverView {
    pub fn new(registry: ArtRegistry, request: ArtRequest, on_activate: DiscoverHandler) -> Self {
        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .margin_top(18)
            .margin_bottom(24)
            .margin_start(18)
            .margin_end(18)
            .build();

        let content = adw::Clamp::builder()
            .maximum_size(1100)
            .child(&body)
            .build();

        let empty = adw::StatusPage::builder()
            .icon_name("folder-music-symbolic")
            .title(i18n::t(Key::DiscoverEmpty))
            .description(i18n::t(Key::DiscoverEmptyBody))
            .build();

        let stack = gtk::Stack::new();
        stack.add_named(&content, Some("content"));
        stack.add_named(&empty, Some("empty"));
        stack.set_visible_child_name("empty");

        Self {
            stack,
            body,
            empty,
            registry,
            request,
            on_activate,
            data: Discover::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn snapshot(&self) -> Discover {
        self.data.clone()
    }

    pub fn relocalize(&self) {
        self.empty.set_title(i18n::t(Key::DiscoverEmpty));
        self.empty
            .set_description(Some(i18n::t(Key::DiscoverEmptyBody)));
        self.fill_body();
    }

    pub fn fill(&mut self, data: Discover) {
        self.data = data;
        self.fill_body();
        self.stack.set_visible_child_name(if self.data.is_empty() {
            "empty"
        } else {
            "content"
        });
    }

    fn fill_body(&self) {
        self.registry.borrow_mut().clear();
        while let Some(child) = self.body.first_child() {
            self.body.remove(&child);
        }

        self.add_mixed_shelf(i18n::t(Key::RecentlyPlayed), &self.data.recently_played);
        self.add_mixed_shelf(i18n::t(Key::MadeForYou), &self.data.recommended_playlists);
        self.add_song_shelf(i18n::t(Key::RecommendedSongs), &self.data.recommended_songs);
        self.add_mixed_shelf(i18n::t(Key::RecentlyAdded), &self.data.recently_added);
        self.add_mixed_shelf(i18n::t(Key::Charts), &self.data.charts);
    }

    fn add_mixed_shelf(&self, title: &str, entries: &[Entry]) {
        if entries.is_empty() {
            return;
        }
        let row = self.shelf_header(title);
        for entry in entries {
            row.append(&self.tile(entry.clone(), None));
        }
        self.body.append(&row.parent_scroller());
    }

    fn add_song_shelf(&self, title: &str, songs: &[Track]) {
        if songs.is_empty() {
            return;
        }
        let row = self.shelf_header(title);
        for (index, song) in songs.iter().enumerate() {
            row.append(&self.tile(Entry::Song(song.clone()), Some((songs.to_vec(), index))));
        }
        self.body.append(&row.parent_scroller());
    }

    fn shelf_header(&self, title: &str) -> ShelfRow {
        let heading = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["heading"])
            .build();

        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .overlay_scrolling(true)
            .child(&row)
            .build();
        scroller.set_propagate_natural_height(true);
        // Nested inside the page scroller, a horizontal ScrolledWindow with
        // no min height can collapse to 0 and hide an otherwise full shelf.
        scroller.set_min_content_height(TILE_PX + 56);

        let wrap = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        wrap.append(&heading);
        wrap.append(&scroller);
        // Stash the wrap on the row via a helper that returns wrap for append.
        ShelfRow { row, wrap }
    }

    fn tile(&self, entry: Entry, play: Option<(Vec<Track>, usize)>) -> gtk::Button {
        let cover = Cover::new(TILE_PX);
        match &entry {
            Entry::Artist(artist) => cover.round(&artist.name),
            Entry::Song(_) => cover.square("audio-x-generic-symbolic"),
            Entry::Album(_) => cover.square("media-optical-symbolic"),
            Entry::Playlist(_) => cover.square("view-list-symbolic"),
        }
        if let Some(art) = artwork_of(&entry) {
            let key = art.cache_key();
            (self.request)(key.clone(), art.clone());
            register(&mut self.registry.borrow_mut(), key, &cover);
        }

        let title = gtk::Label::builder()
            .label(entry.title())
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(1)
            .xalign(0.5)
            .css_classes(["heading"])
            .build();
        title.set_width_request(TILE_PX);
        let subtitle = entry.subtitle();
        let caption = gtk::Label::builder()
            .label(&subtitle)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(1)
            .xalign(0.5)
            .visible(!subtitle.is_empty())
            .css_classes(["caption", "dim-label"])
            .build();
        caption.set_width_request(TILE_PX);

        let inner = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .width_request(TILE_PX)
            .build();
        inner.append(cover.widget());
        inner.append(&title);
        inner.append(&caption);

        let button = gtk::Button::builder()
            .css_classes(["flat"])
            .child(&inner)
            .build();
        let activate = self.on_activate.clone();
        button.connect_clicked(move |_| match &play {
            Some((songs, index)) => activate(DiscoverAction::PlaySongs {
                songs: songs.clone(),
                index: *index,
            }),
            None => activate(DiscoverAction::Open(entry.clone())),
        });
        button
    }
}

struct ShelfRow {
    row: gtk::Box,
    wrap: gtk::Box,
}

impl ShelfRow {
    fn append(&self, child: &impl IsA<gtk::Widget>) {
        self.row.append(child);
    }

    fn parent_scroller(&self) -> gtk::Box {
        self.wrap.clone()
    }
}

fn artwork_of(entry: &Entry) -> Option<&Artwork> {
    match entry {
        Entry::Song(t) => t.artwork.as_ref(),
        Entry::Album(a) => a.artwork.as_ref(),
        Entry::Artist(a) => a.artwork.as_ref(),
        Entry::Playlist(p) => p.artwork.as_ref(),
    }
}
