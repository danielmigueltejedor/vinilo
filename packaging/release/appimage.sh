#!/usr/bin/env bash
# Build a Vinilo AppImage from the tree that `stage.sh` already laid out.
#
# Same Ubuntu image as the .deb job so the bundled GTK / WebKit / Electron
# libraries match what we compile against. Tools are pinned and checksummed.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

VERSION=$(awk -F\" '/^version = / {print $2; exit}' Cargo.toml)
STAGE="${STAGE:-dist/root}"
APPDIR="${APPDIR:-dist/Vinilo.AppDir}"
OUT_DIR="${OUT_DIR:-dist/packages}"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ARCH=x86_64 ;;
  *)
    echo "AppImage release is x86_64-only for now (got $ARCH)" >&2
    exit 1
    ;;
esac

[[ -x "$STAGE/usr/bin/vinilo" ]]
[[ -d "$STAGE/usr/share/vinilo/sidecar/node_modules" ]]

TOOLS="${TOOLS_DIR:-dist/appimage-tools}"
mkdir -p "$TOOLS"

LINUXDEPLOY_VER="1-alpha-20251107-1"
LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/${LINUXDEPLOY_VER}/linuxdeploy-${ARCH}.AppImage"
LINUXDEPLOY_SHA="c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d"

PLUGIN_APPIMAGE_VER="1-alpha-20250213-1"
PLUGIN_APPIMAGE_URL="https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/${PLUGIN_APPIMAGE_VER}/linuxdeploy-plugin-appimage-${ARCH}.AppImage"
PLUGIN_APPIMAGE_SHA="992d502a248e14ab185448ddf6f6e7d25558cb84d4623c354c3af350c25fccb3"

PLUGIN_GTK_REV="7a3fbc31a9e5075073ff8790f26effbac5f84453"
PLUGIN_GTK_URL="https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/${PLUGIN_GTK_REV}/linuxdeploy-plugin-gtk.sh"
PLUGIN_GTK_SHA="b0f4cbc684a0103a9651f0955b635eaea0096b3a66c0f5a2c2aa337960375171"

fetch() {
  local url="$1" dest="$2" sha="$3"
  if [[ -f "$dest" ]]; then
    echo "$sha  $dest" | sha256sum -c -
    return
  fi
  curl --proto '=https' --tlsv1.2 -sSfL -o "$dest" "$url"
  echo "$sha  $dest" | sha256sum -c -
}

fetch "$LINUXDEPLOY_URL" "$TOOLS/linuxdeploy-${ARCH}.AppImage" "$LINUXDEPLOY_SHA"
fetch "$PLUGIN_APPIMAGE_URL" "$TOOLS/linuxdeploy-plugin-appimage-${ARCH}.AppImage" "$PLUGIN_APPIMAGE_SHA"
fetch "$PLUGIN_GTK_URL" "$TOOLS/linuxdeploy-plugin-gtk.sh" "$PLUGIN_GTK_SHA"
chmod +x \
  "$TOOLS/linuxdeploy-${ARCH}.AppImage" \
  "$TOOLS/linuxdeploy-plugin-appimage-${ARCH}.AppImage" \
  "$TOOLS/linuxdeploy-plugin-gtk.sh"

# Nested AppImages need this on hosts without FUSE (CI containers).
export APPIMAGE_EXTRACT_AND_RUN=1
export PATH="$TOOLS:$PATH"

rm -rf "$APPDIR"
mkdir -p "$APPDIR"
cp -a "$STAGE"/. "$APPDIR/"

APPID="dev.danielmiguelt.Vinilo"
# linuxdeploy rewrites Exec=; keep a relative command name.
sed -i 's|^Exec=.*|Exec=vinilo-desktop %U|; s|^TryExec=.*|TryExec=vinilo-desktop|' \
  "$APPDIR/usr/share/applications/$APPID.desktop"

APPRUN="$TOOLS/Vinilo.AppRun"
cat >"$APPRUN" <<'EOF'
#!/bin/sh
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
HERE="$(dirname "$(readlink -f "$0")")"
export APPDIR="${APPDIR:-$HERE}"
export PATH="${APPDIR}/usr/bin:${PATH:-/usr/bin}"
export XDG_DATA_DIRS="${APPDIR}/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export VINILO_SIDECAR="${APPDIR}/usr/share/vinilo/sidecar"
# Chromium's SUID sandbox cannot work from a FUSE mount; Flatpak uses zypak
# for the same reason. Disable only inside the AppImage.
export ELECTRON_DISABLE_SANDBOX=1
exec "${APPDIR}/usr/bin/vinilo-desktop" "$@"
EOF
chmod +x "$APPRUN"

ICON_SRC="$APPDIR/usr/share/icons/hicolor/256x256/apps/$APPID.png"
[[ -f "$ICON_SRC" ]]

ELECTRON_BIN="$APPDIR/usr/share/vinilo/sidecar/node_modules/electron/dist/electron"

# One linuxdeploy pass: app binaries, GTK plugin, Electron deps, custom AppRun,
# then pack. A second pass for Electron alone used to race the gtk plugin.
# vinilo-desktop is a shell launcher already staged in the AppDir: linuxdeploy
# only accepts ELF files as --executable and aborts on anything else.
deploy=(
  --appdir "$APPDIR"
  --executable "$APPDIR/usr/bin/vinilo"
  --executable "$APPDIR/usr/bin/vinilod"
  --desktop-file "$APPDIR/usr/share/applications/$APPID.desktop"
  --icon-file "$ICON_SRC"
  --icon-filename "$APPID"
  --custom-apprun "$APPRUN"
  --plugin gtk
)
if [[ -x "$ELECTRON_BIN" ]]; then
  deploy+=(--executable "$ELECTRON_BIN")
fi

# The bundle holds two toolkits: GTK 4 for Vinilo and GTK 3 pulled in by the
# Electron sidecar. The gtk plugin refuses to guess, so name the app's own.
export DEPLOY_GTK_VERSION=4

"$TOOLS/linuxdeploy-${ARCH}.AppImage" "${deploy[@]}"

mkdir -p "$OUT_DIR"
OUTPUT="$OUT_DIR/Vinilo-${VERSION}-${ARCH}.AppImage"
rm -f "$OUTPUT"

# Tell the appimage plugin where to write, and embed the Cargo version.
export LINUXDEPLOY_OUTPUT_VERSION="$VERSION"
export LDAI_OUTPUT="$OUTPUT"

"$TOOLS/linuxdeploy-${ARCH}.AppImage" \
  --appdir "$APPDIR" \
  --output appimage

# Fall back if the plugin ignored LDAI_OUTPUT and wrote beside AppDir.
if [[ ! -s "$OUTPUT" ]]; then
  built=$(find "$(dirname "$APPDIR")" -maxdepth 1 -name '*.AppImage' -type f | head -1 || true)
  [[ -n "$built" && -s "$built" ]]
  mv "$built" "$OUTPUT"
fi

[[ -s "$OUTPUT" ]]
chmod +x "$OUTPUT"
ls -lh "$OUTPUT"
echo "AppImage ready at $OUTPUT"
