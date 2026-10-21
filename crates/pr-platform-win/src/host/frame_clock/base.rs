//! Port of `PhotoReview.Shell.Win32/Hosting/FrameClockBase.cs` (184 lines, baseline `d92d6aae`). Unit W8-13a.
//!
//! The part of the C-05 frame clock that is the same on every source: the "armed" state (a `Frame` subscriber or a pending `request_frame`),
//! at most one tick per `on_signaled`/`on_deadline`, and the `FrameTick` that is raised. The tick source (a waitable handle plus an optional
//! deadline) is a [`ClockPolicy`]: [`super::timer::TimerPolicy`] (13a) and `swap_chain_clock::SwapChainPolicy` (13b). `MessageLoop` waits only on
//! armed clocks, so no subscriber means no wake-up and no CPU.
//!
//! Rust shape (D-W8B3H-2): a clock is shared (the message loop polls it, windows and presenters subscribe), so [`FrameClockBase`] has `&self` methods
//! over interior mutability and is used as `Rc<dyn HostFrameClock>`; it is `!Send` (UI thread only). `FrameTick`, `FrameHandler` and
//! `FrameHandlerId` are re-exports of `pr_app::input::frame_clock` (amendment A10); the `impl pr_app::input::frame_clock::FrameClock for` a newtype
//! around `Rc<dyn HostFrameClock>` is still to be written by its unit.

use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;

use pr_core::abstractions::{DisplayClock, DisplayTiming};

use crate::host::error::HostError;

// `FrameTick`, `FrameHandler` and `FrameHandlerId` are the `pr_app` types (reconciliation amendment A10; they were field-for-field twins).
pub use pr_app::input::frame_clock::{FrameHandler, FrameHandlerId, FrameTick};

/// Clock seam for tests (C# `Func<long>? timestamp`): Stopwatch ticks, that is QueryPerformanceCounter ticks.
pub type TimestampFn = Box<dyn Fn() -> i64>;

/// `Stopwatch.Frequency`: delegates to `crate::display_clock::qpc_frequency()` (already `pub(crate)`: QueryPerformanceFrequency once, cached, 1e9 when the
/// call fails; do not write a second copy).
pub fn qpc_frequency() -> i64 {
    crate::display_clock::qpc_frequency()
}

/// `Stopwatch.GetTimestamp()`: delegates to `crate::display_clock::qpc_timestamp()`; the 13a porter changes that private `fn` to `pub(crate)` (one word, the
/// only edit outside the owned files).
pub fn qpc_now() -> i64 {
    crate::display_clock::qpc_timestamp()
}

/// C# `NextGridPoint(timing, at)`: the first vblank grid point `>= at`. `refresh_period <= 0` or `at <= last_vblank` gives `max(at, last_vblank)`;
/// else `last_vblank + ceil((at - last_vblank) / refresh_period) * refresh_period`. The arithmetic does not overflow (C# unchecked `long`): use
/// `i128` or `checked_*` with a saturating fallback (trap T7).
pub fn next_grid_point(timing: DisplayTiming, at: i64) -> i64 {
    if timing.refresh_period <= 0 || at <= timing.last_vblank {
        return at.max(timing.last_vblank);
    }
    let period = i128::from(timing.refresh_period);
    let last = i128::from(timing.last_vblank);
    let periods = (i128::from(at) - last + period - 1) / period;
    i64::try_from(last + periods * period).unwrap_or(i64::MAX)
}

/// `(frame_timestamp - epoch) as f64 * 1000.0 / frequency as f64` (C# `(frameTimestamp - _epoch) * 1000.0 / Stopwatch.Frequency`: the
/// subtraction is `long`, the rest `double`). Pure.
pub fn rendering_time_ms(frame_timestamp: i64, epoch: i64, frequency: i64) -> f64 {
    frame_timestamp.wrapping_sub(epoch) as f64 * 1000.0 / frequency as f64
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
        self.display_clock.and_then(|clock| clock.get_timing(self.window))
    }
}

/// The abstract members of C# `FrameClockBase`. A policy owns its state and is called by the base with the `RefCell` borrowed: it must not call
/// back into the clock (it cannot: it only receives a [`ClockEnv`]).
pub trait ClockPolicy {
    /// C# `WaitHandle`: the handle `MsgWaitForMultipleObjectsEx` waits on while armed; `0` = none.
    fn wait_handle(&self) -> isize;

    /// C# virtual `DeadlineTimestamp`: QPC ticks at which the loop must call `on_deadline` although the handle is not signaled; `i64::MAX` = none
    /// (the default). `armed` is the base's `IsArmed` (the swap-chain clock returns `i64::MAX` when not armed).
    fn deadline_timestamp(&self, _armed: bool) -> i64 {
        i64::MAX
    }

    /// C# `ShouldTick(now, signaled, out frameTimestamp)`: `Some(frame_timestamp)` = tick now. May have side effects (the timer clock re-arms
    /// itself on an early signal), hence `&mut self` and the `Result`.
    fn should_tick(&mut self, env: &ClockEnv<'_>, now: i64, signaled: bool) -> Result<Option<i64>, HostError>;

    /// C# `OnArmedChanged(armed, now)`: start or stop the tick source.
    fn on_armed_changed(&mut self, env: &ClockEnv<'_>, armed: bool, now: i64) -> Result<(), HostError>;

    /// C# virtual `OnTicked(frameTimestamp, now)`: after a tick while still armed, schedule the next frame. Default: nothing.
    fn on_ticked(&mut self, _env: &ClockEnv<'_>, _frame_timestamp: i64, _now: i64) -> Result<(), HostError> {
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
    /// Number of `try_tick` calls in progress (handlers may re-enter): removed slots are purged when it returns to 0.
    ticking: u32,
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
        let now: TimestampFn = timestamp.unwrap_or_else(|| Box::new(qpc_now));
        let epoch = now();
        Self {
            display_clock,
            window,
            now,
            epoch,
            state: RefCell::new(State {
                policy,
                slots: Vec::new(),
                next_id: 1,
                requested: false,
                armed: false,
                disposed: false,
                tick_count: 0,
                ticking: 0,
            }),
            _not_send: PhantomData,
        }
    }

    fn env(&self) -> ClockEnv<'_> {
        ClockEnv {
            display_clock: self.display_clock.as_deref(),
            window: self.window,
        }
    }

    /// `Result` form of `subscribe` (C# `Frame +=`).
    pub fn try_subscribe(&self, handler: FrameHandler) -> Result<FrameHandlerId, HostError> {
        let id = {
            let mut state = self.state.borrow_mut();
            let id = state.next_id;
            state.next_id += 1;
            state.slots.push(Slot {
                id,
                handler: Some(handler),
                removed: false,
            });
            id
        };
        self.update_armed()?;
        Ok(id)
    }

    /// `Result` form of `request_frame`.
    pub fn try_request_frame(&self) -> Result<(), HostError> {
        self.state.borrow_mut().requested = true;
        self.update_armed()
    }

    /// `Result` form of `unsubscribe`.
    pub fn try_unsubscribe(&self, id: FrameHandlerId) -> Result<(), HostError> {
        let removed = {
            let mut state = self.state.borrow_mut();
            let ticking = state.ticking > 0;
            match state.slots.iter().position(|slot| slot.id == id) {
                None => None,
                // During a tick the slot stays (snapshot semantics: later handlers of this tick still run); the tick purges it.
                Some(index) if ticking => {
                    state.slots[index].removed = true;
                    None
                }
                Some(index) => Some(state.slots.remove(index)),
            }
        };
        // Dropped outside the borrow: a handler's captured state may run arbitrary `Drop` code.
        drop(removed);
        self.update_armed()
    }

    /// Reads the policy (tests and the concrete clocks' accessors: `scheduled_frame`, `period`).
    pub fn with_policy<R>(&self, read: impl FnOnce(&P) -> R) -> R {
        read(&self.state.borrow().policy)
    }

    /// C# private `TryTick(signaled)`: the whole tick algorithm (see the contract, "Tick algorithm").
    fn try_tick(&self, signaled: bool) -> Result<bool, HostError> {
        if !self.state.borrow().armed {
            return Ok(false);
        }
        let now = (self.now)();
        let frame_timestamp = {
            let mut state = self.state.borrow_mut();
            match state.policy.should_tick(&self.env(), now, signaled)? {
                Some(frame_timestamp) => frame_timestamp,
                None => return Ok(false),
            }
        };
        let ids = {
            let mut state = self.state.borrow_mut();
            state.requested = false;
            state.tick_count += 1;
            state.ticking += 1;
            state
                .slots
                .iter()
                .filter(|slot| !slot.removed)
                .map(|slot| slot.id)
                .collect::<Vec<_>>()
        };
        let tick = FrameTick {
            timestamp: frame_timestamp,
            rendering_time_ms: rendering_time_ms(frame_timestamp, self.epoch, qpc_frequency()),
            timing: self.env().current_timing(),
        };
        let mut panic_payload = None;
        for id in ids {
            let handler = {
                let mut state = self.state.borrow_mut();
                state
                    .slots
                    .iter_mut()
                    .find(|slot| slot.id == id)
                    .and_then(|slot| slot.handler.take())
            };
            let Some(mut handler) = handler else { continue };
            let outcome = catch_unwind(AssertUnwindSafe(|| handler(&tick)));
            let leftover = {
                let mut state = self.state.borrow_mut();
                match state.slots.iter_mut().find(|slot| slot.id == id) {
                    Some(slot) if !slot.removed => {
                        slot.handler = Some(handler);
                        None
                    }
                    _ => Some(handler),
                }
            };
            drop(leftover);
            if let Err(payload) = outcome {
                panic_payload = Some(payload);
                break;
            }
        }
        let purged = {
            let mut state = self.state.borrow_mut();
            state.ticking -= 1;
            if state.ticking == 0 {
                let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut state.slots)
                    .into_iter()
                    .partition(|slot| slot.removed);
                state.slots = kept;
                gone
            } else {
                Vec::new()
            }
        };
        drop(purged);
        // C# `finally`: runs whether or not a handler threw.
        self.update_armed()?;
        {
            let mut state = self.state.borrow_mut();
            if state.armed {
                let after = (self.now)();
                state.policy.on_ticked(&self.env(), frame_timestamp, after)?;
            }
        }
        if let Some(payload) = panic_payload {
            resume_unwind(payload);
        }
        Ok(true)
    }

    /// C# private `UpdateArmed()`: `armed = !disposed && (has live handlers || requested)`; on a change call `policy.on_armed_changed`.
    fn update_armed(&self) -> Result<(), HostError> {
        let mut state = self.state.borrow_mut();
        let armed = !state.disposed && (state.slots.iter().any(|slot| !slot.removed) || state.requested);
        if armed == state.armed {
            return Ok(());
        }
        state.armed = armed;
        let now = (self.now)();
        state.policy.on_armed_changed(&self.env(), armed, now)
    }
}

/// D-W8B3H-2: the C# `Win32Exception` escapes `Frame +=` / `RequestFrame()` to the caller; the infallible trait methods panic with its text.
#[allow(clippy::panic)]
fn expect_ok<T>(result: Result<T, HostError>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{error}"),
    }
}

impl<P: ClockPolicy> HostFrameClock for FrameClockBase<P> {
    fn subscribe(&self, handler: FrameHandler) -> FrameHandlerId {
        expect_ok(self.try_subscribe(handler))
    }
    fn unsubscribe(&self, id: FrameHandlerId) {
        expect_ok(self.try_unsubscribe(id));
    }
    fn request_frame(&self) {
        expect_ok(self.try_request_frame());
    }
    fn is_armed(&self) -> bool {
        self.state.borrow().armed
    }
    fn tick_count(&self) -> i64 {
        self.state.borrow().tick_count
    }
    fn wait_handle(&self) -> isize {
        let state = self.state.borrow();
        if state.disposed { 0 } else { state.policy.wait_handle() }
    }
    fn deadline_timestamp(&self) -> i64 {
        let state = self.state.borrow();
        state.policy.deadline_timestamp(state.armed)
    }
    fn ticks_until_deadline(&self) -> i64 {
        let deadline = self.deadline_timestamp();
        if deadline == i64::MAX {
            i64::MAX
        } else {
            deadline.saturating_sub((self.now)())
        }
    }
    fn on_signaled(&self) -> Result<bool, HostError> {
        self.try_tick(true)
    }
    fn on_deadline(&self) -> Result<bool, HostError> {
        self.try_tick(false)
    }
    fn dispose(&self) {
        let slots = {
            let mut state = self.state.borrow_mut();
            if state.disposed {
                return;
            }
            state.disposed = true;
            state.requested = false;
            std::mem::take(&mut state.slots)
        };
        drop(slots);
        // Disarming only cancels the source; a cancel failure was ignored in C# too.
        let _ = self.update_armed();
        self.state.borrow_mut().policy.dispose_core();
    }
}
