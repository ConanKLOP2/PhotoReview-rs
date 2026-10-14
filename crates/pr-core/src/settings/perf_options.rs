//! Shared performance defaults used by the app, the benchmark and the preload options.
//!
//! Port of `PhotoReview.Core/Settings/PerformanceOptions.cs` (baseline e441d67a; a static class of constants).
//! Contract: `contracts/W1-05/contract.md`. All items are constants (data), so this stub is already complete.
//!
//! C# type to Rust type: `long` is `i64`, `int` is `i32`, `double` is `f64`, `bool` is `bool`. Keep these types: the
//! settings normalizer compares them against `AppSettings` properties of the same C# types.

/// Byte budget of the in-memory preview cache, used only when physical RAM cannot be determined; otherwise
/// [`IMAGE_CACHE_RAM_PERCENT`] decides the budget. 16 GiB.
pub const IMAGE_CACHE_CAPACITY_BYTES: i64 = 16 * 1024 * 1024 * 1024;

/// Default share of physical RAM (percent) for the in-memory preview (+ source-bytes) cache: 16 GiB on a 32 GB machine.
pub const IMAGE_CACHE_RAM_PERCENT: i32 = 50;

/// Largest selectable [`IMAGE_CACHE_RAM_PERCENT`]; the minimum is decided at runtime from the preload window.
pub const MAX_IMAGE_CACHE_RAM_PERCENT: i32 = 90;

/// Absolute floor of [`IMAGE_CACHE_RAM_PERCENT`] when physical RAM is unknown (the real minimum depends on it).
pub const MIN_IMAGE_CACHE_RAM_PERCENT: i32 = 1;

/// Memory kept free for the rest of the system. 2 GiB.
pub const MEMORY_RESERVE_BYTES: i64 = 2 * 1024 * 1024 * 1024;

/// Default number of preload workers.
pub const PRELOAD_WORKER_COUNT: i32 = 8;

/// Upper bound accepted for a hand-edited `PreloadWorkerCount`: each worker holds a decoded image, so a huge value is a
/// memory bomb.
pub const MAX_PRELOAD_WORKER_COUNT: i32 = 64;

/// Preload stops when the system memory load is above this fraction.
pub const PRELOAD_MEMORY_LOAD_LIMIT: f64 = 0.90;

/// Capacity of the on-disk preview cache. 4 GiB.
pub const PREVIEW_DISK_CACHE_CAPACITY_BYTES: i64 = 4 * 1024 * 1024 * 1024;

/// Whether the source-bytes cache is on by default.
pub const USE_SOURCE_BYTES_CACHE: bool = false;

/// Capacity of the source-bytes cache. 16 GiB.
pub const SOURCE_BYTES_CAPACITY_BYTES: i64 = 16 * 1024 * 1024 * 1024;

/// Default forward (direction-of-travel) preload lookahead, in images.
pub const PRELOAD_FORWARD_COUNT: i32 = 32;

/// Default backward preload lookahead, in images.
pub const PRELOAD_BACKWARD_COUNT: i32 = 8;

/// Smallest accepted [`PRELOAD_FORWARD_COUNT`]: at least the current image's neighbour.
pub const MIN_PRELOAD_FORWARD_COUNT: i32 = 1;

/// Smallest accepted [`PRELOAD_BACKWARD_COUNT`]: 0 = no backward preload at all.
pub const MIN_PRELOAD_BACKWARD_COUNT: i32 = 0;

/// Upper bound accepted for a hand-edited preload forward/backward count: each preloaded preview is about 12 MiB at
/// 2304x1280, so a huge value is a memory bomb (like [`MAX_PRELOAD_WORKER_COUNT`]).
pub const MAX_PRELOAD_COUNT: i32 = 500;
