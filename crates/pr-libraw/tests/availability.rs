//! Port tests for `pr_libraw::availability` (C# `PhotoReview.Imaging.LibRaw.LibRawAvailability`, baseline d92d6aae), contract
//! `contracts/W5-15b.md`. Paired with W5-15a (`native`): the native twins need its `LibraryHandle`.
//!
//! Ported behavior and contract tests for the W5-15b availability probe; native-only cases require the pinned library.
//! `ProbeCache` tests cover run-once initialization and concurrent callers.
//!
//! Native tests (`#[ignore = "native: ..."]`) need `libraw.dll` 0.22.2 next to the test executable (and `kernel32.dll`, which
//! every Windows process has). They degrade like C# `RawCorpus.RequireNative`: a missing DLL prints a skip note and returns, unless
//! `PHOTOREVIEW_RS_RAW_CORPUS_STRICT` is `1` or `true`.
//!
//! Not here: `EntryPoints_MatchTheLibraryImportDeclarations` and `LibRawAssembly_DefaultDllImportSearchPath_IsAssemblyDirectoryOnly`
//! (W5-15a, `tests/native.rs`); the other `LibRawDecoderTests` (W5-16).

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Barrier;
use std::sync::atomic::{AtomicUsize, Ordering};

use pr_libraw::availability::{
    LoadedLibrary, NativeError, ProbeCache, ProbeOutcome, build_required_exports, check_exact_version, check_version,
    compute_exact_version, guard_probe, is_exact_pinned_version, probe, probe_loaded_library, probe_with,
    required_exports, try_parse_version,
};
use pr_libraw::native::LibraryHandle;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// The 23 exports C# `LibRawAvailability.RequiredExports` checks, in ordinal order (golden `required-exports`).
const REQUIRED_EXPORTS: [&str; 23] = [
    "libraw_adjust_sizes_info_only",
    "libraw_close",
    "libraw_dcraw_clear_mem",
    "libraw_dcraw_make_mem_image",
    "libraw_dcraw_make_mem_thumb",
    "libraw_dcraw_process",
    "libraw_get_decoder_info",
    "libraw_get_iheight",
    "libraw_get_iparams",
    "libraw_get_iwidth",
    "libraw_get_raw_height",
    "libraw_get_raw_width",
    "libraw_init",
    "libraw_open_buffer",
    "libraw_open_wfile",
    "libraw_set_no_auto_bright",
    "libraw_set_output_bps",
    "libraw_set_output_color",
    "libraw_set_progress_handler",
    "libraw_strerror",
    "libraw_unpack",
    "libraw_unpack_thumb",
    "libraw_version",
];

const DLL: &str = "libraw.dll";

/// C# `RawCorpus.RequireNative` twin: `true` when the native test may run; otherwise a skip note (or a panic in strict mode).
fn require_native(available: bool, reason: Option<&str>) -> bool {
    if available {
        return true;
    }
    let strict = matches!(
        std::env::var("PHOTOREVIEW_RS_RAW_CORPUS_STRICT").as_deref(),
        Ok("1") | Ok("true")
    );
    let reason = reason.unwrap_or("libraw.dll unavailable");
    assert!(!strict, "native test skipped under strict mode: {reason}");
    eprintln!("skipped native test: {reason}");
    false
}

/// Test double of an already loaded library (C# `NativeLibrary` handle): records every call in order.
struct FakeLib {
    exports: BTreeSet<&'static str>,
    version: Result<Option<String>, String>,
    init: Result<bool, String>,
    calls: RefCell<Vec<String>>,
}

impl FakeLib {
    /// A healthy 0.22.2 library: every export, the pinned version, init succeeds.
    fn pinned() -> Self {
        Self {
            exports: REQUIRED_EXPORTS.into_iter().collect(),
            version: Ok(Some("0.22.2".to_owned())),
            init: Ok(true),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn with_version(version: Result<Option<String>, String>) -> Self {
        Self {
            version,
            ..Self::pinned()
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl LoadedLibrary for FakeLib {
    fn has_export(&self, name: &str) -> bool {
        self.calls.borrow_mut().push(format!("has_export:{name}"));
        self.exports.contains(name)
    }

    fn version(&self) -> Result<Option<String>, NativeError> {
        self.calls.borrow_mut().push("version".to_owned());
        self.version.clone().map_err(NativeError::from)
    }

    fn init_and_close(&self) -> Result<bool, NativeError> {
        self.calls.borrow_mut().push("init_and_close".to_owned());
        self.init.clone().map_err(NativeError::from)
    }
}

/// Native adapter for the real `libraw.dll` (test-only; the production one is private to `availability.rs`).
struct NativeLibrary<'a>(&'a LibraryHandle);

impl LoadedLibrary for NativeLibrary<'_> {
    fn has_export(&self, name: &str) -> bool {
        self.0.has_export(name)
    }

    fn version(&self) -> Result<Option<String>, NativeError> {
        // SAFETY: `probe_loaded_library` calls this only after every required export resolved, and `libraw_version` has
        // LibRaw's `const char *libraw_version(void)` signature.
        match unsafe { self.0.call_version() } {
            Some(version) => Ok(version),
            None => Err("libraw_version is not exported".into()),
        }
    }

    fn init_and_close(&self) -> Result<bool, NativeError> {
        // SAFETY: called only after the version check passed (a rejected library is never initialised); `libraw_init` and
        // `libraw_close` have LibRaw's C signatures.
        match unsafe { self.0.call_init_close() } {
            Some(ok) => Ok(ok),
            None => Err("libraw_init/libraw_close is not exported".into()),
        }
    }
}

// ------------------------------------------------------------------------------------------------ exports

// C#: LibRawAvailabilityTests.RequiredExports_CoverEveryImportedEntryPointAndTheVersionQuery (the exact 23 and the ordinal order too).
#[test]
fn required_exports_cover_every_imported_entry_point_and_the_version_query() {
    let exports = required_exports();

    assert!(exports.contains(&"libraw_open_wfile"));
    assert!(exports.contains(&"libraw_dcraw_make_mem_image"));
    assert!(exports.contains(&"libraw_version"));
    assert!(
        exports.len() >= 19,
        "expected all DllImport entry points, found {}",
        exports.len()
    );
    assert_eq!(exports, REQUIRED_EXPORTS.as_slice());
}

// C#: none (Rust, contract "New Rust tests"): `build_required_exports` is the pure part of `RequiredExports`.
#[test]
fn build_required_exports_dedups_and_adds_libraw_version_when_absent() {
    let built = build_required_exports(&["libraw_open_wfile", "libraw_init", "libraw_open_wfile"]);

    assert_eq!(built, vec!["libraw_init", "libraw_open_wfile", "libraw_version"]);
}

// ------------------------------------------------------------------------------------------------ version text

// C#: LibRawAvailabilityTests.TryParseVersion_ParsesMajorMinorOnly (Theory x7).
#[test]
fn try_parse_version_parses_major_minor_only() {
    type Row<'a> = (Option<&'a str>, Option<(i32, i32)>);
    let rows: [Row; 7] = [
        (Some("0.22.2"), Some((0, 22))),
        (Some("0.21.4-Release"), Some((0, 21))),
        (Some("1.0"), Some((1, 0))),
        (Some("0"), None),
        (Some("abc.def"), None),
        (Some(""), None),
        (None, None),
    ];
    for (text, expected) in rows {
        assert_eq!(try_parse_version(text), expected, "input {text:?}");
    }
}

// C#: LibRawManagedLogicTests.CheckExactVersion_OnlyTheTestedPatchReleasePasses (Theory x12).
#[test]
fn check_exact_version_only_the_tested_patch_release_passes() {
    let rows: [(Option<&str>, bool); 12] = [
        (Some("0.22.2"), true),
        (Some("0.22.2-Release"), true),
        (Some("0.22.1"), false),
        (Some("0.22.3"), false),
        (Some("0.22.9-Release"), false),
        (Some("0.22.0"), false),
        (Some("0.22"), false),
        (Some("0.23.2"), false),
        (Some("1.22.2"), false),
        (Some("garbage"), false),
        (Some(""), false),
        (None, false),
    ];
    for (version, accepted) in rows {
        assert_eq!(check_exact_version(version), accepted, "input {version:?}");
    }
}

// C#: LibRawManagedLogicTests.CheckVersion_OnlyThePinnedMajorMinorPasses (Theory x8): a rejection names the display name.
#[test]
fn check_version_only_the_pinned_major_minor_passes() {
    let rows: [(&str, bool); 8] = [
        ("0.22.2", true),
        ("0.22.0", true),
        ("0.22.9-Release", true),
        ("0.23.0", false),
        ("0.21.4", false),
        ("1.22.0", false),
        ("garbage", false),
        ("", false),
    ];
    for (version, accepted) in rows {
        let problem = check_version(Some(version), DLL);
        assert_eq!(problem.is_none(), accepted, "input {version:?}");
        if !accepted {
            assert!(problem.expect("rejection text").contains(DLL), "input {version:?}");
        }
    }
}

// ------------------------------------------------------------------------------------------------ guards

// C#: LibRawSeamGapTests.GuardProbe_ReturnsTheCoreResult_AndTurnsAFailureIntoAReason (reason contains `probe failed: boom`).
#[test]
fn guard_probe_returns_the_core_result_and_turns_a_failure_into_a_reason() {
    assert_eq!(guard_probe(DLL, || Ok(ProbeOutcome::ok())), ProbeOutcome::ok());

    let outcome = guard_probe(DLL, || Err(NativeError::from("boom")));

    assert!(!outcome.available);
    assert!(outcome.reason.expect("reason").contains("probe failed: boom"));
}

// C#: LibRawSeamGapTests.GuardProbe_MemoryExhaustion_Propagates (Rust: a panic is the uncaught class; the test catches it).
#[test]
fn guard_probe_memory_exhaustion_propagates() {
    let caught = catch_unwind(AssertUnwindSafe(|| {
        guard_probe(DLL, || -> Result<ProbeOutcome, NativeError> { panic!("out of memory") })
    }));

    // The payload must be the core's own panic, not a `todo!()` of the stub.
    let payload = caught.expect_err("the memory-exhaustion panic must propagate");
    assert_eq!(payload.downcast_ref::<&str>().copied(), Some("out of memory"));
}

// C#: LibRawSeamGapTests.ComputeExactVersion_NeedsAPassingProbeAndTheExactPatch (the version call count is 0 when the probe is false).
#[test]
fn compute_exact_version_needs_a_passing_probe_and_the_exact_patch() {
    let version_calls = Cell::new(0);
    let version = || {
        version_calls.set(version_calls.get() + 1);
        Ok(Some("0.22.2".to_owned()))
    };

    assert!(!compute_exact_version(|| Ok(false), version));
    assert_eq!(version_calls.get(), 0);
    assert!(compute_exact_version(|| Ok(true), version));
    assert_eq!(version_calls.get(), 1);
    assert!(!compute_exact_version(|| Ok(true), || Ok(Some("0.22.3".to_owned()))));
}

// C#: LibRawSeamGapTests.ComputeExactVersion_AThrowingCallIsFalse_MemoryExhaustionPropagates (Err => false; panic propagates).
#[test]
fn compute_exact_version_a_throwing_call_is_false_memory_exhaustion_propagates() {
    assert!(!compute_exact_version(
        || Err(NativeError::from("dll not found")),
        || Ok(Some("0.22.2".to_owned()))
    ));
    assert!(!compute_exact_version(
        || Ok(true),
        || Err(NativeError::from("entry point not found"))
    ));

    let caught = catch_unwind(AssertUnwindSafe(|| {
        compute_exact_version(
            || -> Result<bool, NativeError> { panic!("out of memory") },
            || Ok(Some("0.22.2".to_owned())),
        )
    }));
    let payload = caught.expect_err("the memory-exhaustion panic must propagate");
    assert_eq!(payload.downcast_ref::<&str>().copied(), Some("out of memory"));
}

// ------------------------------------------------------------------------------------------------ probe_with

// C#: none (Rust, contract "New Rust tests"): the library is freed exactly once when it was loaded.
#[test]
fn probe_with_frees_exactly_once_when_loaded() {
    let freed = Cell::new(0);

    let outcome = probe_with(
        DLL,
        || Some(7_u32),
        |_handle| freed.set(freed.get() + 1),
        |_handle| ProbeOutcome::ok(),
    );

    assert_eq!(outcome, ProbeOutcome::ok());
    assert_eq!(freed.get(), 1);
}

// C#: none (Rust, contract "New Rust tests"): a failed load is unavailable, frees nothing and probes nothing.
#[test]
fn probe_with_zero_frees_when_the_load_failed() {
    let freed = Cell::new(0);
    let probed = Cell::new(0);

    let outcome = probe_with(
        DLL,
        || None::<u32>,
        |_handle| freed.set(freed.get() + 1),
        |_handle| {
            probed.set(probed.get() + 1);
            ProbeOutcome::ok()
        },
    );

    assert!(!outcome.available);
    assert_eq!(
        outcome.reason.as_deref(),
        Some("Could not load libraw.dll (missing, incompatible architecture, or dependent runtime unavailable).")
    );
    assert_eq!(freed.get(), 0);
    assert_eq!(probed.get(), 0);
}

// C#: none (Rust, contract "New Rust tests"): the handle is freed on panic unwinding too (C# `try/finally`).
#[test]
fn probe_with_frees_the_handle_when_probe_loaded_panics() {
    let freed = Cell::new(0);

    let caught = catch_unwind(AssertUnwindSafe(|| {
        probe_with(
            DLL,
            || Some(7_u32),
            |_handle| freed.set(freed.get() + 1),
            |_handle| -> ProbeOutcome { panic!("boom") },
        )
    }));

    assert!(caught.is_err());
    assert_eq!(freed.get(), 1);
}

// ------------------------------------------------------------------------------------------------ probe_loaded_library

// C#: LibRawAvailabilityTests.ProbeLoadedLibrary_LibraryWithoutLibRawExports_NamesTheMissingEntryPoints, fake twin: `has_export` is
// false for every name; the reason lists the full sorted set and `version`/`init_and_close` are never called.
#[test]
fn probe_loaded_library_library_without_libraw_exports_names_the_missing_entry_points() {
    let lib = FakeLib {
        exports: BTreeSet::new(),
        ..FakeLib::pinned()
    };

    let outcome = probe_loaded_library(&lib, "kernel32.dll");

    assert!(!outcome.available);
    let expected = format!(
        "kernel32.dll does not export the required LibRaw entry point(s): {} (wrong or stale library).",
        REQUIRED_EXPORTS.join(", ")
    );
    assert_eq!(outcome.reason.as_deref(), Some(expected.as_str()));
    let calls = lib.calls();
    assert!(
        !calls.iter().any(|call| call == "version" || call == "init_and_close"),
        "{calls:?}"
    );
}

// C#: LibRawSeamGapTests.ProbeLoadedLibrary_LibraryWithoutLibRawExports_IsRejected (the prefix assertion), fake twin.
#[test]
fn probe_loaded_library_library_without_libraw_exports_is_rejected() {
    let lib = FakeLib {
        exports: BTreeSet::new(),
        ..FakeLib::pinned()
    };

    let outcome = probe_loaded_library(&lib, "kernel32.dll");

    assert!(!outcome.available);
    assert!(
        outcome
            .reason
            .expect("reason")
            .starts_with("kernel32.dll does not export the required LibRaw entry point(s): ")
    );
}

// C#: none (Rust, contract semantics 3): every export is probed in the sorted order, then `version`, then `init_and_close`.
#[test]
fn probe_loaded_library_calls_each_step_in_order() {
    let lib = FakeLib::pinned();

    let outcome = probe_loaded_library(&lib, DLL);

    assert_eq!(outcome, ProbeOutcome::ok());
    let mut expected: Vec<String> = REQUIRED_EXPORTS
        .iter()
        .map(|name| format!("has_export:{name}"))
        .collect();
    expected.push("version".to_owned());
    expected.push("init_and_close".to_owned());
    assert_eq!(lib.calls(), expected);
}

// C#: none (Rust, contract semantics 3.2): a wrong version is the reason and `init_and_close` is never called.
#[test]
fn probe_loaded_library_init_is_not_called_after_a_version_rejection() {
    let lib = FakeLib::with_version(Ok(Some("0.23.0".to_owned())));

    let outcome = probe_loaded_library(&lib, DLL);

    assert!(!outcome.available);
    assert_eq!(
        outcome.reason.as_deref(),
        Some("libraw.dll is LibRaw 0.23.0 but this build requires 0.22.x (run tools/fetch-libraw.ps1).")
    );
    assert!(!lib.calls().iter().any(|call| call == "init_and_close"));
}

// C#: none (Rust, contract semantics 3.3): `libraw_init` returning null is a reason; nothing is closed.
#[test]
fn probe_loaded_library_init_returning_null_is_a_reason_without_close() {
    let lib = FakeLib {
        init: Ok(false),
        ..FakeLib::pinned()
    };

    let outcome = probe_loaded_library(&lib, DLL);

    assert!(!outcome.available);
    assert_eq!(
        outcome.reason.as_deref(),
        Some("libraw_init returned a null decoder handle.")
    );
}

// C#: none (Rust, contract "New Rust tests"): an unreadable version (`Ok(None)`, a null pointer) is rejected before init.
#[test]
fn probe_loaded_library_unreadable_version_is_rejected_before_init() {
    let lib = FakeLib::with_version(Ok(None));

    let outcome = probe_loaded_library(&lib, DLL);

    assert!(!outcome.available);
    assert_eq!(
        outcome.reason.as_deref(),
        Some("libraw.dll reported an unreadable LibRaw version ''.")
    );
    assert!(!lib.calls().iter().any(|call| call == "init_and_close"));
}

// C#: none (Rust, contract semantics 3): an error from `version` is "loaded but failed to initialize".
#[test]
fn probe_loaded_library_version_error_is_reported_as_failed_to_initialize() {
    let lib = FakeLib::with_version(Err("access violation".to_owned()));

    let outcome = probe_loaded_library(&lib, DLL);

    assert_eq!(
        outcome.reason.as_deref(),
        Some("libraw.dll loaded but failed to initialize: access violation")
    );
}

// C#: none (Rust, contract semantics 3.3): an error from `init_and_close` is "loaded but failed to initialize".
#[test]
fn probe_loaded_library_init_error_is_reported_as_failed_to_initialize() {
    let lib = FakeLib {
        init: Err("access violation".to_owned()),
        ..FakeLib::pinned()
    };

    let outcome = probe_loaded_library(&lib, DLL);

    assert_eq!(
        outcome.reason.as_deref(),
        Some("libraw.dll loaded but failed to initialize: access violation")
    );
}

// C#: LibRawAvailabilityTests.ProbeLoadedLibrary_PinnedLibRaw_IsAvailable (Native): the real `libraw.dll` next to the test exe.
#[test]
#[ignore = "native: needs libraw.dll next to the test executable"]
fn probe_loaded_library_pinned_libraw_is_available() {
    let Ok(handle) = LibraryHandle::load_next_to_exe(DLL) else {
        require_native(false, Some("libraw.dll not next to the test executable"));
        return;
    };

    let outcome = probe_loaded_library(&NativeLibrary(&handle), DLL);

    assert!(outcome.available, "{:?}", outcome.reason);
    assert_eq!(outcome.reason, None);
}

// C#: LibRawAvailabilityTests.ProbeLoadedLibrary_LibraryWithoutLibRawExports_NamesTheMissingEntryPoints (Native, kernel32).
#[test]
#[ignore = "native: needs kernel32.dll (Windows only)"]
fn probe_loaded_library_library_without_libraw_exports_names_the_missing_entry_points_native() {
    let handle = LibraryHandle::load_system("kernel32.dll").expect("kernel32.dll is always loaded");

    let outcome = probe_loaded_library(&NativeLibrary(&handle), "kernel32.dll");

    assert!(!outcome.available);
    let reason = outcome.reason.expect("reason");
    assert!(reason.contains("libraw_open_wfile"), "{reason}");
    assert!(reason.contains("does not export"), "{reason}");
}

// C#: LibRawSeamGapTests.ProbeLoadedLibrary_LibraryWithoutLibRawExports_IsRejected (Native, kernel32 variant).
#[test]
#[ignore = "native: needs kernel32.dll (Windows only)"]
fn probe_loaded_library_library_without_libraw_exports_is_rejected_native() {
    let handle = LibraryHandle::load_system("kernel32.dll").expect("kernel32.dll is always loaded");

    let outcome = probe_loaded_library(&NativeLibrary(&handle), "kernel32.dll");

    assert!(!outcome.available);
    assert!(
        outcome
            .reason
            .expect("reason")
            .starts_with("kernel32.dll does not export the required LibRaw entry point(s): ")
    );
}

// ------------------------------------------------------------------------------------------------ probe and cache

// C#: LibRawDecoderTests.AvailabilityProbe_LoadsAndInitializesPinnedRuntime (Native): the only test of the real success path. The
// two calls return the identical reference.
#[test]
#[ignore = "native: needs libraw.dll 0.22.2 next to the test executable"]
fn availability_probe_loads_and_initializes_pinned_runtime() {
    let first = probe();
    assert!(first.available, "{:?}", first.reason);
    assert_eq!(first.reason, None);

    let second = probe();
    assert!(second.available, "{:?}", second.reason);
    assert_eq!(second.reason, None);
    assert!(std::ptr::eq(first, second));
}

// Real probe, valid with or without the DLL (C# `Availability_ProbeIsTotal`): never panics, the outcome invariant holds, and
// repeated calls return the same reference.
#[test]
fn probe_is_total_and_cached() {
    let first = probe();
    let second = probe();

    assert_eq!(first.available, first.reason.is_none());
    if !first.available {
        assert!(first.reason.as_deref().is_some_and(|reason| !reason.trim().is_empty()));
    }
    assert!(std::ptr::eq(first, second));
}

// C#: none (Rust, contract "New Rust tests"): the cache runs its closure once for two calls and returns the same reference.
#[test]
fn probe_cache_runs_the_closure_once_for_two_calls_and_returns_the_same_reference() {
    let cache = ProbeCache::new();
    let runs = Cell::new(0);

    let first = cache.get_or_run(|| {
        runs.set(runs.get() + 1);
        ProbeOutcome::unavailable("Could not load libraw.dll.")
    });
    let second = cache.get_or_run(|| {
        runs.set(runs.get() + 1);
        ProbeOutcome::ok()
    });

    assert_eq!(runs.get(), 1);
    assert!(std::ptr::eq(first, second));
}

// C#: none (Rust, contract "New Rust tests"): eight concurrent first callers run the closure exactly once.
#[test]
fn probe_cache_concurrent_first_callers_run_the_closure_once() {
    let cache = ProbeCache::new();
    let runs = AtomicUsize::new(0);
    let barrier = Barrier::new(8);

    let addresses: Vec<usize> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    let outcome = cache.get_or_run(|| {
                        runs.fetch_add(1, Ordering::SeqCst);
                        ProbeOutcome::ok()
                    });
                    outcome as *const ProbeOutcome as usize
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect()
    });

    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(addresses.windows(2).all(|pair| pair[0] == pair[1]));
}

// C#: none (Rust, contract "New Rust tests"): the outcome invariant `available == reason.is_none()` holds on every fake path.
#[test]
fn probe_outcome_invariant_holds_for_every_fake_path() {
    let outcomes = [
        probe_with(DLL, || None::<u32>, |_handle| {}, |_handle| ProbeOutcome::ok()),
        probe_with(
            DLL,
            || Some(1_u32),
            |_handle| {},
            |_handle| ProbeOutcome::unavailable("probe refused"),
        ),
        probe_loaded_library(&FakeLib::pinned(), DLL),
        probe_loaded_library(&FakeLib::with_version(Ok(Some("0.23.0".to_owned()))), DLL),
        probe_loaded_library(
            &FakeLib {
                init: Ok(false),
                ..FakeLib::pinned()
            },
            DLL,
        ),
        guard_probe(DLL, || Err(NativeError::from(""))),
    ];

    for outcome in outcomes {
        assert_eq!(outcome.available, outcome.reason.is_none(), "{outcome:?}");
        if let Some(reason) = &outcome.reason {
            assert!(!reason.trim().is_empty(), "{outcome:?}");
        }
    }
}

// C#: none (Rust): the exact-version gate (`IsExactPinnedVersion`) is cached; a second call agrees with the first.
#[test]
fn is_exact_pinned_version_is_cached_for_the_process() {
    assert_eq!(is_exact_pinned_version(), is_exact_pinned_version());
}

// ------------------------------------------------------------------------------------------------ golden

fn golden_inputs() -> serde_json::Map<String, Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("libraw-version.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|case| (str_of(case, "name").to_owned(), case.clone()))
        .collect()
}

/// `version` of a golden input: a string, or `null` for the null pointer.
fn version_input(input: &Value) -> Option<&str> {
    input.get("version").and_then(Value::as_str)
}

// Golden: `tests/golden/libraw-version.json` (162 cases, C# `oracle-cs libraw-version`, inputs in `tests/golden-inputs/`).
#[test]
fn golden_libraw_version_cases_match_the_csharp_oracle() {
    let golden = golden::load("libraw-version");
    let inputs = golden_inputs();
    assert_eq!(golden.cases.len(), 162);

    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = inputs
            .get(name)
            .unwrap_or_else(|| panic!("no input for golden case {name}"));
        match str_of(case, "op") {
            "try-parse" => {
                let got = try_parse_version(version_input(input));
                if bool_of(case, "ok") {
                    let expected = (i64_of(case, "major") as i32, i64_of(case, "minor") as i32);
                    assert_eq!(got, Some(expected), "{name}");
                } else {
                    assert_eq!(got, None, "{name}");
                }
            }
            "check-exact" => {
                assert_eq!(
                    check_exact_version(version_input(input)),
                    bool_of(case, "value"),
                    "{name}"
                );
            }
            "check-version" => {
                let display = input.get("display").and_then(Value::as_str).unwrap_or(DLL);
                let got = check_version(version_input(input), display);
                assert_eq!(got.as_deref(), case["problem"].as_str(), "{name}");
            }
            "required-exports" => {
                let names: Vec<&str> = case["names"]
                    .as_array()
                    .expect("names")
                    .iter()
                    .map(|name| name.as_str().expect("export name"))
                    .collect();
                assert_eq!(required_exports(), names.as_slice(), "{name}");
                assert_eq!(i64_of(case, "count"), 23, "{name}");
            }
            "compute-exact" => {
                let probe_mode = str_of(input, "probe");
                let version_value = &input["version"];
                let version_calls = Cell::new(0_i64);

                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    compute_exact_version(
                        || match probe_mode {
                            "true" => Ok(true),
                            "false" => Ok(false),
                            "throw" => Err(NativeError::from("probe threw")),
                            "oom" => panic!("out of memory"),
                            other => panic!("unknown probe mode {other}"),
                        },
                        || {
                            version_calls.set(version_calls.get() + 1);
                            match version_value {
                                Value::Null => Ok(None),
                                Value::String(text) => Ok(Some(text.clone())),
                                Value::Object(_) => Err(NativeError::from("version threw")),
                                other => panic!("unknown version input {other}"),
                            }
                        },
                    )
                }));

                match outcome {
                    Ok(value) => {
                        assert!(case["error"].is_null(), "{name}: expected an escaping error");
                        assert_eq!(Some(value), case["value"].as_bool(), "{name}");
                    }
                    Err(_) => {
                        assert_eq!(case["error"].as_str(), Some("OutOfMemoryException"), "{name}");
                        assert!(case["value"].is_null(), "{name}");
                    }
                }
                assert_eq!(version_calls.get(), i64_of(case, "version_calls"), "{name}");
            }
            "guard-probe" => {
                let core = str_of(input, "core");
                let message = str_of(input, "message");

                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    guard_probe(DLL, || match core {
                        "ok" => Ok(ProbeOutcome::ok()),
                        "unavailable" => Ok(ProbeOutcome::unavailable(message)),
                        "throw" => Err(NativeError::from(message)),
                        "oom" => panic!("out of memory"),
                        other => panic!("unknown core {other}"),
                    })
                }));

                match outcome {
                    Ok(result) => {
                        assert!(case["error"].is_null(), "{name}: expected an escaping error");
                        assert_eq!(result.available, bool_of(case, "available"), "{name}");
                        assert_eq!(result.reason.as_deref(), case["reason"].as_str(), "{name}");
                    }
                    Err(_) => {
                        assert_eq!(case["error"].as_str(), Some("OutOfMemoryException"), "{name}");
                    }
                }
            }
            other => panic!("unknown golden op {other} in {name}"),
        }
    }
}
