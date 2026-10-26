//! Port of `PhotoReview.Imaging/ImageCacheKey.cs` (C# baseline d92d6aae): [`ImageCacheKey`] (identity of a decoded
//! bitmap: normalized full path, source length and write time, decode mode and box, orientation, backend, source kind)
//! and [`ImageSourceKind`] (named values of `SourceKind`). PARITY: see contracts/drafts-w4/W4-01a.
//! W9-R4 ports the `ImageCacheKey.WithPath` delta from the same C# source at baseline `7eaee2a2`.
//!
//! Target path: `crates/pr-imaging/src/cache_key.rs` (lead adds `pub mod cache_key;` to `lib.rs`).
//!
//! The key is a value type used as a map key (RAM cache, source-bytes cache, preload set) and its FIELDS feed the
//! persisted disk-cache file name (`PreviewImageService.GetDiskCachePath`, unit W4-14c): the field values, the
//! upper-cased path and the `u8` source kind are therefore part of an on-disk format. The key has NO text form of
//! its own; do not add one here.

use std::fmt;
use std::io;
use std::path::Path;

use pr_core::abstractions::{FileStat, FileSystem};
use pr_core::catalog::types::CatalogEntry;
use pr_core::model::DecoderBackend;
use pr_core::session::store::SessionStore;
use pr_core::text::dotnet_path::{PathError, get_full_path};

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
    /// FEAT-RENAME: copy this identity under a newly normalized path without statting the renamed file.
    pub fn with_path(&self, new_path: &str) -> Result<Self, PathError> {
        let path = normalize_key_path(new_path)?;
        Ok(Self { path, ..self.clone() })
    }
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
        // Original decodes at full size whatever box the caller passed; a preview keeps the box as given.
        let (target_width, target_height) = if is_original {
            (0, 0)
        } else {
            (target_box.width, target_box.height)
        };
        Self {
            path,
            length,
            last_write_utc_ticks,
            is_original,
            target_width,
            target_height,
            orientation_applied: options.orientation_applied,
            backend: options.backend,
            source_kind: options.source_kind,
        }
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
        Self::create(fs, path, is_original, DecodeBox::new(target_width, 0), options)
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
        let full = get_full_path(path).map_err(CacheKeyError::InvalidPath)?;
        let stat = fs.get_file_stat(Path::new(&full));
        Self::create_from_stat(&full, stat, is_original, target_box, options)
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
        let stat = stat.ok_or_else(|| CacheKeyError::SourceNoLongerExists {
            full_name: full_name.to_owned(),
        })?;
        let path = normalize_key_path(full_name).map_err(CacheKeyError::InvalidPath)?;
        let length = i64::try_from(stat.length).unwrap_or(i64::MAX);
        Ok(Self::new(
            path,
            length,
            stat.last_write_utc.ticks(),
            is_original,
            target_box,
            options,
        ))
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
        match (entry.length, entry.last_write_utc) {
            // Both known: the entry's own stat is the identity; no file-system access, no existence check.
            (Some(length), Some(last_write)) => {
                let path = normalize_key_path(entry.path()).map_err(CacheKeyError::InvalidPath)?;
                Ok(Self::new(
                    path,
                    length,
                    last_write.ticks(),
                    is_original,
                    target_box,
                    options,
                ))
            }
            // Either unknown: stat the disk, exactly like a plain path.
            _ => Self::create(fs, entry.path(), is_original, target_box, options),
        }
    }

    /// C# `CreateOriginal(ImageCacheKey source)`: same source identity (path, length, ticks, orientation, backend,
    /// source kind), `is_original = true`, box 0 x 0. No file-system access.
    pub fn create_original(source: &ImageCacheKey) -> Self {
        Self {
            is_original: true,
            target_width: 0,
            target_height: 0,
            ..source.clone()
        }
    }

    /// C# `CreateOriginal(ImageCacheKey source, byte sourceKind)`: as above but with `source_kind` replacing the
    /// source's kind (used for the RAW full decode representation).
    pub fn create_original_with_kind(source: &ImageCacheKey, source_kind: u8) -> Self {
        Self {
            source_kind,
            ..Self::create_original(source)
        }
    }

    /// C# `MatchesCurrentSource()`: the file still exists with the same length and write ticks. `false` when the
    /// file is missing, is a directory, or cannot be statted (C# catches `IOException` and
    /// `UnauthorizedAccessException`; `get_file_stat` returns `None` for those). Stats `self.path()` (the UPPER-CASED
    /// path: Windows file systems are case-insensitive, so it resolves).
    pub fn matches_current_source(&self, fs: &dyn FileSystem) -> bool {
        match fs.get_file_stat(Path::new(&self.path)) {
            Some(stat) => {
                i64::try_from(stat.length).unwrap_or(i64::MAX) == self.length
                    && stat.last_write_utc.ticks() == self.last_write_utc_ticks
            }
            None => false,
        }
    }
}

/// The key's own path normalisation: `Path.GetFullPath(path)` then `ToUpperInvariant` (the text `ImageCacheKey` stores
/// in `path`). `evict_cached_path` uses it so an eviction normalises exactly like the key (W4-14a OQ2, D34).
///
/// # Errors
/// [`PathError`] when the path is blank or contains NUL (C# `ArgumentException`).
pub fn normalize_key_path(path: &str) -> Result<String, PathError> {
    get_full_path(path).map(|full| SessionStore::to_upper_invariant(&full))
}

#[cfg(test)]
mod rename_tests {
    use super::*;
    use pr_core::model::DecoderBackend;
    use pr_test_support::{FakeFileSystem, FsOp, TempRoot};

    // C#: ImageCacheKeyTests.WithPath_NewNameOnNonexistentFile_KeepsIdentityAndNormalizesPath
    #[test]
    fn with_path_new_name_on_nonexistent_file_keeps_identity_and_normalizes_path() {
        let root = TempRoot::new("cache-key-with-path");
        let source = root.join("source.jpg");
        let target = root.join("sub").join("..").join("renamed.jpg");
        let source_text = source.to_string_lossy().into_owned();
        let target_text = target.to_string_lossy().into_owned();
        let fs = FakeFileSystem::new();
        fs.add_file(&source, &[1, 2, 3]);
        let key = ImageCacheKey::create(
            &fs,
            &source_text,
            false,
            DecodeBox::new(2304, 1280),
            KeyOptions {
                orientation_applied: false,
                backend: DecoderBackend::WicDirect,
                source_kind: ImageSourceKind::RAW_PREVIEW,
            },
        )
        .expect("source key");

        let renamed = key.with_path(&target_text).expect("renamed key");
        assert_eq!(
            normalize_key_path(&target_text).expect("normalized path"),
            renamed.path()
        );
        assert_eq!(key.length(), renamed.length());
        assert_eq!(key.last_write_utc_ticks(), renamed.last_write_utc_ticks());
        assert_eq!(key.is_original(), renamed.is_original());
        assert_eq!(key.target_box(), renamed.target_box());
        assert_eq!(key.orientation_applied(), renamed.orientation_applied());
        assert_eq!(key.backend(), renamed.backend());
        assert_eq!(key.source_kind(), renamed.source_kind());
        assert_ne!(key, renamed);
        assert_eq!(fs.count(FsOp::Stat), 1);
    }

    // C#: ImageCacheKeyTests.WithPath_OriginalKey_CreateOriginalOfNewKeyHasNewPath
    #[test]
    fn with_path_original_key_create_original_of_new_key_has_new_path() {
        let root = TempRoot::new("cache-key-original-with-path");
        let source = root.join("source.jpg");
        let target = root.join("renamed.jpg");
        let source_text = source.to_string_lossy().into_owned();
        let target_text = target.to_string_lossy().into_owned();
        let fs = FakeFileSystem::new();
        fs.add_file(&source, &[1]);
        let preview =
            ImageCacheKey::create_width_only(&fs, &source_text, false, 900, KeyOptions::default()).expect("source key");
        let original = ImageCacheKey::create_original(&preview.with_path(&target_text).expect("renamed preview"));
        let renamed_original = ImageCacheKey::create_original(&preview)
            .with_path(&target_text)
            .expect("renamed original");
        assert_eq!(
            normalize_key_path(&target_text).expect("normalized path"),
            original.path()
        );
        assert!(original.is_original());
        assert_eq!(original, renamed_original);
    }

    // C#: ImageCacheKeyTests.WithPath_BlankPath_Throws
    #[test]
    fn with_path_blank_path_throws() {
        let root = TempRoot::new("cache-key-blank-with-path");
        let source = root.join("source.jpg");
        let source_text = source.to_string_lossy().into_owned();
        let fs = FakeFileSystem::new();
        fs.add_file(&source, &[1]);
        let key =
            ImageCacheKey::create_width_only(&fs, &source_text, false, 900, KeyOptions::default()).expect("source key");
        assert_eq!(key.with_path(" "), Err(PathError::Argument));
    }
}
