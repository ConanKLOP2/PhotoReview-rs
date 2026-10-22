//! W9-E2: port of `PhotoReview.Shell.Win32/Dialogs/Native/ExternalEditorsDialog.cs` (408 lines, baseline `7eaee2a2`): the record `ExternalEditorsEdit` and the
//! MODAL external editors manager opened from the Files settings page ("Manage editors..."). Contract: `contracts/W9-E2.md`. Same shape as
//! `dialogs::action_profiles` (W8-18q): pure helpers, a `DialogContent` and a wrapper. Every body is `todo!()`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use pr_core::settings::external_editors::ExternalEditor;

use crate::dialogs::action_profiles::DialogWarningSink;
use crate::dialogs::file_dialog::ShellFileDialog;
use crate::dialogs::native::chrome::NativeDialogChrome;
use crate::dialogs::native::dialog::{DialogContent, ID_CANCEL, ID_OK, ListItemText, NativeDialog};
use crate::dialogs::native::template::DialogItem;
use crate::dialogs::zoom_prompt::{ButtonRowPlan, DialogMeasure, Placement};

/// C# `internal sealed record ExternalEditorsEdit(IReadOnlyList<ExternalEditor> Editors, string DefaultId)`: the result of the dialog after Apply. Lives HERE
/// (not in `pr-core`): in C# it is internal to `Shell.Win32`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalEditorsEdit {
    /// C# `Editors`: the edited list (copies).
    pub editors: Vec<ExternalEditor>,
    /// C# `DefaultId`: `EffectiveDefaultId()` (the stored id when it is in the list, else the first editor's id, else `""`).
    pub default_id: String,
}

/// C# `ListHeaderId`.
pub const LIST_HEADER_ID: i32 = 100;
/// C# `ListId` (owner-draw list box; the badge is `externalEditors.defaultBadge` on the default row).
pub const LIST_ID: i32 = 101;
/// C# `AddId` ("+ Add": picks an executable).
pub const ADD_ID: i32 = 102;
/// C# `RemoveId`.
pub const REMOVE_ID: i32 = 103;
/// C# `SetDefaultId`.
pub const SET_DEFAULT_ID: i32 = 104;
/// C# `DetailsHeaderId`.
pub const DETAILS_HEADER_ID: i32 = 106;
/// C# `NameLabelId`.
pub const NAME_LABEL_ID: i32 = 110;
/// C# `NameId`.
pub const NAME_ID: i32 = 111;
/// C# `PathLabelId`.
pub const PATH_LABEL_ID: i32 = 112;
/// C# `PathId`.
pub const PATH_ID: i32 = 113;
/// C# `BrowseId`.
pub const BROWSE_ID: i32 = 114;
/// C# `ArgumentsLabelId`.
pub const ARGUMENTS_LABEL_ID: i32 = 116;
/// C# `ArgumentsId`.
pub const ARGUMENTS_ID: i32 = 117;
/// C# `HintId`.
pub const HINT_ID: i32 = 118;
/// C# `CancelId` = `ID_CANCEL`.
pub const CANCEL_ID: i32 = ID_CANCEL;
/// C# `ApplyId` = `ID_OK`.
pub const APPLY_ID: i32 = ID_OK;

/// C# `BuildItems()` (static, pure): the 16 controls in Tab order (see the contract for the exact styles): Static list header, owner-draw ListBox, Buttons Add /
/// Remove / Set default, Static details header, Static + Edit name, Static + Edit path, Button Browse, Static + Edit arguments, Static hint, Buttons Cancel /
/// Apply (`BS_DEFPUSHBUTTON`). Texts: `externalEditors.*` and `common.cancel` / `common.apply` of the ambient language.
pub fn build_items() -> Vec<DialogItem> {
    todo!()
}

/// C# `ListItemText(ListId, index)` in range, pure: `Some(ListItemText { text: editors[index].display_name(), badge: Some(externalEditors.defaultBadge) when
/// index is the index of the effective default, else None })`; out of range -> `None` (the caller answers like the base class).
pub fn row_text_of(editors: &[ExternalEditor], default_id: &str, index: i32) -> Option<ListItemText> {
    todo!()
}

/// What `layout_plan` decided, in the C# call order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalEditorsLayoutPlan {
    /// Cancel / Apply at the bottom (C# calls it FIRST).
    pub button_row: ButtonRowPlan,
    /// Every `Place`, in order.
    pub placements: Vec<Placement>,
    /// `SetListItemHeight(ListId, ..)` right after the list is placed: `line_height + dy(5)`.
    pub list_item_height: i32,
}

/// C# `Margin` = `dx(10)`.
pub fn margin(m: &dyn DialogMeasure) -> i32 {
    todo!()
}

/// C# `FieldHeight` and `ButtonHeight` (equal): `max(dy(14), line_height + dy(6))`.
pub fn field_height(m: &dyn DialogMeasure) -> i32 {
    todo!()
}

/// C# `LeftWidth(clientWidth)`: `(client_width / 3)` clamped to `[dx(100), dx(170)]` (written with `max`/`min`, not `i32::clamp`).
pub fn left_width(m: &dyn DialogMeasure, client_width: i32) -> i32 {
    todo!()
}

/// C# `PreferredClientSize()` = `(dx(470), dy(190))`.
pub fn preferred_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    todo!()
}

/// C# `MinimumClientSize()` = `(max(dx(380), button_row_width([CANCEL_ID, APPLY_ID]) + 2 * margin), dy(170))`.
pub fn minimum_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    todo!()
}

/// C# `LayoutControls(clientWidth, clientHeight)` (contract has the arithmetic).
pub fn layout_plan(m: &dyn DialogMeasure, client_width: i32, client_height: i32) -> ExternalEditorsLayoutPlan {
    todo!()
}

/// Runs the plan: the button row first, then every placement, with `set_list_item_height` right after the placement of `LIST_ID`.
pub fn apply_layout(dialog: &NativeDialog, plan: &ExternalEditorsLayoutPlan) {
    todo!()
}

/// `Func<string, string?>` of C# `suggestName` (a path -> a suggested name, `None` = use the file name).
pub type SuggestNameFn = Rc<dyn Fn(&str) -> Option<String>>;

/// The `DialogContent` of the editors dialog. Working copy of the list in a `RefCell` borrowed ONLY for one call; every control read happens BEFORE `borrow_mut`
/// and every control write AFTER it, and a modal warning or file dialog never runs under a borrow (same rule as `ActionProfilesContent`).
pub struct ExternalEditorsContent {
    editors: RefCell<Vec<ExternalEditor>>,
    default_id: RefCell<String>,
    /// C# `_open`: index of the row whose fields are in the edit boxes, `-1` = none.
    open: RefCell<i32>,
    files: Option<Rc<dyn ShellFileDialog>>,
    warnings: Rc<dyn DialogWarningSink>,
    suggest_name: SuggestNameFn,
    items: Vec<DialogItem>,
}

impl fmt::Debug for ExternalEditorsContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalEditorsContent").finish_non_exhaustive()
    }
}

impl ExternalEditorsContent {
    /// C# constructor: copies `editors` (`Clone`), `default_id` `None` -> `""`, `warnings` default `TaskDialogWarningSink::instance()`, `suggest_name` default
    /// `|_| None`, `items = build_items()`.
    pub fn new(
        editors: &[ExternalEditor],
        default_id: Option<&str>,
        files: Option<Rc<dyn ShellFileDialog>>,
        warnings: Option<Rc<dyn DialogWarningSink>>,
        suggest_name: Option<SuggestNameFn>,
    ) -> Self {
        todo!()
    }

    /// C# `Outcome`: copies of the editors and `effective_default_id()`. Valid after Apply (`ID_OK`).
    pub fn outcome(&self) -> ExternalEditorsEdit {
        todo!()
    }

    /// C# `EffectiveDefaultId()`: `default_id` when an editor has it (ordinal), else the first editor's id, else `""`.
    fn effective_default_id(&self) -> String {
        todo!()
    }

    /// C# `SaveCurrent()`: writes name (trimmed), path (trimmed) and arguments (NOT trimmed) of the edit boxes into editor `open`, then `refresh_list`. No-op when
    /// `open` is out of range.
    fn save_current(&self, dialog: &NativeDialog) {
        todo!()
    }

    /// C# `AddEditor()`: `save_current`; `pick_executable("")`; a blank result or a full list (`MAX_EDITORS`) -> nothing. Else a new `ExternalEditor` with that path and
    /// `suggest_name(path)` or `name_from_path(path)`; the first editor of an empty list becomes the default; `fill_list`; select the last row.
    fn add_editor(&self, dialog: &NativeDialog) {
        todo!()
    }

    /// C# `RemoveEditor()`: nothing without a selection; `open = -1` BEFORE the removal (the removed row's fields must not be written back); removing the default
    /// makes the first remaining editor the default (`""` when none); `fill_list`; selects `min(index, count - 1)` or clears the three edit boxes.
    fn remove_editor(&self, dialog: &NativeDialog) {
        todo!()
    }

    /// C# `SetDefault()`: `save_current`; the selected row's id becomes `default_id`; `refresh_list`. Nothing without a selection.
    fn set_default(&self, dialog: &NativeDialog) {
        todo!()
    }

    /// C# `Browse()`: `pick_executable(path box text)`; a non-blank result is written into the path box (`save_current` runs on the next selection change / Apply).
    fn browse(&self, dialog: &NativeDialog) {
        todo!()
    }

    /// C# `PickExecutable(currentPath)`: no file dialog -> `None`. Initial folder = the parent folder of `current_path` when it is an existing file, else `None`
    /// (any path error is swallowed). `files.pick_open_file(hwnd, dialog.externalEditor.addTitle, FileFilterText::parse(dialog.fileFilter.executable), initial)`:
    /// `Ok(x)` -> `x`; `Err` -> `warnings.show_warning(hwnd, dialog.settingsOpenFailed.title, text)` and `None`.
    fn pick_executable(&self, dialog: &NativeDialog, current_path: &str) -> Option<String> {
        todo!()
    }
}

impl DialogContent for ExternalEditorsContent {
    /// `externalEditors.title`.
    fn title(&self) -> String {
        todo!()
    }
    /// The list built in `new` (cloned).
    fn items(&self) -> Vec<DialogItem> {
        todo!()
    }
    /// C# `Resizable` = `true`.
    fn resizable(&self) -> bool {
        todo!()
    }
    /// C# `HeaderControlIds` = `[LIST_HEADER_ID, DETAILS_HEADER_ID]`.
    fn header_control_ids(&self) -> Vec<i32> {
        todo!()
    }
    /// C# `InitialFocusId` = `LIST_ID`.
    fn initial_focus_id(&self) -> i32 {
        todo!()
    }
    /// C# `DefaultButtonId` = `APPLY_ID`.
    fn default_button_id(&self) -> i32 {
        todo!()
    }
    /// `preferred_client_size(dialog)`.
    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        todo!()
    }
    /// `minimum_client_size(dialog)`.
    fn minimum_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        todo!()
    }
    /// `apply_layout(dialog, &layout_plan(dialog, client_width, client_height))`.
    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, client_height: i32) {
        todo!()
    }
    /// C# `OnInitialized()`: `fill_list`; with at least one editor `select_index(index of the default, or 0 when none)`.
    fn on_initialized(&self, dialog: &NativeDialog) {
        todo!()
    }
    /// C# `OnCommand(id, notification)`: `LIST_ID` + `LBN_SELCHANGE` -> selection changed (`save_current`, then show the selected row); `ADD_ID`, `REMOVE_ID`,
    /// `SET_DEFAULT_ID`, `BROWSE_ID` + `BN_CLICKED` -> the matching method; each returns `true`; anything else `false`.
    fn on_command(&self, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        todo!()
    }
    /// C# `OnOk()`: `save_current`; the first editor with a blank (`trim().is_empty()`) path is selected and
    /// `warnings.show_warning(hwnd, externalEditors.title, dialog.externalEditor.exeMissing {path = ""})`, returns `false` (stay open); else `true`.
    fn on_ok(&self, dialog: &NativeDialog) -> bool {
        todo!()
    }
    /// C# `ListItemText(id, index)`: `LIST_ID` with `row_text_of(..)` `Some` -> that; else the base answer.
    fn list_item_text(&self, dialog: &NativeDialog, id: i32, index: i32) -> ListItemText {
        todo!()
    }
}

/// The editors dialog (C# `ExternalEditorsDialog : NativeDialog`). Modal: `native().show_modal(owner, ready, loop)`; `Ok(ID_OK)` means `outcome()` is the result.
pub struct ExternalEditorsDialog {
    native: NativeDialog,
    content: Rc<ExternalEditorsContent>,
}

impl fmt::Debug for ExternalEditorsDialog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalEditorsDialog")
            .field("native", &self.native)
            .finish_non_exhaustive()
    }
}

impl ExternalEditorsDialog {
    /// C# `ExternalEditorsDialog(editors, defaultId, files = null, warnings = null, suggestName = null, chrome = null)`.
    pub fn new(
        editors: &[ExternalEditor],
        default_id: Option<&str>,
        files: Option<Rc<dyn ShellFileDialog>>,
        warnings: Option<Rc<dyn DialogWarningSink>>,
        suggest_name: Option<SuggestNameFn>,
        chrome: Option<Rc<dyn NativeDialogChrome>>,
    ) -> Self {
        todo!()
    }

    /// The base-class handle.
    pub fn native(&self) -> &NativeDialog {
        &self.native
    }

    /// C# `Outcome`.
    pub fn outcome(&self) -> ExternalEditorsEdit {
        self.content.outcome()
    }

    /// C# `RowText(index)` (test seam): text and badge of row `index`.
    pub fn row_text(&self, index: i32) -> ListItemText {
        todo!()
    }
}
