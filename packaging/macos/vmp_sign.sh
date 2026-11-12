#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
#
# VMP-signs the castLabs Electron inside the app, so Apple Music plays in full on macOS:
# Apple's licence server answers an unsigned Widevine player with 90-second previews only.
# Uses castLabs' EVS service with the account in EVS_ACCOUNT_NAME / EVS_PASSWD. Without them
# it warns and leaves the bundle unsigned; Vinilo then does not offer Apple Music on macOS.
# Must run before the bundle is code-signed: the signature files live inside it. bundle.sh
# calls it at that point; run it by hand only on a bundle that has not been signed.
# Usage: vmp_sign.sh <edition .app>
set -euo pipefail

app="${1:?usage: vmp_sign.sh <edition .app>}"
dist="$app/Contents/Resources/sidecar/node_modules/electron/dist"
[[ -d "$dist/Electron.app" ]] || { echo "no castLabs Electron in $app" >&2; exit 1; }

if [[ -z "${EVS_ACCOUNT_NAME:-}" || -z "${EVS_PASSWD:-}" ]]; then
    echo "::warning::No castLabs EVS credentials: the player is not VMP-signed, so this build does not offer Apple Music on macOS."
    exit 0
fi

# The system Python refuses global installs (PEP 668): a throwaway environment for the tool.
venv="$(mktemp -d)/evs"
python3 -m venv "$venv"
"$venv/bin/pip" install --quiet --upgrade castlabs-evs
"$venv/bin/python" -m castlabs_evs.account reauth -A "$EVS_ACCOUNT_NAME" -P "$EVS_PASSWD"
"$venv/bin/python" -m castlabs_evs.vmp sign-pkg "$dist"
"$venv/bin/python" -m castlabs_evs.vmp verify-pkg "$dist"
