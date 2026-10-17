//! Contract tests for W5-01b (`contracts.rs`): `EmbeddedPreview`, `ExifBlock`, the header error, the header and reader traits and
//! `InMemoryRawHeaderSource` (C# baseline d92d6aae). Goldens: `raw-contract-records` (inputs in
//! `tests/golden-inputs/raw-contract-records.json`, the `a`/`b` argument lists the oracle used) and `raw-header-budget`.
//! The contract types and in-memory source are implemented; these tests pin their behavior and C#-compatible record shapes.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::sync::Arc;

use pr_core::sync::{CancelToken, Canceled};
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging_raw::formats::limits;
use pr_imaging_raw::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, InMemoryRawHeaderSource, PreviewColorSpace, RAW_HEADER_BLOCK_SIZE,
    RawContainerInfo, RawContainerReader, RawFormat, RawHeaderError, RawHeaderSource, RawPreviewFallback,
};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// `tests/golden-inputs/<name>.json` (the argument lists the C# oracle was run with).
fn golden_input(name: &str) -> Value {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn hash_of<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn i32_arg(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("integer argument")).expect("argument fits i32")
}

fn kind_from_name(name: &str) -> EmbeddedPreviewKind {
    match name {
        "Jpeg" => EmbeddedPreviewKind::Jpeg,
        other => panic!("unknown EmbeddedPreviewKind in input: {other}"),
    }
}

fn color_from_name(name: &str) -> PreviewColorSpace {
    match name {
        "Unknown" => PreviewColorSpace::Unknown,
        "Srgb" => PreviewColorSpace::Srgb,
        "AdobeRgb" => PreviewColorSpace::AdobeRgb,
        other => panic!("unknown PreviewColorSpace in input: {other}"),
    }
}

/// `EmbeddedPreview` from the oracle argument list `[index, offset, length, kind, width, height, colorSpace, headerResolved?]`.
fn preview_from_args(args: &[Value]) -> EmbeddedPreview {
    let preview = EmbeddedPreview::new(
        i32_arg(&args[0]),
        args[1].as_i64().expect("offset"),
        args[2].as_i64().expect("length"),
        kind_from_name(args[3].as_str().expect("kind")),
        i32_arg(&args[4]),
        i32_arg(&args[5]),
        color_from_name(args[6].as_str().expect("colour space")),
    );
    match args.get(7) {
        Some(flag) => EmbeddedPreview {
            header_resolved: flag.as_bool().expect("headerResolved"),
            ..preview
        },
        None => preview,
    }
}

/// `ExifBlock` from the oracle argument list `[offset, length, isTiffHeader, ifdIsExif?]`.
fn block_from_args(args: &[Value]) -> ExifBlock {
    let offset = args[0].as_i64().expect("offset");
    let length = args[1].as_i64().expect("length");
    let is_tiff_header = args[2].as_bool().expect("isTiffHeader");
    match args.get(3) {
        Some(flag) => ExifBlock::with_ifd_is_exif(offset, length, is_tiff_header, flag.as_bool().expect("ifdIsExif")),
        None => ExifBlock::new(offset, length, is_tiff_header),
    }
}

/// The input argument lists of the golden case `name` (`a` and `b`).
fn record_inputs(name: &str) -> (Vec<Value>, Vec<Value>) {
    let inputs = golden_input("raw-contract-records");
    let case = inputs["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no input for golden case {name}"));
    let list = |key: &str| case[key].as_array().expect("argument list").clone();
    (list("a"), list("b"))
}

// C#: RawContractsAndHeaderSourceTests.InMemoryRawHeaderSource_ReadsExactSlices
#[test]
fn in_memory_raw_header_source_reads_exact_slices() {
    let mut source = InMemoryRawHeaderSource::new(vec![1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(source.length(), 8);
    let span = source.read(2, 4).expect("read 4 bytes at offset 2");
    assert_eq!(span, [3, 4, 5, 6]);
}

// C#: SourceRawHeaderSourceMutationGapTests.Read_NegativeOffsetOrCount_IsRejectedByTheArgumentCheckItself
// (the file-backed twin is W5-12; the in-memory source has the identical argument checks).
#[test]
fn read_negative_offset_or_count_is_rejected_by_the_argument_check_itself() {
    for (offset, count) in [(-1_i64, 1_i32), (0, -1)] {
        let mut source = InMemoryRawHeaderSource::new(vec![0_u8; 1000]);
        match source.read(offset, count) {
            Err(RawHeaderError::ArgumentOutOfRange { param, .. }) => {
                assert_eq!(param, "offset", "offset {offset}, count {count}");
            }
            other => panic!("offset {offset}, count {count}: expected ArgumentOutOfRange, got {other:?}"),
        }
    }
}

// C#: SourceRawHeaderSourceMutationGapTests.Read_ZeroCountInsideTheStream_ReturnsAnEmptySpan
#[test]
fn read_zero_count_inside_the_stream_returns_an_empty_slice() {
    for offset in [10_i64, 1000] {
        let mut source = InMemoryRawHeaderSource::new(vec![0_u8; 1000]);
        let span = source.read(offset, 0).expect("zero-count read inside the source is Ok");
        assert!(span.is_empty(), "offset {offset}");
    }
}

// C#: RawReadInfoAndHeaderSourceParityTests.Read_MultiBlockReadThatExhaustsTheBudget_KeepsTheEarlierBlocksChargedLikeProduction
// (in-memory half; the production half is W5-12 against the same golden `raw-header-budget`).
#[test]
fn read_multi_block_read_that_exhausts_the_budget_keeps_the_earlier_blocks_charged() {
    let block = i64::try_from(RAW_HEADER_BLOCK_SIZE).expect("block size fits i64");
    let max = i64::from(limits::MAX_HEADER_BYTES);
    // 9 MiB of data: a read spanning 3 untouched blocks cannot fit after 126 blocks are charged.
    let mut source = InMemoryRawHeaderSource::new(vec![0_u8; usize::try_from(max + 2 * block).expect("fits usize")]);
    let blocks_before = (max / block) - 2;
    let offset = blocks_before * block;

    source
        .read(0, i32::try_from(offset).expect("fits i32"))
        .expect("charging 126 blocks is within the budget");
    let failed = source.read(offset, i32::try_from(3 * block).expect("fits i32"));
    assert!(
        matches!(failed, Err(RawHeaderError::InvalidData { .. })),
        "got {failed:?}"
    );
    assert_eq!(
        source.total_bytes_read(),
        max,
        "the two blocks charged before the failure stay charged"
    );

    // The blocks kept by the failed read are free to re-read.
    source
        .read(offset, i32::try_from(2 * block).expect("fits i32"))
        .expect("re-reading the kept blocks is free");
    assert_eq!(source.total_bytes_read(), max);
}

/// Error text per variant: `Display` is the C# `Exception.Message`.
// C#: InvalidDataException / ArgumentOutOfRangeException / OperationCanceledException messages (contract W5-01b Display)
#[test]
fn raw_header_error_display_texts_match_the_csharp_messages() {
    assert_eq!(RawHeaderError::invalid_data("bad header").to_string(), "bad header");
    assert_eq!(
        RawHeaderError::invalid_data_io("disk gone", io::Error::other("share lost")).to_string(),
        "disk gone"
    );
    let argument = RawHeaderError::ArgumentOutOfRange {
        param: "offset",
        message: "Specified argument was out of the range of valid values.".to_owned(),
    };
    assert_eq!(
        argument.to_string(),
        "Specified argument was out of the range of valid values. (Parameter 'offset')"
    );
    assert_eq!(RawHeaderError::Canceled.to_string(), "The operation was canceled.");
}

// C#: OperationCanceledException is never wrapped: `Canceled` converts straight from the cancel token's error.
#[test]
fn from_canceled_maps_to_the_canceled_variant() {
    let error: RawHeaderError = Canceled.into();
    assert!(matches!(error, RawHeaderError::Canceled));
}

// Golden `raw-contract-records.json` (type `enums`): declaration order and numeric values (C# enum values 0, 1, 2).
#[test]
fn golden_raw_contract_records_enum_values() {
    let g = golden::load("raw-contract-records");
    let case = g
        .cases
        .iter()
        .find(|c| str_of(c, "name") == "enums")
        .expect("enums case");
    let kinds = case["embedded_preview_kind"].as_object().expect("kinds");
    assert_eq!(kinds.len(), 1);
    assert_eq!(kinds["Jpeg"].as_i64(), Some(0));
    assert_eq!(EmbeddedPreviewKind::Jpeg as i64, 0);

    let spaces = case["preview_color_space"].as_object().expect("colour spaces");
    assert_eq!(spaces.len(), 3);
    assert_eq!(spaces["Unknown"].as_i64(), Some(0));
    assert_eq!(spaces["Srgb"].as_i64(), Some(1));
    assert_eq!(spaces["AdobeRgb"].as_i64(), Some(2));
    assert_eq!(PreviewColorSpace::Unknown as i64, 0);
    assert_eq!(PreviewColorSpace::Srgb as i64, 1);
    assert_eq!(PreviewColorSpace::AdobeRgb as i64, 2);
}

// Golden `raw-contract-records.json` (types `preview` and `block`): the final field values of `a`, value equality with
// `b`, hash equality with `b` and `a with { }` equal to `a`. The argument lists come from `golden-inputs`.
#[test]
fn golden_raw_contract_records_preview_and_block_cases() {
    let g = golden::load("raw-contract-records");
    let mut previews = 0;
    let mut blocks = 0;
    for case in &g.cases {
        match str_of(case, "type") {
            "enums" => {}
            "preview" => {
                let name = str_of(case, "name");
                let (a_args, b_args) = record_inputs(name);
                let a = preview_from_args(&a_args);
                let b = preview_from_args(&b_args);

                assert_eq!(a.index as i64, i64_of(case, "index"), "{name}: index");
                assert_eq!(a.offset, i64_of(case, "offset"), "{name}: offset");
                assert_eq!(a.length, i64_of(case, "length"), "{name}: length");
                assert_eq!(format!("{:?}", a.kind), str_of(case, "kind"), "{name}: kind");
                assert_eq!(a.width as i64, i64_of(case, "width"), "{name}: width");
                assert_eq!(a.height as i64, i64_of(case, "height"), "{name}: height");
                assert_eq!(
                    format!("{:?}", a.color_space),
                    str_of(case, "color_space"),
                    "{name}: colour space"
                );
                assert_eq!(
                    a.header_resolved,
                    bool_of(case, "header_resolved"),
                    "{name}: header_resolved"
                );

                assert_eq!(a == b, bool_of(case, "equals"), "{name}: equals");
                assert_eq!(
                    hash_of(&a) == hash_of(&b),
                    bool_of(case, "hash_equal"),
                    "{name}: hash_equal"
                );
                let copy = a.clone();
                assert_eq!(
                    copy == a,
                    bool_of(case, "with_copy_equals"),
                    "{name}: with-copy equality"
                );
                previews += 1;
            }
            "block" => {
                let name = str_of(case, "name");
                let (a_args, b_args) = record_inputs(name);
                let a = block_from_args(&a_args);
                let b = block_from_args(&b_args);

                assert_eq!(a.offset, i64_of(case, "offset"), "{name}: offset");
                assert_eq!(a.length, i64_of(case, "length"), "{name}: length");
                assert_eq!(
                    a.is_tiff_header,
                    bool_of(case, "is_tiff_header"),
                    "{name}: is_tiff_header"
                );
                assert_eq!(a.ifd_is_exif, bool_of(case, "ifd_is_exif"), "{name}: ifd_is_exif");

                assert_eq!(a == b, bool_of(case, "equals"), "{name}: equals");
                assert_eq!(
                    hash_of(&a) == hash_of(&b),
                    bool_of(case, "hash_equal"),
                    "{name}: hash_equal"
                );
                let copy = a; // `ExifBlock` is `Copy`: the copy is the with-copy of `a`
                assert_eq!(
                    copy == a,
                    bool_of(case, "with_copy_equals"),
                    "{name}: with-copy equality"
                );
                blocks += 1;
            }
            other => panic!("unknown golden case type {other}"),
        }
    }
    assert_eq!(previews, 6, "golden raw-contract-records must carry 6 preview cases");
    assert_eq!(blocks, 5, "golden raw-contract-records must carry 5 block cases");
}

// New Rust test (no C# twin): the record constructors fill the two C# default parameters (`HeaderResolved = false`,
// `IfdIsExif = false`), and a resolved copy differs from the default. Complete in the stub: passes already.
#[test]
fn embedded_preview_new_defaults_header_resolved_to_false() {
    let preview = EmbeddedPreview::new(
        0,
        100,
        2000,
        EmbeddedPreviewKind::Jpeg,
        640,
        480,
        PreviewColorSpace::Unknown,
    );
    assert!(!preview.header_resolved);
    let resolved = EmbeddedPreview {
        header_resolved: true,
        ..preview.clone()
    };
    assert_ne!(resolved, preview);
}

// New Rust test (no C# twin): `ExifBlock::new` has `ifd_is_exif == false`. Complete in the stub: passes already.
#[test]
fn exif_block_new_defaults_ifd_is_exif_to_false() {
    let block = ExifBlock::new(0, 131072, true);
    assert!(!block.ifd_is_exif);
    assert_eq!(block, ExifBlock::with_ifd_is_exif(0, 131072, true, false));
}

/// A `RawContainerReader` double: reads one header byte through the source, then reports a fixed container.
struct TestRawReader {
    format: RawFormat,
}

impl RawContainerReader for TestRawReader {
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        first_64_bytes.len() <= 64 && extension.eq_ignore_ascii_case(".cr2")
    }

    fn read(&self, source: &mut dyn RawHeaderSource, ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        ct.check()?;
        source.read(0, 1)?;
        Ok(RawContainerInfo {
            format: self.format,
            sensor_width: 4000,
            sensor_height: 3000,
            orientation: 1,
            previews: Vec::new(),
            exif_blocks: Vec::new(),
        })
    }
}

// New Rust test (no C# twin): the reader trait is object safe, usable as `Box<dyn RawContainerReader>`, and reads
// through `&mut dyn RawHeaderSource`. Relies on the in-memory source (the double reads through it).
#[test]
fn container_reader_double_is_object_safe_and_reads_through_a_dyn_header_source() {
    let reader: Box<dyn RawContainerReader> = Box::new(TestRawReader { format: RawFormat::Cr2 });
    let mut source = InMemoryRawHeaderSource::new(vec![0_u8; 64]);
    let token = CancelToken::new();
    let info = reader
        .read(&mut source, &token)
        .expect("the double reads one header byte");
    assert_eq!(info.format, RawFormat::Cr2);
    assert!(reader.can_read(&[0_u8; 64], ".CR2"));
}

// New Rust test (no C# twin): the reader and the fallback are `Send + Sync` (the registry and `RawDecoder` share them
// across threads). Compile-time check; passes once the stub compiles.
#[test]
fn reader_and_fallback_traits_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    assert_send_sync::<TestRawReader>();
    assert_send_sync::<dyn RawContainerReader>();
    assert_send_sync::<dyn RawPreviewFallback>();
}

/// A `RawPreviewFallback` double for a format it does not handle (C# `NotSupportedException`).
struct UnsupportedFallback;

impl RawPreviewFallback for UnsupportedFallback {
    fn read_jpeg_thumbnail(&self, path: &str, format: RawFormat) -> Result<Vec<u8>, DecodeError> {
        Err(DecodeError::NotSupported {
            message: format!("no thumbnail for {format:?} at {path}"),
            user_text: None,
            inner_hresult: None,
        })
    }
}

// New Rust test (no C# twin): the fallback is usable as `Arc<dyn RawPreviewFallback>` and reports an unsupported
// format as `DecodeError::NotSupported` (the whitelist of W4-12b keys on that variant).
#[test]
fn preview_fallback_double_is_usable_as_arc_dyn_and_reports_not_supported() {
    let fallback: Arc<dyn RawPreviewFallback> = Arc::new(UnsupportedFallback);
    let error = fallback
        .read_jpeg_thumbnail("photo.cr2", RawFormat::Cr2)
        .expect_err("the double refuses every format");
    assert!(error.is_not_supported());
}

/// Golden `raw-header-budget.json`: each case replays its `steps` against `InMemoryRawHeaderSource` over bytes
/// `i % 251`, comparing ok, len, first, last, sum, the error kind and parameter, and `total_bytes_read` after each step.
// C#: RawReadInfoAndHeaderSourceParityTests (budget semantics, in-memory side), SourceRawHeaderSourceMutationGapTests
#[test]
fn golden_raw_header_budget_cases() {
    let g = golden::load("raw-header-budget");
    assert_eq!(g.cases.len(), 12, "golden raw-header-budget must carry 12 cases");
    for case in &g.cases {
        let name = str_of(case, "name");
        let length = usize::try_from(i64_of(case, "length")).expect("length fits usize");
        let bytes: Vec<u8> = (0..length).map(|i| u8::try_from(i % 251).expect("below 251")).collect();
        let mut source = InMemoryRawHeaderSource::new(bytes);

        for (index, step) in case["steps"].as_array().expect("steps").iter().enumerate() {
            let at = format!("{name} step {index}");
            let offset = i64_of(step, "offset");
            let count = i32::try_from(i64_of(step, "count")).expect("count fits i32");
            // Owned summary of the read so no borrow of `source` outlives the call.
            let outcome = source
                .read(offset, count)
                .map(|slice| {
                    (
                        slice.len(),
                        slice.first().map(|b| i64::from(*b)),
                        slice.last().map(|b| i64::from(*b)),
                        slice.iter().map(|b| i64::from(*b)).sum::<i64>(),
                    )
                })
                .map_err(|error| match error {
                    RawHeaderError::InvalidData { .. } => ("InvalidDataException".to_owned(), None),
                    RawHeaderError::ArgumentOutOfRange { param, .. } => {
                        ("ArgumentOutOfRangeException".to_owned(), Some(param.to_owned()))
                    }
                    RawHeaderError::Canceled => ("OperationCanceledException".to_owned(), None),
                });

            assert_eq!(
                source.total_bytes_read(),
                i64_of(step, "total_bytes_read"),
                "{at}: total_bytes_read"
            );
            if bool_of(step, "ok") {
                let (len, first, last, sum) = outcome.unwrap_or_else(|e| panic!("{at}: expected Ok, got {e:?}"));
                assert_eq!(len as i64, i64_of(step, "len"), "{at}: len");
                assert_eq!(first, step["first"].as_i64(), "{at}: first");
                assert_eq!(last, step["last"].as_i64(), "{at}: last");
                assert_eq!(sum, i64_of(step, "sum"), "{at}: sum");
            } else {
                let (kind, param) = outcome.expect_err("expected an error step");
                assert_eq!(kind, str_of(step, "error"), "{at}: error kind");
                assert_eq!(param.as_deref(), step["param"].as_str(), "{at}: param");
            }
        }
    }
}
