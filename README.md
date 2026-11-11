<!--
SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
SPDX-FileCopyrightText: 2026 Miguel Rincon
SPDX-License-Identifier: GPL-3.0-or-later
-->

<div align="center">
  <a href="https://getnodalia.com">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/danielmigueltejedor/nodalia-web/main/assets/brand/nodalia-wordmark-white.svg">
      <img src="https://raw.githubusercontent.com/danielmigueltejedor/nodalia-web/main/assets/brand/nodalia-wordmark-black.svg" width="150" alt="Nodalia">
    </picture>
  </a>
  <br><br>
  <img src="https://raw.githubusercontent.com/danielmigueltejedor/vinilo/main/data/icons/hicolor/512x512/apps/dev.danielmiguelt.Vinilo.png" width="200" alt="Vinilo">
  <h1>Vinilo</h1>
  <p><strong>A native music player for Linux, drawn in the Nodalia Design Language.</strong></p>

  <p>
    <img src="https://img.shields.io/badge/version-2.0.0-4a86cf" alt="Version 2.0.0">
    <img src="https://img.shields.io/badge/platform-Linux%20x86__64-fcc624?logo=linux&logoColor=black" alt="Linux x86_64">
    <img src="https://img.shields.io/badge/platform-macOS%20arm64-000000?logo=apple&logoColor=white" alt="macOS arm64">
    <img src="https://img.shields.io/badge/GTK4%20%2F%20Nodalia%20Design-4a86cf" alt="GTK4 and Nodalia Design">
    <img src="https://img.shields.io/badge/Rust-dea584?logo=rust&logoColor=black" alt="Written in Rust">
    <img src="https://img.shields.io/badge/interface-System%20%7C%20English%20%7C%20Español-8a63d2" alt="System, English and Spanish">
    <a href="./COPYING"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-2ea44f" alt="GPL-3.0-or-later"></a>
  </p>

  <p>
    <a href="#installation">Installation</a> ·
    <a href="#sources">Sources</a> ·
    <a href="#preview">Preview</a> ·
    <a href="#why-vinilo">Why Vinilo</a> ·
    <a href="#first-launch">First launch</a> ·
    <a href="#contributing">Contributing</a> ·
    <a href="#translations">Translations</a> ·
    <a href="#support">Support</a>
  </p>
</div>

---

Vinilo is a native front-end for the music you already pay for, not a website
wrapped in a window. The interface is GTK4 drawn in the Nodalia Design Language. Apple Music still
needs Apple's own player and Google's official Widevine CDM, so a small
Chromium process stays hidden after sign-in. Local files never touch it.

The app talks to a small engine that keeps playing in the background, so
closing the window does not stop the music.

> [!IMPORTANT]
> **Apple Music** needs an active subscription, an **x86_64** machine and a
> network connection on every play. Linux Widevine cannot keep licences
> offline, so there is no download button and there never will be.

> [!TIP]
> **This computer** plays MP3, FLAC, Ogg and WAV on its own. No Chromium, no
> account, no sidecar. Choose that source on first launch if you only want
> files.

## Preview

<p align="center">
  <img src="docs/screenshots/library.webp" width="48%" alt="Listen Now in Vinilo 2.0: islands, Sora titles and NDL icons">
  <img src="docs/screenshots/player.webp" width="48%" alt="Expanded player with the queue">
</p>

<p align="center">
  <img src="docs/screenshots/playlist.webp" width="48%" alt="Playlist page">
  <img src="docs/screenshots/albums.webp" width="48%" alt="An album page">
</p>

## Why Vinilo

| Interface | Playback | Desktop |
| --- | --- | --- |
| A native app in the Nodalia Design Language, not a website in a frame | Gapless playback, queue, lyrics and karaoke with a voice level on catalogue tracks | Play, pause and skip from the top bar, lock screen or media keys |
| Your library: songs, albums, artists and playlists | The catalogues you already use, searchable from one place | Music keeps going when you close the window |
| English or Spanish, or the system language | Crossfade, local listening stats, optional Last.fm scrobbling | Open audio files straight from the file manager |

Vinilo is built for daily listening: one queue, one native interface, and a small hidden
web layer only where DRM requires it.

## Sources

| Source | Channel | What you need |
| --- | --- | --- |
| Apple Music | Stable | Active subscription, x86_64, network on every play |
| This computer | Stable | Audio files on disk |
| Spotify | Stable | Sign-in. Premium uses librespot; otherwise a fallback |
| YouTube Music | Beta | Sign-in |
| Tidal | Alpha | Sign-in |

Favourites, library saves and new playlists are available from the row menu
where the source allows it. Change language or source later in
**Preferences** (`Ctrl`+`,`).

### Listening stats and Last.fm

Vinilo keeps play counts and time listened on this machine (sidebar →
**Listening**). To mirror that online, open **Preferences → Last.fm**:

1. Create an API account at [last.fm/api/account/create](https://www.last.fm/api/account/create).
2. Paste the **API key** and **shared secret**.
3. Click **Connect**, authorize Vinilo in the browser, then **I authorized Vinilo**.

Scrobbles and now-playing updates go out from the engine for every source.
Credentials live in `~/.config/vinilo/lastfm.json` and are never written to
logs.

## Installation

Build and install from a clone of this repository, **inside that directory**,
into `~/.local` (no `sudo`).

### Arch Linux — recommended

1. Install the build and runtime packages:

   ```bash
   sudo pacman -S --needed base-devel pkgconf rustup gtk4 librsvg \
                           nodejs npm libpulse alsa-lib yt-dlp ffmpeg webkitgtk-6.0
   rustup default stable   # Vinilo needs Rust ≥ 1.93 (edition 2024)
   ```

2. Clone and install:

   ```bash
   git clone https://github.com/danielmigueltejedor/vinilo.git
   cd vinilo
   make install
   ```

3. Open **Vinilo** from the app grid, or run `vinilo`.

The first Apple Music install also fetches castLabs Electron (~200 MB) for the
Widevine CDM. Local-only use skips that once you never open Apple Music.

> [!TIP]
> On **fish**, GNOME's PATH often omits `~/.local/bin`. Add it once with
> `fish_add_path ~/.local/bin`.

### Updating

```bash
cd ~/vinilo
pkill vinilo; pkill vinilod
make update
```

`make update` discards a dirty local `Cargo.lock`, fast-forwards `main` and
reinstalls. If the grid still launches nothing, the desktop file must read
`Exec=/home/YOU/.local/bin/vinilo-desktop`. Log out once if an icon or launcher
looks stale.

### macOS (Apple Silicon)

Download `Vinilo-<version>-macos-arm64.dmg` from the
[latest release](https://github.com/danielmigueltejedor/vinilo/releases/latest),
open it and drag Vinilo to Applications. The app is signed ad-hoc, not
notarized, so the first time macOS refuses to open it: right-click Vinilo in
Applications, choose **Open**, and confirm.

YouTube Music, Tidal and links play through `yt-dlp` and `ffmpeg`:
`brew install yt-dlp ffmpeg`. Those sources sign in through your browser, and
Vinilo reads the session back when you press Done.

To build it yourself: `brew install gtk4 librsvg adwaita-icon-theme pkgconf node`,
then `cargo build --release && make sidecar && packaging/macos/bundle.sh dist/macos`.

### Default player for files

In Files: right-click a track → **Open With** → Vinilo → **Always**. Or:

```bash
xdg-mime default dev.danielmiguelt.Vinilo.desktop audio/mpeg
xdg-mime default dev.danielmiguelt.Vinilo.desktop audio/flac
xdg-mime default dev.danielmiguelt.Vinilo.desktop audio/ogg
```

### Development

```bash
cargo run                 # the app
make sidecar-run          # hidden player, window visible (DRM isolation)
make check                # fmt + clippy + tests
```

## First launch

1. The interface follows the system language (English or Spanish). Change it in
   Preferences if you want.
2. Choose a source: Apple Music, this computer, Spotify, YouTube Music or Tidal.
3. Streaming sources open a **sign-in window** you cannot skip. Sign out from
   the app menu to pick another source.
4. The Apple Music library is cached for the next start. Local files play as
   soon as you open them.

Language lives in `~/.config/vinilo/locale`. The source lives in
`~/.config/vinilo/provider`. Catalogue sessions stay in cookies next to that;
tokens are never written to disk.

## How it works

```
               ┌────────────────────────────┐
               │  Vinilo  GTK4 / Nodalia    │
               │  library · search · player │
               └─────────────┬──────────────┘
                             │ JSON on a Unix socket
┌────────────────────────────▼──────────────────────────────────┐
│  vinilod — the engine                         no window      │
│  queue · desktop controls · artwork · local files · cache    │
│  listen stats · Last.fm scrobbles                             │
└──────────────┬──────────────────────────────┬────────────────┘
               │ Apple Music                  │ files / catalogues
┌──────────────▼──────────────┐    ┌──────────▼────────────────┐
│  sidecar  castLabs Electron │    │  rodio in the engine      │
│  MusicKit + Widevine CDM    │    │  MP3, FLAC, streams…      │
└─────────────────────────────┘    └───────────────────────────┘
```

Apple Music goes through Apple's MusicKit player and Google's official CDM.
Vinilo does not strip DRM, cache decrypted audio, or offer downloads.

## Limitations

| Constraint | Why |
| --- | --- |
| No offline Apple Music | Linux Widevine cannot persist licences |
| ~200 MB Chromium sidecar | Only if you use Apple Music |
| x86_64 only for Apple Music | Linux ARM Widevine is not a stable target |
| Spotify and Apple Music are ready; YouTube Music is **Beta**; Tidal is **Alpha** | Catalogue clients are still settling |
| Karaoke voice attenuation is catalogue / local only | MusicKit never hands Vinilo samples |
| Apple Music on macOS needs Widevine VMP signing | Without it Apple's licence server can refuse playback; local files and Spotify are unaffected |
| macOS: no media keys or Now Playing yet | The shell controls are MPRIS, which is Linux's |

## Contributing

Patches, bug reports and translations are welcome.

1. **Fork** and branch from `main` (`feat/…`, `fix/…`, `docs/…`).
2. Keep changes focused. The engine lives in `crates/vinilo-core` and
   `crates/vinilod`; drawing belongs in `crates/vinilo`. A terminal client that
   draws nothing should still be able to use anything you add to the engine.
3. Before opening a pull request:

   ```bash
   make check    # rustfmt + clippy -D warnings + tests
   ```

4. Use [conventional commits](https://www.conventionalcommits.org/)
   (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`).
5. Do not paste tokens, cookies, `settings.ini` or `lastfm.json` into issues or
   PRs.
6. Read [`CLAUDE.md`](./CLAUDE.md) / [`AGENTS.md`](./AGENTS.md) before large
   changes — especially the DRM line we do not cross, and MusicKit owning the
   queue.

Licensing: every new source file needs the two-line SPDX header
(`GPL-3.0-or-later`).

## Translations

Strings are runtime tables in
[`crates/vinilo-core/src/i18n.rs`](./crates/vinilo-core/src/i18n.rs), not
gettext. The app and the engine both call `t(Key::…)`, so one edit covers
both.

### Add a language

1. Add a variant to `Language` (and usually to `Locale`).
2. Add an `xx(Key) -> &'static str` table beside `en` / `es` — **every** `Key`
   must be covered; the compiler will tell you if one is missing.
3. Wire it in `t()`, `Locale::parse`, `Locale::resolve`, and the Preferences
   language combo (native name + index).
4. Search for other `match current()` arms that build sentences outside `t`
   (plurals, shortcuts) and extend those too.

### Rules of thumb

- Prefer short, GNOME-flavoured wording over marketing copy.
- Keep placeholders (`{}`) identical across languages when a string is filled
  with `replace`.
- Re-run `make check` after editing the tables; `rustfmt` is picky about long
  string literals.

The user's choice is stored in `~/.config/vinilo/locale` (`system`, `en`, or
`es`). System follows `LC_MESSAGES` / `LANG` and falls back to English when the
tag is unsupported.

## Support

- Use [GitHub Issues](https://github.com/danielmigueltejedor/vinilo/issues) for
  reproducible bugs and focused feature requests.
- Include the Vinilo version (`2.0.0`), distribution, GTK version
  and the source (Apple Music, local, Spotify, YouTube Music or Tidal).
- Never paste tokens, cookies, `settings.ini` or `lastfm.json` into an issue.

## Credits

Vinilo is a fork of [Slipmat](https://github.com/SoftARV/Slipmat)
(GPL-3.0-or-later) by Miguel Rincon. Linux Apple Music playback through
castLabs Electron follows the path opened by Sidra and Cider.

## License

Vinilo is released under the [GPL-3.0-or-later](./COPYING).

<p align="center">
  Designed and maintained by Daniel Miguel Tejedor.
</p>
