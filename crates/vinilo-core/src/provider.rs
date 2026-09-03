// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which service Vinilo is playing from.
//!
//! Chosen on the first run, then again from Preferences. The file is a single
//! word in `~/.config/vinilo/provider` so the daemon can read it without glib
//! — the same split as [`crate::i18n`]. Tokens still never live here.

use std::path::PathBuf;

use crate::paths;

/// Where the music is coming from.
///
/// Apple Music and files on this computer are what play today. Spotify,
/// YouTube Music and Tidal are named so the first-run picker can be honest
/// about the roadmap instead of pretending they stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Provider {
    #[default]
    AppleMusic,
    Local,
    Spotify,
    YoutubeMusic,
    Tidal,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AppleMusic => "apple-music",
            Self::Local => "local",
            Self::Spotify => "spotify",
            Self::YoutubeMusic => "youtube-music",
            Self::Tidal => "tidal",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "apple-music" | "apple" => Some(Self::AppleMusic),
            "local" | "files" => Some(Self::Local),
            "spotify" => Some(Self::Spotify),
            "youtube-music" | "youtube" | "ytmusic" => Some(Self::YoutubeMusic),
            "tidal" => Some(Self::Tidal),
            _ => None,
        }
    }

    /// Apple Music and files on disk. The others are listed, not wired.
    pub fn is_available(self) -> bool {
        matches!(self, Self::AppleMusic | Self::Local)
    }

    /// Whether MusicKit (and therefore the Chromium sidecar) is required.
    pub fn needs_sidecar(self) -> bool {
        matches!(self, Self::AppleMusic)
    }

    /// Whether Apple's own sign-in has to run before anything can play.
    pub fn needs_apple(self) -> bool {
        matches!(self, Self::AppleMusic)
    }
}

fn provider_path() -> Option<PathBuf> {
    Some(paths::config_dir()?.join("provider"))
}

/// `None` until the first-run picker writes one.
pub fn load() -> Option<Provider> {
    let text = std::fs::read_to_string(provider_path()?).ok()?;
    Provider::parse(&text)
}

/// Persist the choice. Best-effort: failing to save must never block playback.
pub fn save(provider: Provider) {
    let Some(path) = provider_path() else {
        return;
    };
    if let Some(dir) = path.parent()
        && let Err(err) = std::fs::create_dir_all(dir)
    {
        tracing::warn!(?err, "could not create config directory");
        return;
    }
    if let Err(err) = std::fs::write(&path, format!("{}\n", provider.as_str())) {
        tracing::warn!(?err, "could not save provider");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_round_trips_through_its_string_form() {
        for provider in [
            Provider::AppleMusic,
            Provider::Local,
            Provider::Spotify,
            Provider::YoutubeMusic,
            Provider::Tidal,
        ] {
            assert_eq!(Provider::parse(provider.as_str()), Some(provider));
        }
    }

    #[test]
    fn aliases_are_forgiven() {
        assert_eq!(Provider::parse("apple"), Some(Provider::AppleMusic));
        assert_eq!(Provider::parse("files"), Some(Provider::Local));
        assert_eq!(Provider::parse("ytmusic"), Some(Provider::YoutubeMusic));
    }

    #[test]
    fn unknown_provider_is_none() {
        assert_eq!(Provider::parse("deezer"), None);
        assert_eq!(Provider::parse(""), None);
    }

    #[test]
    fn only_apple_needs_the_sidecar() {
        assert!(Provider::AppleMusic.needs_sidecar());
        assert!(Provider::AppleMusic.needs_apple());
        assert!(!Provider::Local.needs_sidecar());
        assert!(!Provider::Spotify.needs_apple());
    }
}
