#!/usr/bin/env bash
# Fails unless the release notes of a version are written by hand and complete.
#
# Usage:
#   scripts/check-release-notes.sh             the version in Cargo.toml (a -dev version passes)
#   scripts/check-release-notes.sh vX.Y.Z      that version's notes
#   scripts/check-release-notes.sh --changed BASE
#       what a pull request or push changed since commit BASE: the new version when the change
#       bumps it to a release version, and every notes file it adds or edits. This is the check
#       the "Release notes are written" job runs.
#
# Why: a release must never go out with GitHub's generated list or a one-line stub. The notes of
# every version live in docs/release-notes/vX.Y.Z.md, in the format described in that folder's
# README, and Foundation's release-tag workflow publishes that file as the release body.
set -euo pipefail

# The packages' file prefix, as named in the downloads table (APP-X.Y.Z-…).
app=vinilo

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
self="$root/scripts/check-release-notes.sh"

# Reads [workspace.package] version (or [package] version) from a Cargo.toml on stdin.
cargo_version() {
  awk '
    /^\[/ { sec = $0; next }
    $1 == "version" && (sec == "[workspace.package]" || sec == "[package]") {
      v = $0; sub(/^[^=]*=[[:space:]]*"/, "", v); sub(/".*$/, "", v)
      if (sec == "[workspace.package]") { print v; found = 1; exit }
      if (pkg == "") pkg = v
    }
    END { if (!found && pkg != "") print pkg }'
}

if [[ "${1:-}" == "--changed" ]]; then
  base="${2:?--changed needs the base commit}"
  head_version="$(cargo_version <"$root/Cargo.toml")"
  base_version="$(git -C "$root" show "$base:Cargo.toml" 2>/dev/null | cargo_version || true)"
  tags=()
  if [[ -n "$head_version" && "$head_version" != "$base_version" && "$head_version" != *-dev* ]]; then
    tags+=("v$head_version")
  fi
  while IFS= read -r changed; do
    [[ -n "$changed" ]] && tags+=("$changed")
  done < <(git -C "$root" diff --name-only --diff-filter=AM "$base" HEAD -- docs/release-notes \
    | sed -n 's#^docs/release-notes/\(v[0-9][^/]*\)\.md$#\1#p')
  if [[ "${#tags[@]}" -eq 0 ]]; then
    echo "✓ No version bump and no notes changed: nothing to check."
    exit 0
  fi
  status=0
  for tag in $(printf '%s\n' "${tags[@]}" | sort -u); do
    "$self" "$tag" || status=1
  done
  exit "$status"
fi

tag="${1:-}"
if [[ -z "$tag" ]]; then
  version="$(cargo_version <"$root/Cargo.toml")"
  if [[ "$version" == *-dev* ]]; then
    echo "✓ Version $version is a development version: no notes needed."
    exit 0
  fi
  tag="v$version"
fi
if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
  echo "::error::'$tag' is not a version tag like v1.2.3" >&2
  exit 2
fi
version="${tag#v}"
file="$root/docs/release-notes/$tag.md"
rel="docs/release-notes/$tag.md"

fail=0
problem() { echo "::error file=$rel::$1" >&2; echo "✗ $1" >&2; fail=1; }

if [[ ! -f "$file" ]]; then
  echo "::error::Missing $rel. Every version needs hand-written release notes: see docs/release-notes/README.md" >&2
  exit 1
fi

# Summary: a Spanish line, then the same in English in italics.
# The first two non-blank lines, read without `mapfile`: the macOS runners ship bash 3.2.
first=""
second=""
# Windows runners check out with CRLF, so the carriage return is dropped too.
while IFS= read -r line || [[ -n "$line" ]]; do
  line="${line%$'\r'}"
  [[ -z "${line//[[:space:]]/}" ]] && continue
  if [[ -z "$first" ]]; then first="$line"; else second="$line"; break; fi
done < "$file"
[[ -n "$first" && "$first" != \#* && "$first" != \>* && "$first" != -* ]] \
  || problem "The first line must be a one-sentence summary in Spanish."
[[ "$second" =~ ^\*.+\*$ ]] \
  || problem "The second line must be the English summary, in italics (*...*)."

# Headings may be bold (## **Español**): older update windows look for that form.
for heading in 'Español' 'English' 'Descargas · Downloads'; do
  grep -qxE "## (\*\*)?$heading(\*\*)?" "$file" || problem "Missing the heading '## $heading'."
done

# Each language needs a What's new (or, for a fix-only release, Fixes) section with real bullets.
section_bullets() { # heading-regex
  awk -v re="$1" '
    $0 ~ re {on=1; next}
    on && /^##/ {exit}
    on && /^- / {n++}
    END {print n+0}' "$file"
}
# (A first release says "Qué incluye" / "What's in it" instead.)
[[ "$(section_bullets '^### (Novedades|Qué incluye|Correcciones)')" -ge 1 ]] \
  || problem "'### Novedades' (or '### Qué incluye', '### Correcciones') needs at least one bullet."
[[ "$(section_bullets "^### (What's new|What's in it|Fixes)")" -ge 1 ]] \
  || problem "'### What's new' (or '### What's in it', '### Fixes') needs at least one bullet."

# The downloads table names the real files of this version.
downloads="$(awk '/^## (\*\*)?Descargas/{on=1;next} on && /^## /{exit} on{print}' "$file")"
grep -q "$app-$version-" <<<"$downloads" \
  || problem "The downloads section must name the files of this version ($app-$version-…)."
grep -q 'SHA256SUMS' <<<"$downloads" \
  || problem "The downloads section must mention the SHA256SUMS files."

# Never raw generated notes, stubs or leftovers.
if grep -inE "^#+ what's changed|^\*\*full changelog\*\*:|generated by|lorem ipsum" "$file" >/dev/null \
  || grep -nE '\b(TODO|TBD)\b' "$file" >/dev/null; then
  problem "Contains generated text or a placeholder."
fi
if grep -inE '\bfork(s|ed)?\b|\bupstream\b|craft\b' "$file" >/dev/null; then
  problem "Mentions a fork, upstream or a Craft app: $(grep -inE '\bfork(s|ed)?\b|\bupstream\b|craft\b' "$file" | head -1)"
fi
[[ "$(wc -c <"$file")" -ge 600 ]] || problem "Too short to be curated notes (under 600 bytes)."

if [[ "$fail" -ne 0 ]]; then
  echo "Release notes for $tag are not ready. Format: docs/release-notes/README.md" >&2
  exit 1
fi
echo "✓ Release notes for $tag are complete."
