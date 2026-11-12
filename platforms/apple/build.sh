#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Build "Vinilo Native.app" (the SwiftUI edition): the Swift app, the daemon it talks to, and the
# castLabs Electron the daemon plays through. Its own bundle and identifier, so it installs beside
# "Vinilo Nodalia.app" and each keeps its own settings.
# Usage: platforms/apple/build.sh <out-dir>   (run from the repository root, after
# `cargo build --release -p vinilod --locked` and `make sidecar`)
set -euo pipefail

[[ "$(uname -s)" == Darwin ]] || { echo "macOS only" >&2; exit 1; }

out="${1:-dist/macos}"
here="$(cd "$(dirname "$0")" && pwd)"
shared="$here/../../packaging/macos"
version="$(awk -F\" '/^version = / {print $2; exit}' Cargo.toml)"
name="Vinilo Native"
appid="dev.danielmiguelt.Vinilo"   # the icon files carry the product's id, not the edition's

[[ -x target/release/vinilod ]] || { echo "build first: cargo build --release -p vinilod --locked" >&2; exit 1; }
[[ -d sidecar/node_modules/electron/dist/Electron.app ]] || { echo "install the sidecar first: make sidecar" >&2; exit 1; }

swift build --package-path "$here" -c release --product ViniloMac
swift_bin="$(swift build --package-path "$here" -c release --show-bin-path)/ViniloMac"

app="$out/$name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

sed -e "s|@NAME@|$name|g" -e "s|@VERSION@|$version|g" "$here/Info.plist.in" > "$app/Contents/Info.plist"
install -m755 "$swift_bin" "$app/Contents/MacOS/ViniloNative"
# Beside the app's own executable, which is where DaemonLauncher.bundled looks for it.
install -m755 target/release/vinilod "$app/Contents/MacOS/vinilod"

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

sidecar="$app/Contents/Resources/sidecar"
mkdir -p "$sidecar"
cp -R sidecar/package.json sidecar/main.js sidecar/preload.js sidecar/queue-identity.js sidecar/node_modules "$sidecar/"
cp COPYING "$app/Contents/Resources/COPYING"

# Apple Music on a Mac needs the player VMP-signed, and that has to come before the code
# signature below: the signature files live inside the bundle.
"$shared/vmp_sign.sh" "$app"

# Any non-system library the daemon links, copied in, then an ad-hoc signature over everything.
python3 "$shared/deploy_dylibs.py" "$app"

echo "$app"
