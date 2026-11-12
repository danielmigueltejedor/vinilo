#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Wrap a Vinilo .app (either edition) in a compressed installer DMG: the Nodalia background, the
# app on the left and a shortcut to /Applications on the right.
# Usage: create_dmg.sh <Vinilo*.app> <out.dmg>
set -euo pipefail

[[ "$(uname -s)" == "Darwin" ]] || { echo "DMG packaging requires macOS" >&2; exit 1; }
[[ "$#" -eq 2 ]] || { echo "Usage: create_dmg.sh <Vinilo*.app> <out.dmg>" >&2; exit 2; }

app="$1"
out="$2"
here="$(cd "$(dirname "$0")" && pwd)"
[[ -d "$app/Contents/MacOS" && "$app" == *.app ]] || { echo "not an .app bundle: $app" >&2; exit 1; }
app_file="$(basename "$app")"
app_name="${app_file%.app}"
for png in dmg-background.png dmg-background@2x.png; do
  [[ -f "$here/$png" ]] || { echo "Missing installer background: $here/$png" >&2; exit 1; }
done

version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
volname="$app_name $version"
work="$(mktemp -d)"
stage="$work/stage"
rw="$work/vinilo-rw.dmg"
trap 'rm -rf "$work" >/dev/null 2>&1 || true' EXIT

mkdir -p "$stage/.background" "$(dirname "$out")"
ditto "$app" "$stage/$app_file"
ln -s /Applications "$stage/Applications"
# render_dmg_background.py draws both sizes; Finder reads them as one TIFF.
tiffutil -cathidpicheck "$here/dmg-background.png" "$here/dmg-background@2x.png" \
  -out "$stage/.background/background.tiff" >/dev/null

# Room for the app plus file-system overhead, whatever the bundle weighs.
stage_mb="$(du -sm "$stage" | cut -f1)"
hdiutil create -srcfolder "$stage" -volname "$volname" -fs HFS+ \
  -format UDRW -size "$((stage_mb + stage_mb / 5 + 40))m" "$rw" >/dev/null

# The first line of the listing is the whole disk: detaching it releases every volume on it.
dev="$(hdiutil attach -readwrite -noverify -noautoopen "$rw" | awk 'NR==1 {print $1}')"
[[ -n "$dev" ]] || { echo "could not attach $rw" >&2; exit 1; }
mount="/Volumes/$volname"
for _ in $(seq 1 50); do [[ -d "$mount" ]] && break; sleep 0.1; done
[[ -d "$mount" ]] || { echo "Failed to mount $rw" >&2; exit 1; }

# The window layout is cosmetic. Finder scripting can be unavailable (CI
# runners, missing Automation permission): the DMG is still valid without it.
layout_window() {
osascript <<APPLESCRIPT
with timeout of 60 seconds
tell application "Finder"
  tell disk "$volname"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set sidebar width of container window to 0
    try
      set pathbar visible of container window to false
    end try
    set the bounds of container window to {200, 120, 860, 580}
    set theViewOptions to the icon view options of container window
    set arrangement of theViewOptions to not arranged
    set icon size of theViewOptions to 128
    set text size of theViewOptions to 13
    set label position of theViewOptions to bottom
    set background picture of theViewOptions to file ".background:background.tiff"
    set position of item "$app_file" of container window to {160, 190}
    set position of item "Applications" of container window to {500, 190}
    update without registering applications
    delay 2
    close
  end tell
end tell
end timeout
APPLESCRIPT
}
layout_window || echo "warning: could not arrange the DMG window; continuing without the layout" >&2

sync
# Finder or Spotlight can keep the volume busy for a moment after the layout
# step; a failed eject must not fail the release.
attached() { hdiutil info | grep -qE "^${dev}[[:space:]]"; }
detached=0
for _ in 1 2 3 4 5 6 7 8; do
  # Detached by device, so a volume that is already gone (ejected by Finder) cannot
  # leave the image attached behind it.
  if hdiutil detach "$dev" >/dev/null 2>&1 || ! attached; then detached=1; break; fi
  sleep 3
done
((detached)) || ! attached || hdiutil detach -force "$dev" >/dev/null

rm -f "$out"
hdiutil convert "$rw" -format UDZO -imagekey zlib-level=9 -o "$out" >/dev/null
echo "Created $out"
