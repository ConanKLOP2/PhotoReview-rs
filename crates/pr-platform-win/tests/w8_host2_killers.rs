//! Mutation killers of lane cl-host2 (2026-10-30): `host/wndproc.rs` (`make_cookie`, `creation_cookie`). The `ui_dispatcher.rs` and `dpi.rs` mutants
//! were all caught by the existing suites, so they need no killer here. No window is shown: only a message-only window of a private class.
//! No sleeps, no wall clock.
#![cfg(windows)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Once;

use pr_platform_win::host::wndproc::{WindowTarget, free_cookie, make_cookie, shell_window_proc};
use pr_platform_win::win32::api::{kernel32, user32};
use pr_platform_win::win32::messages::{GWLP_USERDATA, WM_NCCREATE};
use pr_platform_win::win32::structs::{Hwnd, Lparam, Lresult, Wparam};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{CREATESTRUCTW, DefWindowProcW, RegisterClassExW, WNDCLASSEXW};
use windows::core::w;

// ---------------------------------------------------------------------------------------------------------------------------------------------
// wndproc.rs: make_cookie / creation_cookie, driven by calling `shell_window_proc` directly with a hand-built WM_NCCREATE
// ---------------------------------------------------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Recorder {
    log: RefCell<Vec<u32>>,
    destroyed: Cell<u32>,
}

impl WindowTarget for Recorder {
    fn process_message(&self, hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        self.log.borrow_mut().push(message);
        // SAFETY: forwards the parameters this call received.
        unsafe { user32::def_window_proc(hwnd, message, wparam, lparam) }
    }
    fn on_native_destroyed(&self) {
        self.destroyed.set(self.destroyed.get() + 1);
    }
}

unsafe extern "system" fn plain_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: forwards the parameters this procedure received.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// A message-only window of a private class whose procedure is `DefWindowProc` (so `GWLP_USERDATA` is free and nothing else touches it).
fn plain_window() -> Hwnd {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(plain_proc),
            hInstance: HINSTANCE(kernel32::get_module_handle().unwrap() as *mut core::ffi::c_void),
            lpszClassName: w!("PhotoReviewRs.Test.Host2Plain"),
            ..Default::default()
        };
        // SAFETY: `class` is a live, correctly sized WNDCLASSEXW with a static class name and a procedure valid for the process.
        assert_ne!(unsafe { RegisterClassExW(&class) }, 0);
    });
    let instance = kernel32::get_module_handle().unwrap();
    // SAFETY: a plain class, `HWND_MESSAGE` (-3) parent, no creation data.
    unsafe {
        user32::create_window_ex(
            0,
            "PhotoReviewRs.Test.Host2Plain",
            "",
            0,
            0,
            0,
            0,
            0,
            -3,
            0,
            instance,
            0,
        )
    }
    .expect("CreateWindowEx")
}

fn call_nc_create(hwnd: Hwnd, lparam: isize) -> isize {
    // SAFETY: `hwnd` is a live window of this thread; `lparam` is 0 or points to a CREATESTRUCTW-sized readable buffer for the call.
    unsafe {
        shell_window_proc(
            HWND(hwnd as *mut core::ffi::c_void),
            WM_NCCREATE,
            WPARAM(0),
            LPARAM(lparam),
        )
    }
    .0
}

/// wndproc.rs:117:5 `creation_cookie -> 0/1/-1`, 117:15 `lparam == 0` `== -> !=`, 117:20 `|| -> &&`, 45:5 `make_cookie -> 0/1/-1`:
/// an aligned CREATESTRUCTW whose `lpCreateParams` is a real cookie binds exactly that cookie and reaches the target.
#[test]
fn nc_create_with_an_aligned_createstruct_binds_the_cookie_and_reaches_the_target() {
    let target = Rc::new(Recorder::default());
    let cookie = make_cookie(Rc::clone(&target) as Rc<dyn WindowTarget>);
    assert!(cookie > 1, "a heap pointer, not 0/1/-1: {cookie}");
    let hwnd = plain_window();
    let create = CREATESTRUCTW {
        lpCreateParams: cookie as *mut core::ffi::c_void,
        ..Default::default()
    };
    assert_eq!(
        (&raw const create) as usize % align_of::<CREATESTRUCTW>(),
        0,
        "the fixture is aligned"
    );
    let result = call_nc_create(hwnd, (&raw const create) as isize);
    assert_ne!(result, 0, "the target's DefWindowProc accepted WM_NCCREATE");
    assert_eq!(user32::get_window_long_ptr(hwnd, GWLP_USERDATA), cookie);
    assert_eq!(*target.log.borrow(), vec![WM_NCCREATE]);
    // SAFETY: clearing the slot first, then freeing the cookie once.
    unsafe {
        user32::set_window_long_ptr(hwnd, GWLP_USERDATA, 0);
        free_cookie(cookie);
    }
    user32::destroy_window(hwnd).unwrap();
}

/// wndproc.rs:117:20 `|| -> &&` and 117:15 `== -> !=`: a null `lparam` cancels creation and is never dereferenced (a mutant would read address 0).
#[test]
fn nc_create_with_a_null_lparam_cancels_creation_without_reading_it() {
    let hwnd = plain_window();
    assert_eq!(call_nc_create(hwnd, 0), 0);
    assert_eq!(user32::get_window_long_ptr(hwnd, GWLP_USERDATA), 0);
    user32::destroy_window(hwnd).unwrap();
}

/// wndproc.rs:117:69 `!= -> ==`, 117:39 `% -> /` and `% -> +`: a misaligned (but readable) pointer is rejected with 0 before the cookie inside it is
/// read, so the slot stays empty and the target is never called.
#[test]
fn nc_create_with_a_misaligned_lparam_is_rejected_and_binds_nothing() {
    let target = Rc::new(Recorder::default());
    let cookie = make_cookie(Rc::clone(&target) as Rc<dyn WindowTarget>);
    let hwnd = plain_window();
    let mut buffer = vec![0u8; size_of::<CREATESTRUCTW>() + 2 * align_of::<CREATESTRUCTW>()];
    let base = buffer.as_mut_ptr() as usize;
    let aligned = base.next_multiple_of(align_of::<CREATESTRUCTW>());
    let misaligned = aligned + 1;
    let create = CREATESTRUCTW {
        lpCreateParams: cookie as *mut core::ffi::c_void,
        ..Default::default()
    };
    // SAFETY: `misaligned + size_of` stays inside `buffer` (it has two alignments of slack); an unaligned write is the point.
    unsafe { std::ptr::write_unaligned(misaligned as *mut CREATESTRUCTW, create) };
    assert_ne!(misaligned % align_of::<CREATESTRUCTW>(), 0);

    assert_eq!(call_nc_create(hwnd, misaligned as isize), 0);
    assert_eq!(
        user32::get_window_long_ptr(hwnd, GWLP_USERDATA),
        0,
        "the cookie behind a misaligned pointer was not bound"
    );
    assert!(target.log.borrow().is_empty());
    // SAFETY: the cookie came from `make_cookie` and was never bound.
    unsafe { free_cookie(cookie) };
    user32::destroy_window(hwnd).unwrap();
}
