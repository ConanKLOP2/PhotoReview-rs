//! Port of `BenchmarkDialog.cs` lines 21 to 42 (the ids), 105 to 136 (`BuildItems`) and 566 to 660 (the whole layout: `Margin`, `FieldHeight`, `ButtonHeight`,
//! `PreferredClientSize`, `MinimumClientSize`, `LayoutControls`, `FitColumns`), baseline `d92d6aae`. Unit W8-18r. Contract: `contracts/W8-18r.md`.
//!
//! Everything arithmetic is a pure function of a `&dyn DialogMeasure` (D-W8B5-501) that returns a plan; `apply_layout` executes the plan on a real dialog. The
//! status-line height of the last layout is the only state ([`LayoutState`], C# `_statusHeight`).
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
    use crate::dialogs::native::controls::{custom::ProgressControl, list_tree::ListViewControl};
    use crate::dialogs::native::template::{DIALOG_STYLE_CHILD as child, DialogControlClass as C, DialogItem as I};
    use crate::win32::control_constants::{LBS_HASSTRINGS, LBS_MULTIPLESEL, LBS_OWNERDRAWFIXED};
    use crate::win32::dialog_native as n;
    let tab = n::WS_TABSTOP;
    let text = child | n::SS_NOPREFIX;
    let edit = child | tab | n::ES_AUTOHSCROLL;
    vec![
        I::new(C::Static, HEADER_ID, pr_l10n::tr::benchmark_header(), text),
        I::new(C::Static, DESCRIPTION_ID, pr_l10n::tr::benchmark_description(), text),
        I::new(C::Edit, FOLDER_ID, "", edit).with_ex_style(n::WS_EX_CLIENTEDGE),
        I::new(
            C::Button,
            BROWSE_ID,
            pr_l10n::tr::benchmark_browse(),
            child | tab | n::BS_PUSHBUTTON,
        ),
        I::new(
            C::Static,
            LIMIT_LABEL_ID,
            pr_l10n::tr::benchmark_image_limit_label(),
            text,
        ),
        I::new(C::Edit, LIMIT_ID, "", edit).with_ex_style(n::WS_EX_CLIENTEDGE),
        I::new(
            C::Static,
            PROFILES_LABEL_ID,
            pr_l10n::tr::benchmark_profiles_label(),
            text,
        ),
        I::new(
            C::Button,
            SELECT_ALL_ID,
            pr_l10n::tr::benchmark_select_all(),
            child | tab | n::BS_PUSHBUTTON,
        ),
        I::new(
            C::Button,
            SELECT_NONE_ID,
            pr_l10n::tr::benchmark_select_none(),
            child | tab | n::BS_PUSHBUTTON,
        ),
        I::new(
            C::ListBox,
            PROFILES_ID,
            "",
            child
                | tab
                | n::WS_VSCROLL
                | n::LBS_NOTIFY
                | n::LBS_NOINTEGRALHEIGHT
                | LBS_MULTIPLESEL
                | LBS_OWNERDRAWFIXED
                | LBS_HASSTRINGS,
        )
        .with_ex_style(n::WS_EX_CLIENTEDGE),
        I::new(
            C::Button,
            RUN_ID,
            pr_l10n::tr::benchmark_run(),
            child | tab | n::BS_DEFPUSHBUTTON,
        ),
        I::new(
            C::Button,
            QUICK_CHECK_ID,
            pr_l10n::tr::benchmark_quick_check(),
            child | tab | n::BS_PUSHBUTTON,
        ),
        I::new(
            C::Button,
            CANCEL_RUN_ID,
            pr_l10n::tr::common_cancel(),
            child | tab | n::BS_PUSHBUTTON | n::WS_DISABLED,
        ),
        I::new(C::Progress, PROGRESS_ID, "", child | ProgressControl::DEFAULT_STYLE),
        I::new(C::Static, STATUS_ID, "", text),
        I::new(C::ListView, RESULTS_ID, "", child | ListViewControl::DEFAULT_STYLE),
        I::new(
            C::Button,
            OPEN_REPORT_ID,
            pr_l10n::tr::benchmark_open_report(),
            child | tab | n::BS_PUSHBUTTON,
        ),
        I::new(
            C::Button,
            OPEN_FOLDER_ID,
            pr_l10n::tr::benchmark_open_result_folder(),
            child | tab | n::BS_PUSHBUTTON,
        ),
        I::new(
            C::Button,
            CLOSE_ID,
            pr_l10n::tr::common_close(),
            child | tab | n::BS_PUSHBUTTON,
        ),
    ]
}

/// C# `Margin` = `dx(10)`.
pub fn margin(m: &dyn DialogMeasure) -> i32 {
    m.dx(10)
}

/// C# `FieldHeight` and `ButtonHeight` (equal): `max(dy(14), line_height + dy(6))`.
pub fn field_height(m: &dyn DialogMeasure) -> i32 {
    m.dy(14).max(m.line_height().wrapping_add(m.dy(6)))
}

/// C# `PreferredClientSize()` = `(dx(520), dy(330))`.
pub fn preferred_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    (m.dx(520), m.dy(330))
}

/// C# `MinimumClientSize()` = `(max(dx(400), button_row_width([OPEN_REPORT_ID, OPEN_FOLDER_ID, CLOSE_ID]) + 2 * margin), dy(280))`.
pub fn minimum_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    (
        m.dx(400).max(
            m.button_row_width(&[OPEN_REPORT_ID, OPEN_FOLDER_ID, CLOSE_ID])
                .wrapping_add(margin(m).wrapping_mul(2)),
        ),
        m.dy(280),
    )
}

/// C# `FitColumns(listWidth)` arithmetic, pure: `fixed_px = sum(dx(u) for u in FIXED_COLUMN_UNITS)` (each unit converted separately); `flex = max(0, list_width -
/// fixed_px - dx(14))` (the vertical scroll bar); `profile = max(dx(70), flex * 2 / 3)`; `status = max(dx(50), flex - profile)`; column `i` is `profile` for 1, `status` for
/// 9, else `dx(FIXED_COLUMN_UNITS[i])`.
pub fn column_widths(m: &dyn DialogMeasure, list_width: i32) -> [i32; COLUMN_COUNT] {
    let fixed = FIXED_COLUMN_UNITS
        .iter()
        .fold(0i32, |sum, u| sum.wrapping_add(m.dx(*u)));
    let flex = list_width.wrapping_sub(fixed).wrapping_sub(m.dx(14)).max(0);
    let profile = m.dx(70).max(flex.wrapping_mul(2) / 3);
    let status = m.dx(50).max(flex.wrapping_sub(profile));
    std::array::from_fn(|i| match i {
        1 => profile,
        9 => status,
        _ => m.dx(FIXED_COLUMN_UNITS[i]),
    })
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
    let x = margin(m);
    let content = client_width.wrapping_sub(x.wrapping_mul(2)).max(1);
    let fh = field_height(m);
    let line = m.line_height();
    let mut y = x;
    let mut p = Vec::with_capacity(16);
    let mut place = |id, px, py, w, h| p.push(Placement::new(id, px, py, w, h));
    let h = m.measure_item_height(HEADER_ID, content);
    place(HEADER_ID, x, y, content, h);
    y = y.wrapping_add(h).wrapping_add(m.dy(2));
    let h = m.measure_item_height(DESCRIPTION_ID, content);
    place(DESCRIPTION_ID, x, y, content, h);
    y = y.wrapping_add(h).wrapping_add(m.dy(8));
    let browse = m.button_width(&[BROWSE_ID]);
    place(
        FOLDER_ID,
        x,
        y,
        m.dx(40).max(content.wrapping_sub(browse).wrapping_sub(m.dx(4))),
        fh,
    );
    place(BROWSE_ID, x.wrapping_add(content).wrapping_sub(browse), y, browse, fh);
    y = y.wrapping_add(fh).wrapping_add(m.dy(6));
    let lw = m.measure_text_width(&m.item_text(LIMIT_LABEL_ID));
    place(
        LIMIT_LABEL_ID,
        x,
        y.wrapping_add(fh.wrapping_sub(line) / 2),
        lw.wrapping_add(m.dx(2)),
        line,
    );
    place(LIMIT_ID, x.wrapping_add(lw).wrapping_add(m.dx(8)), y, m.dx(40), fh);
    y = y.wrapping_add(fh).wrapping_add(m.dy(6));
    let buttons = m.button_width(&[SELECT_ALL_ID, SELECT_NONE_ID]);
    let label_width = m
        .dx(60)
        .max(content.wrapping_sub(buttons.wrapping_mul(2)).wrapping_sub(m.dx(12)));
    let label = m.measure_item_height(PROFILES_LABEL_ID, label_width);
    let row = label.max(fh);
    place(
        PROFILES_LABEL_ID,
        x,
        y.wrapping_add(row.wrapping_sub(label) / 2),
        label_width,
        label,
    );
    let by = y.wrapping_add(row.wrapping_sub(fh) / 2);
    place(
        SELECT_ALL_ID,
        x.wrapping_add(content)
            .wrapping_sub(buttons.wrapping_mul(2))
            .wrapping_sub(m.dx(4)),
        by,
        buttons,
        fh,
    );
    place(
        SELECT_NONE_ID,
        x.wrapping_add(content).wrapping_sub(buttons),
        by,
        buttons,
        fh,
    );
    y = y.wrapping_add(row).wrapping_add(m.dy(3));
    let profiles_item_height = line.wrapping_add(m.dy(5));
    let list_height = m.dy(64);
    place(PROFILES_ID, x, y, content, list_height);
    y = y.wrapping_add(list_height).wrapping_add(m.dy(6));
    let run = m.button_width(&[RUN_ID, QUICK_CHECK_ID, CANCEL_RUN_ID]);
    place(RUN_ID, x, y, run, fh);
    place(QUICK_CHECK_ID, x.wrapping_add(run).wrapping_add(m.dx(6)), y, run, fh);
    place(
        CANCEL_RUN_ID,
        x.wrapping_add(run.wrapping_add(m.dx(6)).wrapping_mul(2)),
        y,
        run,
        fh,
    );
    y = y.wrapping_add(fh).wrapping_add(m.dy(8));
    place(PROGRESS_ID, x, y, content, m.dy(4).max(m.dy(5)));
    y = y.wrapping_add(m.dy(5)).wrapping_add(m.dy(4));
    let status_height = if m.item_text(STATUS_ID).is_empty() {
        0
    } else {
        m.measure_item_height(STATUS_ID, content)
    };
    place(STATUS_ID, x, y, content, status_height);
    y = y.wrapping_add(status_height).wrapping_add(m.dy(6));
    let bottom_top = client_height.wrapping_sub(x).wrapping_sub(fh);
    let button_row = ButtonRowPlan {
        client_width,
        right_margin: x,
        top: bottom_top,
        height: fh,
        ids: vec![OPEN_REPORT_ID, OPEN_FOLDER_ID, CLOSE_ID],
    };
    let results_height = m.dy(30).max(bottom_top.wrapping_sub(m.dy(8)).wrapping_sub(y));
    place(RESULTS_ID, x, y, content, results_height);
    BenchmarkLayoutPlan {
        placements: p,
        profiles_item_height,
        button_row,
        status_height,
        column_widths: column_widths(m, content),
    }
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
    use crate::dialogs::native::controls::list_tree::ListViewControl;
    for p in &plan.placements {
        if p.id == PROFILES_ID {
            dialog.set_list_item_height(PROFILES_ID, plan.profiles_item_height);
        }
        if p.id == RESULTS_ID {
            dialog.place_button_row(
                plan.button_row.client_width,
                plan.button_row.right_margin,
                plan.button_row.top,
                plan.button_row.height,
                &plan.button_row.ids,
            );
        }
        dialog.place(p.id, p.x, p.y, p.width, p.height);
    }
    if dialog.item(RESULTS_ID) != 0 {
        for (i, width) in plan.column_widths.iter().enumerate() {
            ListViewControl::set_column_width(dialog.item(RESULTS_ID), i as i32, *width);
        }
    }
    state.status_height.set(plan.status_height);
}
