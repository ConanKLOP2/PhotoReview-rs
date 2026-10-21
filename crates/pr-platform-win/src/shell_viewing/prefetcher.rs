//! Port of `PhotoReview.Shell.Win32/Viewing/PresentationPrefetcher.cs` (181 lines, WP-21, baseline `d92d6aae`). Unit W8-16d (contract `contracts/W8-16d.md`).
//!
//! `GpuImageCache::prefetch` of the NEXT images in the direction of travel. The preview of a neighbour is decoded into the RAM cache by the preload
//! controller (not in step with navigation), so each navigation runs one pass at once and three retry passes (`RETRY_DELAYS_MS`) until the buffers
//! exist; a pass of an older navigation is dropped (generation). The RAM-cache lookup (it stats files) runs OFF the UI thread; only `prefetch` (UI
//! affine) runs on it, and that uploads stripe by stripe at Background priority (WP-15), so it never blocks the UI. The ids it planned feed
//! `GpuImageCache::retain` so the LRU does not evict them.
//!
//! Threading shape (D-W8B4V-5): the prefetcher is `!Send` and never touches a thread or a timer. The C# `IPrefetchHost.RunBackground` / `PostToUi` /
//! `ScheduleOnUi` become two requests the HOST fulfils ([`PrefetchHost::start_lookup`], [`PrefetchHost::schedule_retry`]) and two calls the host makes back
//! on the UI thread ([`PresentationPrefetcher::apply_lookup`], [`PresentationPrefetcher::run_retry`]); the data crossing threads are plain `Send` values
//! ([`LookupRequest`], `Vec<Option<Arc<PixelBuffer>>>`).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use pr_app::services::render::GpuImageCache;
use pr_app::services::render::RenderError;
use pr_imaging::pixels::PixelBuffer;

use crate::render::image_cache::upload::{BitmapDevice, D2dGpuImageCache};

/// C# `PresentationPrefetcher.Ahead`: images prefetched in the direction of travel.
pub const AHEAD: usize = 2;

/// C# `PresentationPrefetcher.Behind`: images prefetched against it.
pub const BEHIND: usize = 1;

/// C# `RetryDelaysMs` = `[120, 400, 1000]`: the retry passes after a navigation.
pub const RETRY_DELAYS_MS: [u64; 3] = [120, 400, 1000];

/// The two members of the C# `GpuImageCache` the prefetcher uses beyond `IGpuImageCache` (`Contains`) plus `Prefetch`, as a narrow trait so the
/// prefetcher is testable with a fake. `pr_app`'s `GpuImageCache` / `PrefetchStatus` have no `contains` (the real cache has it on the policy type).
pub trait PrefetchCache {
    /// C# `Contains(pixelBufferId)`: the image is cached (complete or still uploading).
    fn contains(&self, pixel_buffer_id: i64) -> bool;

    /// C# `Prefetch(pixels)`: upload in background stripes; a no-op when already cached; a disposed buffer is ignored.
    fn prefetch(&self, pixels: &Arc<PixelBuffer>) -> Result<(), RenderError>;
}

impl<D: BitmapDevice + 'static> PrefetchCache for D2dGpuImageCache<D> {
    fn contains(&self, pixel_buffer_id: i64) -> bool {
        self.inner().contains(pixel_buffer_id)
    }

    fn prefetch(&self, pixels: &Arc<PixelBuffer>) -> Result<(), RenderError> {
        GpuImageCache::prefetch(self, pixels)
    }
}

/// One RAM-cache lookup the host must run OFF the UI thread: C# `IPrefetchHost.LookupCached(paths)` inside `RunBackground`. When it finishes the host
/// posts `apply_lookup(generation, found)` to the UI thread (Background priority, skipped when the window is gone). `found[i]` belongs to `paths[i]`;
/// `None` when the preview is not decoded yet or the file vanished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupRequest {
    pub generation: u64,
    pub paths: Vec<String>,
}

/// One retry pass the host must schedule: after the delay it calls `run_retry(request)` on the UI thread (skipped when the window is gone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryRequest {
    pub generation: u64,
    pub index: i32,
    pub count: i32,
}

/// C# `IPrefetchHost`, reshaped (see the module doc). UI thread only.
pub trait PrefetchHost {
    /// C# `PathAt(index)`: the path of the catalog entry, `None` outside the catalog.
    fn path_at(&self, index: i32) -> Option<String>;

    /// C# `RunBackground(() => PostToUi(Apply(LookupCached(paths))))`: see [`LookupRequest`]. Must not run the lookup inline.
    fn start_lookup(&self, request: LookupRequest);

    /// C# `ScheduleOnUi(delay, pass)`: see [`RetryRequest`].
    fn schedule_retry(&self, delay: Duration, request: RetryRequest);
}

/// C# `Plan(count, index, direction, ahead, behind)`: candidate indexes by priority, `ahead` images in the direction of travel (`direction < 0` is
/// backwards, anything else forwards) then `behind` the other way; indexes outside `0..count` are dropped (no wrap-around).
pub fn plan(count: i32, index: i32, direction: i32, ahead: usize, behind: usize) -> Vec<i32> {
    let mut result = Vec::new();
    let step: i64 = if direction < 0 { -1 } else { 1 };
    for (limit, sign) in [(ahead, step), (behind, -step)] {
        for offset in 1..=limit {
            let Ok(offset) = i64::try_from(offset) else { break };
            let candidate = i64::from(index).saturating_add(sign.saturating_mul(offset));
            if candidate >= 0 && candidate < i64::from(count) {
                if let Ok(candidate) = i32::try_from(candidate) {
                    result.push(candidate);
                }
            }
        }
    }
    result
}

/// C# `PresentationPrefetcher`. Share it as `Rc<PresentationPrefetcher>`; every method takes `&self`.
pub struct PresentationPrefetcher {
    cache: Rc<dyn PrefetchCache>,
    host: Rc<dyn PrefetchHost>,
    planned: RefCell<HashSet<i64>>,
    last_index: Cell<i32>,
    direction: Cell<i32>,
    generation: Cell<u64>,
    prefetch_calls: Cell<i32>,
    next_handler: Cell<u64>,
    handlers: RefCell<BTreeMap<u64, Rc<dyn Fn()>>>,
}

impl std::fmt::Debug for PresentationPrefetcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresentationPrefetcher").finish_non_exhaustive()
    }
}

impl PresentationPrefetcher {
    /// C# constructor `(cache, host)`: direction +1, `last_index = -1`, generation 0.
    pub fn new(cache: Rc<dyn PrefetchCache>, host: Rc<dyn PrefetchHost>) -> Self {
        Self {
            cache,
            host,
            planned: RefCell::new(HashSet::new()),
            last_index: Cell::new(-1),
            direction: Cell::new(1),
            generation: Cell::new(0),
            prefetch_calls: Cell::new(0),
            next_handler: Cell::new(1),
            handlers: RefCell::new(BTreeMap::new()),
        }
    }

    /// C# `Direction`: +1 forwards, -1 backwards (kept while standing still).
    pub fn direction(&self) -> i32 {
        self.direction.get()
    }

    /// C# `PlannedIds`: the buffer ids requested by the latest applied pass (for `GpuImageCache::retain`).
    pub fn planned_ids(&self) -> Vec<i64> {
        self.planned.borrow().iter().copied().collect()
    }

    /// C# `PrefetchCalls`: how many `prefetch` calls were made (diagnostics, tests).
    pub fn prefetch_calls(&self) -> i32 {
        self.prefetch_calls.get()
    }

    /// C# `PlanChanged +=`: raised after each applying pass so the window updates `retain`.
    pub fn subscribe_plan_changed(&self, handler: Rc<dyn Fn()>) -> u64 {
        let id = self.next_handler.get();
        self.next_handler.set(id.wrapping_add(1));
        self.handlers.borrow_mut().insert(id, handler);
        id
    }

    /// C# `PlanChanged -=`; an unknown id is a no-op.
    pub fn unsubscribe_plan_changed(&self, id: u64) {
        self.handlers.borrow_mut().remove(&id);
    }

    /// C# `Navigated(index, count)` (UI thread): the current image (catalog index) changed. `index < 0 || count <= 0` clears the plan, forgets the last
    /// index and bumps the generation (every pass in flight is dropped) WITHOUT raising `PlanChanged`. Otherwise the direction is the sign of
    /// `index - last_index` (kept when equal or on the first call), the generation is bumped, one pass runs now and `RETRY_DELAYS_MS` retry passes are
    /// scheduled with [`PrefetchHost::schedule_retry`].
    pub fn navigated(&self, index: i32, count: i32) {
        if index < 0 || count <= 0 {
            self.planned.borrow_mut().clear();
            self.last_index.set(-1);
            self.generation.set(self.generation.get().wrapping_add(1));
            return;
        }
        let last = self.last_index.get();
        if last >= 0 && index != last {
            self.direction.set(if index > last { 1 } else { -1 });
        }
        self.last_index.set(index);
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        self.pass(index, count, generation);
        for delay in RETRY_DELAYS_MS {
            self.host.schedule_retry(
                Duration::from_millis(delay),
                RetryRequest {
                    generation,
                    index,
                    count,
                },
            );
        }
    }

    /// The host calls this on the UI thread when a scheduled retry is due. Runs a pass only when `request.generation` is still current (the user has
    /// not navigated since).
    pub fn run_retry(&self, request: RetryRequest) {
        if request.generation == self.generation.get() {
            self.pass(request.index, request.count, request.generation);
        }
    }

    /// The host calls this on the UI thread with the result of a [`LookupRequest`] (C# `Apply(found)` inside the posted action). Ignored when
    /// `generation` is no longer current. Otherwise: clear the planned ids; for each `Some` buffer that is not disposed add its id and, when
    /// `contains(id)` is false, `prefetch` it (`prefetch_calls += 1`); then raise `PlanChanged`. The first `prefetch` error stops the pass and is
    /// returned (planned ids so far stay, no `PlanChanged`), exactly like the C# exception leaving the posted action.
    pub fn apply_lookup(&self, generation: u64, found: &[Option<Arc<PixelBuffer>>]) -> Result<(), RenderError> {
        if generation != self.generation.get() {
            return Ok(());
        }
        self.planned.borrow_mut().clear();
        for pixels in found.iter().flatten() {
            if pixels.is_disposed() {
                continue;
            }
            let id = pixels.id();
            self.planned.borrow_mut().insert(id);
            if !self.cache.contains(id) {
                self.cache.prefetch(pixels)?;
                self.prefetch_calls.set(self.prefetch_calls.get().saturating_add(1));
            }
        }
        let handlers: Vec<_> = self.handlers.borrow().values().cloned().collect();
        for handler in handlers {
            handler();
        }
        Ok(())
    }

    fn pass(&self, index: i32, count: i32, generation: u64) {
        let paths: Vec<_> = plan(count, index, self.direction.get(), AHEAD, BEHIND)
            .into_iter()
            .filter_map(|candidate| self.host.path_at(candidate))
            .collect();
        if !paths.is_empty() {
            self.host.start_lookup(LookupRequest { generation, paths });
        }
    }
}
