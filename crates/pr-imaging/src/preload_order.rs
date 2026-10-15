//! Port of `PhotoReview.Imaging/PreloadWindow.cs`, `PreloadOrderService.cs` and `Preload/PreloadOptions.cs`
//! (C# baseline d92d6aae, PARITY I-11).
//!
//! The preload window (how many images are decoded ahead of / behind the current one), the order in which the
//! scheduler queues image indices around the current one, and the scheduler's fallback options. Pure computation:
//! no I/O, no clock, no globals. All index arithmetic is `i32` with wrapping operations, mirroring C# unchecked `int`.

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
        self.forward.wrapping_add(self.backward).wrapping_add(1)
    }

    /// C# `PreloadWindow.FromSettings(settings)`: `(settings.PreloadForwardCount, settings.PreloadBackwardCount)`,
    /// no clamping. The C# `ArgumentNullException` for a null argument has no Rust twin (a reference is never null).
    pub fn from_settings(settings: &AppSettings) -> PreloadWindow {
        PreloadWindow::new(settings.preload_forward_count, settings.preload_backward_count)
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
    let mut order = Vec::new();
    if !(0..count).contains(&center) {
        return order;
    }
    let dir: i32 = if direction < 0 { -1 } else { 1 };
    let lead = lead.clamp(0, count);
    let forward = window.forward;
    let backward = window.backward;

    // Group 1: travel window, starting `lead + 1` images ahead. Bounds wrap like C# unchecked `int`; a wrapped
    // (negative) upper bound yields an empty range, as the C# `for` loop does.
    let travel_end = lead.wrapping_add(forward);
    for offset in lead.wrapping_add(1)..=travel_end {
        push_in_range(&mut order, center.wrapping_add(dir.wrapping_mul(offset)), count);
    }
    // Group 2: the images skipped by a key-held burst, queued right after the shifted window.
    for offset in 1..=lead {
        push_in_range(&mut order, center.wrapping_add(dir.wrapping_mul(offset)), count);
    }
    // Group 3: the small window behind the user.
    for offset in 1..=backward {
        push_in_range(&mut order, center.wrapping_sub(dir.wrapping_mul(offset)), count);
    }
    if !full_folder {
        return order;
    }

    // Group 4: the rest of the folder ahead, stopping at the first out-of-range index (C# `for` with `InRange` bound).
    let mut index = center.wrapping_add(dir.wrapping_mul(lead.wrapping_add(forward).wrapping_add(1)));
    while (0..count).contains(&index) {
        order.push(index);
        index = index.wrapping_add(dir);
    }
    // Group 5: the rest of the folder behind, stopping at the first out-of-range index.
    let mut index = center.wrapping_sub(dir.wrapping_mul(backward.wrapping_add(1)));
    while (0..count).contains(&index) {
        order.push(index);
        index = index.wrapping_sub(dir);
    }
    order
}

/// Appends `index` when it is inside `0..count` (C# `if (InRange(...)) yield return ...`).
fn push_in_range(order: &mut Vec<i32>, index: i32, count: i32) {
    if (0..count).contains(&index) {
        order.push(index);
    }
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
        const GIB: i64 = 1024 * 1024 * 1024;
        PreloadOptions {
            worker_count: 8,
            memory_load_limit: 0.80,
            reserve_bytes: 2 * GIB,
            full_folder_threshold_bytes: 16 * GIB,
            window: PreloadWindow::DEFAULT,
        }
    }
}
