//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/ListDialog.cs` at the BASELINE `d92d6aae` (183 lines; the checkout of the C# repo is newer and also holds
//! `ForChoice`, `InitialSelectedIndex`, `SelectedIndex` and `OnOk`, which belong to the external-editors feature and are NOT ported here, D-W8B5-208).
//! Unit W8-18e. Contract: `contracts/W8-18e.md`.
//!
//! The Win32 view of `SkippedFilesPresenter` and `BatchReviewPresenter`: a read-only list box plus one or two buttons. Resizable, vertical and horizontal
//! scroll (long paths). Returns `IDOK` only for the confirm button (BatchReview).
//!
//! C# `ListDialog : NativeDialog` overrides virtual members of the base class. The Rust base (W8-18h..18k, `dialog::core`) does not exist yet, so this file holds
//! the base-independent parts (content, item list, constants, sizes and the layout arithmetic) behind the narrow trait `ListDialogBase`, which lists exactly the
//! base members the C# class calls. W8-18h/18k implement `ListDialogBase` for the real dialog and add `impl <their content trait> for ListDialog` (D-W8B5-208).
use pr_app::presenters::secondary::NumberFormatting;
use pr_app::presenters::secondary::{BatchReviewPresenter, SkippedFilesPresenter};
use pr_core::abstractions::{BatchReviewItem, SkippedEntry};
use pr_l10n::keys::{BATCH_REVIEW_CONFIRM, BATCH_REVIEW_TITLE, COMMON_CANCEL};
use pr_l10n::localizer::Localizer;

use super::dialog::{DialogContent, NativeDialog};
use super::template::DialogItem;
use crate::win32::structs::Hwnd;

/// The content of a list dialog (C# `ListDialogContent` record, baseline form without `InitialSelectedIndex`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListDialogContent {
    /// The window title.
    pub title: String,
    /// The large heading above the intro; `None` = no heading control.
    pub heading: Option<String>,
    /// The multi-line paragraph below the heading; `None` = no intro control.
    pub intro: Option<String>,
    /// The `Dark.*` palette key of the intro text colour.
    pub intro_color_key: &'static str,
    /// One list box line each, in order.
    pub lines: Vec<String>,
    /// The text of the confirm button (`IDOK`); `None` = only the close button.
    pub confirm_text: Option<String>,
    /// The text of the Cancel / Close button (`IDCANCEL`).
    pub cancel_text: String,
    /// The initial client width in dialog units.
    pub initial_width_dlu: i32,
    /// The initial client height in dialog units.
    pub initial_height_dlu: i32,
}

impl ListDialogContent {
    /// C# `ForSkippedFiles(entries)`: title `SkippedFilesPresenter::title`, no heading, intro `SkippedFilesPresenter::intro` in `Dark.Warning`, lines
    /// `SkippedFilesPresenter::build_lines`, no confirm, cancel text `SkippedFilesPresenter::close_text`, 470 x 205 dialog units (WPF `SkippedFilesWindow`,
    /// 720 x 420, only a Close button).
    pub fn for_skipped_files(entries: &[SkippedEntry], localizer: &Localizer) -> Self {
        Self {
            title: SkippedFilesPresenter::title(localizer),
            heading: None,
            intro: Some(SkippedFilesPresenter::intro(localizer)),
            intro_color_key: "Dark.Warning",
            lines: SkippedFilesPresenter::build_lines(entries, localizer),
            confirm_text: None,
            cancel_text: SkippedFilesPresenter::close_text(localizer),
            initial_width_dlu: 470,
            initial_height_dlu: 205,
        }
    }

    /// C# `ForBatchReview(items)`: title and heading `batchReview.title`, intro `BatchReviewPresenter::summary(items.len())` in `Dark.TextSecondary`, lines
    /// `BatchReviewPresenter::build_lines`, confirm `batchReview.confirm`, cancel `common.cancel`, 470 x 280 dialog units (WPF `BatchReviewWindow`, 720 x 560).
    pub fn for_batch_review(items: &[BatchReviewItem], localizer: &Localizer, numbers: &dyn NumberFormatting) -> Self {
        let title = localizer.get(BATCH_REVIEW_TITLE);
        Self {
            title: title.clone(),
            heading: Some(title),
            intro: Some(BatchReviewPresenter::summary(items.len() as i64, localizer)),
            intro_color_key: "Dark.TextSecondary",
            lines: BatchReviewPresenter::build_lines(items, localizer, numbers),
            confirm_text: Some(localizer.get(BATCH_REVIEW_CONFIRM)),
            cancel_text: localizer.get(COMMON_CANCEL),
            initial_width_dlu: 470,
            initial_height_dlu: 280,
        }
    }
}

/// C# `HeadingId`.
pub const HEADING_ID: i32 = 300;
/// C# `IntroId`.
pub const INTRO_ID: i32 = 301;
/// C# `ListId`.
pub const LIST_ID: i32 = 302;
/// C# `ConfirmId` = `NativeDialog.IdOk`.
pub const CONFIRM_ID: i32 = 1;
/// C# `CancelId` = `NativeDialog.IdCancel`.
pub const CANCEL_ID: i32 = 2;

/// The members of the C# base class `NativeDialog` that `ListDialog` calls (names and meaning of the C# protected helpers). Implemented by the real dialog
/// (W8-18h..18k) and by a recording fake in the tests.
pub trait ListDialogBase {
    /// C# `Item(id)`: the control window, `0` when absent.
    fn item(&self, id: i32) -> Hwnd;
    /// C# `Dx(dialogUnits)`: `(units * avg_char_width + 2) / 4`.
    fn dx(&self, dialog_units: i32) -> i32;
    /// C# `Dy(dialogUnits)`: `(units * line_height + 4) / 8`.
    fn dy(&self, dialog_units: i32) -> i32;
    /// C# `LineHeight`.
    fn line_height(&self) -> i32;
    /// C# `MeasureTextWidth(text)` (the body font).
    fn measure_text_width(&self, text: &str) -> i32;
    /// C# `MeasureItemHeight(id, widthPx)`: the height a wrapped static needs at that width.
    fn measure_item_height(&self, id: i32, width_px: i32) -> i32;
    /// C# `Place(id, x, y, width, height)`.
    fn place(&self, id: i32, x: i32, y: i32, width: i32, height: i32);
    /// C# `PlaceButtonRow(clientWidth, rightMargin, top, height, ids...)`: right-aligned row, the first id leftmost.
    fn place_button_row(&self, client_width: i32, right_margin: i32, top: i32, height: i32, ids: &[i32]);
    /// C# `ButtonRowWidth(ids...)`.
    fn button_row_width(&self, ids: &[i32]) -> i32;
}

/// C# `ListDialog` (without the base class).
#[derive(Debug, Clone)]
pub struct ListDialog {
    content: ListDialogContent,
    items: Vec<DialogItem>,
}

impl ListDialog {
    /// C# constructor (the `chrome` argument belongs to the base class `NativeDialog`, W8-18h): builds `items` with `build_items`.
    pub fn new(content: ListDialogContent) -> Self {
        let items = Self::build_items(&content);
        Self { content, items }
    }

    /// C# `BuildItems()` (pure, the order is the tab order): Static `HEADING_ID` (text = heading, style `DIALOG_STYLE_CHILD | SS_NOPREFIX`) when there is a
    /// heading; Static `INTRO_ID` (same style) when there is an intro; ListBox `LIST_ID` (text `""`, style `DIALOG_STYLE_CHILD | WS_TABSTOP | WS_VSCROLL |
    /// WS_HSCROLL | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT`, extended `WS_EX_CLIENTEDGE`); Button `CANCEL_ID` (cancel text, `DIALOG_STYLE_CHILD | WS_TABSTOP |
    /// (BS_DEFPUSHBUTTON when there is no confirm button, else BS_PUSHBUTTON)`); Button `CONFIRM_ID` (confirm text, `DIALOG_STYLE_CHILD | WS_TABSTOP |
    /// BS_DEFPUSHBUTTON`) when there is one.
    pub fn build_items(content: &ListDialogContent) -> Vec<DialogItem> {
        use super::template::{DIALOG_STYLE_CHILD, DialogControlClass};
        let mut items = Vec::new();
        const WS_TABSTOP: u32 = 0x0001_0000;
        const SS_NOPREFIX: u32 = 0x0000_0080;
        const WS_VSCROLL: u32 = 0x0020_0000;
        const WS_HSCROLL: u32 = 0x0010_0000;
        const LBS_NOTIFY: u32 = 0x0001;
        const LBS_NOINTEGRALHEIGHT: u32 = 0x0100;
        const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;
        const BS_DEFPUSHBUTTON: u32 = 0x0001;
        if let Some(heading) = &content.heading {
            items.push(DialogItem::new(
                DialogControlClass::Static,
                HEADING_ID,
                heading,
                DIALOG_STYLE_CHILD | SS_NOPREFIX,
            ));
        }
        if let Some(intro) = &content.intro {
            items.push(DialogItem::new(
                DialogControlClass::Static,
                INTRO_ID,
                intro,
                DIALOG_STYLE_CHILD | SS_NOPREFIX,
            ));
        }
        items.push(
            DialogItem::new(
                DialogControlClass::ListBox,
                LIST_ID,
                "",
                DIALOG_STYLE_CHILD | WS_TABSTOP | WS_VSCROLL | WS_HSCROLL | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT,
            )
            .with_ex_style(WS_EX_CLIENTEDGE),
        );
        let cancel_style = DIALOG_STYLE_CHILD
            | WS_TABSTOP
            | if content.confirm_text.is_none() {
                BS_DEFPUSHBUTTON
            } else {
                0
            };
        items.push(DialogItem::new(
            DialogControlClass::Button,
            CANCEL_ID,
            &content.cancel_text,
            cancel_style,
        ));
        if let Some(confirm) = &content.confirm_text {
            items.push(DialogItem::new(
                DialogControlClass::Button,
                CONFIRM_ID,
                confirm,
                DIALOG_STYLE_CHILD | WS_TABSTOP | BS_DEFPUSHBUTTON,
            ));
        }
        items
    }

    /// C# `Title`.
    pub fn title(&self) -> &str {
        &self.content.title
    }

    /// C# `Items`.
    pub fn items(&self) -> &[DialogItem] {
        &self.items
    }

    /// C# `Resizable` = `true`.
    pub fn resizable(&self) -> bool {
        true
    }

    /// C# `HeaderControlIds`: `[HEADING_ID]` with a heading, else empty (the header font is larger).
    pub fn header_control_ids(&self) -> Vec<i32> {
        if self.content.heading.is_some() {
            vec![HEADING_ID]
        } else {
            Vec::new()
        }
    }

    /// C# `DefaultButtonId`: `CANCEL_ID` without a confirm button, else `CONFIRM_ID`.
    pub fn default_button_id(&self) -> i32 {
        if self.content.confirm_text.is_some() {
            CONFIRM_ID
        } else {
            CANCEL_ID
        }
    }

    /// C# `InitialFocusId` = `LIST_ID`.
    pub fn initial_focus_id(&self) -> i32 {
        LIST_ID
    }

    /// C# `StaticTextColor(id)`: for `INTRO_ID` the colour of `intro_color_key` (`DialogColors::from_key`, unknown key -> `base()`), else `base()` (the C#
    /// `base.StaticTextColor(id)`, evaluated lazily).
    pub fn static_text_color(&self, id: i32, base: &dyn Fn() -> u32) -> u32 {
        if id == INTRO_ID {
            super::chrome::DialogColors::from_key(self.content.intro_color_key).unwrap_or_else(base)
        } else {
            base()
        }
    }

    /// C# `OnInitialized()`: `list = item(LIST_ID)` (0 returns); `LB_INITSTORAGE` (count, count * 256) once; `LB_ADDSTRING` for every line, tracking
    /// `widest = max(widest, measure_text_width(line))`; then `LB_SETHORIZONTALEXTENT(widest + dx(8))`.
    pub fn on_initialized(&self, base: &dyn ListDialogBase) {
        use crate::win32::dialog_native::user32::send_message;
        const LB_INITSTORAGE: u32 = 0x01A8;
        const LB_ADDSTRING: u32 = 0x0180;
        const LB_SETHORIZONTALEXTENT: u32 = 0x0194;
        let list = base.item(LIST_ID);
        if list == 0 {
            return;
        }
        let count = self.content.lines.len();
        // SAFETY: LB_INITSTORAGE contains only count/capacity scalar values.
        let _ = unsafe { send_message(list, LB_INITSTORAGE, count, count.saturating_mul(256) as isize) };
        let mut widest = 0i32;
        for line in &self.content.lines {
            widest = widest.max(base.measure_text_width(line));
            let mut utf16: Vec<u16> = line.encode_utf16().collect();
            utf16.push(0);
            // SAFETY: the NUL-terminated UTF-16 buffer is alive for this synchronous message.
            let _ = unsafe { send_message(list, LB_ADDSTRING, 0, utf16.as_ptr() as isize) };
        }
        // SAFETY: LB_SETHORIZONTALEXTENT carries a scalar pixel width.
        let _ = unsafe {
            send_message(
                list,
                LB_SETHORIZONTALEXTENT,
                widest.saturating_add(base.dx(8)) as usize,
                0,
            )
        };
    }

    /// C# `PreferredClientSize()`: `(dx(initial_width_dlu), dy(initial_height_dlu))`.
    pub fn preferred_client_size(&self, base: &dyn ListDialogBase) -> (i32, i32) {
        (
            base.dx(self.content.initial_width_dlu),
            base.dy(self.content.initial_height_dlu),
        )
    }

    /// C# `MinimumClientSize()`: `buttons = button_row_width([CANCEL_ID])` or `([CANCEL_ID, CONFIRM_ID])` with a confirm button; `(max(dx(300), buttons +
    /// 2 * margin_x), dy(150))`.
    pub fn minimum_client_size(&self, base: &dyn ListDialogBase) -> (i32, i32) {
        let margin = base.dx(10);
        let ids = if self.content.confirm_text.is_some() {
            [CANCEL_ID, CONFIRM_ID].as_slice()
        } else {
            [CANCEL_ID].as_slice()
        };
        (
            base.dx(300)
                .max(base.button_row_width(ids).saturating_add(margin.saturating_mul(2))),
            base.dy(150),
        )
    }

    /// C# `LayoutControls(clientWidth, clientHeight)`: `content = max(1, client_width - 2 * margin_x)`; `y = margin_y`; heading: `h = measure_item_height(
    /// HEADING_ID, content)`, `place(HEADING_ID, margin_x, y, content, h)`, `y += h + dy(3)`; intro the same with `y += h + dy(6)`; `button_top = client_height -
    /// margin_y - button_height`; the list: `place(LIST_ID, margin_x, y, content, max(dy(20), button_top - dy(6) - y))`; then `place_button_row(client_width,
    /// margin_x, button_top, button_height, [CANCEL_ID])` or `[CANCEL_ID, CONFIRM_ID]`. `margin_x = dx(10)`, `margin_y = dy(8)`, `button_height =
    /// max(dy(14), line_height + dy(6))`.
    pub fn layout_controls(&self, base: &dyn ListDialogBase, client_width: i32, client_height: i32) {
        let margin_x = base.dx(10);
        let margin_y = base.dy(8);
        let width = client_width.saturating_sub(margin_x.saturating_mul(2)).max(1);
        let mut y = margin_y;
        if self.content.heading.is_some() {
            let height = base.measure_item_height(HEADING_ID, width);
            base.place(HEADING_ID, margin_x, y, width, height);
            y = y.saturating_add(height).saturating_add(base.dy(3));
        }
        if self.content.intro.is_some() {
            let height = base.measure_item_height(INTRO_ID, width);
            base.place(INTRO_ID, margin_x, y, width, height);
            y = y.saturating_add(height).saturating_add(base.dy(6));
        }
        let button_height = base.dy(14).max(base.line_height().saturating_add(base.dy(6)));
        let button_top = client_height.saturating_sub(margin_y).saturating_sub(button_height);
        let list_height = base.dy(20).max(button_top.saturating_sub(base.dy(6)).saturating_sub(y));
        base.place(LIST_ID, margin_x, y, width, list_height);
        if self.content.confirm_text.is_some() {
            base.place_button_row(
                client_width,
                margin_x,
                button_top,
                button_height,
                &[CANCEL_ID, CONFIRM_ID],
            );
        } else {
            base.place_button_row(client_width, margin_x, button_top, button_height, &[CANCEL_ID]);
        }
    }
}

/// The plug into the Rust base class (D-W8B5-208, D-W8B5-316): `NativeDialog::new(Rc::new(list_dialog))`. Every method forwards to the inherent method of the
/// same name; `dialog` is the `NativeDialog`, which is a [`ListDialogBase`] (W8-18i, `dialog/layout.rs`). `static_text_color`'s base answer is
/// `DialogColors::text()` (C# `NativeDialog.StaticTextColor` default).
impl DialogContent for ListDialog {
    fn title(&self) -> String {
        self.title().to_owned()
    }

    fn items(&self) -> Vec<DialogItem> {
        self.items().to_vec()
    }

    fn resizable(&self) -> bool {
        self.resizable()
    }

    fn header_control_ids(&self) -> Vec<i32> {
        self.header_control_ids()
    }

    fn initial_focus_id(&self) -> i32 {
        self.initial_focus_id()
    }

    fn default_button_id(&self) -> i32 {
        self.default_button_id()
    }

    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        ListDialog::preferred_client_size(self, dialog)
    }

    fn minimum_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        ListDialog::minimum_client_size(self, dialog)
    }

    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, client_height: i32) {
        ListDialog::layout_controls(self, dialog, client_width, client_height)
    }

    fn on_initialized(&self, dialog: &NativeDialog) {
        ListDialog::on_initialized(self, dialog)
    }

    fn static_text_color(&self, _dialog: &NativeDialog, id: i32) -> u32 {
        ListDialog::static_text_color(self, id, &super::chrome::DialogColors::text)
    }
}
