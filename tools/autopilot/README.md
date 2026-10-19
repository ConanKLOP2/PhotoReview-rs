# Autopilot kit (run the port end to end with Claude Code)

What was used for Waves 2-4 of the PhotoReview-rs port, kept here so the next run starts from files instead of from
memory. Rules that the agents follow are in `AGENTS.md` ("Pull requests", per-worktree Cargo target cleanup) and
`docs/MULTI-AGENT-PLAN.md`; unconfirmed decisions live in `docs/PROPOSED-DECISIONS.md`.

## Files
| File | Purpose |
|---|---|
| `loop-prompt.md` | The prompt of the 15-minute overnight loop (one tick = PR hygiene, drive the next step of the plan; see AGENTS.md for build artifact cleanup after verdict/merge) |
| `workflows/waveN-contracts.js` | Contract Writer workflow: writers (Sonnet/Opus) per unit, reviewer, wiring (stubs, oracle goldens, PR). Wave 3 and 4 versions; Wave 2 was `wave2-missing-contracts.js` |
| `workflows/waveN-port.js` | Port workflow: red tests (Haiku), implement, adversarial verify (mutation), repair, integrate per tier, finish. Wave 2/3/4 versions |
| `workflows/wave4-finish.js`, `wave2-repair.js` | Follow-ups: merge units that were "blocked" by tier stubs, repair FAIL verdicts, gate |

The scripts are plain JavaScript for the `Workflow` tool (`export const meta`, `agent()`, `pipeline()`); they hard-code
unit lists and paths of the wave they were written for. Copy the closest one and edit the `UNITS`/`TIERS` tables.

## Start a new run
1. Make sure `master` is current and the C# baseline in `docs/SYNC-LEDGER.md` is checked against C# `origin/master`.
2. Start the loop in Claude Code: `/loop 15m <contents of loop-prompt.md>` (needs `CronCreate`; session-only, expires
   after 7 days, dies with the session). The prompt drives everything below; you can also run the steps by hand.
3. Wave step by step: contracts workflow (`waveN-contracts.js`) -> PR merged -> port workflow -> PR merged. From
   Wave 5 on schedule by dependency graph (P-28), not by tier barriers (Wave 4 lost 11 units to "blocked by a stub of
   a unit in the same tier").
4. Every PR: bind it (`ccd_pr bind_pr`), Auto-fix on, `set_auto_merge` squash. After merge remove the worktrees,
   branches and `target-wave-*` dirs.

## Settings that save time (user action; an agent cannot grant itself permissions)
Create `.claude/settings.local.json` (git-ignored) so the classifier does not stop PR and cleanup commands:

```json
{"permissions":{"allow":["Bash(gh pr merge:*)","Bash(gh pr create:*)","Bash(gh pr checks:*)","Bash(gh pr comment:*)","Bash(gh run view:*)","Bash(git branch -D:*)","Bash(git worktree add:*)","Bash(git worktree remove:*)","Bash(git worktree prune:*)","Bash(git push origin --delete:*)","Bash(git push -u origin:*)","Bash(git merge:*)","Bash(rm -rf target-*)","Bash(cargo:*)","Bash(powershell -NoProfile -ExecutionPolicy Bypass -File tools/*)"]}}
```

Without them the finish agents of a workflow refuse to push or open the PR; the lead (the main session) does it.

## Claude-free pipeline (Codex only, zero Claude tokens)
Ports the units of one Wave 8 batch with Codex CLI (gpt-6-luna, effort low) driven by plain bash. Optimised for Codex quota and
Claude tokens, not for wall-clock time. Files: `codex-pipeline.sh` (driver), `codex-pack.sh` (context pack), `codex-gate.sh` (machine
checks), `codex-run.sh` (one Codex call, `--json`, `resume`), `CODEX-RULES.md`, `lib/codex-lib.py` (plan/contract/state/lint/telemetry
helpers), `test/pipeline-selftest.sh` (fake-Codex control-flow test, also covers every gate and helper).

Run (terminal, main checkout, contracts of the batch already merged to `master`):
```
bash tools/autopilot/codex-pipeline.sh --smoke                     # env checks + 1 tiny real Codex call + gate on a throwaway unit
bash tools/autopilot/codex-pipeline.sh wave8-b2 --dry-run          # order, deps, state, mutation tier, contract present?
bash tools/autopilot/codex-pipeline.sh wave8-b2 --jobs 2           # real run; --only W8-04a,W8-04b limits it; --jobs 1..4 (default 2)
```
Per unit: worktree `wu/<id>` from `integration/<tag>`, pack (built once), Codex "red tests" if the test files are missing (committed as
its own commit; later gates verify the tests are only extended), Codex "implement", fast gate with up to 3 repair rounds, mutation
(by tier), up to 2 kill-mutant rounds, one full gate (clippy + whole crate test), commit (trailer `Co-Authored-By: Codex gpt-6-luna`),
`merge --no-ff` under one lock per integration branch.

Pipeline hardening:
- **Red-test compile gate**: after the Codex red step the owned test files must pass `cargo test --no-run` and every failing test must fail only through `todo!()` (`codex-lib.py redcheck`); up to 3 `red-repair` Codex rounds, the red-test guard is not applied in this phase; invalid after 3 rounds = failed unit with triage note. If the red tests already exist on the integration branch (written by Claude) the Codex red step is skipped.
- **Rerun of a failed unit**: the unit is reset first (any worktree holding `wu/<id>`, the branch, triage note and state/telemetry removed; an unmerged old tip is kept under `refs/pipeline-old/<id>`), no more `worktree add failed`.
- **Merged units**: at start, units with a merge commit on `integration/<tag>` (since `origin/master`) are marked done, no seeded state file.
- **Target dirs**: one per slot (`-s<slot>`), mutants `-mut`; a dir whose `.cargo-build-lock` is held by a stray process (`Access is denied`) is skipped for an alternate `-x<n>` dir. At the end: workspace gate, telemetry table, push, PR,
`gh pr merge --auto --squash` (only if the workspace gate passed). Never falls back to Claude.

Savings built in:
- **Codex session reuse**: repair and kill-mutant rounds `codex exec resume <session-id>` (id read from the `--json` log) and send only
  the error tail (<= 60 lines) plus one instruction line; a failed resume falls back to a fresh call with the pack. Pack is trimmed
  (C# license headers, `///` doc lines, blank runs, per-file cap `PACK_FILE_CAP`=24000 B, de-duplicated signatures); its size is in the state file.
- **Build reuse**: one `CARGO_TARGET_DIR` per integration tag (`target-wave-pipe-<tag>`) shared by all units run sequentially, one per slot
  (`-s1`..`-s4`) with `--jobs > 1`, plus a dedicated `-mut` dir for cargo-mutants (never the unit's own). Deleted only when the pipeline ends.
  Repair loops run `cargo fmt -p <crate> -- --check` (`lib/fmtcheck.sh`; never the whole workspace on one command line, that fails with Windows os error 206 in deep worktree paths; the workspace gate loops over the members, with a rustfmt batch fallback), `cargo check -p <crate> --all-targets` and only the unit's own test binaries (`--test <name>`, `--lib`
  if the src has `#[cfg(test)]`); clippy + the full crate test run once per unit before the merge. `cargo-nextest` is used when installed (`GATE_NEXTEST=0` disables).
- **Mutation by risk tier** (plan row): green or golden-covered (Test column `G`) = skipped; yellow = 15 min budget; red = 30 min
  (`MUT_BUDGET_YELLOW`/`MUT_BUDGET_RED`/`MUTANTS_TIMEOUT`). `cargo mutants --in-diff` on the owned src files only, `--baseline skip`,
  `-j` `MUT_JOBS` (default 3), only the unit's test binaries, Debug/Display/`fmt`/`Default` impls excluded, re-runs only the survivors
  (`-F`). Budget exhausted = status `partial` (not failure). Note: with `-j > 1` cargo-mutants builds share the dedicated target dir; set `MUT_JOBS=1` if you ever see odd mutant results.
- **Quota**: tokens, seconds and 5 h-quota percent are recorded per unit; a unit is not started when `100 - used%` is below the average
  cost per unit x `QUOTA_MARGIN` (1.2); the 99 % ceiling (`QUOTA_MAX`) is never crossed; waits sleep until the reset (+2 min), no busy loops.
- **Parallelism**: `--jobs N` runs independent units (dependencies merged) concurrently in separate worktrees with separate target dirs.
- **Deterministic review gates** (no LLM, in `codex-gate.sh`): files outside Owned files, `unwrap()`/`expect(` on file-system/user-data lines
  (`// allow-unwrap` opts out), `unsafe` outside `pr-platform-win`, `unsafe` blocks without `// SAFETY:`, `todo!()`/`unimplemented!()`
  left in owned src, `#[allow(` added without a reason comment, `.claude`/target files, red tests weakened (test fns may only grow).
- **Triage**: a failed unit writes `state/needs-claude/<id>.md` (<= 3 KB: error tail, `git diff --stat`, failing tests, last Codex message);
  the PR body embeds them under "Needs Claude". The PR body also has the per-unit telemetry table (status, rounds, tokens in/cached/out,
  seconds, mutants killed/survived/budgeted-out).
- **Preflight**: before the first unit the pipeline checks codex binary, python, shell, git (`safe.directory` for the repo and `CS_REPO`),
  cargo, cargo-mutants, gh, free disk (>= `MIN_DISK_GB`=20) and that no other pipeline holds the lock, and aborts with one clear message.

Resume: run the same command again. `tools/autopilot/state/<tag>.json` (gitignored) records `done`/`partial`/`failed`/`skipped` per unit;
merged units are skipped, failed and skipped ones are retried. `CODEX_BIN`, `CODEX_MODEL`, `CODEX_EFFORT`, `CODEX_TIMEOUT`, `SKIP_MUTANTS=1`,
`PIPELINE_NO_PR=1` override defaults (see the header of `codex-pipeline.sh`).

What Claude (or you) must still do: write the contracts, stubs and goldens (Codex low cannot design); fix units marked `failed` or `skipped`
(read `state/needs-claude/<id>.md` first; the worktree is kept); read the final PR diff (Codex output is data, not authority); decide on
`partial` units (surviving mutants = missing tests).

### Windows notes
- Exclude the repo, `.claude/worktrees` and the `target-wave-pipe-*` dirs (and `%USERPROFILE%\.cargo`, `%USERPROFILE%\.rustup`) from antivirus
  real-time scanning (Windows Security: Virus & threat protection > Exclusions); builds are several times faster.
- Set `CARGO_BUILD_JOBS` (e.g. 4 on 8 GB RAM) so parallel units and cargo-mutants do not exhaust memory; `--jobs 2` x `MUT_JOBS 3` is already 6 builds.
- Enable long paths: `git config --global core.longpaths true` and registry `LongPathsEnabled=1`; cargo-mutants copies the tree to `%TEMP%` (~2 GB each).
- Run from Git Bash; `git config --global --add safe.directory <repo>` (and for the C# checkout) if git reports "dubious ownership".

## Gotchas learned (see AGENTS.md for the rules)
- Windows PowerShell 5.1 has no `&&`; run `tools/*.ps1` from the PowerShell tool with `-ExecutionPolicy Bypass`.
- cargo may not be on PATH in the Bash tool: use `$HOME/.cargo/bin`. Build with `CARGO_PROFILE_DEV_DEBUG=line-tables-only`
  `CARGO_INCREMENTAL=0`; per-worktree `CARGO_TARGET_DIR`; cargo-mutants copies the workspace to `%TEMP%` (~2 GB each).
- A workflow that hits the account usage limit fails its remaining agents; resume it later with `resumeFromRunId`.
