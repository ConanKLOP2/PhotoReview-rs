//! Port tests for `pr_core::caching::lru` (C# `PhotoReview.Core/Caching/BoundedLruCache.cs`, baseline e441d67a),
//! plus the golden trace `tests/golden/lru-trace.json` and Rust-only tests for what the C# suite leaves untested.

use std::thread;

use pr_core::caching::lru::{BoundedLruCache, InvalidCapacity};
use pr_test_support::golden::{self, bool_of, i64_of};
use serde_json::Value;

/// Size of a string value in bytes (the golden oracle uses `value.Length`; its inputs are ASCII).
// `V` is `String` in every test, so `size_of` must take `&String` (a `&str` parameter would not fit `Fn(&V)`).
#[allow(clippy::ptr_arg)]
fn byte_len(value: &String) -> i64 {
    i64::try_from(value.len()).unwrap_or(i64::MAX)
}

fn key(text: &str) -> String {
    text.to_owned()
}

/// C# `Seeded()`: capacity 4, "a" -> "aa" (2 bytes), "b" -> "b" (1 byte).
fn seeded() -> BoundedLruCache<String, String> {
    let cache = BoundedLruCache::new(4, byte_len).expect("capacity 4 is valid");
    cache.set(key("a"), key("aa"));
    cache.set(key("b"), key("b"));
    cache
}

fn assert_send_sync<T: Send + Sync>() {}

// C#: BoundedLruCacheTests.LruRetainsRecentlyAccessedEntry
#[test]
fn lru_retains_recently_accessed_entry() {
    assert!(seeded().try_get(&key("a")).is_some());
}

// C#: BoundedLruCacheTests.LruEvictsLeastRecentlyUsedEntry
#[test]
fn lru_evicts_least_recently_used_entry() {
    let cache = seeded();
    let _ = cache.try_get(&key("a"));
    cache.set(key("c"), key("cc"));
    assert!(cache.try_get(&key("b")).is_none());
}

// C#: BoundedLruCacheTests.LruEnforcesByteCapacity
#[test]
fn lru_enforces_byte_capacity() {
    let cache = seeded();
    let _ = cache.try_get(&key("a"));
    cache.set(key("c"), key("cc"));
    assert!(cache.current_size() <= 4);
}

// C#: BoundedLruCacheTests.RemoveWhereNullPredicateThrows
// No Rust twin: a Rust closure cannot be null, so the C# ArgumentNullException case does not exist here.

// C#: BoundedLruCacheTests.OversizeReplacementDropsStaleEntry
#[test]
fn oversize_replacement_drops_stale_entry() {
    let cache = seeded(); // capacity 4: "a"->"aa", "b"->"b"
    cache.set(key("a"), key("too-big-for-capacity"));
    assert!(cache.try_get(&key("a")).is_none());
    assert_eq!(cache.count(), 1);
    assert_eq!(cache.current_size(), 1);
    assert!(cache.try_get(&key("b")).is_some());
}

// C#: BoundedLruCacheTests (golden trace: ops set/get/remove/remove_prefix/clear, count and size after every op)
#[test]
fn lru_trace_golden_matches_csharp() {
    let g = golden::load("lru-trace");
    for (index, case) in g.cases.iter().enumerate() {
        let capacity = i64_of(case, "capacity");
        if case.get("ctor_error").and_then(Value::as_bool) == Some(true) {
            assert_eq!(
                BoundedLruCache::<String, String>::new(capacity, byte_len).err(),
                Some(InvalidCapacity(capacity)),
                "case {index}: capacity {capacity} must be rejected"
            );
            continue;
        }
        let cache =
            BoundedLruCache::<String, String>::new(capacity, byte_len).unwrap_or_else(|e| panic!("case {index}: {e}"));
        let steps = case["steps"]
            .as_array()
            .unwrap_or_else(|| panic!("case {index}: missing steps"));
        for (si, step) in steps.iter().enumerate() {
            let op = step["op"]
                .as_array()
                .unwrap_or_else(|| panic!("case {index} step {si}: missing op"));
            let name = op[0].as_str().unwrap_or_default();
            let arg = |i: usize| -> String {
                op.get(i)
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("case {index} step {si}: missing argument {i}"))
                    .to_owned()
            };
            let at = format!("case {index} step {si} ({name})");
            match name {
                "set" => cache.set(arg(1), arg(2)),
                "get" => {
                    let got = cache.try_get(&arg(1));
                    assert_eq!(got.is_some(), bool_of(step, "hit"), "{at}: hit");
                    let expected = match &step["value"] {
                        Value::Null => None,
                        Value::String(s) => Some(s.clone()),
                        other => panic!("{at}: unexpected value {other}"),
                    };
                    assert_eq!(got, expected, "{at}: value");
                }
                "remove" => assert_eq!(cache.remove(&arg(1)), bool_of(step, "removed"), "{at}: removed"),
                "remove_prefix" => {
                    let prefix = arg(1);
                    let removed = cache.remove_where(|k| k.starts_with(prefix.as_str()));
                    assert_eq!(
                        i64::try_from(removed).unwrap_or(i64::MAX),
                        i64_of(step, "removed"),
                        "{at}: removed"
                    );
                }
                "clear" => cache.clear(),
                other => panic!("{at}: unknown op '{other}'"),
            }
            assert_eq!(
                i64::try_from(cache.count()).unwrap_or(i64::MAX),
                i64_of(step, "count"),
                "{at}: count"
            );
            assert_eq!(cache.current_size(), i64_of(step, "size"), "{at}: size");
        }
    }
}

// Rust-only: the C# suite never calls Remove.
#[test]
fn remove_present_key_returns_true_and_subtracts_size() {
    let cache = seeded(); // size 3
    assert!(cache.remove(&key("a")));
    assert_eq!(cache.count(), 1);
    assert_eq!(cache.current_size(), 1);
    assert!(cache.try_get(&key("a")).is_none());
}

// Rust-only: Remove on a missing key changes nothing.
#[test]
fn remove_missing_key_returns_false_and_changes_nothing() {
    let cache = seeded();
    assert!(!cache.remove(&key("zz")));
    assert_eq!(cache.count(), 2);
    assert_eq!(cache.current_size(), 3);
}

// Rust-only: RemoveWhere removes every match and reports the count.
#[test]
fn remove_where_removes_matching_keys_and_counts() {
    let cache = BoundedLruCache::new(10, byte_len).expect("capacity 10 is valid");
    cache.set(key("img1"), key("x"));
    cache.set(key("img2"), key("xx"));
    cache.set(key("doc"), key("xxx"));
    assert_eq!(cache.remove_where(|k| k.starts_with("img")), 2);
    assert_eq!(cache.count(), 1);
    assert_eq!(cache.current_size(), 3);
    assert!(cache.try_get(&key("doc")).is_some());
}

// Rust-only: RemoveWhere with no match returns 0 and changes nothing.
#[test]
fn remove_where_no_match_returns_zero() {
    let cache = seeded();
    assert_eq!(cache.remove_where(|k| k.starts_with("zzz")), 0);
    assert_eq!(cache.count(), 2);
    assert_eq!(cache.current_size(), 3);
}

// Rust-only: Clear empties the cache and resets the size to 0.
#[test]
fn clear_empties_cache_and_resets_size() {
    let cache = seeded();
    cache.clear();
    assert_eq!(cache.count(), 0);
    assert_eq!(cache.current_size(), 0);
    assert!(cache.try_get(&key("a")).is_none());
    cache.set(key("a"), key("aa"));
    assert_eq!(cache.current_size(), 2);
}

// Rust-only: a value whose size_of is 0 (or negative) still costs 1 byte.
#[test]
fn zero_size_value_costs_one_byte() {
    let cache = BoundedLruCache::new(2, |_: &String| 0).expect("capacity 2 is valid");
    cache.set(key("a"), key(""));
    assert_eq!(cache.current_size(), 1);
    cache.set(key("b"), key(""));
    assert_eq!(cache.current_size(), 2);
    cache.set(key("c"), key(""));
    assert_eq!(cache.count(), 2);
    assert_eq!(cache.current_size(), 2);

    let negative = BoundedLruCache::new(5, |_: &String| -7).expect("capacity 5 is valid");
    negative.set(key("a"), key("x"));
    assert_eq!(negative.current_size(), 1);
}

// Rust-only: capacity <= 0 is rejected (C# ArgumentOutOfRangeException.ThrowIfNegativeOrZero).
#[test]
fn new_rejects_zero_and_negative_capacity() {
    assert_eq!(
        BoundedLruCache::<String, String>::new(0, byte_len).err(),
        Some(InvalidCapacity(0))
    );
    assert_eq!(
        BoundedLruCache::<String, String>::new(-1, byte_len).err(),
        Some(InvalidCapacity(-1))
    );
    assert_eq!(
        BoundedLruCache::<String, String>::new(i64::MIN, byte_len).err(),
        Some(InvalidCapacity(i64::MIN))
    );
}

// Rust-only: i64::MAX is a valid capacity and sums near it do not overflow.
#[test]
fn huge_capacity_accepted_without_overflow() {
    let cache = BoundedLruCache::new(i64::MAX, |_: &String| i64::MAX - 1).expect("i64::MAX is valid");
    cache.set(key("a"), key("x"));
    cache.set(key("b"), key("y"));
    assert_eq!(cache.count(), 1);
    assert_eq!(cache.current_size(), i64::MAX - 1);
    assert!(cache.try_get(&key("b")).is_some());
}

// Rust-only: a miss changes neither the entries nor their recency.
#[test]
fn try_get_miss_does_not_change_recency() {
    let cache = seeded(); // "a" is least recently used
    assert!(cache.try_get(&key("zz")).is_none());
    cache.set(key("c"), key("cc")); // needs 2 bytes: evicts "a" (LRU), not "b"
    assert!(cache.try_get(&key("a")).is_none());
    assert!(cache.try_get(&key("b")).is_some());
}

// Rust-only: eviction follows recency, not hash-map iteration order.
#[test]
fn eviction_follows_recency_order() {
    let cache = BoundedLruCache::new(3, byte_len).expect("capacity 3 is valid");
    cache.set(key("k0"), key("x"));
    cache.set(key("k1"), key("x"));
    cache.set(key("k2"), key("x"));
    assert!(cache.try_get(&key("k0")).is_some()); // order now k1, k2, k0
    cache.set(key("k3"), key("x")); // evicts k1
    assert!(cache.try_get(&key("k1")).is_none());
    assert!(cache.try_get(&key("k0")).is_some());
    assert!(cache.try_get(&key("k2")).is_some());
    assert!(cache.try_get(&key("k3")).is_some());
}

// Rust-only: replacing a key with a smaller value updates the size and count.
#[test]
fn set_replacement_updates_size_and_count() {
    let cache = seeded(); // "a" is 2 bytes
    cache.set(key("a"), key("a")); // now 1 byte
    assert_eq!(cache.count(), 2);
    assert_eq!(cache.current_size(), 2);
    assert_eq!(cache.try_get(&key("a")), Some(key("a")));
}

// Rust-only: a replacement makes the key the most recently used.
#[test]
fn set_replacement_makes_entry_most_recent() {
    let cache = BoundedLruCache::new(2, byte_len).expect("capacity 2 is valid");
    cache.set(key("a"), key("a"));
    cache.set(key("b"), key("b"));
    cache.set(key("a"), key("a")); // order now b, a
    cache.set(key("c"), key("c")); // evicts b
    assert!(cache.try_get(&key("a")).is_some());
    assert!(cache.try_get(&key("b")).is_none());
}

// Rust-only: the cache is Send + Sync for Send values.
#[test]
fn cache_is_send_and_sync() {
    assert_send_sync::<BoundedLruCache<String, String>>();
}

// Rust-only: four threads doing set/try_get concurrently neither deadlock nor break the size invariant.
#[test]
fn concurrent_set_and_try_get_keep_size_within_capacity() {
    let capacity = 16_i64;
    let cache = BoundedLruCache::new(capacity, byte_len).expect("capacity 16 is valid");
    thread::scope(|scope| {
        for t in 0..4_usize {
            let cache = &cache;
            scope.spawn(move || {
                for i in 0..2_000_usize {
                    let k = key(&format!("k{}", (i * 7 + t) % 32));
                    if i % 2 == 0 {
                        cache.set(k, "v".repeat(1 + (i + t) % 4));
                    } else {
                        let _ = cache.try_get(&k);
                    }
                }
            });
        }
    });
    assert!(cache.current_size() <= capacity);
    assert!(cache.count() <= 32);
    assert!(i64::try_from(cache.count()).unwrap_or(i64::MAX) <= cache.current_size());
}
