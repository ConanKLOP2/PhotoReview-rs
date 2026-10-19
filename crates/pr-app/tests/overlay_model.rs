//! Red tests (W8-15a, test porter) for `pr_app::overlay::model::OverlayModel`: the snapshot the overlay panels draw from.
//!
//! Port of `PhotoReview.Shell.Tests/Overlay/OverlayPartsTests.cs::Model_DefaultsAreHidden` (baseline `d92d6aae`, the C# repo
//! at `C:/MyProjects/PhotoReview`) plus two F tests for the record equality of contract trap T1 (NaN and -0 in the font sizes)
//! and for the string and flag fields (marked "New test"). The C# `OverlayModel.From(MainViewModel)` is not ported here
//! (contract open question O2).
//!
//! Every test calls `OverlayModel::default()` or `==`, both of which reach a `todo!()` in the stub.

use pr_app::overlay::model::OverlayModel;

// C#: OverlayPartsTests.Model_DefaultsAreHidden
#[test]
fn model_defaults_are_hidden_with_12_and_11_point_fonts() {
    let model = OverlayModel::default();
    assert!(!model.has_images);
    assert!(!model.status_panel_visible);
    assert!(!model.folder_info_visible);
    assert!(!model.zoom_indicator_visible);
    assert!(!model.capture_pair_visible);
    assert_eq!(model.font_size, 12.0);
    assert_eq!(model.exif_font_size, 11.0);
    // New test: the rest of the defaults (contract semantic 9): every other flag is false, every string is empty.
    assert!(!model.status_needs_attention);
    assert!(!model.compare_visible);
    assert!(!model.file_info_visible);
    assert!(!model.exif_visible);
    assert!(!model.has_skipped_entries);
    assert_eq!(model.status_text, "");
    assert_eq!(model.exif_text, "");
    assert_eq!(model.skipped_warning_text, "");
    assert_eq!(model.folder_info_text, "");
    assert_eq!(model.zoom_indicator_text, "");
    assert_eq!(model.capture_pair_text, "");
    assert_eq!(model.capture_pair_tool_tip, "");
}

// New test: trap T1. C# record equality compares the font sizes with `double.Equals`: NaN equals NaN, `0.0` equals `-0.0`.
#[test]
fn model_equality_is_nan_aware_for_font_sizes() {
    let mut nan_a = OverlayModel::default();
    nan_a.font_size = f64::NAN;
    nan_a.exif_font_size = f64::NAN;
    let mut nan_b = OverlayModel::default();
    nan_b.font_size = f64::NAN;
    nan_b.exif_font_size = f64::NAN;
    assert!(
        nan_a == nan_b,
        "a NaN font size equals itself (a derived PartialEq would fail here)"
    );

    let mut zero = OverlayModel::default();
    zero.font_size = 0.0;
    let mut negative_zero = OverlayModel::default();
    negative_zero.font_size = -0.0;
    assert!(zero == negative_zero, "0.0 equals -0.0 under double.Equals");
}

// New test: the string and flag fields take part in the equality (a changed status text makes two models unequal).
#[test]
fn model_equality_compares_strings_and_flags() {
    let a = OverlayModel::default();
    let mut b = OverlayModel::default();
    assert!(a == b);
    b.status_text = "changed".to_string();
    assert!(a != b, "a changed status_text makes the models unequal");
    let mut c = OverlayModel::default();
    c.zoom_indicator_visible = true;
    assert!(a != c, "a changed flag makes the models unequal");
}
