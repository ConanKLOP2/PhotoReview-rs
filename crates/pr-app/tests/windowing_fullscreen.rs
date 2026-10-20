//! Red tests for `windowing::fullscreen` (W8-03b).
//!
//! Port of the C# facts in `tests/PhotoReview.App.Tests/Coordinators/FullscreenWindowPlacerResolveExitTests.cs` (11 tests) and
//! of the three review-added tests in `tests/PhotoReview.App.Tests/Windowing/JsonWindowPlacementStoreRoundTripTests.cs`, plus
//! new fake-based controller tests (marked `// new:`) for the handle-based `enter`/`exit` sequence that no C# unit test covers.
//! Source under test: `PhotoReview.App.Shared/Windowing/FullscreenController.cs` (baseline d92d6aae).
//! Contract: `contracts/W8-03b.md`. Until the bodies land every test fails through `todo!("W8-03b")`.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pr_app::windowing::contracts::WindowShowState;
use pr_app::windowing::fullscreen::{
    FullscreenController, MonitorSnapshot, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    WS_CHROME, WindowOps, resolve_normal_exit_rect,
};
use pr_app::windowing::monitor_layout::{FixedMonitorLayout, MonitorLayout, ScreenRect};

use WindowShowState::{Maximized, Minimized, Normal};

/// `SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED` (`0x34`): the move used by enter and by the exact-rect exit.
const MOVE: u32 = SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;
/// `SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED` (`0x37`): the frame refresh before `ShowWindow(MAXIMIZE)`.
const REFRESH: u32 = SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;

/// Primary work area, also the only work area of the baseline layout (C# `Main`).
const MAIN: ScreenRect = ScreenRect::new(0, 0, 1920, 1040);
/// Second monitor to the right of [`MAIN`] (C# `Second`).
const SECOND: ScreenRect = ScreenRect::new(1920, 0, 3840, 1040);
/// The all-zero rect of a failed `GetWindowRect` and of the framework-refresh SetWindowPos.
const ZERO: ScreenRect = ScreenRect::new(0, 0, 0, 0);

/// Baseline window: its normal position and its current rect.
const WINDOW: ScreenRect = ScreenRect::new(100, 100, 700, 500);
/// Baseline `rcMonitor` (covers the taskbar).
const MONITOR: ScreenRect = ScreenRect::new(0, 0, 1920, 1080);
/// Baseline monitor handle.
const MONITOR_HANDLE: isize = 7;
/// Window handle used by the controller tests (any non-zero value).
const HWND: isize = 42;
/// Baseline style: chrome bits plus `WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN`.
const STYLE: isize = 0x16CF_0000;
/// [`STYLE`] without the chrome bits: what enter sets.
const STYLE_FULLSCREEN: isize = 0x1600_0000;

/// One call on the [`WindowOps`] seam that mutates the window (read-only calls are not logged).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    ShowNormal,
    ShowMaximized,
    SetStyle(isize),
    SetPos(ScreenRect, u32),
}

/// Fake user32: read-only calls return the configured fields; mutating calls are appended to `log`.
#[derive(Debug, Default)]
struct FakeOps {
    log: RefCell<Vec<Op>>,
    iconic: bool,
    normal: Option<ScreenRect>,
    window: Option<ScreenRect>,
    monitor: Option<MonitorSnapshot>,
    style: isize,
}

impl FakeOps {
    /// The baseline scenario: a Normal window at [`WINDOW`] on monitor 7 whose work area is [`MAIN`].
    fn baseline() -> Self {
        Self {
            log: RefCell::new(Vec::new()),
            iconic: false,
            normal: Some(WINDOW),
            window: Some(WINDOW),
            monitor: Some(MonitorSnapshot {
                handle: MONITOR_HANDLE,
                monitor: MONITOR,
                work: MAIN,
            }),
            style: STYLE,
        }
    }

    fn ops(&self) -> Vec<Op> {
        self.log.borrow().clone()
    }
}

impl WindowOps for FakeOps {
    fn is_iconic(&self, _hwnd: isize) -> bool {
        self.iconic
    }

    fn show_normal(&self, _hwnd: isize) {
        self.log.borrow_mut().push(Op::ShowNormal);
    }

    fn show_maximized(&self, _hwnd: isize) {
        self.log.borrow_mut().push(Op::ShowMaximized);
    }

    fn normal_position(&self, _hwnd: isize) -> Option<ScreenRect> {
        self.normal
    }

    fn window_rect(&self, _hwnd: isize) -> Option<ScreenRect> {
        self.window
    }

    fn nearest_monitor(&self, _hwnd: isize) -> Option<MonitorSnapshot> {
        self.monitor
    }

    fn window_style(&self, _hwnd: isize) -> isize {
        self.style
    }

    fn set_window_style(&self, _hwnd: isize, style: isize) {
        self.log.borrow_mut().push(Op::SetStyle(style));
    }

    fn set_window_pos(&self, _hwnd: isize, rect: ScreenRect, flags: u32) {
        self.log.borrow_mut().push(Op::SetPos(rect, flags));
    }
}

/// Counts the layout queries the controller makes (to assert the short circuits).
#[derive(Debug, Clone, Default)]
struct Probe {
    work_calls: Arc<AtomicUsize>,
    areas_calls: Arc<AtomicUsize>,
}

impl Probe {
    fn work_calls(&self) -> usize {
        self.work_calls.load(Ordering::SeqCst)
    }

    fn areas_calls(&self) -> usize {
        self.areas_calls.load(Ordering::SeqCst)
    }
}

/// Fake `IMonitorLayout`: the window's monitor, that monitor's work area (`None` = gone), and all work areas.
struct FakeLayout {
    monitor_of: isize,
    work: Option<ScreenRect>,
    areas: Vec<ScreenRect>,
    probe: Probe,
}

impl MonitorLayout for FakeLayout {
    fn monitor_of(&self, _hwnd: isize) -> isize {
        self.monitor_of
    }

    fn try_get_work(&self, _monitor: isize) -> Option<ScreenRect> {
        self.probe.work_calls.fetch_add(1, Ordering::SeqCst);
        self.work
    }

    fn work_areas(&self) -> Vec<ScreenRect> {
        self.probe.areas_calls.fetch_add(1, Ordering::SeqCst);
        self.areas.clone()
    }
}

fn layout(monitor_of: isize, work: Option<ScreenRect>, areas: Vec<ScreenRect>) -> (Box<dyn MonitorLayout>, Probe) {
    let probe = Probe::default();
    let boxed: Box<dyn MonitorLayout> = Box::new(FakeLayout {
        monitor_of,
        work,
        areas,
        probe: probe.clone(),
    });
    (boxed, probe)
}

/// The baseline layout: the window stays on monitor 7 and its work area is unchanged.
fn same_layout() -> (Box<dyn MonitorLayout>, Probe) {
    layout(MONITOR_HANDLE, Some(MAIN), vec![MAIN])
}

/// Enter then exit on the handle [`HWND`] with the given layout; returns the controller for further checks.
fn enter_then_exit(current: WindowShowState, ops: &FakeOps, layout: Box<dyn MonitorLayout>) -> FullscreenController {
    let mut controller = FullscreenController::new(layout);
    controller.enter(ops, HWND, current);
    controller.exit(ops, HWND);
    controller
}

/// The call sequence of a plain enter of the baseline window (style, then the single move onto the monitor).
fn baseline_enter_ops() -> Vec<Op> {
    vec![Op::SetStyle(STYLE_FULLSCREEN), Op::SetPos(MONITOR, MOVE)]
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported: FullscreenWindowPlacerResolveExitTests (pure policy `ResolveNormalExitRect`)
// ---------------------------------------------------------------------------------------------------------------------

// C#: FullscreenWindowPlacerResolveExitTests.Resolve_RectFullyInsideAWorkArea_IsReturnedExactly
#[test]
fn resolve_rect_fully_inside_a_work_area_is_returned_exactly() {
    let saved = ScreenRect::new(300, 200, 1100, 800);
    assert_eq!(resolve_normal_exit_rect(saved, &[MAIN, SECOND]), saved);
}

// C#: FullscreenWindowPlacerResolveExitTests.Resolve_RectOnTheSecondMonitorThatStillExists_IsReturnedExactly
#[test]
fn resolve_rect_on_the_second_monitor_that_still_exists_is_returned_exactly() {
    let saved = ScreenRect::new(2200, 100, 3000, 700);
    assert_eq!(resolve_normal_exit_rect(saved, &[MAIN, SECOND]), saved);
}

// C#: FullscreenWindowPlacerResolveExitTests.Resolve_RectOnAMonitorThatDisappeared_MovesOntoTheRemainingMonitorKeepingItsSize
#[test]
fn resolve_rect_on_a_monitor_that_disappeared_moves_onto_the_remaining_monitor_keeping_its_size() {
    let saved = ScreenRect::new(2200, 100, 3000, 700);
    assert_eq!(
        resolve_normal_exit_rect(saved, &[MAIN]),
        ScreenRect::new(1120, 100, 1920, 700)
    );
}

// C#: FullscreenWindowPlacerResolveExitTests.Resolve_RectLeftOfEverything_MovesToTheNearestMonitorsLeftEdge
#[test]
fn resolve_rect_left_of_everything_moves_to_the_nearest_monitors_left_edge() {
    let saved = ScreenRect::new(-4000, 50, -3200, 650);
    assert_eq!(
        resolve_normal_exit_rect(saved, &[MAIN]),
        ScreenRect::new(0, 50, 800, 650)
    );
}

// C#: FullscreenWindowPlacerResolveExitTests.Resolve_RectBelowTheWorkArea_MovesUpOntoIt
#[test]
fn resolve_rect_below_the_work_area_moves_up_onto_it() {
    let saved = ScreenRect::new(100, 1500, 900, 2100);
    assert_eq!(
        resolve_normal_exit_rect(saved, &[MAIN]),
        ScreenRect::new(100, 440, 900, 1040)
    );
}

// C#: FullscreenWindowPlacerResolveExitTests.Resolve_MovesToTheNearestOfSeveralRemainingMonitors
#[test]
fn resolve_moves_to_the_nearest_of_several_remaining_monitors() {
    let third = ScreenRect::new(-1920, 0, 0, 1040);
    let saved = ScreenRect::new(4000, 100, 4800, 700); // right of Second
    assert_eq!(
        resolve_normal_exit_rect(saved, &[MAIN, third, SECOND]),
        ScreenRect::new(3040, 100, 3840, 700)
    );
}

// C#: FullscreenWindowPlacerResolveExitTests.Resolve_RectBiggerThanTheRemainingWorkArea_IsShrunkToFitIt
#[test]
fn resolve_rect_bigger_than_the_remaining_work_area_is_shrunk_to_fit_it() {
    let saved = ScreenRect::new(3000, -200, 5000, 1500); // 2000x1700 on a vanished monitor
    assert_eq!(resolve_normal_exit_rect(saved, &[MAIN]), MAIN);
}

// C#: FullscreenWindowPlacerResolveExitTests.Resolve_PartiallyVisibleWithTheCaptionReachable_IsKeptExactly
#[test]
fn resolve_partially_visible_with_the_caption_reachable_is_kept_exactly() {
    // 300 px of the left part hangs over the left edge, the caption band is still 500 px wide inside the work area.
    let saved = ScreenRect::new(-300, 100, 500, 700);
    assert_eq!(resolve_normal_exit_rect(saved, &[MAIN]), saved);
}

// C#: FullscreenWindowPlacerResolveExitTests.Resolve_OnlyASliverShowsHorizontally_IsMovedBack
#[test]
fn resolve_only_a_sliver_shows_horizontally_is_moved_back() {
    // 50 px (< 120) of the window is inside the work area: nothing practical to grab, so it is moved in.
    let saved = ScreenRect::new(-750, 100, 50, 700);
    assert_eq!(
        resolve_normal_exit_rect(saved, &[MAIN]),
        ScreenRect::new(0, 100, 800, 700)
    );
}

// C#: FullscreenWindowPlacerResolveExitTests.Resolve_CaptionAboveTheWorkAreaWithOnlyTheBodyShowing_IsMovedDown
#[test]
fn resolve_caption_above_the_work_area_with_only_the_body_showing_is_moved_down() {
    // Top edge 400 px above the work area: the body shows but the caption cannot be reached to drag it.
    let saved = ScreenRect::new(200, -400, 1000, 300);
    assert_eq!(
        resolve_normal_exit_rect(saved, &[MAIN]),
        ScreenRect::new(200, 0, 1000, 700)
    );
}

// C#: FullscreenWindowPlacerResolveExitTests.Resolve_NoMonitorInformation_LeavesTheRectAlone
#[test]
fn resolve_no_monitor_information_leaves_the_rect_alone() {
    let saved = ScreenRect::new(2200, 100, 3000, 700);
    assert_eq!(resolve_normal_exit_rect(saved, &[]), saved);
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported: JsonWindowPlacementStoreRoundTripTests (the three tests the review added for FullscreenController)
// ---------------------------------------------------------------------------------------------------------------------

// C#: JsonWindowPlacementStoreRoundTripTests.Controller_WithoutHwnd_DelegatesToTheFramework
#[test]
fn controller_without_hwnd_delegates_to_the_framework() {
    let mut controller = FullscreenController::new(Box::new(FixedMonitorLayout::new(Vec::new())));
    let ops = FakeOps::baseline();

    controller.enter(&ops, 0, Maximized);
    assert_eq!(controller.state_before(), Maximized);
    assert!(controller.enter_needs_maximized_state());
    assert_eq!(controller.normal_bounds(), None);

    controller.exit(&ops, 0);
    assert!(controller.exit_needs_state_restore());
    assert!(ops.ops().is_empty(), "no OS call without a handle");
}

// C#: JsonWindowPlacementStoreRoundTripTests.Controller_Minimized_CountsAsNormal
#[test]
fn controller_minimized_counts_as_normal() {
    let mut controller = FullscreenController::new(Box::new(FixedMonitorLayout::new(Vec::new())));
    let ops = FakeOps::baseline();

    controller.enter(&ops, 0, Minimized);

    assert_eq!(controller.state_before(), Normal);
    assert!(ops.ops().is_empty(), "no OS call without a handle");
}

// C#: JsonWindowPlacementStoreRoundTripTests.ResolveNormalExitRect_CaptionBandBoundaries
#[test]
fn resolve_normal_exit_rect_caption_band_boundaries() {
    let saved = ScreenRect::new(0, 0, 800, 600);
    let rows = [
        ((680, 0, 2000, 1000), true),  // 120 px wide (800 - 680)
        ((681, 0, 2000, 1000), false), // 119 px wide
        ((0, -100, 1000, 16), true),   // 16 px of the band above the work area's bottom edge
        ((0, -100, 1000, 15), false),  // 15 px
        ((0, 16, 1000, 1000), true),   // band is 32 px tall: 16 px below the work area's top edge
        ((0, 17, 1000, 1000), false),  // 15 px
    ];
    for ((left, top, right, bottom), kept_as_is) in rows {
        let work = ScreenRect::new(left, top, right, bottom);
        let resolved = resolve_normal_exit_rect(saved, &[work]);
        assert_eq!(resolved == saved, kept_as_is, "saved rect against work area {work:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// New: enter (fake-based; the handle-based sequence has no C# unit test)
// ---------------------------------------------------------------------------------------------------------------------

// new: the style loses its chrome bits BEFORE the single move onto the whole monitor; no state change through show_*.
#[test]
fn enter_on_a_monitor_changes_the_style_then_moves_once_onto_the_whole_monitor() {
    let (layout, _probe) = same_layout();
    let mut controller = FullscreenController::new(layout);
    let ops = FakeOps::baseline();

    controller.enter(&ops, HWND, Normal);

    assert_eq!(ops.ops(), baseline_enter_ops());
    assert!(!controller.enter_needs_maximized_state());
    assert_eq!(controller.normal_bounds(), Some((100, 100, 700, 500)));
    assert_eq!(controller.state_before(), Normal);
    assert!(!controller.exit_needs_state_restore());
}

// new: a Maximized window stays Maximized; the same call sequence as the Normal case.
#[test]
fn enter_keeps_the_maximized_state_of_the_window() {
    let (layout, _probe) = same_layout();
    let mut controller = FullscreenController::new(layout);
    let ops = FakeOps::baseline();

    controller.enter(&ops, HWND, Maximized);

    assert_eq!(ops.ops(), baseline_enter_ops());
    assert_eq!(controller.state_before(), Maximized);
    assert!(!controller.enter_needs_maximized_state());
}

// new: a minimized window is restored first (ShowNormal before any measuring) and remembered as Normal.
#[test]
fn enter_restores_a_minimized_window_first_and_remembers_normal() {
    let (layout, _probe) = same_layout();
    let mut controller = FullscreenController::new(layout);
    let mut ops = FakeOps::baseline();
    ops.iconic = true;

    controller.enter(&ops, HWND, Minimized);

    let mut expected = vec![Op::ShowNormal];
    expected.extend(baseline_enter_ops());
    assert_eq!(ops.ops(), expected);
    assert_eq!(controller.state_before(), Normal);
}

// new: without a handle the controller records the state, touches nothing and asks the caller to maximize.
#[test]
fn enter_without_a_handle_needs_a_maximized_state_and_touches_nothing() {
    let cases = [(Normal, Normal), (Maximized, Maximized), (Minimized, Normal)];
    for (current, expected_before) in cases {
        let (layout, _probe) = same_layout();
        let mut controller = FullscreenController::new(layout);
        let ops = FakeOps::baseline();

        controller.enter(&ops, 0, current);

        assert_eq!(controller.state_before(), expected_before, "current {current:?}");
        assert!(ops.ops().is_empty(), "current {current:?}: no OS call without a handle");
        assert!(controller.enter_needs_maximized_state(), "current {current:?}");
        assert_eq!(controller.normal_bounds(), None, "current {current:?}");
    }
}

// new: no monitor info: the style is left alone, no move, the caller must maximize; the saved bounds are still held.
#[test]
fn enter_without_monitor_info_leaves_the_style_alone_and_asks_for_maximize() {
    let (layout, _probe) = same_layout();
    let mut controller = FullscreenController::new(layout);
    let mut ops = FakeOps::baseline();
    ops.monitor = None;

    controller.enter(&ops, HWND, Normal);

    assert!(ops.ops().is_empty(), "no SetStyle and no SetPos without monitor info");
    assert!(controller.enter_needs_maximized_state());
    assert_eq!(controller.normal_bounds(), Some((100, 100, 700, 500)));

    controller.exit(&ops, HWND);

    assert!(controller.exit_needs_state_restore());
    assert!(ops.ops().is_empty(), "the legacy path never touched the window");
}

// new: an unreadable placement moves the window as usual but leaves no restore bounds; exit then asks for a state restore.
#[test]
fn enter_when_the_placement_is_unreadable_has_no_normal_bounds() {
    let (layout, _probe) = same_layout();
    let mut controller = FullscreenController::new(layout);
    let mut ops = FakeOps::baseline();
    ops.normal = None;

    controller.enter(&ops, HWND, Normal);

    assert_eq!(ops.ops(), baseline_enter_ops());
    assert!(!controller.enter_needs_maximized_state());
    assert_eq!(controller.normal_bounds(), None);

    let after_enter = ops.ops();
    controller.exit(&ops, HWND);

    assert!(controller.exit_needs_state_restore());
    assert_eq!(ops.ops(), after_enter, "no style restore and no move on exit");
}

// new: a failed GetWindowRect saves an all-zero rect, so the exit moves the window to the origin (C# quirk, kept).
#[test]
fn enter_with_an_unreadable_window_rect_saves_an_all_zero_rect() {
    let (layout, _probe) = same_layout();
    let mut controller = FullscreenController::new(layout);
    let mut ops = FakeOps::baseline();
    ops.window = None;

    controller.enter(&ops, HWND, Normal);
    controller.exit(&ops, HWND);

    assert_eq!(
        ops.ops(),
        vec![
            Op::SetStyle(STYLE_FULLSCREEN),
            Op::SetPos(MONITOR, MOVE),
            Op::SetStyle(STYLE),
            Op::SetPos(ZERO, MOVE),
        ]
    );
}

// new: no "already fullscreen" guard: a second enter saves the fullscreen state as "before"; exit restores that second snapshot.
#[test]
fn enter_twice_without_exit_saves_the_fullscreen_state_again() {
    let (layout, _probe) = same_layout();
    let mut controller = FullscreenController::new(layout);
    let mut ops = FakeOps::baseline();

    controller.enter(&ops, HWND, Normal);
    // The window is now fullscreen: its rect is the monitor and its style has no chrome bits.
    ops.window = Some(MONITOR);
    ops.style = STYLE_FULLSCREEN;
    controller.enter(&ops, HWND, Normal);
    controller.exit(&ops, HWND);

    assert_eq!(
        ops.ops(),
        vec![
            Op::SetStyle(STYLE_FULLSCREEN),
            Op::SetPos(MONITOR, MOVE),
            Op::SetStyle(STYLE_FULLSCREEN),
            Op::SetPos(MONITOR, MOVE),
            Op::SetStyle(STYLE_FULLSCREEN),
            Op::SetPos(MONITOR, MOVE),
        ]
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// New: exit (fake-based)
// ---------------------------------------------------------------------------------------------------------------------

// new: same work area: style back first, then the exact saved rect in one move; no work_areas query.
#[test]
fn exit_with_the_same_work_area_restores_the_exact_rect_in_one_move() {
    let (layout, probe) = same_layout();
    let ops = FakeOps::baseline();
    let mut controller = FullscreenController::new(layout);
    controller.enter(&ops, HWND, Normal);
    assert_eq!(ops.ops(), baseline_enter_ops());

    controller.exit(&ops, HWND);

    assert_eq!(
        ops.ops(),
        vec![
            Op::SetStyle(STYLE_FULLSCREEN),
            Op::SetPos(MONITOR, MOVE),
            Op::SetStyle(STYLE),
            Op::SetPos(WINDOW, MOVE),
        ]
    );
    assert!(!controller.exit_needs_state_restore());
    assert_eq!(controller.normal_bounds(), None);
    assert_eq!(probe.work_calls(), 1);
    assert_eq!(probe.areas_calls(), 0);
}

// new: a Maximized window with an unchanged work area also gets the exact rect (its rect already has the frame overhang).
#[test]
fn exit_of_a_maximized_window_with_an_unchanged_work_area_uses_the_exact_rect_not_show_maximized() {
    let (layout, _probe) = same_layout();
    let ops = FakeOps::baseline();

    let controller = enter_then_exit(Maximized, &ops, layout);

    assert_eq!(
        ops.ops(),
        vec![
            Op::SetStyle(STYLE_FULLSCREEN),
            Op::SetPos(MONITOR, MOVE),
            Op::SetStyle(STYLE),
            Op::SetPos(WINDOW, MOVE),
        ]
    );
    assert!(!ops.ops().contains(&Op::ShowMaximized));
    assert_eq!(controller.state_before(), Maximized);
}

// new: a Maximized window after the work area changed: style back, frame refresh, then ShowWindow(MAXIMIZE); no work_areas query.
#[test]
fn exit_of_a_maximized_window_after_a_work_area_change_refreshes_the_frame_then_maximizes() {
    let changed = ScreenRect::new(0, 0, 1920, 1000);
    let (layout, probe) = layout(MONITOR_HANDLE, Some(changed), vec![MAIN]);
    let ops = FakeOps::baseline();

    let controller = enter_then_exit(Maximized, &ops, layout);

    assert_eq!(
        ops.ops(),
        vec![
            Op::SetStyle(STYLE_FULLSCREEN),
            Op::SetPos(MONITOR, MOVE),
            Op::SetStyle(STYLE),
            Op::SetPos(ZERO, REFRESH),
            Op::ShowMaximized,
        ]
    );
    assert_eq!(controller.state_before(), Maximized);
    assert_eq!(probe.areas_calls(), 0);
}

// new: a Normal window after the work area changed: the saved rect is resolved against the current work areas (one query).
#[test]
fn exit_of_a_normal_window_after_a_work_area_change_resolves_the_saved_rect() {
    let changed = ScreenRect::new(0, 0, 1920, 1000);
    let (layout, probe) = layout(MONITOR_HANDLE, Some(changed), vec![MAIN]);
    let mut ops = FakeOps::baseline();
    ops.normal = Some(ScreenRect::new(2200, 100, 3000, 700));
    ops.window = Some(ScreenRect::new(2200, 100, 3000, 700));

    let controller = enter_then_exit(Normal, &ops, layout);

    assert_eq!(
        ops.ops(),
        vec![
            Op::SetStyle(STYLE_FULLSCREEN),
            Op::SetPos(MONITOR, MOVE),
            Op::SetStyle(STYLE),
            Op::SetPos(ScreenRect::new(1120, 100, 1920, 700), MOVE),
        ]
    );
    assert_eq!(probe.areas_calls(), 1);
    assert_eq!(controller.normal_bounds(), None);
}

// new: on another monitor the work-area query is skipped (short circuit) and the change counts as "changed".
#[test]
fn exit_when_the_window_is_now_on_another_monitor_counts_as_changed_and_skips_the_work_query() {
    let mut normal_ops = FakeOps::baseline();
    normal_ops.normal = Some(ScreenRect::new(2200, 100, 3000, 700));
    normal_ops.window = Some(ScreenRect::new(2200, 100, 3000, 700));
    let (layout_normal, probe_normal) = layout(9, Some(MAIN), vec![MAIN]);

    let controller = enter_then_exit(Normal, &normal_ops, layout_normal);

    assert_eq!(
        normal_ops.ops(),
        vec![
            Op::SetStyle(STYLE_FULLSCREEN),
            Op::SetPos(MONITOR, MOVE),
            Op::SetStyle(STYLE),
            Op::SetPos(ScreenRect::new(1120, 100, 1920, 700), MOVE),
        ]
    );
    assert_eq!(
        probe_normal.work_calls(),
        0,
        "try_get_work is not called when the monitor differs"
    );
    assert_eq!(probe_normal.areas_calls(), 1);
    assert_eq!(controller.normal_bounds(), None);

    let max_ops = FakeOps::baseline();
    let (layout_max, probe_max) = layout(9, Some(MAIN), vec![MAIN]);

    enter_then_exit(Maximized, &max_ops, layout_max);

    assert_eq!(
        max_ops.ops(),
        vec![
            Op::SetStyle(STYLE_FULLSCREEN),
            Op::SetPos(MONITOR, MOVE),
            Op::SetStyle(STYLE),
            Op::SetPos(ZERO, REFRESH),
            Op::ShowMaximized,
        ]
    );
    assert_eq!(probe_max.work_calls(), 0);
    assert_eq!(probe_max.areas_calls(), 0);
}

// new: equality is on all four edges: a work area one pixel shorter counts as changed.
#[test]
fn exit_when_the_work_area_differs_by_one_pixel_counts_as_changed() {
    let shorter = ScreenRect::new(0, 0, 1920, 1039);
    let (layout, probe) = layout(MONITOR_HANDLE, Some(shorter), vec![MAIN]);
    let ops = FakeOps::baseline();

    enter_then_exit(Maximized, &ops, layout);

    assert_eq!(
        ops.ops(),
        vec![
            Op::SetStyle(STYLE_FULLSCREEN),
            Op::SetPos(MONITOR, MOVE),
            Op::SetStyle(STYLE),
            Op::SetPos(ZERO, REFRESH),
            Op::ShowMaximized,
        ]
    );
    assert_eq!(probe.work_calls(), 1);
}

// new: the original style value (including bits outside WS_CHROME, sign bit set) is written back before the move.
#[test]
fn exit_restores_the_original_style_bits_exactly() {
    let styles = [
        0x94CF_0000u32 as i32 as isize, // sign bit set
        0x16CF_0001,                    // a low bit outside WS_CHROME
    ];
    for style in styles {
        let (layout, _probe) = same_layout();
        let mut ops = FakeOps::baseline();
        ops.style = style;

        enter_then_exit(Normal, &ops, layout);

        let fullscreen_style = style & !WS_CHROME;
        assert_eq!(
            ops.ops(),
            vec![
                Op::SetStyle(fullscreen_style),
                Op::SetPos(MONITOR, MOVE),
                Op::SetStyle(style),
                Op::SetPos(WINDOW, MOVE),
            ],
            "style {style:#x}"
        );
    }
}

// new: exit without a handle asks for a state restore and keeps the saved bounds (early return); a real exit afterwards still restores.
#[test]
fn exit_without_a_handle_asks_for_a_state_restore_and_keeps_the_saved_bounds() {
    let (layout, _probe) = same_layout();
    let mut controller = FullscreenController::new(layout);
    let ops = FakeOps::baseline();
    controller.enter(&ops, HWND, Normal);
    let after_enter = ops.ops();

    controller.exit(&ops, 0);

    assert!(controller.exit_needs_state_restore());
    assert_eq!(ops.ops(), after_enter, "no OS call on the handle-less exit");
    assert_eq!(controller.normal_bounds(), Some((100, 100, 700, 500)));

    controller.exit(&ops, HWND);

    assert!(!controller.exit_needs_state_restore());
    let mut expected = after_enter;
    expected.extend([Op::SetStyle(STYLE), Op::SetPos(WINDOW, MOVE)]);
    assert_eq!(ops.ops(), expected);
    assert_eq!(controller.normal_bounds(), None);
}

// new: a second exit after a completed one takes the early return and calls nothing.
#[test]
fn a_second_exit_asks_for_a_state_restore_and_calls_nothing() {
    let (layout, _probe) = same_layout();
    let mut controller = FullscreenController::new(layout);
    let ops = FakeOps::baseline();
    controller.enter(&ops, HWND, Normal);
    controller.exit(&ops, HWND);
    let after_first_exit = ops.ops();

    controller.exit(&ops, HWND);

    assert!(controller.exit_needs_state_restore());
    assert_eq!(ops.ops(), after_first_exit);
    assert_eq!(controller.normal_bounds(), None);
}

// new: the saved bounds are cleared only by a path that ran (real restore or the no-monitor path), never by the early return.
#[test]
fn exit_clears_the_saved_bounds_only_after_a_real_restore() {
    // A real restore clears them, after an earlier handle-less exit kept them.
    let (layout, _probe) = same_layout();
    let mut controller = FullscreenController::new(layout);
    let ops = FakeOps::baseline();
    controller.enter(&ops, HWND, Normal);
    controller.exit(&ops, 0);
    assert_eq!(controller.normal_bounds(), Some((100, 100, 700, 500)));
    controller.exit(&ops, HWND);
    assert_eq!(controller.normal_bounds(), None);

    // The no-monitor path (path 2) also clears them.
    let (layout, _probe) = same_layout();
    let mut controller = FullscreenController::new(layout);
    let mut ops = FakeOps::baseline();
    ops.monitor = None;
    controller.enter(&ops, HWND, Normal);
    assert_eq!(controller.normal_bounds(), Some((100, 100, 700, 500)));
    controller.exit(&ops, HWND);
    assert!(controller.exit_needs_state_restore());
    assert_eq!(controller.normal_bounds(), None);
}

// ---------------------------------------------------------------------------------------------------------------------
// New: resolve_normal_exit_rect (pure)
// ---------------------------------------------------------------------------------------------------------------------

// new: the caption band keeps a rect at exactly 120 px wide and moves one at 119 px wide.
#[test]
fn resolve_caption_band_width_boundary() {
    let kept = ScreenRect::new(-680, 100, 120, 700);
    assert_eq!(resolve_normal_exit_rect(kept, &[MAIN]), kept);

    let moved = ScreenRect::new(-680, 100, 119, 700);
    assert_eq!(
        resolve_normal_exit_rect(moved, &[MAIN]),
        ScreenRect::new(0, 100, 799, 700)
    );
}

// new: the caption band keeps a rect at exactly 16 px high and moves one at 15 px high.
#[test]
fn resolve_caption_band_height_boundary() {
    let kept = ScreenRect::new(100, -16, 900, 700);
    assert_eq!(resolve_normal_exit_rect(kept, &[MAIN]), kept);

    let moved = ScreenRect::new(100, -17, 900, 700);
    assert_eq!(
        resolve_normal_exit_rect(moved, &[MAIN]),
        ScreenRect::new(100, 0, 900, 717)
    );
}

// new: a tie in squared distance keeps the FIRST work area in the given order.
#[test]
fn resolve_tie_keeps_the_first_work_area() {
    let saved = ScreenRect::new(1000, 2000, 1200, 2200);
    let first = ScreenRect::new(0, 0, 1000, 1000);
    let second = ScreenRect::new(1200, 0, 2200, 1000);

    assert_eq!(
        resolve_normal_exit_rect(saved, &[first, second]),
        ScreenRect::new(800, 800, 1000, 1000)
    );
    assert_eq!(
        resolve_normal_exit_rect(saved, &[second, first]),
        ScreenRect::new(1200, 800, 1400, 1000)
    );
}

// new: the nearest area is chosen by squared distance, not by list order.
#[test]
fn resolve_picks_the_nearest_area_by_squared_distance_not_by_order() {
    let saved = ScreenRect::new(5000, 100, 5800, 700);
    assert_eq!(
        resolve_normal_exit_rect(saved, &[MAIN, SECOND]),
        ScreenRect::new(3040, 100, 3840, 700)
    );
}

// new: a rect larger than the area in one dimension only shrinks that dimension.
#[test]
fn resolve_rect_larger_than_the_area_in_one_dimension_only_shrinks_that_dimension() {
    let saved = ScreenRect::new(-4000, -4000, -3000, -3500); // 1000x500
    let area = ScreenRect::new(0, 0, 800, 1040);
    assert_eq!(
        resolve_normal_exit_rect(saved, &[area]),
        ScreenRect::new(0, 0, 800, 500)
    );
}

// new: extreme coordinates neither panic nor misplace the rect (unchecked C# int arithmetic wraps).
#[test]
fn resolve_extreme_coordinates_do_not_panic_in_debug() {
    let near_max = ScreenRect::new(i32::MAX - 10, 0, i32::MAX, 100);
    assert_eq!(
        resolve_normal_exit_rect(near_max, &[MAIN]),
        ScreenRect::new(1910, 0, 1920, 100)
    );

    let near_min = ScreenRect::new(i32::MIN, i32::MIN, i32::MIN + 100, i32::MIN + 100);
    assert_eq!(
        resolve_normal_exit_rect(near_min, &[MAIN]),
        ScreenRect::new(0, 0, 100, 100)
    );
}

// new: the input slice and the rect are not modified (value semantics, order preserved).
#[test]
fn resolve_does_not_modify_its_inputs() {
    let saved = ScreenRect::new(5000, 100, 5800, 700);
    let works = vec![MAIN, SECOND];
    let works_before = works.clone();

    let _ = resolve_normal_exit_rect(saved, &works);

    assert_eq!(works, works_before);
    assert_eq!(saved, ScreenRect::new(5000, 100, 5800, 700));
}
