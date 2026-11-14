# Release notes

Every version has its notes here, written by hand: `docs/release-notes/vX.Y.Z.md`. They are the
text of the GitHub release, so people reading the release page see exactly this file.

## How a release happens

Bump `version` in `[workspace.package]` of `Cargo.toml` and add `docs/release-notes/vX.Y.Z.md` in
the same pull request. When it is merged, **Release from main** (`release-auto.yml`):

1. tags the merge `vX.Y.Z` and creates a draft release whose text is this file;
2. runs **Release bundles** (`release.yml`), which builds every package and attaches it;
3. publishes the release once every package is attached. If one fails, it stays a draft.

A version with no notes file is not released. Nobody pushes a tag by hand. The **Release notes are
written** check (`scripts/check-release-notes.sh`) fails a pull request whose notes are missing or
incomplete.

A fix for one platform only (a z version for the macOS editions, say) adds this line to its notes:

```markdown
<!-- platforms: macos -->
```

Then only the disk images are built, and the release is published without becoming the latest
one, so installed Linux copies and the website keep the newest release that has their package.

## What goes in

Write them from the merged pull requests, for someone deciding whether to update: what changed
for them, in their words. No generated lists ("What's Changed", "Full Changelog").
