//! Port of `PhotoReview.Imaging.LibRaw/LibRawAvailability.cs` (C# baseline d92d6aae).
//!
//! Decides once per process whether the pinned native LibRaw runtime (`libraw.dll`, 0.22.x) can be used, BEFORE the LibRaw
//! decoder and the preview fallback are registered:
//!
//! * [`probe`]: the cached, process-wide answer (`LibRawAvailability.Probe(out reason)`); never panics, never errors out.
//! * [`probe_with`] / [`probe_loaded_library`]: the injectable seams (`RunProbeCore` / `ProbeLoadedLibrary`); every unit test uses fakes.
//! * [`is_exact_pinned_version`]: the stricter gate for raw struct writes (exactly 0.22.2), see [`check_exact_version`].
//! * Pure version logic: [`try_parse_version`], [`check_version`], [`check_exact_version`], [`required_exports`].
//!
//! No `unsafe` in the logic: loading/freeing the library and resolving exports live in the `native` module (W5-15a). The only `unsafe`
//! of the finished file is the two SAFETY-commented calls of the private `HandleLibrary` adapter (contract semantics 13) that implements
//! [`LoadedLibrary`] over `native::LibraryHandle`. This file otherwise only sequences the steps and builds the reason texts.
#![allow(unused_variables, dead_code)] // D11: stub bodies are todo!(); the porter removes this line.

use std::error::Error;
use std::sync::OnceLock;

/// Boxed failure of a native call (C# `DllNotFoundException`, `BadImageFormatException`, `EntryPointNotFoundException`,
/// `InvalidOperationException`: only the message matters).
pub type NativeError = Box<dyn Error + Send + Sync>;

/// The LibRaw major this build is pinned to (`native/libraw.sha256` pins 0.22.2).
pub const REQUIRED_MAJOR: i32 = 0;
/// The LibRaw minor this build is pinned to; patch releases are ABI compatible for the C API.
pub const REQUIRED_MINOR: i32 = 22;
/// The exact patch release whose `libraw_data_t` layout the raw struct write was derived from and verified against (0.22.2).
pub const REQUIRED_PATCH: i32 = 2;

/// C# `(bool Available, string? Reason)`.
///
/// Invariant: `available == reason.is_none()` (a reason is present exactly when unavailable and is never empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutcome {
    pub available: bool,
    pub reason: Option<String>,
}

impl ProbeOutcome {
    /// `(true, null)`.
    pub fn ok() -> Self {
        Self {
            available: true,
            reason: None,
        }
    }

    /// `(false, reason)`.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
        }
    }
}

/// Run-once cache. C# `Lazy<(bool, string?)>` (`ExecutionAndPublication`): the closure runs at most once even with concurrent first
/// callers and every caller sees the same value. Rust: [`OnceLock::get_or_init`].
#[derive(Debug, Default)]
pub struct ProbeCache {
    cell: OnceLock<ProbeOutcome>,
}

impl ProbeCache {
    pub const fn new() -> Self {
        Self { cell: OnceLock::new() }
    }

    /// Returns the cached outcome, running `run` only on the very first call.
    pub fn get_or_run(&self, run: impl FnOnce() -> ProbeOutcome) -> &ProbeOutcome {
        self.cell.get_or_init(run)
    }
}

/// What the probe needs from an already loaded `libraw.dll`. Implemented by `native` (real, unsafe) and by test fakes.
/// The caller owns the library handle; implementations never free it.
pub trait LoadedLibrary {
    /// C# `NativeLibrary.TryGetExport(handle, name, out _)`: whether the symbol resolves.
    fn has_export(&self, name: &str) -> bool;

    /// C# `Marshal.PtrToStringAnsi(libraw_version())`: `Ok(None)` for a null pointer. `Err` = the call itself failed.
    fn version(&self) -> Result<Option<String>, NativeError>;

    /// C# `libraw_init(0)` then, when the handle is non-null, `libraw_close(handle)`. `Ok(true)` = init gave a handle (and it was
    /// closed), `Ok(false)` = init returned null (nothing to close). `Err` = the call itself failed.
    fn init_and_close(&self) -> Result<bool, NativeError>;
}

/// `LibRawAvailability.Probe(out reason)`: whether the LibRaw backend can be used. Runs the real probe once per process (static
/// [`ProbeCache`]) and returns the cached outcome afterwards, even if the DLL appears or disappears later.
///
/// Real wiring (W5-15a names, contract semantics 13): `guard_probe(native::LIBRARY_NAME, || Ok(probe_with(native::LIBRARY_NAME,
/// || native::LibraryHandle::load_next_to_exe(native::LIBRARY_NAME).ok(), drop, |h| probe_loaded_library(&HandleLibrary(h), native::LIBRARY_NAME))))`,
/// where `HandleLibrary` is a private adapter implementing [`LoadedLibrary`] over `&native::LibraryHandle`.
pub fn probe() -> &'static ProbeOutcome {
    todo!("static PROBE: ProbeCache; PROBE.get_or_run(|| guard_probe(..))")
}

/// `RunProbeCore` with injectable load/free/probe-the-loaded-library steps.
///
/// 1. `try_load()` is `None`: unavailable, reason `Could not load {dll_name} (missing, incompatible architecture, or dependent runtime unavailable).`;
///    `free` and `probe_loaded` are NOT called.
/// 2. Otherwise `probe_loaded(&handle)` runs and `free(handle)` runs exactly once on EVERY exit (C# `finally`): normal return and
///    panic unwinding (a small private guard struct whose `Drop` calls `free`). The probe result is returned as is.
pub fn probe_with<H>(
    dll_name: &str,
    try_load: impl FnOnce() -> Option<H>,
    free: impl FnOnce(H),
    probe_loaded: impl FnOnce(&H) -> ProbeOutcome,
) -> ProbeOutcome {
    todo!("see semantics 2 in contracts/drafts-w5/W5-15b/contract.md")
}

/// `GuardProbe`: runs `core`; an `Err` becomes `(false, "{dll_name} probe failed: {e}")`. A panic stands for C# `OutOfMemoryException`
/// and propagates (no `catch_unwind` in production code).
pub fn guard_probe(dll_name: &str, core: impl FnOnce() -> Result<ProbeOutcome, NativeError>) -> ProbeOutcome {
    todo!("Ok(o) => o; Err(e) => ProbeOutcome::unavailable(<dll_name> probe failed: <e>)")
}

/// `ProbeLoadedLibrary`: validates an already loaded library. In this order:
///
/// 1. Every name of [`required_exports`] must resolve (checked in that sorted order, all of them, none short-circuited). Missing ones
///    => `{display_name} does not export the required LibRaw entry point(s): {a}, {b} (wrong or stale library).` (sorted, `", "`-joined).
/// 2. `version()`; `Err(e)` => `{display_name} loaded but failed to initialize: {e}`. Then [`check_version`]: a problem is the reason
///    (init is NOT called).
/// 3. `init_and_close()`; `Err(e)` => `{display_name} loaded but failed to initialize: {e}`; `Ok(false)` => `libraw_init returned a null decoder handle.`;
///    `Ok(true)` => [`ProbeOutcome::ok`].
pub fn probe_loaded_library(lib: &dyn LoadedLibrary, display_name: &str) -> ProbeOutcome {
    todo!("see semantics 3 in contracts/drafts-w5/W5-15b/contract.md")
}

/// `RequiredExports`: every entry point of `LibRawNativeMethods.EntryPoints` plus `libraw_version`, de-duplicated, ordinal-sorted
/// (23 names today). Built once (static `OnceLock`) from `native::ENTRY_POINTS` (W5-15a) with [`build_required_exports`].
pub fn required_exports() -> &'static [&'static str] {
    todo!("static OnceLock<Vec<&'static str>>; build_required_exports(crate::native::ENTRY_POINTS)")
}

/// Pure part of [`required_exports`]: C# `new SortedSet<string>(entryPoints, StringComparer.Ordinal) { "libraw_version" }`.
/// Plain `str` ordering is already ordinal (UTF-8 byte order equals code point order; the names are ASCII).
pub fn build_required_exports(entry_points: &[&'static str]) -> Vec<&'static str> {
    todo!("sort + dedup + ensure \"libraw_version\"")
}

/// `TryParseVersion`: `"0.22.2"` => `Some((0, 22))`. `None` (C# `major = minor = 0`) unless the text splits on `'.'` into at least
/// two parts whose first two are non-empty runs of ASCII digits that fit in `i32` (`NumberStyles.None`: no sign, no whitespace, leading
/// zeros allowed). Later parts are not looked at, so `"0.21.4-Release"` => `Some((0, 21))` but `"0.22-Release"` => `None`.
pub fn try_parse_version(text: Option<&str>) -> Option<(i32, i32)> {
    todo!("split('.'), all-ASCII-digit check THEN parse::<i32>() (Rust parse accepts a leading '+')")
}

/// `CheckVersion`: `None` when `version` is the pinned major.minor, otherwise the reason the library must be rejected:
/// * unparsable: `{display_name} reported an unreadable LibRaw version '{version}'.` (`None` prints as empty text)
/// * wrong major/minor: `{display_name} is LibRaw {version} but this build requires 0.22.x (run tools/fetch-libraw.ps1).`
pub fn check_version(version: Option<&str>, display_name: &str) -> Option<String> {
    todo!("see semantics 4 in contracts/drafts-w5/W5-15b/contract.md")
}

/// `CheckExactVersion`: true only for the exact pinned 0.22.2 (a suffix such as `-Release` is ignored). `"0.22"` (fewer than 3 parts),
/// `"0.22.3"`, `"0.23.2"`, `"1.22.2"`, garbage and `None` are false. The patch is the leading ASCII-digit run of the third part
/// (empty run => false, overflow => false, leading zeros accepted: `"0.22.02"` is true).
pub fn check_exact_version(version: Option<&str>) -> bool {
    todo!("see semantics 5 in contracts/drafts-w5/W5-15b/contract.md")
}

/// `ComputeExactVersion`: `probe()` first; only when it is `Ok(true)` is `version()` called and passed to [`check_exact_version`].
/// Any `Err` => `false` (C# swallows every exception except `OutOfMemoryException`; a panic propagates).
pub fn compute_exact_version(
    probe: impl FnOnce() -> Result<bool, NativeError>,
    version: impl FnOnce() -> Result<Option<String>, NativeError>,
) -> bool {
    todo!("probe()? && check_exact_version(version()?) with Err => false")
}

/// `IsExactPinnedVersion`: cached [`compute_exact_version`] over the real probe and `native::version_string` (W5-15a). Gate for the
/// raw white-balance struct write: it is only allowed on exactly 0.22.2. Static `OnceLock<bool>`.
pub fn is_exact_pinned_version() -> bool {
    todo!(
        "static EXACT: OnceLock<bool>; compute_exact_version(|| Ok(probe().available), || Ok(native::version_string()))"
    )
}
