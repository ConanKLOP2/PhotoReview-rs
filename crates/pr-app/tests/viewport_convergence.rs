//! Red tests for `pr_app::misc::viewport_convergence` (contract W8-04e, `contracts/W8-04e.md` section "tests/viewport_convergence.rs").
//! Port of `PhotoReview.App.Shared/MainWindowConvergence.cs` (C# baseline d92d6aae), with the cases of
//! `tests/PhotoReview.App.Tests/ViewportConvergenceTests.cs` and `MainWindowSupportTests.cs`.
//!
//! Every test calls the stub, so each one fails through `todo!()` until the porter fills the bodies.

use pr_app::misc::viewport_convergence::{EPSILON, ValidationResult, is_stable_viewport, validate_measurements};
use pr_app::viewport::types::{ViewerStretchMode, ViewportSnapshot};

/// C# `CreateSnapshot` / `Snapshot` helpers: the defaults are a valid 800 x 600 viewport over an 800 x 600 extent.
#[derive(Clone, Copy)]
struct Parts {
    viewport: (f64, f64),
    extent: (f64, f64),
    offsets: (f64, f64),
    scrollbars: (bool, bool),
    max_image: (f64, f64),
    actual_image: (f64, f64),
}

impl Default for Parts {
    fn default() -> Self {
        Self {
            viewport: (800.0, 600.0),
            extent: (800.0, 600.0),
            offsets: (0.0, 0.0),
            scrollbars: (false, false),
            max_image: (800.0, 600.0),
            actual_image: (800.0, 600.0),
        }
    }
}

fn snapshot(parts: Parts) -> ViewportSnapshot {
    ViewportSnapshot {
        zoom: 1.0,
        stretch: ViewerStretchMode::Uniform,
        max_image_width: parts.max_image.0,
        max_image_height: parts.max_image.1,
        actual_image_width: parts.actual_image.0,
        actual_image_height: parts.actual_image.1,
        extent_width: parts.extent.0,
        extent_height: parts.extent.1,
        viewport_width: parts.viewport.0,
        viewport_height: parts.viewport.1,
        horizontal_offset: parts.offsets.0,
        vertical_offset: parts.offsets.1,
        horizontal_scroll_bar_visible: parts.scrollbars.0,
        vertical_scroll_bar_visible: parts.scrollbars.1,
    }
}

fn default_snapshot() -> ViewportSnapshot {
    snapshot(Parts::default())
}

// C#: ViewportConvergenceTests.IsStableViewport_SameSnapshot_ReturnsTrue
#[test]
fn is_stable_same_snapshot_is_true() {
    let s = default_snapshot();

    assert!(is_stable_viewport(&s, &s));
}

// C#: ViewportConvergenceTests.IsStableViewport_SubpixelDifference_ReturnsTrue (0.3 and 0.2 DIP, both below 0.5)
#[test]
fn is_stable_subpixel_difference_is_true() {
    let before = snapshot(Parts {
        viewport: (800.0, 600.0),
        ..Parts::default()
    });
    let after = snapshot(Parts {
        viewport: (800.3, 600.2),
        ..Parts::default()
    });

    assert!(is_stable_viewport(&before, &after));
}

// C#: ViewportConvergenceTests.IsStableViewport_SignificantViewportChange_ReturnsFalse
#[test]
fn is_stable_significant_viewport_change_is_false() {
    let before = snapshot(Parts {
        viewport: (800.0, 600.0),
        ..Parts::default()
    });
    let after = snapshot(Parts {
        viewport: (750.0, 600.0),
        ..Parts::default()
    });

    assert!(!is_stable_viewport(&before, &after));
}

// C#: ViewportConvergenceTests.IsStableViewport_ScrollbarVisibilityChanged_ReturnsFalse
#[test]
fn is_stable_scrollbar_visibility_change_is_false() {
    let before = snapshot(Parts {
        scrollbars: (false, false),
        ..Parts::default()
    });
    let after = snapshot(Parts {
        scrollbars: (true, false),
        ..Parts::default()
    });

    assert!(!is_stable_viewport(&before, &after));
}

// C#: ViewportConvergenceTests.ValidateMeasurements_ValidSnapshot_ReturnsValid
#[test]
fn validate_valid_snapshot_is_valid() {
    let s = snapshot(Parts {
        viewport: (800.0, 600.0),
        extent: (1000.0, 1200.0),
        offsets: (0.0, 0.0),
        ..Parts::default()
    });

    let result = validate_measurements(&s);

    assert!(result.is_valid);
}

// C#: ViewportConvergenceTests.ValidateMeasurements_InvalidViewportWidth_ReturnsInvalid (reason contains "Viewport")
#[test]
fn validate_invalid_viewport_width_is_invalid() {
    let s = snapshot(Parts {
        viewport: (-1.0, 600.0),
        ..Parts::default()
    });

    let result = validate_measurements(&s);

    assert!(!result.is_valid);
    assert!(
        result.reason.is_some_and(|r| r.contains("Viewport")),
        "reason was {:?}",
        result.reason
    );
}

// C#: ViewportConvergenceTests.ValidateMeasurements_NaNViewportHeight_ReturnsInvalid
#[test]
fn validate_nan_viewport_height_is_invalid() {
    let s = snapshot(Parts {
        viewport: (800.0, f64::NAN),
        ..Parts::default()
    });

    assert!(!validate_measurements(&s).is_valid);
}

// C#: ViewportConvergenceTests.ValidateMeasurements_NegativeOffset_ReturnsInvalid (reason contains "Offsets")
#[test]
fn validate_negative_offset_is_invalid() {
    let s = snapshot(Parts {
        offsets: (-1.0, 0.0),
        ..Parts::default()
    });

    let result = validate_measurements(&s);

    assert!(!result.is_valid);
    assert!(
        result.reason.is_some_and(|r| r.contains("Offsets")),
        "reason was {:?}",
        result.reason
    );
}

// C#: ViewportConvergenceTests.Convergence_ScrollbarDisappears_ViewportGrows (layout has not converged yet)
#[test]
fn convergence_scrollbar_disappears_is_not_yet_stable() {
    let before_zoom = snapshot(Parts {
        viewport: (784.0, 576.0),
        extent: (2000.0, 2000.0),
        scrollbars: (true, true),
        ..Parts::default()
    });
    let after_converge = snapshot(Parts {
        viewport: (800.0, 600.0),
        extent: (800.0, 600.0),
        scrollbars: (false, false),
        ..Parts::default()
    });

    assert!(!is_stable_viewport(&before_zoom, &after_converge));
}

/// One row of the first-invalid-group table: the eight measurements, then the expected reason.
type InvalidRow = (f64, f64, f64, f64, f64, f64, f64, f64, &'static str);

// C#: MainWindowSupportTests.ValidateMeasurements_ReportsTheFirstInvalidGroup (Theory, 8 rows, exact reason)
#[test]
fn validate_reports_first_invalid_group() {
    // (viewport w, viewport h, h offset, v offset, max w, max h, actual w, actual h, expected reason)
    let rows: [InvalidRow; 8] = [
        (
            f64::NAN,
            600.0,
            0.0,
            0.0,
            800.0,
            600.0,
            800.0,
            600.0,
            "Viewport not ready",
        ),
        (800.0, 0.0, 0.0, 0.0, 800.0, 600.0, 800.0, 600.0, "Viewport not ready"),
        (800.0, 600.0, -1.0, 0.0, 800.0, 600.0, 800.0, 600.0, "Offsets invalid"),
        (
            800.0,
            600.0,
            0.0,
            f64::INFINITY,
            800.0,
            600.0,
            800.0,
            600.0,
            "Offsets invalid",
        ),
        (800.0, 600.0, 0.0, 0.0, -1.0, 600.0, 800.0, 600.0, "MaxImage not set"),
        (
            800.0,
            600.0,
            0.0,
            0.0,
            800.0,
            f64::NAN,
            800.0,
            600.0,
            "MaxImage not set",
        ),
        (800.0, 600.0, 0.0, 0.0, 800.0, 600.0, -5.0, 600.0, "ActualImage invalid"),
        (
            800.0,
            600.0,
            0.0,
            0.0,
            800.0,
            600.0,
            800.0,
            f64::INFINITY,
            "ActualImage invalid",
        ),
    ];
    for (vw, vh, h_off, v_off, max_w, max_h, act_w, act_h, reason) in rows {
        let s = snapshot(Parts {
            viewport: (vw, vh),
            offsets: (h_off, v_off),
            max_image: (max_w, max_h),
            actual_image: (act_w, act_h),
            ..Parts::default()
        });

        let result = validate_measurements(&s);

        assert!(!result.is_valid, "row with reason {reason} must be invalid");
        assert_eq!(result.reason, Some(reason));
    }
}

// C#: MainWindowSupportTests.ValidateMeasurements_ZeroImageAndOffsetAreValid
#[test]
fn validate_zero_image_and_offset_are_valid() {
    let s = snapshot(Parts {
        max_image: (0.0, 0.0),
        actual_image: (0.0, 0.0),
        ..Parts::default()
    });

    let result = validate_measurements(&s);

    assert!(result.is_valid);
    assert_eq!(result.reason, None);
}

// C#: MainWindowSupportTests.IsStableViewport_IdenticalSnapshotsButAnInvalidAfter_IsNotStable
#[test]
fn is_stable_identical_but_invalid_after_is_not_stable() {
    let bad = snapshot(Parts {
        actual_image: (-1.0, 600.0),
        ..Parts::default()
    });

    assert!(!is_stable_viewport(&bad, &bad));
    assert!(is_stable_viewport(&default_snapshot(), &default_snapshot()));
}

// new: pins the strict `<` of the epsilon compare (contract semantic A.1): a difference of exactly 0.5 DIP is not stable.
#[test]
fn is_stable_difference_exactly_epsilon_is_not_stable() {
    let before = snapshot(Parts {
        viewport: (800.0, 600.0),
        ..Parts::default()
    });
    let after = snapshot(Parts {
        viewport: (800.0 + EPSILON, 600.0),
        ..Parts::default()
    });

    assert!(!is_stable_viewport(&before, &after));
}

// new: the epsilon itself is half a device-independent pixel (contract semantic A.1).
#[test]
fn epsilon_constant_is_half_a_dip() {
    assert_eq!(EPSILON, 0.5);
}

// new: zoom, stretch, offsets and image sizes are not compared (contract semantic A.2).
#[test]
fn is_stable_ignores_zoom_offsets_and_image_sizes() {
    let before = default_snapshot();
    let mut after = snapshot(Parts {
        offsets: (120.0, 40.0),
        max_image: (3000.0, 2000.0),
        actual_image: (2900.0, 1900.0),
        ..Parts::default()
    });
    after.zoom = 3.5;
    after.stretch = ViewerStretchMode::None;

    assert!(is_stable_viewport(&before, &after));
}

// new: an extent change of 0.6 DIP is a real change (contract semantic A.1, extent compared).
#[test]
fn is_stable_extent_change_beyond_epsilon_is_false() {
    let before = default_snapshot();
    let after = snapshot(Parts {
        extent: (800.0, 600.6),
        ..Parts::default()
    });

    assert!(!is_stable_viewport(&before, &after));
}

// new: `before` is not validated, but a NaN value in it fails the comparison (contract semantic A.1 and trap 10).
#[test]
fn is_stable_nan_before_is_not_stable() {
    let before = snapshot(Parts {
        viewport: (f64::NAN, 600.0),
        ..Parts::default()
    });
    let after = default_snapshot();

    assert!(!is_stable_viewport(&before, &after));
}

// new: group (a) wins over a later invalid group (contract trap 9): zero height fails even with a negative actual width.
#[test]
fn validate_first_group_wins_over_later_invalid_group() {
    let s = snapshot(Parts {
        viewport: (800.0, 0.0),
        actual_image: (-5.0, 600.0),
        ..Parts::default()
    });

    let result = validate_measurements(&s);

    assert_eq!(
        result,
        ValidationResult {
            is_valid: false,
            reason: Some("Viewport not ready")
        }
    );
}

// new: an infinite viewport is not finite and so not ready (contract group (a)).
#[test]
fn validate_infinite_viewport_width_is_invalid() {
    let s = snapshot(Parts {
        viewport: (f64::INFINITY, 600.0),
        ..Parts::default()
    });

    assert_eq!(validate_measurements(&s).reason, Some("Viewport not ready"));
}
