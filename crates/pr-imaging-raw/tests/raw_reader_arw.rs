//! Red tests for W5-07 (`readers/arw.rs`): `ArwContainerReader` (Sony ARW, C# baseline d92d6aae).
//!
//! Ported from `tests/PhotoReview.Imaging.Tests/Raw/Tiff/Cr2ArwRafMutationGapTests.cs` (ARW rows), `Raw/Tiff/TiffFamilyReaderTests.cs`
//! (ARW row), `Raw/Tiff/TiffReaderRegressionTests.cs` (ARW rows), `Raw/Tiff/TiffSensorSizeTests.cs` (ARW rows),
//! `Raw/Tiff/TiffSubIfdTypeTests.cs` (ARW rows), `Raw/Tiff/TiffPreviewHardeningTests.cs` (ARW rows),
//! `Raw/Tiff/TiffExifBlockPlacementTests.cs` (ARW row, block lengths only), `Raw/Tiff/MakerNoteGapTests.cs` (Sony half),
//! `Raw/Tiff/TiffCorpusTests.cs` (ARW subset), `Raw/RawSizeConsistencyAuditTests.cs` (reader half of the 3 ARW rows),
//! `Robustness/RawContainerFuzzTests.cs` (`Fuzz_ArwContainerReader_Robust`), and the golden `tests/golden/raw-arw-reader.json`
//! (inputs in `tests/golden-inputs/raw-arw-reader.json`, oracle `tools/oracle-cs/Commands/W5-07.cs`).
//!
//! Shared byte builders: `pr_test_support::raw` (D48). The spec builder that turns the golden `tiff` sources into bytes is a
//! line port of the oracle's `W507Tiff` and lives here; it is a candidate for hoisting into `pr_test_support::raw` once a second
//! RAW unit needs it (contract W5-07, "Shared test helpers").
//!
//! Corpus tests read `PHOTOREVIEW_RAW_CORPUS_DIR` (default `tests/fixtures/raw-corpus/`) and return early when the sample is
//! absent, unless `PHOTOREVIEW_RAW_CORPUS_STRICT` is `1`/`true` (then they panic naming the variable), D49 of
//! `contracts/README.md`. They are NOT `#[ignore]`d: they are data-gated, like C#.
//!
//! Stub bodies are `todo!()`: every test that reaches the reader fails until the porter fills it in.
//! No timing asserts (AGENTS.md): the C# p95 budget of `Fuzz_ArwContainerReader_Robust` is dropped (D54), the cargo job timeout
//! is the hang guard.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use pr_core::session::store::sha256;
use pr_core::sync::CancelToken;
use pr_imaging_raw::formats::limits;
use pr_imaging_raw::header_source::SourceRawHeaderSource;
use pr_imaging_raw::readers::arw::ArwContainerReader;
use pr_imaging_raw::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, InMemoryRawHeaderSource, PreviewColorSpace, RawContainerInfo,
    RawContainerReader, RawHeaderError, RawHeaderSource,
};
use pr_test_support::golden;
use pr_test_support::raw::{Entry, TiffBytes, XorShift64, at, build_tiff, create_minimal_jpeg, from_hex, long, short};
use serde_json::{Map, Value, json};

// ---------------------------------------------------------------- helpers

/// Reads `bytes` as an ARW file with a fresh header budget and a token nobody cancels (C# `ReadArw`).
fn read_arw(bytes: Vec<u8>) -> Result<RawContainerInfo, RawHeaderError> {
    let mut source = InMemoryRawHeaderSource::new(bytes);
    ArwContainerReader.read(&mut source, &CancelToken::none())
}

/// `read_arw` for inputs the reader must accept (C# `Read(new ArwContainerReader(), data)`).
fn read_ok(bytes: Vec<u8>) -> RawContainerInfo {
    read_arw(bytes).expect("ARW container must be read")
}

/// The reader refuses the file with `InvalidData` (C# `Assert.Throws<InvalidDataException>`).
fn assert_invalid_data(result: Result<RawContainerInfo, RawHeaderError>) {
    match result {
        Err(RawHeaderError::InvalidData { .. }) => {}
        other => panic!("expected InvalidData, got {other:?}"),
    }
}

/// The single preview listed at `offset` (C# `Assert.Single(info.Previews, p => p.Offset == offset)`).
fn single_preview_at(info: &RawContainerInfo, offset: i64) -> &EmbeddedPreview {
    let found: Vec<&EmbeddedPreview> = info.previews.iter().filter(|p| p.offset == offset).collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one preview at {offset}, got {:?}",
        info.previews
    );
    found[0]
}

fn len_u32(bytes: &[u8]) -> u32 {
    u32::try_from(bytes.len()).expect("length fits u32")
}

/// The baseline 640x480 JPEG used throughout the synthetic files (C# `BaselineJpeg`).
fn baseline_jpeg() -> Vec<u8> {
    create_minimal_jpeg(640, 480)
}

/// C# `SyntheticRawBuilder.CreateMinimalJpeg`-based budget exhauster: 140 APP0 segments of 65 537 bytes before the SOF, so the
/// frame walk charges more distinct 64 KiB blocks than the 8 MiB header budget (C# `BudgetExhaustingJpeg`, oracle `W507BudgetJpeg`).
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

// ---------------------------------------------------------------- signature (Cr2ArwRafMutationGapTests)

// C#: Cr2ArwRafMutationGapTests.ArwCanRead_ExactlyFourBytesWithEachSignatureByteChecked
#[test]
fn arw_can_read_exactly_four_bytes_with_each_signature_byte_checked() {
    let head = [0x49u8, 0x49, 0x2A, 0x00];
    let reader = ArwContainerReader;

    assert!(reader.can_read(&head, ".arw"));
    assert!(!reader.can_read(&head[..3], ".arw"));
    for position in [0usize, 1, 2, 3] {
        let mut broken = head;
        broken[position] ^= 0x40;
        assert!(!reader.can_read(&broken, ".arw"), "byte {position} must be checked");
    }
}

// C#: Cr2ArwRafMutationGapTests.ArwReader_Synthetic_CanReadPreview (TiffFamilyReaderTests.ArwReader_Synthetic_CanReadPreview)
#[test]
fn arw_reader_synthetic_can_read_preview() {
    let jpeg = baseline_jpeg();
    let data = build_tiff(true, Some(&jpeg), 3, 42);

    let reader = ArwContainerReader;
    assert!(reader.can_read(&data[..data.len().min(64)], ".arw"));

    let info = read_ok(data);

    assert_eq!(info.format, pr_imaging_raw::RawFormat::Arw);
    assert_eq!(info.orientation, 3);
    let preview = info.previews.first().expect("one preview");
    assert_eq!(info.previews.len(), 1);
    assert_eq!(preview.length, i64::try_from(jpeg.len()).expect("fits"));
}

// C#: Cr2ArwRafMutationGapTests.Read_SixteenByteFileWithEmptyIfd_ReturnsEmptyInfoForCr2AndArw (ARW half)
#[test]
fn read_sixteen_byte_file_with_empty_ifd_returns_empty_info_for_arw() {
    let data = TiffBytes::new(true, 16).header(8).ifd(8, 0, &[]).into_vec();

    let info = read_ok(data);

    assert_eq!((info.sensor_width, info.sensor_height), (0, 0));
    assert!(info.previews.is_empty());
    assert_eq!(info.exif_blocks.len(), 1);
}

// C#: Cr2ArwRafMutationGapTests.ReadArw_ValidByteOrderButMagicFortyThree_Throws
#[test]
fn read_arw_valid_byte_order_but_magic_forty_three_throws() {
    let data = TiffBytes::new(true, 64)
        .header(8)
        .put(0, b"II+\0")
        .ifd(8, 0, &[short(0x0100, 100)])
        .into_vec();

    assert_invalid_data(read_arw(data));
}

// ---------------------------------------------------------------- IFD chain and size ties (Cr2ArwRafMutationGapTests)

// C#: Cr2ArwRafMutationGapTests.ReadArw_ChainOfSixtySixIfds_ReadsOnlyTheFirstSixtyFour
#[test]
fn read_arw_chain_of_sixty_six_ifds_reads_only_the_first_sixty_four() {
    const IFD_SIZE: usize = 30;
    let mut file = TiffBytes::new(true, 8 + 66 * IFD_SIZE).header(8);
    for i in 0..66usize {
        let at_offset = 8 + i * IFD_SIZE;
        let next = if i < 65 {
            u32::try_from(at_offset + IFD_SIZE).expect("fits")
        } else {
            0
        };
        let width = u32::try_from(100 + i).expect("fits");
        file = file.ifd(at_offset, next, &[short(0x0100, width), short(0x0101, 100)]);
    }

    let info = read_ok(file.into_vec());

    assert_eq!((info.sensor_width, info.sensor_height), (163, 100));
}

// C#: Cr2ArwRafMutationGapTests.ReadArw_TwoRawIfdsOfEqualArea_KeepsTheFirst
#[test]
fn read_arw_two_raw_ifds_of_equal_area_keeps_the_first() {
    let data = TiffBytes::new(true, 128)
        .header(8)
        .ifd(8, 60, &[short(0x0100, 4000), short(0x0101, 3000)])
        .ifd(60, 0, &[short(0x0100, 3000), short(0x0101, 4000)])
        .into_vec();

    let info = read_ok(data);

    assert_eq!((info.sensor_width, info.sensor_height), (4000, 3000));
}

// C#: Cr2ArwRafMutationGapTests.ReadArw_SameJpegReferencedByTwoIfds_IsListedOnce
#[test]
fn read_arw_same_jpeg_referenced_by_two_ifds_is_listed_once() {
    let jpeg = baseline_jpeg();
    let jl = len_u32(&jpeg);
    let data = TiffBytes::new(true, 400 + jpeg.len())
        .header(8)
        .ifd(8, 60, &[long(0x0201, 400), long(0x0202, jl)])
        .ifd(60, 0, &[long(0x0201, 400), long(0x0202, jl)])
        .put(400, &jpeg)
        .into_vec();

    let info = read_ok(data);

    assert_eq!(info.previews.len(), 1);
}

// ---------------------------------------------------------------- MakerNote (Cr2ArwRafMutationGapTests)

/// IFD0 at 8 -> Exif IFD at 60 with one MakerNote-or-other entry (`exif_tag`, offset `note_offset`) -> note IFD at
/// `note_offset` with one `0x2001` PreviewImage entry at 700 (C# `ArwWithNote`, line port).
fn arw_with_note(
    exif_tag: u16,
    exif_count: u32,
    preview_tag: u16,
    preview_count: u32,
    note_offset: u32,
    size: usize,
    jpeg: &[u8],
) -> Vec<u8> {
    TiffBytes::new(true, size)
        .header(8)
        .ifd(8, 0, &[long(0x8769, 60)])
        .ifd(60, 0, &[at(exif_tag, 7, exif_count, note_offset)])
        .ifd(note_offset as usize, 0, &[at(preview_tag, 7, preview_count, 700)])
        .put(700, jpeg)
        .into_vec()
}

// C#: Cr2ArwRafMutationGapTests.ReadArw_MakerNoteEntry_IsAnOffsetOnlyForTagMakerNoteWithMoreThanFourBytes [InlineData(0x927C, 100, 1)]
#[test]
fn read_arw_maker_note_entry_with_count_above_four_is_an_offset_tag_927c_count_100() {
    let jpeg = baseline_jpeg();
    let data = arw_with_note(0x927C, 100, 0x2001, len_u32(&jpeg), 200, 700 + jpeg.len(), &jpeg);

    assert_eq!(read_ok(data).previews.len(), 1);
}

// C#: Cr2ArwRafMutationGapTests.ReadArw_MakerNoteEntry_IsAnOffsetOnlyForTagMakerNoteWithMoreThanFourBytes [InlineData(0x927C, 5, 1)]
#[test]
fn read_arw_maker_note_entry_with_count_five_is_an_offset_tag_927c_count_5() {
    let jpeg = baseline_jpeg();
    let data = arw_with_note(0x927C, 5, 0x2001, len_u32(&jpeg), 200, 700 + jpeg.len(), &jpeg);

    assert_eq!(read_ok(data).previews.len(), 1);
}

// C#: Cr2ArwRafMutationGapTests.ReadArw_MakerNoteEntry_IsAnOffsetOnlyForTagMakerNoteWithMoreThanFourBytes [InlineData(0x927C, 4, 0)]
/// 4 inline bytes are data, not an offset.
#[test]
fn read_arw_maker_note_entry_with_four_inline_bytes_is_not_an_offset_tag_927c_count_4() {
    let jpeg = baseline_jpeg();
    let data = arw_with_note(0x927C, 4, 0x2001, len_u32(&jpeg), 200, 700 + jpeg.len(), &jpeg);

    assert!(read_ok(data).previews.is_empty());
}

// C#: Cr2ArwRafMutationGapTests.ReadArw_MakerNoteEntry_IsAnOffsetOnlyForTagMakerNoteWithMoreThanFourBytes [InlineData(0x9286, 100, 0)]
/// Another tag is not the MakerNote.
#[test]
fn read_arw_maker_note_entry_of_another_tag_is_not_the_maker_note_tag_9286_count_100() {
    let jpeg = baseline_jpeg();
    let data = arw_with_note(0x9286, 100, 0x2001, len_u32(&jpeg), 200, 700 + jpeg.len(), &jpeg);

    assert!(read_ok(data).previews.is_empty());
}

// C#: Cr2ArwRafMutationGapTests.ReadArw_NotePreviewEntryOfExactlyFourBytes_IsNotAPreview
#[test]
fn read_arw_note_preview_entry_of_exactly_four_bytes_is_not_a_preview() {
    let data = arw_with_note(0x927C, 100, 0x2001, 4, 200, 710, &[0xFF, 0xD8, 0xFF, 0xD9]);

    assert!(read_ok(data).previews.is_empty());
}

// C#: Cr2ArwRafMutationGapTests.ReadArw_NoteStartingLessThanFourteenBytesBeforeTheEnd_IsIgnoredWithoutThrowing
#[test]
fn read_arw_note_starting_less_than_fourteen_bytes_before_the_end_is_ignored_without_throwing() {
    let data = TiffBytes::new(true, 200)
        .header(8)
        .ifd(8, 0, &[long(0x8769, 60)])
        .ifd(60, 0, &[at(0x927C, 7, 100, 192)])
        .into_vec();

    assert!(read_ok(data).previews.is_empty());
}

// C#: Cr2ArwRafMutationGapTests.ReadArw_NoteStartingExactlyFourteenBytesBeforeTheEnd_IsParsed
#[test]
fn read_arw_note_starting_exactly_fourteen_bytes_before_the_end_is_parsed() {
    let jpeg = baseline_jpeg();
    const SIZE: usize = 1000;
    const NOTE: u32 = 986; // SIZE - 14
    let mut entry = Vec::with_capacity(12);
    entry.extend_from_slice(&0x2001u16.to_le_bytes());
    entry.extend_from_slice(&7u16.to_le_bytes());
    entry.extend_from_slice(&len_u32(&jpeg).to_le_bytes());
    entry.extend_from_slice(&300u32.to_le_bytes());
    let data = TiffBytes::new(true, SIZE)
        .header(8)
        .ifd(8, 0, &[long(0x8769, 60)])
        .ifd(60, 0, &[at(0x927C, 7, 100, NOTE)])
        .put(300, &jpeg)
        .u16(NOTE as usize, 1)
        .put(NOTE as usize + 2, &entry)
        .into_vec();

    let info = read_ok(data);

    let preview = info.previews.first().expect("one preview");
    assert_eq!(info.previews.len(), 1);
    assert_eq!(
        (preview.offset, preview.length),
        (300, i64::try_from(jpeg.len()).expect("fits"))
    );
}

// C#: Cr2ArwRafMutationGapTests.ReadArw_NoteStartingAtOffsetEight_IsParsed
#[test]
fn read_arw_note_starting_at_offset_eight_is_parsed() {
    let jpeg = baseline_jpeg();
    let data = TiffBytes::new(true, 700 + jpeg.len())
        .header(400)
        .ifd(8, 0, &[at(0x2001, 7, len_u32(&jpeg), 700)])
        .ifd(400, 0, &[long(0x8769, 450)])
        .ifd(450, 0, &[at(0x927C, 7, 100, 8)])
        .put(700, &jpeg)
        .into_vec();

    let info = read_ok(data);

    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].offset, 700);
}

// ---------------------------------------------------------------- TiffReaderRegressionTests (ARW rows)

// C#: TiffReaderRegressionTests.ArwReader_JpegInSubIfd_IsListedAndRawSubIfdSetsSensorSize
#[test]
fn arw_reader_jpeg_in_sub_ifd_is_listed_and_raw_sub_ifd_sets_sensor_size() {
    let jpeg = baseline_jpeg();
    let data = TiffBytes::new(true, 700 + jpeg.len())
        .header(8)
        .ifd(8, 0, &[at(0x014A, 4, 2, 300)])
        .u32(300, 100)
        .u32(304, 200)
        .ifd(
            100,
            0,
            &[
                long(0x00FE, 0),
                short(0x0100, 6048),
                short(0x0101, 4024),
                short(0x0103, 32767),
            ],
        )
        .ifd(
            200,
            0,
            &[
                long(0x00FE, 1),
                short(0x0100, 640),
                short(0x0101, 480),
                long(0x0201, 700),
                long(0x0202, len_u32(&jpeg)),
            ],
        )
        .put(700, &jpeg)
        .into_vec();

    let info = read_ok(data);

    assert_eq!((info.sensor_width, info.sensor_height), (6048, 4024));
    assert_eq!(info.previews.len(), 1);
    let preview = &info.previews[0];
    assert_eq!((preview.offset, preview.width, preview.height), (700, 640, 480));
}

// C#: TiffReaderRegressionTests.ArwReader_MakerNotePreviewImage_IsListed [InlineData(false)]
// SYNTHETIC ONLY: no file in the real corpus carries the Sony MakerNote 0x2001 PreviewImage tag (C# comment).
#[test]
fn arw_reader_maker_note_preview_image_is_listed_without_sony_dsc_header() {
    assert_maker_note_preview_listed(false);
}

// C#: TiffReaderRegressionTests.ArwReader_MakerNotePreviewImage_IsListed [InlineData(true)]
#[test]
fn arw_reader_maker_note_preview_image_is_listed_with_sony_dsc_header() {
    assert_maker_note_preview_listed(true);
}

fn assert_maker_note_preview_listed(sony_dsc_header: bool) {
    let jpeg = baseline_jpeg();
    let note_ifd: usize = if sony_dsc_header { 200 + 12 } else { 200 };
    let mut tiff = TiffBytes::new(true, 700 + jpeg.len())
        .header(8)
        .ifd(8, 0, &[long(0x8769, 60)])
        .ifd(60, 0, &[at(0x927C, 7, 100, 200)]);
    if sony_dsc_header {
        tiff = tiff.put(200, b"SONY DSC \0\0\0");
    }
    let data = tiff
        .ifd(note_ifd, 0, &[at(0x2001, 7, len_u32(&jpeg), 700)])
        .put(700, &jpeg)
        .into_vec();

    let info = read_ok(data);

    let preview = info.previews.first().expect("one preview");
    assert_eq!(info.previews.len(), 1);
    assert_eq!(
        (preview.offset, preview.length),
        (700, i64::try_from(jpeg.len()).expect("fits"))
    );
}

// C#: TiffReaderRegressionTests.Readers_ArrayEntryCountAboveInt32Max_DoNotThrowUnexpectedExceptions [InlineData(".arw")]
#[test]
fn readers_array_entry_count_above_int32_max_do_not_throw_unexpected_exceptions_arw() {
    let cr2_marker = b"CR\x02\0\0\0\0\0";
    let data = TiffBytes::new(true, 512)
        .header(16)
        .put(8, cr2_marker)
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

    match read_arw(data) {
        Ok(_) | Err(RawHeaderError::InvalidData { .. }) => {}
        Err(other) => panic!("only InvalidData may escape the reader, got {other:?}"),
    }
}

// ---------------------------------------------------------------- TiffSensorSizeTests (ARW rows)

/// Crop variants of the raw IFD's DefaultCropSize (C# `TiffSensorSizeTests.Crop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Crop {
    Absent,
    Shorts,
    Longs,
    Rationals,
}

const RAW_WIDTH: u32 = 6048;
const RAW_HEIGHT: u32 = 4024;

/// ARW-shaped file: IFD0 -> SubIFD (raw IFD with optional crop) and an optional Exif IFD (C# `BuildRawTiff`, line port).
fn build_raw_tiff(crop: Crop, crop_width: u32, crop_height: u32, exif: Option<(u32, u32)>) -> Vec<u8> {
    let mut raw_entries = vec![
        long(0x00FE, 0),
        short(0x0100, RAW_WIDTH),
        short(0x0101, RAW_HEIGHT),
        short(0x0103, 32767),
    ];
    match crop {
        Crop::Absent => {}
        // Two SHORTs fit inline in the 4-byte value field.
        Crop::Shorts => raw_entries.push(Entry {
            tag: 0xC620,
            ty: 3,
            count: 2,
            value: crop_width | (crop_height << 16),
        }),
        Crop::Longs => raw_entries.push(at(0xC620, 4, 2, 400)),
        Crop::Rationals => raw_entries.push(at(0xC620, 5, 2, 400)),
    }

    let mut ifd0 = vec![long(0x014A, 100)];
    if exif.is_some() {
        ifd0.push(long(0x8769, 600));
    }

    let mut file = TiffBytes::new(true, 1000)
        .header(8)
        .ifd(8, 0, &ifd0)
        .ifd(100, 0, &raw_entries);
    match crop {
        Crop::Longs => file = file.u32(400, crop_width).u32(404, crop_height),
        Crop::Rationals => file = file.u32(400, crop_width).u32(404, 1).u32(408, crop_height).u32(412, 1),
        Crop::Absent | Crop::Shorts => {}
    }
    if let Some((w, h)) = exif {
        file = file.ifd(600, 0, &[long(0xA002, w), long(0xA003, h)]);
    }
    file.into_vec()
}

// C#: TiffSensorSizeTests.ArwReader_DefaultCropSize_BeatsRawIfdSizeAndExifSize [InlineData(Crop.Shorts)]
#[test]
fn arw_reader_default_crop_size_beats_raw_ifd_size_and_exif_size_shorts() {
    let info = read_ok(build_raw_tiff(Crop::Shorts, 6000, 4000, Some((5999, 3999))));
    assert_eq!((info.sensor_width, info.sensor_height), (6000, 4000));
}

// C#: TiffSensorSizeTests.ArwReader_DefaultCropSize_BeatsRawIfdSizeAndExifSize [InlineData(Crop.Longs)]
#[test]
fn arw_reader_default_crop_size_beats_raw_ifd_size_and_exif_size_longs() {
    let info = read_ok(build_raw_tiff(Crop::Longs, 6000, 4000, Some((5999, 3999))));
    assert_eq!((info.sensor_width, info.sensor_height), (6000, 4000));
}

// C#: TiffSensorSizeTests.ArwReader_DefaultCropSize_BeatsRawIfdSizeAndExifSize [InlineData(Crop.Rationals)]
#[test]
fn arw_reader_default_crop_size_beats_raw_ifd_size_and_exif_size_rationals() {
    let info = read_ok(build_raw_tiff(Crop::Rationals, 6000, 4000, Some((5999, 3999))));
    assert_eq!((info.sensor_width, info.sensor_height), (6000, 4000));
}

// C#: TiffSensorSizeTests.ArwReader_NoCropSize_UsesExifPixelDimensions
#[test]
fn arw_reader_no_crop_size_uses_exif_pixel_dimensions() {
    let info = read_ok(build_raw_tiff(Crop::Absent, 6000, 4000, Some((6000, 4000))));
    assert_eq!((info.sensor_width, info.sensor_height), (6000, 4000));
}

// C#: TiffSensorSizeTests.ArwReader_NoCropSizeNoExif_FallsBackToRawIfdSize
#[test]
fn arw_reader_no_crop_size_no_exif_falls_back_to_raw_ifd_size() {
    let info = read_ok(build_raw_tiff(Crop::Absent, 6000, 4000, None));
    assert_eq!(
        (info.sensor_width, info.sensor_height),
        (RAW_WIDTH as i32, RAW_HEIGHT as i32)
    );
}

// C#: TiffSensorSizeTests.ArwReader_CropAndExifLargerThanRawIfd_AreIgnored
#[test]
fn arw_reader_crop_and_exif_larger_than_raw_ifd_are_ignored() {
    let info = read_ok(build_raw_tiff(Crop::Longs, 7000, 5000, Some((7000, 5000))));
    assert_eq!(
        (info.sensor_width, info.sensor_height),
        (RAW_WIDTH as i32, RAW_HEIGHT as i32)
    );
}

// ---------------------------------------------------------------- TiffSubIfdTypeTests (ARW rows)

/// IFD0 (no size) -> SubIFD raw IFD of 6048x4024 through a SubIFDs entry of `ty`, inline or as an array (C# `Build`).
fn build_sub_ifd_type(ty: u16, array: bool) -> Vec<u8> {
    let mut file = TiffBytes::new(true, 500).header(8).ifd(
        8,
        0,
        &[if array {
            at(0x014A, ty, 2, 300)
        } else {
            at(0x014A, ty, 1, 100)
        }],
    );
    file = file.ifd(
        100,
        0,
        &[
            long(0x00FE, 0),
            short(0x0100, RAW_WIDTH),
            short(0x0101, RAW_HEIGHT),
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

// C#: TiffSubIfdTypeTests.SubIfdPointers_OfLongOrIfdType_ReachTheRawIfd [arw, 4, inline]
#[test]
fn sub_ifd_pointers_of_long_type_inline_reach_the_raw_ifd_arw() {
    let info = read_ok(build_sub_ifd_type(4, false));
    assert_eq!(
        (info.sensor_width, info.sensor_height),
        (RAW_WIDTH as i32, RAW_HEIGHT as i32)
    );
}

// C#: TiffSubIfdTypeTests.SubIfdPointers_OfLongOrIfdType_ReachTheRawIfd [arw, 4, array]
#[test]
fn sub_ifd_pointers_of_long_type_array_reach_the_raw_ifd_arw() {
    let info = read_ok(build_sub_ifd_type(4, true));
    assert_eq!(
        (info.sensor_width, info.sensor_height),
        (RAW_WIDTH as i32, RAW_HEIGHT as i32)
    );
}

// C#: TiffSubIfdTypeTests.SubIfdPointers_OfLongOrIfdType_ReachTheRawIfd [arw, 13, inline]
#[test]
fn sub_ifd_pointers_of_ifd_type_inline_reach_the_raw_ifd_arw() {
    let info = read_ok(build_sub_ifd_type(13, false));
    assert_eq!(
        (info.sensor_width, info.sensor_height),
        (RAW_WIDTH as i32, RAW_HEIGHT as i32)
    );
}

// C#: TiffSubIfdTypeTests.SubIfdPointers_OfLongOrIfdType_ReachTheRawIfd [arw, 13, array]
#[test]
fn sub_ifd_pointers_of_ifd_type_array_reach_the_raw_ifd_arw() {
    let info = read_ok(build_sub_ifd_type(13, true));
    assert_eq!(
        (info.sensor_width, info.sensor_height),
        (RAW_WIDTH as i32, RAW_HEIGHT as i32)
    );
}

// ---------------------------------------------------------------- TiffPreviewHardeningTests (ARW rows)

const JPEG_AT: u32 = 1000;

/// IFD0 (orientation only) -> IFD at 100 declaring 9000x6000 with a JPEG pointer at `JPEG_AT`; `jpeg` is written there when given,
/// the declared length is 200 otherwise (C# `Build`, arw row).
fn build_declared_preview(jpeg: Option<&[u8]>) -> Vec<u8> {
    let declared = jpeg.map_or(200, len_u32);
    let mut file = TiffBytes::new(true, 3000)
        .header(8)
        .ifd(8, 100, &[short(0x0112, 1)])
        .ifd(
            100,
            0,
            &[
                short(0x0100, 9000),
                short(0x0101, 6000),
                long(0x0201, JPEG_AT),
                long(0x0202, declared),
            ],
        );
    if let Some(bytes) = jpeg {
        file = file.put(JPEG_AT as usize, bytes);
    }
    file.into_vec()
}

/// Same layout as `build_declared_preview`, sized to hold `jpeg` (C# `BuildWithJpeg`, arw row).
fn build_declared_preview_with_jpeg(jpeg: &[u8]) -> Vec<u8> {
    TiffBytes::new(true, JPEG_AT as usize + jpeg.len())
        .header(8)
        .ifd(8, 100, &[short(0x0112, 1)])
        .ifd(
            100,
            0,
            &[
                short(0x0100, 9000),
                short(0x0101, 6000),
                long(0x0201, JPEG_AT),
                long(0x0202, len_u32(jpeg)),
            ],
        )
        .put(JPEG_AT as usize, jpeg)
        .into_vec()
}

// C#: TiffPreviewHardeningTests.Read_JpegInterchangePointerToZeroPadding_IsNotAPreview [arw]
#[test]
fn read_jpeg_interchange_pointer_to_zero_padding_is_not_a_preview_arw() {
    let info = read_ok(build_declared_preview(None));

    assert!(info.previews.iter().all(|p| p.offset != i64::from(JPEG_AT)));
}

// C#: TiffPreviewHardeningTests.Read_DeclaredSizeContradictingTheJpegFrame_UsesTheFrameSize [arw]
#[test]
fn read_declared_size_contradicting_the_jpeg_frame_uses_the_frame_size_arw() {
    let jpeg = create_minimal_jpeg(320, 240);

    let info = read_ok(build_declared_preview(Some(&jpeg)));

    let preview = single_preview_at(&info, i64::from(JPEG_AT));
    assert_eq!((preview.width, preview.height), (320, 240));
}

// C#: TiffPreviewHardeningTests.Read_PreviewJpegExhaustingHeaderBudget_ContainerStillParsesAndKeepsDeclaredSize [arw]
#[test]
fn read_preview_jpeg_exhausting_header_budget_container_still_parses_and_keeps_declared_size_arw() {
    let info = read_ok(build_declared_preview_with_jpeg(&budget_exhausting_jpeg()));

    let preview = single_preview_at(&info, i64::from(JPEG_AT));
    assert!(
        preview.width == 0 || preview.width == 9000,
        "arw: width {}",
        preview.width
    );
    assert!(
        preview.height == 0 || preview.height == 6000,
        "arw: height {}",
        preview.height
    );
}

/// IFD0 at 8 (Exif pointer, IFD1-less), a width-only Exif IFD at 300 first, then the raw IFD at 100 (6048x4024) whose Exif IFD at 400
/// holds 6000x4000 (C# `BuildWidthOnlyThenComplete`).
fn build_width_only_then_complete() -> Vec<u8> {
    TiffBytes::new(true, 1000)
        .header(8)
        .ifd(8, 100, &[short(0x0112, 1), long(0x8769, 300)])
        .ifd(
            100,
            0,
            &[short(0x0100, RAW_WIDTH), short(0x0101, RAW_HEIGHT), long(0x8769, 400)],
        )
        .ifd(300, 0, &[long(0xA002, 5000)])
        .ifd(400, 0, &[long(0xA002, 6000), long(0xA003, 4000)])
        .into_vec()
}

// C#: TiffPreviewHardeningTests.Read_WidthOnlyExifIfdFirst_DoesNotBlockALaterCompleteExifSize [arw]
#[test]
fn read_width_only_exif_ifd_first_does_not_block_a_later_complete_exif_size_arw() {
    let info = read_ok(build_width_only_then_complete());

    assert_eq!(info.sensor_width, 6000);
    assert_eq!(info.sensor_height, 4000);
}

// ---------------------------------------------------------------- TiffExifBlockPlacementTests (ARW row, block lengths only)

/// IFD0 at 8 (dimensions, Make far away, Exif pointer); the Exif IFD (ISO) sits far away as well (C# `Build`).
fn build_exif_placement(size: usize, make_offset: u32, exif_offset: u32) -> Vec<u8> {
    TiffBytes::new(true, size)
        .header(8)
        .ifd(
            8,
            0,
            &[
                short(0x0100, 4000),
                short(0x0101, 3000),
                at(0x010F, 2, 6, make_offset),
                long(0x8769, exif_offset),
            ],
        )
        .put(make_offset as usize, b"Acme\0\0")
        .ifd(exif_offset as usize, 0, &[short(0x8827, 400)])
        .into_vec()
}

// C#: TiffExifBlockPlacementTests.Read_IfdsInsideDefaultWindow_KeepsDefaultBlockLength [arw]
#[test]
fn read_ifds_inside_default_window_keeps_default_block_length_arw() {
    let info = read_ok(build_exif_placement(310_000, 2000, 3000));

    for block in &info.exif_blocks {
        assert_eq!(block.length, i64::from(limits::DEFAULT_EXIF_BLOCK_BYTES));
    }
}

// C#: TiffExifBlockPlacementTests.Read_IfdsBeyondMaxBlock_CapsBlockAndDoesNotThrow [arw]
#[test]
fn read_ifds_beyond_max_block_caps_block_and_does_not_throw_arw() {
    let far = limits::MAX_EXIF_BLOCK_BYTES as usize + 50_000;
    let info = read_ok(build_exif_placement(far + 10_000, far as u32, (far + 100) as u32));

    // An extent that cannot fit keeps the default block (C# was grown to the 4 MiB cap for nothing).
    for block in &info.exif_blocks {
        assert_eq!(block.length, i64::from(limits::DEFAULT_EXIF_BLOCK_BYTES));
    }
}

// ---------------------------------------------------------------- MakerNoteGapTests (Sony half)

/// IFD0 at 8 (ExifIFD pointer + a 0x2001 entry whose length is absurd) and the Exif IFD at 60 whose MakerNote offset is
/// `note_offset`, i.e. it points into IFD0 itself or into the middle of one of its entries (C# `SonyWithNoteInsideAnotherIfd`).
fn sony_with_note_inside_another_ifd(note_offset: u32) -> Vec<u8> {
    let jpeg = baseline_jpeg();
    TiffBytes::new(true, 700 + jpeg.len())
        .header(8)
        .ifd(8, 0, &[long(0x8769, 60), at(0x2001, 7, 0x7FFF_FFF0, 700)])
        .ifd(60, 0, &[at(0x927C, 7, 100, note_offset)])
        .put(700, &jpeg)
        .into_vec()
}

// C#: MakerNoteGapTests.ArwReader_MakerNoteOffsetInsideAnotherIfd_ListsNoPreviewAndDoesNotThrow [InlineData(8u)]
/// Exactly IFD0: its entries are read as the note's entries.
#[test]
fn arw_reader_maker_note_offset_inside_another_ifd_lists_no_preview_at_8() {
    assert_note_inside_another_ifd_lists_nothing(8);
}

// C#: MakerNoteGapTests.ArwReader_MakerNoteOffsetInsideAnotherIfd_ListsNoPreviewAndDoesNotThrow [InlineData(10u)]
/// First entry, misaligned by two bytes.
#[test]
fn arw_reader_maker_note_offset_inside_another_ifd_lists_no_preview_at_10() {
    assert_note_inside_another_ifd_lists_nothing(10);
}

// C#: MakerNoteGapTests.ArwReader_MakerNoteOffsetInsideAnotherIfd_ListsNoPreviewAndDoesNotThrow [InlineData(18u)]
/// Second entry.
#[test]
fn arw_reader_maker_note_offset_inside_another_ifd_lists_no_preview_at_18() {
    assert_note_inside_another_ifd_lists_nothing(18);
}

// C#: MakerNoteGapTests.ArwReader_MakerNoteOffsetInsideAnotherIfd_ListsNoPreviewAndDoesNotThrow [InlineData(62u)]
/// Inside the Exif IFD that holds the pointer.
#[test]
fn arw_reader_maker_note_offset_inside_another_ifd_lists_no_preview_at_62() {
    assert_note_inside_another_ifd_lists_nothing(62);
}

fn assert_note_inside_another_ifd_lists_nothing(note_offset: u32) {
    let info = read_ok(sony_with_note_inside_another_ifd(note_offset));
    assert!(info.previews.is_empty(), "note at {note_offset}: {:?}", info.previews);
}

// C#: MakerNoteGapTests.ArwReader_MakerNoteWithValidPreviewEntry_ListsItExactlyOnce_Control
#[test]
fn arw_reader_maker_note_with_valid_preview_entry_lists_it_exactly_once_control() {
    let jpeg = baseline_jpeg();
    let data = TiffBytes::new(true, 700 + jpeg.len())
        .header(8)
        .ifd(8, 0, &[long(0x8769, 60)])
        .ifd(60, 0, &[at(0x927C, 7, 100, 200)])
        .ifd(200, 0, &[at(0x2001, 7, len_u32(&jpeg), 700)])
        .put(700, &jpeg)
        .into_vec();

    let info = read_ok(data);

    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].offset, 700);
}

// ---------------------------------------------------------------- Rust-only robustness (contract W5-07, "Done when")

/// The three synthetic shapes every hostile-input test walks: IFD0 JPEG, SubIFD + Exif + MakerNote, and the 66-IFD chain.
fn robustness_shapes() -> Vec<(&'static str, Vec<u8>)> {
    let jpeg = baseline_jpeg();
    let jl = len_u32(&jpeg);

    // SubIFD raw image with a JPEG, Exif IFD -> MakerNote IFD ("SONY DSC " header) -> PreviewImage at 700.
    let sub_exif_note = TiffBytes::new(true, 700 + jpeg.len())
        .header(8)
        .ifd(8, 0, &[long(0x014A, 100), long(0x8769, 200)])
        .ifd(
            100,
            0,
            &[
                short(0x0100, 6048),
                short(0x0101, 4024),
                long(0x0201, 700),
                long(0x0202, jl),
            ],
        )
        .ifd(200, 0, &[at(0x927C, 7, 100, 300)])
        .put(300, b"SONY DSC \0\0\0")
        .ifd(312, 0, &[at(0x2001, 7, jl, 700)])
        .put(700, &jpeg)
        .into_vec();

    let chain = {
        const IFD_SIZE: usize = 30;
        let mut file = TiffBytes::new(true, 8 + 66 * IFD_SIZE).header(8);
        for i in 0..66usize {
            let at_offset = 8 + i * IFD_SIZE;
            let next = if i < 65 {
                u32::try_from(at_offset + IFD_SIZE).expect("fits")
            } else {
                0
            };
            file = file.ifd(at_offset, next, &[short(0x0100, 100 + i as u32), short(0x0101, 100)]);
        }
        file.into_vec()
    };

    vec![
        ("ifd0-jpeg", build_tiff(true, None, 1, 42)),
        ("subifd-exif-makernote", sub_exif_note),
        ("ifd-chain-66", chain),
    ]
}

/// A `RawHeaderSource` double that returns empty or truncated slices for out-of-range and odd reads (C# `FuzzTrackingHeaderSource`
/// behaviour, extended with truncation): the reader must never index past the slice it was given.
struct ShortSpanSource {
    bytes: Vec<u8>,
    calls: usize,
}

impl RawHeaderSource for ShortSpanSource {
    fn length(&self) -> i64 {
        i64::try_from(self.bytes.len()).unwrap_or(i64::MAX)
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.calls += 1;
        let len = self.length();
        if offset < 0 || offset >= len || count <= 0 {
            return Ok(&[]);
        }
        let mut available = usize::try_from(i64::from(count).min(len - offset)).expect("fits usize");
        if self.calls % 3 == 0 {
            available /= 2;
        }
        let start = usize::try_from(offset).expect("fits usize");
        Ok(&self.bytes[start..start + available])
    }
}

/// A source that fails with an inner I/O error from the `fail_from`-th read on (C# `InvalidDataException` with an inner exception).
struct IoFailingSource {
    inner: InMemoryRawHeaderSource,
    calls: usize,
    fail_from: usize,
}

impl RawHeaderSource for IoFailingSource {
    fn length(&self) -> i64 {
        self.inner.length()
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let call = self.calls;
        self.calls += 1;
        if call >= self.fail_from {
            return Err(RawHeaderError::invalid_data_io(
                "disk read failed",
                io::Error::other("disk read failed"),
            ));
        }
        self.inner.read(offset, count)
    }
}

/// Cancels the shared token right after the first read (the header), so the next loop-top check observes it.
struct CancelAfterHeaderSource {
    inner: InMemoryRawHeaderSource,
    token: CancelToken,
    calls: usize,
}

impl RawHeaderSource for CancelAfterHeaderSource {
    fn length(&self) -> i64 {
        self.inner.length()
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let call = self.calls;
        self.calls += 1;
        let result = self.inner.read(offset, count);
        if call == 0 {
            self.token.cancel();
        }
        result
    }
}

// Rust-only (contract W5-07, Done when (b)): every prefix of each valid shape returns Ok or InvalidData, never panics.
#[test]
fn read_never_panics_on_every_truncation_of_valid_files() {
    for (shape, full) in robustness_shapes() {
        for keep in 0..=full.len() {
            match read_arw(full[..keep].to_vec()) {
                Ok(_) | Err(RawHeaderError::InvalidData { .. }) => {}
                Err(other) => panic!("{shape} truncated to {keep} bytes: unexpected {other:?}"),
            }
        }
    }
}

// Rust-only (contract W5-07, Done when (c)): a source that returns short slices must not make the reader index past them.
#[test]
fn read_with_short_span_source_never_panics() {
    for (shape, full) in robustness_shapes() {
        let mut source = ShortSpanSource { bytes: full, calls: 0 };
        match ArwContainerReader.read(&mut source, &CancelToken::none()) {
            Ok(_) | Err(RawHeaderError::InvalidData { .. }) => {}
            Err(other) => panic!("{shape} with short spans: unexpected {other:?}"),
        }
    }
}

// Rust-only (contract W5-07, Done when (d)): an I/O failure from the source propagates as the same error, never as "no preview".
#[test]
fn io_failure_from_the_source_propagates() {
    let mut source = IoFailingSource {
        inner: InMemoryRawHeaderSource::new(build_tiff(true, None, 1, 42)),
        calls: 0,
        fail_from: 1, // the header read succeeds, the first IFD read fails
    };

    let err = ArwContainerReader
        .read(&mut source, &CancelToken::none())
        .expect_err("an I/O failure must not be swallowed");

    match &err {
        RawHeaderError::InvalidData { message, io: Some(_) } => assert_eq!(message, "disk read failed"),
        other => panic!("expected the source's InvalidData with its io error, got {other:?}"),
    }
    assert!(err.is_io_failure());
}

// Rust-only (contract W5-07, Done when (e)): a token canceled inside the source after the header read is observed before each IFD.
#[test]
fn canceled_token_is_checked_before_each_ifd() {
    let token = CancelToken::new();
    let mut source = CancelAfterHeaderSource {
        inner: InMemoryRawHeaderSource::new(build_tiff(true, None, 1, 42)),
        token: token.clone(),
        calls: 0,
    };

    let result = ArwContainerReader.read(&mut source, &token);

    assert!(
        matches!(result, Err(RawHeaderError::Canceled)),
        "expected Canceled after the header read, got {result:?}"
    );
}

// Rust-only: a token that is already canceled stops the reader before IFD0 (the top-of-loop check runs first).
#[test]
fn already_canceled_token_stops_the_reader_before_ifd0() {
    let token = CancelToken::new();
    token.cancel();
    let mut source = InMemoryRawHeaderSource::new(build_tiff(true, None, 1, 42));

    let result = ArwContainerReader.read(&mut source, &token);

    assert!(matches!(result, Err(RawHeaderError::Canceled)), "got {result:?}");
}

// ---------------------------------------------------------------- Robustness/RawContainerFuzzTests.Fuzz_ArwContainerReader_Robust

/// C# `JpegBytes.Mutate`: 1..=3 rounds of one of nine mutations (xorshift instead of `System.Random`, AGENTS.md "Tests").
fn mutate(rng: &mut XorShift64, input: &[u8]) -> Vec<u8> {
    let mut data = input.to_vec();
    let rounds = 1 + rng.below(3);
    for _ in 0..rounds {
        if data.is_empty() {
            data.push(u8::try_from(rng.below(256)).expect("byte"));
            continue;
        }
        match rng.below(9) {
            0 => {
                let p = rng.below(data.len());
                data[p] = u8::try_from(rng.below(256)).expect("byte");
            }
            1 => {
                let p = rng.below(data.len());
                let bit = rng.below(8);
                data[p] ^= 1u8 << bit;
            }
            2 => {
                let pos = rng.below(data.len());
                let n = (1 + rng.below(8)).min(data.len() - pos);
                data.drain(pos..pos + n);
            }
            3 => {
                let at_pos = rng.below(data.len() + 1);
                let n = 1 + rng.below(8);
                let inserted: Vec<u8> = (0..n).map(|_| u8::try_from(rng.below(256)).expect("byte")).collect();
                data.splice(at_pos..at_pos, inserted);
            }
            4 => {
                let keep = rng.below(data.len() + 1);
                data.truncate(keep);
            }
            5 => {
                let p = rng.below(data.len());
                data[p] = 0xFF;
            }
            6 => {
                let p = rng.below(data.len());
                data[p] = 0x00;
            }
            7 => {
                let start = rng.below(data.len());
                let n = (1 + rng.below(23)).min(data.len() - start);
                let slice = data[start..start + n].to_vec();
                let at_pos = rng.below(data.len() + 1);
                data.splice(at_pos..at_pos, slice);
            }
            _ => {
                let pos = rng.below(data.len());
                let extreme: [u8; 2] = match rng.below(3) {
                    0 => [0xFF, 0xFF],
                    1 => [0x7F, 0xFF],
                    _ => [0x00, 0x00],
                };
                for (i, byte) in extreme.iter().enumerate() {
                    if pos + i < data.len() {
                        data[pos + i] = *byte;
                    }
                }
            }
        }
    }
    data
}

/// C# `FuzzTrackingHeaderSource`: empty slice out of range, `InvalidData` once the header budget is exceeded.
struct FuzzTrackingHeaderSource {
    bytes: Vec<u8>,
    total_bytes_read: i64,
}

impl RawHeaderSource for FuzzTrackingHeaderSource {
    fn length(&self) -> i64 {
        i64::try_from(self.bytes.len()).unwrap_or(i64::MAX)
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let len = self.length();
        if offset < 0 || offset >= len || count <= 0 {
            return Ok(&[]);
        }
        let available = i64::from(count).min(len - offset);
        self.total_bytes_read += available;
        if self.total_bytes_read > i64::from(limits::MAX_HEADER_BYTES) {
            return Err(RawHeaderError::invalid_data("Exceeded maximum header read budget."));
        }
        let start = usize::try_from(offset).expect("fits usize");
        let end = start + usize::try_from(available).expect("fits usize");
        Ok(&self.bytes[start..end])
    }
}

// C#: Robustness/RawContainerFuzzTests.Fuzz_ArwContainerReader_Robust (2 000 iterations, p95 assert dropped, D54).
#[test]
fn fuzz_arw_container_reader_robust() {
    const ITERATIONS: usize = 2_000;
    let seed = build_tiff(true, None, 1, 42);
    let reader = ArwContainerReader;
    let mut rng = XorShift64::new(103);

    for iteration in 0..ITERATIONS {
        let mutated = mutate(&mut rng, &seed);
        let mut source = FuzzTrackingHeaderSource {
            bytes: mutated.clone(),
            total_bytes_read: 0,
        };

        if reader.can_read(&mutated[..mutated.len().min(64)], ".arw") {
            match reader.read(&mut source, &CancelToken::none()) {
                Ok(info) => assert!(
                    (1..=8).contains(&info.orientation),
                    "iteration {iteration}: orientation {} outside 1..=8",
                    info.orientation
                ),
                Err(RawHeaderError::InvalidData { .. }) => {}
                Err(other) => panic!("iteration {iteration}: only InvalidData may escape, got {other:?}"),
            }
        }

        assert!(
            source.total_bytes_read <= i64::from(limits::MAX_HEADER_BYTES),
            "iteration {iteration}: read {} bytes, budget {}",
            source.total_bytes_read,
            limits::MAX_HEADER_BYTES
        );
    }
}

// ---------------------------------------------------------------- golden tests/golden/raw-arw-reader.json

/// Line port of the oracle's `W507Tiff` (tests/golden-inputs spec): a zero-filled buffer with header, IFD, and raw writes.
struct SpecTiff {
    little: bool,
    buf: Vec<u8>,
}

impl SpecTiff {
    fn new(little: bool, size: usize) -> Self {
        Self {
            little,
            buf: vec![0; size],
        }
    }

    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.buf[at..at + bytes.len()].copy_from_slice(bytes);
    }

    fn u16(&mut self, at: usize, value: u16) {
        let raw = if self.little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        self.put(at, &raw);
    }

    fn u32(&mut self, at: usize, value: u32) {
        let raw = if self.little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        self.put(at, &raw);
    }

    /// Entry kinds: `short` (type 3, count 1), `long` (type 4, count 1), `at` / `raw` (explicit type, count, value). `pad` filler
    /// SHORT entries (tag 0xF000, value 1) come BEFORE the listed entries, so a hostile count (4096, 4097) needs no JSON lines.
    fn ifd(&mut self, at: usize, next: u32, entries: &[Value], pad: usize) {
        let total = pad + entries.len();
        self.u16(at, u16::try_from(total).expect("entry count fits u16"));
        for i in 0..total {
            let p = at + 2 + i * 12;
            let (tag, ty, count, value): (u16, u16, u32, u32) = if i < pad {
                (0xF000, 3, 1, 1)
            } else {
                let e = &entries[i - pad];
                let tag = spec_u16(&e["tag"]);
                match e["k"].as_str().expect("entry kind") {
                    "short" => (tag, 3, 1, spec_u32(&e["v"])),
                    "long" => (tag, 4, 1, spec_u32(&e["v"])),
                    "at" | "raw" => (tag, spec_u16(&e["type"]), spec_u32(&e["count"]), spec_u32(&e["v"])),
                    other => panic!("unknown entry kind {other}"),
                }
            };
            self.u16(p, tag);
            self.u16(p + 2, ty);
            self.u32(p + 4, count);
            if ty == 3 && count == 1 {
                self.u16(p + 8, value as u16); // SHORT left-justified in the value field
            } else {
                self.u32(p + 8, value);
            }
        }
        self.u32(at + 2 + total * 12, next);
    }
}

fn spec_u32(v: &Value) -> u32 {
    u32::try_from(v.as_u64().expect("unsigned value")).expect("value fits u32")
}

fn spec_u16(v: &Value) -> u16 {
    u16::try_from(v.as_u64().expect("unsigned value")).expect("value fits u16")
}

fn spec_usize(v: &Value) -> usize {
    usize::try_from(v.as_u64().expect("unsigned value")).expect("value fits usize")
}

/// Builds the bytes of a golden `tiff` source (oracle `W507Build`, `kind` = tiff).
fn build_tiff_spec(source: &Value) -> Vec<u8> {
    let little = source["le"].as_bool().unwrap_or(true);
    let mut tiff = SpecTiff::new(little, spec_usize(&source["size"]));
    let header = &source["header"];
    let magic: Vec<u8> = match header["signature_hex"].as_str() {
        Some(hex) => from_hex(hex),
        None if little => b"II*\0".to_vec(),
        None => b"MM\0*".to_vec(),
    };
    tiff.put(0, &magic);
    tiff.u32(4, spec_u32(&header["ifd0"]));

    for op in source["ops"].as_array().expect("tiff ops") {
        let at = spec_usize(&op["at"]);
        match op["op"].as_str().expect("op name") {
            "ifd" => {
                let entries = op["entries"].as_array().expect("entries");
                let pad = op["pad"]
                    .as_u64()
                    .map_or(0, |p| usize::try_from(p).expect("pad fits usize"));
                tiff.ifd(at, spec_u32(&op["next"]), entries, pad);
            }
            "u16" => tiff.u16(at, spec_u16(&op["v"])),
            "u32" => tiff.u32(at, spec_u32(&op["v"])),
            "hex" => tiff.put(at, &from_hex(op["hex"].as_str().expect("hex"))),
            "ascii" => {
                let text = op["text"].as_str().expect("text");
                let bytes: Vec<u8> = text
                    .chars()
                    .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
                    .collect();
                tiff.put(at, &bytes);
            }
            "jpeg" => {
                let w = i32::try_from(op["w"].as_u64().expect("w")).expect("w fits i32");
                let h = i32::try_from(op["h"].as_u64().expect("h")).expect("h fits i32");
                tiff.put(at, &create_minimal_jpeg(w, h));
            }
            "budget_jpeg" => tiff.put(at, &budget_exhausting_jpeg()),
            other => panic!("unknown tiff op {other}"),
        }
    }
    tiff.buf
}

/// Bytes of a golden source, or `None` when an optional corpus sample is absent (oracle `W507Build`, all kinds).
fn build_source(source: &Value, optional: bool) -> Option<Vec<u8>> {
    match source["kind"].as_str().expect("source kind") {
        "hex" => Some(from_hex(source["hex"].as_str().expect("hex"))),
        "tiff" => Some(build_tiff_spec(source)),
        "corpus" => corpus_sample(source, optional),
        other => panic!("unknown source kind {other}"),
    }
}

/// One golden case, in the oracle's shape (`W507` `RawArwReader`). `None` when an optional corpus sample is absent.
fn run_golden_case(case: &Value) -> Option<Value> {
    let name = golden::str_of(case, "name");
    let op = case["op"].as_str().unwrap_or("read");
    let ext = case["ext"].as_str().unwrap_or(".arw");
    let optional = case["optional"].as_bool().unwrap_or(false);
    let bytes = build_source(&case["source"], optional)?;

    let mut out = Map::new();
    out.insert("name".to_owned(), json!(name));
    out.insert("op".to_owned(), json!(op));
    let can_read = ArwContainerReader.can_read(&bytes[..bytes.len().min(64)], ext);
    out.insert("can_read".to_owned(), json!(can_read));
    if op == "read" {
        let mut source = InMemoryRawHeaderSource::new(bytes);
        let token = CancelToken::new();
        if case["cancel"].as_bool() == Some(true) {
            token.cancel();
        }
        match ArwContainerReader.read(&mut source, &token) {
            Ok(info) => {
                out.insert("ok".to_owned(), json!(true));
                out.insert("format".to_owned(), json!(format!("{:?}", info.format)));
                out.insert("sensor_width".to_owned(), json!(info.sensor_width));
                out.insert("sensor_height".to_owned(), json!(info.sensor_height));
                out.insert("orientation".to_owned(), json!(info.orientation));
                out.insert(
                    "previews".to_owned(),
                    Value::Array(info.previews.iter().map(preview_json).collect()),
                );
                out.insert(
                    "exif_blocks".to_owned(),
                    Value::Array(info.exif_blocks.iter().map(exif_block_json).collect()),
                );
            }
            Err(err) => {
                let (exception, message) = error_json(&err);
                out.insert("ok".to_owned(), json!(false));
                out.insert("exception".to_owned(), json!(exception));
                out.insert("reader_message".to_owned(), message);
            }
        }
        out.insert("bytes_charged".to_owned(), json!(source.total_bytes_read()));
    }
    Some(Value::Object(out))
}

fn preview_json(p: &EmbeddedPreview) -> Value {
    let kind = match p.kind {
        EmbeddedPreviewKind::Jpeg => "Jpeg",
    };
    let color_space = match p.color_space {
        PreviewColorSpace::Unknown => "Unknown",
        PreviewColorSpace::Srgb => "Srgb",
        PreviewColorSpace::AdobeRgb => "AdobeRgb",
    };
    json!({
        "index": p.index,
        "offset": p.offset,
        "length": p.length,
        "kind": kind,
        "width": p.width,
        "height": p.height,
        "color_space": color_space,
        "header_resolved": p.header_resolved,
    })
}

fn exif_block_json(b: &ExifBlock) -> Value {
    json!({
        "offset": b.offset,
        "length": b.length,
        "is_tiff_header": b.is_tiff_header,
        "ifd_is_exif": b.ifd_is_exif,
    })
}

/// C# exception name and `reader_message`: only the two texts the reader itself raises are pinned (oracle `W507` rule).
fn error_json(err: &RawHeaderError) -> (&'static str, Value) {
    match err {
        RawHeaderError::InvalidData { message, .. } => {
            let text = match message.as_str() {
                "File too short for ARW container." | "Invalid ARW TIFF header." => json!(message),
                _ => Value::Null,
            };
            ("InvalidDataException", text)
        }
        RawHeaderError::ArgumentOutOfRange { .. } => ("ArgumentOutOfRangeException", Value::Null),
        RawHeaderError::Canceled => ("OperationCanceledException", Value::Null),
    }
}

/// Corpus sample named by a golden source: absent and optional gives `None`, absent otherwise panics (fail closed); a hash
/// mismatch always panics.
fn corpus_sample(source: &Value, optional: bool) -> Option<Vec<u8>> {
    let file = source["file"].as_str().expect("corpus file name");
    let path = corpus_dir().join(file);
    if !path.is_file() {
        if optional {
            return None;
        }
        panic!("RAW corpus sample missing (fetch it first): {}", path.display());
    }
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let expected = source["sha256"].as_str().expect("sha256");
    let actual: String = sha256(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    assert!(
        actual.eq_ignore_ascii_case(expected),
        "SHA-256 of {} is {actual}, expected {expected}",
        path.display()
    );
    Some(bytes)
}

fn golden_inputs() -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("raw-arw-reader.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"].as_array().expect("cases array").clone()
}

// C#: tests/golden/raw-arw-reader.json (oracle W5-07.cs `raw-arw-reader`): 181 cases, every output field (corpus cases compare
// only when the sample is present; the committed golden records them as skipped, D49).
#[test]
fn golden_raw_arw_reader_all_cases_match_the_oracle() {
    let golden_file = golden::load("raw-arw-reader");
    let inputs = golden_inputs();
    assert_eq!(
        golden_file.cases.len(),
        inputs.len(),
        "golden and input case counts differ"
    );

    let mut mismatches = Vec::new();
    for (expected, input) in golden_file.cases.iter().zip(&inputs) {
        let name = golden::str_of(expected, "name");
        assert_eq!(
            name,
            golden::str_of(input, "name"),
            "case order differs from the input file"
        );

        let Some(actual) = run_golden_case(input) else {
            eprintln!("skipped: corpus sample absent for {name}");
            continue;
        };
        if expected.get("skipped").is_some() {
            eprintln!("skipped: golden has no data for {name} (regenerate once the corpus is fetched, P-30)");
            continue;
        }
        if *expected != actual {
            mismatches.push(format!("[{name}] expected {expected}, got {actual}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} raw-arw-reader golden cases differ from C#:\n{}",
        mismatches.len(),
        golden_file.cases.len(),
        mismatches.join("\n")
    );
}

// ---------------------------------------------------------------- RAW corpus (TiffCorpusTests, RawSizeConsistencyAuditTests: ARW subset)

const CORPUS_DIR_VAR: &str = "PHOTOREVIEW_RAW_CORPUS_DIR";
const CORPUS_STRICT_VAR: &str = "PHOTOREVIEW_RAW_CORPUS_STRICT";

/// `PHOTOREVIEW_RAW_CORPUS_DIR`, else `tests/fixtures/raw-corpus/` of the repository (D49).
fn corpus_dir() -> PathBuf {
    match std::env::var(CORPUS_DIR_VAR) {
        Ok(value) if !value.trim().is_empty() => PathBuf::from(value.trim()),
        _ => golden::repo_root().join("tests").join("fixtures").join("raw-corpus"),
    }
}

/// `PHOTOREVIEW_RAW_CORPUS_STRICT` is `1` or `true` (trimmed).
fn corpus_strict() -> bool {
    matches!(
        std::env::var(CORPUS_STRICT_VAR).ok().as_deref().map(str::trim),
        Some("1" | "true")
    )
}

/// C# `RawCorpus.RequireDirectory`: `true` when the corpus folder exists. Absent: strict panics naming the variable, otherwise the
/// test prints a skip line and returns early.
fn require_corpus_dir() -> bool {
    let dir = corpus_dir();
    if dir.is_dir() {
        return true;
    }
    if corpus_strict() {
        panic!(
            "{CORPUS_STRICT_VAR} is set but the RAW corpus is missing: {CORPUS_DIR_VAR} / {}",
            dir.display()
        );
    }
    eprintln!("skipped: RAW corpus not present at {}", dir.display());
    false
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension().is_some_and(|e| {
        e.to_string_lossy()
            .eq_ignore_ascii_case(extension.trim_start_matches('.'))
    })
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Corpus files with `extension`, ordered like C# `RawCorpus` (ordinal, ignore case).
fn corpus_files_with_extension(extension: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(corpus_dir())
        .expect("read the corpus folder")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && has_extension(p, extension))
        .collect();
    files.sort_by_key(|p| file_name_of(p).to_ascii_lowercase());
    files
}

/// C# `RawCorpus.TryGetFirst(pattern, contains)`: first sample with the extension whose name contains `contains`.
fn corpus_try_get_first(extension: &str, contains: &str) -> Option<PathBuf> {
    if !require_corpus_dir() {
        return None;
    }
    let found = corpus_files_with_extension(extension)
        .into_iter()
        .find(|p| file_name_of(p).contains(contains));
    if found.is_none() {
        eprintln!("skipped: no {extension} sample containing {contains:?}");
    }
    found
}

/// C# `RawCorpus.TryGetFile(name)`: the named sample when present.
fn corpus_named(file_name: &str) -> Option<PathBuf> {
    if !require_corpus_dir() {
        return None;
    }
    let path = corpus_dir().join(file_name);
    if path.is_file() {
        Some(path)
    } else {
        eprintln!("skipped: {file_name} not present in the RAW corpus");
        None
    }
}

/// The reader's view of a corpus file: the first 64 bytes (for `can_read`) and a streaming header source over the file.
fn open_corpus_file(path: &Path) -> (Vec<u8>, SourceRawHeaderSource) {
    let mut head = vec![0u8; 64];
    let mut file = File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let read = file
        .read(&mut head)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    head.truncate(read);
    let file = File::open(path).unwrap_or_else(|e| panic!("reopen {}: {e}", path.display()));
    let source = SourceRawHeaderSource::from_stream(Box::new(file))
        .unwrap_or_else(|e| panic!("header source for {}: {e}", path.display()));
    (head, source)
}

fn read_corpus_arw(path: &Path) -> RawContainerInfo {
    let (_, mut source) = open_corpus_file(path);
    ArwContainerReader
        .read(&mut source, &CancelToken::none())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The bytes `[offset, offset + length)` of a corpus file (C# `ReadRange`).
fn read_corpus_range(path: &Path, offset: i64, length: i64) -> Vec<u8> {
    use std::io::{Seek, SeekFrom};
    let mut file = File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let file_len = i64::try_from(file.metadata().expect("metadata").len()).expect("fits i64");
    assert!(
        offset >= 0 && length > 0 && offset + length <= file_len,
        "{}: range {offset}+{length} outside file of {file_len} bytes",
        file_name_of(path)
    );
    file.seek(SeekFrom::Start(u64::try_from(offset).expect("fits u64")))
        .expect("seek");
    let mut bytes = vec![0u8; usize::try_from(length).expect("fits usize")];
    file.read_exact(&mut bytes).expect("read range");
    bytes
}

// C#: TiffCorpusTests.AllCorpusTiffSamples_CanBeParsed (ARW twin: every sample is read by the ARW reader directly, no registry).
#[test]
fn all_corpus_tiff_samples_can_be_parsed_arw() {
    if !require_corpus_dir() {
        return;
    }
    let mut parsed = 0usize;
    for path in corpus_files_with_extension(".arw") {
        let (head, mut source) = open_corpus_file(&path);
        assert!(
            ArwContainerReader.can_read(&head, ".arw"),
            "{} is not accepted by the ARW reader",
            file_name_of(&path)
        );
        let info = ArwContainerReader
            .read(&mut source, &CancelToken::none())
            .unwrap_or_else(|e| panic!("{}: {e}", file_name_of(&path)));
        assert!(
            !info.previews.is_empty(),
            "{} had 0 previews found",
            file_name_of(&path)
        );
        assert!(
            info.previews[0].length > 0,
            "{} first preview is empty",
            file_name_of(&path)
        );
        parsed += 1;
    }
    if corpus_strict() {
        assert!(parsed >= 3, "expected at least 3 ARW corpus files, parsed {parsed}");
    }
}

// C#: TiffCorpusTests.ArwCorpus_SensorSizeIsActiveAreaNotMaskedRawIfd [InlineData("ILCE-7M3", 6000, 4000)]
#[test]
fn arw_corpus_sensor_size_is_active_area_not_masked_raw_ifd_ilce_7m3() {
    assert_corpus_arw_sensor_size("ILCE-7M3", 6000, 4000);
}

// C#: TiffCorpusTests.ArwCorpus_SensorSizeIsActiveAreaNotMaskedRawIfd [InlineData("ILCE-7R", 7360, 4912)]
#[test]
fn arw_corpus_sensor_size_is_active_area_not_masked_raw_ifd_ilce_7r() {
    assert_corpus_arw_sensor_size("ILCE-7R", 7360, 4912);
}

// C#: TiffCorpusTests.ArwCorpus_SensorSizeIsActiveAreaNotMaskedRawIfd [InlineData("NEX-6", 4912, 3264)]
#[test]
fn arw_corpus_sensor_size_is_active_area_not_masked_raw_ifd_nex_6() {
    assert_corpus_arw_sensor_size("NEX-6", 4912, 3264);
}

fn assert_corpus_arw_sensor_size(model: &str, width: i32, height: i32) {
    let Some(path) = corpus_try_get_first(".arw", model) else {
        return;
    };
    let info = read_corpus_arw(&path);
    assert_eq!((info.sensor_width, info.sensor_height), (width, height), "{model}");
}

// C#: RawSizeConsistencyAuditTests.CorpusFile_ReaderAndLibRawSizes_ArePinned, reader half of the ARW row
// "Sony - ILCE-7M3 ..." (the LibRaw half is W5-16).
#[test]
fn corpus_file_reader_size_is_pinned_sony_ilce_7m3() {
    assert_corpus_reader_size("Sony - ILCE-7M3 - 14bit 14bit compressed (3_2).ARW", 6000, 4000);
}

// C#: RawSizeConsistencyAuditTests.CorpusFile_ReaderAndLibRawSizes_ArePinned, reader half of the ARW row "Sony - ILCE-7R ...".
#[test]
fn corpus_file_reader_size_is_pinned_sony_ilce_7r() {
    assert_corpus_reader_size("Sony - ILCE-7R - 14bit 14bit compressed (3_2).ARW", 7360, 4912);
}

// C#: RawSizeConsistencyAuditTests.CorpusFile_ReaderAndLibRawSizes_ArePinned, reader half of the ARW row "Sony - NEX-6 ...".
#[test]
fn corpus_file_reader_size_is_pinned_sony_nex_6() {
    assert_corpus_reader_size("Sony - NEX-6 - 12bit 12bit compressed (3_2).ARW", 4912, 3264);
}

fn assert_corpus_reader_size(file_name: &str, width: i32, height: i32) {
    let Some(path) = corpus_named(file_name) else {
        return;
    };
    let info = read_corpus_arw(&path);
    assert_eq!((info.sensor_width, info.sensor_height), (width, height), "{file_name}");
}

// C#: TiffCorpusTests.AllTiffCorpusFiles_EveryListedPreviewLiesInsideFileAndStartsWithSoi (ARW subset).
#[test]
fn all_tiff_corpus_files_arw_subset_every_listed_preview_lies_inside_file_and_starts_with_soi() {
    if !require_corpus_dir() {
        return;
    }
    for path in corpus_files_with_extension(".arw") {
        let info = read_corpus_arw(&path);
        for preview in &info.previews {
            let head = read_corpus_range(&path, preview.offset, preview.length.min(2));
            assert_eq!(
                head,
                vec![0xFF, 0xD8],
                "{}: preview at {} does not start with SOI",
                file_name_of(&path),
                preview.offset
            );
        }
    }
}

// A SubIFD pointer of 0 is skipped WITHOUT consuming one of the 64 IFD slots (C#: `ifdOffset <= 0 || !visited.Add` -> continue).
// Kills the `||` -> `&&` mutant at arw.rs `ifd_offset <= 0 || !visited.insert(..)`.
#[test]
fn zero_subifd_pointer_does_not_consume_an_ifd_slot() {
    let jpeg = create_minimal_jpeg(640, 480);
    let sub_base = 300usize;
    let jpeg_at = sub_base + 63 * 30;
    let mut f = TiffBytes::new(true, jpeg_at + jpeg.len()).header(8);
    f = f.ifd(8, 0, &[at(0x014A, 4, 64, 40)]);
    f = f.u32(40, 0);
    for i in 1..64usize {
        f = f.u32(40 + i * 4, u32::try_from(sub_base + (i - 1) * 30).unwrap());
    }
    for i in 0..63usize {
        let pos = sub_base + i * 30;
        if i == 62 {
            f = f.ifd(
                pos,
                0,
                &[
                    long(0x0201, u32::try_from(jpeg_at).unwrap()),
                    long(0x0202, u32::try_from(jpeg.len()).unwrap()),
                ],
            );
        } else {
            f = f.ifd(pos, 0, &[short(0x0100, 10), short(0x0101, 10)]);
        }
    }
    f = f.put(jpeg_at, &jpeg);
    let mut src = InMemoryRawHeaderSource::new(f.into_vec());
    let info = ArwContainerReader.read(&mut src, &CancelToken::none()).unwrap();
    assert_eq!(info.previews.len(), 1, "63rd sub IFD must be read: {:?}", info.previews);
}

/// Cancels the shared token on the read with index `at`.
struct CancelOnReadSource {
    inner: InMemoryRawHeaderSource,
    token: CancelToken,
    calls: usize,
    at: usize,
}

impl RawHeaderSource for CancelOnReadSource {
    fn length(&self) -> i64 {
        self.inner.length()
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let call = self.calls;
        self.calls += 1;
        if call == self.at {
            self.token.cancel();
        }
        self.inner.read(offset, count)
    }
}

// A last IFD with next=0 must not enqueue another IFD (C#: `if (nextIfdOffset > 0) Enqueue`), so a cancel during it is not observed.
// Kills the `>` -> `>=` mutant at arw.rs `if next_ifd > 0`.
#[test]
fn cancel_during_last_ifd_with_zero_next_is_not_observed() {
    let token = CancelToken::new();
    let mut src = CancelOnReadSource {
        inner: InMemoryRawHeaderSource::new(build_tiff(true, None, 1, 42)),
        token: token.clone(),
        calls: 0,
        at: 1,
    };
    let r = ArwContainerReader.read(&mut src, &token);
    assert!(r.is_ok(), "{r:?}");
}
