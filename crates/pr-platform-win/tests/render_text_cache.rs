#![cfg(windows)]
//! Pure tests of `render::text::layout_cache` and of the pure helpers of `render::text::renderer` (unit W8-12i, test kind F).
//! C# source: the cache assertions of `DWriteTextRendererTests` (baseline `d92d6aae`); no test file targets `TextLayoutCache`
//! directly, so the C# names are noted per test. Hand-written expectations, no DirectWrite, no sleeps.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use pr_platform_win::render::text::layout_cache::{
    AcquireError, InvalidCapacity, NewLayoutEntry, TextLayoutCache, TextLayoutEntry, TextLayoutKey, TextTrimming,
};
use pr_platform_win::render::text::renderer::{
    effective_font_family, is_valid_font_size, normalize_width, resolve_locale,
};

/// Fake native payload: counts its own drops.
struct DropCounter(Rc<Cell<i32>>);

impl Drop for DropCounter {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

type Cache = TextLayoutCache<DropCounter>;
type Entry = Rc<TextLayoutEntry<DropCounter>>;

fn key(text: &str) -> TextLayoutKey {
    TextLayoutKey::new(text, "Segoe UI", 12.0, false, 100.0, TextTrimming::CharacterEllipsis)
}

fn parts(counter: &Rc<Cell<i32>>) -> NewLayoutEntry<DropCounter> {
    NewLayoutEntry {
        native: DropCounter(Rc::clone(counter)),
        width: 10.0,
        height: 16.0,
        line_count: 1,
        is_trimmed: false,
    }
}

fn acquire(cache: &Cache, key: TextLayoutKey, counter: &Rc<Cell<i32>>) -> Entry {
    cache
        .acquire(key, |_| Ok::<_, ()>(parts(counter)))
        .expect("acquire succeeds")
}

fn new_cache(capacity: i32) -> Cache {
    Cache::new(capacity).expect("valid capacity")
}

// C#: DWriteTextRendererTests.SameKey_ReturnsSharedNativeLayout_DifferentKeysMiss
#[test]
fn same_key_returns_the_shared_entry_and_different_keys_miss() {
    let drops = Rc::new(Cell::new(0));
    let cache = new_cache(256);
    let first = acquire(&cache, key("abc"), &drops);
    let second = acquire(&cache, key("abc"), &drops);
    assert_eq!(cache.misses(), 1);
    assert_eq!(cache.hits(), 1);
    assert!(Rc::ptr_eq(&first, &second));
    assert_eq!(first.ref_count(), 2);

    let wider = acquire(
        &cache,
        TextLayoutKey::new("abc", "Segoe UI", 12.0, false, 101.0, TextTrimming::CharacterEllipsis),
        &drops,
    );
    let _none = acquire(
        &cache,
        TextLayoutKey::new("abc", "Segoe UI", 12.0, false, 100.0, TextTrimming::None),
        &drops,
    );
    let _bold = acquire(
        &cache,
        TextLayoutKey::new("abc", "Segoe UI", 12.0, true, 100.0, TextTrimming::CharacterEllipsis),
        &drops,
    );
    let _text = acquire(&cache, key("abd"), &drops);
    let _size = acquire(
        &cache,
        TextLayoutKey::new("abc", "Segoe UI", 13.0, false, 100.0, TextTrimming::CharacterEllipsis),
        &drops,
    );
    let _family = acquire(
        &cache,
        TextLayoutKey::new("abc", "Consolas", 12.0, false, 100.0, TextTrimming::CharacterEllipsis),
        &drops,
    );
    assert_eq!(cache.misses(), 7);
    assert_eq!(cache.count(), 7);
    assert!(!Rc::ptr_eq(&first, &wider));
}

// C#: DWriteTextRendererTests.Cache_EvictsLeastRecentlyUsed_AndKeepsLeasedLayoutAliveUntilDisposed
#[test]
fn eviction_is_least_recently_used_and_a_leased_entry_stays_alive_until_released() {
    let drops = Rc::new(Cell::new(0));
    let cache = new_cache(2);
    let first = acquire(&cache, key("first"), &drops);
    let second = acquire(&cache, key("second"), &drops);
    let third = acquire(&cache, key("third"), &drops);
    // "first" was the least recently used one and is still leased.
    assert_eq!(cache.count(), 2);
    assert_eq!(cache.pending_release(), 1);
    assert!(first.is_evicted());
    assert_eq!(drops.get(), 0);
    assert!(!first.is_released());
    assert!(first.with_native(|native| native.is_some()));

    cache.release(&first);
    assert_eq!(cache.pending_release(), 0);
    assert_eq!(drops.get(), 1);
    assert!(first.is_released());
    assert_eq!(first.ref_count(), 0);
    cache.release(&first);
    assert_eq!(first.ref_count(), 0);
    assert_eq!(drops.get(), 1);

    // "second" is a hit (and becomes the most recent), "fourth" then evicts "third".
    let again = acquire(&cache, key("second"), &drops);
    assert_eq!(cache.hits(), 1);
    assert!(Rc::ptr_eq(&second, &again));
    let _fourth = acquire(&cache, key("fourth"), &drops);
    assert!(third.is_evicted());
    assert_eq!(cache.pending_release(), 1);
    cache.release(&third);
    assert_eq!(cache.pending_release(), 0);
    assert_eq!(drops.get(), 2);
}

// C#: DWriteTextRendererTests.LeakCheck_ManyLayoutsAndEvictions_ReleaseEveryNativeLayout
#[test]
fn leak_check_many_acquires_and_evictions_release_every_payload() {
    let drops = Rc::new(Cell::new(0));
    let cache = new_cache(8);
    for i in 0..400 {
        let k = TextLayoutKey::new(
            &format!("Anh so {i}"),
            "Segoe UI",
            12.0,
            false,
            50.0 + (i % 7) as f32,
            TextTrimming::CharacterEllipsis,
        );
        let entry = acquire(&cache, k, &drops);
        cache.release(&entry);
    }
    assert_eq!(cache.pending_release(), 0);
    assert_eq!(cache.count(), 8);
    assert_eq!(cache.misses(), 400);
    assert_eq!(i64::from(drops.get()), cache.misses() - 8);
}

// C#: DWriteTextRendererTests.RendererDispose_KeepsLeasedLayoutDrawable_ThenReleasesWhenLeaseDisposed (cache part)
#[test]
fn dispose_keeps_leased_entries_alive_then_releases_them() {
    let drops = Rc::new(Cell::new(0));
    let cache = new_cache(4);
    let entry = acquire(&cache, key("abc"), &drops);
    cache.dispose();
    cache.dispose();
    assert_eq!(cache.count(), 0);
    assert_eq!(cache.pending_release(), 1);
    assert_eq!(drops.get(), 0);
    assert!(entry.with_native(|native| native.is_some()));
    let refused = cache.acquire(key("abc"), |_| Ok::<_, ()>(parts(&drops)));
    assert_eq!(refused.err(), Some(AcquireError::Disposed));
    cache.release(&entry);
    assert_eq!(drops.get(), 1);
    assert_eq!(cache.pending_release(), 0);
    assert!(entry.is_released());
}

#[test]
fn a_hit_makes_the_entry_the_most_recent() {
    let drops = Rc::new(Cell::new(0));
    let cache = new_cache(2);
    let a = acquire(&cache, key("a"), &drops);
    let b = acquire(&cache, key("b"), &drops);
    cache.release(&a);
    cache.release(&b);
    let a2 = acquire(&cache, key("a"), &drops);
    cache.release(&a2);
    let c = acquire(&cache, key("c"), &drops);
    cache.release(&c);
    // "b" was the least recent and is gone, "a" is kept.
    assert!(b.is_evicted());
    assert!(!a.is_evicted());
    assert_eq!(drops.get(), 1);
    let hits_before = cache.hits();
    let a3 = acquire(&cache, key("a"), &drops);
    assert_eq!(cache.hits(), hits_before + 1);
    assert!(Rc::ptr_eq(&a, &a3));
}

// T3
#[test]
fn a_new_entry_is_never_the_victim_with_capacity_one() {
    let drops = Rc::new(Cell::new(0));
    let cache = new_cache(1);
    let a = acquire(&cache, key("a"), &drops);
    cache.release(&a);
    let b = acquire(&cache, key("b"), &drops);
    cache.release(&b);
    assert_eq!(drops.get(), 1);
    assert!(a.is_released());
    assert!(!b.is_evicted());
    let b2 = acquire(&cache, key("b"), &drops);
    assert_eq!(cache.hits(), 1);
    assert!(Rc::ptr_eq(&b, &b2));
    assert_eq!(cache.count(), 1);
}

#[test]
fn new_rejects_capacity_below_one() {
    assert_eq!(Cache::new(0).err(), Some(InvalidCapacity(0)));
    assert_eq!(Cache::new(-1).err(), Some(InvalidCapacity(-1)));
    assert!(Cache::new(1).is_ok());
}

// T2
#[test]
fn a_failing_build_counts_a_miss_and_caches_nothing() {
    let drops = Rc::new(Cell::new(0));
    let cache = new_cache(4);
    let failed = cache.acquire(key("abc"), |_| Err::<NewLayoutEntry<DropCounter>, _>("boom"));
    assert_eq!(failed.err(), Some(AcquireError::Build("boom")));
    assert_eq!(cache.misses(), 1);
    assert_eq!(cache.count(), 0);
    let _ok = acquire(&cache, key("abc"), &drops);
    assert_eq!(cache.misses(), 2);
    assert_eq!(cache.hits(), 0);
}

#[test]
fn build_may_read_the_cache_without_deadlock_or_panic() {
    let drops = Rc::new(Cell::new(0));
    let cache = new_cache(4);
    let _first = acquire(&cache, key("one"), &drops);
    let seen = Cell::new((0usize, 0i64, 0i64));
    let _second = cache
        .acquire(key("two"), |_| {
            seen.set((cache.count(), cache.hits(), cache.misses()));
            Ok::<_, ()>(parts(&drops))
        })
        .expect("acquire succeeds");
    // The miss of "two" is counted before `build` runs; "one" is cached.
    assert_eq!(seen.get(), (1, 0, 2));
}

#[test]
fn clear_makes_leased_entries_pending_and_drops_unleased_ones() {
    let drops = Rc::new(Cell::new(0));
    let cache = new_cache(4);
    let leased = acquire(&cache, key("leased"), &drops);
    let released = acquire(&cache, key("released"), &drops);
    cache.release(&released);
    cache.clear();
    assert_eq!(cache.count(), 0);
    assert_eq!(cache.pending_release(), 1);
    assert_eq!(drops.get(), 1);
    assert!(released.is_released());
    assert!(!leased.is_released());
    // `clear` is not `dispose`: the cache still works.
    let _again = acquire(&cache, key("leased"), &drops);
    assert_eq!(cache.count(), 1);
    cache.release(&leased);
    assert_eq!(cache.pending_release(), 0);
    assert_eq!(drops.get(), 2);
}

// T8
#[test]
fn release_is_idempotent_below_zero() {
    let drops = Rc::new(Cell::new(0));
    let cache = new_cache(1);
    let a = acquire(&cache, key("a"), &drops);
    cache.release(&a);
    cache.release(&a);
    cache.release(&a);
    assert_eq!(a.ref_count(), 0);
    assert_eq!(cache.pending_release(), 0);
    assert_eq!(drops.get(), 0);
    // Evict it (unleased: dropped at once), then release again: nothing changes.
    let b = acquire(&cache, key("b"), &drops);
    assert_eq!(drops.get(), 1);
    cache.release(&a);
    cache.release(&b);
    cache.release(&b);
    assert_eq!(cache.pending_release(), 0);
    assert_eq!(drops.get(), 1);
}

// T1
#[test]
fn layout_key_equality_covers_every_field() {
    let base = key("abc");
    assert_eq!(base, key("abc"));
    let variants = [
        key("abd"),
        key("ABC"),
        TextLayoutKey::new("abc", "Consolas", 12.0, false, 100.0, TextTrimming::CharacterEllipsis),
        TextLayoutKey::new("abc", "segoe ui", 12.0, false, 100.0, TextTrimming::CharacterEllipsis),
        TextLayoutKey::new("abc", "Segoe UI", 12.5, false, 100.0, TextTrimming::CharacterEllipsis),
        TextLayoutKey::new("abc", "Segoe UI", 12.0, true, 100.0, TextTrimming::CharacterEllipsis),
        TextLayoutKey::new("abc", "Segoe UI", 12.0, false, 100.5, TextTrimming::CharacterEllipsis),
        TextLayoutKey::new("abc", "Segoe UI", 12.0, false, 100.0, TextTrimming::None),
    ];
    for variant in &variants {
        assert_ne!(&base, variant, "{variant:?}");
    }
    let mut set = HashSet::new();
    assert!(set.insert(key("abc")));
    assert!(!set.insert(key("abc")));
    assert_eq!(set.len(), 1);
    assert_eq!(base.text(), "abc");
    assert_eq!(base.font_family(), "Segoe UI");
    assert_eq!(base.font_size_dip(), 12.0);
    assert!(!base.bold());
    assert_eq!(base.max_width(), 100.0);
    assert_eq!(base.trimming(), TextTrimming::CharacterEllipsis);
}

// C#: DWriteTextRendererTests.NormalizeWidth_MapsUnboundedAndNegative
#[test]
fn normalize_width_maps_unbounded_and_negative() {
    assert_eq!(normalize_width(f64::INFINITY), 1_000_000.0);
    assert_eq!(normalize_width(f64::NAN), 1_000_000.0);
    assert_eq!(normalize_width(5_000_000.0), 1_000_000.0);
    assert_eq!(normalize_width(-3.0), 0.0);
    assert_eq!(normalize_width(0.0), 0.0);
    assert_eq!(normalize_width(123.5), 123.5);
    // Added rows.
    assert_eq!(normalize_width(1_000_000.0), 1_000_000.0);
    assert_eq!(normalize_width(999_999.5), 999_999.5);
    assert_eq!(normalize_width(-0.0).to_bits(), 0);
    assert_eq!(normalize_width(f64::NEG_INFINITY).to_bits(), 0);
    assert_eq!(normalize_width(f64::MAX), 1_000_000.0);
    assert_eq!(normalize_width(1e-300).to_bits(), 0);
}

// C#: DWriteTextRendererTests.CreateLayout_ValidatesArguments (font size rows)
#[test]
fn is_valid_font_size_rejects_nan_zero_negative_and_infinity() {
    for invalid in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(!is_valid_font_size(invalid), "{invalid}");
    }
    for valid in [0.001, 12.0, 1e300] {
        assert!(is_valid_font_size(valid), "{valid}");
    }
}

#[test]
fn effective_font_family_replaces_blank_names_only() {
    for blank in ["", " ", "\t\r\n", "\u{00A0}"] {
        assert_eq!(effective_font_family(blank), "Segoe UI", "{blank:?}");
    }
    assert_eq!(effective_font_family("Consolas"), "Consolas");
    assert_eq!(effective_font_family(" Consolas "), " Consolas ");
}

#[test]
fn resolve_locale_prefers_the_explicit_name() {
    assert_eq!(resolve_locale(Some("vi-VN"), "en-US"), "vi-VN");
    assert_eq!(resolve_locale(None, "fr-FR"), "fr-FR");
    assert_eq!(resolve_locale(Some("  "), "de-DE"), "de-DE");
    assert_eq!(resolve_locale(Some(""), ""), "en-us");
    // T10: a blank UI culture name is not trimmed.
    assert_eq!(resolve_locale(None, " "), " ");
}
