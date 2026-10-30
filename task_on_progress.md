# Task on progress

**Rule: one line per unit here (status, PR number); details belong in the PR body and `docs/PARITY.md`; new sections only for decisions or incidents.**
Keep this file under about 10 KB. When it grows, move old sections verbatim to `docs/archive/` and add a row to the table below.

## How to resume (read this first)
1. Read `AGENTS.md`, this file and `docs/LESSONS.md`; then `docs/INDEX.md` for the map. Several machines: `docs/MULTI-MACHINE.md` (claims, shared files).
2. Open questions are rows `P-nn` in `docs/PROPOSED-DECISIONS.md` (P-01..P-53, unconfirmed proposals, all acted on). Contracts: `contracts/README.md`. Autopilot: `tools/autopilot/README.md`. Plan and status of the port: `docs/PARITY.md` (`bash tools/dev/parity.sh stats`).
3. Standing user instruction: run the port end to end, auto-fix and auto-merge (squash) every PR, never stop to ask; log open questions as a `P-nn` row and act on the recommended option. Replies in Vietnamese.
4. Some workflow agents are refused `gh pr merge`, branch deletion or publish commands by the permission classifier; the main session then pushes and opens the PR (rules: `tools/autopilot/README.md`).
5. Before planning a unit run `git fetch` and `git log origin/master`: a unit already there is done. Older history is in the archives below, never current state.

## Current state (checked 2026-10-28 against immutable master `4af47345`; re-check with `git log origin/master` before relying on it)
- **Not done (PARITY rows):** `X-exe-entry` and `X-ui-smoke` stay open: icon (#298), `exe_process`/`startup_run_native`/`startup_native`/`settings_dialog_frame_e2e` proof files (guard `every_named_proof_file_exists` fails until they exist), icon-less `X-exe-*` rows; `X-startup-composition-root` is `Partial`; W8-12j swap-chain experiment not started; `X-attended-remeasure` is `Todo` (attended session needed); `X-benchmark-compare` and `X-benchmark-parity-harness` are `Todo` (W10-B1). `claim.sh gc` removes claims by unit name found in master history, so do not run it until fixed.
- **Blocked:** nothing known to be blocked on another machine. Needs the human: attended measurements (W8-12j / W10-B2), dependency changes (`Cargo.toml`), unconfirmed `P-nn` rows.
- **Open debts (index):**
  - Wave 5: native LibRaw/TurboJPEG DLL tests, RAW corpus tests and the historical mutation survivor set are unverified (contracts W10-N1..N4, PARITY rows `X-raw-*`; `docs/archive/task_on_progress-wave5.md`).
  - Wave 6: mutation verdicts and native evidence for the platform units were not recorded (`docs/archive/task_on_progress-wave6.md`).
  - Wave 7: `cli-process` golden and `cargo-mutants` on the merged tree not run (`docs/archive/task_on_progress-wave7.md`).
  - Wave 8: B2 items D-W8B2-4/5/6 (`docs/archive/task_on_progress-wave8-b1-b2.md`); B3/B4 `Partial` rows 13b-13e, no hardware (non-WARP) GPU run, native evidence is hidden-window/WARP only (`docs/archive/task_on_progress-wave8-status-contracts-reviews.md`).
  - Native composition glue is excluded from full cargo-mutants by policy; Wave 8 B4-B6 and Wave 9 mutation evidence is partial per PR body.
  - cl-input 2026-10-30: `click_counter.rs` 20/20 caught; `dispatch.rs` 22 of 83 subset caught, 0 missed; input/menu rows stay Partial (`docs/reviews/CL-INPUT-2026-10-30.md`). Settings dialog line moved verbatim to the archive.
  - Local merged-worktree cleanup was retained by machine A after an approval rejection (2026-10-26 sections); current worktree state unverified (use `/janitor`).

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
- CL-INPUTDONE 2026-10-30: X-input-drop-target Done (47/47 mutants, 6 survivors killed, 1 equivalent); X-input-native, -message-handlers, -viewport-surface lowered to Partial (native 58/92 evaluated, 0 missed except 1 equivalent + the touchpad branch; handlers 27/42, surface 34/126 evaluated, 0 missed); beware cargo-mutants `-- --include-ignored` makes every mutant "caught", use `--cargo-test-arg=`; docs/reviews/CL-INPUTDONE-2026-10-30.md.
- CL-REST1 2026-10-30: pr-platform-win popup_host 63 (31 caught, 31 missed = native show path, no GUI run), windowing tail 85 (77 caught, 3 survivors killed), shell_startup locale/warmup/monitor_probe 95 (71 caught, 13 environment equivalents); 15 killers in w8_rest1_killers.rs; rows stay Partial; docs/reviews/CL-REST1-2026-10-30.md.
- CX-ZOOM 2026-10-30: mutation 40/40 (final 36 caught, 4 unviable), retarget killers-alone 4/4; native zoom 8 + WIC EXIF 1 pass; RAW/GUI/log-clock gaps stay Partial; docs/reviews/CX-ZOOM-2026-10-30.md.
- CL-SETTINGSUI 2026-10-30: builder groups first 21 mutants 19 caught, 2 unviable, 10 earlier survivors killed by w8_settingsui_killers.rs; rest of builder/context/host/frame/surface not run; 3 settings-ui rows stay Partial; docs/reviews/CL-SETTINGSUI-2026-10-30.md.
- CL-SEC01 2026-10-30: re-ran the SEC-01/F-05 link walk (21 mutants: cap `hops += 1` now killed by a seam test, `is_path_unreachable` guard `true` + 2 equivalents survive) and P-04 `instance_keys` (34: 22 caught, 11 survivors, 1 timeout); W2-FS native mutation passes invalid (`--cargo-test-arg=--include-ignored` is rejected by cargo); SEC-01/F-05, X-physical-fs, F-23 Done -> Partial; docs/reviews/CL-SEC01-2026-10-30.md.
- CL-FLAGAUDIT 2026-10-30: cargo rejects a lone `--cargo-test-arg=--include-ignored` (and a bare `-- --include-ignored`), and `--baseline skip` then reports every mutant caught; W2-FS physical.rs passes 2-7 void, CL-P01/STARTUP/MAINWIN2/HOST2/REST2 and W6 native rows suspect, pipeline unaffected; mutants.sh lints the form and preflights the test command (`--native`); no row demoted, re-run lanes listed; docs/reviews/AUDIT-MUTATION-FLAG-2026-10-30.md.
- CL-W6RERUN 2026-10-30: W6 + CL-P01 platform native mutation re-run with the corrected flag form: 82 tested 58 caught 16 missed 8 unviable, and 68 tested 54 caught 11 missed 3 unviable; 7 real gaps (registry reader, `absolute_normalized`, forward_client transfer guards, vblank `try_open`) killed by w6_rerun_killers.rs, 7 equivalent, 13 inactive cfg; P-01/P-03/P-05/P-11 stay Partial; docs/reviews/CL-W6RERUN-2026-10-30.md.
- CL-STARTRERUN 2026-10-31: correct-flag rerun of pr-shell startup: group a 62 (57 caught, 5 unviable, 45 MISSED unless the native run binaries and w8_startup_killers2 are in the list), group b 63 (56 caught, 7 unviable, 0 missed); the 7 CX-DLGS secondary_dialogs survivors are real (CL-REST2 was void); MAINWIN2/HOST2 not rerun; docs/reviews/CL-STARTRERUN-2026-10-30.md.
- CL-FSRERUN 2026-10-31: D-05/INV-8 flush mutation rerun for real (17 mutants: 8 caught, 3 unviable, 6 missed = 5 equivalent + 1 real share-mode `|`->`&` now killed by w2_fsrerun_killers.rs; 7 hand mutants all caught); both rows stay Done; docs/reviews/CL-FSRERUN-2026-10-30.md.
