//! Port of `Services/InitialImagePrewarm.cs` (32 lines; C# `internal static class`). Contract: `contracts/W8-04c.md`,
//! rules R-PW-*.
//!
//! P-1 startup: start the viewer decode of the launch file before the presenter asks for it. The presenter later builds the same
//! key and joins this decode (in-flight dedup) or finds it in the RAM cache. Camera RAW files are skipped (their pairing decides
//! which file is shown, and their decode is the costly one).
//!
//! Concurrency (D-W8-2, no async runtime): C# `Task.Run` becomes one short-lived `std::thread` per launch. The key needs a file
//! stat, so the stat runs on that thread, never on the UI thread (C# Q-R29).

#![allow(unused_variables, dead_code)] // D11: stub bodies are todo!(); removed with the last todo!() of this file.

use std::sync::Arc;
use std::thread::JoinHandle;

use pr_core::settings::app_settings::AppSettings;
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
        todo!()
    }

    fn start_viewer_preview(&self, full_path: &str, key: &ImageCacheKey) -> PreviewHandle {
        todo!()
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
        todo!()
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
    todo!()
}
