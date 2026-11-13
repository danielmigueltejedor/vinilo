// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// A version as releases are tagged: `v0.2.0`, `0.2.1-rc.1`. Only what is needed to say which of
/// two is newer.
public struct UpdateVersion: Comparable, Sendable {
    public let parts: [Int]
    public let prerelease: Bool

    public init(_ text: String) {
        var rest = Substring(text.trimmingCharacters(in: .whitespacesAndNewlines))
        if rest.hasPrefix("v") || rest.hasPrefix("V") { rest = rest.dropFirst() }
        // Build metadata says nothing about order.
        let core = rest.split(separator: "+", maxSplits: 1, omittingEmptySubsequences: false).first ?? ""
        let halves = core.split(separator: "-", maxSplits: 1, omittingEmptySubsequences: false)
        parts = (halves.first ?? "").split(separator: ".").map { Int($0) ?? 0 }
        prerelease = halves.count > 1
    }

    private func part(_ index: Int) -> Int {
        index < parts.count ? parts[index] : 0
    }

    public static func < (a: UpdateVersion, b: UpdateVersion) -> Bool {
        for index in 0..<max(a.parts.count, b.parts.count) where a.part(index) != b.part(index) {
            return a.part(index) < b.part(index)
        }
        // 1.0.0-rc.1 comes before 1.0.0.
        return a.prerelease && !b.prerelease
    }

    public static func == (a: UpdateVersion, b: UpdateVersion) -> Bool {
        !(a < b) && !(b < a)
    }
}

/// A published version with a disk image for this edition.
public struct UpdateRelease: Equatable, Sendable {
    public let version: String
    public let page: URL
    public let notes: String
    public let packageName: String
    public let package: URL
    /// The SHA-256 GitHub publishes for the disk image, in lower-case hex. Without it the app only
    /// links to the download: nothing is installed that could not be checked.
    public let sha256: String?
}

public enum UpdateFeed {
    /// Vinilo's releases, newest first.
    public static let url = URL(string: "https://api.github.com/repos/getnodalia/vinilo/releases?per_page=20")

    /// The end of Vinilo Native's disk image name on this Mac, or nil where none is published.
    /// The release carries one image per edition; this edition must never take the other's.
    public static var packageSuffix: String? {
        #if arch(arm64)
            return "-native-macos-arm64.dmg"
        #else
            return nil
        #endif
    }

    /// The newest release after `current` that has this edition's disk image. Drafts and
    /// pre-releases are skipped, and so is a release whose image has not been uploaded yet, which
    /// is why this reads the list and not "latest".
    public static func newest(in data: Data, after current: String, packageSuffix: String) -> UpdateRelease? {
        guard let list = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { return nil }
        let installed = UpdateVersion(current)
        var best: (UpdateVersion, UpdateRelease)?
        for entry in list {
            if entry["draft"] as? Bool == true || entry["prerelease"] as? Bool == true { continue }
            guard let tag = entry["tag_name"] as? String else { continue }
            let version = UpdateVersion(tag)
            guard !version.prerelease, version > installed else { continue }
            if let found = best, found.0 >= version { continue }
            guard let page = (entry["html_url"] as? String).flatMap(URL.init(string:)), trusted(page) else { continue }
            let assets = entry["assets"] as? [[String: Any]] ?? []
            for asset in assets {
                guard let name = asset["name"] as? String, name.lowercased().hasSuffix(packageSuffix),
                    let package = (asset["browser_download_url"] as? String).flatMap(URL.init(string:)),
                    trusted(package)
                else { continue }
                let shown = tag.hasPrefix("v") || tag.hasPrefix("V") ? String(tag.dropFirst()) : tag
                let release = UpdateRelease(
                    version: shown, page: page, notes: entry["body"] as? String ?? "", packageName: name,
                    package: package, sha256: digest(asset["digest"] as? String))
                best = (version, release)
                break
            }
        }
        return best?.1
    }

    /// Only GitHub's own pages and downloads: the list is data from the network.
    static func trusted(_ url: URL) -> Bool {
        url.scheme == "https" && url.host == "github.com"
    }

    /// `sha256:<64 hex digits>` as GitHub writes it, or nil for anything else.
    static func digest(_ text: String?) -> String? {
        guard let text, text.hasPrefix("sha256:") else { return nil }
        let hex = text.dropFirst("sha256:".count).lowercased()
        guard hex.count == 64, hex.allSatisfy({ $0.isHexDigit }) else { return nil }
        return hex
    }
}

/// What the install helper wrote when it finished, read at the next start.
public enum UpdateOutcome: Equatable, Sendable {
    case installed
    case stillRunning, mount, contents, copy, signature, replace, launch, other

    public init(result: String) {
        switch result.trimmingCharacters(in: .whitespacesAndNewlines) {
        case "ok": self = .installed
        case "still-running": self = .stillRunning
        case "mount": self = .mount
        case "contents": self = .contents
        case "copy": self = .copy
        case "signature": self = .signature
        case "replace": self = .replace
        case "launch": self = .launch
        default: self = .other
        }
    }
}

public enum UpdateHelper {
    public static let bundleName = "Vinilo Native.app"

    /// The script that swaps the app once it has quit. It is the helper the other Nodalia apps
    /// use (nodalia-update, where it is tested against a real disk image), with this app's bundle
    /// name: it waits for the app to exit, hashes a private copy of the disk image, mounts it,
    /// checks the copy's signature and publisher, swaps the bundle and opens it, and puts the old
    /// one back if the new one does not stay running.
    /// Arguments: disk image, mount point, bundle, the app's pid, log, result file, SHA-256.
    public static let script = #"""
#!/bin/bash
DMG=$1
MOUNT=$2
BUNDLE=$3
PARENT=$4
LOG=$5
RESULT=$6
EXPECTED=$7
STAGE=
exec >>"$LOG" 2>&1
unset NODALIA_UPDATE_OPEN NODALIA_UPDATE_WAIT_STEPS NODALIA_UPDATE_GRACE_STEPS NODALIA_UPDATE_SKIP_SIGNATURE
echo "== $(date '+%Y-%m-%d %H:%M:%S') update helper: $DMG -> $BUNDLE"
case "$BUNDLE" in
  /*/"Vinilo Native.app") ;;
  *) echo "refusing unexpected bundle path"; printf 'internal\n' >"$RESULT"; exit 2 ;;
esac
NEW="$BUNDLE.nodalia-new"
OLD="$BUNDLE.nodalia-backup"
OPEN="${NODALIA_UPDATE_OPEN:-/usr/bin/open}"

fail() {
  echo "FAILED at: $1"
  printf '%s\n' "$1" >"$RESULT"
  /usr/bin/hdiutil detach "$MOUNT" -force >/dev/null 2>&1
  [ -d "$NEW" ] && /bin/rm -rf "$NEW"
  [ -n "$STAGE" ] && /bin/rm -rf "$STAGE"
  if [ ! -d "$BUNDLE" ] && [ -d "$OLD" ]; then
    if ! /bin/mv "$OLD" "$BUNDLE"; then
      # Reopening a bundle that is not there would show nothing and hide why.
      echo "COULD NOT PUT THE PREVIOUS VERSION BACK. It is at: $OLD"
      echo "To recover, move it to $BUNDLE."
      exit 1
    fi
  fi
  "$OPEN" "$BUNDLE"
  exit 1
}

running() {
  /bin/ps -axo command= | /usr/bin/awk -v p="$1" 'index($0, p) == 1 { found = 1 } END { exit !found }'
}

# Who signed an app, as codesign prints it: the identifier and the team, nothing else.
identity() {
  /usr/bin/codesign -dv --verbose=4 "$1" 2>&1 | /usr/bin/grep -E '^(Identifier|TeamIdentifier)=' | /usr/bin/sort
}

# The application must be gone before its files are replaced.
steps="${NODALIA_UPDATE_WAIT_STEPS:-120}"
while [ "$steps" -gt 0 ] && kill -0 "$PARENT" 2>/dev/null; do
  sleep 0.5
  steps=$((steps - 1))
done
if kill -0 "$PARENT" 2>/dev/null; then
  echo "Vinilo (pid $PARENT) is still running: nothing was changed"
  printf 'still-running\n' >"$RESULT"
  exit 1
fi

/usr/bin/hdiutil detach "$MOUNT" -force >/dev/null 2>&1
/bin/mkdir -p "$MOUNT" || fail mount
# A private copy (mktemp makes it owner-only), hashed and then mounted: there is no gap between
# the check and the use for another process to put a different image into.
STAGE=$(/usr/bin/mktemp -d "${TMPDIR:-/tmp}/nodalia-update.XXXXXX") || fail copy
/bin/cp "$DMG" "$STAGE/update.dmg" || fail copy
ACTUAL=$(/usr/bin/shasum -a 256 "$STAGE/update.dmg" | /usr/bin/awk '{print $1}')
if [ -z "$EXPECTED" ] || [ "$ACTUAL" != "$EXPECTED" ]; then
  echo "the disk image does not match the digest that was verified"
  fail contents
fi
/usr/bin/hdiutil attach "$STAGE/update.dmg" -nobrowse -readonly -mountpoint "$MOUNT" || fail mount
SOURCE="$MOUNT/Vinilo Native.app"
[ -d "$SOURCE" ] || fail contents

[ -e "$NEW" ] && /bin/rm -rf "$NEW"
[ -e "$OLD" ] && /bin/rm -rf "$OLD"
/usr/bin/ditto "$SOURCE" "$NEW" || fail copy
/usr/bin/hdiutil detach "$MOUNT" -force >/dev/null 2>&1
/bin/rm -rf "$STAGE"
STAGE=

# The copy that is about to be installed is the one that is checked, so nothing that changed the
# disk image after the app looked at it can get through: valid signature, same publisher.
if [ -z "$NODALIA_UPDATE_SKIP_SIGNATURE" ]; then
  # Not the strict mode: it refuses the Finder metadata every file from a disk image carries, which
  # `ditto` keeps even with --noextattr, and which is not part of the signature.
  /usr/bin/codesign --verify --deep "$NEW" || fail signature
  NEW_ID=$(identity "$NEW")
  OLD_ID=$(identity "$BUNDLE")
  if [ -z "$NEW_ID" ] || [ "$NEW_ID" != "$OLD_ID" ]; then
    echo "the new app is signed by a different publisher"
    fail signature
  fi
fi

/bin/mv "$BUNDLE" "$OLD" || fail replace
/bin/mv "$NEW" "$BUNDLE" || fail replace
if ! "$OPEN" "$BUNDLE"; then
  # Installed, but it would not start: the previous version stays beside it for recovery.
  echo "updated, but $BUNDLE could not be started. The previous version is at: $OLD"
  printf 'launch\n' >"$RESULT"
  exit 1
fi

# `open` returning only says the launch was handed over. The backup is kept until the new app has
# stayed up for a few seconds; if it is gone by then it crashed on start, and the old one goes back.
steps="${NODALIA_UPDATE_GRACE_STEPS:-10}"
while [ "$steps" -gt 0 ]; do
  sleep 0.5
  steps=$((steps - 1))
done
if running "$BUNDLE/Contents/MacOS/"; then
  /bin/rm -rf "$OLD"
  /bin/rm -f "$DMG"
  printf 'ok\n' >"$RESULT"
  echo "updated; started $BUNDLE"
  /bin/rm -f "$0"
else
  echo "the new $BUNDLE did not stay running; putting the previous version back"
  /bin/rm -rf "$BUNDLE"
  # Only put the backup back into an empty place: `mv` into a directory that is still there would
  # tuck it inside and report success.
  if [ ! -e "$BUNDLE" ] && /bin/mv "$OLD" "$BUNDLE"; then
    printf 'launch\n' >"$RESULT"
    "$OPEN" "$BUNDLE"
  else
    echo "COULD NOT PUT THE PREVIOUS VERSION BACK. It is at: $OLD"
    echo "To recover, remove what is left of $BUNDLE and move it there."
    printf 'replace\n' >"$RESULT"
  fi
  exit 1
fi
"""#
}
