//! Port of `PhotoReview.Shell.Win32/Startup/ShellPlacement.cs` (130 lines, baseline `d92d6aae`): `IWindowPlacementNative` + `NativeWindowPlacement` (lines 7-48) and
//! `ShellPlacement` (lines 50-130). Unit W8-17b. Contract: `contracts/W8-17b.md`.
//!
//! The Win32 side of `WindowPlacementService.RestoreBeforeShow` / `Save` (C-14; the rules are `pr_app::windowing::contracts`): the placement saved at close is applied to a
//! window that already has its HWND but is not shown yet, then the caller shows it straight into the right place and state (no 1200x800 window that jumps). A
//! placement visible on no monitor (the monitor was unplugged): a window closed Maximized still opens Maximized (on the monitor Windows chooses), a Normal one keeps
//! the default size.
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::windowing::contracts::{
    WindowPlacementData, WindowPlacementStore, WindowShowState, plan_state_before_show, resolve_show_command,
};
use pr_core::abstractions::Log;

use crate::host::main_window::core::PlacementSavePort;
use crate::win32::api::user32;
use crate::win32::messages::{SW_HIDE, SW_SHOWMAXIMIZED, SW_SHOWNOACTIVATE, SW_SHOWNORMAL};
use crate::win32::structs::Hwnd;
use crate::win32::structs::{Point, Rect, WindowPlacement, new_window_placement};
use windows::Win32::UI::WindowsAndMessaging::WINDOWPLACEMENT_FLAGS;

/// C# `IWindowPlacementNative` (production = user32; tests use a recording fake).
pub trait WindowPlacementNative {
    /// `SetWindowPlacement` with `showCmd = SW_HIDE`: the Normal position is set, the window is NOT shown.
    fn set_hidden(&self, hwnd: Hwnd, data: &WindowPlacementData) -> bool;
    /// `GetWindowPlacement`; `None` when the call fails.
    fn try_get(&self, hwnd: Hwnd) -> Option<WindowPlacementData>;
}

/// C# `NativeWindowPlacement` (`Instance` is a unit value).
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeWindowPlacement;

impl WindowPlacementNative for NativeWindowPlacement {
    /// A `WINDOWPLACEMENT` with `length`, `flags = data.flags as u32`, `showCmd = SW_HIDE`, `ptMinPosition`, `ptMaxPosition`, `rcNormalPosition` from `data`;
    /// `user32::set_window_placement(..).is_ok()`.
    fn set_hidden(&self, hwnd: Hwnd, data: &WindowPlacementData) -> bool {
        let placement = WindowPlacement {
            flags: WINDOWPLACEMENT_FLAGS(data.flags as u32),
            showCmd: SW_HIDE as u32,
            ptMinPosition: Point {
                x: data.min_x,
                y: data.min_y,
            },
            ptMaxPosition: Point {
                x: data.max_x,
                y: data.max_y,
            },
            rcNormalPosition: Rect {
                left: data.normal_left,
                top: data.normal_top,
                right: data.normal_right,
                bottom: data.normal_bottom,
            },
            ..new_window_placement()
        };
        user32::set_window_placement(hwnd, &placement).is_ok()
    }

    /// `user32::get_window_placement` into a `WindowPlacement`; `Err` -> `None`; else `WindowPlacementData` field by field (`length`, `flags`, `showCmd` as `i32`).
    fn try_get(&self, hwnd: Hwnd) -> Option<WindowPlacementData> {
        let mut placement = WindowPlacement::default();
        user32::get_window_placement(hwnd, &mut placement).ok()?;
        Some(WindowPlacementData {
            length: placement.length as i32,
            flags: placement.flags.0 as i32,
            show_command: placement.showCmd as i32,
            min_x: placement.ptMinPosition.x,
            min_y: placement.ptMinPosition.y,
            max_x: placement.ptMaxPosition.x,
            max_y: placement.ptMaxPosition.y,
            normal_left: placement.rcNormalPosition.left,
            normal_top: placement.rcNormalPosition.top,
            normal_right: placement.rcNormalPosition.right,
            normal_bottom: placement.rcNormalPosition.bottom,
        })
    }
}

/// C# `ShellPlacement.ApplyBeforeShow(hwnd, data, isVisible, native, log)`: the state the window must be shown in, or `None` when nothing is applied (the default).
pub fn apply_before_show(
    hwnd: Hwnd,
    data: Option<&WindowPlacementData>,
    is_visible: &dyn Fn(&WindowPlacementData) -> bool,
    native: &dyn WindowPlacementNative,
    log: Option<&dyn Log>,
) -> Option<WindowShowState> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let data = data?;
        if hwnd == 0 {
            return None;
        }
        let state = plan_state_before_show(data.show_command);
        if !is_visible(data) {
            return (state == WindowShowState::Maximized).then_some(state);
        }
        native.set_hidden(hwnd, data).then_some(state)
    }));
    match result {
        Ok(state) => state,
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("panic");
            let error = std::io::Error::other(message.to_owned());
            if let Some(log) = log {
                log.error("Could not restore window placement before show", Some(&error));
            }
            None
        }
    }
}

/// C# `ShellPlacement.Save(hwnd, placementPath, store, native, fullscreenRestoreState, log, fullscreenNormalBounds)`: `true` when the placement was written.
/// `fullscreen_restore_state` is the state before F11 (`None` = not in fullscreen); `fullscreen_normal_bounds` the Normal rect (physical pixels) when F11 was
/// entered, used only while in F11 (F11 puts a Normal window over the whole monitor without changing its state, so `GetWindowPlacement` reports the monitor).
pub fn save(
    hwnd: Hwnd,
    placement_path: &str,
    store: &dyn WindowPlacementStore,
    native: &dyn WindowPlacementNative,
    fullscreen_restore_state: Option<WindowShowState>,
    log: Option<&dyn Log>,
    fullscreen_normal_bounds: Option<(i32, i32, i32, i32)>,
) -> bool {
    if hwnd == 0 {
        return false;
    }
    let Some(mut current) = native.try_get(hwnd) else {
        return false;
    };
    current.show_command = resolve_show_command(current.show_command, fullscreen_restore_state);
    if fullscreen_restore_state.is_some() {
        if let Some((left, top, right, bottom)) = fullscreen_normal_bounds {
            current.normal_left = left;
            current.normal_top = top;
            current.normal_right = right;
            current.normal_bottom = bottom;
        }
    }
    match store.save(placement_path, &current) {
        Ok(()) => true,
        Err(error) => {
            if let Some(log) = log {
                log.error("Could not save window placement", Some(&error));
            }
            false
        }
    }
}

/// C# `ShellPlacement.ShowCommandFor(state, activate)`: the `ShowWindow` command for the planned state; `activate == false` takes no focus.
pub fn show_command_for(state: Option<WindowShowState>, activate: bool) -> i32 {
    match state {
        Some(WindowShowState::Maximized) => SW_SHOWMAXIMIZED,
        _ if activate => SW_SHOWNORMAL,
        _ => SW_SHOWNOACTIVATE,
    }
}

/// `ShellPlacement.Save` with the store, the native placement and the log bound (what `host::main_window` calls through `PlacementSavePort`, D-W8B4M-4).
pub struct ShellPlacementSaver {
    store: Arc<dyn WindowPlacementStore>,
    native: Rc<dyn WindowPlacementNative>,
    log: Arc<dyn Log>,
}

impl std::fmt::Debug for ShellPlacementSaver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellPlacementSaver").finish_non_exhaustive()
    }
}

impl ShellPlacementSaver {
    /// The three collaborators of the C# `ShellMainWindow` constructor (`placementStore`, `placementNative`, `log`).
    pub fn new(store: Arc<dyn WindowPlacementStore>, native: Rc<dyn WindowPlacementNative>, log: Arc<dyn Log>) -> Self {
        Self { store, native, log }
    }
}

impl PlacementSavePort for ShellPlacementSaver {
    /// `save(hwnd, &file.to_string_lossy(), &*self.store, &*self.native, restore_state, Some(&*self.log), restore_bounds)`.
    fn save(
        &self,
        hwnd: Hwnd,
        file: &Path,
        restore_state: Option<WindowShowState>,
        restore_bounds: Option<(i32, i32, i32, i32)>,
    ) -> bool {
        save(
            hwnd,
            &file.to_string_lossy(),
            &*self.store,
            &*self.native,
            restore_state,
            Some(&*self.log),
            restore_bounds,
        )
    }
}
