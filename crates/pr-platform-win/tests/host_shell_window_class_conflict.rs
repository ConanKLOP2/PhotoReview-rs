//! Test of W8-13e (open question 4 of the contract): `ShellWindow::create` reports `RegisterClassEx (shell window) failed` when the class name is
//! already taken. The class registration is process-wide state, so this is the only test of its own test binary (any other shell-window test in the
//! same process would see the conflicting class). No window is created and no desktop is needed.
#![cfg(windows)]

use std::rc::Rc;

use pr_platform_win::host::error::HostError;
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::host::frame_clock::timer::TimerFrameClock;
use pr_platform_win::host::shell_window::create::CLASS_NAME;
use pr_platform_win::host::shell_window::{ShellWindow, ShellWindowOptions};
use pr_platform_win::win32::api::kernel32;
use pr_platform_win::win32::structs::to_utf16z;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, RegisterClassExW, WNDCLASSEXW};
use windows::core::PCWSTR;

unsafe extern "system" fn plain_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: forwards the parameters this procedure received.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

// new: ERROR_CLASS_ALREADY_EXISTS (1410) from RegisterClassEx; a failure is not remembered, so the second attempt fails the same way
#[test]
fn create_with_a_class_name_that_is_taken_reports_the_register_class_error() {
    let name = to_utf16z(CLASS_NAME);
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(plain_proc),
        hInstance: HINSTANCE(kernel32::get_module_handle().unwrap() as *mut core::ffi::c_void),
        lpszClassName: PCWSTR(name.as_ptr()),
        ..Default::default()
    };
    // SAFETY: `class` is a live, correctly sized WNDCLASSEXW; `name` outlives the call; the procedure is valid for the process.
    assert_ne!(unsafe { RegisterClassExW(&class) }, 0);

    for _ in 0..2 {
        let clock: Rc<dyn HostFrameClock> = Rc::new(TimerFrameClock::create(None, 0).unwrap());
        let error = ShellWindow::create(ShellWindowOptions::default(), clock).unwrap_err();
        match error {
            HostError::Win32 { operation, error } => {
                assert_eq!(operation, "RegisterClassEx (shell window) failed");
                assert_eq!(error.0, 1410);
            }
            other => panic!("unexpected error {other:?}"),
        }
    }
}
