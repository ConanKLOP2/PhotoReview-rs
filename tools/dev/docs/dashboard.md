# dashboard.sh

Purpose: write ONE self-contained HTML page (inline CSS and JS, no network, no external resource, light and dark, phone width) with the project state: `tools/dev/status.sh --json` (quota, disk, processes, pipelines, claims, PRs, worktrees), Wave 8 progress and optional PARITY statuses.

Usage: `bash tools/dev/dashboard.sh [--out FILE]` (default `tools/dev/out/dashboard.html`, git-ignored via `tools/dev/.gitignore`). Prints the path, size and merged/total units.

Wave progress: units are the `W8-*` rows of `docs/WAVE-8-PLAN.md` (batch B1..B6). The count reads subject AND body of every commit on `origin/master` (`git log --format='%s%n%b%x00'`). A commit whose subject starts with docs/chore/ci/style is skipped entirely (subject and body). In the others, a LINE counts when it starts (optionally after a `* ` squash bullet) with feat/port/fix/test/refactor/perf/merge (optional scope) or `Merge W8-`, and never when it contains `red tests` (a test-first commit does not port the unit). Ids on a counted line are found as: explicit tokens and ranges via `expand_unit_ids` of `tools/autopilot/lib/codex-lib.py` (`W8-18l..18n`); lists that follow a `W8-<id>` token (`W8-11a, 11b, 11c`, `W8-13g,13h`, `W8-11d and 11e`), rewritten to full ids before expansion; and bare ids inside parentheses on a line that has a batch token (`feat(W8-B5): ... (18e, 18f, 19b)`). Optional pipeline state files `tools/autopilot/state/*.json` (override `STATE_DIR`): a unit with status done or partial counts only if a non-docs commit on the ref also names it anywhere.

Known limits: the count is a text heuristic. A unit named on a counted line only as a dependency or in a prose list is counted; a unit merged by a squash whose subject and body never name it (for example `feat: Wave 8 B5 ported by Codex pipeline` with an unhelpful body) is missed; a body bullet `feat(W8-x): ...` of a squashed branch counts even if a later commit reverted it; "red tests" bullets are ignored, so a unit that only has red tests stays unmerged. On the current master (147/151): B1 35/35, B2 26/26, B3 22/23, B4 24/24, B5 35/35, B6 5/8. Not attributed: W8-12j (conditional experiment, never started), W8-17e, W8-20a, W8-20b (no code commit names them). B5 35/35 rests on body bullets of the pipeline squashes (#148, #153, #152); the status files say at least 23, the extra units are attributed only by those bullets.

PARITY: if `tools/dev/parity.py` exists it is run as `parity.py --json` (expects `{"Done": n, ...}`); skipped silently otherwise or on error.

Env overrides: `STATUS_JSON` (file, skips status.sh), `STATUS_BIN`, `GIT_BIN`, `PY_BIN`, `PARITY_PY`, `PLAN`, `CODEX_LIB`, `REF` (default origin/master, falls back to HEAD).

Safety: read-only (git log, status.sh); writes only the output file and a mktemp directory it removes; every text from files, commits or status is HTML-escaped; never publishes or uploads anything. A real run takes about 40 s because status.sh queries gh and the process list.

Selftest: `bash tools/dev/test/dashboard-selftest.sh`.
