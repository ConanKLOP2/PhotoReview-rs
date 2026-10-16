//! Port of `tests/PhotoReview.Imaging.Tests/PreloadLateNavigationTests.cs` (79 lines) and
//! `PreloadLifetimeTrackingTests.cs` (46 lines) (C# baseline d92d6aae), Q-R26 / RV-I13: a navigation that lands during an
//! all-cached pass is served by the same scheduler, and many navigations keep the lifetime bookkeeping bounded.
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.
//!
//! Test notes: the C# re-entrant `PreloadAroundAsync` from `OnCheck` runs inside the target's cache lookup; the scheduler
//! reference is held through a `Weak` so the target does not keep the scheduler alive.

mod common;

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use common::preload_fakes::*;
use pr_core::catalog::types::CatalogEntry;
use pr_core::sync::CancelToken;
use pr_imaging::cache_key::{ImageCacheKey, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::preload::scheduler::PreloadScheduler;
use pr_imaging::preload::target::{KeyError, PreloadTarget};
use pr_imaging::preload_order::{FORWARD_LOOKAHEAD, PreloadWindow};
use pr_test_support::FakeFileSystem;

const IMAGE_COUNT: usize = 200;
const OLD_WINDOW_LAST: usize = FORWARD_LOOKAHEAD as usize;
const NEW_CENTER: i32 = 150;

/// The re-entrant cache-lookup hook of [`LateNavTarget`].
type OnCheckHook = Arc<dyn Fn(usize) + Send + Sync>;

/// C# `AllCachedTarget` of `PreloadLateNavigationTests`: everything is cached except index 1 (which becomes cached only
/// after its one preload). `on_check` is called for every cache lookup, re-entrantly from the scheduler's own pass.
struct LateNavTarget {
    fs: Arc<FakeFileSystem>,
    entries: Arc<[CatalogEntry]>,
    index_by_path: Vec<(String, usize)>,
    entered: Arc<Gate>,
    release: Arc<Gate>,
    warm: AtomicBool,
    fired: AtomicUsize,
    checked: Mutex<BTreeSet<usize>>,
    on_check: Mutex<Option<OnCheckHook>>,
}

impl LateNavTarget {
    fn new() -> Arc<Self> {
        let fs = fake_fs();
        let entries = entries(&fs, IMAGE_COUNT, "late-nav");
        let index_by_path = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.path().to_ascii_uppercase(), i))
            .collect();
        Arc::new(Self {
            fs,
            entries,
            index_by_path,
            entered: Gate::new(),
            release: Gate::new(),
            warm: AtomicBool::new(false),
            fired: AtomicUsize::new(0),
            checked: Mutex::new(BTreeSet::new()),
            on_check: Mutex::new(None),
        })
    }

    fn index_of_key(&self, key_path: &str) -> usize {
        self.index_by_path
            .iter()
            .find(|(p, _)| p == key_path)
            .map(|(_, i)| *i)
            .expect("fixture key")
    }
}

impl PreloadTarget for LateNavTarget {
    fn try_get_cached_preview_for_path(&self, _path: &str) -> bool {
        true
    }

    fn try_get_cached_preview(&self, key: &ImageCacheKey) -> bool {
        let index = self.index_of_key(key.path());
        lock(&self.checked).insert(index);
        let hook = lock(&self.on_check).clone();
        if let Some(hook) = hook {
            hook(index);
        }
        index != 1 || self.warm.load(Ordering::SeqCst)
    }

    fn preload(&self, _path: &str, _cancel: &CancelToken) -> Result<(), DecodeError> {
        self.entered.open();
        self.release.wait(None);
        self.warm.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn get_current_cache_key_for_path(&self, path: &str) -> Result<ImageCacheKey, KeyError> {
        Ok(ImageCacheKey::create(
            &*self.fs,
            path,
            false,
            DecodeBox::new(100, 100),
            KeyOptions::default(),
        )?)
    }

    fn get_current_cache_key(&self, entry: &CatalogEntry) -> Result<ImageCacheKey, KeyError> {
        Ok(ImageCacheKey::create_for_entry(
            &*self.fs,
            entry,
            false,
            DecodeBox::new(100, 100),
            KeyOptions::default(),
        )?)
    }

    fn cache_count(&self) -> i32 {
        0
    }

    fn cache_bytes(&self) -> i64 {
        0
    }
}

#[test]
fn navigation_during_all_cached_pass_is_served_by_the_same_scheduler() {
    // C#: PreloadLateNavigationTests.NavigationDuringAllCachedPass_IsServedByTheSameScheduler
    let target = LateNavTarget::new();
    let scheduler: Arc<OnceLock<PreloadScheduler>> = Arc::new(OnceLock::new());
    let weak: Weak<OnceLock<PreloadScheduler>> = Arc::downgrade(&scheduler);
    let hook_target = Arc::clone(&target);
    *lock(&target.on_check) = Some(Arc::new(move |index: usize| {
        // Re-entrant on the scheduler's own pass: the version is bumped after the pass read it.
        if index == OLD_WINDOW_LAST && hook_target.fired.swap(1, Ordering::SeqCst) == 0 {
            if let Some(slot) = weak.upgrade() {
                let _ = slot.get().expect("scheduler set").preload_around(NEW_CENTER);
            }
        }
    }));
    let built = create(
        &target,
        &target.entries,
        Setup::new(1, PreloadWindow::DEFAULT).full_folder_threshold(1),
    );
    assert!(scheduler.set(built).is_ok());
    let built = scheduler.get().expect("scheduler set");

    // One worker: pass 1 queues the single uncached image and waits for it; the pass after it finishes then examines
    // the rest of the (cached) window on a pool thread.
    let lifetime = built.preload_around(0);
    assert!(target.entered.wait(None));
    target.release.open();
    assert!(finished(&lifetime));

    let checked = lock(&target.checked).clone();
    assert!(
        checked.contains(&(NEW_CENTER as usize + 1)),
        "never examined; fired={} checked={checked:?}",
        target.fired.load(Ordering::SeqCst)
    );
}

#[test]
fn many_navigations_keep_tracking_bounded() {
    // C#: PreloadLifetimeTrackingTests.ManyNavigations_KeepTrackingBounded
    let fs = fake_fs();
    let entries = entries(&fs, 8, "lifetimes");
    let target = FakeTarget::new(&fs);
    for entry in entries.iter() {
        target.mark_cached(entry.path());
    }
    let scheduler = create(
        &target,
        &entries,
        Setup::new(2, PreloadWindow::DEFAULT).default_threshold(),
    );
    for i in 0..1_000usize {
        assert!(finished(&scheduler.preload_around((i % entries.len()) as i32)));
        if i % 2 == 0 {
            scheduler.cancel();
        }
    }
    let (lifetimes, runs) = scheduler.tracked_lifetime_counts();
    assert!(
        lifetimes <= 8,
        "{lifetimes} cancellation lifetimes still tracked after 1000 navigations"
    );
    assert!(runs <= 8, "{runs} scheduler runs still tracked after 1000 navigations");
}
