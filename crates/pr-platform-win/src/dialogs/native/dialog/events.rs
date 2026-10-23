//! Port of `NativeDialog.cs` lines 679 to 800 (`HandleCommand`, `HandleNotify`, `HookListView`, `ListViewSubclassProc`, `IsEdit`, `IsButton`), 607 to 614 (the scroll
//! case of `HandleMessage`) and 1209 to 1224 (`SetListViewSort`), baseline `d92d6aae`. Unit W8-18j. Contract: `contracts/W8-18j.md`.
//!
//! Control events: `WM_COMMAND` (OK, Cancel, anything else to the hook), `WM_NOTIFY` (tooltip text, the dark text colour of themed buttons, everything else to
//! the hook), `WM_HSCROLL` / `WM_VSCROLL` of trackbars, and the subclass of a list view that draws its header (the header sends `NM_CUSTOMDRAW` to the LIST
//! VIEW, which does not forward it to the dialog).
//!
//! `unsafe` here: reading the structures behind the `lParam` of `WM_NOTIFY` and the subclass procedure (`extern "system"`, under `pending_panic::guarded`).
use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;

use super::core::NativeDialog;
use super::core::{ID_CANCEL, ID_OK};
use crate::dialogs::native::chrome::DialogColors;
use crate::dialogs::native::controls::list_tree::ListViewControl;
use crate::host::pending_panic;
use crate::shell_input::native::low_word;
use crate::win32::structs::{Hwnd, Lparam, Lresult, Wparam};
use crate::win32::{api::user32, comctl::common_controls, control_constants as cc, dialog_native, messages};
use windows::Win32::UI::Controls::{NMCUSTOMDRAW, NMHDR};

/// C# `ListViewSubclassId`: ASCII `"LV35"`.
pub const LIST_VIEW_SUBCLASS_ID: usize = 0x4C56_3335;

/// List view and notification state (C# `_listViews`, `_sorts`).
pub(super) struct EventsState {
    /// C# `_listViews`: the list views whose header this dialog draws (subclassed by `hook_list_view`, released by `cleanup`).
    pub(super) list_views: RefCell<Vec<Hwnd>>,
    /// C# `_sorts`: `(sort column, direction)` per list view; `direction` 0 = none, 1 = ascending, 2 = descending. Order is never observable: a map.
    pub(super) sorts: RefCell<HashMap<Hwnd, (i32, i32)>>,
}

impl EventsState {
    /// Both empty.
    #[cfg(test)]
    pub(super) fn new() -> Self {
        Self {
            list_views: RefCell::new(Vec::new()),
            sorts: RefCell::new(HashMap::new()),
        }
    }
}

/// How `HandleCommand` routes a command (pure, lines 679 to 702).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRoute {
    /// `id == ID_OK && notification == BN_CLICKED`: `on_ok()`, close with `ID_OK` when it says `true`.
    Ok,
    /// `id == ID_CANCEL && notification == BN_CLICKED`: `on_cancel()`, close with `ID_CANCEL` when it says `false`.
    Cancel,
    /// Anything else: `content.on_command(id, notification)`.
    Other,
}

/// The routing decision of `HandleCommand` (pure). `BN_CLICKED` is `0`.
pub fn command_route(id: i32, notification: i32) -> CommandRoute {
    if notification == 0 && id == ID_OK {
        CommandRoute::Ok
    } else if notification == 0 && id == ID_CANCEL {
        CommandRoute::Cancel
    } else {
        CommandRoute::Other
    }
}

/// Splits the `wParam` of `WM_COMMAND` (pure): `(id, notification) = ((wparam & 0xFFFF) as i32, ((wparam >> 16) & 0xFFFF) as i32)`.
pub fn split_command_wparam(wparam: Wparam) -> (i32, i32) {
    ((wparam & 0xffff) as i32, ((wparam >> 16) & 0xffff) as i32)
}

/// The Win32 class-name test of `IsEdit` / `IsButton` (pure): the name equals `expected` ignoring ASCII case (C# `OrdinalIgnoreCase` on a 4 / 6 char name).
pub fn class_name_is(actual: &str, expected: &str) -> bool {
    actual.eq_ignore_ascii_case(expected)
}

impl NativeDialog {
    /// C# `HandleCommand(id, notification)` (lines 679 to 702); the return value is the dialog procedure's `1` / `0`. `Ok` -> `if content.on_ok(self) {
    /// end_modal(ID_OK) }` and `true`; `Cancel` -> `if !content.on_cancel(self) { end_modal(ID_CANCEL) }` and `true`; `Other` -> `content.on_command(..)`.
    pub(super) fn handle_command(&self, id: i32, notification: i32) -> bool {
        match command_route(id, notification) {
            CommandRoute::Ok => {
                if self.inner.content.on_ok(self) {
                    self.end_modal(ID_OK);
                }
                true
            }
            CommandRoute::Cancel => {
                if !self.inner.content.on_cancel(self) {
                    self.end_modal(ID_CANCEL);
                }
                true
            }
            CommandRoute::Other => self.inner.content.on_command(self, id, notification),
        }
    }

    /// C# `HandleNotify(hwnd, lParam)` (lines 704 to 734). `lparam == 0` -> `0` (the C# would fault). `lparam` is a `*const NMHDR`: `from = hwndFrom`, `code =
    /// code as i32`. In order: (1) `code == TTN_GETDISPINFO` -> `provide_tip_text(lparam)` (tooltip.rs), return `0`; (2) `id = from != 0 ? get_dlg_ctrl_id(from) :
    /// 0`; (3) `code == NM_CUSTOMDRAW && is_button(from)` -> the lParam is an `NMCUSTOMDRAW`: at `dwDrawStage == CDDS_PREPAINT` `set_text_color(nmcd.hdc,
    /// DialogColors::text_strong())` (themed buttons take the text colour from the theme: grey on dark), then `SetWindowLongPtr(hwnd, DWLP_MSGRESULT,
    /// CDRF_DODEFAULT)` and return `1`; (4) `content.on_notify(self, id, code, lparam)`: `Some(r)` -> `SetWindowLongPtr(hwnd, DWLP_MSGRESULT, r)` and `1`; else `0`.
    pub(super) fn handle_notify(&self, hwnd: Hwnd, lparam: Lparam) -> Lresult {
        if lparam == 0 {
            return 0;
        }
        debug_assert_eq!(std::mem::size_of::<NMHDR>(), 24);
        debug_assert_eq!(std::mem::offset_of!(NMCUSTOMDRAW, dwDrawStage), 24);
        debug_assert_eq!(std::mem::offset_of!(NMCUSTOMDRAW, hdc), 32);
        // SAFETY: WM_NOTIFY lParam points to an NMHDR valid for this call.
        let header = unsafe { &*(lparam as *const NMHDR) };
        let from = header.hwndFrom.0 as Hwnd;
        let code = header.code as i32;
        if code == cc::TTN_GETDISPINFO {
            self.provide_tip_text(lparam);
            return 0;
        }
        let id = if from != 0 {
            dialog_native::user32::get_dlg_ctrl_id(from)
        } else {
            0
        };
        if code == cc::NM_CUSTOMDRAW && Self::is_button(from) {
            // SAFETY: NM_CUSTOMDRAW lParam points to an NMCUSTOMDRAW valid for this call.
            let draw = unsafe { &*(lparam as *const NMCUSTOMDRAW) };
            if draw.dwDrawStage.0 == cc::CDDS_PREPAINT {
                let _ = dialog_native::gdi32::set_text_color(draw.hdc.0 as isize, DialogColors::text_strong());
            }
            // SAFETY: DWLP_MSGRESULT is this dialog's own result slot.
            unsafe { user32::set_window_long_ptr(hwnd, dialog_native::DWLP_MSGRESULT, cc::CDRF_DODEFAULT) };
            return 1;
        }
        if let Some(result) = self.inner.content.on_notify(self, id, code, lparam) {
            // SAFETY: DWLP_MSGRESULT is this dialog's own result slot.
            unsafe { user32::set_window_long_ptr(hwnd, dialog_native::DWLP_MSGRESULT, result) };
            1
        } else {
            0
        }
    }

    /// The `WM_HSCROLL` / `WM_VSCROLL` case: `lparam != 0` (the trackbar window) -> `content.on_scroll(self, get_dlg_ctrl_id(lparam), (wparam & 0xFFFF) as i32)`;
    /// the router answers `0`.
    pub(super) fn on_scroll_message(&self, wparam: Wparam, lparam: Lparam) {
        if lparam != 0 {
            self.inner.content.on_scroll(
                self,
                dialog_native::user32::get_dlg_ctrl_id(lparam),
                low_word(wparam as isize),
            );
        }
    }

    /// C# `HookListView(list)` (lines 743 to 747): `list_views.push(list)`; `set_window_subclass(list, list_view_subclass_proc, LIST_VIEW_SUBCLASS_ID,
    /// self_ref_data())`.
    pub(super) fn hook_list_view(&self, list: Hwnd) {
        self.inner.events.list_views.borrow_mut().push(list);
        // SAFETY: the subclass is removed by cleanup or WM_NCDESTROY before the dialog cookie is freed.
        let _ = unsafe {
            common_controls::set_window_subclass(
                list,
                list_view_subclass_proc,
                LIST_VIEW_SUBCLASS_ID,
                self.self_ref_data(),
            )
        };
    }

    /// C# `SetListViewSort(id, column, direction)` (lines 1209 to 1224): the sort arrow of the header of list view `id`. Absent control -> nothing. Else
    /// `sorts[list] = (column, direction)` and `invalidate_rect(ListViewControl::header(list), None, true)` when the header exists.
    pub fn set_list_view_sort(&self, id: i32, column: i32, direction: i32) {
        let list = self.item(id);
        if list == 0 {
            return;
        }
        self.inner.events.sorts.borrow_mut().insert(list, (column, direction));
        let header = ListViewControl::header(list);
        if header != 0 {
            let _ = dialog_native::user32::invalidate_rect(header, None, true);
        }
    }

    /// C# `IsButton(hwnd)` (`protected static`): class name is `Button` (push button, check box, radio, group box). `0` -> `false`. Reads at most 16 UTF-16 units.
    pub fn is_button(hwnd: Hwnd) -> bool {
        hwnd != 0
            && crate::win32::comctl::user32::get_class_name(hwnd).is_some_and(|name| class_name_is(&name, "Button"))
    }

    /// C# `IsEdit(hwnd)` (private static): class name is `Edit`. `0` -> `false`. Reads at most 8 UTF-16 units.
    #[allow(dead_code)] // W8-18k's WM_CTLCOLORSTATIC handler calls this after its stub is ported.
    pub(super) fn is_edit(hwnd: Hwnd) -> bool {
        hwnd != 0 && crate::win32::comctl::user32::get_class_name(hwnd).is_some_and(|name| class_name_is(&name, "Edit"))
    }
}

/// C# `ListViewSubclassProc` (lines 749 to 773), installed by `hook_list_view`. Runs under `pending_panic::guarded`: a panic is captured (the owner of the
/// native call resumes it) and the message goes on to `def_subclass_proc` (D-W8B5-207; the C# swallowed it: "the header is only cosmetic").
/// `WM_NOTIFY` with `lParam != 0`, `code == NM_CUSTOMDRAW`, `hwndFrom != 0 && hwndFrom == ListViewControl::header(hwnd)` and a live dialog from `ref_data`
/// (`NativeDialog::from_cookie`): `(column, direction) = sorts[hwnd]` or `(-1, 0)`, return `ListViewControl::draw_header_item(&NMCUSTOMDRAW, dialog.dx(3), column,
/// direction)`. `WM_NCDESTROY` -> `remove_window_subclass(hwnd, list_view_subclass_proc, id)`. Everything else, and the cases above that did not return:
/// `def_subclass_proc`.
///
/// # Safety
/// Only the OS calls it, as a subclass procedure of a window installed by `hook_list_view`, with `ref_data` = the cookie of a live dialog.
pub(super) unsafe extern "system" fn list_view_subclass_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    id: usize,
    ref_data: usize,
) -> Lresult {
    // SAFETY: the subclass receives a live dialog cookie; cleanup removes this procedure before freeing it.
    let handled = pending_panic::guarded(
        None,
        AssertUnwindSafe(|| {
            if message == cc::WM_NOTIFY && lparam != 0 {
                debug_assert_eq!(std::mem::size_of::<NMHDR>(), 24);
                // SAFETY: WM_NOTIFY lParam points to an NMHDR valid for this call.
                let header = unsafe { &*(lparam as *const NMHDR) };
                let from = header.hwndFrom.0 as Hwnd;
                if header.code as i32 == cc::NM_CUSTOMDRAW && from != 0 && from == ListViewControl::header(hwnd) {
                    // SAFETY: ref_data is this live dialog's cookie until the subclass is removed.
                    if let Some(dialog) = unsafe { NativeDialog::from_cookie(ref_data as isize) } {
                        debug_assert_eq!(std::mem::offset_of!(NMCUSTOMDRAW, dwDrawStage), 24);
                        debug_assert_eq!(std::mem::offset_of!(NMCUSTOMDRAW, hdc), 32);
                        // SAFETY: NM_CUSTOMDRAW lParam points to an NMCUSTOMDRAW valid for this call.
                        let draw = unsafe { &*(lparam as *const NMCUSTOMDRAW) };
                        let sort = dialog
                            .inner
                            .events
                            .sorts
                            .borrow()
                            .get(&hwnd)
                            .copied()
                            .unwrap_or((-1, 0));
                        return Some(ListViewControl::draw_header_item(draw, dialog.dx(3), sort.0, sort.1));
                    }
                }
            }
            if message == messages::WM_NCDESTROY {
                let _ = common_controls::remove_window_subclass(hwnd, list_view_subclass_proc, id);
            }
            None
        }),
    );
    if let Some(result) = handled {
        return result;
    }
    // SAFETY: forwarding the unchanged parameters from this subclass callback to the next procedure.
    unsafe { common_controls::def_subclass_proc(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::{EventsState, LIST_VIEW_SUBCLASS_ID, list_view_subclass_proc};

    #[test]
    fn event_state_starts_empty() {
        // C#: NativeDialog._listViews / _sorts initial state.
        let state = EventsState::new();
        assert!(state.list_views.borrow().is_empty());
        assert!(state.sorts.borrow().is_empty());
    }

    #[test]
    #[ignore = "native: needs desktop"]
    fn list_view_subclass_can_be_queried_and_is_removed_on_destroy() {
        // C#: NativeDialog.HookListView / ListViewSubclassProc.
        use crate::dialogs::native::controls::basic::CommonControlsInit;
        use crate::win32::api::{kernel32, user32};
        use crate::win32::comctl::common_controls;
        use crate::win32::structs::Hwnd;

        struct Window(Hwnd);
        impl Drop for Window {
            fn drop(&mut self) {
                if self.0 != 0 {
                    let _ = user32::destroy_window(self.0);
                }
            }
        }

        CommonControlsInit::ensure();
        // SAFETY: literal class and title, integer style, no creation parameter; the window is destroyed by Window.
        let list = unsafe {
            user32::create_window_ex(
                0,
                "SysListView32",
                "",
                0,
                0,
                0,
                100,
                100,
                0,
                0,
                kernel32::get_module_handle().unwrap_or(0),
                0,
            )
        }
        .expect("create test list view");
        let mut list = Window(list);
        assert!(!common_controls::remove_window_subclass(
            list.0,
            list_view_subclass_proc,
            LIST_VIEW_SUBCLASS_ID
        ));

        let dialog = super::NativeDialog::new(std::rc::Rc::new(Content));
        dialog.hook_list_view(list.0);
        assert!(common_controls::remove_window_subclass(
            list.0,
            list_view_subclass_proc,
            LIST_VIEW_SUBCLASS_ID
        ));
        dialog.hook_list_view(list.0);
        user32::destroy_window(list.0).expect("destroy test list view");
        assert!(!common_controls::remove_window_subclass(
            list.0,
            list_view_subclass_proc,
            LIST_VIEW_SUBCLASS_ID
        ));
        list.0 = 0;
    }

    #[test]
    #[ignore = "native: needs desktop"]
    fn is_edit_checks_the_window_class() {
        // C#: NativeDialog.IsEdit.
        use crate::win32::api::{kernel32, user32};
        struct Window(crate::win32::structs::Hwnd);
        impl Drop for Window {
            fn drop(&mut self) {
                if self.0 != 0 {
                    let _ = user32::destroy_window(self.0);
                }
            }
        }
        assert!(!super::NativeDialog::is_edit(0));
        for (class, expected) in [
            ("Edit", true),
            ("Button", false),
            ("Static", false),
            ("ComboBox", false),
        ] {
            // SAFETY: literal standard window class and title, integer styles, and no creation parameter.
            let window = Window(
                unsafe {
                    user32::create_window_ex(
                        0,
                        class,
                        "",
                        0,
                        0,
                        0,
                        100,
                        20,
                        0,
                        0,
                        kernel32::get_module_handle().unwrap_or(0),
                        0,
                    )
                }
                .expect("create class test window"),
            );
            assert_eq!(super::NativeDialog::is_edit(window.0), expected, "class={class}");
        }
    }

    struct Content;
    impl crate::dialogs::native::dialog::core::DialogContent for Content {
        fn title(&self) -> String {
            String::new()
        }
        fn items(&self) -> Vec<crate::dialogs::native::template::DialogItem> {
            Vec::new()
        }
        fn preferred_client_size(&self, _: &super::NativeDialog) -> (i32, i32) {
            (100, 100)
        }
        fn layout_controls(&self, _: &super::NativeDialog, _: i32, _: i32) {}
    }
}
