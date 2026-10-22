//! Port of the Win32 half of `PhotoReview.App.Shared/Services/InitialViewportPredictor.cs` lines 64-131 (`PredictDecodeBox` P/Invokes, `DpiFor`, `VisibleAreas`, baseline `d92d6aae`):
//! the real [`MonitorProbe`]. Unit W8-17d. Contract: `contracts/W8-17d.md`. (The pure rule is `pr_app::services::initial_viewport::predict_decode_box`, W8-04c, merged; its contract
//! D-04c-5 says the real probe belongs to the start-up unit.)
//!
//! Every call goes through the safe wrappers of `win32::api` (W8-11b); this file adds NO `unsafe`. The probe is a unit value: it keeps no handle and may be used from any thread
//! (the C# prediction runs on a pool thread).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::services::initial_viewport::{MonitorId, MonitorProbe, PixelRect};

/// C# `InitialViewportPredictor` P/Invokes as a [`MonitorProbe`].
#[derive(Debug, Default, Clone, Copy)]
pub struct Win32MonitorProbe;

impl MonitorProbe for Win32MonitorProbe {
    /// `user32::monitor_from_rect(&Rect { left, top, right, bottom }, MONITOR_DEFAULTTONEAREST)`; `0` -> `None`.
    fn monitor_nearest_rect(&self, rect: PixelRect) -> Option<MonitorId> {
        todo!()
    }

    /// `user32::monitor_from_point(Point { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)`; `0` -> `None`.
    fn monitor_primary(&self) -> Option<MonitorId> {
        todo!()
    }

    /// `user32::get_monitor_info(monitor, &mut info)` with `info = win32::structs::new_monitor_info()`; `Err` -> `None`; else `rcWork` as a `PixelRect`.
    fn monitor_work_area(&self, monitor: MonitorId) -> Option<PixelRect> {
        todo!()
    }

    /// C# `DpiFor`: `awareness = user32::get_awareness_from_dpi_awareness_context(user32::get_thread_dpi_awareness_context())`; per-monitor (`DPI_AWARENESS_PER_MONITOR`) and
    /// `shcore::get_dpi_for_monitor(monitor, MDT_EFFECTIVE_DPI)` is `Ok((x, _))` -> `x`; else unaware (`DPI_AWARENESS_UNAWARE`) -> 96; else `user32::get_dpi_for_system()`.
    fn dpi_for(&self, monitor: MonitorId) -> u32 {
        todo!()
    }

    /// `rect = Rect::default()`; `user32::adjust_window_rect_ex_for_dpi(&mut rect, WS_OVERLAPPEDWINDOW, false, 0, dpi)`; `Err` -> `None`; else the rect as a `PixelRect`.
    fn frame_insets(&self, dpi: u32) -> Option<PixelRect> {
        todo!()
    }

    /// `host::main_window::windowing::Win32MonitorLayout::work_areas()` (`enum_display_monitors` mapped through `get_monitor_info`, enumeration order kept, a failing monitor skipped,
    /// a failing enumeration an empty list) converted to `PixelRect` (`left`, `top`, `right`, `bottom`).
    fn work_areas(&self) -> Vec<PixelRect> {
        todo!()
    }
}
