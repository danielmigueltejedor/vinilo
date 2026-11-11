#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Wrap Vinilo.app in a compressed DMG whose window is the installer: Nodalia's dark frame,
# the app on a record, a chevron towards a shortcut to /Applications.
# Usage: create_dmg.sh <Vinilo.app> <out.dmg>
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "DMG packaging requires macOS" >&2
    exit 1
fi

app="$1"
out="$2"
here="$(cd "$(dirname "$0")" && pwd)"
[[ -x "$app/Contents/MacOS/vinilo" ]] || { echo "not a Vinilo.app: $app" >&2; exit 1; }
# 1x + 2x TIFF drawn by render_dmg_background.sh.
bg="$here/dmg-background.tiff"
[[ -f "$bg" ]] || { echo "missing the installer background: $bg" >&2; exit 1; }

version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
volname="Vinilo $version"
work="$(mktemp -d)"
stage="$work/stage"
rw="$work/vinilo-rw.dmg"
trap 'rm -rf "$work"' EXIT

mkdir -p "$stage/.background"
ditto "$app" "$stage/Vinilo.app"
ln -s /Applications "$stage/Applications"
cp "$bg" "$stage/.background/background.tiff"

# Room for the app plus file-system overhead, whatever the bundle weighs.
stage_mb="$(du -sm "$stage" | cut -f1)"
hdiutil create -srcfolder "$stage" -volname "$volname" -fs HFS+ \
    -fsargs "-c c=64,a=16,e=16" -format UDRW -size "$((stage_mb + stage_mb / 5 + 40))m" "$rw" >/dev/null

hdiutil attach -readwrite -noverify -noautoopen "$rw" >/dev/null
mount="/Volumes/$volname"
for _ in $(seq 1 50); do [[ -d "$mount" ]] && break; sleep 0.1; done
[[ -d "$mount" ]] || { echo "failed to mount $rw" >&2; exit 1; }

# The window layout is cosmetic. Finder scripting can be unavailable (CI runners without
# Automation permission): the DMG is still valid without it.
layout_window() {
osascript <<EOF
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
    -- 660 x 460 including the title bar, so the whole composition shows.
    set the bounds of container window to {200, 120, 860, 580}
    set theViewOptions to the icon view options of container window
    set arrangement of theViewOptions to not arranged
    set icon size of theViewOptions to 128
    set text size of theViewOptions to 13
    set label position of theViewOptions to bottom
    set background picture of theViewOptions to file ".background:background.tiff"
    set position of item "Vinilo.app" of container window to {160, 190}
    set position of item "Applications" of container window to {500, 190}
    update without registering applications
    delay 2
    close
  end tell
end tell
end timeout
EOF
}
layout_window || echo "warning: could not arrange the DMG window; continuing without the layout" >&2

sync
# Finder or Spotlight can keep the volume busy for a moment after the layout step; a
# failed eject must not fail the release.
detached=0
for _ in 1 2 3 4 5 6 7 8; do
    if hdiutil detach "$mount" >/dev/null 2>&1; then detached=1; break; fi
    sleep 3
done
if ((!detached)); then
    hdiutil detach -force "$mount" >/dev/null
fi
mkdir -p "$(dirname "$out")"
rm -f "$out"
hdiutil convert "$rw" -format UDZO -imagekey zlib-level=9 -o "$out" >/dev/null
echo "Created $out"
