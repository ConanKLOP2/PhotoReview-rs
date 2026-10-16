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
use pr_test_support::golden;
use serde_json::{Value, json};

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
        Some(QueryValue::U64s(v)) => (json!("u64[]"), json!(v.iter().map(u64::to_string).collect::<Vec<_>>())),
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

/// Native read of frame 0 of `bytes` through the W4-11a wrappers: `exif_ifd_root_of` once, then
/// `read_frame_metadata(frame, true, root)` for the same frame. Returns the root and the read.
fn read_frame0_native(_bytes: &[u8]) -> (Option<&'static str>, MetadataRead) {
    todo!(
        "W4-11d porter: W4-11a create_factory, create_decoder_from_stream(ComStream::from_bytes), get_frame(0); \
         adapters implement DecoderContainer / FrameMetadata over the wrappers; then exif_ifd_root_of + read_frame_metadata"
    )
}

/// A JFIF JPEG with an APP1 EXIF block whose IFD0 Make is `make` and whose orientation is 6 (contract open question 4).
fn jpeg_with_make(_make: &str) -> Vec<u8> {
    todo!("W4-11d porter: hand-built JFIF + APP1 bytes (Make, orientation 6), or a W4-03a fixture builder")
}

/// A little-endian TIFF whose IFD0 Make is `make`.
fn tiff_with_make(_make: &str) -> Vec<u8> {
    todo!("W4-11d porter: hand-built TIFF bytes with IFD0 Make")
}

/// A minimal PNG (no EXIF container).
fn png_bytes() -> Vec<u8> {
    todo!("W4-11d porter: a minimal PNG, or a W4-03a fixture builder")
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
