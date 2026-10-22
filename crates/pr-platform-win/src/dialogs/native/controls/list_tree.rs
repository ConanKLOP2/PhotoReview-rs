//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/NativeControls.cs` lines 254-498 (baseline `d92d6aae`): `ListViewControl`. Unit W8-18g. Contract:
//! `contracts/W8-18g.md`. (The plan calls this file `list_tree`; the C# class is the report-mode list view. Progress, trackbar and check box are `custom`.)
//!
//! A list view in report mode: columns, rows, row selection, the sort arrow, and the two `NM_CUSTOMDRAW` handlers (body rows, and the header cells which the
//! dark theme would draw in unreadable grey). Colours and the classic theme are applied by `chrome::DarkDialogChrome`.
//!
//! The C# code builds `LVCOLUMN`/`LVITEM`/`HDITEM` as raw byte buffers at hard-coded offsets; Rust uses the `windows` structs, whose x64 layout equals those
//! offsets (asserted in the tests against `control_constants::{LVCOLUMNW_SIZE, LVITEMW_SIZE, HDITEMW_SIZE}`). The notification structs come as references: the
//! caller (the dialog's `WM_NOTIFY`, W8-18j) turns `lParam` into `&mut NMLVCUSTOMDRAW` / `&NMCUSTOMDRAW` under its own SAFETY.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use windows::Win32::UI::Controls::{NMCUSTOMDRAW, NMLVCUSTOMDRAW};

use crate::win32::control_constants::{LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL};
use crate::win32::dialog_native::{WS_BORDER, WS_TABSTOP};
use crate::win32::structs::{Hwnd, Rect};

/// `LVM_SETCOLUMNW` (`0x1060`): a literal in the C# `SetColumnText`, not in `control_constants`.
pub const LVM_SETCOLUMNW: u32 = 0x1060;
/// `HDI_TEXT` (`0x2`): a literal in the C# `DrawHeaderItem`.
pub const HDI_TEXT: u32 = 0x0002;
/// `HDF_RIGHT` (`0x1`): a literal in the C# `DrawHeaderItem`.
pub const HDF_RIGHT: i32 = 0x0001;
/// `DT_RIGHT` (`0x2`), the alignment of a right-aligned header cell.
pub const DT_RIGHT: u32 = 0x0002;
/// Longest cell text `cell_text` reads (C# `capacity` 512 units including the NUL).
pub const CELL_TEXT_CAPACITY: usize = 512;
/// Longest header text `draw_header_item` reads into its own buffer (C# 256 units).
pub const HEADER_TEXT_CAPACITY: usize = 256;
/// The sort-ascending glyph of the header (C# `"▲"`).
pub const SORT_ASCENDING_GLYPH: &str = "\u{25B2}";
/// The sort-descending glyph of the header (C# `"▼"`).
pub const SORT_DESCENDING_GLYPH: &str = "\u{25BC}";

/// C# `ListViewControl`.
#[derive(Debug, Clone, Copy)]
pub struct ListViewControl;

impl ListViewControl {
    /// C# `DefaultStyle` = `LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | WS_TABSTOP | WS_BORDER`.
    pub const DEFAULT_STYLE: u32 = LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | WS_TABSTOP | WS_BORDER;

    /// C# `Configure(lv, background, text)`: `LVM_SETEXTENDEDLISTVIEWSTYLE(0, LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_LABELTIP)`, then
    /// `LVM_SETBKCOLOR`, `LVM_SETTEXTBKCOLOR` (both `background`) and `LVM_SETTEXTCOLOR` (`text`); the colours are `COLORREF`s passed zero-extended in `lParam`.
    pub fn configure(lv: Hwnd, background: u32, text: u32) {
        todo!()
    }

    /// C# `AddColumn(lv, index, text, width, rightAligned)`: `LVM_INSERTCOLUMN(index)` with an `LVCOLUMNW` of `mask = LVCF_FMT | LVCF_WIDTH | LVCF_TEXT |
    /// LVCF_SUBITEM`, `fmt = LVCFMT_RIGHT` or `LVCFMT_LEFT`, `cx = width`, `pszText = text`, `iSubItem = index`.
    pub fn add_column(lv: Hwnd, index: i32, text: &str, width: i32, right_aligned: bool) {
        todo!()
    }

    /// C# `SetColumnWidth(lv, index, width)`: `LVM_SETCOLUMNWIDTH`.
    pub fn set_column_width(lv: Hwnd, index: i32, width: i32) {
        todo!()
    }

    /// C# `ColumnWidth(lv, index)`: `LVM_GETCOLUMNWIDTH`.
    pub fn column_width(lv: Hwnd, index: i32) -> i32 {
        todo!()
    }

    /// C# `SetColumnText(lv, index, text)`: `LVM_SETCOLUMNW(index)` with an `LVCOLUMNW` whose `mask = LVCF_TEXT` and `pszText = text`.
    pub fn set_column_text(lv: Hwnd, index: i32, text: &str) {
        todo!()
    }

    /// C# `Count(lv)`: `LVM_GETITEMCOUNT`.
    pub fn count(lv: Hwnd) -> i32 {
        todo!()
    }

    /// C# `Clear(lv)`: `LVM_DELETEALLITEMS`.
    pub fn clear(lv: Hwnd) {
        todo!()
    }

    /// C# `AddRow(lv, cells, param)`: appends a row. `index = count(lv)`; `LVM_INSERTITEM` with an `LVITEMW` of `mask = LVIF_TEXT | LVIF_PARAM`, `iItem =
    /// index`, `pszText = cells[0]` (`""` when `cells` is empty), `lParam = param`; `index` becomes the answer (`-1` when the insert failed: the C# does not
    /// check it and still calls `set_cell` with it, which then does nothing); then `set_cell(lv, index, column, cells[column])` for `column in 1..cells.len()`.
    /// Returns the row index.
    pub fn add_row(lv: Hwnd, cells: &[String], param: isize) -> i32 {
        todo!()
    }

    /// C# `SetCell(lv, row, column, text)`: `LVM_SETITEMTEXT(row)` with an `LVITEMW` of `iSubItem = column` and `pszText = text`.
    pub fn set_cell(lv: Hwnd, row: i32, column: i32, text: &str) {
        todo!()
    }

    /// C# `CellText(lv, row, column)`: `LVM_GETITEMTEXT(row)` into a buffer of `CELL_TEXT_CAPACITY` units; the answer is the length copied, clamped to
    /// `0..=CELL_TEXT_CAPACITY - 1`; the first that many units as the string.
    pub fn cell_text(lv: Hwnd, row: i32, column: i32) -> String {
        todo!()
    }

    /// C# `RowParam(lv, row)`: `LVM_GETITEM` with `mask = LVIF_PARAM`, `iItem = row`; the item's `lParam` when the message succeeded, else `-1`.
    pub fn row_param(lv: Hwnd, row: i32) -> isize {
        todo!()
    }

    /// C# `SelectedRow(lv)`: `LVM_GETNEXTITEM(-1, LVNI_SELECTED)`, `-1` when none.
    pub fn selected_row(lv: Hwnd) -> i32 {
        todo!()
    }

    /// C# `SelectRow(lv, row)`: `LVM_SETITEMSTATE(row)` with `state = stateMask = LVIS_SELECTED | LVIS_FOCUSED`; then, only when `row >= 0`,
    /// `LVM_ENSUREVISIBLE(row)`. (`row == -1` applies the state to EVERY item in the OS: kept, not "fixed"; see the contract.)
    pub fn select_row(lv: Hwnd, row: i32) {
        todo!()
    }

    /// C# `IsRowSelected(lv, row)`: `LVM_GETITEMSTATE(row, LVIS_SELECTED)` has the `LVIS_SELECTED` bit.
    pub fn is_row_selected(lv: Hwnd, row: i32) -> bool {
        todo!()
    }

    /// C# `CustomDraw(header, rowColors)` (`NM_CUSTOMDRAW` of the list view; `custom_draw` is the `NMLVCUSTOMDRAW`): colours each row (best row, failure,
    /// alternating). `stage = nmcd.dwDrawStage`: `CDDS_PREPAINT` -> `CDRF_NOTIFYITEMDRAW`; `CDDS_ITEMPREPAINT` or `CDDS_ITEMPREPAINT | CDDS_SUBITEM` -> `(back,
    /// text) = row_colors(nmcd.dwItemSpec as i32, nmcd.lItemlParam)`, `clrText = text`, `clrTextBk = back`, return `CDRF_NEWFONT`; any other stage ->
    /// `CDRF_DODEFAULT`. The result goes to `DWLP_MSGRESULT` (the caller).
    pub fn custom_draw(
        custom_draw: &mut NMLVCUSTOMDRAW,
        row_colors: &mut dyn FnMut(i32, isize) -> (u32, u32),
    ) -> isize {
        todo!()
    }

    /// C# `DrawHeaderItem(customDraw, padX, sortColumn, sortDirection)`: draws one header cell itself (background `Dark.Surface`, bright text, separators, sort
    /// arrow) because the theme draws unreadable grey text (`NM_CUSTOMDRAW` of the HEADER, forwarded by the list view to its parent; `hwndFrom` is the header).
    /// Stage `CDDS_PREPAINT` -> `CDRF_NOTIFYITEMDRAW`; any stage but `CDDS_ITEMPREPAINT` -> `CDRF_DODEFAULT`. Else: read the cell with `HDM_GETITEM`
    /// (`mask = HDI_FORMAT | HDI_TEXT`, a 256-unit buffer; the text pointer AFTER the call may point at the control's own storage); fill `rc` with
    /// `Dark.Surface`; a 1 px `Dark.BorderSubtle` line at the right edge and at the bottom edge; font = `WM_GETFONT` of the header (selected when non-zero);
    /// `SetBkMode(TRANSPARENT)`; text colour `DialogColors::text_strong()`; `header_item_layout` for the rectangles; the glyph (colour `text_secondary`,
    /// centred) then the text (colour `text_strong`, `DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS`, plus `DT_RIGHT` when `fmt & HDF_RIGHT`);
    /// restore the font; return `CDRF_SKIPDEFAULT`.
    pub fn draw_header_item(custom_draw: &NMCUSTOMDRAW, pad_x: i32, sort_column: i32, sort_direction: i32) -> isize {
        todo!()
    }

    /// C# `Header(lv)`: `LVM_GETHEADER`, the header control window (`0` when none).
    pub fn header(lv: Hwnd) -> Hwnd {
        todo!()
    }
}

/// The sort glyph of a header cell (pure): `None` unless `column == sort_column`; `sort_direction == 1` gives `SORT_ASCENDING_GLYPH`, `2` gives
/// `SORT_DESCENDING_GLYPH`, anything else `None`.
pub fn sort_glyph(column: i32, sort_column: i32, sort_direction: i32) -> Option<&'static str> {
    todo!()
}

/// The rectangles of a header cell (pure, from C# `DrawHeaderItem`): `text = (rc.left + pad_x, rc.top, rc.right - pad_x, rc.bottom - 1)`; with a glyph
/// `text.right -= pad_x * 2` and `glyph = (rc.right - pad_x * 3, rc.top, rc.right - pad_x, rc.bottom - 1)`.
pub fn header_item_layout(rc: Rect, pad_x: i32, has_glyph: bool) -> (Rect, Option<Rect>) {
    todo!()
}
