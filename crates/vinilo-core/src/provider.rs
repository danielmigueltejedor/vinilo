// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which service Vinilo is playing from.
//!
//! Chosen on the first run, then again from Preferences. The file is a single
//! word in `~/.config/vinilo/provider` so the daemon can read it without glib
//! — the same split as [`crate::i18n`]. Tokens still never live here.

use std::path::PathBuf;

use crate::paths;

/// How finished a source is, shown in the picker and Preferences.
///
/// Local files and Apple Music are stable. Spotify and YouTube Music
/// are Beta; Tidal is still Alpha. Apple still needs a heavy Chromium sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maturity {
    Ready,
    Beta,
    Alpha,
}

/// Where the music is coming from.
///
/// Apple Music, files on this computer, and the three catalogues Vinilo
/// searches in-app. Spotify plays through librespot when a Premium session is
/// available (the same approach Sonora uses). YouTube Music prefers InnerTube
/// audio. Tidal still goes through `yt-dlp`. Native audio that fails falls
/// back to `yt-dlp`.
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
    pub const ALL: [Self; 5] = [
        Self::AppleMusic,
        Self::Local,
        Self::Spotify,
        Self::YoutubeMusic,
        Self::Tidal,
    ];

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

    pub fn from_index(index: u32) -> Self {
        Self::ALL
            .get(index as usize)
            .copied()
            .unwrap_or(Self::AppleMusic)
    }

    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0) as u32
    }

    pub fn is_available(self) -> bool {
        true
    }

    /// Whether MusicKit (and therefore the Chromium sidecar) is required.
    pub fn needs_sidecar(self) -> bool {
        matches!(self, Self::AppleMusic)
    }

    /// Whether Apple's own sign-in has to run before anything can play.
    pub fn needs_apple(self) -> bool {
        matches!(self, Self::AppleMusic)
    }

    /// Search-and-play catalogues that are not Apple Music or local files.
    pub fn is_catalog(self) -> bool {
        matches!(self, Self::Spotify | Self::YoutubeMusic | Self::Tidal)
    }

    /// Badge in the picker: nothing for a finished source, Beta or Alpha otherwise.
    pub fn maturity(self) -> Maturity {
        match self {
            Self::Local | Self::AppleMusic => Maturity::Ready,
            Self::Spotify | Self::YoutubeMusic => Maturity::Beta,
            Self::Tidal => Maturity::Alpha,
        }
    }

    /// Stem for caches that must not be shared across sources. Apple Music
    /// keeps the original filenames (`library.json`) so an existing library
    /// is still there after a round trip through Spotify.
    pub fn cache_stem(self) -> Option<&'static str> {
        match self {
            Self::AppleMusic => None,
            Self::Local => Some("local"),
            Self::Spotify => Some("spotify"),
            Self::YoutubeMusic => Some("youtube-music"),
            Self::Tidal => Some("tidal"),
        }
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
        assert!(Provider::Spotify.is_catalog());
        assert_eq!(Provider::from_index(2), Provider::Spotify);
        assert_eq!(Provider::Tidal.index(), 4);
        assert_eq!(Provider::AppleMusic.cache_stem(), None);
        assert_eq!(Provider::Spotify.cache_stem(), Some("spotify"));
    }

    #[test]
    fn maturity_matches_how_ready_each_source_is() {
        assert_eq!(Provider::Local.maturity(), Maturity::Ready);
        assert_eq!(Provider::AppleMusic.maturity(), Maturity::Ready);
        assert_eq!(Provider::Spotify.maturity(), Maturity::Beta);
        assert_eq!(Provider::YoutubeMusic.maturity(), Maturity::Beta);
        assert_eq!(Provider::Tidal.maturity(), Maturity::Alpha);
    }
}
