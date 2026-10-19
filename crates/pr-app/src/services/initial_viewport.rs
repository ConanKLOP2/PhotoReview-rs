//! Port of `Services/InitialViewportPredictor.cs` (131 lines; C# `internal static partial class`). Contract:
//! `contracts/W8-04c.md`, rules R-IV-*.
//!
//! P-1 startup: predicts, before the main window exists, the client area it will be shown with (the saved placement, restored
//! before the show by the placement adapter), so the launch file's decode can start right after `config.json` is read. The
//! prediction only picks the decode box: when it is wrong, the presenter's own request (keyed by the real viewport) decodes again.
//!
//! Split (decision D-04c-5): the geometry ([`predict_client_size`]) and the decode-box rule ([`predict_decode_box`]) are pure and
//! take every Win32 fact through the [`MonitorProbe`] trait. The real probe (`MonitorFromRect`, `GetMonitorInfoW`,
//! `AdjustWindowRectExForDpi`, `GetDpiForMonitor`, `GetDpiForSystem`, `GetThreadDpiAwarenessContext`) lives in `pr-platform-win`
//! (`shell_startup`, W8-17) with `LibraryImport`-style safe wrappers and SAFETY comments there. This module has no `unsafe`.

use pr_imaging::adaptive_preview::calculate_target_decode_box;
use pr_imaging::decode_types::DecodeBox;

use crate::windowing::contracts::{WindowPlacementData, WindowShowState, plan_state_before_show};
use crate::windowing::monitor_layout::ScreenRect;
use crate::windowing::visibility::is_visible;

/// `WS_OVERLAPPEDWINDOW` (0x00CF0000): the style the frame insets are measured with (R-IV-7).
pub const WS_OVERLAPPED_WINDOW: u32 = 0x00CF_0000;
/// `MONITOR_DEFAULTTOPRIMARY` (1).
pub const MONITOR_DEFAULT_TO_PRIMARY: u32 = 1;
/// `MONITOR_DEFAULTTONEAREST` (2).
pub const MONITOR_DEFAULT_TO_NEAREST: u32 = 2;
/// `MDT_EFFECTIVE_DPI` (0) for `GetDpiForMonitor`.
pub const MDT_EFFECTIVE_DPI: i32 = 0;
/// `DPI_AWARENESS_UNAWARE` (0).
pub const DPI_AWARENESS_UNAWARE: i32 = 0;
/// `DPI_AWARENESS_PER_MONITOR_AWARE` (2).
pub const DPI_AWARENESS_PER_MONITOR: i32 = 2;

/// C# `InitialViewportPredictor.PixelRect` (`readonly record struct`, Win32 RECT layout: `right`/`bottom` exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PixelRect {
    /// `Right - Left`, wrapping like C# unchecked arithmetic (R-IV-8).
    pub fn width(&self) -> i32 {
        self.right.wrapping_sub(self.left)
    }

    /// `Bottom - Top`, wrapping like C# unchecked arithmetic (R-IV-8).
    pub fn height(&self) -> i32 {
        self.bottom.wrapping_sub(self.top)
    }
}

/// An `HMONITOR` value. `0` (no monitor) is never represented: the probe returns `None` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorId(pub isize);

/// Every Win32 fact the predictor reads. Implemented by `pr-platform-win` (the real calls) and by the tests (fixed values).
/// A probe returns `None` where the C# call returns zero/false, or throws one of the three exceptions C# catches (R-IV-9).
pub trait MonitorProbe {
    /// `MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST)`; `None` when it yields no monitor.
    fn monitor_nearest_rect(&self, rect: PixelRect) -> Option<MonitorId>;
    /// `MonitorFromPoint((0, 0), MONITOR_DEFAULTTOPRIMARY)`; `None` when it yields no monitor.
    fn monitor_primary(&self) -> Option<MonitorId>;
    /// `GetMonitorInfoW(monitor).Work`; `None` when the call fails (C# `!GetMonitorInfo(...)` returns null).
    fn monitor_work_area(&self, monitor: MonitorId) -> Option<PixelRect>;
    /// C# `DpiFor(monitor)`: when the thread is per-monitor aware and `GetDpiForMonitor(EFFECTIVE)` succeeds, its `x`; when the thread
    /// is unaware, 96; otherwise `GetDpiForSystem()`. The awareness is read by the probe (`GetThreadDpiAwarenessContext`).
    fn dpi_for(&self, monitor: MonitorId) -> u32;
    /// `AdjustWindowRectExForDpi(empty, WS_OVERLAPPED_WINDOW, false, 0, dpi)` as a rect: the insets (negative left/top, positive
    /// right/bottom). `None` when the call returns false.
    fn frame_insets(&self, dpi: u32) -> Option<PixelRect>;
    /// `Win32MonitorLayout.Instance.WorkAreas()` (every monitor's work area, in order), used for the on-screen test.
    fn work_areas(&self) -> Vec<PixelRect>;
}

/// C# `PredictClientSize(maximized, normalBounds, workArea, frameInsets)`: the client size of a standard-frame window.
///
/// Maximized: `(work.Width, work.Height - caption)` with `caption = -frameInsets.Top - frameInsets.Right` (R-IV-2, literal C# formula,
/// `Right` not `Bottom`). Normal: `(normal.Width - insets.Width, normal.Height - insets.Height)` (R-IV-3). `None` unless both
/// sides are greater than 0 (R-IV-4).
pub fn predict_client_size(
    maximized: bool,
    normal_bounds: PixelRect,
    work_area: PixelRect,
    frame_insets: PixelRect,
) -> Option<(i32, i32)> {
    let (width, height) = if maximized {
        // R-IV-2: the literal C# caption formula (-Top - Right), wrapping like unchecked int arithmetic (R-IV-8).
        let caption = frame_insets.top.wrapping_neg().wrapping_sub(frame_insets.right);
        (work_area.width(), work_area.height().wrapping_sub(caption))
    } else {
        // R-IV-3: the saved bounds minus the frame.
        (
            normal_bounds.width().wrapping_sub(frame_insets.width()),
            normal_bounds.height().wrapping_sub(frame_insets.height()),
        )
    };
    // R-IV-4: only a positive client size is a prediction.
    (width > 0 && height > 0).then_some((width, height))
}

/// C# `PredictDecodeBox(placement, defaultWidthDip, defaultHeightDip, qualityMultiplier)`: the decode box the window's first layout
/// will compute for the saved placement (`None` placement = first start, the default size on the primary monitor).
///
/// Steps (R-IV-5): `maximized` = placement present and `plan_state_before_show(show_command) == Maximized`; `normal` = the saved
/// `NormalLeft..NormalBottom`; `on_screen` = `normal` present and inside one of `probe.work_areas()`; monitor = nearest(`normal`) when
/// on screen, else primary; no monitor or no work area = `None`; `dpi = probe.dpi_for(monitor)`; `insets = probe.frame_insets(dpi)`
/// (`None` = `None`); `scale = dpi / 96.0`; `bounds` = `normal` when on screen and not maximized, else the default size
/// `(round_ties_even(default_w * scale), round_ties_even(default_h * scale))` at (0, 0) (R-IV-6); `client = predict_client_size(...)`;
/// result = `calculate_target_decode_box(client.w / scale, client.h / scale, scale, quality_multiplier)` (`pr_imaging::adaptive_preview`),
/// `None` on its `Err` (R-IV-9).
pub fn predict_decode_box(
    placement: Option<&WindowPlacementData>,
    default_width_dip: f64,
    default_height_dip: f64,
    quality_multiplier: f64,
    probe: &dyn MonitorProbe,
) -> Option<DecodeBox> {
    // R-IV-5: maximized = placement present and planned as maximized before the show.
    let maximized =
        placement.is_some_and(|saved| plan_state_before_show(saved.show_command) == WindowShowState::Maximized);
    let normal = placement.map(|saved| PixelRect {
        left: saved.normal_left,
        top: saved.normal_top,
        right: saved.normal_right,
        bottom: saved.normal_bottom,
    });
    let on_screen = normal.is_some_and(|rect| {
        let areas: Vec<ScreenRect> = probe
            .work_areas()
            .into_iter()
            .map(|area| ScreenRect::new(area.left, area.top, area.right, area.bottom))
            .collect();
        is_visible(rect.left, rect.top, rect.right, rect.bottom, &areas)
    });
    let monitor = match (on_screen, normal) {
        (true, Some(rect)) => probe.monitor_nearest_rect(rect),
        _ => probe.monitor_primary(),
    }?;
    let work_area = probe.monitor_work_area(monitor)?;
    let dpi = probe.dpi_for(monitor);
    let insets = probe.frame_insets(dpi)?;
    let scale = f64::from(dpi) / 96.0;

    // R-IV-6: a visible normal placement keeps its bounds; otherwise the default size, rounded half to even (T-4), at (0, 0).
    let bounds = match normal {
        Some(rect) if on_screen && !maximized => rect,
        _ => PixelRect {
            left: 0,
            top: 0,
            right: (default_width_dip * scale).round_ties_even() as i32,
            bottom: (default_height_dip * scale).round_ties_even() as i32,
        },
    };
    let (client_width, client_height) = predict_client_size(maximized, bounds, work_area, insets)?;
    // R-IV-9: a policy error (for example a quality multiplier of 0 or NaN) gives None.
    calculate_target_decode_box(
        f64::from(client_width) / scale,
        f64::from(client_height) / scale,
        scale,
        quality_multiplier,
    )
    .ok()
}
