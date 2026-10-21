//! Port of `PhotoReview.Shell.Win32/Input/ShellMainWindow.Input.cs` (293 lines, baseline `d92d6aae`). Unit W8-14c. Contract: `contracts/W8-14c.md`.
//!
//! The input part of the main window (WP-22): `RegisterInput`, the `IShellInputHost` implementation (the `MainViewModel` calls behind the
//! commands), the drop target registration, and the zoom-menu helpers (`ApplyZoomMenuSelectionAsync`, `ApplyClickZoomLevelAsync`,
//! `OpenClickZoomCustomDialogAsync`, `RefreshView`, `ToggleKeepZoomAcrossImages`).
//!
//! The main window itself is `host::main_window` (W8-13g..13k, written concurrently), so this unit takes what it needs from it as the trait
//! [`MainWindowInputServices`] (D-W8B4I-6): 13h implements it for its window, tests implement it with a fake. A partial class cannot be ported
//! as such; the Rust unit is a struct, [`MainWindowInput`], that the window creates in its own `register_features` and owns.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::input::primitives::PointD;
use pr_app::services::contracts::ZoomPromptService;
use pr_app::viewmodels::main::{ChangeSet, MainViewModel, SettingsSource, ViewerHandle};
use pr_core::abstractions::{Log, UiDispatcher};

use super::dispatch::{ShellInput, ShellInputHost};
use super::drop_target::ShellDropTarget;
use super::environment::ShellInputEnvironment;
use super::surface::ViewportControllerApi;
use crate::host::contracts::ShellWindowApi;

/// What the input wiring needs from the main window (W8-13h). Everything runs on the UI thread; `&self` because the window is re-entered.
pub trait MainWindowInputServices {
    /// C# `_window`.
    fn window(&self) -> Rc<dyn ShellWindowApi>;
    /// C# `_viewport` (the controller, as the surface needs it).
    fn viewport(&self) -> Rc<dyn ViewportControllerApi>;
    /// C# `_viewModel.Viewer`.
    fn viewer(&self) -> ViewerHandle;
    /// C# `_options.SettingsStore` + `_viewModel.Settings`: `current()` for reads, `begin_edit`/`commit_edit` for the saves (R2-F-27).
    fn settings(&self) -> SettingsSource;
    /// C# `_options.ZoomPrompt` (null when the window has no prompt).
    fn zoom_prompt(&self) -> Option<Rc<dyn ZoomPromptService>>;
    /// C# `_log`.
    fn log(&self) -> Arc<dyn Log>;
    /// The UI dispatcher (`MainWindow::dispatcher()`), see `ShellInputDeps::dispatcher`.
    fn dispatcher(&self) -> Arc<dyn UiDispatcher>;
    /// C# `_options.InputEnvironment` (default: the real Win32 reads); the composition root puts it into `MainWindowOptions::extensions`.
    fn input_environment(&self) -> Rc<ShellInputEnvironment>;
    /// `Stopwatch.Frequency / 1000.0` (`pr_core::diagnostics::perf::STOPWATCH_FREQUENCY`).
    fn ticks_per_ms(&self) -> f64;
    /// Runs `f` with the view model, then applies the `ChangeSet` it returns (repaint, events), exactly like the other callers of the view model.
    /// While `f` runs the window holds its view-model borrow; `f` must not call back into the window (D-W8B4I-1).
    fn with_view_model(&self, f: &mut dyn FnMut(&mut MainViewModel) -> ChangeSet);
    /// C# `ReportDropFailure` / `ReportActionFailed` status line: `StatusText = ...`. Applied through `with_view_model` by the implementer.
    fn show_status(&self, text: &str);
    /// C# `StatusFormatter.FolderOpenFailed(UserFacingError.Describe(exception))` for the drop failure.
    fn folder_open_failed_text(&self, error: &dyn std::error::Error) -> String;
    /// C# `StatusFormatter.ActionFailed(actionName, UserFacingError.Describe(exception))`.
    fn action_failed_text(&self, action_name: &str, error: &dyn std::error::Error) -> String;
    /// C# `Tr.*` action names used by the guarded commands (localized at call time).
    fn action_names(&self) -> InputActionNames;
    /// C# `_window.Close()`.
    fn close_window(&self);
    /// C# `_viewport.InvalidateLayout()` + `_window.Invalidate()` of `RefreshView`.
    fn invalidate_layout_and_window(&self);
}

/// The `Tr.*` strings `ShellMainWindow.Input.cs` passes to `RunGuardedAsync` (the name in the "action failed" status).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputActionNames {
    /// `Tr.MainMenuNextFolder`.
    pub next_folder: String,
    /// `Tr.MainMenuPreviousFolder`.
    pub previous_folder: String,
    /// `Tr.ActionMoveToFolderName`.
    pub move_to_folder: String,
    /// `Tr.ActionCopyToFolderName`.
    pub copy_to_folder: String,
    /// `Tr.MainMenuOpenFolder` trimmed of `'…'`, `'.'`, `' '`.
    pub open_folder: String,
    /// `Tr.MainMenuZoom`.
    pub zoom: String,
}

/// The two hooks C# `CreateViewportOptions` puts into `ViewportControllerOptions` (W8-16h builds the options from them).
pub struct ViewportInputHooks {
    /// C# `DragMetricsPixels`: `SM_CXDRAG/SM_CYDRAG` for the window's CURRENT dpi (read at each call, so a monitor change stays right).
    pub drag_metrics_pixels: Box<dyn Fn() -> (i32, i32)>,
    /// C# `CursorScreenPixel`: the real cursor position (`GetCursorPos`).
    pub cursor_screen_pixel: Box<dyn Fn() -> Option<PointD>>,
}

impl fmt::Debug for ViewportInputHooks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewportInputHooks").finish_non_exhaustive()
    }
}

/// C# `CreateViewportOptions(window, displayClock)` without the display clock (16h owns `ViewportControllerOptions`).
pub fn create_viewport_input_hooks(window: Rc<dyn ShellWindowApi>) -> ViewportInputHooks {
    todo!()
}

/// The input wiring of one main window (C# `ShellMainWindow` fields `_input`, `_dropTarget` and the `RegisterInput` body).
pub struct MainWindowInput {
    services: Rc<dyn MainWindowInputServices>,
    input: ShellInput,
    drop_target: std::cell::RefCell<Option<ShellDropTarget>>,
}

impl fmt::Debug for MainWindowInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWindowInput").finish_non_exhaustive()
    }
}

impl MainWindowInput {
    /// C# `RegisterInput()`: builds the surface and `ShellInput` (host = this wiring), then registers the drop target on the window handle. A failed
    /// `RegisterDragDrop` logs the warning "Drag and drop is unavailable (RegisterDragDrop failed)." and goes on. The caller (W8-13h) must register
    /// the scroll bars first and the context menu after (handler order), and forward the window's `Closed` event to [`MainWindowInput::on_window_closed`].
    pub fn register(services: Rc<dyn MainWindowInputServices>) -> MainWindowInput {
        todo!()
    }

    /// C# `Input`.
    pub fn input(&self) -> &ShellInput {
        todo!()
    }

    /// The host the wiring gave to `ShellInput` (also what the Tools/overlay buttons use).
    pub fn input_host(&self) -> Rc<dyn ShellInputHost> {
        todo!()
    }

    /// C# `_onViewModelChangedForInput` + the viewer's `ZoomModeChanged`: `CurrentIndex` changed gives `note_current_index_changed(index)`; a zoom or
    /// stretch bit of `changes.viewer` gives `on_zoom_mode_changed()`. The window calls this with every `ChangeSet` it applies.
    pub fn on_view_model_changes(&self, changes: &ChangeSet) {
        todo!()
    }

    /// C# `_onSettingsChangedForInput`: `_viewModel.Settings = settings; _input.RebuildShortcuts(settings)`. Optional: `ShellInput` also rebuilds
    /// lazily when it sees a different settings object at a key press (D-W8B4I-7).
    pub fn on_settings_changed(&self, settings: &Arc<pr_core::settings::app_settings::AppSettings>) {
        todo!()
    }

    /// C# `OnWindowClosedForInput`: revokes the drop target (`Dispose`), disposes the input (stops the glide, ends a pan). Idempotent.
    pub fn on_window_closed(&self) {
        todo!()
    }

    /// C# `DropTarget` (tests call `Drop` with a fake data object); `None` after the window closed or when registration failed.
    pub fn drop_target_registered(&self) -> bool {
        todo!()
    }

    /// C# `RefreshView()`: `StopKinetic`, `RefreshView()` of the view model, `InvalidateLayout`, `Invalidate`; zoom and offsets unchanged.
    pub fn refresh_view(&self) {
        todo!()
    }

    /// C# `OpenClickZoomCustomDialogAsync()`: asks the zoom prompt with the current click-zoom percent and applies the answer
    /// (`apply_zoom_menu_selection`); a cancelled prompt or no prompt does nothing.
    pub fn open_click_zoom_custom_dialog(&self) {
        todo!()
    }

    /// C# `ApplyZoomMenuSelectionAsync(percent)`: with `SetZoomAlsoSetsClickLevel` it saves and applies the level (`apply_click_zoom_level`), else only
    /// zooms (`Input.set_click_zoom_level(percent)`).
    pub fn apply_zoom_menu_selection(&self, percent: i32) {
        todo!()
    }

    /// C# `ApplyClickZoomLevelAsync(percent)`: saves `ClickZoomPercent`, then zooms even when the save failed (this session still zooms).
    pub fn apply_click_zoom_level(&self, percent: i32) {
        todo!()
    }

    /// C# `ToggleKeepZoomAcrossImages()` (`ReviewCommandType::ToggleKeepZoom`): flips `KeepZoomAcrossImages` and saves it.
    pub fn toggle_keep_zoom_across_images(&self) {
        todo!()
    }
}
