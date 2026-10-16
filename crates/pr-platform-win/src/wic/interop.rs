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
//! - [`create_factory`] / [`check_factory_result`] (`WICCreateImagingFactory_Proxy`, after
//!   [`ensure_com_initialized`] puts the thread in the MTA);
//! - [`ComStream`]: the COM `IStream` over a Rust stream (C# `ManagedIStream`);
//! - [`exception_kind_for_hresult`]: which .NET exception `Marshal.GetExceptionForHR(hr, -1)` makes of a failing
//!   HRESULT. Every C# WIC call throws through that mapping, and the fallback whitelist (W4-12b, INV-12) decides on
//!   the exception TYPE, so the Rust decoders (W4-11c..e) need the same classification.
//!
//! All `unsafe` of the WIC port lives in `pr_platform_win::wic`; every block carries a `// SAFETY:` comment.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::fmt;
use std::fs::File;
use std::io::{self, Cursor, SeekFrom};
use std::marker::PhantomData;
use std::ops::Deref;
use std::os::windows::io::AsRawHandle;
use std::ptr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{
    COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, ISequentialStream_Impl, IStream, IStream_Impl, LOCKTYPE,
    STATFLAG, STATSTG, STGC, STREAM_SEEK,
};
use windows_core::{ComObject, HRESULT, IUnknown, IUnknown_Vtbl, Interface, implement};

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
    /// `System.TypeLoadException`.
    TypeLoad,
    /// `System.Reflection.AmbiguousMatchException`.
    AmbiguousMatch,
    /// `System.Reflection.TargetParameterCountException`.
    TargetParameterCount,
    /// `System.TypeUnloadedException`.
    TypeUnloaded,
    /// `System.Runtime.AmbiguousImplementationException`.
    AmbiguousImplementation,
    /// `System.MethodAccessException`.
    MethodAccess,
    /// `System.Security.Cryptography.CryptographicException`.
    Cryptographic,
    /// `System.ExecutionEngineException`.
    ExecutionEngine,
    /// `System.FieldAccessException`.
    FieldAccess,
    /// `System.Runtime.Serialization.SerializationException`.
    Serialization,
    /// `System.Security.VerificationException`.
    Verification,
    /// `System.MissingFieldException`.
    MissingField,
    /// `System.MissingMemberException`.
    MissingMember,
    /// `System.MulticastNotSupportedException`.
    MulticastNotSupported,
    /// `System.RankException`.
    Rank,
    /// `System.Threading.SynchronizationLockException`.
    SynchronizationLock,
    /// `System.Threading.ThreadInterruptedException`.
    ThreadInterrupted,
    /// `System.MemberAccessException`.
    MemberAccess,
    /// `System.Threading.ThreadStateException`.
    ThreadState,
    /// `System.Threading.ThreadStartException`.
    ThreadStart,
    /// `System.Runtime.InteropServices.InvalidComObjectException`.
    InvalidComObject,
    /// `System.NotFiniteNumberException` (an `ArithmeticException`).
    NotFiniteNumber,
    /// `System.DuplicateWaitObjectException` (an `ArgumentException`).
    DuplicateWaitObject,
    /// `System.Threading.ThreadAbortException`.
    ThreadAbort,
    /// `System.Runtime.InteropServices.InvalidOleVariantTypeException`.
    InvalidOleVariantType,
    /// `System.Resources.MissingManifestResourceException`.
    MissingManifestResource,
    /// `System.Runtime.InteropServices.SafeArrayTypeMismatchException`.
    SafeArrayTypeMismatch,
    /// `System.TypeInitializationException`.
    TypeInitialization,
    /// `System.Runtime.InteropServices.MarshalDirectiveException`.
    MarshalDirective,
    /// `System.Runtime.InteropServices.SafeArrayRankMismatchException`.
    SafeArrayRankMismatch,
    /// `System.InvalidProgramException`.
    InvalidProgram,
    /// `System.DataMisalignedException`.
    DataMisaligned,
    /// `System.Diagnostics.Contracts.ContractException`.
    Contract,
    /// `System.TypeAccessException`.
    TypeAccess,
    /// `System.InsufficientExecutionStackException`.
    InsufficientExecutionStack,
    /// `System.ApplicationException`.
    Application,
    /// `System.Reflection.InvalidFilterCriteriaException`.
    InvalidFilterCriteria,
    /// `System.Reflection.TargetException`.
    Target,
    /// `System.Reflection.CustomAttributeFormatException`.
    CustomAttributeFormat,
}

impl DotnetExceptionKind {
    /// The .NET `Type.FullName` (what the golden records), e.g. `"System.Runtime.InteropServices.COMException"`.
    pub fn type_name(self) -> &'static str {
        match self {
            Self::Com => "System.Runtime.InteropServices.COMException",
            Self::OutOfMemory => "System.OutOfMemoryException",
            Self::Argument => "System.ArgumentException",
            Self::ArgumentOutOfRange => "System.ArgumentOutOfRangeException",
            Self::InvalidCast => "System.InvalidCastException",
            Self::NullReference => "System.NullReferenceException",
            Self::NotImplemented => "System.NotImplementedException",
            Self::NotSupported => "System.NotSupportedException",
            Self::InvalidOperation => "System.InvalidOperationException",
            Self::UnauthorizedAccess => "System.UnauthorizedAccessException",
            Self::FileNotFound => "System.IO.FileNotFoundException",
            Self::DirectoryNotFound => "System.IO.DirectoryNotFoundException",
            Self::DriveNotFound => "System.IO.DriveNotFoundException",
            Self::PathTooLong => "System.IO.PathTooLongException",
            Self::EndOfStream => "System.IO.EndOfStreamException",
            Self::Io => "System.IO.IOException",
            Self::FileLoad => "System.IO.FileLoadException",
            Self::OperationCanceled => "System.OperationCanceledException",
            Self::Overflow => "System.OverflowException",
            Self::Arithmetic => "System.ArithmeticException",
            Self::DivideByZero => "System.DivideByZeroException",
            Self::Format => "System.FormatException",
            Self::IndexOutOfRange => "System.IndexOutOfRangeException",
            Self::ArrayTypeMismatch => "System.ArrayTypeMismatchException",
            Self::BadImageFormat => "System.BadImageFormatException",
            Self::DllNotFound => "System.DllNotFoundException",
            Self::EntryPointNotFound => "System.EntryPointNotFoundException",
            Self::MissingMethod => "System.MissingMethodException",
            Self::ObjectDisposed => "System.ObjectDisposedException",
            Self::Timeout => "System.TimeoutException",
            Self::PlatformNotSupported => "System.PlatformNotSupportedException",
            Self::StackOverflow => "System.StackOverflowException",
            Self::Security => "System.Security.SecurityException",
            Self::System => "System.SystemException",
            Self::Exception => "System.Exception",
            Self::TypeLoad => "System.TypeLoadException",
            Self::AmbiguousMatch => "System.Reflection.AmbiguousMatchException",
            Self::TargetParameterCount => "System.Reflection.TargetParameterCountException",
            Self::TypeUnloaded => "System.TypeUnloadedException",
            Self::AmbiguousImplementation => "System.Runtime.AmbiguousImplementationException",
            Self::MethodAccess => "System.MethodAccessException",
            Self::Cryptographic => "System.Security.Cryptography.CryptographicException",
            Self::ExecutionEngine => "System.ExecutionEngineException",
            Self::FieldAccess => "System.FieldAccessException",
            Self::Serialization => "System.Runtime.Serialization.SerializationException",
            Self::Verification => "System.Security.VerificationException",
            Self::MissingField => "System.MissingFieldException",
            Self::MissingMember => "System.MissingMemberException",
            Self::MulticastNotSupported => "System.MulticastNotSupportedException",
            Self::Rank => "System.RankException",
            Self::SynchronizationLock => "System.Threading.SynchronizationLockException",
            Self::ThreadInterrupted => "System.Threading.ThreadInterruptedException",
            Self::MemberAccess => "System.MemberAccessException",
            Self::ThreadState => "System.Threading.ThreadStateException",
            Self::ThreadStart => "System.Threading.ThreadStartException",
            Self::InvalidComObject => "System.Runtime.InteropServices.InvalidComObjectException",
            Self::NotFiniteNumber => "System.NotFiniteNumberException",
            Self::DuplicateWaitObject => "System.DuplicateWaitObjectException",
            Self::ThreadAbort => "System.Threading.ThreadAbortException",
            Self::InvalidOleVariantType => "System.Runtime.InteropServices.InvalidOleVariantTypeException",
            Self::MissingManifestResource => "System.Resources.MissingManifestResourceException",
            Self::SafeArrayTypeMismatch => "System.Runtime.InteropServices.SafeArrayTypeMismatchException",
            Self::TypeInitialization => "System.TypeInitializationException",
            Self::MarshalDirective => "System.Runtime.InteropServices.MarshalDirectiveException",
            Self::SafeArrayRankMismatch => "System.Runtime.InteropServices.SafeArrayRankMismatchException",
            Self::InvalidProgram => "System.InvalidProgramException",
            Self::DataMisaligned => "System.DataMisalignedException",
            Self::Contract => "System.Diagnostics.Contracts.ContractException",
            Self::TypeAccess => "System.TypeAccessException",
            Self::InsufficientExecutionStack => "System.InsufficientExecutionStackException",
            Self::Application => "System.ApplicationException",
            Self::InvalidFilterCriteria => "System.Reflection.InvalidFilterCriteriaException",
            Self::Target => "System.Reflection.TargetException",
            Self::CustomAttributeFormat => "System.Reflection.CustomAttributeFormatException",
        }
    }

    /// C# `ex is ArgumentException` (the type or a .NET subclass, e.g. `ArgumentOutOfRangeException`). W4-11c has
    /// `catch (ArgumentException)` arms; the golden's `base_types` pins every `is_*` answer.
    pub fn is_argument(self) -> bool {
        matches!(
            self,
            Self::Argument | Self::ArgumentOutOfRange | Self::DuplicateWaitObject
        )
    }

    /// C# `ex is IOException` (`FileNotFound`, `DirectoryNotFound`, `PathTooLong`, `EndOfStream`, ... per golden).
    pub fn is_io(self) -> bool {
        matches!(
            self,
            Self::Io
                | Self::FileNotFound
                | Self::DirectoryNotFound
                | Self::DriveNotFound
                | Self::PathTooLong
                | Self::EndOfStream
                | Self::FileLoad
        )
    }

    /// C# `ex is NotSupportedException` (also `PlatformNotSupportedException`).
    pub fn is_not_supported(self) -> bool {
        matches!(self, Self::NotSupported | Self::PlatformNotSupported)
    }

    /// C# `ex is InvalidOperationException` (also `ObjectDisposedException`).
    pub fn is_invalid_operation(self) -> bool {
        matches!(self, Self::InvalidOperation | Self::ObjectDisposed)
    }

    /// C# `ex is ArithmeticException` (also `OverflowException`, `DivideByZeroException`,
    /// `NotFiniteNumberException`).
    pub fn is_arithmetic(self) -> bool {
        matches!(
            self,
            Self::Arithmetic | Self::Overflow | Self::DivideByZero | Self::NotFiniteNumber
        )
    }
}

/// C# `Marshal.GetExceptionForHR(hr, (IntPtr)(-1))?.GetType()`: `None` for a success code (`hr >= 0`), else the
/// exception kind. Table-driven (CoreCLR `rexcep.h` order: first matching row wins); the golden
/// `wic-hresult-exception` is the authority, every HRESULT in it must map exactly.
pub fn exception_kind_for_hresult(hr: i32) -> Option<DotnetExceptionKind> {
    use DotnetExceptionKind as K;
    if hr >= 0 {
        return None;
    }
    // Every failing code that .NET maps to a specific type: the golden rows plus a full sweep of 19 (round 2: 37) facilities x
    // 65 536 codes (FACILITY_NULL/RPC/DISPATCH/STORAGE/ITF/WIN32/CONTROL/URT/WINCODEC/D2D/NTSTATUS/SECURITY/SSPI/...)
    // through `Marshal.GetExceptionForHR(hr, -1)` on .NET 10, repair round 1 of W4-11a. Every other failing code is
    // a COMException (CoreCLR's default), e.g. all WINCODEC_ERR_*, STG_E_FILENOTFOUND, E_FAIL, E_UNEXPECTED.
    Some(match hr as u32 {
        0x8000_4001 => K::NotImplemented,
        0x8000_4002 => K::InvalidCast,
        0x8000_4003 => K::NullReference,
        0x8007_0005 | 0x800A_004B | 0x800A_014F => K::UnauthorizedAccess,
        0x8007_000E | 0x800A_0007 | 0x800A_7919 => K::OutOfMemory,
        0x8007_0057 | 0x800A_01C1 | 0x800A_01C2 => K::Argument,
        0x8007_0459 | 0x8013_1502 => K::ArgumentOutOfRange,
        0x8000_0013 | 0x8013_1622 => K::ObjectDisposed,
        0x8007_0002 | 0x8007_0015 | 0x8007_0035 | 0x8007_0043 | 0x8007_007B | 0x8007_007E | 0x8007_0485
        | 0x8007_0574 | 0x800A_0035 => K::FileNotFound,
        // INET_E_* (FACILITY_INTERNET 0x0C): CoreCLR maps these "resource not found / cannot connect" codes to
        // FileNotFoundException too (repair round 2: 2.67M-code oracle sweep, 37 facilities).
        0x800C_0004..=0x800C_0008 | 0x800C_000B | 0x800C_000D => K::FileNotFound,
        0x8003_0003 | 0x8007_0003 | 0x800A_004C => K::DirectoryNotFound,
        0x8007_000B | 0x8007_00B6 | 0x8007_00C0 | 0x8007_00C1 | 0x8007_03E6 | 0x8007_0482 | 0x8007_0570
        | 0x8013_1018 | 0x8013_101B | 0x8013_1058 | 0x8013_1107 | 0x8013_110E | 0x8013_1124 | 0x8013_1192
        | 0x8013_141D => K::BadImageFormat,
        0x8007_0004 | 0x8007_0020 | 0x8007_0021 | 0x8007_006E | 0x8007_03ED | 0x8007_03EE | 0x8007_045A
        | 0x8007_0571 | 0x8013_1016 | 0x8013_1040 | 0x8013_1047 | 0x8013_1621 => K::FileLoad,
        0x8007_0026 | 0x800A_003E => K::EndOfStream,
        0x8007_00CE => K::PathTooLong,
        0x8007_0216 => K::Arithmetic,
        0x8007_03E9 | 0x800A_001C => K::StackOverflow,
        0x800A_0006 | 0x8013_1516 => K::Overflow,
        0x800A_0009 | 0x8013_1508 => K::IndexOutOfRange,
        0x8002_0012 | 0x800A_000B => K::DivideByZero,
        0x800A_0039 | 0x800A_793C | 0x800A_793D | 0x8013_1620 => K::Io,
        0x800A_0046 | 0x800A_01A3 | 0x8013_141A | 0x8013_141E | 0x8013_1420 | 0x8013_150A => K::Security,
        0x800A_01B6 | 0x800A_01BD | 0x800A_01CA | 0x800A_01CB | 0x8013_1515 => K::NotSupported,
        0x800A_01CD | 0x8013_1512 => K::MissingMember,
        0x8000_000F | 0x8013_1522 => K::TypeLoad,
        0x8000_211D => K::AmbiguousMatch,
        0x8002_000E => K::TargetParameterCount,
        0x8013_1013 => K::TypeUnloaded,
        0x8013_106A => K::AmbiguousImplementation,
        0x8013_11E6 | 0x8013_1510 => K::MethodAccess,
        0x8013_1430 => K::Cryptographic,
        0x8013_1500 => K::Exception,
        0x8013_1501 => K::System,
        0x8013_1503 => K::ArrayTypeMismatch,
        0x8013_1506 => K::ExecutionEngine,
        0x8013_1507 => K::FieldAccess,
        0x8013_1509 => K::InvalidOperation,
        0x8013_150C => K::Serialization,
        0x8013_150D => K::Verification,
        0x8013_1511 => K::MissingField,
        0x8013_1513 | 0x8013_153E | 0x8013_1602 | 0x8013_1604 => K::MissingMethod,
        0x8013_1514 => K::MulticastNotSupported,
        0x8013_1517 => K::Rank,
        0x8013_1518 => K::SynchronizationLock,
        0x8013_1519 => K::ThreadInterrupted,
        0x8013_151A => K::MemberAccess,
        0x8013_1520 => K::ThreadState,
        0x8013_1523 => K::EntryPointNotFound,
        0x8013_1524 => K::DllNotFound,
        0x8013_1525 => K::ThreadStart,
        0x8013_1527 => K::InvalidComObject,
        0x8013_1528 => K::NotFiniteNumber,
        0x8013_1529 => K::DuplicateWaitObject,
        0x8013_1530 => K::ThreadAbort,
        0x8013_1531 => K::InvalidOleVariantType,
        0x8013_1532 => K::MissingManifestResource,
        0x8013_1533 => K::SafeArrayTypeMismatch,
        0x8013_1534 => K::TypeInitialization,
        0x8013_1535 => K::MarshalDirective,
        0x8013_1537 => K::Format,
        0x8013_1538 => K::SafeArrayRankMismatch,
        0x8013_1539 => K::PlatformNotSupported,
        0x8013_153A => K::InvalidProgram,
        0x8013_153B => K::OperationCanceled,
        0x8013_1541 => K::DataMisaligned,
        0x8013_1542 => K::Contract,
        0x8013_1543 => K::TypeAccess,
        0x8013_1578 => K::InsufficientExecutionStack,
        0x8013_1600 => K::Application,
        0x8013_1601 => K::InvalidFilterCriteria,
        0x8013_1603 => K::Target,
        0x8013_1605 => K::CustomAttributeFormat,
        _ => K::Com,
    })
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
        match self {
            Self::Com { hresult } => *hresult,
            Self::NoInterface { .. } => E_NOINTERFACE,
        }
    }

    /// The .NET exception type C# would throw: `exception_kind_for_hresult(hresult)` (a failing code always maps; a
    /// non-failing code, impossible by construction, gives [`DotnetExceptionKind::Com`], never a panic),
    /// [`DotnetExceptionKind::InvalidCast`] for [`WicError::NoInterface`].
    pub fn exception_kind(&self) -> DotnetExceptionKind {
        match self {
            Self::Com { hresult } => exception_kind_for_hresult(*hresult).unwrap_or(DotnetExceptionKind::Com),
            Self::NoInterface { .. } => DotnetExceptionKind::InvalidCast,
        }
    }

    /// From a `windows` crate error: its `code()`, except code 0 (the crate's `Error::empty()`, returned when a
    /// method succeeded with a null out object) which becomes `E_FAIL` like C# `WicCom.Wrap`. Never reads the
    /// error's message (it may carry a stale thread `IErrorInfo`; C# passes `-1` to ignore it).
    pub fn from_windows(error: &windows_core::Error) -> Self {
        let code = error.code().0;
        Self::Com {
            hresult: if code == 0 { E_FAIL } else { code },
        }
    }
}

impl fmt::Display for WicError {
    /// `Com`: `"WIC call failed (0x{hresult:08X})"`; `NoInterface`: the C# `InvalidCastException` message.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Com { hresult } => write!(f, "WIC call failed (0x{:08X})", *hresult as u32),
            Self::NoInterface { interface } => {
                write!(f, "The COM object does not implement {interface} (E_NOINTERFACE).")
            }
        }
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
    OUTSTANDING.with(Cell::get)
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
    /// Takes ownership of an interface reference and counts it on this thread.
    fn counted(inner: T) -> Self {
        OUTSTANDING.with(|c| c.set(c.get().wrapping_add(1)));
        Self {
            inner,
            _per_thread: PhantomData,
        }
    }

    /// `WicCom.Wrap<T>(nint)`: takes over ONE reference of `raw`. Null -> `Err(Com { E_FAIL })`. Otherwise
    /// `QueryInterface(T::IID)`; on failure `Err(NoInterface { interface })` and nothing is left referenced. The
    /// reference passed in is always released before returning (the wrapper holds the one the QI added).
    ///
    /// # Safety
    /// `raw` is null or a valid COM interface pointer that owns one reference the caller hands over.
    pub unsafe fn wrap(raw: *mut c_void, interface: &'static str) -> WicResult<Self> {
        // SAFETY: the caller's contract for `raw` is the one `wrap_or_none` requires; it is passed on unchanged.
        match unsafe { Self::wrap_or_none(raw, interface) } {
            Ok(Some(com)) => Ok(com),
            Ok(None) => Err(WicError::Com { hresult: E_FAIL }),
            Err(error) => Err(error),
        }
    }

    /// `WicCom.WrapOrNull<T>`: like [`Com::wrap`] but null -> `Ok(None)` (optional out object, e.g. a thumbnail).
    ///
    /// # Safety
    /// As [`Com::wrap`].
    pub unsafe fn wrap_or_none(raw: *mut c_void, interface: &'static str) -> WicResult<Option<Self>> {
        if raw.is_null() {
            return Ok(None);
        }
        // SAFETY: `raw` is a non-null COM interface pointer owning one reference the caller hands over (the
        // function's contract); `IUnknown` takes over exactly that reference and releases it when dropped below.
        let unknown = unsafe { IUnknown::from_raw(raw) };
        // QueryInterface; the result is checked with a pattern, never turned into a thrown error (C# `is`).
        let cast = unknown.cast::<T>();
        drop(unknown);
        match cast {
            Ok(inner) => Ok(Some(Self::counted(inner))),
            Err(_) => Err(WicError::NoInterface { interface }),
        }
    }

    /// Counts an interface the `windows` crate returned from an out-object method: `Ok(t)` -> counted `Com`;
    /// `Err` with code 0 (S_OK + null) -> `E_FAIL`; any other `Err` -> `Com { code }`.
    pub fn adopt(result: windows_core::Result<T>) -> WicResult<Self> {
        match result {
            Ok(inner) => Ok(Self::counted(inner)),
            Err(error) => Err(WicError::from_windows(&error)),
        }
    }

    /// Like [`Com::adopt`] for an OPTIONAL out object: `Err` with code 0 (S_OK + null) -> `Ok(None)`.
    pub fn adopt_optional(result: windows_core::Result<T>) -> WicResult<Option<Self>> {
        match result {
            Ok(inner) => Ok(Some(Self::counted(inner))),
            Err(error) if error.code().0 == 0 => Ok(None),
            Err(error) => Err(WicError::from_windows(&error)),
        }
    }

    /// C# `obj is U` on a generated wrapper: a `QueryInterface`. Any failure (E_NOINTERFACE or other) is `None`,
    /// never an error. A success is a new counted [`Com`].
    pub fn query<U: Interface>(&self) -> Option<Com<U>> {
        self.inner.cast::<U>().ok().map(Com::counted)
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
        OUTSTANDING.with(|c| c.set(c.get().wrapping_sub(1)));
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
    // SAFETY: `raw` is a live COM interface pointer (contract): its first word points to a vtable that starts with
    // the three IUnknown slots, valid while the object lives.
    unsafe {
        let vtable = *(raw as *const *const IUnknown_Vtbl);
        ((*vtable).AddRef)(raw)
    }
}

/// Test seam (C# tests call `Marshal.Release`): releases one reference, returns the remaining count.
///
/// # Safety
/// `raw` is a valid, non-null COM interface pointer on which the caller owns one reference.
#[doc(hidden)]
pub unsafe fn release_raw(raw: *mut c_void) -> u32 {
    // SAFETY: `raw` is a live COM interface pointer on which the caller owns one reference (contract); the vtable
    // read happens before the Release that may destroy the object.
    unsafe {
        let vtable = *(raw as *const *const IUnknown_Vtbl);
        ((*vtable).Release)(raw)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------------------------------------------

/// The per-thread `CoInitializeEx` record: `CoUninitialize` at thread exit when this module initialised COM.
struct ApartmentGuard {
    uninitialize: bool,
}

impl Drop for ApartmentGuard {
    fn drop(&mut self) {
        if self.uninitialize {
            // SAFETY: balances the successful (S_OK/S_FALSE) CoInitializeEx this guard recorded on this thread;
            // thread-local destructors run on the owning thread.
            unsafe { CoUninitialize() };
        }
    }
}

thread_local! {
    /// `Some` once `ensure_com_initialized` succeeded on this thread.
    static APARTMENT: RefCell<Option<ApartmentGuard>> = const { RefCell::new(None) };
}

/// Puts the calling thread in the COM multithreaded apartment once (thread-local guard), as the CLR has already
/// done for every thread a C# WIC call runs on: `CoInitializeEx(None, COINIT_MULTITHREADED)`; `S_OK`/`S_FALSE` ->
/// `CoUninitialize` in the guard's `Drop` at thread exit; `RPC_E_CHANGED_MODE` (already STA, e.g. the UI thread) ->
/// `Ok` with nothing to undo; any other failure -> `Err(Com { hr })`, retried on the next call. Rust addition.
pub fn ensure_com_initialized() -> WicResult<()> {
    APARTMENT
        .try_with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some() {
                return Ok(());
            }
            // SAFETY: no pointer is passed (reserved = NULL); the call only changes this thread's apartment state.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.0 >= 0 {
                *slot = Some(ApartmentGuard { uninitialize: true });
                Ok(())
            } else if hr == RPC_E_CHANGED_MODE {
                *slot = Some(ApartmentGuard { uninitialize: false });
                Ok(())
            } else {
                Err(WicError::Com { hresult: hr.0 })
            }
        })
        // The thread is shutting down (its thread-locals are being destroyed): report CO_E_NOTINITIALIZED-like failure.
        .unwrap_or(Err(WicError::Com { hresult: E_FAIL }))
}

/// `WicDirectDecoder.CreateFactory` / `WicImageEncoder.CreateFactory`: [`ensure_com_initialized`], then
/// `WICCreateImagingFactory_Proxy(WINCODEC_SDK_VERSION1)` from `windowscodecs.dll` (not `CoCreateInstance`).
/// A failing HRESULT or a null factory goes through [`check_factory_result`]; a pointer returned together with a
/// failing HRESULT is released (no leak); success is wrapped with [`Com::wrap`].
pub fn create_factory() -> WicResult<Com<IWICImagingFactory>> {
    ensure_com_initialized()?;
    let mut raw: *mut c_void = ptr::null_mut();
    // SAFETY: `raw` is a local out slot that lives across the call; the proxy writes an owned factory pointer or
    // leaves it null.
    let hr = unsafe { WICCreateImagingFactory_Proxy(WINCODEC_SDK_VERSION1, &mut raw) };
    // SAFETY: `raw` is what the proxy returned: null or a pointer owning one reference.
    unsafe { factory_from_proxy(hr, raw) }
}

/// The tail of [`create_factory`]: check the proxy's result, release a pointer that came back with a failure, wrap
/// a success. Separate so the release path is unit-tested without a failing proxy.
///
/// # Safety
/// `raw` is null or a valid COM interface pointer owning one reference the caller hands over.
unsafe fn factory_from_proxy(hr: HRESULT, raw: *mut c_void) -> WicResult<Com<IWICImagingFactory>> {
    if let Err(error) = check_factory_result(hr.0, !raw.is_null()) {
        if !raw.is_null() {
            // SAFETY: a failing HRESULT still returned a pointer: it owns one reference we must give back.
            unsafe { release_raw(raw) };
        }
        return Err(error);
    }
    // SAFETY: `raw` is non-null here (checked above) and owns one reference (this function's contract).
    unsafe { Com::wrap(raw, "IWICImagingFactory") }
}

/// `WicDirectDecoder.ThrowIfFactoryFailed(hr, factoryCreated)`: `hr < 0` -> `Err(Com { hr })` (even when a factory
/// came back); `hr >= 0` without a factory -> `Err(Com { E_FAIL })`; else `Ok(())`.
pub fn check_factory_result(hr: i32, factory_created: bool) -> WicResult<()> {
    if hr < 0 {
        Err(WicError::Com { hresult: hr })
    } else if !factory_created {
        Err(WicError::Com { hresult: E_FAIL })
    } else {
        Ok(())
    }
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
    // SAFETY: `factory` and `stream` are live interfaces borrowed for the call; the vendor GUID pointer is NULL,
    // which WIC documents as "no preferred vendor".
    Com::adopt(unsafe { factory.CreateDecoderFromStream(stream, ptr::null(), options) })
}

/// `IWICImagingFactory::CreateFormatConverter`.
pub fn create_format_converter(factory: &IWICImagingFactory) -> WicResult<Com<IWICFormatConverter>> {
    // SAFETY: `factory` is a live interface borrowed for the call; no other pointer is passed.
    Com::adopt(unsafe { factory.CreateFormatConverter() })
}

/// `IWICImagingFactory::CreateBitmapScaler`.
pub fn create_bitmap_scaler(factory: &IWICImagingFactory) -> WicResult<Com<IWICBitmapScaler>> {
    // SAFETY: `factory` is a live interface borrowed for the call; no other pointer is passed.
    Com::adopt(unsafe { factory.CreateBitmapScaler() })
}

/// `IWICImagingFactory::CreateBitmapFlipRotator`.
pub fn create_bitmap_flip_rotator(factory: &IWICImagingFactory) -> WicResult<Com<IWICBitmapFlipRotator>> {
    // SAFETY: `factory` is a live interface borrowed for the call; no other pointer is passed.
    Com::adopt(unsafe { factory.CreateBitmapFlipRotator() })
}

/// `IWICImagingFactory::CreateStream`.
pub fn create_stream(factory: &IWICImagingFactory) -> WicResult<Com<IWICStream>> {
    // SAFETY: `factory` is a live interface borrowed for the call; no other pointer is passed.
    Com::adopt(unsafe { factory.CreateStream() })
}

/// `IWICImagingFactory::CreateColorContext`.
pub fn create_color_context(factory: &IWICImagingFactory) -> WicResult<Com<IWICColorContext>> {
    // SAFETY: `factory` is a live interface borrowed for the call; no other pointer is passed.
    Com::adopt(unsafe { factory.CreateColorContext() })
}

/// `IWICImagingFactory::CreateColorTransformer`.
pub fn create_color_transformer(factory: &IWICImagingFactory) -> WicResult<Com<IWICColorTransform>> {
    // SAFETY: `factory` is a live interface borrowed for the call; no other pointer is passed.
    Com::adopt(unsafe { factory.CreateColorTransformer() })
}

/// `IWICBitmapDecoder::GetFrame(index)`.
pub fn get_frame(decoder: &IWICBitmapDecoder, index: u32) -> WicResult<Com<IWICBitmapFrameDecode>> {
    // SAFETY: `decoder` is a live interface borrowed for the call; the index is checked by WIC.
    Com::adopt(unsafe { decoder.GetFrame(index) })
}

/// `IWICBitmapFrameDecode::GetMetadataQueryReader`.
pub fn get_metadata_query_reader(frame: &IWICBitmapFrameDecode) -> WicResult<Com<IWICMetadataQueryReader>> {
    // SAFETY: `frame` is a live interface borrowed for the call; no other pointer is passed.
    Com::adopt(unsafe { frame.GetMetadataQueryReader() })
}

/// `IWICBitmapFrameDecode::GetThumbnail`: the embedded thumbnail; `Ok(None)` when the codec returns S_OK without
/// an object. A failing HRESULT (e.g. `WINCODEC_ERR_CODECNOTHUMBNAIL`) is `Err` as in C#.
pub fn get_thumbnail(frame: &IWICBitmapFrameDecode) -> WicResult<Option<Com<IWICBitmapSource>>> {
    // SAFETY: `frame` is a live interface borrowed for the call; no other pointer is passed.
    Com::adopt_optional(unsafe { frame.GetThumbnail() })
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
    /// What `Stream.Seek` throws for a target before 0 (or past `i64::MAX`), the position unchanged. Default:
    /// [`StreamFault::SeekBeforeBegin`] (`MemoryStream`, `COR_E_IO`). `File` overrides it with
    /// `HRESULT_FROM_WIN32(ERROR_INVALID_PARAMETER)` (`0x80070057`, the `IOException.HResult` of .NET `FileStream`).
    /// Rust addition (repair round 1): the C# stream decides, so the backend does.
    fn seek_before_begin_fault(&self) -> StreamFault {
        StreamFault::SeekBeforeBegin
    }
    /// For the owner to get the concrete backend back after [`ComStream::dispose`] (e.g. the encoded bytes).
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send>;
}

/// A file opened by the caller (C#: `FileStream` with share `ReadWrite | Delete`, W4-11c opens it). `set_len` also
/// moves the position to the new length when it was beyond it (.NET `FileStream.SetLength`); `flush` is
/// `Write::flush`, never `sync_all` (C# `Flush()`, not `Flush(true)`).
///
/// .NET `FileStream` checks `CanRead`/`CanWrite` before any I/O and throws `NotSupportedException` (also for 0
/// bytes); a Rust `File` would report `ERROR_ACCESS_DENIED`. So a read/write/set-length on a handle without that
/// access (`NtQueryInformationFile(FileAccessInformation)`, asked only for 0 bytes or after an access-denied error,
/// never on the hot path) fails with `ErrorKind::Unsupported`. A seek before 0 is `0x80070057`
/// ([`StreamBackend::seek_before_begin_fault`]).
impl StreamBackend for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return if file_lacks_access(self, FILE_READ_DATA) {
                Err(not_supported("Stream does not support reading."))
            } else {
                Ok(0)
            };
        }
        io::Read::read(self, buf).map_err(|error| refine_access_denied(self, error, FILE_READ_DATA))
    }
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        if data.is_empty() {
            return if file_lacks_access(self, FILE_WRITE_DATA | FILE_APPEND_DATA) {
                Err(not_supported("Stream does not support writing."))
            } else {
                Ok(())
            };
        }
        io::Write::write_all(self, data)
            .map_err(|error| refine_access_denied(self, error, FILE_WRITE_DATA | FILE_APPEND_DATA))
    }
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        io::Seek::seek(self, pos)
    }
    fn set_len(&mut self, len: u64) -> io::Result<()> {
        File::set_len(self, len).map_err(|error| {
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER) {
                // .NET `RandomAccess.SetFileLength`: ERROR_INVALID_PARAMETER -> ArgumentOutOfRangeException
                // ("file length too big", e.g. 2^44 on NTFS).
                argument_out_of_range("The file length is too big.")
            } else {
                refine_access_denied(self, error, FILE_WRITE_DATA | FILE_APPEND_DATA)
            }
        })?;
        let position = io::Seek::stream_position(self)?;
        if position > len {
            io::Seek::seek(self, SeekFrom::Start(len))?;
        }
        Ok(())
    }
    fn len(&mut self) -> io::Result<u64> {
        Ok(self.metadata()?.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(self)
    }
    fn seek_before_begin_fault(&self) -> StreamFault {
        StreamFault::Io(io::Error::from_raw_os_error(ERROR_INVALID_PARAMETER))
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

/// `FILE_READ_DATA` (winnt.h): the handle may read.
const FILE_READ_DATA: u32 = 0x0001;
/// `FILE_WRITE_DATA` (winnt.h): the handle may write.
const FILE_WRITE_DATA: u32 = 0x0002;
/// `FILE_APPEND_DATA` (winnt.h): the handle may append (.NET `FileAccess.Write` with `FileMode.Append`).
const FILE_APPEND_DATA: u32 = 0x0004;
/// `ERROR_ACCESS_DENIED`.
const ERROR_ACCESS_DENIED: i32 = 5;
/// `ERROR_INVALID_PARAMETER`: .NET `FileStream` seek before 0 (`IOException`, HResult `0x80070057`).
const ERROR_INVALID_PARAMETER: i32 = 87;
/// `ERROR_NEGATIVE_SEEK`: `SetFilePointerEx` to a negative position.
const ERROR_NEGATIVE_SEEK: i32 = 131;
/// `ERROR_OPERATION_ABORTED`: .NET `Win32Marshal` turns it into `OperationCanceledException`.
const ERROR_OPERATION_ABORTED: i32 = 995;
/// `COR_E_OPERATIONCANCELED`: HRESULT of a C# `OperationCanceledException`.
const COR_E_OPERATIONCANCELED: i32 = 0x8013_153B_u32 as i32;
/// `FileAccessInformation` (`FILE_INFORMATION_CLASS`).
const FILE_ACCESS_INFORMATION_CLASS: i32 = 8;

/// `IO_STATUS_BLOCK` (the status/pointer union is pointer-sized).
#[repr(C)]
struct IoStatusBlock {
    status: usize,
    information: usize,
}

/// The access mask the handle was opened with (`FILE_ACCESS_INFORMATION.AccessFlags`), `None` if the query fails.
fn file_access_mask(file: &File) -> Option<u32> {
    let mut status = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut access: u32 = 0;
    // SAFETY: `file` owns a live handle for the call; `status` and `access` are local out slots of the sizes
    // NtQueryInformationFile writes for FileAccessInformation (an IO_STATUS_BLOCK and one ULONG, length 4).
    let nt = unsafe {
        NtQueryInformationFile(
            file.as_raw_handle(),
            &mut status,
            (&mut access as *mut u32).cast(),
            4,
            FILE_ACCESS_INFORMATION_CLASS,
        )
    };
    (nt >= 0).then_some(access)
}

/// The handle provably lacks every right of `rights` (.NET `!CanRead` / `!CanWrite`); `false` when unknown.
fn file_lacks_access(file: &File, rights: u32) -> bool {
    file_access_mask(file).is_some_and(|mask| mask & rights == 0)
}

/// `ERROR_ACCESS_DENIED` on a handle without `rights` -> `ErrorKind::Unsupported` (.NET `NotSupportedException`).
fn refine_access_denied(file: &File, error: io::Error, rights: u32) -> io::Error {
    if error.raw_os_error() == Some(ERROR_ACCESS_DENIED) && file_lacks_access(file, rights) {
        let what = if rights == FILE_READ_DATA { "reading" } else { "writing" };
        not_supported(&format!("Stream does not support {what}."))
    } else {
        error
    }
}

fn not_supported(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message.to_owned())
}

/// Payload of a backend `io::Error` that stands for a .NET `ArgumentOutOfRangeException` (memory seek/set-length
/// above `int.MaxValue`, `FileStream.SetLength` too big). [`StreamState`] turns it into
/// [`StreamFault::SeekOutOfRange`] / [`StreamFault::SizeOutOfRange`] (`COR_E_ARGUMENTOUTOFRANGE`).
#[derive(Debug)]
struct ArgumentOutOfRange(&'static str);

impl fmt::Display for ArgumentOutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ArgumentOutOfRange {}

fn argument_out_of_range(message: &'static str) -> io::Error {
    io::Error::other(ArgumentOutOfRange(message))
}

fn is_argument_out_of_range(error: &io::Error) -> bool {
    error.get_ref().is_some_and(|inner| inner.is::<ArgumentOutOfRange>())
}

/// `ERROR_OUTOFMEMORY`: `HRESULT_FROM_WIN32(14)` is `E_OUTOFMEMORY` (`0x8007000E`), the HResult of the
/// `OutOfMemoryException` a `MemoryStream` throws when its buffer cannot grow.
const ERROR_OUTOFMEMORY: i32 = 14;

/// .NET `MemoryStream.MemStreamMaxLength` (`int.MaxValue`): no position or length above it.
const MEM_STREAM_MAX_LENGTH: i64 = i32::MAX as i64;

/// .NET `MemoryStream.Seek` (`SeekCore`, `_origin` 0) from `position` in a stream of `length` bytes: `offset >
/// int.MaxValue - base` -> `ArgumentOutOfRangeException` (position kept); a target before 0 -> `InvalidInput`
/// (`IOException` "before the beginning", via [`StreamBackend::seek_before_begin_fault`]); a target past the end
/// is allowed.
fn memory_seek_target(position: u64, length: u64, pos: SeekFrom) -> io::Result<u64> {
    let (base, offset) = match pos {
        SeekFrom::Start(offset) => (0, i64::try_from(offset).unwrap_or(i64::MAX)),
        SeekFrom::Current(offset) => (i64::try_from(position).unwrap_or(i64::MAX), offset),
        SeekFrom::End(offset) => (i64::try_from(length).unwrap_or(i64::MAX), offset),
    };
    if offset > MEM_STREAM_MAX_LENGTH - base {
        return Err(argument_out_of_range(
            "Stream length must be non-negative and less than 2^31 - 1 - origin.",
        ));
    }
    match base.checked_add(offset).map(u64::try_from) {
        Some(Ok(target)) => Ok(target),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "An attempt was made to move the position before the beginning of the stream.",
        )),
    }
}

/// .NET `MemoryStream.SetLength` range check, which runs BEFORE the writable check: above `int.MaxValue` ->
/// `ArgumentOutOfRangeException`, nothing allocated.
fn check_memory_length(len: u64) -> io::Result<usize> {
    if len > MEM_STREAM_MAX_LENGTH as u64 {
        return Err(argument_out_of_range(
            "Stream length must be non-negative and less than 2^31 - 1 - origin.",
        ));
    }
    usize::try_from(len).map_err(|_| argument_out_of_range("Stream length is out of range."))
}

/// Grows `bytes` to `new_len` zero bytes without aborting on allocation failure (`ERROR_OUTOFMEMORY`).
fn grow_zeroed(bytes: &mut Vec<u8>, new_len: usize) -> io::Result<()> {
    if new_len > bytes.len() {
        bytes
            .try_reserve(new_len - bytes.len())
            .map_err(|_| io::Error::from_raw_os_error(ERROR_OUTOFMEMORY))?;
        bytes.resize(new_len, 0);
    }
    Ok(())
}

/// A growable memory stream (C# `new MemoryStream()`, the encoders' output, W4-11e). Seek, write and set-length
/// follow .NET `MemoryStream`: no position or length above `int.MaxValue`.
impl StreamBackend for Cursor<Vec<u8>> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        io::Read::read(self, buf)
    }
    /// .NET `MemoryStream.Write`: `position + count` above `int.MaxValue` -> `IOException` "Stream was too long"
    /// (`COR_E_IO`), nothing changed; past the end the gap is zero-filled and the length becomes `position + count`
    /// (also for 0 bytes); a buffer that cannot grow -> `E_OUTOFMEMORY`, never an abort.
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        let start = self.position();
        let end = match start.checked_add(data.len() as u64) {
            Some(end) if end <= MEM_STREAM_MAX_LENGTH as u64 => end,
            _ => return Err(io::Error::other("Stream was too long.")),
        };
        let (Ok(start_index), Ok(end_index)) = (usize::try_from(start), usize::try_from(end)) else {
            return Err(io::Error::other("Stream was too long."));
        };
        let bytes = self.get_mut();
        grow_zeroed(bytes, end_index)?;
        if let Some(target) = bytes.get_mut(start_index..end_index) {
            target.copy_from_slice(data);
        }
        self.set_position(end);
        Ok(())
    }
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = memory_seek_target(self.position(), self.get_ref().len() as u64, pos)?;
        self.set_position(target);
        Ok(target)
    }
    fn set_len(&mut self, len: u64) -> io::Result<()> {
        let new_len = check_memory_length(len)?;
        let bytes = self.get_mut();
        grow_zeroed(bytes, new_len)?;
        bytes.truncate(new_len);
        if self.position() > len {
            self.set_position(len);
        }
        Ok(())
    }
    fn len(&mut self) -> io::Result<u64> {
        Ok(self.get_ref().len() as u64)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

/// Read-only bytes (C# `new MemoryStream(bytes, writable: false)` / `ReadOnlyMemoryStreamFactory`, W4-16b):
/// write and set-length fail with `ErrorKind::Unsupported` (C# `NotSupportedException`); seek follows .NET
/// `MemoryStream` (no target above `int.MaxValue`).
impl StreamBackend for Cursor<Arc<[u8]>> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        io::Read::read(self, buf)
    }
    fn write_all(&mut self, _data: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Stream does not support writing.",
        ))
    }
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = memory_seek_target(self.position(), self.get_ref().len() as u64, pos)?;
        self.set_position(target);
        Ok(target)
    }
    /// .NET `MemoryStream.SetLength`: the range check (above `int.MaxValue` -> `ArgumentOutOfRangeException`)
    /// comes before the writable check (`NotSupportedException`).
    fn set_len(&mut self, len: u64) -> io::Result<()> {
        check_memory_length(len)?;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Stream does not support writing.",
        ))
    }
    fn len(&mut self) -> io::Result<u64> {
        Ok(self.get_ref().len() as u64)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
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
    /// Seek target before 0 on a memory backend (C# `MemoryStream`: `IOException` "before the beginning",
    /// `COR_E_IO`); the position is unchanged. A `File` reports [`StreamFault::Io`] with `ERROR_INVALID_PARAMETER`
    /// instead (.NET `FileStream`), see [`StreamBackend::seek_before_begin_fault`].
    SeekBeforeBegin,
    /// `dwOrigin` not 0/1/2 (C# `ArgumentException` "Invalid seek origin", `E_INVALIDARG`).
    InvalidSeekOrigin(u32),
    /// `SetSize` above `i64::MAX` (C# `long` parameter is negative: `ArgumentOutOfRangeException`), or a length the
    /// backend's range check refuses (`MemoryStream.SetLength` above `int.MaxValue`, `FileStream.SetLength` too big
    /// for the file system).
    SizeOutOfRange(u64),
    /// `Seek` offset whose target is above `int.MaxValue` on a memory backend (.NET `MemoryStream.Seek`:
    /// `ArgumentOutOfRangeException`, `COR_E_ARGUMENTOUTOFRANGE`); the position is unchanged. Rust addition (repair
    /// round 2 of W4-11a), carries the `dlibMove` offset.
    SeekOutOfRange(i64),
    /// `Read`/`Write` with `cb > i32::MAX` (C# `int cb` is negative: `new Span<byte>(pv, cb)` throws
    /// `ArgumentOutOfRangeException`, `COR_E_ARGUMENTOUTOFRANGE`); `pv` is not touched.
    CountOutOfRange(u32),
    /// Any other backend error: `HRESULT_FROM_WIN32(raw_os_error)` when it has an OS code (C# `IOException.HResult`
    /// of a `FileStream` error), else `E_INVALIDARG` for `ErrorKind::InvalidInput` (a .NET `ArgumentException` crossing
    /// the CCW; W4-11c relies on it), else `COR_E_IO`. OS error 995 (`ERROR_OPERATION_ABORTED`) is
    /// `COR_E_OPERATIONCANCELED` (`0x8013153B`), as .NET `Win32Marshal` makes an `OperationCanceledException` of it.
    /// (`ErrorKind::Unsupported` never gets here: it is [`StreamFault::NotSupported`]; in `Seek`, `InvalidInput` /
    /// OS error 131 is the backend's [`StreamBackend::seek_before_begin_fault`].)
    Io(io::Error),
}

impl StreamFault {
    /// The HRESULT the COM caller receives.
    pub fn hresult(&self) -> i32 {
        match self {
            Self::Disposed => COR_E_OBJECTDISPOSED,
            Self::NotSupported => COR_E_NOTSUPPORTED,
            Self::SeekBeforeBegin => COR_E_IO,
            Self::InvalidSeekOrigin(_) => E_INVALIDARG,
            Self::SizeOutOfRange(_) | Self::SeekOutOfRange(_) | Self::CountOutOfRange(_) => COR_E_ARGUMENTOUTOFRANGE,
            Self::Io(error) => {
                if let Some(code) = error.raw_os_error() {
                    if code == ERROR_OPERATION_ABORTED {
                        // .NET `Win32Marshal.GetExceptionForWin32Error(995)`: `OperationCanceledException`.
                        return COR_E_OPERATIONCANCELED;
                    }
                    let hr = hresult_from_win32(code as u32);
                    if hr < 0 {
                        return hr;
                    }
                }
                if error.kind() == io::ErrorKind::InvalidInput {
                    E_INVALIDARG
                } else {
                    COR_E_IO
                }
            }
        }
    }

    /// Backend error of any op: `Unsupported` -> [`StreamFault::NotSupported`], else [`StreamFault::Io`].
    fn from_io(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::Unsupported {
            Self::NotSupported
        } else {
            Self::Io(error)
        }
    }
}

/// A backend seek error that means "target before 0 / overflow": `InvalidInput` (`Cursor`, and `File`'s
/// `ERROR_INVALID_PARAMETER`) or `ERROR_NEGATIVE_SEEK` (131, `SetFilePointerEx`).
fn is_seek_before_begin(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::InvalidInput || error.raw_os_error() == Some(ERROR_NEGATIVE_SEEK)
}

impl fmt::Display for StreamFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disposed => f.write_str("Cannot access a closed Stream."),
            Self::NotSupported => f.write_str("Specified method is not supported."),
            Self::SeekBeforeBegin => {
                f.write_str("An attempt was made to move the position before the beginning of the stream.")
            }
            Self::InvalidSeekOrigin(origin) => write!(f, "Invalid seek origin ({origin})."),
            Self::SizeOutOfRange(size) => write!(f, "Stream length {size} is out of range."),
            Self::SeekOutOfRange(offset) => write!(f, "Seek offset {offset} is out of range."),
            Self::CountOutOfRange(count) => write!(f, "Byte count {count} is out of range."),
            Self::Io(error) => write!(f, "Stream I/O failed: {error}"),
        }
    }
}

impl std::error::Error for StreamFault {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// Shared state of the COM object: the backend (`None` once disposed) and the last fault a COM call returned.
struct StreamState {
    backend: Option<Box<dyn StreamBackend>>,
    last_fault: Option<StreamFault>,
}

impl StreamState {
    fn backend(&mut self) -> Result<&mut Box<dyn StreamBackend>, StreamFault> {
        self.backend.as_mut().ok_or(StreamFault::Disposed)
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, StreamFault> {
        self.backend()?.read(buf).map_err(StreamFault::from_io)
    }

    fn write(&mut self, data: &[u8]) -> Result<(), StreamFault> {
        self.backend()?.write_all(data).map_err(StreamFault::from_io)
    }

    fn seek(&mut self, offset: i64, origin: u32) -> Result<u64, StreamFault> {
        let backend = self.backend()?;
        let target = match seek_from(offset, origin) {
            Ok(target) => target,
            Err(StreamFault::SeekBeforeBegin) => return Err(backend.seek_before_begin_fault()),
            Err(fault) => return Err(fault),
        };
        match backend.seek(target) {
            Ok(position) => Ok(position),
            Err(error) if is_seek_before_begin(&error) => Err(backend.seek_before_begin_fault()),
            Err(error) if is_argument_out_of_range(&error) => Err(StreamFault::SeekOutOfRange(offset)),
            Err(error) => Err(StreamFault::from_io(error)),
        }
    }

    fn set_size(&mut self, size: u64) -> Result<(), StreamFault> {
        let backend = self.backend()?;
        if size > i64::MAX as u64 {
            return Err(StreamFault::SizeOutOfRange(size));
        }
        backend.set_len(size).map_err(|error| {
            if is_argument_out_of_range(&error) {
                StreamFault::SizeOutOfRange(size)
            } else {
                StreamFault::from_io(error)
            }
        })
    }

    fn commit(&mut self) -> Result<(), StreamFault> {
        match self.backend.as_mut() {
            Some(backend) => backend.flush().map_err(StreamFault::from_io),
            None => Ok(()),
        }
    }

    fn length(&mut self) -> Result<u64, StreamFault> {
        self.backend()?.len().map_err(StreamFault::from_io)
    }

    /// Records a failed COM call's fault and returns its HRESULT.
    fn fail(&mut self, fault: StreamFault) -> i32 {
        let hr = fault.hresult();
        self.last_fault = Some(fault);
        hr
    }
}

/// `E_POINTER`: a null buffer/out pointer the call needs (Rust-only guard, never reached through WIC).
const E_POINTER: i32 = 0x8000_4003_u32 as i32;

/// The `Err` of an `IStream_Impl` shim: carries only the code (never the thread's `IErrorInfo`).
fn com_error(hr: i32) -> windows_core::Error {
    windows_core::Error::from_hresult(HRESULT(hr))
}

/// The COM object (`#[implement(IStream)]`). Its `IStream`/`ISequentialStream` methods call the safe
/// [`ComStream`] logic and turn a [`StreamFault`] into its HRESULT; they never panic (a panic across
/// `extern "system"` aborts the process): lock with `unwrap_or_else(PoisonError::into_inner)`, no `unwrap`.
#[implement(IStream)]
struct ComStreamObject {
    state: Mutex<StreamState>,
}

impl ComStreamObject {
    fn lock(&self) -> MutexGuard<'_, StreamState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `cb` above `i32::MAX` is a negative C# `int`: `ArgumentOutOfRangeException`.
fn check_count(cb: u32) -> Result<usize, StreamFault> {
    if cb > i32::MAX as u32 {
        Err(StreamFault::CountOutOfRange(cb))
    } else {
        Ok(cb as usize)
    }
}

impl ISequentialStream_Impl for ComStreamObject_Impl {
    /// C# `Read(byte* pv, int cb, nint pcbRead)`: disposed, then `cb > i32::MAX` (`COR_E_ARGUMENTOUTOFRANGE`); one
    /// backend read into `pv[..cb]`; `*pcbRead = n` when not null; S_OK also for a short read or 0 at end (C# never
    /// returns S_FALSE).
    fn Read(&self, pv: *mut c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        let mut state = self.lock();
        if state.backend.is_none() {
            return HRESULT(state.fail(StreamFault::Disposed));
        }
        let count = match check_count(cb) {
            Ok(count) => count,
            Err(fault) => return HRESULT(state.fail(fault)),
        };
        let read = if count == 0 {
            // The backend still sees the call: a write-only `FileStream` throws NotSupported even for 0 bytes.
            state.read(&mut [])
        } else if pv.is_null() {
            return HRESULT(E_POINTER);
        } else {
            // SAFETY: `pv` is non-null and, by the IStream::Read contract, points to a caller buffer of at least
            // `cb` writable bytes that stays valid for the duration of this call (C# `new Span<byte>(pv, cb)`).
            let buf = unsafe { std::slice::from_raw_parts_mut(pv.cast::<u8>(), count) };
            state.read(buf)
        };
        match read {
            Ok(n) => {
                if !pcbread.is_null() {
                    // SAFETY: `pcbread` is non-null and points to the caller's ULONG out slot (IStream::Read contract).
                    unsafe { pcbread.write(n as u32) };
                }
                HRESULT(0)
            }
            Err(fault) => HRESULT(state.fail(fault)),
        }
    }

    /// C# `Write`: disposed, then `cb > i32::MAX` (`COR_E_ARGUMENTOUTOFRANGE`, before the read-only check); writes all
    /// `cb` bytes; `*pcbWritten = cb` when not null.
    fn Write(&self, pv: *const c_void, cb: u32, pcbwritten: *mut u32) -> HRESULT {
        let mut state = self.lock();
        if state.backend.is_none() {
            return HRESULT(state.fail(StreamFault::Disposed));
        }
        let count = match check_count(cb) {
            Ok(count) => count,
            Err(fault) => return HRESULT(state.fail(fault)),
        };
        let written = if count == 0 {
            state.write(&[])
        } else if pv.is_null() {
            return HRESULT(E_POINTER);
        } else {
            // SAFETY: `pv` is non-null and, by the IStream::Write contract, points to `cb` readable bytes that stay
            // valid for the duration of this call (C# `new ReadOnlySpan<byte>(pv, cb)`).
            let data = unsafe { std::slice::from_raw_parts(pv.cast::<u8>(), count) };
            state.write(data)
        };
        match written {
            Ok(()) => {
                if !pcbwritten.is_null() {
                    // SAFETY: `pcbwritten` is non-null and points to the caller's ULONG out slot (IStream::Write
                    // contract).
                    unsafe { pcbwritten.write(cb) };
                }
                HRESULT(0)
            }
            Err(fault) => HRESULT(state.fail(fault)),
        }
    }
}

impl IStream_Impl for ComStreamObject_Impl {
    /// C# `Seek(dlibMove, dwOrigin, plibNewPosition)`: `*plibNewPosition` = new absolute position when not null.
    fn Seek(&self, dlibmove: i64, dworigin: STREAM_SEEK, plibnewposition: *mut u64) -> windows_core::Result<()> {
        let mut state = self.lock();
        match state.seek(dlibmove, dworigin.0) {
            Ok(position) => {
                if !plibnewposition.is_null() {
                    // SAFETY: `plibnewposition` is non-null and points to the caller's ULARGE_INTEGER out slot
                    // (IStream::Seek contract); written only on success.
                    unsafe { plibnewposition.write(position) };
                }
                Ok(())
            }
            Err(fault) => Err(com_error(state.fail(fault))),
        }
    }

    /// C# `SetSize` -> `Stream.SetLength`.
    fn SetSize(&self, libnewsize: u64) -> windows_core::Result<()> {
        let mut state = self.lock();
        match state.set_size(libnewsize) {
            Ok(()) => Ok(()),
            Err(fault) => Err(com_error(state.fail(fault))),
        }
    }

    /// C# `CopyTo`: always `NotSupported` (also after dispose).
    fn CopyTo(
        &self,
        _pstm: windows_core::Ref<IStream>,
        _cb: u64,
        _pcbread: *mut u64,
        _pcbwritten: *mut u64,
    ) -> windows_core::Result<()> {
        Err(com_error(self.lock().fail(StreamFault::NotSupported)))
    }

    /// C# `Commit`: flushes; S_OK after dispose (WIC may commit after the owner disposed).
    fn Commit(&self, _grfcommitflags: &STGC) -> windows_core::Result<()> {
        let mut state = self.lock();
        match state.commit() {
            Ok(()) => Ok(()),
            Err(fault) => Err(com_error(state.fail(fault))),
        }
    }

    /// C# `Revert`: no-op, S_OK (also after dispose).
    fn Revert(&self) -> windows_core::Result<()> {
        Ok(())
    }

    /// C# `LockRegion`: no-op, S_OK (also after dispose).
    fn LockRegion(&self, _liboffset: u64, _cb: u64, _dwlocktype: &LOCKTYPE) -> windows_core::Result<()> {
        Ok(())
    }

    /// C# `UnlockRegion`: no-op, S_OK (also after dispose).
    fn UnlockRegion(&self, _liboffset: u64, _cb: u64, _dwlocktype: u32) -> windows_core::Result<()> {
        Ok(())
    }

    /// C# `Stat`: `type = STGTY_STREAM (2)`, `cbSize = length`, everything else 0 (name pointer null, `grfMode` 0 =
    /// `STGM_READ`); the flag is ignored. Zero all 80 bytes first (padding 12..16 is pinned by the golden), then
    /// write the fields. Errors are built with `Error::from_hresult` (never `Error::new` / `From<HRESULT>`).
    fn Stat(&self, pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> windows_core::Result<()> {
        let mut state = self.lock();
        let length = match state.length() {
            Ok(length) => length,
            Err(fault) => return Err(com_error(state.fail(fault))),
        };
        if pstatstg.is_null() {
            return Err(com_error(E_POINTER));
        }
        // SAFETY: `pstatstg` is non-null and points to the caller's writable STATSTG (IStream::Stat contract);
        // zeroing every byte (padding included) and then writing two fields in place stays inside that struct.
        unsafe {
            ptr::write_bytes(pstatstg, 0, 1);
            ptr::addr_of_mut!((*pstatstg).r#type).write(STGTY_STREAM);
            ptr::addr_of_mut!((*pstatstg).cbSize).write(length);
        }
        Ok(())
    }

    /// C# `Clone`: always `NotSupported`.
    fn Clone(&self) -> windows_core::Result<IStream> {
        Err(com_error(self.lock().fail(StreamFault::NotSupported)))
    }
}

/// `STGTY_STREAM`: the `STATSTG.type` of a stream.
const STGTY_STREAM: u32 = 2;

/// C# `ManagedIStream`: a COM `IStream` over a [`StreamBackend`], handed to WIC with [`ComStream::istream`].
/// The owner keeps the `ComStream`; dropping it disposes the backend (C# `using var stream = new ManagedIStream(..)`)
/// even if WIC still holds the COM object, whose later calls then fail with `COR_E_OBJECTDISPOSED`.
pub struct ComStream {
    object: ComObject<ComStreamObject>,
}

impl ComStream {
    /// `new ManagedIStream(stream)`.
    pub fn new(backend: Box<dyn StreamBackend>) -> Self {
        Self {
            object: ComObject::new(ComStreamObject {
                state: Mutex::new(StreamState {
                    backend: Some(backend),
                    last_fault: None,
                }),
            }),
        }
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
        self.object.to_interface()
    }

    /// `Read` without the pointers: bytes read.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, StreamFault> {
        self.object.lock().read(buf)
    }

    /// `Write` without the pointers.
    pub fn write(&self, data: &[u8]) -> Result<(), StreamFault> {
        self.object.lock().write(data)
    }

    /// `Seek(offset, origin)` with the raw `STREAM_SEEK` origin (0 set, 1 current, 2 end): the new position.
    pub fn seek(&self, offset: i64, origin: u32) -> Result<u64, StreamFault> {
        self.object.lock().seek(offset, origin)
    }

    /// `SetSize`.
    pub fn set_size(&self, size: u64) -> Result<(), StreamFault> {
        self.object.lock().set_size(size)
    }

    /// `Commit`: flush; `Ok` when disposed.
    pub fn commit(&self) -> Result<(), StreamFault> {
        self.object.lock().commit()
    }

    /// `Stat`.
    pub fn stat(&self) -> Result<STATSTG, StreamFault> {
        let length = self.object.lock().length()?;
        Ok(STATSTG {
            r#type: STGTY_STREAM,
            cbSize: length,
            ..STATSTG::default()
        })
    }

    /// `Dispose`: drops the stream from the COM object and returns it (`None` if already disposed).
    pub fn dispose(&self) -> Option<Box<dyn StreamBackend>> {
        self.object.lock().backend.take()
    }

    /// The fault of the last failed COM call on this stream (cleared by the read), so W4-11c can report the real
    /// `io::Error` when WIC turns a stream failure into its own HRESULT. Rust addition, see open questions.
    pub fn take_last_fault(&self) -> Option<StreamFault> {
        self.object.lock().last_fault.take()
    }
}

impl Drop for ComStream {
    fn drop(&mut self) {
        drop(self.dispose());
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
    match origin {
        0 => u64::try_from(offset)
            .map(SeekFrom::Start)
            .map_err(|_| StreamFault::SeekBeforeBegin),
        1 => Ok(SeekFrom::Current(offset)),
        2 => Ok(SeekFrom::End(offset)),
        other => Err(StreamFault::InvalidSeekOrigin(other)),
    }
}

/// `HRESULT_FROM_WIN32`: `0` stays `0`, a positive Win32 code `c` becomes `0x8007_0000 | (c & 0xFFFF)`.
pub fn hresult_from_win32(code: u32) -> i32 {
    if code as i32 <= 0 {
        code as i32
    } else {
        ((code & 0xFFFF) | 0x8007_0000) as i32
    }
}

// `raw-dylib`: no import library needed.
#[link(name = "windowscodecs", kind = "raw-dylib")]
unsafe extern "system" {
    /// Declared here because the `windows` crate does not export it (it is in `wincodec_proxy.h`, not in the
    /// Windows metadata the crate is generated from).
    fn WICCreateImagingFactory_Proxy(sdk_version: u32, factory: *mut *mut c_void) -> HRESULT;
}

#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    /// Asks a file handle's granted access (`FileAccessInformation`) for the .NET `CanRead`/`CanWrite` checks of the
    /// `File` backend; not in the `windows` features this crate enables.
    fn NtQueryInformationFile(
        handle: *mut c_void,
        io_status: *mut IoStatusBlock,
        information: *mut c_void,
        length: u32,
        class: i32,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::COINIT_APARTMENTTHREADED;

    /// One more reference on `raw` and returns the count, then gives it back (net zero).
    fn ref_count(raw: *mut c_void) -> u32 {
        // SAFETY: the callers pass the raw pointer of an `IStream` they keep alive; AddRef/Release are balanced.
        unsafe {
            let count = add_ref_raw(raw);
            release_raw(raw);
            count
        }
    }

    #[test]
    fn factory_from_proxy_releases_a_pointer_returned_with_a_failure() {
        let com = ComStream::writable_memory();
        let stream = com.istream();
        let raw = stream.as_raw();
        let before = ref_count(raw);
        // SAFETY: `raw` is live (held by `stream`); the extra reference is handed over to the call.
        unsafe { add_ref_raw(raw) };
        // SAFETY: as above: `raw` owns the reference just added.
        let result = unsafe { factory_from_proxy(HRESULT(WINCODEC_ERR_NOTINITIALIZED), raw) };
        assert_eq!(
            result.err(),
            Some(WicError::Com {
                hresult: WINCODEC_ERR_NOTINITIALIZED
            })
        );
        assert_eq!(
            ref_count(raw),
            before,
            "the reference returned with the failure is released"
        );
        // A failing HRESULT with no pointer: nothing to release.
        // SAFETY: null is allowed by the contract.
        let result = unsafe { factory_from_proxy(HRESULT(E_FAIL), ptr::null_mut()) };
        assert_eq!(result.err(), Some(WicError::Com { hresult: E_FAIL }));
        // Success HRESULT with an object that is no factory: NoInterface, the reference is released too.
        // SAFETY: `raw` is live; the added reference is handed over.
        unsafe { add_ref_raw(raw) };
        // SAFETY: as above.
        let result = unsafe { factory_from_proxy(HRESULT(0), raw) };
        assert_eq!(
            result.err(),
            Some(WicError::NoInterface {
                interface: "IWICImagingFactory"
            })
        );
        assert_eq!(ref_count(raw), before);
        // S_OK with null: E_FAIL.
        // SAFETY: null is allowed by the contract.
        let result = unsafe { factory_from_proxy(HRESULT(0), ptr::null_mut()) };
        assert_eq!(result.err(), Some(WicError::Com { hresult: E_FAIL }));
    }

    #[test]
    fn apartment_guard_uninitializes_only_what_it_initialized() {
        std::thread::spawn(|| {
            // SAFETY: no pointer is passed; changes this fresh thread's apartment only.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            assert_eq!(hr.0, 0, "fresh thread: S_OK");
            drop(ApartmentGuard { uninitialize: true });
            // SAFETY: as above.
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            assert_eq!(hr.0, 0, "the guard left the MTA, so the thread may enter an STA");
            drop(ApartmentGuard { uninitialize: false });
            // SAFETY: as above.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            assert_eq!(
                hr, RPC_E_CHANGED_MODE,
                "a guard that did not initialise leaves the STA in place"
            );
            // SAFETY: balances the successful STA CoInitializeEx above.
            unsafe { CoUninitialize() };
        })
        .join()
        .expect("apartment thread");
    }
}
