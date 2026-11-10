# Lessons from the parallel Codex port

Read this before launching lanes or cleaning up; apply it on every machine and cloud session.
Use repository evidence over recollection; recheck script defaults when they change.

## A. Codex runs

- Inspect `tools/autopilot/codex-run.sh` before choosing a model; set `CODEX_MODEL` and `CODEX_EFFORT` explicitly when needed.
  Why: its explicit `-m` and effort override config defaults; the checked script defaults to `gpt-6.1-sol` / `medium` (owner choice 2026-10-23; `gpt-6-luna` / `low` is no longer the default).
- Inspect live arguments with PowerShell `Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, CommandLine` before reporting a running model.
  Why: a script default or an agent's report does not establish the arguments of a live process.
- Launch long rounds in the main session's background runner and wait for its completion notice and exit status.
  Why: handing off before the child finishes leaves its lifetime and result unverified; keep the owning session alive.
- Keep Codex's `CARGO_TARGET_DIR` inside its worktree; run cargo-mutants from the lead with a target outside the source worktree.
  Why: `AGENTS.md` requires both; `tools/autopilot/README.md` records per-workspace sandbox SIDs and mutation tree copies.
- Let the lead run Git mutations and PR operations; check permissions before assigning other tools such as dotnet.
  Why: `tools/autopilot/CODEX-RULES.md` assigns commits to the orchestrator; tool access is not established by an agent's claim.
- Keep each resumed session ID with one lane and worktree.
  Why: `tools/autopilot/codex-pipeline.sh` resets `SID` per unit and resumes that unit's conversation.
- Size concurrent Codex lanes for the machine (AGENTS.md "Sizing parallel agents"; two is only the default) and stop at 95% of either quota window; check both machines' usage.
  Why: `docs/MULTI-MACHINE.md` records a shared account and both limits; the pipeline defaults to two jobs and enforces the 95% ceiling on the 5 h window only, `tools/dev/codex-sched.sh` (`MAX_LANES` default 2, `QUOTA_MAX=95`) enforces both windows.
- Inspect `git diff --stat`, encoding, line endings and remaining stubs, then rerun fmt, clippy and tests yourself.
  Why: `done` is a claim; `tools/autopilot/test/pipeline-selftest.sh` includes a gate check rejecting `todo!()`.

## B. Pipeline

- Verify that the integration PR exists; merge `origin/master`, resolve conflicts and rerun gates before opening a missing final PR.
  Why: `tools/autopilot/codex-pipeline.sh` attempts PR creation after push but only logs creation failure; failed/skipped units do not themselves suppress it.
- Check `gh pr list --head <branch> --state merged` and the content diff before declaring a branch unmerged or deleting it.
  Why: squash merging does not preserve branch-head ancestry; `AGENTS.md` requires a content check.
- Retain `FIXUP=1` and propagate `GATE_TEST_RELAXED=1` to every gate after an accepted test correction.
  Why: `fixup_once` sets `FIXUP_GUARD`; `gate_loop` forwards it, but the direct post-mutant fast-gate call does not.
- Expand unit ranges such as `W8-18l..18n` before counting or checking merged work.
  Why: `expand_unit_ids` in `tools/autopilot/lib/codex-lib.py` handles ranges and slash lists; token counting misses units.

## C. Safety and Windows

- Find processes by command line and stop only exact PIDs you started; never kill by image name.
  Why: `AGENTS.md` forbids stopping another lane's compiler, Cargo or mutation process.
- Treat a denied action as final for that agent; report the command and reason to the main session without retrying through another tool.
  Why: `task_on_progress.md` assigns permission-blocked actions to the main session; agents cannot grant themselves permissions.
- Use `cargo fmt -p <pkg>` when Windows rejects workspace formatting with os error 206.
  Why: `tools/autopilot/lib/fmtcheck.sh` also provides a batched fallback for long command lines.
- Pass explicit shared paths between Bash and Python; prefer repo-relative scratch paths within the allowed workspace.
  Why: a shell-specific `/tmp` spelling does not prove that another runtime resolves the same directory.
- Inspect worktree registrations, locks, dirty files and unique history before cleanup; unlock an abandoned `initializing` registration before removing it.
  Why: `tools/autopilot/codex-pipeline.sh` retains recovery refs during reset; never unlock a live worktree operation.
- Resolve and verify the absolute cleanup target before deep-path removal; preserve recovery evidence and use the permitted native filesystem tool.
  Why: `tools/autopilot/README.md` documents Windows long paths; a path-length failure is not permission to widen deletion.
- Write local permission settings as valid UTF-8 without BOM and verify that the host loads their rules.
  Why: `task_on_progress.md` relies on that local file for permissions; parsing success alone does not prove host acceptance.
- Run PowerShell scripts with `powershell -NoProfile -ExecutionPolicy Bypass -File`.
  Why: `tools/autopilot/README.md` records the execution-policy requirement and PowerShell 5.1 shell differences.
- Parse command structure in hooks and guards; distinguish executable arguments from quoted commit-message text.
  Why: matching text such as `cargo fmt --all` anywhere can reject a message that merely discusses the command.

## D. Verification

- Reconcile agent reports against local pipeline state, `gh pr list --state merged`, `git log` and the final diff.
  Why: state is machine-local, squash changes ancestry, and commit subjects can describe several units or documentation only.
- Preserve failures in `crates/pr-app/tests/misc_app_log.rs` and `crates/pr-imaging/tests/cache_preview_file.rs`; reproduce alone to diagnose load effects.
  Why: both binaries exist, but their alleged timing flakiness is not established by the sources; fix the cause rather than rerun until green.
- Run ignored native tests explicitly in a desktop session and report them separately from ordinary CI.
  Why: `#[ignore]` tests are skipped by default; `crates/pr-platform-win/tests/shell_overlay_tooltips.rs` marks desktop requirements.
- Use an interactive desktop for cursor-dependent tooltip tests in `crates/pr-platform-win/tests/dialogs_native_dialog_events_native.rs`.
  Why: its `cursor_over_first_row` helper requires `SetCursorPos` to succeed; a headless result is not desktop verification.
- Correct a test only when the C# oracle proves it wrong; cite the exact source/test and changed Rust functions.
  Why: the pipeline's fixup prompt requires those citations and forbids weakening tests or adding ignores.

## E. Cost

- Start with "How to resume" and the newest dated status in `task_on_progress.md`; expand into older sections only for relevant details.
  Why: that file sends historical entries to `docs/archive/`; `CLAUDE.md` requires reading status but does not prescribe a shortened reading mode.

## F. Mutation sweeps (sessions 2026-11-04 to 2026-11-06)

- Count an infrastructure failure (build died, paging file too small, LNK1102, E0463, a log overwritten by a sibling mutant) as NOT RUN, never as tested or unviable.
  Why: the P3 verifier rejected `pixels.rs` "tested 8, caught 6" because 2 build failures were counted as tested; `tested = caught + missed + unviable + timeout` must hold.
- Treat "proven equivalent" as a claim until the run covers the whole domain or every input class; a lattice of sampled exponents and mantissas is run-supported, open.
  Why: N2 found the M1 equivalence claims for `layout.rs` 361:12 and 349:15 wrong (overflow near f64::MAX), and the P4 verifier did not accept class-complete lattices as closure (P-395).
- Never write a wait loop whose success pattern differs in case from the real output; give every `until grep` a timeout and a fallback read.
  Why: P2 waited on `NOT run` while `pre-pr.sh` printed `NOT RUN`, so the loop could not match and idled until its 595 s timeout.
- Open the evidence PRs first and the PARITY/PROPOSED-DECISIONS PR after they merge.
  Why: `pre-pr.sh` doc-links fails on links to `docs/reviews/*` files that are not yet on master.
- Pin the machine budget before a long sweep: one `cargo-mutants -j 1` per agent, and expect memory exhaustion when several agents link `pr-platform-win` at once.
  Why: P3 got 335 of 1000 mutants because of the time box and commit-memory exhaustion; 40+ verdicts were infrastructure failures. Exclude `target-*` from Windows Defender and lower test debug info before the next sweep (not yet measured; owner action tracked in docs/reviews/BACKLOG.md).

## G. Performance (from an external write-up, unverified, kept short)

A public write-up of a 1 vCPU, 2 GB server contest (SQLite, io_uring) is mostly about servers and does not apply to this desktop app. These points do carry over:

- Measure a tuning constant before reasoning about it; one in-flight accept constant (16 to 256) doubled a contestant's score, and a targeted cancel instead of a per-socket scan raised another from 226k to 270k.
  Why: both were found by measuring, not by design; apply it to cache sizes, thread counts and queue depths here, with a benchmark in `pr-bench`.
- Bound every batch (count or time) so a fuller batch cannot break the tail latency; one entry stalled for 13 s when batching had no bound.
  Why: the UI thread and the thumbnail/preview pipeline need a worst case, not only a good average.
- Keep housekeeping (checkpoint, flush, cache cleanup) off the thread that serves the user, and wake it by signal instead of a fixed sleep (57 ms to 6 ms in the write-up).
  Why: same rule as the project's UI-thread rule in `AGENTS.md`; check any sleep-polling loop against it.
- A fast path that relies on a data invariant needs a runtime check and a fallback to the plain path.
  Why: the Rust and Lean entries read the feed by ID range only when IDs are consecutive and otherwise run the standard query.
- Do not copy the extreme tricks (hand-written assembly, bypassing the SQL layer, reimplementing the storage engine).
  Why: the contest author himself said nobody would run these in production because of data-loss risk and maintenance cost.
