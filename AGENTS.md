# AGENTS.md

Project instructions for Codex. Read this file before working in the repository.

## Instruction sources

`CLAUDE.md` remains the canonical shared project guide. Read it in full before
starting any task and follow every rule as if it appeared in this file. Treat
references to Claude or Claude Code as references to Codex.

If `CLAUDE.local.md` exists, read it in full after `CLAUDE.md`. It contains
personal context and machine-specific guidance. Its instructions take
precedence when they conflict with the shared guide.

Keep these files as the source of truth. Do not copy their contents into this
file; parallel copies would drift and leave Claude Code and Codex following
different project rules.

## Repository-specific deterministic quality contract (2026-10-26)

**Project:** Rust GNOME media application with auxiliary sidecar.

**Engineering requirements:** Protect library scans, metadata parsing, player state and native desktop integration; do not test against user music files.

**Verification commands:** cargo fmt --all -- --check; cargo metadata --locked --no-deps --format-version 1; targeted cargo test.

Only claim checks passed after executing them. Every feature and fix should add or maintain tests for behavior, boundaries and failure cases. Never disable/weaken tests, suppress warnings or reduce coverage thresholds solely to pass CI. `AGENTS.md`, `QUALITY_GATES.md`, tests and workflows are review-sensitive. Require `quality` in branch protection. Disclose native dependencies and pre-existing CI failures. Preserve existing project rules.
