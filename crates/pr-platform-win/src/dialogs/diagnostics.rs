//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/DiagnosticsDialog.cs` (205 lines, baseline `d92d6aae`). Unit W8-18p (second half). Contract: `contracts/W8-18p.md`.
//!
//! The Win32 view of `DiagnosticsPresenter`: a read-only table of 15 label / value pairs (static texts that wrap instead of clipping) and a Copy button that puts the
//! whole table on the clipboard as `label<TAB>value<CR LF>` lines. Modal; the only way out is Close (`IDCANCEL`: Esc, Enter, the X button and Alt+F4 all close it).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use pr_app::presenters::secondary::DiagnosticsDisplay;

use super::native::chrome::NativeDialogChrome;
use super::native::dialog::{DialogContent, ID_CANCEL, NativeDialog};
use super::native::template::DialogItem;
use super::zoom_prompt::{ButtonRowPlan, DialogMeasure, Placement};

/// C# `HeaderId`.
pub const HEADER_ID: i32 = 200;
/// C# `NoteId`: the privacy note under the header (colour `Dark.TextSubtle`).
pub const NOTE_ID: i32 = 201;
/// C# `FirstLabelId`: label of row `r` is `FIRST_LABEL_ID + r`.
pub const FIRST_LABEL_ID: i32 = 210;
/// C# `FirstValueId`: value of row `r` is `FIRST_VALUE_ID + r`.
pub const FIRST_VALUE_ID: i32 = 240;
/// C# `CopyId`.
pub const COPY_ID: i32 = 270;
/// C# `CloseId` = `IdCancel` (the default button).
pub const CLOSE_ID: i32 = ID_CANCEL;
/// C# `RowCount`.
pub const ROW_COUNT: usize = 15;

/// C# `LabelText(row)`: the label of row `row`, in the order of the WPF label column: 0 `diagnostics.label.cacheHits`, 1 `cacheMisses`, 2 `cacheHitRate`,
/// 3 `sourceBytesRead`, 4 `sourceFileReads`, 5 `decodeTime`, 6 `keyToPresentLatency`, 7 `explorerOrder`, 8 `explorerSortGroup`, 9 `sourceOpens`,
/// 10 `metadataQueries`, 11 `sessionWrites`, 12 `decoderFallbacks`, 13 `presentLatencyHistogram`, 14 `crossThreadPresents` (the `tr::diagnostics_label_*`
/// functions, ambient language). The C# `ArgumentOutOfRangeException` for `row >= 15` is `None`.
pub fn label_text(row: usize) -> Option<String> {
    todo!()
}

/// C# `ValueText(display, row)`: the field of `display` for row `row`, in the order of `DiagnosticsDisplay`'s declaration: `cache_hits`, `cache_misses`,
/// `hit_rate`, `source_bytes`, `source_reads`, `decode_time`, `present_latency`, `explorer_order`, `explorer_metadata`, `source_opens`, `stat_count`,
/// `session_writes`, `decoder_fallbacks`, `present_histogram`, `cross_thread_presents`. `None` for `row >= 15`.
pub fn value_text(display: &DiagnosticsDisplay, row: usize) -> Option<&str> {
    todo!()
}

/// C# `BuildCopyText(display)`: for `row` in `0..15`: `label_text(row)`, `'\t'`, `value_text(display, row)`, `"\r\n"` (C# `Environment.NewLine` on Windows).
/// Every line, including the last, ends with the line break.
pub fn build_copy_text(display: &DiagnosticsDisplay) -> String {
    todo!()
}

/// C# `BuildItems()`: Static `HEADER_ID` (`tr::diagnostics_header()`, `DIALOG_STYLE_CHILD | SS_NOPREFIX`), Static `NOTE_ID` (`tr::diagnostics_privacy_note()`, same
/// style), then for each row Static `FIRST_LABEL_ID + r` (`label_text(r)`) and Static `FIRST_VALUE_ID + r` (`value_text(display, r)`), same style; then Button
/// `COPY_ID` (`tr::diagnostics_copy()`, `DIALOG_STYLE_CHILD | WS_TABSTOP | BS_PUSHBUTTON`) and Button `CLOSE_ID` (`tr::common_close()`, `DIALOG_STYLE_CHILD |
/// WS_TABSTOP | BS_DEFPUSHBUTTON`). 34 items; the order is the Tab order.
pub fn build_items(display: &DiagnosticsDisplay) -> Vec<DialogItem> {
    todo!()
}

/// What `layout_plan` decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsLayoutPlan {
    /// Header, note, then label and value of each of the 15 rows, in the C# order.
    pub placements: Vec<Placement>,
    /// `[COPY_ID, CLOSE_ID]` at the bottom.
    pub button_row: ButtonRowPlan,
}

/// C# `ButtonHeight()`: `max(dy(14), line_height + dy(6))`.
pub fn button_height(m: &dyn DialogMeasure) -> i32 {
    todo!()
}

/// C# `RowHeights(clientWidth)` + `PreferredClientSize()`: `margin_x = dx(14)`, `margin_y = dy(12)`, `label_width = dx(125)`, `column_gap = dx(6)`; `width =
/// dx(400)`; `value_width = width - 2 * margin_x - label_width - column_gap`; `row[r] = max(measure_item_height(FIRST_LABEL_ID + r, label_width),
/// measure_item_height(FIRST_VALUE_ID + r, value_width))`; `height = margin_y + measure_item_height(HEADER_ID, width - 2 * margin_x) + dy(3) +
/// measure_item_height(NOTE_ID, width - 2 * margin_x) + dy(10) + sum(row[r] + dy(3)) + dy(8) + button_height + margin_y`. Wrapping `i32` arithmetic.
pub fn preferred_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    todo!()
}

/// C# `LayoutControls(clientWidth, clientHeight)` (`client_height` unused): `content = client_width - 2 * margin_x`; `y = margin_y`; header `Place(HEADER_ID,
/// margin_x, y, content, h)`, `y += h + dy(3)`; note the same with `y += h + dy(10)`; `value_x = margin_x + label_width + column_gap`; `value_width = client_width -
/// margin_x - value_x`; per row `height = max(measure label at label_width, measure value at value_width)`, `Place(label, margin_x, y, label_width, height)`,
/// `Place(value, value_x, y, value_width, height)`, `y += height + dy(3)`; `y += dy(8)`; the row `[COPY_ID, CLOSE_ID]` at `top = y`, `right_margin = margin_x`,
/// `height = button_height`.
pub fn layout_plan(m: &dyn DialogMeasure, client_width: i32) -> DiagnosticsLayoutPlan {
    todo!()
}

/// C# `Func<string, bool>? copy`: puts text on the clipboard; `true` = success.
pub type CopyFn = Box<dyn Fn(&str) -> bool>;

/// The `DialogContent` of the diagnostics dialog.
pub struct DiagnosticsContent {
    display: DiagnosticsDisplay,
    copy: Option<CopyFn>,
    items: Vec<DialogItem>,
    copy_count: Cell<usize>,
}

impl fmt::Debug for DiagnosticsContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiagnosticsContent")
            .field("copy_count", &self.copy_count.get())
            .finish_non_exhaustive()
    }
}

impl DiagnosticsContent {
    /// C# constructor body: `items = build_items(&display)` evaluated now.
    pub fn new(display: DiagnosticsDisplay, copy: Option<CopyFn>) -> Self {
        todo!()
    }

    /// C# `CopyCount`: how many times the Copy button put text on the clipboard successfully.
    pub fn copy_count(&self) -> usize {
        self.copy_count.get()
    }
}

impl DialogContent for DiagnosticsContent {
    /// `tr::diagnostics_title()`.
    fn title(&self) -> String {
        todo!()
    }

    /// The list built in `new` (cloned).
    fn items(&self) -> Vec<DialogItem> {
        todo!()
    }

    /// C# `HeaderControlIds` = `[HEADER_ID]`.
    fn header_control_ids(&self) -> Vec<i32> {
        todo!()
    }

    /// C# `InitialFocusId` = `CLOSE_ID`.
    fn initial_focus_id(&self) -> i32 {
        todo!()
    }

    /// C# `DefaultButtonId` = `CLOSE_ID`.
    fn default_button_id(&self) -> i32 {
        todo!()
    }

    /// `preferred_client_size(dialog)`.
    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        todo!()
    }

    /// Applies `layout_plan(dialog, client_width)`.
    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, client_height: i32) {
        todo!()
    }

    /// C# `OnCommand(id, notification)`: `id == COPY_ID && notification == BN_CLICKED` -> `copy` (when given) is called with `build_copy_text(&display)`; `true`
    /// adds one to `copy_count`; the command is handled (`true`) whether or not a `copy` function exists. Everything else `false`.
    fn on_command(&self, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        todo!()
    }

    /// C# `StaticTextColor(id)`: `NOTE_ID` -> `DialogColors::from_key("Dark.TextSubtle")` (a missing key falls back to the base answer), else `DialogColors::text()`.
    fn static_text_color(&self, dialog: &NativeDialog, id: i32) -> u32 {
        todo!()
    }
}

/// The diagnostics dialog (C# `DiagnosticsDialog`). Show it with `dialog.native().show_modal(owner, ready, loop)`; the result is always `ID_CANCEL`.
pub struct DiagnosticsDialog {
    native: NativeDialog,
    content: Rc<DiagnosticsContent>,
}

impl fmt::Debug for DiagnosticsDialog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiagnosticsDialog")
            .field("native", &self.native)
            .finish_non_exhaustive()
    }
}

impl DiagnosticsDialog {
    /// C# `DiagnosticsDialog(display, copy = null, chrome = null)`. The C# `ArgumentNullException` for a null `display` cannot happen.
    pub fn new(display: DiagnosticsDisplay, copy: Option<CopyFn>, chrome: Option<Rc<dyn NativeDialogChrome>>) -> Self {
        todo!()
    }

    /// The base-class handle.
    pub fn native(&self) -> &NativeDialog {
        &self.native
    }

    /// C# `CopyCount`.
    pub fn copy_count(&self) -> usize {
        self.content.copy_count()
    }
}
