//! Red tests for W5-02a (`tiff_nav.rs`): `TiffHeaderNavigator` (C# baseline d92d6aae).
//!
//! Ported from `tests/PhotoReview.Imaging.Tests/Raw/Tiff/TiffHeaderNavigatorMutationGapTests.cs` (every row),
//! the TIFF section of `Raw/Tiff/TiffReaderRegressionTests.cs`, `Raw/RawErrorHandlingReviewTests.cs` (2 rows),
//! `Raw/Tiff/TiffPreviewHardeningTests.cs` (1), `Raw/Tiff/TiffSensorSizeTests.cs` (1), the navigator fuzz of
//! `Robustness/BinaryReaderFuzzTests.cs` and the golden `tests/golden/tiff-navigator.json` (inputs in
//! `tests/golden-inputs/tiff-navigator.json`). Shared byte builders: `pr_test_support::raw` (D48).
//!
//! Stub bodies are `todo!()`: every test that reaches the navigator fails until the porter fills it in. The round-trip
//! test only uses builders and the already-implemented `pr_imaging::tiff::try_read_header`, so it is green by design.

use std::io;
use std::panic::AssertUnwindSafe;

use pr_imaging::tiff::try_read_header;
use pr_imaging_raw::formats::limits;
use pr_imaging_raw::tiff_nav::{
    DEFAULT_ARRAY_ITEMS, IfdRead, TiffEntry, choose_active_sensor_size, clamp_to_int, compute_exif_block,
    has_square_default_scale, is_range_in_file, read_ifd_entries, read_image_size, read_tag_unsigned,
    read_tag_unsigned_array, read_tag_value, reconcile_jpeg_size, starts_with_soi, try_get_entry,
    try_read_default_crop_size, try_read_exif_pixel_dimensions, try_read_jpeg_interchange, try_read_single_strip,
};
use pr_imaging_raw::{ExifBlock, InMemoryRawHeaderSource, RawHeaderError, RawHeaderSource};
use pr_test_support::golden;
use pr_test_support::raw::{
    Case, TiffBytes, XorShift64, at, build_iso_bmff, build_raf, build_tiff, create_minimal_jpeg, golden_bytes, long,
    mutants, short,
};
use serde_json::{Map, Value, json};

/// Byte order of most tests.
const LE: bool = true;

// ---------------------------------------------------------------- helpers

fn src(bytes: Vec<u8>) -> InMemoryRawHeaderSource {
    InMemoryRawHeaderSource::new(bytes)
}

fn len_i64(bytes: &[u8]) -> i64 {
    i64::try_from(bytes.len()).expect("length fits i64")
}

/// C# `TiffHeaderNavigator.TiffEntry(tag, type, count, valueOrOffset)`.
fn te(tag: u16, field_type: u16, count: u32, value_or_offset: u32) -> TiffEntry {
    TiffEntry {
        tag,
        field_type,
        count,
        value_or_offset,
    }
}

/// C# `E(type, count, valueOrOffset)` of the navigator tests: tag 0x0100.
fn tag_entry(field_type: u16, count: u32, value_or_offset: u32) -> TiffEntry {
    te(0x0100, field_type, count, value_or_offset)
}

/// Little-endian 12-byte IFD entry (C# `Entry(tag, type, count, value)` of the mutation-gap tests).
fn entry_bytes(tag: u16, field_type: u16, count: u32, value: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&tag.to_le_bytes());
    bytes.extend_from_slice(&field_type.to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// `read_ifd_entries` on a little-endian source (test shorthand).
fn ifd_read(data: Vec<u8>, offset: i64) -> IfdRead {
    read_ifd_entries(&mut src(data), offset, LE).expect("source read")
}

/// `read_tag_unsigned` on a little-endian source (test shorthand).
fn tag_unsigned(data: Vec<u8>, entry: &TiffEntry) -> Option<i64> {
    read_tag_unsigned(&mut src(data), entry, LE).expect("source read")
}

/// `read_tag_unsigned_array` with the default item cap (test shorthand).
fn tag_array(data: Vec<u8>, entry: &TiffEntry) -> Vec<i64> {
    read_tag_unsigned_array(&mut src(data), entry, LE, DEFAULT_ARRAY_ITEMS).expect("source read")
}

/// `compute_exif_block` for an IFD0 at 8 (test shorthand).
fn exif_block_of(data: Vec<u8>) -> ExifBlock {
    compute_exif_block(&mut src(data), LE, 8).expect("non-I/O failures keep the default block")
}

/// Crop test file: 64 zero bytes with two LE u32 values at 16 and 20, and the DefaultCropSize entry at 16 (C# `CropFile`).
fn crop_file(count: u32, first: u32, second: u32) -> (Vec<u8>, Vec<TiffEntry>) {
    let mut data = vec![0u8; 64];
    data[16..20].copy_from_slice(&first.to_le_bytes());
    data[20..24].copy_from_slice(&second.to_le_bytes());
    (data, vec![te(0xC620, 4, count, 16)])
}

/// DefaultScale test file (C# `ScaleFile`): `ratio` = hn, hd, vn, vd written at `offset` when they fit.
fn scale_file(ratio: [u32; 4], field_type: u16, count: u32, offset: u32, length: usize) -> (Vec<u8>, Vec<TiffEntry>) {
    let mut data = vec![0u8; length];
    let at_offset = offset as usize;
    if at_offset + 16 <= length {
        for (i, value) in ratio.iter().enumerate() {
            data[at_offset + 4 * i..at_offset + 4 * i + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    (data, vec![te(0xC61E, field_type, count, offset)])
}

/// Source whose every read fails (C# `FailingSource`, length 1_000_000).
#[derive(Debug, Clone, Copy)]
enum FailureKind {
    /// `InvalidData` with an I/O inner error (disk failure).
    Io,
    /// `InvalidData` without an inner error (budget exhausted).
    Budget,
}

#[derive(Debug)]
struct FailingSource(FailureKind);

impl RawHeaderSource for FailingSource {
    fn length(&self) -> i64 {
        1_000_000
    }

    fn read(&mut self, _offset: i64, _count: i32) -> Result<&[u8], RawHeaderError> {
        Err(match self.0 {
            FailureKind::Io => RawHeaderError::invalid_data_io("read failed", io::Error::other("disk")),
            FailureKind::Budget => RawHeaderError::invalid_data("budget exhausted"),
        })
    }
}

// ---------------------------------------------------------------- ReadIfdEntries (mutation gap)

// C#: TiffHeaderNavigatorMutationGapTests.ReadIfdEntries_OffsetInsideTheHeader_ReturnsNothing
#[test]
fn read_ifd_entries_offset_inside_the_header_returns_nothing() {
    // Bytes 4..7 hold IFD0's offset (8): read as a count they would announce eight entries.
    let data = TiffBytes::new(LE, 200)
        .header(8)
        .ifd(8, 0, &[short(0x0100, 1)])
        .into_vec();

    let read = ifd_read(data, 4);

    assert!(read.entries.is_empty());
    assert_eq!(read.next_ifd_offset, 0);
}

// C#: TiffHeaderNavigatorMutationGapTests.ReadIfdEntries_OffsetOneByteBeforeEndOfSource_ReturnsNothingWithoutThrowing
#[test]
fn read_ifd_entries_offset_one_byte_before_end_of_source_returns_nothing_without_throwing() {
    let read = ifd_read(vec![0u8; 64], 63);

    assert!(read.entries.is_empty());
}

// C#: TiffHeaderNavigatorMutationGapTests.ReadIfdEntries_CountLargerThanTheBytesThatExist_ReturnsOnlyTheWholeEntriesThatFit
#[test]
fn read_ifd_entries_count_larger_than_the_bytes_that_exist_returns_only_the_whole_entries_that_fit() {
    // Count 10 but only 3 whole entries (+5 bytes) fit after the count.
    let data = TiffBytes::new(LE, 8 + 2 + (3 * 12) + 5)
        .header(8)
        .put(10, &entry_bytes(0x0100, 3, 1, 1))
        .put(22, &entry_bytes(0x0101, 3, 1, 2))
        .put(34, &entry_bytes(0x0102, 3, 1, 3))
        .u16(8, 10)
        .into_vec();

    let read = ifd_read(data, 8);

    let tags: Vec<u16> = read.entries.iter().map(|e| e.tag).collect();
    assert_eq!(tags, vec![0x0100, 0x0101, 0x0102]);
    assert_eq!(read.next_ifd_offset, 0);
}

// C#: TiffHeaderNavigatorMutationGapTests.ReadIfdEntries_CountOfFourThousandNinetySix_StillReadsTheNextIfdPointer
#[test]
fn read_ifd_entries_count_of_four_thousand_ninety_six_still_reads_the_next_ifd_pointer() {
    let entries = vec![short(0x0100, 1); 4096];
    let data = TiffBytes::new(LE, 8 + 2 + (4096 * 12) + 4)
        .header(8)
        .ifd(8, 8, &entries)
        .into_vec();

    let read = ifd_read(data, 8);

    assert_eq!(read.entries.len(), 4096);
    assert_eq!(read.next_ifd_offset, 8);
}

// C#: TiffHeaderNavigatorMutationGapTests.ReadIfdEntries_CountAboveTheLimit_ReadsTheLimitAndEndsTheChain
#[test]
fn read_ifd_entries_count_above_the_limit_reads_the_limit_and_ends_the_chain() {
    let entries = vec![short(0x0100, 1); 4096];
    let data = TiffBytes::new(LE, 8 + 2 + (4096 * 12) + 64)
        .header(8)
        .ifd(8, 8, &entries)
        .u16(8, 4097)
        .into_vec();

    let read = ifd_read(data, 8);

    assert_eq!(read.entries.len(), 4096);
    assert_eq!(read.next_ifd_offset, 0);
}

// C#: TiffHeaderNavigatorMutationGapTests.ReadIfdEntries_NextIfdPointer_MustLandAfterTheHeaderWithRoomForAnEmptyIfd
// Theory rows: (8, 8), (20, 20) = the largest accepted pointer (file length 26 - 6), (21, 0), (7, 0), (0, 0).
#[test]
fn read_ifd_entries_next_ifd_pointer_must_land_after_the_header_with_room_for_an_empty_ifd() {
    for (next_pointer, expected) in [(8u32, 8u32), (20, 20), (21, 0), (7, 0), (0, 0)] {
        let data = TiffBytes::new(LE, 26)
            .header(8)
            .ifd(8, next_pointer, &[short(0x0100, 1)])
            .into_vec();

        let read = ifd_read(data, 8);

        assert_eq!(read.next_ifd_offset, expected, "row next pointer {next_pointer}");
    }
}

// C#: TiffHeaderNavigatorMutationGapTests.ReadIfdEntries_NextPointerFieldEndingExactlyAtEndOfSource_IsRead
#[test]
fn read_ifd_entries_next_pointer_field_ending_exactly_at_end_of_source_is_read() {
    let data = TiffBytes::new(LE, 26)
        .header(8)
        .ifd(8, 12, &[short(0x0100, 1)])
        .into_vec();

    let read = ifd_read(data, 8);

    assert_eq!(read.next_ifd_offset, 12);
}

// C#: TiffHeaderNavigatorMutationGapTests.ReadIfdEntries_NoRoomForTheNextPointer_ReturnsEntriesWithoutThrowing
#[test]
fn read_ifd_entries_no_room_for_the_next_pointer_returns_entries_without_throwing() {
    // One entry, then only two bytes of file: the 4-byte pointer does not exist.
    let data = TiffBytes::new(LE, 24)
        .header(8)
        .u16(8, 1)
        .put(10, &entry_bytes(0x0100, 3, 1, 7))
        .into_vec();

    let read = ifd_read(data, 8);

    assert_eq!(read.entries.len(), 1);
    assert_eq!(read.next_ifd_offset, 0);
}

// C#: RawHeaderHostileInputTests / Robustness rows of the task: 0-, 1- and 9-byte sources read nothing and never throw.
#[test]
fn read_ifd_entries_on_short_sources_returns_nothing() {
    for length in [0usize, 1, 9] {
        let read = ifd_read(vec![0u8; length], 8);

        assert!(read.entries.is_empty(), "length {length}");
        assert_eq!(read.next_ifd_offset, 0, "length {length}");
    }
}

// ---------------------------------------------------------------- ReadTagUnsigned / ReadTagUnsignedArray (mutation gap)

// C#: TiffHeaderNavigatorMutationGapTests.ReadTagUnsigned_UnknownTypeOrZeroCount_IsNull (rows: unknown type 0 count 1; type 3 count 0)
#[test]
fn read_tag_unsigned_unknown_type_or_zero_count_is_none() {
    assert_eq!(tag_unsigned(vec![0u8; 32], &tag_entry(0, 1, 6)), None, "unknown type");
    assert_eq!(tag_unsigned(vec![0u8; 32], &tag_entry(3, 0, 6)), None, "zero count");
}

// C#: TiffHeaderNavigatorMutationGapTests.ReadTagUnsigned_OutOfLineValue_MustLieFullyInsideTheSource
// Rows: offset 28 (last four bytes, legal), 29 (one past the end), 100 (far beyond the end).
#[test]
fn read_tag_unsigned_out_of_line_value_must_lie_fully_inside_the_source() {
    let mut data = vec![0u8; 32];
    data[28..32].copy_from_slice(&0xCAFE_BABEu32.to_le_bytes());

    assert_eq!(
        tag_unsigned(data.clone(), &tag_entry(4, 2, 28)),
        Some(0xCAFE_BABE),
        "row offset 28"
    );
    assert_eq!(tag_unsigned(data.clone(), &tag_entry(4, 2, 29)), None, "row offset 29");
    assert_eq!(tag_unsigned(data, &tag_entry(4, 2, 100)), None, "row offset 100");
}

// C#: TiffHeaderNavigatorMutationGapTests.ReadTagUnsigned_OutOfLineValueAtOffsetZero_ReadsTheFirstBytesOfTheSource
#[test]
fn read_tag_unsigned_out_of_line_value_at_offset_zero_reads_the_first_bytes_of_the_source() {
    let data = TiffBytes::new(LE, 32).header(8).into_vec();

    assert_eq!(tag_unsigned(data, &tag_entry(4, 2, 0)), Some(0x002A_4949));
}

// C#: TiffHeaderNavigatorMutationGapTests.ReadTagUnsignedArray_OutOfLineArray_MustLieFullyInsideTheSource
#[test]
fn read_tag_unsigned_array_out_of_line_array_must_lie_fully_inside_the_source() {
    let mut data = vec![0u8; 32];
    data[24..28].copy_from_slice(&11u32.to_le_bytes());
    data[28..32].copy_from_slice(&22u32.to_le_bytes());

    assert_eq!(tag_array(data.clone(), &tag_entry(4, 2, 24)), vec![11, 22]);
    assert!(
        tag_array(data.clone(), &tag_entry(4, 2, 25)).is_empty(),
        "one byte too far"
    );
    assert!(
        tag_array(data, &tag_entry(4, 3, 24)).is_empty(),
        "third item runs past the end"
    );
}

// C#: TiffHeaderNavigatorMutationGapTests.ReadTagUnsignedArray_OutOfLineArrayAtOffsetZero_ReadsTheFirstBytesOfTheSource
#[test]
fn read_tag_unsigned_array_out_of_line_array_at_offset_zero_reads_the_first_bytes_of_the_source() {
    let data = TiffBytes::new(LE, 32).header(8).into_vec();

    assert_eq!(tag_array(data, &tag_entry(4, 2, 0)), vec![0x002A_4949, 8]);
}

// ---------------------------------------------------------------- IsRangeInFile / strips / exif pixel size (mutation gap)

// C#: TiffHeaderNavigatorMutationGapTests.IsRangeInFile_Boundaries (6 rows)
#[test]
fn is_range_in_file_boundaries() {
    let rows: [(i64, i64, i64, bool); 6] = [
        (10, 90, 100, true), // ends exactly at the end
        (10, 91, 100, false),
        (0, 10, 100, false), // offset 0 is never a payload start
        (10, 0, 100, false),
        (1, 1, 100, true),
        (-1, 10, 100, false),
    ];
    for (offset, length, file_length, expected) in rows {
        assert_eq!(
            is_range_in_file(offset, length, file_length),
            expected,
            "offset {offset} length {length} file {file_length}"
        );
    }
}

// C#: Rust-only addition (contract): the i64 extremes must not overflow or panic.
#[test]
fn is_range_in_file_handles_i64_extremes_without_overflow() {
    assert!(is_range_in_file(1, i64::MAX - 1, i64::MAX));
    assert!(is_range_in_file(i64::MAX - 1, 1, i64::MAX));
    assert!(!is_range_in_file(1, i64::MAX, i64::MAX));
    assert!(!is_range_in_file(i64::MAX, 1, i64::MAX));
    assert!(!is_range_in_file(1, 1, i64::MIN));
}

// C#: TiffHeaderNavigatorMutationGapTests.TryReadSingleStrip_MoreThanOneStripEntry_IsRejected (rows (2, 1), (1, 2))
#[test]
fn try_read_single_strip_more_than_one_strip_entry_is_rejected() {
    for (offset_count, length_count) in [(2u32, 1u32), (1, 2)] {
        let entries = [te(0x0111, 4, offset_count, 100), te(0x0117, 4, length_count, 50)];

        let strip = try_read_single_strip(&mut src(vec![0u8; 256]), &entries, LE).expect("source read");

        assert_eq!(strip, None, "counts ({offset_count}, {length_count})");
    }
}

// C#: TiffHeaderNavigatorMutationGapTests.TryReadSingleStrip_SingleStripInsideTheFile_IsReturned
#[test]
fn try_read_single_strip_single_strip_inside_the_file_is_returned() {
    let entries = [te(0x0111, 4, 1, 100), te(0x0117, 4, 1, 50)];

    let strip = try_read_single_strip(&mut src(vec![0u8; 256]), &entries, LE).expect("source read");

    assert_eq!(strip, Some((100, 50)));
}

// C#: TiffHeaderNavigatorMutationGapTests.TryReadExifPixelDimensions_OnlyOneOfWidthAndHeight_ReturnsFalseAndZeros
#[test]
fn try_read_exif_pixel_dimensions_only_one_of_width_and_height_returns_none() {
    let only_height = TiffBytes::new(LE, 128)
        .header(8)
        .ifd(8, 0, &[long(0xA003, 480)])
        .into_vec();
    let only_width = TiffBytes::new(LE, 128)
        .header(8)
        .ifd(8, 0, &[long(0xA002, 640)])
        .into_vec();
    let both = TiffBytes::new(LE, 128)
        .header(8)
        .ifd(8, 0, &[long(0xA002, 640), long(0xA003, 480)])
        .into_vec();

    let dims = |data: Vec<u8>| try_read_exif_pixel_dimensions(&mut src(data), 8, LE).expect("source read");

    assert_eq!(dims(only_height), None, "height only");
    assert_eq!(dims(only_width), None, "width only");
    assert_eq!(dims(both), Some((640, 480)), "both present");
}

// C#: TiffPreviewHardeningTests.TryReadExifPixelDimensions_WidthOnly_ReturnsFalseAndZeroOutputs (C# out values -> None)
#[test]
fn try_read_exif_pixel_dimensions_width_only_returns_none() {
    let data = TiffBytes::new(LE, 200)
        .header(8)
        .ifd(8, 0, &[long(0xA002, 5000)])
        .into_vec();

    let dims = try_read_exif_pixel_dimensions(&mut src(data), 8, LE).expect("source read");

    assert_eq!(dims, None);
}

// ---------------------------------------------------------------- DefaultCropSize / DefaultScale / active size (mutation gap)

// C#: TiffHeaderNavigatorMutationGapTests.TryReadDefaultCropSize_NeedsTwoPositiveIntRangeValues (8 rows)
#[test]
fn try_read_default_crop_size_needs_two_positive_int_range_values() {
    // (count, first, second, expected)
    let rows = [
        (2, 6000, 4000, Some((6000, 4000))),
        (2, i32::MAX as u32, 100, Some((i32::MAX, 100))),
        (2, 100, i32::MAX as u32, Some((100, i32::MAX))),
        (2, 0, 100, None),
        (2, 100, 0, None),
        (2, 2_147_483_648, 100, None),
        (2, 100, 2_147_483_648, None),
        (1, 6000, 4000, None),
    ];
    for (count, first, second, expected) in rows {
        let (data, entries) = crop_file(count, first, second);

        let crop = try_read_default_crop_size(&mut src(data), &entries, LE).expect("source read");

        assert_eq!(crop, expected, "row count {count} first {first} second {second}");
    }
}

// C#: TiffHeaderNavigatorMutationGapTests.HasSquareDefaultScale_AbsentEntry_IsSquare
#[test]
fn has_square_default_scale_absent_entry_is_square() {
    let square = has_square_default_scale(&mut src(vec![0u8; 16]), &[], LE).expect("source read");

    assert!(square);
}

// C#: TiffHeaderNavigatorMutationGapTests.HasSquareDefaultScale_RatioCheck (10 rows, ratio = hn, hd, vn, vd)
#[test]
fn has_square_default_scale_ratio_check() {
    let rows: [([u32; 4], bool); 10] = [
        ([1, 1, 1, 1], true),
        ([4, 2, 4, 2], true), // equal ratios with a denominator other than 1
        ([2, 1, 1, 1], false),
        ([1, 2, 1, 3], false),
        ([0, 1, 1, 1], false), // zero numerator
        ([1, 1, 0, 1], false),
        ([1, 0, 1, 1], false), // zero denominator
        ([1, 1, 1, 0], false),
        ([0, 0, 1, 1], false),
        ([1, 1, 0, 0], false),
    ];
    for (ratio, expected) in rows {
        let (data, entries) = scale_file(ratio, 5, 2, 16, 64);

        let square = has_square_default_scale(&mut src(data), &entries, LE).expect("source read");

        assert_eq!(square, expected, "ratio {ratio:?}");
    }
}

// C#: TiffHeaderNavigatorMutationGapTests.HasSquareDefaultScale_EntryShapeAndRange (7 rows: type, count, offset, length)
#[test]
fn has_square_default_scale_entry_shape_and_range() {
    // (field_type, count, offset, length, expected)
    let rows: [(u16, u32, u32, usize, bool); 7] = [
        (5, 2, 8, 64, true), // offset 8: the first allowed
        (5, 2, 7, 64, false),
        (5, 2, 48, 64, true), // data ends exactly at the end of the source
        (5, 2, 49, 64, false),
        (5, 1, 16, 64, false), // a single rational is not a pair
        (4, 2, 16, 64, false), // wrong type
        (5, 3, 16, 64, true),
    ];
    for (field_type, count, offset, length, expected) in rows {
        let (data, entries) = scale_file([1, 1, 1, 1], field_type, count, offset, length);

        let square = has_square_default_scale(&mut src(data), &entries, LE).expect("source read");

        assert_eq!(
            square, expected,
            "type {field_type} count {count} offset {offset} length {length}"
        );
    }
}

// C#: TiffHeaderNavigatorMutationGapTests.ChooseActiveSensorSize_PrefersCropThenExifThenRawWithinTheRawSize (12 rows)
#[test]
fn choose_active_sensor_size_prefers_crop_then_exif_then_raw_within_the_raw_size() {
    // (raw_w, raw_h, crop_w, crop_h, exif_w, exif_h, expected_w, expected_h)
    let rows = [
        (4000, 3000, 3990, 2990, 3980, 2980, 3990, 2990), // crop first
        (4000, 3000, 4000, 3000, 3990, 2990, 4000, 3000), // crop equal to raw still fits
        (4000, 3000, 3990, 3000, 3980, 2980, 3990, 3000), // equal height fits
        (4000, 3000, 4000, 2990, 3980, 2980, 4000, 2990), // equal width fits
        (4000, 3000, 4001, 3000, 3980, 2980, 3980, 2980), // crop wider than raw: exif
        (4000, 3000, 3990, 3001, 3980, 2980, 3980, 2980), // crop taller than raw: exif
        (4000, 3000, 0, 100, 3980, 2980, 3980, 2980),     // a zero crop side is no crop
        (4000, 3000, 100, 0, 3980, 2980, 3980, 2980),
        (4000, 3000, 0, 0, 4001, 2980, 4000, 3000), // nothing fits: raw
        (0, 3000, 50, 50, 0, 0, 50, 50),            // unknown raw width: any positive crop
        (4000, 0, 50, 50, 0, 0, 50, 50),            // unknown raw height
        (0, 0, 50, 50, 0, 0, 50, 50),
    ];
    for (raw_w, raw_h, crop_w, crop_h, exif_w, exif_h, expected_w, expected_h) in rows {
        assert_eq!(
            choose_active_sensor_size(raw_w, raw_h, crop_w, crop_h, exif_w, exif_h),
            (expected_w, expected_h),
            "raw {raw_w}x{raw_h} crop {crop_w}x{crop_h} exif {exif_w}x{exif_h}"
        );
    }
}

// C#: TiffSensorSizeTests.ChooseActiveSensorSize_PrefersCropThenExifThenRaw
#[test]
fn choose_active_sensor_size_prefers_crop_then_exif_then_raw() {
    assert_eq!(choose_active_sensor_size(100, 200, 10, 20, 30, 40), (10, 20));
    assert_eq!(choose_active_sensor_size(100, 200, 0, 0, 30, 40), (30, 40));
    assert_eq!(choose_active_sensor_size(100, 200, 0, 0, 0, 0), (100, 200));
    assert_eq!(choose_active_sensor_size(100, 200, 10, 0, 300, 40), (100, 200));
}

// ---------------------------------------------------------------- reconcile_jpeg_size (mutation gap, C# twins)

// C#: TiffHeaderNavigatorMutationGapTests.ReconcileJpegSize_OnlyOneDeclaredDimension_LeavesBothUntouched (rows (100, 0), (0, 100))
// The C# walker is replaced by a fake `read_frame` that must NOT be called (note 12 of the contract).
#[test]
fn reconcile_jpeg_size_only_one_declared_dimension_leaves_both_untouched() {
    for (declared_w, declared_h) in [(100, 0), (0, 100)] {
        let jpeg = create_minimal_jpeg(640, 480);
        let (mut width, mut height) = (declared_w, declared_h);
        let mut called = false;

        let result = reconcile_jpeg_size(
            &mut src(jpeg.clone()),
            0,
            len_i64(&jpeg),
            &mut width,
            &mut height,
            |_, _, _| {
                called = true;
                Ok(None)
            },
        );

        assert!(result.is_ok());
        assert_eq!((width, height), (declared_w, declared_h));
        assert!(!called, "read_frame must not run with one declared side");
    }
}

// C#: TiffHeaderNavigatorMutationGapTests.ReconcileJpegSize_BothDeclared_TheFrameHeaderWins
#[test]
fn reconcile_jpeg_size_both_declared_the_frame_header_wins() {
    let jpeg = create_minimal_jpeg(640, 480);
    let (mut width, mut height) = (100, 50);

    let result = reconcile_jpeg_size(
        &mut src(jpeg.clone()),
        0,
        len_i64(&jpeg),
        &mut width,
        &mut height,
        |_, _, _| Ok(Some((640, 480))),
    );

    assert!(result.is_ok());
    assert_eq!((width, height), (640, 480));
}

// Rust-only (contract): a non-I/O header error from read_frame keeps the declared sizes and gives Ok(()).
#[test]
fn reconcile_jpeg_size_non_io_error_keeps_declared_size_and_succeeds() {
    let (mut width, mut height) = (100, 50);

    let result = reconcile_jpeg_size(&mut src(vec![0u8; 64]), 0, 64, &mut width, &mut height, |_, _, _| {
        Err(RawHeaderError::invalid_data("not a frame"))
    });

    assert!(result.is_ok());
    assert_eq!((width, height), (100, 50));
}

// Rust-only (contract): an I/O failure from read_frame is returned as Err.
#[test]
fn reconcile_jpeg_size_io_error_is_returned() {
    let (mut width, mut height) = (100, 50);

    let result = reconcile_jpeg_size(&mut src(vec![0u8; 64]), 0, 64, &mut width, &mut height, |_, _, _| {
        Err(RawHeaderError::invalid_data_io("disk", io::Error::other("disk")))
    });

    assert!(result.is_err_and(|e| e.is_io_failure()));
}

// Rust-only (contract): Ok(None) from read_frame leaves both sizes untouched.
#[test]
fn reconcile_jpeg_size_none_from_the_walker_leaves_both_untouched() {
    let (mut width, mut height) = (100, 50);

    let result = reconcile_jpeg_size(&mut src(vec![0u8; 64]), 0, 64, &mut width, &mut height, |_, _, _| {
        Ok(None)
    });

    assert!(result.is_ok());
    assert_eq!((width, height), (100, 50));
}

// Rust-only (contract): a width or height <= 0 never calls read_frame.
#[test]
fn reconcile_jpeg_size_non_positive_side_never_calls_read_frame() {
    for (declared_w, declared_h) in [(0, 50), (50, 0), (-1, 50)] {
        let (mut width, mut height) = (declared_w, declared_h);
        let mut called = false;

        let result = reconcile_jpeg_size(&mut src(vec![0u8; 64]), 0, 64, &mut width, &mut height, |_, _, _| {
            called = true;
            Ok(Some((640, 480)))
        });

        assert!(result.is_ok(), "declared {declared_w}x{declared_h}");
        assert!(!called, "declared {declared_w}x{declared_h}");
    }
}

// ---------------------------------------------------------------- ComputeExifBlock (mutation gap)

const DEFAULT_BLOCK: i64 = 128 * 1024;

// C#: TiffHeaderNavigatorMutationGapTests.ComputeExifBlock_ExifIfdBeyondTheDefaultBlock_GrowsToCoverItsEntriesAndNextPointer
#[test]
fn compute_exif_block_exif_ifd_beyond_the_default_block_grows_to_cover_its_entries_and_next_pointer() {
    let data = TiffBytes::new(LE, 300_000)
        .header(8)
        .ifd(8, 0, &[long(0x8769, 200_000)])
        .ifd(200_000, 0, &[short(0x8827, 100), short(0xA001, 1), short(0x9209, 0)])
        .into_vec();

    let block = exif_block_of(data);

    assert_eq!(
        (block.offset, block.length, block.is_tiff_header, block.ifd_is_exif),
        (0, 200_000 + 2 + 36 + 4, true, false)
    );
}

// C#: TiffHeaderNavigatorMutationGapTests.ComputeExifBlock_WantedOutOfLineValueBeyondTheDefaultBlock_GrowsToItsEnd
#[test]
fn compute_exif_block_wanted_out_of_line_value_beyond_the_default_block_grows_to_its_end() {
    let data = TiffBytes::new(LE, 300_000)
        .header(8)
        .ifd(8, 0, &[long(0x8769, 100)])
        .ifd(100, 0, &[at(0xA434, 2, 20, 250_000)])
        .into_vec();

    assert_eq!(exif_block_of(data).length, 250_020);
}

// C#: TiffHeaderNavigatorMutationGapTests.ComputeExifBlock_ExtentEndingExactlyAtTheEndOfTheFile_StillGrows
#[test]
fn compute_exif_block_extent_ending_exactly_at_the_end_of_the_file_still_grows() {
    let data = TiffBytes::new(LE, 250_020)
        .header(8)
        .ifd(8, 0, &[long(0x8769, 100)])
        .ifd(100, 0, &[at(0xA434, 2, 20, 250_000)])
        .into_vec();

    assert_eq!(exif_block_of(data).length, 250_020);
}

// C#: TiffHeaderNavigatorMutationGapTests.ComputeExifBlock_ExtentBeyondTheEndOfTheFile_KeepsTheDefaultBlock
#[test]
fn compute_exif_block_extent_beyond_the_end_of_the_file_keeps_the_default_block() {
    let data = TiffBytes::new(LE, 250_000)
        .header(8)
        .ifd(8, 0, &[long(0x8769, 100)])
        .ifd(100, 0, &[at(0xA434, 2, 20, 250_000)])
        .into_vec();

    assert_eq!(exif_block_of(data).length, DEFAULT_BLOCK);
}

// C#: TiffHeaderNavigatorMutationGapTests.ComputeExifBlock_UnwantedTagWithAFarValue_DoesNotGrowTheBlock
#[test]
fn compute_exif_block_unwanted_tag_with_a_far_value_does_not_grow_the_block() {
    let data = TiffBytes::new(LE, 300_000)
        .header(8)
        .ifd(8, 0, &[at(0x9286, 7, 100, 250_000)])
        .into_vec();

    assert_eq!(exif_block_of(data).length, DEFAULT_BLOCK);
}

// C#: TiffHeaderNavigatorMutationGapTests.ComputeExifBlock_WantedIfd0TagOtherThanTheFirstWithAFarValue_GrowsTheBlock
#[test]
fn compute_exif_block_wanted_ifd0_tag_other_than_the_first_with_a_far_value_grows_the_block() {
    let data = TiffBytes::new(LE, 300_000)
        .header(8)
        .ifd(8, 0, &[at(0x0110, 2, 20, 250_000)])
        .into_vec();

    assert_eq!(exif_block_of(data).length, 250_020);
}

// C#: TiffHeaderNavigatorMutationGapTests.ComputeExifBlock_FourByteInlineWantedValue_IsNotTreatedAsAnOffset
#[test]
fn compute_exif_block_four_byte_inline_wanted_value_is_not_treated_as_an_offset() {
    // Make is 4 inline bytes: "ValueOrOffset" holds text, not an offset, so it must not stretch the block.
    let data = TiffBytes::new(LE, 300_000)
        .header(8)
        .ifd(8, 0, &[at(0x010F, 2, 4, 0xFFFF_FFF0), long(0x8769, 200_000)])
        .ifd(200_000, 0, &[short(0x8827, 100)])
        .into_vec();

    assert_eq!(exif_block_of(data).length, 200_000 + 2 + 12 + 4);
}

// C#: RawErrorHandlingReviewTests.ComputeExifBlock_IoFailure_Propagates
#[test]
fn compute_exif_block_io_failure_propagates() {
    let result = compute_exif_block(&mut FailingSource(FailureKind::Io), LE, 8);

    assert!(matches!(result, Err(ref e) if e.is_io_failure()));
}

// C#: RawErrorHandlingReviewTests.ComputeExifBlock_NonIoFailure_KeepsDefaultBlock
#[test]
fn compute_exif_block_non_io_failure_keeps_default_block() {
    let block = compute_exif_block(&mut FailingSource(FailureKind::Budget), LE, 8)
        .expect("a non-I/O failure keeps the default block");

    assert_eq!(block.length, i64::from(limits::DEFAULT_EXIF_BLOCK_BYTES));
}

// ---------------------------------------------------------------- TiffReaderRegressionTests (TiffHeaderNavigator section)

// C#: TiffReaderRegressionTests.ReadTagUnsignedArray_CountAboveInt32Max_ReturnsBoundedListWithoutThrowing
#[test]
fn read_tag_unsigned_array_count_above_int32_max_returns_bounded_list_without_throwing() {
    let data = TiffBytes::new(LE, 512).header(8).into_vec();
    let entry = te(0x014A, 4, 0xFFFF_FFFF, 100);

    let values = read_tag_unsigned_array(&mut src(data), &entry, LE, 8).expect("source read");

    assert!(values.len() <= 8);
}

// C#: TiffReaderRegressionTests.ReadTagUnsignedArray_MaxItemsZero_ReturnsEmpty
#[test]
fn read_tag_unsigned_array_max_items_zero_returns_empty() {
    let data = TiffBytes::new(LE, 512).header(8).into_vec();
    let entry = te(0x014A, 4, 2, 100);

    let values = read_tag_unsigned_array(&mut src(data), &entry, LE, 0).expect("source read");

    assert!(values.is_empty());
}

// C#: TiffReaderRegressionTests.ReadTagUnsignedArray_RationalType_RoundsToWholeNumbers
#[test]
fn read_tag_unsigned_array_rational_type_rounds_to_whole_numbers() {
    let data = TiffBytes::new(LE, 512)
        .header(8)
        .u32(100, 4672)
        .u32(104, 1)
        .u32(108, 31041)
        .u32(112, 10)
        .into_vec();
    let entry = te(0xC620, 5, 2, 100);

    let values = read_tag_unsigned_array(&mut src(data), &entry, LE, 2).expect("source read");

    assert_eq!(values, vec![4672, 3104]);
}

// C#: TiffReaderRegressionTests.ReadIfdEntries_NextIfdPointerOutsideUsableRange_ReportsChainEnd (rows 4, 0xFFFFFFF0, 510)
#[test]
fn read_ifd_entries_next_ifd_pointer_outside_usable_range_reports_chain_end() {
    for next in [4u32, 0xFFFF_FFF0, 510] {
        let data = TiffBytes::new(LE, 512)
            .header(8)
            .ifd(8, next, &[short(0x0112, 1)])
            .into_vec();

        let read = ifd_read(data, 8);

        assert_eq!(read.next_ifd_offset, 0, "row next pointer {next}");
    }
}

// C#: TiffReaderRegressionTests.ReadIfdEntries_NextIfdPointerInsideFile_IsReturned
#[test]
fn read_ifd_entries_next_ifd_pointer_inside_file_is_returned() {
    let data = TiffBytes::new(LE, 512)
        .header(8)
        .ifd(8, 200, &[short(0x0112, 1)])
        .into_vec();

    let read = ifd_read(data, 8);

    assert_eq!(read.next_ifd_offset, 200);
}

// C#: TiffReaderRegressionTests.TryReadSingleStrip_MultipleStrips_IsRejected
#[test]
fn try_read_single_strip_multiple_strips_is_rejected() {
    let data = TiffBytes::new(LE, 512).header(8).into_vec();
    let entries = [te(0x0111, 4, 2, 100), te(0x0117, 4, 2, 108)];

    let strip = try_read_single_strip(&mut src(data), &entries, LE).expect("source read");

    assert_eq!(strip, None);
}

// ---------------------------------------------------------------- Rust-only additions (contract)

// Rust-only (contract): read_tag_unsigned_array with max_items = i32::MAX and count = u32::MAX stays bounded by the source.
#[test]
fn read_tag_unsigned_array_max_items_int32_max_with_count_uint32_max_is_bounded() {
    let data = TiffBytes::new(LE, 512).header(8).into_vec();
    let entry = te(0x014A, 4, u32::MAX, 100);

    let values = read_tag_unsigned_array(&mut src(data), &entry, LE, i32::MAX).expect("source read");

    // DIVERGENCE: C# overflows int here and throws; the port never overflows (contract open question 4).
    assert!(values.len() <= 128, "{} items from a 512-byte file", values.len());
}

// ---------------------------------------------------------------- golden tests/golden/tiff-navigator.json

fn golden_input(name: &str) -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"].as_array().expect("cases array").clone()
}

/// Golden `entry` / `entries` node: `{tag, type, count, value}`.
fn entry_of(node: &Value) -> TiffEntry {
    let field = |key: &str| node[key].as_u64().unwrap_or_else(|| panic!("entry {key}: {node}"));
    te(
        u16::try_from(field("tag")).expect("tag u16"),
        u16::try_from(field("type")).expect("type u16"),
        u32::try_from(field("count")).expect("count u32"),
        u32::try_from(field("value")).expect("value u32"),
    )
}

fn entries_of(node: &Value) -> Vec<TiffEntry> {
    node.as_array().expect("entries array").iter().map(entry_of).collect()
}

fn entry_json(entry: &TiffEntry) -> Value {
    json!({
        "tag": entry.tag,
        "type": entry.field_type,
        "count": entry.count,
        "value": entry.value_or_offset,
    })
}

fn entries_json(entries: &[TiffEntry]) -> Value {
    Value::Array(entries.iter().map(entry_json).collect())
}

fn pair_json(found: Option<(i64, i64)>) -> Value {
    match found {
        Some((offset, length)) => json!({"ok": true, "offset": offset, "length": length}),
        None => json!({"ok": false, "offset": 0, "length": 0}),
    }
}

fn size_json(found: Option<(i32, i32)>) -> Value {
    match found {
        Some((width, height)) => json!({"ok": true, "width": width, "height": height}),
        None => json!({"ok": false, "width": 0, "height": 0}),
    }
}

fn exif_block_json(block: &ExifBlock) -> Value {
    json!({
        "offset": block.offset,
        "length": block.length,
        "is_tiff_header": block.is_tiff_header,
        "ifd_is_exif": block.ifd_is_exif,
    })
}

/// C# `W502Source`: golden bytes, bounds checks, and an optional failure after `fail_after` Read calls (every call counts).
struct GoldenSource {
    data: Vec<u8>,
    /// `Some(true)` = I/O failure, `Some(false)` = budget failure, `None` = never fails.
    fail: Option<bool>,
    fail_after: usize,
    reads: usize,
}

impl RawHeaderSource for GoldenSource {
    fn length(&self) -> i64 {
        len_i64(&self.data)
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let call = self.reads;
        self.reads += 1;
        if let Some(io_failure) = self.fail {
            if call >= self.fail_after {
                return Err(if io_failure {
                    RawHeaderError::invalid_data_io("read failed", io::Error::other("disk"))
                } else {
                    RawHeaderError::invalid_data("budget exhausted")
                });
            }
        }
        if offset < 0 || count < 0 {
            return Err(RawHeaderError::ArgumentOutOfRange {
                param: "offset",
                message: "Offset and count must be non-negative.".to_owned(),
            });
        }
        if offset > self.length() - i64::from(count) {
            return Err(RawHeaderError::invalid_data("past end"));
        }
        if count > limits::MAX_HEADER_BYTES {
            return Err(RawHeaderError::invalid_data("over the hard limit"));
        }
        let start = usize::try_from(offset).expect("offset checked");
        let end = start + usize::try_from(count).expect("count checked");
        Ok(&self.data[start..end])
    }
}

fn golden_source(input: &Value) -> GoldenSource {
    let fail = input.get("fail").and_then(Value::as_str).map(|kind| kind == "io");
    GoldenSource {
        data: input.get("data").map(golden_bytes).unwrap_or_default(),
        fail,
        fail_after: usize::try_from(input.get("fail_after").and_then(Value::as_u64).unwrap_or(0))
            .expect("fail_after fits usize"),
        reads: 0,
    }
}

/// Golden `error` tag: `io` for an I/O failure, `data` for other `InvalidData`, `argument` for a bug-class error.
fn error_tag(error: &RawHeaderError) -> &'static str {
    match error {
        RawHeaderError::InvalidData { io, .. } => {
            if io.is_some() {
                "io"
            } else {
                "data"
            }
        }
        RawHeaderError::ArgumentOutOfRange { .. } => "argument",
        RawHeaderError::Canceled => "canceled",
    }
}

/// Outcome of the `ExerciseTiffNavigator` battery (C# `walk` op): the fields computed before a failure, and the failure.
struct Walk {
    report: Map<String, Value>,
    error: Option<RawHeaderError>,
}

/// The walk battery of `BinaryReaderFuzzTests.ExerciseTiffNavigator`, in its order, stopping at the first error.
/// `reconcile` adds the final `reconcile_jpeg_size` call with a fake walker that reads two bytes (fuzz only; the golden
/// omits it, see the oracle header).
fn walk(source: &mut dyn RawHeaderSource, little: bool, ifd0: i64, reconcile: bool) -> Walk {
    let mut report = Map::new();
    let error = walk_steps(source, little, ifd0, reconcile, &mut report).err();
    Walk { report, error }
}

fn walk_steps(
    source: &mut dyn RawHeaderSource,
    little: bool,
    ifd0: i64,
    reconcile: bool,
    report: &mut Map<String, Value>,
) -> Result<(), RawHeaderError> {
    let ifd = read_ifd_entries(source, ifd0, little)?;
    report.insert("entries".to_owned(), entries_json(&ifd.entries));
    report.insert("next".to_owned(), json!(ifd.next_ifd_offset));

    let mut tag_values: Vec<Value> = Vec::new();
    let mut tag_arrays: Vec<Value> = Vec::new();
    let mut failure = None;
    for entry in ifd.entries.iter().take(64) {
        match read_tag_unsigned(source, entry, little) {
            Ok(value) => tag_values.push(json!(value)),
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
        match read_tag_unsigned_array(source, entry, little, DEFAULT_ARRAY_ITEMS) {
            Ok(values) => tag_arrays.push(json!(values)),
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
    }
    report.insert("tag_values".to_owned(), Value::Array(tag_values));
    report.insert("tag_arrays".to_owned(), Value::Array(tag_arrays));
    if let Some(e) = failure {
        return Err(e);
    }

    let next_entries = if ifd.next_ifd_offset != 0 {
        json!(
            read_ifd_entries(source, i64::from(ifd.next_ifd_offset), little)?
                .entries
                .len()
        )
    } else {
        Value::Null
    };
    report.insert("next_entries".to_owned(), next_entries);

    let block = compute_exif_block(source, little, ifd0)?;
    report.insert("exif_block".to_owned(), exif_block_json(&block));

    let strip = try_read_single_strip(source, &ifd.entries, little)?;
    report.insert("strip".to_owned(), pair_json(strip));

    let crop = try_read_default_crop_size(source, &ifd.entries, little)?;
    report.insert("crop".to_owned(), size_json(crop));

    let square = has_square_default_scale(source, &ifd.entries, little)?;
    report.insert("square_scale".to_owned(), json!(square));

    let (width, height) = read_image_size(source, &ifd.entries, little)?;
    report.insert("image_size".to_owned(), json!({"width": width, "height": height}));

    let exif_dims = try_read_exif_pixel_dimensions(source, ifd0, little)?;
    report.insert("exif_dims".to_owned(), size_json(exif_dims));

    let jpeg = try_read_jpeg_interchange(source, &ifd.entries, little)?;
    report.insert("jpeg".to_owned(), pair_json(jpeg));

    if reconcile {
        if let Some((offset, length)) = jpeg {
            let (mut w, mut h) = (64, 48);
            reconcile_jpeg_size(source, offset, length, &mut w, &mut h, |s, o, _| {
                s.read(o, 2)?;
                Ok(None)
            })?;
        }
    }
    Ok(())
}

/// One golden case: runs the op of `input` and returns `{error, ...fields}` in the golden's shape.
fn run_golden_case(input: &Value) -> Value {
    let op = input["op"].as_str().expect("op");
    let little = input.get("little").and_then(Value::as_bool).unwrap_or(true);
    let mut fields = Map::new();
    let result = run_op(op, input, little, &mut fields);
    let error = match &result {
        Ok(()) => Value::Null,
        Err(e) => json!(error_tag(e)),
    };
    fields.insert("error".to_owned(), error);
    Value::Object(fields)
}

fn run_op(op: &str, input: &Value, little: bool, fields: &mut Map<String, Value>) -> Result<(), RawHeaderError> {
    match op {
        "ifd-entries" => {
            let mut source = golden_source(input);
            let read = read_ifd_entries(&mut source, input["offset"].as_i64().expect("offset"), little)?;
            fields.insert("entries".to_owned(), entries_json(&read.entries));
            fields.insert("next".to_owned(), json!(read.next_ifd_offset));
        }
        "tag-unsigned" => {
            let mut source = golden_source(input);
            let value = read_tag_unsigned(&mut source, &entry_of(&input["entry"]), little)?;
            fields.insert("value".to_owned(), json!(value));
        }
        "tag-array" => {
            let mut source = golden_source(input);
            let max_items = input
                .get("max_items")
                .and_then(Value::as_i64)
                .map_or(DEFAULT_ARRAY_ITEMS, |v| i32::try_from(v).expect("max_items i32"));
            let values = read_tag_unsigned_array(&mut source, &entry_of(&input["entry"]), little, max_items)?;
            fields.insert("values".to_owned(), json!(values));
        }
        "exif-block" => {
            let mut source = golden_source(input);
            let block = compute_exif_block(&mut source, little, input["ifd0"].as_i64().expect("ifd0"))?;
            for (key, value) in exif_block_json(&block).as_object().expect("object") {
                fields.insert(key.clone(), value.clone());
            }
        }
        "single-strip" => {
            let mut source = golden_source(input);
            let strip = try_read_single_strip(&mut source, &entries_of(&input["entries"]), little)?;
            merge(fields, &pair_json(strip));
        }
        "crop-size" => {
            let mut source = golden_source(input);
            let crop = try_read_default_crop_size(&mut source, &entries_of(&input["entries"]), little)?;
            merge(fields, &size_json(crop));
        }
        "square-scale" => {
            let mut source = golden_source(input);
            let square = has_square_default_scale(&mut source, &entries_of(&input["entries"]), little)?;
            fields.insert("value".to_owned(), json!(square));
        }
        "jpeg-interchange" => {
            let mut source = golden_source(input);
            let jpeg = try_read_jpeg_interchange(&mut source, &entries_of(&input["entries"]), little)?;
            merge(fields, &pair_json(jpeg));
        }
        "image-size" => {
            let mut source = golden_source(input);
            let (width, height) = read_image_size(&mut source, &entries_of(&input["entries"]), little)?;
            fields.insert("width".to_owned(), json!(width));
            fields.insert("height".to_owned(), json!(height));
        }
        "tag-value" => {
            let mut source = golden_source(input);
            let tag = u16::try_from(input["tag"].as_u64().expect("tag")).expect("tag u16");
            let value = read_tag_value(&mut source, &entries_of(&input["entries"]), tag, little)?;
            fields.insert("value".to_owned(), json!(value));
        }
        "exif-dims" => {
            let mut source = golden_source(input);
            let dims = try_read_exif_pixel_dimensions(&mut source, input["offset"].as_i64().expect("offset"), little)?;
            merge(fields, &size_json(dims));
        }
        "soi" => {
            let mut source = golden_source(input);
            let value = starts_with_soi(
                &mut source,
                input["offset"].as_i64().expect("offset"),
                input["length"].as_i64().expect("length"),
            )?;
            fields.insert("value".to_owned(), json!(value));
        }
        "range" => {
            let value = is_range_in_file(
                input["offset"].as_i64().expect("offset"),
                input["length"].as_i64().expect("length"),
                input["file_length"].as_i64().expect("file_length"),
            );
            fields.insert("value".to_owned(), json!(value));
        }
        "clamp" => {
            let value = clamp_to_int(input["value"].as_i64());
            fields.insert("value".to_owned(), json!(value));
        }
        "sensor-size" => {
            let i32_of = |key: &str| i32::try_from(input[key].as_i64().expect(key)).expect("i32");
            let (width, height) = choose_active_sensor_size(
                i32_of("raw_w"),
                i32_of("raw_h"),
                i32_of("crop_w"),
                i32_of("crop_h"),
                i32_of("exif_w"),
                i32_of("exif_h"),
            );
            fields.insert("width".to_owned(), json!(width));
            fields.insert("height".to_owned(), json!(height));
        }
        "get-entry" => {
            let tag = u16::try_from(input["tag"].as_u64().expect("tag")).expect("tag u16");
            let found = try_get_entry(&entries_of(&input["entries"]), tag);
            fields.insert("ok".to_owned(), json!(found.is_some()));
            fields.insert("entry".to_owned(), found.as_ref().map_or(Value::Null, entry_json));
        }
        "walk" => {
            let mut source = golden_source(input);
            let outcome = walk(&mut source, little, input["ifd0"].as_i64().expect("ifd0"), false);
            fields.extend(outcome.report);
            if let Some(e) = outcome.error {
                return Err(e);
            }
        }
        other => panic!("unknown golden op {other}"),
    }
    Ok(())
}

fn merge(fields: &mut Map<String, Value>, object: &Value) {
    for (key, value) in object.as_object().expect("object") {
        fields.insert(key.clone(), value.clone());
    }
}

// C#: tests/golden/tiff-navigator.json (oracle W5-02a.cs `tiff-navigator`): 538 cases, every reported field and `error`.
#[test]
fn golden_tiff_navigator_matches_csharp_for_every_case() {
    let golden_file = golden::load("tiff-navigator");
    let inputs = golden_input("tiff-navigator");
    assert_eq!(
        golden_file.cases.len(),
        inputs.len(),
        "golden and input case counts differ"
    );

    let mut mismatches = Vec::new();
    for (expected_case, input) in golden_file.cases.iter().zip(&inputs) {
        let name = golden::str_of(expected_case, "name");
        assert_eq!(
            name,
            golden::str_of(input, "name"),
            "case order differs from the input file"
        );

        let mut expected = expected_case.as_object().expect("golden case object").clone();
        expected.remove("name");
        expected.remove("op");
        let actual = run_golden_case(input);

        if Value::Object(expected.clone()) != actual {
            mismatches.push(format!("[{name}] expected {}, got {actual}", Value::Object(expected)));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} tiff-navigator golden cases differ from C#:\n{}",
        mismatches.len(),
        golden_file.cases.len(),
        mismatches.join("\n")
    );
}

// ---------------------------------------------------------------- fuzz (BinaryReaderFuzzTests.Fuzz_TiffHeaderNavigator_FailsOnlyCleanly)

/// The C# `ExerciseTiffNavigator` battery on one mutant: header (None = skipped), then the walk with the reconcile call.
/// `Ok(accepted)` where `accepted` is "entries found"; `Err` only for a documented `InvalidData`.
fn exercise_battery(bytes: &[u8]) -> Result<bool, RawHeaderError> {
    let Some(header) = try_read_header(bytes) else {
        return Ok(false);
    };
    let mut source = InMemoryRawHeaderSource::new(bytes.to_vec());
    let outcome = walk(&mut source, header.little_endian, i64::from(header.ifd0_offset), true);
    match outcome.error {
        Some(e) => Err(e),
        None => Ok(outcome.report["entries"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty())),
    }
}

/// Runs one mutant: a panic or a non-`InvalidData` error is a defect that names the family and the label.
fn check_mutant(family: &str, case: &Case) -> bool {
    match std::panic::catch_unwind(AssertUnwindSafe(|| exercise_battery(&case.bytes))) {
        Err(_) => panic!("{family} mutant [{}] ({} bytes) panicked", case.label, case.bytes.len()),
        Ok(Ok(accepted)) => accepted,
        Ok(Err(error)) => {
            assert!(
                matches!(error, RawHeaderError::InvalidData { .. }),
                "{family} mutant [{}] escaped with a non-documented error: {error}",
                case.label
            );
            false
        }
    }
}

/// The 1400-byte "walk base" of the golden inputs, in one byte order (IFD0 at 8 with 12 entries, IFD1 at 200, Exif IFD at 300).
fn walk_base(little: bool) -> Vec<u8> {
    let name = if little { "walk-base-le" } else { "walk-base-be" };
    let input = golden_input("tiff-navigator")
        .into_iter()
        .find(|case| case["name"] == name)
        .unwrap_or_else(|| panic!("golden input {name} missing"));
    golden_bytes(&input["data"])
}

/// Seeded byte edits (1..4 edits per mutant) concentrated on offsets 0..520 of the walk base.
fn edit_mutants(base: &[u8], count: usize) -> Vec<Case> {
    let mut rng = XorShift64::new(0x9E37_79B9);
    let window = 521usize.min(base.len());
    (0..count)
        .map(|index| {
            let mut data = base.to_vec();
            let edits = 1 + rng.below(4);
            let mut label = format!("edit#{index}");
            for _ in 0..edits {
                let pos = rng.below(window);
                let value = match rng.below(4) {
                    0 => 0xFF,
                    1 => u8::try_from(rng.below(256)).expect("byte"),
                    2 => 0,
                    _ => data[pos].wrapping_add(1),
                };
                data[pos] = value;
                label.push_str(&format!(" {pos}={value:02X}"));
            }
            Case { label, bytes: data }
        })
        .collect()
}

// C#: BinaryReaderFuzzTests.Fuzz_TiffHeaderNavigator_FailsOnlyCleanly (+ the walk-base family and the 2000 byte-edit
// mutants of the contract). Each family yields more than 500 cases; both byte orders accept at least one mutant.
#[test]
fn fuzz_tiff_navigator_never_panics() {
    let mut accepted_per_order = [0usize; 2];
    let mut total_cases = 0usize;
    for (order, little) in [true, false].into_iter().enumerate() {
        let seed = build_tiff(little, Some(&create_minimal_jpeg(64, 48)), 6, 42);
        let base = walk_base(little);
        let rng_seed = 4200 + u64::from(little);
        let families = [
            ("tiff-seed", mutants(&seed, rng_seed, 1500)),
            ("walk-base", mutants(&base, rng_seed, 1500)),
            ("walk-edits", edit_mutants(&base, 2000)),
        ];
        for (family, cases) in families {
            assert!(cases.len() > 500, "{family}: only {} cases", cases.len());
            for case in &cases {
                if check_mutant(family, case) {
                    accepted_per_order[order] += 1;
                }
            }
            total_cases += cases.len();
        }
    }
    assert!(
        accepted_per_order.iter().all(|&accepted| accepted > 0),
        "every byte order must accept at least one mutant (per order: {accepted_per_order:?}, cases: {total_cases})"
    );
}

// ---------------------------------------------------------------- builder round trip (D48, green by design)

// C#: RawContractsAndHeaderSourceTests.SyntheticRawBuilder_RoundTripsMinimalContainers
#[test]
fn synthetic_raw_builder_round_trips_minimal_containers() {
    let tiff_le = build_tiff(true, None, 1, 42);
    assert_eq!(&tiff_le[..2], b"II");
    let header = try_read_header(&tiff_le).expect("TIFF header parses");
    assert!(header.little_endian);
    assert_eq!(header.ifd0_offset, 8);

    let tiff_be = build_tiff(false, None, 1, 42);
    assert_eq!(&tiff_be[..2], b"MM");

    let bmff = build_iso_bmff(None);
    assert_eq!(&bmff[4..8], b"ftyp");

    let raf = build_raf(None);
    assert!(raf.starts_with(b"FUJIFILMCCD-RAW"));
}
