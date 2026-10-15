//! Port of `PhotoReview.Imaging/Decoding/Wic/WicInterop.cs` (C# baseline d92d6aae), plus the one factory
//! creation that C# duplicates in `WicDirectDecoder.CreateFactory`/`ThrowIfFactoryFailed` and
//! `WicImageEncoder.CreateFactory`.
//!
//! The C# file hand-declares every WIC interface with `[GeneratedComInterface]`. In Rust the `windows` crate
//! (P-22) supplies the interfaces, vtables, GUIDs and enums from the Windows metadata, so this module keeps only
//! what the crate does not give with C# semantics:
//! - [`guids`]: the GUID constants C# names (pinned against the C# values by the `wic-interop-constants` golden);
//! - [`Com`]: an owned, counted COM reference (C# `WicCom.Wrap/WrapOrNull/Release/OutstandingOnThisThread`),
//!   which also turns the `windows` crate's "S_OK with a null out object" error (HRESULT 0) into C#'s `E_FAIL`;
//! - the `WicComExtensions` helpers (one function per C# extension method);
//! - [`create_factory`] / [`check_factory_result`] (`WICCreateImagingFactory_Proxy`, no `CoInitialize` needed);
//! - [`ComStream`]: the COM `IStream` over a Rust stream (C# `ManagedIStream`);
//! - [`exception_kind_for_hresult`]: which .NET exception `Marshal.GetExceptionForHR(hr, -1)` makes of a failing
//!   HRESULT. Every C# WIC call throws through that mapping, and the fallback whitelist (W4-12b, INV-12) decides on
//!   the exception TYPE, so the Rust decoders (W4-11c..e) need the same classification.
//!
//! All `unsafe` of the WIC port lives in `pr_platform_win::wic`; every block carries a `// SAFETY:` comment.

// Stub: bodies are todo!() until W4-11a is ported; the porter removes this line.
#![allow(dead_code, unused_variables, clippy::missing_safety_doc)]

use std::any::Any;
use std::cell::Cell;
use std::ffi::c_void;
use std::fmt;
use std::fs::File;
use std::io::{self, Cursor, SeekFrom};
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::{Arc, Mutex};

use windows::Win32::System::Com::{
    ISequentialStream_Impl, IStream, IStream_Impl, LOCKTYPE, STATFLAG, STATSTG, STGC, STREAM_SEEK,
};
use windows_core::{ComObject, HRESULT, Interface, implement};

pub use windows::Win32::Graphics::Imaging::{
    IWICBitmapDecoder, IWICBitmapFlipRotator, IWICBitmapFrameDecode, IWICBitmapScaler, IWICBitmapSource,
    IWICBitmapSourceTransform, IWICColorContext, IWICColorTransform, IWICFormatConverter, IWICImagingFactory,
    IWICMetadataQueryReader, IWICStream, WICBitmapDitherType, WICBitmapInterpolationMode, WICBitmapPaletteType,
    WICBitmapTransformOptions, WICColorContextType, WICDecodeOptions,
};
pub use windows_core::GUID;

/// `WicNativeMethods.WINCODEC_SDK_VERSION1`: the SDK version passed to `WICCreateImagingFactory_Proxy`.
pub const WINCODEC_SDK_VERSION1: u32 = 0x0236;

/// `E_FAIL`: what C# throws for an out object WIC returned as S_OK + null (`WicCom.Wrap`, `CreateFactory`).
pub const E_FAIL: i32 = 0x8000_4005_u32 as i32;
/// `E_NOINTERFACE`: the HRESULT a failed `QueryInterface` returns (C# `InvalidCastException`).
pub const E_NOINTERFACE: i32 = 0x8000_4002_u32 as i32;
/// `E_INVALIDARG` (= .NET `COR_E_ARGUMENT`): HRESULT of a C# `ArgumentException` crossing the `IStream` CCW.
pub const E_INVALIDARG: i32 = 0x8007_0057_u32 as i32;
/// `WINCODEC_ERR_PROPERTYNOTFOUND`: a missing metadata tag (`IWICMetadataQueryReader::GetMetadataByName`, which
/// C# declares `[PreserveSig]` so that a missing tag is not an exception; W4-11d tests for this code).
pub const WINCODEC_ERR_PROPERTYNOTFOUND: i32 = 0x8898_2F40_u32 as i32;
/// `WINCODEC_ERR_NOTINITIALIZED` (used by the factory-failure tests).
pub const WINCODEC_ERR_NOTINITIALIZED: i32 = 0x8898_2F0C_u32 as i32;
/// `COR_E_IO`: HRESULT of a plain C# `IOException` (e.g. `MemoryStream` seek before the beginning).
pub const COR_E_IO: i32 = 0x8013_1620_u32 as i32;
/// `COR_E_NOTSUPPORTED`: HRESULT of a C# `NotSupportedException` (`CopyTo`, `Clone`, write to a read-only stream).
pub const COR_E_NOTSUPPORTED: i32 = 0x8013_1515_u32 as i32;
/// `COR_E_OBJECTDISPOSED`: HRESULT of a C# `ObjectDisposedException` (stream used after `Dispose`).
pub const COR_E_OBJECTDISPOSED: i32 = 0x8013_1622_u32 as i32;
/// `COR_E_ARGUMENTOUTOFRANGE`: HRESULT of a C# `ArgumentOutOfRangeException` (`SetSize` above `i64::MAX`).
pub const COR_E_ARGUMENTOUTOFRANGE: i32 = 0x8013_1502_u32 as i32;

/// `WicGuids`: the GUIDs C# declares (pixel formats and container formats). Same values as the `windows` crate
/// constants of the same name; kept here so the C# list (and its "opaque formats" subset used by W4-11c) is one place.
pub mod guids {
    use windows_core::GUID;

    /// `GUID_WICPixelFormat32bppBGRA`.
    pub const PIXEL_FORMAT_32BPP_BGRA: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc90f);
    /// `GUID_WICPixelFormat32bppBGR`.
    pub const PIXEL_FORMAT_32BPP_BGR: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc90e);
    /// `GUID_WICPixelFormat32bppPBGRA`.
    pub const PIXEL_FORMAT_32BPP_PBGRA: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc910);
    /// `GUID_ContainerFormatJpeg`.
    pub const CONTAINER_FORMAT_JPEG: GUID = GUID::from_u128(0x19e4a5aa_5662_4fc5_a0c0_1758028e1057);
    /// `GUID_ContainerFormatTiff`.
    pub const CONTAINER_FORMAT_TIFF: GUID = GUID::from_u128(0x163bcc30_e2e9_4f0b_961d_a3e9fdb788a3);

    // Pixel formats WIC defines without an alpha channel (C# comment: anything not listed is treated as
    // potentially transparent and decoded to premultiplied BGRA, see WicDirectDecoder / W4-11c).
    /// `GUID_WICPixelFormatBlackWhite`.
    pub const PIXEL_FORMAT_BLACK_WHITE: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc905);
    /// `GUID_WICPixelFormat2bppGray`.
    pub const PIXEL_FORMAT_2BPP_GRAY: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc906);
    /// `GUID_WICPixelFormat4bppGray`.
    pub const PIXEL_FORMAT_4BPP_GRAY: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc907);
    /// `GUID_WICPixelFormat8bppGray`.
    pub const PIXEL_FORMAT_8BPP_GRAY: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc908);
    /// `GUID_WICPixelFormat16bppGray`.
    pub const PIXEL_FORMAT_16BPP_GRAY: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc90b);
    /// `GUID_WICPixelFormat16bppBGR555`.
    pub const PIXEL_FORMAT_16BPP_BGR555: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc909);
    /// `GUID_WICPixelFormat16bppBGR565`.
    pub const PIXEL_FORMAT_16BPP_BGR565: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc90a);
    /// `GUID_WICPixelFormat24bppBGR`.
    pub const PIXEL_FORMAT_24BPP_BGR: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc90c);
    /// `GUID_WICPixelFormat24bppRGB`.
    pub const PIXEL_FORMAT_24BPP_RGB: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc90d);
    /// `GUID_WICPixelFormat48bppRGB`.
    pub const PIXEL_FORMAT_48BPP_RGB: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc915);
    /// `GUID_WICPixelFormat48bppBGR`.
    pub const PIXEL_FORMAT_48BPP_BGR: GUID = GUID::from_u128(0xe605a384_b468_46ce_bb2e_36f180e64313);
    /// `GUID_WICPixelFormat32bppCMYK`.
    pub const PIXEL_FORMAT_32BPP_CMYK: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc91c);
    /// `GUID_WICPixelFormat64bppCMYK`.
    pub const PIXEL_FORMAT_64BPP_CMYK: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc91f);

    /// Every constant above as (C# field name, value), in C# declaration order (golden `wic-interop-constants`).
    pub const ALL: [(&str, GUID); 18] = [
        ("GUID_WICPixelFormat32bppBGRA", PIXEL_FORMAT_32BPP_BGRA),
        ("GUID_WICPixelFormat32bppBGR", PIXEL_FORMAT_32BPP_BGR),
        ("GUID_WICPixelFormat32bppPBGRA", PIXEL_FORMAT_32BPP_PBGRA),
        ("GUID_ContainerFormatJpeg", CONTAINER_FORMAT_JPEG),
        ("GUID_ContainerFormatTiff", CONTAINER_FORMAT_TIFF),
        ("GUID_WICPixelFormatBlackWhite", PIXEL_FORMAT_BLACK_WHITE),
        ("GUID_WICPixelFormat2bppGray", PIXEL_FORMAT_2BPP_GRAY),
        ("GUID_WICPixelFormat4bppGray", PIXEL_FORMAT_4BPP_GRAY),
        ("GUID_WICPixelFormat8bppGray", PIXEL_FORMAT_8BPP_GRAY),
        ("GUID_WICPixelFormat16bppGray", PIXEL_FORMAT_16BPP_GRAY),
        ("GUID_WICPixelFormat16bppBGR555", PIXEL_FORMAT_16BPP_BGR555),
        ("GUID_WICPixelFormat16bppBGR565", PIXEL_FORMAT_16BPP_BGR565),
        ("GUID_WICPixelFormat24bppBGR", PIXEL_FORMAT_24BPP_BGR),
        ("GUID_WICPixelFormat24bppRGB", PIXEL_FORMAT_24BPP_RGB),
        ("GUID_WICPixelFormat48bppRGB", PIXEL_FORMAT_48BPP_RGB),
        ("GUID_WICPixelFormat48bppBGR", PIXEL_FORMAT_48BPP_BGR),
        ("GUID_WICPixelFormat32bppCMYK", PIXEL_FORMAT_32BPP_CMYK),
        ("GUID_WICPixelFormat64bppCMYK", PIXEL_FORMAT_64BPP_CMYK),
    ];
}

// ---------------------------------------------------------------------------------------------------------------
// HRESULT -> .NET exception type
// ---------------------------------------------------------------------------------------------------------------

/// The .NET exception type `Marshal.GetExceptionForHR(hr, (IntPtr)(-1))` creates for a failing HRESULT (the stale
/// thread `IErrorInfo` is ignored, as C# asks with `-1`). Only the TYPE matters for parity: messages come from the
/// OS (`FormatMessage`) and are never compared.
///
/// `#[non_exhaustive]`: the porter MAY add a variant when the `wic-hresult-exception` golden names a type that is
/// not listed here (and only then); [`DotnetExceptionKind::type_name`] must return the exact .NET full name.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DotnetExceptionKind {
    /// `System.Runtime.InteropServices.COMException`: every HRESULT .NET has no specific type for (all
    /// `WINCODEC_ERR_*`, `E_FAIL`, `E_UNEXPECTED`, ...). Fallbackable in C# (W4-12b "allowed codec errors").
    Com,
    /// `System.OutOfMemoryException` (`E_OUTOFMEMORY`, ...). Never fallbackable.
    OutOfMemory,
    /// `System.ArgumentException` (`E_INVALIDARG`).
    Argument,
    /// `System.ArgumentOutOfRangeException`.
    ArgumentOutOfRange,
    /// `System.InvalidCastException` (`E_NOINTERFACE`).
    InvalidCast,
    /// `System.NullReferenceException` (`E_POINTER` may instead map to `AccessViolationException` in CoreCLR: the golden decides).
    NullReference,
    /// `System.NotImplementedException` (`E_NOTIMPL`).
    NotImplemented,
    /// `System.NotSupportedException`.
    NotSupported,
    /// `System.InvalidOperationException`.
    InvalidOperation,
    /// `System.UnauthorizedAccessException` (`E_ACCESSDENIED`).
    UnauthorizedAccess,
    /// `System.IO.FileNotFoundException`. Never fallbackable.
    FileNotFound,
    /// `System.IO.DirectoryNotFoundException`. Never fallbackable.
    DirectoryNotFound,
    /// `System.IO.DriveNotFoundException`.
    DriveNotFound,
    /// `System.IO.PathTooLongException`.
    PathTooLong,
    /// `System.IO.EndOfStreamException`.
    EndOfStream,
    /// `System.IO.IOException`.
    Io,
    /// `System.IO.FileLoadException`.
    FileLoad,
    /// `System.OperationCanceledException`. Never fallbackable.
    OperationCanceled,
    /// `System.OverflowException`.
    Overflow,
    /// `System.ArithmeticException`.
    Arithmetic,
    /// `System.DivideByZeroException`.
    DivideByZero,
    /// `System.FormatException`.
    Format,
    /// `System.IndexOutOfRangeException`.
    IndexOutOfRange,
    /// `System.ArrayTypeMismatchException`.
    ArrayTypeMismatch,
    /// `System.BadImageFormatException`.
    BadImageFormat,
    /// `System.DllNotFoundException`.
    DllNotFound,
    /// `System.EntryPointNotFoundException`.
    EntryPointNotFound,
    /// `System.MissingMethodException`.
    MissingMethod,
    /// `System.ObjectDisposedException`.
    ObjectDisposed,
    /// `System.TimeoutException`.
    Timeout,
    /// `System.PlatformNotSupportedException`.
    PlatformNotSupported,
    /// `System.StackOverflowException`.
    StackOverflow,
    /// `System.Security.SecurityException`.
    Security,
    /// `System.SystemException`.
    System,
    /// `System.Exception` (`COR_E_EXCEPTION`).
    Exception,
}

impl DotnetExceptionKind {
    /// The .NET `Type.FullName` (what the golden records), e.g. `"System.Runtime.InteropServices.COMException"`.
    pub fn type_name(self) -> &'static str {
        todo!("W4-11a: one arm per variant")
    }

    /// C# `ex is ArgumentException` (the type or a .NET subclass, e.g. `ArgumentOutOfRangeException`). W4-11c has
    /// `catch (ArgumentException)` arms; the golden's `base_types` pins every `is_*` answer.
    pub fn is_argument(self) -> bool {
        todo!("W4-11a")
    }

    /// C# `ex is IOException` (`FileNotFound`, `DirectoryNotFound`, `PathTooLong`, `EndOfStream`, ... per golden).
    pub fn is_io(self) -> bool {
        todo!("W4-11a")
    }

    /// C# `ex is NotSupportedException` (also `PlatformNotSupportedException`).
    pub fn is_not_supported(self) -> bool {
        todo!("W4-11a")
    }

    /// C# `ex is InvalidOperationException` (also `ObjectDisposedException`).
    pub fn is_invalid_operation(self) -> bool {
        todo!("W4-11a")
    }

    /// C# `ex is ArithmeticException` (also `OverflowException`, `DivideByZeroException`).
    pub fn is_arithmetic(self) -> bool {
        todo!("W4-11a")
    }
}

/// C# `Marshal.GetExceptionForHR(hr, (IntPtr)(-1))?.GetType()`: `None` for a success code (`hr >= 0`), else the
/// exception kind. Table-driven (CoreCLR `rexcep.h` order: first matching row wins); the golden
/// `wic-hresult-exception` is the authority, every HRESULT in it must map exactly.
pub fn exception_kind_for_hresult(hr: i32) -> Option<DotnetExceptionKind> {
    todo!("W4-11a: table from the golden; unknown failing codes -> Com")
}

// ---------------------------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------------------------

/// A failed WIC/COM step. W4-11c..e convert it into `pr_imaging::decoder_traits::DecodeError` with
/// [`WicError::exception_kind`] (the conversion lives in W4-11c, the crate that depends on both).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WicError {
    /// A failing HRESULT (C#: the exception `Marshal.GetExceptionForHR(hresult, -1)` builds).
    Com {
        /// The HRESULT as the C# `Exception.HResult` (`i32`).
        hresult: i32,
    },
    /// `WicCom.Wrap<T>`: the object does not implement `interface` (C# `InvalidCastException` with the message
    /// `"The COM object does not implement {interface} (E_NOINTERFACE)."`), whatever HRESULT the `QueryInterface`
    /// returned and whatever `IErrorInfo` the thread holds.
    NoInterface {
        /// The C# interface name, e.g. `"IWICStream"`.
        interface: &'static str,
    },
}

impl WicError {
    /// `Exception.HResult` of the C# exception: `hresult`, or `E_NOINTERFACE` for [`WicError::NoInterface`].
    pub fn hresult(&self) -> i32 {
        todo!("W4-11a")
    }

    /// The .NET exception type C# would throw: `exception_kind_for_hresult(hresult)` (a failing code always maps; a
    /// non-failing code, impossible by construction, gives [`DotnetExceptionKind::Com`], never a panic),
    /// [`DotnetExceptionKind::InvalidCast`] for [`WicError::NoInterface`].
    pub fn exception_kind(&self) -> DotnetExceptionKind {
        todo!("W4-11a")
    }

    /// From a `windows` crate error: its `code()`, except code 0 (the crate's `Error::empty()`, returned when a
    /// method succeeded with a null out object) which becomes `E_FAIL` like C# `WicCom.Wrap`. Never reads the
    /// error's message (it may carry a stale thread `IErrorInfo`; C# passes `-1` to ignore it).
    pub fn from_windows(error: &windows_core::Error) -> Self {
        todo!("W4-11a")
    }
}

impl fmt::Display for WicError {
    /// `Com`: `"WIC call failed (0x{hresult:08X})"`; `NoInterface`: the C# `InvalidCastException` message.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W4-11a")
    }
}

impl std::error::Error for WicError {}

impl From<windows_core::Error> for WicError {
    fn from(error: windows_core::Error) -> Self {
        Self::from_windows(&error)
    }
}

/// Result of a WIC step.
pub type WicResult<T> = Result<T, WicError>;

// ---------------------------------------------------------------------------------------------------------------
// Com<T>: WicCom.Wrap / WrapOrNull / Release / OutstandingOnThisThread
// ---------------------------------------------------------------------------------------------------------------

thread_local! {
    /// `WicCom.t_outstanding` ([ThreadStatic]): wrappers created minus wrappers released on this thread.
    static OUTSTANDING: Cell<i32> = const { Cell::new(0) };
}

/// `WicCom.OutstandingOnThisThread`: [`Com`] values created on this thread and not yet dropped. Back to its
/// starting value after every complete call chain (the lifetime-balance tests of W4-11c..e assert it).
pub fn outstanding_on_this_thread() -> i32 {
    todo!("W4-11a")
}

/// One owned, counted COM reference (C# wrapper from `WicCom.Wrap<T>`, released by `WicCom.Release`). Dropping it
/// releases the reference at once (C# `FinalRelease`) and decrements the thread counter. Not `Clone` (one count per
/// wrapper; borrow the interface through `Deref` instead) and neither `Send` nor `Sync` (the counter is per thread;
/// C# runs every WIC call chain on one thread).
pub struct Com<T: Interface> {
    inner: T,
    _per_thread: PhantomData<*const ()>,
}

impl<T: Interface> Com<T> {
    /// `WicCom.Wrap<T>(nint)`: takes over ONE reference of `raw`. Null -> `Err(Com { E_FAIL })`. Otherwise
    /// `QueryInterface(T::IID)`; on failure `Err(NoInterface { interface })` and nothing is left referenced. The
    /// reference passed in is always released before returning (the wrapper holds the one the QI added).
    ///
    /// # Safety
    /// `raw` is null or a valid COM interface pointer that owns one reference the caller hands over.
    pub unsafe fn wrap(raw: *mut c_void, interface: &'static str) -> WicResult<Self> {
        todo!("W4-11a")
    }

    /// `WicCom.WrapOrNull<T>`: like [`Com::wrap`] but null -> `Ok(None)` (optional out object, e.g. a thumbnail).
    ///
    /// # Safety
    /// As [`Com::wrap`].
    pub unsafe fn wrap_or_none(raw: *mut c_void, interface: &'static str) -> WicResult<Option<Self>> {
        todo!("W4-11a")
    }

    /// Counts an interface the `windows` crate returned from an out-object method: `Ok(t)` -> counted `Com`;
    /// `Err` with code 0 (S_OK + null) -> `E_FAIL`; any other `Err` -> `Com { code }`.
    pub fn adopt(result: windows_core::Result<T>) -> WicResult<Self> {
        todo!("W4-11a")
    }

    /// Like [`Com::adopt`] for an OPTIONAL out object: `Err` with code 0 (S_OK + null) -> `Ok(None)`.
    pub fn adopt_optional(result: windows_core::Result<T>) -> WicResult<Option<Self>> {
        todo!("W4-11a")
    }

    /// C# `obj is U` on a generated wrapper: a `QueryInterface`. Any failure (E_NOINTERFACE or other) is `None`,
    /// never an error. A success is a new counted [`Com`].
    pub fn query<U: Interface>(&self) -> Option<Com<U>> {
        todo!("W4-11a")
    }

    /// The raw interface pointer (borrowed; no reference is added).
    pub fn as_raw(&self) -> *mut c_void {
        self.inner.as_raw()
    }

    /// `WicCom.Release(wrapper)`: gives the reference back now. Same as dropping.
    pub fn release(self) {
        drop(self);
    }
}

impl<T: Interface> Deref for Com<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T: Interface> Drop for Com<T> {
    /// Releases the reference (the inner `T` drop) and decrements the thread counter.
    fn drop(&mut self) {
        // Stub: the counter decrement is part of the port.
    }
}

impl<T: Interface> fmt::Debug for Com<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Com").field("raw", &self.inner.as_raw()).finish()
    }
}

/// Test seam (C# tests call `Marshal.AddRef`): adds a reference, returns the new count.
///
/// # Safety
/// `raw` is a valid, non-null COM interface pointer.
#[doc(hidden)]
pub unsafe fn add_ref_raw(raw: *mut c_void) -> u32 {
    todo!("W4-11a")
}

/// Test seam (C# tests call `Marshal.Release`): releases one reference, returns the remaining count.
///
/// # Safety
/// `raw` is a valid, non-null COM interface pointer on which the caller owns one reference.
#[doc(hidden)]
pub unsafe fn release_raw(raw: *mut c_void) -> u32 {
    todo!("W4-11a")
}

// ---------------------------------------------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------------------------------------------

/// Puts the calling thread in the COM multithreaded apartment once (thread-local guard), as the CLR has already
/// done for every thread a C# WIC call runs on: `CoInitializeEx(None, COINIT_MULTITHREADED)`; `S_OK`/`S_FALSE` ->
/// `CoUninitialize` in the guard's `Drop` at thread exit; `RPC_E_CHANGED_MODE` (already STA, e.g. the UI thread) ->
/// `Ok` with nothing to undo; any other failure -> `Err(Com { hr })`, retried on the next call. Rust addition.
pub fn ensure_com_initialized() -> WicResult<()> {
    todo!("W4-11a")
}

/// `WicDirectDecoder.CreateFactory` / `WicImageEncoder.CreateFactory`: [`ensure_com_initialized`], then
/// `WICCreateImagingFactory_Proxy(WINCODEC_SDK_VERSION1)` from `windowscodecs.dll` (not `CoCreateInstance`).
/// A failing HRESULT or a null factory goes through [`check_factory_result`]; a pointer returned together with a
/// failing HRESULT is released (no leak); success is wrapped with [`Com::wrap`].
pub fn create_factory() -> WicResult<Com<IWICImagingFactory>> {
    todo!("W4-11a")
}

/// `WicDirectDecoder.ThrowIfFactoryFailed(hr, factoryCreated)`: `hr < 0` -> `Err(Com { hr })` (even when a factory
/// came back); `hr >= 0` without a factory -> `Err(Com { E_FAIL })`; else `Ok(())`.
pub fn check_factory_result(hr: i32, factory_created: bool) -> WicResult<()> {
    todo!("W4-11a")
}

// ---------------------------------------------------------------------------------------------------------------
// WicComExtensions: out-object helpers (null -> E_FAIL, optional thumbnail -> None, every result counted)
// ---------------------------------------------------------------------------------------------------------------

/// `IWICImagingFactory::CreateDecoderFromStream(stream, vendor = NULL, options)`.
pub fn create_decoder_from_stream(
    factory: &IWICImagingFactory,
    stream: &IStream,
    options: WICDecodeOptions,
) -> WicResult<Com<IWICBitmapDecoder>> {
    todo!("W4-11a")
}

/// `IWICImagingFactory::CreateFormatConverter`.
pub fn create_format_converter(factory: &IWICImagingFactory) -> WicResult<Com<IWICFormatConverter>> {
    todo!("W4-11a")
}

/// `IWICImagingFactory::CreateBitmapScaler`.
pub fn create_bitmap_scaler(factory: &IWICImagingFactory) -> WicResult<Com<IWICBitmapScaler>> {
    todo!("W4-11a")
}

/// `IWICImagingFactory::CreateBitmapFlipRotator`.
pub fn create_bitmap_flip_rotator(factory: &IWICImagingFactory) -> WicResult<Com<IWICBitmapFlipRotator>> {
    todo!("W4-11a")
}

/// `IWICImagingFactory::CreateStream`.
pub fn create_stream(factory: &IWICImagingFactory) -> WicResult<Com<IWICStream>> {
    todo!("W4-11a")
}

/// `IWICImagingFactory::CreateColorContext`.
pub fn create_color_context(factory: &IWICImagingFactory) -> WicResult<Com<IWICColorContext>> {
    todo!("W4-11a")
}

/// `IWICImagingFactory::CreateColorTransformer`.
pub fn create_color_transformer(factory: &IWICImagingFactory) -> WicResult<Com<IWICColorTransform>> {
    todo!("W4-11a")
}

/// `IWICBitmapDecoder::GetFrame(index)`.
pub fn get_frame(decoder: &IWICBitmapDecoder, index: u32) -> WicResult<Com<IWICBitmapFrameDecode>> {
    todo!("W4-11a")
}

/// `IWICBitmapFrameDecode::GetMetadataQueryReader`.
pub fn get_metadata_query_reader(frame: &IWICBitmapFrameDecode) -> WicResult<Com<IWICMetadataQueryReader>> {
    todo!("W4-11a")
}

/// `IWICBitmapFrameDecode::GetThumbnail`: the embedded thumbnail; `Ok(None)` when the codec returns S_OK without
/// an object. A failing HRESULT (e.g. `WINCODEC_ERR_CODECNOTHUMBNAIL`) is `Err` as in C#.
pub fn get_thumbnail(frame: &IWICBitmapFrameDecode) -> WicResult<Option<Com<IWICBitmapSource>>> {
    todo!("W4-11a")
}

// ---------------------------------------------------------------------------------------------------------------
// ComStream: C# ManagedIStream
// ---------------------------------------------------------------------------------------------------------------

/// The stream a [`ComStream`] reads/writes (C# `System.IO.Stream`). One call per COM call: no retry loops
/// (C# calls `Stream.Read` once per `IStream::Read`).
pub trait StreamBackend: Send {
    /// `Stream.Read(Span<byte>)`: bytes read (0 at end), may be fewer than asked.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    /// `Stream.Write(ReadOnlySpan<byte>)`: all or error. Read-only backends fail with `ErrorKind::Unsupported`.
    fn write_all(&mut self, data: &[u8]) -> io::Result<()>;
    /// `Stream.Seek`: new absolute position. A target before 0 fails without moving.
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64>;
    /// `Stream.SetLength`. Read-only backends fail with `ErrorKind::Unsupported`.
    fn set_len(&mut self, len: u64) -> io::Result<()>;
    /// `Stream.Length`.
    fn len(&mut self) -> io::Result<u64>;
    /// `Stream.Length == 0` (clippy's `len_without_is_empty`).
    fn is_empty(&mut self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }
    /// `Stream.Flush`.
    fn flush(&mut self) -> io::Result<()>;
    /// For the owner to get the concrete backend back after [`ComStream::dispose`] (e.g. the encoded bytes).
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send>;
}

/// A file opened by the caller (C#: `FileStream` with share `ReadWrite | Delete`, W4-11c opens it). `set_len` also
/// moves the position to the new length when it was beyond it (.NET `FileStream.SetLength`); `flush` is
/// `Write::flush`, never `sync_all` (C# `Flush()`, not `Flush(true)`).
impl StreamBackend for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        todo!("W4-11a")
    }
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        todo!("W4-11a")
    }
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        todo!("W4-11a")
    }
    fn set_len(&mut self, len: u64) -> io::Result<()> {
        todo!("W4-11a")
    }
    fn len(&mut self) -> io::Result<u64> {
        todo!("W4-11a")
    }
    fn flush(&mut self) -> io::Result<()> {
        todo!("W4-11a")
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

/// A growable memory stream (C# `new MemoryStream()`, the encoders' output, W4-11e).
impl StreamBackend for Cursor<Vec<u8>> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        todo!("W4-11a")
    }
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        todo!("W4-11a")
    }
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        todo!("W4-11a")
    }
    fn set_len(&mut self, len: u64) -> io::Result<()> {
        todo!("W4-11a: grow with zeros / truncate; clamp the position to the new length (MemoryStream.SetLength)")
    }
    fn len(&mut self) -> io::Result<u64> {
        todo!("W4-11a")
    }
    fn flush(&mut self) -> io::Result<()> {
        todo!("W4-11a")
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

/// Read-only bytes (C# `new MemoryStream(bytes, writable: false)` / `ReadOnlyMemoryStreamFactory`, W4-16b):
/// write and set-length fail with `ErrorKind::Unsupported` (C# `NotSupportedException`).
impl StreamBackend for Cursor<Arc<[u8]>> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        todo!("W4-11a")
    }
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        todo!("W4-11a")
    }
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        todo!("W4-11a")
    }
    fn set_len(&mut self, len: u64) -> io::Result<()> {
        todo!("W4-11a")
    }
    fn len(&mut self) -> io::Result<u64> {
        todo!("W4-11a")
    }
    fn flush(&mut self) -> io::Result<()> {
        todo!("W4-11a")
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

/// Why a [`ComStream`] call failed; [`StreamFault::hresult`] is what the COM caller (WIC) sees, equal to the
/// `Exception.HResult` the C# `ManagedIStream` CCW returns (golden `wic-managed-istream`).
#[derive(Debug)]
pub enum StreamFault {
    /// Used after `dispose` (C# `ObjectDisposedException`, `COR_E_OBJECTDISPOSED`).
    Disposed,
    /// `CopyTo`, `Clone`, or a write/set-size on a read-only backend (C# `NotSupportedException`,
    /// `COR_E_NOTSUPPORTED`).
    NotSupported,
    /// Seek target before 0 (C# `IOException` "before the beginning", `COR_E_IO`); the position is unchanged.
    SeekBeforeBegin,
    /// `dwOrigin` not 0/1/2 (C# `ArgumentException` "Invalid seek origin", `E_INVALIDARG`).
    InvalidSeekOrigin(u32),
    /// `SetSize` above `i64::MAX` (C# `long` parameter is negative: `ArgumentOutOfRangeException`).
    SizeOutOfRange(u64),
    /// `Read`/`Write` with `cb > i32::MAX` (C# `int cb` is negative: `new Span<byte>(pv, cb)` throws
    /// `ArgumentOutOfRangeException`, `COR_E_ARGUMENTOUTOFRANGE`); `pv` is not touched.
    CountOutOfRange(u32),
    /// Any other backend error: `HRESULT_FROM_WIN32(raw_os_error)` when it has an OS code (C# `IOException.HResult`
    /// of a `FileStream` error), else `E_INVALIDARG` for `ErrorKind::InvalidInput` (a .NET `ArgumentException` crossing
    /// the CCW; W4-11c relies on it), else `COR_E_IO`. (`ErrorKind::Unsupported` never gets here: it is
    /// [`StreamFault::NotSupported`]; in `Seek`, `InvalidInput` / OS error 131 is [`StreamFault::SeekBeforeBegin`].)
    Io(io::Error),
}

impl StreamFault {
    /// The HRESULT the COM caller receives.
    pub fn hresult(&self) -> i32 {
        todo!("W4-11a")
    }
}

impl fmt::Display for StreamFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W4-11a")
    }
}

impl std::error::Error for StreamFault {}

/// Shared state of the COM object: the backend (`None` once disposed) and the last fault a COM call returned.
struct StreamState {
    backend: Option<Box<dyn StreamBackend>>,
    last_fault: Option<StreamFault>,
}

/// The COM object (`#[implement(IStream)]`). Its `IStream`/`ISequentialStream` methods call the safe
/// [`ComStream`] logic and turn a [`StreamFault`] into its HRESULT; they never panic (a panic across
/// `extern "system"` aborts the process): lock with `unwrap_or_else(PoisonError::into_inner)`, no `unwrap`.
#[implement(IStream)]
struct ComStreamObject {
    state: Mutex<StreamState>,
}

impl ISequentialStream_Impl for ComStreamObject_Impl {
    /// C# `Read(byte* pv, int cb, nint pcbRead)`: disposed, then `cb > i32::MAX` (`COR_E_ARGUMENTOUTOFRANGE`); one
    /// backend read into `pv[..cb]`; `*pcbRead = n` when not null; S_OK also for a short read or 0 at end (C# never
    /// returns S_FALSE).
    fn Read(&self, pv: *mut c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        todo!("W4-11a: SAFETY per pointer")
    }

    /// C# `Write`: disposed, then `cb > i32::MAX` (`COR_E_ARGUMENTOUTOFRANGE`, before the read-only check); writes all
    /// `cb` bytes; `*pcbWritten = cb` when not null.
    fn Write(&self, pv: *const c_void, cb: u32, pcbwritten: *mut u32) -> HRESULT {
        todo!("W4-11a: SAFETY per pointer")
    }
}

impl IStream_Impl for ComStreamObject_Impl {
    /// C# `Seek(dlibMove, dwOrigin, plibNewPosition)`: `*plibNewPosition` = new absolute position when not null.
    fn Seek(&self, dlibmove: i64, dworigin: STREAM_SEEK, plibnewposition: *mut u64) -> windows_core::Result<()> {
        todo!("W4-11a")
    }

    /// C# `SetSize` -> `Stream.SetLength`.
    fn SetSize(&self, libnewsize: u64) -> windows_core::Result<()> {
        todo!("W4-11a")
    }

    /// C# `CopyTo`: always `NotSupported` (also after dispose).
    fn CopyTo(
        &self,
        pstm: windows_core::Ref<IStream>,
        cb: u64,
        pcbread: *mut u64,
        pcbwritten: *mut u64,
    ) -> windows_core::Result<()> {
        todo!("W4-11a")
    }

    /// C# `Commit`: flushes; S_OK after dispose (WIC may commit after the owner disposed).
    fn Commit(&self, grfcommitflags: &STGC) -> windows_core::Result<()> {
        todo!("W4-11a")
    }

    /// C# `Revert`: no-op, S_OK (also after dispose).
    fn Revert(&self) -> windows_core::Result<()> {
        todo!("W4-11a")
    }

    /// C# `LockRegion`: no-op, S_OK (also after dispose).
    fn LockRegion(&self, liboffset: u64, cb: u64, dwlocktype: &LOCKTYPE) -> windows_core::Result<()> {
        todo!("W4-11a")
    }

    /// C# `UnlockRegion`: no-op, S_OK (also after dispose).
    fn UnlockRegion(&self, liboffset: u64, cb: u64, dwlocktype: u32) -> windows_core::Result<()> {
        todo!("W4-11a")
    }

    /// C# `Stat`: `type = STGTY_STREAM (2)`, `cbSize = length`, everything else 0 (name pointer null, `grfMode` 0 =
    /// `STGM_READ`); the flag is ignored. Zero all 80 bytes first (padding 12..16 is pinned by the golden), then
    /// write the fields. Errors are built with `Error::from_hresult` (never `Error::new` / `From<HRESULT>`).
    fn Stat(&self, pstatstg: *mut STATSTG, grfstatflag: &STATFLAG) -> windows_core::Result<()> {
        todo!("W4-11a")
    }

    /// C# `Clone`: always `NotSupported`.
    fn Clone(&self) -> windows_core::Result<IStream> {
        todo!("W4-11a")
    }
}

/// C# `ManagedIStream`: a COM `IStream` over a [`StreamBackend`], handed to WIC with [`ComStream::istream`].
/// The owner keeps the `ComStream`; dropping it disposes the backend (C# `using var stream = new ManagedIStream(..)`)
/// even if WIC still holds the COM object, whose later calls then fail with `COR_E_OBJECTDISPOSED`.
pub struct ComStream {
    object: ComObject<ComStreamObject>,
}

impl ComStream {
    /// `new ManagedIStream(stream)`.
    pub fn new(backend: Box<dyn StreamBackend>) -> Self {
        todo!("W4-11a")
    }

    /// Over an open file.
    pub fn from_file(file: File) -> Self {
        Self::new(Box::new(file))
    }

    /// Over read-only bytes, position 0.
    pub fn from_bytes(bytes: Arc<[u8]>) -> Self {
        Self::new(Box::new(Cursor::new(bytes)))
    }

    /// Over an empty growable memory stream (encoder output).
    pub fn writable_memory() -> Self {
        Self::new(Box::new(Cursor::new(Vec::new())))
    }

    /// The `IStream` interface for WIC (adds a COM reference).
    pub fn istream(&self) -> IStream {
        todo!("W4-11a")
    }

    /// `Read` without the pointers: bytes read.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, StreamFault> {
        todo!("W4-11a")
    }

    /// `Write` without the pointers.
    pub fn write(&self, data: &[u8]) -> Result<(), StreamFault> {
        todo!("W4-11a")
    }

    /// `Seek(offset, origin)` with the raw `STREAM_SEEK` origin (0 set, 1 current, 2 end): the new position.
    pub fn seek(&self, offset: i64, origin: u32) -> Result<u64, StreamFault> {
        todo!("W4-11a")
    }

    /// `SetSize`.
    pub fn set_size(&self, size: u64) -> Result<(), StreamFault> {
        todo!("W4-11a")
    }

    /// `Commit`: flush; `Ok` when disposed.
    pub fn commit(&self) -> Result<(), StreamFault> {
        todo!("W4-11a")
    }

    /// `Stat`.
    pub fn stat(&self) -> Result<STATSTG, StreamFault> {
        todo!("W4-11a")
    }

    /// `Dispose`: drops the stream from the COM object and returns it (`None` if already disposed).
    pub fn dispose(&self) -> Option<Box<dyn StreamBackend>> {
        todo!("W4-11a")
    }

    /// The fault of the last failed COM call on this stream (cleared by the read), so W4-11c can report the real
    /// `io::Error` when WIC turns a stream failure into its own HRESULT. Rust addition, see open questions.
    pub fn take_last_fault(&self) -> Option<StreamFault> {
        todo!("W4-11a")
    }
}

impl Drop for ComStream {
    fn drop(&mut self) {
        // Stub: the port calls `self.dispose()` here.
    }
}

impl fmt::Debug for ComStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComStream").finish_non_exhaustive()
    }
}

/// Maps the raw `dwOrigin` of `IStream::Seek` like C# `(SeekOrigin)dwOrigin` + `Stream.Seek`: 0 -> `Start` (a
/// negative offset is [`StreamFault::SeekBeforeBegin`]), 1 -> `Current`, 2 -> `End`, anything else ->
/// [`StreamFault::InvalidSeekOrigin`]. Pure; tested without COM.
pub fn seek_from(offset: i64, origin: u32) -> Result<SeekFrom, StreamFault> {
    todo!("W4-11a")
}

/// `HRESULT_FROM_WIN32`: `0` stays `0`, a positive Win32 code `c` becomes `0x8007_0000 | (c & 0xFFFF)`.
pub fn hresult_from_win32(code: u32) -> i32 {
    todo!("W4-11a")
}

// `raw-dylib`: no import library needed.
#[link(name = "windowscodecs", kind = "raw-dylib")]
unsafe extern "system" {
    /// Declared here because the `windows` crate does not export it (it is in `wincodec_proxy.h`, not in the
    /// Windows metadata the crate is generated from).
    fn WICCreateImagingFactory_Proxy(sdk_version: u32, factory: *mut *mut c_void) -> HRESULT;
}
