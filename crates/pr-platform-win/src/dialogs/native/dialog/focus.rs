//! Port of `NativeDialog.cs` lines 974 to 1028 (`InitializeDialog`, `FirstTabStop`) and 1055 to 1074 (`CenterSingleLineEdits`), baseline `d92d6aae`.
//! Unit W8-18j. Contract: `contracts/W8-18j.md`.
//!
//! `WM_INITDIALOG` is where a dialog becomes usable: the DPI, the brushes, the controls and their dark chrome, the fonts, the default button, the first layout and
//! the initial focus. The dialog manager (`IsDialogMessage`) handles Tab, Enter and Esc; this file only chooses the control that starts with the focus.
use super::core::NativeDialog;
use crate::dialogs::native::chrome::DialogChromeControl;
use crate::dialogs::native::edit_centering;
use crate::dialogs::native::template::{DialogControlClass, DialogItem};
use crate::host::icons;
use crate::win32::structs::Hwnd;
use crate::win32::{api::user32, dialog_native};

/// `WS_TABSTOP` test of `FirstTabStop` (pure): `(style & 0x00010000) != 0`.
pub fn is_tab_stop(style: u32) -> bool {
    style & 0x0001_0000 != 0
}

impl NativeDialog {
    /// C# `InitializeDialog(hwnd)` (lines 976 to 1014), the `WM_INITDIALOG` handler. Steps, in order (the order is observable: hooks see a half-built dialog):
    /// 1. `modal.hwnd = hwnd`; `metrics.dpi = dpi_for(owner != 0 ? owner : hwnd)`; `host::icons::apply(hwnd, dpi)` (the app icon of the title bar and Alt+Tab).
    /// 2. `create_brushes()` (18k): the window brush and the input brush.
    /// 3. `items = content.items()`. For each item in order: `child = get_dlg_item(hwnd, item.id)`; when non-zero: a `ListView` is `hook_list_view(child)` (18j,
    ///    `events.rs`), the child joins `controls.children`, and `DialogChromeControl { hwnd: child, class }` joins the chrome list.
    /// 4. `apply_fonts()` (18i); `chrome.apply(hwnd, &chrome_list)` (EXACTLY once); `center_single_line_edits(hwnd, &items)`.
    /// 5. `SendMessage(hwnd, DM_SETDEFID, default_button_id, 0)`; `content.on_initialized(self)`.
    /// 6. `size = content.preferred_client_size(self)`; `place_initially(size)`; `laid_out = true`; `run_layout()`.
    /// 7. `focus = initial_focus_id != 0 ? get_dlg_item(hwnd, initial_focus_id) : first_tab_stop()`; non-zero -> `set_focus(focus)`.
    ///
    /// The router answers `0` (FALSE: the focus was set by hand).
    pub(super) fn initialize_dialog(&self, hwnd: Hwnd) {
        self.inner.modal.hwnd.set(hwnd);
        let owner = self.inner.modal.owner.get();
        let dpi = Self::dpi_for(if owner != 0 { owner } else { hwnd });
        self.inner.metrics.dpi.set(dpi);
        let _ = icons::apply(hwnd, dpi);
        self.create_brushes();
        let items = self.inner.content.items();
        let mut chrome = Vec::new();
        for item in &items {
            let child = dialog_native::user32::get_dlg_item(hwnd, item.id);
            if child == 0 {
                continue;
            }
            if item.class == DialogControlClass::ListView {
                self.hook_list_view(child);
            }
            self.inner.controls.children.borrow_mut().push(child);
            chrome.push(DialogChromeControl {
                hwnd: child,
                class: item.class,
            });
        }
        self.apply_fonts();
        self.inner.chrome.apply(hwnd, &chrome);
        self.center_single_line_edits(hwnd, &items);
        // SAFETY: DM_SETDEFID carries only an integer control ID and no pointers.
        unsafe {
            dialog_native::user32::send_message(
                hwnd,
                dialog_native::DM_SETDEFID,
                self.inner.content.default_button_id() as usize,
                0,
            )
        };
        self.inner.content.on_initialized(self);
        let (width, height) = self.inner.content.preferred_client_size(self);
        self.place_initially(width, height);
        self.inner.modal.laid_out.set(true);
        self.run_layout();
        let focus = if self.inner.content.initial_focus_id() != 0 {
            dialog_native::user32::get_dlg_item(hwnd, self.inner.content.initial_focus_id())
        } else {
            self.first_tab_stop()
        };
        if focus != 0 {
            let _ = dialog_native::user32::set_focus(focus);
        }
    }

    /// C# `FirstTabStop()` (lines 1016 to 1028): the first registered child whose style has `WS_TABSTOP` (`get_window_long_ptr(child, GWL_STYLE) as u32`); `0`
    /// when none.
    pub(super) fn first_tab_stop(&self) -> Hwnd {
        let children = self.inner.controls.children.borrow().clone();
        children
            .into_iter()
            .find(|&child| is_tab_stop(user32::get_window_long_ptr(child, -16) as u32))
            .unwrap_or(0)
    }

    /// C# `CenterSingleLineEdits(hwnd)` (lines 1059 to 1074): for each item of class `Edit` WITHOUT `ES_MULTILINE` (`4`) whose control exists:
    /// `edit_centering::attach(edit)`. A multi-line box honours `EM_SETRECT` and is laid out by its dialog.
    pub(super) fn center_single_line_edits(&self, hwnd: Hwnd, items: &[DialogItem]) {
        for item in items {
            if item.class == DialogControlClass::Edit && item.style & dialog_native::ES_MULTILINE == 0 {
                let edit = dialog_native::user32::get_dlg_item(hwnd, item.id);
                if edit != 0 {
                    let _ = edit_centering::attach(edit);
                }
            }
        }
    }
}
