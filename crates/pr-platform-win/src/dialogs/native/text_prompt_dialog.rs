//! W9-E3: port of `PhotoReview.Shell.Win32/Dialogs/Native/TextPromptDialog.cs` (99 lines, baseline `7eaee2a2`): a one-line text box (the display name of a newly added
//! editor): a label, an edit box, Cancel and Apply. A blank text is refused (the dialog stays open). Contract: `contracts/W9-E3.md`. Same shape as
//! `dialogs::zoom_prompt` (W8-18p); the layout vocabulary (`DialogMeasure`, `Placement`, `ButtonRowPlan`) is imported from there.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::dialogs::native::chrome::NativeDialogChrome;
use crate::dialogs::native::dialog::{DialogContent, ID_CANCEL, ID_OK, NativeDialog};
use crate::dialogs::native::template::DialogItem;
use crate::dialogs::zoom_prompt::{ButtonRowPlan, DialogMeasure, Placement};

/// C# `LabelId`.
pub const LABEL_ID: i32 = 100;
/// C# `EditId`.
pub const EDIT_ID: i32 = 101;
/// C# `CancelId` = `IdCancel`.
pub const CANCEL_ID: i32 = ID_CANCEL;
/// C# `ApplyId` = `IdOk` (the default button).
pub const APPLY_ID: i32 = ID_OK;

/// C# `Items`, pure: Static `LABEL_ID` (`label`, `DIALOG_STYLE_CHILD | SS_NOPREFIX`); Edit `EDIT_ID` (`""`, `DIALOG_STYLE_CHILD | WS_TABSTOP | ES_AUTOHSCROLL`, ex
/// `WS_EX_CLIENTEDGE`); Button `CANCEL_ID` (`common.cancel`, `child | tab | BS_PUSHBUTTON`); Button `APPLY_ID` (`common.apply`, `child | tab | BS_DEFPUSHBUTTON`).
pub fn build_items(label: &str, cancel_text: &str, apply_text: &str) -> Vec<DialogItem> {
    use super::template::{DIALOG_STYLE_CHILD as CHILD, DialogControlClass as C};
    use crate::win32::dialog_native as n;
    vec![
        DialogItem::new(C::Static, LABEL_ID, label, CHILD | n::SS_NOPREFIX),
        DialogItem::new(C::Edit, EDIT_ID, "", CHILD | n::WS_TABSTOP | n::ES_AUTOHSCROLL)
            .with_ex_style(n::WS_EX_CLIENTEDGE),
        DialogItem::new(
            C::Button,
            CANCEL_ID,
            cancel_text,
            CHILD | n::WS_TABSTOP | n::BS_PUSHBUTTON,
        ),
        DialogItem::new(
            C::Button,
            APPLY_ID,
            apply_text,
            CHILD | n::WS_TABSTOP | n::BS_DEFPUSHBUTTON,
        ),
    ]
}

/// C# `EditHeight()` and `ButtonHeight()` (equal): `max(dy(14), line_height + dy(6))`.
pub fn control_height(m: &dyn DialogMeasure) -> i32 {
    m.dy(14).max(m.line_height().wrapping_add(m.dy(6)))
}

/// C# `PreferredClientSize()`: `margin_x = dx(12)`, `margin_y = dy(10)`; `width = max(dx(220), button_row_width([CANCEL_ID, APPLY_ID]) + 2 * margin_x)`;
/// `content = width - 2 * margin_x`; `height = margin_y + measure_item_height(LABEL_ID, content) + dy(4) + control_height + dy(10) + control_height + margin_y`.
pub fn preferred_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    let margin_x = m.dx(12);
    let margin_y = m.dy(10);
    let width = m.dx(220).max(
        m.button_row_width(&[CANCEL_ID, APPLY_ID])
            .wrapping_add(margin_x.wrapping_mul(2)),
    );
    let content = width.wrapping_sub(margin_x.wrapping_mul(2));
    let height = margin_y
        .wrapping_add(m.measure_item_height(LABEL_ID, content))
        .wrapping_add(m.dy(4))
        .wrapping_add(control_height(m))
        .wrapping_add(m.dy(10))
        .wrapping_add(control_height(m))
        .wrapping_add(margin_y);
    (width, height)
}

/// What `layout_plan` decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPromptLayoutPlan {
    /// Label then box.
    pub placements: Vec<Placement>,
    /// `[CANCEL_ID, APPLY_ID]`.
    pub button_row: ButtonRowPlan,
}

/// C# `LayoutControls(clientWidth, clientHeight)` (`client_height` unused): label at `(margin_x, margin_y)` with `measure_item_height`, `y += label + dy(4)`, the box
/// `control_height` high, `y += control_height + dy(10)`, the button row at `top = y`, `right_margin = margin_x`, `height = control_height`.
pub fn layout_plan(m: &dyn DialogMeasure, client_width: i32) -> TextPromptLayoutPlan {
    let margin_x = m.dx(12);
    let margin_y = m.dy(10);
    let content = client_width.wrapping_sub(margin_x.wrapping_mul(2));
    let label_height = m.measure_item_height(LABEL_ID, content);
    let edit_y = margin_y.wrapping_add(label_height).wrapping_add(m.dy(4));
    let height = control_height(m);
    TextPromptLayoutPlan {
        placements: vec![
            Placement::new(LABEL_ID, margin_x, margin_y, content, label_height),
            Placement::new(EDIT_ID, margin_x, edit_y, content, height),
        ],
        button_row: ButtonRowPlan {
            client_width,
            right_margin: margin_x,
            top: edit_y.wrapping_add(height).wrapping_add(m.dy(10)),
            height,
            ids: vec![CANCEL_ID, APPLY_ID],
        },
    }
}

/// The `DialogContent` of the prompt (C# `TextPromptDialog` without the base class).
pub struct TextPromptContent {
    title: String,
    initial: String,
    items: Vec<DialogItem>,
    value: RefCell<Option<String>>,
}

impl fmt::Debug for TextPromptContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextPromptContent").finish_non_exhaustive()
    }
}

impl TextPromptContent {
    /// C# constructor body: `initial` `None`-like -> `""` (C# `initial ?? string.Empty`), `items = build_items(label, ..)` evaluated now.
    pub fn new(title: &str, label: &str, initial: &str) -> Self {
        Self {
            title: title.to_owned(),
            initial: initial.to_owned(),
            items: build_items(label, &pr_l10n::tr::common_cancel(), &pr_l10n::tr::common_apply()),
            value: RefCell::new(None),
        }
    }

    /// C# `Value`: the text typed, trimmed, after Apply; `None` before and after a cancel.
    pub fn value(&self) -> Option<String> {
        self.value.borrow().clone()
    }
}

impl DialogContent for TextPromptContent {
    /// The title given to the constructor.
    fn title(&self) -> String {
        self.title.clone()
    }
    /// The list built in `new` (cloned).
    fn items(&self) -> Vec<DialogItem> {
        self.items.clone()
    }
    /// C# `InitialFocusId` = `EDIT_ID`.
    fn initial_focus_id(&self) -> i32 {
        EDIT_ID
    }
    /// `preferred_client_size(dialog)`.
    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        preferred_client_size(dialog)
    }
    /// Applies `layout_plan(dialog, client_width)`.
    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, _client_height: i32) {
        let plan = layout_plan(dialog, client_width);
        crate::dialogs::zoom_prompt::apply_placements(dialog, &plan.placements);
        crate::dialogs::zoom_prompt::apply_button_row(dialog, &plan.button_row);
    }
    /// C# `OnInitialized()`: the box text is `initial`, whole text selected (`EM_SETSEL(0, -1)`).
    fn on_initialized(&self, dialog: &NativeDialog) {
        dialog.set_item_text(EDIT_ID, &self.initial);
        let hwnd = dialog.item(EDIT_ID);
        if hwnd != 0 {
            // SAFETY: EM_SETSEL takes scalar indices; the edit handle belongs to this live dialog.
            unsafe {
                crate::win32::dialog_native::user32::send_message(hwnd, crate::win32::dialog_native::EM_SETSEL, 0, -1);
            }
        }
    }
    /// C# `OnOk()`: `text = get_item_text(EDIT_ID).trim()`; empty -> focus the box and return `false` (stay open, no message); else `value = Some(text)` and `true`.
    fn on_ok(&self, dialog: &NativeDialog) -> bool {
        let text = dialog.get_item_text(EDIT_ID);
        let text = text.trim();
        if text.is_empty() {
            let hwnd = dialog.item(EDIT_ID);
            if hwnd != 0 {
                crate::win32::dialog_native::user32::set_focus(hwnd);
            }
            return false;
        }
        *self.value.borrow_mut() = Some(text.to_owned());
        true
    }
}

/// The text prompt (C# `TextPromptDialog`): the handle plus its content. `Ok(ID_OK)` from `native().show_modal(..)` means `value()` is `Some`.
pub struct TextPromptDialog {
    native: NativeDialog,
    content: Rc<TextPromptContent>,
}

impl fmt::Debug for TextPromptDialog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextPromptDialog")
            .field("native", &self.native)
            .finish_non_exhaustive()
    }
}

impl TextPromptDialog {
    /// C# `TextPromptDialog(title, label, initial, chrome = null)`.
    pub fn new(title: &str, label: &str, initial: &str, chrome: Option<Rc<dyn NativeDialogChrome>>) -> Self {
        let content = Rc::new(TextPromptContent::new(title, label, initial));
        let native = NativeDialog::with_chrome(content.clone(), chrome);
        Self { native, content }
    }

    /// The base-class handle.
    pub fn native(&self) -> &NativeDialog {
        &self.native
    }

    /// C# `Value`.
    pub fn value(&self) -> Option<String> {
        self.content.value()
    }
}
