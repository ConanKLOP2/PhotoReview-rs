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
- CL-REST2RERUN 2026-10-31: correct-flag rerun: initial_viewport 41/41 caught, benchmark `dispose` killers confirmed by hand (2 mutants kill both tests), adapters.rs really 22 caught + 7 MISSED + 5 unviable (CL-REST2's 0 missed was void; killed by CX-TEXTS w8_texts_killers); no row raised; docs/reviews/CL-REST2RERUN-2026-10-31.md.
- CX-JRNL2 2026-10-31: tests only; 26 new caught, combined 48/179; lane ran 33 of 179 (31 caught, 2 source-unviable); four dismiss field deletions MISSED in the narrow killers-only binary but caught in the full suite; 127 original NOT RUN remain; native reconcile concurrency preflight failed, rows Partial; docs/reviews/CX-JRNL2-2026-10-31.md.
- CL-JRNL3 2026-10-31: journal mutants rerun (docs/reviews/CL-JRNL3-2026-10-31.md): 22 decided of 127 NOT RUN (16 caught incl. as_str survivor killed by w3_jrnl_killers7, 4 timeout, 2 unviable); combined 64/179 caught, 105 NOT RUN; journal rows stay Partial.
- CL-PHYS3 2026-10-31: physical.rs +43 mutants (26 caught, 17 missed = 8 equivalent + 9 real gaps now killed by w2_phys3_killers); ~395 not run; X-physical-fs/AR16 Partial; docs/reviews/CL-PHYS3-2026-10-31.md.
- CL-TEXTS2 2026-10-31: texts.rs remaining mutants rerun with correct flags: 122 + 2 re-checked caught, 0 missed/timeouts, localizer unviable; all 161 texts mutants accounted for; X-composition-texts-adapters stays Partial (adapters gaps); docs/reviews/CL-TEXTS2-2026-10-31.md.
- CL-PHYS4 2026-10-31: physical.rs pattern group 115 of 438 run (flag form verified): 92 caught, 20 missed (16 real gaps killed by w2_phys4_killers, 4 equivalent), 3 TIMEOUT (infinite loops, counted undetected); 323 of 438 not run by this lane, X-physical-fs/AR16 stay Partial; docs/reviews/CL-PHYS4-2026-10-31.md.
- CL-PHYS5 2026-10-31: physical.rs groups create_directory_impl/is_device/open_directory_handle/prefix helpers: 96 run (51 caught, 40 missed, 2 unviable, 3 timeout), killers w2_phys5_killers (incl. fail-fast match_pattern guard); about 227 unrun; rows stay Partial. Evidence docs/reviews/CL-PHYS5-2026-10-31.md.
- CX-SETUI 2026-10-31: Partial; 32/446 unique mutants evaluated (29 caught, 3 unviable), wrap survivor killed by w8_setui_killers (14 pass); 414 NOT RUN; native preflight 254+81 pass, fmt/clippy/doc-links pass; docs/reviews/CX-SETUI-2026-10-31.md.
- CX-HOSTREST 2026-10-31: Partial; 27/153 current mutants caught (messages 12/33, loop 15/84, swap 0/36 rerun), 126 NOT RUN; 7 killers pass 20x, 88 covering tests pass; docs/reviews/CX-HOSTREST-2026-10-31.md.
- CX-PERFCSV 2026-10-31: tests only; 30/178 distinct (29 caught, 1 unviable), 148 NOT RUN; 1 MISSED + 4 suite timeouts killed on killers alone; 6 killers pass 20x, fmt/clippy/doc-links pass; X-perf-diagnostics/F-23/F-22 write Partial; docs/reviews/CX-PERFCSV-2026-10-31.md.
