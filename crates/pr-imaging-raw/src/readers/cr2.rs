//! Canon CR2 container reader (port of `PhotoReview.Imaging.Raw/Tiff/Cr2ContainerReader.cs`, W5-03, ADR 0009).
//!
//! CR2 is a little-endian TIFF with the two bytes `CR` at offset 8 (the Canon marker sits where classic TIFF has the
//! first IFD entry count, so IFD0 is normally at offset 16):
//!
//! * IFD0 carries the FULL-SIZE JPEG preview as a single strip (`StripOffsets` 0x0111 / `StripByteCounts` 0x0117); IFD0 is
//!   NOT the raw image, so its `ImageWidth`/`ImageLength` are the preview size (sRAW: larger than the sensor data).
//! * IFD1 (and later IFDs) may carry a smaller JPEG through `JPEGInterchangeFormat` 0x0201 / Length 0x0202.
//! * Orientation is read from IFD0 only (0x0112).
//! * Sensor size = Exif `PixelXDimension`/`PixelYDimension` of the first Exif IFD that has both, else the largest IFD.
//!
//! Hostile-input safe: the IFD chain is capped at [`MAX_IFD_COUNT`] and cycle-checked, and every failure is a
//! [`RawHeaderError`] (`InvalidData`, or `Canceled`), never a panic. Pure parser: no file system, no clock, no `unsafe`.
//! Header bytes come only through [`RawHeaderSource`], so the 8 MiB header budget of the source is honoured.
//!
//! Depends on W5-01 (types, traits, limits), W5-02 (`TiffHeaderNavigator`) and `pr_imaging::tiff` (W4-03a).
#![allow(unused_variables, unused_imports, dead_code)] // D11: remove with the last `todo!()` of this stub.

use pr_core::sync::CancelToken;
use pr_imaging::tiff::try_read_header;

use crate::tiff_nav::{self, TiffEntry};
use crate::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, PreviewColorSpace, RawContainerInfo, RawContainerReader,
    RawFormat, RawHeaderError, RawHeaderSource,
};

/// `RawContainerLimits.MaxIfdCount`: IFDs visited by one `read` (C# `const int`, kept `i32`).
pub const MAX_IFD_COUNT: i32 = 64;

/// A CR2 file must hold at least the 16 bytes `read` fetches first (C# `source.Length < 16`).
pub const MIN_FILE_LENGTH: i64 = 16;

/// The TIFF magic `read` requires (the sniffing `can_read` also requires `II`; `read` accepts `MM` too).
pub const TIFF_MAGIC: u16 = 42;

/// `Orientation` tag, read from IFD0 only.
pub const TAG_ORIENTATION: u16 = 0x0112;

/// `ExifIFD` pointer tag.
pub const TAG_EXIF_IFD: u16 = 0x8769;

/// Stateless reader for Canon `.cr2` files (C# `Cr2ContainerReader`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cr2ContainerReader;

impl Cr2ContainerReader {
    pub const fn new() -> Self {
        Self
    }
}

impl RawContainerReader for Cr2ContainerReader {
    /// `true` when `extension` equals `.cr2` (OrdinalIgnoreCase, via `pr_core::text::eq_ignore_case`) AND
    /// `first_64_bytes` holds at least 10 bytes that start `49 49 2A 00` (`II*\0`, little-endian only) and have `C` `R`
    /// at offsets 8 and 9. Bytes 4..8 (the IFD0 offset) are not looked at.
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        todo!("W5-03: signature sniffing, see contract.md semantics 1")
    }

    /// Parses the container through bounded header reads.
    ///
    /// Errors: `RawHeaderError::InvalidData` for a source shorter than 16 bytes, an unreadable TIFF header, a magic other
    /// than 42, and whatever the navigator or the source raises (range outside the source, exhausted header budget,
    /// I/O failure wrapped by the source); `RawHeaderError::Canceled` when `ct` is canceled at the top of an IFD iteration.
    /// Never panics.
    fn read(&self, source: &mut dyn RawHeaderSource, ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        todo!("W5-03: IFD loop, see contract.md semantics 2..9")
    }
}
