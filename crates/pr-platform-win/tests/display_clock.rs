#[path = "../src/display_clock.rs"]
mod display_clock;
#[path = "../src/vblank_driver.rs"]
mod vblank_driver;

use std::{
    collections::HashMap,
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

use display_clock::{IDLE_STOP_MS, RETRY_FAILED_AFTER_MS, WindowsDisplayClock};
use pr_core::abstractions::{DisplayClock, DisplayTiming, Log};
use vblank_driver::{IVBlankDriver, NativeImportError};

const WINDOW_A: isize = 0xA0;
const WINDOW_B: isize = 0xB0;
const MONITOR_A: isize = 0xA1;
const MONITOR_B: isize = 0xB1;
const DWM_SENTINEL: DisplayTiming = DisplayTiming {
    last_vblank: 111,
    refresh_period: 222,
};
const SIGNAL_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
enum DriverEvent {
    Open(isize),
    Waiting,
    Closed(u32),
}

enum WaitOutcome {
    Status(i32),
    ImportError(NativeImportError),
    Panic(&'static str),
}

struct FakeDriver {
    window_monitors: Mutex<HashMap<isize, isize>>,
    opened_monitors: Mutex<Vec<isize>>,
    closed_adapters: Mutex<Vec<u32>>,
    event_log: Mutex<Vec<DriverEvent>>,
    event_tx: Sender<DriverEvent>,
    event_rx: Mutex<Receiver<DriverEvent>>,
    vblank_tx: Sender<()>,
    vblank_rx: Mutex<Receiver<()>>,
    next_outcome: Mutex<Option<WaitOutcome>>,
    open_succeeds: AtomicBool,
    now: AtomicI64,
    stopping: AtomicBool,
}

impl FakeDriver {
    fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let (vblank_tx, vblank_rx) = mpsc::channel();
        Self {
            window_monitors: Mutex::new(HashMap::from([(WINDOW_A, MONITOR_A), (WINDOW_B, MONITOR_B)])),
            opened_monitors: Mutex::new(Vec::new()),
            closed_adapters: Mutex::new(Vec::new()),
            event_log: Mutex::new(Vec::new()),
            event_tx,
            event_rx: Mutex::new(event_rx),
            vblank_tx,
            vblank_rx: Mutex::new(vblank_rx),
            next_outcome: Mutex::new(None),
            open_succeeds: AtomicBool::new(true),
            now: AtomicI64::new(qpc_frequency() * 100),
            stopping: AtomicBool::new(false),
        }
    }

    fn now(&self) -> i64 {
        self.now.load(Ordering::Acquire)
    }

    fn advance(&self, ticks: i64) {
        self.now.fetch_add(ticks, Ordering::AcqRel);
    }

    fn set_wait_outcome(&self, outcome: WaitOutcome) {
        *lock(&self.next_outcome) = Some(outcome);
    }

    fn release_vblank(&self) {
        self.vblank_tx.send(()).expect("fake worker still owns the receiver");
    }

    fn release_and_wait_for_next(&self) {
        self.release_vblank();
        self.await_event(|event| *event == DriverEvent::Waiting, "next vblank wait");
    }

    fn await_event(&self, wanted: impl Fn(&DriverEvent) -> bool, description: &str) -> DriverEvent {
        let start = Instant::now();
        loop {
            let remaining = SIGNAL_TIMEOUT.saturating_sub(start.elapsed());
            assert!(!remaining.is_zero(), "timed out waiting for {description}");
            let event = lock(&self.event_rx)
                .recv_timeout(remaining)
                .unwrap_or_else(|_| panic!("timed out waiting for {description}"));
            if wanted(&event) {
                return event;
            }
        }
    }

    fn opened(&self) -> Vec<isize> {
        lock(&self.opened_monitors).clone()
    }

    fn closed(&self) -> Vec<u32> {
        lock(&self.closed_adapters).clone()
    }

    fn events(&self) -> Vec<DriverEvent> {
        lock(&self.event_log).clone()
    }

    fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        for _ in 0..1024 {
            if self.vblank_tx.send(()).is_err() {
                break;
            }
        }
    }

    fn emit(&self, event: DriverEvent) {
        lock(&self.event_log).push(event.clone());
        let _ = self.event_tx.send(event);
    }
}

impl IVBlankDriver for FakeDriver {
    fn monitor_from_window(&self, window: isize) -> isize {
        lock(&self.window_monitors).get(&window).copied().unwrap_or(0)
    }

    fn try_open(&self, monitor: isize) -> Result<Option<(u32, u32)>, NativeImportError> {
        lock(&self.opened_monitors).push(monitor);
        self.emit(DriverEvent::Open(monitor));
        if self.open_succeeds.load(Ordering::Acquire) {
            Ok(Some((monitor as u32 & 0xFFFF, 7)))
        } else {
            Ok(None)
        }
    }

    fn wait_for_vblank(&self, _adapter: u32, _source: u32) -> Result<i32, NativeImportError> {
        self.emit(DriverEvent::Waiting);
        lock(&self.vblank_rx)
            .recv_timeout(Duration::from_secs(30))
            .expect("test teardown releases the fake vblank wait");
        if self.stopping.load(Ordering::Acquire) {
            return Ok(-1);
        }
        match lock(&self.next_outcome).take().unwrap_or(WaitOutcome::Status(0)) {
            WaitOutcome::Status(status) => {
                self.advance(qpc_frequency() / 60);
                Ok(status)
            }
            WaitOutcome::ImportError(error) => Err(error),
            WaitOutcome::Panic(message) => panic!("{message}"),
        }
    }

    fn close(&self, adapter: u32) {
        lock(&self.closed_adapters).push(adapter);
        self.emit(DriverEvent::Closed(adapter));
    }

    fn dwm_timing(&self) -> Option<DisplayTiming> {
        Some(DWM_SENTINEL)
    }
}

#[derive(Default)]
struct FakeLog {
    messages: Mutex<Vec<String>>,
    warn_tx: Mutex<Option<Sender<()>>>,
    warn_rx: Mutex<Option<Receiver<()>>>,
    error_tx: Mutex<Option<Sender<()>>>,
    error_rx: Mutex<Option<Receiver<()>>>,
}

impl FakeLog {
    fn new() -> Self {
        let (warn_tx, warn_rx) = mpsc::channel();
        let (error_tx, error_rx) = mpsc::channel();
        Self {
            messages: Mutex::new(Vec::new()),
            warn_tx: Mutex::new(Some(warn_tx)),
            warn_rx: Mutex::new(Some(warn_rx)),
            error_tx: Mutex::new(Some(error_tx)),
            error_rx: Mutex::new(Some(error_rx)),
        }
    }

    fn wait_warn(&self) {
        lock(&self.warn_rx)
            .as_ref()
            .expect("warning receiver present")
            .recv_timeout(SIGNAL_TIMEOUT)
            .expect("warning should be logged");
    }

    fn wait_error(&self) {
        lock(&self.error_rx)
            .as_ref()
            .expect("error receiver present")
            .recv_timeout(SIGNAL_TIMEOUT)
            .expect("error should be logged");
    }

    fn messages(&self) -> Vec<String> {
        lock(&self.messages).clone()
    }
}

impl Log for FakeLog {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, message: &str) {
        lock(&self.messages).push(format!("INFO: {message}"));
    }

    fn warn(&self, message: &str) {
        lock(&self.messages).push(format!("WARN: {message}"));
        let _ = lock(&self.warn_tx).as_ref().expect("warning sender present").send(());
    }

    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        lock(&self.messages).push(format!(
            "ERROR: {message} {}",
            error.map(ToString::to_string).unwrap_or_default()
        ));
        let _ = lock(&self.error_tx).as_ref().expect("error sender present").send(());
    }
}

struct Fixture {
    clock: WindowsDisplayClock,
    driver: Arc<FakeDriver>,
    log: Arc<FakeLog>,
}

impl Fixture {
    fn new() -> Self {
        let driver = Arc::new(FakeDriver::new());
        let log = Arc::new(FakeLog::new());
        let clock_driver: Arc<dyn IVBlankDriver> = driver.clone();
        let timestamp_driver = Arc::clone(&driver);
        let timestamp: Arc<dyn Fn() -> i64 + Send + Sync> = Arc::new(move || timestamp_driver.now());
        let clock = WindowsDisplayClock::with_driver(Some(log.clone()), Some(clock_driver), Some(timestamp))
            .expect("fake dependencies are provided");
        Self { clock, driver, log }
    }

    fn warm_up(&self, window: isize) {
        assert_eq!(self.clock.get_timing(window), None);
        self.driver
            .await_event(|event| matches!(event, DriverEvent::Open(_)), "adapter open");
        self.driver
            .await_event(|event| *event == DriverEvent::Waiting, "first vblank wait");
        for _ in 0..4 {
            self.driver.release_and_wait_for_next();
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.clock.request_stop();
        self.driver.stop();
        self.clock.shutdown_and_join();
    }
}

#[test]
fn constructor_null_driver_or_clock_throws() {
    // C#: WindowsDisplayClockTests.Constructor_NullDriverOrClock_Throws.
    let driver = Arc::new(FakeDriver::new());
    let trait_driver: Arc<dyn IVBlankDriver> = driver.clone();
    let timestamp_driver = Arc::clone(&driver);
    let timestamp: Arc<dyn Fn() -> i64 + Send + Sync> = Arc::new(move || timestamp_driver.now());
    assert!(WindowsDisplayClock::with_driver(None, None, Some(timestamp.clone())).is_err());
    assert!(WindowsDisplayClock::with_driver(None, Some(trait_driver), None).is_err());
}

#[test]
fn get_timing_no_window_returns_dwm_without_starting_a_thread() {
    // C#: WindowsDisplayClockTests.GetTiming_NoWindow_ReturnsDwmTimingWithoutStartingAThread.
    let fixture = Fixture::new();
    assert_eq!(fixture.clock.get_timing(0), Some(DWM_SENTINEL));
    assert!(fixture.driver.opened().is_empty());
}

#[test]
fn get_timing_window_without_monitor_returns_dwm_timing() {
    // C#: WindowsDisplayClockTests.GetTiming_WindowWithoutMonitor_ReturnsDwmTiming.
    let fixture = Fixture::new();
    assert_eq!(fixture.clock.get_timing(0xDEAD), Some(DWM_SENTINEL));
    assert!(fixture.driver.opened().is_empty());
}

#[test]
fn get_timing_after_enough_vblanks_reports_the_measured_grid() {
    // C#: WindowsDisplayClockTests.GetTiming_AfterEnoughVBlanks_ReportsTheMeasuredGrid.
    let fixture = Fixture::new();
    fixture.warm_up(WINDOW_A);
    let timing = fixture
        .clock
        .get_timing(WINDOW_A)
        .expect("estimate after minimum samples");
    assert_eq!(timing.refresh_period, qpc_frequency() / 60);
    assert_eq!(timing.last_vblank, fixture.driver.now());
    assert_eq!(fixture.driver.opened(), [MONITOR_A]);
}

#[test]
fn get_timing_fewer_than_min_samples_stays_null() {
    // C#: WindowsDisplayClockTests.GetTiming_FewerThanMinSamples_StaysNull.
    let fixture = Fixture::new();
    assert_eq!(fixture.clock.get_timing(WINDOW_A), None);
    fixture
        .driver
        .await_event(|event| *event == DriverEvent::Waiting, "initial wait");
    for _ in 0..3 {
        fixture.driver.release_and_wait_for_next();
    }
    assert_eq!(fixture.clock.get_timing(WINDOW_A), None);
}

#[test]
fn get_timing_second_calls_reuse_the_running_thread() {
    // C#: WindowsDisplayClockTests.GetTiming_SecondCallsReuseTheRunningThread.
    let fixture = Fixture::new();
    fixture.warm_up(WINDOW_A);
    let _ = fixture.clock.get_timing(WINDOW_A);
    let _ = fixture.clock.get_timing(WINDOW_A);
    assert_eq!(fixture.driver.opened(), [MONITOR_A]);
}

#[test]
fn get_timing_window_moves_to_another_monitor_reopens_and_remeasures() {
    // C#: WindowsDisplayClockTests.GetTiming_WindowMovesToAnotherMonitor_ReopensAndRemeasures.
    let fixture = Fixture::new();
    fixture.warm_up(WINDOW_A);
    assert!(fixture.clock.get_timing(WINDOW_A).is_some());

    assert_eq!(fixture.clock.get_timing(WINDOW_B), None);
    fixture.driver.release_and_wait_for_next();
    assert_eq!(fixture.driver.opened(), [MONITOR_A, MONITOR_B]);
    assert_eq!(fixture.driver.closed(), [MONITOR_A as u32]);
    assert_eq!(fixture.clock.get_timing(WINDOW_B), None);
    let events = fixture.driver.events();
    let close_a = events
        .iter()
        .position(|event| *event == DriverEvent::Closed(MONITOR_A as u32))
        .unwrap();
    let open_b = events
        .iter()
        .position(|event| *event == DriverEvent::Open(MONITOR_B))
        .unwrap();
    assert!(close_a < open_b, "adapter A must close before adapter B opens");

    fixture.driver.release_and_wait_for_next();
    assert_eq!(
        fixture.clock.get_timing(WINDOW_B),
        None,
        "monitor B needs its own minimum samples"
    );
    for _ in 0..3 {
        fixture.driver.release_and_wait_for_next();
    }
    let timing = fixture
        .clock
        .get_timing(WINDOW_B)
        .expect("fresh B estimate after minimum samples");
    assert_eq!(timing.refresh_period, qpc_frequency() / 60);
}

#[test]
fn open_failure_falls_back_to_dwm_for_that_monitor_and_retries_after_the_window() {
    // C#: WindowsDisplayClockTests.OpenFailure_FallsBackToDwmForThatMonitorAndRetriesAfterTheWindow.
    let fixture = Fixture::new();
    fixture.driver.open_succeeds.store(false, Ordering::Release);
    assert_eq!(fixture.clock.get_timing(WINDOW_A), None);
    fixture
        .driver
        .await_event(|event| *event == DriverEvent::Open(MONITOR_A), "failed adapter open");
    fixture.log.wait_warn();
    fixture.clock.await_failed_monitor(MONITOR_A);
    assert!(
        fixture
            .log
            .messages()
            .iter()
            .any(|message| { message.contains("opening the adapter failed") && message.contains("0xA1") })
    );
    assert_eq!(fixture.clock.get_timing(WINDOW_A), Some(DWM_SENTINEL));
    assert_eq!(fixture.driver.opened(), [MONITOR_A]);

    let frequency = qpc_frequency();
    fixture
        .driver
        .advance(frequency * RETRY_FAILED_AFTER_MS as i64 / 1000 - 1);
    assert_eq!(fixture.clock.get_timing(WINDOW_A), Some(DWM_SENTINEL));
    assert_eq!(fixture.driver.opened(), [MONITOR_A]);
    fixture.driver.advance(1); // C# retries when elapsed milliseconds are no longer strictly less than 8000.
    fixture.driver.open_succeeds.store(true, Ordering::Release);
    assert_eq!(fixture.clock.get_timing(WINDOW_A), None);
    fixture
        .driver
        .await_event(|event| *event == DriverEvent::Open(MONITOR_A), "retry adapter open");
    fixture
        .driver
        .await_event(|event| *event == DriverEvent::Waiting, "retry vblank wait");
    assert_eq!(fixture.driver.opened(), [MONITOR_A, MONITOR_A]);
}

#[test]
fn open_failure_other_monitor_is_not_affected() {
    // C#: WindowsDisplayClockTests.OpenFailure_OtherMonitorIsNotAffected.
    let fixture = Fixture::new();
    fixture.driver.open_succeeds.store(false, Ordering::Release);
    assert_eq!(fixture.clock.get_timing(WINDOW_A), None);
    fixture.log.wait_warn();
    fixture.clock.await_failed_monitor(MONITOR_A);
    fixture.driver.open_succeeds.store(true, Ordering::Release);
    assert_eq!(fixture.clock.get_timing(WINDOW_B), None);
    fixture
        .driver
        .await_event(|event| *event == DriverEvent::Open(MONITOR_B), "monitor B open");
    fixture
        .driver
        .await_event(|event| *event == DriverEvent::Waiting, "monitor B wait");
    assert_eq!(fixture.driver.opened(), [MONITOR_A, MONITOR_B]);
}

#[test]
fn wait_failure_closes_the_adapter_and_falls_back_to_dwm() {
    // C#: WindowsDisplayClockTests.WaitFailure_ClosesTheAdapterAndFallsBackToDwm.
    let fixture = Fixture::new();
    fixture.warm_up(WINDOW_A);
    fixture
        .driver
        .set_wait_outcome(WaitOutcome::Status(0xC000_0001_u32 as i32));
    fixture.driver.release_vblank();
    fixture.log.wait_warn();
    assert!(
        fixture
            .log
            .messages()
            .iter()
            .any(|message| { message.contains("D3DKMTWaitForVerticalBlankEvent returned 0xC0000001") })
    );
    fixture.driver.await_event(
        |event| *event == DriverEvent::Closed(MONITOR_A as u32),
        "failed adapter close",
    );
    assert_eq!(fixture.clock.get_timing(WINDOW_A), Some(DWM_SENTINEL));
}

#[test]
fn missing_native_library_falls_back_to_dwm() {
    // C#: WindowsDisplayClockTests.MissingNativeLibrary_FallsBackToDwm.
    for missing in [NativeImportError::DllNotFound, NativeImportError::EntryPointNotFound] {
        let fixture = Fixture::new();
        assert_eq!(fixture.clock.get_timing(WINDOW_A), None);
        fixture
            .driver
            .await_event(|event| *event == DriverEvent::Waiting, "initial native wait");
        fixture.driver.set_wait_outcome(WaitOutcome::ImportError(missing));
        fixture.driver.release_vblank();
        fixture.log.wait_warn();
        fixture.clock.await_failed_monitor(MONITOR_A);
        assert!(
            fixture
                .log
                .messages()
                .iter()
                .any(|message| message.contains(missing.type_name()))
        );
        fixture
            .driver
            .await_event(|event| *event == DriverEvent::Closed(MONITOR_A as u32), "adapter close");
        assert_eq!(fixture.clock.get_timing(WINDOW_A), Some(DWM_SENTINEL));
        assert!(
            !fixture
                .log
                .messages()
                .iter()
                .any(|message| message.starts_with("ERROR:"))
        );
    }
}

#[test]
fn unexpected_failure_is_logged_and_falls_back_to_dwm() {
    // C#: WindowsDisplayClockTests.UnexpectedFailure_IsLoggedAndFallsBackToDwm.
    let fixture = Fixture::new();
    assert_eq!(fixture.clock.get_timing(WINDOW_A), None);
    fixture
        .driver
        .await_event(|event| *event == DriverEvent::Waiting, "initial wait");
    fixture.driver.set_wait_outcome(WaitOutcome::Panic("boom"));
    fixture.driver.release_vblank();
    fixture.log.wait_error();
    fixture.log.wait_warn();
    fixture.clock.await_failed_monitor(MONITOR_A);
    assert!(
        fixture
            .log
            .messages()
            .iter()
            .any(|message| message.contains("thread failed with boom"))
    );
    assert_eq!(fixture.clock.get_timing(WINDOW_A), Some(DWM_SENTINEL));
}

#[test]
fn idle_thread_exits_drops_its_stale_estimate_and_restarts_on_the_next_call() {
    // C#: WindowsDisplayClockTests.IdleThread_ExitsDropsItsStaleEstimateAndRestartsOnTheNextCall.
    let fixture = Fixture::new();
    fixture.warm_up(WINDOW_A);
    assert!(fixture.clock.get_timing(WINDOW_A).is_some());

    let idle_ticks = qpc_frequency() * (IDLE_STOP_MS as i64 + 1) / 1000;
    fixture.driver.advance(idle_ticks);
    fixture.driver.release_vblank();
    fixture.driver.await_event(
        |event| *event == DriverEvent::Closed(MONITOR_A as u32),
        "idle adapter close",
    );

    assert_eq!(fixture.clock.get_timing(WINDOW_A), None);
    fixture
        .driver
        .await_event(|event| *event == DriverEvent::Open(MONITOR_A), "fresh adapter open");
    fixture
        .driver
        .await_event(|event| *event == DriverEvent::Waiting, "fresh worker wait");
    assert_eq!(fixture.driver.opened(), [MONITOR_A, MONITOR_A]);
}

#[test]
fn elapsed_equal_to_idle_stop_boundary_does_not_stop_worker() {
    // C#: WindowsDisplayClockTests.IdleThread_ExitsDropsItsStaleEstimateAndRestartsOnTheNextCall (strict boundary).
    let fixture = Fixture::new();
    fixture.clock.get_timing(WINDOW_A);
    fixture
        .driver
        .await_event(|event| matches!(event, DriverEvent::Open(_)), "adapter open");
    fixture
        .driver
        .await_event(|event| *event == DriverEvent::Waiting, "initial wait");
    let idle_ticks = qpc_frequency() * IDLE_STOP_MS as i64 / 1000;
    fixture.driver.advance(idle_ticks - qpc_frequency() / 60);
    fixture.driver.release_and_wait_for_next();
    assert!(fixture.driver.closed().is_empty());
    assert_eq!(fixture.driver.opened(), [MONITOR_A]);
}

fn qpc_frequency() -> i64 {
    display_clock::qpc_frequency()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
