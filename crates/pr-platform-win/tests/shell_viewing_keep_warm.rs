#[path = "common/viewing_fakes_16d.rs"]
mod viewing_fakes;

use pr_platform_win::host::contracts::{WindowMessage, WindowMessageHandler};
use pr_platform_win::render::keep_alive::{KeepAliveWork, OpenKeepAliveWork, ticks_from_ms};
use pr_platform_win::shell_viewing::keep_warm::*;
use pr_platform_win::win32::messages::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI64, Ordering},
};
use std::time::Duration;
use viewing_fakes::FakeKeepWarmWindow;

fn parse(name: &str, value: Option<&str>) -> KeepWarmOptions {
    KeepWarmOptions::from_environment(&|key| (key == name).then(|| value.map(str::to_owned)).flatten())
}
struct Harness {
    active: Rc<Cell<bool>>,
    now: Rc<Cell<i64>>,
    deadlines: Rc<RefCell<Vec<i64>>>,
    controller: KeepWarmController,
}
impl Harness {
    fn new(options: KeepWarmOptions) -> Self {
        let active = Rc::new(Cell::new(true));
        let now = Rc::new(Cell::new(ticks_from_ms(1000)));
        let deadlines = Rc::new(RefCell::new(Vec::new()));
        let a = active.clone();
        let n = now.clone();
        let d = deadlines.clone();
        let controller = KeepWarmController::new(
            options,
            Box::new(move || a.get()),
            Box::new(move |until| d.borrow_mut().push(until)),
            Some(Box::new(move || n.get())),
        );
        Self {
            active,
            now,
            deadlines,
            controller,
        }
    }
    fn advance(&self, ms: i64) {
        self.now.set(self.now.get() + ticks_from_ms(ms));
    }
}
#[test]
fn defaults_are_enabled_with_five_seconds_and_twenty_milliseconds() {
    // C#: KeepWarmTests.Defaults_AreEnabledWithFiveSecondsAndTwentyMilliseconds
    assert_eq!(
        KeepWarmOptions::default(),
        KeepWarmOptions {
            enabled: true,
            window_ms: 5000,
            interval_ms: 20
        }
    );
    assert_eq!(KeepWarmOptions::from_environment(&|_| None), KeepWarmOptions::default());
}
#[test]
fn disable_values_turn_it_off() {
    // C#: KeepWarmTests.DisableValues_TurnItOff
    for value in ["0", "off", "OFF", "false", "No", " 0 "] {
        assert!(!parse(ENABLED_VARIABLE, Some(value)).enabled);
    }
}
#[test]
fn any_other_value_keeps_it_on() {
    // C#: KeepWarmTests.AnyOtherValue_KeepsItOn
    for value in ["1", "on", "", "whatever"] {
        assert!(parse(ENABLED_VARIABLE, Some(value)).enabled);
    }
}
#[test]
fn window_ms_is_parsed_and_clamped() {
    // C#: KeepWarmTests.WindowMs_IsParsedAndClamped
    for (value, expected) in [
        (Some("3000"), 3000),
        (Some("10"), 250),
        (Some("999999"), 60000),
        (Some("abc"), 5000),
        (None, 5000),
    ] {
        assert_eq!(parse(WINDOW_VARIABLE, value).window_ms, expected);
    }
}
#[test]
fn interval_ms_is_parsed_and_clamped() {
    // C#: KeepWarmTests.IntervalMs_IsParsedAndClamped
    for (value, expected) in [("16", 16), ("1", 5), ("5000", 100), ("x", 20)] {
        assert_eq!(parse(INTERVAL_VARIABLE, Some(value)).interval_ms, expected);
    }
}
#[test]
fn from_environment_integer_forms() {
    for (value, expected) in [
        ("+5000", 5000),
        (" 5000 ", 5000),
        ("5 000", 5000),
        ("５０００", 5000),
        ("３０００", 5000),
        ("3 000", 5000),
        ("2147483648", 5000),
        ("-1", 250),
        ("\u{b}3000\u{c}", 3000),
    ] {
        assert_eq!(parse(WINDOW_VARIABLE, Some(value)).window_ms, expected);
    }
}
#[test]
fn is_interaction_classifies_window_messages() {
    // C#: KeepWarmTests.IsInteraction_ClassifiesWindowMessages
    for message in [
        WM_KEYDOWN,
        WM_SYSKEYDOWN,
        WM_MOUSEMOVE,
        WM_LBUTTONDOWN,
        WM_RBUTTONDOWN,
        WM_MBUTTONDOWN,
        WM_MOUSEWHEEL,
        WM_MOUSEHWHEEL,
        WM_ACTIVATE,
        WM_TOUCH,
        WM_POINTER_UPDATE,
        WM_POINTER_DOWN,
    ] {
        assert!(is_interaction(message));
    }
    for message in [WM_KEYUP, WM_PAINT, WM_TIMER, WM_SIZE, WM_LBUTTONUP] {
        assert!(!is_interaction(message));
    }
}
#[test]
fn is_interaction_includes_wm_xbuttondown() {
    assert!(is_interaction(WM_XBUTTONDOWN));
}
#[test]
fn interaction_extends_the_warm_window_by_window_ms() {
    // C#: KeepWarmTests.Interaction_ExtendsTheWarmWindowByWindowMs
    let h = Harness::new(KeepWarmOptions {
        window_ms: 3000,
        ..Default::default()
    });
    h.controller.note_interaction();
    assert_eq!(*h.deadlines.borrow(), [h.now.get() + ticks_from_ms(3000)]);
    assert_eq!(h.controller.activations(), 1);
}
#[test]
fn inactive_window_never_warms() {
    // C#: KeepWarmTests.InactiveWindow_NeverWarms
    let h = Harness::new(Default::default());
    h.active.set(false);
    h.controller.note_interaction();
    h.controller.note_interaction();
    assert!(h.deadlines.borrow().is_empty());
}
#[test]
fn disabled_options_never_warm() {
    // C#: KeepWarmTests.DisabledOptions_NeverWarm
    let h = Harness::new(KeepWarmOptions {
        enabled: false,
        ..Default::default()
    });
    h.controller.note_interaction();
    assert!(h.deadlines.borrow().is_empty());
}
#[test]
fn warm_at_launch_extends_the_warm_window_without_an_interaction_and_leaves_the_interaction_counters_alone() {
    // C#: KeepWarmTests.WarmAtLaunch_ExtendsTheWarmWindowWithoutAnInteraction_AndLeavesTheInteractionCountersAlone
    let h = Harness::new(Default::default());
    h.controller.warm_at_launch();
    assert_eq!(h.deadlines.borrow().len(), 1);
    assert_eq!(h.controller.launch_warmups(), 1);
    assert_eq!(h.controller.activations(), 0);
    h.controller.note_interaction();
    assert_eq!(h.deadlines.borrow().len(), 2);
}
#[test]
fn warm_at_launch_when_disabled_does_nothing() {
    // C#: KeepWarmTests.WarmAtLaunch_WhenDisabled_DoesNothing
    let h = Harness::new(KeepWarmOptions {
        enabled: false,
        ..Default::default()
    });
    h.controller.warm_at_launch();
    assert_eq!(h.controller.launch_warmups(), 0);
    assert!(h.deadlines.borrow().is_empty());
}
#[test]
fn dense_interactions_are_throttled_to_one_extension_per_throttle_window() {
    // C#: KeepWarmTests.DenseInteractions_AreThrottled_ToOneExtensionPerThrottleWindow
    let h = Harness::new(Default::default());
    for _ in 0..100 {
        h.controller.note_interaction();
        h.advance(1);
    }
    assert_eq!(h.deadlines.borrow().len(), 1);
    h.controller.note_interaction();
    assert_eq!(h.deadlines.borrow().len(), 2);
}
#[test]
fn interaction_while_inactive_does_not_consume_the_throttle() {
    // C#: KeepWarmTests.InteractionWhileInactive_DoesNotConsumeTheThrottle
    let h = Harness::new(Default::default());
    h.active.set(false);
    h.controller.note_interaction();
    h.active.set(true);
    h.controller.note_interaction();
    assert_eq!(h.deadlines.borrow().len(), 1);
}
fn message(msg: u32) -> WindowMessage {
    WindowMessage {
        hwnd: 1,
        msg,
        wparam: 0,
        lparam: 0,
    }
}
#[test]
fn try_handle_never_claims_the_message_and_only_interactions_activate() {
    // C#: KeepWarmTests.TryHandle_NeverClaimsTheMessage_AndOnlyInteractionsActivate
    let h = Harness::new(Default::default());
    assert_eq!(h.controller.try_handle(&message(WM_PAINT)), None);
    assert!(h.deadlines.borrow().is_empty());
    assert_eq!(h.controller.try_handle(&message(WM_KEYDOWN)), None);
    assert_eq!(h.deadlines.borrow().len(), 1);
}
#[test]
fn claims_nothing_for_any_message() {
    let h = Harness::new(Default::default());
    for msg in [
        WM_KEYDOWN,
        WM_SYSKEYDOWN,
        WM_MOUSEMOVE,
        WM_LBUTTONDOWN,
        WM_RBUTTONDOWN,
        WM_MBUTTONDOWN,
        WM_XBUTTONDOWN,
        WM_MOUSEWHEEL,
        WM_MOUSEHWHEEL,
        WM_ACTIVATE,
        WM_TOUCH,
        WM_POINTER_UPDATE,
        WM_POINTER_DOWN,
        WM_KEYUP,
        WM_PAINT,
        WM_TIMER,
        WM_SIZE,
        WM_LBUTTONUP,
    ] {
        assert_eq!(h.controller.try_handle(&message(msg)), None);
    }
}

struct Work(Option<Arc<AtomicBool>>);
impl KeepAliveWork for Work {
    fn touch(&mut self) -> Result<(), pr_platform_win::gfx::com::GfxError> {
        Ok(())
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        if let Some(flag) = &self.0 {
            flag.store(true, Ordering::SeqCst);
        }
    }
}
fn work() -> OpenKeepAliveWork {
    Box::new(|| Ok(Some(Box::new(Work(None)))))
}
fn registration(enabled: bool, prefer_warp: bool, with_work: bool) -> KeepWarmRegistration {
    KeepWarmRegistration {
        frame_log_path: None,
        keep_warm: Some(KeepWarmOptions {
            enabled,
            ..Default::default()
        }),
        prefer_warp,
        work: with_work.then(work),
        timestamp: Some(Arc::new(|| ticks_from_ms(1000))),
    }
}
fn register(window: &Rc<FakeKeepWarmWindow>, r: KeepWarmRegistration) -> KeepWarmSetup {
    let w: Rc<dyn KeepWarmWindow> = window.clone();
    register_keep_warm(&w, r)
}
#[test]
fn switch_off_creates_no_keep_alive_at_all() {
    // C#: ShellKeepWarmTests.SwitchOff_CreatesNoKeepAliveAtAll
    let w = Rc::new(FakeKeepWarmWindow::default());
    let s = register(&w, registration(false, false, false));
    assert!(s.keep_alive.is_none());
    assert!(s.controller.is_none());
    assert!(w.handlers.borrow().is_empty());
}
#[test]
fn warp_surface_with_the_default_work_creates_no_keep_alive() {
    // C#: ShellKeepWarmTests.WarpSurface_WithTheDefaultWork_CreatesNoKeepAlive
    let w = Rc::new(FakeKeepWarmWindow::default());
    let s = register(&w, registration(true, true, false));
    assert!(s.keep_alive.is_none());
    let mut s = register(&w, registration(true, true, true));
    assert!(s.keep_alive.is_some());
    s.dispose();
}
#[test]
fn interaction_while_the_window_is_not_active_does_not_warm() {
    // C#: ShellKeepWarmTests.InteractionWhileTheWindowIsNotActive_DoesNotWarm
    let w = Rc::new(FakeKeepWarmWindow::default());
    let mut s = register(&w, registration(true, false, true));
    s.controller.as_ref().unwrap().note_interaction();
    assert!(!s.keep_alive.as_ref().unwrap().has_thread());
    s.dispose();
}
#[test]
fn deactivation_stops_further_extensions() {
    // C#: ShellKeepWarmTests.Deactivation_StopsFurtherExtensions
    let w = Rc::new(FakeKeepWarmWindow::default());
    w.active.set(true);
    let now = Arc::new(AtomicI64::new(ticks_from_ms(1000)));
    let mut r = registration(true, false, true);
    let clock = now.clone();
    r.timestamp = Some(Arc::new(move || clock.load(Ordering::SeqCst)));
    let mut s = register(&w, r);
    let c = s.controller.as_ref().unwrap();
    c.note_interaction();
    assert_eq!(c.activations(), 1);
    now.store(ticks_from_ms(1200), Ordering::SeqCst);
    w.active.set(false);
    c.note_interaction();
    assert_eq!(c.activations(), 1);
    s.dispose();
}
#[test]
fn active_window_interaction_warms_the_gpu_then_stops_and_closing_returns_the_device() {
    // C#: ShellKeepWarmTests.ActiveWindowInteraction_WarmsTheGpuThenStops_AndClosingReturnsTheDevice
    let w = Rc::new(FakeKeepWarmWindow::default());
    w.active.set(true);
    let now = Arc::new(AtomicI64::new(ticks_from_ms(1000)));
    let dropped = Arc::new(AtomicBool::new(false));
    let mut r = registration(true, false, false);
    let clock = now.clone();
    r.timestamp = Some(Arc::new(move || clock.load(Ordering::SeqCst)));
    let flag = dropped.clone();
    r.work = Some(Box::new(move || Ok(Some(Box::new(Work(Some(flag)))))));
    let mut s = register(&w, r);
    s.controller.as_ref().unwrap().note_interaction();
    assert!(s.keep_alive.as_ref().unwrap().has_thread());
    now.store(ticks_from_ms(7000), Ordering::SeqCst);
    assert!(s.keep_alive.as_ref().unwrap().wait_blocked(1, Duration::from_secs(5)));
    s.dispose();
    assert!(s.keep_alive.is_none());
    assert!(dropped.load(Ordering::SeqCst));
}
#[test]
fn controller_does_not_keep_the_window_alive() {
    let w = Rc::new(FakeKeepWarmWindow::default());
    let weak = Rc::downgrade(&w);
    let mut s = register(&w, registration(true, false, true));
    drop(w);
    assert!(weak.upgrade().is_none());
    s.controller.as_ref().unwrap().note_interaction();
    assert_eq!(s.controller.as_ref().unwrap().activations(), 0);
    s.dispose();
}
#[test]
fn frame_log_writes_frames_and_closing_the_window_releases_the_file() {
    // C#: ShellKeepWarmTests.FrameLog_WritesFrames_AndClosingTheWindowReleasesTheFile
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target-cx")
        .join(format!("frames-{}.log", std::process::id()));
    let w = Rc::new(FakeKeepWarmWindow::default());
    let mut r = registration(false, false, false);
    r.frame_log_path = Some(path.clone());
    let mut s = register(&w, r);
    s.frame_log.as_mut().unwrap().record(1, 2, 0.0, 0.0);
    s.dispose();
    assert!(std::fs::read_to_string(&path).unwrap().contains("F gap="));
    std::fs::remove_file(path).unwrap();
}
