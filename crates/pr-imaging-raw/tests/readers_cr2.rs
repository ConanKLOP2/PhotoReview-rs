//! Red tests for W5-03 (`readers/cr2.rs`): `Cr2ContainerReader` (C# baseline d92d6aae, `Cr2ContainerReader.cs`).
//!
//! Ported (CR2 rows only, the other readers' rows belong to their own units):
//! - `tests/PhotoReview.Imaging.Tests/Raw/Tiff/Cr2ArwRafMutationGapTests.cs` (CR2 tests; the ARW half is W5-07)
//! - `Raw/Tiff/TiffReaderRegressionTests.cs` (`Cr2Reader_*`, `Readers_ArrayEntryCountAboveInt32Max_*` `.cr2` row)
//! - `Raw/Tiff/TiffFamilyReaderTests.cs` (`Cr2Reader_Synthetic_CanReadAndParsePreview`)
//! - `Raw/Tiff/TiffStripPreviewTests.cs` (`cr2` row + `Read_Cr2StripLengthAboveIntMaxValue...`)
//! - `Raw/Tiff/TiffPreviewHardeningTests.cs` (`cr2` rows)
//! - `Raw/Tiff/TiffExifBlockPlacementTests.cs` (`cr2` rows, block assertions only; the `RawExif` line is W5-11)
//! - `Raw/RawErrorHandlingReviewTests.cs` (`Cr2_ManyIfdsWithExifPointer_YieldsOneExifBlock`)
//! - `Robustness/RawContainerFuzzTests.cs` (`Fuzz_Cr2ContainerReader_Robust`, without the p95 assert)
//! - `Robustness/BinaryReaderFuzzTests.cs` (the CR2 target of `Fuzz_RawContainerReaders_FailOnlyCleanly`)
//! - `Raw/Tiff/TiffCorpusTests.cs` (`cr2` rows; corpus absent here, so these skip by early return)
//! - Golden: `tests/golden/cr2-container.json` (153 cases) and `tests/golden/cr2-container-corpus.json` (0 cases).
//!
//! Stub bodies are `todo!()`: every test that reaches `can_read` or `read` fails until the porter fills it in.
//!
//! The golden spec interpreter (`file` objects of the golden, port of `W503Build` in `tools/oracle-cs/Commands/W5-03.cs`)
//! lives in this file. Shared byte builders come from `pr_test_support::raw` (D48).
//!
//! DIVERGENCE: `Fuzz_Cr2ContainerReader_Robust` asserts a p95 parse time in C#; AGENTS.md forbids timing asserts, so the
//! port keeps the 10 000-iteration loop, the header budget assert and the clean-failure assert (contract Open question 5).
//! DIVERGENCE: the fuzz mutator uses [`XorShift64`] instead of `System.Random` (same families, not the same bytes).
//! DIVERGENCE: corpus tests read whole files into `InMemoryRawHeaderSource`, not `SourceRawHeaderSource` over a file
//! stream, so they do not assert `bytes_read` (the golden corpus has zero cases here).

#[path = "common/corpus_skip.rs"]
mod corpus_skip;

use std::path::{Path, PathBuf};

use pr_core::session::store::sha256;
use pr_core::sync::CancelToken;
use pr_imaging_raw::formats::limits::{DEFAULT_EXIF_BLOCK_BYTES, MAX_EXIF_BLOCK_BYTES, MAX_HEADER_BYTES};
use pr_imaging_raw::readers::cr2::Cr2ContainerReader;
use pr_imaging_raw::{
    InMemoryRawHeaderSource, RawContainerInfo, RawContainerReader, RawFormat, RawHeaderError, RawHeaderSource,
};
use pr_test_support::golden;
use pr_test_support::raw::{
    TiffBytes, XorShift64, at, build_tiff, create_minimal_jpeg, from_hex, golden_bytes, long, short,
};
use serde_json::{Value, json};

// ---------------------------------------------------------------- golden spec interpreter (W503Build)

/// Byte-order aware writers for the spec interpreter.
fn put_u16(buf: &mut [u8], pos: usize, value: u16, little: bool) {
    let bytes = if little {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    };
    buf[pos..pos + 2].copy_from_slice(&bytes);
}

fn put_u32(buf: &mut [u8], pos: usize, value: u32, little: bool) {
    let bytes = if little {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    };
    buf[pos..pos + 4].copy_from_slice(&bytes);
}

fn spec_u64(node: &Value, key: &str) -> u64 {
    node[key]
        .as_u64()
        .unwrap_or_else(|| panic!("spec has no integer '{key}': {node}"))
}

/// `W503Jpeg`: the minimal JPEG, `lossless` rewrites its first SOF0 to SOF3, `app_segments` prepends N 65537-byte APP0s.
fn jpeg_from_spec(spec: &Value) -> Vec<u8> {
    let width = i32::try_from(spec_u64(spec, "width")).expect("width fits i32");
    let height = i32::try_from(spec_u64(spec, "height")).expect("height fits i32");
    let mut jpeg = create_minimal_jpeg(width, height);

    if spec["lossless"].as_bool() == Some(true) {
        for i in 0..jpeg.len().saturating_sub(1) {
            if jpeg[i] == 0xFF && jpeg[i + 1] == 0xC0 {
                jpeg[i + 1] = 0xC3;
                break;
            }
        }
    }

    if let Some(segments) = spec.get("app_segments").and_then(Value::as_u64) {
        let mut app = vec![0u8; 2 + 65_535];
        app[0] = 0xFF;
        app[1] = 0xE0;
        app[2] = 0xFF;
        app[3] = 0xFF;
        let mut out = vec![0xFF, 0xD8];
        for _ in 0..segments {
            out.extend_from_slice(&app);
        }
        out.extend_from_slice(&jpeg[2..]);
        jpeg = out;
    }

    jpeg
}

/// `W503Build`: applies the ops of a golden `file` object in order to a zeroed buffer of `size` bytes.
fn build_from_spec(file: &Value) -> Vec<u8> {
    let little = file["little"].as_bool().unwrap_or(true);
    let size = usize::try_from(spec_u64(file, "size")).expect("size fits usize");
    let mut buf = vec![0u8; size];

    for op in file["ops"].as_array().expect("file.ops") {
        match op["op"].as_str().expect("op name") {
            "header" => {
                let magic = match op["signature"].as_str() {
                    Some(hex) => from_hex(hex),
                    None if little => b"II*\0".to_vec(),
                    None => b"MM\0*".to_vec(),
                };
                buf[..magic.len()].copy_from_slice(&magic);
                put_u32(
                    &mut buf,
                    4,
                    u32::try_from(spec_u64(op, "ifd0")).expect("ifd0 fits u32"),
                    little,
                );
            }
            "put" => {
                let bytes = match op["hex"].as_str() {
                    Some(hex) => from_hex(hex),
                    None => jpeg_from_spec(&op["jpeg"]),
                };
                let at = usize::try_from(spec_u64(op, "at")).expect("at fits usize");
                buf[at..at + bytes.len()].copy_from_slice(&bytes);
            }
            "u16" => {
                let at = usize::try_from(spec_u64(op, "at")).expect("at fits usize");
                put_u16(&mut buf, at, u16::try_from(spec_u64(op, "value")).expect("u16"), little);
            }
            "u32" => {
                let at = usize::try_from(spec_u64(op, "at")).expect("at fits usize");
                put_u32(&mut buf, at, u32::try_from(spec_u64(op, "value")).expect("u32"), little);
            }
            "ifd" => {
                let at = usize::try_from(spec_u64(op, "at")).expect("at fits usize");
                let entries = op["entries"].as_array().expect("ifd.entries");
                put_u16(&mut buf, at, u16::try_from(entries.len()).expect("entry count"), little);
                for (i, entry) in entries.iter().enumerate() {
                    let p = at + 2 + i * 12;
                    let ty = u16::try_from(spec_u64(entry, "type")).expect("type u16");
                    let count = u32::try_from(spec_u64(entry, "count")).expect("count u32");
                    let value = u32::try_from(spec_u64(entry, "value")).expect("value u32");
                    put_u16(
                        &mut buf,
                        p,
                        u16::try_from(spec_u64(entry, "tag")).expect("tag u16"),
                        little,
                    );
                    put_u16(&mut buf, p + 2, ty, little);
                    put_u32(&mut buf, p + 4, count, little);
                    if ty == 3 && count == 1 {
                        // SHORT x 1 is left-justified in the value field.
                        put_u16(&mut buf, p + 8, u16::try_from(value).expect("short value"), little);
                    } else {
                        put_u32(&mut buf, p + 8, value, little);
                    }
                }
                let next = u32::try_from(spec_u64(op, "next")).expect("next u32");
                put_u32(&mut buf, at + 2 + entries.len() * 12, next, little);
            }
            other => panic!("unknown spec op {other}"),
        }
    }

    buf
}

// ---------------------------------------------------------------- helpers

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn reader() -> Cr2ContainerReader {
    Cr2ContainerReader::new()
}

/// Port of `Cr2ContainerReader.Read` through an in-memory header source.
fn read_bytes(data: Vec<u8>) -> Result<RawContainerInfo, RawHeaderError> {
    let mut source = InMemoryRawHeaderSource::new(data);
    reader().read(&mut source, &CancelToken::new())
}

fn read_ok(data: Vec<u8>) -> RawContainerInfo {
    read_bytes(data).expect("CR2 read should succeed")
}

/// The `Cr2Marker` of the regression tests: `CR` plus the 6 bytes after it.
const CR2_MARKER: &[u8; 8] = b"CR\x02\0\0\0\0\0";

/// `TiffPreviewHardeningTests.BudgetExhaustingJpeg`: 140 APP0 segments of 65537 bytes, then the minimal JPEG.
fn budget_exhausting_jpeg() -> Vec<u8> {
    let tail = create_minimal_jpeg(320, 240);
    let mut app = vec![0u8; 2 + 65_535];
    app[0] = 0xFF;
    app[1] = 0xE0;
    app[2] = 0xFF;
    app[3] = 0xFF;
    let mut out = vec![0xFF, 0xD8];
    for _ in 0..140 {
        out.extend_from_slice(&app);
    }
    out.extend_from_slice(&tail[2..]);
    out
}

/// `TiffPreviewHardeningTests.Build` for the CR2 row: IFD0 (orientation) -> IFD1 at 100 with an interchange JPEG at 1000.
fn cr2_interchange_file(jpeg: Option<&[u8]>) -> Vec<u8> {
    let mut file = TiffBytes::new(true, 3000)
        .header(8)
        .ifd(8, 100, &[short(0x0112, 1)])
        .ifd(
            100,
            0,
            &[
                short(0x0100, 9000),
                short(0x0101, 6000),
                long(0x0201, 1000),
                long(0x0202, u32::try_from(jpeg.map_or(200, <[u8]>::len)).expect("len")),
            ],
        );
    if let Some(bytes) = jpeg {
        file = file.put(1000, bytes);
    }
    file.into_vec()
}

/// `TiffPreviewHardeningTests.BuildWithJpeg` for the CR2 row: the file is sized to hold the whole JPEG.
fn cr2_interchange_file_sized(jpeg: &[u8]) -> Vec<u8> {
    TiffBytes::new(true, 1000 + jpeg.len())
        .header(8)
        .ifd(8, 100, &[short(0x0112, 1)])
        .ifd(
            100,
            0,
            &[
                short(0x0100, 9000),
                short(0x0101, 6000),
                long(0x0201, 1000),
                long(0x0202, u32::try_from(jpeg.len()).expect("len")),
            ],
        )
        .put(1000, jpeg)
        .into_vec()
}

/// `TiffExifBlockPlacementTests.Build`: IFD0 with dimensions, a Make far away and an Exif pointer; the Exif IFD far away too.
fn exif_placement_file(size: usize, make_offset: usize, exif_offset: usize) -> Vec<u8> {
    let make_offset_u32 = u32::try_from(make_offset).expect("make offset");
    let exif_offset_u32 = u32::try_from(exif_offset).expect("exif offset");
    let mut tiff = TiffBytes::new(true, size).header(8).ifd(
        8,
        0,
        &[
            short(0x0100, 4000),
            short(0x0101, 3000),
            at(0x010F, 2, 6, make_offset_u32),
            long(0x8769, exif_offset_u32),
        ],
    );
    tiff = tiff.put(make_offset, b"Acme\0\0");
    tiff.ifd(exif_offset, 0, &[short(0x8827, 400)]).into_vec()
}

/// `info` of a golden case, rebuilt from the Rust result with the same field names and enum spellings.
fn info_json(info: &RawContainerInfo) -> Value {
    json!({
        "format": format!("{:?}", info.format),
        "sensor_width": info.sensor_width,
        "sensor_height": info.sensor_height,
        "orientation": info.orientation,
        "previews": info.previews.iter().map(|p| json!({
            "index": p.index,
            "offset": p.offset,
            "length": p.length,
            "kind": format!("{:?}", p.kind),
            "width": p.width,
            "height": p.height,
            "color_space": format!("{:?}", p.color_space),
            "header_resolved": p.header_resolved,
        })).collect::<Vec<_>>(),
        "exif_blocks": info.exif_blocks.iter().map(|b| json!({
            "offset": b.offset,
            "length": b.length,
            "is_tiff_header": b.is_tiff_header,
            "ifd_is_exif": b.ifd_is_exif,
        })).collect::<Vec<_>>(),
    })
}

/// A header source that reports a larger file than the bytes behind it (C# `HugeFileSource` / `W503HugeSource`).
struct ReportedLengthSource {
    inner: InMemoryRawHeaderSource,
    length: i64,
}

impl RawHeaderSource for ReportedLengthSource {
    fn length(&self) -> i64 {
        self.length
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.inner.read(offset, count)
    }
}

// ---------------------------------------------------------------- CanRead and header (Cr2ArwRafMutationGapTests)

// C#: Cr2ArwRafMutationGapTests.Cr2CanRead_ExactlyTenBytesWithEachSignatureByteChecked
#[test]
fn cr2_can_read_exactly_ten_bytes_with_each_signature_byte_checked() {
    let head: [u8; 10] = [0x49, 0x49, 0x2A, 0x00, 0x10, 0x00, 0x00, 0x00, b'C', b'R'];
    let reader = reader();

    assert!(reader.can_read(&head, ".cr2"));
    assert!(!reader.can_read(&head[..9], ".cr2"));
    for position in [0usize, 1, 2, 3, 8, 9] {
        let mut broken = head;
        broken[position] ^= 0x40;
        assert!(
            !reader.can_read(&broken, ".cr2"),
            "byte {position} flipped must be rejected"
        );
    }
}

// C#: Cr2ArwRafMutationGapTests.Read_SixteenByteFileWithEmptyIfd_ReturnsEmptyInfoForCr2AndArw (CR2 half)
#[test]
fn read_sixteen_byte_file_with_empty_ifd_returns_empty_info_for_cr2() {
    let info = read_ok(TiffBytes::new(true, 16).header(8).ifd(8, 0, &[]).into_vec());

    assert_eq!((info.sensor_width, info.sensor_height), (0, 0));
    assert!(info.previews.is_empty());
    assert_eq!(info.exif_blocks.len(), 1);
}

// C#: Cr2ArwRafMutationGapTests.ReadCr2_ChainOfSixtySixIfds_ReadsOnlyTheFirstSixtyFour
#[test]
fn read_cr2_chain_of_sixty_six_ifds_reads_only_the_first_sixty_four() {
    const IFD_SIZE: usize = 30;
    let mut file = TiffBytes::new(true, 8 + 66 * IFD_SIZE).header(8);
    for i in 0..66usize {
        let at = 8 + i * IFD_SIZE;
        let next = if i < 65 {
            u32::try_from(at + IFD_SIZE).expect("next")
        } else {
            0
        };
        file = file.ifd(
            at,
            next,
            &[short(0x0100, u32::try_from(100 + i).expect("w")), short(0x0101, 100)],
        );
    }

    let info = read_ok(file.into_vec());

    assert_eq!((info.sensor_width, info.sensor_height), (163, 100));
}

// C#: Cr2ArwRafMutationGapTests.ReadCr2_TwoIfdsOfEqualArea_KeepsTheFirst
#[test]
fn read_cr2_two_ifds_of_equal_area_keeps_the_first() {
    let data = TiffBytes::new(true, 128)
        .header(8)
        .ifd(8, 60, &[short(0x0100, 4000), short(0x0101, 3000)])
        .ifd(60, 0, &[short(0x0100, 3000), short(0x0101, 4000)])
        .into_vec();

    let info = read_ok(data);

    assert_eq!((info.sensor_width, info.sensor_height), (4000, 3000));
}

// C#: Cr2ArwRafMutationGapTests.ReadCr2_Ifd0StripThatDoesNotStartWithSoi_IsNotAPreview
#[test]
fn read_cr2_ifd0_strip_that_does_not_start_with_soi_is_not_a_preview() {
    let data = TiffBytes::new(true, 512)
        .header(8)
        .ifd(
            8,
            0,
            &[
                short(0x0100, 100),
                short(0x0101, 100),
                long(0x0111, 300),
                long(0x0117, 100),
            ],
        )
        .into_vec();

    assert!(read_ok(data).previews.is_empty());
}

// C#: Cr2ArwRafMutationGapTests.ReadCr2_Ifd0StripWithSoi_IsAPreview
#[test]
fn read_cr2_ifd0_strip_with_soi_is_a_preview() {
    let jpeg = create_minimal_jpeg(640, 480);
    let jpeg_len = u32::try_from(jpeg.len()).expect("jpeg len");
    let data = TiffBytes::new(true, 300 + jpeg.len())
        .header(8)
        .ifd(
            8,
            0,
            &[
                short(0x0100, 100),
                short(0x0101, 100),
                long(0x0111, 300),
                long(0x0117, jpeg_len),
            ],
        )
        .put(300, &jpeg)
        .into_vec();

    let info = read_ok(data);

    assert_eq!(info.previews.len(), 1);
    let preview = &info.previews[0];
    assert_eq!(
        (preview.offset, preview.length, preview.width, preview.height),
        (300, i64::from(jpeg_len), 640, 480)
    );
}

// C#: Cr2ArwRafMutationGapTests.ReadCr2_WithoutAnExifPointer_StillReportsOneExifBlock
#[test]
fn read_cr2_without_an_exif_pointer_still_reports_one_exif_block() {
    let data = TiffBytes::new(true, 128)
        .header(8)
        .ifd(8, 0, &[short(0x0100, 100), short(0x0101, 100)])
        .into_vec();

    let info = read_ok(data);

    assert_eq!(info.exif_blocks.len(), 1);
    assert_eq!(
        (info.exif_blocks[0].offset, info.exif_blocks[0].is_tiff_header),
        (0, true)
    );
}

// ---------------------------------------------------------------- TiffFamilyReaderTests / TiffReaderRegressionTests

// C#: TiffFamilyReaderTests.Cr2Reader_Synthetic_CanReadAndParsePreview
#[test]
fn cr2_reader_synthetic_can_read_and_parse_preview() {
    let jpeg = create_minimal_jpeg(640, 480);
    let jpeg_offset: u32 = 16 + 2 + 3 * 12 + 4;
    let mut data = Vec::new();
    data.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00]); // II, 42
    data.extend_from_slice(&[0x10, 0x00, 0x00, 0x00]); // IFD0 at offset 16
    data.extend_from_slice(b"CR\x02\0\0\0\0\0"); // CR2 marker
    data.extend_from_slice(&[0x03, 0x00]); // 3 entries
    write_entry(&mut data, 0x0111, 4, 1, jpeg_offset); // StripOffsets
    write_entry(&mut data, 0x0117, 4, 1, u32::try_from(jpeg.len()).expect("len")); // StripByteCounts
    write_entry(&mut data, 0x0112, 3, 1, 6); // Orientation = 6
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // next IFD = 0
    data.extend_from_slice(&jpeg);

    let cr2 = reader();
    assert!(cr2.can_read(&data[..data.len().min(64)], ".cr2"));
    let info = read_ok(data);

    assert_eq!(info.format, RawFormat::Cr2);
    assert_eq!(info.orientation, 6);
    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].offset, i64::from(jpeg_offset));
    assert_eq!(info.previews[0].length, i64::try_from(jpeg.len()).expect("len"));
}

/// Local `WriteEntry` of `TiffFamilyReaderTests` (SHORT values are left-justified).
fn write_entry(out: &mut Vec<u8>, tag: u16, ty: u16, count: u32, value: u32) {
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&ty.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    if ty == 3 {
        out.extend_from_slice(&(value as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
    } else {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

// C#: TiffReaderRegressionTests.Cr2Reader_Ifd0IsFullSizePreview_TakesSensorSizeFromExifPixelDimensions
#[test]
fn cr2_reader_ifd0_is_full_size_preview_takes_sensor_size_from_exif_pixel_dimensions() {
    // sRAW: IFD0 preview is 5184x3456 while the actual image is 2592x1728.
    let baseline = create_minimal_jpeg(640, 480);
    let data = TiffBytes::new(true, 600 + baseline.len())
        .header(16)
        .put(8, CR2_MARKER)
        .ifd(
            16,
            0,
            &[
                short(0x0100, 5184),
                short(0x0101, 3456),
                short(0x0103, 6),
                long(0x0111, 600),
                long(0x0117, u32::try_from(baseline.len()).expect("len")),
                long(0x8769, 300),
            ],
        )
        .ifd(300, 0, &[short(0xA002, 2592), short(0xA003, 1728)])
        .put(600, &baseline)
        .into_vec();

    let info = read_ok(data);

    assert_eq!((info.sensor_width, info.sensor_height), (2592, 1728));
    assert_eq!(info.previews.len(), 1);
    assert_eq!(info.previews[0].offset, 600);
}

// C#: TiffReaderRegressionTests.Cr2Reader_NoExifDimensions_FallsBackToLargestIfd
#[test]
fn cr2_reader_no_exif_dimensions_falls_back_to_largest_ifd() {
    let data = TiffBytes::new(true, 900)
        .header(16)
        .put(8, CR2_MARKER)
        .ifd(16, 200, &[short(0x0100, 400), short(0x0101, 300)])
        .ifd(200, 0, &[short(0x0100, 6880), short(0x0101, 4544)])
        .into_vec();

    let info = read_ok(data);

    assert_eq!((info.sensor_width, info.sensor_height), (6880, 4544));
}

// C#: TiffReaderRegressionTests.Cr2Reader_PreviewStoredOverSeveralStrips_IsNotListed
#[test]
fn cr2_reader_preview_stored_over_several_strips_is_not_listed() {
    let baseline = create_minimal_jpeg(640, 480);
    let data = TiffBytes::new(true, 600 + baseline.len())
        .header(16)
        .put(8, CR2_MARKER)
        .ifd(16, 0, &[short(0x0103, 6), at(0x0111, 4, 2, 300), at(0x0117, 4, 2, 308)])
        .u32(300, 600)
        .u32(304, 610)
        .u32(308, 10)
        .u32(312, u32::try_from(baseline.len() - 10).expect("len"))
        .put(600, &baseline)
        .into_vec();

    assert!(read_ok(data).previews.is_empty());
}

// ---------------------------------------------------------------- TiffStripPreviewTests (cr2 row)

/// `TiffStripPreviewTests.Build`: IFD0 declaring 9000x6000, Compression 7, one strip at 1000.
fn strip_file(jpeg: &[u8], declared_strip_length: u32) -> Vec<u8> {
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
                long(0x0117, declared_strip_length),
            ],
        )
        .put(1000, jpeg)
        .into_vec()
}

// C#: TiffStripPreviewTests.Read_StripPreviewWhoseDeclaredSizeContradictsTheJpegFrame_UsesTheFrameSize (cr2 row)
#[test]
fn read_strip_preview_whose_declared_size_contradicts_the_jpeg_frame_uses_the_frame_size_cr2() {
    let jpeg = create_minimal_jpeg(320, 240);
    let info = read_ok(strip_file(&jpeg, u32::try_from(jpeg.len()).expect("len")));

    let preview = info
        .previews
        .iter()
        .find(|p| p.offset == 1000)
        .expect("preview at 1000");
    assert_eq!((preview.width, preview.height), (320, 240));
}

/// `HugeFileSource` (C# `TiffStripPreviewTests`): a 3.5 GB report over the 3000-byte buffer.
// C#: TiffStripPreviewTests.Read_Cr2StripLengthAboveIntMaxValue_DoesNotWrapNegativeOrThrowANonCorruptException
#[test]
fn read_cr2_strip_length_above_int_max_value_does_not_wrap_negative_or_throw_a_non_corrupt_exception() {
    let jpeg = create_minimal_jpeg(320, 240);
    let data = strip_file(&jpeg, 3_000_000_000);
    let mut source = ReportedLengthSource {
        inner: InMemoryRawHeaderSource::new(data),
        length: 3_500_000_000,
    };

    // Before the fix (an `i32` narrowing of the strip length) this wrapped negative and threw ArgumentOutOfRange.
    let info = reader()
        .read(&mut source, &CancelToken::new())
        .expect("CR2 read should succeed");

    let preview = info
        .previews
        .iter()
        .find(|p| p.offset == 1000)
        .expect("preview at 1000");
    assert_eq!(preview.length, 3_000_000_000);
}

// ---------------------------------------------------------------- TiffPreviewHardeningTests (cr2 rows)

// C#: TiffPreviewHardeningTests.Read_JpegInterchangePointerToZeroPadding_IsNotAPreview (cr2 row)
#[test]
fn read_jpeg_interchange_pointer_to_zero_padding_is_not_a_preview_cr2() {
    let info = read_ok(cr2_interchange_file(None));

    assert!(
        info.previews.iter().all(|p| p.offset != 1000),
        "zero padding must not be listed"
    );
}

// C#: TiffPreviewHardeningTests.Read_DeclaredSizeContradictingTheJpegFrame_UsesTheFrameSize (cr2 row)
#[test]
fn read_declared_size_contradicting_the_jpeg_frame_uses_the_frame_size_cr2() {
    let jpeg = create_minimal_jpeg(320, 240);
    let info = read_ok(cr2_interchange_file(Some(&jpeg)));

    let preview = info
        .previews
        .iter()
        .find(|p| p.offset == 1000)
        .expect("preview at 1000");
    assert_eq!((preview.width, preview.height), (320, 240));
}

// C#: TiffPreviewHardeningTests.Read_PreviewJpegExhaustingHeaderBudget_ContainerStillParsesAndKeepsDeclaredSize (cr2 row)
#[test]
fn read_preview_jpeg_exhausting_header_budget_container_still_parses_and_keeps_declared_size_cr2() {
    let info = read_ok(cr2_interchange_file_sized(&budget_exhausting_jpeg()));

    let preview = info
        .previews
        .iter()
        .find(|p| p.offset == 1000)
        .expect("preview at 1000");
    assert!(preview.width == 0 || preview.width == 9000, "width {}", preview.width);
    assert!(
        preview.height == 0 || preview.height == 6000,
        "height {}",
        preview.height
    );
}

// ---------------------------------------------------------------- TiffExifBlockPlacementTests (cr2 rows, block assertions)

// C#: TiffExifBlockPlacementTests.TryReadExif_IfdsAfter128KiB_StillYieldsSummary (cr2 row; the RawExif half is W5-11)
#[test]
fn try_read_exif_ifds_after_128_kib_still_yields_summary_cr2() {
    let info = read_ok(exif_placement_file(310_000, 300_000, 200_100));

    assert!(!info.exif_blocks.is_empty());
    for block in &info.exif_blocks {
        assert_eq!(block.offset, 0);
        assert!(block.length > 128 * 1024, "block of {} bytes", block.length);
    }
}

// C#: TiffExifBlockPlacementTests.Read_IfdsInsideDefaultWindow_KeepsDefaultBlockLength (cr2 row)
#[test]
fn read_ifds_inside_default_window_keeps_default_block_length_cr2() {
    let info = read_ok(exif_placement_file(310_000, 2000, 3000));

    for block in &info.exif_blocks {
        assert_eq!(block.length, i64::from(DEFAULT_EXIF_BLOCK_BYTES));
    }
}

// C#: TiffExifBlockPlacementTests.Read_IfdsBeyondMaxBlock_CapsBlockAndDoesNotThrow (cr2 row, first assert only)
#[test]
fn read_ifds_beyond_max_block_caps_block_and_does_not_throw_cr2() {
    let far = usize::try_from(MAX_EXIF_BLOCK_BYTES).expect("cap fits usize") + 50_000;
    let info = read_ok(exif_placement_file(far + 10_000, far, far + 100));

    // An extent that cannot fit keeps the default block (it was grown to the 4 MiB cap for nothing).
    for block in &info.exif_blocks {
        assert_eq!(block.length, i64::from(DEFAULT_EXIF_BLOCK_BYTES));
    }
}

// ---------------------------------------------------------------- RawErrorHandlingReviewTests

// C#: RawErrorHandlingReviewTests.Cr2_ManyIfdsWithExifPointer_YieldsOneExifBlock
#[test]
fn cr2_many_ifds_with_exif_pointer_yields_one_exif_block() {
    let data = TiffBytes::new(true, 4096)
        .header(8)
        .ifd(8, 200, &[short(0x0100, 4000), short(0x0101, 3000), long(0x8769, 1000)])
        .ifd(200, 400, &[short(0x0100, 100), short(0x0101, 100), long(0x8769, 1000)])
        .ifd(400, 0, &[short(0x0100, 50), short(0x0101, 50), long(0x8769, 1000)])
        .ifd(1000, 0, &[short(0x8827, 400)])
        .into_vec();

    let info = read_ok(data);

    assert_eq!(info.exif_blocks.len(), 1);
}

// C#: TiffReaderRegressionTests.Readers_ArrayEntryCountAboveInt32Max_DoNotThrowUnexpectedExceptions (.cr2 row)
#[test]
fn readers_array_entry_count_above_int32_max_do_not_throw_unexpected_exceptions_cr2() {
    let mut data = TiffBytes::new(true, 512)
        .header(16)
        .put(8, CR2_MARKER)
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
    data[2] = 0x2A;

    // Only `Ok` or `InvalidData` is allowed: anything else (a panic, `Canceled`, `ArgumentOutOfRange`) is a bug.
    match read_bytes(data) {
        Ok(_) => {}
        Err(RawHeaderError::InvalidData { .. }) => {}
        Err(other) => panic!("unexpected error: {other}"),
    }
}

// ---------------------------------------------------------------- Robustness (fuzz)

/// Seed of the CR2 fuzz: golden case `read-fuzz-seed-shape-cr-overwrites-entry-count` (215 bytes, `CR` over the entry count).
fn cr2_fuzz_seed() -> Vec<u8> {
    let golden_doc = golden::load("cr2-container");
    let case = golden_doc
        .cases
        .iter()
        .find(|c| c["name"] == "read-fuzz-seed-shape-cr-overwrites-entry-count")
        .expect("fuzz seed case in cr2-container.json");
    golden_bytes(&case["data"])
}

/// One mutant: 1 to 4 bit flips, byte stores or a truncation, drawn from the xorshift stream.
fn mutate(seed: &[u8], rng: &mut XorShift64) -> Vec<u8> {
    let mut out = seed.to_vec();
    for _ in 0..(1 + rng.below(4)) {
        if out.is_empty() {
            break;
        }
        match rng.below(3) {
            0 => {
                let i = rng.below(out.len());
                out[i] ^= 1 << rng.below(8);
            }
            1 => {
                let i = rng.below(out.len());
                out[i] = u8::try_from(rng.next_u64() & 0xFF).expect("byte");
            }
            _ => {
                let len = rng.below(out.len() + 1);
                out.truncate(len);
            }
        }
    }
    out
}

// C#: RawContainerFuzzTests.Fuzz_Cr2ContainerReader_Robust (the p95 timing assert is dropped, see the module DIVERGENCE)
#[test]
fn fuzz_cr2_container_reader_robust() {
    let seed = cr2_fuzz_seed();
    let cr2 = reader();
    let mut rng = XorShift64::new(101);

    for iteration in 0..10_000usize {
        let mutated = mutate(&seed, &mut rng);
        let head = &mutated[..mutated.len().min(64)];
        if !cr2.can_read(head, ".cr2") {
            continue;
        }
        let mut source = InMemoryRawHeaderSource::new(mutated);
        match cr2.read(&mut source, &CancelToken::new()) {
            Ok(info) => {
                assert!(
                    (1..=8).contains(&info.orientation),
                    "iteration {iteration}: orientation {}",
                    info.orientation
                );
                assert!(
                    info.sensor_width >= 0 && info.sensor_height >= 0,
                    "iteration {iteration}: negative size"
                );
            }
            Err(RawHeaderError::InvalidData { .. }) => {}
            Err(other) => panic!("iteration {iteration}: unexpected error {other}"),
        }
        assert!(
            source.total_bytes_read() <= i64::from(MAX_HEADER_BYTES),
            "iteration {iteration}: {} bytes charged, budget {MAX_HEADER_BYTES}",
            source.total_bytes_read()
        );
    }
}

// C#: BinaryReaderFuzzTests.Fuzz_RawContainerReaders_FailOnlyCleanly (the CR2 target, same seed family)
#[test]
fn fuzz_raw_container_readers_fail_only_cleanly_cr2_target() {
    let seed = {
        // `SyntheticRawBuilder.BuildTiff(true, Jpeg() /* 64x48 */, 6, 42)` with `CR` spliced in at offset 8.
        let tiff = build_tiff(true, Some(&create_minimal_jpeg(64, 48)), 6, 42);
        let mut cr2 = Vec::with_capacity(tiff.len() + 2);
        cr2.extend_from_slice(&tiff[..8]);
        cr2.extend_from_slice(b"CR");
        cr2.extend_from_slice(&tiff[8..]);
        cr2
    };
    let cr2 = reader();

    // The seed itself is accepted.
    let info = read_bytes(seed.clone()).expect("the CR2 seed must be accepted");
    assert!((1..=8).contains(&info.orientation));

    for case in pr_test_support::raw::mutants(&seed, 4400, 600) {
        let head = &case.bytes[..case.bytes.len().min(64)];
        if !cr2.can_read(head, ".cr2") {
            continue;
        }
        let mut source = InMemoryRawHeaderSource::new(case.bytes.clone());
        match cr2.read(&mut source, &CancelToken::new()) {
            Ok(info) => {
                assert!(
                    (1..=8).contains(&info.orientation),
                    "{}: orientation {}",
                    case.label,
                    info.orientation
                );
                assert!(
                    info.sensor_width >= 0 && info.sensor_height >= 0,
                    "{}: negative size",
                    case.label
                );
                for preview in &info.previews {
                    let end = preview.offset.checked_add(preview.length);
                    assert!(
                        preview.offset >= 0 && preview.length >= 0 && end.is_some_and(|e| e <= case.bytes.len() as i64),
                        "{}: preview {}+{} escapes the {}-byte file",
                        case.label,
                        preview.offset,
                        preview.length,
                        case.bytes.len()
                    );
                }
            }
            Err(RawHeaderError::InvalidData { .. }) => {}
            Err(other) => panic!("{}: unexpected error {other}", case.label),
        }
    }
}

// Extra (contract "Done when": "the every-truncation test proves it"): no prefix of the seed panics.
#[test]
fn cr2_every_truncation_of_the_seed_fails_cleanly() {
    let seed = cr2_fuzz_seed();
    for len in 0..seed.len() {
        match read_bytes(seed[..len].to_vec()) {
            Ok(_) => {}
            Err(RawHeaderError::InvalidData { .. }) => {}
            Err(other) => panic!("truncated to {len} bytes: unexpected error {other}"),
        }
    }
}

// ---------------------------------------------------------------- Golden: cr2-container.json

// C#: oracle `cr2-container` `can-read` cases (21: every signature byte flipped, every extension spelling)
#[test]
fn golden_cr2_container_can_read_matches_csharp() {
    let golden_doc = golden::load("cr2-container");
    let cases: Vec<&Value> = golden_doc.cases.iter().filter(|c| c["op"] == "can-read").collect();
    assert_eq!(cases.len(), 21, "golden case count");

    for case in cases {
        let name = golden::str_of(case, "name");
        let head = from_hex(golden::str_of(&case["head"], "hex"));
        let ext = golden::str_of(case, "ext");
        assert_eq!(
            reader().can_read(&head, ext),
            golden::bool_of(case, "can_read"),
            "case {name}"
        );
    }
}

// C#: oracle `cr2-container` inputs: the spec interpreter (`build_from_spec`, port of `W503Build`) builds the same bytes as
// C# for every `read` case (length and SHA-256). Independent of `read`, so it is the one check that must pass before the port.
#[test]
fn golden_cr2_container_inputs_match_csharp_digests() {
    let golden_doc = golden::load("cr2-container");
    for case in golden_doc.cases.iter().filter(|c| c["op"] == "read") {
        let name = golden::str_of(case, "name");
        let data = match case.get("file") {
            Some(file) => build_from_spec(file),
            None => golden_bytes(&case["data"]),
        };
        assert_eq!(
            i64::try_from(data.len()).expect("len"),
            golden::i64_of(case, "data_length"),
            "case {name}: data_length"
        );
        assert_eq!(
            hex(&sha256(&data)),
            golden::str_of(case, "data_sha256"),
            "case {name}: data_sha256"
        );
    }
}

// C#: oracle `cr2-container` `read` cases (132: header, IFD, chain, strip, interchange, Exif, hostile, cancel, budget)
#[test]
fn golden_cr2_container_read_matches_csharp() {
    let golden_doc = golden::load("cr2-container");
    let cases: Vec<&Value> = golden_doc.cases.iter().filter(|c| c["op"] == "read").collect();
    assert_eq!(cases.len(), 132, "golden case count");

    for case in cases {
        let name = golden::str_of(case, "name");

        // Input bytes and their digest first: they prove the spec interpreter builds the same file as C#.
        let data = match case.get("file") {
            Some(file) => build_from_spec(file),
            None => golden_bytes(&case["data"]),
        };
        assert_eq!(
            i64::try_from(data.len()).expect("len"),
            golden::i64_of(case, "data_length"),
            "case {name}: data_length"
        );
        assert_eq!(
            hex(&sha256(&data)),
            golden::str_of(case, "data_sha256"),
            "case {name}: data_sha256"
        );

        let token = CancelToken::new();
        if case["cancel"].as_bool() == Some(true) {
            token.cancel();
        }

        let (result, bytes_read) = match case.get("reported_length").and_then(Value::as_i64) {
            Some(length) => {
                let mut source = ReportedLengthSource {
                    inner: InMemoryRawHeaderSource::new(data),
                    length,
                };
                let result = reader().read(&mut source, &token);
                let charged = source.inner.total_bytes_read();
                (result, charged)
            }
            None => {
                let mut source = InMemoryRawHeaderSource::new(data);
                let result = reader().read(&mut source, &token);
                let charged = source.total_bytes_read();
                (result, charged)
            }
        };

        assert_eq!(
            bytes_read,
            golden::i64_of(case, "bytes_read"),
            "case {name}: bytes_read"
        );

        match case["throws"].as_str() {
            None => {
                let info = result.unwrap_or_else(|e| panic!("case {name}: expected Ok, got {e}"));
                assert_eq!(info_json(&info), case["info"], "case {name}: info");
            }
            Some("InvalidDataException") => {
                let err = result
                    .err()
                    .unwrap_or_else(|| panic!("case {name}: expected InvalidData"));
                assert!(
                    matches!(err, RawHeaderError::InvalidData { .. }),
                    "case {name}: got {err}"
                );
                assert_eq!(
                    Value::Bool(err.is_io_failure()),
                    case["io_failure"],
                    "case {name}: io_failure"
                );
            }
            Some("OperationCanceledException") => {
                let err = result.err().unwrap_or_else(|| panic!("case {name}: expected Canceled"));
                assert!(matches!(err, RawHeaderError::Canceled), "case {name}: got {err}");
            }
            Some(other) => panic!("case {name}: unknown throws kind {other}"),
        }
    }
}

// C#: oracle `cr2-container-corpus` (zero cases until the RAW corpus is fetched, D49 / P-30)
#[test]
fn golden_cr2_container_corpus_matches_csharp() {
    let path = golden::repo_root()
        .join("tests")
        .join("golden")
        .join("cr2-container-corpus.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let cases = doc["cases"].as_array().expect("cases array");

    if cases.is_empty() {
        // No corpus means no cases (D49 / P-30). Corpus cases are compared once the corpus is fetched, with the schema of
        // the corpus cases (`can_read`, `info`, `bytes_read`, `previews_start_with_soi`) checked by the porter then.
        return;
    }
    panic!(
        "cr2-container-corpus.json has {} cases: the corpus comparison is not ported yet",
        cases.len()
    );
}

// ---------------------------------------------------------------- Corpus (skips when the RAW corpus is absent, D49 / P-30)

/// `PHOTOREVIEW_RAW_CORPUS_DIR`, else `<repo>/tests/fixtures/raw-corpus`. `None` (skip) when absent, a panic when
/// `PHOTOREVIEW_RAW_CORPUS_STRICT` is `1` or `true`.
fn corpus_dir() -> Option<PathBuf> {
    let strict = std::env::var("PHOTOREVIEW_RAW_CORPUS_STRICT")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true"))
        .unwrap_or(false);
    let dir = std::env::var_os("PHOTOREVIEW_RAW_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| golden::repo_root().join("tests").join("fixtures").join("raw-corpus"));
    if dir.is_dir() {
        return Some(dir);
    }
    assert!(
        !strict,
        "PHOTOREVIEW_RAW_CORPUS_STRICT is set but no corpus at {}",
        dir.display()
    );
    corpus_skip::skipped(&format!("no corpus at {}", dir.display()));
    None
}

/// Corpus files with the given extension, sorted by name.
fn corpus_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        })
        .collect();
    files.sort();
    files
}

fn corpus_read(path: &Path) -> RawContainerInfo {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    read_ok(bytes)
}

/// `TiffCorpusTests.Cr2Corpus_SensorSizeIsTheImageSizeNotTheIfd0PreviewSize` (one row per camera model).
fn check_cr2_corpus_sensor_size(model: &str, width: i32, height: i32) {
    let Some(dir) = corpus_dir() else { return };
    let Some(file) = corpus_files(&dir, "cr2").into_iter().find(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_lowercase().contains(&model.to_lowercase()))
    }) else {
        return;
    };

    let info = corpus_read(&file);

    assert_eq!(
        (info.sensor_width, info.sensor_height),
        (width, height),
        "{}",
        file.display()
    );
}

// C#: TiffCorpusTests.Cr2Corpus_SensorSizeIsTheImageSizeNotTheIfd0PreviewSize [350D]
#[test]
fn cr2_corpus_sensor_size_is_the_image_size_not_the_ifd0_preview_size_350d() {
    check_cr2_corpus_sensor_size("350D", 3456, 2304);
}

// C#: TiffCorpusTests.Cr2Corpus_SensorSizeIsTheImageSizeNotTheIfd0PreviewSize [5D Mark IV]
#[test]
fn cr2_corpus_sensor_size_is_the_image_size_not_the_ifd0_preview_size_5d_mark_iv() {
    check_cr2_corpus_sensor_size("5D Mark IV", 6720, 4480);
}

// C#: TiffCorpusTests.Cr2Corpus_SensorSizeIsTheImageSizeNotTheIfd0PreviewSize [sRAW]
#[test]
fn cr2_corpus_sensor_size_is_the_image_size_not_the_ifd0_preview_size_sraw() {
    check_cr2_corpus_sensor_size("sRAW", 2592, 1728);
}

// C#: TiffCorpusTests.AllCorpusTiffSamples_CanBeParsed (the .cr2 part; the registry lookup is W5-11)
#[test]
fn all_corpus_tiff_samples_can_be_parsed_cr2() {
    let Some(dir) = corpus_dir() else { return };
    for file in corpus_files(&dir, "cr2") {
        let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        assert!(
            reader().can_read(&bytes[..bytes.len().min(64)], ".cr2"),
            "{}",
            file.display()
        );
        let info = read_ok(bytes);
        assert!(!info.previews.is_empty(), "{}: no preview", file.display());
        assert!(info.previews[0].length > 0, "{}: empty preview", file.display());
    }
}

// C#: TiffCorpusTests.AllTiffCorpusFiles_EveryListedPreviewLiesInsideFileAndStartsWithSoi (the .cr2 part)
#[test]
fn all_tiff_corpus_files_every_listed_preview_lies_inside_file_and_starts_with_soi_cr2() {
    let Some(dir) = corpus_dir() else { return };
    for file in corpus_files(&dir, "cr2") {
        let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        let info = read_ok(bytes.clone());
        for preview in &info.previews {
            let start = usize::try_from(preview.offset).expect("offset");
            let end = start.checked_add(2).expect("range");
            assert!(
                end <= bytes.len(),
                "{}: preview at {} escapes the file",
                file.display(),
                preview.offset
            );
            assert_eq!(
                &bytes[start..end],
                &[0xFF, 0xD8],
                "{}: preview at {} is not SOI",
                file.display(),
                preview.offset
            );
        }
    }
}
