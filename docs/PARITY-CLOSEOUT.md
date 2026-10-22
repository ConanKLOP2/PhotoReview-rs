# Parity closeout

## Baseline and status inventory

`docs/PARITY.md` names C# inventory baseline `e441d67a`, generated 2026-10-14, in its header. `docs/SYNC-LEDGER.md` separately names current spec baseline `d92d6aae` (2026-10-14). Reconcile the difference before calling the inventory current. These counts parse status cells in the tracking rows, Wave 4 final-status table, and later wave rows. In sections 1–9 (`ID | Item | C# source | Notes`), they include the eight rows whose second cell is a `Doing (...)` or `Different (...)` status; rows whose second cell is a plain item name, such as `F-21`'s “Partial copy policy,” are excluded. They count rows, including repeated IDs and historical snapshots, rather than unique features or achieved parity. Two `Done` rows (`X-main-view-model-session`, `X-main-view-model-notify-move`) have their proving test and note in one cell; the empty Rust-location cell of `X-exe-versioninfo` is retained. The Wave 4 section calls earlier `Superseded` duplicates stale and its summary authoritative (`docs/PARITY.md`).

| Status prefix | Rows | Meaning |
|---|---:|---|
| Done | 148 | Recorded completion; check qualifications. |
| Todo | 3 | Unstarted. |
| Doing | 7 | In progress. |
| Partial | 79 | Implementation or evidence remains. |
| Different | 15 | Declared intentional difference, requiring a Rust ADR. |
| Superseded | 17 | Historical; reconcile with later status. |
| Contract | 47 | Contract/planned work, not completion. |
| Ported | 3 | Ported, pending a Done or ADR closeout decision. |
| Implementation Done (qualified) | 3 | Implementation reported done; verification qualified. |
| **Total** | **322** | Duplicate IDs retained. |

A `Done` row can still contain a `Different` qualification or note about unrun evidence. `Contract`, `Ported`, and qualified `Implementation Done` occur in `docs/PARITY.md` and remain separate. No `Retired` status row occurs in this parse; `docs/WAVE-8-PLAN.md` lists retired C# parts with replacements.

## Closeout procedure

1. Re-parse all status-bearing rows in `docs/PARITY.md`; retain repeated IDs and the historical Wave 4 summary. Resolve an ID using the later authoritative row, not an old Done snapshot.
2. For each active `Todo`, `Doing`, `Partial`, `Contract`, `Ported`, or qualified `Implementation Done` row, finish the port with a proving test and record `Done`; or document an intentional difference in a Rust ADR and mark it `Different`; or mark it `Retired` with reason and replacement. Keep native, GUI, RAW corpus, mutation and benchmark evidence distinct where the row requires them (`AGENTS.md`, `docs/PARITY.md`).
3. Link an accepted ADR in `docs/adr/` for each intentional difference, and update PARITY status and test in the same PR (`AGENTS.md`). `R0002` covers only its stated root/name separation.
4. Reconcile C# changes through `docs/SYNC-LEDGER.md`. When synced assets or fixtures change, run `tools/sync-from-csharp.ps1` and log the new baseline; never hand-edit them (`AGENTS.md`).
5. Lead runs the exit checks on the final head, reviews qualified Done rows, and records the comparative benchmark separately (`docs/PLAN.md`, Phases 2 and 7).

## Rows requiring disposition

`Status text` reproduces the source cell except that crate-relative test paths are expanded to their verified repo paths. `Owner unit` is the first Wxx-yy token in status, ID or note: a routing hint, not current assignment. `Evidence missing` uses an explicit clue or `see row`. Wave 4 summary rows have no Rust-location column. Source: `docs/PARITY.md`.

### Todo (3)

| ID | Status text (as in PARITY.md) | Owner unit | Evidence missing | Rust location |
|---|---|---|---|---|
| X-http-transport | Todo | W2-18 | Implementation and test; see row. | `pr_core::updates::checker::HttpTransport` |
| X-swapchain-latency-experiment (R0001 windowed `EndDraw` stall) | Todo (contract W8-12j, conditional, contract only, no stub, NOT started) | W8-12j | Implementation and test; see row. | none until the trigger fires; conditional experiment and env `PHOTOREVIEW_RS_SWAPCHAIN_EXPERIMENT` |
| X-embedded-thumbnail-backend (`EmbeddedThumbnailReader` over WIC) | Todo; Different until done (`UnavailableEmbeddedThumbnails`: the thumbnail cache decodes the file instead, W8-09b O1) | W8-09b | Implementation and test; see row. | none |

### Doing (7)

| ID | Status text (as in PARITY.md) | Owner unit | Evidence missing | Rust location |
|---|---|---|---|---|
| INV-8 | Doing (W2-14: file-system part done) | W2-14 | Remaining work; see row. | `PhysicalFileSystem::open_read_shared` |
| RV-S06 | Doing (W2-14: resolver fact) | W2-14 | Remaining work; see row. | `PhysicalFileSystem::resolve_real_path` |
| F-22 | Doing (W2-13: write part done) | W2-13 | Remaining work; see row. | `SessionWriter` -> `SessionStore::save` |
| P-03 | Doing (W6-02a/b/c/d/e source/tests integrated; static-only on combined tree, build/test pending) | W6-02a/b/c/d/e | Combined build/test and native COM evidence pending; see inventory row. | not stated in inventory C# source column |
| P-04 | Doing (W6-03a/e source/tests integrated; static-only, compile/test pending) | W6-03a/e | Compile/test and native ownership evidence pending; see inventory row. | not stated in inventory C# source column |
| P-05 | Doing (W6-03b/c/d/e source and tests integrated; static-only, compile/test pending) | W6-03b/c/d/e | Compile/test and native named-pipe evidence pending; see inventory row. | not stated in inventory C# source column |
| P-11 | Doing (W6-04a/b source integrated; lane tests reported 14/14 state-machine and 5/5 ABI/device-name/mapping, native DWM ignored; no combined-tree gate) | W6-04a/b | Combined-tree and native DWM evidence pending; see inventory row. | not stated in inventory C# source column |

### Partial (79)

| ID | Status text (as in PARITY.md) | Owner unit | Evidence missing | Rust location |
|---|---|---|---|---|
| C-07..C-09, C-11, C-13, C-15, C-16, K-38, S-ctxmenu | Partial (W8-14e/14f) | W8-14e/14f | see row | `pr_core::settings::context_menu` |
| X-physical-fs | Partial (W2-14) | W2-14 | see row | `pr_core::io::physical::PhysicalFileSystem` |
| X-counting-fs | Partial (W2-14) | W2-14 | see row | `pr_core::io::counting::CountingFileSystem` |
| S-64 | Partial (W2-03; repair-dependent tests need W2-02 `normalizer`) | W2-03 | see row | `pr_core::settings::store::SettingsStore::{load, parse_text, parse, migrate, try_salvage}` |
| S-65 | Partial (W2-03) | W2-03 | see row | `pr_core::settings::store::SettingsStore::{save, persist}` |
| INV-11 | Partial (W2-03) | W2-03 | see row | `pr_core::settings::store::SettingsStore::load` |
| D-02 | Partial (W2-03) | W2-03 | see row | `pr_core::settings::store::SettingsStore::unique_backup_path` |
| D-10 | Partial for the store part (W2-03) | W2-03 | see row | `pr_core::settings::store::SettingsStore::parse_text` |
| C-28 | Partial for the repair list (W2-03) | W2-03 | see row | `SettingsStore::last_load_repairs`, `parse_text(..).repairs()` |
| X-config-import | Partial (W2-03; Rust-only, R0002) | W2-03 | see row | `pr_core::settings::store::SettingsStore::load_first_run_with_import`, `csharp_config_path` |
| D-05 | Partial (W2-14, physical part) | W2-14 | see row | `PhysicalFileSystem::write_all_text_atomic` |
| RV-C05 | Partial (W2-14) | W2-14 | see row | `PhysicalFileSystem::write_all_text_atomic` |
| AR16 | Partial (W2-14) | W2-14 | see row | `PhysicalFileSystem::enumerate_files_with_stat` |
| SEC-01 / F-05 | Partial (W2-08 policy; W2-14 resolver) | W2-08 | see row | `PhysicalFileSystem::resolve_real_path`; `file_actions::destination_policy` |
| X-perf-diagnostics | Partial (W2-17; P-16, W-05) | W2-17 | see row | `pr_core::diagnostics::{perf, perf_csv, metrics}` |
| X-stj-reader | Partial (W2-01: surrogate-guard and Infinity-sign mutants killed; 2 equivalent mutants remain, nothing left to kill) | W2-01 | see row | `pr_core::settings::app_settings::json` |
| P-09 | Partial (W2-18) | W2-18 | see row | `pr_core::updates::checker` |
| P-01 (Explorer COM boundary) | Partial (W6-01a focused tests, fmt, and Clippy pass; workspace/mutation gates pending crate-root wiring) | W6-01a | Native/desktop evidence; see row. | `pr_platform_win::explorer_com_interop::{ShellWindowsSource, ShellWebBrowser, ExplorerNativeVtable}` |
| W6-01b (Explorer window selection policy) | Partial (18 focused C#-named tests pass; crate-root integration and mutation verdict pending) | W6-01b | see row | `pr_platform_win::explorer_window_selector::{select, try_canonicalize_location}` |
| F-23 | Partial (W2-13) | W2-13 | see row | `pr_core::session::writer::SessionWriter` |
| D-03 / F-11 / F-19 / F-20 / S-21 (journal file) | Partial (W3-01 verdict FAIL: cargo-mutants not done, 228 of 231 mutants untested; open list = all untested mutants) | W3-01 | Mutation verdict; see row. | `pr_core::file_actions::journal::OperationJournal::{append, dismiss, read_entries, read_pending_and_failed_operations, read_committed_moves, read_latest_entry, append_if_unchanged_since, append_if}`, `decode_stream_reader_lines` |
| F-12 / F-15 (transaction) | Partial (W3-01 verdict FAIL: cargo-mutants not done, 228 of 231 mutants untested; open list = all untested mutants) with two Different notes | W3-01 | Mutation verdict; see row. | `pr_core::file_actions::transaction::JournalTransaction` |
| F-13 | Partial (W3-01 verdict FAIL: cargo-mutants not done, 228 of 231 mutants untested; open list = all untested mutants) with one Different note | W3-01 | Mutation verdict; see row. | `OperationJournal::append` (`append_lines`, `open_append_with_retry`, `tail_lacks_newline`) |
| INV-6 (journal part) | Partial (W3-01) | W3-01 | Mutation verdict; see row. | `OperationJournal`, `JournalTransaction` |
| F-16 / D-14 | Partial (W3-02 plan + primitives done; W3-01 `OperationJournal::try_compact` verdict FAIL: cargo-mutants not done, 228 of 231 mutants untested; open list = all untested mutants) | W3-02 | Mutation verdict; see row. | `pr_core::file_actions::{compaction::build, journal_rules}`, `pr_core::io::journal_compaction_files` |
| F-10 / INV-4 (undo) / INV-6 (undo) | Partial (W3-05 verdict FAIL: the verifier's cargo-mutants re-run is unfinished, 0 of 195 mutants evaluated by it; the porter's round-3 full run was 198 mutants, 178 caught, 18 unviable, 2 missed and killed; no behaviour diff found; `undo.rs` does not use the P-12 seam `try_get_file_stat`, an over-long path gives `None` where C# throws) | W3-05 | Mutation verdict; see row. | `pr_core::file_actions::undo::UndoService` |
| I-05 | Partial (W4-05a): policy only; App half (`MW.UpdateTargetDecodeBox`, 2200x1400 fallback viewport) is W8 | W4-05a | see row | `pr_imaging::adaptive_preview` |
| I-11 (order, pace, backoff, policy) | Partial (W4-05b, W4-06b): window + order + options defaults + busy backoff + preview size sampler (14 tests in tests/preload_backoff.rs, 17 hand mutants killed); pace, policy still Todo (W4-06a, W4-15a contract + stub) | W4-05b | see row | `pr_imaging::{preload_order, navigation_pace, preload_backoff}`, `pr_imaging::preload::policy` |
| I-15b / INV-8 / INV-12 (LibRaw native layer, gate, guard) | Partial (W5-15ab implementation and W5-16a verifier PASS; W5-16b memory guard done. Native `libraw.dll` tests and corpus-backed cases remain unverified on this machine) | W5-15a | RAW corpus evidence; see row. | `pr_libraw::{native, availability, full_decode_gate, memory_guard}` |
| I-15 / I-14 / I-12 / INV-12 / S-09 (LibRaw decoder, W5-16c) | Partial (W5-16c; native pipeline verified against the pinned `libraw.dll` for the DLL-only tests, corpus tests skip: P-30; structural-walk and handoff mutation evidence is now targeted, full historical survivor set remains unreconstructed) | W5-16c | RAW corpus evidence; see row. | `pr_libraw::decoder` (`LibRawDecoder`, `read_jpeg_thumbnail`, `trim_to_end_of_image`, `to_extended_length_path`, `open_file_with`, `NativeLibRaw`) |
| W4-01c decoder traits | Partial | W4-01c | see row | not stated in Wave 4 summary row; see tracking rows |
| W4-02b embedded thumbnail | Partial | W4-02b | Native/desktop evidence; see row. | not stated in Wave 4 summary row; see tracking rows |
| W4-10b thumbnail cache | Partial | W4-10b | Native/desktop evidence; see row. | not stated in Wave 4 summary row; see tracking rows |
| W4-11c WIC direct decode | Partial | W4-11c | see row | not stated in Wave 4 summary row; see tracking rows |
| W4-11e WIC cache codec | Partial | W4-11e | see row | not stated in Wave 4 summary row; see tracking rows |
| W4-14b service decode path | Partial | W4-14b | see row | not stated in Wave 4 summary row; see tracking rows |
| W4-14c service disk | Partial | W4-14c | Mutation verdict; see row. | not stated in Wave 4 summary row; see tracking rows |
| W4-14d preload adapter | Partial | W4-14d | see row | not stated in Wave 4 summary row; see tracking rows |
| W4-16a pixels | Partial | W4-16a | see row | not stated in Wave 4 summary row; see tracking rows |
| W5 final status (integration/wave-5, 2026-10-17) | Partial | W5-14b | RAW corpus evidence; see row. | not stated in summary row |
| X-bench-slowlink-fs | Partial (W7-02a; sleep truncation fix 2d602dd is on master and tested, the DIVERGENCE items below stay open) | W7-02a | see row | `pr_bench::slow_link_fs` |
| X-bench-executor | Partial (W7-03b, FAIL verdict) | W7-03b | see row | `pr_bench::image_executor` |
| X-bench-workload-runner | Partial (W7-03c repair: independently verified against BenchmarkWorkloadRunner.cs; no behavioural difference found beyond the documented DIVERGENCE items; mutants run, 7 survivors are unreachable zero-dimension checks) | W7-03c | see row | `pr_bench::workload_runner` |
| X-bench-cli-guard | Partial (W7-03d repair: all FAIL findings fixed; replace is probe + POSIX rename, not MoveFileEx) | W7-03d | see row | `pr_bench::cli::tool_path_guard` |
| X-viewport-state | Partial (W8-01a, engine W8-01b pending); K-27 Different (ArrowPanRule new rule, ADR owed) | W8-01a | see row | `pr_app::{viewport::{types, state, arrow_pan}, input::primitives}` |
| X-overlay-tree (K-40) | Partial (W8-15a: contracts, element template, layer, model; 15c, 15d, 15e: see the rows below) | W8-15a | see row | `pr_app::overlay::{contracts, element, layer, model}` |
| X-overlay-button | Partial (W8-15c): background states, click gating, size rule | W8-15c | see row | `pr_app::overlay::button` |
| X-overlay-text | Partial (W8-15c): layout lease, click-through | W8-15c | see row | `pr_app::overlay::text` |
| X-viewing-pure | Partial (W8-16a; verifier mutation pass pending) | W8-16a | see row | `pr_app::viewing::{scaling_policy, image_layer}`, `pr_platform_win::shell_viewing::{frame_stats, pacer, frame_log}` |
| X-pointer-click-pan (K-34, K-35, K-36, K-37, K-15, C-10, K-27 arrows, S-35, S-46, S-51) | Partial (W8-05h done; shell wiring in W8-14 pending) | W8-05h | see row | `pr_app::input::pointer::{click_zoom, pan}` |
| X-composition-factories | Partial (W8-09a; W8-09b wires the registry) | W8-09a | see row | `pr_shell::composition::{decoders, factories, contracts}` |
| X-settings-presenter | Partial (W8-10d done: `load`, `validate`; W8-10e done: `apply`, `shortcuts`, import/export Q-R50, language picker, Explorer integration, manual update check, shortcut capture; `export_translation` writes the todo file through `LocalizationService::export_todo`); update check is three steps (D-W8-2, `begin`/`UpdateCheckJob::run`/`finish`) = Different, same observable results; native dialog run recorded by W8-19 | W8-10d | see row | `pr_app::presenters::settings::*` |
| X-recovery-window-presenter (INV-5) | Partial (W8-10g) | W8-10g | see row | `pr_app::presenters::recovery_window` |
| X-benchmark-presenter | Partial (W8-10i; Done once W8-18r/18s record a desktop run) | W8-10i | see row | `pr_app::presenters::benchmark` |
| X-viewer-state-fitviewer | Partial (W8-06a) | W8-06a | see row | `impl FitViewer for ViewerState` |
| X-zoom-detail-loader / I-16 (zoom detail: on demand, not cached, at most one, dropped on navigation) / I-15 (RAW full decode: current image only, 300 ms indicator) / INV-1 (stale completion dropped) | Partial (W8-07g; the presenter integration of W8-07h..k and the native twin of `ExifRotatedSource_UsesPostOrientationOriginalSize` are still open) | W8-07g | Native/desktop evidence; see row. | `pr_app::coordinators::zoom_detail::{ZoomDetailLoader, ZoomDetailRefreshResult, ZoomDetailEffect, CompletionInbox, BackgroundRunner, ThreadRunner}` |
| X-host-swap-chain-clock, X-host-shell-contracts, X-host-icons (C-05 swap chain, C-15 contracts, `ShellIcons`, `NativeRect`) | Partial (W8-13b ported; native tests run once on a desktop, 3 runs green; the 12d smoke test is not written because `D2dRenderSurface::create_window_surface` is still a stub on master) | W8-13b | Native/desktop evidence; see row. | `pr_platform_win::host::{swap_chain_clock, contracts, icons, native}` |
| X-host-message-loop (`MessageLoop`, WndProc exception capture) | Partial (W8-13c ported; native tests run once on a desktop, 3 runs green; not yet driven by the real `Win32UiDispatcher`, W8-13d) | W8-13c | see row | `pr_platform_win::host::{message_loop, pending_panic}` |
| X-host-ui-dispatcher (C-04 on Win32: `Win32UiDispatcher`) | Partial (W8-13d: implemented; F 9 tests pass; N 22 tests pass locally with `--include-ignored` x3 on a desktop; not Done until they also run against the real `MessageLoop`/`pending_panic` of W8-13c, see the note) | W8-13d | see row | `pr_platform_win::host::ui_dispatcher` (implements `pr_core::abstractions::{UiScheduler, UiDispatcher}` and `LoopDispatcher`) |
| X-host-wndproc-shell-window (`WndProcThunk`, `ShellWindow` creation, lifecycle, public API) | Partial (W8-13e ported; `show`/`show_command` of a visible window not exercised: windows stay hidden in tests) | W8-13e | see row | `pr_platform_win::host::{wndproc, shell_window::create}` |
| X-viewing-compare-layer (WP-21 `CompareLayout`/`CompareLayer`: panel geometry, borders, uniform images, hit test) | Partial (W8-16b: shell_viewing_compare_crossfade 29 pure; shell_viewing_crossfade_native 7 WARP spot checks; C# scene golden pending) | W8-16b | Native/desktop evidence; see row. | `pr_platform_win::shell_viewing::compare` |
| X-viewing-crossfade (WP-21 Q-R37 `CrossfadeLayer`: old image fades 1 -> 0 over `ImageTransitionMs`, QuadraticOut, restart not queue) | Partial (W8-16b: shell_viewing_compare_crossfade 29 pure; shell_viewing_crossfade_native 7 WARP spot checks; C# scene golden pending) | W8-16b | Native/desktop evidence; see row. | `pr_platform_win::shell_viewing::crossfade` |
| X-viewing-composer (WP-21 `PresentationComposer`: draw order, same-image upgrade prefetch with `MaxDeferMs`, settle filter, retained ids) | Partial (W8-16c: shell_viewing_composer 56 pure; shell_viewing_composer_native 4 WARP rows; remaining native rows pending) | W8-16c | Native/desktop evidence; see row. | `pr_platform_win::shell_viewing::composer` |
| X-scrollbars-state-layout (WP-26 `ShellScrollBars` part 1: colours, options, geometry queries, compare-panel occlusion, hover rule) | Partial (W8-16f; the mouse-driven `ShellScrollBarsTests` are W8-16g) | W8-16f | see row | `pr_platform_win::shell_viewing::scrollbars::{state, layout}` |
| X-input-shell-input (WP-22 `ShellInput`: K-01..K-30 key order, mouse regions, wheel, command execution; G-INPUT) | Partial (G-INPUT replay pending the synced golden) | none named | see row | `pr_platform_win::shell_input::dispatch` |
| X-input-main-window (WP-22 `ShellMainWindow.Input`: `RegisterInput`, host commands, zoom menu helpers, drop registration, `CreateViewportOptions`) | Partial (W8-14c): pure tests green; the end-to-end tests are written but cannot run before W8-13i..13k are merged | W8-14c | see row | `pr_platform_win::shell_input::main_window` |
| X-menu-native-popup-host (WP-18 / C-12 host `NativePopupMenuHost`: `CreatePopupMenu`, `TrackPopupMenuEx`) | Partial (W8-14f): ported; 28 tests pass in `crates/pr-platform-win/tests/shell_menus_host.rs` incl. the 16 native ones on a desktop run (hidden `STATIC` owner, keys posted first); stays Partial until the end-to-end run of `X-menu-main-window` | W8-14f | see row | `pr_platform_win::shell_menus::popup_host` |
| X-menu-main-window (WP-24 `ShellMainWindow.Menus`: `RegisterContextMenu`, `IContextMenuActions` of the window, `ReportMenuFailed`) | Partial (W8-14f): ported; 19 pure tests pass (`crates/pr-platform-win/tests/shell_menus_main_window.rs`); the 17 end-to-end tests (`crates/pr-platform-win/tests/shell_menus_e2e.rs`, `#[ignore]`) ran against a window with temporary 13i stubs: 12 pass, 5 zoom ones fail on a viewer `RefCell` double borrow of the 14c/13g integration (see the PR); stays Partial until a full desktop run | W8-14f | see row | `pr_platform_win::shell_menus::main_window` |
| X-menu-owner-draw-session (WP-24 `OwnerDrawMenuSession`: GDI painting of the dark menu, window subclass) | Partial (W8-14g): ported, 16 native tests pass on a desktop run; stays Partial because pixels depend on the machine font and DPI and the popup capture ports wait for W8-14f | W8-14g | see row | `pr_platform_win::shell_menus::owner_draw` |
| X-main-window-core (`ShellMainWindow` fields, constructor, `RegisterFeatures`, `AttachRenderer`, `OpenInitialAsync`, options) | Partial (code complete, `core.rs` has no `todo!()`; native tests pass only with temporary local stand-ins for the 13i/13j/13k stubs, so they run on master once those units land) | none named | see row | `pr_platform_win::host::main_window::core` |
| X-main-window-lifecycle (`IForwardedOpenTarget` members, decode box, change handling, `OnClosing`/`OnClosed`, `ReleaseRenderer`, `Dispose`) | Partial (code complete, `lifecycle.rs` has no `todo!()`; native tests pass only with temporary local stand-ins for the 13i/13j/13k stubs; not exercised: minimise/restore (would show a window), F11 placement pair (13k), a panicking `close_session`) | W8-17e | see row | `pr_platform_win::host::main_window::lifecycle` |
| X-main-window-frame (`ShellMainWindow.Frame` `Present`/`RenderNow`/first-present events; `ShellMainWindow.ScrollBars` glue) | Partial (code complete, `frame.rs` and `scrollbars.rs` have no `todo!()`; the native tests pass only with a temporary local stand-in for the 13j stubs, so they run on master once 13j lands; not exercised: minimised window (needs a zero client size), a deferred frame (`wait_for_next_frame` has no seam), the bar-press fan-out and the end-to-end bar tests (7 `ShellScrollBarEndToEndTests` stay with 14c / the B6 harness), first image (13j)) | none named | see row | `pr_platform_win::host::main_window::{frame, scrollbars}` |
| X-main-window-viewing (`ShellMainWindow.Viewing`: composer, prefetcher, image changes, GPU retain/clear, compare hit-test, `PrefetchHost`) | Partial (code complete, `viewing.rs` has no `todo!()`; the 10 native tests pass only with a temporary local stand-in for the 13i/13k stubs and need a desktop; the lead runs `--include-ignored` once before PARITY leaves Partial) | none named | see row | `pr_platform_win::host::main_window::viewing` |
| X-main-window-windowing (`ShellFullscreen`, `TitleBar`, `ShellMainWindow.Windowing`, real `WindowOps`, `Win32MonitorLayout`; WPF `FullscreenWindowPlacer` retired) | Partial (code complete, `windowing.rs` has no `todo!()`; the window-glue native tests pass only with temporary local stand-ins for the 13i/13j stubs, so they run on master once those land; not exercised: `ShellFullscreenFrameGeometryTests` (need 13j viewport+image), real maximised/minimised F11 (would show a window), real multi-monitor unplug) | W8-03a | see row | `pr_platform_win::host::main_window::windowing` |
| X-dialogs-painter (`NativeDialogChromePainter`: kinds, predicates, message table, colours, attach, subclass procedures) | Partial (W8-18l) | W8-18l | see row | `pr_platform_win::dialogs::native::painter::{mod, geometry}` |
| X-chrome-painter-controls (`NativeDialogChromePainter`: double buffering, `Fill`/`Frame`, text helpers, push button, list view selected row) | Partial (W8-18m) | W8-18m | see row | `pr_platform_win::dialogs::native::painter::controls` |
| X-chrome-painter-widgets (`NativeDialogChromePainter`: check box, radio, combo box, progress bar, trackbar) | Partial (W8-18n) | W8-18n | see row | `pr_platform_win::dialogs::native::painter::widgets` |
| X-chrome-painter-frame (`NativeDialogChromePainter`: group box, etched lines, header filler, flat borders, wiring through `DarkDialogChrome`) | Partial (W8-18n) | W8-18n | see row | `pr_platform_win::dialogs::native::painter::frame` |
| X-composition-texts-adapters (localized `*Texts`, `TextSource`, `FormatContextSource`, dialog/clipboard/sink bridges) | Partial (W8-09b S1 coded); Different (one type `LocalizedTexts` for twelve traits; `Rc` bridges because of the orphan rule) | W8-09b | see row | `pr_shell::composition::{texts, adapters}` |

### Different (15)

| ID | Status text (as in PARITY.md) | Owner unit | Evidence missing | Rust location |
|---|---|---|---|---|
| X-exe-versioninfo | Different (no `VERSIONINFO` resource, D-W8B6-53) | none named | Rust ADR or confirmation; see audit. | none stated in row |
| D-config-paths | Different (R0002) | none named | Rust ADR or confirmation; see audit. | `pr_core::app_paths` |
| P-07 | Different (R0002) | none named | ADR exists; separated Rust data root and environment overrides. | not stated in inventory C# source column |
| P-14 | Different (R0002) | none named | ADR exists; separate user language folder. Later `P-14 (localization: shipped + user catalogs)` tracking row is `Done (W2-04)`. | not stated in inventory C# source column |
| D-13 | Different (R0002) | none named | ADR exists; separate mutex and pipe names. | not stated in inventory C# source column |
| D-15 | Different (R0002) | none named | ADR exists; separate language/export folder. Later `D-15` tracking row is `Done (W2-04, read side)`. | not stated in inventory C# source column |
| F-06 (unexpected exception in the group compensation) | Different (W3-04, open question 2 option B) | W3-04 | Rust ADR or confirmation; see audit. | `FileActionService::execute_group` |
| F-12 (PowerLossSafe Prepared on a pool thread) | Different (W3-04, open question 6; R0003) | W3-04 | Rust ADR or confirmation; see audit. | `FileActionService::execute` |
| X-bench-cli-batch | Different; Partial (W7-03d repair) | W7-03d | Rust ADR or confirmation; see audit. | `pr_bench::cli::batch`, `pr_bench::cli::run`, `pr-bench` binary |
| X-image-presenter-stat-cancel | Different | none named | Rust ADR or confirmation; see audit. | `pr_app::coordinators::presenter::photo_info::ImagePresenter::submit_stat` |
| X-image-presenter-ownership | Different | W8-1 | Rust ADR or confirmation; see audit. | `pr_app::coordinators::presenter::state::{ImagePresenter, PresenterEnv, PresenterChanges}` |
| X-viewer-state-notify | Different | W8-1 | Rust ADR or confirmation; see audit. | `pr_app::viewmodels::viewer_state::{ViewerChanges, ViewerUpdate}` |
| D-actions-unknown-operation | Different (C# #533 `7eaee2a2`) | none named | Rust ADR or confirmation; see audit. | `pr_app::presenters::settings_field_map` (config.json load), `pr_app::presenters::action_list_json` |
| D-presentation-sink-cross-thread (ADR 0005 `InvokeUi` marshalling and `CrossThreadPresentCount`) | Different (compile-time; proposed by contract W8-16i, lead to confirm) | W8-16i | Rust ADR or confirmation; see audit. | `pr_platform_win::shell_viewing::presentation_sink` |
| X-exe-panic-exit | Different (an escaped panic ends the process with exit code 101, D-W8B6-54) | none named | Rust ADR or confirmation; see audit. | `pr_shell::entry` |

### Superseded (17)

| ID | Status text (as in PARITY.md) | Owner unit | Evidence missing | Rust location |
|---|---|---|---|---|
| I-02 / I-03 / INV-12 (decode types and traits) | Superseded by "Wave 4 final status": Doing (W4-01c seams; W4-01b decode types still Todo) | W4-01c | Reconcile with Wave 4 final status. | `pr_imaging::{decode_types, decoder_traits}` |
| I-18 (orientation, EXIF) | Superseded by "Wave 4 final status": Doing (W4-02a done: value rules, read order, transform table, `crates/pr-imaging/tests/orientation.rs` 13 tests incl. goldens `orientation-values`/`orientation-read-wic`/`orientation-pixels`; W4-03a done (TiffStructure, `tiff::tests::golden_tiff_structure`); W4-03b done (`pr_imaging::exif_parser`: marker walk, IFD0 + Exif IFD reader, orientation reader; `crates/pr-imaging/tests/exif_parser.rs` 55 tests incl. golden `exif-parser` 146 cases, 12 manual-mutation guards, no panic on truncated/corrupted input); W4-02b, W4-04a, W4-04b, W4-11c todo; W4-11d WicExifReader done: `wic::exif_reader` PROPVARIANT decode, orientation/EXIF independence, container roots, goldens `wic-exif-values` 62 + `wic-exif-variant` 59, S-30 one metadata reader per frame) | W4-02a | Reconcile with Wave 4 final status. | `pr_imaging::{orientation, embedded_thumbnail, tiff, exif_parser, exif_summary, exif_query}` |
| I-08 (cache files) | Superseded by "Wave 4 final status": Partial (W4-08a done: `atomic_file`; W4-08b todo) | W4-08a | Reconcile with Wave 4 final status. | `pr_imaging::cache::{atomic_file, preview_file}` |
| I-08 (cache files) / D-06 (entry format) | Superseded by "Wave 4 final status": Partial (W4-08b): entry file format (`preview_file`); `atomic_file` W4-08a; store, quota, persist queue are W4-09 / W4-14c | W4-08b | Reconcile with Wave 4 final status. | `pr_imaging::cache::{atomic_file, preview_file}` |
| I-08 / I-09 / C-05 / INV-2 (store) | Superseded by "Wave 4 final status": Partial (W4-09: store done; persist workers/queue/epoch W4-14c, thumbnail cache W4-10b; `write_atomically` tests wait for W4-08a) | W4-09 | Reconcile with Wave 4 final status. | `pr_imaging::cache::disk_store::DiskCacheStore` |
| I-18 (orientation, EXIF) | Superseded by "Wave 4 final status": Partial (W4-04a: `exif_summary` + `codec` done, 28/28 green (`ExifRational` lives in `tiff`, W4-03a merged), 24 mutants of the owned file killed, 1 equivalent (`year == 0` guard: `DateTimeUtc::from_ticks` rejects negative ticks); W4-02a, W4-02b, W4-03a, W4-03b, W4-04b still todo) | W4-04a | Reconcile with Wave 4 final status. | `pr_imaging::{orientation, embedded_thumbnail, tiff, exif_parser, exif_summary, exif_query}` |
| I-18 (orientation, EXIF) | Superseded by "Wave 4 final status": Todo (W4-02a, W4-02b, W4-03a, W4-03b, W4-04a, W4-04b contract + stub) | W4-02a | Reconcile with Wave 4 final status. | `pr_imaging::{orientation, embedded_thumbnail, tiff, exif_parser, exif_summary, exif_query}` |
| I-11 (scheduler) | Superseded by "Wave 4 final status": Todo (W4-15b contract + stub) | W4-15b | Reconcile with Wave 4 final status. | `pr_imaging::preload::{scheduler, worker, timer}` |
| I-08 / C-05 / INV-2 (store) | Superseded by "Wave 4 final status": Todo (W4-09 contract + stub) | W4-09 | Reconcile with Wave 4 final status. | `pr_imaging::cache::disk_store::DiskCacheStore` |
| I-09 | Superseded by "Wave 4 final status": Todo (W4-10b contract + stub) | W4-10b | Reconcile with Wave 4 final status. | `pr_imaging::cache::thumbnails::ThumbnailCache` |
| I-02 / I-03 / INV-12 (factory, fallback, routing) | Superseded by "Wave 4 final status": Doing (W4-12b fallback.rs; W4-12a Done: `factory.rs`, `crates/pr-imaging/tests/factory.rs`, golden `decoder-factory`; W4-12c Todo) | W4-12b | Reconcile with Wave 4 final status. | `pr_imaging::{factory, fallback, webp_heic}`; W4-12b: `fallback.rs`, `crates/pr-imaging/tests/fallback.rs` (goldens `fallback-is-fallbackable`, `fallback-decoder`) |
| I-02 / I-03 / INV-12 (factory, fallback, routing) | Superseded by "Wave 4 final status": Todo (W4-12a, W4-12b); routing Done (W4-12c) | W4-12a | Reconcile with Wave 4 final status. | `pr_imaging::{factory, fallback, webp_heic}` |
| I-02 / I-12 / I-20 / INV-8 (WIC boundary) | Superseded by "Wave 4 final status": Doing (W4-11a interop) | W4-11a | Reconcile with Wave 4 final status. | `pr_platform_win::wic::interop` (`crates/pr-platform-win/src/wic/interop.rs`, `crates/pr-platform-win/tests/wic_interop.rs`: 73 tests, 8 native `#[ignore]` pass with `--include-ignored`; 2 unit tests in `interop.rs`) |
| I-02 / I-03 / I-20 / INV-8 / INV-12 (WIC decode) | Superseded by "Wave 4 final status": Doing (W4-11c WicDirectDecoder ported + repair round 1; W4-11d separate) | W4-11c | Reconcile with Wave 4 final status. | `pr_platform_win::wic::{decoder, exif_reader}` (`decoder.rs` incl. 8 unit tests; `crates/pr-platform-win/tests/wic_decoder.rs` 38, `crates/pr-platform-win/tests/wic_decoder_buffers.rs` 6, `crates/pr-platform-win/tests/wic_decoder_native.rs` 69, native `#[ignore]` pass with `--include-ignored` once W4-11a/W4-11d/W4-01b are merged) |
| I-02 / I-03 / I-20 (WIC decode) | Superseded by "Wave 4 final status": Todo (W4-11c, W4-11d contract + stub) | W4-11c | Reconcile with Wave 4 final status. | `pr_platform_win::wic::{decoder, exif_reader}` |
| I-04 / I-07 / I-08 / I-16 / INV-1 / INV-2 (service) | Superseded by "Wave 4 final status": Todo (W4-14a, W4-14b, W4-14c contract + stub; W4-14d proposed) | W4-14a | Reconcile with Wave 4 final status. | `pr_imaging::service::*` |
| I-04 / I-07 / I-08 / I-16 / INV-1 / INV-2 (service) | Superseded by "Wave 4 final status": Todo (W4-14a, W4-14b, W4-14c contract + stub; W4-14d adapter `impl PreloadTarget for PreviewImageService` written + 5 mapping unit tests in `service::preload_adapter_tests` green; its 9 integration tests in `crates/pr-imaging/tests/preview_service_preload.rs` stay red, BLOCKED: `service/disk.rs` (W4-14c) and `service/decode.rs` (W4-14b) are still `todo!()` on integration/wave-4, so no `PreviewImageService` can be built; mutants on the `try_get_cached_preview*`/`cached_preview_bytes`/`has_disk_cached_preview`/`preload` cancel arm are only killable after 14b/14c merge) | W4-14a | Reconcile with Wave 4 final status. | `pr_imaging::service::*` |

### Contract (47)

| ID | Status text (as in PARITY.md) | Owner unit | Evidence missing | Rust location |
|---|---|---|---|---|
| X-viewport-layout | Contract (W8-01b) | W8-01b | Implementation and proving evidence; see row. | `pr_app::viewport::layout` |
| X-render-geometry | Contract (W8-01c) | W8-01c | Implementation and proving evidence; see row. | `pr_app::viewport::render_geometry` |
| X-fit-zoom-rules | Contract (W8-01d) | W8-01d | Implementation and proving evidence; see row. | `pr_app::{viewport::fit, menus::zoom_rules}` |
| X-menu-model | Contract (W8-02a) | W8-02a | Implementation and proving evidence; see row. | `pr_app::menus::{model, builder}` |
| X-format-exif, X-format-status, X-format-title-bar, X-settings-reload-path (S-30, S-31, S-41) | Contract (W8-02b) | W8-02b | Implementation and proving evidence; see row. | `pr_app::viewmodels::format` |
| X-window-placement (P-13, K-21, W-01) | Contract (W8-03a); Different (invalid UTF-8 is an I/O error) | W8-03a | Implementation and proving evidence; see row. | `pr_app::windowing::{contracts, monitor_layout, visibility, placement_store}` |
| X-fullscreen (K-21) | Contract (W8-03b) | W8-03b | Implementation and proving evidence; see row. | `pr_app::windowing::fullscreen` |
| X-app-services (K-32 shortcut part) | Contract (W8-04c); Different (prewarm and predictor errors give `None`) | W8-04c | Implementation and proving evidence; see row. | `pr_app::services::*` |
| X-app-log, X-build-info (Different), X-launch-args, X-task-logging (P-08 note) | Contract (W8-04d) | W8-04d | Implementation and proving evidence; see row. | `pr_app::misc::{app_log, build_info, launch_args, task_logging}` |
| X-file-hash, X-viewport-convergence, X-window-helpers; V-FWD (Not needed) | Contract (W8-04e) | W8-04e | Implementation and proving evidence; see row. | `pr_app::misc::{file_hash, viewport_convergence, window_helpers}` |
| X-input-core | Contract (W8-05a) | W8-05a | Implementation and proving evidence; see row. | `pr_app::input::{frame_clock, operation_version, wheel_message, surface, review_command}` |
| X-keyid-mirror | Contract (W8-05b) | W8-05b | Implementation and proving evidence; see row. | `pr_app::input::key_id` |
| K-43 (key mapping) | Contract (W8-05c) | W8-05c | Implementation and proving evidence; see row. | `pr_app::input::key_mapping` |
| K-* (shortcut router) | Contract (W8-05d) | W8-05d | Implementation and proving evidence; see row. | `pr_app::input::shortcut_router` |
| K-33, K-34, K-35 (mouse and touchpad gestures) | Contract (W8-05e) | W8-05e | Implementation and proving evidence; see row. | `pr_app::input::{mouse_gestures, touchpad_gestures}` |
| X-kinetic-pan | Contract (W8-05f) | W8-05f | Implementation and proving evidence; see row. | `pr_app::input::kinetic_pan` |
| X-viewer-state | Contract (W8-06a) | W8-06a | Implementation and proving evidence; see row. | `pr_app::viewmodels::viewer_state` |
| X-info-overlay (K-40) | Contract (W8-06b) | W8-06b | Implementation and proving evidence; see row. | `pr_app::{viewmodels::info_overlay, coordinators::{autohide, fade_gate, transition}}` |
| X-coordinator-traits | Contract (W8-06d) | W8-06d | Implementation and proving evidence; see row. | `pr_app::coordinators::traits` |
| X-action-profiles | Contract (W8-10a); Different (null list item rejected, I/O message carries the path) | W8-10a | Implementation and proving evidence; see row. | `pr_app::presenters::{action_list_json, action_profiles}` |
| X-settings-field-map | Contract (W8-10b); Different (undefined enum values) | W8-10b | Implementation and proving evidence; see row. | `pr_app::presenters::settings_field_map` |
| X-secondary-presenters | Contract (W8-10c) | W8-10c | Implementation and proving evidence; see row. | `pr_app::presenters::{settings_form_state, secondary}` |
| X-recovery-presentation | Contract (W8-10f) | W8-10f | Implementation and proving evidence; see row. | `pr_app::presenters::{recovery_filter, recovery_presentation}` |
| X-overlay-animator, X-overlay-frame-delay, X-overlay-palette | Contract (W8-15b) | W8-15b | Implementation and proving evidence; see row. | `pr_app::overlay::{animator, frame_delay, palette}` |
| X-scrollbar | Contract (W8-16e) | W8-16e | Implementation and proving evidence; see row. | `pr_app::viewing::scrollbar_geometry`, `pr_platform_win::shell_viewing::scrollbar_native` |
| X-pointer-input (K-33..K-36, K-27 pan) | Contract (W8-05g, W8-05h) | W8-05g | Implementation and proving evidence; see row. | `pr_app::input::pointer::*` |
| X-compare-view-model | Contract (W8-06c) | W8-06c | Implementation and proving evidence; see row. | `pr_app::viewmodels::compare` |
| X-file-action-gate (INV-4 caller side) | Contract (W8-06e) | W8-06e | Implementation and proving evidence; see row. | `pr_app::coordinators::file_action_gate` |
| X-zoom-detail | Contract (W8-07g) | W8-07g | Implementation and proving evidence; see row. | `pr_app::coordinators::zoom_detail` |
| X-dialogs-pickers (`FolderPicker`, `NativeShellFolderDialog`, `NativeShellFileDialog`, `TaskDialogs`, `Win32DialogService`, `Win32ZoomPromptService`, `Win32ClipboardService`) | Contract (W8-18a, 18b, 18c); Different (`pick_folder` logs a COM failure and answers `None`, D-W8B5-2 / 1008) | W8-18a | Implementation and proving evidence; see row. | `pr_platform_win::dialogs::{folder_dialog, file_dialog, task_dialog, dialog_service, zoom_prompt_service, clipboard}` |
| X-dialogs-native-base (`DialogTemplate`, `DarkDialogChrome`, `EditTextCentering`, `ListDialog`, `NativeControls`) | Contract (W8-18d, 18e, 18f, 18g); `ListDialog` follows the baseline (post-baseline `ForChoice` not ported) | W8-18d | Implementation and proving evidence; see row. | `pr_platform_win::dialogs::native::{template, chrome, edit_centering, list_dialog, controls}` |
| X-dialogs-native-dialog (`NativeDialog` base class) | Contract (W8-18h..18k) | W8-18h | Implementation and proving evidence; see row. | `pr_platform_win::dialogs::native::dialog::*` |
| X-dialogs-secondary-port (`Win32SecondaryWindowPort`, `SecondaryWindowRouter`) | Contract (W8-18o); production `SecondaryWindowDialogs` is part of W8-17c (P-43) | W8-18o | Implementation and proving evidence; see row. | `pr_platform_win::dialogs::secondary_window_port` |
| X-dialogs-zoom-diagnostics-profiles-benchmark (`ZoomPromptDialog`, `DiagnosticsDialog`, `ActionProfilesDialog`, `BenchmarkDialog`) | Contract (W8-18p, 18q, 18r, 18s); Different (benchmark window closed mid-run, D-W8B5-506) | W8-18p | Implementation and proving evidence; see row. | `pr_platform_win::dialogs::{zoom_prompt, diagnostics, action_profiles, benchmark::*}` |
| X-dialogs-recovery (`RecoveryDialog`) | Contract (W8-18t, 18u, 18v) | W8-18t | Implementation and proving evidence; see row. | `pr_platform_win::dialogs::recovery::*` |
| X-settings-ui-framework (`SettingsPageContext`, `SettingsPageCatalog`, layout elements, `SettingsPageBuilder`) | Contract (W8-19a, 19b, 19c) | W8-19a | Implementation and proving evidence; see row. | `pr_platform_win::dialogs::settings::{page_view, catalog, context, builder::*}` |
| X-settings-ui-pages (Diagnostics, Performance, Display, Files, General, Mouse, Shortcuts pages) | Contract (W8-19d..19i); Partial (Shortcuts uses English action names until `settings_texts()` exists, D-W8B5-806); baseline pages only (post-baseline external editors, P-44) | W8-19d | Implementation and proving evidence; see row. | `pr_platform_win::dialogs::settings::pages::*` |
| X-settings-ui-window (`SettingsDialog` frame, state and key capture, commands and messages, layout and close) | Contract (W8-19j..19m) | W8-19j | Implementation and proving evidence; see row. | `pr_platform_win::dialogs::settings::dialog::*` |
| X-startup-launch (`ShellLaunch`, `ShellInstanceNames`, `ForwardedOpenHandler`) | Contract (W8-17a) | W8-17a | Implementation and proving evidence; see row. | `pr_app::startup::{launch, forwarded_open, context}` |
| X-startup-error-log (`ShellErrorLog`) | Contract (W8-17a); Different (type names and stacks are Rust's, D-W8B6-3) | W8-17a | Implementation and proving evidence; see row. | `pr_app::startup::error_log` |
| X-startup-cloak-placement (`StartupCloak`, window placement apply/save, show command) | Contract (W8-17b) | W8-17b | Implementation and proving evidence; see row. | `pr_platform_win::shell_startup::{cloak, placement, dpi}` |
| X-startup-composition-root (`ShellCompositionRoot`, `SecondaryWindowDialogs`, warm-ups, locale services) | Contract (W8-17c); Different (`ShellNoticeSink`, `SecondaryWindowFallback`, `UnwiredShellFolderDialog` retired, D-W8B6-8; non-en-US percent patterns approximate, D-W8B6-9) | W8-17c | Implementation and proving evidence; see row. | `pr_shell::startup::*`, `pr_platform_win::shell_startup::{warmup, locale}` |
| X-startup-options-run (`ShellStartup` options, run, window, recovery, smoke, shutdown) | Contract (W8-17d, 17e); Different (general log timestamps are UTC, D-W8B6-38; no `UnobservedTaskException`) | W8-17d | Implementation and proving evidence; see row. | `pr_app::startup::*`, `pr_shell::startup::*` |
| X-exe-entry (`Program.Main`, STA, manifest, icon group) | Contract (W8-20a) | W8-20a | Implementation and proving evidence; see row. | `pr_shell::{main, entry}`, `build.rs`, `shell_startup::apartment`, `exe_resources` |
| X-ui-smoke (UI smoke tests and parity map of the shell) | Contract (W8-20b) | W8-20b | Implementation and proving evidence; see row. | planned shell UI smoke tests and parity map; `pr_platform_win::ui_driver` |
| X-composition-registry (`SharedServiceRegistration.AddPhotoReviewShared`, `AppHost.BuildServices`; C-16 shared graph) | Contract (W8-09b S3); Different (no DI container: lazy `OnceLock` cells, P-38; null-argument and missing-host-service tests unrepresentable; Wpf registrations retired) | W8-09b | Implementation and proving evidence; see row. | `pr_shell::composition::{registry, platform}`, `pr_shell::composition::graph` (`GraphOverrides`, `production_graph_builder_with`) |
| X-composition-main-view-model (`MainViewModelCompositionRoot`, `ApplyInitialView`) | Contract (W8-09b S4); Different (no VM/sink cycle: `PresentationBridge`; preload entries through `CatalogFeed`, amendment A12, D-W8B6-155) | W8-09b | Implementation and proving evidence; see row. | `pr_shell::composition::main_view_model` |

### Implementation Done (qualified) (3)

| ID | Status text (as in PARITY.md) | Owner unit | Evidence missing | Rust location |
|---|---|---|---|---|
| I-13 (container readers) | Implementation Done (W5-03..10b; corpus validation remains unavailable) | W5-03 | RAW corpus evidence; see row. | `pr_imaging_raw::readers::{cr2, nef, orf, dng, arw, rw2, raf, cr3}` |
| I-13 / I-14 / INV-1 / INV-8 / INV-12 (RawDecoder, routing) | Implementation and golden cases Done (W5-12b1 core/fallback and W5-12c routing integrated); broad mutation identity adjudication complete, original historical 277-set remains unreconstructed | W5-12b | Mutation verdict; see row. | `pr_imaging_raw::{decoder, decoder::fallback, routing}` |
| I-02 / I-03 / I-12 / I-18 / I-20 / INV-8 / INV-12 (TurboJPEG decoder) | Implementation Done (W5-14b); mutation verification Partial | W5-14b | Mutation verdict; see row. | `pr_turbojpeg::decoder` |

### Ported (3)

| ID | Status text (as in PARITY.md) | Owner unit | Evidence missing | Rust location |
|---|---|---|---|---|
| F-14 / F-11 (FA-01) / F-15 / F-16 (startup hook) / INV-6 / INV-10 (reconcile) | Ported (W3-03; its 78 tests + 8 native twins are green only once W3-01's `OperationJournal` bodies and the `ReconcileJournal` impl land) | W3-03 | Journal integration condition; see row. | `pr_core::file_actions::{reconcile, startup_recovery}` |
| F-17 | Ported (W3-07) | W3-07 | see row | `pr_core::file_actions::recovery_check::RecoveryFileCheck` |
| X-platform-adapters (`WindowsShellIntegration`, Explorer provider constructor, physical memory, culture and date text, WIC payload codec and cache image reader) | Ported (W8-09b S2); native tests run once by hand on a desktop; no mutation run (native glue); `X-embedded-thumbnail-backend` stays open (O1) | W8-09b | Embedded thumbnail backend still open; see row. | `pr_platform_win::{shell_integration, explorer_order_provider, physical_memory, culture, wic::cache_adapters}` |

## Difference ADR audit

Every tracking row whose status cell contains `Different` is listed, including Done, Partial and Contract rows, plus the four `Different (R0002)` inventory rows. Review note-only divergences as well under `AGENTS.md`. Titles are proposals; assign the next ADR number when written. A C# ADR alone does not satisfy the Rust ADR rule.

| PARITY row | ADR disposition / proposed title | Rationale from row |
|---|---|---|
| D-config-paths | ADR exists: `R0002` (verify its scope) | Same layout as `AppPaths.cs`, root `PhotoReviewRs` |
| P-07 | ADR exists: `docs/adr/R0002-coexistence-with-csharp-app.md` | Rust uses `%LOCALAPPDATA%\PhotoReviewRs` and `PHOTOREVIEW_RS_*`; the C# app keeps its separate root and variables. |
| P-14 | ADR exists: `docs/adr/R0002-coexistence-with-csharp-app.md` | Shipped catalogs are synced; Rust user catalogs use a separate `PhotoReviewRs\Languages` folder. Later `P-14 (localization: shipped + user catalogs)` is `Done (W2-04)`. |
| D-13 | ADR exists: `docs/adr/R0002-coexistence-with-csharp-app.md` | Rust uses `PhotoReviewRs` mutex and forward-pipe prefixes while C# retains `PhotoReview` names. |
| D-15 | ADR exists: `docs/adr/R0002-coexistence-with-csharp-app.md` | Rust user language files and exports use the separate `PhotoReviewRs\Languages` folder. Later `D-15` is `Done (W2-04, read side)`. |
| F-12 / F-15 (transaction) | ADR exists: `R0003` (verify its scope) | The row says the PowerLossSafe Prepared pool-thread step has no twin; R0003 uses blocking begin. |
| F-13 | ADR-<next id>? Unrepresentable path in durable journal append (number assigned when written) | The note says a Rust String cannot contain an unpaired surrogate, so this ThrowIfUnrepresentable arm cannot run here. |
| F-06 (unexpected exception in the group compensation) | ADR-<next id>? Unexpected group-compensation exception (number assigned when written) | C# catches a non-IO exception of the compensation/inspection and returns an unknown-state result; Rust has no such arm (every fake error is an `io::Error`, already handled per... |
| F-12 (PowerLossSafe Prepared on a pool thread) | ADR exists: `R0003` (verify its scope) | No thread hop: the whole action runs on the caller's (file-action lane) thread; only the append(durable) < move < append(durable) order is ported |
| F-18 / F-06 (group retry) / F-03 (F-WIN-2 on retry) / INV-4 (retry gate) / INV-6 (retry journal sequence) | ADR exists: `R0003` (verify its scope) | The note says the blocking API replaces C# off-caller-thread retry tests (R0003). |
| X-bench-recycle-bin | ADR exists: `R0002` (verify its scope) | Env var `PHOTOREVIEW_RS_BENCH_REAL_RECYCLE_BIN` instead of `PHOTOREVIEW_BENCH_REAL_RECYCLE_BIN` (R0002, D4, D64); File.Delete semantics pinned by golden (read-only file throws)... |
| X-bench-cli-batch | ADR-<next id>? Benchmark CLI output and failure handling (number assigned when written) | The note says the DONE line uses invariant F1, unlike C# current culture; failure output and per-profile failure handling also differ. |
| X-viewport-state | ADR-<next id>? Arrow panning rule (number assigned when written) | The status says K-27 uses a new ArrowPanRule; the note does not explain the rule. See row. |
| X-window-placement (P-13, K-21, W-01) | ADR-<next id>? Invalid UTF-8 placement file handling (number assigned when written) | The status says invalid UTF-8 is an I/O error; the note confirms the on-disk format remains identical. |
| X-localization-service (I18N L08, L09, Reload translations message) | ADR-<next id>? Localization change notification and culture (number assigned when written) | The status says lookup/change notification replaces LocalizationSource and there is no process-wide UI culture equivalent. |
| X-app-services (K-32 shortcut part) | ADR-<next id>? Prewarm and predictor failure results (number assigned when written) | The status says prewarm and predictor errors return None; the note names the new key validator. |
| X-action-profiles | ADR-<next id>? Action profile import validation (number assigned when written) | The status says null list items are rejected and I/O messages carry a path; the note does not explain this difference. |
| X-settings-field-map | ADR-<next id>? Undefined settings enum values (number assigned when written) | The status identifies undefined enum values; the note says this row depends on 10a and 10c. |
| X-image-presenter-stat-cancel | ADR-<next id>? Cancelled image stat event (number assigned when written) | A stat cancelled before it ran (the C# `OperationCanceledException` of `StatOffUiThreadAsync`) is "no event" in Rust, so the `ShowImage superseded during stat` log line has no... |
| X-image-presenter-ownership | ADR-<next id>? Image presenter ownership and completion pump (number assigned when written) | The presenter owns its sink, the compare view model and the zoom loader and borrows catalog, settings, session and format context per call (`PresenterEnv`); completions are... |
| X-settings-presenter | ADR-<next id>? Settings update check step machine (number assigned when written) | The status says the update check uses begin/run/finish steps with the same observable results. |
| X-viewer-state-notify | ADR-<next id>? Viewer state change notifications (number assigned when written) | C# `PropertyChanged` / `ZoomModeChanged` / `SourceSizeSwapping` become a change set and return values (D-W8-1, D-06a-2) |
| D-actions-unknown-operation | ADR-<next id>? Unknown operation in config loading (number assigned when written) | The note says C# config load maps an unknown Operation to Move, while Rust drops the actions list. |
| D-presentation-sink-cross-thread (ADR 0005 `InvokeUi` marshalling and `CrossThreadPresentCount`) | ADR-<next id>? Cross-thread presentation sink (number assigned when written) | The Rust sink and hub are `!Send`, so an off-thread update cannot be written; the C# queue / FIFO guard and the counter (always 0) are not ported |
| X-dialogs-pickers (`FolderPicker`, `NativeShellFolderDialog`, `NativeShellFileDialog`, `TaskDialogs`, `Win32DialogService`, `Win32ZoomPromptService`, `Win32ClipboardService`) | ADR-<next id>? Folder picker COM failure result (number assigned when written) | The status says pick_folder logs a COM failure and returns None; the note does not explain this difference. |
| X-dialogs-zoom-diagnostics-profiles-benchmark (`ZoomPromptDialog`, `DiagnosticsDialog`, `ActionProfilesDialog`, `BenchmarkDialog`) | ADR-<next id>? Closing benchmark window during a run (number assigned when written) | The status records a difference when the benchmark window closes mid-run; the note only names the pr-bench dependency. |
| X-startup-error-log (`ShellErrorLog`) | ADR-<next id>? Rust error names and stacks in shell log (number assigned when written) | The status says Rust type names and stacks differ; the note says the text format is byte-identical. |
| X-startup-composition-root (`ShellCompositionRoot`, `SecondaryWindowDialogs`, warm-ups, locale services) | ADR-<next id>? Startup service and locale substitutions (number assigned when written) | The status says ShellNoticeSink, SecondaryWindowFallback and UnwiredShellFolderDialog retire, and non-en-US percent patterns approximate. |
| X-startup-options-run (`ShellStartup` options, run, window, recovery, smoke, shutdown) | ADR-<next id>? UTC startup log and unobserved tasks (number assigned when written) | The status says general log timestamps use UTC and no UnobservedTaskException hook exists. |
| X-exe-panic-exit | ADR-<next id>? Escaped panic exit code (number assigned when written) | The status says an escaped panic exits with code 101; the note gives no further rationale. |
| X-exe-versioninfo | ADR-<next id>? Missing VERSIONINFO resource (number assigned when written) | The status says there is no VERSIONINFO resource; the note calls it an optional follow-up. |
| X-composition-registry (`SharedServiceRegistration.AddPhotoReviewShared`, `AppHost.BuildServices`; C-16 shared graph) | ADR-<next id>? Composition registry without DI container (number assigned when written) | The status says lazy OnceLock cells replace a DI container and some null/host tests are unrepresentable. |
| X-composition-main-view-model (`MainViewModelCompositionRoot`, `ApplyInitialView`) | ADR-<next id>? Main view model composition without reference cycle (number assigned when written) | The status says PresentationBridge avoids a VM/sink cycle; the note only lists blocking items. |
| X-composition-texts-adapters (localized `*Texts`, `TextSource`, `FormatContextSource`, dialog/clipboard/sink bridges) | ADR-<next id>? Localized text adapters and bridges (number assigned when written) | The status says one LocalizedTexts type implements twelve traits and Rc bridges satisfy the orphan rule. |
| X-embedded-thumbnail-backend (`EmbeddedThumbnailReader` over WIC) | ADR-<next id>? Unavailable embedded thumbnail backend (number assigned when written) | The status says unavailable embedded thumbnails make the cache decode the file; the note says no unit owns the WIC backend. |

## Exit check

Run from the repo root in PowerShell. These searches print remaining candidates. The lead must still resolve repeated IDs, qualified Done rows and ADR coverage. They do not run native or benchmark tests.

```powershell
Select-String -Path docs/PARITY.md -Encoding UTF8 -Pattern '^\| [^|]+ \| (Todo|Doing|Partial|Contract|Ported|Implementation Done|Implementation and golden cases Done)\b' | Where-Object { $_.LineNumber -le 132 -or $_.LineNumber -ge 426 }
Select-String -Path docs/PARITY.md -Encoding UTF8 -Pattern '^\| [^|]+ \| (Different|Done[^|]*Different|Todo[^|]*Different|Partial[^|]*Different|Contract[^|]*Different)' | Where-Object { $_.LineNumber -le 132 -or $_.LineNumber -ge 426 }
Select-String -Path docs/PARITY.md -Encoding UTF8 -Pattern '^\| [^|]+ \| Superseded\b' | Where-Object { $_.LineNumber -le 132 -or $_.LineNumber -ge 426 }
Select-String -Path docs/PARITY.md -Encoding UTF8 -Pattern '^\| (P-03|P-04|P-05|P-11|P-07|P-14|D-13|D-15) \| (Doing|Different) \('
```

Inspect the Wave 4 `| Unit | Status |` summary and later rows too. Reconcile the eight inventory rows with their later status entries rather than requiring the historical inventory text itself to disappear. Review `docs/adr/` links and `docs/SYNC-LEDGER.md`. Phase 7 separately requires a `pr_bench` result **equal to or better than C#** and the two-week trial (`docs/PLAN.md`, Phase 7).

## Open items

| Item | State |
|---|---|
| Authoritative disposition for repeated historical IDs | TBD (not in the repo) |
| Accepted Rust ADRs for differences beyond `R0002`/`R0003` scope | TBD (not in the repo) |
| Rationale in the note cell for status-only differences where that note is silent | TBD (not in the repo) |
| Missing Rust locations in Wave 4/W5 summary rows | TBD (not in the repo) |
| Final native, GUI, RAW corpus and mutation evidence for qualified rows | TBD (not in the repo) |
| Comparative C# and Rust benchmark result and trial/release decision | TBD (not in the repo) |
