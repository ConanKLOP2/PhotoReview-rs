//! Port of `PhotoReview.Shell.Win32/Viewing/KeepWarm.cs` (138 lines), `ShellMainWindow.KeepWarm.cs` (39 lines) and `ShellViewingRegistration.cs`
//! (24 lines, retired: DI registration), baseline `d92d6aae`. Unit W8-16d (contract `contracts/W8-16d.md`).
//!
//! WP21-STALL: decides WHEN the GPU is kept warm. Every user interaction while the window is ACTIVE (key, mouse, wheel, touch, window activation)
//! extends the warm window by `KeepWarmOptions::window_ms`; nothing runs without interaction. The thread that does the work is
//! `render::keep_alive::GpuKeepAlive` (W8-12h); this file is its policy ([`KeepWarmController`], a window message handler that only LISTENS and never
//! claims a message), its configuration ([`KeepWarmOptions`], the `PHOTOREVIEW_RS_GPU_KEEPWARM*` switches, D-W8B3S-7) and the window glue
//! ([`register_keep_warm`] / [`KeepWarmSetup::dispose`], the C# `RegisterKeepWarm` / `DisposeKeepWarm`).
//!
//! Ticks are the nanoseconds of `render::keep_alive::monotonic_ticks` (D-W8B3S-8): the same clock as `GpuKeepAlive`, never `Stopwatch`/QPC units.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use super::frame_log::FrameLog;
use crate::host::contracts::{ShellWindowApi, WindowMessage, WindowMessageHandler};
use crate::host::shell_window::ShellWindow;
use crate::render::keep_alive::{
    GpuKeepAlive, KeepAliveOptions, OpenKeepAliveWork, TickClock, monotonic_ticks, open_offscreen_hardware_work,
    ticks_from_ms,
};
use crate::win32::messages::*;
use crate::win32::structs::Lresult;

/// `PHOTOREVIEW_RS_GPU_KEEPWARM` (C# `PHOTOREVIEW_GPU_KEEPWARM`, R0002): `0`, `off`, `false`, `no` (any case, trimmed) switch it off.
pub const ENABLED_VARIABLE: &str = "PHOTOREVIEW_RS_GPU_KEEPWARM";
/// `PHOTOREVIEW_RS_GPU_KEEPWARM_MS`: warm window after the last interaction, clamped to `MIN_WINDOW_MS..=MAX_WINDOW_MS`.
pub const WINDOW_VARIABLE: &str = "PHOTOREVIEW_RS_GPU_KEEPWARM_MS";
/// `PHOTOREVIEW_RS_GPU_KEEPWARM_INTERVAL_MS`: gap between two submissions, clamped to `MIN_INTERVAL_MS..=MAX_INTERVAL_MS`.
pub const INTERVAL_VARIABLE: &str = "PHOTOREVIEW_RS_GPU_KEEPWARM_INTERVAL_MS";
pub const DEFAULT_WINDOW_MS: i32 = 5000;
pub const DEFAULT_INTERVAL_MS: i32 = 20;
pub const MIN_WINDOW_MS: i32 = 250;
pub const MAX_WINDOW_MS: i32 = 60_000;
pub const MIN_INTERVAL_MS: i32 = 5;
pub const MAX_INTERVAL_MS: i32 = 100;

/// C# `KeepWarmController.ThrottleMs`: two extensions are at least this far apart (a 1 kHz mouse costs one extension per 100 ms).
pub const THROTTLE_MS: i64 = 100;

/// `WM_TOUCH`, `WM_POINTERUPDATE`, `WM_POINTERDOWN` (the C# private constants; not in `win32::messages`).
pub const WM_TOUCH: u32 = 0x0240;
pub const WM_POINTER_UPDATE: u32 = 0x0245;
pub const WM_POINTER_DOWN: u32 = 0x0246;

/// C# `KeepWarmOptions(Enabled = true, WindowMs = 5000, IntervalMs = 20)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepWarmOptions {
    pub enabled: bool,
    pub window_ms: i32,
    pub interval_ms: i32,
}

impl Default for KeepWarmOptions {
    /// Enabled, 5000 ms, 20 ms.
    fn default() -> Self {
        Self {
            enabled: true,
            window_ms: DEFAULT_WINDOW_MS,
            interval_ms: DEFAULT_INTERVAL_MS,
        }
    }
}

impl KeepWarmOptions {
    /// C# `FromEnvironment(get)` with the three variables above; `get` is the environment reader (tests inject a map; production passes
    /// `|name| std::env::var(name).ok()`). Enabled variable: present and (trimmed) `0` / `off` / `false` / `no` (ASCII case-insensitive) = disabled, any other
    /// value (also empty) = enabled, absent = enabled. The two numbers: parsed like .NET `int.TryParse(NumberStyles.Integer, InvariantCulture)`
    /// (ASCII whitespace and one sign allowed, anything else or an overflow = the default), then clamped.
    pub fn from_environment(get: &dyn Fn(&str) -> Option<String>) -> KeepWarmOptions {
        let enabled = get(ENABLED_VARIABLE).is_none_or(|raw| {
            let value = raw.trim();
            !(value == "0" || ["off", "false", "no"].iter().any(|off| value.eq_ignore_ascii_case(off)))
        });
        fn read_int(get: &dyn Fn(&str) -> Option<String>, name: &str, fallback: i32, min: i32, max: i32) -> i32 {
            get(name)
                .and_then(|raw| {
                    raw.trim_matches(|ch| matches!(ch, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r'))
                        .parse::<i32>()
                        .ok()
                })
                .map_or(fallback, |value| value.clamp(min, max))
        }
        Self {
            enabled,
            window_ms: read_int(get, WINDOW_VARIABLE, DEFAULT_WINDOW_MS, MIN_WINDOW_MS, MAX_WINDOW_MS),
            interval_ms: read_int(
                get,
                INTERVAL_VARIABLE,
                DEFAULT_INTERVAL_MS,
                MIN_INTERVAL_MS,
                MAX_INTERVAL_MS,
            ),
        }
    }
}

/// C# `KeepWarmController.IsInteraction(message)`: `WM_KEYDOWN`, `WM_SYSKEYDOWN`, `WM_MOUSEMOVE`, `WM_LBUTTONDOWN`, `WM_RBUTTONDOWN`, `WM_MBUTTONDOWN`,
/// `WM_XBUTTONDOWN`, `WM_MOUSEWHEEL`, `WM_MOUSEHWHEEL`, `WM_ACTIVATE`, `WM_TOUCH`, `WM_POINTERUPDATE`, `WM_POINTERDOWN`.
pub fn is_interaction(message: u32) -> bool {
    matches!(
        message,
        WM_KEYDOWN
            | WM_SYSKEYDOWN
            | WM_MOUSEMOVE
            | WM_LBUTTONDOWN
            | WM_RBUTTONDOWN
            | WM_MBUTTONDOWN
            | WM_XBUTTONDOWN
            | WM_MOUSEWHEEL
            | WM_MOUSEHWHEEL
            | WM_ACTIVATE
            | WM_TOUCH
            | WM_POINTER_UPDATE
            | WM_POINTER_DOWN
    )
}

/// C# `KeepWarmController` (a message handler registered FIRST in the chain; it always answers "not handled").
pub struct KeepWarmController {
    options: KeepWarmOptions,
    is_window_active: Box<dyn Fn() -> bool>,
    activate: Box<dyn Fn(i64)>,
    timestamp: Box<dyn Fn() -> i64>,
    last_activation: Cell<i64>,
    has_activation: Cell<bool>,
    activations: Cell<i64>,
    launch_warmups: Cell<i64>,
}

impl std::fmt::Debug for KeepWarmController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeepWarmController")
            .field("options", &self.options)
            .field("activations", &self.activations.get())
            .field("launch_warmups", &self.launch_warmups.get())
            .finish_non_exhaustive()
    }
}

impl KeepWarmController {
    /// C# constructor `(options, isWindowActive, activate, timestamp)`; `timestamp` `None` = `render::keep_alive::monotonic_ticks`.
    /// `activate(until_ticks)` is `GpuKeepAlive::activate`.
    pub fn new(
        options: KeepWarmOptions,
        is_window_active: Box<dyn Fn() -> bool>,
        activate: Box<dyn Fn(i64)>,
        timestamp: Option<Box<dyn Fn() -> i64>>,
    ) -> Self {
        Self {
            options,
            is_window_active,
            activate,
            timestamp: timestamp.unwrap_or_else(|| Box::new(monotonic_ticks)),
            last_activation: Cell::new(0),
            has_activation: Cell::new(false),
            activations: Cell::new(0),
            launch_warmups: Cell::new(0),
        }
    }

    /// C# `Activations`: how many times the warm window was extended by an interaction (diagnostics, tests).
    pub fn activations(&self) -> i64 {
        self.activations.get()
    }

    /// C# `LaunchWarmups`: how many launch warm-ups ran (not counted in `activations`).
    pub fn launch_warmups(&self) -> i64 {
        self.launch_warmups.get()
    }

    /// C# `WarmAtLaunch()`: warm as soon as the first image was presented, without waiting for an interaction (perf gate 3). Disabled = nothing.
    /// Otherwise `launch_warmups += 1` and `activate(timestamp() + ticks_from_ms(window_ms))` once; it does not touch `activations` or the throttle.
    pub fn warm_at_launch(&self) {
        if !self.options.enabled {
            return;
        }
        self.launch_warmups.set(self.launch_warmups.get().saturating_add(1));
        (self.activate)((self.timestamp)().saturating_add(ticks_from_ms(i64::from(self.options.window_ms))));
    }

    /// C# `NoteInteraction()`: disabled or the window not active = nothing (and the throttle is NOT consumed). Otherwise, unless an extension was made
    /// less than `THROTTLE_MS` ago: remember `now`, `activations += 1`, `activate(now + ticks_from_ms(window_ms))`.
    pub fn note_interaction(&self) {
        if !self.options.enabled || !(self.is_window_active)() {
            return;
        }
        let now = (self.timestamp)();
        if self.has_activation.get() && now.saturating_sub(self.last_activation.get()) < ticks_from_ms(THROTTLE_MS) {
            return;
        }
        self.last_activation.set(now);
        self.has_activation.set(true);
        self.activations.set(self.activations.get().saturating_add(1));
        (self.activate)(now.saturating_add(ticks_from_ms(i64::from(self.options.window_ms))));
    }
}

impl WindowMessageHandler for KeepWarmController {
    /// C# `TryHandle`: an interaction message calls `note_interaction`; the answer is ALWAYS `None` (input behaviour is untouched).
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        if is_interaction(message.msg) {
            self.note_interaction();
        }
        None
    }
}

/// The two window members the glue needs (`ShellWindow` implements it; tests use a fake).
pub trait KeepWarmWindow {
    /// C# `IShellWindow.IsActive`.
    fn is_active(&self) -> bool;

    /// C# `InsertMessageHandlerFirst`.
    fn insert_message_handler_first(&self, handler: Rc<dyn WindowMessageHandler>);
}

impl KeepWarmWindow for ShellWindow {
    fn is_active(&self) -> bool {
        ShellWindowApi::is_active(self)
    }

    fn insert_message_handler_first(&self, handler: Rc<dyn WindowMessageHandler>) {
        ShellWindow::insert_message_handler_first(self, handler);
    }
}

/// The `ShellMainWindowOptions` members `RegisterKeepWarm` reads (C# `FrameLogPath`, `KeepWarm`, `PreferWarp`, `KeepWarmWork`, `KeepWarmTimestamp`).
/// The main window (13g) builds it from its own options.
pub struct KeepWarmRegistration {
    /// `PHOTOREVIEW_RS_FRAME_LOG` override: a path here wins over the environment.
    pub frame_log_path: Option<PathBuf>,
    /// `None` = `KeepWarmOptions::from_environment` on the process environment.
    pub keep_warm: Option<KeepWarmOptions>,
    /// The surface is WARP (no GPU to keep warm).
    pub prefer_warp: bool,
    /// Replaces the hardware work (tests); `None` = `render::keep_alive::open_offscreen_hardware_work`.
    pub work: Option<OpenKeepAliveWork>,
    /// The tick clock shared by the keep-alive thread and the controller; `None` = `monotonic_ticks`.
    pub timestamp: Option<TickClock>,
}

impl std::fmt::Debug for KeepWarmRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeepWarmRegistration")
            .field("frame_log_path", &self.frame_log_path)
            .field("keep_warm", &self.keep_warm)
            .field("prefer_warp", &self.prefer_warp)
            .finish_non_exhaustive()
    }
}

/// What `register_keep_warm` created (C# fields `_keepAlive`, `_keepWarm`, `_frameLog` of `ShellMainWindow`).
pub struct KeepWarmSetup {
    /// `None` when disabled or WARP (and no injected work).
    pub keep_alive: Option<Arc<GpuKeepAlive>>,
    pub controller: Option<Rc<KeepWarmController>>,
    /// `None` when `PHOTOREVIEW_RS_FRAME_LOG` is unset and no path was given.
    pub frame_log: Option<FrameLog>,
}

impl std::fmt::Debug for KeepWarmSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeepWarmSetup")
            .field("keep_alive", &self.keep_alive.is_some())
            .field("controller", &self.controller.is_some())
            .field("frame_log", &self.frame_log.is_some())
            .finish()
    }
}

impl KeepWarmSetup {
    /// C# `DisposeKeepWarm()`: disposes the keep-alive thread (joins it) and closes the frame log; idempotent.
    pub fn dispose(&mut self) {
        if let Some(keep_alive) = self.keep_alive.take() {
            keep_alive.dispose();
        }
        self.frame_log.take();
    }
}

/// C# `RegisterKeepWarm()`: opens the frame log (path from the registration, else `FrameLog::from_environment`); resolves the options; when they are
/// disabled, or the surface is WARP and no work was injected, creates NOTHING (no thread, no device, no handler). Otherwise creates the
/// `GpuKeepAlive` (interval = `options.interval_ms`, work = the injected one or the hardware work) and the controller (active = `window.is_active`,
/// activate = `keep_alive.activate`) and registers the controller first in the window's handler chain.
pub fn register_keep_warm(window: &Rc<dyn KeepWarmWindow>, registration: KeepWarmRegistration) -> KeepWarmSetup {
    let KeepWarmRegistration {
        frame_log_path,
        keep_warm,
        prefer_warp,
        work,
        timestamp,
    } = registration;
    let frequency = crate::display_clock::qpc_frequency();
    let frame_log = match frame_log_path.filter(|path| !path.as_os_str().is_empty()) {
        Some(path) => FrameLog::from_environment(|_| Some(path.to_string_lossy().into_owned()), frequency),
        None => FrameLog::from_environment(|name| std::env::var(name).ok(), frequency),
    };
    let options = keep_warm.unwrap_or_else(|| KeepWarmOptions::from_environment(&|name| std::env::var(name).ok()));
    if !options.enabled || (prefer_warp && work.is_none()) {
        return KeepWarmSetup {
            keep_alive: None,
            controller: None,
            frame_log,
        };
    }
    let clock = timestamp.unwrap_or_else(|| Arc::new(monotonic_ticks));
    let keep_alive = Arc::new(GpuKeepAlive::new(
        options.interval_ms,
        work.unwrap_or_else(|| Box::new(open_offscreen_hardware_work)),
        KeepAliveOptions {
            timestamp: Some(clock.clone()),
            below_normal_priority: true,
        },
    ));
    let weak_window: Weak<dyn KeepWarmWindow> = Rc::downgrade(window);
    let active = Box::new(move || weak_window.upgrade().is_some_and(|window| window.is_active()));
    let activate_keep_alive = keep_alive.clone();
    let controller = Rc::new(KeepWarmController::new(
        options,
        active,
        Box::new(move |until| activate_keep_alive.activate(until)),
        Some(Box::new(move || clock())),
    ));
    window.insert_message_handler_first(controller.clone());
    KeepWarmSetup {
        keep_alive: Some(keep_alive),
        controller: Some(controller),
        frame_log,
    }
}
