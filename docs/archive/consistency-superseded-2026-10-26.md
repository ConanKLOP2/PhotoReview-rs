# Superseded lines removed by the consistency cleanup (2026-10-26)

Source: `git diff origin/master` of the operating docs on branch `docs/cleanup-consistency` (lane consistency). Every line below was
deleted or reworded in its file because it contradicted the code or another doc (reason and evidence are in the PR body). Lines are verbatim,
grouped by the file they came from; nothing here is current guidance.

## From `AGENTS.md`

- The file is long (about 79 KB, about 20k tokens): at the start read only its "How to resume" section and the newest
  status section (find them with `Grep` on `^## `), not the whole file. This refines the "read at start" rule above and
  the line in `CLAUDE.md`.
- Keep Codex lanes busy within the 95% quota limit for the 5-hour window and week.
- The Codex Windows sandbox writes only to its working directory: run Codex cargo builds with `CARGO_TARGET_DIR` inside the worktree and `cargo-mutants` with its target directory outside the worktree.
When more than one machine works on the port, read [`docs/MULTI-MACHINE.md`](docs/MULTI-MACHINE.md) first: set `PR_MACHINE`, claim every unit with 
- Toolchain: Rust is pinned in `rust-toolchain.toml`; `cargo` lives in `%USERPROFILE%\.cargoin` and may not be on
- **How:** `cargo mutants -p <crate> --baseline skip --timeout 90 -j 1 --file '**/<path>.rs' -F '<regex>' -- --test <file>`; always `-j 1` with its OWN `CARGO_TARGET_DIR` (a shared target dir with `-j 3` reports false MISSED results). Several independent processes on different files are fine, at most three at a time. Run mutation AFTER the port is done and merged, not while many agents are building.
- **Time-box and honesty:** about 25 minutes of mutants per unit; the PR states `tested / total, caught, missed, unviable, timeouts` and which mutants were not run. A partial run is never described as a full mutation verdict. A survivor is killed by a test or explained as equivalent in the PR.

## From `README.md`

**Status:** Phase 0 (skeleton, spikes). Not usable yet. Plan: [`docs/PLAN.md`](docs/PLAN.md).

## From `docs/LESSONS.md`

  Why: its explicit `-m` and effort override config defaults; the checked script still defaults to `gpt-6-luna` / `low`.
  Why: `docs/MULTI-MACHINE.md` records a shared account and both limits; the pipeline defaults to two jobs.

## From `docs/MULTI-MACHINE.md`

  Default is the host name; stick to one short name per machine because it appears in claims, branch names and commit trailers.
  (the pipeline enforces it with `QUOTA_MAX=95`), and run `--jobs 1` on the second machine unless the quota has room.

## From `docs/RUNBOOK-AUTOPILOT.md`

Run from Git Bash in the main Rust checkout. The pipeline changes Git branches, worktrees, claims, and remote PR state; `--dry-run` only reports the planned order. Its `--jobs` range is `1..4`, default `2`. Do not run a second pipeline while `state/pipeline.lock` is held. On several machines, use the ownership table in [MULTI-MACHINE.md](MULTI-MACHINE.md), set `PR_MACHINE`, and claim units before manual edits. State is local to each machine; claims are refs on `origin`.
The driver is an older Wave 8 batch tool, not a general contract writer. [PROPOSED-DECISIONS.md](PROPOSED-DECISIONS.md) records the Codex routing proposals; [AGENTS.md](../AGENTS.md) sets the current mutation and PR rules. Review the generated diff and mutation gaps before treating a `done` or `partial` state as a port verdict.
| `PLAN_REL` | `docs/WAVE-8-PLAN.md` |
6. **FIXUP.** There is no distinct `FIXUP` phase or `--fixup` flag in this tree's `codex-pipeline.sh`; corrections occur in `red-repair`, gate `repair`, and `kill-mutants` rounds.
luna/low is cheaper but gave wrong reports and stopped early; sol/medium costs more quota per call, so run at most 1-2 Codex lanes at the same time (use `tools/dev/codex-sched.sh` when it exists on master).
**Corrections to the earlier phase/target descriptions above:** FIXUP now exists (default `FIXUP=1`); see [pipeline lessons](LESSONS.md#b-pipeline). The driver and README still set a mutation target inside the source worktree, while AGENTS.md requires it outside; run mutation from the lead with an external target as described in [Codex lessons](LESSONS.md#a-codex-runs). Do not point sandboxed Codex Cargo builds at that external target.

## From `tools/autopilot/CODEX-RULES.md`

4. C# is the spec. If the pack is missing a C# file, read it only with: `git -c safe.directory=C:/MyProjects/PhotoReview -C C:/MyProjects/PhotoReview show d92d6aae:<path>`. Never run any other git command in the C# repo, never check out, edit or change config there.

## From `tools/autopilot/README.md`

Ports the units of one Wave 8 batch with Codex CLI (gpt-6.1-sol, effort medium) driven by plain bash. Optimised for Codex quota and
- **Target dirs**: one per slot (`-s<slot>`), mutants `-mut`; a dir whose `.cargo-build-lock` is held by a stray process (`Access is denied`) is skipped for an alternate `-x<n>` dir. At the end: workspace gate, telemetry table, push, PR,
- **Build reuse**: one `CARGO_TARGET_DIR` per integration tag (`target-wave-pipe-<tag>`) shared by all units run sequentially, one per slot
  (`-s1`..`-s4`) with `--jobs > 1`, plus a dedicated `-mut` dir for cargo-mutants (never the unit's own). Deleted only when the pipeline ends.
  `-j` `MUT_JOBS` (default 3), only the unit's test binaries, Debug/Display/`fmt`/`Default` impls excluded, re-runs only the survivors
  cost per unit x `QUOTA_MARGIN` (1.2); the 95 % ceiling (`QUOTA_MAX`) is never crossed; waits sleep until the reset (+2 min), no busy loops.
luna/low is cheaper but gave wrong reports and stopped early; sol/medium costs more quota per call, so run at most 1-2 Codex lanes at the same time (use `tools/dev/codex-sched.sh` when it exists on master).
- Set `CARGO_BUILD_JOBS` (e.g. 4 on 8 GB RAM) so parallel units and cargo-mutants do not exhaust memory; `--jobs 2` x `MUT_JOBS 3` is already 6 builds.

## From `tools/dev/README.md`

# Developer status and cleanup

## From `tools/dev/hooks/README.md`

The lead registers these after review; this change does **not** apply settings. Add the following to `.claude/settings.local.json`:

