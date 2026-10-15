//! Port of `PhotoReview.Imaging/AdaptivePreviewPolicy.cs` (C# baseline d92d6aae, PARITY I-05).
//!
//! The preview decode box: viewport (DIPs) x DPI x quality, long side raised to at least
//! [`MINIMUM_BOX_LONG_SIDE`], each side rounded UP to a multiple of [`BOX_QUANTUM`] and capped at
//! [`MAXIMUM_BOX_SIDE`]. The box is part of the preview cache key, so the arithmetic (IEEE `f64`, same operation
//! order as C#) must match bit for bit. Pure computation: no I/O, no clock, no globals.

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
    validate(viewport_width, "viewportWidth")?;
    validate(viewport_height, "viewportHeight")?;
    validate(dpi_scale, "dpiScale")?;
    validate(quality_multiplier, "qualityMultiplier")?;

    // Operation order is part of the contract: (vw * dpi) * quality, never vw * (dpi * quality).
    let mut width = viewport_width * dpi_scale * quality_multiplier;
    let mut height = viewport_height * dpi_scale * quality_multiplier;
    // Both operands are finite or +inf and never NaN here (all inputs are finite and > 0), so `max` matches Math.Max.
    let long_side = width.max(height);
    if long_side < f64::from(MINIMUM_BOX_LONG_SIDE) {
        // A zero `long_side` (underflow of both sides) gives scale = +inf and 0 * inf = NaN; quantize maps NaN to 0,
        // i.e. the UNBOUNDED box. This is the C# quirk kept on purpose (contract trap 3, PARITY I-05).
        let scale = f64::from(MINIMUM_BOX_LONG_SIDE) / long_side;
        width *= scale;
        height *= scale;
    }

    Ok(DecodeBox::new(quantize(width), quantize(height)))
}

/// C# `AdaptivePreviewPolicy.Quantize(side)`: `steps = max(1, ceil(side / 128 - 1e-9))`,
/// result `min(steps * 128, 4000)` truncated to `i32`. Total over all `f64` (see contract trap 2 for NaN).
pub fn quantize(side: f64) -> i32 {
    // C# `(int)Math.Min(NaN, ..)` is 0 on .NET 9+; Rust `max`/`min` would ignore NaN, so handle it before them.
    if side.is_nan() {
        return 0;
    }
    // The 1e-9 is subtracted from the quotient, not from `side` (contract semantics note 4).
    let steps = (side / f64::from(BOX_QUANTUM) - 1e-9).ceil().max(1.0);
    // `steps * 128` stays in f64 so huge sides do not overflow an i32; the value is in 128..=4000 before truncation.
    (steps * f64::from(BOX_QUANTUM)).min(f64::from(MAXIMUM_BOX_SIDE)) as i32
}

/// C# `Validate`: rejects NaN, +-Infinity, zero (including -0.0) and negatives, naming the first bad parameter.
fn validate(value: f64, param_name: &'static str) -> Result<(), InvalidPreviewArgument> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(InvalidPreviewArgument { param_name })
    }
}
