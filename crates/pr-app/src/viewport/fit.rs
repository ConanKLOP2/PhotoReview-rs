//! Port of `PhotoReview.App.Shared/Coordinators/FitViewController.cs` (46 lines), `Coordinators/IFitSurface.cs` (32 lines),
//! `ViewModels/ViewRefreshOutcome.cs` (10 lines), and the `ZoomDetailRefreshResult` enum of `Coordinators/ZoomDetailLoader.cs`
//! (lines 294 to 310, moved here as its type owner, contract decision D-01d-2).
//! `ViewerStretchMode` is NOT defined here: it lives in `viewport::types` (W8-01a), see D-01d-3.
//!
//! Async becomes a continuation (D-W8-2): `apply_fit_start` runs the C# code up to the render-priority yield and returns
//! `FitStep::AwaitRender(continuation)`. The shell posts `apply_fit_resume` to the UI dispatcher at Render priority (13d) and
//! passes the continuation back. No async runtime, no `Rc<RefCell>`, no controller field (D-01d-5).
use crate::viewport::types::ViewportSnapshot;

/// C# `ZoomDetailRefreshResult` (ZoomDetailLoader.cs:294). Discriminants equal the C# enum values.
/// W8-07g (`coordinators::zoom_detail`) re-exports this type instead of defining its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomDetailRefreshResult {
    /// The shown bitmap is already full resolution (or there is no image): nothing to load.
    AlreadyFullResolution = 0,
    /// The preview already has at least as many pixels as the screen shows.
    PreviewSufficient = 1,
    /// The held original was put on screen at once.
    ShowedOriginal = 2,
    /// A full-resolution decode started; it replaces the preview when done.
    LoadingOriginal = 3,
    /// Like `LoadingOriginal`, after an earlier decode of this image had failed.
    RetryingOriginal = 4,
}

/// C# `ViewRefreshOutcome` record: what the Refresh command changed on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewRefreshOutcome {
    /// The main image was not drawn with HighQuality scaling before (ScalingQuality setting Linear).
    pub scaling_changed: bool,
    /// What happened about the number of source pixels on screen.
    pub resolution: ZoomDetailRefreshResult,
}

/// C# `IFitSurface`: what the Fit convergence loop needs from the image view. The C# `YieldToRenderAsync` is gone:
/// the yield is the `FitStep::AwaitRender` return value, and the shell performs it.
pub trait FitSurface {
    /// C# `ViewportSize`: the size of ImageScroll inside its border, in DIP. Called once per Fit.
    fn viewport_size(&self) -> (f64, f64);
    /// C# `IsLoaded`: false after the window has unloaded.
    fn is_loaded(&self) -> bool;
    /// C# `UpdateLayout()`.
    fn update_layout(&mut self);
    /// C# `UpdateFitSize()`: pushes the viewport size to the viewer and the decode box.
    fn update_fit_size(&mut self);
    /// C# `Capture()` (CaptureViewportSnapshot): read-only.
    fn capture(&self) -> ViewportSnapshot;
    /// C# `ScrollHome()`: scrolls home, then horizontal and vertical offset 0.
    fn scroll_home(&mut self);
}

/// The part of C# `ViewerState` that Fit drives (`ResetFit(width, height)`). Implemented by `viewmodels::viewer_state`
/// (W8-06a, which depends on this unit, so this trait avoids a dependency cycle, D-01d-1).
pub trait FitViewer {
    /// C# `ViewerState.ResetFit(viewportWidth, viewportHeight)`.
    fn reset_fit(&mut self, viewport_width: f64, viewport_height: f64);
}

/// The shared viewport-operation counter (C# `Input/ViewportOperationVersion`, AR13). Implemented by
/// `input::primitives` (W8-05a). Zoom-at-point and Fit share one instance, owned by the shell.
pub trait OperationVersion {
    /// C# `Next()`: starts an operation that supersedes any pending one, returns its version (`++Current`).
    fn next(&mut self) -> u64;
    /// C# `Current`.
    fn current(&self) -> u64;
}

/// C# `ViewportConvergence.IsStableViewport(before, after)`. The caller passes
/// `misc::viewport_convergence::is_stable_viewport` (W8-04e); the argument order is (last, current) (D-01d-4).
pub type StabilityRule = fn(&ViewportSnapshot, &ViewportSnapshot) -> bool;

/// What the shell does next after a Fit step.
#[must_use]
#[derive(Debug, Clone, PartialEq)]
pub enum FitStep {
    /// A pass is waiting for the render-priority yield: post `apply_fit_resume` with this continuation, at Render priority.
    AwaitRender(FitContinuation),
    /// The Fit ended: converged and scrolled home, superseded by a later operation, or the surface is not loaded.
    Finished,
}

/// Opaque state of a Fit that waits for the render yield. The shell owns it while the yield is pending.
/// Each Fit carries its own state, so a superseded Fit still finishes its pending snapshot exactly as C# does.
#[derive(Debug, Clone, PartialEq)]
pub struct FitContinuation {
    /// Version taken by the Fit's `start` (C# `version`).
    version: u64,
    /// 0-based index of the pass that is waiting for the yield (0, 1 or 2; C# `pass`).
    pass: u8,
    /// Snapshot of the previous pass (C# `lastSnapshot`); `None` before the first capture.
    last: Option<ViewportSnapshot>,
}

impl FitContinuation {
    /// The operation version this continuation belongs to.
    pub fn version(&self) -> u64 {
        self.version
    }
}

/// C# `FitViewController.ApplyFitAsync`, from the start up to the first render yield.
/// Order, exactly as C#: `cancel_pan()`, `versions.next()`, `surface.viewport_size()`, `viewer.reset_fit(w, h)`, then pass 0
/// (version and loaded check, `update_layout`, `update_fit_size`, return `AwaitRender`).
/// Returns `Finished` without any layout call when the surface is not loaded (the version cannot be superseded synchronously here).
pub fn apply_fit_start(
    surface: &mut dyn FitSurface,
    viewer: &mut dyn FitViewer,
    versions: &mut dyn OperationVersion,
    cancel_pan: &mut dyn FnMut(),
    stable: StabilityRule,
) -> FitStep {
    let _ = stable;
    cancel_pan();
    let version = versions.next();
    let (width, height) = surface.viewport_size();
    viewer.reset_fit(width, height);
    start_pass(surface, versions, version, 0, None)
}

/// One pass of the convergence loop, up to its render yield (C# loop body before `await`). The version and loaded check
/// comes first; when it fails the Fit ends without any layout call. `pass` is the index of the pass that will wait.
fn start_pass(
    surface: &mut dyn FitSurface,
    versions: &mut dyn OperationVersion,
    version: u64,
    pass: u8,
    last: Option<ViewportSnapshot>,
) -> FitStep {
    if versions.current() != version || !surface.is_loaded() {
        return FitStep::Finished;
    }
    surface.update_layout();
    surface.update_fit_size();
    FitStep::AwaitRender(FitContinuation { version, pass, last })
}

/// C# final section: version and loaded check, `UpdateLayout`, `ScrollHome`.
fn finish(surface: &mut dyn FitSurface, versions: &mut dyn OperationVersion, version: u64) -> FitStep {
    if versions.current() != version || !surface.is_loaded() {
        return FitStep::Finished;
    }
    surface.update_layout();
    surface.scroll_home();
    FitStep::Finished
}

/// The code after `await surface.YieldToRenderAsync()` in C#, followed by the next passes and the final section.
/// Order, exactly as C#: `capture()` (no version check before it), then `stable(last, current)` when `last` is `Some`
/// (break to the final section), else `last = current`; then the next pass starts with the version and loaded check.
/// After the third pass the loop ends and the final section runs: version and loaded check, `update_layout`, `scroll_home`.
/// Note: the final section calls `update_layout` again but NOT `update_fit_size`.
pub fn apply_fit_resume(
    continuation: FitContinuation,
    surface: &mut dyn FitSurface,
    versions: &mut dyn OperationVersion,
    stable: StabilityRule,
) -> FitStep {
    // No version or loaded check before the capture: C# captures even when superseded (D-01d-5).
    let FitContinuation { version, pass, last } = continuation;
    let current = surface.capture();
    if let Some(previous) = last {
        if stable(&previous, &current) {
            // C# `break`: the loop ends and the final section runs.
            return finish(surface, versions, version);
        }
    }
    let next_pass = pass + 1;
    if next_pass < 3 {
        return start_pass(surface, versions, version, next_pass, Some(current));
    }
    finish(surface, versions, version)
}
