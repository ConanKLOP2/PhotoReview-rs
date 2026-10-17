//! Port of `PhotoReview.Imaging.TurboJpeg/TurboJpegAvailability.cs` (C# baseline d92d6aae).
//!
//! Probes once whether the native `turbojpeg.dll` is present, loadable and functional, BEFORE the backend is registered with the
//! decoder factory (AR01: without the probe every decode would throw and be caught as a fallbackable error on the hot path).
//!
//! * [`probe`]: the cached, process-wide answer (`TurboJpegAvailability.Probe(out reason)`).
//! * [`probe_with`]: the injectable seam (`RunProbe(tryLoad, free, createDecompressor)`); all unit tests use fakes, no DLL needed.
//! * [`ProbeCache`]: run-once cache (`Lazy<(bool, string?)>`), instantiable so the caching rule is testable without the native library.
//!
//! No `unsafe` here: loading and freeing the library and creating the decompressor are delegated to the `native` module (W5-13a),
//! which owns every FFI block. This file only sequences them.

use std::error::Error;
use std::sync::OnceLock;

use crate::native;

/// Result of the probe: C# `(bool Available, string? Reason)`.
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

/// Run-once cache. C# `Lazy<(bool, string?)>` with the default `ExecutionAndPublication` mode: the closure runs at most once even
/// with concurrent first callers, every caller sees the same value. Rust: [`OnceLock::get_or_init`].
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

/// `TurboJpegAvailability.Probe(out reason)`: whether the TurboJPEG backend can be used. Runs the real probe once per process
/// (static [`ProbeCache`]) and returns the cached outcome afterwards. Never panics, never returns `Err`: every failure is a reason.
///
/// Real wiring (W5-13a names, see contract semantics 5): `probe_with(native::DLL_NAME, || default_library_path().and_then(|p|
/// TurboJpegLibrary::load(&p).ok()), drop, || native::create_decompressor().map_err(|e| Box::new(e) as _))`.
pub fn probe() -> &'static ProbeOutcome {
    static PROBE: ProbeCache = ProbeCache::new();
    PROBE.get_or_run(|| {
        probe_with(
            native::DLL_NAME,
            || native::default_library_path().and_then(|path| native::TurboJpegLibrary::load(&path).ok()),
            drop,
            || native::create_decompressor().map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>),
        )
    })
}

/// `TurboJpegAvailability.RunProbe(tryLoad, free, createDecompressor)` (internal in C#, `pub` here so integration tests and the
/// App crate's composition tests can inject fakes).
///
/// * `dll_name`: only used in the reason texts (C# `TurboJpegNative.DllName`, `"turbojpeg.dll"`).
/// * `try_load`: `Some(handle)` when the library loaded, `None` otherwise (C# `NativeLibrary.TryLoad`).
/// * `free`: releases the library handle; called exactly once iff `try_load` returned `Some`, on every path (success, init error,
///   panic unwinding out of `create_decompressor`), AFTER the decompressor has been dropped.
/// * `create_decompressor`: creates the decompressor; the returned value is dropped immediately (C# `using var`). `Err(e)` is the
///   C# "exception" case.
///
/// Reason texts (exact, culture invariant):
/// * not loaded: `Could not load {dll_name} (not found or incompatible with the current process architecture).`
/// * init failed: `{dll_name} loaded but failed to initialize: {e}` (`e` = `Display` of the error, C# `ex.Message`).
pub fn probe_with<H, D>(
    dll_name: &str,
    try_load: impl FnOnce() -> Option<H>,
    free: impl FnOnce(H),
    create_decompressor: impl FnOnce() -> Result<D, Box<dyn Error + Send + Sync>>,
) -> ProbeOutcome {
    let Some(handle) = try_load() else {
        return ProbeOutcome::unavailable(format!(
            "Could not load {dll_name} (not found or incompatible with the current process architecture)."
        ));
    };
    // C# `finally`: the guard frees the library exactly once on every exit, after the decompressor is dropped.
    let _guard = FreeGuard {
        state: Some((handle, free)),
    };
    match create_decompressor() {
        // C# `using var decompressor`: dropped here, before the guard.
        Ok(decompressor) => {
            drop(decompressor);
            ProbeOutcome::ok()
        }
        Err(error) => ProbeOutcome::unavailable(format!("{dll_name} loaded but failed to initialize: {error}")),
    }
}

/// Calls `free(handle)` once when dropped (also while a panic unwinds).
struct FreeGuard<H, F: FnOnce(H)> {
    state: Option<(H, F)>,
}

impl<H, F: FnOnce(H)> Drop for FreeGuard<H, F> {
    fn drop(&mut self) {
        if let Some((handle, free)) = self.state.take() {
            free(handle);
        }
    }
}
