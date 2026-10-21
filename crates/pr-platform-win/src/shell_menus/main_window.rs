//! Port of `PhotoReview.Shell.Win32/Menus/ShellMainWindow.Menus.cs` (92 lines, baseline `d92d6aae`). Unit W8-14f. Contract: `contracts/W8-14f.md`.
//!
//! The menu part of the main window (WP-24): `RegisterContextMenu`, the `IContextMenuActions` implementation (the `MainViewModel` and `ShellInput` calls
//! behind each menu command) and `ReportMenuFailed`. The main window itself is `host::main_window` (W8-13g..13k, written concurrently), so what this
//! unit needs from it is the trait [`MainWindowMenuServices`] (D-W8B4I-6); the zoom helpers it calls live in `shell_input::main_window::MainWindowInput`
//! (W8-14c).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::error::Error;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::viewmodels::main::{ChangeSet, MainViewModel, SettingsSource};
use pr_core::abstractions::Log;

use super::controller::{ContextMenuActions, ContextMenuController};
use crate::host::contracts::ShellWindowApi;
use crate::shell_input::main_window::MainWindowInput;
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
        todo!()
    }

    /// C# `ContextMenu`.
    pub fn context_menu(&self) -> &Rc<ContextMenuController> {
        &self.controller
    }

    /// C# `ReportMenuFailed(context, exception)`: `shell-errors.log` entry, the general log `"{context} failed"`, then the status line. Callable while a
    /// menu is modal; touches the view model only on the UI thread.
    pub fn report_menu_failed(&self, context: &str, error: &dyn Error) {
        todo!()
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
}

impl fmt::Debug for MainWindowMenuActions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWindowMenuActions").finish_non_exhaustive()
    }
}

impl ContextMenuActions for MainWindowMenuActions {
    fn settings(&self) -> Arc<pr_core::settings::app_settings::AppSettings> {
        todo!()
    }
    fn has_image(&self) -> bool {
        todo!()
    }
    fn can_copy_path(&self) -> bool {
        todo!()
    }
    fn can_undo(&self) -> bool {
        todo!()
    }
    fn can_open_in_external_editor(&self) -> bool {
        todo!()
    }
    fn is_compare_visible(&self) -> bool {
        todo!()
    }
    fn effective_zoom(&self) -> Option<f64> {
        todo!()
    }
    fn execute(&self, command: pr_app::input::review_command::ReviewCommand) -> super::controller::ActionResult {
        todo!()
    }
    fn fit(&self) -> super::controller::ActionResult {
        todo!()
    }
    fn zoom_to_configured_level(&self) -> super::controller::ActionResult {
        todo!()
    }
    fn apply_zoom_preset(&self, percent: i32) -> super::controller::ActionResult {
        todo!()
    }
    fn custom_zoom(&self) -> super::controller::ActionResult {
        todo!()
    }
    fn set_zoom_also_sets_click_level(&self, value: bool) {
        todo!()
    }
    fn set_click_zoom_percent(&self, percent: i32) {
        todo!()
    }
    fn open_in_external_editor(&self) -> super::controller::ActionResult {
        todo!()
    }
    fn copy_file_name(&self) -> super::controller::ActionResult {
        todo!()
    }
    fn copy_full_pathname(&self) -> super::controller::ActionResult {
        todo!()
    }
    fn show_settings(&self) -> super::controller::ActionResult {
        todo!()
    }
    fn report_action_failed(&self, action_name: &str, error: &dyn Error) {
        todo!()
    }
    fn report_menu_failed(&self, error: &dyn Error) {
        todo!()
    }
    fn zoom_action_name(&self) -> String {
        self.services.zoom_action_name()
    }
}
