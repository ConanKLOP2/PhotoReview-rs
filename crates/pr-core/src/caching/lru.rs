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

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::hash::Hash;
use std::sync::{Mutex, MutexGuard, PoisonError};

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

/// Removes `key` from `state` (both maps and the size sum). Returns true when an entry existed.
fn remove_entry<K: Eq + Hash, V>(state: &mut LruState<K, V>, key: &K) -> bool {
    match state.items.remove(key) {
        Some(slot) => {
            state.order.remove(&slot.stamp);
            state.size = state.size.saturating_sub(slot.size);
            true
        }
        None => false,
    }
}

impl<K: Eq + Hash + Clone, V: Clone> BoundedLruCache<K, V> {
    /// C# constructor. `capacity` is in the same unit as `size_of` (bytes). `capacity <= 0` is an error
    /// ([`InvalidCapacity`]); `i64::MAX` is allowed.
    pub fn new(capacity: i64, size_of: impl Fn(&V) -> i64 + Send + Sync + 'static) -> Result<Self, InvalidCapacity> {
        if capacity <= 0 {
            return Err(InvalidCapacity(capacity));
        }
        Ok(Self {
            capacity,
            size_of: Box::new(size_of),
            state: Mutex::new(LruState {
                items: HashMap::new(),
                order: BTreeMap::new(),
                next_stamp: 0,
                size: 0,
            }),
        })
    }

    /// Takes the lock; a poisoned lock is recovered (the C# `lock` simply continues).
    fn lock(&self) -> MutexGuard<'_, LruState<K, V>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Number of entries.
    pub fn count(&self) -> usize {
        self.lock().items.len()
    }

    /// Sum of the entry sizes (always `<= capacity`).
    pub fn current_size(&self) -> i64 {
        self.lock().size
    }

    /// `TryGet`: on a hit returns a clone of the value and makes the entry the most recently used; on a miss
    /// returns `None` and changes nothing.
    pub fn try_get(&self, key: &K) -> Option<V> {
        let mut guard = self.lock();
        let LruState {
            items,
            order,
            next_stamp,
            ..
        } = &mut *guard;
        let slot = items.get_mut(key)?;
        *next_stamp = next_stamp.wrapping_add(1);
        let stamp = *next_stamp;
        order.remove(&slot.stamp);
        order.insert(stamp, key.clone());
        slot.stamp = stamp;
        Some(slot.value.clone())
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
        let size = (self.size_of)(&value).max(1);
        let mut guard = self.lock();
        let state = &mut *guard;
        remove_entry(state, &key);
        if size > self.capacity {
            return;
        }
        // Equivalent to `state.size + size > capacity` without the sum: `0 <= state.size <= capacity` holds here,
        // so the subtraction cannot overflow even when `capacity` is near `i64::MAX`.
        while size > self.capacity - state.size {
            let Some((_, lru_key)) = state.order.pop_first() else {
                break;
            };
            if let Some(slot) = state.items.remove(&lru_key) {
                state.size = state.size.saturating_sub(slot.size);
            }
        }
        state.next_stamp = state.next_stamp.wrapping_add(1);
        let stamp = state.next_stamp;
        state.order.insert(stamp, key.clone());
        state.items.insert(key, Slot { value, size, stamp });
        state.size = state.size.saturating_add(size);
    }

    /// `Remove`: true when an entry existed and was removed.
    pub fn remove(&self, key: &K) -> bool {
        let mut guard = self.lock();
        remove_entry(&mut guard, key)
    }

    /// `RemoveWhere`: removes every entry whose key satisfies `predicate`; returns how many were removed. The
    /// predicate is called once per key while the lock is held, so it must not call back into this cache
    /// (a `std::sync::Mutex` is not re-entrant; the C# `lock` was).
    pub fn remove_where(&self, predicate: impl Fn(&K) -> bool) -> usize {
        let mut guard = self.lock();
        let state = &mut *guard;
        let doomed: Vec<K> = state.items.keys().filter(|key| predicate(*key)).cloned().collect();
        for key in &doomed {
            remove_entry(state, key);
        }
        doomed.len()
    }

    /// `Clear`: drops every entry and resets the size to 0.
    pub fn clear(&self) {
        let mut guard = self.lock();
        guard.items.clear();
        guard.order.clear();
        guard.size = 0;
    }
}
