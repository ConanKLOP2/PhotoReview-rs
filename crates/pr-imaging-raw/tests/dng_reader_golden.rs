//! Golden `raw-dng-synthetic` (tests/golden/raw-dng-synthetic.json, 258 cases) for W5-06 `DngContainerReader`.
//!
//! The byte layouts are rebuilt here from the case language documented in `tools/oracle-cs/Commands/W5-06.cs`, with the
//! oracle's builder (header, ifds, u32, u16, jpegs, puts, in that order; later writes win). Every reported field is
//! compared: `can_read`, `file_length`, `info` (format, sensor, orientation, each preview, each EXIF block) or `throws`
//! with `reader_message` when it is not null. Red until the porter fills `readers/dng.rs`.

use pr_core::sync::CancelToken;
use pr_imaging_raw::readers::dng::DngContainerReader;
use pr_imaging_raw::{InMemoryRawHeaderSource, RawContainerInfo, RawContainerReader, RawHeaderError};
use pr_test_support::golden;
use pr_test_support::raw::from_hex;
use serde_json::Value;

/// PhotometricInterpretation CFA (32803).
const CFA: u16 = 32803;
/// PhotometricInterpretation RGB (2).
const RGB: u16 = 2;

// ---------------------------------------------------------------- byte builder (oracle `W506Buf`)

/// Little or big endian buffer with the oracle's writers.
struct Buf {
    le: bool,
    bytes: Vec<u8>,
}

impl Buf {
    fn new(le: bool, size: usize) -> Self {
        Self {
            le,
            bytes: vec![0; size],
        }
    }

    fn put(&mut self, pos: usize, data: &[u8]) {
        self.bytes[pos..pos + data.len()].copy_from_slice(data);
    }

    fn u16(&mut self, pos: usize, value: u16) {
        let raw = if self.le {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        self.put(pos, &raw);
    }

    fn u32(&mut self, pos: usize, value: u32) {
        let raw = if self.le {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        self.put(pos, &raw);
    }

    /// Byte order magic (or the case's `signature_hex`), then IFD0 offset.
    fn header(&mut self, ifd0: u32, signature: Option<&[u8]>) {
        let default: &[u8] = if self.le {
            &[0x49, 0x49, 0x2A, 0x00]
        } else {
            &[0x4D, 0x4D, 0x00, 0x2A]
        };
        self.put(0, signature.unwrap_or(default));
        self.u32(4, ifd0);
    }

    /// `(tag, type, count, value)` entries; a SHORT with count 1 is written left-justified as 2 bytes.
    fn ifd(&mut self, pos: usize, next: u32, entries: &[(u16, u16, u32, u32)]) {
        self.u16(pos, entries.len() as u16);
        for (i, &(tag, ty, count, value)) in entries.iter().enumerate() {
            let at = pos + 2 + i * 12;
            self.u16(at, tag);
            self.u16(at + 2, ty);
            self.u32(at + 4, count);
            if ty == 3 && count == 1 {
                self.u16(at + 8, value as u16);
            } else {
                self.u32(at + 8, value);
            }
        }
        self.u32(pos + 2 + entries.len() * 12, next);
    }
}

/// Oracle `W506Image`: width, height and photometric SHORT entries.
fn image(width: u32, height: u32, photometric: u16) -> Vec<(u16, u16, u32, u32)> {
    vec![
        (0x0100, 3, 1, width),
        (0x0101, 3, 1, height),
        (0x0106, 3, 1, u32::from(photometric)),
    ]
}

/// Oracle `W506MinimalJpeg`: SOI, APP0 JFIF, DQT, SOF0, DHT, SOS, 6 data bytes, EOI (163 bytes at 320x240).
fn minimal_jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut j = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    j.extend_from_slice(b"JFIF\0");
    j.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
    j.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    j.extend(vec![16u8; 64]);
    j.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
    j.extend_from_slice(&[(height >> 8) as u8, height as u8, (width >> 8) as u8, width as u8]);
    j.push(3);
    j.extend_from_slice(&[0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
    j.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x1F, 0x00]);
    j.extend_from_slice(&[
        0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]);
    j.extend_from_slice(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B]);
    j.extend_from_slice(&[
        0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
    ]);
    j.extend_from_slice(&[0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55]);
    j.extend_from_slice(&[0xFF, 0xD9]);
    j
}

/// Oracle `W506Jpeg` (`budget`): 140 maximal APP0 segments one 64 KiB block apart, then the minimal frame.
fn budget_jpeg(width: u32, height: u32) -> Vec<u8> {
    let tail = minimal_jpeg(width, height);
    let mut big = vec![0xFF, 0xD8];
    let mut app = vec![0u8; 2 + 65535];
    app[0] = 0xFF;
    app[1] = 0xE0;
    app[2] = 0xFF;
    app[3] = 0xFF;
    for _ in 0..140 {
        big.extend_from_slice(&app);
    }
    big.extend_from_slice(&tail[2..]);
    big
}

/// Oracle `W506Jpeg`: `sof` "c3" rewrites the SOF marker to lossless, `precision` (default 8), `fill` 0xFF bytes after SOI.
fn jpeg_bytes(spec: &Value) -> Vec<u8> {
    let width = int(&spec["w"]) as u32;
    let height = int(&spec["h"]) as u32;
    if spec.get("budget").and_then(Value::as_bool) == Some(true) {
        return budget_jpeg(width, height);
    }
    let mut jpeg = minimal_jpeg(width, height);
    let sof = jpeg.iter().position(|&b| b == 0xC0).expect("SOF0 present") - 1;
    if spec.get("sof").and_then(Value::as_str) == Some("c3") {
        jpeg[sof + 1] = 0xC3;
    }
    jpeg[sof + 4] = spec.get("precision").and_then(Value::as_u64).unwrap_or(8) as u8;
    let fill = spec.get("fill").and_then(Value::as_u64).unwrap_or(0) as usize;
    let mut out = jpeg[..2].to_vec();
    out.extend(vec![0xFF; fill]);
    out.extend_from_slice(&jpeg[2..]);
    out
}

// ---------------------------------------------------------------- generators (oracle `gen`)

/// `chain-66`: IFD i is 100+i wide; the chain stops after 64 IFDs, IFD 65 is never reached.
fn gen_chain_66() -> Vec<u8> {
    const IFD_SIZE: usize = 42;
    let mut f = Buf::new(true, 8 + 66 * IFD_SIZE);
    f.header(8, None);
    for i in 0..66 {
        let at = 8 + i * IFD_SIZE;
        let next = if i < 65 { (at + IFD_SIZE) as u32 } else { 0 };
        f.ifd(at, next, &image(100 + i as u32, 100, CFA));
    }
    f.bytes
}

/// `subifd-65`: IFD0 -> chain IFD with a 64-entry SubIFD array and one more SubIFD as a single pointer.
fn gen_subifd_65() -> Vec<u8> {
    const IFD_SIZE: usize = 42;
    const ARRAY_AT: usize = 80;
    const CHAIN_IFD_AT: usize = 340;
    const SUBS_AT: usize = 400;
    let mut f = Buf::new(true, SUBS_AT + 65 * IFD_SIZE + 8);
    f.header(8, None);
    let mut entries = image(10, 10, RGB);
    entries.push((0x014A, 4, 64, ARRAY_AT as u32));
    f.ifd(8, CHAIN_IFD_AT as u32, &entries);
    f.ifd(CHAIN_IFD_AT, 0, &[(0x014A, 4, 1, (SUBS_AT + 64 * IFD_SIZE) as u32)]);
    for i in 0..64 {
        f.u32(ARRAY_AT + i * 4, (SUBS_AT + i * IFD_SIZE) as u32);
    }
    for i in 0..64 {
        f.ifd(SUBS_AT + i * IFD_SIZE, 0, &image(50, 50, CFA));
    }
    f.ifd(SUBS_AT + 64 * IFD_SIZE, 0, &image(5000, 4000, CFA));
    f.bytes
}

/// `sub64-chain`: 64 SubIFD pointers do not stop the IFD chain early.
fn gen_sub64_chain() -> Vec<u8> {
    const SUB_COUNT: usize = 64;
    let mut f = Buf::new(true, 4000);
    f.header(8, None);
    f.ifd(8, 2000, &[(0x014A, 4, SUB_COUNT as u32, 1000)]);
    f.ifd(2000, 2100, &[(0x0100, 3, 1, 100), (0x0101, 3, 1, 80)]);
    f.ifd(
        2100,
        0,
        &[
            (0x00FE, 4, 1, 0),
            (0x0100, 3, 1, 4000),
            (0x0101, 3, 1, 3000),
            (0x0106, 3, 1, u32::from(CFA)),
        ],
    );
    for i in 0..SUB_COUNT {
        f.u32(1000 + i * 4, (2200 + i * 8) as u32);
        f.ifd(2200 + i * 8, 0, &[]);
    }
    f.bytes
}

// ---------------------------------------------------------------- case language (oracle `W506Build`)

fn int(node: &Value) -> i64 {
    node.as_i64().unwrap_or_else(|| panic!("expected an integer: {node}"))
}

fn array<'a>(node: &'a Value, key: &str) -> &'a [Value] {
    match node.get(key).and_then(Value::as_array) {
        Some(items) => items.as_slice(),
        None => &[],
    }
}

/// Oracle `W506Entry`: `short` / `long` (value, wrapping to u32) or `at` (type, count, off); tag is hex.
fn entry(node: &Value) -> (u16, u16, u32, u32) {
    let tag = u16::from_str_radix(node["tag"].as_str().expect("tag"), 16).expect("hex tag");
    match node["k"].as_str().expect("entry kind") {
        "short" => (tag, 3, 1, int(&node["v"]) as u32),
        "long" => (tag, 4, 1, int(&node["v"]) as u32),
        "at" => (
            tag,
            int(&node["type"]) as u16,
            int(&node["count"]) as u32,
            int(&node["off"]) as u32,
        ),
        other => panic!("unknown entry kind {other}"),
    }
}

/// Oracle `W506Build`: the generators, or the explicit layout of the case.
fn build_case(case: &Value) -> Vec<u8> {
    match case.get("gen").and_then(Value::as_str) {
        Some("chain-66") => return gen_chain_66(),
        Some("subifd-65") => return gen_subifd_65(),
        Some("sub64-chain") => return gen_sub64_chain(),
        Some(other) => panic!("unknown gen {other}"),
        None => {}
    }
    let le = case.get("le").and_then(Value::as_bool).unwrap_or(true);
    let mut buf = Buf::new(le, int(&case["size"]) as usize);
    let signature = case.get("signature_hex").and_then(Value::as_str).map(from_hex);
    let ifd0 = case.get("ifd0").and_then(Value::as_i64).unwrap_or(8) as u32;
    buf.header(ifd0, signature.as_deref());
    for node in array(case, "ifds") {
        let entries: Vec<(u16, u16, u32, u32)> = array(node, "entries").iter().map(entry).collect();
        let next = node.get("next").and_then(Value::as_i64).unwrap_or(0) as u32;
        buf.ifd(int(&node["at"]) as usize, next, &entries);
    }
    for pair in array(case, "u32") {
        buf.u32(int(&pair[0]) as usize, int(&pair[1]) as u32);
    }
    for pair in array(case, "u16") {
        buf.u16(int(&pair[0]) as usize, int(&pair[1]) as u16);
    }
    for node in array(case, "jpegs") {
        buf.put(int(&node["at"]) as usize, &jpeg_bytes(node));
    }
    for node in array(case, "puts") {
        let raw = from_hex(node["hex"].as_str().expect("puts hex"));
        buf.put(int(&node["at"]) as usize, &raw);
    }
    buf.bytes
}

// ---------------------------------------------------------------- comparison

fn read_bytes(bytes: Vec<u8>) -> Result<RawContainerInfo, RawHeaderError> {
    let mut source = InMemoryRawHeaderSource::new(bytes);
    DngContainerReader::new().read(&mut source, &CancelToken::new())
}

fn expect_eq<T: PartialEq + std::fmt::Debug>(failures: &mut Vec<String>, name: &str, field: &str, want: T, got: T) {
    if want != got {
        failures.push(format!("{name}: {field}: want {want:?}, got {got:?}"));
    }
}

/// Compares every field of an `info` node (format, sensor, orientation, previews, EXIF blocks).
fn compare_info(failures: &mut Vec<String>, name: &str, want: &Value, got: &RawContainerInfo) {
    expect_eq(
        failures,
        name,
        "format",
        want["format"].as_str().unwrap_or_default().to_owned(),
        format!("{:?}", got.format),
    );
    expect_eq(
        failures,
        name,
        "sensor_width",
        int(&want["sensor_width"]),
        i64::from(got.sensor_width),
    );
    expect_eq(
        failures,
        name,
        "sensor_height",
        int(&want["sensor_height"]),
        i64::from(got.sensor_height),
    );
    expect_eq(
        failures,
        name,
        "orientation",
        int(&want["orientation"]),
        i64::from(got.orientation),
    );

    let want_previews = array(want, "previews");
    expect_eq(failures, name, "previews.len", want_previews.len(), got.previews.len());
    for (i, (w, g)) in want_previews.iter().zip(&got.previews).enumerate() {
        let field = |f: &str| format!("previews[{i}].{f}");
        expect_eq(failures, name, &field("index"), int(&w["index"]), i64::from(g.index));
        expect_eq(failures, name, &field("offset"), int(&w["offset"]), g.offset);
        expect_eq(failures, name, &field("length"), int(&w["length"]), g.length);
        expect_eq(
            failures,
            name,
            &field("kind"),
            w["kind"].as_str().unwrap_or_default().to_owned(),
            format!("{:?}", g.kind),
        );
        expect_eq(failures, name, &field("width"), int(&w["width"]), i64::from(g.width));
        expect_eq(failures, name, &field("height"), int(&w["height"]), i64::from(g.height));
        expect_eq(
            failures,
            name,
            &field("color_space"),
            w["color_space"].as_str().unwrap_or_default().to_owned(),
            format!("{:?}", g.color_space),
        );
        expect_eq(
            failures,
            name,
            &field("header_resolved"),
            w["header_resolved"].as_bool(),
            Some(g.header_resolved),
        );
    }

    let want_blocks = array(want, "exif_blocks");
    expect_eq(
        failures,
        name,
        "exif_blocks.len",
        want_blocks.len(),
        got.exif_blocks.len(),
    );
    for (i, (w, g)) in want_blocks.iter().zip(&got.exif_blocks).enumerate() {
        let field = |f: &str| format!("exif_blocks[{i}].{f}");
        expect_eq(failures, name, &field("offset"), int(&w["offset"]), g.offset);
        expect_eq(failures, name, &field("length"), int(&w["length"]), g.length);
        expect_eq(
            failures,
            name,
            &field("is_tiff_header"),
            w["is_tiff_header"].as_bool(),
            Some(g.is_tiff_header),
        );
        expect_eq(
            failures,
            name,
            &field("ifd_is_exif"),
            w["ifd_is_exif"].as_bool(),
            Some(g.ifd_is_exif),
        );
    }
}

#[test]
fn golden_raw_dng_synthetic_matches_csharp_for_every_case() {
    let golden = golden::load("raw-dng-synthetic");
    let reader = DngContainerReader::new();
    let mut failures = Vec::new();

    for case in &golden.cases {
        let name = golden::str_of(case, "name");
        let bytes = build_case(case);
        let ext = case.get("ext").and_then(Value::as_str).unwrap_or(".dng");

        expect_eq(
            &mut failures,
            name,
            "file_length",
            golden::i64_of(case, "file_length"),
            bytes.len() as i64,
        );
        let head = &bytes[..bytes.len().min(64)];
        expect_eq(
            &mut failures,
            name,
            "can_read",
            golden::bool_of(case, "can_read"),
            reader.can_read(head, ext),
        );

        let result = read_bytes(bytes);
        if let Some(want) = case.get("info") {
            match result {
                Ok(info) => compare_info(&mut failures, name, want, &info),
                Err(error) => failures.push(format!("{name}: expected info, got {error:?}")),
            }
        } else if case.get("throws").is_some() {
            match result {
                Err(RawHeaderError::InvalidData { message, .. }) => {
                    if let Some(want_message) = case["reader_message"].as_str() {
                        expect_eq(&mut failures, name, "reader_message", want_message.to_owned(), message);
                    }
                }
                Err(other) => failures.push(format!("{name}: expected InvalidDataException, got {other:?}")),
                Ok(info) => failures.push(format!("{name}: expected InvalidDataException, got {info:?}")),
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} golden cases differ from the C# oracle:\n{}",
        failures.len(),
        golden.cases.len(),
        failures.join("\n")
    );
}
