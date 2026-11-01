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
    --nodalix-radius: 18px;
    --nodalix-radius-cover: 18%;
    --nodalix-radius-row: 14px;
    --nodalix-pad: 14px;
    --nodalix-shadow: 0 10px 28px alpha(#000000, 0.28), 0 2px 6px alpha(#000000, 0.18);
    --nodalix-shadow-soft: 0 4px 14px alpha(#000000, 0.18), 0 1px 3px alpha(#000000, 0.12);
    --nodalix-shadow-hover: 0 16px 36px alpha(#000000, 0.34), 0 4px 10px alpha(#000000, 0.20);
}

.nodalix .navigation-sidebar {
    padding: 8px 10px 14px;
}
.nodalix .navigation-sidebar > row {
    border-radius: var(--nodalix-radius-row);
    margin: 2px 4px;
    min-height: 40px;
    transition: background-color 160ms ease-out;
}
.nodalix .navigation-sidebar > row:hover {
    background-color: alpha(@window_fg_color, 0.06);
}
.nodalix .navigation-sidebar > row:selected {
    background-color: alpha(@accent_bg_color, 0.16);
    color: @window_fg_color;
}
.nodalix .navigation-sidebar > row:selected:hover {
    background-color: alpha(@accent_bg_color, 0.22);
}

/* The whole tile lifts. Scaling the cover alone clips inside its radius. */
.tile {
    transition: transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}
.tile:hover {
    transform: translateY(-4px);
}
.tile:hover .nodalix-cover-frame {
    box-shadow: var(--nodalix-shadow-hover);
}

/* Shadow lives on the frame, not the clipped image — overflow:hidden on the
   picture would eat a box-shadow painted on the same widget. */
.nodalix-cover-frame {
    border-radius: var(--nodalix-radius-cover);
    box-shadow: var(--nodalix-shadow);
    transition: box-shadow 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}
.np-bar .nodalix-cover-frame,
.stats-cover-frame {
    box-shadow: var(--nodalix-shadow-soft);
}

.nodalix-cover,
.np-cover {
    border-radius: var(--nodalix-radius-cover);
}

.nodalix .np-bar {
    border-radius: 0;
}
.nodalix .np-row {
    padding: 12px 16px;
}
.nodalix .np-row button.suggested-action {
    min-width: 40px;
    min-height: 40px;
    box-shadow: 0 4px 14px alpha(@accent_bg_color, 0.38);
}
.nodalix .np-sheet button.suggested-action.circular {
    box-shadow: 0 8px 24px alpha(@accent_bg_color, 0.42);
}
.nodalix button.page-play {
    min-width: 128px;
    min-height: 40px;
    box-shadow: 0 6px 18px alpha(@accent_bg_color, 0.36);
}

.nodalix .heading {
    letter-spacing: -0.02em;
}
.nodalix .title-1,
.nodalix .title-2 {
    letter-spacing: -0.03em;
    font-weight: 700;
}
.nodalix .tile-grid {
    padding: 18px;
}

.nodalix .np-sheet {
    padding: 4px 0 0;
}

/* Shelf headings on Listen Now — more presence than a plain sidebar heading. */
.nodalix .shelf-heading {
    letter-spacing: -0.025em;
    font-weight: 700;
    opacity: 0.92;
    margin-bottom: 2px;
}

/* Not under .nodalix: GtkScaleButton's popover is its own window. */
scale.horizontal > trough,
scale.vertical > trough {
    border-radius: 9999px;
    background-color: alpha(@window_fg_color, 0.14);
}
scale.horizontal > trough {
    min-height: 6px;
}
scale.vertical > trough {
    min-width: 6px;
}
scale.horizontal > trough > highlight,
scale.vertical > trough > highlight {
    border-radius: 9999px;
    background-color: @accent_bg_color;
}
scale.horizontal > trough > highlight {
    min-height: 6px;
}
scale.vertical > trough > highlight {
    min-width: 6px;
}
scale > trough > slider {
    min-width: 18px;
    min-height: 18px;
    margin: -6px;
    border-radius: 9999px;
    background-color: @view_bg_color;
    box-shadow: 0 1px 4px alpha(#000000, 0.32);
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
