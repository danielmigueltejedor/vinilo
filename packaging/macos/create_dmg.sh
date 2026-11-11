#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Wrap Vinilo.app in a compressed DMG with a shortcut to /Applications.
# Usage: create_dmg.sh <Vinilo.app> <out.dmg>
set -euo pipefail

app="$1"
out="$2"
[[ -x "$app/Contents/MacOS/vinilo" ]] || { echo "not a Vinilo.app: $app" >&2; exit 1; }

version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

ditto "$app" "$stage/Vinilo.app"
ln -s /Applications "$stage/Applications"
mkdir -p "$(dirname "$out")"
rm -f "$out"
hdiutil create -volname "Vinilo $version" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$out"
