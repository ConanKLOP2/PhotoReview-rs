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

#![allow(unused_variables, dead_code)] // W8-12h stub: the porter removes this line when the last `todo!()` is gone.

use std::fmt;
use std::sync::Arc;

use crate::gfx::com::GfxError;

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

/// C# `Stopwatch.GetTimestamp()`: monotonic ticks (since a fixed process-wide epoch), never negative.
#[must_use]
pub fn monotonic_ticks() -> i64 {
    todo!("W8-12h")
}

/// `ms * TICKS_PER_SECOND / 1000` in `i64` with saturation (C# `ms * Stopwatch.Frequency / 1000`); W8-16d computes `now + ticks_from_ms(window)`.
#[must_use]
pub fn ticks_from_ms(ms: i64) -> i64 {
    todo!("W8-12h")
}

/// C# `Math.Clamp(intervalMs, 1, 1000)`.
#[must_use]
pub fn clamp_interval_ms(interval_ms: i32) -> i32 {
    todo!("W8-12h")
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
        todo!("W8-12h")
    }
}

impl fmt::Debug for KeepAliveOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W8-12h")
    }
}

/// The slice of W8-12b/12c `DeviceResources` used by the production work (C# `HardwareWork`): an offscreen 16x16 device on the hardware
/// adapter. PLACEHOLDER (D-W8B3S-4): the real `DeviceResources` implements it; the porter may drop the trait and name the type.
pub trait KeepAliveDevice {
    /// C# `IsWarp`.
    fn is_warp(&self) -> bool;
    /// C# `DeviceResources.TouchGpu()`: `BeginDraw`, `Clear` black, `EndDraw` (checked), `ID3D11DeviceContext::Flush`; no `Present`.
    fn touch_gpu(&mut self) -> Result<(), GfxError>;
}

/// C# `GpuKeepAlive.OpenHardwareWork()` for an injected device factory (`create` = `DeviceResources::create_offscreen(16, 16, 1.0,
/// prefer_warp = false)`): `Ok(None)` when the device is WARP (the device is dropped at once), otherwise the production work.
pub fn open_hardware_work(
    create: impl FnOnce() -> Result<Box<dyn KeepAliveDevice>, GfxError>,
) -> Result<Option<Box<dyn KeepAliveWork>>, GfxError> {
    todo!("W8-12h")
}

/// The keep-warm controller (C# `GpuKeepAlive`). `Drop` = `Dispose`.
pub struct GpuKeepAlive {
    // interval_ms, open_work (taken by the thread), clock, wake event (Mutex<bool> + Condvar, auto-reset), gate Mutex<{thread handle,
    // done flag}>, atomics: until_ticks (fetch_max), ticks, idle_waits, stop, failed, blocked.
}

impl fmt::Debug for GpuKeepAlive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W8-12h")
    }
}

impl GpuKeepAlive {
    /// C# constructor. `interval_ms` is clamped to 1..=1000 ([`clamp_interval_ms`]). No thread, no device, no allocation of GPU resources.
    pub fn new(interval_ms: i32, open_work: OpenKeepAliveWork, options: KeepAliveOptions) -> Self {
        todo!("W8-12h")
    }

    /// C# `Ticks`: number of successful touches (diagnostics / tests).
    #[must_use]
    pub fn ticks(&self) -> i64 {
        todo!("W8-12h")
    }

    /// C# `IdleWaits`: how many times the thread entered the wait on the activation event (each `activate` wakes it exactly once).
    #[must_use]
    pub fn idle_waits(&self) -> i64 {
        todo!("W8-12h")
    }

    /// C# `IsBlocked`, narrowed: the thread exists and is parked on the activation event (not in the interval sleep). Observable
    /// "not spinning" signal that needs no CPU measurement.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        todo!("W8-12h")
    }

    /// NEW (replaces the C# `SpinWait.SpinUntil(() => IdleWaits >= n && IsBlocked)` polling of the tests, AGENTS.md forbids sleep polling): blocks the
    /// caller on a `Condvar` until the thread is parked on the activation event with `idle_waits() >= min_idle_waits`, or until `timeout`. Returns
    /// whether the condition was reached. Diagnostics / tests only.
    #[must_use]
    pub fn wait_blocked(&self, min_idle_waits: i64, timeout: std::time::Duration) -> bool {
        todo!("W8-12h")
    }

    /// C# `IsDisabled`: `failed || stop` (no real GPU, broken device, or disposed).
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        todo!("W8-12h")
    }

    /// C# `HasThread`: false until the first `activate`.
    #[must_use]
    pub fn has_thread(&self) -> bool {
        todo!("W8-12h")
    }

    /// C# `Activate(untilTimestamp)`: extends the window to `until_ticks` (never shortens it), starts the thread at the first call, wakes
    /// the thread once. No-op when disabled. A failing `thread::Builder::spawn` disables the keep-alive (never panics).
    pub fn activate(&self, until_ticks: i64) {
        todo!("W8-12h")
    }

    /// C# `Dispose()`: idempotent; sets `stop`, wakes the thread, waits up to [`DISPOSE_JOIN_TIMEOUT_MS`] for the thread to finish
    /// (it drops the work on its own thread first), then returns. A thread that does not finish in time is left detached.
    pub fn dispose(&self) {
        todo!("W8-12h")
    }
}

impl Drop for GpuKeepAlive {
    fn drop(&mut self) {
        // The porter calls `self.dispose()`.
    }
}
