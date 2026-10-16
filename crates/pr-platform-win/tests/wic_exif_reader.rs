//! Red tests for W4-11d `WicExifReader` (`crates/pr-platform-win/src/wic/exif_reader.rs`).
//!
//! Port of the C# tests at baseline d92d6aae:
//! * `tests/PhotoReview.Imaging.Tests/Decoding/WicMetadataIndependenceTests.cs`
//! * `tests/PhotoReview.Imaging.Tests/Decoding/WicDirectInternalsMutationTests.cs` (PROPVARIANT shapes, orientation tag
//!   range, native decodes)
//! * `tests/PhotoReview.Imaging.Tests/Decoding/WicDirectFaultInjectionTests.cs` (ReadFrameMetadata / ExifIfdRootOf)
//!
//! Golden-driven: `tests/golden/wic-exif-values.json` and `tests/golden/wic-exif-variant.json`, inputs in
//! `tests/golden-inputs/`. Every test fails via `todo!()` until the bodies are implemented. Native tests need WIC:
//! run them with `-- --ignored`.
//!
//! C# tests not ported (documented skip): `ReadFrameMetadata_OutOfMemory_IsNotSwallowed` (type system: Rust aborts on
//! out-of-memory and has no catch branch to port).
#![cfg(windows)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use pr_core::catalog::explorer_types::Guid;
use pr_imaging::exif_query::{JPEG_IFD_ROOT, QueryValue, TIFF_IFD_ROOT};
use pr_imaging::exif_summary::ExifSummary;
use pr_imaging::orientation::QueryFailed;
use pr_imaging::tiff::ExifRational;
use pr_platform_win::wic::exif_reader::{
    CONTAINER_FORMAT_JPEG, CONTAINER_FORMAT_TIFF, DecoderContainer, FrameMetadata, HResult, MetadataQueryReader,
    MetadataRead, PROP_VARIANT_SIZE, PropVariant, VT_VECTOR, VariantMemory, decode_variant,
    exif_ifd_root_for_container, exif_ifd_root_of, read_frame_metadata, read_metadata_values,
};
use pr_platform_win::wic::interop::{
    Com, ComStream, create_decoder_from_stream, create_factory, get_frame, get_metadata_query_reader,
};
use pr_test_support::golden;
use serde_json::{Value, json};
use windows::Win32::Graphics::Imaging::{
    IWICBitmapDecoder, IWICBitmapFrameDecode, IWICMetadataQueryReader, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::core::PCWSTR;

/// `E_FAIL`, the HRESULT of a failing COM call.
const E_FAIL: HResult = 0x8000_4005u32 as i32;
/// `/app1/ifd/{ushort=274}`: the EXIF orientation tag (C# `OrientationQuery`).
const ORIENTATION_QUERY: &str = "/app1/ifd/{ushort=274}";
/// `System.Photo.Orientation`: the Windows fallback property.
const WINDOWS_ORIENTATION_QUERY: &str = "System.Photo.Orientation";
/// Arbitrary non-zero addresses that the fake memory maps to payload bytes.
const TEXT_ADDR: usize = 0x1000;
const ELEMENT_ADDR: usize = 0x2000;

// ---------------------------------------------------------------------------------------------------------------
// Fakes and helpers
// ---------------------------------------------------------------------------------------------------------------

/// Fake `VariantMemory`: payload bytes keyed by the address the PROPVARIANT points at.
#[derive(Default)]
struct FakeMemory {
    regions: BTreeMap<usize, Vec<u8>>,
}

impl FakeMemory {
    fn with(mut self, addr: usize, bytes: Vec<u8>) -> Self {
        self.regions.insert(addr, bytes);
        self
    }

    fn bytes_at(&self, addr: usize) -> &[u8] {
        let (base, bytes) = self
            .regions
            .range(..=addr)
            .next_back()
            .unwrap_or_else(|| panic!("no fake memory at {addr:#x}"));
        &bytes[addr - base..]
    }
}

impl VariantMemory for FakeMemory {
    fn utf8_z(&self, addr: usize) -> String {
        let bytes = self.bytes_at(addr);
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    }

    fn utf16_z(&self, addr: usize) -> String {
        let units: Vec<u16> = self
            .bytes_at(addr)
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|&unit| unit != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }

    fn u16_at(&self, addr: usize) -> u16 {
        let bytes = self.bytes_at(addr);
        u16::from_le_bytes([bytes[0], bytes[1]])
    }

    fn u32_at(&self, addr: usize) -> u32 {
        let bytes = self.bytes_at(addr);
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    fn u64_at(&self, addr: usize) -> u64 {
        let bytes = self.bytes_at(addr);
        let mut array = [0u8; 8];
        array.copy_from_slice(&bytes[..8]);
        u64::from_le_bytes(array)
    }
}

/// PROPVARIANT bytes with only `vt` set.
fn raw_with_vt(vt: u16) -> [u8; PROP_VARIANT_SIZE] {
    let mut raw = [0u8; PROP_VARIANT_SIZE];
    raw[0..2].copy_from_slice(&vt.to_le_bytes());
    raw
}

/// PROPVARIANT bytes with `vt` and an i64 scalar at byte 8 (the upper bits are garbage the decoder must ignore).
fn raw_with_scalar(vt: u16, scalar: i64) -> [u8; PROP_VARIANT_SIZE] {
    let mut raw = raw_with_vt(vt);
    raw[8..16].copy_from_slice(&scalar.to_le_bytes());
    raw
}

/// Writes a pointer (`usize`) at `offset`.
fn put_pointer(raw: &mut [u8; PROP_VARIANT_SIZE], offset: usize, addr: usize) {
    raw[offset..offset + std::mem::size_of::<usize>()].copy_from_slice(&addr.to_le_bytes());
}

/// PROPVARIANT bytes of a vector: `count: i32` at byte 8, element pointer at `8 + size_of::<usize>()`.
fn raw_vector(vt: u16, count: i32, pointer: Option<usize>) -> [u8; PROP_VARIANT_SIZE] {
    let mut raw = raw_with_vt(vt);
    raw[8..12].copy_from_slice(&count.to_le_bytes());
    if let Some(addr) = pointer {
        put_pointer(&mut raw, 8 + std::mem::size_of::<usize>(), addr);
    }
    raw
}

/// NUL-terminated UTF-16LE bytes.
fn utf16z(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or_else(|e| panic!("bad hex {hex}: {e}")))
        .collect()
}

/// An integer that the golden writes either as a JSON number or as a decimal string.
fn int_of(value: &Value) -> i128 {
    match value {
        Value::String(text) => text.parse().unwrap_or_else(|e| panic!("not an integer {text:?}: {e}")),
        Value::Number(number) => i128::from(number.as_i64().expect("golden integer fits i64")),
        other => panic!("not an integer: {other}"),
    }
}

/// Loads `tests/golden-inputs/<file>` (the inputs of the golden outputs; not part of `golden::load`).
fn load_inputs(file: &str) -> Value {
    let path = golden::repo_root().join("tests").join("golden-inputs").join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// One query answer of the `wic-exif-values` inputs.
enum Answer {
    Value(QueryValue),
    Throw,
}

fn parse_answer(answer: &Value) -> Answer {
    let value = &answer["value"];
    let query_value = match answer["type"].as_str().expect("answer type") {
        "throw" => return Answer::Throw,
        "u16" => QueryValue::U16(u16::try_from(int_of(value)).expect("u16")),
        "i16" => QueryValue::I16(i16::try_from(int_of(value)).expect("i16")),
        "u8" => QueryValue::U8(u8::try_from(int_of(value)).expect("u8")),
        "i32" => QueryValue::I32(i32::try_from(int_of(value)).expect("i32")),
        "u32" => QueryValue::U32(u32::try_from(int_of(value)).expect("u32")),
        "u64" => QueryValue::U64(u64::try_from(int_of(value)).expect("u64")),
        "u16[]" => QueryValue::U16s(
            value
                .as_array()
                .expect("u16[] array")
                .iter()
                .map(|v| u16::try_from(int_of(v)).expect("u16"))
                .collect(),
        ),
        "i16[]" => QueryValue::I16s(
            value
                .as_array()
                .expect("i16[] array")
                .iter()
                .map(|v| i16::try_from(int_of(v)).expect("i16"))
                .collect(),
        ),
        "string" => QueryValue::Text(value.as_str().expect("string").to_owned()),
        "chars" => QueryValue::Chars(value.as_str().expect("chars").encode_utf16().collect()),
        "double" => QueryValue::Other,
        other => panic!("unknown answer type {other}"),
    };
    Answer::Value(query_value)
}

/// `summary` of the golden output (`null` or the eight fields).
fn summary_json(exif: Option<&ExifSummary>) -> Value {
    let Some(summary) = exif else {
        return Value::Null;
    };
    json!({
        "date_taken": summary.date_taken.map(|d| d.to_json().trim_end_matches('Z').to_owned()),
        "camera_make": summary.camera_make,
        "camera_model": summary.camera_model,
        "lens_model": summary.lens_model,
        "iso": summary.iso,
        "focal_length": rational_json(summary.focal_length),
        "f_number": rational_json(summary.f_number),
        "exposure_time": rational_json(summary.exposure_time),
    })
}

fn rational_json(rational: Option<ExifRational>) -> Value {
    match rational {
        None => Value::Null,
        Some(r) => json!({ "numerator": r.numerator, "denominator": r.denominator }),
    }
}

/// `(type, value)` of a `wic-exif-variant` output row, the way the oracle prints it.
fn describe(value: Option<QueryValue>) -> (Value, Value) {
    match value {
        None => (Value::Null, Value::Null),
        Some(QueryValue::Text(text)) => (json!("string"), json!(text)),
        Some(QueryValue::U8(v)) => (json!("u8"), json!(v.to_string())),
        Some(QueryValue::U16(v)) => (json!("u16"), json!(v.to_string())),
        Some(QueryValue::I16(v)) => (json!("i16"), json!(v.to_string())),
        Some(QueryValue::U32(v)) => (json!("u32"), json!(v.to_string())),
        Some(QueryValue::I32(v)) => (json!("i32"), json!(v.to_string())),
        Some(QueryValue::U64(v)) => (json!("u64"), json!(v.to_string())),
        Some(QueryValue::I64(v)) => (json!("i64"), json!(v.to_string())),
        Some(QueryValue::U16s(v)) => (json!("u16[]"), json!(v.iter().map(u16::to_string).collect::<Vec<_>>())),
        Some(QueryValue::U32s(v)) => (json!("u32[]"), json!(v.iter().map(u32::to_string).collect::<Vec<_>>())),
        Some(QueryValue::I64s(v)) => (json!("i64[]"), json!(v.iter().map(i64::to_string).collect::<Vec<_>>())),
        // The oracle labels BOTH `long[]` and `ulong[]` vectors "i64[]" (values stay unsigned for the UI8 vector), so
        // `U64s` is printed with the same label to compare exactly what the golden holds.
        Some(QueryValue::U64s(v)) => (json!("i64[]"), json!(v.iter().map(u64::to_string).collect::<Vec<_>>())),
        Some(other) => panic!("unexpected decode result {other:?}"),
    }
}

/// Builds the PROPVARIANT bytes and the fake memory of one `wic-exif-variant` input.
fn variant_from_golden(case: &Value) -> ([u8; PROP_VARIANT_SIZE], FakeMemory) {
    let vt = u16::try_from(case["vt"].as_u64().expect("vt")).expect("vt fits a WORD");
    let mut raw = raw_with_vt(vt);
    if let Some(hex) = case["reserved_hex"].as_str() {
        raw[2..8].copy_from_slice(&hex_bytes(hex));
    }
    if let Some(scalar) = case["scalar"].as_str() {
        raw[8..16].copy_from_slice(&scalar.parse::<i64>().expect("scalar").to_le_bytes());
    }
    if let Some(count) = case["count"].as_i64() {
        raw[8..12].copy_from_slice(&i32::try_from(count).expect("count").to_le_bytes());
    }
    let mut memory = FakeMemory::default();
    if !case["payload"].is_null() {
        let (offset, addr) = if vt & VT_VECTOR != 0 {
            (8 + std::mem::size_of::<usize>(), ELEMENT_ADDR)
        } else {
            (8, TEXT_ADDR)
        };
        put_pointer(&mut raw, offset, addr);
        memory = memory.with(addr, payload_bytes(&case["payload"]));
    }
    (raw, memory)
}

fn payload_bytes(payload: &Value) -> Vec<u8> {
    match payload["kind"].as_str().expect("payload kind") {
        // Text is written exactly as the golden input stores it. The non-ASCII rows are self-consistent that way (input
        // and expected value are the same string). `wide-string-supplementary-plane` is not: its input is double-encoded
        // while its expected value is the real emoji, so no decoder can meet it. Reported as a golden defect.
        "wide" => utf16z(payload["text"].as_str().expect("wide text")),
        "utf8" => {
            let mut bytes = match payload["text"].as_str() {
                Some(text) => text.as_bytes().to_vec(),
                None => hex_bytes(payload["bytes_hex"].as_str().expect("utf8 bytes_hex")),
            };
            bytes.push(0);
            bytes
        }
        "u16" => payload["elements"]
            .as_array()
            .expect("elements")
            .iter()
            .flat_map(|v| u16::try_from(int_of(v)).expect("u16").to_le_bytes())
            .collect(),
        "u32" => payload["elements"]
            .as_array()
            .expect("elements")
            .iter()
            .flat_map(|v| u32::try_from(int_of(v)).expect("u32").to_le_bytes())
            .collect(),
        "i64" => payload["elements"]
            .as_array()
            .expect("elements")
            .iter()
            .flat_map(|v| i64::try_from(int_of(v)).expect("i64").to_le_bytes())
            .collect(),
        "u64" => payload["elements"]
            .as_array()
            .expect("elements")
            .iter()
            .flat_map(|v| u64::try_from(int_of(v)).expect("u64").to_le_bytes())
            .collect(),
        other => panic!("unknown payload kind {other}"),
    }
}

/// `guid` as .NET `Guid.ToString("D")` prints it (the `constants` row of `wic-exif-values`).
fn guid_text(guid: &Guid) -> String {
    let b = guid.0;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15],
    )
}

/// Orientation of `read_metadata_values` for one EXIF answer and one Windows answer (`None` = absent).
fn orientation_with(exif: Option<QueryValue>, windows: Option<QueryValue>, read_orientation: bool) -> i32 {
    let query = |name: &str| -> Result<Option<QueryValue>, QueryFailed> {
        Ok(match name {
            ORIENTATION_QUERY => exif.clone(),
            WINDOWS_ORIENTATION_QUERY => windows.clone(),
            _ => None,
        })
    };
    read_metadata_values(query, read_orientation, None).orientation
}

fn camera_make(read: &MetadataRead) -> Option<&str> {
    read.exif.as_ref().and_then(|exif| exif.camera_make.as_deref())
}

/// A frame whose `GetMetadataQueryReader` fails (no metadata reader at all).
struct FrameWithoutReader;

/// The reader type of [`FrameWithoutReader`]; never constructed.
struct NeverReader;

impl MetadataQueryReader for NeverReader {
    fn get_metadata_by_name(&self, _name: &str, _value: &mut PropVariant) -> HResult {
        unreachable!("a frame without a metadata reader never gives one")
    }
}

impl FrameMetadata for FrameWithoutReader {
    type Reader = NeverReader;

    fn metadata_query_reader(&self) -> Result<NeverReader, HResult> {
        Err(E_FAIL)
    }
}

/// A decoder whose `GetContainerFormat` returns `container`.
struct FakeDecoder {
    container: Result<Guid, HResult>,
}

impl DecoderContainer for FakeDecoder {
    fn container_format(&self) -> Result<Guid, HResult> {
        self.container
    }
}

/// `IWICMetadataQueryReader` of the W4-11a wrapper as a [`MetadataQueryReader`].
struct NativeReader(Com<IWICMetadataQueryReader>);

impl MetadataQueryReader for NativeReader {
    fn get_metadata_by_name(&self, name: &str, value: &mut PropVariant) -> HResult {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call; `value` is a live 24-byte, 8-byte aligned buffer, the
        // size and alignment of a PROPVARIANT, that the callee fills.
        let result = unsafe {
            self.0
                .GetMetadataByName(PCWSTR(wide.as_ptr()), value.as_mut_ptr().cast::<PROPVARIANT>())
        };
        match result {
            Ok(()) => 0,
            Err(error) => error.code().0,
        }
    }
}

/// A frame of the W4-11a wrapper as a [`FrameMetadata`].
struct NativeFrame(Com<IWICBitmapFrameDecode>);

impl FrameMetadata for NativeFrame {
    type Reader = NativeReader;

    fn metadata_query_reader(&self) -> Result<NativeReader, HResult> {
        get_metadata_query_reader(&self.0)
            .map(NativeReader)
            .map_err(|error| error.hresult())
    }
}

/// A decoder of the W4-11a wrapper as a [`DecoderContainer`].
struct NativeDecoder(Com<IWICBitmapDecoder>);

impl DecoderContainer for NativeDecoder {
    fn container_format(&self) -> Result<Guid, HResult> {
        // SAFETY: plain COM call on a live decoder interface, no pointers passed.
        let guid = unsafe { self.0.GetContainerFormat() }.map_err(|error| error.code().0)?;
        // .NET `Guid.ToByteArray()` order: the first three fields little endian, the last eight bytes as they are.
        let mut bytes = [0u8; 16];
        bytes[..4].copy_from_slice(&guid.data1.to_le_bytes());
        bytes[4..6].copy_from_slice(&guid.data2.to_le_bytes());
        bytes[6..8].copy_from_slice(&guid.data3.to_le_bytes());
        bytes[8..].copy_from_slice(&guid.data4);
        Ok(Guid(bytes))
    }
}

/// Native read of frame 0 of `bytes` through the W4-11a wrappers: `exif_ifd_root_of` once, then
/// `read_frame_metadata(frame, true, root)` for the same frame. Returns the root and the read.
fn read_frame0_native(bytes: &[u8]) -> (Option<&'static str>, MetadataRead) {
    let factory = create_factory().expect("WIC factory");
    let stream = ComStream::from_bytes(Arc::from(bytes));
    let decoder = create_decoder_from_stream(&factory, &stream.istream(), WICDecodeMetadataCacheOnDemand)
        .expect("decoder from stream");
    let frame = get_frame(&decoder, 0).expect("frame 0");
    let decoder = NativeDecoder(decoder);
    let root = exif_ifd_root_of(&decoder);
    let read = read_frame_metadata(&NativeFrame(frame), true, root);
    (root, read)
}

/// Appends one JPEG segment `FF <marker> <length> <payload>`.
fn jpeg_segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&u16::try_from(payload.len() + 2).expect("segment length").to_be_bytes());
    out.extend_from_slice(payload);
}

/// A little-endian TIFF/EXIF block: IFD0 with Make (ASCII) and Orientation = 6 (the APP1 payload after `Exif\0\0`).
fn exif_block_with_make(make: &str) -> Vec<u8> {
    let mut text = make.as_bytes().to_vec();
    text.push(0);
    let mut out = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
    out.extend_from_slice(&2u16.to_le_bytes());
    // Make (0x010F), ASCII (2), count, offset 38 = 8 + 2 + 2 * 12 + 4.
    out.extend_from_slice(&0x010Fu16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&u32::try_from(text.len()).expect("make length").to_le_bytes());
    out.extend_from_slice(&38u32.to_le_bytes());
    // Orientation (0x0112), SHORT (3), count 1, value 6.
    out.extend_from_slice(&0x0112u16.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&[6, 0, 0, 0]);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&text);
    out
}

/// A JFIF JPEG with an APP1 EXIF block whose IFD0 Make is `make` and whose orientation is 6 (contract open question 4).
/// The image is a valid 1x1 baseline grayscale JPEG (all-ones quantisation table, one-code Huffman tables).
fn jpeg_with_make(make: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&exif_block_with_make(make));
    jpeg_segment(&mut out, 0xE1, &app1);
    let mut dqt = vec![0u8];
    dqt.extend_from_slice(&[1u8; 64]);
    jpeg_segment(&mut out, 0xDB, &dqt);
    jpeg_segment(&mut out, 0xC0, &[8, 0, 1, 0, 1, 1, 1, 0x11, 0]);
    for class_and_id in [0x00u8, 0x10] {
        let mut dht = vec![class_and_id, 1];
        dht.extend_from_slice(&[0u8; 15]);
        dht.push(0);
        jpeg_segment(&mut out, 0xC4, &dht);
    }
    jpeg_segment(&mut out, 0xDA, &[1, 1, 0x00, 0, 63, 0]);
    out.push(0x3F);
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

/// A little-endian 1x1 grayscale TIFF whose IFD0 Make is `make`.
fn tiff_with_make(make: &str) -> Vec<u8> {
    let mut text = make.as_bytes().to_vec();
    text.push(0);
    const ENTRIES: u32 = 10;
    let make_offset = 8 + 2 + ENTRIES * 12 + 4;
    let pixel_offset = make_offset + u32::try_from(text.len()).expect("make length");
    let entry = |tag: u16, kind: u16, count: u32, value: u32| -> Vec<u8> {
        let mut e = Vec::new();
        e.extend_from_slice(&tag.to_le_bytes());
        e.extend_from_slice(&kind.to_le_bytes());
        e.extend_from_slice(&count.to_le_bytes());
        e.extend_from_slice(&value.to_le_bytes());
        e
    };
    let mut out = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
    out.extend_from_slice(&u16::try_from(ENTRIES).expect("entries").to_le_bytes());
    out.extend(entry(256, 4, 1, 1)); // ImageWidth
    out.extend(entry(257, 4, 1, 1)); // ImageLength
    out.extend(entry(258, 3, 1, 8)); // BitsPerSample
    out.extend(entry(259, 3, 1, 1)); // Compression: none
    out.extend(entry(262, 3, 1, 1)); // Photometric: BlackIsZero
    out.extend(entry(
        271,
        2,
        u32::try_from(text.len()).expect("make length"),
        make_offset,
    )); // Make
    out.extend(entry(273, 4, 1, pixel_offset)); // StripOffsets
    out.extend(entry(277, 3, 1, 1)); // SamplesPerPixel
    out.extend(entry(278, 4, 1, 1)); // RowsPerStrip
    out.extend(entry(279, 4, 1, 1)); // StripByteCounts
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&text);
    out.push(0x80);
    out
}

/// A minimal 1x1 grayscale PNG (no EXIF container).
fn png_bytes() -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&u32::try_from(data.len()).expect("chunk length").to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    chunk(&mut out, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]);
    // zlib: header, one stored block holding the filter byte and one pixel, Adler-32 of [0, 0] = 0x00030001.
    chunk(
        &mut out,
        b"IDAT",
        &[
            0x78, 0x01, 0x01, 0x02, 0x00, 0xFD, 0xFF, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01,
        ],
    );
    chunk(&mut out, b"IEND", &[]);
    out
}

// ---------------------------------------------------------------------------------------------------------------
// WicMetadataIndependenceTests
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn exif_failure_keeps_the_orientation() {
    // C#: WicMetadataIndependenceTests.ExifFailure_KeepsTheOrientation
    let query = |name: &str| -> Result<Option<QueryValue>, QueryFailed> {
        if name == ORIENTATION_QUERY {
            Ok(Some(QueryValue::U16(6)))
        } else {
            Err(QueryFailed)
        }
    };

    let read = read_metadata_values(query, true, Some(JPEG_IFD_ROOT));

    assert_eq!(read.orientation, 6);
    assert_eq!(read.exif, None);
}

#[test]
fn orientation_failure_keeps_the_exif() {
    // C#: WicMetadataIndependenceTests.OrientationFailure_KeepsTheExif
    let query = |name: &str| -> Result<Option<QueryValue>, QueryFailed> {
        if name.contains("274") || name == WINDOWS_ORIENTATION_QUERY {
            Err(QueryFailed)
        } else if name.ends_with("{ushort=271}") {
            Ok(Some(QueryValue::Text("Canon".to_owned())))
        } else {
            Ok(None)
        }
    };

    let read = read_metadata_values(query, true, Some(JPEG_IFD_ROOT));

    assert_eq!(read.orientation, 1);
    assert_eq!(camera_make(&read), Some("Canon"));
}

// ---------------------------------------------------------------------------------------------------------------
// WicDirectInternalsMutationTests: PROPVARIANT shapes (ReadVariant)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn read_variant_wide_string_is_read_unless_the_pointer_is_null() {
    // C#: WicDirectInternalsMutationTests.ReadVariant_WideString_IsReadUnlessThePointerIsNull
    let memory = FakeMemory::default().with(TEXT_ADDR, utf16z("Canon"));
    let mut raw = raw_with_vt(31);
    put_pointer(&mut raw, 8, TEXT_ADDR);

    assert_eq!(
        decode_variant(&raw, &memory),
        Some(QueryValue::Text("Canon".to_owned()))
    );
    assert_eq!(decode_variant(&raw_with_vt(31), &memory), None);
}

#[test]
fn read_variant_ansi_string_is_read_unless_the_pointer_is_null() {
    // C#: WicDirectInternalsMutationTests.ReadVariant_AnsiString_IsReadUnlessThePointerIsNull
    let memory = FakeMemory::default().with(TEXT_ADDR, b"Nikon\0".to_vec());
    let mut raw = raw_with_vt(30);
    put_pointer(&mut raw, 8, TEXT_ADDR);

    assert_eq!(
        decode_variant(&raw, &memory),
        Some(QueryValue::Text("Nikon".to_owned()))
    );
    assert_eq!(decode_variant(&raw_with_vt(30), &memory), None);
}

#[test]
fn read_variant_scalar_shapes_match_what_wpf_returns() {
    // C#: WicDirectInternalsMutationTests.ReadVariant_ScalarShapes_MatchWhatWpfReturns
    let memory = FakeMemory::default();

    assert_eq!(
        decode_variant(&raw_with_scalar(2, -5), &memory),
        Some(QueryValue::I16(-5))
    );
    assert_eq!(
        decode_variant(&raw_with_scalar(3, 70_000), &memory),
        Some(QueryValue::I32(70_000))
    );
    assert_eq!(
        decode_variant(&raw_with_scalar(17, 200), &memory),
        Some(QueryValue::U8(200))
    );
    assert_eq!(
        decode_variant(&raw_with_scalar(18, 6), &memory),
        Some(QueryValue::U16(6))
    );
    assert_eq!(
        decode_variant(&raw_with_scalar(19, 4_000_000_000), &memory),
        Some(QueryValue::U32(4_000_000_000))
    );
    assert_eq!(
        decode_variant(&raw_with_scalar(20, -9), &memory),
        Some(QueryValue::I64(-9))
    );
    assert_eq!(
        decode_variant(&raw_with_scalar(21, 0x0000_0002_0000_0001), &memory),
        Some(QueryValue::U64(0x0000_0002_0000_0001))
    );
    assert_eq!(decode_variant(&raw_with_vt(0), &memory), None);
}

#[test]
fn read_variant_vector_returns_the_first_element_only() {
    // C#: WicDirectInternalsMutationTests.ReadVariant_Vector_ReturnsTheFirstElementOnly
    let memory = FakeMemory::default().with(ELEMENT_ADDR, vec![6, 0, 9, 0]);
    let raw = raw_vector(VT_VECTOR | 18, 2, Some(ELEMENT_ADDR));

    assert_eq!(decode_variant(&raw, &memory), Some(QueryValue::U16s(vec![6])));
}

#[test]
fn read_variant_vector_without_elements_is_null_even_with_a_pointer() {
    // C#: WicDirectInternalsMutationTests.ReadVariant_VectorWithoutElements_IsNullEvenWithAPointer
    // Rows: count 0 and count -1 (the count is a signed int32).
    let memory = FakeMemory::default().with(ELEMENT_ADDR, vec![9, 0, 9, 0, 9, 0, 9, 0]);

    for count in [0, -1] {
        let raw = raw_vector(VT_VECTOR | 18, count, Some(ELEMENT_ADDR));
        assert_eq!(decode_variant(&raw, &memory), None, "count {count}");
    }
}

#[test]
fn read_variant_vector_with_count_but_no_pointer_is_null() {
    // C#: WicDirectInternalsMutationTests.ReadVariant_VectorWithCountButNoPointer_IsNull
    let memory = FakeMemory::default();

    assert_eq!(decode_variant(&raw_vector(VT_VECTOR | 18, 3, None), &memory), None);
}

// ---------------------------------------------------------------------------------------------------------------
// WicDirectInternalsMutationTests: orientation tag range (ReadMetadataValues)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn read_metadata_values_exif_orientation_tag_is_accepted_only_in_one_to_eight() {
    // C#: WicDirectInternalsMutationTests.ReadMetadataValues_ExifOrientationTag_IsAcceptedOnlyInOneToEight
    // Rows (tag, expected). The C# passes -1 as an int32 and the others as ushort.
    let rows: [(QueryValue, i32); 8] = [
        (QueryValue::U16(0), 1),
        (QueryValue::U16(1), 1),
        (QueryValue::U16(2), 2),
        (QueryValue::U16(7), 7),
        (QueryValue::U16(8), 8),
        (QueryValue::U16(9), 1),
        (QueryValue::I32(-1), 1),
        (QueryValue::U16(65_535), 1),
    ];

    for (tag, expected) in rows {
        assert_eq!(orientation_with(Some(tag.clone()), None, true), expected, "tag {tag:?}");
    }
}

#[test]
fn read_metadata_values_windows_fallback_is_accepted_only_in_one_to_eight_when_the_exif_tag_is_unusable() {
    // C#: WicDirectInternalsMutationTests.ReadMetadataValues_WindowsFallback_IsAcceptedOnlyInOneToEightWhenTheExifTagIsUnusable
    // Rows (windows tag, expected); each row runs with three EXIF shapes: absent, 0, 9.
    let rows: [(u16, i32); 5] = [(0, 1), (1, 1), (6, 6), (8, 8), (9, 1)];
    let exif_shapes: [Option<QueryValue>; 3] = [None, Some(QueryValue::U16(0)), Some(QueryValue::U16(9))];

    for (tag, expected) in rows {
        for exif in exif_shapes.iter() {
            let windows = Some(QueryValue::U16(tag));
            assert_eq!(
                orientation_with(exif.clone(), windows, true),
                expected,
                "windows {tag} exif {exif:?}"
            );
        }
    }
}

#[test]
fn read_metadata_values_usable_exif_tag_wins_over_the_windows_property() {
    // C#: WicDirectInternalsMutationTests.ReadMetadataValues_UsableExifTag_WinsOverTheWindowsProperty
    assert_eq!(
        orientation_with(Some(QueryValue::U16(3)), Some(QueryValue::U16(6)), true),
        3
    );
}

#[test]
fn read_metadata_values_exif_tag_in_range_never_falls_through_to_the_windows_property() {
    // C#: WicDirectInternalsMutationTests.ReadMetadataValues_ExifTagInRange_NeverFallsThroughToTheWindowsProperty
    // Rows (exif, windows, expected).
    let rows: [(u16, u16, i32); 3] = [(1, 6, 1), (8, 3, 8), (2, 6, 2)];

    for (exif, windows, expected) in rows {
        let got = orientation_with(Some(QueryValue::U16(exif)), Some(QueryValue::U16(windows)), true);
        assert_eq!(got, expected, "exif {exif} windows {windows}");
    }
}

#[test]
fn read_metadata_values_orientation_not_requested_is_never_queried() {
    // C#: WicDirectInternalsMutationTests.ReadMetadataValues_OrientationNotRequested_IsNeverQueried
    let query = |_name: &str| -> Result<Option<QueryValue>, QueryFailed> { panic!("must not be queried") };

    let read = read_metadata_values(query, false, None);

    assert_eq!(read.orientation, 1);
    assert_eq!(read.exif, None);
}

// ---------------------------------------------------------------------------------------------------------------
// WicDirectFaultInjectionTests: ReadFrameMetadata / ExifIfdRootOf
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn read_frame_metadata_frame_without_metadata_reader_is_orientation_one_and_no_exif() {
    // C#: WicDirectFaultInjectionTests.ReadFrameMetadata_FrameWithoutMetadataReader_IsOrientationOneAndNoExif
    // Rows: readOrientation true and false.
    for read_orientation in [true, false] {
        let read = read_frame_metadata(&FrameWithoutReader, read_orientation, Some(JPEG_IFD_ROOT));

        assert_eq!(
            read,
            MetadataRead {
                orientation: 1,
                exif: None
            },
            "readOrientation {read_orientation}"
        );
    }
}

// C#: WicDirectFaultInjectionTests.ReadFrameMetadata_OutOfMemory_IsNotSwallowed is not ported (type system: Rust
// aborts on out-of-memory and has no catch branch to test).

#[test]
fn exif_ifd_root_of_container_format_fails_is_none() {
    // C#: WicDirectFaultInjectionTests.ExifIfdRootOf_ContainerFormatFails_IsNull
    let decoder = FakeDecoder { container: Err(E_FAIL) };

    assert_eq!(exif_ifd_root_of(&decoder), None);
}

#[test]
fn exif_ifd_root_of_unknown_container_is_none() {
    // C#: WicDirectFaultInjectionTests.ExifIfdRootOf_UnknownContainer_IsNull
    let decoder = FakeDecoder {
        container: Ok(Guid([0; 16])),
    };

    assert_eq!(exif_ifd_root_of(&decoder), None);
}

// ---------------------------------------------------------------------------------------------------------------
// Golden-driven tests
// ---------------------------------------------------------------------------------------------------------------

/// The `constants` row of `wic-exif-values`: the two container GUIDs (.NET text) and the root each one maps to.
#[test]
fn wic_exif_constants_golden() {
    let golden = golden::load("wic-exif-values");
    let row = golden
        .cases
        .iter()
        .find(|case| case["name"] == "constants")
        .expect("constants row");

    assert_eq!(
        guid_text(&CONTAINER_FORMAT_JPEG),
        row["jpeg"].as_str().expect("jpeg guid")
    );
    assert_eq!(
        guid_text(&CONTAINER_FORMAT_TIFF),
        row["tiff"].as_str().expect("tiff guid")
    );
    assert_eq!(exif_ifd_root_for_container(&CONTAINER_FORMAT_JPEG), Some(JPEG_IFD_ROOT));
    assert_eq!(exif_ifd_root_for_container(&CONTAINER_FORMAT_TIFF), Some(TIFF_IFD_ROOT));
    assert_eq!(exif_ifd_root_for_container(&Guid([0; 16])), None);
}

/// `wic-exif-values`: 62 cases. Each answers the queries from `golden-inputs`; the recorded query order, the
/// orientation and the summary must match the oracle exactly.
#[test]
fn wic_exif_values_golden_cases() {
    let inputs = load_inputs("wic-exif-values.json");
    let outputs = golden::load("wic-exif-values");
    let cases = inputs["cases"].as_array().expect("input cases");
    assert_eq!(cases.len(), 62);

    // Every mismatch is collected so one failing row does not hide the others.
    let mut mismatches = Vec::new();
    for input in cases {
        let name = input["name"].as_str().expect("case name");
        let expected = outputs
            .cases
            .iter()
            .find(|case| case["name"] == name)
            .unwrap_or_else(|| panic!("no golden row for {name}"));
        let answers: BTreeMap<String, Answer> = input["answers"]
            .as_object()
            .expect("answers")
            .iter()
            .map(|(key, answer)| (key.clone(), parse_answer(answer)))
            .collect();
        let recorded = RefCell::new(Vec::<String>::new());
        let query = |query_name: &str| -> Result<Option<QueryValue>, QueryFailed> {
            recorded.borrow_mut().push(query_name.to_owned());
            match answers.get(query_name) {
                None => Ok(None),
                Some(Answer::Throw) => Err(QueryFailed),
                Some(Answer::Value(value)) => Ok(Some(value.clone())),
            }
        };
        let root = input["root"].as_str();

        let read = read_metadata_values(query, input["read_orientation"].as_bool().expect("flag"), root);

        let queried = recorded.into_inner();
        let got = json!({
            "queried": queried,
            "orientation": read.orientation,
            "summary": summary_json(read.exif.as_ref()),
        });
        let want = json!({
            "queried": expected["queried"],
            "orientation": expected["orientation"],
            "summary": expected["summary"],
        });
        if got != want {
            mismatches.push(format!("{name}: got {got} want {want}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of 62 rows differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

/// `wic-exif-variant`: 59 cases. Each builds the 24-byte PROPVARIANT and its payload memory; the decoded `(type,
/// value)` must match the oracle exactly.
#[test]
fn wic_exif_variant_golden_cases() {
    let inputs = load_inputs("wic-exif-variant.json");
    let outputs = golden::load("wic-exif-variant");
    let cases = inputs["cases"].as_array().expect("input cases");
    assert_eq!(cases.len(), 59);

    // Every mismatch is collected so one failing row does not hide the others.
    let mut mismatches = Vec::new();
    for input in cases {
        let name = input["name"].as_str().expect("case name");
        let expected = outputs
            .cases
            .iter()
            .find(|case| case["name"] == name)
            .unwrap_or_else(|| panic!("no golden row for {name}"));
        let (raw, memory) = variant_from_golden(input);

        let got = describe(decode_variant(&raw, &memory));
        let want = (expected["type"].clone(), expected["value"].clone());
        if got != want {
            mismatches.push(format!("{name}: got {got:?} want {want:?}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of 59 rows differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Native tests (WIC; run with `-- --ignored`)
// ---------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs WIC"]
fn decode_tiff_with_exif_make_reads_it_through_the_tiff_ifd_root() {
    // C#: WicDirectInternalsMutationTests.Decode_TiffWithExifMake_ReadsItThroughTheTiffIfdRoot
    let (root, read) = read_frame0_native(&tiff_with_make("TiffMaker"));

    assert_eq!(root, Some(TIFF_IFD_ROOT));
    assert_eq!(camera_make(&read), Some("TiffMaker"));
}

#[test]
#[ignore = "native: needs WIC"]
fn decode_jpeg_with_exif_make_reads_it_through_the_jpeg_ifd_root() {
    // C#: WicDirectInternalsMutationTests.Decode_JpegWithExifMake_ReadsItThroughTheJpegIfdRoot
    let (root, read) = read_frame0_native(&jpeg_with_make("JpegMaker"));

    assert_eq!(root, Some(JPEG_IFD_ROOT));
    assert_eq!(camera_make(&read), Some("JpegMaker"));
}

#[test]
#[ignore = "native: needs WIC"]
fn decode_png_without_exif_container_has_no_exif() {
    // C#: WicDirectInternalsMutationTests.Decode_PngWithoutExifContainer_HasNoExif
    let (root, read) = read_frame0_native(&png_bytes());

    assert_eq!(root, None);
    assert_eq!(read.exif, None);
}

// --- Repair round 1: mutants missed by the first verifier (raw reads, clear, trait-level orientation) ---
mod repair_round_1 {
    use pr_core::catalog::explorer_types::Guid;
    use pr_imaging::exif_query::{JPEG_IFD_ROOT, TIFF_IFD_ROOT};
    use pr_platform_win::wic::exif_reader::{
        CONTAINER_FORMAT_JPEG, CONTAINER_FORMAT_TIFF, DecoderContainer, FrameMetadata, HResult, MetadataQueryReader,
        PROP_VARIANT_DATA_OFFSET, PROP_VARIANT_SIZE, PropVariant, RawMemory, VariantMemory, exif_ifd_root_of,
        read_frame_metadata, read_orientation,
    };

    struct FixedContainer(Guid);
    impl DecoderContainer for FixedContainer {
        fn container_format(&self) -> Result<Guid, HResult> {
            Ok(self.0)
        }
    }

    #[test]
    fn exif_ifd_root_of_maps_jpeg_and_tiff() {
        assert_eq!(
            exif_ifd_root_of(&FixedContainer(CONTAINER_FORMAT_JPEG)),
            Some(JPEG_IFD_ROOT)
        );
        assert_eq!(
            exif_ifd_root_of(&FixedContainer(CONTAINER_FORMAT_TIFF)),
            Some(TIFF_IFD_ROOT)
        );
    }

    /// Answers the orientation query with VT_UI2 = 6 and the given HRESULT (S_OK, S_FALSE or a failure).
    struct OrientationReader(HResult);
    impl MetadataQueryReader for OrientationReader {
        fn get_metadata_by_name(&self, name: &str, value: &mut PropVariant) -> HResult {
            if name == "/app1/ifd/{ushort=274}" {
                let pointer = value.as_mut_ptr();
                // SAFETY: `pointer` addresses the 24-byte PROPVARIANT buffer; offsets 0..2 (vt) and 8..10 (value) are inside it.
                unsafe {
                    *pointer = 18; // vt = VT_UI2 (18)
                    *pointer.add(PROP_VARIANT_DATA_OFFSET) = 6;
                }
                return self.0;
            }
            -1
        }
    }
    struct OrientationFrame(HResult);
    impl FrameMetadata for OrientationFrame {
        type Reader = OrientationReader;
        fn metadata_query_reader(&self) -> Result<OrientationReader, HResult> {
            Ok(OrientationReader(self.0))
        }
    }

    #[test]
    fn read_orientation_through_reader_ok_and_s_false_and_failure() {
        assert_eq!(read_orientation(&OrientationFrame(0)), 6);
        assert_eq!(read_orientation(&OrientationFrame(1)), 6);
        assert_eq!(read_orientation(&OrientationFrame(-1)), 1);
        assert_eq!(read_frame_metadata(&OrientationFrame(0), true, None).orientation, 6);
    }

    #[test]
    fn clear_zeroes() {
        let mut bytes = [0u8; PROP_VARIANT_SIZE];
        bytes[0] = 2;
        bytes[8] = 7;
        let mut variant = PropVariant::from_bytes(bytes);
        variant.clear();
        assert_eq!(variant.as_bytes(), &[0u8; PROP_VARIANT_SIZE]);
    }

    #[test]
    fn raw_memory_reads() {
        // SAFETY: every address passed below comes from a live local that is read inside its length.
        let memory = unsafe { RawMemory::new() };
        let ansi = b"Canon\0";
        assert_eq!(memory.utf8_z(ansi.as_ptr() as usize), "Canon");
        let wide: Vec<u16> = "Ab\u{e9}\0xyz".encode_utf16().collect();
        assert_eq!(memory.utf16_z(wide.as_ptr() as usize), "Ab\u{e9}");
        let u16s = [0x1234u16, 9];
        assert_eq!(memory.u16_at(u16s.as_ptr() as usize), 0x1234);
        let u32s = [0xDEAD_BEEFu32];
        assert_eq!(memory.u32_at(u32s.as_ptr() as usize), 0xDEAD_BEEF);
        let u64s = [0x0123_4567_89AB_CDEFu64];
        assert_eq!(memory.u64_at(u64s.as_ptr() as usize), 0x0123_4567_89AB_CDEF);
    }
}
