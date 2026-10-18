//! Red tests (W8-01a, test porter) for `pr_app::viewport::types`: `ViewportSnapshot` text, the .NET `F` format rule,
//! and the NaN-aware and `-0`-aware equality of the viewport structs (contract traps T1 and T5).
//!
//! Port of `PhotoReview.App.Shared/Viewport/ViewportSnapshot.cs`, `Viewport/ViewportContracts.cs` and
//! `ViewModels/ViewerStretchMode.cs` (baseline `d92d6aae`). Every test here fails through `todo!()` until the bodies land.

use pr_app::input::primitives::RectD;
use pr_app::viewport::types::{
    ScrollBarPolicy, ViewerStretchMode, ViewportInput, ViewportLayout, ViewportSnapshot, format_fixed,
};

fn layout_with_image_rect(image_rect: RectD) -> ViewportLayout {
    ViewportLayout {
        viewport_width: 800.0,
        viewport_height: 600.0,
        extent_width: 1600.0,
        extent_height: 1200.0,
        horizontal_bar_visible: true,
        vertical_bar_visible: true,
        image_rect,
        max_horizontal_offset: 800.0,
        max_vertical_offset: 600.0,
    }
}

fn input_with_client_width(client_width: f64) -> ViewportInput {
    ViewportInput {
        client_width,
        client_height: 600.0,
        scroll_bar_thickness: 17.0,
        scroll_bars: ScrollBarPolicy::Auto,
        stretch: ViewerStretchMode::Uniform,
        image_width: 1200.0,
        image_height: 900.0,
        max_image_width: 800.0,
        max_image_height: 600.0,
        bitmap_width: 1200.0,
        bitmap_height: 900.0,
    }
}

// C#: MainWindowSupportTests.ViewportSnapshot_ToString_ListsEveryMeasurementWithRoundedFormats
// The C# expectation uses the current culture (`{1.23:F2}`); the Rust text is pinned to the invariant culture
// (decision D-W8-01a-5, contract open question O5), which is the literal below.
#[test]
fn viewport_snapshot_to_string_lists_every_measurement_with_rounded_formats() {
    let snapshot = ViewportSnapshot {
        zoom: 1.234,
        stretch: ViewerStretchMode::Uniform,
        max_image_width: 100.4,
        max_image_height: 200.6,
        actual_image_width: 300.2,
        actual_image_height: 400.8,
        extent_width: 500.0,
        extent_height: 600.0,
        viewport_width: 700.0,
        viewport_height: 800.0,
        horizontal_offset: 1.26,
        vertical_offset: 2.34,
        horizontal_scroll_bar_visible: true,
        vertical_scroll_bar_visible: false,
    };

    assert_eq!(
        snapshot.to_string(),
        "Zoom=1.23 Stretch=Uniform MaxImage=(100,201) Actual=(300,401) Extent=(500,600) \
         Viewport=(700,800) Offset=(1.3,2.3) Scrollbars=(Visible,Collapsed)"
    );
}

// New test: C# has no direct test of `format_fixed`; the rule is .NET `ToString("F<n>")` (contract trap T5):
// exact binary value, rounded half away from zero on the decimal digits.
#[test]
fn format_fixed_rounds_halves_away_from_zero() {
    assert_eq!(format_fixed(0.125, 2), "0.13");
    assert_eq!(format_fixed(2.5, 0), "3");
    assert_eq!(format_fixed(-2.5, 0), "-3");
    assert_eq!(format_fixed(0.5, 0), "1");
    assert_eq!(format_fixed(0.25, 1), "0.3");
    // Rust `{:.2}` would print "0.12" for 0.125 (round half to even); .NET prints "0.13" (asserted above).
    assert_eq!(format_fixed(-0.125, 2), "-0.13");
}

// New test: a negative zero keeps its sign in `F` formats (contract trap T3 shows it in the snapshot text).
#[test]
fn format_fixed_keeps_sign_of_negative_zero() {
    assert_eq!(format_fixed(-0.0, 2), "-0.00");
    assert_eq!(format_fixed(-0.0, 0), "-0");
    assert_eq!(format_fixed(0.0, 1), "0.0");
    // A negative value that rounds to zero keeps its sign too (golden viewport-snapshot-text, zoom -0.001).
    assert_eq!(format_fixed(-0.001, 2), "-0.00");
}

// New test: C# record struct equality uses `double.Equals`: NaN equals NaN (contract trap T1).
#[test]
fn viewport_input_nan_fields_compare_equal() {
    let mut a = input_with_client_width(800.0);
    let mut b = input_with_client_width(800.0);
    a.image_width = f64::NAN;
    b.image_width = f64::NAN;

    assert!(a == b, "a NaN field must equal the same NaN field");

    b.image_width = 1200.0;
    assert!(a != b, "NaN against a number is not equal");
}

// New test: `0.0` equals `-0.0` in C# `double.Equals` (contract trap T1), so the two inputs are equal.
#[test]
fn viewport_input_negative_zero_equals_zero() {
    let mut a = input_with_client_width(0.0);
    let mut b = input_with_client_width(-0.0);
    a.scroll_bar_thickness = 0.0;
    b.scroll_bar_thickness = -0.0;

    assert!(a == b, "0.0 and -0.0 are equal in record struct equality");
}

// New test: `ViewportLayout` equality is NaN-aware in `image_rect` and compares the other fields as doubles.
#[test]
fn viewport_layout_equality_is_nan_aware() {
    let nan_rect = RectD {
        x: f64::NAN,
        y: 0.0,
        width: f64::NAN,
        height: 1200.0,
    };
    let a = layout_with_image_rect(nan_rect);
    let b = layout_with_image_rect(nan_rect);
    assert!(a == b, "NaN image rect fields must compare equal");

    let c = layout_with_image_rect(RectD {
        x: 0.0,
        y: 0.0,
        width: 1600.0,
        height: 1200.0,
    });
    let d = layout_with_image_rect(RectD {
        x: 0.0,
        y: 0.0,
        width: 1600.0000000000002,
        height: 1200.0,
    });
    assert!(c != d, "a changed image width is not equal");
}

// New test: `default(ViewportInput)` in C# is all zeros, `ScrollBarPolicy.Auto` and `ViewerStretchMode.None`
// (contract semantics note 8). The assertion goes through `PartialEq`, so it fails on the stub until that body lands.
#[test]
fn default_input_matches_csharp_default_struct() {
    let expected = ViewportInput {
        client_width: 0.0,
        client_height: 0.0,
        scroll_bar_thickness: 0.0,
        scroll_bars: ScrollBarPolicy::Auto,
        stretch: ViewerStretchMode::None,
        image_width: 0.0,
        image_height: 0.0,
        max_image_width: 0.0,
        max_image_height: 0.0,
        bitmap_width: 0.0,
        bitmap_height: 0.0,
    };
    assert!(
        ViewportInput::default() == expected,
        "default(ViewportInput) must be all zeros, Auto, None"
    );
    assert_eq!(ViewerStretchMode::default(), ViewerStretchMode::None);
    assert_eq!(ScrollBarPolicy::default(), ScrollBarPolicy::Auto);
}
