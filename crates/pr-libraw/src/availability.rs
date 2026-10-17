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

use std::error::Error;
use std::sync::OnceLock;

use crate::native;

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
    static PROBE: ProbeCache = ProbeCache::new();
    PROBE.get_or_run(|| {
        guard_probe(native::LIBRARY_NAME, || {
            Ok(probe_with(
                native::LIBRARY_NAME,
                || native::LibraryHandle::load_next_to_exe(native::LIBRARY_NAME).ok(),
                drop,
                |handle| probe_loaded_library(&HandleLibrary(handle), native::LIBRARY_NAME),
            ))
        })
    })
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
    let Some(handle) = try_load() else {
        return ProbeOutcome::unavailable(format!(
            "Could not load {dll_name} (missing, incompatible architecture, or dependent runtime unavailable)."
        ));
    };
    let guard = FreeOnDrop {
        handle: Some(handle),
        free: Some(free),
    };
    match guard.handle.as_ref() {
        Some(handle) => probe_loaded(handle),
        // Unreachable: the guard was built with `Some(handle)` just above.
        None => ProbeOutcome::unavailable(format!("{dll_name} probe failed: the loaded handle was lost")),
    }
}

/// C# `finally { NativeLibrary.Free(handle) }`: frees the handle exactly once, on normal return and on panic unwinding.
struct FreeOnDrop<H, F: FnOnce(H)> {
    handle: Option<H>,
    free: Option<F>,
}

impl<H, F: FnOnce(H)> Drop for FreeOnDrop<H, F> {
    fn drop(&mut self) {
        if let (Some(handle), Some(free)) = (self.handle.take(), self.free.take()) {
            free(handle);
        }
    }
}

/// `GuardProbe`: runs `core`; an `Err` becomes `(false, "{dll_name} probe failed: {e}")`. A panic stands for C# `OutOfMemoryException`
/// and propagates (no `catch_unwind` in production code).
pub fn guard_probe(dll_name: &str, core: impl FnOnce() -> Result<ProbeOutcome, NativeError>) -> ProbeOutcome {
    match core() {
        Ok(outcome) => outcome,
        Err(e) => ProbeOutcome::unavailable(format!("{dll_name} probe failed: {e}")),
    }
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
    let missing: Vec<&str> = required_exports()
        .iter()
        .copied()
        .filter(|name| !lib.has_export(name))
        .collect();
    if !missing.is_empty() {
        return ProbeOutcome::unavailable(format!(
            "{display_name} does not export the required LibRaw entry point(s): {} (wrong or stale library).",
            missing.join(", ")
        ));
    }

    let failed =
        |e: NativeError| ProbeOutcome::unavailable(format!("{display_name} loaded but failed to initialize: {e}"));
    let version = match lib.version() {
        Ok(version) => version,
        Err(e) => return failed(e),
    };
    if let Some(problem) = check_version(version.as_deref(), display_name) {
        return ProbeOutcome::unavailable(problem);
    }
    match lib.init_and_close() {
        Ok(true) => ProbeOutcome::ok(),
        Ok(false) => ProbeOutcome::unavailable("libraw_init returned a null decoder handle."),
        Err(e) => failed(e),
    }
}

/// [`LoadedLibrary`] over a real `native::LibraryHandle` (contract semantics 13). The caller owns (and frees) the handle.
struct HandleLibrary<'a>(&'a native::LibraryHandle);

impl LoadedLibrary for HandleLibrary<'_> {
    fn has_export(&self, name: &str) -> bool {
        self.0.has_export(name)
    }

    fn version(&self) -> Result<Option<String>, NativeError> {
        // SAFETY: `probe_loaded_library` calls this only after every required export resolved, so the library exports
        // the LibRaw API; `libraw_version` has LibRaw's `const char *libraw_version(void)` signature (static string).
        match unsafe { self.0.call_version() } {
            Some(version) => Ok(version),
            None => Err("libraw_version is not exported".into()),
        }
    }

    fn init_and_close(&self) -> Result<bool, NativeError> {
        // SAFETY: called only after all exports resolved and the version check passed (a rejected library is never
        // initialised); `libraw_init` / `libraw_close` have LibRaw's C signatures.
        match unsafe { self.0.call_init_close() } {
            Some(ok) => Ok(ok),
            None => Err("libraw_init/libraw_close is not exported".into()),
        }
    }
}

/// `RequiredExports`: every entry point of `LibRawNativeMethods.EntryPoints` plus `libraw_version`, de-duplicated, ordinal-sorted
/// (23 names today). Built once (static `OnceLock`) from `native::ENTRY_POINTS` (W5-15a) with [`build_required_exports`].
pub fn required_exports() -> &'static [&'static str] {
    static EXPORTS: OnceLock<Vec<&'static str>> = OnceLock::new();
    EXPORTS.get_or_init(|| build_required_exports(&native::ENTRY_POINTS))
}

/// Pure part of [`required_exports`]: C# `new SortedSet<string>(entryPoints, StringComparer.Ordinal) { "libraw_version" }`.
/// Plain `str` ordering is already ordinal (UTF-8 byte order equals code point order; the names are ASCII).
pub fn build_required_exports(entry_points: &[&'static str]) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = entry_points.to_vec();
    names.push("libraw_version");
    names.sort_unstable();
    names.dedup();
    names
}

/// `TryParseVersion`: `"0.22.2"` => `Some((0, 22))`. `None` (C# `major = minor = 0`) unless the text splits on `'.'` into at least
/// two parts whose first two are non-empty runs of ASCII digits that fit in `i32` (`NumberStyles.None`: no sign, no whitespace, leading
/// zeros allowed). Later parts are not looked at, so `"0.21.4-Release"` => `Some((0, 21))` but `"0.22-Release"` => `None`.
pub fn try_parse_version(text: Option<&str>) -> Option<(i32, i32)> {
    let mut parts = text.unwrap_or("").split('.');
    let major = parse_number_style_none(parts.next()?)?;
    let minor = parse_number_style_none(parts.next()?)?;
    Some((major, minor))
}

/// .NET `int.TryParse(text, NumberStyles.None, InvariantCulture)`: a non-empty run of ASCII digits that fits in `i32`; no sign,
/// no whitespace. Like .NET (`Number.TrailingZeros`), trailing `'\0'` characters after the digits are tolerated (golden cases
/// `0.22\0` and `0\0.22.2`: the C# oracle parses both, so an inner NUL before the `'.'` passes too).
fn parse_number_style_none(text: &str) -> Option<i32> {
    let digits = text.trim_end_matches('\0');
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i32>().ok()
}

/// `CheckVersion`: `None` when `version` is the pinned major.minor, otherwise the reason the library must be rejected:
/// * unparsable: `{display_name} reported an unreadable LibRaw version '{version}'.` (`None` prints as empty text)
/// * wrong major/minor: `{display_name} is LibRaw {version} but this build requires 0.22.x (run tools/fetch-libraw.ps1).`
pub fn check_version(version: Option<&str>, display_name: &str) -> Option<String> {
    let text = version.unwrap_or("");
    match try_parse_version(version) {
        None => Some(format!(
            "{display_name} reported an unreadable LibRaw version '{text}'."
        )),
        Some((major, minor)) if major != REQUIRED_MAJOR || minor != REQUIRED_MINOR => Some(format!(
            "{display_name} is LibRaw {text} but this build requires {REQUIRED_MAJOR}.{REQUIRED_MINOR}.x (run tools/fetch-libraw.ps1)."
        )),
        Some(_) => None,
    }
}

/// `CheckExactVersion`: true only for the exact pinned 0.22.2 (a suffix such as `-Release` is ignored). `"0.22"` (fewer than 3 parts),
/// `"0.22.3"`, `"0.23.2"`, `"1.22.2"`, garbage and `None` are false. The patch is the leading ASCII-digit run of the third part
/// (empty run => false, overflow => false, leading zeros accepted: `"0.22.02"` is true).
pub fn check_exact_version(version: Option<&str>) -> bool {
    if try_parse_version(version) != Some((REQUIRED_MAJOR, REQUIRED_MINOR)) {
        return false;
    }
    let Some(patch_part) = version.unwrap_or("").split('.').nth(2) else {
        return false;
    };
    let digit_len = patch_part.bytes().take_while(u8::is_ascii_digit).count();
    let digits = patch_part.get(..digit_len).unwrap_or("");
    parse_number_style_none(digits) == Some(REQUIRED_PATCH)
}

/// `ComputeExactVersion`: `probe()` first; only when it is `Ok(true)` is `version()` called and passed to [`check_exact_version`].
/// Any `Err` => `false` (C# swallows every exception except `OutOfMemoryException`; a panic propagates).
pub fn compute_exact_version(
    probe: impl FnOnce() -> Result<bool, NativeError>,
    version: impl FnOnce() -> Result<Option<String>, NativeError>,
) -> bool {
    match probe() {
        Ok(true) => match version() {
            Ok(text) => check_exact_version(text.as_deref()),
            Err(_) => false,
        },
        Ok(false) | Err(_) => false,
    }
}

/// `IsExactPinnedVersion`: cached [`compute_exact_version`] over the real probe and `native::version_string` (W5-15a). Gate for the
/// raw white-balance struct write: it is only allowed on exactly 0.22.2. Static `OnceLock<bool>`.
pub fn is_exact_pinned_version() -> bool {
    static EXACT: OnceLock<bool> = OnceLock::new();
    *EXACT.get_or_init(|| compute_exact_version(|| Ok(probe().available), || Ok(native::version_string())))
}
