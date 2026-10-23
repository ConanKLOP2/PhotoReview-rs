//! Port of `PhotoReview.Shell.Win32/Menus/ContextMenuController.cs` (207 lines, with `IContextMenuActions`) and `Menus/IPopupMenuHost.cs` (10 lines),
//! baseline `d92d6aae`. Unit W8-14e. Contract: `contracts/W8-14e.md`.
//!
//! WP-24: the right-click menu of the viewer. Builds the model with `pr_app::menus::builder::build` from the settings and the state AT OPEN TIME
//! (like `ImageContextMenu_Opened`), shows it through a [`PopupMenuHost`] (modal), then runs the command of the chosen id exactly like the `*_Click`
//! handlers of the WPF `MainWindow`. Pure over two traits: the actions the window performs and the host that shows the menu.
//!
//! Differences from C# (contract section "Rust shape"): no `Task`; commands run synchronously after the menu closed and a failure is `Err` (reported
//! here); `OperationCanceledException` has no Rust twin; `ShowAsync` is [`ContextMenuController::show`].

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::input::primitives::PointD;
use pr_app::input::review_command::ReviewCommand;
use pr_app::menus::builder::{self, MenuBuildError};
use pr_app::menus::model::{ContextMenuBuildContext, MenuItemModel};
use pr_app::menus::zoom_rules;
use pr_core::abstractions::Log;
use pr_core::settings::app_settings::AppSettings;

/// C# `ZoomPresetPrefix`: ids `Zoom.Preset.<percent>` (digits only, invariant culture).
pub const ZOOM_PRESET_PREFIX: &str = "Zoom.Preset.";

/// What a command can end with: `Err` is reported with the command name and never escapes `show`.
pub type ActionResult = Result<(), Box<dyn Error>>;

/// C# `IPopupMenuHost` (C-12 host): shows the model modally and returns the chosen id, or `None` when the user dismissed it. `Err` means the menu
/// could not be built or shown (the controller reports it as a menu failure).
pub trait PopupMenuHost {
    /// C# `Show(items, screenPointDip)`: `screen_point_dip` is in DIP of the owner window's DPI.
    fn show(&self, items: &[MenuItemModel], screen_point_dip: PointD) -> Result<Option<String>, Box<dyn Error>>;
}

/// C# `IContextMenuActions`: what the menu needs from the main window. `&self` (the window is re-entered while the menu is modal).
pub trait ContextMenuActions {
    /// C# `Settings`: read again at every open (a change in Settings shows at once).
    fn settings(&self) -> Arc<AppSettings>;
    /// C# `HasImage`.
    fn has_image(&self) -> bool;
    /// C# `CanCopyPath` (`MainViewModel.CanCopyCurrentFilePath`).
    fn can_copy_path(&self) -> bool;
    /// C# `CanUndo` (always true in the window: an empty history is a no-op).
    fn can_undo(&self) -> bool;
    /// C# `CanOpenInExternalEditor`.
    fn can_open_in_external_editor(&self) -> bool;
    /// C# `IsCompareVisible`.
    fn is_compare_visible(&self) -> bool;
    /// C# `EffectiveZoom`: `None` while Fit.
    fn effective_zoom(&self) -> Option<f64>;
    /// C# `ExecuteAsync(command)` (the key path `ShellInput::execute`): Undo, Recycle, Refresh, OpenFolder, NextFolder, PreviousFolder, FitWidth, FitWidth2, FitHeight.
    fn execute(&self, command: ReviewCommand) -> ActionResult;
    /// C# `FitAsync()` (`ClickZoomFit_Click`).
    fn fit(&self) -> ActionResult;
    /// C# `ZoomToConfiguredLevelAsync()` (`ZoomToLevel_Click`): zoom to `ClickZoomPercent`, level unchanged.
    fn zoom_to_configured_level(&self) -> ActionResult;
    /// C# `ApplyZoomPresetAsync(percent)`.
    fn apply_zoom_preset(&self, percent: i32) -> ActionResult;
    /// C# `CustomZoomAsync()`.
    fn custom_zoom(&self) -> ActionResult;
    /// C# `SetZoomAlsoSetsClickLevel(value)`: write and save.
    fn set_zoom_also_sets_click_level(&self, value: bool);
    /// C# `SetClickZoomPercent(percent)`: write and save, no zoom.
    fn set_click_zoom_percent(&self, percent: i32);
    /// C# `OpenInExternalEditor()` (the DEFAULT editor).
    fn open_in_external_editor(&self) -> ActionResult;
    /// W9-E9, C# `OpenInExternalEditor(string editorId)` (default interface method: `OpenInExternalEditor()`): one entry of the editors submenu.
    fn open_in_external_editor_by_id(&self, _editor_id: &str) -> ActionResult {
        self.open_in_external_editor()
    }
    /// W9-E9, C# `AddExternalEditor()` ("Add more editors..."; default interface method: nothing).
    fn add_external_editor(&self) -> ActionResult {
        Ok(())
    }
    /// W9-E9, C# `ChooseDefaultExternalEditor()` ("Set default editor..."; default interface method: nothing).
    fn choose_default_external_editor(&self) -> ActionResult {
        Ok(())
    }
    /// C# `CopyFileName()`.
    fn copy_file_name(&self) -> ActionResult;
    /// C# `CopyFullPathname()`.
    fn copy_full_pathname(&self) -> ActionResult;
    /// C# `ShowSettings()`.
    fn show_settings(&self) -> ActionResult;
    /// C# `ReportActionFailed(actionName, exception)`.
    fn report_action_failed(&self, action_name: &str, error: &dyn Error);
    /// C# `ReportMenuFailed(exception)`: `shell-errors.log` + the "Menu error" status line. C# default: `ReportActionFailed("Menu", exception)`.
    fn report_menu_failed(&self, error: &dyn Error) {
        self.report_action_failed("Menu", error);
    }
    /// C# `Tr.MainMenuZoom` (the localized "Zoom", the name of the zoom commands in a failure report).
    fn zoom_action_name(&self) -> String;
}

/// W9-E9, C# `ExternalEditorOpenPrefix`: ids of the editors submenu are `"ExternalEditor.Open." + editor id` (the same text as
/// `pr_app::menus::external_editors::EXTERNAL_EDITOR_OPEN_PREFIX`; a test compares the two).
pub const EXTERNAL_EDITOR_OPEN_PREFIX: &str = "ExternalEditor.Open.";
/// W9-E9, C# `ExternalEditorAddId`.
pub const EXTERNAL_EDITOR_ADD_ID: &str = "ExternalEditor.Add";
/// W9-E9, C# `ExternalEditorSetDefaultId`.
pub const EXTERNAL_EDITOR_SET_DEFAULT_ID: &str = "ExternalEditor.SetDefault";

/// C# `ContextMenuController`.
pub struct ContextMenuController {
    actions: Rc<dyn ContextMenuActions>,
    menu_host: RefCell<Rc<dyn PopupMenuHost>>,
    log: Option<Arc<dyn Log>>,
    is_open: Cell<bool>,
}

impl ContextMenuController {
    /// C# constructor (`actions` and `menuHost` are never null: unrepresentable).
    pub fn new(
        actions: Rc<dyn ContextMenuActions>,
        menu_host: Rc<dyn PopupMenuHost>,
        log: Option<Arc<dyn Log>>,
    ) -> Self {
        Self {
            actions,
            menu_host: RefCell::new(menu_host),
            log,
            is_open: Cell::new(false),
        }
    }

    /// C# `MenuHost` getter.
    pub fn menu_host(&self) -> Rc<dyn PopupMenuHost> {
        Rc::clone(&self.menu_host.borrow())
    }

    /// C# `MenuHost` setter (tests install a fake that picks an id).
    pub fn set_menu_host(&self, host: Rc<dyn PopupMenuHost>) {
        *self.menu_host.borrow_mut() = host;
    }

    /// C# `IsOpen`: true while the modal menu is open; opening again is ignored.
    pub fn is_open(&self) -> bool {
        self.is_open.get()
    }

    /// C# `BuildModel()`: `ContextMenuBuildContext { settings, has_image, can_undo, is_fit: effective_zoom.is_none(), effective_zoom,
    /// can_open_in_external_editor, can_copy_path, is_compare_visible }` through `menus::builder::build`.
    pub fn build_model(&self) -> Result<Vec<MenuItemModel>, MenuBuildError> {
        let settings = self.actions.settings();
        let effective_zoom = self.actions.effective_zoom();
        let context = ContextMenuBuildContext {
            settings: &settings,
            has_image: self.actions.has_image(),
            can_undo: self.actions.can_undo(),
            is_fit: effective_zoom.is_none(),
            effective_zoom,
            can_open_in_external_editor: self.actions.can_open_in_external_editor(),
            can_copy_path: self.actions.can_copy_path(),
            is_compare_visible: self.actions.is_compare_visible(),
        };
        builder::build(&context)
    }

    /// C# `ShowAsync(screenPointDip)`: already open: nothing. Else `is_open = true`; build and (when not empty) show; a build or host error clears the
    /// choice and goes to `report_menu_failed`; `is_open = false` in every case; THEN the command of the chosen id runs, guarded, with the menu closed.
    /// No `RefCell` borrow is held while the host runs.
    pub fn show(&self, screen_point_dip: PointD) {
        if self.is_open.get() {
            return;
        }
        let chosen = {
            // The flag is cleared on every exit of this block (C# `finally`), a panic of the host included.
            let _open = OpenGuard::set(&self.is_open);
            match self.build_model() {
                Ok(items) if items.is_empty() => None,
                Ok(items) => {
                    // Cloned out of the `RefCell` first: the modal host re-enters the window procedure (D-W8B4I-1).
                    let host = self.menu_host();
                    match host.show(&items, screen_point_dip) {
                        Ok(chosen) => chosen,
                        Err(error) => {
                            self.actions.report_menu_failed(&*error);
                            None
                        }
                    }
                }
                Err(error) => {
                    self.actions.report_menu_failed(&error);
                    None
                }
            }
        };
        if let Some(id) = chosen {
            self.guarded(&id, || self.execute(&id));
        }
    }

    /// C# `ExecuteAsync(id)`: the command of a model id (names as in `ContextMenuModelBuilder`). Unknown ids and separators do nothing (`Ok`).
    /// Commands marked guarded in the C# switch report their own failure (`report_action_failed(zoom_action_name, error)`) and return `Ok`.
    pub fn execute(&self, id: &str) -> ActionResult {
        let actions = &*self.actions;
        match id {
            "Undo" => actions.execute(ReviewCommand::UNDO),
            "MoveToRecycleBin" => actions.execute(ReviewCommand::RECYCLE),
            "Fit" => actions.fit(),
            "ZoomToLevel" => self.guard_zoom(|| actions.zoom_to_configured_level()),
            "ZoomFitWidth" => self.guard_zoom(|| actions.execute(ReviewCommand::FIT_WIDTH)),
            "ZoomFitWidth2" => self.guard_zoom(|| actions.execute(ReviewCommand::FIT_WIDTH2)),
            "ZoomFitHeight" => self.guard_zoom(|| actions.execute(ReviewCommand::FIT_HEIGHT)),
            "Refresh" => actions.execute(ReviewCommand::REFRESH),
            "OpenFolder" => actions.execute(ReviewCommand::OPEN_FOLDER),
            "NextFolder" => actions.execute(ReviewCommand::NEXT_FOLDER),
            "PreviousFolder" => actions.execute(ReviewCommand::PREVIOUS_FOLDER),
            "ExternalEditor" => self.run(|| actions.open_in_external_editor()),
            "RenameFile" => actions.execute(ReviewCommand::RENAME_FILE),
            "CopyFileName" => self.run(|| actions.copy_file_name()),
            "CopyFullPath" => self.run(|| actions.copy_full_pathname()),
            "Settings" => self.run(|| actions.show_settings()),
            "Zoom.Custom" => self.guard_zoom(|| actions.custom_zoom()),
            "Zoom.AlsoSetClickLevel" => {
                let current = actions.settings().set_zoom_also_sets_click_level;
                actions.set_zoom_also_sets_click_level(!current);
                Ok(())
            }
            "Zoom.SetCurrentAsClickLevel" => {
                if let Some(percent) = zoom_rules::calculate_click_level_from_effective_zoom(actions.effective_zoom()) {
                    actions.set_click_zoom_percent(percent);
                }
                Ok(())
            }
            _ => match try_parse_preset(id) {
                Some(percent) => self.guard_zoom(|| actions.apply_zoom_preset(percent)),
                None => Ok(()),
            },
        }
    }

    /// C# `GuardedAsync(actionName, action)`: a failure goes to the log and to `report_action_failed`; it never escapes.
    fn guarded(&self, action_name: &str, action: impl FnOnce() -> ActionResult) {
        if let Err(error) = action() {
            if let Some(log) = &self.log {
                log.error(&format!("Context menu command {action_name} failed"), Some(&*error));
            }
            self.actions.report_action_failed(action_name, &*error);
        }
    }

    /// A command guarded with the localized name of the zoom commands (`Tr.MainMenuZoom`), read at call time.
    fn guard_zoom(&self, action: impl FnOnce() -> ActionResult) -> ActionResult {
        self.guarded(&self.actions.zoom_action_name(), action);
        Ok(())
    }

    /// C# `Run(action)`: guarded with the EMPTY action name.
    fn run(&self, action: impl FnOnce() -> ActionResult) -> ActionResult {
        self.guarded("", action);
        Ok(())
    }
}

/// Sets the open flag and clears it again when dropped (C# `try`/`finally`).
struct OpenGuard<'a>(&'a Cell<bool>);

impl<'a> OpenGuard<'a> {
    fn set(flag: &'a Cell<bool>) -> Self {
        flag.set(true);
        Self(flag)
    }
}

impl Drop for OpenGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl fmt::Debug for ContextMenuController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContextMenuController")
            .field("is_open", &self.is_open.get())
            .finish_non_exhaustive()
    }
}

/// C# `TryParsePreset(id, out percent)`: `Zoom.Preset.` followed by digits only (`NumberStyles.None`, invariant); no sign, no space, no overflow.
pub fn try_parse_preset(id: &str) -> Option<i32> {
    let digits = id.strip_prefix(ZOOM_PRESET_PREFIX)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i32>().ok()
}
