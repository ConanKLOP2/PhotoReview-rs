//! Port of `tests/PhotoReview.App.Tests/Input/InputPrimitivesTests.cs` (unit W8-05a, contract `contracts/W8-05a.md`).
//!
//! The code under test is `input::primitives` (W8-01a) and is already implemented, so these tests pass on the stub.
//! `Contains` expectations are written by hand from the closed-interval rule (contract Semantics 1), not read from WPF.
//!
//! Not ported: `WpfInputAdapters_AreLossless` (the WPF adapters are retired; the `KeyId` part belongs to W8-05b/05c).

use pr_app::input::primitives::{KeyModifiers, PointD, PointerButton, RectD};

/// Closed-interval rule written by hand: a negative width or height is empty, otherwise all four edges are inside.
/// Every origin, size and probe used below is exactly representable, so `x + width` has no rounding.
fn closed_interval_rule(x: f64, y: f64, width: f64, height: f64, px: f64, py: f64) -> bool {
    if width < 0.0 || height < 0.0 {
        return false;
    }
    px >= x && px <= x + width && py >= y && py <= y + height
}

// C#: InputPrimitivesTests.Contains_MatchesWpfRect
#[test]
fn rect_contains_matches_closed_wpf_rule_on_edges_and_corners() {
    let origins = [-5.0, 0.0, 10.5];
    let sizes = [0.0, 0.5, 20.0];
    let probes = [-6.0, -5.0, -0.0001, 0.0, 5.0, 10.5, 15.5, 30.5, 31.0, 100.0];
    let mut checked = 0_usize;
    for &x in &origins {
        for &y in &origins {
            for &width in &sizes {
                for &height in &sizes {
                    for &px in &probes {
                        for &py in &probes {
                            let expected = closed_interval_rule(x, y, width, height, px, py);
                            let actual = RectD { x, y, width, height }.contains(PointD { x: px, y: py });
                            assert_eq!(actual, expected, "rect ({x},{y},{width},{height}) point ({px},{py})");
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(checked > 2000, "checked only {checked} cases");
}

// C#: InputPrimitivesTests.Contains_NegativeSize_IsEmpty (3 InlineData rows, one test here)
#[test]
fn rect_with_negative_size_contains_no_point() {
    for (width, height) in [(-1.0, 10.0), (10.0, -1.0), (-1.0, -1.0)] {
        let rect = RectD {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        assert!(
            !rect.contains(PointD { x: 0.0, y: 0.0 }),
            "width {width} height {height}: origin must be outside"
        );
        assert!(
            !rect.contains(PointD { x: -0.5, y: -0.5 }),
            "width {width} height {height}: point before origin must be outside"
        );
    }
}

// C#: InputPrimitivesTests.RightAndBottom_AreTheFarEdges
#[test]
fn right_and_bottom_are_the_far_edges() {
    let rect = RectD {
        x: 10.0,
        y: 20.0,
        width: 30.0,
        height: 40.0,
    };
    assert_eq!(rect.right(), 40.0);
    assert_eq!(rect.bottom(), 60.0);
}

// new: the numeric values of the WPF enums (`MouseButton`, `ModifierKeys`) that the shell adapters rely on.
#[test]
fn pointer_button_and_key_modifier_values_match_wpf() {
    let buttons = [
        PointerButton::Left,
        PointerButton::Middle,
        PointerButton::Right,
        PointerButton::XButton1,
        PointerButton::XButton2,
    ];
    for (index, button) in buttons.iter().enumerate() {
        assert_eq!(*button as usize, index, "PointerButton ordinal of {button:?}");
    }

    assert_eq!(KeyModifiers::ALT.0, 1);
    assert_eq!(KeyModifiers::CONTROL.0, 2);
    assert_eq!(KeyModifiers::SHIFT.0, 4);
    assert_eq!(KeyModifiers::WINDOWS.0, 8);
    assert!(KeyModifiers::ALT.contains(KeyModifiers::NONE));
    assert_eq!(
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        KeyModifiers(6),
        "BitOr combines the raw bits"
    );
}
