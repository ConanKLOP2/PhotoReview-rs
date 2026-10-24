//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/ZoomPromptDialog.cs` (134 lines, baseline `d92d6aae`; the file also holds the `DialogStyles` class, whose
//! `Child` constant is `template::DIALOG_STYLE_CHILD`, D-W8B5-203). Unit W8-18p (first half). Contract: `contracts/W8-18p.md`.
//!
//! The Win32 view of `ClickZoomCustomPresenter` ("Click zoom level > Custom..."): a label, one number box (10..800), an error line that appears under the box, Cancel
//! and Apply. Modal. An invalid value keeps the dialog open, shows the message under the box and grows the window.
//!
//! This file also holds the layout vocabulary the four secondary dialogs of W8-18p..18s share (D-W8B5-501): [`DialogMeasure`] (the C# `protected` helpers the layout
//! code calls, implemented for [`NativeDialog`] and by fakes in the tests), [`Placement`] and [`ButtonRowPlan`] (a recorded `Place` / `PlaceButtonRow` call). The
//! layout of every dialog is a PURE function from a `&dyn DialogMeasure` to a plan; the `DialogContent` hook only applies the plan, so the arithmetic is tested
//! without a window.
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use pr_app::presenters::secondary::ClickZoomCustomPresenter;

use super::native::chrome::NativeDialogChrome;
use super::native::dialog::{DialogContent, ID_CANCEL, ID_OK, NativeDialog};
use super::native::template::DialogItem;

/// C# `DialogStyles.Child` (`WS_CHILD | WS_VISIBLE`), declared in `ZoomPromptDialog.cs`; it lives in `native::template` (D-W8B5-203) and is re-exported here under
/// the name every dialog uses.
pub use super::native::template::DIALOG_STYLE_CHILD;

/// C# `LabelId`: the caption above the box.
pub const LABEL_ID: i32 = 100;
/// C# `EditId`: the number box.
pub const EDIT_ID: i32 = 101;
/// C# `ErrorId`: the error line (created without `WS_VISIBLE`, shown by `on_ok`).
pub const ERROR_ID: i32 = 102;
/// C# `CancelId` = `IdCancel`.
pub const CANCEL_ID: i32 = ID_CANCEL;
/// C# `ApplyId` = `IdOk` (the default button).
pub const APPLY_ID: i32 = ID_OK;

// ---- layout vocabulary shared by W8-18p..18s ------------------------------------------------------------------------------------------------

/// The C# `protected` helpers the layout code of the four secondary dialogs calls, plus `GetItemText`. Implemented below for [`NativeDialog`] (a one-line
/// delegation each) and by `FakeMeasure` in the tests. All numbers are pixels at the current DPI.
pub trait DialogMeasure {
    /// C# `Dx(dialogUnits)`.
    fn dx(&self, dialog_units: i32) -> i32;
    /// C# `Dy(dialogUnits)`.
    fn dy(&self, dialog_units: i32) -> i32;
    /// C# `LineHeight`.
    fn line_height(&self) -> i32;
    /// C# `MeasureItemHeight(id, widthPx)`: the height the text of control `id` needs word-wrapped in `width_px`; `0` for an absent control.
    fn measure_item_height(&self, id: i32, width_px: i32) -> i32;
    /// C# `MeasureTextWidth(text)` with the body font: one line.
    fn measure_text_width(&self, text: &str) -> i32;
    /// C# `MeasureTextHeight(text, widthPx, BodyFont)`.
    fn measure_text_height(&self, text: &str, width_px: i32) -> i32;
    /// C# `GetItemText(id)`: `""` for an absent control.
    fn item_text(&self, id: i32) -> String;
    /// C# `ButtonWidth(ids)`.
    fn button_width(&self, ids: &[i32]) -> i32;
    /// C# `ButtonRowWidth(ids)`.
    fn button_row_width(&self, ids: &[i32]) -> i32;
}

impl DialogMeasure for NativeDialog {
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
        NativeDialog::measure_text_width(self, text, 0)
    }

    fn measure_text_height(&self, text: &str, width_px: i32) -> i32 {
        NativeDialog::measure_text_height(self, text, width_px, self.body_font())
    }

    fn item_text(&self, id: i32) -> String {
        NativeDialog::get_item_text(self, id)
    }

    fn button_width(&self, ids: &[i32]) -> i32 {
        NativeDialog::button_width(self, ids)
    }

    fn button_row_width(&self, ids: &[i32]) -> i32 {
        NativeDialog::button_row_width(self, ids)
    }
}

/// One recorded C# `Place(id, x, y, width, height)` call. `NativeDialog::place` clamps a negative width or height to `0`; a plan keeps the computed numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// The control id.
    pub id: i32,
    /// Left, client pixels.
    pub x: i32,
    /// Top, client pixels.
    pub y: i32,
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
}

impl Placement {
    /// `Placement { id, x, y, width, height }`.
    pub fn new(id: i32, x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            id,
            x,
            y,
            width,
            height,
        }
    }
}

/// One recorded C# `PlaceButtonRow(clientWidth, rightMargin, top, height, ids)` call (a right-aligned row, `ids[0]` leftmost).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonRowPlan {
    /// The client width the row is aligned to.
    pub client_width: i32,
    /// The gap between the last button and the right edge.
    pub right_margin: i32,
    /// Top of the row.
    pub top: i32,
    /// Height of every button.
    pub height: i32,
    /// The button ids, leftmost first.
    pub ids: Vec<i32>,
}

/// Runs `dialog.place(..)` for every placement, in order.
pub fn apply_placements(dialog: &NativeDialog, placements: &[Placement]) {
    for p in placements {
        dialog.place(p.id, p.x, p.y, p.width, p.height);
    }
}

/// Runs `dialog.place_button_row(row.client_width, row.right_margin, row.top, row.height, &row.ids)`.
pub fn apply_button_row(dialog: &NativeDialog, row: &ButtonRowPlan) {
    dialog.place_button_row(row.client_width, row.right_margin, row.top, row.height, &row.ids);
}

// ---- items and layout of the zoom prompt ------------------------------------------------------------------------------------------------------

/// C# `Items` (the list initialiser), pure: Static `LABEL_ID` (`label_text`, `DIALOG_STYLE_CHILD | SS_NOPREFIX`); Edit `EDIT_ID` (`""`, `DIALOG_STYLE_CHILD |
/// WS_TABSTOP | ES_AUTOHSCROLL`, ex `WS_EX_CLIENTEDGE`); Static `ERROR_ID` (`""`, `WS_CHILD | SS_NOPREFIX` WITHOUT `WS_VISIBLE`); Button `CANCEL_ID`
/// (`cancel_text`, `DIALOG_STYLE_CHILD | WS_TABSTOP | BS_PUSHBUTTON`); Button `APPLY_ID` (`apply_text`, `DIALOG_STYLE_CHILD | WS_TABSTOP | BS_DEFPUSHBUTTON`).
pub fn build_items(label_text: &str, cancel_text: &str, apply_text: &str) -> Vec<DialogItem> {
    use super::native::template::{DialogControlClass as C, DialogItem as I};
    use crate::win32::dialog_native as n;
    vec![
        I::new(C::Static, LABEL_ID, label_text, DIALOG_STYLE_CHILD | n::SS_NOPREFIX),
        I::new(
            C::Edit,
            EDIT_ID,
            "",
            DIALOG_STYLE_CHILD | n::WS_TABSTOP | n::ES_AUTOHSCROLL,
        )
        .with_ex_style(n::WS_EX_CLIENTEDGE),
        I::new(
            C::Static,
            ERROR_ID,
            "",
            crate::win32::messages::WS_CHILD | n::SS_NOPREFIX,
        ),
        I::new(
            C::Button,
            CANCEL_ID,
            cancel_text,
            DIALOG_STYLE_CHILD | n::WS_TABSTOP | n::BS_PUSHBUTTON,
        ),
        I::new(
            C::Button,
            APPLY_ID,
            apply_text,
            DIALOG_STYLE_CHILD | n::WS_TABSTOP | n::BS_DEFPUSHBUTTON,
        ),
    ]
}

/// What `layout_plan` decided: every `Place` call in the C# order, then the button row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoomLayoutPlan {
    /// Label, box and (only when the error is visible) the error line, in that order.
    pub placements: Vec<Placement>,
    /// `[CANCEL_ID, APPLY_ID]` at the bottom.
    pub button_row: ButtonRowPlan,
}

/// C# `EditHeight()` and `ButtonHeight()` (equal): `max(dy(14), line_height + dy(6))`.
pub fn control_height(m: &dyn DialogMeasure) -> i32 {
    m.dy(14).max(m.line_height().wrapping_add(m.dy(6)))
}

/// C# `PreferredClientSize()`: `margin_x = dx(12)`, `margin_y = dy(10)`; `width = max(dx(200), button_row_width([CANCEL_ID, APPLY_ID]) + 2 * margin_x)`;
/// `content = width - 2 * margin_x`; `height = margin_y + measure_item_height(LABEL_ID, content) + dy(4) + control_height`; with `error_visible`
/// `height += dy(4) + measure_item_height(ERROR_ID, content)`; `height += dy(10) + control_height + margin_y`. Wrapping `i32` arithmetic (D-W8B5-312).
pub fn preferred_client_size(m: &dyn DialogMeasure, error_visible: bool) -> (i32, i32) {
    let mx = m.dx(12);
    let my = m.dy(10);
    let ch = control_height(m);
    let width = m.dx(200).max(
        m.button_row_width(&[CANCEL_ID, APPLY_ID])
            .wrapping_add(mx.wrapping_mul(2)),
    );
    let content = width.wrapping_sub(mx.wrapping_mul(2));
    let mut height = my
        .wrapping_add(m.measure_item_height(LABEL_ID, content))
        .wrapping_add(m.dy(4))
        .wrapping_add(ch);
    if error_visible {
        height = height
            .wrapping_add(m.dy(4))
            .wrapping_add(m.measure_item_height(ERROR_ID, content));
    }
    height = height.wrapping_add(m.dy(10)).wrapping_add(ch).wrapping_add(my);
    (width, height)
}

/// C# `LayoutControls(clientWidth, clientHeight)` (`client_height` is not used): `content = client_width - 2 * margin_x`; `y = margin_y`; label height =
/// `measure_item_height(LABEL_ID, content)`, `Place(LABEL_ID, margin_x, y, content, label)`, `y += label + dy(4)`; `Place(EDIT_ID, margin_x, y, content,
/// control_height)`, `y += control_height`; with `error_visible`: `y += dy(4)`, `error = measure_item_height(ERROR_ID, content)`, `Place(ERROR_ID, margin_x, y,
/// content, error)`, `y += error`; `y += dy(10)`; the row `[CANCEL_ID, APPLY_ID]` at `top = y` with `right_margin = margin_x`, `height = control_height`.
pub fn layout_plan(m: &dyn DialogMeasure, client_width: i32, error_visible: bool) -> ZoomLayoutPlan {
    let mx = m.dx(12);
    let mut y = m.dy(10);
    let content = client_width.wrapping_sub(mx.wrapping_mul(2));
    let ch = control_height(m);
    let lh = m.measure_item_height(LABEL_ID, content);
    let mut placements = vec![Placement::new(LABEL_ID, mx, y, content, lh)];
    y = y.wrapping_add(lh).wrapping_add(m.dy(4));
    placements.push(Placement::new(EDIT_ID, mx, y, content, ch));
    y = y.wrapping_add(ch);
    if error_visible {
        y = y.wrapping_add(m.dy(4));
        let eh = m.measure_item_height(ERROR_ID, content);
        placements.push(Placement::new(ERROR_ID, mx, y, content, eh));
        y = y.wrapping_add(eh);
    }
    y = y.wrapping_add(m.dy(10));
    ZoomLayoutPlan {
        placements,
        button_row: ButtonRowPlan {
            client_width,
            right_margin: mx,
            top: y,
            height: ch,
            ids: vec![CANCEL_ID, APPLY_ID],
        },
    }
}

// ---- the dialog -----------------------------------------------------------------------------------------------------------------------------

/// The `DialogContent` of the zoom prompt (C# `ZoomPromptDialog` without the base class). State in `Cell`/`RefCell`; no borrow is held across an OS call or a
/// hook (D-W8B4I-1). The presenter is borrowed only for `try_accept`.
pub struct ZoomPromptContent {
    presenter: RefCell<ClickZoomCustomPresenter>,
    items: Vec<DialogItem>,
    error_visible: Cell<bool>,
    error_message: RefCell<Option<String>>,
    value: Cell<Option<i32>>,
}

impl fmt::Debug for ZoomPromptContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZoomPromptContent")
            .field("error_visible", &self.error_visible.get())
            .field("value", &self.value.get())
            .finish_non_exhaustive()
    }
}

impl ZoomPromptContent {
    /// C# constructor body: `presenter = ClickZoomCustomPresenter::new(current_percent)`; `items = build_items(tr::dialog_click_zoom_custom_label(),
    /// tr::common_cancel(), tr::common_apply())` evaluated NOW (the C# list initialiser ran in the constructor: the texts keep the language of the construction);
    /// no error, no value.
    pub fn new(current_percent: i32) -> Self {
        let presenter = ClickZoomCustomPresenter::new(current_percent);
        let items = build_items(
            &pr_l10n::tr::dialog_click_zoom_custom_label(),
            &pr_l10n::tr::common_cancel(),
            &pr_l10n::tr::common_apply(),
        );
        Self {
            presenter: RefCell::new(presenter),
            items,
            error_visible: Cell::new(false),
            error_message: RefCell::new(None),
            value: Cell::new(None),
        }
    }

    /// C# `Value`: the validated value after Apply; `None` before and after a cancel.
    pub fn value(&self) -> Option<i32> {
        self.value.get()
    }

    /// C# `ErrorMessage`: the last refusal text; never cleared (the C# never cleared it).
    pub fn error_message(&self) -> Option<String> {
        self.error_message.borrow().clone()
    }
}

impl DialogContent for ZoomPromptContent {
    /// `tr::dialog_click_zoom_custom_title()` (ambient language, read at call time like the C# `Tr` property).
    fn title(&self) -> String {
        pr_l10n::tr::dialog_click_zoom_custom_title()
    }

    /// The list built in `new` (cloned).
    fn items(&self) -> Vec<DialogItem> {
        self.items.clone()
    }

    /// C# `InitialFocusId` = `EDIT_ID`.
    fn initial_focus_id(&self) -> i32 {
        EDIT_ID
    }

    /// `preferred_client_size(dialog, self.error_visible.get())`.
    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        preferred_client_size(dialog, self.error_visible.get())
    }

    /// `apply_placements` + `apply_button_row` of `layout_plan(dialog, client_width, self.error_visible.get())`.
    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, _client_height: i32) {
        let p = layout_plan(dialog, client_width, self.error_visible.get());
        apply_placements(dialog, &p.placements);
        apply_button_row(dialog, &p.button_row)
    }

    /// C# `OnInitialized()`: `dialog.set_item_text(EDIT_ID, presenter.initial_text())`; then, when the box exists, select its whole text
    /// (`EM_SETSEL(0, -1)`, one SAFETY-commented `user32::send_message`).
    fn on_initialized(&self, dialog: &NativeDialog) {
        let text = self.presenter.borrow().initial_text().to_owned();
        dialog.set_item_text(EDIT_ID, &text);
        let hwnd = dialog.item(EDIT_ID);
        if hwnd != 0 {
            // SAFETY: EM_SETSEL has two integer parameters and no pointer arguments.
            unsafe {
                crate::win32::dialog_native::user32::send_message(hwnd, crate::win32::dialog_native::EM_SETSEL, 0, -1);
            }
        }
    }

    /// C# `StaticTextColor(id)`: `DialogColors::warning()` for `ERROR_ID`, else `DialogColors::text()` (the base answer).
    fn static_text_color(&self, _dialog: &NativeDialog, id: i32) -> u32 {
        if id == ERROR_ID {
            super::native::chrome::DialogColors::warning()
        } else {
            super::native::chrome::DialogColors::text()
        }
    }

    /// C# `OnOk()`: `text = dialog.get_item_text(EDIT_ID)`; `presenter.try_accept(&Localizer::current(), Some(&text))`. `Err(message)`: remember it as the error
    /// message, `error_visible = true`, `set_item_text(ERROR_ID, message)`, `show_item(ERROR_ID, true)`, `(w, h) = preferred_client_size(dialog)`,
    /// `resize_client(w, h)`, `run_layout()`, then focus the box (`set_focus`) and select its text again; return `false` (stay open). `Ok`: `value =
    /// Some(presenter.value())`; return `true`.
    fn on_ok(&self, dialog: &NativeDialog) -> bool {
        let text = dialog.get_item_text(EDIT_ID);
        let result = {
            let mut presenter = self.presenter.borrow_mut();
            presenter
                .try_accept(&pr_l10n::localizer::Localizer::current(), Some(&text))
                .map(|()| presenter.value())
        };
        match result {
            Ok(value) => {
                self.value.set(Some(value));
                true
            }
            Err(message) => {
                *self.error_message.borrow_mut() = Some(message.clone());
                self.error_visible.set(true);
                dialog.set_item_text(ERROR_ID, &message);
                dialog.show_item(ERROR_ID, true);
                let (w, h) = preferred_client_size(dialog, true);
                dialog.resize_client(w, h);
                dialog.run_layout();
                let hwnd = dialog.item(EDIT_ID);
                if hwnd != 0 {
                    // SAFETY: EM_SETSEL takes integer indices and does not dereference lParam.
                    unsafe {
                        crate::win32::dialog_native::user32::set_focus(hwnd);
                        crate::win32::dialog_native::user32::send_message(
                            hwnd,
                            crate::win32::dialog_native::EM_SETSEL,
                            0,
                            -1,
                        );
                    }
                }
                false
            }
        }
    }
}

/// The zoom prompt (C# `ZoomPromptDialog`): the handle plus its content (D-W8B5-301). Show it with `dialog.native().show_modal(owner, ready, loop)`; `Ok(ID_OK)` means
/// `value()` is `Some`.
pub struct ZoomPromptDialog {
    native: NativeDialog,
    content: Rc<ZoomPromptContent>,
}

impl fmt::Debug for ZoomPromptDialog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZoomPromptDialog")
            .field("native", &self.native)
            .finish_non_exhaustive()
    }
}

impl ZoomPromptDialog {
    /// C# `ZoomPromptDialog(currentPercent, chrome = null)`: `content = Rc::new(ZoomPromptContent::new(current_percent))`, `native =
    /// NativeDialog::with_chrome(content.clone(), chrome)`.
    pub fn new(current_percent: i32, chrome: Option<Rc<dyn NativeDialogChrome>>) -> Self {
        let content = Rc::new(ZoomPromptContent::new(current_percent));
        let native = NativeDialog::with_chrome(content.clone(), chrome);
        Self { native, content }
    }

    /// The base-class handle (`show_modal`, `close`, `hwnd`, `is_closed`, `dpi`, `item`, ...).
    pub fn native(&self) -> &NativeDialog {
        &self.native
    }

    /// C# `Value`.
    pub fn value(&self) -> Option<i32> {
        self.content.value()
    }

    /// C# `ErrorMessage`.
    pub fn error_message(&self) -> Option<String> {
        self.content.error_message()
    }
}
