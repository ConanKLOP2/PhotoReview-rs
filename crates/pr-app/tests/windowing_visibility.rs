//! Port of `PhotoReview.App.Shared/Windowing/WindowPlacementVisibility.cs` (baseline d92d6aae) and of the C# facts in
//! `tests/PhotoReview.App.Tests/Services/WindowPlacementVisibilityTests.cs` and `JsonWindowPlacementStoreRoundTripTests`
//! (`IsVisible_RequiresAnEightyPixelIntersection`), plus golden `window-placement-visibility` (oracle-cs, 37 cases).
//! Contract: `contracts/W8-03a.md`.
//!
//! The C# facts read the real monitors by reflection. Here every scenario uses a fixed work area `(0,0,1920,1040)`
//! and each negative scenario gets a two-monitor twin (an extra work area that does not reach the placement).

use std::sync::Arc;

use pr_app::windowing::contracts::{SHOW_MAXIMIZED, WindowPlacementData};
use pr_app::windowing::monitor_layout::{FixedMonitorLayout, ScreenRect};
use pr_app::windowing::placement_store::JsonWindowPlacementStore;
use pr_app::windowing::visibility::is_visible as direct_is_visible;
use pr_test_support::FakeFileSystem;
use pr_test_support::golden::{self, bool_of, str_of};
use serde_json::Value;

const PRIMARY: ScreenRect = ScreenRect::new(0, 0, 1920, 1040);

/// A second monitor to the left of the primary one; it never reaches the scenarios below (the twin of a negative case).
const LEFT_OF_PRIMARY: ScreenRect = ScreenRect::new(-1920, 0, 0, 1040);

fn store_over(works: &[ScreenRect]) -> JsonWindowPlacementStore {
    JsonWindowPlacementStore::new(
        Arc::new(FakeFileSystem::new()),
        Arc::new(FixedMonitorLayout::new(works.to_vec())),
    )
}

fn placement_with_normal(left: i32, top: i32, right: i32, bottom: i32) -> WindowPlacementData {
    WindowPlacementData {
        length: 44,
        flags: 2,
        show_command: SHOW_MAXIMIZED,
        min_x: 0,
        min_y: 0,
        max_x: 0,
        max_y: 0,
        normal_left: left,
        normal_top: top,
        normal_right: right,
        normal_bottom: bottom,
    }
}

/// Visibility through the store (the path the app uses): the Normal rect against the given work areas.
fn store_says_visible(works: &[ScreenRect], left: i32, top: i32, right: i32, bottom: i32) -> bool {
    store_over(works).is_visible(&placement_with_normal(left, top, right, bottom))
}

/// Four integers `[left, top, right, bottom]` from a golden array.
fn ints(value: &Value) -> [i32; 4] {
    let items = value.as_array().unwrap_or_else(|| panic!("not an array: {value}"));
    let at = |index: usize| {
        let number = items[index]
            .as_i64()
            .unwrap_or_else(|| panic!("not an integer: {value}"));
        i32::try_from(number).unwrap_or_else(|_| panic!("does not fit an i32: {value}"))
    };
    [at(0), at(1), at(2), at(3)]
}

// C#: golden window-placement-visibility (oracle-cs window-placement-visibility, 37 cases)
#[test]
fn golden_window_placement_visibility() {
    let golden = golden::load("window-placement-visibility");
    assert_eq!(golden.cases.len(), 37, "the golden has 37 cases");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let [left, top, right, bottom] = ints(&case["rect"]);
        let works: Vec<ScreenRect> = case["work"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: no work array"))
            .iter()
            .map(|area| {
                let [l, t, r, b] = ints(area);
                ScreenRect::new(l, t, r, b)
            })
            .collect();

        let store = store_over(&works);
        assert_eq!(
            store.is_visible(&placement_with_normal(left, top, right, bottom)),
            bool_of(case, "store"),
            "store: {name}"
        );
        assert_eq!(
            direct_is_visible(left, top, right, bottom, &works),
            bool_of(case, "direct"),
            "direct: {name}"
        );
    }
}

// C#: WindowPlacementVisibilityTests.IsVisible_PlacementFullyInsideTheWorkArea_IsValid
#[test]
fn is_visible_placement_fully_inside_the_work_area_is_valid() {
    assert!(direct_is_visible(50, 50, 450, 450, &[PRIMARY]));
    assert!(store_says_visible(&[PRIMARY], 50, 50, 450, 450));
}

// C#: WindowPlacementVisibilityTests.IsVisible_PlacementPartlyUnderTheTaskbarButWithEnoughWorkAreaPixels_StaysValid
// (hangs 400 px past the bottom-right corner of the work area; 100x100 px remain inside it)
#[test]
fn is_visible_placement_partly_under_taskbar_with_enough_work_area_pixels_stays_valid() {
    assert!(direct_is_visible(1820, 940, 2320, 1440, &[PRIMARY]));
    assert!(store_says_visible(&[PRIMARY], 1820, 940, 2320, 1440));
    assert!(store_says_visible(&[LEFT_OF_PRIMARY, PRIMARY], 1820, 940, 2320, 1440));
}

// C#: WindowPlacementVisibilityTests.IsVisible_PlacementWithOnly79PixelsInsideTheWorkArea_IsRejected
// (79 px wide, 79 px high, and exactly 80 x 80 which is accepted)
#[test]
fn is_visible_placement_with_only_79_pixels_inside_the_work_area_is_rejected() {
    let too_narrow = (1841, 840, 2320, 1440);
    let too_short = (1720, 961, 2320, 1440);
    let exact = (1840, 960, 2320, 1440);

    for works in [vec![PRIMARY], vec![LEFT_OF_PRIMARY, PRIMARY]] {
        let (l, t, r, b) = too_narrow;
        assert!(!direct_is_visible(l, t, r, b, &works), "79 px wide, {works:?}");
        assert!(!store_says_visible(&works, l, t, r, b), "79 px wide, {works:?}");
        let (l, t, r, b) = too_short;
        assert!(!direct_is_visible(l, t, r, b, &works), "79 px high, {works:?}");
        assert!(!store_says_visible(&works, l, t, r, b), "79 px high, {works:?}");
        let (l, t, r, b) = exact;
        assert!(direct_is_visible(l, t, r, b, &works), "exactly 80 x 80, {works:?}");
        assert!(store_says_visible(&works, l, t, r, b), "exactly 80 x 80, {works:?}");
    }
}

// C#: WindowPlacementVisibilityTests.IsVisible_PlacementEntirelyBelowTheWorkArea_IsRejectedEvenThoughItMayBeOnTheMonitor
// (starts at the bottom edge of the work area: in the taskbar strip, never inside the work area)
#[test]
fn is_visible_placement_entirely_below_the_work_area_is_rejected() {
    assert!(!direct_is_visible(50, 1040, 450, 1340, &[PRIMARY]));
    assert!(!store_says_visible(&[PRIMARY], 50, 1040, 450, 1340));
    assert!(!direct_is_visible(50, 1040, 450, 1340, &[LEFT_OF_PRIMARY, PRIMARY]));
    assert!(!store_says_visible(&[LEFT_OF_PRIMARY, PRIMARY], 50, 1040, 450, 1340));
}

// C#: WindowPlacementVisibilityTests.IsVisible_EmptyOrInvertedBounds_AreRejected (zero width, zero height, inverted)
#[test]
fn is_visible_empty_or_inverted_bounds_are_rejected() {
    let rows = [(100, 100, 100, 400), (100, 100, 400, 100), (400, 400, 100, 100)];
    for works in [vec![PRIMARY], vec![LEFT_OF_PRIMARY, PRIMARY]] {
        for (l, t, r, b) in rows {
            assert!(!direct_is_visible(l, t, r, b, &works), "{l},{t},{r},{b} over {works:?}");
            assert!(
                !store_says_visible(&works, l, t, r, b),
                "{l},{t},{r},{b} over {works:?}"
            );
        }
    }
}

// C#: JsonWindowPlacementStoreRoundTripTests.IsVisible_RequiresAnEightyPixelIntersection (5 rows)
#[test]
fn is_visible_requires_an_eighty_pixel_intersection() {
    let rows = [
        (1840, 960, 3000, 2000, true),  // exactly 80x80 inside
        (1841, 960, 3000, 2000, false), // 79 wide
        (1840, 961, 3000, 2000, false), // 79 high
        (100, 100, 100, 500, false),    // empty
        (500, 500, 400, 600, false),    // inverted
    ];
    for (l, t, r, b, expected) in rows {
        assert_eq!(
            store_says_visible(&[PRIMARY], l, t, r, b),
            expected,
            "store {l},{t},{r},{b}"
        );
        assert_eq!(
            direct_is_visible(l, t, r, b, &[PRIMARY]),
            expected,
            "direct {l},{t},{r},{b}"
        );
    }
}

// new (no C# twin): with no work area at all nothing is visible
#[test]
fn is_visible_with_no_work_areas_is_false() {
    assert!(!direct_is_visible(50, 50, 450, 450, &[]));
    assert!(!store_says_visible(&[], 50, 50, 450, 450));
}

// new (no C# twin): only the Normal rect decides; the show command does not (contract note 9)
#[test]
fn is_visible_ignores_the_show_command() {
    let store = store_over(&[PRIMARY]);
    for show_command in [1, SHOW_MAXIMIZED] {
        let visible = WindowPlacementData {
            show_command,
            ..placement_with_normal(50, 50, 450, 450)
        };
        let off_screen = WindowPlacementData {
            show_command,
            ..placement_with_normal(5000, 5000, 6000, 6000)
        };
        assert!(store.is_visible(&visible), "show command {show_command}, visible rect");
        assert!(
            !store.is_visible(&off_screen),
            "show command {show_command}, off-screen rect"
        );
    }
}
