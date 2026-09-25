// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Listening stats page: totals and top tracks from the local cache.

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::gtk::gdk::Texture;
use relm4::gtk::gdk_pixbuf::Pixbuf;
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
    for track in &top {
        let mut subtitle = track.artist.clone();
        if !track.album.is_empty() {
            subtitle.push_str(" · ");
            subtitle.push_str(&track.album);
        }
        if !track.year.is_empty() {
            subtitle.push_str(" · ");
            subtitle.push_str(&track.year);
        }
        subtitle.push_str(&format!(" · {} plays", track.play_count));
        if !track.source.is_empty() {
            subtitle.push_str(" · ");
            subtitle.push_str(&track.source);
        }
        if let Some(views) = track.public_plays {
            subtitle.push_str(" · ");
            subtitle.push_str(&listen_stats::format_count(views));
            if track.id.starts_with("yt:") {
                subtitle.push_str(" views");
            }
        }

        let row = adw::ActionRow::builder()
            .title(&track.title)
            .subtitle(&subtitle)
            .build();

        let cover = gtk::Image::builder()
            .pixel_size(56)
            .icon_name("audio-x-generic-symbolic")
            .css_classes(["stats-cover"])
            .build();
        if let Some(url) = track.artwork.as_deref() {
            // Local file:// or already-cached path; remote URLs stay as placeholder.
            let path = url.strip_prefix("file://").unwrap_or(url);
            if path.starts_with('/') {
                if let Ok(pixbuf) = Pixbuf::from_file_at_scale(path, 56, 56, true) {
                    cover.set_paintable(Some(&Texture::for_pixbuf(&pixbuf)));
                }
            }
        }
        row.add_prefix(&cover);
        list.append(&row);
    }
    root.append(&list);
}
