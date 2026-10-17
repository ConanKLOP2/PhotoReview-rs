//! Port of `PhotoReview.Imaging.Raw/RawExif.cs` (C# baseline d92d6aae). Target: `crates/pr-imaging-raw/src/raw_exif.rs`.
//!
//! Builds one [`ExifSummary`] from the EXIF blocks a container reader found ([`RawContainerInfo::exif_blocks`]), reading the
//! block bytes through the bounded [`RawHeaderSource`] (no whole-file read, RAW-15). Never panics and never returns an error:
//! a block that cannot be read only clears the `complete` flag, so the caller does not remember a `None` summary as
//! "this file has no EXIF".
//!
//! Depends on W5-01a (`RawContainerInfo`, `limits`), W5-01b (`ExifBlock`, `RawHeaderSource`) and the merged W4-03b/W4-04a
//! (`pr_imaging::exif_parser`, `pr_imaging::exif_summary`). Pinned by `tests/golden/raw-exif.json`.

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
    let mut complete = true;
    let mut merged: Option<ExifSummary> = None;
    let mut date_from_exif_ifd = false;

    for block in &container_info.exif_blocks {
        let length = source.length();
        if block.offset < 0 || block.offset >= length || block.length <= 0 {
            continue;
        }
        // Clamp to the bytes that exist and to the per-block cap; both bounds keep the value in 1..=4 MiB.
        let to_read = block.length.min(MAX_BLOCK_READ_BYTES).min(length - block.offset);
        let Ok(count) = i32::try_from(to_read) else {
            continue;
        };

        let summary = match source.read(block.offset, count) {
            Ok([]) => continue,
            Ok(span) => {
                if block.is_tiff_header {
                    try_parse_tiff_block_with(span, block.ifd_is_exif)
                } else {
                    try_parse_jpeg(span)
                }
            }
            Err(_) => {
                complete = false;
                continue;
            }
        };

        let Some(summary) = summary.filter(|s| !s.is_empty()) else {
            continue;
        };

        match merged.as_ref() {
            None => {
                date_from_exif_ifd = block.ifd_is_exif && summary.date_taken.is_some();
                merged = Some(summary);
            }
            Some(first) => {
                let prefer_later_date = block.ifd_is_exif && !date_from_exif_ifd && summary.date_taken.is_some();
                let mut next = merge(first, &summary);
                if prefer_later_date {
                    next.date_taken = summary.date_taken;
                    date_from_exif_ifd = true;
                }
                merged = Some(next);
            }
        }

        if merged.as_ref().is_some_and(is_complete) {
            break;
        }
    }

    RawExifRead {
        summary: merged,
        complete,
    }
}

/// `RawExif.IsComplete` (private): all eight fields are `Some`.
fn is_complete(summary: &ExifSummary) -> bool {
    summary.date_taken.is_some()
        && summary.camera_make.is_some()
        && summary.camera_model.is_some()
        && summary.lens_model.is_some()
        && summary.iso.is_some()
        && summary.focal_length.is_some()
        && summary.f_number.is_some()
        && summary.exposure_time.is_some()
}

/// `RawExif.Merge` (private): per field, the first `Some` wins.
fn merge(first: &ExifSummary, later: &ExifSummary) -> ExifSummary {
    ExifSummary {
        date_taken: first.date_taken.or(later.date_taken),
        camera_make: first.camera_make.clone().or_else(|| later.camera_make.clone()),
        camera_model: first.camera_model.clone().or_else(|| later.camera_model.clone()),
        lens_model: first.lens_model.clone().or_else(|| later.lens_model.clone()),
        iso: first.iso.or(later.iso),
        focal_length: first.focal_length.or(later.focal_length),
        f_number: first.f_number.or(later.f_number),
        exposure_time: first.exposure_time.or(later.exposure_time),
    }
}
