//! Port of `PhotoReview.Imaging/Pixels/WicPixelScaler.cs` (C# baseline d92d6aae): the final shrink of an already decoded
//! [`PixelBuffer`] by WIC's Fant scaler (`IWICBitmapScaler`, `WICBitmapInterpolationModeFant`), the fine-scale step of the
//! TurboJPEG pixel path (its only C# caller, `TurboJpegDecoder.FinishWithCodec`; WIC Direct and the cache reader build their own
//! scalers inside their pipelines). PARITY: see contracts/drafts-w4/W4-11e.
//!
//! Target path: `crates/pr-platform-win/src/wic/scaler.rs` (lead adds `pub mod scaler;` to
//! `crates/pr-platform-win/src/wic/mod.rs`).
//!
//! The argument checks and the same-size copy are pure; only [`native::scale`] is `unsafe` (W4-11a wrappers, one
//! `// SAFETY:` per block, every COM pointer released by guards on every path).
//!
//! `PixelBuffer` / `PixelLayout` come from `pr_imaging::pixels` (re-exported by `cache_image.rs`).
#![allow(unused_variables, dead_code)] // D11: only while a body is `todo!()`; the porter removes this line.

use std::fmt;

use super::cache_image::{PixelBuffer, PixelBufferError};

/// Errors of [`resize`]. The .NET exception each variant stands for is in its doc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScaleError {
    /// The source was disposed (`ObjectDisposedException`). Checked FIRST, also for a same-size request.
    Disposed,
    /// `width < 1` (`ArgumentOutOfRangeException`). Checked before `height`.
    WidthOutOfRange { width: i32 },
    /// `height < 1` (`ArgumentOutOfRangeException`).
    HeightOutOfRange { height: i32 },
    /// `source.byte_count() > u32::MAX` (`ArgumentException`, "Pixel buffer is too large for WIC.").
    TooLarge { byte_count: i64 },
    /// WIC reported `E_OUTOFMEMORY` (`InsufficientMemoryException`, an `OutOfMemoryException`): the caller's "too large for
    /// the available memory" verdict applies, never a raw COM error.
    InsufficientMemory { hresult: i32 },
    /// Any other WIC failure (C# raw `COMException`); `hresult` is signed.
    Com { hresult: i32 },
    /// The destination buffer could not be allocated or read (same-size copy path included).
    Pixels(PixelBufferError),
}

impl fmt::Display for ScaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!(
            "English texts of the C# exceptions; InsufficientMemory => \"Not enough memory to scale the decoded image.\""
        )
    }
}

impl std::error::Error for ScaleError {}

/// Which way [`resize`] produces the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizePlan {
    /// `width == source.width && height == source.height`: a plain row-by-row copy into a new buffer, WIC is not called.
    Copy,
    /// Everything else (shrink, enlargement, one axis only): `CreateBitmapFromMemory` + Fant scaler + `CopyPixels`.
    Wic,
}

/// The managed checks of C# `Resize`, in the C# order: disposed, `width < 1`, `height < 1`, `byte_count > u32::MAX`; then the
/// plan. (The C# `ArgumentNullException` has no Rust twin: a reference cannot be null.)
pub fn plan_resize(
    source_disposed: bool,
    source_width: i32,
    source_height: i32,
    source_byte_count: i64,
    width: i32,
    height: i32,
) -> Result<ResizePlan, ScaleError> {
    todo!()
}

/// C# `WicPixelScaler.Resize`: a NEW `width x height` buffer with the layout of `source`; the caller owns it and `source`
/// is only read (never disposed, never mutated).
///
/// * Meant for shrinking; a same-size request is a plain copy (a distinct buffer with its own `id` and address) and an
///   enlargement is allowed.
/// * `Bgr32`: the unused fourth byte of the result is whatever WIC writes (not part of the contract); `Pbgra32`: alpha is
///   kept and the colour stays premultiplied.
/// * Exactly one buffer is allocated (the result); on any error nothing is leaked (the destination guard drops it).
pub fn resize(source: &PixelBuffer, width: i32, height: i32) -> Result<PixelBuffer, ScaleError> {
    todo!("plan_resize, then Copy => copy_same_size, Wic => native::scale")
}

/// The `ResizePlan::Copy` body: allocate `width x height` with the layout of `source`, copy `height` rows of `stride` bytes.
fn copy_same_size(source: &PixelBuffer) -> Result<PixelBuffer, ScaleError> {
    todo!()
}

pub(crate) mod native {
    use super::{PixelBuffer, ScaleError};

    /// Steps of the C# WIC branch (objects released by guards on every path, reverse order: scaler, bitmap, factory):
    /// 1. `WICCreateImagingFactory_Proxy(WINCODEC_SDK_VERSION1)` (`hr < 0` or null => `E_FAIL`).
    /// 2. `CreateBitmapFromMemory(source.width, source.height, source_format(layout), stride, byte_count, address)`: WIC
    ///    cannot borrow the caller's memory, so this is the one copy of the source.
    /// 3. `CreateBitmapScaler`; `Initialize(bitmap, width, height, WICBitmapInterpolationModeFant)`.
    /// 4. `PixelBuffer::allocate(width, height, layout)` (AFTER the scaler is ready, as C#), then
    ///    `scaler.CopyPixels(null rect, stride, byte_count, address)` straight into it.
    /// 5. A COM error with `E_OUTOFMEMORY` => `ScaleError::InsufficientMemory`; any other => `ScaleError::Com`.
    pub(crate) fn scale(source: &PixelBuffer, width: i32, height: i32) -> Result<PixelBuffer, ScaleError> {
        todo!("W4-11a wrappers; see the doc comment for the exact call order")
    }
}
