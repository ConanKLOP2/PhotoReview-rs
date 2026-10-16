//! Port of `PhotoReview.Imaging.Raw/{EmbeddedPreview, ExifBlock, IRawContainerReader, IRawHeaderSource,
//! IRawPreviewFallback}.cs` (C# baseline d92d6aae) plus the shared header-error type and the in-memory header source
//! (`tests/PhotoReview.TestSupport.Windows/Fixtures/InMemoryRawHeaderSource.cs`) that nearly every RAW test uses.
//!
//! Shapes only: the container readers (W5-03..10), `PreviewSelector`/`RawExif` (W5-11) and `SourceRawHeaderSource`/
//! `RawDecoder` (W5-12) plug into these. The C# exception contract of `IRawHeaderSource`/`IRawContainerReader`
//! ("`InvalidDataException` only, `OperationCanceledException` never wrapped") becomes ONE error enum,
//! [`RawHeaderError`]; the C# `RawHeaderErrors.IsIoFailure` helper (W5-01a) is a method on it
//! ([`RawHeaderError::is_io_failure`]) so both units share the type without a cycle.

#![allow(unused_variables, dead_code)] // D11: removed together with the last `todo!()`.

use std::collections::HashSet;
use std::fmt;
use std::io;

use pr_core::sync::{CancelToken, Canceled};
use pr_imaging::decoder_traits::DecodeError;

// Defined by W5-01a (`RawFormat`, `RawContainerInfo`); re-exported at the crate root by the lead (see mod-lines.txt).
use crate::{RawContainerInfo, RawFormat};

/// `SourceRawHeaderSource.BlockSize`: header reads are charged to the budget in whole 64 KiB-aligned blocks.
/// Defined here (not in W5-12) so the in-memory source below needs no dependency on the file-backed one.
pub const RAW_HEADER_BLOCK_SIZE: usize = 64 * 1024;

/// `EmbeddedPreviewKind`. Only JPEG exists; the enum is kept so `kind` checks in `PreviewSelector`/`RawDecoder` port 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmbeddedPreviewKind {
    Jpeg,
}

/// `PreviewColorSpace` (declaration order = C# numeric values 0, 1, 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreviewColorSpace {
    Unknown,
    Srgb,
    AdobeRgb,
}

/// `EmbeddedPreview` record: an embedded preview image within a camera RAW container. C# `int`/`long` stay `i32`/`i64`
/// (sentinels such as `width == 0` and wrap-around checks depend on the width). `PartialEq`/`Eq`/`Hash` are the C#
/// record's value equality; `with { .. }` is struct-update syntax on a clone (`PreviewSelector` uses it).
///
/// `width`/`height` may be 0 (unknown until the JPEG SOF header is parsed). `header_resolved` records that
/// `PreviewSelector` already walked the JPEG header for this preview: dimensions and colour space are then final.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EmbeddedPreview {
    pub index: i32,
    pub offset: i64,
    pub length: i64,
    pub kind: EmbeddedPreviewKind,
    pub width: i32,
    pub height: i32,
    pub color_space: PreviewColorSpace,
    pub header_resolved: bool,
}

impl EmbeddedPreview {
    /// `new EmbeddedPreview(index, offset, length, kind, width, height, colorSpace)`: `header_resolved` defaults to false.
    pub fn new(
        index: i32,
        offset: i64,
        length: i64,
        kind: EmbeddedPreviewKind,
        width: i32,
        height: i32,
        color_space: PreviewColorSpace,
    ) -> Self {
        Self {
            index,
            offset,
            length,
            kind,
            width,
            height,
            color_space,
            header_resolved: false,
        }
    }
}

/// `ExifBlock` record: byte span of a TIFF/EXIF metadata block within a RAW container.
/// `ifd_is_exif`: the block's IFD0 is itself the Exif IFD (exposure fields directly, no 0x8769 pointer), as in the Canon
/// CR3 "CMT2" box; only meaningful with `is_tiff_header`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExifBlock {
    pub offset: i64,
    pub length: i64,
    pub is_tiff_header: bool,
    pub ifd_is_exif: bool,
}

impl ExifBlock {
    /// `new ExifBlock(offset, length, isTiffHeader)`: `ifd_is_exif` defaults to false.
    pub fn new(offset: i64, length: i64, is_tiff_header: bool) -> Self {
        Self {
            offset,
            length,
            is_tiff_header,
            ifd_is_exif: false,
        }
    }

    /// `new ExifBlock(offset, length, isTiffHeader, ifdIsExif)`.
    pub fn with_ifd_is_exif(offset: i64, length: i64, is_tiff_header: bool, ifd_is_exif: bool) -> Self {
        Self {
            offset,
            length,
            is_tiff_header,
            ifd_is_exif,
        }
    }
}

/// Everything a header source or a container reader may fail with. C# throws `InvalidDataException` for every malformed,
/// hostile, over-budget, truncated or unreadable input ("callers that catch only that type stay total"), an
/// `ArgumentOutOfRangeException` for a negative offset or count, and `OperationCanceledException` (never wrapped).
/// `Display` is the C# `Exception.Message`.
#[derive(Debug)]
pub enum RawHeaderError {
    /// `InvalidDataException`. `io` is the C# `InnerException` when the source wrapped a disk/share failure
    /// (`IOException`/`ObjectDisposedException`, see [`RawHeaderError::is_io_failure`]); `ObjectDisposedException` has no
    /// Rust twin, a closed/failed stream is an `io::Error` too.
    InvalidData { message: String, io: Option<io::Error> },
    /// `ArgumentOutOfRangeException(nameof(offset), ...)` from `read` with a negative offset or count: a caller bug, not
    /// a hostile file. `param` is `"offset"` (the C# `ParamName`, pinned by a test).
    ArgumentOutOfRange { param: &'static str, message: String },
    /// `OperationCanceledException` from a container reader that observed its `CancelToken`.
    Canceled,
}

impl RawHeaderError {
    /// `new InvalidDataException(message)`.
    pub fn invalid_data(message: impl Into<String>) -> Self {
        todo!()
    }

    /// `new InvalidDataException(message, inner)` for a wrapped I/O failure.
    pub fn invalid_data_io(message: impl Into<String>, io: io::Error) -> Self {
        todo!()
    }

    /// `RawHeaderErrors.IsIoFailure(ex)`: true only for `InvalidData` with an inner `io` error (a disk/share failure, not a
    /// malformed or over-budget RAW). Such an error must keep propagating as an I/O problem, never be swallowed as
    /// "preview unusable". `ArgumentOutOfRange` and `Canceled` are never I/O failures.
    pub fn is_io_failure(&self) -> bool {
        todo!()
    }
}

impl fmt::Display for RawHeaderError {
    /// `InvalidData`: the message; `ArgumentOutOfRange`: .NET's `"{message} (Parameter '{param}')"`;
    /// `Canceled`: `"The operation was canceled."`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl std::error::Error for RawHeaderError {
    /// The inner `io` error of `InvalidData`, else `None`.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        todo!()
    }
}

impl From<Canceled> for RawHeaderError {
    /// `ct.ThrowIfCancellationRequested()` becomes `ct.check()?`.
    fn from(_: Canceled) -> Self {
        todo!()
    }
}

/// `IRawHeaderSource`: random-access window reads into the container header without reading the whole file.
///
/// `read` takes `&mut self` and returns a slice borrowed from `self`: the borrow checker enforces the C# rule "the returned
/// span is valid only until the next `Read` on the same source" (a multi-block read is assembled in a shared buffer that
/// the next multi-block read reuses), so copy the bytes you need to keep. Implementations buffer block-aligned segments
/// and enforce `RawContainerLimits::MAX_HEADER_BYTES` (W5-01a); all failures are `RawHeaderError::InvalidData`
/// (range outside the source, exhausted budget, truncated or unreadable data; an I/O failure is `io`).
pub trait RawHeaderSource {
    /// Total byte length of the underlying source.
    fn length(&self) -> i64;

    /// Reads `count` bytes starting at `offset`. `offset < 0 || count < 0` is `ArgumentOutOfRange { param: "offset" }`;
    /// `offset > length - count` (overflow-free) is `InvalidData` "past end"; `count > MAX_HEADER_BYTES` is `InvalidData`;
    /// `count == 0` inside the source (including `offset == length`) is an empty slice.
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError>;
}

/// `IRawContainerReader`: parser for one camera RAW container format family. Readers are stateless and shared across
/// threads (the registry holds them in a list), hence `Send + Sync`.
pub trait RawContainerReader: Send + Sync {
    /// True if the first 64 bytes (fewer for a tiny file) and the file extension (with the dot, any case) match this reader.
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool;

    /// Reads container info (dimensions, previews, EXIF blocks) through bounded header reads. `InvalidData` if the
    /// container is corrupt or hostile (never a panic, never an unbounded loop); `Canceled` when `ct` fires.
    fn read(&self, source: &mut dyn RawHeaderSource, ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError>;
}

/// `IRawPreviewFallback`: an alternate encoded JPEG preview when a supported RAW container's embedded preview cannot be
/// decoded. The returned length is an estimate of thumbnail source bytes consumed; a native provider may do more reads.
/// Errors are `DecodeError` (C# throws `NotSupportedException` for a format the provider does not handle, or the LibRaw
/// exceptions); `RawDecoder`'s recoverable-failure test (W5-12) classifies them with the W4-12b whitelist.
pub trait RawPreviewFallback: Send + Sync {
    /// Reads a bounded JPEG thumbnail from `path` for `format`.
    fn read_jpeg_thumbnail(&self, path: &str, format: RawFormat) -> Result<Vec<u8>, DecodeError>;
}

/// `InMemoryRawHeaderSource` (C# test fixture, promoted to the crate because every reader test and `tests/` integration
/// file needs it and `cfg(test)` items are invisible to them). Backed by a byte buffer. Budget semantics match
/// `SourceRawHeaderSource` exactly: the header budget is charged once per distinct [`RAW_HEADER_BLOCK_SIZE`] block (the
/// last block counts only the bytes it holds); a multi-block read that runs out of budget part-way keeps the blocks it
/// already charged (later reads of them are free); the returned slice borrows the buffer directly (no copy).
#[derive(Debug, Clone)]
pub struct InMemoryRawHeaderSource {
    bytes: Vec<u8>,
    touched_blocks: HashSet<i64>,
    total_bytes_read: i64,
}

impl InMemoryRawHeaderSource {
    /// Both C# constructors (`ReadOnlyMemory<byte>` and `byte[]`).
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
            touched_blocks: HashSet::new(),
            total_bytes_read: 0,
        }
    }

    /// Bytes charged against the header budget so far (whole distinct 64 KiB blocks), C# `TotalBytesRead`.
    pub fn total_bytes_read(&self) -> i64 {
        self.total_bytes_read
    }
}

impl RawHeaderSource for InMemoryRawHeaderSource {
    fn length(&self) -> i64 {
        todo!()
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        todo!()
    }
}
