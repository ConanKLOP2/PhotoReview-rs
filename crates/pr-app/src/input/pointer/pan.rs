//! Port of `PhotoReview.App.Shared/Input/PointerInputController.cs` (672 lines, baseline `d92d6aae`), W8-05h part 2: the mouse press /
//! move / release state machine (drag pan, click tracking, double-click Fit), the kinetic glide (start, impulse, the render-frame
//! step), arrow-key panning, and the window-close cleanup. Contract: `contracts/W8-05h.md`. The struct is defined in `wheel.rs`
//! (W8-05g); this file adds an `impl` block. `can_pan`, `cancel_pan` and `stop_kinetic` live in `wheel.rs` (05g uses them).
//!
//! The glide is driven by the surface's render frames. The C# code registers its own `EventHandler`. A Rust closure cannot
//! borrow the controller, so the controller registers a NO-OP handler (`Box::new(|_| {})`) only to obtain the [`RenderHookId`]
//! (`hook_render_frame` / `unhook_render_frame` stay balanced, which is what the C# tests observe) and the SHELL routes every
//! frame of a hooked surface to [`PointerInputController::on_kinetic_frame`] while `is_kinetic_hooked()` is true. Recommended
//! B1 change (lead, optional): drop the handler parameter of `ImageSurface::hook_render_frame` (contract O1).

use pr_core::model::enums::KineticGlideSmoothing;

use crate::input::key_id::KeyId;
use crate::input::kinetic_pan::{
    KeyboardPanResult, ScrollBounds, keyboard_pan_consumes_key, keyboard_pan_impulse_velocity, keyboard_pan_step,
};
use crate::input::mouse_gestures::{PointerReleaseAction, classify_release, is_fit_double_click};
use crate::input::pointer::wheel::{PointerContext, PointerInputController};
use crate::input::pointer::zoom::PointerStep;
use crate::input::primitives::{PointD, PointerButton, SizeD};
use crate::input::surface::ImageSurface;
use crate::misc::window_helpers::{calculate_pan_offsets, is_beyond_drag_threshold};

/// Result of [`PointerInputController::on_image_release`]: the C# return value (`e.Handled`) plus the click-zoom pass that C#
/// `FireAndLog`s (its render yield must still be resumed by the shell).
#[derive(Debug, Clone)]
#[must_use]
pub struct PointerRelease {
    /// The value for `e.Handled`.
    pub handled: bool,
    /// `Done`, or the `AwaitRender` of a click-to-zoom started by this release.
    pub step: PointerStep,
}

/// The surface's current scroll state as a `ScrollBounds` (C# `new ScrollBounds(ExtentWidth, ExtentHeight, ViewportWidth,
/// ViewportHeight)`).
fn scroll_bounds(surface: &dyn ImageSurface) -> ScrollBounds {
    ScrollBounds::new(
        surface.extent_width(),
        surface.extent_height(),
        surface.viewport_width(),
        surface.viewport_height(),
    )
}

impl PointerInputController {
    /// True while the glide's render hook is registered (the shell delivers frames to `on_kinetic_frame` only then).
    pub fn is_kinetic_hooked(&self) -> bool {
        self.kinetic_hook.is_some()
    }

    /// C# `OnWindowPreviewMouseDown()`: the window-level tunnel, before the image's handler: `press_stopped_glide = stop_kinetic()`.
    /// Overwrites the flag (a stale `true` from a press that never reached the image is replaced by the next press's own value,
    /// test `GlideStoppedByAnUnrelatedPress_ThenASeparateImageClick_StillTriggersClickToZoom`).
    pub fn on_window_preview_mouse_down(&mut self, surface: &mut dyn ImageSurface) {
        self.press_stopped_glide = self.stop_kinetic(surface);
    }

    /// C# `OnImagePress(changedButton, clickCount, position, timestamp)`; returns `e.Handled`. In this order:
    /// 1. `stopped = press_stopped_glide | stop_kinetic()` with a NON-short-circuit or: `stop_kinetic` runs even when the flag is
    ///    already set; then `press_stopped_glide = false`;
    /// 2. left button and `is_fit_double_click(click_count)`: `cancel_pan`, `commands.apply_fit()`, return true (before the pan check:
    ///    `can_pan` is false at Fit; its release finds no tracked press);
    /// 3. `can_pan = Self::can_pan(..)`; `!can_pan && !settings.click_to_zoom_enabled`: return false (not tracked, no capture);
    /// 4. track: `is_panning = true`, `press_can_pan = can_pan`, `press_consumed = stopped`, `pan_moved = false`,
    ///    `pan_start_point = pan_last_point = position`, `press_timestamp = timestamp`, `pan_velocity.reset()`,
    ///    `pan_velocity.add(0.0, x, y)`;
    /// 5. `can_pan`: `surface.set_pan_cursor(true)`; and when `settings.kinetic_pan_enabled` and the smoothing is not `Off`,
    ///    one read of `surface.display_timing()` (starts the monitor's vblank clock during the drag; the value is dropped);
    /// 6. `surface.capture_mouse()`; return true.
    pub fn on_image_press(
        &mut self,
        ctx: &mut PointerContext<'_>,
        button: PointerButton,
        click_count: i32,
        position: PointD,
        timestamp: i32,
    ) -> bool {
        // Bit-or, not `||`: the glide must be stopped even when the window-level flag is already set.
        let stopped = self.press_stopped_glide | self.stop_kinetic(ctx.surface);
        self.press_stopped_glide = false;

        if button == PointerButton::Left && is_fit_double_click(click_count) {
            self.cancel_pan(ctx.surface);
            ctx.commands.apply_fit();
            return true;
        }

        let can_pan = Self::can_pan(ctx.viewer, ctx.surface);
        if !can_pan && !ctx.settings.click_to_zoom_enabled {
            return false;
        }

        self.is_panning = true;
        self.press_can_pan = can_pan;
        self.press_consumed = stopped;
        self.pan_moved = false;
        self.pan_start_point = position;
        self.pan_last_point = position;
        self.press_timestamp = timestamp;
        self.pan_velocity.reset();
        self.pan_velocity.add(0.0, position.x, position.y);
        if can_pan {
            ctx.surface.set_pan_cursor(true);
            if ctx.settings.kinetic_pan_enabled
                && ctx.settings.kinetic_glide_smoothing_or_default() != KineticGlideSmoothing::Off
            {
                // Reading the timing starts the monitor's vblank clock during the drag; the value itself is not needed.
                let _ = ctx.surface.display_timing();
            }
        }
        ctx.surface.capture_mouse();
        true
    }

    /// C# `OnImageMove(leftButtonPressed, position, timestamp)`; returns `e.Handled`. Not panning or the left button up: false,
    /// nothing changes. Else: `delta = position - pan_last_point`, `pan_last_point = position`,
    /// `pan_velocity.add(timestamp.wrapping_sub(press_timestamp) as f64, x, y)`; while `!pan_moved`, `pan_moved =
    /// window_helpers::is_beyond_drag_threshold(total delta from pan_start_point, surface.drag_threshold())` (sticky: not tested
    /// again once true); when `press_can_pan`, `calculate_pan_offsets(current offsets, delta, extent, viewport)` and one
    /// `surface.scroll_to`; returns `pan_moved`.
    pub fn on_image_move(
        &mut self,
        ctx: &mut PointerContext<'_>,
        left_button_pressed: bool,
        position: PointD,
        timestamp: i32,
    ) -> bool {
        if !self.is_panning || !left_button_pressed {
            return false;
        }

        let delta = PointD {
            x: position.x - self.pan_last_point.x,
            y: position.y - self.pan_last_point.y,
        };
        self.pan_last_point = position;
        self.pan_velocity.add(
            f64::from(timestamp.wrapping_sub(self.press_timestamp)),
            position.x,
            position.y,
        );
        // OC15: once the drag threshold is crossed it stays crossed until the pan ends, so only test it while still below.
        if !self.pan_moved {
            let (minimum_horizontal, minimum_vertical) = ctx.surface.drag_threshold();
            if is_beyond_drag_threshold(
                position.x - self.pan_start_point.x,
                position.y - self.pan_start_point.y,
                minimum_horizontal,
                minimum_vertical,
            ) {
                self.pan_moved = true;
            }
        }

        if self.press_can_pan {
            let offsets = calculate_pan_offsets(
                PointD {
                    x: ctx.surface.horizontal_offset(),
                    y: ctx.surface.vertical_offset(),
                },
                delta,
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
        self.pan_moved
    }

    /// C# `OnImageRelease(position, timestamp)`; returns `e.Handled` (and the click-zoom pass). Not panning: `cancel_pan`, handled
    /// false. Else: `pan_velocity.add(timestamp.wrapping_sub(press_timestamp) as f64, x, y)`;
    /// `action = classify_release(pan_moved, press_can_pan, press_consumed, settings.click_to_zoom_enabled, settings.kinetic_pan_enabled)`;
    /// remember `moved = pan_moved`; `cancel_pan` (capture released, cursor reset, flags cleared) BEFORE the action runs;
    /// `ClickZoom`: `click_zoom(position)` and handled true; `StartKinetic`: `(vx, vy) = pan_velocity.velocity()` then
    /// `start_kinetic(vx, vy)`; `None`: nothing. Finally `if moved { handled = true }`.
    pub fn on_image_release(
        &mut self,
        ctx: &mut PointerContext<'_>,
        position: PointD,
        timestamp: i32,
    ) -> PointerRelease {
        if !self.is_panning {
            self.cancel_pan(ctx.surface);
            return PointerRelease {
                handled: false,
                step: PointerStep::Done,
            };
        }
        let mut handled = false;
        let mut step = PointerStep::Done;
        self.pan_velocity.add(
            f64::from(timestamp.wrapping_sub(self.press_timestamp)),
            position.x,
            position.y,
        );
        let action = classify_release(
            self.pan_moved,
            self.press_can_pan,
            self.press_consumed,
            ctx.settings.click_to_zoom_enabled,
            ctx.settings.kinetic_pan_enabled,
        );
        let moved = self.pan_moved;
        // The classification above reads the flags that `cancel_pan` clears, so it must come first.
        self.cancel_pan(ctx.surface);

        match action {
            PointerReleaseAction::ClickZoom => {
                step = self.click_zoom(ctx, position);
                handled = true;
            }
            PointerReleaseAction::StartKinetic => {
                let (velocity_x, velocity_y) = self.pan_velocity.velocity();
                self.start_kinetic(ctx, velocity_x, velocity_y);
            }
            PointerReleaseAction::None => {}
        }
        if moved {
            handled = true;
        }
        PointerRelease { handled, step }
    }

    /// C# `OnLostCapture()` (MainImage LostMouseCapture): `cancel_pan`.
    pub fn on_lost_capture(&mut self, surface: &mut dyn ImageSurface) {
        self.cancel_pan(surface);
    }

    /// C# private `StartKinetic(pointerVelocityX, pointerVelocityY)`: `kinetic.start(vx, vy)` returning false gives nothing more
    /// (the scroller stopped itself; an existing hook is left as is). Else `glide_clock.start(settings.kinetic_glide_smoothing_or_default(),
    /// ticks_per_ms)`; when not hooked, register the frame hook and remember its id.
    fn start_kinetic(&mut self, ctx: &mut PointerContext<'_>, pointer_velocity_x: f64, pointer_velocity_y: f64) {
        if !self.kinetic.start(pointer_velocity_x, pointer_velocity_y) {
            return;
        }
        self.glide_clock
            .start(ctx.settings.kinetic_glide_smoothing_or_default(), self.ticks_per_ms);
        self.hook_kinetic_frame(ctx.surface);
    }

    /// C# private `StartKineticImpulse(velocityX, velocityY)` (arrow keys with a glide): `was_active = kinetic.is_active()`,
    /// `kinetic.add_impulse(vx, vy)`, `glide_clock.start(..)` ONLY when `!was_active` (an impulse mid-glide keeps its timing),
    /// and register the frame hook only when not already hooked (no re-hook on auto-repeat).
    fn start_kinetic_impulse(&mut self, ctx: &mut PointerContext<'_>, velocity_x: f64, velocity_y: f64) {
        let was_active = self.kinetic.is_active();
        self.kinetic.add_impulse(velocity_x, velocity_y);
        if !was_active {
            self.glide_clock
                .start(ctx.settings.kinetic_glide_smoothing_or_default(), self.ticks_per_ms);
        }
        self.hook_kinetic_frame(ctx.surface);
    }

    /// Registers the glide's render hook once (`kinetic_hook` is the single source of truth). The handler is a no-op: the shell
    /// routes the frames to [`PointerInputController::on_kinetic_frame`] while [`PointerInputController::is_kinetic_hooked`].
    fn hook_kinetic_frame(&mut self, surface: &mut dyn ImageSurface) {
        if self.kinetic_hook.is_none() {
            self.kinetic_hook = Some(surface.hook_render_frame(Box::new(|_| {})));
        }
    }

    /// C# `TryPanByArrow(key, isRepeat)`; returns whether the key was used. Arrow keys on a zoomed image move the view by
    /// `settings.arrow_pan_step_percent / 100` of the SHORT side (equal pixels on both axes, D-W8B1-8) instead of navigating.
    /// 1. `(dx, dy)`: `KeyId::LEFT` (-1, 0), `RIGHT` (1, 0), `UP` (0, -1), `DOWN` (0, 1), any other key (0, 0).
    /// 2. `(0, 0)`, or `!surface.is_loaded()`, or `!commands.has_images()`: false.
    /// 3. `bounds = ScrollBounds::new(extent_w, extent_h, viewport_w, viewport_h)`;
    ///    `(result, h, v) = kinetic_pan::keyboard_pan_step(dx, dy, offsets, bounds, percent as f64 / 100.0)`.
    /// 4. `Panned`: with `settings.kinetic_pan_enabled`, `(vx, vy) = keyboard_pan_impulse_velocity(h - h_offset, v - v_offset)`
    ///    then `start_kinetic_impulse(vx, vy)` (NO instant scroll); else `stop_kinetic` then `surface.scroll_to(h, v)`. Returns true.
    ///    `NotScrollable` / `AtEdge`: `keyboard_pan_consumes_key(result, is_repeat, settings.arrow_key_navigates_at_zoom_edge, bounds)`.
    pub fn try_pan_by_arrow(&mut self, ctx: &mut PointerContext<'_>, key: KeyId, is_repeat: bool) -> bool {
        let (dx, dy) = match key {
            KeyId::LEFT => (-1, 0),
            KeyId::RIGHT => (1, 0),
            KeyId::UP => (0, -1),
            KeyId::DOWN => (0, 1),
            _ => (0, 0),
        };
        if (dx, dy) == (0, 0) || !ctx.surface.is_loaded() || !ctx.commands.has_images() {
            return false;
        }
        let bounds = scroll_bounds(ctx.surface);
        let horizontal_offset = ctx.surface.horizontal_offset();
        let vertical_offset = ctx.surface.vertical_offset();
        let (result, horizontal, vertical) = keyboard_pan_step(
            dx,
            dy,
            horizontal_offset,
            vertical_offset,
            bounds,
            f64::from(ctx.settings.arrow_pan_step_percent) / 100.0,
        );
        if result == KeyboardPanResult::Panned {
            if ctx.settings.kinetic_pan_enabled {
                let (velocity_x, velocity_y) =
                    keyboard_pan_impulse_velocity(horizontal - horizontal_offset, vertical - vertical_offset);
                self.start_kinetic_impulse(ctx, velocity_x, velocity_y);
            } else {
                self.stop_kinetic(ctx.surface);
                ctx.surface.scroll_to(horizontal, vertical);
            }
            return true;
        }
        keyboard_pan_consumes_key(result, is_repeat, ctx.settings.arrow_key_navigates_at_zoom_edge, bounds)
    }

    /// C# private `OnKineticFrame(sender, e)`, one render frame of a hooked surface. `rendering_time_ticks` is the frame time of
    /// the render event in `TimeSpan` ticks (100 ns), or `None` when the args carry none (C# `RenderingTime(e)` null).
    /// 1. not `kinetic.is_active()` or not `surface.is_loaded()`: `stop_kinetic`, return;
    /// 2. `None` time: return (nothing else changes);
    /// 3. `elapsed = glide_clock.advance(ticks as f64 / 10_000.0, surface.timestamp(), if glide_clock.needs_display_timing()
    ///    { surface.display_timing() } else { None })` (the timing is read only when the clock needs it; Off never reads it);
    /// 4. `elapsed <= 0`: return; else `(h, v) = kinetic.step(elapsed, offsets, ScrollBounds::new(..))`, `surface.scroll_to(h, v)`,
    ///    and when `!kinetic.is_active()` afterwards, `stop_kinetic` (unhook).
    pub fn on_kinetic_frame(&mut self, ctx: &mut PointerContext<'_>, rendering_time_ticks: Option<i64>) {
        if !self.kinetic.is_active() || !ctx.surface.is_loaded() {
            self.stop_kinetic(ctx.surface);
            return;
        }
        let Some(ticks) = rendering_time_ticks else {
            return;
        };
        let timing = if self.glide_clock.needs_display_timing() {
            ctx.surface.display_timing()
        } else {
            None
        };
        let elapsed = self
            .glide_clock
            .advance(ticks as f64 / 10_000.0, ctx.surface.timestamp(), timing);
        if elapsed <= 0.0 {
            return;
        }

        let (horizontal, vertical) = self.kinetic.step(
            elapsed,
            ctx.surface.horizontal_offset(),
            ctx.surface.vertical_offset(),
            scroll_bounds(ctx.surface),
        );
        ctx.surface.scroll_to(horizontal, vertical);
        if !self.kinetic.is_active() {
            self.stop_kinetic(ctx.surface);
        }
    }

    /// C# `OnWindowClosed()`: `stop_kinetic` then `cancel_pan` (the render-frame event is static in WPF and would keep the window alive).
    pub fn on_window_closed(&mut self, surface: &mut dyn ImageSurface) {
        self.stop_kinetic(surface);
        self.cancel_pan(surface);
    }
}
