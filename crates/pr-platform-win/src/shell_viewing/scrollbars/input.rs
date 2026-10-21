//! `ShellScrollBars` part 3 (W8-16g, contract `contracts/W8-16g.md`): the mouse handling of `ScrollBars.cs` lines 144-401 minus the geometry queries of
//! `layout.rs`: `TryHandle`, `OnLeftDown`, `OnMove`, `OnLeftUp`, `OnLeave`, `DragTo`, `ApplyAxisOffset`, `PageOnce`, `OnFrame`, the frame hook and
//! `EndInteraction`. Baseline `d92d6aae`.
//!
//! Left press on a thumb: drag (capture the pointer; the offset follows the pointer proportionally, the thumb stays under the grab point, clamped
//! to `0..=max`). Left press on a track: one page (= the viewport length), then repeat at the system keyboard rhythm while held, stopping when the thumb
//! reaches the pointer. Every left press on a bar or the corner is swallowed. Wheel and touchpad messages are not handled here (they go to `ShellInput`).
//!
//! Re-entrancy (D-W8B3H-6, D-W8B4V-9): every field is a `Cell`, no borrow is held across a window, viewport or callback call. `on_left_down` keeps the C#
//! order: `end_interaction(false)`, then the flags, THEN `capture_pointer()`.

use std::rc::Rc;

use pr_app::input::primitives::PointD;
use pr_app::viewing::scrollbar_geometry::{self, ScrollAxis, ScrollBarPart};

use super::state::ShellScrollBars;
use crate::host::contracts::{ShellWindowApi, WindowMessage, WindowMessageHandler};
use crate::shell_input::native::{signed_high_word, signed_low_word};
use crate::shell_viewing::scrollbar_native::{WM_CANCEL_MODE, WM_MOUSE_LEAVE};
use crate::win32::messages::{WM_CAPTURECHANGED, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE};
use crate::win32::structs::Lresult;

impl WindowMessageHandler for ShellScrollBars {
    /// C# `TryHandle`: `WM_LBUTTONDOWN` / `WM_LBUTTONDBLCLK` -> `on_left_down` (`Some(0)` when taken); `WM_MOUSEMOVE` -> `on_move`, never taken;
    /// `WM_LBUTTONUP` -> `on_left_up` (`Some(0)` only when a drag or a paging ended); `WM_CAPTURECHANGED` -> `end_interaction(false)`, not taken;
    /// `WM_CANCELMODE` -> `end_interaction(true)`, not taken; `WM_MOUSELEAVE` -> `on_leave`, not taken; everything else `None`.
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        let x = signed_low_word(message.lparam);
        let y = signed_high_word(message.lparam);
        match message.msg {
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => self.on_left_down(x, y).then_some(0),
            WM_MOUSEMOVE => {
                self.on_move(x, y);
                None
            }
            WM_LBUTTONUP => self.on_left_up().then_some(0),
            WM_CAPTURECHANGED => {
                // The capture is already gone (also when the system took it): end the interaction, do not release again.
                self.end_interaction(false);
                None
            }
            WM_CANCEL_MODE => {
                self.end_interaction(true);
                None
            }
            WM_MOUSE_LEAVE => {
                self.on_leave();
                None
            }
            _ => None,
        }
    }
}

impl ShellScrollBars {
    fn window_api(&self) -> Option<Rc<dyn ShellWindowApi>> {
        self.window.upgrade()
    }

    fn invalidate_window(&self) {
        if let Some(window) = self.window_api() {
            window.invalidate();
        }
    }

    fn capture_window(&self) {
        if let Some(window) = self.window_api() {
            window.capture_pointer();
        }
    }

    /// `viewport.timestamp() + milliseconds_to_ticks(delay)` (the first repeat after the keyboard delay).
    fn arm_repeat_delay(&self) {
        let delay = (self.options.repeat_timing)().0;
        self.next_repeat_ticks.set(
            self.viewport
                .timestamp()
                .saturating_add(self.milliseconds_to_ticks(delay)),
        );
    }

    /// C# `OnLeftDown(pixelX, pixelY)`: see the contract "Press". Returns whether the press was taken (anything but `ScrollBarPart::None`).
    pub(super) fn on_left_down(&self, pixel_x: i32, pixel_y: i32) -> bool {
        let point = self.to_dip(pixel_x, pixel_y);
        let layout = self.current_layout();
        let part = self.hit_test_in(&layout, point);
        if part == ScrollBarPart::None {
            return false;
        }

        self.pointer.set(point);
        self.pointer_inside.set(true);
        // A press whose release was lost. T2: end first, set the flags, THEN capture (the capture can re-enter `WM_CAPTURECHANGED`).
        self.end_interaction(false);
        if let Some(on_press) = &self.on_press {
            on_press();
        }

        let axis = scrollbar_geometry::axis_at(&layout, point);
        match (part, axis) {
            (ScrollBarPart::Thumb, Some(thumb_axis)) => {
                let bar = layout.box_for(thumb_axis);
                self.dragging.set(true);
                self.drag_axis.set(thumb_axis);
                self.drag_grab.set(bar.along(point) - bar.thumb_start());
                self.capture_window();
                self.invalidate_window();
            }
            (ScrollBarPart::TrackBefore | ScrollBarPart::TrackAfter, Some(page_axis)) => {
                self.page_axis.set(page_axis);
                self.page_direction
                    .set(if part == ScrollBarPart::TrackBefore { -1 } else { 1 });
                self.paging.set(true);
                self.page_region_active.set(true);
                self.capture_window();
                self.page_once();
                self.arm_repeat_delay();
                self.hook_frames();
                self.invalidate_window();
            }
            _ => {}
        }
        true // the bars and the corner sit on top of the image: every left press is swallowed
    }

    /// C# `OnMove(pixelX, pixelY)`: updates the pointer; asks `track_mouse_leave` once per entry; drags; or re-arms the page repeat; or invalidates
    /// when the hovered thumb changed.
    pub(super) fn on_move(&self, pixel_x: i32, pixel_y: i32) {
        let point = self.to_dip(pixel_x, pixel_y);
        let hover_before = self.hovered_thumb_axis();
        self.pointer.set(point);
        self.pointer_inside.set(true);
        if !self.tracking.get() {
            if let Some(window) = self.window_api() {
                let hwnd = window.hwnd();
                if hwnd != 0 {
                    self.tracking.set(true);
                    (self.options.track_mouse_leave)(hwnd);
                }
            }
        }

        if self.dragging.get() {
            self.drag_to(point);
            return;
        }

        if self.paging.get() {
            let layout = self.current_layout();
            let over = self.is_over_page_region(&layout, point);
            if over && !self.page_region_active.get() {
                // RepeatButton: re-entering while held restarts the delay (no immediate page).
                self.arm_repeat_delay();
            }
            self.page_region_active.set(over);
            return;
        }

        if self.hovered_thumb_axis() != hover_before {
            self.invalidate_window();
        }
    }

    /// C# `OnLeftUp()`: ends a drag or a paging and releases the capture; `false` (not taken) when neither was active.
    pub(super) fn on_left_up(&self) -> bool {
        if !self.dragging.get() && !self.paging.get() {
            return false;
        }
        self.end_interaction(true);
        true
    }

    /// C# `OnLeave()` (`WM_MOUSELEAVE`): clears the tracking flag and the inside flag; invalidates when a thumb was hovered.
    pub(super) fn on_leave(&self) {
        self.tracking.set(false);
        let had_hover = self.hovered_thumb_axis().is_some();
        self.pointer_inside.set(false);
        if had_hover {
            self.invalidate_window();
        }
    }

    /// C# `DragTo(point)`: `offset_for_thumb_start(box, box.along(point) - drag_grab)` applied to the dragged axis; nothing when the bar is no longer visible.
    pub(super) fn drag_to(&self, point: PointD) {
        let axis = self.drag_axis.get();
        let layout = self.current_layout();
        let bar = layout.box_for(axis);
        if !bar.visible {
            return; // the layout changed mid-drag (Fit...): nothing left to drag
        }
        let offset = scrollbar_geometry::offset_for_thumb_start(bar, bar.along(point) - self.drag_grab.get());
        self.apply_axis_offset(axis, offset);
    }

    /// C# `ApplyAxisOffset(axis, offset)`: `viewport.scroll_to(h, v)` with the other axis unchanged, `viewport.update_layout()` (synchronous), invalidate.
    pub(super) fn apply_axis_offset(&self, axis: ScrollAxis, offset: f64) {
        let mut horizontal = self.viewport.horizontal_offset();
        let mut vertical = self.viewport.vertical_offset();
        match axis {
            ScrollAxis::Vertical => vertical = offset,
            ScrollAxis::Horizontal => horizontal = offset,
        }
        self.viewport.scroll_to(horizontal, vertical);
        self.viewport.update_layout();
        self.invalidate_window();
    }

    /// C# `PageOnce()`: moves the paged axis by `page_direction * box.viewport_length`; nothing when the bar is not visible.
    pub(super) fn page_once(&self) {
        let axis = self.page_axis.get();
        let layout = self.current_layout();
        let bar = layout.box_for(axis);
        if !bar.visible {
            return;
        }
        let current = match axis {
            ScrollAxis::Vertical => self.viewport.vertical_offset(),
            ScrollAxis::Horizontal => self.viewport.horizontal_offset(),
        };
        self.apply_axis_offset(
            axis,
            current + f64::from(self.page_direction.get()) * bar.viewport_length,
        );
    }

    /// C# `OnFrame(sender, e)` (the render-frame hook): not paging -> unhook; otherwise page again when the repeat is due, the pointer is still over
    /// the page region, and stop repeating (`page_region_active = false`) when the thumb arrived under the pointer.
    pub(super) fn on_frame(&self) {
        if !self.paging.get() {
            self.unhook_frames();
            return;
        }

        let now = self.viewport.timestamp();
        if !self.page_region_active.get() || now < self.next_repeat_ticks.get() {
            return;
        }

        // The track button vanishes under the pointer (the thumb arrived): the RepeatButton stops repeating.
        let layout = self.current_layout();
        if !self.is_over_page_region(&layout, self.pointer.get()) {
            self.page_region_active.set(false);
            return;
        }

        self.page_once();
        let interval = (self.options.repeat_timing)().1;
        self.next_repeat_ticks
            .set(now.saturating_add(self.milliseconds_to_ticks(interval)));
    }

    /// C# `MillisecondsToTicks(ms)`: `max(0, ms) * ticks_per_second / 1000`.
    pub(super) fn milliseconds_to_ticks(&self, milliseconds: i32) -> i64 {
        i64::from(milliseconds.max(0)).saturating_mul(self.options.ticks_per_second) / 1000
    }

    /// C# `HookFrames()`: one `viewport.hook_render_frame` (idempotent while hooked).
    pub(super) fn hook_frames(&self) {
        if self.frame_hook.get().is_some() {
            return;
        }
        let weak = self.this.clone();
        let id = self.viewport.hook_render_frame(Box::new(move |_| {
            if let Some(bars) = weak.upgrade() {
                bars.on_frame();
            }
        }));
        self.frame_hook.set(Some(id));
    }

    /// C# `UnhookFrames()`.
    pub(super) fn unhook_frames(&self) {
        if let Some(id) = self.frame_hook.take() {
            self.viewport.unhook_render_frame(id);
        }
    }

    /// C# `EndInteraction(releaseCapture)`: clears drag / paging / region-active, unhooks the frame, and, when something was active, releases the pointer
    /// (only if `release_capture` and the window still holds the capture) and invalidates.
    pub(super) fn end_interaction(&self, release_capture: bool) {
        let was_active = self.dragging.get() || self.paging.get();
        self.dragging.set(false);
        self.paging.set(false);
        self.page_region_active.set(false);
        self.unhook_frames();
        if !was_active {
            return;
        }
        if let Some(window) = self.window_api() {
            if release_capture && window.has_pointer_capture() {
                window.release_pointer();
            }
            window.invalidate();
        }
    }
}
