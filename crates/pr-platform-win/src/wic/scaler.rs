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
    /// Any other WIC failure (C# `Marshal.GetExceptionForHR` type, see [`ScaleError::dotnet_kind`]); `hresult` is signed.
    Com { hresult: i32 },
    /// The destination buffer could not be allocated or read (same-size copy path included).
    Pixels(PixelBufferError),
}

impl fmt::Display for ScaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disposed => f.write_str("Cannot access a disposed object (PixelBuffer)."),
            Self::WidthOutOfRange { width } => write!(f, "Width must be at least 1 (was {width})."),
            Self::HeightOutOfRange { height } => write!(f, "Height must be at least 1 (was {height})."),
            Self::TooLarge { byte_count } => {
                write!(f, "Pixel buffer is too large for WIC ({byte_count} bytes).")
            }
            Self::InsufficientMemory { .. } => f.write_str("Not enough memory to scale the decoded image."),
            Self::Com { hresult } => write!(f, "WIC call failed (0x{:08X}).", *hresult as u32),
            Self::Pixels(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ScaleError {}

impl ScaleError {
    /// The .NET exception type C# throws for a raw WIC failure: `Marshal.GetExceptionForHR` maps the HRESULT, so a `Com`
    /// error is not always a `COMException` (0x80070216 gives `ArithmeticException`, E_NOTIMPL `NotImplementedException`,
    /// E_POINTER `NullReferenceException`). `None` for the variants that are not a raw WIC failure.
    pub fn dotnet_kind(&self) -> Option<crate::wic::interop::DotnetExceptionKind> {
        match self {
            Self::Com { hresult } => Some(
                crate::wic::interop::exception_kind_for_hresult(*hresult)
                    .unwrap_or(crate::wic::interop::DotnetExceptionKind::Com),
            ),
            Self::InsufficientMemory { .. } => Some(crate::wic::interop::DotnetExceptionKind::OutOfMemory),
            _ => None,
        }
    }
}

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
    if source_disposed {
        return Err(ScaleError::Disposed);
    }
    if width < 1 {
        return Err(ScaleError::WidthOutOfRange { width });
    }
    if height < 1 {
        return Err(ScaleError::HeightOutOfRange { height });
    }
    if source_byte_count > i64::from(u32::MAX) {
        return Err(ScaleError::TooLarge {
            byte_count: source_byte_count,
        });
    }
    if width == source_width && height == source_height {
        Ok(ResizePlan::Copy)
    } else {
        Ok(ResizePlan::Wic)
    }
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
    match plan_resize(
        source.is_disposed(),
        source.width(),
        source.height(),
        source.byte_count(),
        width,
        height,
    )? {
        ResizePlan::Copy => copy_same_size(source),
        ResizePlan::Wic => native::scale(source, width, height),
    }
}

/// The `ResizePlan::Copy` body: allocate `width x height` with the layout of `source`, copy `height` rows of `stride` bytes.
fn copy_same_size(source: &PixelBuffer) -> Result<PixelBuffer, ScaleError> {
    let mut copy =
        PixelBuffer::allocate(source.width(), source.height(), source.layout()).map_err(ScaleError::Pixels)?;
    // Whole-buffer copy: both buffers have the same stride (`width * 4`, no padding), so this is the row-by-row copy of C#.
    let from = source.bytes().map_err(ScaleError::Pixels)?;
    let to = copy.bytes_mut().map_err(ScaleError::Pixels)?;
    let count = from.len().min(to.len());
    to[..count].copy_from_slice(&from[..count]);
    Ok(copy)
}

pub(crate) mod native {
    use super::{PixelBuffer, ScaleError};
    use crate::wic::cache_image::{E_OUTOFMEMORY, source_format};
    use crate::wic::interop::{self, Com, WICBitmapInterpolationMode, WicError};
    use windows::Win32::Graphics::Imaging::{IWICBitmap, WICBitmapInterpolationModeFant};

    fn wic_error(error: &WicError) -> ScaleError {
        let hresult = error.hresult();
        if hresult == E_OUTOFMEMORY {
            ScaleError::InsufficientMemory { hresult }
        } else {
            ScaleError::Com { hresult }
        }
    }

    fn com_error(error: &windows_core::Error) -> ScaleError {
        wic_error(&WicError::from_windows(error))
    }

    /// Steps of the C# WIC branch (objects released by guards on every path, reverse order: scaler, bitmap, factory):
    /// 1. `WICCreateImagingFactory_Proxy(WINCODEC_SDK_VERSION1)` (`hr < 0` or null => `E_FAIL`).
    /// 2. `CreateBitmapFromMemory(source.width, source.height, source_format(layout), stride, byte_count, address)`: WIC
    ///    cannot borrow the caller's memory, so this is the one copy of the source.
    /// 3. `CreateBitmapScaler`; `Initialize(bitmap, width, height, WICBitmapInterpolationModeFant)`.
    /// 4. `PixelBuffer::allocate(width, height, layout)` (AFTER the scaler is ready, as C#), then
    ///    `scaler.CopyPixels(null rect, stride, byte_count, address)` straight into it.
    /// 5. A COM error with `E_OUTOFMEMORY` => `ScaleError::InsufficientMemory`; any other => `ScaleError::Com`.
    pub(crate) fn scale(source: &PixelBuffer, width: i32, height: i32) -> Result<PixelBuffer, ScaleError> {
        // Drop order is the reverse of declaration: scaler, bitmap, factory (C# `finally` order).
        let factory = interop::create_factory().map_err(|e| wic_error(&e))?;
        let format = crate::wic::cache_image::guid_to_windows(&source_format(source.layout()));
        let bytes = source.bytes().map_err(ScaleError::Pixels)?;
        // SAFETY: `factory` is a live `IWICImagingFactory`; `format` outlives the call; `bytes` is the source buffer slice
        // (`byte_count <= u32::MAX` was checked by `plan_resize`, so WIC's 32-bit sizes hold) and is only read, WIC copies it.
        let bitmap_result: windows_core::Result<IWICBitmap> = unsafe {
            factory.CreateBitmapFromMemory(
                source.width() as u32,
                source.height() as u32,
                &format,
                source.stride() as u32,
                bytes,
            )
        };
        let bitmap = Com::adopt(bitmap_result).map_err(|e| wic_error(&e))?;
        let scaler = interop::create_bitmap_scaler(&factory).map_err(|e| wic_error(&e))?;
        let mode: WICBitmapInterpolationMode = WICBitmapInterpolationModeFant;
        // SAFETY: `scaler` and `bitmap` are live COM objects owned by guards in this frame; the bitmap is a valid
        // `IWICBitmapSource` and the size arguments are positive (`plan_resize`).
        unsafe { scaler.Initialize(&*bitmap, width as u32, height as u32, mode) }.map_err(|e| com_error(&e))?;

        // Allocated only now, like C#; a failure below drops it (the C# `finally { pixels?.Dispose(); }`).
        let mut destination = PixelBuffer::allocate(width, height, source.layout()).map_err(ScaleError::Pixels)?;
        let stride = destination.stride() as u32;
        let target = destination.bytes_mut().map_err(ScaleError::Pixels)?;
        // SAFETY: `scaler` is initialised; `target` is exactly `stride * height` writable bytes owned by `destination`, which
        // outlives the call; a null rect means the whole image.
        unsafe { scaler.CopyPixels(std::ptr::null(), stride, target) }.map_err(|e| com_error(&e))?;
        Ok(destination)
    }
}
