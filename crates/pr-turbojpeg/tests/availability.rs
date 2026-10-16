//! Red tests for W5-13b (`contracts/W5-13b.md`): the TurboJPEG availability probe `pr_turbojpeg::availability`.
//!
//! Ports of `PhotoReview.Imaging.Tests/Decoding/TurboJpegNativeSeamTests.cs` (the five `RunProbe_*` tests; the handle and
//! wrap half belongs to W5-13a), `TurboJpegAvailabilityProbeTests.cs` (the `Probe_*` native test),
//! `TurboJpegGuardMutationTests.cs` (`TurboJpegAvailability_WithTheNativeDllPresent_ProbesSuccessfully`) and
//! `ErrorHandlingReviewDecodingTests.cs` (`Availability_ProbeIsTotal`), all at baseline d92d6aae.
//! Rust-only tests (no C# twin) are marked `// Rust-only:`.
//!
//! The fake-based tests need no DLL. The native test is `#[ignore = "native"]` and skips with a note when no
//! `turbojpeg.dll` sits next to the test executable (the application folder rule of the probe).

use std::error::Error;
use std::fmt;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use pr_turbojpeg::availability::{ProbeCache, ProbeOutcome, probe, probe_with};
use pr_turbojpeg::native::DLL_NAME;

/// The fake library handle value (C# `handle = 0x1234`).
const FAKE_HANDLE: usize = 0x1234;

/// Exact reason of the "not loaded" path (C# `RunProbe` first branch).
const NOT_LOADED_REASON: &str =
    "Could not load turbojpeg.dll (not found or incompatible with the current process architecture).";

type Log = Arc<Mutex<Vec<&'static str>>>;

/// Stands in for the C# `DisposeCounter`: records `"dispose"` when dropped (C# `using var`).
struct Decompressor {
    log: Log,
}

impl Drop for Decompressor {
    fn drop(&mut self) {
        self.log.lock().unwrap().push("dispose");
    }
}

type Create = Result<Decompressor, Box<dyn Error + Send + Sync>>;

/// Builds the boxed init error of one case (C# `Activator.CreateInstance(exceptionType, "boom-detail")`).
type MakeError = fn() -> Box<dyn Error + Send + Sync>;

/// Counters and log of one fake probe run (C# `ProbeFakes`: `FreeCalls`, `FreedHandle`, `Disposed`). `Disposed` is the
/// number of `"dispose"` entries in the shared log, which `Decompressor::drop` writes.
#[derive(Default)]
struct Fakes {
    free_calls: AtomicUsize,
    freed_handle: AtomicUsize,
    create_calls: AtomicUsize,
    log: Log,
}

impl Fakes {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn log(&self) -> Vec<&'static str> {
        self.log.lock().unwrap().clone()
    }

    fn free_calls(&self) -> usize {
        self.free_calls.load(Ordering::SeqCst)
    }

    fn create_calls(&self) -> usize {
        self.create_calls.load(Ordering::SeqCst)
    }

    fn disposed(&self) -> usize {
        self.log().iter().filter(|entry| **entry == "dispose").count()
    }
}

/// Runs `probe_with` with a fake load that succeeds (`loads == true`) or reports "not found", a `free` that counts and
/// records the handle, and a `create` closure supplied by the test (it may return `Err` or panic).
fn run_fake(fakes: &Arc<Fakes>, loads: bool, create: impl FnOnce(&Log) -> Create) -> ProbeOutcome {
    let free_fakes = Arc::clone(fakes);
    let create_fakes = Arc::clone(fakes);
    probe_with(
        DLL_NAME,
        move || loads.then_some(FAKE_HANDLE),
        move |handle: usize| {
            free_fakes.free_calls.fetch_add(1, Ordering::SeqCst);
            free_fakes.freed_handle.store(handle, Ordering::SeqCst);
            free_fakes.log.lock().unwrap().push("free");
        },
        move || {
            create_fakes.create_calls.fetch_add(1, Ordering::SeqCst);
            let log = Arc::clone(&create_fakes.log);
            create(&log)
        },
    )
}

/// Creates a decompressor that is dropped at the end of the probe (the success case).
fn create_ok(log: &Log) -> Create {
    Ok(Decompressor { log: Arc::clone(log) })
}

/// A non-fatal init error with a fixed `Display` text, standing in for the C# exception types.
#[derive(Debug)]
struct InitError {
    kind: &'static str,
    message: String,
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            return f.write_str("");
        }
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl Error for InitError {}

// ---- seam tests (C# TurboJpegNativeSeamTests.RunProbe_*) ----

// C#: TurboJpegNativeSeamTests.RunProbe_LibraryDoesNotLoad_ReportsUnavailableWithReasonAndInitsNothing
#[test]
fn run_probe_library_does_not_load_reports_unavailable_with_reason_and_inits_nothing() {
    let fakes = Fakes::new();

    let outcome = run_fake(&fakes, false, |_| panic!("must not be called"));

    assert!(!outcome.available);
    let reason = outcome.reason.as_deref().expect("a reason when unavailable");
    assert!(reason.contains(DLL_NAME), "reason names the dll: {reason}");
    assert!(reason.contains("Could not load"), "reason says why: {reason}");
    assert_eq!(reason, NOT_LOADED_REASON);
    assert_eq!(fakes.free_calls(), 0, "nothing was loaded, so nothing is freed");
    assert_eq!(
        fakes.create_calls(),
        0,
        "no decompressor is created when the load failed"
    );
    assert_eq!(fakes.disposed(), 0);
}

// C#: TurboJpegNativeSeamTests.RunProbe_DecompressorCreated_IsAvailableWithNoReasonAndReleasesEverything
#[test]
fn run_probe_decompressor_created_is_available_with_no_reason_and_releases_everything() {
    let fakes = Fakes::new();

    let outcome = run_fake(&fakes, true, create_ok);

    assert!(outcome.available);
    assert_eq!(outcome.reason, None);
    assert_eq!(fakes.disposed(), 1, "the decompressor is disposed once");
    assert_eq!(fakes.free_calls(), 1, "the library is freed once");
    assert_eq!(fakes.freed_handle.load(Ordering::SeqCst), FAKE_HANDLE);
}

// C#: TurboJpegNativeSeamTests.RunProbe_InitThrowsANonFatalException_ReportsUnavailableWithItsMessageAndFreesTheLibrary
// The C# Theory has four exception types (DllNotFound, BadImageFormat, InvalidOperation, EntryPointNotFound); in Rust
// each becomes an `Err` value with a different `Display` text, all carrying the same detail "boom-detail".
#[test]
fn run_probe_init_throws_a_non_fatal_exception_reports_unavailable_with_its_message_and_frees_the_library() {
    let cases: [(&str, MakeError); 4] = [
        ("DllNotFound-like io::Error", || {
            Box::new(io::Error::new(io::ErrorKind::NotFound, "boom-detail"))
        }),
        ("BadImageFormat-like custom error", || {
            Box::new(InitError {
                kind: "BadImageFormat",
                message: "boom-detail".to_owned(),
            })
        }),
        ("InvalidOperation-like custom error", || {
            Box::new(InitError {
                kind: "InvalidOperation",
                message: "boom-detail".to_owned(),
            })
        }),
        ("EntryPointNotFound-like custom error", || {
            Box::new(InitError {
                kind: "EntryPointNotFound",
                message: "boom-detail".to_owned(),
            })
        }),
    ];

    for (case, make_error) in cases {
        let fakes = Fakes::new();

        let outcome = run_fake(&fakes, true, |_| Err(make_error()));

        assert!(!outcome.available, "{case}: unavailable");
        let reason = outcome.reason.as_deref().expect("a reason when unavailable");
        assert!(
            reason.contains("loaded but failed to initialize"),
            "{case}: reason says init failed: {reason}"
        );
        assert!(
            reason.contains("boom-detail"),
            "{case}: reason carries the detail: {reason}"
        );
        assert!(reason.contains(DLL_NAME), "{case}: reason names the dll: {reason}");
        assert_eq!(fakes.free_calls(), 1, "{case}: the library is freed once");
        assert_eq!(fakes.disposed(), 0, "{case}: no decompressor was created");
    }
}

// C#: TurboJpegNativeSeamTests.RunProbe_InitRunsOutOfMemory_IsNotSwallowedButTheLibraryIsStillFreed
// Rust twin of the fatal class: the create closure PANICS. The probe must not catch it (no `catch_unwind` in production),
// the panic propagates to the caller, and the guard still frees the library during unwinding.
#[test]
fn run_probe_init_runs_out_of_memory_is_not_swallowed_but_the_library_is_still_freed() {
    let fakes = Fakes::new();

    let result = catch_unwind(AssertUnwindSafe(|| {
        run_fake(&fakes, true, |_| panic!("out of memory (fake)"))
    }));

    assert!(result.is_err(), "the fatal panic propagates out of probe_with");
    assert_eq!(fakes.free_calls(), 1, "the library is still freed exactly once");
    assert_eq!(fakes.disposed(), 0);
}

// ---- Rust-only seam guards ----

// Rust-only: the decompressor is dropped BEFORE the library is freed (contract semantics 1.3 and 1.4: the decompressor must
// not outlive the library). The shared log must read exactly ["dispose", "free"].
#[test]
fn run_probe_disposes_the_decompressor_before_freeing_the_library() {
    let fakes = Fakes::new();

    let outcome = run_fake(&fakes, true, create_ok);

    assert!(outcome.available);
    assert_eq!(fakes.log(), vec!["dispose", "free"]);
}

// Rust-only: the exact reason strings (semantics 2 and the seam reason texts), both built by `format!`.
#[test]
fn run_probe_reason_texts_are_exact() {
    let not_loaded = run_fake(&Fakes::new(), false, |_| panic!("not called"));
    assert_eq!(not_loaded.reason.as_deref(), Some(NOT_LOADED_REASON));

    let failed = run_fake(&Fakes::new(), true, |_| {
        Err(Box::new(InitError {
            kind: "InvalidOperation",
            message: "boom-detail".to_owned(),
        }))
    });
    assert_eq!(
        failed.reason.as_deref(),
        Some("turbojpeg.dll loaded but failed to initialize: InvalidOperation: boom-detail")
    );
}

// Rust-only: an error with an EMPTY message still yields a non-empty reason, because the prefix is always present
// (semantics 2: no `Debug`, no type name, no source chain is added to the text).
#[test]
fn run_probe_empty_error_message_still_yields_a_non_empty_reason() {
    let outcome = run_fake(&Fakes::new(), true, |_| {
        Err(Box::new(InitError {
            kind: "Empty",
            message: String::new(),
        }))
    });

    assert!(!outcome.available);
    let reason = outcome.reason.as_deref().expect("a reason when unavailable");
    assert!(!reason.is_empty());
    assert!(reason.contains("loaded but failed to initialize"), "reason: {reason}");
}

// Rust-only: the invariant `available == reason.is_none()` and a non-empty reason, over every fake path of the probe.
#[test]
fn run_probe_available_reason_invariant_holds_for_every_fake_outcome() {
    let outcomes = [
        run_fake(&Fakes::new(), true, create_ok),
        run_fake(&Fakes::new(), false, |_| panic!("not called")),
        run_fake(&Fakes::new(), true, |_| {
            Err(Box::new(InitError {
                kind: "Other",
                message: "boom-detail".to_owned(),
            }))
        }),
    ];

    for outcome in outcomes {
        assert_eq!(outcome.available, outcome.reason.is_none(), "{outcome:?}");
        if let Some(reason) = &outcome.reason {
            assert!(!reason.is_empty(), "{outcome:?}");
        }
    }
}

// Rust-only: the constructors of `ProbeOutcome` (`ok()` is `(true, None)`, `unavailable(r)` is `(false, Some(r))`).
#[test]
fn probe_outcome_constructors_match_the_csharp_tuple() {
    assert_eq!(
        ProbeOutcome::ok(),
        ProbeOutcome {
            available: true,
            reason: None,
        }
    );
    assert_eq!(
        ProbeOutcome::unavailable("why"),
        ProbeOutcome {
            available: false,
            reason: Some("why".to_owned()),
        }
    );
}

// ---- ProbeCache (Lazy<(bool, string?)>; Rust-only, the caching rule has no C# unit test without the DLL) ----

// Rust-only: the closure runs once for two calls and both return the same reference and value.
#[test]
fn probe_cache_runs_the_closure_once_for_two_calls_and_returns_the_same_value() {
    let cache = ProbeCache::new();
    let runs = AtomicUsize::new(0);

    let first = cache.get_or_run(|| {
        runs.fetch_add(1, Ordering::SeqCst);
        ProbeOutcome::ok()
    });
    let second = cache.get_or_run(|| {
        runs.fetch_add(1, Ordering::SeqCst);
        ProbeOutcome::unavailable("second closure must not run")
    });

    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(std::ptr::eq(first, second), "the same cached reference");
    assert_eq!(first, &ProbeOutcome::ok());
}

// Rust-only: a cached UNAVAILABLE outcome is never recomputed either (the answer never changes later in the process).
#[test]
fn probe_cache_keeps_an_unavailable_outcome_even_if_a_later_run_would_succeed() {
    let cache = ProbeCache::new();
    let runs = AtomicUsize::new(0);

    let first = cache.get_or_run(|| {
        runs.fetch_add(1, Ordering::SeqCst);
        ProbeOutcome::unavailable("missing")
    });
    let second = cache.get_or_run(|| {
        runs.fetch_add(1, Ordering::SeqCst);
        ProbeOutcome::ok()
    });

    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(!second.available);
    assert_eq!(second.reason.as_deref(), Some("missing"));
    assert!(std::ptr::eq(first, second));
}

// Rust-only: concurrent first callers (8 threads, released together by a barrier): the closure runs exactly once and
// every thread sees the same cached reference (C# `LazyThreadSafetyMode.ExecutionAndPublication`).
#[test]
fn probe_cache_concurrent_first_callers_run_the_closure_once() {
    use std::sync::Barrier;

    const THREADS: usize = 8;
    let cache = ProbeCache::new();
    let runs = AtomicUsize::new(0);
    let barrier = Barrier::new(THREADS);

    let addresses: Vec<usize> = thread::scope(|scope| {
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    let outcome = cache.get_or_run(|| {
                        runs.fetch_add(1, Ordering::SeqCst);
                        ProbeOutcome::ok()
                    });
                    assert!(outcome.available);
                    outcome as *const ProbeOutcome as usize
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("thread")).collect()
    });

    assert_eq!(runs.load(Ordering::SeqCst), 1, "the closure ran once for 8 callers");
    assert!(
        addresses.windows(2).all(|pair| pair[0] == pair[1]),
        "every caller sees the same cached outcome"
    );
}

// ---- real probe (C# TurboJpegAvailabilityProbeTests / TurboJpegGuardMutationTests / ErrorHandlingReviewDecodingTests) ----

/// True when `turbojpeg.dll` sits in the application folder, where the real probe loads it from.
fn native_dll_next_to_test_exe() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(DLL_NAME)))
        .is_some_and(|path| path.is_file())
}

// C#: TurboJpegAvailabilityProbeTests.Probe_WithTheNativeDllNextToTheTests_SucceedsWithNoReasonAndIsCached
// C#: TurboJpegGuardMutationTests.TurboJpegAvailability_WithTheNativeDllPresent_ProbesSuccessfully
#[test]
#[ignore = "native: needs turbojpeg.dll next to the test executable"]
fn probe_with_the_native_dll_next_to_the_tests_succeeds_with_no_reason_and_is_cached() {
    if !native_dll_next_to_test_exe() {
        eprintln!("skip: no {DLL_NAME} next to the test executable");
        return;
    }

    let first = probe();
    let second = probe();

    assert!(first.available, "the probe finds the native library");
    assert_eq!(first.reason, None);
    assert!(
        std::ptr::eq(first, second),
        "the second call returns the identical cached reference"
    );
}

// C#: ErrorHandlingReviewDecodingTests.Availability_ProbeIsTotal
// Not ignored: runs the real probe and must hold with or without the DLL (never throws, reason iff unavailable, non-empty).
#[test]
fn availability_probe_is_total() {
    let outcome = probe();

    assert_eq!(outcome.available, outcome.reason.is_none());
    if !outcome.available {
        let reason = outcome.reason.as_deref().expect("a reason when unavailable");
        assert!(!reason.is_empty(), "the reason is non-empty");
    }
}
