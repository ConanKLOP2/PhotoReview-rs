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
//!
//! DIVERGENCE (unreachable, documented): [`api`] caches a failed load for the process lifetime, while C# P/Invoke re-attempts
//! a failed load on every call; harmless because the W5-15b probe caches its answer too and nothing calls LibRaw when the
//! probe failed. After [`LibRawHandle::close`] every method returns a neutral value instead of throwing
//! `ObjectDisposedException` (W5-16 never calls after close).

use std::error::Error;
use std::ffi::{CStr, OsStr, c_char, c_int, c_uint, c_void};
use std::fmt;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
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
    if block.len() < IPARAMS_READ_LEN {
        return None;
    }
    let colors = read_ne_i32(block, IPARAMS_COLORS_OFFSET)?;
    if !(1..=4).contains(&colors) {
        return None;
    }
    let count = usize::try_from(colors).ok()?;
    let cdesc = block.get(IPARAMS_CDESC_OFFSET..IPARAMS_CDESC_OFFSET.checked_add(count)?)?;
    if !cdesc.iter().all(u8::is_ascii_uppercase) {
        return None;
    }
    // C# `unchecked((uint)...)`: negative filters are valid bit patterns.
    let filters = read_ne_i32(block, IPARAMS_FILTERS_OFFSET)? as u32;
    Some(RawStructure::new(colors, filters))
}

/// The native-endian `i32` at `offset` of `block`, `None` when it does not fit.
fn read_ne_i32(block: &[u8], offset: usize) -> Option<i32> {
    let bytes = block.get(offset..offset.checked_add(4)?)?;
    Some(i32::from_ne_bytes(bytes.try_into().ok()?))
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
    if !version_pinned() {
        return false;
    }
    target.set_output_color(SENTINEL_OUTPUT_COLOR);
    target.set_output_bps(SENTINEL_OUTPUT_BPS);
    target.set_no_auto_bright(SENTINEL_NO_AUTO_BRIGHT);
    let layout_matches = target.read_i32(layout.output_color) == Some(SENTINEL_OUTPUT_COLOR)
        && target.read_i32(layout.output_bps) == Some(SENTINEL_OUTPUT_BPS)
        && target.read_i32(layout.no_auto_bright) == Some(SENTINEL_NO_AUTO_BRIGHT);
    target.set_output_color(output_color);
    target.set_output_bps(output_bps);
    target.set_no_auto_bright(no_auto_bright);
    if !layout_matches {
        return false;
    }
    if !target.write_i32(layout.use_camera_wb, 1) {
        return false;
    }
    target.read_i32(layout.use_camera_wb) == Some(1)
}

/// C# `FormatError` text rule: the `libraw_strerror` message, or `"LibRaw error {code}"` when it is `None`, empty or only
/// whitespace (C# `string.IsNullOrWhiteSpace`). The message is returned unchanged (no trim).
pub fn format_error_message(code: i32, message: Option<&str>) -> String {
    match message {
        Some(text) if !text.trim().is_empty() => text.to_owned(),
        _ => format!("LibRaw error {code}"),
    }
}

/// C# `FormatError(int)`: [`format_error_message`] over `libraw_strerror(code)`. When the library is unavailable the
/// fallback text is returned (C# would throw `DllNotFoundException`; unreachable there because a decode never starts
/// without a successful probe).
pub fn format_error(code: i32) -> String {
    let message = match api() {
        Ok(api) => {
            // SAFETY: `libraw_strerror` is LibRaw's `const char *libraw_strerror(int)`; it returns null or a static C string.
            let text = unsafe { (api.libraw_strerror)(code) };
            // SAFETY: `text` is null or a static NUL-terminated string (see above).
            unsafe { c_string(text) }
        }
        Err(_) => None,
    };
    format_error_message(code, message.as_deref())
}

/// The loaded runtime's version string (for example `"0.22.2-Release"`), `None` when the library is unavailable or
/// `libraw_version` returns null. C# `GetVersionString`. ANSI text decoded lossily (LibRaw strings are ASCII).
pub fn version_string() -> Option<String> {
    let api = api().ok()?;
    // SAFETY: `libraw_version` is LibRaw's `const char *libraw_version(void)` (a static string).
    let text = unsafe { (api.libraw_version)() };
    // SAFETY: `text` is null or a static NUL-terminated string.
    unsafe { c_string(text) }
}

/// `Marshal.PtrToStringAnsi`: `None` for null, else the lossily decoded text (LibRaw strings are ASCII).
///
/// # Safety
/// `text` must be null or point to a NUL-terminated string that stays valid during the call.
unsafe fn c_string(text: *const c_char) -> Option<String> {
    if text.is_null() {
        return None;
    }
    // SAFETY: non-null and NUL-terminated by the caller's contract.
    let text = unsafe { CStr::from_ptr(text) };
    Some(text.to_string_lossy().into_owned())
}

/// The progress-callback verdict (C# `LibRawDecoder.CheckCancellation` minus the `GCHandle` lookup): `1` (abort) when
/// the token is cancelled or there is no token (C# null/bad pointer => `1`), else `0`.
pub fn progress_verdict(token: Option<&CancelToken>) -> i32 {
    match token {
        Some(token) if !token.is_canceled() => 0,
        _ => 1,
    }
}

/// The `libraw_progress_handler` installed by [`LibRawHandle::set_cancel_token`]. `data` is the address of the handle's
/// boxed [`CancelToken`] (or null). Must never unwind into C: it only reads an atomic.
unsafe extern "C" fn cancellation_progress_callback(
    data: *mut c_void,
    _stage: c_int,
    _iteration: c_int,
    _expected: c_int,
) -> c_int {
    if data.is_null() {
        return 1;
    }
    // SAFETY: the only non-null `data` LibRaw passes is the address `set_cancel_token` installed: the handle's own
    // `Box<CancelToken>`, alive until after `libraw_close` (`owner` is declared, so dropped, before `cancel`).
    let token = unsafe { &*data.cast::<CancelToken>() };
    progress_verdict(Some(token))
}

/// `exe.parent().join(name)`; `None` when `exe` has no parent folder: `parent()` is `None` OR the empty path (a bare
/// `"PhotoReviewRs.exe"` has `parent() == Some("")`). Never falls back to a bare name, which would let `LoadLibraryExW`
/// search PATH / the working directory.
pub fn library_path_next_to(exe: &Path, name: &str) -> Option<PathBuf> {
    let folder = exe.parent()?;
    if folder.as_os_str().is_empty() {
        return None;
    }
    Some(folder.join(name))
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
        Self {
            raw,
            release: Some(Box::new(release)),
            closed: false,
        }
    }

    pub fn raw(&self) -> usize {
        self.raw
    }

    pub fn is_invalid(&self) -> bool {
        self.raw == 0 || self.raw == usize::MAX
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn close(&mut self) {
        self.closed = true;
        if self.is_invalid() {
            return;
        }
        if let Some(release) = self.release.take() {
            release(self.raw);
        }
    }
}

impl Drop for NativeOwner {
    fn drop(&mut self) {
        self.close();
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
        match self {
            Self::NoExecutableDirectory => f.write_str("the folder of the running executable is unknown"),
            Self::LoadFailed { path, os_error } => {
                write!(f, "could not load {} (OS error {os_error})", path.display())
            }
            Self::MissingExport { name } => write!(f, "the library does not export {name}"),
            Self::Unsupported => f.write_str("libraw.dll can only be loaded on Windows"),
        }
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
        let exe = std::env::current_exe().map_err(|_| LoadError::NoExecutableDirectory)?;
        let path = library_path_next_to(&exe, name).ok_or(LoadError::NoExecutableDirectory)?;
        Self::load_from(&path)
    }

    /// Loads an absolute `path` with [`LOAD_WITH_ALTERED_SEARCH_PATH`]. A relative path (`!path.is_absolute()`, which on
    /// Windows includes `C:foo` and `\foo`) is refused (`LoadFailed` with `os_error` 87 = `ERROR_INVALID_PARAMETER`)
    /// without calling the loader. The relative check runs BEFORE the `cfg(not(windows))` => `Unsupported` branch, so
    /// that refusal is the same on every platform.
    pub fn load_from(path: &Path) -> Result<Self, LoadError> {
        if !path.is_absolute() {
            return Err(LoadError::LoadFailed {
                path: path.to_path_buf(),
                os_error: ERROR_INVALID_PARAMETER,
            });
        }
        Self::load_with_flags(path.as_os_str(), LOAD_WITH_ALTERED_SEARCH_PATH)
    }

    /// Loads a system DLL by name from System32 only ([`LOAD_LIBRARY_SEARCH_SYSTEM32`]); for the W5-15b
    /// "library without LibRaw exports" test (`kernel32.dll`).
    pub fn load_system(name: &str) -> Result<Self, LoadError> {
        Self::load_with_flags(OsStr::new(name), LOAD_LIBRARY_SEARCH_SYSTEM32)
    }

    #[cfg(windows)]
    fn load_with_flags(file: &OsStr, flags: u32) -> Result<Self, LoadError> {
        let wide = wide_nul(file);
        // SAFETY: `wide` is a NUL-terminated UTF-16 string alive for the call; the reserved `hFile` argument is null.
        let module = unsafe { LoadLibraryExW(wide.as_ptr(), ptr::null_mut(), flags) };
        match NonNull::new(module) {
            Some(module) => Ok(Self { module }),
            None => Err(LoadError::LoadFailed {
                path: PathBuf::from(file),
                // SAFETY: plain thread-local read of the error the failed `LoadLibraryExW` just set.
                os_error: unsafe { GetLastError() },
            }),
        }
    }

    #[cfg(not(windows))]
    fn load_with_flags(_file: &OsStr, _flags: u32) -> Result<Self, LoadError> {
        Err(LoadError::Unsupported)
    }

    /// `GetProcAddress(name) != null`; `false` for a name containing NUL. C# `NativeLibrary.TryGetExport`.
    pub fn has_export(&self, name: &str) -> bool {
        self.export(name).is_some()
    }

    /// The names of `names` this library does not export, in the order given.
    pub fn missing_exports<'a>(&self, names: &[&'a str]) -> Vec<&'a str> {
        names.iter().copied().filter(|name| !self.has_export(name)).collect()
    }

    /// Calls the library's `libraw_version()`: `None` when not exported, `Some(None)` when it returned null, else the
    /// lossily decoded text.
    ///
    /// # Safety
    /// The library must be a LibRaw build: its `libraw_version` export must have the C signature
    /// `const char *libraw_version(void)` and return a static string.
    pub unsafe fn call_version(&self) -> Option<Option<String>> {
        let export = self.export("libraw_version")?;
        // SAFETY: the caller guarantees the export is `const char *libraw_version(void)`; a fn pointer has the size of a
        // data pointer on every Windows target.
        let version =
            unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> *const c_char>(export.as_ptr()) };
        // SAFETY: calling LibRaw's prototype (caller contract); it returns null or a static string.
        let text = unsafe { version() };
        // SAFETY: `text` is null or a static NUL-terminated string.
        Some(unsafe { c_string(text) })
    }

    /// Calls `libraw_init(0)` and, when it returned a non-null handle, `libraw_close` on it. `None` when either export
    /// is missing, `Some(false)` when init returned null, `Some(true)` after a successful init/close pair.
    ///
    /// # Safety
    /// Same as [`call_version`](Self::call_version): `libraw_init` / `libraw_close` must have LibRaw's C signatures.
    pub unsafe fn call_init_close(&self) -> Option<bool> {
        let init = self.export("libraw_init")?;
        let close = self.export("libraw_close")?;
        // SAFETY: the caller guarantees `libraw_data_t *libraw_init(unsigned int flags)`.
        let init =
            unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn(c_uint) -> *mut c_void>(init.as_ptr()) };
        // SAFETY: the caller guarantees `void libraw_close(libraw_data_t *)`.
        let close = unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(close.as_ptr()) };
        // SAFETY: LibRaw's prototype; flags 0 as in C#.
        let data = unsafe { init(0) };
        if data.is_null() {
            return Some(false);
        }
        // SAFETY: `data` is the non-null handle `libraw_init` just returned, closed exactly once here.
        unsafe { close(data) };
        Some(true)
    }

    #[cfg(windows)]
    fn export(&self, name: &str) -> Option<NonNull<c_void>> {
        let name = std::ffi::CString::new(name).ok()?;
        // SAFETY: `module` is a live HMODULE owned by `self`; `name` is a NUL-terminated ANSI string alive for the call.
        NonNull::new(unsafe { GetProcAddress(self.module.as_ptr(), name.as_ptr()) })
    }

    #[cfg(not(windows))]
    fn export(&self, _name: &str) -> Option<NonNull<c_void>> {
        None
    }
}

/// `ERROR_INVALID_PARAMETER`: the `os_error` of a refused relative library path.
const ERROR_INVALID_PARAMETER: u32 = 87;

/// NUL-terminated UTF-16 for the `W` APIs (WTF-16 on Windows: unpaired surrogates pass through like a C# string).
#[cfg(windows)]
fn wide_nul(text: &OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    text.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(not(windows))]
fn wide_nul(text: &OsStr) -> Vec<u16> {
    text.to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

impl Drop for LibraryHandle {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            // SAFETY: `module` came from `LoadLibraryExW` and is freed exactly once, here (the result is ignored like C#).
            unsafe {
                FreeLibrary(self.module.as_ptr());
            }
        }
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
        macro_rules! resolve_all {
            ($library:ident; $($name:ident),* $(,)?) => {
                LibRawApi {
                    $($name: resolve_export(&$library, stringify!($name))?,)*
                    library: $library,
                }
            };
        }
        // SAFETY: each field's fn-pointer type is the LibRaw 0.22 prototype of that export from `libraw.h` (cdecl = "C" on
        // x64; `libraw_data_t *` as `*mut c_void`, `libraw_processed_image_t *` as `*mut ProcessedImageHeader`), and the
        // library is a LibRaw build (`api` loads only `libraw.dll` from the app folder; W5-15b rejects anything else first).
        let api = unsafe {
            resolve_all!(
                library;
                libraw_init,
                libraw_close,
                libraw_open_wfile,
                libraw_open_buffer,
                libraw_unpack,
                libraw_unpack_thumb,
                libraw_dcraw_process,
                libraw_dcraw_make_mem_image,
                libraw_dcraw_make_mem_thumb,
                libraw_dcraw_clear_mem,
                libraw_adjust_sizes_info_only,
                libraw_get_iwidth,
                libraw_get_iheight,
                libraw_get_raw_width,
                libraw_get_raw_height,
                libraw_get_decoder_info,
                libraw_get_iparams,
                libraw_set_progress_handler,
                libraw_set_output_color,
                libraw_set_output_bps,
                libraw_set_no_auto_bright,
                libraw_version,
                libraw_strerror,
            )
        };
        Ok(api)
    }
}

/// Resolves `name` and reinterprets the address as the fn-pointer type `F`; `MissingExport` when it is absent.
///
/// # Safety
/// `F` must be an `extern "C"` fn-pointer type matching the export's C prototype.
unsafe fn resolve_export<F: Copy>(library: &LibraryHandle, name: &'static str) -> Result<F, LoadError> {
    const { assert!(std::mem::size_of::<F>() == std::mem::size_of::<*mut c_void>()) };
    let address = library.export(name).ok_or(LoadError::MissingExport { name })?.as_ptr();
    // SAFETY: `F` is pointer-sized (checked at compile time) and, by the caller's contract, the export's prototype; the
    // address is non-null.
    Ok(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&address) })
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
    API.get_or_init(|| LibraryHandle::load_next_to_exe(LIBRARY_NAME).and_then(LibRawApi::load))
        .as_ref()
        .map_err(Clone::clone)
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
        match self {
            Self::Unavailable(error) => write!(f, "LibRaw is unavailable: {error}"),
            Self::InitReturnedNull => f.write_str("libraw_init returned a null decoder handle"),
        }
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
        let api = api().map_err(NativeError::Unavailable)?;
        // SAFETY: `libraw_data_t *libraw_init(unsigned int)` (resolved by `LibRawApi::load`).
        let data = unsafe { (api.libraw_init)(flags) };
        if data.is_null() {
            return Err(NativeError::InitReturnedNull);
        }
        let owner = NativeOwner::new(data.expose_provenance(), move |raw| {
            // SAFETY: `raw` is the non-null `libraw_init` result exposed above; `NativeOwner` releases it at most once.
            unsafe { (api.libraw_close)(ptr::with_exposed_provenance_mut(raw)) }
        });
        Ok(Self {
            owner,
            api,
            cancel: None,
            shared_buffer: None,
            _buffer: PhantomData,
        })
    }

    /// The `libraw_data_t` address (C# `DangerousGetHandle`), for diagnostics only.
    pub fn raw(&self) -> usize {
        self.owner.raw()
    }

    /// The live `libraw_data_t *`, `None` after close (no LibRaw call may follow then).
    fn data(&self) -> Option<*mut c_void> {
        (!self.owner.is_closed()).then(|| ptr::with_exposed_provenance_mut(self.owner.raw()))
    }

    /// The `[offset, offset + 4)` window check of the raw `i32` access: `0 <= offset` and `offset + 4 <= DATA_PROBE_LIMIT`.
    fn probe_window(offset: i32) -> Option<usize> {
        let end = offset.checked_add(4)?;
        if offset < 0 || end > DATA_PROBE_LIMIT {
            return None;
        }
        usize::try_from(offset).ok()
    }

    /// Calls an `int f(libraw_data_t *)` export; `-1` after close.
    fn call_int(&self, function: unsafe extern "C" fn(*mut c_void) -> c_int) -> i32 {
        let Some(data) = self.data() else {
            return -1;
        };
        // SAFETY: `function` is a resolved LibRaw export taking the handle; `data` is the live, open handle.
        unsafe { function(data) }
    }

    /// Calls a `void f(libraw_data_t *, int)` setter; no-op after close.
    fn call_setter(&mut self, function: unsafe extern "C" fn(*mut c_void, c_int), value: i32) {
        if let Some(data) = self.data() {
            // SAFETY: `function` is a resolved LibRaw setter; `data` is the live, open handle.
            unsafe { function(data, value) }
        }
    }

    /// Calls a `libraw_dcraw_make_mem_*` export; `Err(-1)` after close.
    fn make_mem(
        &mut self,
        function: unsafe extern "C" fn(*mut c_void, *mut c_int) -> *mut ProcessedImageHeader,
    ) -> Result<LibRawImage, i32> {
        let Some(data) = self.data() else {
            return Err(-1);
        };
        let mut error: c_int = 0;
        // SAFETY: `function` is a resolved `libraw_processed_image_t *f(libraw_data_t *, int *)`; `data` is live and
        // `error` a valid out pointer for the call.
        let image = unsafe { function(data, &mut error) };
        let Some(image) = NonNull::new(image) else {
            return Err(error);
        };
        // SAFETY: LibRaw returns a `malloc`ed `libraw_processed_image_t` that starts with the 16-byte header.
        let header = unsafe { ptr::read_unaligned(image.as_ptr()) };
        let api = self.api;
        let owner = NativeOwner::new(image.as_ptr().expose_provenance(), move |raw| {
            // SAFETY: `raw` is the non-null image exposed above; `NativeOwner` frees it at most once.
            unsafe { (api.libraw_dcraw_clear_mem)(ptr::with_exposed_provenance_mut(raw)) }
        });
        Ok(LibRawImage { owner, header })
    }

    /// The `i32` at byte `offset` of `libraw_data_t`, `None` unless `0 <= offset && offset + 4 <= DATA_PROBE_LIMIT`
    /// (native tests' `ReadInt32`; same window as the [`WhiteBalanceTarget`] impl).
    pub fn read_data_i32(&self, offset: i32) -> Option<i32> {
        WhiteBalanceTarget::read_i32(self, offset)
    }

    /// `libraw_open_wfile`: `path` encoded as NUL-terminated UTF-16 (`encode_wide` on Windows; an interior NUL ends the
    /// path for LibRaw, as with the C# string marshaller). Returns LibRaw's error code (0 = success).
    pub fn open_wfile(&mut self, path: &OsStr) -> i32 {
        let Some(data) = self.data() else {
            return -1;
        };
        let wide = wide_nul(path);
        // SAFETY: `data` is the live handle; `wide` is a NUL-terminated UTF-16 path alive for the call (LibRaw copies it).
        unsafe { (self.api.libraw_open_wfile)(data, wide.as_ptr()) }
    }

    /// `libraw_open_buffer(buffer.as_ptr(), buffer.len())`. The buffer stays borrowed for `'buf`.
    pub fn open_buffer(&mut self, buffer: &'buf [u8]) -> i32 {
        self.open_raw_buffer(buffer)
    }

    fn open_raw_buffer(&mut self, buffer: &[u8]) -> i32 {
        let Some(data) = self.data() else {
            return -1;
        };
        // SAFETY: `data` is the live handle; the buffer outlives every later LibRaw call on it (`'buf` borrow or the
        // `shared_buffer` field, both outliving `libraw_close`).
        unsafe { (self.api.libraw_open_buffer)(data, buffer.as_ptr().cast(), buffer.len()) }
    }

    /// `libraw_open_buffer` over a shared buffer the handle keeps alive itself (for W5-16c's `'static` session, which
    /// receives a `SourceBytes = Arc<[u8]>`; C# pins the array before creating the handle). The `Arc` is stored after the
    /// call (an earlier one is dropped then: LibRaw's open recycles the previous datastream first) and freed only after
    /// `libraw_close` (field order).
    pub fn open_shared_buffer(&mut self, buffer: Arc<[u8]>) -> i32 {
        if self.owner.is_closed() {
            return -1;
        }
        let code = self.open_raw_buffer(&buffer);
        self.shared_buffer = Some(buffer);
        code
    }

    pub fn unpack(&mut self) -> i32 {
        self.call_int(self.api.libraw_unpack)
    }

    pub fn unpack_thumb(&mut self) -> i32 {
        self.call_int(self.api.libraw_unpack_thumb)
    }

    pub fn dcraw_process(&mut self) -> i32 {
        self.call_int(self.api.libraw_dcraw_process)
    }

    pub fn adjust_sizes_info_only(&mut self) -> i32 {
        self.call_int(self.api.libraw_adjust_sizes_info_only)
    }

    /// `libraw_dcraw_make_mem_image`: `Err(error_code)` when it returned null (the code may be 0), else the image whose
    /// header is copied at creation. The image does not borrow the handle (it is `malloc`ed and freed with
    /// `libraw_dcraw_clear_mem`), so W5-16 can close the handle first.
    pub fn dcraw_make_mem_image(&mut self) -> Result<LibRawImage, i32> {
        self.make_mem(self.api.libraw_dcraw_make_mem_image)
    }

    /// `libraw_dcraw_make_mem_thumb`, same contract as [`dcraw_make_mem_image`](Self::dcraw_make_mem_image).
    pub fn dcraw_make_mem_thumb(&mut self) -> Result<LibRawImage, i32> {
        self.make_mem(self.api.libraw_dcraw_make_mem_thumb)
    }

    pub fn iwidth(&self) -> i32 {
        self.size(self.api.libraw_get_iwidth)
    }

    pub fn iheight(&self) -> i32 {
        self.size(self.api.libraw_get_iheight)
    }

    pub fn raw_width(&self) -> i32 {
        self.size(self.api.libraw_get_raw_width)
    }

    pub fn raw_height(&self) -> i32 {
        self.size(self.api.libraw_get_raw_height)
    }

    /// A size getter; `0` after close.
    fn size(&self, function: unsafe extern "C" fn(*mut c_void) -> c_int) -> i32 {
        if self.owner.is_closed() {
            0
        } else {
            self.call_int(function)
        }
    }

    /// C# `TryGetDecoderName`: the decoder name LibRaw picked while opening (e.g. `"lossless_dng_load_raw()"`), `None`
    /// when `libraw_get_decoder_info` is non-zero or the name pointer is null.
    pub fn decoder_name(&self) -> Option<String> {
        let data = self.data()?;
        let mut info = DecoderInfo {
            name: ptr::null(),
            flags: 0,
        };
        // SAFETY: `data` is the live handle; `info` is a valid `libraw_decoder_info_t` out pointer for the call.
        if unsafe { (self.api.libraw_get_decoder_info)(data, &mut info) } != 0 {
            return None;
        }
        // SAFETY: LibRaw sets `name` to null or a static decoder-name C string.
        unsafe { c_string(info.name) }
    }

    /// C# `TryGetRawStructure` (production): [`raw_structure_with`](Self::raw_structure_with) gated by
    /// `crate::availability::is_exact_pinned_version` (W5-15b).
    pub fn raw_structure(&self) -> Option<RawStructure> {
        self.raw_structure_with(crate::availability::is_exact_pinned_version)
    }

    /// `version_pinned()` false => `None` without calling `libraw_get_iparams`; null iparams => `None`; else
    /// [`try_read_raw_structure`] over the first [`IPARAMS_READ_LEN`] bytes of the block.
    pub fn raw_structure_with(&self, version_pinned: impl FnOnce() -> bool) -> Option<RawStructure> {
        if !version_pinned() {
            return None;
        }
        let data = self.data()?;
        // SAFETY: `data` is the live handle; `libraw_iparams_t *libraw_get_iparams(libraw_data_t *)`.
        let iparams = unsafe { (self.api.libraw_get_iparams)(data) };
        if iparams.is_null() {
            return None;
        }
        // SAFETY: `libraw_get_iparams` returns `&imgdata.idata`, a `libraw_iparams_t` of more than IPARAMS_READ_LEN bytes
        // inside the live `libraw_data_t`; the slice does not outlive `&self`.
        let block = unsafe { std::slice::from_raw_parts(iparams.cast::<u8>().cast_const(), IPARAMS_READ_LEN) };
        try_read_raw_structure(block)
    }

    /// Installs [`cancellation_progress_callback`] with this token (C# `LibRawSetProgressHandler(raw, CancellationCallback,
    /// state.Pointer)`). The token is boxed inside the handle so its address outlives every LibRaw call; a second call
    /// installs the new box first and only then frees the old one.
    pub fn set_cancel_token(&mut self, token: CancelToken) {
        let Some(data) = self.data() else {
            return;
        };
        let boxed = Box::new(token);
        let user = ptr::from_ref::<CancelToken>(&boxed).cast_mut().cast::<c_void>();
        // SAFETY: `data` is the live handle; the callback matches `libraw_progress_handler` and never unwinds; `user` is
        // the boxed token, stored below and freed only after `libraw_close` (field order).
        unsafe { (self.api.libraw_set_progress_handler)(data, Some(cancellation_progress_callback), user) };
        // The old box (if any) is freed only now, after LibRaw holds the new pointer.
        self.cancel = Some(boxed);
    }

    pub fn set_output_color(&mut self, value: i32) {
        self.call_setter(self.api.libraw_set_output_color, value);
    }

    pub fn set_output_bps(&mut self, value: i32) {
        self.call_setter(self.api.libraw_set_output_bps, value);
    }

    pub fn set_no_auto_bright(&mut self, value: i32) {
        self.call_setter(self.api.libraw_set_no_auto_bright, value);
    }

    /// C# production `TrySetUseCameraWb(handle, outputColor, outputBps, noAutoBright)`: [`try_set_use_camera_wb_with`]
    /// with `crate::availability::is_exact_pinned_version` (W5-15b) and [`PINNED_WHITE_BALANCE_LAYOUT`].
    ///
    /// [`try_set_use_camera_wb_with`]: Self::try_set_use_camera_wb_with
    pub fn try_set_use_camera_wb(&mut self, output_color: i32, output_bps: i32, no_auto_bright: i32) -> bool {
        self.try_set_use_camera_wb_with(
            output_color,
            output_bps,
            no_auto_bright,
            crate::availability::is_exact_pinned_version,
            PINNED_WHITE_BALANCE_LAYOUT,
        )
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
        try_set_use_camera_wb_core(self, output_color, output_bps, no_auto_bright, version_pinned, layout)
    }

    /// Closes now (idempotent); later calls of the other methods are a bug and must not reach LibRaw: they return the
    /// neutral value (`-1` error codes: LIBRAW_UNSPECIFIED_ERROR, `0` sizes, `None`, no-op setters). C# `Dispose`.
    pub fn close(&mut self) {
        self.owner.close();
        // `libraw_close` ran: LibRaw no longer holds the callback or buffer pointers.
        self.cancel = None;
        self.shared_buffer = None;
    }

    pub fn is_closed(&self) -> bool {
        self.owner.is_closed()
    }
}

impl WhiteBalanceTarget for LibRawHandle<'_> {
    fn set_output_color(&mut self, value: i32) {
        LibRawHandle::set_output_color(self, value);
    }

    fn set_output_bps(&mut self, value: i32) {
        LibRawHandle::set_output_bps(self, value);
    }

    fn set_no_auto_bright(&mut self, value: i32) {
        LibRawHandle::set_no_auto_bright(self, value);
    }

    fn read_i32(&self, offset: i32) -> Option<i32> {
        let offset = Self::probe_window(offset)?;
        let data = self.data()?;
        // SAFETY: `offset + 4 <= DATA_PROBE_LIMIT` (65 536) lies inside the live `libraw_data_t`, which is larger: its
        // `color` member (after `params` at 5232, `rawparams`, `progress_flags`, `process_warnings`) starts with
        // `ushort curve[0x10000]` (128 KiB). Unaligned read like `Marshal.ReadInt32`; the handle is open and not shared.
        Some(unsafe { ptr::read_unaligned(data.cast::<u8>().add(offset).cast::<i32>()) })
    }

    fn write_i32(&mut self, offset: i32, value: i32) -> bool {
        let (Some(offset), Some(data)) = (Self::probe_window(offset), self.data()) else {
            return false;
        };
        // SAFETY: same window as `read_i32` (inside the live `libraw_data_t`); `&mut self` means no other access.
        unsafe { ptr::write_unaligned(data.cast::<u8>().add(offset).cast::<i32>(), value) };
        true
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
        self.header
    }

    /// The `data_size` bytes at [`PROCESSED_IMAGE_DATA_OFFSET`]; `None` after [`close`](Self::close).
    pub fn data(&self) -> Option<&[u8]> {
        if self.owner.is_closed() {
            return None;
        }
        let base = ptr::with_exposed_provenance::<u8>(self.owner.raw());
        // SAFETY: LibRaw allocates `sizeof(header) + data_size` bytes for the image (`dcraw_make_mem_image` /
        // `_thumb` in `utils_libraw.cpp`); it stays allocated until `close`, and the slice borrows `&self`.
        Some(unsafe {
            std::slice::from_raw_parts(base.add(PROCESSED_IMAGE_DATA_OFFSET), self.header.data_size as usize)
        })
    }

    pub fn close(&mut self) {
        self.owner.close();
    }

    /// C# `image.IsClosed` (W5-16's "source-released" stage).
    pub fn is_closed(&self) -> bool {
        self.owner.is_closed()
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

#[cfg(test)]
mod tests {
    use super::{DATA_PROBE_LIMIT, LibRawHandle};

    // Kills the `probe_window` boundary mutants without the DLL: the window is exactly `0 <= offset` and
    // `offset + 4 <= DATA_PROBE_LIMIT`, and an `offset + 4` overflow fails closed.
    #[test]
    fn probe_window_accepts_exactly_the_in_window_offsets() {
        assert_eq!(LibRawHandle::probe_window(0), Some(0));
        assert_eq!(LibRawHandle::probe_window(4), Some(4));
        assert_eq!(LibRawHandle::probe_window(DATA_PROBE_LIMIT - 4), Some(65_532));
        assert_eq!(LibRawHandle::probe_window(DATA_PROBE_LIMIT - 3), None);
        assert_eq!(LibRawHandle::probe_window(DATA_PROBE_LIMIT - 1), None);
        assert_eq!(LibRawHandle::probe_window(DATA_PROBE_LIMIT), None);
        assert_eq!(LibRawHandle::probe_window(DATA_PROBE_LIMIT + 4), None);
        assert_eq!(LibRawHandle::probe_window(i32::MAX - 3), None);
        assert_eq!(LibRawHandle::probe_window(i32::MAX), None);
        assert_eq!(LibRawHandle::probe_window(-1), None);
        assert_eq!(LibRawHandle::probe_window(-4), None);
        assert_eq!(LibRawHandle::probe_window(i32::MIN), None);
    }
}
