// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Listening stats page: totals, cover grid, and top tracks.

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::gtk::gdk::Texture;
use relm4::gtk::gdk_pixbuf::Pixbuf;
use vinilo_core::i18n::{self, Key};
use vinilo_core::listen_stats::{self, TrackStat};

/// Fill `root` with the current listening summary.
pub fn refresh(root: &gtk::Box) {
    while let Some(child) = root.first_child() {
        root.remove(&child);
    }
    root.set_orientation(gtk::Orientation::Vertical);
    root.set_spacing(18);
    root.set_margin_top(24);
    root.set_margin_bottom(24);
    root.set_margin_start(24);
    root.set_margin_end(24);

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
        .spacing(4)
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
            .label(format!("{} · {}", i18n::t(Key::StatsTopTracks), unique))
            .css_classes(["dim-label"])
            .halign(gtk::Align::Start)
            .build(),
    );
    root.append(&summary);

    // Cover strip for the top handful — the visual weight issue #5 asked for.
    let covers = top.iter().take(8).collect::<Vec<_>>();
    if covers.iter().any(|t| load_cover(t, 120).is_some()) {
        let heading = gtk::Label::builder()
            .label(i18n::t(Key::StatsTopTracks))
            .css_classes(["title-2"])
            .halign(gtk::Align::Start)
            .margin_top(8)
            .build();
        root.append(&heading);

        let scroller = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .height_request(168)
            .build();
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();
        for track in &covers {
            let tile = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .width_request(120)
                .build();
            let image = gtk::Image::builder()
                .pixel_size(120)
                .icon_name("audio-x-generic-symbolic")
                .css_classes(["stats-cover"])
                .build();
            if let Some(texture) = load_cover(track, 120) {
                image.set_paintable(Some(&texture));
            }
            tile.append(&image);
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
        .css_classes(["title-2"])
        .halign(gtk::Align::Start)
        .margin_top(8)
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

        let cover = gtk::Image::builder()
            .pixel_size(64)
            .icon_name("audio-x-generic-symbolic")
            .css_classes(["stats-cover"])
            .build();
        if let Some(texture) = load_cover(track, 64) {
            cover.set_paintable(Some(&texture));
        }
        row.add_prefix(&cover);

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
                .label(format!("{} plays", track.play_count))
                .css_classes(["caption", "dim-label"])
                .halign(gtk::Align::End)
                .build(),
        );
        if let Some(views) = track.public_plays {
            let mut label = listen_stats::format_count(views);
            if track.id.starts_with("yt:") {
                label.push_str(" views");
            }
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

fn load_cover(track: &TrackStat, size: i32) -> Option<Texture> {
    let url = track.artwork.as_deref()?;
    let path = url.strip_prefix("file://").unwrap_or(url);
    if !path.starts_with('/') {
        return None;
    }
    let pixbuf = Pixbuf::from_file_at_scale(path, size, size, true).ok()?;
    Some(Texture::for_pixbuf(&pixbuf))
}
