// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Nodalix chrome: Colloid icons, and the tokens other apps in the suite copy.
//!
//! Vinilo is the first window that wears this. Archivador and the rest of the
//! family should take the same icon pick and the `--nodalix-*` variables — not
//! Adwaita's defaults — so a Nodalix app is recognisable without looking like a
//! restyled GNOME template.
//!
//! Colloid is **preferred, never required**. If it is not installed the system
//! theme stays, and named icons still resolve through Adwaita / hicolor.

use std::path::{Path, PathBuf};

use relm4::adw;
use relm4::gtk::{self, gdk};

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
    overflow: hidden;
}

.nodalix .np-bar {
    border-top-left-radius: var(--nodalix-radius);
    border-top-right-radius: var(--nodalix-radius);
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
"#;

/// Point GTK at Colloid and keep Dark/Light in step with libadwaita.
pub fn init() {
    add_icon_roots();
    apply_icons();
    adw::StyleManager::default().connect_dark_notify(|_| apply_icons());
}

/// Re-pick Colloid-Dark or Colloid-Light. Safe to call on every theme flip.
pub fn apply_icons() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let theme = gtk::IconTheme::for_display(&display);
    let dark = adw::StyleManager::default().is_dark();
    let installed = installed_colloid();
    match choose_colloid(dark, &installed) {
        Some(name) => {
            theme.set_theme_name(Some(&name));
            tracing::info!(theme = %name, "nodalix: Colloid icons");
        }
        None => {
            tracing::debug!("nodalix: Colloid is not installed; keeping the system icon theme");
        }
    }
}

fn add_icon_roots() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let theme = gtk::IconTheme::for_display(&display);
    for root in icon_roots() {
        theme.add_search_path(root);
    }
}

fn icon_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut push = |p: PathBuf| {
        if p.is_dir() && !roots.iter().any(|have| have == &p) {
            roots.push(p);
        }
    };
    push(gtk::glib::user_data_dir().join("icons"));
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        push(home.join(".icons"));
        push(home.join(".local/share/icons"));
    }
    for dir in gtk::glib::system_data_dirs() {
        push(dir.join("icons"));
    }
    roots
}

fn installed_colloid() -> Vec<String> {
    let mut names = Vec::new();
    for root in icon_roots() {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_colloid_theme(&path) {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if !names.iter().any(|have| have == name) {
                    names.push(name.to_owned());
                }
            }
        }
    }
    names.sort();
    names
}

fn is_colloid_theme(path: &Path) -> bool {
    path.join("index.theme").is_file()
        && path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n == "Colloid" || n.starts_with("Colloid-"))
}

/// Prefer the unsuffixed Dark/Light pack over a colour variant (Teal, Nord…).
pub(crate) fn choose_colloid(dark: bool, installed: &[String]) -> Option<String> {
    let exact = if dark {
        "Colloid-Dark"
    } else {
        "Colloid-Light"
    };
    if installed.iter().any(|n| n == exact) {
        return Some(exact.to_owned());
    }
    let prefix = if dark {
        "Colloid-Dark-"
    } else {
        "Colloid-Light-"
    };
    if let Some(n) = installed.iter().find(|n| n.starts_with(prefix)) {
        return Some(n.clone());
    }
    if installed.iter().any(|n| n == "Colloid") {
        return Some("Colloid".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_prefers_plain_colloid_dark_over_a_colour_variant() {
        let installed = [
            "Colloid-Dark-Teal".into(),
            "Colloid-Dark".into(),
            "Colloid-Light".into(),
        ];
        assert_eq!(
            choose_colloid(true, &installed).as_deref(),
            Some("Colloid-Dark")
        );
        assert_eq!(
            choose_colloid(false, &installed).as_deref(),
            Some("Colloid-Light")
        );
    }

    #[test]
    fn a_colour_variant_is_used_when_it_is_the_only_pack() {
        let installed = ["Colloid-Dark-Nord".into()];
        assert_eq!(
            choose_colloid(true, &installed).as_deref(),
            Some("Colloid-Dark-Nord")
        );
    }

    #[test]
    fn unsuffixed_colloid_is_the_last_named_fallback() {
        let installed = ["Colloid".into()];
        assert_eq!(choose_colloid(true, &installed).as_deref(), Some("Colloid"));
        assert_eq!(
            choose_colloid(false, &installed).as_deref(),
            Some("Colloid")
        );
    }

    #[test]
    fn a_dark_only_install_does_not_paint_dark_icons_on_light() {
        let installed = ["Colloid-Dark".into()];
        assert_eq!(choose_colloid(false, &installed), None);
        assert_eq!(
            choose_colloid(true, &installed).as_deref(),
            Some("Colloid-Dark")
        );
    }

    #[test]
    fn nothing_is_forced_when_colloid_is_absent() {
        assert_eq!(choose_colloid(true, &[]), None);
        assert_eq!(choose_colloid(false, &["Adwaita".into()]), None);
    }

    #[test]
    fn chrome_is_static_and_carries_the_cover_radius() {
        assert!(
            !CHROME.contains("animation") && !CHROME.contains("keyframes"),
            "an animation here pins the frame clock (#126)"
        );
        assert!(CHROME.contains("--nodalix-radius-cover: 22%"));
        assert!(CHROME.contains(".nodalix .navigation-sidebar"));
    }
}
