//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/RenamePromptDialog.cs` (224 lines, baseline `7eaee2a2`). Unit W9-R8. Contract: `contracts/W9-R8.md`.
//!
//! FEAT-RENAME: the modal box of "Rename..." / F2. One single-line edit pre-filled with the name WITHOUT its extension (all selected), the extension fixed to the
//! right of the box. The name is checked as it is typed through `RenamePromptRequest::validate` (`is_commit = false`): a refusal shows under the box and
//! disables Rename; the final OK asks again with `is_commit = true` (it may probe the disk). Enter = Rename (when valid), Escape / close / Alt+F4 = cancel.
//! Same shape as `zoom_prompt` / `native::text_prompt_dialog`: the layout is a pure function of a [`DialogMeasure`].
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use pr_core::abstractions::RenamePromptRequest;

use super::native::chrome::{DialogColors, NativeDialogChrome};
use super::native::dialog::{DialogContent, ID_CANCEL, ID_OK, NativeDialog};
use super::native::template::{DIALOG_STYLE_CHILD, DialogControlClass, DialogItem};
use super::secondary_window_port::PreparedDialog;
use super::zoom_prompt::{ButtonRowPlan, DialogMeasure, Placement, apply_button_row, apply_placements};
use crate::win32::dialog_native::{self as n, user32};
use crate::win32::messages::WS_CHILD;
use crate::win32::structs::Hwnd;

/// C# `LabelId`.
pub const LABEL_ID: i32 = 100;
/// C# `EditId`.
pub const EDIT_ID: i32 = 101;
/// C# `ExtensionId`.
pub const EXTENSION_ID: i32 = 102;
/// C# `ErrorId` (created without `WS_VISIBLE`).
pub const ERROR_ID: i32 = 103;
/// C# `CancelId` = `IdCancel`.
pub const CANCEL_ID: i32 = ID_CANCEL;
/// C# `RenameId` = `IdOk` (the default button).
pub const RENAME_ID: i32 = ID_OK;

/// C# `Items`, pure: Static label, Edit (`WS_EX_CLIENTEDGE`), Static extension, Static error (no `WS_VISIBLE`), Cancel, Rename (default button), in that order.
pub fn build_items(label: &str, extension: &str, cancel_text: &str, rename_text: &str) -> Vec<DialogItem> {
    use DialogControlClass as C;
    vec![
        DialogItem::new(C::Static, LABEL_ID, label, DIALOG_STYLE_CHILD | n::SS_NOPREFIX),
        DialogItem::new(
            C::Edit,
            EDIT_ID,
            "",
            DIALOG_STYLE_CHILD | n::WS_TABSTOP | n::ES_AUTOHSCROLL,
        )
        .with_ex_style(n::WS_EX_CLIENTEDGE),
        DialogItem::new(C::Static, EXTENSION_ID, extension, DIALOG_STYLE_CHILD | n::SS_NOPREFIX),
        DialogItem::new(C::Static, ERROR_ID, "", WS_CHILD | n::SS_NOPREFIX),
        DialogItem::new(
            C::Button,
            CANCEL_ID,
            cancel_text,
            DIALOG_STYLE_CHILD | n::WS_TABSTOP | n::BS_PUSHBUTTON,
        ),
        DialogItem::new(
            C::Button,
            RENAME_ID,
            rename_text,
            DIALOG_STYLE_CHILD | n::WS_TABSTOP | n::BS_DEFPUSHBUTTON,
        ),
    ]
}

/// C# `EditHeight()` and `ButtonHeight()` (equal): `max(dy(14), line_height + dy(6))`.
pub fn control_height(m: &dyn DialogMeasure) -> i32 {
    m.dy(14).max(m.line_height().wrapping_add(m.dy(6)))
}

/// C# `ExtensionWidth`: `0` for no extension, else the text width plus `dx(4)`.
pub fn extension_width(m: &dyn DialogMeasure, extension: &str) -> i32 {
    if extension.is_empty() {
        0
    } else {
        m.measure_text_width(extension).wrapping_add(m.dx(4))
    }
}

/// C# `PreferredClientSize()`: `width = max(dx(260), button_row_width([CANCEL, RENAME]) + 2 * dx(12))`; `height = dy(10) + label + dy(4) + edit`, with the
/// error `+ dy(4) + error`, then `+ dy(10) + button + dy(10)`.
pub fn preferred_client_size(m: &dyn DialogMeasure, error_visible: bool) -> (i32, i32) {
    let margin_x = m.dx(12);
    let margin_y = m.dy(10);
    let width = m.dx(260).max(
        m.button_row_width(&[CANCEL_ID, RENAME_ID])
            .wrapping_add(margin_x.wrapping_mul(2)),
    );
    let content = width.wrapping_sub(margin_x.wrapping_mul(2));
    let mut height = margin_y
        .wrapping_add(m.measure_item_height(LABEL_ID, content))
        .wrapping_add(m.dy(4))
        .wrapping_add(control_height(m));
    if error_visible {
        height = height
            .wrapping_add(m.dy(4))
            .wrapping_add(m.measure_item_height(ERROR_ID, content));
    }
    height = height
        .wrapping_add(m.dy(10))
        .wrapping_add(control_height(m))
        .wrapping_add(margin_y);
    (width, height)
}

/// What `layout_plan` decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameLayoutPlan {
    /// Label, box, extension and (only when the error is visible) the error line, in the C# `Place` order.
    pub placements: Vec<Placement>,
    /// `[CANCEL_ID, RENAME_ID]` at the bottom.
    pub button_row: ButtonRowPlan,
}

/// C# `LayoutControls(clientWidth, clientHeight)` (`client_height` unused). The box is `content - extension - gap` wide (at least `dx(40)`), `gap = dx(3)` when
/// there is an extension; the extension label is as high as one text row (`min(edit, line_height + dy(2))`) and vertically centred on the box; without an
/// extension it is placed at `(margin_x, y)` with size `0 x 0`.
pub fn layout_plan(m: &dyn DialogMeasure, extension: &str, client_width: i32, error_visible: bool) -> RenameLayoutPlan {
    let margin_x = m.dx(12);
    let mut y = m.dy(10);
    let content = client_width.wrapping_sub(margin_x.wrapping_mul(2));
    let edit_height = control_height(m);
    let label_height = m.measure_item_height(LABEL_ID, content);
    let mut placements = vec![Placement::new(LABEL_ID, margin_x, y, content, label_height)];
    y = y.wrapping_add(label_height).wrapping_add(m.dy(4));

    let ext = extension_width(m, extension);
    let gap = if ext == 0 { 0 } else { m.dx(3) };
    placements.push(Placement::new(
        EDIT_ID,
        margin_x,
        y,
        m.dx(40).max(content.wrapping_sub(ext).wrapping_sub(gap)),
        edit_height,
    ));
    if ext > 0 {
        let text_height = edit_height.min(m.line_height().wrapping_add(m.dy(2)));
        placements.push(Placement::new(
            EXTENSION_ID,
            margin_x.wrapping_add(content).wrapping_sub(ext),
            y.wrapping_add(edit_height.wrapping_sub(text_height) / 2),
            ext,
            text_height,
        ));
    } else {
        placements.push(Placement::new(EXTENSION_ID, margin_x, y, 0, 0));
    }

    y = y.wrapping_add(edit_height);
    if error_visible {
        y = y.wrapping_add(m.dy(4));
        let error_height = m.measure_item_height(ERROR_ID, content);
        placements.push(Placement::new(ERROR_ID, margin_x, y, content, error_height));
        y = y.wrapping_add(error_height);
    }
    y = y.wrapping_add(m.dy(10));
    RenameLayoutPlan {
        placements,
        button_row: ButtonRowPlan {
            client_width,
            right_margin: margin_x,
            top: y,
            height: edit_height,
            ids: vec![CANCEL_ID, RENAME_ID],
        },
    }
}

/// The `DialogContent` of the rename box (C# `RenamePromptDialog` without the base class). No `RefCell` borrow is held across an OS call or the validate callback.
pub struct RenamePromptContent {
    request: RenamePromptRequest,
    items: Vec<DialogItem>,
    error_visible: Cell<bool>,
    error_message: RefCell<Option<String>>,
    value: RefCell<Option<String>>,
}

impl fmt::Debug for RenamePromptContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenamePromptContent")
            .field("request", &self.request)
            .field("error_visible", &self.error_visible.get())
            .finish_non_exhaustive()
    }
}

impl RenamePromptContent {
    /// C# constructor body: the items are built now (the texts keep the language of the construction).
    pub fn new(request: RenamePromptRequest) -> Self {
        let items = build_items(
            &pr_l10n::tr::dialog_rename_label(),
            &request.extension,
            &pr_l10n::tr::common_cancel(),
            &pr_l10n::tr::dialog_rename_ok(),
        );
        Self {
            request,
            items,
            error_visible: Cell::new(false),
            error_message: RefCell::new(None),
            value: RefCell::new(None),
        }
    }

    /// C# `Value`: the trimmed text after Rename; `None` when cancelled.
    pub fn value(&self) -> Option<String> {
        self.value.borrow().clone()
    }

    /// C# `ErrorMessage`: the refusal shown under the box (`None` when none).
    pub fn error_message(&self) -> Option<String> {
        self.error_message.borrow().clone()
    }

    /// C# `IsRenameEnabled`.
    pub fn is_rename_enabled(&self, dialog: &NativeDialog) -> bool {
        let button = dialog.item(RENAME_ID);
        button != 0 && user32::is_window_enabled(button)
    }

    /// C# `Selection`: `(start, end)` of the box selection.
    pub fn selection(&self, dialog: &NativeDialog) -> (i32, i32) {
        let edit = dialog.item(EDIT_ID);
        if edit == 0 {
            return (0, 0);
        }
        // SAFETY: EM_GETSEL with null output pointers returns the packed indices in the result; the handle is a live control of this dialog.
        let packed = unsafe { user32::send_message(edit, n::EM_GETSEL, 0, 0) } as i64;
        ((packed & 0xFFFF) as i32, ((packed >> 16) & 0xFFFF) as i32)
    }

    /// C# `Revalidate()`: the typed (trimmed) name, without the disk.
    fn revalidate(&self, dialog: &NativeDialog) {
        let text = dialog.get_item_text(EDIT_ID);
        let error = (self.request.validate)(text.trim(), false);
        self.show_error(dialog, error);
    }

    /// C# `ShowError(error)`: Rename is disabled while an error shows; the line, the window size and the layout follow only when the text changed.
    fn show_error(&self, dialog: &NativeDialog, error: Option<String>) {
        let changed = *self.error_message.borrow() != error;
        *self.error_message.borrow_mut() = error.clone();
        let visible = error.as_deref().is_some_and(|text| !text.is_empty());
        let button = dialog.item(RENAME_ID);
        if button != 0 {
            user32::enable_window(button, !visible);
        }
        if !changed {
            return;
        }
        dialog.set_item_text(ERROR_ID, error.as_deref().unwrap_or(""));
        dialog.show_item(ERROR_ID, visible);
        self.error_visible.set(visible);
        let (w, h) = preferred_client_size(dialog, visible);
        dialog.resize_client(w, h);
        dialog.run_layout();
    }
}

impl DialogContent for RenamePromptContent {
    /// `tr::dialog_rename_title()`.
    fn title(&self) -> String {
        pr_l10n::tr::dialog_rename_title()
    }

    fn items(&self) -> Vec<DialogItem> {
        self.items.clone()
    }

    /// C# `InitialFocusId` = `EDIT_ID`.
    fn initial_focus_id(&self) -> i32 {
        EDIT_ID
    }

    fn preferred_client_size(&self, dialog: &NativeDialog) -> (i32, i32) {
        preferred_client_size(dialog, self.error_visible.get())
    }

    fn layout_controls(&self, dialog: &NativeDialog, client_width: i32, _client_height: i32) {
        let plan = layout_plan(dialog, &self.request.extension, client_width, self.error_visible.get());
        apply_placements(dialog, &plan.placements);
        apply_button_row(dialog, &plan.button_row);
    }

    /// C# `OnInitialized()`: the stem in the box, all of it selected, then a first validation.
    fn on_initialized(&self, dialog: &NativeDialog) {
        dialog.set_item_text(EDIT_ID, &self.request.initial_name);
        let edit = dialog.item(EDIT_ID);
        if edit != 0 {
            // SAFETY: EM_SETSEL takes two integer indices and no pointer; the handle is a live control of this dialog.
            unsafe {
                user32::send_message(edit, n::EM_SETSEL, 0, -1);
            }
        }
        self.revalidate(dialog);
    }

    /// C# `StaticTextColor`: warning for the error line, secondary for the extension, else the base text colour.
    fn static_text_color(&self, _dialog: &NativeDialog, id: i32) -> u32 {
        match id {
            ERROR_ID => DialogColors::warning(),
            EXTENSION_ID => DialogColors::text_secondary(),
            _ => DialogColors::text(),
        }
    }

    /// C# `OnCommand`: `EN_CHANGE` of the box revalidates (handled); everything else is left to the base class.
    fn on_command(&self, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        if id == EDIT_ID && notification == n::EN_CHANGE {
            self.revalidate(dialog);
            return true;
        }
        false
    }

    /// C# `OnOk()`: validates once more with `is_commit = true`; a refusal shows, the box gets the focus and the dialog stays open.
    fn on_ok(&self, dialog: &NativeDialog) -> bool {
        let text = dialog.get_item_text(EDIT_ID);
        let text = text.trim();
        if let Some(error) = (self.request.validate)(text, true) {
            self.show_error(dialog, Some(error));
            let edit = dialog.item(EDIT_ID);
            if edit != 0 {
                user32::set_focus(edit);
            }
            return false;
        }
        *self.value.borrow_mut() = Some(text.to_owned());
        true
    }
}

/// The modal Win32 rename dialog (C# `RenamePromptDialog`): the native handle plus its content.
pub struct RenamePromptDialog {
    native: NativeDialog,
    content: Rc<RenamePromptContent>,
}

impl fmt::Debug for RenamePromptDialog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenamePromptDialog")
            .field("native", &self.native)
            .finish_non_exhaustive()
    }
}

impl RenamePromptDialog {
    /// C# `LabelId`.
    pub const LABEL_ID: i32 = LABEL_ID;
    /// C# `EditId`.
    pub const EDIT_ID: i32 = EDIT_ID;
    /// C# `ExtensionId`.
    pub const EXTENSION_ID: i32 = EXTENSION_ID;
    /// C# `ErrorId`.
    pub const ERROR_ID: i32 = ERROR_ID;
    /// C# `CancelId`.
    pub const CANCEL_ID: i32 = CANCEL_ID;
    /// C# `RenameId`.
    pub const RENAME_ID: i32 = RENAME_ID;

    /// C# `RenamePromptDialog(request, chrome = null)`.
    pub fn new(request: RenamePromptRequest, chrome: Option<Rc<dyn NativeDialogChrome>>) -> Self {
        let content = Rc::new(RenamePromptContent::new(request));
        let native = NativeDialog::with_chrome(content.clone(), chrome);
        Self { native, content }
    }

    /// The base-class handle (`show_modal`, `close`, `item`, ...).
    pub fn native(&self) -> &NativeDialog {
        &self.native
    }

    /// The content (value, error, enabled state, selection).
    pub fn content(&self) -> &Rc<RenamePromptContent> {
        &self.content
    }

    /// C# `Value`.
    pub fn value(&self) -> Option<String> {
        self.content.value()
    }

    /// C# `ErrorMessage`.
    pub fn error_message(&self) -> Option<String> {
        self.content.error_message()
    }

    /// C# `IsRenameEnabled`.
    pub fn is_rename_enabled(&self) -> bool {
        self.content.is_rename_enabled(&self.native)
    }

    /// C# `Selection`.
    pub fn selection(&self) -> (i32, i32) {
        self.content.selection(&self.native)
    }

    /// C# `CurrentText`.
    pub fn current_text(&self) -> String {
        self.native.get_item_text(EDIT_ID)
    }

    /// C# `ExtensionText`.
    pub fn extension_text(&self) -> String {
        self.native.get_item_text(EXTENSION_ID)
    }

    /// Show the dialog modally over `owner` and return only the accepted name part (`None` when cancelled, closed or when the window could not be created).
    pub fn show(&self, owner: Hwnd) -> Option<String> {
        match self.native.show_modal(owner, None, None) {
            Ok(ID_OK) => self.value(),
            _ => None,
        }
    }

    /// The port's way in: the dialog with the reading of its result (`finish(ID_OK)` is the accepted name, any other id `None`).
    pub fn prepare(
        request: RenamePromptRequest,
        chrome: Option<Rc<dyn NativeDialogChrome>>,
    ) -> PreparedDialog<Option<String>> {
        let dialog = Self::new(request, chrome);
        let content = Rc::clone(&dialog.content);
        PreparedDialog::new(
            dialog.native,
            Box::new(move |id| if id == ID_OK { content.value() } else { None }),
        )
    }
}
