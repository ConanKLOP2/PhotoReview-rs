//! Port of `PhotoReview.Imaging/ImageCacheKey.cs` (C# baseline d92d6aae): [`ImageCacheKey`] (identity of a decoded
//! bitmap: normalized full path, source length and write time, decode mode and box, orientation, backend, source kind)
//! and [`ImageSourceKind`] (named values of `SourceKind`). PARITY: see contracts/drafts-w4/W4-01a.
//!
//! Target path: `crates/pr-imaging/src/cache_key.rs` (lead adds `pub mod cache_key;` to `lib.rs`).
//!
//! The key is a value type used as a map key (RAM cache, source-bytes cache, preload set) and its FIELDS feed the
//! persisted disk-cache file name (`PreviewImageService.GetDiskCachePath`, unit W4-14c): the field values, the
//! upper-cased path and the `u8` source kind are therefore part of an on-disk format. The key has NO text form of
//! its own; do not add one here.
#![allow(unused_variables, dead_code)] // D11: only while a body is `todo!()`; the porter removes this line.

use std::fmt;
use std::io;

use pr_core::abstractions::{FileStat, FileSystem};
use pr_core::catalog::types::CatalogEntry;
use pr_core::model::DecoderBackend;
use pr_core::text::dotnet_path::PathError;

use crate::decode_types::DecodeBox;

/// Named values of [`ImageCacheKey::source_kind`] (C# static class `ImageSourceKind`; the key keeps a `u8` for
/// compatibility, and the value is written into the disk-cache identity as `kind<N>`).
#[derive(Debug, Clone, Copy)]
pub struct ImageSourceKind;

impl ImageSourceKind {
    /// An ordinary image file.
    pub const STANDARD: u8 = 0;
    /// A camera RAW container decoded through its embedded preview.
    pub const RAW_PREVIEW: u8 = 1;
    /// A camera RAW container decoded at full resolution by the LibRaw full decoder; never RAM- or disk-cached.
    pub const RAW_FULL_DECODE: u8 = 2;
}

/// The three optional trailing arguments every C# `Create` overload has (`orientationApplied = true`,
/// `backend = DecoderBackend.Wpf`, `sourceKind = 0`). `Default` reproduces the C# defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyOptions {
    pub orientation_applied: bool,
    pub backend: DecoderBackend,
    pub source_kind: u8,
}

impl Default for KeyOptions {
    fn default() -> Self {
        Self {
            orientation_applied: true,
            backend: DecoderBackend::Wpf,
            source_kind: ImageSourceKind::STANDARD,
        }
    }
}

/// Why a key could not be built (C# throws). W4-01b turns imaging exceptions into one error enum: the lead may fold
/// this into it, the variants stay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheKeyError {
    /// C# `FileNotFoundException("Image source no longer exists", fullName)` wrapped by `UserFacingError.Localized`
    /// with catalog key `err.io.sourceNoLongerExists` (`{path}` = `full_name`). Also raised when the path names a
    /// directory (`FileInfo.Exists` is false for directories). `full_name` is the full path BEFORE upper-casing.
    SourceNoLongerExists { full_name: String },
    /// `Path.GetFullPath` / `new FileInfo(path)` rejected the path: `ArgumentException` (empty, only spaces, NUL) or
    /// `PathTooLongException`.
    InvalidPath(PathError),
}

impl fmt::Display for CacheKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // The English `Exception.Message` (for logs); the UI text comes from the catalog.
            Self::SourceNoLongerExists { .. } => f.write_str("Image source no longer exists"),
            Self::InvalidPath(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for CacheKeyError {}

impl From<CacheKeyError> for io::Error {
    /// `SourceNoLongerExists` is C# `FileNotFoundException` (an `IOException`): kind `NotFound`; `TooLong` is an
    /// `IOException` too (kind `Other`); `Argument` is `InvalidInput`. Callers that need the localized sentence keep
    /// the [`CacheKeyError`] itself.
    fn from(err: CacheKeyError) -> Self {
        let kind = match &err {
            CacheKeyError::SourceNoLongerExists { .. } => io::ErrorKind::NotFound,
            CacheKeyError::InvalidPath(PathError::TooLong) => io::ErrorKind::Other,
            CacheKeyError::InvalidPath(PathError::Argument) => io::ErrorKind::InvalidInput,
        };
        io::Error::new(kind, err)
    }
}

/// Identity of a decoded bitmap, including its source version and decode quality (C# `readonly record struct
/// ImageCacheKey`). The fields are private: [`create`](Self::create) and friends are the only way to build a valid
/// value, so the full-path / upper-case normalization cannot be bypassed.
///
/// Equality and hashing are over every field (C# record-struct equality; the `TargetBox` property is derived from
/// `target_width`/`target_height`, not a field). Use as a `HashMap` key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageCacheKey {
    path: String,
    length: i64,
    last_write_utc_ticks: i64,
    is_original: bool,
    target_width: i32,
    target_height: i32,
    orientation_applied: bool,
    backend: DecoderBackend,
    source_kind: u8,
}

impl ImageCacheKey {
    /// Full path, upper-cased with `ToUpperInvariant` (ordinal-comparable; NOT the on-disk spelling).
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Source length in bytes (C# `long`).
    pub fn length(&self) -> i64 {
        self.length
    }

    /// Source last-write time in UTC .NET ticks.
    pub fn last_write_utc_ticks(&self) -> i64 {
        self.last_write_utc_ticks
    }

    pub fn is_original(&self) -> bool {
        self.is_original
    }

    /// Decode box width (displayed pixels); 0 = unconstrained. Always 0 for Original.
    pub fn target_width(&self) -> i32 {
        self.target_width
    }

    /// Decode box height (displayed pixels); 0 = unconstrained (width-only). Always 0 for Original.
    pub fn target_height(&self) -> i32 {
        self.target_height
    }

    pub fn orientation_applied(&self) -> bool {
        self.orientation_applied
    }

    pub fn backend(&self) -> DecoderBackend {
        self.backend
    }

    /// One of [`ImageSourceKind`]'s values (any `u8` is representable).
    pub fn source_kind(&self) -> u8 {
        self.source_kind
    }

    /// C# `TargetBox`: `new DecodeBox(TargetWidth, TargetHeight)` (`UNBOUNDED` for Original).
    pub fn target_box(&self) -> DecodeBox {
        DecodeBox {
            width: self.target_width,
            height: self.target_height,
        }
    }

    /// C# private constructor: stores the values as given except that Original mode forces the box to 0 x 0
    /// ("Original mode always decodes at full size, whatever box the caller passed").
    fn new(
        path: String,
        length: i64,
        last_write_utc_ticks: i64,
        is_original: bool,
        target_box: DecodeBox,
        options: KeyOptions,
    ) -> Self {
        todo!("Original => width/height 0, else the box as given; no clamping of negative sides")
    }

    /// C# `Create(string path, bool isOriginal, int targetWidth, ...)`: width-only key (height 0 = unconstrained),
    /// i.e. `create(fs, path, is_original, DecodeBox { width: target_width, height: 0 }, options)`.
    pub fn create_width_only(
        fs: &dyn FileSystem,
        path: &str,
        is_original: bool,
        target_width: i32,
        options: KeyOptions,
    ) -> Result<Self, CacheKeyError> {
        todo!("delegate to create with a width-only box")
    }

    /// C# `Create(string path, bool isOriginal, DecodeBox box, ...)`: `new FileInfo(path)` (full path via
    /// `get_full_path`, errors become [`CacheKeyError::InvalidPath`]) then the stat overload. ONE `get_file_stat` call.
    pub fn create(
        fs: &dyn FileSystem,
        path: &str,
        is_original: bool,
        target_box: DecodeBox,
        options: KeyOptions,
    ) -> Result<Self, CacheKeyError> {
        todo!("get_full_path(path), fs.get_file_stat(Path::new(&full)), create_from_stat")
    }

    /// C# `Create(FileInfo info, ...)`: reuse a stat the caller already took. `full_name` is `FileInfo.FullName` (an
    /// already full path); `stat` is `None` when the file does not exist (or is a directory). `None` =>
    /// [`CacheKeyError::SourceNoLongerExists`] with `full_name`. Otherwise the key path is
    /// `to_upper_invariant(get_full_path(full_name))`, length = `stat.length` saturated to `i64::MAX`, ticks =
    /// `stat.last_write_utc.ticks()`.
    pub fn create_from_stat(
        full_name: &str,
        stat: Option<FileStat>,
        is_original: bool,
        target_box: DecodeBox,
        options: KeyOptions,
    ) -> Result<Self, CacheKeyError> {
        todo!("None => SourceNoLongerExists; else normalize path and build")
    }

    /// C# `Create(CatalogEntry entry, ...)`: when `entry.length` or `entry.last_write_utc` is `None` fall back to
    /// [`create`](Self::create) on `entry.path()`; otherwise build from the entry's own length and ticks WITHOUT
    /// touching the file system (no existence check) after `to_upper_invariant(get_full_path(entry.path()))`.
    pub fn create_for_entry(
        fs: &dyn FileSystem,
        entry: &CatalogEntry,
        is_original: bool,
        target_box: DecodeBox,
        options: KeyOptions,
    ) -> Result<Self, CacheKeyError> {
        todo!("entry fast path vs create fallback")
    }

    /// C# `CreateOriginal(ImageCacheKey source)`: same source identity (path, length, ticks, orientation, backend,
    /// source kind), `is_original = true`, box 0 x 0. No file-system access.
    pub fn create_original(source: &ImageCacheKey) -> Self {
        todo!("clone source with is_original = true and box unbounded")
    }

    /// C# `CreateOriginal(ImageCacheKey source, byte sourceKind)`: as above but with `source_kind` replacing the
    /// source's kind (used for the RAW full decode representation).
    pub fn create_original_with_kind(source: &ImageCacheKey, source_kind: u8) -> Self {
        todo!("clone source with is_original = true, box unbounded, kind replaced")
    }

    /// C# `MatchesCurrentSource()`: the file still exists with the same length and write ticks. `false` when the
    /// file is missing, is a directory, or cannot be statted (C# catches `IOException` and
    /// `UnauthorizedAccessException`; `get_file_stat` returns `None` for those). Stats `self.path()` (the UPPER-CASED
    /// path: Windows file systems are case-insensitive, so it resolves).
    pub fn matches_current_source(&self, fs: &dyn FileSystem) -> bool {
        todo!("fs.get_file_stat(Path::new(&self.path)) compared on length and ticks")
    }
}

/// The key's own path normalisation: `Path.GetFullPath(path)` then `ToUpperInvariant` (the text `ImageCacheKey` stores
/// in `path`). `evict_cached_path` uses it so an eviction normalises exactly like the key (W4-14a OQ2, D34).
///
/// # Errors
/// [`PathError`] when the path is blank or contains NUL (C# `ArgumentException`).
pub fn normalize_key_path(path: &str) -> Result<String, PathError> {
    todo!("W4-01a")
}
