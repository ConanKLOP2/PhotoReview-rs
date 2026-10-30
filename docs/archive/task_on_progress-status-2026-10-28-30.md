# task_on_progress archive: status lines 2026-10-28..30 (cx-* lanes, W9-R series, exe runs, zoom wiring)

Moved verbatim from `task_on_progress.md` (compaction on origin/master 043d673f, branch docs/tp-compact, 2026-10-30).
Why: that file had grown to about 14 KB and must stay near 7-8 KB; these dated status lines are history, not current state.
States, paths and next steps are those of the date in each heading. Current state: `../../task_on_progress.md`.

## cx-* lanes, 2026-10-30 (verbatim)

- **CX-IMAGING2 (2026-10-30, feat/cx-imaging2):** W4-01c/W4-14d Done; W4-14b/W4-14c/W4-16a/W4-10b Partial. New service killers; current pixel run 194/200 (6 not run, 4 loop timeouts), other selected gaps resolved or explicitly bounded. Focused tests/native WIC/gates in [evidence](docs/reviews/CX-IMAGING2-2026-10-30.md); no remote/PR claim.
- **cx-imaging (2026-10-30):** imaging 1240 pass; mutation files: compare.rs 221/246 caught (1 equivalent, 5 hang-detected, 19 unviable), report.rs 52/53 (1 equivalent), imaging files closed with killers; new killers `w4_mutation_killers`, `w7_followup_killers`; production adapter for embedded thumbnails not wired; rows stay Partial; [evidence](docs/reviews/CX-IMAGING-2026-10-30.md).
- **cx-host2 (2026-10-30):** P-01/P-03/P-05/P-11 remain Partial; native/fake binaries re-verified, new `w6_glue_killers` 6 tests pass 20x; display selected3/3 and settings2/2 caught, driver error selected6: 4 caught/2 unviable (3 incidental native field mutants missed by killers); crate gates green; no production changes; [evidence](docs/reviews/CX-HOST2-2026-10-30.md).
- **cx-core2 (2026-10-30):** JSON 97/312 tested: 92 caught, 4 equivalent MISSED, 1 unviable, 0 timeout; 215 not run. New `w2_json_killers` 3 pass; covering core 36/App 128/native 7 pass; per-crate fmt/Clippy and doc-links pass. F-21 Done retained, other lane rows Partial; handoff unstaged (Git index.lock permission denied); [evidence](docs/reviews/CX-CORE2-2026-10-30.md).
- **cx-json2 (2026-10-30):** X-stj-reader Partial; added 123/215 JSON identities: 112 caught, 3 equivalent MISSED, 5 compile-unviable, 3 timeouts; combined 220/312, 92 not run. Killers2 4 pass, fmt/Clippy pass; unstaged for orchestrator. [Evidence](docs/reviews/CX-JSON2-2026-10-30.md).
- **cx-release (2026-10-30, feat/cx-release):** offline release44/workflow4/stage7/PE18 checks pass; HTTP policy19/19 (16 caught,3 unviable); checker number13/13 + hex4 11/11 caught by new killers after 6 survivors; native graph4/4, HTTP2 + Settings1 rig placeholders fail explicitly; cargo-deny absent. Six rows stay Partial; [evidence](docs/reviews/CX-RELEASE-2026-10-30.md). Uncommitted handoff.

## Status lines 2026-10-28..29 (verbatim)

- **Exe runs (verified 2026-10-29, master `fa287028`, debug build, isolated data root, WARP):** workspace tests 10260 passed/0 failed/1394 ignored (`cargo test --workspace --locked -j 2`), workspace clippy clean. Native on this desktop, last run before #313/#314 (which add tests and docs only): `exe_process` 3/3 (DLLs staged by `tools/dev/stage-native.ps1`; without them the exe logs 2 ERROR lines and 2 of 3 fail), `startup_native` 3/3, `startup_run_native` 23/23, `ui_smoke` 6/6, `_dialogs` 7/7, `_overlay` 6/6, `_viewing` 9/9, `_startup` 4/4; `ui_smoke_perf` stays `todo!()` (attended hardware measurement). By hand: Right, `Enter` (Move), `Ctrl+Z` (Undo), `F2` (native rename). Mutation (partial, per-file): rename #310 plus round 3 #314 (231 mutants, 167 caught, 25 of 32 survivors killed, 5 equivalent, report `docs/reviews/W9-RENAME-MUTATION-3.md`), startup pure files #313 (23 mutants, 0 missed, `docs/reviews/W8-17E-MUTATION.md`). Partial rename undo leaves the catalog xmp key stale in C# and Rust alike (pinned by #311; fix C# first). Not checked: RAW (no samples), Delete on the real Recycle Bin, performance and attended measurements (W8-12j, W10-B2), mutation of rename key/menu routing and the other startup modules, thumb-drag ending early when posted (rig or shell unknown). Machine B is down; machine A owns all units.
- **Merged, native input fixture:** PR #269 squash `90826a2`; A claim `FIX-native-input-fixture` released; source `bc62104` fmt12/Clippy/workspace9862pass/0fail/1331ignored, regression101/101PASS (resource-less icon branch unexercised). Full startup/corpus/mutation limits and inherited13doc-link issues stay explicit in PARITY/PR; [history](docs/archive/task_on_progress-native-input-fixture.md).
- **Merged, rename R1:** #275/#277, selected12/97caught,85notrun; claim released, cleanup policy-blocked. [Evidence](docs/reviews/W9-R1-MUTATION.md).
- **Merged, W9-R2:** #278/#279;26F/N explicit PASS; selected9/277caught after regressions,268notrun; claim released. [Evidence](docs/reviews/W9-R2-MUTATION.md).
- **Merged, W9-R4:** #280/#281; selected8/22caught after regressions,14notrun; claim released. [Evidence](docs/reviews/W9-R4-MUTATION.md).
- **Merged, W9-R3:** #282/#283 `3f0870bc`; workspace9994/0/1335ignored, fmt12/Clippy/pre-pr PASS; selected10/80caught,70notrun. Claim released; cleanup policy-blocked, no deletions. [Evidence](docs/reviews/W9-R3-MUTATION.md).
- **Merged, W9-R5:** #284 `da42ca1e`,15F and workspace10009/0/1335ignored/fmt12/Clippy/pre-pr PASS. Follow-up adds the omitted catalog-conflict log; selected identity3/199caught,196notrun. [Evidence](docs/reviews/W9-R5-MUTATION.md). Follow-up #286 merged `2da34aa9`; fmt12/workspaceClippy/pre-pr PASS, workspace10012/0/1336ignored; claim released.
- **In progress, W9-R9:** A claim, master `4af47345`, branch `claude/rename-matrices`; owner 32F+1N explicit PASS, 42-method inventory mapped; 286 normal/70 nested-reconcile/132 outer+834 nested-retry fault runs. [Evidence](docs/reviews/W9-R9-VALIDATION.md). Gates on `4af47345`: fmt, workspace clippy, `cargo test -p pr-core -p pr-app` (4801 pass/0 fail/165 ignored) and pre-pr PASS. Tests only; no new mutation claim.
- Last verified gate: R9 on master `4af47345`: fmt (per package), workspace all-target Clippy, pr-core+pr-app tests (4801/0/165 ignored) and pre-pr PASS. Earlier combined run: workspace 10074 passed/0 failed/1337 ignored. Native evidence and mutation are recorded per unit.
- **In progress, W9-R6 (2026-10-28):** A claim, `codex/w9-rename-retarget`; retarget/session/command integration and suspended-present/stat-abandonment parity. Focused146PASS then suffix5/rename20PASS; integrated base2da34aa9 fmt12/workspaceClippy/pre-pr PASS, workspace10041/0/1336ignored; PR #287 merged `520d7291`; selected22/140 caught after one new Dimensions regression (118notrun); combined source fmt12/workspaceClippy/workspace10074/0/1337ignored PASS. [Evidence](docs/reviews/W9-R6-MUTATION.md).
- **Merged, benchmark fixture repair:** #285 `cf325668`; workspace10012/0/1336ignored, fmt12/workspaceClippy/pre-pr PASS; native Move/Delete/Copy10successes each/0sharing in this run. Claim released; cleanup retained. [Evidence](docs/reviews/BENCH-FILE-ACTION-DETERMINISM.md).
- **zoom-detail-wire (2026-10-29, A):** D1 / wheel / keys never loaded the full-resolution image in the exe: `ShellInput` changed the viewer without telling the presenter. Added `ShellInputHost::on_viewer_zoom_mode_changed` -> `MainViewModel::on_viewer_zoom_mode_changed`; real exe before/after, tests in [evidence](docs/reviews/ZOOM-DETAIL-WIRING-2026-10-29.md). PARITY row X-zoom-detail-loader stays Partial.
- **W9-R7 (2026-10-28):** claim retained; branch `codex/w9-rename-prompts` WIP. Final tests have stray module-scope `let` syntax errors; fix, then rerun tests. Stopped at quota 95%.

## Moved 2026-10-30 (lane cl-pixels, verbatim)

- **CX-JSON3 2026-10-30 / X-stj-reader:** Partial; 73/92 new identities: 65 caught, 2 unviable, 6 timeouts, 19 skip_ws without verdict; combined 293/312, 9 unresolved timeouts. Killers3 4/4, fmt/Clippy passed; unstaged for orchestrator, no PR. [Evidence](docs/reviews/CX-JSON3-2026-10-30.md).

## Moved 2026-10-30 (lane status lines, verbatim)

Moved verbatim from `task_on_progress.md` (branch docs/tp-compact2, 2026-10-30). Current state: `../../task_on_progress.md`.

- **CX-MUTW4 (2026-10-30, feat/cx-mutw4):** W4-02b/10b/11c/11e remain Partial; 223 current mutation identities tested (202 caught, 7 explained MISSED, 14 unviable, no unresolved timeouts), killers imaging 10/10 and platform 5/5; OQ1 and WPF/UI gaps retained. Unstaged handoff for orchestrator, no commit/push. [Evidence](docs/reviews/CX-MUTW4-2026-10-30.md).
| [`CX-IMAGING3`](docs/reviews/CX-IMAGING3-2026-10-30.md) | 2026-10-30 | W4-02b production embedded thumbnail adapter wired; W4-10b/11c/11e rows stay Partial, no mutation run |
| [`CL-UNDO`](docs/reviews/CL-UNDO-2026-10-30.md) | 2026-10-30 | `undo.rs` mutants re-checked (3 old survivors caught by killers alone; full re-run not finished); row F-10/INV-4/INV-6 (undo) stays Partial: 3 stat sites still skip the P-12 seam |
| [`CL-ENTRY`](docs/reviews/CL-ENTRY-2026-10-30.md) | 2026-10-30 | `pr_shell::entry` mutation 9 mutants, 6 caught, 3 `production_run` missed (needs real window); X-exe-entry stays Partial |
| [`CL-APPMUT`](docs/reviews/CL-APPMUT-2026-10-30.md) | 2026-10-30 | I-11 order/pace/backoff/policy full mutation 235/235 (221 caught, 14 unviable, 0 missed); I-05 App half verified; both rows Done |
- **CX-JSON3 2026-10-30 / X-stj-reader:** Partial; 73/92 new identities: 65 caught, 2 unviable, 6 timeouts, 19 skip_ws without verdict; combined 293/312, 9 unresolved timeouts. Killers3 4/4, fmt/Clippy passed; unstaged for orchestrator, no PR. [Evidence](docs/reviews/CX-JSON3-2026-10-30.md).
- **CL-FSFLUSH 2026-10-30 / D-05, INV-8 (fs part):** both Done; write-through and SequentialScan observed via handle mode, flush call counted; 17 mutants 15 caught 2 unviable 0 missed. Small seam in `physical.rs`. [Evidence](docs/reviews/CL-FSFLUSH-2026-10-30.md).
| [`CL-PIXELS`](docs/reviews/CL-PIXELS-2026-10-30.md) | 2026-10-30 | W4-16a `pixels.rs` 200/200 accounted: 184 caught (6 never-run + 4 hang-only `copy_transposed` mutants now caught by assertion), 1 equivalent, 15 unviable; row Done |
- **CX-JSON3 2026-10-30 / X-stj-reader:** Partial; 73/92 new identities: 65 caught, 2 unviable, 6 timeouts, 19 skip_ws without verdict; combined 293/312, 9 unresolved timeouts. Killers3 4/4, fmt/Clippy passed; unstaged for orchestrator, no PR. [Evidence](docs/reviews/CX-JSON3-2026-10-30.md).
- **CX-UNDOSTAT 2026-10-30 / F-10 / INV-4 (undo) / INV-6 (undo):** Partial; three Recycle stat sites use the fallible seam after test-first regression; 131 tests + fmt/Clippy/doc-links pass, selected mutation 42/199 = 39 caught/3 unviable, no missed/timeouts; single-Recycle exception/API parity and full-file mutation re-verification remain open. Uncommitted for orchestrator. [Evidence](docs/reviews/CX-UNDOSTAT-2026-10-30.md).

## Moved 2026-10-30 by lane cl-overlay (status lines, verbatim)
- **cl-undo2 (2026-10-30):** `undo.rs` 199/199 mutants, 180 caught, 19 unviable, 0 missed; stat divergence accepted; F-10/INV-4/INV-6 Done ([evidence](docs/reviews/CL-UNDO2-2026-10-30.md)).
- CX-JSON4 (2026-10-30): X-stj-reader Partial, 312/312 tested: 289 caught, 7 equivalent, 8 unviable, 8 timeouts; killers4 4 pass; evidence `docs/reviews/CX-JSON4-2026-10-30.md`; unstaged for orchestrator.
- CX-W4REST 2026-10-30: six rows Partial; 236 new mutants measured; disk reproof budget-exhausted; final gates/20x killers pass; docs/reviews/CX-W4REST-2026-10-30.md.
## Moved 2026-10-30 (cl-input), verbatim
  - Settings dialog (D-10/C-28, cl-dlg 2026-10-30): window logic mutation clean (0 missed in 680), pr-app presenters now complete (cl-prapp: 133 run, 0 real survivors, `docs/reviews/CL-PRAPP-2026-10-30.md`); builder groups/rows/inputs, context, host, frame, surface and the `phase_two` repair-notice glue not mutated (`docs/reviews/CL-DLG-2026-10-30.md`).

## Moved 2026-10-30 (CX-BENCH size maintenance, verbatim)
## Moved 2026-10-30 (CX-SURV lane status lines, verbatim)

- CX-JSON4 (2026-10-30): X-stj-reader Partial, 312/312 tested: 289 caught, 7 equivalent, 8 unviable, 8 timeouts; killers4 4 pass; evidence `docs/reviews/CX-JSON4-2026-10-30.md`; unstaged for orchestrator.
- CX-W4REST 2026-10-30: six rows Partial; 236 new mutants measured; disk reproof budget-exhausted; final gates/20x killers pass; docs/reviews/CX-W4REST-2026-10-30.md.

## Moved 2026-10-30 (compact3 lane status lines, verbatim)

Moved verbatim from `task_on_progress.md` (branch docs/tp-compact3, 2026-10-30). Current state: `../../task_on_progress.md`.

- CX-BENCH 2026-10-30: Partial; 67/362 new mutants: 59 caught, 4 survivors, 4 unviable; 6 killers 20x pass; evidence `docs/reviews/CX-BENCH-2026-10-30.md`; unstaged for orchestrator.
- CX-W4REST 2026-10-30: six rows Partial; 236 new mutants measured; disk reproof budget-exhausted; final gates/20x killers pass; docs/reviews/CX-W4REST-2026-10-30.md.
- CL-HOSTX 2026-10-30: swap_chain_clock 36 (0 missed), message_loop 84 (18 missed killed); rows stay Partial; docs/reviews/CL-HOSTX-2026-10-30.md.
- CL-MAINWIN 2026-10-30: lifecycle InnerSteps + core registry killers (10 tests); no row changes, 10 mutants still open; docs/reviews/CL-MAINWIN-2026-10-30.md.
- CX-SURV 2026-10-30: disk reproof 6/6 caught (3 killers-only completion guards, 3 lib assertions); JSON 8 fixed-point loops and disk bounded-wait gap remain Partial; docs/reviews/CX-SURV-2026-10-30.md.

## Moved 2026-10-30 by CX-DLGS (verbatim historical status)

- **Merged:** Wave 8 B6 (#164), B5 dialogs and startup (#223-#231: startup policies, lazy graph, zoom/diagnostics, Action Profiles, benchmark, Recovery, Settings window, secondary window routing). Wave 9 external editors E1-E9: E1 #233, E3 #234, E2 #235, E7 #236, E4 #237, E6 #238, E5 #239, E8 #254, E9 #256. Root repairs: #240 preview registration, #255 viewer borrow in zoom/menu, #260 shell handler lifetime. Dev tooling #242-#258 (status, janitor, hooks, agents, dashboard, triage, mutants, parity, oracle, native-tests, unsafe-audit).
- **PARITY.md** (`parity.sh stats`, 351 status rows): Done 168, Partial 91, Ported 41, Todo 12, Superseded 17, Different 14, Doing 3, Implementation 3, Contract 2.

## Moved 2026-10-30 by CX-REL (verbatim status lines)

- CL-OVERLAY 2026-10-30: X-overlay-button Done (71/71 mutation + native overlay/pointer wiring re-run); X-pointer-click-pan, recovery/benchmark/settings presenters, composition-factories stay Partial; benchmark Debug mutants killed, 2 `dispose` mutants MISSED (no seam); docs/reviews/CL-OVERLAY-2026-10-30.md.
- CL-VIEWING 2026-10-30: X-viewing-pure Done (88/88 platform mutants run); compare 139/139 and crossfade rest 39/39 mutated (all non-equivalent killed); composer 31/89 only; compare/crossfade/composer stay Partial (pixel golden vs WPF, WARP, native twins); docs/reviews/CL-VIEWING-2026-10-30.md.
- CL-HOST2 2026-10-30: X-host-ui-dispatcher Done (90 mutants: 79 caught, 0 missed, 11 unviable); wndproc.rs 11/11 and dpi.rs 27/27 viable caught; X-host-wndproc-shell-window stays Partial (create.rs WNDCLASSEXW glue, exe icon W8-20b); docs/reviews/CL-HOST2-2026-10-30.md.
- CX-REL 2026-10-30: offline release PE verified; bounded scanner/proxy mutation and escape killers; all six lane rows Partial; evidence docs/reviews/CX-REL-2026-10-30.md; unstaged for orchestrator.
- CL-OVERLAY 2026-10-30: X-overlay-button Done (71/71 mutation + native overlay/pointer wiring re-run); X-pointer-click-pan, recovery/benchmark/settings presenters, composition-factories stay Partial; benchmark Debug mutants killed, 2 `dispose` mutants MISSED (no seam); docs/reviews/CL-OVERLAY-2026-10-30.md.
- CL-VIEWING 2026-10-30: X-viewing-pure Done (88/88 platform mutants run); compare 139/139 and crossfade rest 39/39 mutated (all non-equivalent killed); composer 31/89 only; compare/crossfade/composer stay Partial (pixel golden vs WPF, WARP, native twins); docs/reviews/CL-VIEWING-2026-10-30.md.
- CL-MAINWIN2 2026-10-30: main_window frame+scrollbars 56/57 caught (1 unviable), viewing 69/76 (7 unviable), windowing 76/90 evaluated of 175 (14 unviable), 0 missed anywhere, no production change; rows stay Partial (native gaps); docs/reviews/CL-MAINWIN2-2026-10-30.md.
- CL-STARTUP 2026-10-30: pr-shell startup (9+4 files) and pr-platform-win shell_startup placement/dpi/local_time/cloak mutated: 185 tested, 169 caught, 0 missed, 16 unviable; locale/warmup/monitor_probe/secondary_dialogs not run; X-startup-* rows stay Partial/Ported (non-mutation gaps); docs/reviews/CL-STARTUP-2026-10-30.md.
- CL-HOST2 2026-10-30: X-host-ui-dispatcher Done (90 mutants: 79 caught, 0 missed, 11 unviable); wndproc.rs 11/11 and dpi.rs 27/27 viable caught; X-host-wndproc-shell-window stays Partial (create.rs WNDCLASSEXW glue, exe icon W8-20b); docs/reviews/CL-HOST2-2026-10-30.md.
- CL-INPUT2 2026-10-30: mutation 0 missed over composer rest 60, dispatch 173, input main_window 98, menus main_window 58 (1 composer Debug survivor killed); rows stay Partial (native/human gaps); popup_host 31 not run; docs/reviews/CL-INPUT2-2026-10-30.md.
- CL-AUDIT1 2026-10-30: audit of 26 Done rows ([report](docs/reviews/AUDIT-DONE-2026-10-30.md)): 11 pass, 15 doubt, 0 wrong; strongest doubts INV-6/F-16 (journal.rs 179/179 vs 94/179 in evidence) and SEC-01 (cap mutant); no PARITY change.
- CL-AUDIT2 2026-10-30: PARITY audit of all 84 Partial rows: 13 stale-note rows reworded (no status change), 36 machine-runnable gaps with proposed lanes, 34 human-only; short human list in docs/reviews/PARITY-GAP-MAP-2026-10-30.md.
- CL-JOURNAL 2026-10-30: journal.rs 179 of 179 claim NOT reproduced (only 94 of 179 documented, loaded machine made the re-run unusable); INV-6 (journal) and F-16/D-14 demoted to Partial; docs/reviews/CL-JOURNAL-2026-10-30.md.
