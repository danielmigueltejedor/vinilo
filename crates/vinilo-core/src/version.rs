// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which of two running builds should own the app.
//!
//! The app stays alive in the background after its window closes, so a newly
//! installed build can find an older one already holding the bus name. GTK
//! would then hand the launch to the old process and exit, and the update
//! would look as if it never installed.

/// This build's version, as the running app advertises it.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Whether a running instance reporting `running` should give way to this
/// build. `None` means it advertised no version, which only builds before
/// this check did.
pub fn should_replace(running: Option<&str>, ours: &str) -> bool {
    match running {
        None => true,
        Some(theirs) => parse(theirs) < parse(ours),
    }
}

/// `1.1.5`, `1.2.0-dev` → numeric components, with a pre-release sorting below
/// its release.
fn parse(version: &str) -> (Vec<u64>, bool) {
    let (core, pre) = match version.trim().split_once('-') {
        Some((core, _)) => (core, true),
        None => (version.trim(), false),
    };
    let numbers = core
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect();
    (numbers, !pre)
}

#[cfg(test)]
mod tests {
    use super::should_replace;

    #[test]
    fn an_instance_without_a_version_is_older() {
        assert!(should_replace(None, "1.1.6"));
    }

    #[test]
    fn an_older_release_gives_way() {
        assert!(should_replace(Some("1.1.2"), "1.1.6"));
        assert!(should_replace(Some("1.1.9"), "1.2.0"));
    }

    #[test]
    fn the_same_or_a_newer_build_keeps_the_app() {
        assert!(!should_replace(Some("1.1.6"), "1.1.6"));
        assert!(!should_replace(Some("1.2.0"), "1.1.6"));
    }

    #[test]
    fn a_pre_release_sorts_below_its_release() {
        assert!(should_replace(Some("1.2.0-dev"), "1.2.0"));
        assert!(!should_replace(Some("1.2.0"), "1.2.0-dev"));
    }

    #[test]
    fn junk_never_panics() {
        assert!(!should_replace(Some("x.y"), ""));
        assert!(should_replace(Some(""), "0.0.1"));
    }
}
