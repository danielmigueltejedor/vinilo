# Deterministic Quality Gates

AI agents must supply actual tool output, write regression tests for changed behavior, and never weaken tests, quality thresholds, or CI to force a pass. Formatting and Cargo lock integrity are mandatory; the repository's existing CI remains responsible for builds and tests unless extended explicitly.

Machine-enforced by the `quality` job:
- Git patch whitespace integrity
- `cargo fmt --all -- --check`
- `cargo metadata --locked --no-deps --format-version 1`

**This does not enforce a numerical coverage or mutation score.** Those need separately measured baselines and instrumented tools. Make `quality` a required status check in the branch ruleset and require human review of CI/policy changes.
