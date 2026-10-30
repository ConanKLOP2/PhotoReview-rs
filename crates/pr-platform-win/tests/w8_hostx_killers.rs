//! cl-hostx lane killers: the real `Win32LoopOs` seam (no window is created, only the calling thread's message queue and semaphores).
#![cfg(windows)]

use pr_platform_win::host::message_loop::{LoopOs, WaitOutcome, Win32LoopOs};
use pr_platform_win::win32::structs::Msg;
use windows::Win32::Foundation::{CloseHandle, HANDLE, LPARAM, WPARAM};
use windows::Win32::System::Threading::{CreateSemaphoreW, GetCurrentThreadId, ReleaseSemaphore};
use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;

const PROBE: u32 = 0x8000 + 0x4F1; // WM_APP + 0x4F1: a thread message nobody dispatches.

/// Runs `body` on a fresh thread so the queue and the posted messages never leak between tests.
fn on_fresh_thread(body: impl FnOnce() + Send + 'static) {
    std::thread::spawn(body).join().unwrap();
}

/// Forces the thread's message queue into existence (PeekMessage creates it), then drains it.
fn init_queue() {
    let mut scratch = Msg::default();
    assert!(!Win32LoopOs.peek_message(&mut scratch, true));
}

fn post_probe(wparam: usize, lparam: isize) {
    // SAFETY: plain thread-message post to the calling thread, whose queue exists (`init_queue`).
    unsafe { PostThreadMessageW(GetCurrentThreadId(), PROBE, WPARAM(wparam), LPARAM(lparam)) }.unwrap();
}

fn semaphore(initial: i32) -> HANDLE {
    // SAFETY: anonymous semaphore, closed by the caller.
    unsafe { CreateSemaphoreW(None, initial, 4, None) }.unwrap()
}

fn raw(handle: HANDLE) -> isize {
    handle.0 as isize
}

// message_loop.rs:115 `peek_message -> bool with true/false`; remove flag `if remove {PM_REMOVE} else {PM_NOREMOVE}` (:117).
#[test]
fn win32_loop_os_peek_message_reports_and_removes_only_when_asked() {
    on_fresh_thread(|| {
        init_queue();
        let mut msg = Msg::default();
        assert!(!Win32LoopOs.peek_message(&mut msg, false), "empty queue");
        post_probe(7, 9);
        assert!(Win32LoopOs.peek_message(&mut msg, false), "peek without remove sees it");
        assert_eq!((msg.message, msg.wParam.0, msg.lParam.0), (PROBE, 7, 9));
        let mut again = Msg::default();
        assert!(
            Win32LoopOs.peek_message(&mut again, false),
            "still queued after a no-remove peek"
        );
        assert!(Win32LoopOs.peek_message(&mut again, true), "remove peek returns it");
        assert_eq!(again.message, PROBE);
        assert!(
            !Win32LoopOs.peek_message(&mut again, true),
            "gone after the removing peek"
        );
    });
}

// message_loop.rs:120 `is_message_pending -> bool with true/false`; must not consume the message.
#[test]
fn win32_loop_os_is_message_pending_does_not_consume() {
    on_fresh_thread(|| {
        init_queue();
        assert!(!Win32LoopOs.is_message_pending());
        post_probe(1, 2);
        assert!(Win32LoopOs.is_message_pending());
        assert!(Win32LoopOs.is_message_pending(), "second ask: still there");
        let mut msg = Msg::default();
        assert!(Win32LoopOs.peek_message(&mut msg, true));
        assert!(!Win32LoopOs.is_message_pending());
    });
}

// message_loop.rs:132 `poll_handle -> bool with true/false`: a signaled semaphore is consumed, an empty one is not signaled.
#[test]
fn win32_loop_os_poll_handle_is_true_only_for_a_signaled_handle_and_consumes_it() {
    let empty = semaphore(0);
    let full = semaphore(1);
    assert!(!Win32LoopOs.poll_handle(raw(empty)));
    assert!(Win32LoopOs.poll_handle(raw(full)));
    assert!(!Win32LoopOs.poll_handle(raw(full)), "the poll took the only count");
    // SAFETY: handles created above.
    unsafe {
        CloseHandle(empty).unwrap();
        CloseHandle(full).unwrap();
    }
}

// message_loop.rs:136-146 `wait`: result decoding (`result < count` -> Handle, `== count` -> Messages, else Timeout, WAIT_FAILED -> Failed).
#[test]
fn win32_loop_os_wait_decodes_handle_messages_timeout_and_failure() {
    on_fresh_thread(|| {
        init_queue();
        let first = semaphore(0);
        let second = semaphore(0);
        let handles = [raw(first), raw(second)];
        assert_eq!(Win32LoopOs.wait(&handles, 0), WaitOutcome::Timeout);
        assert_eq!(Win32LoopOs.wait(&[], 0), WaitOutcome::Timeout, "no handle, no message");

        // SAFETY: handle created above.
        unsafe { ReleaseSemaphore(second, 1, None) }.unwrap();
        assert_eq!(Win32LoopOs.wait(&handles, 0), WaitOutcome::Handle(1));
        // SAFETY: as above.
        unsafe { ReleaseSemaphore(first, 1, None) }.unwrap();
        assert_eq!(Win32LoopOs.wait(&handles, 0), WaitOutcome::Handle(0));

        post_probe(0, 0);
        assert_eq!(Win32LoopOs.wait(&handles, 0), WaitOutcome::Messages, "index == count");
        assert_eq!(
            Win32LoopOs.wait(&[], 0),
            WaitOutcome::Messages,
            "count 0, message queued"
        );
        let mut drained = Msg::default();
        assert!(Win32LoopOs.peek_message(&mut drained, true));
        // A stale handle value fails the wait and carries the OS error read straight after the call.
        let stale = semaphore(0);
        let stale_raw = raw(stale);
        // SAFETY: handle created above; closed so the value is stale.
        unsafe { CloseHandle(stale).unwrap() };
        match Win32LoopOs.wait(&[stale_raw], 0) {
            WaitOutcome::Failed(error) => assert_ne!(error.0, 0),
            other => panic!("expected Failed, got {other:?}"),
        }
        // SAFETY: handles created above.
        unsafe {
            CloseHandle(first).unwrap();
            CloseHandle(second).unwrap();
        }
    });
}

// message_loop.rs:152 `post_quit -> ()`: PostQuitMessage makes the thread's next PeekMessage return WM_QUIT with the exit code in wParam.
#[test]
fn win32_loop_os_post_quit_queues_wm_quit_with_the_exit_code() {
    on_fresh_thread(|| {
        init_queue();
        Win32LoopOs.post_quit(41);
        let mut msg = Msg::default();
        assert!(Win32LoopOs.peek_message(&mut msg, true));
        assert_eq!(msg.message, 0x0012, "WM_QUIT");
        assert_eq!(msg.wParam.0, 41);
    });
}

// message_loop.rs:280 `MessageLoop::quit -> ()`.
#[test]
fn message_loop_quit_posts_wm_quit_for_the_calling_thread() {
    on_fresh_thread(|| {
        init_queue();
        pr_platform_win::host::message_loop::MessageLoop::quit(-3);
        let mut msg = Msg::default();
        assert!(Win32LoopOs.peek_message(&mut msg, true));
        assert_eq!(msg.message, 0x0012, "WM_QUIT");
        assert_eq!(msg.wParam.0 as i32, -3);
    });
}

// message_loop.rs:290 `MessageLoop::is_message_pending -> bool with true/false`.
#[test]
fn message_loop_is_message_pending_follows_the_thread_queue() {
    on_fresh_thread(|| {
        use pr_platform_win::host::message_loop::MessageLoop;
        init_queue();
        assert!(!MessageLoop::is_message_pending());
        post_probe(0, 0);
        assert!(MessageLoop::is_message_pending());
    });
}

// message_loop.rs:168:72 `- with /` in `wait_timeout_ms` (the `frequency - 1` of the ceiling division) and the neighbouring operators.
#[test]
fn wait_timeout_ms_rounds_up_exactly() {
    use pr_platform_win::host::message_loop::wait_timeout_ms;
    assert_eq!(wait_timeout_ms(1, 1000), 1);
    assert_eq!(wait_timeout_ms(1000, 1000), 1000, "exact multiple must not round up");
    assert_eq!(wait_timeout_ms(3, 7), 429, "ceil(3000 / 7)");
    assert_eq!(wait_timeout_ms(1500, 1000), 1500);
    assert_eq!(wait_timeout_ms(1, 10_000_000), 1);
    assert_eq!(wait_timeout_ms(10_000_001, 10_000_000), 1001);
    assert_eq!(wait_timeout_ms(5, 0), 5000, "frequency below 1 is clamped to 1");
    assert_eq!(wait_timeout_ms(0, 1000), 0);
    assert_eq!(wait_timeout_ms(-1, 1000), 0);
    assert_eq!(wait_timeout_ms(i64::MAX, 1000), 0xFFFF_FFFF);
    assert_eq!(wait_timeout_ms(i64::MAX - 1, 1), 0xFFFF_FFFE, "capped below INFINITE");
}

// message_loop.rs:195 `<impl Debug for MessageLoop>::fmt -> Ok(Default::default())` (empty output).
#[test]
fn message_loop_debug_names_the_type() {
    use pr_platform_win::host::error::HostError;
    use pr_platform_win::host::message_loop::{LoopDispatcher, MessageLoop};
    use std::rc::Rc;
    struct Idle;
    impl LoopDispatcher for Idle {
        fn verify_access(&self) -> Result<(), HostError> {
            Ok(())
        }
        fn has_high_priority_work(&self) -> bool {
            false
        }
        fn run_high_priority_batch(&self) {}
        fn has_background_work(&self) -> bool {
            false
        }
        fn try_run_background(&self) -> bool {
            false
        }
    }
    let message_loop = MessageLoop::new(Rc::new(Idle)).unwrap();
    assert!(format!("{message_loop:?}").starts_with("MessageLoop"));
}

// message_loop.rs:285 `MessageLoop::quit_with -> ()`: goes through the loop's own `LoopOs`.
#[test]
fn message_loop_quit_with_posts_wm_quit_through_the_os_seam() {
    on_fresh_thread(|| {
        use pr_platform_win::host::error::HostError;
        use pr_platform_win::host::message_loop::{LoopDispatcher, MessageLoop};
        use std::rc::Rc;
        struct Idle;
        impl LoopDispatcher for Idle {
            fn verify_access(&self) -> Result<(), HostError> {
                Ok(())
            }
            fn has_high_priority_work(&self) -> bool {
                false
            }
            fn run_high_priority_batch(&self) {}
            fn has_background_work(&self) -> bool {
                false
            }
            fn try_run_background(&self) -> bool {
                false
            }
        }
        init_queue();
        let message_loop = MessageLoop::with_os(Rc::new(Idle), Rc::new(Win32LoopOs)).unwrap();
        message_loop.quit_with(23);
        let mut msg = Msg::default();
        assert!(Win32LoopOs.peek_message(&mut msg, true));
        assert_eq!((msg.message, msg.wParam.0), (0x0012, 23));
    });
}
