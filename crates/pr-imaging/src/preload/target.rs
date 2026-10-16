//! Port of `PhotoReview.Imaging/Preload/IPreloadTarget.cs` (C# baseline d92d6aae, 38 lines) and of the public
//! `DelegateMemoryProbe` class at the end of `Preload/PreloadScheduler.cs` (lines 960-966). Contract:
//! `contracts/W4-15a.md` (draft: `contracts/drafts-w4/W4-15a/contract.md`). PARITY I-11.
//!
//! Target path: `crates/pr-imaging/src/preload/target.rs`.
//!
//! [`PreloadTarget`] is the seam between the preload scheduler (W4-15b) and the preview cache that it warms
//! (`PreviewImageService`, W4-14, implements it; tests use fakes). Every method is BLOCKING (R0003: no async runtime);
//! [`PreloadTarget::preload`] is only ever called on a preload worker thread, never on the thread that called
//! `PreloadScheduler::preload_around` (the UI thread in the app). All other methods may be called on any thread,
//! including the UI thread, and must be cheap (a RAM-cache lookup, or a key built from the scan's metadata).

use std::error::Error;
use std::fmt;
use std::io;

use pr_core::abstractions::{MemoryProbe, MemorySnapshot};
use pr_core::catalog::types::CatalogEntry;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::PathError;

use crate::cache_key::{CacheKeyError, ImageCacheKey}; // W4-01a
use crate::decoder_traits::DecodeError; // W4-01c

/// Why the target could not build a cache key (the exceptions C# `GetCurrentCacheKey` can throw).
///
/// The scheduler has a dedicated `catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)` arm at
/// both of its key lookups; every other exception escapes them. The two variants keep that split (trap 9: one arm per
/// C# catch, never merged).
#[derive(Debug)]
pub enum KeyError {
    /// C# `IOException` (incl. `FileNotFoundException`, `PathTooLongException`) or `UnauthorizedAccessException`
    /// (`io::ErrorKind::PermissionDenied`). The scheduler skips the candidate with a warning, or uses an unbounded box
    /// for the preload centre.
    Io(io::Error),
    /// Any other exception (C# `ArgumentException` for a malformed path, a bug in the target, ...). NOT caught by the
    /// key arms: the run ends with `"Preload scheduler failed"` (W4-15b).
    Other(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyError::Io(error) => fmt::Display::fmt(error, f),
            KeyError::Other(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl Error for KeyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            KeyError::Io(error) => Some(error),
            KeyError::Other(error) => Some(error.as_ref()),
        }
    }
}

impl From<io::Error> for KeyError {
    fn from(error: io::Error) -> Self {
        KeyError::Io(error)
    }
}

impl From<CacheKeyError> for KeyError {
    /// `SourceNoLongerExists` (C# `FileNotFoundException`) and `InvalidPath(TooLong)` (C# `PathTooLongException`) are
    /// `IOException`s => [`KeyError::Io`] (via `From<CacheKeyError> for io::Error`, W4-01a); `InvalidPath(Argument)`
    /// (C# `ArgumentException`) => [`KeyError::Other`].
    fn from(error: CacheKeyError) -> Self {
        match error {
            CacheKeyError::InvalidPath(PathError::Argument) => KeyError::Other(Box::new(error)),
            CacheKeyError::SourceNoLongerExists { .. } | CacheKeyError::InvalidPath(PathError::TooLong) => {
                KeyError::Io(io::Error::from(error))
            }
        }
    }
}

/// Abstraction of the preview cache that the preload scheduler warms and queries (C# `IPreloadTarget`).
///
/// The three default methods mirror the C# default interface members (targets without a viewer, real sizes or a disk
/// cache, i.e. test fakes, keep the defaults).
pub trait PreloadTarget: Send + Sync {
    /// C# `TryGetCachedPreview(string path)`: the preview of `path` at the current box is in the RAM cache. Not used by
    /// the scheduler itself (it always has a key); kept for parity of the interface.
    fn try_get_cached_preview_for_path(&self, path: &str) -> bool;

    /// C# `TryGetCachedPreview(ImageCacheKey key)`: `key` is in the RAM cache. Cheap; called on the scan thread for
    /// every candidate examined.
    fn try_get_cached_preview(&self, key: &ImageCacheKey) -> bool;

    /// C# `Task PreloadAsync(string path, CancellationToken)` awaited to completion: decode `path` into the RAM cache
    /// (joining a decode already in flight for it). BLOCKING; called on a preload worker thread only. `Ok(())` does not
    /// mean the preview is cached (the scheduler re-checks with a fresh key). Errors are mapped by the scheduler one
    /// arm per C# catch (W4-15b): `Busy`, `Canceled` (foreign or own token), `MissingCodec`, `Io` (and the other
    /// `IOException`-like variants), anything else.
    fn preload(&self, path: &str, cancel: &CancelToken) -> Result<(), DecodeError>;

    /// C# `GetCurrentCacheKey(string path)`: stats the file (a real target re-reads length and write time). Called by
    /// the worker AFTER `preload` to learn the identity the decode actually stored (INV-1).
    fn get_current_cache_key_for_path(&self, path: &str) -> Result<ImageCacheKey, KeyError>;

    /// C# `GetCurrentCacheKey(CatalogEntry entry)`: reuses the entry's `length`/`last_write_utc` from the folder scan
    /// instead of stat-ing the path (no disk access when both are known).
    fn get_current_cache_key(&self, entry: &CatalogEntry) -> Result<ImageCacheKey, KeyError>;

    /// C# `CacheCount`: previews in the RAM cache (log lines only).
    fn cache_count(&self) -> i32;

    /// C# `CacheBytes`: bytes held by the RAM preview cache (whole-folder fill limit, resume verdict, log lines).
    fn cache_bytes(&self) -> i64;

    /// C# `ActiveViewerDecodes` (default 0): viewer decodes (the image being navigated to) running right now. While
    /// non-zero, preload holds back to its viewer-busy worker limit.
    fn active_viewer_decodes(&self) -> i32 {
        0
    }

    /// C# `CachedPreviewBytes(ImageCacheKey)` (default `None`): decoded size of the cached preview for `key`, `None`
    /// when not cached. Feeds the measured whole-folder estimate (Q-R17, `PreviewSizeSampler`).
    fn cached_preview_bytes(&self, _key: &ImageCacheKey) -> Option<i64> {
        None
    }

    /// C# `HasDiskCachedPreview(ImageCacheKey)` (default `false`): the on-disk preview cache holds `key`, so a decode
    /// reads that instead of the original and the source-bytes prefetch is skipped.
    fn has_disk_cached_preview(&self, _key: &ImageCacheKey) -> bool {
        false
    }
}

/// A memory probe answering `has_headroom` with a delegate of the maximum load (C# public `DelegateMemoryProbe`).
/// The reserve is ignored; the snapshot is the constant `{ load_percent: 50, available_bytes: 16 GiB }`.
pub struct DelegateMemoryProbe {
    has_headroom: Box<dyn Fn(f64) -> bool + Send + Sync>,
}

impl DelegateMemoryProbe {
    /// C# `new DelegateMemoryProbe(Func<double, bool> hasHeadroom)`.
    pub fn new(has_headroom: impl Fn(f64) -> bool + Send + Sync + 'static) -> Self {
        Self {
            has_headroom: Box::new(has_headroom),
        }
    }
}

impl fmt::Debug for DelegateMemoryProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DelegateMemoryProbe").finish_non_exhaustive()
    }
}

impl MemoryProbe for DelegateMemoryProbe {
    /// `has_headroom(maximum_load)`; `reserve_bytes` is ignored (as in C#).
    fn has_headroom(&self, maximum_load: f64, _reserve_bytes: i64) -> bool {
        (self.has_headroom)(maximum_load)
    }

    /// Always `Some(MemorySnapshot { load_percent: 50, available_bytes: 16 * 1024 * 1024 * 1024 })`.
    fn get_snapshot(&self) -> Option<MemorySnapshot> {
        Some(MemorySnapshot {
            load_percent: 50,
            available_bytes: 16 * 1024 * 1024 * 1024,
        })
    }
}
