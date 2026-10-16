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
    match walk_lossy_frame(source, offset, length) {
        Ok(frame) => Ok(frame),
        Err(e) if e.is_io_failure() => Err(e),
        Err(_) => Ok(None),
    }
}

/// Reads `count` bytes at `pos` into a fixed array. A short slice (trait violation) is `None`, never a panic.
fn read_array<const N: usize>(source: &mut dyn RawHeaderSource, pos: i64) -> Result<Option<[u8; N]>, RawHeaderError> {
    let slice = source.read(pos, N as i32)?;
    Ok(<[u8; N]>::try_from(slice).ok())
}

/// C# private `WalkLossyFrame`: the bounded marker walk (see the contract for the exact loop, caps and order of checks).
fn walk_lossy_frame(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
) -> Result<Option<LossyFrame>, RawHeaderError> {
    if length < 4 || offset < 0 || offset > source.length() - length {
        return Ok(None);
    }
    let end = offset + length;
    match read_array::<2>(source, offset)? {
        Some([0xFF, 0xD8]) => {}
        _ => return Ok(None),
    }
    let mut pos = offset + 2;
    let mut fill_bytes: i32 = 0;
    let mut segment: i32 = 0;
    while segment < MAX_JPEG_MARKER_SEGMENTS && pos + 4 <= end {
        let Some(head) = read_array::<4>(source, pos)? else {
            return Ok(None);
        };
        if head[0] != 0xFF {
            return Ok(None);
        }
        let marker = head[1];
        if marker == 0xFF {
            fill_bytes += 1;
            if fill_bytes > MAX_JPEG_FILL_BYTES {
                return Ok(None);
            }
            pos += 1;
            continue;
        }
        fill_bytes = 0;
        if marker == 0x00 || marker == 0xDA || marker == 0xD9 {
            return Ok(None);
        }
        if marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            pos += 2;
            segment += 1;
            continue;
        }
        let segment_length = (i64::from(head[2]) << 8) | i64::from(head[3]);
        if segment_length < 2 {
            return Ok(None);
        }
        let is_sof = (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC;
        if is_sof {
            if marker > 0xC2 || segment_length < 8 || pos + 9 > end {
                return Ok(None);
            }
            let Some(f) = read_array::<5>(source, pos + 4)? else {
                return Ok(None);
            };
            if f[0] != 8 {
                return Ok(None);
            }
            let height = (i32::from(f[1]) << 8) | i32::from(f[2]);
            let width = (i32::from(f[3]) << 8) | i32::from(f[4]);
            return Ok((width > 0 && height > 0).then_some(LossyFrame { width, height }));
        }
        pos += 2 + segment_length;
        segment += 1;
    }
    Ok(None)
}
