//! Tests of W8-13b: `host::swap_chain_clock`. Port of the 5 swap-chain tests of
//! `tests/PhotoReview.Shell.Tests/Hosting/FrameClockTests.cs` (baseline `d92d6aae`) with a fake timestamp function and a fixed display clock, plus
//! the `new:` tests of the contract. The handle is `1` (never waited on) except in the semaphore test.
#![cfg(windows)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use pr_core::abstractions::{DisplayClock, DisplayTiming};
use pr_platform_win::host::error::HostError;
use pr_platform_win::host::frame_clock::base::{FrameTick, HostFrameClock, qpc_frequency};
use pr_platform_win::host::swap_chain_clock::{SwapChainFrameClock, SwapChainPolicy};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateSemaphoreW, ReleaseSemaphore, WaitForSingleObject};

// ---- fixtures ----

/// C# `FakeTime`.
#[derive(Clone)]
struct FakeTime(Rc<Cell<i64>>);

impl FakeTime {
    fn new(start: i64) -> Self {
        Self(Rc::new(Cell::new(start)))
    }
    fn set(&self, value: i64) {
        self.0.set(value);
    }
    fn advance(&self, by: i64) {
        self.0.set(self.0.get() + by);
    }
    fn read_fn(&self) -> Box<dyn Fn() -> i64> {
        let cell = Rc::clone(&self.0);
        Box::new(move || cell.get())
    }
}

/// C# `FixedDisplayClock`.
struct FixedDisplayClock(DisplayTiming);

impl DisplayClock for FixedDisplayClock {
    fn get_timing(&self, _window: isize) -> Option<DisplayTiming> {
        Some(self.0)
    }
}

fn period() -> i64 {
    qpc_frequency() / 60
}

fn clock_with(time: &FakeTime, display: Option<DisplayTiming>, fixed_period: Option<i64>) -> SwapChainFrameClock {
    let display_clock: Option<Arc<dyn DisplayClock>> =
        display.map(|timing| Arc::new(FixedDisplayClock(timing)) as Arc<dyn DisplayClock>);
    SwapChainFrameClock::with_seams(1, display_clock, 0, Some(time.read_fn()), fixed_period).unwrap()
}

fn recorder(log: &Rc<RefCell<Vec<i64>>>) -> Box<dyn FnMut(&FrameTick)> {
    let log = Rc::clone(log);
    Box::new(move |tick| log.borrow_mut().push(tick.timestamp))
}

// ---- ported from FrameClockTests ----

// C#: FrameClockTests.SwapChainClock_ZeroHandle_IsRejected
#[test]
fn swap_chain_clock_zero_handle_is_rejected() {
    let error = SwapChainFrameClock::create(0, None, 0).unwrap_err();
    assert_eq!(
        error,
        HostError::InvalidArgument {
            param: "frame_latency_wait_handle",
            message: "A swap chain frame latency waitable handle is required.",
        }
    );
}

// C#: FrameClockTests.SwapChainClock_Signal_TicksAndAnEarlyRepeatSignalIsDeferredToTheNextPeriod
#[test]
fn swap_chain_clock_signal_ticks_and_an_early_repeat_signal_is_deferred_to_the_next_period() {
    let time = FakeTime::new(100_000);
    let clock = clock_with(&time, None, Some(period()));
    let ticks = Rc::new(RefCell::new(Vec::new()));
    clock.subscribe(recorder(&ticks));

    assert_eq!(clock.deadline_timestamp(), 100_000 + 2 * period()); // fallback when nobody presents
    assert!(clock.on_signaled().unwrap());
    time.advance(period() / 4);
    assert!(!clock.on_signaled().unwrap()); // burst signal inside the same frame
    assert_eq!(clock.deadline_timestamp(), 100_000 + period());
    time.set(100_000 + period() - 1);
    assert!(!clock.on_deadline().unwrap());
    time.set(100_000 + period());
    assert!(clock.on_deadline().unwrap());

    assert_eq!(*ticks.borrow(), vec![100_000, 100_000 + period()]);
}

// C#: FrameClockTests.SwapChainClock_NoPresentAfterATick_FallsBackToTheTwoPeriodDeadline
#[test]
fn swap_chain_clock_no_present_after_a_tick_falls_back_to_the_two_period_deadline() {
    let time = FakeTime::new(0);
    let clock = clock_with(&time, None, Some(period()));
    clock.subscribe(Box::new(|_| {}));
    time.set(10);
    assert!(clock.on_signaled().unwrap());

    assert_eq!(clock.deadline_timestamp(), 10 + 2 * period());
    time.set(10 + 2 * period() - 1);
    assert!(!clock.on_deadline().unwrap());
    time.set(10 + 2 * period());
    assert!(clock.on_deadline().unwrap());
    assert_eq!(clock.tick_count(), 2);
}

// C#: FrameClockTests.SwapChainClock_NotArmed_HasNoDeadlineAndIgnoresSignals
#[test]
fn swap_chain_clock_not_armed_has_no_deadline_and_ignores_signals() {
    let time = FakeTime::new(0);
    let clock = clock_with(&time, None, Some(period()));

    assert_eq!(clock.deadline_timestamp(), i64::MAX);
    assert!(!clock.on_signaled().unwrap());
    time.set(i64::MAX / 2);
    assert!(!clock.on_deadline().unwrap());
}

// C#: FrameClockTests.SwapChainClock_WithDisplayTiming_ReportsTheNextVBlankAsFrameTime
#[test]
fn swap_chain_clock_with_display_timing_reports_the_next_vblank_as_frame_time() {
    let time = FakeTime::new(10_050);
    let timing = DisplayTiming {
        last_vblank: 10_000,
        refresh_period: 100,
    };
    let clock = clock_with(&time, Some(timing), None);
    let ticks = Rc::new(RefCell::new(Vec::new()));
    clock.subscribe(recorder(&ticks));

    assert!(clock.on_signaled().unwrap());

    assert_eq!(*ticks.borrow(), vec![10_100]);
}

// ---- new ----

// new: the handle is checked before the period, as in C#
#[test]
fn swap_chain_clock_rejects_non_positive_fixed_period_after_the_handle_check() {
    let both = SwapChainFrameClock::with_seams(0, None, 0, None, Some(0)).unwrap_err();
    assert!(matches!(both, HostError::InvalidArgument { .. }), "{both:?}");
    for bad in [0, -1, i64::MIN] {
        let error = SwapChainFrameClock::with_seams(1, None, 0, None, Some(bad)).unwrap_err();
        assert_eq!(error, HostError::ArgumentOutOfRange { param: "fixed_period" });
    }
    assert!(SwapChainPolicy::new(1, Some(1)).is_ok());
    assert_eq!(SwapChainPolicy::new(1, None).unwrap().period(), period());
}

// new (T4): the early-signal test is strict: exactly half a period later ticks
#[test]
fn swap_chain_clock_signal_exactly_at_half_a_period_ticks() {
    let time = FakeTime::new(1_000);
    let clock = clock_with(&time, None, Some(100));
    let ticks = Rc::new(RefCell::new(Vec::new()));
    clock.subscribe(recorder(&ticks));
    assert!(clock.on_signaled().unwrap()); // last_tick = 1000

    time.set(1_049);
    assert!(!clock.on_signaled().unwrap()); // 49 < 50: deferred
    assert!(clock.with_policy(|policy| policy.deferred()));
    time.set(1_050);
    assert!(clock.on_signaled().unwrap()); // 50 < 50 is false: ticks
    assert!(!clock.with_policy(|policy| policy.deferred()));

    assert_eq!(*ticks.borrow(), vec![1_000, 1_050]);
}

// new (T4): an odd period truncates `period / 2` like the C# integer division
#[test]
fn swap_chain_clock_odd_period_truncates_the_half_period() {
    let time = FakeTime::new(1_000);
    let clock = clock_with(&time, None, Some(101)); // half = 50
    clock.subscribe(Box::new(|_| {}));
    assert!(clock.on_signaled().unwrap());

    time.set(1_049);
    assert!(!clock.on_signaled().unwrap());
    time.set(1_050);
    assert!(clock.on_signaled().unwrap());
}

// new (T3): `deferred` is cleared by arming, so a stale `last_tick + period` deadline does not survive a re-arm
#[test]
fn swap_chain_clock_deferred_flag_is_cleared_on_rearm() {
    let time = FakeTime::new(1_000);
    let clock = clock_with(&time, None, Some(100));
    let id = clock.subscribe(Box::new(|_| {}));
    assert!(clock.on_signaled().unwrap()); // last_tick = 1000
    time.set(1_010);
    assert!(!clock.on_signaled().unwrap()); // early: deferred
    assert_eq!(clock.deadline_timestamp(), 1_000 + 100);

    clock.unsubscribe(id); // disarm: nothing is reset
    time.set(5_000);
    clock.subscribe(Box::new(|_| {})); // arm again at 5000

    assert!(!clock.with_policy(|policy| policy.deferred()));
    assert_eq!(clock.deadline_timestamp(), 5_000 + 2 * 100); // max(last_tick, armed_at) + 2 * period
}

// new (T3): `deferred` is also cleared by the tick that consumes it
#[test]
fn swap_chain_clock_deferred_flag_is_cleared_by_the_deadline_tick() {
    let time = FakeTime::new(1_000);
    let clock = clock_with(&time, None, Some(100));
    clock.subscribe(Box::new(|_| {}));
    assert!(clock.on_signaled().unwrap());
    time.set(1_010);
    assert!(!clock.on_signaled().unwrap());
    time.set(1_100);
    assert!(clock.on_deadline().unwrap());

    assert!(!clock.with_policy(|policy| policy.deferred()));
    assert_eq!(clock.deadline_timestamp(), 1_100 + 200);
}

// new (T2): deadline sums saturate instead of wrapping
#[test]
fn swap_chain_clock_deadline_saturates() {
    let time = FakeTime::new(10);
    let clock = clock_with(&time, None, Some(i64::MAX));
    let _id = clock.subscribe(Box::new(|_| {}));
    assert_eq!(clock.deadline_timestamp(), i64::MAX); // 10 + 2 * i64::MAX
    assert_eq!(clock.ticks_until_deadline(), i64::MAX);

    // The deferred branch: last_tick + period.
    assert!(clock.on_signaled().unwrap());
    time.set(11);
    assert!(!clock.on_signaled().unwrap());
    assert_eq!(clock.deadline_timestamp(), i64::MAX);

    // A big but not maximal period: last_tick near i64::MAX / 2.
    let time = FakeTime::new(i64::MAX / 2);
    let clock = clock_with(&time, None, Some(i64::MAX / 2));
    clock.subscribe(Box::new(|_| {}));
    assert_eq!(clock.deadline_timestamp(), i64::MAX);
}

// new: the period follows the display's refresh period unless a fixed period was given
#[test]
fn swap_chain_clock_period_follows_the_display_unless_fixed() {
    let timing = DisplayTiming {
        last_vblank: 10_000,
        refresh_period: 100,
    };

    let time = FakeTime::new(10_050);
    let following = clock_with(&time, Some(timing), None);
    following.subscribe(Box::new(|_| {}));
    assert_eq!(following.with_policy(|policy| policy.period()), period());
    assert!(following.on_signaled().unwrap());
    assert_eq!(following.with_policy(|policy| policy.period()), 100);

    let fixed = clock_with(&time, Some(timing), Some(777));
    fixed.subscribe(Box::new(|_| {}));
    assert!(fixed.on_signaled().unwrap());
    assert_eq!(fixed.with_policy(|policy| policy.period()), 777);

    // A timing with a refresh period of 0 is "unknown": the period stays.
    let unknown = DisplayTiming {
        last_vblank: 10_000,
        refresh_period: 0,
    };
    let unknown_clock = clock_with(&time, Some(unknown), None);
    unknown_clock.subscribe(Box::new(|_| {}));
    assert!(unknown_clock.on_signaled().unwrap());
    assert_eq!(unknown_clock.with_policy(|policy| policy.period()), period());
}

// new: the frame time is the vblank grid point but `last_tick` is the wall time `now`
#[test]
fn swap_chain_clock_next_tick_frame_is_the_grid_point_but_last_tick_is_now() {
    let time = FakeTime::new(10_050);
    let timing = DisplayTiming {
        last_vblank: 10_000,
        refresh_period: 100,
    };
    let clock = clock_with(&time, Some(timing), None);
    let ticks = Rc::new(RefCell::new(Vec::new()));
    clock.subscribe(recorder(&ticks));

    assert!(clock.on_signaled().unwrap());

    assert_eq!(*ticks.borrow(), vec![10_100]);
    assert_eq!(clock.with_policy(|policy| policy.last_tick()), 10_050);
    assert_eq!(clock.deadline_timestamp(), 10_050 + 2 * 100);
}

// new: the wait handle is the one given, and a disposed clock reports 0
#[test]
fn swap_chain_clock_reports_its_handle_until_disposed() {
    let time = FakeTime::new(0);
    let clock = SwapChainFrameClock::with_seams(42, None, 0, Some(time.read_fn()), Some(period())).unwrap();
    assert_eq!(clock.wait_handle(), 42);
    clock.dispose();
    assert_eq!(clock.wait_handle(), 0);
}

// new: the swap chain owns the handle; dropping or disposing the clock must not close it
#[test]
fn swap_chain_clock_does_not_close_the_handle() {
    // SAFETY: plain arguments (initial 0, maximum 1, no name, default security).
    let semaphore = unsafe { CreateSemaphoreW(None, 0, 1, None) }.unwrap();
    let raw = semaphore.0 as isize;
    let time = FakeTime::new(0);
    {
        let clock = SwapChainFrameClock::with_seams(raw, None, 0, Some(time.read_fn()), Some(period())).unwrap();
        clock.subscribe(Box::new(|_| {}));
        clock.dispose();
    } // dropped here

    // SAFETY: `semaphore` is still a valid handle created above; if the clock had closed it these calls would fail.
    unsafe {
        ReleaseSemaphore(semaphore, 1, None).expect("the handle must still be open after the clock is gone");
        assert_eq!(WaitForSingleObject(semaphore, 5_000), WAIT_OBJECT_0);
        CloseHandle(HANDLE(raw as *mut _)).unwrap();
    }
}

// new (W8-13b smoke, written after W8-12d merged): the real swap-chain frame-latency handle of a window surface drives the clock
#[test]
#[ignore = "native: needs desktop"]
fn swap_chain_clock_ticks_from_a_real_surface_handle() {
    use pr_platform_win::host::frame_clock::timer::TimerFrameClock;
    use pr_platform_win::host::shell_window::{ShellWindow, ShellWindowOptions};
    use pr_platform_win::render::draw_context::ColorF;
    use pr_platform_win::render::surface::{D2dRenderSurface, SurfaceOptions};

    // A hidden shell window is the target (it is never shown); the swap chain is the software (WARP) one.
    let timer: Rc<dyn HostFrameClock> = Rc::new(TimerFrameClock::create(None, 0).unwrap());
    let window = ShellWindow::create(ShellWindowOptions::default(), timer).unwrap();
    let hwnd = pr_platform_win::host::contracts::ShellWindowApi::hwnd(&window);
    let mut surface = D2dRenderSurface::create_window_surface(
        hwnd,
        SurfaceOptions {
            prefer_warp: true,
            ..SurfaceOptions::default()
        },
    )
    .unwrap();
    let handle = surface.frame_latency_wait_handle();
    assert_ne!(handle, 0, "a window surface has a frame-latency waitable object");

    let clock = SwapChainFrameClock::create(handle, None, hwnd).unwrap();
    assert_eq!(clock.wait_handle(), handle);
    let ticks = Rc::new(RefCell::new(Vec::new()));
    clock.subscribe(recorder(&ticks));
    assert!(clock.is_armed());

    surface.begin_draw_context().unwrap().clear(ColorF {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    });
    surface.end_draw_and_present().unwrap();

    // SAFETY: `handle` is the live waitable object of `surface`, which outlives the wait; waiting only reads it.
    let waited = unsafe { WaitForSingleObject(HANDLE(handle as *mut core::ffi::c_void), 60_000) };
    assert_eq!(waited, WAIT_OBJECT_0, "the handle signals within the bound");
    assert!(clock.on_signaled().unwrap());
    assert_eq!(ticks.borrow().len(), 1);
    clock.dispose();
    surface.dispose();
}
