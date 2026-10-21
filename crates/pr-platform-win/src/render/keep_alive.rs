//! Port of `PhotoReview.Shell.Rendering/GpuKeepAlive.cs` (231 lines, baseline `d92d6aae`). Unit W8-12h (test kinds F + N).
//!
//! WP21-STALL: keeps the GPU "awake" with a background thread that submits a tiny draw command every `interval_ms` while an activation
//! window (`activate(until_ticks)`) is open. Measured root cause (C# `docs/refactoring/perf/2026-10-12-wp21-stall.md`): on the Intel UHD 630
//! the first submission after the GPU has been idle for about 25-33 ms blocks for about 0.47 s in 25-50 % of the cases (in `EndDraw`,
//! or in `Present` when the frame is drawn on another adapter); swap chain, buffer count, latency, flush and D2D/D3D threading are
//! unrelated, WARP is not affected. Submissions at most 25 ms apart from a separate thread with its own D3D/D2D device are enough: the
//! UI thread never waits.
//!
//! The device is opened lazily at the first activation, ON the background thread (nothing is spent until the user interacts). No real GPU
//! (WARP only) or a failing device disables the keep-alive for good (`is_disabled`); nothing is ever propagated to the caller. Outside an
//! activation window the thread sleeps on an event (no wake-ups, no CPU, no GPU).
//!
//! This file does NOT read the environment: the switches `PHOTOREVIEW_RS_GPU_KEEPWARM`, `..._MS` and `..._INTERVAL_MS` (C#
//! `PHOTOREVIEW_GPU_KEEPWARM*`, R0002) belong to `KeepWarm.cs` (W8-16d, `KeepWarmOptions`).
//!
//! Threads: `activate`, `dispose` and the getters are callable from any thread. The work object ([`KeepAliveWork`]) is created, used and
//! dropped on the keep-warm thread only, so it needs no `Send` (a Direct3D / Direct2D device is not shared across threads).

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};

use super::device::{DeviceResources, RenderFault};
use crate::gfx::com::GfxError;
use crate::gfx::extras::WaitableTimer;

/// Name of the keep-warm thread (C# `Name = "GpuKeepAlive"`).
pub const THREAD_NAME: &str = "GpuKeepAlive";
/// C# `Math.Clamp(intervalMs, 1, 1000)`: smallest interval in milliseconds.
pub const MIN_INTERVAL_MS: i32 = 1;
/// Largest interval in milliseconds.
pub const MAX_INTERVAL_MS: i32 = 1000;
/// C# `thread.Join(3000)` in `Dispose`.
pub const DISPOSE_JOIN_TIMEOUT_MS: u32 = 3000;
/// Unit of every `*_ticks` value of this file: nanoseconds of a monotonic clock (C# used `Stopwatch` ticks; the unit is private to the
/// caller pair `monotonic_ticks` / `ticks_from_ms`, so any monotonic unit is faithful).
pub const TICKS_PER_SECOND: i64 = 1_000_000_000;

/// `E_FAIL`, used for a device that reports a non-COM failure (for example "disposed") through [`GfxError`].
const E_FAIL_HRESULT: i32 = 0x8000_4005_u32 as i32;

/// C# `Stopwatch.GetTimestamp()`: monotonic ticks (since a fixed process-wide epoch), never negative.
#[must_use]
pub fn monotonic_ticks() -> i64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    i64::try_from(epoch.elapsed().as_nanos()).unwrap_or(i64::MAX)
}

/// `ms * TICKS_PER_SECOND / 1000` in `i64` with saturation (C# `ms * Stopwatch.Frequency / 1000`); W8-16d computes `now + ticks_from_ms(window)`.
#[must_use]
pub fn ticks_from_ms(ms: i64) -> i64 {
    ms.saturating_mul(TICKS_PER_SECOND / 1000)
}

/// C# `Math.Clamp(intervalMs, 1, 1000)`.
#[must_use]
pub fn clamp_interval_ms(interval_ms: i32) -> i32 {
    interval_ms.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS)
}

/// C# `IGpuKeepAliveWork` minus `IDisposable` (`Drop`): one tiny "touch" of the GPU. Created, used and dropped on the keep-warm thread.
pub trait KeepAliveWork {
    /// Submits an empty command to the GPU. `Err` means the device is broken: the keep-alive disables itself (C#: `Touch` throws).
    fn touch(&mut self) -> Result<(), GfxError>;
}

/// C# `Func<IGpuKeepAliveWork?> openWork`, called ON the background thread, once: `Ok(None)` = no real GPU (the keep-alive disables
/// itself), `Err` = the device could not be created (same effect, C# caught the exception).
pub type OpenKeepAliveWork = Box<dyn FnOnce() -> Result<Option<Box<dyn KeepAliveWork>>, GfxError> + Send + 'static>;

/// C# `Func<long>? timestamp`.
pub type TickClock = Arc<dyn Fn() -> i64 + Send + Sync + 'static>;

/// The optional constructor arguments of C#.
#[derive(Clone)]
pub struct KeepAliveOptions {
    /// Injected clock; `None` = [`monotonic_ticks`]. Tests inject a fake so the activation window never depends on thread scheduling.
    pub timestamp: Option<TickClock>,
    /// C# `ThreadPriority.BelowNormal` (default `true`: the keep-warm thread never competes with the UI for CPU). Tests pass `false`.
    pub below_normal_priority: bool,
}

impl Default for KeepAliveOptions {
    fn default() -> Self {
        Self {
            timestamp: None,
            below_normal_priority: true,
        }
    }
}

impl fmt::Debug for KeepAliveOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeepAliveOptions")
            .field("timestamp", &self.timestamp.as_ref().map(|_| "<clock>"))
            .field("below_normal_priority", &self.below_normal_priority)
            .finish()
    }
}

/// The slice of `DeviceResources` used by the production work (C# `HardwareWork`): an offscreen 16x16 device on the hardware adapter.
/// Implemented for `Rc<DeviceResources>`; the trait keeps [`open_hardware_work`] testable with a fake device.
pub trait KeepAliveDevice {
    /// C# `IsWarp`.
    fn is_warp(&self) -> bool;
    /// C# `DeviceResources.TouchGpu()`: `BeginDraw`, `Clear` black, `EndDraw` (checked), `ID3D11DeviceContext::Flush`; no `Present`.
    fn touch_gpu(&mut self) -> Result<(), GfxError>;
}

/// `RenderFault` (the error type of `DeviceResources`) as a [`GfxError`]: a failed COM call keeps its HRESULT, any other fault
/// (disposed, bad argument) becomes `E_FAIL` named after the call.
fn fault_to_gfx(operation: &'static str, fault: RenderFault) -> GfxError {
    match fault {
        RenderFault::Device(error) => error,
        _ => GfxError::new(operation, E_FAIL_HRESULT),
    }
}

impl KeepAliveDevice for Rc<DeviceResources> {
    fn is_warp(&self) -> bool {
        DeviceResources::is_warp(self)
    }

    fn touch_gpu(&mut self) -> Result<(), GfxError> {
        DeviceResources::touch_gpu(self).map_err(|fault| fault_to_gfx("DeviceResources::TouchGpu", fault))
    }
}

/// The production work (C# `HardwareWork`): `touch` = `touch_gpu`, drop = the device is dropped.
struct HardwareWork {
    device: Box<dyn KeepAliveDevice>,
}

impl KeepAliveWork for HardwareWork {
    fn touch(&mut self) -> Result<(), GfxError> {
        self.device.touch_gpu()
    }
}

/// C# `GpuKeepAlive.OpenHardwareWork()` for an injected device factory (`create` = `DeviceResources::create_offscreen(16, 16, 1.0,
/// prefer_warp = false)`): `Ok(None)` when the device is WARP (the device is dropped at once), otherwise the production work.
pub fn open_hardware_work(
    create: impl FnOnce() -> Result<Box<dyn KeepAliveDevice>, GfxError>,
) -> Result<Option<Box<dyn KeepAliveWork>>, GfxError> {
    let device = create()?;
    if device.is_warp() {
        drop(device);
        return Ok(None);
    }
    Ok(Some(Box::new(HardwareWork { device })))
}

/// The production factory for [`OpenKeepAliveWork`] (W8-16d): [`open_hardware_work`] over
/// `DeviceResources::create_offscreen(16, 16, 1.0, false)`. Must run on the keep-warm thread (it does when passed to
/// [`GpuKeepAlive::new`]).
pub fn open_offscreen_hardware_work() -> Result<Option<Box<dyn KeepAliveWork>>, GfxError> {
    open_hardware_work(|| {
        let resources = DeviceResources::create_offscreen(16, 16, 1.0, false)
            .map_err(|fault| fault_to_gfx("DeviceResources::CreateOffscreen", fault))?;
        Ok(Box::new(resources) as Box<dyn KeepAliveDevice>)
    })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// State of the wake event (an auto-reset event) and of the "parked" flag, behind one mutex.
#[derive(Default)]
struct WakeState {
    /// The latch: `set` stores `true`, the thread's wait consumes it.
    signaled: bool,
    /// The thread is (about to be) blocked on the event. Set under the mutex immediately before `Condvar::wait`.
    blocked: bool,
}

/// Shared between the object and the keep-warm thread.
struct Shared {
    interval_ms: i32,
    clock: TickClock,
    until_ticks: AtomicI64,
    ticks: AtomicI64,
    idle_waits: AtomicI64,
    stop: AtomicBool,
    failed: AtomicBool,
    wake: Mutex<WakeState>,
    wake_cv: Condvar,
    /// Notified each time the thread sets `blocked` (for `wait_blocked`).
    blocked_cv: Condvar,
    /// Set by the thread as its LAST act (after the work was dropped).
    done: Mutex<bool>,
    done_cv: Condvar,
}

impl Shared {
    /// The auto-reset event `Set()`.
    fn set_wake(&self) {
        lock(&self.wake).signaled = true;
        self.wake_cv.notify_one();
    }

    fn is_blocked_with(&self, state: &WakeState, min_idle_waits: i64) -> bool {
        state.blocked && self.idle_waits.load(Ordering::SeqCst) >= min_idle_waits
    }

    /// The auto-reset event `WaitOne()` with the bookkeeping of the loop head: `idle_waits += 1` once per outer iteration, `blocked`
    /// while parked.
    fn wait_for_activation(&self) {
        let mut state = lock(&self.wake);
        self.idle_waits.fetch_add(1, Ordering::SeqCst);
        while !state.signaled {
            state.blocked = true;
            self.blocked_cv.notify_all();
            state = self.wake_cv.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        state.signaled = false;
        state.blocked = false;
    }

    /// C# `SleepInterval(timer)`.
    fn sleep_interval(&self, timer: Option<&WaitableTimer>) {
        let interval = i64::from(self.interval_ms);
        if let Some(timer) = timer
            && timer.set_relative_ms(interval).is_ok()
        {
            let timeout = u32::try_from(self.interval_ms)
                .unwrap_or(MAX_INTERVAL_MS as u32)
                .saturating_add(100);
            let _ = timer.wait(timeout, false);
            return;
        }
        thread::sleep(Duration::from_millis(u64::try_from(self.interval_ms).unwrap_or(1)));
    }
}

/// C# `ThreadPriority.BelowNormal` on the calling thread (a courtesy: a failure is ignored).
fn lower_current_thread_priority() {
    // SAFETY: `GetCurrentThread` returns a pseudo-handle that needs no closing and is valid only on the calling thread, which is where it
    // is used; `SetThreadPriority` takes it by value and has no pointer arguments.
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
}

/// The thread entry: the body under `catch_unwind` (a panic acts like the C# `catch (Exception)`), then the `done` signal.
fn run(shared: &Shared, open_work: OpenKeepAliveWork, below_normal: bool) {
    let outcome = catch_unwind(AssertUnwindSafe(|| run_body(shared, open_work, below_normal)));
    if outcome.is_err() {
        shared.failed.store(true, Ordering::SeqCst);
    }
    *lock(&shared.done) = true;
    shared.done_cv.notify_all();
}

fn run_body(shared: &Shared, open_work: OpenKeepAliveWork, below_normal: bool) {
    let mut work = match open_work() {
        Ok(Some(work)) => work,
        // No real GPU, or the device could not be created: disable for good.
        Ok(None) | Err(_) => {
            shared.failed.store(true, Ordering::SeqCst);
            return;
        }
    };
    // The timer belongs to this thread (`!Send`); a failure leaves `None` and the loop sleeps with `thread::sleep`.
    let timer = WaitableTimer::new().ok();
    if below_normal {
        lower_current_thread_priority();
    }
    let looped = catch_unwind(AssertUnwindSafe(|| {
        keep_warm_loop(shared, work.as_mut(), timer.as_ref())
    }));
    if looped.is_err() {
        shared.failed.store(true, Ordering::SeqCst);
    }
    // C# `finally`: close the timer, dispose the work (a panic in its `drop` is swallowed).
    drop(timer);
    let _ = catch_unwind(AssertUnwindSafe(move || drop(work)));
}

fn keep_warm_loop(shared: &Shared, work: &mut dyn KeepAliveWork, timer: Option<&WaitableTimer>) {
    while !shared.stop.load(Ordering::SeqCst) {
        shared.wait_for_activation();
        while !shared.stop.load(Ordering::SeqCst) && (shared.clock)() < shared.until_ticks.load(Ordering::SeqCst) {
            if work.touch().is_err() {
                shared.failed.store(true, Ordering::SeqCst);
                return;
            }
            shared.ticks.fetch_add(1, Ordering::SeqCst);
            shared.sleep_interval(timer);
        }
    }
}

/// The thread handle and the "was started" flag, under the C# `_gate`.
struct Gate {
    handle: Option<JoinHandle<()>>,
}

/// The keep-warm controller (C# `GpuKeepAlive`). `Drop` = `Dispose`.
pub struct GpuKeepAlive {
    shared: Arc<Shared>,
    below_normal: bool,
    /// Taken by the first `activate` and moved to the thread.
    open_work: Mutex<Option<OpenKeepAliveWork>>,
    gate: Mutex<Gate>,
    /// A thread was spawned (stays true after it ended or its handle was taken by `dispose`).
    spawned: AtomicBool,
}

impl fmt::Debug for GpuKeepAlive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GpuKeepAlive")
            .field("interval_ms", &self.shared.interval_ms)
            .field("ticks", &self.ticks())
            .field("idle_waits", &self.idle_waits())
            .field("has_thread", &self.has_thread())
            .field("is_disabled", &self.is_disabled())
            .finish()
    }
}

impl GpuKeepAlive {
    /// C# constructor. `interval_ms` is clamped to 1..=1000 ([`clamp_interval_ms`]). No thread, no device, no allocation of GPU resources.
    pub fn new(interval_ms: i32, open_work: OpenKeepAliveWork, options: KeepAliveOptions) -> Self {
        let clock: TickClock = options.timestamp.unwrap_or_else(|| Arc::new(monotonic_ticks));
        Self {
            shared: Arc::new(Shared {
                interval_ms: clamp_interval_ms(interval_ms),
                clock,
                until_ticks: AtomicI64::new(0),
                ticks: AtomicI64::new(0),
                idle_waits: AtomicI64::new(0),
                stop: AtomicBool::new(false),
                failed: AtomicBool::new(false),
                wake: Mutex::new(WakeState::default()),
                wake_cv: Condvar::new(),
                blocked_cv: Condvar::new(),
                done: Mutex::new(false),
                done_cv: Condvar::new(),
            }),
            below_normal: options.below_normal_priority,
            open_work: Mutex::new(Some(open_work)),
            gate: Mutex::new(Gate { handle: None }),
            spawned: AtomicBool::new(false),
        }
    }

    /// C# `Ticks`: number of successful touches (diagnostics / tests).
    #[must_use]
    pub fn ticks(&self) -> i64 {
        self.shared.ticks.load(Ordering::SeqCst)
    }

    /// C# `IdleWaits`: how many times the thread entered the wait on the activation event (each `activate` wakes it exactly once).
    #[must_use]
    pub fn idle_waits(&self) -> i64 {
        self.shared.idle_waits.load(Ordering::SeqCst)
    }

    /// C# `IsBlocked`, narrowed: the thread exists and is parked on the activation event (not in the interval sleep). Observable
    /// "not spinning" signal that needs no CPU measurement.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        self.has_thread() && lock(&self.shared.wake).blocked
    }

    /// NEW (replaces the C# `SpinWait.SpinUntil(() => IdleWaits >= n && IsBlocked)` polling of the tests, AGENTS.md forbids sleep polling): blocks the
    /// caller on a `Condvar` until the thread is parked on the activation event with `idle_waits() >= min_idle_waits`, or until `timeout`. Returns
    /// whether the condition was reached. Diagnostics / tests only.
    #[must_use]
    pub fn wait_blocked(&self, min_idle_waits: i64, timeout: Duration) -> bool {
        let shared = &self.shared;
        let guard = lock(&shared.wake);
        let (guard, _) = shared
            .blocked_cv
            .wait_timeout_while(guard, timeout, |state| !shared.is_blocked_with(state, min_idle_waits))
            .unwrap_or_else(PoisonError::into_inner);
        shared.is_blocked_with(&guard, min_idle_waits)
    }

    /// NEW (sibling of [`wait_blocked`](Self::wait_blocked), no C# twin, tests / diagnostics only): blocks the caller until the keep-warm
    /// thread has ended (every failure path ends it, after setting `failed`) or until `timeout`, then returns [`is_disabled`](Self::is_disabled).
    /// Replaces polling `is_disabled()` for the "no GPU" / "broken device" tests.
    #[must_use]
    pub fn wait_disabled(&self, timeout: Duration) -> bool {
        if self.is_disabled() {
            return true;
        }
        let done = lock(&self.shared.done);
        let (done, _) = self
            .shared
            .done_cv
            .wait_timeout_while(done, timeout, |finished| !*finished)
            .unwrap_or_else(PoisonError::into_inner);
        drop(done);
        self.is_disabled()
    }

    /// C# `IsDisabled`: `failed || stop` (no real GPU, broken device, or disposed).
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        self.shared.failed.load(Ordering::SeqCst) || self.shared.stop.load(Ordering::SeqCst)
    }

    /// C# `HasThread`: false until the first `activate`.
    #[must_use]
    pub fn has_thread(&self) -> bool {
        self.spawned.load(Ordering::SeqCst)
    }

    /// C# `Activate(untilTimestamp)`: extends the window to `until_ticks` (never shortens it), starts the thread at the first call, wakes
    /// the thread once. No-op when disabled. A failing `thread::Builder::spawn` disables the keep-alive (never panics).
    pub fn activate(&self, until_ticks: i64) {
        let shared = &self.shared;
        if self.is_disabled() {
            return;
        }
        {
            let mut gate = lock(&self.gate);
            if shared.stop.load(Ordering::SeqCst) {
                return;
            }
            shared.until_ticks.fetch_max(until_ticks, Ordering::SeqCst);
            if !self.spawned.load(Ordering::SeqCst) {
                let Some(open_work) = lock(&self.open_work).take() else {
                    shared.failed.store(true, Ordering::SeqCst);
                    return;
                };
                let thread_shared = Arc::clone(shared);
                let below_normal = self.below_normal;
                let spawned = thread::Builder::new()
                    .name(THREAD_NAME.to_owned())
                    .spawn(move || run(&thread_shared, open_work, below_normal));
                match spawned {
                    Ok(handle) => {
                        gate.handle = Some(handle);
                        self.spawned.store(true, Ordering::SeqCst);
                    }
                    Err(_) => {
                        shared.failed.store(true, Ordering::SeqCst);
                        return;
                    }
                }
            }
        }
        // Outside the lock, like C#.
        shared.set_wake();
    }

    /// C# `Dispose()`: idempotent; sets `stop`, wakes the thread, waits up to [`DISPOSE_JOIN_TIMEOUT_MS`] for the thread to finish
    /// (it drops the work on its own thread first), then returns. A thread that does not finish in time is left detached.
    pub fn dispose(&self) {
        let shared = &self.shared;
        let handle = {
            let mut gate = lock(&self.gate);
            if shared.stop.swap(true, Ordering::SeqCst) {
                return;
            }
            gate.handle.take()
        };
        // `stop` is set before the wake (T4); the latch makes the wake impossible to lose (T3).
        shared.set_wake();
        let Some(handle) = handle else {
            return;
        };
        let done = lock(&shared.done);
        let (done, _) = shared
            .done_cv
            .wait_timeout_while(
                done,
                Duration::from_millis(u64::from(DISPOSE_JOIN_TIMEOUT_MS)),
                |finished| !*finished,
            )
            .unwrap_or_else(PoisonError::into_inner);
        let finished = *done;
        drop(done);
        if finished {
            let _ = handle.join();
        }
        // Otherwise the handle is dropped here and the thread stays detached (never block forever).
    }
}

impl Drop for GpuKeepAlive {
    fn drop(&mut self) {
        self.dispose();
    }
}
