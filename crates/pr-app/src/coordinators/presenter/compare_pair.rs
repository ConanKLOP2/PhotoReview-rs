//! Port of `PhotoReview.App.Shared/Coordinators/ImagePresenter.cs` lines 772 to 791 (`HasComparePair`, `GetComparePair` with its
//! versioned index cache) and 865 to 884 (`LoadComparePreviewAsync`, blocking form), baseline d92d6aae. Contract:
//! `contracts/W8-07k.md`. Target: `crates/pr-app/src/coordinators/presenter/compare_pair.rs`
//! (`coordinators::presenter::compare_pair`).
//!
//! The pair rules themselves (numbered pairs, tie order) are `pr_core::catalog::compare` (W2-06); this file only caches the index
//! against `ReviewCatalog::structural_version` and picks the capture-group pair. The C# `lock (_compareIndexGate)` is not needed:
//! the presenter is UI-thread-only and every method takes `&mut self` (the lock existed because overlapping `PresentAsync` bodies ran
//! on thread-pool threads).

use std::sync::Arc;
use std::time::{Duration, Instant};

use pr_core::catalog::compare::{CompareIndex, build_index};
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::PathError;
use pr_imaging::decode_types::{DecodedImage, DecodedImageOptions, DecodedImageView};
use pr_imaging::service::{PreviewError, PreviewImageService};

use crate::misc::app_log::AppLog;
use crate::viewmodels::compare::try_get_file_size;

use super::state::ImagePresenter;

/// Waits before the single retry of a compare decode the LibRaw gate refused (C# `Func<TimeSpan, CancellationToken, Task>`
/// `_busyRetryDelay`), blocking form: waits `delay` or until the token is cancelled; true when the delay elapsed, false when it
/// was cancelled. Runs on a worker. Default (`default_busy_retry_delay`): sleeps in slices of at most 20 ms and re-checks the token.
/// Tests inject an instant one that counts its calls.
pub type BusyRetryDelay = Arc<dyn Fn(Duration, &CancelToken) -> bool + Send + Sync>;

/// C# `BusyRetryDelay = 250 ms`.
pub const BUSY_RETRY_DELAY: Duration = Duration::from_millis(250);

/// The default [`BusyRetryDelay`].
pub fn default_busy_retry_delay() -> BusyRetryDelay {
    /// Longest the wait sleeps before it looks at the token again (a `CancelToken` has no wake-up).
    const SLICE: Duration = Duration::from_millis(20);
    Arc::new(|delay: Duration, cancel: &CancelToken| {
        let deadline = Instant::now().checked_add(delay);
        loop {
            if cancel.is_canceled() {
                return false;
            }
            let remaining = match deadline {
                Some(deadline) => deadline.saturating_duration_since(Instant::now()),
                None => SLICE,
            };
            if remaining.is_zero() {
                return true;
            }
            std::thread::sleep(remaining.min(SLICE));
        }
    })
}

/// C# `_compareIndexVersion` / `_compareIndex`: `ComparePairService.BuildIndex(catalog.Paths)` cached against
/// `ReviewCatalog.StructuralVersion` (rebuilt only when membership or order changed: folder load, file action, reorder), so a
/// navigation does not pay O(n log n).
#[derive(Debug)]
pub struct ComparePairCache {
    /// C# `-1` initially (`StructuralVersion` starts at 0, so the first lookup always builds).
    version: i32,
    index: Option<CompareIndex>,
}

impl ComparePairCache {
    /// Empty cache (`version = -1`, no index).
    pub fn new() -> Self {
        Self {
            version: -1,
            index: None,
        }
    }
}

impl Default for ComparePairCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ImagePresenter {
    /// C# `HasComparePair(path)` = `GetComparePair(path) is not null` with the default `includeCaptureGroup = true`.
    /// `Err` only when `pr_core::catalog::compare::build_index` rejects a catalog path (C# lets the exception escape).
    pub fn has_compare_pair(&mut self, catalog: &ReviewCatalog, path: &str) -> Result<bool, PathError> {
        Ok(self.get_compare_pair(catalog, path, true)?.is_some())
    }

    /// C# `GetComparePair(path, includeCaptureGroup)`:
    /// 1. `include_capture_group` and `catalog.find(path)` has a capture group: `Some((group.jpeg_path(), group.raw_path()))` (always JPEG first, RAW second; the
    ///    group is found by ANY member path because `ReviewCatalog::find` resolves members).
    /// 2. else the cached `CompareIndex` (rebuilt first when `catalog.structural_version() != cache.version`): `index.get(path)` cloned (lookup
    ///    OrdinalIgnoreCase). Note the cache is consulted for a capture-group entry too when `include_capture_group` is false.
    pub(super) fn get_compare_pair(
        &mut self,
        catalog: &ReviewCatalog,
        path: &str,
        include_capture_group: bool,
    ) -> Result<Option<(String, String)>, PathError> {
        if include_capture_group {
            if let Some(group) = catalog.find(path).and_then(|entry| entry.capture_group.as_ref()) {
                return Ok(Some((group.jpeg_path().to_owned(), group.raw_path().to_owned())));
            }
        }
        let cache = &mut self.compare_cache;
        if cache.version != catalog.structural_version() || cache.index.is_none() {
            let index = build_index(&catalog.paths())?;
            cache.index = Some(index);
            cache.version = catalog.structural_version();
        }
        Ok(cache.index.as_ref().and_then(|index| index.get(path)).cloned())
    }
}

/// C# `LoadComparePreviewAsync(path, cancellationToken)`, blocking (runs on a worker): the compare member's preview at VIEWER priority
/// (`service.get_viewer_preview(path, &service.get_current_cache_key(path)?, cancel, Duration::ZERO)` then `.wait()`), returning the
/// preview's platform image. A `PreviewError::is_busy()` result on attempt 0 is retried ONCE after `delay(BUSY_RETRY_DELAY, cancel)`
/// (when the delay reports cancelled: `Err(PreviewError::Canceled)`); a second busy result and every other error are returned.
/// A key failure (`get_current_cache_key` is `Err`) is returned as `PreviewError::Key`.
pub(super) fn load_compare_preview_blocking(
    service: &PreviewImageService,
    path: &str,
    cancel: &CancelToken,
    delay: &BusyRetryDelay,
) -> Result<Arc<DecodedImage>, PreviewError> {
    for attempt in 0..2 {
        // The key is rebuilt for the path on every attempt, exactly like C#.
        let key = service.get_current_cache_key(path).map_err(PreviewError::Key)?;
        match service.get_viewer_preview(path, &key, cancel, Duration::ZERO).wait() {
            Ok(preview) => return decoded_image_of(preview.as_ref()),
            Err(error) if error.is_busy() && attempt == 0 => {
                // A DecoderBusy (possibly inherited from a preload decode this request joined) is retried once after a short, cancellable pause.
                if !delay(BUSY_RETRY_DELAY, cancel) {
                    return Err(PreviewError::Canceled);
                }
            }
            Err(error) => return Err(error),
        }
    }
    // Unreachable: the second attempt always returns above.
    Err(PreviewError::Canceled)
}

/// The compare view model keeps `DecodedImage`s: copies the shape of a preview (same platform image, sizes and EXIF).
fn decoded_image_of(view: &dyn DecodedImageView) -> Result<Arc<DecodedImage>, PreviewError> {
    let options = DecodedImageOptions {
        downscaled: view.downscaled(),
        orientation: view.orientation(),
        actual_backend: view.actual_backend(),
        original_width: view.original_width(),
        original_height: view.original_height(),
        exif: view.exif().cloned(),
        is_degraded_fallback: view.is_degraded_fallback(),
    };
    DecodedImage::new(
        view.platform_image().clone(),
        view.pixel_width(),
        view.pixel_height(),
        view.estimated_bytes(),
        options,
    )
    .map(Arc::new)
    .map_err(|error| PreviewError::Panicked(format!("the compare preview could not be wrapped: {error}")))
}

/// C# `CompareViewModel.TryGetFileSize(path)` (the presenter passes no `getFileSize`, so the view model stats both files off the UI
/// thread): `fi.Exists ? fi.Length : null` through `pr_core::io::physical::PhysicalFileSystem` (NOT the presenter's injected file system: C# uses
/// `FileInfo` directly). Expected failures (`ErrorKind::PermissionDenied`, `Unsupported`, `InvalidInput`, other I/O) are logged with
/// `AppLog` (`Compare file size unavailable path=..`) and give `None`; the size suffix is cosmetic. Blocking: runs on a worker.
/// W8-06c may expose the same function; keep one of them (open question O1 of the W8-07j contract).
pub(super) fn compare_file_size(path: &str) -> Option<i64> {
    try_get_file_size(path, &PhysicalFileSystem, AppLog::global())
}
