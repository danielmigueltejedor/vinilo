#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Draws the installer window's background: Nodalia's dark frame, a record under the app
# icon, a cyan chevron towards Applications and the hint below. Writes the 1x + 2x TIFF
# create_dmg.sh puts in the volume. Needs ImageMagick 7 and Inter (nodalia-design/fonts).
#   packaging/macos/render_dmg_background.sh <Inter.ttf>
#
# Geometry matches create_dmg.sh: a 660x432 point window with the icons at (160,190) and
# (500,190). The canvas is 2000x1300 points because Finder never scales the picture: a
# window opened larger keeps showing the frame, not a white band.
set -euo pipefail
font="${1:?usage: render_dmg_background.sh <Inter.ttf>}"
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

render() {
    local k=$1 cx=$((160 * $1)) cy=$((190 * $1)) grooves="" r
    for r in $(seq 76 6 146); do grooves+=" circle $cx,$cy $((cx + r * k)),$cy"; done
    magick -size $((2000 * k))x$((1300 * k)) xc:"#0B0C10" \
        -fill "#121419" -stroke none -draw "circle $cx,$cy $((cx + 150 * k)),$cy" \
        -fill none -stroke "#1D2029" -strokewidth "$k" -draw "$grooves" \
        -stroke "#2CC5D3" -strokewidth $((9 * k)) \
        -draw "stroke-linecap round stroke-linejoin round polyline $((305 * k)),$((166 * k)) $((329 * k)),$((190 * k)) $((305 * k)),$((214 * k))" \
        -draw "stroke-linecap round stroke-linejoin round polyline $((335 * k)),$((166 * k)) $((359 * k)),$((190 * k)) $((335 * k)),$((214 * k))" \
        -stroke none -fill "#8A8F9C" -font "$font" -pointsize $((15 * k)) -gravity northwest \
        -annotate +$((215 * k))+$((360 * k)) "Arrastra Vinilo a Aplicaciones" \
        "$work/bg@${k}x.png"
}
render 1
render 2
magick \( "$work/bg@1x.png" -set units PixelsPerInch -set density 72 \) \
    \( "$work/bg@2x.png" -set units PixelsPerInch -set density 144 \) \
    -alpha off -compress lzw "$here/dmg-background.tiff"
echo "Wrote $here/dmg-background.tiff"
