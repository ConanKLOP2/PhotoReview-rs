//! Red tests for `pr_app::misc::window_helpers` (contract W8-04e, section "tests/window_helpers.rs"). Port of the pure math of
//! `PhotoReview.App.Shared/MainWindowHelpers.cs` (C# baseline d92d6aae), helper-level only: the gesture tests stay with
//! their owners (contract "Tests NOT ported here").
//!
//! Sources: `tests/PhotoReview.App.Tests/Input/MouseGesturesTests.cs` (ZoomToPoint), `ViewModels/ViewerMathPropertyTests.cs`
//! (seeded property tests) and `ViewModels/ViewerStateTests.cs` (helper tests). The property tests use the deterministic
//! generator below, so the values differ from C#; the properties are what is tested.
//!
//! Every test calls the stub, so each one fails through `todo!()` until the porter fills the bodies.

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::misc::window_helpers::{
    FIT_WIDTH_BOTTOM_THIRD, FIT_WIDTH_TOP_THIRD, IMAGE_CENTRE, ZoomImagePoint, ZoomViewportOffsets,
    calculate_fit_width_anchor_point, calculate_offsets_from_anchor_delta, calculate_pan_offsets,
    calculate_uniform_image_point, calculate_zoom_to_point_offsets, fit_width_anchor_for, is_beyond_drag_threshold,
    normalize_image_point,
};
use pr_core::model::enums::{FitWidthAnchor, KeyboardZoomAnchor};
use pr_core::settings::app_settings::AppSettings;

/// Tolerance of C# `Assert.Equal(expected, actual, 6)` (six decimal places).
const TOLERANCE: f64 = 1e-6;

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= TOLERANCE,
        "expected {expected}, got {actual}"
    );
}

fn offsets(horizontal: f64, vertical: f64) -> ZoomViewportOffsets {
    ZoomViewportOffsets { horizontal, vertical }
}

// ---- deterministic generator (xorshift64*) for the seeded property tests ----

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

/// C# `Finite(rng, scale)`: an ordinary value in `[0, scale)`.
fn finite(rng: &mut Rng, scale: f64) -> f64 {
    rng.unit() * scale
}

/// C# `Wild(rng, scale)`: mostly ordinary values, with a steady share of the values that break naive math.
fn wild(rng: &mut Rng, scale: f64) -> f64 {
    match rng.below(14) {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => f64::NEG_INFINITY,
        3 => 0.0,
        4 => -0.0,
        5 => f64::MAX,
        6 => f64::from_bits(1), // the smallest positive subnormal (C# double.Epsilon)
        7 => -rng.unit() * scale,
        _ => rng.unit() * scale,
    }
}

/// One row of the pan table: start offsets, drag delta, extent, viewport, expected offsets.
type PanRow = (f64, f64, f64, f64, f64, f64, f64, f64, f64, f64);

/// C# `ScrollableRange`: `max(0, extent - viewport)` when that is finite, else 0.
fn scrollable_range(extent: f64, viewport: f64) -> f64 {
    let room = extent - viewport;
    if room.is_finite() { room.max(0.0) } else { 0.0 }
}

fn assert_offsets_in_scrollable_range(o: ZoomViewportOffsets, extent_w: f64, extent_h: f64, view_w: f64, view_h: f64) {
    assert!(
        o.horizontal.is_finite() && o.vertical.is_finite(),
        "non-finite offset {o:?}"
    );
    assert!(
        (0.0..=scrollable_range(extent_w, view_w)).contains(&o.horizontal),
        "horizontal {} outside [0, {}]",
        o.horizontal,
        scrollable_range(extent_w, view_w)
    );
    assert!(
        (0.0..=scrollable_range(extent_h, view_h)).contains(&o.vertical),
        "vertical {} outside [0, {}]",
        o.vertical,
        scrollable_range(extent_h, view_h)
    );
}

// ---- zoom to point (C# MouseGesturesTests, ZoomToPoint_*) ----

// C#: MouseGesturesTests.ZoomToPoint_FromFit_KeepsTheImagePointUnderTheCursor
#[test]
fn zoom_to_point_from_fit_keeps_image_point_under_cursor() {
    let o = calculate_zoom_to_point_offsets(
        ZoomImagePoint { x: 0.25, y: 0.5 },
        RectD {
            x: 0.0,
            y: 0.0,
            width: 4000.0,
            height: 2000.0,
        },
        PointD { x: 300.0, y: 200.0 },
        PointD { x: 0.0, y: 0.0 },
        SizeD {
            width: 4000.0,
            height: 2000.0,
        },
        SizeD {
            width: 800.0,
            height: 600.0,
        },
    );

    assert_close(o.horizontal, 700.0); // 0.25 * 4000 - 300
    assert_close(o.vertical, 800.0); // 0.5 * 2000 - 200
}

// C#: MouseGesturesTests.ZoomToPoint_FromScrolledZoom_AccountsForTheCurrentOffsetAndClamps
#[test]
fn zoom_to_point_from_scrolled_zoom_accounts_for_offset_and_clamps() {
    // Already scrolled by (500, 100): the image origin sits at (-500, -100) in the viewport.
    let o = calculate_zoom_to_point_offsets(
        ZoomImagePoint { x: 0.9, y: 0.05 },
        RectD {
            x: -500.0,
            y: -100.0,
            width: 3000.0,
            height: 1500.0,
        },
        PointD { x: 700.0, y: 10.0 },
        PointD { x: 500.0, y: 100.0 },
        SizeD {
            width: 3000.0,
            height: 1500.0,
        },
        SizeD {
            width: 800.0,
            height: 600.0,
        },
    );

    assert_close(o.horizontal, 2000.0); // 0.9 * 3000 - 700
    assert_close(o.vertical, 65.0); // 0.05 * 1500 - 10
}

// C#: MouseGesturesTests.ZoomToPoint_SmallerThanViewport_StaysAtZero
#[test]
fn zoom_to_point_smaller_than_viewport_stays_at_zero() {
    let o = calculate_zoom_to_point_offsets(
        ZoomImagePoint { x: 0.5, y: 0.5 },
        RectD {
            x: 250.0,
            y: 200.0,
            width: 300.0,
            height: 200.0,
        },
        PointD { x: 400.0, y: 300.0 },
        PointD { x: 0.0, y: 0.0 },
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        SizeD {
            width: 800.0,
            height: 600.0,
        },
    );

    assert_close(o.horizontal, 0.0);
    assert_close(o.vertical, 0.0);
}

// C#: MouseGesturesTests.ZoomToPoint_PastTheFarEdge_ClampsToTheMaximumOffset
#[test]
fn zoom_to_point_past_far_edge_clamps_to_maximum_offset() {
    let o = calculate_zoom_to_point_offsets(
        ZoomImagePoint { x: 1.0, y: 1.0 },
        RectD {
            x: 0.0,
            y: 0.0,
            width: 2000.0,
            height: 1000.0,
        },
        PointD { x: 0.0, y: 0.0 },
        PointD { x: 0.0, y: 0.0 },
        SizeD {
            width: 2000.0,
            height: 1000.0,
        },
        SizeD {
            width: 800.0,
            height: 600.0,
        },
    );

    assert_close(o.horizontal, 1200.0);
    assert_close(o.vertical, 400.0);
}

// ---- seeded property tests (C# ViewerMathPropertyTests, Theory over seeds 1..8, 400 iterations each) ----

const SEEDS: std::ops::RangeInclusive<u64> = 1..=8;
const ITERATIONS: usize = 400;

// C#: ViewerMathPropertyTests.PanOffsets_AlwaysFiniteAndInsideTheScrollableRange
#[test]
fn pan_offsets_always_finite_and_inside_scrollable_range() {
    for seed in SEEDS {
        let mut rng = Rng::new(seed);
        for _ in 0..ITERATIONS {
            let extent_w = finite(&mut rng, 20000.0);
            let extent_h = finite(&mut rng, 20000.0);
            let view_w = finite(&mut rng, 4000.0);
            let view_h = finite(&mut rng, 4000.0);
            let current = PointD {
                x: wild(&mut rng, 5000.0),
                y: wild(&mut rng, 5000.0),
            };
            let delta = PointD {
                x: wild(&mut rng, 5000.0),
                y: wild(&mut rng, 5000.0),
            };

            let o = calculate_pan_offsets(
                current,
                delta,
                SizeD {
                    width: extent_w,
                    height: extent_h,
                },
                SizeD {
                    width: view_w,
                    height: view_h,
                },
            );

            assert_offsets_in_scrollable_range(o, extent_w, extent_h, view_w, view_h);
        }
    }
}

// C#: ViewerMathPropertyTests.OffsetClamp_WithNonFiniteExtentOrViewport_NeverEscapesOrProducesNaN
#[test]
fn offset_clamp_nonfinite_extent_or_viewport_never_escapes_or_nan() {
    for seed in SEEDS {
        let mut rng = Rng::new(seed);
        for _ in 0..ITERATIONS {
            let extent_w = wild(&mut rng, 5000.0);
            let extent_h = wild(&mut rng, 5000.0);
            let view_w = wild(&mut rng, 5000.0);
            let view_h = wild(&mut rng, 5000.0);
            let current = PointD {
                x: finite(&mut rng, 5000.0),
                y: finite(&mut rng, 5000.0),
            };
            let delta = PointD {
                x: finite(&mut rng, 200.0),
                y: finite(&mut rng, 200.0),
            };

            let o = calculate_pan_offsets(
                current,
                delta,
                SizeD {
                    width: extent_w,
                    height: extent_h,
                },
                SizeD {
                    width: view_w,
                    height: view_h,
                },
            );

            assert!(
                o.horizontal.is_finite() && o.vertical.is_finite(),
                "non-finite offset {o:?}"
            );
            assert!((0.0..=scrollable_range(extent_w, view_w)).contains(&o.horizontal));
            assert!((0.0..=scrollable_range(extent_h, view_h)).contains(&o.vertical));
        }
    }
}

// C#: ViewerMathPropertyTests.ZoomToPointOffsets_AlwaysFiniteAndInsideTheScrollableRange
#[test]
fn zoom_to_point_offsets_always_finite_and_inside_scrollable_range() {
    for seed in SEEDS {
        let mut rng = Rng::new(seed);
        for _ in 0..ITERATIONS {
            let extent_w = finite(&mut rng, 20000.0);
            let extent_h = finite(&mut rng, 20000.0);
            let view_w = finite(&mut rng, 4000.0);
            let view_h = finite(&mut rng, 4000.0);
            let anchor = ZoomImagePoint {
                x: wild(&mut rng, 2.0),
                y: wild(&mut rng, 2.0),
            };
            let image = RectD {
                x: wild(&mut rng, 5000.0),
                y: wild(&mut rng, 5000.0),
                width: wild(&mut rng, 5000.0),
                height: wild(&mut rng, 5000.0),
            };
            let cursor = PointD {
                x: wild(&mut rng, 5000.0),
                y: wild(&mut rng, 5000.0),
            };
            let current = PointD {
                x: wild(&mut rng, 5000.0),
                y: wild(&mut rng, 5000.0),
            };

            let o = calculate_zoom_to_point_offsets(
                anchor,
                image,
                cursor,
                current,
                SizeD {
                    width: extent_w,
                    height: extent_h,
                },
                SizeD {
                    width: view_w,
                    height: view_h,
                },
            );

            assert_offsets_in_scrollable_range(o, extent_w, extent_h, view_w, view_h);
        }
    }
}

// C#: ViewerMathPropertyTests.UniformImagePoint_StaysInsideTheSourceImage_ForAnyPointer
#[test]
fn uniform_image_point_inside_source_for_any_pointer() {
    for seed in SEEDS {
        let mut rng = Rng::new(seed);
        for _ in 0..ITERATIONS {
            let source_w = 1.0 + finite(&mut rng, 100000.0);
            let source_h = 1.0 + finite(&mut rng, 100000.0);
            let element = SizeD {
                width: wild(&mut rng, 5000.0),
                height: wild(&mut rng, 5000.0),
            };
            let pointer = PointD {
                x: wild(&mut rng, 8000.0),
                y: wild(&mut rng, 8000.0),
            };

            let p = calculate_uniform_image_point(
                element,
                SizeD {
                    width: source_w,
                    height: source_h,
                },
                pointer,
            );

            assert!(p.x.is_finite() && p.y.is_finite(), "non-finite point {p:?}");
            assert!((0.0..=source_w).contains(&p.x), "x {} outside [0, {source_w}]", p.x);
            assert!((0.0..=source_h).contains(&p.y), "y {} outside [0, {source_h}]", p.y);
        }
    }
}

// C#: ViewerMathPropertyTests.UniformImagePoint_TheElementCenterMapsToTheSourceCenter
#[test]
fn uniform_image_point_centre_maps_to_source_centre() {
    for seed in SEEDS {
        let mut rng = Rng::new(seed);
        for _ in 0..ITERATIONS {
            let element_w = 1.0 + finite(&mut rng, 4000.0);
            let element_h = 1.0 + finite(&mut rng, 4000.0);
            let source_w = 1.0 + finite(&mut rng, 100000.0);
            let source_h = 1.0 + finite(&mut rng, 100000.0);

            let p = calculate_uniform_image_point(
                SizeD {
                    width: element_w,
                    height: element_h,
                },
                SizeD {
                    width: source_w,
                    height: source_h,
                },
                PointD {
                    x: element_w / 2.0,
                    y: element_h / 2.0,
                },
            );

            assert_close(p.x, source_w / 2.0);
            assert_close(p.y, source_h / 2.0);
        }
    }
}

// C#: ViewerMathPropertyTests.NormalizeImagePoint_NeverReturnsNonFinite
#[test]
fn normalize_image_point_never_nonfinite() {
    for seed in SEEDS {
        let mut rng = Rng::new(seed);
        for _ in 0..ITERATIONS {
            let point = PointD {
                x: wild(&mut rng, 5000.0),
                y: wild(&mut rng, 5000.0),
            };
            let size = SizeD {
                width: wild(&mut rng, 5000.0),
                height: wild(&mut rng, 5000.0),
            };

            let p = normalize_image_point(point, size);

            assert!(p.x.is_finite() && p.y.is_finite(), "non-finite anchor {p:?}");
        }
    }
}

// ---- helper tests (C# ViewerStateTests, helper-level) ----

// C#: ViewerStateTests.UniformImagePoint_RemovesLetterboxAndMapsToSourceCoordinates
#[test]
fn uniform_image_point_removes_letterbox() {
    let p = calculate_uniform_image_point(
        SizeD {
            width: 1000.0,
            height: 800.0,
        },
        SizeD {
            width: 1000.0,
            height: 500.0,
        },
        PointD { x: 500.0, y: 400.0 },
    );

    assert_close(p.x, 500.0);
    assert_close(p.y, 250.0);
}

// C#: ViewerStateTests.AnchorDeltaOffsets_KeepSameImagePointAtPointer
#[test]
fn anchor_delta_offsets_keep_same_image_point_at_pointer() {
    let o = calculate_offsets_from_anchor_delta(
        PointD { x: 40.0, y: 30.0 },
        PointD { x: 300.0, y: 200.0 },
        PointD { x: 440.0, y: 290.0 },
        SizeD {
            width: 1600.0,
            height: 1200.0,
        },
        SizeD {
            width: 800.0,
            height: 600.0,
        },
    );

    assert_close(o.horizontal, 180.0);
    assert_close(o.vertical, 120.0);
}

// C#: ViewerStateTests.PanOffsets_MoveOppositeDragAndClampToExtent (Theory, 2 rows)
#[test]
fn pan_offsets_move_opposite_drag_and_clamp_to_extent() {
    // (horizontal, vertical, delta x, delta y, extent w, extent h, viewport w, viewport h, expected h, expected v)
    let rows: [PanRow; 2] = [
        (100.0, 80.0, 20.0, -15.0, 1000.0, 800.0, 400.0, 300.0, 80.0, 95.0),
        (0.0, 0.0, -1000.0, 1000.0, 1000.0, 800.0, 400.0, 300.0, 600.0, 0.0),
    ];
    for (h, v, dx, dy, ew, eh, vw, vh, expected_h, expected_v) in rows {
        let o = calculate_pan_offsets(
            PointD { x: h, y: v },
            PointD { x: dx, y: dy },
            SizeD { width: ew, height: eh },
            SizeD { width: vw, height: vh },
        );

        assert_close(o.horizontal, expected_h);
        assert_close(o.vertical, expected_v);
    }
}

// C#: ViewerStateTests.DragThreshold_IsPerAxisAbsoluteAndInclusive (Theory, 5 rows, threshold 4 on both axes)
#[test]
fn drag_threshold_is_per_axis_absolute_and_inclusive() {
    let rows: [(f64, f64, bool); 5] = [
        (0.0, 0.0, false),  // click without drag
        (3.9, -3.9, false), // below threshold on both axes
        (4.0, 0.0, true),   // horizontal threshold is inclusive
        (0.0, -4.0, true),  // vertical threshold, negative direction
        (-10.0, 2.0, true), // beyond on one axis only
    ];
    for (dx, dy, expected) in rows {
        assert_eq!(
            is_beyond_drag_threshold(dx, dy, 4.0, 4.0),
            expected,
            "delta ({dx}, {dy})"
        );
    }
}

// new: NaN deltas are never beyond the threshold (contract semantic B.12).
#[test]
fn drag_threshold_nan_is_not_beyond() {
    assert!(!is_beyond_drag_threshold(f64::NAN, f64::NAN, 4.0, 4.0));
}

// C#: ViewerStateTests.FitWidthAnchorPoint_Centre_IsImageCentre
#[test]
fn fit_width_anchor_point_centre_is_image_centre() {
    let point = calculate_fit_width_anchor_point(FitWidthAnchor::Centre);

    assert_close(point.x, 0.5);
    assert_close(point.y, 0.5);
}

// C#: ViewerStateTests.FitWidthAnchorPoint_TopThird_IsOneThirdDownTheImage
#[test]
fn fit_width_anchor_point_top_third_is_one_third_down() {
    let point = calculate_fit_width_anchor_point(FitWidthAnchor::TopThird);

    assert_close(point.x, 0.5);
    assert_close(point.y, 1.0 / 3.0);
}

// C#: ViewerStateTests.FitWidthAnchorPoint_BottomThird_IsTwoThirdsDownTheImage
#[test]
fn fit_width_anchor_point_bottom_third_is_two_thirds_down() {
    let point = calculate_fit_width_anchor_point(FitWidthAnchor::BottomThird);

    assert_close(point.x, 0.5);
    assert_close(point.y, 2.0 / 3.0);
}

// C#: ViewerStateTests.FitWidthAnchorFor_SecondCommandUsesAnchor2_EveryOtherCommandUsesAnchor1_NeverTheKeyboardZoomAnchor
#[test]
fn fit_width_anchor_for_second_command_uses_anchor2_never_keyboard_anchor() {
    // `second_command` replaces C# `ReviewCommandType.FitWidth2` (contract open question 2): true = FitWidth2.
    let settings = AppSettings {
        fit_width_anchor: FitWidthAnchor::TopThird,
        fit_width_anchor2: Some(FitWidthAnchor::BottomThird),
        keyboard_zoom_anchor: Some(KeyboardZoomAnchor::Pointer),
        ..AppSettings::default()
    };

    assert_eq!(fit_width_anchor_for(&settings, true), FitWidthAnchor::BottomThird);
    assert_eq!(fit_width_anchor_for(&settings, false), FitWidthAnchor::TopThird);
}

// new: the second command falls back to BottomThird when FitWidthAnchor2 is undefined (contract semantic B.10).
#[test]
fn fit_width_anchor_for_second_command_defaults_to_bottom_third_when_undefined() {
    let settings = AppSettings {
        fit_width_anchor2: None,
        ..AppSettings::default()
    };

    assert_eq!(fit_width_anchor_for(&settings, true), FitWidthAnchor::BottomThird);
}

// new: the anchor constants are the contract values (semantic B.9 and the image centre used by the "no anchor" callers).
#[test]
fn fit_width_anchor_constants_match_contract_values() {
    assert_close(FIT_WIDTH_TOP_THIRD.x, 0.5);
    assert_close(FIT_WIDTH_TOP_THIRD.y, 1.0 / 3.0);
    assert_close(FIT_WIDTH_BOTTOM_THIRD.x, 0.5);
    assert_close(FIT_WIDTH_BOTTOM_THIRD.y, 2.0 / 3.0);
    assert_eq!(IMAGE_CENTRE, ZoomImagePoint { x: 0.5, y: 0.5 });
}

// new: a non-positive element size returns the corner (0, 0), not the centre (contract trap 4).
#[test]
fn uniform_image_point_invalid_element_size_returns_corner() {
    let p = calculate_uniform_image_point(
        SizeD {
            width: 0.0,
            height: 100.0,
        },
        SizeD {
            width: 1000.0,
            height: 500.0,
        },
        PointD { x: 500.0, y: 250.0 },
    );

    assert_eq!(p, ZoomImagePoint { x: 0.0, y: 0.0 });
}

// new: a non-finite pointer also returns the corner (0, 0) (contract semantic B.5).
#[test]
fn uniform_image_point_nonfinite_pointer_returns_corner() {
    let p = calculate_uniform_image_point(
        SizeD {
            width: 1000.0,
            height: 800.0,
        },
        SizeD {
            width: 1000.0,
            height: 500.0,
        },
        PointD { x: f64::NAN, y: 400.0 },
    );

    assert_eq!(p, ZoomImagePoint { x: 0.0, y: 0.0 });
}

// new: normalize is NOT clamped (contract semantic B.6): a point past the image edge keeps its fraction.
#[test]
fn normalize_image_point_is_not_clamped() {
    let p = normalize_image_point(
        PointD { x: 2000.0, y: -100.0 },
        SizeD {
            width: 1000.0,
            height: 500.0,
        },
    );

    assert_close(p.x, 2.0);
    assert_close(p.y, -0.2);
}

// new: a zero or non-finite size gives the centre (contract semantic B.6).
#[test]
fn normalize_image_point_zero_size_returns_centre() {
    let p = normalize_image_point(
        PointD { x: 10.0, y: 10.0 },
        SizeD {
            width: 0.0,
            height: 10.0,
        },
    );

    assert_eq!(p, IMAGE_CENTRE);
}

// new: the finiteness check is on the quotient x / width, not on x (contract trap 3): 1e308 / 1e-300 overflows to infinity.
#[test]
fn normalize_image_point_checks_quotient_not_numerator() {
    let p = normalize_image_point(
        PointD { x: 1e308, y: 1.0 },
        SizeD {
            width: 1e-300,
            height: 1.0,
        },
    );

    assert_eq!(p, IMAGE_CENTRE);
}

// new: ZoomImagePoint equality follows C# EqualityComparer<double>.Default, so NaN equals NaN (contract semantic B.13).
#[test]
fn zoom_value_types_compare_nan_equal() {
    let a = ZoomImagePoint { x: f64::NAN, y: 0.0 };
    let b = ZoomImagePoint { x: f64::NAN, y: 0.0 };

    assert_eq!(a, b);
    assert_eq!(offsets(f64::NAN, 1.0), offsets(f64::NAN, 1.0));
}
