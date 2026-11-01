//! Synthetic-byte regressions for CL-MUT-RAW3; inline bytes only, no RAW corpus or golden files.

use pr_core::sync::CancelToken;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging_raw::decoder::{
    MAX_FALLBACK_THUMBNAIL_BYTES, is_corrupt_data_failure, is_corrupt_hresult, is_recoverable_preview_failure,
    is_resource_exhaustion, next_preview_candidates,
};
use pr_imaging_raw::readers::cr2::Cr2ContainerReader;
use pr_imaging_raw::{
    EmbeddedPreview, EmbeddedPreviewKind, InMemoryRawHeaderSource, PreviewColorSpace, RawContainerInfo,
    RawContainerReader,
};
use pr_test_support::raw::{TiffBytes, at, long, short};

const CR2_MARKER: &[u8; 8] = b"CR\x02\0\0\0\0\0";

fn read_cr2(data: Vec<u8>) -> RawContainerInfo {
    let mut source = InMemoryRawHeaderSource::new(data);
    Cr2ContainerReader::new()
        .read(&mut source, &CancelToken::new())
        .expect("synthetic CR2 parses")
}

// readers/cr2.rs:161:45, `*` -> `+` in `area = width * height` (C# Cr2ContainerReader largest-IFD fallback).
// IFD0 100x100 has the larger area (10_000 vs 1_000) but the smaller sum (200 vs 1_001).
#[test]
fn cr2_largest_ifd_is_chosen_by_area_not_by_width_plus_height() {
    let data = TiffBytes::new(true, 900)
        .header(16)
        .put(8, CR2_MARKER)
        .ifd(16, 200, &[short(0x0100, 100), short(0x0101, 100)])
        .ifd(200, 0, &[short(0x0100, 1000), short(0x0101, 1)])
        .into_vec();
    let info = read_cr2(data);
    assert_eq!((info.sensor_width, info.sensor_height), (100, 100));
}

// ---------------------------------------------------------------- decoder/fallback.rs pure helpers
// C# RawDecoder.cs: MaxFallbackThumbnailBytes, IsResourceExhaustion, IsCorruptHResult, IsRecoverablePreviewFailure,
// IsCorruptDataFailure, SelectNextPreview (pure half).

const OOM: u32 = 0x8007_000E;
const NOT_ENOUGH_MEMORY: u32 = 0x8007_0008;
const NO_SYSTEM_RESOURCES: u32 = 0x8007_05AA;
const COMMITMENT_LIMIT: u32 = 0x8007_05AF;
const BAD_IMAGE: u32 = 0x8898_2F60;
const BAD_HEADER: u32 = 0x8898_2F61;
const UNKNOWN_FORMAT: u32 = 0x8898_2F07;

fn hr(value: u32) -> i32 {
    i32::from_ne_bytes(value.to_ne_bytes())
}

fn com(value: u32) -> DecodeError {
    DecodeError::Com {
        hresult: hr(value),
        message: "com".to_owned(),
    }
}

fn not_supported_inner(value: u32) -> DecodeError {
    DecodeError::not_supported_with_inner_hresult("ns", hr(value))
}

fn not_supported_plain() -> DecodeError {
    DecodeError::NotSupported {
        message: "x".to_owned(),
        user_text: None,
        inner_hresult: None,
    }
}

fn invalid_data() -> DecodeError {
    DecodeError::InvalidData {
        message: "x".to_owned(),
    }
}

// fallback.rs:23:52, `<<` -> `>>`: C# `32 << 20`.
#[test]
fn max_fallback_thumbnail_bytes_is_thirty_two_mebibytes() {
    assert_eq!(MAX_FALLBACK_THUMBNAIL_BYTES, 33_554_432);
}

// fallback.rs:48:5 (both constant replacements) and the four HRESULT values.
#[test]
fn is_resource_exhaustion_matches_exactly_the_four_memory_hresults() {
    for value in [OOM, NOT_ENOUGH_MEMORY, NO_SYSTEM_RESOURCES, COMMITMENT_LIMIT] {
        assert!(is_resource_exhaustion(hr(value)), "{value:x}");
    }
    for value in [0, 1, BAD_IMAGE, BAD_HEADER, UNKNOWN_FORMAT, 0x8007_000F] {
        assert!(!is_resource_exhaustion(hr(value)), "{value:x}");
    }
}

// fallback.rs:56:5: exactly BADIMAGE, BADHEADER, UNKNOWNIMAGEFORMAT are corrupt.
#[test]
fn is_corrupt_hresult_matches_exactly_the_three_wic_hresults() {
    for value in [BAD_IMAGE, BAD_HEADER, UNKNOWN_FORMAT] {
        assert!(is_corrupt_hresult(hr(value)), "{value:x}");
    }
    for value in [0, 1, OOM, COMMITMENT_LIMIT, 0x8898_2F62] {
        assert!(!is_corrupt_hresult(hr(value)), "{value:x}");
    }
}

// fallback.rs:71-80: Com arm (with the `!`), the InvalidData/Format/NotSupported/MissingCodec arm, the default arm and
// the nested-COM-exhaustion guard.
#[test]
fn is_recoverable_preview_failure_follows_the_csharp_whitelist() {
    assert!(is_recoverable_preview_failure(&com(BAD_IMAGE)));
    assert!(is_recoverable_preview_failure(&com(0)));
    assert!(!is_recoverable_preview_failure(&com(OOM)));
    assert!(!is_recoverable_preview_failure(&com(COMMITMENT_LIMIT)));
    assert!(is_recoverable_preview_failure(&invalid_data()));
    assert!(is_recoverable_preview_failure(&DecodeError::Format {
        message: "x".to_owned()
    }));
    assert!(is_recoverable_preview_failure(&DecodeError::missing_codec()));
    assert!(is_recoverable_preview_failure(&not_supported_plain()));
    assert!(is_recoverable_preview_failure(&not_supported_inner(BAD_IMAGE)));
    assert!(!is_recoverable_preview_failure(&not_supported_inner(
        NO_SYSTEM_RESOURCES
    )));
    assert!(!is_recoverable_preview_failure(&not_supported_inner(OOM)));
    for error in [
        DecodeError::Canceled,
        DecodeError::OutOfMemory {
            message: "x".to_owned(),
        },
        DecodeError::FileNotFound {
            message: "x".to_owned(),
        },
        DecodeError::InvalidCast {
            message: "x".to_owned(),
        },
        DecodeError::Overflow {
            message: "x".to_owned(),
        },
        DecodeError::busy(),
        DecodeError::Io(std::io::Error::other("io")),
    ] {
        assert!(!is_recoverable_preview_failure(&error), "{error:?}");
    }
}

// fallback.rs:87-99: InvalidData|Format arm, Com arm, `||` with the nested COM HRESULT, inner_com_hresult not None.
#[test]
fn is_corrupt_data_failure_follows_the_csharp_rule() {
    assert!(is_corrupt_data_failure(&invalid_data()));
    assert!(is_corrupt_data_failure(&DecodeError::Format {
        message: "x".to_owned()
    }));
    assert!(is_corrupt_data_failure(&com(BAD_IMAGE)));
    assert!(is_corrupt_data_failure(&com(BAD_HEADER)));
    assert!(is_corrupt_data_failure(&com(UNKNOWN_FORMAT)));
    assert!(!is_corrupt_data_failure(&com(OOM)));
    assert!(!is_corrupt_data_failure(&com(0)));
    assert!(!is_corrupt_data_failure(&DecodeError::missing_codec()));
    assert!(!is_corrupt_data_failure(&DecodeError::Io(std::io::Error::other("io"))));
    assert!(!is_corrupt_data_failure(&not_supported_plain()));
    assert!(is_corrupt_data_failure(&not_supported_inner(BAD_IMAGE)));
    assert!(is_corrupt_data_failure(&not_supported_inner(UNKNOWN_FORMAT)));
    assert!(!is_corrupt_data_failure(&not_supported_inner(OOM)));
}

fn preview(offset: i64, length: i64, width: i32, height: i32) -> EmbeddedPreview {
    EmbeddedPreview::new(
        0,
        offset,
        length,
        EmbeddedPreviewKind::Jpeg,
        width,
        height,
        PreviewColorSpace::Srgb,
    )
}

fn offsets(result: Option<Vec<EmbeddedPreview>>) -> Option<Vec<i64>> {
    result.map(|list| list.iter().map(|p| p.offset).collect())
}

// fallback.rs:120-143 `next_preview_candidates`: remaining filter (length > 0, tried offsets, Jpeg kind) and the
// `largest_failed_long_side <= 0` shortcut.
#[test]
fn next_preview_candidates_remaining_filter_drops_empty_and_tried_previews() {
    assert_eq!(offsets(next_preview_candidates(&[], &[], 0, false)), None);
    let previews = [
        preview(10, 0, 4000, 3000),
        preview(20, -5, 4000, 3000),
        preview(30, 5, 4000, 3000),
        preview(40, 5, 4000, 3000),
    ];
    assert_eq!(
        offsets(next_preview_candidates(&previews, &[40], 0, false)),
        Some(vec![30])
    );
    assert_eq!(
        offsets(next_preview_candidates(&previews, &[], 0, false)),
        Some(vec![30, 40])
    );
    assert_eq!(offsets(next_preview_candidates(&previews, &[30, 40], 0, true)), None);
    // A non-positive failed long side keeps thumbnails too (no filtering).
    let thumbs = [preview(50, 5, 300, 500)];
    assert_eq!(offsets(next_preview_candidates(&thumbs, &[], 0, false)), Some(vec![50]));
    assert_eq!(
        offsets(next_preview_candidates(&thumbs, &[], -3, false)),
        Some(vec![50])
    );
}

// fallback.rs:104 (long_side) and 132-139: thumbnail test `0 < long < 1000`, `long >= failed`.
#[test]
fn next_preview_candidates_thumbnail_filter_uses_long_side_and_failed_long_side() {
    let previews = [
        preview(1, 5, 300, 500),  // long 500: thumbnail below the failed size
        preview(2, 5, 500, 300),  // long 500 (width is the long side)
        preview(3, 5, 0, 0),      // unknown size: never a thumbnail
        preview(4, 5, 1000, 10),  // long 1000: not a thumbnail (boundary)
        preview(5, 5, 1500, 900), // long 1500: not a thumbnail, below the failed size
        preview(6, 5, 3000, 2000),
    ];
    assert_eq!(
        offsets(next_preview_candidates(&previews, &[], 2000, false)),
        Some(vec![3, 4, 5, 6])
    );
    // A thumbnail that is at least as large as the failed one stays usable.
    let small = [preview(7, 5, 300, 500), preview(8, 5, 200, 100)];
    assert_eq!(offsets(next_preview_candidates(&small, &[], 400, false)), Some(vec![7]));
    // A 1 pixel long side is a thumbnail (long > 0), an unknown 0 is not.
    let tiny = [preview(9, 5, 1, 1), preview(10, 5, 0, 0)];
    assert_eq!(
        offsets(next_preview_candidates(&tiny, &[], 2000, false)),
        Some(vec![10])
    );
}

// ---------------------------------------------------------------- readers/arw.rs

fn read_arw(data: Vec<u8>) -> RawContainerInfo {
    use pr_imaging_raw::readers::arw::ArwContainerReader;
    let mut source = InMemoryRawHeaderSource::new(data);
    ArwContainerReader
        .read(&mut source, &CancelToken::new())
        .expect("synthetic ARW parses")
}

// arw.rs:155:41, `*` -> `+` in `area = width * height`: IFD0 100x100 (area 10_000, sum 200) must beat IFD1 1000x1
// (area 1_000, sum 1_001) as the sensor size.
#[test]
fn arw_largest_ifd_is_chosen_by_area_not_by_width_plus_height() {
    let data = TiffBytes::new(true, 600)
        .header(16)
        .ifd(16, 200, &[short(0x0100, 100), short(0x0101, 100)])
        .ifd(200, 0, &[short(0x0100, 1000), short(0x0101, 1)])
        .into_vec();
    let info = read_arw(data);
    assert_eq!((info.sensor_width, info.sensor_height), (100, 100));
}

// arw.rs `note_offset < MAKER_NOTE_MIN_OFFSET` (8) boundary: a MakerNote IFD at exactly offset 8 is parsed (the
// boundary is inclusive); the Sony PreviewImage entry must be listed. (The `<` -> `<=` / `==` mutants at 243:15 are
// `length < 4` in the same function, which is unreachable-redundant with `count <= 4`, so they are equivalent.)
#[test]
fn arw_maker_note_at_the_minimum_offset_is_parsed() {
    let mut jpeg = vec![0xFF, 0xD8];
    jpeg.resize(100, 0);
    let data = TiffBytes::new(true, 600)
        .header(100)
        .ifd(100, 0, &[long(0x8769, 200)])
        .ifd(200, 0, &[at(0x927C, 7, 100, 8)])
        .ifd(8, 0, &[at(0x2001, 7, 100, 300)])
        .put(300, &jpeg)
        .into_vec();
    let info = read_arw(data);
    assert_eq!(info.previews.len(), 1);
    assert_eq!((info.previews[0].offset, info.previews[0].length), (300, 100));
}

// fallback.rs:141-146: only thumbnails remain; corrupt data keeps them, other failures stop.
#[test]
fn next_preview_candidates_when_only_thumbnails_remain_depends_on_corrupt_data() {
    let thumbs = [preview(1, 5, 300, 500), preview(2, 5, 400, 200)];
    assert_eq!(offsets(next_preview_candidates(&thumbs, &[], 2000, false)), None);
    assert_eq!(
        offsets(next_preview_candidates(&thumbs, &[], 2000, true)),
        Some(vec![1, 2])
    );
}
