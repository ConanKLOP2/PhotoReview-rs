//! Port of `SettingsDialog.cs` lines 566 to 631 (`ActivatePage`, `CyclePage`, `HandleKeyForTest`, `OnDialogKey`) and 778 (`PageOf`), baseline `d92d6aae`. Unit W8-19j.
//! Contract: `contracts/W8-19j.md`.

use super::frame::SettingsContent;
use crate::dialogs::native::dialog::NativeDialog;
use crate::win32::structs::{Msg, Wparam};

/// C# `VkTab`.
pub const VK_TAB: u32 = 0x09;
/// C# `VkControl`.
pub const VK_CONTROL: i32 = 0x11;
/// C# `VkShift`.
pub const VK_SHIFT: i32 = 0x10;

/// C# `PageOf(id)`: `id >= 1000 ? id / 1000 - 1 : -1` (the page whose id range contains `id`; may be >= the page count for an id >= 8000, callers range-check).
pub fn page_of(id: i32) -> i32 {
    if id >= 1000 { id / 1000 - 1 } else { -1 }
}

/// What `OnDialogKey` decided for one key message, before any side effect (pure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKeyAction {
    /// Not ours: the dialog manager handles it (plain Tab moves the focus, also out of a key box).
    NotHandled,
    /// Ctrl+Tab / Ctrl+Shift+Tab: `cycle_page(forward)` and the key is consumed.
    CyclePage {
        /// `false` when Shift is down.
        forward: bool,
    },
    /// A key message for a key capture box: `capture_key(..)` decides (it returns `false` for Tab, Esc, modifiers, IME markers: they are not swallowed).
    CaptureKey,
}

/// C# `OnDialogKey` decision (lines 608 to 631), pure: `message == WM_KEYDOWN && wparam == VK_TAB` -> `CyclePage { forward: !shift_down }` when `control_down`, else
/// `NotHandled` (a plain Tab is NEVER captured, also not in a key box); otherwise `message` is `WM_KEYDOWN` or `WM_SYSKEYDOWN` and `target_is_key_box` -> `CaptureKey`;
/// otherwise `NotHandled`. `WM_SYSKEYDOWN` + Tab is NOT a page cycle.
pub fn classify_dialog_key(
    message: u32,
    wparam: Wparam,
    control_down: bool,
    shift_down: bool,
    target_is_key_box: bool,
) -> DialogKeyAction {
    const WM_KEYDOWN: u32 = 0x0100;
    const WM_SYSKEYDOWN: u32 = 0x0104;
    if message == WM_KEYDOWN && wparam == VK_TAB as Wparam {
        return if control_down {
            DialogKeyAction::CyclePage { forward: !shift_down }
        } else {
            DialogKeyAction::NotHandled
        };
    }
    if (message == WM_KEYDOWN || message == WM_SYSKEYDOWN) && target_is_key_box {
        DialogKeyAction::CaptureKey
    } else {
        DialogKeyAction::NotHandled
    }
}

impl SettingsContent {
    /// C# `ActivatePage(index, remember, layout)`: `index` out of range -> nothing. Else, in this order: when another page is showing, hide its viewport (`SW_HIDE`);
    /// `ensure_page_created(index)`; `active = index`; `remember` -> `SettingsPresenter::remember_page(key)`; when the navigation list selects another row, select this one
    /// (`select_single`, no notification); `set_item_text(PAGE_TITLE_ID, page.title())`; `scroll[index] = 0`; `page.on_activated()` (skipped when the page is borrowed);
    /// `show_window(viewport(index), SW_SHOW)`; `layout` -> `dialog.run_layout()`.
    pub fn activate_page(&self, dialog: &NativeDialog, index: i32, remember: bool, layout: bool) {
        let Ok(index_usize) = usize::try_from(index) else {
            return;
        };
        let Some(page) = self.pages.get(index_usize).cloned() else {
            return;
        };
        let old = self.view.active.get();
        if old >= 0 && old != index {
            crate::win32::api::user32::show_window(
                dialog.item(crate::dialogs::settings::dialog::frame::viewport_id(old)),
                crate::win32::messages::SW_HIDE as i32,
            );
        }
        self.ensure_page_created(dialog, index_usize);
        self.view.active.set(index);
        let title_and_key = page.try_borrow().ok().map(|page| (page.title(), page.key().to_owned()));
        let Some((title, key)) = title_and_key else { return };
        if remember {
            pr_app::presenters::settings::SettingsPresenter::remember_page(&key);
        }
        let nav = crate::dialogs::native::controls::basic::ListBoxControl::selected_index(
            dialog.item(crate::dialogs::settings::dialog::frame::NAV_ID),
        );
        if nav != index {
            crate::dialogs::native::controls::basic::ListBoxControl::select_single(
                dialog.item(crate::dialogs::settings::dialog::frame::NAV_ID),
                index,
            );
        }
        dialog.set_item_text(crate::dialogs::settings::dialog::frame::PAGE_TITLE_ID, &title);
        if let Ok(mut views) = self.view.pages.try_borrow_mut() {
            if let Some(view) = views.get_mut(index_usize) {
                view.scroll = 0;
            }
        }
        if let Ok(mut page) = page.try_borrow_mut() {
            page.on_activated();
        }
        crate::win32::api::user32::show_window(
            dialog.item(crate::dialogs::settings::dialog::frame::viewport_id(index)),
            crate::win32::messages::SW_SHOW as i32,
        );
        if layout {
            dialog.run_layout();
        }
    }

    /// C# `CyclePage(forward)`: `activate_page(SettingsPresenter::next_page_index(max(0, active), page_count, forward), true, true)`.
    pub fn cycle_page(&self, dialog: &NativeDialog, forward: bool) {
        let active = self.view.active.get().max(0);
        let next =
            pr_app::presenters::settings::SettingsPresenter::next_page_index(active, self.pages.len() as i32, forward);
        self.activate_page(dialog, next, true, true)
    }

    /// C# `OnDialogKey(in Msg)` (lines 608 to 631): `classify_dialog_key(message.message, message.wParam, control_down(), shift_down(), is_key_box(message.hwnd))`;
    /// `control_down()` = `options.control_key_down()` or `get_key_state(VK_CONTROL) < 0`; `shift_down()` = `get_key_state(VK_SHIFT) < 0`.
    /// The native modal loop calls this hook for EVERY message it pulls (mouse moves, paints, timers), so the three inputs are computed LAZILY, as the C# short-circuit does:
    /// `message.message` other than `WM_KEYDOWN` / `WM_SYSKEYDOWN` -> `false` at once (no `get_dlg_ctrl_id`, no `get_key_state`, the `control_key_down` option is NOT called);
    /// `control_down()` and `shift_down()` only for `WM_KEYDOWN` + `VK_TAB`; `is_key_box` only for the other key messages. `CyclePage` -> `cycle_page` and `true`;
    /// `CaptureKey` -> `capture_key(dialog, message.hwnd, message.wParam, message.lParam)`; `NotHandled` -> `false`.
    pub fn handle_dialog_key(&self, dialog: &NativeDialog, message: &Msg) -> bool {
        const WM_KEYDOWN: u32 = 0x0100;
        const WM_SYSKEYDOWN: u32 = 0x0104;
        if message.message != WM_KEYDOWN && message.message != WM_SYSKEYDOWN {
            return false;
        }
        let wparam = message.wParam.0;
        let hwnd = message.hwnd.0 as isize;
        let lparam = message.lParam.0;
        let is_tab = message.message == WM_KEYDOWN && wparam == VK_TAB as usize;
        let (control_down, shift_down) = if is_tab {
            let control = self
                .options
                .control_key_down
                .as_ref()
                .map(|check| check())
                .unwrap_or_else(|| key_down(VK_CONTROL));
            (control, key_down(VK_SHIFT))
        } else {
            (false, false)
        };
        match classify_dialog_key(
            message.message,
            wparam,
            control_down,
            shift_down,
            !is_tab && self.is_key_box(dialog, hwnd),
        ) {
            DialogKeyAction::NotHandled => false,
            DialogKeyAction::CyclePage { forward } => {
                self.cycle_page(dialog, forward);
                true
            }
            DialogKeyAction::CaptureKey => self.capture_key(dialog, hwnd, wparam, lparam),
        }
    }

    /// The navigation list selection changed (`LBN_SELCHANGE` on `NAV_ID`): `activate_page(max(0, ListBoxControl::selected_index(item(NAV_ID))), true, true)`.
    pub(super) fn on_nav_selection_changed(&self, dialog: &NativeDialog) {
        let selected = crate::dialogs::native::controls::basic::ListBoxControl::selected_index(
            dialog.item(crate::dialogs::settings::dialog::frame::NAV_ID),
        );
        self.activate_page(dialog, selected.max(0), true, true)
    }
}

fn key_down(key: i32) -> bool {
    // SAFETY: GetKeyState accepts a virtual-key code and has no pointer arguments.
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(key) < 0 }
}
