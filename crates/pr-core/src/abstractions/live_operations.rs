//! Port of `ILiveOperationRegistry.cs` (Q-R27 decision D): a marker "this journal operation is executing right now"
//! that startup reconcile respects. The Windows implementation is an OS named object (`pr-platform-win`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

/// Keeps an operation live until dropped (C#: the `IDisposable` handle).
pub type LiveMarker = Box<dyn Send>;

pub trait LiveOperationRegistry: Send + Sync {
    /// Marks `operation_id` live until the returned marker is dropped. Must not fail: when the marker cannot be
    /// created the implementation logs and returns a marker that marks nothing.
    fn begin(&self, operation_id: &str) -> LiveMarker;
    /// True while some marker from [`begin`](Self::begin) for `operation_id` is alive.
    fn is_live(&self, operation_id: &str) -> bool;
}

/// Process-local registry: the default when no OS registry is injected (tests, tools). Thread-safe; one id may be
/// begun more than once (reference counted). Ids compare ordinally (case-sensitive), as in C#.
#[derive(Debug, Default, Clone)]
pub struct InProcessLiveOperationRegistry {
    live: Arc<Mutex<HashMap<String, usize>>>,
}

impl InProcessLiveOperationRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

impl LiveOperationRegistry for InProcessLiveOperationRegistry {
    /// # Panics
    /// When `operation_id` is empty (C# throws `ArgumentException`: a caller bug, not a runtime condition).
    fn begin(&self, operation_id: &str) -> LiveMarker {
        assert!(!operation_id.is_empty(), "operation id must not be empty");
        let mut live = self.live.lock().unwrap_or_else(PoisonError::into_inner);
        *live.entry(operation_id.to_owned()).or_insert(0) += 1;
        Box::new(Marker {
            live: Arc::clone(&self.live),
            operation_id: operation_id.to_owned(),
        })
    }

    fn is_live(&self, operation_id: &str) -> bool {
        !operation_id.is_empty()
            && self
                .live
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains_key(operation_id)
    }
}

struct Marker {
    live: Arc<Mutex<HashMap<String, usize>>>,
    operation_id: String,
}

impl Drop for Marker {
    fn drop(&mut self) {
        let mut live = self.live.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = live.get_mut(&self.operation_id) {
            if *count <= 1 {
                live.remove(&self.operation_id);
            } else {
                *count -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_is_reference_counted() {
        let registry = InProcessLiveOperationRegistry::new();
        let first = registry.begin("op-1");
        let second = registry.begin("op-1");
        drop(first);
        assert!(registry.is_live("op-1"));
        drop(second);
        assert!(!registry.is_live("op-1"));
    }

    #[test]
    fn is_live_is_case_sensitive_and_false_for_empty() {
        let registry = InProcessLiveOperationRegistry::new();
        let _marker = registry.begin("Op");
        assert!(!registry.is_live("op"));
        assert!(!registry.is_live(""));
    }

    #[test]
    #[should_panic(expected = "must not be empty")]
    fn begin_with_empty_id_panics() {
        let _ = InProcessLiveOperationRegistry::new().begin("");
    }
}
