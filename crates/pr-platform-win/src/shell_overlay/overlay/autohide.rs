//! `ShellOverlay` part 4, auto-hide and fading (PURE over the injected clock, timestamp and settings). Port of `ShellOverlay.cs` lines 427 to 514
//! (`ApplyToolbarVisibility` ... `OnAnimationChanged`), baseline `d92d6aae`, plus the frame wiring that the C# `Animator` and `FrameDelayTimer` did
//! themselves by subscribing to the clock. Unit W8-15h (contract `contracts/W8-15h.md`); moved from the second half of the C# file because `update`,
//! `pointer_moved` and `settings_changed` (15h) call it (D-W8B4O-3).
//!
//! Frame wiring (D-W8B4O-8): `pr_app::overlay::{animator, frame_delay}` do not subscribe to a clock (contract W8-15b). `ShellOverlay` owns ONE subscription
//! to its clock, present while the animator has a tween or either timer runs; its handler (holding a `Weak<ShellOverlay>`) calls `on_frame`.
//! Order inside one frame: animator, toolbar timer, info timer.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use super::state::ShellOverlay;
use crate::host::frame_clock::FrameTick;

/// Which root a fade is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FadeTarget {
    Toolbar,
    InfoLayer,
}

impl ShellOverlay {
    /// One frame of the single subscription: `animator.on_frame` (`changed` means `request_render`), then the toolbar timer (when it elapses:
    /// `on_toolbar_hide_timer`), then the info timer (`on_info_idle_timer`); finally `sync_frame_subscription`. `tick` is the host twin of
    /// `pr_app::input::frame_clock::FrameTick`; the animator and timers take the `pr_app` type (same fields, converted by a private helper).
    pub(super) fn on_frame(&self, tick: &FrameTick) {
        todo!()
    }

    /// Makes the subscription match the need: subscribe when `animator.is_animating() || toolbar_timer.is_running() || info_timer.is_running()`
    /// and there is none; unsubscribe when none of them is and there is one. Called after every operation that can change those three.
    pub(super) fn sync_frame_subscription(&self) {
        todo!()
    }

    /// C# `ApplyToolbarVisibility()` (lines 429 to 451): `keep = ToolbarAutoHidePolicy::is_shown(settings.toolbar_auto_hide, model.has_images, tools_open,
    /// toolbar.has_keyboard_focus(), toolbar_mouse_inside_hot_zone)`. When `keep`: stop the toolbar timer and fade the toolbar in. Else when it was kept
    /// visible until now: start the timer with `max(1, ToolbarAutoHidePolicy::delay_ms(settings))` ms (the countdown does NOT restart on later moves outside the hot
    /// zone). Finally `toolbar_was_kept_visible = keep`. A timer start requests a frame.
    pub(super) fn apply_toolbar_visibility(&self) {
        todo!()
    }

    /// C# `OnToolbarHideTimer()`: fade the toolbar out.
    pub(super) fn on_toolbar_hide_timer(&self) {
        todo!()
    }

    /// C# `SetToolbarOpacity(visible, settings)`: `fade(Toolbar, visible, ToolbarAutoHidePolicy::target_opacity(settings.toolbar_opacity_percent))`.
    pub(super) fn set_toolbar_opacity(&self, visible: bool) {
        todo!()
    }

    /// C# `NoteInfoActivity()` (lines 459 to 470): `info_idle_elapsed = false`, stop the info timer, `apply_info_overlay_visibility()`; when the overlay is
    /// kept visible by a rule (`info_was_kept_visible`) nothing more; else start the info timer with `max(1, InfoOverlayAutoHidePolicy::delay_ms(settings))` ms.
    pub(super) fn note_info_activity(&self) {
        todo!()
    }

    /// C# `OnInfoIdleTimer()`: `info_idle_elapsed = true`, `apply_info_overlay_visibility()`.
    pub(super) fn on_info_idle_timer(&self) {
        todo!()
    }

    /// C# `ApplyInfoOverlayVisibility()` (lines 478 to 496): `keep = InfoOverlayAutoHidePolicy::must_stay_visible(settings.info_overlay_auto_hide, has_images,
    /// status_needs_attention, compare_visible, is_window_active())`; `was_kept = info_was_kept_visible; info_was_kept_visible = keep`; when `was_kept && !keep`
    /// the delay counts from now: `note_info_activity()` and return; else `Outcome = evaluate(.., info_idle_elapsed)` and `fade(InfoLayer, outcome.opacity > 0, 1.0)`.
    pub(super) fn apply_info_overlay_visibility(&self) {
        todo!()
    }

    /// C# `Fade(element, gate, visible, visibleOpacity)` (lines 501 to 512): `target = visible ? visibleOpacity : 0`; the root's `FadeTargetGate::try_change(target)`
    /// false: nothing. Else `animator.fade_to(id, target, 150 ms in / 200 ms out, Linear)` (a returned `request_frame` becomes `clock.request_frame()`, `changed` a
    /// `request_render`), the root's `is_hit_test_visible = visible` AT ONCE (a hidden element is click-through from the start of the fade), `request_render`.
    pub(super) fn fade(&self, target: FadeTarget, visible: bool, visible_opacity: f32) {
        todo!()
    }
}
