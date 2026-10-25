//! Port of `SettingsDialog.cs` lines 633 to 743 and 1421 (`IsKeyBox`, `CaptureKey`, `HookKeyBox`, `KeyBoxProc`, `KeyBoxColors`, `IsKeyBoxId`), baseline `d92d6aae`.
//! Unit W8-19k. Contract: `contracts/W8-19k.md`.
//!
//! The key capture boxes of the Shortcuts page are read-only edit boxes that turn a pressed key into its canonical name. The OS part (subclass, IME, colours) is native; the
//! decision "which key becomes which text" is pure: [`key_box_text`] (virtual key + extended bit -> `KeyId` -> `SettingsPresenter::capture_shortcut`), checked against the
//! golden `settings-dialog-state`.
//!
//! `unsafe` here: `set_window_subclass` / `remove_window_subclass` / `def_subclass_proc` (unsafe wrappers of `win32::comctl`), the subclass procedure [`key_box_proc`] (an `extern "system" fn` registered with `set_window_subclass`; its `ref_data` is the REGISTRY KEY of the content, never
//! a pointer, D-W8B5-908) and the `Hdc` handed to the colour answer.

use super::frame::SettingsContent;
use crate::dialogs::native::dialog::NativeDialog;
use crate::win32::structs::{Hdc, Hwnd, Lparam, Lresult, Wparam};

/// C# `WmDeadChar`.
pub const WM_DEADCHAR: u32 = 0x0103;
/// C# `WmSysDeadChar`.
pub const WM_SYSDEADCHAR: u32 = 0x0107;
/// C# `WM_IME_CHAR`.
pub const WM_IME_CHAR: u32 = 0x0286;
/// C# `WM_IME_STARTCOMPOSITION`.
pub const WM_IME_STARTCOMPOSITION: u32 = 0x010D;
/// C# `WM_IME_COMPOSITION`.
pub const WM_IME_COMPOSITION: u32 = 0x010F;
/// C# `WM_PASTE`.
pub const WM_PASTE: u32 = 0x0302;
/// C# `WM_CUT`.
pub const WM_CUT: u32 = 0x0300;
/// C# `WM_CLEAR`.
pub const WM_CLEAR: u32 = 0x0303;
/// C# `WM_UNDO`.
pub const WM_UNDO: u32 = 0x0304;
/// The extended-key bit of a key message's `lParam` (bit 24): the right Ctrl/Alt, the numpad Enter, the arrow cluster.
pub const EXTENDED_KEY_BIT: isize = 0x0100_0000;

/// Does the key box swallow this message (answer `0`, nothing reaches the box)? `true` for `WM_CHAR`, `WM_SYSCHAR`, `WM_DEADCHAR`, `WM_SYSDEADCHAR`, `WM_IME_CHAR`,
/// `WM_IME_STARTCOMPOSITION`, `WM_IME_COMPOSITION`, `WM_PASTE`, `WM_CUT`, `WM_CLEAR`, `WM_UNDO` (so no text ever reaches the box except through `capture_key`);
/// `false` for everything else, in particular `WM_COPY` (copying from the read-only box is allowed). Pure.
pub fn is_swallowed_message(message: u32) -> bool {
    matches!(message,0x0102|0x0106|WM_DEADCHAR|WM_SYSDEADCHAR|WM_IME_CHAR|WM_IME_STARTCOMPOSITION|WM_IME_COMPOSITION|WM_PASTE|WM_CUT|WM_CLEAR|WM_UNDO)
}

/// The virtual key a key message means: `wparam & 0xFFFF`; when that is `VK_PROCESSKEY` (229) an IME swallowed the key and `imm_virtual_key()` (called ONLY then) is the real
/// one, used when it is not `0` (on Windows 11 the OS answers 229 itself when no key is being processed; the result then stays 229, which maps to a reserved key). Pure.
pub fn resolve_virtual_key(wparam: Wparam, imm_virtual_key: impl FnOnce() -> u32) -> i32 {
    let key=(wparam & 0xffff) as i32;
    if key==229 {let real=imm_virtual_key();if real!=0{return real as i32;}}
    key
}

/// The text a key box shows after this key, `None` = the key is not swallowed (Tab, Esc, modifiers, IME and dead-char markers keep focus movement and Esc-cancel).
/// `KeyId = key_mapping::from_virtual_key(virtual_key, extended)`; `SettingsPresenter::capture_shortcut(key, key.name())`. Pure; the golden `settings-dialog-state` kind
/// `key_capture` pins it for every virtual key 0..=255 with and without the extended bit.
pub fn key_box_text(virtual_key: i32, extended: bool) -> Option<String> {
    let key=pr_app::input::key_mapping::from_virtual_key(virtual_key,extended);
    pr_app::presenters::settings::SettingsPresenter::capture_shortcut(key,key.name())
}

impl SettingsContent {
    /// C# `IsKeyBox(hwnd)`: `false` for `0` or when no key box is declared; else `id = get_dlg_ctrl_id(hwnd)`, `id != 0 && key_boxes.contains(id) && dialog.item(id) == hwnd`.
    pub fn is_key_box(&self, dialog: &NativeDialog, hwnd: Hwnd) -> bool {
        if hwnd==0 || self.declared.key_boxes.is_empty(){return false;}
        let id=crate::win32::dialog_native::user32::get_dlg_ctrl_id(hwnd);
        id!=0 && self.is_key_box_id(id) && dialog.item(id)==hwnd
    }

    /// C# `IsKeyBoxId(id)`: `key_boxes.contains(id)` (no window needed).
    pub fn is_key_box_id(&self, id: i32) -> bool {
        self.declared.key_boxes.contains(&id)
    }

    /// C# `CaptureKey(hwnd, wParam, lParam)`: `id = get_dlg_ctrl_id(hwnd)`; `id == 0` or not a key box id -> `false`. `virtual_key = resolve_virtual_key(wparam, || imm_get_virtual_key(hwnd))`,
    /// `text = key_box_text(virtual_key, lparam & EXTENDED_KEY_BIT != 0)`; `None` -> `false`. Else `dialog.set_item_text(id, text)` (which makes the OS send `EN_CHANGE`
    /// to the dialog synchronously: the page's `on_command` runs as one edit), `send_message(hwnd, EM_SETSEL, 0, -1)` (select all), `true`.
    pub fn capture_key(&self, dialog: &NativeDialog, hwnd: Hwnd, wparam: Wparam, lparam: Lparam) -> bool {
        let id=crate::win32::dialog_native::user32::get_dlg_ctrl_id(hwnd);
        if id==0 || !self.is_key_box_id(id){return false;}
        let virtual_key=resolve_virtual_key(wparam,||crate::win32::key_capture::imm_get_virtual_key(hwnd));
        let Some(text)=key_box_text(virtual_key,lparam & EXTENDED_KEY_BIT!=0) else{return false;};
        dialog.set_item_text(id,&text);
        // SAFETY: EM_SETSEL carries only integer selection bounds to an OS-validated control handle.
        unsafe {crate::win32::dialog_native::user32::send_message(hwnd,0x00b1,0,-1);}
        true
    }

    /// C# `HookKeyBox(box)`: `box == 0` -> nothing. `imm_associate_context(box, 0)` (no IME context: Telex can neither compose into the box nor swallow the key), then
    /// `set_window_subclass(box, key_box_proc, KEY_BOX_SUBCLASS_ID, self.subclass_key.get())`.
    pub(super) fn hook_key_box(&self, _dialog: &NativeDialog, key_box: Hwnd) {
        if key_box==0{return;}
        crate::win32::key_capture::imm_associate_context(key_box,0);
        // SAFETY: this static procedure has the SUBCLASSPROC signature; its registry key identifies this UI-thread content and is removed on WM_NCDESTROY.
        unsafe {crate::win32::comctl::common_controls::set_window_subclass(key_box,key_box_proc,super::frame::KEY_BOX_SUBCLASS_ID,self.subclass_key.get());}
    }

    /// C# `KeyBoxColors(hdc)`: `set_text_color(hdc, DialogColors::text_strong())`, `set_bk_color(hdc, DialogColors::input())`, and the brush of `DialogColors::input()` (created
    /// on first use, kept in `key_box_brush`, deleted by `close::on_closed`); the brush handle is the `WM_CTLCOLORSTATIC` answer.
    pub(super) fn key_box_colors(&self, hdc: Hdc) -> Lresult {
        use crate::dialogs::native::chrome::DialogColors;
        use crate::win32::dialog_native::gdi32;
        gdi32::set_text_color(hdc,DialogColors::text_strong());
        gdi32::set_bk_color(hdc,DialogColors::input());
        if self.view.key_box_brush.get()==0 {self.view.key_box_brush.set(gdi32::create_solid_brush(DialogColors::input()));}
        self.view.key_box_brush.get()
    }
}

/// C# `KeyBoxProc` (`[UnmanagedCallersOnly]`): `WM_KEYDOWN` / `WM_SYSKEYDOWN` -> the content found by `from_subclass_key(ref_data)`; when `capture_key` answers `true` the message ends
/// here (`0`); a swallowed message (see [`is_swallowed_message`]) ends here with `0`; `WM_NCDESTROY` removes the subclass; everything else (and a missing content) goes to
/// `def_subclass_proc`. The body runs under `pending_panic::guarded` (D-W8B5-314 style): a panic is captured, the message still goes to `def_subclass_proc`. A content whose
/// window is gone (`with_native` answers `None`) is a missing content: the message goes on.
///
/// # Safety
/// Only the OS calls it, as a `SUBCLASSPROC` installed by `hook_key_box`, with the arguments of the window message.
pub(super) unsafe extern "system" fn key_box_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    id: usize,
    ref_data: usize,
) -> Lresult {
    let handled=crate::host::pending_panic::guarded(None,std::panic::AssertUnwindSafe(||{
        if message==crate::win32::messages::WM_NCDESTROY {
            crate::win32::comctl::common_controls::remove_window_subclass(hwnd,key_box_proc,id);
            return None;
        }
        let content=SettingsContent::from_subclass_key(ref_data)?;
        content.with_native(|dialog|{
            if matches!(message,0x0100|0x0104) && content.capture_key(dialog,hwnd,wparam,lparam){Some(0)}
            else if is_swallowed_message(message){Some(0)}else{None}
        }).flatten()
    }));
    if let Some(result)=handled{return result;}
    // SAFETY: installed as a SUBCLASSPROC; unhandled messages and panic fallbacks retain the OS's unchanged message arguments.
    unsafe {crate::win32::comctl::common_controls::def_subclass_proc(hwnd,message,wparam,lparam)}
}
