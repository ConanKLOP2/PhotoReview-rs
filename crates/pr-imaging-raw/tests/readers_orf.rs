//! Red tests for W5-05 (`readers/orf.rs`): `OrfContainerReader` (Olympus / OM System ORF; C# baseline d92d6aae,
//! `src/PhotoReview.Imaging.Raw/Tiff/OrfContainerReader.cs`).
//!
//! Ported from `tests/PhotoReview.Imaging.Tests/Raw/Tiff/NefOrfRw2MutationGapTests.cs` (ORF rows only), `Raw/Tiff/OrfRw2HeaderTests.cs`
//! (ORF methods only), `Raw/Tiff/OrfLegacyMakerNoteTests.cs`, the ORF section of `Raw/Tiff/TiffReaderRegressionTests.cs`,
//! `Raw/Tiff/TiffFamilyReaderTests.cs` (`OrfReader_Synthetic_CanReadHeader`), the `orf` rows of `Raw/Tiff/TiffPreviewHardeningTests.cs`
//! and `Raw/Tiff/TiffExifBlockPlacementTests.cs` (reader half only), `Raw/OrfDisplayedSizeTests.cs` (reader half of the synthetic rows),
//! the ORF target of `Robustness/RawContainerFuzzTests.cs` and `Robustness/BinaryReaderFuzzTests.cs`, and the golden
//! `tests/golden/orf-container.json` (inputs in `tests/golden-inputs/orf-container.json`). Shared byte builders: `pr_test_support::raw` (D48).
//!
//! BLOCKED (not ported here, see the W5-05 report): the corpus tests (`AllOrfCorpusFiles_MakerNotePreviewIsWholeJpegLocatedRelativeToMakerNote`,
//! the `.orf` part of `AllCorpusTiffSamples_CanBeParsed` and `AllTiffCorpusFiles_EveryListedPreviewLiesInsideFileAndStartsWithSoi`, and the
//! reader half of `CorpusOrf_ReaderSizeEqualsLibRawSize`) need a `pr_test_support::raw_corpus` helper that does not exist yet. The golden
//! corpus cases (`present: false` in every committed golden) are skipped; a `present: true` corpus case fails loudly until the helper lands.
//!
//! Stub bodies are `todo!()`: every test that reaches the reader fails until the porter fills it in.

use std::panic::AssertUnwindSafe;

use pr_core::sync::CancelToken;
use pr_imaging_raw::formats::limits;
use pr_imaging_raw::readers::orf::{
    DECODED_WIDTH_WITHOUT_MASKED_COLUMNS, OrfContainerReader, RAW_FRAME_WIDTH_WITH_MASKED_COLUMNS,
};
use pr_imaging_raw::{
    EmbeddedPreviewKind, InMemoryRawHeaderSource, PreviewColorSpace, RawContainerInfo, RawContainerReader,
    RawHeaderError,
};
use pr_test_support::golden;
use pr_test_support::raw::{TiffBytes, XorShift64, at, build_tiff, create_minimal_jpeg, long, short};
use serde_json::{Map, Value, json};

// ---------------------------------------------------------------- helpers

/// The four accepted signatures: IIRO, IIRS, MMOR, plain `II*\0` (C# `SignatureCase`, ORF rows).
const ORF_SIGNATURES: [[u8; 4]; 4] = [*b"IIRO", *b"IIRS", *b"MMOR", [b'I', b'I', 0x2A, 0x00]];

/// Parses `bytes` with a fresh in-memory source and an uncancelled token (C# `ReadOrf`).
fn read_orf(bytes: Vec<u8>) -> Result<RawContainerInfo, RawHeaderError> {
    let mut source = InMemoryRawHeaderSource::new(bytes);
    OrfContainerReader::new().read(&mut source, &CancelToken::new())
}

/// `read_orf` for inputs the C# test expects to parse.
fn ok_info(bytes: Vec<u8>) -> RawContainerInfo {
    read_orf(bytes).expect("ORF read should succeed")
}

/// Asserts the error is the documented `InvalidData` kind (C# `InvalidDataException`).
fn assert_invalid_data(result: Result<RawContainerInfo, RawHeaderError>, what: &str) {
    match result {
        Err(RawHeaderError::InvalidData { .. }) => {}
        other => panic!("{what}: expected InvalidData, got {other:?}"),
    }
}

/// Little-endian ORF with IFD0 at 8 and the signature `IIRO` (C# `Orf(size)` = `TiffBytes(true, size).Header(8, "IIRO")`).
fn orf_file(size: usize) -> TiffBytes {
    TiffBytes::new(true, size).put(0, b"IIRO").u32(4, 8)
}

/// Baseline 640x480 JPEG used by most MakerNote tests (C# `Jpeg(640, 480)`).
fn jpeg_640x480() -> Vec<u8> {
    create_minimal_jpeg(640, 480)
}

// ---------------------------------------------------------------- CanRead

#[test]
// C#: NefOrfRw2MutationGapTests.CanRead_ExactlyFourSignatureBytes_IsAccepted (rows orf-iiro, orf-iirs, orf-mmor, orf-tiff)
fn can_read_exactly_four_signature_bytes_is_accepted() {
    let reader = OrfContainerReader::new();
    for head in ORF_SIGNATURES {
        assert!(reader.can_read(&head, ".orf"), "signature {head:?} must be accepted");
        assert!(
            !reader.can_read(&head[..3], ".orf"),
            "a 3-byte span of {head:?} must be rejected"
        );
    }
}

#[test]
// C#: NefOrfRw2MutationGapTests.CanRead_AnySingleSignatureByteWrong_IsRejected (the 16 ORF rows: 4 signatures x 4 positions)
fn can_read_any_single_signature_byte_wrong_is_rejected() {
    let reader = OrfContainerReader::new();
    for signature in ORF_SIGNATURES {
        for position in 0..4 {
            let mut head = signature;
            head[position] ^= 0x40;
            assert!(
                !reader.can_read(&head, ".orf"),
                "{signature:?} with byte {position} flipped must be rejected"
            );
        }
    }
}

// ---------------------------------------------------------------- short file / empty IFD

#[test]
// C#: NefOrfRw2MutationGapTests.Read_SixteenByteFileWithEmptyIfd_ReturnsEmptyInfoForNefOrfAndRw2 (ORF part)
fn read_sixteen_byte_file_with_empty_ifd_returns_empty_info_for_orf() {
    let info = ok_info(TiffBytes::new(true, 16).header(8).ifd(8, 0, &[]).into_vec());

    assert_eq!((info.sensor_width, info.sensor_height), (0, 0));
    assert_eq!(info.orientation, 1);
    assert!(info.previews.is_empty());
}

// ---------------------------------------------------------------- ORF: header, chain, orientation, size

#[test]
// C#: NefOrfRw2MutationGapTests.ReadOrf_ChainOfSixtySixIfds_ListsOnlyTheFirstSixtyFourPreviews
fn read_orf_chain_of_sixty_six_ifds_lists_only_the_first_sixty_four_previews() {
    const IFD_SIZE: usize = 30;
    let jpeg = jpeg_640x480();
    let jpeg_at = 8 + 66 * IFD_SIZE;
    let jpeg_len = u32::try_from(jpeg.len()).expect("jpeg length fits u32");
    let mut file = orf_file(jpeg_at + jpeg.len());
    for i in 0..66 {
        let at_pos = 8 + i * IFD_SIZE;
        let next = if i < 65 {
            u32::try_from(at_pos + IFD_SIZE).expect("offset fits u32")
        } else {
            0
        };
        file = file.ifd(
            at_pos,
            next,
            &[
                long(0x0201, u32::try_from(jpeg_at).expect("offset fits u32")),
                long(0x0202, jpeg_len),
            ],
        );
    }
    let file = file.put(jpeg_at, &jpeg).into_vec();

    assert_eq!(ok_info(file).previews.len(), 64);
}

#[test]
// C#: NefOrfRw2MutationGapTests.ReadOrf_OrientationOfSecondIfd_IsIgnored
fn read_orf_orientation_of_second_ifd_is_ignored() {
    let data = orf_file(128)
        .ifd(8, 60, &[short(0x0112, 6)])
        .ifd(60, 0, &[short(0x0112, 3)])
        .into_vec();

    assert_eq!(ok_info(data).orientation, 6);
}

#[test]
// C#: NefOrfRw2MutationGapTests.ReadOrf_SecondOrientationEntryInIfd0_OverridesOnlyWithinOneToEight (4 rows)
fn read_orf_second_orientation_entry_in_ifd0_overrides_only_within_one_to_eight() {
    for (second, expected) in [(1u32, 1), (8, 8), (0, 6), (9, 6)] {
        let data = orf_file(128)
            .ifd(8, 0, &[short(0x0112, 6), short(0x0112, second)])
            .into_vec();

        assert_eq!(ok_info(data).orientation, expected, "second orientation entry {second}");
    }
}

#[test]
// C#: NefOrfRw2MutationGapTests.ReadOrf_Ifd0WithAZeroDimension_ReportsNoSensorSize (2 rows)
fn read_orf_ifd0_with_a_zero_dimension_reports_no_sensor_size() {
    for (width, height) in [(0u32, 3000u32), (4000, 0)] {
        let data = orf_file(128)
            .ifd(8, 0, &[short(0x0100, width), short(0x0101, height)])
            .into_vec();

        let info = ok_info(data);

        assert_eq!(
            (info.sensor_width, info.sensor_height),
            (0, 0),
            "width {width}, height {height}"
        );
    }
}

#[test]
// C#: NefOrfRw2MutationGapTests.ReadOrf_SecondIfdSize_NeverReplacesIfd0Size
fn read_orf_second_ifd_size_never_replaces_ifd0_size() {
    let data = orf_file(160)
        .ifd(8, 60, &[short(0x0100, 4000), short(0x0101, 3000)])
        .ifd(60, 0, &[short(0x0100, 100), short(0x0101, 100)])
        .into_vec();

    let info = ok_info(data);

    assert_eq!((info.sensor_width, info.sensor_height), (4000, 3000));
}

// ---------------------------------------------------------------- ORF: MakerNote

#[test]
// C#: NefOrfRw2MutationGapTests.ReadOrf_MakerNoteTagInIfd0_IsAnOffsetOnlyWithMoreThanFourBytes (3 rows)
fn read_orf_maker_note_tag_in_ifd0_is_an_offset_only_with_more_than_four_bytes() {
    for (count, expected_previews) in [(8u32, 1usize), (5, 1), (4, 0)] {
        let jpeg = jpeg_640x480();
        let data = orf_file(400 + jpeg.len())
            .ifd(8, 0, &[at(0x927C, 7, count, 200)])
            .ifd(
                200,
                0,
                &[at(0x0100, 7, u32::try_from(jpeg.len()).expect("fits u32"), 400)],
            )
            .put(400, &jpeg)
            .into_vec();

        let previews = ok_info(data).previews;

        assert_eq!(previews.len(), expected_previews, "MakerNote count {count}");
        if expected_previews > 0 {
            assert_eq!(previews[0].offset, 400);
            assert_eq!(previews[0].length, i64::try_from(jpeg.len()).expect("fits i64"));
        }
    }
}

#[test]
// C#: NefOrfRw2MutationGapTests.ReadOrf_MakerNoteNearEndOfFile_IsParsedOnlyWhenMoreThanSixteenBytesRemain (2 rows)
fn read_orf_maker_note_near_end_of_file_is_parsed_only_when_more_than_sixteen_bytes_remain() {
    for (bytes_before_end, expected_previews) in [(17usize, 1usize), (16, 0)] {
        let jpeg = jpeg_640x480();
        const SIZE: usize = 600;
        let note = SIZE - bytes_before_end;
        let data = orf_file(SIZE)
            .ifd(8, 0, &[at(0x927C, 7, 8, u32::try_from(note).expect("fits u32"))])
            .put(100, &jpeg)
            .u16(note, 1)
            .u16(note + 2, 0x0100)
            .u16(note + 4, 7)
            .u32(note + 6, u32::try_from(jpeg.len()).expect("fits u32"))
            .u32(note + 10, 100)
            .into_vec();

        assert_eq!(
            ok_info(data).previews.len(),
            expected_previews,
            "{bytes_before_end} bytes before the end"
        );
    }
}

#[test]
// C#: NefOrfRw2MutationGapTests.ReadOrf_NoteThumbnailEntry_NeedsTagAndMoreThanFourBytes (3 rows)
fn read_orf_note_thumbnail_entry_needs_tag_and_more_than_four_bytes() {
    for (tag, count, expected_previews) in [(0x0100u16, 4u32, 0usize), (0x0100, 5, 1), (0x0200, 250, 0)] {
        let jpeg = jpeg_640x480();
        let data = orf_file(400 + jpeg.len())
            .ifd(8, 0, &[at(0x927C, 7, 8, 200)])
            .ifd(200, 0, &[at(tag, 7, count, 400)])
            .put(400, &jpeg)
            .into_vec();

        assert_eq!(
            ok_info(data).previews.len(),
            expected_previews,
            "tag {tag:#06x} count {count}"
        );
    }
}

#[test]
// C#: NefOrfRw2MutationGapTests.ReadOrf_NoteThumbnailOutsideTheFile_IsSkippedWithoutThrowing
fn read_orf_note_thumbnail_outside_the_file_is_skipped_without_throwing() {
    let data = orf_file(400)
        .ifd(8, 0, &[at(0x927C, 7, 8, 200)])
        .ifd(200, 0, &[at(0x0100, 7, 200, 0x7FFF_FF00)])
        .into_vec();

    assert!(ok_info(data).previews.is_empty());
}

#[test]
// C#: NefOrfRw2MutationGapTests.ReadOrf_CameraSettingsPreview_NeedsAtLeastFourBytes (2 rows)
fn read_orf_camera_settings_preview_needs_at_least_four_bytes() {
    for (length, expected_previews) in [(3u32, 0usize), (4, 1)] {
        let data = orf_file(600)
            .ifd(8, 0, &[at(0x927C, 7, 8, 200)])
            .ifd(200, 0, &[at(0x2020, 13, 1, 300)])
            .ifd(300, 0, &[long(0x0101, 500), long(0x0102, length)])
            .put(500, &[0xFF, 0xD8, 0xFF, 0xD9])
            .into_vec();

        let previews = ok_info(data).previews;

        assert_eq!(previews.len(), expected_previews, "CameraSettings length {length}");
        if expected_previews > 0 {
            assert_eq!((previews[0].offset, previews[0].length), (500, 4));
        }
    }
}

#[test]
// C#: NefOrfRw2MutationGapTests.ReadOrf_NoteThumbnailOfAnAlreadyListedJpeg_IsNotListedTwice
fn read_orf_note_thumbnail_of_an_already_listed_jpeg_is_not_listed_twice() {
    let jpeg = jpeg_640x480();
    let len = u32::try_from(jpeg.len()).expect("fits u32");
    let data = orf_file(400 + jpeg.len())
        .ifd(8, 0, &[at(0x927C, 7, 8, 200), long(0x0201, 400), long(0x0202, len)])
        .ifd(200, 0, &[at(0x0100, 7, len, 400)])
        .put(400, &jpeg)
        .into_vec();

    assert_eq!(ok_info(data).previews.len(), 1);
}

#[test]
// C#: NefOrfRw2MutationGapTests.ReadOrf_SelfRelativeNote_ByteOrderNeedsBothMarkBytes (2 rows)
fn read_orf_self_relative_note_byte_order_needs_both_mark_bytes() {
    const NOTE: usize = 200;
    const JPEG_AT: usize = 400;
    for (signature, ifd_delta, mark_at) in [(&b"OLYMPUS\0"[..], 12usize, 8usize), (&b"OM SYSTEM\0"[..], 16, 12)] {
        // The mark bytes are 'M','I': not "II", so the note is big-endian (the file itself is little-endian).
        let jpeg = jpeg_640x480();
        let note_bytes = TiffBytes::new(false, 64)
            .put(0, signature)
            .put(mark_at, b"MI")
            .ifd(
                ifd_delta,
                0,
                &[at(
                    0x0100,
                    7,
                    u32::try_from(jpeg.len()).expect("fits u32"),
                    u32::try_from(JPEG_AT - NOTE).expect("fits u32"),
                )],
            )
            .into_vec();
        let data = orf_file(JPEG_AT + jpeg.len())
            .ifd(8, 0, &[at(0x927C, 7, 64, u32::try_from(NOTE).expect("fits u32"))])
            .put(NOTE, &note_bytes)
            .put(JPEG_AT, &jpeg)
            .into_vec();

        let info = ok_info(data);

        assert_eq!(info.previews.len(), 1, "{signature:?}");
        assert_eq!(
            info.previews[0].offset,
            i64::try_from(JPEG_AT).expect("fits i64"),
            "{signature:?}"
        );
    }
}

// ---------------------------------------------------------------- header errors and byte orders (OrfRw2HeaderTests, ORF methods)

#[test]
// C#: OrfRw2HeaderTests.OrfRead_JunkHeader_ThrowsInvalidData
fn orf_read_junk_header_throws_invalid_data() {
    let junk = vec![0x5A; 64];

    assert_invalid_data(read_orf(junk), "junk header");
}

#[test]
// C#: OrfRw2HeaderTests.OrfRead_TiffByteOrderWithUnknownMagic_ThrowsInvalidData
fn orf_read_tiff_byte_order_with_unknown_magic_throws_invalid_data() {
    let data = TiffBytes::new(true, 200)
        .put(0, &[b'I', b'I', 0x34, 0x12])
        .u32(4, 8)
        .ifd(8, 0, &[])
        .into_vec();

    assert_invalid_data(read_orf(data), "magic 0x1234");
}

#[test]
// C#: OrfRw2HeaderTests.OrfRead_AcceptedLittleEndianMagics_Parse (3 rows: IIRO, IIRS, plain TIFF)
fn orf_read_accepted_little_endian_magics_parse() {
    for signature in [*b"IIRO", *b"IIRS", [b'I', b'I', 0x2A, 0x00]] {
        let data = TiffBytes::new(true, 200)
            .put(0, &signature)
            .u32(4, 8)
            .ifd(8, 0, &[short(0x0100, 4000), short(0x0101, 3000)])
            .into_vec();

        let info = ok_info(data);

        assert_eq!(info.sensor_width, 4000, "{signature:?}");
        assert_eq!(info.sensor_height, 3000, "{signature:?}");
    }
}

#[test]
// C#: OrfRw2HeaderTests.OrfRead_BigEndianMmor_Parses
fn orf_read_big_endian_mmor_parses() {
    let data = TiffBytes::new(false, 200)
        .put(0, b"MMOR")
        .u32(4, 8)
        .ifd(8, 0, &[short(0x0100, 4000), short(0x0101, 3000)])
        .into_vec();

    assert_eq!(ok_info(data).sensor_width, 4000);
}

#[test]
// C#: OrfRw2HeaderTests.OrfRead_ImageWidthFfffffff_ClampsInsteadOfWrappingNegative
fn orf_read_image_width_ffffffff_clamps_instead_of_wrapping_negative() {
    let data = TiffBytes::new(true, 200)
        .put(0, b"IIRO")
        .u32(4, 8)
        .ifd(8, 0, &[long(0x0100, 0xFFFF_FFFF), long(0x0101, 0xFFFF_FFFF)])
        .into_vec();

    let info = ok_info(data);

    assert_eq!(info.sensor_width, i32::MAX);
    assert_eq!(info.sensor_height, i32::MAX);
}

// ---------------------------------------------------------------- legacy MakerNote (OrfLegacyMakerNoteTests)

const LEGACY_NOTE_OFFSET: usize = 200;
const LEGACY_JPEG_OFFSET: usize = 400;

/// C# `OrfLegacyMakerNoteTests.Build`: IFD0 -> Exif IFD at 100 -> MakerNote at 200 (the legacy signature), thumbnail entry at +signature.
fn legacy_maker_note_file(little_endian: bool, signature: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let jpeg = create_minimal_jpeg(160, 120);
    let magic: &[u8; 4] = if little_endian { b"IIRO" } else { b"MMOR" };
    let file = TiffBytes::new(little_endian, LEGACY_JPEG_OFFSET + jpeg.len())
        .put(0, magic)
        .u32(4, 8)
        .ifd(8, 0, &[long(0x8769, 100)])
        .ifd(100, 0, &[at(0x927C, 7, 100, LEGACY_NOTE_OFFSET as u32)])
        .put(LEGACY_NOTE_OFFSET, signature)
        // ThumbnailImage (0x0100): UNDEFINED bytes at a file-absolute offset.
        .ifd(
            LEGACY_NOTE_OFFSET + signature.len(),
            0,
            &[at(
                0x0100,
                7,
                u32::try_from(jpeg.len()).expect("fits u32"),
                LEGACY_JPEG_OFFSET as u32,
            )],
        )
        .put(LEGACY_JPEG_OFFSET, &jpeg);
    (file.into_vec(), jpeg)
}

#[test]
// C#: OrfLegacyMakerNoteTests.OrfReader_LegacyOlympNote_ThumbnailOffsetIsFileAbsoluteAndIfdIsAtPlusEight (LE and BE)
fn orf_reader_legacy_olymp_note_thumbnail_offset_is_file_absolute_and_ifd_is_at_plus_eight() {
    for little_endian in [true, false] {
        let (data, jpeg) = legacy_maker_note_file(little_endian, b"OLYMP\0\x01\0");

        let info = ok_info(data);

        assert_eq!(info.previews.len(), 1, "little_endian {little_endian}");
        assert_eq!(
            info.previews[0].offset,
            i64::try_from(LEGACY_JPEG_OFFSET).expect("fits i64")
        );
        assert_eq!(info.previews[0].length, i64::try_from(jpeg.len()).expect("fits i64"));
    }
}

#[test]
// C#: OrfLegacyMakerNoteTests.OrfReader_LegacyOlympNote_DoesNotParseSignatureBytesAsIfd
fn orf_reader_legacy_olymp_note_does_not_parse_signature_bytes_as_ifd() {
    // Signature only, nothing at +8: the reader must not throw or invent a preview.
    let (mut data, _) = legacy_maker_note_file(true, b"OLYMP\0\x01\0");
    data[LEGACY_NOTE_OFFSET + 8..LEGACY_NOTE_OFFSET + 48].fill(0);

    assert!(ok_info(data).previews.is_empty());
}

// ---------------------------------------------------------------- TiffReaderRegressionTests: ORF section

/// C# `TiffReaderRegressionTests.BuildOrf`: Exif pointer -> MakerNote at 200 (count given), CameraSettings IFD at 300 (+100), JPEG at 600 (+400).
fn orf_camera_settings_file(note_signature: &[u8], maker_note_count: u32, preview_bytes: Option<&[u8]>) -> Vec<u8> {
    let baseline = jpeg_640x480();
    let preview = preview_bytes.unwrap_or(&baseline);
    const MAKER_NOTE: usize = 200;
    const SETTINGS_RELATIVE: u32 = 100;
    const PREVIEW_RELATIVE: u32 = 400;
    orf_file(MAKER_NOTE + PREVIEW_RELATIVE as usize + preview.len())
        .ifd(8, 0, &[long(0x8769, 100)])
        .ifd(100, 0, &[at(0x927C, 7, maker_note_count, MAKER_NOTE as u32)])
        .put(MAKER_NOTE, note_signature)
        .ifd(MAKER_NOTE + note_signature.len(), 0, &[long(0x2020, SETTINGS_RELATIVE)])
        .ifd(
            MAKER_NOTE + SETTINGS_RELATIVE as usize,
            0,
            &[
                long(0x0101, PREVIEW_RELATIVE),
                long(0x0102, u32::try_from(preview.len()).expect("fits u32")),
            ],
        )
        .put(MAKER_NOTE + PREVIEW_RELATIVE as usize, preview)
        .into_vec()
}

const OLYMPUS_SIGNATURE: &[u8] = b"OLYMPUS\0II\x03\0";
const OM_SYSTEM_SIGNATURE: &[u8] = b"OM SYSTEM\0\0\0II\x04\0";

#[test]
// C#: TiffReaderRegressionTests.OrfReader_CameraSettingsPreview_OffsetsAreRelativeToMakerNoteStart (Olympus and OM System rows)
fn orf_reader_camera_settings_preview_offsets_are_relative_to_maker_note_start() {
    let baseline = jpeg_640x480();
    for signature in [OLYMPUS_SIGNATURE, OM_SYSTEM_SIGNATURE] {
        let info = ok_info(orf_camera_settings_file(signature, 100, None));

        assert_eq!(info.previews.len(), 1, "{signature:?}");
        assert_eq!(info.previews[0].offset, 200 + 400, "{signature:?}");
        assert_eq!(
            info.previews[0].length,
            i64::try_from(baseline.len()).expect("fits i64"),
            "{signature:?}"
        );
    }
}

#[test]
// C#: TiffReaderRegressionTests.OrfReader_CameraSettingsPreviewNotJpeg_IsIgnored
fn orf_reader_camera_settings_preview_not_jpeg_is_ignored() {
    let not_jpeg = vec![0u8; jpeg_640x480().len()];

    let info = ok_info(orf_camera_settings_file(OLYMPUS_SIGNATURE, 100, Some(&not_jpeg)));

    assert!(info.previews.is_empty());
}

#[test]
// C#: TiffReaderRegressionTests.OrfReader_MakerNoteEntryHoldingInlineBytes_IsNotFollowedAsOffset
fn orf_reader_maker_note_entry_holding_inline_bytes_is_not_followed_as_offset() {
    // Count 4 means the four value bytes ARE the data; they must not be dereferenced as a MakerNote offset.
    let info = ok_info(orf_camera_settings_file(OLYMPUS_SIGNATURE, 4, None));

    assert!(info.previews.is_empty());
}

#[test]
// C#: TiffReaderRegressionTests.OrfReader_MakerNoteTruncatedNearEndOfFile_IsSkippedWithoutThrowing
fn orf_reader_maker_note_truncated_near_end_of_file_is_skipped_without_throwing() {
    // 16..31 bytes remain after the MakerNote start: less than the 32-byte header probe.
    let data = orf_file(300)
        .ifd(8, 0, &[long(0x8769, 100)])
        .ifd(100, 0, &[at(0x927C, 7, 100, 280)])
        .into_vec();

    assert!(ok_info(data).previews.is_empty());
}

#[test]
// C#: TiffReaderRegressionTests.OrfReader_UnprefixedMakerNoteInBigEndianFile_UsesFileByteOrderAndAbsoluteOffsets
fn orf_reader_unprefixed_maker_note_in_big_endian_file_uses_file_byte_order_and_absolute_offsets() {
    const MAKER_NOTE: usize = 200;
    let baseline = jpeg_640x480();
    let data = TiffBytes::new(false, 900 + baseline.len())
        .put(0, b"MMOR")
        .u32(4, 8)
        .ifd(8, 0, &[long(0x8769, 100)])
        .ifd(100, 0, &[at(0x927C, 7, 100, MAKER_NOTE as u32)])
        .ifd(MAKER_NOTE, 0, &[long(0x2020, 500)]) // absolute: no signature, so no MakerNote-relative base
        .ifd(
            500,
            0,
            &[
                long(0x0101, 900),
                long(0x0102, u32::try_from(baseline.len()).expect("fits u32")),
            ],
        )
        .put(900, &baseline)
        .into_vec();

    let info = ok_info(data);

    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].offset, 900);
}

#[test]
// C#: TiffReaderRegressionTests.Readers_ArrayEntryCountAboveInt32Max_DoNotThrowUnexpectedExceptions (the .orf row)
fn readers_array_entry_count_above_int32_max_do_not_throw_unexpected_exceptions() {
    let cr2_marker = *b"CR\x02\0\0\0\0\0";
    let data = TiffBytes::new(true, 512)
        .header(16)
        .put(8, &cr2_marker)
        .ifd(
            16,
            0,
            &[
                at(0x014A, 4, 0xFFFF_FFFF, 100),
                at(0x0111, 4, 0xFFFF_FFFF, 200),
                at(0x0117, 4, 0xFFFF_FFFF, 300),
            ],
        )
        .into_vec();

    // Result is Ok or an InvalidData error; a panic or another error kind fails the test.
    match read_orf(data) {
        Ok(_) | Err(RawHeaderError::InvalidData { .. }) => {}
        other => panic!("array entry count above i32::MAX escaped as {other:?}"),
    }
}

// ---------------------------------------------------------------- TiffFamilyReaderTests

#[test]
// C#: TiffFamilyReaderTests.OrfReader_Synthetic_CanReadHeader
fn orf_reader_synthetic_can_read_header() {
    let jpeg = create_minimal_jpeg(320, 240);
    let jpeg_pos = 40;
    let data = TiffBytes::new(true, jpeg_pos + jpeg.len())
        .put(0, b"IIRO")
        .u32(4, 8)
        .ifd(
            8,
            0,
            &[
                long(0x0201, jpeg_pos as u32),
                long(0x0202, u32::try_from(jpeg.len()).expect("fits u32")),
            ],
        )
        .put(jpeg_pos, &jpeg)
        .into_vec();

    let reader = OrfContainerReader::new();
    assert!(reader.can_read(&data[..data.len().min(64)], ".orf"));

    let info = ok_info(data);

    assert_eq!(info.format, pr_imaging_raw::RawFormat::Orf);
    assert_eq!(info.previews.len(), 1);
}

// ---------------------------------------------------------------- TiffPreviewHardeningTests (orf row)

const HARDENING_JPEG_AT: usize = 1000;

/// C# `TiffPreviewHardeningTests.Build` / `BuildWithJpeg` for `kind == "orf"`: IFD0 (orientation) -> IFD at 100 declaring 9000x6000
/// with a JPEG pointer at 1000 (declared length 200 when no JPEG is given).
fn hardening_file(jpeg: Option<&[u8]>) -> Vec<u8> {
    let declared = jpeg.map_or(200, <[u8]>::len);
    let size = match jpeg {
        None => 3000,
        Some(bytes) => HARDENING_JPEG_AT + bytes.len(),
    };
    let mut file = orf_file(size).ifd(8, 100, &[short(0x0112, 1)]).ifd(
        100,
        0,
        &[
            short(0x0100, 9000),
            short(0x0101, 6000),
            long(0x0201, HARDENING_JPEG_AT as u32),
            long(0x0202, u32::try_from(declared).expect("fits u32")),
        ],
    );
    if let Some(bytes) = jpeg {
        file = file.put(HARDENING_JPEG_AT, bytes);
    }
    file.into_vec()
}

/// A JPEG whose APP segments sit one 64 KB block apart: walking to the frame header charges more distinct blocks than the
/// 8 MiB header budget allows, so the walker fails with `InvalidData` before it reaches the SOF.
fn budget_exhausting_jpeg() -> Vec<u8> {
    const SEGMENTS: usize = 140;
    let tail = create_minimal_jpeg(320, 240);
    let mut out = vec![0xFF, 0xD8];
    let mut app = vec![0u8; 2 + 65535];
    app[0] = 0xFF;
    app[1] = 0xE0;
    app[2] = 0xFF;
    app[3] = 0xFF;
    for _ in 0..SEGMENTS {
        out.extend_from_slice(&app);
    }
    out.extend_from_slice(&tail[2..]);
    out
}

#[test]
// C#: TiffPreviewHardeningTests.Read_JpegInterchangePointerToZeroPadding_IsNotAPreview (orf row)
fn read_jpeg_interchange_pointer_to_zero_padding_is_not_a_preview() {
    let info = ok_info(hardening_file(None));

    assert!(!info.previews.iter().any(|p| p.offset == HARDENING_JPEG_AT as i64));
}

#[test]
// C#: TiffPreviewHardeningTests.Read_DeclaredSizeContradictingTheJpegFrame_UsesTheFrameSize (orf row)
fn read_declared_size_contradicting_the_jpeg_frame_uses_the_frame_size() {
    let jpeg = create_minimal_jpeg(320, 240);
    let info = ok_info(hardening_file(Some(&jpeg)));

    let preview = info
        .previews
        .iter()
        .find(|p| p.offset == HARDENING_JPEG_AT as i64)
        .expect("one preview at the JPEG pointer");
    assert_eq!((preview.width, preview.height), (320, 240));
}

#[test]
// C#: TiffPreviewHardeningTests.Read_PreviewJpegExhaustingHeaderBudget_ContainerStillParsesAndKeepsDeclaredSize (orf row)
fn read_preview_jpeg_exhausting_header_budget_container_still_parses_and_keeps_declared_size() {
    let info = ok_info(hardening_file(Some(&budget_exhausting_jpeg())));

    let matching: Vec<_> = info
        .previews
        .iter()
        .filter(|p| p.offset == HARDENING_JPEG_AT as i64)
        .collect();
    assert_eq!(matching.len(), 1, "one preview at the JPEG pointer");
    let preview = matching[0];
    assert!(matches!(preview.width, 0 | 9000), "width {}", preview.width);
    assert!(matches!(preview.height, 0 | 6000), "height {}", preview.height);
}

// ---------------------------------------------------------------- TiffExifBlockPlacementTests (orf row, reader half only)

const EXIF_MAKE_OFFSET: usize = 300_000;
const EXIF_IFD_OFFSET: usize = 200_100;

/// C# `TiffExifBlockPlacementTests.Build`: IFD0 at 8 (dimensions, Make far away, Exif pointer); the Exif IFD (ISO) sits far away too.
fn exif_placement_file(size: usize, make_offset: usize, exif_offset: usize) -> Vec<u8> {
    let make = u32::try_from(make_offset).expect("fits u32");
    let exif = u32::try_from(exif_offset).expect("fits u32");
    TiffBytes::new(true, size)
        .header(8)
        .ifd(
            8,
            0,
            &[
                short(0x0100, 4000),
                short(0x0101, 3000),
                at(0x010F, 2, 6, make),
                long(0x8769, exif),
            ],
        )
        .put(make_offset, b"Acme\0\0")
        .ifd(exif_offset, 0, &[short(0x8827, 400)])
        .into_vec()
}

#[test]
// C#: TiffExifBlockPlacementTests.TryReadExif_IfdsAfter128KiB_StillYieldsSummary (orf row, reader half: block offset and extent only)
fn try_read_exif_ifds_after_128kib_still_yields_summary() {
    let info = ok_info(exif_placement_file(310_000, EXIF_MAKE_OFFSET, EXIF_IFD_OFFSET));

    assert!(
        info.exif_blocks.iter().all(|b| b.offset == 0),
        "every block starts at 0: {:?}",
        info.exif_blocks
    );
    // The Exif IFD (2-byte count + one 12-byte entry + 4-byte next pointer) must lie inside the block.
    let far_ifd_end = i64::try_from(EXIF_IFD_OFFSET + 2 + 12 + 4).expect("fits i64");
    assert!(
        info.exif_blocks.iter().all(|b| b.length >= far_ifd_end),
        "{:?}",
        info.exif_blocks
    );
}

#[test]
// C#: TiffExifBlockPlacementTests.Read_IfdsInsideDefaultWindow_KeepsDefaultBlockLength (orf row)
fn read_ifds_inside_default_window_keeps_default_block_length() {
    let info = ok_info(exif_placement_file(310_000, 2000, 3000));

    let default = i64::from(limits::DEFAULT_EXIF_BLOCK_BYTES);
    assert!(
        info.exif_blocks.iter().all(|b| b.length == default),
        "{:?}",
        info.exif_blocks
    );
}

#[test]
// C#: TiffExifBlockPlacementTests.Read_IfdsBeyondMaxBlock_CapsBlockAndDoesNotThrow (orf row, block length assert only)
fn read_ifds_beyond_max_block_caps_block_and_does_not_throw() {
    let far = limits::MAX_EXIF_BLOCK_BYTES as usize + 50_000;
    let info = ok_info(exif_placement_file(far + 10_000, far, far + 100));

    // An extent that cannot fit keeps the default block (was grown to the 4 MiB cap for nothing).
    let default = i64::from(limits::DEFAULT_EXIF_BLOCK_BYTES);
    assert!(
        info.exif_blocks.iter().all(|b| b.length == default),
        "{:?}",
        info.exif_blocks
    );
}

// ---------------------------------------------------------------- OrfDisplayedSizeTests (synthetic rows)

/// C# `OrfDisplayedSizeTests.BuildOrf`: IFD0 with ImageWidth / ImageLength only.
fn displayed_size_file(width: u32, height: u32, little_endian: bool) -> Vec<u8> {
    let magic: &[u8; 4] = if little_endian { b"IIRO" } else { b"MMOR" };
    TiffBytes::new(little_endian, 200)
        .put(0, magic)
        .u32(4, 8)
        .ifd(8, 0, &[long(0x0100, width), long(0x0101, height)])
        .into_vec()
}

#[test]
// C#: OrfDisplayedSizeTests.Read_Ifd0RawFrame_ReportsTheSizeLibRawDecodes (5 rows x LE and BE)
fn read_ifd0_raw_frame_reports_the_size_libraw_decodes() {
    // The 4080 -> 4056 rule is exact, not a range: 4079 and 4081 stay as they are.
    assert_eq!(RAW_FRAME_WIDTH_WITH_MASKED_COLUMNS, 4080);
    assert_eq!(DECODED_WIDTH_WITHOUT_MASKED_COLUMNS, 4056);
    let rows = [
        (4080u32, 3040u32, 4056, 3040), // E-P3 / E-PL3 / E-PM1 raw frame: LibRaw drops the 24 masked columns
        (4640, 3472, 4640, 3472),       // E-M1: no rule, IFD0 == LibRaw
        (5220, 3912, 5220, 3912),       // OM-1: no rule, IFD0 == LibRaw
        (4079, 3040, 4079, 3040),
        (4081, 3040, 4081, 3040),
    ];
    for (ifd0_width, ifd0_height, width, height) in rows {
        for little_endian in [true, false] {
            let info = ok_info(displayed_size_file(ifd0_width, ifd0_height, little_endian));

            assert_eq!(
                (info.sensor_width, info.sensor_height),
                (width, height),
                "IFD0 {ifd0_width}x{ifd0_height}, little_endian {little_endian}"
            );
        }
    }
}

#[test]
// C#: OrfDisplayedSizeTests.Read_HeightIsNeverTouchedByTheWidthRule
fn read_height_is_never_touched_by_the_width_rule() {
    let info = ok_info(displayed_size_file(4056, 4080, true));

    assert_eq!(info.sensor_height, 4080);
}

// ---------------------------------------------------------------- Robustness: mutated ORF files

/// C# `RawContainerFuzzTests.Fuzz_OrfContainerReader_Robust` / `BinaryReaderFuzzTests` ORF target, Rust twin.
/// Deterministic xorshift mutator (flip 1-8 bytes, truncate, overwrite a u32 with 0 / 0xFFFFFFFF / 0x7FFFFFFF).
fn mutate(rng: &mut XorShift64, seed: &[u8]) -> Vec<u8> {
    const EXTREMES: [u32; 3] = [0, 0xFFFF_FFFF, 0x7FFF_FFFF];
    let mut bytes = seed.to_vec();
    match rng.below(3) {
        0 => {
            let flips = 1 + rng.below(8);
            for _ in 0..flips {
                let at_pos = rng.below(bytes.len());
                let mask = u8::try_from(1 + rng.below(255)).expect("1..=255 fits u8");
                bytes[at_pos] ^= mask;
            }
        }
        1 => {
            let keep = rng.below(bytes.len());
            bytes.truncate(keep);
        }
        _ => {
            if bytes.len() >= 4 {
                let at_pos = rng.below(bytes.len() - 3);
                let value = EXTREMES[rng.below(EXTREMES.len())];
                let raw = if rng.below(2) == 0 {
                    value.to_le_bytes()
                } else {
                    value.to_be_bytes()
                };
                bytes[at_pos..at_pos + 4].copy_from_slice(&raw);
            }
        }
    }
    bytes
}

#[test]
// C#: RawContainerFuzzTests.Fuzz_OrfContainerReader_Robust; BinaryReaderFuzzTests.Fuzz_RawContainerReaders_FailOnlyCleanly (ORF target)
fn orf_reader_fails_only_cleanly_on_mutated_files() {
    const MUTANTS_PER_SEED: usize = 5_000;
    let jpeg = create_minimal_jpeg(640, 480);
    // Seeds: the C# `SyntheticRawBuilder.BuildTiff(littleEndian, jpeg, 1, 0x4F52)` in both byte orders.
    let seeds = [
        build_tiff(true, Some(&jpeg), 1, 0x4F52),
        build_tiff(false, Some(&jpeg), 1, 0x4F52),
    ];

    for (index, seed) in seeds.iter().enumerate() {
        let mut rng = XorShift64::new(0x0F0A_0000 + index as u64);
        for _ in 0..MUTANTS_PER_SEED {
            let mutated = mutate(&mut rng, seed);
            let mutated_len = i64::try_from(mutated.len()).expect("fits i64");
            let mut source = InMemoryRawHeaderSource::new(mutated);
            let token = CancelToken::new();

            let outcome =
                std::panic::catch_unwind(AssertUnwindSafe(|| OrfContainerReader::new().read(&mut source, &token)));
            let result = outcome
                .unwrap_or_else(|_| panic!("seed {index}: ORF reader panicked on a mutant ({mutated_len} bytes)"));

            match result {
                Ok(info) => {
                    assert!((1..=8).contains(&info.orientation), "orientation {}", info.orientation);
                    assert!(
                        info.sensor_width >= 0 && info.sensor_height >= 0,
                        "negative sensor size"
                    );
                    for preview in &info.previews {
                        assert!(
                            preview.offset >= 0 && preview.length > 0 && preview.length <= mutated_len - preview.offset,
                            "preview {}+{} escapes the {mutated_len}-byte file",
                            preview.offset,
                            preview.length
                        );
                    }
                }
                Err(RawHeaderError::InvalidData { .. }) => {}
                Err(other) => panic!("seed {index}: undocumented error kind {other:?}"),
            }
            assert!(
                source.total_bytes_read() <= i64::from(limits::MAX_HEADER_BYTES),
                "header budget exceeded: {} > {}",
                source.total_bytes_read(),
                limits::MAX_HEADER_BYTES
            );
        }
    }
}

// ---------------------------------------------------------------- golden tests/golden/orf-container.json

/// Golden inputs (`tests/golden-inputs/orf-container.json`), in the golden file's order.
fn golden_inputs() -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("orf-container.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .expect("golden inputs have a cases array")
        .clone()
}

/// Applies repeated-byte patches from the oracle input format, which `pr_test_support::raw::golden_bytes` does not yet support.
fn orf_golden_bytes(node: &Value) -> Vec<u8> {
    if let Some(hex) = node["hex"].as_str() {
        return pr_test_support::raw::from_hex(hex);
    }
    let len = usize::try_from(node["len"].as_u64().expect("data len")).expect("length fits usize");
    let mut bytes = vec![0; len];
    for patch in node["patches"].as_array().into_iter().flatten() {
        let at = usize::try_from(patch["at"].as_u64().expect("patch at")).expect("patch offset fits usize");
        let value = pr_test_support::raw::from_hex(patch["hex"].as_str().expect("patch hex"));
        let times = usize::try_from(patch["times"].as_u64().unwrap_or(1)).expect("repeat count fits usize");
        let repeated = value.repeat(times);
        let total = repeated.len();
        let end = at.checked_add(total).expect("repeated patch end fits usize");
        bytes[at..end].copy_from_slice(&repeated);
    }
    bytes
}

fn kind_name(kind: EmbeddedPreviewKind) -> &'static str {
    match kind {
        EmbeddedPreviewKind::Jpeg => "Jpeg",
    }
}

fn color_space_name(space: PreviewColorSpace) -> &'static str {
    match space {
        PreviewColorSpace::Unknown => "Unknown",
        PreviewColorSpace::Srgb => "Srgb",
        PreviewColorSpace::AdobeRgb => "AdobeRgb",
    }
}

/// Runs one `read` golden input and returns the fields the golden file records.
fn run_read_case(bytes: Vec<u8>) -> Value {
    let head = &bytes[..bytes.len().min(64)];
    let can_read = OrfContainerReader::new().can_read(head, ".orf");
    let mut out = Map::new();
    out.insert("can_read".to_owned(), json!(can_read));
    match read_orf(bytes) {
        Ok(info) => {
            out.insert("ok".to_owned(), json!(true));
            out.insert("error".to_owned(), Value::Null);
            out.insert("message".to_owned(), Value::Null);
            out.insert("format".to_owned(), json!("Orf"));
            out.insert("sensor_width".to_owned(), json!(info.sensor_width));
            out.insert("sensor_height".to_owned(), json!(info.sensor_height));
            out.insert("orientation".to_owned(), json!(info.orientation));
            let previews: Vec<Value> = info
                .previews
                .iter()
                .map(|p| {
                    json!({
                        "index": p.index,
                        "offset": p.offset,
                        "length": p.length,
                        "kind": kind_name(p.kind),
                        "width": p.width,
                        "height": p.height,
                        "color_space": color_space_name(p.color_space),
                        "header_resolved": p.header_resolved,
                    })
                })
                .collect();
            out.insert("previews".to_owned(), Value::Array(previews));
            let blocks: Vec<Value> = info
                .exif_blocks
                .iter()
                .map(|b| {
                    json!({
                        "offset": b.offset,
                        "length": b.length,
                        "is_tiff_header": b.is_tiff_header,
                        "ifd_is_exif": b.ifd_is_exif,
                    })
                })
                .collect();
            out.insert("exif_blocks".to_owned(), Value::Array(blocks));
        }
        Err(error) => {
            out.insert("ok".to_owned(), json!(false));
            let kind = match &error {
                RawHeaderError::InvalidData { .. } => "InvalidDataException".to_owned(),
                other => format!("{other:?}"),
            };
            out.insert("error".to_owned(), json!(kind));
            out.insert("message".to_owned(), json!(error.to_string()));
        }
    }
    Value::Object(out)
}

/// Runs one golden input and returns the fields to compare against the expected case.
fn run_golden_case(input: &Value) -> Value {
    let bytes = orf_golden_bytes(&input["data"]);
    match input["op"].as_str().expect("golden input op") {
        "can-read" => {
            let extension = input["ext"].as_str().expect("can-read ext");
            let head = &bytes[..bytes.len().min(64)];
            json!({ "can_read": OrfContainerReader::new().can_read(head, extension) })
        }
        "read" => run_read_case(bytes),
        other => panic!("unexpected golden op {other}"),
    }
}

#[test]
// Golden `tests/golden/orf-container.json` (233 cases: 33 can-read, 197 read, 3 corpus). Reports every failing case by name.
fn golden_orf_container() {
    let golden_file = golden::load("orf-container");
    let inputs = golden_inputs();
    assert_eq!(
        golden_file.cases.len(),
        inputs.len(),
        "golden and input case counts differ"
    );

    let mut mismatches = Vec::new();
    let mut skipped_corpus = 0usize;
    for (expected_case, input) in golden_file.cases.iter().zip(&inputs) {
        let name = golden::str_of(expected_case, "name");
        assert_eq!(
            name,
            golden::str_of(input, "name"),
            "case order differs from the input file"
        );

        if golden::str_of(expected_case, "op") == "corpus" {
            if expected_case["present"] == Value::Bool(false) {
                skipped_corpus += 1;
                continue;
            }
            mismatches.push(format!(
                "[{name}] corpus case present, but the pr_test_support::raw_corpus helper is BLOCKED (not ported)"
            ));
            continue;
        }

        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| run_golden_case(input)));
        let Ok(actual) = outcome else {
            mismatches.push(format!("[{name}] panicked (todo!() or a reader defect)"));
            continue;
        };

        let expected = expected_case.as_object().expect("golden case object");
        for (key, value) in expected {
            if matches!(key.as_str(), "name" | "op" | "ext") {
                continue;
            }
            if actual.get(key) != Some(value) {
                mismatches.push(format!(
                    "[{name}] {key}: expected {value}, got {}",
                    actual.get(key).unwrap_or(&Value::Null)
                ));
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} of {} orf-container golden cases differ from C# ({skipped_corpus} corpus cases skipped):\n{}",
        mismatches.len(),
        golden_file.cases.len(),
        mismatches.join("\n")
    );
}
