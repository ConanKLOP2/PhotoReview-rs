//! Port of `PhotoReview.App.Shared/Viewport/ViewportLayoutEngine.cs` (226 lines) at baseline `d92d6aae`, including the
//! private `ViewportLayoutMath`, `ImageElement` and `MeasuredSize` it uses. Target: `pr_app::viewport::layout` (W8-01b,
//! docs/WAVE-8-PLAN.md). Pure: no clock, no I/O, no `windows` types, no allocation (AGENTS.md, WAVE-8-PLAN "Crates and layering").
//!
//! .NET `Math.Max`/`Math.Min` are NOT `f64::max`/`f64::min` (NaN propagation, `+0` above `-0`): every C# `Math.Max`/`Math.Min`
//! here is `net_max`/`net_min`. Operation order follows the C# source exactly so the results match bit for bit (golden test).

use crate::input::primitives::RectD;
use crate::viewport::types::{ScrollBarPolicy, ViewerStretchMode, ViewportInput, ViewportLayout};

/// Port of `ViewportLayoutEngine.Compute` (C# lines 26-44).
///
/// Order of operations (must not change, the bars depend on the measured extent):
/// 1. sanitize client width and height (`sanitize_length`);
/// 2. `ImageElement::from_input`, then `measure` gives the desired (clamped) extent and the unclipped size;
/// 3. `resolve_bars` with the sanitized thickness gives the bar flags and the viewport size;
/// 4. `arrange` gives the image rectangle in extent coordinates (before the scroll offset is subtracted);
/// 5. `max_horizontal_offset = scrollable_length(desired.width, viewport_width)`, same for vertical.
pub fn compute(input: &ViewportInput) -> ViewportLayout {
    let client_width = sanitize_length(input.client_width);
    let client_height = sanitize_length(input.client_height);
    let element = ImageElement::from_input(input);
    let desired = element.measure();

    let (horizontal_bar, vertical_bar, viewport_width, viewport_height) = resolve_bars(
        input.scroll_bars,
        desired.width,
        desired.height,
        client_width,
        client_height,
        sanitize_length(input.scroll_bar_thickness),
    );

    let image_rect = element.arrange(desired, viewport_width, viewport_height);
    ViewportLayout {
        viewport_width,
        viewport_height,
        extent_width: desired.width,
        extent_height: desired.height,
        horizontal_bar_visible: horizontal_bar,
        vertical_bar_visible: vertical_bar,
        image_rect,
        max_horizontal_offset: scrollable_length(desired.width, viewport_width),
        max_vertical_offset: scrollable_length(desired.height, viewport_height),
    }
}

/// Port of `ViewportLayoutEngine.ClampOffset` (C# lines 51-52): `ScrollContentPresenter.CoerceOffset` on both axes.
/// NaN offsets read as 0 (inside `coerce_offset`).
pub fn clamp_offset(layout: &ViewportLayout, horizontal: f64, vertical: f64) -> (f64, f64) {
    (
        coerce_offset(horizontal, layout.extent_width, layout.viewport_width),
        coerce_offset(vertical, layout.extent_height, layout.viewport_height),
    )
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
        if value1 == value2 {
            return true;
        }
        let eps = (value1.abs() + value2.abs() + 10.0) * WPF_DOUBLE_EPSILON;
        let delta = value1 - value2;
        -eps < delta && eps > delta
    }

    /// `DoubleUtil.GreaterThan` (C# 118): `value1 > value2 && !are_close(value1, value2)`.
    pub fn greater_than(value1: f64, value2: f64) -> bool {
        value1 > value2 && !are_close(value1, value2)
    }

    /// `DoubleUtil.IsZero` (C# 130): `|value| < 10 * WPF_DOUBLE_EPSILON` (strict).
    pub fn is_zero(value: f64) -> bool {
        value.abs() < 10.0 * WPF_DOUBLE_EPSILON
    }
}

/// .NET `Math.Max(a, b)` (.NET Core 3.0+). NaN propagates; +0 is treated as greater than -0.
/// Rust `f64::max` differs on both points (it ignores NaN and may return either zero), so the layout must not call it.
fn net_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        // Equal values: only the zeros differ in bits; pick +0.
        return if a.is_sign_positive() { a } else { b };
    }
    if a > b { a } else { b }
}

/// .NET `Math.Min(a, b)` (.NET Core 3.0+). NaN propagates; -0 is treated as less than +0.
fn net_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        // Equal values: only the zeros differ in bits; pick -0.
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b { a } else { b }
}

/// `ViewportLayoutMath.CoerceOffset` (C# 64-70): NaN gives 0; first `if offset > extent - viewport` set it to
/// `extent - viewport`; then `if offset < 0` set it to 0. Plain comparisons, no `net_max`/`net_min`.
fn coerce_offset(offset: f64, extent: f64, viewport: f64) -> f64 {
    if offset.is_nan() {
        return 0.0;
    }
    let mut offset = offset;
    if offset > extent - viewport {
        offset = extent - viewport;
    }
    if offset < 0.0 {
        offset = 0.0;
    }
    offset
}

/// `ViewportLayoutMath.ScrollableLength` (C# 72): `Math.Max(0.0, extent - viewport)`.
fn scrollable_length(extent: f64, viewport: f64) -> f64 {
    net_max(0.0, extent - viewport)
}

/// `ViewportLayoutMath.SanitizeLength` (C# 133): finite and `> 0` keeps the value, everything else (NaN, negative,
/// infinite, zero, -0.0) becomes the literal `+0.0`.
fn sanitize_length(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 { value } else { 0.0 }
}

/// `ViewportLayoutMath.BarredLength` (C# 114-115): `max(0, client - thickness)` when the bar is visible, else `client`.
fn barred_length(client: f64, thickness: f64, bar_visible: bool) -> f64 {
    if bar_visible {
        net_max(0.0, client - thickness)
    } else {
        client
    }
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
    match policy {
        ScrollBarPolicy::Overlay => {
            let horizontal = wpf_double::greater_than(extent_width, client_width);
            let vertical = wpf_double::greater_than(extent_height, client_height);
            return (horizontal, vertical, client_width, client_height);
        }
        ScrollBarPolicy::Hidden => return (false, false, client_width, client_height),
        ScrollBarPolicy::Auto => {}
    }

    // ScrollViewer.MeasureOverride, Auto/Auto. Pass 1: both bars collapsed, viewport = whole client.
    let mut horizontal = wpf_double::greater_than(extent_width, client_width);
    let mut vertical = wpf_double::greater_than(extent_height, client_height);
    // Pass 3 (only when exactly one bar is already visible): the appearance of one bar may make the other overflow.
    if horizontal != vertical {
        if !horizontal {
            horizontal = wpf_double::greater_than(extent_width, barred_length(client_width, thickness, true));
        } else {
            vertical = wpf_double::greater_than(extent_height, barred_length(client_height, thickness, true));
        }
    }

    let viewport_width = barred_length(client_width, thickness, vertical);
    let viewport_height = barred_length(client_height, thickness, horizontal);
    (horizontal, vertical, viewport_width, viewport_height)
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
        Self {
            uniform: input.stretch == ViewerStretchMode::Uniform,
            width: Self::sanitize_explicit(input.image_width),
            height: Self::sanitize_explicit(input.image_height),
            max_width: Self::sanitize_max(input.max_image_width),
            max_height: Self::sanitize_max(input.max_image_height),
            natural_width: sanitize_length(input.bitmap_width),
            natural_height: sanitize_length(input.bitmap_height),
        }
    }

    /// `SanitizeExplicit` (C# 149): `is_finite && value >= 0` keeps the value (-0.0 passes), otherwise NaN.
    fn sanitize_explicit(value: f64) -> f64 {
        if value.is_finite() && value >= 0.0 {
            value
        } else {
            f64::NAN
        }
    }

    /// `SanitizeMax` (C# 151): NaN or `< 0` becomes `+inf`, otherwise the value is kept.
    fn sanitize_max(value: f64) -> f64 {
        if value.is_nan() || value < 0.0 {
            f64::INFINITY
        } else {
            value
        }
    }

    /// `MaxW` (C# 154): `net_max(net_min(width or +inf when NaN, max_width), 0)`.
    fn max_w(&self) -> f64 {
        let width = if self.width.is_nan() { f64::INFINITY } else { self.width };
        net_max(net_min(width, self.max_width), 0.0)
    }

    /// `MaxH` (C# 156).
    fn max_h(&self) -> f64 {
        let height = if self.height.is_nan() {
            f64::INFINITY
        } else {
            self.height
        };
        net_max(net_min(height, self.max_height), 0.0)
    }

    /// `MinW` (C# 158): `net_max(net_min(max_w, width or 0 when NaN), 0)`.
    fn min_w(&self) -> f64 {
        let width = if self.width.is_nan() { 0.0 } else { self.width };
        net_max(net_min(self.max_w(), width), 0.0)
    }

    /// `MinH` (C# 160).
    fn min_h(&self) -> f64 {
        let height = if self.height.is_nan() { 0.0 } else { self.height };
        net_max(net_min(self.max_h(), height), 0.0)
    }

    /// `FrameworkElement.MeasureCore` with infinite available size (C# 166-175). Box = (`max_w`, `max_h`) through
    /// `stretch_to_box`; unclipped = `net_max(override, min)`; desired = `net_min(unclipped, max)`.
    fn measure(&self) -> MeasuredSize {
        let max_w = self.max_w();
        let max_h = self.max_h();
        let (override_width, override_height) = self.stretch_to_box(max_w, max_h);
        let unclipped_width = net_max(override_width, self.min_w());
        let unclipped_height = net_max(override_height, self.min_h());
        MeasuredSize {
            width: net_min(unclipped_width, max_w),
            height: net_min(unclipped_height, max_h),
            unclipped_width,
            unclipped_height,
        }
    }

    /// `ScrollContentPresenter.ArrangeOverride` + `FrameworkElement.ArrangeCore` + `Image.ArrangeOverride` (C# 181-192).
    /// Slot = `net_max(measured.width, viewport_width)` (same for height). The alignment ink is `net_min(render, max)`,
    /// NOT the render size itself (C# 189-190). Returns the rectangle in extent coordinates.
    fn arrange(&self, measured: MeasuredSize, viewport_width: f64, viewport_height: f64) -> RectD {
        let slot_width = net_max(measured.width, viewport_width);
        let slot_height = net_max(measured.height, viewport_height);
        let max_w = self.max_w();
        let max_h = self.max_h();
        let arrange_width = arrange_length(slot_width, measured.unclipped_width, max_w);
        let arrange_height = arrange_length(slot_height, measured.unclipped_height, max_h);
        let (render_width, render_height) = self.stretch_to_box(arrange_width, arrange_height);
        RectD {
            x: alignment_offset(slot_width, net_min(render_width, max_w)),
            y: alignment_offset(slot_height, net_min(render_height, max_h)),
            width: render_width,
            height: render_height,
        }
    }

    /// `Image.MeasureArrangeHelper` + `Viewbox.ComputeScaleFactor` with `StretchDirection.Both` (C# 207-222).
    /// Returns `(natural_width * scale_x, natural_height * scale_y)`.
    /// - `IsZero(natural)` gives scale 0 (C# 215-216, `wpf_double::is_zero`, not `== 0`).
    /// - Only when a side is constrained (not `+inf`): one unconstrained side copies the other side's scale; both
    ///   constrained: `uniform` takes `min(scale_x, scale_y)`, otherwise each side keeps its own scale (Fill).
    fn stretch_to_box(&self, box_width: f64, box_height: f64) -> (f64, f64) {
        let mut scale_x = 1.0;
        let mut scale_y = 1.0;
        let constrained_width = box_width != f64::INFINITY;
        let constrained_height = box_height != f64::INFINITY;
        if constrained_width || constrained_height {
            scale_x = if wpf_double::is_zero(self.natural_width) {
                0.0
            } else {
                box_width / self.natural_width
            };
            scale_y = if wpf_double::is_zero(self.natural_height) {
                0.0
            } else {
                box_height / self.natural_height
            };
            if !constrained_width {
                scale_x = scale_y;
            } else if !constrained_height {
                scale_y = scale_x;
            } else if self.uniform {
                let scale = net_min(scale_x, scale_y);
                scale_x = scale;
                scale_y = scale;
            }
        }
        (self.natural_width * scale_x, self.natural_height * scale_y)
    }
}

/// `ArrangeLength` (C# 194-201): `length = slot`; `if length < unclipped` then `length = unclipped`;
/// `effective_max = max(unclipped, max)`; `if effective_max < length` then `length = effective_max`.
fn arrange_length(slot: f64, unclipped: f64, max: f64) -> f64 {
    let mut length = slot;
    if length < unclipped {
        length = unclipped;
    }
    let effective_max = net_max(unclipped, max);
    if effective_max < length {
        length = effective_max;
    }
    length
}

/// `AlignmentOffset` (C# 204): `ink > client ? 0 : (client - ink) * 0.5` (multiply by 0.5, not divide by 2).
fn alignment_offset(client: f64, ink: f64) -> f64 {
    if ink > client { 0.0 } else { (client - ink) * 0.5 }
}

/// `ViewportLayoutMath.MeasuredSize` (C# 225): desired (clamped) size and the unclipped size used for arranging.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MeasuredSize {
    width: f64,
    height: f64,
    unclipped_width: f64,
    unclipped_height: f64,
}
