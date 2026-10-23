//! Port of `RecoveryDialog.cs`: the control ids and `BuildItems` (lines 41-68, 137-179), the sizes and `LayoutControls` (861-923, 863-876). Unit W8-18t.
//! Contract: `contracts/W8-18t.md`. The arithmetic is pure over the narrow trait [`RecoveryLayoutBase`] (D-W8B5-607, the precedent of `ListDialogBase`), so it is
//! tested with a recording fake and needs no window; `impl RecoveryLayoutBase for NativeDialog` (below) is the thin delegation to the base-class helpers.

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
    use crate::dialogs::native::template::DialogControlClass::{Button, ComboBox, Edit, ListBox, Static};
    use crate::win32::dialog_native::{WS_TABSTOP, WS_VSCROLL};
    use crate::win32::messages::{WS_CHILD, WS_VISIBLE};
    let child = WS_CHILD | WS_VISIBLE;
    let tab = WS_TABSTOP;
    let text_style = child | 0x0080;
    let button = child | tab;
    let client_edge = 0x0000_0200;
    let mut items = vec![
        DialogItem::new(Static, HEADER_ID, texts.text("RECOVERY_HEADER"), text_style),
        DialogItem::new(Static, SUMMARY_ID, "", text_style),
        DialogItem::new(Static, FILTER_LABEL_ID, texts.text("RECOVERY_FILTER_LABEL"), text_style),
        DialogItem::new(
            ComboBox,
            FILTER_ID,
            "",
            child | crate::dialogs::native::controls::basic::ComboBoxControl::DEFAULT_STYLE,
        ),
        DialogItem::new(Button, RECHECK_ID, texts.text("RECOVERY_CHECK_RECHECK"), button),
        DialogItem::new(Static, CHECK_STATUS_ID, "", text_style),
        DialogItem::new(
            ListBox,
            LIST_ID,
            "",
            child | tab | WS_VSCROLL | 0x0001 | 0x0002 | 0x0008 | 0x0010 | 0x0040 | 0x0100,
        )
        .with_ex_style(client_edge),
        DialogItem::new(
            Static,
            FILTER_EMPTY_ID,
            texts.text("RECOVERY_FILTER_EMPTY"),
            child | 0x0080 | SS_CENTER,
        ),
        DialogItem::new(
            Static,
            DETAILS_EMPTY_ID,
            texts.text("RECOVERY_DETAIL_EMPTY"),
            child | 0x0080 | SS_CENTER,
        ),
        DialogItem::new(
            Static,
            VERDICT_LABEL_ID,
            texts.text("RECOVERY_DETAIL_VERDICT"),
            text_style,
        ),
        DialogItem::new(Static, VERDICT_BADGE_ID, "", text_style),
        DialogItem::new(Static, ERROR_ID, "", text_style),
        DialogItem::new(
            Static,
            SOURCE_TITLE_ID,
            texts.text("RECOVERY_DETAIL_SOURCE"),
            text_style,
        ),
        DialogItem::new(Static, SOURCE_STATUS_ID, "", text_style),
        DialogItem::new(Edit, SOURCE_PATH_ID, "", child | tab | 0x0004 | 0x0800).with_ex_style(client_edge),
        DialogItem::new(Button, COPY_SOURCE_ID, texts.text("RECOVERY_DETAIL_COPY_PATH"), button),
        DialogItem::new(
            Button,
            SHOW_SOURCE_ID,
            texts.text("RECOVERY_DETAIL_SHOW_IN_EXPLORER"),
            button,
        ),
        DialogItem::new(
            Static,
            DESTINATION_TITLE_ID,
            texts.text("RECOVERY_DETAIL_DESTINATION"),
            text_style,
        ),
        DialogItem::new(Static, DESTINATION_STATUS_ID, "", text_style),
        DialogItem::new(Edit, DESTINATION_PATH_ID, "", child | tab | 0x0004 | 0x0800).with_ex_style(client_edge),
        DialogItem::new(
            Button,
            COPY_DESTINATION_ID,
            texts.text("RECOVERY_DETAIL_COPY_PATH"),
            button,
        ),
        DialogItem::new(
            Button,
            SHOW_DESTINATION_ID,
            texts.text("RECOVERY_DETAIL_SHOW_IN_EXPLORER"),
            button,
        ),
        DialogItem::new(
            Static,
            NO_DESTINATION_ID,
            texts.text("RECOVERY_DETAIL_NO_DESTINATION"),
            text_style,
        ),
        DialogItem::new(
            Edit,
            DETAILS_ID,
            "",
            child | tab | WS_VSCROLL | 0x0004 | 0x0004 | 0x1000,
        )
        .with_ex_style(client_edge),
        DialogItem::new(Button, CLEAR_SELECTED_ID, texts.text("RECOVERY_CLEAR_SELECTED"), button),
        DialogItem::new(Button, CLEAR_ALL_ID, texts.text("RECOVERY_CLEAR_ALL"), button),
        DialogItem::new(Button, RETRY_ID, texts.text("RECOVERY_RETRY"), button),
        DialogItem::new(Button, CLOSE_ID, texts.text("COMMON_CLOSE"), child | tab | 1),
    ];
    // Preserve the C# template order exactly; the mut binding also keeps the construction easy to audit.
    items.shrink_to_fit();
    items
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
    base.dx(10)
}

/// C# `FieldHeight`: `max(Dy(14), LineHeight + Dy(6))`.
pub fn field_height(base: &dyn RecoveryLayoutBase) -> i32 {
    base.dy(14).max(base.line_height().wrapping_add(base.dy(6)))
}

/// C# `ButtonHeight`: `max(Dy(14), LineHeight + Dy(6))`.
pub fn button_height(base: &dyn RecoveryLayoutBase) -> i32 {
    field_height(base)
}

/// C# `PreferredClientSize()`: `(Dx(700), Dy(330))`.
pub fn preferred_client_size(base: &dyn RecoveryLayoutBase) -> (i32, i32) {
    (base.dx(700), base.dy(330))
}

/// C# `MinimumClientSize()` (lines 871-876): `buttons = ButtonRowWidth(ClearSelected, ClearAll) + Dx(16) + ButtonRowWidth(Retry, Close) + 2 * Margin`;
/// `pane = Dx(150) + Dx(10) + 2 * ButtonWidth(CopySource, ShowSource) + Dx(80) + Dx(8) + 2 * Margin`; `(max(max(Dx(440), buttons), pane), Dy(220))`.
pub fn minimum_client_size(base: &dyn RecoveryLayoutBase) -> (i32, i32) {
    let m = margin(base);
    let buttons = base
        .button_row_width(&[CLEAR_SELECTED_ID, CLEAR_ALL_ID])
        .wrapping_add(base.dx(16))
        .wrapping_add(base.button_row_width(&[RETRY_ID, CLOSE_ID]))
        .wrapping_add(m.wrapping_mul(2));
    let pane = base
        .dx(150)
        .wrapping_add(base.dx(10))
        .wrapping_add(base.button_width(&[COPY_SOURCE_ID, SHOW_SOURCE_ID]).wrapping_mul(2))
        .wrapping_add(base.dx(80))
        .wrapping_add(base.dx(8))
        .wrapping_add(m.wrapping_mul(2));
    (base.dx(440).max(buttons).max(pane), base.dy(220))
}

/// C# `LayoutControls(clientWidth, clientHeight)` (lines 878-923) in its order: header, summary, the filter row (label, combo box, Re-check, check
/// status), the bottom button rows, the two columns (list 5/11 with `SetListItemHeight(ListId, 2 * LineHeight + Dy(5))`, the centred "filter empty" static),
/// then `path_panel::layout_details(base, details, rightX, y, rightWidth, contentBottom)`.
pub fn layout_controls(base: &dyn RecoveryLayoutBase, details: DetailsLayout, client_width: i32, client_height: i32) {
    let m = margin(base);
    let gap = base.dx(10);
    let header_y = m;
    let header_h = base.measure_item_height(HEADER_ID, client_width.wrapping_sub(m.wrapping_mul(2)));
    base.place(
        HEADER_ID,
        m,
        header_y,
        client_width.wrapping_sub(m.wrapping_mul(2)),
        header_h,
    );
    let summary_y = header_y.wrapping_add(header_h).wrapping_add(base.dy(4));
    let summary_h = base.measure_item_height(SUMMARY_ID, client_width.wrapping_sub(m.wrapping_mul(2)));
    base.place(
        SUMMARY_ID,
        m,
        summary_y,
        client_width.wrapping_sub(m.wrapping_mul(2)),
        summary_h,
    );
    let row_y = summary_y.wrapping_add(summary_h).wrapping_add(base.dy(8));
    let label_w = base.measure_text_width(&base.get_item_text(FILTER_LABEL_ID));
    let field_h = field_height(base);
    base.place(
        FILTER_LABEL_ID,
        m,
        row_y.wrapping_add(field_h / 2).wrapping_sub(base.line_height() / 2),
        label_w,
        base.line_height(),
    );
    let combo_x = m.wrapping_add(label_w).wrapping_add(base.dx(8));
    let combo_w = base.dx(85);
    let combo_h = field_h.wrapping_add(base.line_height().wrapping_add(base.dy(2)).wrapping_mul(4));
    base.place(FILTER_ID, combo_x, row_y, combo_w, combo_h);
    let recheck_x = combo_x.wrapping_add(combo_w).wrapping_add(base.dx(8));
    let recheck_w = base.button_width(&[RECHECK_ID]);
    base.place(RECHECK_ID, recheck_x, row_y, recheck_w, field_h);
    let status_x = recheck_x.wrapping_add(recheck_w).wrapping_add(base.dx(8));
    base.place(
        CHECK_STATUS_ID,
        status_x,
        row_y.wrapping_add(field_h / 2).wrapping_sub(base.line_height() / 2),
        client_width.wrapping_sub(status_x).wrapping_sub(m),
        base.line_height(),
    );
    let bottom_y = client_height.wrapping_sub(m).wrapping_sub(field_h);
    base.place_button_row(client_width, m, bottom_y, field_h, &[RETRY_ID, CLOSE_ID]);
    base.place_button_row(
        client_width
            .wrapping_sub(base.button_row_width(&[RETRY_ID, CLOSE_ID]))
            .wrapping_sub(gap),
        m,
        bottom_y,
        field_h,
        &[CLEAR_SELECTED_ID, CLEAR_ALL_ID],
    );
    let content_top = row_y.wrapping_add(field_h).wrapping_add(base.dy(6));
    let content_bottom = bottom_y.wrapping_sub(base.dy(8));
    let content_h = content_bottom.wrapping_sub(content_top).max(base.dy(30));
    let inner = client_width.wrapping_sub(m.wrapping_mul(2));
    let left_w = base.dx(150).max(inner.wrapping_sub(gap).wrapping_mul(5) / 11);
    let right_x = m.wrapping_add(left_w).wrapping_add(gap);
    let right_w = base.dx(150).max(client_width.wrapping_sub(m).wrapping_sub(right_x));
    base.place(LIST_ID, m, content_top, left_w, content_h);
    base.set_list_item_height(LIST_ID, base.line_height().wrapping_mul(2).wrapping_add(base.dy(5)));
    base.place(
        FILTER_EMPTY_ID,
        m,
        content_top.wrapping_add(content_h / 2),
        left_w,
        base.line_height(),
    );
    base.place(
        DETAILS_EMPTY_ID,
        right_x,
        content_top.wrapping_add(content_h / 2),
        right_w,
        base.line_height(),
    );
    super::path_panel::layout_details(base, details, right_x, content_top, right_w, content_bottom);
}
