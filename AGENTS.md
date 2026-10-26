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
- The file is long (about 79 KB, about 20k tokens): at the start read only its "How to resume" section and the newest
  status section (find them with `Grep` on `^## `), not the whole file. This refines the "read at start" rule above and
  the line in `CLAUDE.md`.

## Coding conventions

- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`: zero warnings, CI enforces it.
- New `#[allow(...)]` needs a one-line justification comment.
- Every `unsafe` block has a `// SAFETY:` comment (FFI only: WIC/COM, Shell, TurboJPEG, LibRaw, and the test-only Win32 ProcessStatus helper).
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

## Who does what: Codex first

- Read docs/LESSONS.md before launching Codex lanes: it lists the traps that already cost hours.
- Give every task to Codex CLI first: implementation, tests, failure repair (including `needs-claude` notes), mutant-killer tests, contracts, infrastructure/scripts, and docs. Use `tools/autopilot/codex-run.sh` or the pipeline.
- Claude only orchestrates: prepare the prompt, start Codex, inspect its diff, rerun fmt/clippy/tests before trusting its report, merge and open PRs, and handle work Codex cannot do (for example `git index.lock`, PR/CI/claim tools). Claude sub-agents are at most Sonnet.
- Claude writes code only after Codex fails twice for quota or tool reasons; state this in the PR.
- Keep Codex lanes busy within the 95% quota limit for the 5-hour window and week.
- A unit that fails the pipeline gate gets one automatic Codex fixup round (`FIXUP=1` by default; see `tools/autopilot/README.md`) before human or Claude triage.
- The Codex Windows sandbox writes only to its working directory: run Codex cargo builds with `CARGO_TARGET_DIR` inside the worktree and `cargo-mutants` with its target directory outside the worktree.

## Several machines

When more than one machine works on the port, read [`docs/MULTI-MACHINE.md`](docs/MULTI-MACHINE.md) first: set `PR_MACHINE`, claim every unit with 
`bash tools/autopilot/claim.sh claim <unit>` before the first edit (the pipeline does it for you), work only on the units of your machine, and keep 
the state of `tools/autopilot/state/` local.

## Pull requests: auto-fix and auto-merge (do this every time, without being asked)

- Open ONE PR per wave/change (base `master`, never stacked) with `gh pr create`, then immediately call
  `ccd_pr get_status` (bind with `bind_pr` if it is not bound). Auto-fix must be on (`set_monitor auto_fix=true`);
  never poll CI yourself, the app sends `<ci-monitor-event>`; fix, verify, commit, push on each event.
- Every PR, however small, gets auto-fix AND auto-merge in the same step that creates it (`gh pr create`, then
  `set_auto_merge` straight away): do not wait for the user to ask or to remind you. When the user asks for several
  small PRs (one per unit or dependency group), each one follows these rules and branches from `master`; a PR that
  needs another one first is opened only after that one has merged.
- Keep a PR's own CI evidence honest: if a check fails on a test that is not yours (flaky or a stale local path), fix the
  cause in its own PR (root cause first, see "Flaky tests" in docs/AGENT-HYGIENE of the C# repo), do not re-run until it passes.
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

## Build artifact hygiene

- Build with `CARGO_PROFILE_DEV_DEBUG=line-tables-only` and `CARGO_INCREMENTAL=0` in agent worktrees; one
  `CARGO_TARGET_DIR` per worktree, deleted as soon as that unit's verdict is in. No fixed cap on live target dirs (P-29).
- After a PR merges: remove its worktrees, branches and `target-wave-*` dirs.

## Model cap (P-34)
Agents and subagents use Sonnet 5.5 at medium effort or lower (Haiku for mechanical work). Never use Opus or another heavier model without asking the user first. Mutation testing in verify steps is time-boxed (about 15 minutes, only the unit's owned files).

## Mutation testing and parallel agents (learned in Wave 8; every agent follows this)

- **Cost:** every mutant rebuilds the crate (about 95% of its time: 50 s on an idle machine, 70-180 s with eight agents building). A 186-mutant file takes 4-9 hours. Plan for it.
- **Where to run it:** only on pure logic with real branching (policies, counters, geometry, state machines, parsers), and only on the risky functions with `-F '<regex>'` or `--in-diff`, never a whole large file. Skip native/FFI/composition glue (window hosts, COM objects, menus, drop targets, sinks); test those with native tests run by hand instead.
- **How:** `cargo mutants -p <crate> --baseline skip --timeout 90 -j 1 --file '**/<path>.rs' -F '<regex>' -- --test <file>`; always `-j 1` with its OWN `CARGO_TARGET_DIR` (a shared target dir with `-j 3` reports false MISSED results). Several independent processes on different files are fine, at most three at a time. Run mutation AFTER the port is done and merged, not while many agents are building.
- **Time-box and honesty:** about 25 minutes of mutants per unit; the PR states `tested / total, caught, missed, unviable, timeouts` and which mutants were not run. A partial run is never described as a full mutation verdict. A survivor is killed by a test or explained as equivalent in the PR.
- **Never kill what is not yours:** no `Stop-Process` on `cargo`, `rustc` or `cargo-mutants`, and no `cargo clean` or deleting a target dir that another agent or a running process uses. Stop only the process ids you started.
- **Tests:** no hard-coded repo-relative or build-directory paths (`../../target-*`, `CARGO_MANIFEST_DIR/..`, `C:\...`); use `pr_test_support::TempRoot`. Helper files under `tests/common/` get a name unique to the unit (`<unit>_fixture.rs`) so parallel agents do not collide on the same file.
- **Before a PR:** after merging `master` into the branch run `cargo clippy -p <crate> --all-targets --locked -- -D warnings` (a new trait method on master can break a fake in your tests), `cargo fmt -p <crate> --check` (CI formats per package) and `git grep -n -I '^<<<<<<<'`; check that your PARITY rows are still there after resolving conflicts (merge row by row, never take one side of the file).
- **Machine load:** with more than six agents building, use `-j 2` for clippy and tests and run long commands in the background; do not start a new agent when less than 6 GB of RAM is free.

## Asking the user to decide

Never ask a bare question: give the current state and why it matters, 2-4 options with detailed pros/cons, one
recommendation with the reason, and what you will do after the choice. Reply in Vietnamese. Record the outcome as an ADR.
