//! Port of `PhotoReview.Imaging.TurboJpeg/Native/TurboJpegNative.cs` and `Native/SafeTurboJpegHandle.cs` (W5-13a).
//!
//! The libjpeg-turbo 3.x boundary: the `tj3*` entry points (resolved at run time from `turbojpeg.dll`, which is never
//! linked at build time), the `TJINIT`/`TJPARAM`/`TJPF` values, the by-value `tjscalingfactor` struct and an owning
//! instance handle (C# `SafeTurboJpegHandle`) that releases through `tj3Destroy` exactly once.
//!
//! Loading follows the C# `[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.AssemblyDirectory)]` rule: the
//! DLL is only ever loaded by absolute path from the application folder (`current_exe()`'s directory), never through
//! the default search order (working directory, `PATH`), so a planted `turbojpeg.dll` is not picked up. A missing or
//! incompatible DLL is an error value (`NativeError::DllNotFound` / `BadImageFormat`), never a panic, so the
//! availability probe (W5-13b) reports "unavailable" exactly like the C# probe.
//!
//! Safety design (see contract W5-13a, "Semantics" 6..9): every `tj3*` call goes through a method of
//! [`TurboJpegHandle`] that refuses closed or invalid handles, and [`TurboJpegHandle::decompress8`] re-parses the header
//! of the very slice it is given and checks the destination length before the native call writes into it. This is the
//! only file of the crate with `unsafe`; every block has a `// SAFETY:` comment.
#![allow(unused_variables, dead_code)] // D11: stub with todo!() bodies; the porter removes this with the last todo!().

use std::ffi::{c_char, c_int, c_uchar, c_void};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use pr_imaging::decoder_traits::DecodeError;

/// C# `TurboJpegNative.DllName`. The file name only; the folder is always the application folder.
pub const DLL_NAME: &str = "turbojpeg.dll";

/// C# `WrapInitResult` message (`InvalidOperationException`); the C# test checks it contains "decompressor".
pub const INIT_FAILED_MESSAGE: &str = "Failed to initialize TurboJPEG decompressor instance.";

/// Rust-only twin of [`INIT_FAILED_MESSAGE`] for the test-support compressor (C# tests assert `tj3Init(0) != 0`).
pub const COMPRESSOR_INIT_FAILED_MESSAGE: &str = "Failed to initialize TurboJPEG compressor instance.";

/// .NET `ObjectDisposedException` message for a closed `SafeHandle` passed to a P/Invoke.
pub const HANDLE_CLOSED_MESSAGE: &str = "Safe handle has been closed.";

/// Rust-only guard message: a handle whose raw value is 0 or -1 is never passed to `tj3*` (DIVERGENCE, contract trap T6).
pub const HANDLE_INVALID_MESSAGE: &str = "TurboJPEG handle is invalid.";

/// C# `TjInit` (`TJINIT_*`).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TjInit {
    Compress = 0,
    Decompress = 1,
    Transform = 2,
}

impl TjInit {
    /// The `int` passed to `tj3Init` (C# `(int)TjInit.X`).
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}

/// C# `TjParam` (`TJPARAM_*`, libjpeg-turbo 3.0 numbering).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TjParam {
    StopOnWarning = 0,
    BottomUp = 1,
    NoRealloc = 2,
    Quality = 3,
    Subsamp = 4,
    JpegWidth = 5,
    JpegHeight = 6,
    Precision = 7,
    Colorspace = 8,
    FastUpsample = 9,
    FastDct = 10,
    Optimize = 11,
    Progressive = 12,
    ScanLimit = 13,
    Arithmetic = 14,
    Lossless = 15,
    LosslessPsv = 16,
    LosslessPt = 17,
    RestartBlocks = 18,
    RestartRows = 19,
    XDensity = 20,
    YDensity = 21,
    DensityUnits = 22,
}

impl TjParam {
    /// The `int` passed to `tj3Get`/`tj3Set` (C# `(int)TjParam.X`).
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}

/// C# `TjPixelFormat` (`TJPF_*`).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TjPixelFormat {
    Rgb = 0,
    Bgr = 1,
    Rgbx = 2,
    Bgrx = 3,
    Xbgr = 4,
    Xrgb = 5,
    Gray = 6,
    Rgba = 7,
    Bgra = 8,
    Abgr = 9,
    Argb = 10,
    Cmyk = 11,
}

impl TjPixelFormat {
    /// The `int` passed to `tj3Decompress8`/`tj3Compress8` (C# `(int)TjPixelFormat.X`).
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    /// libjpeg-turbo `tjPixelSize[]`: 3 for Rgb/Bgr, 1 for Gray, 4 for every other format (incl. Cmyk).
    pub fn bytes_per_pixel(self) -> i32 {
        todo!()
    }
}

/// C# `TjScalingFactor` (`tjscalingfactor`): passed BY VALUE to `tj3SetScalingFactor`, so the layout is `#[repr(C)]`
/// `{ int num; int denom; }`. Equality and hash need both fields (C# `Equals`/`GetHashCode`; derived here).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TjScalingFactor {
    pub num: c_int,
    pub denom: c_int,
}

impl TjScalingFactor {
    /// C# `TjScalingFactor.One` (1/1, no DCT scaling).
    pub const ONE: Self = Self { num: 1, denom: 1 };

    /// C# `new TjScalingFactor(num, denom)`.
    pub const fn new(num: i32, denom: i32) -> Self {
        Self { num, denom }
    }

    /// libjpeg-turbo `TJSCALED(dimension, factor)` in 64-bit: `(dimension * num + denom - 1) / denom` (integer division
    /// truncating toward zero, as C# `long` division). `None` when `denom <= 0` (never true for a factor that
    /// `tj3SetScalingFactor` accepted). Same arithmetic as C# `TurboJpegDecoder.CalculateOutputBuffer`.
    pub fn scaled(self, dimension: i32) -> Option<i64> {
        todo!()
    }
}

impl fmt::Display for TjScalingFactor {
    /// C# `ToString()`: `"{Num}/{Denom}"` (invariant, e.g. `"3/8"`, `"-1/8"`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// Errors of the native layer. Each variant is the .NET exception type the C# code would surface, so that the
/// conversion into [`DecodeError`] keeps the INV-12 fallback classification (`fallback.rs`): `DllNotFound`,
/// `BadImageFormat`, `EntryPointNotFound` fall back; `InvalidOperation`, `Disposed`, `InvalidArgument` do not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeError {
    /// .NET `DllNotFoundException`: the DLL (or one of its dependencies) is missing, or no application folder.
    DllNotFound { message: String },
    /// .NET `BadImageFormatException`: the DLL is not a valid image for this process (e.g. 32-bit DLL, x64 process).
    BadImageFormat { message: String },
    /// .NET `EntryPointNotFoundException`: the DLL loaded but lacks the `tj3*` export the call needs.
    EntryPointNotFound { message: String },
    /// .NET `InvalidOperationException`: `tj3Init` returned NULL ([`INIT_FAILED_MESSAGE`]), or a destroy seam failed.
    InvalidOperation { message: String },
    /// .NET `ObjectDisposedException`: a call on a closed handle ([`HANDLE_CLOSED_MESSAGE`]).
    Disposed { message: String },
    /// Rust-only precondition guards that keep the FFI sound (invalid raw handle, non-absolute DLL path, `pitch <= 0`,
    /// destination buffer too small). Unreachable from the C#-shaped call sequences of W5-14b (contract trap T6/T7).
    InvalidArgument { message: String },
}

impl NativeError {
    /// The exception message (what C# `ex.Message` would be).
    pub fn message(&self) -> &str {
        todo!()
    }
}

impl fmt::Display for NativeError {
    /// Writes [`NativeError::message`] only.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl std::error::Error for NativeError {}

impl From<NativeError> for DecodeError {
    /// 1:1 onto the `DecodeError` variant of the same .NET exception type, message unchanged.
    fn from(error: NativeError) -> Self {
        todo!()
    }
}

// ---- raw entry points (libjpeg-turbo 3.0 `turbojpeg.h`; cdecl == the x64 C ABI) ----

type Tj3Init = unsafe extern "C" fn(init_type: c_int) -> *mut c_void;
type Tj3Destroy = unsafe extern "C" fn(handle: *mut c_void);
type Tj3DecompressHeader =
    unsafe extern "C" fn(handle: *mut c_void, jpeg_buf: *const c_uchar, jpeg_size: usize) -> c_int;
type Tj3Get = unsafe extern "C" fn(handle: *mut c_void, param: c_int) -> c_int;
type Tj3Set = unsafe extern "C" fn(handle: *mut c_void, param: c_int, value: c_int) -> c_int;
type Tj3SetScalingFactor = unsafe extern "C" fn(handle: *mut c_void, scaling_factor: TjScalingFactor) -> c_int;
type Tj3Decompress8 = unsafe extern "C" fn(
    handle: *mut c_void,
    jpeg_buf: *const c_uchar,
    jpeg_size: usize,
    dst_buf: *mut c_uchar,
    pitch: c_int,
    pixel_format: c_int,
) -> c_int;
type Tj3GetErrorStr = unsafe extern "C" fn(handle: *mut c_void) -> *mut c_char;
type Tj3Compress8 = unsafe extern "C" fn(
    handle: *mut c_void,
    src_buf: *const c_uchar,
    width: c_int,
    pitch: c_int,
    height: c_int,
    pixel_format: c_int,
    jpeg_buf: *mut *mut c_uchar,
    jpeg_size: *mut usize,
) -> c_int;
type Tj3Free = unsafe extern "C" fn(buffer: *mut c_void);

/// Every export this layer uses, resolved once at load. A missing export is `None` and surfaces as
/// `EntryPointNotFound` only when it is first CALLED (C# P/Invoke binds lazily per entry point; contract trap T4).
#[derive(Clone, Copy, Default)]
struct Symbols {
    init: Option<Tj3Init>,
    destroy: Option<Tj3Destroy>,
    decompress_header: Option<Tj3DecompressHeader>,
    get: Option<Tj3Get>,
    set: Option<Tj3Set>,
    set_scaling_factor: Option<Tj3SetScalingFactor>,
    decompress8: Option<Tj3Decompress8>,
    get_error_str: Option<Tj3GetErrorStr>,
    /// Test support only (C# `TurboJpegSyntheticFormatTests` declares its own `tj3Compress8`/`tj3Free`).
    compress8: Option<Tj3Compress8>,
    free: Option<Tj3Free>,
}

/// kernel32 loader calls. Declared by hand: `pr-turbojpeg` takes no `windows` crate dependency (D36 keeps it in
/// `pr-platform-win`) and no `libloading` (no new external crate; contract open question 1).
#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;

    /// `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR`: dependencies of the DLL are looked up in the DLL's own folder.
    pub(super) const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: u32 = 0x0000_0100;
    /// `LOAD_LIBRARY_SEARCH_DEFAULT_DIRS`: application dir + System32 + AddDllDirectory dirs; never cwd or `PATH`.
    pub(super) const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: u32 = 0x0000_1000;
    /// `ERROR_FILE_NOT_FOUND`, `ERROR_PATH_NOT_FOUND`, `ERROR_MOD_NOT_FOUND`: map to `DllNotFound`.
    pub(super) const ERROR_FILE_NOT_FOUND: i32 = 2;
    pub(super) const ERROR_PATH_NOT_FOUND: i32 = 3;
    pub(super) const ERROR_MOD_NOT_FOUND: i32 = 126;
    /// `ERROR_BAD_EXE_FORMAT`: maps to `BadImageFormat` (wrong architecture or not a PE image).
    pub(super) const ERROR_BAD_EXE_FORMAT: i32 = 193;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub(super) fn LoadLibraryExW(lib_file_name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        pub(super) fn GetProcAddress(module: *mut c_void, proc_name: *const u8) -> *mut c_void;
        pub(super) fn FreeLibrary(lib_module: *mut c_void) -> i32;
    }
}

/// The loaded `turbojpeg.dll` plus its resolved exports. Dropping the last `Arc` frees the module; every
/// [`TurboJpegHandle`] created from it holds an `Arc`, so the module outlives its handles. The process-wide instance
/// ([`TurboJpegLibrary::shared`]) is never dropped (C# keeps a DllImport'ed library loaded for the process lifetime).
pub struct TurboJpegLibrary {
    /// `HMODULE` bits (0 never stored: a failed load is an `Err`).
    module: usize,
    /// The absolute path the module was loaded from.
    path: PathBuf,
    symbols: Symbols,
}

impl fmt::Debug for TurboJpegLibrary {
    /// Prints the path and which exports resolved (never the module pointer value).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl TurboJpegLibrary {
    /// Loads `path` (must be absolute; a relative path is `InvalidArgument` and nothing is searched) with
    /// `LoadLibraryExW(path, NULL, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS)` and resolves
    /// every `tj3*` export of [`Symbols`] with `GetProcAddress` (missing ones stay `None`). Load errors:
    /// `ERROR_BAD_EXE_FORMAT` -> `BadImageFormat`, everything else -> `DllNotFound`, message
    /// `"Unable to load DLL 'turbojpeg.dll' or one of its dependencies: {os message}"`. Non-Windows targets always
    /// return `DllNotFound`. Never panics.
    pub fn load(path: &Path) -> Result<Arc<TurboJpegLibrary>, NativeError> {
        todo!()
    }

    /// The process-wide library from [`default_library_path`], loaded on first use and cached in a `OnceLock`
    /// (success AND failure are cached: the C# probe result is cached too, and W5-14b only runs when it succeeded).
    pub fn shared() -> Result<Arc<TurboJpegLibrary>, NativeError> {
        todo!()
    }

    /// The absolute path this module was loaded from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// C# `TurboJpegNative.CreateDecompressor()` against THIS library: `tj3Init(TJINIT_DECOMPRESS)` then
    /// [`wrap_init_result`] semantics (NULL -> `InvalidOperation(INIT_FAILED_MESSAGE)`). Missing `tj3Init` ->
    /// `EntryPointNotFound` naming `tj3Init`. The handle is owning and holds an `Arc` of this library.
    pub fn create_decompressor(self: &Arc<Self>) -> Result<TurboJpegHandle, NativeError> {
        todo!()
    }

    /// Test support: `tj3Init(TJINIT_COMPRESS)`; NULL -> `InvalidOperation(COMPRESSOR_INIT_FAILED_MESSAGE)`.
    pub fn create_compressor(self: &Arc<Self>) -> Result<TurboJpegHandle, NativeError> {
        todo!()
    }
}

impl Drop for TurboJpegLibrary {
    /// `FreeLibrary(module)`; the result is ignored (nothing can be done about a failed unload).
    fn drop(&mut self) {
        todo!()
    }
}

/// `<directory of std::env::current_exe()>\turbojpeg.dll` (C# `DllImportSearchPath.AssemblyDirectory`: the app
/// folder). `None` when the executable path cannot be determined. No environment variable can redirect it.
pub fn default_library_path() -> Option<PathBuf> {
    todo!()
}

/// C# `TurboJpegNative.CreateDecompressor()`: [`TurboJpegLibrary::shared`] then
/// [`TurboJpegLibrary::create_decompressor`]. A missing DLL is `DllNotFound` (C# `DllNotFoundException`).
pub fn create_decompressor() -> Result<TurboJpegHandle, NativeError> {
    todo!()
}

/// C# `TurboJpegNative.WrapInitResult(IntPtr)`: 0 -> `InvalidOperation(INIT_FAILED_MESSAGE)`; any other value
/// (including -1, exactly like C#) -> an owning handle bound to the process-wide library, which is only resolved
/// when the handle is used or released (C#'s static `tj3Destroy` binds lazily too). Loads nothing itself.
pub fn wrap_init_result(ptr: isize) -> Result<TurboJpegHandle, NativeError> {
    todo!()
}

/// Release override (C# `SafeTurboJpegHandle(IntPtr, bool, Action<IntPtr>?)` test seam). Gets the raw value; an
/// `Err` is what C#'s throwing `Action` would propagate out of `Dispose`.
pub type DestroyFn = Box<dyn FnMut(isize) -> Result<(), NativeError> + Send>;

/// Where a handle's native calls (and its default release) go.
enum Binding {
    /// Created by [`TurboJpegLibrary::create_decompressor`]/`create_compressor`.
    Library(Arc<TurboJpegLibrary>),
    /// Created by [`TurboJpegHandle::from_raw`]/[`wrap_init_result`]: resolved via [`TurboJpegLibrary::shared`] on use.
    Shared,
}

/// C# `SafeTurboJpegHandle` (`SafeHandleZeroOrMinusOneIsInvalid`): an owning `tjhandle`.
///
/// - `is_invalid` is true for raw values 0 and -1; such a handle never reaches `tj3Destroy` or any `tj3*` call.
/// - `close` (C# `Dispose`) is idempotent: the first call marks the handle closed and, when it owns a valid value,
///   calls the destroy seam or `tj3Destroy` exactly once; later calls do nothing. `Drop` calls `close` and ignores
///   its error (C# finalizer).
/// - All native calls take `&mut self` (a `tjhandle` must not be used from two threads at once). The type is `Send`
///   (moving a `tjhandle` between threads is allowed) and not `Sync`.
pub struct TurboJpegHandle {
    /// C# `IntPtr handle` (signed, so the -1 sentinel is representable).
    raw: isize,
    /// C# `ownsHandle`.
    owns: bool,
    /// C# `IsClosed`.
    closed: bool,
    binding: Binding,
    /// `Some` = the test seam replaces `tj3Destroy`.
    destroy: Option<DestroyFn>,
    /// The scaling factor libjpeg-turbo holds for this instance: `ONE` until a `set_scaling_factor` call returned 0.
    /// Used only by the `decompress8` destination-size guard.
    scaling: TjScalingFactor,
}

impl fmt::Debug for TurboJpegHandle {
    /// Prints raw value, owns, closed, whether a destroy seam is set and the scaling factor.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl TurboJpegHandle {
    /// C# `new SafeTurboJpegHandle(IntPtr preexistingHandle, bool ownsHandle)`: released through the process-wide
    /// library's `tj3Destroy`. Does not load the library.
    pub fn from_raw(raw: isize, owns: bool) -> Self {
        todo!()
    }

    /// C# `new SafeTurboJpegHandle(IntPtr, bool, Action<IntPtr>? destroy)` with a non-null `destroy` (test seam):
    /// `destroy` replaces `tj3Destroy`. Native calls on this handle still go to the process-wide library.
    pub fn with_destroy(
        raw: isize,
        owns: bool,
        destroy: impl FnMut(isize) -> Result<(), NativeError> + Send + 'static,
    ) -> Self {
        todo!()
    }

    /// C# `IsInvalid`: raw value 0 or -1 (independent of `closed`).
    pub fn is_invalid(&self) -> bool {
        todo!()
    }

    /// C# `IsClosed`.
    pub fn is_closed(&self) -> bool {
        todo!()
    }

    /// C# `DangerousGetHandle()`: the raw value, also after close.
    pub fn raw_value(&self) -> isize {
        todo!()
    }

    /// C# `SetHandleAsInvalid()`: marks the handle closed WITHOUT releasing it (tests use it on fake pointers).
    pub fn set_handle_as_invalid(&mut self) {
        todo!()
    }

    /// C# `Dispose()` / `ReleaseHandle()`. First call: `closed = true`, then if `owns` and the raw value is neither 0
    /// nor -1, the seam (if set) or `tj3Destroy(raw)` runs once and its error is returned (the handle stays closed).
    /// A `Shared` binding whose library cannot be loaded returns that error (handle leaked, closed). Later calls: `Ok`.
    pub fn close(&mut self) -> Result<(), NativeError> {
        todo!()
    }

    /// `tj3DecompressHeader(handle, jpeg.as_ptr(), jpeg.len())`. `Ok(code)` is the raw native return (0 = success,
    /// -1 = failure, see [`error_message`](Self::error_message)), exactly what C# compares with 0. `Err` only for
    /// a closed (`Disposed`) or invalid (`InvalidArgument`) handle, an unloadable library or a missing export.
    /// An empty slice is passed through (libjpeg-turbo rejects size 0 itself and sets the error string).
    pub fn decompress_header(&mut self, jpeg: &[u8]) -> Result<i32, NativeError> {
        todo!()
    }

    /// `tj3Get(handle, param)`; `Ok` is the native value (-1 for an unknown or not-yet-known parameter).
    pub fn get(&mut self, param: TjParam) -> Result<i32, NativeError> {
        todo!()
    }

    /// `tj3Set(handle, param, value)`; `Ok` is the native return (0 / -1).
    pub fn set(&mut self, param: TjParam, value: i32) -> Result<i32, NativeError> {
        todo!()
    }

    /// `tj3SetScalingFactor(handle, factor)` (struct by value); `Ok` is the native return. On `Ok(0)` the factor
    /// is remembered for the `decompress8` guard; on any other result the remembered factor is unchanged.
    pub fn set_scaling_factor(&mut self, factor: TjScalingFactor) -> Result<i32, NativeError> {
        todo!()
    }

    /// `tj3Decompress8(handle, jpeg, len, dst, pitch, format)` behind a soundness guard (contract "Semantics" 8):
    /// 1. closed/invalid handle, missing exports -> `Err` as above; `pitch <= 0` -> `Err(InvalidArgument)`;
    /// 2. `tj3DecompressHeader` on the SAME `jpeg` slice; a non-zero return is returned as `Ok(code)` unchanged
    ///    (the native error string stays readable through `error_message`);
    /// 3. `w = scaled(JpegWidth)`, `h = scaled(JpegHeight)` with the remembered factor; `pitch < w * bytes_per_pixel`
    ///    or `dst.len() < pitch * h` (all in `i64`) -> `Err(InvalidArgument)` and NO native decode;
    /// 4. otherwise the native call; `Ok` is its return (0 / -1).
    pub fn decompress8(
        &mut self,
        jpeg: &[u8],
        dst: &mut [u8],
        pitch: i32,
        format: TjPixelFormat,
    ) -> Result<i32, NativeError> {
        todo!()
    }

    /// C# `TurboJpegNative.GetErrorMessage(handle)`: `tj3GetErrorStr(handle)` copied at once (`CStr`, lossy UTF-8;
    /// libjpeg-turbo messages are ASCII, so this equals C# `Marshal.PtrToStringAnsi`). `None` for a NULL pointer, and
    /// also when the handle is closed/invalid or the export/library is missing (no `Err` path: C# callers use it only
    /// to enrich another error, always with a `?? "..."` fallback).
    pub fn error_message(&mut self) -> Option<String> {
        todo!()
    }

    /// Test support (C# `TurboJpegSyntheticFormatTests.Compress`): `tj3Compress8` with a library-allocated output
    /// (`jpegBuf = NULL`, `jpegSize = 0`), the bytes copied into a `Vec`, then `tj3Free`. Guard: `src.len() >= pitch *
    /// height` with `pitch >= width * bytes_per_pixel` and all three > 0, else `InvalidArgument`. A non-zero native
    /// return is `Err(InvalidOperation("tj3Compress8 failed: {error string}"))`; the buffer is freed on every path.
    pub fn compress8(
        &mut self,
        src: &[u8],
        width: i32,
        pitch: i32,
        height: i32,
        format: TjPixelFormat,
    ) -> Result<Vec<u8>, NativeError> {
        todo!()
    }
}

impl Drop for TurboJpegHandle {
    /// `let _ = self.close();`
    fn drop(&mut self) {
        todo!()
    }
}

/// The cache behind [`TurboJpegLibrary::shared`].
static SHARED: OnceLock<Result<Arc<TurboJpegLibrary>, NativeError>> = OnceLock::new();
