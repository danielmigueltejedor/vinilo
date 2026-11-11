<!--
SPDX-FileCopyrightText: 2026 Miguel Rincon
SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Screenshots

What the top-level README expects to find here, and what each shot should show.

`icon.png` is **generated, not captured** — it is the app's own SVG rendered at
128px, so it can never drift from what the app actually installs:

```bash
cp data/icons/hicolor/128x128/apps/dev.danielmiguelt.Vinilo.png docs/screenshots/icon.png
```

## The shots

| File | Shows |
| --- | --- |
| `player.webp` | The open player, dark theme: the cover's colour from the headers down |
| `queue.webp` | The queue inside the open player, light theme |
| `preferences.webp` | Preferences, dark theme |
| `about.webp` | About, dark theme |

The music in them is a demo collection: invented albums with covers drawn for the
shots, so no account, real cover art or personal library appears.

## Taking them

- **Dark theme** for most of them, since that is what the app looks like on a
  default GNOME install; one light shot shows the other mode.
- Capture the **window, not the screen** — `Alt`+`PrtSc` — with its rounded
  corners transparent around it: the window's own shape is part of the
  Nodalia look.
- Play something first, and keep the *same* track across the set. A Now Playing
  bar reading "Nothing playing" undersells every screenshot it appears in, and a
  different track in each one reads as four unrelated apps.
- Avoid a half-loaded state: no spinners, no placeholder covers.
- Show what is *new*. A shot that could have been taken three versions ago is a
  wasted one.

## Processing

Resize to 1600px wide, then pick a format **by what is in the picture**:

```bash
# UI and text — lossless, so glyphs stay crisp
magick shot.png -resize 1600x -strip \
  -define webp:lossless=true -define webp:method=6 docs/screenshots/library.webp

# Mostly album art — lossy, since lossless gains nothing and costs 4x
magick shot.png -resize 1600x -strip \
  -quality 92 -define webp:method=6 docs/screenshots/albums.webp
```

Keep each one under ~500 KB. If one is stubborn, `-resize 1400x` first — nobody
reads a README at full resolution.
