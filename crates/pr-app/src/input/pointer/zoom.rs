//! Port of `PhotoReview.App.Shared/Input/PointerInputController.cs` (672 lines, baseline `d92d6aae`), W8-05g part 2: zoom at a
//! point (`ZoomAtPointAsync`, `ZoomToImagePointAsync`, `ScrollAnchorTo`, `CaptureZoomAnchor`), the keyboard / menu zoom
//! (`ZoomActualSizeAsync`, `ZoomInAsync`, `ZoomOutAsync`, anchor resolution), Fit width / Fit height / initial view with the
//! side-scrollbar correction, and the bitmap-swap re-anchor. Contract: `contracts/W8-05g.md`.
//!
//! The cut between W8-05g and W8-05h is moved from "line 300" to the method boundary that keeps the dependency direction
//! (05h depends on 05g): `FitWidthAsync` starts at line 290 but needs `CorrectForSideScrollbarAsync`, `ApplyFitWidth` and
//! `CaptureZoomAnchor` (lines 334 to 381), so all of that lives here, and `can_pan`, `cancel_pan`, `stop_kinetic` (used by
//! 05g's wheel and zoom entry points, lines 542 to 553 and 630 to 640) live in `wheel.rs`.
//!
//! D-W8-2: each C# `await _surface.YieldToRenderAsync()` becomes a return of [`PointerStep::AwaitRender`]. The shell posts
//! [`PointerInputController::resume`] at Render priority and passes the continuation back. The surface's own
//! `ImageSurface::yield_to_render` is NOT used by this controller (the shell performs the yield, as for `FitStep`).

// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]
// Stub only (contract text in doc comments, boxed seams): the porter fixes the doc layout / introduces type aliases and removes this line.
#![allow(clippy::doc_lazy_continuation)]

use pr_core::model::enums::{FitWidthAnchor, InitialViewMode};

use crate::input::pointer::wheel::{PointerContext, PointerInputController};
use crate::input::primitives::{PointD, f64_same};
use crate::input::surface::ImageSurface;
use crate::misc::window_helpers::ZoomImagePoint;
use crate::viewmodels::viewer_state::ViewerState;

/// What the controller asks the shell to do after a call that contains a C# `await`.
#[must_use]
#[derive(Debug, Clone)]
pub enum PointerStep {
    /// The C# task would be complete (no render yield, or the pass ended).
    Done,
    /// A render pass is awaited: post `PointerInputController::resume(continuation, ctx)` to the UI dispatcher at Render
    /// priority. The continuation must ALWAYS be resumed (also when the window closed): `resume` itself checks
    /// `is_loaded` and the operation version, and balances the awaiting-layout counter.
    AwaitRender(PointerContinuation),
}

/// The zoom change a pass applies (C# `Action applyZoom`). An enum instead of a closure so continuations are plain data
/// (`Debug`, `Clone`) and a pass can re-apply it (the side-scrollbar correction re-runs the Fit).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoomAction {
    /// C# `static () => { }`: the zoom was already applied by `ViewerState` (initial view); this pass only places the scroll.
    Nothing,
    /// C# `() => _viewer.WheelZoom(delta)`.
    WheelZoom(i32),
    /// C# `() => _viewer.ZoomIn()`.
    ZoomIn,
    /// C# `() => _viewer.ZoomOut()`.
    ZoomOut,
    /// C# `_commands.ZoomActualSize` = `ViewerState.ZoomToActualSize()` (see `PointerCommands`).
    ActualSize,
    /// C# `() => viewer.SetZoom(target)`.
    SetZoom(f64),
    /// C# `ApplyFitWidth`: `viewer.update_viewport(viewport_width, viewport_height, true)` then `viewer.zoom_to_fit_width()`.
    FitWidth,
    /// C# `ApplyFitHeight`: `update_viewport(.., true)` then `zoom_to_fit_height()`.
    FitHeight,
}

/// C# `LayoutMetrics` (`private readonly record struct`): the six numbers compared before and after the render pass.
/// `PartialEq` is by hand with `f64_same` (record struct equality uses `double.Equals`: NaN equals NaN, `0.0 == -0.0`).
#[derive(Debug, Clone, Copy)]
pub(super) struct LayoutMetrics {
    pub(super) image_width: f64,
    pub(super) image_height: f64,
    pub(super) extent_width: f64,
    pub(super) extent_height: f64,
    pub(super) viewport_width: f64,
    pub(super) viewport_height: f64,
}

impl PartialEq for LayoutMetrics {
    fn eq(&self, other: &Self) -> bool {
        todo!()
    }
}

/// What runs after a zoom pass completed and was not superseded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum AfterZoom {
    /// Nothing (plain zoom gestures; the second pass of the scrollbar correction).
    Nothing,
    /// C# `CorrectForSideScrollbarAsync(anchor, reapply, widthOnly)`.
    CorrectSideScrollbar { reapply: ZoomAction, width_only: bool },
}

/// One zoom pass parked at its render yield (C# `ZoomToImagePointAsync` after `await YieldToRenderAsync`).
#[derive(Debug, Clone)]
pub(super) struct ZoomPass {
    /// C# `version`: taken from `versions.next_version()` when the pass started.
    pub(super) version: i64,
    pub(super) anchor: ZoomImagePoint,
    pub(super) viewport_point: PointD,
    /// C# `placed`: `None` when the surface was not loaded when the scroll should have been placed.
    pub(super) placed: Option<LayoutMetrics>,
    pub(super) then: AfterZoom,
}

/// The bitmap-swap re-anchor parked at its render yield (C# `AnchorAfterSwapAsync` after the yield).
#[derive(Debug, Clone)]
pub(super) struct ReanchorPass {
    /// C# `version` argument: `versions.current()` at the moment of the swap (it is NOT incremented).
    pub(super) version: i64,
    pub(super) anchor: ZoomImagePoint,
    pub(super) viewport_point: PointD,
}

/// Opaque state of a controller call that waits for a render pass (like `viewport::fit::FitContinuation`).
#[derive(Debug, Clone)]
pub struct PointerContinuation {
    kind: ContinuationKind,
}

#[derive(Debug, Clone)]
enum ContinuationKind {
    Zoom(ZoomPass),
    Reanchor(ReanchorPass),
}

impl PointerContinuation {
    /// The operation version this continuation belongs to.
    pub fn version(&self) -> i64 {
        todo!()
    }
}

/// C# `CaptureZoomAnchor(mouse)` as a free function (the swap handler has only `&ViewerState` and `&dyn ImageSurface`):
/// `element = surface.to_image_element(mouse)`. When `is_fit` and `surface.source_size()` is `Some` with width > 0 and
/// height > 0: `source_point = window_helpers::calculate_uniform_image_point(SizeD(image_actual_width, image_actual_height),
/// source, element)` then `window_helpers::normalize_image_point(source_point as PointD, source)`. Otherwise
/// `normalize_image_point(element, SizeD(image_actual_width, image_actual_height))`. `to_image_element` is called exactly once
/// per capture (a test pins the call list).
pub(super) fn capture_zoom_anchor(is_fit: bool, surface: &dyn ImageSurface, mouse: PointD) -> ZoomImagePoint {
    todo!()
}

/// C# `ViewportCentre`: `(viewport_width / 2, viewport_height / 2)`.
pub(super) fn viewport_centre(surface: &dyn ImageSurface) -> PointD {
    todo!()
}

impl PointerInputController {
    // ---- keyboard / menu zoom ----

    /// C# `ZoomActualSizeAsync()`: no images gives `Done` with nothing touched; else `cancel_pan`, `stop_kinetic`, then
    /// `zoom_at_point(resolve_keyboard_anchor(), ZoomAction::ActualSize)`.
    pub fn zoom_actual_size(&mut self, ctx: &mut PointerContext<'_>) -> PointerStep {
        todo!()
    }

    /// C# `ZoomInAsync()`: as `zoom_actual_size` with `ZoomAction::ZoomIn`.
    pub fn zoom_in(&mut self, ctx: &mut PointerContext<'_>) -> PointerStep {
        todo!()
    }

    /// C# `ZoomOutAsync()`: as `zoom_actual_size` with `ZoomAction::ZoomOut`.
    pub fn zoom_out(&mut self, ctx: &mut PointerContext<'_>) -> PointerStep {
        todo!()
    }

    /// C# `ResolveKeyboardAnchor()`: `mouse_gestures::resolve_keyboard_zoom_anchor(settings.keyboard_zoom_anchor_or_default(),
    /// surface.pointer_position(), viewport_width, viewport_height)`. Used by keyboard / menu zoom and by the click-zoom key
    /// (`click_zoom.rs`); never by the wheel or the mouse click, which anchor at the cursor.
    pub(super) fn resolve_keyboard_anchor(&self, ctx: &PointerContext<'_>) -> PointD {
        todo!()
    }

    // ---- zoom at a point ----

    /// C# `ZoomAtPointAsync(mouse, applyZoom)` = `zoom_to_image_point(capture_zoom_anchor(viewer.is_fit(), surface, mouse), mouse,
    /// action, AfterZoom::Nothing)`. The anchor is captured BEFORE the zoom is applied.
    pub(super) fn zoom_at_point(
        &mut self,
        ctx: &mut PointerContext<'_>,
        mouse: PointD,
        action: ZoomAction,
    ) -> PointerStep {
        todo!()
    }

    /// C# `ZoomToImagePointAsync(anchor, viewportPoint, applyZoom)` up to its render yield. In this order:
    /// 1. `version = versions.next_version()`; `zooms_awaiting_layout += 1`;
    /// 2. `apply_zoom(action)` (records the `ViewerUpdate`);
    /// 3. when `surface.is_loaded()`: `scroll_anchor_to(anchor, viewport_point)` then `placed = Some(capture_layout_metrics())`
    ///    (the scroll is placed in the SAME operation as the zoom, before the render pass: no ghosting frame);
    /// 4. returns `AwaitRender` with a `ZoomPass { version, anchor, viewport_point, placed, then }`.
    pub(super) fn zoom_to_image_point(
        &mut self,
        ctx: &mut PointerContext<'_>,
        anchor: ZoomImagePoint,
        viewport_point: PointD,
        action: ZoomAction,
        then: AfterZoom,
    ) -> PointerStep {
        todo!()
    }

    /// Resumes a continuation after the render pass (UI thread, Render priority).
    ///
    /// `Zoom` pass, C# code after the yield:
    /// 1. superseded (`version != versions.current()`) or `!surface.is_loaded()`: `zooms_awaiting_layout -= 1`, `Done`
    ///    (the `then` step does NOT run: R04, a superseded Fit must not re-apply itself);
    /// 2. else, when `placed != Some(capture_layout_metrics())` (also when `placed` is `None`), `scroll_anchor_to(anchor,
    ///    viewport_point)` again; then `zooms_awaiting_layout -= 1` (the C# `finally`);
    /// 3. `then`: `Nothing` gives `Done`; `CorrectSideScrollbar` gives `correct_for_side_scrollbar`, which may return a second
    ///    `AwaitRender`.
    /// `Reanchor` pass: superseded or not loaded gives `Done`; else `scroll_anchor_to(anchor, viewport_point)`, `Done`.
    pub fn resume(&mut self, ctx: &mut PointerContext<'_>, continuation: PointerContinuation) -> PointerStep {
        todo!()
    }

    /// C# `ApplyFitWidth` / `ApplyFitHeight` and the closures of `applyZoom`: runs `action` against `ctx.viewer` and records
    /// the resulting `ViewerUpdate`. `Fit*` first call `viewer.update_viewport(surface.viewport_width(), surface.viewport_height(),
    /// true)`; `ActualSize` is `viewer.zoom_to_actual_size()`; `Nothing` does nothing.
    pub(super) fn apply_zoom(&mut self, ctx: &mut PointerContext<'_>, action: ZoomAction) {
        todo!()
    }

    /// C# `ScrollAnchorTo(anchor, viewportPoint)`: `surface.update_layout()`, then
    /// `window_helpers::calculate_zoom_to_point_offsets(anchor, RectD(image_origin, image_actual_width, image_actual_height),
    /// viewport_point, current offsets, extent, viewport)` and ONE `surface.scroll_to(offsets.horizontal, offsets.vertical)`.
    pub(super) fn scroll_anchor_to(
        &self,
        surface: &mut dyn ImageSurface,
        anchor: ZoomImagePoint,
        viewport_point: PointD,
    ) {
        todo!()
    }

    /// C# `CaptureLayoutMetrics()`: image actual size, extent and viewport size from the surface.
    pub(super) fn capture_layout_metrics(surface: &dyn ImageSurface) -> LayoutMetrics {
        todo!()
    }

    // ---- bitmap swap (ADR 0008 amendment R4) ----

    /// C# `OnSourceSizeSwapping` (the `ViewerState.SourceSizeSwapping` handler). The shell calls it from the closure it passes to
    /// `ViewerState::swap_source_size`, with the viewer state BEFORE the size changes. Does nothing when
    /// `zooms_awaiting_layout > 0`, the surface is not loaded, or `image_actual_width <= 0` or `image_actual_height <= 0`
    /// (a NaN size does not stop it). Else `centre = viewport_centre`, `anchor = capture_zoom_anchor(viewer.is_fit(), surface,
    /// centre)` (the layout still has the OLD size), and stores a `Reanchor` continuation with `version = versions.current()`
    /// in `pending_continuation`; the shell fetches it with `take_pending_continuation` and posts the resume at Render priority.
    pub fn on_source_size_swapping(
        &mut self,
        viewer: &ViewerState,
        surface: &dyn ImageSurface,
        versions: &crate::input::operation_version::ViewportOperationVersion,
    ) {
        todo!()
    }

    // ---- Fit width / Fit height / initial view ----

    /// C# `FitWidthAsync(fitWidthAnchor)`: no images gives `Done`; else `cancel_pan`, `stop_kinetic`,
    /// `anchor = window_helpers::calculate_fit_width_anchor_point(fit_width_anchor)`, then
    /// `zoom_to_image_point(anchor, viewport_centre, ZoomAction::FitWidth, CorrectSideScrollbar { FitWidth, width_only: true })`.
    /// The anchor is the CALLER's choice (`window_helpers::fit_width_anchor_for`), never the keyboard zoom anchor setting.
    pub fn fit_width(&mut self, ctx: &mut PointerContext<'_>, fit_width_anchor: FitWidthAnchor) -> PointerStep {
        todo!()
    }

    /// C# `FitHeightAsync()`: as `fit_width` with `IMAGE_CENTRE` (0.5, 0.5), `ZoomAction::FitHeight`, `width_only: false`.
    pub fn fit_height(&mut self, ctx: &mut PointerContext<'_>) -> PointerStep {
        todo!()
    }

    /// C# `ApplyInitialViewAsync(mode, clickZoomPercent)`. No images gives `Done`. Else
    /// `applied = viewer.apply_initial_view_mode(mode, viewport_width, viewport_height, click_zoom_percent,
    /// settings.keep_zoom_across_images)` (records its update; `None` means nothing was done). `None`, or a mode other than
    /// `FitWidth` / `FitHeight`, gives `Done` (a click-zoom level or Fit places no scroll). Else the scroll placement:
    /// anchor = `calculate_fit_width_anchor_point(settings.fit_width_anchor)` for `FitWidth`, `IMAGE_CENTRE` for `FitHeight`;
    /// `zoom_to_image_point(anchor, viewport_centre, ZoomAction::Nothing, CorrectSideScrollbar { reapply: FitWidth / FitHeight,
    /// width_only: mode == FitWidth })`.
    pub fn apply_initial_view(
        &mut self,
        ctx: &mut PointerContext<'_>,
        mode: InitialViewMode,
        click_zoom_percent: i32,
    ) -> PointerStep {
        todo!()
    }

    /// C# `CorrectForSideScrollbarAsync(anchor, reapply, widthOnly)`: `overflowed = extent_width > viewport_width + 0.5` (width
    /// only) or `extent_height > viewport_height + 0.5`; an overflow of exactly 0.5 does NOT count. Overflowed gives a second
    /// `zoom_to_image_point(anchor, viewport_centre, reapply, AfterZoom::Nothing)`, else `Done`. Reached only from `resume`
    /// of a pass that was not superseded.
    pub(super) fn correct_for_side_scrollbar(
        &mut self,
        ctx: &mut PointerContext<'_>,
        anchor: ZoomImagePoint,
        reapply: ZoomAction,
        width_only: bool,
    ) -> PointerStep {
        todo!()
    }
}
