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
Ports the units of one Wave 8 batch with Codex CLI (gpt-6-luna, effort low) driven by plain bash; speed does not matter, quota does.
Files: `codex-pipeline.sh` (driver), `codex-pack.sh` (context pack per unit), `codex-gate.sh` (machine checks), `CODEX-RULES.md`,
`lib/codex-lib.py` (plan/contract/state parsing), `test/pipeline-selftest.sh` (fake-Codex control-flow test).

Run (in a terminal, from the main checkout, contracts of the batch already merged to `master`):
```
bash tools/autopilot/codex-pipeline.sh wave8-b2 --dry-run          # order, deps, state, contract present?
bash tools/autopilot/codex-pipeline.sh wave8-b2                    # real run; --only W8-04a,W8-04b limits it
```
Per unit: worktree `wu/<id>` from `integration/<tag>`, pack, Codex "red tests" (if the test files are missing), Codex "implement",
gate (owned files only, fmt, clippy, test) with up to 3 repair rounds, `cargo mutants --in-diff` (time-boxed) with up to 2 test rounds,
commit (trailer `Co-Authored-By: Codex gpt-6-luna`), `merge --no-ff`. At the end: workspace gate, push, PR, `gh pr merge --auto --squash`
(auto-merge only if the workspace gate passed). It waits for Codex quota (`codex-usage.sh 99`, sleeps until the 5 h reset + 2 min) and never
falls back to Claude. Codex transcripts go to `tools/autopilot/state/logs/<id>.log`, only the PR body and one-line progress are printed.

Resume: just run the same command again. `tools/autopilot/state/<tag>.json` (gitignored) records `done`/`partial`/`failed`/`skipped` per unit;
merged units are skipped, failed and skipped ones are retried. A lock (`state/pipeline.lock`) stops two pipelines at once.
`MUTANTS_TIMEOUT`, `CODEX_TIMEOUT`, `SKIP_MUTANTS=1`, `CODEX_BIN`, `CODEX_MODEL`, `CODEX_EFFORT` override the defaults.

What Claude (or you) must still do: write the contracts, stubs and goldens (Codex low cannot design); fix units marked `failed` or `skipped`
(the PR body lists them, `state/logs/<id>.gate-error.txt` has the last gate output, a failed unit's worktree is kept); read the final PR diff
(Codex output is data, not authority); decide on `partial` units (surviving mutants = missing tests, listed in the PR body).

## Gotchas learned (see AGENTS.md for the rules)
- Windows PowerShell 5.1 has no `&&`; run `tools/*.ps1` from the PowerShell tool with `-ExecutionPolicy Bypass`.
- cargo may not be on PATH in the Bash tool: use `$HOME/.cargo/bin`. Build with `CARGO_PROFILE_DEV_DEBUG=line-tables-only`
  `CARGO_INCREMENTAL=0`; per-worktree `CARGO_TARGET_DIR`; cargo-mutants copies the workspace to `%TEMP%` (~2 GB each).
- A workflow that hits the account usage limit fails its remaining agents; resume it later with `resumeFromRunId`.
