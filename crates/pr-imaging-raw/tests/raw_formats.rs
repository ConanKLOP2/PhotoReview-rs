//! Red tests for W5-01a (`formats.rs`): the RAW vocabulary of `PhotoReview.Imaging.Raw` (C# baseline d92d6aae):
//! `RawFileTypes`, `RawContainerLimits`, `RawFormat`, `RawHeaderErrors.IsIoFailure` (through
//! `RawHeaderError::is_io_failure`, D45) and the golden `raw-formats` (kinds `extension`, `enum`, `limit`, `probe`,
//! `io-failure`). The stub bodies are `todo!()`, so the tests that call them fail until the porter fills them in.

use std::io;

use pr_imaging_raw::RawHeaderError;
use pr_imaging_raw::formats::{RAW_EXTENSIONS, RawFormat, is_raw_extension, limits};
use pr_test_support::golden::{self, i64_of, str_of};
use serde_json::Value;

// C#: RawFileTypes.IsRawExtension (the eight extensions; `.nrw` and `.pef` are NOT accepted)
#[test]
fn raw_file_types_contains_expected_extensions() {
    for ext in [".cr2", ".cr3", ".nef", ".arw", ".dng", ".raf", ".orf", ".rw2"] {
        let path = format!("test{ext}");
        assert!(is_raw_extension(Some(path.as_str())), "{ext} must be a RAW extension");
    }
    assert!(is_raw_extension(Some("test.CR3")));
    assert!(!is_raw_extension(Some("test.jpg")));
    assert!(!is_raw_extension(Some("test.nrw")));
    assert!(!is_raw_extension(Some("test.pef")));
}

// C#: RawFileTypes (the `s_extensions` set, lower case with dot). Complete in the stub: passes already.
#[test]
fn raw_extensions_constant_lists_the_eight_extensions() {
    assert_eq!(
        RAW_EXTENSIONS,
        [".cr2", ".cr3", ".nef", ".arw", ".dng", ".raf", ".orf", ".rw2"]
    );
}

// C#: RawContainerLimits.InitialProbeLength (internal)
#[test]
fn initial_probe_length_stays_bounded_for_large_sources() {
    assert_eq!(limits::initial_probe_length(i64::from(i32::MAX) + 1), 64);
    assert_eq!(limits::initial_probe_length(i64::MAX), 64);
    assert_eq!(limits::initial_probe_length(12), 12);
    assert_eq!(limits::initial_probe_length(0), 0);
}

// C#: RawContainerLimits_HasSpecifiedConstants. Complete in the stub: passes already.
#[test]
fn raw_container_limits_has_specified_constants() {
    assert_eq!(limits::MAX_IFD_COUNT, 64);
    assert_eq!(limits::MAX_ENTRIES_PER_IFD, 4096);
    assert_eq!(limits::MAX_PREVIEW_BYTES, 128 << 20);
    assert_eq!(limits::MAX_HEADER_BYTES, 8 << 20);
    assert_eq!(limits::DEFAULT_EXIF_BLOCK_BYTES, 128 * 1024);
    assert_eq!(limits::MAX_EXIF_BLOCK_BYTES, 4 << 20);
    assert_eq!(limits::MAX_JPEG_MARKER_SEGMENTS, 512);
    assert_eq!(limits::MAX_JPEG_FILL_BYTES, 64 * 1024);
}

/// Maps the C# constant name used in `raw-formats.json` (`kind: limit`) to the Rust constant.
fn limit_by_csharp_name(name: &str) -> i32 {
    match name {
        "MaxIfdCount" => limits::MAX_IFD_COUNT,
        "MaxEntriesPerIfd" => limits::MAX_ENTRIES_PER_IFD,
        "MaxPreviewBytes" => limits::MAX_PREVIEW_BYTES,
        "MaxHeaderBytes" => limits::MAX_HEADER_BYTES,
        "DefaultExifBlockBytes" => limits::DEFAULT_EXIF_BLOCK_BYTES,
        "MaxExifBlockBytes" => limits::MAX_EXIF_BLOCK_BYTES,
        "MaxJpegMarkerSegments" => limits::MAX_JPEG_MARKER_SEGMENTS,
        "MaxJpegFillBytes" => limits::MAX_JPEG_FILL_BYTES,
        other => panic!("unknown limit name in golden: {other}"),
    }
}

/// Maps the C# enum member name used in `raw-formats.json` (`kind: enum`) to the Rust variant.
fn format_by_csharp_name(name: &str) -> RawFormat {
    match name {
        "Unknown" => RawFormat::Unknown,
        "Cr2" => RawFormat::Cr2,
        "Cr3" => RawFormat::Cr3,
        "Nef" => RawFormat::Nef,
        "Nrw" => RawFormat::Nrw,
        "Arw" => RawFormat::Arw,
        "Dng" => RawFormat::Dng,
        "Raf" => RawFormat::Raf,
        "Orf" => RawFormat::Orf,
        "Rw2" => RawFormat::Rw2,
        "Pef" => RawFormat::Pef,
        other => panic!("unknown RawFormat name in golden: {other}"),
    }
}

/// Golden `raw-formats.json`, kind `extension` (90 cases): `RawFileTypes.IsRawExtension(path)` for every path,
/// including null, empty, whitespace-only, NUL, separators, UNC, alternate streams and Unicode case folds.
#[test]
fn golden_raw_formats_extension_cases() {
    let g = golden::load("raw-formats");
    let mut checked = 0;
    for case in g.cases.iter().filter(|c| str_of(c, "kind") == "extension") {
        // `path` is JSON null for the C# `null` case: `None` is the only "null" on the Rust side.
        let path: Option<&str> = case["path"].as_str();
        let expected = case["is_raw"].as_bool().unwrap_or_else(|| panic!("no is_raw: {case}"));
        assert_eq!(is_raw_extension(path), expected, "path {path:?}");
        checked += 1;
    }
    assert_eq!(checked, 90, "golden raw-formats must carry 90 extension cases");
}

/// Golden `raw-formats.json`, kind `enum` (11 cases): declaration order and numeric values of `RawFormat`.
#[test]
fn golden_raw_formats_enum_cases() {
    let g = golden::load("raw-formats");
    let cases: Vec<&Value> = g.cases.iter().filter(|c| str_of(c, "kind") == "enum").collect();
    let names: Vec<&str> = cases.iter().map(|c| str_of(c, "name")).collect();
    assert_eq!(
        names,
        [
            "Unknown", "Cr2", "Cr3", "Nef", "Nrw", "Arw", "Dng", "Raf", "Orf", "Rw2", "Pef"
        ],
        "golden must list exactly the 11 RawFormat members, in order"
    );
    for case in cases {
        let format = format_by_csharp_name(str_of(case, "name"));
        assert_eq!(i64::from(format as i32), i64_of(case, "value"), "{format:?}");
    }
    assert_eq!(RawFormat::default(), RawFormat::Unknown);
}

/// Golden `raw-formats.json`, kind `limit` (8 cases): every `RawContainerLimits` constant, by C# name.
#[test]
fn golden_raw_formats_limit_cases() {
    let g = golden::load("raw-formats");
    let cases: Vec<&Value> = g.cases.iter().filter(|c| str_of(c, "kind") == "limit").collect();
    let mut names: Vec<&str> = cases.iter().map(|c| str_of(c, "name")).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "DefaultExifBlockBytes",
            "MaxEntriesPerIfd",
            "MaxExifBlockBytes",
            "MaxHeaderBytes",
            "MaxIfdCount",
            "MaxJpegFillBytes",
            "MaxJpegMarkerSegments",
            "MaxPreviewBytes",
        ],
        "golden must carry exactly the 8 limit constants"
    );
    for case in cases {
        let value = limit_by_csharp_name(str_of(case, "name"));
        assert_eq!(i64::from(value), i64_of(case, "value"), "{}", str_of(case, "name"));
    }
}

/// Golden `raw-formats.json`, kind `probe` (17 cases): `RawContainerLimits.InitialProbeLength(length)`, including the
/// negative and overflow inputs whose unchecked `(int)` narrowing is pinned by the C# oracle.
#[test]
fn golden_raw_formats_probe_cases() {
    let g = golden::load("raw-formats");
    let cases: Vec<&Value> = g.cases.iter().filter(|c| str_of(c, "kind") == "probe").collect();
    assert_eq!(cases.len(), 17, "golden raw-formats must carry 17 probe cases");
    for case in cases {
        let length = i64_of(case, "length");
        let expected = i64_of(case, "result");
        assert_eq!(
            i64::from(limits::initial_probe_length(length)),
            expected,
            "length {length}"
        );
    }
}

/// Builds the error a C# reader would throw for an `InvalidDataException` whose `InnerException` is `inner`
/// (golden `kind: io-failure`). The Rust enum has no "other inner" variant (D45): a non-I/O inner is a plain
/// `InvalidData`, which `is_io_failure` must also reject.
fn error_with_inner(inner: &str) -> RawHeaderError {
    let io_kind = match inner {
        "none"
        | "InvalidDataException"
        | "ArgumentException"
        | "UnauthorizedAccessException"
        | "InvalidOperationException" => None,
        "IOException" | "ObjectDisposedException" => Some(io::ErrorKind::Other),
        "FileNotFoundException" => Some(io::ErrorKind::NotFound),
        "EndOfStreamException" => Some(io::ErrorKind::UnexpectedEof),
        other => panic!("unknown inner exception kind in golden: {other}"),
    };
    match io_kind {
        Some(kind) => RawHeaderError::invalid_data_io("raw header", io::Error::new(kind, inner)),
        None => RawHeaderError::invalid_data("raw header"),
    }
}

/// Golden `raw-formats.json`, kind `io-failure` (9 cases): `RawHeaderErrors.IsIoFailure(InvalidDataException)` by the
/// class of the inner exception.
#[test]
fn golden_raw_formats_io_failure_cases() {
    let g = golden::load("raw-formats");
    let cases: Vec<&Value> = g.cases.iter().filter(|c| str_of(c, "kind") == "io-failure").collect();
    assert_eq!(cases.len(), 9, "golden raw-formats must carry 9 io-failure cases");
    for case in cases {
        let inner = str_of(case, "inner");
        let expected = case["result"].as_bool().unwrap_or_else(|| panic!("no result: {case}"));
        assert_eq!(error_with_inner(inner).is_io_failure(), expected, "inner {inner}");
    }
}

// C#: RawHeaderErrors.IsIoFailure (only exercised through the readers' catch filters, see contract W5-01a)
#[test]
fn is_io_failure_true_for_invalid_data_with_an_io_error() {
    let error = RawHeaderError::invalid_data_io("disk gone", io::Error::other("disk gone"));
    assert!(error.is_io_failure());
}

// C#: RawHeaderErrors.IsIoFailure (a malformed or over-budget RAW is not a disk failure)
#[test]
fn is_io_failure_false_for_plain_invalid_data() {
    assert!(!RawHeaderError::invalid_data("malformed container").is_io_failure());
}

// C#: RawHeaderErrors.IsIoFailure (`ArgumentOutOfRangeException` is a caller bug, never an I/O failure)
#[test]
fn is_io_failure_false_for_argument_out_of_range() {
    let error = RawHeaderError::ArgumentOutOfRange {
        param: "offset",
        message: "negative".to_owned(),
    };
    assert!(!error.is_io_failure());
}

// C#: RawHeaderErrors.IsIoFailure (`OperationCanceledException` is never wrapped and never an I/O failure)
#[test]
fn is_io_failure_false_for_canceled() {
    assert!(!RawHeaderError::Canceled.is_io_failure());
}

// C#: RawHeaderErrors.IsIoFailure (the inner error is exposed through `Error::source` for the I/O case only)
#[test]
fn source_returns_the_io_error_only_for_the_io_variant() {
    use std::error::Error;
    let io_error = RawHeaderError::invalid_data_io("disk gone", io::Error::other("disk gone"));
    let source = io_error.source().expect("io variant exposes its inner error");
    assert!(source.downcast_ref::<io::Error>().is_some());

    assert!(RawHeaderError::invalid_data("malformed").source().is_none());
    assert!(RawHeaderError::Canceled.source().is_none());
}

// Hardening (contract W5-01a item 7): `is_raw_extension` is total, it never panics on any input.
#[test]
fn is_raw_extension_never_panics() {
    let long: String = "é中😀Ж".chars().cycle().take(4096).collect();
    assert!(!is_raw_extension(Some(long.as_str())));
    let _ = is_raw_extension(Some("a\0b.cr2"));
    assert!(!is_raw_extension(Some("\0")));
    assert!(!is_raw_extension(Some("..........")));
    assert!(!is_raw_extension(Some("")));
    assert!(!is_raw_extension(None));
}

// Golden cross-check (W5-01a test 6): `ImageFileTypes.IsRaw` (W1-07 golden) and `RawFileTypes` list the same 8
// extensions. A disagreeing case is an open question for the lead, not something to special-case here.
#[test]
fn raw_extensions_agree_with_image_file_types_golden() {
    let g = golden::load("image-file-types");
    let mut compared = 0;
    for case in &g.cases {
        let Some(path) = case["path"].as_str() else {
            continue;
        };
        let expected = case["is_raw"].as_bool().unwrap_or_else(|| panic!("no is_raw: {case}"));
        assert_eq!(is_raw_extension(Some(path)), expected, "path {path:?}");
        compared += 1;
    }
    assert!(compared > 0, "image-file-types golden has no non-null paths");
}
