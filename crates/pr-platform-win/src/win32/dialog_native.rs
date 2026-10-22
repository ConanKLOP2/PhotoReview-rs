//! Port of `PhotoReview.Shell.Interop/Win32/DialogNative.cs` (baseline `d92d6aae`, 223 lines): the user32/gdi32/kernel32/uxtheme calls and the
//! `DialogConstants` the native Win32 dialogs (`Shell.Win32/Dialogs/Native`) need: modeless dialog creation from an in-memory template, dialog message routing,
//! window text, `SendMessage`, device contexts, text measurement, fonts and the stock non-client metrics. The hand-drawn dark chrome is `dialog_chrome.rs`.
//!
//! Unit W8-11d. Same wrapper rules as `api.rs` (D-W8B3-4): `isize` handles, `Result<_, Win32Error>` where the C# declaration is `SetLastError = true`, `unsafe fn`
//! where an integer argument is interpreted as a pointer by the callee (`send_message`, `create_dialog_indirect_param`, `is_dialog_message`).

use core::ffi::c_void;

use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND};
use windows::Win32::Graphics::Gdi::{HDC, HGDIOBJ};

use super::structs::{
    Hdc, Hgdi, Hinstance, Hresult, Hwnd, Lparam, Lresult, Msg, Point, Rect, Win32Error, WndProc, Wparam, to_utf16z,
    utf16_to_string,
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
pub const EN_CHANGE: i32 = 0x0300; // EnChange
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
    format & !DT_MODIFYSTRING
}

/// `isize` handle to `HWND` (a handle value only; nothing is dereferenced).
pub(super) fn to_hwnd(handle: Hwnd) -> HWND {
    HWND(handle as *mut c_void)
}

/// `isize` handle to `HDC`.
pub(super) fn to_hdc(handle: Hdc) -> HDC {
    HDC(handle as *mut c_void)
}

/// `isize` handle to `HGDIOBJ`.
pub(super) fn to_hgdiobj(handle: Hgdi) -> HGDIOBJ {
    HGDIOBJ(handle as *mut c_void)
}

/// The Win32 error code of a `windows` error: the low 16 bits of an `HRESULT_FROM_WIN32` value (what `GetLastError` returned), else the raw code.
fn win32_error(error: &windows::core::Error) -> Win32Error {
    let code = error.code().0 as u32;
    if code >> 16 == 0x8007 {
        Win32Error(code & 0xFFFF)
    } else {
        Win32Error(code)
    }
}

/// `user32.dll` additions of `DialogNative.cs` (the `User32` partial).
pub mod user32 {
    use super::*;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        DRAW_TEXT_FORMAT, DrawTextW, GetDC, InvalidateRect, ReleaseDC, ScreenToClient,
    };
    use windows::Win32::System::Threading::{GET_GUI_RESOURCES_FLAGS, GetGuiResources};
    use windows::Win32::UI::HiDpi::SystemParametersInfoForDpi;
    use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetFocus, IsWindowEnabled, SetFocus};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateDialogIndirectParamW, DLGTEMPLATE, GET_WINDOW_CMD, GetDlgCtrlID, GetDlgItem, GetWindow,
        GetWindowTextLengthW, GetWindowTextW, IsDialogMessageW, IsWindow, MoveWindow, NONCLIENTMETRICSW, SendMessageW,
        SetWindowTextW,
    };
    use windows::core::PCWSTR;

    /// The dialog procedure type of the OS (`DLGPROC` without the `Option`).
    type NativeDlgProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> isize;

    /// Smallest possible `DLGTEMPLATE` header (style, extended style, item count, x, y, cx, cy): anything shorter cannot be a template.
    const DLGTEMPLATE_HEADER_SIZE: usize = 18;

    /// C# `CreateDialogIndirectParam(instance, byte* template, parent, dialogProc, initParam)`: a modeless dialog from a `DLGTEMPLATE`/`DLGTEMPLATEEX` in memory;
    /// `WM_INITDIALOG` receives `init_param`. The wrapper copies `template` into a 4-byte aligned buffer first (the C# caller pins a `byte[]`, whose start is aligned;
    /// a Rust `&[u8]` is not). `Err` when the handle is `0`, and `Err(ERROR_INVALID_PARAMETER)` without calling the OS when `template` is shorter than the
    /// 18-byte `DLGTEMPLATE` header.
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
        if template.len() < DLGTEMPLATE_HEADER_SIZE {
            return Err(Win32Error(87)); // ERROR_INVALID_PARAMETER
        }
        // A `Vec<u32>` is 4-aligned whatever the alignment of the caller's bytes (trap T4); the template is little-endian like every Windows target.
        let mut words = vec![0u32; template.len().div_ceil(4)];
        for (word, chunk) in words.iter_mut().zip(template.chunks(4)) {
            let mut bytes = [0u8; 4];
            if let Some(head) = bytes.get_mut(..chunk.len()) {
                head.copy_from_slice(chunk);
            }
            *word = u32::from_le_bytes(bytes);
        }
        // SAFETY: `WndProc` and `DLGPROC` have the same `extern "system"` ABI: `Hwnd` is a pointer-sized integer where `HWND` is a pointer, `Wparam`/`Lparam`/`Lresult`
        // have the size of `WPARAM`/`LPARAM`/`INT_PTR`.
        let native = unsafe { core::mem::transmute::<WndProc, NativeDlgProc>(dialog_proc) };
        // SAFETY: `words` is a `Vec<u32>` that owns a copy of the template, 4-aligned, alive for the call; the template is well-formed and `dialog_proc` is valid
        // (`# Safety`, upheld by the caller of this function). The OS does not keep the template pointer after the call.
        let created = unsafe {
            CreateDialogIndirectParamW(
                Some(HINSTANCE(instance as *mut c_void)),
                words.as_ptr().cast::<DLGTEMPLATE>(),
                Some(to_hwnd(parent)),
                Some(native),
                LPARAM(init_param),
            )
        };
        created.map(|h| h.0 as isize).map_err(|e| win32_error(&e))
    }

    /// C# `IsDialogMessage(dialog, Msg*)`: `true` when the message was processed for the dialog (do not translate or dispatch it).
    ///
    /// # Safety
    /// `msg` comes from the thread's message queue (`get_message`/`peek_message`): the dialog and its controls run window procedures with its parameters.
    pub unsafe fn is_dialog_message(dialog: Hwnd, msg: &mut Msg) -> bool {
        // SAFETY: `msg` is a live `MSG` that the OS reads; its parameters are valid for the window procedures it reaches (upheld by the caller of this function).
        unsafe { IsDialogMessageW(to_hwnd(dialog), msg) }.as_bool()
    }

    /// C# `EnableWindow`: `true` when the window was disabled before.
    pub fn enable_window(hwnd: Hwnd, enable: bool) -> bool {
        // SAFETY: a handle value and a flag; the OS validates the handle.
        unsafe { EnableWindow(to_hwnd(hwnd), enable) }.as_bool()
    }

    /// C# `IsWindowEnabled`.
    pub fn is_window_enabled(hwnd: Hwnd) -> bool {
        // SAFETY: a handle value; the OS validates it.
        unsafe { IsWindowEnabled(to_hwnd(hwnd)) }.as_bool()
    }

    /// C# `IsWindow`.
    pub fn is_window(hwnd: Hwnd) -> bool {
        // SAFETY: a handle value; the OS validates it.
        unsafe { IsWindow(Some(to_hwnd(hwnd))) }.as_bool()
    }

    /// C# `GetDlgItem(dialog, id)`: `0` when there is no such child.
    pub fn get_dlg_item(dialog: Hwnd, id: i32) -> Hwnd {
        // SAFETY: a handle value and an id; the OS validates the handle and returns NULL (an `Err` here) for an unknown id.
        unsafe { GetDlgItem(Some(to_hwnd(dialog)), id) }
            .map(|h| h.0 as isize)
            .unwrap_or(0)
    }

    /// C# `SetWindowText`.
    pub fn set_window_text(hwnd: Hwnd, text: &str) -> Result<(), Win32Error> {
        let text = to_utf16z(text);
        // SAFETY: `text` is a NUL-terminated UTF-16 buffer in a local that lives past the call; the OS copies it.
        unsafe { SetWindowTextW(to_hwnd(hwnd), PCWSTR(text.as_ptr())) }.map_err(|e| win32_error(&e))
    }

    /// C# `GetWindowTextLength`: UTF-16 units without the NUL.
    pub fn get_window_text_length(hwnd: Hwnd) -> i32 {
        // SAFETY: a handle value; the OS validates it.
        unsafe { GetWindowTextLengthW(to_hwnd(hwnd)) }
    }

    /// C# `GetWindowText(hwnd, char* buffer, maxCount)` for the C# idiom "length, then a buffer of length + 1": the whole text (`""` for none). The text can change
    /// between the two calls; a longer text is cut to the buffer, never overrun.
    pub fn get_window_text(hwnd: Hwnd) -> String {
        let length = usize::try_from(get_window_text_length(hwnd)).unwrap_or(0);
        let mut buffer = vec![0u16; length.saturating_add(1)];
        // SAFETY: the buffer is `len + 1` units and that is the size passed (the slice length); the OS writes at most that many units, NUL included.
        let copied = unsafe { GetWindowTextW(to_hwnd(hwnd), &mut buffer) };
        let copied = usize::try_from(copied).unwrap_or(0).min(buffer.len());
        utf16_to_string(buffer.get(..copied).unwrap_or(&[]))
    }

    /// C# `SendMessage` (102 call sites in the C# shell).
    ///
    /// # Safety
    /// For messages that carry pointers (`WM_GETTEXT`, `LVM_INSERTITEM`, `CB_GETLBTEXT`, `TTM_ADDTOOL`, ...) `wparam`/`lparam` point to memory of the exact
    /// size and type the message defines, valid for the (synchronous) call, and the target window procedure may run arbitrary code.
    pub unsafe fn send_message(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        // SAFETY: upheld by the caller of this function (pointers in the parameters match the message and are valid for the synchronous call).
        unsafe { SendMessageW(to_hwnd(hwnd), message, Some(WPARAM(wparam)), Some(LPARAM(lparam))) }.0
    }

    /// C# `MoveWindow(hwnd, x, y, width, height, repaint)`.
    pub fn move_window(hwnd: Hwnd, x: i32, y: i32, width: i32, height: i32, repaint: bool) -> Result<(), Win32Error> {
        // SAFETY: a handle value and integers; the OS validates the handle.
        unsafe { MoveWindow(to_hwnd(hwnd), x, y, width, height, repaint) }.map_err(|e| win32_error(&e))
    }

    /// C# `SetFocus`: the previous focus window.
    pub fn set_focus(hwnd: Hwnd) -> Hwnd {
        // SAFETY: a handle value; the OS validates it (a NULL previous focus is an `Err` here, `0` like the C# result).
        unsafe { SetFocus(Some(to_hwnd(hwnd))) }
            .map(|h| h.0 as isize)
            .unwrap_or(0)
    }

    /// C# `GetFocus`.
    pub fn get_focus() -> Hwnd {
        // SAFETY: no arguments.
        unsafe { GetFocus() }.0 as isize
    }

    /// C# `GetDC`: release it with `release_dc`.
    pub fn get_dc(hwnd: Hwnd) -> Hdc {
        // SAFETY: a handle value (`0` = the whole screen); the OS validates it.
        unsafe { GetDC(Some(to_hwnd(hwnd))) }.0 as isize
    }

    /// C# `ReleaseDC`: `1` when released.
    pub fn release_dc(hwnd: Hwnd, hdc: Hdc) -> i32 {
        // SAFETY: handle values; the OS validates both.
        unsafe { ReleaseDC(Some(to_hwnd(hwnd)), to_hdc(hdc)) }
    }

    /// C# `DrawText(hdc, char* text, count, Rect*, format)`: the height of the text drawn; with `DT_CALCRECT` `rect` receives the needed size. The wrapper passes
    /// the length as `count` (no NUL needed) and clears `DT_MODIFYSTRING` (0x10000) from `format`, because the text is read-only (D-W8B3-9).
    pub fn draw_text(hdc: Hdc, text: &str, rect: &mut Rect, format: u32) -> i32 {
        let mut units: Vec<u16> = text.encode_utf16().collect();
        if i32::try_from(units.len()).is_err() {
            return 0;
        }
        let format = DRAW_TEXT_FORMAT(draw_text_format_without_modify_string(format));
        // SAFETY: `units` is a local `Vec<u16>` of exactly `count` units (the slice length), `rect` is a live `RECT`, `format` has `DT_MODIFYSTRING` cleared so the OS
        // never writes to `units`.
        unsafe { DrawTextW(to_hdc(hdc), &mut units, rect, format) }
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
        let data = if data.is_null() { None } else { Some(data) };
        // SAFETY: `data` is null or points to the structure `action` defines, valid for the call (upheld by the caller of this function).
        unsafe { SystemParametersInfoForDpi(action, param, data, win_ini, dpi) }.map_err(|e| win32_error(&e))
    }

    /// The C# use of `SystemParametersInfoForDpi`: `SPI_GETNONCLIENTMETRICS` into a `NONCLIENTMETRICSW` whose `cbSize` is `NONCLIENTMETRICSW_SIZE` (504); the
    /// dialog font is `lfMessageFont` (offset 408, `NONCLIENTMETRICSW_LFMESSAGEFONT_OFFSET`).
    pub fn non_client_metrics_for_dpi(
        dpi: u32,
    ) -> Result<windows::Win32::UI::WindowsAndMessaging::NONCLIENTMETRICSW, Win32Error> {
        const _: () = assert!(size_of::<NONCLIENTMETRICSW>() == NONCLIENTMETRICSW_SIZE as usize);
        let mut metrics = NONCLIENTMETRICSW {
            cbSize: NONCLIENTMETRICSW_SIZE as u32, // 504, asserted above
            ..Default::default()
        };
        // SAFETY: the pointer is a local `NONCLIENTMETRICSW` whose `cbSize` is its exact size (504); the OS fills it and does not keep the pointer.
        unsafe {
            system_parameters_info_for_dpi(
                SPI_GETNONCLIENTMETRICS,
                NONCLIENTMETRICSW_SIZE as u32,
                (&mut metrics as *mut NONCLIENTMETRICSW).cast::<c_void>(),
                0,
                dpi,
            )
        }?;
        Ok(metrics)
    }

    /// C# `ScreenToClient(hwnd, Point*)`.
    pub fn screen_to_client(hwnd: Hwnd, point: &mut Point) -> bool {
        // SAFETY: `point` is a live `&mut POINT` that the OS updates in place and does not keep.
        unsafe { ScreenToClient(to_hwnd(hwnd), point) }.as_bool()
    }

    /// C# `GetGuiResources(process, flags)` (`GR_GDIOBJECTS` = 0, `GR_USEROBJECTS` = 1): the handle count of the process (the leak tests).
    pub fn get_gui_resources(process: isize, flags: u32) -> u32 {
        // SAFETY: a process handle value (the `-1` pseudo handle from `get_current_process`) and a flag; the OS validates both.
        unsafe { GetGuiResources(HANDLE(process as *mut c_void), GET_GUI_RESOURCES_FLAGS(flags)) }
    }

    /// C# `GetWindow(hwnd, command)` (`GW_*`).
    pub fn get_window(hwnd: Hwnd, command: u32) -> Hwnd {
        // SAFETY: a handle value and a command; the OS validates both (NULL is an `Err` here, `0` like the C# result).
        unsafe { GetWindow(to_hwnd(hwnd), GET_WINDOW_CMD(command)) }
            .map(|h| h.0 as isize)
            .unwrap_or(0)
    }

    /// C# `GetDlgCtrlID`: `0` on failure.
    pub fn get_dlg_ctrl_id(hwnd: Hwnd) -> i32 {
        // SAFETY: a handle value; the OS validates it.
        unsafe { GetDlgCtrlID(to_hwnd(hwnd)) }
    }

    /// C# `InvalidateRect(hwnd, Rect*, erase)`; `None` invalidates the whole client area.
    pub fn invalidate_rect(hwnd: Hwnd, rect: Option<&Rect>, erase: bool) -> bool {
        let rect_ptr = rect.map(|r| r as *const Rect);
        // SAFETY: `rect_ptr` is None or a live `&RECT` that the OS only reads.
        unsafe { InvalidateRect(Some(to_hwnd(hwnd)), rect_ptr, erase) }.as_bool()
    }
}

/// `gdi32.dll` additions of `DialogNative.cs` (the `Gdi32` partial).
pub mod gdi32 {
    use super::*;
    use windows::Win32::Foundation::{COLORREF, SIZE};
    use windows::Win32::Graphics::Gdi::{
        CreateFontIndirectW, CreateSolidBrush, DeleteObject, GetBkColor, GetObjectW, GetTextColor,
        GetTextExtentPoint32W, GetTextMetricsW, LOGFONTW, SelectObject, SetBkColor, SetTextColor, TEXTMETRICW,
    };

    /// C# `CreateSolidBrush(colorRef)`: `0` on failure.
    pub fn create_solid_brush(color_ref: u32) -> Hgdi {
        // SAFETY: a colour value; the result is a GDI handle that is not dereferenced here.
        unsafe { CreateSolidBrush(COLORREF(color_ref)) }.0 as isize
    }

    /// C# `DeleteObject`: `false` when the object is selected into a DC or invalid.
    pub fn delete_object(object: Hgdi) -> bool {
        // SAFETY: a handle value; the OS validates it and refuses an object that is still selected into a DC.
        unsafe { DeleteObject(to_hgdiobj(object)) }.as_bool()
    }

    /// C# `SetTextColor`: the previous colour (`CLR_INVALID` = `0xFFFF_FFFF` on failure).
    pub fn set_text_color(hdc: Hdc, color_ref: u32) -> u32 {
        // SAFETY: handle and colour values; the OS validates the DC.
        unsafe { SetTextColor(to_hdc(hdc), COLORREF(color_ref)) }.0
    }

    /// C# `SetBkColor`.
    pub fn set_bk_color(hdc: Hdc, color_ref: u32) -> u32 {
        // SAFETY: handle and colour values; the OS validates the DC.
        unsafe { SetBkColor(to_hdc(hdc), COLORREF(color_ref)) }.0
    }

    /// C# `GetTextColor`.
    pub fn get_text_color(hdc: Hdc) -> u32 {
        // SAFETY: a handle value; the OS validates it.
        unsafe { GetTextColor(to_hdc(hdc)) }.0
    }

    /// C# `GetBkColor`.
    pub fn get_bk_color(hdc: Hdc) -> u32 {
        // SAFETY: a handle value; the OS validates it.
        unsafe { GetBkColor(to_hdc(hdc)) }.0
    }

    /// C# `SelectObject`: the previous object of that kind (`0` on failure). The caller reselects the old object before deleting a GDI object it created.
    pub fn select_object(hdc: Hdc, object: Hgdi) -> Hgdi {
        // SAFETY: handle values; the OS validates both.
        unsafe { SelectObject(to_hdc(hdc), to_hgdiobj(object)) }.0 as isize
    }

    /// C# `GetTextExtentPoint32(hdc, char* text, count, Point* size)`: the size as a `Point` (`x` = cx, `y` = cy), `None` on failure.
    pub fn get_text_extent_point32(hdc: Hdc, text: &str) -> Option<Point> {
        let units: Vec<u16> = text.encode_utf16().collect();
        if i32::try_from(units.len()).is_err() {
            return None;
        }
        let mut size = SIZE::default();
        // SAFETY: `units` is a live local slice whose length is the count; `size` is a live local `SIZE` that the OS fills.
        let ok = unsafe { GetTextExtentPoint32W(to_hdc(hdc), &units, &mut size) }.as_bool();
        ok.then_some(Point { x: size.cx, y: size.cy })
    }

    /// C# `GetTextMetrics(hdc, void* metrics)` with a `TEXTMETRICW` (60 bytes; the C# code reads the first five `LONG`s).
    pub fn get_text_metrics(hdc: Hdc) -> Option<windows::Win32::Graphics::Gdi::TEXTMETRICW> {
        let mut metrics = TEXTMETRICW::default();
        // SAFETY: the pointer is to a live local `TEXTMETRICW` (the exact SDK struct) that the OS fills and does not keep.
        let ok = unsafe { GetTextMetricsW(to_hdc(hdc), &mut metrics) }.as_bool();
        ok.then_some(metrics)
    }

    /// C# `CreateFontIndirect(void* logFont)` with a `LOGFONTW` (92 bytes): `0` on failure.
    pub fn create_font_indirect(log_font: &windows::Win32::Graphics::Gdi::LOGFONTW) -> Hgdi {
        // SAFETY: the pointer is to a live `&LOGFONTW` (the exact SDK struct) that the OS only reads (it copies the description into the font).
        unsafe { CreateFontIndirectW(log_font) }.0 as isize
    }

    /// C# `GetObject(handle, size, void* buffer)`: the number of bytes written (`0` on failure). The OS writes at most `buffer.len()` bytes.
    pub fn get_object(handle: Hgdi, buffer: &mut [u8]) -> i32 {
        let Ok(size) = i32::try_from(buffer.len()) else {
            return 0;
        };
        // SAFETY: the pointer is to a slice whose length is passed as the size; the OS writes at most `size` bytes.
        unsafe { GetObjectW(to_hgdiobj(handle), size, Some(buffer.as_mut_ptr().cast::<c_void>())) }
    }

    /// `get_object` for a font handle into a `LOGFONTW` (the 92 byte C# use): `None` when fewer than 92 bytes were written.
    pub fn get_log_font(font: Hgdi) -> Option<windows::Win32::Graphics::Gdi::LOGFONTW> {
        const _: () = assert!(size_of::<LOGFONTW>() == LOGFONTW_SIZE as usize);
        let mut log_font = LOGFONTW::default();
        // SAFETY: the pointer is to a live local `LOGFONTW` of exactly `LOGFONTW_SIZE` (92) bytes and that is the size passed.
        let written = unsafe {
            GetObjectW(
                to_hgdiobj(font),
                LOGFONTW_SIZE,
                Some((&mut log_font as *mut LOGFONTW).cast::<c_void>()),
            )
        };
        (written >= LOGFONTW_SIZE).then_some(log_font)
    }
}

/// `kernel32.dll` addition of `DialogNative.cs` (the `Kernel32` partial).
pub mod kernel32 {
    /// C# `GetCurrentProcess`: the pseudo handle `-1` (needs no `CloseHandle`).
    pub fn get_current_process() -> isize {
        // SAFETY: no arguments; the result is a pseudo handle, never dereferenced.
        unsafe { windows::Win32::System::Threading::GetCurrentProcess() }.0 as isize
    }
}

/// `uxtheme.dll` (C# `Uxtheme`).
pub mod uxtheme {
    use super::*;
    use windows::Win32::UI::Controls::SetWindowTheme;
    use windows::core::PCWSTR;

    /// C# `SetWindowTheme(hwnd, subAppName, subIdList)`: `("DarkMode_Explorer", None)`, `("DarkMode_CFD", None)`..., `(Some(""), Some(""))` removes the theme.
    pub fn set_window_theme(hwnd: Hwnd, sub_app_name: Option<&str>, sub_id_list: Option<&str>) -> Result<(), Hresult> {
        let app = sub_app_name.map(to_utf16z);
        let list = sub_id_list.map(to_utf16z);
        let app_ptr = app.as_ref().map_or(PCWSTR::null(), |t| PCWSTR(t.as_ptr()));
        let list_ptr = list.as_ref().map_or(PCWSTR::null(), |t| PCWSTR(t.as_ptr()));
        // SAFETY: each pointer is null or points into a NUL-terminated UTF-16 buffer in a local that lives past the call; the OS copies the names.
        unsafe { SetWindowTheme(to_hwnd(hwnd), app_ptr, list_ptr) }.map_err(|e| Hresult(e.code().0))
    }
}
