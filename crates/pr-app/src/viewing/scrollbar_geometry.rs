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

use crate::input::primitives::{PointD, RectD, SizeD, f64_same};
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
#[derive(Debug, Clone, Copy)]
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

impl PartialEq for ScrollBarMetrics {
    /// C# record-struct equality: `EqualityComparer<double>` per field (NaN equals NaN), see `f64_same`.
    fn eq(&self, other: &Self) -> bool {
        f64_same(self.thickness, other.thickness)
            && f64_same(self.track_margin_along, other.track_margin_along)
            && f64_same(self.track_margin_across, other.track_margin_across)
            && f64_same(self.min_thumb_length, other.min_thumb_length)
            && f64_same(self.thumb_radius, other.thumb_radius)
    }
}

impl Default for ScrollBarMetrics {
    fn default() -> Self {
        Self {
            thickness: 10.0,
            track_margin_along: 2.0,
            track_margin_across: 1.0,
            min_thumb_length: DEFAULT_MIN_THUMB_LENGTH,
            thumb_radius: 4.0,
        }
    }
}

/// C# `ScrollBarBox` (record struct): geometry of one bar. `thumb` is `RectD::default()` when `thumb_visible` is false.
#[derive(Debug, Clone, Copy)]
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

impl PartialEq for ScrollBarBox {
    /// C# record-struct equality: `EqualityComparer<double>` per double field (NaN equals NaN), see `f64_same`.
    fn eq(&self, other: &Self) -> bool {
        self.axis == other.axis
            && self.visible == other.visible
            && self.bar == other.bar
            && self.track == other.track
            && self.thumb == other.thumb
            && self.thumb_visible == other.thumb_visible
            && f64_same(self.max_offset, other.max_offset)
            && f64_same(self.offset, other.offset)
            && f64_same(self.viewport_length, other.viewport_length)
    }
}

impl ScrollBarBox {
    /// C# `ScrollBarBox.Hidden(axis)`: `(axis, false, default, default, default, false, 0, 0, 0)`.
    pub fn hidden(axis: ScrollAxis) -> Self {
        Self {
            axis,
            visible: false,
            bar: RectD::default(),
            track: RectD::default(),
            thumb: RectD::default(),
            thumb_visible: false,
            max_offset: 0.0,
            offset: 0.0,
            viewport_length: 0.0,
        }
    }

    /// C# `TrackStart`: `track.y` for a vertical bar, `track.x` for a horizontal one.
    pub fn track_start(&self) -> f64 {
        match self.axis {
            ScrollAxis::Vertical => self.track.y,
            ScrollAxis::Horizontal => self.track.x,
        }
    }

    /// C# `TrackLength`: `track.height` (vertical) or `track.width` (horizontal).
    pub fn track_length(&self) -> f64 {
        match self.axis {
            ScrollAxis::Vertical => self.track.height,
            ScrollAxis::Horizontal => self.track.width,
        }
    }

    /// C# `ThumbStart`: `thumb.y` (vertical) or `thumb.x` (horizontal).
    pub fn thumb_start(&self) -> f64 {
        match self.axis {
            ScrollAxis::Vertical => self.thumb.y,
            ScrollAxis::Horizontal => self.thumb.x,
        }
    }

    /// C# `ThumbLength`: `thumb.height` (vertical) or `thumb.width` (horizontal).
    pub fn thumb_length(&self) -> f64 {
        match self.axis {
            ScrollAxis::Vertical => self.thumb.height,
            ScrollAxis::Horizontal => self.thumb.width,
        }
    }

    /// C# `Travel` = `Math.Max(0.0, TrackLength - ThumbLength)` (NaN propagates, see `cs_max`).
    pub fn travel(&self) -> f64 {
        cs_max(0.0, self.track_length() - self.thumb_length())
    }

    /// C# `Along(point)`: `point.y` (vertical) or `point.x` (horizontal).
    pub fn along(&self, point: PointD) -> f64 {
        match self.axis {
            ScrollAxis::Vertical => point.y,
            ScrollAxis::Horizontal => point.x,
        }
    }
}

/// C# `ScrollBarLayout` (record struct): both bars and the corner square.
#[derive(Debug, Clone, Copy)]
pub struct ScrollBarLayout {
    pub vertical: ScrollBarBox,
    pub horizontal: ScrollBarBox,
    pub corner: RectD,
    pub has_corner: bool,
}

impl PartialEq for ScrollBarLayout {
    /// C# record-struct equality (members compared with their own equality, NaN-aware).
    fn eq(&self, other: &Self) -> bool {
        self.vertical == other.vertical
            && self.horizontal == other.horizontal
            && self.corner == other.corner
            && self.has_corner == other.has_corner
    }
}

impl ScrollBarLayout {
    /// C# `ScrollBarLayout.None`: `(Hidden(Vertical), Hidden(Horizontal), default, false)`.
    pub fn none() -> Self {
        Self {
            vertical: ScrollBarBox::hidden(ScrollAxis::Vertical),
            horizontal: ScrollBarBox::hidden(ScrollAxis::Horizontal),
            corner: RectD::default(),
            has_corner: false,
        }
    }

    /// C# `AnyVisible` = `Vertical.Visible || Horizontal.Visible`.
    pub fn any_visible(&self) -> bool {
        self.vertical.visible || self.horizontal.visible
    }

    /// C# `Box(axis)`.
    pub fn box_for(&self, axis: ScrollAxis) -> &ScrollBarBox {
        match axis {
            ScrollAxis::Vertical => &self.vertical,
            ScrollAxis::Horizontal => &self.horizontal,
        }
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
    let width = sanitize(client.width);
    let height = sanitize(client.height);
    let thickness = sanitize(metrics.thickness);
    let hv = layout.horizontal_bar_visible;
    let vv = layout.vertical_bar_visible;
    let bt_x = cs_min(thickness, width);
    let bt_y = cs_min(thickness, height);

    let vertical_bar = if vv {
        RectD {
            x: cs_max(0.0, width - thickness),
            y: 0.0,
            width: bt_x,
            height: cs_max(0.0, height - if hv { thickness } else { 0.0 }),
        }
    } else {
        RectD::default()
    };
    let horizontal_bar = if hv {
        RectD {
            x: 0.0,
            y: cs_max(0.0, height - thickness),
            width: cs_max(0.0, width - if vv { thickness } else { 0.0 }),
            height: bt_y,
        }
    } else {
        RectD::default()
    };

    let vertical = if vv {
        build_box(
            ScrollAxis::Vertical,
            vertical_bar,
            layout.max_vertical_offset,
            vertical_offset,
            layout.viewport_height,
            metrics,
        )
    } else {
        ScrollBarBox::hidden(ScrollAxis::Vertical)
    };
    let horizontal = if hv {
        build_box(
            ScrollAxis::Horizontal,
            horizontal_bar,
            layout.max_horizontal_offset,
            horizontal_offset,
            layout.viewport_width,
            metrics,
        )
    } else {
        ScrollBarBox::hidden(ScrollAxis::Horizontal)
    };

    let both = vv && hv;
    let corner = if both {
        RectD {
            x: cs_max(0.0, width - thickness),
            y: cs_max(0.0, height - thickness),
            width: bt_x,
            height: bt_y,
        }
    } else {
        RectD::default()
    };

    ScrollBarLayout {
        vertical,
        horizontal,
        corner,
        has_corner: both,
    }
}

/// C# `ScrollBarGeometry.OffsetForThumbStart(box, thumbStart)`: offset for a thumb starting at `thumb_start` (client coordinate
/// along the axis), clamped to `[0, max_offset]` with `cs_min`/`cs_max`; `+0.0` when `!(travel > 0) || !(max_offset > 0)` (a NaN
/// travel or max gives 0). A NaN `thumb_start` is NOT guarded: the ratio is NaN and so is the result (golden `box_helpers`).
pub fn offset_for_thumb_start(b: &ScrollBarBox, thumb_start: f64) -> f64 {
    let travel = b.travel();
    // `!(x > 0.0)` on purpose: a NaN travel or max offset must return 0 (C# `!(travel > 0.0)`), so `gt_zero` is not `<= 0.0`.
    if !gt_zero(travel) || !gt_zero(b.max_offset) {
        return 0.0;
    }
    let ratio = (thumb_start - b.track_start()) / travel;
    cs_min(b.max_offset, cs_max(0.0, ratio * b.max_offset))
}

/// C# `ScrollBarGeometry.HitTest(layout, point)`: corner first (only if `has_corner`), then the vertical bar, then the horizontal bar.
pub fn hit_test(layout: &ScrollBarLayout, point: PointD) -> ScrollBarPart {
    if layout.has_corner && inside(layout.corner, point) {
        return ScrollBarPart::Corner;
    }
    let part = hit_box(&layout.vertical, point);
    if part != ScrollBarPart::None {
        part
    } else {
        hit_box(&layout.horizontal, point)
    }
}

/// C# `ScrollBarGeometry.AxisAt(layout, point)`: vertical when inside the visible vertical bar, else horizontal when inside the
/// visible horizontal bar, else `None`. The corner square is in neither bar rectangle, so it gives `None`.
pub fn axis_at(layout: &ScrollBarLayout, point: PointD) -> Option<ScrollAxis> {
    if layout.vertical.visible && inside(layout.vertical.bar, point) {
        Some(ScrollAxis::Vertical)
    } else if layout.horizontal.visible && inside(layout.horizontal.bar, point) {
        Some(ScrollAxis::Horizontal)
    } else {
        None
    }
}

/// C# `ScrollBarGeometry.Inside(rect, point)` (`internal`, also used by `ScrollBars.HitTest` in W8-16f, hence `pub`):
/// half-open: `rect.width > 0 && rect.height > 0 && x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height`.
/// This is NOT `RectD::contains` (closed on all four sides).
pub fn inside(rect: RectD, point: PointD) -> bool {
    rect.width > 0.0
        && rect.height > 0.0
        && point.x >= rect.x
        && point.x < rect.x + rect.width
        && point.y >= rect.y
        && point.y < rect.y + rect.height
}

/// C# `Math.Min(a, b)`: NaN if either is NaN; `-0.0` is smaller than `+0.0`.
fn cs_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
}

/// C# `Math.Max(a, b)`: NaN if either is NaN; `+0.0` is larger than `-0.0`.
fn cs_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b || (a == b && a.is_sign_positive()) {
        a
    } else {
        b
    }
}

/// `x > 0.0` as a plain predicate: false for NaN, true only for a strictly positive value (C# `x > 0.0`).
fn gt_zero(x: f64) -> bool {
    x > 0.0
}

/// C# `Sanitize`: `value` when finite and `> 0`, else `0.0` (NaN, negative, zero and infinities all give `+0.0`).
fn sanitize(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 { value } else { 0.0 }
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
    let vertical = axis == ScrollAxis::Vertical;
    let along = metrics.track_margin_along;
    let across = metrics.track_margin_across;
    let track = if vertical {
        RectD {
            x: bar.x + across,
            y: bar.y + along,
            width: cs_max(0.0, bar.width - 2.0 * across),
            height: cs_max(0.0, bar.height - 2.0 * along),
        }
    } else {
        RectD {
            x: bar.x + along,
            y: bar.y + across,
            width: cs_max(0.0, bar.width - 2.0 * along),
            height: cs_max(0.0, bar.height - 2.0 * across),
        }
    };
    let track_length = if vertical { track.height } else { track.width };

    let range = cs_max(0.0, max_offset);
    let clamped = cs_min(range, cs_max(0.0, if offset.is_nan() { 0.0 } else { offset }));
    let viewport = sanitize(viewport_length);
    let total = range + viewport;
    let mut thumb = if total > 0.0 {
        track_length * viewport / total
    } else {
        track_length
    };
    thumb = cs_min(cs_max(0.0, thumb), track_length);
    thumb = cs_max(metrics.min_thumb_length, thumb);
    let thumb_visible = track_length > metrics.min_thumb_length || (track_length >= thumb && track_length > 0.0);

    if !thumb_visible {
        return ScrollBarBox {
            axis,
            visible: true,
            bar,
            track,
            thumb: RectD::default(),
            thumb_visible: false,
            max_offset: range,
            offset: clamped,
            viewport_length: viewport,
        };
    }

    thumb = cs_min(thumb, track_length);
    let travel = track_length - thumb;
    let track_origin = if vertical { track.y } else { track.x };
    let start = track_origin + if range > 0.0 { travel * clamped / range } else { 0.0 };
    let thumb_rect = if vertical {
        RectD {
            x: track.x,
            y: start,
            width: track.width,
            height: thumb,
        }
    } else {
        RectD {
            x: start,
            y: track.y,
            width: thumb,
            height: track.height,
        }
    };

    ScrollBarBox {
        axis,
        visible: true,
        bar,
        track,
        thumb: thumb_rect,
        thumb_visible: true,
        max_offset: range,
        offset: clamped,
        viewport_length: viewport,
    }
}

/// C# `HitBox` (private).
fn hit_box(b: &ScrollBarBox, point: PointD) -> ScrollBarPart {
    if !b.visible || !inside(b.bar, point) {
        return ScrollBarPart::None;
    }
    if b.thumb_visible && inside(b.thumb, point) {
        return ScrollBarPart::Thumb;
    }
    if inside(b.track, point) {
        if !b.thumb_visible {
            ScrollBarPart::Background
        } else if b.along(point) < b.thumb_start() {
            ScrollBarPart::TrackBefore
        } else {
            ScrollBarPart::TrackAfter
        }
    } else {
        ScrollBarPart::Background
    }
}
