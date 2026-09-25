// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Listening stats page: totals and top tracks from the local cache.

use gtk::prelude::*;
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use vinilo_core::i18n::{self, Key};
use vinilo_core::listen_stats;

/// Fill `root` with the current listening summary.
pub fn refresh(root: &gtk::Box) {
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }
    root.set_orientation(gtk::Orientation::Vertical);
    root.set_spacing(12);
    root.set_margin_top(24);
    root.set_margin_bottom(24);
    root.set_margin_start(24);
    root.set_margin_end(24);

    let total = listen_stats::total_ms();
    let top = listen_stats::top_tracks(20);
    if total == 0 && top.is_empty() {
        let page = adw::StatusPage::builder()
            .icon_name("preferences-system-time-symbolic")
            .title(i18n::t(Key::StatsEmpty))
            .description(i18n::t(Key::StatsEmptyBody))
            .build();
        root.append(&page);
        return;
    }

    let hours = total / 3_600_000;
    let mins = (total % 3_600_000) / 60_000;
    let total_label = gtk::Label::builder()
        .label(format!("{}: {hours}h {mins}m", i18n::t(Key::StatsTotal)))
        .css_classes(["title-1"])
        .halign(gtk::Align::Start)
        .build();
    root.append(&total_label);

    let heading = gtk::Label::builder()
        .label(i18n::t(Key::StatsTopTracks))
        .css_classes(["title-2"])
        .halign(gtk::Align::Start)
        .margin_top(12)
        .build();
    root.append(&heading);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    for (i, track) in top.iter().enumerate() {
        let row = adw::ActionRow::builder()
            .title(&track.title)
            .subtitle(format!("{} · {} plays", track.artist, track.play_count))
            .build();
        row.add_prefix(&gtk::Label::new(Some(&format!("{}.", i + 1))));
        list.append(&row);
    }
    root.append(&list);
}
