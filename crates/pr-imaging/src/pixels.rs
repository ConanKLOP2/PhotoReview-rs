//! Port of `PhotoReview.Imaging/Pixels/PixelBuffer.cs` and `Pixels/PixelOps.cs` (C# baseline d92d6aae):
//! [`PixelBuffer`] (4 bytes per pixel, native, 64-byte aligned, uninitialised after allocation, unique `id`) and the
//! managed pixel operations in this module ([`apply_orientation`], [`has_alpha`], [`is_fully_opaque`]).
//!
//! Target path: `crates/pr-imaging/src/pixels.rs` (lead adds `pub mod pixels;` to `lib.rs`).
//!
//! Ownership model (replaces `IDisposable`): a `PixelBuffer` has exactly one owner and frees its memory on `Drop`.
//! Sharing is `Arc<PixelBuffer>` (read-only), which is how W4-01b/W4-01c carry it. The C# `Dispose()` stays as
//! [`PixelBuffer::dispose`] (early release, idempotent) because C# callers and tests observe `IsDisposed` and
//! `ObjectDisposedException`; every accessor of a disposed buffer returns [`PixelBufferError::Disposed`].
//!
//! No `unsafe` in this file: all raw memory handling lives in `NativePixelMemory` (W4-16b, `pr_imaging::pixel_memory`).
//!
//! PARITY: I-18 (orientation baked into pixels; this is the pixel loop), D-06 (the alpha flag of the preview cache
//! header is `has_alpha`), INV-1 (the `id` is part of the GPU cache key).
#![allow(unused_variables, dead_code)] // D11: only while a body is `todo!()`; the porter removes this line.

use std::fmt;
use std::sync::atomic::AtomicI64;

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
        todo!("0 => Bgr32, 1 => Pbgra32, else UnknownLayout(value)")
    }

    /// The C# enum value (0 or 1).
    pub fn as_i32(self) -> i32 {
        todo!("self as i32")
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
        todo!("English texts of the C# exceptions")
    }
}

impl std::error::Error for PixelBufferError {}

impl PixelBufferError {
    /// The .NET exception type name: `ArgumentOutOfRangeException`, `OutOfMemoryException` or `ObjectDisposedException`.
    pub fn dotnet_type_name(&self) -> &'static str {
        todo!("see the variant docs")
    }
}

static LAST_ID: AtomicI64 = AtomicI64::new(0);

/// `false` exactly when `byte_count > i32::MAX` (C# `TryGetSpan`). Public so the 2 GiB boundary is testable without
/// allocating 2 GiB.
pub fn fits_in_span(byte_count: i64) -> bool {
    todo!("byte_count <= i32::MAX as i64")
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

impl PixelBuffer {
    /// C# `Allocate(width, height, layout)`. Checks in this order: width, height; then one allocation of
    /// `width * 4 * height` bytes; the `id` is taken only after the allocation succeeded (ids need only be unique and
    /// increasing). The C# unknown-layout check is [`PixelLayout::from_i32`] in Rust.
    pub fn allocate(width: i32, height: i32, layout: PixelLayout) -> Result<Self, PixelBufferError> {
        todo!("range checks, NativePixelMemory::allocate, LAST_ID.fetch_add(1) + 1")
    }

    pub fn width(&self) -> i32 {
        todo!("self.width")
    }

    pub fn height(&self) -> i32 {
        todo!("self.height")
    }

    /// `width * 4`, no padding.
    pub fn stride(&self) -> i32 {
        todo!("self.stride")
    }

    pub fn layout(&self) -> PixelLayout {
        todo!("self.layout")
    }

    /// `stride as i64 * height as i64`. Still readable after dispose.
    pub fn byte_count(&self) -> i64 {
        todo!("self.byte_count")
    }

    /// Unique in the process, increasing from 1 (the GPU image cache key).
    pub fn id(&self) -> i64 {
        todo!("self.id")
    }

    pub fn is_disposed(&self) -> bool {
        todo!("self.memory.is_none()")
    }

    /// First byte, aligned to 64. `Err(Disposed)` after dispose. The pointer is valid while the buffer is alive and
    /// not disposed; dereferencing it is the caller's `unsafe` (D2D / WIC upload code in `pr-platform-win`).
    pub fn address(&self) -> Result<*const u8, PixelBufferError> {
        todo!("memory.as_ptr()")
    }

    /// Row `y` (`stride` bytes). `Err(Disposed)` is reported before `Err(RowOutOfRange)` (C# order).
    pub fn get_row(&self, y: i32) -> Result<&[u8], PixelBufferError> {
        todo!("slice [y*stride .. (y+1)*stride]")
    }

    /// Writable row; same checks. Replaces the writable `Span<byte>` of C# `GetRow`.
    pub fn get_row_mut(&mut self, y: i32) -> Result<&mut [u8], PixelBufferError> {
        todo!("slice [y*stride .. (y+1)*stride]")
    }

    /// The whole buffer (`byte_count` bytes), whatever its size (Rust slices are not limited to 2 GiB).
    pub fn bytes(&self) -> Result<&[u8], PixelBufferError> {
        todo!("memory.as_slice()")
    }

    pub fn bytes_mut(&mut self) -> Result<&mut [u8], PixelBufferError> {
        todo!("memory.as_mut_slice()")
    }

    /// C# `TryGetSpan`: `Ok(None)` when `byte_count > i32::MAX` (parity for callers that feed 32-bit length APIs such
    /// as WIC `CopyPixels`), `Ok(Some(all bytes))` otherwise, `Err(Disposed)` after dispose.
    pub fn try_get_span(&self) -> Result<Option<&[u8]>, PixelBufferError> {
        todo!("disposed check, fits_in_span, bytes")
    }

    /// Mutable twin of [`PixelBuffer::try_get_span`].
    pub fn try_get_span_mut(&mut self) -> Result<Option<&mut [u8]>, PixelBufferError> {
        todo!("disposed check, fits_in_span, bytes_mut")
    }

    /// Frees the memory now; calling it again is a no-op. Geometry, `layout`, `byte_count` and `id` stay readable.
    pub fn dispose(&mut self) {
        todo!("self.memory = None")
    }

    /// Test hook for the C# `UnsafeAccessor` constructor: a buffer of any valid geometry over an unallocated block, to
    /// test the 2 GiB branches without allocating. Every slice accessor of such a buffer is empty and it is not
    /// disposed. Not for production code.
    #[doc(hidden)]
    pub fn unallocated_for_tests(width: i32, height: i32, layout: PixelLayout) -> Self {
        todo!("same fields as allocate, NativePixelMemory::unallocated()")
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
    todo!("see the contract, semantics 4 to 7")
}

/// `layout == Pbgra32 && !is_fully_opaque(px)`; `Err(Disposed)` for a disposed buffer (checked first).
pub fn has_alpha(px: &PixelBuffer) -> Result<bool, PixelBufferError> {
    todo!("disposed check, layout check, is_fully_opaque")
}

/// Every alpha byte is 255; always `true` for `Bgr32` (its fourth byte is ignored). Row by row, stops at the first
/// pixel whose byte 3 is below 255. `Err(Disposed)` for a disposed buffer (checked first, also for `Bgr32`).
pub fn is_fully_opaque(px: &PixelBuffer) -> Result<bool, PixelBufferError> {
    todo!("per-row scan of byte 3 of every pixel")
}

/// Orientations 2..=4: destination row `y` is source row `y` (2) or `height-1-y` (3, 4), mirrored per PIXEL (not per
/// byte) for 2 and 3.
fn copy_rows(src: &PixelBuffer, dst: &mut PixelBuffer, orientation: i32) {
    todo!("row copy, reverse the 4-byte pixels of the row when mirrored")
}

/// Orientations 5..=8, tiled by [`TRANSPOSE_TILE`]. Destination `(x, y)` takes source pixel index
/// `origin + x * step_x + y * step_y` (source `W x H`, destination `H x W`):
/// 5 transpose `src(y, x)`; 6 rotate 90 CW `src(y, H-1-x)`; 7 transverse `src(W-1-y, H-1-x)`; 8 rotate 270 CW
/// `src(W-1-y, x)`.
fn copy_transposed(src: &PixelBuffer, dst: &mut PixelBuffer, orientation: i32) {
    todo!("origin/step table, tile loops, 4-byte pixel copies")
}
