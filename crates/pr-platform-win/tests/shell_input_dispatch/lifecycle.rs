//! The tests new to Rust (contract W8-14b, test plan): borrow discipline (T1, T2), click counting (T3), the lazy router rebuild (D-W8B4I-7),
//! continuations (T9), subscriptions (T8), the perf row and the error report (T10).
use std::sync::{Arc, Mutex};

use pr_app::input::key_id::KeyId;
use pr_app::input::review_command::{ReviewCommand, ReviewCommandType};
use pr_app::services::shortcut_key_name::try_parse;
use pr_core::abstractions::UiPriority;
use pr_core::diagnostics::perf::{PerfEvent, PerfListener, PerfValue, PhotoReviewPerf};
use pr_platform_win::host::contracts::ShellWindowEvent;
use pr_platform_win::shell_input::native::MK_LBUTTON;
use pr_platform_win::win32::messages::{WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL};

use crate::fixture::{LogLevel, Rig, RigOptions};

const MKL: usize = MK_LBUTTON as usize;

fn actual_size(rig: &Rig) {
    rig.input.execute(ReviewCommand::ZOOM_ACTUAL_SIZE);
    rig.drain();
}

fn render_posts(rig: &Rig) -> usize {
    rig.dispatcher
        .posted_priorities()
        .iter()
        .filter(|priority| **priority == UiPriority::Render)
        .count()
}

/// A Ctrl+wheel zoom that is NOT drained: its zoom pass waits at the render yield.
fn zoom_without_drain(rig: &Rig) {
    let _ = rig
        .window
        .dispatch(WM_MOUSEWHEEL, (120_usize << 16) | 0x0008, Rig::pack(500, 300));
}

#[test]
fn the_input_registers_three_handlers_and_one_deactivated_subscription() {
    let rig = Rig::new();
    assert_eq!(3, rig.window.handler_count());
    assert_eq!(1, rig.window.subscriber_count(ShellWindowEvent::Deactivated));
}

// new: T1. `ReleaseCapture` sends WM_CAPTURECHANGED to our own window synchronously, while the release still holds the controller state.
#[test]
fn capture_changed_during_release_does_not_panic_and_cancels_the_pan() {
    let mut offsets = Vec::new();
    let mut releases = 0;
    // One rig at a time: the fixture's render-post hook is per thread.
    for reentrant in [false, true] {
        let rig = Rig::new();
        rig.window.capture_changed_on_release.set(reentrant);
        actual_size(&rig);
        rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
        assert!(rig.window.captured.get());
        rig.mouse(WM_MOUSEMOVE, 450, 280, MKL);
        rig.mouse(WM_LBUTTONUP, 450, 280, 0);
        assert!(!rig.window.captured.get(), "the release gave the capture back");
        let after_release = rig.offsets();
        // The pan is over: a further move with the button reported pressed does not scroll.
        rig.mouse(WM_MOUSEMOVE, 300, 200, MKL);
        assert_eq!(after_release, rig.offsets());
        offsets.push(after_release);
        releases = rig.window.releases.get();
    }
    assert_eq!(
        offsets[0], offsets[1],
        "same observable state as the non-re-entrant run"
    );
    assert!(releases >= 1);
}

// new: T1. A capture loss that arrives while the press is cancelled by a Fit is replayed after the borrows are gone.
#[test]
fn capture_changed_during_fit_does_not_panic() {
    let rig = Rig::new();
    rig.window.capture_changed_on_release.set(true);
    actual_size(&rig);
    rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
    rig.fit();
    assert!(rig.viewer.borrow().is_fit());
    assert!(!rig.window.captured.get());
}

// new: T2. The surface reads the viewer in `capture()` (Fit) while the wheel zoom holds it; a double borrow would panic.
#[test]
fn fit_and_wheel_zoom_with_a_viewer_reading_surface_do_not_double_borrow() {
    let rig = Rig::new();
    rig.fit();
    rig.wheel(WM_MOUSEWHEEL, 120, 500, 300, true);
    assert!(!rig.viewer.borrow().is_fit());
    rig.fit();
    assert!(rig.viewer.borrow().is_fit());
    rig.input.execute(ReviewCommand::FIT_WIDTH);
    rig.drain();
    rig.input.execute(ReviewCommand::TOGGLE_FIT);
    rig.drain();
    assert!(rig.viewer.borrow().is_fit());
}

// new: T3. The click counter is fed by every press, also by one the routing rejects (compare view visible).
#[test]
fn rejected_presses_still_feed_the_click_counter() {
    let rig = Rig::new();
    rig.fit();
    rig.message_time.set(1000);
    rig.host.is_compare_visible.set(true);
    let (handled, _) = rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
    assert!(!handled, "the compare view takes the press away from the image");
    rig.host.is_compare_visible.set(false);
    rig.message_time.set(1100);
    // The second press is click number 2 = a double click = Fit: handled even though the image is already at Fit.
    let (handled, _) = rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
    assert!(handled);
}

// new: D-W8B4I-7. A different settings `Arc` rebuilds the table at the next key; the same `Arc` does not.
#[test]
fn settings_object_change_rebuilds_the_router_at_the_next_key() {
    let rig = Rig::new();
    let key = try_parse(Some("F9")).expect("F9");
    assert!(!rig.key_down(key, false, false).0, "unbound before");
    rig.update_settings(|settings| settings.shortcuts.refresh = "F9".to_owned());
    let (handled, _) = rig.key_down(key, false, false);
    assert!(handled);
    assert_eq!(vec![ReviewCommand::REFRESH], rig.host.commands_snapshot());
    // No change of the object: the table is kept (an unrelated key stays unbound).
    assert!(!rig.key_down(KeyId::F8, false, false).0);
}

// new: T9. A continuation posted before `dispose` and run after it is dropped, not resumed on a dead core.
#[test]
fn a_continuation_after_dispose_is_dropped() {
    let rig = Rig::new();
    rig.fit();
    zoom_without_drain(&rig);
    assert!(rig.dispatcher.pending() > 0);
    let zoom_pass_posts = render_posts(&rig);
    assert!(zoom_pass_posts >= 1);
    rig.input.dispose();
    rig.drain();
    assert_eq!(0, rig.dispatcher.pending());
}

// new: D-W8-2. A Fit that is not converged after a render yield posts again.
#[test]
fn resume_posts_again_when_the_controller_returns_another_await() {
    let rig = Rig::new();
    let before = render_posts(&rig);
    rig.input.apply_fit();
    rig.drain();
    assert!(render_posts(&rig) - before >= 2, "Fit needs at least two render yields");
}

// new: T6. `Deactivated` stops a glide but does not end a pan.
#[test]
fn deactivated_stops_the_glide_but_keeps_the_pan() {
    let rig = Rig::new();
    actual_size(&rig);
    rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
    assert!(rig.window.captured.get());
    rig.mouse(WM_MOUSEMOVE, 450, 300, MKL);
    let panned = rig.offsets();
    rig.window.raise_deactivated();
    assert!(rig.window.captured.get(), "the pan keeps the capture");
    rig.mouse(WM_MOUSEMOVE, 400, 300, MKL);
    assert!(rig.offsets() != panned, "the pan goes on");
}

// new: T8. `dispose` removes the `Deactivated` subscription, is idempotent, and leaves no frame hook.
#[test]
fn dispose_removes_every_window_subscription_and_the_registry_entry() {
    let rig = Rig::new();
    actual_size(&rig);
    rig.input.dispose();
    assert_eq!(0, rig.window.subscriber_count(ShellWindowEvent::Deactivated));
    assert_eq!(1, rig.window.unsubscribes.get());
    rig.input.dispose();
    assert_eq!(1, rig.window.unsubscribes.get(), "the second dispose does nothing");
    assert_eq!(0, rig.clock.subscriber_count());
    // The handlers stay on the window, but the registry entry is gone: a posted resume is dropped.
    zoom_without_drain(&rig);
    rig.drain();
    assert_eq!(0, rig.dispatcher.pending());
}

struct Collect(Mutex<Vec<(String, f64)>>);

impl PerfListener for Collect {
    fn on_event_written(&self, event: &PerfEvent) {
        if let (Some(PerfValue::Str(key)), Some(PerfValue::Double(delay))) =
            (event.payload.get(1), event.payload.get(2))
        {
            self.0.lock().unwrap().push((key.clone(), *delay));
        }
    }
}

// new: step 4 of HandleKeyDown. The delay is `tick_count - message_time` (wrapping). The listener is process-wide, so the delay is made unique.
#[test]
fn key_input_perf_row_is_written_when_a_listener_is_subscribed() {
    let rig = Rig::new();
    rig.message_time.set(123);
    rig.tick_count.set(5_000_123);
    let listener = Arc::new(Collect(Mutex::new(Vec::new())));
    let subscription = PhotoReviewPerf::log().subscribe(Arc::clone(&listener) as Arc<dyn PerfListener>);
    rig.key_down(KeyId::F8, false, false);
    drop(subscription);
    let rows = listener.0.lock().unwrap().clone();
    assert!(
        rows.iter()
            .any(|(key, delay)| key == KeyId::F8.name() && *delay == 5_000_000.0),
        "rows: {rows:?}"
    );
}

// new: T10. The host error is reported once, with the command name; the log has the error.
#[test]
fn host_error_is_reported_with_the_command_name_once() {
    let rig = Rig::new();
    *rig.host.execute_failure.borrow_mut() = Some("boom".to_owned());
    rig.input.execute(ReviewCommand::new(ReviewCommandType::Recycle));
    assert_eq!(vec!["failed:Recycle:boom".to_owned()], rig.host.log_snapshot());
    assert!(rig.log.contains(LogLevel::Error, "Command Recycle failed"));
    assert_eq!(1, rig.log.entries().len());
}

// new: C# `LastPointerPosition` in DIP (pixels over the DPI scale).
#[test]
fn last_pointer_position_is_the_last_move_in_dips() {
    let rig = Rig::with(RigOptions {
        dpi_scale: 2.0,
        ..RigOptions::default()
    });
    assert!(rig.input.last_pointer_position().is_none());
    rig.mouse(WM_MOUSEMOVE, 300, 100, 0);
    let position = rig.input.last_pointer_position().expect("position");
    assert_eq!((150.0, 50.0), (position.x, position.y));
}

// new: `ShellInput` is `Debug` (the stub contract); the handle prints its type name.
#[test]
fn the_input_handle_has_a_debug_form() {
    let rig = Rig::new();
    assert!(format!("{:?}", rig.input).contains("ShellInput"));
}
