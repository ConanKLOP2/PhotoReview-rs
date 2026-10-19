//! Red tests (W8-01d, test porter) for `pr_app::viewport::fit` (C# `Coordinators/FitViewController.cs`, baseline `d92d6aae`).
//!
//! Ported from `tests/PhotoReview.App.Tests/Coordinators/FitViewControllerTests.cs` and
//! `FitViewControllerMutationGapTests.cs`. Each test names its C# test in a comment. Tests marked "new" have no C# twin
//! (contract "Test plan", section `tests/viewport_fit.rs`).
//!
//! The C# `async` method keeps its locals across the render yield. The Rust port returns the state in `FitStep::AwaitRender`;
//! the harness resumes it, logging `Yield` first, which is the C# trace. Two C# checks belong to the ViewerState twin
//! (W8-06a) and are not asserted here: `IsFit` and `MaxImageWidth`/`MaxImageHeight` of `ResetsTheViewerToFit...`.
//! The stability rule is the real `misc::viewport_convergence::is_stable_viewport` (W8-04e, test-only dependency, D-01d-4).
//!
//! Every test fails through `todo!()` on the stub.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pr_app::misc::viewport_convergence::is_stable_viewport;
use pr_app::viewport::fit::{
    FitContinuation, FitStep, FitSurface, FitViewer, OperationVersion, StabilityRule, apply_fit_resume, apply_fit_start,
};
use pr_app::viewport::types::{ViewerStretchMode, ViewportSnapshot};

/// One shared call log: the surface calls, `CancelPan`, the version calls, `ViewportSize`, `ResetFit`, and `Yield`.
type Log = Rc<RefCell<Vec<String>>>;

/// The calls that make up the C# trace (`FitViewControllerTests.Trace()`); the others are filtered out of `trace()`.
const TRACE_CALLS: [&str; 6] = [
    "CancelPan",
    "UpdateLayout",
    "UpdateFitSize",
    "Yield",
    "Capture",
    "ScrollHome",
];

/// C# `FitViewControllerTests.Snapshot(double viewportWidth)`: every value is fixed except the viewport width.
fn snapshot(viewport_width: f64) -> ViewportSnapshot {
    ViewportSnapshot {
        zoom: 1.0,
        stretch: ViewerStretchMode::Uniform,
        max_image_width: viewport_width,
        max_image_height: 600.0,
        actual_image_width: viewport_width,
        actual_image_height: 600.0,
        extent_width: viewport_width,
        extent_height: 600.0,
        viewport_width,
        viewport_height: 600.0,
        horizontal_offset: 0.0,
        vertical_offset: 0.0,
        horizontal_scroll_bar_visible: false,
        vertical_scroll_bar_visible: false,
    }
}

/// C# `FakeFitSurface`. `capture` takes `&self` (contract), so the counters and the loaded flag are `Cell`s.
struct FakeSurface {
    log: Log,
    snapshots: Vec<ViewportSnapshot>,
    captured: Cell<usize>,
    is_loaded: Cell<bool>,
    /// The window unloads right after this many captures (C# `UnloadingSurface`: `if (++Captures == 2) IsLoaded = false`).
    unload_after_capture: Option<usize>,
    viewport: (f64, f64),
}

impl FitSurface for FakeSurface {
    fn viewport_size(&self) -> (f64, f64) {
        self.log.borrow_mut().push("ViewportSize".to_string());
        self.viewport
    }

    fn is_loaded(&self) -> bool {
        self.is_loaded.get()
    }

    fn update_layout(&mut self) {
        self.log.borrow_mut().push("UpdateLayout".to_string());
    }

    fn update_fit_size(&mut self) {
        self.log.borrow_mut().push("UpdateFitSize".to_string());
    }

    fn capture(&self) -> ViewportSnapshot {
        self.log.borrow_mut().push("Capture".to_string());
        let index = self.captured.get();
        self.captured.set(index + 1);
        if self.unload_after_capture == Some(index + 1) {
            self.is_loaded.set(false);
        }
        self.snapshots[index]
    }

    fn scroll_home(&mut self) {
        self.log.borrow_mut().push("ScrollHome".to_string());
    }
}

/// C# `ViewerState.ResetFit`: records the call in the shared log as `ResetFit <width> <height>`.
struct FakeViewer {
    log: Log,
}

impl FitViewer for FakeViewer {
    fn reset_fit(&mut self, viewport_width: f64, viewport_height: f64) {
        self.log
            .borrow_mut()
            .push(format!("ResetFit {viewport_width} {viewport_height}"));
    }
}

/// C# `ViewportOperationVersion`: `next()` is `++Current`, so the first operation gets version 1.
struct FakeVersions {
    log: Log,
    current: u64,
}

impl OperationVersion for FakeVersions {
    fn next(&mut self) -> u64 {
        self.log.borrow_mut().push("Next".to_string());
        self.current += 1;
        self.current
    }

    fn current(&self) -> u64 {
        self.current
    }
}

/// The C# test fixture: one surface, one viewer, one version counter, all sharing one log.
struct Rig {
    log: Log,
    surface: FakeSurface,
    viewer: FakeViewer,
    versions: FakeVersions,
}

impl Rig {
    fn new(snapshots: Vec<ViewportSnapshot>) -> Self {
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        Self {
            surface: FakeSurface {
                log: log.clone(),
                snapshots,
                captured: Cell::new(0),
                is_loaded: Cell::new(true),
                unload_after_capture: None,
                viewport: (800.0, 600.0),
            },
            viewer: FakeViewer { log: log.clone() },
            versions: FakeVersions {
                log: log.clone(),
                current: 0,
            },
            log,
        }
    }

    /// `ApplyFitAsync` up to the first yield. The C# `cancelPan` callback logs `CancelPan`.
    fn start(&mut self, stable: StabilityRule) -> FitStep {
        let cancel_log = self.log.clone();
        let mut cancel_pan = move || cancel_log.borrow_mut().push("CancelPan".to_string());
        apply_fit_start(
            &mut self.surface,
            &mut self.viewer,
            &mut self.versions,
            &mut cancel_pan,
            stable,
        )
    }

    /// One render yield: the C# trace logs `Yield`, then the shell calls `apply_fit_resume` (D-01d-6).
    fn resume(&mut self, continuation: FitContinuation, stable: StabilityRule) -> FitStep {
        self.log.borrow_mut().push("Yield".to_string());
        apply_fit_resume(continuation, &mut self.surface, &mut self.versions, stable)
    }

    /// Resumes until the Fit finishes. The hang guard stops a loop that never ends (no unbounded test runs).
    fn drive(&mut self, mut step: FitStep, stable: StabilityRule) {
        for _ in 0..10 {
            match step {
                FitStep::AwaitRender(continuation) => step = self.resume(continuation, stable),
                FitStep::Finished => return,
            }
        }
        panic!("the Fit did not finish within 10 resumes");
    }

    /// C# `Trace()`: the trace calls joined by spaces, a pass starts with " | UpdateLayout".
    fn trace(&self) -> String {
        let kept: Vec<String> = self
            .log
            .borrow()
            .iter()
            .filter(|entry| TRACE_CALLS.contains(&entry.as_str()))
            .cloned()
            .collect();
        kept.join(" ").replace(" UpdateLayout", " | UpdateLayout")
    }

    fn count(&self, call: &str) -> usize {
        self.log.borrow().iter().filter(|entry| entry.as_str() == call).count()
    }
}

/// The real convergence rule (W8-04e), passed as a `fn` pointer (D-01d-4).
fn real_rule() -> StabilityRule {
    is_stable_viewport
}

/// Extracts the continuation of an `AwaitRender` step; any other step fails the test.
fn expect_await(step: FitStep) -> FitContinuation {
    match step {
        FitStep::AwaitRender(continuation) => continuation,
        FitStep::Finished => panic!("expected AwaitRender, got Finished"),
    }
}

// C#: FitViewControllerTests.StableOnTheSecondPass_StopsThere_ThenScrollsHomeOnce
#[test]
fn stable_on_the_second_pass_stops_there_then_scrolls_home_once() {
    let mut rig = Rig::new(vec![snapshot(800.0), snapshot(800.0), snapshot(900.0)]);

    let step = rig.start(real_rule());
    rig.drive(step, real_rule());

    assert_eq!(
        rig.trace(),
        "CancelPan | UpdateLayout UpdateFitSize Yield Capture | UpdateLayout UpdateFitSize Yield Capture | UpdateLayout ScrollHome"
    );
}

// C#: FitViewControllerTests.NeverStable_RunsAllThreePasses_ThenScrollsHome
#[test]
fn never_stable_runs_all_three_passes_then_scrolls_home() {
    let mut rig = Rig::new(vec![snapshot(800.0), snapshot(820.0), snapshot(840.0), snapshot(860.0)]);

    let step = rig.start(real_rule());
    rig.drive(step, real_rule());

    assert_eq!(rig.count("Capture"), 3);
    assert_eq!(rig.count("UpdateFitSize"), 3);
    assert_eq!(rig.count("ScrollHome"), 1);
    assert_eq!(
        rig.trace(),
        "CancelPan | UpdateLayout UpdateFitSize Yield Capture | UpdateLayout UpdateFitSize Yield Capture | UpdateLayout UpdateFitSize Yield Capture | UpdateLayout ScrollHome"
    );
}

// C#: FitViewControllerTests.ResetsTheViewerToFitWithTheViewportSize_AfterCancellingThePan
// The C# `IsFit`, `MaxImageWidth` and `MaxImageHeight` assertions belong to the ViewerState twin (W8-06a) and are skipped here.
// This test asserts the recorded `reset_fit(1000, 700)` call and that `CancelPan` came first.
#[test]
fn resets_the_viewer_to_fit_with_the_viewport_size_after_cancelling_the_pan() {
    let mut rig = Rig::new(vec![snapshot(800.0), snapshot(800.0)]);
    rig.surface.viewport = (1000.0, 700.0);

    let step = rig.start(real_rule());
    rig.drive(step, real_rule());

    let log = rig.log.borrow();
    let cancel = log.iter().position(|entry| entry == "CancelPan");
    let reset = log.iter().position(|entry| entry == "ResetFit 1000 700");
    assert_eq!(log[0], "CancelPan", "the pan is cancelled first");
    assert!(reset.is_some(), "reset_fit(1000, 700) is recorded");
    assert!(cancel < reset, "CancelPan comes before reset_fit");
}

// C#: FitViewControllerTests.ASecondFitWhileTheFirstWaits_SupersedesIt_SoHomeIsScrolledOnce
// The two continuations are resumed in start order: first, then second (the C# TaskCompletionSource release order).
#[test]
fn a_second_fit_while_the_first_waits_supersedes_it_so_home_is_scrolled_once() {
    let mut rig = Rig::new(vec![snapshot(800.0), snapshot(800.0), snapshot(800.0), snapshot(800.0)]);

    let first = expect_await(rig.start(real_rule()));
    let second = expect_await(rig.start(real_rule()));
    assert_eq!(first.version(), 1);
    assert_eq!(second.version(), 2);

    // The first Fit wakes up after the second took version 2: it captures once, then stops at the loop check.
    let first_next = rig.resume(first, real_rule());
    assert!(matches!(first_next, FitStep::Finished), "the superseded Fit ends");

    // The second Fit continues and converges: stable on its second capture, then the final section.
    let second_next = rig.resume(second, real_rule());
    rig.drive(second_next, real_rule());

    assert_eq!(rig.count("ScrollHome"), 1);
}

// C#: FitViewControllerTests.AZoomWhileTheFitWaits_SupersedesIt
#[test]
fn a_zoom_while_the_fit_waits_supersedes_it() {
    let mut rig = Rig::new(vec![snapshot(800.0), snapshot(820.0), snapshot(840.0)]);

    let continuation = expect_await(rig.start(real_rule()));
    // ZoomAtPoint took a newer version while the Fit waits.
    OperationVersion::next(&mut rig.versions);
    let step = rig.resume(continuation, real_rule());

    assert!(matches!(step, FitStep::Finished));
    assert_eq!(
        rig.count("Capture"),
        1,
        "the pass that was waiting finishes its snapshot, then stops"
    );
    assert_eq!(rig.count("ScrollHome"), 0);
}

// C#: FitViewControllerTests.NotLoaded_DoesNoLayoutWork
#[test]
fn not_loaded_does_no_layout_work() {
    let mut rig = Rig::new(vec![snapshot(800.0)]);
    rig.surface.is_loaded.set(false);

    let step = rig.start(real_rule());

    assert!(matches!(step, FitStep::Finished));
    assert_eq!(
        rig.trace(),
        "CancelPan",
        "only the pan is cancelled; the viewer is reset (not traced)"
    );
    assert!(rig.log.borrow().iter().any(|entry| entry == "ResetFit 800 600"));
}

// C#: FitViewControllerMutationGapTests.ApplyFitAsync_WindowUnloadsAfterTheLoopConverged_DoesNotLayOutOrScrollHome
// The window unloads right after the second capture. The loop converged, but the final section must not lay out or scroll.
#[test]
fn window_unloads_after_the_loop_converged_does_not_lay_out_or_scroll_home() {
    let mut rig = Rig::new(vec![snapshot(800.0), snapshot(800.0)]);
    rig.surface.unload_after_capture = Some(2);

    let step = rig.start(real_rule());
    rig.drive(step, real_rule());

    assert_eq!(rig.count("Capture"), 2);
    assert_eq!(rig.count("ScrollHome"), 0);
    assert_eq!(rig.count("UpdateLayout"), 2, "the two passes only, not the final one");
}

// new: `apply_fit_start` order: cancel_pan, then one version, then the viewport size, then the viewer reset, then pass 0.
#[test]
fn start_cancels_pan_then_takes_one_version_then_resets_the_viewer() {
    let mut rig = Rig::new(vec![snapshot(800.0), snapshot(800.0)]);

    let continuation = expect_await(rig.start(real_rule()));

    assert_eq!(continuation.version(), 1);
    assert_eq!(rig.versions.current(), 1, "exactly one version is taken");
    assert_eq!(
        *rig.log.borrow(),
        vec![
            "CancelPan".to_string(),
            "Next".to_string(),
            "ViewportSize".to_string(),
            "ResetFit 800 600".to_string(),
            "UpdateLayout".to_string(),
            "UpdateFitSize".to_string(),
        ]
    );
}

fn always_stable(_before: &ViewportSnapshot, _after: &ViewportSnapshot) -> bool {
    true
}

// new: the first capture never ends the loop. With an always-true rule the Fit still makes a second pass.
#[test]
fn first_capture_never_ends_the_loop() {
    let mut rig = Rig::new(vec![snapshot(800.0), snapshot(800.0)]);

    let step = rig.start(always_stable);
    rig.drive(step, always_stable);

    assert_eq!(rig.count("Capture"), 2, "the first capture does not stop the loop");
    assert_eq!(rig.count("UpdateLayout"), 3, "pass 0, pass 1, and the final section");
    assert_eq!(rig.count("ScrollHome"), 1);
}

thread_local! {
    /// Arguments of every stability-rule call on this test thread, as (before, after) viewport widths.
    static RULE_ARGUMENTS: RefCell<Vec<(f64, f64)>> = const { RefCell::new(Vec::new()) };
}

fn recording_rule(before: &ViewportSnapshot, after: &ViewportSnapshot) -> bool {
    RULE_ARGUMENTS.with(|recorded| {
        recorded
            .borrow_mut()
            .push((before.viewport_width, after.viewport_width))
    });
    false
}

// new: the stability rule gets (last, current), as C# `IsStableViewport(lastSnapshot, currentSnapshot)`.
#[test]
fn stability_rule_gets_last_then_current() {
    let mut rig = Rig::new(vec![snapshot(800.0), snapshot(820.0), snapshot(840.0)]);

    let step = rig.start(recording_rule);
    rig.drive(step, recording_rule);

    let recorded = RULE_ARGUMENTS.with(|recorded| recorded.borrow().clone());
    assert_eq!(
        recorded,
        vec![(800.0, 820.0), (820.0, 840.0)],
        "the first argument is the previous capture; the first capture calls no rule"
    );
}

// new: a superseded continuation still captures once (it consumes one snapshot), then stops at the loop check.
#[test]
fn superseded_continuation_still_captures_once_then_stops() {
    let mut rig = Rig::new(vec![snapshot(800.0), snapshot(820.0), snapshot(840.0)]);

    let continuation = expect_await(rig.start(real_rule()));
    OperationVersion::next(&mut rig.versions);
    let step = rig.resume(continuation, real_rule());

    assert!(matches!(step, FitStep::Finished));
    assert_eq!(rig.surface.captured.get(), 1, "exactly one snapshot is consumed");
    assert_eq!(rig.count("UpdateLayout"), 1, "no layout after the superseded capture");
}

// new: the final section calls update_layout (not update_fit_size) after the last capture, then scroll_home.
#[test]
fn final_section_uses_update_layout_not_update_fit_size() {
    let mut rig = Rig::new(vec![snapshot(800.0), snapshot(800.0)]);

    let step = rig.start(real_rule());
    rig.drive(step, real_rule());

    let surface_calls: Vec<String> = rig
        .log
        .borrow()
        .iter()
        .filter(|entry| entry.as_str() != "Next" && !entry.starts_with("ResetFit") && entry.as_str() != "ViewportSize")
        .cloned()
        .collect();
    let tail = &surface_calls[surface_calls.len() - 3..];
    assert_eq!(tail, ["Capture", "UpdateLayout", "ScrollHome"]);
    assert_eq!(rig.count("UpdateFitSize"), 2, "update_fit_size runs in the passes only");
}
