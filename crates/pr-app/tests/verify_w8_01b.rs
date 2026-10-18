//! Adversarial mutation-kill tests for W8-01b. Each kills a cargo-mutants survivor that the
//! committed tests/goldens miss; expected values follow the C# semantics at d92d6aae (NaN-propagating Math.Max/Min,
//! `value > 0 ? value : 0` giving +0, CoerceOffset leaving -0 untouched).

use pr_app::viewport::layout::{clamp_offset, compute};
use pr_app::viewport::types::{ScrollBarPolicy, ViewerStretchMode, ViewportInput};

fn base() -> ViewportInput {
    ViewportInput {
        client_width: 100.0,
        client_height: 100.0,
        scroll_bar_thickness: 10.0,
        scroll_bars: ScrollBarPolicy::Auto,
        stretch: ViewerStretchMode::None,
        image_width: f64::NAN,
        image_height: f64::NAN,
        max_image_width: f64::INFINITY,
        max_image_height: f64::INFINITY,
        bitmap_width: 50.0,
        bitmap_height: 50.0,
    }
}

/// natural width 0 times an overflowed (inf) shared scale = NaN; .NET Math.Max/Min propagate it into the extent.
/// Kills `||`->`&&` in net_max (first operand NaN) and net_min (first operand NaN).
#[test]
fn nan_override_propagates_through_net_max_and_net_min() {
    let mut i = base();
    i.bitmap_width = 0.0;
    i.bitmap_height = 3e-15; // not IsZero (>= 10*eps), so the scale is box/natural = 1e308/3e-15 = +inf
    i.max_image_height = 1e308;
    let l = compute(&i);
    assert!(l.extent_width.is_nan(), "extent_width = {}", l.extent_width);
}

/// Kills `>`->`>=` in coerce_offset (both comparisons): -0.0 must stay -0.0 (C# 67-68 are plain comparisons).
#[test]
fn clamp_offset_keeps_negative_zero() {
    let mut i = base();
    i.bitmap_width = 100.0; // extent == viewport -> extent - viewport = +0.0
    i.bitmap_height = 100.0;
    let l = compute(&i);
    let (h, v) = clamp_offset(&l, -0.0, -0.0);
    assert_eq!(h.to_bits(), (-0.0f64).to_bits());
    assert_eq!(v.to_bits(), (-0.0f64).to_bits());
}

/// Kills `>`->`>=` in sanitize_length: -0.0 client becomes the literal +0.0 (C# `value > 0 ? value : 0`).
#[test]
fn sanitize_length_negative_zero_becomes_positive_zero() {
    let mut i = base();
    i.client_width = -0.0;
    i.client_height = -0.0;
    let l = compute(&i);
    assert_eq!(l.viewport_width.to_bits(), 0.0f64.to_bits());
    assert_eq!(l.viewport_height.to_bits(), 0.0f64.to_bits());
}
