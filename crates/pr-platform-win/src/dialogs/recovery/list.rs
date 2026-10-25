//! Port of `RecoveryDialog.cs` lines 235-381: the filter, the owner-draw multi-select list and its selection (`SelectedRows`, `SingleSelected`, `VisibleRows`,
//! `SelectRows`, `RowView`, `ListItemText` / `Subtitle` / `BadgeColor`, `ToColorRef`, `RefreshEntries`, `OnSelectionChanged`, `OnRowResultChanged`). Unit W8-18t.
//! Contract: `contracts/W8-18t.md`. Rows are INDICES into the presenter rows (D-W8B5-604): the visible list is rebuilt in the same UI turn as every mutation of
//! the rows, selection is carried over by entry id, every access is bounds-checked.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;

use pr_app::presenters::recovery_filter::{RecoveryFilter, RecoveryTone};

use super::RecoveryContent;
use crate::dialogs::native::dialog::{ListItemText, NativeDialog};

/// State of the list (C# `_visible`, `_initialized`).
#[derive(Debug)]
pub struct ListState {
    /// C# `_visible`: indices into the presenter rows of the rows the filter lets through, in display order.
    pub(super) visible: RefCell<Vec<usize>>,
    /// The entry ids of `visible`, snapshotted at the same moment (same length, same order). The presenter indices go stale the instant a row is removed
    /// (`dismiss`, a superseded retry) while the list still shows the old rows until `refresh_entries` runs; the selection carry-over of `refresh_entries` reads
    /// the selected ids from THIS snapshot, never from `presenter.rows()[visible[i]]` (that would select the neighbours of the removed rows). D-W8B5-604.
    pub(super) visible_ids: RefCell<Vec<String>>,
    /// C# `_initialized`: set at the end of `on_initialized`; `Relayout` does nothing before.
    pub(super) initialized: Cell<bool>,
}

impl ListState {
    /// Empty list, not initialised.
    pub fn new() -> Self {
        Self {
            visible: RefCell::new(Vec::new()),
            visible_ids: RefCell::new(Vec::new()),
            initialized: Cell::new(false),
        }
    }
}

impl Default for ListState {
    fn default() -> Self {
        Self::new()
    }
}

/// C# tuple `(string Text, string? Badge, string? Subtitle, uint? Color)` of `RowView(index)` (tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    /// The title (first line).
    pub text: String,
    /// The badge text (`RecoveryRowModel::badge_text`).
    pub badge: Option<String>,
    /// The second, smaller line.
    pub subtitle: Option<String>,
    /// The `COLORREF` of the badge (`to_color_ref(badge_tone)`).
    pub color: Option<u32>,
}

/// C# `ToColorRef(tone)`: `let (r, g, b) = recovery_presentation::rgb(tone); r | (g << 8) | (b << 16)` as a `COLORREF`. `rgb` returns `u8` channels: widen each to
/// `u32` BEFORE shifting (`u8 << 8` overflows).
pub fn to_color_ref(tone: RecoveryTone) -> u32 {
    let (r, g, b) = pr_app::presenters::recovery_presentation::rgb(tone);
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
}

/// C# `CurrentFilter`: `(RecoveryFilter)Math.Max(0, ComboBoxControl.SelectedIndex(Item(FilterId)))`; an index above 3 (impossible, 4 entries) maps to `All`.
pub fn filter_from_index(selected_index: i32) -> RecoveryFilter {
    match selected_index {
        i if i <= 0 => RecoveryFilter::All,
        1 => RecoveryFilter::CanRetry,
        2 => RecoveryFilter::AlreadyDone,
        3 => RecoveryFilter::Problems,
        _ => RecoveryFilter::All,
    }
}

/// Pure core of the selection carry-over of `RefreshEntries` (lines 322, 333-339): the positions in `new_visible_ids` whose id is in `selected_ids`.
pub fn carried_selection(selected_ids: &HashSet<String>, new_visible_ids: &[String]) -> Vec<usize> {
    new_visible_ids
        .iter()
        .enumerate()
        .filter_map(|(position, id)| selected_ids.contains(id).then_some(position))
        .collect()
}

/// The `LB_SETTOPINDEX` argument of `RefreshEntries` (lines 323, 341-344): `Some(top)` when `top > 0 && top < shown_count`, else `None`.
pub fn top_index_to_restore(top: i32, shown_count: usize) -> Option<i32> {
    let top = usize::try_from(top).ok()?;
    (top > 0 && top < shown_count)
        .then(|| i32::try_from(top).ok())
        .flatten()
}

/// C# `SelectedRows()`: the presenter indices of the selected list items, in display order; empty when the list window is `0`; an item index outside
/// `visible` is skipped. The `visible` borrow is released before `ListBoxControl::selected_indices` is called and re-taken to map (no `RefCell` borrow across a
/// `SendMessage`: `WM_DRAWITEM` re-enters the `list_item_*` hooks, which borrow `visible`).
pub fn selected_rows(content: &RecoveryContent, dialog: &NativeDialog) -> Vec<usize> {
    use crate::dialogs::native::controls::basic::ListBoxControl;
    let hwnd = dialog.item(super::layout::LIST_ID);
    if hwnd == 0 {
        return Vec::new();
    }
    let visible = content.list.visible.borrow().clone();
    ListBoxControl::selected_indices(hwnd)
        .into_iter()
        .filter_map(|i| usize::try_from(i).ok())
        .filter_map(|i| visible.get(i).copied())
        .collect()
}

/// C# `SingleSelected()`: `Some(index)` when exactly one row is selected, else `None`.
pub fn single_selected(content: &RecoveryContent, dialog: &NativeDialog) -> Option<usize> {
    let rows = selected_rows(content, dialog);
    (rows.len() == 1).then(|| rows.first().copied()).flatten()
}

/// C# `VisibleRows`.
pub fn visible_rows(content: &RecoveryContent) -> Vec<usize> {
    content.list.visible.borrow().clone()
}

/// C# `SelectRows(params int[])` (lines 273-283): clear the selection, select the given visible indices, then `on_selection_changed`.
pub fn select_rows(content: &RecoveryContent, dialog: &NativeDialog, visible_indices: &[i32]) {
    use crate::dialogs::native::controls::basic::ListBoxControl;
    let hwnd = dialog.item(super::layout::LIST_ID);
    if hwnd == 0 {
        return;
    }
    ListBoxControl::select_all(hwnd, false);
    for &index in visible_indices {
        if index >= 0
            && usize::try_from(index)
                .ok()
                .is_some_and(|i| i < content.list.visible.borrow().len())
        {
            ListBoxControl::set_selected(hwnd, index, true);
        }
    }
    on_selection_changed(content, dialog);
}

/// C# `RowView(index)`.
pub fn row_view(content: &RecoveryContent, _dialog: &NativeDialog, index: i32) -> RowView {
    let Some(row_index) = usize::try_from(index)
        .ok()
        .and_then(|i| content.list.visible.borrow().get(i).copied())
    else {
        return RowView {
            text: String::new(),
            badge: None,
            subtitle: None,
            color: None,
        };
    };
    let Ok(presenter) = content.presenter.try_borrow() else {
        return RowView {
            text: String::new(),
            badge: None,
            subtitle: None,
            color: None,
        };
    };
    let Some(row) = presenter.rows().get(row_index) else {
        return RowView {
            text: String::new(),
            badge: None,
            subtitle: None,
            color: None,
        };
    };
    let texts = content.texts.texts();
    RowView {
        text: row.title(&texts),
        badge: Some(row.badge_text(&texts)),
        subtitle: Some(row.subtitle(&texts)),
        color: Some(to_color_ref(row.badge_tone())),
    }
}

/// C# `ListItemText(id, index)` (lines 290-299): for `LIST_ID` and an index inside `visible`: `(row.title(texts), Some(row.badge_text(texts)))`; otherwise the default
/// (`ListBoxControl::text_at(dialog.item(id), index)`, no badge).
pub fn list_item_text(content: &RecoveryContent, dialog: &NativeDialog, id: i32, index: i32) -> ListItemText {
    if id != super::layout::LIST_ID {
        return ListItemText {
            text: crate::dialogs::native::controls::basic::ListBoxControl::text_at(dialog.item(id), index),
            badge: None,
        };
    }
    let view = row_view(content, dialog, index);
    if view.badge.is_none() {
        return ListItemText {
            text: crate::dialogs::native::controls::basic::ListBoxControl::text_at(dialog.item(id), index),
            badge: None,
        };
    }
    ListItemText {
        text: view.text,
        badge: view.badge,
    }
}

/// C# `ListItemSubtitle(id, index)`: `Some(row.subtitle)` for `LIST_ID` and an index inside `visible`, else `None`.
pub fn list_item_subtitle(content: &RecoveryContent, dialog: &NativeDialog, id: i32, index: i32) -> Option<String> {
    (id == super::layout::LIST_ID)
        .then(|| row_view(content, dialog, index).subtitle)
        .flatten()
}

/// C# `ListItemBadgeColor(id, index)`: `Some(to_color_ref(row.badge_tone))` for `LIST_ID` and an index inside `visible`, else `None`.
pub fn list_item_badge_color(content: &RecoveryContent, dialog: &NativeDialog, id: i32, index: i32) -> Option<u32> {
    (id == super::layout::LIST_ID)
        .then(|| row_view(content, dialog, index).color)
        .flatten()
}

/// C# `RefreshEntries()` (lines 314-357), in order: nothing when the list window is `0`; remember the selected ENTRY IDS (from the `visible_ids` snapshot, by
/// list position) and `LB_GETTOPINDEX`; `shown = presenter.shown(current_filter)` (short borrow); `visible = shown` and `visible_ids` = their entry ids (each
/// `RefCell` borrow is replaced in one statement and released before any control call); clear the list, `LB_INITSTORAGE(count, count * 64)`, add every
/// `row.title(texts)`; re-select by id (`carried_selection`); restore the top index (`LB_GETTOPINDEX` / `LB_SETTOPINDEX` / `LB_INITSTORAGE` have no
/// `ListBoxControl` wrapper: `user32::send_message` with value arguments, each call with its own `// SAFETY:`); `SUMMARY_ID` text = `presenter.summary_text(localizer)`; `empty = presenter.filter_empty_visible(count)`;
/// show / hide `FILTER_EMPTY_ID`; when `empty` and the focus is the list, focus the filter combo box; show `LIST_ID` when not `empty`; then
/// `path_panel::update_details` and `actions::update_buttons`.
pub fn refresh_entries(content: &RecoveryContent, dialog: &NativeDialog) {
    use crate::dialogs::native::controls::basic::ListBoxControl;
    let hwnd = dialog.item(super::layout::LIST_ID);
    if hwnd == 0 {
        return;
    }
    let selected_positions = ListBoxControl::selected_indices(hwnd);
    let selected_ids: HashSet<String> = {
        let old_ids = content.list.visible_ids.borrow();
        selected_positions
            .into_iter()
            .filter_map(|position| usize::try_from(position).ok())
            .filter_map(|position| old_ids.get(position).cloned())
            .collect()
    };
    // SAFETY: LB_GETTOPINDEX accepts only integer parameters and this is the live test-independent list handle.
    let top = unsafe {
        crate::win32::dialog_native::user32::send_message(hwnd, crate::win32::control_constants::LB_GETTOPINDEX, 0, 0)
    } as i32;
    let filter = filter_from_index(
        crate::dialogs::native::controls::basic::ComboBoxControl::selected_index(dialog.item(super::layout::FILTER_ID)),
    );
    let (shown, ids, titles, summary, empty) = {
        let presenter = content.presenter.borrow();
        let shown = presenter.shown(filter);
        let rows: Vec<_> = shown.iter().filter_map(|&index| presenter.rows().get(index)).collect();
        let ids = rows.iter().map(|row| row.entry().id.clone()).collect::<Vec<_>>();
        let titles = rows
            .iter()
            .map(|row| row.title(&content.texts.texts()))
            .collect::<Vec<_>>();
        let empty = presenter.filter_empty_visible(shown.len());
        (
            shown,
            ids,
            titles,
            presenter.summary_text(content.texts.localizer()),
            empty,
        )
    };
    // Snapshot vectors are replaced before any list-box message can synchronously re-enter owner-draw hooks.
    *content.list.visible.borrow_mut() = shown;
    *content.list.visible_ids.borrow_mut() = ids.clone();
    ListBoxControl::clear(hwnd);
    // SAFETY: LB_INITSTORAGE consumes a count and byte estimate, with no pointer arguments.
    unsafe {
        crate::win32::dialog_native::user32::send_message(
            hwnd,
            crate::win32::dialog_native::LB_INITSTORAGE,
            ids.len(),
            ids.len().saturating_mul(64) as isize,
        );
    }
    for title in titles {
        ListBoxControl::add(hwnd, &title);
    }
    for position in carried_selection(&selected_ids, &ids) {
        if let Ok(i) = i32::try_from(position) {
            ListBoxControl::set_selected(hwnd, i, true);
        }
    }
    if let Some(top) = top_index_to_restore(top, ids.len()) {
        // SAFETY: LB_SETTOPINDEX consumes an in-range index and no pointer arguments.
        unsafe {
            crate::win32::dialog_native::user32::send_message(
                hwnd,
                crate::win32::control_constants::LB_SETTOPINDEX,
                top as usize,
                0,
            );
        }
    }
    dialog.set_item_text(super::layout::SUMMARY_ID, &summary);
    dialog.show_item(super::layout::FILTER_EMPTY_ID, empty);
    if empty && crate::win32::dialog_native::user32::get_focus() == hwnd {
        crate::win32::dialog_native::user32::set_focus(dialog.item(super::layout::FILTER_ID));
    }
    dialog.show_item(super::layout::LIST_ID, !empty);
    super::path_panel::update_details(content, dialog);
    super::actions::update_buttons(content, dialog);
}

/// C# `OnSelectionChanged()`: `path_panel::update_details` then `actions::update_buttons`.
pub fn on_selection_changed(content: &RecoveryContent, dialog: &NativeDialog) {
    super::path_panel::update_details(content, dialog);
    super::actions::update_buttons(content, dialog);
}

/// C# `OnRowResultChanged(row)` (lines 367-381), fed by `CheckEffect::RowChanged(row_index)`: nothing when the list window is `0`; `InvalidateRect(list,
/// erase)` (the "Checking..." badge becomes the result); when `row_index` is the single selected row: `update_details` and `update_buttons`.
pub fn on_row_result_changed(content: &RecoveryContent, dialog: &NativeDialog, row_index: usize) {
    let hwnd = dialog.item(super::layout::LIST_ID);
    if hwnd == 0 {
        return;
    }
    crate::win32::dialog_native::user32::invalidate_rect(hwnd, None, false);
    if single_selected(content, dialog) == Some(row_index) {
        on_selection_changed(content, dialog);
    }
}
