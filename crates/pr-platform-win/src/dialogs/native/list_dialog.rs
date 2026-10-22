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
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::presenters::secondary::NumberFormatting;
use pr_core::abstractions::{BatchReviewItem, SkippedEntry};
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
        todo!()
    }

    /// C# `ForBatchReview(items)`: title and heading `batchReview.title`, intro `BatchReviewPresenter::summary(items.len())` in `Dark.TextSecondary`, lines
    /// `BatchReviewPresenter::build_lines`, confirm `batchReview.confirm`, cancel `common.cancel`, 470 x 280 dialog units (WPF `BatchReviewWindow`, 720 x 560).
    pub fn for_batch_review(items: &[BatchReviewItem], localizer: &Localizer, numbers: &dyn NumberFormatting) -> Self {
        todo!()
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
        todo!()
    }

    /// C# `BuildItems()` (pure, the order is the tab order): Static `HEADING_ID` (text = heading, style `DIALOG_STYLE_CHILD | SS_NOPREFIX`) when there is a
    /// heading; Static `INTRO_ID` (same style) when there is an intro; ListBox `LIST_ID` (text `""`, style `DIALOG_STYLE_CHILD | WS_TABSTOP | WS_VSCROLL |
    /// WS_HSCROLL | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT`, extended `WS_EX_CLIENTEDGE`); Button `CANCEL_ID` (cancel text, `DIALOG_STYLE_CHILD | WS_TABSTOP |
    /// (BS_DEFPUSHBUTTON when there is no confirm button, else BS_PUSHBUTTON)`); Button `CONFIRM_ID` (confirm text, `DIALOG_STYLE_CHILD | WS_TABSTOP |
    /// BS_DEFPUSHBUTTON`) when there is one.
    pub fn build_items(content: &ListDialogContent) -> Vec<DialogItem> {
        todo!()
    }

    /// C# `Title`.
    pub fn title(&self) -> &str {
        todo!()
    }

    /// C# `Items`.
    pub fn items(&self) -> &[DialogItem] {
        todo!()
    }

    /// C# `Resizable` = `true`.
    pub fn resizable(&self) -> bool {
        todo!()
    }

    /// C# `HeaderControlIds`: `[HEADING_ID]` with a heading, else empty (the header font is larger).
    pub fn header_control_ids(&self) -> Vec<i32> {
        todo!()
    }

    /// C# `DefaultButtonId`: `CANCEL_ID` without a confirm button, else `CONFIRM_ID`.
    pub fn default_button_id(&self) -> i32 {
        todo!()
    }

    /// C# `InitialFocusId` = `LIST_ID`.
    pub fn initial_focus_id(&self) -> i32 {
        todo!()
    }

    /// C# `StaticTextColor(id)`: for `INTRO_ID` the colour of `intro_color_key` (`DialogColors::from_key`, unknown key -> `base()`), else `base()` (the C#
    /// `base.StaticTextColor(id)`, evaluated lazily).
    pub fn static_text_color(&self, id: i32, base: &dyn Fn() -> u32) -> u32 {
        todo!()
    }

    /// C# `OnInitialized()`: `list = item(LIST_ID)` (0 returns); `LB_INITSTORAGE` (count, count * 256) once; `LB_ADDSTRING` for every line, tracking
    /// `widest = max(widest, measure_text_width(line))`; then `LB_SETHORIZONTALEXTENT(widest + dx(8))`.
    pub fn on_initialized(&self, base: &dyn ListDialogBase) {
        todo!()
    }

    /// C# `PreferredClientSize()`: `(dx(initial_width_dlu), dy(initial_height_dlu))`.
    pub fn preferred_client_size(&self, base: &dyn ListDialogBase) -> (i32, i32) {
        todo!()
    }

    /// C# `MinimumClientSize()`: `buttons = button_row_width([CANCEL_ID])` or `([CANCEL_ID, CONFIRM_ID])` with a confirm button; `(max(dx(300), buttons +
    /// 2 * margin_x), dy(150))`.
    pub fn minimum_client_size(&self, base: &dyn ListDialogBase) -> (i32, i32) {
        todo!()
    }

    /// C# `LayoutControls(clientWidth, clientHeight)`: `content = max(1, client_width - 2 * margin_x)`; `y = margin_y`; heading: `h = measure_item_height(
    /// HEADING_ID, content)`, `place(HEADING_ID, margin_x, y, content, h)`, `y += h + dy(3)`; intro the same with `y += h + dy(6)`; `button_top = client_height -
    /// margin_y - button_height`; the list: `place(LIST_ID, margin_x, y, content, max(dy(20), button_top - dy(6) - y))`; then `place_button_row(client_width,
    /// margin_x, button_top, button_height, [CANCEL_ID])` or `[CANCEL_ID, CONFIRM_ID]`. `margin_x = dx(10)`, `margin_y = dy(8)`, `button_height =
    /// max(dy(14), line_height + dy(6))`.
    pub fn layout_controls(&self, base: &dyn ListDialogBase, client_width: i32, client_height: i32) {
        todo!()
    }
}

/// The plug into the Rust base class (D-W8B5-208, D-W8B5-316): `NativeDialog::new(Rc::new(list_dialog))`. Every method forwards to the inherent method of the
/// same name; `dialog` is the `NativeDialog`, which is a [`ListDialogBase`] (W8-18i, `dialog/layout.rs`). `static_text_color`'s base answer is
/// `DialogColors::text()` (C# `NativeDialog.StaticTextColor` default).
impl DialogContent for ListDialog {
    fn title(&self) -> String {
        todo!()
    }

    fn items(&self) -> Vec<DialogItem> {
        todo!()
    }

    fn resizable(&self) -> bool {
        todo!()
    }

    fn header_control_ids(&self) -> Vec<i32> {
        todo!()
    }

    fn initial_focus_id(&self) -> i32 {
        todo!()
    }

    fn default_button_id(&self) -> i32 {
        todo!()
    }

    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        todo!()
    }

    fn minimum_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        todo!()
    }

    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, client_height: i32) {
        todo!()
    }

    fn on_initialized(&self, dialog: &NativeDialog) {
        todo!()
    }

    fn static_text_color(&self, dialog: &NativeDialog, id: i32) -> u32 {
        todo!()
    }
}
