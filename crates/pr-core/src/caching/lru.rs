//! Port of `PhotoReview.Core/Caching/BoundedLruCache.cs` (C# baseline e441d67a).
//!
//! A thread-safe, byte-bounded least-recently-used cache. Each value has a size (from the `size_of` function given at
//! construction, at least 1); the sum of the sizes never exceeds the capacity. Reading ([`try_get`]) and writing
//! ([`set`]) make an entry the most recently used; when a write would exceed the capacity the least recently used
//! entries are dropped first.
//!
//! The C# `IEqualityComparer<TKey>` argument has no Rust twin: key equality is `K: Eq + Hash`. A caller that needs
//! the C# `StringComparer.OrdinalIgnoreCase` passes keys already mapped with `crate::text::ignore_case_key` /
//! `path_key`.
//!
//! [`try_get`]: BoundedLruCache::try_get
//! [`set`]: BoundedLruCache::set
// STUB (contract writer): the bodies are `todo!()`; the porter removes this allow together with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::hash::Hash;
use std::sync::Mutex;

/// The capacity passed to [`BoundedLruCache::new`] was zero or negative (C# `ArgumentOutOfRangeException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidCapacity(pub i64);

impl fmt::Display for InvalidCapacity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "capacity must be greater than zero (was {})", self.0)
    }
}

impl std::error::Error for InvalidCapacity {}

/// One cached value with its recency stamp.
#[derive(Debug)]
struct Slot<V> {
    value: V,
    /// `max(1, size_of(value))`.
    size: i64,
    /// Key into `LruState::order`; larger = more recently used.
    stamp: u64,
}

/// Everything guarded by the lock.
#[derive(Debug)]
struct LruState<K, V> {
    items: HashMap<K, Slot<V>>,
    /// `stamp -> key`, ascending: the FIRST entry is the least recently used. Replaces the C# `LinkedList`.
    order: BTreeMap<u64, K>,
    /// Next stamp to hand out (increment per touch; `u64` never wraps in practice).
    next_stamp: u64,
    /// Sum of `Slot::size` over `items`.
    size: i64,
}

/// Thread-safe, byte-bounded LRU cache (C# `BoundedLruCache<TKey, TValue>`).
pub struct BoundedLruCache<K, V> {
    capacity: i64,
    size_of: Box<dyn Fn(&V) -> i64 + Send + Sync>,
    state: Mutex<LruState<K, V>>,
}

impl<K, V> fmt::Debug for BoundedLruCache<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundedLruCache")
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

impl<K: Eq + Hash + Clone, V: Clone> BoundedLruCache<K, V> {
    /// C# constructor. `capacity` is in the same unit as `size_of` (bytes). `capacity <= 0` is an error
    /// ([`InvalidCapacity`]); `i64::MAX` is allowed.
    pub fn new(capacity: i64, size_of: impl Fn(&V) -> i64 + Send + Sync + 'static) -> Result<Self, InvalidCapacity> {
        todo!()
    }

    /// Number of entries.
    pub fn count(&self) -> usize {
        todo!()
    }

    /// Sum of the entry sizes (always `<= capacity`).
    pub fn current_size(&self) -> i64 {
        todo!()
    }

    /// `TryGet`: on a hit returns a clone of the value and makes the entry the most recently used; on a miss
    /// returns `None` and changes nothing.
    pub fn try_get(&self, key: &K) -> Option<V> {
        todo!()
    }

    /// `Set`: insert or replace.
    ///
    /// 1. `size = max(1, size_of(&value))`, computed BEFORE taking the lock (the callback never runs under the lock).
    /// 2. Under the lock: remove any existing entry for `key` (a replacement supersedes the old value even when the
    ///    new one is too large to cache).
    /// 3. If `size > capacity`: stop; the value is NOT stored (the old entry is already gone).
    /// 4. While `current_size + size > capacity` and an entry exists: remove the least recently used entry.
    ///    (Use `saturating_add` for the sum so `capacity` near `i64::MAX` cannot overflow.)
    /// 5. Insert as the most recently used and add `size`.
    pub fn set(&self, key: K, value: V) {
        todo!()
    }

    /// `Remove`: true when an entry existed and was removed.
    pub fn remove(&self, key: &K) -> bool {
        todo!()
    }

    /// `RemoveWhere`: removes every entry whose key satisfies `predicate`; returns how many were removed. The
    /// predicate is called once per key while the lock is held, so it must not call back into this cache
    /// (a `std::sync::Mutex` is not re-entrant; the C# `lock` was).
    pub fn remove_where(&self, predicate: impl Fn(&K) -> bool) -> usize {
        todo!()
    }

    /// `Clear`: drops every entry and resets the size to 0.
    pub fn clear(&self) {
        todo!()
    }
}
