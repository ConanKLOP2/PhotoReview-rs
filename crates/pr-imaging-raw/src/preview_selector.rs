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
//!
//! ASSUMED sibling names (the lead reconciles the `use` lines at merge, nothing else changes):
//! `crate::contracts::{EmbeddedPreview, EmbeddedPreviewKind, PreviewColorSpace, RawHeaderError, RawHeaderSource}` (W5-01b)
//! and `crate::formats::limits::{MAX_JPEG_MARKER_SEGMENTS, MAX_JPEG_FILL_BYTES}` (W5-01a, both `i32`).
#![allow(unused_variables, unused_imports, dead_code)] // D11: stub bodies are todo!(); the porter removes this line with the last todo!().

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
    todo!("W5-11b: select_preview_resolved(..).map(|s| s.chosen)")
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
    todo!("W5-11b")
}

/// C# `SelectPreviewCore`. Returns the INDEX of the chosen preview in `state` (every walked preview has already been
/// written back into `state[i]`), `None` for an empty list.
///
/// * empty -> `None`; no JPEG preview -> index 0 (unreachable while [`EmbeddedPreviewKind`] has one variant, keep the
///   branch so a second variant ports 1:1); exactly one JPEG -> [`resolve_dimensions`] on it and return it (no box test).
/// * several: JPEG candidates in `state` order, stable-sorted by `length` DESCENDING; [`resolve_dimensions`] on EVERY one in
///   that order (known sizes cost nothing), writing each result back to its `state` slot.
/// * `request_box.is_unbounded()` -> best by (`score`, `length`) descending, ties -> first in the length-descending order.
/// * else candidates where the visible sides cover the box (sides swapped when `pr_imaging::orientation::is_transposed`),
///   one-axis boxes test one axis; smallest by (`score`, `length`) ascending, ties -> first; none fit -> as unbounded.
fn select_preview_core(
    source: &mut dyn RawHeaderSource,
    state: &mut [EmbeddedPreview],
    request_box: DecodeBox,
    orientation: i32,
) -> Result<Option<usize>, RawHeaderError> {
    todo!("W5-11b")
}

/// C# `Score`: `(tier, value)` compared lexicographically. Known size (`width > 0 && height > 0`): tier 1 when
/// `max(width, height) >= 1000`, else 0, value = `width * height` in `i64`. Unknown size: `(0, if length <= 0 { 0 } else
/// { length.wrapping_mul(4) })` (wraps for lengths above `i64::MAX / 4`, e.g. `i64::MAX` scores `(0, -4)`; keep it).
fn score(preview: &EmbeddedPreview) -> (i32, i64) {
    todo!("W5-11b")
}

/// C# `ResolveDimensions`. Returns `preview` unchanged when `header_resolved` or both sides are positive, or when
/// `offset < 0 || offset >= source.length() || length < 4`. Otherwise walks (`walk_jpeg_header(.., need_frame = true)`):
/// * `Ok(walk)` -> a copy with `width`/`height` from the frame when present (else kept), `color_space` from the walk when
///   not `Unknown` (else kept), `header_resolved = true`. ALSO when no frame was found (lossless/truncated): the colour
///   space the walk saw is final.
/// * `Err(InvalidData { io: None, .. })` (budget exhausted, past end, unreadable range) -> `preview` unchanged and NOT
///   marked resolved, so a later call may retry.
/// * any other `Err` (an I/O failure; also `ArgumentOutOfRange`/`Canceled`, which C# does not catch) -> propagated.
fn resolve_dimensions(
    source: &mut dyn RawHeaderSource,
    preview: &EmbeddedPreview,
) -> Result<EmbeddedPreview, RawHeaderError> {
    todo!("W5-11b")
}

/// C# `ResolveColorSpace` (only for the CHOSEN preview): returns it unchanged when `header_resolved`, `color_space !=
/// Unknown`, `offset < 0 || offset >= source.length()` or `length < 4`. Otherwise `walk_jpeg_header(.., need_frame =
/// false)` (stops at the frame header) and returns a copy with the colour space (kept when the walk found none) and
/// `header_resolved = true`. Same error mapping as [`resolve_dimensions`]: non-I/O `InvalidData` -> unchanged, the rest
/// propagates.
fn resolve_color_space(
    source: &mut dyn RawHeaderSource,
    preview: &EmbeddedPreview,
) -> Result<EmbeddedPreview, RawHeaderError> {
    todo!("W5-11b")
}

/// `PreviewSelector.TryReadJpegFrame(source, offset, length, out width, out height, out colorSpace)`: `Ok(Some(frame))` when
/// the range `[offset, offset + length)` starts with SOI and its first frame header is SOF0/1/2 with non-zero size.
/// `Ok(None)` for everything else that is not an error (no SOI, lossless/arithmetic/hierarchical frame, malformed or
/// truncated segment, segment cap, fill cap, `length < 4`, `offset` outside the source). Does NOT check the sample
/// precision (unlike the W5-02b `JpegMarkerProbe`). Unlike the probe it does NOT swallow source errors: budget exhaustion
/// and I/O failures are `Err` (callers such as `reconcile_jpeg_size` and [`resolve_dimensions`] decide).
///
/// # Errors
/// Whatever `source.read` returns.
pub fn try_read_jpeg_frame(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
) -> Result<Option<JpegFrame>, RawHeaderError> {
    todo!("W5-11b: walk_jpeg_header(.., true) then map frame + color_space")
}

/// C# private `WalkJpeg(source, offset, length, needFrame, ..)`, public here so integration tests and the golden can see
/// the colour space of a walk that found no frame. The loop is the spec (see the contract, "Semantics" 8):
/// range check, SOI, then up to `MAX_JPEG_MARKER_SEGMENTS` iterations with the fill-byte / standalone / terminator /
/// length / SOF / APP1 rules. `need_frame == false` stops at the first frame-header marker (colour-space-only walk).
///
/// # Errors
/// Whatever `source.read` returns.
pub fn walk_jpeg_header(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
    need_frame: bool,
) -> Result<JpegHeaderWalk, RawHeaderError> {
    todo!("W5-11b")
}

/// C# `IsAdobeRgbExif` (`internal`; `pub` + `#[doc(hidden)]` here because integration tests and the fuzz twin call it, and
/// W4's lesson: cross-crate/test symbols are `pub` from the start). True when `payload` ("Exif\0\0" + TIFF) has
/// IFD0 -> 0x8769 (LONG x 1) -> Exif IFD -> 0xA005 (LONG x 1) -> Interop IFD -> 0x0001 ASCII, count 4, inline `"R03\0"`.
/// Anything malformed is `false`; never panics.
#[doc(hidden)]
#[must_use]
pub fn is_adobe_rgb_exif(payload: &[u8]) -> bool {
    todo!("W5-11b")
}

/// C# `TryFindLongEntry`: the entry for `tag` in the IFD at `ifd_offset` must be type 4 (LONG) with count 1; returns its
/// value (the pointed-to IFD offset, `u32` widened to `i64`).
fn try_find_long_entry(tiff: &[u8], ifd_offset: i64, tag: u16, little: bool) -> Option<i64> {
    todo!("W5-11b")
}

/// C# `TryFindEntry`: `ifd_offset < 8 || ifd_offset + 2 > tiff.len()` -> `None`; scans the `count` (u16) entries, `None` as
/// soon as one entry would end past `tiff.len()`, otherwise the offset of the FIRST entry with `tag`.
fn try_find_entry(tiff: &[u8], ifd_offset: i64, tag: u16, little: bool) -> Option<i64> {
    todo!("W5-11b")
}

/// `TiffStructure.ReadU16` at an `i64` offset that the callers already bounds-checked (a value that does not fit `i32` is
/// outside any real payload and reads as 0, like the shared reader's out-of-range fallback). Use
/// `pr_imaging::tiff::read_u16`.
fn read_u16_at(data: &[u8], offset: i64, little: bool) -> u16 {
    todo!("W5-11b")
}

/// As [`read_u16_at`] for `pr_imaging::tiff::read_u32`.
fn read_u32_at(data: &[u8], offset: i64, little: bool) -> u32 {
    todo!("W5-11b")
}
