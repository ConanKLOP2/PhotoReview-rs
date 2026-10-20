use pr_app::input::primitives::RectD;
use pr_app::viewport::arrow_pan::{step, step_length};
use pr_app::viewport::types::{ViewerStretchMode, ViewportLayout, ViewportSnapshot, format_fixed};
const D: &str = "C:/Users/adminvti/AppData/Local/Temp/claude/C--MyProjects-PhotoReview-rs/95c30150-ba6c-40c7-91dc-fcb1e52da422/scratchpad/orc";
fn f(h: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(h, 16).unwrap())
}
fn rd(n: &str) -> String {
    std::fs::read_to_string(format!("{D}/{n}")).unwrap()
}

#[test]
#[ignore = "needs oracle files in a local scratchpad directory (D), not available on CI"]
fn oracle_format_fixed() {
    let mut bad = vec![];
    for l in rd("fmt.txt").lines() {
        let p: Vec<&str> = l.split(' ').collect();
        let v = f(p[0]);
        for (i, d) in [0usize, 1, 2].iter().enumerate() {
            let got = format_fixed(v, *d);
            if got != p[1 + i] {
                bad.push(format!("{} F{} rust={} cs={}", v, d, got, p[1 + i]));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} diffs, first: {:?}",
        bad.len(),
        &bad[..bad.len().min(10)]
    );
}

#[test]
#[ignore = "needs oracle files in a local scratchpad directory (D), not available on CI"]
fn oracle_snapshot_text() {
    let mut bad = vec![];
    for l in rd("snap.txt").lines() {
        let (a, text) = l.split_once('|').unwrap();
        let p: Vec<&str> = a.split(' ').collect();
        let g = |i: usize| f(p[i]);
        let s = ViewportSnapshot {
            zoom: g(0),
            stretch: if p[11] == "1" {
                ViewerStretchMode::Uniform
            } else {
                ViewerStretchMode::None
            },
            max_image_width: g(1),
            max_image_height: g(2),
            actual_image_width: g(3),
            actual_image_height: g(4),
            extent_width: g(5),
            extent_height: g(6),
            viewport_width: g(7),
            viewport_height: g(8),
            horizontal_offset: g(9),
            vertical_offset: g(10),
            horizontal_scroll_bar_visible: p[12] == "1",
            vertical_scroll_bar_visible: p[13] == "1",
        };
        if s.to_string() != text {
            bad.push(format!("rust={} cs={}", s, text));
        }
    }
    assert!(
        bad.is_empty(),
        "{} diffs, first: {:?}",
        bad.len(),
        &bad[..bad.len().min(3)]
    );
}

#[test]
#[ignore = "needs oracle files in a local scratchpad directory (D), not available on CI"]
fn oracle_step_length() {
    for l in rd("len.txt").lines() {
        let p: Vec<&str> = l.split(' ').collect();
        let got = step_length(f(p[0]), f(p[1]), f(p[2]));
        assert_eq!(got.to_bits(), f(p[3]).to_bits(), "{l}");
    }
}

#[test]
#[ignore = "needs oracle files in a local scratchpad directory (D), not available on CI"]
fn oracle_step() {
    let mut bad = vec![];
    let mut total = 0;
    for l in rd("pan.txt").lines() {
        let p: Vec<&str> = l.split(' ').collect();
        let (dx, dy): (i32, i32) = (p[4].parse().unwrap(), p[5].parse().unwrap());
        if dx.checked_add(dy).is_none() {
            continue;
        }
        total += 1;
        let layout = ViewportLayout {
            viewport_width: f(p[0]),
            viewport_height: f(p[1]),
            max_horizontal_offset: f(p[2]),
            max_vertical_offset: f(p[3]),
            extent_width: 0.0,
            extent_height: 0.0,
            horizontal_bar_visible: false,
            vertical_bar_visible: false,
            image_rect: RectD {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
        };
        let cur = f(p[7]);
        let (o, h, v) = step(
            p[4].parse().unwrap(),
            p[5].parse().unwrap(),
            cur,
            cur / 2.0,
            &layout,
            f(p[6]),
        );
        let same = |a: f64, b: f64| a == b || (a.is_nan() && b.is_nan());
        if o as i32 != p[8].parse::<i32>().unwrap() || !same(h, f(p[9])) || !same(v, f(p[10])) {
            if bad.len() < 2000 {
                bad.push(l.to_string());
            } else {
                bad.push(String::new());
            }
        }
    }
    let first: Vec<_> = bad.iter().take(5).collect();
    assert!(bad.is_empty(), "{} of {total} diffs; first {:?}", bad.len(), first);
}

#[test]
#[ignore = "needs oracle files in a local scratchpad directory (D), not available on CI"]
fn csharp_unchecked_int_overflow_of_dx_plus_dy() {
    // C#: (dx + dy) is an unchecked int sum: int.MaxValue + 1 wraps to int.MinValue, so the step goes backwards.
    let layout = ViewportLayout {
        viewport_width: 800.0,
        viewport_height: 600.0,
        max_horizontal_offset: 800.0,
        max_vertical_offset: 600.0,
        extent_width: 0.0,
        extent_height: 0.0,
        horizontal_bar_visible: false,
        vertical_bar_visible: false,
        image_rect: RectD {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        },
    };
    let (o, h, _) = step(i32::MAX, 1, 400.0, 0.0, &layout, 0.1);
    // C# gives Panned with horizontal = clamp(400 + 60 * -2147483648) = 0
    assert_eq!((o as i32, h), (2, 0.0));
}

// ---- tests that kill the mutants that survive the committed tests (kept out of the commit) ----
use pr_app::input::primitives::{KeyModifiers, PointD, SizeD};
use pr_app::viewport::state::{LayoutEngine, ViewportState};
use pr_app::viewport::types::ViewportInput;

#[test]
#[ignore = "needs oracle files in a local scratchpad directory (D), not available on CI"]
fn point_and_size_equality_field_by_field() {
    assert_eq!(PointD { x: 1.0, y: 2.0 }, PointD { x: 1.0, y: 2.0 });
    assert_ne!(PointD { x: 1.0, y: 2.0 }, PointD { x: 1.0, y: 3.0 });
    assert_ne!(PointD { x: 1.0, y: 2.0 }, PointD { x: 0.0, y: 2.0 });
    assert_eq!(
        SizeD {
            width: 1.0,
            height: 2.0
        },
        SizeD {
            width: 1.0,
            height: 2.0
        }
    );
    assert_ne!(
        SizeD {
            width: 1.0,
            height: 2.0
        },
        SizeD {
            width: 1.0,
            height: 3.0
        }
    );
    assert_ne!(
        SizeD {
            width: 1.0,
            height: 2.0
        },
        SizeD {
            width: 0.0,
            height: 2.0
        }
    );
    assert_eq!(
        SizeD {
            width: f64::NAN,
            height: -0.0
        },
        SizeD {
            width: f64::NAN,
            height: 0.0
        }
    );
    assert_eq!(PointD { x: f64::NAN, y: -0.0 }, PointD { x: f64::NAN, y: 0.0 });
}

#[test]
#[ignore = "needs oracle files in a local scratchpad directory (D), not available on CI"]
fn key_modifiers_has_flag_and_or() {
    let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
    assert_eq!(ctrl_shift.0, 6);
    assert!(ctrl_shift.contains(KeyModifiers::CONTROL));
    assert!(ctrl_shift.contains(KeyModifiers::SHIFT));
    assert!(ctrl_shift.contains(ctrl_shift));
    assert!(ctrl_shift.contains(KeyModifiers::NONE));
    assert!(!ctrl_shift.contains(KeyModifiers::ALT));
    assert!(!KeyModifiers::CONTROL.contains(ctrl_shift));
    assert!(!KeyModifiers::NONE.contains(KeyModifiers::ALT));
    assert_eq!((KeyModifiers::ALT | KeyModifiers::ALT).0, 1);
    assert_eq!((KeyModifiers::ALT | KeyModifiers::WINDOWS).0, 9);
}

struct Id;
impl LayoutEngine for Id {
    fn compute(&self, _: &ViewportInput) -> ViewportLayout {
        ViewportLayout::default()
    }
    fn clamp_offset(&self, _: &ViewportLayout, h: f64, v: f64) -> (f64, f64) {
        (h, v)
    }
}

#[test]
#[ignore = "needs oracle files in a local scratchpad directory (D), not available on CI"]
fn state_input_returns_the_input_set() {
    let mut s = ViewportState::new(Id);
    let input = ViewportInput {
        client_width: 800.0,
        client_height: 600.0,
        ..Default::default()
    };
    s.set_input(input);
    assert_eq!(s.input(), input);
    assert_ne!(s.input(), ViewportInput::default());
}

#[test]
#[ignore = "needs oracle files in a local scratchpad directory (D), not available on CI"]
fn snapshot_equality_every_field() {
    let base = ViewportSnapshot {
        zoom: 1.0,
        stretch: ViewerStretchMode::None,
        max_image_width: 2.0,
        max_image_height: 3.0,
        actual_image_width: 4.0,
        actual_image_height: 5.0,
        extent_width: 6.0,
        extent_height: 7.0,
        viewport_width: 8.0,
        viewport_height: 9.0,
        horizontal_offset: 10.0,
        vertical_offset: 11.0,
        horizontal_scroll_bar_visible: false,
        vertical_scroll_bar_visible: false,
    };
    assert_eq!(base, base);
    let mut nan = base;
    nan.zoom = f64::NAN;
    assert_eq!(nan, nan);
    let mut z = base;
    z.vertical_offset = -0.0;
    let mut z2 = base;
    z2.vertical_offset = 0.0;
    assert_eq!(z, z2);
    let muts: Vec<SnapshotMutation> = vec![
        Box::new(|s| s.zoom += 1.0),
        Box::new(|s| s.stretch = ViewerStretchMode::Uniform),
        Box::new(|s| s.max_image_width += 1.0),
        Box::new(|s| s.max_image_height += 1.0),
        Box::new(|s| s.actual_image_width += 1.0),
        Box::new(|s| s.actual_image_height += 1.0),
        Box::new(|s| s.extent_width += 1.0),
        Box::new(|s| s.extent_height += 1.0),
        Box::new(|s| s.viewport_width += 1.0),
        Box::new(|s| s.viewport_height += 1.0),
        Box::new(|s| s.horizontal_offset += 1.0),
        Box::new(|s| s.vertical_offset += 1.0),
        Box::new(|s| s.horizontal_scroll_bar_visible = true),
        Box::new(|s| s.vertical_scroll_bar_visible = true),
    ];
    for (i, m) in muts.iter().enumerate() {
        let mut s = base;
        m(&mut s);
        assert_ne!(s, base, "field {i}");
    }
}

#[test]
#[ignore = "needs oracle files in a local scratchpad directory (D), not available on CI"]
fn scroll_to_negative_zero_is_normalised_to_positive_zero() {
    // C# Math.Max(0.0, -0.0) is +0.0.
    let mut s = ViewportState::new(Id);
    s.scroll_to(-0.0, -0.0);
    s.update_layout();
    assert!(s.horizontal_offset().is_sign_positive());
    assert!(s.vertical_offset().is_sign_positive());
}

type SnapshotMutation = Box<dyn Fn(&mut ViewportSnapshot)>;
