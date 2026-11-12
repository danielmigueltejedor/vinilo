#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Build "Vinilo Nodalia.app" (the GTK edition) from a release build and Homebrew's GTK.
# The SwiftUI edition is built by platforms/apple/build.sh, with its own bundle and identifier.
# Usage: packaging/macos/bundle.sh <out-dir>   (run from the repository root,
# after `cargo build --release` and `make sidecar`)
set -euo pipefail

[[ "$(uname -s)" == Darwin ]] || { echo "macOS only" >&2; exit 1; }

out="${1:-dist/macos}"
here="$(cd "$(dirname "$0")" && pwd)"
brew="$(brew --prefix)"
version="$(awk -F\" '/^version = / {print $2; exit}' Cargo.toml)"
min_macos="${MACOSX_DEPLOYMENT_TARGET:-$(sw_vers -productVersion | cut -d. -f1).0}"
appid="dev.danielmiguelt.Vinilo"
# What the edition is called to people. The identifier stays the one every Nodalia-edition
# install already has, so settings and caches do not move; the other edition has its own.
name="Vinilo Nodalia"

[[ -x target/release/vinilo && -x target/release/vinilod ]] || { echo "build first: cargo build --release" >&2; exit 1; }
[[ -d sidecar/node_modules/electron/dist/Electron.app ]] || { echo "install the sidecar first: make sidecar" >&2; exit 1; }

app="$out/$name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/share" "$app/Contents/Resources/lib"

sed -e "s|@NAME@|$name|g" -e "s|@VERSION@|$version|g" -e "s|@MIN_MACOS@|$min_macos|g" "$here/Info.plist.in" > "$app/Contents/Info.plist"
install -m755 "$here/launcher.sh" "$app/Contents/MacOS/Vinilo"
install -m755 target/release/vinilo target/release/vinilod "$app/Contents/MacOS/"
# The Now Playing bridge: Control Center and the media keys, which GTK cannot reach. Swift, the
# same code as Vinilo Native (platforms/apple), started by vinilo and ended with it.
swift build -c release --package-path platforms/apple --product ViniloNowPlaying
install -m755 "$(swift build -c release --package-path platforms/apple --show-bin-path)/ViniloNowPlaying" \
    "$app/Contents/MacOS/vinilo-nowplaying"

# The icon, from the PNGs every other package already ships.
iconset="$(mktemp -d)/Vinilo.iconset"
mkdir -p "$iconset"
png() { echo "data/icons/hicolor/${1}x${1}/apps/$appid.png"; }
cp "$(png 16)" "$iconset/icon_16x16.png"
cp "$(png 32)" "$iconset/icon_16x16@2x.png"
cp "$(png 32)" "$iconset/icon_32x32.png"
cp "$(png 64)" "$iconset/icon_32x32@2x.png"
cp "$(png 128)" "$iconset/icon_128x128.png"
cp "$(png 256)" "$iconset/icon_128x128@2x.png"
cp "$(png 256)" "$iconset/icon_256x256.png"
cp "$(png 512)" "$iconset/icon_256x256@2x.png"
cp "$(png 512)" "$iconset/icon_512x512.png"
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Vinilo.icns"

# Vinilo's own icons, and the themes GTK draws symbolic icons from.
share="$app/Contents/Resources/share"
mkdir -p "$share/icons"
cp -R data/icons/hicolor "$share/icons/"
rm -rf "$share/icons/hicolor/source"
for theme in Adwaita hicolor; do
    [[ -d "$brew/share/icons/$theme" ]] && ditto "$brew/share/icons/$theme" "$share/icons/$theme"
done
gtk4-update-icon-cache -q -t -f "$share/icons/hicolor" || true

# GSettings schemas GTK reads at start-up (file chooser, colour scheme).
mkdir -p "$share/glib-2.0/schemas"
cp "$brew"/share/glib-2.0/schemas/org.gtk.gtk4.*.xml "$share/glib-2.0/schemas/" 2>/dev/null || true
glib-compile-schemas "$share/glib-2.0/schemas"

# Image loaders, with a cache whose paths the launcher fills in.
pixbuf="$app/Contents/Resources/lib/gdk-pixbuf-2.0/2.10.0"
mkdir -p "$pixbuf/loaders"
cp "$brew"/lib/gdk-pixbuf-2.0/2.10.0/loaders/*.so "$pixbuf/loaders/" 2>/dev/null || true
GDK_PIXBUF_MODULEDIR="$pixbuf/loaders" gdk-pixbuf-query-loaders "$pixbuf"/loaders/*.so 2>/dev/null \
    | sed "s|$app/Contents/Resources|@RESOURCES@|g" > "$pixbuf/loaders.cache.in" || true

# The sidecar, with the castLabs Electron for this machine.
sidecar="$app/Contents/Resources/sidecar"
mkdir -p "$sidecar"
cp -R sidecar/package.json sidecar/main.js sidecar/preload.js sidecar/queue-identity.js sidecar/node_modules "$sidecar/"

cp COPYING "$app/Contents/Resources/COPYING"

# Apple Music on a Mac needs the player VMP-signed (castLabs EVS). That has to happen BEFORE the
# code signature below: the signature files live inside the bundle and would break its seal.
"$here/vmp_sign.sh" "$app"

# Every non-system library, copied in and pointed at, then an ad-hoc signature.
python3 "$here/deploy_dylibs.py" "$app"

echo "$app"
