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

use std::ffi::{CStr, c_char, c_int, c_uchar, c_void};
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
        match self {
            Self::Rgb | Self::Bgr => 3,
            Self::Gray => 1,
            Self::Rgbx
            | Self::Bgrx
            | Self::Xbgr
            | Self::Xrgb
            | Self::Rgba
            | Self::Bgra
            | Self::Abgr
            | Self::Argb
            | Self::Cmyk => 4,
        }
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
        if self.denom <= 0 {
            return None;
        }
        let denom = i64::from(self.denom);
        // |dimension * num| <= 2^62 and denom - 1 < 2^31: no i64 overflow for any pair of i32 inputs.
        Some((i64::from(dimension) * i64::from(self.num) + denom - 1) / denom)
    }
}

impl fmt::Display for TjScalingFactor {
    /// C# `ToString()`: `"{Num}/{Denom}"` (invariant, e.g. `"3/8"`, `"-1/8"`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.denom)
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
        match self {
            Self::DllNotFound { message }
            | Self::BadImageFormat { message }
            | Self::EntryPointNotFound { message }
            | Self::InvalidOperation { message }
            | Self::Disposed { message }
            | Self::InvalidArgument { message } => message,
        }
    }
}

impl fmt::Display for NativeError {
    /// Writes [`NativeError::message`] only.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for NativeError {}

impl From<NativeError> for DecodeError {
    /// 1:1 onto the `DecodeError` variant of the same .NET exception type, message unchanged.
    fn from(error: NativeError) -> Self {
        match error {
            NativeError::DllNotFound { message } => Self::DllNotFound { message },
            NativeError::BadImageFormat { message } => Self::BadImageFormat { message },
            NativeError::EntryPointNotFound { message } => Self::EntryPointNotFound { message },
            NativeError::InvalidOperation { message } => Self::InvalidOperation { message },
            NativeError::Disposed { message } => Self::Disposed { message },
            NativeError::InvalidArgument { message } => Self::InvalidArgument { message },
        }
    }
}

/// .NET `EntryPointNotFoundException` text for a missing export.
fn entry_point_not_found(name: &str) -> NativeError {
    NativeError::EntryPointNotFound {
        message: format!("Unable to find an entry point named '{name}' in DLL '{DLL_NAME}'."),
    }
}

/// A Rust-only precondition failure (`ArgumentException`).
fn invalid_argument(message: String) -> NativeError {
    NativeError::InvalidArgument { message }
}

/// `usize` length as `i64` without truncation (a slice never exceeds `isize::MAX`; saturate anyway).
fn len_i64(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
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

impl Symbols {
    /// Every export the decode path calls. A build missing one of them would load and pass `tj3Init`, then fail every decode
    /// with `EntryPointNotFound` (FIX-FFI-HARDENING L2). The test-only `compress8`/`free` are not required.
    fn has_decode_exports(&self) -> bool {
        self.init.is_some()
            && self.destroy.is_some()
            && self.decompress_header.is_some()
            && self.get.is_some()
            && self.set.is_some()
            && self.set_scaling_factor.is_some()
            && self.decompress8.is_some()
            && self.get_error_str.is_some()
    }
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
    /// The `LoadLibraryExW` flags (C# AssemblyDirectory): never 0, which would mean the default search order (T1).
    pub(super) const LOAD_FLAGS: u32 = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;
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
        let s = &self.symbols;
        let resolved: Vec<&str> = [
            ("tj3Init", s.init.is_some()),
            ("tj3Destroy", s.destroy.is_some()),
            ("tj3DecompressHeader", s.decompress_header.is_some()),
            ("tj3Get", s.get.is_some()),
            ("tj3Set", s.set.is_some()),
            ("tj3SetScalingFactor", s.set_scaling_factor.is_some()),
            ("tj3Decompress8", s.decompress8.is_some()),
            ("tj3GetErrorStr", s.get_error_str.is_some()),
            ("tj3Compress8", s.compress8.is_some()),
            ("tj3Free", s.free.is_some()),
        ]
        .into_iter()
        .filter_map(|(name, present)| present.then_some(name))
        .collect();
        f.debug_struct("TurboJpegLibrary")
            .field("path", &self.path)
            .field("exports", &resolved)
            .finish()
    }
}

/// `DllNotFoundException` / `BadImageFormatException` text of a failed load (.NET wording).
fn load_failure_message(detail: &dyn fmt::Display) -> String {
    format!("Unable to load DLL '{DLL_NAME}' or one of its dependencies: {detail}")
}

#[cfg(windows)]
impl TurboJpegLibrary {
    /// `LoadLibraryExW` with the AssemblyDirectory-equivalent flags, then `GetProcAddress` for every export.
    fn load_windows(path: &Path) -> Result<Self, NativeError> {
        use std::os::windows::ffi::OsStrExt;

        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            // A NUL would silently truncate the path handed to the loader (a different file could be loaded).
            return Err(invalid_argument(format!(
                "TurboJPEG library path must not contain NUL: {}",
                path.display()
            )));
        }
        wide.push(0);

        let flags = win32::LOAD_FLAGS;
        // SAFETY: `wide` is a NUL-terminated UTF-16 string that lives for the whole call; the reserved `hFile`
        // argument must be NULL; the flags restrict the search to the DLL's folder and the safe default dirs.
        let module = unsafe { win32::LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), flags) };
        if module.is_null() {
            let error = std::io::Error::last_os_error();
            let message = load_failure_message(&error);
            return Err(match error.raw_os_error() {
                Some(win32::ERROR_BAD_EXE_FORMAT) => NativeError::BadImageFormat { message },
                // The usual codes (missing file, folder or dependency) ...
                Some(win32::ERROR_FILE_NOT_FOUND | win32::ERROR_PATH_NOT_FOUND | win32::ERROR_MOD_NOT_FOUND) => {
                    NativeError::DllNotFound { message }
                }
                // ... and every other loader failure (access denied, ...) are `DllNotFoundException` in .NET too.
                _ => NativeError::DllNotFound { message },
            });
        }

        let resolve = |name: &CStr| -> *mut c_void {
            // SAFETY: `module` is the live HMODULE returned above (freed only by `Drop`, after this function);
            // `name` is a NUL-terminated ASCII export name.
            unsafe { win32::GetProcAddress(module, name.as_ptr().cast()) }
        };
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let address = resolve($name);
                if address.is_null() {
                    None
                } else {
                    // SAFETY: `address` is a non-NULL export of this module named `$name`; `$ty` is the type alias
                    // declared above from that function's prototype in libjpeg-turbo 3.0 `turbojpeg.h` (cdecl == the
                    // x64 C ABI), and a data pointer and a fn pointer have the same size on Windows.
                    Some(unsafe { std::mem::transmute::<*mut c_void, $ty>(address) })
                }
            }};
        }
        let symbols = Symbols {
            init: symbol!(c"tj3Init", Tj3Init),
            destroy: symbol!(c"tj3Destroy", Tj3Destroy),
            decompress_header: symbol!(c"tj3DecompressHeader", Tj3DecompressHeader),
            get: symbol!(c"tj3Get", Tj3Get),
            set: symbol!(c"tj3Set", Tj3Set),
            set_scaling_factor: symbol!(c"tj3SetScalingFactor", Tj3SetScalingFactor),
            decompress8: symbol!(c"tj3Decompress8", Tj3Decompress8),
            get_error_str: symbol!(c"tj3GetErrorStr", Tj3GetErrorStr),
            compress8: symbol!(c"tj3Compress8", Tj3Compress8),
            free: symbol!(c"tj3Free", Tj3Free),
        };
        Ok(Self {
            module: module as usize,
            path: path.to_path_buf(),
            symbols,
        })
    }
}

impl TurboJpegLibrary {
    /// `true` when the library exports everything the decode path needs (used by the availability probe so a partial build
    /// reports "unavailable" up front instead of failing every decode).
    pub fn has_decode_exports(&self) -> bool {
        self.symbols.has_decode_exports()
    }

    /// `tj3Init(init)` wrapped into an owning handle bound to this library; NULL -> `InvalidOperation(null_message)`.
    fn create(self: &Arc<Self>, init: TjInit, null_message: &str) -> Result<TurboJpegHandle, NativeError> {
        let tj3_init = self.symbols.init.ok_or_else(|| entry_point_not_found("tj3Init"))?;
        // SAFETY: `tj3_init` is the `tj3Init` export of this (still loaded: `self` is alive) module with the
        // `turbojpeg.h` 3.0 signature `tjhandle tj3Init(int)`; it takes no pointer arguments.
        let instance = unsafe { tj3_init(init.as_i32()) };
        if instance.is_null() {
            return Err(NativeError::InvalidOperation {
                message: null_message.to_owned(),
            });
        }
        Ok(TurboJpegHandle {
            raw: instance as isize,
            owns: true,
            closed: false,
            binding: Binding::Library(Arc::clone(self)),
            destroy: None,
            scaling: TjScalingFactor::ONE,
        })
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
        if !path.is_absolute() {
            return Err(invalid_argument(format!(
                "TurboJPEG library path must be absolute: {}",
                path.display()
            )));
        }
        #[cfg(windows)]
        {
            Self::load_windows(path).map(Arc::new)
        }
        #[cfg(not(windows))]
        {
            Err(NativeError::DllNotFound {
                message: load_failure_message(&"run-time loading is only implemented on Windows"),
            })
        }
    }

    /// The process-wide library from [`default_library_path`], loaded on first use and cached in a `OnceLock`
    /// (success AND failure are cached: the C# probe result is cached too, and W5-14b only runs when it succeeded).
    pub fn shared() -> Result<Arc<TurboJpegLibrary>, NativeError> {
        SHARED
            .get_or_init(|| match default_library_path() {
                Some(path) => Self::load(&path),
                None => Err(NativeError::DllNotFound {
                    message: format!("Unable to load DLL '{DLL_NAME}': the application folder is unknown."),
                }),
            })
            .clone()
    }

    /// The absolute path this module was loaded from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// C# `TurboJpegNative.CreateDecompressor()` against THIS library: `tj3Init(TJINIT_DECOMPRESS)` then
    /// [`wrap_init_result`] semantics (NULL -> `InvalidOperation(INIT_FAILED_MESSAGE)`). Missing `tj3Init` ->
    /// `EntryPointNotFound` naming `tj3Init`. The handle is owning and holds an `Arc` of this library.
    pub fn create_decompressor(self: &Arc<Self>) -> Result<TurboJpegHandle, NativeError> {
        self.create(TjInit::Decompress, INIT_FAILED_MESSAGE)
    }

    /// Test support: `tj3Init(TJINIT_COMPRESS)`; NULL -> `InvalidOperation(COMPRESSOR_INIT_FAILED_MESSAGE)`.
    pub fn create_compressor(self: &Arc<Self>) -> Result<TurboJpegHandle, NativeError> {
        self.create(TjInit::Compress, COMPRESSOR_INIT_FAILED_MESSAGE)
    }
}

impl Drop for TurboJpegLibrary {
    /// `FreeLibrary(module)`; the result is ignored (nothing can be done about a failed unload).
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            // SAFETY: `module` is the HMODULE this value obtained from `LoadLibraryExW` (a value is only built after a
            // successful load) and it is freed exactly once, here. No handle or fn pointer of this library outlives
            // it: every `TurboJpegHandle` holds an `Arc` of `self`, and the `Symbols` copy dies with `self`.
            let _ = unsafe { win32::FreeLibrary(self.module as *mut c_void) };
        }
    }
}

/// `<directory of std::env::current_exe()>\turbojpeg.dll` (C# `DllImportSearchPath.AssemblyDirectory`: the app
/// folder). `None` when the executable path cannot be determined. No environment variable can redirect it.
pub fn default_library_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.parent().map(|folder| folder.join(DLL_NAME))
}

/// C# `TurboJpegNative.CreateDecompressor()`: [`TurboJpegLibrary::shared`] then
/// [`TurboJpegLibrary::create_decompressor`]. A missing DLL is `DllNotFound` (C# `DllNotFoundException`).
pub fn create_decompressor() -> Result<TurboJpegHandle, NativeError> {
    TurboJpegLibrary::shared()?.create_decompressor()
}

/// C# `TurboJpegNative.WrapInitResult(IntPtr)`: 0 -> `InvalidOperation(INIT_FAILED_MESSAGE)`; any other value
/// (including -1, exactly like C#) -> an owning handle bound to the process-wide library, which is only resolved
/// when the handle is used or released (C#'s static `tj3Destroy` binds lazily too). Loads nothing itself.
///
/// Test-only (cargo feature `test-support`, off by default): it turns an arbitrary integer into an owning handle.
#[cfg(any(test, feature = "test-support"))]
pub fn wrap_init_result(ptr: isize) -> Result<TurboJpegHandle, NativeError> {
    if ptr == 0 {
        return Err(NativeError::InvalidOperation {
            message: INIT_FAILED_MESSAGE.to_owned(),
        });
    }
    Ok(TurboJpegHandle::from_raw(ptr, true))
}

/// Release override (C# `SafeTurboJpegHandle(IntPtr, bool, Action<IntPtr>?)` test seam). Gets the raw value; an
/// `Err` is what C#'s throwing `Action` would propagate out of `Dispose`.
pub type DestroyFn = Box<dyn FnMut(isize) -> Result<(), NativeError> + Send>;

/// Where a handle's native calls (and its default release) go.
enum Binding {
    /// Created by [`TurboJpegLibrary::create_decompressor`]/`create_compressor`.
    Library(Arc<TurboJpegLibrary>),
    /// Created by `TurboJpegHandle::from_raw`/`wrap_init_result` (test-only): resolved via [`TurboJpegLibrary::shared`] on use.
    #[cfg(any(test, feature = "test-support"))]
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
        f.debug_struct("TurboJpegHandle")
            .field("raw", &self.raw)
            .field("owns", &self.owns)
            .field("closed", &self.closed)
            .field("destroy_seam", &self.destroy.is_some())
            .field("scaling", &self.scaling)
            .finish()
    }
}

impl TurboJpegHandle {
    /// C# `new SafeTurboJpegHandle(IntPtr preexistingHandle, bool ownsHandle)`: released through the process-wide
    /// library's `tj3Destroy`. Does not load the library.
    ///
    /// Test-only (cargo feature `test-support`, off by default): the safe signature would accept any integer as a
    /// native `tjhandle`, which `close` and every `tj3*` call would then pass to the DLL. Production handles come from
    /// [`TurboJpegLibrary::create_decompressor`]/`create_compressor` only.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_raw(raw: isize, owns: bool) -> Self {
        Self {
            raw,
            owns,
            closed: false,
            binding: Binding::Shared,
            destroy: None,
            scaling: TjScalingFactor::ONE,
        }
    }

    /// C# `new SafeTurboJpegHandle(IntPtr, bool, Action<IntPtr>? destroy)` with a non-null `destroy` (test seam):
    /// `destroy` replaces `tj3Destroy`. Native calls on this handle still go to the process-wide library.
    ///
    /// Test-only (cargo feature `test-support`, off by default): the safe signature would accept any integer as a
    /// native `tjhandle`, which `close` and every `tj3*` call would then pass to the DLL. Production handles come from
    /// [`TurboJpegLibrary::create_decompressor`]/`create_compressor` only.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_destroy(
        raw: isize,
        owns: bool,
        destroy: impl FnMut(isize) -> Result<(), NativeError> + Send + 'static,
    ) -> Self {
        let mut handle = Self::from_raw(raw, owns);
        handle.destroy = Some(Box::new(destroy));
        handle
    }

    /// C# `IsInvalid`: raw value 0 or -1 (independent of `closed`).
    pub fn is_invalid(&self) -> bool {
        self.raw == 0 || self.raw == -1
    }

    /// C# `IsClosed`.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// C# `DangerousGetHandle()`: the raw value, also after close.
    pub fn raw_value(&self) -> isize {
        self.raw
    }

    /// C# `SetHandleAsInvalid()`: marks the handle closed WITHOUT releasing it (tests use it on fake pointers).
    pub fn set_handle_as_invalid(&mut self) {
        self.closed = true;
    }

    /// C# `Dispose()` / `ReleaseHandle()`. First call: `closed = true`, then if `owns` and the raw value is neither 0
    /// nor -1, the seam (if set) or `tj3Destroy(raw)` runs once and its error is returned (the handle stays closed).
    /// A `Shared` binding whose library cannot be loaded returns that error (handle leaked, closed). Later calls: `Ok`.
    pub fn close(&mut self) -> Result<(), NativeError> {
        if self.closed {
            return Ok(());
        }
        // Closed BEFORE the release runs: a failing or panicking seam can never lead to a second destroy (trap T5).
        self.closed = true;
        if !self.owns || self.is_invalid() {
            return Ok(());
        }
        if let Some(destroy) = self.destroy.as_mut() {
            return destroy(self.raw);
        }
        let library = self.library()?;
        let tj3_destroy = library
            .symbols
            .destroy
            .ok_or_else(|| entry_point_not_found("tj3Destroy"))?;
        // SAFETY: the raw value is a non-NULL, non-(-1) `tjhandle` this value owns and has not destroyed (`closed`
        // was false and is now true, so this runs once); `library` keeps the module loaded for the call; signature
        // `void tj3Destroy(tjhandle)` from `turbojpeg.h` 3.0.
        unsafe { tj3_destroy(self.ptr()) };
        Ok(())
    }

    /// Guards (a) and (b) of every `tj3*` call: closed -> `Disposed`, raw 0/-1 -> `InvalidArgument`.
    fn check_usable(&self) -> Result<(), NativeError> {
        if self.closed {
            return Err(NativeError::Disposed {
                message: HANDLE_CLOSED_MESSAGE.to_owned(),
            });
        }
        if self.is_invalid() {
            // DIVERGENCE (trap T6): C# would marshal 0/-1 into the native call; Rust refuses before any load.
            return Err(invalid_argument(HANDLE_INVALID_MESSAGE.to_owned()));
        }
        Ok(())
    }

    /// Guard (c): the library this handle's calls go to.
    fn library(&self) -> Result<Arc<TurboJpegLibrary>, NativeError> {
        match &self.binding {
            Binding::Library(library) => Ok(Arc::clone(library)),
            #[cfg(any(test, feature = "test-support"))]
            Binding::Shared => TurboJpegLibrary::shared(),
        }
    }

    /// The `tjhandle` pointer (only called after `check_usable`, so neither 0 nor -1).
    fn ptr(&self) -> *mut c_void {
        self.raw as *mut c_void
    }

    /// `tj3DecompressHeader(handle, jpeg.as_ptr(), jpeg.len())`. `Ok(code)` is the raw native return (0 = success,
    /// -1 = failure, see [`error_message`](Self::error_message)), exactly what C# compares with 0. `Err` only for
    /// a closed (`Disposed`) or invalid (`InvalidArgument`) handle, an unloadable library or a missing export.
    /// An empty slice is passed through (libjpeg-turbo rejects size 0 itself and sets the error string).
    pub fn decompress_header(&mut self, jpeg: &[u8]) -> Result<i32, NativeError> {
        self.check_usable()?;
        let library = self.library()?;
        let header = library
            .symbols
            .decompress_header
            .ok_or_else(|| entry_point_not_found("tj3DecompressHeader"))?;
        // SAFETY: live, not destroyed handle (`check_usable`; `&mut self` = no concurrent use); `jpeg.as_ptr()` and
        // `jpeg.len()` come from the same slice, which outlives the call and is only read (size 0 is rejected by
        // libjpeg-turbo before any read); `library` keeps the module loaded; signature `int tj3DecompressHeader(
        // tjhandle, const unsigned char *, size_t)` from `turbojpeg.h` 3.0.
        let code = unsafe { header(self.ptr(), jpeg.as_ptr(), jpeg.len()) };
        Ok(code)
    }

    /// `tj3Get(handle, param)`; `Ok` is the native value (-1 for an unknown or not-yet-known parameter).
    pub fn get(&mut self, param: TjParam) -> Result<i32, NativeError> {
        self.check_usable()?;
        let library = self.library()?;
        let tj3_get = library.symbols.get.ok_or_else(|| entry_point_not_found("tj3Get"))?;
        // SAFETY: live, not destroyed handle used by this thread only; no pointer arguments; `library` keeps the
        // module loaded; signature `int tj3Get(tjhandle, int)` from `turbojpeg.h` 3.0.
        let value = unsafe { tj3_get(self.ptr(), param.as_i32()) };
        Ok(value)
    }

    /// `tj3Set(handle, param, value)`; `Ok` is the native return (0 / -1).
    pub fn set(&mut self, param: TjParam, value: i32) -> Result<i32, NativeError> {
        self.check_usable()?;
        let library = self.library()?;
        let tj3_set = library.symbols.set.ok_or_else(|| entry_point_not_found("tj3Set"))?;
        // SAFETY: live, not destroyed handle used by this thread only; no pointer arguments; `library` keeps the
        // module loaded; signature `int tj3Set(tjhandle, int, int)` from `turbojpeg.h` 3.0.
        let code = unsafe { tj3_set(self.ptr(), param.as_i32(), value) };
        Ok(code)
    }

    /// `tj3SetScalingFactor(handle, factor)` (struct by value); `Ok` is the native return. On `Ok(0)` the factor
    /// is remembered for the `decompress8` guard; on any other result the remembered factor is unchanged.
    pub fn set_scaling_factor(&mut self, factor: TjScalingFactor) -> Result<i32, NativeError> {
        self.check_usable()?;
        let library = self.library()?;
        let tj3_set_scaling_factor = library
            .symbols
            .set_scaling_factor
            .ok_or_else(|| entry_point_not_found("tj3SetScalingFactor"))?;
        // SAFETY: live, not destroyed handle used by this thread only; `TjScalingFactor` is `#[repr(C)] { int num;
        // int denom; }` passed BY VALUE as `turbojpeg.h` 3.0 declares `int tj3SetScalingFactor(tjhandle,
        // tjscalingfactor)` (trap T2); `library` keeps the module loaded.
        let code = unsafe { tj3_set_scaling_factor(self.ptr(), factor) };
        if code == 0 {
            self.scaling = factor;
        }
        Ok(code)
    }

    /// `tj3Decompress8(handle, jpeg, len, dst, pitch, format)` behind a soundness guard (contract "Semantics" 8):
    /// 1. closed/invalid handle, missing exports -> `Err` as above; `pitch <= 0` -> `Err(InvalidArgument)`;
    /// 2. `tj3DecompressHeader` on the SAME `jpeg` slice; a non-zero return is returned as `Ok(code)` unchanged
    ///    (the native error string stays readable through `error_message`);
    /// 3. `w = scaled(JpegWidth)`, `h = scaled(JpegHeight)` with the remembered factor (`ONE` when `tj3Get(Lossless)`
    ///    is non-zero: libjpeg-turbo ignores scaling for lossless JPEGs); `pitch < w * bytes_per_pixel`
    ///    or `dst.len() < pitch * h` (all in `i64`) -> `Err(InvalidArgument)` and NO native decode;
    /// 4. otherwise the native call; `Ok` is its return (0 / -1).
    pub fn decompress8(
        &mut self,
        jpeg: &[u8],
        dst: &mut [u8],
        pitch: i32,
        format: TjPixelFormat,
    ) -> Result<i32, NativeError> {
        self.check_usable()?;
        let library = self.library()?;
        let tj3_decompress8 = library
            .symbols
            .decompress8
            .ok_or_else(|| entry_point_not_found("tj3Decompress8"))?;
        let header = library
            .symbols
            .decompress_header
            .ok_or_else(|| entry_point_not_found("tj3DecompressHeader"))?;
        let tj3_get = library.symbols.get.ok_or_else(|| entry_point_not_found("tj3Get"))?;
        if pitch <= 0 {
            return Err(invalid_argument(format!(
                "TurboJPEG destination pitch must be positive: {pitch}."
            )));
        }

        // Re-parse THIS slice: libjpeg-turbo re-reads the header from `jpeg` inside tj3Decompress8, so only the
        // dimensions of this very buffer bound what it writes (an earlier header call may have seen another one).
        // SAFETY: live, not destroyed handle used by this thread only; pointer and length from the same slice,
        // alive and only read for the call; `library` keeps the module loaded; `turbojpeg.h` 3.0 signature.
        let code = unsafe { header(self.ptr(), jpeg.as_ptr(), jpeg.len()) };
        if code != 0 {
            return Ok(code);
        }
        // SAFETY: live, not destroyed handle used by this thread only; no pointer arguments; `int tj3Get(tjhandle,
        // int)` from `turbojpeg.h` 3.0.
        let (jpeg_width, jpeg_height) = unsafe {
            (
                tj3_get(self.ptr(), TjParam::JpegWidth.as_i32()),
                tj3_get(self.ptr(), TjParam::JpegHeight.as_i32()),
            )
        };
        // SAFETY: as above (`int tj3Get(tjhandle, int)`, no pointer arguments).
        let lossless = unsafe { tj3_get(self.ptr(), TjParam::Lossless.as_i32()) };
        // libjpeg-turbo 3.0 ignores the scaling factor for a lossless JPEG (tj3SetScalingFactor still returned 0), so
        // tj3Decompress8 writes the FULL size: bound it with ONE. Any non-zero value (1, or -1 = unknown) takes the
        // conservative full size; ONE is never smaller than a downscaled size.
        let scaling = if lossless != 0 {
            TjScalingFactor::ONE
        } else {
            self.scaling
        };
        let (Some(width), Some(height)) = (scaling.scaled(jpeg_width), scaling.scaled(jpeg_height)) else {
            return Err(invalid_argument(format!(
                "TurboJPEG scaling factor {scaling} has no scaled size."
            )));
        };
        if width <= 0 || height <= 0 {
            // Trap T11: a tables-only stream leaves the dimensions at -1; refuse instead of decoding blind.
            return Err(invalid_argument(format!(
                "TurboJPEG image dimensions are invalid: {width}x{height}."
            )));
        }
        let row_bytes = width.saturating_mul(i64::from(format.bytes_per_pixel()));
        if i64::from(pitch) < row_bytes {
            return Err(invalid_argument(format!(
                "TurboJPEG destination pitch {pitch} is smaller than one {width}-pixel row ({row_bytes} bytes)."
            )));
        }
        let required = i64::from(pitch).saturating_mul(height);
        let available = len_i64(dst.len());
        if available < required {
            return Err(invalid_argument(format!(
                "TurboJPEG destination buffer is too small: {required} bytes required for {width}x{height} at pitch \
                 {pitch}, {available} available."
            )));
        }

        // SAFETY: live, not destroyed handle used by this thread only; `jpeg` pointer/length from one live slice that
        // libjpeg-turbo only reads; `dst` is a live exclusive slice of at least `pitch * height` bytes, where `height`
        // rows of at most `pitch` bytes (`pitch >= width * bytes_per_pixel`) is exactly what tj3Decompress8 writes for
        // the header of THIS `jpeg` at the scaling factor libjpeg-turbo applies (`self.scaling` mirrors the instance's
        // factor, and a lossless image, for which libjpeg-turbo ignores it, is bounded at full size); `library`
        // keeps the module loaded; signature `int tj3Decompress8(tjhandle, const unsigned char *, size_t, unsigned
        // char *, int, int)` from `turbojpeg.h` 3.0.
        let code = unsafe {
            tj3_decompress8(
                self.ptr(),
                jpeg.as_ptr(),
                jpeg.len(),
                dst.as_mut_ptr(),
                pitch,
                format.as_i32(),
            )
        };
        Ok(code)
    }

    /// C# `TurboJpegNative.GetErrorMessage(handle)`: `tj3GetErrorStr(handle)` copied at once (`CStr`, lossy UTF-8;
    /// libjpeg-turbo messages are ASCII, so this equals C# `Marshal.PtrToStringAnsi`). `None` for a NULL pointer, and
    /// also when the handle is closed/invalid or the export/library is missing (no `Err` path: C# callers use it only
    /// to enrich another error, always with a `?? "..."` fallback).
    pub fn error_message(&mut self) -> Option<String> {
        self.check_usable().ok()?;
        let library = self.library().ok()?;
        let get_error_str = library.symbols.get_error_str?;
        // SAFETY: live, not destroyed handle used by this thread only; `char *tj3GetErrorStr(tjhandle)` from
        // `turbojpeg.h` 3.0; `library` keeps the module loaded.
        let text = unsafe { get_error_str(self.ptr()) };
        if text.is_null() {
            return None;
        }
        // SAFETY: a non-NULL result is a NUL-terminated string owned by the instance (or a thread-local); it is copied
        // here, before any other call on this handle can overwrite it (trap T8).
        let message = unsafe { CStr::from_ptr(text) };
        Some(message.to_string_lossy().into_owned())
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
        self.check_usable()?;
        let library = self.library()?;
        let tj3_compress8 = library
            .symbols
            .compress8
            .ok_or_else(|| entry_point_not_found("tj3Compress8"))?;
        if width <= 0 || pitch <= 0 || height <= 0 {
            return Err(invalid_argument(format!(
                "TurboJPEG compress size must be positive: width {width}, pitch {pitch}, height {height}."
            )));
        }
        let row_bytes = i64::from(width) * i64::from(format.bytes_per_pixel());
        if i64::from(pitch) < row_bytes {
            return Err(invalid_argument(format!(
                "TurboJPEG source pitch {pitch} is smaller than one {width}-pixel row ({row_bytes} bytes)."
            )));
        }
        let required = i64::from(pitch) * i64::from(height);
        let available = len_i64(src.len());
        if available < required {
            return Err(invalid_argument(format!(
                "TurboJPEG source buffer is too small: {required} bytes required, {available} available."
            )));
        }

        let mut jpeg_buf: *mut c_uchar = std::ptr::null_mut();
        let mut jpeg_size: usize = 0;
        // SAFETY: live, not destroyed handle used by this thread only; `src` is a live slice of at least `pitch *
        // height` bytes with `pitch >= width * bytes_per_pixel`, which is all tj3Compress8 reads; `jpeg_buf`/`jpeg_size`
        // are valid out-pointers to locals (NULL buffer = libjpeg-turbo allocates); `library` keeps the module loaded;
        // signature `int tj3Compress8(tjhandle, const unsigned char *, int, int, int, int, unsigned char **, size_t *)`
        // from `turbojpeg.h` 3.0.
        let code = unsafe {
            tj3_compress8(
                self.ptr(),
                src.as_ptr(),
                width,
                pitch,
                height,
                format.as_i32(),
                &mut jpeg_buf,
                &mut jpeg_size,
            )
        };
        let result = if code != 0 {
            let detail = self.error_message().unwrap_or_else(|| "no detail".to_owned());
            Err(NativeError::InvalidOperation {
                message: format!("tj3Compress8 failed: {detail}"),
            })
        } else if jpeg_buf.is_null() {
            Err(NativeError::InvalidOperation {
                message: "tj3Compress8 failed: no output buffer".to_owned(),
            })
        } else {
            // SAFETY: on success libjpeg-turbo set `jpeg_buf` to a buffer it allocated holding `jpeg_size`
            // initialised bytes; it stays valid until the `tj3Free` below and is copied out first.
            Ok(unsafe { std::slice::from_raw_parts(jpeg_buf, jpeg_size) }.to_vec())
        };
        if !jpeg_buf.is_null() {
            // A missing tj3Free leaks the buffer: it must never be released by Rust's allocator.
            if let Some(tj3_free) = library.symbols.free {
                // SAFETY: `jpeg_buf` was allocated by this module's tj3Compress8 and is freed once, here, after the
                // copy above; `void tj3Free(void *)` from `turbojpeg.h` 3.0.
                unsafe { tj3_free(jpeg_buf.cast()) };
                #[cfg(test)]
                tests::FREED.with(|freed| freed.set(freed.get() + 1));
            }
        }
        result
    }
}

impl Drop for TurboJpegHandle {
    /// `let _ = self.close();`
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// The cache behind [`TurboJpegLibrary::shared`].
static SHARED: OnceLock<Result<Arc<TurboJpegLibrary>, NativeError>> = OnceLock::new();

#[cfg(test)]
mod tests {
    //! In-crate tests for private details the integration tests cannot observe: the loader flags and the
    //! `tj3Free` of the `compress8` output buffer.

    use super::*;
    use std::cell::Cell;

    thread_local! {
        /// Number of `tj3Free` calls `compress8` made on this thread.
        pub(super) static FREED: Cell<usize> = const { Cell::new(0) };
    }

    /// `<repo>/native/x64/turbojpeg.dll` (absolute, no `..` components: the loader wants a fully qualified path).
    fn native_dll_path() -> PathBuf {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo = manifest
            .parent()
            .and_then(Path::parent)
            .expect("crates/<name> sits two levels below the repository root");
        repo.join("native").join("x64").join(DLL_NAME)
    }

    // Trap T1: 0 would mean the default search order (working directory, PATH); the exact flags are the C#
    // AssemblyDirectory equivalent.
    // FIX-FFI-HARDENING L2: pure; no DLL.
    #[test]
    fn decode_exports_are_all_required() {
        assert!(!Symbols::default().has_decode_exports());
        extern "C" fn init(_: c_int) -> *mut c_void {
            std::ptr::null_mut()
        }
        let only_init = Symbols {
            init: Some(init),
            ..Symbols::default()
        };
        assert!(!only_init.has_decode_exports());
    }

    #[cfg(windows)]
    #[test]
    fn load_flags_restrict_the_search_to_the_dll_folder_and_the_safe_default_dirs() {
        assert_eq!(win32::LOAD_FLAGS, 0x0000_1100);
        assert_eq!(win32::LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, 0x0000_0100);
        assert_eq!(win32::LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, 0x0000_1000);
    }

    #[test]
    #[ignore = "native: needs turbojpeg.dll"]
    fn compress8_frees_the_library_allocated_output_exactly_once() {
        let path = native_dll_path();
        assert!(
            path.is_file(),
            "missing {}: run tools/fetch-native.ps1 first",
            path.display()
        );
        let library = TurboJpegLibrary::load(&path).expect("load turbojpeg.dll");
        let mut compressor = library.create_compressor().expect("compressor");
        assert_eq!(compressor.set(TjParam::Subsamp, 0).expect("set subsamp"), 0);
        assert_eq!(compressor.set(TjParam::Quality, 90).expect("set quality"), 0);
        let (width, height) = (16i32, 8i32);
        let src = vec![0x80u8; (width * 3 * height) as usize];

        let before = FREED.with(Cell::get);
        let jpeg = compressor
            .compress8(&src, width, width * 3, height, TjPixelFormat::Rgb)
            .expect("compress");
        assert!(jpeg.starts_with(&[0xFF, 0xD8]), "output is a JPEG");
        assert_eq!(
            FREED.with(Cell::get) - before,
            1,
            "the output buffer is released through tj3Free once"
        );
    }
}
