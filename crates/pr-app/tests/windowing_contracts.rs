//! Port of the placement rules of `PhotoReview.App.Shared/Windowing/WindowingContracts.cs` (baseline d92d6aae) and of the
//! C# tests that pin them, plus golden `window-placement-rules` (oracle-cs, 66 cases). Contract: `contracts/W8-03a.md`.
//! Red tests: the rule bodies are `todo!()` until the port lands.

use pr_app::windowing::contracts::{
    WindowShowState, normalize_show_command, plan_state_before_show, resolve_show_command,
};
use pr_app::windowing::monitor_layout::{FixedMonitorLayout, MonitorLayout, ScreenRect};
use pr_test_support::golden::{self, i64_of, str_of};
use serde_json::Value;

fn i32_of(case: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(case, key)).unwrap_or_else(|_| panic!("'{key}' does not fit an i32: {case}"))
}

fn state_named(name: &str) -> WindowShowState {
    match name {
        "Normal" => WindowShowState::Normal,
        "Minimized" => WindowShowState::Minimized,
        "Maximized" => WindowShowState::Maximized,
        other => panic!("unknown WindowShowState name '{other}'"),
    }
}

// C#: golden window-placement-rules (oracle-cs window-placement-rules, 17 normalize + 17 plan + 32 resolve cases)
#[test]
fn golden_window_placement_rules() {
    let golden = golden::load("window-placement-rules");
    assert_eq!(golden.cases.len(), 66, "the golden has 66 cases");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let show_command = i32_of(case, "show_command");
        match str_of(case, "kind") {
            "normalize" => assert_eq!(
                i64::from(normalize_show_command(show_command)),
                i64_of(case, "value"),
                "normalize {name}"
            ),
            "plan" => assert_eq!(
                plan_state_before_show(show_command).name(),
                str_of(case, "value"),
                "plan {name}"
            ),
            "resolve" => {
                let restore = case["restore_state"].as_str().map(state_named);
                assert_eq!(
                    i64::from(resolve_show_command(show_command, restore)),
                    i64_of(case, "value"),
                    "resolve {name}"
                );
            }
            other => panic!("unknown kind '{other}' in {name}"),
        }
    }
}

// C#: WindowPlacementServiceTests.NormalizeShowCommand_AcceptsOnlyNormalAndMaximized
// C#: JsonWindowPlacementStoreRoundTripTests.Rules_Normalize (the same six rows; PlanStateBeforeShow is checked too)
#[test]
fn normalize_show_command_accepts_only_normal_and_maximized() {
    for (input, expected) in [(0, 1), (1, 1), (2, 1), (3, 3), (7, 1), (-5, 1)] {
        assert_eq!(normalize_show_command(input), expected, "normalize {input}");
        let state = if expected == 3 {
            WindowShowState::Maximized
        } else {
            WindowShowState::Normal
        };
        assert_eq!(plan_state_before_show(input), state, "plan {input}");
    }
}

// C#: WindowPlacementServiceTests.ResolveShowCommand_InFullscreen_UsesStateBeforeFullscreen
// C#: JsonWindowPlacementStoreRoundTripTests.Rules_ResolveShowCommand (rows that carry a restore state)
#[test]
fn resolve_show_command_in_fullscreen_uses_state_before_fullscreen() {
    assert_eq!(resolve_show_command(3, Some(WindowShowState::Normal)), 1);
    assert_eq!(resolve_show_command(3, Some(WindowShowState::Minimized)), 1);
    assert_eq!(resolve_show_command(3, Some(WindowShowState::Maximized)), 3);
    assert_eq!(resolve_show_command(1, Some(WindowShowState::Maximized)), 3);
}

// C#: WindowPlacementServiceTests.ResolveShowCommand_NotFullscreen_NormalizesCurrent
// C#: JsonWindowPlacementStoreRoundTripTests.Rules_ResolveShowCommand (rows without a restore state)
#[test]
fn resolve_show_command_not_fullscreen_normalizes_current() {
    for (current, expected) in [(1, 1), (3, 3), (2, 1)] {
        assert_eq!(
            resolve_show_command(current, None),
            expected,
            "resolve {current} without a restore state"
        );
    }
}

// C#: StartupFirstShowTests.PlanStateBeforeShow_ReopensOnlyMaximizedOrNormal (rows 3, 1, 2 SW_SHOWMINIMIZED, 0 SW_HIDE)
#[test]
fn plan_state_before_show_reopens_only_maximized_or_normal() {
    assert_eq!(plan_state_before_show(3), WindowShowState::Maximized);
    assert_eq!(plan_state_before_show(1), WindowShowState::Normal);
    assert_eq!(plan_state_before_show(2), WindowShowState::Normal);
    assert_eq!(plan_state_before_show(0), WindowShowState::Normal);
}

// new (no C# twin): the discriminants equal the C# enum values and the names are the C# ToString() texts
#[test]
fn window_show_state_values_names_and_from_i32() {
    assert_eq!(WindowShowState::Normal as i32, 0);
    assert_eq!(WindowShowState::Minimized as i32, 1);
    assert_eq!(WindowShowState::Maximized as i32, 2);
    assert_eq!(WindowShowState::Normal.name(), "Normal");
    assert_eq!(WindowShowState::Minimized.name(), "Minimized");
    assert_eq!(WindowShowState::Maximized.name(), "Maximized");
    assert_eq!(WindowShowState::from_i32(0), Some(WindowShowState::Normal));
    assert_eq!(WindowShowState::from_i32(1), Some(WindowShowState::Minimized));
    assert_eq!(WindowShowState::from_i32(2), Some(WindowShowState::Maximized));
    assert_eq!(WindowShowState::from_i32(3), None);
    assert_eq!(WindowShowState::from_i32(-1), None);
}

// new (no C# twin): Width and Height wrap like C# unchecked int arithmetic (contract note 5)
#[test]
fn screen_rect_width_and_height_wrap_like_unchecked_int() {
    assert_eq!(ScreenRect::new(i32::MIN, 0, i32::MAX, 0).width(), -1);
    assert_eq!(ScreenRect::new(10, 20, 110, 70).width(), 100);
    assert_eq!(ScreenRect::new(10, 20, 110, 70).height(), 50);
    assert_eq!(ScreenRect::new(110, 70, 10, 20).width(), -100);
    assert_eq!(ScreenRect::new(110, 70, 10, 20).height(), -50);
}

// new (no C# twin): the test double of C# FixedLayout (contract note 6)
#[test]
fn fixed_monitor_layout_returns_first_work_area_and_the_list() {
    let first = ScreenRect::new(0, 0, 1920, 1040);
    let second = ScreenRect::new(1920, 0, 3840, 1040);
    let layout = FixedMonitorLayout::new(vec![first, second]);
    assert_eq!(layout.monitor_of(0x1234), 1);
    assert_eq!(layout.try_get_work(1), Some(first));
    assert_eq!(layout.work_areas(), vec![first, second]);

    let empty = FixedMonitorLayout::new(Vec::new());
    assert_eq!(empty.try_get_work(1), None);
    assert!(empty.work_areas().is_empty());
}
