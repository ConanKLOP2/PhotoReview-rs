//! Port of `PhotoReview.Imaging/Decoding/ExifOrientation.cs` and the transform table of
//! `PhotoReview.Imaging.Wpf/WpfExifOrientation.cs` (C# baseline d92d6aae).
//!
//! * [`normalize`] / [`is_transposed`]: the EXIF orientation value rules (1..=8 valid, anything else is 1).
//! * [`read_from_wic`]: the two-query read order (EXIF IFD0 tag, then the Windows photo policy). A damaged tag never
//!   fails a decode: any failure is orientation 1.
//! * [`create_transform`] / [`map_pixel`] / [`oriented_size`]: the pure replacement of `WpfExifOrientation.CreateTransform`
//!   (no WPF in Rust). The pixel loop itself is `pixels::apply_orientation` (W4-16a), which uses this table as its
//!   definition of correct.
//!
//! PARITY: I-18 (orientation baked into pixels).
#![allow(unused_variables, dead_code)] // D11: removed with the last `todo!()`.

/// EXIF IFD0 orientation tag (274) inside a JPEG APP1 block: the WIC metadata query that reads it.
pub const EXIF_ORIENTATION_QUERY: &str = "/app1/ifd/{ushort=274}";

/// Photo-metadata policy name WIC maps onto the container's own orientation (TIFF, HEIF, ...).
pub const WINDOWS_ORIENTATION_QUERY: &str = "System.Photo.Orientation";

/// A metadata query that could not be answered (WIC raised any exception or returned an unreadable shape).
/// [`read_from_wic`] maps it to orientation 1; allocation failure is not representable here (Rust aborts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryFailed;

/// Geometric operation that takes a stored image to its upright display, as WPF composed it:
/// optional mirror(s) FIRST, then a clockwise rotation (screen coordinates, y down).
/// Orientation 1 (and any invalid value) is [`OrientationTransform::IDENTITY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrientationTransform {
    /// `ScaleTransform(-1, 1)`: left-right mirror.
    pub flip_horizontal: bool,
    /// `ScaleTransform(1, -1)`: top-bottom mirror.
    pub flip_vertical: bool,
    /// `RotateTransform(angle)`: one of 0, 90, 180, 270, clockwise, applied after the mirrors.
    pub rotate_clockwise_degrees: u16,
}

impl OrientationTransform {
    /// No-op (`Transform.Identity`).
    pub const IDENTITY: Self = Self {
        flip_horizontal: false,
        flip_vertical: false,
        rotate_clockwise_degrees: 0,
    };
}

/// True for the orientations (5 to 8) that rotate by 90 degrees and so swap width and height. C#: `IsTransposed`.
pub fn is_transposed(orientation: i32) -> bool {
    todo!("(5..=8).contains(&orientation)")
}

/// The orientation itself when it is a valid EXIF value (1 to 8); 1 (normal) for anything else. C#: `Normalize`.
pub fn normalize(orientation: i32) -> i32 {
    todo!("1..=8 kept, else 1")
}

/// Reads the orientation through a metadata query function (C# `ReadFromWic(Func<string, object?>)`).
///
/// `query(name)` answers one WIC metadata query already reduced to an integer with the W4-04b `exif_query::as_integer`
/// rule (first element of an array; `Ok(None)` for an absent tag or a shape that is not an integer). Order, exactly as
/// C#: ask [`EXIF_ORIENTATION_QUERY`]; only when that value is not in 1..=8 ask [`WINDOWS_ORIENTATION_QUERY`]; a final
/// value outside 1..=8, an absent tag or `Err(QueryFailed)` from EITHER query gives 1. The second query is never asked
/// when the first yields a valid value.
pub fn read_from_wic<F>(query: F) -> i32
where
    F: FnMut(&str) -> Result<Option<i64>, QueryFailed>,
{
    todo!("two queries, EXIF first, any Err => 1")
}

/// The transform table (C# `WpfExifOrientation.CreateTransform`): 2 flip-h; 3 rot 180; 4 flip-v; 5 flip-h then rot 270;
/// 6 rot 90; 7 flip-h then rot 90; 8 rot 270; 1 and every other value the identity.
pub fn create_transform(orientation: i32) -> OrientationTransform {
    todo!("table")
}

/// Destination `(x, y)` of source pixel `(x, y)` in a `width` x `height` image after the EXIF orientation
/// (the `Expected` function of C# `ExifOrientationTests`): 2 `(w-1-x, y)`, 3 `(w-1-x, h-1-y)`, 4 `(x, h-1-y)`,
/// 5 `(y, x)`, 6 `(h-1-y, x)`, 7 `(h-1-y, w-1-x)`, 8 `(y, w-1-x)`; 1 and invalid values map to `(x, y)`.
/// Preconditions: `x < width`, `y < height`.
pub fn map_pixel(orientation: i32, x: u32, y: u32, width: u32, height: u32) -> (u32, u32) {
    todo!("table")
}

/// `(width, height)` after the orientation: swapped for [`is_transposed`] values, unchanged otherwise.
pub fn oriented_size(orientation: i32, width: u32, height: u32) -> (u32, u32) {
    todo!("swap when transposed")
}
