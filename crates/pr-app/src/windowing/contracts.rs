//! Placement and fullscreen contracts (C-14): the data shape of `window-placement.json`, the pure show-command rules and the
//! seams the window host implements.
//!
//! Port of `PhotoReview.App.Shared/Windowing/WindowingContracts.cs` (baseline d92d6aae). Contract:
//! `contracts/W8-03a.md`. No `windows` crate and no `unsafe`: a window or monitor handle is a plain `isize`.
//!
//! `IWindowPlacementStore` is [`WindowPlacementStore`] (implemented by `placement_store::JsonWindowPlacementStore`),
//! `IFullscreenController` is not a trait here (lead decision D-W8B1-5): W8-03b's `FullscreenController` takes a `WindowOps` on every call,
//! `WindowPlacementRules` is the three free functions below.

use std::io;
use std::thread::JoinHandle;

/// A native window handle (`nint` in C#, `HWND` in Win32). Zero means "no window yet".
pub type WindowHandle = isize;

/// A native monitor handle (`HMONITOR`). Zero means "unknown".
pub type MonitorHandle = isize;

/// `SW_SHOWNORMAL`.
pub const SHOW_NORMAL: i32 = 1;

/// `SW_SHOWMAXIMIZED`. Also the default `ShowCommand` of a placement file that lacks the property.
pub const SHOW_MAXIMIZED: i32 = 3;

/// `WindowShowState`: the three window states the app distinguishes. Discriminants equal the C# enum values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowShowState {
    Normal = 0,
    Minimized = 1,
    Maximized = 2,
}

impl WindowShowState {
    /// The name C# `ToString()` gives (`"Normal"`, `"Minimized"`, `"Maximized"`); used by the goldens.
    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Minimized => "Minimized",
            Self::Maximized => "Maximized",
        }
    }

    /// `None` for a value that is not one of the three members (C# can hold such a value in an enum; Rust cannot).
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Normal),
            1 => Some(Self::Minimized),
            2 => Some(Self::Maximized),
            _ => None,
        }
    }
}

/// `WindowPlacementData`: the flat form of Win32 `WINDOWPLACEMENT` that is persisted. Field order is the C# constructor order.
/// There is no `Default`: a zero `show_command` is not the file default (3), so callers write every field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowPlacementData {
    pub length: i32,
    pub flags: i32,
    pub show_command: i32,
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub normal_left: i32,
    pub normal_top: i32,
    pub normal_right: i32,
    pub normal_bottom: i32,
}

/// `WindowPlacementRules.NormalizeShowCommand`: only "maximized" (3) survives; every other value (0, 1, 2, 7, negatives...) is
/// "normal" (1). R2-F-26: a minimized or hidden show command from a damaged file must never reopen a hidden window.
pub fn normalize_show_command(show_command: i32) -> i32 {
    if show_command == SHOW_MAXIMIZED {
        SHOW_MAXIMIZED
    } else {
        SHOW_NORMAL
    }
}

/// `WindowPlacementRules.ResolveShowCommand`: `Some(Maximized)` gives 3; `Some(Normal)` and `Some(Minimized)` give 1; `None`
/// gives [`normalize_show_command`] of `show_command`. R7-10: closing while in fullscreen saves the state from BEFORE fullscreen.
pub fn resolve_show_command(show_command: i32, fullscreen_restore_state: Option<WindowShowState>) -> i32 {
    match fullscreen_restore_state {
        Some(WindowShowState::Maximized) => SHOW_MAXIMIZED,
        Some(WindowShowState::Normal | WindowShowState::Minimized) => SHOW_NORMAL,
        None => normalize_show_command(show_command),
    }
}

/// `WindowPlacementRules.PlanStateBeforeShow`: `Maximized` when the normalized command is 3, otherwise `Normal`
/// (never `Minimized`).
pub fn plan_state_before_show(saved_show_command: i32) -> WindowShowState {
    if normalize_show_command(saved_show_command) == SHOW_MAXIMIZED {
        WindowShowState::Maximized
    } else {
        WindowShowState::Normal
    }
}

/// The result of [`WindowPlacementStore::prefetch`]: C# returns a `Task<WindowPlacementData?>` started on the thread pool.
/// Rust runs the read on a plain named thread (decision in the contract, open question 2) and exposes only `join`.
#[derive(Debug)]
pub struct PrefetchHandle {
    /// `None` when the thread could not be started: `join` then answers `None` and `take_prefetched` finds nothing.
    pub(crate) handle: Option<JoinHandle<Option<WindowPlacementData>>>,
}

impl PrefetchHandle {
    /// Waits for the read. `None` when the file was missing, damaged, unreadable or the thread panicked
    /// (the C# task result is `null` for the first three; a panic cannot happen in C#).
    pub fn join(self) -> Option<WindowPlacementData> {
        // A panicked worker gives `None` (the caller reads the file itself).
        self.handle?.join().ok().flatten()
    }
}

/// `IWindowPlacementStore`. Paths are the strings the C# API takes (D1); the implementation turns them into `&Path` for the
/// file system. All methods are callable from any thread.
pub trait WindowPlacementStore: Send + Sync {
    /// Starts reading `placement_path` on another thread NOW and remembers the outcome for [`take_prefetched`]. The slot is
    /// registered before this returns. A later prefetch replaces the earlier slot. The value is NOT filtered by visibility.
    ///
    /// [`take_prefetched`]: WindowPlacementStore::take_prefetched
    fn prefetch(&self, placement_path: &str) -> PrefetchHandle;

    /// The saved placement, or `None` when the file is missing, damaged, unreadable, or not visible on any monitor
    /// (see `visibility::is_visible`).
    fn read(&self, placement_path: &str) -> Option<WindowPlacementData>;

    /// Writes the placement atomically (unique temp name, then replace), creating the folder when it is missing.
    fn save(&self, placement_path: &str, data: &WindowPlacementData) -> io::Result<()>;

    /// The prefetched placement of `placement_path` (compared OrdinalIgnoreCase), at most once; `None` when nothing was
    /// prefetched for that path, the read has not finished or failed. A different path leaves the slot untouched.
    fn take_prefetched(&self, placement_path: &str) -> Option<WindowPlacementData>;
}
