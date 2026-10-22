//! Port of `BenchmarkDialog.cs` lines 332 to 396 (`OnNotify`, `RowColors`, `ProfileBadge`, `ListItemText`, `StaticTextColor`), baseline `d92d6aae`. Unit W8-18s.
//! Contract: `contracts/W8-18s.md`.
//!
//! The grid and list side of the window: the custom draw of the result rows (best = green, failed = red, the rest alternate), the header click that sorts, the text and
//! badge of the owner-draw profile list and the colours of the two secondary static texts. The sort itself (`sort_by`, `display_order`, `compare_rows`) is 18r's `state.rs`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::presenters::benchmark_presentation::BenchmarkProfileItem;
use pr_l10n::localizer::Localizer;

use super::state::BenchmarkContent;
use crate::dialogs::native::dialog::{ListItemText, NativeDialog};
use crate::win32::structs::{Lparam, Lresult};

/// The background colour key of a result row, pure (C# `RowColors`): `is_best` -> `"Dark.SuccessBackground"` (checked FIRST: a best row that failed cannot exist, but the
/// order is the C#); else `is_fail` -> `"Dark.ErrorBackground"`; else `"Dark.Panel"` when `displayed_row % 2 == 0` and `"Dark.RowAlt"` otherwise. The text colour is
/// always `DialogColors::text()`.
pub fn row_color_key(is_best: bool, is_fail: bool, displayed_row: i32) -> &'static str {
    todo!()
}

/// C# `ListItemText(ProfilesId, index)` for an index in range, pure: `Some(ListItemText { text: items[index].name(texts), badge: Some(loading_mode_text(items[index].profile
/// .loading_mode, texts)) })`; `None` for an index outside `0..items.len()` (the caller answers like the base class).
pub fn profile_list_text(items: &[BenchmarkProfileItem], index: i32, texts: &Localizer) -> Option<ListItemText> {
    todo!()
}

/// The column of an `LVN_COLUMNCLICK` notification (C# `*(int*)(header + 28)` = `NMLISTVIEW.iSubItem`). `header` is the `lParam` (a `*const NMLISTVIEW`, valid for the call
/// only); `0` -> `None`. A typed read under a SAFETY comment with an `offset_of!(NMLISTVIEW, iSubItem) == 28` assertion (D-W8B5-311).
pub fn column_click_sub_item(header: Lparam) -> Option<i32> {
    todo!()
}

impl BenchmarkContent {
    /// C# `OnNotify(id, code, header, out result)`: `id != RESULTS_ID` -> `None`. `code == NM_CUSTOMDRAW` -> `Some(ListViewControl::custom_draw(&mut NMLVCUSTOMDRAW,
    /// &mut |displayed, source| self.row_colors(displayed, source)))` (the `lParam` becomes `&mut NMLVCUSTOMDRAW` under a SAFETY comment; null -> `None`);
    /// `code == LVN_COLUMNCLICK` -> `sort_by(dialog, column_click_sub_item(header))` and `Some(0)` (a null header -> `None`); any other code `None`.
    pub(super) fn notify(&self, dialog: &NativeDialog, id: i32, code: i32, header: Lparam) -> Option<Lresult> {
        todo!()
    }

    /// C# `RowColors(displayedRow, source)`: `source` is the row's `lParam` (its index in `presenter.rows()`). `presenter.try_borrow()`; a source in range gives
    /// `(from_key(row_color_key(row.is_best, row.status() == Fail, displayed)), DialogColors::text())`; a source out of range, or a presenter that is borrowed (cannot
    /// happen), gives the alternating colour of `displayed_row`. A missing palette key falls back to `DialogColors::window()`.
    pub(super) fn row_colors(&self, displayed_row: i32, source: isize) -> (u32, u32) {
        todo!()
    }

    /// C# `ListItemText(id, index)`: `PROFILES_ID` with `profile_list_text(..)` `Some` -> that (names and badges are read at paint time with the ambient language, so a language
    /// switch only needs an `invalidate_rect`); anything else the base answer `ListItemText { text: ListBoxControl::text_at(dialog.item(id), index), badge: None }`.
    pub(super) fn list_text(&self, dialog: &NativeDialog, id: i32, index: i32) -> ListItemText {
        todo!()
    }

    /// C# `StaticTextColor(id)`: `DESCRIPTION_ID` -> `DialogColors::from_key("Dark.TextSubtle")` (missing key: the base answer); `STATUS_ID` ->
    /// `DialogColors::text_secondary()`; else `DialogColors::text()`.
    pub(super) fn static_color(&self, dialog: &NativeDialog, id: i32) -> u32 {
        todo!()
    }
}
