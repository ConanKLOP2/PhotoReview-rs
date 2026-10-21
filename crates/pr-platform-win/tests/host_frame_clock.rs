//! Tests of W8-13a: `host::frame_clock::{base, timer}` and `host::error`. Port of the 12 timer-clock tests of
//! `tests/PhotoReview.Shell.Tests/Hosting/FrameClockTests.cs` (baseline `d92d6aae`) with a fake timestamp function and a fixed display clock.
//! The waitable-timer handle is real but nobody waits on it, except the one `#[ignore]`d test.
#![cfg(windows)]

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;

use pr_core::abstractions::{DisplayClock, DisplayTiming};
use pr_platform_win::gfx::com::GfxError;
use pr_platform_win::gfx::extras::WaitResult;
use pr_platform_win::host::error::HostError;
use pr_platform_win::host::frame_clock::base::{
    ClockEnv, ClockPolicy, FrameClockBase, FrameTick, HostFrameClock, next_grid_point, qpc_frequency, qpc_now,
    rendering_time_ms,
};
use pr_platform_win::host::frame_clock::timer::{
    NextDue, TimerFrameClock, compute_next_due, early_tolerance, relative_due_100ns,
};
use pr_platform_win::win32::structs::Win32Error;

// ---- fixtures ----

/// C# `FakeTime`.
#[derive(Clone)]
struct FakeTime(Rc<Cell<i64>>);

impl FakeTime {
    fn new(start: i64) -> Self {
        Self(Rc::new(Cell::new(start)))
    }
    fn now(&self) -> i64 {
        self.0.get()
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

fn fixed_display(timing: DisplayTiming) -> Option<Arc<dyn DisplayClock>> {
    Some(Arc::new(FixedDisplayClock(timing)))
}

fn period() -> i64 {
    qpc_frequency() / 60
}

fn timer_clock(time: &FakeTime, fixed_period: Option<i64>) -> TimerFrameClock {
    TimerFrameClock::with_seams(None, 0, Some(time.read_fn()), fixed_period).unwrap()
}

/// A handler that records the timestamp of every tick it sees.
fn recorder(log: &Rc<RefCell<Vec<i64>>>) -> Box<dyn FnMut(&FrameTick)> {
    let log = Rc::clone(log);
    Box::new(move |tick| log.borrow_mut().push(tick.timestamp))
}

// ---- ported from FrameClockTests ----

// C#: FrameClockTests.TimerClock_WithoutSubscriberOrRequest_IsNotArmedAndNeverTicks
#[test]
fn timer_clock_without_subscriber_or_request_is_not_armed_and_never_ticks() {
    let time = FakeTime::new(10_000);
    let clock = timer_clock(&time, Some(period()));

    assert!(!clock.is_armed());
    assert_eq!(clock.scheduled_frame(), None);
    assert!(!clock.on_signaled().unwrap());
    assert!(!clock.on_deadline().unwrap());
    assert_eq!(clock.tick_count(), 0);
    assert_eq!(clock.ticks_until_deadline(), i64::MAX);
}

// C#: FrameClockTests.TimerClock_Subscribed_TicksAtMostOncePerFrame
#[test]
fn timer_clock_subscribed_ticks_at_most_once_per_frame() {
    let time = FakeTime::new(10_000);
    let clock = timer_clock(&time, Some(period()));
    let ticks = Rc::new(RefCell::new(Vec::new()));
    clock.subscribe(recorder(&ticks));

    assert!(clock.is_armed());
    assert_eq!(clock.scheduled_frame(), Some(10_000)); // first frame: at once

    assert!(clock.on_signaled().unwrap());
    assert_eq!(clock.scheduled_frame(), Some(10_000 + period()));
    assert!(!clock.on_signaled().unwrap()); // same frame: no second tick
    time.advance(period() / 2);
    assert!(!clock.on_signaled().unwrap()); // half a frame: not yet
    time.set(10_000 + period());
    assert!(clock.on_signaled().unwrap());
    assert!(!clock.on_deadline().unwrap()); // the timer has no deadline of its own

    assert_eq!(*ticks.borrow(), vec![10_000, 10_000 + period()]);
    assert_eq!(clock.tick_count(), 2);
}

// C#: FrameClockTests.TimerClock_SignalWithinEarlyTolerance_TicksWithTheScheduledFrameTime
#[test]
fn timer_clock_signal_within_early_tolerance_ticks_with_the_scheduled_frame_time() {
    let time = FakeTime::new(0);
    let clock = timer_clock(&time, Some(period() * 1_000_000));
    let ticks = Rc::new(RefCell::new(Vec::new()));
    clock.subscribe(recorder(&ticks));
    assert!(clock.on_signaled().unwrap());
    let due = clock.scheduled_frame().unwrap();

    time.set(due - early_tolerance() - 1);
    assert!(!clock.on_signaled().unwrap());
    assert_eq!(clock.scheduled_frame(), Some(due)); // still scheduled for exactly that frame
    time.set(due - early_tolerance());
    assert!(clock.on_signaled().unwrap());

    assert_eq!(*ticks.borrow(), vec![0, due]);
}

// C#: FrameClockTests.TimerClock_LateSignal_SkipsMissedFramesInsteadOfBursting
#[test]
fn timer_clock_late_signal_skips_missed_frames_instead_of_bursting() {
    let time = FakeTime::new(0);
    let clock = timer_clock(&time, Some(period()));
    let ticks = Rc::new(RefCell::new(Vec::new()));
    clock.subscribe(recorder(&ticks));
    assert!(clock.on_signaled().unwrap());

    time.set(3 * period() + period() / 2); // the UI was busy for 3.5 frames
    assert!(clock.on_signaled().unwrap());
    assert!(!clock.on_signaled().unwrap());

    assert_eq!(*ticks.borrow().last().unwrap(), time.now()); // anchored to the present
    assert_eq!(clock.scheduled_frame(), Some(time.now() + period()));
}

// C#: FrameClockTests.TimerClock_Unsubscribed_DisarmsAndIgnoresAStaleSignal
#[test]
fn timer_clock_unsubscribed_disarms_and_ignores_a_stale_signal() {
    let time = FakeTime::new(5);
    let clock = timer_clock(&time, Some(period()));
    let count = Rc::new(Cell::new(0));
    let counter = Rc::clone(&count);
    let id = clock.subscribe(Box::new(move |_| counter.set(counter.get() + 1)));
    assert!(clock.on_signaled().unwrap());

    clock.unsubscribe(id);
    time.advance(period());

    assert!(!clock.is_armed());
    assert_eq!(clock.scheduled_frame(), None);
    assert!(!clock.on_signaled().unwrap());
    assert_eq!(count.get(), 1);
}

// C#: FrameClockTests.RequestFrame_WithoutSubscriber_TicksOnceThenDisarms
#[test]
fn request_frame_without_subscriber_ticks_once_then_disarms() {
    let time = FakeTime::new(1_000);
    let clock = timer_clock(&time, Some(period()));

    clock.request_frame();
    clock.request_frame(); // coalesced
    assert!(clock.is_armed());
    assert!(clock.on_signaled().unwrap());

    assert!(!clock.is_armed());
    time.advance(period());
    assert!(!clock.on_signaled().unwrap());
    assert_eq!(clock.tick_count(), 1);
}

// C#: FrameClockTests.RequestFrame_FromInsideAHandler_KeepsTheClockArmedForOneMoreFrame
#[test]
fn request_frame_from_inside_a_handler_keeps_the_clock_armed_for_one_more_frame() {
    let time = FakeTime::new(1_000);
    let clock = Rc::new(timer_clock(&time, Some(period())));
    let count = Rc::new(Cell::new(0));
    let own_id = Rc::new(Cell::new(0_u64));
    let handler_clock = Rc::clone(&clock);
    let handler_count = Rc::clone(&count);
    let handler_id = Rc::clone(&own_id);
    let id = clock.subscribe(Box::new(move |_| {
        handler_count.set(handler_count.get() + 1);
        handler_clock.unsubscribe(handler_id.get());
        if handler_count.get() == 1 {
            handler_clock.request_frame();
        }
    }));
    own_id.set(id);

    assert!(clock.on_signaled().unwrap());
    assert!(clock.is_armed());
    time.advance(period());
    assert!(clock.on_signaled().unwrap());
    assert!(!clock.is_armed());
    assert_eq!(count.get(), 1); // the second frame has no subscriber any more
    assert_eq!(clock.tick_count(), 2);
}

// C#: FrameClockTests.Frame_ReportsRenderingTimeSinceTheClockStartedAndTheDisplayTiming
#[test]
fn frame_reports_rendering_time_since_the_clock_started_and_the_display_timing() {
    let time = FakeTime::new(1_000);
    let timing = DisplayTiming {
        last_vblank: 900,
        refresh_period: period(),
    };
    let clock = TimerFrameClock::with_seams(fixed_display(timing), 0, Some(time.read_fn()), Some(period())).unwrap();
    let seen: Rc<Cell<Option<FrameTick>>> = Rc::new(Cell::new(None));
    let sink = Rc::clone(&seen);
    clock.subscribe(Box::new(move |tick| sink.set(Some(*tick))));

    assert!(clock.on_signaled().unwrap());

    let tick = seen.get().unwrap();
    assert_eq!(tick.timing, Some(timing));
    let expected = (tick.timestamp - 1_000) as f64 * 1000.0 / qpc_frequency() as f64;
    assert!(
        (tick.rendering_time_ms - expected).abs() < 5e-10,
        "{} vs {expected}",
        tick.rendering_time_ms
    );
}

// C#: FrameClockTests.TimerClock_WithDisplayTimingAndNoFixedPeriod_AnchorsFramesToTheVBlankGrid
#[test]
fn timer_clock_with_display_timing_and_no_fixed_period_anchors_frames_to_the_vblank_grid() {
    let refresh = qpc_frequency() / 100; // 100 Hz
    let time = FakeTime::new(50_000_000);
    let timing = DisplayTiming {
        last_vblank: 50_000_000 - refresh / 3,
        refresh_period: refresh,
    };
    let clock = TimerFrameClock::with_seams(fixed_display(timing), 0, Some(time.read_fn()), None).unwrap();
    clock.subscribe(Box::new(|_| {}));

    // The first frame after idle is at once (not the grid); from the second frame on (continuous animation) it snaps to the vblank grid.
    assert_eq!(clock.scheduled_frame(), Some(time.now()));
    assert_eq!(clock.period(), refresh);

    assert!(clock.on_signaled().unwrap());
    let second = clock.scheduled_frame().unwrap();
    assert_eq!(second, timing.last_vblank + refresh); // first grid point >= previous frame + half a period

    time.set(second);
    assert!(clock.on_signaled().unwrap());
    assert_eq!(clock.scheduled_frame(), Some(second + refresh));
}

// C#: FrameClockTests.TimerClock_WithDisplayTiming_FrameAfterAnIdleGapIsImmediateAgain_ButABurstStaysOnTheGrid
#[test]
fn timer_clock_with_display_timing_frame_after_an_idle_gap_is_immediate_again_but_a_burst_stays_on_the_grid() {
    let refresh = qpc_frequency() / 100;
    let time = FakeTime::new(50_000_000);
    let timing = DisplayTiming {
        last_vblank: 50_000_000 - refresh / 3,
        refresh_period: refresh,
    };
    let clock = TimerFrameClock::with_seams(fixed_display(timing), 0, Some(time.read_fn()), None).unwrap();
    let id = clock.subscribe(Box::new(|_| {}));
    assert!(clock.on_signaled().unwrap());
    clock.unsubscribe(id);

    // The next request inside the same period (not idle): still waits for the grid point, no burst.
    time.advance(refresh / 10);
    let id = clock.subscribe(Box::new(|_| {}));
    assert!(clock.scheduled_frame().unwrap() > time.now());
    clock.unsubscribe(id);

    // Idle for at least a period since the last frame: draw at once, at the request time.
    time.advance(refresh * 3);
    clock.subscribe(Box::new(|_| {}));
    assert_eq!(clock.scheduled_frame(), Some(time.now()));
}

// C#: FrameClockTests.FrameClock_UsedOffItsUiThread_Throws
// The Rust clock is `!Send`: a call from another thread does not compile (D-W8B3H-4, T10). Compile-time proof:
trait AmbiguousIfSend<A> {
    fn some_item() {}
}
impl<T: ?Sized> AmbiguousIfSend<()> for T {}
impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}

#[test]
fn frame_clock_base_is_not_send() {
    // new: if `TimerFrameClock` ever became `Send`, the call below would be ambiguous and this file would not compile.
    let _ = <TimerFrameClock as AmbiguousIfSend<_>>::some_item;
    let _ = <FrameClockBase<ScriptPolicy> as AmbiguousIfSend<_>>::some_item;
}

// C#: FrameClockTests.FrameClock_Disposed_DisarmsAndDropsSubscribers
#[test]
fn frame_clock_disposed_disarms_and_drops_subscribers() {
    let time = FakeTime::new(1_000);
    let clock = timer_clock(&time, Some(period()));
    clock.subscribe(Box::new(|_| {}));

    clock.dispose();
    clock.dispose();

    assert!(!clock.is_armed());
    assert!(!clock.on_signaled().unwrap());
}

// C#: FrameClockTests.FrameHandlerThrows_PropagatesAndTheClockStaysConsistent
#[test]
fn frame_handler_throws_propagates_and_the_clock_stays_consistent() {
    let time = FakeTime::new(1_000);
    let clock = timer_clock(&time, Some(period()));
    clock.subscribe(Box::new(|_| panic!("render failed")));

    let outcome = catch_unwind(AssertUnwindSafe(|| clock.on_signaled()));

    let payload = outcome.unwrap_err();
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"render failed"));
    assert!(clock.is_armed());
    assert_eq!(clock.scheduled_frame(), Some(1_000 + period())); // the next frame is still scheduled
}

// ---- new: pure functions ----

// new: FrameTick equality
#[test]
fn frame_tick_equality_is_double_equals() {
    let tick = |timestamp: i64, ms: f64, timing: Option<DisplayTiming>| FrameTick {
        timestamp,
        rendering_time_ms: ms,
        timing,
    };
    let timing = Some(DisplayTiming {
        last_vblank: 1,
        refresh_period: 2,
    });
    assert_eq!(tick(5, f64::NAN, None), tick(5, f64::NAN, None));
    assert_eq!(tick(5, 0.0, None), tick(5, -0.0, None));
    assert_eq!(tick(5, 1.5, timing), tick(5, 1.5, timing));
    assert_ne!(tick(5, 1.5, None), tick(6, 1.5, None));
    assert_ne!(tick(5, 1.5, None), tick(5, 1.25, None));
    assert_ne!(tick(5, f64::NAN, None), tick(5, 1.0, None));
    assert_ne!(tick(5, 1.5, timing), tick(5, 1.5, None));
    assert_ne!(
        tick(5, 1.5, timing),
        tick(
            5,
            1.5,
            Some(DisplayTiming {
                last_vblank: 1,
                refresh_period: 3
            })
        )
    );
}

// new: NextGridPoint table
#[test]
fn next_grid_point_table() {
    let timing = |last_vblank: i64, refresh_period: i64| DisplayTiming {
        last_vblank,
        refresh_period,
    };
    assert_eq!(next_grid_point(timing(1_000, 0), 500), 1_000); // refresh <= 0: max(at, last)
    assert_eq!(next_grid_point(timing(1_000, -5), 2_000), 2_000);
    assert_eq!(next_grid_point(timing(1_000, 100), 900), 1_000); // at < last
    assert_eq!(next_grid_point(timing(1_000, 100), 1_000), 1_000); // at == last
    assert_eq!(next_grid_point(timing(1_000, 100), 1_200), 1_200); // exactly on a grid point
    assert_eq!(next_grid_point(timing(1_000, 100), 1_201), 1_300); // one tick after
    assert_eq!(next_grid_point(timing(1_000, 100), 1_001), 1_100);
    let far = i64::MAX / 2;
    let expected = (i128::from(far) + 999) / 1000 * 1000;
    assert_eq!(i128::from(next_grid_point(timing(0, 1_000), far)), expected);
    assert_eq!(next_grid_point(timing(0, i64::MAX), 1), i64::MAX); // refresh_period = i64::MAX
    assert_eq!(next_grid_point(timing(0, 1_000), i64::MAX), i64::MAX); // result beyond i64 saturates
    assert_eq!(next_grid_point(timing(i64::MIN, 10), i64::MAX), i64::MAX);
}

// new: rendering time formula
#[test]
fn rendering_time_ms_matches_the_formula() {
    assert_eq!(rendering_time_ms(1_000, 1_000, 10_000_000), 0.0);
    assert_eq!(rendering_time_ms(10_001_000, 1_000, 10_000_000), 1000.0);
    assert_eq!(rendering_time_ms(11_000, 1_000, 10_000_000), 1.0);
    assert_eq!(rendering_time_ms(500, 1_000, 1_000), -500.0 * 1000.0 / 1000.0);
    assert!((rendering_time_ms(i64::MAX, 0, 1_000_000_000) - i64::MAX as f64 / 1_000_000.0).abs() < 1e6);
}

// new
#[test]
fn qpc_frequency_is_positive() {
    assert!(qpc_frequency() > 0);
    // `qpc_now` is the live counter: it moves (bounded busy loop, no sleeping) and is not a constant.
    let first = qpc_now();
    let mut next = first;
    for _ in 0..50_000_000 {
        next = qpc_now();
        if next > first {
            break;
        }
    }
    assert!(first > 1 && next > first);
}

// new
#[test]
fn early_tolerance_is_a_five_hundredth_of_a_second() {
    assert_eq!(early_tolerance(), qpc_frequency() / 500);
    assert!(early_tolerance() > 0);
}

// new: ArmTimer arithmetic (T1)
#[test]
fn relative_due_100ns_table() {
    assert_eq!(relative_due_100ns(500, 500, 10_000_000), -1); // "now" still fires
    assert_eq!(relative_due_100ns(400, 500, 10_000_000), -1); // already due
    assert_eq!(relative_due_100ns(10_500, 500, 10_000_000), -10_000); // 1 ms at 10 MHz
    assert_eq!(relative_due_100ns(1_500, 500, 1_000), -10_000_000); // 1 s at 1 kHz
    // The product is 1e19 > i64::MAX: it must not wrap into "fire now".
    assert_eq!(
        relative_due_100ns(1_000_000_000_500, 500, 10_000_000),
        -1_000_000_000_000
    );
    assert_eq!(relative_due_100ns(i64::MAX, i64::MIN, 1), -i64::MAX); // clamps
    assert_eq!(relative_due_100ns(i64::MAX, 0, 10_000_000), -i64::MAX);
    assert_eq!(relative_due_100ns(1, 0, 0), -10_000_000); // a zero frequency cannot divide by zero
}

// new: the four rules of Schedule
#[test]
fn compute_next_due_table() {
    let grid = Some(DisplayTiming {
        last_vblank: 1_000,
        refresh_period: 100,
    });
    let due = |fixed, period, has_last, last, now, timing| compute_next_due(fixed, period, has_last, last, now, timing);

    // Fixed period: rule 4 only, the timing is ignored and the period never changes.
    assert_eq!(
        due(Some(50), 50, false, 0, 1_234, grid),
        NextDue { due: 1_234, period: 50 }
    );
    assert_eq!(
        due(Some(50), 50, true, 1_000, 1_020, grid),
        NextDue { due: 1_050, period: 50 }
    );
    assert_eq!(
        due(Some(50), 50, true, 1_000, 1_100, grid),
        NextDue { due: 1_100, period: 50 }
    );
    assert_eq!(
        due(Some(50), 50, true, 1_000, 1_050, None),
        NextDue { due: 1_050, period: 50 }
    );
    // No fixed period, timing unknown or without a refresh period: rule 4 with the old period.
    assert_eq!(due(None, 60, false, 0, 500, None), NextDue { due: 500, period: 60 });
    assert_eq!(due(None, 60, true, 480, 500, None), NextDue { due: 540, period: 60 });
    let no_refresh = Some(DisplayTiming {
        last_vblank: 1_000,
        refresh_period: 0,
    });
    assert_eq!(
        due(None, 60, true, 480, 500, no_refresh),
        NextDue { due: 540, period: 60 }
    );
    // Rule 1 + 2: timing known, no previous frame: the period follows the display, draw at once.
    assert_eq!(
        due(None, 60, false, 0, 1_234, grid),
        NextDue {
            due: 1_234,
            period: 100
        }
    );
    // Rule 2 boundary: idle for exactly one period draws at once, one tick less stays on the grid (rule 3).
    assert_eq!(
        due(None, 60, true, 1_134, 1_234, grid),
        NextDue {
            due: 1_234,
            period: 100
        }
    );
    assert_eq!(
        due(None, 60, true, 1_135, 1_234, grid),
        NextDue {
            due: 1_300,
            period: 100
        }
    );
    // Rule 3: earliest = max(now, last + period / 2), then the first grid point >= earliest.
    assert_eq!(
        due(None, 100, true, 1_200, 1_234, grid),
        NextDue {
            due: 1_300,
            period: 100
        }
    );
    assert_eq!(
        due(None, 100, true, 1_184, 1_200, grid),
        NextDue {
            due: 1_300,
            period: 100
        }
    ); // 1_234 -> 1_300
    assert_eq!(
        due(None, 100, true, 1_150, 1_200, grid),
        NextDue {
            due: 1_200,
            period: 100
        }
    ); // half-period boundary
    assert_eq!(
        due(None, 100, true, 1_149, 1_200, grid),
        NextDue {
            due: 1_200,
            period: 100
        }
    );
    assert_eq!(
        due(None, 100, true, 1_151, 1_200, grid),
        NextDue {
            due: 1_300,
            period: 100
        }
    ); // 1_201 -> 1_300
    // The idle test uses the period updated by rule 1 (100), not the old one (60): 80 ticks since the last frame is not idle.
    assert_eq!(
        due(None, 60, true, 1_150, 1_230, grid),
        NextDue {
            due: 1_300,
            period: 100
        }
    );
}

// ---- new: clock behaviour ----

// new: T4
#[test]
fn snapshot_semantics_unsubscribed_later_handler_still_runs_this_tick() {
    let time = FakeTime::new(0);
    let clock = Rc::new(timer_clock(&time, Some(period())));
    let log = Rc::new(RefCell::new(Vec::<&'static str>::new()));
    let second_id = Rc::new(Cell::new(0_u64));

    let (a_clock, a_log, a_second) = (Rc::clone(&clock), Rc::clone(&log), Rc::clone(&second_id));
    clock.subscribe(Box::new(move |_| {
        a_log.borrow_mut().push("a");
        a_clock.unsubscribe(a_second.get());
    }));
    let b_log = Rc::clone(&log);
    second_id.set(clock.subscribe(Box::new(move |_| b_log.borrow_mut().push("b"))));
    let c_log = Rc::clone(&log);
    clock.subscribe(Box::new(move |_| c_log.borrow_mut().push("c")));

    assert!(clock.on_signaled().unwrap());
    assert_eq!(*log.borrow(), vec!["a", "b", "c"]); // b was present when the tick started
    log.borrow_mut().clear();
    time.advance(period());
    assert!(clock.on_signaled().unwrap());
    assert_eq!(*log.borrow(), vec!["a", "c"]); // but not from the next tick on
}

// new: T4
#[test]
fn handler_added_during_a_tick_is_called_from_the_next_tick() {
    let time = FakeTime::new(0);
    let clock = Rc::new(timer_clock(&time, Some(period())));
    let log = Rc::new(RefCell::new(Vec::<&'static str>::new()));
    let added = Rc::new(Cell::new(false));

    let (h_clock, h_log, h_added) = (Rc::clone(&clock), Rc::clone(&log), Rc::clone(&added));
    clock.subscribe(Box::new(move |_| {
        h_log.borrow_mut().push("first");
        if !h_added.replace(true) {
            let inner = Rc::clone(&h_log);
            h_clock.subscribe(Box::new(move |_| inner.borrow_mut().push("late")));
        }
    }));

    assert!(clock.on_signaled().unwrap());
    assert_eq!(*log.borrow(), vec!["first"]);
    time.advance(period());
    assert!(clock.on_signaled().unwrap());
    assert_eq!(*log.borrow(), vec!["first", "first", "late"]);
}

// new
#[test]
fn unsubscribe_inside_own_handler_and_unknown_id_is_a_noop() {
    let time = FakeTime::new(0);
    let clock = Rc::new(timer_clock(&time, Some(period())));
    let calls = Rc::new(Cell::new(0));
    let own = Rc::new(Cell::new(0_u64));
    let (h_clock, h_calls, h_own) = (Rc::clone(&clock), Rc::clone(&calls), Rc::clone(&own));
    own.set(clock.subscribe(Box::new(move |_| {
        h_calls.set(h_calls.get() + 1);
        h_clock.unsubscribe(h_own.get());
        h_clock.unsubscribe(h_own.get()); // twice: the second one is a no-op
    })));

    clock.unsubscribe(9_999); // unknown id
    assert!(clock.is_armed());
    assert!(clock.on_signaled().unwrap());
    assert!(!clock.is_armed());
    assert_eq!(clock.scheduled_frame(), None);
    time.advance(period());
    assert!(!clock.on_signaled().unwrap());
    assert_eq!(calls.get(), 1);
}

// new
#[test]
fn ids_are_distinct_increasing_and_never_reused() {
    let time = FakeTime::new(0);
    let clock = timer_clock(&time, Some(period()));
    let first = clock.subscribe(Box::new(|_| {}));
    let second = clock.subscribe(Box::new(|_| {}));
    clock.unsubscribe(second);
    let third = clock.subscribe(Box::new(|_| {}));
    assert_eq!(first, 1);
    assert!(first < second && second < third);
    assert_ne!(third, second);
}

// new: an unsubscribed handler is dropped at once outside a tick, and by the end of the tick inside one
#[test]
fn unsubscribed_handlers_are_dropped_not_kept_as_tombstones() {
    let time = FakeTime::new(0);
    let clock = Rc::new(timer_clock(&time, Some(period())));
    let witness = Rc::new(());

    let held = Rc::clone(&witness);
    let id = clock.subscribe(Box::new(move |_| {
        let _keep = &held;
    }));
    assert_eq!(Rc::strong_count(&witness), 2);
    clock.unsubscribe(id);
    assert_eq!(Rc::strong_count(&witness), 1); // outside a tick: dropped right away

    let held = Rc::clone(&witness);
    let own = Rc::new(Cell::new(0_u64));
    let (h_clock, h_own) = (Rc::clone(&clock), Rc::clone(&own));
    own.set(clock.subscribe(Box::new(move |_| {
        let _keep = &held;
        h_clock.unsubscribe(h_own.get());
    })));
    assert_eq!(Rc::strong_count(&witness), 2);
    assert!(clock.on_signaled().unwrap());
    assert_eq!(Rc::strong_count(&witness), 1); // inside a tick: dropped when the tick ends

    // A handler that already ran and is unsubscribed later in the same tick is dropped by the end of that tick (the purge)...
    let held = Rc::clone(&witness);
    let first = clock.subscribe(Box::new(move |_| {
        let _keep = &held;
    }));
    let (u_clock, u_first) = (Rc::clone(&clock), first);
    clock.subscribe(Box::new(move |_| u_clock.unsubscribe(u_first)));
    assert_eq!(Rc::strong_count(&witness), 2);
    time.advance(period());
    assert!(clock.on_signaled().unwrap());
    assert_eq!(Rc::strong_count(&witness), 1);

    // ...and a handler removed from inside its own run is dropped before the next handler of that tick runs.
    let held = Rc::clone(&witness);
    let own = Rc::new(Cell::new(0_u64));
    let (h_clock, h_own) = (Rc::clone(&clock), Rc::clone(&own));
    own.set(clock.subscribe(Box::new(move |_| {
        let _keep = &held;
        h_clock.unsubscribe(h_own.get());
    })));
    let seen_count = Rc::new(Cell::new(0));
    let (probe, sink) = (Rc::clone(&witness), Rc::clone(&seen_count));
    clock.subscribe(Box::new(move |_| sink.set(Rc::strong_count(&probe))));
    time.advance(period());
    assert!(clock.on_signaled().unwrap());
    assert_eq!(seen_count.get(), 2); // the probe's own clone + the witness itself: the self-removed handler is already gone

    // Dispose drops the subscribers too.
    let held = Rc::clone(&witness);
    clock.subscribe(Box::new(move |_| {
        let _keep = &held;
    }));
    assert_eq!(Rc::strong_count(&witness), 3); // + the probe's clone
    clock.dispose();
    assert_eq!(Rc::strong_count(&witness), 1);
}

// new: T3
#[test]
fn a_panicking_handler_does_not_stop_the_clock_bookkeeping() {
    let time = FakeTime::new(1_000);
    let clock = timer_clock(&time, Some(period()));
    let later_calls = Rc::new(Cell::new(0));
    let first_calls = Rc::new(Cell::new(0));
    let fc = Rc::clone(&first_calls);
    clock.subscribe(Box::new(move |_| {
        fc.set(fc.get() + 1);
        if fc.get() == 1 {
            panic!("boom");
        }
    }));
    let lc = Rc::clone(&later_calls);
    clock.subscribe(Box::new(move |_| lc.set(lc.get() + 1)));

    assert!(catch_unwind(AssertUnwindSafe(|| clock.on_signaled())).is_err());

    assert_eq!(clock.tick_count(), 1);
    assert!(clock.is_armed());
    assert_eq!(clock.scheduled_frame(), Some(1_000 + period()));
    assert_eq!(later_calls.get(), 0); // a panic aborts the rest of the tick, like the C# exception out of `Invoke`
    // The panicking handler is still subscribed and the clock keeps ticking.
    time.advance(period());
    assert!(clock.on_signaled().unwrap());
    assert_eq!(first_calls.get(), 2);
    assert_eq!(later_calls.get(), 1);
}

// new: the failing request inside a handler also keeps `requested` cleared before the handlers (T9)
#[test]
fn a_request_made_before_the_tick_is_cleared_by_the_tick() {
    let time = FakeTime::new(1_000);
    let clock = timer_clock(&time, Some(period()));
    clock.request_frame();
    assert!(clock.is_armed());
    assert!(clock.on_signaled().unwrap());
    assert!(!clock.is_armed());
}

// new
#[test]
fn dispose_cancels_the_timer_and_zeroes_the_wait_handle() {
    let time = FakeTime::new(1_000);
    let clock = timer_clock(&time, Some(period()));
    let id = clock.subscribe(Box::new(|_| {}));
    assert_ne!(clock.wait_handle(), 0);
    assert_eq!(clock.scheduled_frame(), Some(1_000));

    clock.dispose();
    assert_eq!(clock.wait_handle(), 0);
    assert_eq!(clock.scheduled_frame(), None);
    assert!(!clock.is_armed());
    clock.dispose(); // idempotent

    let after = clock.subscribe(Box::new(|_| {}));
    assert!(after > id);
    clock.request_frame();
    assert!(!clock.is_armed());
    assert_eq!(clock.scheduled_frame(), None);
    assert!(!clock.on_signaled().unwrap());
    clock.unsubscribe(id); // after dispose: a no-op
}

// new: `wait_handle` is the timer's own handle
#[test]
fn wait_handle_is_the_timers_handle() {
    let time = FakeTime::new(0);
    let clock = timer_clock(&time, Some(period()));
    let raw = clock.with_policy(|policy| policy.timer().handle().0 as isize);
    assert_ne!(raw, 0);
    assert_eq!(clock.wait_handle(), raw);
}

// new: a deadline call never ticks the timer clock, even when a frame is due
#[test]
fn on_deadline_does_not_tick_a_due_timer_clock() {
    let time = FakeTime::new(1_000);
    let clock = timer_clock(&time, Some(period()));
    clock.subscribe(Box::new(|_| {}));
    assert_eq!(clock.scheduled_frame(), Some(1_000)); // due now

    assert!(!clock.on_deadline().unwrap());
    assert_eq!(clock.tick_count(), 0);
    assert_eq!(clock.scheduled_frame(), Some(1_000));
    assert!(clock.on_signaled().unwrap());
}

// new: arming really programs the timer: a frame due now makes the handle signal (bounded wait, not a poll; the timer fires within ~1 ms)
#[test]
fn arming_programs_the_timer_for_the_scheduled_frame() {
    let clock = TimerFrameClock::with_seams(None, 0, None, Some(qpc_frequency() / 1000)).unwrap();
    clock.request_frame();
    assert_eq!(
        clock.with_policy(|policy| policy.timer().wait(10_000, false)),
        WaitResult::Signaled
    );
    assert!(clock.on_signaled().unwrap());
    assert!(!clock.is_armed());
}

// new: the only test that uses the real timer
#[test]
#[ignore = "native: waits on a real waitable timer"]
fn disarm_cancels_the_timer() {
    let clock = TimerFrameClock::with_seams(None, 0, None, Some(qpc_frequency() / 1000)).unwrap();
    let id = clock.subscribe(Box::new(|_| {}));

    // The timer was armed for "now": it signals within the bound.
    assert_eq!(
        clock.with_policy(|policy| policy.timer().wait(5_000, false)),
        WaitResult::Signaled
    );
    assert!(clock.on_signaled().unwrap());
    // The tick re-armed the timer for the next 1 ms frame; disarming cancels it.
    clock.unsubscribe(id);
    assert_eq!(
        clock.with_policy(|policy| policy.timer().wait(200, false)),
        WaitResult::Timeout
    );
    assert!(!clock.on_signaled().unwrap());
    assert!(!clock.is_armed());
}

// new
#[test]
fn constructor_rejects_non_positive_fixed_period() {
    for bad in [0, -1, i64::MIN] {
        let error = TimerFrameClock::with_seams(None, 0, None, Some(bad)).unwrap_err();
        assert_eq!(error, HostError::ArgumentOutOfRange { param: "fixed_period" });
    }
    assert!(TimerFrameClock::with_seams(None, 0, None, Some(1)).is_ok());
}

// new
#[test]
fn create_uses_real_qpc_and_a_sixtieth_of_a_second() {
    let clock = TimerFrameClock::create(None, 0).unwrap();
    assert_eq!(clock.period(), qpc_frequency() / 60);
    assert!(!clock.is_armed());
    assert_ne!(clock.wait_handle(), 0);
}

// new
#[test]
fn timer_clock_follows_the_display_period_when_known() {
    let time = FakeTime::new(0);
    let timing = DisplayTiming {
        last_vblank: 0,
        refresh_period: 123_456,
    };
    let clock = TimerFrameClock::with_seams(fixed_display(timing), 7, Some(time.read_fn()), None).unwrap();
    assert_eq!(clock.period(), qpc_frequency() / 60);
    clock.request_frame();
    assert_eq!(clock.period(), 123_456);
}

// new
#[test]
fn host_error_display_texts() {
    let win32 = HostError::Win32 {
        operation: "x",
        error: Win32Error(5),
    };
    assert_eq!(win32.to_string(), "x (Win32 error 5)");
    let gfx = HostError::from(GfxError::new("SetWaitableTimer", 0x8007_0005_u32 as i32));
    assert_eq!(gfx.to_string(), "SetWaitableTimer failed with HRESULT 0x80070005");
    assert_eq!(
        gfx,
        HostError::Gfx(GfxError::new("SetWaitableTimer", 0x8007_0005_u32 as i32))
    );
    let invalid = HostError::InvalidOperation {
        message: "MessageLoop.Run is already running on this thread.",
    };
    assert_eq!(
        invalid.to_string(),
        "MessageLoop.Run is already running on this thread."
    );
    let argument = HostError::InvalidArgument {
        param: "waitHandle",
        message: "A wait handle is required.",
    };
    assert_eq!(
        argument.to_string(),
        "A wait handle is required. (Parameter 'waitHandle')"
    );
    let range = HostError::ArgumentOutOfRange { param: "fixed_period" };
    assert_eq!(
        range.to_string(),
        "Specified argument was out of the range of valid values. (Parameter 'fixed_period')"
    );
    let _: &dyn std::error::Error = &range;
}

// new
#[test]
fn host_frame_clock_is_object_safe() {
    let time = FakeTime::new(10);
    let clock: Rc<dyn HostFrameClock> = Rc::new(timer_clock(&time, Some(period())));
    let count = Rc::new(Cell::new(0));
    let counter = Rc::clone(&count);
    let id = clock.subscribe(Box::new(move |_| counter.set(counter.get() + 1)));
    assert!(clock.is_armed());
    assert!(clock.on_signaled().unwrap());
    clock.unsubscribe(id);
    assert!(!clock.is_armed());
    clock.request_frame();
    assert!(clock.is_armed());
    assert_eq!(clock.tick_count(), 1);
    assert_eq!(count.get(), 1);
    assert_eq!(clock.deadline_timestamp(), i64::MAX);
    assert_eq!(clock.ticks_until_deadline(), i64::MAX);
    assert_ne!(clock.wait_handle(), 0);
    clock.dispose();
    assert!(!clock.is_armed());
}

// ---- new: the base with a scripted policy ----

#[derive(Default)]
struct Script {
    log: Vec<String>,
    deadline: i64,
    tick_at: Option<i64>,
    fail_arming: bool,
    fail_should_tick: bool,
    disposed: bool,
    env_debug: String,
}

struct ScriptPolicy(Rc<RefCell<Script>>);

impl ClockPolicy for ScriptPolicy {
    fn wait_handle(&self) -> isize {
        77
    }
    fn deadline_timestamp(&self, armed: bool) -> i64 {
        if armed { self.0.borrow().deadline } else { i64::MAX }
    }
    fn should_tick(&mut self, env: &ClockEnv<'_>, now: i64, signaled: bool) -> Result<Option<i64>, HostError> {
        let mut script = self.0.borrow_mut();
        script.log.push(format!(
            "should_tick now={now} signaled={signaled} timing={:?}",
            env.current_timing()
        ));
        script.env_debug = format!("{env:?}");
        if script.fail_should_tick {
            return Err(HostError::InvalidOperation {
                message: "should_tick failed",
            });
        }
        Ok(script.tick_at)
    }
    fn on_armed_changed(&mut self, _env: &ClockEnv<'_>, armed: bool, now: i64) -> Result<(), HostError> {
        let mut script = self.0.borrow_mut();
        script.log.push(format!("armed={armed} now={now}"));
        if script.fail_arming && armed {
            return Err(HostError::Gfx(GfxError::new("SetWaitableTimer", -2_147_024_809)));
        }
        Ok(())
    }
    fn on_ticked(&mut self, _env: &ClockEnv<'_>, frame_timestamp: i64, now: i64) -> Result<(), HostError> {
        self.0
            .borrow_mut()
            .log
            .push(format!("ticked frame={frame_timestamp} now={now}"));
        Ok(())
    }
    fn dispose_core(&mut self) {
        let mut script = self.0.borrow_mut();
        script.log.push("dispose_core".to_owned());
        script.disposed = true;
    }
}

fn scripted(time: &FakeTime, timing: Option<DisplayTiming>) -> (FrameClockBase<ScriptPolicy>, Rc<RefCell<Script>>) {
    let script = Rc::new(RefCell::new(Script {
        deadline: i64::MAX,
        ..Script::default()
    }));
    let display = timing.and_then(fixed_display);
    let clock = FrameClockBase::new(ScriptPolicy(Rc::clone(&script)), display, 5, Some(time.read_fn()));
    (clock, script)
}

// new
#[test]
fn tick_algorithm_calls_the_policy_in_c_sharp_order() {
    let time = FakeTime::new(100);
    let timing = DisplayTiming {
        last_vblank: 1,
        refresh_period: 2,
    };
    let (clock, script) = scripted(&time, Some(timing));
    script.borrow_mut().tick_at = Some(150);

    // not armed: the policy and the clock are not consulted at all
    assert!(!clock.on_signaled().unwrap());
    assert!(script.borrow().log.is_empty());

    let ticks = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&ticks);
    let log = Rc::clone(&script);
    let handler_time = time.clone();
    clock.subscribe(Box::new(move |tick| {
        sink.borrow_mut().push(*tick);
        log.borrow_mut().log.push("handler".to_owned());
        handler_time.set(400); // the clock read for `on_ticked` happens after the handlers
    }));
    time.set(300);
    assert!(clock.on_signaled().unwrap());

    let expected = [
        "armed=true now=100".to_owned(),
        format!("should_tick now=300 signaled=true timing={:?}", Some(timing)),
        "handler".to_owned(),
        "ticked frame=150 now=400".to_owned(),
    ];
    assert_eq!(script.borrow().log, expected);
    let ticks = ticks.borrow();
    assert_eq!(ticks.len(), 1);
    assert_eq!(ticks[0].timestamp, 150);
    assert_eq!(ticks[0].timing, Some(timing));
    // epoch = the clock reading at construction (100)
    assert_eq!(ticks[0].rendering_time_ms, rendering_time_ms(150, 100, qpc_frequency()));
    assert!(ticks[0].rendering_time_ms > 0.0);
}

// new
#[test]
fn clock_env_debug_names_the_window() {
    let time = FakeTime::new(0);
    let (clock, script) = scripted(&time, None);
    clock.request_frame();
    script.borrow_mut().tick_at = Some(0);
    assert!(clock.on_signaled().unwrap());
    let text = script.borrow().env_debug.clone();
    assert!(text.contains("ClockEnv") && text.contains("window: 5"), "{text}");
}

// new
#[test]
fn on_deadline_passes_signaled_false_and_none_means_no_tick() {
    let time = FakeTime::new(0);
    let (clock, script) = scripted(&time, None);
    clock.request_frame();

    assert!(!clock.on_deadline().unwrap()); // tick_at = None
    assert!(clock.is_armed());
    assert_eq!(clock.tick_count(), 0);
    assert!(
        script
            .borrow()
            .log
            .contains(&"should_tick now=0 signaled=false timing=None".to_owned())
    );
    assert!(!script.borrow().log.iter().any(|line| line.starts_with("ticked")));

    script.borrow_mut().tick_at = Some(0);
    assert!(clock.on_deadline().unwrap());
    assert!(!clock.is_armed()); // the request was consumed
    assert!(!script.borrow().log.iter().any(|line| line.starts_with("ticked"))); // not armed any more: no on_ticked
}

// new
#[test]
fn policy_errors_propagate_out_of_the_tick() {
    let time = FakeTime::new(0);
    let (clock, script) = scripted(&time, None);
    clock.request_frame();
    script.borrow_mut().fail_should_tick = true;
    script.borrow_mut().tick_at = Some(0);

    assert_eq!(
        clock.on_signaled(),
        Err(HostError::InvalidOperation {
            message: "should_tick failed"
        })
    );
    assert_eq!(clock.tick_count(), 0);
    assert!(clock.is_armed());
}

// new: D-W8B3H-2
#[test]
fn arming_failure_is_an_error_from_try_and_a_panic_from_the_trait_methods() {
    let time = FakeTime::new(0);
    let (clock, script) = scripted(&time, None);
    script.borrow_mut().fail_arming = true;

    let error = clock.try_subscribe(Box::new(|_| {})).unwrap_err();
    assert_eq!(error, HostError::Gfx(GfxError::new("SetWaitableTimer", -2_147_024_809)));
    assert!(clock.try_request_frame().is_ok()); // already armed (state was stored before the failure): no second call

    let (clock, script) = scripted(&time, None);
    script.borrow_mut().fail_arming = true;
    let payload = catch_unwind(AssertUnwindSafe(|| clock.subscribe(Box::new(|_| {})))).unwrap_err();
    let message = payload.downcast_ref::<String>().unwrap();
    assert_eq!(message, "SetWaitableTimer failed with HRESULT 0x80070057");

    let (clock, script) = scripted(&time, None);
    script.borrow_mut().fail_arming = true;
    assert!(catch_unwind(AssertUnwindSafe(|| clock.request_frame())).is_err());
}

// new: the policy's view of armed changes, only on changes
#[test]
fn on_armed_changed_is_called_only_when_the_state_changes() {
    let time = FakeTime::new(7);
    let (clock, script) = scripted(&time, None);
    let a = clock.subscribe(Box::new(|_| {}));
    let b = clock.subscribe(Box::new(|_| {}));
    clock.request_frame();
    clock.unsubscribe(a);
    clock.unsubscribe(b); // still requested: stays armed
    assert!(clock.is_armed());
    time.set(9);
    clock.dispose();
    assert_eq!(
        script.borrow().log,
        vec!["armed=true now=7", "armed=false now=9", "dispose_core"]
    );
    clock.dispose();
    assert_eq!(script.borrow().log.len(), 3);
}

// new
#[test]
fn deadline_and_ticks_until_deadline_follow_the_policy() {
    let time = FakeTime::new(1_000);
    let (clock, script) = scripted(&time, None);
    script.borrow_mut().deadline = 1_250;

    // not armed: the policy gets `armed = false` and answers MAX
    assert_eq!(clock.deadline_timestamp(), i64::MAX);
    assert_eq!(clock.ticks_until_deadline(), i64::MAX);

    clock.request_frame();
    assert_eq!(clock.deadline_timestamp(), 1_250);
    assert_eq!(clock.ticks_until_deadline(), 250);
    time.set(1_300);
    assert_eq!(clock.ticks_until_deadline(), -50);
    script.borrow_mut().deadline = i64::MIN;
    assert_eq!(clock.ticks_until_deadline(), i64::MIN); // saturates instead of wrapping
    assert_eq!(clock.wait_handle(), 77);
    clock.dispose();
    assert_eq!(clock.wait_handle(), 0);
    assert!(script.borrow().disposed);
}

// new
#[test]
fn default_policy_members_are_no_ops() {
    struct Bare;
    impl ClockPolicy for Bare {
        fn wait_handle(&self) -> isize {
            1
        }
        fn should_tick(&mut self, _: &ClockEnv<'_>, now: i64, _: bool) -> Result<Option<i64>, HostError> {
            Ok(Some(now))
        }
        fn on_armed_changed(&mut self, _: &ClockEnv<'_>, _: bool, _: i64) -> Result<(), HostError> {
            Ok(())
        }
    }
    let clock = FrameClockBase::new(Bare, None, 0, Some(Box::new(|| 5)));
    assert_eq!(clock.deadline_timestamp(), i64::MAX);
    clock.request_frame();
    assert!(clock.on_signaled().unwrap());
    clock.dispose();
    assert_eq!(clock.with_policy(|_| 3), 3);
    assert!(format!("{clock:?}").contains("FrameClockBase"));
}
