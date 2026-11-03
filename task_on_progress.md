# Task on progress

**Rule: one line per unit here (status, PR number); details belong in the PR body and `docs/PARITY.md`; new sections only for decisions or incidents.**
Keep this file under about 10 KB. When it grows, move old sections verbatim to `docs/archive/` and add a row to the table below.

## How to resume (read this first)
1. Read `AGENTS.md`, this file and `docs/LESSONS.md`; then `docs/INDEX.md` for the map. Several machines: `docs/MULTI-MACHINE.md` (claims, shared files).
2. Open questions are rows `P-nn` in `docs/PROPOSED-DECISIONS.md` (P-01..P-68, unconfirmed proposals, all acted on). Contracts: `contracts/README.md`. Autopilot: `tools/autopilot/README.md`. Plan and status of the port: `docs/PARITY.md` (`bash tools/dev/parity.sh stats`).
3. Standing user instruction: run the port end to end, auto-fix and auto-merge (squash) every PR, never stop to ask; log open questions as a `P-nn` row and act on the recommended option. Replies in Vietnamese.
4. Some workflow agents are refused `gh pr merge`, branch deletion or publish commands by the permission classifier; the main session then pushes and opens the PR (rules: `tools/autopilot/README.md`).
5. Before planning a unit run `git fetch` and `git log origin/master`: a unit already there is done. Older history is in the archives below, never current state.

## Current state (2026-10-31)

- Snapshot 2026-10-28 (Not done, Blocked, open debts): verbatim in the archive, section "Moved 2026-10-31 (tp-compact5, verbatim)". Re-check `git log origin/master` first.
- ST-LOOP4 2026-11-03: #590 (P-73 A), #591 (baseline b6847767), #592 (P-72 C / P-84 A journal notice), #593 to #601 merged (#598 README links, #599 G20 Rename: 115/115 C# test names already had a Rust twin, 3 stricter twins added, #600 journal_line 136/136 non-glue, #601 recovery_retry group path 128/160 measured), G28 seam tests (cross-drain duplication = P-140) (mutation evidence: tiff_nav 49/52 caught, perf-diagnostics 11/11, physical listing 0 survivors, journal_line 92/136, recovery_retry 54/160; the 44 `journal_line` and about 106 `recovery_retry` group-path mutants are still NOT run). C# draft PR ConanKLOP2/PhotoReview#538 (register a late single result for Undo) waits for the owner; after it merges, sync the baseline and make the Rust single-late path register Undo. Open owner decisions: P-92, P-86, P-88, P-89, P-74.
- ST-LOOP5 2026-11-03: #598 to #610 merged. Evidence/docs: G28 seam tests (P-140), G29 mostly closed in #474 (P-150), G30 done in #532, G31 helper still unwired (P-160), G21 corrected, G18 L2..L4 tests (P-170). C# #538 (merged by the owner) ported with the baseline move to e7c2fcd8 (#608: late single result registers Undo). On the owner's "port both": #609 ports C# #534 D1 (group Recycle retry re-stat, the re-stat already existed Rust-only; golden `recovery-retry` replayed unfiltered), #610 ports C# #535 D2 (`find_groups`, hashed stats, `core.duplicate.fileChanged`) and removes the Rust-only D-07f-5 / DV-7 pre-check (to be confirmed, see P-191). Open owner decisions: P-74, P-86, P-88, P-89, P-92, P-110, P-111, P-140, P-150, P-160, P-170, and the D-07f-5 removal.
- ST-LOOP6 2026-11-04: owner decisions of 2026-11-03 recorded. Re-checks #612..#621, decisions #622 to #626 and #629 to #635 merged; #627 (P-92 A) and #628 (P-321) were open at the time of writing. New rows P-300..P-390 in `docs/PROPOSED-DECISIONS.md`, BACKLOG G1..G31 table, PARITY wording in [`PARITY-WORDING-OWNER-DECISIONS`](docs/reviews/PARITY-WORDING-OWNER-DECISIONS-2026-11-03.md) (PARITY.md not touched). Open: P-140, P-311, P-370 owners, P-86 trial night; P-330 Rust part and the P-331 C# merge wait for the local, unpushed C# branches `a3dc30cc` and `396d5ac2`.

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
