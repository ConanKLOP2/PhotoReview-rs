#![cfg(windows)]
//! Tests of `render::keep_alive` (unit W8-12h, test kinds F + N). C# twins: `GpuKeepAliveTests` (14 tests, fake work and fake clock) and
//! `DeviceKeepAliveNativeTests` (baseline `d92d6aae`). No sleep polling: the fakes publish events through `std::sync::mpsc` and the tests
//! wait with `recv_timeout(60 s)` (a hang bound, not a speed budget); "parked on the event" is observed with `wait_blocked`, "disabled"
//! with `wait_disabled`, "work dropped" through a `Condvar` the fake signals in `Drop`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use pr_platform_win::gfx::com::GfxError;
use pr_platform_win::gfx::extras::WaitableTimer;
use pr_platform_win::render::keep_alive::{
    DISPOSE_JOIN_TIMEOUT_MS, GpuKeepAlive, KeepAliveDevice, KeepAliveOptions, KeepAliveWork, MAX_INTERVAL_MS,
    MIN_INTERVAL_MS, OpenKeepAliveWork, THREAD_NAME, TICKS_PER_SECOND, clamp_interval_ms, monotonic_ticks,
    open_hardware_work, open_offscreen_hardware_work, ticks_from_ms,
};

/// Hang bound for every wait.
const HANG: Duration = Duration::from_secs(60);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------------------------------------------------

/// What the fake work records; shared between the keep-warm thread and the test.
struct FakeState {
    touches: AtomicI64,
    opens: AtomicI64,
    open_thread: Mutex<Option<ThreadId>>,
    touch_threads: Mutex<HashSet<ThreadId>>,
    drop_thread: Mutex<Option<ThreadId>>,
    dropped: Mutex<bool>,
    dropped_cv: Condvar,
    fail_on_touch: AtomicBool,
    panic_on_touch: AtomicBool,
    /// One message per successful touch.
    touch_tx: Mutex<Sender<()>>,
    /// `touch` announces here before it blocks on `release_rx`.
    entered_tx: Mutex<Sender<()>>,
    /// When set, `touch` blocks on it (until the test sends or drops the sender).
    release_rx: Mutex<Option<Receiver<()>>>,
}

struct Harness {
    state: Arc<FakeState>,
    touch_rx: Receiver<()>,
    entered_rx: Receiver<()>,
}

impl Harness {
    fn new() -> Self {
        Self::with_release(None)
    }

    fn with_release(release_rx: Option<Receiver<()>>) -> Self {
        let (touch_tx, touch_rx) = mpsc::channel();
        let (entered_tx, entered_rx) = mpsc::channel();
        Self {
            state: Arc::new(FakeState {
                touches: AtomicI64::new(0),
                opens: AtomicI64::new(0),
                open_thread: Mutex::new(None),
                touch_threads: Mutex::new(HashSet::new()),
                drop_thread: Mutex::new(None),
                dropped: Mutex::new(false),
                dropped_cv: Condvar::new(),
                fail_on_touch: AtomicBool::new(false),
                panic_on_touch: AtomicBool::new(false),
                touch_tx: Mutex::new(touch_tx),
                entered_tx: Mutex::new(entered_tx),
                release_rx: Mutex::new(release_rx),
            }),
            touch_rx,
            entered_rx,
        }
    }

    fn touches(&self) -> i64 {
        self.state.touches.load(Ordering::SeqCst)
    }

    /// Blocks until `count` more touch events arrived (hang bound 60 s).
    fn wait_touches(&self, count: usize) {
        for _ in 0..count {
            self.touch_rx
                .recv_timeout(HANG)
                .expect("a touch event within the hang bound");
        }
    }

    fn is_dropped(&self) -> bool {
        *lock(&self.state.dropped)
    }

    fn wait_dropped(&self) -> bool {
        let guard = lock(&self.state.dropped);
        let (guard, _) = self
            .state
            .dropped_cv
            .wait_timeout_while(guard, HANG, |dropped| !*dropped)
            .unwrap_or_else(PoisonError::into_inner);
        *guard
    }

    /// The `openWork` closure: records the opening thread and hands out the fake work.
    fn open(&self) -> OpenKeepAliveWork {
        let state = Arc::clone(&self.state);
        Box::new(move || {
            state.opens.fetch_add(1, Ordering::SeqCst);
            *lock(&state.open_thread) = Some(thread::current().id());
            Ok(Some(Box::new(FakeWork { state }) as Box<dyn KeepAliveWork>))
        })
    }

    fn keep_alive(&self, interval_ms: i32, clock: &FakeClock) -> GpuKeepAlive {
        make(interval_ms, self.open(), clock)
    }
}

struct FakeWork {
    state: Arc<FakeState>,
}

impl KeepAliveWork for FakeWork {
    fn touch(&mut self) -> Result<(), GfxError> {
        assert!(
            thread::current().name() == Some(THREAD_NAME),
            "touch must run on the keep-warm thread"
        );
        if self.state.panic_on_touch.load(Ordering::SeqCst) {
            panic!("fake touch panics");
        }
        if self.state.fail_on_touch.load(Ordering::SeqCst) {
            return Err(GfxError::new("FakeWork::touch", -2_147_467_259));
        }
        let blocker = lock(&self.state.release_rx).take();
        if let Some(rx) = blocker {
            let _ = lock(&self.state.entered_tx).send(());
            let _ = rx.recv();
        }
        lock(&self.state.touch_threads).insert(thread::current().id());
        self.state.touches.fetch_add(1, Ordering::SeqCst);
        let _ = lock(&self.state.touch_tx).send(());
        Ok(())
    }
}

impl Drop for FakeWork {
    fn drop(&mut self) {
        *lock(&self.state.drop_thread) = Some(thread::current().id());
        *lock(&self.state.dropped) = true;
        self.state.dropped_cv.notify_all();
    }
}

/// C# `FakeClock`: a settable tick counter.
#[derive(Clone)]
struct FakeClock(Arc<AtomicI64>);

impl FakeClock {
    fn new(start: i64) -> Self {
        Self(Arc::new(AtomicI64::new(start)))
    }

    fn set(&self, value: i64) {
        self.0.store(value, Ordering::SeqCst);
    }

    fn options(&self) -> KeepAliveOptions {
        let clock = Arc::clone(&self.0);
        KeepAliveOptions {
            timestamp: Some(Arc::new(move || clock.load(Ordering::SeqCst))),
            below_normal_priority: false,
        }
    }
}

fn make(interval_ms: i32, open: OpenKeepAliveWork, clock: &FakeClock) -> GpuKeepAlive {
    GpuKeepAlive::new(interval_ms, open, clock.options())
}

// ---------------------------------------------------------------------------------------------------------------------
// Pure
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn clamp_interval_ms_bounds() {
    for low in [i32::MIN, -5, 0, 1] {
        assert_eq!(clamp_interval_ms(low), 1, "{low}");
    }
    assert_eq!(clamp_interval_ms(5), 5);
    assert_eq!(clamp_interval_ms(1000), 1000);
    for high in [1001, 60_000, i32::MAX] {
        assert_eq!(clamp_interval_ms(high), 1000, "{high}");
    }
    assert_eq!((MIN_INTERVAL_MS, MAX_INTERVAL_MS), (1, 1000));
}

#[test]
fn ticks_from_ms_converts_milliseconds_to_ticks() {
    assert_eq!(ticks_from_ms(5000), 5 * TICKS_PER_SECOND);
    assert_eq!(ticks_from_ms(0), 0);
    assert_eq!(ticks_from_ms(1), TICKS_PER_SECOND / 1000);
    assert_eq!(ticks_from_ms(i64::MAX), i64::MAX);
    assert_eq!(ticks_from_ms(i64::MIN), i64::MIN);
}

#[test]
fn monotonic_ticks_never_decreases() {
    let first = monotonic_ticks();
    let second = monotonic_ticks();
    assert!(first >= 0);
    assert!(second >= first);
}

#[test]
fn keep_alive_options_default_lowers_priority_and_uses_the_real_clock() {
    let options = KeepAliveOptions::default();
    assert!(options.timestamp.is_none());
    assert!(options.below_normal_priority);
    assert!(format!("{options:?}").contains("below_normal_priority"));
}

struct FakeDevice {
    warp: bool,
    touches: Arc<AtomicI64>,
    drops: Arc<AtomicI64>,
}

impl KeepAliveDevice for FakeDevice {
    fn is_warp(&self) -> bool {
        self.warp
    }

    fn touch_gpu(&mut self) -> Result<(), GfxError> {
        self.touches.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl Drop for FakeDevice {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

// C#: GpuKeepAlive.OpenHardwareWork (WARP -> null; hardware -> HardwareWork over TouchGpu)
#[test]
fn open_hardware_work_drops_a_warp_device_and_wraps_a_hardware_one() {
    let touches = Arc::new(AtomicI64::new(0));
    let drops = Arc::new(AtomicI64::new(0));
    let device = |warp: bool| {
        let (touches, drops) = (Arc::clone(&touches), Arc::clone(&drops));
        move || Ok(Box::new(FakeDevice { warp, touches, drops }) as Box<dyn KeepAliveDevice>)
    };
    assert!(open_hardware_work(device(true)).expect("ok").is_none());
    assert_eq!(drops.load(Ordering::SeqCst), 1);

    let mut work = open_hardware_work(device(false)).expect("ok").expect("hardware work");
    work.touch().expect("touch");
    work.touch().expect("touch");
    assert_eq!(touches.load(Ordering::SeqCst), 2);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    drop(work);
    assert_eq!(drops.load(Ordering::SeqCst), 2);

    let failed = open_hardware_work(|| Err(GfxError::new("create", -1)));
    assert_eq!(failed.err(), Some(GfxError::new("create", -1)));
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported (fake work, fake clock)
// ---------------------------------------------------------------------------------------------------------------------

// C#: GpuKeepAliveTests.NothingRunsUntilTheFirstActivation
#[test]
fn nothing_runs_until_the_first_activation() {
    let harness = Harness::new();
    let keep_alive = harness.keep_alive(5, &FakeClock::new(1000));
    assert!(!keep_alive.has_thread());
    assert_eq!(harness.state.opens.load(Ordering::SeqCst), 0);
    assert!(lock(&harness.state.open_thread).is_none());
    assert_eq!(keep_alive.ticks(), 0);
    assert!(!keep_alive.is_disabled());
}

// C#: GpuKeepAliveTests.Activation_TouchesRepeatedlyOnABackgroundThread_ThenStopsAtTheDeadline
#[test]
fn activation_touches_repeatedly_on_a_background_thread_then_stops_at_the_deadline() {
    let harness = Harness::new();
    let clock = FakeClock::new(1000);
    let keep_alive = harness.keep_alive(5, &clock);
    keep_alive.activate(1100);
    harness.wait_touches(5);
    assert!(keep_alive.has_thread());
    let open_thread = lock(&harness.state.open_thread).expect("opened on a thread");
    assert_ne!(open_thread, thread::current().id());
    {
        let threads = lock(&harness.state.touch_threads);
        assert_eq!(threads.len(), 1);
        assert!(threads.contains(&open_thread));
    }
    clock.set(2000);
    assert!(keep_alive.wait_blocked(2, HANG));
    assert_eq!(harness.touches(), keep_alive.ticks());
    assert!(!harness.is_dropped());
}

// C#: GpuKeepAliveTests.ReactivationAfterTheDeadline_ResumesOnTheSameDevice
#[test]
fn reactivation_after_the_deadline_resumes_on_the_same_device() {
    let harness = Harness::new();
    let clock = FakeClock::new(1000);
    let keep_alive = harness.keep_alive(5, &clock);
    keep_alive.activate(1100);
    harness.wait_touches(3);
    clock.set(2000);
    assert!(keep_alive.wait_blocked(2, HANG));
    let before = harness.touches();
    keep_alive.activate(2100);
    // Events of the first window may still be queued; wait until the counter moved by 3.
    while harness.touches() < before + 3 {
        harness.wait_touches(1);
    }
    assert_eq!(harness.state.opens.load(Ordering::SeqCst), 1);
    assert!(!harness.is_dropped());
}

// C#: GpuKeepAliveTests.ActivationNeverShortensAnEarlierDeadline
#[test]
fn activation_never_shortens_an_earlier_deadline() {
    let harness = Harness::new();
    let keep_alive = harness.keep_alive(5, &FakeClock::new(1000));
    keep_alive.activate(1500);
    keep_alive.activate(1);
    harness.wait_touches(3);
    let before = harness.touches();
    while harness.touches() <= before + 2 {
        harness.wait_touches(1);
    }
    assert!(!keep_alive.is_disabled());
}

// C#: GpuKeepAliveTests.IntervalIsRespected_NotABusyLoop
#[test]
fn interval_is_respected_not_a_busy_loop() {
    let harness = Harness::new();
    let keep_alive = harness.keep_alive(50, &FakeClock::new(1000));
    let started = Instant::now();
    keep_alive.activate(i64::MAX);
    harness.wait_touches(6);
    // Read the touches BEFORE the elapsed time: the one-sided bound cannot fail on a slow machine.
    let touches = harness.touches();
    let elapsed_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    assert!(touches <= elapsed_ms / 50 + 2, "{touches} touches in {elapsed_ms} ms");
}

// C#: GpuKeepAliveTests.AfterTheDeadline_TheThreadSleepsInsteadOfSpinning
#[test]
fn after_the_deadline_the_thread_sleeps_instead_of_spinning() {
    let harness = Harness::new();
    let clock = FakeClock::new(1000);
    let keep_alive = harness.keep_alive(5, &clock);
    keep_alive.activate(1100);
    harness.wait_touches(3);
    clock.set(2000);
    assert!(keep_alive.wait_blocked(2, HANG));
    assert_eq!(keep_alive.idle_waits(), 2);
    assert!(keep_alive.is_blocked());
    let touches = harness.touches();
    // A stale activation: one wake-up, no touch (T1).
    keep_alive.activate(1500);
    assert!(keep_alive.wait_blocked(3, HANG));
    assert_eq!(keep_alive.idle_waits(), 3);
    assert_eq!(harness.touches(), touches);
    assert_eq!(keep_alive.ticks(), touches);
}

// C#: GpuKeepAliveTests.NonPositiveInterval_IsClampedToOneMillisecond_NotABusyLoop
#[test]
fn non_positive_interval_is_clamped_to_one_millisecond() {
    let harness = Harness::new();
    let keep_alive = harness.keep_alive(-5, &FakeClock::new(1000));
    let started = Instant::now();
    keep_alive.activate(i64::MAX);
    harness.wait_touches(40);
    let touches = harness.touches();
    let elapsed_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    assert!(touches <= elapsed_ms + 5, "{touches} touches in {elapsed_ms} ms");
}

// C#: GpuKeepAliveTests.HugeInterval_IsClampedToOneSecond_SoTheDeviceStillGetsTouched
#[test]
fn huge_interval_is_clamped_to_one_second_so_the_device_still_gets_touched() {
    let harness = Harness::new();
    let keep_alive = harness.keep_alive(60_000, &FakeClock::new(1000));
    keep_alive.activate(i64::MAX);
    harness.wait_touches(2);
    assert!(keep_alive.ticks() >= 1);
}

// C#: GpuKeepAliveTests.NoGpu_DisablesItselfWithoutThrowing
#[test]
fn no_gpu_disables_itself_without_panicking() {
    let clock = FakeClock::new(1000);
    let keep_alive = make(5, Box::new(|| Ok(None)), &clock);
    keep_alive.activate(i64::MAX);
    assert!(keep_alive.wait_disabled(HANG));
    assert_eq!(keep_alive.ticks(), 0);
    keep_alive.activate(i64::MAX);
    assert_eq!(keep_alive.ticks(), 0);
    assert!(keep_alive.is_disabled());
}

// C#: GpuKeepAliveTests.FactoryThatThrows_DisablesItselfWithoutThrowing
#[test]
fn factory_that_fails_disables_itself() {
    let clock = FakeClock::new(1000);
    let keep_alive = make(5, Box::new(|| Err(GfxError::new("open", -1))), &clock);
    keep_alive.activate(i64::MAX);
    assert!(keep_alive.wait_disabled(HANG));
    assert_eq!(keep_alive.ticks(), 0);
}

// C#: GpuKeepAliveTests.FactoryThatThrows_DisablesItselfWithoutThrowing (the exception flavour)
#[test]
fn factory_that_panics_disables_itself() {
    let clock = FakeClock::new(1000);
    let keep_alive = make(5, Box::new(|| panic!("fake open panics")), &clock);
    keep_alive.activate(i64::MAX);
    assert!(keep_alive.wait_disabled(HANG));
    assert_eq!(keep_alive.ticks(), 0);
}

// C#: GpuKeepAliveTests.TouchFailure_DisablesAndDisposesTheDevice_OnTheBackgroundThread
#[test]
fn touch_failure_disables_and_drops_the_device_on_the_background_thread() {
    let harness = Harness::new();
    harness.state.fail_on_touch.store(true, Ordering::SeqCst);
    let keep_alive = harness.keep_alive(5, &FakeClock::new(1000));
    keep_alive.activate(i64::MAX);
    assert!(keep_alive.wait_disabled(HANG));
    assert!(harness.wait_dropped());
    assert_eq!(harness.touches(), 0);
    assert_eq!(keep_alive.ticks(), 0);
    let open_thread = *lock(&harness.state.open_thread);
    let drop_thread = *lock(&harness.state.drop_thread);
    assert!(open_thread.is_some());
    assert_eq!(drop_thread, open_thread);
    assert_ne!(drop_thread, Some(thread::current().id()));
}

// C#: GpuKeepAliveTests.Dispose_StopsTheThread_DisposesTheDevice_AndLaterActivationsAreNoOps
#[test]
fn dispose_stops_the_thread_disposes_the_device_and_later_activations_are_no_ops() {
    let harness = Harness::new();
    let keep_alive = harness.keep_alive(5, &FakeClock::new(1000));
    keep_alive.activate(i64::MAX);
    harness.wait_touches(2);
    keep_alive.dispose();
    // `done` is set after the work was dropped, so no waiting is needed here.
    assert!(harness.is_dropped());
    let touches = harness.touches();
    keep_alive.activate(i64::MAX);
    assert_eq!(harness.touches(), touches);
    assert!(keep_alive.is_disabled());
    keep_alive.dispose();
}

// C#: GpuKeepAliveTests.Dispose_BeforeAnyActivation_DoesNothing
#[test]
fn dispose_before_any_activation_does_nothing() {
    let harness = Harness::new();
    let keep_alive = harness.keep_alive(5, &FakeClock::new(1000));
    keep_alive.dispose();
    keep_alive.activate(i64::MAX);
    assert!(!keep_alive.has_thread());
    assert_eq!(harness.state.opens.load(Ordering::SeqCst), 0);
    assert!(keep_alive.is_disabled());
}

// ---------------------------------------------------------------------------------------------------------------------
// New
// ---------------------------------------------------------------------------------------------------------------------

// T5, T6
#[test]
fn dispose_returns_within_the_join_bound_even_if_the_work_blocks() {
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let harness = Harness::with_release(Some(release_rx));
    let keep_alive = harness.keep_alive(5, &FakeClock::new(1000));
    keep_alive.activate(i64::MAX);
    harness
        .entered_rx
        .recv_timeout(HANG)
        .expect("touch entered and is now blocked");
    let started = Instant::now();
    keep_alive.dispose();
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(u64::from(DISPOSE_JOIN_TIMEOUT_MS) - 500),
        "{waited:?}"
    );
    assert!(waited < Duration::from_secs(10), "{waited:?}");
    assert!(!harness.is_dropped(), "the blocked thread is detached, not finished");
    // Releasing the touch lets the detached thread finish: it sees `stop` and drops the work without a panic.
    release_tx.send(()).expect("release");
    assert!(harness.wait_dropped());
}

// T6
#[test]
fn a_panicking_work_disables_instead_of_unwinding() {
    let harness = Harness::new();
    harness.state.panic_on_touch.store(true, Ordering::SeqCst);
    let keep_alive = harness.keep_alive(5, &FakeClock::new(1000));
    keep_alive.activate(i64::MAX);
    assert!(keep_alive.wait_disabled(HANG));
    assert!(harness.wait_dropped());
    assert_eq!(keep_alive.ticks(), 0);
}

#[test]
fn drop_disposes() {
    let harness = Harness::new();
    let keep_alive = harness.keep_alive(5, &FakeClock::new(1000));
    keep_alive.activate(i64::MAX);
    harness.wait_touches(2);
    drop(keep_alive);
    assert!(harness.is_dropped());
}

#[test]
fn concurrent_activate_and_dispose_do_not_deadlock() {
    let harness = Harness::new();
    let keep_alive = Arc::new(harness.keep_alive(1, &FakeClock::new(1000)));
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let keep_alive = Arc::clone(&keep_alive);
            let done_tx = done_tx.clone();
            thread::spawn(move || {
                while !keep_alive.is_disabled() {
                    keep_alive.activate(i64::MAX);
                }
                let _ = done_tx.send(());
            })
        })
        .collect();
    harness.wait_touches(1);
    keep_alive.dispose();
    for _ in 0..workers.len() {
        done_rx
            .recv_timeout(HANG)
            .expect("an activating thread ended after dispose");
    }
    for worker in workers {
        worker.join().expect("worker joined");
    }
    assert!(harness.is_dropped());
}

// ---------------------------------------------------------------------------------------------------------------------
// Native
// ---------------------------------------------------------------------------------------------------------------------

// C#: DeviceKeepAliveNativeTests.OpenHardwareWork_IsNullWithoutARealGpu_OtherwiseARunnableDevice
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn open_hardware_work_is_none_without_a_real_gpu_otherwise_a_runnable_device() {
    match open_offscreen_hardware_work() {
        Ok(None) => println!("WARP only: no hardware work"),
        Ok(Some(mut work)) => {
            work.touch().expect("first touch");
            work.touch().expect("second touch");
        }
        Err(error) => panic!("device creation failed: {error}"),
    }
}

/// Reports what the real work does without polling.
enum Event {
    Touched,
    NoGpu,
}

struct Reporting {
    inner: Box<dyn KeepAliveWork>,
    tx: Sender<Event>,
}

impl KeepAliveWork for Reporting {
    fn touch(&mut self) -> Result<(), GfxError> {
        self.inner.touch()?;
        let _ = self.tx.send(Event::Touched);
        Ok(())
    }
}

// C#: DeviceKeepAliveNativeTests.RealKeepAlive_OnThisMachine_EitherRunsOrDisablesItself_AndDisposesCleanly
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn real_keep_alive_on_this_machine_either_runs_or_disables_itself() {
    let (tx, rx) = mpsc::channel::<Event>();
    let open: OpenKeepAliveWork = Box::new(move || match open_offscreen_hardware_work() {
        Ok(Some(inner)) => Ok(Some(Box::new(Reporting { inner, tx }) as Box<dyn KeepAliveWork>)),
        Ok(None) => {
            let _ = tx.send(Event::NoGpu);
            Ok(None)
        }
        Err(error) => Err(error),
    });
    let keep_alive = GpuKeepAlive::new(10, open, KeepAliveOptions::default());
    keep_alive.activate(monotonic_ticks() + ticks_from_ms(200));
    match rx.recv_timeout(Duration::from_secs(20)).expect("an event within 20 s") {
        Event::NoGpu => assert!(keep_alive.wait_disabled(Duration::from_secs(20))),
        Event::Touched => {
            let second = rx.recv_timeout(Duration::from_secs(20)).expect("a second touch");
            assert!(matches!(second, Event::Touched));
            assert!(!keep_alive.is_disabled());
        }
    }
    keep_alive.dispose();
    assert!(keep_alive.is_disabled());
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn waitable_timer_interval_sleep_is_used_when_available() {
    let timer = WaitableTimer::new().expect("a waitable timer");
    println!("high resolution timer: {}", timer.is_high_resolution());
    timer.set_relative_ms(1).expect("arm");
    assert!(timer.wait(1100, false).is_signaled());
}
