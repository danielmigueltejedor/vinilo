// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vinilo chrome and the visual tokens shared with the Nodalix suite.
//!
//! Vinilo is the first window that wears this. Archivador and the rest of the
//! family should take the same icon pick and the `--nodalix-*` variables — not
//! Adwaita's defaults — so a Nodalix app is recognisable without looking like a
//! restyled GNOME template.
//!
//! Native GNOME/Adwaita icons are used regardless of the system icon theme.
//! theme stays, and named icons still resolve through Adwaita / hicolor.


use relm4::gtk;

/// CSS the rest of the suite can paste. Variables first, then the chrome that
/// reads them — one file, one look.
pub(crate) const CHROME: &str = r#"
:root {
    --nodalix-radius: 16px;
    --nodalix-radius-cover: 22%;
    --nodalix-radius-row: 14px;
    --nodalix-pad: 12px;
}

.nodalix .navigation-sidebar {
    padding: 6px 8px 12px;
}
.nodalix .navigation-sidebar > row {
    border-radius: var(--nodalix-radius-row);
    margin: 2px 4px;
    min-height: 38px;
}

.nodalix-cover,
.np-cover {
    border-radius: var(--nodalix-radius-cover);
}

.nodalix .np-bar {
    border-radius: 0;
}
.nodalix .np-row {
    padding: 12px 14px;
}
.nodalix .np-row button.suggested-action {
    min-width: 36px;
    min-height: 36px;
}

.nodalix .heading {
    letter-spacing: -0.02em;
}
.nodalix .tile-grid {
    padding: 16px;
}

/* Not under .nodalix: GtkScaleButton's popover is its own window. */
scale.horizontal > trough,
scale.vertical > trough {
    border-radius: 9999px;
    background-color: alpha(@window_fg_color, 0.16);
}
scale.horizontal > trough {
    min-height: 12px;
}
scale.vertical > trough {
    min-width: 12px;
}
scale.horizontal > trough > highlight,
scale.vertical > trough > highlight {
    border-radius: 9999px;
    background-color: @accent_bg_color;
}
scale.horizontal > trough > highlight {
    min-height: 12px;
}
scale.vertical > trough > highlight {
    min-width: 12px;
}
scale > trough > slider {
    min-width: 22px;
    min-height: 22px;
    margin: -5px;
    border-radius: 9999px;
    background-color: @view_bg_color;
    box-shadow: 0 1px 3px alpha(#000000, 0.28);
}
.scale-popup {
    padding: 8px 4px;
}
.scale-popup scale.vertical {
    min-height: 140px;
}
.volume-osd levelbar > trough {
    min-height: 12px;
    border-radius: 9999px;
    background-color: alpha(@window_fg_color, 0.16);
}
.volume-osd levelbar > trough > block.filled {
    border-radius: 9999px;
    background-color: @accent_bg_color;
}
"#;

/// Use Adwaita icons inside Vinilo regardless of the desktop icon theme.
pub fn init() {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_icon_theme_name(Some("Adwaita"));
    }
}


/// Native GNOME/Adwaita icon handling.
pub fn icon(name: &'static str) -> &'static str {
name
}


/// Mute, high, low, medium — the order `GtkScaleButton` documents.
pub fn volume_icons() -> [&'static str; 4] {
[
"audio-volume-muted-symbolic",
"audio-volume-high-symbolic",
"audio-volume-low-symbolic",
"audio-volume-medium-symbolic",
]
}


/// Native GNOME sidebar toggle.
///
/// The toggle state already communicates whether the sidebar is open,
/// so both states use the standard libadwaita sidebar glyph.
pub fn sidebar_toggle_icon(_shown: bool) -> &'static str {
    "adw-sidebar-symbolic"
}
