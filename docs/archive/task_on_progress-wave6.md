# task_on_progress archive: Wave 6 (platform)

Moved verbatim from `task_on_progress.md` (DOC-PROGRESS-TRIM, machine C). The 2026-10-18 State/Next steps snapshot and the Wave 6 entries.
The text below is historical: states, paths and next steps are those of the date in each heading, not current.
Current state: `../../task_on_progress.md`.

## State (verified, 2026-10-18)
- `origin/master` is `70a35d7` and includes Wave 6 contracts PR #27 (`70a35d7`, squash merge). Wave 5 PR #26 is merged; W5 remains Partial because native LibRaw/TurboJPEG DLL tests, RAW corpus tests, and part of the historical mutation coverage remain unverified.
- Wave 6 implementation is in progress on `codex/integration-wave-6`, based on `70a35d7`. Integrated W6-01a (`3984fa6`), W6-01b (`593b8f3`), W6-01c (`c00e18f`), W6-01d (`52c22da` plus follow-ups through `145f6ce`), W6-02a/b/c/d/e (`19adc8b`, `8117cde`, `24c63c0`, `4cecee0`, `8dc2c98`), W6-03a/b/c/d/e (`b31f517`, `013c5d0`, `f29835b`, `d085b2c`, `b757773`, `5e11290`), and W6-04a/b (`34c56a5`/`e65ecf2`); review fixes through `2ab03a9` now preserve loader messages, request Highest worker priority, and propagate non-missing-module loader errors per C# behavior. Lead-owned wiring exposes Explorer query, Recycle Bin candidate/restore backend, forwarding client/server, and instance scope. Root prefix reconciliation is `c471700`; it uses `PhotoReviewRs` for Rust mutex/pipe names and keeps `PhotoReview` as the C# canonical no-folder input per R0002. Individual checks are lane-local; no post-integration build has run because C: has about 320 MiB free. Full package and workspace gates remain required.
- W6-01b focused tests pass 18/18; W6-02b 26/26; W6-02a 17/17 (24/31 mutants caught; seven documented as native wrapper or Windows-normalization gaps); W6-03b 11 pass/2 native ignored; W6-04a 14/14; W6-04b 5 pass/1 native DWM ignored. Explorer COM tests pass 10/10. Package-wide tests passed with native/WIC cases ignored as declared. No native shell, DWM, ACL, or Recycle Bin runtime was verified, and no Recycle Bin operation touched user data.
- Mutation check for W6-01a was time-boxed and interrupted after 66/253 candidates (9 caught, 7 missed, 50 unviable); this is not a full mutation verdict. A later focused recheck caught all seven original survivors plus three additional mutants; one selected mutant was unviable and the run has no final `end_time`, so the broad scan remains incomplete. Several build targets and mutant output directories remain because the executor rejected verified recursive cleanup commands; do not retry command variants. No C# files were edited and no corpus was downloaded.
- W6-01d source/tests now cover the 25 contract scenarios across 32 fake test functions; only rustfmt/diff checks ran after later additions, with compile/test/mutation verdicts pending. W6-02c had 20 facade + 1 architecture test and focused Clippy pass before integration; W6-02e adds fake-only restore tests, static diff-check only. W6-03c had 16 focused tests pass + 1 native ignored before integration; W6-03d has fake outcome/SID/foreground error tests and ignored native pipe test, but no Cargo/rustfmt/compile verdict. W6-03e is now integrated; W6-04a/b and W6-05 are present. P-04/P-05 are updated to static-only pending integrated gates. Full workspace gates and the final Wave 6 PR are not ready until verification and review finish.
- Every active worktree uses its own `CARGO_TARGET_DIR`; cargo processes are checked before builds. Current C: free space was measured at about 320 MiB, so Cargo build/test/mutation work is paused because the volume cannot accommodate workspace outputs. The full `cargo fmt --all --check` gate passed after rustfmt normalization, committed through `2898cca`; no compile/runtime result is inferred. The integration branch is pushed through handoff commit `6e7d0f9` and has untracked mutation evidence directories that are being preserved. No C# files were edited, no corpus was downloaded, and no real Recycle Bin operation was used.

- R0001 Phase 0 UI spike (branch `spike/r0001`, feature `ui-spike`, bin `r0001-spike`), measured 2026-10-18, see the Decision in `docs/adr/R0001-ui-framework.md`: preloaded switch p50 5.3 ms windowed / 0.47 ms without swap chain, idle CPU 0; windowed p95 has ~510 ms `EndDraw` stalls on this laptop, so R0001 stays Proposed until re-measured attended. IME, 150% DPI, second monitor not tested.

## Next steps
1. Finish W6-01a's broad mutation adjudication and resume required gates when C: can hold build outputs; the seven old focused survivors are caught in the selected recheck.
2. Update W6 contract/PARITY evidence for any implementation-stage discovery and rerun all required workspace gates on the final Wave 6 head.
3. Open one Wave 6 PR against `master`, enable squash auto-merge, follow app CI events, and reconcile task status/worktrees/targets after merge.
4. Continue into Wave 7 and subsequent plan waves without pausing at the Wave 6 boundary; preserve W5 native/corpus evidence limits.

## Wave 6 contract PR update (2026-10-18)
- Wave 5 PR #26 merged into `origin/master` at `87c9bf4` while the Wave 6 contracts PR was being prepared.
- Wave 6 contracts PR #27 (`integration/wave-6-contracts`) has squash auto-merge enabled and is rebased onto `87c9bf4` at head `e070f54`. The base advance made the PR report `DIRTY`; the two conflicts were resolved by preserving merged Wave 5 progress/decision records and Wave 6 contract updates.
- Post-rebase gates passed: `cargo fmt --all --check`, workspace Clippy, workspace tests (4,847 passed, 0 failed, 420 ignored across 209 binaries), `tools/sync-from-csharp.ps1 -Check`, and `git diff --check`.
- PR #27 last observed state: OPEN/BLOCKED with auto-merge enabled; CI is left to the app monitor. The dedicated `target-wave6-contracts` directory remains for cleanup after PR completion because recursive deletion was rejected by the command safety policy.

## Wave 6 integration update (2026-10-18, W6-01a mutation tests)
- Added three focused COM interop assertions: the shell window iterator never requests an index beyond the reported count, `ShellComError` preserves and formats the HRESULT, and `OwnedUnknown` raw-pointer accessors preserve ownership transfer semantics.
- Focused validation: `cargo test -p pr-platform-win --test explorer_com_interop --locked` passed 12/12; `cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings`, `cargo fmt --all`, and `git diff --check` passed.
- Targeted Cargo Mutants rerun remains incomplete: 10 mutants caught and 1 compile-time unviable outcome were recorded before interruption; the output has no final `end_time`, so no full mutation verdict is claimed. The prior seven survivors are among the caught mutations in this rerun.

## Wave 6 integration update (2026-10-18, W6-02c and W6-03c)
- Integrated W6-02c (`24c63c0`) and W6-03c (`f29835b`) and wired `recycle_bin` and `forward_server` in `pr-platform-win`. Updated P-03/P-05 parity rows to identify the implemented fake-tested behavior and pending/native gaps.
- W6-02c lane evidence before integration: fmt check, 20 fake facade tests + 1 architecture test, focused Clippy passed. Mutation did not reach a verdict because Cargo Mutants stopped on OS error 112 while copying an artifact. No real Recycle Bin operation was used.
- W6-03c lane evidence before integration: fmt/diff-check, focused Clippy and package tests passed before the final named-pipe path literal correction; after correction the focused suite passed 16 tests with 1 native disposal test ignored. No native named-pipe runtime claim.
- After both cherry-picks, C: free space measured about 195 MB. Cargo/rustc/linker process check was empty; builds/tests/mutation work are paused until storage is available. Workspace integration gates have not been run on this combined head.
- Wave 6 contracts PR #27 is merged in `origin/master` (`70a35d7`); the older note above describing it as OPEN/BLOCKED is superseded by this state update.

## Wave 6 integration update (2026-10-18, W6-01d, W6-02e, W6-03d)
- Integrated W6-02e (`8dc2c98`), W6-03d (`d085b2c`, `b757773`), and W6-01d (`52c22da`, `4be55f4`, `e5d13f4`, `252f744`, `08731b6`, `145f6ce`). Added lead-owned module wiring for Explorer native view, Recycle Bin candidate/COM/restore, and forwarding client.
- W6-01d fake-test source covers all contract scenarios in 32 test functions; latest lane static checks were rustfmt and `git diff --check`. Cargo tests, Clippy, and mutations were not run after its latest edits.
- W6-02e has fake-only candidate/disposal/proof tests and `git diff --cached --check`; no Cargo, rustfmt, compile, tests, mutation, or native COM was run. Restore invokes one canonical `undelete` route and tests never touch a user's bin.
- W6-03d includes fake outcome, SID, foreground-best-effort cases and an ignored unique-pipe native test; `git diff --cached --check` passed, but Cargo/rustfmt/compile/tests/Clippy were not run.
- C: free space remains critically low (last measured about 127 MB) and no Cargo/rustc/linker process was running at that check. No workspace integration gate has run on the combined head. Do not run Cargo until storage is available.

## Wave 6 integration update (2026-10-18, W6-03e and R0002 prefix reconciliation)
- Cherry-picked W6-03e instance-scope ownership implementation (`0767c53`) into integration as `5e11290`; source provides fake/unit tests for ownership claims, pending opens, release, and stop-listener coordination.
- Reconciled W6-03a and W6-03e default kernel-object names with AGENTS.md and ADR R0002: Rust mutex/forward pipe defaults use `PhotoReviewRs`; the no-folder canonical hash input stays `PhotoReview` to match the C# oracle. Updated scope expectations, contract W6-03a, P-04/P-05 and D-13 parity records; exposed `instance_scope` from the Windows crate root.
- Static validation only: `git diff --check` passed. Cargo/rustfmt/compile/tests/clippy/mutation have not run on this combined head due to critically low C: free space (about 119 MiB at check); no native ownership or named-pipe behavior is claimed.

## Wave 6 static review follow-up (2026-10-18)
- Independent W6-01 review found that `ExplorerNativeVtable::query_service` and `query_active_shell_view` leaked a non-null owned COM output when HRESULT failed. Both now release that reference before returning, with fake-refcount tests for each path. The production selector also now treats every `ShellComError` from `LocationURL` as an unavailable window, matching C#'s silent skip for COM/invalid-cast exceptions.
- Independent W6-03 review found that a current-SID DACL alone does not reject remote clients authenticated as the same domain user. Native pipe creation now includes `PIPE_REJECT_REMOTE_CLIENTS`, with an assertion on the mode helper and an explicit contract rule. The same review found blank folder input was treated as another owner; the Core ownership API now has `FolderOwnershipError::InvalidFolder`, and the scope rejects whitespace input with a C#-twin test.
- Independent W6-02 review found a narrow C# parity gap for stale permanent-delete paths: `File.Delete` succeeds when missing, while Rust `remove_file` returned NotFound. The production adapter now maps only NotFound to success, with fake error-preservation tests and a contract update.
- These are source changes pending rustfmt/diff static checks; no Cargo/build/test/clippy/native checks due C: disk constraints. W6-03 remote rejection has no live cross-host test and no Recycle Bin operation was used.

## Wave 6 static review follow-up (2026-10-18, W6-04/05)
- Independent review found W6-04 import errors lost their loader message, and the worker did not request C# `ThreadPriority.Highest`. `NativeImportError` now carries the OS message into the fallback log; Windows workers attempt Highest and log if the OS refuses the setting. The fake display-clock test now checks both exception type and original message.
- W6-05 DWM/natural-comparer DLL loading previously panicked for loader errors other than `ERROR_MOD_NOT_FOUND`. Both now fail closed/fall back for any `LoadLibraryW` error; contracts and PARITY were aligned.
- Static `rustfmt --check` and `git diff --check` passed after these edits; no Cargo/build/test/clippy/native checks due C: space constraint. The display clock and platform primitives remain without a post-integration compile verdict.

## Wave 6 static audit correction (2026-10-18, W6-05)
- Independent W6 audit found natural comparer and dark title bar were swallowing non-missing-module `LoadLibraryW` errors, unlike the C# catches. Both now fall back only for `ERROR_MOD_NOT_FOUND`; other loader failures propagate. Added focused error-code classification tests and restored narrow behavior in W6-05 contract/PARITY.
- `rustfmt --check` and `git diff --check` passed. No Cargo/build/test/clippy/native check was run on this correction due to C: free space; tests are source-only until the required gates can run.

## Wave 6 follow-up (2026-10-18, independent W6-05 audit and disk cleanup)
- Independent static re-review of `2ab03a9` found no new W6-05 issue. It confirmed `windows` 0.62.2 maps `LoadLibraryW` failures through `Error::from_thread()` / `HRESULT_FROM_WIN32`, so checking the low 16 bits recognizes `ERROR_MOD_NOT_FOUND`; the focused tests use APIs present in the locked windows crate. No Cargo compile/test was run.
- C: has about 115 MiB free and no Cargo/rustc process was running. PR #24 (Wave 5 integration) and PR #26 (Wave 5 continuation) are merged; `integ-w5` is clean at `f1e5053`. An attempt to remove only its verified stale `target` was rejected by automatic policy because the command used `Remove-Item -Recurse`; no files were removed and no command variant will be retried. The dirty primary checkout and current mutation outputs remain untouched.
- Wave 6 remains ineligible for PR until fmt/clippy/workspace tests and mutation triage receive required verdicts. The W6 integration branch remains pushed at `2ab03a9`; no Wave 7 changes are stacked on it.

## Wave 6 workspace formatting gate (2026-10-18)
- `cargo fmt --all --check` found unformatted W6-03 forwarding and W6-02 recycle files. Applied `cargo fmt --all`; the full formatting gate and `git diff --check` now pass. Changes are rustfmt-only layout and import ordering.
- No Clippy or workspace tests were attempted: C: remains under 1 GiB free and the stale merged Wave 5 target cleanup was blocked by automatic policy. Wave 6 remains unready for PR until the remaining required gates run.

## Latest heartbeat check (2026-10-18)
- `codex/integration-wave-6` is clean except the two preserved untracked mutation-output directories and remains synced to `origin/codex/integration-wave-6` at `6e7d0f9`; `origin/master` remains `70a35d7`.
- C: free space is about 320 MiB, with no Cargo/rustc process and no other filesystem volume available. The merged W5 integration target still exists; no cleanup retry was made after automatic review blocked recursive removal. This is still insufficient for safe workspace build/test outputs, so Clippy/tests and the Wave 6 PR remain pending. The primary checkout still has pre-existing dirty files and has not been changed.
