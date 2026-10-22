//! Port of `RecoveryDialog.cs`: the control ids and `BuildItems` (lines 41-68, 137-179), the sizes and `LayoutControls` (861-923, 863-876). Unit W8-18t.
//! Contract: `contracts/W8-18t.md`. The arithmetic is pure over the narrow trait [`RecoveryLayoutBase`] (D-W8B5-607, the precedent of `ListDialogBase`), so it is
//! tested with a recording fake and needs no window; `impl RecoveryLayoutBase for NativeDialog` (below) is the thin delegation to the base-class helpers.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use super::RecoveryTextSource;
use crate::dialogs::native::dialog::{ID_CANCEL, NativeDialog};
use crate::dialogs::native::template::DialogItem;

/// C# `HeaderId`.
pub const HEADER_ID: i32 = 100;
/// C# `SummaryId`.
pub const SUMMARY_ID: i32 = 101;
/// C# `FilterLabelId`.
pub const FILTER_LABEL_ID: i32 = 102;
/// C# `FilterId` (the combo box).
pub const FILTER_ID: i32 = 103;
/// C# `RecheckId`.
pub const RECHECK_ID: i32 = 104;
/// C# `CheckStatusId`.
pub const CHECK_STATUS_ID: i32 = 105;
/// C# `ListId` (owner-draw, multi-select list box).
pub const LIST_ID: i32 = 106;
/// C# `FilterEmptyId`.
pub const FILTER_EMPTY_ID: i32 = 107;
/// C# `DetailsEmptyId`.
pub const DETAILS_EMPTY_ID: i32 = 108;
/// C# `VerdictLabelId`.
pub const VERDICT_LABEL_ID: i32 = 110;
/// C# `VerdictBadgeId`.
pub const VERDICT_BADGE_ID: i32 = 111;
/// C# `ErrorId`.
pub const ERROR_ID: i32 = 112;
/// C# `SourceTitleId`.
pub const SOURCE_TITLE_ID: i32 = 113;
/// C# `SourceStatusId`.
pub const SOURCE_STATUS_ID: i32 = 114;
/// C# `SourcePathId`.
pub const SOURCE_PATH_ID: i32 = 115;
/// C# `CopySourceId`.
pub const COPY_SOURCE_ID: i32 = 116;
/// C# `ShowSourceId`.
pub const SHOW_SOURCE_ID: i32 = 117;
/// C# `DestinationTitleId`.
pub const DESTINATION_TITLE_ID: i32 = 118;
/// C# `DestinationStatusId`.
pub const DESTINATION_STATUS_ID: i32 = 119;
/// C# `DestinationPathId`.
pub const DESTINATION_PATH_ID: i32 = 120;
/// C# `CopyDestinationId`.
pub const COPY_DESTINATION_ID: i32 = 121;
/// C# `ShowDestinationId`.
pub const SHOW_DESTINATION_ID: i32 = 122;
/// C# `NoDestinationId`.
pub const NO_DESTINATION_ID: i32 = 123;
/// C# `DetailsId` (the scrollable read-only text).
pub const DETAILS_ID: i32 = 124;
/// C# `ClearSelectedId`.
pub const CLEAR_SELECTED_ID: i32 = 130;
/// C# `ClearAllId`.
pub const CLEAR_ALL_ID: i32 = 131;
/// C# `RetryId`.
pub const RETRY_ID: i32 = 132;
/// C# `CloseId = IdCancel`.
pub const CLOSE_ID: i32 = ID_CANCEL;

/// C# `SsCenter` (`SS_CENTER`).
pub const SS_CENTER: u32 = 0x1;

/// Every control id in template order (the order of the C# `BuildItems`; also the list of the C# layout test `Problems`).
pub const ALL_CONTROL_IDS: [i32; 28] = [
    HEADER_ID,
    SUMMARY_ID,
    FILTER_LABEL_ID,
    FILTER_ID,
    RECHECK_ID,
    CHECK_STATUS_ID,
    LIST_ID,
    FILTER_EMPTY_ID,
    DETAILS_EMPTY_ID,
    VERDICT_LABEL_ID,
    VERDICT_BADGE_ID,
    ERROR_ID,
    SOURCE_TITLE_ID,
    SOURCE_STATUS_ID,
    SOURCE_PATH_ID,
    COPY_SOURCE_ID,
    SHOW_SOURCE_ID,
    DESTINATION_TITLE_ID,
    DESTINATION_STATUS_ID,
    DESTINATION_PATH_ID,
    COPY_DESTINATION_ID,
    SHOW_DESTINATION_ID,
    NO_DESTINATION_ID,
    DETAILS_ID,
    CLEAR_SELECTED_ID,
    CLEAR_ALL_ID,
    RETRY_ID,
    CLOSE_ID,
];

/// C# `BuildItems()` (lines 137-179): the 28 children in Z / Tab order, texts from `texts` (the key of each C# `Tr.*` property is listed in the contract),
/// styles exactly as the C# (`child`, `tab`, `text`, `readOnlyLine`, the list box and combo box words, `WS_EX_CLIENTEDGE` where the C# has it).
pub fn build_items(texts: &RecoveryTextSource) -> Vec<DialogItem> {
    todo!()
}

/// Which parts of the details pane are shown (the C# fields `_hasDetails`, `_errorVisible`, `_sourceVisible`, `_destinationVisible`,
/// `_noDestinationVisible`); read by `layout_controls`, written by `path_panel::update_details`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DetailsLayout {
    /// C# `_hasDetails`: exactly one row is selected.
    pub has_details: bool,
    /// C# `_errorVisible`.
    pub error_visible: bool,
    /// C# `_sourceVisible`.
    pub source_visible: bool,
    /// C# `_destinationVisible`.
    pub destination_visible: bool,
    /// C# `_noDestinationVisible`.
    pub no_destination_visible: bool,
}

/// The members of the C# base class `NativeDialog` that the Recovery layout calls (C# names in the docs). Implemented below for [`NativeDialog`] and by a
/// recording fake in the tests; the same trait serves `path_panel::{layout_details, place_path_group}` (18u).
pub trait RecoveryLayoutBase {
    /// C# `Dx(units)`: `(units * avg_char_width + 2) / 4`.
    fn dx(&self, dialog_units: i32) -> i32;
    /// C# `Dy(units)`: `(units * line_height + 4) / 8`.
    fn dy(&self, dialog_units: i32) -> i32;
    /// C# `LineHeight`.
    fn line_height(&self) -> i32;
    /// C# `MeasureItemHeight(id, widthPx)`: height of a wrapped static at that width.
    fn measure_item_height(&self, id: i32, width_px: i32) -> i32;
    /// C# `MeasureTextWidth(text)` with the body font.
    fn measure_text_width(&self, text: &str) -> i32;
    /// C# `MeasureTextHeight(text, widthPx, BodyFont)`.
    fn measure_text_height(&self, text: &str, width_px: i32) -> i32;
    /// C# `GetItemText(id)`.
    fn get_item_text(&self, id: i32) -> String;
    /// C# `Place(id, x, y, width, height)`.
    fn place(&self, id: i32, x: i32, y: i32, width: i32, height: i32);
    /// C# `PlaceButtonRow(clientWidth, rightMargin, top, height, ids)`: right-aligned row, the first id leftmost; returns the row width.
    fn place_button_row(&self, client_width: i32, right_margin: i32, top: i32, height: i32, ids: &[i32]) -> i32;
    /// C# `ButtonRowWidth(ids)`.
    fn button_row_width(&self, ids: &[i32]) -> i32;
    /// C# `ButtonWidth(ids)`: the widest of the buttons.
    fn button_width(&self, ids: &[i32]) -> i32;
    /// C# `SetListItemHeight(id, heightPx)`.
    fn set_list_item_height(&self, id: i32, height_px: i32);
}

impl RecoveryLayoutBase for NativeDialog {
    fn dx(&self, dialog_units: i32) -> i32 {
        NativeDialog::dx(self, dialog_units)
    }
    fn dy(&self, dialog_units: i32) -> i32 {
        NativeDialog::dy(self, dialog_units)
    }
    fn line_height(&self) -> i32 {
        NativeDialog::line_height(self)
    }
    fn measure_item_height(&self, id: i32, width_px: i32) -> i32 {
        NativeDialog::measure_item_height(self, id, width_px)
    }
    fn measure_text_width(&self, text: &str) -> i32 {
        NativeDialog::measure_text_width(self, text, self.body_font())
    }
    fn measure_text_height(&self, text: &str, width_px: i32) -> i32 {
        NativeDialog::measure_text_height(self, text, width_px, self.body_font())
    }
    fn get_item_text(&self, id: i32) -> String {
        NativeDialog::get_item_text(self, id)
    }
    fn place(&self, id: i32, x: i32, y: i32, width: i32, height: i32) {
        NativeDialog::place(self, id, x, y, width, height)
    }
    fn place_button_row(&self, client_width: i32, right_margin: i32, top: i32, height: i32, ids: &[i32]) -> i32 {
        NativeDialog::place_button_row(self, client_width, right_margin, top, height, ids)
    }
    fn button_row_width(&self, ids: &[i32]) -> i32 {
        NativeDialog::button_row_width(self, ids)
    }
    fn button_width(&self, ids: &[i32]) -> i32 {
        NativeDialog::button_width(self, ids)
    }
    fn set_list_item_height(&self, id: i32, height_px: i32) {
        NativeDialog::set_list_item_height(self, id, height_px)
    }
}

/// C# `Margin`: `Dx(10)`.
pub fn margin(base: &dyn RecoveryLayoutBase) -> i32 {
    todo!()
}

/// C# `FieldHeight`: `max(Dy(14), LineHeight + Dy(6))`.
pub fn field_height(base: &dyn RecoveryLayoutBase) -> i32 {
    todo!()
}

/// C# `ButtonHeight`: `max(Dy(14), LineHeight + Dy(6))`.
pub fn button_height(base: &dyn RecoveryLayoutBase) -> i32 {
    todo!()
}

/// C# `PreferredClientSize()`: `(Dx(700), Dy(330))`.
pub fn preferred_client_size(base: &dyn RecoveryLayoutBase) -> (i32, i32) {
    todo!()
}

/// C# `MinimumClientSize()` (lines 871-876): `buttons = ButtonRowWidth(ClearSelected, ClearAll) + Dx(16) + ButtonRowWidth(Retry, Close) + 2 * Margin`;
/// `pane = Dx(150) + Dx(10) + 2 * ButtonWidth(CopySource, ShowSource) + Dx(80) + Dx(8) + 2 * Margin`; `(max(max(Dx(440), buttons), pane), Dy(220))`.
pub fn minimum_client_size(base: &dyn RecoveryLayoutBase) -> (i32, i32) {
    todo!()
}

/// C# `LayoutControls(clientWidth, clientHeight)` (lines 878-923) in its order: header, summary, the filter row (label, combo box, Re-check, check
/// status), the bottom button rows, the two columns (list 5/11 with `SetListItemHeight(ListId, 2 * LineHeight + Dy(5))`, the centred "filter empty" static),
/// then `path_panel::layout_details(base, details, rightX, y, rightWidth, contentBottom)`.
pub fn layout_controls(base: &dyn RecoveryLayoutBase, details: DetailsLayout, client_width: i32, client_height: i32) {
    todo!()
}
