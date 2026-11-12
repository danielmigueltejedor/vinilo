// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which edition of Vinilo this process belongs to.
//!
//! macOS ships two official editions that share this engine: *Nodalia* (the GTK interface, the
//! default) and *Native* (SwiftUI). They are meant to be installed side by side, so everything
//! one of them writes or locks has to be a different file from the other's: the daemon's
//! socket and lock, the queue it restores, its configuration and caches, the sidecar's browser
//! profile (Chromium refuses to share one between two running processes).
//!
//! The default edition keeps the names it has always had, so nobody's existing data moves. An
//! edition is chosen by the launcher through `VINILO_EDITION`; the daemon and the sidecar it
//! spawns inherit it, which is how the three processes of one edition agree without a flag
//! threaded through every call.

/// The environment variable the launcher sets.
pub const VAR: &str = "VINILO_EDITION";

/// The edition's slug, if it is not the default: lowercase ASCII letters, digits and `-`, at
/// most 16 characters. Anything else is ignored rather than trusted into a path.
pub fn slug() -> Option<String> {
    clean(&std::env::var(VAR).ok()?)
}

fn clean(raw: &str) -> Option<String> {
    let slug = raw.trim().to_ascii_lowercase();
    let valid = !slug.is_empty()
        && slug.len() <= 16
        && !slug.starts_with('-')
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    // `nodalia` names the default edition on purpose: it is what the packages call it, so a
    // launcher that sets it spells the same directories as one that sets nothing.
    (valid && slug != "nodalia").then_some(slug)
}

/// The folder name for this edition under a config, cache or state base: `vinilo`, or
/// `vinilo-native`.
pub fn dir_name() -> String {
    dir_name_for(slug().as_deref())
}

fn dir_name_for(edition: Option<&str>) -> String {
    match edition {
        Some(edition) => format!("vinilo-{edition}"),
        None => "vinilo".to_owned(),
    }
}

/// A runtime file name such as `vinilo.sock`, or `vinilo-native.sock`.
pub fn runtime_name(extension: &str) -> String {
    format!("{}.{extension}", dir_name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_edition_keeps_the_names_it_always_had() {
        assert_eq!(clean(""), None);
        assert_eq!(clean("nodalia"), None);
        assert_eq!(clean("  Nodalia "), None);
    }

    #[test]
    fn an_edition_is_a_short_lowercase_slug() {
        assert_eq!(clean("native").as_deref(), Some("native"));
        assert_eq!(clean(" Native ").as_deref(), Some("native"));
        assert_eq!(clean("ipad-1").as_deref(), Some("ipad-1"));
    }

    #[test]
    fn two_editions_never_share_a_folder_or_a_socket() {
        assert_eq!(dir_name_for(None), "vinilo");
        assert_eq!(dir_name_for(Some("native")), "vinilo-native");
        assert_ne!(dir_name_for(None), dir_name_for(Some("native")));
    }

    #[test]
    fn nothing_that_could_leave_the_directory_is_accepted() {
        for bad in [
            "../x",
            "a/b",
            "a b",
            "-x",
            "x.y",
            "ñ",
            "a_very_long_edition_name",
        ] {
            assert_eq!(clean(bad), None, "{bad}");
        }
    }
}
