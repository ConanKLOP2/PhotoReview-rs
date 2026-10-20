//! Port of `PhotoReview.Shell.Win32/Overlay/Animator.cs` (C-11, WP-17): opacity tweens driven by a frame clock.
//! Also the `IOpacityTarget` seam (C# `Func<string, IOpacityTarget?>` resolver + `Opacity` property) as [`OpacityStore`].
//! Baseline `d92d6aae`. Owned by W8-15b; pure (no clock, no `windows` crate, no `unsafe`).
//!
//! Design (D-W8-1 / D-W8-2, no observers): the animator does not subscribe to the clock itself. The host (`ShellOverlay`,
//! W8-15h, or `CrossfadeLayer`, W8-16) owns ONE frame subscription and forwards each `FrameTick` to [`Animator::on_frame`].
//! The host subscribes when [`Animator::is_animating`] becomes true, unsubscribes when it becomes false, and calls
//! `request_frame` when [`AnimatorEffect::request_frame`] is set. C# `Changed` is returned as [`AnimatorEffect::changed`].

use crate::input::frame_clock::FrameTick;
use crate::overlay::contracts::Easing;

/// The element side of C# `IOpacityTarget` plus the C# resolver `Func<string, IOpacityTarget?>`, as one trait the host implements
/// over its own element table. Only the animator writes opacity; the element reads it back for `element.Opacity == target`.
pub trait OpacityStore {
    /// C# `resolve(id)?.Opacity`: `None` when the element does not exist (C# `null`).
    fn opacity(&self, element_id: &str) -> Option<f32>;

    /// C# `element.Opacity = value` for an element that [`OpacityStore::opacity`] found. The caller guarantees it exists.
    fn set_opacity(&mut self, element_id: &str, opacity: f32);
}

/// What one call changed, for the host to act on (C# side effects of `FadeTo` / `OnFrame`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnimatorEffect {
    /// C# `Changed` fired: at least one element opacity value changed during this call.
    pub changed: bool,
    /// C# `_clock.RequestFrame()` was called: the host must ask its frame clock for one more frame.
    pub request_frame: bool,
}

/// Construction and use errors (C# `ArgumentOutOfRangeException` / `ObjectDisposedException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimatorError {
    /// `ticks_per_second <= 0` (C# `ArgumentOutOfRangeException`).
    InvalidTicksPerSecond(i64),
    /// `fade_to` after `dispose` (C# `ObjectDisposedException`).
    Disposed,
}

impl std::fmt::Display for AnimatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTicksPerSecond(value) => write!(f, "ticks_per_second must be positive, got {value}"),
            Self::Disposed => write!(f, "animator is disposed"),
        }
    }
}

impl std::error::Error for AnimatorError {}

/// One running tween (C# `private sealed class Tween`). `start` is the timestamp of the FIRST frame after `fade_to` (C# `Start`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tween {
    from: f32,
    target: f32,
    /// C# `DurationSeconds` = `duration_ticks / 1e7` (TimeSpan.TotalSeconds), computed once in `fade_to`.
    duration_seconds: f64,
    easing: Easing,
    start: Option<i64>,
}

/// C# `Animator` (`IAnimator` contract, W8-15a): opacity tweens keyed by element id.
///
/// Invariant: `tweens` holds at most one tween per element id; the order of `tweens` is insertion order (never observable,
/// kept deterministic). `is_animating()` is `!tweens.is_empty()`, which is exactly when C# listens to the frame clock.
#[derive(Debug)]
pub struct Animator {
    ticks_per_second: i64,
    tweens: Vec<(String, Tween)>,
    disposed: bool,
}

impl Animator {
    /// C# `Animator(IFrameClock, resolve, long ticksPerSecond)`. Errors with `InvalidTicksPerSecond` when `ticks_per_second <= 0`.
    /// The C# default (`Stopwatch.Frequency`) is NOT a default here: the host passes the frequency it uses for timestamps.
    pub fn new(ticks_per_second: i64) -> Result<Self, AnimatorError> {
        if ticks_per_second <= 0 {
            return Err(AnimatorError::InvalidTicksPerSecond(ticks_per_second));
        }
        Ok(Self {
            ticks_per_second,
            tweens: Vec::new(),
            disposed: false,
        })
    }

    /// C# `IsAnimating`: at least one tween is running.
    pub fn is_animating(&self) -> bool {
        !self.tweens.is_empty()
    }

    /// C# `ActiveCount`: number of running tweens.
    pub fn active_count(&self) -> usize {
        self.tweens.len()
    }

    /// C# `FadeTo(elementId, target, duration, easing)`. `duration_ticks` is a .NET `TimeSpan` tick count (100 ns units; callers
    /// convert a `Duration` with `as_nanos() / 100`); `<= 0` means immediate. Rules, in C# order:
    /// 1. `Disposed` error when disposed (checked first, before anything else).
    /// 2. `target`: NaN becomes 0, otherwise clamped to `0.0..=1.0`.
    /// 3. Element missing (`store.opacity` is `None`): remove any tween for `element_id`, return the default effect.
    /// 4. A running tween for the same element with the same (clamped) target: no-op, default effect.
    /// 5. Otherwise remove the old tween. If `duration_ticks <= 0` or the element already has `target` opacity: set it
    ///    (when it differs, `changed` is set), return. No tween is created and no frame is requested.
    /// 6. Otherwise add a tween from the CURRENT opacity; `request_frame` is set.
    pub fn fade_to(
        &mut self,
        store: &mut dyn OpacityStore,
        element_id: &str,
        target: f32,
        duration_ticks: i64,
        easing: Easing,
    ) -> Result<AnimatorEffect, AnimatorError> {
        // Step 1: disposed check comes first (T10).
        if self.disposed {
            return Err(AnimatorError::Disposed);
        }
        // Step 2: NaN becomes 0 before clamping (T3, f32::clamp alone would keep NaN).
        let target = if target.is_nan() { 0.0 } else { target.clamp(0.0, 1.0) };
        // Step 3: element gone, drop its tween and do nothing else.
        let Some(current) = store.opacity(element_id) else {
            self.remove(element_id);
            return Ok(AnimatorEffect::default());
        };
        // Step 4: a running tween to the same target is left alone (duration is ignored, no restart).
        if self
            .tweens
            .iter()
            .any(|(id, tween)| id == element_id && tween.target == target)
        {
            return Ok(AnimatorEffect::default());
        }
        // Step 5: remove any old tween before the equality check (T2).
        self.remove(element_id);
        if duration_ticks <= 0 || current == target {
            let changed = current != target;
            if changed {
                store.set_opacity(element_id, target);
            }
            return Ok(AnimatorEffect {
                changed,
                request_frame: false,
            });
        }
        // Step 6: a new tween from the current opacity; the start is set on the first frame (semantic 1).
        self.tweens.push((
            element_id.to_owned(),
            Tween {
                from: current,
                target,
                // TimeSpan.TotalSeconds: 100 ns ticks per second is 1e7.
                duration_seconds: duration_ticks as f64 / 1e7,
                easing,
                start: None,
            },
        ));
        Ok(AnimatorEffect {
            changed: false,
            request_frame: true,
        })
    }

    /// C# `Cancel(elementId)`: removes the tween; opacity keeps its current value. No `changed`, no frame request.
    pub fn cancel(&mut self, element_id: &str) {
        self.remove(element_id);
    }

    /// Removes the tween of `element_id`, if any (C# `_tweens.Remove`).
    fn remove(&mut self, element_id: &str) {
        self.tweens.retain(|(id, _)| id != element_id);
    }

    /// C# `OnFrame(sender, e)` for one frame. For each tween in order: an element that `store` no longer has ends its tween
    /// (no write); otherwise `start` is set to `tick.timestamp` on the first frame, `elapsed = max(0, (now - start) / tps)` with
    /// `now - start` as a WRAPPING i64 subtraction, `progress = min(1, elapsed / duration_seconds)`, and the value is `target`
    /// when `progress >= 1`, else `from + (target - from) * ease(easing, progress)` cast to f32. A value is written only when it
    /// differs from the current opacity (`changed`). Tweens with `progress >= 1` end after writing. `request_frame` is never set.
    pub fn on_frame(&mut self, store: &mut dyn OpacityStore, tick: &FrameTick) -> AnimatorEffect {
        let ticks_per_second = self.ticks_per_second as f64;
        let mut changed = false;
        self.tweens.retain_mut(|(element_id, tween)| {
            // An element that is gone ends its tween without a write (T4 keeps `from` for a restored element).
            let Some(current) = store.opacity(element_id) else {
                return false;
            };
            // The first frame after `fade_to` is the start (semantic 1). Wrapping subtraction (T6), as C# unchecked.
            let start = *tween.start.get_or_insert(tick.timestamp);
            let elapsed = (tick.timestamp.wrapping_sub(start) as f64 / ticks_per_second).max(0.0);
            let progress = (elapsed / tween.duration_seconds).min(1.0);
            let value = if progress >= 1.0 {
                tween.target
            } else {
                // Semantic 4: the subtraction is f32, the rest is f64, the result is cast back to f32.
                let delta = tween.target - tween.from;
                (f64::from(tween.from) + f64::from(delta) * ease(tween.easing, progress)) as f32
            };
            // Write only on a real change (T5).
            if value != current {
                store.set_opacity(element_id, value);
                changed = true;
            }
            // A finished tween is removed after its last write.
            progress < 1.0
        });
        AnimatorEffect {
            changed,
            request_frame: false,
        }
    }

    /// C# `Dispose()`: clears all tweens (no writes, no `changed`) and makes later `fade_to` calls return `Disposed`. Idempotent.
    pub fn dispose(&mut self) {
        self.tweens.clear();
        self.disposed = true;
    }
}

/// C# `Animator.Ease` (`internal static`, exposed for the golden): 0..1 -> 0..1.
/// `Linear` is `t`; `QuadraticOut` is `1 - (1 - t) * (1 - t)`, computed in f64 in that operation order.
pub fn ease(easing: Easing, t: f64) -> f64 {
    match easing {
        Easing::Linear => t,
        Easing::QuadraticOut => 1.0 - ((1.0 - t) * (1.0 - t)),
    }
}
