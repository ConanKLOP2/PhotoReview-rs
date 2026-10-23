# Wave 8 closing report (status at 2026-10-22)

## 1. Scope and outcome

1. Wave 8 ports the App/UI path to Win32, Direct2D and DirectWrite, following [the plan](WAVE-8-PLAN.md) and [R0001](adr/R0001-ui-framework.md).
2. The plan has 151 sub-units across B1–B6; batches order contract writing, while implementation follows dependencies.
3. B1 and B2 are recorded as merged in [task_on_progress.md](../task_on_progress.md); B3 has one conditional unit outstanding.
4. B4's remaining popup-menu unit landed in PR #144 after the dated B4 status note.
5. B5 has merged dialog, painter, builder and page slices, with native dialog and settings-window units still at contract stage.
6. B6 has two implementation slices of W8-09b; startup, executable and whole-shell smoke units have contracts and stubs only.
7. Thus Wave 8 is **open**, and this report closes the documentation snapshot rather than claiming a finished executable.
8. The baseline C# revision named by the plan is `d92d6aae`; this report makes no fresh C# behavior claim.
9. A merged implementation commit establishes the unit status below; it does not establish attended UI, hardware GPU or corpus parity.
10. Statuses and PR numbers below come from the plan roster and this branch's `git log --format='%h %ad %s' --date=short --grep='W8-'` (read 2026-10-23). An em dash means no unit PR number was present in a matching implementation subject.

## 2. Units by batch

Titles are shortened from [the unit roster](WAVE-8-PLAN.md). “Merged” means the branch history has an implementation commit, including the B1/B2 and B3/B4 group commits recorded in [task_on_progress.md](../task_on_progress.md). “Contract only” means a contract or stub exists without a matching implementation merge. W8-12j is conditional and has no merged commit. W8-09b is in progress because only slices S1 and S2 have merged.

### B1 — pure leaves and contracts

| Unit | Title | PR | Status |
|---|---|---|---|
| W8-01a | Viewport state and arrow pan | — | merged |
| W8-01b | Viewport layout | — | merged |
| W8-01c | Render geometry | — | merged |
| W8-01d | Fit and zoom rules | — | merged |
| W8-02a | Context menu model | #48 | merged |
| W8-02b | Text formatters | — | merged |
| W8-03a | Window placement | — | merged |
| W8-03b | Fullscreen controller | — | merged |
| W8-04a | Localization service | #82 | merged |
| W8-04b | Translation export | — | merged |
| W8-04c | Small services | — | merged |
| W8-04d | Log, build and launch helpers | — | merged |
| W8-04e | Hash and window helpers | #46, #49 | merged |
| W8-05a | Input primitives | — | merged |
| W8-05b | Key identifiers | — | merged |
| W8-05c | Key mapping | — | merged |
| W8-05d | Shortcut router | — | merged |
| W8-05e | Mouse and touchpad gestures | — | merged |
| W8-05f | Kinetic pan | — | merged |
| W8-06a | Viewer state | — | merged |
| W8-06b | Overlay state and policies | — | merged |
| W8-06d | Coordinator traits | — | merged |
| W8-10a | Action profiles | #47 | merged |
| W8-10b | Settings field map | #47 | merged |
| W8-10c | Settings form and secondary presenters | — | merged |
| W8-10f | Recovery presentation | #50 | merged |
| W8-10h | Benchmark presentation | — | merged |
| W8-12a | Render contracts and tiles | #98 | merged |
| W8-15a | Overlay tree | #51 | merged |
| W8-15b | Animator, timer and palette | #51 | merged |
| W8-15c | Overlay button and text | #51 | merged |
| W8-15d | Overlay panels | #112 | merged |
| W8-15e | Status and toolbar panels | #112 | merged |
| W8-16a | Viewing leaves | — | merged |
| W8-16e | Scroll-bar geometry | — | merged |

### B2 — coordinators, view models and presenters

| Unit | Title | PR | Status |
|---|---|---|---|
| W8-05g | Pointer wheel and zoom | #58 | merged |
| W8-05h | Pointer pan and click zoom | #71 | merged |
| W8-06c | Compare view model | #59 | merged |
| W8-06e | File-action gate | #57 | merged |
| W8-06f | Sibling-folder navigator | #70 | merged |
| W8-07a | Folder load | #63 | merged |
| W8-07b | Folder reload | #63 | merged |
| W8-07c | File-action recycle | #69 | merged |
| W8-07d | File-action undo | #69 | merged |
| W8-07e | File-action move and copy | #69 | merged |
| W8-07f | Duplicate cleanup | #73 | merged |
| W8-07g | Zoom-detail loader | #60 | merged |
| W8-07h | Image presenter state | #67 | merged |
| W8-07i | Image presentation, first half | #74 | merged |
| W8-07j | Image presentation, second half | #74 | merged |
| W8-07k | Missing item and photo info | #74 | merged |
| W8-08a | Main view-model state | #75 | merged |
| W8-08b | Main view-model navigation | #77 | merged |
| W8-08c | Main view-model commands | #77 | merged |
| W8-08d | Main view-model session | #77 | merged |
| W8-08e | Main view-model notifications | #77 | merged |
| W8-09a | Decoder composition | #66 | merged |
| W8-10d | Settings load and validation | #61 | merged |
| W8-10e | Settings apply and shortcuts | #65 | merged |
| W8-10g | Recovery window presenter | #68 | merged |
| W8-10i | Benchmark presenter | #64 | merged |

### B3 — Win32 foundation and rendering

| Unit | Title | PR | Status |
|---|---|---|---|
| W8-11a | Win32 structs and messages | #93 | merged |
| W8-11b | Win32 API wrappers | #93 | merged |
| W8-11c | Common controls | #93 | merged |
| W8-11d | Native dialog helpers | #97 | merged |
| W8-11e | Key capture, settings and Shell COM | #97 | merged |
| W8-11f | Graphics COM helpers | #92 | merged |
| W8-11g | D3D11 and DXGI helpers | #92 | merged |
| W8-11h | Direct2D and DirectWrite helpers | #92 | merged |
| W8-12b | Device creation and loss | #94 | merged |
| W8-12c | Device resources | #94 | merged |
| W8-12d | Render surface | #100 | merged |
| W8-12e | Draw context | #100 | merged |
| W8-12f | GPU cache policy | #95 | merged |
| W8-12g | GPU cache upload | #103 | merged |
| W8-12h | GPU keep-alive | #99 | merged |
| W8-12i | DirectWrite text | #99, #145 | merged |
| W8-12j | Conditional swap-chain latency experiment | — | not started |
| W8-13a | Host frame clocks | #96 | merged |
| W8-13b | Swap-chain clock and host contracts | #102 | merged |
| W8-13c | Message loop | #102 | merged |
| W8-13d | UI dispatcher | #104 | merged |
| W8-13e | Shell window creation | #106 | merged |
| W8-13f | Shell window messages and DPI | #106 | merged |

### B4 — main window, input, menus, overlay and viewing

| Unit | Title | PR | Status |
|---|---|---|---|
| W8-13g | Main window core | #130 | merged |
| W8-13h | Main window lifecycle | #130 | merged |
| W8-13i | Frame and scroll-bar wiring | #140 | merged |
| W8-13j | Main window viewing | #139 | merged |
| W8-13k | Main window windowing | #135 | merged |
| W8-14a | Native input foundation | #116 | merged |
| W8-14b | Input dispatcher | #125 | merged |
| W8-14c | Viewport image surface | #137 | merged |
| W8-14d | OLE drop target | #119 | merged |
| W8-14e | Context-menu controller | #123 | merged |
| W8-14f | Popup-menu host | #144 | merged |
| W8-14g | Owner-draw menu | #132 | merged |
| W8-15f | Overlay host | #118 | merged |
| W8-15g | Tooltip host | #120 | merged |
| W8-15h | Overlay state and layout | #128 | merged |
| W8-15i | Overlay input and paint | #133 | merged |
| W8-15j | Overlay main-window feature | #138 | merged |
| W8-16b | Compare and crossfade | #121 | merged |
| W8-16c | Presentation composer | #127 | merged |
| W8-16d | Presentation prefetch | #117 | merged |
| W8-16f | Scroll-bar state and layout | #131 | merged |
| W8-16g | Scroll-bar input and paint | #134 | merged |
| W8-16h | Viewport controller | #115 | merged |
| W8-16i | Presentation sink and hub | #141 | merged |

### B5 — dialogs and settings UI

| Unit | Title | PR | Status |
|---|---|---|---|
| W8-18a | Shell file and folder pickers | #150, #154 | merged |
| W8-18b | Task dialogs and dialog services | #150 | merged |
| W8-18c | Clipboard service | #148, #150, #154 | merged |
| W8-18d | Dialog template and chrome | #150, #151 | merged |
| W8-18e | Edit centering and list dialog | #153 | merged |
| W8-18f | Native controls, first half | #153 | merged |
| W8-18g | Native controls, second half | #153 | merged |
| W8-18h | Native dialog creation | — | contract only |
| W8-18i | Native dialog layout | — | contract only |
| W8-18j | Native dialog input | — | contract only |
| W8-18k | Native dialog close | — | contract only |
| W8-18l | Chrome painter geometry | #152 | merged |
| W8-18m | Chrome painter controls | #152 | merged |
| W8-18n | Chrome painter widgets and frame | #152 | merged |
| W8-18o | Secondary-window router | — | contract only |
| W8-18p | Zoom and diagnostics dialogs | — | contract only |
| W8-18q | Action-profiles dialog | — | contract only |
| W8-18r | Benchmark dialog, first half | — | contract only |
| W8-18s | Benchmark dialog, second half | — | contract only |
| W8-18t | Recovery dialog, first half | — | contract only |
| W8-18u | Recovery dialog, second half | — | contract only |
| W8-18v | Recovery dialog, third part | — | contract only |
| W8-19a | Settings page framework | #150, #151 | merged |
| W8-19b | Settings builder layout | #153 | merged |
| W8-19c | Settings builder inputs | #161 | merged |
| W8-19d | Diagnostics and performance pages | #161 | merged |
| W8-19e | Display page | #161 | merged |
| W8-19f | Files page | #161 | merged |
| W8-19g | General page | #161, #162 | merged |
| W8-19h | Mouse page | #161 | merged |
| W8-19i | Shortcuts page | #161 | merged |
| W8-19j | Settings window frame | — | contract only |
| W8-19k | Settings window state | — | contract only |
| W8-19l | Settings window apply | — | contract only |
| W8-19m | Settings window close | — | contract only |

### B6 — startup, composition and integration

| Unit | Title | PR | Status |
|---|---|---|---|
| W8-09b | Shared service graph | #158, #159 (S1/S2 only) | in progress |
| W8-17a | Launch and error log | — | contract only |
| W8-17b | Cloak and placement | — | contract only |
| W8-17c | Startup composition root | — | contract only |
| W8-17d | Startup options and seams | — | contract only |
| W8-17e | Startup run and shutdown | — | contract only |
| W8-20a | Executable entry and resources | — | contract only |
| W8-20b | Whole-shell smoke and parity map | — | contract only |

## 3. Test counts and their limits

| Evidence date | Count | How obtained | Limit |
|---|---|---|---|
| 2026-10-18 | 5,465 passed; 0 failed; 431 ignored | B1 contract gate in [task_on_progress.md](../task_on_progress.md) | Stub gate; no new unit test had run. |
| 2026-10-21 | 8,383 passed; 0 failed; 754 ignored | Post-B3 workspace gate in [task_on_progress.md](../task_on_progress.md) | Earlier master snapshot; ignored tests excluded from passing total. |
| 2026-10-23 | `pr-app`: 2,646; `pr-platform-win`: 2,556; `pr-shell`: 92 | Count of literal `#[test]` lines in each crate's `.rs` files using recursive `Select-String` in this checkout | Source declarations, not test executions; parameterized and ignored tests are not separated. |

No current integrated workspace test result after PR #162 was found in the requested status records. The B4 note's unit count predates PR #144, and the B5/B6 contract notes predate their implementation slices.

## 4. Mutation evidence

The columns reproduce only recorded outcomes in [task_on_progress.md](../task_on_progress.md) and [PARITY.md](PARITY.md). “Unrecorded” means the source did not give that field; it is not zero. A later killer test does not retroactively change the original run's tally.

| Unit group | Tested / total | Caught | Missed | Unviable | Timeouts | Verdict and source |
|---|---:|---:|---:|---:|---:|---|
| W8-05g | 117 / total unrecorded | unrecorded | 0 | unrecorded | unrecorded | Focused count in [task status](../task_on_progress.md); split not recorded. |
| W8-06c | 24 / 24 | 24 | 0 | 0 | 0 | [Task status](../task_on_progress.md) and the W8-06c killer-test subject; no broader file claim. |
| W8-08a | 220 / total unrecorded | unrecorded | 3 judged equivalent | unrecorded | unrecorded | `state.rs` only, [task status](../task_on_progress.md). |
| W8-12f | 72 / 72 | 61 | 5 | 3 | 3 | Four misses later covered; one adjudicated equivalent, [PARITY](PARITY.md). |
| W8-13a | unrecorded | unrecorded | one equivalent survivor | unrecorded | unrecorded | Aggregate not recorded in [task status](../task_on_progress.md). |
| B3 host group | 514 / total unrecorded | unrecorded | 0 | unrecorded | unrecorded | Group-level count in the dated B4 [task status](../task_on_progress.md); no per-unit split. |
| W8-18a | **PARTIAL**, 60 / total unrecorded | 47 | 9 equivalent survivors | 4 | unrecorded | Combined folder/file-dialog runs did not finish within the budget, [PARITY](PARITY.md). |
| W8-18c | **PARTIAL**, 25 / total unrecorded | 21 | 3 survivors (two equivalent, one native gap) | 1 | unrecorded | Clipboard runs did not finish within the budget, [PARITY](PARITY.md). |
| Other B1, B2, B3, B4, B5 and B6 unit groups | NONE | — | — | — | — | No tested/total and full outcome split found in the requested status documents. Commit subjects record targeted mutant-killing tests for some of these units, but give no run tally. |

## 5. Verification that has not been recorded

- A completed `W8-20b` whole-shell desktop session and its UI parity map have not been recorded; that unit remains at contract stage ([PARITY](PARITY.md), [B6 status](../task_on_progress.md)). Some isolated native tests **were** run by hand, including B3 WARP tests and B5 dialog/painter tests; those runs must not be confused with an end-to-end desktop verdict.
- Production rendering has no recorded hardware GPU run. The B3 tests cited in [task_on_progress.md](../task_on_progress.md) used WARP, hidden or message-only windows. The separate R0001 spike did measure hardware adapters, with a windowed `EndDraw` stall; it was not a production viewer run ([R0001](adr/R0001-ui-framework.md)).
- No attended re-measure of the production windowed p95 is recorded. W8-12j remains conditional, and R0001 remains Proposed ([R0001](adr/R0001-ui-framework.md), [plan](WAVE-8-PLAN.md)).
- RAW corpus tests remain unrun in the [task status](../task_on_progress.md); this is an inherited parity limit rather than a new Wave 8 test result. R0001 also lists untested IME input, 150% DPI, second monitor fullscreen, secondary windows, RAW/HEIC and cold-cache reads for its spike.
- The B5 and B6 contract notes explicitly say their new goldens had not yet been read by Rust tests at contract time. Later merged implementation tests must be evaluated per unit; the notes alone do not establish an integrated B5/B6 golden verdict.

## 6. Open debts and decisions

Wave 8-related proposals below retain the state shown in [PROPOSED-DECISIONS.md](PROPOSED-DECISIONS.md). Earlier proposals remain in that register; these are the ones governing the Wave 8 scope, scheduling or outstanding work.

| Decision | State and debt |
|---|---|
| P-06 | Proposed: Win32/Direct2D direction chosen; attended windowed p95 remains open. |
| P-28 | Proposed, applied: schedule units by direct dependency. |
| P-29 | Proposed, applied: machine-load and worktree build policy. |
| P-34 | Proposed, applied: model cap and bounded mutation runs. |
| P-38 | Proposed: Wave 8 roster and conditional W8-12j. |
| P-39 | Proposed: B1 contract choices and crate-layer reconciliation. |
| P-40 | Proposed: Codex-first workflow routing; see its later restriction in P-42. |
| P-41 | Proposed: B2 contract choices and explicit state machines. |
| P-42 | Proposed: Codex implementation scope restricted to contracted units with red tests. |
| P-43 | Proposed: production secondary-window adapter belongs to W8-17c, still contract only. |
| P-44 | Proposed: post-baseline external-editor, rename and text dialogs deferred to W8-18x after the sync patch. |
| P-45 | Proposed: B5 contract choices; native dialog and settings window remain open. |
| P-46 | Proposed: B6 contract choices; startup and executable units remain open. |
| P-47 | Proposed: numeric spike-deletion gate, no version resource and K-29 retirement are product calls still awaiting confirmation. |

| ADR | State and Wave 8 relevance |
|---|---|
| [R0001](adr/R0001-ui-framework.md) | Proposed: UI direction chosen; windowed p95 and attended UI checks remain open. |
| [R0002](adr/R0002-coexistence-with-csharp-app.md) | Accepted: Rust data and process identity stay separate from the C# app. |
| [R0003](adr/R0003-concurrency.md) | Accepted: explicit threads, UI affinity and generation-based stale-result handling govern ports. |
| [D59](adr/D59-decode-error-inner-hresult.md) | Accepted: nested decode HRESULT preservation is prior imaging work, not a Wave 8 completion proof. |
| [D60](adr/D60-raw-orientation-test-helper.md) | Accepted: malformed test-only RAW orientation inputs fail closed; not a Wave 8 UI gate. |

Additional code debts recorded in [PARITY](PARITY.md) include W8-09b registry/main-view-model slices S3/S4, the unowned embedded-thumbnail backend, incomplete B5 dialogs, and B6 startup and smoke coverage. The [unsafe review](UNSAFE-REVIEW-W8-11.md) found no source-level defect in its reviewed Win32 wrappers; it is a static audit, not native runtime proof.

## 7. Lessons learned

- The [Wave 8 rules in AGENTS.md](../AGENTS.md) explain that a large-file mutation sweep is dominated by rebuild time. Select branching policy functions, use one job and a private target directory, time-box the run, and label partial coverage.
- Native/FFI glue needs targeted native tests and an attended whole-shell pass; a clean pure-logic mutation score cannot substitute for COM, window, GPU or dialog behavior.
- The [autopilot history](../tools/autopilot/README.md) records the shift from tier barriers to dependency-driven units, disjoint owned files, a single integration owner and final workspace gates. The B4 task snapshot also records native tests that passed only against temporary stand-ins, illustrating why the integrated tree needs its own verdict.
- Contracts and goldens can compile while their stubs still contain `todo!()`; the B1, B5 and B6 status notes explicitly separate these gates from implementation and runtime evidence.

## 8. Next steps

Complete W8-09b S3/S4, then the remaining B5 native-dialog and settings-window units and B6 startup, entry and smoke units according to [the dependency plan](WAVE-8-PLAN.md) and [contracts](../contracts/README.md). Run the full integration gate on the final head, record an attended desktop session and hardware GPU measurement, and resolve the R0001 windowed p95 and P-47 decisions. W8-12j starts only if its stated re-measure condition is met. No Wave 9 or Wave 10 plan was found in the inspected plan, status, parity, contract roster or `docs/` plan filenames; their scope is **(not verified)**.
