// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Listening stats page: totals, cover grid, and top tracks.

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::gtk::gdk::Texture;
use vinilo_core::i18n::{self, Key};
use vinilo_core::listen_stats::{self, TrackStat};

/// Fill `root` with the current listening summary.
pub fn refresh(root: &gtk::Box) {
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }
    root.set_orientation(gtk::Orientation::Vertical);
    root.set_spacing(22);
    root.set_margin_top(28);
    root.set_margin_bottom(32);
    root.set_margin_start(28);
    root.set_margin_end(28);

    let total = listen_stats::total_ms();
    let top = listen_stats::top_tracks(24);
    if total == 0 && top.is_empty() {
        let page = adw::StatusPage::builder()
            .icon_name("preferences-system-time-symbolic")
            .title(i18n::t(Key::StatsEmpty))
            .description(i18n::t(Key::StatsEmptyBody))
            .build();
        root.append(&page);
        return;
    }

    let summary = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .halign(gtk::Align::Start)
        .build();
    summary.append(
        &gtk::Label::builder()
            .label(i18n::t(Key::StatsTotal))
            .css_classes(["heading"])
            .halign(gtk::Align::Start)
            .build(),
    );
    summary.append(
        &gtk::Label::builder()
            .label(listen_stats::format_listen_ms(total))
            .css_classes(["title-1"])
            .halign(gtk::Align::Start)
            .build(),
    );
    let unique = listen_stats::unique_track_count();
    summary.append(
        &gtk::Label::builder()
            .label(i18n::tracks_n(unique))
            .css_classes(["dim-label"])
            .halign(gtk::Align::Start)
            .build(),
    );
    root.append(&summary);

    // Cover strip first — visual weight without a second "Most played" heading.
    let covers = top.iter().take(8).collect::<Vec<_>>();
    if covers.iter().any(|t| load_cover(t).is_some()) {
        let scroller = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .height_request(176)
            .margin_top(4)
            .build();
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(14)
            .build();
        for track in &covers {
            let tile = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(8)
                .width_request(120)
                .css_classes(["tile"])
                .build();
            let frame = gtk::Box::builder()
                .halign(gtk::Align::Center)
                .css_classes(["nodalix-cover-frame", "stats-cover-frame"])
                .build();
            let image = gtk::Image::builder()
                .pixel_size(120)
                .width_request(120)
                .height_request(120)
                .overflow(gtk::Overflow::Hidden)
                .icon_name("audio-x-generic-symbolic")
                .css_classes(["nodalix-cover", "stats-cover"])
                .build();
            if let Some(texture) = load_cover(track) {
                image.set_paintable(Some(&texture));
            }
            frame.append(&image);
            tile.append(&frame);
            tile.append(
                &gtk::Label::builder()
                    .label(&track.title)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .max_width_chars(14)
                    .css_classes(["caption-heading"])
                    .halign(gtk::Align::Start)
                    .build(),
            );
            tile.append(
                &gtk::Label::builder()
                    .label(listen_stats::format_listen_ms(track.total_ms))
                    .css_classes(["caption", "dim-label"])
                    .halign(gtk::Align::Start)
                    .build(),
            );
            row.append(&tile);
        }
        scroller.set_child(Some(&row));
        root.append(&scroller);
    }

    let list_heading = gtk::Label::builder()
        .label(i18n::t(Key::StatsTopTracks))
        .css_classes(["title-2", "shelf-heading"])
        .halign(gtk::Align::Start)
        .margin_top(4)
        .build();
    root.append(&list_heading);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    for track in &top {
        let mut subtitle = track.artist.clone();
        if !track.album.is_empty() {
            if !subtitle.is_empty() {
                subtitle.push_str(" · ");
            }
            subtitle.push_str(&track.album);
        }
        if !track.year.is_empty() {
            subtitle.push_str(" · ");
            subtitle.push_str(&track.year);
        }
        if !track.source.is_empty() {
            subtitle.push_str(" · ");
            subtitle.push_str(&track.source);
        }

        let row = adw::ActionRow::builder()
            .title(&track.title)
            .subtitle(&subtitle)
            .build();

        let frame = gtk::Box::builder()
            .valign(gtk::Align::Center)
            .css_classes(["nodalix-cover-frame", "stats-cover-frame"])
            .build();
        let cover = gtk::Image::builder()
            .pixel_size(48)
            .width_request(48)
            .height_request(48)
            .overflow(gtk::Overflow::Hidden)
            .icon_name("audio-x-generic-symbolic")
            .css_classes(["nodalix-cover", "stats-cover"])
            .build();
        if let Some(texture) = load_cover(track) {
            cover.set_paintable(Some(&texture));
        }
        frame.append(&cover);
        row.add_prefix(&frame);

        let meta = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .valign(gtk::Align::Center)
            .build();
        meta.append(
            &gtk::Label::builder()
                .label(listen_stats::format_listen_ms(track.total_ms))
                .css_classes(["caption-heading"])
                .halign(gtk::Align::End)
                .build(),
        );
        meta.append(
            &gtk::Label::builder()
                .label(i18n::plays_n(track.play_count))
                .css_classes(["caption", "dim-label"])
                .halign(gtk::Align::End)
                .build(),
        );
        if let Some(views) = track.public_plays {
            let compact = listen_stats::format_count(views);
            let label = i18n::public_count_label(&compact, track.id.starts_with("yt:"));
            meta.append(
                &gtk::Label::builder()
                    .label(label)
                    .css_classes(["caption", "dim-label"])
                    .halign(gtk::Align::End)
                    .build(),
            );
        }
        row.add_suffix(&meta);
        list.append(&row);
    }
    root.append(&list);
}

fn load_cover(track: &TrackStat) -> Option<Texture> {
    let url = track.artwork.as_deref()?;
    let path = url.strip_prefix("file://").unwrap_or(url);
    if !path.starts_with('/') {
        return None;
    }
    // `from_filename` scales through the image's pixel size. `for_pixbuf`
    // has been deprecated since GTK 4.20, and that is an error under clippy.
    Texture::from_filename(path).ok()
}
