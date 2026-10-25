//! W9-E2: port of `PhotoReview.Shell.Win32/Dialogs/Native/ExternalEditorsDialog.cs` (408 lines, baseline `7eaee2a2`): the record `ExternalEditorsEdit` and the
//! MODAL external editors manager opened from the Files settings page ("Manage editors..."). Contract: `contracts/W9-E2.md`. Same shape as
//! `dialogs::action_profiles` (W8-18q): pure helpers, a `DialogContent` and a wrapper.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use pr_core::settings::external_editors::{ExternalEditor, ExternalEditors};
use pr_l10n::tr;

use crate::dialogs::action_profiles::{DialogWarningSink, FileFilterText, TaskDialogWarningSink};
use crate::dialogs::file_dialog::ShellFileDialog;
use crate::dialogs::native::chrome::NativeDialogChrome;
use crate::dialogs::native::controls::basic::ListBoxControl;
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
    use super::template::{DIALOG_STYLE_CHILD as CHILD, DialogControlClass as C};
    use crate::win32::dialog_native as n;
    let text = CHILD | n::SS_NOPREFIX;
    let edit = CHILD | n::WS_TABSTOP | n::ES_AUTOHSCROLL;
    let button = CHILD | n::WS_TABSTOP | n::BS_PUSHBUTTON;
    vec![
        DialogItem::new(C::Static, LIST_HEADER_ID, tr::external_editors_list_header(), text),
        DialogItem::new(
            C::ListBox,
            LIST_ID,
            "",
            CHILD
                | n::WS_TABSTOP
                | n::WS_VSCROLL
                | n::LBS_NOTIFY
                | n::LBS_NOINTEGRALHEIGHT
                | crate::win32::control_constants::LBS_OWNERDRAWFIXED
                | crate::win32::control_constants::LBS_HASSTRINGS,
        )
        .with_ex_style(n::WS_EX_CLIENTEDGE),
        DialogItem::new(C::Button, ADD_ID, tr::external_editors_add(), button),
        DialogItem::new(C::Button, REMOVE_ID, tr::external_editors_remove(), button),
        DialogItem::new(C::Button, SET_DEFAULT_ID, tr::external_editors_set_default(), button),
        DialogItem::new(
            C::Static,
            DETAILS_HEADER_ID,
            tr::external_editors_details_header(),
            text,
        ),
        DialogItem::new(C::Static, NAME_LABEL_ID, tr::external_editors_name_label(), text),
        DialogItem::new(C::Edit, NAME_ID, "", edit).with_ex_style(n::WS_EX_CLIENTEDGE),
        DialogItem::new(C::Static, PATH_LABEL_ID, tr::external_editors_path_label(), text),
        DialogItem::new(C::Edit, PATH_ID, "", edit).with_ex_style(n::WS_EX_CLIENTEDGE),
        DialogItem::new(C::Button, BROWSE_ID, tr::external_editors_browse(), button),
        DialogItem::new(
            C::Static,
            ARGUMENTS_LABEL_ID,
            tr::external_editors_arguments_label(),
            text,
        ),
        DialogItem::new(C::Edit, ARGUMENTS_ID, "", edit).with_ex_style(n::WS_EX_CLIENTEDGE),
        DialogItem::new(C::Static, HINT_ID, tr::external_editors_arguments_hint(), text),
        DialogItem::new(C::Button, CANCEL_ID, tr::common_cancel(), button),
        DialogItem::new(
            C::Button,
            APPLY_ID,
            tr::common_apply(),
            CHILD | n::WS_TABSTOP | n::BS_DEFPUSHBUTTON,
        ),
    ]
}

/// C# `ListItemText(ListId, index)` in range, pure: `Some(ListItemText { text: editors[index].display_name(), badge: Some(externalEditors.defaultBadge) when
/// index is the index of the effective default, else None })`; out of range -> `None` (the caller answers like the base class).
pub fn row_text_of(editors: &[ExternalEditor], default_id: &str, index: i32) -> Option<ListItemText> {
    let index = usize::try_from(index).ok()?;
    let editor = editors.get(index)?;
    let default_index = editors.iter().position(|editor| editor.id == default_id).unwrap_or(0);
    Some(ListItemText {
        text: editor.display_name(),
        badge: (index == default_index).then(tr::external_editors_default_badge),
    })
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
    m.dx(10)
}

/// C# `FieldHeight` and `ButtonHeight` (equal): `max(dy(14), line_height + dy(6))`.
pub fn field_height(m: &dyn DialogMeasure) -> i32 {
    m.dy(14).max(m.line_height().wrapping_add(m.dy(6)))
}

/// C# `LeftWidth(clientWidth)`: `(client_width / 3)` clamped to `[dx(100), dx(170)]` (written with `max`/`min`, not `i32::clamp`).
pub fn left_width(m: &dyn DialogMeasure, client_width: i32) -> i32 {
    (client_width / 3).max(m.dx(100)).min(m.dx(170))
}

/// C# `PreferredClientSize()` = `(dx(470), dy(190))`.
pub fn preferred_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    (m.dx(470), m.dy(190))
}

/// C# `MinimumClientSize()` = `(max(dx(380), button_row_width([CANCEL_ID, APPLY_ID]) + 2 * margin), dy(170))`.
pub fn minimum_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    (
        m.dx(380).max(
            m.button_row_width(&[CANCEL_ID, APPLY_ID])
                .wrapping_add(margin(m).wrapping_mul(2)),
        ),
        m.dy(170),
    )
}

/// C# `LayoutControls(clientWidth, clientHeight)` (contract has the arithmetic).
pub fn layout_plan(m: &dyn DialogMeasure, client_width: i32, client_height: i32) -> ExternalEditorsLayoutPlan {
    let margin = margin(m);
    let left = left_width(m, client_width);
    let right_x = margin.wrapping_add(left).wrapping_add(margin);
    let right = m.dx(60).max(client_width.wrapping_sub(right_x).wrapping_sub(margin));
    let height = field_height(m);
    let bottom = client_height.wrapping_sub(margin).wrapping_sub(height);
    let content_bottom = bottom.wrapping_sub(m.dy(8));
    let header = m.measure_item_height(LIST_HEADER_ID, left);
    let list_y = margin.wrapping_add(header).wrapping_add(m.dy(4));
    let gap = m.dx(4);
    let half = left.wrapping_sub(gap) / 2;
    let row2 = content_bottom.wrapping_sub(height);
    let row1 = row2.wrapping_sub(m.dy(4)).wrapping_sub(height);
    let mut placements = vec![
        Placement::new(LIST_HEADER_ID, margin, margin, left, header),
        Placement::new(
            LIST_ID,
            margin,
            list_y,
            left,
            m.dy(30).max(row1.wrapping_sub(m.dy(4)).wrapping_sub(list_y)),
        ),
        Placement::new(ADD_ID, margin, row1, half, height),
        Placement::new(
            REMOVE_ID,
            margin.wrapping_add(half).wrapping_add(gap),
            row1,
            left.wrapping_sub(half).wrapping_sub(gap),
            height,
        ),
        Placement::new(SET_DEFAULT_ID, margin, row2, left, height),
    ];
    let details = m.measure_item_height(DETAILS_HEADER_ID, right);
    placements.push(Placement::new(DETAILS_HEADER_ID, right_x, margin, right, details));
    let mut y = margin.wrapping_add(details).wrapping_add(m.dy(8));
    let field = |label_id, edit_id, y: &mut i32, placements: &mut Vec<Placement>| {
        let label = m.measure_item_height(label_id, right);
        placements.push(Placement::new(label_id, right_x, *y, right, label));
        *y = y.wrapping_add(label).wrapping_add(m.dy(2));
        placements.push(Placement::new(edit_id, right_x, *y, right, height));
        *y = y.wrapping_add(height).wrapping_add(m.dy(8));
    };
    field(NAME_LABEL_ID, NAME_ID, &mut y, &mut placements);
    let path_label = m.measure_item_height(PATH_LABEL_ID, right);
    placements.push(Placement::new(PATH_LABEL_ID, right_x, y, right, path_label));
    y = y.wrapping_add(path_label).wrapping_add(m.dy(2));
    let browse = m
        .dx(48)
        .max(m.measure_text_width(&m.item_text(BROWSE_ID)).wrapping_add(m.dx(14)));
    placements.push(Placement::new(
        PATH_ID,
        right_x,
        y,
        m.dx(40).max(right.wrapping_sub(browse).wrapping_sub(gap)),
        height,
    ));
    placements.push(Placement::new(
        BROWSE_ID,
        right_x.wrapping_add(right).wrapping_sub(browse),
        y,
        browse,
        height,
    ));
    y = y.wrapping_add(height).wrapping_add(m.dy(8));
    field(ARGUMENTS_LABEL_ID, ARGUMENTS_ID, &mut y, &mut placements);
    placements.push(Placement::new(
        HINT_ID,
        right_x,
        y.wrapping_sub(m.dy(4)),
        right,
        m.measure_item_height(HINT_ID, right),
    ));
    ExternalEditorsLayoutPlan {
        button_row: ButtonRowPlan {
            client_width,
            right_margin: margin,
            top: bottom,
            height,
            ids: vec![CANCEL_ID, APPLY_ID],
        },
        placements,
        list_item_height: m.line_height().wrapping_add(m.dy(5)),
    }
}

/// Runs the plan: the button row first, then every placement, with `set_list_item_height` right after the placement of `LIST_ID`.
pub fn apply_layout(dialog: &NativeDialog, plan: &ExternalEditorsLayoutPlan) {
    crate::dialogs::zoom_prompt::apply_button_row(dialog, &plan.button_row);
    for placement in &plan.placements {
        crate::dialogs::zoom_prompt::apply_placements(dialog, std::slice::from_ref(placement));
        if placement.id == LIST_ID {
            dialog.set_list_item_height(LIST_ID, plan.list_item_height);
        }
    }
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
        Self {
            editors: RefCell::new(editors.to_vec()),
            default_id: RefCell::new(default_id.unwrap_or("").to_owned()),
            open: RefCell::new(-1),
            files,
            warnings: warnings.unwrap_or_else(TaskDialogWarningSink::instance),
            suggest_name: suggest_name.unwrap_or_else(|| Rc::new(|_| None)),
            items: build_items(),
        }
    }

    /// C# `Outcome`: copies of the editors and `effective_default_id()`. Valid after Apply (`ID_OK`).
    pub fn outcome(&self) -> ExternalEditorsEdit {
        ExternalEditorsEdit {
            editors: self.editors.borrow().clone(),
            default_id: self.effective_default_id(),
        }
    }

    /// C# `EffectiveDefaultId()`: `default_id` when an editor has it (ordinal), else the first editor's id, else `""`.
    fn effective_default_id(&self) -> String {
        let id = self.default_id.borrow();
        let editors = self.editors.borrow();
        if editors.iter().any(|editor| editor.id == *id) {
            id.clone()
        } else {
            editors.first().map_or_else(String::new, |editor| editor.id.clone())
        }
    }

    fn fill_list(&self, dialog: &NativeDialog) {
        let list = dialog.item(LIST_ID);
        // SAFETY: LB_GETTOPINDEX is a scalar query on the live list control.
        let top = unsafe { crate::win32::dialog_native::user32::send_message(list, 0x018e, 0, 0) } as i32;
        let names: Vec<_> = self.editors.borrow().iter().map(ExternalEditor::display_name).collect();
        ListBoxControl::clear(list);
        for name in &names {
            ListBoxControl::add(list, name);
        }
        if top > 0 && (top as usize) < names.len() {
            // SAFETY: LB_SETTOPINDEX takes a validated scalar index and no pointers.
            unsafe {
                crate::win32::dialog_native::user32::send_message(list, 0x0197, top as usize, 0);
            }
        }
    }

    fn refresh_list(&self, dialog: &NativeDialog) {
        let index = ListBoxControl::selected_index(dialog.item(LIST_ID));
        self.fill_list(dialog);
        let valid = index >= 0 && (index as usize) < self.editors.borrow().len();
        if valid {
            ListBoxControl::select_single(dialog.item(LIST_ID), index);
        }
    }

    fn select_index(&self, dialog: &NativeDialog, index: i32) {
        ListBoxControl::select_single(dialog.item(LIST_ID), index);
        self.on_selection_changed(dialog);
    }

    fn on_selection_changed(&self, dialog: &NativeDialog) {
        self.save_current(dialog);
        let index = ListBoxControl::selected_index(dialog.item(LIST_ID));
        let editor = usize::try_from(index)
            .ok()
            .and_then(|i| self.editors.borrow().get(i).cloned());
        if let Some(editor) = editor {
            *self.open.borrow_mut() = index;
            dialog.set_item_text(NAME_ID, &editor.name);
            dialog.set_item_text(PATH_ID, &editor.path);
            dialog.set_item_text(ARGUMENTS_ID, &editor.arguments);
        }
    }

    /// C# `SaveCurrent()`: writes name (trimmed), path (trimmed) and arguments (NOT trimmed) of the edit boxes into editor `open`, then `refresh_list`. No-op when
    /// `open` is out of range.
    fn save_current(&self, dialog: &NativeDialog) {
        let Ok(index) = usize::try_from(*self.open.borrow()) else {
            return;
        };
        if index >= self.editors.borrow().len() {
            return;
        }
        let name = dialog.get_item_text(NAME_ID).trim().to_owned();
        let path = dialog.get_item_text(PATH_ID).trim().to_owned();
        let arguments = dialog.get_item_text(ARGUMENTS_ID);
        {
            let mut editors = self.editors.borrow_mut();
            let Some(editor) = editors.get_mut(index) else {
                return;
            };
            editor.name = name;
            editor.path = path;
            editor.arguments = arguments;
        }
        self.refresh_list(dialog);
    }

    /// C# `AddEditor()`: `save_current`; `pick_executable("")`; a blank result or a full list (`MAX_EDITORS`) -> nothing. Else a new `ExternalEditor` with that path and
    /// `suggest_name(path)` or `name_from_path(path)`; the first editor of an empty list becomes the default; `fill_list`; select the last row.
    fn add_editor(&self, dialog: &NativeDialog) {
        self.save_current(dialog);
        let Some(path) = self.pick_executable(dialog, "").filter(|path| !path.trim().is_empty()) else {
            return;
        };
        if self.editors.borrow().len() >= ExternalEditors::MAX_EDITORS {
            return;
        }
        let name = (self.suggest_name)(&path).unwrap_or_else(|| ExternalEditors::name_from_path(Some(&path)));
        let editor = ExternalEditor {
            name,
            path,
            ..ExternalEditor::new()
        };
        let id = editor.id.clone();
        let count = {
            let mut editors = self.editors.borrow_mut();
            editors.push(editor);
            editors.len()
        };
        if count == 1 {
            *self.default_id.borrow_mut() = id;
        }
        self.fill_list(dialog);
        self.select_index(dialog, (count - 1) as i32);
    }

    /// C# `RemoveEditor()`: nothing without a selection; `open = -1` BEFORE the removal (the removed row's fields must not be written back); removing the default
    /// makes the first remaining editor the default (`""` when none); `fill_list`; selects `min(index, count - 1)` or clears the three edit boxes.
    fn remove_editor(&self, dialog: &NativeDialog) {
        let Ok(index) = usize::try_from(ListBoxControl::selected_index(dialog.item(LIST_ID))) else {
            return;
        };
        let (removed, first, count) = {
            let mut editors = self.editors.borrow_mut();
            if index >= editors.len() {
                return;
            }
            *self.open.borrow_mut() = -1;
            let removed = editors.remove(index);
            (
                removed,
                editors.first().map_or_else(String::new, |editor| editor.id.clone()),
                editors.len(),
            )
        };
        if removed.id == *self.default_id.borrow() {
            *self.default_id.borrow_mut() = first;
        }
        self.fill_list(dialog);
        if count > 0 {
            self.select_index(dialog, index.min(count - 1) as i32);
        } else {
            for id in [NAME_ID, PATH_ID, ARGUMENTS_ID] {
                dialog.set_item_text(id, "");
            }
        }
    }

    /// C# `SetDefault()`: `save_current`; the selected row's id becomes `default_id`; `refresh_list`. Nothing without a selection.
    fn set_default(&self, dialog: &NativeDialog) {
        self.save_current(dialog);
        let index = ListBoxControl::selected_index(dialog.item(LIST_ID));
        let id = usize::try_from(index)
            .ok()
            .and_then(|i| self.editors.borrow().get(i).map(|editor| editor.id.clone()));
        if let Some(id) = id {
            *self.default_id.borrow_mut() = id;
            self.refresh_list(dialog);
        }
    }

    /// C# `Browse()`: `pick_executable(path box text)`; a non-blank result is written into the path box (`save_current` runs on the next selection change / Apply).
    fn browse(&self, dialog: &NativeDialog) {
        let current = dialog.get_item_text(PATH_ID);
        if let Some(path) = self
            .pick_executable(dialog, &current)
            .filter(|path| !path.trim().is_empty())
        {
            dialog.set_item_text(PATH_ID, &path);
        }
    }

    /// C# `PickExecutable(currentPath)`: no file dialog -> `None`. Initial folder = the parent folder of `current_path` when it is an existing file, else `None`
    /// (any path error is swallowed). `files.pick_open_file(hwnd, dialog.externalEditor.addTitle, FileFilterText::parse(dialog.fileFilter.executable), initial)`:
    /// `Ok(x)` -> `x`; `Err` -> `warnings.show_warning(hwnd, dialog.settingsOpenFailed.title, text)` and `None`.
    fn pick_executable(&self, dialog: &NativeDialog, current_path: &str) -> Option<String> {
        let files = self.files.as_ref()?;
        let initial = if !current_path.trim().is_empty() && std::path::Path::new(current_path).is_file() {
            pr_core::text::dotnet_path::get_directory_name(current_path)
        } else {
            None
        };
        match files.pick_open_file(
            dialog.hwnd(),
            &tr::dialog_external_editor_add_title(),
            &FileFilterText::parse(&tr::dialog_file_filter_executable()),
            initial.as_deref(),
        ) {
            Ok(path) => path,
            Err(error) => {
                let message = windows::core::Error::from_hresult(windows::core::HRESULT(error.0)).message();
                self.warnings
                    .show_warning(dialog.hwnd(), &tr::dialog_settings_open_failed_title(), &message);
                None
            }
        }
    }
}

impl DialogContent for ExternalEditorsContent {
    /// `externalEditors.title`.
    fn title(&self) -> String {
        tr::external_editors_title()
    }
    /// The list built in `new` (cloned).
    fn items(&self) -> Vec<DialogItem> {
        self.items.clone()
    }
    /// C# `Resizable` = `true`.
    fn resizable(&self) -> bool {
        true
    }
    /// C# `HeaderControlIds` = `[LIST_HEADER_ID, DETAILS_HEADER_ID]`.
    fn header_control_ids(&self) -> Vec<i32> {
        vec![LIST_HEADER_ID, DETAILS_HEADER_ID]
    }
    /// C# `InitialFocusId` = `LIST_ID`.
    fn initial_focus_id(&self) -> i32 {
        LIST_ID
    }
    /// C# `DefaultButtonId` = `APPLY_ID`.
    fn default_button_id(&self) -> i32 {
        APPLY_ID
    }
    /// `preferred_client_size(dialog)`.
    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        preferred_client_size(dialog)
    }
    /// `minimum_client_size(dialog)`.
    fn minimum_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        minimum_client_size(dialog)
    }
    /// `apply_layout(dialog, &layout_plan(dialog, client_width, client_height))`.
    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, client_height: i32) {
        apply_layout(dialog, &layout_plan(dialog, client_width, client_height));
    }
    /// C# `OnInitialized()`: `fill_list`; with at least one editor `select_index(index of the default, or 0 when none)`.
    fn on_initialized(&self, dialog: &NativeDialog) {
        self.fill_list(dialog);
        let default = self.effective_default_id();
        let index = self.editors.borrow().iter().position(|editor| editor.id == default);
        if let Some(index) = index {
            self.select_index(dialog, index as i32);
        }
    }
    /// C# `OnCommand(id, notification)`: `LIST_ID` + `LBN_SELCHANGE` -> selection changed (`save_current`, then show the selected row); `ADD_ID`, `REMOVE_ID`,
    /// `SET_DEFAULT_ID`, `BROWSE_ID` + `BN_CLICKED` -> the matching method; each returns `true`; anything else `false`.
    fn on_command(&self, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        if id == LIST_ID && notification == crate::win32::control_constants::LBN_SELCHANGE {
            self.on_selection_changed(dialog);
            return true;
        }
        if notification != 0 {
            return false;
        }
        match id {
            ADD_ID => self.add_editor(dialog),
            REMOVE_ID => self.remove_editor(dialog),
            SET_DEFAULT_ID => self.set_default(dialog),
            BROWSE_ID => self.browse(dialog),
            _ => return false,
        }
        true
    }
    /// C# `OnOk()`: `save_current`; the first editor with a blank (`trim().is_empty()`) path is selected and
    /// `warnings.show_warning(hwnd, externalEditors.title, dialog.externalEditor.exeMissing {path = ""})`, returns `false` (stay open); else `true`.
    fn on_ok(&self, dialog: &NativeDialog) -> bool {
        self.save_current(dialog);
        let blank = self
            .editors
            .borrow()
            .iter()
            .position(|editor| editor.path.trim().is_empty());
        if let Some(index) = blank {
            self.select_index(dialog, index as i32);
            self.warnings.show_warning(
                dialog.hwnd(),
                &tr::external_editors_title(),
                &tr::dialog_external_editor_exe_missing(""),
            );
            return false;
        }
        true
    }
    /// C# `ListItemText(id, index)`: `LIST_ID` with `row_text_of(..)` `Some` -> that; else the base answer.
    fn list_item_text(&self, dialog: &NativeDialog, id: i32, index: i32) -> ListItemText {
        if id == LIST_ID {
            let editors = self.editors.borrow().clone();
            let default = self.default_id.borrow().clone();
            if let Some(row) = row_text_of(&editors, &default, index) {
                return row;
            }
        }
        ListItemText {
            text: ListBoxControl::text_at(dialog.item(id), index),
            badge: None,
        }
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
        let content = Rc::new(ExternalEditorsContent::new(
            editors,
            default_id,
            files,
            warnings,
            suggest_name,
        ));
        let native = NativeDialog::with_chrome(content.clone(), chrome);
        Self { native, content }
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
        self.content.list_item_text(&self.native, LIST_ID, index)
    }
}
