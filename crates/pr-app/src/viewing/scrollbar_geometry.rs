//! Port of `PhotoReview.Shell.Win32/Viewing/ScrollBarGeometry.cs` (WP-26 NE-3 a): pure geometry of the self-drawn scroll bars.
//!
//! Baseline `d92d6aae`. Everything is DIP in client coordinates. The module holds the C# types `ScrollAxis`, `ScrollBarPart`,
//! `ScrollBarMetrics`, `ScrollBarBox`, `ScrollBarLayout` and the static class `ScrollBarGeometry` (its methods are the free
//! functions `compute`, `offset_for_thumb_start`, `hit_test`, `axis_at`, `inside`). Layering decision D-W8-16e-1: the module lives
//! in `pr_app`, not in `pr_platform_win::shell_viewing` as the plan row says, because it consumes
//! `pr_app::viewport::types::ViewportLayout` and `pr_app::input::primitives::{PointD, SizeD, RectD}`, and `pr-app` depends on
//! `pr-platform-win`, not the reverse.
//!
//! .NET arithmetic that must be reproduced (contract traps T1 to T3): `Math.Min`/`Math.Max` propagate NaN and order `-0.0` below
//! `+0.0`, Rust `f64::min`/`f64::max` ignore a NaN operand. Use the private helpers `cs_min`/`cs_max` for every C# `Math.Min`/`Math.Max`.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::input::primitives::{PointD, RectD, SizeD};
use crate::viewport::types::ViewportLayout;

/// C# `ScrollAxis`: `Vertical = 0`, `Horizontal = 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollAxis {
    Vertical = 0,
    Horizontal = 1,
}

/// C# `ScrollBarPart` (what `hit_test` found): `None = 0`, `Thumb`, `TrackBefore` (PageUp/PageLeft), `TrackAfter`
/// (PageDown/PageRight), `Background` (inside the bar but outside the track margin, or no thumb shown: swallows the mouse),
/// `Corner` (only when both bars show).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollBarPart {
    None = 0,
    Thumb = 1,
    TrackBefore = 2,
    TrackAfter = 3,
    Background = 4,
    Corner = 5,
}

/// C# `ScrollBarMetrics.DefaultMinThumbLength` = 8 (`Math.Floor(button / 2)` of the WPF track).
pub const DEFAULT_MIN_THUMB_LENGTH: f64 = 8.0;

/// C# `ScrollBarMetrics` (record struct with `init` properties). `Default` is the WPF 96 DPI set, NOT all zeros:
/// `thickness 10`, `track_margin_along 2`, `track_margin_across 1`, `min_thumb_length 8`, `thumb_radius 4`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollBarMetrics {
    /// `ScrollBar Width/Height = 10`.
    pub thickness: f64,
    /// Margin of the track along the axis (vertical `Margin="1,2"`: 2; horizontal `"2,1"`: 2).
    pub track_margin_along: f64,
    /// Margin of the track across the axis (1).
    pub track_margin_across: f64,
    /// Minimum thumb length (`Track.ComputeScrollBarLengths`).
    pub min_thumb_length: f64,
    /// `CornerRadius="4"` of `Dark.ScrollThumb`.
    pub thumb_radius: f64,
}

impl Default for ScrollBarMetrics {
    fn default() -> Self {
        todo!()
    }
}

/// C# `ScrollBarBox` (record struct): geometry of one bar. `thumb` is `RectD::default()` when `thumb_visible` is false.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollBarBox {
    pub axis: ScrollAxis,
    pub visible: bool,
    /// The whole bar (background `Dark.ScrollTrack`).
    pub bar: RectD,
    /// The `Track` rectangle (bar minus margins).
    pub track: RectD,
    pub thumb: RectD,
    pub thumb_visible: bool,
    /// `ScrollViewer.ScrollableWidth/Height`.
    pub max_offset: f64,
    /// `ScrollBar.Value`.
    pub offset: f64,
    /// `ViewportSize` along the axis (one page).
    pub viewport_length: f64,
}

impl ScrollBarBox {
    /// C# `ScrollBarBox.Hidden(axis)`: `(axis, false, default, default, default, false, 0, 0, 0)`.
    pub fn hidden(axis: ScrollAxis) -> Self {
        todo!()
    }

    /// C# `TrackStart`: `track.y` for a vertical bar, `track.x` for a horizontal one.
    pub fn track_start(&self) -> f64 {
        todo!()
    }

    /// C# `TrackLength`: `track.height` (vertical) or `track.width` (horizontal).
    pub fn track_length(&self) -> f64 {
        todo!()
    }

    /// C# `ThumbStart`: `thumb.y` (vertical) or `thumb.x` (horizontal).
    pub fn thumb_start(&self) -> f64 {
        todo!()
    }

    /// C# `ThumbLength`: `thumb.height` (vertical) or `thumb.width` (horizontal).
    pub fn thumb_length(&self) -> f64 {
        todo!()
    }

    /// C# `Travel` = `Math.Max(0.0, TrackLength - ThumbLength)` (NaN propagates, see `cs_max`).
    pub fn travel(&self) -> f64 {
        todo!()
    }

    /// C# `Along(point)`: `point.y` (vertical) or `point.x` (horizontal).
    pub fn along(&self, point: PointD) -> f64 {
        todo!()
    }
}

/// C# `ScrollBarLayout` (record struct): both bars and the corner square.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollBarLayout {
    pub vertical: ScrollBarBox,
    pub horizontal: ScrollBarBox,
    pub corner: RectD,
    pub has_corner: bool,
}

impl ScrollBarLayout {
    /// C# `ScrollBarLayout.None`: `(Hidden(Vertical), Hidden(Horizontal), default, false)`.
    pub fn none() -> Self {
        todo!()
    }

    /// C# `AnyVisible` = `Vertical.Visible || Horizontal.Visible`.
    pub fn any_visible(&self) -> bool {
        todo!()
    }

    /// C# `Box(axis)`.
    pub fn box_for(&self, axis: ScrollAxis) -> &ScrollBarBox {
        todo!()
    }
}

/// C# `ScrollBarGeometry.Compute(client, layout, horizontalOffset, verticalOffset, metrics)`. Pure: no clock, no I/O.
/// Reads only `layout.{horizontal,vertical}_bar_visible`, `layout.max_{horizontal,vertical}_offset` and
/// `layout.viewport_{width,height}` (the C# `extent` parameter of `BuildBox` is unused: do not port it; `layout.image_rect` and
/// `layout.extent_*` are never read).
pub fn compute(
    client: SizeD,
    layout: &ViewportLayout,
    horizontal_offset: f64,
    vertical_offset: f64,
    metrics: &ScrollBarMetrics,
) -> ScrollBarLayout {
    todo!()
}

/// C# `ScrollBarGeometry.OffsetForThumbStart(box, thumbStart)`: offset for a thumb starting at `thumb_start` (client coordinate
/// along the axis), clamped to `[0, max_offset]` with `cs_min`/`cs_max`; `+0.0` when `!(travel > 0) || !(max_offset > 0)` (a NaN
/// travel or max gives 0). A NaN `thumb_start` is NOT guarded: the ratio is NaN and so is the result (golden `box_helpers`).
pub fn offset_for_thumb_start(b: &ScrollBarBox, thumb_start: f64) -> f64 {
    todo!()
}

/// C# `ScrollBarGeometry.HitTest(layout, point)`: corner first (only if `has_corner`), then the vertical bar, then the horizontal bar.
pub fn hit_test(layout: &ScrollBarLayout, point: PointD) -> ScrollBarPart {
    todo!()
}

/// C# `ScrollBarGeometry.AxisAt(layout, point)`: vertical when inside the visible vertical bar, else horizontal when inside the
/// visible horizontal bar, else `None`. The corner square is in neither bar rectangle, so it gives `None`.
pub fn axis_at(layout: &ScrollBarLayout, point: PointD) -> Option<ScrollAxis> {
    todo!()
}

/// C# `ScrollBarGeometry.Inside(rect, point)` (`internal`, also used by `ScrollBars.HitTest` in W8-16f, hence `pub`):
/// half-open: `rect.width > 0 && rect.height > 0 && x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height`.
/// This is NOT `RectD::contains` (closed on all four sides).
pub fn inside(rect: RectD, point: PointD) -> bool {
    todo!()
}

/// C# `Math.Min(a, b)`: NaN if either is NaN; `-0.0` is smaller than `+0.0`.
fn cs_min(a: f64, b: f64) -> f64 {
    todo!()
}

/// C# `Math.Max(a, b)`: NaN if either is NaN; `+0.0` is larger than `-0.0`.
fn cs_max(a: f64, b: f64) -> f64 {
    todo!()
}

/// C# `Sanitize`: `value` when finite and `> 0`, else `0.0` (NaN, negative, zero and infinities all give `+0.0`).
fn sanitize(value: f64) -> f64 {
    todo!()
}

/// C# `BuildBox` (private). `bar` is the already computed bar rectangle.
fn build_box(
    axis: ScrollAxis,
    bar: RectD,
    max_offset: f64,
    offset: f64,
    viewport_length: f64,
    metrics: &ScrollBarMetrics,
) -> ScrollBarBox {
    todo!()
}

/// C# `HitBox` (private).
fn hit_box(b: &ScrollBarBox, point: PointD) -> ScrollBarPart {
    todo!()
}
