//! Port of `tests/PhotoReview.TestSupport/FakeMemoryProbe.cs` (settable snapshot; headroom computed from it).

use std::sync::{Mutex, PoisonError};

use pr_core::abstractions::{MemoryProbe, MemorySnapshot};

#[derive(Debug)]
pub struct FakeMemoryProbe {
    snapshot: Mutex<Option<MemorySnapshot>>,
}

impl FakeMemoryProbe {
    /// 30 % load, 16 GiB available.
    pub fn new() -> Self {
        Self::with(Some(MemorySnapshot {
            load_percent: 30,
            available_bytes: 16 << 30,
        }))
    }

    pub fn with(snapshot: Option<MemorySnapshot>) -> Self {
        Self {
            snapshot: Mutex::new(snapshot),
        }
    }

    pub fn set(&self, snapshot: Option<MemorySnapshot>) {
        *self.snapshot.lock().unwrap_or_else(PoisonError::into_inner) = snapshot;
    }
}

impl Default for FakeMemoryProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryProbe for FakeMemoryProbe {
    /// Same rule as the Windows probe: `load < maximum_load` (percent) and `available >= reserve`. No snapshot = no
    /// headroom.
    fn has_headroom(&self, maximum_load: f64, reserve_bytes: i64) -> bool {
        self.get_snapshot().is_some_and(|s| {
            f64::from(s.load_percent) < maximum_load && i128::from(s.available_bytes) >= i128::from(reserve_bytes)
        })
    }

    fn get_snapshot(&self) -> Option<MemorySnapshot> {
        *self.snapshot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
