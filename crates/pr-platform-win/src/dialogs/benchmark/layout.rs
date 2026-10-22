//! Port of `BenchmarkDialog.cs` lines 21 to 42 (the ids), 105 to 136 (`BuildItems`) and 566 to 660 (the whole layout: `Margin`, `FieldHeight`, `ButtonHeight`,
//! `PreferredClientSize`, `MinimumClientSize`, `LayoutControls`, `FitColumns`), baseline `d92d6aae`. Unit W8-18r. Contract: `contracts/W8-18r.md`.
//!
//! Everything arithmetic is a pure function of a `&dyn DialogMeasure` (D-W8B5-501) that returns a plan; `apply_layout` executes the plan on a real dialog. The
//! status-line height of the last layout is the only state ([`LayoutState`], C# `_statusHeight`).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::Cell;

use crate::dialogs::native::dialog::NativeDialog;
use crate::dialogs::native::template::DialogItem;
use crate::dialogs::zoom_prompt::{ButtonRowPlan, DialogMeasure, Placement};

/// C# `HeaderId`.
pub const HEADER_ID: i32 = 100;
/// C# `DescriptionId`.
pub const DESCRIPTION_ID: i32 = 101;
/// C# `FolderId`: the folder box.
pub const FOLDER_ID: i32 = 102;
/// C# `BrowseId`.
pub const BROWSE_ID: i32 = 103;
/// C# `LimitLabelId`.
pub const LIMIT_LABEL_ID: i32 = 104;
/// C# `LimitId`: the image-limit box.
pub const LIMIT_ID: i32 = 105;
/// C# `ProfilesLabelId`.
pub const PROFILES_LABEL_ID: i32 = 106;
/// C# `SelectAllId`.
pub const SELECT_ALL_ID: i32 = 107;
/// C# `SelectNoneId`.
pub const SELECT_NONE_ID: i32 = 108;
/// C# `ProfilesId`: the multi-select owner-draw list of profiles.
pub const PROFILES_ID: i32 = 109;
/// C# `RunId` (the default button).
pub const RUN_ID: i32 = 110;
/// C# `QuickCheckId`.
pub const QUICK_CHECK_ID: i32 = 111;
/// C# `CancelRunId` (starts disabled).
pub const CANCEL_RUN_ID: i32 = 112;
/// C# `ProgressId`.
pub const PROGRESS_ID: i32 = 113;
/// C# `StatusId`.
pub const STATUS_ID: i32 = 114;
/// C# `ResultsId`: the result grid (list view).
pub const RESULTS_ID: i32 = 115;
/// C# `OpenReportId`.
pub const OPEN_REPORT_ID: i32 = 116;
/// C# `OpenFolderId`.
pub const OPEN_FOLDER_ID: i32 = 117;
/// C# `CloseId`.
pub const CLOSE_ID: i32 = 118;

/// C# `ColumnCount`: the columns of the grid (= `BenchmarkResultRow::cell_texts().len()`).
pub const COLUMN_COUNT: usize = 10;

/// C# `fixedUnits` of `FitColumns`, in dialog units: best marker, profile (flexible, 0 here), loading mode, workload, samples, P50, P95, P99, Max, status (flexible).
pub const FIXED_COLUMN_UNITS: [i32; COLUMN_COUNT] = [16, 0, 54, 62, 32, 36, 36, 36, 36, 0];

/// C# `BuildItems()` (static), pure: the 19 controls in Tab order. `child = DIALOG_STYLE_CHILD`, `tab = WS_TABSTOP`, `text = child | SS_NOPREFIX`, `edit = child | tab |
/// ES_AUTOHSCROLL`: Static `HEADER_ID` (`tr::benchmark_header()`, `text`); Static `DESCRIPTION_ID` (`benchmark_description`, `text`); Edit `FOLDER_ID` (`""`, `edit`, ex
/// `WS_EX_CLIENTEDGE`); Button `BROWSE_ID` (`benchmark_browse`, `child | tab | BS_PUSHBUTTON`); Static `LIMIT_LABEL_ID` (`benchmark_image_limit_label`, `text`); Edit
/// `LIMIT_ID` (`edit`, ex `WS_EX_CLIENTEDGE`); Static `PROFILES_LABEL_ID` (`benchmark_profiles_label`, `text`); Button `SELECT_ALL_ID` (`benchmark_select_all`) and
/// `SELECT_NONE_ID` (`benchmark_select_none`), `child | tab | BS_PUSHBUTTON`; ListBox `PROFILES_ID` (`""`, `child | tab | WS_VSCROLL | LBS_NOTIFY |
/// LBS_NOINTEGRALHEIGHT | LBS_MULTIPLESEL | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS`, ex `WS_EX_CLIENTEDGE`); Button `RUN_ID` (`benchmark_run`, `child | tab |
/// BS_DEFPUSHBUTTON`); Button `QUICK_CHECK_ID` (`benchmark_quick_check`, `child | tab | BS_PUSHBUTTON`); Button `CANCEL_RUN_ID` (`common_cancel`, `child | tab |
/// BS_PUSHBUTTON | WS_DISABLED`); Progress `PROGRESS_ID` (`""`, `child | ProgressControl::DEFAULT_STYLE`); Static `STATUS_ID` (`""`, `text`); ListView `RESULTS_ID`
/// (`""`, `child | ListViewControl::DEFAULT_STYLE`); Button `OPEN_REPORT_ID` (`benchmark_open_report`), `OPEN_FOLDER_ID` (`benchmark_open_result_folder`), `CLOSE_ID`
/// (`common_close`), each `child | tab | BS_PUSHBUTTON`. Texts are the ambient language at the call.
pub fn build_items() -> Vec<DialogItem> {
    todo!()
}

/// C# `Margin` = `dx(10)`.
pub fn margin(m: &dyn DialogMeasure) -> i32 {
    todo!()
}

/// C# `FieldHeight` and `ButtonHeight` (equal): `max(dy(14), line_height + dy(6))`.
pub fn field_height(m: &dyn DialogMeasure) -> i32 {
    todo!()
}

/// C# `PreferredClientSize()` = `(dx(520), dy(330))`.
pub fn preferred_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    todo!()
}

/// C# `MinimumClientSize()` = `(max(dx(400), button_row_width([OPEN_REPORT_ID, OPEN_FOLDER_ID, CLOSE_ID]) + 2 * margin), dy(280))`.
pub fn minimum_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    todo!()
}

/// C# `FitColumns(listWidth)` arithmetic, pure: `fixed_px = sum(dx(u) for u in FIXED_COLUMN_UNITS)` (each unit converted separately); `flex = max(0, list_width -
/// fixed_px - dx(14))` (the vertical scroll bar); `profile = max(dx(70), flex * 2 / 3)`; `status = max(dx(50), flex - profile)`; column `i` is `profile` for 1, `status` for
/// 9, else `dx(FIXED_COLUMN_UNITS[i])`.
pub fn column_widths(m: &dyn DialogMeasure, list_width: i32) -> [i32; COLUMN_COUNT] {
    todo!()
}

/// What `layout_plan` decided, in the C# call order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkLayoutPlan {
    /// Every `Place` in order: header, description, folder, browse, limit label, limit, profiles label, select all, select none, profiles, run, quick check, cancel run,
    /// progress, status, results. (The bottom button row is a separate call in the C#, made just before the results.)
    pub placements: Vec<Placement>,
    /// `SetListItemHeight(ProfilesId, ..)`, called right BEFORE the profile list is placed: `line_height + dy(5)`.
    pub profiles_item_height: i32,
    /// `[OPEN_REPORT_ID, OPEN_FOLDER_ID, CLOSE_ID]` at the bottom; applied after the status line and before the results.
    pub button_row: ButtonRowPlan,
    /// The height given to the status line (`0` when its text is empty): becomes `_statusHeight`.
    pub status_height: i32,
    /// `column_widths(m, content)`, applied after the results are placed (only when the grid exists).
    pub column_widths: [i32; COLUMN_COUNT],
}

/// C# `LayoutControls(clientWidth, clientHeight)` (lines 579 to 639), the whole body: `m = margin`; `content = max(1, client_width - 2 * m)`; `y = m`. Header: `h =
/// measure_item_height(HEADER_ID, content)`, place `(m, y, content, h)`, `y += h + dy(2)`. Description the same with `y += h + dy(8)`. Folder row: `browse =
/// button_width([BROWSE_ID])`; `Place(FOLDER_ID, m, y, max(dx(40), content - browse - dx(4)), fh)`; `Place(BROWSE_ID, m + content - browse, y, browse, fh)`; `y += fh +
/// dy(6)` (`fh = field_height`). Limit row: `lw = measure_text_width(item_text(LIMIT_LABEL_ID))`; `Place(LIMIT_LABEL_ID, m, y + (fh - line_height) / 2, lw + dx(2),
/// line_height)`; `Place(LIMIT_ID, m + lw + dx(8), y, dx(40), fh)`; `y += fh + dy(6)`. Profiles label row: `buttons = button_width([SELECT_ALL_ID, SELECT_NONE_ID])`;
/// `label_width = max(dx(60), content - 2 * buttons - dx(12))`; `label = measure_item_height(PROFILES_LABEL_ID, label_width)`; `row = max(label, fh)`;
/// `Place(PROFILES_LABEL_ID, m, y + (row - label) / 2, label_width, label)`; `Place(SELECT_ALL_ID, m + content - 2 * buttons - dx(4), y + (row - fh) / 2, buttons, fh)`;
/// `Place(SELECT_NONE_ID, m + content - buttons, same y, buttons, fh)`; `y += row + dy(3)`. Profile list: `profiles_item_height = line_height + dy(5)`, `list_height =
/// dy(64)`, `Place(PROFILES_ID, m, y, content, dy(64))`, `y += dy(64) + dy(6)`. Run row: `run = button_width([RUN_ID, QUICK_CHECK_ID, CANCEL_RUN_ID])`; `Place(RUN_ID, m, y,
/// run, fh)`; `Place(QUICK_CHECK_ID, m + run + dx(6), y, run, fh)`; `Place(CANCEL_RUN_ID, m + 2 * (run + dx(6)), y, run, fh)`; `y += fh + dy(8)`. Progress:
/// `Place(PROGRESS_ID, m, y, content, max(dy(4), dy(5)))`, `y += dy(5) + dy(4)`. Status: `status = 0` when `item_text(STATUS_ID)` is empty, else
/// `measure_item_height(STATUS_ID, content)`; `Place(STATUS_ID, m, y, content, status)`; `y += status + dy(6)`. Bottom: `bottom_top = client_height - m - fh`; the button row at
/// `bottom_top` (right margin `m`, height `fh`); `results = max(dy(30), bottom_top - dy(8) - y)`; `Place(RESULTS_ID, m, y, content, results)`; `column_widths(m, content)`.
/// Wrapping `i32` arithmetic and truncating division (D-W8B5-312).
pub fn layout_plan(m: &dyn DialogMeasure, client_width: i32, client_height: i32) -> BenchmarkLayoutPlan {
    todo!()
}

/// C# `_statusHeight`: the status-line height the last layout gave (`SetStatus` re-runs the layout only when the new text needs another height).
#[derive(Debug, Default)]
pub struct LayoutState {
    /// C# `_statusHeight`; starts at `0`.
    pub status_height: Cell<i32>,
}

/// Executes a plan on a live dialog, in the C# order: placements up to the profile list (with `set_list_item_height(PROFILES_ID, ..)` right before the
/// `PROFILES_ID` placement), the rest, the bottom button row right before the `RESULTS_ID` placement, then the column widths (`ListViewControl::set_column_width` for
/// each column, only when the grid window exists) and `state.status_height.set(plan.status_height)`.
pub fn apply_layout(dialog: &NativeDialog, plan: &BenchmarkLayoutPlan, state: &LayoutState) {
    todo!()
}
