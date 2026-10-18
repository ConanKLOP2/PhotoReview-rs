//! Port of `PhotoReview.App.Shared/MainWindowHelpers.cs` (C# `MainWindowHelpers`, baseline d92d6aae), without the two members listed
//! under "Not ported" in the contract: `CalculateClickLevelFromEffectiveZoom` (same function as `menus::zoom_rules`, W8-01d) and
//! `ForwardingFolderSink` (a WPF window forwarder, not a helper; the Rust composition passes the sink itself).
//! Unit W8-04e. Contract: `contracts/W8-04e.md`.
//!
//! Pure module: no `unsafe`, no `windows` crate. Every function is total: a non-finite or unusable input gives the C# fallback value,
//! never a panic (the C# `double` arithmetic never throws).

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::input::primitives::{PointD, RectD, SizeD};
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
        todo!()
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
        todo!()
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
    todo!()
}

/// C# `NormalizeImagePoint(x, y, width, height)`: expresses a point on the image as a fraction of its size, so the wheel anchor
/// survives a zoom step (the element's own size changes, not a `LayoutTransform`). NOT clamped: a pointer beside a centred,
/// smaller-than-viewport image keeps its old behaviour. Returns `(0.5, 0.5)` when `width` or `height` is not positive and finite,
/// or when `x / width` or `y / height` is not finite.
pub fn normalize_image_point(point: PointD, size: SizeD) -> ZoomImagePoint {
    todo!()
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
    todo!()
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
    todo!()
}

/// C# `CalculateFitWidthAnchorPoint(FitWidthAnchor)`: `TopThird` -> [`FIT_WIDTH_TOP_THIRD`], `BottomThird` ->
/// [`FIT_WIDTH_BOTTOM_THIRD`], `Centre` (and any other value) -> [`IMAGE_CENTRE`].
pub fn calculate_fit_width_anchor_point(anchor: FitWidthAnchor) -> ZoomImagePoint {
    todo!()
}

/// C# `FitWidthAnchorFor(settings, type)`: the anchor a Fit width command uses. `second_command` is true for the command
/// C# calls `ReviewCommandType.FitWidth2` (the caller maps its command enum, see open question 2). That command uses
/// `FitWidthAnchor2` (read with `AppSettings::fit_width_anchor2_or_default`, C# default `BottomThird` when undefined); every
/// other Fit width command uses `FitWidthAnchor`. Never `KeyboardZoomAnchor`, which does not affect Fit width.
pub fn fit_width_anchor_for(settings: &AppSettings, second_command: bool) -> FitWidthAnchor {
    todo!()
}

/// C# `CalculatePanOffsets(current H/V offset, deltaX, deltaY, extent, viewport)`: the pointer moved by `delta`, so the offset
/// moves by `-delta` (opposite to the drag), each axis clamped by [`clamp_offset`].
pub fn calculate_pan_offsets(current: PointD, delta: PointD, extent: SizeD, viewport: SizeD) -> ZoomViewportOffsets {
    todo!()
}

/// C# `IsBeyondDragThreshold(totalDeltaX, totalDeltaY, minimumHorizontal, minimumVertical)`: true when
/// `|totalDeltaX| >= minimumHorizontal` OR `|totalDeltaY| >= minimumVertical` (per axis, inclusive). NaN is false.
pub fn is_beyond_drag_threshold(
    total_delta_x: f64,
    total_delta_y: f64,
    minimum_horizontal: f64,
    minimum_vertical: f64,
) -> bool {
    todo!()
}

/// C# `ClampOffset(value, extent, viewport)`, private. `room = extent - viewport`; `maximum = room` when finite (clamped to
/// at least 0), else 0; a non-finite `value` becomes 0; the result is `value` clamped to `[0, maximum]`.
fn clamp_offset(value: f64, extent: f64, viewport: f64) -> f64 {
    todo!()
}
