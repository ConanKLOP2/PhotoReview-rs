//! Olympus / OM System ORF container reader (port of `PhotoReview.Imaging.Raw/Tiff/OrfContainerReader.cs`, W5-05).
//!
//! Reads an ORF through bounded header reads: orientation (IFD0 only), the raw frame size (IFD0 ImageWidth/ImageLength, with the
//! one hard-coded 4080 -> 4056 LibRaw rule), every JPEG preview of the IFD chain (JPEGInterchangeFormat 0x0201/0x0202), and the
//! previews hidden in the Olympus MakerNote (ThumbnailImage 0x0100 and CameraSettings 0x2020 -> PreviewImageStart/Length).
//! Hostile-input safe: nothing here may panic, every offset sum is `i64`, every loop is bounded by `limits::MAX_IFD_COUNT` and
//! `limits::MAX_ENTRIES_PER_IFD`, and a failed header read surfaces as `RawHeaderError` (C# `InvalidDataException`).
//!
//! Pure parser: no `unsafe`, no file system (the caller supplies a [`RawHeaderSource`]), no clock.

// D11: allowed only while this file still has `todo!()` bodies; the porter deletes the line with the last `todo!()`
// (`unused_imports` / `ptr_arg`: the real bodies use every import and push to the `Vec`).
#![allow(unused_variables, unused_imports, dead_code, clippy::ptr_arg)]

use pr_core::sync::CancelToken;

use crate::formats::limits;
use crate::tiff_nav::{self as nav, TiffEntry};
use crate::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, PreviewColorSpace, RawContainerInfo, RawContainerReader,
    RawFormat, RawHeaderError, RawHeaderSource, preview_selector,
};

/// Raw frame width (IFD0 ImageWidth) of the E-PM1 / E-PL3 / E-P3 sensor: 24 of its 4080 columns are not image
/// (C# `RawFrameWidthWithMaskedColumns`, `internal`).
pub const RAW_FRAME_WIDTH_WITH_MASKED_COLUMNS: i32 = 4080;

/// Width LibRaw decodes for [`RAW_FRAME_WIDTH_WITH_MASKED_COLUMNS`]: its identify.cpp trims the 24 columns
/// (C# `DecodedWidthWithoutMaskedColumns`, `internal`).
pub const DECODED_WIDTH_WITHOUT_MASKED_COLUMNS: i32 = 4056;

/// ORF reader. Stateless: one value can be shared by every thread (the registry holds one instance).
#[derive(Debug, Clone, Copy, Default)]
pub struct OrfContainerReader;

impl OrfContainerReader {
    /// A new reader (C# `new OrfContainerReader()`).
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl RawContainerReader for OrfContainerReader {
    /// `extension` equals `.orf` ignoring case (OrdinalIgnoreCase, `pr_core::text::eq_ignore_case`), at least 4 bytes are given, and
    /// the first four bytes are exactly one of `IIRO`, `IIRS`, `MMOR`, `II*\0`. Plain big-endian `MM\0*` is NOT accepted here (but
    /// [`read`](Self::read) accepts it).
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        todo!("W5-05: see contract, 'CanRead'")
    }

    /// Parses the container. Errors: [`RawHeaderError::invalid_data`] with `"File too short for ORF container."` (length < 16) or
    /// `"Invalid ORF header."` (byte order letters not `II`/`MM`, or magic not 42 / 0x4F52 / 0x5352); any error of the navigator or the
    /// header source (budget exhausted, I/O failure) propagates unchanged; a cancelled `token` yields the cancellation error
    /// (checked at the top of every IFD iteration).
    fn read(&self, source: &mut dyn RawHeaderSource, token: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        todo!("W5-05: see contract, 'Read'")
    }
}

/// The width the zoom (LibRaw) decode returns: `4056` for an IFD0 width of exactly `4080`, else the width unchanged
/// (C# private `DecodedWidth`). Height is never touched.
fn decoded_width(ifd0_width: i32) -> i32 {
    todo!("W5-05")
}

/// Olympus / OM System MakerNote at `note_offset` (file offset, `0 < note_offset < length - 16`, guaranteed by the caller).
///
/// Signature dispatch on the first bytes of the note (order matters): `"OM SYSTEM\0"` (IFD at +16, byte order = marks at +12/+13 are
/// exactly `II`, offsets relative to the note), `"OLYMPUS\0"` (IFD at +12, marks at +8/+9, relative), `"OLYMP\0"` (legacy: IFD at +8,
/// file byte order, file-absolute offsets), anything else (plain TIFF IFD at the note start, file byte order, file-absolute).
/// Then: every `0x0100` entry with `count > 4` is a thumbnail (`base + value`, length `count`); the FIRST `0x2020` entry points
/// (`base + value`) at the CameraSettings IFD whose FIRST `0x0101` / `0x0102` give PreviewImageStart / Length (`base + start`).
fn parse_olympus_maker_note(
    source: &mut dyn RawHeaderSource,
    note_offset: i64,
    file_little_endian: bool,
    previews: &mut Vec<EmbeddedPreview>,
) -> Result<(), RawHeaderError> {
    todo!("W5-05")
}

/// Adds a JPEG preview found through the MakerNote: ignored unless `nav::is_range_in_file(offset, length, len)`, `length >= 4`, the two
/// bytes at `offset` are `FF D8`, and no preview with the same `offset` is listed yet. Width and height stay 0 (resolved lazily by the
/// preview selector); `index` is the list length before the push; colour space `Unknown`; `header_resolved` false.
fn add_preview(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
    previews: &mut Vec<EmbeddedPreview>,
) -> Result<(), RawHeaderError> {
    todo!("W5-05")
}
