//! Killer tests for surviving input mutants (lane B). Comments in English.
use pr_app::input::key_mapping::{precheck_rejects, try_parse};
use pr_app::input::touchpad_gestures::{WheelInput, pan_offset_delta, pan_offset_delta_scaled};

// key_mapping.rs:629 `||` -> `&&` (three operators): each clause must reject on its own.
#[test]
fn precheck_rejects_each_clause_alone() {
    assert!(precheck_rejects("A,B", 'A'), "comma alone");
    assert!(precheck_rejects("1x", '1'), "digit alone");
    assert!(precheck_rejects("\u{0664}x", '\u{0664}'), "unicode digit alone");
    assert!(precheck_rejects("-x", '-'), "minus alone");
    assert!(precheck_rejects("+x", '+'), "plus alone");
    assert!(!precheck_rejects("Enter", 'E'));
    assert!(!precheck_rejects("Enter+", 'E'));
}

#[test]
fn try_parse_still_rejects_prechecked_names() {
    for n in [",", "1", "-A", "+A", "A,B"] {
        assert_eq!(try_parse(n), None, "{n}");
    }
    assert!(try_parse("Enter").is_some());
}

// touchpad_gestures.rs:262 `*` -> `/` (and 260): a scale other than 1 distinguishes multiply from divide.
#[test]
fn pan_offset_delta_scales_by_multiplication() {
    let h = pan_offset_delta_scaled(WheelInput::new(10, true, false, 0), 2.0);
    assert_eq!(h, (20.0, 0.0));
    let v = pan_offset_delta_scaled(WheelInput::new(10, false, false, 0), 2.0);
    assert_eq!(v, (0.0, -20.0));
    let v = pan_offset_delta_scaled(WheelInput::new(-6, false, false, 0), 0.5);
    assert_eq!(v, (0.0, 3.0));
    // Production scale stays 1.0.
    assert_eq!(pan_offset_delta(WheelInput::new(7, true, false, 0)), (7.0, 0.0));
    assert_eq!(pan_offset_delta(WheelInput::new(7, false, false, 0)), (0.0, -7.0));
}
