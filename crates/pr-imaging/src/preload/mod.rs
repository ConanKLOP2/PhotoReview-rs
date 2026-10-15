//! Background preload: port of `PhotoReview.Imaging/Preload/{IPreloadTarget, PreloadScheduler}.cs` (C# baseline
//! d92d6aae; PARITY I-11). Lead-owned module file: W4-15a owns `target.rs`, `policy.rs`, `queue.rs`; W4-15b owns
//! `scheduler.rs`, `timer.rs`, `worker.rs`. The sibling helpers live in their own units: `crate::preload_order`
//! (W4-05b), `crate::navigation_pace` (W4-06a), `crate::preload_backoff` (W4-06b), `crate::ram_budget` (W4-07a).
//!
//! Target path: `crates/pr-imaging/src/preload/mod.rs` (lead adds `pub mod preload;` to `lib.rs`).

pub mod policy;
pub mod queue;
pub mod scheduler;
pub mod target;
pub mod timer;
mod worker;

pub use policy::{PreloadOutcome, Shape, WHOLE_FOLDER_CACHE_FILL_LIMIT};
pub use scheduler::{PreloadScheduler, PreloadSchedulerDeps, RunHandle, ThresholdOptions};
pub use target::{DelegateMemoryProbe, KeyError, PreloadTarget};
pub use timer::{OneShotTimer, OneShotTimerFactory, ThreadTimerFactory};
