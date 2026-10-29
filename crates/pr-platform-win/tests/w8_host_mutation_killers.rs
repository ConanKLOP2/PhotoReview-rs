//! Host lane regressions: real dispatcher/loop integration and deterministic mutation killers.
#![cfg(windows)]

#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use pr_core::abstractions::{NullLog, UiDispatcher, UiPriority};
use pr_platform_win::host::contracts::WindowMessage;
use pr_platform_win::host::message_loop::{MessageFilter, MessageLoop};
use pr_platform_win::host::pending_panic;
use pr_platform_win::host::ui_dispatcher::{WAKE_MESSAGE, Win32UiDispatcher};

fn on_ui_thread(body: impl FnOnce() + Send + 'static) {
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = catch_unwind(AssertUnwindSafe(body));
        tx.send(result).unwrap();
    });
    let result = rx.recv_timeout(Duration::from_secs(10)).expect("UI operation hung");
    worker.join().unwrap();
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

// C#: MessageLoop / Win32UiDispatcher priority contract; closes the test-local pump gap.
#[test]
#[ignore = "native: real Win32 dispatcher and message loop"]
fn real_message_loop_drains_real_dispatcher_in_priority_order_before_background_quit() {
    on_ui_thread(|| {
        let dispatcher = Win32UiDispatcher::new(Arc::new(NullLog)).unwrap();
        let message_loop = MessageLoop::new(Rc::new(dispatcher.clone())).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        for (priority, label) in [
            (UiPriority::Background, "background"),
            (UiPriority::Render, "render"),
            (UiPriority::Normal, "normal"),
            (UiPriority::Send, "send"),
        ] {
            let calls = Arc::clone(&calls);
            dispatcher.post_with_priority(
                Box::new(move || {
                    calls.lock().unwrap().push(label);
                    if priority == UiPriority::Background {
                        MessageLoop::quit(37);
                    }
                }),
                priority,
            );
        }
        let result = message_loop.run();
        dispatcher.shutdown();
        assert_eq!(result.unwrap(), 37);
        assert_eq!(*calls.lock().unwrap(), ["send", "normal", "render", "background"]);
    });
}

struct CaptureOnWake;
impl MessageFilter for CaptureOnWake {
    fn pre_filter_message(&self, message: &WindowMessage) -> bool {
        if message.msg == WAKE_MESSAGE {
            pending_panic::capture(Box::new("real-loop-pending"));
        }
        false
    }
}

// C#: WndProcThunk.RethrowPending; the real dispatcher wake passes through the real loop.
#[test]
#[ignore = "native: real Win32 dispatcher wake and pending panic"]
fn real_message_loop_resumes_pending_panic_after_real_dispatcher_wake() {
    on_ui_thread(|| {
        let dispatcher = Win32UiDispatcher::new(Arc::new(NullLog)).unwrap();
        let message_loop = MessageLoop::new(Rc::new(dispatcher.clone())).unwrap();
        message_loop.add_message_filter(Rc::new(CaptureOnWake));
        dispatcher.post_with_priority(Box::new(|| {}), UiPriority::Normal);
        let outcome = catch_unwind(AssertUnwindSafe(|| message_loop.run()));
        dispatcher.shutdown();
        let payload = outcome.expect_err("the loop must resume the captured panic");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"real-loop-pending"));
        assert!(!pending_panic::has_pending());
    });
}

// C#: CrossfadeLayerTests.Start_WithNothingToFade_ReturnsFalseAndSubscribesNothing.
// Mutant shell_viewing/crossfade.rs:191:52, replace > with >= (zero-height outgoing frame).
#[test]
fn crossfade_zero_height_is_rejected_without_retaining_pixels_or_requesting_frames() {
    use pr_app::input::primitives::RectD;
    use pr_app::viewport::render_geometry::ImageInterpolation;
    use pr_imaging::pixels::{PixelBuffer, PixelLayout};
    use pr_platform_win::shell_viewing::crossfade::CrossfadeLayer;
    use std::cell::Cell;

    let clock = Rc::new(viewing_fakes::FakeClock::default());
    let invalidations = Rc::new(Cell::new(0));
    let calls = Rc::clone(&invalidations);
    let layer = CrossfadeLayer::new(clock.clone(), Rc::new(move || calls.set(calls.get() + 1)), 1000);
    let pixels = Arc::new(PixelBuffer::allocate(2, 3, PixelLayout::Pbgra32).unwrap());
    let weak = Arc::downgrade(&pixels);
    assert_eq!(
        layer.start(
            pixels,
            RectD {
                x: 2.0,
                y: 3.0,
                width: 10.0,
                height: 0.0
            },
            ImageInterpolation::Linear,
            Duration::from_millis(100)
        ),
        Ok(false)
    );
    assert!(!layer.is_active());
    assert!(layer.outgoing().is_none());
    assert!(weak.upgrade().is_none(), "the rejected buffer must not be retained");
    assert_eq!(layer.start_count(), 0);
    assert_eq!(layer.opacity(), 0.0);
    assert_eq!(clock.subscribers(), 0);
    assert_eq!(clock.requests.get(), 0);
    assert_eq!(invalidations.get(), 0);
}

// C#: WndProcThunk Capture/RethrowPending, first exception wins.
// Mutants host/pending_panic.rs:23 capture -> (), :40 has_pending -> true/false,
// :48 rethrow_pending -> (), :57 guarded -> Default::default().
#[test]
fn pending_panic_preserves_first_payload_clears_slot_and_guards_return_values() {
    assert!(!pending_panic::has_pending());
    pending_panic::capture(Box::new("first"));
    pending_panic::capture(Box::new("second"));
    assert!(pending_panic::has_pending());
    let first = catch_unwind(pending_panic::rethrow_pending).expect_err("capture must preserve a payload");
    assert_eq!(first.downcast_ref::<&str>(), Some(&"first"));
    assert!(!pending_panic::has_pending());
    pending_panic::rethrow_pending();
    assert_eq!(pending_panic::guarded(41, || 97), 97);
    assert_eq!(pending_panic::guarded(41, || panic!("guarded payload")), 41);
    assert!(pending_panic::has_pending());
    let guarded = catch_unwind(pending_panic::rethrow_pending).expect_err("guarded panic must be captured");
    assert_eq!(guarded.downcast_ref::<&str>(), Some(&"guarded payload"));
    assert!(!pending_panic::has_pending());
}

// C#: ShellWindow.Show(bool)/Show(int); observable Win32 visibility and show state.
// Native glue is intentionally checked against Windows rather than cargo-mutants.
#[test]
#[ignore = "native: briefly shows only the test window, then destroys it"]
fn shell_window_show_and_show_command_change_real_visibility_and_minimized_state() {
    use pr_platform_win::host::contracts::ShellWindowApi;
    use pr_platform_win::host::shell_window::{ShellWindow, ShellWindowOptions};
    use pr_platform_win::win32::{api::user32, messages, startup};

    on_ui_thread(|| {
        let window = ShellWindow::create(
            ShellWindowOptions {
                title: "PhotoReviewRs cx-host visibility test".into(),
                width_dip: 400.0,
                height_dip: 300.0,
                ..Default::default()
            },
            Rc::new(viewing_fakes::FakeClock::default()),
        )
        .unwrap();
        let hwnd = window.hwnd();
        assert!(!user32::is_window_visible(hwnd));
        window.show(false);
        assert!(user32::is_window_visible(hwnd));
        window.show_command(messages::SW_HIDE);
        assert!(!user32::is_window_visible(hwnd));
        window.show(true);
        assert!(user32::is_window_visible(hwnd));
        window.show_command(messages::SW_MINIMIZE);
        assert!(startup::is_iconic(hwnd));
        window.show_command(messages::SW_RESTORE);
        assert!(!startup::is_iconic(hwnd));
        assert!(user32::is_window_visible(hwnd));
        window.dispose();
        assert!(window.is_destroyed());
    });
}

// Mutant crossfade.rs:150 `CrossfadeLayer::start_count -> i32 with 0`: a started fade must be counted.
#[test]
fn crossfade_start_count_counts_accepted_starts() {
    use pr_app::input::primitives::RectD;
    use pr_app::viewport::render_geometry::ImageInterpolation;
    use pr_imaging::pixels::{PixelBuffer, PixelLayout};
    use pr_platform_win::shell_viewing::crossfade::CrossfadeLayer;

    let clock = Rc::new(viewing_fakes::FakeClock::default());
    let layer = CrossfadeLayer::new(clock, Rc::new(|| {}), 1000);
    let frame = RectD {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
    };
    for expected in 1..=2 {
        let pixels = Arc::new(PixelBuffer::allocate(2, 2, PixelLayout::Pbgra32).unwrap());
        assert_eq!(
            layer.start(pixels, frame, ImageInterpolation::Linear, Duration::from_millis(100)),
            Ok(true)
        );
        assert_eq!(layer.start_count(), expected);
    }
}
