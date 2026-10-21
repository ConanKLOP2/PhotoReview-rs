//! Unit W8-11b tests: the `pr_platform_win::win32::api` wrappers (`User32`, `Kernel32`, `Kernel32Loader`, `Gdi32`, `Shell32`, `Shcore`,
//! `Dwmapi`, `Ole32`). Pure tests: constants against the `win32-constants` golden (written by W8-11a) and the signature shapes.
//! Native tests (`#[ignore = "native: needs desktop"]`): the four C# `InteropSignatureTests` `Native` methods plus message-only and hidden
//! windows created on the test's own thread. Every native test destroys what it creates; none touches the user's clipboard content, cursor,
//! capture or visible windows.

use core::ffi::c_void;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU32, Ordering};

use pr_platform_win::win32::api::{dwmapi, gdi32, kernel32, ole32, shcore, user32};
use pr_platform_win::win32::messages::*;
use pr_platform_win::win32::structs::*;
use pr_test_support::golden::{self, i64_of, str_of};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::core::PCWSTR;

// ---------------------------------------------------------------- pure tests

/// The golden constants of one class as `name -> value`.
fn golden_class(class: &str) -> Vec<(String, i64)> {
    golden::load("win32-constants")
        .cases
        .iter()
        .filter(|c| str_of(c, "kind") == "const" && str_of(c, "class") == class)
        .map(|c| (str_of(c, "name").to_owned(), i64_of(c, "value")))
        .collect()
}

fn assert_class(class: &str, table: &[(&str, i64)]) {
    let golden = golden_class(class);
    let golden_names: BTreeSet<&str> = golden.iter().map(|(n, _)| n.as_str()).collect();
    let table_names: BTreeSet<&str> = table.iter().map(|(n, _)| *n).collect();
    assert_eq!(table_names, golden_names, "{class}: a missed or invented constant");
    for (name, value) in table {
        let want = golden.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
        assert_eq!(want, Some(*value), "{class}.{name}");
    }
}

#[test]
fn golden_api_constants_equal_the_csharp_constants() {
    assert_class(
        "Kernel32",
        &[(
            "LoadLibrarySearchSystem32",
            kernel32::LOAD_LIBRARY_SEARCH_SYSTEM32 as i64,
        )],
    );
    assert_class("Gdi32", &[("BlackBrush", gdi32::BLACK_BRUSH as i64)]);
    assert_class("Shcore", &[("MdtEffectiveDpi", shcore::MDT_EFFECTIVE_DPI as i64)]);
    assert_class(
        "Dwmapi",
        &[
            ("DwmwaNcRenderingPolicy", dwmapi::DWMWA_NCRENDERING_POLICY as i64),
            (
                "DwmwaTransitionsForceDisabled",
                dwmapi::DWMWA_TRANSITIONS_FORCEDISABLED as i64,
            ),
            ("DwmwaCloak", dwmapi::DWMWA_CLOAK as i64),
            (
                "DwmwaUseImmersiveDarkMode",
                dwmapi::DWMWA_USE_IMMERSIVE_DARK_MODE as i64,
            ),
            (
                "DwmwaWindowCornerPreference",
                dwmapi::DWMWA_WINDOW_CORNER_PREFERENCE as i64,
            ),
            ("DwmwaBorderColor", dwmapi::DWMWA_BORDER_COLOR as i64),
            ("DwmwaCaptionColor", dwmapi::DWMWA_CAPTION_COLOR as i64),
        ],
    );
    // `User32` has 9 constants; `DpiAwarenessContextPerMonitorAwareV2` (-4) belongs to W8-11e.
    let user32_golden: Vec<(String, i64)> = golden_class("User32")
        .into_iter()
        .filter(|(n, _)| n != "DpiAwarenessContextPerMonitorAwareV2")
        .collect();
    let table: Vec<(&str, i64)> = vec![
        ("WmCreate", user32::WM_CREATE as i64),
        ("WmDestroy", user32::WM_DESTROY as i64),
        ("WmClose", user32::WM_CLOSE as i64),
        ("WsOverlappedWindow", user32::WS_OVERLAPPEDWINDOW as i64),
        ("CwUseDefault", user32::CW_USEDEFAULT as i64),
        ("SwShowNoActivate", user32::SW_SHOWNOACTIVATE as i64),
        ("SwShowDefault", user32::SW_SHOWDEFAULT as i64),
        ("IdcArrow", user32::IDC_ARROW as i64),
    ];
    assert_eq!(user32_golden.len(), table.len());
    for (name, value) in &table {
        let want = user32_golden.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
        assert_eq!(want, Some(*value), "User32.{name}");
    }
    // The aliases are the single definitions of `messages` (trap T6).
    assert_eq!(user32::WM_CREATE, WM_CREATE);
    assert_eq!(user32::WS_OVERLAPPEDWINDOW, WS_OVERLAPPEDWINDOW);
    // Trap T2: `CW_USEDEFAULT` is `i32::MIN`, not `0x8000_0000u32`.
    assert_eq!(user32::CW_USEDEFAULT, i32::MIN);
}

/// Assigning each safe wrapper to a typed function pointer pins its signature: a change fails to compile.
#[test]
#[allow(clippy::type_complexity)] // the long fn-pointer types are the point of this test: they pin the wrapper signatures
fn api_functions_have_the_documented_shapes() {
    let _: unsafe fn(&WndClassEx) -> Result<u16, Win32Error> = user32::register_class_ex;
    let _: fn(&str, Hinstance) -> Result<(), Win32Error> = user32::unregister_class;
    let _: unsafe fn(
        u32,
        &str,
        &str,
        u32,
        i32,
        i32,
        i32,
        i32,
        Hwnd,
        Hmenu,
        Hinstance,
        isize,
    ) -> Result<Hwnd, Win32Error> = user32::create_window_ex;
    let _: unsafe fn(Hwnd, u32, Wparam, Lparam) -> Lresult = user32::def_window_proc;
    let _: fn(Hwnd, i32) -> bool = user32::show_window;
    let _: fn(Hwnd) -> bool = user32::is_window_visible;
    let _: fn(Hwnd) -> Result<(), Win32Error> = user32::destroy_window;
    let _: unsafe fn(Hwnd, u32, Wparam, Lparam) -> Result<(), Win32Error> = user32::post_message;
    let _: fn(i32) = user32::post_quit_message;
    let _: fn(&mut Msg, Hwnd, u32, u32) -> user32::MessageOutcome = user32::get_message;
    let _: fn(&mut Msg, Hwnd, u32, u32, u32) -> bool = user32::peek_message;
    let _: fn(&Msg) -> bool = user32::translate_message;
    let _: unsafe fn(&Msg) -> Lresult = user32::dispatch_message;
    let _: fn(&[isize], u32, u32, u32) -> u32 = user32::msg_wait_for_multiple_objects_ex;
    let _: fn(i32) -> Result<Hcursor, Win32Error> = user32::load_system_cursor;
    let _: fn(Hcursor) -> Hcursor = user32::set_cursor;
    let _: fn(Hwnd) -> Hwnd = user32::set_capture;
    let _: fn() -> bool = user32::release_capture;
    let _: fn(Hwnd) -> u32 = user32::get_dpi_for_window;
    let _: fn() -> u32 = user32::get_dpi_for_system;
    let _: fn(i32) -> i32 = user32::get_system_metrics;
    let _: fn(i32, u32) -> i32 = user32::get_system_metrics_for_dpi;
    let _: fn(&mut Rect, u32, bool, u32, u32) -> Result<(), Win32Error> = user32::adjust_window_rect_ex_for_dpi;
    let _: fn() -> isize = user32::get_thread_dpi_awareness_context;
    let _: fn(isize) -> i32 = user32::get_awareness_from_dpi_awareness_context;
    let _: fn(Hwnd, u32) -> Hmonitor = user32::monitor_from_window;
    let _: fn(&Rect, u32) -> Hmonitor = user32::monitor_from_rect;
    let _: fn(Point, u32) -> Hmonitor = user32::monitor_from_point;
    let _: fn(Hmonitor, &mut MonitorInfo) -> Result<(), Win32Error> = user32::get_monitor_info;
    let _: fn(Hmonitor, &mut MonitorInfoEx) -> Result<(), Win32Error> = user32::get_monitor_info_ex;
    let _: fn(Option<&Rect>) -> Result<Vec<user32::MonitorEnumItem>, Win32Error> = user32::enum_display_monitors;
    let _: fn(Hwnd, &mut WindowPlacement) -> Result<(), Win32Error> = user32::get_window_placement;
    let _: fn(Hwnd, &WindowPlacement) -> Result<(), Win32Error> = user32::set_window_placement;
    let _: fn(Hwnd, Hwnd, i32, i32, i32, i32, u32) -> Result<(), Win32Error> = user32::set_window_pos;
    let _: fn(Hwnd) -> Result<Rect, Win32Error> = user32::get_window_rect;
    let _: fn(Hwnd) -> Result<Rect, Win32Error> = user32::get_client_rect;
    let _: fn(Hwnd, i32) -> isize = user32::get_window_long_ptr;
    let _: unsafe fn(Hwnd, i32, isize) -> isize = user32::set_window_long_ptr;
    let _: fn() -> Option<InputMessageSource> = user32::get_current_input_message_source;
    let _: fn(Hwnd) -> Result<(), Win32Error> = user32::open_clipboard;
    let _: fn() -> Result<(), Win32Error> = user32::empty_clipboard;
    let _: unsafe fn(u32, isize) -> Result<isize, Win32Error> = user32::set_clipboard_data;
    let _: fn(u32) -> Result<isize, Win32Error> = user32::get_clipboard_data;
    let _: fn() -> Result<(), Win32Error> = user32::close_clipboard;
    let _: fn() -> Result<Hmenu, Win32Error> = user32::create_popup_menu;
    let _: fn(Hmenu) -> Result<(), Win32Error> = user32::destroy_menu;
    let _: fn(Hmenu, u32, usize, Option<&str>) -> Result<(), Win32Error> = user32::append_menu;
    let _: fn(Hmenu, u32, i32, i32, Hwnd) -> i32 = user32::track_popup_menu_ex;

    let _: fn() -> Result<Hinstance, Win32Error> = kernel32::get_module_handle;
    let _: fn(u32, usize) -> Result<isize, Win32Error> = kernel32::global_alloc;
    let _: fn(isize) -> Result<*mut c_void, Win32Error> = kernel32::global_lock;
    let _: fn(isize) -> bool = kernel32::global_unlock;
    let _: unsafe fn(isize) -> isize = kernel32::global_free;
    let _: fn(&str, u32) -> Result<Hinstance, Win32Error> = kernel32::load_library_ex;
    let _: fn(Hinstance, u16) -> Option<usize> = kernel32::get_proc_address_by_ordinal;

    let _: fn(i32) -> Hgdi = gdi32::get_stock_object;
    let _: fn(Hmonitor, i32) -> Result<(u32, u32), Hresult> = shcore::get_dpi_for_monitor;
    let _: fn() -> Result<(), Hresult> = ole32::ole_initialize;
    let _: fn() = ole32::ole_uninitialize;
    let _: fn(Hwnd) -> Result<(), Hresult> = ole32::revoke_drag_drop;
}

// ------------------------------------------------------------- native helpers

const E_INVALIDARG: i32 = 0x8007_0057_u32 as i32;
static COUNTER: AtomicU32 = AtomicU32::new(0);

thread_local! {
    /// `(message, wparam, lparam)` of every `WM_APP` the test window procedure of this thread received.
    static RECORDER: RefCell<Vec<(u32, usize, isize)>> = const { RefCell::new(Vec::new()) };
}

/// The window procedure of the test classes: records `WM_APP`, forwards everything else to `def_window_proc`.
unsafe extern "system" fn test_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_APP {
        RECORDER.with(|r| r.borrow_mut().push((message, wparam.0, lparam.0)));
        return LRESULT(0);
    }
    // SAFETY: called with the parameters this window procedure received.
    LRESULT(unsafe { user32::def_window_proc(hwnd.0 as isize, message, wparam.0, lparam.0) })
}

fn unique_class_name(test: &str) -> String {
    format!(
        "PhotoReviewRsTest_{}_{}_{test}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn instance() -> Hinstance {
    kernel32::get_module_handle().expect("GetModuleHandle(null)")
}

/// Registers `name` with `test_proc`; returns the atom.
fn register(name: &str) -> u16 {
    let wide = to_utf16z(name);
    let mut class = new_wnd_class_ex();
    class.lpfnWndProc = Some(test_proc);
    class.hInstance = HINSTANCE(instance() as *mut c_void);
    class.lpszClassName = PCWSTR(wide.as_ptr());
    // SAFETY: `lpszClassName` points into `wide`, a NUL-terminated UTF-16 buffer that lives past the call (the OS copies the name), and
    // `test_proc` is a valid window procedure for the whole process.
    unsafe { user32::register_class_ex(&class) }.expect("RegisterClassEx")
}

/// A registered class that is unregistered on drop, and the windows created from it that are destroyed on drop (newest first).
struct TestClass {
    name: String,
    windows: Vec<Hwnd>,
}

impl TestClass {
    fn new(test: &str) -> Self {
        let name = unique_class_name(test);
        let atom = register(&name);
        assert_ne!(atom, 0);
        Self {
            name,
            windows: Vec::new(),
        }
    }

    fn create(&mut self, ex_style: u32, style: u32, parent: Hwnd) -> Hwnd {
        // SAFETY: the class's window procedure is `test_proc`, which accepts a zero creation parameter.
        let hwnd = unsafe {
            user32::create_window_ex(
                ex_style,
                &self.name,
                "PhotoReviewRs W8-11b test",
                style,
                user32::CW_USEDEFAULT,
                user32::CW_USEDEFAULT,
                user32::CW_USEDEFAULT,
                user32::CW_USEDEFAULT,
                parent,
                0,
                instance(),
                0,
            )
        }
        .expect("CreateWindowEx");
        assert_ne!(hwnd, 0);
        self.windows.push(hwnd);
        hwnd
    }
}

impl Drop for TestClass {
    fn drop(&mut self) {
        for hwnd in self.windows.drain(..).rev() {
            let _ = user32::destroy_window(hwnd); // already destroyed by a test: ignore
        }
        let _ = user32::unregister_class(&self.name, instance());
    }
}

const HWND_MESSAGE: Hwnd = -3;

// ---------------------------------------------------------------- native tests

#[test]
#[ignore = "native: needs desktop"]
fn get_dpi_for_system_call_returns_at_least_96() {
    // C#: InteropSignatureTests.GetDpiForSystem_Call_ReturnsAtLeast96
    assert!(user32::get_dpi_for_system() >= 96);
}

#[test]
#[ignore = "native: needs desktop"]
fn get_system_metrics_calls_return_positive_drag_threshold() {
    // C#: InteropSignatureTests.GetSystemMetrics_Calls_ReturnPositiveDragThreshold
    assert!(user32::get_system_metrics(SM_CXDRAG) > 0);
    assert!(user32::get_system_metrics_for_dpi(SM_CXDRAG, 96) > 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn monitor_from_point_and_get_monitor_info_ex_primary_monitor_fills_device_name() {
    // C#: InteropSignatureTests.MonitorFromPointAndGetMonitorInfoEx_PrimaryMonitor_FillsDeviceName
    let monitor = user32::monitor_from_point(Point { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
    assert_ne!(monitor, 0);
    let mut info = new_monitor_info_ex();
    user32::get_monitor_info_ex(monitor, &mut info).expect("GetMonitorInfo");
    assert_ne!(info.szDevice[0], 0, "the device name is filled");
    assert_ne!(info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY, 0);
    assert!(utf16_to_string(&info.szDevice).starts_with("\\\\.\\DISPLAY"));
}

#[test]
#[ignore = "native: needs desktop"]
fn register_class_ex_then_unregister_class_round_trips() {
    // C#: InteropSignatureTests.RegisterClassEx_ThenUnregisterClass_RoundTrips
    let name = unique_class_name("roundtrip");
    let atom = register(&name);
    assert_ne!(atom, 0);
    assert_eq!(user32::unregister_class(&name, instance()), Ok(()));
    // Trap T9: unregistering twice is ERROR_CLASS_DOES_NOT_EXIST (1411).
    assert_eq!(user32::unregister_class(&name, instance()), Err(Win32Error(1411)));
}

#[test]
#[ignore = "native: needs desktop"]
fn create_window_ex_message_only_window_round_trips() {
    let mut class = TestClass::new("msgonly");
    let hwnd = class.create(0, 0, HWND_MESSAGE);
    assert!(!user32::is_window_visible(hwnd));
    let rect = user32::get_window_rect(hwnd).expect("GetWindowRect");
    assert!(rect.right >= rect.left && rect.bottom >= rect.top);
    assert_eq!(user32::destroy_window(hwnd), Ok(()));
    // The window is gone: ERROR_INVALID_WINDOW_HANDLE (1400).
    assert_eq!(user32::destroy_window(hwnd), Err(Win32Error(1400)));
}

#[test]
#[ignore = "native: needs desktop"]
fn post_message_get_message_dispatch_delivers_to_the_window_procedure() {
    let mut class = TestClass::new("post");
    let hwnd = class.create(0, 0, HWND_MESSAGE);
    RECORDER.with(|r| r.borrow_mut().clear());

    // SAFETY: `WM_APP` carries plain integers, no pointers.
    unsafe { user32::post_message(hwnd, WM_APP, 7, 9) }.expect("PostMessage");
    let mut msg = Msg::default();
    // The message was posted by this test, so the filtered `get_message` returns at once (hang guard).
    assert_eq!(
        user32::get_message(&mut msg, hwnd, WM_APP, WM_APP),
        user32::MessageOutcome::Message
    );
    assert_eq!(msg.message, WM_APP);
    let _ = user32::translate_message(&msg); // WM_APP is not a key message: nothing to translate
    // SAFETY: `msg` came from `get_message`.
    unsafe { user32::dispatch_message(&msg) };
    assert_eq!(RECORDER.with(|r| r.borrow().clone()), vec![(WM_APP, 7, 9)]);

    // `post_quit_message` posts WM_QUIT to this thread; the exit code arrives in `msg.wParam` (trap T7). Other posted messages, if any,
    // come first, so the loop is bounded.
    user32::post_quit_message(7);
    let mut quit = false;
    for _ in 0..100 {
        match user32::get_message(&mut msg, 0, 0, 0) {
            user32::MessageOutcome::Quit => {
                quit = true;
                break;
            }
            user32::MessageOutcome::Message => {
                // SAFETY: `msg` came from `get_message`.
                unsafe { user32::dispatch_message(&msg) };
            }
            user32::MessageOutcome::Failed(error) => panic!("GetMessage failed: {error}"),
        }
    }
    assert!(quit, "WM_QUIT was not retrieved");
    assert_eq!(msg.wParam.0, 7);
}

#[test]
#[ignore = "native: needs desktop"]
fn get_message_with_a_bad_window_fails() {
    // Trap T3: -1 must not be reported as a message.
    let mut msg = Msg::default();
    let outcome = user32::get_message(&mut msg, 0x7FFF_FFF0, 0, 0);
    assert!(matches!(outcome, user32::MessageOutcome::Failed(_)), "{outcome:?}");
}

#[test]
#[ignore = "native: needs desktop"]
fn peek_message_on_an_empty_queue_returns_false() {
    let mut msg = Msg::default();
    assert!(!user32::peek_message(&mut msg, 0, WM_APP, WM_APP, PM_REMOVE));
}

#[test]
#[ignore = "native: needs desktop"]
fn msg_wait_for_multiple_objects_ex_times_out_without_input_and_wakes_for_a_posted_message() {
    use windows::Win32::System::Threading::{CreateEventW, SetEvent};
    let mut class = TestClass::new("wait");
    let hwnd = class.create(0, 0, HWND_MESSAGE);
    // SAFETY: a manual-reset event without a name or security attributes; closed at the end of the test.
    let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.expect("CreateEvent");
    let handles = [event.0 as isize];

    // Clear the "new input" state of this thread's queue.
    let mut msg = Msg::default();
    while user32::peek_message(&mut msg, 0, 0, 0, PM_REMOVE) {}

    assert_eq!(
        user32::msg_wait_for_multiple_objects_ex(&handles, 0, QS_ALLINPUT, 0),
        0x102,
        "WAIT_TIMEOUT"
    );

    // SAFETY: `WM_APP` carries plain integers.
    unsafe { user32::post_message(hwnd, WM_APP, 1, 2) }.expect("PostMessage");
    assert_eq!(
        user32::msg_wait_for_multiple_objects_ex(&handles, 1000, QS_ALLINPUT, 0),
        handles.len() as u32,
        "WAIT_OBJECT_0 + count when input is available"
    );
    while user32::peek_message(&mut msg, 0, 0, 0, PM_REMOVE) {}

    // SAFETY: `event` is the live event created above.
    unsafe { SetEvent(event) }.expect("SetEvent");
    assert_eq!(
        user32::msg_wait_for_multiple_objects_ex(&handles, 1000, QS_ALLINPUT, 0),
        0,
        "WAIT_OBJECT_0 + 0"
    );
    // Trap T6: an empty handle list passes count 0 and a null pointer.
    assert_eq!(user32::msg_wait_for_multiple_objects_ex(&[], 0, QS_ALLINPUT, 0), 0x102);
    // SAFETY: the handle is owned by this test and not used afterwards.
    unsafe { windows::Win32::Foundation::CloseHandle(event) }.expect("CloseHandle");
}

#[test]
#[ignore = "native: needs desktop"]
fn window_long_ptr_user_data_round_trips() {
    let mut class = TestClass::new("userdata");
    let hwnd = class.create(0, 0, HWND_MESSAGE);
    // SAFETY: `test_proc` never interprets `GWLP_USERDATA`.
    assert_eq!(unsafe { user32::set_window_long_ptr(hwnd, GWLP_USERDATA, 0x1234) }, 0);
    assert_eq!(user32::get_window_long_ptr(hwnd, GWLP_USERDATA), 0x1234);
    // SAFETY: as above; clears the value again.
    assert_eq!(unsafe { user32::set_window_long_ptr(hwnd, GWLP_USERDATA, 0) }, 0x1234);
}

#[test]
#[ignore = "native: needs desktop"]
fn set_window_pos_moves_a_top_level_window() {
    let mut class = TestClass::new("pos");
    // Never shown: no WS_VISIBLE and no ShowWindow.
    let hwnd = class.create(0, WS_OVERLAPPEDWINDOW, 0);
    assert!(!user32::is_window_visible(hwnd));
    assert_eq!(
        user32::get_window_long_ptr(hwnd, GWL_STYLE) as u32 & WS_OVERLAPPEDWINDOW,
        WS_OVERLAPPEDWINDOW
    );
    user32::set_window_pos(hwnd, 0, 10, 20, 300, 200, SWP_NOACTIVATE | SWP_NOZORDER).expect("SetWindowPos");
    let rect = user32::get_window_rect(hwnd).expect("GetWindowRect");
    assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (10, 20, 310, 220));
    let client = user32::get_client_rect(hwnd).expect("GetClientRect");
    assert_eq!((client.left, client.top), (0, 0));
    assert!(
        client.right < 300 && client.bottom < 200,
        "the client area is smaller than the window"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn window_placement_round_trips_normal_position() {
    let mut class = TestClass::new("placement");
    let hwnd = class.create(0, WS_OVERLAPPEDWINDOW, 0);
    let mut placement = new_window_placement();
    user32::get_window_placement(hwnd, &mut placement).expect("GetWindowPlacement");
    assert_eq!(placement.length, 44);
    placement.rcNormalPosition = Rect {
        left: 50,
        top: 60,
        right: 350,
        bottom: 260,
    };
    // SW_HIDE keeps the window invisible: SetWindowPlacement applies `showCmd`.
    placement.showCmd = 0;
    user32::set_window_placement(hwnd, &placement).expect("SetWindowPlacement");
    let mut back = new_window_placement();
    user32::get_window_placement(hwnd, &mut back).expect("GetWindowPlacement");
    let rect = back.rcNormalPosition;
    assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (50, 60, 350, 260));
    assert!(!user32::is_window_visible(hwnd));
}

#[test]
#[ignore = "native: needs desktop"]
fn adjust_window_rect_ex_for_dpi_96_grows_a_client_rect_symmetrically_left_and_right() {
    let mut rect = Rect {
        left: 0,
        top: 0,
        right: 800,
        bottom: 600,
    };
    user32::adjust_window_rect_ex_for_dpi(&mut rect, WS_OVERLAPPEDWINDOW, false, 0, 96)
        .expect("AdjustWindowRectExForDpi");
    assert!(
        rect.left < 0 && rect.top < rect.left,
        "the caption makes the top grow more than the sides"
    );
    assert!(rect.right > 800 && rect.bottom > 600);
    assert_eq!(-rect.left, rect.right - 800, "left and right borders are equal");
}

#[test]
#[ignore = "native: needs desktop"]
fn get_monitor_info_sets_cbsize_itself() {
    let monitor = user32::monitor_from_point(Point { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
    assert_ne!(monitor, 0);
    // Built with `cbSize = 0` on purpose (D-W8B3-6).
    let mut info = MonitorInfo::default();
    user32::get_monitor_info(monitor, &mut info).expect("GetMonitorInfo");
    assert_eq!(info.cbSize, 40);
    assert_ne!(info.dwFlags & MONITORINFOF_PRIMARY, 0);
    assert!(info.rcMonitor.right > info.rcMonitor.left);
    let mut ex = MonitorInfoEx::default();
    user32::get_monitor_info_ex(monitor, &mut ex).expect("GetMonitorInfo (ex)");
    assert_eq!(ex.monitorInfo.cbSize, 104);
    assert_ne!(ex.szDevice[0], 0);
    // An invalid monitor fails with the OS error.
    assert!(user32::get_monitor_info(0, &mut info).is_err());
}

#[test]
#[ignore = "native: needs desktop"]
fn enum_display_monitors_lists_the_primary_monitor() {
    let items = user32::enum_display_monitors(None).expect("EnumDisplayMonitors");
    assert!(!items.is_empty());
    let mut primary = 0;
    for item in &items {
        let mut info = new_monitor_info();
        user32::get_monitor_info(item.monitor, &mut info).expect("GetMonitorInfo");
        assert_eq!(info.rcMonitor, item.rect);
        if info.dwFlags & MONITORINFOF_PRIMARY != 0 {
            primary += 1;
        }
    }
    assert_eq!(primary, 1);
    let huge = Rect {
        left: -1_000_000,
        top: -1_000_000,
        right: 1_000_000,
        bottom: 1_000_000,
    };
    let clipped = user32::enum_display_monitors(Some(&huge)).expect("EnumDisplayMonitors(clip)");
    assert_eq!(clipped.len(), items.len());
}

#[test]
#[ignore = "native: needs desktop"]
fn get_dpi_for_monitor_effective_dpi_is_at_least_96() {
    let monitor = user32::monitor_from_point(Point { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
    let (x, y) = shcore::get_dpi_for_monitor(monitor, shcore::MDT_EFFECTIVE_DPI).expect("GetDpiForMonitor");
    assert!(x >= 96 && y >= 96, "{x}x{y}");
}

#[test]
#[ignore = "native: needs desktop"]
fn create_popup_menu_append_string_and_separator_then_destroy() {
    let menu = user32::create_popup_menu().expect("CreatePopupMenu");
    assert_ne!(menu, 0);
    // `track_popup_menu_ex` is modal and is not called here.
    assert_eq!(user32::append_menu(menu, MF_STRING, 1, Some("Item")), Ok(()));
    assert_eq!(user32::append_menu(menu, MF_SEPARATOR, 0, None), Ok(()));
    assert_eq!(user32::destroy_menu(menu), Ok(()));
    assert!(
        user32::destroy_menu(menu).is_err(),
        "a destroyed menu handle is invalid"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn open_clipboard_then_close_round_trips() {
    // Never `empty_clipboard`: it would erase the user's clipboard. Another process (a clipboard manager) may hold the clipboard for a
    // moment (ERROR_ACCESS_DENIED = 5): retry without sleeping, then skip rather than fail on an environment condition.
    let mut opened = false;
    for _ in 0..200 {
        match user32::open_clipboard(0) {
            Ok(()) => {
                opened = true;
                break;
            }
            Err(Win32Error(5)) => std::thread::yield_now(),
            Err(other) => panic!("OpenClipboard failed: {other}"),
        }
    }
    if !opened {
        eprintln!("skipped: the clipboard stayed locked by another process");
        return;
    }
    assert_eq!(user32::close_clipboard(), Ok(()));
    // Closing twice: the clipboard is not open (ERROR_CLIPBOARD_NOT_OPEN = 1418).
    assert_eq!(user32::close_clipboard(), Err(Win32Error(1418)));
}

#[test]
#[ignore = "native: needs desktop"]
fn global_alloc_lock_unlock_free_round_trips() {
    let handle = kernel32::global_alloc(GMEM_MOVEABLE | GMEM_ZEROINIT, 16).expect("GlobalAlloc");
    assert_ne!(handle, 0);
    let pointer = kernel32::global_lock(handle).expect("GlobalLock");
    assert!(!pointer.is_null());
    // SAFETY: the block is 16 zero-initialised bytes and stays locked until `global_unlock` below.
    let bytes = unsafe { core::slice::from_raw_parts(pointer.cast::<u8>(), 16) };
    assert!(bytes.iter().all(|&b| b == 0));
    // Trap T4: `false` = the lock count reached zero, and that is not an error.
    assert!(!kernel32::global_unlock(handle));
    assert_eq!(Win32Error::last(), Win32Error(0));
    // SAFETY: the block is unlocked and owned by this test only.
    assert_eq!(unsafe { kernel32::global_free(handle) }, 0, "0 on success");
}

#[test]
#[ignore = "native: needs desktop"]
fn get_module_handle_is_not_zero() {
    assert_ne!(kernel32::get_module_handle().expect("GetModuleHandle"), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn kernel32_loader_loads_uxtheme_from_system32_and_finds_ordinal_135() {
    // C#: NativeDialogChromeTests (LoadLibraryEx("uxtheme.dll", 0, LoadLibrarySearchSystem32) + GetProcAddress(module, 135))
    let module =
        kernel32::load_library_ex("uxtheme.dll", kernel32::LOAD_LIBRARY_SEARCH_SYSTEM32).expect("LoadLibraryEx");
    assert_ne!(module, 0);
    match kernel32::get_proc_address_by_ordinal(module, 135) {
        Some(address) => assert_ne!(address, 0),
        None => eprintln!("skipped: uxtheme ordinal 135 (SetPreferredAppMode) is missing on a Windows older than 1903"),
    }
    assert_eq!(kernel32::get_proc_address_by_ordinal(module, 60_000), None);
    assert!(
        kernel32::load_library_ex("PhotoReviewRs-no-such-dll.dll", kernel32::LOAD_LIBRARY_SEARCH_SYSTEM32).is_err()
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn get_stock_object_black_brush_is_not_zero() {
    assert_ne!(gdi32::get_stock_object(gdi32::BLACK_BRUSH), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn load_system_cursor_arrow_is_not_zero() {
    // `set_cursor`/`set_capture` are not called: they would change the user's real cursor and mouse capture.
    assert_ne!(user32::load_system_cursor(user32::IDC_ARROW).expect("LoadCursor"), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn dpi_awareness_context_of_the_thread_is_a_known_awareness() {
    let context = user32::get_thread_dpi_awareness_context();
    let awareness = user32::get_awareness_from_dpi_awareness_context(context);
    assert!((0..=2).contains(&awareness), "DPI_AWARENESS {awareness}");
    assert_eq!(
        user32::get_awareness_from_dpi_awareness_context(0),
        -1,
        "an invalid context"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn ole_initialize_then_uninitialize_on_an_sta_thread() {
    std::thread::spawn(|| {
        // S_OK and S_FALSE are both `Ok`.
        assert_eq!(ole32::ole_initialize(), Ok(()));
        ole32::ole_uninitialize();
    })
    .join()
    .expect("the OLE thread panicked");
}

#[test]
#[ignore = "native: needs desktop"]
fn dwm_set_window_attribute_dark_mode_returns_ok_or_invalid_arg_on_a_top_level_window() {
    let mut class = TestClass::new("dwm");
    let hwnd = class.create(0, WS_OVERLAPPEDWINDOW, 0);
    match dwmapi::dwm_set_window_attribute(hwnd, dwmapi::DWMWA_USE_IMMERSIVE_DARK_MODE, &1i32) {
        Ok(()) => {}
        Err(Hresult(code)) => assert_eq!(
            code, E_INVALIDARG,
            "an old Windows rejects the attribute with E_INVALIDARG"
        ),
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn dwm_set_window_attribute_passes_the_size_of_t() {
    let mut class = TestClass::new("dwmsize");
    let hwnd = class.create(0, WS_OVERLAPPEDWINDOW, 0);
    // A 1-byte value for a 4-byte attribute is rejected by the OS (E_INVALIDARG), proving that `size_of::<T>()` is what is passed.
    assert_eq!(
        dwmapi::dwm_set_window_attribute(hwnd, dwmapi::DWMWA_USE_IMMERSIVE_DARK_MODE, &1u8),
        Err(Hresult(E_INVALIDARG))
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn get_current_input_message_source_does_not_crash() {
    // Succeeds on a thread that has processed input; on this test thread it may be `None`. Either way the call must be safe.
    let _ = user32::get_current_input_message_source();
}
