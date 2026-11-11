// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! What Vinilo adds on top of the Nodalia Design Language.
//!
//! NDL (the `nodalia-gtk` crate) draws every widget. This is only what is
//! Vinilo's own — record sleeves, the tiles lifting, the player's controls —
//! and it reads NDL's `--ndl-*` variables rather than carrying numbers, so a
//! change of density or theme reaches it too.
//!
//! Icons come from NDL's own theme, which `nodalia-gtk` installs: the names
//! used here are the standard ones, and that theme answers them.

/// CSS the rest of the suite can paste. Variables first, then the chrome that
/// reads them — one file, one look.
pub(crate) const CHROME: &str = r#"
:root {
    --nodalix-radius-cover: 18%;
    --nodalix-radius-row: var(--ndl-radius-lg);
    --nodalix-shadow: var(--ndl-elevation-overlay);
    --nodalix-shadow-soft: var(--ndl-elevation-raised);
    --nodalix-shadow-hover: var(--ndl-elevation-overlay), var(--ndl-elevation-raised);
}

.nodalix .navigation-sidebar > row {
    min-height: calc(var(--ndl-density-row-height) + var(--ndl-space-sm));
}

/* The whole tile lifts. Scaling the cover alone clips inside its radius. */
.tile {
    transition: transform var(--ndl-duration-slow) var(--ndl-easing-standard);
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
    transition: box-shadow var(--ndl-duration-slow) var(--ndl-easing-standard);
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
    padding: var(--ndl-space-md) var(--ndl-space-lg);
}
/* Play is the one solid shape in the player, in the text colour: the accent
   is kept for what moves (the progress line) and for focus. */
.nodalix .np-row button.suggested-action {
    min-width: 40px;
    min-height: 40px;
}
.nodalix .np-row button.suggested-action,
.nodalix button.suggested-action.circular {
    background-color: var(--ndl-color-text-primary);
    color: var(--ndl-color-surface-window);
}
.nodalix .np-row button.suggested-action:hover,
.nodalix button.suggested-action.circular:hover {
    background-color: var(--ndl-color-text-secondary);
}
.nodalix button.page-play {
    min-width: 128px;
    min-height: 40px;
}

/* The weight ladder: a view's name and the playing title heaviest, then
   names in a list, then everything that describes them. */
.nodalix .title-1,
.nodalix .title-2 {
    letter-spacing: -0.02em;
    font-weight: 800;
}
.nodalix .np-row .heading {
    font-weight: 700;
}
.nodalix .track-title {
    font-weight: 600;
}
.nodalix .tile-grid {
    padding: var(--ndl-space-lg);
}

.nodalix .np-sheet {
    padding: var(--ndl-space-xs) 0 0;
}

/* Shelf headings on Listen Now — more presence than a plain sidebar heading. */
.nodalix .shelf-heading {
    font-family: var(--ndl-font-family-display);
    font-weight: 800;
    letter-spacing: -0.02em;
    margin-bottom: var(--ndl-space-xxs);
}

.scale-popup {
    padding: var(--ndl-space-sm) var(--ndl-space-xs);
}
.scale-popup scale.vertical {
    min-height: 140px;
}
.volume-osd levelbar > trough {
    min-height: 12px;
    border-radius: var(--ndl-radius-full);
    background-color: var(--ndl-color-control-track);
}
.volume-osd levelbar > trough > block.filled {
    border-radius: var(--ndl-radius-full);
    background-color: var(--ndl-color-accent-default);
}
"#;

/// An icon by its standard name; NDL's icon theme draws it.
pub fn icon(name: &'static str) -> &'static str {
    name
}

/// A level popup as NDL draws it: just the slider. GTK's step buttons above
/// and below it are the part that read as another toolkit.
pub fn slider_only(button: &relm4::gtk::ScaleButton) {
    use relm4::gtk::prelude::*;
    button.plus_button().set_visible(false);
    button.minus_button().set_visible(false);
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

/// The sidebar toggle. The button's pressed state already says whether the
/// sidebar is open, so both states share one glyph.
pub fn sidebar_toggle_icon(_shown: bool) -> &'static str {
    "sidebar-show-symbolic"
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// Every `*-symbolic` name in the source, read from the files themselves.
    fn names_in(dir: &Path, found: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                names_in(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for piece in text.split('"').skip(1).step_by(2) {
                    let is_name = piece.ends_with("-symbolic")
                        && piece.len() > "-symbolic".len()
                        && piece
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
                    if is_name {
                        found.push((piece.to_owned(), path.display().to_string()));
                    }
                }
            }
        }
    }

    /// No icon of another set may reach the screen: each name Vinilo asks for
    /// must be one NDL's theme draws.
    #[test]
    fn every_icon_vinilo_uses_is_drawn_by_ndl() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut found = Vec::new();
        for crate_dir in ["vinilo/src", "vinilo-core/src", "vinilod/src"] {
            names_in(&root.join(crate_dir), &mut found);
        }
        assert!(found.len() > 30, "found only {}", found.len());
        let missing: Vec<_> = found
            .iter()
            .filter(|(name, _)| !nodalia_gtk::has_icon(name))
            .collect();
        assert!(missing.is_empty(), "not in the NDL icon theme: {missing:?}");
    }
}
