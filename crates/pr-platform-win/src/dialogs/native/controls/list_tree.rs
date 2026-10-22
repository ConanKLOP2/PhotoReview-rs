//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/NativeControls.cs` lines 254-498 (baseline `d92d6aae`): `ListViewControl`. Unit W8-18g. Contract:
//! `contracts/W8-18g.md`. (The plan calls this file `list_tree`; the C# class is the report-mode list view. Progress, trackbar and check box are `custom`.)
//!
//! A list view in report mode: columns, rows, row selection, the sort arrow, and the two `NM_CUSTOMDRAW` handlers (body rows, and the header cells which the
//! dark theme would draw in unreadable grey). Colours and the classic theme are applied by `chrome::DarkDialogChrome`.
//!
//! The C# code builds `LVCOLUMN`/`LVITEM`/`HDITEM` as raw byte buffers at hard-coded offsets; Rust uses the `windows` structs, whose x64 layout equals those
//! offsets (asserted in the tests against `control_constants::{LVCOLUMNW_SIZE, LVITEMW_SIZE, HDITEMW_SIZE}`). The notification structs come as references: the
//! caller (the dialog's `WM_NOTIFY`, W8-18j) turns `lParam` into `&mut NMLVCUSTOMDRAW` / `&NMCUSTOMDRAW` under its own SAFETY.
use windows::Win32::Foundation::{COLORREF, LPARAM};
use windows::Win32::UI::Controls::{
    HDI_MASK, HDITEMW, LIST_VIEW_ITEM_FLAGS, LIST_VIEW_ITEM_STATE_FLAGS, LVCOLUMNW, LVCOLUMNW_MASK, LVITEMW,
    NMCUSTOMDRAW, NMLVCUSTOMDRAW,
};

use crate::win32::control_constants::*;
use crate::win32::dialog_native::{
    self, DT_CENTER, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, WS_BORDER, WS_TABSTOP,
};
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
        let _ = super::basic::send_int(
            lv,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_LABELTIP) as isize,
        );
        for (message, color) in [
            (LVM_SETBKCOLOR, background),
            (LVM_SETTEXTBKCOLOR, background),
            (LVM_SETTEXTCOLOR, text),
        ] {
            let _ = super::basic::send_int(lv, message, 0, color as isize);
        }
    }

    /// C# `AddColumn(lv, index, text, width, rightAligned)`: `LVM_INSERTCOLUMN(index)` with an `LVCOLUMNW` of `mask = LVCF_FMT | LVCF_WIDTH | LVCF_TEXT |
    /// LVCF_SUBITEM`, `fmt = LVCFMT_RIGHT` or `LVCFMT_LEFT`, `cx = width`, `pszText = text`, `iSubItem = index`.
    pub fn add_column(lv: Hwnd, index: i32, text: &str, width: i32, right_aligned: bool) {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let column = LVCOLUMNW {
            mask: LVCOLUMNW_MASK(LVCF_FMT | LVCF_WIDTH | LVCF_TEXT | LVCF_SUBITEM),
            fmt: windows::Win32::UI::Controls::LVCOLUMNW_FORMAT(if right_aligned { LVCFMT_RIGHT } else { LVCFMT_LEFT }),
            cx: width,
            pszText: windows::core::PWSTR(wide.as_ptr() as *mut _),
            iSubItem: index,
            ..Default::default()
        };
        // SAFETY: the live local column and NUL-terminated text buffer remain valid for this synchronous message.
        let _ = unsafe {
            dialog_native::user32::send_message(
                lv,
                LVM_INSERTCOLUMN,
                index as usize,
                (&column as *const LVCOLUMNW) as isize,
            )
        };
    }

    /// C# `SetColumnWidth(lv, index, width)`: `LVM_SETCOLUMNWIDTH`.
    pub fn set_column_width(lv: Hwnd, index: i32, width: i32) {
        let _ = super::basic::send_int(lv, LVM_SETCOLUMNWIDTH, index as usize, width as isize);
    }

    /// C# `ColumnWidth(lv, index)`: `LVM_GETCOLUMNWIDTH`.
    pub fn column_width(lv: Hwnd, index: i32) -> i32 {
        super::basic::send_int(lv, LVM_GETCOLUMNWIDTH, index as usize, 0) as i32
    }

    /// C# `SetColumnText(lv, index, text)`: `LVM_SETCOLUMNW(index)` with an `LVCOLUMNW` whose `mask = LVCF_TEXT` and `pszText = text`.
    pub fn set_column_text(lv: Hwnd, index: i32, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let column = LVCOLUMNW {
            mask: LVCOLUMNW_MASK(LVCF_TEXT),
            pszText: windows::core::PWSTR(wide.as_ptr() as *mut _),
            ..Default::default()
        };
        // SAFETY: the live local column and NUL-terminated text buffer remain valid for this synchronous message.
        let _ = unsafe {
            dialog_native::user32::send_message(
                lv,
                LVM_SETCOLUMNW,
                index as usize,
                (&column as *const LVCOLUMNW) as isize,
            )
        };
    }

    /// C# `Count(lv)`: `LVM_GETITEMCOUNT`.
    pub fn count(lv: Hwnd) -> i32 {
        super::basic::send_int(lv, LVM_GETITEMCOUNT, 0, 0) as i32
    }

    /// C# `Clear(lv)`: `LVM_DELETEALLITEMS`.
    pub fn clear(lv: Hwnd) {
        let _ = super::basic::send_int(lv, LVM_DELETEALLITEMS, 0, 0);
    }

    /// C# `AddRow(lv, cells, param)`: appends a row. `index = count(lv)`; `LVM_INSERTITEM` with an `LVITEMW` of `mask = LVIF_TEXT | LVIF_PARAM`, `iItem =
    /// index`, `pszText = cells[0]` (`""` when `cells` is empty), `lParam = param`; `index` becomes the answer (`-1` when the insert failed: the C# does not
    /// check it and still calls `set_cell` with it, which then does nothing); then `set_cell(lv, index, column, cells[column])` for `column in 1..cells.len()`.
    /// Returns the row index.
    pub fn add_row(lv: Hwnd, cells: &[String], param: isize) -> i32 {
        let first = cells.first().map(String::as_str).unwrap_or("");
        let wide: Vec<u16> = first.encode_utf16().chain(std::iter::once(0)).collect();
        let row = Self::count(lv);
        let item = LVITEMW {
            mask: LIST_VIEW_ITEM_FLAGS(LVIF_TEXT | LVIF_PARAM),
            iItem: row,
            pszText: windows::core::PWSTR(wide.as_ptr() as *mut _),
            lParam: LPARAM(param),
            ..Default::default()
        };
        // SAFETY: the live local item and its text buffer remain valid for this synchronous message.
        let inserted =
            unsafe { dialog_native::user32::send_message(lv, LVM_INSERTITEM, 0, (&item as *const LVITEMW) as isize) }
                as i32;
        for (column, cell) in cells.iter().enumerate().skip(1) {
            Self::set_cell(lv, inserted, column as i32, cell);
        }
        inserted
    }

    /// C# `SetCell(lv, row, column, text)`: `LVM_SETITEMTEXT(row)` with an `LVITEMW` of `iSubItem = column` and `pszText = text`.
    pub fn set_cell(lv: Hwnd, row: i32, column: i32, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let item = LVITEMW {
            iSubItem: column,
            pszText: windows::core::PWSTR(wide.as_ptr() as *mut _),
            ..Default::default()
        };
        // SAFETY: the live local item and NUL-terminated text buffer remain valid for this synchronous message.
        let _ = unsafe {
            dialog_native::user32::send_message(lv, LVM_SETITEMTEXT, row as usize, (&item as *const LVITEMW) as isize)
        };
    }

    /// C# `CellText(lv, row, column)`: `LVM_GETITEMTEXT(row)` into a buffer of `CELL_TEXT_CAPACITY` units; the answer is the length copied, clamped to
    /// `0..=CELL_TEXT_CAPACITY - 1`; the first that many units as the string.
    pub fn cell_text(lv: Hwnd, row: i32, column: i32) -> String {
        let mut buffer = vec![0u16; CELL_TEXT_CAPACITY];
        let mut item = LVITEMW {
            iSubItem: column,
            pszText: windows::core::PWSTR(buffer.as_mut_ptr()),
            cchTextMax: CELL_TEXT_CAPACITY as i32,
            ..Default::default()
        };
        // SAFETY: item and its initialized writable buffer live across the synchronous message; capacity matches cchTextMax.
        let length = unsafe {
            dialog_native::user32::send_message(lv, LVM_GETITEMTEXT, row as usize, (&mut item as *mut LVITEMW) as isize)
        }
        .clamp(0, (CELL_TEXT_CAPACITY - 1) as isize) as usize;
        String::from_utf16_lossy(buffer.get(..length).unwrap_or(&[]))
    }

    /// C# `RowParam(lv, row)`: `LVM_GETITEM` with `mask = LVIF_PARAM`, `iItem = row`; the item's `lParam` when the message succeeded, else `-1`.
    pub fn row_param(lv: Hwnd, row: i32) -> isize {
        let mut item = LVITEMW {
            mask: LIST_VIEW_ITEM_FLAGS(LVIF_PARAM),
            iItem: row,
            ..Default::default()
        };
        // SAFETY: the live local item is writable by this synchronous message.
        if unsafe { dialog_native::user32::send_message(lv, LVM_GETITEM, 0, (&mut item as *mut LVITEMW) as isize) } != 0
        {
            item.lParam.0
        } else {
            -1
        }
    }

    /// C# `SelectedRow(lv)`: `LVM_GETNEXTITEM(-1, LVNI_SELECTED)`, `-1` when none.
    pub fn selected_row(lv: Hwnd) -> i32 {
        super::basic::send_int(lv, LVM_GETNEXTITEM, (-1isize) as usize, LVNI_SELECTED as isize) as i32
    }

    /// C# `SelectRow(lv, row)`: `LVM_SETITEMSTATE(row)` with `state = stateMask = LVIS_SELECTED | LVIS_FOCUSED`; then, only when `row >= 0`,
    /// `LVM_ENSUREVISIBLE(row)`. (`row == -1` applies the state to EVERY item in the OS: kept, not "fixed"; see the contract.)
    pub fn select_row(lv: Hwnd, row: i32) {
        let item = LVITEMW {
            state: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED | LVIS_FOCUSED),
            stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED | LVIS_FOCUSED),
            ..Default::default()
        };
        // SAFETY: the live local state item is read synchronously by the control.
        let _ = unsafe {
            dialog_native::user32::send_message(lv, LVM_SETITEMSTATE, row as usize, (&item as *const LVITEMW) as isize)
        };
        if row >= 0 {
            let _ = super::basic::send_int(lv, LVM_ENSUREVISIBLE, row as usize, 0);
        }
    }

    /// C# `IsRowSelected(lv, row)`: `LVM_GETITEMSTATE(row, LVIS_SELECTED)` has the `LVIS_SELECTED` bit.
    pub fn is_row_selected(lv: Hwnd, row: i32) -> bool {
        (super::basic::send_int(lv, LVM_GETITEMSTATE, row as usize, LVIS_SELECTED as isize) as u32 & LVIS_SELECTED) != 0
    }

    /// C# `CustomDraw(header, rowColors)` (`NM_CUSTOMDRAW` of the list view; `custom_draw` is the `NMLVCUSTOMDRAW`): colours each row (best row, failure,
    /// alternating). `stage = nmcd.dwDrawStage`: `CDDS_PREPAINT` -> `CDRF_NOTIFYITEMDRAW`; `CDDS_ITEMPREPAINT` or `CDDS_ITEMPREPAINT | CDDS_SUBITEM` -> `(back,
    /// text) = row_colors(nmcd.dwItemSpec as i32, nmcd.lItemlParam)`, `clrText = text`, `clrTextBk = back`, return `CDRF_NEWFONT`; any other stage ->
    /// `CDRF_DODEFAULT`. The result goes to `DWLP_MSGRESULT` (the caller).
    pub fn custom_draw(
        custom_draw: &mut NMLVCUSTOMDRAW,
        row_colors: &mut dyn FnMut(i32, isize) -> (u32, u32),
    ) -> isize {
        let stage = custom_draw.nmcd.dwDrawStage.0;
        if stage == CDDS_PREPAINT {
            return CDRF_NOTIFYITEMDRAW;
        }
        if stage == CDDS_ITEMPREPAINT || stage == (CDDS_ITEMPREPAINT | CDDS_SUBITEM) {
            let (back, text) = row_colors(custom_draw.nmcd.dwItemSpec as i32, custom_draw.nmcd.lItemlParam.0);
            custom_draw.clrText = COLORREF(text);
            custom_draw.clrTextBk = COLORREF(back);
            CDRF_NEWFONT
        } else {
            CDRF_DODEFAULT
        }
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
        let stage = custom_draw.dwDrawStage.0;
        if stage == CDDS_PREPAINT {
            return CDRF_NOTIFYITEMDRAW;
        }
        if stage != CDDS_ITEMPREPAINT {
            return CDRF_DODEFAULT;
        }
        let hwnd = custom_draw.hdr.hwndFrom.0 as isize;
        let mut text_buffer = [0u16; HEADER_TEXT_CAPACITY];
        let mut item = HDITEMW {
            mask: HDI_MASK(HDI_FORMAT | HDI_TEXT),
            pszText: windows::core::PWSTR(text_buffer.as_mut_ptr()),
            cchTextMax: HEADER_TEXT_CAPACITY as i32,
            ..Default::default()
        };
        // SAFETY: item and its writable buffer remain valid for the synchronous HDM_GETITEM call.
        let ok = unsafe {
            dialog_native::user32::send_message(
                hwnd,
                HDM_GETITEM,
                custom_draw.dwItemSpec,
                (&mut item as *mut HDITEMW) as isize,
            )
        } != 0;
        let fmt = if ok { item.fmt.0 } else { 0 };
        let text = if ok {
            // SAFETY: when the control retains our pointer, the bounded local array is valid; when repointed, the notification's header owns the NUL-terminated text during this synchronous draw.
            unsafe {
                if item.pszText.0 == text_buffer.as_mut_ptr() {
                    let end = text_buffer.iter().position(|c| *c == 0).unwrap_or(HEADER_TEXT_CAPACITY);
                    String::from_utf16_lossy(&text_buffer[..end])
                } else {
                    let mut len = 0usize;
                    while len < HEADER_TEXT_CAPACITY && *item.pszText.0.add(len) != 0 {
                        len += 1;
                    }
                    String::from_utf16_lossy(std::slice::from_raw_parts(item.pszText.0, len))
                }
            }
        } else {
            String::new()
        };
        let rect = custom_draw.rc;
        use crate::dialogs::native::chrome::DialogColors;
        let surface = DialogColors::from_key("Dark.Surface").unwrap_or(0);
        let border = DialogColors::from_key("Dark.BorderSubtle").unwrap_or(0);
        let brush = crate::win32::dialog_native::gdi32::create_solid_brush(surface);
        let _ = crate::win32::comctl::user32::fill_rect(custom_draw.hdc.0 as isize, &rect, brush);
        let edge_brush = crate::win32::dialog_native::gdi32::create_solid_brush(border);
        let right = Rect {
            left: rect.right.saturating_sub(1),
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        let bottom = Rect {
            left: rect.left,
            top: rect.bottom.saturating_sub(1),
            right: rect.right,
            bottom: rect.bottom,
        };
        let _ = crate::win32::comctl::user32::fill_rect(custom_draw.hdc.0 as isize, &right, edge_brush);
        let _ = crate::win32::comctl::user32::fill_rect(custom_draw.hdc.0 as isize, &bottom, edge_brush);
        let _ = crate::win32::dialog_native::gdi32::delete_object(brush);
        let _ = crate::win32::dialog_native::gdi32::delete_object(edge_brush);
        // SAFETY: WM_GETFONT uses only the header handle and has no pointer parameters.
        let font = unsafe { dialog_native::user32::send_message(hwnd, dialog_native::WM_GETFONT, 0, 0) };
        let old_font = if font != 0 {
            crate::win32::dialog_native::gdi32::select_object(custom_draw.hdc.0 as isize, font)
        } else {
            0
        };
        let _ = crate::win32::comctl::gdi32::set_bk_mode(custom_draw.hdc.0 as isize, dialog_native::TRANSPARENT);
        let strong = DialogColors::text_strong();
        let secondary = DialogColors::text_secondary();
        let glyph = sort_glyph(custom_draw.dwItemSpec as i32, sort_column, sort_direction);
        let (mut text_rect, glyph_rect) = header_item_layout(rect, pad_x, glyph.is_some());
        if let (Some(glyph), Some(mut glyph_rect)) = (glyph, glyph_rect) {
            let _ = crate::win32::dialog_native::gdi32::set_text_color(custom_draw.hdc.0 as isize, secondary);
            let _ = dialog_native::user32::draw_text(
                custom_draw.hdc.0 as isize,
                glyph,
                &mut glyph_rect,
                DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
            );
        }
        let _ = crate::win32::dialog_native::gdi32::set_text_color(custom_draw.hdc.0 as isize, strong);
        let align = if fmt & HDF_RIGHT != 0 { DT_RIGHT } else { 0 };
        let _ = dialog_native::user32::draw_text(
            custom_draw.hdc.0 as isize,
            &text,
            &mut text_rect,
            DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS | align,
        );
        if old_font != 0 {
            let _ = crate::win32::dialog_native::gdi32::select_object(custom_draw.hdc.0 as isize, old_font);
        }
        CDRF_SKIPDEFAULT
    }

    /// C# `Header(lv)`: `LVM_GETHEADER`, the header control window (`0` when none).
    pub fn header(lv: Hwnd) -> Hwnd {
        super::basic::send_int(lv, LVM_GETHEADER, 0, 0) as Hwnd
    }
}

/// The sort glyph of a header cell (pure): `None` unless `column == sort_column`; `sort_direction == 1` gives `SORT_ASCENDING_GLYPH`, `2` gives
/// `SORT_DESCENDING_GLYPH`, anything else `None`.
pub fn sort_glyph(column: i32, sort_column: i32, sort_direction: i32) -> Option<&'static str> {
    if column != sort_column {
        None
    } else if sort_direction == 1 {
        Some(SORT_ASCENDING_GLYPH)
    } else if sort_direction == 2 {
        Some(SORT_DESCENDING_GLYPH)
    } else {
        None
    }
}

/// The rectangles of a header cell (pure, from C# `DrawHeaderItem`): `text = (rc.left + pad_x, rc.top, rc.right - pad_x, rc.bottom - 1)`; with a glyph
/// `text.right -= pad_x * 2` and `glyph = (rc.right - pad_x * 3, rc.top, rc.right - pad_x, rc.bottom - 1)`.
pub fn header_item_layout(rc: Rect, pad_x: i32, has_glyph: bool) -> (Rect, Option<Rect>) {
    let bottom = rc.bottom.saturating_sub(1);
    let mut text = Rect {
        left: rc.left.saturating_add(pad_x),
        top: rc.top,
        right: rc.right.saturating_sub(pad_x),
        bottom,
    };
    if has_glyph {
        text.right = text.right.saturating_sub(pad_x.saturating_mul(2));
        let glyph = Rect {
            left: rc.right.saturating_sub(pad_x.saturating_mul(3)),
            top: rc.top,
            right: rc.right.saturating_sub(pad_x),
            bottom,
        };
        (text, Some(glyph))
    } else {
        (text, None)
    }
}
