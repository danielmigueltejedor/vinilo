#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:-dist/root}"
APPID="dev.danielmiguelt.Vinilo"
AGUJA="dev.danielmiguelt.Aguja"

test -x target/release/vinilo
test -x target/release/vinilod
test -x target/release/aguja
test -d sidecar/node_modules

rm -rf "$ROOT"

install -Dm755 target/release/vinilo "$ROOT/usr/bin/vinilo"
install -Dm755 target/release/vinilod "$ROOT/usr/bin/vinilod"
install -Dm755 target/release/aguja "$ROOT/usr/bin/aguja"
install -Dm755 data/vinilo-desktop "$ROOT/usr/bin/vinilo-desktop"

install -d "$ROOT/usr/share/applications"
sed "s|@BINDIR@|/usr/bin|g" "data/$APPID.desktop" > "$ROOT/usr/share/applications/$APPID.desktop"
sed "s|@BINDIR@|/usr/bin|g" "data/$AGUJA.desktop" > "$ROOT/usr/share/applications/$AGUJA.desktop"

install -d "$ROOT/usr/lib/systemd/user"
sed "s|@BINDIR@|/usr/bin|g" packaging/systemd/vinilod.service > "$ROOT/usr/lib/systemd/user/vinilod.service"

install -d "$ROOT/usr/share/dbus-1/services"
sed "s|@BINDIR@|/usr/bin|g" "packaging/dbus/$APPID.service" > "$ROOT/usr/share/dbus-1/services/$APPID.service"

for size in 16 32 48 64 128 256 512; do
    install -Dm644 "data/icons/hicolor/${size}x${size}/apps/$APPID.png" "$ROOT/usr/share/icons/hicolor/${size}x${size}/apps/$APPID.png"
    install -Dm644 "data/icons/hicolor/${size}x${size}/apps/$AGUJA.png" "$ROOT/usr/share/icons/hicolor/${size}x${size}/apps/$AGUJA.png"
done

install -Dm644 "data/icons/hicolor/symbolic/apps/$APPID-symbolic.svg" "$ROOT/usr/share/icons/hicolor/symbolic/apps/$APPID-symbolic.svg"

for icon in data/icons/hicolor/symbolic/actions/*.svg; do
    install -Dm644 "$icon" "$ROOT/usr/share/icons/hicolor/symbolic/actions/$(basename "$icon")"
done

install -d "$ROOT/usr/share/vinilo/sidecar"
cp -a sidecar/package.json sidecar/main.js sidecar/preload.js sidecar/queue-identity.js sidecar/node_modules "$ROOT/usr/share/vinilo/sidecar/"

install -Dm644 COPYING "$ROOT/usr/share/licenses/vinilo/COPYING"
install -Dm644 README.md "$ROOT/usr/share/doc/vinilo/README.md"

echo "Release tree ready at $ROOT"
