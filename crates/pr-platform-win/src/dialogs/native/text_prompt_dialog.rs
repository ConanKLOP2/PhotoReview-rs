//! W9-E3: port of `PhotoReview.Shell.Win32/Dialogs/Native/TextPromptDialog.cs` (99 lines, baseline `7eaee2a2`): a one-line text box (the display name of a newly added
//! editor): a label, an edit box, Cancel and Apply. A blank text is refused (the dialog stays open). Contract: `contracts/W9-E3.md`. Same shape as
//! `dialogs::zoom_prompt` (W8-18p); the layout vocabulary (`DialogMeasure`, `Placement`, `ButtonRowPlan`) is imported from there. Every body is `todo!()`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

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
    todo!()
}

/// C# `EditHeight()` and `ButtonHeight()` (equal): `max(dy(14), line_height + dy(6))`.
pub fn control_height(m: &dyn DialogMeasure) -> i32 {
    todo!()
}

/// C# `PreferredClientSize()`: `margin_x = dx(12)`, `margin_y = dy(10)`; `width = max(dx(220), button_row_width([CANCEL_ID, APPLY_ID]) + 2 * margin_x)`;
/// `content = width - 2 * margin_x`; `height = margin_y + measure_item_height(LABEL_ID, content) + dy(4) + control_height + dy(10) + control_height + margin_y`.
pub fn preferred_client_size(m: &dyn DialogMeasure) -> (i32, i32) {
    todo!()
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
    todo!()
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
        todo!()
    }

    /// C# `Value`: the text typed, trimmed, after Apply; `None` before and after a cancel.
    pub fn value(&self) -> Option<String> {
        self.value.borrow().clone()
    }
}

impl DialogContent for TextPromptContent {
    /// The title given to the constructor.
    fn title(&self) -> String {
        todo!()
    }
    /// The list built in `new` (cloned).
    fn items(&self) -> Vec<DialogItem> {
        todo!()
    }
    /// C# `InitialFocusId` = `EDIT_ID`.
    fn initial_focus_id(&self) -> i32 {
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
    /// C# `OnInitialized()`: the box text is `initial`, whole text selected (`EM_SETSEL(0, -1)`).
    fn on_initialized(&self, dialog: &NativeDialog) {
        todo!()
    }
    /// C# `OnOk()`: `text = get_item_text(EDIT_ID).trim()`; empty -> focus the box and return `false` (stay open, no message); else `value = Some(text)` and `true`.
    fn on_ok(&self, dialog: &NativeDialog) -> bool {
        todo!()
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
        todo!()
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
