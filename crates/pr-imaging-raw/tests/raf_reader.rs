//! Red tests for W5-09 (`readers/raf.rs`): `RafContainerReader`, the Fujifilm RAF container reader (C# baseline d92d6aae,
//! `src/PhotoReview.Imaging.Raw/Raf/RafContainerReader.cs`). Ports the RAF parts of `Raw/RafSensorSizeTests`,
//! `Raw/RafPreviewRangeTests`, `Raw/Tiff/Cr2ArwRafMutationGapTests`, `Raw/Cr3AndRafReaderTests`, `Raw/RawOrientationTests`,
//! `Raw/Cr3AndRafCorpusTests`, the RAF row of `Robustness/RawContainerFuzzTests` and of `Robustness/BinaryReaderFuzzTests`.
//! Every ported test names its C# test in a `// C#:` comment. Goldens: `raf-container` (123 cases), `raf-can-read` (38),
//! `raf-corpus` (3, native). Not here (they call this reader but test other code): `Decode_RafWithTruncatedJpeg_FallsBackToTheFullDecoder`
//! (W5-12), `RawReadInfoAndHeaderSourceParityTests` RAF facts, `PreviewSelectorJpegFrameTests`, `RawContractsAndHeaderSourceTests`
//! (W5-11/W5-12), and the `RawDecoder_*` / RW2 / CR3 tests of `RawOrientationTests`.
//!
//! The C# `SyntheticRawBuilder` and `FixtureGenerator` are not ported as a whole: the layouts below are the ones pinned by the
//! golden file (`raf-container`, layout `synthetic-default-1024x768`) and by `tests/golden-inputs` of `gen-inputs.js`
//! (`jpegWithExif`), so the test bytes match the bytes the golden was produced from.
//!
//! Stub bodies are `todo!()`, so every test that reaches the reader fails until the implementation lands.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use pr_core::sync::CancelToken;
use pr_imaging_raw::header_source::SourceRawHeaderSource;
use pr_imaging_raw::readers::raf::{
    CfaSizes, MAGIC, MAX_CFA_RECORDS, RafContainerReader, choose_sensor_size, parse_cfa_records,
};
use pr_imaging_raw::{
    EmbeddedPreviewKind, InMemoryRawHeaderSource, PreviewColorSpace, RawContainerInfo, RawContainerReader, RawFormat,
    RawHeaderError, RawHeaderSource,
};
use pr_test_support::golden::{self, bool_of, i64_of, repo_root, str_of};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------------
// Byte builders (port of the C# builders; layouts pinned by the goldens)
// ---------------------------------------------------------------------------------------------------------------------

/// `hex` -> bytes (the golden `hex` parts; ASCII hex, even length).
fn hex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "odd hex length: {text}");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex digit"))
        .collect()
}

/// APP0 + DQT header of the synthetic JPEG (golden `synthetic-default-1024x768`, part 3).
const JPEG_APP0_DQT_HEX: &str = "FFE000104A46494600010100000100010000FFDB004300";
/// SOF0 tail after height/width (golden part 5): components, DHT, SOS, EOI.
const JPEG_SOF_TAIL_HEX: &str = "03012200021101031101FFC4001F0000010501010101010100000000000000000102030405060708090A0BFFDA000C03010002110311003F00AA55AA55AA55FFD9";

/// A JPEG frame: SOI, the given APP1 segment bytes (may be empty), APP0/DQT, 64 DQT values, SOF0 `width` x `height`, the rest.
/// Byte-for-byte the golden `synthetic-default-1024x768` JPEG when `app1` is empty.
fn jpeg_frame(app1: &[u8], width: u16, height: u16) -> Vec<u8> {
    let mut out = hex("FFD8");
    out.extend_from_slice(app1);
    out.extend(hex(JPEG_APP0_DQT_HEX));
    out.extend(std::iter::repeat_n(0x10u8, 64));
    out.extend(hex("FFC00011"));
    out.push(0x08);
    out.extend(height.to_be_bytes());
    out.extend(width.to_be_bytes());
    out.extend(hex(JPEG_SOF_TAIL_HEX));
    out
}

/// `CreateMinimalJpeg(width, height)`: a JPEG without EXIF (golden `synthetic-default-1024x768` for 1024x768).
fn minimal_jpeg(width: u16, height: u16) -> Vec<u8> {
    jpeg_frame(&[], width, height)
}

/// `jpeg_with_exif(orientation)`: a 640x480 JPEG whose first APP1 Exif holds IFD0 Orientation `orientation`
/// (golden `orientation-1-big-endian` / `orientation-3-little-endian`; `gen-inputs.js` `jpegWithExif`).
fn jpeg_with_exif(orientation: u16, little_endian: bool) -> Vec<u8> {
    // APP1 segment (FFE1, length 0x22), `Exif\0\0`, TIFF header, IFD0 with one entry 0x0112 (SHORT, count 1), next IFD 0.
    // The value field is the 2-byte orientation followed by the golden's `0000` padding.
    let app1 = if little_endian {
        let value = orientation.to_le_bytes();
        format!(
            "FFE10022457869660000{}{}{}{}{}{:02X}{:02X}0000{}",
            "49492A0008000000", // TIFF header, little-endian
            "0100",             // IFD0 entry count
            "1201",             // tag 0x0112 (LE)
            "0300",             // type SHORT (LE)
            "01000000",         // count 1 (LE)
            value[0],
            value[1],
            "00000000",
        )
    } else {
        format!(
            "FFE100224578696600004D4D002A00000008{}{}{}{}{:04X}{}{}",
            "0001",     // IFD0 entry count
            "0112",     // tag 0x0112
            "0003",     // type SHORT
            "00000001", // count 1
            orientation,
            "0000",     // value padding
            "00000000", // next IFD offset
        )
    };
    jpeg_frame(&hex(&app1), 640, 480)
}

/// The RAF pointer table: magic (16 bytes), 68 zero bytes, then four big-endian u32 fields at 84/88/92/96 (100 bytes total).
fn raf_header(jpeg_offset: u32, jpeg_length: u32, cfa_offset: u32, cfa_length: u32) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.resize(84, 0);
    out.extend(jpeg_offset.to_be_bytes());
    out.extend(jpeg_length.to_be_bytes());
    out.extend(cfa_offset.to_be_bytes());
    out.extend(cfa_length.to_be_bytes());
    assert_eq!(out.len(), 100);
    out
}

/// A RAF with the embedded JPEG at offset 160 (golden `raf_header` + `zeros 60` + JPEG) and no CFA header.
fn synthetic_raf(jpeg: &[u8]) -> Vec<u8> {
    let length = u32::try_from(jpeg.len()).expect("jpeg fits u32");
    let mut out = raf_header(160, length, 0, 0);
    out.extend(std::iter::repeat_n(0u8, 60));
    out.extend_from_slice(jpeg);
    out
}

/// `SyntheticRawBuilder.BuildRaf()`: the golden `synthetic-default-1024x768` file (JPEG 1024x768 at offset 160, length 163).
fn build_raf_default() -> Vec<u8> {
    synthetic_raf(&minimal_jpeg(1024, 768))
}

/// One CFA record of the C# tests: tag, height, width, `size` 4.
fn record(tag: u16, height: u16, width: u16) -> Vec<u8> {
    let mut out = tag.to_be_bytes().to_vec();
    out.extend(4u16.to_be_bytes());
    out.extend(height.to_be_bytes());
    out.extend(width.to_be_bytes());
    out
}

/// One CFA record with an explicit `size`; the data is height, width and 12 zero bytes so every size reads in bounds.
fn record_sized(tag: u16, height: u16, width: u16, size: u16) -> Vec<u8> {
    let mut out = tag.to_be_bytes().to_vec();
    out.extend(size.to_be_bytes());
    out.extend(height.to_be_bytes());
    out.extend(width.to_be_bytes());
    out.extend([0u8; 12]);
    out
}

/// A CFA header: big-endian record count, then the records.
fn cfa(count: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = count.to_be_bytes().to_vec();
    for r in records {
        out.extend_from_slice(r);
    }
    out
}

/// C# `RawExif`-free `RafPreviewRangeTests.Build`: the default synthetic file, a CFA header appended after it, and the four
/// pointer fields patched to (`jpeg_offset`, `jpeg_length`, the appended CFA header).
fn build(jpeg_offset: u32, jpeg_length: u32, records: &[(u16, u16, u16)]) -> Vec<u8> {
    let mut data = build_raf_default();
    let cfa_offset = u32::try_from(data.len()).expect("fits u32");
    let records: Vec<Vec<u8>> = records.iter().map(|&(tag, h, w)| record(tag, h, w)).collect();
    let block = cfa(u32::try_from(records.len()).expect("fits u32"), &records);
    let cfa_length = u32::try_from(block.len()).expect("fits u32");
    data.extend(block);
    data[84..88].copy_from_slice(&jpeg_offset.to_be_bytes());
    data[88..92].copy_from_slice(&jpeg_length.to_be_bytes());
    data[92..96].copy_from_slice(&cfa_offset.to_be_bytes());
    data[96..100].copy_from_slice(&cfa_length.to_be_bytes());
    data
}

/// C# `RafSensorSizeTests.BuildRaf(records)`: the default synthetic file with only the CFA pointer pair patched.
fn build_raf_with_records(records: &[(u16, u16, u16)]) -> Vec<u8> {
    build(160, 163, records)
}

/// C# `RafFile(jpegOffset, jpegLength, cfa, cfaLengthOverride)` of `Cr2ArwRafMutationGapTests`: a zero-filled file of at least
/// 400 bytes; the CFA header (when given) sits at 300 and its pointer may be overridden.
fn raf_file(
    jpeg_offset: u32,
    jpeg_length: u32,
    cfa_block: Option<Vec<u8>>,
    cfa_length_override: Option<u32>,
) -> Vec<u8> {
    let size = 400usize.max(300 + cfa_block.as_ref().map_or(0, Vec::len));
    let mut data = vec![0u8; size];
    data[..16].copy_from_slice(&MAGIC);
    data[84..88].copy_from_slice(&jpeg_offset.to_be_bytes());
    data[88..92].copy_from_slice(&jpeg_length.to_be_bytes());
    if let Some(block) = cfa_block {
        let length = cfa_length_override.unwrap_or_else(|| u32::try_from(block.len()).expect("fits u32"));
        data[92..96].copy_from_slice(&300u32.to_be_bytes());
        data[96..100].copy_from_slice(&length.to_be_bytes());
        data[300..300 + block.len()].copy_from_slice(&block);
    }
    data
}

/// The 100-byte pointer table only (`raf_header(0, 0, 0, 0)`), for the container-size cases.
fn header_only() -> Vec<u8> {
    raf_header(0, 0, 0, 0)
}

// ---------------------------------------------------------------------------------------------------------------------
// Reader helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Reads `bytes` through the in-memory header source with a fresh token.
fn read_bytes(bytes: &[u8]) -> Result<RawContainerInfo, RawHeaderError> {
    let mut source = InMemoryRawHeaderSource::new(bytes.to_vec());
    RafContainerReader::new().read(&mut source, &CancelToken::new())
}

/// `read_bytes` that must succeed.
fn read_ok(bytes: &[u8]) -> RawContainerInfo {
    read_bytes(bytes).unwrap_or_else(|e| panic!("RAF read failed: {e}"))
}

/// `(sensor_width, sensor_height)`, the order the C# tests assert.
fn size_of(info: &RawContainerInfo) -> (i32, i32) {
    (info.sensor_width, info.sensor_height)
}

/// Asserts the result is the `InvalidData` kind (message not compared).
fn assert_invalid_data(result: &Result<RawContainerInfo, RawHeaderError>, context: &str) {
    assert!(
        matches!(result, Err(RawHeaderError::InvalidData { .. })),
        "{context}: expected InvalidData, got {result:?}"
    );
}

/// A source that wraps an in-memory image, reports `length` 4096 and fails its `fail_on`-th read (1-based; 0 never fails)
/// with an I/O failure. Records every `read(offset, count)` call in order.
#[derive(Debug)]
struct FailingSource {
    inner: InMemoryRawHeaderSource,
    length: i64,
    fail_on: usize,
    calls: Vec<(i64, i32)>,
}

impl FailingSource {
    fn new(bytes: &[u8], fail_on: usize) -> Self {
        let mut padded = bytes.to_vec();
        padded.resize(4096, 0);
        Self {
            length: 4096,
            inner: InMemoryRawHeaderSource::new(padded),
            fail_on,
            calls: Vec::new(),
        }
    }
}

impl RawHeaderSource for FailingSource {
    fn length(&self) -> i64 {
        self.length
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.calls.push((offset, count));
        if self.calls.len() == self.fail_on {
            return Err(RawHeaderError::invalid_data_io(
                "disk read failed",
                io::Error::other("simulated disk failure"),
            ));
        }
        self.inner.read(offset, count)
    }
}

/// Deterministic xorshift64 (no `rand` crate). The seed is a constant of the test, so a failure replays.
#[derive(Debug)]
struct XorShift64(u64);

impl XorShift64 {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform-enough index in `0..bound` (`bound > 0`).
    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next_u64() % u64::try_from(bound).expect("bound fits u64")).expect("index fits usize")
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Corpus helpers (native tests). The folder is `PHOTOREVIEW_RAW_CORPUS`, else `<repo>/tests/fixtures/raw-corpus`.
// ---------------------------------------------------------------------------------------------------------------------

fn corpus_dir() -> PathBuf {
    match std::env::var_os("PHOTOREVIEW_RAW_CORPUS") {
        Some(dir) => PathBuf::from(dir),
        None => repo_root().join("tests").join("fixtures").join("raw-corpus"),
    }
}

/// `PHOTOREVIEW_RAW_CORPUS_STRICT` is `1` or `true`: missing corpus material fails instead of skipping.
fn strict_corpus() -> bool {
    matches!(
        std::env::var("PHOTOREVIEW_RAW_CORPUS_STRICT").as_deref(),
        Ok("1" | "true")
    )
}

/// Missing corpus material: the caller returns (skips); with the strict variable set it fails.
fn missing_corpus(reason: &str) {
    assert!(
        !strict_corpus(),
        "RAW corpus missing ({reason}) and PHOTOREVIEW_RAW_CORPUS_STRICT is set"
    );
}

/// The `.raf` files of the corpus whose file name contains `needle` (case-insensitive), sorted. Empty when the folder is absent.
fn corpus_rafs_matching(needle: &str) -> Vec<PathBuf> {
    let needle = needle.to_ascii_lowercase();
    let Ok(entries) = std::fs::read_dir(corpus_dir()) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("raf"))
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.to_ascii_lowercase().contains(&needle))
        })
        .collect();
    found.sort();
    found
}

/// A file-backed header source over the corpus file (the production `SourceRawHeaderSource`, 64 KiB blocks, budgeted).
fn open_corpus(path: &Path) -> SourceRawHeaderSource {
    let file = File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    SourceRawHeaderSource::from_stream(Box::new(file)).unwrap_or_else(|e| panic!("source {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden helpers
// ---------------------------------------------------------------------------------------------------------------------

fn i32_of(value: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(value, key)).unwrap_or_else(|_| panic!("{key} does not fit i32: {value}"))
}

fn kind_of(name: &str) -> EmbeddedPreviewKind {
    match name {
        "Jpeg" => EmbeddedPreviewKind::Jpeg,
        other => panic!("unknown preview kind {other}"),
    }
}

fn color_space_of(name: &str) -> PreviewColorSpace {
    match name {
        "Unknown" => PreviewColorSpace::Unknown,
        "Srgb" => PreviewColorSpace::Srgb,
        "AdobeRgb" => PreviewColorSpace::AdobeRgb,
        other => panic!("unknown color space {other}"),
    }
}

/// The bytes of one golden case: its `parts` in order, then `patches`, then `truncate_to`.
/// Part kinds: `raf_header` (100 bytes), `zeros`, `ascii`, `hex` (with optional `repeat`), `cfa` (`count` defaults to the number
/// of records; a record is `tag`, `size` (default 4) and, only when `size >= 4`, height, width and `size - 4` zero bytes),
/// `pad_to_total` (zero-pads up to that length when shorter).
fn golden_case_bytes(case: &Value) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for part in case["parts"].as_array().expect("parts") {
        if let Some(total) = part.get("pad_to_total") {
            let total = usize::try_from(total.as_u64().expect("pad")).expect("fits");
            if out.len() < total {
                out.resize(total, 0);
            }
        } else if let Some(header) = part.get("raf_header") {
            out.extend(raf_header(
                u32::try_from(i64_of(header, "jpeg_offset")).expect("u32"),
                u32::try_from(i64_of(header, "jpeg_length")).expect("u32"),
                u32::try_from(i64_of(header, "cfa_offset")).expect("u32"),
                u32::try_from(i64_of(header, "cfa_length")).expect("u32"),
            ));
        } else if let Some(count) = part.get("zeros") {
            out.extend(std::iter::repeat_n(
                0u8,
                usize::try_from(count.as_u64().expect("zeros")).expect("fits"),
            ));
        } else if let Some(text) = part.get("ascii") {
            out.extend(text.as_str().expect("ascii").as_bytes());
        } else if let Some(text) = part.get("hex") {
            let bytes = hex(text.as_str().expect("hex"));
            let repeat = part.get("repeat").and_then(Value::as_u64).unwrap_or(1);
            for _ in 0..repeat {
                out.extend_from_slice(&bytes);
            }
        } else if let Some(block) = part.get("cfa") {
            let records = block["records"].as_array().expect("records");
            let count = block
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or(records.len() as u64);
            out.extend(u32::try_from(count).expect("count").to_be_bytes());
            for r in records {
                let size = u16::try_from(r.get("size").and_then(Value::as_u64).unwrap_or(4)).expect("size");
                out.extend(u16::try_from(i64_of(r, "tag")).expect("tag").to_be_bytes());
                out.extend(size.to_be_bytes());
                // Every record occupies 4 + size bytes (the golden `cfa_length` values count the data of size 2 and 3
                // records too): height and width first when size >= 4, then zero bytes up to `size`.
                if size >= 4 {
                    out.extend(u16::try_from(i64_of(r, "height")).expect("h").to_be_bytes());
                    out.extend(u16::try_from(i64_of(r, "width")).expect("w").to_be_bytes());
                    out.extend(std::iter::repeat_n(0u8, usize::from(size) - 4));
                } else {
                    out.extend(std::iter::repeat_n(0u8, usize::from(size)));
                }
            }
        } else {
            panic!("unknown golden part {part}");
        }
    }
    if let Some(patches) = case.get("patches").and_then(Value::as_array) {
        for patch in patches {
            let offset = usize::try_from(i64_of(patch, "offset")).expect("offset");
            let bytes = hex(str_of(patch, "hex"));
            out[offset..offset + bytes.len()].copy_from_slice(&bytes);
        }
    }
    if let Some(total) = case.get("truncate_to").and_then(Value::as_u64) {
        out.truncate(usize::try_from(total).expect("fits"));
    }
    out
}

/// The bytes of the golden `raf-container` case `name` (seed material for the hardening tests).
fn golden_container_bytes(name: &str) -> Vec<u8> {
    let golden = golden::load("raf-container");
    let case = golden
        .cases
        .iter()
        .find(|case| str_of(case, "name") == name)
        .unwrap_or_else(|| panic!("golden case {name} not found"));
    golden_case_bytes(case)
}

/// The three `raf-container` `previews` fields and the `exif_blocks` fields, compared with the golden.
fn assert_previews_match(info: &RawContainerInfo, case: &Value, context: &str) {
    let previews = case["previews"].as_array().expect("previews");
    assert_eq!(info.previews.len(), previews.len(), "{context}: preview count");
    for (actual, expected) in info.previews.iter().zip(previews) {
        assert_eq!(
            i64::from(actual.index),
            i64::from(i32_of(expected, "index")),
            "{context}: index"
        );
        assert_eq!(actual.offset, i64_of(expected, "offset"), "{context}: preview offset");
        assert_eq!(actual.length, i64_of(expected, "length"), "{context}: preview length");
        assert_eq!(actual.kind, kind_of(str_of(expected, "kind")), "{context}: kind");
        assert_eq!(actual.width, i32_of(expected, "width"), "{context}: preview width");
        assert_eq!(actual.height, i32_of(expected, "height"), "{context}: preview height");
        assert_eq!(
            actual.color_space,
            color_space_of(str_of(expected, "color_space")),
            "{context}: color space"
        );
        assert_eq!(
            actual.header_resolved,
            bool_of(expected, "header_resolved"),
            "{context}: header_resolved"
        );
    }
    let blocks = case["exif_blocks"].as_array().expect("exif_blocks");
    assert_eq!(info.exif_blocks.len(), blocks.len(), "{context}: exif block count");
    for (actual, expected) in info.exif_blocks.iter().zip(blocks) {
        assert_eq!(actual.offset, i64_of(expected, "offset"), "{context}: exif offset");
        assert_eq!(actual.length, i64_of(expected, "length"), "{context}: exif length");
        assert_eq!(
            actual.is_tiff_header,
            bool_of(expected, "is_tiff_header"),
            "{context}: is_tiff_header"
        );
        assert_eq!(
            actual.ifd_is_exif,
            bool_of(expected, "ifd_is_exif"),
            "{context}: ifd_is_exif"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Goldens
// ---------------------------------------------------------------------------------------------------------------------

/// `raf-container`: every case is built from its parts, read through `InMemoryRawHeaderSource`, and compared field by field.
#[test]
fn golden_raf_container() {
    let golden = golden::load("raf-container");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let bytes = golden_case_bytes(case);
        assert_eq!(
            bytes.len(),
            usize::try_from(i64_of(case, "total_length")).expect("total_length"),
            "{name}: builder length"
        );

        let mut source = InMemoryRawHeaderSource::new(bytes);
        let token = CancelToken::new();
        if case.get("cancelled").and_then(Value::as_bool).unwrap_or(false) {
            token.cancel();
        }
        let result = RafContainerReader::new().read(&mut source, &token);
        assert_eq!(
            source.total_bytes_read(),
            i64_of(case, "bytes_charged"),
            "{name}: bytes_charged"
        );

        if bool_of(case, "ok") {
            let info = result.unwrap_or_else(|e| panic!("{name}: expected Ok, got {e}"));
            assert_eq!(info.format, RawFormat::Raf, "{name}: format");
            assert_eq!(info.sensor_width, i32_of(case, "sensor_width"), "{name}: sensor_width");
            assert_eq!(
                info.sensor_height,
                i32_of(case, "sensor_height"),
                "{name}: sensor_height"
            );
            assert_eq!(info.orientation, i32_of(case, "orientation"), "{name}: orientation");
            assert_previews_match(&info, case, name);
        } else {
            let expected = &case["error"];
            let err = match result {
                Ok(_) => panic!("{name}: expected an error"),
                Err(err) => err,
            };
            assert!(!err.is_io_failure(), "{name}: io_failure must be false");
            match err {
                RawHeaderError::InvalidData { message, .. } => {
                    if str_of(expected, "origin") == "reader" {
                        assert_eq!(message, str_of(expected, "message"), "{name}: reader message");
                    }
                }
                other => panic!("{name}: expected InvalidData, got {other}"),
            }
        }
    }
}

/// `raf-can-read`: `can_read` over the head bytes and the extension of each case.
#[test]
fn golden_raf_can_read() {
    let golden = golden::load("raf-can-read");
    let reader = RafContainerReader::new();
    for case in &golden.cases {
        let name = str_of(case, "name");
        let head = hex(str_of(case, "head_hex"));
        let extension = str_of(case, "extension");
        assert_eq!(
            reader.can_read(&head, extension),
            bool_of(case, "result"),
            "{name}: can_read"
        );
    }
}

/// `raf-corpus`: the three real files (X-E2S, X-T2, X100V). Skipped when the corpus is absent (`PHOTOREVIEW_RAW_CORPUS`).
#[test]
#[ignore = "native: needs the RAW corpus (tools fetch-raw-samples); degrades like RawCorpus: missing folder or file => return, or panic when PHOTOREVIEW_RAW_CORPUS_STRICT is 1/true"]
fn golden_raf_corpus() {
    let golden = golden::load("raf-corpus");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let path = corpus_dir().join(str_of(case, "file_name"));
        if !path.is_file() {
            missing_corpus(name);
            continue;
        }
        let file_length = std::fs::metadata(&path).expect("metadata").len();
        let mut source = open_corpus(&path);
        let info = RafContainerReader::new()
            .read(&mut source, &CancelToken::new())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(info.format, RawFormat::Raf, "{name}: format");
        assert_eq!(info.previews.len(), 1, "{name}: exactly one preview");
        let preview = &info.previews[0];
        let bytes = std::fs::read(&path).expect("read corpus file");
        let start = usize::try_from(preview.offset).expect("offset");
        assert_eq!(
            &bytes[start..start + 2],
            &[0xFF, 0xD8],
            "{name}: preview starts with SOI"
        );
        if let Some(expected) = case.get("file_length").and_then(Value::as_u64) {
            assert_eq!(file_length, expected, "{name}: file_length");
        }
        if let Some(expected) = case.get("sensor_width").and_then(Value::as_i64) {
            assert_eq!(i64::from(info.sensor_width), expected, "{name}: sensor_width");
        }
        if let Some(expected) = case.get("sensor_height").and_then(Value::as_i64) {
            assert_eq!(i64::from(info.sensor_height), expected, "{name}: sensor_height");
        }
        if let Some(expected) = case.get("orientation").and_then(Value::as_i64) {
            assert_eq!(i64::from(info.orientation), expected, "{name}: orientation");
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported: RafSensorSizeTests (C#: Raw/RafSensorSizeTests.cs)
// ---------------------------------------------------------------------------------------------------------------------

// C#: RafSensorSizeTests.Read_CroppedAndFullRecords_PrefersCroppedSize
#[test]
fn read_cropped_and_full_records_prefers_cropped_size() {
    let info = read_ok(&build_raf_with_records(&[(0x0100, 4032, 6160), (0x0111, 4000, 6000)]));

    assert_eq!((info.sensor_width, info.sensor_height), (6000, 4000));
}

// C#: RafSensorSizeTests.Read_OnlyFullRecord_UsesFullSize
#[test]
fn read_only_full_record_uses_full_size() {
    let info = read_ok(&build_raf_with_records(&[(0x0100, 3296, 4992)]));

    assert_eq!((info.sensor_width, info.sensor_height), (4992, 3296));
}

// C#: RafSensorSizeTests.Read_CroppedLargerThanFull_IsIgnored
#[test]
fn read_cropped_larger_than_full_is_ignored() {
    let info = read_ok(&build_raf_with_records(&[(0x0100, 3296, 4992), (0x0111, 9000, 9000)]));

    assert_eq!((info.sensor_width, info.sensor_height), (4992, 3296));
}

// C#: RafSensorSizeTests.Read_NoCfaHeader_KeepsSizeUnknown
#[test]
fn read_no_cfa_header_keeps_size_unknown() {
    let info = read_ok(&build_raf_default());

    assert_eq!((info.sensor_width, info.sensor_height), (0, 0));
}

// C#: RafSensorSizeTests.Read_CfaRangePastEndOfFile_IsIgnoredWithoutThrowing
#[test]
fn read_cfa_range_past_end_of_file_is_ignored_without_throwing() {
    let mut bytes = build_raf_default();
    bytes[92..96].copy_from_slice(&(u32::MAX - 8).to_be_bytes());
    bytes[96..100].copy_from_slice(&64u32.to_be_bytes());

    let info = read_ok(&bytes);

    assert_eq!((info.sensor_width, info.sensor_height), (0, 0));
}

// C#: RafSensorSizeTests.Read_HugeRecordCount_StopsAtTheEndOfTheHeader
#[test]
fn read_huge_record_count_stops_at_the_end_of_the_header() {
    let mut bytes = build_raf_with_records(&[(0x0100, 3296, 4992)]);
    let cfa_offset = usize::try_from(u32::from_be_bytes(bytes[92..96].try_into().expect("4 bytes"))).expect("offset");
    bytes[cfa_offset..cfa_offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());

    let info = read_ok(&bytes);

    assert_eq!((info.sensor_width, info.sensor_height), (4992, 3296));
}

// C#: RafSensorSizeTests.Corpus_RafReportsRawSizeNotEmbeddedJpegSize (theory row X-T2)
#[test]
#[ignore = "native: needs the RAW corpus (tools fetch-raw-samples); degrades like RawCorpus"]
fn corpus_raf_reports_raw_size_not_embedded_jpeg_size_x_t2() {
    check_corpus_raf_size("x-t2", 6000, 4000);
}

// C#: RafSensorSizeTests.Corpus_RafReportsRawSizeNotEmbeddedJpegSize (theory row X-E2S)
#[test]
#[ignore = "native: needs the RAW corpus (tools fetch-raw-samples); degrades like RawCorpus"]
fn corpus_raf_reports_raw_size_not_embedded_jpeg_size_x_e2s() {
    check_corpus_raf_size("x-e2s", 4896, 3264);
}

// C#: RafSensorSizeTests.Corpus_RafReportsRawSizeNotEmbeddedJpegSize (theory row X100V)
#[test]
#[ignore = "native: needs the RAW corpus (tools fetch-raw-samples); degrades like RawCorpus"]
fn corpus_raf_reports_raw_size_not_embedded_jpeg_size_x100v() {
    check_corpus_raf_size("x100v", 6240, 4160);
}

fn check_corpus_raf_size(camera: &str, width: i32, height: i32) {
    let files = corpus_rafs_matching(camera);
    let Some(path) = files.first() else {
        missing_corpus(camera);
        return;
    };
    let mut source = open_corpus(path);
    let info = RafContainerReader::new()
        .read(&mut source, &CancelToken::new())
        .expect("corpus RAF reads");
    assert_eq!(
        size_of(&info),
        (width, height),
        "{camera}: RAW size, not the embedded JPEG size"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported: RafPreviewRangeTests (C#: Raw/RafPreviewRangeTests.cs)
// ---------------------------------------------------------------------------------------------------------------------

/// `Full` of `RafPreviewRangeTests`: the full readout record (tag 0x0100, height 4032, width 6160).
const FULL: (u16, u16, u16) = (0x0100, 4032, 6160);

// C#: RafPreviewRangeTests.Read_InvalidJpegRange_DropsThePreviewButKeepsTheContainerAndCfaSize
// Theory rows (offset, length): (160,0), (0,100), (50,100), (99,100), (160,0x7FFFFFFF), (u32::MAX,100).
#[test]
fn read_invalid_jpeg_range_drops_the_preview_but_keeps_the_container_and_cfa_size() {
    let rows: [(u32, u32); 6] = [
        (160, 0),
        (0, 100),
        (50, 100),
        (99, 100),
        (160, 0x7FFF_FFFF),
        (u32::MAX, 100),
    ];
    for (offset, length) in rows {
        let info = read_ok(&build(offset, length, &[FULL, (0x0111, 4000, 6000)]));

        assert!(info.previews.is_empty(), "({offset},{length}): no preview");
        assert!(info.exif_blocks.is_empty(), "({offset},{length}): no EXIF block");
        assert_eq!(size_of(&info), (6000, 4000), "({offset},{length}): CFA size kept");
    }
}

// C#: RafPreviewRangeTests.Read_ValidJpegRange_StillYieldsThePreview
#[test]
fn read_valid_jpeg_range_still_yields_the_preview() {
    let jpeg = minimal_jpeg(1024, 768);
    let info = read_ok(&build(160, u32::try_from(jpeg.len()).expect("fits"), &[FULL]));

    assert_eq!(info.previews.len(), 1, "one preview");
    assert_eq!(info.previews[0].offset, 160);
}

// C#: RafPreviewRangeTests.Read_JpegRangeWithoutSoi_DropsThePreviewButKeepsTheContainerAndCfaSize
#[test]
fn read_jpeg_range_without_soi_drops_the_preview_but_keeps_the_container_and_cfa_size() {
    let mut data = build(160, 32, &[FULL, (0x0111, 4000, 6000)]);
    data[160..192].fill(0);

    let info = read_ok(&data);

    assert!(info.previews.is_empty(), "no preview");
    assert!(info.exif_blocks.is_empty(), "no EXIF block");
    assert_eq!(size_of(&info), (6000, 4000));
}

// C#: RafPreviewRangeTests.Read_NoCroppedRecord_UsesRawImageSizeRecord0x0121
#[test]
fn read_no_cropped_record_uses_raw_image_size_record_0x0121() {
    let info = read_ok(&build(160, 10, &[FULL, (0x0121, 4000, 6032)]));

    assert_eq!(size_of(&info), (6032, 4000));
}

// C#: RafPreviewRangeTests.Read_CroppedRecordPresent_BeatsRawImageSize
#[test]
fn read_cropped_record_present_beats_raw_image_size() {
    let info = read_ok(&build(160, 10, &[FULL, (0x0111, 4000, 6000), (0x0121, 4032, 6032)]));

    assert_eq!(size_of(&info), (6000, 4000));
}

// C#: RafPreviewRangeTests.Read_RawImageSizeLargerThanFullReadout_IsIgnored
#[test]
fn read_raw_image_size_larger_than_full_readout_is_ignored() {
    let info = read_ok(&build(160, 10, &[FULL, (0x0121, 9000, 9000)]));

    assert_eq!(size_of(&info), (6160, 4032));
}

// C#: RafPreviewRangeTests.Read_RawImageSizeWidth4284_IsWidenedTo4287LikeLibRaw
#[test]
fn read_raw_image_size_width_4284_is_widened_to_4287_like_libraw() {
    let info = read_ok(&build(160, 10, &[(0x0121, 2856, 4284)]));

    assert_eq!(size_of(&info), (4287, 2856));
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported: Cr2ArwRafMutationGapTests, RAF section (C#: Raw/Tiff/Cr2ArwRafMutationGapTests.cs)
// ---------------------------------------------------------------------------------------------------------------------

// C#: Cr2ArwRafMutationGapTests.RafCanRead_ExactlySixteenByteMagic_IsAcceptedAndFifteenIsNot
#[test]
fn raf_can_read_exactly_sixteen_byte_magic_is_accepted_and_fifteen_is_not() {
    let reader = RafContainerReader::new();

    assert!(reader.can_read(&MAGIC, ".raf"));
    assert!(!reader.can_read(&MAGIC[..15], ".raf"));
}

// C#: Cr2ArwRafMutationGapTests.ReadRaf_WrongMagicInALongEnoughFile_Throws
#[test]
fn read_raf_wrong_magic_in_a_long_enough_file_throws() {
    let mut data = raf_file(0, 0, None, None);
    data[3] ^= 0x40;

    assert_invalid_data(&read_bytes(&data), "wrong magic");
}

// C#: Cr2ArwRafMutationGapTests.ReadRaf_JpegOfExactlyTwoBytesAtTheFirstAllowedOffset_IsAPreview
#[test]
fn read_raf_jpeg_of_exactly_two_bytes_at_the_first_allowed_offset_is_a_preview() {
    let mut data = raf_file(100, 2, None, None);
    data[100] = 0xFF;
    data[101] = 0xD8;

    let info = read_ok(&data);

    assert_eq!(info.previews.len(), 1, "one preview");
    assert_eq!((info.previews[0].offset, info.previews[0].length), (100, 2));
}

// C#: Cr2ArwRafMutationGapTests.ReadRaf_JpegRangeOutsideTheAllowedWindow_IsNotAPreview
// Theory rows (offset, length): (99, 2) starts inside the pointer table; (100, 1) too short for SOI.
#[test]
fn read_raf_jpeg_range_outside_the_allowed_window_is_not_a_preview() {
    for (offset, length) in [(99u32, 2u32), (100, 1)] {
        let mut data = raf_file(offset, length, None, None);
        data[offset as usize] = 0xFF;
        data[offset as usize + 1] = 0xD8;

        let info = read_ok(&data);

        assert!(info.previews.is_empty(), "({offset},{length}) is not a preview");
    }
}

// C#: Cr2ArwRafMutationGapTests.ReadRaf_CfaRangeBeyondTheEndOfTheFile_ReportsNoSizeWithoutThrowing
#[test]
fn read_raf_cfa_range_beyond_the_end_of_the_file_reports_no_size_without_throwing() {
    let data = raf_file(0, 0, Some(cfa(1, &[record(0x0100, 3000, 4000)])), Some(1000));

    let info = read_ok(&data);

    assert_eq!(size_of(&info), (0, 0));
}

// C#: Cr2ArwRafMutationGapTests.ReadRaf_RecordsBeyondTheDeclaredCount_AreNotRead
#[test]
fn read_raf_records_beyond_the_declared_count_are_not_read() {
    // Count 1: the full size is read; the second record (a valid cropped size) is beyond the count.
    let data = raf_file(
        0,
        0,
        Some(cfa(1, &[record(0x0100, 3000, 4000), record(0x0111, 2990, 3990)])),
        None,
    );

    let info = read_ok(&data);

    assert_eq!(size_of(&info), (4000, 3000));
}

// C#: Cr2ArwRafMutationGapTests.ReadRaf_RecordWhoseDataIsCutOffByTheCfaLength_EndsTheWalk
#[test]
fn read_raf_record_whose_data_is_cut_off_by_the_cfa_length_ends_the_walk() {
    // CFA length 10: count + one record header + only 2 of the 4 data bytes.
    let data = raf_file(0, 0, Some(cfa(1, &[record(0x0100, 3000, 4000)])), Some(10));

    let info = read_ok(&data);

    assert_eq!(size_of(&info), (0, 0));
}

// C#: Cr2ArwRafMutationGapTests.ReadRaf_RecordWithAnUnknownTag_IsIgnored
#[test]
fn read_raf_record_with_an_unknown_tag_is_ignored() {
    let data = raf_file(0, 0, Some(cfa(1, &[record(0x0200, 100, 100)])), None);

    let info = read_ok(&data);

    assert_eq!(size_of(&info), (0, 0));
}

// C#: Cr2ArwRafMutationGapTests.ReadRaf_FullSizeRecordWithAZeroSide_IsIgnored
// Theory rows (width, height): (0, 3000) and (4000, 0).
#[test]
fn read_raf_full_size_record_with_a_zero_side_is_ignored() {
    for (width, height) in [(0u16, 3000u16), (4000, 0)] {
        let data = raf_file(0, 0, Some(cfa(1, &[record(0x0100, height, width)])), None);

        let info = read_ok(&data);

        assert_eq!(size_of(&info), (0, 0), "full record {width}x{height} is ignored");
    }
}

// C#: Cr2ArwRafMutationGapTests.ReadRaf_CroppedSize_IsUsedOnlyWhenItFitsInsideTheFullReadout
// Theory rows (croppedWidth, croppedHeight, expectedWidth, expectedHeight).
#[test]
fn read_raf_cropped_size_is_used_only_when_it_fits_inside_the_full_readout() {
    let rows: [(u16, u16, i32, i32); 4] = [
        (3990, 3000, 3990, 3000), // cropped height equal to the full height still fits
        (4000, 2990, 4000, 2990), // cropped width equal to the full width still fits
        (3990, 3100, 4000, 3000), // taller than the full readout: ignored
        (4100, 2990, 4000, 3000), // wider than the full readout: ignored
    ];
    for (cropped_width, cropped_height, expected_width, expected_height) in rows {
        let data = raf_file(
            0,
            0,
            Some(cfa(
                2,
                &[
                    record(0x0100, 3000, 4000),
                    record(0x0111, cropped_height, cropped_width),
                ],
            )),
            None,
        );

        let info = read_ok(&data);

        assert_eq!(
            size_of(&info),
            (expected_width, expected_height),
            "cropped {cropped_width}x{cropped_height}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported: Cr3AndRafReaderTests, RAF half (C#: Raw/Cr3AndRafReaderTests.cs)
// ---------------------------------------------------------------------------------------------------------------------

// C#: Cr3AndRafReaderTests.RafReader_Synthetic_CanReadPreview (the registry half goes to W5-11: here `can_read` directly)
#[test]
fn raf_reader_synthetic_can_read_preview() {
    let jpeg = minimal_jpeg(640, 480);
    let data = synthetic_raf(&jpeg);
    let reader = RafContainerReader::new();
    assert!(reader.can_read(&data[..data.len().min(64)], ".raf"));

    let info = read_ok(&data);

    assert_eq!(info.format, RawFormat::Raf);
    assert_eq!(info.previews.len(), 1, "one preview");
    assert_eq!(info.previews[0].offset, 160);
    assert_eq!(info.previews[0].length, i64::try_from(jpeg.len()).expect("fits"));
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported: RawOrientationTests, RAF half (C#: Raw/RawOrientationTests.cs)
// ---------------------------------------------------------------------------------------------------------------------

// C#: RawOrientationTests.RafReader_EmbeddedJpegExifOrientation_IsReported (theory rows 3, 6, 8; the JPEG is built in the test)
#[test]
fn raf_reader_embedded_jpeg_exif_orientation_is_reported() {
    for orientation in [1u16, 3, 6, 8] {
        let raf = synthetic_raf(&jpeg_with_exif(orientation, false));

        assert_eq!(
            read_ok(&raf).orientation,
            i32::from(orientation),
            "orientation {orientation}"
        );
    }
}

// C#: RawOrientationTests.RafReader_JpegWithoutExif_DefaultsToNormalOrientation
#[test]
fn raf_reader_jpeg_without_exif_defaults_to_normal_orientation() {
    let raf = synthetic_raf(&minimal_jpeg(640, 480));

    assert_eq!(read_ok(&raf).orientation, 1);
}

/// C# `RawOrientationPatcher.TryWriteOrientation`, RAF branch (non-CR3, no TIFF-at-zero try): finds the APP1 Exif of the
/// embedded JPEG and rewrites its IFD0 Orientation.
fn try_write_raf_orientation(file: &mut [u8], info: &RawContainerInfo, orientation: u16) -> bool {
    let Some(preview) = info.previews.first() else {
        return false;
    };
    let Ok(jpeg_start) = usize::try_from(preview.offset) else {
        return false;
    };
    let mut offset = jpeg_start + 2;
    while offset + 4 <= file.len() && file[offset] == 0xFF {
        let marker = file[offset + 1];
        let length = usize::from(u16::from_be_bytes([file[offset + 2], file[offset + 3]]));
        if marker == 0xE1 && length >= 8 && offset + 10 <= file.len() && file[offset + 4..offset + 10] == *b"Exif\0\0" {
            return try_write_tiff_orientation(file, offset + 10, orientation);
        }
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        offset += 2 + length;
    }
    false
}

/// C# `RawOrientationPatcher.TryWriteTiffOrientation`: the IFD0 0x0112 entry, which must be a SHORT, gets the value.
fn try_write_tiff_orientation(file: &mut [u8], tiff_start: usize, orientation: u16) -> bool {
    let Some((entry, little)) = find_ifd0_entry(file, tiff_start, 0x0112) else {
        return false;
    };
    if read_u16(file, entry + 2, little) != 3 {
        return false;
    }
    let value = if little {
        orientation.to_le_bytes()
    } else {
        orientation.to_be_bytes()
    };
    file[entry + 8..entry + 10].copy_from_slice(&value);
    true
}

fn find_ifd0_entry(file: &[u8], tiff_start: usize, tag: u16) -> Option<(usize, bool)> {
    if tiff_start + 8 > file.len() {
        return None;
    }
    let little = file[tiff_start] == b'I';
    let ifd0 = tiff_start + usize::try_from(read_u32(file, tiff_start + 4, little)).ok()?;
    if ifd0 + 2 > file.len() {
        return None;
    }
    let count = usize::from(read_u16(file, ifd0, little));
    for i in 0..count {
        let entry = ifd0 + 2 + i * 12;
        if entry + 12 > file.len() {
            break;
        }
        if read_u16(file, entry, little) == tag {
            return Some((entry, little));
        }
    }
    None
}

fn read_u16(file: &[u8], at: usize, little: bool) -> u16 {
    let bytes = [file[at], file[at + 1]];
    if little {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    }
}

fn read_u32(file: &[u8], at: usize, little: bool) -> u32 {
    let bytes = [file[at], file[at + 1], file[at + 2], file[at + 3]];
    if little {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    }
}

// C#: RawOrientationTests.RafAndRw2CorpusFile_ContainerOrientationMatchesEmbeddedJpegExif (RAF half; the WIC oracle half is W5-12)
#[test]
#[ignore = "native: needs the RAW corpus (tools fetch-raw-samples); degrades like RawCorpus"]
fn raf_and_rw2_corpus_file_container_orientation_matches_embedded_jpeg_exif() {
    let files = corpus_rafs_matching("");
    if files.is_empty() {
        missing_corpus("RAF samples");
        return;
    }
    for path in files {
        let bytes = std::fs::read(&path).expect("read corpus file");
        // raf-corpus golden: every corpus sample is a landscape shot, orientation 1.
        assert_eq!(read_ok(&bytes).orientation, 1, "{}", path.display());
    }
}

// C#: RawOrientationTests.RafAndRw2CorpusFile_RewrittenOrientation_IsReportedByContainerReader (RAF half)
#[test]
#[ignore = "native: needs the RAW corpus (tools fetch-raw-samples); degrades like RawCorpus"]
fn raf_and_rw2_corpus_file_rewritten_orientation_is_reported_by_container_reader() {
    let files = corpus_rafs_matching("");
    if files.is_empty() {
        missing_corpus("RAF samples");
        return;
    }
    for path in files {
        let original = std::fs::read(&path).expect("read corpus file");
        let original_info = read_ok(&original);
        for orientation in [6u16, 8, 3] {
            let mut rewritten = original.clone();
            assert!(
                try_write_raf_orientation(&mut rewritten, &original_info, orientation),
                "{}: no orientation to rewrite",
                path.display()
            );
            assert_eq!(
                read_ok(&rewritten).orientation,
                i32::from(orientation),
                "{}",
                path.display()
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported: Cr3AndRafCorpusTests, RAF half (C#: Raw/Cr3AndRafCorpusTests.cs)
// ---------------------------------------------------------------------------------------------------------------------

// C#: Cr3AndRafCorpusTests.AllCorpusCr3AndRafSamples_CanBeParsed (RAF half; the CR3 half is W5-10)
#[test]
#[ignore = "native: needs the RAW corpus (tools fetch-raw-samples); degrades like RawCorpus"]
fn all_corpus_raf_samples_can_be_parsed() {
    let files = corpus_rafs_matching("");
    if files.is_empty() {
        missing_corpus("RAF samples");
        return;
    }
    let reader = RafContainerReader::new();
    for path in &files {
        let head = std::fs::read(path).expect("read corpus file");
        assert!(
            reader.can_read(&head[..head.len().min(64)], ".raf"),
            "{}: can_read",
            path.display()
        );
        let mut source = open_corpus(path);
        let info = reader
            .read(&mut source, &CancelToken::new())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(!info.previews.is_empty(), "{}: no previews found", path.display());
        assert!(info.previews[0].length > 0, "{}: empty preview", path.display());
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported: RawContainerFuzzTests.Fuzz_RafContainerReader_Robust and the RAF row of
// BinaryReaderFuzzTests.Fuzz_RawContainerReaders_FailOnlyCleanly (C#: Robustness/*.cs). A self-contained twin of the shared
// fuzz harness: deterministic, no `rand`, bounded.
// ---------------------------------------------------------------------------------------------------------------------

const FUZZ_ITERATIONS: usize = 20_000;
const HEADER_BUDGET_BYTES: i64 = 8 * 1024 * 1024;

/// Offset of the CFA count (the CFA header offset of the seed, when it points inside the seed), else `None`.
fn cfa_count_offset(seed: &[u8]) -> Option<usize> {
    if seed.len() < 100 {
        return None;
    }
    let offset = usize::try_from(u32::from_be_bytes([seed[92], seed[93], seed[94], seed[95]])).ok()?;
    (offset + 4 <= seed.len()).then_some(offset)
}

/// One mutation of `seed`: random bit flips, a random byte, or a 4-byte write of an interesting value at offsets 80..100 or
/// at the CFA count.
fn mutate(rng: &mut XorShift64, seed: &[u8]) -> Vec<u8> {
    let mut data = seed.to_vec();
    match rng.below(3) {
        0 => {
            for _ in 0..=rng.below(4) {
                let index = rng.below(data.len());
                data[index] ^= 1 << rng.below(8);
            }
        }
        1 => {
            let values = [
                0u32,
                u32::MAX,
                0x7FFF_FFFF,
                100,
                u32::try_from(seed.len()).unwrap_or(u32::MAX),
                u32::try_from(seed.len() + 1).unwrap_or(u32::MAX),
            ];
            let value = values[rng.below(values.len())];
            let at = match cfa_count_offset(seed) {
                Some(count_at) if rng.below(2) == 0 => count_at,
                _ => 80 + rng.below(20),
            };
            if at + 4 <= data.len() {
                data[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
        }
        _ => {
            let index = rng.below(data.len());
            data[index] = u8::try_from(rng.next_u64() & 0xFF).expect("masked");
        }
    }
    data
}

/// The outcome contract of the hostile-input tests: `Ok` with sane fields, or `InvalidData`; never another error, never a
/// read over the header budget.
fn check_hostile_outcome(bytes: &[u8], result: &Result<RawContainerInfo, RawHeaderError>, charged: i64, context: &str) {
    assert!(charged <= HEADER_BUDGET_BYTES, "{context}: charged {charged} bytes");
    match result {
        Ok(info) => {
            assert!(
                (1..=8).contains(&info.orientation),
                "{context}: orientation {}",
                info.orientation
            );
            assert!(
                info.sensor_width >= 0 && info.sensor_height >= 0,
                "{context}: negative sensor size"
            );
            assert!(info.previews.len() <= 1, "{context}: more than one preview");
            assert!(info.exif_blocks.len() <= 1, "{context}: more than one EXIF block");
            let file_len = i64::try_from(bytes.len()).expect("fits");
            for preview in &info.previews {
                assert!(
                    preview.offset >= 0 && preview.length > 0 && preview.length <= file_len - preview.offset,
                    "{context}: preview {}+{} escapes the {file_len}-byte file",
                    preview.offset,
                    preview.length
                );
            }
        }
        Err(RawHeaderError::InvalidData { .. }) => {}
        Err(other) => panic!("{context}: unexpected error kind: {other}"),
    }
}

// C#: RawContainerFuzzTests.Fuzz_RafContainerReader_Robust
#[test]
fn fuzz_raf_container_reader_robust() {
    let seeds = [
        synthetic_raf(&minimal_jpeg(640, 480)),
        golden_container_bytes("preview-and-cfa-together"),
    ];
    let reader = RafContainerReader::new();
    let mut rng = XorShift64(0x9E37_79B9_7F4A_7C15);
    let started = std::time::Instant::now();

    for iteration in 0..FUZZ_ITERATIONS {
        let seed = &seeds[iteration % seeds.len()];
        let bytes = mutate(&mut rng, seed);
        let head = &bytes[..bytes.len().min(64)];
        let expected_can_read = bytes.len() >= 16 && bytes[..16] == MAGIC;
        assert_eq!(
            reader.can_read(head, ".raf"),
            expected_can_read,
            "iteration {iteration}: can_read"
        );

        let mut source = InMemoryRawHeaderSource::new(bytes.clone());
        let result = reader.read(&mut source, &CancelToken::new());
        check_hostile_outcome(
            &bytes,
            &result,
            source.total_bytes_read(),
            &format!("iteration {iteration}"),
        );
    }

    assert!(
        started.elapsed().as_secs() < 60,
        "fuzz run took {:?}",
        started.elapsed()
    );
}

// C#: BinaryReaderFuzzTests.Fuzz_RawContainerReaders_FailOnlyCleanly (RAF row): every truncation of two seeds fails only cleanly.
#[test]
fn fuzz_raf_container_reader_fails_only_cleanly_on_every_truncation() {
    let seeds = [build_raf_default(), golden_container_bytes("preview-and-cfa-together")];
    let reader = RafContainerReader::new();
    for seed in &seeds {
        for length in 0..=seed.len() {
            let bytes = &seed[..length];
            let mut source = InMemoryRawHeaderSource::new(bytes.to_vec());
            let result = reader.read(&mut source, &CancelToken::new());
            check_hostile_outcome(
                bytes,
                &result,
                source.total_bytes_read(),
                &format!("truncated to {length}"),
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// New tests that pin what C# only implies (contract W5-09, "Tests to write" item 4)
// ---------------------------------------------------------------------------------------------------------------------

// C#: (implied by the C# reader never observing its CancellationToken) read_ignores_a_cancelled_token
#[test]
fn read_ignores_a_cancelled_token() {
    let token = CancelToken::new();
    token.cancel();
    let mut source = InMemoryRawHeaderSource::new(build(160, 163, &[FULL]));

    let info = RafContainerReader::new()
        .read(&mut source, &token)
        .expect("a cancelled token still reads normally");

    assert_eq!(info.format, RawFormat::Raf);
    assert_eq!(size_of(&info), (6160, 4032));
}

// C#: (implied) read_never_returns_canceled: the reader never produces RawHeaderError::Canceled
#[test]
fn read_never_returns_canceled() {
    let token = CancelToken::new();
    token.cancel();
    let mut source = InMemoryRawHeaderSource::new(build_raf_default());

    let result = RafContainerReader::new().read(&mut source, &token);

    assert!(!matches!(result, Err(RawHeaderError::Canceled)), "never Canceled");
    assert!(result.is_ok(), "valid file reads");
}

// C#: (implied) source_error_propagates_unchanged: a failing read N (1 header, 2 SOI, 3 EXIF, 4 CFA pointer, 5 CFA bytes) is the
// same I/O error; the reads before it happened in the order of semantics 9.
#[test]
fn source_error_propagates_unchanged() {
    let default_calls = [(0i64, 92i32), (160, 2), (160, 163), (92, 8)];
    let with_cfa = build(160, 163, &[FULL]);
    let cfa_length = i32::try_from(with_cfa.len() - 323).expect("fits");

    for fail_on in 1..=4usize {
        let mut source = FailingSource::new(&build_raf_default(), fail_on);
        let result = RafContainerReader::new().read(&mut source, &CancelToken::new());

        let err = result.expect_err("the failing read is an error");
        assert!(err.is_io_failure(), "fail_on {fail_on}: is_io_failure stays true");
        assert_eq!(
            source.calls,
            default_calls[..fail_on],
            "fail_on {fail_on}: read sequence"
        );
    }

    let mut source = FailingSource::new(&with_cfa, 5);
    let result = RafContainerReader::new().read(&mut source, &CancelToken::new());
    let err = result.expect_err("the CFA read fails");
    assert!(err.is_io_failure(), "CFA read: is_io_failure stays true");
    assert_eq!(source.calls.len(), 5, "CFA read is the fifth call");
    assert_eq!(source.calls[4], (323, cfa_length), "CFA read covers the CFA header");

    // No preview and no CFA size: exactly the header read and the CFA pointer read, nothing else.
    let mut source = FailingSource::new(&header_only(), 0);
    let info = RafContainerReader::new()
        .read(&mut source, &CancelToken::new())
        .expect("a 100-byte container with no preview reads");
    assert!(info.previews.is_empty());
    assert_eq!(source.calls, [(0, 92), (92, 8)], "no-preview read sequence");
}

// C#: (implied) container_with_92_to_99_bytes_fails_on_the_cfa_pointer_read (kind only)
#[test]
fn container_with_92_to_99_bytes_fails_on_the_cfa_pointer_read() {
    let header = header_only();
    for length in 92..=99usize {
        let result = read_bytes(&header[..length]);

        assert_invalid_data(&result, &format!("{length} bytes"));
    }
}

// C#: (implied) container_with_100_bytes_is_ok_with_no_preview_and_no_size
#[test]
fn container_with_100_bytes_is_ok_with_no_preview_and_no_size() {
    let info = read_ok(&header_only());

    assert!(info.previews.is_empty(), "no preview");
    assert!(info.exif_blocks.is_empty(), "no EXIF block");
    assert_eq!(size_of(&info), (0, 0));
    assert_eq!(info.orientation, 1);
}

// C#: (implied) exif_window_is_capped_at_131072_bytes: the preview is the whole JPEG, the EXIF block is capped
#[test]
fn exif_window_is_capped_at_131072_bytes() {
    for length in [131_071usize, 131_072, 131_073, 200_000] {
        let mut jpeg = vec![0xFFu8, 0xD8];
        jpeg.resize(length, 0x11);
        let info = read_ok(&synthetic_raf(&jpeg));

        let preview = info.previews.first().expect("one preview");
        assert_eq!(
            preview.length,
            i64::try_from(length).expect("fits"),
            "preview length is the whole JPEG"
        );
        let block = info.exif_blocks.first().expect("one EXIF block");
        assert_eq!(block.offset, 160);
        assert_eq!(
            block.length,
            i64::try_from(length.min(131_072)).expect("fits"),
            "EXIF window {length}"
        );
    }
}

// C#: (implied) orientation_values_one_to_eight_and_invalid_ones: the EXIF matrix, big- and little-endian
#[test]
fn orientation_values_one_to_eight_and_invalid_ones() {
    for orientation in 1u16..=8 {
        let raf = synthetic_raf(&jpeg_with_exif(orientation, false));
        assert_eq!(
            read_ok(&raf).orientation,
            i32::from(orientation),
            "big-endian {orientation}"
        );
    }
    for orientation in [3u16, 6, 8] {
        let raf = synthetic_raf(&jpeg_with_exif(orientation, true));
        assert_eq!(
            read_ok(&raf).orientation,
            i32::from(orientation),
            "little-endian {orientation}"
        );
    }
    for orientation in [0u16, 9, 255, 65535] {
        for little_endian in [false, true] {
            let raf = synthetic_raf(&jpeg_with_exif(orientation, little_endian));
            assert_eq!(
                read_ok(&raf).orientation,
                1,
                "invalid {orientation} (little {little_endian}) defaults to 1"
            );
        }
    }
}

// C#: (implied) orientation_zero_and_nine_default_to_one: the direct twin of the two invalid values
#[test]
fn orientation_zero_and_nine_default_to_one() {
    assert_eq!(read_ok(&synthetic_raf(&jpeg_with_exif(0, false))).orientation, 1);
    assert_eq!(read_ok(&synthetic_raf(&jpeg_with_exif(9, false))).orientation, 1);
}

// C#: (implied) parse_cfa_records_is_total: no prefix, record size or count panics; every result is a sane size
#[test]
fn parse_cfa_records_is_total() {
    let valid = cfa(
        3,
        &[
            record(0x0100, 3000, 4000),
            record(0x0111, 2990, 3990),
            record(0x0121, 2856, 4284),
        ],
    );

    let check = |block: &[u8], context: &str| {
        let sizes = parse_cfa_records(block);
        let chosen = choose_sensor_size(&sizes);
        assert!(chosen.0 >= 0 && chosen.1 >= 0, "{context}: negative size {chosen:?}");
        let candidates = [sizes.full, sizes.cropped, sizes.image_size, (0, 0)];
        assert!(
            candidates.contains(&chosen),
            "{context}: {chosen:?} is not one of the parsed sizes"
        );
    };

    for length in 0..=40usize {
        check(&valid[..length.min(valid.len())], &format!("prefix {length}"));
    }
    for size in 0..=12u16 {
        check(
            &cfa(1, &[record_sized(0x0100, 3000, 4000, size)]),
            &format!("size {size}"),
        );
    }
    for count in [0u32, 1, 255, 256, 257, u32::MAX] {
        check(
            &cfa(count, &[record(0x0100, 3000, 4000), record(0x0111, 2990, 3990)]),
            &format!("count {count}"),
        );
    }
    assert_eq!(MAX_CFA_RECORDS, 256, "the record walk is capped at 256 records");
}

// C#: (implied) choose_sensor_size_table: the fit rules on plain CfaSizes values
#[test]
fn choose_sensor_size_table() {
    let rows: [(CfaSizes, (i32, i32), &str); 7] = [
        (
            CfaSizes {
                full: (0, 0),
                cropped: (9000, 9000),
                image_size: (0, 0),
            },
            (9000, 9000),
            "no full: cropped always fits",
        ),
        (
            CfaSizes {
                full: (4000, 3000),
                cropped: (4000, 3000),
                image_size: (0, 0),
            },
            (4000, 3000),
            "equal cropped fits",
        ),
        (
            CfaSizes {
                full: (4000, 3000),
                cropped: (3990, 2990),
                image_size: (0, 0),
            },
            (3990, 2990),
            "smaller cropped fits",
        ),
        (
            CfaSizes {
                full: (4000, 3000),
                cropped: (4100, 2990),
                image_size: (3990, 2990),
            },
            (3990, 2990),
            "cropped too wide, image size fits",
        ),
        (
            CfaSizes {
                full: (4000, 3000),
                cropped: (0, 0),
                image_size: (3990, 2990),
            },
            (3990, 2990),
            "no cropped: image size",
        ),
        (
            CfaSizes {
                full: (4000, 3000),
                cropped: (4100, 3100),
                image_size: (4100, 3100),
            },
            (4000, 3000),
            "both invalid: full",
        ),
        (
            CfaSizes {
                full: (0, 0),
                cropped: (0, 0),
                image_size: (0, 0),
            },
            (0, 0),
            "all absent stays unknown",
        ),
    ];
    for (sizes, expected, context) in rows {
        assert_eq!(choose_sensor_size(&sizes), expected, "{context}");
    }
}

// C#: (implied) magic_is_exactly_sixteen_bytes_with_trailing_space
#[test]
fn magic_is_exactly_sixteen_bytes_with_trailing_space() {
    assert_eq!(MAGIC.len(), 16);
    assert_eq!(MAGIC[15], 0x20);
    assert_eq!(&MAGIC, b"FUJIFILMCCD-RAW ");
}
