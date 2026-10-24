//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/DiagnosticsDialog.cs` (205 lines, baseline `d92d6aae`). Unit W8-18p (second half). Contract: `contracts/W8-18p.md`.
//!
//! The Win32 view of `DiagnosticsPresenter`: a read-only table of 15 label / value pairs (static texts that wrap instead of clipping) and a Copy button that puts the
//! whole table on the clipboard as `label<TAB>value<CR LF>` lines. Modal; the only way out is Close (`IDCANCEL`: Esc, Enter, the X button and Alt+F4 all close it).
use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use pr_app::presenters::secondary::DiagnosticsDisplay;

use super::native::chrome::NativeDialogChrome;
use super::native::dialog::{DialogContent, ID_CANCEL, NativeDialog};
use super::native::template::DialogItem;
use super::zoom_prompt::{ButtonRowPlan, DIALOG_STYLE_CHILD, DialogMeasure, Placement};

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
    Some(match row {
        0 => pr_l10n::tr::diagnostics_label_cache_hits(),
        1 => pr_l10n::tr::diagnostics_label_cache_misses(),
        2 => pr_l10n::tr::diagnostics_label_cache_hit_rate(),
        3 => pr_l10n::tr::diagnostics_label_source_bytes_read(),
        4 => pr_l10n::tr::diagnostics_label_source_file_reads(),
        5 => pr_l10n::tr::diagnostics_label_decode_time(),
        6 => pr_l10n::tr::diagnostics_label_key_to_present_latency(),
        7 => pr_l10n::tr::diagnostics_label_explorer_order(),
        8 => pr_l10n::tr::diagnostics_label_explorer_sort_group(),
        9 => pr_l10n::tr::diagnostics_label_source_opens(),
        10 => pr_l10n::tr::diagnostics_label_metadata_queries(),
        11 => pr_l10n::tr::diagnostics_label_session_writes(),
        12 => pr_l10n::tr::diagnostics_label_decoder_fallbacks(),
        13 => pr_l10n::tr::diagnostics_label_present_latency_histogram(),
        14 => pr_l10n::tr::diagnostics_label_cross_thread_presents(),
        _ => return None,
    })
}

/// C# `ValueText(display, row)`: the field of `display` for row `row`, in the order of `DiagnosticsDisplay`'s declaration: `cache_hits`, `cache_misses`,
/// `hit_rate`, `source_bytes`, `source_reads`, `decode_time`, `present_latency`, `explorer_order`, `explorer_metadata`, `source_opens`, `stat_count`,
/// `session_writes`, `decoder_fallbacks`, `present_histogram`, `cross_thread_presents`. `None` for `row >= 15`.
pub fn value_text(display: &DiagnosticsDisplay, row: usize) -> Option<&str> {
    Some(match row {
        0 => &display.cache_hits,
        1 => &display.cache_misses,
        2 => &display.hit_rate,
        3 => &display.source_bytes,
        4 => &display.source_reads,
        5 => &display.decode_time,
        6 => &display.present_latency,
        7 => &display.explorer_order,
        8 => &display.explorer_metadata,
        9 => &display.source_opens,
        10 => &display.stat_count,
        11 => &display.session_writes,
        12 => &display.decoder_fallbacks,
        13 => &display.present_histogram,
        14 => &display.cross_thread_presents,
        _ => return None,
    })
}

/// C# `BuildCopyText(display)`: for `row` in `0..15`: `label_text(row)`, `'\t'`, `value_text(display, row)`, `"\r\n"` (C# `Environment.NewLine` on Windows).
/// Every line, including the last, ends with the line break.
pub fn build_copy_text(display: &DiagnosticsDisplay) -> String {
    let mut out = String::new();
    for row in 0..ROW_COUNT {
        if let (Some(label), Some(value)) = (label_text(row), value_text(display, row)) {
            out.push_str(&label);
            out.push('\t');
            out.push_str(value);
            out.push_str("\r\n");
        }
    }
    out
}

/// C# `BuildItems()`: Static `HEADER_ID` (`tr::diagnostics_header()`, `DIALOG_STYLE_CHILD | SS_NOPREFIX`), Static `NOTE_ID` (`tr::diagnostics_privacy_note()`, same
/// style), then for each row Static `FIRST_LABEL_ID + r` (`label_text(r)`) and Static `FIRST_VALUE_ID + r` (`value_text(display, r)`), same style; then Button
/// `COPY_ID` (`tr::diagnostics_copy()`, `DIALOG_STYLE_CHILD | WS_TABSTOP | BS_PUSHBUTTON`) and Button `CLOSE_ID` (`tr::common_close()`, `DIALOG_STYLE_CHILD |
/// WS_TABSTOP | BS_DEFPUSHBUTTON`). 34 items; the order is the Tab order.
pub fn build_items(display: &DiagnosticsDisplay) -> Vec<DialogItem> {
    use super::native::template::{DialogControlClass as C, DialogItem as I};
    use crate::win32::dialog_native as n;
    let mut items = vec![
        I::new(
            C::Static,
            HEADER_ID,
            pr_l10n::tr::diagnostics_header(),
            DIALOG_STYLE_CHILD | n::SS_NOPREFIX,
        ),
        I::new(
            C::Static,
            NOTE_ID,
            pr_l10n::tr::diagnostics_privacy_note(),
            DIALOG_STYLE_CHILD | n::SS_NOPREFIX,
        ),
    ];
    for row in 0..ROW_COUNT {
        if let (Some(label), Some(value)) = (label_text(row), value_text(display, row)) {
            items.push(I::new(
                C::Static,
                FIRST_LABEL_ID + row as i32,
                label,
                DIALOG_STYLE_CHILD | n::SS_NOPREFIX,
            ));
            items.push(I::new(
                C::Static,
                FIRST_VALUE_ID + row as i32,
                value,
                DIALOG_STYLE_CHILD | n::SS_NOPREFIX,
            ));
        }
    }
    items.push(I::new(
        C::Button,
        COPY_ID,
        pr_l10n::tr::diagnostics_copy(),
        DIALOG_STYLE_CHILD | n::WS_TABSTOP | n::BS_PUSHBUTTON,
    ));
    items.push(I::new(
        C::Button,
        CLOSE_ID,
        pr_l10n::tr::common_close(),
        DIALOG_STYLE_CHILD | n::WS_TABSTOP | n::BS_DEFPUSHBUTTON,
    ));
    items
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
    m.dy(14).max(m.line_height().wrapping_add(m.dy(6)))
}

/// C# `RowHeights(clientWidth)` + `PreferredClientSize()`: `margin_x = dx(14)`, `margin_y = dy(12)`, `label_width = dx(125)`, `column_gap = dx(6)`; `width =
/// dx(400)`; `value_width = width - 2 * margin_x - label_width - column_gap`; `row[r] = max(measure_item_height(FIRST_LABEL_ID + r, label_width),
/// measure_item_height(FIRST_VALUE_ID + r, value_width))`; `height = margin_y + measure_item_height(HEADER_ID, width - 2 * margin_x) + dy(3) +
/// measure_item_height(NOTE_ID, width - 2 * margin_x) + dy(10) + sum(row[r] + dy(3)) + dy(8) + button_height + margin_y`. Wrapping `i32` arithmetic.
pub fn preferred_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    let mx = m.dx(14);
    let my = m.dy(12);
    let width = m.dx(400);
    let lw = m.dx(125);
    let gap = m.dx(6);
    let cw = width.wrapping_sub(mx.wrapping_mul(2));
    let vw = width
        .wrapping_sub(mx.wrapping_mul(2))
        .wrapping_sub(lw)
        .wrapping_sub(gap);
    let mut h = my
        .wrapping_add(m.measure_item_height(HEADER_ID, cw))
        .wrapping_add(m.dy(3))
        .wrapping_add(m.measure_item_height(NOTE_ID, cw))
        .wrapping_add(m.dy(10));
    for row in 0..ROW_COUNT {
        h = h
            .wrapping_add(
                m.measure_item_height(FIRST_LABEL_ID + row as i32, lw)
                    .max(m.measure_item_height(FIRST_VALUE_ID + row as i32, vw)),
            )
            .wrapping_add(m.dy(3));
    }
    h = h.wrapping_add(m.dy(8)).wrapping_add(button_height(m)).wrapping_add(my);
    (width, h)
}

/// C# `LayoutControls(clientWidth, clientHeight)` (`client_height` unused): `content = client_width - 2 * margin_x`; `y = margin_y`; header `Place(HEADER_ID,
/// margin_x, y, content, h)`, `y += h + dy(3)`; note the same with `y += h + dy(10)`; `value_x = margin_x + label_width + column_gap`; `value_width = client_width -
/// margin_x - value_x`; per row `height = max(measure label at label_width, measure value at value_width)`, `Place(label, margin_x, y, label_width, height)`,
/// `Place(value, value_x, y, value_width, height)`, `y += height + dy(3)`; `y += dy(8)`; the row `[COPY_ID, CLOSE_ID]` at `top = y`, `right_margin = margin_x`,
/// `height = button_height`.
pub fn layout_plan(m: &dyn DialogMeasure, client_width: i32) -> DiagnosticsLayoutPlan {
    let mx = m.dx(14);
    let my = m.dy(12);
    let content = client_width.wrapping_sub(mx.wrapping_mul(2));
    let lw = m.dx(125);
    let gap = m.dx(6);
    let vx = mx.wrapping_add(lw).wrapping_add(gap);
    let vw = client_width.wrapping_sub(mx).wrapping_sub(vx);
    let mut y = my;
    let mut p = Vec::with_capacity(32);
    let hh = m.measure_item_height(HEADER_ID, content);
    p.push(Placement::new(HEADER_ID, mx, y, content, hh));
    y = y.wrapping_add(hh).wrapping_add(m.dy(3));
    let nh = m.measure_item_height(NOTE_ID, content);
    p.push(Placement::new(NOTE_ID, mx, y, content, nh));
    y = y.wrapping_add(nh).wrapping_add(m.dy(10));
    for row in 0..ROW_COUNT {
        let h = m
            .measure_item_height(FIRST_LABEL_ID + row as i32, lw)
            .max(m.measure_item_height(FIRST_VALUE_ID + row as i32, vw));
        p.push(Placement::new(FIRST_LABEL_ID + row as i32, mx, y, lw, h));
        p.push(Placement::new(FIRST_VALUE_ID + row as i32, vx, y, vw, h));
        y = y.wrapping_add(h).wrapping_add(m.dy(3));
    }
    y = y.wrapping_add(m.dy(8));
    DiagnosticsLayoutPlan {
        placements: p,
        button_row: ButtonRowPlan {
            client_width,
            right_margin: mx,
            top: y,
            height: button_height(m),
            ids: vec![COPY_ID, CLOSE_ID],
        },
    }
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
        let items = build_items(&display);
        Self {
            display,
            copy,
            items,
            copy_count: Cell::new(0),
        }
    }

    /// C# `CopyCount`: how many times the Copy button put text on the clipboard successfully.
    pub fn copy_count(&self) -> usize {
        self.copy_count.get()
    }
}

impl DialogContent for DiagnosticsContent {
    /// `tr::diagnostics_title()`.
    fn title(&self) -> String {
        pr_l10n::tr::diagnostics_title()
    }

    /// The list built in `new` (cloned).
    fn items(&self) -> Vec<DialogItem> {
        self.items.clone()
    }

    /// C# `HeaderControlIds` = `[HEADER_ID]`.
    fn header_control_ids(&self) -> Vec<i32> {
        vec![HEADER_ID]
    }

    /// C# `InitialFocusId` = `CLOSE_ID`.
    fn initial_focus_id(&self) -> i32 {
        CLOSE_ID
    }

    /// C# `DefaultButtonId` = `CLOSE_ID`.
    fn default_button_id(&self) -> i32 {
        CLOSE_ID
    }

    /// `preferred_client_size(dialog)`.
    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        preferred_client_size(dialog)
    }

    /// Applies `layout_plan(dialog, client_width)`.
    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, _client_height: i32) {
        let p = layout_plan(dialog, client_width);
        for x in p.placements {
            dialog.place(x.id, x.x, x.y, x.width, x.height);
        }
        dialog.place_button_row(
            p.button_row.client_width,
            p.button_row.right_margin,
            p.button_row.top,
            p.button_row.height,
            &p.button_row.ids,
        );
    }

    /// C# `OnCommand(id, notification)`: `id == COPY_ID && notification == BN_CLICKED` -> `copy` (when given) is called with `build_copy_text(&display)`; `true`
    /// adds one to `copy_count`; the command is handled (`true`) whether or not a `copy` function exists. Everything else `false`.
    fn on_command(&self, _dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        if id == COPY_ID && notification == crate::win32::dialog_native::BN_CLICKED as i32 {
            if let Some(copy) = &self.copy {
                if copy(&build_copy_text(&self.display)) {
                    self.copy_count.set(self.copy_count.get().saturating_add(1));
                }
            }
            true
        } else {
            false
        }
    }

    /// C# `StaticTextColor(id)`: `NOTE_ID` -> `DialogColors::from_key("Dark.TextSubtle")` (a missing key falls back to the base answer), else `DialogColors::text()`.
    fn static_text_color(&self, _dialog: &NativeDialog, id: i32) -> u32 {
        if id == NOTE_ID {
            super::native::chrome::DialogColors::from_key("Dark.TextSubtle")
                .unwrap_or_else(super::native::chrome::DialogColors::text)
        } else {
            super::native::chrome::DialogColors::text()
        }
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
        let content = Rc::new(DiagnosticsContent::new(display, copy));
        let native = NativeDialog::with_chrome(content.clone(), chrome);
        Self { native, content }
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
