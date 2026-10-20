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

use pr_core::model::enums::{FitWidthAnchor, InitialViewMode};

use crate::input::mouse_gestures::resolve_keyboard_zoom_anchor;
use crate::input::pointer::wheel::{PointerContext, PointerInputController};
use crate::input::primitives::{PointD, RectD, SizeD, f64_same};
use crate::input::surface::ImageSurface;
use crate::misc::window_helpers::{self, IMAGE_CENTRE, ZoomImagePoint};
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
        f64_same(self.image_width, other.image_width)
            && f64_same(self.image_height, other.image_height)
            && f64_same(self.extent_width, other.extent_width)
            && f64_same(self.extent_height, other.extent_height)
            && f64_same(self.viewport_width, other.viewport_width)
            && f64_same(self.viewport_height, other.viewport_height)
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
        match &self.kind {
            ContinuationKind::Zoom(pass) => pass.version,
            ContinuationKind::Reanchor(pass) => pass.version,
        }
    }
}

/// C# `CaptureZoomAnchor(mouse)` as a free function (the swap handler has only `&ViewerState` and `&dyn ImageSurface`):
/// `element = surface.to_image_element(mouse)`. When `is_fit` and `surface.source_size()` is `Some` with width > 0 and
/// height > 0: `source_point = window_helpers::calculate_uniform_image_point(SizeD(image_actual_width, image_actual_height),
/// source, element)` then `window_helpers::normalize_image_point(source_point as PointD, source)`. Otherwise
/// `normalize_image_point(element, SizeD(image_actual_width, image_actual_height))`. `to_image_element` is called exactly once
/// per capture (a test pins the call list).
pub(super) fn capture_zoom_anchor(is_fit: bool, surface: &dyn ImageSurface, mouse: PointD) -> ZoomImagePoint {
    let element = surface.to_image_element(mouse);
    let actual = SizeD {
        width: surface.image_actual_width(),
        height: surface.image_actual_height(),
    };
    if is_fit
        && let Some(source) = surface.source_size()
        && source.width > 0.0
        && source.height > 0.0
    {
        let source_point = window_helpers::calculate_uniform_image_point(actual, source, element);
        return window_helpers::normalize_image_point(
            PointD {
                x: source_point.x,
                y: source_point.y,
            },
            source,
        );
    }
    window_helpers::normalize_image_point(element, actual)
}

/// C# `ViewportCentre`: `(viewport_width / 2, viewport_height / 2)`.
pub(super) fn viewport_centre(surface: &dyn ImageSurface) -> PointD {
    PointD {
        x: surface.viewport_width() / 2.0,
        y: surface.viewport_height() / 2.0,
    }
}

impl PointerInputController {
    // ---- keyboard / menu zoom ----

    /// C# `ZoomActualSizeAsync()`: no images gives `Done` with nothing touched; else `cancel_pan`, `stop_kinetic`, then
    /// `zoom_at_point(resolve_keyboard_anchor(), ZoomAction::ActualSize)`.
    pub fn zoom_actual_size(&mut self, ctx: &mut PointerContext<'_>) -> PointerStep {
        if !ctx.commands.has_images() {
            return PointerStep::Done;
        }
        self.cancel_pan(ctx.surface);
        self.stop_kinetic(ctx.surface);
        let point = self.resolve_keyboard_anchor(ctx);
        self.zoom_at_point(ctx, point, ZoomAction::ActualSize)
    }

    /// C# `ZoomInAsync()`: as `zoom_actual_size` with `ZoomAction::ZoomIn`.
    pub fn zoom_in(&mut self, ctx: &mut PointerContext<'_>) -> PointerStep {
        if !ctx.commands.has_images() {
            return PointerStep::Done;
        }
        self.cancel_pan(ctx.surface);
        self.stop_kinetic(ctx.surface);
        let point = self.resolve_keyboard_anchor(ctx);
        self.zoom_at_point(ctx, point, ZoomAction::ZoomIn)
    }

    /// C# `ZoomOutAsync()`: as `zoom_actual_size` with `ZoomAction::ZoomOut`.
    pub fn zoom_out(&mut self, ctx: &mut PointerContext<'_>) -> PointerStep {
        if !ctx.commands.has_images() {
            return PointerStep::Done;
        }
        self.cancel_pan(ctx.surface);
        self.stop_kinetic(ctx.surface);
        let point = self.resolve_keyboard_anchor(ctx);
        self.zoom_at_point(ctx, point, ZoomAction::ZoomOut)
    }

    /// C# `ResolveKeyboardAnchor()`: `mouse_gestures::resolve_keyboard_zoom_anchor(settings.keyboard_zoom_anchor_or_default(),
    /// surface.pointer_position(), viewport_width, viewport_height)`. Used by keyboard / menu zoom and by the click-zoom key
    /// (`click_zoom.rs`); never by the wheel or the mouse click, which anchor at the cursor.
    pub(super) fn resolve_keyboard_anchor(&self, ctx: &PointerContext<'_>) -> PointD {
        resolve_keyboard_zoom_anchor(
            ctx.settings.keyboard_zoom_anchor_or_default(),
            ctx.surface.pointer_position(),
            ctx.surface.viewport_width(),
            ctx.surface.viewport_height(),
        )
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
        let anchor = capture_zoom_anchor(ctx.viewer.is_fit(), ctx.surface, mouse);
        self.zoom_to_image_point(ctx, anchor, mouse, action, AfterZoom::Nothing)
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
        let version = ctx.versions.next_version();
        self.zooms_awaiting_layout += 1;
        self.apply_zoom(ctx, action);
        let placed = if ctx.surface.is_loaded() {
            self.scroll_anchor_to(ctx.surface, anchor, viewport_point);
            Some(Self::capture_layout_metrics(ctx.surface))
        } else {
            None
        };
        PointerStep::AwaitRender(PointerContinuation {
            kind: ContinuationKind::Zoom(ZoomPass {
                version,
                anchor,
                viewport_point,
                placed,
                then,
            }),
        })
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
    ///
    /// `Reanchor` pass: superseded or not loaded gives `Done`; else `scroll_anchor_to(anchor, viewport_point)`, `Done`.
    pub fn resume(&mut self, ctx: &mut PointerContext<'_>, continuation: PointerContinuation) -> PointerStep {
        match continuation.kind {
            ContinuationKind::Reanchor(pass) => {
                if pass.version == ctx.versions.current() && ctx.surface.is_loaded() {
                    self.scroll_anchor_to(ctx.surface, pass.anchor, pass.viewport_point);
                }
                PointerStep::Done
            }
            ContinuationKind::Zoom(pass) => {
                self.zooms_awaiting_layout = self.zooms_awaiting_layout.saturating_sub(1);
                if pass.version != ctx.versions.current() || !ctx.surface.is_loaded() {
                    return PointerStep::Done;
                }
                if pass.placed != Some(Self::capture_layout_metrics(ctx.surface)) {
                    self.scroll_anchor_to(ctx.surface, pass.anchor, pass.viewport_point);
                }
                match pass.then {
                    AfterZoom::Nothing => PointerStep::Done,
                    AfterZoom::CorrectSideScrollbar { reapply, width_only } => {
                        self.correct_for_side_scrollbar(ctx, pass.anchor, reapply, width_only)
                    }
                }
            }
        }
    }

    /// C# `ApplyFitWidth` / `ApplyFitHeight` and the closures of `applyZoom`: runs `action` against `ctx.viewer` and records
    /// the resulting `ViewerUpdate`. `Fit*` first call `viewer.update_viewport(surface.viewport_width(), surface.viewport_height(),
    /// true)`; `ActualSize` is `viewer.zoom_to_actual_size()`; `Nothing` does nothing.
    pub(super) fn apply_zoom(&mut self, ctx: &mut PointerContext<'_>, action: ZoomAction) {
        match action {
            ZoomAction::Nothing => {}
            ZoomAction::WheelZoom(delta) => self.record_viewer_update(ctx.viewer.wheel_zoom(delta)),
            ZoomAction::ZoomIn => self.record_viewer_update(ctx.viewer.zoom_in()),
            ZoomAction::ZoomOut => self.record_viewer_update(ctx.viewer.zoom_out()),
            ZoomAction::ActualSize => self.record_viewer_update(ctx.viewer.zoom_to_actual_size()),
            ZoomAction::SetZoom(value) => self.record_viewer_update(ctx.viewer.set_zoom(value)),
            ZoomAction::FitWidth => {
                self.record_viewer_update(ctx.viewer.update_viewport(
                    ctx.surface.viewport_width(),
                    ctx.surface.viewport_height(),
                    true,
                ));
                self.record_viewer_update(ctx.viewer.zoom_to_fit_width());
            }
            ZoomAction::FitHeight => {
                self.record_viewer_update(ctx.viewer.update_viewport(
                    ctx.surface.viewport_width(),
                    ctx.surface.viewport_height(),
                    true,
                ));
                self.record_viewer_update(ctx.viewer.zoom_to_fit_height());
            }
        }
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
        surface.update_layout();
        let origin = surface.image_origin();
        let offsets = window_helpers::calculate_zoom_to_point_offsets(
            anchor,
            RectD {
                x: origin.x,
                y: origin.y,
                width: surface.image_actual_width(),
                height: surface.image_actual_height(),
            },
            viewport_point,
            PointD {
                x: surface.horizontal_offset(),
                y: surface.vertical_offset(),
            },
            SizeD {
                width: surface.extent_width(),
                height: surface.extent_height(),
            },
            SizeD {
                width: surface.viewport_width(),
                height: surface.viewport_height(),
            },
        );
        surface.scroll_to(offsets.horizontal, offsets.vertical);
    }

    /// C# `CaptureLayoutMetrics()`: image actual size, extent and viewport size from the surface.
    pub(super) fn capture_layout_metrics(surface: &dyn ImageSurface) -> LayoutMetrics {
        LayoutMetrics {
            image_width: surface.image_actual_width(),
            image_height: surface.image_actual_height(),
            extent_width: surface.extent_width(),
            extent_height: surface.extent_height(),
            viewport_width: surface.viewport_width(),
            viewport_height: surface.viewport_height(),
        }
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
        if self.zooms_awaiting_layout > 0
            || !surface.is_loaded()
            || surface.image_actual_width() <= 0.0
            || surface.image_actual_height() <= 0.0
        {
            return;
        }
        let centre = viewport_centre(surface);
        let anchor = capture_zoom_anchor(viewer.is_fit(), surface, centre);
        self.pending_continuation = Some(PointerContinuation {
            kind: ContinuationKind::Reanchor(ReanchorPass {
                version: versions.current(),
                anchor,
                viewport_point: centre,
            }),
        });
    }

    // ---- Fit width / Fit height / initial view ----

    /// C# `FitWidthAsync(fitWidthAnchor)`: no images gives `Done`; else `cancel_pan`, `stop_kinetic`,
    /// `anchor = window_helpers::calculate_fit_width_anchor_point(fit_width_anchor)`, then
    /// `zoom_to_image_point(anchor, viewport_centre, ZoomAction::FitWidth, CorrectSideScrollbar { FitWidth, width_only: true })`.
    /// The anchor is the CALLER's choice (`window_helpers::fit_width_anchor_for`), never the keyboard zoom anchor setting.
    pub fn fit_width(&mut self, ctx: &mut PointerContext<'_>, fit_width_anchor: FitWidthAnchor) -> PointerStep {
        if !ctx.commands.has_images() {
            return PointerStep::Done;
        }
        self.cancel_pan(ctx.surface);
        self.stop_kinetic(ctx.surface);
        let anchor = window_helpers::calculate_fit_width_anchor_point(fit_width_anchor);
        let centre = viewport_centre(ctx.surface);
        self.zoom_to_image_point(
            ctx,
            anchor,
            centre,
            ZoomAction::FitWidth,
            AfterZoom::CorrectSideScrollbar {
                reapply: ZoomAction::FitWidth,
                width_only: true,
            },
        )
    }

    /// C# `FitHeightAsync()`: as `fit_width` with `IMAGE_CENTRE` (0.5, 0.5), `ZoomAction::FitHeight`, `width_only: false`.
    pub fn fit_height(&mut self, ctx: &mut PointerContext<'_>) -> PointerStep {
        if !ctx.commands.has_images() {
            return PointerStep::Done;
        }
        self.cancel_pan(ctx.surface);
        self.stop_kinetic(ctx.surface);
        let centre = viewport_centre(ctx.surface);
        self.zoom_to_image_point(
            ctx,
            IMAGE_CENTRE,
            centre,
            ZoomAction::FitHeight,
            AfterZoom::CorrectSideScrollbar {
                reapply: ZoomAction::FitHeight,
                width_only: false,
            },
        )
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
        if !ctx.commands.has_images() {
            return PointerStep::Done;
        }
        let applied = ctx.viewer.apply_initial_view_mode(
            mode,
            ctx.surface.viewport_width(),
            ctx.surface.viewport_height(),
            click_zoom_percent,
            ctx.settings.keep_zoom_across_images,
        );
        let Some(update) = applied else {
            return PointerStep::Done;
        };
        self.record_viewer_update(update);
        let (anchor, reapply, width_only) = match mode {
            InitialViewMode::FitWidth => (
                window_helpers::calculate_fit_width_anchor_point(ctx.settings.fit_width_anchor),
                ZoomAction::FitWidth,
                true,
            ),
            InitialViewMode::FitHeight => (IMAGE_CENTRE, ZoomAction::FitHeight, false),
            _ => return PointerStep::Done,
        };
        let centre = viewport_centre(ctx.surface);
        self.zoom_to_image_point(
            ctx,
            anchor,
            centre,
            ZoomAction::Nothing,
            AfterZoom::CorrectSideScrollbar { reapply, width_only },
        )
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
        let overflowed = if width_only {
            ctx.surface.extent_width() > ctx.surface.viewport_width() + 0.5
        } else {
            ctx.surface.extent_height() > ctx.surface.viewport_height() + 0.5
        };
        if overflowed {
            let centre = viewport_centre(ctx.surface);
            self.zoom_to_image_point(ctx, anchor, centre, reapply, AfterZoom::Nothing)
        } else {
            PointerStep::Done
        }
    }
}
