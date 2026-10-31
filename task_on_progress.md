# Task on progress

**Rule: one line per unit here (status, PR number); details belong in the PR body and `docs/PARITY.md`; new sections only for decisions or incidents.**
Keep this file under about 10 KB. When it grows, move old sections verbatim to `docs/archive/` and add a row to the table below.

## How to resume (read this first)
1. Read `AGENTS.md`, this file and `docs/LESSONS.md`; then `docs/INDEX.md` for the map. Several machines: `docs/MULTI-MACHINE.md` (claims, shared files).
2. Open questions are rows `P-nn` in `docs/PROPOSED-DECISIONS.md` (P-01..P-53, unconfirmed proposals, all acted on). Contracts: `contracts/README.md`. Autopilot: `tools/autopilot/README.md`. Plan and status of the port: `docs/PARITY.md` (`bash tools/dev/parity.sh stats`).
3. Standing user instruction: run the port end to end, auto-fix and auto-merge (squash) every PR, never stop to ask; log open questions as a `P-nn` row and act on the recommended option. Replies in Vietnamese.
4. Some workflow agents are refused `gh pr merge`, branch deletion or publish commands by the permission classifier; the main session then pushes and opens the PR (rules: `tools/autopilot/README.md`).
5. Before planning a unit run `git fetch` and `git log origin/master`: a unit already there is done. Older history is in the archives below, never current state.

## Current state (2026-10-31)

- Snapshot 2026-10-28 (Not done, Blocked, open debts): verbatim in the archive, section "Moved 2026-10-31 (tp-compact5, verbatim)". Re-check `git log origin/master` first.
- 2026-10-31 fix-recycle-c1c2 ([report](docs/reviews/FIX-RECYCLE-C1C2-2026-10-31.md)): C1 not reproduced for directory junctions (`GetVolumePathNameW` follows them, run on this machine; symlinks untestable, no privilege); `query_mount_point` now resolves containing-folder links, fail closed; C2 (progress sink) deferred with design. Stays Partial.
- 2026-10-31 fix-restore-m3 (ST-INVMAP M3, data safety): after an executor panic the catalog entry is restored only if the file still exists on disk; no Undo after a panic (open: journal reconcile). Tests `file_action_core` 68 green, clippy -p pr-app clean; see `docs/reviews/FIX-RESTORE-M3-2026-10-31.md`.

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
- CX-SETUI 2026-10-31: Partial; 32/446 unique mutants evaluated (29 caught, 3 unviable), wrap survivor killed by w8_setui_killers (14 pass); 414 NOT RUN; native preflight 254+81 pass, fmt/clippy/doc-links pass; docs/reviews/CX-SETUI-2026-10-31.md.
- CX-HOSTREST 2026-10-31: Partial; 27/153 current mutants caught (messages 12/33, loop 15/84, swap 0/36 rerun), 126 NOT RUN; 7 killers pass 20x, 88 covering tests pass; docs/reviews/CX-HOSTREST-2026-10-31.md.
- CX-PERFCSV 2026-10-31: tests only; 30/178 distinct (29 caught, 1 unviable), 148 NOT RUN; 1 MISSED + 4 suite timeouts killed on killers alone; 6 killers pass 20x, fmt/clippy/doc-links pass; X-perf-diagnostics/F-23/F-22 write Partial; docs/reviews/CX-PERFCSV-2026-10-31.md.
- CX-SETUI2 2026-10-31: Partial; new 70/446 verdicts (64 caught, 6 unviable, 0 missed/timeouts); groups now fully accounted; inputs 156 + rows 80 + context 87 remain; docs/reviews/CX-SETUI2-2026-10-31.md.
- CX-PERF2 2026-10-31: Partial (CSV live); 107/148 new evaluated (55 caught, 13 missed, 35 unviable, 4 timeout), 41 unresolved; final killers 9 pass 20x; docs/reviews/CX-PERF2-2026-10-31.md.
- ST-PARITY-SYNC2 2026-10-31: docs + native-nightly list only; PARITY synced to #437/#445/#448/#450/#453..#458 (F-18 H1 DIVERGENCE, F-13 load-time refusal, ShellWindow M1/M2/L1, restore 120 s bound, painter/settings/journal/physical numbers), BACKLOG G25 closed; no status change (Done 183, Partial 102); composer binaries added to native-nightly (unconfirmed until one night).
- FIX-READONLY-DELETE 2026-10-31: data-safety fix; permanent delete now refuses read-only files like C# (`DeleteFileW`, was `std::fs::remove_file`); red-then-green test `recycle_bin_readonly_delete`; Q1 (journal default Fast) logged as P-57; docs/reviews/FIX-READONLY-DELETE-2026-10-31.md.
- FIX-DUPCLEAN-M1 2026-10-31: data-safety fix; duplicate cleanup re-stats the whole content group (keeper + copies) after the review dialog and skips a changed/vanished group (DIVERGENCE D-07f-5, C# had no re-check); 3 red-then-green tests; batch Undo still absent (as C#, RV-D5); docs/reviews/FIX-DUPCLEAN-M1-2026-10-31.md.
- FIX-WORKER-PANICS 2026-10-31: reliability fix; a panic in the duplicate-hash job, the folder scan job or the FileHashService worker is now a failure result (gate released, load finished, waiter woken; DIVERGENCE, C# catch/finally); 3 red-then-green tests; presenter/zoom/overlay jobs (M3) left for later; docs/reviews/FIX-WORKER-PANICS-2026-10-31.md.
