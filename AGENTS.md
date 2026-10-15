# Mandatory Rules for PhotoReviewRs (Rust port of PhotoReview)

This repo rewrites the C# PhotoReview (https://github.com/ConanKLOP2/PhotoReview) in Rust with **identical behaviour**.
The C# app keeps running and evolving in parallel. The C# repo is the **spec and oracle**: when behaviour is unclear,
the C# code at the baseline in [`docs/SYNC-LEDGER.md`](docs/SYNC-LEDGER.md) decides, not intuition.

## Priorities (same as the C# app)

1. **Minimize disk reads:** read each file once, reuse bytes, cache, read sequentially.
2. **Use RAM generously:** 32 GB machine, target at least 16 GB where useful; a folder under 16 GB is loaded entirely into RAM when safe.
3. **Review speed first:** photo switching, rendering and review actions beat everything else.

## Running next to the C# app (docs/adr/R0002, never break this)

- Data lives under `%LOCALAPPDATA%\PhotoReviewRs` (`pr_core::identity`). Never read or write the C# app's journal,
  sessions, cache or log. The only C# file ever read is `config.json`, once, read-only, for the first-run import.
- Mutex/pipe prefix `PhotoReviewRs`, exe `PhotoReviewRs.exe`, env vars `PHOTOREVIEW_RS_*`.
- Journal/session/config **formats** stay identical to C# (oracle tests diff them); only their location differs.

## Port workflow

- Every ported unit names its C# source in its module doc (`Port of PhotoReview.Core/AppPaths.cs`).
- Update its row in [`docs/PARITY.md`](docs/PARITY.md) in the same PR (status + the test that proves it).
- An intentional behaviour difference needs an ADR in `docs/adr/` and a `Different` row in PARITY.md.
- Assets under `assets/languages/` and `tests/fixtures/` are synced from C#: never hand-edit; run
  `tools/sync-from-csharp.ps1` and log the new baseline in `docs/SYNC-LEDGER.md`.
- Never edit, commit or check out branches in the C# repo from here. Read it with `git -C ../PhotoReview show <sha>:<path>`.

## Maintain work status

- `task_on_progress.md` at the root: read at start, update after substantial changes and before handoff (date, state,
  next steps, test results; never unverified claims). Update when a documented PR is merged/closed.

## Coding conventions

- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`: zero warnings, CI enforces it.
- New `#[allow(...)]` needs a one-line justification comment.
- Every `unsafe` block has a `// SAFETY:` comment (FFI only: WIC/COM, Shell, TurboJPEG, LibRaw).
- `pr-core` must not depend on UI or Windows-only crates; Windows code lives in `pr-platform-win` and decoder crates.
- Paths: compare case-insensitively the way C# `OrdinalIgnoreCase` does; machine-read files (journal, log, CSV) use
  invariant formatting, never the user's locale. UI text comes from the language catalogs.
- Errors: no `unwrap()`/`expect()` in production paths that touch the file system or user data.

## Tests

- A test must fail when the guarded code is broken (mutate to check; `cargo-mutants` for journal/file-action code).
- No fixed-delay asserts or sleep polling. Real-OS tests are `#[ignore]`d with a `native`/`slow` reason and clean up after themselves.
- Test names mirror the C# test they port (`method_scenario_expected_result`), with a comment naming the C# test.
- **Hang guard:** never run tests unbounded; CI has job/step timeouts.
- **Recycle Bin:** never run code that deletes from, sweeps or empties the user's real Recycle Bin, including mutation
  checks of such code (C# 2026-09-24 incident). Use fakes; native bin tests only remove their own items.

## Git

- `master` is the only source of truth; work in `git worktree add .claude/worktrees/<name> -b <branch> origin/master`;
  never leave edits in the main checkout; no bare `git stash`; no stacked PRs.
- After each completed change: commit and push a branch for review.
- A squash-merged branch head is NOT an ancestor of master (merge-base check says no). Confirm the content is in master (`git diff <head> origin/master` shows only unrelated files), then delete branch and worktree.
- Worktrees: every worktree builds in its own cargo target dir (`tools/wave.ps1` does it); a shared one hands one worktree another one's stale artifacts.

## Pull requests: auto-fix and auto-merge (do this every time, without being asked)

- Open ONE PR per wave/change (base `master`, never stacked) with `gh pr create`, then immediately call
  `ccd_pr get_status` (bind with `bind_pr` if it is not bound). Auto-fix must be on (`set_monitor auto_fix=true`);
  never poll CI yourself, the app sends `<ci-monitor-event>`; fix, verify, commit, push on each event.
- Auto-merge: the user's standing request is "auto-fix + auto-merge" for PRs of this repo. Call `set_auto_merge
  enabled=true merge_method=squash` as soon as the PR is bound. If GitHub answers "clean status" (all checks already
  green, nothing to wait for), merge it with `gh pr merge <n> --squash`. If the permission classifier blocks either,
  stop and tell the user the exact command and the settings rule needed (`Bash(gh pr merge:*)`); do not retry
  variants.
- A PR is not "done" until it is merged. After the merge: delete the remote and local branch, remove the worktrees
  and the `target-*` dirs it used (check by diff, AGENTS.md "Git" above), update `task_on_progress.md`.
- Before opening the PR: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked` on the final head; say in the PR body which of these were NOT run.
- CI runs on `windows-latest`: temp paths can be 8.3 short names (`RUNNER~1`); never compare a path string built from
  `TempRoot` with one the code under test printed, compare a stable tail.
- Toolchain: Rust is pinned in `rust-toolchain.toml`; `cargo` lives in `%USERPROFILE%\.cargoin` and may not be on
  PATH in the Bash tool. Run `tools/*.ps1` with `powershell -NoProfile -ExecutionPolicy Bypass -File`, from the
  PowerShell tool (the Bash `tar` breaks `oracle.ps1`).

## Disk hygiene (C: filled up once: 33 GB of per-worktree debug builds)

- Build with `CARGO_PROFILE_DEV_DEBUG=line-tables-only` and `CARGO_INCREMENTAL=0` in agent worktrees; one
  `CARGO_TARGET_DIR` per worktree, deleted as soon as that unit's verdict is in. At most 8 live target dirs.
- After a PR merges: remove its worktrees, branches and `target-wave-*` dirs. Check free space (`Get-PSDrive C`) at
  every loop tick; below 20 GB, delete target dirs idle for 2 hours (never one a cargo process is writing).

## Asking the user to decide

Never ask a bare question: give the current state and why it matters, 2-4 options with detailed pros/cons, one
recommendation with the reason, and what you will do after the choice. Reply in Vietnamese. Record the outcome as an ADR.
