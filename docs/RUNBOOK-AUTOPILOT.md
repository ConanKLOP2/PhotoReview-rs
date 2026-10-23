# Autopilot operator runbook

## 1. Overview and when to use it

`tools/autopilot/codex-pipeline.sh` ports a batch from the plan into `integration/<tag>` with Codex CLI, contract-owned files, machine gates, optional mutation checks, and a PR to `master`. Use it only after the batch's contracts, stubs, and required oracle material are on `master`; check the batch and dependencies with `--dry-run` first. The C# checkout is the read-only specification. See [README](../tools/autopilot/README.md), [Codex rules](../tools/autopilot/CODEX-RULES.md), [multi-machine rules](MULTI-MACHINE.md), and [AGENTS.md](../AGENTS.md).

Run from Git Bash in the main Rust checkout. The pipeline changes Git branches, worktrees, claims, and remote PR state; `--dry-run` only reports the planned order. Its `--jobs` range is `1..4`, default `2`. Do not run a second pipeline while `state/pipeline.lock` is held. On several machines, use the ownership table in [MULTI-MACHINE.md](MULTI-MACHINE.md), set `PR_MACHINE`, and claim units before manual edits. State is local to each machine; claims are refs on `origin`.

The driver is an older Wave 8 batch tool, not a general contract writer. [PROPOSED-DECISIONS.md](PROPOSED-DECISIONS.md) records the Codex routing proposals; [AGENTS.md](../AGENTS.md) sets the current mutation and PR rules. Review the generated diff and mutation gaps before treating a `done` or `partial` state as a port verdict.

## 2. Prerequisites and environment

Preflight checks the Codex executable, Python 3.8+, Bash, both Git checkouts, Cargo, `cargo-mutants` unless disabled, `gh` unless PR publishing is disabled, free disk, and the pipeline lock. `cargo-nextest` is optional. The pipeline prepends `$HOME/.cargo/bin` to `PATH`. Its smoke mode also makes one real Codex call and checks that the gate accepts a good tiny crate and rejects `todo!()`.

These are the operator-controlled environment variables read by the pipeline or its helpers. “Unset” means the script supplies no value; a referenced tool then uses its own behavior. `HOME` and `PATH` come from the shell.

| Variable | Default / effect |
|---|---|
| `CODEX_BIN` | `$HOME/.codex/plugins/.plugin-appserver/codex.exe` |
| `CODEX_MODEL`, `CODEX_EFFORT` | `gpt-6-luna`, `low`; passed by `codex-run.sh` for fresh and resumed sessions |
| `CODEX_USAGE_SH` | `tools/autopilot/codex-usage.sh` |
| `CODEX_TIMEOUT` | `2400` seconds per Codex call |
| `PLAN_REL` | `docs/WAVE-8-PLAN.md` |
| `CS_REPO`, `CS_BASELINE` | `C:/MyProjects/PhotoReview`; contract baseline if present, otherwise `d92d6aae` |
| `PACK_FILE_CAP` | `24000` bytes per C# file in the pack |
| `SKIP_MUTANTS`, `PIPELINE_NO_PR` | `0` by behavior; `1` skips mutation or skips push/PR respectively |
| `MUT_JOBS` | `4` in this driver; see the mutation caution below |
| `MUT_BUDGET_YELLOW`, `MUT_BUDGET_RED` | `900`, `1800` seconds; `MUTANTS_TIMEOUT` overrides both when set |
| `MUT_TEST_TIMEOUT` | `90` seconds per mutant test run |
| `MIN_DISK_GB` | `20` GB preflight minimum |
| `QUOTA_MAX`, `QUOTA_MARGIN` | `95` percent and `1.2`; cap and estimated unit-cost margin |
| `USAGE_CACHE_TTL`, `QUOTA_SLEEP_OVERRIDE` | `45` seconds; unset (normal wait uses the reset time, capped by the driver) |
| `RED_RUN_TIMEOUT` | `900` seconds for the red-test run |
| `GATE_NEXTEST`, `GATE_TEST_BASE` | `auto` (use nextest if installed; `0` disables); unset until the driver sets the red-test commit |
| `FMT_FORCE_BATCH` | `0`; `1` forces `fmtcheck.sh` to check Rust files in batches |
| `PR_MACHINE` | `tools/autopilot/state/machine`, then host name |
| `CARGO_BUILD_JOBS` | Unset; set for memory pressure. The pipeline warns when absent. |
| `CARGO_PROFILE_DEV_DEBUG`, `CARGO_INCREMENTAL` | Pipeline/gate set `line-tables-only`, `0`; mutant builds use debug `0`. |
| `CARGO_TARGET_DIR` | Driver sets `<unit worktree>/target` for Codex and unit gates, `<unit worktree>/target-mut` for mutants, and a tag-specific target for the workspace gate. Do not redirect Codex builds outside its worktree. |

`codex-usage.sh` takes an optional positional threshold, default `99`; the pipeline passes `QUOTA_MAX` instead. `status.sh` takes an optional integration branch, default `integration/wave-8`, and uses its hard-coded Rust checkout path. `unit-pr.sh` also reads `PLAN_REL`.

## 3. Pipeline phases, in order

1. **Select and preflight.** `--only U1,U2` limits the batch, `--jobs N` chooses concurrent slots, and `--dry-run` prints dependency order, prior state, mutation tier, and contract presence without creating worktrees. A real run takes `state/pipeline.lock`, checks tools/disk, fetches `origin`, and creates or reuses `integration/<tag>`. Existing merge commits on that branch can mark units `done`. The scheduler waits for same-batch dependencies, claims each unit, and waits for quota. A blocked dependency or another machine's claim makes a unit `skipped`.
2. **Reset, worktree, pack.** Each started unit is reset, receives `wu/<unit>` at `.claude/worktrees/wu-<lowercase-unit>`, and writes `.codex-pack.md` with plan row, contract, baseline C# sources, dependency signatures, and Codex rules. The pack must build successfully and the contract must name owned files and a crate. A pack or worktree failure writes a triage note and marks `failed`.
3. **Red tests.** If an owned test file already exists, this Codex step is skipped. Otherwise Codex may create only owned `tests/*.rs` files. `red_check` runs `cargo test -p <crate> <unit test selectors> --no-run`, then `timeout "$RED_RUN_TIMEOUT" cargo test -p <crate> <selectors> --no-fail-fast`. Every failing test must reach a stub's `todo!()` or `unimplemented!()`; compilation, assertions, crashes, and timeouts do not qualify. Up to three `red-repair` calls may correct the tests. On success, the tests get their own commit and its HEAD becomes `GATE_TEST_BASE`; on failure, the unit is `failed`. The later red-test guard is deliberately absent during this phase.
4. **Implement and fast gate.** Codex edits owned source. `codex-gate.sh <unit> <worktree> <crate> fast` checks owned changes, static rules, the red-test guard, package formatting through `lib/fmtcheck.sh`, `cargo check -p <crate> --all-targets`, and only the unit's test binaries (`--test <name>` and, where applicable, `--lib`). It uses nextest if available unless disabled. A failure gets at most three Codex repair calls with the gate's short error tail; the loop reruns the fast gate. Failure after those calls marks `failed`.
5. **Full unit gate.** Before mutation, `codex-gate.sh ... full` repeats ownership, static, red-test, and formatting checks, then runs `cargo clippy -p <crate> --all-targets -- -D warnings` and the unit's selected tests. It has the same three-repair limit. The full gate is run again after kill-mutant rounds if they changed tests. This gate does not run the whole workspace.
6. **FIXUP.** There is no distinct `FIXUP` phase or `--fixup` flag in this tree's `codex-pipeline.sh`; corrections occur in `red-repair`, gate `repair`, and `kill-mutants` rounds.
7. **Mutation tiers and kill-mutant rounds.** `lib/codex-lib.py tier` maps green or golden-covered rows to `skip`, red rows to `red`, and other rows to `yellow`. Mutation runs only against the owned source diff, with `cargo mutants -p <crate> --in-diff <diff> --baseline skip --timeout "$MUT_TEST_TIMEOUT" -j "$MUT_JOBS" --no-shuffle`, the unit's test selectors, and exclusions for Debug/Display/Default and `fmt`. Yellow and red have the budgets above. It writes mutant output/logs and survivor lists under `state/logs`. Up to two Codex rounds add or strengthen owned tests for survivors; each must pass the fast gate, then mutation reruns with `-F` limited to survivors. An exhausted budget is `partial`, as are remaining survivors. Missing mutants or an empty diff are recorded as skips. The driver defaults to `MUT_JOBS=4`, but [AGENTS.md](../AGENTS.md) requires `-j 1` with a separate target for reliable mutation verdicts; set `MUT_JOBS=1` for a run whose mutation results will be used as evidence. Time-box and report untested mutants.
8. **Merge and state.** A green unit is committed and merged with `--no-ff` into `integration/<tag>` under a merge lock. State becomes `done`, or `partial` for survivors or an exhausted mutation budget; merge conflict or commit failure becomes `failed` and keeps the worktree. A merged unit's worktree is removed. The script does not itself verify native GUI behavior or resolve equivalent survivors.
9. **Workspace gate and PR.** If any unit merged, the integration worktree runs `lib/fmtcheck.sh --all`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked`; output goes to `state/logs/workspace-gate-<tag>.log`. `PIPELINE_NO_PR=1` stops before push and returns the gate verdict. Otherwise the driver writes a PR body, pushes the integration branch, creates or edits a PR against `master`, and calls `gh pr merge <branch> --auto --squash` only if the workspace gate passed. A failed workspace gate is reported in the PR body and does not enable auto-merge. Follow [AGENTS.md](../AGENTS.md) for PR monitoring, final merge verification, and cleanup; the pipeline ends before that lifecycle is complete.

## 4. Helper scripts

`codex-run.sh <worktree> <prompt-file> [last-message-file] [session-id]` starts `codex exec --json` in workspace-write mode. With a session ID it uses `codex exec resume` from that worktree; the driver gets the ID and token totals from JSONL. A failed resume falls back to a new call with the pack. `CODEX_MODEL` and `CODEX_EFFORT` select the model and effort.

`codex-usage.sh [threshold_percent]` asks the Codex app server for the five-hour and weekly usage percentages and reset times. It returns JSON and exit `0` only when ordinary use is allowed, no rate limit is reported, and five-hour use is below the passed threshold; otherwise exit `1`. `QUOTA_MAX=95` is the pipeline's five-hour ceiling. [MULTI-MACHINE.md](MULTI-MACHINE.md) also sets a 95 percent weekly operator limit: the helper reports weekly use, but its exit condition does **not** compare weekly percent with `QUOTA_MAX`. Watch that field across machines. The driver estimates the next unit's cost from completed units times `QUOTA_MARGIN`, waits when capacity is insufficient, and sleeps until a reset rather than busy polling.

`claim.sh claim <unit> [note]` uses `refs/claims/<unit>` on `origin`. Exit `0` means claimed or already mine, `1` means another machine holds it, and `2` means origin/push could not establish a claim (or invalid use). `claim.sh list` reports unit, machine, time, note; `release <unit>` removes one's own claim; `who <unit>` names its holder; `gc` removes claims whose unit appears merged on `origin/master` or whose claim is older than the script's age limit. `release <unit> --force` is for an agreed handover only. Read the output; the pipeline currently continues after claim exit `2`, so an unreachable origin is an operator issue, not proof of ownership. See [claim.sh](../tools/autopilot/claim.sh) and [MULTI-MACHINE.md](MULTI-MACHINE.md).

`unit-pr.sh <unit> [--ibr <branch>] [--dry-run]` publishes one already implemented unit from an integration branch as its own PR. It checks same-batch dependencies on `origin/master`, copies owned files, tests and formats them, and may copy completed source modules needed by `todo!()` failures. Its dry run performs checks without staging, committing, pushing, or opening a PR. This is separate from the batch pipeline. `status.sh [integration-branch]` gives a short PR, branch, workflow, quota, and working-tree status snapshot.

## 5. Local state files and logs

The scripts keep local files in `tools/autopilot/state/` (git-ignored and not copied between machines):

| Location | Meaning |
|---|---|
| `<tag>.json` | `units` map and `meta` object; a missing file reads as empty state. Each unit has `status` (`done`, `partial`, `failed`, or `skipped`) and `note`. `survivors` counts currently reported missed mutants. |
| Unit telemetry in `<tag>.json` | `tier`; `pack_bytes`; `rounds` (Codex calls); `tin`, `tcached`, `tout` (JSONL token totals); `secs` (elapsed unit seconds on completion); `pct` (nonnegative five-hour percentage change); `killed` and `budgeted` (mutation counts). `lib/codex-lib.py table` renders these fields. A partial mutation run is not a full verdict. |
| `machine` | Local machine name fallback for `claim.sh` when `PR_MACHINE` is unset. |
| `pipeline.lock/`, `merge-<tag>.lock/`, `<tag>.json.lock` | Driver, integration merge, and short JSON update locks. Do not remove a live lock. |
| `usage.cache` | Cached quota JSON and timestamp, normally reused only within `USAGE_CACHE_TTL`. |
| `logs/pipeline-<tag>.log`, `logs/<unit>.log` | Driver events and the unit's Codex JSONL / Git command output. Other `logs/<unit>.*` files hold sent prompts, gate errors, red output, mutant diff/output/survivors, and `logs/pr-body-<tag>.md`. |
| `logs/workspace-gate-<tag>.log`, `logs/claims.log` | Workspace command output and claim command output. |
| `needs-claude/<unit>.md` | A bounded triage note for a failed unit: reason, error tail, `git diff --stat`, parsed failing tests, and last Codex message. The lead/operator or a Claude lane reads and resolves it; Codex does not automatically fix it after the retry limit. |

`done` and `partial` are skipped on resume; `failed` and `skipped` can be retried. The note and telemetry are machine-local, not a remote status authority. See [codex-lib.py](../tools/autopilot/lib/codex-lib.py) for the JSON writer and [pipeline selftest](../tools/autopilot/test/pipeline-selftest.sh) for state/triage checks.

## 6. Redo a unit

Use `bash tools/autopilot/codex-pipeline.sh <tag> --only <unit> --redo <unit> --jobs 1` only after checking the integration branch and the unit's state. `--redo` marks the requested unit `failed` so the scheduler starts it even if its old state said `done` or `partial`. At unit start, `reset_unit` removes a worktree holding `wu/<unit>`, removes the expected leftover worktree directory, prunes registrations, deletes the local branch, removes the old triage note, and clears that unit's JSON state and telemetry. If its old branch tip was not merged into the integration branch, it saves that tip at `refs/pipeline-old/<unit>` first. It then creates a fresh unit branch from `integration/<tag>`.

`--redo` does **not** remove the earlier merge from `integration/<tag>`, reset another unit, or delete the retained `refs/pipeline-old/<unit>`. It is therefore a fresh attempt against the current integration head, not an undo of published history. Inspect the recovery ref and branch before cleanup.

## 7. Known failure modes

| Symptom | Cause and evidence | Fix |
|---|---|---|
| Codex's Cargo gets `Access is denied (os error 5)` | Codex Windows sandbox writes only to its worktree cwd; a shared target outside it fails. [README](../tools/autopilot/README.md) and the driver record the fix. | Keep `CARGO_TARGET_DIR=<unit worktree>/target` for Codex calls and the mutation target in that worktree. Do not override it with a shared external target. |
| Red tests compile but the unit is rejected as invalid | The red guard requires failures through a stub `todo!()`/`unimplemented!()`, not a wrong assertion, missing API, crash, or timeout. The [selftest](../tools/autopilot/test/pipeline-selftest.sh) covers those cases. | Read `logs/<unit>.red.out` and the red-repair prompt; correct only owned test files against the contract/stub. After three failed red repairs, inspect `needs-claude/<unit>.md`. |
| Gate rejects otherwise passing tests on formatting | `lib/fmtcheck.sh` checks the whole crate, including tests; long Windows command lines can produce os error 206. [fmtcheck.sh](../tools/autopilot/lib/fmtcheck.sh) has a batched rustfmt fallback. | Run `cargo fmt -p <crate>` in the unit worktree, then rerun the gate; use `FMT_FORCE_BATCH=1` if the single-package check itself hits the command-line limit. |
| `STATUS_DLL_INIT_FAILED` under load | [MULTI-MACHINE.md](MULTI-MACHINE.md) identifies this as a build-blocking machine condition; a specific root cause is not documented there. | Stop adding concurrent work, retry with fewer `--jobs` and lower `CARGO_BUILD_JOBS`; report the problem in the claim note and release the claims if the machine cannot build. Do not classify the affected test as a code pass. |
| Windows reports “Filename too long” while removing a worktree | A removal-specific incident is **not verified** in the checked repo. [README](../tools/autopilot/README.md) documents Windows long-path setup, and [fmtcheck.sh](../tools/autopilot/lib/fmtcheck.sh) documents a separate long-command failure. | Enable Git long paths (`git config --global core.longpaths true`) and the Windows `LongPathsEnabled` setting as the README directs, then retry `git worktree remove` for the verified worktree. Check `git worktree list --porcelain` and preserve dirty or unmerged work before any manual cleanup. |
| `misc_app_log` or `cache_preview_file` fails during heavy parallel runs | Both test binaries exist at `crates/pr-app/tests/misc_app_log.rs` and `crates/pr-imaging/tests/cache_preview_file.rs`. Their *flakiness under load is not verified* in the checked sources. [AGENTS.md](../AGENTS.md) requires root cause first for a failing check. | Preserve the failing output, inspect the test and host/resource condition, reduce build pressure if indicated, and fix the cause in its own PR. Do not blindly rerun until green. |
| Mutant run consumes large disk/time or reports suspicious survivors | [README](../tools/autopilot/README.md) records that cargo-mutants copies the tree; [AGENTS.md](../AGENTS.md) records rebuild cost and false `MISSED` with shared-target parallel runs. | Keep the mutation target separate and inside the unit worktree for sandbox access, use `MUT_JOBS=1`, narrow to owned risky logic, and time-box. Report tested/total and untested mutants; never turn a budgeted `partial` into a full pass. |
| A retry says `worktree add failed` or a target build lock is inaccessible | A stale `wu/<unit>` worktree/branch or an outlived Cargo process can block reuse. The [driver](../tools/autopilot/codex-pipeline.sh) resets unit worktrees and probes target locks; the [selftest](../tools/autopilot/test/pipeline-selftest.sh) covers moved worktrees. | Rerun the same unit so reset runs; inspect retained `refs/pipeline-old/<unit>` before removing it. For a locked target, let the driver choose its alternate directory and stop only processes you own. |

## 8. Quick commands

```bash
bash tools/autopilot/codex-pipeline.sh --smoke
bash tools/autopilot/codex-pipeline.sh wave8-b2 --dry-run
bash tools/autopilot/codex-pipeline.sh wave8-b2 --only W8-04a,W8-04b --jobs 1
bash tools/autopilot/codex-pipeline.sh wave8-b2 --only W8-04a --redo W8-04a --jobs 1
bash tools/autopilot/codex-usage.sh 95
bash tools/autopilot/claim.sh list
bash tools/autopilot/claim.sh claim W8-04a "porting"
bash tools/autopilot/claim.sh release W8-04a
bash tools/autopilot/status.sh integration/wave-8
bash tools/autopilot/unit-pr.sh W8-06f --ibr integration/wave8-b2 --dry-run
```

These are syntax examples from the scripts, not a claim that the example units are pending in the current branch. Read `tools/autopilot/state/<tag>.json`, its logs, the integration diff, and the PR checks before declaring completion.
