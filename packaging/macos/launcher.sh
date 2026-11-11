#!/bin/sh
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The bundle's executable. GTK looks for its schemas, icons and image loaders
# under a Unix prefix; inside Vinilo.app that prefix is Contents/Resources, so
# point it there before handing over to the real binary.

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
res="$here/../Resources"

export XDG_DATA_DIRS="$res/share"
export GSETTINGS_SCHEMA_DIR="$res/share/glib-2.0/schemas"
# The loader cache holds absolute paths, and the bundle can live anywhere, so
# it ships with a placeholder that is filled in for wherever it is now.
cache="${HOME}/Library/Caches/dev.danielmiguelt.Vinilo"
mkdir -p "$cache" 2>/dev/null || true
if [ -f "$res/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache.in" ]; then
    sed "s|@RESOURCES@|$res|g" "$res/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache.in" > "$cache/loaders.cache"
    export GDK_PIXBUF_MODULE_FILE="$cache/loaders.cache"
fi
export GDK_PIXBUF_MODULEDIR="$res/lib/gdk-pixbuf-2.0/2.10.0/loaders"
export GTK_PATH="$res/lib/gtk-4.0"
# Finder starts apps with a bare PATH; yt-dlp and ffmpeg from Homebrew are
# what the YouTube Music, Tidal and URL sources play through.
export PATH="$PATH:/opt/homebrew/bin:/usr/local/bin"

exec "$here/vinilo" "$@"
