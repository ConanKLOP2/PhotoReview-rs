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
use core::ffi::c_void;

use windows::Win32::Foundation::{HANDLE, HGLOBAL, HINSTANCE, HMODULE, HWND, LPARAM, WPARAM};
use windows::core::{PCSTR, PCWSTR};

use super::structs::{
    Hcursor, Hgdi, Hinstance, Hmenu, Hmonitor, Hresult, Hwnd, InputMessageSource, Lparam, Lresult, MonitorInfo,
    MonitorInfoEx, Msg, Point, Rect, Win32Error, WindowPlacement, WndClassEx, Wparam, to_utf16z,
};

/// `isize` handle to the `windows` type (a handle value only; nothing is dereferenced).
fn to_hwnd(handle: Hwnd) -> HWND {
    HWND(handle as *mut c_void)
}

/// `isize` handle to `HINSTANCE`.
fn to_hinstance(handle: Hinstance) -> HINSTANCE {
    HINSTANCE(handle as *mut c_void)
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

/// `user32.dll`.
pub mod user32 {
    use super::*;
    use windows::Win32::Foundation::{RECT, TRUE};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_FROM_FLAGS, MonitorFromPoint, MonitorFromRect,
        MonitorFromWindow,
    };
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::UI::HiDpi::{
        AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT, GetAwarenessFromDpiAwarenessContext, GetDpiForSystem,
        GetDpiForWindow, GetSystemMetricsForDpi, GetThreadDpiAwarenessContext,
    };
    use windows::Win32::UI::Input::GetCurrentInputMessageSource;
    use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW,
        GetClientRect, GetMessageW, GetSystemMetrics, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect, HCURSOR,
        HMENU, IsWindowVisible, LoadCursorW, MENU_ITEM_FLAGS, MSG_WAIT_FOR_MULTIPLE_OBJECTS_EX_FLAGS,
        MsgWaitForMultipleObjectsEx, PEEK_MESSAGE_REMOVE_TYPE, PeekMessageW, PostMessageW, PostQuitMessage,
        QUEUE_STATUS_FLAGS, RegisterClassExW, SET_WINDOW_POS_FLAGS, SHOW_WINDOW_CMD, SYSTEM_METRICS_INDEX, SetCursor,
        SetWindowLongPtrW, SetWindowPlacement, SetWindowPos, ShowWindow, TrackPopupMenuEx, TranslateMessage,
        UnregisterClassW, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE,
    };

    /// The `EnumDisplayMonitors` callback: pushes one `MonitorEnumItem` into the `Vec` that `lparam` points to and keeps going. It cannot unwind
    /// (a failed allocation aborts), so nothing crosses the FFI boundary as a panic.
    unsafe extern "system" fn enum_monitor_proc(
        monitor: HMONITOR,
        _hdc: HDC,
        rect: *mut RECT,
        lparam: LPARAM,
    ) -> windows::core::BOOL {
        // SAFETY: `lparam` is the address of the `Vec` on the stack of `enum_display_monitors`, which is still inside its `EnumDisplayMonitors` call.
        let items = unsafe { &mut *(lparam.0 as *mut Vec<MonitorEnumItem>) };
        let rect = if rect.is_null() {
            Rect::default()
        } else {
            // SAFETY: non-null, and it points to the monitor rectangle that the OS keeps alive during the callback.
            unsafe { *rect }
        };
        items.push(MonitorEnumItem {
            monitor: monitor.0 as isize,
            rect,
        });
        TRUE
    }

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
        // SAFETY: `class` is a live, correctly sized WNDCLASSEXW; its pointer members are valid per the `# Safety` contract upheld by the caller of this function.
        let atom = unsafe { RegisterClassExW(class) };
        if atom == 0 { Err(Win32Error::last()) } else { Ok(atom) }
    }

    /// C# `UnregisterClass(string, nint)`.
    pub fn unregister_class(class_name: &str, instance: Hinstance) -> Result<(), Win32Error> {
        let name = to_utf16z(class_name);
        // SAFETY: `name` is a NUL-terminated UTF-16 buffer in a local that lives past the call; the instance is a handle value the OS validates.
        unsafe { UnregisterClassW(PCWSTR(name.as_ptr()), Some(to_hinstance(instance))) }.map_err(|e| win32_error(&e))
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
        let class = to_utf16z(class_name);
        let title = to_utf16z(window_name);
        // SAFETY: both strings are NUL-terminated UTF-16 buffers in locals that live past the call; handles are values the OS validates;
        // `param` is handed to the window procedure as the caller of this function guarantees.
        let created = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(ex_style),
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WINDOW_STYLE(style),
                x,
                y,
                width,
                height,
                Some(to_hwnd(parent)),
                Some(HMENU(menu as *mut c_void)),
                Some(to_hinstance(instance)),
                Some(param as *const c_void),
            )
        };
        created.map(|h| h.0 as isize).map_err(|e| win32_error(&e))
    }

    /// C# `DefWindowProc`.
    ///
    /// # Safety
    /// Call it only from a window procedure with the message parameters that procedure received (some messages carry pointers in `lparam`).
    pub unsafe fn def_window_proc(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        // SAFETY: upheld by the caller of this function (message parameters come from the window procedure that received them).
        unsafe { DefWindowProcW(to_hwnd(hwnd), message, WPARAM(wparam), LPARAM(lparam)) }.0
    }

    /// C# `ShowWindow`: `true` when the window was visible before.
    pub fn show_window(hwnd: Hwnd, command: i32) -> bool {
        // SAFETY: a handle value and a command; the OS validates the handle.
        unsafe { ShowWindow(to_hwnd(hwnd), SHOW_WINDOW_CMD(command)) }.as_bool()
    }

    /// C# `IsWindowVisible`.
    pub fn is_window_visible(hwnd: Hwnd) -> bool {
        // SAFETY: a handle value; the OS validates it.
        unsafe { IsWindowVisible(to_hwnd(hwnd)) }.as_bool()
    }

    /// C# `DestroyWindow` (only from the thread that created the window, else the OS fails with `ERROR_ACCESS_DENIED`).
    pub fn destroy_window(hwnd: Hwnd) -> Result<(), Win32Error> {
        // SAFETY: a handle value; the OS validates it and fails the call for a stale or foreign-thread window.
        unsafe { DestroyWindow(to_hwnd(hwnd)) }.map_err(|e| win32_error(&e))
    }

    /// C# `PostMessage`.
    ///
    /// # Safety
    /// A message that carries a pointer in `wparam`/`lparam` outlives this call (it is delivered later); the caller keeps that pointer valid until delivery.
    pub unsafe fn post_message(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Result<(), Win32Error> {
        // SAFETY: upheld by the caller of this function (any pointer in the parameters stays valid until delivery).
        unsafe { PostMessageW(Some(to_hwnd(hwnd)), message, WPARAM(wparam), LPARAM(lparam)) }
            .map_err(|e| win32_error(&e))
    }

    /// C# `PostQuitMessage`.
    pub fn post_quit_message(exit_code: i32) {
        // SAFETY: plain integer argument; posts WM_QUIT to the calling thread's queue.
        unsafe { PostQuitMessage(exit_code) }
    }

    /// C# `GetMessage(Msg*, hwnd, min, max)`; `hwnd == 0` retrieves for the whole thread.
    pub fn get_message(msg: &mut Msg, hwnd: Hwnd, filter_min: u32, filter_max: u32) -> MessageOutcome {
        // SAFETY: `msg` is a live `&mut MSG` of the right size; the OS fills it and does not keep the pointer.
        let result = unsafe { GetMessageW(msg, Some(to_hwnd(hwnd)), filter_min, filter_max) };
        match result.0 {
            0 => MessageOutcome::Quit,
            -1 => MessageOutcome::Failed(Win32Error::last()),
            _ => MessageOutcome::Message,
        }
    }

    /// C# `PeekMessage`: `true` when a message was stored.
    pub fn peek_message(msg: &mut Msg, hwnd: Hwnd, filter_min: u32, filter_max: u32, remove_flags: u32) -> bool {
        // SAFETY: `msg` is a live `&mut MSG` of the right size; the OS fills it and does not keep the pointer.
        unsafe {
            PeekMessageW(
                msg,
                Some(to_hwnd(hwnd)),
                filter_min,
                filter_max,
                PEEK_MESSAGE_REMOVE_TYPE(remove_flags),
            )
        }
        .as_bool()
    }

    /// C# `TranslateMessage`.
    pub fn translate_message(msg: &Msg) -> bool {
        // SAFETY: `msg` is a live `&MSG`; the OS only reads it.
        unsafe { TranslateMessage(msg) }.as_bool()
    }

    /// C# `DispatchMessage`.
    ///
    /// # Safety
    /// `msg` comes from `get_message`/`peek_message` (or is built so that the target window procedure accepts its `wparam`/`lparam`).
    pub unsafe fn dispatch_message(msg: &Msg) -> Lresult {
        // SAFETY: upheld by the caller of this function; `msg` is a live `&MSG` that the OS only reads.
        unsafe { DispatchMessageW(msg) }.0
    }

    /// C# `MsgWaitForMultipleObjectsEx(count, handles, ms, wakeMask, flags)`: the raw `DWORD` (`WAIT_OBJECT_0 + n`, `WAIT_FAILED` = `0xFFFF_FFFF`,
    /// `WAIT_TIMEOUT`, ...). `handles.len()` is the count (at most 63 for the OS; more is `WAIT_FAILED`). On `WAIT_FAILED` the caller reads `Win32Error::last()`.
    pub fn msg_wait_for_multiple_objects_ex(handles: &[isize], milliseconds: u32, wake_mask: u32, flags: u32) -> u32 {
        let list: Vec<HANDLE> = handles.iter().map(|&h| HANDLE(h as *mut c_void)).collect();
        let slice = if list.is_empty() { None } else { Some(list.as_slice()) };
        // SAFETY: `slice` is None or a live slice of handle values that outlives the call; the OS validates the handles.
        unsafe {
            MsgWaitForMultipleObjectsEx(
                slice,
                milliseconds,
                QUEUE_STATUS_FLAGS(wake_mask),
                MSG_WAIT_FOR_MULTIPLE_OBJECTS_EX_FLAGS(flags),
            )
        }
        .0
    }

    /// C# `LoadCursor(0, id)` (every caller passes instance `0`): a shared system cursor (`IDC_ARROW`, `IDC_WAIT`, `IDC_SIZEALL`...).
    pub fn load_system_cursor(id: i32) -> Result<Hcursor, Win32Error> {
        // SAFETY: with a null instance the "name" is `MAKEINTRESOURCE(id)`, an integer in a pointer that the OS never dereferences as text.
        unsafe { LoadCursorW(None, PCWSTR(id as usize as *const u16)) }
            .map(|c| c.0 as isize)
            .map_err(|e| win32_error(&e))
    }

    /// C# `SetCursor`: the previous cursor.
    pub fn set_cursor(cursor: Hcursor) -> Hcursor {
        // SAFETY: a handle value; the OS validates it.
        unsafe { SetCursor(Some(HCURSOR(cursor as *mut c_void))) }.0 as isize
    }

    /// C# `SetCapture`: the previous capture window (`0` for none).
    pub fn set_capture(hwnd: Hwnd) -> Hwnd {
        // SAFETY: a handle value; the OS validates it.
        unsafe { SetCapture(to_hwnd(hwnd)) }.0 as isize
    }

    /// C# `ReleaseCapture`.
    pub fn release_capture() -> bool {
        // SAFETY: no arguments.
        unsafe { ReleaseCapture() }.is_ok()
    }

    /// C# `GetDpiForWindow` (`0` for an invalid window).
    pub fn get_dpi_for_window(hwnd: Hwnd) -> u32 {
        // SAFETY: a handle value; the OS validates it (0 for an invalid window).
        unsafe { GetDpiForWindow(to_hwnd(hwnd)) }
    }

    /// C# `GetDpiForSystem`.
    pub fn get_dpi_for_system() -> u32 {
        // SAFETY: no arguments.
        unsafe { GetDpiForSystem() }
    }

    /// C# `GetSystemMetrics`.
    pub fn get_system_metrics(index: i32) -> i32 {
        // SAFETY: plain integer argument.
        unsafe { GetSystemMetrics(SYSTEM_METRICS_INDEX(index)) }
    }

    /// C# `GetSystemMetricsForDpi`.
    pub fn get_system_metrics_for_dpi(index: i32, dpi: u32) -> i32 {
        // SAFETY: plain integer arguments.
        unsafe { GetSystemMetricsForDpi(SYSTEM_METRICS_INDEX(index), dpi) }
    }

    /// C# `AdjustWindowRectExForDpi(Rect*, style, menu, exStyle, dpi)`: grows `rect` from a client rectangle to a window rectangle.
    pub fn adjust_window_rect_ex_for_dpi(
        rect: &mut Rect,
        style: u32,
        menu: bool,
        ex_style: u32,
        dpi: u32,
    ) -> Result<(), Win32Error> {
        // SAFETY: `rect` is a live `&mut RECT` that the OS updates in place and does not keep.
        unsafe { AdjustWindowRectExForDpi(rect, WINDOW_STYLE(style), menu, WINDOW_EX_STYLE(ex_style), dpi) }
            .map_err(|e| win32_error(&e))
    }

    /// C# `GetThreadDpiAwarenessContext` (a pseudo handle such as `-4`, not a real handle).
    pub fn get_thread_dpi_awareness_context() -> isize {
        // SAFETY: no arguments; the result is a pseudo handle, never dereferenced.
        unsafe { GetThreadDpiAwarenessContext() }.0 as isize
    }

    /// C# `GetAwarenessFromDpiAwarenessContext` (`DPI_AWARENESS`: -1 invalid, 0 unaware, 1 system, 2 per monitor).
    pub fn get_awareness_from_dpi_awareness_context(context: isize) -> i32 {
        // SAFETY: a pseudo handle value the OS interprets; it is never dereferenced by us.
        unsafe { GetAwarenessFromDpiAwarenessContext(DPI_AWARENESS_CONTEXT(context as *mut c_void)) }.0
    }

    /// C# `MonitorFromWindow(hwnd, flags)` (`MONITOR_DEFAULTTO*`); `0` when `MONITOR_DEFAULTTONULL` finds none.
    pub fn monitor_from_window(hwnd: Hwnd, flags: u32) -> Hmonitor {
        // SAFETY: a handle value; the OS validates it.
        unsafe { MonitorFromWindow(to_hwnd(hwnd), MONITOR_FROM_FLAGS(flags)) }.0 as isize
    }

    /// C# `MonitorFromRect`.
    pub fn monitor_from_rect(rect: &Rect, flags: u32) -> Hmonitor {
        // SAFETY: `rect` is a live `&RECT` that the OS only reads.
        unsafe { MonitorFromRect(rect, MONITOR_FROM_FLAGS(flags)) }.0 as isize
    }

    /// C# `MonitorFromPoint(Point, flags)` (the point is passed by value).
    pub fn monitor_from_point(point: Point, flags: u32) -> Hmonitor {
        // SAFETY: plain value arguments.
        unsafe { MonitorFromPoint(point, MONITOR_FROM_FLAGS(flags)) }.0 as isize
    }

    /// C# `GetMonitorInfo`: fills `info`; the wrapper sets `info.cbSize` itself (D-W8B3-6), so a forgotten size cannot make the call fail.
    pub fn get_monitor_info(monitor: Hmonitor, info: &mut MonitorInfo) -> Result<(), Win32Error> {
        info.cbSize = size_of::<MonitorInfo>() as u32;
        // SAFETY: `info` is a live `&mut MONITORINFO` whose `cbSize` is set to its size; the OS writes at most that many bytes.
        let ok = unsafe { GetMonitorInfoW(HMONITOR(monitor as *mut c_void), info) }.as_bool();
        if ok { Ok(()) } else { Err(Win32Error::last()) }
    }

    /// C# `GetMonitorInfoEx` (the same `GetMonitorInfoW` with a `MONITORINFOEXW`): fills `szDevice`. The wrapper sets `cbSize` to 104.
    pub fn get_monitor_info_ex(monitor: Hmonitor, info: &mut MonitorInfoEx) -> Result<(), Win32Error> {
        info.monitorInfo.cbSize = size_of::<MonitorInfoEx>() as u32;
        // SAFETY: `info` is a live `&mut MONITORINFOEXW` whose first member is a MONITORINFO; `cbSize` is 104 so the OS writes at most
        // the 104 bytes of the whole structure.
        let ok = unsafe {
            GetMonitorInfoW(
                HMONITOR(monitor as *mut c_void),
                (info as *mut MonitorInfoEx).cast::<MonitorInfo>(),
            )
        }
        .as_bool();
        if ok { Ok(()) } else { Err(Win32Error::last()) }
    }

    /// C# `EnumDisplayMonitors(hdc, clip, callback, data)` for the only use the shell has: every monitor, in the OS order, as a list
    /// (D-W8B3-7: the unmanaged callback and the `data` pointer stay inside this function). `clip` limits to monitors that intersect it.
    pub fn enum_display_monitors(clip: Option<&Rect>) -> Result<Vec<MonitorEnumItem>, Win32Error> {
        let mut items: Vec<MonitorEnumItem> = Vec::new();
        let clip_ptr = clip.map(|r| r as *const Rect);
        // SAFETY: `lparam` is the address of `items`, which lives on this stack frame; the callback runs synchronously on this thread inside
        // `EnumDisplayMonitors` and never after it returns; `clip_ptr` is None or a live `&RECT` the OS only reads.
        let ok = unsafe {
            EnumDisplayMonitors(
                None,
                clip_ptr,
                Some(enum_monitor_proc),
                LPARAM(&mut items as *mut Vec<MonitorEnumItem> as isize),
            )
        }
        .as_bool();
        if ok { Ok(items) } else { Err(Win32Error::last()) }
    }

    /// C# `GetWindowPlacement`: the wrapper sets `placement.length`.
    pub fn get_window_placement(hwnd: Hwnd, placement: &mut WindowPlacement) -> Result<(), Win32Error> {
        placement.length = size_of::<WindowPlacement>() as u32;
        // SAFETY: `placement` is a live `&mut WINDOWPLACEMENT` with `length` set; the OS fills it and does not keep the pointer.
        unsafe { GetWindowPlacement(to_hwnd(hwnd), placement) }.map_err(|e| win32_error(&e))
    }

    /// C# `SetWindowPlacement`: the wrapper sets `length` on a copy.
    pub fn set_window_placement(hwnd: Hwnd, placement: &WindowPlacement) -> Result<(), Win32Error> {
        let mut copy = *placement;
        copy.length = size_of::<WindowPlacement>() as u32;
        // SAFETY: `copy` is a live local WINDOWPLACEMENT with `length` set; the OS only reads it.
        unsafe { SetWindowPlacement(to_hwnd(hwnd), &copy) }.map_err(|e| win32_error(&e))
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
        // SAFETY: handle values and integers; the OS validates the handles.
        unsafe {
            SetWindowPos(
                to_hwnd(hwnd),
                Some(to_hwnd(insert_after)),
                x,
                y,
                cx,
                cy,
                SET_WINDOW_POS_FLAGS(flags),
            )
        }
        .map_err(|e| win32_error(&e))
    }

    /// C# `GetWindowRect(hwnd, Rect*)`, returned by value.
    pub fn get_window_rect(hwnd: Hwnd) -> Result<Rect, Win32Error> {
        let mut rect = Rect::default();
        // SAFETY: `rect` is a live local RECT that the OS fills.
        unsafe { GetWindowRect(to_hwnd(hwnd), &mut rect) }.map_err(|e| win32_error(&e))?;
        Ok(rect)
    }

    /// C# `GetClientRect`.
    pub fn get_client_rect(hwnd: Hwnd) -> Result<Rect, Win32Error> {
        let mut rect = Rect::default();
        // SAFETY: `rect` is a live local RECT that the OS fills.
        unsafe { GetClientRect(to_hwnd(hwnd), &mut rect) }.map_err(|e| win32_error(&e))?;
        Ok(rect)
    }

    /// C# `GetWindowLongPtr(hwnd, index)` (`GWL_STYLE`, `GWL_EXSTYLE`, `GWLP_*`); `0` is also a valid value, so no `Result`.
    pub fn get_window_long_ptr(hwnd: Hwnd, index: i32) -> isize {
        // SAFETY: a handle value and an index; the OS validates both (0 on failure).
        unsafe { GetWindowLongPtrW(to_hwnd(hwnd), WINDOW_LONG_PTR_INDEX(index)) }
    }

    /// C# `SetWindowLongPtr`: the previous value.
    ///
    /// # Safety
    /// `GWLP_WNDPROC` needs a valid window procedure and `GWLP_USERDATA`/`DWLP_USER` data the window procedure interprets: the caller owns that contract.
    pub unsafe fn set_window_long_ptr(hwnd: Hwnd, index: i32, value: isize) -> isize {
        // SAFETY: upheld by the caller of this function.
        unsafe { SetWindowLongPtrW(to_hwnd(hwnd), WINDOW_LONG_PTR_INDEX(index), value) }
    }

    /// C# `GetCurrentInputMessageSource`: `None` when the call fails.
    pub fn get_current_input_message_source() -> Option<InputMessageSource> {
        let mut source = InputMessageSource::default();
        // SAFETY: `source` is a live local INPUT_MESSAGE_SOURCE that the OS fills.
        unsafe { GetCurrentInputMessageSource(&mut source) }
            .ok()
            .map(|()| source)
    }

    /// C# `OpenClipboard`.
    pub fn open_clipboard(owner: Hwnd) -> Result<(), Win32Error> {
        // SAFETY: a handle value (0 = the current task); the OS validates it.
        unsafe { OpenClipboard(Some(to_hwnd(owner))) }.map_err(|e| win32_error(&e))
    }

    /// C# `EmptyClipboard` (needs `open_clipboard` first).
    pub fn empty_clipboard() -> Result<(), Win32Error> {
        // SAFETY: no arguments.
        unsafe { EmptyClipboard() }.map_err(|e| win32_error(&e))
    }

    /// C# `SetClipboardData(format, memory)`: on success the system owns `memory`.
    ///
    /// # Safety
    /// `memory` is a moveable global memory handle (`GMEM_MOVEABLE`) whose content matches `format`; after `Ok` the caller must not free or lock it.
    pub unsafe fn set_clipboard_data(format: u32, memory: isize) -> Result<isize, Win32Error> {
        // SAFETY: upheld by the caller of this function (`memory` is a moveable HGLOBAL whose ownership passes to the system on success).
        unsafe { SetClipboardData(format, Some(HANDLE(memory as *mut c_void))) }
            .map(|h| h.0 as isize)
            .map_err(|e| win32_error(&e))
    }

    /// C# `GetClipboardData`: a handle owned by the clipboard (do not free it).
    pub fn get_clipboard_data(format: u32) -> Result<isize, Win32Error> {
        // SAFETY: a format id; the returned handle is owned by the clipboard and is not dereferenced here.
        unsafe { GetClipboardData(format) }
            .map(|h| h.0 as isize)
            .map_err(|e| win32_error(&e))
    }

    /// C# `CloseClipboard`.
    pub fn close_clipboard() -> Result<(), Win32Error> {
        // SAFETY: no arguments.
        unsafe { CloseClipboard() }.map_err(|e| win32_error(&e))
    }

    /// C# `CreatePopupMenu`.
    pub fn create_popup_menu() -> Result<Hmenu, Win32Error> {
        // SAFETY: no arguments.
        unsafe { CreatePopupMenu() }
            .map(|m| m.0 as isize)
            .map_err(|e| win32_error(&e))
    }

    /// C# `DestroyMenu`.
    pub fn destroy_menu(menu: Hmenu) -> Result<(), Win32Error> {
        // SAFETY: a handle value; the OS validates it.
        unsafe { DestroyMenu(HMENU(menu as *mut c_void)) }.map_err(|e| win32_error(&e))
    }

    /// C# `AppendMenu(menu, flags, itemId, newItem)`: `new_item` is the text for `MF_STRING` and `None` for `MF_SEPARATOR`; `item_id` is a command id or,
    /// with `MF_POPUP`, a sub-menu handle. Flags that change the meaning of `new_item` (`MF_BITMAP`, `MF_OWNERDRAW`) are not used by the shell.
    pub fn append_menu(menu: Hmenu, flags: u32, item_id: usize, new_item: Option<&str>) -> Result<(), Win32Error> {
        let text = new_item.map(to_utf16z);
        let text_ptr = text.as_ref().map_or(PCWSTR::null(), |t| PCWSTR(t.as_ptr()));
        // SAFETY: `text_ptr` is null (separator) or points into `text`, a NUL-terminated UTF-16 buffer in a local that lives past the call;
        // the OS copies the text for `MF_STRING`. `item_id` is a command id or a sub-menu handle the OS validates.
        unsafe { AppendMenuW(HMENU(menu as *mut c_void), MENU_ITEM_FLAGS(flags), item_id, text_ptr) }
            .map_err(|e| win32_error(&e))
    }

    /// C# `TrackPopupMenuEx(menu, flags, x, y, hwnd, 0)` (every caller passes a null `TPMPARAMS`): with `TPM_RETURNCMD` the chosen command id, `0` for none.
    pub fn track_popup_menu_ex(menu: Hmenu, flags: u32, x: i32, y: i32, hwnd: Hwnd) -> i32 {
        // SAFETY: handle values and integers; the OS validates the handles; the TPMPARAMS pointer is null.
        unsafe { TrackPopupMenuEx(HMENU(menu as *mut c_void), flags, x, y, to_hwnd(hwnd), None) }.0
    }
}

/// `kernel32.dll` (`Kernel32.cs`, `Kernel32Loader.cs`).
pub mod kernel32 {
    use super::*;
    use windows::Win32::Foundation::GlobalFree;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_FLAGS, LoadLibraryExW};
    use windows::Win32::System::Memory::{GLOBAL_ALLOC_FLAGS, GlobalAlloc, GlobalLock, GlobalUnlock};

    pub const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0800; // LoadLibrarySearchSystem32

    /// C# `GetModuleHandle(0)`: the handle of the running `.exe` (every caller passes `0`).
    pub fn get_module_handle() -> Result<Hinstance, Win32Error> {
        // SAFETY: a null module name selects the running executable.
        unsafe { GetModuleHandleW(PCWSTR::null()) }
            .map(|m| m.0 as isize)
            .map_err(|e| win32_error(&e))
    }

    /// C# `GlobalAlloc(flags, bytes)` (`GMEM_MOVEABLE | GMEM_ZEROINIT` for clipboard data).
    pub fn global_alloc(flags: u32, bytes: usize) -> Result<isize, Win32Error> {
        // SAFETY: plain integer arguments; the result is a movable-memory handle that is not dereferenced here.
        unsafe { GlobalAlloc(GLOBAL_ALLOC_FLAGS(flags), bytes) }
            .map(|h| h.0 as isize)
            .map_err(|e| win32_error(&e))
    }

    /// C# `GlobalLock`: the address of the block (null with `Err` on failure). The pointer is valid until the matching `global_unlock`.
    pub fn global_lock(memory: isize) -> Result<*mut core::ffi::c_void, Win32Error> {
        // SAFETY: a handle value the OS validates; the returned pointer is valid until the matching `global_unlock` and is not read here.
        let ptr = unsafe { GlobalLock(HGLOBAL(memory as *mut c_void)) };
        if ptr.is_null() {
            Err(Win32Error::last())
        } else {
            Ok(ptr)
        }
    }

    /// C# `GlobalUnlock`: `true` while the block is still locked. `false` is NOT an error when the lock count reached zero (`GetLastError()` is `NO_ERROR`).
    pub fn global_unlock(memory: isize) -> bool {
        // SAFETY: a handle value; the OS validates it. `Err` means the lock count reached zero (or an error): both are "not locked any more".
        unsafe { GlobalUnlock(HGLOBAL(memory as *mut c_void)) }.is_ok()
    }

    /// C# `GlobalFree`: `0` on success, the handle itself on failure.
    ///
    /// # Safety
    /// `memory` is not locked and not owned by the clipboard or an OLE data object any more.
    pub unsafe fn global_free(memory: isize) -> isize {
        // SAFETY: upheld by the caller of this function (the block is unlocked and not owned by the clipboard or an OLE data object).
        // The windows-rs wrapper maps the raw result "NULL on success, the handle on failure" to `Ok(handle)` for a non-null raw result and
        // `Err` for NULL, i.e. inverted: `Ok` here is the failure.
        match unsafe { GlobalFree(Some(HGLOBAL(memory as *mut c_void))) } {
            Ok(failed_handle) => failed_handle.0 as isize,
            Err(_) => 0,
        }
    }

    /// C# `LoadLibraryEx(fileName, 0, flags)` (Kernel32Loader; `LOAD_LIBRARY_SEARCH_SYSTEM32` loads from System32 only). The module is never freed
    /// (C# has no `FreeLibrary`): it is the dark-mode menu uxtheme, kept for the process lifetime.
    pub fn load_library_ex(file_name: &str, flags: u32) -> Result<Hinstance, Win32Error> {
        let name = to_utf16z(file_name);
        // SAFETY: `name` is a NUL-terminated UTF-16 buffer in a local that lives past the call.
        unsafe { LoadLibraryExW(PCWSTR(name.as_ptr()), None, LOAD_LIBRARY_FLAGS(flags)) }
            .map(|m| m.0 as isize)
            .map_err(|e| win32_error(&e))
    }

    /// C# `GetProcAddress(module, ordinal)`: the export with that ordinal (`MAKEINTRESOURCE`), `None` when missing. Calling the address is the caller's `unsafe`.
    pub fn get_proc_address_by_ordinal(module: Hinstance, ordinal: u16) -> Option<usize> {
        // SAFETY: a module handle the OS validates; the "name" is `MAKEINTRESOURCE(ordinal)`, an integer in a pointer the OS never dereferences as text.
        unsafe { GetProcAddress(HMODULE(module as *mut c_void), PCSTR(ordinal as usize as *const u8)) }
            .map(|f| f as usize)
    }
}

/// `gdi32.dll` (`Gdi32.cs`).
pub mod gdi32 {
    use super::*;
    use windows::Win32::Graphics::Gdi::{GET_STOCK_OBJECT_FLAGS, GetStockObject};

    pub const BLACK_BRUSH: i32 = 4; // BlackBrush

    /// C# `GetStockObject(index)` (`BLACK_BRUSH`...); a stock object must not be deleted.
    pub fn get_stock_object(index: i32) -> Hgdi {
        // SAFETY: plain integer argument; a stock object must not be deleted by the caller.
        unsafe { GetStockObject(GET_STOCK_OBJECT_FLAGS(index)) }.0 as isize
    }
}

/// `shell32.dll` (`Shell32.cs`).
pub mod shell32 {
    use super::*;
    use windows::Win32::UI::Shell::DragAcceptFiles;

    /// C# `DragAcceptFiles(hwnd, accept)`: the legacy `WM_DROPFILES` switch (the window also needs `WS_EX_ACCEPTFILES`).
    pub fn drag_accept_files(hwnd: Hwnd, accept: bool) {
        // SAFETY: a handle value; the OS validates it.
        unsafe { DragAcceptFiles(to_hwnd(hwnd), accept) }
    }
}

/// `shcore.dll` (`Shcore.cs`).
pub mod shcore {
    use super::*;
    use windows::Win32::Graphics::Gdi::HMONITOR;
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MONITOR_DPI_TYPE};

    pub const MDT_EFFECTIVE_DPI: i32 = 0; // MdtEffectiveDpi

    /// C# `GetDpiForMonitor(monitor, dpiType, &x, &y)`: `(dpi_x, dpi_y)` or the failing `HRESULT`.
    pub fn get_dpi_for_monitor(monitor: Hmonitor, dpi_type: i32) -> Result<(u32, u32), Hresult> {
        let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
        // SAFETY: both out pointers are live locals.
        unsafe {
            GetDpiForMonitor(
                HMONITOR(monitor as *mut c_void),
                MONITOR_DPI_TYPE(dpi_type),
                &mut dpi_x,
                &mut dpi_y,
            )
        }
        .map(|()| (dpi_x, dpi_y))
        .map_err(|e| Hresult(e.code().0))
    }
}

/// `dwmapi.dll` (`Dwmapi.cs`).
pub mod dwmapi {
    use super::*;
    use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};

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
        // SAFETY: `value` is a live `&T`; exactly `size_of::<T>()` bytes are passed and the OS only reads them.
        unsafe {
            DwmSetWindowAttribute(
                to_hwnd(hwnd),
                DWMWINDOWATTRIBUTE(attribute as i32),
                (value as *const T).cast::<c_void>(),
                size_of::<T>() as u32,
            )
        }
        .map_err(|e| Hresult(e.code().0))
    }
}

/// `ole32.dll` (`Ole32.cs`).
pub mod ole32 {
    use super::*;
    use windows::Win32::System::Ole::{IDropTarget, OleInitialize, OleUninitialize, RegisterDragDrop, RevokeDragDrop};

    /// C# `OleInitialize(0)`: `S_OK`/`S_FALSE` are `Ok` (both need one `ole_uninitialize`), a negative `HRESULT` is `Err`. Must run on the STA thread of the window.
    pub fn ole_initialize() -> Result<(), Hresult> {
        // SAFETY: the reserved argument is null; `S_OK` and `S_FALSE` both come back as `Ok`.
        unsafe { OleInitialize(None) }.map_err(|e| Hresult(e.code().0))
    }

    /// C# `OleUninitialize` (balanced with a successful `ole_initialize`).
    pub fn ole_uninitialize() {
        // SAFETY: no arguments; balanced with a successful `ole_initialize` by the caller.
        unsafe { OleUninitialize() }
    }

    /// C# `RegisterDragDrop(hwnd, IDropTarget*)`; the typed `IDropTarget` replaces the raw `nint` (W8-14 implements it with `#[implement]`).
    pub fn register_drag_drop(hwnd: Hwnd, drop_target: &IDropTarget) -> Result<(), Hresult> {
        // SAFETY: a handle value the OS validates; `drop_target` is a live COM reference that OLE AddRefs.
        unsafe { RegisterDragDrop(to_hwnd(hwnd), drop_target) }.map_err(|e| Hresult(e.code().0))
    }

    /// C# `RevokeDragDrop`.
    pub fn revoke_drag_drop(hwnd: Hwnd) -> Result<(), Hresult> {
        // SAFETY: a handle value; the OS validates it.
        unsafe { RevokeDragDrop(to_hwnd(hwnd)) }.map_err(|e| Hresult(e.code().0))
    }
}
