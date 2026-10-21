//! Port of `PhotoReview.Shell.Win32/Hosting/FrameClockBase.cs` (184 lines, baseline `d92d6aae`). Unit W8-13a.
//!
//! The part of the C-05 frame clock that is the same on every source: the "armed" state (a `Frame` subscriber or a pending `request_frame`),
//! at most one tick per `on_signaled`/`on_deadline`, and the `FrameTick` that is raised. The tick source (a waitable handle plus an optional
//! deadline) is a [`ClockPolicy`]: [`super::timer::TimerPolicy`] (13a) and `swap_chain_clock::SwapChainPolicy` (13b). `MessageLoop` waits only on
//! armed clocks, so no subscriber means no wake-up and no CPU.
//!
//! Rust shape (D-W8B3H-2): a clock is shared (the message loop polls it, windows and presenters subscribe), so [`FrameClockBase`] has `&self` methods
//! over interior mutability and is used as `Rc<dyn HostFrameClock>`; it is `!Send` (UI thread only). `FrameTick`, `FrameHandler` and
//! `FrameHandlerId` are interim twins of `pr_app::input::frame_clock` (same fields, same meaning); the layering PR replaces them by imports and adds
//! `impl pr_app::input::frame_clock::FrameClock for` a newtype around `Rc<dyn HostFrameClock>`.

#![allow(unused_variables, dead_code)] // W8-13a stub: the porter removes this line when the last `todo!()` is gone.

use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use pr_core::abstractions::{DisplayClock, DisplayTiming};

use crate::host::error::HostError;

/// Identifies one subscription (replaces C# delegate identity for `-=`). Same type as `pr_app::input::frame_clock::FrameHandlerId`.
pub type FrameHandlerId = u64;

/// A `Frame` subscriber; runs on the UI thread. Same type as `pr_app::input::frame_clock::FrameHandler`.
pub type FrameHandler = Box<dyn FnMut(&FrameTick)>;

/// Clock seam for tests (C# `Func<long>? timestamp`): Stopwatch ticks, that is QueryPerformanceCounter ticks.
pub type TimestampFn = Box<dyn Fn() -> i64>;

/// Interim twin of `pr_app::input::frame_clock::FrameTick` (C# `FrameTick(long Timestamp, double RenderingTimeMs, DisplayTiming? Timing)`).
#[derive(Debug, Clone, Copy)]
pub struct FrameTick {
    /// QPC ticks of the frame (the scheduled vblank grid point or `now`).
    pub timestamp: i64,
    /// `(timestamp - epoch) * 1000.0 / Frequency`, epoch = the clock's creation time.
    pub rendering_time_ms: f64,
    /// `DisplayClock::get_timing(window)` at tick time; `None` when unknown.
    pub timing: Option<DisplayTiming>,
}

impl PartialEq for FrameTick {
    /// C# record equality: `rendering_time_ms` compares like `double.Equals` (NaN equals NaN, `0.0` equals `-0.0`), the rest field by field.
    fn eq(&self, other: &Self) -> bool {
        todo!("W8-13a")
    }
}

/// `Stopwatch.Frequency`: delegates to `crate::display_clock::qpc_frequency()` (already `pub(crate)`: QueryPerformanceFrequency once, cached, 1e9 when the
/// call fails; do not write a second copy).
pub fn qpc_frequency() -> i64 {
    todo!("W8-13a")
}

/// `Stopwatch.GetTimestamp()`: delegates to `crate::display_clock::qpc_timestamp()`; the 13a porter changes that private `fn` to `pub(crate)` (one word, the
/// only edit outside the owned files).
pub fn qpc_now() -> i64 {
    todo!("W8-13a")
}

/// C# `NextGridPoint(timing, at)`: the first vblank grid point `>= at`. `refresh_period <= 0` or `at <= last_vblank` gives `max(at, last_vblank)`;
/// else `last_vblank + ceil((at - last_vblank) / refresh_period) * refresh_period`. The arithmetic does not overflow (C# unchecked `long`): use
/// `i128` or `checked_*` with a saturating fallback (trap T7).
pub fn next_grid_point(timing: DisplayTiming, at: i64) -> i64 {
    todo!("W8-13a")
}

/// `(frame_timestamp - epoch) as f64 * 1000.0 / frequency as f64` (C# `(frameTimestamp - _epoch) * 1000.0 / Stopwatch.Frequency`: the
/// subtraction is `long`, the rest `double`). Pure.
pub fn rendering_time_ms(frame_timestamp: i64, epoch: i64, frequency: i64) -> f64 {
    todo!("W8-13a")
}

/// What a policy may ask of its clock: the display timing of the window's monitor (C# `CurrentTiming()`).
#[derive(Clone, Copy)]
pub struct ClockEnv<'a> {
    pub(super) display_clock: Option<&'a dyn DisplayClock>,
    pub(super) window: isize,
}

impl fmt::Debug for ClockEnv<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClockEnv")
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

impl ClockEnv<'_> {
    /// C# `_displayClock?.GetTiming(_window)`.
    pub fn current_timing(&self) -> Option<DisplayTiming> {
        todo!("W8-13a")
    }
}

/// The abstract members of C# `FrameClockBase`. A policy owns its state and is called by the base with the `RefCell` borrowed: it must not call
/// back into the clock (it cannot: it only receives a [`ClockEnv`]).
pub trait ClockPolicy {
    /// C# `WaitHandle`: the handle `MsgWaitForMultipleObjectsEx` waits on while armed; `0` = none.
    fn wait_handle(&self) -> isize;

    /// C# virtual `DeadlineTimestamp`: QPC ticks at which the loop must call `on_deadline` although the handle is not signaled; `i64::MAX` = none
    /// (the default). `armed` is the base's `IsArmed` (the swap-chain clock returns `i64::MAX` when not armed).
    fn deadline_timestamp(&self, armed: bool) -> i64 {
        i64::MAX
    }

    /// C# `ShouldTick(now, signaled, out frameTimestamp)`: `Some(frame_timestamp)` = tick now. May have side effects (the timer clock re-arms
    /// itself on an early signal), hence `&mut self` and the `Result`.
    fn should_tick(&mut self, env: &ClockEnv<'_>, now: i64, signaled: bool) -> Result<Option<i64>, HostError>;

    /// C# `OnArmedChanged(armed, now)`: start or stop the tick source.
    fn on_armed_changed(&mut self, env: &ClockEnv<'_>, armed: bool, now: i64) -> Result<(), HostError>;

    /// C# virtual `OnTicked(frameTimestamp, now)`: after a tick while still armed, schedule the next frame. Default: nothing.
    fn on_ticked(&mut self, env: &ClockEnv<'_>, frame_timestamp: i64, now: i64) -> Result<(), HostError> {
        Ok(())
    }

    /// C# virtual `DisposeCore()`: release native resources early (the timer handle). Default: nothing. Drop must release them too.
    fn dispose_core(&mut self) {}
}

/// The object-safe view of a clock that the other host parts hold (`Rc<dyn HostFrameClock>`): subscribers (`ShellWindow`), the message loop
/// (`add_frame_clock`) and the layering adapter. All `&self`, UI thread only.
pub trait HostFrameClock {
    /// C# `Frame += handler`. The id unsubscribes. Panics with the `HostError` text when arming the source fails (D-W8B3H-2, same as the C#
    /// `Win32Exception` escaping `Frame +=`); [`FrameClockBase::try_subscribe`] is the `Result` form.
    fn subscribe(&self, handler: FrameHandler) -> FrameHandlerId;
    /// C# `Frame -= handler`; an unknown id is a no-op. Safe to call from inside a handler.
    fn unsubscribe(&self, id: FrameHandlerId);
    /// C# `RequestFrame()`: one more frame even without a subscriber (coalesced).
    fn request_frame(&self);
    /// C# `IsArmed`.
    fn is_armed(&self) -> bool;
    /// C# `TickCount`.
    fn tick_count(&self) -> i64;
    /// C# `WaitHandle`.
    fn wait_handle(&self) -> isize;
    /// C# `DeadlineTimestamp`.
    fn deadline_timestamp(&self) -> i64;
    /// C# `TicksUntilDeadline`: `i64::MAX` when there is no deadline, else `deadline - now`.
    fn ticks_until_deadline(&self) -> i64;
    /// C# `OnSignaled()` (the loop calls it when the handle signaled): `Ok(true)` if a frame was raised.
    fn on_signaled(&self) -> Result<bool, HostError>;
    /// C# `OnDeadline()`.
    fn on_deadline(&self) -> Result<bool, HostError>;
    /// C# `Dispose()`: idempotent; drops all subscribers and disarms.
    fn dispose(&self);
}

struct Slot {
    id: FrameHandlerId,
    /// `None` while the handler runs (taken out so no borrow is held during the call).
    handler: Option<FrameHandler>,
    /// Unsubscribed while it was running: dropped when it comes back.
    removed: bool,
}

struct State<P> {
    policy: P,
    slots: Vec<Slot>,
    next_id: FrameHandlerId,
    requested: bool,
    armed: bool,
    disposed: bool,
    tick_count: i64,
}

/// C# `FrameClockBase` for a policy `P`. See the module doc for the sharing model.
pub struct FrameClockBase<P: ClockPolicy> {
    display_clock: Option<Arc<dyn DisplayClock>>,
    window: isize,
    now: TimestampFn,
    epoch: i64,
    state: RefCell<State<P>>,
    _not_send: PhantomData<*const ()>,
}

impl<P: ClockPolicy> fmt::Debug for FrameClockBase<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameClockBase")
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

impl<P: ClockPolicy> FrameClockBase<P> {
    /// C# protected constructor: `epoch = now()`; `timestamp = None` uses [`qpc_now`].
    pub fn new(
        policy: P,
        display_clock: Option<Arc<dyn DisplayClock>>,
        window: isize,
        timestamp: Option<TimestampFn>,
    ) -> Self {
        todo!("W8-13a")
    }

    /// `Result` form of `subscribe` (C# `Frame +=`).
    pub fn try_subscribe(&self, handler: FrameHandler) -> Result<FrameHandlerId, HostError> {
        todo!("W8-13a")
    }

    /// `Result` form of `request_frame`.
    pub fn try_request_frame(&self) -> Result<(), HostError> {
        todo!("W8-13a")
    }

    /// `Result` form of `unsubscribe`.
    pub fn try_unsubscribe(&self, id: FrameHandlerId) -> Result<(), HostError> {
        todo!("W8-13a")
    }

    /// Reads the policy (tests and the concrete clocks' accessors: `scheduled_frame`, `period`).
    pub fn with_policy<R>(&self, read: impl FnOnce(&P) -> R) -> R {
        todo!("W8-13a")
    }

    /// C# private `TryTick(signaled)`: the whole tick algorithm (see the contract, "Tick algorithm").
    fn try_tick(&self, signaled: bool) -> Result<bool, HostError> {
        todo!("W8-13a")
    }

    /// C# private `UpdateArmed()`: `armed = !disposed && (has live handlers || requested)`; on a change call `policy.on_armed_changed`.
    fn update_armed(&self) -> Result<(), HostError> {
        todo!("W8-13a")
    }
}

impl<P: ClockPolicy> HostFrameClock for FrameClockBase<P> {
    fn subscribe(&self, handler: FrameHandler) -> FrameHandlerId {
        todo!("W8-13a")
    }
    fn unsubscribe(&self, id: FrameHandlerId) {
        todo!("W8-13a")
    }
    fn request_frame(&self) {
        todo!("W8-13a")
    }
    fn is_armed(&self) -> bool {
        todo!("W8-13a")
    }
    fn tick_count(&self) -> i64 {
        todo!("W8-13a")
    }
    fn wait_handle(&self) -> isize {
        todo!("W8-13a")
    }
    fn deadline_timestamp(&self) -> i64 {
        todo!("W8-13a")
    }
    fn ticks_until_deadline(&self) -> i64 {
        todo!("W8-13a")
    }
    fn on_signaled(&self) -> Result<bool, HostError> {
        todo!("W8-13a")
    }
    fn on_deadline(&self) -> Result<bool, HostError> {
        todo!("W8-13a")
    }
    fn dispose(&self) {
        todo!("W8-13a")
    }
}
