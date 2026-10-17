//! Port of `WindowsDisplayClock` from `PhotoReview.Platform.Windows/WindowsDisplayClock.cs` (baseline `d92d6aae`).
//!
//! The caller only reads the latest monitor-tagged publication. A worker owns vblank waits and never holds the state
//! mutex while blocked in the driver.

#[cfg(not(windows))]
use std::time::Instant;
use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    thread::{self, JoinHandle},
};

use pr_core::{
    abstractions::{DisplayClock, DisplayTiming, Log, NullLog},
    display::vblank_estimator::VBlankEstimator,
};

use crate::vblank_driver::{IVBlankDriver, NativeImportError, NativeVBlankDriver};

/// C# `WindowsDisplayClock.IdleStopMs`.
pub const IDLE_STOP_MS: u64 = 1500;
/// C# `WindowsDisplayClock.RetryFailedAfterMs`.
pub const RETRY_FAILED_AFTER_MS: u64 = 8000;

const CLOCK_THREAD_NAME: &str = "PhotoReview vblank clock";

struct Published {
    monitor: isize,
    timing: DisplayTiming,
}

#[derive(Default)]
struct State {
    monitor: isize,
    worker: Option<JoinHandle<()>>,
    retired_workers: Vec<JoinHandle<()>>,
    published: Option<Published>,
    failed_monitor: isize,
    failed_at: i64,
}

struct Shared {
    state: Mutex<State>,
    state_changed: Condvar,
    last_use: AtomicI64,
    stopping: AtomicBool,
    driver: Arc<dyn IVBlankDriver>,
    timestamp: Arc<dyn Fn() -> i64 + Send + Sync>,
    log: Arc<dyn Log>,
}

/// Asynchronous vblank clock for the monitor containing an HWND.
pub struct WindowsDisplayClock {
    shared: Arc<Shared>,
}

impl WindowsDisplayClock {
    /// Creates the native clock. The native driver is stateless; worker state belongs to this instance.
    pub fn new() -> Self {
        Self::with_driver(
            Some(Arc::new(NullLog)),
            Some(Arc::new(NativeVBlankDriver::INSTANCE)),
            Some(Arc::new(qpc_timestamp)),
        )
        .unwrap_or_else(|_| unreachable!("native clock dependencies are present"))
    }

    /// Process singleton matching C# `WindowsDisplayClock.Instance`.
    #[allow(dead_code)] // Public production entry point; the test module includes this source before lead-owned lib wiring.
    pub fn instance() -> &'static Self {
        static INSTANCE: OnceLock<WindowsDisplayClock> = OnceLock::new();
        INSTANCE.get_or_init(Self::new)
    }

    /// Test seam matching the C# internal constructor. The log is optional; driver and QPC source are required.
    pub(crate) fn with_driver(
        log: Option<Arc<dyn Log>>,
        driver: Option<Arc<dyn IVBlankDriver>>,
        timestamp: Option<Arc<dyn Fn() -> i64 + Send + Sync>>,
    ) -> Result<Self, &'static str> {
        let driver = driver.ok_or("driver must not be null")?;
        let timestamp = timestamp.ok_or("timestamp source must not be null")?;
        Ok(Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                state_changed: Condvar::new(),
                last_use: AtomicI64::new(0),
                stopping: AtomicBool::new(false),
                driver,
                timestamp,
                log: log.unwrap_or_else(|| Arc::new(NullLog)),
            }),
        })
    }

    /// The compositor timing (primary display), or `None` if the native API is unavailable.
    #[allow(dead_code)] // Public production entry point; native DWM access is deliberately excluded from fake tests.
    pub fn dwm_timing() -> Option<DisplayTiming> {
        NativeVBlankDriver::INSTANCE.dwm_timing()
    }

    /// Stops and joins workers. Callers that inject a blocking driver must release its wait first.
    pub(crate) fn shutdown_and_join(&self) {
        self.shared.stopping.store(true, Ordering::Release);
        loop {
            let handles = {
                let mut state = lock(&self.shared.state);
                let mut handles = std::mem::take(&mut state.retired_workers);
                if let Some(worker) = state.worker.take() {
                    handles.push(worker);
                }
                handles
            };
            if handles.is_empty() {
                return;
            }
            for handle in handles {
                if handle.thread().id() != thread::current().id() {
                    let _ = handle.join();
                }
            }
        }
    }

    pub(crate) fn request_stop(&self) {
        self.shared.stopping.store(true, Ordering::Release);
    }

    /// Waits for the failure state transition without timing-based polling in fake-driven tests.
    pub(crate) fn await_failed_monitor(&self, monitor: isize) {
        let (state, _result) = self
            .shared
            .state_changed
            .wait_timeout_while(lock(&self.shared.state), std::time::Duration::from_secs(20), |state| {
                state.failed_monitor != monitor
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            state.failed_monitor, monitor,
            "timed out waiting for monitor failure transition"
        );
    }
}

impl Default for WindowsDisplayClock {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayClock for WindowsDisplayClock {
    fn get_timing(&self, window: isize) -> Option<DisplayTiming> {
        let monitor = if window != 0 {
            self.shared.driver.monitor_from_window(window)
        } else {
            0
        };
        if monitor == 0 {
            return self.shared.driver.dwm_timing();
        }

        self.shared.last_use.store((self.shared.timestamp)(), Ordering::Release);
        let mut state = lock(&self.shared.state);
        reap_finished_workers(&mut state);
        if monitor == state.failed_monitor {
            let elapsed =
                ((self.shared.timestamp)().wrapping_sub(state.failed_at)) as f64 * 1000.0 / qpc_frequency() as f64;
            if elapsed < RETRY_FAILED_AFTER_MS as f64 {
                return self.shared.driver.dwm_timing();
            }
            state.failed_monitor = 0;
        }
        state.monitor = monitor;
        if state.worker.is_none() && !self.shared.stopping.load(Ordering::Acquire) {
            let shared = Arc::clone(&self.shared);
            match thread::Builder::new()
                .name(CLOCK_THREAD_NAME.to_owned())
                .spawn(move || worker_main(shared))
            {
                Ok(worker) => state.worker = Some(worker),
                Err(error) => {
                    self.shared
                        .log
                        .error("Vblank clock thread could not start", Some(&error));
                    state.failed_monitor = monitor;
                    state.failed_at = (self.shared.timestamp)();
                    return self.shared.driver.dwm_timing();
                }
            }
        }
        state
            .published
            .as_ref()
            .filter(|published| published.monitor == monitor)
            .map(|published| published.timing)
    }
}

fn worker_main(shared: Arc<Shared>) {
    let mut adapter = None;
    let mut opened = 0;
    let mut target = 0;
    let mut estimator = VBlankEstimator::new();

    let result = catch_unwind(AssertUnwindSafe(|| {
        loop {
            let wanted = {
                let mut state = lock(&shared.state);
                let idle = ((shared.timestamp)().wrapping_sub(shared.last_use.load(Ordering::Acquire))) as f64 * 1000.0
                    / qpc_frequency() as f64;
                if idle > IDLE_STOP_MS as f64 || shared.stopping.load(Ordering::Acquire) {
                    state.published = None;
                    retire_active_worker(&mut state);
                    return;
                }
                target = state.monitor;
                target
            };

            if wanted != opened {
                close_adapter(&shared, &mut adapter);
                estimator.reset();
                lock(&shared.state).published = None;
                opened = wanted;
                match shared.driver.try_open(wanted) {
                    Ok(Some(opened_adapter)) => adapter = Some(opened_adapter),
                    Ok(None) => {
                        mark_failed(
                            &shared,
                            wanted,
                            "opening the adapter failed (GetMonitorInfo, CreateDC or D3DKMTOpenAdapterFromHdc)",
                        );
                        return;
                    }
                    Err(error) => {
                        mark_failed(&shared, wanted, &native_import_message(error));
                        return;
                    }
                };
            }

            let Some((adapter_id, source_id)) = adapter else {
                mark_failed(
                    &shared,
                    target,
                    "opening the adapter failed (driver returned no adapter)",
                );
                return;
            };
            let wait_status = match shared.driver.wait_for_vblank(adapter_id, source_id) {
                Ok(status) => status,
                Err(error) => {
                    if !shared.stopping.load(Ordering::Acquire) {
                        mark_failed(&shared, opened, &native_import_message(error));
                    }
                    return;
                }
            };
            if shared.stopping.load(Ordering::Acquire) {
                return;
            }
            if wait_status != 0 {
                mark_failed(
                    &shared,
                    opened,
                    &format!("D3DKMTWaitForVerticalBlankEvent returned 0x{wait_status:08X}"),
                );
                return;
            }
            estimator.add((shared.timestamp)());
            if let Some(timing) = estimator.current() {
                lock(&shared.state).published = Some(Published {
                    monitor: opened,
                    timing,
                });
            }
        }
    }));

    if let Err(payload) = result {
        let message = panic_message(payload.as_ref());
        let error = std::io::Error::other(message.clone());
        shared
            .log
            .error("Vblank clock thread failed; falling back to DWM timing", Some(&error));
        let failing = if opened != 0 { opened } else { target };
        if failing != 0 {
            mark_failed(&shared, failing, &format!("thread failed with {message}"));
        } else {
            retire_worker(&shared);
        }
    }
    close_adapter(&shared, &mut adapter);
    retire_worker(&shared);
}

fn mark_failed(shared: &Shared, monitor: isize, step: &str) {
    shared.log.warn(&format!(
        "Vblank clock unavailable for monitor 0x{:X}, falling back to DWM timing (primary display only) for {} ms: {step}",
        monitor as u64, RETRY_FAILED_AFTER_MS
    ));
    let mut state = lock(&shared.state);
    state.failed_monitor = monitor;
    state.failed_at = (shared.timestamp)();
    retire_active_worker(&mut state);
    shared.state_changed.notify_all();
}

fn native_import_message(error: NativeImportError) -> String {
    format!("{}: native vblank dependency is unavailable", error.type_name())
}

fn close_adapter(shared: &Shared, adapter: &mut Option<(u32, u32)>) {
    if let Some((adapter_id, _)) = adapter.take() {
        shared.driver.close(adapter_id);
    }
}

fn retire_worker(shared: &Shared) {
    let mut state = lock(&shared.state);
    retire_active_worker(&mut state);
}

fn retire_active_worker(state: &mut State) {
    if let Some(worker) = state.worker.take() {
        state.retired_workers.push(worker);
    }
}

fn reap_finished_workers(state: &mut State) {
    let mut still_running = Vec::new();
    for worker in state.retired_workers.drain(..) {
        if worker.is_finished() {
            let _ = worker.join();
        } else {
            still_running.push(worker);
        }
    }
    state.retired_workers = still_running;
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|message| (*message).to_owned()))
        .unwrap_or_else(|| "unknown panic payload".to_owned())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(windows)]
pub(crate) fn qpc_frequency() -> i64 {
    static FREQUENCY: OnceLock<i64> = OnceLock::new();
    *FREQUENCY.get_or_init(|| {
        let mut frequency = 0_i64;
        // SAFETY: `frequency` is a valid writable i64 for the synchronous QueryPerformanceFrequency call.
        if unsafe { QueryPerformanceFrequency(&mut frequency) } != 0 && frequency > 0 {
            frequency
        } else {
            1_000_000_000
        }
    })
}

#[cfg(windows)]
fn qpc_timestamp() -> i64 {
    let mut counter = 0_i64;
    // SAFETY: `counter` is a valid writable i64 for the synchronous QueryPerformanceCounter call.
    if unsafe { QueryPerformanceCounter(&mut counter) } != 0 {
        counter
    } else {
        0
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryPerformanceCounter(counter: *mut i64) -> i32;
    fn QueryPerformanceFrequency(frequency: *mut i64) -> i32;
}

#[cfg(not(windows))]
pub(crate) fn qpc_frequency() -> i64 {
    1_000_000_000
}

#[cfg(not(windows))]
fn qpc_timestamp() -> i64 {
    static START: OnceLock<Instant> = OnceLock::new();
    i64::try_from(START.get_or_init(Instant::now).elapsed().as_nanos()).unwrap_or(i64::MAX)
}
