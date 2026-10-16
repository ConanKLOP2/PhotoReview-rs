//! Port of `PhotoReview.Imaging.LibRaw/LibRawNativeMethods.cs` and `PhotoReview.Imaging.LibRaw/SafeLibRawHandle.cs`
//! (C# baseline d92d6aae): the LibRaw C API binding of the Rust app.
//!
//! `libraw.dll` stays dynamically linked (LGPL/CDDL) and is loaded at RUN time, only from the folder of the running
//! executable (C# `[assembly: DefaultDllImportSearchPaths(AssemblyDirectory)]`): never from the working directory or
//! PATH, so a planted `libraw.dll` is never loaded. A missing or wrong DLL is not an error of this module: [`api`] reports
//! it as a [`LoadError`] and everything above it (W5-15b availability probe, W5-16 decoder) degrades to "LibRaw
//! unavailable".
//!
//! Every `unsafe` of this unit lives in this file (FFI only, each block with a `// SAFETY:` comment; the W5-15b probe
//! adapter and, unless the lead removes it, W5-16c's progress callback are the only other `unsafe` of `pr-libraw`). The pure parts
//! (layout proof, iparams plausibility check, error text, cancellation verdict, handle ownership) are safe functions over
//! plain data so they are tested without the DLL.
//!
//! PARITY: I-15 (`libraw.dll` loaded only from the app folder), I-14 (LibRaw is the RAW preview fallback), S-09.

// Stub scaffolding (D11): the porter removes this line together with the last `todo!()`.
#![allow(unused_variables, unused_imports, dead_code)]

use std::error::Error;
use std::ffi::{OsStr, c_char, c_int, c_uint, c_void};
use std::fmt;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::{Arc, OnceLock};

use pr_core::sync::CancelToken;

// ------------------------------------------------------------------------------------------------ constants

/// C# `LibRawNativeMethods.LibraryName`.
pub const LIBRARY_NAME: &str = "libraw.dll";

/// `LIBRAW_IMAGE_BITMAP` (`libraw_processed_image_t::type`). C# `ImageBitmap`.
pub const IMAGE_BITMAP: i32 = 2;

/// `LIBRAW_IMAGE_JPEG`. C# `ImageJpeg`.
pub const IMAGE_JPEG: i32 = 1;

/// Every `libraw_*` entry point this module binds (C# `EntryPoints`, WP-34), in the C# order. [`LibRawApi`] has exactly
/// one field per name (the test `entry_points_match_the_api_table_declarations` scans this file's source between the
/// `BEGIN ENTRY POINTS` / `END ENTRY POINTS` markers). W5-15b builds `RequiredExports` from this list.
pub const ENTRY_POINTS: [&str; 23] = [
    "libraw_init",
    "libraw_close",
    "libraw_open_wfile",
    "libraw_open_buffer",
    "libraw_unpack",
    "libraw_unpack_thumb",
    "libraw_dcraw_process",
    "libraw_dcraw_make_mem_image",
    "libraw_dcraw_make_mem_thumb",
    "libraw_dcraw_clear_mem",
    "libraw_adjust_sizes_info_only",
    "libraw_get_iwidth",
    "libraw_get_iheight",
    "libraw_get_raw_width",
    "libraw_get_raw_height",
    "libraw_get_decoder_info",
    "libraw_get_iparams",
    "libraw_set_progress_handler",
    "libraw_set_output_color",
    "libraw_set_output_bps",
    "libraw_set_no_auto_bright",
    "libraw_version",
    "libraw_strerror",
];

/// `LoadLibraryExW` flag used for the app-folder load: the path is absolute and the DLL's own dependencies are searched
/// from the DLL's folder first (what .NET does for `DllImportSearchPath.AssemblyDirectory`).
pub const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x0000_0008;

/// `LoadLibraryExW` flag for [`LibraryHandle::load_system`] (System32 only; used by the W5-15b kernel32 probe test).
pub const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0000_0800;

// libraw_iparams_t (LibRaw 0.22.x, x64 MSVC, libraw_types.h of 0.22.2; ints 4 B, no packing pragma). libraw_get_iparams
// returns a pointer to the struct itself, so only offsets INSIDE it matter: guard[4] + make/model/software/
// normalized_make/normalized_model (5 x 64 B) = 324 -> maker_index 324, raw_count 328, dng_version 332, is_foveon 336,
// colors 340, filters 344, xtrans[6][6] 348, xtrans_abs[6][6] 384, cdesc[5] 420.

/// C# `IParamsColorsOffset`.
pub const IPARAMS_COLORS_OFFSET: usize = 340;
/// C# `IParamsFiltersOffset`.
pub const IPARAMS_FILTERS_OFFSET: usize = 344;
/// C# `IParamsCdescOffset`.
pub const IPARAMS_CDESC_OFFSET: usize = 420;
/// Bytes of `libraw_iparams_t` the reader looks at: up to and including `cdesc[4]` (420 + 5). The native wrapper views
/// exactly this many bytes; [`try_read_raw_structure`] returns `None` for a shorter block.
pub const IPARAMS_READ_LEN: usize = IPARAMS_CDESC_OFFSET + 5;

/// Byte offsets, from the `libraw_data_t` pointer `libraw_init` returns, of the `libraw_output_params_t` fields the
/// white-balance write depends on. C# `WhiteBalanceLayout` (record struct of four `int`). Kept `i32` like C# so the
/// tests can build shifted layouts (`real.output_color - 8`); an offset outside the probe window is a layout mismatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WhiteBalanceLayout {
    pub output_color: i32,
    pub output_bps: i32,
    pub no_auto_bright: i32,
    pub use_camera_wb: i32,
}

/// LibRaw 0.22.x, x64 MSVC, derived from libraw_types.h of 0.22.2: `offsetof(libraw_data_t, params) = 5232`;
/// `use_camera_wb` +152, `output_color` +160, `output_bps` +200, `no_auto_bright` +256. C# `PinnedWhiteBalanceLayout`.
pub const PINNED_WHITE_BALANCE_LAYOUT: WhiteBalanceLayout = WhiteBalanceLayout {
    output_color: 5392,
    output_bps: 5432,
    no_auto_bright: 5488,
    use_camera_wb: 5384,
};

/// Exclusive upper bound of `offset + 4` for a raw `i32` read/write inside a live `libraw_data_t` (see the SAFETY note of
/// the [`WhiteBalanceTarget`] impl of [`LibRawHandle`]: `libraw_data_t` of 0.22.2 is far larger, its `color` member
/// (after `params`, `rawparams`, `progress_flags`, `process_warnings`) starts with `ushort curve[0x10000]` = 128 KiB).
/// Rust-only guard; C# reads wherever the layout says.
pub const DATA_PROBE_LIMIT: i32 = 65_536;

const SENTINEL_OUTPUT_COLOR: i32 = 0x5EA1_0C01;
const SENTINEL_OUTPUT_BPS: i32 = 0x5EA1_0C02;
const SENTINEL_NO_AUTO_BRIGHT: i32 = 0x5EA1_0C03;

// ------------------------------------------------------------------------------------------------ pure logic

/// The file's sampling structure read from `libraw_iparams_t` (C# `DecodeMemoryGuard.RawStructure(int Colors, uint
/// Filters)` + `IsBayer`). Defined by W5-16b in `crate::memory_guard` (where C# defines it; pure data, no FFI layout);
/// re-exported here so callers of the native reader can name it as `native::RawStructure` too. One type, two paths.
pub use crate::memory_guard::RawStructure;

/// C# `TryReadRawStructure(IntPtr iparams)` over a byte view of the `libraw_iparams_t` block (native-endian, unaligned
/// reads, like `Marshal.ReadInt32`). `None` (fail closed) when the block is shorter than [`IPARAMS_READ_LEN`] (the C#
/// null pointer twin is the empty slice), when `colors` is outside `1..=4`, or when any of the first `colors` bytes of
/// `cdesc` is not an ASCII capital letter `A..=Z`. Only `colors` bytes are checked. `filters` is the `i32` reinterpreted
/// as `u32` (C# `unchecked((uint)...)`). Never panics.
pub fn try_read_raw_structure(block: &[u8]) -> Option<RawStructure> {
    todo!()
}

/// What the white-balance layout proof writes through and reads from: the three supported LibRaw setters plus raw `i32`
/// access at byte offsets of the `libraw_data_t` block. Implemented by [`LibRawHandle`] (native) and by the tests' fake
/// block (a `Vec<u8>` whose setters write where a LibRaw with a given real layout would). C# passes a pointer and three
/// `Action<int>` instead; one trait avoids aliasing the block between the closures and the reads.
pub trait WhiteBalanceTarget {
    fn set_output_color(&mut self, value: i32);
    fn set_output_bps(&mut self, value: i32);
    fn set_no_auto_bright(&mut self, value: i32);
    /// The native-endian `i32` at `offset`, or `None` when `offset..offset + 4` is outside the block (a layout that points
    /// outside the block does not match).
    fn read_i32(&self, offset: i32) -> Option<i32>;
    /// Writes the `i32` at `offset`; `false` (nothing written) when outside the block.
    fn write_i32(&mut self, offset: i32, value: i32) -> bool;
}

/// C# `TrySetUseCameraWb(IntPtr, Action<int> x3, int, int, int, Func<bool>, WhiteBalanceLayout)`: the layout proof.
///
/// 1. `version_pinned()` (called exactly once) false => return `false` with NO setter call and no memory access.
/// 2. Set the three sentinels through the setters (color, bps, bright in that order), read them back at `layout`.
/// 3. Set the REAL `output_color`, `output_bps`, `no_auto_bright` through the setters (always, whatever step 2 found).
/// 4. Mismatch (any read differs or is `None`) => `false`, `use_camera_wb` untouched.
/// 5. Write `1` at `layout.use_camera_wb`, return whether it reads back as `1`.
pub fn try_set_use_camera_wb_core<T: WhiteBalanceTarget + ?Sized>(
    target: &mut T,
    output_color: i32,
    output_bps: i32,
    no_auto_bright: i32,
    version_pinned: impl FnOnce() -> bool,
    layout: WhiteBalanceLayout,
) -> bool {
    todo!()
}

/// C# `FormatError` text rule: the `libraw_strerror` message, or `"LibRaw error {code}"` when it is `None`, empty or only
/// whitespace (C# `string.IsNullOrWhiteSpace`). The message is returned unchanged (no trim).
pub fn format_error_message(code: i32, message: Option<&str>) -> String {
    todo!()
}

/// C# `FormatError(int)`: [`format_error_message`] over `libraw_strerror(code)`. When the library is unavailable the
/// fallback text is returned (C# would throw `DllNotFoundException`; unreachable there because a decode never starts
/// without a successful probe).
pub fn format_error(code: i32) -> String {
    todo!()
}

/// The loaded runtime's version string (for example `"0.22.2-Release"`), `None` when the library is unavailable or
/// `libraw_version` returns null. C# `GetVersionString`. ANSI text decoded lossily (LibRaw strings are ASCII).
pub fn version_string() -> Option<String> {
    todo!()
}

/// The progress-callback verdict (C# `LibRawDecoder.CheckCancellation` minus the `GCHandle` lookup): `1` (abort) when
/// the token is cancelled or there is no token (C# null/bad pointer => `1`), else `0`.
pub fn progress_verdict(token: Option<&CancelToken>) -> i32 {
    todo!()
}

/// The `libraw_progress_handler` installed by [`LibRawHandle::set_cancel_token`]. `data` is the address of the handle's
/// boxed [`CancelToken`] (or null). Must never unwind into C: it only reads an atomic.
unsafe extern "C" fn cancellation_progress_callback(
    data: *mut c_void,
    stage: c_int,
    iteration: c_int,
    expected: c_int,
) -> c_int {
    todo!()
}

/// `exe.parent().join(name)`; `None` when `exe` has no parent folder: `parent()` is `None` OR the empty path (a bare
/// `"PhotoReviewRs.exe"` has `parent() == Some("")`). Never falls back to a bare name, which would let `LoadLibraryExW`
/// search PATH / the working directory.
pub fn library_path_next_to(exe: &Path, name: &str) -> Option<PathBuf> {
    todo!()
}

// ------------------------------------------------------------------------------------------------ handle ownership

/// Ownership of one native pointer with a release action: the Rust twin of `SafeHandleZeroOrMinusOneIsInvalid` as used
/// by `SafeLibRawHandle` and `SafeLibRawImageHandle` (C# test seam constructors `(IntPtr, Action<IntPtr>)`).
///
/// - `raw == 0` or `raw == usize::MAX` is invalid: `release` is never called.
/// - [`close`](Self::close) is idempotent: `release(raw)` runs at most once (first close or drop); afterwards
///   [`is_closed`](Self::is_closed) is `true` (also for an invalid handle, like `SafeHandle.IsClosed` after `Dispose`).
/// - [`raw`](Self::raw) keeps returning the value after close (C# `DangerousGetHandle`).
///
/// Safe to build with any value: it never dereferences `raw`, only hands it to `release`.
pub struct NativeOwner {
    raw: usize,
    release: Option<Box<dyn FnOnce(usize) + Send>>,
    closed: bool,
}

impl NativeOwner {
    pub fn new(raw: usize, release: impl FnOnce(usize) + Send + 'static) -> Self {
        todo!()
    }

    pub fn raw(&self) -> usize {
        todo!()
    }

    pub fn is_invalid(&self) -> bool {
        todo!()
    }

    pub fn is_closed(&self) -> bool {
        todo!()
    }

    pub fn close(&mut self) {
        todo!()
    }
}

impl Drop for NativeOwner {
    fn drop(&mut self) {
        todo!()
    }
}

impl fmt::Debug for NativeOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeOwner")
            .field("raw", &format_args!("{:#x}", self.raw))
            .field("closed", &self.closed)
            .finish()
    }
}

// ------------------------------------------------------------------------------------------------ dynamic library

/// Why the library (or one export) could not be used. `Display` is diagnostic only; W5-15b writes its own reason texts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// `current_exe()` failed or has no parent folder.
    NoExecutableDirectory,
    /// `LoadLibraryExW` returned null; `os_error` is `GetLastError()`.
    LoadFailed { path: PathBuf, os_error: u32 },
    /// The library loaded but does not export `name`.
    MissingExport { name: &'static str },
    /// Not built for Windows: there is no `libraw.dll` to load.
    Unsupported,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl Error for LoadError {}

/// An `HMODULE` from `LoadLibraryExW`, freed with `FreeLibrary` on drop (C# `NativeLibrary.TryLoad` / `Free`). Used by
/// the W5-15b probe (load, check exports, call version/init/close, free) and, leaked into a `static`, by [`api`].
pub struct LibraryHandle {
    module: NonNull<c_void>,
}

impl LibraryHandle {
    /// Loads `name` from the running executable's folder only: `library_path_next_to(current_exe()?, name)` with
    /// [`LOAD_WITH_ALTERED_SEARCH_PATH`]. Missing file, wrong architecture, missing dependent runtime => `LoadFailed`.
    pub fn load_next_to_exe(name: &str) -> Result<Self, LoadError> {
        todo!()
    }

    /// Loads an absolute `path` with [`LOAD_WITH_ALTERED_SEARCH_PATH`]. A relative path (`!path.is_absolute()`, which on
    /// Windows includes `C:foo` and `\foo`) is refused (`LoadFailed` with `os_error` 87 = `ERROR_INVALID_PARAMETER`)
    /// without calling the loader. The relative check runs BEFORE the `cfg(not(windows))` => `Unsupported` branch, so
    /// that refusal is the same on every platform.
    pub fn load_from(path: &Path) -> Result<Self, LoadError> {
        todo!()
    }

    /// Loads a system DLL by name from System32 only ([`LOAD_LIBRARY_SEARCH_SYSTEM32`]); for the W5-15b
    /// "library without LibRaw exports" test (`kernel32.dll`).
    pub fn load_system(name: &str) -> Result<Self, LoadError> {
        todo!()
    }

    /// `GetProcAddress(name) != null`; `false` for a name containing NUL. C# `NativeLibrary.TryGetExport`.
    pub fn has_export(&self, name: &str) -> bool {
        todo!()
    }

    /// The names of `names` this library does not export, in the order given.
    pub fn missing_exports<'a>(&self, names: &[&'a str]) -> Vec<&'a str> {
        todo!()
    }

    /// Calls the library's `libraw_version()`: `None` when not exported, `Some(None)` when it returned null, else the
    /// lossily decoded text.
    ///
    /// # Safety
    /// The library must be a LibRaw build: its `libraw_version` export must have the C signature
    /// `const char *libraw_version(void)` and return a static string.
    pub unsafe fn call_version(&self) -> Option<Option<String>> {
        todo!()
    }

    /// Calls `libraw_init(0)` and, when it returned a non-null handle, `libraw_close` on it. `None` when either export
    /// is missing, `Some(false)` when init returned null, `Some(true)` after a successful init/close pair.
    ///
    /// # Safety
    /// Same as [`call_version`](Self::call_version): `libraw_init` / `libraw_close` must have LibRaw's C signatures.
    pub unsafe fn call_init_close(&self) -> Option<bool> {
        todo!()
    }

    fn export(&self, name: &str) -> Option<NonNull<c_void>> {
        todo!()
    }
}

impl Drop for LibraryHandle {
    fn drop(&mut self) {
        todo!()
    }
}

impl fmt::Debug for LibraryHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibraryHandle").field("module", &self.module).finish()
    }
}

// SAFETY: an HMODULE is a process-wide value usable from any thread; GetProcAddress and FreeLibrary are thread-safe.
unsafe impl Send for LibraryHandle {}
// SAFETY: as above; `&LibraryHandle` only allows GetProcAddress and calls through resolved pointers.
unsafe impl Sync for LibraryHandle {}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(file_name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, proc_name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

// ------------------------------------------------------------------------------------------------ C API table

/// `libraw_processed_image_t` header (C# `LibRawDecoder.ProcessedImageHeader`, `LayoutKind.Sequential`):
/// `int type; ushort height, width, colors, bits; uint data_size; uchar data[]` at [`PROCESSED_IMAGE_DATA_OFFSET`].
/// Defined here (FFI layout); W5-16 uses it instead of declaring its own.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProcessedImageHeader {
    pub image_type: i32,
    pub height: u16,
    pub width: u16,
    pub colors: u16,
    pub bits: u16,
    pub data_size: u32,
}

impl ProcessedImageHeader {
    /// C# `ProcessedImageHeader.DataOffset`: byte offset of `data[]` from the start of the block (the name W5-16c uses).
    pub const DATA_OFFSET: usize = 16;
}

/// C# `ProcessedImageHeader.DataOffset` (same value as [`ProcessedImageHeader::DATA_OFFSET`]).
pub const PROCESSED_IMAGE_DATA_OFFSET: usize = ProcessedImageHeader::DATA_OFFSET;

/// `libraw_decoder_info_t`: the selected `load_raw` decoder's name (static C string) and flags. C# `DecoderInfo`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DecoderInfo {
    pub name: *const c_char,
    pub flags: c_uint,
}

/// `libraw_progress_handler` (C# `ProgressCallback`, cdecl).
pub type ProgressCallback =
    unsafe extern "C" fn(data: *mut c_void, stage: c_int, iteration: c_int, expected: c_int) -> c_int;

/// The resolved C API. Field names are the export names; one field per [`ENTRY_POINTS`] entry. `*mut c_void` in first
/// position is `libraw_data_t *`.
pub struct LibRawApi {
    library: LibraryHandle,
    // BEGIN ENTRY POINTS
    libraw_init: unsafe extern "C" fn(flags: c_uint) -> *mut c_void,
    libraw_close: unsafe extern "C" fn(data: *mut c_void),
    libraw_open_wfile: unsafe extern "C" fn(data: *mut c_void, path: *const u16) -> c_int,
    libraw_open_buffer: unsafe extern "C" fn(data: *mut c_void, buffer: *const c_void, size: usize) -> c_int,
    libraw_unpack: unsafe extern "C" fn(data: *mut c_void) -> c_int,
    libraw_unpack_thumb: unsafe extern "C" fn(data: *mut c_void) -> c_int,
    libraw_dcraw_process: unsafe extern "C" fn(data: *mut c_void) -> c_int,
    libraw_dcraw_make_mem_image:
        unsafe extern "C" fn(data: *mut c_void, error: *mut c_int) -> *mut ProcessedImageHeader,
    libraw_dcraw_make_mem_thumb:
        unsafe extern "C" fn(data: *mut c_void, error: *mut c_int) -> *mut ProcessedImageHeader,
    libraw_dcraw_clear_mem: unsafe extern "C" fn(image: *mut ProcessedImageHeader),
    libraw_adjust_sizes_info_only: unsafe extern "C" fn(data: *mut c_void) -> c_int,
    libraw_get_iwidth: unsafe extern "C" fn(data: *mut c_void) -> c_int,
    libraw_get_iheight: unsafe extern "C" fn(data: *mut c_void) -> c_int,
    libraw_get_raw_width: unsafe extern "C" fn(data: *mut c_void) -> c_int,
    libraw_get_raw_height: unsafe extern "C" fn(data: *mut c_void) -> c_int,
    libraw_get_decoder_info: unsafe extern "C" fn(data: *mut c_void, info: *mut DecoderInfo) -> c_int,
    libraw_get_iparams: unsafe extern "C" fn(data: *mut c_void) -> *mut c_void,
    libraw_set_progress_handler:
        unsafe extern "C" fn(data: *mut c_void, callback: Option<ProgressCallback>, user: *mut c_void),
    libraw_set_output_color: unsafe extern "C" fn(data: *mut c_void, value: c_int),
    libraw_set_output_bps: unsafe extern "C" fn(data: *mut c_void, value: c_int),
    libraw_set_no_auto_bright: unsafe extern "C" fn(data: *mut c_void, value: c_int),
    libraw_version: unsafe extern "C" fn() -> *const c_char,
    libraw_strerror: unsafe extern "C" fn(code: c_int) -> *const c_char,
    // END ENTRY POINTS
}

impl LibRawApi {
    /// Resolves every [`ENTRY_POINTS`] export of `library` (first missing one => `MissingExport`), taking ownership of it.
    pub fn load(library: LibraryHandle) -> Result<Self, LoadError> {
        todo!()
    }
}

impl fmt::Debug for LibRawApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibRawApi")
            .field("library", &self.library)
            .finish_non_exhaustive()
    }
}

/// The process-wide C API: on first call `LibraryHandle::load_next_to_exe(LIBRARY_NAME)` + [`LibRawApi::load`], cached
/// in a `static OnceLock` for the process lifetime (success AND failure; the library is never freed, like P/Invoke).
/// Independent of the W5-15b probe's own load/free, exactly as C# P/Invoke resolves independently of `NativeLibrary`.
pub fn api() -> Result<&'static LibRawApi, LoadError> {
    todo!()
}

static API: OnceLock<Result<LibRawApi, LoadError>> = OnceLock::new();

// ------------------------------------------------------------------------------------------------ safe wrappers

/// Why [`LibRawHandle::init`] gave no handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeError {
    /// [`api`] failed (DLL missing, wrong, or non-Windows build).
    Unavailable(LoadError),
    /// `libraw_init` returned null (W5-16 maps it to `"LibRaw could not initialize its decoder."`).
    InitReturnedNull,
}

impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl Error for NativeError {}

/// An open `libraw_data_t` (C# `SafeLibRawHandle`): `libraw_close` on drop or [`close`](Self::close).
///
/// `'buf` is the lifetime of the buffer given to [`open_buffer`](Self::open_buffer): LibRaw keeps a pointer into it
/// (no copy) that `unpack`/`dcraw_process` read later, so the borrow checker enforces what C# does by declaring the pin
/// before the handle. `Send` (one thread at a time), not `Sync` (LibRaw is not thread-safe per handle).
///
/// Field order is drop order: `owner` (libraw_close) runs before `cancel` and `shared_buffer` are freed, so LibRaw never
/// holds a dangling progress-callback or buffer pointer.
pub struct LibRawHandle<'buf> {
    owner: NativeOwner,
    api: &'static LibRawApi,
    cancel: Option<Box<CancelToken>>,
    /// The buffer of [`open_shared_buffer`](Self::open_shared_buffer), kept alive until after `libraw_close`.
    shared_buffer: Option<Arc<[u8]>>,
    _buffer: PhantomData<&'buf [u8]>,
}

impl<'buf> LibRawHandle<'buf> {
    /// `libraw_init(flags)` through [`api`]. C# `new SafeLibRawHandle(LibRawInit(0))` (+ W5-16's null check).
    pub fn init(flags: u32) -> Result<Self, NativeError> {
        todo!()
    }

    /// The `libraw_data_t` address (C# `DangerousGetHandle`), for diagnostics only.
    pub fn raw(&self) -> usize {
        todo!()
    }

    /// The `i32` at byte `offset` of `libraw_data_t`, `None` unless `0 <= offset && offset + 4 <= DATA_PROBE_LIMIT`
    /// (native tests' `ReadInt32`; same window as the [`WhiteBalanceTarget`] impl).
    pub fn read_data_i32(&self, offset: i32) -> Option<i32> {
        todo!()
    }

    /// `libraw_open_wfile`: `path` encoded as NUL-terminated UTF-16 (`encode_wide` on Windows; an interior NUL ends the
    /// path for LibRaw, as with the C# string marshaller). Returns LibRaw's error code (0 = success).
    pub fn open_wfile(&mut self, path: &OsStr) -> i32 {
        todo!()
    }

    /// `libraw_open_buffer(buffer.as_ptr(), buffer.len())`. The buffer stays borrowed for `'buf`.
    pub fn open_buffer(&mut self, buffer: &'buf [u8]) -> i32 {
        todo!()
    }

    /// `libraw_open_buffer` over a shared buffer the handle keeps alive itself (for W5-16c's `'static` session, which
    /// receives a `SourceBytes = Arc<[u8]>`; C# pins the array before creating the handle). The `Arc` is stored after the
    /// call (an earlier one is dropped then: LibRaw's open recycles the previous datastream first) and freed only after
    /// `libraw_close` (field order).
    pub fn open_shared_buffer(&mut self, buffer: Arc<[u8]>) -> i32 {
        todo!()
    }

    pub fn unpack(&mut self) -> i32 {
        todo!()
    }

    pub fn unpack_thumb(&mut self) -> i32 {
        todo!()
    }

    pub fn dcraw_process(&mut self) -> i32 {
        todo!()
    }

    pub fn adjust_sizes_info_only(&mut self) -> i32 {
        todo!()
    }

    /// `libraw_dcraw_make_mem_image`: `Err(error_code)` when it returned null (the code may be 0), else the image whose
    /// header is copied at creation. The image does not borrow the handle (it is `malloc`ed and freed with
    /// `libraw_dcraw_clear_mem`), so W5-16 can close the handle first.
    pub fn dcraw_make_mem_image(&mut self) -> Result<LibRawImage, i32> {
        todo!()
    }

    /// `libraw_dcraw_make_mem_thumb`, same contract as [`dcraw_make_mem_image`](Self::dcraw_make_mem_image).
    pub fn dcraw_make_mem_thumb(&mut self) -> Result<LibRawImage, i32> {
        todo!()
    }

    pub fn iwidth(&self) -> i32 {
        todo!()
    }

    pub fn iheight(&self) -> i32 {
        todo!()
    }

    pub fn raw_width(&self) -> i32 {
        todo!()
    }

    pub fn raw_height(&self) -> i32 {
        todo!()
    }

    /// C# `TryGetDecoderName`: the decoder name LibRaw picked while opening (e.g. `"lossless_dng_load_raw()"`), `None`
    /// when `libraw_get_decoder_info` is non-zero or the name pointer is null.
    pub fn decoder_name(&self) -> Option<String> {
        todo!()
    }

    /// C# `TryGetRawStructure` (production): [`raw_structure_with`](Self::raw_structure_with) gated by
    /// `crate::availability::is_exact_pinned_version` (W5-15b).
    pub fn raw_structure(&self) -> Option<RawStructure> {
        todo!()
    }

    /// `version_pinned()` false => `None` without calling `libraw_get_iparams`; null iparams => `None`; else
    /// [`try_read_raw_structure`] over the first [`IPARAMS_READ_LEN`] bytes of the block.
    pub fn raw_structure_with(&self, version_pinned: impl FnOnce() -> bool) -> Option<RawStructure> {
        todo!()
    }

    /// Installs [`cancellation_progress_callback`] with this token (C# `LibRawSetProgressHandler(raw, CancellationCallback,
    /// state.Pointer)`). The token is boxed inside the handle so its address outlives every LibRaw call; a second call
    /// installs the new box first and only then frees the old one.
    pub fn set_cancel_token(&mut self, token: CancelToken) {
        todo!()
    }

    pub fn set_output_color(&mut self, value: i32) {
        todo!()
    }

    pub fn set_output_bps(&mut self, value: i32) {
        todo!()
    }

    pub fn set_no_auto_bright(&mut self, value: i32) {
        todo!()
    }

    /// C# production `TrySetUseCameraWb(handle, outputColor, outputBps, noAutoBright)`: [`try_set_use_camera_wb_with`]
    /// with `crate::availability::is_exact_pinned_version` (W5-15b) and [`PINNED_WHITE_BALANCE_LAYOUT`].
    ///
    /// [`try_set_use_camera_wb_with`]: Self::try_set_use_camera_wb_with
    pub fn try_set_use_camera_wb(&mut self, output_color: i32, output_bps: i32, no_auto_bright: i32) -> bool {
        todo!()
    }

    /// C# `TrySetUseCameraWb(SafeLibRawHandle, int, int, int, Func<bool>, WhiteBalanceLayout)`:
    /// [`try_set_use_camera_wb_core`] over this handle.
    pub fn try_set_use_camera_wb_with(
        &mut self,
        output_color: i32,
        output_bps: i32,
        no_auto_bright: i32,
        version_pinned: impl FnOnce() -> bool,
        layout: WhiteBalanceLayout,
    ) -> bool {
        todo!()
    }

    /// Closes now (idempotent); later calls of the other methods are a bug and must not reach LibRaw: they return the
    /// neutral value (`-1` error codes: LIBRAW_UNSPECIFIED_ERROR, `0` sizes, `None`, no-op setters). C# `Dispose`.
    pub fn close(&mut self) {
        todo!()
    }

    pub fn is_closed(&self) -> bool {
        todo!()
    }
}

impl WhiteBalanceTarget for LibRawHandle<'_> {
    fn set_output_color(&mut self, value: i32) {
        todo!()
    }

    fn set_output_bps(&mut self, value: i32) {
        todo!()
    }

    fn set_no_auto_bright(&mut self, value: i32) {
        todo!()
    }

    fn read_i32(&self, offset: i32) -> Option<i32> {
        todo!()
    }

    fn write_i32(&mut self, offset: i32, value: i32) -> bool {
        todo!()
    }
}

impl fmt::Debug for LibRawHandle<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibRawHandle")
            .field("owner", &self.owner)
            .field("cancel", &self.cancel.is_some())
            .finish()
    }
}

/// A `libraw_processed_image_t` from `dcraw_make_mem_image` / `_thumb` (C# `SafeLibRawImageHandle`):
/// `libraw_dcraw_clear_mem` on drop or [`close`](Self::close). `Send`.
pub struct LibRawImage {
    owner: NativeOwner,
    header: ProcessedImageHeader,
}

impl LibRawImage {
    /// The header copied when the image was created (C# `Marshal.PtrToStructure<ProcessedImageHeader>`).
    pub fn header(&self) -> ProcessedImageHeader {
        todo!()
    }

    /// The `data_size` bytes at [`PROCESSED_IMAGE_DATA_OFFSET`]; `None` after [`close`](Self::close).
    pub fn data(&self) -> Option<&[u8]> {
        todo!()
    }

    pub fn close(&mut self) {
        todo!()
    }

    /// C# `image.IsClosed` (W5-16's "source-released" stage).
    pub fn is_closed(&self) -> bool {
        todo!()
    }
}

impl fmt::Debug for LibRawImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibRawImage")
            .field("owner", &self.owner)
            .field("header", &self.header)
            .finish()
    }
}
