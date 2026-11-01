//! W9-E9 editor forwarding ports the corresponding C# source at baseline `7eaee2a2`.
//! Port of `PhotoReview.Shell.Win32/Input/ShellMainWindow.Input.cs` (293 lines, baseline `d92d6aae`). Unit W8-14c. Contract: `contracts/W8-14c.md`.
//!
//! The input part of the main window (WP-22): `RegisterInput`, the `IShellInputHost` implementation (the `MainViewModel` calls behind the
//! commands), the drop target registration, and the zoom-menu helpers (`ApplyZoomMenuSelectionAsync`, `ApplyClickZoomLevelAsync`,
//! `OpenClickZoomCustomDialogAsync`, `RefreshView`, `ToggleKeepZoomAcrossImages`).
//!
//! The main window itself is `host::main_window` (W8-13g..13k, written concurrently), so this unit takes what it needs from it as the trait
//! [`MainWindowInputServices`] (D-W8B4I-6): 13h implements it for its window, tests implement it with a fake. A partial class cannot be ported
//! as such; the Rust unit is a struct, [`MainWindowInput`], that the window creates in its own `register_features` and owns.
use std::cell::{Cell, RefCell};
use std::error::Error;
use std::fmt;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pr_app::input::primitives::PointD;
use pr_app::input::review_command::{ReviewCommand, ReviewCommandType};
use pr_app::services::contracts::ZoomPromptService;
use pr_app::viewmodels::main::{ChangeSet, MainViewModel, PresentationBridge, SettingsSource, ViewerHandle};
use pr_app::viewmodels::viewer_state::ViewerProperty;
use pr_core::abstractions::{DialogService, FileSystem, Log, UiDispatcher};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::settings::app_settings::AppSettings;
use pr_core::text::eq_ignore_case;

use super::dispatch::{InputSurface, ShellInput, ShellInputDeps, ShellInputHost};
use super::drop_target::{DropCallbacks, ShellDropTarget};
use super::environment::ShellInputEnvironment;
use super::native;
use super::surface::{ViewportControllerApi, ViewportImageSurface};
use crate::host::contracts::{EventHandlerId, ShellWindowApi, ShellWindowEvent};
use crate::host::main_window::core::{MainWindowFeature, MainWindowId, MainWindowInner, with_window};

/// What the input wiring needs from the main window (W8-13h). Everything runs on the UI thread; `&self` because the window is re-entered.
pub trait MainWindowInputServices {
    /// C# `_window`.
    fn window(&self) -> Rc<dyn ShellWindowApi>;
    /// C# `_viewport` (the controller, as the surface needs it).
    fn viewport(&self) -> Rc<dyn ViewportControllerApi>;
    /// C# `_viewModel.Viewer`.
    fn viewer(&self) -> ViewerHandle;
    /// The view model's presentation bridge, whose `SourceSizeSwapping` hook `register` points at `ShellInput::on_source_size_swapping` (C# `PointerInputController`
    /// constructor: `_viewer.SourceSizeSwapping += OnSourceSizeSwapping`). `None` (the default) installs nothing.
    fn presentation_bridge(&self) -> Option<PresentationBridge> {
        None
    }
    /// C# `_options.SettingsStore` + `_viewModel.Settings`: `current()` for reads, `begin_edit`/`commit_edit` for the saves (R2-F-27).
    fn settings(&self) -> SettingsSource;
    /// C# `_options.ZoomPrompt` (null when the window has no prompt).
    fn zoom_prompt(&self) -> Option<Rc<dyn ZoomPromptService>>;
    /// C# `_log`.
    fn log(&self) -> Arc<dyn Log>;
    /// W9-R8: the dialog service that shows the rename box (C# `MainViewModel._dialogService`); `None` = no dialog, so F2 / "Rename..." does nothing (C# returns null).
    fn dialog_service(&self) -> Option<Rc<dyn DialogService>> {
        None
    }
    /// W9-R8: the file system the confirming call of the rename box asks (C# `MainViewModel._fileSystem`, one `FileExists`). Default: the physical one.
    fn file_system(&self) -> Arc<dyn FileSystem> {
        Arc::new(PhysicalFileSystem)
    }
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

/// C# `CreateViewportOptions(window, displayClock)` without the display clock (16h owns `ViewportControllerOptions`). T6: the DPI is read from the window at
/// EVERY call, never captured.
pub fn create_viewport_input_hooks(window: Rc<dyn ShellWindowApi>) -> ViewportInputHooks {
    ViewportInputHooks {
        drag_metrics_pixels: Box::new(move || native::drag_metrics_pixels(native::dpi_of(window.dpi_scale()))),
        cursor_screen_pixel: Box::new(native::cursor_screen_pixel),
    }
}

/// An error reduced to its text, so that it can go to `Log::error` (which wants `'static`) from a `&dyn Error` of any lifetime.
#[derive(Debug)]
struct LoggedError(String);

impl fmt::Display for LoggedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for LoggedError {}

/// The `IShellInputHost` of C# `ShellMainWindow` (rule 5): every member forwards to the view model through the services.
struct MainWindowInputHost {
    services: Rc<dyn MainWindowInputServices>,
    owner: Weak<MainWindowInput>,
}

impl MainWindowInputHost {
    /// Reads something from the view model (the closure returns an empty `ChangeSet`).
    fn query<R>(&self, read: impl Fn(&mut MainViewModel) -> R) -> Option<R> {
        let mut result = None;
        self.services.with_view_model(&mut |vm| {
            result = Some(read(vm));
            ChangeSet::default()
        });
        result
    }

    /// Runs one view-model call whose `ChangeSet` the window applies.
    fn call(&self, run: impl Fn(&mut MainViewModel) -> ChangeSet) {
        self.services.with_view_model(&mut |vm| run(vm));
    }

    /// C# `PromptRenameCurrentAsync` (F2 and the "Rename..." menu item share this branch): the view model prepares the prompt and releases its borrow; the modal box
    /// runs with NO borrow held (its validate closure owns a snapshot of the listing); the answer goes back to the view model, which renames only when the same
    /// photo is still selected. Without a dialog service, or when cancelled, nothing is renamed.
    fn rename_file(&self) {
        let mut prepared = None;
        let mut listing = None;
        self.services.with_view_model(&mut |vm| {
            let changes = vm.prompt_rename_current();
            prepared = vm.take_rename_prompt();
            listing = Some(vm.catalog().paths());
            changes
        });
        let (Some(prepared), Some(listing)) = (prepared, listing) else {
            return;
        };
        let Some(dialogs) = self.services.dialog_service() else {
            self.call(|vm| vm.finish_rename_prompt(prepared.source(), None));
            return;
        };
        // Typing asks the listing only (no I/O); the confirming call also asks the disk, which covers files the listing does not know.
        let listed: Arc<dyn Fn(&str) -> bool> =
            Arc::new(move |full_path| listing.iter().any(|path| eq_ignore_case(path, full_path)));
        let typed = match prepared.request(listed, self.services.file_system()) {
            Ok(request) => dialogs.prompt_rename(&request),
            Err(_) => None,
        };
        self.call(|vm| vm.finish_rename_prompt(prepared.source(), typed.as_deref()));
    }

    /// C# `RunGuardedAsync`: an `Err` is reported once and swallowed.
    fn guarded(&self, name: &str, run: impl FnOnce() -> Result<(), Box<dyn Error>>) {
        if let Err(error) = run() {
            self.report_action_failed(name, error.as_ref());
        }
    }

    /// `view_model.navigate_sibling_folder(direction)`; `Err` is the failure of the action.
    fn navigate_sibling(&self, direction: i32) -> Result<(), Box<dyn Error>> {
        let mut failure: Option<Box<dyn Error>> = None;
        self.services
            .with_view_model(&mut |vm| match vm.navigate_sibling_folder(direction) {
                Ok(changes) => changes,
                Err(error) => {
                    failure = Some(Box::new(error));
                    ChangeSet::default()
                }
            });
        failure.map_or(Ok(()), Err)
    }

    /// The name of the profile of `RunAction` (`""` for an index outside the list).
    fn action_profile_name(&self, index: i32) -> String {
        let settings = self.services.settings().current();
        usize::try_from(index)
            .ok()
            .and_then(|index| settings.actions.get(index))
            .map(|action| action.name.clone())
            .unwrap_or_default()
    }
}

impl ShellInputHost for MainWindowInputHost {
    fn has_images(&self) -> bool {
        self.query(|vm| vm.has_images()).unwrap_or(false)
    }

    fn is_fullscreen(&self) -> bool {
        self.services.viewer().borrow().is_fullscreen()
    }

    fn is_compare_visible(&self) -> bool {
        self.query(|vm| vm.compare().is_visible()).unwrap_or(false)
    }

    fn current_has_compare_pair(&self) -> bool {
        self.query(|vm| vm.current_has_compare_pair()).unwrap_or(false)
    }

    fn current_has_capture_pair(&self) -> bool {
        self.query(|vm| vm.current_has_capture_pair()).unwrap_or(false)
    }

    fn next(&self) {
        self.call(|vm| vm.next());
    }

    fn previous(&self) {
        self.call(|vm| vm.previous());
    }

    fn cancel_duplicate_check(&self) -> bool {
        self.query(|vm| vm.cancel_duplicate_check()).unwrap_or(false)
    }

    /// The viewer's `ZoomModeChanged` for a change made by the input: forwards the effective zoom to the presenter (zoom detail loader). `with_view_model`
    /// uses `try_borrow_mut`, so a call that arrives while the view model is busy is skipped and logged, never a RefCell panic.
    fn on_viewer_zoom_mode_changed(&self) {
        self.call(|vm| vm.on_viewer_zoom_mode_changed());
    }

    /// C# `Compare.SelectLeft()` / `SelectRight()`; the view model only changes the compare selection (the window repaints the borders from the returned `ChangeSet`).
    fn select_compare_side(&self, left: bool) {
        self.call(|vm| {
            if left {
                vm.select_compare_left()
            } else {
                vm.select_compare_right()
            }
        });
    }

    fn execute(&self, command: ReviewCommand) -> Result<(), Box<dyn Error>> {
        let names = self.services.action_names();
        let owner = self.owner.upgrade();
        match command.kind {
            ReviewCommandType::Fullscreen => self.call(|vm| vm.toggle_fullscreen()),
            ReviewCommandType::ExitFullscreen => self.call(|vm| vm.exit_fullscreen()),
            // Outside `with_view_model`: closing sends messages back into the handler chain (T2).
            ReviewCommandType::Close => self.services.close_window(),
            ReviewCommandType::NextFolder => self.guarded(&names.next_folder, || self.navigate_sibling(1)),
            ReviewCommandType::PreviousFolder => self.guarded(&names.previous_folder, || self.navigate_sibling(-1)),
            ReviewCommandType::FirstImage => self.call(|vm| vm.first()),
            ReviewCommandType::LastImage => self.call(|vm| vm.last()),
            ReviewCommandType::ToggleInfoOverlay => self.call(|vm| vm.toggle_info_overlay()),
            ReviewCommandType::Undo => self.call(|vm| vm.undo()),
            ReviewCommandType::ToggleCompare => self.call(|vm| vm.toggle_compare()),
            ReviewCommandType::ToggleCaptureMember => self.call(|vm| vm.toggle_capture_group_member()),
            ReviewCommandType::RunAction => {
                let index = command.action_index;
                let name = self.action_profile_name(index);
                self.guarded(&name, || {
                    self.call(|vm| vm.run_action(index));
                    Ok(())
                });
            }
            ReviewCommandType::Recycle => self.call(|vm| vm.recycle()),
            ReviewCommandType::Skip => self.call(|vm| vm.skip()),
            ReviewCommandType::Next => self.call(|vm| vm.next()),
            ReviewCommandType::Previous => self.call(|vm| vm.previous()),
            ReviewCommandType::MoveToFolder => self.guarded(&names.move_to_folder, || {
                self.call(|vm| vm.move_to_folder(command.force_picker));
                Ok(())
            }),
            ReviewCommandType::CopyToFolder => self.guarded(&names.copy_to_folder, || {
                self.call(|vm| vm.copy_to_folder(command.force_picker));
                Ok(())
            }),
            ReviewCommandType::ToggleKeepZoom => {
                if let Some(owner) = owner {
                    owner.toggle_keep_zoom_across_images();
                }
            }
            ReviewCommandType::OpenFolder => self.guarded(&names.open_folder, || {
                self.call(|vm| vm.pick_and_open_folder());
                Ok(())
            }),
            ReviewCommandType::CustomZoom => self.guarded(&names.zoom, || {
                if let Some(owner) = owner {
                    owner.open_click_zoom_custom_dialog();
                }
                Ok(())
            }),
            ReviewCommandType::Refresh => {
                if let Some(owner) = owner {
                    owner.refresh_view();
                }
            }
            // W9-E9 (C# `ShellMainWindow.Input.cs`): `_viewModel.OpenInExternalEditor()` = the DEFAULT editor (opens Settings when none is declared); an
            // `Err(ExternalEditorBug)` is returned as the `Err` of `execute` like the menu path does.
            ReviewCommandType::OpenInDefaultEditor => {
                let mut failure = None;
                self.services
                    .with_view_model(&mut |vm| match vm.open_in_external_editor() {
                        Ok(changes) => changes,
                        Err(bug) => {
                            failure = Some(bug);
                            ChangeSet::default()
                        }
                    });
                if let Some(bug) = failure {
                    return Err(Box::new(bug));
                }
            }
            ReviewCommandType::RenameFile => self.rename_file(),
            // The viewport commands never arrive here: `ShellInput` handled them.
            ReviewCommandType::ToggleFit
            | ReviewCommandType::ZoomIn
            | ReviewCommandType::ZoomOut
            | ReviewCommandType::ZoomActualSize
            | ReviewCommandType::ClickZoom
            | ReviewCommandType::FitWidth
            | ReviewCommandType::FitHeight
            | ReviewCommandType::FitWidth2 => {}
        }
        Ok(())
    }

    fn report_action_failed(&self, action_name: &str, error: &dyn Error) {
        let log = self.services.log();
        log.error(&format!("{action_name} failed"), Some(&LoggedError(error.to_string())));
        self.services
            .show_status(&self.services.action_failed_text(action_name, error));
    }

    fn zoom_action_name(&self) -> String {
        self.services.action_names().zoom
    }
}

/// The input wiring of one main window (C# `ShellMainWindow` fields `_input`, `_dropTarget` and the `RegisterInput` body).
pub struct MainWindowInput {
    services: Rc<dyn MainWindowInputServices>,
    input: ShellInput,
    host: Rc<MainWindowInputHost>,
    drop_target: RefCell<Option<ShellDropTarget>>,
    closed_subscription: Cell<Option<EventHandlerId>>,
}

impl fmt::Debug for MainWindowInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWindowInput").finish_non_exhaustive()
    }
}

impl MainWindowInput {
    /// C# `RegisterInput()`: builds the surface and `ShellInput` (host = this wiring), then registers the drop target on the window handle. A failed
    /// `RegisterDragDrop` logs the warning "Drag and drop is unavailable (RegisterDragDrop failed)." and goes on. The caller (W8-13h) must register
    /// the scroll bars first and the context menu after (handler order). This function subscribes the window's `Closed` event itself (D-W8B4X-16).
    pub fn register(services: Rc<dyn MainWindowInputServices>) -> Rc<MainWindowInput> {
        let this = Rc::new_cyclic(|weak: &Weak<MainWindowInput>| {
            let surface = Rc::new(RefCell::new(ViewportImageSurface::new(
                services.viewport(),
                services.viewer(),
            )));
            let host = Rc::new(MainWindowInputHost {
                services: Rc::clone(&services),
                owner: weak.clone(),
            });
            let source = services.settings();
            let input = ShellInput::new(ShellInputDeps {
                window: services.window(),
                surface: surface as Rc<RefCell<dyn InputSurface>>,
                viewer: services.viewer(),
                settings: Rc::new(move || source.current()),
                host: Rc::clone(&host) as Rc<dyn ShellInputHost>,
                dispatcher: services.dispatcher(),
                environment: services.input_environment(),
                log: services.log(),
                ticks_per_ms: services.ticks_per_ms(),
            });
            MainWindowInput {
                services: Rc::clone(&services),
                input,
                host,
                drop_target: RefCell::new(None),
                closed_subscription: Cell::new(None),
            }
        });
        this.register_drop_target();
        this.install_source_size_swapping();
        let weak = Rc::downgrade(&this);
        let subscription = this.services.window().subscribe(
            ShellWindowEvent::Closed,
            Box::new(move || {
                if let Some(input) = weak.upgrade() {
                    input.on_window_closed();
                }
            }),
        );
        this.closed_subscription.set(Some(subscription));
        this
    }

    /// C# `PointerInputController` constructor `ViewerState.SourceSizeSwapping += OnSourceSizeSwapping` (ADR 0008 amendment R4): after a RAW preview is replaced by the
    /// full decode of another size the image point at the viewport centre is re-anchored. The hook runs inside `PresentationBridge::on_set_current_image` while the
    /// viewer is borrowed mutably (the `before` argument is the viewer state) and possibly while the view model is borrowed, so it touches neither: it only needs the
    /// input's own state, and a busy input (`try_borrow_mut` inside) skips the re-anchor instead of panicking (PRs #318, #332, #333). The hook holds a `Weak`, so it
    /// neither keeps the input alive nor outlives it.
    fn install_source_size_swapping(self: &Rc<Self>) {
        let Some(bridge) = self.services.presentation_bridge() else {
            return;
        };
        let weak = Rc::downgrade(self);
        bridge.set_source_size_swapping_hook(Some(Box::new(move |before| {
            if let Some(input) = weak.upgrade() {
                input.input.on_source_size_swapping(before);
            }
        })));
    }

    /// Rule 6: the drop target opens the first dropped file inline; a refused registration is a warning, not an error.
    fn register_drop_target(self: &Rc<Self>) {
        let open_owner = Rc::downgrade(self);
        let failure_owner = Rc::downgrade(self);
        let target = ShellDropTarget::new(DropCallbacks {
            open_path: Box::new(move |path| {
                if let Some(owner) = open_owner.upgrade() {
                    owner.open_dropped_path(path);
                }
                Ok(())
            }),
            on_failure: Box::new(move |error| {
                if let Some(owner) = failure_owner.upgrade() {
                    owner.report_drop_failure(error);
                }
            }),
            files: None,
            has_files: None,
        });
        match target.register(self.services.window().hwnd()) {
            Ok(()) => *self.drop_target.borrow_mut() = Some(target),
            Err(_) => self
                .services
                .log()
                .warn("Drag and drop is unavailable (RegisterDragDrop failed)."),
        }
    }

    /// C# `OpenDroppedPathAsync(path)`: starts the view model's open (its step machine) and applies the returned `ChangeSet`; runs inline inside the OLE
    /// callback (open question 3 of the contract).
    pub fn open_dropped_path(&self, path: &str) {
        self.services.with_view_model(&mut |vm| vm.open_path(path));
    }

    /// C# `ReportDropFailure(exception)`: log, then the "folder open failed" status.
    pub fn report_drop_failure(&self, error: &dyn Error) {
        self.services
            .log()
            .error("Drop open failed", Some(&LoggedError(error.to_string())));
        self.services.show_status(&self.services.folder_open_failed_text(error));
    }

    /// C# `Input`.
    pub fn input(&self) -> &ShellInput {
        &self.input
    }

    /// The host the wiring gave to `ShellInput` (also what the Tools/overlay buttons use).
    pub fn input_host(&self) -> Rc<dyn ShellInputHost> {
        Rc::clone(&self.host) as Rc<dyn ShellInputHost>
    }

    /// The viewer's `ZoomModeChanged`: a zoom or stretch bit of `changes.viewer` stops the glide (also for changes made by the toolbar or the menu).
    /// The `CurrentIndex` path is the feature hook `on_current_index_changed` of the window (D-W8B4M-7), so it is not repeated here.
    pub fn on_view_model_changes(&self, changes: &ChangeSet) {
        if changes.viewer.contains(ViewerProperty::Zoom) || changes.viewer.contains(ViewerProperty::Stretch) {
            self.input.on_zoom_mode_changed();
        }
    }

    /// C# `_onSettingsChangedForInput`: `_viewModel.Settings = settings; _input.RebuildShortcuts(settings)`. Optional: `ShellInput` also rebuilds
    /// lazily when it sees a different settings object at a key press (D-W8B4I-7).
    pub fn on_settings_changed(&self, settings: &Arc<AppSettings>) {
        self.input.rebuild_shortcuts(settings);
    }

    /// C# `OnWindowClosedForInput`: revokes the drop target (`Dispose`), disposes the input (stops the glide, ends a pan). Idempotent.
    pub fn on_window_closed(&self) {
        if let Some(subscription) = self.closed_subscription.take() {
            self.services.window().unsubscribe(subscription);
        }
        if let Some(bridge) = self.services.presentation_bridge() {
            bridge.set_source_size_swapping_hook(None);
        }
        // Taken out first: the revoke talks to the OS and must not run under the borrow (T2, T3).
        let target = self.drop_target.borrow_mut().take();
        if let Some(target) = target {
            target.dispose();
        }
        self.input.dispose();
    }

    /// C# `DropTarget` (tests call `Drop` with a fake data object); false after the window closed or when registration failed.
    pub fn drop_target_registered(&self) -> bool {
        self.drop_target
            .borrow()
            .as_ref()
            .is_some_and(ShellDropTarget::is_registered)
    }

    /// The COM drop target, for tests that call `Drop` with a fake data object; `None` after the window closed or when registration failed.
    pub fn drop_target_interface(&self) -> Option<windows::Win32::System::Ole::IDropTarget> {
        self.drop_target
            .borrow()
            .as_ref()
            .map(|target| target.interface().clone())
    }

    /// C# `RefreshView()`: `StopKinetic`, `RefreshView()` of the view model, `InvalidateLayout`, `Invalidate`; zoom and offsets unchanged.
    pub fn refresh_view(&self) {
        self.input.stop_kinetic();
        self.services.with_view_model(&mut |vm| vm.refresh_view().1);
        self.services.invalidate_layout_and_window();
    }

    /// C# `OpenClickZoomCustomDialogAsync()`: asks the zoom prompt with the current click-zoom percent and applies the answer
    /// (`apply_zoom_menu_selection`); a cancelled prompt or no prompt does nothing.
    pub fn open_click_zoom_custom_dialog(&self) {
        let Some(prompt) = self.services.zoom_prompt() else {
            return;
        };
        let current = self.services.settings().current().click_zoom_percent;
        if let Some(percent) = prompt.prompt_custom_zoom(current) {
            self.apply_zoom_menu_selection(percent);
        }
    }

    /// C# `ApplyZoomMenuSelectionAsync(percent)`: with `SetZoomAlsoSetsClickLevel` it saves and applies the level (`apply_click_zoom_level`), else only
    /// zooms (`Input.set_click_zoom_level(percent)`).
    pub fn apply_zoom_menu_selection(&self, percent: i32) {
        if self.services.settings().current().set_zoom_also_sets_click_level {
            self.apply_click_zoom_level(percent);
        } else {
            self.input.set_click_zoom_level(percent);
        }
    }

    /// C# `ApplyClickZoomLevelAsync(percent)`: saves `ClickZoomPercent`, then zooms even when the save failed (this session still zooms).
    pub fn apply_click_zoom_level(&self, percent: i32) {
        self.save_setting(
            |settings| settings.click_zoom_percent = percent,
            "Could not save the click zoom level setting",
        );
        self.input.set_click_zoom_level(percent);
    }

    /// C# `ToggleKeepZoomAcrossImages()` (`ReviewCommandType::ToggleKeepZoom`): flips `KeepZoomAcrossImages` and saves it.
    pub fn toggle_keep_zoom_across_images(&self) {
        self.save_setting(
            |settings| settings.keep_zoom_across_images = !settings.keep_zoom_across_images,
            "Could not save the keep-zoom setting",
        );
    }

    /// C# `SaveSettings(settings, failureText)`: the edit is visible at once (a new `Arc` per edit, T5); a failed commit is logged and the provisional
    /// value stays for this session.
    fn save_setting(&self, mutate: impl FnOnce(&mut AppSettings), failure_text: &str) {
        let source = self.services.settings();
        let edit = source.begin_edit(mutate);
        if let Err(error) = source.commit_edit(edit) {
            self.services.log().error(failure_text, Some(&error));
        }
    }
}

// ---- production ----------------------------------------------------------------------------------------------------------------------------------

/// The `ZoomPromptService` of the window options (C# `ShellMainWindowOptions.ZoomPrompt`), as a typed `FeatureOptions` entry (`FeatureOptions::insert`
/// needs a sized `Any` type).
#[derive(Clone)]
pub struct ZoomPromptOption(pub Rc<dyn ZoomPromptService>);

impl fmt::Debug for ZoomPromptOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZoomPromptOption").finish_non_exhaustive()
    }
}

/// W9-R8: the `DialogService` of the window options (the rename box of F2 / "Rename..."), as a typed `FeatureOptions` entry like [`ZoomPromptOption`].
#[derive(Clone)]
pub struct DialogServiceOption(pub Rc<dyn DialogService>);

impl fmt::Debug for DialogServiceOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DialogServiceOption").finish_non_exhaustive()
    }
}

/// The production [`MainWindowInputServices`]: everything the window hands out is held directly; the view model and the change fan-out are reached
/// through the window registry by id (so the adapter never keeps the window alive and never forms a cycle with it).
struct MainWindowInputAdapter {
    id: MainWindowId,
    window: Rc<dyn ShellWindowApi>,
    viewport: Rc<dyn ViewportControllerApi>,
    viewer: ViewerHandle,
    bridge: PresentationBridge,
    settings: SettingsSource,
    zoom_prompt: Option<Rc<dyn ZoomPromptService>>,
    dialogs: Option<Rc<dyn DialogService>>,
    log: Arc<dyn Log>,
    dispatcher: Arc<dyn UiDispatcher>,
    environment: Rc<ShellInputEnvironment>,
}

impl MainWindowInputAdapter {
    fn new(inner: &MainWindowInner) -> Self {
        let extensions = &inner.options().extensions;
        let (settings, bridge) = {
            let view_model = inner.view_model().borrow();
            (view_model.settings_source(), view_model.bridge().clone())
        };
        Self {
            id: inner.id(),
            window: Rc::clone(inner.window()) as Rc<dyn ShellWindowApi>,
            viewport: Rc::clone(inner.viewport()) as Rc<dyn ViewportControllerApi>,
            viewer: inner.viewer(),
            bridge,
            settings,
            zoom_prompt: extensions.get::<ZoomPromptOption>().map(|option| Rc::clone(&option.0)),
            dialogs: extensions
                .get::<DialogServiceOption>()
                .map(|option| Rc::clone(&option.0)),
            log: Arc::clone(inner.log()),
            dispatcher: Arc::new(inner.dispatcher().clone()),
            environment: extensions
                .get::<ShellInputEnvironment>()
                .unwrap_or_else(|| Rc::new(ShellInputEnvironment::default())),
        }
    }
}

impl MainWindowInputServices for MainWindowInputAdapter {
    fn window(&self) -> Rc<dyn ShellWindowApi> {
        Rc::clone(&self.window)
    }
    fn viewport(&self) -> Rc<dyn ViewportControllerApi> {
        Rc::clone(&self.viewport)
    }
    fn presentation_bridge(&self) -> Option<PresentationBridge> {
        Some(self.bridge.clone())
    }
    fn viewer(&self) -> ViewerHandle {
        self.viewer.clone()
    }
    fn settings(&self) -> SettingsSource {
        self.settings.clone()
    }
    fn zoom_prompt(&self) -> Option<Rc<dyn ZoomPromptService>> {
        self.zoom_prompt.clone()
    }
    fn log(&self) -> Arc<dyn Log> {
        Arc::clone(&self.log)
    }
    fn dialog_service(&self) -> Option<Rc<dyn DialogService>> {
        self.dialogs.clone()
    }
    fn dispatcher(&self) -> Arc<dyn UiDispatcher> {
        Arc::clone(&self.dispatcher)
    }
    fn input_environment(&self) -> Rc<ShellInputEnvironment> {
        Rc::clone(&self.environment)
    }
    fn ticks_per_ms(&self) -> f64 {
        pr_core::diagnostics::perf::STOPWATCH_FREQUENCY as f64 / 1000.0
    }
    /// D-W8B4M-2: the view-model borrow ends before the `ChangeSet` is applied; an empty set is not applied (a query changes nothing).
    fn with_view_model(&self, f: &mut dyn FnMut(&mut MainViewModel) -> ChangeSet) {
        with_window(self.id, |inner| {
            let changes = {
                let Ok(mut view_model) = inner.view_model().try_borrow_mut() else {
                    inner.note_busy_drop("Input command");
                    return;
                };
                f(&mut view_model)
            };
            if !changes.is_empty() {
                inner.apply_changes(&changes);
            }
            // BACKLOG G1: a modal command (Settings, the folder picker) ends here: drain the wake, open the queued paths, replay a recorded close.
            inner.after_view_model_call();
        });
    }
    fn show_status(&self, text: &str) {
        self.with_view_model(&mut |vm| vm.set_status_text(text));
    }
    fn folder_open_failed_text(&self, error: &dyn Error) -> String {
        pr_l10n::tr::status_folder_open_failed(error.to_string())
    }
    fn action_failed_text(&self, action_name: &str, error: &dyn Error) -> String {
        pr_l10n::tr::status_action_failed(action_name, error.to_string())
    }
    fn action_names(&self) -> InputActionNames {
        use pr_l10n::tr;
        InputActionNames {
            next_folder: tr::main_menu_next_folder(),
            previous_folder: tr::main_menu_previous_folder(),
            move_to_folder: tr::action_move_to_folder_name(),
            copy_to_folder: tr::action_copy_to_folder_name(),
            open_folder: tr::main_menu_open_folder()
                .trim_end_matches(['\u{2026}', '.', ' '])
                .to_owned(),
            zoom: tr::main_menu_zoom(),
        }
    }
    fn close_window(&self) {
        self.window.close();
    }
    fn invalidate_layout_and_window(&self) {
        self.viewport.invalidate_layout();
        self.window.invalidate();
    }
}

/// The feature the window holds (C# `RegisterInput` + `OnWindowClosedForInput`), name `"input"` (D-W8B4X-9): the hooks of [`MainWindowFeature`] map to
/// the input; `MainWindow::with_feature::<MainWindowInputFeature>` gives 14f and 17e the handle.
pub struct MainWindowInputFeature {
    input: Rc<MainWindowInput>,
}

impl MainWindowInputFeature {
    /// Wraps an input wiring (production: made by [`MainWindowInner::register_input`]).
    pub fn new(input: Rc<MainWindowInput>) -> Self {
        Self { input }
    }

    /// The input wiring.
    pub fn input(&self) -> &Rc<MainWindowInput> {
        &self.input
    }
}

impl fmt::Debug for MainWindowInputFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWindowInputFeature").finish_non_exhaustive()
    }
}

impl MainWindowFeature for MainWindowInputFeature {
    fn name(&self) -> &'static str {
        "input"
    }
    /// C# `OnWindowClosedForInput` again (the window's `Closed` already ran it; it is idempotent).
    fn dispose(&self) {
        self.input.on_window_closed();
    }
    fn on_changes(&self, changes: &ChangeSet) {
        self.input.on_view_model_changes(changes);
    }
    fn on_current_index_changed(&self, index: i32) {
        self.input.input().note_current_index_changed(index);
    }
    /// C# `onPress: () => _input?.Pointer.OnWindowPreviewMouseDown()`.
    fn on_scroll_bar_press(&self) {
        self.input.input().pointer_press_tunnel();
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl MainWindowInner {
    /// C# `RegisterInput()`, the 2nd line of `register_features` (after the scroll bars, before windowing and the context menu): builds the adapter from the
    /// window, registers the input and adds the [`MainWindowInputFeature`].
    pub(crate) fn register_input(&self) {
        let services = Rc::new(MainWindowInputAdapter::new(self));
        let input = MainWindowInput::register(services);
        self.add_feature(Rc::new(MainWindowInputFeature::new(input)));
    }
}
