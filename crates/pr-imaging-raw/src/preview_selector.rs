//! Port of `PhotoReview.Imaging.Raw/PreviewSelector.cs` (C# baseline d92d6aae, 355 lines): RAW-21 / ADR 0009 embedded
//! preview selection, the bounded JPEG marker walk that resolves unknown preview sizes (and the Adobe RGB marker), and the
//! strict EXIF Interoperability check.
//!
//! * [`select_preview`] / [`select_preview_resolved`]: among JPEG previews pick the smallest whose BOTH sides (after the
//!   orientation transpose) cover the request box, else the largest. Unknown sizes are resolved by walking the JPEG header
//!   in largest-byte-length order; every unknown-size candidate is walked (no early stop). The resolved list is returned
//!   so the caller can cache it (`header_resolved` stops a later call from walking again).
//! * [`try_read_jpeg_frame`] / [`walk_jpeg_header`]: the bounded marker walk (SOF may sit past 64 KiB, Fujifilm RAF).
//! * [`is_adobe_rgb_exif`]: InteropIndex `"R03"` through IFD0 -> Exif IFD -> Interop IFD, every offset bounds-checked.
//!
//! No unsafe, no I/O except through the injected header source. NEVER panics on hostile input: every index into data that
//! came from a source or a payload is bounds-checked (`get`, `try_into`, the `pr_imaging::tiff` readers).

use pr_imaging::decode_types::DecodeBox;

use crate::contracts::{EmbeddedPreview, EmbeddedPreviewKind, PreviewColorSpace, RawHeaderError, RawHeaderSource};
use crate::formats::limits::{MAX_JPEG_FILL_BYTES, MAX_JPEG_MARKER_SEGMENTS};

/// `UnknownAreaPerByte`: an unknown-size preview is ranked by `length * 4` (about 0.25 byte per pixel). A RANKING score
/// only, never reported as dimensions. The product wraps like unchecked C# `long` arithmetic: use `wrapping_mul`.
const UNKNOWN_AREA_PER_BYTE: i64 = 4;

/// `ViewablePreviewLongSide`: a known-size preview whose longer side is at least this many pixels is "viewable" (tier 1) and
/// always outranks unknown-size entries and tiny thumbnails.
const VIEWABLE_PREVIEW_LONG_SIDE: i32 = 1000;

/// Result of the `out resolvedPreviews` overload: the chosen preview and `previews` with every preview whose JPEG header was
/// walked during the selection replaced by its resolved copy (dimensions, colour space, `header_resolved = true`), in the
/// same order and with the same length as the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSelection {
    pub chosen: Option<EmbeddedPreview>,
    pub resolved_previews: Vec<EmbeddedPreview>,
}

/// Outcome of the marker walk when a frame header was found: `width`/`height` are `1..=65535` (16-bit fields, zero
/// rejected). `color_space` is `AdobeRgb` when an EXIF APP1 seen BEFORE the frame header carried the R03 marker, else
/// `Unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JpegFrame {
    pub width: i32,
    pub height: i32,
    pub color_space: PreviewColorSpace,
}

/// What [`walk_jpeg_header`] learned. C# reports `colorSpace` through an `out` parameter even when the method returns
/// `false` ("frame size unreadable, but the APP1 before it was looked at"), and `ResolveDimensions` relies on it, so the
/// walk result keeps the two facts apart: `frame` is `Some((width, height))` only when the C# method returns `true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JpegHeaderWalk {
    pub frame: Option<(i32, i32)>,
    pub color_space: PreviewColorSpace,
}

/// `PreviewSelector.SelectPreview(source, previews, requestBox, orientation)`.
///
/// # Errors
/// Only an I/O failure of the source (`RawHeaderError::is_io_failure`), or a programming error that the in-crate sources
/// cannot produce. Budget exhaustion, a range outside the file and a truncated header are NOT errors: the preview's size
/// simply stays unknown (see [`select_preview_resolved`]).
pub fn select_preview(
    source: &mut dyn RawHeaderSource,
    previews: &[EmbeddedPreview],
    request_box: DecodeBox,
    orientation: i32,
) -> Result<Option<EmbeddedPreview>, RawHeaderError> {
    select_preview_resolved(source, previews, request_box, orientation).map(|s| s.chosen)
}

/// `PreviewSelector.SelectPreview(.., out resolvedPreviews)`. Steps (the order of reads is observable through the budget):
/// 1. `state = previews.to_vec()`; `chosen = select_preview_core(..)`.
/// 2. If `chosen` is a JPEG, [`resolve_color_space`] on it (a no-op when already resolved or the colour space is known).
/// 3. Return both. On `Err` nothing is returned (C# leaves `resolvedPreviews` unassigned by the throw).
///
/// # Errors
/// As [`select_preview`].
pub fn select_preview_resolved(
    source: &mut dyn RawHeaderSource,
    previews: &[EmbeddedPreview],
    request_box: DecodeBox,
    orientation: i32,
) -> Result<PreviewSelection, RawHeaderError> {
    let mut state = previews.to_vec();
    let idx = select_preview_core(source, &mut state, request_box, orientation)?;
    if let Some(i) = idx {
        if state[i].kind == EmbeddedPreviewKind::Jpeg {
            let resolved = resolve_color_space(source, &state[i])?;
            state[i] = resolved;
        }
    }
    let chosen = idx.map(|i| state[i].clone());
    Ok(PreviewSelection {
        chosen,
        resolved_previews: state,
    })
}

/// Index of the best candidate by `(score, length)`; ties go to the first in `order` (strict `>`).
fn best_of(state: &[EmbeddedPreview], order: &[usize]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for &i in order {
        let key = (score(&state[i]), state[i].length);
        match best {
            Some(b) if key <= (score(&state[b]), state[b].length) => {}
            _ => best = Some(i),
        }
    }
    best
}

/// C# `SelectPreviewCore`. Returns the INDEX of the chosen preview in `state` (every walked preview has already been
/// written back into `state[i]`), `None` for an empty list.
fn select_preview_core(
    source: &mut dyn RawHeaderSource,
    state: &mut [EmbeddedPreview],
    request_box: DecodeBox,
    orientation: i32,
) -> Result<Option<usize>, RawHeaderError> {
    if state.is_empty() {
        return Ok(None);
    }
    let mut jpeg: Vec<usize> = (0..state.len())
        .filter(|&i| state[i].kind == EmbeddedPreviewKind::Jpeg)
        .collect();
    if jpeg.is_empty() {
        return Ok(Some(0));
    }
    if jpeg.len() == 1 {
        let i = jpeg[0];
        let resolved = resolve_dimensions(source, &state[i])?;
        state[i] = resolved;
        return Ok(Some(i));
    }
    // Stable sort by length descending: equal lengths keep input order. No early stop: every candidate is resolved.
    jpeg.sort_by(|&a, &b| state[b].length.cmp(&state[a].length));
    for &i in &jpeg {
        let resolved = resolve_dimensions(source, &state[i])?;
        state[i] = resolved;
    }

    if request_box.is_unbounded() {
        return Ok(best_of(state, &jpeg));
    }

    let transposed = pr_imaging::orientation::is_transposed(orientation);
    let (req_w, req_h) = (request_box.width, request_box.height);
    let mut smallest: Option<usize> = None;
    for &i in &jpeg {
        let p = &state[i];
        let (vis_w, vis_h) = if transposed {
            (p.height, p.width)
        } else {
            (p.width, p.height)
        };
        let fits = if req_w > 0 && req_h > 0 {
            vis_w >= req_w && vis_h >= req_h
        } else if req_w > 0 {
            vis_w >= req_w
        } else if req_h > 0 {
            vis_h >= req_h
        } else {
            true
        };
        if !fits {
            continue;
        }
        let key = (score(p), p.length);
        match smallest {
            Some(b) if key >= (score(&state[b]), state[b].length) => {}
            _ => smallest = Some(i),
        }
    }
    if smallest.is_some() {
        return Ok(smallest);
    }
    Ok(best_of(state, &jpeg))
}

/// C# `Score`: `(tier, value)` compared lexicographically. Known size (`width > 0 && height > 0`): tier 1 when
/// `max(width, height) >= 1000`, else 0, value = `width * height` in `i64`. Unknown size: `(0, if length <= 0 { 0 } else
/// { length.wrapping_mul(4) })` (wraps for lengths above `i64::MAX / 4`, e.g. `i64::MAX` scores `(0, -4)`; keep it).
fn score(preview: &EmbeddedPreview) -> (i32, i64) {
    if preview.width > 0 && preview.height > 0 {
        let area = i64::from(preview.width) * i64::from(preview.height);
        let tier = i32::from(preview.width.max(preview.height) >= VIEWABLE_PREVIEW_LONG_SIDE);
        return (tier, area);
    }
    let value = if preview.length <= 0 {
        0
    } else {
        preview.length.wrapping_mul(UNKNOWN_AREA_PER_BYTE)
    };
    (0, value)
}

/// C# `ResolveDimensions`. Returns `preview` unchanged when `header_resolved` or both sides are positive, or when
/// `offset < 0 || offset >= source.length() || length < 4`. Otherwise walks (`need_frame = true`); a non-I/O
/// `InvalidData` leaves the preview unchanged and NOT marked resolved, any other error propagates.
fn resolve_dimensions(
    source: &mut dyn RawHeaderSource,
    preview: &EmbeddedPreview,
) -> Result<EmbeddedPreview, RawHeaderError> {
    if preview.header_resolved || (preview.width > 0 && preview.height > 0) {
        return Ok(preview.clone());
    }
    if preview.offset < 0 || preview.offset >= source.length() || preview.length < 4 {
        return Ok(preview.clone());
    }
    match walk_jpeg_header(source, preview.offset, preview.length, true) {
        Ok(walk) => {
            let mut result = preview.clone();
            if let Some((w, h)) = walk.frame {
                result.width = w;
                result.height = h;
            }
            if walk.color_space != PreviewColorSpace::Unknown {
                result.color_space = walk.color_space;
            }
            result.header_resolved = true;
            Ok(result)
        }
        Err(RawHeaderError::InvalidData { io: None, .. }) => Ok(preview.clone()),
        Err(e) => Err(e),
    }
}

/// C# `ResolveColorSpace` (only for the CHOSEN preview): returns it unchanged when `header_resolved`, `color_space !=
/// Unknown`, `offset < 0 || offset >= source.length()` or `length < 4`. Otherwise a colour-space-only walk and a copy with
/// the colour space (kept when the walk found none) and `header_resolved = true`. Same error mapping as
/// [`resolve_dimensions`].
fn resolve_color_space(
    source: &mut dyn RawHeaderSource,
    preview: &EmbeddedPreview,
) -> Result<EmbeddedPreview, RawHeaderError> {
    if preview.header_resolved
        || preview.color_space != PreviewColorSpace::Unknown
        || preview.offset < 0
        || preview.offset >= source.length()
        || preview.length < 4
    {
        return Ok(preview.clone());
    }
    match walk_jpeg_header(source, preview.offset, preview.length, false) {
        Ok(walk) => {
            let mut result = preview.clone();
            if walk.color_space != PreviewColorSpace::Unknown {
                result.color_space = walk.color_space;
            }
            result.header_resolved = true;
            Ok(result)
        }
        Err(RawHeaderError::InvalidData { io: None, .. }) => Ok(preview.clone()),
        Err(e) => Err(e),
    }
}

/// `PreviewSelector.TryReadJpegFrame(source, offset, length, out width, out height, out colorSpace)`: `Ok(Some(frame))` when
/// the range `[offset, offset + length)` starts with SOI and its first frame header is SOF0/1/2 with non-zero size.
/// `Ok(None)` for everything else that is not an error. Does NOT check the sample precision and does NOT swallow source
/// errors.
///
/// # Errors
/// Whatever `source.read` returns.
pub fn try_read_jpeg_frame(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
) -> Result<Option<JpegFrame>, RawHeaderError> {
    let walk = walk_jpeg_header(source, offset, length, true)?;
    Ok(walk.frame.map(|(width, height)| JpegFrame {
        width,
        height,
        color_space: walk.color_space,
    }))
}

/// C# private `WalkJpeg(source, offset, length, needFrame, ..)`, public here so integration tests and the golden can see
/// the colour space of a walk that found no frame. `need_frame == false` stops at the first frame-header marker.
///
/// # Errors
/// Whatever `source.read` returns.
pub fn walk_jpeg_header(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
    need_frame: bool,
) -> Result<JpegHeaderWalk, RawHeaderError> {
    let mut color_space = PreviewColorSpace::Unknown;
    if length < 4 || offset < 0 || offset >= source.length() {
        return Ok(JpegHeaderWalk {
            frame: None,
            color_space,
        });
    }
    // Unchecked C# `long` arithmetic: the sum may wrap.
    let end = offset.wrapping_add(length).min(source.length());
    if end.wrapping_sub(offset) < 4 || source.read(offset, 2)? != [0xFF, 0xD8] {
        return Ok(JpegHeaderWalk {
            frame: None,
            color_space,
        });
    }

    let mut frame: Option<(i32, i32)> = None;
    let mut pos = offset.wrapping_add(2);
    let mut fill_bytes: i32 = 0;
    let mut segment: i32 = 0;
    while segment < MAX_JPEG_MARKER_SEGMENTS && pos.wrapping_add(4) <= end {
        let head: [u8; 4] = match source.read(pos, 4)?.try_into() {
            Ok(h) => h,
            Err(_) => break,
        };
        if head[0] != 0xFF {
            break;
        }
        let marker = head[1];
        if marker == 0xFF {
            // Fill byte: does not consume a segment iteration.
            fill_bytes += 1;
            if fill_bytes > MAX_JPEG_FILL_BYTES {
                break;
            }
            pos = pos.wrapping_add(1);
            continue;
        }
        fill_bytes = 0;
        if marker == 0x00 || marker == 0xDA || marker == 0xD9 {
            break;
        }
        if marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            pos = pos.wrapping_add(2);
            segment += 1;
            continue;
        }
        let seg_len = (i32::from(head[2]) << 8) | i32::from(head[3]);
        if seg_len < 2 || pos.wrapping_add(2).wrapping_add(i64::from(seg_len)) > end {
            break;
        }
        let payload_len = seg_len - 2;
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            if !need_frame || marker > 0xC2 || payload_len < 5 {
                break;
            }
            let bytes = source.read(pos.wrapping_add(4), 5)?;
            if let Some(&[_, h1, h0, w1, w0]) = bytes.first_chunk::<5>() {
                let height = (i32::from(h1) << 8) | i32::from(h0);
                let width = (i32::from(w1) << 8) | i32::from(w0);
                if width > 0 && height > 0 {
                    frame = Some((width, height));
                }
            }
            break;
        }
        if marker == 0xE1 && payload_len >= 14 && color_space == PreviewColorSpace::Unknown {
            let payload = source.read(pos.wrapping_add(4), payload_len)?;
            if is_adobe_rgb_exif(payload) {
                color_space = PreviewColorSpace::AdobeRgb;
                if !need_frame {
                    break;
                }
            }
        }
        pos = pos.wrapping_add(2).wrapping_add(i64::from(seg_len));
        segment += 1;
    }
    Ok(JpegHeaderWalk { frame, color_space })
}

/// C# `IsAdobeRgbExif` (`internal`; `pub` + `#[doc(hidden)]` here because integration tests and the fuzz twin call it).
/// True when `payload` ("Exif\0\0" + TIFF) has IFD0 -> 0x8769 (LONG x 1) -> Exif IFD -> 0xA005 (LONG x 1) -> Interop IFD ->
/// 0x0001 ASCII, count 4, inline `"R03\0"`. Anything malformed is `false`; never panics.
#[doc(hidden)]
#[must_use]
pub fn is_adobe_rgb_exif(payload: &[u8]) -> bool {
    if payload.len() < 14 || !payload.starts_with(b"Exif\0\0") {
        return false;
    }
    let tiff = &payload[6..];
    let little = match (tiff[0], tiff[1]) {
        (b'I', b'I') => true,
        (b'M', b'M') => false,
        _ => return false,
    };
    if read_u16_at(tiff, 2, little) != 42 {
        return false;
    }
    let ifd0 = i64::from(read_u32_at(tiff, 4, little));
    let Some(exif_ifd) = try_find_long_entry(tiff, ifd0, 0x8769, little) else {
        return false;
    };
    let Some(interop_ifd) = try_find_long_entry(tiff, exif_ifd, 0xA005, little) else {
        return false;
    };
    let Some(entry) = try_find_entry(tiff, interop_ifd, 0x0001, little) else {
        return false;
    };
    read_u16_at(tiff, entry + 2, little) == 2
        && read_u32_at(tiff, entry + 4, little) == 4
        && entry + 12 <= tiff.len() as i64
        && usize::try_from(entry + 8)
            .ok()
            .and_then(|s| tiff.get(s..s + 4))
            .is_some_and(|b| b == b"R03\0")
}

/// C# `TryFindLongEntry`: the entry for `tag` in the IFD at `ifd_offset` must be type 4 (LONG) with count 1; returns its
/// value (the pointed-to IFD offset, `u32` widened to `i64`).
fn try_find_long_entry(tiff: &[u8], ifd_offset: i64, tag: u16, little: bool) -> Option<i64> {
    let entry = try_find_entry(tiff, ifd_offset, tag, little)?;
    if read_u16_at(tiff, entry + 2, little) != 4 || read_u32_at(tiff, entry + 4, little) != 1 {
        return None;
    }
    Some(i64::from(read_u32_at(tiff, entry + 8, little)))
}

/// C# `TryFindEntry`: `ifd_offset < 8 || ifd_offset + 2 > tiff.len()` -> `None`; scans the `count` (u16) entries, `None` as
/// soon as one entry would end past `tiff.len()`, otherwise the offset of the FIRST entry with `tag`.
fn try_find_entry(tiff: &[u8], ifd_offset: i64, tag: u16, little: bool) -> Option<i64> {
    let len = tiff.len() as i64;
    if ifd_offset < 8 || ifd_offset + 2 > len {
        return None;
    }
    let count = read_u16_at(tiff, ifd_offset, little);
    for i in 0..i64::from(count) {
        let entry = ifd_offset + 2 + 12 * i;
        if entry + 12 > len {
            return None;
        }
        if read_u16_at(tiff, entry, little) == tag {
            return Some(entry);
        }
    }
    None
}

/// `TiffStructure.ReadU16` at an `i64` offset that the callers already bounds-checked (a value that does not fit `i32` is
/// outside any real payload and reads as 0, like the shared reader's out-of-range fallback).
fn read_u16_at(data: &[u8], offset: i64, little: bool) -> u16 {
    pr_imaging::tiff::read_u16(data, i32::try_from(offset).unwrap_or(i32::MAX), little)
}

/// As [`read_u16_at`] for `pr_imaging::tiff::read_u32`.
fn read_u32_at(data: &[u8], offset: i64, little: bool) -> u32 {
    pr_imaging::tiff::read_u32(data, i32::try_from(offset).unwrap_or(i32::MAX), little)
}
