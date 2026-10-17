//! Red tests for W5-13a (`contracts/W5-13a.md`): the TurboJPEG native layer `pr_turbojpeg::native`.
//!
//! Ports of `PhotoReview.Imaging.Tests/Decoding/TurboJpegNativeSeamTests.cs` (handle and wrap half),
//! `TurboJpegAvailabilityProbeTests.cs` (the two handle tests; the `Probe_*` ones belong to W5-13b) and
//! `TurboJpegGuardMutationTests.cs` (the three native-layer tests; the rest belongs to W5-14b), all at baseline d92d6aae.
//! Rust-only guard tests are marked `// Rust-only:`. Golden-driven tests live in `native_golden.rs`.
//!
//! Native tests are `#[ignore = "native: ..."]` and load `native/x64/turbojpeg.dll` by absolute path
//! (`tools/fetch-native.ps1`); the loader tests use temp files or `version.dll` and need no `turbojpeg.dll`.

use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::fallback::FallbackImageDecoder;
use pr_test_support::TempRoot;
use pr_test_support::golden;
use pr_turbojpeg::native::{
    COMPRESSOR_INIT_FAILED_MESSAGE, DLL_NAME, DestroyFn, HANDLE_CLOSED_MESSAGE, HANDLE_INVALID_MESSAGE,
    INIT_FAILED_MESSAGE, NativeError, TjParam, TjPixelFormat, TjScalingFactor, TurboJpegHandle, TurboJpegLibrary,
    default_library_path, wrap_init_result,
};

/// The pinned `turbojpeg.dll` next to the test binaries is NOT used: tests run from `target\...\deps`, so the DLL is
/// addressed by absolute path under the repository's `native/x64/` (contract "Native tests").
fn native_dll_path() -> PathBuf {
    golden::repo_root().join("native").join("x64").join(DLL_NAME)
}

/// Loads the pinned DLL. An explicit `--ignored` run expects the DLL, so a missing file panics with the fix.
fn load_native_library() -> Arc<TurboJpegLibrary> {
    let path = native_dll_path();
    assert!(
        path.is_file(),
        "missing {}: run tools/fetch-native.ps1 first",
        path.display()
    );
    TurboJpegLibrary::load(&path).unwrap_or_else(|e| panic!("load {}: {e}", path.display()))
}

fn fixture(relative: &str) -> PathBuf {
    golden::repo_root().join("tests").join("fixtures").join(relative)
}

fn plain_64x48() -> Vec<u8> {
    std::fs::read(fixture("wic-direct/plain-64x48.jpg")).expect("fixture wic-direct/plain-64x48.jpg")
}

/// A destroy seam that records every raw value it is given and succeeds (C# `destroyed.Add`).
fn recording() -> (Arc<Mutex<Vec<isize>>>, DestroyFn) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    let seam: DestroyFn = Box::new(move |raw| {
        sink.lock().unwrap().push(raw);
        Ok(())
    });
    (log, seam)
}

fn hash_of<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn assert_disposed<T: Debug>(result: Result<T, NativeError>) {
    match result {
        Err(NativeError::Disposed { message }) => assert_eq!(message, HANDLE_CLOSED_MESSAGE),
        other => panic!("expected Disposed, got {other:?}"),
    }
}

fn assert_invalid_argument<T: Debug>(result: Result<T, NativeError>) {
    match result {
        Err(NativeError::InvalidArgument { message }) => assert_eq!(message, HANDLE_INVALID_MESSAGE),
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

// ---- wrap and init (C# TurboJpegNativeSeamTests) ----

// C#: TurboJpegNativeSeamTests.WrapInitResult_NullPointer_ThrowsInvalidOperationNamingTheDecompressor
#[test]
fn wrap_init_result_null_pointer_throws_invalid_operation_naming_the_decompressor() {
    match wrap_init_result(0) {
        Err(NativeError::InvalidOperation { message }) => {
            assert!(
                message.contains("decompressor"),
                "message names the decompressor: {message}"
            );
            assert_eq!(message, INIT_FAILED_MESSAGE);
        }
        other => panic!("expected InvalidOperation, got {other:?}"),
    }
}

// C#: TurboJpegNativeSeamTests.WrapInitResult_NonNullPointer_ReturnsAnOwningValidHandle
#[test]
fn wrap_init_result_non_null_pointer_returns_an_owning_valid_handle() {
    let mut handle = wrap_init_result(0x40).expect("a non-null tj3Init result wraps");
    assert!(!handle.is_invalid());
    assert!(!handle.is_closed());
    assert_eq!(handle.raw_value(), 0x40);
    handle.set_handle_as_invalid(); // the pointer is fake: never let the real tj3Destroy see it
}

// C#: TurboJpegNativeSeamTests.IsInvalid_ReflectsZeroAndMinusOne
#[test]
fn is_invalid_reflects_zero_and_minus_one() {
    for (value, expected) in [(0isize, true), (-1, true), (1, false), (0x1000, false)] {
        let handle = TurboJpegHandle::from_raw(value, false);
        assert_eq!(handle.is_invalid(), expected, "raw value {value}");
    }
}

// C#: TurboJpegNativeSeamTests.Dispose_OwnedValidHandle_DestroysItExactlyOnceWithItsValue
#[test]
fn dispose_owned_valid_handle_destroys_it_exactly_once_with_its_value() {
    let (log, seam) = recording();
    let mut handle = TurboJpegHandle::with_destroy(0x40, true, seam);

    handle.close().expect("first close");
    handle.close().expect("second close is a no-op");

    assert_eq!(*log.lock().unwrap(), vec![0x40]);
    assert!(handle.is_closed());
}

// C#: TurboJpegNativeSeamTests.Dispose_ZeroOrMinusOneHandle_NeverCallsDestroy
#[test]
fn dispose_zero_or_minus_one_handle_never_calls_destroy() {
    for value in [0isize, -1] {
        let (log, seam) = recording();
        let mut handle = TurboJpegHandle::with_destroy(value, true, seam);

        handle.close().expect("an invalid handle closes without error");

        assert!(log.lock().unwrap().is_empty(), "raw {value} must not reach destroy");
        assert!(handle.is_closed());
    }
}

// C#: TurboJpegNativeSeamTests.Dispose_HandleThatIsNotOwned_NeverCallsDestroy
#[test]
fn dispose_handle_that_is_not_owned_never_calls_destroy() {
    let (log, seam) = recording();
    let mut handle = TurboJpegHandle::with_destroy(0x40, false, seam);

    handle.close().expect("an unowned handle closes without error");

    assert!(log.lock().unwrap().is_empty());
}

// C#: TurboJpegNativeSeamTests.Dispose_DestroyThrows_PropagatesTheFailureAndTheHandleStaysClosed
#[test]
fn dispose_destroy_throws_propagates_the_failure_and_the_handle_stays_closed() {
    let mut handle = TurboJpegHandle::with_destroy(0x40, true, |_| {
        Err(NativeError::InvalidOperation {
            message: "destroy failed".to_owned(),
        })
    });

    match handle.close() {
        Err(NativeError::InvalidOperation { message }) => assert_eq!(message, "destroy failed"),
        other => panic!("expected the destroy failure, got {other:?}"),
    }
    assert!(handle.is_closed());
}

// ---- availability probe handle tests (C# TurboJpegAvailabilityProbeTests) ----

// C#: TurboJpegAvailabilityProbeTests.Dispose_InvalidHandleValue_DoesNotCrashAndClosesTheHandle
// (no DLL needed: a 0 or -1 handle never resolves the library, so this test is not ignored)
#[test]
fn dispose_invalid_handle_value_does_not_crash_and_closes_the_handle() {
    for value in [0isize, -1] {
        let mut handle = TurboJpegHandle::from_raw(value, true);
        assert!(handle.is_invalid());

        handle
            .close()
            .expect("an invalid handle never reaches tj3Destroy, so its release cannot fail");

        assert!(handle.is_closed());
    }
}

// C#: TurboJpegAvailabilityProbeTests.Dispose_RealDecompressorHandle_IsValidUntilDisposedAndDisposeTwiceIsHarmless
#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn dispose_real_decompressor_handle_is_valid_until_disposed_and_dispose_twice_is_harmless() {
    let library = load_native_library();
    let mut handle = library
        .create_decompressor()
        .expect("tj3Init(decompress) succeeds with the pinned DLL");
    assert!(!handle.is_invalid());
    assert!(!handle.is_closed());

    handle.close().expect("first dispose");
    handle.close().expect("second dispose is harmless");

    assert!(handle.is_closed());
}

// ---- guard mutation tests (C# TurboJpegGuardMutationTests, native-layer subset) ----

// C#: TurboJpegGuardMutationTests.TjScalingFactor_Equality_NeedsBothNumeratorAndDenominator
// (derived PartialEq/Hash: this passes on the stub; the contract says equality is derived)
#[test]
fn tj_scaling_factor_equality_needs_both_numerator_and_denominator() {
    let rows = [
        (1, 2, 1, 2, true),
        (1, 2, 1, 4, false),
        (1, 2, 3, 2, false),
        (1, 2, 3, 4, false),
    ];
    for (n1, d1, n2, d2, equal) in rows {
        let a = TjScalingFactor::new(n1, d1);
        let b = TjScalingFactor::new(n2, d2);

        assert_eq!(a == b, equal, "{n1}/{d1} == {n2}/{d2}");
        assert_eq!(a != b, !equal, "{n1}/{d1} != {n2}/{d2}");
        assert_eq!(a.eq(&b), equal, "Equals(object) of {n1}/{d1} and {n2}/{d2}");
        if equal {
            assert_eq!(hash_of(&a), hash_of(&b), "equal factors hash alike");
        }
    }
}

// C#: TurboJpegGuardMutationTests.SafeTurboJpegHandle_InvalidHandleValue_IsInvalidAndDisposesWithoutCallingIntoTheNativeLibrary
#[test]
fn safe_turbojpeg_handle_invalid_handle_value_is_invalid_and_disposes_without_calling_into_the_native_library() {
    for value in [0isize, -1] {
        let mut handle = TurboJpegHandle::from_raw(value, true);
        assert!(handle.is_invalid());
        assert!(handle.close().is_ok(), "raw {value} disposes without an error");
        assert!(handle.is_closed());
    }
}

// C#: TurboJpegGuardMutationTests.SafeTurboJpegHandle_RealHandle_IsValidAndClosesOnDispose
#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn safe_turbojpeg_handle_real_handle_is_valid_and_closes_on_dispose() {
    let library = load_native_library();
    let mut handle = library
        .create_decompressor()
        .expect("tj3Init(decompress) succeeds with the pinned DLL");

    assert!(!handle.is_invalid());
    handle.close().expect("first dispose");
    handle.close().expect("second dispose");
    assert!(handle.is_closed());
}

// ---- Rust-only: pure values and guards (no DLL) ----

// Rust-only: `tj3SetScalingFactor` takes `tjscalingfactor` by value; the layout must be two C ints, num first.
// (layout is declared in the stub; passes on the stub, guards a later field reorder)
#[test]
fn tj_scaling_factor_is_two_c_ints_by_value() {
    assert_eq!(std::mem::size_of::<TjScalingFactor>(), 8);
    assert_eq!(std::mem::align_of::<TjScalingFactor>(), 4);
    assert_eq!(std::mem::offset_of!(TjScalingFactor, num), 0);
    assert_eq!(std::mem::offset_of!(TjScalingFactor, denom), 4);
}

// Rust-only: compile-time check that a `tjhandle` may move between threads (contract trap T9).
#[test]
fn turbojpeg_handle_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<TurboJpegHandle>();
}

// Rust-only: the library is shared through `Arc` and the process-wide cache, so it must be Send + Sync (trap T9).
#[test]
fn turbojpeg_library_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TurboJpegLibrary>();
}

// Rust-only: libjpeg-turbo `tjPixelSize`: 3 for Rgb/Bgr, 1 for Gray, 4 for the other nine formats (incl. Cmyk).
#[test]
fn bytes_per_pixel_matches_tj_pixel_size() {
    let expected = [
        (TjPixelFormat::Rgb, 3),
        (TjPixelFormat::Bgr, 3),
        (TjPixelFormat::Rgbx, 4),
        (TjPixelFormat::Bgrx, 4),
        (TjPixelFormat::Xbgr, 4),
        (TjPixelFormat::Xrgb, 4),
        (TjPixelFormat::Gray, 1),
        (TjPixelFormat::Rgba, 4),
        (TjPixelFormat::Bgra, 4),
        (TjPixelFormat::Abgr, 4),
        (TjPixelFormat::Argb, 4),
        (TjPixelFormat::Cmyk, 4),
    ];
    for (format, bytes) in expected {
        assert_eq!(format.bytes_per_pixel(), bytes, "{format:?}");
    }
}

// Rust-only: `TJSCALED` in 64-bit arithmetic; a non-positive denominator has no scaled size.
#[test]
fn scaled_rejects_non_positive_denominator() {
    assert_eq!(TjScalingFactor::new(1, 0).scaled(800), None);
    assert_eq!(TjScalingFactor::new(1, -1).scaled(800), None);
    assert_eq!(TjScalingFactor::new(1, 8).scaled(800), Some(100));
}

// Rust-only: a handle closed first refuses every call with `Disposed` and never loads the library (no DLL here).
#[test]
fn calls_on_a_closed_handle_are_disposed_and_never_reach_the_library() {
    let mut handle = TurboJpegHandle::with_destroy(0x40, true, |_| Ok(()));
    handle.close().expect("close before the calls");
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xD9];
    let mut dst = [0u8; 4];

    assert_disposed(handle.get(TjParam::JpegWidth));
    assert_disposed(handle.set(TjParam::Quality, 90));
    assert_disposed(handle.decompress_header(JPEG));
    assert_disposed(handle.set_scaling_factor(TjScalingFactor::ONE));
    assert_disposed(handle.decompress8(JPEG, &mut dst, 4, TjPixelFormat::Bgra));
    assert_disposed(handle.compress8(&dst, 1, 4, 1, TjPixelFormat::Bgra));
    assert_eq!(handle.error_message(), None);
}

// Rust-only: raw 0 and -1 (never owned) refuse every call before the library is touched (contract trap T6, DIVERGENCE).
#[test]
fn calls_on_an_invalid_handle_are_refused_before_the_library_is_resolved() {
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xD9];
    for raw in [0isize, -1] {
        let mut handle = TurboJpegHandle::from_raw(raw, false);
        let mut dst = [0u8; 4];

        assert_invalid_argument(handle.get(TjParam::JpegWidth));
        assert_invalid_argument(handle.set(TjParam::Quality, 90));
        assert_invalid_argument(handle.decompress_header(JPEG));
        assert_invalid_argument(handle.set_scaling_factor(TjScalingFactor::ONE));
        assert_invalid_argument(handle.decompress8(JPEG, &mut dst, 4, TjPixelFormat::Bgra));
        assert_invalid_argument(handle.compress8(&dst, 1, 4, 1, TjPixelFormat::Bgra));
        assert_eq!(handle.error_message(), None, "raw {raw}");
    }
}

// Rust-only: closing an unowned or invalid handle never resolves the process-wide library (the destroy is not called,
// so a missing DLL must not turn this into an error). Unit-level proof of the cache needs a crate-private probe.
#[test]
fn close_of_unowned_or_invalid_handle_never_resolves_the_shared_library() {
    let mut unowned = TurboJpegHandle::from_raw(0x40, false);
    assert!(unowned.close().is_ok());
    for raw in [0isize, -1] {
        let mut invalid = TurboJpegHandle::from_raw(raw, true);
        assert!(
            invalid.close().is_ok(),
            "raw {raw} with owns=true never reaches tj3Destroy"
        );
    }
}

// Rust-only: dropping an open owned handle releases it exactly once (C# finalizer, `Drop` = `close`).
#[test]
fn drop_closes_an_open_owned_handle_once() {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&count);
    let handle = TurboJpegHandle::with_destroy(0x40, true, move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });

    drop(handle);

    assert_eq!(count.load(Ordering::SeqCst), 1);
}

// Rust-only: an explicit close followed by `Drop` must not destroy a second time (contract trap T5).
#[test]
fn drop_after_close_does_not_destroy_again() {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&count);
    let mut handle = TurboJpegHandle::with_destroy(0x40, true, move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    handle.close().expect("explicit close");

    drop(handle);

    assert_eq!(count.load(Ordering::SeqCst), 1);
}

/// The `DecodeError` message of the six native variants (the variant itself is checked by the caller).
fn decode_message(error: &DecodeError) -> &str {
    match error {
        DecodeError::DllNotFound { message }
        | DecodeError::BadImageFormat { message }
        | DecodeError::EntryPointNotFound { message }
        | DecodeError::InvalidOperation { message }
        | DecodeError::Disposed { message }
        | DecodeError::InvalidArgument { message } => message,
        other => panic!("not a native variant: {other:?}"),
    }
}

// Rust-only: each native exception maps onto the `DecodeError` variant of the same .NET type, message unchanged, and the
// INV-12 classification holds: DllNotFound, BadImageFormat and EntryPointNotFound fall back, the other three do not.
#[test]
fn native_error_maps_onto_the_decode_error_of_the_same_exception_type() {
    let cases = [
        (
            NativeError::DllNotFound {
                message: "dll".to_owned(),
            },
            true,
        ),
        (
            NativeError::BadImageFormat {
                message: "bad".to_owned(),
            },
            true,
        ),
        (
            NativeError::EntryPointNotFound {
                message: "entry".to_owned(),
            },
            true,
        ),
        (
            NativeError::InvalidOperation {
                message: "init".to_owned(),
            },
            false,
        ),
        (
            NativeError::Disposed {
                message: "closed".to_owned(),
            },
            false,
        ),
        (
            NativeError::InvalidArgument {
                message: "arg".to_owned(),
            },
            false,
        ),
    ];
    for (native, fallbackable) in cases {
        let expected_message = native.message().to_owned();
        let decode = DecodeError::from(native.clone());

        let same_variant = matches!(
            (&native, &decode),
            (NativeError::DllNotFound { .. }, DecodeError::DllNotFound { .. })
                | (NativeError::BadImageFormat { .. }, DecodeError::BadImageFormat { .. })
                | (
                    NativeError::EntryPointNotFound { .. },
                    DecodeError::EntryPointNotFound { .. }
                )
                | (
                    NativeError::InvalidOperation { .. },
                    DecodeError::InvalidOperation { .. }
                )
                | (NativeError::Disposed { .. }, DecodeError::Disposed { .. })
                | (NativeError::InvalidArgument { .. }, DecodeError::InvalidArgument { .. })
        );
        assert!(same_variant, "{native:?} maps to {decode:?}");
        assert_eq!(decode_message(&decode), expected_message);
        assert_eq!(
            FallbackImageDecoder::is_fallbackable(&decode),
            fallbackable,
            "{decode:?}"
        );
    }
}

// Rust-only: a relative DLL path is refused and nothing is searched (contract trap T1, DLL search hijack).
#[test]
fn load_relative_path_is_invalid_argument_and_searches_nothing() {
    match TurboJpegLibrary::load(Path::new("turbojpeg.dll")) {
        Err(NativeError::InvalidArgument { message }) => {
            assert!(message.contains("must be absolute"), "{message}");
            assert!(message.contains("turbojpeg.dll"), "{message}");
        }
        other => panic!("expected InvalidArgument for a relative path, got {other:?}"),
    }
}

// Rust-only: the default library path is `turbojpeg.dll` in the executable's folder (C# AssemblyDirectory), nothing else.
#[test]
fn default_library_path_is_next_to_the_current_exe() {
    let exe = std::env::current_exe().expect("current_exe");
    let folder = exe.parent().expect("exe has a folder");
    assert_eq!(default_library_path(), Some(folder.join(DLL_NAME)));
}

// ---- OS loader without the real DLL (contract "Native tests", W5-13a) ----

#[cfg(windows)]
#[test]
#[ignore = "native: LoadLibraryExW on temp files, no turbojpeg.dll needed"]
fn load_missing_file_reports_dll_not_found_without_panicking() {
    let root = TempRoot::new("turbo-native-missing");
    match TurboJpegLibrary::load(&root.join(DLL_NAME)) {
        Err(NativeError::DllNotFound { message }) => {
            assert!(message.contains("turbojpeg.dll"), "{message}");
        }
        other => panic!("expected DllNotFound for a missing file, got {other:?}"),
    }
}

#[cfg(windows)]
#[test]
#[ignore = "native: LoadLibraryExW on temp files, no turbojpeg.dll needed"]
fn load_non_pe_file_reports_bad_image_format() {
    let root = TempRoot::new("turbo-native-not-pe");
    std::fs::write(root.join(DLL_NAME), b"this is text, not a PE image\r\n").expect("write fake dll");
    match TurboJpegLibrary::load(&root.join(DLL_NAME)) {
        Err(NativeError::BadImageFormat { .. }) => {}
        other => panic!("expected BadImageFormat for a text file, got {other:?}"),
    }
}

#[cfg(windows)]
#[test]
#[ignore = "native: LoadLibraryExW on version.dll, no turbojpeg.dll needed"]
fn load_of_a_dll_without_tj3_exports_succeeds_and_each_call_reports_entry_point_not_found() {
    let system_root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let version_dll = system_root.join("System32").join("version.dll");

    let library = TurboJpegLibrary::load(&version_dll).expect("a real DLL without tj3* loads");
    match library.create_decompressor() {
        Err(NativeError::EntryPointNotFound { message }) => {
            assert!(message.contains("tj3Init"), "names the missing export: {message}");
        }
        other => panic!("expected EntryPointNotFound for tj3Init, got {other:?}"),
    }
}

// ---- with the real DLL (contract "Native tests": decompress8 guards, handle lifetime, round trip) ----

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decompress8_refuses_a_destination_one_byte_short_without_writing() {
    let library = load_native_library();
    let mut handle = library.create_decompressor().expect("decompressor");
    let jpeg = plain_64x48();
    assert_eq!(handle.decompress_header(&jpeg).expect("header call"), 0);
    let pitch = 64 * 4;
    let mut dst = vec![0xAB; pitch * 48 - 1];

    match handle.decompress8(&jpeg, &mut dst, pitch as i32, TjPixelFormat::Bgrx) {
        Err(NativeError::InvalidArgument { .. }) => {}
        other => panic!("expected InvalidArgument for a short destination, got {other:?}"),
    }
    assert!(dst.iter().all(|&b| b == 0xAB), "the destination must not be written");
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decompress8_refuses_a_pitch_below_width_times_pixel_size() {
    let library = load_native_library();
    let mut handle = library.create_decompressor().expect("decompressor");
    let jpeg = plain_64x48();
    assert_eq!(handle.decompress_header(&jpeg).expect("header call"), 0);
    let pitch = 64 * 4 - 1;
    let mut dst = vec![0u8; pitch * 48];

    assert!(matches!(
        handle.decompress8(&jpeg, &mut dst, pitch as i32, TjPixelFormat::Bgrx),
        Err(NativeError::InvalidArgument { .. })
    ));
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decompress8_refuses_zero_and_negative_pitch() {
    let library = load_native_library();
    let mut handle = library.create_decompressor().expect("decompressor");
    let jpeg = plain_64x48();
    let mut dst = vec![0u8; 1];

    for pitch in [0, -4] {
        assert!(
            matches!(
                handle.decompress8(&jpeg, &mut dst, pitch, TjPixelFormat::Bgrx),
                Err(NativeError::InvalidArgument { .. })
            ),
            "pitch {pitch} is refused"
        );
    }
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decompress8_uses_the_remembered_scaling_factor_for_its_guard() {
    let library = load_native_library();
    let mut handle = library.create_decompressor().expect("decompressor");
    let jpeg = plain_64x48();
    assert_eq!(handle.decompress_header(&jpeg).expect("header call"), 0);

    // 1/8 of 64x48 is 8x6: a buffer sized for it succeeds.
    assert_eq!(
        handle.set_scaling_factor(TjScalingFactor::new(1, 8)).expect("set 1/8"),
        0
    );
    let pitch = 8 * 4;
    let mut dst = vec![0u8; pitch * 6];
    assert_eq!(
        handle
            .decompress8(&jpeg, &mut dst, pitch as i32, TjPixelFormat::Bgrx)
            .expect("decode"),
        0
    );

    // An invalid factor is refused by libjpeg-turbo and the guard keeps using 1/8 (still a valid buffer).
    assert_ne!(
        handle.set_scaling_factor(TjScalingFactor::new(2, 3)).expect("set 2/3"),
        0
    );
    assert_eq!(
        handle
            .decompress8(&jpeg, &mut dst, pitch as i32, TjPixelFormat::Bgrx)
            .expect("decode"),
        0
    );
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decompress8_with_a_different_jpeg_than_the_header_call_is_still_bounded() {
    let library = load_native_library();
    let mut handle = library.create_decompressor().expect("decompressor");
    assert_eq!(
        handle.decompress_header(&plain_64x48()).expect("header call on 64x48"),
        0
    );
    let big = std::fs::read(fixture("wic-direct/gradient-800x600.jpg")).expect("fixture 800x600");
    let mut dst = vec![0u8; 64 * 4 * 48];

    // The soundness guard re-parses THIS slice (800x600) and refuses the 64x48 destination before the native decode.
    assert!(matches!(
        handle.decompress8(&big, &mut dst, 64 * 4, TjPixelFormat::Bgrx),
        Err(NativeError::InvalidArgument { .. })
    ));
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn error_message_after_a_failed_header_is_the_native_text() {
    let decode = golden::load("turbojpeg-native-decode");
    let case = decode
        .cases
        .iter()
        .find(|c| golden::str_of(c, "name") == "strict-wic-direct-text")
        .expect("golden case strict-wic-direct-text");
    let expected = golden::str_of(case, "header_error");

    let library = load_native_library();
    let mut handle = library.create_decompressor().expect("decompressor");
    let bytes = std::fs::read(fixture(golden::str_of(case, "fixture"))).expect("fixture text.jpg");
    assert_ne!(handle.decompress_header(&bytes).expect("header call"), 0);

    assert_eq!(handle.error_message().as_deref(), Some(expected));
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn two_libraries_loaded_from_the_same_path_share_one_module() {
    let path = native_dll_path();
    let first = TurboJpegLibrary::load(&path).expect("first load");
    let second = TurboJpegLibrary::load(&path).expect("second load of the same path");
    drop(first); // FreeLibrary is reference counted: the module stays loaded for `second`

    let mut handle = second
        .create_decompressor()
        .expect("the surviving library still initialises");
    assert_eq!(handle.decompress_header(&plain_64x48()).expect("header call"), 0);
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn compress8_round_trips_through_decompress8() {
    let library = load_native_library();
    let mut compressor = library.create_compressor().expect("compressor");
    assert_eq!(compressor.set(TjParam::Quality, 92).expect("set quality"), 0);
    // libjpeg-turbo 3.0 refuses to compress without TJPARAM_SUBSAMP (C# TurboJpegSyntheticFormatTests sets it too).
    assert_eq!(compressor.set(TjParam::Subsamp, 0).expect("set subsamp 4:4:4"), 0);

    let (width, height) = (64i32, 48i32);
    let pitch = width * 4;
    let mut src = vec![0u8; (pitch * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let at = (y * pitch + x * 4) as usize;
            src[at] = (x * 255 / width) as u8;
            src[at + 1] = (y * 255 / height) as u8;
            src[at + 2] = 128;
            src[at + 3] = 255;
        }
    }
    let jpeg = compressor
        .compress8(&src, width, pitch, height, TjPixelFormat::Bgra)
        .expect("compress");

    let mut decoder = library.create_decompressor().expect("decompressor");
    assert_eq!(decoder.decompress_header(&jpeg).expect("header call"), 0);
    assert_eq!(decoder.get(TjParam::JpegWidth).expect("width"), width);
    assert_eq!(decoder.get(TjParam::JpegHeight).expect("height"), height);
    let mut dst = vec![0u8; (pitch * height) as usize];
    assert_eq!(
        decoder
            .decompress8(&jpeg, &mut dst, pitch, TjPixelFormat::Bgra)
            .expect("decode"),
        0
    );
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn ffi_wrapper_never_panics_on_mutated_jpegs() {
    let library = load_native_library();
    let mut decoder = library.create_decompressor().expect("decompressor");
    let jpeg = plain_64x48();
    let mut dst = vec![0u8; 64 * 4 * 48];

    // Accepted outcomes: a native return code, or the Rust-side InvalidArgument guard. Anything else fails the test.
    let accept = |result: Result<i32, NativeError>| match result {
        Ok(_) | Err(NativeError::InvalidArgument { .. }) => {}
        Err(other) => panic!("unexpected error on mutated input: {other:?}"),
    };

    for len in 0..jpeg.len() {
        let prefix = &jpeg[..len];
        accept(decoder.decompress_header(prefix));
        accept(decoder.decompress8(prefix, &mut dst, 64 * 4, TjPixelFormat::Bgrx));
    }

    let mut state: u64 = 0x9E37_79B9_7F4A_7C15; // xorshift64, fixed seed: the same 500 flips every run
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..500 {
        let mut mutated = jpeg.clone();
        let bit = (next() % (mutated.len() as u64 * 8)) as usize;
        mutated[bit / 8] ^= 1 << (bit % 8);
        accept(decoder.decompress_header(&mutated));
        accept(decoder.decompress8(&mutated, &mut dst, 64 * 4, TjPixelFormat::Bgrx));
    }
}

// Test support: the compressor init message is a distinct constant (C# asserts `tj3Init(0) != 0`); checked here so the
// constant cannot silently drift from the decompressor message.
#[test]
fn compressor_init_message_is_distinct_from_the_decompressor_message() {
    assert_ne!(COMPRESSOR_INIT_FAILED_MESSAGE, INIT_FAILED_MESSAGE);
    assert!(COMPRESSOR_INIT_FAILED_MESSAGE.contains("compressor"));
}

// ---- Rust-only: repair round 1 (verifier findings) ----

/// A lossless (`TJPARAM_LOSSLESS = 1`) BGRA JPEG of `width` x `height` from the pinned DLL.
fn lossless_jpeg(library: &Arc<TurboJpegLibrary>, width: i32, height: i32) -> Vec<u8> {
    let mut compressor = library.create_compressor().expect("compressor");
    assert_eq!(compressor.set(TjParam::Subsamp, 0).expect("set subsamp"), 0);
    assert_eq!(compressor.set(TjParam::Quality, 90).expect("set quality"), 0);
    assert_eq!(compressor.set(TjParam::Lossless, 1).expect("set lossless"), 0);
    let pitch = width * 4;
    let src: Vec<u8> = (0..(pitch * height) as usize).map(|i| (i * 7 % 251) as u8).collect();
    compressor
        .compress8(&src, width, pitch, height, TjPixelFormat::Bgra)
        .expect("compress lossless")
}

// Rust-only: libjpeg-turbo 3.0 ignores the scaling factor for a lossless JPEG although tj3SetScalingFactor returns 0,
// so the guard must bound the destination at FULL size (C# CalculateOutputBuffer has this hole; contract Semantics 8).
#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decompress8_bounds_a_lossless_jpeg_at_full_size_despite_a_remembered_scaling_factor() {
    let library = load_native_library();
    let (width, height) = (64i32, 48i32);
    let jpeg = lossless_jpeg(&library, width, height);
    let mut decoder = library.create_decompressor().expect("decompressor");
    assert_eq!(decoder.decompress_header(&jpeg).expect("header"), 0);
    assert_eq!(decoder.get(TjParam::Lossless).expect("lossless"), 1);
    assert_eq!(
        decoder.set_scaling_factor(TjScalingFactor::new(1, 8)).expect("scaling"),
        0
    );

    // The 1/8 size (8x6 BGRA) inside one larger allocation: the tail is a sentinel that must stay untouched.
    let small_pitch = 8 * 4;
    let small = (small_pitch * 6) as usize;
    let mut backing = vec![0xCDu8; (width * 4 * height) as usize + 4096];
    let result = {
        let (dst, _) = backing.split_at_mut(small);
        decoder.decompress8(&jpeg, dst, small_pitch, TjPixelFormat::Bgra)
    };
    assert!(
        matches!(result, Err(NativeError::InvalidArgument { .. })),
        "a 1/8-sized destination is refused for a lossless JPEG, got {result:?}"
    );
    assert!(
        backing[small..].iter().all(|&b| b == 0xCD),
        "nothing was written past the slice"
    );

    // The full size decodes (the remembered 1/8 is ignored by libjpeg-turbo and by the guard).
    let pitch = width * 4;
    let mut dst = vec![0u8; (pitch * height) as usize];
    assert_eq!(
        decoder
            .decompress8(&jpeg, &mut dst, pitch, TjPixelFormat::Bgra)
            .expect("decode"),
        0
    );
}

fn assert_compress_refused(result: Result<Vec<u8>, NativeError>, needle: &str) {
    match result {
        Err(NativeError::InvalidArgument { message }) => assert!(message.contains(needle), "{message}"),
        other => panic!("expected InvalidArgument containing {needle:?}, got {other:?}"),
    }
}

// Rust-only: compress8 refuses a zero or negative width, pitch or height, each one on its own.
#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn compress8_refuses_non_positive_sizes_one_at_a_time() {
    let library = load_native_library();
    let mut compressor = library.create_compressor().expect("compressor");
    assert_eq!(compressor.set(TjParam::Subsamp, 0).expect("set subsamp"), 0);
    assert_eq!(compressor.set(TjParam::Quality, 90).expect("set quality"), 0);
    let src = vec![0u8; 64 * 4 * 48];
    for (width, pitch, height) in [
        (0, 256, 48),
        (64, 0, 48),
        (64, 256, 0),
        (-1, 256, 48),
        (64, -4, 48),
        (64, 256, -1),
    ] {
        assert_compress_refused(
            compressor.compress8(&src, width, pitch, height, TjPixelFormat::Bgra),
            "must be positive",
        );
    }
    // The valid neighbour of those rows compresses.
    assert!(compressor.compress8(&src, 64, 256, 48, TjPixelFormat::Bgra).is_ok());
}

// Rust-only: a source pitch below width * pixel size is refused (tj3Compress8 itself would accept it).
#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn compress8_refuses_a_pitch_below_width_times_pixel_size() {
    let library = load_native_library();
    let mut compressor = library.create_compressor().expect("compressor");
    assert_eq!(compressor.set(TjParam::Subsamp, 0).expect("set subsamp"), 0);
    assert_eq!(compressor.set(TjParam::Quality, 90).expect("set quality"), 0);
    let src = vec![0u8; 16 * 3 * 8];
    // 16 RGB pixels = 48 bytes per row; 47 is one byte short (and larger than 16 + 3 and 16 / 3).
    assert_compress_refused(
        compressor.compress8(&src, 16, 47, 8, TjPixelFormat::Rgb),
        "smaller than one",
    );
    assert!(compressor.compress8(&src, 16, 48, 8, TjPixelFormat::Rgb).is_ok());
    assert!(compressor.compress8(&src, 16, 49, 7, TjPixelFormat::Rgb).is_ok());
}

// Rust-only: a source slice shorter than pitch * height is refused before tj3Compress8 reads past it.
#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn compress8_refuses_a_source_one_byte_short() {
    let library = load_native_library();
    let mut compressor = library.create_compressor().expect("compressor");
    assert_eq!(compressor.set(TjParam::Subsamp, 0).expect("set subsamp"), 0);
    assert_eq!(compressor.set(TjParam::Quality, 90).expect("set quality"), 0);
    // One allocation, so even a broken guard never reads outside the test's memory.
    let backing = vec![0x40u8; 16 * 3 * 8 * 2];
    let required = 16 * 3 * 8; // 384 (pitch + height = 56, pitch / height = 6)
    assert_compress_refused(
        compressor.compress8(&backing[..required - 1], 16, 48, 8, TjPixelFormat::Rgb),
        "too small",
    );
    assert!(
        compressor
            .compress8(&backing[..required], 16, 48, 8, TjPixelFormat::Rgb)
            .is_ok()
    );
}

// Rust-only: dropping the last Arc of a library unloads the module (FreeLibrary): a loaded DLL file cannot be
// deleted on Windows, an unloaded one can.
#[cfg(windows)]
#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn dropping_the_last_library_reference_unloads_the_module() {
    let source = native_dll_path();
    assert!(
        source.is_file(),
        "missing {}: run tools/fetch-native.ps1 first",
        source.display()
    );
    let root = TempRoot::new("turbo-native-unload");
    let copy = root.join(DLL_NAME);
    std::fs::copy(&source, &copy).expect("copy turbojpeg.dll");

    let library = TurboJpegLibrary::load(&copy).expect("load the copy");
    assert_eq!(library.path(), copy.as_path());
    assert!(std::fs::remove_file(&copy).is_err(), "a loaded module's file is locked");
    drop(library);
    std::fs::remove_file(&copy).expect("after the last Arc is dropped the module is unloaded");
}

// Rust-only: Display writes the message (what C# ex.Message is), for every variant.
#[test]
fn native_error_display_is_the_message() {
    let errors = [
        NativeError::DllNotFound {
            message: "dll".to_owned(),
        },
        NativeError::BadImageFormat {
            message: "image".to_owned(),
        },
        NativeError::EntryPointNotFound {
            message: "entry".to_owned(),
        },
        NativeError::InvalidOperation {
            message: "operation".to_owned(),
        },
        NativeError::Disposed {
            message: "disposed".to_owned(),
        },
        NativeError::InvalidArgument {
            message: "argument".to_owned(),
        },
    ];
    for error in errors {
        assert_eq!(error.to_string(), error.message());
        assert!(!error.to_string().is_empty());
    }
}

// Rust-only: the Debug output names the type and its state (never empty).
#[test]
fn handle_debug_shows_its_state() {
    let mut handle = TurboJpegHandle::from_raw(0x1234, false);
    handle.set_handle_as_invalid();
    let text = format!("{handle:?}");
    for needle in [
        "TurboJpegHandle",
        "4660",
        "owns: false",
        "closed: true",
        "destroy_seam: false",
        "scaling",
    ] {
        assert!(text.contains(needle), "{needle} missing from {text}");
    }
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn library_debug_shows_the_path_and_the_resolved_exports() {
    let library = load_native_library();
    let text = format!("{library:?}");
    for needle in ["TurboJpegLibrary", DLL_NAME, "tj3Init", "tj3Decompress8", "tj3Free"] {
        assert!(text.contains(needle), "{needle} missing from {text}");
    }
}
