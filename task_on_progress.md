# Task on progress

**Rule: one line per unit here (status, PR number); details belong in the PR body and `docs/PARITY.md`; new sections only for decisions or incidents.**
Keep this file under about 10 KB. When it grows, move old sections verbatim to `docs/archive/` and add a row to the table below.

## How to resume (read this first)
1. Read `AGENTS.md`, this file and `docs/LESSONS.md`; then `docs/INDEX.md` for the map. Several machines: `docs/MULTI-MACHINE.md` (claims, shared files).
2. Open questions are rows `P-nn` in `docs/PROPOSED-DECISIONS.md` (unconfirmed proposals that were acted on; a row whose Status column says OPEN still waits for the owner). Contracts: `contracts/README.md`. Autopilot: `tools/autopilot/README.md`. Plan and status of the port: `docs/PARITY.md` (`bash tools/dev/parity.sh stats`).
3. Standing user instruction: run the port end to end, auto-fix and auto-merge (squash) every PR, never stop to ask; log open questions as a `P-nn` row and act on the recommended option. Replies in Vietnamese.
4. Some workflow agents are refused `gh pr merge`, branch deletion or publish commands by the permission classifier; the main session then pushes and opens the PR (rules: `tools/autopilot/README.md`).
5. Before planning a unit run `git fetch` and `git log origin/master`: a unit already there is done. Older history is in the archives below, never current state.

## Current state (2026-11-11)

- P-396 2026-11-10: closed by #698 (dead `wait_timeout_ms` try_from fallback removed, no timeout change).
- Snapshot 2026-10-28 (Not done, Blocked, open debts): verbatim in the archive, section "Moved 2026-10-31 (tp-compact5, verbatim)". Re-check `git log origin/master` first.
- ST-LOOP12 2026-11-10 (docs only, branch docs/owner-decisions-1110, #708): owner decisions recorded as P-398..P-403. P-402 (679:13/680:13 stay open, reason recorded) and P-403 (wic/interop.rs and recycle_bin*.rs excluded from the mutation gate, native tests instead; ADR-D-2026-11-10-mutation-gate-exclusions, AGENTS.md note) are not to be reopened. MERGED since: P-398 (tool_path_guard replace refusal gated to Windows, twin removed, #709, [P393-GUARD](docs/reviews/P393-GUARD-2026-11-10.md)), P-400 (redundant guards removed in arrow_pan/layout/viewer_state, #710 and #711), P-401 (Settings glue mutation PARTIAL run, 79 of 846 identities, flaky GUI-count test fixed, #712, [D10-SETTINGS-GLUE](docs/reviews/D10-SETTINGS-GLUE-2026-11-10.md)), P-399 (LibRaw version-gate seam closes try_set_use_camera_wb without a second DLL, #713, [P399-MUT](docs/reviews/P399-MUT-2026-11-11.md)). Worktrees d10-settings and p399 were merged and removed by janitor on 2026-11-11.
- ST-LOOP13 2026-11-11: units merged, closeout pending. P-404 (DONE) units #715 win-seams ([WIN-SEAMS](docs/reviews/WIN-SEAMS-2026-11-11.md)), #716 bench-presenter, #717 tpg-const, #718 pixels-row, #719 raw-guards, #720 raw-exif-strict, #721 tj-plan; closeout on branch refactor/compaction-flag-consts: `journal_compaction_files.rs` verdict decided ([COMPACTION-FILES-MUT](docs/reviews/COMPACTION-FILES-MUT-2026-11-11.md): 100 mutants, 79 caught, 13 unviable, 3 equivalent, 4 not compiled on Windows, 1 removed by refactor) and PARITY synced; the five journal rows stay Partial (rule B: 3 equivalents argued only, 4 non-Windows twins undecided, final source rerun: 441:5, 453:5 only). Unit worktrees removed (comp-closeout after merge). Still open after this loop: D-10 Settings glue 767 of 846 NOT RUN, physical.rs remainder, pr-app Ported-unmutated rows, overlay rows, 679:13/680:13 (P-402), benchmark.rs 832/833 cancel poll, win-seams 12 pre-existing `Url::parse` mutants not run, selector 376:22 (pre-existing). Owner-blocked: attended runs, RAW corpus, release review, origin/ci/ci-v3-affected branch decision. The six 'failed' units in status.sh (W8-04e, 10b, 08b, 18q, 18u, 19j) are stale local state, all in master.
- ST-LOOP14 2026-11-11 (branch test/d10-settings-glue-mut): D-10 Settings glue, 4 of 7 files decided ([D10-GLUE-MUT](docs/reviews/D10-GLUE-MUT-2026-11-11.md): 446 mutants, 372 caught, 38 unviable, 36 equivalent by argument). Open: `dialog/host.rs` + `dialog/surface.rs` (native lane running, TEMP `mut-d10-c`), `dialog/frame.rs` (190, not started; one native lane at a time).

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
| [`task_on_progress-w9-r1-rename-policy.md`](docs/archive/task_on_progress-w9-r1-rename-policy.md) | 2026-10-26 | R1 policy/plan implementation, oracle and focused evidence, intermediate integration gates |

| [`task_on_progress-w9-r1-r4-merged.md`](docs/archive/task_on_progress-w9-r1-r4-merged.md) | 2026-10-27 | Verbatim merged R1-R4 implementation, selected mutation, native/source gate, claim and cleanup status before compaction |

| [`task_on_progress-status-2026-10-28-30.md`](docs/archive/task_on_progress-status-2026-10-28-30.md) | 2026-10-28..30 | cx-* lane status lines (CX-IMAGING2, cx-imaging, cx-host2, cx-core2, cx-json2, cx-release), W9-R1..R9 merged/in-progress lines, native input fixture and benchmark repair status, exe runs 2026-10-29, zoom-detail-wire |
| [`task_on_progress-status-2026-10-28-30.md`](docs/archive/task_on_progress-status-2026-10-28-30.md) | 2026-10-30 | Section "Moved 2026-10-30 (lane status lines, verbatim)": CX-MUTW4, CX-IMAGING3, CL-UNDO, CL-ENTRY, CL-APPMUT, CL-FSFLUSH, CL-PIXELS, CX-JSON3, CX-UNDOSTAT lane status lines |
| [`task_on_progress-doc-link-audit.md`](docs/archive/task_on_progress-doc-link-audit.md) | 2026-10-26 | Verbatim documentation link audit before compaction |
| [`task_on_progress-status-2026-10-28-30.md`](docs/archive/task_on_progress-status-2026-10-28-30.md) | 2026-10-28..31 | Section "Moved 2026-10-31 (tp-compact5)": 2026-10-28 state snapshot, CL-INPUTDONE..CL-FSRERUN status lines |
| [`task_on_progress-status-2026-10-28-30.md`](docs/archive/task_on_progress-status-2026-10-28-30.md) | 2026-10-31 | Section "Moved 2026-10-31 (st-parity-sync3, verbatim)": CX-SETUI, CX-HOSTREST, CX-PERFCSV, CX-SETUI2, CX-PERF2 status lines |
| [`task_on_progress-status-2026-10-28-30.md`](docs/archive/task_on_progress-status-2026-10-28-30.md) | 2026-10-31 | Section "Moved 2026-10-31 (st-parity-sync4, verbatim)": ST-PARITY-SYNC2, FIX-READONLY-DELETE, FIX-DUPCLEAN-M1 status lines |
| [`task_on_progress-status-2026-10-28-30.md`](docs/archive/task_on_progress-status-2026-10-28-30.md) | 2026-10-31 | Section "Moved 2026-10-31 (st-parity-sync5, verbatim)": FIX-WORKER-PANICS and ST-PARITY-SYNC3 status lines |
| [`task_on_progress-status-2026-10-28-30.md`](docs/archive/task_on_progress-status-2026-10-28-30.md) | 2026-10-31 | Section "Moved 2026-10-31 (st-parity-sync6, verbatim)": FIX-RECYCLE-C1C2 and FIX-RESTORE-M3 status lines |
| [`task_on_progress-status-2026-10-28-30.md`](docs/archive/task_on_progress-status-2026-10-28-30.md) | 2026-10-31 | Section "Moved 2026-10-31 (st-parity-sync7, verbatim)": ST-PARITY-SYNC4 and ST-PARITY-SYNC5 status lines |
| [`task_on_progress-status-2026-10-28-30.md`](docs/archive/task_on_progress-status-2026-10-28-30.md) | 2026-10-31 | Section "Moved 2026-10-31 (st-parity-sync8, verbatim)": ST-PARITY-SYNC6 and ST-PARITY-SYNC7 status lines |
| [`task_on_progress-status-2026-10-28-30.md`](docs/archive/task_on_progress-status-2026-10-28-30.md) | 2026-10-31..11-01 | Section "Moved 2026-11-03 (st-loop3, verbatim)": ST-PARITY-SYNC8, SYNC9, SYNC10, ST-DECISIONS-CONFIRM, SYNC11 status lines |
| [`task_on_progress-status-2026-10-28-30.md`](docs/archive/task_on_progress-status-2026-10-28-30.md) | 2026-11-03 | Section "Moved 2026-11-03 (st-loop5, verbatim)": ST-LOOP1 and ST-LOOP2 status lines |
| [`task_on_progress-status-2026-10-28-30.md`](docs/archive/task_on_progress-status-2026-10-28-30.md) | 2026-11-03 | Section "Moved 2026-11-04 (st-loop6, verbatim)": ST-LOOP3 status line |
| [`task_on_progress-st-loop4-5.md`](docs/archive/task_on_progress-st-loop4-5.md) | 2026-11-03..05 | ST-LOOP4 to ST-LOOP7 status lines (#590..#660) |
| [`task_on_progress-st-loop4-5.md`](docs/archive/task_on_progress-st-loop4-5.md) | 2026-11-06..10 | Section "Moved 2026-11-10 (st-loop11, verbatim)": ST-LOOP8 and ST-LOOP9 status lines (their open items are listed in ST-LOOP13 above) |
| [`task_on_progress-st-loop4-5.md`](docs/archive/task_on_progress-st-loop4-5.md) | 2026-11-10 | Section "Moved 2026-11-11 (st-loop13, verbatim)": ST-LOOP10 and ST-LOOP11 status lines (open items now in ST-LOOP13 above) |
