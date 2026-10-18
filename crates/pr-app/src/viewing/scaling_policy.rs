//! Port of `PhotoReview.Shell.Win32/Viewing/ScalingPolicy.cs` (46 lines, WP-21, baseline d92d6aae): chooses the interpolation
//! filter for each frame. Cubic (`HighQualityCubic`) costs 27 to 45 ms of GPU time per frame on a 24 MP downscale, so while the
//! image MOVES (pan or zoom) and the downscale is heavy the frame uses `Linear`; once still (`SETTLE_MS`) the viewer redraws
//! with the requested filter. A resting frame, a freshly swapped image or a light downscale always uses the requested filter.
//!
//! Stub: bodies are `todo!()`; signatures are final (contract W8-16a, Public API).

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::viewport::render_geometry::ImageInterpolation;

/// C# `HeavyDownscaleAreaRatio`: source pixel area over destination pixel area above which a cubic downscale is "heavy".
pub const HEAVY_DOWNSCALE_AREA_RATIO: f64 = 4.0;

/// C# `SettleMs`: milliseconds still before a frame is redrawn with the requested filter.
pub const SETTLE_MS: i32 = 120;

/// C# `ScalingPolicy.Choose`. Returns `requested` unless ALL of: `requested == HighQualityCubic`, `in_motion`, and
/// `is_heavy_downscale(...)`; then `Linear`.
pub fn choose(
    requested: ImageInterpolation,
    source_pixel_width: i32,
    source_pixel_height: i32,
    destination_pixel_width: f64,
    destination_pixel_height: f64,
    in_motion: bool,
) -> ImageInterpolation {
    todo!()
}

/// C# `ScalingPolicy.IsHeavyDownscale`: `true` when `(sw * sh) / (dw * dh) > HEAVY_DOWNSCALE_AREA_RATIO` (strictly greater).
/// Any non-positive source or destination side, or a NaN destination side, is `false`.
pub fn is_heavy_downscale(
    source_pixel_width: i32,
    source_pixel_height: i32,
    destination_pixel_width: f64,
    destination_pixel_height: f64,
) -> bool {
    todo!()
}
