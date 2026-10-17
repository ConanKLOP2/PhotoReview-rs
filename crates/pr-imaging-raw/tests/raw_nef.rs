//! Red tests for W5-04 (`readers/nef.rs`): `NefContainerReader` (port of `PhotoReview.Imaging.Raw/Tiff/NefContainerReader.cs`,
//! C# baseline d92d6aae).
//!
//! Ports the NEF rows of: `Raw/Tiff/NefOrfRw2MutationGapTests.cs`, `Raw/Tiff/MakerNoteGapTests.cs` (Nikon part),
//! `Raw/Tiff/TiffReaderRegressionTests.cs`, `Raw/Tiff/TiffSensorSizeTests.cs`, `Raw/Tiff/TiffFamilyReaderTests.cs`,
//! `Raw/Tiff/TiffPreviewHardeningTests.cs`, `Raw/Tiff/TiffStripPreviewTests.cs`, `Raw/Tiff/TiffSubIfdTypeTests.cs`,
//! `Raw/Tiff/TiffExifBlockPlacementTests.cs`, `Raw/Tiff/TiffCorpusTests.cs` (corpus-gated), `Robustness/RawContainerFuzzTests.cs`
//! (`Fuzz_NefContainerReader_Robust`), `Robustness/BinaryReaderFuzzTests.cs` (NEF and NEF-BE targets) and the golden
//! `tests/golden/nef-container.json` (246 cases).
//!
//! Every synthetic-buffer test is checked against its golden case (the same bytes, from the C# side): the golden case
//! pins the full output (format, sensor size, orientation, previews, EXIF blocks, `bytes_read`), so a named test asserts at
//! least the property its C# test asserts. The golden patch DSL (`header`, `ifd`, `u32`, `jpeg`, `budget_jpeg`, `hex`,
//! `fit`, `clip`) is materialised here exactly as `tools/oracle-cs/Commands/W5-04.cs` does (`nef_bytes`).
//!
//! Stub bodies are `todo!()`: every test that reaches `NefContainerReader::can_read` or `read` fails until the porter fills
//! the stub in. Corpus tests skip when the RAW corpus is absent (P-30); `PHOTOREVIEW_RS_RAW_CORPUS_STRICT=1` makes an
//! absent corpus a failure. Native tests: none (this unit is pure bytes).

// C# unchecked casts of golden numbers (16/32-bit fields, `(ushort)`, `(uint)`, `(byte)`): the truncation is the point.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use pr_core::sync::CancelToken;
use pr_imaging_raw::raw_exif::try_read_exif;
use pr_imaging_raw::readers::nef::NefContainerReader;
use pr_imaging_raw::{InMemoryRawHeaderSource, RawContainerReader, RawHeaderError, RawHeaderSource};
use pr_test_support::golden;
use pr_test_support::raw::{TiffBytes, at, build_tiff, create_minimal_jpeg, from_hex, long, mutants};
use serde_json::{Map, Value, json};

const GOLDEN: &str = "nef-container";

/// A bounded test source with deterministic read hooks for cancellation and I/O-only mutation killers.
struct HookedRawSource {
    bytes: Vec<u8>,
    reads: Vec<(i64, i32)>,
    fail_on: Option<(i64, i32, usize)>,
    matching_reads: usize,
    cancel_on: Option<(i64, i32, CancelToken)>,
}

impl HookedRawSource {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            reads: Vec::new(),
            fail_on: None,
            matching_reads: 0,
            cancel_on: None,
        }
    }

    fn fail_on_nth_read(mut self, offset: i64, count: i32, nth: usize) -> Self {
        self.fail_on = Some((offset, count, nth));
        self
    }

    fn cancel_on_read(mut self, offset: i64, count: i32, token: CancelToken) -> Self {
        self.cancel_on = Some((offset, count, token));
        self
    }
}

impl RawHeaderSource for HookedRawSource {
    fn length(&self) -> i64 {
        i64::try_from(self.bytes.len()).unwrap_or(i64::MAX)
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.reads.push((offset, count));
        if let Some((fail_offset, fail_count, nth)) = self.fail_on
            && (offset, count) == (fail_offset, fail_count)
        {
            self.matching_reads += 1;
            if self.matching_reads == nth {
                return Err(RawHeaderError::invalid_data_io(
                    "scripted read failure",
                    io::Error::other("scripted read failure"),
                ));
            }
        }
        if let Some((cancel_offset, cancel_count, token)) = &self.cancel_on
            && (offset, count) == (*cancel_offset, *cancel_count)
        {
            token.cancel();
        }
        let start = usize::try_from(offset).map_err(|_| RawHeaderError::invalid_data("negative test read offset"))?;
        let len = usize::try_from(count).map_err(|_| RawHeaderError::invalid_data("negative test read count"))?;
        let end = start
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| RawHeaderError::invalid_data("test read out of range"))?;
        Ok(&self.bytes[start..end])
    }
}

fn raw_with_two_jpeg_previews(raw_width: u32, raw_height: u32, first: (u32, u32), second: (u32, u32)) -> Vec<u8> {
    let first_jpeg = create_minimal_jpeg(first.0 as i32, first.1 as i32);
    let second_jpeg = create_minimal_jpeg(second.0 as i32, second.1 as i32);
    let first_offset = 512u32;
    let second_offset = first_offset + u32::try_from(first_jpeg.len()).expect("fixture JPEG length fits u32") + 16;
    let second_end = usize::try_from(second_offset).expect("fixture offset fits usize") + second_jpeg.len();
    let tiff = TiffBytes::new(true, second_end)
        .header(8)
        .ifd(
            8,
            0,
            &[long(256, raw_width), long(257, raw_height), at(0x014A, 4, 2, 64)],
        )
        .u32(64, 128)
        .u32(68, 200)
        .ifd(
            128,
            0,
            &[
                long(256, first.0),
                long(257, first.1),
                long(0x0201, first_offset),
                long(
                    0x0202,
                    u32::try_from(first_jpeg.len()).expect("fixture JPEG length fits u32"),
                ),
            ],
        )
        .ifd(
            200,
            0,
            &[
                long(256, second.0),
                long(257, second.1),
                long(0x0201, second_offset),
                long(
                    0x0202,
                    u32::try_from(second_jpeg.len()).expect("fixture JPEG length fits u32"),
                ),
            ],
        )
        .put(
            usize::try_from(first_offset).expect("fixture offset fits usize"),
            &first_jpeg,
        )
        .put(
            usize::try_from(second_offset).expect("fixture offset fits usize"),
            &second_jpeg,
        );
    // Keep the test fixture's complete byte vector alive only until it is returned.
    tiff.into_vec()
}

fn raw_with_unknown_jpeg_preview(raw_width: u32, raw_height: u32) -> (Vec<u8>, i64) {
    let jpeg = create_minimal_jpeg(900, 900);
    let jpeg_offset = 512u32;
    let bytes = TiffBytes::new(true, jpeg_offset as usize + jpeg.len())
        .header(8)
        .ifd(8, 0, &[long(256, raw_width), long(257, raw_height), long(0x014A, 128)])
        .u32(64, 128)
        .ifd(
            128,
            0,
            &[
                long(256, 0),
                long(257, 0),
                long(0x0201, jpeg_offset),
                long(0x0202, u32::try_from(jpeg.len()).expect("fixture JPEG length fits u32")),
            ],
        )
        .put(jpeg_offset as usize, &jpeg)
        .into_vec();
    (bytes, i64::from(jpeg_offset))
}

// ---------------------------------------------------------------- golden DSL (tools/oracle-cs/Commands/W5-04.cs)

/// Golden number: a JSON integer, or a decimal / `0x..` string (C# `W504Num`).
fn num(node: &Value) -> i64 {
    if let Some(text) = node.as_str() {
        return match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            Some(hex) => i64::from_str_radix(hex, 16).expect("hex golden number"),
            None => text.parse().expect("decimal golden number"),
        };
    }
    node.as_i64().expect("integer golden node")
}

/// The data buffer of one golden case: zero filled (or `fit`), patches applied in order, byte order from `le`.
struct Buf {
    le: bool,
    fit: bool,
    bytes: Vec<u8>,
}

impl Buf {
    /// C# `Put`: a patch past the end grows the buffer for `fit`, otherwise it is a generator bug; `clip` cuts it instead.
    fn put(&mut self, at: i64, bytes: &[u8], clip: bool) {
        let at = usize::try_from(at).expect("patch offset is non-negative");
        let mut bytes = bytes;
        if clip && !self.fit && at + bytes.len() > self.bytes.len() {
            bytes = &bytes[..self.bytes.len() - at];
        }
        let end = at + bytes.len();
        if end > self.bytes.len() {
            assert!(
                self.fit,
                "patch ends at {end}, beyond the {}-byte buffer",
                self.bytes.len()
            );
            self.bytes.resize(end, 0);
        }
        self.bytes[at..end].copy_from_slice(bytes);
    }

    fn u16(&mut self, at: i64, v: i64) {
        let v = v as u16;
        let b = if self.le { v.to_le_bytes() } else { v.to_be_bytes() };
        self.put(at, &b, false);
    }

    fn u32(&mut self, at: i64, v: i64) {
        let v = v as u32;
        let b = if self.le { v.to_le_bytes() } else { v.to_be_bytes() };
        self.put(at, &b, false);
    }

    fn apply(&mut self, patch: &Value) {
        if let Some(header) = patch.get("header") {
            let sig = match header["sig_hex"].as_str() {
                Some(hex) => from_hex(hex),
                None if self.le => b"II*\0".to_vec(),
                None => b"MM\0*".to_vec(),
            };
            self.put(0, &sig, false);
            self.u32(4, num(&header["ifd0"]));
        } else if let Some(ifd) = patch.get("ifd") {
            let at = num(&ifd["at"]);
            let entries = ifd["entries"].as_array().expect("ifd entries");
            self.u16(at, entries.len() as i64);
            for (i, entry) in entries.iter().enumerate() {
                let e = entry.as_array().expect("ifd entry [tag,type,count,value]");
                let p = at + 2 + i as i64 * 12;
                let (tag, ty, count, value) = (num(&e[0]), num(&e[1]), num(&e[2]), num(&e[3]));
                self.u16(p, tag);
                self.u16(p + 2, ty);
                self.u32(p + 4, count);
                // SHORT x 1 is left-justified in the value field (TiffBytes.Ifd).
                if ty == 3 && count == 1 {
                    self.u16(p + 8, value);
                } else {
                    self.u32(p + 8, value);
                }
            }
            self.u32(at + 2 + entries.len() as i64 * 12, num(&ifd["next"]));
        } else if let Some(u) = patch.get("u32") {
            self.u32(num(&u["at"]), num(&u["v"]));
        } else if let Some(j) = patch.get("jpeg") {
            let clip = j["clip"].as_bool() == Some(true);
            self.put(num(&patch["at"]), &jpeg_bytes(j), clip);
        } else if let Some(b) = patch.get("budget_jpeg") {
            self.put(num(&patch["at"]), &budget_jpeg(num(&b["segments"]) as usize), false);
        } else {
            self.put(
                num(&patch["at"]),
                &from_hex(patch["hex"].as_str().expect("patch hex")),
                false,
            );
        }
    }
}

/// `SyntheticRawBuilder.CreateMinimalJpeg(w, h)` plus the golden options (`lossless`, `sof`, `precision`, `corrupt_sof`, `pad`).
fn jpeg_bytes(o: &Value) -> Vec<u8> {
    let mut bytes = create_minimal_jpeg(num(&o["w"]) as i32, num(&o["h"]) as i32);
    let sof = bytes
        .windows(2)
        .position(|w| w == [0xFF, 0xC0])
        .expect("SOF0 marker in the minimal JPEG");
    if o["lossless"].as_bool() == Some(true) {
        bytes[sof + 1] = 0xC3;
    }
    if let Some(s) = o.get("sof") {
        bytes[sof + 1] = (0xC0 + num(s)) as u8;
    }
    if let Some(p) = o.get("precision") {
        bytes[sof + 4] = num(p) as u8;
    }
    if o["corrupt_sof"].as_bool() == Some(true) {
        bytes[sof + 5..sof + 9].fill(0);
    }
    let pad = o.get("pad").map_or(0, num) as usize;
    bytes.resize(bytes.len() + pad, 0);
    bytes
}

/// SOI + `segments` x (APP0 of 65535 bytes) + the 320x240 minimal JPEG without its SOI (`TiffPreviewHardeningTests.BudgetExhaustingJpeg`).
fn budget_jpeg(segments: usize) -> Vec<u8> {
    let tail = create_minimal_jpeg(320, 240);
    let mut out = vec![0xFF, 0xD8];
    let mut app = vec![0u8; 2 + 65535];
    app[..4].copy_from_slice(&[0xFF, 0xE0, 0xFF, 0xFF]);
    for _ in 0..segments {
        out.extend_from_slice(&app);
    }
    out.extend_from_slice(&tail[2..]);
    out
}

/// The `data` node of a `read` case (C# `W504Build`).
fn nef_bytes(data: &Value) -> Vec<u8> {
    let le = data["le"].as_bool().expect("data.le");
    let fit = data["len"].as_str() == Some("fit");
    let len = if fit {
        0
    } else {
        usize::try_from(num(&data["len"])).expect("data.len")
    };
    let mut buf = Buf {
        le,
        fit,
        bytes: vec![0; len],
    };
    for patch in data["patches"].as_array().expect("data.patches") {
        buf.apply(patch);
    }
    buf.bytes
}

// ---------------------------------------------------------------- golden loading and comparison

fn golden_cases() -> Vec<Value> {
    golden::load(GOLDEN).cases
}

fn golden_case(name: &str) -> Value {
    golden_cases()
        .into_iter()
        .find(|case| case["name"] == name)
        .unwrap_or_else(|| panic!("no golden case named '{name}' in tests/golden/{GOLDEN}.json"))
}

/// The output of a `read` op in the oracle's shape (C# `W504ReadStream`). `bytes_read` is the header budget charged.
fn read_observed(bytes: Vec<u8>) -> Value {
    let mut src = InMemoryRawHeaderSource::new(bytes);
    let cancel = CancelToken::new();
    let result = NefContainerReader.read(&mut src, &cancel);
    let mut out = Map::new();
    match &result {
        Ok(info) => {
            out.insert("outcome".to_owned(), json!("ok"));
            out.insert("format".to_owned(), json!(format!("{:?}", info.format)));
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
                        "kind": format!("{:?}", p.kind),
                        "width": p.width,
                        "height": p.height,
                        "color_space": format!("{:?}", p.color_space),
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
        Err(RawHeaderError::InvalidData { .. }) => {
            out.insert("outcome".to_owned(), json!("throws"));
            out.insert("throws".to_owned(), json!("InvalidDataException"));
        }
        Err(other) => {
            out.insert("outcome".to_owned(), json!("other-error"));
            out.insert("throws".to_owned(), json!(format!("{other:?}")));
        }
    }
    out.insert("bytes_read".to_owned(), json!(src.total_bytes_read()));
    Value::Object(out)
}

/// Compares every expected key of a golden case with the observed value (message names the case and the key).
fn compare(name: &str, actual: &Value, case: &Value) {
    let expected = case.as_object().expect("golden case is an object");
    for (key, want) in expected {
        if matches!(
            key.as_str(),
            "name" | "op" | "data" | "head_hex" | "ext" | "note" | "file_contains" | "optional" | "present"
        ) {
            continue;
        }
        assert_eq!(actual.get(key), Some(want), "golden case '{name}': key '{key}'");
    }
}

/// Runs one non-corpus golden case (`can-read` or `read`) and compares it with the golden expectation.
fn assert_case(case: &Value) {
    let name = case["name"].as_str().expect("case name");
    let actual = match case["op"].as_str() {
        Some("can-read") => {
            let head = from_hex(case["head_hex"].as_str().expect("head_hex"));
            let ext = case["ext"].as_str().expect("ext");
            json!({ "can_read": NefContainerReader.can_read(&head, ext) })
        }
        Some("read") => read_observed(nef_bytes(&case["data"])),
        other => panic!("golden case '{name}': unexpected op {other:?}"),
    };
    compare(name, &actual, case);
}

/// Checks one named golden case (the shared bytes of the C# test that the calling Rust test names).
fn check_case(name: &str) {
    assert_case(&golden_case(name));
}

// ---------------------------------------------------------------- RAW corpus (P-30: not downloaded, corpus tests skip)

/// `PHOTOREVIEW_RS_RAW_CORPUS` or `tests/fixtures/raw-corpus/`: the first `*.nef` (OrdinalIgnoreCase order) whose name contains `contains`.
fn corpus_file(contains: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("PHOTOREVIEW_RS_RAW_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| golden::repo_root().join("tests").join("fixtures").join("raw-corpus"));
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("nef")))
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()));
    files
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().contains(contains)))
}

/// The corpus bytes, or `None` (skip) when the sample is absent; strict mode turns the absence into a failure.
fn corpus_bytes(contains: &str) -> Option<Vec<u8>> {
    if let Some(path) = corpus_file(contains) {
        return Some(std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())));
    }
    if std::env::var("PHOTOREVIEW_RS_RAW_CORPUS_STRICT").as_deref() == Ok("1") {
        panic!("RAW corpus sample '*.nef' containing '{contains}' not found (PHOTOREVIEW_RS_RAW_CORPUS_STRICT=1)");
    }
    eprintln!("skipped: RAW corpus sample '*.nef' containing '{contains}' is absent (P-30)");
    None
}

// ---------------------------------------------------------------- the golden loop (all 246 cases)

#[test]
fn golden_nef_container_cases_match_csharp() {
    let cases = golden_cases();
    assert_eq!(cases.len(), 246, "tests/golden/{GOLDEN}.json case count");
    for case in &cases {
        if case["op"].as_str() == Some("read-corpus") {
            let contains = case["file_contains"].as_str().expect("file_contains");
            if let Some(bytes) = corpus_bytes(contains) {
                let actual = read_observed(bytes);
                assert_eq!(actual["outcome"], "ok", "corpus case '{}' must read", case["name"]);
            }
        } else {
            assert_case(case);
        }
    }
}

/// Every `can-read` golden case, in one test (the oracle's 27 signature / extension rows).
#[test]
fn golden_nef_can_read_cases_match_csharp() {
    for case in golden_cases().iter().filter(|c| c["op"] == "can-read") {
        assert_case(case);
    }
}

/// Every `read` golden case, in one test (the oracle's 216 synthetic buffers).
#[test]
fn golden_nef_read_cases_match_csharp() {
    for case in golden_cases().iter().filter(|c| c["op"] == "read") {
        assert_case(case);
    }
}

// ---------------------------------------------------------------- explicit unit assertions (not only golden)

#[test]
fn can_read_accepts_both_signatures_with_nef_extension() {
    assert!(NefContainerReader.can_read(&[0x49, 0x49, 0x2A, 0x00], ".nef"));
    assert!(NefContainerReader.can_read(&[0x4D, 0x4D, 0x00, 0x2A], ".NEF"));
}

#[test]
fn can_read_rejects_other_extension_even_with_nef_signature() {
    assert!(!NefContainerReader.can_read(&[0x49, 0x49, 0x2A, 0x00], ".orf"));
    assert!(!NefContainerReader.can_read(&[0x49, 0x49, 0x2A, 0x00], "nef"));
}

#[test]
fn can_read_rejects_head_shorter_than_four_bytes() {
    assert!(!NefContainerReader.can_read(&[0x49, 0x49, 0x2A], ".nef"));
}

/// C# `ReadNef_...` header rule: a source under 16 bytes is `InvalidData`, and so is a file that is 16 bytes long with a
/// bad signature. Only the bytes read so far are charged (no read past the header).
#[test]
fn read_source_shorter_than_sixteen_bytes_is_invalid_data() {
    let mut src = InMemoryRawHeaderSource::new(vec![0x49, 0x49, 0x2A, 0x00, 0, 0, 0, 0]);
    let cancel = CancelToken::new();
    let result = NefContainerReader.read(&mut src, &cancel);
    assert!(matches!(result, Err(RawHeaderError::InvalidData { .. })));
}

/// The cancellation check runs once per dequeued IFD, so a pre-cancelled token maps to `Canceled` for any file that gets
/// past the header (contract "Cancellation granularity"; a Rust-side check of the error mapping).
#[test]
fn read_pre_cancelled_token_returns_canceled_past_the_header() {
    let case = golden_case("queue-order-subifds-before-next-ifd");
    let mut src = InMemoryRawHeaderSource::new(nef_bytes(&case["data"]));
    let cancel = CancelToken::new();
    cancel.cancel();
    let result = NefContainerReader.read(&mut src, &cancel);
    assert!(matches!(result, Err(RawHeaderError::Canceled)));
}

// C# source: NefContainerReader.cs:64; Rust-only mutation killer.
#[test]
fn read_cyclic_ifd_is_not_reopened_after_being_visited() {
    let bytes = TiffBytes::new(true, 128)
        .header(8)
        .ifd(8, 8, &[long(256, 160), long(257, 120)])
        .into_vec();
    // The normal parse reads (8, 2) once and Exif block sizing reads it once more. A third read is the cycle.
    let mut src = HookedRawSource::new(bytes).fail_on_nth_read(8, 2, 3);
    let result = NefContainerReader.read(&mut src, &CancelToken::new());
    assert!(
        result.is_ok(),
        "visited IFD should be skipped before another source read"
    );
    assert_eq!(src.reads.iter().filter(|read| **read == (8, 2)).count(), 2);
}

// C# source: NefContainerReader.cs:133-134; Rust-only mutation killer.
#[test]
fn read_zero_next_ifd_does_not_create_a_cancellation_boundary() {
    let bytes = TiffBytes::new(true, 128)
        .header(8)
        .ifd(8, 0, &[long(256, 160), long(257, 120)])
        .into_vec();
    let token = CancelToken::new();
    let mut src = HookedRawSource::new(bytes).cancel_on_read(34, 4, token.clone());
    let result = NefContainerReader.read(&mut src, &token);
    assert!(
        token.is_canceled(),
        "fixture cancels after reading the zero next pointer"
    );
    assert!(
        result.is_ok(),
        "zero next pointer must not dequeue another IFD and observe cancellation"
    );
}

// C# source: NefContainerReader.cs:180; Rust-only mutation killer.
#[test]
fn read_nonpositive_raw_axis_skips_full_size_jpeg_probe() {
    let (bytes, jpeg_offset) = raw_with_unknown_jpeg_preview(0, 1000);
    let mut src = HookedRawSource::new(bytes).fail_on_nth_read(jpeg_offset, 2, 2);
    let result = NefContainerReader.read(&mut src, &CancelToken::new());
    assert!(
        result.is_ok(),
        "raw width zero must suppress the later JPEG frame probe"
    );
    assert_eq!(src.reads.iter().filter(|read| **read == (jpeg_offset, 2)).count(), 1);
}

// C# source: NefContainerReader.cs:194-197; Rust-only mutation killer.
#[test]
fn read_full_size_jpeg_probe_propagates_io_failure() {
    let (bytes, jpeg_offset) = raw_with_unknown_jpeg_preview(1000, 1000);
    let mut src = HookedRawSource::new(bytes).fail_on_nth_read(jpeg_offset, 2, 2);
    let result = NefContainerReader.read(&mut src, &CancelToken::new());
    assert!(matches!(result, Err(error) if error.is_io_failure()));
}

// C# source: NefContainerReader.cs:206-212; Rust-only mutation killer.
#[test]
fn read_full_size_jpeg_previews_are_ranked_by_area_not_axis_sum() {
    let bytes = raw_with_two_jpeg_previews(1000, 1000, (1000, 744), (870, 870));
    let mut src = InMemoryRawHeaderSource::new(bytes);
    let info = NefContainerReader
        .read(&mut src, &CancelToken::new())
        .expect("synthetic NEF with two full-size frames");
    assert_eq!((info.sensor_width, info.sensor_height), (870, 870));
}

// C# source: NefContainerReader.cs:238-240; Rust-only mutation killer.
#[test]
fn read_maker_note_exactly_eighteen_bytes_before_eof_reads_its_header() {
    let case = golden_case("maker-note-starting-exactly-18-bytes-before-eof");
    let mut src = HookedRawSource::new(nef_bytes(&case["data"]));
    let result = NefContainerReader.read(&mut src, &CancelToken::new());
    assert!(result.is_ok());
    assert!(
        src.reads.contains(&(182, 18)),
        "exact-boundary MakerNote must read its 18-byte header"
    );
}

/// `RawExif` is W5-11: the EXIF block of this reader is checked through `raw_exif::try_read_exif` (RED until W5-11 is
/// merged; the `exif_blocks` assertions of the same golden cases are checked meanwhile).
#[test]
fn read_ifds_after_128_kib_still_yields_exif_summary_via_raw_exif() {
    let case = golden_case("exif-block-ifds-after-128kib-grow-the-block");
    let mut src = InMemoryRawHeaderSource::new(nef_bytes(&case["data"]));
    let cancel = CancelToken::new();
    let info = NefContainerReader
        .read(&mut src, &cancel)
        .expect("NEF with IFDs after 128 KiB reads");
    let exif = try_read_exif(&mut src, &info);
    assert!(exif.complete);
    assert!(exif.summary.is_some());
}

// ---------------------------------------------------------------- named tests (C# names, golden-backed)

/// Declares one test per C# test row group; each row is a golden case of the same bytes.
macro_rules! golden_test {
    ($test:ident, $($name:literal),+ $(,)?) => {
        #[test]
        fn $test() {
            $( check_case($name); )+
        }
    };
}

// C#: NefOrfRw2MutationGapTests.CanRead_ExactlyFourSignatureBytes_IsAccepted (nef-le, nef-be)
golden_test!(
    can_read_exactly_four_signature_bytes_is_accepted,
    "can-read-le-exact",
    "can-read-be-exact"
);

// C#: NefOrfRw2MutationGapTests.CanRead_AnySingleSignatureByteWrong_IsRejected (nef-le/nef-be x position 0..3)
golden_test!(
    can_read_any_single_signature_byte_wrong_is_rejected,
    "can-read-le-byte0-wrong",
    "can-read-le-byte1-wrong",
    "can-read-le-byte2-wrong",
    "can-read-le-byte3-wrong",
    "can-read-be-byte0-wrong",
    "can-read-be-byte1-wrong",
    "can-read-be-byte2-wrong",
    "can-read-be-byte3-wrong",
);

// C#: NefOrfRw2MutationGapTests.Read_SixteenByteFileWithEmptyIfd_ReturnsEmptyInfoForNefOrfAndRw2 (NEF third only)
golden_test!(
    read_sixteen_byte_file_with_empty_ifd_returns_empty_info_for_nef,
    "sixteen-byte-file-empty-ifd",
    "sixteen-byte-file-empty-ifd-be",
);

// C#: NefOrfRw2MutationGapTests.ReadNef_ValidByteOrderButMagicFortyThree_Throws
golden_test!(
    read_nef_valid_byte_order_but_magic_forty_three_throws,
    "magic-43-throws",
    "magic-43-be-throws",
);

// C#: NefOrfRw2MutationGapTests.ReadNef_ChainOfSixtySixIfds_ReadsOnlyTheFirstSixtyFour (63/64/65/66 widen-and-narrow pairs)
golden_test!(
    read_nef_chain_of_sixty_six_ifds_reads_only_the_first_sixty_four,
    "ifd-chain-63-ifds",
    "ifd-chain-64-ifds",
    "ifd-chain-65-ifds",
    "ifd-chain-66-ifds",
);

// C#: NefOrfRw2MutationGapTests.ReadNef_OrientationOfSubIfdIsIgnoredInFavourOfIfd0
golden_test!(
    read_nef_orientation_of_sub_ifd_is_ignored_in_favour_of_ifd0,
    "orientation-subifd-ignored-ifd0-wins"
);

// C#: NefOrfRw2MutationGapTests.ReadNef_ReducedResolutionIfdLargerThanRaw_DoesNotWin
golden_test!(
    read_nef_reduced_resolution_ifd_larger_than_raw_does_not_win,
    "reduced-resolution-ifd-larger-than-raw-does-not-win"
);

// C#: NefOrfRw2MutationGapTests.ReadNef_JpegPreviewIfdWithLargerDeclaredSize_IsNotTheRawImage
golden_test!(
    read_nef_jpeg_preview_ifd_with_larger_declared_size_is_not_the_raw_image,
    "jpeg-preview-ifd-with-larger-declared-size-is-not-the-raw"
);

// C#: NefOrfRw2MutationGapTests.ReadNef_TwoRawIfdsOfEqualArea_KeepsTheFirst
golden_test!(
    read_nef_two_raw_ifds_of_equal_area_keeps_the_first,
    "two-raw-ifds-equal-area-keeps-first"
);

// C#: NefOrfRw2MutationGapTests.ReadNef_OnlyReducedResolutionIfd_FallsBackToIfd0Size
golden_test!(
    read_nef_only_reduced_resolution_ifd_falls_back_to_ifd0_size,
    "only-reduced-resolution-ifd-falls-back-to-ifd0-size"
);

// C#: NefOrfRw2MutationGapTests.ReadNef_PreviewDeclaringOnlyOneDimension_StillGetsItsFrameSizeRead (2 rows)
golden_test!(
    read_nef_preview_declaring_only_one_dimension_still_gets_its_frame_size_read,
    "jpg-from-raw-declaring-width-only-gets-frame-size",
    "jpg-from-raw-declaring-height-only-gets-frame-size",
);

// C#: NefOrfRw2MutationGapTests.ReadNef_FullSizeJpegFrame_ReplacesRawSizeOnlyWithinTheMaskedMarginRange (7 rows; golden holds 11)
golden_test!(
    read_nef_full_size_jpeg_frame_replaces_raw_size_only_within_the_masked_margin_range,
    "jpg-from-raw-frame-4000x3000-raw-4000x3010",
    "jpg-from-raw-frame-4000x3000-raw-4010x3000",
    "jpg-from-raw-frame-4000x3000-raw-4256x3000",
    "jpg-from-raw-frame-4000x3000-raw-4000x3256",
    "jpg-from-raw-frame-4000x3000-raw-4257x3000",
    "jpg-from-raw-frame-4000x3000-raw-4000x3257",
    "jpg-from-raw-frame-4000x3000-raw-3990x3000",
    "jpg-from-raw-frame-4000x3000-raw-4000x3000",
    "jpg-from-raw-frame-4000x3000-raw-4256x3256",
    "jpg-from-raw-frame-4000x3000-raw-4257x3257",
    "jpg-from-raw-frame-4000x3000-raw-4256x3257",
);

// C#: NefOrfRw2MutationGapTests.ReadNef_TwoFramesWithinMargin_PicksTheLargerAreaEvenWhenTheSmallerFileIsBigger
golden_test!(
    read_nef_two_frames_within_margin_picks_larger_area_even_when_smaller_file_is_bigger,
    "jpg-from-raw-two-frames-larger-area-wins-though-smaller-file-is-bigger",
);

// C#: NefOrfRw2MutationGapTests.ReadNef_TwoFramesOfEqualArea_KeepsTheFirstExamined
golden_test!(
    read_nef_two_frames_of_equal_area_keeps_the_first_examined,
    "jpg-from-raw-two-frames-equal-area-keeps-first-examined",
    "jpg-from-raw-two-frames-equal-length-equal-area-keeps-list-order",
);

// C#: NefOrfRw2MutationGapTests.ReadNef_DefaultCropSizePresent_IsNotOverriddenByAJpegFrame
golden_test!(
    read_nef_default_crop_size_present_is_not_overridden_by_a_jpeg_frame,
    "default-crop-present-is-not-overridden-by-a-jpeg-frame"
);

// C#: NefOrfRw2MutationGapTests.ReadNef_SameJpegReferencedByTwoIfds_IsListedOnce
golden_test!(
    read_nef_same_jpeg_referenced_by_two_ifds_is_listed_once,
    "same-jpeg-referenced-by-two-ifds-is-listed-once"
);

// C#: NefOrfRw2MutationGapTests.ReadNef_MakerNoteEntry_IsAnOffsetOnlyForTagMakerNoteWithMoreThanFourBytes (3 rows: count 4, 5, uint max)
golden_test!(
    read_nef_maker_note_entry_is_an_offset_only_for_tag_maker_note_with_more_than_four_bytes,
    "maker-note-exif-entry-tag-927c-count-4",
    "maker-note-exif-entry-tag-927c-count-5",
    "maker-note-exif-entry-tag-927c-count-4294967295",
);

// C#: NefOrfRw2MutationGapTests.ReadNef_MakerNoteStartingLessThanEighteenBytesBeforeEndOfFile_IsIgnoredWithoutThrowing
golden_test!(
    read_nef_maker_note_starting_less_than_eighteen_bytes_before_end_of_file_is_ignored_without_throwing,
    "maker-note-starting-17-bytes-before-eof-ignored",
    "maker-note-starting-exactly-18-bytes-before-eof",
);

// C#: NefOrfRw2MutationGapTests.ReadNef_MakerNoteStartingAtOffsetEight_IsParsed
golden_test!(
    read_nef_maker_note_starting_at_offset_eight_is_parsed,
    "maker-note-starting-at-offset-eight-is-parsed"
);

// C#: MakerNoteGapTests.NefReader_NikonMakerNoteVersionTwo_ListsThePreview_Control
golden_test!(
    nef_reader_nikon_maker_note_version_two_lists_the_preview_control,
    "maker-note-version-2"
);

// C#: MakerNoteGapTests.NefReader_NikonMakerNoteVersionOtherThanTwo_IsIgnored (0, 1, 3, 0xFF)
golden_test!(
    nef_reader_nikon_maker_note_version_other_than_two_is_ignored,
    "maker-note-version-0",
    "maker-note-version-1",
    "maker-note-version-3",
    "maker-note-version-255",
);

// C#: TiffReaderRegressionTests.NefReader_ThumbnailInIfd0_TakesSensorSizeFromRawSubIfd (little and big endian)
golden_test!(
    nef_reader_thumbnail_in_ifd0_takes_sensor_size_from_raw_sub_ifd,
    "thumbnail-in-ifd0-takes-sensor-size-from-raw-subifd",
    "thumbnail-in-ifd0-takes-sensor-size-from-raw-subifd-be",
);

// C#: TiffReaderRegressionTests.NefReader_MakerNotePreviewIfd_OffsetsRelativeToMakerNoteTiffHeader (little and big endian)
golden_test!(
    nef_reader_maker_note_preview_ifd_offsets_relative_to_maker_note_tiff_header,
    "maker-note-preview-ifd-offsets-relative-to-note-tiff-header-le",
    "maker-note-preview-ifd-offsets-relative-to-note-tiff-header-be",
);

// C#: TiffReaderRegressionTests.NefReader_MakerNotePreviewNotJpeg_IsIgnored (little and big endian)
golden_test!(
    nef_reader_maker_note_preview_not_jpeg_is_ignored,
    "maker-note-preview-not-jpeg-ignored-le",
    "maker-note-preview-not-jpeg-ignored-be"
);

// C#: TiffReaderRegressionTests.Readers_ArrayEntryCountAboveInt32Max_DoNotThrowUnexpectedExceptions (.nef row)
// The result must be Ok or InvalidData, never a panic: `subifd-array-entry-count-uint-max` is the golden form of that entry.
golden_test!(
    readers_array_entry_count_above_int32_max_do_not_throw_unexpected_exceptions,
    "subifd-array-entry-count-uint-max"
);

// C#: TiffSensorSizeTests.NefReader_DefaultCropSize_BeatsRawIfdSize (shorts, longs, rationals)
golden_test!(
    nef_reader_default_crop_size_beats_raw_ifd_size,
    "default-crop-shorts-beats-raw-ifd",
    "default-crop-longs-beats-raw-ifd",
    "default-crop-rationals-beats-raw-ifd",
);

// C#: TiffSensorSizeTests.NefReader_NoCropSize_UsesExifPixelDimensions
golden_test!(
    nef_reader_no_crop_size_uses_exif_pixel_dimensions,
    "no-crop-uses-exif-pixel-dimensions"
);

// C#: TiffSensorSizeTests.NefReader_NoCropNoExif_UsesFullSizeJpegFromRawFrameWithinMaskedMargin
golden_test!(
    nef_reader_no_crop_no_exif_uses_full_size_jpeg_from_raw_frame_within_masked_margin,
    "jpg-from-raw-no-crop-no-exif-near-full-size-6016x4016"
);

// C#: TiffSensorSizeTests.NefReader_JpegFromRawUnrelatedToRawSize_KeepsRawIfdSize (3 rows)
golden_test!(
    nef_reader_jpeg_from_raw_unrelated_to_raw_size_keeps_raw_ifd_size,
    "jpg-from-raw-no-crop-no-exif-reduced-1616x1080",
    "jpg-from-raw-no-crop-no-exif-larger-6300x4200",
    "jpg-from-raw-no-crop-no-exif-margin-over-256-5700x4000",
);

// C#: TiffFamilyReaderTests.NefReader_Synthetic_CanReadSubIfdPreview (golden proxy: the SubIFD pointer shape of that synthetic file)
golden_test!(
    nef_reader_synthetic_can_read_sub_ifd_preview,
    "subifd-pointer-type-4-inline"
);

// C#: TiffPreviewHardeningTests.Read_JpegInterchangePointerToZeroPadding_IsNotAPreview
golden_test!(
    read_jpeg_interchange_pointer_to_zero_padding_is_not_a_preview,
    "jpeg-interchange-pointer-to-zero-padding-is-not-a-preview"
);

// C#: TiffPreviewHardeningTests.Read_DeclaredSizeContradictingTheJpegFrame_UsesTheFrameSize
golden_test!(
    read_declared_size_contradicting_the_jpeg_frame_uses_the_frame_size,
    "jpeg-interchange-declared-size-contradicting-frame-uses-frame-size"
);

// C#: TiffPreviewHardeningTests.Read_PreviewJpegExhaustingHeaderBudget_ContainerStillParsesAndKeepsDeclaredSize
golden_test!(
    read_preview_jpeg_exhausting_header_budget_container_still_parses_and_keeps_declared_size,
    "jpeg-interchange-budget-exhausting-jpeg-keeps-declared-size",
);

// C#: TiffPreviewHardeningTests.Read_NefFullSizeJpegFrameWalkExhaustingHeaderBudget_ContainerStillParses
golden_test!(
    read_nef_full_size_jpeg_frame_walk_exhausting_header_budget_container_still_parses,
    "jpg-from-raw-walk-exhausting-header-budget-container-still-parses",
);

// C#: TiffPreviewHardeningTests.Read_WidthOnlyExifIfdFirst_DoesNotBlockALaterCompleteExifSize (nef row)
golden_test!(
    read_width_only_exif_ifd_first_does_not_block_a_later_complete_exif_size,
    "exif-width-only-ifd0-then-complete-in-raw-ifd"
);

// C#: TiffStripPreviewTests.Read_StripPreviewWhoseDeclaredSizeContradictsTheJpegFrame_UsesTheFrameSize (nef row)
golden_test!(
    read_strip_preview_whose_declared_size_contradicts_the_jpeg_frame_uses_the_frame_size,
    "strip-preview-compression-7-declared-size-contradicts-frame",
);

// C#: TiffSubIfdTypeTests.SubIfdPointers_OfLongOrIfdType_ReachTheRawIfd (type 4 and 13, inline and array)
golden_test!(
    sub_ifd_pointers_of_long_or_ifd_type_reach_the_raw_ifd,
    "subifd-pointer-type-4-inline",
    "subifd-pointer-type-13-inline",
    "subifd-pointer-type-4-array",
    "subifd-pointer-type-13-array",
);

// C#: TiffExifBlockPlacementTests.Read_IfdsInsideDefaultWindow_KeepsDefaultBlockLength (nef row)
golden_test!(
    read_ifds_inside_default_window_keeps_default_block_length,
    "exif-block-ifds-inside-default-window-keep-default"
);

// C#: TiffExifBlockPlacementTests.TryReadExif_IfdsAfter128KiB_StillYieldsSummary (nef row; the RawExif half is the test above, RED until W5-11)
golden_test!(
    try_read_exif_ifds_after_128_kib_still_yields_summary,
    "exif-block-ifds-after-128kib-grow-the-block"
);

// C#: TiffExifBlockPlacementTests.Read_IfdsBeyondMaxBlock_CapsBlockAndDoesNotThrow (nef row)
golden_test!(
    read_ifds_beyond_max_block_caps_block_and_does_not_throw,
    "exif-block-ifds-beyond-max-block-keep-default"
);

// C#: BinaryReaderFuzzTests / RawContainerFuzzTests golden form of the "10 000 mutants" walk over the synthetic NEF (first, third level).
golden_test!(
    nef_synthetic_little_endian_orientation_one,
    "synthetic-le-orientation-1"
);
golden_test!(nef_synthetic_big_endian_orientation_eight, "synthetic-be-orientation-8");
golden_test!(
    nef_synthetic_little_endian_orientation_six,
    "synthetic-le-orientation-6"
);

// Golden-only rows (no own C# test name; they pin the contract's traps: byte-exact boundary cases of `Read`).
golden_test!(
    maker_note_preview_length_one_at_last_byte_throws_on_soi_probe,
    "maker-note-preview-length-one-at-last-byte-throws-on-soi-probe"
);
golden_test!(
    maker_note_preview_length_two_at_end_is_listed,
    "maker-note-preview-length-two-at-end-is-listed"
);
golden_test!(maker_note_offset_seven_is_ignored, "maker-note-offset-7-ignored");
golden_test!(
    ifd_chain_of_sixty_four_and_sixty_five_boundaries,
    "ifd-chain-64-ifds",
    "ifd-chain-65-ifds"
);
golden_test!(
    next_ifd_pointer_too_small_and_one_too_far_are_dropped,
    "next-ifd-pointer-too-small",
    "next-ifd-pointer-one-too-far-95"
);

// ---------------------------------------------------------------- corpus-gated (TiffCorpusTests, RawContainerFuzz)

/// C#: TiffCorpusTests.NefCorpus_SensorSizeIsActiveAreaOfFullSizeJpegAndMakerNotePreviewIsListed
/// D40X 3872x2592 / preview 13508, D800 7360x4912 / 25336, Z 7 8256x5504 / 76188. Every listed preview starts with SOI.
/// Corpus-gated: skipped while the sample is absent (P-30).
#[test]
fn nef_corpus_sensor_size_is_active_area_of_full_size_jpeg_and_maker_note_preview_is_listed() {
    let samples = [
        ("D40X", 3872, 2592, 13508),
        ("D800", 7360, 4912, 25336),
        ("Z 7", 8256, 5504, 76188),
    ];
    for (contains, width, height, preview_length) in samples {
        let Some(bytes) = corpus_bytes(contains) else {
            continue;
        };
        let mut src = InMemoryRawHeaderSource::new(bytes);
        let cancel = CancelToken::new();
        let info = NefContainerReader
            .read(&mut src, &cancel)
            .unwrap_or_else(|e| panic!("{contains}: corpus NEF must read: {e:?}"));
        assert_eq!(
            (info.sensor_width, info.sensor_height),
            (width, height),
            "{contains}: sensor size"
        );
        let preview = info
            .previews
            .iter()
            .find(|p| p.length == preview_length)
            .unwrap_or_else(|| panic!("{contains}: MakerNote preview of {preview_length} bytes is not listed"));
        let mut head = [0u8; 2];
        let soi = src.read(preview.offset, 2).expect("preview SOI readable");
        head.copy_from_slice(soi);
        assert_eq!(head, [0xFF, 0xD8], "{contains}: preview must start with SOI");
    }
}

// ---------------------------------------------------------------- robustness (RawContainerFuzzTests, BinaryReaderFuzzTests)

/// Runs every mutant through `NefContainerReader::read`: no panic; `Ok` only with orientation 1..=8, non-negative sensor
/// size and previews inside the file; `InvalidData` otherwise. Port of `RunFuzzLoop` / `BinaryFuzz.Run` for the NEF targets.
fn fuzz_nef(label: &str, seed: &[u8], rng_seed: u64, count: usize) {
    for case in mutants(seed, rng_seed, count) {
        let file_len = i64::try_from(case.bytes.len()).expect("length fits i64");
        let mut src = InMemoryRawHeaderSource::new(case.bytes.clone());
        let cancel = CancelToken::new();
        let outcome = catch_unwind(AssertUnwindSafe(|| NefContainerReader.read(&mut src, &cancel)));
        let Ok(result) = outcome else {
            panic!("{label}: NefContainerReader panicked on mutant '{}'", case.label);
        };
        match result {
            Ok(info) => {
                assert!(
                    (1..=8).contains(&info.orientation),
                    "{label}: '{}' orientation {}",
                    case.label,
                    info.orientation
                );
                assert!(
                    info.sensor_width >= 0 && info.sensor_height >= 0,
                    "{label}: '{}' sensor size {}x{}",
                    case.label,
                    info.sensor_width,
                    info.sensor_height
                );
                for p in &info.previews {
                    assert!(
                        p.offset >= 0 && p.length > 0 && p.length <= file_len - p.offset,
                        "{label}: '{}' preview offset {} length {} outside a {file_len}-byte file",
                        case.label,
                        p.offset,
                        p.length
                    );
                }
            }
            Err(RawHeaderError::InvalidData { .. }) => {}
            Err(other) => panic!(
                "{label}: '{}' must fail only with InvalidData, got {other:?}",
                case.label
            ),
        }
    }
}

/// C#: RawContainerFuzzTests.Fuzz_NefContainerReader_Robust (10 000 mutants, seed `BuildTiff(little, Jpeg(), 1, 42)`).
#[test]
fn fuzz_nef_container_reader_robust() {
    let seed = build_tiff(true, Some(&create_minimal_jpeg(64, 48)), 1, 42);
    fuzz_nef("NEF", &seed, 102, 10_000);
}

/// C#: BinaryReaderFuzzTests.Fuzz_RawContainerReaders_FailOnlyCleanly, the `NEF` target (little endian, orientation 1).
#[test]
fn fuzz_raw_container_readers_fail_only_cleanly_nef() {
    let seed = build_tiff(true, Some(&create_minimal_jpeg(64, 48)), 1, 42);
    fuzz_nef("NEF", &seed, 4403, 1_500);
}

/// C#: BinaryReaderFuzzTests.Fuzz_RawContainerReaders_FailOnlyCleanly, the `NEF-BE` target (big endian, orientation 8).
#[test]
fn fuzz_raw_container_readers_fail_only_cleanly_nef_be() {
    let seed = build_tiff(false, Some(&create_minimal_jpeg(64, 48)), 8, 42);
    fuzz_nef("NEF-BE", &seed, 4407, 1_500);
}

/// C#: RawContainerFuzzTests.Fuzz_NefContainerReader_Robust timing (p95 wall clock). Machine dependent: ignored by default.
#[test]
#[ignore = "perf: wall-clock budget is machine dependent"]
fn fuzz_nef_container_reader_whole_loop_within_budget() {
    let seed = build_tiff(true, Some(&create_minimal_jpeg(64, 48)), 1, 42);
    let started = Instant::now();
    fuzz_nef("NEF", &seed, 102, 10_000);
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "fuzz loop took {:?}",
        started.elapsed()
    );
}
