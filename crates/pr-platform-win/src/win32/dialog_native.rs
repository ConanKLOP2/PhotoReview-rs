//! Port of `PhotoReview.Shell.Interop/Win32/DialogNative.cs` (baseline `d92d6aae`, 223 lines): the user32/gdi32/kernel32/uxtheme calls and the
//! `DialogConstants` the native Win32 dialogs (`Shell.Win32/Dialogs/Native`) need: modeless dialog creation from an in-memory template, dialog message routing,
//! window text, `SendMessage`, device contexts, text measurement, fonts and the stock non-client metrics. The hand-drawn dark chrome is `dialog_chrome.rs`.
//!
//! Unit W8-11d. Same wrapper rules as `api.rs` (D-W8B3-4): `isize` handles, `Result<_, Win32Error>` where the C# declaration is `SetLastError = true`, `unsafe fn`
//! where an integer argument is interpreted as a pointer by the callee (`send_message`, `create_dialog_indirect_param`, `is_dialog_message`).
#![allow(unused_variables, dead_code)] // stub: removed by the W8-11d porter when the last `todo!()` is gone.

use super::structs::{
    Hdc, Hgdi, Hinstance, Hresult, Hwnd, Lparam, Lresult, Msg, Point, Rect, Win32Error, WndProc, Wparam,
};

pub const WM_SETFONT: u32 = 0x0030; // WmSetFont
pub const WM_GETFONT: u32 = 0x0031; // WmGetFont
pub const WM_INITDIALOG: u32 = 0x0110; // WmInitDialog
pub const WM_CTLCOLORMSGBOX: u32 = 0x0132; // WmCtlColorMsgBox
pub const WM_CTLCOLOREDIT: u32 = 0x0133; // WmCtlColorEdit
pub const WM_CTLCOLORLISTBOX: u32 = 0x0134; // WmCtlColorListBox
pub const WM_CTLCOLORBTN: u32 = 0x0135; // WmCtlColorBtn
pub const WM_CTLCOLORDLG: u32 = 0x0136; // WmCtlColorDlg
pub const WM_CTLCOLORSCROLLBAR: u32 = 0x0137; // WmCtlColorScrollBar
pub const WM_CTLCOLORSTATIC: u32 = 0x0138; // WmCtlColorStatic
pub const DM_GETDEFID: u32 = 0x0400; // DmGetDefId
pub const DM_SETDEFID: u32 = 0x0401; // DmSetDefId
pub const BM_SETSTYLE: u32 = 0x00F4; // BmSetStyle
pub const EM_SETSEL: u32 = 0x00B1; // EmSetSel
pub const EM_GETSEL: u32 = 0x00B0; // EmGetSel
pub const EM_SETLIMITTEXT: u32 = 0x00C5; // EmSetLimitText
pub const EM_GETRECT: u32 = 0x00B2; // EmGetRect
pub const EM_SETRECT: u32 = 0x00B3; // EmSetRect
pub const ES_MULTILINE: u32 = 4; // EsMultiline
pub const LB_ADDSTRING: u32 = 0x0180; // LbAddString
pub const LB_RESETCONTENT: u32 = 0x0184; // LbResetContent
pub const LB_SETCURSEL: u32 = 0x0186; // LbSetCurSel
pub const LB_GETCOUNT: u32 = 0x018B; // LbGetCount
pub const LB_GETTEXTLEN: u32 = 0x018A; // LbGetTextLen
pub const LB_GETTEXT: u32 = 0x0189; // LbGetText
pub const LB_SETHORIZONTALEXTENT: u32 = 0x0194; // LbSetHorizontalExtent
pub const LB_INITSTORAGE: u32 = 0x01A8; // LbInitStorage
pub const BN_CLICKED: u32 = 0; // BnClicked
pub const DWLP_MSGRESULT: i32 = 0; // DwlpMsgResult
pub const DWLP_USER: i32 = 16; // DwlpUser
pub const WS_TABSTOP: u32 = 0x00010000; // WsTabStop
pub const WS_GROUP: u32 = 0x00020000; // WsGroup
pub const WS_BORDER: u32 = 0x00800000; // WsBorder
pub const WS_VSCROLL: u32 = 0x00200000; // WsVScroll
pub const WS_HSCROLL: u32 = 0x00100000; // WsHScroll
pub const WS_DISABLED: u32 = 0x08000000; // WsDisabled
pub const WS_DLGFRAME: u32 = 0x00400000; // WsDlgFrame
pub const WS_EX_DLGMODALFRAME: u32 = 1; // WsExDlgModalFrame
pub const WS_EX_CLIENTEDGE: u32 = 0x0200; // WsExClientEdge
pub const WS_EX_CONTROLPARENT: u32 = 0x00010000; // WsExControlParent
pub const BS_PUSHBUTTON: u32 = 0; // BsPushButton
pub const BS_DEFPUSHBUTTON: u32 = 1; // BsDefPushButton
pub const ES_AUTOHSCROLL: u32 = 0x0080; // EsAutoHScroll
pub const SS_LEFT: u32 = 0; // SsLeft
pub const SS_NOPREFIX: u32 = 0x0080; // SsNoPrefix
pub const LBS_NOTIFY: u32 = 1; // LbsNotify
pub const LBS_NOINTEGRALHEIGHT: u32 = 0x0100; // LbsNoIntegralHeight
pub const DS_MODALFRAME: u32 = 0x0080; // DsModalFrame
pub const DS_SETFONT: u32 = 0x0040; // DsSetFont
pub const DS_NOFAILCREATE: u32 = 0x0010; // DsNoFailCreate
pub const DT_CENTER: u32 = 1; // DtCenter
pub const DT_VCENTER: u32 = 4; // DtVCenter
pub const DT_END_ELLIPSIS: u32 = 0x8000; // DtEndEllipsis
pub const DT_WORDBREAK: u32 = 0x0010; // DtWordBreak
pub const DT_SINGLELINE: u32 = 0x0020; // DtSingleLine
pub const DT_CALCRECT: u32 = 0x0400; // DtCalcRect
pub const DT_NOPREFIX: u32 = 0x0800; // DtNoPrefix
pub const DT_EDITCONTROL: u32 = 0x2000; // DtEditControl
pub const SPI_GETNONCLIENTMETRICS: u32 = 0x0029; // SpiGetNonClientMetrics
pub const NONCLIENTMETRICSW_SIZE: i32 = 504; // NonClientMetricsSize
pub const NONCLIENTMETRICSW_LFMESSAGEFONT_OFFSET: i32 = 408; // NonClientMessageFontOffset
pub const LOGFONTW_SIZE: i32 = 92; // LogFontSize
pub const GR_GDIOBJECTS: i32 = 0; // GrGdiObjects
pub const GR_USEROBJECTS: i32 = 1; // GrUserObjects
pub const TRANSPARENT: i32 = 1; // TransparentBackground

/// `DT_MODIFYSTRING`: makes `DrawText` write the shortened text back into the string. Not in the C# constants; `user32::draw_text` removes it (D-W8B3-9).
pub const DT_MODIFYSTRING: u32 = 0x0001_0000;

/// `format` without `DT_MODIFYSTRING`: what `user32::draw_text` passes to the OS (pure, so it can be tested without a DC).
pub fn draw_text_format_without_modify_string(format: u32) -> u32 {
    todo!()
}

/// `user32.dll` additions of `DialogNative.cs` (the `User32` partial).
pub mod user32 {
    use super::*;

    /// C# `CreateDialogIndirectParam(instance, byte* template, parent, dialogProc, initParam)`: a modeless dialog from a `DLGTEMPLATE`/`DLGTEMPLATEEX` in memory;
    /// `WM_INITDIALOG` receives `init_param`. The wrapper copies `template` into a 4-byte aligned buffer first (the C# caller pins a `byte[]`, whose start is aligned;
    /// a Rust `&[u8]` is not). `Err` when the handle is `0`.
    ///
    /// # Safety
    /// `template` is a complete, well-formed dialog template (the OS parses the item count it declares, a truncated one reads past the slice), and `dialog_proc`
    /// is a valid dialog procedure that does not unwind.
    pub unsafe fn create_dialog_indirect_param(
        instance: Hinstance,
        template: &[u8],
        parent: Hwnd,
        dialog_proc: WndProc,
        init_param: isize,
    ) -> Result<Hwnd, Win32Error> {
        todo!()
    }

    /// C# `IsDialogMessage(dialog, Msg*)`: `true` when the message was processed for the dialog (do not translate or dispatch it).
    ///
    /// # Safety
    /// `msg` comes from the thread's message queue (`get_message`/`peek_message`): the dialog and its controls run window procedures with its parameters.
    pub unsafe fn is_dialog_message(dialog: Hwnd, msg: &mut Msg) -> bool {
        todo!()
    }

    /// C# `EnableWindow`: `true` when the window was disabled before.
    pub fn enable_window(hwnd: Hwnd, enable: bool) -> bool {
        todo!()
    }

    /// C# `IsWindowEnabled`.
    pub fn is_window_enabled(hwnd: Hwnd) -> bool {
        todo!()
    }

    /// C# `IsWindow`.
    pub fn is_window(hwnd: Hwnd) -> bool {
        todo!()
    }

    /// C# `GetDlgItem(dialog, id)`: `0` when there is no such child.
    pub fn get_dlg_item(dialog: Hwnd, id: i32) -> Hwnd {
        todo!()
    }

    /// C# `SetWindowText`.
    pub fn set_window_text(hwnd: Hwnd, text: &str) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `GetWindowTextLength`: UTF-16 units without the NUL.
    pub fn get_window_text_length(hwnd: Hwnd) -> i32 {
        todo!()
    }

    /// C# `GetWindowText(hwnd, char* buffer, maxCount)` for the C# idiom "length, then a buffer of length + 1": the whole text (`""` for none). The text can change
    /// between the two calls; a longer text is cut to the buffer, never overrun.
    pub fn get_window_text(hwnd: Hwnd) -> String {
        todo!()
    }

    /// C# `SendMessage` (102 call sites in the C# shell).
    ///
    /// # Safety
    /// For messages that carry pointers (`WM_GETTEXT`, `LVM_INSERTITEM`, `CB_GETLBTEXT`, `TTM_ADDTOOL`, ...) `wparam`/`lparam` point to memory of the exact
    /// size and type the message defines, valid for the (synchronous) call, and the target window procedure may run arbitrary code.
    pub unsafe fn send_message(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        todo!()
    }

    /// C# `MoveWindow(hwnd, x, y, width, height, repaint)`.
    pub fn move_window(hwnd: Hwnd, x: i32, y: i32, width: i32, height: i32, repaint: bool) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `SetFocus`: the previous focus window.
    pub fn set_focus(hwnd: Hwnd) -> Hwnd {
        todo!()
    }

    /// C# `GetFocus`.
    pub fn get_focus() -> Hwnd {
        todo!()
    }

    /// C# `GetDC`: release it with `release_dc`.
    pub fn get_dc(hwnd: Hwnd) -> Hdc {
        todo!()
    }

    /// C# `ReleaseDC`: `1` when released.
    pub fn release_dc(hwnd: Hwnd, hdc: Hdc) -> i32 {
        todo!()
    }

    /// C# `DrawText(hdc, char* text, count, Rect*, format)`: the height of the text drawn; with `DT_CALCRECT` `rect` receives the needed size. The wrapper passes
    /// the length as `count` (no NUL needed) and clears `DT_MODIFYSTRING` (0x10000) from `format`, because the text is read-only (D-W8B3-9).
    pub fn draw_text(hdc: Hdc, text: &str, rect: &mut Rect, format: u32) -> i32 {
        todo!()
    }

    /// C# `SystemParametersInfoForDpi(action, param, void* data, winIni, dpi)`. The only C# use is `SPI_GETNONCLIENTMETRICS`: see
    /// `non_client_metrics_for_dpi`.
    ///
    /// # Safety
    /// `data` points to the structure `action` defines (for `SPI_GETNONCLIENTMETRICS` a `NONCLIENTMETRICSW` with `cbSize` set), valid for the call.
    pub unsafe fn system_parameters_info_for_dpi(
        action: u32,
        param: u32,
        data: *mut core::ffi::c_void,
        win_ini: u32,
        dpi: u32,
    ) -> Result<(), Win32Error> {
        todo!()
    }

    /// The C# use of `SystemParametersInfoForDpi`: `SPI_GETNONCLIENTMETRICS` into a `NONCLIENTMETRICSW` whose `cbSize` is `NONCLIENTMETRICSW_SIZE` (504); the
    /// dialog font is `lfMessageFont` (offset 408, `NONCLIENTMETRICSW_LFMESSAGEFONT_OFFSET`).
    pub fn non_client_metrics_for_dpi(
        dpi: u32,
    ) -> Result<windows::Win32::UI::WindowsAndMessaging::NONCLIENTMETRICSW, Win32Error> {
        todo!()
    }

    /// C# `ScreenToClient(hwnd, Point*)`.
    pub fn screen_to_client(hwnd: Hwnd, point: &mut Point) -> bool {
        todo!()
    }

    /// C# `GetGuiResources(process, flags)` (`GR_GDIOBJECTS` = 0, `GR_USEROBJECTS` = 1): the handle count of the process (the leak tests).
    pub fn get_gui_resources(process: isize, flags: u32) -> u32 {
        todo!()
    }

    /// C# `GetWindow(hwnd, command)` (`GW_*`).
    pub fn get_window(hwnd: Hwnd, command: u32) -> Hwnd {
        todo!()
    }

    /// C# `GetDlgCtrlID`: `0` on failure.
    pub fn get_dlg_ctrl_id(hwnd: Hwnd) -> i32 {
        todo!()
    }

    /// C# `InvalidateRect(hwnd, Rect*, erase)`; `None` invalidates the whole client area.
    pub fn invalidate_rect(hwnd: Hwnd, rect: Option<&Rect>, erase: bool) -> bool {
        todo!()
    }
}

/// `gdi32.dll` additions of `DialogNative.cs` (the `Gdi32` partial).
pub mod gdi32 {
    use super::*;

    /// C# `CreateSolidBrush(colorRef)`: `0` on failure.
    pub fn create_solid_brush(color_ref: u32) -> Hgdi {
        todo!()
    }

    /// C# `DeleteObject`: `false` when the object is selected into a DC or invalid.
    pub fn delete_object(object: Hgdi) -> bool {
        todo!()
    }

    /// C# `SetTextColor`: the previous colour (`CLR_INVALID` = `0xFFFF_FFFF` on failure).
    pub fn set_text_color(hdc: Hdc, color_ref: u32) -> u32 {
        todo!()
    }

    /// C# `SetBkColor`.
    pub fn set_bk_color(hdc: Hdc, color_ref: u32) -> u32 {
        todo!()
    }

    /// C# `GetTextColor`.
    pub fn get_text_color(hdc: Hdc) -> u32 {
        todo!()
    }

    /// C# `GetBkColor`.
    pub fn get_bk_color(hdc: Hdc) -> u32 {
        todo!()
    }

    /// C# `SelectObject`: the previous object of that kind (`0` on failure). The caller reselects the old object before deleting a GDI object it created.
    pub fn select_object(hdc: Hdc, object: Hgdi) -> Hgdi {
        todo!()
    }

    /// C# `GetTextExtentPoint32(hdc, char* text, count, Point* size)`: the size as a `Point` (`x` = cx, `y` = cy), `None` on failure.
    pub fn get_text_extent_point32(hdc: Hdc, text: &str) -> Option<Point> {
        todo!()
    }

    /// C# `GetTextMetrics(hdc, void* metrics)` with a `TEXTMETRICW` (60 bytes; the C# code reads the first five `LONG`s).
    pub fn get_text_metrics(hdc: Hdc) -> Option<windows::Win32::Graphics::Gdi::TEXTMETRICW> {
        todo!()
    }

    /// C# `CreateFontIndirect(void* logFont)` with a `LOGFONTW` (92 bytes): `0` on failure.
    pub fn create_font_indirect(log_font: &windows::Win32::Graphics::Gdi::LOGFONTW) -> Hgdi {
        todo!()
    }

    /// C# `GetObject(handle, size, void* buffer)`: the number of bytes written (`0` on failure). The OS writes at most `buffer.len()` bytes.
    pub fn get_object(handle: Hgdi, buffer: &mut [u8]) -> i32 {
        todo!()
    }

    /// `get_object` for a font handle into a `LOGFONTW` (the 92 byte C# use): `None` when fewer than 92 bytes were written.
    pub fn get_log_font(font: Hgdi) -> Option<windows::Win32::Graphics::Gdi::LOGFONTW> {
        todo!()
    }
}

/// `kernel32.dll` addition of `DialogNative.cs` (the `Kernel32` partial).
pub mod kernel32 {
    /// C# `GetCurrentProcess`: the pseudo handle `-1` (needs no `CloseHandle`).
    pub fn get_current_process() -> isize {
        todo!()
    }
}

/// `uxtheme.dll` (C# `Uxtheme`).
pub mod uxtheme {
    use super::*;

    /// C# `SetWindowTheme(hwnd, subAppName, subIdList)`: `("DarkMode_Explorer", None)`, `("DarkMode_CFD", None)`..., `(Some(""), Some(""))` removes the theme.
    pub fn set_window_theme(hwnd: Hwnd, sub_app_name: Option<&str>, sub_id_list: Option<&str>) -> Result<(), Hresult> {
        todo!()
    }
}
