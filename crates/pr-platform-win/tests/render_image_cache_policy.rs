#![cfg(windows)]
//! Ports of `GpuImageCacheTests.cs` (baseline `d92d6aae`, 16 of its 18 tests; the other two belong to W8-12a / W8-12g) plus the
//! trap tests of `contracts/W8-12f.md`. Test kind F: hand-written expectations, no GPU, no sleeps. The C# `FakeBackend` and
//! `ManualDispatcher` are replaced by [`FakeBackend`] and [`ManualScheduler`].

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::gfx::com::GfxError;
use pr_platform_win::render::image_cache::policy::{
    BudgetedImageCache, CACHE_OBJECT_NAME, CacheError, CachedImage, DEFAULT_BUDGET_SCREENS, DEFAULT_STRIPE_BYTES,
    GpuImageBackend, MIN_STRIPE_BYTES, MINIMUM_SCREEN_BYTES, PIXEL_BUFFER_OBJECT_NAME, StripeScheduler, StripeToken,
    default_budget_bytes,
};

// ---------------------------------------------------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Debug)]
struct FakeImage {
    id: i64,
    width: i32,
    height: i32,
    bytes: i64,
    uploaded_rows: Cell<i32>,
    released: Cell<bool>,
}

impl CachedImage for FakeImage {
    fn pixel_buffer_id(&self) -> i64 {
        self.id
    }
    fn pixel_height(&self) -> i32 {
        self.height
    }
    fn gpu_bytes(&self) -> i64 {
        self.bytes
    }
    fn uploaded_rows(&self) -> i32 {
        self.uploaded_rows.get()
    }
    fn set_uploaded_rows(&self, rows: i32) {
        self.uploaded_rows.set(rows);
    }
    fn is_complete(&self) -> bool {
        self.uploaded_rows.get() >= self.height
    }
    fn is_released(&self) -> bool {
        self.released.get()
    }
    fn set_released(&self, released: bool) {
        self.released.set(released);
    }
}

fn gfx_error() -> GfxError {
    GfxError::new("FakeBackend", -2_147_467_259)
}

type DeviceHandler = (u64, Rc<dyn Fn()>);

#[derive(Default)]
struct FakeState {
    uploads: RefCell<Vec<(i64, bool)>>,
    row_uploads: RefCell<Vec<(i32, i32)>>,
    released: RefCell<Vec<i64>>,
    images: RefCell<Vec<Rc<FakeImage>>>,
    handlers: RefCell<Vec<DeviceHandler>>,
    next_handler: Cell<u64>,
    fail_next_upload: Cell<Option<GfxError>>,
    fail_next_rows: Cell<Option<GfxError>>,
}

impl FakeState {
    fn raise_device_recreated(&self) {
        let handlers: Vec<Rc<dyn Fn()>> = self.handlers.borrow().iter().map(|(_, h)| Rc::clone(h)).collect();
        for handler in handlers {
            handler();
        }
    }
    fn has_subscribers(&self) -> bool {
        !self.handlers.borrow().is_empty()
    }
    fn uploads(&self) -> Vec<(i64, bool)> {
        self.uploads.borrow().clone()
    }
    fn row_uploads(&self) -> Vec<(i32, i32)> {
        self.row_uploads.borrow().clone()
    }
    fn released(&self) -> Vec<i64> {
        self.released.borrow().clone()
    }
    fn image_of(&self, id: i64) -> Rc<FakeImage> {
        self.images
            .borrow()
            .iter()
            .rev()
            .find(|image| image.id == id)
            .map(Rc::clone)
            .unwrap()
    }
}

struct FakeBackend {
    state: Rc<FakeState>,
}

impl GpuImageBackend for FakeBackend {
    type Image = FakeImage;

    fn upload(&self, pixels: &PixelBuffer, with_data: bool) -> Result<Rc<FakeImage>, GfxError> {
        if let Some(error) = self.state.fail_next_upload.take() {
            return Err(error);
        }
        self.state.uploads.borrow_mut().push((pixels.id(), with_data));
        let image = Rc::new(FakeImage {
            id: pixels.id(),
            width: pixels.width(),
            height: pixels.height(),
            bytes: i64::from(pixels.width()) * i64::from(pixels.height()) * 4,
            uploaded_rows: Cell::new(0),
            released: Cell::new(false),
        });
        self.state.images.borrow_mut().push(Rc::clone(&image));
        Ok(image)
    }

    fn upload_rows(
        &self,
        _image: &FakeImage,
        _pixels: &PixelBuffer,
        start_row: i32,
        end_row: i32,
    ) -> Result<(), GfxError> {
        if let Some(error) = self.state.fail_next_rows.take() {
            return Err(error);
        }
        self.state.row_uploads.borrow_mut().push((start_row, end_row));
        Ok(())
    }

    fn release(&self, image: &FakeImage) {
        self.state.released.borrow_mut().push(image.id);
    }

    fn subscribe_device_recreated(&self, handler: Box<dyn Fn()>) -> u64 {
        let id = self.state.next_handler.get();
        self.state.next_handler.set(id + 1);
        self.state.handlers.borrow_mut().push((id, Rc::from(handler)));
        id
    }

    fn unsubscribe_device_recreated(&self, id: u64) {
        self.state
            .handlers
            .borrow_mut()
            .retain(|(handler_id, _)| *handler_id != id);
    }
}

#[derive(Default)]
struct ManualScheduler {
    jobs: RefCell<VecDeque<StripeToken>>,
}

impl StripeScheduler for ManualScheduler {
    fn schedule_stripe(&self, token: StripeToken) {
        self.jobs.borrow_mut().push_back(token);
    }
}

type Cache = BudgetedImageCache<FakeBackend>;

impl ManualScheduler {
    fn pending_count(&self) -> usize {
        self.jobs.borrow().len()
    }
    fn pending(&self) -> Vec<StripeToken> {
        self.jobs.borrow().iter().copied().collect()
    }
    fn run_one_result(&self, cache: &Cache) -> Option<Result<(), CacheError>> {
        let token = self.jobs.borrow_mut().pop_front()?;
        Some(cache.run_stripe(token))
    }
    fn run_one(&self, cache: &Cache) -> bool {
        match self.run_one_result(cache) {
            Some(result) => {
                result.unwrap();
                true
            }
            None => false,
        }
    }
    fn run_all(&self, cache: &Cache) -> usize {
        let mut count = 0;
        while self.run_one(cache) {
            count += 1;
        }
        count
    }
}

struct Fixture {
    cache: Rc<Cache>,
    backend: Rc<FakeState>,
    scheduler: Rc<ManualScheduler>,
}

/// `Cache(budget, stripe_bytes)` of the C# tests.
fn fixture(budget: i64, stripe_bytes: i32) -> Fixture {
    let backend = Rc::new(FakeState::default());
    let scheduler = Rc::new(ManualScheduler::default());
    let cache = BudgetedImageCache::new(
        FakeBackend {
            state: Rc::clone(&backend),
        },
        Rc::clone(&scheduler) as Rc<dyn StripeScheduler>,
        budget,
        stripe_bytes,
    )
    .unwrap();
    Fixture {
        cache,
        backend,
        scheduler,
    }
}

fn cache(budget: i64) -> Fixture {
    fixture(budget, 40)
}

/// `Image()` of the C# tests: 10x10 Pbgra32 = 400 bytes, stride 40.
fn image() -> Arc<PixelBuffer> {
    sized(10, 10)
}

fn sized(width: i32, height: i32) -> Arc<PixelBuffer> {
    Arc::new(PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap())
}

fn collect_completed(fx: &Fixture) -> Rc<RefCell<Vec<i64>>> {
    let completed = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&completed);
    fx.cache
        .subscribe_upload_completed(Rc::new(move |id| sink.borrow_mut().push(id)));
    completed
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported from GpuImageCacheTests.cs
// ---------------------------------------------------------------------------------------------------------------------

// C#: GpuImageCacheTests.GetOrUpload_SameBuffer_UploadsOnceAndReturnsSameImage
#[test]
fn get_or_upload_same_buffer_uploads_once_and_returns_the_same_rc() {
    let fx = cache(10_000);
    let px = image();

    let first = fx.cache.get_or_upload(&px).unwrap();
    let second = fx.cache.get_or_upload(&px).unwrap();

    assert!(Rc::ptr_eq(&first, &second));
    assert_eq!(fx.backend.uploads().len(), 1);
    assert_eq!(
        (first.id, first.width, first.height, first.bytes),
        (px.id(), 10, 10, 400)
    );
    assert_eq!(fx.cache.total_gpu_bytes(), 400);
    assert!(fx.backend.uploads()[0].1);
    assert_eq!(first.uploaded_rows(), 10);
}

// C#: GpuImageCacheTests.GetOrUpload_OverBudget_EvictsLeastRecentlyUsed
#[test]
fn get_or_upload_over_budget_evicts_least_recently_used() {
    let fx = cache(1000);
    let (a, b, c) = (image(), image(), image());

    fx.cache.get_or_upload(&a).unwrap();
    fx.cache.get_or_upload(&b).unwrap();
    fx.cache.get_or_upload(&a).unwrap();
    fx.cache.get_or_upload(&c).unwrap();

    assert!(fx.cache.contains(a.id()));
    assert!(!fx.cache.contains(b.id()));
    assert!(fx.cache.contains(c.id()));
    assert_eq!(fx.cache.total_gpu_bytes(), 800);
    assert_eq!(fx.backend.released(), vec![b.id()]);
}

// C#: GpuImageCacheTests.GetOrUpload_ImageLargerThanBudget_IsKeptAsCurrent
#[test]
fn get_or_upload_image_larger_than_budget_is_kept_as_current() {
    let fx = cache(100);
    let small = sized(2, 2);
    let big = sized(100, 100);

    fx.cache.get_or_upload(&small).unwrap();
    let big_image = fx.cache.get_or_upload(&big).unwrap();

    assert!(fx.cache.contains(big.id()));
    assert!(!fx.cache.contains(small.id()));
    assert_eq!(fx.cache.total_gpu_bytes(), big_image.gpu_bytes());
    assert_eq!(big_image.gpu_bytes(), 40_000);
    assert!(!big_image.is_released());
}

// C#: GpuImageCacheTests.Prefetch_DoesNotEvictCurrentImage
#[test]
fn prefetch_does_not_evict_current_image() {
    let fx = cache(500);
    let (current, next) = (image(), image());

    fx.cache.get_or_upload(&current).unwrap();
    fx.cache.prefetch(&next).unwrap();

    assert!(fx.cache.contains(current.id()));
    assert!(fx.cache.contains(next.id()));
    assert!(fx.backend.released().is_empty());
}

// C#: GpuImageCacheTests.Retain_PinsIdsBeyondBudget_AndUnpinningEvicts
#[test]
fn retain_pins_ids_beyond_budget_and_unpinning_evicts() {
    let fx = cache(10_000);
    let (a, b, c) = (image(), image(), image());
    for px in [&a, &b, &c] {
        fx.cache.get_or_upload(px).unwrap();
    }

    fx.cache.retain(&[a.id(), b.id()]).unwrap();
    fx.cache.set_budget_bytes(0).unwrap();

    assert!(fx.cache.contains(a.id()));
    assert!(fx.cache.contains(b.id()));
    assert!(fx.cache.contains(c.id())); // current

    fx.cache.retain(&[b.id()]).unwrap();

    assert!(!fx.cache.contains(a.id()));
    assert!(fx.cache.contains(b.id()));
    assert_eq!(fx.cache.total_gpu_bytes(), 800);
}

// C#: GpuImageCacheTests.BudgetBytes_Lowered_EvictsImmediately_NegativeRejected
#[test]
fn budget_lowered_evicts_immediately_and_negative_is_rejected() {
    let fx = cache(10_000);
    let (a, b, c) = (image(), image(), image());
    for px in [&a, &b, &c] {
        fx.cache.get_or_upload(px).unwrap();
    }

    fx.cache.set_budget_bytes(800).unwrap();

    assert_eq!(fx.cache.total_gpu_bytes(), 800);
    assert!(!fx.cache.contains(a.id()));
    assert_eq!(fx.cache.budget_bytes(), 800);
    assert_eq!(
        fx.cache.set_budget_bytes(-1),
        Err(CacheError::NegativeBudget { value: -1 })
    );
    assert_eq!(fx.cache.budget_bytes(), 800);
    assert_eq!(fx.cache.count(), 2);
}

// C#: GpuImageCacheTests.Prefetch_RaisesUploadCompletedOnce_OnlyAfterTheLastStripe_AndTheImageIsThenFullyUploaded
#[test]
fn prefetch_raises_upload_completed_once_only_after_the_last_stripe() {
    let fx = fixture(10_000, 80);
    let completed = collect_completed(&fx);
    let px = sized(10, 9); // stride 40: 2 rows per stripe, 5 stripes

    fx.cache.prefetch(&px).unwrap();
    assert!(!fx.cache.is_fully_uploaded(px.id()));

    for _ in 0..4 {
        assert!(fx.scheduler.run_one(&fx.cache));
        assert!(completed.borrow().is_empty());
        assert!(!fx.cache.is_fully_uploaded(px.id()));
    }
    assert!(fx.scheduler.run_one(&fx.cache));

    assert_eq!(*completed.borrow(), vec![px.id()]);
    assert!(fx.cache.is_fully_uploaded(px.id()));
    assert_eq!(fx.scheduler.pending_count(), 0);
    assert!(!fx.scheduler.run_one(&fx.cache));
}

// C#: GpuImageCacheTests.Prefetch_UploadsInBackgroundStripes_NotSynchronously
#[test]
fn prefetch_uploads_in_background_stripes_not_synchronously() {
    let fx = fixture(10_000, 80);
    let px = sized(10, 9);

    fx.cache.prefetch(&px).unwrap();

    assert!(!fx.backend.uploads()[0].1);
    assert!(fx.backend.row_uploads().is_empty());
    assert_eq!(fx.cache.pending_prefetch_count(), 1);
    assert_eq!(fx.scheduler.pending_count(), 1);

    assert_eq!(fx.scheduler.run_all(&fx.cache), 5);

    assert_eq!(fx.backend.row_uploads(), vec![(0, 2), (2, 4), (4, 6), (6, 8), (8, 9)]);
    assert_eq!(fx.cache.pending_prefetch_count(), 0);
    let done = fx.cache.get_or_upload(&px).unwrap();
    assert!(done.is_complete());
    assert_eq!(fx.backend.uploads().len(), 1);
}

// C#: GpuImageCacheTests.GetOrUpload_DuringPrefetch_UploadsRemainingRowsAtOnce
#[test]
fn get_or_upload_during_prefetch_uploads_remaining_rows_at_once() {
    let fx = fixture(10_000, 80);
    let px = sized(10, 9);

    fx.cache.prefetch(&px).unwrap();
    assert!(fx.scheduler.run_one(&fx.cache));
    let image = fx.cache.get_or_upload(&px).unwrap();

    assert_eq!(fx.backend.row_uploads(), vec![(0, 2), (2, 9)]);
    assert!(image.is_complete());
    assert_eq!(fx.scheduler.run_all(&fx.cache), 1); // the queued job is inert
    assert_eq!(fx.backend.row_uploads().len(), 2);
}

// C#: GpuImageCacheTests.Prefetch_AlreadyCached_IsNoOp
#[test]
fn prefetch_already_cached_is_a_no_op() {
    let fx = cache(10_000);
    let px = image();

    fx.cache.get_or_upload(&px).unwrap();
    fx.cache.prefetch(&px).unwrap();
    fx.cache.prefetch(&px).unwrap();

    assert_eq!(fx.backend.uploads().len(), 1);
    assert_eq!(fx.scheduler.pending_count(), 0);
}

// C#: GpuImageCacheTests.Prefetch_SourceDisposedMidway_DropsPartialImage
// Different row (ownership, contract note 12): the owner "disposing" the buffer is the last other `Arc` being dropped.
#[test]
fn prefetch_source_dropped_midway_drops_partial_image() {
    let fx = fixture(10_000, 80);
    let completed = collect_completed(&fx);
    let px = sized(10, 9);
    let id = px.id();

    fx.cache.prefetch(&px).unwrap();
    assert!(fx.scheduler.run_one(&fx.cache));
    drop(px);
    fx.scheduler.run_all(&fx.cache);

    assert!(!fx.cache.contains(id));
    assert_eq!(fx.cache.total_gpu_bytes(), 0);
    assert_eq!(fx.backend.released(), vec![id]);
    assert_eq!(fx.backend.row_uploads().len(), 1);
    assert!(completed.borrow().is_empty());
}

// C#: GpuImageCacheTests.Prefetch_EvictedBeforeStripesRun_StopsUploading
#[test]
fn prefetch_evicted_before_stripes_run_stops_uploading() {
    let fx = fixture(10_000, 80);
    let px = sized(10, 9);

    fx.cache.prefetch(&px).unwrap();
    fx.cache.clear();
    fx.scheduler.run_all(&fx.cache);

    assert!(fx.backend.row_uploads().is_empty());
    assert_eq!(fx.backend.released(), vec![px.id()]);
}

// C#: GpuImageCacheTests.DeviceRecreated_ReleasesEverythingAndNextGetUploadsAgain
#[test]
fn device_recreated_releases_everything_and_next_get_uploads_again() {
    let fx = cache(10_000);
    let (a, b) = (image(), image());
    let before = fx.cache.get_or_upload(&a).unwrap();
    fx.cache.get_or_upload(&b).unwrap();

    fx.backend.raise_device_recreated();

    assert_eq!(fx.cache.count(), 0);
    assert_eq!(fx.cache.total_gpu_bytes(), 0);
    assert!(before.is_released());
    assert_eq!(fx.backend.released().len(), 2);
    let after = fx.cache.get_or_upload(&a).unwrap();
    assert!(!Rc::ptr_eq(&before, &after));
    assert_eq!(fx.backend.uploads().len(), 3);
}

// C#: GpuImageCacheTests.Dispose_ReleasesAllAndUnsubscribes
#[test]
fn dispose_releases_all_and_unsubscribes() {
    let fx = cache(10_000);
    let a = image();
    fx.cache.get_or_upload(&a).unwrap();

    fx.cache.dispose();
    fx.cache.dispose();

    assert_eq!(fx.backend.released(), vec![a.id()]);
    assert!(!fx.backend.has_subscribers());
    assert!(fx.cache.is_disposed());
    assert_eq!(
        fx.cache.get_or_upload(&a).unwrap_err(),
        CacheError::Disposed {
            object: CACHE_OBJECT_NAME
        }
    );
}

// C#: GpuImageCacheTests.GetOrUpload_DisposedBuffer_Throws
#[test]
fn get_or_upload_disposed_buffer_is_an_error_and_prefetch_ignores_it() {
    let fx = cache(10_000);
    let mut buffer = PixelBuffer::allocate(2, 2, PixelLayout::Pbgra32).unwrap();
    buffer.dispose();
    let px = Arc::new(buffer);

    assert_eq!(
        fx.cache.get_or_upload(&px).unwrap_err(),
        CacheError::Disposed {
            object: PIXEL_BUFFER_OBJECT_NAME
        }
    );
    assert_eq!(fx.cache.prefetch(&px), Ok(()));
    assert!(fx.backend.uploads().is_empty());
    assert_eq!(fx.scheduler.pending_count(), 0);
}

// C#: GpuImageCacheTests.ThousandUploads_NeverExceedBudgetBeyondCurrent_AndReleaseEverything
#[test]
fn thousand_uploads_never_exceed_budget_beyond_current_and_release_everything() {
    let fx = cache(4_000); // 10 images of 400 bytes
    for i in 0..1000 {
        let px = image();
        if i % 3 == 0 {
            fx.cache.prefetch(&px).unwrap();
            fx.scheduler.run_all(&fx.cache);
        }
        fx.cache.get_or_upload(&px).unwrap();
        assert!(
            fx.cache.total_gpu_bytes() <= fx.cache.budget_bytes(),
            "{} > budget at {i}",
            fx.cache.total_gpu_bytes()
        );
    }

    assert_eq!(fx.cache.count(), 10);
    fx.cache.clear();
    assert_eq!(fx.backend.released().len(), fx.backend.uploads().len());
    assert_eq!(fx.cache.total_gpu_bytes(), 0);
}

// ---------------------------------------------------------------------------------------------------------------------
// New tests (contract traps)
// ---------------------------------------------------------------------------------------------------------------------

// new: budget formula in i64
#[test]
fn default_budget_is_six_screens_of_at_least_full_hd() {
    assert_eq!(default_budget_bytes(64, 64), 49_766_400);
    assert_eq!(default_budget_bytes(1920, 1080), 49_766_400);
    assert_eq!(default_budget_bytes(2560, 1440), 88_473_600);
    assert_eq!(default_budget_bytes(40_000, 40_000), 38_400_000_000);
    assert_eq!(default_budget_bytes(1, 1), 49_766_400);
    // just above Full HD: the surface wins over the minimum
    assert_eq!(default_budget_bytes(1921, 1080), 6 * 1921 * 1080 * 4);
    assert_eq!(default_budget_bytes(1920, 1081), 6 * 1920 * 1081 * 4);
}

// new: constructor validation
#[test]
fn new_validates_budget_and_stripe_size() {
    let make = |budget: i64, stripe: i32| {
        BudgetedImageCache::new(
            FakeBackend {
                state: Rc::new(FakeState::default()),
            },
            Rc::new(ManualScheduler::default()),
            budget,
            stripe,
        )
    };
    assert_eq!(make(-1, 40).unwrap_err(), CacheError::NegativeBudget { value: -1 });
    assert_eq!(make(0, 3).unwrap_err(), CacheError::StripeBytesTooSmall { value: 3 });
    // budget is checked first
    assert_eq!(make(-5, 3).unwrap_err(), CacheError::NegativeBudget { value: -5 });
    let cache = make(0, 4).unwrap();
    assert_eq!(cache.budget_bytes(), 0);
    assert!(!cache.is_disposed());
}

// new: a failed constructor must not subscribe to the backend
#[test]
fn failed_new_does_not_subscribe() {
    let state = Rc::new(FakeState::default());
    let result = BudgetedImageCache::new(
        FakeBackend {
            state: Rc::clone(&state),
        },
        Rc::new(ManualScheduler::default()),
        -1,
        40,
    );
    assert!(result.is_err());
    assert!(!state.has_subscribers());
}

// new: T1, a stripe job of a cleared entry stays inert for the re-cached buffer
#[test]
fn stale_stripe_after_clear_is_inert_for_a_recached_buffer() {
    let fx = fixture(10_000, 80);
    let px = sized(10, 9);

    fx.cache.prefetch(&px).unwrap();
    fx.cache.clear();
    fx.cache.prefetch(&px).unwrap();

    let jobs = fx.scheduler.pending();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].pixel_buffer_id, jobs[1].pixel_buffer_id);
    assert_ne!(jobs[0].serial, jobs[1].serial);

    assert!(fx.scheduler.run_one(&fx.cache)); // the stale job
    assert!(fx.backend.row_uploads().is_empty());
    assert_eq!(fx.scheduler.run_all(&fx.cache), 5);
    assert_eq!(fx.backend.row_uploads().len(), 5);
    assert!(fx.cache.is_fully_uploaded(px.id()));
}

// new: T1 for a second flavour, the stale job goes through `run_stripe` directly with a wrong serial
#[test]
fn run_stripe_with_a_foreign_serial_is_inert() {
    let fx = fixture(10_000, 80);
    let px = sized(10, 9);
    fx.cache.prefetch(&px).unwrap();
    let token = fx.scheduler.pending()[0];

    let wrong = StripeToken {
        pixel_buffer_id: token.pixel_buffer_id,
        serial: token.serial + 1,
    };
    assert_eq!(fx.cache.run_stripe(wrong), Ok(()));
    assert!(fx.backend.row_uploads().is_empty());
    assert_eq!(fx.cache.run_stripe(token), Ok(()));
    assert_eq!(fx.backend.row_uploads(), vec![(0, 2)]);
}

// new: T2, eviction exceptions
#[test]
fn a_new_prefetch_can_evict_an_older_prefetched_image_but_never_current_or_itself() {
    let fx = cache(800);
    let (c, p1, p2) = (image(), image(), image());

    fx.cache.get_or_upload(&c).unwrap();
    fx.cache.prefetch(&p1).unwrap();
    fx.cache.prefetch(&p2).unwrap();

    assert!(fx.cache.contains(c.id()));
    assert!(fx.cache.contains(p2.id()));
    assert!(!fx.cache.contains(p1.id()));
    assert_eq!(fx.backend.released(), vec![p1.id()]);
    assert_eq!(fx.scheduler.run_all(&fx.cache), 11); // p1's job is inert, p2's 10 stripes run
    assert_eq!(fx.backend.row_uploads().len(), 10);
}

// new: a prefetch never becomes the protected current image
#[test]
fn prefetch_does_not_become_current() {
    let fx = cache(10_000);
    let (a, b) = (image(), image());
    fx.cache.get_or_upload(&a).unwrap();
    fx.cache.prefetch(&b).unwrap();

    fx.cache.set_budget_bytes(0).unwrap();

    assert!(fx.cache.contains(a.id())); // still current
    assert!(!fx.cache.contains(b.id())); // prefetched, not current, not retained
}

// new: T2, an image evicted by a retained set change is released exactly once
#[test]
fn eviction_walks_oldest_first_and_stops_as_soon_as_the_cache_fits() {
    let fx = cache(10_000);
    let ids: Vec<_> = (0..5).map(|_| image()).collect();
    for px in &ids {
        fx.cache.get_or_upload(px).unwrap();
    }

    fx.cache.set_budget_bytes(1000).unwrap(); // 2000 bytes must go: exactly the 3 oldest

    assert_eq!(fx.cache.total_gpu_bytes(), 800);
    assert_eq!(fx.backend.released(), vec![ids[0].id(), ids[1].id(), ids[2].id()]);
    // total == budget does not evict anything
    fx.cache.set_budget_bytes(800).unwrap();
    assert_eq!(fx.cache.count(), 2);
    assert_eq!(fx.backend.released().len(), 3);
}

// new: T3, touch order
#[test]
fn touch_by_get_or_upload_and_by_prefetch_changes_eviction_order() {
    for use_prefetch in [true, false] {
        let fx = cache(800);
        let (a, b, c) = (image(), image(), image());
        fx.cache.get_or_upload(&a).unwrap();
        fx.cache.get_or_upload(&b).unwrap();
        if use_prefetch {
            fx.cache.prefetch(&a).unwrap(); // cached: touch only
        } else {
            fx.cache.get_or_upload(&a).unwrap();
        }

        fx.cache.get_or_upload(&c).unwrap();

        assert!(!fx.cache.contains(b.id()), "use_prefetch={use_prefetch}");
        assert!(fx.cache.contains(a.id()));
        assert!(fx.cache.contains(c.id()));
        assert_eq!(fx.backend.released(), vec![b.id()]);
    }
}

// new: T4
#[test]
fn half_prefetched_image_counts_its_full_bytes() {
    let fx = fixture(10_000, 80);
    let px = image();

    fx.cache.prefetch(&px).unwrap();

    assert_eq!(fx.cache.total_gpu_bytes(), 400);
    assert_eq!(fx.cache.pending_prefetch_count(), 1);
    assert_eq!(fx.cache.count(), 1);
    assert!(fx.cache.contains(px.id()));
}

// new: T5
#[test]
fn rows_per_stripe_is_at_least_one() {
    let fx = fixture(10_000, 4);
    let px = sized(10, 9);

    fx.cache.prefetch(&px).unwrap();
    assert_eq!(fx.scheduler.run_all(&fx.cache), 9);

    let expected: Vec<(i32, i32)> = (0..9).map(|i| (i, i + 1)).collect();
    assert_eq!(fx.backend.row_uploads(), expected);
}

// new: integer division of stripe_bytes / stride (stripe 100, stride 40 -> 2 rows, not 3)
#[test]
fn rows_per_stripe_uses_integer_division() {
    let fx = fixture(10_000, 100);
    let px = sized(10, 9);

    fx.cache.prefetch(&px).unwrap();
    fx.scheduler.run_all(&fx.cache);

    assert_eq!(fx.backend.row_uploads(), vec![(0, 2), (2, 4), (4, 6), (6, 8), (8, 9)]);
}

// new: a stripe larger than the whole image finishes in one job and clamps to the height
#[test]
fn a_stripe_bigger_than_the_image_finishes_in_one_job() {
    let fx = fixture(10_000, i32::MAX);
    let completed = collect_completed(&fx);
    let px = sized(10, 9);

    fx.cache.prefetch(&px).unwrap();
    assert_eq!(fx.scheduler.run_all(&fx.cache), 1);

    assert_eq!(fx.backend.row_uploads(), vec![(0, 9)]);
    assert_eq!(*completed.borrow(), vec![px.id()]);
}

// new: T6
#[test]
fn retained_ids_survive_clear() {
    let fx = cache(10_000);
    let (a, b) = (image(), image());
    fx.cache.retain(&[a.id()]).unwrap();
    fx.cache.clear();

    fx.cache.get_or_upload(&a).unwrap();
    fx.cache.get_or_upload(&b).unwrap();
    fx.cache.set_budget_bytes(0).unwrap();

    assert!(fx.cache.contains(a.id())); // retained
    assert!(fx.cache.contains(b.id())); // current
}

// new: `retain` replaces the set (no union)
#[test]
fn retain_replaces_the_previous_set() {
    let fx = cache(10_000);
    let (a, b, c) = (image(), image(), image());
    for px in [&a, &b, &c] {
        fx.cache.get_or_upload(px).unwrap();
    }
    fx.cache.retain(&[a.id()]).unwrap();
    fx.cache.retain(&[b.id()]).unwrap();

    fx.cache.set_budget_bytes(0).unwrap();

    assert!(!fx.cache.contains(a.id()));
    assert!(fx.cache.contains(b.id()));
    assert!(fx.cache.contains(c.id()));
}

// new: clear() resets the protected "current" image
#[test]
fn clear_resets_the_current_image() {
    let fx = cache(10_000);
    let (a, b) = (image(), image());
    fx.cache.get_or_upload(&a).unwrap();
    fx.cache.clear();
    fx.cache.prefetch(&b).unwrap();

    fx.cache.set_budget_bytes(0).unwrap();

    assert_eq!(fx.cache.count(), 0); // nothing is current any more
}

// new: a failed upload keeps the previous current image
#[test]
fn failed_upload_leaves_state_and_current_image_unchanged() {
    let fx = cache(10_000);
    let (a, b) = (image(), image());
    fx.cache.get_or_upload(&a).unwrap();
    fx.backend.fail_next_upload.set(Some(gfx_error()));

    let error = fx.cache.get_or_upload(&b).unwrap_err();

    assert_eq!(error, CacheError::Backend(gfx_error()));
    assert_eq!(fx.cache.count(), 1);
    fx.cache.set_budget_bytes(0).unwrap();
    assert!(fx.cache.contains(a.id())); // still current
    assert_eq!(fx.cache.total_gpu_bytes(), 400);
}

// new: a failing prefetch upload adds nothing and schedules nothing
#[test]
fn failed_prefetch_upload_adds_nothing() {
    let fx = cache(10_000);
    let a = image();
    fx.backend.fail_next_upload.set(Some(gfx_error()));

    assert_eq!(fx.cache.prefetch(&a), Err(CacheError::Backend(gfx_error())));

    assert_eq!(fx.cache.count(), 0);
    assert_eq!(fx.scheduler.pending_count(), 0);
}

// new: T3, a failed completion neither touches nor protects
#[test]
fn failed_completion_upload_does_not_touch_or_protect() {
    let fx = fixture(10_000, 80);
    let (a, b, c) = (sized(10, 10), sized(10, 10), sized(10, 10));
    fx.cache.prefetch(&a).unwrap();
    assert!(fx.scheduler.run_one(&fx.cache)); // a: 2 rows
    fx.cache.get_or_upload(&b).unwrap();
    fx.cache.get_or_upload(&c).unwrap();
    fx.backend.fail_next_rows.set(Some(gfx_error()));

    let error = fx.cache.get_or_upload(&a).unwrap_err();

    assert_eq!(error, CacheError::Backend(gfx_error()));
    assert_eq!(fx.backend.image_of(a.id()).uploaded_rows(), 2);
    assert!(!fx.cache.is_fully_uploaded(a.id()));
    fx.cache.set_budget_bytes(800).unwrap();
    assert!(!fx.cache.contains(a.id())); // oldest, not touched, not protected
    assert!(fx.cache.contains(b.id()));
    assert!(fx.cache.contains(c.id()));
}

// new: a stripe failure
#[test]
fn failed_stripe_keeps_the_entry_and_does_not_reschedule() {
    let fx = fixture(10_000, 80);
    let completed = collect_completed(&fx);
    let px = sized(10, 9);
    fx.cache.prefetch(&px).unwrap();
    fx.backend.fail_next_rows.set(Some(gfx_error()));

    let result = fx.scheduler.run_one_result(&fx.cache);

    assert_eq!(result, Some(Err(CacheError::Backend(gfx_error()))));
    assert_eq!(fx.scheduler.pending_count(), 0);
    assert!(fx.cache.contains(px.id()));
    assert_eq!(fx.backend.image_of(px.id()).uploaded_rows(), 0);
    assert!(completed.borrow().is_empty());
    let image = fx.cache.get_or_upload(&px).unwrap();
    assert!(image.is_complete());
    assert!(fx.cache.is_fully_uploaded(px.id()));
}

// new: T9, handlers run after the borrow is released
#[test]
fn upload_completed_handler_may_call_back_into_the_cache() {
    let fx = fixture(10_000, 80);
    let px = sized(10, 9);
    let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let weak = Rc::downgrade(&fx.cache);

    let (log1, px1) = (Rc::clone(&log), Arc::clone(&px));
    fx.cache.subscribe_upload_completed(Rc::new(move |id| {
        let cache = weak.upgrade().unwrap();
        assert!(cache.contains(id));
        let image = cache.get_or_upload(&px1).unwrap();
        log1.borrow_mut().push(format!("h1:{id}:{}", image.is_complete()));
    }));
    let log2 = Rc::clone(&log);
    fx.cache
        .subscribe_upload_completed(Rc::new(move |id| log2.borrow_mut().push(format!("h2:{id}"))));
    let log3 = Rc::clone(&log);
    let removed = fx
        .cache
        .subscribe_upload_completed(Rc::new(move |id| log3.borrow_mut().push(format!("h3:{id}"))));
    fx.cache.unsubscribe_upload_completed(removed);
    fx.cache.unsubscribe_upload_completed(9999); // unknown id: no-op

    fx.cache.prefetch(&px).unwrap();
    fx.scheduler.run_all(&fx.cache);

    let id = px.id();
    assert_eq!(*log.borrow(), vec![format!("h1:{id}:true"), format!("h2:{id}")]);
}

// new: a handler may schedule/subscribe while being called (the list is cloned first)
#[test]
fn upload_completed_handler_may_unsubscribe_itself() {
    let fx = fixture(10_000, 80);
    let calls = Rc::new(Cell::new(0));
    let weak = Rc::downgrade(&fx.cache);
    let handler_id = Rc::new(Cell::new(0_u64));

    let (calls_in, id_in) = (Rc::clone(&calls), Rc::clone(&handler_id));
    let id = fx.cache.subscribe_upload_completed(Rc::new(move |_| {
        calls_in.set(calls_in.get() + 1);
        weak.upgrade().unwrap().unsubscribe_upload_completed(id_in.get());
    }));
    handler_id.set(id);

    for _ in 0..2 {
        let px = sized(10, 9);
        fx.cache.prefetch(&px).unwrap();
        fx.scheduler.run_all(&fx.cache);
    }

    assert_eq!(calls.get(), 1);
}

// new
#[test]
fn run_stripe_after_dispose_is_inert_and_set_budget_still_works() {
    let fx = fixture(10_000, 80);
    let (px, other) = (sized(10, 9), image());
    fx.cache.prefetch(&px).unwrap();

    fx.cache.dispose();
    assert_eq!(fx.scheduler.run_all(&fx.cache), 1);

    assert!(fx.backend.row_uploads().is_empty());
    assert_eq!(fx.cache.set_budget_bytes(5), Ok(()));
    assert_eq!(fx.cache.budget_bytes(), 5);
    let disposed = CacheError::Disposed {
        object: CACHE_OBJECT_NAME,
    };
    assert_eq!(fx.cache.retain(&[1]), Err(disposed));
    assert_eq!(fx.cache.prefetch(&other), Err(disposed));
    assert_eq!(fx.cache.count(), 0);
    assert_eq!(fx.cache.total_gpu_bytes(), 0);
    assert!(!fx.cache.contains(px.id()));
}

// new: T7, the disposed cache is reported before a disposed buffer
#[test]
fn disposed_cache_is_reported_before_a_disposed_buffer() {
    let fx = cache(10_000);
    let mut buffer = PixelBuffer::allocate(2, 2, PixelLayout::Pbgra32).unwrap();
    buffer.dispose();
    fx.cache.dispose();

    assert_eq!(
        fx.cache.get_or_upload(&buffer).unwrap_err(),
        CacheError::Disposed {
            object: CACHE_OBJECT_NAME
        }
    );
    assert_eq!(
        fx.cache.prefetch(&Arc::new(buffer)),
        Err(CacheError::Disposed {
            object: CACHE_OBJECT_NAME
        })
    );
}

// new
#[test]
fn device_handler_does_nothing_after_the_cache_is_dropped() {
    let fx = cache(10_000);
    let a = image();
    fx.cache.get_or_upload(&a).unwrap();
    let Fixture { cache, backend, .. } = fx;
    assert!(backend.has_subscribers());

    drop(cache);
    backend.raise_device_recreated();

    assert!(!backend.has_subscribers());
    assert_eq!(backend.released(), vec![a.id()]); // Drop = dispose released the image
}

// new: the device handler is the subscribed `on_device_recreated`, also with a pending prefetch
#[test]
fn device_recreated_with_pending_prefetch_makes_the_stripe_inert() {
    let fx = fixture(10_000, 80);
    let px = sized(10, 9);
    fx.cache.prefetch(&px).unwrap();

    fx.backend.raise_device_recreated();
    fx.scheduler.run_all(&fx.cache);

    assert_eq!(fx.cache.count(), 0);
    assert!(fx.backend.row_uploads().is_empty());
}

// new: the last stripe drops the pending source (T10)
#[test]
fn completed_prefetch_releases_the_source_buffer() {
    let fx = fixture(10_000, 80);
    let px = sized(10, 9);

    fx.cache.prefetch(&px).unwrap();
    assert_eq!(Arc::strong_count(&px), 2);
    fx.scheduler.run_all(&fx.cache);

    assert_eq!(Arc::strong_count(&px), 1);
    // an eviction of a pending image also drops the source
    let other = sized(10, 9);
    fx.cache.prefetch(&other).unwrap();
    assert_eq!(Arc::strong_count(&other), 2);
    fx.cache.clear();
    assert_eq!(Arc::strong_count(&other), 1);
}

// new: the uploaded rows are set by `get_or_upload` for a fresh buffer and `is_fully_uploaded` needs a live image
#[test]
fn is_fully_uploaded_is_false_for_unknown_partial_and_cleared_images() {
    let fx = fixture(10_000, 80);
    let px = sized(10, 9);
    assert!(!fx.cache.is_fully_uploaded(px.id()));

    fx.cache.prefetch(&px).unwrap();
    assert!(!fx.cache.is_fully_uploaded(px.id()));
    fx.cache.get_or_upload(&px).unwrap();
    assert!(fx.cache.is_fully_uploaded(px.id()));
    fx.cache.clear();
    assert!(!fx.cache.is_fully_uploaded(px.id()));
}

// new: a released flag set by the cache is idempotent (no double backend release)
#[test]
fn a_released_image_is_given_back_only_once() {
    let fx = cache(10_000);
    let a = image();
    fx.cache.get_or_upload(&a).unwrap();

    fx.cache.clear();
    fx.cache.clear();
    fx.cache.dispose();

    assert_eq!(fx.backend.released(), vec![a.id()]);
}

// new: Debug does not panic while the state is borrowed and shows the counters
#[test]
fn debug_output_names_the_type() {
    let fx = cache(10_000);
    let text = format!("{:?}", fx.cache);
    assert!(text.contains("BudgetedImageCache"));
    assert!(text.contains("budget_bytes"));
}

// new: error texts
#[test]
fn cache_error_display_mentions_the_values() {
    let text = CacheError::NegativeBudget { value: -7 }.to_string();
    assert!(text.contains("-7"));
    assert!(CacheError::StripeBytesTooSmall { value: 3 }.to_string().contains('3'));
    assert!(
        CacheError::Disposed {
            object: CACHE_OBJECT_NAME
        }
        .to_string()
        .contains(CACHE_OBJECT_NAME)
    );
    assert!(CacheError::Backend(gfx_error()).to_string().contains("FakeBackend"));
    assert_eq!(CacheError::from(gfx_error()), CacheError::Backend(gfx_error()));
}

// new: the public constants keep the C# values
#[test]
fn constants_match_the_csharp_values() {
    assert_eq!(DEFAULT_STRIPE_BYTES, 2_097_152);
    assert_eq!(DEFAULT_BUDGET_SCREENS, 6);
    assert_eq!(MINIMUM_SCREEN_BYTES, 8_294_400);
    assert_eq!(MIN_STRIPE_BYTES, 4);
}
