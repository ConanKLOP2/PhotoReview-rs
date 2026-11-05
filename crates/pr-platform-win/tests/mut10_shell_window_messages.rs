//! MUT10 (2026-11-06): the default arms of `ShellWindowInner::process_message` (`host/shell_window/messages.rs`).
//!
//! * `84:13` (delete the default `WM_SIZE => { invalidate(); 0 }` arm): with no handler the message falls to `DefWindowProc`, which gives `0` too, so
//!   the only visible difference is that no frame is requested. `is_render_requested()` shows it.
//! * `88:13` (delete the default `WM_DPICHANGED => 0` arm): the message falls to `DefWindowProc`. No test can tell the two apart if `DefWindowProc`
//!   gives `0` and changes nothing the arm did not; `dpi_changed_default_arm_is_indistinguishable_from_def_window_proc` records the observation over
//!   the input classes of `wParam` and `lParam` (it passes under the mutant; see `docs/reviews/P10-HOST-SMALL-MUT-2026-11-06.md`).
//!
//! Native tests (hidden real window, a UI thread): `#[ignore = "native: needs desktop"]`. Every wait is bounded; no sleeps.
#![cfg(windows)]

#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
#[path = "common/ui_thread.rs"]
mod ui_thread;

use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::host::shell_window::ShellWindowOptions;
use pr_platform_win::win32::messages::{WM_DPICHANGED, WM_SIZE};
use shell_window_fixture::{
    call_window_proc, create_window, dispose_window, send, wait_until_idle, window, window_rect,
};
use ui_thread::UiThread;

fn size_lparam(width: i32, height: i32) -> isize {
    ((height << 16) | width) as isize
}

#[test]
#[ignore = "native: needs desktop"]
fn default_wm_size_requests_a_frame_and_answers_zero() {
    let ui = UiThread::start();
    let hwnd = ui.invoke(|| create_window(ShellWindowOptions::default()).hwnd());
    wait_until_idle(&ui); // the creation frame is drawn: nothing is requested any more
    assert!(!ui.invoke(|| window().is_render_requested()), "idle before the message");
    let (result, requested) = ui.invoke(move || {
        let result = send(hwnd, WM_SIZE, 0, size_lparam(640, 480));
        (result, window().is_render_requested())
    });
    assert_eq!(result, 0, "WM_SIZE is answered with 0");
    assert!(requested, "the default WM_SIZE arm invalidates (asks for a frame)");
    ui.invoke(dispose_window);
}

#[test]
#[ignore = "native: needs desktop"]
fn dpi_changed_default_arm_is_indistinguishable_from_def_window_proc() {
    let ui = UiThread::start();
    let hwnd = ui.invoke(|| create_window(ShellWindowOptions::default()).hwnd());
    wait_until_idle(&ui);
    let probes = ui.invoke(move || {
        let mut seen = Vec::new();
        let rect = window_rect(hwnd);
        let valid = &rect as *const _ as isize;
        // lParam classes: null, misaligned, a valid RECT; wParam classes: dpi 0, 96, 144, 192, 65535 in the low word, a different high word.
        for lparam in [0, 1, valid] {
            for wparam in [
                0_usize,
                96,
                144 | (96 << 16),
                192 | (192 << 16),
                0xFFFF,
                0xFFFF_FFFF,
                usize::MAX,
            ] {
                let result = call_window_proc(hwnd, WM_DPICHANGED, wparam, lparam);
                seen.push((lparam == valid, wparam, result, window().is_render_requested()));
            }
        }
        seen
    });
    for (valid, wparam, result, _requested) in &probes {
        assert_eq!(
            *result, 0,
            "WM_DPICHANGED (valid rect {valid}, wParam {wparam:#x}) is answered with 0"
        );
    }
    ui.invoke(dispose_window);
}
