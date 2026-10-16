//! Port of `PhotoReview.Imaging.Raw/Tiff/JpegMarkerProbe.cs` (C# baseline d92d6aae, 79 lines).
//!
//! Walks the JPEG marker segments of a candidate embedded-preview byte range to tell a decodable lossy JPEG (SOF0/1/2,
//! 8-bit) from lossless JPEG (SOF3) sensor data, which shares TIFF `Compression=7` (and the CR3 raw track) with real
//! previews. Used by the DNG/NEF strip-preview path (W5-04/W5-06) and the CR3 track probe (W5-10).
//!
//! Never panics: every hostile offset/length/segment length yields `Ok(None)`. Only a genuine I/O failure of the header
//! source (`RawHeaderError::is_io_failure`) is returned as `Err`; an exhausted header budget or truncated data is
//! "not a usable preview range" (`Ok(None)`), like the C# `catch (InvalidDataException) when (!IsIoFailure)`.
//!
//! ASSUMED W5-01 names (see contract, "Depends on"): `crate::header_source::{RawHeaderSource, RawHeaderError}` and
//! `crate::limits::{MAX_JPEG_MARKER_SEGMENTS, MAX_JPEG_FILL_BYTES}`. The lead reconciles the paths when W5-01 lands.
#![allow(unused_variables, unused_imports, dead_code)] // D11: stub with todo!() bodies; remove with the last todo!().

use crate::contracts::{RawHeaderError, RawHeaderSource};
use crate::formats::limits::{MAX_JPEG_FILL_BYTES, MAX_JPEG_MARKER_SEGMENTS};

/// Pixel size of the first lossy frame header found by [`try_read_lossy_frame`] (C# `out int width, out int height`).
/// Both values are `1..=65535` (16-bit fields, zero rejected).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LossyFrame {
    pub width: i32,
    pub height: i32,
}

/// C# `JpegMarkerProbe.TryReadLossyFrame(source, offset, length, out width, out height)`.
///
/// `Ok(Some(frame))` when the range `[offset, offset + length)` starts with SOI (`FF D8`) and its first frame header is
/// SOF0/1/2 with 8-bit precision and non-zero width and height. `Ok(None)` for everything else: range outside the
/// source (`length < 4`, `offset < 0`, `offset > source.length() - length`), no SOI, SOS/EOI/stuffed byte before a frame,
/// segment length below 2, lossless (SOF3), arithmetic or hierarchical frames (SOF5..SOF15 except the DHT/JPG/DAC
/// markers), SOF segment shorter than 8 bytes or cut off by the end of the range, precision other than 8, zero size,
/// more than `MAX_JPEG_MARKER_SEGMENTS` segments visited, more than `MAX_JPEG_FILL_BYTES` consecutive fill bytes, and a
/// source error that is not an I/O failure. `Err` only when `RawHeaderError::is_io_failure()`.
///
/// On `Ok(None)` the C# `out` values are unspecified in one corner (a zero-width frame leaves a non-zero height behind);
/// no caller reads them on `false`, so the Rust API returns nothing there.
pub fn try_read_lossy_frame(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
) -> Result<Option<LossyFrame>, RawHeaderError> {
    todo!("W5-02b: wrap walk_lossy_frame, map non-I/O InvalidData to Ok(None)")
}

/// C# private `WalkLossyFrame`: the bounded marker walk (see the contract for the exact loop, caps and order of checks).
fn walk_lossy_frame(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
) -> Result<Option<LossyFrame>, RawHeaderError> {
    todo!("W5-02b")
}
