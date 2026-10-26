# dashboard.sh

Purpose: write ONE self-contained HTML page (inline CSS and JS, no network, no external resource, light and dark, phone width) with the project state: `tools/dev/status.sh --json` (quota, disk, processes, pipelines, claims, PRs, worktrees), Wave 8 progress and optional PARITY statuses.

Usage: `bash tools/dev/dashboard.sh [--out FILE]` (default `tools/dev/out/dashboard.html`, git-ignored via `tools/dev/.gitignore`). Prints the path, size and merged/total units.

Wave progress: units are the `W8-*` rows of `docs/WAVE-8-PLAN.md` (batch B1..B6); a unit counts as merged when `origin/master` has a commit whose subject starts with feat/port/fix/test/refactor/perf (optional scope) or `Merge W8-` and names it (ranges expanded by `expand_unit_ids` of `tools/autopilot/lib/codex-lib.py`). docs/chore subjects do not count.

PARITY: if `tools/dev/parity.py` exists it is run as `parity.py --json` (expects `{"Done": n, ...}`); skipped silently otherwise or on error.

Env overrides: `STATUS_JSON` (file, skips status.sh), `STATUS_BIN`, `GIT_BIN`, `PY_BIN`, `PARITY_PY`, `PLAN`, `CODEX_LIB`, `REF` (default origin/master, falls back to HEAD).

Safety: read-only (git log, status.sh); writes only the output file and a mktemp directory it removes; every text from files, commits or status is HTML-escaped; never publishes or uploads anything. A real run takes about 40 s because status.sh queries gh and the process list.

Selftest: `bash tools/dev/test/dashboard-selftest.sh`.
