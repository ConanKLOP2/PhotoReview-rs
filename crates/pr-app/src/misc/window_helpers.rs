//! Port of `PhotoReview.App.Shared/MainWindowHelpers.cs` (C# `MainWindowHelpers`, baseline d92d6aae), without the two members listed
//! under "Not ported" in the contract: `CalculateClickLevelFromEffectiveZoom` (same function as `menus::zoom_rules`, W8-01d) and
//! `ForwardingFolderSink` (a WPF window forwarder, not a helper; the Rust composition passes the sink itself).
//! Unit W8-04e. Contract: `contracts/W8-04e.md`.
//!
//! Pure module: no `unsafe`, no `windows` crate. Every function is total: a non-finite or unusable input gives the C# fallback value,
//! never a panic (the C# `double` arithmetic never throws).

use crate::input::primitives::{PointD, RectD, SizeD, f64_same};
use pr_core::model::enums::FitWidthAnchor;
use pr_core::settings::app_settings::AppSettings;

/// C# `MainWindowHelpers.ZoomViewportOffsets` (`internal readonly record struct (double Horizontal, double Vertical)`).
/// `PartialEq` is field-wise with `f64_same` (C# `EqualityComparer<double>.Default`: NaN equals NaN), not `==`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ZoomViewportOffsets {
    pub horizontal: f64,
    pub vertical: f64,
}

impl PartialEq for ZoomViewportOffsets {
    fn eq(&self, other: &Self) -> bool {
        f64_same(self.horizontal, other.horizontal) && f64_same(self.vertical, other.vertical)
    }
}

/// C# `MainWindowHelpers.ZoomImagePoint` (`internal readonly record struct (double X, double Y)`): a point on the image as a
/// fraction of its size (see [`normalize_image_point`]) or in source pixels (see [`calculate_uniform_image_point`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct ZoomImagePoint {
    pub x: f64,
    pub y: f64,
}

impl PartialEq for ZoomImagePoint {
    fn eq(&self, other: &Self) -> bool {
        f64_same(self.x, other.x) && f64_same(self.y, other.y)
    }
}

/// C# `FitWidthAnchor` image fraction for `TopThird` (`(0.5, 1/3)`) and `BottomThird` (`(0.5, 2/3)`), see
/// [`calculate_fit_width_anchor_point`].
pub const FIT_WIDTH_TOP_THIRD: ZoomImagePoint = ZoomImagePoint { x: 0.5, y: 1.0 / 3.0 };
pub const FIT_WIDTH_BOTTOM_THIRD: ZoomImagePoint = ZoomImagePoint { x: 0.5, y: 2.0 / 3.0 };
/// The image centre, used by `FitWidthAnchor::Centre` and by the "no anchor" callers (C# `new ZoomImagePoint(0.5, 0.5)`).
pub const IMAGE_CENTRE: ZoomImagePoint = ZoomImagePoint { x: 0.5, y: 0.5 };

/// C# `CalculateUniformImagePoint(elementWidth, elementHeight, sourceWidth, sourceHeight, pointerX, pointerY)`.
///
/// Maps a pointer position inside a letterboxed (`Uniform`) element to a point in SOURCE pixels. Returns `(0, 0)` when any
/// element or source size is not positive and finite, or the pointer is not finite (the C# early return, not the centre).
/// The image is scaled by `min(elementWidth / sourceWidth, elementHeight / sourceHeight)` and centred. Each coordinate is
/// clamped to `[0, source size]`, and a NaN coordinate (from a denormal scale) becomes 0.
pub fn calculate_uniform_image_point(element: SizeD, source: SizeD, pointer: PointD) -> ZoomImagePoint {
    let positive = |v: f64| v > 0.0 && v.is_finite();
    if !positive(element.width)
        || !positive(element.height)
        || !positive(source.width)
        || !positive(source.height)
        || !pointer.x.is_finite()
        || !pointer.y.is_finite()
    {
        return ZoomImagePoint { x: 0.0, y: 0.0 };
    }
    let scale = (element.width / source.width).min(element.height / source.height);
    let left = (element.width - source.width * scale) / 2.0;
    let top = (element.height - source.height * scale) / 2.0;
    ZoomImagePoint {
        x: clamp_to_source((pointer.x - left) / scale, source.width),
        y: clamp_to_source((pointer.y - top) / scale, source.height),
    }
}

/// C# `NormalizeImagePoint(x, y, width, height)`: expresses a point on the image as a fraction of its size, so the wheel anchor
/// survives a zoom step (the element's own size changes, not a `LayoutTransform`). NOT clamped: a pointer beside a centred,
/// smaller-than-viewport image keeps its old behaviour. Returns `(0.5, 0.5)` when `width` or `height` is not positive and finite,
/// or when `x / width` or `y / height` is not finite.
pub fn normalize_image_point(point: PointD, size: SizeD) -> ZoomImagePoint {
    let positive = |v: f64| v > 0.0 && v.is_finite();
    if positive(size.width) && positive(size.height) {
        let x = point.x / size.width;
        let y = point.y / size.height;
        if x.is_finite() && y.is_finite() {
            return ZoomImagePoint { x, y };
        }
    }
    IMAGE_CENTRE
}

/// C# `CalculateOffsetsFromAnchorDelta(current H/V offset, anchor before X/Y, anchor after X/Y, new extent W/H, viewport W/H)`:
/// the new scroll offset `current + anchorAfter - anchorBefore`, clamped per axis by [`clamp_offset`].
pub fn calculate_offsets_from_anchor_delta(
    current: PointD,
    anchor_before: PointD,
    anchor_after: PointD,
    new_extent: SizeD,
    viewport: SizeD,
) -> ZoomViewportOffsets {
    ZoomViewportOffsets {
        horizontal: clamp_offset(
            current.x + anchor_after.x - anchor_before.x,
            new_extent.width,
            viewport.width,
        ),
        vertical: clamp_offset(
            current.y + anchor_after.y - anchor_before.y,
            new_extent.height,
            viewport.height,
        ),
    }
}

/// C# `CalculateZoomToPointOffsets`: the scroll offsets that put the image point `anchor` (a fraction, see
/// [`normalize_image_point`]) back under the cursor after a zoom to any level.
///
/// `image` is where the image element sits in the viewport after the new layout at the current offsets (its x/y) and its new
/// size (width/height); `cursor` is the cursor in viewport coordinates; `current` the offsets before the zoom; `extent` and
/// `viewport` the new extent and the viewport. Implemented as [`calculate_offsets_from_anchor_delta`] with
/// `anchor_before = cursor` and `anchor_after = image.origin + anchor * image.size`.
pub fn calculate_zoom_to_point_offsets(
    anchor: ZoomImagePoint,
    image: RectD,
    cursor: PointD,
    current: PointD,
    extent: SizeD,
    viewport: SizeD,
) -> ZoomViewportOffsets {
    let anchor_after = PointD {
        x: image.x + anchor.x * image.width,
        y: image.y + anchor.y * image.height,
    };
    calculate_offsets_from_anchor_delta(current, cursor, anchor_after, extent, viewport)
}

/// C# `CalculateFitWidthAnchorPoint(FitWidthAnchor)`: `TopThird` -> [`FIT_WIDTH_TOP_THIRD`], `BottomThird` ->
/// [`FIT_WIDTH_BOTTOM_THIRD`], `Centre` (and any other value) -> [`IMAGE_CENTRE`].
pub fn calculate_fit_width_anchor_point(anchor: FitWidthAnchor) -> ZoomImagePoint {
    match anchor {
        FitWidthAnchor::TopThird => FIT_WIDTH_TOP_THIRD,
        FitWidthAnchor::BottomThird => FIT_WIDTH_BOTTOM_THIRD,
        _ => IMAGE_CENTRE,
    }
}

/// C# `FitWidthAnchorFor(settings, type)`: the anchor a Fit width command uses. `second_command` is true for the command
/// C# calls `ReviewCommandType.FitWidth2` (the caller maps its command enum, see open question 2). That command uses
/// `FitWidthAnchor2` (read with `AppSettings::fit_width_anchor2_or_default`, C# default `BottomThird` when undefined); every
/// other Fit width command uses `FitWidthAnchor`. Never `KeyboardZoomAnchor`, which does not affect Fit width.
pub fn fit_width_anchor_for(settings: &AppSettings, second_command: bool) -> FitWidthAnchor {
    if second_command {
        settings.fit_width_anchor2_or_default()
    } else {
        settings.fit_width_anchor
    }
}

/// C# `CalculatePanOffsets(current H/V offset, deltaX, deltaY, extent, viewport)`: the pointer moved by `delta`, so the offset
/// moves by `-delta` (opposite to the drag), each axis clamped by [`clamp_offset`].
pub fn calculate_pan_offsets(current: PointD, delta: PointD, extent: SizeD, viewport: SizeD) -> ZoomViewportOffsets {
    ZoomViewportOffsets {
        horizontal: clamp_offset(current.x - delta.x, extent.width, viewport.width),
        vertical: clamp_offset(current.y - delta.y, extent.height, viewport.height),
    }
}

/// C# `IsBeyondDragThreshold(totalDeltaX, totalDeltaY, minimumHorizontal, minimumVertical)`: true when
/// `|totalDeltaX| >= minimumHorizontal` OR `|totalDeltaY| >= minimumVertical` (per axis, inclusive). NaN is false.
pub fn is_beyond_drag_threshold(
    total_delta_x: f64,
    total_delta_y: f64,
    minimum_horizontal: f64,
    minimum_vertical: f64,
) -> bool {
    total_delta_x.abs() >= minimum_horizontal || total_delta_y.abs() >= minimum_vertical
}

/// C# `ClampOffset(value, extent, viewport)`, private. `room = extent - viewport`; `maximum = room` when finite (clamped to
/// at least 0), else 0; a non-finite `value` becomes 0; the result is `value` clamped to `[0, maximum]`.
fn clamp_offset(value: f64, extent: f64, viewport: f64) -> f64 {
    let room = extent - viewport;
    let maximum = if room.is_finite() { room.max(0.0) } else { 0.0 };
    let value = if value.is_finite() { value } else { 0.0 };
    value.clamp(0.0, maximum)
}

/// C# `ClampToSource(value, max)`: NaN becomes 0 (a denormal scale makes `Infinity - Infinity` or `0 * Infinity`), else the
/// value is clamped to `[0, max]`. `max` is positive and finite here (the caller's guard), so `clamp` cannot panic.
fn clamp_to_source(value: f64, max: f64) -> f64 {
    if value.is_nan() { 0.0 } else { value.clamp(0.0, max) }
}
