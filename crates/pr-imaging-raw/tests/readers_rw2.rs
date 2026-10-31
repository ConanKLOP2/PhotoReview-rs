//! Red tests for W5-08 (`readers/rw2.rs`): `Rw2ContainerReader` (C# baseline d92d6aae, `Tiff/Rw2ContainerReader.cs`).
//!
//! Ported from `Raw/Tiff/TiffFamilyReaderTests.cs` (`Rw2Reader_Synthetic_CanReadPreview`), `Raw/Tiff/TiffReaderRegressionTests.cs`
//! (the RW2 section and the `.rw2` row of `Readers_ArrayEntryCountAboveInt32Max_DoNotThrowUnexpectedExceptions`),
//! `Raw/Tiff/OrfRw2HeaderTests.cs` (RW2 rows), `Raw/Tiff/NefOrfRw2MutationGapTests.cs` (RW2 rows), `Raw/RawOrientationTests.cs`
//! (synthetic RW2 rows plus the RW2 part of the corpus tests), `Raw/Tiff/TiffCorpusTests.cs` (RW2 rows), `Robustness/RawContainerFuzzTests.cs`
//! (`Fuzz_Rw2ContainerReader_Robust`), `Robustness/BinaryReaderFuzzTests.cs` (RW2 target of `Fuzz_RawContainerReaders_FailOnlyCleanly`,
//! reader part only), and the goldens `tests/golden/rw2-container.json` (235 cases) and `tests/golden/rw2-corpus.json` (3 cases).
//!
//! Every test that reaches the reader fails with `todo!()` until the porter fills `readers/rw2.rs`. Corpus tests skip when
//! `tests/fixtures/raw-corpus` is absent (strict switch `PHOTOREVIEW_RS_RAW_CORPUS_STRICT=1` makes that a failure).
//! The C# `OrientedJpeg` (WPF encoder) is replaced by a hand-built EXIF APP1 (the reader only needs the EXIF).
//! Helper copies live in the `support` and `fuzz` modules below (no cross-unit visibility, Wave 4 lesson 15).
#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: a failed precondition should panic with its message.

#[path = "common/corpus_skip.rs"]
mod corpus_skip;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use pr_core::sync::CancelToken;
use pr_imaging::exif_parser::try_read_orientation_from_jpeg;
use pr_imaging_raw::formats::limits::{DEFAULT_EXIF_BLOCK_BYTES, MAX_HEADER_BYTES};
use pr_imaging_raw::formats::{RawContainerInfo, RawFormat};
use pr_imaging_raw::readers::rw2::{
    INVALID_HEADER_MESSAGE, Rw2ContainerReader, TOO_SHORT_MESSAGE, exif_block_length, exif_read_length,
    ifd0_orientation, image_extent,
};
use pr_imaging_raw::{InMemoryRawHeaderSource, RawContainerReader, RawHeaderError, RawHeaderSource};
use pr_test_support::golden;
use pr_test_support::raw::{
    Case, TiffBytes, XorShift64, at, build_tiff, create_minimal_jpeg, golden_bytes, long, mutants, short,
};
use serde_json::{Value, json};

// ---------------------------------------------------------------- shared helpers (C# `TiffBytes` / `SyntheticRawBuilder` / `JpegBytes` ports)

mod support {
    use super::*;

    /// C# `new TiffBytes(true, size).Header(8, "IIU\0"u8)`: the RW2 header with IFD0 at 8 (magic 0x0055 at offset 2).
    pub fn rw2_builder(size: usize) -> TiffBytes {
        TiffBytes::new(true, size).put(0, b"IIU\0").u32(4, 8)
    }

    /// Reads a whole byte vector through the in-memory source with the default 8 MiB header budget.
    pub fn read_rw2(data: Vec<u8>) -> Result<RawContainerInfo, RawHeaderError> {
        Rw2ContainerReader.read(&mut InMemoryRawHeaderSource::new(data), &CancelToken::new())
    }

    /// `Read` that must succeed (C# `reader.Read(...)` without an expected exception).
    pub fn read_ok(data: Vec<u8>) -> RawContainerInfo {
        read_rw2(data).expect("RW2 read should succeed")
    }

    /// The reader fails only with `InvalidData` (C# `Assert.Throws<InvalidDataException>`).
    pub fn assert_invalid_data(result: Result<RawContainerInfo, RawHeaderError>) {
        match result {
            Err(RawHeaderError::InvalidData { .. }) => {}
            Err(other) => panic!("expected InvalidData, got {other}"),
            Ok(info) => panic!("expected InvalidData, got Ok({info:?})"),
        }
    }

    /// Fuzz / robustness invariants on a result the reader accepted (C# `Assert.InRange` / `Assert.True` lines).
    pub fn assert_container_invariants(info: &RawContainerInfo, file_length: i64) {
        assert!(
            (1..=8).contains(&info.orientation),
            "orientation {} outside 1..=8",
            info.orientation
        );
        assert!(
            info.sensor_width >= 0 && info.sensor_height >= 0,
            "negative sensor size"
        );
        for preview in &info.previews {
            assert!(
                preview.offset >= 0 && preview.length > 0 && preview.length <= file_length - preview.offset,
                "preview {}+{} escapes the {file_length}-byte file",
                preview.offset,
                preview.length
            );
        }
    }

    /// Every failure of the reader is `InvalidData` (never `ArgumentOutOfRange`, never `Canceled`).
    pub fn assert_clean_failure(error: &RawHeaderError) {
        assert!(
            matches!(error, RawHeaderError::InvalidData { .. }),
            "reader failed with a non-InvalidData error: {error}"
        );
    }

    /// `ORF`-style TIFF block holding one SHORT Orientation (C# `JpegBytes.OrientationTiff`): header, IFD0 at 8, next pointer 0.
    pub fn orientation_tiff(little: bool, orientation: u16) -> Vec<u8> {
        let size = 8 + 2 + 12 + 4;
        let magic: &[u8] = if little { b"II" } else { b"MM" };
        TiffBytes::new(little, size)
            .put(0, magic)
            .u16(2, 42)
            .u32(4, 8)
            .ifd(8, 0, &[short(0x0112, u32::from(orientation))])
            .into_vec()
    }

    /// C# `OrientedJpeg` replacement: the minimal JPEG with an Exif APP1 carrying `orientation` (the reader only needs the EXIF).
    pub fn oriented_jpeg(orientation: u16) -> Vec<u8> {
        let minimal = create_minimal_jpeg(640, 480);
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(&orientation_tiff(true, orientation));
        let length = u16::try_from(payload.len() + 2).expect("APP1 fits u16");
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(&payload);
        out.extend_from_slice(&minimal[2..]);
        out
    }

    /// C# `SyntheticRawBuilder.BuildRw2(jpeg, ifd0Orientation)`: JpgFromRaw (type 4, count = length, offset 64), optional
    /// SHORT Orientation, next pointer 0, the JPEG at 64.
    pub fn build_rw2(jpeg: &[u8], ifd0_orientation: Option<u16>) -> Vec<u8> {
        let mut entries = vec![at(0x002E, 4, u32::try_from(jpeg.len()).expect("jpeg fits u32"), 64)];
        if let Some(orientation) = ifd0_orientation {
            entries.push(short(0x0112, u32::from(orientation)));
        }
        TiffBytes::new(true, 64 + jpeg.len())
            .put(0, b"II\x55\x00")
            .u32(4, 8)
            .ifd(8, 0, &entries)
            .put(64, jpeg)
            .into_vec()
    }

    /// Port of `RawOrientationPatcher` (TIFF IFD0 and embedded-JPEG EXIF parts; the CR3 and RAF paths are other units).
    pub mod patch {
        use super::*;

        fn u16_at(file: &[u8], at: usize, little: bool) -> Option<u16> {
            let raw = file.get(at..at + 2)?;
            let pair = [raw[0], raw[1]];
            Some(if little {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            })
        }

        fn u32_at(file: &[u8], at: usize, little: bool) -> Option<u32> {
            let raw = file.get(at..at + 4)?;
            let quad = [raw[0], raw[1], raw[2], raw[3]];
            Some(if little {
                u32::from_le_bytes(quad)
            } else {
                u32::from_be_bytes(quad)
            })
        }

        /// `TryFindIfd0Entry`: the byte offset of the first IFD0 entry with `tag`, and the byte order of the TIFF block.
        fn find_ifd0_entry(file: &[u8], tiff_start: usize, tag: u16) -> Option<(usize, bool)> {
            let little = *file.get(tiff_start)? == b'I';
            let ifd0 = tiff_start.checked_add(usize::try_from(u32_at(file, tiff_start + 4, little)?).ok()?)?;
            if ifd0 + 2 > file.len() {
                return None;
            }
            let count = u16_at(file, ifd0, little)?;
            for i in 0..usize::from(count) {
                let entry = ifd0 + 2 + i * 12;
                if entry + 12 > file.len() {
                    break;
                }
                if u16_at(file, entry, little)? == tag {
                    return Some((entry, little));
                }
            }
            None
        }

        /// `TryWriteTiffOrientation`: rewrites a SHORT Orientation value in IFD0 of the TIFF block at `tiff_start`.
        pub fn write_tiff_orientation(file: &mut [u8], tiff_start: usize, orientation: u16) -> bool {
            let Some((entry, little)) = find_ifd0_entry(file, tiff_start, 0x0112) else {
                return false;
            };
            if u16_at(file, entry + 2, little) != Some(3) {
                return false;
            }
            let bytes = if little {
                orientation.to_le_bytes()
            } else {
                orientation.to_be_bytes()
            };
            file[entry + 8..entry + 10].copy_from_slice(&bytes);
            true
        }

        /// `TryRenameIfd0Tag`: hides the IFD0 entry `tag` by rewriting its tag number to `replacement`.
        pub fn rename_ifd0_tag(file: &mut [u8], tiff_start: usize, tag: u16, replacement: u16) -> bool {
            let Some((entry, little)) = find_ifd0_entry(file, tiff_start, tag) else {
                return false;
            };
            let bytes = if little {
                replacement.to_le_bytes()
            } else {
                replacement.to_be_bytes()
            };
            file[entry..entry + 2].copy_from_slice(&bytes);
            true
        }

        /// `TryWriteOrientation` for RW2: IFD0 first, else the Exif APP1 of the embedded JPEG (C# `RawOrientationPatcher`).
        pub fn write_orientation(file: &mut [u8], extension: &str, info: &RawContainerInfo, orientation: u16) -> bool {
            if extension != ".raf" && write_tiff_orientation(file, 0, orientation) {
                return true;
            }
            let Some(preview) = info.previews.first() else {
                return false;
            };
            let Ok(jpeg_start) = usize::try_from(preview.offset) else {
                return false;
            };
            let mut offset = jpeg_start + 2;
            while offset + 4 <= file.len() && file[offset] == 0xFF {
                let marker = file[offset + 1];
                let length = (usize::from(file[offset + 2]) << 8) | usize::from(file[offset + 3]);
                if marker == 0xE1 && length >= 8 && file.get(offset + 4..offset + 10) == Some(b"Exif\0\0".as_slice()) {
                    return write_tiff_orientation(file, offset + 10, orientation);
                }
                if marker == 0xDA || marker == 0xD9 {
                    break;
                }
                offset += 2 + length;
            }
            false
        }
    }
}

use support::{
    assert_clean_failure, assert_container_invariants, assert_invalid_data, build_rw2, oriented_jpeg, patch, read_ok,
    read_rw2, rw2_builder,
};

/// C# `BaselineJpeg` (`SyntheticRawBuilder.CreateMinimalJpeg(640, 480)`).
fn baseline_jpeg() -> Vec<u8> {
    create_minimal_jpeg(640, 480)
}

/// C# `Jpeg(width, height, padding)`: the minimal JPEG followed by zero bytes.
fn jpeg_with_padding(width: i32, height: i32, padding: usize) -> Vec<u8> {
    let mut bytes = create_minimal_jpeg(width, height);
    bytes.extend(std::iter::repeat_n(0u8, padding));
    bytes
}

// ---------------------------------------------------------------- CanRead (NefOrfRw2MutationGapTests, RW2 rows)

// C#: NefOrfRw2MutationGapTests.CanRead_ExactlyFourSignatureBytes_IsAccepted("rw2")
#[test]
fn can_read_exactly_four_signature_bytes_is_accepted() {
    let head = [0x49u8, 0x49, 0x55, 0x00];
    let reader = Rw2ContainerReader;

    assert!(reader.can_read(&head, ".rw2"));
    assert!(!reader.can_read(&head[..3], ".rw2"));
}

// C#: NefOrfRw2MutationGapTests.CanRead_AnySingleSignatureByteWrong_IsRejected("rw2", 0..=3)
#[test]
fn can_read_any_single_signature_byte_wrong_is_rejected() {
    let reader = Rw2ContainerReader;
    for position in 0..4 {
        let mut head = [0x49u8, 0x49, 0x55, 0x00];
        head[position] ^= 0x40;

        assert!(
            !reader.can_read(&head, ".rw2"),
            "byte {position} wrong must be rejected"
        );
    }
}

// ---------------------------------------------------------------- short file / magic (OrfRw2HeaderTests, NefOrfRw2MutationGapTests)

// C#: OrfRw2HeaderTests.Rw2Read_JunkHeader_ThrowsInvalidData
#[test]
fn rw2_read_junk_header_throws_invalid_data() {
    assert_invalid_data(read_rw2(vec![0x5A; 64]));
}

// C#: OrfRw2HeaderTests.Rw2Read_WrongMagic_ThrowsInvalidData (2 rows: `MM` + `55 00`, `II` + `2A 00`)
#[test]
fn rw2_read_wrong_magic_throws_invalid_data() {
    let big_endian_mm = TiffBytes::new(false, 200)
        .put(0, b"MM\x55\x00")
        .u32(4, 8)
        .ifd(8, 0, &[])
        .into_vec();
    let plain_tiff_magic = TiffBytes::new(true, 200)
        .put(0, b"II\x2A\x00")
        .u32(4, 8)
        .ifd(8, 0, &[])
        .into_vec();

    assert_invalid_data(read_rw2(big_endian_mm));
    assert_invalid_data(read_rw2(plain_tiff_magic));
}

// C#: OrfRw2HeaderTests.Rw2Read_ValidIiuHeader_Parses
#[test]
fn rw2_read_valid_iiu_header_parses() {
    let data = TiffBytes::new(true, 200)
        .put(0, b"IIU\x00")
        .u32(4, 8)
        .ifd(8, 0, &[short(0x0002, 5000), short(0x0003, 3000)])
        .into_vec();

    let info = read_ok(data);

    assert_eq!((info.sensor_width, info.sensor_height), (5000, 3000));
}

// C#: NefOrfRw2MutationGapTests.Read_SixteenByteFileWithEmptyIfd_ReturnsEmptyInfoForNefOrfAndRw2 (RW2 third)
#[test]
fn read_sixteen_byte_file_with_empty_ifd_returns_empty_info_for_rw2() {
    let info = read_ok(rw2_builder(16).ifd(8, 0, &[]).into_vec());

    assert_eq!((info.sensor_width, info.sensor_height), (0, 0));
    assert_eq!(info.orientation, 1);
    assert!(info.previews.is_empty());
}

// ---------------------------------------------------------------- synthetic files (TiffFamilyReaderTests, TiffReaderRegressionTests)

// C#: TiffFamilyReaderTests.Rw2Reader_Synthetic_CanReadPreview (JPEG 320x240 at offset 32, count = JPEG length, type 4)
#[test]
fn rw2_reader_synthetic_can_read_preview() {
    let jpeg = create_minimal_jpeg(320, 240);
    let jpeg_pos: u32 = 32;
    let data = TiffBytes::new(true, jpeg_pos as usize + jpeg.len())
        .put(0, b"II\x55\x00")
        .u32(4, 8)
        .ifd(
            8,
            0,
            &[at(
                0x002E,
                4,
                u32::try_from(jpeg.len()).expect("jpeg fits u32"),
                jpeg_pos,
            )],
        )
        .put(jpeg_pos as usize, &jpeg)
        .into_vec();

    assert!(Rw2ContainerReader.can_read(&data[..data.len().min(64)], ".rw2"));
    let info = read_ok(data);

    assert_eq!(info.format, RawFormat::Rw2);
    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].offset, i64::from(jpeg_pos));
    assert_eq!(
        info.previews[0].length,
        i64::try_from(jpeg.len()).expect("length fits i64")
    );
}

/// C# `BuildRw2(jpegCount, jpegOffset, fileSize, extra...)`: JpgFromRaw type 7 at the given count/offset, the baseline JPEG at 300.
fn build_rw2_with_entries(
    jpeg_count: u32,
    jpeg_offset: u32,
    file_size: usize,
    extra: &[pr_test_support::raw::Entry],
) -> Vec<u8> {
    let mut entries = vec![at(0x002E, 7, jpeg_count, jpeg_offset)];
    entries.extend_from_slice(extra);
    TiffBytes::new(true, file_size)
        .put(0, b"IIU\x00")
        .u32(4, 8)
        .ifd(8, 0, &entries)
        .put(300, &baseline_jpeg())
        .into_vec()
}

// C#: TiffReaderRegressionTests.Rw2Reader_JpegLengthBeyondEndOfFile_IsNotListed
#[test]
fn rw2_reader_jpeg_length_beyond_end_of_file_is_not_listed() {
    let file_size = 300 + baseline_jpeg().len();
    let data = build_rw2_with_entries(5_000_000, 300, file_size, &[]);

    assert!(read_ok(data).previews.is_empty());
}

// C#: TiffReaderRegressionTests.Rw2Reader_JpegOffsetWithinFourBytesOfEnd_IsIgnoredWithoutThrowing (1, 2, 3)
#[test]
fn rw2_reader_jpeg_offset_within_four_bytes_of_end_is_ignored_without_throwing() {
    const SIZE: usize = 512;
    for bytes_before_end in 1..=3usize {
        let offset = u32::try_from(SIZE - bytes_before_end).expect("offset fits u32");
        let data = build_rw2_with_entries(100, offset, SIZE, &[]);

        let info = read_ok(data);

        assert!(
            info.previews.is_empty(),
            "{bytes_before_end} bytes before the end must list no preview"
        );
    }
}

// C#: TiffReaderRegressionTests.Rw2Reader_SensorBordersPresent_ImageSizeExcludesMargins (5184x3888)
#[test]
fn rw2_reader_sensor_borders_present_image_size_excludes_margins() {
    let jpeg_length = u32::try_from(baseline_jpeg().len()).expect("jpeg fits u32");
    let file_size = 300 + baseline_jpeg().len();
    let data = build_rw2_with_entries(
        jpeg_length,
        300,
        file_size,
        &[
            short(0x0002, 5264),
            short(0x0003, 3904),
            short(0x0004, 8),
            short(0x0005, 12),
            short(0x0006, 3896),
            short(0x0007, 5196),
        ],
    );

    let info = read_ok(data);

    assert_eq!((info.sensor_width, info.sensor_height), (5184, 3888));
}

// C#: TiffReaderRegressionTests.Rw2Reader_NoBorderTags_FallsBackToSensorWidthAndHeightTags (4060x3016)
#[test]
fn rw2_reader_no_border_tags_falls_back_to_sensor_width_and_height_tags() {
    let jpeg_length = u32::try_from(baseline_jpeg().len()).expect("jpeg fits u32");
    let file_size = 300 + baseline_jpeg().len();
    let data = build_rw2_with_entries(jpeg_length, 300, file_size, &[short(0x0002, 4060), short(0x0003, 3016)]);

    let info = read_ok(data);

    assert_eq!((info.sensor_width, info.sensor_height), (4060, 3016));
}

// C#: TiffReaderRegressionTests.Readers_ArrayEntryCountAboveInt32Max_DoNotThrowUnexpectedExceptions (the `.rw2` row)
#[test]
fn readers_array_entry_count_above_int32_max_do_not_throw_unexpected_exceptions() {
    let mut data = TiffBytes::new(true, 512)
        .header(16)
        .put(8, b"CR\x02\x00\x00\x00\x00\x00")
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
    data[2] = 0x55; // RW2 magic
    let file_length = i64::try_from(data.len()).expect("length fits i64");

    match read_rw2(data) {
        Ok(info) => assert_container_invariants(&info, file_length),
        Err(error) => assert_clean_failure(&error),
    }
}

// ---------------------------------------------------------------- mutation gaps (NefOrfRw2MutationGapTests, RW2 rows)

// C#: NefOrfRw2MutationGapTests.ReadRw2_EqualBorders_FallBackToSensorSizeTags
#[test]
fn read_rw2_equal_borders_fall_back_to_sensor_size_tags() {
    let data = rw2_builder(256)
        .ifd(
            8,
            0,
            &[
                long(0x0002, 6000),
                long(0x0003, 4000),
                long(0x0004, 50),
                long(0x0005, 100),
                long(0x0006, 50),
                long(0x0007, 100),
            ],
        )
        .into_vec();

    let info = read_ok(data);

    assert_eq!((info.sensor_width, info.sensor_height), (6000, 4000));
}

// C#: NefOrfRw2MutationGapTests.ReadRw2_BordersOfIntMax_AreAccepted
#[test]
fn read_rw2_borders_of_int_max_are_accepted() {
    let data = rw2_builder(256)
        .ifd(
            8,
            0,
            &[
                long(0x0004, 0),
                long(0x0005, 0),
                long(0x0006, i32::MAX as u32),
                long(0x0007, i32::MAX as u32),
            ],
        )
        .into_vec();

    let info = read_ok(data);

    assert_eq!((info.sensor_width, info.sensor_height), (i32::MAX, i32::MAX));
}

// C#: NefOrfRw2MutationGapTests.ReadRw2_BordersCropTheSensorSizeTags
#[test]
fn read_rw2_borders_crop_the_sensor_size_tags() {
    let data = rw2_builder(256)
        .ifd(
            8,
            0,
            &[
                long(0x0002, 6024),
                long(0x0003, 4016),
                long(0x0004, 8),
                long(0x0005, 24),
                long(0x0006, 4008),
                long(0x0007, 6024),
            ],
        )
        .into_vec();

    let info = read_ok(data);

    assert_eq!((info.sensor_width, info.sensor_height), (6000, 4000));
}

// C#: NefOrfRw2MutationGapTests.ReadRw2_SensorSizeTagOfIntMax_IsAcceptedAndAboveIntMaxIsRejected
#[test]
fn read_rw2_sensor_size_tag_of_int_max_is_accepted_and_above_int_max_is_rejected() {
    let ok = rw2_builder(128)
        .ifd(8, 0, &[long(0x0002, i32::MAX as u32), long(0x0003, i32::MAX as u32)])
        .into_vec();
    let too_big = rw2_builder(128)
        .ifd(8, 0, &[long(0x0002, 0xFFFF_FFFF), long(0x0003, 0x8000_0000)])
        .into_vec();

    let accepted = read_ok(ok);
    let rejected = read_ok(too_big);

    assert_eq!((accepted.sensor_width, accepted.sensor_height), (i32::MAX, i32::MAX));
    assert_eq!((rejected.sensor_width, rejected.sensor_height), (0, 0));
}

// C#: NefOrfRw2MutationGapTests.ReadRw2_JpgFromRaw_NeedsMoreThanFourBytes (4u => 0, 5u => 1)
#[test]
fn read_rw2_jpg_from_raw_needs_more_than_four_bytes() {
    for (length, expected_previews) in [(4u32, 0usize), (5, 1)] {
        let data = rw2_builder(128)
            .ifd(8, 0, &[at(0x002E, 4, length, 64)])
            .put(64, &[0xFF, 0xD8, 0xFF, 0xD9, 0x00])
            .into_vec();

        let info = read_ok(data);

        assert_eq!(info.previews.len(), expected_previews, "JpgFromRaw count {length}");
    }
}

// C#: NefOrfRw2MutationGapTests.ReadRw2_ExifBlockCoversThePreviewUpToOneHundredTwentyEightKiB (1000 => 1000, 200000 => 131072)
#[test]
fn read_rw2_exif_block_covers_the_preview_up_to_one_hundred_twenty_eight_kib() {
    for (jpeg_length, expected_block_length) in [(1000usize, 1000i64), (200_000, 128 * 1024)] {
        let base = create_minimal_jpeg(640, 480).len();
        let jpeg = jpeg_with_padding(640, 480, jpeg_length - base);
        let data = rw2_builder(64 + jpeg.len())
            .ifd(
                8,
                0,
                &[at(0x002E, 4, u32::try_from(jpeg.len()).expect("jpeg fits u32"), 64)],
            )
            .put(64, &jpeg)
            .into_vec();

        let info = read_ok(data);

        assert_eq!(
            info.exif_blocks.len(),
            1,
            "one EXIF block for a {jpeg_length}-byte preview"
        );
        let block = info.exif_blocks[0];
        assert_eq!(
            (block.offset, block.length, block.is_tiff_header),
            (64, expected_block_length, false)
        );
    }
}

// ---------------------------------------------------------------- orientation (RawOrientationTests, synthetic rows)

// C#: RawOrientationTests.Rw2Reader_Ifd0Orientation_IsReported (3, 6, 8)
#[test]
fn rw2_reader_ifd0_orientation_is_reported() {
    for orientation in [3u16, 6, 8] {
        let rw2 = build_rw2(&create_minimal_jpeg(640, 480), Some(orientation));

        assert_eq!(read_ok(rw2).orientation, i32::from(orientation));
    }
}

// C#: RawOrientationTests.Rw2Reader_NoIfd0Orientation_FallsBackToEmbeddedJpegExif (6, 8)
#[test]
fn rw2_reader_no_ifd0_orientation_falls_back_to_embedded_jpeg_exif() {
    for orientation in [6u16, 8] {
        let rw2 = build_rw2(&oriented_jpeg(orientation), None);

        assert_eq!(read_ok(rw2).orientation, i32::from(orientation));
    }
}

// C#: RawOrientationTests.Rw2Reader_Ifd0OrientationWinsOverEmbeddedJpegExif
#[test]
fn rw2_reader_ifd0_orientation_wins_over_embedded_jpeg_exif() {
    let rw2 = build_rw2(&oriented_jpeg(8), Some(6));

    assert_eq!(read_ok(rw2).orientation, 6);
}

// ---------------------------------------------------------------- golden: rw2-container (235 cases)

/// Ported `InMemoryRawHeaderSource` of the oracle: 64 KiB block accounting, settable budget (golden `budget`), a read that runs
/// out of budget part-way keeps the blocks it already charged. Offset/count are checked first (past end, then the hard limit).
struct MemSource {
    bytes: Vec<u8>,
    budget: i64,
    charged: i64,
    touched: HashSet<i64>,
}

const BLOCK: i64 = 64 * 1024;

impl MemSource {
    fn new(bytes: Vec<u8>, budget: Option<i64>) -> Self {
        Self {
            bytes,
            budget: budget.unwrap_or(i64::from(MAX_HEADER_BYTES)),
            charged: 0,
            touched: HashSet::new(),
        }
    }

    fn bytes_read(&self) -> i64 {
        self.charged
    }
}

impl RawHeaderSource for MemSource {
    fn length(&self) -> i64 {
        i64::try_from(self.bytes.len()).expect("length fits i64")
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        if offset < 0 || count < 0 {
            return Err(RawHeaderError::ArgumentOutOfRange {
                param: "offset",
                message: "Offset and count must be non-negative.".to_owned(),
            });
        }
        if offset > self.length() - i64::from(count) {
            return Err(RawHeaderError::invalid_data("past end"));
        }
        if count > MAX_HEADER_BYTES {
            return Err(RawHeaderError::invalid_data("over the hard limit"));
        }
        if count == 0 {
            return Ok(&[]);
        }
        let start_block = offset / BLOCK;
        let end_block = (offset + i64::from(count) - 1) / BLOCK;
        for block in start_block..=end_block {
            if self.touched.contains(&block) {
                continue;
            }
            let block_bytes = (self.length() - block * BLOCK).min(BLOCK);
            if block_bytes > self.budget - self.charged {
                return Err(RawHeaderError::invalid_data("RAW header read exceeded its budget."));
            }
            self.touched.insert(block);
            self.charged += block_bytes;
        }
        let start = usize::try_from(offset).expect("offset checked");
        let end = start + usize::try_from(count).expect("count checked");
        Ok(&self.bytes[start..end])
    }
}

/// C# `FuzzTrackingHeaderSource`: out-of-range or empty requests return an empty slice; the budget counts bytes handed out.
struct TrackingSource {
    bytes: Vec<u8>,
    total: i64,
}

impl RawHeaderSource for TrackingSource {
    fn length(&self) -> i64 {
        i64::try_from(self.bytes.len()).expect("length fits i64")
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let length = self.length();
        if offset < 0 || offset >= length || count <= 0 {
            return Ok(&[]);
        }
        let available = i64::from(count).min(length - offset);
        self.total += available;
        if self.total > i64::from(MAX_HEADER_BYTES) {
            return Err(RawHeaderError::invalid_data("Exceeded maximum header read budget."));
        }
        let start = usize::try_from(offset).expect("offset checked");
        let end = start + usize::try_from(available).expect("available fits usize");
        Ok(&self.bytes[start..end])
    }
}

/// Golden `error` tag: the two reader messages by text, every other invalid-data error is a source error, anything else is a bug.
fn error_tag(error: &RawHeaderError) -> String {
    match error {
        RawHeaderError::InvalidData { message, .. } if message == TOO_SHORT_MESSAGE => "too-short".to_owned(),
        RawHeaderError::InvalidData { message, .. } if message == INVALID_HEADER_MESSAGE => "invalid-header".to_owned(),
        RawHeaderError::InvalidData { .. } => "source".to_owned(),
        other => format!("unexpected:{other:?}"),
    }
}

fn preview_json(preview: &pr_imaging_raw::EmbeddedPreview) -> Value {
    json!({
        "index": preview.index,
        "offset": preview.offset,
        "length": preview.length,
        "kind": format!("{:?}", preview.kind),
        "width": preview.width,
        "height": preview.height,
        "color_space": format!("{:?}", preview.color_space),
        "header_resolved": preview.header_resolved,
    })
}

fn info_json(info: &RawContainerInfo) -> Value {
    json!({
        "format": format!("{:?}", info.format),
        "sensor_width": info.sensor_width,
        "sensor_height": info.sensor_height,
        "orientation": info.orientation,
        "previews": info.previews.iter().map(preview_json).collect::<Vec<_>>(),
        "exif_blocks": info.exif_blocks.iter().map(|block| json!({
            "offset": block.offset,
            "length": block.length,
            "is_tiff_header": block.is_tiff_header,
            "ifd_is_exif": block.ifd_is_exif,
        })).collect::<Vec<_>>(),
    })
}

/// Runs one golden `read` case; the result has the golden keys `ok`, `error`, `info`, `bytes_read`.
fn golden_read(case: &Value) -> Value {
    let bytes = golden_bytes(&case["data"]);
    let budget = case["budget"].as_i64();
    let mut source = MemSource::new(bytes, budget);
    let result = Rw2ContainerReader.read(&mut source, &CancelToken::new());
    let (ok, error, info) = match result {
        Ok(info) => (true, Value::Null, info_json(&info)),
        Err(error) => (false, json!(error_tag(&error)), Value::Null),
    };
    json!({ "ok": ok, "error": error, "info": info, "bytes_read": source.bytes_read() })
}

/// Runs one golden `can-read` case: the first 64 bytes and the extension (C# `CanRead(data[..64], ext)`).
fn golden_can_read(case: &Value) -> Value {
    let bytes = golden_bytes(&case["data"]);
    let head = &bytes[..bytes.len().min(64)];
    let extension = golden::str_of(case, "ext");
    json!({ "can_read": Rw2ContainerReader.can_read(head, extension) })
}

// C#: tests/golden/rw2-container.json (oracle W5-08.cs `rw2-container`): 235 cases (194 `read`, 41 `can-read`), every field and `error`.
#[test]
fn golden_rw2_container_matches_csharp_for_every_case() {
    let golden_file = golden::load("rw2-container");
    assert_eq!(golden_file.cases.len(), 235, "rw2-container golden case count");

    let mut mismatches = Vec::new();
    for case in &golden_file.cases {
        let name = golden::str_of(case, "name");
        let actual = match golden::str_of(case, "op") {
            "read" => golden_read(case),
            "can-read" => golden_can_read(case),
            other => panic!("[{name}] unknown golden op {other}"),
        };

        let mut expected = case.as_object().expect("golden case is an object").clone();
        for key in ["name", "op", "data", "budget", "ext"] {
            expected.remove(key);
        }
        if Value::Object(expected.clone()) != actual {
            mismatches.push(format!("[{name}] expected {}, got {actual}", Value::Object(expected)));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} rw2-container golden cases differ from C#:\n{}",
        mismatches.len(),
        golden_file.cases.len(),
        mismatches.join("\n")
    );
}

// ---------------------------------------------------------------- pure helpers: one test per boundary (contract mutation list)

// C#: the extent rule of Rw2ContainerReader.Read (width from 4..7 borders, height from the same pattern), equal/reversed/off-by-one
// borders, `i32::MAX`, negative start, zero and over-range size tags.
#[test]
fn image_extent_boundaries_match_the_csharp_extent_rule() {
    let max = i64::from(i32::MAX);
    // (start, end, size tag, expected extent)
    type ExtentCase = (Option<i64>, Option<i64>, Option<i64>, i32);
    let cases: [ExtentCase; 16] = [
        (Some(0), Some(100), None, 100),        // plain borders
        (Some(50), Some(50), Some(777), 777),   // equal borders: tag fallback
        (Some(100), Some(50), Some(777), 777),  // reversed borders: tag fallback
        (Some(100), Some(50), None, 0),         // reversed without tag: 0
        (Some(100), Some(101), None, 1),        // differ by one
        (Some(0), Some(max), None, i32::MAX),   // end == i32::MAX accepted
        (Some(0), Some(max + 1), Some(42), 42), // end above i32::MAX: tag fallback
        (Some(0), Some(max + 1), None, 0),      // ... and 0 without a tag
        (Some(-1), Some(100), Some(42), 42),    // negative start: tag fallback
        (Some(0), Some(100), Some(42), 100),    // start == 0 is accepted
        (None, None, Some(1), 1),               // tag 1 is the smallest accepted size
        (None, None, Some(0), 0),               // tag 0 is not a size
        (None, None, Some(max), i32::MAX),      // tag i32::MAX accepted
        (None, None, Some(max + 1), 0),         // tag above i32::MAX rejected
        (None, None, Some(-5), 0),              // negative size tag rejected
        (None, None, None, 0),                  // no borders, no tag
    ];
    for (start, end, tag, expected) in cases {
        assert_eq!(
            image_extent(start, end, tag),
            expected,
            "image_extent({start:?}, {end:?}, {tag:?})"
        );
    }
}

// C#: the orientation range 1..=8 of Rw2ContainerReader (0, 1, 8, 9, negative, out of range).
#[test]
fn ifd0_orientation_accepts_only_one_to_eight() {
    let cases: [(Option<i64>, Option<i32>); 8] = [
        (None, None),
        (Some(0), None),
        (Some(1), Some(1)),
        (Some(6), Some(6)),
        (Some(8), Some(8)),
        (Some(9), None),
        (Some(255), None),
        (Some(-1), None),
    ];
    for (value, expected) in cases {
        assert_eq!(ifd0_orientation(value), expected, "ifd0_orientation({value:?})");
    }
}

// C#: DEFAULT_EXIF_BLOCK_BYTES = 128 KiB (131 071, 131 072, 131 073).
#[test]
fn exif_block_length_caps_at_the_default_block_size() {
    assert_eq!(i64::from(DEFAULT_EXIF_BLOCK_BYTES), 128 * 1024);
    assert_eq!(exif_block_length(131_071), 131_071);
    assert_eq!(exif_block_length(131_072), 131_072);
    assert_eq!(exif_block_length(131_073), 131_072);
    assert_eq!(exif_block_length(1000), 1000);
}

// C#: the orientation fallback window `min(min(length, 128 KiB), file_length - offset)`; `<= 0` means do not read.
#[test]
fn exif_read_length_is_bounded_by_block_and_file_end() {
    assert_eq!(exif_read_length(200_000, 100, 1000), 900);
    assert_eq!(exif_read_length(200_000, 0, 1_000_000), 131_072);
    assert_eq!(exif_read_length(100, 50, 120), 70);
    assert!(exif_read_length(100, 120, 120) <= 0, "no bytes left: do not read");
    assert!(
        exif_read_length(100, 130, 120) <= 0,
        "offset past the file: do not read"
    );
}

// ---------------------------------------------------------------- corpus (skip when tests/fixtures/raw-corpus is absent)

/// The corpus directory, or `None` (skip, like C# `RawCorpus.RequireDirectory`). Strict mode turns the absence into a failure.
fn corpus_dir() -> Option<PathBuf> {
    let dir = golden::repo_root().join("tests").join("fixtures").join("raw-corpus");
    if dir.is_dir() {
        return Some(dir);
    }
    let strict = std::env::var("PHOTOREVIEW_RS_RAW_CORPUS_STRICT").is_ok_and(|value| value == "1");
    assert!(
        !strict,
        "PHOTOREVIEW_RS_RAW_CORPUS_STRICT=1 but {} is missing",
        dir.display()
    );
    corpus_skip::skipped(&format!("no corpus at {}", dir.display()));
    None
}

/// Every `.rw2` file of the corpus, sorted by name (C# `RawCorpus.Files(".rw2")`).
fn corpus_rw2_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("corpus dir readable")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("rw2")))
        .collect();
    files.sort();
    files
}

/// The first `.rw2` whose file name contains `model` (C# `RawCorpus.TryGetFirst("*.rw2", model)`).
fn corpus_rw2_for_model(dir: &Path, model: &str) -> Option<PathBuf> {
    corpus_rw2_files(dir).into_iter().find(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().contains(&model.to_ascii_lowercase()))
    })
}

fn read_corpus_file(path: &Path) -> (Vec<u8>, RawContainerInfo) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let info = read_rw2(bytes.clone()).unwrap_or_else(|e| panic!("{}: RW2 read failed: {e}", path.display()));
    (bytes, info)
}

// C#: TiffCorpusTests.Rw2Corpus_SensorSizeExcludesMaskedBorders (GH5 5184x3888, DC-S1 6000x4000, GF1 4000x3000)
#[test]
fn rw2_corpus_sensor_size_excludes_masked_borders() {
    let Some(dir) = corpus_dir() else { return };
    for (model, width, height) in [("GH5", 5184, 3888), ("DC-S1", 6000, 4000), ("GF1", 4000, 3000)] {
        let Some(path) = corpus_rw2_for_model(&dir, model) else {
            continue;
        };

        let (_, info) = read_corpus_file(&path);

        assert_eq!((info.sensor_width, info.sensor_height), (width, height), "{model}");
    }
}

// C#: TiffCorpusTests.AllTiffCorpusFiles_EveryListedPreviewLiesInsideFileAndStartsWithSoi (RW2 slice)
#[test]
fn all_rw2_corpus_files_every_listed_preview_lies_inside_file_and_starts_with_soi() {
    let Some(dir) = corpus_dir() else { return };
    for path in corpus_rw2_files(&dir) {
        let (bytes, info) = read_corpus_file(&path);
        for preview in &info.previews {
            let start = usize::try_from(preview.offset).expect("offset fits usize");
            let length = usize::try_from(preview.length).expect("length fits usize");
            let head = bytes.get(start..start + length.min(2));
            assert_eq!(
                head,
                Some(&[0xFF, 0xD8][..]),
                "{}: preview at {} does not start with SOI",
                path.display(),
                preview.offset
            );
        }
    }
}

// C#: RawOrientationTests.RafAndRw2CorpusFile_ContainerOrientationMatchesEmbeddedJpegExif (RW2 part: no WIC oracle here, so the
// embedded JPEG EXIF reader is the oracle: `orientation == try_read_orientation_from_jpeg(preview).unwrap_or(1)`)
#[test]
fn rw2_corpus_file_container_orientation_matches_embedded_jpeg_exif() {
    let Some(dir) = corpus_dir() else { return };
    for path in corpus_rw2_files(&dir) {
        let (bytes, info) = read_corpus_file(&path);
        let preview = info
            .previews
            .first()
            .unwrap_or_else(|| panic!("{}: no preview", path.display()));
        let start = usize::try_from(preview.offset).expect("offset fits usize");
        let end = start + usize::try_from(preview.length).expect("length fits usize");
        let expected = try_read_orientation_from_jpeg(&bytes[start..end]).unwrap_or(1);

        assert_eq!(info.orientation, i32::from(expected), "{}", path.display());
    }
}

// C#: RawOrientationTests.RafAndRw2CorpusFile_RewrittenOrientation_IsReportedByContainerReader (RW2 part: IFD0 6/8/3)
#[test]
fn rw2_corpus_file_rewritten_orientation_is_reported_by_container_reader() {
    let Some(dir) = corpus_dir() else { return };
    for path in corpus_rw2_files(&dir) {
        let (original, original_info) = read_corpus_file(&path);
        for orientation in [6u16, 8, 3] {
            let mut rewritten = original.clone();
            assert!(
                patch::write_orientation(&mut rewritten, ".rw2", &original_info, orientation),
                "{}: no orientation to rewrite",
                path.display()
            );

            assert_eq!(
                read_ok(rewritten).orientation,
                i32::from(orientation),
                "{}",
                path.display()
            );
        }
    }
}

// C#: RawOrientationTests.Rw2CorpusFile_WithoutIfd0Orientation_FallsBackToEmbeddedJpegExif
#[test]
fn rw2_corpus_file_without_ifd0_orientation_falls_back_to_embedded_jpeg_exif() {
    let Some(dir) = corpus_dir() else { return };
    for path in corpus_rw2_files(&dir) {
        let (original, original_info) = read_corpus_file(&path);
        let mut file = original.clone();
        assert!(
            patch::rename_ifd0_tag(&mut file, 0, 0x0112, 0xFFF0),
            "{}: expected an IFD0 orientation tag to hide",
            path.display()
        );
        assert!(
            patch::write_orientation(&mut file, ".rw2", &original_info, 6),
            "{}: no embedded JPEG EXIF orientation to rewrite",
            path.display()
        );

        assert_eq!(read_ok(file).orientation, 6, "{}", path.display());
    }
}

// C#: tests/golden/rw2-corpus.json (oracle W5-08.cs `rw2-corpus`): 3 cases, `present:false` on a machine without the corpus.
// A present file must read as RW2 with a positive sensor size; a golden `present:true` without the local file is a failure.
#[test]
fn golden_rw2_corpus_cases_match_the_local_corpus() {
    let golden_file = golden::load("rw2-corpus");
    assert_eq!(golden_file.cases.len(), 3, "rw2-corpus golden case count");
    let dir = corpus_dir();

    for case in &golden_file.cases {
        let name = golden::str_of(case, "name");
        let file = golden::str_of(case, "file");
        let present = golden::bool_of(case, "present");
        let local = dir.as_ref().map(|d| d.join(file)).filter(|p| p.is_file());
        match local {
            Some(path) => {
                let (_, info) = read_corpus_file(&path);
                assert_eq!(info.format, RawFormat::Rw2, "[{name}]");
                assert!(
                    info.sensor_width > 0 && info.sensor_height > 0,
                    "[{name}] sensor size must be positive"
                );
            }
            None => assert!(
                !present,
                "[{name}] golden says present but {file} is missing from the corpus"
            ),
        }
    }
}

// ---------------------------------------------------------------- fuzz (RawContainerFuzzTests, BinaryReaderFuzzTests, RW2 rows)

mod fuzz {
    use super::*;

    fn random_byte(rng: &mut XorShift64) -> u8 {
        u8::try_from(rng.below(256)).expect("byte")
    }

    /// Port of `JpegBytes.Mutate` (1..3 structural or bit-level mutations; never returns the input instance).
    pub fn mutate(rng: &mut XorShift64, input: &[u8]) -> Vec<u8> {
        let mut data = input.to_vec();
        let rounds = 1 + rng.below(3);
        for _ in 0..rounds {
            if data.is_empty() {
                data.push(random_byte(rng));
                continue;
            }
            match rng.below(9) {
                0 => {
                    let p = rng.below(data.len());
                    data[p] = random_byte(rng);
                }
                1 => {
                    let p = rng.below(data.len());
                    data[p] ^= 1u8 << rng.below(8);
                }
                2 => {
                    let pos = rng.below(data.len());
                    let n = (1 + rng.below(8)).min(data.len() - pos);
                    data.drain(pos..pos + n);
                }
                3 => {
                    let at = rng.below(data.len() + 1);
                    let n = 1 + rng.below(8);
                    let extra: Vec<u8> = (0..n).map(|_| random_byte(rng)).collect();
                    data.splice(at..at, extra);
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
                    let at = rng.below(data.len() + 1);
                    data.splice(at..at, slice);
                }
                _ => {
                    let pos = rng.below(data.len());
                    let extreme: [u8; 2] = match rng.below(3) {
                        0 => [0xFF, 0xFF],
                        1 => [0x7F, 0xFF],
                        _ => [0x00, 0x00],
                    };
                    for (i, byte) in extreme.iter().enumerate() {
                        if let Some(slot) = data.get_mut(pos + i) {
                            *slot = *byte;
                        }
                    }
                }
            }
        }
        data
    }
}

// C#: RawContainerFuzzTests.Fuzz_Rw2ContainerReader_Robust (seed 106, BuildTiff(little, orientation 1, magic 0x55)).
// Iterations 2 000 (C# 10 000) so the debug build stays fast; the seed is fixed; no wall-clock assert (AGENTS.md).
#[test]
fn fuzz_rw2_container_reader_robust() {
    const ITERATIONS: usize = 2_000;
    let seed = build_tiff(true, None, 1, 0x0055);
    let mut rng = XorShift64::new(106);

    for iteration in 0..ITERATIONS {
        let mutated = fuzz::mutate(&mut rng, &seed);
        let mut source = TrackingSource {
            bytes: mutated.clone(),
            total: 0,
        };
        let length = i64::try_from(mutated.len()).expect("length fits i64");
        if Rw2ContainerReader.can_read(&mutated[..mutated.len().min(64)], ".rw2") {
            match Rw2ContainerReader.read(&mut source, &CancelToken::new()) {
                Ok(info) => assert_container_invariants(&info, length),
                Err(error) => assert_clean_failure(&error),
            }
        }
        assert!(
            source.total <= i64::from(MAX_HEADER_BYTES),
            "iteration {iteration}: {} header bytes read exceed MAX_HEADER_BYTES",
            source.total
        );
    }
}

// C#: BinaryReaderFuzzTests.Fuzz_RawContainerReaders_FailOnlyCleanly, the RW2 target (index 6 => rng seed 4406; seed
// BuildRw2(Jpeg(64, 48), 6); `Mutants` default 1 500 random cases). Reader part only: RawExif / PreviewSelector are W5-11b/c.
#[test]
fn fuzz_rw2_container_reader_fails_only_cleanly() {
    let seed = build_rw2(&create_minimal_jpeg(64, 48), Some(6));
    let cases: Vec<Case> = mutants(&seed, 4406, 1_500);
    let mut reached = 0usize;

    for case in &cases {
        let bytes = &case.bytes;
        if !Rw2ContainerReader.can_read(&bytes[..bytes.len().min(64)], ".rw2") {
            continue;
        }
        reached += 1;
        let length = i64::try_from(bytes.len()).expect("length fits i64");
        match read_rw2(bytes.clone()) {
            Ok(info) => assert_container_invariants(&info, length),
            Err(error) => assert_clean_failure(&error),
        }
    }
    assert!(
        reached > 0,
        "no mutant reached the RW2 reader; the seed is not exercising it"
    );
}
