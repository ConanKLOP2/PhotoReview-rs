# Task on progress

**Rule: one line per unit here (status, PR number); details belong in the PR body and `docs/PARITY.md`; new sections only for decisions or incidents.**
Keep this file under about 10 KB. When it grows, move old sections verbatim to `docs/archive/` and add a row to the table below.

## How to resume (read this first)
1. Read `AGENTS.md`, this file and `docs/LESSONS.md`; then `docs/INDEX.md` for the map. Several machines: `docs/MULTI-MACHINE.md` (claims, shared files).
2. Open questions are rows `P-nn` in `docs/PROPOSED-DECISIONS.md` (unconfirmed proposals that were acted on; a row whose Status column says OPEN still waits for the owner). Contracts: `contracts/README.md`. Autopilot: `tools/autopilot/README.md`. Plan and status of the port: `docs/PARITY.md` (`bash tools/dev/parity.sh stats`).
3. Standing user instruction: run the port end to end, auto-fix and auto-merge (squash) every PR, never stop to ask; log open questions as a `P-nn` row and act on the recommended option. Replies in Vietnamese.
4. Some workflow agents are refused `gh pr merge`, branch deletion or publish commands by the permission classifier; the main session then pushes and opens the PR (rules: `tools/autopilot/README.md`).
5. Before planning a unit run `git fetch` and `git log origin/master`: a unit already there is done. Older history is in the archives below, never current state.

## Current state (2026-11-10)

- P-396 2026-11-10: closed by #698 (dead `wait_timeout_ms` try_from fallback removed, no timeout change).
- Snapshot 2026-10-28 (Not done, Blocked, open debts): verbatim in the archive, section "Moved 2026-10-31 (tp-compact5, verbatim)". Re-check `git log origin/master` first.
- ST-LOOP10 2026-11-10: Codex-first routing rule removed (#692, #693; Codex is an optional lane). Mutants closed by no-behaviour-change refactors or seams: P-396 (#698), P-397 (#694), 232:38 (#697), 562:68 (#696), icons 30:16 (#695). Still open: 679:13/680:13 (needs real WIC). Not machine-closable: wic/interop.rs, recycle_bin*.rs, tool_path_guard 299, LibRaw try_set_use_camera_wb. Cleanup pending: 7 merged worktrees (~19 GB) plus cl-mut-settings3 and fix-g16-g23 need `janitor.sh --apply`, denied by the permission classifier.
- ST-LOOP11 2026-11-10 (docs only, branch docs/st-loop11-hygiene, PR_NUM): stale status text fixed (P-396 #698, P-393 #681/#682); closure notes for 170:72, 792:9, 88:13, 232:38, 562:68 and icons 30:16 in PARITY and P3-MUT; journal_line and recovery_retry NOT RUN lists marked superseded (journal_line survivors have N1 run proofs, only recovery_retry 375:13 has none); X-stj-reader Done under rule B (N1 312/312, PARITY Done 198 to 199); P-370 per-family owner table drafted as an unconfirmed proposal (G17-IGNORED-TESTS section 8); 8 doc-links glob warnings fixed. Not done here: P-393 tool_path_guard 299, LibRaw try_set_use_camera_wb (owner or DLL), MUT-2..6 and other parity-partial tasks (other agents).

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
| [`task_on_progress-st-loop4-5.md`](docs/archive/task_on_progress-st-loop4-5.md) | 2026-11-06..10 | Section "Moved 2026-11-10 (st-loop11, verbatim)": ST-LOOP8 and ST-LOOP9 status lines (their open items are listed in ST-LOOP10 above) |
