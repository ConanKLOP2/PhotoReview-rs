//! W9-E9 editor forwarding ports the corresponding C# source at baseline `7eaee2a2`.
//! Port of `PhotoReview.Shell.Win32/Menus/ShellMainWindow.Menus.cs` (92 lines, baseline `d92d6aae`). Unit W8-14f. Contract: `contracts/W8-14f.md`.
//!
//! The menu part of the main window (WP-24): `RegisterContextMenu`, the `IContextMenuActions` implementation (the `MainViewModel` and `ShellInput` calls
//! behind each menu command) and `ReportMenuFailed`. The main window itself is `host::main_window` (W8-13g..13k, written concurrently), so what this
//! unit needs from it is the trait [`MainWindowMenuServices`] (D-W8B4I-6); the zoom helpers it calls live in `shell_input::main_window::MainWindowInput`
//! (W8-14c).

use std::error::Error;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::input::review_command::ReviewCommand;
use pr_app::viewmodels::main::{ChangeSet, MainViewModel, SettingsSource};
use pr_core::abstractions::Log;
use pr_core::settings::app_settings::AppSettings;

use super::controller::{ActionResult, ContextMenuActions, ContextMenuController};
use super::message_handler::ContextMenuMessageHandler;
use super::popup_host::NativePopupMenuHost;
use crate::host::contracts::ShellWindowApi;
use crate::host::main_window::core::{MainWindowFeature, MainWindowId, MainWindowInner, with_window};
use crate::shell_input::main_window::{MainWindowInput, MainWindowInputFeature};
use crate::win32::structs::Hwnd;

/// What the menu wiring needs from the main window (W8-13h). UI thread; `&self` because the window is re-entered while a menu is modal.
pub trait MainWindowMenuServices {
    /// C# `_window`.
    fn window(&self) -> Rc<dyn ShellWindowApi>;
    /// C# `_window.IsDestroyed ? 0 : _window.Hwnd`: the owner of the popup menu.
    fn owner_hwnd(&self) -> Hwnd;
    /// C# `_log`.
    fn log(&self) -> Arc<dyn Log>;
    /// C# `_viewModel.Settings` and `SaveSettings` (`begin_edit`/`commit_edit`).
    fn settings(&self) -> SettingsSource;
    /// Runs `f` with the view model, then applies the returned `ChangeSet` (see `MainWindowInputServices::with_view_model`).
    fn with_view_model(&self, f: &mut dyn FnMut(&mut MainViewModel) -> ChangeSet);
    /// C# `ShellErrorLog.Report(context, exception)` (`shell-errors.log`, also when the general log is off). Seam for W8-17a.
    fn report_shell_error(&self, context: &str, error: &dyn Error);
    /// C# `_viewModel.StatusText = Tr.StatusMenuFailed(UserFacingError.Describe(exception))` (a status line that does not block).
    fn show_menu_failed_status(&self, error: &dyn Error);
    /// C# `Tr.MainMenuZoom`.
    fn zoom_action_name(&self) -> String;
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

/// C# `ReportMenuFailed(context, exception)`: the `shell-errors.log` entry, the general log `"{context} failed"`, then the status line. Each of the
/// three is one call on the services, so no view-model borrow outlives a call (it is used from inside the modal loop).
fn report_menu_failed(services: &dyn MainWindowMenuServices, context: &str, error: &dyn Error) {
    services.report_shell_error(context, error);
    services
        .log()
        .error(&format!("{context} failed"), Some(&LoggedError(error.to_string())));
    services.show_menu_failed_status(error);
}

/// The menu wiring of one main window (C# `_contextMenu` and the `Register`/`Report` methods).
pub struct MainWindowMenus {
    services: Rc<dyn MainWindowMenuServices>,
    controller: Rc<ContextMenuController>,
}

impl MainWindowMenus {
    /// C# `RegisterContextMenu()`: a `ContextMenuController` over [`MainWindowMenuActions`] and a `NativePopupMenuHost` whose owner is
    /// `services.owner_hwnd()`, then `window.add_message_handler(ContextMenuMessageHandler)`. The caller (W8-13h) registers the scroll bars first, the
    /// input second and this last (handler order).
    pub fn register(services: Rc<dyn MainWindowMenuServices>, input: Rc<MainWindowInput>) -> MainWindowMenus {
        // The host lives inside the controller, which the window (and so, in the composition root, the services) owns: it holds a `Weak` to the
        // services so there is no reference cycle (D-W8B3H-18). A services object that is gone has no window, i.e. owner 0.
        let weak = Rc::downgrade(&services);
        let host = NativePopupMenuHost::new(Box::new(move || {
            weak.upgrade().map_or(0, |services| services.owner_hwnd())
        }))
        .with_log(services.log());
        let actions = Rc::new(MainWindowMenuActions::new(Rc::clone(&services), input));
        let controller = Rc::new(ContextMenuController::new(actions, Rc::new(host), Some(services.log())));
        let window = services.window();
        window.add_message_handler(Rc::new(ContextMenuMessageHandler::new(
            Rc::clone(&window),
            Rc::clone(&controller),
        )));
        MainWindowMenus { services, controller }
    }

    /// C# `ContextMenu`.
    pub fn context_menu(&self) -> &Rc<ContextMenuController> {
        &self.controller
    }

    /// C# `ReportMenuFailed(context, exception)`: `shell-errors.log` entry, the general log `"{context} failed"`, then the status line. Callable while a
    /// menu is modal; touches the view model only on the UI thread.
    pub fn report_menu_failed(&self, context: &str, error: &dyn Error) {
        report_menu_failed(self.services.as_ref(), context, error);
    }
}

impl fmt::Debug for MainWindowMenus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWindowMenus").finish_non_exhaustive()
    }
}

/// The `IContextMenuActions` implementation of the main window (C# explicit interface members of `ShellMainWindow`).
pub struct MainWindowMenuActions {
    services: Rc<dyn MainWindowMenuServices>,
    input: Rc<MainWindowInput>,
}

impl MainWindowMenuActions {
    /// Builds the actions over the services and the input wiring.
    pub fn new(services: Rc<dyn MainWindowMenuServices>, input: Rc<MainWindowInput>) -> Self {
        Self { services, input }
    }

    /// Reads a value from the view model (one `with_view_model` call; the change set is empty). `R::default()` if the services did not run `f`.
    fn read<R: Default>(&self, f: impl Fn(&MainViewModel) -> R) -> R {
        let mut out = None;
        self.services.with_view_model(&mut |vm| {
            out = Some(f(vm));
            ChangeSet::default()
        });
        out.unwrap_or_default()
    }

    /// C# `SaveSettings(settings, failureText)` for the two setters: the edit is visible at once, a failed save is logged.
    fn save_setting(&self, mutate: impl FnOnce(&mut AppSettings), failure_text: &str) {
        let source = self.services.settings();
        let edit = source.begin_edit(mutate);
        if let Err(error) = source.commit_edit(edit) {
            self.services.log().error(failure_text, Some(&error));
        }
    }
}

impl fmt::Debug for MainWindowMenuActions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWindowMenuActions").finish_non_exhaustive()
    }
}

impl ContextMenuActions for MainWindowMenuActions {
    fn settings(&self) -> Arc<AppSettings> {
        self.services.settings().current()
    }
    fn has_image(&self) -> bool {
        self.read(MainViewModel::has_images)
    }
    fn can_copy_path(&self) -> bool {
        self.read(MainViewModel::can_copy_current_file_path)
    }
    /// "Always on like WPF: an empty history is a no-op."
    fn can_undo(&self) -> bool {
        true
    }
    fn can_open_in_external_editor(&self) -> bool {
        self.read(MainViewModel::can_open_in_external_editor)
    }
    fn is_compare_visible(&self) -> bool {
        self.read(|vm| vm.compare().is_visible())
    }
    fn effective_zoom(&self) -> Option<f64> {
        self.read(|vm| vm.viewer().borrow().effective_zoom())
    }
    fn execute(&self, command: ReviewCommand) -> ActionResult {
        // A failure of the command is reported inside `ShellInput`.
        self.input.input().execute(command);
        Ok(())
    }
    fn fit(&self) -> ActionResult {
        self.input.input().apply_fit();
        Ok(())
    }
    fn zoom_to_configured_level(&self) -> ActionResult {
        let percent = self.services.settings().current().click_zoom_percent;
        self.input.input().set_click_zoom_level(percent);
        Ok(())
    }
    fn apply_zoom_preset(&self, percent: i32) -> ActionResult {
        self.input.apply_zoom_menu_selection(percent);
        Ok(())
    }
    fn custom_zoom(&self) -> ActionResult {
        self.input.open_click_zoom_custom_dialog();
        Ok(())
    }
    fn set_zoom_also_sets_click_level(&self, value: bool) {
        self.save_setting(
            |settings| settings.set_zoom_also_sets_click_level = value,
            "Could not save the 'also set as click zoom level' setting",
        );
    }
    fn set_click_zoom_percent(&self, percent: i32) {
        self.save_setting(
            |settings| settings.click_zoom_percent = percent,
            "Could not save the click zoom level setting",
        );
    }
    fn open_in_external_editor(&self) -> ActionResult {
        let mut failure = None;
        self.services
            .with_view_model(&mut |vm| match vm.open_in_external_editor() {
                Ok(changes) => changes,
                Err(bug) => {
                    failure = Some(bug);
                    ChangeSet::default()
                }
            });
        match failure {
            Some(bug) => Err(Box::new(bug)),
            None => Ok(()),
        }
    }
    /// W9-E9: `vm.open_in_external_editor_by_id(editor_id)`, an `Err(ExternalEditorBug)` becomes the `Err` (same shape as `open_in_external_editor`).
    fn open_in_external_editor_by_id(&self, editor_id: &str) -> ActionResult {
        let mut failure = None;
        self.services
            .with_view_model(&mut |vm| match vm.open_in_external_editor_by_id(editor_id) {
                Ok(changes) => changes,
                Err(bug) => {
                    failure = Some(bug);
                    ChangeSet::default()
                }
            });
        match failure {
            Some(bug) => Err(Box::new(bug)),
            None => Ok(()),
        }
    }
    /// W9-E9: `vm.add_external_editor()` (C# `ShellMainWindow.Menus.cs`: `_viewModel.AddExternalEditor()`).
    fn add_external_editor(&self) -> ActionResult {
        self.services.with_view_model(&mut |vm| vm.add_external_editor());
        Ok(())
    }
    /// W9-E9: `vm.choose_default_external_editor()`.
    fn choose_default_external_editor(&self) -> ActionResult {
        self.services
            .with_view_model(&mut |vm| vm.choose_default_external_editor());
        Ok(())
    }
    fn copy_file_name(&self) -> ActionResult {
        self.services.with_view_model(&mut |vm| vm.copy_file_name());
        Ok(())
    }
    fn copy_full_pathname(&self) -> ActionResult {
        self.services.with_view_model(&mut |vm| vm.copy_full_pathname());
        Ok(())
    }
    fn show_settings(&self) -> ActionResult {
        self.services.with_view_model(&mut |vm| vm.show_settings());
        Ok(())
    }
    fn report_action_failed(&self, action_name: &str, error: &dyn Error) {
        self.input.input_host().report_action_failed(action_name, error);
    }
    fn report_menu_failed(&self, error: &dyn Error) {
        report_menu_failed(self.services.as_ref(), "ContextMenu", error);
    }
    fn zoom_action_name(&self) -> String {
        self.services.zoom_action_name()
    }
}

// ---- production ----------------------------------------------------------------------------------------------------------------------------------

/// `ShellErrorLog.Report(context, error)`.
pub type ShellErrorSink = Rc<dyn Fn(&str, &dyn Error)>;

/// The sink of `shell-errors.log` (C# `ShellErrorLog.Report(context, exception)`), as a typed `FeatureOptions` entry. The composition root inserts it once
/// W8-17a exists (contract open question 3); without it `report_shell_error` writes nothing (the general log line of `report_menu_failed` remains).
#[derive(Clone)]
pub struct ShellErrorSinkOption(pub ShellErrorSink);

impl fmt::Debug for ShellErrorSinkOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellErrorSinkOption").finish_non_exhaustive()
    }
}

/// The production [`MainWindowMenuServices`]: the window and the log are held directly; the view model is reached through the window registry by id, so the
/// adapter never keeps the window alive and never forms a cycle with it.
struct MainWindowMenuAdapter {
    id: MainWindowId,
    window: Rc<dyn ShellWindowApi>,
    settings: SettingsSource,
    log: Arc<dyn Log>,
    shell_errors: Option<ShellErrorSink>,
}

impl MainWindowMenuAdapter {
    fn new(inner: &MainWindowInner) -> Self {
        Self {
            id: inner.id(),
            window: Rc::clone(inner.window()) as Rc<dyn ShellWindowApi>,
            settings: inner.view_model().borrow().settings_source(),
            log: Arc::clone(inner.log()),
            shell_errors: inner
                .options()
                .extensions
                .get::<ShellErrorSinkOption>()
                .map(|option| Rc::clone(&option.0)),
        }
    }
}

impl MainWindowMenuServices for MainWindowMenuAdapter {
    fn window(&self) -> Rc<dyn ShellWindowApi> {
        Rc::clone(&self.window)
    }
    fn owner_hwnd(&self) -> Hwnd {
        if self.window.is_destroyed() {
            0
        } else {
            self.window.hwnd()
        }
    }
    fn log(&self) -> Arc<dyn Log> {
        Arc::clone(&self.log)
    }
    fn settings(&self) -> SettingsSource {
        self.settings.clone()
    }
    /// Same rule as the input adapter (D-W8B4M-2): the view-model borrow ends before the `ChangeSet` is applied; an empty set is not applied.
    fn with_view_model(&self, f: &mut dyn FnMut(&mut MainViewModel) -> ChangeSet) {
        with_window(self.id, |inner| {
            let changes = {
                let Ok(mut view_model) = inner.view_model().try_borrow_mut() else {
                    inner
                        .log()
                        .warn("Menu command skipped: the view model is busy (re-entrant call).");
                    return;
                };
                f(&mut view_model)
            };
            if !changes.is_empty() {
                inner.apply_changes(&changes);
            }
        });
    }
    fn report_shell_error(&self, context: &str, error: &dyn Error) {
        if let Some(sink) = &self.shell_errors {
            sink(context, error);
        }
    }
    fn show_menu_failed_status(&self, error: &dyn Error) {
        let text = pr_l10n::tr::status_menu_failed(error.to_string());
        self.with_view_model(&mut |vm| vm.set_status_text(&text));
    }
    fn zoom_action_name(&self) -> String {
        pr_l10n::tr::main_menu_zoom()
    }
}

/// The feature the window holds (C# `_contextMenu`), name `"context-menu"`: `MainWindow::with_feature::<MainWindowMenuFeature>` gives 17e/15j the handle.
pub struct MainWindowMenuFeature {
    menus: MainWindowMenus,
}

impl MainWindowMenuFeature {
    /// Wraps a menu wiring (production: made by [`MainWindowInner::register_context_menu`]).
    pub fn new(menus: MainWindowMenus) -> Self {
        Self { menus }
    }

    /// The menu wiring.
    pub fn menus(&self) -> &MainWindowMenus {
        &self.menus
    }
}

impl fmt::Debug for MainWindowMenuFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWindowMenuFeature").finish_non_exhaustive()
    }
}

impl MainWindowFeature for MainWindowMenuFeature {
    fn name(&self) -> &'static str {
        "context-menu"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl MainWindowInner {
    /// C# `RegisterContextMenu()`, the 4th line of `register_features` (after scroll bars, input and windowing: the handler is the LAST of the appended
    /// ones, D-W8B4X-15). The input wiring comes from the `"input"` feature (D-W8B4X-9); without it nothing is registered and the log says so.
    pub(crate) fn register_context_menu(&self) {
        let Some(input) = self.with_feature::<MainWindowInputFeature, _>(|feature| Rc::clone(feature.input())) else {
            self.log()
                .warn("The context menu is not registered: the input feature is missing.");
            return;
        };
        let services = Rc::new(MainWindowMenuAdapter::new(self));
        let menus = MainWindowMenus::register(services, input);
        self.add_feature(Rc::new(MainWindowMenuFeature::new(menus)));
    }
}
