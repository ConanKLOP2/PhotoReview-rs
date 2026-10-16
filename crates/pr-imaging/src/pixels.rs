//! Port of `PhotoReview.Imaging/Pixels/PixelBuffer.cs` and `Pixels/PixelOps.cs` (C# baseline d92d6aae):
//! [`PixelBuffer`] (4 bytes per pixel, native, 64-byte aligned, uninitialised after allocation, unique `id`) and the
//! managed pixel operations in this module ([`apply_orientation`], [`has_alpha`], [`is_fully_opaque`]).
//!
//! Ownership model (replaces `IDisposable`): a `PixelBuffer` has exactly one owner and frees its memory on `Drop`.
//! Sharing is `Arc<PixelBuffer>` (read-only), which is how W4-01b/W4-01c carry it. The C# `Dispose()` stays as
//! [`PixelBuffer::dispose`] (early release, idempotent) because C# callers and tests observe `IsDisposed` and
//! `ObjectDisposedException`; every accessor of a disposed buffer returns [`PixelBufferError::Disposed`].
//!
//! Allocation and freeing live in `NativePixelMemory` (W4-16b, `pr_imaging::pixel_memory`), which hands out only a raw
//! pointer. The only `unsafe` of this file are the two slice views over that block (`block_slice`, `block_slice_mut`),
//! each with one SAFETY comment; they are the single place where a `&[u8]` is made from the block.
//!
//! PARITY: I-18 (orientation baked into pixels; this is the pixel loop), D-06 (the alpha flag of the preview cache
//! header is `has_alpha`), INV-1 (the `id` is part of the GPU cache key).

use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};

/// Bytes per pixel of every layout (C# `BytesPerPixel`).
pub const BYTES_PER_PIXEL: i32 = 4;

/// Side of the square tile of the transposing copy (orientations 5 to 8): 32 x 32 pixels, 4 KB in and 4 KB out.
const TRANSPOSE_TILE: usize = 32;

use crate::pixel_memory::NativePixelMemory;

/// Byte order of a pixel in memory: B, G, R, then X (Bgr32, unused) or A (Pbgra32). C# `PixelLayout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelLayout {
    /// `GUID_WICPixelFormat32bppBGR` / DXGI `B8G8R8A8_UNORM` with the fourth byte ignored.
    Bgr32 = 0,
    /// `GUID_WICPixelFormat32bppPBGRA`: premultiplied alpha in byte 3.
    Pbgra32 = 1,
}

impl PixelLayout {
    /// C# `(PixelLayout)n`: 0 and 1 only; anything else is [`PixelBufferError::UnknownLayout`].
    pub fn from_i32(value: i32) -> Result<Self, PixelBufferError> {
        match value {
            0 => Ok(Self::Bgr32),
            1 => Ok(Self::Pbgra32),
            other => Err(PixelBufferError::UnknownLayout(other)),
        }
    }

    /// The C# enum value (0 or 1).
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

/// Every failure of this module. The .NET exception type each variant stands for is given by
/// [`PixelBufferError::dotnet_type_name`] (the golden test compares these names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PixelBufferError {
    /// `width < 1` or `width > i32::MAX / 4` (the stride must fit `i32`). `ArgumentOutOfRangeException`.
    WidthOutOfRange { width: i32 },
    /// `height < 1`. `ArgumentOutOfRangeException`.
    HeightOutOfRange { height: i32 },
    /// A layout value that is neither 0 nor 1. `ArgumentOutOfRangeException`.
    UnknownLayout(i32),
    /// The system refused `byte_count` bytes. `OutOfMemoryException`.
    OutOfMemory { byte_count: i64 },
    /// The buffer was disposed. `ObjectDisposedException`.
    Disposed,
    /// `y < 0 || y >= height` in [`PixelBuffer::get_row`]. `ArgumentOutOfRangeException`.
    RowOutOfRange { y: i32, height: i32 },
}

impl fmt::Display for PixelBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WidthOutOfRange { width } => {
                let max = i32::MAX / BYTES_PER_PIXEL;
                if *width < 1 {
                    write!(
                        f,
                        "width ('{width}') must be greater than or equal to '1'. (Parameter 'width')\nActual value was {width}."
                    )
                } else {
                    write!(
                        f,
                        "width ('{width}') must be less than or equal to '{max}'. (Parameter 'width')\nActual value was {width}."
                    )
                }
            }
            Self::HeightOutOfRange { height } => write!(
                f,
                "height ('{height}') must be greater than or equal to '1'. (Parameter 'height')\nActual value was {height}."
            ),
            Self::UnknownLayout(value) => {
                write!(
                    f,
                    "Unknown pixel layout. (Parameter 'layout')\nActual value was {value}."
                )
            }
            // .NET `OutOfMemoryException` default message (from `NativeMemory.AlignedAlloc`); the byte count is data of
            // the variant, not part of the text.
            Self::OutOfMemory { .. } => f.write_str("Insufficient memory to continue the execution of the program."),
            Self::Disposed => f.write_str(
                "Cannot access a disposed object.\r\nObject name: 'PhotoReview.Imaging.Pixels.PixelBuffer'.",
            ),
            Self::RowOutOfRange { y, height } => {
                if *y < 0 {
                    write!(
                        f,
                        "y ('{y}') must be a non-negative value. (Parameter 'y')\nActual value was {y}."
                    )
                } else {
                    write!(
                        f,
                        "y ('{y}') must be less than '{height}'. (Parameter 'y')\nActual value was {y}."
                    )
                }
            }
        }
    }
}

impl std::error::Error for PixelBufferError {}

impl PixelBufferError {
    /// The .NET exception type name: `ArgumentOutOfRangeException`, `OutOfMemoryException` or `ObjectDisposedException`.
    pub fn dotnet_type_name(&self) -> &'static str {
        match self {
            Self::WidthOutOfRange { .. }
            | Self::HeightOutOfRange { .. }
            | Self::UnknownLayout(_)
            | Self::RowOutOfRange { .. } => "ArgumentOutOfRangeException",
            Self::OutOfMemory { .. } => "OutOfMemoryException",
            Self::Disposed => "ObjectDisposedException",
        }
    }
}

static LAST_ID: AtomicI64 = AtomicI64::new(0);

/// `false` exactly when `byte_count > i32::MAX` (C# `TryGetSpan`). Public so the 2 GiB boundary is testable without
/// allocating 2 GiB.
pub fn fits_in_span(byte_count: i64) -> bool {
    byte_count <= i64::from(i32::MAX)
}

/// Native BGRA/BGRX pixel store. Contents are NOT initialised by [`PixelBuffer::allocate`] (the decoder overwrites all).
#[derive(Debug)]
pub struct PixelBuffer {
    width: i32,
    height: i32,
    layout: PixelLayout,
    stride: i32,
    byte_count: i64,
    id: i64,
    /// `None` after [`PixelBuffer::dispose`].
    memory: Option<NativePixelMemory>,
}

// Compile-time guard: the buffer is shared across threads as `Arc<PixelBuffer>`.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PixelBuffer>();
};

/// Shared view of the first `byte_count` bytes of `memory`; empty for an unallocated block.
fn block_slice(memory: &NativePixelMemory, byte_count: i64) -> &[u8] {
    let ptr = memory.as_mut_ptr();
    if ptr.is_null() || byte_count <= 0 {
        return &[];
    }
    // SAFETY: `ptr` is the non-null start of a live block of at least `byte_count` bytes (`PixelBuffer` allocates exactly
    // `byte_count` and only keeps `memory` while it is not disposed); the block is freed only by `memory`'s drop or
    // dispose, both of which need `&mut`/ownership of the `PixelBuffer` and so cannot overlap this borrow; writes go
    // through `block_slice_mut`, which needs `&mut PixelBuffer`, so no `&mut` alias exists while this `&` lives.
    unsafe { std::slice::from_raw_parts(ptr, byte_count as usize) }
}

/// Exclusive view of the first `byte_count` bytes of `memory`; empty for an unallocated block.
fn block_slice_mut(memory: &mut NativePixelMemory, byte_count: i64) -> &mut [u8] {
    let ptr = memory.as_mut_ptr();
    if ptr.is_null() || byte_count <= 0 {
        return &mut [];
    }
    // SAFETY: as in `block_slice`; additionally the caller holds `&mut` to the owning `PixelBuffer` (through
    // `memory: &mut`), so no other reference into the block exists for the lifetime of the returned slice.
    unsafe { std::slice::from_raw_parts_mut(ptr, byte_count as usize) }
}

impl PixelBuffer {
    /// C# `Allocate(width, height, layout)`. Checks in this order: width, height; then one allocation of
    /// `width * 4 * height` bytes; the `id` is taken only after the allocation succeeded (ids need only be unique and
    /// increasing). The C# unknown-layout check is [`PixelLayout::from_i32`] in Rust.
    pub fn allocate(width: i32, height: i32, layout: PixelLayout) -> Result<Self, PixelBufferError> {
        // C# order: width < 1, height < 1, then width > i32::MAX / 4 (so a huge width with a bad height reports height).
        if width < 1 {
            return Err(PixelBufferError::WidthOutOfRange { width });
        }
        if height < 1 {
            return Err(PixelBufferError::HeightOutOfRange { height });
        }
        if width > i32::MAX / BYTES_PER_PIXEL {
            return Err(PixelBufferError::WidthOutOfRange { width });
        }
        let stride = width * BYTES_PER_PIXEL;
        let byte_count = i64::from(stride) * i64::from(height);
        let memory =
            NativePixelMemory::allocate(byte_count).map_err(|_| PixelBufferError::OutOfMemory { byte_count })?;
        Ok(Self {
            width,
            height,
            layout,
            stride,
            byte_count,
            id: LAST_ID.fetch_add(1, Ordering::Relaxed) + 1,
            memory: Some(memory),
        })
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    /// `width * 4`, no padding.
    pub fn stride(&self) -> i32 {
        self.stride
    }

    pub fn layout(&self) -> PixelLayout {
        self.layout
    }

    /// `stride as i64 * height as i64`. Still readable after dispose.
    pub fn byte_count(&self) -> i64 {
        self.byte_count
    }

    /// Unique in the process, increasing from 1 (the GPU image cache key).
    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn is_disposed(&self) -> bool {
        self.memory.is_none()
    }

    /// First byte, aligned to 64. `Err(Disposed)` after dispose. The pointer is valid while the buffer is alive and
    /// not disposed; dereferencing it is the caller's `unsafe` (D2D / WIC upload code in `pr-platform-win`).
    pub fn address(&self) -> Result<*const u8, PixelBufferError> {
        match &self.memory {
            Some(memory) => Ok(memory.as_mut_ptr().cast_const()),
            None => Err(PixelBufferError::Disposed),
        }
    }

    fn row_range(&self, y: i32) -> Result<std::ops::Range<usize>, PixelBufferError> {
        if self.memory.is_none() {
            return Err(PixelBufferError::Disposed);
        }
        if y < 0 || y >= self.height {
            return Err(PixelBufferError::RowOutOfRange { y, height: self.height });
        }
        let stride = self.stride as usize;
        let start = y as usize * stride;
        Ok(start..start + stride)
    }

    /// Row `y` (`stride` bytes). `Err(Disposed)` is reported before `Err(RowOutOfRange)` (C# order).
    pub fn get_row(&self, y: i32) -> Result<&[u8], PixelBufferError> {
        let range = self.row_range(y)?;
        let all = self.bytes()?;
        // An unallocated test block is empty: its rows are empty too.
        Ok(all.get(range).unwrap_or(&[]))
    }

    /// Writable row; same checks. Replaces the writable `Span<byte>` of C# `GetRow`.
    pub fn get_row_mut(&mut self, y: i32) -> Result<&mut [u8], PixelBufferError> {
        let range = self.row_range(y)?;
        let all = self.bytes_mut()?;
        Ok(all.get_mut(range).unwrap_or(&mut []))
    }

    /// The whole buffer (`byte_count` bytes), whatever its size (Rust slices are not limited to 2 GiB).
    pub fn bytes(&self) -> Result<&[u8], PixelBufferError> {
        match &self.memory {
            Some(memory) => Ok(block_slice(memory, self.byte_count)),
            None => Err(PixelBufferError::Disposed),
        }
    }

    pub fn bytes_mut(&mut self) -> Result<&mut [u8], PixelBufferError> {
        let byte_count = self.byte_count;
        match &mut self.memory {
            Some(memory) => Ok(block_slice_mut(memory, byte_count)),
            None => Err(PixelBufferError::Disposed),
        }
    }

    /// C# `TryGetSpan`: `Ok(None)` when `byte_count > i32::MAX` (parity for callers that feed 32-bit length APIs such
    /// as WIC `CopyPixels`), `Ok(Some(all bytes))` otherwise, `Err(Disposed)` after dispose.
    pub fn try_get_span(&self) -> Result<Option<&[u8]>, PixelBufferError> {
        if self.is_disposed() {
            return Err(PixelBufferError::Disposed);
        }
        if !fits_in_span(self.byte_count) {
            return Ok(None);
        }
        self.bytes().map(Some)
    }

    /// Mutable twin of [`PixelBuffer::try_get_span`].
    pub fn try_get_span_mut(&mut self) -> Result<Option<&mut [u8]>, PixelBufferError> {
        if self.is_disposed() {
            return Err(PixelBufferError::Disposed);
        }
        if !fits_in_span(self.byte_count) {
            return Ok(None);
        }
        self.bytes_mut().map(Some)
    }

    /// Frees the memory now; calling it again is a no-op. Geometry, `layout`, `byte_count` and `id` stay readable.
    pub fn dispose(&mut self) {
        self.memory = None;
    }

    /// Test hook for the C# `UnsafeAccessor` constructor: a buffer of any valid geometry over an unallocated block, to
    /// test the 2 GiB branches without allocating. Every slice accessor of such a buffer is empty and it is not
    /// disposed. Not for production code. Hidden from the docs because it is public only so the integration tests
    /// (a separate crate) can reach it.
    #[doc(hidden)]
    pub fn unallocated_for_tests(width: i32, height: i32, layout: PixelLayout) -> Self {
        let stride = width.saturating_mul(BYTES_PER_PIXEL);
        Self {
            width,
            height,
            layout,
            stride,
            byte_count: i64::from(width) * i64::from(BYTES_PER_PIXEL) * i64::from(height),
            id: LAST_ID.fetch_add(1, Ordering::Relaxed) + 1,
            memory: Some(NativePixelMemory::unallocated()),
        }
    }
}

/// `apply_orientation` failed; the caller still owns `source` (C#: "if it throws, src is NOT disposed").
#[derive(Debug)]
pub struct ApplyOrientationError {
    pub error: PixelBufferError,
    pub source: PixelBuffer,
}

/// C# `PixelOps.ApplyOrientation(src, exifOrientation)`.
///
/// * `src` already disposed: `Err` (`Disposed`), even for orientation 1. Checked first.
/// * orientation outside 2..=8 (0, 1, 9, negatives, `i32::MIN/MAX`): `Ok(src)` unchanged, same `id`.
/// * 2..=4: new `width x height` buffer; 5..=8: new `height x width` buffer; same layout; `src` is dropped (disposed)
///   after the copy. The result has a NEW `id`.
/// * destination allocation failure: `Err` carrying `src` back, untouched.
pub fn apply_orientation(src: PixelBuffer, exif_orientation: i32) -> Result<PixelBuffer, ApplyOrientationError> {
    if src.is_disposed() {
        return Err(ApplyOrientationError {
            error: PixelBufferError::Disposed,
            source: src,
        });
    }
    if !(2..=8).contains(&exif_orientation) {
        return Ok(src);
    }
    let (dst_width, dst_height) = if exif_orientation >= 5 {
        (src.height, src.width)
    } else {
        (src.width, src.height)
    };
    let mut dst = match PixelBuffer::allocate(dst_width, dst_height, src.layout) {
        Ok(dst) => dst,
        Err(error) => return Err(ApplyOrientationError { error, source: src }),
    };
    if exif_orientation >= 5 {
        copy_transposed(&src, &mut dst, exif_orientation);
    } else {
        copy_rows(&src, &mut dst, exif_orientation);
    }
    drop(src);
    Ok(dst)
}

/// `layout == Pbgra32 && !is_fully_opaque(px)`; `Err(Disposed)` for a disposed buffer (checked first).
pub fn has_alpha(px: &PixelBuffer) -> Result<bool, PixelBufferError> {
    if px.is_disposed() {
        return Err(PixelBufferError::Disposed);
    }
    if px.layout != PixelLayout::Pbgra32 {
        return Ok(false);
    }
    Ok(!is_fully_opaque(px)?)
}

/// Every alpha byte is 255; always `true` for `Bgr32` (its fourth byte is ignored). Row by row, stops at the first
/// pixel whose byte 3 is below 255. `Err(Disposed)` for a disposed buffer (checked first, also for `Bgr32`).
pub fn is_fully_opaque(px: &PixelBuffer) -> Result<bool, PixelBufferError> {
    let bytes = px.bytes()?;
    if px.layout != PixelLayout::Pbgra32 {
        return Ok(true);
    }
    let stride = px.stride as usize;
    if stride == 0 {
        return Ok(true);
    }
    Ok(bytes
        .chunks_exact(stride)
        .all(|row| row.chunks_exact(4).all(|pixel| pixel[3] == 255)))
}

/// Orientations 2..=4: destination row `y` is source row `y` (2) or `height-1-y` (3, 4), mirrored per PIXEL (not per
/// byte) for 2 and 3.
fn copy_rows(src: &PixelBuffer, dst: &mut PixelBuffer, orientation: i32) {
    let stride = src.stride as usize;
    let height = src.height as usize;
    let (Ok(source), Ok(target)) = (src.bytes(), dst.bytes_mut()) else {
        return;
    };
    let mirror_columns = orientation == 2 || orientation == 3;
    let flip_rows = orientation == 3 || orientation == 4;
    for y in 0..height {
        let src_y = if flip_rows { height - 1 - y } else { y };
        let from = &source[src_y * stride..(src_y + 1) * stride];
        let to = &mut target[y * stride..(y + 1) * stride];
        if mirror_columns {
            for (dst_pixel, src_pixel) in to.chunks_exact_mut(4).zip(from.chunks_exact(4).rev()) {
                dst_pixel.copy_from_slice(src_pixel);
            }
        } else {
            to.copy_from_slice(from);
        }
    }
}

/// Orientations 5..=8, tiled by [`TRANSPOSE_TILE`]. Destination `(x, y)` takes source pixel index
/// `origin + x * step_x + y * step_y` (source `W x H`, destination `H x W`):
/// 5 transpose `src(y, x)`; 6 rotate 90 CW `src(y, H-1-x)`; 7 transverse `src(W-1-y, H-1-x)`; 8 rotate 270 CW
/// `src(W-1-y, x)`.
fn copy_transposed(src: &PixelBuffer, dst: &mut PixelBuffer, orientation: i32) {
    let w = i64::from(src.width);
    let h = i64::from(src.height);
    let (origin, step_x, step_y): (i64, i64, i64) = match orientation {
        5 => (0, w, 1),
        6 => ((h - 1) * w, -w, 1),
        7 => ((h - 1) * w + w - 1, -w, -1),
        8 => (w - 1, w, -1),
        _ => return,
    };
    let (Ok(source), Ok(target)) = (src.bytes(), dst.bytes_mut()) else {
        return;
    };
    let dst_width = src.height as usize; // H
    let dst_height = src.width as usize; // W
    let mut tile_y = 0;
    while tile_y < dst_height {
        let end_y = (tile_y + TRANSPOSE_TILE).min(dst_height);
        let mut tile_x = 0;
        while tile_x < dst_width {
            let end_x = (tile_x + TRANSPOSE_TILE).min(dst_width);
            for y in tile_y..end_y {
                for x in tile_x..end_x {
                    let src_index = origin + x as i64 * step_x + y as i64 * step_y;
                    let from = src_index as usize * 4;
                    let to = (y * dst_width + x) * 4;
                    target[to..to + 4].copy_from_slice(&source[from..from + 4]);
                }
            }
            tile_x = end_x;
        }
        tile_y = end_y;
    }
}
