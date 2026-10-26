# pre-pr.sh

Purpose: one command for the checks AGENTS.md asks for before a pull request, run only for what the branch changed. Read-only; it never edits, commits or pushes.

Usage: `bash tools/dev/pre-pr.sh [--base REF] [--no-clippy] [--test] [--list]`

- Changed files = `merge-base(BASE, HEAD)..HEAD` plus uncommitted and untracked files (default BASE `origin/master`; run `git fetch` first).
- Always: no `<<<<<<<`/`>>>>>>>` markers, no UTF-8 BOM (Codex has written BOMs), a warning for whole-file rewrites, `bash -n` on changed `.sh`.
- Per changed crate (`crates/<name>/`): `cargo fmt -p <name> --check` (never `--all`: os error 206), `cargo clippy -p <name> --all-targets --locked -- -D warnings`, with `--test` also `cargo test -p <name> --locked` (`TEST_TIMEOUT` seconds, default 1500).
- When `Cargo.toml`/`Cargo.lock` changed: `cargo metadata --locked`. When `.rs` changed: `unsafe-audit.sh` and a warning if `crates/*/src` changed without `docs/PARITY.md`. When `.md`/`.rs` changed: `doc-links.py`, errors counted only in changed files. When skills or agents changed: `agents-check.sh`.
- Ends with a "NOT RUN" list to paste into the PR body (AGENTS.md: say which checks were not run). Exit 0 all passed, 1 a check failed, 2 usage error.

Env: `CARGO_TARGET_DIR` (default `<repo>/target-prepr`; delete it when done), `PY_BIN`. Selftest: `tools/dev/test/pre-pr-selftest.sh`.

Limits: clippy runs only for changed crates, so a break in a dependent crate (for example a new trait method that a fake in another crate must implement) shows up only in CI; run `cargo clippy --workspace --all-targets --locked -- -D warnings` for that.
