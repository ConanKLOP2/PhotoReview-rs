//! Red tests for W5-06 (`readers/dng.rs`): `DngContainerReader` (C# baseline d92d6aae).
//!
//! Ported from `tests/PhotoReview.Imaging.Tests/Raw/Tiff/`: `DngCropSizeTests.cs`, `DngPrimaryIfdTests.cs` (synthetic
//! rows), `DngMutationGapTests.cs`, `JpegProbeAndDngChainTests.cs`, `TiffReaderRegressionTests.cs`,
//! `TiffSubIfdTypeTests.cs`, `TiffFamilyReaderTests.cs`, `TiffStripPreviewTests.cs`, `TiffPreviewHardeningTests.cs` and
//! `TiffExifBlockPlacementTests.cs` (the DNG rows only), plus the DNG rows of `Robustness/RawContainerFuzzTests.cs` and
//! `Robustness/BinaryReaderFuzzTests.cs`. Each test names its C# twin in a comment; `// Rust-only:` marks new tests.
//! The corpus twins are in `dng_reader_corpus.rs` (not written: `pr_test_support::RawCorpus` is missing) and the golden
//! in `dng_reader_golden.rs`.
//!
//! Stub bodies are `todo!()`, so every test here fails until the porter fills them in.

use std::io;

use pr_core::sync::CancelToken;
use pr_imaging_raw::formats::limits::{DEFAULT_EXIF_BLOCK_BYTES, MAX_EXIF_BLOCK_BYTES};
use pr_imaging_raw::readers::dng::DngContainerReader;
use pr_imaging_raw::{
    EmbeddedPreview, InMemoryRawHeaderSource, RawContainerInfo, RawContainerReader, RawFormat, RawHeaderError,
    RawHeaderSource,
};
use pr_test_support::raw::{Entry, TiffBytes, at, build_tiff, create_minimal_jpeg, long, mutants, short};

/// PhotometricInterpretation CFA (32803).
const CFA: u32 = 32803;
/// PhotometricInterpretation LinearRaw (34892).
const LINEAR_RAW: u32 = 34892;
/// PhotometricInterpretation RGB (2).
const RGB: u32 = 2;
/// The two reader texts (C# `InvalidDataException` messages of `DngContainerReader`).
const TOO_SHORT_MESSAGE: &str = "File too short for DNG container.";
const INVALID_HEADER_MESSAGE: &str = "Invalid DNG TIFF header.";

// ---------------------------------------------------------------- helpers

fn read_bytes(bytes: Vec<u8>) -> Result<RawContainerInfo, RawHeaderError> {
    let mut source = InMemoryRawHeaderSource::new(bytes);
    DngContainerReader::new().read(&mut source, &CancelToken::new())
}

fn read_ok(bytes: Vec<u8>) -> RawContainerInfo {
    match read_bytes(bytes) {
        Ok(info) => info,
        Err(error) => panic!("DNG reader rejected a valid layout: {error:?}"),
    }
}

fn invalid_message(result: Result<RawContainerInfo, RawHeaderError>) -> String {
    match result {
        Err(RawHeaderError::InvalidData { message, .. }) => message,
        other => panic!("expected InvalidData, got {other:?}"),
    }
}

fn sensor(info: &RawContainerInfo) -> (i32, i32) {
    (info.sensor_width, info.sensor_height)
}

fn only_preview(info: &RawContainerInfo) -> &EmbeddedPreview {
    assert_eq!(
        info.previews.len(),
        1,
        "expected exactly one preview: {:?}",
        info.previews
    );
    &info.previews[0]
}

/// Width, height and photometric entries of an image IFD (C# `Image(width, height, photometric)`).
fn image(width: u32, height: u32, photometric: u32) -> Vec<Entry> {
    vec![short(0x0100, width), short(0x0101, height), short(0x0106, photometric)]
}

/// C# `TiffBytes.AsLosslessJpeg`: the first SOF0 becomes SOF3. Local copy: `pr_test_support::raw` has no such helper yet.
fn as_lossless_jpeg(jpeg: &[u8]) -> Vec<u8> {
    let mut out = jpeg.to_vec();
    let sof0 = jpeg.windows(2).position(|w| w == [0xFF, 0xC0]).expect("SOF0 present");
    out[sof0 + 1] = 0xC3;
    out
}

/// C# `Bytes(jpeg, fill)` of the strip tests: `fill` 0xFF bytes after SOI, before the next marker.
fn with_fill_bytes(jpeg: &[u8], fill: usize) -> Vec<u8> {
    let mut out = jpeg[..2].to_vec();
    out.extend(vec![0xFF; fill]);
    out.extend_from_slice(&jpeg[2..]);
    out
}

// ---------------------------------------------------------------- CanRead

// C#: DngMutationGapTests.CanRead_FourByteTiffHeader_IsAccepted (2 rows: little and big endian, plus the 3-byte slice
// and the `.tif` extension)
#[test]
fn can_read_four_byte_tiff_header_is_accepted() {
    let reader = DngContainerReader::new();
    for little in [true, false] {
        let head: [u8; 4] = if little {
            [0x49, 0x49, 0x2A, 0x00]
        } else {
            [0x4D, 0x4D, 0x00, 0x2A]
        };
        assert!(
            reader.can_read(&head, ".dng"),
            "little={little}: 4-byte header accepted"
        );
        assert!(
            !reader.can_read(&head[..3], ".dng"),
            "little={little}: 3-byte slice rejected"
        );
        assert!(
            !reader.can_read(&head, ".tif"),
            "little={little}: .tif extension rejected"
        );
    }
}

// C#: DngMutationGapTests.CanRead_AnySingleSignatureByteWrong_IsRejected (8 rows: byte order x signature position)
#[test]
fn can_read_any_single_signature_byte_wrong_is_rejected() {
    let reader = DngContainerReader::new();
    for little in [true, false] {
        for position in 0..4 {
            let mut head: [u8; 4] = if little {
                [0x49, 0x49, 0x2A, 0x00]
            } else {
                [0x4D, 0x4D, 0x00, 0x2A]
            };
            head[position] ^= 0x40;
            assert!(!reader.can_read(&head, ".dng"), "little={little} position={position}");
        }
    }
}

// Rust-only: can_read on an empty slice and on 1..3 bytes is false, whatever the extension.
#[test]
fn can_read_empty_and_short_slices_are_false() {
    let reader = DngContainerReader::new();
    assert!(!reader.can_read(&[], ".dng"));
    for n in 1..=3 {
        assert!(!reader.can_read(&[0x49, 0x49, 0x2A][..n], ".dng"), "{n} bytes");
    }
}

// Rust-only: the reader is Send + Sync and usable as Box<dyn RawContainerReader>.
#[test]
fn dng_reader_is_send_sync_and_usable_as_boxed_trait_object() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DngContainerReader>();
    let boxed: Box<dyn RawContainerReader> = Box::new(DngContainerReader::new());
    assert!(boxed.can_read(b"II*\0", ".dng"));
}

// ---------------------------------------------------------------- header and prologue

// Rust-only: a source of exactly 15 bytes is "too short"; 16 bytes is the smallest accepted file (the exact text).
#[test]
fn length_15_is_too_short_and_16_is_accepted() {
    let too_short = TiffBytes::new(true, 15).header(8).into_vec();
    assert_eq!(invalid_message(read_bytes(too_short)), TOO_SHORT_MESSAGE);

    let smallest = TiffBytes::new(true, 16).header(8).ifd(8, 0, &[]).into_vec();
    let info = read_ok(smallest);
    assert_eq!(sensor(&info), (0, 0));
    assert_eq!(info.orientation, 1);
    assert!(info.previews.is_empty());
}

// C#: DngMutationGapTests.Read_SixteenByteFileWithEmptyIfd_ReturnsEmptyInfo
#[test]
fn read_sixteen_byte_file_with_empty_ifd_returns_empty_info() {
    let info = read_ok(TiffBytes::new(true, 16).header(8).ifd(8, 0, &[]).into_vec());

    assert_eq!(sensor(&info), (0, 0));
    assert_eq!(info.orientation, 1);
    assert!(info.previews.is_empty());
}

// C#: DngMutationGapTests.Read_ValidByteOrderButMagicFortyThree_Throws
#[test]
fn read_valid_byte_order_but_magic_forty_three_throws() {
    let data = TiffBytes::new(true, 64)
        .header(8)
        .put(0, b"II+\0")
        .ifd(8, 0, &image(100, 100, CFA))
        .into_vec();

    assert_eq!(invalid_message(read_bytes(data)), INVALID_HEADER_MESSAGE);
}

// ---------------------------------------------------------------- chain and SubIFD caps

// C#: DngMutationGapTests.Read_ChainOfSixtySixIfds_ConsidersOnlyTheFirstSixtyFive
#[test]
fn read_chain_of_sixty_six_ifds_considers_only_the_first_sixty_five() {
    // IFD i is 100+i wide: the chain stops after 64 IFDs, its last next-pointer registers the 65th, the 66th is never read.
    const IFD_SIZE: usize = 42;
    let mut file = TiffBytes::new(true, 8 + 66 * IFD_SIZE).header(8);
    for i in 0..66 {
        let pos = 8 + i * IFD_SIZE;
        let next = if i < 65 { (pos + IFD_SIZE) as u32 } else { 0 };
        file = file.ifd(pos, next, &image(100 + i as u32, 100, CFA));
    }

    let info = read_ok(file.into_vec());

    assert_eq!(sensor(&info), (164, 100));
}

// C#: DngMutationGapTests.Read_SixtyFiveSubIfdsAcrossTwoEntries_IgnoresTheSixtyFifth
#[test]
fn read_sixty_five_sub_ifds_across_two_entries_ignores_the_sixty_fifth() {
    const IFD_SIZE: usize = 42;
    const ARRAY_AT: usize = 80;
    const CHAIN_IFD_AT: usize = 340;
    const SUBS_AT: usize = 400;
    let mut entries = image(10, 10, RGB);
    entries.push(at(0x014A, 4, 64, ARRAY_AT as u32));
    let mut file = TiffBytes::new(true, SUBS_AT + 65 * IFD_SIZE + 8)
        .header(8)
        .ifd(8, CHAIN_IFD_AT as u32, &entries)
        .ifd(CHAIN_IFD_AT, 0, &[long(0x014A, (SUBS_AT + 64 * IFD_SIZE) as u32)]);
    for i in 0..64 {
        file = file.u32(ARRAY_AT + i * 4, (SUBS_AT + i * IFD_SIZE) as u32);
    }
    for i in 0..64 {
        file = file.ifd(SUBS_AT + i * IFD_SIZE, 0, &image(50, 50, CFA));
    }
    file = file.ifd(SUBS_AT + 64 * IFD_SIZE, 0, &image(5000, 4000, CFA));

    let info = read_ok(file.into_vec());

    assert_eq!(sensor(&info), (50, 50));
}

// C#: DngMutationGapTests.Read_NextIfdPointerEqualToARegisteredSubIfd_ParsesThatIfdOnce
#[test]
fn read_next_ifd_pointer_equal_to_a_registered_sub_ifd_parses_that_ifd_once() {
    let jpeg = create_minimal_jpeg(640, 480);
    let mut ifd0 = image(4000, 3000, CFA);
    ifd0.push(long(0x014A, 200));
    let data = TiffBytes::new(true, 400 + jpeg.len())
        .header(8)
        .ifd(8, 200, &ifd0)
        .ifd(
            200,
            0,
            &[
                short(0x0100, 640),
                short(0x0101, 480),
                long(0x0201, 400),
                long(0x0202, jpeg.len() as u32),
            ],
        )
        .put(400, &jpeg)
        .into_vec();

    let info = read_ok(data);

    let preview = only_preview(&info);
    assert_eq!((preview.width, preview.height), (640, 480));
    assert_eq!(sensor(&info), (4000, 3000));
}

// ---------------------------------------------------------------- orientation

// C#: DngMutationGapTests.Read_SecondOrientationEntryInIfd0_OverridesOnlyWithinOneToEight (4 rows)
#[test]
fn read_second_orientation_entry_in_ifd0_overrides_only_within_one_to_eight() {
    for (second, expected) in [(1u32, 1), (8, 8), (0, 6), (9, 6)] {
        let mut entries = image(100, 100, CFA);
        entries.push(short(0x0112, 6));
        entries.push(short(0x0112, second));
        let data = TiffBytes::new(true, 128).header(8).ifd(8, 0, &entries).into_vec();

        assert_eq!(read_ok(data).orientation, expected, "second orientation {second}");
    }
}

// ---------------------------------------------------------------- primary IFD choice

// C#: DngMutationGapTests.Read_RawIfdWithAZeroDimension_NeverBecomesThePrimaryImage (2 rows)
#[test]
fn read_raw_ifd_with_a_zero_dimension_never_becomes_the_primary_image() {
    for (width, height) in [(0u32, 100u32), (100, 0)] {
        let data = TiffBytes::new(true, 256)
            .header(8)
            .ifd(8, 120, &image(4000, 3000, RGB))
            .ifd(120, 0, &image(width, height, CFA))
            .into_vec();

        assert_eq!(sensor(&read_ok(data)), (4000, 3000), "raw IFD {width}x{height}");
    }
}

// C#: DngMutationGapTests.Read_TwoRawIfdsOfEqualArea_KeepsTheFirst
#[test]
fn read_two_raw_ifds_of_equal_area_keeps_the_first() {
    let data = TiffBytes::new(true, 256)
        .header(8)
        .ifd(8, 120, &image(4000, 3000, CFA))
        .ifd(120, 0, &image(3000, 4000, CFA))
        .into_vec();

    assert_eq!(sensor(&read_ok(data)), (4000, 3000));
}

// C#: DngMutationGapTests.Read_SmallerNonRawIfdAfterLargerNonRawIfd_KeepsTheLarger
#[test]
fn read_smaller_non_raw_ifd_after_larger_non_raw_ifd_keeps_the_larger() {
    let data = TiffBytes::new(true, 256)
        .header(8)
        .ifd(8, 120, &image(4000, 3000, RGB))
        .ifd(120, 0, &image(100, 100, RGB))
        .into_vec();

    assert_eq!(sensor(&read_ok(data)), (4000, 3000));
}

// C#: DngMutationGapTests.Read_RawIfdFollowedBySmallerNonRawIfd_KeepsTheRaw
#[test]
fn read_raw_ifd_followed_by_smaller_non_raw_ifd_keeps_the_raw() {
    let data = TiffBytes::new(true, 256)
        .header(8)
        .ifd(8, 120, &image(100, 100, CFA))
        .ifd(120, 0, &image(4000, 3000, RGB))
        .into_vec();

    assert_eq!(sensor(&read_ok(data)), (100, 100));
}

// C#: DngMutationGapTests.Read_OnlyPreviewIfds_SensorSizeIsTheLargestPreview (2 rows: largest first / second)
#[test]
fn read_only_preview_ifds_sensor_size_is_the_largest_preview() {
    for largest_first in [true, false] {
        let small = create_minimal_jpeg(320, 240);
        let large = create_minimal_jpeg(640, 480);
        let (first, second) = if largest_first {
            (&large, &small)
        } else {
            (&small, &large)
        };
        let (first_size, second_size) = if largest_first {
            ((640, 480), (320, 240))
        } else {
            ((320, 240), (640, 480))
        };
        let data = TiffBytes::new(true, 600 + first.len() + second.len())
            .header(8)
            .ifd(
                8,
                120,
                &[
                    short(0x0100, first_size.0),
                    short(0x0101, first_size.1),
                    long(0x0201, 600),
                    long(0x0202, first.len() as u32),
                ],
            )
            .ifd(
                120,
                0,
                &[
                    short(0x0100, second_size.0),
                    short(0x0101, second_size.1),
                    long(0x0201, (600 + first.len()) as u32),
                    long(0x0202, second.len() as u32),
                ],
            )
            .put(600, first)
            .put(600 + first.len(), second)
            .into_vec();

        let info = read_ok(data);

        assert_eq!(info.previews.len(), 2, "largest_first={largest_first}");
        assert_eq!(sensor(&info), (640, 480), "largest_first={largest_first}");
    }
}

// ---------------------------------------------------------------- DefaultCropSize and DefaultScale

/// C# `DngCropSizeTests.SensorSize`: a 4032x3024 raw IFD with DefaultCropSize (0xC620) and optional DefaultScale (0xC61E).
fn crop_sensor_size(crop: (u32, u32), scale: Option<(u32, u32, u32, u32)>) -> (i32, i32) {
    const RAW_WIDTH: u32 = 4032;
    const RAW_HEIGHT: u32 = 3024;
    let mut entries = vec![
        long(0x00FE, 0),
        short(0x0100, RAW_WIDTH),
        short(0x0101, RAW_HEIGHT),
        short(0x0106, CFA),
        at(0xC620, 4, 2, 200),
    ];
    if scale.is_some() {
        entries.push(at(0xC61E, 5, 2, 300));
    }
    let mut file = TiffBytes::new(true, 500)
        .header(8)
        .ifd(8, 0, &entries)
        .u32(200, crop.0)
        .u32(204, crop.1);
    if let Some((h_num, h_den, v_num, v_den)) = scale {
        file = file.u32(300, h_num).u32(304, h_den).u32(308, v_num).u32(312, v_den);
    }
    sensor(&read_ok(file.into_vec()))
}

// C#: DngCropSizeTests.DefaultCropSize_SmallerThanRawIfd_IsUsed
#[test]
fn default_crop_size_smaller_than_raw_ifd_is_used() {
    assert_eq!(crop_sensor_size((4000, 3000), None), (4000, 3000));
}

// C#: DngCropSizeTests.DefaultCropSize_LargerThanRawIfd_IsIgnored (3 rows)
#[test]
fn default_crop_size_larger_than_raw_ifd_is_ignored() {
    for crop in [(5000, 3000), (4000, 3100), (9000, 9000)] {
        assert_eq!(crop_sensor_size(crop, None), (4032, 3024), "crop {crop:?}");
    }
}

// C#: DngCropSizeTests.DefaultScale_OneToOne_KeepsTheCrop
#[test]
fn default_scale_one_to_one_keeps_the_crop() {
    assert_eq!(crop_sensor_size((4000, 3000), Some((1, 1, 2, 2))), (4000, 3000));
}

// C#: DngCropSizeTests.DefaultScale_NotSquare_FallsBackToRawIfdSize (3 rows: wide pixels, tall pixels, zero denominator)
#[test]
fn default_scale_not_square_falls_back_to_raw_ifd_size() {
    for scale in [(2, 1, 1, 1), (1, 1, 3, 2), (1, 0, 1, 1)] {
        assert_eq!(
            crop_sensor_size((4000, 3000), Some(scale)),
            (4032, 3024),
            "scale {scale:?}"
        );
    }
}

// C#: DngCropSizeTests.DefaultCropSize (Rust-only companion of the crop-fit rule): a crop with no declared raw size is
// accepted (the golden `crop-with-undeclared-raw-size-is-accepted` covers the same rule).
// Rust-only: a crop of exactly the raw size is used (fit is inclusive: `cw <= width && ch <= height`).
#[test]
fn default_crop_size_equal_to_raw_ifd_is_used() {
    assert_eq!(crop_sensor_size((4032, 3024), None), (4032, 3024));
}

// ---------------------------------------------------------------- primary IFD (DngPrimaryIfdTests, synthetic rows)

/// C# `DngPrimaryIfdTests.Build`: IFD0 is the CFA raw image 4000x3000 with a SubIFD at 200 holding `other`.
fn raw_plus_sub_ifd(other: &[Entry]) -> Vec<u8> {
    TiffBytes::new(true, 500)
        .header(8)
        .ifd(
            8,
            0,
            &[
                long(0x00FE, 0),
                short(0x0100, 4000),
                short(0x0101, 3000),
                short(0x0103, 7),
                short(0x0106, CFA),
                long(0x014A, 200),
            ],
        )
        .ifd(200, 0, other)
        .into_vec()
}

// C#: DngPrimaryIfdTests.SecondaryImageIfd_LargerThanRawIfd_DoesNotWin (4 rows: masks, 0x8, 0x10 LinearRaw, reduced 0x1)
#[test]
fn secondary_image_ifd_larger_than_raw_ifd_does_not_win() {
    for (sub_file_type, photometric) in [(4u32, CFA), (8, CFA), (0x10, LINEAR_RAW), (1, CFA)] {
        let other = [
            long(0x00FE, sub_file_type),
            short(0x0100, 6000),
            short(0x0101, 4500),
            short(0x0103, 1),
            short(0x0106, photometric),
        ];
        assert_eq!(
            sensor(&read_ok(raw_plus_sub_ifd(&other))),
            (4000, 3000),
            "subfiletype {sub_file_type:#x}"
        );
    }
}

// C#: DngPrimaryIfdTests.NonRawIfdWithoutSubFileType_LargerThanRawIfd_DoesNotWin
#[test]
fn non_raw_ifd_without_sub_file_type_larger_than_raw_ifd_does_not_win() {
    let other = [
        short(0x0100, 6000),
        short(0x0101, 4500),
        short(0x0103, 1),
        short(0x0106, RGB),
    ];

    assert_eq!(sensor(&read_ok(raw_plus_sub_ifd(&other))), (4000, 3000));
}

// C#: DngPrimaryIfdTests.RawIfdOutranksLargerNonRawIfd_RegardlessOfOrder
#[test]
fn raw_ifd_outranks_larger_non_raw_ifd_regardless_of_order() {
    // The non-raw IFD comes first in the chain, the CFA IFD second.
    let data = TiffBytes::new(true, 500)
        .header(8)
        .ifd(
            8,
            200,
            &[
                long(0x00FE, 0),
                short(0x0100, 6000),
                short(0x0101, 4500),
                short(0x0103, 1),
                short(0x0106, RGB),
            ],
        )
        .ifd(
            200,
            0,
            &[
                long(0x00FE, 0),
                short(0x0100, 4000),
                short(0x0101, 3000),
                short(0x0103, 7),
                short(0x0106, CFA),
            ],
        )
        .into_vec();

    assert_eq!(sensor(&read_ok(data)), (4000, 3000));
}

// C#: DngPrimaryIfdTests.LargerRawIfd_StillWinsOverSmallerRawIfd
#[test]
fn larger_raw_ifd_still_wins_over_smaller_raw_ifd() {
    let data = TiffBytes::new(true, 500)
        .header(8)
        .ifd(
            8,
            200,
            &[
                long(0x00FE, 0),
                short(0x0100, 100),
                short(0x0101, 80),
                short(0x0106, CFA),
            ],
        )
        .ifd(
            200,
            0,
            &[
                long(0x00FE, 0),
                short(0x0100, 4000),
                short(0x0101, 3000),
                short(0x0106, LINEAR_RAW),
            ],
        )
        .into_vec();

    assert_eq!(sensor(&read_ok(data)), (4000, 3000));
}

// ---------------------------------------------------------------- JPEG strip previews and the DNG chain

const STRIP_AT: usize = 1000;

/// C# `JpegProbeAndDngChainTests.ReadStripDng`: IFD0 is a compression-7 single-strip 320x240 image holding `jpeg`.
fn strip_dng(jpeg: &[u8]) -> Vec<u8> {
    TiffBytes::new(true, STRIP_AT + jpeg.len() + 16)
        .header(8)
        .ifd(
            8,
            0,
            &[
                long(0x00FE, 0),
                short(0x0100, 320),
                short(0x0101, 240),
                short(0x0103, 7),
                short(0x0106, 2),
                long(0x0111, STRIP_AT as u32),
                long(0x0117, jpeg.len() as u32),
            ],
        )
        .put(STRIP_AT, jpeg)
        .into_vec()
}

// C#: JpegProbeAndDngChainTests.StripJpeg_WithFillBytesBeforeMarker_IsStillAPreview (3 rows: 0, 600, 5000 fill bytes)
#[test]
fn strip_jpeg_with_fill_bytes_before_marker_is_still_a_preview() {
    for fill in [0, 600, 5000] {
        let jpeg = with_fill_bytes(&create_minimal_jpeg(320, 240), fill);

        let info = read_ok(strip_dng(&jpeg));

        assert_eq!(
            info.previews.iter().filter(|p| p.offset == STRIP_AT as i64).count(),
            1,
            "fill {fill}"
        );
    }
}

// C#: JpegProbeAndDngChainTests.StripJpeg_With12BitPrecision_IsNotAPreview
#[test]
fn strip_jpeg_with_12_bit_precision_is_not_a_preview() {
    let mut jpeg = create_minimal_jpeg(320, 240);
    let sof = jpeg.iter().position(|&b| b == 0xC0).expect("SOF0 present") - 1;
    assert_eq!(jpeg[sof + 4], 0x08);
    jpeg[sof + 4] = 12;

    let info = read_ok(strip_dng(&jpeg));

    assert!(
        info.previews.iter().all(|p| p.offset != STRIP_AT as i64),
        "{:?}",
        info.previews
    );
}

// C#: JpegProbeAndDngChainTests.Dng_SixtyFourSubIfds_DoNotStopTheIfdChainEarly
#[test]
fn dng_sixty_four_sub_ifds_do_not_stop_the_ifd_chain_early() {
    const SUB_COUNT: usize = 64;
    let mut file = TiffBytes::new(true, 4000)
        .header(8)
        .ifd(8, 2000, &[at(0x014A, 4, SUB_COUNT as u32, 1000)])
        .ifd(2000, 2100, &[short(0x0100, 100), short(0x0101, 80)])
        .ifd(
            2100,
            0,
            &[
                long(0x00FE, 0),
                short(0x0100, 4000),
                short(0x0101, 3000),
                short(0x0106, CFA),
            ],
        );
    for i in 0..SUB_COUNT {
        file = file.u32(1000 + i * 4, (2200 + i * 8) as u32).ifd(2200 + i * 8, 0, &[]);
    }

    let info = read_ok(file.into_vec());

    assert_eq!(sensor(&info), (4000, 3000));
}

// ---------------------------------------------------------------- TiffReaderRegressionTests (DNG rows)

/// C# `TiffReaderRegressionTests.BuildDngWithLosslessRawAndPreview`: a lossless-JPEG raw SubIFD (bigger than the
/// preview) and a baseline 640x480 preview SubIFD. Returns the file and the preview offset.
fn dng_with_lossless_raw_and_preview(crop_as_rational: bool) -> (Vec<u8>, usize) {
    let baseline = create_minimal_jpeg(640, 480);
    let lossless = as_lossless_jpeg(&baseline);
    let mut sensor_bytes = vec![0u8; baseline.len() + 5000];
    sensor_bytes[..lossless.len()].copy_from_slice(&lossless);
    let sensor_pos = 600;
    let preview_pos = sensor_pos + sensor_bytes.len();
    let crop_entry = if crop_as_rational {
        at(0xC620, 5, 2, 400)
    } else {
        at(0xC620, 4, 2, 400)
    };
    let file = TiffBytes::new(true, preview_pos + baseline.len())
        .header(8)
        .ifd(8, 0, &[at(0x014A, 4, 2, 300)])
        .u32(300, 100)
        .u32(304, 200)
        .ifd(
            100,
            0,
            &[
                long(0x00FE, 0),
                long(0x0100, 4000),
                long(0x0101, 3000),
                short(0x0103, 7),
                long(0x0111, sensor_pos as u32),
                long(0x0117, sensor_bytes.len() as u32),
                crop_entry,
            ],
        )
        .ifd(
            200,
            0,
            &[
                long(0x00FE, 1),
                long(0x0100, 640),
                long(0x0101, 480),
                short(0x0103, 7),
                long(0x0111, preview_pos as u32),
                long(0x0117, baseline.len() as u32),
            ],
        )
        .put(sensor_pos, &sensor_bytes)
        .put(preview_pos, &baseline);
    let file = if crop_as_rational {
        file.u32(400, 3900).u32(404, 1).u32(408, 2900).u32(412, 1)
    } else {
        file.u32(400, 3900).u32(404, 2900)
    };
    (file.into_vec(), preview_pos)
}

// C#: TiffReaderRegressionTests.DngReader_LosslessJpegRawInSubIfd_IsNotListedAsPreview
#[test]
fn dng_reader_lossless_jpeg_raw_in_sub_ifd_is_not_listed_as_preview() {
    let (data, preview_offset) = dng_with_lossless_raw_and_preview(false);

    let info = read_ok(data);

    let preview = only_preview(&info);
    assert_eq!(preview.offset, preview_offset as i64);
    assert_eq!(preview.length, create_minimal_jpeg(640, 480).len() as i64);
}

// C#: TiffReaderRegressionTests.DngReader_LosslessJpegWithoutNewSubFileType_IsNotListedAsPreview
#[test]
fn dng_reader_lossless_jpeg_without_new_sub_file_type_is_not_listed_as_preview() {
    let lossless = as_lossless_jpeg(&create_minimal_jpeg(640, 480));
    let data = TiffBytes::new(true, 400 + lossless.len())
        .header(8)
        .ifd(
            8,
            0,
            &[
                long(0x0100, 800),
                long(0x0101, 600),
                short(0x0103, 7),
                long(0x0111, 400),
                long(0x0117, lossless.len() as u32),
            ],
        )
        .put(400, &lossless)
        .into_vec();

    let info = read_ok(data);

    assert!(info.previews.is_empty());
    assert_eq!(sensor(&info), (800, 600));
}

// C#: TiffReaderRegressionTests.DngReader_DefaultCropSize_UsesCropForSensorSizeWhetherLongOrRational (2 rows)
#[test]
fn dng_reader_default_crop_size_uses_crop_for_sensor_size_whether_long_or_rational() {
    for rational in [false, true] {
        let (data, _) = dng_with_lossless_raw_and_preview(rational);

        assert_eq!(sensor(&read_ok(data)), (3900, 2900), "rational={rational}");
    }
}

// C#: TiffReaderRegressionTests.DngReader_JpegSplitOverSeveralStrips_IsNotListedAsPreview
#[test]
fn dng_reader_jpeg_split_over_several_strips_is_not_listed_as_preview() {
    let jpeg = create_minimal_jpeg(640, 480);
    let half = jpeg.len() / 2;
    let data = TiffBytes::new(true, 700 + jpeg.len())
        .header(8)
        .ifd(
            8,
            0,
            &[
                long(0x00FE, 1),
                long(0x0100, 640),
                long(0x0101, 480),
                short(0x0103, 7),
                at(0x0111, 4, 2, 300),
                at(0x0117, 4, 2, 308),
            ],
        )
        .u32(300, 700)
        .u32(304, (700 + half) as u32)
        .u32(308, half as u32)
        .u32(312, (jpeg.len() - half) as u32)
        .put(700, &jpeg)
        .into_vec();

    assert!(read_ok(data).previews.is_empty());
}

// C#: TiffReaderRegressionTests.Readers_ArrayEntryCountAboveInt32Max_DoNotThrowUnexpectedExceptions (the `.dng` row)
#[test]
fn readers_array_entry_count_above_int32_max_do_not_throw_unexpected_exceptions_dng() {
    let data = TiffBytes::new(true, 512)
        .header(16)
        .put(8, b"CR\x02\0\0\0\0\0")
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

    match read_bytes(data) {
        Ok(_) | Err(RawHeaderError::InvalidData { .. }) => {}
        other => panic!("only Ok or InvalidData allowed, got {other:?}"),
    }
}

// ---------------------------------------------------------------- TiffSubIfdTypeTests (DNG rows)

/// C# `TiffSubIfdTypeTests.Build` for DNG: IFD0 -> SubIFD raw IFD 6048x4024, the pointer of type `entry_type`, inline or
/// as an array of two pointers.
fn sub_ifd_build(entry_type: u16, array: bool) -> Vec<u8> {
    let pointer = if array {
        at(0x014A, entry_type, 2, 300)
    } else {
        at(0x014A, entry_type, 1, 100)
    };
    let mut file = TiffBytes::new(true, 500).header(8).ifd(8, 0, &[pointer]).ifd(
        100,
        0,
        &[
            long(0x00FE, 0),
            short(0x0100, 6048),
            short(0x0101, 4024),
            short(0x0103, 32767),
        ],
    );
    if array {
        file = file.u32(300, 100).u32(304, 400);
    }
    file.ifd(
        400,
        0,
        &[
            long(0x00FE, 1),
            short(0x0100, 160),
            short(0x0101, 120),
            short(0x0103, 1),
        ],
    )
    .into_vec()
}

// C#: TiffSubIfdTypeTests.SubIfdPointers_OfLongOrIfdType_ReachTheRawIfd (the 4 `dng` rows: types 4 and 13, inline/array)
#[test]
fn sub_ifd_pointers_of_long_or_ifd_type_reach_the_raw_ifd() {
    for entry_type in [4u16, 13] {
        for array in [false, true] {
            let info = read_ok(sub_ifd_build(entry_type, array));

            assert_eq!(sensor(&info), (6048, 4024), "type {entry_type} array={array}");
        }
    }
}

// C#: TiffSubIfdTypeTests.Dng_NegativeImageSize_NeverYieldsNegativeSizes (2 rows: SSHORT 8 and SLONG 9)
#[test]
fn dng_negative_image_size_never_yields_negative_sizes() {
    for entry_type in [8u16, 9] {
        let jpeg = create_minimal_jpeg(64, 48);
        let minus_five = (-5i32) as u32;
        let data = TiffBytes::new(true, 1000 + jpeg.len())
            .header(8)
            .ifd(
                8,
                0,
                &[
                    at(0x0100, entry_type, 1, minus_five),
                    at(0x0101, entry_type, 1, minus_five),
                    long(0x0201, 1000),
                    long(0x0202, jpeg.len() as u32),
                ],
            )
            .put(1000, &jpeg)
            .into_vec();

        let info = read_ok(data);

        assert!(
            info.sensor_width >= 0 && info.sensor_height >= 0,
            "type {entry_type}: sensor {}x{}",
            info.sensor_width,
            info.sensor_height
        );
        for p in &info.previews {
            assert!(
                p.width >= 0 && p.height >= 0,
                "type {entry_type}: preview {}x{}",
                p.width,
                p.height
            );
        }
    }
}

// C#: TiffSubIfdTypeTests.Dng_CompressionAboveUShort_IsNotTreatedAsJpegCompression
#[test]
fn dng_compression_above_ushort_is_not_treated_as_jpeg_compression() {
    // Compression LONG 65542 = 0x10006: truncated to (ushort) it became 6 and the lossy-JPEG strip was taken as a preview.
    let jpeg = create_minimal_jpeg(64, 48);
    let data = TiffBytes::new(true, 1000 + jpeg.len())
        .header(8)
        .ifd(
            8,
            0,
            &[long(0x0103, 65542), long(0x0111, 1000), long(0x0117, jpeg.len() as u32)],
        )
        .put(1000, &jpeg)
        .into_vec();

    assert!(read_ok(data).previews.is_empty());
}

// C#: TiffSubIfdTypeTests.Dng_CompressionSix_StillYieldsTheStripPreview
#[test]
fn dng_compression_six_still_yields_the_strip_preview() {
    let jpeg = create_minimal_jpeg(64, 48);
    let data = TiffBytes::new(true, 1000 + jpeg.len())
        .header(8)
        .ifd(
            8,
            0,
            &[short(0x0103, 6), long(0x0111, 1000), long(0x0117, jpeg.len() as u32)],
        )
        .put(1000, &jpeg)
        .into_vec();

    let info = read_ok(data);

    let preview = only_preview(&info);
    assert_eq!((preview.width, preview.height), (64, 48));
}

// ---------------------------------------------------------------- TiffFamilyReaderTests / TiffStripPreviewTests

// C#: TiffFamilyReaderTests.DngReader_Synthetic_CanReadPreview
#[test]
fn dng_reader_synthetic_can_read_preview() {
    let jpeg = create_minimal_jpeg(640, 480);
    let data = build_tiff(true, Some(&jpeg), 1, 42);
    let reader = DngContainerReader::new();
    assert!(reader.can_read(&data[..data.len().min(64)], ".dng"));

    let info = read_ok(data);

    assert_eq!(info.format, RawFormat::Dng);
    assert_eq!(info.previews.len(), 1);
}

/// C# `TiffStripPreviewTests.Build`: IFD0 declares 9000x6000 and one compression-7 strip at 1000 with `declared` bytes.
fn strip_declared_size(jpeg: &[u8], declared: u32) -> Vec<u8> {
    TiffBytes::new(true, 3000)
        .header(8)
        .ifd(
            8,
            0,
            &[
                short(0x0100, 9000),
                short(0x0101, 6000),
                short(0x0103, 7),
                long(0x0111, 1000),
                long(0x0117, declared),
            ],
        )
        .put(1000, jpeg)
        .into_vec()
}

// C#: TiffStripPreviewTests.Read_StripPreviewWhoseDeclaredSizeContradictsTheJpegFrame_UsesTheFrameSize (the `dng` row)
#[test]
fn read_strip_preview_whose_declared_size_contradicts_the_jpeg_frame_uses_the_frame_size() {
    let jpeg = create_minimal_jpeg(320, 240);

    let info = read_ok(strip_declared_size(&jpeg, jpeg.len() as u32));

    let preview = info
        .previews
        .iter()
        .find(|p| p.offset == 1000)
        .unwrap_or_else(|| panic!("no preview at 1000: {:?}", info.previews));
    assert_eq!((preview.width, preview.height), (320, 240));
}

// ---------------------------------------------------------------- TiffPreviewHardeningTests (DNG rows)

/// C# `TiffPreviewHardeningTests.Build` / `BuildWithJpeg` for DNG: IFD0 orientation, IFD at 100 declares 9000x6000 with a
/// JPEG-interchange pointer at 1000 (length `declared_len`); the JPEG is written at 1000 when given.
fn interchange_dng(jpeg: Option<&[u8]>, declared_len: u32, size: usize) -> Vec<u8> {
    let mut file = TiffBytes::new(true, size)
        .header(8)
        .ifd(8, 100, &[short(0x0112, 1)])
        .ifd(
            100,
            0,
            &[
                short(0x0100, 9000),
                short(0x0101, 6000),
                long(0x0201, 1000),
                long(0x0202, declared_len),
            ],
        );
    if let Some(jpeg) = jpeg {
        file = file.put(1000, jpeg);
    }
    file.into_vec()
}

/// C# `TiffPreviewHardeningTests.BudgetExhaustingJpeg`: 140 maximal APP0 segments one 64 KiB block apart, so walking to
/// the SOF charges more than the 8 MiB header budget.
fn budget_exhausting_jpeg() -> Vec<u8> {
    let tail = create_minimal_jpeg(320, 240);
    let mut out = vec![0xFF, 0xD8];
    let mut app = vec![0u8; 2 + 65535];
    app[0] = 0xFF;
    app[1] = 0xE0;
    app[2] = 0xFF;
    app[3] = 0xFF;
    for _ in 0..140 {
        out.extend_from_slice(&app);
    }
    out.extend_from_slice(&tail[2..]);
    out
}

// C#: TiffPreviewHardeningTests.Read_JpegInterchangePointerToZeroPadding_IsNotAPreview (the `dng` row)
#[test]
fn read_jpeg_interchange_pointer_to_zero_padding_is_not_a_preview() {
    let info = read_ok(interchange_dng(None, 200, 3000));

    assert!(info.previews.iter().all(|p| p.offset != 1000), "{:?}", info.previews);
}

// C#: TiffPreviewHardeningTests.Read_DeclaredSizeContradictingTheJpegFrame_UsesTheFrameSize (the `dng` row)
#[test]
fn read_declared_size_contradicting_the_jpeg_frame_uses_the_frame_size() {
    let jpeg = create_minimal_jpeg(320, 240);

    let info = read_ok(interchange_dng(Some(&jpeg), jpeg.len() as u32, 3000));

    let preview = only_preview(&info);
    assert_eq!(preview.offset, 1000);
    assert_eq!((preview.width, preview.height), (320, 240));
}

// C#: TiffPreviewHardeningTests.Read_PreviewJpegExhaustingHeaderBudget_ContainerStillParsesAndKeepsDeclaredSize (`dng`)
#[test]
fn read_preview_jpeg_exhausting_header_budget_container_still_parses_and_keeps_declared_size() {
    let jpeg = budget_exhausting_jpeg();

    let info = read_ok(interchange_dng(Some(&jpeg), jpeg.len() as u32, 1000 + jpeg.len()));

    let preview = only_preview(&info);
    assert_eq!(preview.offset, 1000);
    assert!(preview.width == 0 || preview.width == 9000, "width {}", preview.width);
    assert!(
        preview.height == 0 || preview.height == 6000,
        "height {}",
        preview.height
    );
}

// ---------------------------------------------------------------- TiffExifBlockPlacementTests (DNG rows, block length)

/// C# `TiffExifBlockPlacementTests.Build`: IFD0 with dimensions, Make at `make_offset`, Exif pointer; Exif IFD at `exif`.
fn exif_placement_dng(size: usize, make_offset: usize, exif: usize) -> Vec<u8> {
    TiffBytes::new(true, size)
        .header(8)
        .ifd(
            8,
            0,
            &[
                short(0x0100, 4000),
                short(0x0101, 3000),
                at(0x010F, 2, 6, make_offset as u32),
                long(0x8769, exif as u32),
            ],
        )
        .put(make_offset, b"Acme\0\0")
        .ifd(exif, 0, &[short(0x8827, 400)])
        .into_vec()
}

// C#: TiffExifBlockPlacementTests.Read_IfdsInsideDefaultWindow_KeepsDefaultBlockLength (the `dng` row)
#[test]
fn read_ifds_inside_default_window_keeps_default_block_length() {
    let info = read_ok(exif_placement_dng(310_000, 2000, 3000));

    assert_eq!(info.exif_blocks.len(), 1);
    for block in &info.exif_blocks {
        assert_eq!(block.length, i64::from(DEFAULT_EXIF_BLOCK_BYTES));
        assert_eq!(block.offset, 0);
        assert!(block.is_tiff_header);
    }
}

// C#: TiffExifBlockPlacementTests.Read_IfdsBeyondMaxBlock_CapsBlockAndDoesNotThrow (the `dng` row, block length only)
#[test]
fn read_ifds_beyond_max_block_caps_block_and_does_not_throw() {
    let far = MAX_EXIF_BLOCK_BYTES as usize + 50_000;

    let info = read_ok(exif_placement_dng(far + 10_000, far, far + 100));

    assert_eq!(info.exif_blocks.len(), 1);
    for block in &info.exif_blocks {
        assert_eq!(block.length, i64::from(DEFAULT_EXIF_BLOCK_BYTES));
    }
}

// ---------------------------------------------------------------- hostile input and fuzz (DNG seed)

/// "Rich" DNG seed: IFD0 with a SubIFDs array, a chain IFD1, a compression-7 strip, DefaultCropSize and an Exif pointer.
fn rich_dng() -> Vec<u8> {
    let jpeg = create_minimal_jpeg(320, 240);
    TiffBytes::new(true, 2048)
        .header(8)
        .ifd(
            8,
            200,
            &[
                long(0x00FE, 0),
                short(0x0100, 4000),
                short(0x0101, 3000),
                short(0x0103, 7),
                short(0x0106, CFA),
                short(0x0112, 6),
                long(0x0111, 1000),
                long(0x0117, jpeg.len() as u32),
                at(0x014A, 4, 2, 300),
                at(0xC620, 4, 2, 320),
                long(0x8769, 600),
            ],
        )
        .ifd(
            200,
            0,
            &[
                long(0x00FE, 1),
                short(0x0100, 640),
                short(0x0101, 480),
                short(0x0103, 6),
                long(0x0201, 1300),
                long(0x0202, jpeg.len() as u32),
            ],
        )
        .u32(300, 400)
        .u32(304, 500)
        .u32(320, 3900)
        .u32(324, 2900)
        .ifd(
            400,
            0,
            &[
                long(0x00FE, 0),
                short(0x0100, 4000),
                short(0x0101, 3000),
                short(0x0106, CFA),
            ],
        )
        .ifd(
            500,
            0,
            &[
                long(0x00FE, 1),
                short(0x0100, 640),
                short(0x0101, 480),
                short(0x0103, 6),
                long(0x0201, 1300),
                long(0x0202, jpeg.len() as u32),
            ],
        )
        .ifd(600, 0, &[short(0x8827, 400)])
        .put(1000, &jpeg)
        .put(1300, &jpeg)
        .into_vec()
}

/// Checks the accepted-input invariants of the C# fuzz test (orientation, sizes, one EXIF block, preview ranges).
fn check_dng_invariants(label: &str, bytes: &[u8], info: &RawContainerInfo) {
    assert!(
        (1..=8).contains(&info.orientation),
        "{label}: orientation {}",
        info.orientation
    );
    assert!(
        info.sensor_width >= 0 && info.sensor_height >= 0,
        "{label}: negative sensor size"
    );
    assert_eq!(info.exif_blocks.len(), 1, "{label}: exactly one EXIF block");
    let file_len = bytes.len() as i64;
    for p in &info.previews {
        assert!(
            p.offset >= 0 && p.length > 0 && p.length <= file_len - p.offset && p.width >= 0 && p.height >= 0,
            "{label}: preview {}+{} ({}x{}) escapes the {file_len}-byte file",
            p.offset,
            p.length,
            p.width,
            p.height
        );
    }
}

/// Deterministic mutants (C# `RawContainerFuzzTests` / `BinaryFuzz.Mutants`): `can_read` never panics; an accepted file
/// reads as `Ok` or `InvalidData`, and an `Ok` result satisfies the invariants. The C# p95 timing check is replaced by
/// "the loop terminates" (a hang fails by the cargo test timeout).
fn run_dng_fuzz(seed: &[u8], rng_seed: u64) {
    let reader = DngContainerReader::new();
    for case in mutants(seed, rng_seed, 2000) {
        let head = &case.bytes[..case.bytes.len().min(64)];
        if !reader.can_read(head, ".dng") {
            continue;
        }
        match read_bytes(case.bytes.clone()) {
            Ok(info) => check_dng_invariants(&case.label, &case.bytes, &info),
            Err(RawHeaderError::InvalidData { .. }) => {}
            Err(other) => panic!("{}: unexpected {other:?}", case.label),
        }
    }
}

// C#: RawContainerFuzzTests.Fuzz_DngContainerReader_Robust (LE orientation 1, seed 104, plus the BE and rich seeds)
#[test]
fn fuzz_dng_container_reader_robust() {
    let jpeg = create_minimal_jpeg(64, 48);
    run_dng_fuzz(&build_tiff(true, None, 1, 42), 104);
    run_dng_fuzz(&build_tiff(false, Some(&jpeg), 8, 42), 1040);
    run_dng_fuzz(&rich_dng(), 1041);
}

// C#: BinaryReaderFuzzTests.Fuzz_RawContainerReaders_FailOnlyCleanly (the DNG seed row; rng seed 4400 + 5)
#[test]
fn fuzz_raw_container_readers_fail_only_cleanly_dng_seed() {
    let seed = build_tiff(true, Some(&create_minimal_jpeg(64, 48)), 1, 42);
    run_dng_fuzz(&seed, 4405);
}

// C#: RawContainerFuzzTests.Fuzz_DngContainerReader_Robust (hostile array counts): `Readers_ArrayEntryCountAboveInt32Max`
// and golden `hostile-array-counts-above-int32-max`: `Ok` or `InvalidData`, no panic, no allocation proportional to the count.
#[test]
fn hostile_array_counts_above_int32_max_never_panic() {
    let mut hostile = rich_dng();
    // IFD0 entry 8 is the SubIFDs pointer (tag 0x014A); its count field sits 4 bytes into the 12-byte entry.
    let count_pos = 8 + 2 + 12 * 8 + 4;
    hostile[count_pos..count_pos + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    match read_bytes(hostile) {
        Ok(_) | Err(RawHeaderError::InvalidData { .. }) => {}
        other => panic!("only Ok or InvalidData allowed, got {other:?}"),
    }
}

// ---------------------------------------------------------------- Rust-only: cancellation and I/O failures

/// Forwards to an in-memory source and cancels `token` at the `cancel_at`-th read call.
struct CancelAtRead {
    inner: InMemoryRawHeaderSource,
    token: CancelToken,
    cancel_at: usize,
    calls: usize,
}

impl RawHeaderSource for CancelAtRead {
    fn length(&self) -> i64 {
        self.inner.length()
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.calls += 1;
        if self.calls == self.cancel_at {
            self.token.cancel();
        }
        self.inner.read(offset, count)
    }
}

/// Forwards to an in-memory source and fails the `fail_at`-th read call with an I/O error (`io: Some`).
struct FailAtRead {
    inner: InMemoryRawHeaderSource,
    fail_at: usize,
    calls: usize,
}

impl RawHeaderSource for FailAtRead {
    fn length(&self) -> i64 {
        self.inner.length()
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.calls += 1;
        if self.calls == self.fail_at {
            return Err(RawHeaderError::invalid_data_io(
                "source read failed",
                io::Error::other("share lost"),
            ));
        }
        self.inner.read(offset, count)
    }
}

// Rust-only: a token cancelled before the call gives `Canceled`, never `InvalidData`.
#[test]
fn read_with_token_cancelled_before_the_call_returns_canceled() {
    let token = CancelToken::new();
    token.cancel();
    let mut source = InMemoryRawHeaderSource::new(rich_dng());

    let result = DngContainerReader::new().read(&mut source, &token);

    assert!(matches!(result, Err(RawHeaderError::Canceled)), "{result:?}");
}

// Rust-only: a token cancelled by the source during the first IFD read gives `Canceled` at the next step.
#[test]
fn read_with_token_cancelled_by_source_after_first_ifd_read_returns_canceled() {
    let token = CancelToken::new();
    let mut source = CancelAtRead {
        inner: InMemoryRawHeaderSource::new(rich_dng()),
        token: token.clone(),
        cancel_at: 2,
        calls: 0,
    };

    let result = DngContainerReader::new().read(&mut source, &token);

    assert!(matches!(result, Err(RawHeaderError::Canceled)), "{result:?}");
}

// Rust-only: an I/O failure at every read call of the walk (header, IFD0, the chain, a SubIFD, the strip probe, the EXIF
// block) comes back as an I/O failure, never as `InvalidData` or `Ok`.
#[test]
fn read_io_failure_at_each_read_call_is_returned_as_io_failure() {
    let data = rich_dng();
    let mut counting = FailAtRead {
        inner: InMemoryRawHeaderSource::new(data.clone()),
        fail_at: usize::MAX,
        calls: 0,
    };
    let baseline = DngContainerReader::new().read(&mut counting, &CancelToken::new());
    assert!(baseline.is_ok(), "rich layout must read without failures: {baseline:?}");
    let total = counting.calls;
    assert!(total >= 6, "the walk makes at least 6 reads, got {total}");

    for n in 1..=total {
        let mut source = FailAtRead {
            inner: InMemoryRawHeaderSource::new(data.clone()),
            fail_at: n,
            calls: 0,
        };
        match DngContainerReader::new().read(&mut source, &CancelToken::new()) {
            Err(error) => assert!(error.is_io_failure(), "read #{n}: {error:?} is not an I/O failure"),
            Ok(_) => panic!("read #{n}: the I/O failure was swallowed"),
        }
    }
}
