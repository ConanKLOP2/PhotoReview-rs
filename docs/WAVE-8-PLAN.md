# Wave 8 plan: App/UI (proposed 2026-10-18, baseline C# `d92d6aae`)

Supersedes the Wave 8 sketch in `MULTI-AGENT-PLAN.md` section 3 (nine coarse WUs written before R0001 and before the C# move to
Win32 + Direct2D, C# ADR 0010). Exemplar: `WAVE-4-PLAN.md`. Granularity: P-18 (about 400 C# lines per sub-unit). Scheduling:
P-28 dependency graph (the `Depends on` column), not tier barriers. Models: Haiku or Sonnet only (P-34). Line counts are C# lines
at `d92d6aae` (`git show d92d6aae:<path> | wc -l`); a split of one file gives approximate line ranges, the contract writer fixes
the cut at method boundaries. Direction (R0001, P-06): Win32 window + Direct2D/DirectWrite through `windows-rs`; status of the ADR
is still Proposed because of the unresolved windowed p95 `EndDraw` stall (see "Risks").

## Scope
In scope: 289 C# files, about 47 900 lines, in `PhotoReview.App.Shared`, `PhotoReview.App`,
`PhotoReview.Shell.Win32`, `PhotoReview.Shell.Interop`, `PhotoReview.Shell.Rendering`.

| C# project | Files / lines | What happens |
|---|---|---|
| App.Shared | 102 / 14 900 | Ported (units 01 to 10). Pure logic, testable without a window |
| Shell.Win32 | 124 / 24 500 | Ported (units 11 to 19). The real app: window, overlay, viewing, dialogs, startup |
| Shell.Rendering | 13 / 2 560 | Ported (unit 12). Direct2D draw context, device, caches, text |
| Shell.Interop | 29 / 3 270 | Mostly replaced by the `windows` crate (D36: only `pr-platform-win` depends on it). Units 11a to 11h port only the custom parts and audit the rest |
| App (WPF) | 21 / 2 690 | Retired, with a mapping table below. `App.WpfWindows`, `Shell.WpfBridge`, `Imaging.Wpf` are not in scope |

## Crates and layering (hard rules from AGENTS.md)
- **`pr_app` (lib, no `unsafe`, no `windows` crate in the pure modules):** `viewport`, `menus`, `windowing`, `input`, `viewmodels`,
  `coordinators`, `presenters`, `overlay` (element tree, animator, palette), `services`, `composition`, `startup` (logic), `localization`.
  Everything in these modules is tested without a window. OS access goes through traits (`WindowOps`, `UiDispatcher`, `Clipboard`,
  `DialogService`, `FolderPicker`, `FrameClock`, `RenderSurface`) implemented in `pr-platform-win`.
- **`pr_platform_win` (all `unsafe`, each block with SAFETY):** new modules `win32` (constants, safe wrappers), `gfx` (D3D11/DXGI/D2D/DWrite
  helpers), `render` (Shell.Rendering), `host` (message loop, UI dispatcher, windows, frame clocks), `shell_input`, `shell_menus`,
  `shell_viewing`, `shell_overlay`, `shell_startup`, `dialogs` (native dialogs, settings UI). Reuses what exists: `spike_ui.rs` (R0001),
  `dark_title_bar`, `forward_*`, `instance_*`, `recycle_bin*`, `explorer_*`, `wic`.
- **`pr-app` binary:** `main.rs` is the Rust `Program.cs` (W8-20a). The `ui-spike` feature and `r0001-spike` bin stay until W8-20b.
- Reused from earlier waves (do not re-port): `pr_core::{settings, shortcuts, catalog, file_actions, diagnostics, text}`, `pr_l10n`,
  `pr_imaging::{service, preload, cache}`, `pr_bench`, `pr_perf`.
- WPF-only members are never ported: dispatcher priorities become the UI dispatcher trait (13d); `INotifyPropertyChanged` /
  `ObservableObject` becomes an explicit `ChangeSet` returned by view-model methods (decided in the 08a contract, see D-W8-1 below).

## Legend
Colour and model follow `MULTI-AGENT-PLAN.md` section 1: 🟢 Haiku port, 🟡 Sonnet port, 🔴 Sonnet port plus manual review in the
PR (P-34 replaces the Opus verifier). **Test** column: **G** pure, golden from `tools/oracle-cs` (the C# class run on the baseline);
**F** pure but needs fakes/hand-written expectations from the C# tests (no oracle value); **N** native-only, tests are
`#[ignore = "native: needs desktop"]` and run on `windows-latest` (hidden window or off-screen target); **G+N** pure core with golden plus
a native twin. Batch column = contract batch (section "Batches").

## Units

### 01 Viewport and geometry (`pr_app::viewport`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-01a | `Viewport/{ViewportContracts 65, ViewportSnapshot 27, ViewportState 99, ArrowPanRule 52}` = 243 | `viewport::{types, state, arrow_pan}` | 🟢 | Haiku | G | none | B1 |
| W8-01b | `Viewport/ViewportLayoutEngine` 226 | `viewport::layout` (Fit, source-pixel zoom, ADR 0008) | 🟢 | Haiku | G | 01a | B1 |
| W8-01c | `Shell.Rendering/RenderGeometry` 235 | `viewport::render_geometry` (pure; the draw context only consumes it) | 🟢 | Haiku | G | 01a | B1 |
| W8-01d | `Coordinators/{FitViewController 46, IFitSurface 32}`, `Menus/ZoomMenuRules 15`, `ViewModels/{ViewerStretchMode 11, ViewRefreshOutcome 10}` = 114 | `viewport::fit`, `menus::zoom_rules` | 🟢 | Haiku | F | 01a | B1 |

### 02 Menus and text formatters (`pr_app::menus`, `pr_app::viewmodels::format`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-02a | `Menus/{ContextMenuModelBuilder 136, MenuContracts 32}` = 168 | `menus::{model, builder}` (C-* rows) | 🟢 | Haiku | G | 01d | B1 |
| W8-02b | `ViewModels/{ExifFormatter 113, StatusFormatter 117, TitleBarFormatter 72, SettingsReloadPath 32}` = 334 | `viewmodels::format` | 🟢 | Haiku | G | `pr_l10n`, `pr_imaging::exif_summary` | B1 |

### 03 Window placement and fullscreen (`pr_app::windowing`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-03a | `Windowing/{WindowingContracts 72, MonitorLayout 80, WindowPlacementVisibility 24, WindowPlacementJsonContext 35, JsonWindowPlacementStore 150}` = 361 | `windowing::{contracts, monitor_layout, visibility, placement_store}` (file format identical, lives under `%LOCALAPPDATA%\PhotoReviewRs`) | 🟡 | Sonnet | G | `pr_core::identity` | B1 |
| W8-03b | `Windowing/FullscreenController` 210 | `windowing::fullscreen` over a `WindowOps` trait (style bits, one SetWindowPos, exact restore) | 🟡 | Sonnet | F | 03a | B1 |

### 04 Localization glue and small services (`pr_app::{localization, services, misc}`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-04a | `Localization/{LanguageOptions 53, LocalizationSource 24, TranslationProblems 28, LocalizationService 121}` = 226 | `localization::*`; check overlap with `pr_l10n` (W2-04) first, port only the delta | 🟢 | Haiku | G | `pr_l10n` | B1 |
| W8-04b | `Localization/TranslationExport` 188 | `localization::export` | 🟢 | Haiku | G | 04a | B1 |
| W8-04c | `Services/{IClipboardService 8, IPresentationObserver 19, IZoomPromptService 10, InitialImagePrewarm 32, PreviewStateContext 16, RenderFrameTrace 43, ShortcutKeyName 28, UnavailableClipboardService 12, ViewportSizeSource 52, InitialViewportPredictor 131}` = 351 (`WpfKeyNameValidator 9` retired: `KeyId` mapping replaces it) | `services::*` | 🟢 | Haiku | F | 01a | B1 |
| W8-04d | `{AppLog 73, BuildInfo 50, LaunchArguments 33, TaskLogging 49}` = 205 | `misc::{app_log, build_info, launch_args, task_logging}` | 🟢 | Haiku | F | `pr_core::diagnostics` | B1 |
| W8-04e | `{FileHashService 128, MainWindowConvergence 52, MainWindowHelpers 169}` = 349 | `misc::{file_hash, viewport_convergence, window_helpers}`; drop any WPF-only helper and say so in the PARITY row | 🟡 | Sonnet | F | 01a | B1 |

### 05 Input (`pr_app::input`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-05a | `Input/{InputPrimitives 45, FrameClock 24, ViewportOperationVersion 14, WheelMessageSource 54, IImageSurface 76, ReviewCommand 98}` = 311 | `input::{primitives, frame_clock, surface, review_command}` | 🟢 | Haiku | F | 01a | B1 |
| W8-05b | `Input/KeyId` 219 | `input::key_id` | 🟢 | Haiku | G | none | B1 |
| W8-05c | `Input/KeyIdMapping` 419 (table, one file) | `input::key_mapping` (VK to `KeyId`, names, K-* rows) | 🟢 | Haiku | G | 05b | B1 |
| W8-05d | `Input/ShortcutRouter` 300 | `input::shortcut_router` (K-* rows) | 🟡 | Sonnet | G | 05a, 05b, 05c, `pr_core::shortcuts` | B1 |
| W8-05e | `Input/{MouseGestures 161, TouchpadGestures 172}` = 333 | `input::{mouse_gestures, touchpad_gestures}` | 🟢 | Haiku | G | 05a | B1 |
| W8-05f | `Input/KineticPan` 377 | `input::kinetic_pan` (clock injected, `pr_core::time`) | 🟡 | Sonnet | G | 05a | B1 |
| W8-05g | `Input/PointerInputController` lines 1 to 300 (wheel nav, zoom at point, anchor scroll, fit height) | `input::pointer::{wheel, zoom}` | 🔴 | Sonnet | F | 01b, 01d, 05a, 05e | B2 |
| W8-05h | `Input/PointerInputController` lines 300 to 672 (fit width, click zoom, pan, kinetic hook, arrow pan) | `input::pointer::{click_zoom, pan}` | 🔴 | Sonnet | F | 05g, 05f | B2 |

### 06 View-model state, policies and coordinator seams (`pr_app::{viewmodels, coordinators}`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-06a | `ViewModels/ViewerState` 450 (zoom/fit/fullscreen/stretch state machine) | `viewmodels::viewer_state` | 🟡 | Sonnet | G+F | 01a, 01d | B1 |
| W8-06b | `ViewModels/InfoOverlayViewModel 196`, `Coordinators/{InfoOverlayAutoHidePolicy 33, ToolbarAutoHidePolicy 38, FadeTargetGate 18, ImageTransitionDecision 29}` = 314 | `viewmodels::info_overlay`, `coordinators::{autohide, fade_gate, transition}` | 🟢 | Haiku | G | 02b | B1 |
| W8-06c | `ViewModels/CompareViewModel` 289 | `viewmodels::compare` | 🟡 | Sonnet | F | 02b | B2 |
| W8-06d | `Coordinators/{IDuplicateCleanupSink 31, IFileActionExecutor 32, IFileActionSink 39, IFolderLoadSink 56, IFolderPicker 14, IMoveOverride 14, IPreloadController 44, IPresentationSink 27, ISiblingNavigatorSink 14, PreloadControllerAdapter 32}` = 303 | `coordinators::traits` (traits only; Rust sinks take a `&mut dyn` or return a `ChangeSet`, D-W8-1) | 🟢 | Haiku | F | none | B1 |
| W8-06e | `Coordinators/FileActionGate` 152 (serializes file actions, INV-3..6 caller side) | `coordinators::file_action_gate` | 🔴 | Sonnet | F | 06d | B2 |
| W8-06f | `Coordinators/SiblingFolderNavigator` 130 | `coordinators::sibling_navigator` | 🟡 | Sonnet | F | 06d | B2 |

### 07 Coordinators (`pr_app::coordinators`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-07a | `Coordinators/FolderLoadCoordinator` lines 1 to 270 (load, generation token, INV-9) | `coordinators::folder_load::{state, load}` | 🔴 | Sonnet | F | 06d, `pr_core::catalog` | B2 |
| W8-07b | `Coordinators/FolderLoadCoordinator` lines 270 to 544 (reload, skipped entries, cancel) | `coordinators::folder_load::{reload, skipped}` | 🔴 | Sonnet | F | 07a | B2 |
| W8-07c | `Coordinators/FileActionController` lines 1 to 290 (ctor, Recycle, no-bin refusal, permanent-delete prompt) | `coordinators::file_actions::{recycle}` | 🔴 | Sonnet | F | 06d, 06e, `pr_core::file_actions` | B2 |
| W8-07d | `Coordinators/FileActionController` lines 290 to 580 (action profiles, undo, restored-move positions, late completion) | `coordinators::file_actions::{actions, undo}` | 🔴 | Sonnet | F | 07c | B2 |
| W8-07e | `Coordinators/FileActionController` lines 580 to 858 (move/copy to folder, `ReloadPathAfterUndo`) | `coordinators::file_actions::move_copy` | 🔴 | Sonnet | F | 07d | B2 |
| W8-07f | `Coordinators/DuplicateCleanupController` 305 | `coordinators::duplicate_cleanup` | 🔴 | Sonnet | F | 06d, 06e, 07c | B2 |
| W8-07g | `Coordinators/ZoomDetailLoader` 310 (full-resolution decode on zoom, cancel) | `coordinators::zoom_detail` | 🔴 | Sonnet | F | 01a, 06d, `pr_imaging::service` | B2 |
| W8-07h | `Coordinators/ImagePresenter` lines 1 to 232 (fields, ctor, `ShowZoomDetailImage`, `SetViewerZoom`, evict) | `coordinators::presenter::{state, zoom}` | 🔴 | Sonnet | F | 06b, 06c, 06d, 07g | B2 |
| W8-07i | `Coordinators/ImagePresenter` lines 232 to 500 (`PresentCoreAsync` first half, INV-1 choke point) | `coordinators::presenter::present_a` | 🔴 | Sonnet | F | 07h | B2 |
| W8-07j | `Coordinators/ImagePresenter` lines 500 to 735 (`PresentCoreAsync` second half, compare, failure paths) | `coordinators::presenter::present_b` | 🔴 | Sonnet | F | 07i | B2 |
| W8-07k | `Coordinators/ImagePresenter` lines 735 to 949 (`RemoveMissingCatalogItemAsync`, compare pair, stat, `PhotoInfo`) | `coordinators::presenter::{missing, compare_pair, photo_info}` | 🔴 | Sonnet | F | 07j | B2 |

### 08 MainViewModel (`pr_app::viewmodels::main`; one Rust module, five files)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-08a | `ViewModels/MainViewModel` lines 1 to 330 (fields, ctor, properties, status, prewarm) | `viewmodels::main::state` | 🟡 | Sonnet | F | 06a, 06b, 06c, 06d | B2 |
| W8-08b | lines 330 to 565 (capture group, compare, open folder/path, next/prev/first/last/skip, sibling folder) | `viewmodels::main::navigation` | 🔴 | Sonnet | F | 08a, 07a, 07b, 06f, 07k | B2 |
| W8-08c | lines 565 to 860 (run action, recycle, undo, zoom, overlay, fullscreen, refresh, external editor, copy path, duplicates, clear cache, show dialogs) | `viewmodels::main::commands` | 🔴 | Sonnet | F | 08b, 07c, 07d, 07f | B2 |
| W8-08d | lines 860 to 1100 (settings dialog entry, session persist, instance label, title bar, localized refresh) | `viewmodels::main::session` | 🟡 | Sonnet | F | 08c, 02b | B2 |
| W8-08e | lines 1100 to 1385 (change notification, move/copy to folder, remembered folders, `ExifAutomationName`) | `viewmodels::main::notify_move` | 🟡 | Sonnet | F | 08d, 07e | B2 |

### 09 Composition (`pr_shell::composition`; no DI container, explicit builder)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-09a | `Composition/{DecoderProviders 84, LibRawPreviewFallback 16, ServiceFactories 80, SharedServiceContracts 23}` = 203 | `composition::{decoders, factories, contracts}` | 🟡 | Sonnet | F | `pr_imaging`, `pr_imaging_raw` | B2 |
| W8-09b | `Composition/{SharedServiceRegistration 243, MainViewModelCompositionRoot 98}` = 341; `App/Composition/AppHost 24` folded in | `composition::{registry, main_view_model}` | 🟡 | Sonnet | F | 09a, 08e, 07a..07k | B6 |

### 10 Presenters (`pr_app::presenters`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-10a | `Presenters/{ActionListJson 53, ActionProfilesPresenter 221}` = 274 | `presenters::action_profiles` | 🟢 | Haiku | G+F | `pr_core::settings` | B1 |
| W8-10b | `Presenters/SettingsFieldMap` 411 | `presenters::settings_field_map` | 🟢 | Haiku | G | `pr_core::settings` | B1 |
| W8-10c | `Presenters/{SettingsFormState 128, SecondaryDialogPresenters 160}` = 288 | `presenters::{settings_form_state, secondary}` | 🟢 | Haiku | F | none | B1 |
| W8-10d | `Presenters/SettingsPresenter` lines 1 to 320 | `presenters::settings::{load, validate}` | 🟡 | Sonnet | F | 10b, 10c, 04a | B2 |
| W8-10e | `Presenters/SettingsPresenter` lines 320 to 636 | `presenters::settings::{apply, shortcuts}` | 🟡 | Sonnet | F | 10d, 05c | B2 |
| W8-10f | `Presenters/{RecoveryFilter 23, RecoveryPresentation 237}` = 260 | `presenters::{recovery_filter, recovery_presentation}` | 🟢 | Haiku | G | `pr_core::file_actions` | B1 |
| W8-10g | `Presenters/RecoveryWindowPresenter` 332 (INV-5 on screen: Restore/Dismiss decisions) | `presenters::recovery_window` | 🟡 | Sonnet | F | 10f, 06e | B2 |
| W8-10h | `Presenters/BenchmarkPresentation` 143 | `presenters::benchmark_presentation` | 🟢 | Haiku | G | `pr_bench` | B1 |
| W8-10i | `Presenters/BenchmarkPresenter` 416 | `presenters::benchmark` | 🟡 | Sonnet | F | 10h, `pr_bench` | B2 |

### 11 Win32 foundation (`pr_platform_win::{win32, gfx}`): replaces `Shell.Interop` (3 270 lines)
Shell.Interop is P/Invoke and COM vtable declarations. The `windows` crate (`windows-rs`, already used by the R0001 spike for D3D11, DXGI,
Direct2D and DirectWrite) generates them, so the units port only the custom constants and wrappers and audit the rest. The audit
output is a table in the contract: C# member, windows-rs item or `GAP`; a `GAP` becomes a hand-written declaration with SAFETY.
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-11a | `Win32/{Structs 137, WindowMessages 192}` = 329 | `win32::{structs, messages}` (custom `WM_*`, constants equal the C# values) | 🟢 | Haiku | G (constants) | none | B3 |
| W8-11b | `Win32/{User32 184, Kernel32 23, Kernel32Loader 17, Gdi32 12, Shell32 10, Shcore 13, Dwmapi 19, Ole32 20}` = 298 | `win32::api` (feature list, safe wrappers used by hosts) | 🟡 | Sonnet | N | 11a | B3 |
| W8-11c | `Win32/{CommonControlsNative 280, Comctl32 39, ActivationContext 39}` = 358 | `win32::comctl` (manifest/activation context, comctl v6) | 🟡 | Sonnet | N | 11a | B3 |
| W8-11d | `Win32/{DialogChromeNative 143, DialogNative 223}` = 366 | `win32::dialog_native` | 🟡 | Sonnet | N | 11a | B3 |
| W8-11e | `Win32/{KeyCaptureNative 20, SettingsDialogNative 61, StartupAdditions 27}`, `Shell/{IDataObject 81, IDropTarget 38, IFileOpenDialog 222}` = 449 | `win32::{key_capture, settings_native, startup}`; Shell COM interfaces come from windows-rs | 🟡 | Sonnet | N | 11a | B3 |
| W8-11f | `Graphics/{ComInterop 70, Guids 52, RenderingAdditions 94, DWriteAdditions 64}` = 280 | `gfx::{com, guids, extras}` | 🔴 | Sonnet | N | none | B3 |
| W8-11g | `Graphics/{D3D11 195, Dxgi 264}` = 459 (audit only) | `gfx::{d3d11, dxgi}` thin helpers over windows-rs (swap chain, device flags); audit table of C# members | 🔴 | Sonnet | N | 11f | B3 |
| W8-11h | `Graphics/{D2D1 476, DWrite 251}` = 727 (audit only) | `gfx::{d2d, dwrite}` thin helpers over windows-rs; audit table | 🔴 | Sonnet | N | 11f, 11g | B3 |

### 12 Rendering (`pr_platform_win::render`, from `Shell.Rendering`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-12a | `Contracts/{GpuImageContracts 33, RenderSurfaceContracts 94, TextContracts 26}`, `TiledGpuImage 62`, `D2DRenderSurfaceFactory 13` = 228 | `render::contracts` as traits in `pr_app::services::render` (no `windows` types), `render::tiled` | 🟢 | Haiku | F | 01c | B1 |
| W8-12b | `DeviceResources` lines 1 to 230 (device creation, adapter choice, device loss) | `render::device::{create, loss}` | 🔴 | Sonnet | N | 11f, 11g, 11h, 12a | B3 |
| W8-12c | `DeviceResources` lines 230 to 456 (brushes, caches, text factories) | `render::device::resources` | 🔴 | Sonnet | N | 12b, 11g, 11h | B3 |
| W8-12d | `D2DRenderSurface` 236 (swap chain, resize, `EndDraw`, `Present`) | `render::surface` | 🔴 | Sonnet | N | 12c, 11f, 11g, 12e (type; 12d and 12e are ported together) | B3 |
| W8-12e | `D2DDrawContext` 359 | `render::draw_context` | 🔴 | Sonnet | N | 12c, 12d, 12a, 01c, 11h | B3 |
| W8-12f | `GpuImageCache` lines 1 to 200 (budget and LRU eviction policy, pure part) | `render::image_cache::policy` | 🟡 | Sonnet | F | 12a, `pr_core::caching::lru` | B3 |
| W8-12g | `GpuImageCache` lines 200 to 414 (bitmap upload, tiles, device-loss rebuild) | `render::image_cache::upload` | 🔴 | Sonnet | N | 12f, 12c, 11f | B3 |
| W8-12h | `GpuKeepAlive` 231 (WP21-STALL keep-warm thread, env vars `PHOTOREVIEW_RS_GPU_KEEPWARM*`) | `render::keep_alive` | 🔴 | Sonnet | F+N | 12c, 11f | B3 |
| W8-12i | `Text/{DWriteTextRenderer 201, TextLayoutCache 197}` = 398 | `render::text::{renderer, layout_cache}` | 🟡 | Sonnet | F+N | 12c, 11f | B3 |
| W8-12j | NEW, no C# source: swap-chain latency experiment (waitable swap chain, `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT`, DirectComposition) behind an env switch; only started if an attended re-measure still shows the `EndDraw` stall (R0001) | `render::surface` option | 🟡 | Sonnet | N | 12d | B3 (conditional) |

### 13 Window host (`pr_platform_win::host`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-13a | `Hosting/{FrameClockBase 184, TimerFrameClock 149}` = 333 | `host::frame_clock::{base, timer}` | 🟡 | Sonnet | F | 05a | B3 |
| W8-13b | `Hosting/{SwapChainFrameClock 100, ShellWindowContracts 67, ShellIcons 68, NativeRect 13}` = 248; `PendingInterop/PendingNativeMethods 102` retired (windows-rs) | `host::{swap_chain_clock, contracts, icons}` | 🟡 | Sonnet | N | 13a, 12d | B3 |
| W8-13c | `Hosting/MessageLoop` 354 | `host::message_loop` | 🔴 | Sonnet | F+N | 11b | B3 |
| W8-13d | `Hosting/Win32UiDispatcher 389`; `Win32UiSynchronizationContext 80` retired (becomes a channel plus `PostMessage` wake) | `host::ui_dispatcher` implementing the `UiDispatcher` trait | 🔴 | Sonnet | F+N | 11b, 13c | B3 |
| W8-13e | `Hosting/WndProcThunk 104`, `ShellWindow` lines 1 to 250 | `host::{wndproc, shell_window::create}` (GWLP_USERDATA trampoline, panic guard at the FFI boundary) | 🔴 | Sonnet | N | 11a, 11b, 13c | B3 |
| W8-13f | `Hosting/ShellWindow` lines 250 to 516 | `host::shell_window::{messages, dpi}` | 🔴 | Sonnet | N | 13e | B3 |
| W8-13g | `ShellMainWindow.cs` lines 1 to 226 (fields, ctor, composition of parts) | `host::main_window::core` | 🔴 | Sonnet | N | 13f, 12d | B4 |
| W8-13h | `ShellMainWindow.cs` lines 226 to 452 | `host::main_window::lifecycle` | 🔴 | Sonnet | N | 13g | B4 |
| W8-13i | `ShellMainWindow.Frame 169`, `ShellMainWindow.ScrollBars 34`, `Program.cs 16` = 219 | `host::main_window::{frame, scrollbars}` (Program becomes W8-20a) | 🟡 | Sonnet | N | 13h | B4 |
| W8-13j | `ShellMainWindow.Viewing` 336 | `host::main_window::viewing` | 🔴 | Sonnet | F+N | 13h, 16h | B4 |
| W8-13k | `Windowing/{ShellFullscreen 163, TitleBar 52, ShellMainWindow.Windowing 127}` = 342; WPF `FullscreenWindowPlacer 63` retired | `host::main_window::windowing` (real `WindowOps`), `dark_title_bar` reuse | 🟡 | Sonnet | F+N | 03b, 13h | B4 |

### 14 Win32 input and menus (`pr_platform_win::{shell_input, shell_menus}`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-14a | `Input/{ClickCounter 58, InputNative 99, KeyboardMessageHandler 60, PointerMessageHandler 50, WheelMessageHandler 31, ShellInputEnvironment 30}` = 328; WPF `WpfInputAdapters 23` retired | `shell_input::{click_counter, handlers}` (decode WPARAM/LPARAM; tested with synthetic messages) | 🟡 | Sonnet | F | 05a, 05c, 11a | B4 |
| W8-14b | `Input/ShellInput` 420 | `shell_input::dispatch` | 🔴 | Sonnet | F | 14a, 05d, 05g, 05h | B4 |
| W8-14c | `Input/{ViewportImageSurface 87, ShellMainWindow.Input 293}` = 380; WPF `WpfImageSurface 135` retired | `shell_input::{surface, main_window}` | 🟡 | Sonnet | F | 14b, 13h | B4 |
| W8-14d | `Input/DropTarget` 249 (OLE drop: `#[implement]` COM object) | `shell_input::drop_target` | 🔴 | Sonnet | N | 11e, 13h | B4 |
| W8-14e | `Menus/{ContextMenuController 207, ContextMenuMessageHandler 57, IPopupMenuHost 10, NativeMenuMapping 35, MenuOwnerDraw 59}` = 368 | `shell_menus::{controller, mapping}` | 🟡 | Sonnet | F | 02a, 11b | B4 |
| W8-14f | `Menus/{NativePopupMenuHost 149, ShellMainWindow.Menus 92}` = 241 | `shell_menus::{popup_host, main_window}` | 🟡 | Sonnet | N | 14e, 13h | B4 |
| W8-14g | `Menus/OwnerDrawMenuSession` 336 (owner-draw GDI, dark palette) | `shell_menus::owner_draw` | 🔴 | Sonnet | N | 14e, 15b | B4 |

### 15 Overlay (`pr_app::overlay` pure element tree; `pr_platform_win::shell_overlay` Win32 parts)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-15a | `Overlay/{OverlayContracts 42, OverlayModel 84, OverlayElement 185, LayerElement 102}` = 413 | `overlay::{contracts, model, element, layer}` | 🟢 | Haiku | F | 12a | B1 |
| W8-15b | `Overlay/{Animator 198, FrameDelayTimer 84, DarkPalette 98}` = 380 | `overlay::{animator, frame_delay, palette}` (clock injected) | 🟢 | Haiku | G | 05a | B1 |
| W8-15c | `Overlay/{ButtonElement 177, TextElement 114}` = 291 | `overlay::{button, text}` | 🟢 | Haiku | F | 15a, 15b | B1 |
| W8-15d | `Overlay/{PanelElement 166, CapturePairBadge 38, FolderInfoPanel 39}` = 243 | `overlay::{panel, capture_badge, folder_info}` | 🟢 | Haiku | F | 15c | B1 |
| W8-15e | `Overlay/{StatusPanel 92, ToolbarPanel 117, ToolsMenu 74, ZoomIndicatorPanel 38}` = 321 | `overlay::{status, toolbar, tools_menu, zoom_indicator}` | 🟢 | Haiku | F | 15d, 06b | B1 |
| W8-15f | `Overlay/{OverlayHost 101, OverlayMessageHandler 111, OverlayNative 134, ShellOverlayServices 19}` = 365 | `shell_overlay::{host, messages}` | 🟡 | Sonnet | F+N | 15a, 11b | B4 |
| W8-15g | `Overlay/{NativeTooltipHost 191, DeferredTooltips 79}` = 270 | `shell_overlay::tooltips` | 🟡 | Sonnet | N | 11c, 15b | B4 |
| W8-15h | `Overlay/ShellOverlay` lines 1 to 338 | `shell_overlay::overlay::{layout, hit_test}` | 🔴 | Sonnet | F | 15e, 15f, 12e | B4 |
| W8-15i | `Overlay/ShellOverlay` lines 338 to 675 | `shell_overlay::overlay::{input, paint}` | 🔴 | Sonnet | F+N | 15h, 15g | B4 |
| W8-15j | `Overlay/ShellMainWindow.Overlay` 284 | `shell_overlay::main_window` | 🟡 | Sonnet | F | 15i, 13h | B4 |

### 16 Viewing (`pr_platform_win::shell_viewing`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-16a | `Viewing/{ImageLayer 54, ScalingPolicy 46, FrameLog 70, FrameStats 104, FramePacer 108}` = 382 | `shell_viewing::{image_layer, scaling_policy, frame_stats, pacer}`; `FrameLog` output format is persisted by perf tooling (`pr_perf`) | 🟢 | Haiku | G | 12a | B1 |
| W8-16b | `Viewing/{CompareLayer 166, CrossfadeLayer 165}` = 331 | `shell_viewing::{compare, crossfade}` | 🔴 | Sonnet | F+N | 16a, 12e, 06c | B4 |
| W8-16c | `Viewing/PresentationComposer` 354 | `shell_viewing::composer` | 🔴 | Sonnet | F | 16b, 16a | B4 |
| W8-16d | `Viewing/{PresentationPrefetcher 181, KeepWarm 138, ShellMainWindow.KeepWarm 39, ShellViewingRegistration 24}` = 382 | `shell_viewing::{prefetcher, keep_warm}` | 🟡 | Sonnet | F | 12h, 16c | B4 |
| W8-16e | `Viewing/{ScrollBarGeometry 255, ScrollBarNative 69}` = 324 | `shell_viewing::scrollbar_geometry` (pure), thin native wrapper | 🟢 | Haiku | G | 01a | B1 |
| W8-16f | `Viewing/ScrollBars` lines 1 to 245 | `shell_viewing::scrollbars::{state, layout}` | 🟡 | Sonnet | F | 16e, 11c | B4 |
| W8-16g | `Viewing/ScrollBars` lines 245 to 490 | `shell_viewing::scrollbars::{input, paint}` | 🟡 | Sonnet | F+N | 16f | B4 |
| W8-16h | `Viewing/ViewportController` 283 | `shell_viewing::viewport_controller` | 🔴 | Sonnet | F | 01b, 16e, 05h | B4 |
| W8-16i | `Viewing/Win32PresentationSink 103`, `Startup/ShellPresentationSink 89` = 192; WPF `WpfPresentationSink 116`, `WpfPresentationSinkFactory 23` retired | `shell_viewing::presentation_sink` | 🟡 | Sonnet | F | 16c, 06d | B4 |

### 17 Startup and process lifecycle (`pr_platform_win::shell_startup`, logic in `pr_app::startup`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-17a | `Startup/{DpiAwarenessBootstrap 50, ShellLaunch 49, ShellStartupContext 8, ForwardedOpenHandler 73, ShellErrorLog 117}` = 297 | `startup::{dpi, launch, forwarded_open, error_log}` (C-18, C-19 with the 750 ms coalescer, P-15) | 🟡 | Sonnet | F | `pr_platform_win::forward_*`, 04d | B6 |
| W8-17b | `Startup/{ShellPlacement 130, StartupCloak 100}` = 230; WPF `StartupWindowReveal 104`, `WindowPlacementService 260` retired | `shell_startup::{placement, cloak}` (DWM cloak until first Present) | 🟡 | Sonnet | F+N | 03a, 13k | B6 |
| W8-17c | `Startup/{ShellWarmup 84, ShellCompositionRoot 109}` = 193; WPF `StartupWarmup 46` retired | `shell_startup::{warmup, composition_root}` | 🟡 | Sonnet | F | 09b | B6 |
| W8-17d | `Startup/ShellStartup` lines 1 to 343 (options and seams, single instance, settings load/repair C-28) | `shell_startup::startup::{options, instance, settings}` | 🔴 | Sonnet | F | 17a, 17b, 17c, `pr_platform_win::instance_*` | B6 |
| W8-17e | `Startup/ShellStartup` lines 343 to 685 (recovery prompt C-27, open path, crash handling, shutdown, W-11 boxes) | `shell_startup::startup::{recovery, run, shutdown}` | 🔴 | Sonnet | F+N | 17d, 13i, 15j, 08e | B6 |

### 18 Dialogs (`pr_platform_win::dialogs`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-18a | `Dialogs/{NativeShellFileDialog 181, NativeShellFolderDialog 96, FolderPicker 44}` = 321; WPF `WpfFolderPicker 23` retired | `dialogs::{file_dialog, folder_dialog}` (IFileOpenDialog via windows-rs) | 🟡 | Sonnet | N | 11e | B5 |
| W8-18b | `Dialogs/{TaskDialogs 114, Win32DialogService 144, Win32ZoomPromptService 31, SecondaryWindowPortHost 19}` = 308; WPF `WpfDialogService 106`, `IDialogHost 23`, `WpfZoomPromptService 16`, `SecondaryWindowFallback 117` retired | `dialogs::{task_dialog, dialog_service, zoom_prompt_service}` | 🟡 | Sonnet | F | 11b, 04c | B5 |
| W8-18c | `Dialogs/Win32ClipboardService` 116; WPF `WpfClipboardService 23` retired | `dialogs::clipboard` | 🟡 | Sonnet | N | 11b | B5 |
| W8-18d | `Native/{DialogTemplate 127, NativeDialogChrome 156}` = 283 | `dialogs::native::{template, chrome}` | 🟡 | Sonnet | F | 11d | B5 |
| W8-18e | `Native/{EditTextCentering 167, ListDialog 183}` = 350 | `dialogs::native::{edit_centering, list_dialog}` | 🟡 | Sonnet | N | 18d | B5 |
| W8-18f | `Native/NativeControls` lines 1 to 276 | `dialogs::native::controls::{basic}` | 🟡 | Sonnet | N | 18d, 11c | B5 |
| W8-18g | `Native/NativeControls` lines 276 to 552 | `dialogs::native::controls::{list_tree, custom}` | 🟡 | Sonnet | N | 18f | B5 |
| W8-18h | `Native/NativeDialog` lines 1 to 365 (class, create, modal loop) | `dialogs::native::dialog::{core, modal}` | 🔴 | Sonnet | N | 18d, 13c | B5 |
| W8-18i | `Native/NativeDialog` lines 365 to 730 (layout, DPI, dark mode) | `dialogs::native::dialog::{layout, dpi}` | 🔴 | Sonnet | F+N | 18h | B5 |
| W8-18j | `Native/NativeDialog` lines 730 to 1095 (focus, keyboard, control events) | `dialogs::native::dialog::{focus, events}` | 🔴 | Sonnet | N | 18i, 18f | B5 |
| W8-18k | `Native/NativeDialog` lines 1095 to 1459 (owner-draw, close, result) | `dialogs::native::dialog::{draw, result}` | 🔴 | Sonnet | N | 18j | B5 |
| W8-18l | `Native/NativeDialogChromePainter` lines 1 to 330 (geometry, colours) | `dialogs::native::painter::geometry` | 🟡 | Sonnet | G+N | 18d, 15b | B5 |
| W8-18m | `Native/NativeDialogChromePainter` lines 330 to 660 | `dialogs::native::painter::controls` | 🟡 | Sonnet | N | 18l | B5 |
| W8-18n | `Native/NativeDialogChromePainter` lines 660 to 991 | `dialogs::native::painter::{title_bar, frame}` | 🟡 | Sonnet | N | 18m | B5 |
| W8-18o | `Native/Win32SecondaryWindowPort` 374 (router: which dialog opens which window) | `dialogs::secondary_window_port` | 🟡 | Sonnet | F | 18k, 10c | B5 |
| W8-18p | `Native/{ZoomPromptDialog 134, DiagnosticsDialog 205}` = 339 | `dialogs::{zoom_prompt, diagnostics}` | 🟢 | Haiku | F+N | 18k, 10c | B5 |
| W8-18q | `Native/ActionProfilesDialog` 414 | `dialogs::action_profiles` | 🟡 | Sonnet | F+N | 18k, 10a | B5 |
| W8-18r | `Native/BenchmarkDialog` lines 1 to 330 | `dialogs::benchmark::{layout, state}` | 🟡 | Sonnet | F | 18k, 10i | B5 |
| W8-18s | `Native/BenchmarkDialog` lines 330 to 661 | `dialogs::benchmark::{run, results}` | 🟡 | Sonnet | F+N | 18r | B5 |
| W8-18t | `Native/RecoveryDialog` lines 1 to 330 | `dialogs::recovery::{layout, list}` | 🟡 | Sonnet | F | 18k, 10g | B5 |
| W8-18u | `Native/RecoveryDialog` lines 330 to 660 | `dialogs::recovery::{path_panel, actions}` | 🟡 | Sonnet | F | 18t | B5 |
| W8-18v | `Native/RecoveryDialog` lines 660 to 986 | `dialogs::recovery::{restore, dismiss}` (INV-5 UI side) | 🟡 | Sonnet | F+N | 18u | B5 |

### 19 Settings UI (`pr_platform_win::dialogs::settings`)
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-19a | `SettingsUi/{ISettingsPageView 100, SettingsPageCatalog 56, SettingsPageContext 166}` = 322 | `dialogs::settings::{page_view, catalog, context}` (catalog lists the pages in C# order) | 🟢 | Haiku | F | 10b, 10c | B5 |
| W8-19b | `SettingsUi/SettingsPageBuilder` lines 1 to 322 | `dialogs::settings::builder::{rows, groups}` | 🟡 | Sonnet | F+N | 19a, 18g | B5 |
| W8-19c | `SettingsUi/SettingsPageBuilder` lines 322 to 645 | `dialogs::settings::builder::{inputs, layout}` | 🟡 | Sonnet | F+N | 19b | B5 |
| W8-19d | `Pages/{DiagnosticsSettingsPage 41, PerformanceSettingsPage 164}` = 205 | `dialogs::settings::pages::{diagnostics, performance}` | 🟢 | Haiku | F | 19c | B5 |
| W8-19e | `Pages/DisplaySettingsPage` 247 | `...::display` | 🟢 | Haiku | F | 19c | B5 |
| W8-19f | `Pages/FilesSettingsPage` 223 | `...::files` | 🟢 | Haiku | F | 19c | B5 |
| W8-19g | `Pages/GeneralSettingsPage` 305 | `...::general` | 🟢 | Haiku | F | 19c | B5 |
| W8-19h | `Pages/MouseSettingsPage` 268 | `...::mouse` | 🟢 | Haiku | F | 19c | B5 |
| W8-19i | `Pages/ShortcutsSettingsPage` 323 (key capture) | `...::shortcuts` | 🟡 | Sonnet | F+N | 19c, 05c, 10e | B5 |
| W8-19j | `SettingsUi/SettingsDialog` lines 1 to 378 (frame, page list, navigation) | `dialogs::settings::dialog::{frame, nav}` | 🟡 | Sonnet | N | 19c, 18k | B5 |
| W8-19k | `SettingsUi/SettingsDialog` lines 378 to 756 | `dialogs::settings::dialog::{state, dirty}` | 🟡 | Sonnet | F+N | 19j, 10e | B5 |
| W8-19l | `SettingsUi/SettingsDialog` lines 756 to 1134 | `dialogs::settings::dialog::{apply, validate}` | 🟡 | Sonnet | F+N | 19k | B5 |
| W8-19m | `SettingsUi/SettingsDialog` lines 1134 to 1512 | `dialogs::settings::dialog::{footer, close}` | 🟡 | Sonnet | N | 19l, 19d..19i | B5 |

### 20 Integration (new, no 1:1 C# source)
| WU | Content | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-20a | Rust `Program.cs` + `main.rs`: parse args, DPI bootstrap, run `shell_startup`, exit codes (startup failure = 1) | `crates/pr-app/src/main.rs` | 🟡 | Sonnet | N | 17e, 13i | B6 |
| W8-20b | UI parity smoke: hidden-window harness driving open folder, next/prev, zoom, recycle with fakes, plus the C-*/K-*/W-* rows of `PARITY.md` mapped to tests; remove the `ui-spike` feature/bin when the real window reproduces its measurements | `crates/pr-app/tests/ui_smoke.rs`, `PARITY.md` | 🟡 | Sonnet | N | everything | B6 |

## Counts
Sub-units: 01 4, 02 2, 03 2, 04 5, 05 8, 06 6, 07 11, 08 5, 09 2, 10 9, 11 8, 12 10, 13 11, 14 7, 15 10, 16 9, 17 5, 18 22, 19 13, 20 2 =
**151 sub-units**. By model: Haiku 37 (🟢), Sonnet 114 (🟡 and 🔴). Native-only (**N**) or with a native twin (**F+N**, **G+N**): 60.
Pure and covered by golden/oracle (**G**, **G+N**, **G+F**, constants): 24 units. Pure with fakes only (**F**): 68 units.

## Retired C# (not ported) and its replacement
| C# | Lines | Replaced by |
|---|---|---|
| `App/App.xaml.cs`, `MainWindow.xaml.cs`, `AssemblyInfo` | 1 530 | 17d/17e (startup), 13g..13k (window), 20a |
| `App/WindowPlacementService`, `Coordinators/FullscreenWindowPlacer` | 323 | 03a, 03b, 13k, 17b |
| `App/Services/{WpfImageSurface, WpfPresentationSink(+Factory), WpfInputAdapters, WpfDialogService, IDialogHost, WpfFolderPicker, WpfClipboardService, WpfZoomPromptService}` | 464 | 14c, 16i, 14a, 18a, 18b, 18c |
| `App/Services/{DispatcherUiScheduler, AmbientDispatcher}`, `Diagnostics/{PerfDispatcherHooks, PhotoReviewPerf}`, `Composition/AppHost` | 200 | 13d (UI dispatcher), 09b. Dispatcher long-op perf hook is dropped (no WPF dispatcher); log as a `Different` row |
| `App/Services/{StartupWarmup, StartupWindowReveal}` | 150 | 17b, 17c |
| `Dialogs/SecondaryWindowFallback`, `Hosting/Win32UiSynchronizationContext`, `Hosting/PendingInterop/*`, `App.Shared/Services/WpfKeyNameValidator` | 308 | not needed in Rust (WPF fallback, CLR sync context, P/Invoke, WPF key names) |
| `Shell.Interop/Graphics/{D2D1, D3D11, Dxgi, DWrite}` COM vtable declarations | 1 186 | `windows` crate; units 11g/11h keep an audit table |

## Open design points the contract writers must settle (all with a recommended default)
- **D-W8-1 Change notification.** C# view models are `ObservableObject`s raising `PropertyChanged`. Rust default: each mutating method
  returns a `ChangeSet` (bit set of changed properties) and the shell repaints from it; no observer lists, no `Rc<RefCell>` graphs.
- **D-W8-2 Async.** C# `async Task` coordinators become sync code on the UI thread that submit work to the existing worker pool and are
  completed through `UiDispatcher::post`; a generation token (INV-9) is checked on every completion. No async runtime is added.
- **D-W8-3 Render contracts live in `pr_app`** (traits with no `windows` types), implementations in `pr_platform_win::render`, so layout and
  composition logic (`PresentationComposer`, overlay) is testable with a recording fake surface.
- **D-W8-4 Localization.** UI text comes from `pr_l10n` only; units must not embed Vietnamese or English literals except tests.
- **D-W8-5 Persisted files** (window placement, session) keep the C# format, under `%LOCALAPPDATA%\PhotoReviewRs` (R0002).

## Risks
- **R0001 gate is open.** Windowed p95 shows 300 to 510 ms `EndDraw` stalls on the unattended laptop; unit 12d/12j are where a fix would land.
  Wave 8 proceeds on P-06; an attended re-measure before W8-12d is cheap and decides whether 12j runs.
- **Native tests need a desktop.** Units marked N run `#[ignore]` locally and on `windows-latest`; the lead must run `--include-ignored`
  once on an interactive session before marking any N unit Done in PARITY.md.
- **Oracle cost.** Goldens for batch B1 need new `tools/oracle-cs` cases for App.Shared classes (many are `internal`); the golden
  generator needs `InternalsVisibleTo` or reflection in the oracle project, never edits to the C# repo.
- **Big files split across units share one Rust module:** contracts must pre-declare the `mod` files (P-18 rule) so two agents never edit the same file.

## Batches (contract writing order; ports start per P-28 as soon as direct dependencies merge)
| Batch | Units | Why this order |
|---|---|---|
| **B1 Pure leaves and contracts** | 01a..d, 02a..b, 03a..b, 04a..e, 05a..f, 06a, 06b, 06d, 10a..c, 10f, 10h, 12a, 15a..e, 16a, 16e (35 units) | No dependency on unwritten code; goldens from the oracle; Haiku-heavy; unblocks everything else |
| **B2 Coordinators, view models, presenters** | 05g..h, 06c, 06e..f, 07a..k, 08a..e, 09a, 10d..e, 10g, 10i (26 units) | Pure logic with fakes; INV-1/3..6/9 live here; needs B1 traits and `pr_imaging`/`pr_core` |
| **B3 Win32 foundation and rendering** | 11a..h, 12b..j, 13a..f (23 units) | `unsafe` core: windows-rs audit, Direct2D device/surface, message loop, UI dispatcher; independent of B2, can be written in parallel with it (one workflow at a time per P-29, so right after B2) |
| **B4 Main window, input, menus, overlay, viewing** | 13g..k, 14a..g, 15f..j, 16b..d, 16f..i (24 units) | Needs the host (B3) and the traits of B1/B2 |
| **B5 Dialogs and settings UI** | 18a..v, 19a..m (35 units) | Needs the native dialog base (11c/11d/13c) and presenters (B2); 19d..19h are the cheapest Haiku units of the wave |
| **B6 Startup, composition, integration** | 09b, 17a..e, 20a..b (8 units) | Wires everything; last by dependency; 20b closes the wave |

Check: 35 + 26 + 23 + 24 + 35 + 8 = 151. Reading rule: a unit in batch Bn may start before Bn-1 is fully merged if its `Depends on` units are
merged (P-28); the batch only fixes the order in which contracts (stubs, goldens, `Depends on`) are written.

## Per-unit gate
Contract (stub, goldens/fakes, `Depends on`), red tests (Haiku), port, adversarial verifier (Sonnet), time-boxed mutation on pure files only
(about 15 min, P-34; skipped for native and trait-only units). One PR for the wave on `integration/wave-8`; the lead owns `PARITY.md`.
PARITY rows to update: C-*, K-*, W-*, P-08, P-15 (UI side). Native units stay `Partial` until a desktop run is recorded.
