//! Port of `PhotoReview.App.Shared/Viewport/ViewportLayoutEngine.cs` (226 lines) at baseline `d92d6aae`, including the
//! private `ViewportLayoutMath`, `ImageElement` and `MeasuredSize` it uses. Target: `pr_app::viewport::layout` (W8-01b,
//! docs/WAVE-8-PLAN.md). Pure: no clock, no I/O, no `windows` types (AGENTS.md, WAVE-8-PLAN "Crates and layering").
//!
//! Stub: signatures are final, every body is `todo!()`. The porter fills bodies only.
//! Types from other units: `ViewportInput`, `ViewportLayout`, `ScrollBarPolicy`, `ViewerStretchMode` from W8-01a
//! (`viewport::types`), `RectD` from W8-05a (`input::primitives`). See contract.md, "Depends on" and open question 1.

#![allow(unused_variables, dead_code)] // stub: removed when the port lands (W7-01b convention)

use crate::input::primitives::RectD;
use crate::viewport::types::{ScrollBarPolicy, ViewportInput, ViewportLayout};

/// Port of `ViewportLayoutEngine.Compute` (C# lines 26-44).
///
/// Order of operations (must not change, the bars depend on the measured extent):
/// 1. sanitize client width and height (`sanitize_length`);
/// 2. `ImageElement::from_input`, then `measure` gives the desired (clamped) extent and the unclipped size;
/// 3. `resolve_bars` with the sanitized thickness gives the bar flags and the viewport size;
/// 4. `arrange` gives the image rectangle in extent coordinates (before the scroll offset is subtracted);
/// 5. `max_horizontal_offset = scrollable_length(desired.width, viewport_width)`, same for vertical.
pub fn compute(input: &ViewportInput) -> ViewportLayout {
    todo!("W8-01b: port ViewportLayoutEngine.Compute")
}

/// Port of `ViewportLayoutEngine.ClampOffset` (C# lines 51-52): `ScrollContentPresenter.CoerceOffset` on both axes.
/// NaN offsets read as 0 (inside `coerce_offset`).
pub fn clamp_offset(layout: &ViewportLayout, horizontal: f64, vertical: f64) -> (f64, f64) {
    todo!("W8-01b: port ViewportLayoutEngine.ClampOffset")
}

/// WPF `DoubleUtil` as used by the layout (C# `ViewportLayoutMath`, lines 61-130). Public so the integration tests
/// can port the four `DoubleUtil_*` C# tests directly; nothing outside the layout module needs it.
pub mod wpf_double {
    /// `DoubleUtil.DBL_EPSILON` = 2^-52 = Rust `f64::EPSILON`. Not `f64::MIN_POSITIVE`, and not .NET `double.Epsilon`
    /// (the smallest denormal).
    pub const WPF_DOUBLE_EPSILON: f64 = f64::EPSILON;

    /// `DoubleUtil.AreClose` (C# 121-127): equal values are close; otherwise
    /// `eps = (|a| + |b| + 10) * WPF_DOUBLE_EPSILON` and `-eps < a - b < eps` (strict on both edges).
    pub fn are_close(value1: f64, value2: f64) -> bool {
        todo!("W8-01b: DoubleUtil.AreClose")
    }

    /// `DoubleUtil.GreaterThan` (C# 118): `value1 > value2 && !are_close(value1, value2)`.
    pub fn greater_than(value1: f64, value2: f64) -> bool {
        todo!("W8-01b: DoubleUtil.GreaterThan")
    }

    /// `DoubleUtil.IsZero` (C# 130): `|value| < 10 * WPF_DOUBLE_EPSILON` (strict).
    pub fn is_zero(value: f64) -> bool {
        todo!("W8-01b: DoubleUtil.IsZero")
    }
}

/// .NET `Math.Max(a, b)` (.NET Core 3.0+). NaN propagates; +0 is treated as greater than -0.
/// Rust `f64::max` differs on both points (it ignores NaN and may return either zero), so the layout must not call it.
fn net_max(a: f64, b: f64) -> f64 {
    todo!("W8-01b: .NET Math.Max semantics")
}

/// .NET `Math.Min(a, b)` (.NET Core 3.0+). NaN propagates; -0 is treated as less than +0.
fn net_min(a: f64, b: f64) -> f64 {
    todo!("W8-01b: .NET Math.Min semantics")
}

/// `ViewportLayoutMath.CoerceOffset` (C# 64-70): NaN gives 0; first `if offset > extent - viewport` set it to
/// `extent - viewport`; then `if offset < 0` set it to 0. Plain comparisons, no `net_max`/`net_min`.
fn coerce_offset(offset: f64, extent: f64, viewport: f64) -> f64 {
    todo!("W8-01b: CoerceOffset")
}

/// `ViewportLayoutMath.ScrollableLength` (C# 72): `Math.Max(0.0, extent - viewport)`.
fn scrollable_length(extent: f64, viewport: f64) -> f64 {
    todo!("W8-01b: ScrollableLength")
}

/// `ViewportLayoutMath.SanitizeLength` (C# 133): finite and `> 0` keeps the value, everything else (NaN, negative,
/// infinite, zero, -0.0) becomes the literal `+0.0`.
fn sanitize_length(value: f64) -> f64 {
    todo!("W8-01b: SanitizeLength")
}

/// `ViewportLayoutMath.BarredLength` (C# 114-115): `max(0, client - thickness)` when the bar is visible, else `client`.
fn barred_length(client: f64, thickness: f64, bar_visible: bool) -> f64 {
    todo!("W8-01b: BarredLength")
}

/// `ViewportLayoutMath.ResolveBars` (C# 79-111). Returns `(horizontal_bar_visible, vertical_bar_visible,
/// viewport_width, viewport_height)`; C# `out` parameters become the tuple.
///
/// - `ScrollBarPolicy::Overlay` (C# 85-90): flags from `greater_than(extent, client)`, viewport = client, no thickness.
/// - `ScrollBarPolicy::Hidden` (C# 91-94): `(false, false)`, viewport = client.
/// - `ScrollBarPolicy::Auto` (C# 97-110): pass 1 flags from `greater_than(extent, client)`. Only when exactly one flag is
///   set, pass 2 recomputes the OTHER flag against the client narrowed by the visible bar's thickness. Then the viewport
///   is `barred_length` using the FINAL flags (C# 108-109), not the pass-1 flags.
/// - `Auto` falls through; there is no `Auto` arm that returns early.
///
/// `extent_*` is the desired (clamped) size from `ImageElement::measure`; `client_*` and `thickness` are already sanitized
/// by the caller.
fn resolve_bars(
    policy: ScrollBarPolicy,
    extent_width: f64,
    extent_height: f64,
    client_width: f64,
    client_height: f64,
    thickness: f64,
) -> (bool, bool, f64, f64) {
    todo!("W8-01b: ResolveBars")
}

/// `ViewportLayoutMath.ImageElement` (C# 140-223): a bitmap in an `Image` with `MaxWidth`/`MaxHeight`, `Width`/`Height`
/// (NaN means auto), `Stretch` (`uniform` = `Uniform`, `None` = Fill) and the natural size of the bitmap.
/// All fields are already sanitized by `from_input`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ImageElement {
    /// `Stretch == Uniform` (C# 144).
    uniform: bool,
    /// Explicit width: NaN = auto. Finite and `>= 0` kept, everything else NaN.
    width: f64,
    /// Explicit height: NaN = auto.
    height: f64,
    /// Max width: NaN or negative becomes `+inf`; otherwise kept (0 is a valid bound).
    max_width: f64,
    /// Max height, same rule as `max_width`.
    max_height: f64,
    /// Natural width of the bitmap (`sanitize_length`); 0 when there is no bitmap.
    natural_width: f64,
    /// Natural height (`sanitize_length`).
    natural_height: f64,
}

impl ImageElement {
    /// `ImageElement.From` (C# 143-147).
    fn from_input(input: &ViewportInput) -> Self {
        todo!("W8-01b: ImageElement.From")
    }

    /// `SanitizeExplicit` (C# 149): `is_finite && value >= 0` keeps the value (-0.0 passes), otherwise NaN.
    fn sanitize_explicit(value: f64) -> f64 {
        todo!("W8-01b: SanitizeExplicit")
    }

    /// `SanitizeMax` (C# 151): NaN or `< 0` becomes `+inf`, otherwise the value is kept.
    fn sanitize_max(value: f64) -> f64 {
        todo!("W8-01b: SanitizeMax")
    }

    /// `MaxW` (C# 154): `net_max(net_min(width or +inf when NaN, max_width), 0)`.
    fn max_w(&self) -> f64 {
        todo!("W8-01b: MaxW")
    }

    /// `MaxH` (C# 156).
    fn max_h(&self) -> f64 {
        todo!("W8-01b: MaxH")
    }

    /// `MinW` (C# 158): `net_max(net_min(max_w, width or 0 when NaN), 0)`.
    fn min_w(&self) -> f64 {
        todo!("W8-01b: MinW")
    }

    /// `MinH` (C# 160).
    fn min_h(&self) -> f64 {
        todo!("W8-01b: MinH")
    }

    /// `FrameworkElement.MeasureCore` with infinite available size (C# 166-175). Box = (`max_w`, `max_h`) through
    /// `stretch_to_box`; unclipped = `net_max(override, min)`; desired = `net_min(unclipped, max)`.
    fn measure(&self) -> MeasuredSize {
        todo!("W8-01b: ImageElement.Measure")
    }

    /// `ScrollContentPresenter.ArrangeOverride` + `FrameworkElement.ArrangeCore` + `Image.ArrangeOverride` (C# 181-192).
    /// Slot = `net_max(measured.width, viewport_width)` (same for height). The alignment ink is `net_min(render, max)`,
    /// NOT the render size itself (C# 189-190). Returns the rectangle in extent coordinates.
    fn arrange(&self, measured: MeasuredSize, viewport_width: f64, viewport_height: f64) -> RectD {
        todo!("W8-01b: ImageElement.Arrange")
    }

    /// `Image.MeasureArrangeHelper` + `Viewbox.ComputeScaleFactor` with `StretchDirection.Both` (C# 207-222).
    /// Returns `(natural_width * scale_x, natural_height * scale_y)`.
    /// - `IsZero(natural)` gives scale 0 (C# 215-216, `wpf_double::is_zero`, not `== 0`).
    /// - Only when a side is constrained (not `+inf`): one unconstrained side copies the other side's scale; both
    ///   constrained: `uniform` takes `min(scale_x, scale_y)`, otherwise each side keeps its own scale (Fill).
    fn stretch_to_box(&self, box_width: f64, box_height: f64) -> (f64, f64) {
        todo!("W8-01b: StretchToBox")
    }
}

/// `ArrangeLength` (C# 194-201): `length = slot`; `if length < unclipped` then `length = unclipped`;
/// `effective_max = max(unclipped, max)`; `if effective_max < length` then `length = effective_max`.
fn arrange_length(slot: f64, unclipped: f64, max: f64) -> f64 {
    todo!("W8-01b: ArrangeLength")
}

/// `AlignmentOffset` (C# 204): `ink > client ? 0 : (client - ink) * 0.5` (multiply by 0.5, not divide by 2).
fn alignment_offset(client: f64, ink: f64) -> f64 {
    todo!("W8-01b: AlignmentOffset")
}

/// `ViewportLayoutMath.MeasuredSize` (C# 225): desired (clamped) size and the unclipped size used for arranging.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MeasuredSize {
    width: f64,
    height: f64,
    unclipped_width: f64,
    unclipped_height: f64,
}
