//! Port of `PhotoReview.Shell.Rendering/GpuImageCache.cs` lines 1-300 (the class `GpuImageCache` and its private `Entry`), baseline
//! `d92d6aae`. Unit W8-12f (pure, test kind F).
//!
//! WP-15 (C-10): a GPU image cache keyed by `PixelBuffer::id()` (INV-1), LRU by bytes up to `budget_bytes`. The image of the latest
//! `get_or_upload` and the ids of the latest `retain` are never evicted, even over budget (a 24 MP current image is larger than the
//! default budget and must still draw). `prefetch` creates empty textures and uploads them in stripes of about
//! [`DEFAULT_STRIPE_BYTES`], one background job per stripe, so the UI thread is never blocked longer than one stripe. A lost device
//! (the backend's device-recreated notification) releases everything; the next draw uploads again from the buffer the caller still owns.
//!
//! No `windows` type and no Direct2D call here: the backend ([`GpuImageBackend`]) and the stripe scheduler ([`StripeScheduler`]) are
//! seams, so the whole policy runs on fakes (the C# tests use `FakeBackend` + `ManualDispatcher`).
//! `pr_core::caching::lru::BoundedLruCache` is NOT used: it is thread-safe, evicts on insert and has no "protected" entries
//! (D-W8B3S-3).
//!
//! UI thread only (`Rc`, `RefCell`, no `Send`). The cache is shared as `Rc<BudgetedImageCache<B>>`.

#![allow(unused_variables, dead_code)] // W8-12f stub: the porter removes this line when the last `todo!()` is gone.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use pr_imaging::pixels::PixelBuffer;

use crate::gfx::com::GfxError;

/// C# `GpuImageCache.DefaultStripeBytes`: 2 MiB per prefetch stripe.
pub const DEFAULT_STRIPE_BYTES: i32 = 2 * 1024 * 1024;
/// C# `GpuImageCache.DefaultBudgetScreens`: the default budget is this many "screens".
pub const DEFAULT_BUDGET_SCREENS: i64 = 6;
/// C# `MinimumScreenBytes` = `1920L * 1080 * 4`: a screen is never counted smaller than Full HD.
pub const MINIMUM_SCREEN_BYTES: i64 = 1920 * 1080 * 4;
/// C# smallest legal `stripeBytes` (`ThrowIfLessThan(stripeBytes, 4)`).
pub const MIN_STRIPE_BYTES: i32 = 4;
/// `ObjectDisposedException.ObjectName` of the cache (becomes `RenderError::Disposed { object }`).
pub const CACHE_OBJECT_NAME: &str = "PhotoReview.Shell.Rendering.GpuImageCache";
/// `ObjectDisposedException.ObjectName` of a disposed source buffer.
pub const PIXEL_BUFFER_OBJECT_NAME: &str = "PhotoReview.Imaging.Pixels.PixelBuffer";

/// C# public constructor: `DefaultBudgetScreens * Math.Max(MinimumScreenBytes, (long)width * height * 4)`, all in `i64`
/// (surface size in PIXELS: `surface.PixelWidth`, `PixelHeight`). 64x64 gives `6 * 1920 * 1080 * 4`; 2560x1440 gives
/// `6 * 2560 * 1440 * 4`.
#[must_use]
pub fn default_budget_bytes(pixel_width: i32, pixel_height: i32) -> i64 {
    todo!("W8-12f")
}

/// What the cache needs from an image (the Rust form of the members of C# `TiledGpuImage` it touches). Implemented for
/// `pr_app::services::render::tiled::TiledGpuImage<B>` after the layering PR (a local trait on a foreign type is legal); the tests
/// implement it for `TiledGpuImage<()>` or a small fake. All methods take `&self`: the image is shared (`Rc`) with the draw code.
pub trait CachedImage {
    /// `PixelBufferId`.
    fn pixel_buffer_id(&self) -> i64;
    /// `PixelHeight`.
    fn pixel_height(&self) -> i32;
    /// `GpuBytes` of the WHOLE image (partially uploaded images count fully).
    fn gpu_bytes(&self) -> i64;
    /// `UploadedRows`.
    fn uploaded_rows(&self) -> i32;
    /// `UploadedRows = value` (no validation, like C#).
    fn set_uploaded_rows(&self, rows: i32);
    /// `IsComplete`: `uploaded_rows >= pixel_height`.
    fn is_complete(&self) -> bool;
    /// `IsReleased`.
    fn is_released(&self) -> bool;
    /// `IsReleased = value`.
    fn set_released(&self, released: bool);
}

/// C# `IGpuImageBackend` (lines 303-315): the seam between the cache policy and Direct2D. UI thread only; `&self` with interior
/// mutability because the cache holds the backend by value and also subscribes a handler to it.
///
/// RULE: a backend method must never call back into the cache synchronously (the cache holds its state borrowed during `upload`,
/// `upload_rows` and `release`). The device-recreated notification is raised by the surface inside `resize` /
/// `end_draw_and_present`, never inside these three methods.
pub trait GpuImageBackend {
    /// The image type created by [`upload`](Self::upload) (`TiledGpuImage<ID2D1Bitmap1>` in production).
    type Image: CachedImage + 'static;

    /// C# `Upload(pixels, withData)`: creates the tiles; `with_data == false` makes empty textures (a prefetch fills them later with
    /// [`upload_rows`](Self::upload_rows)). `uploaded_rows` of the returned image is 0; the generation is the device's current one.
    fn upload(&self, pixels: &PixelBuffer, with_data: bool) -> Result<Rc<Self::Image>, GfxError>;

    /// C# `UploadRows(image, pixels, startRow, endRow)`: writes the rows `[start_row, end_row)` into every tile that they cut through.
    fn upload_rows(
        &self,
        image: &Self::Image,
        pixels: &PixelBuffer,
        start_row: i32,
        end_row: i32,
    ) -> Result<(), GfxError>;

    /// C# `Release(image)`: gives every tile bitmap back (the cache has already set `is_released`).
    fn release(&self, image: &Self::Image);

    /// C# `event DeviceRecreated +=`: `handler` runs on the UI thread each time the device is recreated. Returns an id for
    /// [`unsubscribe_device_recreated`](Self::unsubscribe_device_recreated).
    fn subscribe_device_recreated(&self, handler: Box<dyn Fn()>) -> u64;

    /// C# `event DeviceRecreated -=`; an unknown id is a no-op.
    fn unsubscribe_device_recreated(&self, id: u64);
}

/// Identifies one scheduled stripe job. `serial` is unique per cache ENTRY (not per id): a stripe posted for an entry that was evicted
/// or cleared must stay inert even when the same `PixelBuffer` id is cached again later (the C# closure captured the `Entry` object).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StripeToken {
    /// `PixelBuffer::id()` of the entry.
    pub pixel_buffer_id: i64,
    /// Entry serial number (increases by one per created entry, never reused).
    pub serial: u64,
}

/// C# `IUiDispatcher.Post(() => RunStripe(entry), UiPriority.Background)` as a seam (D-W8B3S-2). A
/// `pr_core::abstractions::ui::UiAction` is `Send`, the cache is not, so the cache cannot hand a closure to the dispatcher. The
/// implementation (host, W8-13d) posts a `Send` job at `UiPriority::Background` that, on the UI thread, calls
/// `cache.run_stripe(token)` on the cache it can reach (the composition owner).
pub trait StripeScheduler {
    /// Queue ONE background job for `token`. Must not run it synchronously.
    fn schedule_stripe(&self, token: StripeToken);
}

/// Failure of a cache call. Copy-able twin of the `RenderError` variants the C# class throws (converted field by field after the
/// layering PR: `Disposed` -> `RenderError::Disposed`, `NegativeBudget` -> `NegativeBudget`, `Backend` -> `Device`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheError {
    /// `ObjectDisposedException`: `object` is [`CACHE_OBJECT_NAME`] or [`PIXEL_BUFFER_OBJECT_NAME`].
    Disposed { object: &'static str },
    /// `ArgumentOutOfRangeException.ThrowIfNegative(value)` of the budget (constructor or setter).
    NegativeBudget { value: i64 },
    /// `ArgumentOutOfRangeException.ThrowIfLessThan(stripeBytes, 4)`.
    StripeBytesTooSmall { value: i32 },
    /// The backend failed (`ComInterop.Check` threw `COMException` in C#). The cache state is unchanged by the failed step.
    Backend(GfxError),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W8-12f")
    }
}

impl std::error::Error for CacheError {}

impl From<GfxError> for CacheError {
    fn from(error: GfxError) -> Self {
        CacheError::Backend(error)
    }
}

/// An `UploadCompleted` handler (argument = `PixelBuffer::id()`); `Fn`, not `FnMut`, so it can be called after the state borrow is released.
pub type UploadCompletedHandler = Rc<dyn Fn(i64)>;

/// One cached image (C# private `Entry`).
struct Entry<I> {
    serial: u64,
    image: Rc<I>,
    /// LRU stamp: key of `State::order`; larger = more recently used (replaces the C# `LinkedListNode`).
    stamp: u64,
    /// `PendingSource`: the buffer a prefetch still has to upload (an `Arc` clone, D-W8-12a-4). `None` once complete or removed.
    pending_source: Option<Arc<PixelBuffer>>,
}

/// Everything behind the `RefCell`.
struct State<I> {
    entries: HashMap<i64, Entry<I>>,
    /// `stamp -> pixel_buffer_id`, ascending: the FIRST element is the least recently used (C# `_lru` head).
    order: BTreeMap<u64, i64>,
    retained: HashSet<i64>,
    budget_bytes: i64,
    total_gpu_bytes: i64,
    /// C# `_protectedId` (-1 = none). `Option` in Rust: no sentinel.
    protected_id: Option<i64>,
    next_stamp: u64,
    next_serial: u64,
    disposed: bool,
}

/// The budgeted LRU cache (C# `GpuImageCache`). Implements `pr_app::services::render::GpuImageCache` through the W8-12g adapter.
pub struct BudgetedImageCache<B: GpuImageBackend> {
    backend: B,
    scheduler: Rc<dyn StripeScheduler>,
    stripe_bytes: i32,
    state: RefCell<State<B::Image>>,
    /// Subscription id of the device-recreated handler (`None` after dispose).
    device_subscription: Cell<Option<u64>>,
    /// `UploadCompleted` handlers in subscription order.
    upload_completed: RefCell<Vec<(u64, UploadCompletedHandler)>>,
    next_handler_id: Cell<u64>,
}

impl<B: GpuImageBackend> fmt::Debug for BudgetedImageCache<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W8-12f")
    }
}

impl<B: GpuImageBackend> BudgetedImageCache<B> {
    /// C# `internal GpuImageCache(backend, dispatcher, budgetBytes, stripeBytes)`. Validation first (`budget_bytes < 0` ->
    /// `NegativeBudget`, `stripe_bytes < 4` -> `StripeBytesTooSmall`), then `Rc::new_cyclic` and the subscription of
    /// [`on_device_recreated`](Self::on_device_recreated) to the backend through a `Weak` (the handler is a no-op once the cache
    /// is gone).
    pub fn new(
        backend: B,
        scheduler: Rc<dyn StripeScheduler>,
        budget_bytes: i64,
        stripe_bytes: i32,
    ) -> Result<Rc<Self>, CacheError> {
        todo!("W8-12f")
    }

    /// C# `BudgetBytes` getter.
    #[must_use]
    pub fn budget_bytes(&self) -> i64 {
        todo!("W8-12f")
    }

    /// C# `BudgetBytes` setter: negative -> `Err(NegativeBudget { value })` with the budget UNCHANGED; otherwise assigns and evicts at
    /// once (`evict_over_budget(None)`). No disposed check (C# has none).
    pub fn set_budget_bytes(&self, value: i64) -> Result<(), CacheError> {
        todo!("W8-12f")
    }

    /// C# `TotalGpuBytes`: sum of `gpu_bytes` of all cached images, partially uploaded ones included.
    #[must_use]
    pub fn total_gpu_bytes(&self) -> i64 {
        todo!("W8-12f")
    }

    /// C# `Count`.
    #[must_use]
    pub fn count(&self) -> usize {
        todo!("W8-12f")
    }

    /// C# `PendingPrefetchCount`: entries whose image is not complete.
    #[must_use]
    pub fn pending_prefetch_count(&self) -> usize {
        todo!("W8-12f")
    }

    /// C# `Contains(pixelBufferId)`.
    #[must_use]
    pub fn contains(&self, pixel_buffer_id: i64) -> bool {
        todo!("W8-12f")
    }

    /// C# `IsFullyUploaded`: cached AND complete AND not released.
    #[must_use]
    pub fn is_fully_uploaded(&self, pixel_buffer_id: i64) -> bool {
        todo!("W8-12f")
    }

    /// C# `GetOrUpload(pixels)`. Errors in C# order: cache disposed, then `pixels.is_disposed()`. A cached image is completed at once
    /// (`upload_rows(.., pixel_height)` with the buffer given HERE, not the pending source), touched (most recent) and becomes the
    /// protected "current" image; a new buffer is uploaded with data, `uploaded_rows = pixel_height`, becomes current, then added.
    pub fn get_or_upload(&self, pixels: &PixelBuffer) -> Result<Rc<B::Image>, CacheError> {
        todo!("W8-12f")
    }

    /// C# `Prefetch(pixels)`. Cache disposed -> `Err`; a disposed buffer or an already cached id is not an error (the latter only
    /// touches). Otherwise uploads empty textures, adds the entry (protecting ONLY this id during that eviction), stores the `Arc`
    /// as pending source and schedules the first stripe. Does NOT change the protected "current" image.
    pub fn prefetch(&self, pixels: &Arc<PixelBuffer>) -> Result<(), CacheError> {
        todo!("W8-12f")
    }

    /// C# `Retain(ids)`: replaces the retained set, then evicts. Cache disposed -> `Err`.
    pub fn retain(&self, pixel_buffer_ids: &[i64]) -> Result<(), CacheError> {
        todo!("W8-12f")
    }

    /// C# `Clear()`: releases every image (oldest first), empties the cache, resets the protected id. The retained set is KEPT.
    /// Never fails.
    pub fn clear(&self) {
        todo!("W8-12f")
    }

    /// C# `Dispose()`: idempotent; unsubscribes from the backend and clears. Also run by `Drop`.
    pub fn dispose(&self) {
        todo!("W8-12f")
    }

    /// C# `_disposed`.
    #[must_use]
    pub fn is_disposed(&self) -> bool {
        todo!("W8-12f")
    }

    /// C# `OnDeviceRecreated` = `clear()`; public so the host can call it directly and the tests can raise it without a backend event.
    pub fn on_device_recreated(&self) {
        todo!("W8-12f")
    }

    /// C# `RunStripe(entry)` for a job queued by the scheduler. Inert (`Ok`) when the entry is gone, has another serial, is released,
    /// complete or has no pending source, or the cache is disposed. A source that no other owner holds any more
    /// (`Arc::strong_count == 1`) or that `is_disposed()` removes the partial entry (C# "source disposed midway"). Otherwise uploads
    /// `max(1, stripe_bytes / stride)` more rows, reschedules while incomplete and raises `UploadCompleted(id)` after the last stripe.
    /// A backend failure is returned, the entry stays as it was and nothing is rescheduled (C# lets the exception escape the job).
    pub fn run_stripe(&self, token: StripeToken) -> Result<(), CacheError> {
        todo!("W8-12f")
    }

    /// C# `event UploadCompleted +=` (WP-21): raised on the UI thread, after the state borrow is released, once per finished prefetch.
    /// Returns an id for [`unsubscribe_upload_completed`](Self::unsubscribe_upload_completed).
    pub fn subscribe_upload_completed(&self, handler: UploadCompletedHandler) -> u64 {
        todo!("W8-12f")
    }

    /// C# `event UploadCompleted -=`; an unknown id is a no-op.
    pub fn unsubscribe_upload_completed(&self, id: u64) {
        todo!("W8-12f")
    }

    // Private helpers (same names as C#): add, touch, evict_over_budget(also_keep: Option<i64>), remove, release_image,
    // schedule_stripe, upload_rows. See contracts/W8-12f.md "Semantics notes".
}

impl<B: GpuImageBackend> Drop for BudgetedImageCache<B> {
    fn drop(&mut self) {
        // C# `using` = Dispose: the porter calls `self.dispose()` here.
    }
}
