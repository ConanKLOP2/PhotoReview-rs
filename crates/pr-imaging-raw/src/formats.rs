//! Port of `PhotoReview.Imaging.Raw/{RawFormat, RawFileTypes, RawContainerInfo, RawContainerLimits, RawHeaderErrors}.cs`
//! (C# baseline d92d6aae): the vocabulary every RAW container reader shares.
//!
//! * [`RawFormat`]: container format id (C# enum values kept: `Unknown = 0` .. `Pef = 10`).
//! * [`is_raw_extension`]: the 8 supported RAW extensions (`NRW` and `PEF` are deliberately NOT accepted).
//! * [`RawContainerInfo`]: what a container reader returns (sensor size, orientation, previews, EXIF blocks).
//! * [`limits`]: hostile-input caps shared by all readers (values are part of the hardening tests).
//! * `EmbeddedPreview`, `ExifBlock` and the header error type live in [`crate::contracts`] (D45); `is_io_failure` is
//!   `RawHeaderError::is_io_failure` there.
//!
//! No unsafe, no I/O, no Windows dependency. All sizes keep the C# `int`/`long` width (`i32`/`i64`).
#![allow(unused_variables, dead_code)] // D11: stub bodies are todo!(); the porter removes this line.

use crate::contracts::{EmbeddedPreview, ExifBlock};

/// Camera RAW container format. Discriminants equal the C# enum values.
///
/// `Nrw` and `Pef` have no container reader (LibRaw decodes them); they exist for the LibRaw preview-fallback mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(i32)]
pub enum RawFormat {
    #[default]
    Unknown = 0,
    Cr2 = 1,
    Cr3 = 2,
    Nef = 3,
    Nrw = 4,
    Arw = 5,
    Dng = 6,
    Raf = 7,
    Orf = 8,
    Rw2 = 9,
    Pef = 10,
}

/// The extensions (lower case, with dot) [`is_raw_extension`] accepts, compared `OrdinalIgnoreCase`.
pub const RAW_EXTENSIONS: [&str; 8] = [".cr2", ".cr3", ".nef", ".arw", ".dng", ".raf", ".orf", ".rw2"];

/// `RawFileTypes.IsRawExtension(path)`: `false` for `None`, empty or all-whitespace (`char.IsWhiteSpace`, equal to Rust
/// `char::is_whitespace`); otherwise `pr_core::catalog::types::get_extension(path)` (.NET `Path.GetExtension`, never
/// throws) must be non-empty and equal to one of [`RAW_EXTENSIONS`] under `OrdinalIgnoreCase`
/// (`pr_core::text::eq_ignore_case`, NOT `to_lowercase`: Unicode folds such as the Kelvin sign must not match).
#[must_use]
pub fn is_raw_extension(path: Option<&str>) -> bool {
    todo!()
}

/// Structural metadata parsed from a RAW container header (`RawContainerInfo` record).
///
/// `orientation` is the EXIF value (1 = normal; readers store 1 when unknown); `sensor_*` may be 0 when unknown.
/// C# record equality compares the two lists by reference; the Rust derive compares them by value (no test relies on
/// the C# behaviour).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawContainerInfo {
    pub format: RawFormat,
    pub sensor_width: i32,
    pub sensor_height: i32,
    pub orientation: i32,
    pub previews: Vec<EmbeddedPreview>,
    pub exif_blocks: Vec<ExifBlock>,
}

/// `RawContainerLimits`: hostile-input caps shared by every RAW container reader (OOM / hang / unbounded loop guards).
pub mod limits {
    /// Most IFD structures in a TIFF-based RAW container.
    pub const MAX_IFD_COUNT: i32 = 64;
    /// Most directory entries per IFD.
    pub const MAX_ENTRIES_PER_IFD: i32 = 4096;
    /// Largest embedded preview read into memory: 128 MiB (`128 << 20`). Larger is treated as corrupt before allocating.
    pub const MAX_PREVIEW_BYTES: i32 = 128 << 20;
    /// Hard cap on total bytes read from a header: 8 MiB (`8 << 20`).
    pub const MAX_HEADER_BYTES: i32 = 8 << 20;
    /// Default (minimum) size of the TIFF-header EXIF block: 128 KiB.
    pub const DEFAULT_EXIF_BLOCK_BYTES: i32 = 128 * 1024;
    /// Upper bound of an EXIF block grown to reach IFDs written after the pixel data: 4 MiB (`4 << 20`).
    pub const MAX_EXIF_BLOCK_BYTES: i32 = 4 << 20;
    /// Most JPEG marker segments the bounded JPEG walkers visit (C# `internal`).
    pub const MAX_JPEG_MARKER_SEGMENTS: i32 = 512;
    /// Extra `0xFF` fill bytes before a marker the walkers skip without using a segment iteration (C# `internal`): 64 KiB.
    pub const MAX_JPEG_FILL_BYTES: i32 = 64 * 1024;

    /// `InitialProbeLength(sourceLength)`: `(int)Math.Min(64L, sourceLength)`. Compare in `i64` BEFORE narrowing so a
    /// source longer than `i32::MAX` gives 64 (the point of the helper). A negative length is returned unchanged
    /// (`as i32` truncates like the unchecked C# cast; `i64::MIN as i32` is 0 in both languages).
    #[must_use]
    pub fn initial_probe_length(source_length: i64) -> i32 {
        todo!()
    }
}
