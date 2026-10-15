//! Port of `PhotoReview.Imaging/PreloadWindow.cs`, `PreloadOrderService.cs` and `Preload/PreloadOptions.cs`
//! (C# baseline d92d6aae, PARITY I-11).
//!
//! The preload window (how many images are decoded ahead of / behind the current one), the order in which the
//! scheduler queues image indices around the current one, and the scheduler's fallback options. Pure computation:
//! no I/O, no clock, no globals. All index arithmetic is `i32` with wrapping operations, mirroring C# unchecked `int`.

#![allow(unused_variables, dead_code)] // D11: remove with the last `todo!()`.

use pr_core::settings::app_settings::AppSettings;

/// Size of the window on the side the user is moving towards (C# `PreloadOrderService.ForwardLookahead`).
pub const FORWARD_LOOKAHEAD: i32 = 32;
/// Size of the small window kept behind the user (C# `PreloadOrderService.BackwardLookahead`).
pub const BACKWARD_LOOKAHEAD: i32 = 8;

/// User-configurable preload window (C# `readonly record struct PreloadWindow(int Forward, int Backward)`).
///
/// Deliberately has NO `Default` impl: C# `default(PreloadWindow)` is `(0, 0)` while `PreloadWindow.Default` is
/// `(32, 8)`; use [`PreloadWindow::DEFAULT`] explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreloadWindow {
    /// Lookahead on the side of travel, images.
    pub forward: i32,
    /// Lookahead behind the user, images.
    pub backward: i32,
}

impl PreloadWindow {
    /// C# `PreloadWindow.Default`: the historical window `(32, 8)`.
    pub const DEFAULT: PreloadWindow = PreloadWindow {
        forward: FORWARD_LOOKAHEAD,
        backward: BACKWARD_LOOKAHEAD,
    };

    /// C# `new PreloadWindow(forward, backward)`; no validation (the settings normalizer clamps upstream).
    pub const fn new(forward: i32, backward: i32) -> Self {
        PreloadWindow { forward, backward }
    }

    /// C# `ImageCount`: `forward + backward + 1` (unchecked `int` addition: wraps).
    pub fn image_count(&self) -> i32 {
        todo!("W4-05b")
    }

    /// C# `PreloadWindow.FromSettings(settings)`: `(settings.PreloadForwardCount, settings.PreloadBackwardCount)`,
    /// no clamping. The C# `ArgumentNullException` for a null argument has no Rust twin (a reference is never null).
    pub fn from_settings(settings: &AppSettings) -> PreloadWindow {
        todo!("W4-05b")
    }
}

/// C# `PreloadOrderService.Build(center, count, fullFolder, direction, lead, window)`, materialized.
///
/// Empty when `center` is outside `0..count`. Otherwise (`dir = if direction < 0 { -1 } else { 1 }`,
/// `lead = clamp(lead, 0, count)`), in this order, each index only when inside `0..count`:
/// 1. `center + dir * offset` for `offset` in `lead+1 ..= lead+forward`;
/// 2. `center + dir * offset` for `offset` in `1 ..= lead`;
/// 3. `center - dir * offset` for `offset` in `1 ..= backward`;
///
/// then, only when `full_folder`:
///
/// 4. `index` from `center + dir * (lead + forward + 1)` step `dir` while in range;
/// 5. `index` from `center - dir * (backward + 1)` step `-dir` while in range.
///
/// `clamp(lead, 0, count)` with `count < 0` follows .NET `Math.Clamp` (throws when min > max): unreachable here because
/// `center` in range implies `count >= 1`.
pub fn build_preload_order(
    center: i32,
    count: i32,
    full_folder: bool,
    direction: i32,
    lead: i32,
    window: PreloadWindow,
) -> Vec<i32> {
    todo!("W4-05b")
}

/// C# `Preload.PreloadOptions` record: fallback defaults only (production passes the user's `PerformanceOptions`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreloadOptions {
    /// C# default `8` (a literal, independent of `perf::PRELOAD_WORKER_COUNT`).
    pub worker_count: i32,
    /// C# default `0.80` (NOT the `0.90` of `perf::PRELOAD_MEMORY_LOAD_LIMIT`).
    pub memory_load_limit: f64,
    /// C# default `2L * 1024 * 1024 * 1024` = 2 GiB.
    pub reserve_bytes: i64,
    /// C# default `16L * 1024 * 1024 * 1024` = 16 GiB.
    pub full_folder_threshold_bytes: i64,
    /// C# `Window` init property, default [`PreloadWindow::DEFAULT`].
    pub window: PreloadWindow,
}

impl Default for PreloadOptions {
    /// C# `new PreloadOptions()`.
    fn default() -> Self {
        todo!("W4-05b")
    }
}
