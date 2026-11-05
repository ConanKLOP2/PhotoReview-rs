//! MUT3-HOST (2026-11-06): direct, bounded tests of the real OS seam `Win32LoopOs` (`host/message_loop.rs`), called on the test thread without
//! running a `MessageLoop`. The loop-level tests of `host_message_loop_native` spin or block forever when one of these seam methods is mutated
//! (a cargo-mutants TIMEOUT, which is never "caught"); here every call is a single non-blocking OS call (timeout 0) so a wrong answer is an assertion
//! failure. Only thread messages and a message-only window are used; no window is ever shown.
#![cfg(windows)]

use std::cell::Cell;

use pr_platform_win::host::message_loop::{LoopOs, WaitOutcome, Win32LoopOs};
use pr_platform_win::host::native::HWND_MESSAGE;
use pr_platform_win::win32::structs::{Msg, Win32Error};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateEventW, GetCurrentThreadId, SetEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, PostMessageW, PostThreadMessageW, RegisterClassExW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW,
};
use windows::core::w;

const WM_APP: u32 = 0x8000;
const WM_QUIT: u32 = 0x0012;

/// Removes everything queued for this thread (also creates the thread's queue).
fn drain(os: &Win32LoopOs) {
    let mut msg = Msg::default();
    let mut guard = 0;
    while os.peek_message(&mut msg, true) {
        guard += 1;
        assert!(guard < 10_000, "the queue never drained");
    }
}

fn post_thread(message: u32, wparam: usize, lparam: isize) {
    // SAFETY: posts to the calling thread, whose queue exists (`drain` ran first).
    unsafe { PostThreadMessageW(GetCurrentThreadId(), message, WPARAM(wparam), LPARAM(lparam)) }
        .expect("PostThreadMessageW");
}

#[test]
fn peek_and_pending_follow_the_real_queue_without_blocking() {
    let os = Win32LoopOs;
    drain(&os);
    let mut msg = Msg::default();
    // Empty queue: neither peek nor pending may report a message.
    assert!(!os.is_message_pending(), "empty queue: pending");
    assert!(!os.peek_message(&mut msg, false), "empty queue: peek without remove");
    assert!(!os.peek_message(&mut msg, true), "empty queue: peek with remove");

    post_thread(WM_APP + 1, 7, 9);
    assert!(os.is_message_pending(), "one queued message: pending");
    assert!(os.is_message_pending(), "pending must not consume the message");
    let mut seen = Msg::default();
    assert!(os.peek_message(&mut seen, false), "peek without remove sees it");
    assert_eq!((seen.message, seen.wParam.0, seen.lParam.0), (WM_APP + 1, 7, 9));
    assert!(
        os.peek_message(&mut seen, false),
        "peek without remove leaves it queued"
    );
    assert!(os.is_message_pending(), "still pending after a non-removing peek");
    let mut taken = Msg::default();
    assert!(os.peek_message(&mut taken, true), "peek with remove returns it");
    assert_eq!((taken.message, taken.wParam.0, taken.lParam.0), (WM_APP + 1, 7, 9));
    assert!(!os.is_message_pending(), "removed: not pending any more");
    assert!(!os.peek_message(&mut msg, true), "removed: nothing left");

    // Two messages come out in order, one per removing peek.
    post_thread(WM_APP + 2, 1, 0);
    post_thread(WM_APP + 3, 2, 0);
    assert!(os.peek_message(&mut msg, true));
    assert_eq!(msg.message, WM_APP + 2);
    assert!(os.is_message_pending());
    assert!(os.peek_message(&mut msg, true));
    assert_eq!(msg.message, WM_APP + 3);
    assert!(!os.is_message_pending());
}

thread_local! {
    static HITS: Cell<u32> = const { Cell::new(0) };
    static LAST: Cell<(u32, usize, isize)> = const { Cell::new((0, 0, 0)) };
}

extern "system" fn probe_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_APP + 5 {
        HITS.with(|hits| hits.set(hits.get() + 1));
        LAST.with(|last| last.set((message, wparam.0, lparam.0)));
        return LRESULT(0);
    }
    // SAFETY: forwards the parameters this procedure received.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[test]
fn translate_and_dispatch_reaches_the_window_procedure_exactly_once() {
    let os = Win32LoopOs;
    drain(&os);
    // SAFETY: a null module name selects the running executable.
    let instance = unsafe { GetModuleHandleW(None) }.unwrap();
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(probe_proc),
        hInstance: instance.into(),
        lpszClassName: w!("PhotoReviewRsMut3LoopOs"),
        ..Default::default()
    };
    // Registration may already exist when the binary runs the test twice (never in one process); ignore the atom.
    // SAFETY: `class` is fully initialised and outlives the call.
    let _ = unsafe { RegisterClassExW(&class) };
    // SAFETY: message-only window of this thread; destroyed below.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("PhotoReviewRsMut3LoopOs"),
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND(HWND_MESSAGE as *mut _)),
            None,
            Some(instance.into()),
            None,
        )
    }
    .unwrap();
    HITS.with(|hits| hits.set(0));
    // SAFETY: posting to a window of this thread.
    unsafe { PostMessageW(Some(hwnd), WM_APP + 5, WPARAM(11), LPARAM(22)) }.unwrap();
    let mut msg = Msg::default();
    assert!(os.peek_message(&mut msg, true));
    assert_eq!(msg.message, WM_APP + 5);
    assert_eq!(HITS.with(Cell::get), 0, "peeking must not dispatch");
    os.translate_and_dispatch(&msg);
    assert_eq!(HITS.with(Cell::get), 1, "dispatched exactly once");
    assert_eq!(LAST.with(Cell::get), (WM_APP + 5, 11, 22));
    // SAFETY: destroys the window created above on its own thread.
    unsafe { DestroyWindow(hwnd) }.unwrap();
}

fn event(manual_reset: bool, signaled: bool) -> HANDLE {
    // SAFETY: an unnamed event with default security.
    unsafe { CreateEventW(None, manual_reset, signaled, None) }.expect("CreateEventW")
}

#[test]
fn poll_handle_reports_signaled_state_and_consumes_an_auto_reset_event() {
    let os = Win32LoopOs;
    let unsignaled = event(true, false);
    let signaled = event(true, true);
    assert!(!os.poll_handle(unsignaled.0 as isize), "unsignaled event");
    assert!(os.poll_handle(signaled.0 as isize), "signaled manual-reset event");
    assert!(
        os.poll_handle(signaled.0 as isize),
        "a manual-reset event stays signaled"
    );
    let auto = event(false, true);
    assert!(os.poll_handle(auto.0 as isize), "signaled auto-reset event");
    assert!(
        !os.poll_handle(auto.0 as isize),
        "the first poll consumed the auto-reset event"
    );
    // SAFETY: sets an event this test owns.
    unsafe { SetEvent(auto) }.unwrap();
    assert!(os.poll_handle(auto.0 as isize), "signaled again");
    // SAFETY: closes handles this test created.
    unsafe {
        let _ = CloseHandle(unsignaled);
        let _ = CloseHandle(signaled);
        let _ = CloseHandle(auto);
    }
    // A closed (not reused) handle is a failed wait, never "signaled".
    assert!(!os.poll_handle(0), "null handle");
}

#[test]
fn wait_decodes_handle_messages_timeout_and_failure() {
    let os = Win32LoopOs;
    drain(&os);
    let idle = event(true, false);
    let hot = event(true, true);
    let idle_raw = idle.0 as isize;
    let hot_raw = hot.0 as isize;

    // Nothing signaled and no input: a zero-timeout wait times out for any handle count.
    assert_eq!(os.wait(&[], 0), WaitOutcome::Timeout, "no handles, no input");
    assert_eq!(os.wait(&[idle_raw], 0), WaitOutcome::Timeout, "one idle handle");
    assert_eq!(
        os.wait(&[idle_raw, idle_raw], 0),
        WaitOutcome::Timeout,
        "two idle handles"
    );
    // A signaled handle reports its index (first signaled handle wins).
    assert_eq!(os.wait(&[hot_raw], 0), WaitOutcome::Handle(0));
    assert_eq!(os.wait(&[idle_raw, hot_raw], 0), WaitOutcome::Handle(1));
    assert_eq!(os.wait(&[hot_raw, idle_raw], 0), WaitOutcome::Handle(0));

    // Input available: the queue slot is index == handles.len(), for 0, 1 and 2 handles.
    post_thread(WM_APP + 9, 0, 0);
    assert_eq!(os.wait(&[], 0), WaitOutcome::Messages, "0 handles, input");
    assert_eq!(os.wait(&[idle_raw], 0), WaitOutcome::Messages, "1 handle, input");
    assert_eq!(
        os.wait(&[idle_raw, idle_raw], 0),
        WaitOutcome::Messages,
        "2 handles, input"
    );
    // MWMO_INPUTAVAILABLE: the message was not removed by the wait, and it stays reported until it is taken.
    assert_eq!(os.wait(&[idle_raw], 0), WaitOutcome::Messages, "input stays available");
    drain(&os);
    assert_eq!(os.wait(&[idle_raw], 0), WaitOutcome::Timeout, "queue drained");

    // WAIT_FAILED carries the error read straight after the call.
    match os.wait(&[0x7777_0000], 0) {
        WaitOutcome::Failed(Win32Error(code)) => assert_eq!(code, 6, "ERROR_INVALID_HANDLE"),
        other => panic!("an invalid handle must fail the wait, got {other:?}"),
    }
    // SAFETY: closes handles this test created.
    unsafe {
        let _ = CloseHandle(idle);
        let _ = CloseHandle(hot);
    }
}

#[test]
fn post_quit_queues_wm_quit_with_the_exit_code() {
    let os = Win32LoopOs;
    drain(&os);
    os.post_quit(0);
    let mut msg = Msg::default();
    assert!(os.peek_message(&mut msg, true), "WM_QUIT queued (code 0)");
    assert_eq!((msg.message, msg.wParam.0), (WM_QUIT, 0));
    // WM_QUIT is only returned by PeekMessage after the queue ran dry of other messages; with nothing else it is the next message.
    os.post_quit(42);
    assert!(os.is_message_pending(), "WM_QUIT counts as pending");
    assert!(os.peek_message(&mut msg, true));
    assert_eq!((msg.message, msg.wParam.0), (WM_QUIT, 42));
    assert!(!os.peek_message(&mut msg, true), "queue empty again");
    os.post_quit(-3);
    assert!(os.peek_message(&mut msg, true));
    assert_eq!((msg.message, msg.wParam.0 as i32), (WM_QUIT, -3));
}
