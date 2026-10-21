//! Port of `PhotoReview.Shell.Interop/Win32/{User32, Kernel32, Kernel32Loader, Gdi32, Shell32, Shcore, Dwmapi, Ole32}.cs` (baseline `d92d6aae`,
//! 298 lines): the P/Invokes the window host, the message loop and the placement code call, as safe wrappers over the `windows` crate.
//!
//! Unit W8-11b. One nested module per DLL, function names are the C# names in snake case (`RegisterClassEx` is `user32::register_class_ex`).
//! Rules (D-W8B3-4, written out in `contracts/W8-11b.md`): handles are `isize`; a C# `SetLastError = true` function that returns `bool`/`nint 0` on
//! failure returns `Result<_, Win32Error>` with the error read immediately after the call; a function that returns an `HRESULT` returns
//! `Result<_, Hresult>`; a wrapper is `pub fn` only when no integer argument can make the callee read or write memory (message forwarding, window
//! procedure replacement and clipboard ownership transfer are `pub unsafe fn`); strings are `&str`, converted to NUL-terminated UTF-16 inside.
//! The functions that C# declares in the other partials of `User32`/`Gdi32` (`CommonControlsNative`, `DialogNative`, `SettingsDialogNative`,
//! `StartupAdditions`) belong to W8-11c/11d/11e.
#![allow(unused_variables, dead_code)] // stub: removed by the W8-11b porter when the last `todo!()` is gone.

use super::structs::{
    Hcursor, Hgdi, Hinstance, Hmenu, Hmonitor, Hresult, Hwnd, InputMessageSource, Lparam, Lresult, MonitorInfo,
    MonitorInfoEx, Msg, Point, Rect, Win32Error, WindowPlacement, WndClassEx, Wparam,
};

/// `user32.dll`.
pub mod user32 {
    use super::*;

    // C# `User32` constants of `User32.cs`; the other six aliases (`WmCreate`, `WmDestroy`, `WmClose`, `WsOverlappedWindow`, `SwShowNoActivate`,
    // `SwShowDefault`) are the `messages` constants and are re-exported here under the same SDK names.
    pub use crate::win32::messages::{
        SW_SHOWDEFAULT, SW_SHOWNOACTIVATE, WM_CLOSE, WM_CREATE, WM_DESTROY, WS_OVERLAPPEDWINDOW,
    };
    /// C# `CwUseDefault` = `unchecked((int)0x80000000)`.
    pub const CW_USEDEFAULT: i32 = -2147483648;
    /// C# `IdcArrow` (`MAKEINTRESOURCE(32512)`).
    pub const IDC_ARROW: i32 = 32512;

    /// What `GetMessageW` returned (C# `int`: `> 0`, `0`, `-1`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MessageOutcome {
        /// A message other than `WM_QUIT` was stored in `msg`.
        Message,
        /// `WM_QUIT` was retrieved (C# result `0`); `msg.wParam` is the exit code.
        Quit,
        /// The call failed (C# result `-1`, e.g. a bad `hwnd`).
        Failed(Win32Error),
    }

    /// One monitor of `enum_display_monitors`.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct MonitorEnumItem {
        /// `HMONITOR`.
        pub monitor: Hmonitor,
        /// The monitor rectangle in virtual-screen coordinates (the `lprcMonitor` argument of the callback).
        pub rect: Rect,
    }

    /// C# `RegisterClassEx(WndClassEx*)`: the atom, `Err` when `0`. `class.cbSize` must be `size_of::<WndClassEx>()` (use `new_wnd_class_ex`);
    /// the strings `lpszClassName`/`lpszMenuName` and the window procedure must stay valid while the class is registered.
    ///
    /// # Safety
    /// `class.lpszClassName`, `class.lpszMenuName` (when non-null) must point to NUL-terminated UTF-16 text that outlives the registration call
    /// (the OS copies the names), and `class.lpfnWndProc` must be a valid window procedure for the lifetime of the class.
    pub unsafe fn register_class_ex(class: &WndClassEx) -> Result<u16, Win32Error> {
        todo!()
    }

    /// C# `UnregisterClass(string, nint)`.
    pub fn unregister_class(class_name: &str, instance: Hinstance) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `CreateWindowEx(...)`; `Err` when the handle is `0`. `menu` is the `HMENU` or the child-window id; `param` is `lpCreateParams`
    /// (arrives in `WM_NCCREATE`/`WM_CREATE`).
    ///
    /// # Safety
    /// The window procedure of `class_name` runs inside this call (`WM_NCCREATE`...); `param` is handed to it as is, so it must be what that procedure expects.
    #[allow(clippy::too_many_arguments)] // mirrors CreateWindowExW 1:1
    pub unsafe fn create_window_ex(
        ex_style: u32,
        class_name: &str,
        window_name: &str,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: Hmenu,
        instance: Hinstance,
        param: isize,
    ) -> Result<Hwnd, Win32Error> {
        todo!()
    }

    /// C# `DefWindowProc`.
    ///
    /// # Safety
    /// Call it only from a window procedure with the message parameters that procedure received (some messages carry pointers in `lparam`).
    pub unsafe fn def_window_proc(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        todo!()
    }

    /// C# `ShowWindow`: `true` when the window was visible before.
    pub fn show_window(hwnd: Hwnd, command: i32) -> bool {
        todo!()
    }

    /// C# `IsWindowVisible`.
    pub fn is_window_visible(hwnd: Hwnd) -> bool {
        todo!()
    }

    /// C# `DestroyWindow` (only from the thread that created the window, else the OS fails with `ERROR_ACCESS_DENIED`).
    pub fn destroy_window(hwnd: Hwnd) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `PostMessage`.
    ///
    /// # Safety
    /// A message that carries a pointer in `wparam`/`lparam` outlives this call (it is delivered later); the caller keeps that pointer valid until delivery.
    pub unsafe fn post_message(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `PostQuitMessage`.
    pub fn post_quit_message(exit_code: i32) {
        todo!()
    }

    /// C# `GetMessage(Msg*, hwnd, min, max)`; `hwnd == 0` retrieves for the whole thread.
    pub fn get_message(msg: &mut Msg, hwnd: Hwnd, filter_min: u32, filter_max: u32) -> MessageOutcome {
        todo!()
    }

    /// C# `PeekMessage`: `true` when a message was stored.
    pub fn peek_message(msg: &mut Msg, hwnd: Hwnd, filter_min: u32, filter_max: u32, remove_flags: u32) -> bool {
        todo!()
    }

    /// C# `TranslateMessage`.
    pub fn translate_message(msg: &Msg) -> bool {
        todo!()
    }

    /// C# `DispatchMessage`.
    ///
    /// # Safety
    /// `msg` comes from `get_message`/`peek_message` (or is built so that the target window procedure accepts its `wparam`/`lparam`).
    pub unsafe fn dispatch_message(msg: &Msg) -> Lresult {
        todo!()
    }

    /// C# `MsgWaitForMultipleObjectsEx(count, handles, ms, wakeMask, flags)`: the raw `DWORD` (`WAIT_OBJECT_0 + n`, `WAIT_FAILED` = `0xFFFF_FFFF`,
    /// `WAIT_TIMEOUT`, ...). `handles.len()` is the count (at most 63 for the OS; more is `WAIT_FAILED`). On `WAIT_FAILED` the caller reads `Win32Error::last()`.
    pub fn msg_wait_for_multiple_objects_ex(handles: &[isize], milliseconds: u32, wake_mask: u32, flags: u32) -> u32 {
        todo!()
    }

    /// C# `LoadCursor(0, id)` (every caller passes instance `0`): a shared system cursor (`IDC_ARROW`, `IDC_WAIT`, `IDC_SIZEALL`...).
    pub fn load_system_cursor(id: i32) -> Result<Hcursor, Win32Error> {
        todo!()
    }

    /// C# `SetCursor`: the previous cursor.
    pub fn set_cursor(cursor: Hcursor) -> Hcursor {
        todo!()
    }

    /// C# `SetCapture`: the previous capture window (`0` for none).
    pub fn set_capture(hwnd: Hwnd) -> Hwnd {
        todo!()
    }

    /// C# `ReleaseCapture`.
    pub fn release_capture() -> bool {
        todo!()
    }

    /// C# `GetDpiForWindow` (`0` for an invalid window).
    pub fn get_dpi_for_window(hwnd: Hwnd) -> u32 {
        todo!()
    }

    /// C# `GetDpiForSystem`.
    pub fn get_dpi_for_system() -> u32 {
        todo!()
    }

    /// C# `GetSystemMetrics`.
    pub fn get_system_metrics(index: i32) -> i32 {
        todo!()
    }

    /// C# `GetSystemMetricsForDpi`.
    pub fn get_system_metrics_for_dpi(index: i32, dpi: u32) -> i32 {
        todo!()
    }

    /// C# `AdjustWindowRectExForDpi(Rect*, style, menu, exStyle, dpi)`: grows `rect` from a client rectangle to a window rectangle.
    pub fn adjust_window_rect_ex_for_dpi(
        rect: &mut Rect,
        style: u32,
        menu: bool,
        ex_style: u32,
        dpi: u32,
    ) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `GetThreadDpiAwarenessContext` (a pseudo handle such as `-4`, not a real handle).
    pub fn get_thread_dpi_awareness_context() -> isize {
        todo!()
    }

    /// C# `GetAwarenessFromDpiAwarenessContext` (`DPI_AWARENESS`: -1 invalid, 0 unaware, 1 system, 2 per monitor).
    pub fn get_awareness_from_dpi_awareness_context(context: isize) -> i32 {
        todo!()
    }

    /// C# `MonitorFromWindow(hwnd, flags)` (`MONITOR_DEFAULTTO*`); `0` when `MONITOR_DEFAULTTONULL` finds none.
    pub fn monitor_from_window(hwnd: Hwnd, flags: u32) -> Hmonitor {
        todo!()
    }

    /// C# `MonitorFromRect`.
    pub fn monitor_from_rect(rect: &Rect, flags: u32) -> Hmonitor {
        todo!()
    }

    /// C# `MonitorFromPoint(Point, flags)` (the point is passed by value).
    pub fn monitor_from_point(point: Point, flags: u32) -> Hmonitor {
        todo!()
    }

    /// C# `GetMonitorInfo`: fills `info`; the wrapper sets `info.cbSize` itself (D-W8B3-6), so a forgotten size cannot make the call fail.
    pub fn get_monitor_info(monitor: Hmonitor, info: &mut MonitorInfo) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `GetMonitorInfoEx` (the same `GetMonitorInfoW` with a `MONITORINFOEXW`): fills `szDevice`. The wrapper sets `cbSize` to 104.
    pub fn get_monitor_info_ex(monitor: Hmonitor, info: &mut MonitorInfoEx) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `EnumDisplayMonitors(hdc, clip, callback, data)` for the only use the shell has: every monitor, in the OS order, as a list
    /// (D-W8B3-7: the unmanaged callback and the `data` pointer stay inside this function). `clip` limits to monitors that intersect it.
    pub fn enum_display_monitors(clip: Option<&Rect>) -> Result<Vec<MonitorEnumItem>, Win32Error> {
        todo!()
    }

    /// C# `GetWindowPlacement`: the wrapper sets `placement.length`.
    pub fn get_window_placement(hwnd: Hwnd, placement: &mut WindowPlacement) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `SetWindowPlacement`: the wrapper sets `length` on a copy.
    pub fn set_window_placement(hwnd: Hwnd, placement: &WindowPlacement) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `SetWindowPos`; `insert_after` is a window handle or `HWND_TOP`/`HWND_BOTTOM`/`HWND_TOPMOST`/`HWND_NOTOPMOST`.
    pub fn set_window_pos(
        hwnd: Hwnd,
        insert_after: Hwnd,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `GetWindowRect(hwnd, Rect*)`, returned by value.
    pub fn get_window_rect(hwnd: Hwnd) -> Result<Rect, Win32Error> {
        todo!()
    }

    /// C# `GetClientRect`.
    pub fn get_client_rect(hwnd: Hwnd) -> Result<Rect, Win32Error> {
        todo!()
    }

    /// C# `GetWindowLongPtr(hwnd, index)` (`GWL_STYLE`, `GWL_EXSTYLE`, `GWLP_*`); `0` is also a valid value, so no `Result`.
    pub fn get_window_long_ptr(hwnd: Hwnd, index: i32) -> isize {
        todo!()
    }

    /// C# `SetWindowLongPtr`: the previous value.
    ///
    /// # Safety
    /// `GWLP_WNDPROC` needs a valid window procedure and `GWLP_USERDATA`/`DWLP_USER` data the window procedure interprets: the caller owns that contract.
    pub unsafe fn set_window_long_ptr(hwnd: Hwnd, index: i32, value: isize) -> isize {
        todo!()
    }

    /// C# `GetCurrentInputMessageSource`: `None` when the call fails.
    pub fn get_current_input_message_source() -> Option<InputMessageSource> {
        todo!()
    }

    /// C# `OpenClipboard`.
    pub fn open_clipboard(owner: Hwnd) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `EmptyClipboard` (needs `open_clipboard` first).
    pub fn empty_clipboard() -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `SetClipboardData(format, memory)`: on success the system owns `memory`.
    ///
    /// # Safety
    /// `memory` is a moveable global memory handle (`GMEM_MOVEABLE`) whose content matches `format`; after `Ok` the caller must not free or lock it.
    pub unsafe fn set_clipboard_data(format: u32, memory: isize) -> Result<isize, Win32Error> {
        todo!()
    }

    /// C# `GetClipboardData`: a handle owned by the clipboard (do not free it).
    pub fn get_clipboard_data(format: u32) -> Result<isize, Win32Error> {
        todo!()
    }

    /// C# `CloseClipboard`.
    pub fn close_clipboard() -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `CreatePopupMenu`.
    pub fn create_popup_menu() -> Result<Hmenu, Win32Error> {
        todo!()
    }

    /// C# `DestroyMenu`.
    pub fn destroy_menu(menu: Hmenu) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `AppendMenu(menu, flags, itemId, newItem)`: `new_item` is the text for `MF_STRING` and `None` for `MF_SEPARATOR`; `item_id` is a command id or,
    /// with `MF_POPUP`, a sub-menu handle. Flags that change the meaning of `new_item` (`MF_BITMAP`, `MF_OWNERDRAW`) are not used by the shell.
    pub fn append_menu(menu: Hmenu, flags: u32, item_id: usize, new_item: Option<&str>) -> Result<(), Win32Error> {
        todo!()
    }

    /// C# `TrackPopupMenuEx(menu, flags, x, y, hwnd, 0)` (every caller passes a null `TPMPARAMS`): with `TPM_RETURNCMD` the chosen command id, `0` for none.
    pub fn track_popup_menu_ex(menu: Hmenu, flags: u32, x: i32, y: i32, hwnd: Hwnd) -> i32 {
        todo!()
    }
}

/// `kernel32.dll` (`Kernel32.cs`, `Kernel32Loader.cs`).
pub mod kernel32 {
    use super::*;

    pub const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0800; // LoadLibrarySearchSystem32

    /// C# `GetModuleHandle(0)`: the handle of the running `.exe` (every caller passes `0`).
    pub fn get_module_handle() -> Result<Hinstance, Win32Error> {
        todo!()
    }

    /// C# `GlobalAlloc(flags, bytes)` (`GMEM_MOVEABLE | GMEM_ZEROINIT` for clipboard data).
    pub fn global_alloc(flags: u32, bytes: usize) -> Result<isize, Win32Error> {
        todo!()
    }

    /// C# `GlobalLock`: the address of the block (null with `Err` on failure). The pointer is valid until the matching `global_unlock`.
    pub fn global_lock(memory: isize) -> Result<*mut core::ffi::c_void, Win32Error> {
        todo!()
    }

    /// C# `GlobalUnlock`: `true` while the block is still locked. `false` is NOT an error when the lock count reached zero (`GetLastError()` is `NO_ERROR`).
    pub fn global_unlock(memory: isize) -> bool {
        todo!()
    }

    /// C# `GlobalFree`: `0` on success, the handle itself on failure.
    ///
    /// # Safety
    /// `memory` is not locked and not owned by the clipboard or an OLE data object any more.
    pub unsafe fn global_free(memory: isize) -> isize {
        todo!()
    }

    /// C# `LoadLibraryEx(fileName, 0, flags)` (Kernel32Loader; `LOAD_LIBRARY_SEARCH_SYSTEM32` loads from System32 only). The module is never freed
    /// (C# has no `FreeLibrary`): it is the dark-mode menu uxtheme, kept for the process lifetime.
    pub fn load_library_ex(file_name: &str, flags: u32) -> Result<Hinstance, Win32Error> {
        todo!()
    }

    /// C# `GetProcAddress(module, ordinal)`: the export with that ordinal (`MAKEINTRESOURCE`), `None` when missing. Calling the address is the caller's `unsafe`.
    pub fn get_proc_address_by_ordinal(module: Hinstance, ordinal: u16) -> Option<usize> {
        todo!()
    }
}

/// `gdi32.dll` (`Gdi32.cs`).
pub mod gdi32 {
    use super::*;

    pub const BLACK_BRUSH: i32 = 4; // BlackBrush

    /// C# `GetStockObject(index)` (`BLACK_BRUSH`...); a stock object must not be deleted.
    pub fn get_stock_object(index: i32) -> Hgdi {
        todo!()
    }
}

/// `shell32.dll` (`Shell32.cs`).
pub mod shell32 {
    use super::*;

    /// C# `DragAcceptFiles(hwnd, accept)`: the legacy `WM_DROPFILES` switch (the window also needs `WS_EX_ACCEPTFILES`).
    pub fn drag_accept_files(hwnd: Hwnd, accept: bool) {
        todo!()
    }
}

/// `shcore.dll` (`Shcore.cs`).
pub mod shcore {
    use super::*;

    pub const MDT_EFFECTIVE_DPI: i32 = 0; // MdtEffectiveDpi

    /// C# `GetDpiForMonitor(monitor, dpiType, &x, &y)`: `(dpi_x, dpi_y)` or the failing `HRESULT`.
    pub fn get_dpi_for_monitor(monitor: Hmonitor, dpi_type: i32) -> Result<(u32, u32), Hresult> {
        todo!()
    }
}

/// `dwmapi.dll` (`Dwmapi.cs`).
pub mod dwmapi {
    use super::*;

    pub const DWMWA_NCRENDERING_POLICY: u32 = 2; // DwmwaNcRenderingPolicy
    pub const DWMWA_TRANSITIONS_FORCEDISABLED: u32 = 3; // DwmwaTransitionsForceDisabled
    pub const DWMWA_CLOAK: u32 = 0x000D; // DwmwaCloak
    pub const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 0x0014; // DwmwaUseImmersiveDarkMode
    pub const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 0x0021; // DwmwaWindowCornerPreference
    pub const DWMWA_BORDER_COLOR: u32 = 0x0022; // DwmwaBorderColor
    pub const DWMWA_CAPTION_COLOR: u32 = 0x0023; // DwmwaCaptionColor

    /// C# `DwmSetWindowAttribute(hwnd, attribute, void* value, size)`: `value` is a `u32`/`i32`/`BOOL`-sized scalar (`DWMWA_USE_IMMERSIVE_DARK_MODE`: `1`/`0`,
    /// `DWMWA_CAPTION_COLOR`: a `COLORREF`); the size passed is `size_of::<T>()`. A size the attribute does not accept is `E_INVALIDARG`, not undefined behaviour.
    pub fn dwm_set_window_attribute<T: Copy>(hwnd: Hwnd, attribute: u32, value: &T) -> Result<(), Hresult> {
        todo!()
    }
}

/// `ole32.dll` (`Ole32.cs`).
pub mod ole32 {
    use super::*;
    use windows::Win32::System::Ole::IDropTarget;

    /// C# `OleInitialize(0)`: `S_OK`/`S_FALSE` are `Ok` (both need one `ole_uninitialize`), a negative `HRESULT` is `Err`. Must run on the STA thread of the window.
    pub fn ole_initialize() -> Result<(), Hresult> {
        todo!()
    }

    /// C# `OleUninitialize` (balanced with a successful `ole_initialize`).
    pub fn ole_uninitialize() {
        todo!()
    }

    /// C# `RegisterDragDrop(hwnd, IDropTarget*)`; the typed `IDropTarget` replaces the raw `nint` (W8-14 implements it with `#[implement]`).
    pub fn register_drag_drop(hwnd: Hwnd, drop_target: &IDropTarget) -> Result<(), Hresult> {
        todo!()
    }

    /// C# `RevokeDragDrop`.
    pub fn revoke_drag_drop(hwnd: Hwnd) -> Result<(), Hresult> {
        todo!()
    }
}
