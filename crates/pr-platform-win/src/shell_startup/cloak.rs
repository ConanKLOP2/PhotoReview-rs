//! Port of `PhotoReview.Shell.Win32/Startup/StartupCloak.cs` (100 lines, baseline `d92d6aae`) and `PhotoReview.Platform.Windows/WindowsDwmCloak.cs` (the `TrySetCloaked`
//! default). Unit W8-17b. Contract: `contracts/W8-17b.md`.
//!
//! No white frame at start-up: the window is cloaked (DWM) as soon as its HWND exists, BEFORE `ShowWindow`, and is un-cloaked after the FIRST frame has been
//! presented (`note_first_present`). The cloak slows nothing else: the window is shown, activated, laid out and painted as usual. `reveal` is idempotent and also
//! runs at a safety deadline ([`SAFETY_TIMEOUT`]) if no frame ever comes; `stop` (the window closes) cancels the deadline. A DWM that refuses the cloak changes
//! nothing (the window just shows). State is changed on the UI thread only; the deadline thread only posts a job to the UI dispatcher.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use pr_core::abstractions::{UiAction, UiScheduler};
use pr_core::instance::coalescer::WindowCancel;

use crate::host::main_window::core::StartupCloakPort;
use crate::win32::api::dwmapi;
use crate::win32::structs::Hwnd;

/// C# `StartupCloak.SafetyTimeout`: the longest a window stays cloaked when no frame is presented.
pub const SAFETY_TIMEOUT: Duration = Duration::from_secs(3);

/// C# `Func<nint, bool, bool> setCloaked`: `(hwnd, cloaked)` -> DWM accepted it.
pub type SetCloakedFn = Arc<dyn Fn(Hwnd, bool) -> bool + Send + Sync>;

/// C# `Action? onRevealed`.
pub type RevealedFn = Arc<dyn Fn() + Send + Sync>;

/// A one-shot timer; DROPPING the handle cancels it (C# `IDisposable.Dispose`).
pub trait TimerHandle: Send {}

/// C# `Func<TimeSpan, Action, IDisposable> scheduleTimeout`: `(delay, job)` -> the handle. The job runs once on ANOTHER thread, unless the handle was dropped first.
pub type ScheduleTimeoutFn = Arc<dyn Fn(Duration, Box<dyn FnOnce() + Send>) -> Box<dyn TimerHandle> + Send + Sync>;

/// C# `WindowsDwmCloak.TrySetCloaked(handle, cloaked)`: `false` for handle 0; `dwmapi::dwm_set_window_attribute(hwnd, DWMWA_CLOAK, &(1 or 0 as i32))` with `is_ok()`.
pub fn try_set_cloaked(hwnd: Hwnd, cloaked: bool) -> bool {
    if hwnd == 0 {
        return false;
    }
    let value: i32 = if cloaked { 1 } else { 0 };
    dwmapi::dwm_set_window_attribute(hwnd, dwmapi::DWMWA_CLOAK, &value).is_ok()
}

/// The production [`ScheduleTimeoutFn`]: one named thread that waits on a `pr_core::instance::coalescer::WindowCancel` for the delay and runs the job unless
/// cancelled (C# `System.Threading.Timer`).
pub fn schedule_on_thread(delay: Duration, job: Box<dyn FnOnce() + Send>) -> Box<dyn TimerHandle> {
    struct DeadlineHandle(WindowCancel);
    impl TimerHandle for DeadlineHandle {}
    impl Drop for DeadlineHandle {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }

    let cancel = WindowCancel::new();
    let thread_cancel = cancel.clone();
    let spawned = std::thread::Builder::new()
        .name("startup-cloak-deadline".to_owned())
        .spawn(move || {
            if !thread_cancel.wait_timeout(delay) {
                job();
            }
        });
    if spawned.is_err() {
        cancel.cancel();
    }
    Box::new(DeadlineHandle(cancel))
}

/// The optional constructor arguments of the C# `StartupCloak` (`None` = the C# default).
#[derive(Default)]
pub struct StartupCloakOptions {
    /// C# `onRevealed`.
    pub on_revealed: Option<RevealedFn>,
    /// C# `setCloaked`; `None` = [`try_set_cloaked`].
    pub set_cloaked: Option<SetCloakedFn>,
    /// C# `safetyTimeout`; `None` = [`SAFETY_TIMEOUT`].
    pub safety_timeout: Option<Duration>,
    /// C# `scheduleTimeout`; `None` = [`schedule_on_thread`].
    pub schedule_timeout: Option<ScheduleTimeoutFn>,
}

impl std::fmt::Debug for StartupCloakOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartupCloakOptions").finish_non_exhaustive()
    }
}

/// The mutable state of the cloak (C# `_handle`, `_cloaked`, `_revealed`, `_timer`).
struct CloakState {
    handle: Hwnd,
    cloaked: bool,
    revealed: bool,
    timer: Option<Box<dyn TimerHandle>>,
}

/// The shared part: the deadline job holds an `Arc` of it (the job must be `Send`).
struct CloakInner {
    state: Mutex<CloakState>,
    dispatcher: Arc<dyn UiScheduler>,
    on_revealed: Option<RevealedFn>,
    set_cloaked: SetCloakedFn,
    safety_timeout: Duration,
    schedule_timeout: ScheduleTimeoutFn,
}

/// C# `StartupCloak`. `Drop` = C# `Dispose` = [`stop`](Self::stop).
pub struct StartupCloak {
    inner: Arc<CloakInner>,
}

impl std::fmt::Debug for StartupCloak {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartupCloak").finish_non_exhaustive()
    }
}

impl StartupCloak {
    /// C# constructor. `dispatcher` receives the safety-deadline reveal.
    pub fn new(dispatcher: Arc<dyn UiScheduler>, options: StartupCloakOptions) -> Self {
        let set_cloaked = options.set_cloaked.unwrap_or_else(|| Arc::new(try_set_cloaked));
        let safety_timeout = options.safety_timeout.unwrap_or(SAFETY_TIMEOUT);
        let schedule_timeout = options.schedule_timeout.unwrap_or_else(|| Arc::new(schedule_on_thread));
        Self {
            inner: Arc::new(CloakInner {
                state: Mutex::new(CloakState {
                    handle: 0,
                    cloaked: false,
                    revealed: false,
                    timer: None,
                }),
                dispatcher,
                on_revealed: options.on_revealed,
                set_cloaked,
                safety_timeout,
                schedule_timeout,
            }),
        }
    }

    /// C# `IsHidden`: cloaked and not revealed.
    pub fn is_hidden(&self) -> bool {
        let state = self.inner.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.cloaked && !state.revealed
    }

    /// C# `IsRevealed`.
    pub fn is_revealed(&self) -> bool {
        self.inner.state.lock().unwrap_or_else(PoisonError::into_inner).revealed
    }

    /// C# `Hide(hwnd)`: cloaks `hwnd` unless this gate was already used (one window, one cloak) or `hwnd` is 0; arms the safety deadline when DWM accepted.
    pub fn hide(&self, hwnd: Hwnd) {
        {
            let mut state = self.inner.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.revealed || state.cloaked || hwnd == 0 {
                return;
            }
            state.handle = hwnd;
        }

        let cloaked = catch_unwind(AssertUnwindSafe(|| (self.inner.set_cloaked)(hwnd, true))).unwrap_or(false);
        {
            let mut state = self.inner.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.cloaked = cloaked;
        }
        if !cloaked {
            return;
        }

        let inner = Arc::clone(&self.inner);
        let dispatcher = Arc::clone(&self.inner.dispatcher);
        let timer = catch_unwind(AssertUnwindSafe(|| {
            (self.inner.schedule_timeout)(
                self.inner.safety_timeout,
                Box::new(move || {
                    let inner = Arc::clone(&inner);
                    let action: UiAction = Box::new(move || reveal_inner(&inner));
                    dispatcher.post(action);
                }),
            )
        }))
        .ok();
        if let Some(timer) = timer {
            let mut state = self.inner.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.revealed {
                drop(state);
                drop(timer);
            } else {
                state.timer = Some(timer);
            }
        }
    }

    /// C# `NoteFirstPresent()` = [`reveal`](Self::reveal).
    pub fn note_first_present(&self) {
        self.reveal();
    }

    /// C# `Reveal()`: stops the deadline, then un-cloaks once; nothing when it never cloaked; calls `on_revealed` after the un-cloak.
    pub fn reveal(&self) {
        reveal_inner(&self.inner);
    }

    /// C# `Stop()`: cancels the deadline and keeps the cloak state (the window is closing).
    pub fn stop(&self) {
        let timer = {
            let mut state = self.inner.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.timer.take()
        };
        drop(timer);
    }
}

impl Drop for StartupCloak {
    /// C# `Dispose()`.
    fn drop(&mut self) {
        self.stop();
    }
}

/// The two members `host::main_window` calls (D-W8B4M-4).
impl StartupCloakPort for StartupCloak {
    fn stop(&self) {
        StartupCloak::stop(self)
    }

    fn note_first_present(&self) {
        StartupCloak::note_first_present(self)
    }
}

fn reveal_inner(inner: &Arc<CloakInner>) {
    let timer = {
        let mut state = inner.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.timer.take()
    };
    drop(timer);

    let handle = {
        let mut state = inner.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.revealed {
            return;
        }
        state.revealed = true;
        if !state.cloaked {
            return;
        }
        state.handle
    };

    let _ = catch_unwind(AssertUnwindSafe(|| (inner.set_cloaked)(handle, false)));
    if let Some(on_revealed) = &inner.on_revealed {
        let _ = catch_unwind(AssertUnwindSafe(|| on_revealed()));
    }
}
