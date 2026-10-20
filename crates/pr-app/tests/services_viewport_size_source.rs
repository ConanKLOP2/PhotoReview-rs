//! Tests of `pr_app::services::viewport_size_source` (contract `contracts/W8-04c.md`, rules R-VS-1..5).
//!
//! Port of `ViewportSizeSourceTests.cs` (`[Trait("Category", "HotPath")]`). The private packing bit patterns are unit tests inside
//! `viewport_size_source.rs` (`pack_unpack_matches_csharp_bits`).

use std::sync::Arc;

use pr_app::services::viewport_size_source::ViewportSizeSource;
use pr_imaging::decode_types::DecodeBox;

// C#: ViewportSizeSourceTests.TargetDecodeBox_Default_IsUnbounded
#[test]
fn target_default_is_unbounded() {
    let source = ViewportSizeSource::new();
    assert!(source.target_decode_box().is_unbounded());
}

// C#: ViewportSizeSourceTests.TargetDecodeBox_PublishesWidthAndHeightTogether (Theory, 4 cases)
#[test]
fn target_publishes_width_and_height_together() {
    for (width, height) in [(2304, 1280), (4000, 4000), (128, 0), (0, 1664)] {
        let source = ViewportSizeSource::new();
        source.set_target_decode_box(DecodeBox::new(width, height));
        assert_eq!(
            source.target_decode_box(),
            DecodeBox::new(width, height),
            "box ({width}, {height})"
        );
    }
}

// Contract test (R-VS-3): negative sizes round-trip through the packed form.
#[test]
fn negative_height_round_trips() {
    let source = ViewportSizeSource::new();
    source.set_target_decode_box(DecodeBox::new(0, -5));
    assert_eq!(source.target_decode_box(), DecodeBox::new(0, -5));
}

// Contract test (R-VS-4): no startup prediction by default.
#[test]
fn startup_default_is_none() {
    let source = ViewportSizeSource::new();
    assert_eq!(source.startup_prediction(), None);
}

// Contract test (R-VS-4): storing `None` reads back as `None`, and a stored box reads back unchanged.
#[test]
fn startup_none_round_trips() {
    let source = ViewportSizeSource::new();
    source.set_startup_prediction(Some(DecodeBox::new(1280, 768)));
    assert_eq!(source.startup_prediction(), Some(DecodeBox::new(1280, 768)));

    source.set_startup_prediction(None);
    assert_eq!(source.startup_prediction(), None);
}

// Contract test (R-VS-1): the viewport accessor reads `(0, 0)` until the window installs one.
#[test]
fn viewport_size_default_is_zero() {
    let source = ViewportSizeSource::new();
    assert_eq!(source.viewport_size(), (0.0, 0.0));
}

// Contract test (R-VS-5): installing a new accessor replaces the old one; the value is returned unchanged.
#[test]
fn set_viewport_size_fn_replaces_the_accessor() {
    let source = ViewportSizeSource::new();

    source.set_viewport_size_fn(Arc::new(|| (100.0, 50.0)));
    assert_eq!(source.viewport_size(), (100.0, 50.0));

    source.set_viewport_size_fn(Arc::new(|| (1.5, 2.5)));
    assert_eq!(source.viewport_size(), (1.5, 2.5));
}
