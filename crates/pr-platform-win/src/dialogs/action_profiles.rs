//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/ActionProfilesDialog.cs` (414 lines, baseline `d92d6aae`; the file also holds `IDialogWarningSink`,
//! `TaskDialogWarningSink` and `FileFilterText`, which the Settings dialog, its Files page and the secondary window port use too, D-W8B5-503). Unit W8-18q.
//! Contract: `contracts/W8-18q.md`.
//!
//! The Win32 view of `ActionProfilesPresenter` (Settings > Files > "Edit actions..."): a resizable MODAL dialog with the list of actions (owner-draw list box, the
//! badge is the shortcut) and Add / Remove / Import / Export, the detail fields of the open action (name, shortcut, operation = Move / Copy / Recycle combo,
//! destination folder, "always ask" check box) and Cancel / Apply. The presenter owns every rule; the view only copies values between it and the controls and
//! keeps the C# order "save the open row, then open the new one". It can be opened from inside the Settings dialog (modal inside modal).
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use pr_app::presenters::action_profiles::ActionProfilesPresenter;
use pr_core::settings::shortcuts::ReviewAction;
use pr_l10n::localizer::Localizer;
use pr_l10n::tr;

use super::file_dialog::{FileDialogFilter, ShellFileDialog};
use super::native::chrome::NativeDialogChrome;
use super::native::controls::basic::{ComboBoxControl, ListBoxControl};
use super::native::controls::custom::ButtonStateControl;
use super::native::dialog::{DialogContent, ID_CANCEL, ID_OK, ListItemText, NativeDialog};
use super::native::template::DialogItem;
use super::task_dialog::{NativeTaskDialogPort, TaskDialogKind, TaskDialogPort};
use super::zoom_prompt::{ButtonRowPlan, DialogMeasure, Placement, apply_button_row, apply_placements};
use crate::win32::structs::Hwnd;

/// C# `ListHeaderId`.
pub const LIST_HEADER_ID: i32 = 100;
/// C# `ListId`.
pub const LIST_ID: i32 = 101;
/// C# `AddId`.
pub const ADD_ID: i32 = 102;
/// C# `RemoveId`.
pub const REMOVE_ID: i32 = 103;
/// C# `ImportId`.
pub const IMPORT_ID: i32 = 104;
/// C# `ExportId`.
pub const EXPORT_ID: i32 = 105;
/// C# `DetailsHeaderId`.
pub const DETAILS_HEADER_ID: i32 = 106;
/// C# `NameLabelId`.
pub const NAME_LABEL_ID: i32 = 110;
/// C# `NameId`.
pub const NAME_ID: i32 = 111;
/// C# `ShortcutLabelId`.
pub const SHORTCUT_LABEL_ID: i32 = 112;
/// C# `ShortcutId`.
pub const SHORTCUT_ID: i32 = 113;
/// C# `OperationLabelId`.
pub const OPERATION_LABEL_ID: i32 = 114;
/// C# `OperationId`: the Move / Copy / Recycle combo box.
pub const OPERATION_ID: i32 = 115;
/// C# `DestinationLabelId`.
pub const DESTINATION_LABEL_ID: i32 = 116;
/// C# `DestinationId`.
pub const DESTINATION_ID: i32 = 117;
/// C# `ConfirmId`: the multi-line check box.
pub const CONFIRM_ID: i32 = 118;
/// C# `CancelId` = `IdCancel`.
pub const CANCEL_ID: i32 = ID_CANCEL;
/// C# `ApplyId` = `IdOk` (the default button).
pub const APPLY_ID: i32 = ID_OK;

// ---- the three helper types of ActionProfilesDialog.cs ----------------------------------------------------------------------------------------

/// C# `IDialogWarningSink`: the modal warning box of the Win32 views (the real one is a task dialog; the tests replace it with a recorder).
pub trait DialogWarningSink {
    /// C# `ShowWarning(owner, title, message)`: shows a modal warning over `owner` and blocks until the user closes it.
    fn show_warning(&self, owner: Hwnd, title: &str, message: &str);
}

/// C# `TaskDialogWarningSink`: `NativeTaskDialogPort::new().show(owner, title, message, TaskDialogKind::Error)` (warning icon, one OK button), like the WPF
/// `MessageBox`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskDialogWarningSink;

impl TaskDialogWarningSink {
    /// C# `TaskDialogWarningSink.Instance`: a shared value for `Option<Rc<dyn DialogWarningSink>>` parameters.
    pub fn instance() -> Rc<dyn DialogWarningSink> {
        Rc::new(Self)
    }
}

impl DialogWarningSink for TaskDialogWarningSink {
    fn show_warning(&self, owner: Hwnd, title: &str, message: &str) {
        NativeTaskDialogPort::new().show(owner, title, message, TaskDialogKind::Error);
    }
}

/// C# `FileFilterText` (static class): the WPF-style filter string of the language catalog.
#[derive(Debug, Clone, Copy)]
pub struct FileFilterText;

impl FileFilterText {
    /// C# `Parse(text)`: `"Name|pattern|Name|pattern"` to a list of filters. `text.split('|')`; for `i = 0, 2, 4, ...` while `i + 1 < parts.len()`
    /// one `FileDialogFilter { name: parts[i], pattern: parts[i + 1] }`; a trailing name without a pattern is dropped; `""` gives an empty list; empty names
    /// and patterns are kept as they are (no trimming).
    pub fn parse(text: &str) -> Vec<FileDialogFilter> {
        text.split('|')
            .collect::<Vec<_>>()
            .chunks_exact(2)
            .map(|pair| FileDialogFilter {
                name: pair[0].to_owned(),
                pattern: pair[1].to_owned(),
            })
            .collect()
    }
}

// ---- items, row text and layout (pure) -------------------------------------------------------------------------------------------------------

/// C# `BuildItems()` (static), pure: the 18 controls in Tab order. `child = DIALOG_STYLE_CHILD`, `tab = WS_TABSTOP`, `text = child | SS_NOPREFIX`, `edit = child |
/// tab | ES_AUTOHSCROLL`, in this order: Static `LIST_HEADER_ID` (`tr::action_profiles_list_header()`, `text`); ListBox `LIST_ID` (`""`, `child | tab |
/// WS_VSCROLL | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS`, ex `WS_EX_CLIENTEDGE`); Button `ADD_ID` (`action_profiles_add`),
/// `REMOVE_ID` (`action_profiles_remove`), `IMPORT_ID` (`action_profiles_import`), `EXPORT_ID` (`action_profiles_export`), each `child | tab | BS_PUSHBUTTON`;
/// Static `DETAILS_HEADER_ID` (`action_profiles_details_header`, `text`); Static `NAME_LABEL_ID` (`action_profiles_name_label`, `text`); Edit `NAME_ID` (`""`,
/// `edit`, ex `WS_EX_CLIENTEDGE`); Static `SHORTCUT_LABEL_ID` (`action_profiles_shortcut_label`) and Edit `SHORTCUT_ID` (same pattern); Static
/// `OPERATION_LABEL_ID` (`action_profiles_operation_label`); ComboBox `OPERATION_ID` (`""`, `child | ComboBoxControl::DEFAULT_STYLE`); Static
/// `DESTINATION_LABEL_ID` (`action_profiles_destination_label`) and Edit `DESTINATION_ID` (`edit`, ex `WS_EX_CLIENTEDGE`); Button `CONFIRM_ID`
/// (`action_profiles_confirm_label`, `child | tab | BS_AUTOCHECKBOX | BS_MULTILINE`); Button `CANCEL_ID` (`common_cancel`, `child | tab | BS_PUSHBUTTON`); Button
/// `APPLY_ID` (`common_apply`, `child | tab | BS_DEFPUSHBUTTON`). Texts are the ambient language at the call.
pub fn build_items() -> Vec<DialogItem> {
    use super::native::template::{DialogControlClass as C, DialogItem as I};
    use super::zoom_prompt::DIALOG_STYLE_CHILD as child;
    use crate::win32::{control_constants as cc, dialog_native as n};
    let tab = n::WS_TABSTOP;
    let text = child | n::SS_NOPREFIX;
    let edit = child | tab | n::ES_AUTOHSCROLL;
    vec![
        I::new(C::Static, LIST_HEADER_ID, tr::action_profiles_list_header(), text),
        I::new(
            C::ListBox,
            LIST_ID,
            "",
            child
                | tab
                | n::WS_VSCROLL
                | n::LBS_NOTIFY
                | n::LBS_NOINTEGRALHEIGHT
                | cc::LBS_OWNERDRAWFIXED
                | cc::LBS_HASSTRINGS,
        )
        .with_ex_style(n::WS_EX_CLIENTEDGE),
        I::new(
            C::Button,
            ADD_ID,
            tr::action_profiles_add(),
            child | tab | n::BS_PUSHBUTTON,
        ),
        I::new(
            C::Button,
            REMOVE_ID,
            tr::action_profiles_remove(),
            child | tab | n::BS_PUSHBUTTON,
        ),
        I::new(
            C::Button,
            IMPORT_ID,
            tr::action_profiles_import(),
            child | tab | n::BS_PUSHBUTTON,
        ),
        I::new(
            C::Button,
            EXPORT_ID,
            tr::action_profiles_export(),
            child | tab | n::BS_PUSHBUTTON,
        ),
        I::new(C::Static, DETAILS_HEADER_ID, tr::action_profiles_details_header(), text),
        I::new(C::Static, NAME_LABEL_ID, tr::action_profiles_name_label(), text),
        I::new(C::Edit, NAME_ID, "", edit).with_ex_style(n::WS_EX_CLIENTEDGE),
        I::new(C::Static, SHORTCUT_LABEL_ID, tr::action_profiles_shortcut_label(), text),
        I::new(C::Edit, SHORTCUT_ID, "", edit).with_ex_style(n::WS_EX_CLIENTEDGE),
        I::new(
            C::Static,
            OPERATION_LABEL_ID,
            tr::action_profiles_operation_label(),
            text,
        ),
        I::new(C::ComboBox, OPERATION_ID, "", child | ComboBoxControl::DEFAULT_STYLE),
        I::new(
            C::Static,
            DESTINATION_LABEL_ID,
            tr::action_profiles_destination_label(),
            text,
        ),
        I::new(C::Edit, DESTINATION_ID, "", edit).with_ex_style(n::WS_EX_CLIENTEDGE),
        I::new(
            C::Button,
            CONFIRM_ID,
            tr::action_profiles_confirm_label(),
            child | tab | cc::BS_AUTOCHECKBOX | 0x2000,
        ),
        I::new(
            C::Button,
            CANCEL_ID,
            tr::common_cancel(),
            child | tab | n::BS_PUSHBUTTON,
        ),
        I::new(
            C::Button,
            APPLY_ID,
            tr::common_apply(),
            child | tab | n::BS_DEFPUSHBUTTON,
        ),
    ]
}

/// C# `RowText(index)` / the in-range branch of `ListItemText(ListId, index)`, pure: `Some(ListItemText { text: action.name, badge: action.shortcut when it is not
/// empty })` for `0 <= index < actions.len()`, else `None` (the caller then answers like the base class).
pub fn row_text_of(actions: &[ReviewAction], index: i32) -> Option<ListItemText> {
    let i = usize::try_from(index).ok()?;
    let action = actions.get(i)?;
    Some(ListItemText {
        text: action.name.clone(),
        badge: (!action.shortcut.is_empty()).then(|| action.shortcut.clone()),
    })
}

/// What `layout_plan` decided, in the C# call order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionProfilesLayoutPlan {
    /// Cancel / Apply at the bottom (C# calls it FIRST).
    pub button_row: ButtonRowPlan,
    /// Every `Place`, in order: list header, list, Add, Remove, Import, Export, details header, then the fields.
    pub placements: Vec<Placement>,
    /// `SetListItemHeight(ListId, ..)`, called right after the list is placed: `line_height + dy(5)`.
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

/// C# `LeftWidth(clientWidth)`: `clamp(client_width / 3, dx(100), dx(170))` written as `(client_width / 3).max(dx(100)).min(dx(170))` (`i32::clamp` panics when the
/// bounds cross; they cannot, but R0003).
pub fn left_width(m: &dyn DialogMeasure, client_width: i32) -> i32 {
    (client_width / 3).max(m.dx(100)).min(m.dx(170))
}

/// C# `PreferredClientSize()` = `(dx(440), dy(230))`.
pub fn preferred_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    (m.dx(440), m.dy(230))
}

/// C# `MinimumClientSize()` = `(max(dx(340), button_row_width([CANCEL_ID, APPLY_ID]) + 2 * margin), dy(210))`.
pub fn minimum_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    let margin = margin(m);
    (
        m.dx(340).max(
            m.button_row_width(&[CANCEL_ID, APPLY_ID])
                .wrapping_add(margin.wrapping_mul(2)),
        ),
        m.dy(210),
    )
}

/// C# `LayoutControls(clientWidth, clientHeight)` (lines 360 to 404): `m = margin`; `left = left_width`; `right_x = m + left + m`; `right_width = max(dx(60),
/// client_width - right_x - m)`; `bh = field_height`; `bottom_top = client_height - m - bh`; the button row `[CANCEL_ID, APPLY_ID]` at `bottom_top` (right margin
/// `m`, height `bh`); `content_bottom = bottom_top - dy(8)`. LEFT column: `y = m`; `header = measure_item_height(LIST_HEADER_ID, left)`, place it, `y += header +
/// dy(4)`; `gap = dx(4)`; `half = (left - gap) / 2`; `row2 = content_bottom - bh`; `row1 = row2 - dy(4) - bh`; list `Place(LIST_ID, m, y, left, max(dy(30), row1 - dy(4) -
/// y))`, `list_item_height = line_height + dy(5)`; Add `(m, row1, half, bh)`; Remove `(m + half + gap, row1, left - half - gap, bh)`; Import `(m, row2, half, bh)`;
/// Export `(m + half + gap, row2, left - half - gap, bh)`. RIGHT column: `y = m`; details header `measure_item_height(DETAILS_HEADER_ID, right_width)`, place,
/// `y += details + dy(8)`; `place_field(NAME_LABEL_ID, NAME_ID)`, then `SHORTCUT`; the operation: `combo_height = bh + 4 * (line_height + dy(2))`, `label =
/// measure_item_height(OPERATION_LABEL_ID, right_width)`, `Place(OPERATION_LABEL_ID, right_x, y, right_width, label)`, `Place(OPERATION_ID, right_x, y + label +
/// dy(2), right_width, combo_height)`, `y += label + dy(2) + bh + dy(8)` (the combo's drop-down list is taller than its closed box: only `bh` is reserved);
/// `place_field(DESTINATION_LABEL_ID, DESTINATION_ID)`; the check box: `box = dx(14)`, `confirm = max(bh, measure_text_height(item_text(CONFIRM_ID), max(1,
/// right_width - box - dx(4))))`, `Place(CONFIRM_ID, right_x, y + dy(2), right_width, confirm)`. `place_field(label, edit)`: `h = measure_item_height(label,
/// right_width)`, `Place(label, right_x, y, right_width, h)`, `y += h + dy(2)`, `Place(edit, right_x, y, right_width, bh)`, `y += bh + dy(8)`.
pub fn layout_plan(m: &dyn DialogMeasure, client_width: i32, client_height: i32) -> ActionProfilesLayoutPlan {
    let mrg = margin(m);
    let left = left_width(m, client_width);
    let right_x = mrg.wrapping_add(left).wrapping_add(mrg);
    let right_width = m.dx(60).max(client_width.wrapping_sub(right_x).wrapping_sub(mrg));
    let bh = field_height(m);
    let bottom_top = client_height.wrapping_sub(mrg).wrapping_sub(bh);
    let button_row = ButtonRowPlan {
        client_width,
        right_margin: mrg,
        top: bottom_top,
        height: bh,
        ids: vec![CANCEL_ID, APPLY_ID],
    };
    let content_bottom = bottom_top.wrapping_sub(m.dy(8));
    let mut placements = Vec::with_capacity(17);
    let mut y = mrg;
    let header = m.measure_item_height(LIST_HEADER_ID, left);
    placements.push(Placement {
        id: LIST_HEADER_ID,
        x: mrg,
        y,
        width: left,
        height: header,
    });
    y = y.wrapping_add(header).wrapping_add(m.dy(4));
    let gap = m.dx(4);
    let half = left.wrapping_sub(gap) / 2;
    let row2 = content_bottom.wrapping_sub(bh);
    let row1 = row2.wrapping_sub(m.dy(4)).wrapping_sub(bh);
    placements.push(Placement {
        id: LIST_ID,
        x: mrg,
        y,
        width: left,
        height: m.dy(30).max(row1.wrapping_sub(m.dy(4)).wrapping_sub(y)),
    });
    let list_item_height = m.line_height().wrapping_add(m.dy(5));
    placements.push(Placement {
        id: ADD_ID,
        x: mrg,
        y: row1,
        width: half,
        height: bh,
    });
    placements.push(Placement {
        id: REMOVE_ID,
        x: mrg.wrapping_add(half).wrapping_add(gap),
        y: row1,
        width: left.wrapping_sub(half).wrapping_sub(gap),
        height: bh,
    });
    placements.push(Placement {
        id: IMPORT_ID,
        x: mrg,
        y: row2,
        width: half,
        height: bh,
    });
    placements.push(Placement {
        id: EXPORT_ID,
        x: mrg.wrapping_add(half).wrapping_add(gap),
        y: row2,
        width: left.wrapping_sub(half).wrapping_sub(gap),
        height: bh,
    });
    y = mrg;
    let details = m.measure_item_height(DETAILS_HEADER_ID, right_width);
    placements.push(Placement {
        id: DETAILS_HEADER_ID,
        x: right_x,
        y,
        width: right_width,
        height: details,
    });
    y = y.wrapping_add(details).wrapping_add(m.dy(8));
    let place_field = |id_label: i32, id_edit: i32, y: &mut i32, ps: &mut Vec<Placement>| {
        let h = m.measure_item_height(id_label, right_width);
        ps.push(Placement {
            id: id_label,
            x: right_x,
            y: *y,
            width: right_width,
            height: h,
        });
        *y = y.wrapping_add(h).wrapping_add(m.dy(2));
        ps.push(Placement {
            id: id_edit,
            x: right_x,
            y: *y,
            width: right_width,
            height: bh,
        });
        *y = y.wrapping_add(bh).wrapping_add(m.dy(8));
    };
    place_field(NAME_LABEL_ID, NAME_ID, &mut y, &mut placements);
    place_field(SHORTCUT_LABEL_ID, SHORTCUT_ID, &mut y, &mut placements);
    let combo_height = bh.wrapping_add(4i32.wrapping_mul(m.line_height().wrapping_add(m.dy(2))));
    let operation_label = m.measure_item_height(OPERATION_LABEL_ID, right_width);
    placements.push(Placement {
        id: OPERATION_LABEL_ID,
        x: right_x,
        y,
        width: right_width,
        height: operation_label,
    });
    placements.push(Placement {
        id: OPERATION_ID,
        x: right_x,
        y: y.wrapping_add(operation_label).wrapping_add(m.dy(2)),
        width: right_width,
        height: combo_height,
    });
    y = y
        .wrapping_add(operation_label)
        .wrapping_add(m.dy(2))
        .wrapping_add(bh)
        .wrapping_add(m.dy(8));
    place_field(DESTINATION_LABEL_ID, DESTINATION_ID, &mut y, &mut placements);
    let box_width = m.dx(14);
    let confirm_height = bh.max(m.measure_text_height(
        &m.item_text(CONFIRM_ID),
        (right_width.wrapping_sub(box_width).wrapping_sub(m.dx(4))).max(1),
    ));
    placements.push(Placement {
        id: CONFIRM_ID,
        x: right_x,
        y: y.wrapping_add(m.dy(2)),
        width: right_width,
        height: confirm_height,
    });
    ActionProfilesLayoutPlan {
        button_row,
        placements,
        list_item_height,
    }
}

/// Runs the plan: the button row first, then every placement in order, with `dialog.set_list_item_height(LIST_ID, plan.list_item_height)` right after the
/// placement of `LIST_ID`.
pub fn apply_layout(dialog: &NativeDialog, plan: &ActionProfilesLayoutPlan) {
    apply_button_row(dialog, &plan.button_row);
    for placement in &plan.placements {
        apply_placements(dialog, std::slice::from_ref(placement));
        if placement.id == LIST_ID {
            dialog.set_list_item_height(LIST_ID, plan.list_item_height);
        }
    }
}

// ---- the dialog -----------------------------------------------------------------------------------------------------------------------------

/// The `DialogContent` of the action profiles dialog. The presenter is in a `RefCell` borrowed ONLY for one presenter call; every control read happens BEFORE
/// `borrow_mut` and every control write AFTER it (D-W8B4I-1, D-W8B5-504), and a modal warning or file dialog never runs under a borrow.
pub struct ActionProfilesContent {
    presenter: RefCell<ActionProfilesPresenter>,
    files: Option<Rc<dyn ShellFileDialog>>,
    warnings: Rc<dyn DialogWarningSink>,
    items: Vec<DialogItem>,
}

impl fmt::Debug for ActionProfilesContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionProfilesContent").finish_non_exhaustive()
    }
}

impl ActionProfilesContent {
    /// C# constructor body: `presenter = ActionProfilesPresenter::new(actions)` (copies the list); `warnings` defaults to `TaskDialogWarningSink::instance()`;
    /// `items = build_items()` now.
    pub fn new(
        actions: &[ReviewAction],
        files: Option<Rc<dyn ShellFileDialog>>,
        warnings: Option<Rc<dyn DialogWarningSink>>,
    ) -> Self {
        Self {
            presenter: RefCell::new(ActionProfilesPresenter::new(actions)),
            files,
            warnings: warnings.unwrap_or_else(TaskDialogWarningSink::instance),
            items: build_items(),
        }
    }

    /// C# `Actions`: the edited list (a copy). Valid after Apply (`ID_OK`).
    pub fn actions(&self) -> Vec<ReviewAction> {
        self.presenter
            .try_borrow()
            .map(|p| p.actions().to_vec())
            .unwrap_or_default()
    }

    /// C# `FillList()`: remember `LB_GETTOPINDEX`, `ListBoxControl::clear`, add every action name, and when `0 < top < count` restore it (`LB_SETTOPINDEX`).
    fn fill_list(&self, dialog: &NativeDialog) {
        let list = dialog.item(LIST_ID);
        // SAFETY: list is a live list-box handle during the dialog callback; both messages carry integer indices only.
        // SAFETY: LB_GETTOPINDEX uses integer-only message parameters and list is the live list-box child.
        let top = unsafe { crate::win32::dialog_native::user32::send_message(list, 0x018E, 0, 0) } as i32;
        let names = self
            .presenter
            .try_borrow()
            .map(|presenter| {
                presenter
                    .actions()
                    .iter()
                    .map(|action| action.name.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        ListBoxControl::clear(list);
        for name in &names {
            ListBoxControl::add(list, name);
        }
        let count = i32::try_from(names.len()).unwrap_or(i32::MAX);
        if top > 0 && top < count {
            // SAFETY: LB_SETTOPINDEX uses an integer index and list is the live list-box child.
            unsafe {
                crate::win32::dialog_native::user32::send_message(list, 0x0197, top as usize, 0);
            }
        }
    }

    /// C# `RefreshList()`: remember the selected index, `fill_list`, and re-select it when it is still in range.
    fn refresh_list(&self, dialog: &NativeDialog) {
        let list = dialog.item(LIST_ID);
        let selected = ListBoxControl::selected_index(list);
        self.fill_list(dialog);
        if let Ok(presenter) = self.presenter.try_borrow() {
            if selected >= 0 && usize::try_from(selected).is_ok_and(|i| i < presenter.actions().len()) {
                ListBoxControl::select_single(list, selected);
            }
        }
    }

    /// C# `CurrentFields()`: name, shortcut, `max(0, combo selected index)`, destination, check state, read from the controls.
    fn current_fields(&self, dialog: &NativeDialog) -> pr_app::presenters::action_profiles::ActionProfileForm {
        use pr_app::presenters::action_profiles::ActionProfileForm;
        ActionProfileForm {
            name: dialog.get_item_text(NAME_ID),
            shortcut: dialog.get_item_text(SHORTCUT_ID),
            operation_index: ComboBoxControl::selected_index(dialog.item(OPERATION_ID)).max(0),
            destination: dialog.get_item_text(DESTINATION_ID),
            confirm: ButtonStateControl::is_checked(dialog.item(CONFIRM_ID)),
        }
    }

    /// C# `SaveCurrent()`: `presenter.save_current(&current_fields)` and, when it returned `true`, `refresh_list`.
    fn save_current(&self, dialog: &NativeDialog) {
        let fields = self.current_fields(dialog);
        let changed = self
            .presenter
            .try_borrow_mut()
            .ok()
            .is_some_and(|mut p| p.save_current(&fields));
        if changed {
            self.refresh_list(dialog);
        }
    }

    /// C# `SelectIndex(index)`: `ListBoxControl::select_single(list, index)` then `on_selection_changed`.
    fn select_index(&self, dialog: &NativeDialog, index: i32) {
        let list = dialog.item(LIST_ID);
        ListBoxControl::select_single(list, index);
        self.on_selection_changed(dialog);
    }

    /// C# `OnSelectionChanged()`: `save_current`; the selected index maps to a `RowId` (`presenter.row_id`), `presenter.open(id)`; `None` returns without
    /// touching the fields; `Some(form)` fills name, shortcut, destination, the check box and the combo selection.
    fn on_selection_changed(&self, dialog: &NativeDialog) {
        self.save_current(dialog);
        let index = ListBoxControl::selected_index(dialog.item(LIST_ID));
        let selected = self
            .presenter
            .try_borrow()
            .ok()
            .and_then(|p| usize::try_from(index).ok().and_then(|i| p.row_id(i)));
        let fields = self.presenter.try_borrow_mut().ok().and_then(|mut p| p.open(selected));
        if let Some(fields) = fields {
            dialog.set_item_text(NAME_ID, &fields.name);
            dialog.set_item_text(SHORTCUT_ID, &fields.shortcut);
            dialog.set_item_text(DESTINATION_ID, &fields.destination);
            ButtonStateControl::set_checked(dialog.item(CONFIRM_ID), fields.confirm);
            ComboBoxControl::select(dialog.item(OPERATION_ID), fields.operation_index);
        }
    }

    /// C# `AddAction()`: `save_current`; `presenter.add(&Localizer::current())`; `fill_list`; `select_index` of the new row (found with `row_id`).
    fn add_action(&self, dialog: &NativeDialog) {
        self.save_current(dialog);
        let id = self
            .presenter
            .try_borrow_mut()
            .ok()
            .map(|mut p| p.add(&Localizer::current()));
        if let Some(id) = id {
            self.fill_list(dialog);
            let index = self
                .presenter
                .try_borrow()
                .ok()
                .and_then(|p| (0..p.actions().len()).find(|&i| p.row_id(i) == Some(id)))
                .and_then(|i| i32::try_from(i).ok());
            if let Some(index) = index {
                self.select_index(dialog, index);
            }
        }
    }

    /// C# `RemoveAction()`: nothing when no row is selected; `next = presenter.remove(row, index)`; `fill_list`; `Some(next)` -> `select_index(next)`; `None` ->
    /// clear name, shortcut and destination and uncheck "confirm".
    fn remove_action(&self, dialog: &NativeDialog) {
        let index = ListBoxControl::selected_index(dialog.item(LIST_ID));
        let result = self.presenter.try_borrow().ok().and_then(|p| {
            usize::try_from(index)
                .ok()
                .filter(|&i| i < p.actions().len())
                .and_then(|i| p.row_id(i).map(|row| (row, i)))
        });
        let Some((row, selected_index)) = result else {
            return;
        };
        let next = self
            .presenter
            .try_borrow_mut()
            .ok()
            .and_then(|mut p| p.remove(row, selected_index));
        self.fill_list(dialog);
        if let Some(next) = next {
            if let Ok(next) = i32::try_from(next) {
                self.select_index(dialog, next);
            }
            return;
        }
        dialog.set_item_text(NAME_ID, "");
        dialog.set_item_text(SHORTCUT_ID, "");
        dialog.set_item_text(DESTINATION_ID, "");
        ButtonStateControl::set_checked(dialog.item(CONFIRM_ID), false);
    }

    /// C# `ImportActions()`: no file dialog -> nothing. `files.pick_open_file(dialog.hwnd(), tr::action_profiles_import(),
    /// FileFilterText::parse(tr::dialog_file_filter_json_or_all()), None)`: `Ok(None)` or `Err(hr)` (logged with `AppLog`, then treated as a cancel,
    /// D-W8B5-2) -> nothing; `Ok(Some(path))` -> `try_import(path)`: `Err(text)` -> `warnings.show_warning(hwnd, tr::dialog_import_actions_failed_title(), text)`;
    /// `Ok(list)` -> `presenter.replace_all(&list)`, `fill_list`, `select_index(0)`.
    fn import_actions(&self, dialog: &NativeDialog) {
        let Some(files) = &self.files else {
            return;
        };
        let path = match files.pick_open_file(
            dialog.hwnd(),
            &tr::action_profiles_import(),
            &FileFilterText::parse(&tr::dialog_file_filter_json_or_all()),
            None,
        ) {
            Ok(Some(path)) => path,
            Ok(None) => return,
            Err(error) => {
                pr_app::misc::app_log::AppLog::global().error("File dialog failed", Some(&error));
                return;
            }
        };
        let imported =
            pr_app::presenters::action_profiles::try_import(std::path::Path::new(&path), &Localizer::current());
        match imported {
            Err(message) => {
                self.warnings
                    .show_warning(dialog.hwnd(), &tr::dialog_import_actions_failed_title(), &message)
            }
            Ok(actions) => {
                if let Ok(mut presenter) = self.presenter.try_borrow_mut() {
                    presenter.replace_all(&actions);
                } else {
                    return;
                }
                self.fill_list(dialog);
                self.select_index(dialog, 0);
            }
        }
    }

    /// C# `ExportActions()`: `save_current` FIRST (even without a file dialog); no file dialog -> nothing. `files.pick_save_file(hwnd, tr::action_profiles_export(),
    /// FileFilterText::parse(tr::dialog_file_filter_json()), Some("json"), Some("photoreview-actions.json"), None)`; cancel or `Err` -> nothing; `try_export(path,
    /// actions)` returning `Some(text)` -> `show_warning(hwnd, tr::dialog_export_actions_failed_title(), text)`.
    fn export_actions(&self, dialog: &NativeDialog) {
        self.save_current(dialog);
        let Some(files) = &self.files else {
            return;
        };
        let path = match files.pick_save_file(
            dialog.hwnd(),
            &tr::action_profiles_export(),
            &FileFilterText::parse(&tr::dialog_file_filter_json()),
            Some("json"),
            Some("photoreview-actions.json"),
            None,
        ) {
            Ok(Some(path)) => path,
            Ok(None) => return,
            Err(error) => {
                pr_app::misc::app_log::AppLog::global().error("File dialog failed", Some(&error));
                return;
            }
        };
        let actions = self.actions();
        if let Some(message) = pr_app::presenters::action_profiles::try_export(
            std::path::Path::new(&path),
            &actions,
            &Localizer::current(),
        ) {
            self.warnings
                .show_warning(dialog.hwnd(), &tr::dialog_export_actions_failed_title(), &message);
        }
    }
}

impl DialogContent for ActionProfilesContent {
    /// `tr::action_profiles_title()`.
    fn title(&self) -> String {
        tr::action_profiles_title()
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
        apply_layout(dialog, &layout_plan(dialog, client_width, client_height))
    }

    /// C# `OnInitialized()`: the combo gets `tr::enum_file_operation_move()`, `..._copy()`, `..._recycle()` (index 0 Move, 1 Copy, 2 Recycle = `operation_to_index`)
    /// and selection 0; `fill_list`; with at least one action `select_index(0)`.
    fn on_initialized(&self, dialog: &NativeDialog) {
        let combo = dialog.item(OPERATION_ID);
        ComboBoxControl::add(combo, &tr::enum_file_operation_move());
        ComboBoxControl::add(combo, &tr::enum_file_operation_copy());
        ComboBoxControl::add(combo, &tr::enum_file_operation_recycle());
        ComboBoxControl::select(combo, 0);
        self.fill_list(dialog);
        if self.presenter.try_borrow().is_ok_and(|p| !p.actions().is_empty()) {
            self.select_index(dialog, 0);
        }
    }

    /// C# `OnCommand(id, notification)`: `LIST_ID` with `LBN_SELCHANGE` -> `on_selection_changed`; `ADD_ID`, `REMOVE_ID`, `IMPORT_ID`, `EXPORT_ID` with `BN_CLICKED`
    /// -> the matching method; each returns `true`. Anything else `false`.
    fn on_command(&self, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        const LBN_SELCHANGE: i32 = 1;
        const BN_CLICKED: i32 = 0;
        match (id, notification) {
            (LIST_ID, LBN_SELCHANGE) => {
                self.on_selection_changed(dialog);
                true
            }
            (ADD_ID, BN_CLICKED) => {
                self.add_action(dialog);
                true
            }
            (REMOVE_ID, BN_CLICKED) => {
                self.remove_action(dialog);
                true
            }
            (IMPORT_ID, BN_CLICKED) => {
                self.import_actions(dialog);
                true
            }
            (EXPORT_ID, BN_CLICKED) => {
                self.export_actions(dialog);
                true
            }
            _ => false,
        }
    }

    /// C# `OnOk()`: `save_current`; `result = presenter.apply(&Localizer::current())`; not accepted -> `warnings.show_warning(hwnd,
    /// tr::dialog_action_profiles_invalid_title(), result.message or "")` and `false` (stay open); accepted -> `true`.
    fn on_ok(&self, dialog: &NativeDialog) -> bool {
        self.save_current(dialog);
        let result = self.presenter.try_borrow().ok().map(|p| p.apply(&Localizer::current()));
        let Some(result) = result else {
            return false;
        };
        if !result.accepted {
            self.warnings.show_warning(
                dialog.hwnd(),
                &tr::dialog_action_profiles_invalid_title(),
                result.message.as_deref().unwrap_or(""),
            );
            return false;
        }
        true
    }

    /// C# `ListItemText(id, index)`: `LIST_ID` with `row_text_of(actions, index)` `Some` -> that; anything else the base answer
    /// `ListItemText { text: ListBoxControl::text_at(dialog.item(id), index), badge: None }`.
    fn list_item_text(&self, dialog: &NativeDialog, id: i32, index: i32) -> ListItemText {
        if id == LIST_ID {
            if let Some(text) = row_text_of(&self.actions(), index) {
                return text;
            }
        }
        ListItemText {
            text: ListBoxControl::text_at(dialog.item(id), index),
            badge: None,
        }
    }
}

/// The action profiles dialog (C# `ActionProfilesDialog`). Modal; show it with `dialog.native().show_modal(owner, ready, loop)`: `Ok(ID_OK)` means `actions()` is
/// the edited list, anything else means "cancelled, use the original list".
pub struct ActionProfilesDialog {
    native: NativeDialog,
    content: Rc<ActionProfilesContent>,
}

impl fmt::Debug for ActionProfilesDialog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionProfilesDialog")
            .field("native", &self.native)
            .finish_non_exhaustive()
    }
}

impl ActionProfilesDialog {
    /// C# `ActionProfilesDialog(actions, files = null, warnings = null, chrome = null)`.
    pub fn new(
        actions: &[ReviewAction],
        files: Option<Rc<dyn ShellFileDialog>>,
        warnings: Option<Rc<dyn DialogWarningSink>>,
        chrome: Option<Rc<dyn NativeDialogChrome>>,
    ) -> Self {
        let content = Rc::new(ActionProfilesContent::new(actions, files, warnings));
        let native = NativeDialog::with_chrome(content.clone(), chrome);
        Self { native, content }
    }

    /// The base-class handle.
    pub fn native(&self) -> &NativeDialog {
        &self.native
    }

    /// C# `Actions`.
    pub fn actions(&self) -> Vec<ReviewAction> {
        self.content.actions()
    }

    /// C# `RowText(index)` (test seam): text and badge of row `index` as the owner-draw list asks for them.
    pub fn row_text(&self, index: i32) -> ListItemText {
        row_text_of(&self.content.actions(), index).unwrap_or_else(|| ListItemText {
            text: String::new(),
            badge: None,
        })
    }
}
