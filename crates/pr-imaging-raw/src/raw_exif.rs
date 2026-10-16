//! Port of `PhotoReview.Imaging.Raw/RawExif.cs` (C# baseline d92d6aae). Target: `crates/pr-imaging-raw/src/raw_exif.rs`.
//!
//! Builds one [`ExifSummary`] from the EXIF blocks a container reader found ([`RawContainerInfo::exif_blocks`]), reading the
//! block bytes through the bounded [`RawHeaderSource`] (no whole-file read, RAW-15). Never panics and never returns an error:
//! a block that cannot be read only clears the `complete` flag, so the caller does not remember a `None` summary as
//! "this file has no EXIF".
//!
//! Depends on W5-01a (`RawContainerInfo`, `limits`), W5-01b (`ExifBlock`, `RawHeaderSource`) and the merged W4-03b/W4-04a
//! (`pr_imaging::exif_parser`, `pr_imaging::exif_summary`). Pinned by `tests/golden/raw-exif.json`.

#![allow(unused_variables, unused_imports, dead_code)] // D11: removed together with the last `todo!()`.

use pr_imaging::exif_parser::{try_parse_jpeg, try_parse_tiff_block_with};
use pr_imaging::exif_summary::ExifSummary;

use crate::formats::limits::MAX_EXIF_BLOCK_BYTES;
use crate::{RawContainerInfo, RawHeaderSource};

/// `RawExif.MaxBlockReadBytes` (private const in C#, = `RawContainerLimits.MaxExifBlockBytes`, 4 MiB): the most bytes
/// read from any one block, whatever length the container claims.
const MAX_BLOCK_READ_BYTES: i64 = MAX_EXIF_BLOCK_BYTES as i64;

/// What `RawExif.TryReadExif` returns: the return value plus its `out bool complete`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawExifRead {
    /// Merged summary of all readable blocks; `None` when no block yielded a non-empty summary (or there are no blocks).
    pub summary: Option<ExifSummary>,
    /// `false` when at least one block could not be read (the source returned an error: hostile data, exhausted header
    /// budget, I/O error). A `None` summary with `complete == false` must not be remembered as "no EXIF". Stays `false`
    /// even when a later block succeeds. Blocks skipped by the range checks do NOT clear it.
    pub complete: bool,
}

/// `RawExif.TryReadExif(source, containerInfo, out complete)`: extracts an [`ExifSummary`] from the container's EXIF blocks.
///
/// Walks `container_info.exif_blocks` in order (see the contract, "Semantics notes", for the exact merge rules):
/// - a block is skipped (no read, `complete` untouched) when `offset < 0`, `offset >= source.length()` or `length <= 0`;
/// - otherwise `to_read = min(block.length, MAX_BLOCK_READ_BYTES, source.length() - block.offset)` bytes are read;
///   `Err(_)` from `source.read` sets `complete = false` and goes on with the next block; an empty slice is skipped;
/// - `is_tiff_header` blocks go through `try_parse_tiff_block_with(span, ifd_is_exif)`, the others (RAF/RW2 JPEG
///   previews) through `try_parse_jpeg(span)`;
/// - a non-empty summary becomes the merged one (first block) or is merged field by field, first `Some` wins, except that a
///   date from an `ifd_is_exif` block replaces a merged date that did not come from an `ifd_is_exif` block;
/// - the walk stops as soon as the merged summary has all eight fields.
///
/// `container_info.exif_blocks` empty gives `{ summary: None, complete: true }`. The source is taken as `&mut dyn`
/// (`read` borrows `self` mutably and its slice is only valid until the next read: parse it before reading again).
pub fn try_read_exif(source: &mut dyn RawHeaderSource, container_info: &RawContainerInfo) -> RawExifRead {
    todo!()
}

/// `RawExif.IsComplete` (private): all eight fields (`date_taken`, `camera_make`, `camera_model`, `lens_model`, `iso`,
/// `focal_length`, `f_number`, `exposure_time`) are `Some`.
fn is_complete(summary: &ExifSummary) -> bool {
    todo!()
}

/// `RawExif.Merge` (private): `first with { field = first.field ?? later.field, .. }` for all eight fields.
fn merge(first: &ExifSummary, later: &ExifSummary) -> ExifSummary {
    todo!()
}
