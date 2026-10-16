//! Sony ARW container reader (W5-07). Port of `PhotoReview.Imaging.Raw/Tiff/ArwContainerReader.cs` (170 lines, baseline d92d6aae).
//!
//! ARW is a little-endian TIFF (`II*\0`): IFD0 carries the large JPEG preview in 0x0201/0x0202, IFD1 the thumbnail, the
//! SubIFDs (0x014A) the raw image (sometimes a JPEG), and the Exif MakerNote the Sony `PreviewImage` (0x2001).
//!
//! Hostile-input rules (hardening tests in C#): never panics, every offset/length goes through the W5-02 navigator's
//! bounds checks, at most [`MAX_IFD_COUNT`] IFDs are read, a repeated IFD offset is read once. The only errors are the
//! ones the header source raises (`RawHeaderError::InvalidData`, budget exhausted, I/O) and `RawHeaderError::Canceled`.
//! Pure: no file system access of its own (all bytes come through the `RawHeaderSource`), no `unsafe`.

// D11: removed in the commit that removes the last `todo!()` (the imports are used by the bodies; `ptr_arg` fires only while
// the `Vec` is never pushed to).
#![allow(unused_variables, dead_code, unused_imports, clippy::ptr_arg)]

use pr_core::sync::CancelToken;

use crate::formats::limits::MAX_IFD_COUNT;
use crate::tiff_nav::{self as nav, TiffEntry};
use crate::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, PreviewColorSpace, RawContainerInfo, RawContainerReader,
    RawFormat, RawHeaderError, RawHeaderSource,
};

/// The only extension this reader accepts, compared with `OrdinalIgnoreCase` (use `pr_core::text::eq_ignore_case`).
const ARW_EXTENSION: &str = ".arw";

/// A file shorter than this is `InvalidData("File too short for ARW container.")`.
const MIN_FILE_LENGTH: i64 = 16;

/// TIFF tags used by this reader.
const TAG_ORIENTATION: u16 = 0x0112;
const TAG_SUB_IFDS: u16 = 0x014A;
const TAG_EXIF_IFD: u16 = 0x8769;
const TAG_MAKER_NOTE: u16 = 0x927C;
/// Sony MakerNote `PreviewImage` (UNDEFINED bytes, `count` = length, offset relative to the TIFF header).
const TAG_SONY_PREVIEW_IMAGE: u16 = 0x2001;

/// Some bodies prefix the MakerNote with `SONY DSC \0\0\0`; its IFD then starts 12 bytes in. Offsets stay file-absolute.
const SONY_DSC_PREFIX: &[u8; 9] = b"SONY DSC ";
const SONY_DSC_IFD_SKIP: i64 = 12;

/// A MakerNote that starts closer than this to the end of the file is ignored without reading (`note > length - 14`).
const MAKER_NOTE_MIN_TAIL: i64 = 14;
/// A MakerNote offset below the end of the TIFF header (8) is ignored.
const MAKER_NOTE_MIN_OFFSET: i64 = 8;

/// Reader for Sony ARW (C# `ArwContainerReader`). Stateless; one value serves every file.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArwContainerReader;

impl RawContainerReader for ArwContainerReader {
    /// C# `CanRead`: extension equals `.arw` ignoring case AND at least 4 bytes AND bytes `49 49 2A 00` (little-endian
    /// TIFF only; a big-endian `MM\0*` file and magic 43 are NOT accepted). Looks only at the first four bytes.
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        todo!()
    }

    /// C# `Read`. See contract.md "Algorithm" for the exact step order; the order of the IFD queue decides the order of
    /// `previews`, so it is part of the behaviour.
    fn read(&self, source: &mut dyn RawHeaderSource, ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        todo!()
    }
}

/// C# `AddPreview`: appends a JPEG preview unless one with the same `offset` is already listed (offset-only dedupe).
/// `index` = the list length before the push; `color_space` = `Unknown`; `header_resolved` = false.
fn add_preview(previews: &mut Vec<EmbeddedPreview>, offset: i64, length: i64, width: i32, height: i32) {
    todo!()
}

/// C# `ParseSonyMakerNotePreview`: finds the 0x2001 `PreviewImage` entry of the Sony MakerNote at `note_offset` and lists
/// it with width/height 0 (the W5-11 `PreviewSelector` resolves them lazily). Anything wrong with the note (offset out of
/// range, no 0x2001 entry, count <= 4, range outside the file, not SOI) returns `Ok(())` without a preview; only errors
/// raised by the source propagate.
fn parse_sony_maker_note_preview(
    source: &mut dyn RawHeaderSource,
    note_offset: i64,
    little_endian: bool,
    previews: &mut Vec<EmbeddedPreview>,
) -> Result<(), RawHeaderError> {
    todo!()
}
