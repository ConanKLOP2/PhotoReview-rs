//! Port of `PhotoReview.Core/Catalog/{ImageFileTypes,CatalogEntry,CaptureGroup,LatestExplorerSnapshot}.cs`
//! (C# baseline d92d6aae). `ExplorerOrderTypes` is already ported in [`super::explorer_types`]; `ExplorerReason`
//! lives in [`super::explorer_reason`].
//!
//! Paths in this module are `String`/`&str` exactly like the C# `string` paths (they are compared and stored as
//! text, never touched on disk here). Convert with `Path::new(&entry.path())` at the file-system boundary.
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use crate::abstractions::FileStat;
use crate::model::RawPairMode;
use crate::text::{eq_ignore_case, ignore_case_key};
use crate::time::DateTimeUtc;

use super::explorer_types::ExplorerViewSnapshot;

// ---------------------------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------------------------

/// A constructor or method argument was rejected (C# `ArgumentException` family).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentError {
    /// `ArgumentException`: the string argument was null/empty/whitespace-only.
    BlankPath { param_name: &'static str },
    /// `ArgumentOutOfRangeException`: the argument value is not allowed for this operation.
    OutOfRange { param_name: &'static str },
}

impl fmt::Display for ArgumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArgumentError::BlankPath { param_name } => write!(f, "{param_name} cannot be null or whitespace."),
            ArgumentError::OutOfRange { param_name } => write!(f, "{param_name} is out of range."),
        }
    }
}

impl std::error::Error for ArgumentError {}

// ---------------------------------------------------------------------------------------------------------------
// ImageFileTypes
// ---------------------------------------------------------------------------------------------------------------

/// `ImageFileTypes.SupportedExtensions` (compared OrdinalIgnoreCase). Order is not significant.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[".jpg", ".jpeg", ".png", ".bmp", ".gif", ".tif", ".tiff"];

/// `ImageFileTypes.JpegExtensions`: the only extensions that form the JPEG side of a capture group.
pub const JPEG_EXTENSIONS: &[&str] = &[".jpg", ".jpeg"];

/// `ImageFileTypes.RawExtensions`: exactly the eight approved RAW formats.
pub const RAW_EXTENSIONS: &[&str] = &[".cr2", ".cr3", ".nef", ".arw", ".dng", ".raf", ".orf", ".rw2"];

/// `ImageFileTypes.WebpHeicExtensions` (Q-FMT-WEBP-HEIC): listed only when WebP/HEIC support is enabled; decoded through WIC.
pub const WEBP_HEIC_EXTENSIONS: &[&str] = &[".webp", ".heic", ".heif"];

/// C# `WicImageFormat`: an image format decoded only through an optional Windows (WIC) codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WicImageFormat {
    /// Not one of the optional-codec formats.
    #[default]
    None,
    /// WebP (`.webp`).
    WebP,
    /// HEIC/HEIF (`.heic`, `.heif`).
    Heif,
}

/// .NET `Path.GetExtension(path)` on Windows: scanning from the END of `path`, stop at the first `'\\'` or `'/'`
/// (a directory separator; `':'` is NOT a stop) and return `""`; the first `'.'` found returns the text from that dot
/// to the end, unless the dot is the last char (then `""`). No `'.'` at all gives `""`. The result keeps the original
/// case and includes the dot. Examples: `"a.JPG"` -> `".JPG"`; `".jpg"` -> `".jpg"`; `"a."` -> `""`;
/// `"a..jpg"` -> `".jpg"`; `"C:\\d.jpg\\f"` -> `""`; `"C:f.jpg"` -> `".jpg"`; `"a.jpg "` -> `".jpg "`.
/// Pinned by golden `image-file-types` (field `extension`).
pub fn get_extension(path: &str) -> &str {
    // Scan from the end: a '.' ends the search (the extension starts there unless it is the last char), a directory
    // separator stops it without a result. ':' is not a stop (.NET Path.GetExtension on Windows).
    for (i, ch) in path.char_indices().rev() {
        if ch == '.' {
            return if i + 1 == path.len() { "" } else { &path[i..] };
        }
        if ch == '\\' || ch == '/' {
            break;
        }
    }
    ""
}

/// True when `ext` (with its dot) is in `list`, compared with `crate::text::eq_ignore_case`.
pub fn extension_in(list: &[&str], ext: &str) -> bool {
    list.iter().any(|candidate| eq_ignore_case(candidate, ext))
}

/// `ImageFileTypes.IsRawPath`: true when [`get_extension`] of `path` is one of [`RAW_EXTENSIONS`]
/// (OrdinalIgnoreCase). `""` is false. (C# also accepts a null string and returns false: callers holding an
/// `Option<&str>` use `path.is_some_and(is_raw_path)`.)
pub fn is_raw_path(path: &str) -> bool {
    extension_in(RAW_EXTENSIONS, get_extension(path))
}

/// `ImageFileTypes.GetWicFormat`: which optional-codec format `path` is, by [`get_extension`] (OrdinalIgnoreCase).
pub fn get_wic_format(path: &str) -> WicImageFormat {
    let ext = get_extension(path);
    if eq_ignore_case(ext, ".webp") {
        WicImageFormat::WebP
    } else if eq_ignore_case(ext, ".heic") || eq_ignore_case(ext, ".heif") {
        WicImageFormat::Heif
    } else {
        WicImageFormat::None
    }
}

/// `ImageFileTypes.IsSupported(path)` = `is_supported_with_raw(path, false)`.
pub fn is_supported(path: &str) -> bool {
    is_supported_with_raw(path, false)
}

/// `ImageFileTypes.IsSupported(path, rawEnabled)`: false when `path` is empty or only whitespace
/// (`char::is_whitespace` matches .NET `IsNullOrWhiteSpace`); false when the extension is empty; true when the
/// extension is in [`SUPPORTED_EXTENSIONS`]; otherwise `raw_enabled && ` extension in [`RAW_EXTENSIONS`].
pub fn is_supported_with_raw(path: &str, raw_enabled: bool) -> bool {
    is_supported_with(path, raw_enabled, false)
}

/// `ImageFileTypes.IsSupported(path, rawEnabled, webpHeicEnabled)`: [`is_supported_with_raw`] plus, when
/// `webp_heic_enabled`, the [`WEBP_HEIC_EXTENSIONS`] (Q-FMT-WEBP-HEIC).
pub fn is_supported_with(path: &str, raw_enabled: bool, webp_heic_enabled: bool) -> bool {
    if path.trim().is_empty() {
        return false;
    }
    let ext = get_extension(path);
    if ext.is_empty() {
        return false;
    }
    if extension_in(SUPPORTED_EXTENSIONS, ext) {
        return true;
    }
    if raw_enabled && extension_in(RAW_EXTENSIONS, ext) {
        return true;
    }
    webp_heic_enabled && extension_in(WEBP_HEIC_EXTENSIONS, ext)
}

// ---------------------------------------------------------------------------------------------------------------
// CaptureGroup
// ---------------------------------------------------------------------------------------------------------------

/// A same-name JPEG+RAW capture and its optional XMP sidecar. Immutable once built.
///
/// Equality and hashing use ONLY the three paths, compared OrdinalIgnoreCase (`crate::text`): two groups whose paths
/// differ only in case are equal and hash equally. `xmp_path` `None` equals only `None`.
#[derive(Debug, Clone)]
pub struct CaptureGroup {
    jpeg_path: String,
    raw_path: String,
    xmp_path: Option<String>,
    /// `[jpeg, raw]` or `[jpeg, raw, xmp]` (C# `Paths`); the first two entries are `ImagePaths`.
    paths: Vec<String>,
}

impl CaptureGroup {
    /// C# constructor. `jpeg_path` and `raw_path` must not be empty or whitespace-only; `xmp_path`, when `Some`,
    /// must not be either (`Some("")` is an error, not "no sidecar"). Errors: [`ArgumentError::BlankPath`] with
    /// `param_name` `"jpegPath"`, `"rawPath"`, `"xmpPath"` (checked in that order).
    pub fn new(
        jpeg_path: impl Into<String>,
        raw_path: impl Into<String>,
        xmp_path: Option<String>,
    ) -> Result<Self, ArgumentError> {
        let jpeg_path: String = jpeg_path.into();
        let raw_path: String = raw_path.into();
        if jpeg_path.trim().is_empty() {
            return Err(ArgumentError::BlankPath { param_name: "jpegPath" });
        }
        if raw_path.trim().is_empty() {
            return Err(ArgumentError::BlankPath { param_name: "rawPath" });
        }
        if let Some(xmp) = &xmp_path {
            if xmp.trim().is_empty() {
                return Err(ArgumentError::BlankPath { param_name: "xmpPath" });
            }
        }
        let mut paths = vec![jpeg_path.clone(), raw_path.clone()];
        if let Some(xmp) = &xmp_path {
            paths.push(xmp.clone());
        }
        Ok(Self {
            jpeg_path,
            raw_path,
            xmp_path,
            paths,
        })
    }

    pub fn jpeg_path(&self) -> &str {
        &self.jpeg_path
    }

    pub fn raw_path(&self) -> &str {
        &self.raw_path
    }

    pub fn xmp_path(&self) -> Option<&str> {
        self.xmp_path.as_deref()
    }

    /// `GetRepresentativePath`: `PreferJpeg` => JPEG path, `PreferRaw` => RAW path, `Separate` =>
    /// `Err(ArgumentError::OutOfRange { param_name: "mode" })` ("Separate mode does not create capture groups").
    pub fn get_representative_path(&self, mode: RawPairMode) -> Result<&str, ArgumentError> {
        match mode {
            RawPairMode::PreferJpeg => Ok(&self.jpeg_path),
            RawPairMode::PreferRaw => Ok(&self.raw_path),
            RawPairMode::Separate => Err(ArgumentError::OutOfRange { param_name: "mode" }),
        }
    }

    /// `ImagePaths`: `[jpeg, raw]`.
    pub fn image_paths(&self) -> &[String] {
        // `paths` always starts with the JPEG and RAW paths (checked in `new`), so the first two entries exist.
        &self.paths[..2]
    }

    /// `Paths`: `[jpeg, raw]` plus the sidecar when there is one (always in this order).
    pub fn paths(&self) -> &[String] {
        &self.paths
    }
}

impl PartialEq for CaptureGroup {
    /// Three paths, OrdinalIgnoreCase (`xmp_path` None == None only).
    fn eq(&self, other: &Self) -> bool {
        let xmp_equal = match (&self.xmp_path, &other.xmp_path) {
            (None, None) => true,
            (Some(a), Some(b)) => eq_ignore_case(a, b),
            _ => false,
        };
        eq_ignore_case(&self.jpeg_path, &other.jpeg_path)
            && eq_ignore_case(&self.raw_path, &other.raw_path)
            && xmp_equal
    }
}

impl Eq for CaptureGroup {}

impl Hash for CaptureGroup {
    /// Must agree with `eq`: hash `crate::text::ignore_case_key` of the JPEG path, of the RAW path and of the sidecar
    /// (`Option` of the key, so `None` and `Some("")` hash differently).
    fn hash<H: Hasher>(&self, state: &mut H) {
        ignore_case_key(&self.jpeg_path).hash(state);
        ignore_case_key(&self.raw_path).hash(state);
        self.xmp_path.as_deref().map(ignore_case_key).hash(state);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// CatalogEntry
// ---------------------------------------------------------------------------------------------------------------

/// A single image item in a review catalog (C# record `CatalogEntry`).
///
/// `path` is private so it can never become blank: it is validated by [`CatalogEntry::new`] and
/// [`CatalogEntry::with_path`]. The other fields are plain data. Derived equality is the C# record equality (every
/// field; `path` compared exactly, case-sensitive; `capture_group` by its own OrdinalIgnoreCase equality).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    path: String,
    /// File size in bytes, when known.
    pub length: Option<i64>,
    pub last_write_utc: Option<DateTimeUtc>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub capture_group: Option<CaptureGroup>,
}

impl CatalogEntry {
    /// C# `new CatalogEntry(path)`: all other fields `None`. Errors: [`ArgumentError::BlankPath`] (`"path"`) when
    /// `path` is empty or whitespace-only (`str::trim().is_empty()`).
    pub fn new(path: impl Into<String>) -> Result<Self, ArgumentError> {
        let path = require_path(path.into())?;
        Ok(Self {
            path,
            length: None,
            last_write_utc: None,
            width: None,
            height: None,
            capture_group: None,
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// C# `entry with { Path = value }`: same validation as [`CatalogEntry::new`]; keeps every other field.
    pub fn with_path(self, path: impl Into<String>) -> Result<Self, ArgumentError> {
        let path = require_path(path.into())?;
        Ok(Self { path, ..self })
    }

    /// `Matches(FileStat)`: true when `length` is `Some` and equals `stat.length`, and `last_write_utc` is `Some` and
    /// equals `stat.last_write_utc` (exact tick equality). A `None` field never matches. `stat.length` above
    /// `i64::MAX` never matches.
    pub fn matches(&self, stat: &FileStat) -> bool {
        let length_matches = match (self.length, i64::try_from(stat.length)) {
            (Some(known), Ok(actual)) => known == actual,
            _ => false,
        };
        length_matches && self.last_write_utc == Some(stat.last_write_utc)
    }

    /// `WithMetadata`: records a fresh stat. Returns a copy with `length` and `last_write_utc` set and:
    /// `unchanged` = (`self.length == Some(length)` and `self.last_write_utc == Some(last_write_utc)`, computed from
    /// the OLD values);
    /// `width` = the supplied `width` if `Some`, else the old width when `unchanged`, else `None`; same for `height`.
    /// The path and capture group are kept.
    pub fn with_metadata(
        &self,
        length: i64,
        last_write_utc: DateTimeUtc,
        width: Option<i32>,
        height: Option<i32>,
    ) -> CatalogEntry {
        let unchanged = self.length == Some(length) && self.last_write_utc == Some(last_write_utc);
        CatalogEntry {
            path: self.path.clone(),
            length: Some(length),
            last_write_utc: Some(last_write_utc),
            width: width.or(if unchanged { self.width } else { None }),
            height: height.or(if unchanged { self.height } else { None }),
            capture_group: self.capture_group.clone(),
        }
    }
}

/// C# `RequirePath`: a path that is empty or whitespace-only is an error; otherwise it is kept exactly as given.
fn require_path(path: String) -> Result<String, ArgumentError> {
    if path.trim().is_empty() {
        Err(ArgumentError::BlankPath { param_name: "path" })
    } else {
        Ok(path)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// LatestExplorerSnapshot
// ---------------------------------------------------------------------------------------------------------------

/// Stand-in for C# `Task<ExplorerViewSnapshot>`: a shared, write-once completion cell (R0003: no async runtime).
/// The producer (Explorer query thread) calls [`complete`](Self::complete) or [`fail`](Self::fail) at most once;
/// clones share the same cell. The state is: pending (nothing stored), completed successfully (`Some(snapshot)`),
/// or failed/canceled (`None` stored).
#[derive(Debug, Clone, Default)]
pub struct ExplorerQueryHandle {
    cell: Arc<OnceLock<Option<Arc<ExplorerViewSnapshot>>>>,
}

impl ExplorerQueryHandle {
    /// A new pending handle.
    pub fn new() -> Self {
        Self {
            cell: Arc::new(OnceLock::new()),
        }
    }

    /// Stores the snapshot. Returns false (and changes nothing) when the handle was already completed or failed.
    pub fn complete(&self, snapshot: ExplorerViewSnapshot) -> bool {
        self.cell.set(Some(Arc::new(snapshot))).is_ok()
    }

    /// Marks the query failed or canceled. Returns false when it was already completed or failed.
    pub fn fail(&self) -> bool {
        self.cell.set(None).is_ok()
    }

    /// The snapshot when the query completed successfully; `None` while pending, failed or canceled.
    pub fn snapshot(&self) -> Option<Arc<ExplorerViewSnapshot>> {
        self.cell.get().cloned().flatten()
    }
}

/// Keeps the Explorer-order query of the folder load that is (or was last) running, so the Diagnostics window can
/// show the real result without a second query. Nothing is read from disk or Explorer here. Thread-safe.
#[derive(Debug, Default)]
pub struct LatestExplorerSnapshot {
    handle: Mutex<Option<ExplorerQueryHandle>>,
}

impl LatestExplorerSnapshot {
    pub fn new() -> Self {
        Self {
            handle: Mutex::new(None),
        }
    }

    /// `Set`: records the query of a new load; `None` when that load does not use the Explorer order. Replaces the
    /// previous one. Lock poisoning is recovered (`PoisonError::into_inner`), never `unwrap()`.
    pub fn set(&self, handle: Option<ExplorerQueryHandle>) {
        let mut slot: MutexGuard<'_, Option<ExplorerQueryHandle>> =
            self.handle.lock().unwrap_or_else(PoisonError::into_inner);
        *slot = handle;
    }

    /// `Current`: the completed snapshot of the latest load, or `None` when none was recorded, it is still running,
    /// it failed or it was canceled. Never blocks. Do not hold the lock while calling into the handle.
    pub fn current(&self) -> Option<Arc<ExplorerViewSnapshot>> {
        // Clone the handle out, release the lock, then read the cell.
        let handle = self.handle.lock().unwrap_or_else(PoisonError::into_inner).clone()?;
        handle.snapshot()
    }
}
