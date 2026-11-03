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
- ST-LOOP1 2026-11-03: #556 (settings killers, clippy `type_complexity` fixed), #561 (X-exe-icon Done), #562 (P-71, P-72), #563 (G19 cross-drive Move fault tests), #564 (`.claude/settings.json`), #565 (G21..G23 recheck, P-73), #566 (G9 L1 presenter borrow released before `open_url`), #567 (AGENTS.md "Sizing parallel agents", no fixed lane limit), #568 (G17 audit: 1548 `#[ignore]`, 1272 run in no workflow, P-80) merged. G9 M2 fixed by #536, M3 guarded by the dialog host (no code change). `parity.sh stats` after #561: 361 status rows, Done 184, Partial 102, Todo 3; `reconcile` flags 51 rows, none inconsistent. Open for the owner: P-71, P-72, P-73, P-80. Remote branches of the merged PRs are not deleted (needs the owner).
- ST-LOOP2 2026-11-03: #569 (this sync), #570 (ADR-D settings backup failure, P-66 `Different` row), #571 (G28 short-write test), #572 (P-74, G14 L3 candidate fix `d646739` kept local), #573 (X-embedded-thumbnail-backend Todo -> Partial), #574 (SEC-01 killer), #575 (G12 L5) merged. AR16 RemovePaths mutation (cargo-mutants 27.1.0): 24 run, 20 caught, 3 equivalent, 1 unviable ([report](docs/reviews/AR16-REMOVEPATHS-MUTATION-2026-11-03.md)). X-render-surface: its 3 survivors need a desktop (no change). Open for the owner: P-71..P-74, P-80; #576 (G16 L8) merged; `gh pr merge` is allowed in `.claude/settings.json` by the owner.
- ST-LOOP3 2026-11-03: #576 (G16 L8 quoted codex-lib), #578 (`gh pr merge` allowed in `.claude/settings.json`), #579 (owner decisions), #580 (P-84), #581 (P-80 nightly evidence, P-86), #582 (P-88, P-89), #583 (X-stj-reader: 8 of 8 former timeouts caught by a 10 s bounded wait), #584 (P-71 option C: `GateToken`, no public `end`), #585 (SEC-01 mutation: 27 run, 17 caught, 10 survivors explained) merged. Draft PR ConanKLOP2/PhotoReview#536 (fragments RP-GATE-END-01, RP-JOURNAL-NOTICE-01, RP-SETTINGS-BACKUP-01) asks the C# owner to review the same three decisions in the original app; the C# repo is now cloned at ../PhotoReview (baseline `7eaee2a2`), no .NET SDK 10.0.401 yet. Open for the owner: P-72 (blocked by P-84), P-73 (needs a C# oracle run), P-74, the P-80 reason-prefix sub-question, P-86, P-88, P-89.
- ST-LOOP4 2026-11-03: #590 (P-73 A), #591 (baseline b6847767), #592 (P-72 C / P-84 A journal notice), #593 to #597 merged (mutation evidence: tiff_nav 49/52 caught, perf-diagnostics 11/11, physical listing 0 survivors, journal_line 92/136, recovery_retry 54/160; the 44 `journal_line` and about 106 `recovery_retry` group-path mutants are still NOT run). C# draft PR ConanKLOP2/PhotoReview#538 (register a late single result for Undo) waits for the owner; after it merges, sync the baseline and make the Rust single-late path register Undo. Open owner decisions: P-92, P-86, P-88, P-89, P-74.

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
