//! Port of `PhotoReview.App.Shared/Input/PointerInputController.cs` (672 lines, baseline `d92d6aae`), part 1 of 2 units:
//! W8-05g owns `wheel.rs` (this file: the struct, the seams, wheel and touchpad routing, the shared pan / glide stop helpers)
//! and `zoom.rs` (zoom at a point, keyboard zoom, Fit width / Fit height, the render-pass continuation). W8-05h adds
//! `click_zoom.rs` and `pan.rs`. Contract: `contracts/W8-05g.md` (and `W8-05h.md`).
//!
//! The C# class holds `IImageSurface`, `ViewerState`, `Func<AppSettings>`, `ViewportOperationVersion` and `PointerCommands`
//! for its whole life, and awaits `YieldToRenderAsync` inside zoom and Fit. The Rust form (D-W8-1, D-W8-2):
//! - the controller owns only its OWN state (pan flags, gesture helpers, glide state). Everything it reads or writes
//!   (surface, viewer, settings, version counter, commands, log) is passed per call in a [`PointerContext`], built by the
//!   shell from the objects it owns. No `Rc<RefCell>` graph, no stored references.
//! - an `await` becomes a [`PointerStep::AwaitRender`] return value with a [`PointerContinuation`]: the shell posts the
//!   resume to the UI dispatcher at Render priority (the same pattern as `viewport::fit::FitStep`).
//! - `PropertyChanged` / `ZoomModeChanged` of `ViewerState` become the [`ViewerUpdate`]s the controller accumulates and
//!   hands out with [`PointerInputController::take_viewer_update`].
//! - `async Task` commands (`NextAsync`, `PreviousAsync`, `ApplyFitAsync`) become fire-and-forget calls on
//!   [`PointerCommands`] that only POST the work; the implementation must not call back into the controller synchronously.
//!
//! Pure module: no `unsafe`, no `windows` crate. UI thread only.

use std::fmt;

use pr_core::abstractions::Log;
use pr_core::settings::app_settings::AppSettings;

use crate::input::kinetic_pan::{GlideFrameClock, KineticScroller, PanVelocityTracker};
use crate::input::mouse_gestures::{WheelGestureInterpreter, WheelOutcomeKind};
use crate::input::operation_version::ViewportOperationVersion;
use crate::input::pointer::zoom::{PointerContinuation, PointerStep, ZoomAction};
use crate::input::primitives::{PointD, SizeD};
use crate::input::surface::{ImageSurface, RenderHookId};
use crate::input::touchpad_gestures::{
    self, TouchpadSwipeNavigator, TouchpadWheelAction, TouchpadWheelClassifier, WheelInput,
};
use crate::misc::window_helpers::calculate_pan_offsets;
use crate::viewmodels::viewer_state::{ViewerState, ViewerUpdate};

/// C# `PointerCommands(HasImages, NextAsync, PreviousAsync, ZoomActualSize, ApplyFitAsync)`: what the gestures do outside the
/// image view. Differences from C#:
/// - `NextAsync`, `PreviousAsync`, `ApplyFitAsync` return `()`: the call only posts the request (D-W8-2). The shell
///   implements `apply_fit` by posting `viewport::fit::apply_fit_start` with `PointerInputController::cancel_pan` as its
///   `cancel_pan` callback.
/// - `ZoomActualSize` is NOT a member: in every production wiring it is `MainViewModel.ZoomActualSize`, which is
///   `if (!HasImages) return; _viewerState.ZoomToActualSize()`. The controller checks `has_images` itself and calls
///   `ViewerState::zoom_to_actual_size` directly (a callback could not borrow the viewer the context already holds).
pub trait PointerCommands {
    /// C# `HasImages()`.
    fn has_images(&self) -> bool;
    /// C# `NextAsync()`: show the next image (same path as the Next key). Posts the request.
    fn next(&mut self);
    /// C# `PreviousAsync()`.
    fn previous(&mut self);
    /// C# `ApplyFitAsync()`: Fit with the convergence loop. Posts the request.
    fn apply_fit(&mut self);
}

/// Everything one controller call reads or writes besides its own state. Built by the shell for the duration of ONE call.
/// `settings` is the settings object current at the call (C# `_settings()` is read at each use; within one call it is the
/// same value on the UI thread).
pub struct PointerContext<'a> {
    /// C# `_surface`.
    pub surface: &'a mut dyn ImageSurface,
    /// C# `_viewer`.
    pub viewer: &'a mut ViewerState,
    /// C# `_settings()`.
    pub settings: &'a AppSettings,
    /// C# `_viewportVersion`: shared with the Fit driver.
    pub versions: &'a mut ViewportOperationVersion,
    /// C# `_commands`.
    pub commands: &'a mut dyn PointerCommands,
    /// C# `AppLog` (only the touchpad swipe diagnostics line uses it).
    pub log: &'a dyn Log,
}

impl fmt::Debug for PointerContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PointerContext").finish_non_exhaustive()
    }
}

/// C# `PointerInputController`: wheel (zoom / navigate), zoom at a point, click-to-zoom, drag-pan with kinetic glide, mouse
/// capture. All fields are `pub(super)`: the `impl` blocks of `zoom.rs`, `click_zoom.rs` and `pan.rs` use them.
// W8-05h will read the press, pan, and glide fields when its stub methods are implemented.
#[expect(dead_code, reason = "W8-05h pointer press and kinetic pan methods are still stubs")]
#[derive(Debug)]
pub struct PointerInputController {
    /// C# `_isPanning`: a press is tracked (pan or click).
    pub(super) is_panning: bool,
    /// C# `_panMoved`: the drag threshold was crossed (sticky until the press ends).
    pub(super) pan_moved: bool,
    /// C# `_panStartPoint`.
    pub(super) pan_start_point: PointD,
    /// C# `_panLastPoint`.
    pub(super) pan_last_point: PointD,
    /// C# `_pressCanPan`: the tracked press scrolls the image.
    pub(super) press_can_pan: bool,
    /// C# `_pressConsumed`: the tracked press only stopped a glide; its release is never a click.
    pub(super) press_consumed: bool,
    /// C# `_pressStoppedGlide`: set by the window-level tunnel, read (and cleared) by the image press.
    pub(super) press_stopped_glide: bool,
    /// C# `_pressTimestamp` (ms, `Environment.TickCount` clock; differences use wrapping arithmetic).
    pub(super) press_timestamp: i32,
    /// C# `_lastSeenIndex`, starts at -1.
    pub(super) last_seen_index: i32,
    /// C# `_wheelGestures`.
    pub(super) wheel_gestures: WheelGestureInterpreter,
    /// C# `_touchpadClassifier`.
    pub(super) touchpad_classifier: TouchpadWheelClassifier,
    /// C# `_swipeNavigator`.
    pub(super) swipe_navigator: TouchpadSwipeNavigator,
    /// C# `_panVelocity`.
    pub(super) pan_velocity: PanVelocityTracker,
    /// C# `_kinetic`.
    pub(super) kinetic: KineticScroller,
    /// C# `_kineticHooked` + the handler identity: `Some(id)` while the glide's render hook is registered.
    pub(super) kinetic_hook: Option<RenderHookId>,
    /// C# `_glideClock`.
    pub(super) glide_clock: GlideFrameClock,
    /// C# `_zoomsAwaitingLayout`: zoom passes between "zoom applied" and "render pass done".
    pub(super) zooms_awaiting_layout: u32,
    /// C# `TicksPerMs = Stopwatch.Frequency / 1000.0`. Passed in because the controller has no clock: it must be the tick
    /// rate of `ImageSurface::timestamp` (production: `pr_core::diagnostics::perf::STOPWATCH_FREQUENCY as f64 / 1000.0`).
    pub(super) ticks_per_ms: f64,
    /// A render-pass continuation produced where the C# code `FireAndLog`s a task: the source-size swap re-anchor and
    /// nothing else. Taken by [`PointerInputController::take_pending_continuation`].
    pub(super) pending_continuation: Option<PointerContinuation>,
    /// Union of every `ViewerUpdate` the controller caused since the last [`PointerInputController::take_viewer_update`].
    pub(super) viewer_update: ViewerUpdate,
}

impl PointerInputController {
    /// C# constructor, minus the references (see the module doc) and minus `_viewer.SourceSizeSwapping += OnSourceSizeSwapping`
    /// (the shell passes `PointerInputController::on_source_size_swapping` to `ViewerState::swap_source_size`).
    /// State: nothing tracked, `last_seen_index = -1`, no glide, no hook, no pending continuation, no pending viewer update.
    pub fn new(ticks_per_ms: f64) -> Self {
        Self {
            is_panning: false,
            pan_moved: false,
            pan_start_point: PointD::default(),
            pan_last_point: PointD::default(),
            press_can_pan: false,
            press_consumed: false,
            press_stopped_glide: false,
            press_timestamp: 0,
            last_seen_index: -1,
            wheel_gestures: WheelGestureInterpreter::new(),
            touchpad_classifier: TouchpadWheelClassifier::new(),
            swipe_navigator: TouchpadSwipeNavigator::new(),
            pan_velocity: PanVelocityTracker::new(),
            kinetic: KineticScroller::default(),
            kinetic_hook: None,
            glide_clock: GlideFrameClock::new(),
            zooms_awaiting_layout: 0,
            ticks_per_ms,
            pending_continuation: None,
            viewer_update: ViewerUpdate::default(),
        }
    }

    /// Everything the controller changed on the viewer since the last call (the property set of every `ViewerState` mutator it
    /// called, unioned, and `zoom_mode_changed` OR-ed), then resets the accumulator. The shell repaints from it and stops
    /// the glide on `zoom_mode_changed` (C# `ShellInput.OnZoomModeChanged`).
    pub fn take_viewer_update(&mut self) -> ViewerUpdate {
        std::mem::take(&mut self.viewer_update)
    }

    /// The continuation created by [`PointerInputController::on_source_size_swapping`], if any. The shell calls it right after
    /// `ViewerState::swap_source_size` returns and, when `Some`, posts the resume at Render priority.
    pub fn take_pending_continuation(&mut self) -> Option<PointerContinuation> {
        self.pending_continuation.take()
    }

    /// Adds one viewer mutator result to the accumulator (`changes` union, `zoom_mode_changed` OR).
    pub(super) fn record_viewer_update(&mut self, update: ViewerUpdate) {
        self.viewer_update.changes = self.viewer_update.changes.union(update.changes);
        self.viewer_update.zoom_mode_changed |= update.zoom_mode_changed;
    }

    /// C# `OnCurrentIndexChanged(currentIndex)`: navigation stops a glide; a full-resolution swap of the same image
    /// (same index) does not. A changed index: remember it, `stop_kinetic`, `wheel_gestures.reset()`. The same index: nothing.
    pub fn on_current_index_changed(&mut self, surface: &mut dyn ImageSurface, current_index: i32) {
        if current_index != self.last_seen_index {
            self.last_seen_index = current_index;
            self.stop_kinetic(surface);
            self.wheel_gestures.reset();
        }
    }

    /// C# `OnWheelAsync(int delta, bool ctrl, PointD position)`: a vertical wheel message with timestamp 0 and no device hint:
    /// `on_wheel(ctx, WheelInput::new(delta, false, ctrl, 0), position)`.
    pub fn on_wheel_vertical(
        &mut self,
        ctx: &mut PointerContext<'_>,
        delta: i32,
        ctrl: bool,
        position: PointD,
    ) -> PointerStep {
        self.on_wheel(ctx, WheelInput::new(delta, false, ctrl, 0), position)
    }

    /// C# `OnWheelAsync(WheelInput input, PointD position)` (ImageScroll PreviewMouseWheel and the WM_MOUSEHWHEEL hook; the caller
    /// has already marked the message handled). In this order:
    /// 1. `stop_kinetic` (always, first).
    /// 2. `is_touchpad = settings.touchpad_swipe_enabled && !input.ctrl && touchpad_classifier.is_touchpad(delta, timestamp, hint)`
    ///    (short-circuit: the classifier is not fed when the feature is off or Ctrl is held).
    /// 3. `touchpad_gestures::route(input, settings.touchpad_swipe_enabled, is_touchpad, can_pan())`:
    ///    `Ignore` returns `Done`; `Pan`: `wheel_gestures.reset()`, `swipe_navigator.reset()`, `pan_by_touchpad`, `Done`;
    ///    `Navigate`: `wheel_gestures.reset()`, `swipe = swipe_navigator.handle(delta, timestamp, settings.touchpad_swipe_distance_per_image)`,
    ///    the diagnostics line when `swipe != None` and `ctx.log.enabled()`, `navigate(swipe)`, `Done`; `MouseWheel` falls through.
    /// 4. Mouse wheel: `swipe_navigator.reset()`, `outcome = wheel_gestures.handle(delta, ctrl, settings.mouse_wheel_action)`;
    ///    `Zoom` gives `zoom_at_point(position, ZoomAction::WheelZoom(delta))`, any other outcome `navigate(outcome)` and `Done`.
    ///
    /// The diagnostics line (invariant culture): `Touchpad swipe: {swipe:?} after delta={gesture_total} images={gesture_images}
    /// distancePerImage={distance} hint={hint:?}` (`WheelOutcomeKind` and `WheelDeviceHint` print their variant names, which are
    /// the C# enum names).
    pub fn on_wheel(&mut self, ctx: &mut PointerContext<'_>, input: WheelInput, position: PointD) -> PointerStep {
        self.stop_kinetic(ctx.surface);
        let is_touchpad = ctx.settings.touchpad_swipe_enabled
            && !input.ctrl
            && self
                .touchpad_classifier
                .is_touchpad(input.delta, input.timestamp, input.hint);
        match touchpad_gestures::route(
            input,
            ctx.settings.touchpad_swipe_enabled,
            is_touchpad,
            Self::can_pan(ctx.viewer, ctx.surface),
        ) {
            TouchpadWheelAction::Ignore => return PointerStep::Done,
            TouchpadWheelAction::Pan => {
                self.wheel_gestures.reset();
                self.swipe_navigator.reset();
                self.pan_by_touchpad(ctx, input);
                return PointerStep::Done;
            }
            TouchpadWheelAction::Navigate => {
                self.wheel_gestures.reset();
                let swipe = self.swipe_navigator.handle(
                    input.delta,
                    input.timestamp,
                    ctx.settings.touchpad_swipe_distance_per_image,
                );
                if swipe != WheelOutcomeKind::None && ctx.log.enabled() {
                    ctx.log.info(&format!(
                        "Touchpad swipe: {swipe:?} after delta={} images={} distancePerImage={} hint={:?}",
                        self.swipe_navigator.gesture_total(),
                        self.swipe_navigator.gesture_images(),
                        ctx.settings.touchpad_swipe_distance_per_image,
                        input.hint
                    ));
                }
                self.navigate(ctx, swipe);
                return PointerStep::Done;
            }
            TouchpadWheelAction::MouseWheel => {}
        }
        self.swipe_navigator.reset();
        let outcome = self
            .wheel_gestures
            .handle(input.delta, input.ctrl, ctx.settings.mouse_wheel_action);
        if outcome == WheelOutcomeKind::Zoom {
            self.zoom_at_point(ctx, position, ZoomAction::WheelZoom(input.delta))
        } else {
            self.navigate(ctx, outcome);
            PointerStep::Done
        }
    }

    /// C# `NavigateAsync(outcome)`: `Next` calls `commands.next()` and `Previous` calls `commands.previous()`, each only when
    /// `commands.has_images()`; every other outcome does nothing. Same path as the Next/Previous keys.
    fn navigate(&mut self, ctx: &mut PointerContext<'_>, outcome: crate::input::mouse_gestures::WheelOutcomeKind) {
        match outcome {
            WheelOutcomeKind::Next if ctx.commands.has_images() => ctx.commands.next(),
            WheelOutcomeKind::Previous if ctx.commands.has_images() => ctx.commands.previous(),
            _ => {}
        }
    }

    /// C# `PanByTouchpad(input)`: `(h, v) = touchpad_gestures::pan_offset_delta(input)`, then
    /// `window_helpers::calculate_pan_offsets(current offsets, delta = (-h, -v), extent, viewport)` (the helper takes a POINTER
    /// movement, so the OFFSET change is passed negated, `-0.0` included) and one `surface.scroll_to(offsets)`.
    fn pan_by_touchpad(&mut self, ctx: &mut PointerContext<'_>, input: WheelInput) {
        let (horizontal, vertical) = touchpad_gestures::pan_offset_delta(input);
        let offsets = calculate_pan_offsets(
            PointD {
                x: ctx.surface.horizontal_offset(),
                y: ctx.surface.vertical_offset(),
            },
            PointD {
                x: -horizontal,
                y: -vertical,
            },
            SizeD {
                width: ctx.surface.extent_width(),
                height: ctx.surface.extent_height(),
            },
            SizeD {
                width: ctx.surface.viewport_width(),
                height: ctx.surface.viewport_height(),
            },
        );
        ctx.surface.scroll_to(offsets.horizontal, offsets.vertical);
    }

    /// C# `CanPan()`: `!viewer.is_fit() && (extent_width > viewport_width + 0.5 || extent_height > viewport_height + 0.5)`.
    /// NaN extents give false. Shared by the wheel (here) and the press (`pan.rs`).
    pub(super) fn can_pan(viewer: &ViewerState, surface: &dyn ImageSurface) -> bool {
        !viewer.is_fit()
            && (surface.extent_width() > surface.viewport_width() + 0.5
                || surface.extent_height() > surface.viewport_height() + 0.5)
    }

    /// C# `CancelPan()`, public because the Fit driver calls it first: `is_panning`, `pan_moved`, `press_can_pan` and
    /// `press_consumed` become false, `surface.release_mouse_capture()`, `surface.set_pan_cursor(false)`. Always runs both
    /// surface calls, even when nothing was tracked.
    pub fn cancel_pan(&mut self, surface: &mut dyn ImageSurface) {
        self.is_panning = false;
        self.pan_moved = false;
        self.press_can_pan = false;
        self.press_consumed = false;
        surface.release_mouse_capture();
        surface.set_pan_cursor(false);
    }

    /// C# `StopKinetic()`: `was_active = kinetic.is_active()`, `kinetic.stop()`, and when hooked `surface.unhook_render_frame(id)`
    /// with the stored id and `kinetic_hook = None`. Returns `was_active`. Safe to call when idle.
    pub fn stop_kinetic(&mut self, surface: &mut dyn ImageSurface) -> bool {
        let active = self.kinetic.is_active();
        self.kinetic.stop();
        if let Some(id) = self.kinetic_hook.take() {
            surface.unhook_render_frame(id);
        }
        active
    }
}
