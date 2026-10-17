//! Port tests for `pr_libraw::native` (C# `PhotoReview.Imaging.LibRaw.LibRawNativeMethods` and `SafeLibRawHandle`, baseline
//! d92d6aae), contract `contracts/W5-15a.md`.
//!
//! Red tests: the stub bodies are `todo!()`, so every test that reaches one fails until the W5-15a porter fills them in.
//! Tests that only check constants or layouts already complete in the stub (`ENTRY_POINTS`, the offsets, the struct layouts)
//! pass on the stub by design; they are guards, not behaviour.
//!
//! Native tests are `#[ignore = "native: ..."]`: they need `libraw.dll` 0.22.2 next to the test executable and, for the
//! corpus ones, the RAW corpus named by `PHOTOREVIEW_RS_RAW_CORPUS`. Without them they print a skip note and return
//! (panic only when `PHOTOREVIEW_RS_RAW_CORPUS_STRICT` is `1` or `true`), like C# `RawCorpus.RequireNative`.
//!
//! DIVERGENCE (documented, not tested): the C# `CancellationState_Dispose_IsIdempotent` test has no Rust twin. Rust has no
//! `GCHandle`; the boxed token is freed exactly once by its owner's drop.
//!
//! Not here (other owners): `RequiredExports_*`, `TryParseVersion_*`, `ProbeLoadedLibrary_*` (W5-15b, `tests/availability.rs`);
//! `Decode_*`, `Classify`/`EstimatePeakBytes` and the `LibRawDecoder` tests (W5-16); the `RgbBgraResampler` tests (W5-17).

use std::mem::{offset_of, size_of};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pr_core::sync::CancelToken;
use pr_libraw::availability;
use pr_libraw::native::{
    DATA_PROBE_LIMIT, DecoderInfo, IMAGE_JPEG, IPARAMS_CDESC_OFFSET, IPARAMS_COLORS_OFFSET, IPARAMS_FILTERS_OFFSET,
    IPARAMS_READ_LEN, LOAD_WITH_ALTERED_SEARCH_PATH, LibRawHandle, LibraryHandle, LoadError, NativeError, NativeOwner,
    PINNED_WHITE_BALANCE_LAYOUT, PROCESSED_IMAGE_DATA_OFFSET, ProcessedImageHeader, RawStructure, WhiteBalanceLayout,
    WhiteBalanceTarget, api, format_error, format_error_message, library_path_next_to, progress_verdict,
    try_read_raw_structure, try_set_use_camera_wb_core, version_string,
};

/// Real C API entry points (C# `LibRawNativeMethods.EntryPoints`), 23 names.
const CSHARP_ENTRY_POINT_COUNT: usize = 23;

/// `libraw_processed_image_t` / `libraw_iparams_t` blocks are built as byte vectors; the C# tests use 440 and 512.
fn iparams_block(len: usize, colors: i32, cdesc: &[u8], filters: i32) -> Vec<u8> {
    let mut block = vec![0u8; len];
    block[IPARAMS_COLORS_OFFSET..IPARAMS_COLORS_OFFSET + 4].copy_from_slice(&colors.to_ne_bytes());
    block[IPARAMS_FILTERS_OFFSET..IPARAMS_FILTERS_OFFSET + 4].copy_from_slice(&filters.to_ne_bytes());
    block[IPARAMS_CDESC_OFFSET..IPARAMS_CDESC_OFFSET + cdesc.len()].copy_from_slice(cdesc);
    block
}

/// The filters value of the C# tests (`0x94949494`, an RGGB-like CFA pattern) as the `i32` the block stores.
const BAYER_FILTERS: i32 = 0x9494_9494_u32 as i32;

/// Corpus file under `PHOTOREVIEW_RS_RAW_CORPUS`, when it exists.
fn corpus_file(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("PHOTOREVIEW_RS_RAW_CORPUS")?;
    let path = PathBuf::from(dir).join(name);
    path.is_file().then_some(path)
}

/// C# `RawCorpus.RequireNative` twin: `true` when the native test may run. Otherwise prints a skip note and returns `false`,
/// or panics when the strict flag is set.
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

/// Shared by the native tests: the probe's own answer, so a missing DLL is a skip and not a failure.
fn native_runtime_ready() -> bool {
    let outcome = availability::probe();
    require_native(outcome.available, outcome.reason.as_deref())
}

// ------------------------------------------------------------------------------------------------ constants and layouts

// C#: LibRawAvailabilityTests.EntryPoints_MatchTheLibraryImportDeclarations (WP-34). Rust twin: the `extern` field declarations
// of `LibRawApi` (between the BEGIN/END markers of `native.rs`) are exactly the `ENTRY_POINTS` names, sorted, no duplicates.
#[test]
fn entry_points_match_the_api_table_declarations() {
    let source = include_str!("../src/native.rs");
    let begin = source.find("// BEGIN ENTRY POINTS").expect("BEGIN ENTRY POINTS marker");
    let end = source.find("// END ENTRY POINTS").expect("END ENTRY POINTS marker");
    assert!(begin < end, "markers out of order");
    let section = &source[begin..end];

    let mut declared: Vec<&str> = section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let colon = trimmed.find(':')?;
            let ident = &trimmed[..colon];
            let is_entry = ident.starts_with("libraw_")
                && ident
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            is_entry.then_some(ident)
        })
        .collect();
    declared.sort_unstable();

    assert!(
        !declared.is_empty(),
        "no entry point declarations found between the markers"
    );
    let mut entries: Vec<&str> = native_entry_points().to_vec();
    entries.sort_unstable();
    assert_eq!(declared, entries, "ENTRY_POINTS must list exactly the declared fields");
    let mut deduplicated = entries.clone();
    deduplicated.dedup();
    assert_eq!(deduplicated.len(), entries.len(), "ENTRY_POINTS has duplicates");
    assert_eq!(entries.len(), CSHARP_ENTRY_POINT_COUNT);
}

fn native_entry_points() -> &'static [&'static str] {
    &pr_libraw::native::ENTRY_POINTS
}

// C#: LibRawAssembly_DefaultDllImportSearchPath_IsAssemblyDirectoryOnly (I1). Rust has no assembly attribute; the twin pins the
// loader rule instead (DIVERGENCE): the library is looked up next to the executable only, never by bare name.
#[test]
fn library_path_is_next_to_the_executable_never_a_bare_name() {
    assert_eq!(LOAD_WITH_ALTERED_SEARCH_PATH, 0x8);
    assert_eq!(library_path_next_to(Path::new("PhotoReviewRs.exe"), "libraw.dll"), None);
    assert_windows_exe_folder_join();

    // A relative path is refused without calling the loader, on every platform (`ERROR_INVALID_PARAMETER` = 87).
    match LibraryHandle::load_from(Path::new("libraw.dll")) {
        Err(LoadError::LoadFailed { os_error, .. }) => assert_eq!(os_error, 87),
        other => panic!("relative path must be refused with os_error 87, got {other:?}"),
    }
}

#[cfg(windows)]
fn assert_windows_exe_folder_join() {
    assert_eq!(
        library_path_next_to(Path::new(r"C:\app\PhotoReviewRs.exe"), "libraw.dll"),
        Some(PathBuf::from(r"C:\app\libraw.dll"))
    );
}

#[cfg(not(windows))]
fn assert_windows_exe_folder_join() {}

// C#: none (Rust-only layout guard, contract "New Rust tests").
#[test]
fn processed_image_header_layout_matches_libraw() {
    assert_eq!(size_of::<ProcessedImageHeader>(), 16);
    assert_eq!(ProcessedImageHeader::DATA_OFFSET, 16);
    assert_eq!(PROCESSED_IMAGE_DATA_OFFSET, 16);
    assert_eq!(offset_of!(ProcessedImageHeader, image_type), 0);
    assert_eq!(offset_of!(ProcessedImageHeader, height), 4);
    assert_eq!(offset_of!(ProcessedImageHeader, width), 6);
    assert_eq!(offset_of!(ProcessedImageHeader, colors), 8);
    assert_eq!(offset_of!(ProcessedImageHeader, bits), 10);
    assert_eq!(offset_of!(ProcessedImageHeader, data_size), 12);
}

// C#: none (Rust-only layout guard: `libraw_decoder_info_t` is a pointer, a uint and padding).
#[test]
fn decoder_info_layout() {
    assert_eq!(size_of::<DecoderInfo>(), 16);
    assert_eq!(offset_of!(DecoderInfo, flags), 8);
}

// C#: the `PinnedWhiteBalanceLayout` and `IParams*Offset` constants (LibRaw 0.22.2, x64 MSVC). Rust-only pin of the numbers.
#[test]
fn pinned_layout_values() {
    assert_eq!(
        PINNED_WHITE_BALANCE_LAYOUT,
        WhiteBalanceLayout {
            output_color: 5392,
            output_bps: 5432,
            no_auto_bright: 5488,
            use_camera_wb: 5384,
        }
    );
    assert_eq!(IPARAMS_COLORS_OFFSET, 340);
    assert_eq!(IPARAMS_FILTERS_OFFSET, 344);
    assert_eq!(IPARAMS_CDESC_OFFSET, 420);
    assert_eq!(IPARAMS_READ_LEN, 425);
    assert_eq!(DATA_PROBE_LIMIT, 65_536);
}

// ------------------------------------------------------------------------------------------------ raw structure reader

// C#: LibRawDecodeMemoryTests.TryReadRawStructure_PlausibleBlock_ReturnsColorsAndFilters (also asserts `IsBayer`).
#[test]
fn try_read_raw_structure_plausible_block_returns_colors_and_filters() {
    let block = iparams_block(440, 3, b"RGBG", BAYER_FILTERS);

    let structure = try_read_raw_structure(&block);

    assert_eq!(structure, Some(RawStructure::new(3, 0x9494_9494)));
    assert!(structure.expect("plausible block").is_bayer());
}

// C#: LibRawDecodeMemoryTests.TryReadRawStructure_ColorsOutOfRange_FailsClosed (colors 0, 7, -1).
#[test]
fn try_read_raw_structure_colors_out_of_range_fails_closed() {
    for colors in [0, 7, -1] {
        let block = iparams_block(440, colors, b"RGBG", BAYER_FILTERS);
        assert_eq!(try_read_raw_structure(&block), None, "colors {colors}");
    }
}

// C#: LibRawDecodeMemoryTests.TryReadRawStructure_CdescNotLetters_FailsClosedBecauseTheLayoutDoesNotMatch.
#[test]
fn try_read_raw_structure_cdesc_not_letters_fails_closed_because_the_layout_does_not_match() {
    let block = iparams_block(440, 3, &[0, b'G', b'B', b'G'], BAYER_FILTERS);
    assert_eq!(try_read_raw_structure(&block), None);
}

// C#: LibRawDecodeMemoryTests.TryReadRawStructure_NullPointer_IsNull (twin: the empty slice). A block shorter than
// IPARAMS_READ_LEN (424 bytes here) is also refused.
#[test]
fn try_read_raw_structure_null_pointer_is_none() {
    assert_eq!(try_read_raw_structure(&[]), None);
    let short = iparams_block(424, 3, b"RGB", BAYER_FILTERS);
    assert_eq!(try_read_raw_structure(&short), None);
}

// Rust-only edge: a block of exactly IPARAMS_READ_LEN (425) bytes, the length `raw_structure_with` passes, is read.
#[test]
fn try_read_raw_structure_block_of_exactly_the_read_length_is_read() {
    for cdesc in [&b"RGB"[..], &b"RGBG"[..]] {
        let block = iparams_block(IPARAMS_READ_LEN, 3, cdesc, BAYER_FILTERS);
        assert_eq!(
            try_read_raw_structure(&block),
            Some(RawStructure::new(3, 0x9494_9494)),
            "cdesc {cdesc:?}"
        );
    }
}

// C#: RgbBgraResamplerMutationGapTests.TryReadRawStructure_PlausibleColourDescription_ReturnsTheStructure (x4: G, AZ, RGB, RGBG).
#[test]
fn try_read_raw_structure_plausible_colour_description_returns_the_structure() {
    let cases: [(i32, &[u8]); 4] = [(1, b"G"), (2, b"AZ"), (3, b"RGB"), (4, b"RGBG")];
    for (colors, cdesc) in cases {
        let block = iparams_block(512, colors, cdesc, 0x9494_9494_u32 as i32);
        assert_eq!(
            try_read_raw_structure(&block),
            Some(RawStructure::new(colors, 0x9494_9494)),
            "colors {colors} cdesc {cdesc:?}"
        );
    }
}

// C#: RgbBgraResamplerMutationGapTests.TryReadRawStructure_ColourCountOutsideOneToFour_IsImplausible (x3: 0 "", 5 "RGBGE", -1 "R").
#[test]
fn try_read_raw_structure_colour_count_outside_one_to_four_is_implausible() {
    let cases: [(i32, &[u8]); 3] = [(0, b""), (5, b"RGBGE"), (-1, b"R")];
    for (colors, cdesc) in cases {
        let block = iparams_block(512, colors, cdesc, 1);
        assert_eq!(try_read_raw_structure(&block), None, "colors {colors}");
    }
}

// C#: RgbBgraResamplerMutationGapTests.TryReadRawStructure_ColourDescriptionThatIsNotCapitalLetters_IsImplausible (x4).
#[test]
fn try_read_raw_structure_colour_description_that_is_not_capital_letters_is_implausible() {
    let cases: [&[u8]; 4] = [b"RG\0", b"RGb", b"R@B", b"R[B"];
    for cdesc in cases {
        let block = iparams_block(512, 3, cdesc, 1);
        assert_eq!(try_read_raw_structure(&block), None, "cdesc {cdesc:?}");
    }
}

// C#: RgbBgraResamplerMutationGapTests.TryReadRawStructure_OnlyAsManyDescriptionBytesAsColoursAreChecked (colours 2, "RG" then
// a terminator byte that is not a letter: not part of the description).
#[test]
fn try_read_raw_structure_only_as_many_description_bytes_as_colours_are_checked() {
    let block = iparams_block(512, 2, b"RG", 1);
    assert!(try_read_raw_structure(&block).is_some());
}

// C#: RgbBgraResamplerMutationGapTests.TryReadRawStructure_NullPointer_IsImplausible (twin: the empty slice).
#[test]
fn try_read_raw_structure_null_pointer_is_implausible() {
    assert_eq!(try_read_raw_structure(&[]), None);
}

// ------------------------------------------------------------------------------------------------ white-balance layout proof

/// C# `LibRawManagedLogicTests.FakeLibRaw`: a zeroed block whose setters write at the REAL layout while the proof reads at the
/// layout it is given.
struct FakeLibRaw {
    block: Vec<u8>,
    real: WhiteBalanceLayout,
    calls: Vec<&'static str>,
}

impl FakeLibRaw {
    fn new(real: WhiteBalanceLayout) -> Self {
        Self {
            block: vec![0; 6000],
            real,
            calls: Vec::new(),
        }
    }

    fn snapshot(&self) -> (i32, i32, i32, i32) {
        (
            self.read_i32(self.real.output_color)
                .expect("real output_color inside the block"),
            self.read_i32(self.real.output_bps)
                .expect("real output_bps inside the block"),
            self.read_i32(self.real.no_auto_bright)
                .expect("real no_auto_bright inside the block"),
            self.read_i32(self.real.use_camera_wb)
                .expect("real use_camera_wb inside the block"),
        )
    }
}

impl WhiteBalanceTarget for FakeLibRaw {
    fn set_output_color(&mut self, value: i32) {
        self.calls.push("color");
        let offset = self.real.output_color;
        self.write_i32(offset, value);
    }

    fn set_output_bps(&mut self, value: i32) {
        self.calls.push("bps");
        let offset = self.real.output_bps;
        self.write_i32(offset, value);
    }

    fn set_no_auto_bright(&mut self, value: i32) {
        self.calls.push("bright");
        let offset = self.real.no_auto_bright;
        self.write_i32(offset, value);
    }

    fn read_i32(&self, offset: i32) -> Option<i32> {
        let start = usize::try_from(offset).ok()?;
        let bytes = self.block.get(start..start.checked_add(4)?)?;
        Some(i32::from_ne_bytes(bytes.try_into().ok()?))
    }

    fn write_i32(&mut self, offset: i32, value: i32) -> bool {
        let Ok(start) = usize::try_from(offset) else {
            return false;
        };
        let Some(end) = start.checked_add(4) else {
            return false;
        };
        match self.block.get_mut(start..end) {
            Some(bytes) => {
                bytes.copy_from_slice(&value.to_ne_bytes());
                true
            }
            None => false,
        }
    }
}

// C#: LibRawManagedLogicTests.TrySetUseCameraWb_LayoutMatches_SetsFlagAndLeavesTheRealOutputSettings.
#[test]
fn try_set_use_camera_wb_layout_matches_sets_flag_and_leaves_the_real_output_settings() {
    let layout = PINNED_WHITE_BALANCE_LAYOUT;
    let mut fake = FakeLibRaw::new(layout);

    let applied = try_set_use_camera_wb_core(&mut fake, 1, 8, 1, || true, layout);

    assert!(applied);
    // The sentinels are gone, the real settings and the flag are in.
    assert_eq!(fake.snapshot(), (1, 8, 1, 1));
}

// C#: LibRawManagedLogicTests.TrySetUseCameraWb_WrongOffset_FailsClosedWithoutWritingTheFlag (x4 shifts).
#[test]
fn try_set_use_camera_wb_wrong_offset_fails_closed_without_writing_the_flag() {
    let real = PINNED_WHITE_BALANCE_LAYOUT;
    let shifts: [(i32, i32, i32); 4] = [(4, 0, 0), (0, -4, 0), (0, 0, 8), (-8, -8, -8)];
    for (color_shift, bps_shift, bright_shift) in shifts {
        let mut fake = FakeLibRaw::new(real);
        let wrong = WhiteBalanceLayout {
            output_color: real.output_color + color_shift,
            output_bps: real.output_bps + bps_shift,
            no_auto_bright: real.no_auto_bright + bright_shift,
            use_camera_wb: real.use_camera_wb,
        };

        let applied = try_set_use_camera_wb_core(&mut fake, 1, 8, 1, || true, wrong);

        assert!(!applied, "shift {:?}", (color_shift, bps_shift, bright_shift));
        assert_eq!(fake.read_i32(real.use_camera_wb), Some(0), "flag written on a mismatch");
        // The output settings still reach LibRaw through the supported setters (daylight WB, sRGB, 8 bit).
        assert_eq!(fake.snapshot(), (1, 8, 1, 0));
    }
}

// C#: LibRawManagedLogicTests.TrySetUseCameraWb_VersionNotPinned_TouchesNoMemoryAndCallsNoSetter.
#[test]
fn try_set_use_camera_wb_version_not_pinned_touches_no_memory_and_calls_no_setter() {
    let layout = PINNED_WHITE_BALANCE_LAYOUT;
    let mut fake = FakeLibRaw::new(layout);
    let gate_calls = std::cell::Cell::new(0);

    let applied = try_set_use_camera_wb_core(
        &mut fake,
        1,
        8,
        1,
        || {
            gate_calls.set(gate_calls.get() + 1);
            false
        },
        layout,
    );

    assert!(!applied);
    assert_eq!(gate_calls.get(), 1, "the gate runs exactly once");
    assert!(fake.calls.is_empty(), "no setter call before the gate passed");
    assert_eq!(fake.snapshot(), (0, 0, 0, 0));
}

// C#: none (Rust-only window guard, semantics 4): a layout pointing beyond DATA_PROBE_LIMIT is a mismatch, and no flag is written.
#[test]
fn wb_target_out_of_window_is_a_mismatch() {
    let real = PINNED_WHITE_BALANCE_LAYOUT;
    let mut fake = FakeLibRaw::new(real);
    let wrong = WhiteBalanceLayout {
        output_color: DATA_PROBE_LIMIT,
        ..real
    };

    let applied = try_set_use_camera_wb_core(&mut fake, 1, 8, 1, || true, wrong);

    assert!(!applied);
    assert_eq!(
        fake.read_i32(real.use_camera_wb),
        Some(0),
        "no flag write on an out-of-window layout"
    );
}

// ------------------------------------------------------------------------------------------------ error text and version

// C#: LibRawSeamGapTests.FormatError (the `FormatError(IoError)` text rule). The message is returned unchanged.
#[test]
fn format_error_message_keeps_a_library_text_unchanged() {
    assert_eq!(format_error_message(5, Some("Out of memory")), "Out of memory");
}

// C#: LibRawSeamGapTests.FormatError: `IsNullOrWhiteSpace` falls back to the code text.
#[test]
fn format_error_message_blank_text_falls_back_to_the_code() {
    assert_eq!(format_error_message(5, None), "LibRaw error 5");
    assert_eq!(format_error_message(5, Some("")), "LibRaw error 5");
    assert_eq!(format_error_message(5, Some("  \t")), "LibRaw error 5");
}

// C#: LibRawSeamGapTests.FormatError with a negative LibRaw code.
#[test]
fn format_error_message_negative_code_in_the_fallback() {
    assert_eq!(format_error_message(-100_009, None), "LibRaw error -100009");
}

// C#: LibRawSeamGapTests.OpenFile_RetryAlsoFails_ReportsTheOriginalError uses `FormatError`; the version call is in W5-15a too.
#[test]
fn api_unavailable_degrades() {
    match api() {
        Ok(_) => {}
        Err(_) => {
            assert!(matches!(LibRawHandle::init(0), Err(NativeError::Unavailable(_))));
            assert_eq!(version_string(), None);
            assert_eq!(format_error(-1), "LibRaw error -1");
        }
    }
}

// ------------------------------------------------------------------------------------------------ progress verdict

// C#: LibRawSeamGapTests.CheckCancellation_ReportsTheTokenState (the token is shared with the handle's box through clones).
#[test]
fn check_cancellation_reports_the_token_state() {
    let token = CancelToken::new();
    assert_eq!(progress_verdict(Some(&token)), 0);
    token.clone().cancel();
    assert_eq!(progress_verdict(Some(&token)), 1);
}

// C#: LibRawSeamGapTests.CheckCancellation_UnrelatedTarget_DoesNotCancel_AndABadPointerDoes: only the bad-pointer half. The
// "unrelated GCHandle target" case cannot exist in Rust (the callback data is always the handle's own boxed token): DIVERGENCE.
#[test]
fn check_cancellation_unrelated_target_does_not_cancel_and_a_bad_pointer_does() {
    assert_eq!(progress_verdict(None), 1);
}

// ------------------------------------------------------------------------------------------------ handle ownership

fn recorder() -> (Arc<Mutex<Vec<usize>>>, impl FnOnce(usize) + Send + 'static) {
    let released = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&released);
    (released, move |raw: usize| {
        sink.lock().expect("recorder lock").push(raw)
    })
}

// C#: LibRawSeamGapTests.SafeLibRawHandle_Dispose_ClosesTheNativeHandleOnce (twin: NativeOwner with the test seam).
#[test]
fn safe_lib_raw_handle_dispose_closes_the_native_handle_once() {
    let (released, release) = recorder();
    let mut owner = NativeOwner::new(1234, release);

    owner.close();
    owner.close();
    assert!(owner.is_closed());
    drop(owner);

    assert_eq!(*released.lock().expect("recorder lock"), vec![1234]);
}

// C#: LibRawSeamGapTests.SafeLibRawImageHandle_Dispose_ClearsTheNativeImageOnce (the C# test does not assert `IsClosed`; asserting it
// here is an addition).
#[test]
fn safe_lib_raw_image_handle_dispose_clears_the_native_image_once() {
    let (cleared, clear) = recorder();
    let mut owner = NativeOwner::new(4321, clear);

    owner.close();
    owner.close();
    assert!(owner.is_closed());
    drop(owner);

    assert_eq!(*cleared.lock().expect("recorder lock"), vec![4321]);
}

// C#: LibRawSeamGapTests.SafeLibRawHandle_ZeroHandle_IsInvalidAndNeverClosed (+ the `-1` half of SafeHandleZeroOrMinusOneIsInvalid).
#[test]
fn safe_lib_raw_handle_zero_handle_is_invalid_and_never_closed() {
    for invalid in [0, usize::MAX] {
        let (released, release) = recorder();
        let mut owner = NativeOwner::new(invalid, release);

        assert!(owner.is_invalid(), "raw {invalid:#x}");
        owner.close();
        drop(owner);

        assert!(
            released.lock().expect("recorder lock").is_empty(),
            "raw {invalid:#x} was released"
        );
    }
}

// Rust addition (contract "New Rust tests"): dropping alone releases a valid handle exactly once.
#[test]
fn native_owner_drop_alone_releases_once() {
    let (released, release) = recorder();
    drop(NativeOwner::new(77, release));
    assert_eq!(*released.lock().expect("recorder lock"), vec![77]);
}

// ------------------------------------------------------------------------------------------------ native (ignored)

// C#: LibRawWhiteBalanceAndErrorTests.TrySetUseCameraWb_PinnedLayout_SetsFlagAndAppliesOutputSettings (Native).
#[test]
#[ignore = "native: needs libraw.dll 0.22.2 next to the test executable"]
fn try_set_use_camera_wb_pinned_layout_sets_flag_and_applies_output_settings() {
    if !native_runtime_ready() {
        return;
    }
    let mut handle = LibRawHandle::init(0).expect("libraw_init");
    let layout = PINNED_WHITE_BALANCE_LAYOUT;
    assert_eq!(handle.read_data_i32(layout.use_camera_wb), Some(0));

    assert!(handle.try_set_use_camera_wb(1, 8, 1));

    assert_eq!(handle.read_data_i32(layout.use_camera_wb), Some(1));
    // The sentinels used for the layout proof must be gone: the real settings are what LibRaw will use.
    assert_eq!(handle.read_data_i32(layout.output_color), Some(1));
    assert_eq!(handle.read_data_i32(layout.output_bps), Some(8));
    assert_eq!(handle.read_data_i32(layout.no_auto_bright), Some(1));
}

// C#: LibRawWhiteBalanceAndErrorTests.TrySetUseCameraWb_WrongOffset_FailsClosedWithoutWritingTheFlag (x4, Native).
#[test]
#[ignore = "native: needs libraw.dll 0.22.2 next to the test executable"]
fn try_set_use_camera_wb_wrong_offset_fails_closed_without_writing_the_flag_native() {
    if !native_runtime_ready() {
        return;
    }
    let real = PINNED_WHITE_BALANCE_LAYOUT;
    let shifts: [(i32, i32, i32); 4] = [(4, 0, 0), (0, -4, 0), (0, 0, 8), (-8, -8, -8)];
    for (color_shift, bps_shift, bright_shift) in shifts {
        let mut handle = LibRawHandle::init(0).expect("libraw_init");
        let wrong = WhiteBalanceLayout {
            output_color: real.output_color + color_shift,
            output_bps: real.output_bps + bps_shift,
            no_auto_bright: real.no_auto_bright + bright_shift,
            use_camera_wb: real.use_camera_wb,
        };

        assert!(!handle.try_set_use_camera_wb_with(1, 8, 1, || true, wrong));

        assert_eq!(handle.read_data_i32(real.use_camera_wb), Some(0));
        assert_eq!(handle.read_data_i32(real.output_color), Some(1));
        assert_eq!(handle.read_data_i32(real.output_bps), Some(8));
        assert_eq!(handle.read_data_i32(real.no_auto_bright), Some(1));
    }
}

// Native twin of `wb_target_out_of_window_is_a_mismatch`: a real handle, an out-of-window layout, no flag write.
#[test]
#[ignore = "native: needs libraw.dll 0.22.2 next to the test executable"]
fn wb_target_out_of_window_is_a_mismatch_native() {
    if !native_runtime_ready() {
        return;
    }
    let mut handle = LibRawHandle::init(0).expect("libraw_init");
    let wrong = WhiteBalanceLayout {
        output_color: DATA_PROBE_LIMIT,
        ..PINNED_WHITE_BALANCE_LAYOUT
    };

    assert!(!handle.try_set_use_camera_wb_with(1, 8, 1, || true, wrong));

    assert_eq!(handle.read_data_i32(PINNED_WHITE_BALANCE_LAYOUT.use_camera_wb), Some(0));
}

// Rust-only: the raw `i32` access window on a real handle is exactly `0 <= offset && offset + 4 <= DATA_PROBE_LIMIT`.
#[test]
#[ignore = "native: needs libraw.dll 0.22.2 next to the test executable"]
fn read_data_i32_window_edges_native() {
    if !native_runtime_ready() {
        return;
    }
    let handle = LibRawHandle::init(0).expect("libraw_init");

    assert!(handle.read_data_i32(0).is_some());
    assert!(handle.read_data_i32(DATA_PROBE_LIMIT - 4).is_some());
    for offset in [-1, DATA_PROBE_LIMIT - 3, DATA_PROBE_LIMIT, i32::MAX - 3, i32::MAX] {
        assert_eq!(handle.read_data_i32(offset), None, "offset {offset}");
    }
}

// C#: LibRawRawStructureNativeTests.TryGetRawStructure_MosaicCorpusFile_IsBayerWithThreeOrFourColors (x4 corpus files). The final
// `Classify(...) == Bayer` assertion moves to W5-16's native tests (it needs `DecodeMemoryGuard`).
#[test]
#[ignore = "native: needs libraw.dll and the RAW corpus"]
fn try_get_raw_structure_mosaic_corpus_file_is_bayer_with_three_or_four_colors() {
    if !native_runtime_ready() {
        return;
    }
    let files = [
        "Canon - EOS 350D - RAW (3_2).CR2",
        "Pentax - K-7 - 12bit (3_2).DNG",
        "Apple - iPhone 6s Plus - 16bit (4_3).DNG",
        "Fujifilm - X-T2 - 14bit 14bit uncompressed (3_2).RAF",
    ];
    for name in files {
        let Some(path) = corpus_file(name) else {
            eprintln!("skipped {name}: not in the corpus");
            continue;
        };
        let mut handle = LibRawHandle::init(0).expect("libraw_init");
        assert_eq!(handle.open_wfile(path.as_os_str()), 0, "{name}");

        let structure = handle
            .raw_structure()
            .unwrap_or_else(|| panic!("{name}: no raw structure"));

        assert!(
            structure.is_bayer(),
            "{name}: colors {}, filters 0x{:X}",
            structure.colors,
            structure.filters
        );
        assert!(
            (3..=4).contains(&structure.colors),
            "{name}: colors {}",
            structure.colors
        );
    }
}

// Native `open_buffer` + `dcraw_make_mem_thumb`: the JPEG thumbnail of a corpus CR2 starts with the JPEG SOI marker.
#[test]
#[ignore = "native: needs libraw.dll and the RAW corpus"]
fn open_buffer_and_make_mem_thumb_round_trip() {
    if !native_runtime_ready() {
        return;
    }
    let Some(path) = corpus_file("Canon - EOS 350D - RAW (3_2).CR2") else {
        eprintln!("skipped: corpus CR2 not present");
        return;
    };
    let bytes = std::fs::read(path).expect("read corpus CR2");
    let mut handle = LibRawHandle::init(0).expect("libraw_init");
    assert_eq!(handle.open_buffer(&bytes), 0);
    assert_eq!(handle.unpack_thumb(), 0);

    let mut image = handle.dcraw_make_mem_thumb().expect("dcraw_make_mem_thumb");

    assert_eq!(image.header().image_type, IMAGE_JPEG);
    assert_eq!(&image.data().expect("thumbnail data")[..2], &[0xFF, 0xD8]);
    handle.close();
    image.close();
    assert_eq!(image.data(), None);
    assert!(image.is_closed());
}

// Native `open_shared_buffer`: the handle keeps the buffer alive after the caller's Arc is dropped.
#[test]
#[ignore = "native: needs libraw.dll and the RAW corpus"]
fn open_shared_buffer_keeps_the_buffer_alive_after_the_callers_arc_is_dropped() {
    if !native_runtime_ready() {
        return;
    }
    let Some(path) = corpus_file("Canon - EOS 350D - RAW (3_2).CR2") else {
        eprintln!("skipped: corpus CR2 not present");
        return;
    };
    let bytes: Arc<[u8]> = Arc::from(std::fs::read(path).expect("read corpus CR2"));
    let mut handle = LibRawHandle::init(0).expect("libraw_init");
    assert_eq!(handle.open_shared_buffer(Arc::clone(&bytes)), 0);
    drop(bytes);

    assert_eq!(handle.unpack_thumb(), 0);
}

// C#: none (Rust-only, contract "New Rust tests"): a token cancelled before `unpack` aborts it through the progress callback. -100012
// is LIBRAW_CANCELLED_BY_CALLBACK; the same run proves the callback pointer is valid.
#[test]
#[ignore = "native: needs libraw.dll and the RAW corpus"]
fn cancelled_token_aborts_unpack() {
    if !native_runtime_ready() {
        return;
    }
    let Some(path) = corpus_file("Canon - EOS 350D - RAW (3_2).CR2") else {
        eprintln!("skipped: corpus CR2 not present");
        return;
    };
    let mut handle = LibRawHandle::init(0).expect("libraw_init");
    assert_eq!(handle.open_wfile(path.as_os_str()), 0);
    let token = CancelToken::new();
    handle.set_cancel_token(token.clone());
    token.cancel();

    let code = handle.unpack();

    assert_ne!(code, 0);
    assert_eq!(code, -100_012);
}

// Native `version_string`: the pinned runtime reports 0.22.2 (the `libraw.dll` the build ships).
#[test]
#[ignore = "native: needs libraw.dll next to the test executable"]
fn version_string_is_pinned() {
    if !native_runtime_ready() {
        return;
    }
    let version = version_string().expect("libraw_version returned a string");
    assert!(version.starts_with("0.22.2"), "version {version}");
}
