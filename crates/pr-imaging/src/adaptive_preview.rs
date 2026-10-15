//! Port of `PhotoReview.Imaging/AdaptivePreviewPolicy.cs` (C# baseline d92d6aae, PARITY I-05).
//!
//! The preview decode box: viewport (DIPs) x DPI x quality, long side raised to at least
//! [`MINIMUM_BOX_LONG_SIDE`], each side rounded UP to a multiple of [`BOX_QUANTUM`] and capped at
//! [`MAXIMUM_BOX_SIDE`]. The box is part of the preview cache key, so the arithmetic (IEEE `f64`, same operation
//! order as C#) must match bit for bit. Pure computation: no I/O, no clock, no globals.

#![allow(unused_variables, dead_code)] // D11: remove with the last `todo!()`.

use std::fmt;

/// The box's long side is scaled up (keeping the viewport's aspect) to at least this.
pub const MINIMUM_BOX_LONG_SIDE: i32 = 1200;
/// Each box side is capped here (applied after quantization).
pub const MAXIMUM_BOX_SIDE: i32 = 4000;
/// Box sides are rounded up to a multiple of this.
pub const BOX_QUANTUM: i32 = 128;
/// C# default argument value of `dpiScale`.
pub const DEFAULT_DPI_SCALE: f64 = 1.0;
/// C# default argument value of `qualityMultiplier`.
pub const DEFAULT_QUALITY_MULTIPLIER: f64 = 1.0;

use crate::decode_types::DecodeBox;

/// C# `ArgumentOutOfRangeException(paramName)`: the argument is not finite or not greater than zero.
/// `param_name` is the C# parameter name (`"viewportWidth"`, `"viewportHeight"`, `"dpiScale"`, `"qualityMultiplier"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidPreviewArgument {
    pub param_name: &'static str,
}

impl fmt::Display for InvalidPreviewArgument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} must be finite and greater than zero", self.param_name)
    }
}

impl std::error::Error for InvalidPreviewArgument {}

/// C# `AdaptivePreviewPolicy.CalculateTargetDecodeBox(viewportWidth, viewportHeight, dpiScale, qualityMultiplier)`.
///
/// Validation order (first failure wins): `viewport_width`, `viewport_height`, `dpi_scale`, `quality_multiplier`;
/// each must be finite and `> 0`. Then `w = vw * dpi * quality`, `h = vh * dpi * quality` (left to right),
/// `long = max(w, h)`; when `long < 1200` both are multiplied by `1200 / long`; result `(quantize(w), quantize(h))`.
pub fn calculate_target_decode_box(
    viewport_width: f64,
    viewport_height: f64,
    dpi_scale: f64,
    quality_multiplier: f64,
) -> Result<DecodeBox, InvalidPreviewArgument> {
    todo!("W4-05a")
}

/// C# `AdaptivePreviewPolicy.Quantize(side)`: `steps = max(1, ceil(side / 128 - 1e-9))`,
/// result `min(steps * 128, 4000)` truncated to `i32`. Total over all `f64` (see contract trap 2 for NaN).
pub fn quantize(side: f64) -> i32 {
    todo!("W4-05a")
}
