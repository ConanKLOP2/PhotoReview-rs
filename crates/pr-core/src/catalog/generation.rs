//! Port of `PhotoReview.Core/Catalog/GenerationClock.cs` (C# baseline e441d67a).
//!
//! Three independent monotonic counters ("generations") used to drop stale results (INV-1, INV-5, INV-7; ADR
//! R0003 "Cancellation and staleness"): navigation (which image is shown), folder (which folder scan/load is
//! current) and interaction (catalog interactions). A piece of async work records the value returned by `next_*`
//! when it starts and, when it finishes, applies its result only if `is_*_current(recorded)` is still true.
//! Thread-safe and lock-free. Share it between threads as `Arc<GenerationClock>`.
//!
//! Counter type: `i64` (C# `long`, signed, increments wrap on overflow like `Interlocked.Increment`), not the `u64`
//! the ADR mentions, so that the constructor and `current_*` keep the C# signatures.
use std::sync::atomic::{AtomicI64, Ordering};

/// Thread-safe generation clock (C# `GenerationClock`).
#[derive(Debug, Default)]
pub struct GenerationClock {
    navigation: AtomicI64,
    folder: AtomicI64,
    interaction: AtomicI64,
}

impl GenerationClock {
    /// C# `new GenerationClock(initialNavigation, initialFolder, initialInteraction)` (all default 0; `new(0, 0, 0)`
    /// equals `GenerationClock::default()`). Any `i64`, including negative values, is accepted.
    pub fn new(initial_navigation: i64, initial_folder: i64, initial_interaction: i64) -> Self {
        Self {
            navigation: AtomicI64::new(initial_navigation),
            folder: AtomicI64::new(initial_folder),
            interaction: AtomicI64::new(initial_interaction),
        }
    }

    /// `CurrentNavigation`: the current navigation generation.
    pub fn current_navigation(&self) -> i64 {
        self.navigation.load(Ordering::SeqCst)
    }

    /// `NextNavigation`: atomically advances by one (wrapping) and returns the NEW value.
    pub fn next_navigation(&self) -> i64 {
        self.navigation.fetch_add(1, Ordering::SeqCst).wrapping_add(1)
    }

    /// `IsNavigationCurrent`: true when `generation` equals the current navigation generation.
    pub fn is_navigation_current(&self, generation: i64) -> bool {
        self.navigation.load(Ordering::SeqCst) == generation
    }

    /// `CurrentFolder`.
    pub fn current_folder(&self) -> i64 {
        self.folder.load(Ordering::SeqCst)
    }

    /// `NextFolder`: atomically advances by one (wrapping) and returns the NEW value.
    pub fn next_folder(&self) -> i64 {
        self.folder.fetch_add(1, Ordering::SeqCst).wrapping_add(1)
    }

    /// `IsFolderCurrent`.
    pub fn is_folder_current(&self, generation: i64) -> bool {
        self.folder.load(Ordering::SeqCst) == generation
    }

    /// `CurrentInteraction`.
    pub fn current_interaction(&self) -> i64 {
        self.interaction.load(Ordering::SeqCst)
    }

    /// `NextInteraction`: atomically advances by one (wrapping) and returns the NEW value.
    pub fn next_interaction(&self) -> i64 {
        self.interaction.fetch_add(1, Ordering::SeqCst).wrapping_add(1)
    }

    /// `IsInteractionCurrent`.
    pub fn is_interaction_current(&self, generation: i64) -> bool {
        self.interaction.load(Ordering::SeqCst) == generation
    }

    /// `StopForAction` (legacy "StopImageReadsForAction"): advances navigation, folder and interaction by one each,
    /// to invalidate every pending read, decode and scan when a file action begins. Three independent atomic
    /// increments in that order, NOT one atomic step (callers run on the UI thread; consumers only compare tokens).
    pub fn stop_for_action(&self) {
        self.next_navigation();
        self.next_folder();
        self.next_interaction();
    }
}
