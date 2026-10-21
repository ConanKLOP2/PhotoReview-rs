//! `ShellOverlay` part 4, auto-hide and fading (PURE over the injected clock, timestamp and settings). Port of `ShellOverlay.cs` lines 427 to 514
//! (`ApplyToolbarVisibility` ... `OnAnimationChanged`), baseline `d92d6aae`, plus the frame wiring that the C# `Animator` and `FrameDelayTimer` did
//! themselves by subscribing to the clock. Unit W8-15h (contract `contracts/W8-15h.md`); moved from the second half of the C# file because `update`,
//! `pointer_moved` and `settings_changed` (15h) call it (D-W8B4O-3).
//!
//! Frame wiring (D-W8B4O-8): `pr_app::overlay::{animator, frame_delay}` do not subscribe to a clock (contract W8-15b). `ShellOverlay` owns ONE subscription
//! to its clock, present while the animator has a tween or either timer runs; its handler (holding a `Weak<ShellOverlay>`) calls `on_frame`.
//! Order inside one frame: animator, toolbar timer, info timer.
//!
//! No `RefCell` borrow is held while `request_render`, the clock or the settings function run (D-W8B4O-7).

use pr_app::coordinators::autohide::{InfoOverlayAutoHidePolicy, ToolbarAutoHidePolicy};
use pr_app::overlay::contracts::Easing;
use pr_app::overlay::element::{ElementBase, ElementNode};
use pr_app::overlay::toolbar;

use super::state::{FADE_IN_MS, FADE_OUT_MS, INFO_LAYER_ID, OverlayState, ShellOverlay};
use crate::host::frame_clock::FrameTick;

/// .NET `TimeSpan` ticks (100 ns) per millisecond: the unit `Animator::fade_to` and `FrameDelayTimer::start` take their durations in.
const TICKS_PER_MS: i64 = 10_000;

/// Which root a fade is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FadeTarget {
    Toolbar,
    InfoLayer,
}

impl FadeTarget {
    fn element_id(self) -> &'static str {
        match self {
            Self::Toolbar => toolbar::PANEL_ID,
            Self::InfoLayer => INFO_LAYER_ID,
        }
    }
}

impl OverlayState {
    /// The `ElementBase` of the faded root (`None` after dispose).
    fn root_base_mut(&mut self, target: FadeTarget) -> Option<&mut ElementBase> {
        match target {
            FadeTarget::Toolbar => self.toolbar_mut().map(|panel| panel.base_mut()),
            FadeTarget::InfoLayer => self.info_layer_mut().map(|layer| layer.base_mut()),
        }
    }
}

impl ShellOverlay {
    /// One frame of the single subscription: `animator.on_frame` (`changed` means `request_render`), then the toolbar timer (when it elapses:
    /// `on_toolbar_hide_timer`), then the info timer (`on_info_idle_timer`); finally `sync_frame_subscription`. `tick` is the host twin of
    /// `pr_app::input::frame_clock::FrameTick`; the animator and timers take the `pr_app` type (the host type re-exports it: same type, no conversion).
    pub(super) fn on_frame(&self, tick: &FrameTick) {
        let changed = {
            let mut guard = self.state.borrow_mut();
            let state = &mut *guard;
            if state.disposed {
                return;
            }
            let mut store = state.host.opacity_store();
            state.animator.on_frame(&mut store, tick).changed
        };
        if changed {
            (self.request_render)();
        }

        let toolbar_elapsed = self.state.borrow_mut().toolbar_timer.on_frame(tick);
        if toolbar_elapsed {
            self.on_toolbar_hide_timer();
        }
        let info_elapsed = self.state.borrow_mut().info_timer.on_frame(tick);
        if info_elapsed {
            self.on_info_idle_timer();
        }
        self.sync_frame_subscription();
    }

    /// Makes the subscription match the need: subscribe when `animator.is_animating() || toolbar_timer.is_running() || info_timer.is_running()`
    /// and there is none; unsubscribe when none of them is and there is one. Called after every operation that can change those three.
    pub(super) fn sync_frame_subscription(&self) {
        let (needed, existing) = {
            let state = self.state.borrow();
            (
                !state.disposed
                    && (state.animator.is_animating()
                        || state.toolbar_timer.is_running()
                        || state.info_timer.is_running()),
                state.frame_subscription,
            )
        };
        match (needed, existing) {
            (true, None) => {
                let weak = self.weak_self.clone();
                let id = self.clock.subscribe(Box::new(move |tick| {
                    if let Some(overlay) = weak.upgrade() {
                        overlay.on_frame(tick);
                    }
                }));
                self.state.borrow_mut().frame_subscription = Some(id);
            }
            (false, Some(id)) => {
                self.state.borrow_mut().frame_subscription = None;
                self.clock.unsubscribe(id);
            }
            _ => {}
        }
    }

    /// C# `ApplyToolbarVisibility()` (lines 429 to 451): `keep = ToolbarAutoHidePolicy::is_shown(settings.toolbar_auto_hide, model.has_images, tools_open,
    /// toolbar.has_keyboard_focus(), toolbar_mouse_inside_hot_zone)`. When `keep`: stop the toolbar timer and fade the toolbar in. Else when it was kept
    /// visible until now: start the timer with `max(1, ToolbarAutoHidePolicy::delay_ms(settings))` ms (the countdown does NOT restart on later moves outside the hot
    /// zone). Finally `toolbar_was_kept_visible = keep`. A timer start requests a frame.
    pub(super) fn apply_toolbar_visibility(&self) {
        let settings = (self.settings)();
        let now = (self.timestamp)();
        let (keep, timer_started) = {
            let mut guard = self.state.borrow_mut();
            let state = &mut *guard;
            let focus_inside = state.toolbar().is_some_and(|panel| panel.has_keyboard_focus());
            let keep = ToolbarAutoHidePolicy::is_shown(
                settings.toolbar_auto_hide,
                state.model.has_images,
                state.tools_open,
                focus_inside,
                state.toolbar_mouse_inside_hot_zone,
            );
            let mut timer_started = false;
            if keep {
                state.toolbar_timer.stop();
            } else if state.toolbar_was_kept_visible {
                // Just became eligible to hide: count down from now (not restarted by later moves outside the hot zone).
                let delay_ms = i64::from(ToolbarAutoHidePolicy::delay_ms(&settings)).max(1);
                timer_started = state.toolbar_timer.start(now, delay_ms * TICKS_PER_MS).is_ok();
            }
            state.toolbar_was_kept_visible = keep;
            (keep, timer_started)
        };
        if keep {
            self.fade(
                FadeTarget::Toolbar,
                true,
                ToolbarAutoHidePolicy::target_opacity(settings.toolbar_opacity_percent) as f32,
            );
        }
        self.sync_frame_subscription();
        if timer_started {
            self.clock.request_frame();
        }
    }

    /// C# `OnToolbarHideTimer()`: fade the toolbar out.
    pub(super) fn on_toolbar_hide_timer(&self) {
        self.set_toolbar_opacity(false);
    }

    /// C# `SetToolbarOpacity(visible, settings)`: `fade(Toolbar, visible, ToolbarAutoHidePolicy::target_opacity(settings.toolbar_opacity_percent))`.
    pub(super) fn set_toolbar_opacity(&self, visible: bool) {
        let settings = (self.settings)();
        self.fade(
            FadeTarget::Toolbar,
            visible,
            ToolbarAutoHidePolicy::target_opacity(settings.toolbar_opacity_percent) as f32,
        );
    }

    /// C# `NoteInfoActivity()` (lines 459 to 470): `info_idle_elapsed = false`, stop the info timer, `apply_info_overlay_visibility()`; when the overlay is
    /// kept visible by a rule (`info_was_kept_visible`) nothing more; else start the info timer with `max(1, InfoOverlayAutoHidePolicy::delay_ms(settings))` ms.
    pub(super) fn note_info_activity(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.info_idle_elapsed = false;
            state.info_timer.stop();
        }
        self.apply_info_overlay_visibility();
        let settings = (self.settings)();
        let now = (self.timestamp)();
        let timer_started = {
            let mut guard = self.state.borrow_mut();
            let state = &mut *guard;
            if state.info_was_kept_visible {
                // Nothing can hide it right now (feature off, message, compare, inactive window): no countdown.
                false
            } else {
                let delay_ms = i64::from(InfoOverlayAutoHidePolicy::delay_ms(&settings)).max(1);
                state.info_timer.start(now, delay_ms * TICKS_PER_MS).is_ok()
            }
        };
        self.sync_frame_subscription();
        if timer_started {
            self.clock.request_frame();
        }
    }

    /// C# `OnInfoIdleTimer()`: `info_idle_elapsed = true`, `apply_info_overlay_visibility()`.
    pub(super) fn on_info_idle_timer(&self) {
        self.state.borrow_mut().info_idle_elapsed = true;
        self.apply_info_overlay_visibility();
    }

    /// C# `ApplyInfoOverlayVisibility()` (lines 478 to 496): `keep = InfoOverlayAutoHidePolicy::must_stay_visible(settings.info_overlay_auto_hide, has_images,
    /// status_needs_attention, compare_visible, is_window_active())`; `was_kept = info_was_kept_visible; info_was_kept_visible = keep`; when `was_kept && !keep`
    /// the delay counts from now: `note_info_activity()` and return; else `Outcome = evaluate(.., info_idle_elapsed)` and `fade(InfoLayer, outcome.opacity > 0, 1.0)`.
    pub(super) fn apply_info_overlay_visibility(&self) {
        let settings = (self.settings)();
        let active = (self.is_window_active)();
        let visible = {
            let mut state = self.state.borrow_mut();
            let model = &state.model;
            let keep = InfoOverlayAutoHidePolicy::must_stay_visible(
                settings.info_overlay_auto_hide,
                model.has_images,
                model.status_needs_attention,
                model.compare_visible,
                active,
            );
            let was_kept = state.info_was_kept_visible;
            state.info_was_kept_visible = keep;
            if was_kept && !keep {
                None
            } else {
                let model = &state.model;
                let outcome = InfoOverlayAutoHidePolicy::evaluate(
                    settings.info_overlay_auto_hide,
                    model.has_images,
                    model.status_needs_attention,
                    model.compare_visible,
                    active,
                    state.info_idle_elapsed,
                );
                Some(outcome.opacity > 0.0)
            }
        };
        match visible {
            // A message or dialog just ended: the delay counts from now, not from the last activity.
            None => self.note_info_activity(),
            Some(visible) => {
                self.fade(FadeTarget::InfoLayer, visible, 1.0);
                self.sync_frame_subscription();
            }
        }
    }

    /// C# `Fade(element, gate, visible, visibleOpacity)` (lines 501 to 512): `target = visible ? visibleOpacity : 0`; the root's `FadeTargetGate::try_change(target)`
    /// false: nothing. Else `animator.fade_to(id, target, 150 ms in / 200 ms out, Linear)` (a returned `request_frame` becomes `clock.request_frame()`, `changed` a
    /// `request_render`), the root's `is_hit_test_visible = visible` AT ONCE (a hidden element is click-through from the start of the fade), `request_render`.
    pub(super) fn fade(&self, target: FadeTarget, visible: bool, visible_opacity: f32) {
        let target_opacity = if visible { visible_opacity } else { 0.0 };
        let effect = {
            let mut guard = self.state.borrow_mut();
            let state = &mut *guard;
            let gate = match target {
                FadeTarget::Toolbar => &mut state.toolbar_gate,
                FadeTarget::InfoLayer => &mut state.info_gate,
            };
            // The gate sees the widened `f32` (C# `double?` compared with a widened `float`).
            if !gate.try_change(f64::from(target_opacity)) {
                return;
            }
            let duration_ms = if visible { FADE_IN_MS } else { FADE_OUT_MS };
            let mut store = state.host.opacity_store();
            let effect = state
                .animator
                .fade_to(
                    &mut store,
                    target.element_id(),
                    target_opacity,
                    duration_ms * TICKS_PER_MS,
                    Easing::Linear,
                )
                .unwrap_or_default();
            if let Some(base) = state.root_base_mut(target) {
                base.is_hit_test_visible = visible;
            }
            effect
        };
        if effect.changed {
            (self.request_render)();
        }
        self.sync_frame_subscription();
        if effect.request_frame {
            self.clock.request_frame();
        }
        (self.request_render)();
    }
}
