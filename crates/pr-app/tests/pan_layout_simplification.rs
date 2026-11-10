//! P-395 / task 3B part 2: `arrow_pan::step` and `wpf_double::greater_than` were rewritten with `partial_cmp(..).is_some_and(..)`
//! instead of `<` / `>` so that no boundary operator is left to mutate. These differential tests pin the new code to the old
//! operator form bit for bit (NaN, +-0.0, +-inf, subnormals, ties) on a value lattice. The old forms are copied below.

use pr_app::input::primitives::RectD;
use pr_app::viewport::arrow_pan::{ArrowPanOutcome, step, step_length};
use pr_app::viewport::layout::wpf_double::{are_close, greater_than};
use pr_app::viewport::types::ViewportLayout;

fn pool() -> Vec<f64> {
    let mut v = vec![
        0.0,
        -0.0,
        0.25,
        -0.25,
        0.5,
        -0.5,
        0.5000000000000001,
        0.4999999999999999,
        1.0,
        -1.0,
        2.0,
        10.0,
        -10.0,
        100.0,
        1e6,
        -1e6,
        1e300,
        -1e300,
        f64::MAX,
        f64::MIN,
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        5e-324,
        -5e-324,
        f64::EPSILON,
        -f64::EPSILON,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ];
    let base = [1.0_f64, 0.5, 10.0, 123.456, 1e10];
    for b in base {
        let mut up = b;
        let mut down = b;
        for _ in 0..4 {
            up = f64::from_bits(up.to_bits() + 1);
            down = f64::from_bits(down.to_bits() - 1);
            v.extend([up, down, -up, -down]);
        }
    }
    v
}

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

fn layout(vw: f64, vh: f64, max_h: f64, max_v: f64) -> ViewportLayout {
    ViewportLayout {
        viewport_width: vw,
        viewport_height: vh,
        extent_width: 0.0,
        extent_height: 0.0,
        horizontal_bar_visible: true,
        vertical_bar_visible: true,
        image_rect: RectD::default(),
        max_horizontal_offset: max_h,
        max_vertical_offset: max_v,
    }
}

/// The pre-change `step`, operators as they were.
fn old_step(
    dx: i32,
    dy: i32,
    horizontal: f64,
    vertical: f64,
    l: &ViewportLayout,
    frac: f64,
) -> (ArrowPanOutcome, f64, f64) {
    let horizontal_axis = dx != 0;
    let (current, max) = if horizontal_axis {
        (horizontal, l.max_horizontal_offset)
    } else {
        (vertical, l.max_vertical_offset)
    };
    if max <= 0.5 {
        return (ArrowPanOutcome::NotScrollable, horizontal, vertical);
    }
    let length = step_length(l.viewport_width, l.viewport_height, frac);
    let moved = current + length * f64::from(dx.wrapping_add(dy));
    let target = if moved < 0.0 {
        0.0
    } else if moved > max {
        max
    } else {
        moved
    };
    if (target - current).abs() < 0.5 {
        return (ArrowPanOutcome::AtEdge, horizontal, vertical);
    }
    if horizontal_axis {
        (ArrowPanOutcome::Panned, target, vertical)
    } else {
        (ArrowPanOutcome::Panned, horizontal, target)
    }
}

#[test]
fn step_equals_the_operator_form_bit_for_bit_on_the_lattice() {
    let p = pool();
    let small: Vec<f64> = vec![0.0, -0.0, 0.1, 1.0, 100.0, 1e300, f64::NAN, f64::INFINITY, -1.0];
    let dirs = [(-1, 0), (1, 0), (0, -1), (0, 1), (0, 0), (i32::MIN, 0), (i32::MAX, 1)];
    let mut n = 0u64;
    for &cur in &p {
        for &max in &p {
            for &vw in &small {
                for &frac in &small {
                    for &(dx, dy) in &dirs {
                        let (h, v, mh, mv) = if dx != 0 {
                            (cur, 7.0, max, 3.0)
                        } else {
                            (7.0, cur, 3.0, max)
                        };
                        let l = layout(vw, 480.0, mh, mv);
                        let a = step(dx, dy, h, v, &l, frac);
                        let b = old_step(dx, dy, h, v, &l, frac);
                        assert!(
                            a.0 == b.0 && same(a.1, b.1) && same(a.2, b.2),
                            "cur {cur:?} max {max:?} vw {vw:?} frac {frac:?} dir {dx},{dy}"
                        );
                        n += 1;
                    }
                }
            }
        }
    }
    assert!(n > 500_000, "{n}");
}

#[test]
fn step_golden_cases() {
    let l = layout(1000.0, 500.0, 300.0, 300.0);
    // step length 50
    assert_eq!(step(1, 0, 10.0, 0.0, &l, 0.1), (ArrowPanOutcome::Panned, 60.0, 0.0));
    assert_eq!(step(-1, 0, 10.0, 0.0, &l, 0.1), (ArrowPanOutcome::Panned, 0.0, 0.0));
    assert_eq!(step(-1, 0, 0.0, 0.0, &l, 0.1), (ArrowPanOutcome::AtEdge, 0.0, 0.0));
    assert_eq!(step(1, 0, 280.0, 0.0, &l, 0.1), (ArrowPanOutcome::Panned, 300.0, 0.0));
    assert_eq!(step(1, 0, 300.0, 0.0, &l, 0.1), (ArrowPanOutcome::AtEdge, 300.0, 0.0));
    let small = layout(1000.0, 500.0, 0.5, 0.5);
    assert_eq!(
        step(1, 0, 0.0, 0.0, &small, 0.1),
        (ArrowPanOutcome::NotScrollable, 0.0, 0.0)
    );
    // NaN max reaches the clamp and is never selected.
    let nan_max = layout(1000.0, 500.0, f64::NAN, 0.0);
    let r = step(1, 0, 10.0, 0.0, &nan_max, 0.1);
    assert!(r.0 == ArrowPanOutcome::Panned && r.1 == 60.0);
}

fn old_greater_than(a: f64, b: f64) -> bool {
    a > b && !are_close(a, b)
}

#[test]
fn greater_than_equals_the_operator_form_on_the_lattice() {
    let p = pool();
    let mut n = 0u64;
    for &a in &p {
        for &b in &p {
            assert_eq!(greater_than(a, b), old_greater_than(a, b), "{a:?} {b:?}");
            n += 1;
        }
    }
    assert!(n > 1000);
    assert!(!greater_than(0.0, -0.0) && !greater_than(-0.0, 0.0));
    assert!(!greater_than(f64::NAN, 1.0) && !greater_than(1.0, f64::NAN));
    assert!(greater_than(2.0, 1.0) && !greater_than(1.0, 2.0) && !greater_than(1.0, 1.0));
}
