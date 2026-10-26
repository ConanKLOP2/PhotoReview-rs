# Task on progress

**Rule: one line per unit here (status, PR number); details belong in the PR body and `docs/PARITY.md`; new sections only for decisions or incidents.**
Keep this file under about 10 KB. When it grows, move old sections verbatim to `docs/archive/` and add a row to the table below.

## How to resume (read this first)
1. Read `AGENTS.md`, this file and `docs/LESSONS.md`; then `docs/INDEX.md` for the map. Several machines: `docs/MULTI-MACHINE.md` (claims, shared files).
2. Open questions are rows `P-nn` in `docs/PROPOSED-DECISIONS.md` (P-01..P-52, unconfirmed proposals, all acted on). Contracts: `contracts/README.md`. Autopilot: `tools/autopilot/README.md`. Plan and status of the port: `docs/PARITY.md` (`bash tools/dev/parity.sh stats`).
3. Standing user instruction: run the port end to end, auto-fix and auto-merge (squash) every PR, never stop to ask; log open questions as a `P-nn` row and act on the recommended option. Replies in Vietnamese.
4. Some workflow agents are refused `gh pr merge`, branch deletion or publish commands by the permission classifier; the main session then pushes and opens the PR (rules: `tools/autopilot/README.md`).
5. Before planning a unit run `git fetch` and `git log origin/master`: a unit already there is done. Older history is in the archives below, never current state.

## Current state (checked 2026-10-26 against `origin/master` 8b93fbf; re-check with `git log origin/master` before relying on it)
- **Merged:** Wave 8 B6 (#164), B5 dialogs and startup (#223-#231: startup policies, lazy graph, zoom/diagnostics, Action Profiles, benchmark, Recovery, Settings window, secondary window routing). Wave 9 external editors E1-E9: E1 #233, E3 #234, E2 #235, E7 #236, E4 #237, E6 #238, E5 #239, E8 #254, E9 #256. Root repairs: #240 preview registration, #255 viewer borrow in zoom/menu, #260 shell handler lifetime. Dev tooling #242-#258 (status, janitor, hooks, agents, dashboard, triage, mutants, parity, oracle, native-tests, unsafe-audit).
- **PARITY.md** (`parity.sh stats`, 349 status rows): Done 163, Partial 89, Ported 41, Todo 18, Superseded 17, Different 13, Doing 3, Implementation 3, Contract 2.
- **Open PRs:** none at the time of writing (`gh pr list --state open` returned an empty list).
- **In progress, native input fixture:** A claim `FIX-native-input-fixture`, branch `codex/fix-native-input-fixture`; source `34bf37b` fmt12/Clippy/workspace 9862 pass, 0 fail, 1331 ignored; combined regression 101/101 pass (resource-less icon branch unexercised). Docs-only master `4006635` integrated; final head gate/PR pending. Full startup/corpus/mutation limits stay in PARITY; [history](docs/archive/task_on_progress-native-input-fixture.md).
- **In progress, rename R1:** A claim `W9-R1`, branch `codex/w9-rename-policy`; 53 G/F functions and pre-latest-master gate on `8161e88` 9904 pass, 0 fail, 1328 ignored; integrate merged fixture/master, final gates and PR, then selected postmerge mutation. R2-R11 not started; roster #241 is on master.
- **Not done (PARITY rows):** `X-exe-entry` (W8-20a, `Contract`) and `X-ui-smoke` (W8-20b, `Contract`): no `PhotoReviewRs.exe` window yet, the wave stays open; `X-startup-composition-root` is `Partial` (W8-17c production factory adapter, W8-18w); W8-12j swap-chain experiment is conditional and not started; `X-attended-remeasure` is `Todo` (needs an attended session on a real screen); `X-benchmark-compare` and `X-benchmark-parity-harness` are `Todo` (W10-B1).
- **Blocked:** nothing known to be blocked on another machine. Needs the human: attended measurements (W8-12j / W10-B2), dependency changes (`Cargo.toml`), unconfirmed `P-nn` rows.
- **Open debts (index):**
  - Wave 5: native LibRaw/TurboJPEG DLL tests, RAW corpus tests and the historical mutation survivor set are unverified (contracts W10-N1..N4, PARITY rows `X-raw-*`; `docs/archive/task_on_progress-wave5.md`).
  - Wave 6: mutation verdicts and native evidence for the platform units were not recorded (`docs/archive/task_on_progress-wave6.md`).
  - Wave 7: `cli-process` golden and `cargo-mutants` on the merged tree not run (`docs/archive/task_on_progress-wave7.md`).
  - Wave 8: B2 items D-W8B2-4/5/6 (`docs/archive/task_on_progress-wave8-b1-b2.md`); B3/B4 `Partial` rows 13b-13e, no hardware (non-WARP) GPU run, native evidence is hidden-window/WARP only (`docs/archive/task_on_progress-wave8-status-contracts-reviews.md`).
  - Native composition glue is excluded from full cargo-mutants by policy; Wave 8 B4-B6 and Wave 9 mutation evidence is partial per PR body.
  - Local merged-worktree cleanup was retained by machine A after an approval rejection (2026-10-26 sections); current worktree state unverified (use `/janitor`).
- Last verified full gate recorded in the archive (2026-10-26, before #260): workspace tests 9862 passed, 0 failed, 1331 ignored, clippy `-D warnings` clean. Not re-run for this file.

## Documentation link audit (2026-10-26)
- Default-scope doc-links: 13 inherited missing path references verified against tracked baseline `4006635`; new task/archive targets exist and are not among its errors. Docs check FAIL remains a tooling/docs follow-up; [exact evidence](docs/archive/task_on_progress-native-input-fixture.md). Native Rust gates are recorded separately.

## Archives (`docs/archive/`, verbatim, history only)
Find any section title with `grep -n '^## ' <file>`. Original section order is kept inside each file.

| File | Period | Holds |
|---|---|---|
| [`task_on_progress-wave5.md`](docs/archive/task_on_progress-wave5.md) | 2026-10-17..18 | Wave 5 (RAW, TurboJPEG, LibRaw): units, mutation ledger, integration |
| [`task_on_progress-wave6.md`](docs/archive/task_on_progress-wave6.md) | 2026-10-18 | Wave 6 (platform units) |
| [`task_on_progress-wave7.md`](docs/archive/task_on_progress-wave7.md) | 2026-10-18 | Wave 7 (benchmarking): readiness inventory, contracts PR and integration |
| [`task_on_progress-wave8-b1-b2.md`](docs/archive/task_on_progress-wave8-b1-b2.md) | 2026-10-18..19 | Wave 8 B1 and B2 entries |
| [`task_on_progress-wave8-status-contracts-reviews.md`](docs/archive/task_on_progress-wave8-status-contracts-reviews.md) | 2026-10-21..23 | PR #164 conflict note, old "How to resume" and "Open debts", Wave 8 status (B1-B3), Wave 8 B4 status, B5 and B6 contracts, Wave 9 sync contracts (EXTERNAL-EDITORS UI, FEAT-RENAME), machine C reviews (INVARIANTS, BACKLOG, PRS, B4, W9-CONTRACTS, TEST-PLAN, INVARIANTS-STORAGE, RECOVERY-FLOW, HOST-DISPATCHER, RECYCLE-POLICY, BACKLOG-CLOSEOUT) |
| [`task_on_progress-wave8-b5-b6.md`](docs/archive/task_on_progress-wave8-b5-b6.md) | 2026-10-23..25 | Wave 8 B6 machine B (3 sections); Wave 8 B5 machine A: W8-18p, 18q, 18r/s, 19j-m, 18t/u/v, 18o, Settings checkpoint, focused verification, merge and secondary port verification |
| [`task_on_progress-wave9-e1-e7.md`](docs/archive/task_on_progress-wave9-e1-e7.md) | 2026-10-25 | Wave 9 external editors W9-E1, E3, E2, E4, E7, E5, E6 |
| [`task_on_progress-wave9-e5-e9-and-repairs.md`](docs/archive/task_on_progress-wave9-e5-e9-and-repairs.md) | 2026-10-25..26 | W9-E8 secondary port, CI preview single-flight repair, E5/E8/E9 integration gates, popup evidence, native menu zoom root repair, shell handler lifetime root repair, rename ownership |
| [`task_on_progress-native-input-fixture.md`](docs/archive/task_on_progress-native-input-fixture.md) | 2026-10-26 | Native pixel/initial-view/frame/latency fixture, baseline comparisons, lifetime dependency and Background wait incident |
