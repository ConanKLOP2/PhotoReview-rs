# Parity checklist (C# → Rust)

Inventory of the C# app at baseline `e441d67a` (see SYNC-LEDGER.md), generated 2026-10-14 by a read-only pass over the C# code. C# paths are relative to the C# repo root.

**Status tracking:** an item is done when its ID appears in the table below with the Rust test that proves it. An intentional difference needs an ADR.

| ID | Status (Todo / Doing / Done / Different) | Rust location | Proving test | Note |
|---|---|---|---|---|
| D-config-paths | Different (R0002) | `pr_core::app_paths` | `new_never_points_into_csharp_app_root` | Same layout as `AppPaths.cs`, root `PhotoReviewRs` |
| X-datetime-json | Done | `pr_core::time::DateTimeUtc` | `golden_text_time::datetime_json_*` (golden `datetime-json`) | Every on-disk UTC timestamp (journal, session, fingerprint); .NET ticks |
| X-ordinal-ignore-case | Done | `pr_core::text` | `golden_text_time::ordinal_ignore_case_matches_dotnet` | Table generated from .NET 10 (1422 mappings) |
| X-dotnet-path | Done (W2-00: 3 mutants killed by 3 added tests) | `pr_core::text::dotnet_path` | `text_dotnet_path::*` (golden `dotnet-path`) | .NET Path.* on Windows; underpins P-01, P-02, I-17, C-22, C-25, F-05, K-39 |
| Q-R29c | Done (W2-15) | `pr_core::io::navigation_stat` | `io_navigation_stat::*` (golden `navigation-stat-worker`) | Stat off the UI thread: one FIFO thread; panic isolation; thread priority AboveNormal left to the App layer (owner ImagePresenter, App wave W8) |
| X-abstractions | Done (traits) | `pr_core::abstractions` | per-trait unit tests; fakes in `pr-test-support` | Port of `Core/Abstractions` per R0003; `VBlankEstimator` → W1-22, `ExplorerOrderTypes` moved here from W1-07 |
| X-benchmark-percentile | Done (W1-15) | `pr_core::diagnostics::bench_stats` | golden `percentile` | No C# PARITY row; BenchmarkStatistics |
| X-perf-analyzer | Done (W1-16..21) | `pr_perf` | goldens `perf-csv`, `perf-nav`, `perf-stats`, `perf-rules`, `perf-report`, `perf-analyze` | PerfAnalysis tool; output byte-identical (CRLF). Accepted divergences (README D5): UTF-16 BOM side files, `-0`/`1e400` number tokens, junction-cycle protection |
| X-wave1-units | Done (22/22 when W1-20 dedupe lands) | see contracts/README.md roster | per-unit tests + goldens | Wave 1 leaf units: model enums, lenient JSON, flag fields, shortcuts, perf options, natural comparer, catalog types, LRU, file-action records, update version/URL policy, LocTemplate, GenerationClock, journal errors and line format, percentile/DiagOptions, perf analyzer, VBlankEstimator |
| C-07..C-09, C-11, C-13, C-15, C-16, K-38, S-ctxmenu | Partial (W2-20) | `pr_core::settings::context_menu` | `settings_context_menu::*` (golden `context-menu`, 510 cases) | Context menu item ids, item table, hidden-set parsing (`Enum.TryParse` quirks emulated), legacy flag migration, pure visibility rule; S-ctxmenu = hidden items list / legacy flags |
| P-02, S-05, K-05, K-06 | Done (W2-05) | `pr_core::catalog::sorting` | `catalog_sorting::*` (goldens `image-sort-entries`, `image-sort-paths`, `sibling-folders`; native twins `#[ignore]`) | Fallback sort (stable, NameDescending = reversed ascending, `ToUpperInvariant` quirk for `ſ`/`ı`), sibling folder list (Hidden/System skipped via std::fs attributes) |
| X-physical-fs | Partial (W2-14) | `pr_core::io::physical::PhysicalFileSystem` | `io_physical::golden_physical_fs_ops` (145 cases, native), `golden_physical_fs_pattern_*`, `golden_physical_fs_read_text_*`, `golden_physical_fs_resolve` (`native`, passes with `--include-ignored`), `file_system_contract_*` | Real-disk `IFileSystem`; never-overwrite move/copy through `MoveFileExW`/`CopyFileExW`, `DeleteFileW` (private FFI). Copy onto a directory is an `IsADirectory` error without OS code (.NET IOException HResult 5). Listings and FileStream-style opens use `GetFullPath` (trailing space/dot, `/`) and the .NET directory-handle codes (file => 87, missing => 3); `create_directory` ports `Directory.CreateDirectory` (dangling junction Ok, device name raw 3), not `create_dir_all`; `create_directory_*`, `enumerate_*_normalised` |
| X-counting-fs | Partial (W2-14) | `pr_core::io::counting::CountingFileSystem` | `io_counting::*`, `counting_file_system_forwards_every_method_to_the_inner_file_system` | Every trait method forwarded (also the defaulted ones); stat queries counted first |
| S-64 | Partial (W2-03; repair-dependent tests need W2-02 `normalizer`) | `pr_core::settings::store::SettingsStore::{load, parse_text, parse, migrate, try_salvage}` | `settings_store::*`; goldens `settings-store`, `settings-parse-text` | Retries 50/100/200/400 ms, salvage + `.corrupt-` backup, `.repaired-` backup before writing repairs, newer-build file never rewritten on load, explicit path never writes the default file |
| S-65 | Partial (W2-03) | `pr_core::settings::store::SettingsStore::{save, persist}` | `settings_store::save_*`, `settings_save_uses_durable_atomic_write`, golden `settings-store` | Canonicalise, stamp 3, durable atomic write, `Changed` once; `.newer-` backup (3 kept) before overwriting a newer-build file |
| INV-11 | Partial (W2-03) | `pr_core::settings::store::SettingsStore::load` | `settings_store::load_inv11_*`, `load_config_locked_later_save_keeps_the_valid_file`, `load_corrupt_config_backup_fails_later_save_keeps_the_corrupt_file` | Corrupt config backed up and reset; locked config gives in-memory defaults and is never overwritten |
| D-02 | Partial (W2-03) | `pr_core::settings::store::SettingsStore::unique_backup_path` | `settings_store::load_corrupt_config_with_*_backup_name_*`, `backup_stamp_is_the_utc_time_of_the_injected_clock` | `config.json.corrupt-yyyyMMddHHmmss`, `-2` .. `-99`; the clock is injected |
| D-10 | Partial for the store part (W2-03) | `pr_core::settings::store::SettingsStore::parse_text` | golden `settings-parse-text` | Import uses the same parse pipeline; the dialog itself is a later wave |
| C-28 | Partial for the repair list (W2-03) | `SettingsStore::last_load_repairs`, `parse_text(..).repairs()` | golden `settings-parse-text` (`repairs`, `repair_text`), `settings_store::last_load_repairs_is_reset_*` | The dialog text is W2-02 `build_repair_text`; the dialog is a later wave |
| X-config-import | Partial (W2-03; Rust-only, R0002) | `pr_core::settings::store::SettingsStore::load_first_run_with_import`, `csharp_config_path` | `settings_store::import_*` | First-run read-only import of the C# `config.json`; no write, copy or backup under the C# folder (asserted through the fake file system counts) |
| D-05 | Partial (W2-14, physical part) | `PhysicalFileSystem::write_all_text_atomic` | golden `physical-fs-ops` `writeatomic:*`, `physical_non_durable_write_*`, `write_all_text_atomic_concurrent_writers_leave_a_complete_file` | `<target>.<guidN>.tmp`, durable = write-through + fsync; rename retried 8x on contention (`replace_with_retry`) |
| RV-C05 | Partial (W2-14) | `PhysicalFileSystem::write_all_text_atomic` | `write_all_text_atomic_rename_fails_leaves_no_temp_file`, ops `tmp_files` probes | No temp file outlives a failed rename |
| R01 | Partial (W2-14, physical part) | `PhysicalFileSystem::try_copy_new_with_proof` | ops `trycopynew-proof:*` | Proof raised only after the copy returned |
| AR16 | Partial (W2-14) | `PhysicalFileSystem::enumerate_files_with_stat` | `fast_listing_does_not_probe_locked_file_is_listed_and_the_probe_reports_it`, ops `enumstat:*` | Stat from the directory entry, no per-file open; ListingInterrupted after the first entry (ADR 0007 section 3) |
| INV-8 | Doing (W2-14: file-system part done) | `PhysicalFileSystem::open_read_shared` | ops `openread-then-rename` | Share ReadWrite or Delete + SequentialScan; decoders come later |
| SEC-01 / F-05 | Partial (W2-08 policy; W2-14 resolver) | `PhysicalFileSystem::resolve_real_path`; `file_actions::destination_policy` | ops `resolve:*`, `resolve_real_path_junction_*`; `file_actions_destination_policy::*` (golden `destination-policy`) | Fails closed on any resolve error |
| RV-S06 | Doing (W2-14: resolver fact) | `PhysicalFileSystem::resolve_real_path` | `resolve_real_path_reparse_loop_fails_and_a_path_beside_it_resolves` | Loop returns Err; the fail-closed policy test lands with W2-08 |
| P-08 / INV-10 / D-08 | Done (W2-16) | `pr_core::diagnostics::file_log` | `diagnostics_file_log::*` (goldens `file-log-format`, `file-log-backup-name`, `file-log-bytes`) | `FileLog`: off by default and then no thread/dir/file, queue 10 000 (oldest dropped), rotate at `>=` max to `<stem>.1<ext>`, CRLF + no BOM. Accepted differences: error text is `Display` + source chain rendered on the caller's thread; `SystemLocalTime` reads UTC (D16: the app injects local time); handle-lifetime tests not ported |
| X-perf-diagnostics | Partial (W2-17; P-16, W-05) | `pr_core::diagnostics::{perf, perf_csv, metrics}` | goldens `perf-csv-escape`, `perf-csv-row`, `perf-csv-file`, `perf-path-id`, `review-metrics`; `diagnostics_perf_csv::*`, `diagnostics_perf::*`, `diagnostics_review_metrics::*` | PhotoReviewPerf event table (ids 1-9, 11-27), PerfCsvListener file format byte-identical (header keeps C# `PHOTOREVIEW_DIAG_*` names; Rust reads `PHOTOREVIEW_RS_PERF_TRACE` / `PHOTOREVIEW_RS_DIAG_*`), ReviewMetrics snapshot JSON. Divergences: `nav_context` is thread-local, `qpcTicks` is a monotonic counter, `ms_since_process_start` counts from first use unless `init_process_start`, `path_id` never panics (empty/NUL path hashed as given), SHA-256 hand-written until `sha2` is a workspace dependency; `path_id` needs W2-00 `get_full_path` |
| X-stj-reader | Partial (W2-01: surrogate-guard and Infinity-sign mutants killed; 2 equivalent mutants remain, nothing left to kill) | `pr_core::settings::app_settings::json` | `settings_app_settings::golden_settings_json` | hand-written System.Text.Json subset |
| P-09 | Partial (W2-18) | `pr_core::updates::checker` | `updates_checker::*`, golden `update-checker` (126 cases) | UpdateChecker behind `HttpTransport`; strict STJ-style JSON reader (depth 64, lone surrogates); `Retry-After` dates match the 12 golden formats only |
| P-14 (localization: shipped + user catalogs) | Done (W2-04) | `pr_l10n::{catalog, localizer, loader}` | `catalog::*`, `localizer::*`, `loader::*`, goldens `l10n-catalog`, `l10n-localizer`, `l10n-loader` | Hand-written System.Text.Json clone (lazy strings, depth 64, comments); user folder is `%LOCALAPPDATA%\PhotoReviewRs\Languages` (R0002), passed in by the App; native-name collation = `cmp_ignore_case` (injectable) |
| S-20 | Done (W2-04, `AUTO_CODE`/resolution) | `pr_l10n::loader::{AUTO_CODE, LanguageLoader::resolve}` | `loader::resolve_*`, `loader::load_auto_*`, golden `l10n-loader` | `UiCulture` carries `Name` + `TwoLetterISOLanguageName`; the platform layer builds it from `GetLocaleInfoEx` |
| D-15 | Done (W2-04, read side) | `pr_l10n::loader::LanguageLoader` | `loader::*` (dotted names skipped, 1 MiB limit with and without stat, BOM, locked file) | `TranslationExport` (`<code>.todo.json`) is App wave |
| I18N translator modes | Done (W2-04, plumbing) | `pr_l10n::localizer::{create_pseudo, create_show_keys}` | `localizer::create_pseudo_*`, `localizer::create_show_keys_*`, golden `l10n-localizer` | `--i18n-pseudo` / `--i18n-keys` CLI args are App wave |
| X-l10n-keygen | Done (W2-04) | `crates/pr-l10n/build.rs` + `build/keygen.rs` (C# `TrGenerator`, `FlatJsonReader`, `LocDiagnostics`) | `key_generator::*`, `flat_json::*`, `generated_tr::*`, goldens `l10n-generator`, `l10n-flat-json` | PRLOC001..007 are build errors; Rust naming `keys::SCREAMING_SNAKE` / `tr::snake_case`, keywords `r#`-escaped; `self`/`crate`/`super` keys are PRLOC003 |
| I-17 | Done (W2-06) | `pr_core::catalog::capture_groups` | `catalog_capture_groups::*` (golden `capture-groups`, 39 cases) | JPEG+RAW pairing and XMP sidecar choice; ReviewCatalog-based tests belong to W2-10 |
| I-17 / INV-3 / INV-5 / INV-7 / INV-9 (catalog) | Done (W2-10) | `pr_core::catalog::review_catalog::ReviewCatalog` | `catalog_review_catalog::*` (golden `review-catalog`, 101 cases / 1877 steps) | Ordered entries + current position, lazy path index with incremental remove, StructuralVersion, Restore/RestoreMembers/ReformGroup, ReplaceOrder, Explorer-order collapse |
| AR16 (RemovePaths) | Done (W2-10) | `ReviewCatalog::remove_paths` | `remove_paths_*`, golden `review-catalog` | Unreadable removal, capture degrade in place, current advances like Remove |
| RV-C09 | Done (W2-10) | `ReviewCatalog::remove` | `remove_entry_before_current_keeps_same_current_path`, `remove_entry_after_current_keeps_same_current_path` | Remove keeps the same photo current |
| CORE-01 | Done (W2-10) | `ReviewCatalog::replace_order` | `replace_order_rejects_duplicates_even_when_count_and_members_match`, `replace_order_not_a_permutation_returns_false_and_keeps_order` | ReplaceOrder is a true permutation |
| ADR0005-catalog-guard | Done (W2-10) | `ReviewCatalog::bind_to_current_thread` | `bound_mutated_from_other_thread_throws_only_in_debug` | Owner-thread guard, debug builds only; message uses Rust thread ids |
| C-25 | Done (W2-07) | `pr_core::catalog::compare::{find, build_index}` | `catalog_compare::*` (golden `compare-pair`, `numbered-pattern`) | Hand-written .NET regex matchers (`\d` = Unicode Nd, `$` before final `
`); Find/BuildIndex trailing-space-extension quirk kept |
| C-06 | Done (W2-07, decision part) | `pr_core::file_actions::duplicates::find` | `file_actions_duplicates::*` (golden `duplicates`) | Size groups, naming rule, survivor rule; hash is a caller closure (`HashFailure`); recycling is the App's (W8) |
| P-01 / INV-7 | Done (W2-09, validation part) | `pr_core::catalog::snapshot_validator` | `catalog_snapshot_validator::*` (golden `snapshot-validator`) | Explorer snapshot validation; W2-10 ports the two ReviewCatalog tests |
| X-http-transport | Todo | `pr_core::updates::checker::HttpTransport` | - | Trait only; the real transport (proxy, trust store, 50 redirects, whole-exchange timeout) is a later platform unit (contract W2-18 open question 1) |
| K-39 | Done (W2-11) | `pr_core::catalog::drag_drop::parse` | `catalog_drag_drop::*` (golden `drag-drop`, 35 cases) | Drop: the first existing folder wins; otherwise the first supported existing image opens its folder at that image. Warning texts are mapped by the App layer (`DragDropWarning::catalog_key`). |
| Q-FMT-WEBP-HEIC (drag and drop) | Done (W2-11, drop part) | `pr_core::catalog::drag_drop::parse` | `catalog_drag_drop::parse_heic_file_with_webp_heic_switch_on_is_valid`, `..._switch_off_is_invalid` | A dropped .heic or .webp opens only while the WebP/HEIC switch is on. |
| F-23 | Partial (W2-13) | `pr_core::session::writer::SessionWriter` | `session_writer::*` (29 C# twins, golden `session-writer-sequence`) | 500 ms debounce on one short-lived timer thread per window (injectable `DebounceTimer`), newest state per folder (per-folder versions; `C:\a\`/`C:\A` one key), bounded flush (2 s budget for lock + save on a `PhotoReview.SessionSave` thread), dispose waits at most 2 s in total, `Drop` = dispose. No twin: `Update_NullSkipped_DoesNotThrow`, negative-debounce rows (`Vec`/`Duration` cannot be null/negative) |
| F-22 | Doing (W2-13: write part done) | `SessionWriter` -> `SessionStore::save` | `session_writer::*` | Debounced writes of the session state; the store itself (format, load, sweep) is W2-12 |
| F-22 (store) | Done (W2-12: M20 killed by `depth_guard_ignores_brackets_after_an_escaped_quote_inside_a_string`) | `pr_core::session::store::SessionStore::{load, save, sweep}` | `session_store::*` (goldens `session-json-read`, `session-json-write`, `session-key`) | Session file format, canonical folder key, startup sweep |
| F-12 / F-13 / F-19 / INV-6 (journal) | Todo (W3-01 contract + stub; golden-first) | `pr_core::file_actions::{journal, transaction}` | `file_actions_journal::*`, `file_actions_transaction::*` (goldens `journal-stream-lines`, `journal-ops`, `journal-committed-moves`, `journal-compact`, `journal-transaction`) | Open: flush seam for PowerLossSafe (contract W3-01 OQ1), UnrepresentablePath location (OQ2); `Prepared` on a pool thread has no Rust twin (R0003) |
| F-16 | Partial (W3-02) | `pr_core::file_actions::{compaction::build, journal_rules}`, `pr_core::io::journal_compaction_files` | goldens `journal-compaction-plan`, `journal-tail-committed-move`; `file_actions_compaction::*`, `file_actions_journal_rules::*`, `io_journal_compaction_files::*` (native) | Plan, line rules and the Windows primitives ported (kept lines are the original bytes, D-03; FA-01 rule shared with the readers, F-11; temp file creation/deletion only, D-14 naming/sweep is W3-01). `TryCompact` is W3-01; the observational-equivalence tests (INV-6) go green once W3-01 merges (D24). `io::physical` helpers are still private, so this file carries small copies of `full_path`/extended-prefix/`DeleteFileW` (contract open question 1, alternative) |
| F-14 | Todo (W3-03 contract + stub) | `pr_core::file_actions::{reconcile, startup_recovery}` | `journal_reconcile::*`, `journal_startup_recovery::*` (goldens `journal-reconcile`, `journal-startup-recovery`) | Move/Copy need size AND write time within 2 s; group Recycle-undo needs exact identity; older-build repair lives here behind `ReconcileJournal` |
| F-01..F-06 / INV-4 / INV-6 (service) | Todo (W3-04 contract + stub) | `pr_core::file_actions::service::FileActionService` | `file_actions_service::*` (golden `file-action-scenarios`, 111 cases) | 176 C# tests assigned; fault/crash matrices go to a later integration unit |
| F-10 / INV-6 (undo) | Todo (W3-05 contract + stub) | `pr_core::file_actions::undo::UndoService` | `file_actions_undo::*` (golden `undo-scenarios`, 100 cases) | P-13: `RecycleBin::try_restore` returns `io::Result<bool>`, so the shell fault text matches C# (no Different row) |
| F-18 | Todo (W3-06 contract + stub) | `pr_core::file_actions::recovery_retry::RecoveryRetryService` | `recovery_retry::*` (golden `recovery-retry`, 106 cases) | Needs the stat seam shared with W3-07; two off-thread tests not portable (R0003) |
| F-17 | Todo (W3-07 contract + stub) | `pr_core::file_actions::recovery_check::RecoveryFileCheck` | `file_actions_recovery_check::*` (goldens `recovery-check` 735, `recovery-decide` 720) | P1 seam `FileSystem::try_get_file_stat` landed (P-12): unreadable verdict portable |
| F-21 / R01 (partial cleanup) | Todo (W3-08 contract + stub) | `pr_core::file_actions::partial_cleanup::{remove_if_partial, is_same_file}` | `file_actions_partial_cleanup::*` (golden `partial-cleanup`, 26 cases) | Swallows Err/None only; a panicking FileSystem propagates |
| K-32, C-28, S-64 (normalize part), S-* range rows | Done (W2-02) | `pr_core::settings::normalizer::{normalize, disable_conflicting_optional_shortcuts, build_repair_text, SettingsValidator, SettingsParseResult}` | `settings_normalizer::*` (goldens `settings-normalize` 294, `settings-validate` 84, `settings-repair-text` 22) | Repair report order pinned by golden; `SettingsStore::parse_text` pipeline is W2-03 |

---


Every row was checked against the source code, not only the docs. Paths are relative to `D:\MyProject\PhotoReview\`. "SM" means `src/PhotoReview.Core/Settings/ShortcutMappings.cs`. "MW" means `src/PhotoReview.App/MainWindow.xaml(.cs)`.

## 1. Keyboard shortcuts and mouse gestures

| ID | Item | C# source | Notes |
|---|---|---|---|
| K-01 | Next image: `Right` | SM.Next; `App/Input/ShortcutRouter.cs` (step 13) | Mandatory. On a zoomed image an arrow key pans first (K-27). Waits for a pending Explorer order before moving. |
| K-02 | Previous image: `Left` | SM.Previous | Mandatory. Clamped at index 0, no wrap-around. |
| K-03 | First image: `Home` | SM.FirstImage; `MainViewModel.FirstAsync` | Mandatory. Does nothing when no image is open. |
| K-04 | Last image: `End` | SM.LastImage; `MainViewModel.LastAsync` | Optional (empty means disabled). Does nothing when no image is open. |
| K-05 | Next sibling folder: `PageDown` | SM.NextFolder; `SiblingFolderNavigator` | Mandatory. Opens the next sibling folder that contains a supported image. Works with no image open. |
| K-06 | Previous sibling folder: `PageUp` | SM.PreviousFolder | Mandatory. Same search, direction -1. |
| K-07 | Undo: `Ctrl+Z` | SM.Undo="Z" plus Ctrl required in the router | Mandatory. Undo is exclusive on the gate (F-11). |
| K-08 | Recycle: `Delete` | SM.SendToRecycleBin | Mandatory. Fires only with no modifier (Ctrl/Shift+Delete do nothing). Confirmation rules are in F-03/F-04. |
| K-09 | Action profiles: `Enter`, `F3`, `F4`, `F5` | `ReviewAction.Defaults()`; router step 8 | Enter moves to `Group-2`, F3 to `Group-3`, F4 to `Group-4`, F5 copies to `Backup`. No modifier allowed. Checked before Recycle/Skip in router order. |
| K-10 | Skip: `Space` | SM.Skip; `MainViewModel.SkipAsync` | Adds the current path to `Session.Skipped` (no duplicates), persists the session, advances to the next image. |
| K-11 | Compare toggle: `C` | SM.Compare; `MainViewModel.ToggleCompare` | Ignored unless the current image has a compare pair or Compare is already open (pairing rule in C-25). |
| K-12 | Toggle Fit: `F` | SM.ToggleFit; `FitViewController.ApplyFitAsync` | Mandatory. Converges with a re-pass when scrollbars appear (T89). |
| K-13 | Zoom in / out: `Add` / `Subtract` | SM.ZoomIn/ZoomOut; `PointerInputController.ZoomIn/OutAsync` | Step = `KeyboardZoomStepPercent`. Anchor follows `KeyboardZoomAnchor`. Zoom clamped 0.10..8.0; a step stays within 0.25..4.0 (`ViewerState`). |
| K-14 | Zoom 100 %: `D1` | SM.ZoomActualSize | Optional. 100 % means 1 source pixel per device pixel (ADR 0008). |
| K-15 | Click-zoom toggle: `D2` | SM.ClickZoom; `ToggleClickZoomAsync` | Optional. Toggles Fit and `ClickZoomPercent`. Works even when `ClickToZoomEnabled` is off. |
| K-16 | Custom zoom dialog: `D3` | SM.CustomZoom; `OpenClickZoomCustomDialogAsync` | Optional. Opens W-09. |
| K-17 | Fit width: `W` | SM.FitWidth; `FitWidthAsync` | Optional. Vertical anchor always follows `FitWidthAnchor`. Re-applied once if a scrollbar appears. |
| K-18 | Fit height: `H` | SM.FitHeight; `FitHeightAsync` | Optional. Always centred at image point (0.5, 0.5). |
| K-19 | Toggle keep zoom across images: `K` | SM.ToggleKeepZoom; `MW.ToggleKeepZoomAcrossImages` | Optional. Flips `KeepZoomAcrossImages` and saves config immediately. Works with no image. |
| K-20 | Toggle info overlay: `I` | SM.ToggleInfoOverlay; `MainViewModel.ToggleInfoOverlay` | Optional. Flips `ShowInfoOverlay` and saves immediately. Works with no image. |
| K-21 | Fullscreen: `F11` | SM.Fullscreen; `MW.ApplyFullscreenState` | Matches `Key.System` too. Borderless Maximized; a Maximized window goes Normal first so the taskbar is covered. Restores the earlier state on exit. |
| K-22 | `Escape` | Router step 2; `MW.Window_KeyDown` | Priority: (1) close the Tools popup, (2) cancel a running duplicate hash, (3) leave fullscreen, (4) close the window. Not rebindable. |
| K-23 | Move to…: `M` (Shift+M forces the picker) | SM.MoveToFolder; `FileActionController.MoveOrCopyToFolderAsync` | Optional. Ctrl+M does nothing. Reuses the last folder when `MoveCopyReuseLastFolder` is on and that folder still exists. |
| K-24 | Copy to…: `Y` (Shift+Y forces the picker) | SM.CopyToFolder | Optional. Same rules as K-23, using `LastCopyToFolder`. |
| K-25 | Open folder: `Ctrl+O` | SM.OpenFolder="O" plus Ctrl required | Optional. Works with no image. Folder picker starts at the current folder. |
| K-26 | Switch JPEG/RAW member | SM.ToggleCaptureMember (default "") | Unbound by default. Only when the current entry is a capture group and Compare is closed. |
| K-27 | Arrow keys on a zoomed image pan | `PointerInputController.TryPanByArrow`; `KeyboardPan` (in KineticPan.cs) | Step = `ArrowPanStepPercent` % of the viewport. Never changes image unless `ArrowKeyNavigatesAtZoomEdge` is on (then a fresh press at the edge navigates and auto-repeat is swallowed). With `KineticPanEnabled`, adds a glide impulse capped at MaxVelocity 8. Skipped when a modifier is held or Compare is open. |
| K-28 | Auto-repeat is ignored for some commands | `ReviewCommandTypeExtensions.IgnoresAutoRepeat` | Recycle, RunAction, Undo, Move/Copy-to, ToggleInfoOverlay, Fullscreen, ToggleCompare, ClickZoom, ToggleKeepZoom, OpenFolder, CustomZoom, ToggleCaptureMember. Navigation and zoom keys repeat. |
| K-29 | Space/Enter are not taken from a focused button or compare pane | `MW.OwnsActivationKeys`, `ReturnFocusAfterButtonClick` | After a toolbar click, focus returns to the window so Space/Enter work as shortcuts again. |
| K-30 | Compare pane select | `MW.CompareLeft/Right_Click`, `_KeyDown` | Click, Enter or Space on a pane selects it. The selected path becomes the target of file actions. |
| K-31 | Router priority order | `ShortcutRouter.TryResolve` | Fullscreen, Esc, NextFolder/PreviousFolder, First/Last, Ctrl+Undo, ToggleInfoOverlay, ToggleKeepZoom, Ctrl+OpenFolder, then (only with an image) Compare, CaptureMember, Actions, Recycle, Skip, ToggleFit, ZoomIn/Out/100 %, ClickZoom, FitW/FitH, CustomZoom, Next/Prev, Move-to/Copy-to. |
| K-32 | Shortcut validation | `SettingsValidator`; `ShortcutKeyCanonical`; `App/Services/ShortcutKeyName.cs` | Mandatory keys cannot be blank. All bindings (shortcuts plus actions) must be unique after canonicalisation (Return=Enter, Prior=PageUp, Next=PageDown, Oem aliases, ...). Numbers, comma lists, Escape/Tab and modifier keys are refused. |
| K-33 | Mouse wheel | `WheelGestureInterpreter` (`App/Input/MouseGestures.cs`); `ViewerState.WheelZoom` | `MouseWheelAction=Zoom`: every event is one zoom step anchored at the cursor. `Navigate`: one image per 120-delta notch; partial deltas accumulate and reset on direction change; at most one image per event (excess dropped). Ctrl+wheel always zooms. |
| K-34 | Left click on image: click-to-zoom | `PointerGestures.ClassifyRelease`/`DecideClickZoom` | Only when `ClickToZoomEnabled`. Fires on mouse-up with no drag. Fit or another zoom goes to `ClickZoomPercent` at the cursor; at that zoom (±0.001) it goes back to Fit. A press that only stopped a glide does not count as a click. |
| K-35 | Double-click on image: Fit | `PointerGestures.IsFitDoubleClick` (ClickCount ≥ 2) | No click delay. The second press always applies Fit. |
| K-36 | Left-drag pan plus kinetic glide | `PointerInputController.OnImagePress/Move/Release`; `KineticScroller` | Pans only when zoomed and the image overflows. Uses the system drag threshold. Releasing a drag starts a glide when `KineticPanEnabled` (TimeConstantMs 325, release factor 0.65, stops below velocity 0.02). Smoothing follows `KineticGlideSmoothing` (vblank-timed, P-11). |
| K-37 | Stop a glide | `PointerInputController.StopKinetic` | Any mouse press, non-arrow key, zoom change, navigation, window deactivation or window close. |
| K-38 | Right click: context menu | MW `ImageContextMenu` | Contents in section 2 (C-07..C-16). |
| K-39 | Drag and drop | `MW.Window_Drop`; `DragDropInputService.Parse` | FileDrop only. Opens the first item: a file opens its folder at that file, a folder opens the folder. Copy effect shown. |
| K-40 | Mouse activity: toolbar and overlay | `ToolbarAutoHidePolicy` (24 px hot zone), `InfoOverlayAutoHidePolicy` | Mouse move, wheel, key, navigation or activation counts as activity. Fade in 150 ms, fade out 200 ms. Faded elements are click-through. |
| K-41 | Settings window: `Ctrl+Tab` / `Ctrl+Shift+Tab` | `SettingsWindow.xaml.cs` | Cycles pages whatever control has focus. |
| K-42 | Shortcut capture boxes | `SettingsWindow`, `ActionProfilesWindow` | A key press is captured as its canonical key name. Clear buttons exist for optional shortcuts only. |

## 2. Commands and menu items

| ID | Item | C# source | Notes |
|---|---|---|---|
| C-01 | Toolbar: 📂 Open folder | `MainWindow.xaml` ToolbarPanel; `OpenFolder_Click` | Same as K-25. |
| C-02 | Toolbar: ⚙ Settings | `Settings_Click`; `MainViewModel.ShowSettings` | Opens W-02 modal. After Save it may reload or re-present (C-20). |
| C-03 | Toolbar: Fit | `FitImage_Click` | Same as K-12. |
| C-04 | Toolbar: ⋮ Tools popup | `ToolsButton` (ToggleButton) | Contains Recovery, Diagnostics, Benchmark, Clear cache, Remove numbered duplicates, Remove original duplicates. Keeps the toolbar visible while open. |
| C-05 | Tools: Clear cache | `DuplicateCleanupController.ClearCacheAsync` | Yes/No confirmation. Clears preview RAM and disk, thumbnail and source-bytes caches; epoch bump (INV-2). |
| C-06 | Tools: Remove numbered / original duplicates | `DuplicateCleanupController.RemoveDuplicatesAsync(bool)`; `DuplicateFinder` | Size groups, then SHA-256. Numbered pattern is `" \(\d+\)$"` on the stem. Esc cancels hashing. Batch is refused if any file is on a drive without a Recycle Bin (never permanent). Confirmed through W-07, then recycled; folder reloaded. Exclusive on the gate. |
| C-07 | Context menu: Undo | `UndoLastAction_Click` | Always shown. |
| C-08 | Context menu: Move to Recycle Bin | `RecycleMenuItem` | Shown only when `ShowRecycleMenuItem`. Shows the configured shortcut as gesture text. |
| C-09 | Context menu: Fit | `FitMenuItem` | Part of the zoom group, shown only when `ShowZoomMenuItems` (default hidden). Gesture text = ToggleFit key. |
| C-10 | Context menu: Zoom to N % | `ZoomToLevelMenuItem`; `SetClickZoomLevelAsync` | Zoom group. N = `ClickZoomPercent`. Anchored at the viewport centre. Does not change the setting. |
| C-11 | Context menu: Zoom submenu | `MW.BuildZoomMenu`, `ZoomMenu_SubmenuOpened` | Fit width (W), Fit height (H), separator, presets 50/70/100/150/200/300/400 % (checked = current ClickZoomPercent), Custom…, separator, "Also set as click zoom level" (checkable), "Set current zoom as click level" (disabled in Fit; rounds and clamps). Texts are refreshed on every open. |
| C-12 | Preset or Custom selection | `ApplyZoomMenuSelectionAsync` | When `SetZoomAlsoSetsClickLevel` is on: saves `ClickZoomPercent` and zooms. When off: zooms once only. |
| C-13 | Context menu: Open folder / Next folder / Previous folder | `OpenFolderMenuItem` etc. | Folder group with its own separator, hidden together when `ShowFolderMenuItems` is false. Their shortcuts still work. |
| C-14 | Context menu: Open in external editor | `MainViewModel.OpenInExternalEditor` | Shown only when `ExternalEditorPath` is set and an image is open. Runs the exe with the path as one argument (no shell execute). Target = compare selection, else presented path, else current. Error dialog if the file is missing or the launch fails. |
| C-15 | Context menu: Settings | `Settings_Click` | Always shown, last. |
| C-16 | Context menu separator rules | `ImageContextMenu_Opened` | Each group owns the separator above it, so two separators never appear in a row. Visibility is re-read on every open. |
| C-17 | Status panel: "View list" (skipped files) | `SkippedFiles_Click` | Shown when a scan skipped unreadable files. Opens W-08. |
| C-18 | Open a path from the command line | `App.StartupCoreAsync` | First arg that is an existing file, else an existing folder. Explorer order is prefetched (3 s budget) in parallel. |
| C-19 | Forwarded open from a second launch | `App.OpenForwarded`; `ForwardedOpenCoalescer` (750 ms) | Restores a minimised window (SC_RESTORE), activates it, opens the path. An empty path list just brings the window to front. |
| C-20 | Settings Save side effects | `MainViewModel.ShowSettings` | Reload the folder at the current file if RawSupport, RawPairMode (RAW on) or ImageSortMode changed. Re-present the current image if LoadingMode, DecoderBackend or RawFullDecode changed. A backend change clears the RAM cache plus the disk cache (on a pool thread) and preloaded keys. |
| C-21 | Run an action profile | `FileActionController.RunActionAsync` | Profile `Confirm` asks Yes/No, unless a permanent-delete or group prompt will ask anyway. Empty destination gives a status message. Operation can be Move, Copy or Recycle. |
| C-22 | Move to… / Copy to… | `FileActionController.MoveOrCopyToFolderAsync`; `ActionDestinationPolicy.ValidatePickedFolder` | Picked folder must be absolute, existing and not the photo folder. Remembers `LastMoveToFolder` / `LastCopyToFolder` on success. Status "Moved to…/Copied to…". Rust: `file_actions::destination_policy` (W2-08, golden `destination-policy`). |
| C-23 | Undo command | `MainViewModel.UndoAsync` | If the restored file lies outside the current folder (a Recycle undo, or a Move made in a previous folder), opens that folder at the file, unless the user opened another folder meanwhile (epoch check). |
| C-24 | Switch capture member | `ToggleCaptureGroupMemberAsync` | Presents the other member of the same catalog entry; the badge updates. |
| C-25 | Compare mode | `CompareViewModel`; `ComparePairService` | Pairs `X.ext` with `X (n).ext` in the same folder and extension (lowest n). Shows both images; the size and hash checks follow `CompareSizeEnabled` / `CompareHashEnabled`. Clicking a pane makes it the action target. A capture group in Compare shows both members. |
| C-26 | Window close while a file action runs | `MW.OnClosing`; `MainViewModel.DeferCloseForFileAction` | Close is deferred until the gate is released. A duplicate check that is only hashing is cancelled. |
| C-27 | Startup recovery prompt | `App.RecoverJournalAsync` | If reconcile marked entries Failed, asks Yes/No to open Recovery (W-04). |
| C-28 | Settings repair notice | `App.StartupCoreAsync`; `SettingsLoadRepairText` | One information dialog listing the settings that were reset or disabled at load. |

## 3. Settings (`src/PhotoReview.Core/Settings/AppSettings.cs`; repairs in `SettingsNormalizer.cs`)

Shared rules (not separate rows):
- Every property missing from the file keeps its default.
- "Clamp" means the value is clamped into the range and its name is added to the repair list.
- An enum marked **(S)** uses `SettingsEnumConverter`: an unparsable value is reset to its default and reported.
- Any other enum uses the type-level `LenientEnumConverter`: an unknown value silently becomes enum value 0, and a numeric value is accepted if it is defined.

| ID | Item | C# source | Notes |
|---|---|---|---|
| S-01 | ConfigVersion = 3 | AppSettings | No key in the file means version 1 (legacy). Below v2: actions defaulted. Below v3: `UiLanguage="vi"` unless the file declares it. Always saved as 3. A file newer than 3 is never rewritten with repairs. |
| S-02 | InitialViewMode = Fit | InitialViewMode enum | Values: Fit, Percent100 ("100%"), Percent200 ("200%"), Percent400 ("400%", migrated to Percent200), FitWidth ("fit-width"), FitHeight, ClickZoomLevel. Undefined resets to Fit. |
| S-03 | LoadingMode = Preview (S) | LoadingMode | Values: Fast, Preview, Original. |
| S-04 | LoggingEnabled = false | AppSettings | INV-10. When on, the title gets a logging suffix. |
| S-05 | ImageSortMode = Default (S) | ImageSortMode | Values: Name(0, alias PortraitFirst), SizeDescending(1, alias Size), SizeAscending, Default, NameAscending, NameDescending. Default, NameAscending and NameDescending never use the Explorer order. |
| S-06 | CompareHashEnabled = true | AppSettings | Compare shows hash equality. |
| S-07 | CompareSizeEnabled = true | AppSettings | Compare shows size equality. |
| S-08 | ScalingQuality = HighQuality (S) | ScalingQuality | Values: Linear, HighQuality. Render-only; not part of the cache key. |
| S-09 | DecoderBackend = WicDirect (S) | DecoderBackend | Values: Wpf, WicDirect, TurboJpeg. The internal-only `LibRaw` value is reset to WicDirect. |
| S-10 | ImageCacheCapacityBytes = 16 GiB | PerformanceOptions | Must be > 0, else reset. Used only when physical RAM is unknown. |
| S-11 | ImageCacheRamPercent = 50 | PerformanceOptions | Clamped 1..90. A runtime minimum `RamBudgetPolicy.MinimumCachePercent` depends on the preload window. Applies after restart. |
| S-12 | MemoryReserveBytes = 2 GiB | PerformanceOptions | Negative resets. Persisted only; preload uses `PreloadOptions` default 2 GiB. |
| S-13 | PreloadWorkerCount = 8 | PerformanceOptions | ≤ 0 resets to 8; > 64 clamps to 64. Applies after restart. |
| S-14 | PreloadMemoryLoadLimit = 0.90 | PerformanceOptions | Must be in (0, 1], else reset. |
| S-15 | PreviewDiskCacheCapacityBytes = 4 GiB | PerformanceOptions | Negative resets. Persisted only; `PreviewImageService` uses its own 4 GiB default. |
| S-16 | UseSourceBytesCache = false | PerformanceOptions | Read once at composition; restart needed. |
| S-17 | SourceBytesCapacityBytes = 16 GiB | PerformanceOptions | ≤ 0 resets. Further clamped by `RamBudgetPolicy` (20 % of RAM and the RAM-percent share). |
| S-18 | Actions = 4 defaults | ReviewAction | Each action has {Name, Shortcut, Operation (Move/Copy/Recycle, alias "Delete"), Destination, Confirm}. Null entries removed; null strings become ""; invalid Operation becomes Move. |
| S-19 | Shortcuts | ShortcutMappings | Mandatory defaults: Next Right, Previous Left, SendToRecycleBin Delete, Compare C, NextFolder PageDown, PreviousFolder PageUp, FirstImage Home, ZoomIn Add, ZoomOut Subtract, ToggleFit F, Skip Space, Undo Z, Fullscreen F11, MoveToFolder2 Enter (legacy alias, ignored). Optional defaults: LastImage End, ZoomActualSize D1, ToggleInfoOverlay I, ClickZoom D2, FitWidth W, FitHeight H, ToggleKeepZoom K, OpenFolder O, CustomZoom D3, MoveToFolder M, CopyToFolder Y, ToggleCaptureMember "". A blank mandatory key resets to its default; an action holding that key loses its shortcut (reported). An optional key that conflicts at load is disabled (reported). Saved in canonical form. |
| S-20 | UiLanguage = "auto" | LanguageLoader.AutoCode | Code such as `en`/`vi`, or `auto` (follow Windows). Blank becomes auto. Live switch. |
| S-21 | JournalDurability = Fast | JournalDurability | Values: Fast, PowerLossSafe. Read live on every journal write. |
| S-22 | AllowPermanentDeleteWithoutRecycleBin = false | AppSettings | When on, Recycle on a removable/network drive deletes permanently after an explicit confirmation. |
| S-23 | LastMoveToFolder = null | AppSettings | Starting folder of the Move-to picker. |
| S-24 | LastCopyToFolder = null | AppSettings | Starting folder of the Copy-to picker. |
| S-25 | MoveCopyReuseLastFolder = false | AppSettings | Skip the picker when the last folder still exists (Shift forces the picker). |
| S-26 | InstanceMode = SingleWindow | InstanceMode | Values: SingleWindow, PerFolder. Read before the instance lock is taken, so it applies at the next start. |
| S-27 | ShowInfoOverlay = true | AppSettings | Master switch for the overlays; toggled by `I`. |
| S-28 | ShowFileInfo = true | AppSettings | Bottom-left status block. |
| S-29 | ShowFolderInfo = false | AppSettings | Bottom-right folder and sibling block. Siblings are not computed when hidden. |
| S-30 | ShowExifInfo = false | AppSettings | Photo info line; EXIF comes from the decode, never an extra read. |
| S-31 | ExifInfoFields = Default | ExifInfoFields [Flags] | Flags: FileName 1, DateTaken 2, Dimensions 4, Camera 8, Lens 16, Iso 32, FocalLength 64, Aperture 128, ShutterSpeed 256, ModifiedDate 512. Default = All minus FileName, Dimensions, ModifiedDate. Saved as "A, B" (or "None"); numbers accepted (masked); garbage gives Default. |
| S-32 | MouseWheelAction = Zoom | MouseWheelAction | Values: Zoom, Navigate. |
| S-33 | ClickToZoomEnabled = false | AppSettings | |
| S-34 | ClickZoomPercent = 100 | AppSettings | Clamped 10..800. |
| S-35 | KineticPanEnabled = false | AppSettings | |
| S-36 | PreloadForwardCount = 32 | PerformanceOptions | Clamped 1..500. Applies after restart. |
| S-37 | PreloadBackwardCount = 8 | PerformanceOptions | Clamped 0..500. Applies after restart. |
| S-38 | ToolbarAutoHide = false | AppSettings | |
| S-39 | ToolbarAutoHideDelayMs = 1500 | AppSettings | Clamped 0..10000. |
| S-40 | InfoOverlayFontSize = 12 | AppSettings | Clamped 8..24; NaN or infinity resets to 12. The EXIF line is 1 pt smaller. |
| S-41 | TitleBarFields = FolderName | TitleBarFields [Flags] | Flags: FolderName 1, FolderPath 2, IndexCount 4, FileName 8, FileSize 16, Dimensions 32, ModifiedDate 64, DateTaken 128, Camera 256, Lens 512, Iso 1024, FocalLength 2048, Aperture 4096, ShutterSpeed 8192. Same JSON rules as S-31. An empty result falls back to the folder name. |
| S-42 | KeyboardZoomStepPercent = 10 | AppSettings | Clamped 5..100. |
| S-43 | ConfirmBeforeDelete = false | AppSettings | Yes/No before Recycle (one prompt only, never two). |
| S-44 | ShowZoomIndicator = false | AppSettings | Zoom % HUD at the top right. |
| S-45 | ExternalEditorPath = "" | AppSettings | Empty hides C-14. |
| S-46 | KineticGlideSmoothing = Predict (S) | KineticGlideSmoothing | Values: Off, Predict. |
| S-47 | InfoOverlayAutoHide = false | AppSettings | Overlays never hide while a message is shown, no folder is open, Compare is open or the window is inactive. |
| S-48 | InfoOverlayAutoHideDelayMs = 3000 | AppSettings | Clamped 0..10000. |
| S-49 | ToolbarOpacityPercent = 100 | AppSettings | Clamped 20..100. |
| S-50 | ArrowKeyNavigatesAtZoomEdge = false | AppSettings | K-27. |
| S-51 | ArrowPanStepPercent = 10 | AppSettings | Clamped 1..100. |
| S-52 | KeepZoomAcrossImages = false | AppSettings | When on, an image change keeps the view (no-op on view mode). |
| S-53 | FitWidthAnchor = Centre | FitWidthAnchor | Values: Centre, TopThird. Undefined resets. |
| S-54 | KeyboardZoomAnchor = ViewportCentre (S) | KeyboardZoomAnchor | Values: Pointer(0), ViewportCentre(1). |
| S-55 | ImageTransition = None | ImageTransition | Values: None, Fade. |
| S-56 | ImageTransitionMs = 120 | AppSettings | Clamped 40..400. |
| S-57 | SetZoomAlsoSetsClickLevel = true | AppSettings | C-12. |
| S-58 | ShowFolderMenuItems = true | AppSettings | C-13. |
| S-59 | ShowZoomMenuItems = false | AppSettings | C-09..C-11. |
| S-60 | ShowRecycleMenuItem = false | AppSettings | C-08. |
| S-61 | RawSupportEnabled = true | AppSettings | Lists and decodes the 8 RAW extensions. |
| S-62 | RawFullDecode = Never | RawFullDecode | Values: Never, OnZoom. |
| S-63 | RawPairMode = Separate | RawPairMode | Values: Separate, PreferJpeg, PreferRaw. |
| S-64 | Load and repair pipeline | `SettingsStore.Load/ParseText` | Read retries 50/100/200/400 ms on IO or access errors. On JSON error: salvage property by property and back up the original. Then migrate, normalize, disable conflicting optional shortcuts. Repairs are written back once (only for the default file). Comments and trailing commas allowed; numbers may be read from strings. |
| S-65 | Save | `SettingsStore.Persist` | Canonicalizes shortcuts, stamps version 3, writes indented JSON atomically with fsync, raises `Changed`. If the corrupt file could not be backed up, settings are kept in memory only. |

## 4. Windows and dialogs

| ID | Item | C# source | Notes |
|---|---|---|---|
| W-01 | MainWindow | MW | 1200×800, dark theme, AllowDrop. Contains the image ScrollViewer, OutgoingImage (fade layer), ComparePanel, toolbar, InfoOverlayHost (status, EXIF, folder info, zoom indicator, capture badge), context menu. Title = `FolderTitle` with an optional `[label]` prefix. Placement restored and saved (D-11). |
| W-02 | SettingsWindow (modal) | `App/SettingsWindow.xaml(.cs)` | Pages: General, Display, Mouse & zoom, Shortcuts, Files, Performance, Diagnostics (last page remembered for the session). Buttons: Save (validates), Defaults (restores defaults including menu visibility), Import/Export settings (import uses `ParseText`), Edit actions (opens W-03), language combo, reload translations, export translation (`.todo.json`), open languages folder, open log location, browse external editor, check for update, open download page, RAM % slider (min depends on the preload window, recomputed live). |
| W-03 | ActionProfilesWindow (modal) | `App/ActionProfilesWindow.xaml.cs` | List of actions plus fields Name, Shortcut, Operation (Move/Copy/Recycle), Destination, Confirm. Add (next free shortcut, destination "Output"), Remove, Import/Export JSON list (`photoreview-actions.json`), Apply validates shortcuts and destinations. |
| W-04 | RecoveryWindow (modal) | `App/RecoveryWindow.xaml.cs`; `RecoveryPresenter`; `RecoveryPathPanel` | Lists Prepared then Failed journal entries with verdicts (F-21) computed in the background. Buttons: Retry (only for CanRetry, with confirmation, plus an extra confirmation for a CancelledByUser entry), Clear selected and Clear all (Dismiss, confirmation), Re-check. Path panel: Show in Explorer, Copy path. Stays open until a running retry completes. |
| W-05 | DiagnosticsWindow (modal) | `App/DiagnosticsWindow.xaml.cs` | Read-only `ReviewMetrics` snapshot, Explorer snapshot reason (`ExplorerReason`), CrossThreadPresentCount. |
| W-06 | BenchmarkWindow (modeless) | `App/BenchmarkWindow.xaml.cs`; `PhotoReview.Benchmarking` | Profile grid (Select all/none), Browse folder, Run, Quick check, Cancel, Open report, Open result folder. |
| W-07 | BatchReviewWindow (modal) | `App/BatchReviewWindow.xaml.cs` | Lists the duplicates to recycle with sizes (missing files marked). Confirm proceeds. |
| W-08 | SkippedFilesWindow (modal, built in code) | `App/SkippedFilesWindow.cs` | Read-only list "path (reason)" plus Close. |
| W-09 | ClickZoomCustomDialog (modal) | `App/ClickZoomCustomDialog.xaml.cs` | Integer 10..800 with inline error. OK returns the value. |
| W-10 | Folder picker | `WpfFolderPicker`, `WpfDialogService.PickFolder` | Native folder dialog with an initial folder. |
| W-11 | Startup message boxes | `App.xaml.cs` | Startup failed (error, then shutdown code 1); app already running / no response, or folder open in another instance (info); settings repaired (info); startup recovery failed with open-Recovery Yes/No. |
| W-12 | File-action confirmations (Yes/No) | `FileActionController` | Profile Confirm; ConfirmBeforeDelete; permanent delete (single file); group recycle; group permanent delete; Compare-group Move/Recycle (always asked). After any dialog: if the folder changed, the gate is busy or the target is gone, the action is abandoned. |
| W-13 | Other prompts | `DuplicateCleanupController`, `MainViewModel` | Clear cache Yes/No; batch errors / drive without bin (warning); external editor failed or file missing (warning). |
| W-14 | Recovery and Settings sub-prompts | RecoveryWindow, SettingsWindow, ActionProfilesWindow | Dismiss confirm/failed/stale; retry confirm/rejected/stale/result; settings invalid, save failed, open failed, translation problems, export translation failed, import result, open editor failed; actions import/export failed and invalid. |

## 5. File actions and safety

| ID | Item | C# source | Notes |
|---|---|---|---|
| F-01 | Move (single file) | `Core/FileActions/FileActionService.ExecuteAsync` | Steps: gate (TryBegin) → validate destination → resolve folder (relative = under the source folder) → refuse same folder → reparse-point escape check → refuse if destination exists → stat source → create folder → journal Prepared → `IFileSystem.Move` on the pool → verify (source gone and destination size equal, else `VerifySizeChanged`) → Committed. Id = Guid "N". |
| F-02 | Copy | same | Create-new copy (`TryCopyNew` with a creation proof). Verify destination size. A partial copy this call created is deleted only while it is strictly shorter than the source. Copy is never undone and never advances the catalog. |
| F-03 | Recycle | same; `WindowsRecycleBin` | Stat, then `CanRecycle` (fixed local drives only). Without a bin and not allowed: refused before journaling. Bin off or too small (`FitsInRecycleBin`, F-WIN-2): refused. Prepared (with Permanent if applicable), shell recycle, Committed. |
| F-04 | Permanent delete | `WindowsRecycleBin.DeletePermanently` | Needs S-22 plus a confirmation every time. Only for a positively identified Removable, Network, CDRom or Ram volume, or a UNC path. Fixed or unknown volumes are refused. Journal `Permanent:true`. |
| F-05 | Destination policy | `ActionDestinationPolicy.Validate/ValidateNoEscapeViaReparsePoint` | Relative destinations may not contain ".." and must not escape through a symlink or junction (fails closed). Absolute destinations are allowed. Invalid characters, `*` `?`, drive-relative `a:b` and rooted-without-drive `\x` are refused (`DestinationOutsideSource`). Rust: `file_actions::destination_policy` (W2-08, golden `destination-policy`). |
| F-06 | Capture-group actions (JPEG+RAW+XMP) | `FileActionService.ExecuteGroupAsync`; `CaptureGroupActionRequest/Result`; `RecycleBinCapacity` | Every member is preflighted, then one manifest line is Prepared (`GroupId`, `GroupMembers`). On failure: Copy deletes created copies; Move moves members back when provably safe. Fully rolled back means Dismissed; otherwise one Failed Recovery item. Each volume's bin must hold all its members. A missing partner is skipped and reported. |
| F-07 | Advance before I/O | `FileActionController.ExecuteFileActionCoreAsync` | Move/Recycle: stop the generation, cancel preload, remove from the catalog, present the next image without awaiting, then do the I/O. On failure the catalog position is restored. With nothing left: "All images processed". |
| F-08 | Gate and queue | `App/Coordinators/FileActionGate.cs`; `FileActionService.TryBegin` | Move/Copy/Recycle/actions queue in order (`RunQueuedAsync`). Undo and duplicate cleanup are exclusive and do nothing if busy. A queued command is dropped if a different folder was opened since it was submitted (epoch). |
| F-09 | Stale-folder completion | `FileActionController` | If the folder changed during I/O, a successful action is still registered for Undo and a late status ("moved to X, Ctrl+Z to undo"…) is shown; the new catalog is untouched. |
| F-10 | Undo | `Core/FileActions/UndoService.UndoLastAsync` | One level (`_lastUndoAction`), current session only; journal history is not reloaded at start (P03). Move: destination checked by size plus write time within ±2 s (`FileFingerprint`), source must be absent, journaled as a Move with `Undo:true`, original folder recreated. Recycle: refused if the original path is occupied; shell restore; success only if size and write time match exactly. Permanent: "cannot restore". Group variants support partial restore and retry. |
| F-11 | Journal states | `Core/Model/JournalState.cs` | Prepared(0), Committed(1), Failed(2), Dismissed(3). The latest line per Id wins. A reconcile Failed (codes PendingUnconfirmed / SourceStillExistsAfterRecovery) that follows Committed is ignored. |
| F-12 | Journal transaction | `Core/FileActions/JournalTransaction.cs` | Live marker → Prepared → mutation → Committed/Failed (or Dismissed when rolled back untouched). In PowerLossSafe mode Prepared is written on a pool thread. A failed Committed append still reports success with a JournalError. |
| F-13 | Append robustness | `OperationJournal.AppendLines` | 5 attempts with 10 ms × attempt backoff on sharing or lock violations. Fast: plain Flush. PowerLossSafe: WriteThrough plus Flush(true). A torn tail (no final '\n') gets a newline first. Unpaired surrogates are refused (`UnrepresentablePath`). |
| F-14 | Startup reconcile | `JournalStartupRecovery.RunAsync`; `OperationJournal.ReconcilePendingOperations` | Only Prepared entries older than startup, skipping live markers. Move: source gone and destination size equal → Committed, else Failed (PendingUnconfirmed). Copy: destination size equal → Committed. Recycle: source gone → Committed, else Failed (SourceStillExistsAfterRecovery). Groups: all members. Repairs group lines settled by an older build. Then compaction. The outcome is appended only if the entry is still Prepared. |
| F-15 | Live operation markers | `WindowsLiveOperationRegistry` | One named event per running operation, shared by every process of the user session. Reconcile skips live ids. |
| F-16 | Compaction | `OperationJournal.TryCompact`; `JournalCompactionPlan` | Only when the file is ≥ 1 MiB and at least 25 % can be dropped. Snapshot → temp `*.compact.tmp` fsynced → open the journal denying writers → copy lines appended since → atomic replace. Stale temp files older than 1 h are removed. |
| F-17 | Recovery verdicts | `Core/FileActions/RecoveryFileCheck.cs` (docs/architecture.md table) | Move/Copy Prepared or Failed, by source S and destination D: S exists & D missing → CanRetry; S changed & D missing → SourceChanged; S missing & D exists → AlreadyDone; S missing & D changed → DestinationChanged; both missing → Lost; both exist → Copy AlreadyDone, Move Conflict; unreadable → Unknown. Committed: D exists → AlreadyDone. Recycle: RecycleUnverifiable, PermanentlyDeleted, PartiallyPermanentlyDeleted, NotRecycled. |
| F-18 | Recovery retry | `Core/FileActions/RecoveryRetryService.cs` | Takes the app gate. Refuses a CancelledByUser entry without explicit confirmation. Appends only if nothing else changed the Id since the snapshot (`AppendIfUnchangedSince`); otherwise "Superseded". Same Id is reused. Create-new copy. Group recycle retry included. |
| F-19 | Dismiss | `OperationJournal.Dismiss` | Appends Dismissed (Error and ErrorCode cleared) only when the latest line equals the snapshot; otherwise the entry is reported as skipped. Files are never touched. |
| F-20 | Error codes | `Core/FileActions/JournalErrors.cs` | SourceStillExistsAfterRecovery, PendingUnconfirmed, VerifySizeChanged, RetryVerifyFailed, DestinationOutsideSource, MoveSourceNotRemoved, CancelledByUser, UnrepresentablePath. `Error` holds invariant English text; the UI localizes by code. |
| F-21 | Partial copy policy (COPY-PARTIAL-01) | docs/architecture.md | A single Copy failing mid-way after `File.Copy` leaves the partial file (Failed / Conflict); the user deletes it. Only the create-new path deletes a provably own, shorter file. |
| F-22 | Session state | `Core/Session/SessionStore.cs` | Fields {Folder, CurrentPath, Skipped[], UpdatedUtc}. Atomic write without fsync. Empty or corrupt file means no session (logged). `*.tmp` files older than 1 day swept at start. |
| F-23 | Session writer | `Core/Session/SessionWriter.cs` | 500 ms debounce, newest state per folder. Flush on folder change. Dispose waits at most 2 s. |
| F-24 | Unreadable files | `FolderLoadCoordinator` (probe ×8) | Removed after the first frame and after the Explorer order settled. Reported as "Skipped N" (W-08). If the current image is removed, behaves like Delete. |

## 6. Imaging

| ID | Item | C# source | Notes |
|---|---|---|---|
| I-01 | Standard formats | `Core/Catalog/ImageFileTypes.cs` | .jpg .jpeg .png .bmp .gif .tif .tiff. Only .jpg/.jpeg can be the JPEG side of a capture group. |
| I-02 | Decoder backends | `ImageDecoderFactory`; `WpfBitmapImageDecoder`; `Decoding/Wic/WicDirectDecoder`; `Imaging.TurboJpeg/TurboJpegDecoder` | Wpf, WicDirect (default), TurboJpeg (registered only if `TurboJpegAvailability.Probe()` succeeds). Non-WPF backends are wrapped by `FallbackImageDecoder`. |
| I-03 | Fallback to WPF | `FallbackImageDecoder.IsFallbackable` | Falls back on NotSupported, InvalidCast, Overflow, FileFormat, InvalidData and allowed codec errors. Never on FileNotFound, DirectoryNotFound, cancellation, OOM, memory admission or decoder busy. Fallbacks are counted in metrics. |
| I-04 | Load modes | `LoadingMode`; `ImagePresenter`; `PreviewImageService` | Fast: adaptive decode only. Preview (default): thumbnail first, then adaptive. Original: no decode-size limit, never written to disk cache. |
| I-05 | Adaptive decode box | `Imaging/AdaptivePreviewPolicy.cs`; `MW.UpdateTargetDecodeBox` | viewport × DPI × 1.15; long side raised to at least 1200; sides rounded up to multiples of 128 and capped at 4000; fallback viewport 2200×1400. |
| I-06 | Cache key | `Imaging/ImageCacheKey.cs` | Full path upper-cased + Length + LastWriteUtcTicks + IsOriginal + TargetW×H (0 for Original) + OrientationApplied + Backend + SourceKind (0 standard, 1 RAW preview, 2 RAW full decode). Fingerprint is re-checked after decode (INV-1). |
| I-07 | RAM preview cache | `PreviewImageService`; `RamBudgetPolicy` | LRU by bytes. Budget = `ImageCacheRamPercent` of RAM minus the source-bytes cache, at most 50 % of RAM; `ImageCacheCapacityBytes` if RAM is unknown. In-flight dedup. 2 viewer decode slots. Original-dimensions cache of 200,000 entries. |
| I-08 | Disk preview cache | `PreviewImageService` (persist), `DiskCacheStore` | Directory `cache\`, `*.pv4` files, 4 GiB quota with LRU prune (access touch at most every 10 min), 2 persist workers, queue of 16 (drop when full), epoch check. Temp `*.tmp` older than 10 min removed. Format in D-06. |
| I-09 | Thumbnail cache | `Imaging/Caching/ThumbnailCache.cs` | Max width 800. RAM LRU 1 GiB. Disk `thumbnails\*.png` with a 1 GiB quota. Used for the Preview-mode first frame. Uses an embedded EXIF thumbnail when possible (`EmbeddedThumbnailReader`). |
| I-10 | Source-bytes cache | `SourceBytesCache`; `SourceBytesCachePolicy` | Off by default. LRU by path, length and mtime; 16 GiB requested, clamped to 20 % of RAM and the RAM-percent share. In-flight dedup. Generation and per-path version guard. Shared by preview, hash, preload and JPEG thumbnail. |
| I-11 | Preload | `Imaging/Preload/PreloadScheduler.cs`; `PreloadOrderService`; `NavigationPace` | Window: forward 32, backward 8 in the travel direction. During key-hold bursts the window starts up to 24 images ahead. Workers = PreloadWorkerCount. While the viewer decodes: limit max(2, cores/3), or 1 if source reads take > 200 ms. Whole-folder preload only when the estimate fits the budget (fill limit 0.9) and memory has headroom (load < limit and available ≥ reserve). Busy back-off: 3 passes, at most 5 retries. Restarted on navigation, Explorer reorder or catalog change. |
| I-12 | Memory guards | `Decoding/MemoryHeadroom.cs`; `WindowsMemoryProbe` | Decoders check output size × 2 against RAM above 128 MiB. Probe uses GlobalMemoryStatusEx and fails closed. |
| I-13 | RAW formats | `Imaging.Raw/RawFileTypes.cs`; Bmff, Tiff, Raf readers | .cr2 .cr3 .nef .arw .dng .raf .orf .rw2. `FormatRoutingDecoder` sends RAW to `RawDecoder`; with RAW off a RAW extension throws NotSupported. Container errors never fall back to WPF. |
| I-14 | RAW embedded preview | `Imaging.Raw/PreviewSelector.cs`; `RawJpegIccProfile` | Picks the smallest JPEG that covers the box, else the largest. Unknown sizes resolved by walking JPEG markers. Adobe RGB previews without ICC get a built-in CC0 profile. LibRaw is the fallback (ORF thumbnail, RAW without JPEG). |
| I-15 | RAW full decode | `Imaging.LibRaw/LibRawDecoder.cs`; `FullDecodeGate`; `ZoomDetailLoader` | Only with `RawFullDecode=OnZoom` and RAW on. Current image only, one gate slot, 300 ms indicator delay. `libraw.dll` loaded only from the app folder. |
| I-16 | Zoom detail (original) | `App/Coordinators/ZoomDetailLoader.cs`; `PreviewImageService.DecodeOriginalAsync` | When not in Fit, decodes the original of the current image (not cached), swaps it in without layout change, keeps at most one, drops it on navigation; view re-anchored on a size change. |
| I-17 | Capture groups and pair modes | `Core/Catalog/CaptureGroupBuilder.cs`; `CaptureGroup` | Same folder and basename, exactly one JPEG and one RAW. XMP chosen when unambiguous (`base.xmp`, else `file.ext.xmp`). Separate (default) = no grouping; PreferJpeg / PreferRaw = one catalog entry showing that member; actions apply to all members. |
| I-18 | Orientation and EXIF | `ExifOrientation`, `Metadata/ExifParser`, `ExifSummaryCodec` | Orientation baked into pixels. EXIF summary comes from the decode and is cached in the v7 disk header. |
| I-19 | Image transition | `ImageTransitionDecision`; `MW.StartImageFade` | Fade only on a change to another file: never for progressive upgrades of the same image, the first image after an open, or Compare. Outgoing layer opacity 1→0 with QuadraticEase-out; a new navigation replaces the fade (no queue). |
| I-20 | Decode file handles | All `IImageDecoder`, `ThumbnailCache` | Opened with FileShare.ReadWrite or Delete and SequentialScan, closed right after decode, bitmaps frozen (INV-8). |

## 7. Platform

| ID | Item | C# source | Notes |
|---|---|---|---|
| P-01 | Explorer view order | `Platform.Windows/Explorer/ExplorerOrderService.cs`; `ExplorerWindowSelector`; `ExplorerSnapshotValidator` | One STA COM thread. Reads all items via `IFolderView::Items(SVGIO_ALLVIEW\|VIEWORDER)` in batches of 512 PIDLs, falling back to per-item reads. Several windows or tabs are tried. Query timeout 2 s; startup prefetch 3 s; prefetch max age 30 s. Applied only if valid and the catalog has not been interacted with (INV-7). |
| P-02 | Fallback sort | `Core/Catalog/ImageSortService.cs`; `WindowsNaturalComparer` (StrCmpLogicalW); `ManagedNaturalComparer` | Default = file-system order; Name / NameAscending / NameDescending natural; Size ascending/descending then name. `SortModePolicy` decides whether Explorer order is used. |
| P-03 | Recycle Bin | `Platform.Windows/WindowsRecycleBin.cs`; `RecycleBinCapacity` | Recycle through VisualBasic FileIO / shell. Restore enumerates the bin and runs the shell verb, matching path, size and time; refused if the target exists. Capacity read from the BitBucket registry (per volume, NukeOnDelete, NoRecycleFiles, RecycleBinSize policy), failing closed. |
| P-04 | Single instance | `InstanceScope`; `InstanceKeys` | SingleWindow: one lock per user. PerFolder: lock per canonical folder, which follows the shown folder. A second launch forwards (timeout 1.5 s, `SecondInstanceHandoff`), else shows a message and exits with code 0. |
| P-05 | Forward pipe | `InstanceForwardServer/Client`; `Core/Instance/ForwardedPathProtocol.cs` | Pipe ACL allows only the current user; server retry delay 500 ms. Wire format in D-12. Paths validated (absolute, no "..", no device prefix, must exist). |
| P-06 | Folder ownership on open | `IFolderOwnership.BeforeOpenAsync` | PerFolder: a folder owned by another instance is forwarded to it; status "opened in other window" or "no response". |
| P-07 | Paths and env vars | `Core/AppPaths.cs` | Root `%LOCALAPPDATA%\PhotoReview`. `PHOTOREVIEW_DATA_ROOT` moves the journal, sessions and logs; `PHOTOREVIEW_ISOLATE_CONFIG` also moves config, caches and placement (tests). Empty LocalAppData falls back to Temp. |
| P-08 | Logging | `Core/Diagnostics/FileLog.cs`; `App/AppLog.cs` | Off unless `LoggingEnabled` or `PHOTOREVIEW_DIAG_FORCE_LOG`. Background queue (max 10,000). Rotates at 10 MiB to `app.1.log`. Startup errors, unhandled exceptions and DIAG mode are written even when logging is off (temporarily forced, then flushed). |
| P-09 | Update check | `Core/Updates/UpdateChecker.cs`; `UpdateUrlPolicy`; `AppVersion` | Manual only (Settings button). GET `https://api.github.com/repos/ConanKLOP2/PhotoReview/releases/latest`, User-Agent `PhotoReview/update-check`, 10 s timeout. Reads `tag_name` / `html_url`; drafts and prereleases ignored; compares major.minor.patch. Results: UpToDate, UpdateAvailable, or Failed (Offline, Timeout, RateLimited, BadResponse, InvalidVersion). Only opens `https://github.com/ConanKLOP2/PhotoReview/...` URLs. |
| P-10 | Memory probe | `WindowsMemoryProbe` | GlobalMemoryStatusEx. Headroom = load < limit and available ≥ reserve; fails closed. |
| P-11 | Display clock | `WindowsDisplayClock` | Vblank thread (`D3DKMTWaitForVerticalBlankEvent`) for the monitor under the window, falling back to DWM timing. Stops when idle. Used for the kinetic glide. |
| P-12 | Dark chrome | `WindowsDarkTitleBar`; `Themes/DarkMenus.xaml` | DWM immersive dark mode and caption colour; no-op if unsupported. |
| P-13 | Window placement | `App/WindowPlacementService.cs` | Restore and save with GET/SetWindowPlacement. Never reopens minimised. Leaving fullscreen saves the state before fullscreen. Ignored if less than 80×80 px is visible on any monitor's work area. |
| P-14 | Localization | `App/Localization/LocalizationService.cs`; `Core/Localization/LanguageLoader` | Shipped `<app>\Languages\*.json` plus user `%LOCALAPPDATA%\PhotoReview\Languages`. Args `--i18n-keys` / `--i18n-pseudo`. Live switch. |
| P-15 | Crash handling | `App.xaml.cs` | Dispatcher exceptions are marked handled; AppDomain and unobserved task exceptions are logged with forced logging. Startup failure shows a dialog and shuts down with code 1. |
| P-16 | Diagnostic env vars | `DiagOptions`, `PerfCsvListener` | `PHOTOREVIEW_DIAG_DISABLE_DISKCACHE`, `_FORCE_LOG`, `_INSTANCE_LABEL`, `_PRELOAD_WORKERS`, `_PREREAD`, `PHOTOREVIEW_PERF_TRACE` (CSV perf trace), `PHOTOREVIEW_BENCH_REAL_RECYCLE_BIN`. |

## 8. Invariants (docs/architecture.md)

| ID | Item | Guarding types | Main test classes |
|---|---|---|---|
| INV-1 | Full cache identity; stale pixels never presented | `ImageCacheKey`, `PreviewImageService`, `ImagePresenter`, `GenerationClock` | ImageCacheKeyTests, ImagePresenterTests (incl. .Adversarial), GenerationClockTests, PreviewImageServiceTests, WarmNavigationReadBoundsTests |
| INV-2 | Clear/evict invalidates in-flight and persist work | `PreviewImageService`, `DiskCacheStore`, `ThumbnailCache`, `SourceBytesCache` | PreviewImageServicePersistStaleTests, PreviewImageServiceTests, DiskCacheStoreTests, DiskCacheStorePruneRaceTests, ThumbnailCacheLifecycleTests, SourceBytesCacheTests, SourceBytesCacheRobustnessTests |
| INV-3 | Advance once before I/O; no re-present after the action | `MainViewModel`, `ImagePresenter`, `FileActionController` | MainWindowBehaviorTests (.Actions), MainViewModelFileActionTests, FileActionControllerOrderingTests |
| INV-4 | One file action at a time | `FileActionService`, `UndoService`, `FileActionGate` | FileActionServiceTests, FileActionGateTests, FileActionGateStressTests, UndoServiceTests, RecoveryRetryServiceTests, MainViewModelFileActionTests, CompositionRootTests |
| INV-5 | Old actions never modify catalog/undo after a folder change | `MainViewModel`, `GenerationClock`, `ReviewCatalog`, `FileActionController` | FileActionControllerFolderScopeTests, FileActionControllerFolderIdentityTests, MainViewModelFileActionTests (.FolderChange/.FolderScope), MainViewModelMoveCopyToTests, MainWindowBehaviorTests (.FolderSwitch) |
| INV-6 | Recycle; journal Prepared → Committed/Failed; startup reconcile | `FileActionService`, `OperationJournal`, `JournalTransaction`, `RecoveryRetryService`, `UndoService`, `JournalStartupRecovery` | JournalStartupRecoveryTests, OperationJournalTests, JournalTransactionTests, JournalReconcileRaceTests, JournalGroupReconcileTests, FileActionServiceTests, UndoServiceTests |
| INV-7 | Late Explorer snapshots dropped; probe results applied after the order settles | `FolderLoadCoordinator`, `ReviewCatalog`, `ExplorerSnapshotValidator` | FolderLoadCoordinatorTests (partials), ExplorerSnapshotValidatorTests, MainWindowBehaviorTests (.Explorer), CacheExplorerRegressionTests |
| INV-8 | Decode handles share ReadWrite/Delete, SequentialScan, closed early | all `IImageDecoder`, `ThumbnailCache` | DecoderQualityGateTests, WarmNavigationReadBoundsTests, ThumbnailCacheTests, TurboJpegSharingAndHeaderGrowthGapTests, FileActionConcurrencyTests |
| INV-9 | Opened file presented at once, navigation waits for `PendingOrder`; folder shows fallback first; probe after the first frame | `FolderLoadCoordinator`, `MainViewModel` | FolderLoadCoordinatorTests (incl. .Probe), MainWindowBehaviorTests (.Explorer), FolderLoadSkippedFilesTests |
| INV-10 | Logging off by default; off creates no log | `FileLog`, `AppLog`, `AppSettings.LoggingEnabled` | FileLogTests, AppLogTests, FileLogLifecycleTests |
| INV-11 | Corrupt config backed up and reset; locked config gives in-memory defaults | `SettingsStore` | SettingsStoreTests, SettingsRobustnessTests, SettingsStoreFailureTests, SettingsStoreLegacyAndWriteFailureTests, CoreFileSystemBoundaryTests |
| INV-12 | Backend failure falls back to WPF; cache never mixes backends | `ImageDecoderFactory`, `FallbackImageDecoder`, `ImageCacheKey`, `ReviewMetrics` | DecoderQualityGateTests, FactoryTests, FallbackExceptionCoverageTests, WicDirectFactoryFailureTests |

Also relevant: AppThreadAffinityTests (ADR 0005 threading rule).

## 9. On-disk formats to keep compatible

| ID | Item | C# source | Notes |
|---|---|---|---|
| D-01 | `config.json` | `SettingsStore`; `AppSettingsJsonContext` | Location `%LOCALAPPDATA%\PhotoReview\config.json`. UTF-8 without BOM, indented. PascalCase property names in the order of S-01..S-63. Enums written as names; InitialViewMode written as "Fit", "100%", "200%", "400%", else the name. Flags written as "A, B" or "None". `Actions` is an array of {Name, Shortcut, Operation, Destination, Confirm}; `Shortcuts` is an object. Readers must accept comments, trailing commas, numbers given as strings, enum aliases, case-insensitive enum names and numeric enum values. |
| D-02 | Corrupt config backup | `SettingsStore.UniqueBackupPath` | `config.json.corrupt-yyyyMMddHHmmss` (UTC), with `-2` .. `-99` suffix on collision. |
| D-03 | `operations.jsonl` | `OperationJournal`; `JournalEntry`; `JournalLineParser` | Location `<dataRoot>\operations.jsonl` (default `%LOCALAPPDATA%\PhotoReview\Data`). One compact JSON object per line, UTF-8 without BOM, line ending `Environment.NewLine` (CRLF). Field order: Id (32 hex), Type ("Move", "Copy", "Recycle"; "Delete" accepted on read), State, Source, Destination (null for Recycle), Size, LastWriteUtc, TimestampUtc (ISO-8601 DateTime), Error (written even when null), then only when non-null: ErrorCode, Permanent, Undo, GroupId, GroupMembers [{Source, Destination, Size, LastWriteUtc, Permanent}]. A line is rejected if malformed, Type/State unknown or a comma list, Id empty, or Source blank or containing NUL. Writers append only; readers skip bad lines. |
| D-04 | Session files | `SessionStore.GetPath` | `<dataRoot>\Sessions\<SHA256 hex (uppercase) of UTF-8(UPPER(canonical folder))>.json`. Canonical folder = full path without trailing separator; a drive root keeps its separator. Body indented {Folder, CurrentPath, Skipped[], UpdatedUtc}. |
| D-05 | Atomic temp naming | `PhysicalFileSystem.WriteAllTextAtomic`, `AtomicCacheFile`, `WindowPlacementService` | `<target>.<guidN>.tmp` then rename/replace. Settings write fsynced; session not. |
| D-06 | Preview disk cache | `PreviewImageService.GetDiskCachePath`; `PreviewCacheFile` | `cache\<SHA256 HEX upper>.pv4`. Identity string: `preview-v5-box\|{UPPERPATH}\|{Length}\|{Ticks}\|{IsOriginal True/False}\|{W}x{H}\|{OrientationApplied}\|{Backend}`, plus `\|kind{n}` for non-standard sources. File header, little-endian: "PRVC", version 7 (6 still readable), backend byte, orientation 1-8, alpha flag, width, height, original width, original height, then (v7) u16 EXIF length + EXIF summary bytes, then the JPEG payload (q95, 4:4:4). |
| D-07 | Thumbnail disk cache | `ThumbnailCache.BuildKey` | `thumbnails\<sha256 hex lowercase>.png`. Stamp: `{fullPath as given, not upper-cased}\|{length}\|{lastWriteUtc.Ticks}\|800\|orient=1`. |
| D-08 | Log format | `FileLog.FormatEntry` | `logs\app.log` under the data root or app root. Line: `yyyy-MM-dd HH:mm:ss.fff [Level] [T<threadId>] message` (invariant culture), followed by `\n` + exception text when present. UTF-8 without BOM, append. Rotates to `app.1.log` at ≥ 10 MiB. |
| D-09 | Action export file | `ActionProfilesWindow.TryExport/TryImport` | JSON array of ReviewAction (default file name `photoreview-actions.json`). Import refuses an empty list or null items. |
| D-10 | Settings import/export | `SettingsWindow` Import/Export; `SettingsStore.ParseText` | Same schema as D-01; import goes through the full repair pipeline. |
| D-11 | `window-placement.json` | `WindowPlacementService` | Indented JSON with fields: Length, Flags, ShowCommand (1 normal / 3 maximized), MinPosition {X,Y}, MaxPosition {X,Y}, NormalPosition {Left,Top,Right,Bottom}. |
| D-12 | Forward pipe protocol | `ForwardedPathProtocol` | Strict UTF-8: `PHOTOREVIEW-OPEN 1\n` + one path per line + an empty line. Max 64 KiB, 16 paths, 32767 characters per path. |
| D-13 | Kernel object names | `InstanceKeys`, `InstanceForwardPipe`, `WindowsLiveOperationRegistry` | SingleWindow mutex: `Local\PhotoReview_App_<SHA256 hex of "app\|SID">`. PerFolder mutex: `Local\PhotoReview_<SHA256 hex of UPPER(full path without trailing separator), or "PhotoReview" when no folder>`. Pipe: `PhotoReview.Forward.<SHA256 hex of "mutex\|SID\|sessionId">`. Live-operation event: `Local\PhotoReview_LiveOp_<SHA256 hex of "liveop\|SID\|id">`. Keep these so old and new builds detect each other. |
| D-14 | Journal compaction temp | `OperationJournal.CompactionFileSuffix` | Suffix `.compact.tmp` next to the journal. Stale ones (older than 1 h) removed. |
| D-15 | User language files | `LanguageLoader`, `TranslationExport` | `%LOCALAPPDATA%\PhotoReview\Languages\<code>.json` (flat JSON) and the exported `<code>.todo.json`. |

## Gaps to carry over (persisted settings with no runtime effect)

Two settings are saved and repaired but nothing in the app reads them at runtime. Keep them in the schema for compatibility; whether to wire them up is a decision for the rewrite.
- **S-12 `MemoryReserveBytes`**: preload uses the hard-coded `PreloadOptions.ReserveBytes` (2 GiB) instead.
- **S-15 `PreviewDiskCacheCapacityBytes`**: `PreviewImageService` uses its own built-in 4 GiB quota instead.
| Q-R10 | Done (W2-19) | `pr_core::instance::protocol` | `instance_forwarding::golden_forwarded_path` (golden `forwarded-path`) | Wire format and limits |
| Q-R12 | Done (W2-19) | `pr_core::instance::handoff` | `instance_forwarding::handoff_exits_only_when_the_owner_took_or_likely_took_the_request` | `Unknown` exits like `Delivered` |
| Q-R18 | Done (W2-19) | `pr_core::instance::ownership` | (data and trait only) | Decision enum; implementation is Wave 6 |
| SEC-02 | Done (W2-19) | `pr_core::instance::protocol::encode` | `instance_forwarding::protocol_encode_rejects_what_decode_would_reject` | Encode shares is_acceptable_path |
| RV-S05 | Done (W2-19) | `pr_core::instance::protocol::try_decode` | `instance_forwarding::try_decode_bytes_of_lone_surrogate_returns_false` | Strict UTF-8; lone surrogate unrepresentable in `&str` |
| RV-P01 | Done (W2-19: mutant killed by `coalescer_dispose_while_delay_ignores_cancel_still_does_not_open`) | `pr_core::instance::coalescer` | `instance_forwarding::coalescer_submit_does_not_run_or_wait_for_the_open_callback` | Submit never runs the open callback |
