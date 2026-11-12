#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
#
# AppKit draws a window's traffic lights in the style of the SDK the executable was linked
# against: linked against 15, macOS 26 keeps the old flat lights instead of the Liquid Glass
# ones. The macOS 15 runners carry Xcode 26 beside their default, so the build selects it while
# still running on 15 (Homebrew's GTK, and so the app's minimum macOS, stays at 15).
#
# Usage: xcode_sdk.sh select          pick the newest Xcode 26+, export it to $GITHUB_ENV
#        xcode_sdk.sh check <binary>   fail unless <binary> was linked against SDK 26 or newer
set -euo pipefail

min=26

case "${1:-}" in
select)
  xcode=$(ls -d /Applications/Xcode_[0-9]*.app 2>/dev/null \
    | awk -F'[_.]' -v min="$min" '$2 >= min' | sort -V | tail -n 1)
  [[ -n "$xcode" ]] || { echo "no Xcode ${min}+ on this machine" >&2; exit 1; }
  developer="$xcode/Contents/Developer"
  sdk=$(DEVELOPER_DIR="$developer" xcrun --sdk macosx --show-sdk-path)
  echo "using $xcode ($(DEVELOPER_DIR="$developer" xcrun --sdk macosx --show-sdk-version))"
  if [[ -n "${GITHUB_ENV:-}" ]]; then
    { echo "DEVELOPER_DIR=$developer"; echo "SDKROOT=$sdk"; } >> "$GITHUB_ENV"
  else
    echo "export DEVELOPER_DIR=$developer SDKROOT=$sdk"
  fi
  ;;
check)
  bin="${2:?usage: xcode_sdk.sh check <binary>}"
  sdk=$(otool -l "$bin" | awk '/LC_BUILD_VERSION/ {found=1} found && $1 == "sdk" {print $2; exit}')
  [[ -n "$sdk" ]] || { echo "$bin: no LC_BUILD_VERSION" >&2; exit 1; }
  if (( ${sdk%%.*} < min )); then
    echo "$bin was linked against SDK $sdk; macOS draws old window controls below $min" >&2
    exit 1
  fi
  echo "$bin: SDK $sdk"
  ;;
*)
  echo "usage: xcode_sdk.sh select | check <binary>" >&2
  exit 2
  ;;
esac
