//! Mutation killers (M1, 2026-11-04) for `ViewportSnapshot::eq` in `pr_app::viewport::types`.
//! Hand-written, no golden. Each field is varied alone, so a `&&` turned into `||` at any position, or a constant `eq`, fails.

use pr_app::viewport::types::{ViewerStretchMode, ViewportSnapshot};

fn base() -> ViewportSnapshot {
    ViewportSnapshot {
        zoom: 1.5,
        stretch: ViewerStretchMode::None,
        max_image_width: 100.0,
        max_image_height: 101.0,
        actual_image_width: 102.0,
        actual_image_height: 103.0,
        extent_width: 104.0,
        extent_height: 105.0,
        viewport_width: 106.0,
        viewport_height: 107.0,
        horizontal_offset: 108.0,
        vertical_offset: 109.0,
        horizontal_scroll_bar_visible: false,
        vertical_scroll_bar_visible: false,
    }
}

#[test]
fn identical_snapshots_are_equal() {
    assert!(base() == base());
    assert!(!(base() != base()));
}

#[test]
fn snapshots_differing_in_exactly_one_field_are_not_equal() {
    let variants: Vec<(&str, ViewportSnapshot)> = vec![
        ("zoom", ViewportSnapshot { zoom: 2.5, ..base() }),
        (
            "stretch",
            ViewportSnapshot {
                stretch: ViewerStretchMode::Uniform,
                ..base()
            },
        ),
        (
            "max_image_width",
            ViewportSnapshot {
                max_image_width: 1.0,
                ..base()
            },
        ),
        (
            "max_image_height",
            ViewportSnapshot {
                max_image_height: 1.0,
                ..base()
            },
        ),
        (
            "actual_image_width",
            ViewportSnapshot {
                actual_image_width: 1.0,
                ..base()
            },
        ),
        (
            "actual_image_height",
            ViewportSnapshot {
                actual_image_height: 1.0,
                ..base()
            },
        ),
        (
            "extent_width",
            ViewportSnapshot {
                extent_width: 1.0,
                ..base()
            },
        ),
        (
            "extent_height",
            ViewportSnapshot {
                extent_height: 1.0,
                ..base()
            },
        ),
        (
            "viewport_width",
            ViewportSnapshot {
                viewport_width: 1.0,
                ..base()
            },
        ),
        (
            "viewport_height",
            ViewportSnapshot {
                viewport_height: 1.0,
                ..base()
            },
        ),
        (
            "horizontal_offset",
            ViewportSnapshot {
                horizontal_offset: 1.0,
                ..base()
            },
        ),
        (
            "vertical_offset",
            ViewportSnapshot {
                vertical_offset: 1.0,
                ..base()
            },
        ),
        (
            "horizontal_scroll_bar_visible",
            ViewportSnapshot {
                horizontal_scroll_bar_visible: true,
                ..base()
            },
        ),
        (
            "vertical_scroll_bar_visible",
            ViewportSnapshot {
                vertical_scroll_bar_visible: true,
                ..base()
            },
        ),
    ];
    for (name, other) in variants {
        assert!(base() != other, "differs only in {name}: must not be equal");
        assert!(other != base(), "differs only in {name} (reversed): must not be equal");
    }
}

#[test]
fn snapshot_equality_treats_nan_as_equal_and_signed_zero_as_equal() {
    let a = ViewportSnapshot {
        zoom: f64::NAN,
        horizontal_offset: 0.0,
        ..base()
    };
    let b = ViewportSnapshot {
        zoom: f64::NAN,
        horizontal_offset: -0.0,
        ..base()
    };
    assert!(a == b);
}
