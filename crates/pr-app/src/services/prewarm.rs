//! Port of `Services/InitialImagePrewarm.cs` (32 lines; C# `internal static class`). Contract: `contracts/W8-04c.md`,
//! rules R-PW-*.
//!
//! P-1 startup: start the viewer decode of the launch file before the presenter asks for it. The presenter later builds the same
//! key and joins this decode (in-flight dedup) or finds it in the RAM cache. Camera RAW files are skipped (their pairing decides
//! which file is shown, and their decode is the costly one).
//!
//! Concurrency (D-W8-2, no async runtime): C# `Task.Run` becomes one short-lived `std::thread` per launch. The key needs a file
//! stat, so the stat runs on that thread, never on the UI thread (C# Q-R29).

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use pr_core::catalog::types::is_supported_with;
use pr_core::settings::app_settings::AppSettings;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::get_full_path;
use pr_imaging::cache_key::{CacheKeyError, ImageCacheKey};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::service::PreviewImageService;
use pr_imaging::service::handle::PreviewHandle;

/// The two calls of the C# `PreviewImageService` the prewarm makes. `PreviewImageService` implements it (below); the unit tests use
/// a fake that records the calls. `Send + Sync`: the prewarm thread holds an `Arc` of it.
pub trait PrewarmPreviews: Send + Sync {
    /// C# `GetCurrentCacheKey(fullPath, box)` when `target_box` is `Some`, else `GetCurrentCacheKey(fullPath)` (the service then reads
    /// its own target box). Performs one file stat: call it only from the prewarm thread.
    fn current_cache_key(&self, full_path: &str, target_box: Option<DecodeBox>)
    -> Result<ImageCacheKey, CacheKeyError>;

    /// C# `GetViewerPreviewAsync(fullPath, key, CancellationToken.None)`: no start delay, never cancelled.
    fn start_viewer_preview(&self, full_path: &str, key: &ImageCacheKey) -> PreviewHandle;
}

impl PrewarmPreviews for PreviewImageService {
    fn current_cache_key(
        &self,
        full_path: &str,
        target_box: Option<DecodeBox>,
    ) -> Result<ImageCacheKey, CacheKeyError> {
        match target_box {
            Some(target_box) => self.get_current_cache_key_for_box(full_path, target_box),
            None => self.get_current_cache_key(full_path),
        }
    }

    fn start_viewer_preview(&self, full_path: &str, key: &ImageCacheKey) -> PreviewHandle {
        // C# GetViewerPreviewAsync(fullPath, key, CancellationToken.None): no start delay, never cancelled.
        self.get_viewer_preview(full_path, key, &CancelToken::none(), Duration::ZERO)
    }
}

/// The started prewarm (C# `Task`). `wait` blocks until the viewer decode has finished (tests join deterministically; the app
/// never waits). A key failure (unreadable or damaged file) ends the thread silently (R-PW-5); a panicking thread is reported to
/// the caller of `wait` only, never propagated.
#[derive(Debug)]
pub struct PrewarmTask {
    join: JoinHandle<()>,
}

impl PrewarmTask {
    /// Joins the prewarm thread: `Ok(())` when it ended normally, `Err` when it panicked.
    pub fn wait(self) -> std::thread::Result<()> {
        self.join.join()
    }
}

/// C# `InitialImagePrewarm.Start(previews, settings, path, targetBox)`.
///
/// `previews` `None` = no service (C# null). `path` `None` or blank = nothing. `target_box` `None` = the service's current viewport
/// box. Returns `None` when nothing was started (R-PW-1..R-PW-4), else the started prewarm.
pub fn start(
    previews: Option<Arc<dyn PrewarmPreviews>>,
    settings: &AppSettings,
    path: Option<&str>,
    target_box: Option<DecodeBox>,
) -> Option<PrewarmTask> {
    // R-PW-1: no service, nothing to start.
    let previews = previews?;
    // R-PW-2: blank text (IsNullOrWhiteSpace) starts nothing.
    let path = path.filter(|text| !text.trim().is_empty())?;
    // R-PW-3: the RAW rule (rawEnabled: false); WebP follows the setting.
    if !is_supported_with(path, false, settings.webp_heic_support_enabled) {
        return None;
    }
    // R-PW-4 / R-PW-6: Path.GetFullPath (never std::path::absolute, T-7). A rejected path starts nothing.
    // DIVERGENCE: C# lets the exception out of the launch code; the presenter's own request still decodes.
    let full_path = get_full_path(path).ok()?;
    // R-PW-5 / T-10: the key needs a file stat, so the whole prewarm runs on its own thread, never on the caller's.
    let join = std::thread::Builder::new()
        .name("initial-image-prewarm".to_owned())
        .spawn(move || {
            let Ok(key) = previews.current_cache_key(&full_path, target_box) else {
                return;
            };
            // The fault of a failed decode is observed and ignored (C# unobserved task exception).
            let _ = previews.start_viewer_preview(&full_path, &key).wait();
        })
        .ok()?;
    Some(PrewarmTask { join })
}
