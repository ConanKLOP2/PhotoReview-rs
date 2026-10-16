//! Port of `PhotoReview.Imaging.Raw/Bmff/BmffBoxNavigator.cs` (C# baseline d92d6aae): parser and walker for ISO-BMFF boxes.
//! Hostile-input safe: overflow-free size arithmetic, a cap on the children returned per box ([`MAX_CHILD_BOXES`]) and bounds
//! checks against the source length. The CR3 reader (W5-10b) walks a fixed, shallow `moov/uuid/trak` path with these two
//! functions; it never recurses.
//!
//! The C# class is static with `bool TryReadBox(source, offset, out box, containerEnd = -1)` and
//! `List<BmffBox> ReadChildBoxes(source, in parent, payloadSkip = 0)`. The Rust twins return `Result<Option<_>, _>` /
//! `Result<Vec<_>, _>`: `Ok(None)` is C# `false` (EOF or invalid structure), `Err` is an exception that escaped the C# method, which
//! can only come from `source.read` (budget exhausted, I/O failure, `InvalidDataException` of `IRawHeaderSource`). Neither function
//! panics, whatever the bytes or the (even hostile) `BmffBox` handed to [`read_child_boxes`].

#![allow(unused_variables, dead_code)] // D11: removed together with the last `todo!()`.

use crate::contracts::{RawHeaderError, RawHeaderSource};

/// `BmffBoxNavigator.MaxChildBoxes`: immediate children returned by [`read_child_boxes`]; further children are ignored.
pub const MAX_CHILD_BOXES: usize = 256;

/// `BmffBoxNavigator.BmffBox` (a C# `readonly record struct`). `i64` everywhere, exactly as C# `long`: a hostile caller can
/// build a `BmffBox` with negative or `i64::MAX` fields and [`read_child_boxes`] must survive it (golden `parent-*` cases).
///
/// `box_type` is `Encoding.ASCII.GetString` of the four type bytes: every byte above 0x7F becomes `'?'` (so the Rust `String`
/// is always 4 ASCII chars; compare with `==` against `"moov"`, `"uuid"`, ...). `uuid` is `Some` only for a parsed `"uuid"` box
/// (the 16-byte extended type read after the regular header); C# compares the array by reference in `==`, Rust by content, which is
/// what every caller (`u.SequenceEqual(..)`) wants.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BmffBox {
    pub box_type: String,
    /// File offset of the box header.
    pub offset: i64,
    /// Header + payload, in bytes.
    pub total_size: i64,
    /// File offset of the first payload byte (after the 8/16-byte header and, for `uuid`, the 16-byte extended type).
    pub payload_offset: i64,
    pub payload_size: i64,
    pub uuid: Option<[u8; 16]>,
}

impl BmffBox {
    /// `new BmffBox(type, offset, totalSize, payloadOffset, payloadSize)`: `uuid` defaults to `None` (the C# tests build hostile
    /// parents this way).
    pub fn new(
        box_type: impl Into<String>,
        offset: i64,
        total_size: i64,
        payload_offset: i64,
        payload_size: i64,
    ) -> Self {
        todo!()
    }
}

/// `BmffBoxNavigator.TryReadBox`: reads the box header at `offset`.
///
/// `Ok(Some(box))` for a valid box; `Ok(None)` on EOF or invalid structure (offset outside `0..=length-8`, a short read, a 64-bit
/// `largesize` above `i64::MAX` or cut off by the end of the source, a total size below its header size or past the source end
/// (compared overflow-free as `total_size > length - offset`), a `uuid` box too small for its 16-byte extended type).
///
/// `container_end`: size 0 means "to the end of the enclosing container": `Some(end)` (clamped to the source length) or `None`
/// for a top-level box (end of the source). C# uses `-1` as "unspecified" and tests `containerEnd >= 0`, so `Some(negative)` behaves
/// as `None` here. `Some(0)` is a real, empty container: a size-0 box then ends before it starts and the result is `Ok(None)`.
pub fn try_read_box(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    container_end: Option<i64>,
) -> Result<Option<BmffBox>, RawHeaderError> {
    todo!()
}

/// `BmffBoxNavigator.ReadChildBoxes`: the immediate children inside `parent`'s payload, at most [`MAX_CHILD_BOXES`].
///
/// `payload_skip` skips a fixed-size prefix of the payload before the first child (Canon's preview `uuid` box has an 8-byte one);
/// negative or larger than the payload gives an empty list. Never fails for malformed sizes (enumeration just stops: a child that
/// cannot be parsed, or whose declared size exceeds the parent's payload, ends the walk and is not returned); `Err` only when
/// `source.read` fails. Children returned before such an error are lost, like the C# exception losing its half-built list.
/// The payload limit is `payload_offset + min(payload_size, length - payload_offset)`, so a parent claiming `i64::MAX` bytes is
/// clamped to the source; `payload_offset < 0`, `payload_offset > length` or `payload_size < 0` give an empty list.
pub fn read_child_boxes(
    source: &mut dyn RawHeaderSource,
    parent: &BmffBox,
    payload_skip: i32,
) -> Result<Vec<BmffBox>, RawHeaderError> {
    todo!()
}
