//! Port of `PhotoReview.App.Shared/Coordinators/ImagePresenter.cs` lines 772 to 791 (`HasComparePair`, `GetComparePair` with its
//! versioned index cache) and 865 to 884 (`LoadComparePreviewAsync`, blocking form), baseline d92d6aae. Contract:
//! `contracts/W8-07k.md`. Target: `crates/pr-app/src/coordinators/presenter/compare_pair.rs`
//! (`coordinators::presenter::compare_pair`).
//!
//! The pair rules themselves (numbered pairs, tie order) are `pr_core::catalog::compare` (W2-06); this file only caches the index
//! against `ReviewCatalog::structural_version` and picks the capture-group pair. The C# `lock (_compareIndexGate)` is not needed:
//! the presenter is UI-thread-only and every method takes `&mut self` (the lock existed because overlapping `PresentAsync` bodies ran
//! on thread-pool threads).
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use std::sync::Arc;
use std::time::Duration;

use pr_core::catalog::compare::CompareIndex;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::PathError;
use pr_imaging::decode_types::PlatformImage;
use pr_imaging::service::{PreviewError, PreviewImageService};

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
    todo!()
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
        todo!()
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
        todo!()
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
) -> Result<PlatformImage, PreviewError> {
    todo!()
}

/// C# `CompareViewModel.TryGetFileSize(path)` (the presenter passes no `getFileSize`, so the view model stats both files off the UI
/// thread): `fi.Exists ? fi.Length : null` through `pr_core::io::physical::PhysicalFileSystem` (NOT the presenter's injected file system: C# uses
/// `FileInfo` directly). Expected failures (`ErrorKind::PermissionDenied`, `Unsupported`, `InvalidInput`, other I/O) are logged with
/// `AppLog` (`Compare file size unavailable path=..`) and give `None`; the size suffix is cosmetic. Blocking: runs on a worker.
/// W8-06c may expose the same function; keep one of them (open question O1 of the W8-07j contract).
pub(super) fn compare_file_size(path: &str) -> Option<i64> {
    todo!()
}
