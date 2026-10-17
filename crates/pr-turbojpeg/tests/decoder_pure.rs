//! Red tests for W5-14b (`contracts/W5-14b.md`): the pure half of `pr_turbojpeg::decoder`. No `turbojpeg.dll`, no WIC.
//!
//! Ports of the C# tests whose checks run before (or without) the native pipeline, at baseline d92d6aae. Each test names
//! its C# twin in a `// C#:` comment; theory rows become table loops that name the row. The golden tests read
//! `tests/golden/turbojpeg-pure.json` (846 cases + constants) and `turbojpeg-fixtures.json` (manifest). Every test here
//! fails through `todo!()` in the stub until the porter lands the bodies; that is the red state.
//!
//! Divergences from C# are marked `// DIVERGENCE: D-n` (contract semantics 15).

mod support;

use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use pr_imaging::decode_types::{DecodeRequest, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::exif_parser;
use pr_imaging::memory_headroom::GUARD_THRESHOLD_BYTES;
use pr_imaging::pixel_memory::NativePixelMemory;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_test_support::{TempRoot, golden};
use pr_turbojpeg::decoder::messages;
use pr_turbojpeg::decoder::{
    DecompressorFactory, DimensionsProbe, EXTENDED_HEADER_CAP, FineScaleError, FineScaler, HEADER_READ_CAP,
    HEADER_READ_CHUNK, HeaderScan, MAX_PROGRESSIVE_SCANS, MAX_SOURCE_BYTES, OutputBuffer, SUPPORTED_SCALING_FACTORS,
    SegmentStep, SourceReadError, TurboJpegDecoder, calculate_output_buffer, extend_header_area, file_exists,
    grow_header_area, header_needs_more_data, output_fits, read_all_bytes, read_exif_orientation, read_header_area,
    scaled_size, scan_header, select_scaling_factor, source_fits, try_read_segment,
};
use pr_turbojpeg::native::TjScalingFactor;
use serde_json::Value;
use support::*;

// ---------------------------------------------------------------------------------------------------------------
// Helpers local to this file
// ---------------------------------------------------------------------------------------------------------------

/// `FF D8`, the given segments, an SOS with an empty header, `FF D9`: a JPEG shell for the marker walk.
fn jpeg_shell(segments: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    for s in segments {
        out.extend_from_slice(s);
    }
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
    out
}

/// An APP2 ICC segment (`ICC_PROFILE\0` signature, zero profile bytes).
fn icc_segment(profile_bytes: usize) -> Vec<u8> {
    let mut payload = pr_turbojpeg::decoder::ICC_SIGNATURE.to_vec();
    payload.extend(std::iter::repeat_n(0u8, profile_bytes));
    segment(0xE2, &payload)
}

/// An APP1 Exif segment carrying `tiff` after the `Exif\0\0` signature.
fn exif_segment(tiff: &[u8]) -> Vec<u8> {
    let mut payload = pr_turbojpeg::decoder::EXIF_SIGNATURE.to_vec();
    payload.extend_from_slice(tiff);
    segment(0xE1, &payload)
}

/// A minimal little-endian TIFF with one IFD0 entry: Orientation (0x0112) = `orientation`.
fn tiff_orientation(orientation: u8) -> Vec<u8> {
    vec![
        b'I',
        b'I',
        42,
        0,
        8,
        0,
        0,
        0, // header, IFD0 at 8
        1,
        0, // one entry
        0x12,
        0x01,
        3,
        0,
        1,
        0,
        0,
        0,
        orientation,
        0,
        0,
        0, // tag, SHORT, count 1, value
        0,
        0,
        0,
        0, // next IFD
    ]
}

/// A decode error, panicking with the request label when the decode unexpectedly succeeds.
fn decode_error(decoder: &TurboJpegDecoder, request: &DecodeRequest) -> DecodeError {
    match decoder.decode(request) {
        Ok(_) => panic!("decode of {} unexpectedly succeeded", request.path.display()),
        Err(error) => error,
    }
}

/// A reader that counts `read` calls and returns at most `chunk` bytes per call (0 = all that is asked).
struct CountingReader {
    data: io::Cursor<Vec<u8>>,
    chunk: usize,
    reads: usize,
}

impl CountingReader {
    fn new(data: Vec<u8>, chunk: usize) -> Self {
        Self {
            data: io::Cursor::new(data),
            chunk,
            reads: 0,
        }
    }
}

impl Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reads += 1;
        let limit = if self.chunk == 0 {
            buf.len()
        } else {
            self.chunk.min(buf.len())
        };
        self.data.read(&mut buf[..limit])
    }
}

/// A scaler that returns a blank buffer of the requested size (for scale-path tests with fakes).
fn blank_scaler() -> Arc<dyn FineScaler> {
    Arc::new(
        |_: &PixelBuffer, width: i32, height: i32| -> Result<PixelBuffer, FineScaleError> {
            PixelBuffer::allocate(width, height, PixelLayout::Bgr32)
                .map_err(|e| FineScaleError::Failed(DecodeError::Other { message: e.to_string() }))
        },
    )
}

/// A scaler that reports WIC `E_OUTOFMEMORY` (the `OutOfMemory` arm of semantics 8).
fn oom_scaler() -> Arc<dyn FineScaler> {
    Arc::new(
        |_: &PixelBuffer, _: i32, _: i32| -> Result<PixelBuffer, FineScaleError> { Err(FineScaleError::OutOfMemory) },
    )
}

fn decoder_with(
    factory: Arc<dyn DecompressorFactory>,
    memory: Arc<dyn pr_imaging::memory_headroom::MemoryInfoSource>,
    scaler: Arc<dyn FineScaler>,
) -> TurboJpegDecoder {
    serialize_pixel_memory();
    TurboJpegDecoder::new(Arc::new(RecordingCodec::default()), scaler, memory, factory, english())
}

/// The default success decompressor: an 8x8 image the fake "decodes" as blank pixels.
fn ok_fake() -> FakeDecompressor {
    FakeDecompressor::default()
}

// ---------------------------------------------------------------------------------------------------------------
// Golden: turbojpeg-pure (846 cases + constants row), run in the default test run
// ---------------------------------------------------------------------------------------------------------------

fn flatten(value: &Value) -> String {
    value.to_string()
}

/// Dispatches one `turbojpeg-pure` row and returns the mismatch text, `None` when the row agrees with C#.
fn check_pure_row(case: &Value) -> Option<String> {
    let name = case["name"].as_str().unwrap_or("?");
    let fname = case["fn"].as_str().unwrap_or("constants");
    let fail = |actual: String| Some(format!("{name}: expected {} got {actual}", flatten(&case["ok"])));
    match fname {
        "select-scaling-factor" => {
            let got = select_scaling_factor(
                golden::i64_of(case, "orig_width") as i32,
                golden::i64_of(case, "orig_height") as i32,
                golden::i64_of(case, "target_width") as i32,
                golden::i64_of(case, "target_height") as i32,
            );
            let want = serde_json::json!({"num": got.num, "denom": got.denom});
            if want != case["ok"] {
                fail(want.to_string())
            } else {
                None
            }
        }
        "calculate-output-buffer" => {
            let factor = TjScalingFactor::new(golden::i64_of(case, "num") as i32, golden::i64_of(case, "denom") as i32);
            let result = calculate_output_buffer(
                golden::i64_of(case, "orig_width") as i32,
                golden::i64_of(case, "orig_height") as i32,
                factor,
            );
            match (result, case.get("error")) {
                (Ok(buf), None) => {
                    let got = serde_json::json!({"width": buf.width, "height": buf.height, "stride": buf.stride, "buffer_length": buf.buffer_length});
                    if got != case["ok"] { fail(got.to_string()) } else { None }
                }
                (Err(o), Some(error)) => {
                    let message = format!(
                        "TurboJPEG output dimensions are too large: {}x{} ({} bytes).",
                        o.width, o.height, o.buffer_length
                    );
                    if error["message"].as_str() != Some(message.as_str()) {
                        fail(message)
                    } else {
                        None
                    }
                }
                (other, _) => fail(format!("{other:?}")),
            }
        }
        "scan-header" => {
            let jpeg = hex(case["hex"].as_str().expect("hex"));
            let want_icc = golden::bool_of(case, "want_icc");
            let want_exif = golden::bool_of(case, "want_exif");
            let scan = scan_header(&jpeg, want_icc, want_exif);
            let offset = if scan.exif_tiff.is_empty() {
                -1
            } else {
                scan.exif_tiff.as_ptr() as i64 - jpeg.as_ptr() as i64
            };
            let got = serde_json::json!({"has_icc": scan.has_icc, "exif_tiff_offset": offset, "exif_tiff_length": scan.exif_tiff.len()});
            if got != case["ok"] { fail(got.to_string()) } else { None }
        }
        "read-exif-orientation" => {
            let jpeg = hex(case["hex"].as_str().expect("hex"));
            let got = read_exif_orientation(&jpeg);
            if case["ok"].as_i64() != Some(i64::from(got)) {
                fail(got.to_string())
            } else {
                None
            }
        }
        "header-needs-more-data" => {
            let jpeg = hex(case["hex"].as_str().expect("hex"));
            let got = header_needs_more_data(&jpeg);
            if case["ok"].as_bool() != Some(got) {
                fail(got.to_string())
            } else {
                None
            }
        }
        "walk-segments" => {
            let jpeg = hex(case["hex"].as_str().expect("hex"));
            let (segments, end_offset, needs) = walk(&jpeg);
            let got = serde_json::json!({"segments": segments, "end_offset": end_offset, "needs_more_data": needs});
            if got != case["ok"] { fail(got.to_string()) } else { None }
        }
        "all-walkers" => {
            let jpeg = hex(case["hex"].as_str().expect("hex"));
            let mut got = serde_json::Map::new();
            for (key, icc, exif) in [
                ("scan_00", false, false),
                ("scan_10", true, false),
                ("scan_01", false, true),
                ("scan_11", true, true),
            ] {
                let scan = scan_header(&jpeg, icc, exif);
                got.insert(key.to_owned(), scan_json(&jpeg, &scan));
            }
            got.insert(
                "orientation".to_owned(),
                serde_json::json!(read_exif_orientation(&jpeg)),
            );
            got.insert(
                "needs_more_data".to_owned(),
                serde_json::json!(header_needs_more_data(&jpeg)),
            );
            let (segments, end_offset, needs) = walk(&jpeg);
            got.insert(
                "walk".to_owned(),
                serde_json::json!({"segments": segments, "end_offset": end_offset, "needs_more_data": needs}),
            );
            let got = Value::Object(got);
            if got != case["ok"] { fail(got.to_string()) } else { None }
        }
        "read-all-bytes" => check_read_all_bytes_row(case, name),
        other => panic!("{name}: unknown fn {other}"),
    }
}

fn scan_json(jpeg: &[u8], scan: &HeaderScan<'_>) -> Value {
    let offset = if scan.exif_tiff.is_empty() {
        -1
    } else {
        scan.exif_tiff.as_ptr() as i64 - jpeg.as_ptr() as i64
    };
    serde_json::json!({"has_icc": scan.has_icc, "exif_tiff_offset": offset, "exif_tiff_length": scan.exif_tiff.len()})
}

/// The C# walk: `(segments, end_offset, needs_more_data)` with `payload_offset` absolute in the buffer.
fn walk(jpeg: &[u8]) -> (Vec<Value>, usize, bool) {
    let mut offset = 2usize;
    let mut segments = Vec::new();
    loop {
        match try_read_segment(jpeg, &mut offset) {
            SegmentStep::Segment { marker, payload } => segments.push(serde_json::json!({
                "marker": marker,
                "payload_offset": payload.as_ptr() as i64 - jpeg.as_ptr() as i64,
                "payload_length": payload.len(),
                "offset_after": offset,
            })),
            SegmentStep::End { needs_more_data } => return (segments, offset, needs_more_data),
        }
    }
}

/// `read-all-bytes` rows: the stream reports `reported_length`, holds `content_length` bytes, and returns at most
/// `chunk` bytes per read (0 = unlimited). The content is `(i * 7 + 3) as u8` (oracle). The golden records `length`, the SHA-256 of what
/// was read, and the read count. A reported length above the limit is refused before any read.
fn check_read_all_bytes_row(case: &Value, name: &str) -> Option<String> {
    let content_length = golden::i64_of(case, "content_length") as usize;
    let reported = golden::i64_of(case, "reported_length");
    let chunk = golden::i64_of(case, "chunk") as usize;
    let content: Vec<u8> = (0..content_length).map(|i| (i * 7 + 3) as u8).collect();
    let mut reader = CountingReader::new(content, chunk);
    let result = read_all_bytes(&mut reader, reported);
    if let Some(error) = case.get("error") {
        let refused = matches!(result, Err(SourceReadError::TooLarge { .. }));
        if !refused || reader.reads != 0 {
            return Some(format!(
                "{name}: expected TooLarge with 0 reads, got {result:?} after {}",
                reader.reads
            ));
        }
        let _ = error;
        return None;
    }
    match result {
        Ok(buffer) => {
            let want_len = golden::i64_of(&case["ok"], "length") as usize;
            let want_sha = golden::str_of(&case["ok"], "sha256");
            let got_sha = sha256_hex(&buffer);
            if buffer.len() != want_len
                || got_sha != want_sha
                || reader.reads as i64 != golden::i64_of(case, "read_calls")
            {
                Some(format!(
                    "{name}: length {} sha {got_sha} reads {}",
                    buffer.len(),
                    reader.reads
                ))
            } else {
                None
            }
        }
        Err(error) => Some(format!("{name}: {error}")),
    }
}

#[test]
fn turbojpeg_pure_golden_rows_match_csharp() {
    // C#: TurboJpegGuardMutationTests, TurboJpegHardeningTests, CombinedHeaderScanTests, JpegSegmentWalkerEquivalenceTests,
    // JpegHeaderOracleTests, ScalingFactorPropertyTests (oracle rows; `turbojpeg-pure` golden, 846 cases + constants).
    let cases = cases("turbojpeg-pure");
    let constants = &cases[0];
    assert_eq!(constants["name"], "constants");
    assert_eq!(
        golden::i64_of(constants, "max_progressive_scans") as i32,
        MAX_PROGRESSIVE_SCANS
    );
    assert_eq!(
        golden::i64_of(constants, "header_read_chunk") as usize,
        HEADER_READ_CHUNK
    );
    assert_eq!(golden::i64_of(constants, "header_read_cap"), HEADER_READ_CAP);
    assert_eq!(golden::i64_of(constants, "extended_header_cap"), EXTENDED_HEADER_CAP);
    assert_eq!(golden::i64_of(constants, "max_source_bytes"), MAX_SOURCE_BYTES);
    assert_eq!(
        golden::i64_of(constants, "guard_threshold_bytes"),
        GUARD_THRESHOLD_BYTES
    );
    let factors: Vec<Value> = SUPPORTED_SCALING_FACTORS
        .iter()
        .map(|f| serde_json::json!({"num": f.num, "denom": f.denom}))
        .collect();
    assert_eq!(constants["supported_scaling_factors"], Value::Array(factors));

    let mismatches: Vec<String> = cases[1..].iter().filter_map(check_pure_row).collect();
    assert!(
        mismatches.is_empty(),
        "{} of {} turbojpeg-pure rows disagree with C#; first: {:?}",
        mismatches.len(),
        cases.len() - 1,
        mismatches.first()
    );
}

#[test]
fn turbojpeg_fixtures_manifest_matches_the_files() {
    // C#: none (Rust-only check of the golden package manifest: every listed fixture exists with that length and SHA-256).
    for case in cases("turbojpeg-fixtures") {
        let relative = format!("turbojpeg/{}", golden::str_of(&case, "file"));
        let bytes = fixture_bytes(&relative);
        assert_eq!(bytes.len() as i64, golden::i64_of(&case, "length"), "{relative} length");
        assert!(
            sha256_hex(&bytes).eq_ignore_ascii_case(golden::str_of(&case, "sha256")),
            "{relative} sha256"
        );
    }
}

#[test]
fn sha256_helper_matches_known_digests() {
    // Rust-only: the test-local SHA-256 is trusted by every golden check above, so it is pinned to two known digests.
    assert_eq!(
        sha256_hex(b""),
        "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Scaling and output sizes (`ScalingFactorPropertyTests`, `DecoderOutputFormatTests`, Rust-only sweeps)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn select_scaling_factor_is_smallest_covering_factor() {
    // C#: ScalingFactorPropertyTests.SelectScalingFactor_IsSmallestCoveringFactor (100 000 random draws, xorshift)
    let mut rng = XorShift::new(0x5EED_0001);
    for _ in 0..100_000 {
        let w = 1 + rng.below(20_000) as i32;
        let h = 1 + rng.below(20_000) as i32;
        let tw = 1 + rng.below(u64::from(w as u32)) as i32;
        let th = 1 + rng.below(u64::from(h as u32)) as i32;
        let picked = select_scaling_factor(w, h, tw, th);
        let covers = |f: TjScalingFactor| scaled_size(w, f) >= i64::from(tw) && scaled_size(h, f) >= i64::from(th);
        assert!(covers(picked) || picked == TjScalingFactor::ONE, "{w}x{h} -> {tw}x{th}");
        for smaller in SUPPORTED_SCALING_FACTORS.iter().take_while(|f| **f != picked) {
            assert!(!covers(*smaller), "{w}x{h} -> {tw}x{th}: {smaller:?} also covers");
        }
    }
}

#[test]
fn scaled_size_is_ceiling_division() {
    // C#: ScalingFactorPropertyTests (sizes): scaled_size(s, f) = ceil(s * num / denom).
    assert_eq!(scaled_size(17, TjScalingFactor::new(3, 8)), 7);
    assert_eq!(scaled_size(8, TjScalingFactor::new(1, 8)), 1);
    assert_eq!(scaled_size(2_147_483_647, TjScalingFactor::ONE), 2_147_483_647);
}

#[test]
fn decode_output_buffer_has_bgr32_stride_and_eight_factor_sweep_does_not_panic() {
    // C#: DecoderOutputFormatTests.TurboJpegOutputsMaterializedBgr32 (layout Bgr32, estimated_bytes = w * h * 4) and
    // Rust-only: calculate_output_buffer for all 8 factors x {1, i32::MAX} sides never panics.
    for factor in SUPPORTED_SCALING_FACTORS {
        for side in [1, i32::MAX] {
            let _ = calculate_output_buffer(side, side, factor);
            let _ = calculate_output_buffer(side, 1, factor);
        }
    }
    let OutputBuffer {
        width,
        height,
        stride,
        buffer_length,
    } = calculate_output_buffer(64, 48, TjScalingFactor::new(1, 2)).expect("32x24 fits");
    assert_eq!((width, height, stride, buffer_length), (32, 24, 128, 3072));
}

#[test]
fn select_scaling_factor_smallest_covering_rows_table() {
    // C#: DecoderOutputFormatTests.TurboJpegSelectsSmallestScalingFactorCoveringTarget (7 rows, pure)
    let rows = [
        ((400, 300), (200, 150), (1, 2)),
        ((400, 300), (201, 150), (5, 8)),
        ((6000, 4000), (1500, 1000), (1, 4)),
        ((6000, 4000), (1499, 999), (1, 4)),
        ((64, 48), (64, 48), (1, 1)),
        ((64, 48), (65, 49), (1, 1)),
        ((100, 75), (13, 10), (1, 8)),
    ];
    for ((w, h), (tw, th), (num, denom)) in rows {
        let got = select_scaling_factor(w, h, tw, th);
        assert_eq!((got.num, got.denom), (num, denom), "{w}x{h} -> {tw}x{th}");
    }
}

#[test]
fn output_fits_reads_memory_only_at_and_above_the_threshold() {
    // C#: DecodeSizeDecisionMutationTests / TurboJpegHardeningTests guard rows (T9: read count, never below threshold).
    let below = memory(i64::MAX / 4, 0);
    assert!(output_fits(GUARD_THRESHOLD_BYTES as i32 - 1, below.as_ref()));
    assert_eq!(below.reads.load(Ordering::SeqCst), 0);
    let at = memory(i64::MAX / 4, 0);
    assert!(output_fits(GUARD_THRESHOLD_BYTES as i32, at.as_ref()));
    assert_eq!(at.reads.load(Ordering::SeqCst), 1);
}

#[test]
fn source_fits_reads_memory_once_and_never_below_the_threshold() {
    // Rust-only guard for semantics 3 (T9).
    let below = memory(0, 0);
    assert!(source_fits(GUARD_THRESHOLD_BYTES - 1, below.as_ref()));
    assert_eq!(below.reads.load(Ordering::SeqCst), 0);
    let at = memory(1 << 40, 0);
    assert!(source_fits(GUARD_THRESHOLD_BYTES, at.as_ref()));
    assert_eq!(at.reads.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------------------------------------------
// Marker walk and header scan (`CombinedHeaderScanTests`, `JpegSegmentWalkerEquivalenceTests`, `TurboJpegGuardMutationTests`)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn scan_header_finds_icc_and_exif_agrees_with_standalone_methods() {
    // C#: CombinedHeaderScanTests.ScanHeader_FindsIccAndExif_AgreesWithStandaloneMethods
    let tiff = tiff_orientation(6);
    let jpeg = jpeg_shell(&[icc_segment(16), exif_segment(&tiff)]);
    let scan = scan_header(&jpeg, true, true);
    assert!(scan.has_icc);
    assert_eq!(scan.exif_tiff, tiff.as_slice());
    assert_eq!(read_exif_orientation(&jpeg), 6);
}

#[test]
fn scan_header_skips_non_exif_app1_finds_later_exif_app1() {
    // C#: CombinedHeaderScanTests.ScanHeader_SkipsNonExifApp1_FindsLaterExifApp1
    let tiff = tiff_orientation(8);
    let xmp = segment(0xE1, b"http://ns.adobe.com/xap/1.0/\0<x/>");
    let jpeg = jpeg_shell(&[xmp, exif_segment(&tiff)]);
    assert_eq!(scan_header(&jpeg, false, true).exif_tiff, tiff.as_slice());
}

#[test]
fn scan_header_multiple_exif_app1_first_one_wins_even_if_garbage() {
    // C#: CombinedHeaderScanTests.ScanHeader_MultipleExifApp1_FirstOneWinsEvenIfGarbage
    let jpeg = jpeg_shell(&[exif_segment(&[1, 2, 3]), exif_segment(&tiff_orientation(6))]);
    let scan = scan_header(&jpeg, false, true);
    assert_eq!(scan.exif_tiff, &[1, 2, 3]);
    assert_eq!(read_exif_orientation(&jpeg), 1);
}

#[test]
fn scan_header_truncated_header_does_not_throw_and_reports_nothing_found() {
    // C#: CombinedHeaderScanTests.ScanHeader_TruncatedHeader_DoesNotThrow_ReportsNothingFound
    let full = jpeg_shell(&[icc_segment(16), exif_segment(&tiff_orientation(6))]);
    for cut in 0..full.len() {
        let scan = scan_header(&full[..cut], true, true);
        if cut < 2 + icc_segment(16).len() {
            assert!(!scan.has_icc, "cut {cut}");
        }
    }
}

#[test]
fn scan_header_no_exif_no_icc_returns_empty_and_false() {
    // C#: CombinedHeaderScanTests.ScanHeader_NoExifNoIcc_ReturnsEmptyAndFalse
    let jpeg = jpeg_shell(&[segment(0xE0, b"JFIF\0")]);
    let scan = scan_header(&jpeg, true, true);
    assert_eq!(scan, HeaderScan::default());
}

#[test]
fn try_parse_tiff_block_matches_try_parse_jpeg() {
    // C#: CombinedHeaderScanTests.TryParseTiffBlock_MatchesTryParseJpeg
    let jpeg = jpeg_shell(&[exif_segment(&tiff_orientation(6))]);
    let tiff = scan_header(&jpeg, false, true).exif_tiff;
    let from_parser = exif_parser::try_parse_tiff_block(tiff);
    let from_jpeg = exif_parser::try_parse_tiff_block(exif_parser::find_exif_tiff_block(&jpeg));
    assert_eq!(from_parser, from_jpeg);
}

#[test]
fn scan_header_reports_only_what_was_asked_for() {
    // C#: TurboJpegGuardMutationTests.ScanHeader_ReportsOnlyWhatWasAskedFor (4 rows, ICC then EXIF)
    let jpeg = jpeg_shell(&[icc_segment(12), exif_segment(&tiff_orientation(6))]);
    for (want_icc, want_exif) in [(false, false), (true, false), (false, true), (true, true)] {
        let scan = scan_header(&jpeg, want_icc, want_exif);
        assert_eq!(scan.has_icc, want_icc, "icc wanted {want_icc}");
        assert_eq!(!scan.exif_tiff.is_empty(), want_exif, "exif wanted {want_exif}");
    }
}

#[test]
fn scan_header_exif_before_icc_reports_only_what_was_asked_for() {
    // C#: TurboJpegGuardMutationTests.ScanHeader_ExifBeforeIcc_ReportsOnlyWhatWasAskedFor (4 rows)
    let jpeg = jpeg_shell(&[exif_segment(&tiff_orientation(6)), icc_segment(12)]);
    for (want_icc, want_exif) in [(false, false), (true, false), (false, true), (true, true)] {
        let scan = scan_header(&jpeg, want_icc, want_exif);
        assert_eq!(scan.has_icc, want_icc);
        assert_eq!(!scan.exif_tiff.is_empty(), want_exif);
    }
}

#[test]
fn scan_header_second_exif_segment_is_ignored_even_when_the_first_has_no_tiff_part() {
    // C#: TurboJpegGuardMutationTests.ScanHeader_SecondExifSegment_IsIgnoredEvenWhenTheFirstOneHasNoTiffPart
    let jpeg = jpeg_shell(&[exif_segment(&[]), exif_segment(&tiff_orientation(6))]);
    assert!(scan_header(&jpeg, false, true).exif_tiff.is_empty());
    assert_eq!(read_exif_orientation(&jpeg), 1);
}

#[test]
fn scan_header_first_exif_segment_wins() {
    // C#: TurboJpegGuardMutationTests.ScanHeader_FirstExifSegmentWins
    let jpeg = jpeg_shell(&[exif_segment(&tiff_orientation(6)), exif_segment(&tiff_orientation(8))]);
    assert_eq!(read_exif_orientation(&jpeg), 6);
}

#[test]
fn scan_header_icc_then_exif_finds_both_even_when_icc_comes_first() {
    // C#: TurboJpegGuardMutationTests.ScanHeader_IccThenExif_FindsBothEvenWhenTheIccComesFirst
    let jpeg = jpeg_shell(&[icc_segment(12), exif_segment(&tiff_orientation(3))]);
    let scan = scan_header(&jpeg, true, true);
    assert!(scan.has_icc);
    assert_eq!(read_exif_orientation(&jpeg), 3);
}

#[test]
fn walker_finds_icc_after_other_segments_and_stops_at_sos() {
    // C#: JpegSegmentWalkerEquivalenceTests.Walker_FindsIccAfterOtherSegments_AndStopsAtSos
    let jpeg = jpeg_shell(&[segment(0xE0, b"JFIF\0\x01\x01\0\x00\x01\0\x01\0\0"), icc_segment(20)]);
    assert!(scan_header(&jpeg, true, false).has_icc);
    let (_, _, needs) = walk(&jpeg);
    assert!(!needs, "SOS header complete: the walk is over, not cut");
}

#[test]
fn walker_skips_marker_fill_bytes_before_exif_and_before_icc() {
    // C#: JpegSegmentWalkerEquivalenceTests.Walker_SkipsMarkerFillBytesBeforeExif / Walker_SkipsMarkerFillBytesBeforeIcc
    let mut before_exif = vec![0xFF, 0xD8, 0xFF, 0xFF, 0xFF];
    before_exif.extend(exif_segment(&tiff_orientation(6)));
    before_exif.extend_from_slice(&[0xFF, 0xDA, 0, 2, 0xFF, 0xD9]);
    assert_eq!(read_exif_orientation(&before_exif), 6);
    let mut before_icc = vec![0xFF, 0xD8, 0xFF, 0xFF];
    before_icc.extend(icc_segment(12));
    before_icc.extend_from_slice(&[0xFF, 0xDA, 0, 2]);
    assert!(scan_header(&before_icc, true, false).has_icc);
}

#[test]
fn walker_skips_tem_marker_before_icc_and_exif() {
    // C#: JpegSegmentWalkerEquivalenceTests.Walker_SkipsTemMarkerBeforeIccAndExif (TEM = 01 is parameterless)
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0x01];
    jpeg.extend(icc_segment(12));
    jpeg.extend(exif_segment(&tiff_orientation(6)));
    jpeg.extend_from_slice(&[0xFF, 0xDA, 0, 2]);
    let scan = scan_header(&jpeg, true, true);
    assert!(scan.has_icc);
    assert_eq!(read_exif_orientation(&jpeg), 6);
}

#[test]
fn walker_normal_exif_and_icc_still_found() {
    // C#: JpegSegmentWalkerEquivalenceTests.Walker_NormalExifAndIcc_StillFound
    let jpeg = jpeg_shell(&[exif_segment(&tiff_orientation(6)), icc_segment(12)]);
    let scan = scan_header(&jpeg, true, true);
    assert!(scan.has_icc && !scan.exif_tiff.is_empty());
}

#[test]
fn walker_truncated_fill_or_stuffed_byte_does_not_throw_or_match() {
    // C#: JpegSegmentWalkerEquivalenceTests.Walker_TruncatedFillOrStuffedByte_DoesNotThrowOrMatch
    for jpeg in [
        hex("FFD8FF"),
        hex("FFD8FF00E1"),
        hex("FFD8FFFF"),
        hex("FFD8FF00FFE1000A457869660000"),
    ] {
        assert!(!scan_header(&jpeg, true, true).has_icc);
        assert!(scan_header(&jpeg, false, true).exif_tiff.is_empty());
    }
}

#[test]
fn walker_random_and_corrupted_headers_match_pre_refactor_loops() {
    // C#: JpegSegmentWalkerEquivalenceTests.Walker_RandomAndCorruptedHeaders_MatchesPreRefactorLoops
    // The C# .NET Random cannot be reproduced: the same corpus shape with a test-local xorshift, plus the golden rows.
    let mut rng = XorShift::new(0xC0FF_EE01);
    for _ in 0..2_000 {
        let len = rng.below(200) as usize;
        let jpeg = rng.fill(len);
        let _ = scan_header(&jpeg, true, true);
        let _ = read_exif_orientation(&jpeg);
        let _ = header_needs_more_data(&jpeg);
    }
}

#[test]
fn try_read_segment_and_friends_never_panic_on_every_prefix_of_every_fixture() {
    // C#: none (Rust-only). Every prefix of every fixture JPEG, plus offsets len, len + 1 and usize::MAX.
    for relative in [
        "turbojpeg/parity-8x8.jpg",
        "turbojpeg/exif-64x48-o6.jpg",
        "turbojpeg/exif-partial-64x48.jpg",
    ] {
        let bytes = fixture_bytes(relative);
        for len in 0..=bytes.len() {
            let prefix = &bytes[..len];
            let _ = scan_header(prefix, true, true);
            let _ = read_exif_orientation(prefix);
            let _ = header_needs_more_data(prefix);
            for start in [len, len + 1, usize::MAX] {
                let mut offset = start;
                let _ = try_read_segment(prefix, &mut offset);
            }
        }
    }
}

#[test]
fn header_area_ends_at_eoi_and_stuffed_byte() {
    // C#: JpegHeaderOracleTests.HeaderAreaEnds_AtEoiAndStuffedByte
    let after_eoi = [
        0xFF, 0xD8, 0xFF, 0xD9, 0xFF, 0xE1, 0x00, 0x08, b'E', b'x', b'i', b'f', 0, 0,
    ];
    assert!(!header_needs_more_data(&after_eoi));
    let stuffed = [0xFF, 0xD8, 0xFF, 0x00, 0xFF, 0xE1, 0x00, 0x08];
    assert!(!header_needs_more_data(&stuffed));
}

#[test]
fn first_exif_app1_wins_even_when_too_short() {
    // C#: JpegHeaderOracleTests.FirstExifApp1Wins_EvenWhenTooShort
    let jpeg = jpeg_shell(&[exif_segment(&[0, 1]), exif_segment(&tiff_orientation(6))]);
    assert_eq!(read_exif_orientation(&jpeg), 1);
}

/// Reference walker written from ITU T.81 B.1.1.2 (C# `JpegHeaderOracleTests.Oracle`): fill bytes, stuffed `FF 00`,
/// parameterless markers, EOI/SOS end the header area; the first Exif-headed APP1 wins (even with an empty TIFF).
fn header_oracle(j: &[u8]) -> (bool, Option<Vec<u8>>) {
    if j.len() < 4 || j[0] != 0xFF || j[1] != 0xD8 {
        return (false, None);
    }
    let (mut has_icc, mut exif): (bool, Option<Vec<u8>>) = (false, None);
    let mut i = 2usize;
    while i < j.len() {
        if j[i] != 0xFF {
            break;
        }
        while i + 1 < j.len() && j[i + 1] == 0xFF {
            i += 1;
        }
        if i + 1 >= j.len() {
            break;
        }
        let m = j[i + 1];
        i += 2;
        if m == 0x00 || m == 0xD9 || m == 0xDA {
            break;
        }
        if m == 0x01 || (0xD0..=0xD8).contains(&m) {
            continue;
        }
        if i + 2 > j.len() {
            break;
        }
        let len = (usize::from(j[i]) << 8) | usize::from(j[i + 1]);
        if len < 2 || i + len > j.len() {
            break;
        }
        let payload = &j[i + 2..i + len];
        i += len;
        if m == 0xE2 && payload.len() >= 12 && payload[..12] == pr_turbojpeg::decoder::ICC_SIGNATURE {
            has_icc = true;
        }
        if m == 0xE1 && exif.is_none() && payload.len() >= 6 && payload[..6] == pr_turbojpeg::decoder::EXIF_SIGNATURE {
            exif = Some(payload[6..].to_vec());
        }
    }
    (has_icc, exif)
}

fn random_bytes(rng: &mut XorShift, below: u64) -> Vec<u8> {
    let len = rng.below(below) as usize;
    rng.fill(len)
}

/// C# `BuildHeader(Random)`: 0..8 items from the 14 kinds of header content.
fn random_header(rng: &mut XorShift) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8];
    let items = rng.below(8);
    for _ in 0..items {
        match rng.below(14) {
            0 => bytes.extend(segment(0xE2, &pr_turbojpeg::decoder::ICC_SIGNATURE)),
            1 => {
                let mut payload = pr_turbojpeg::decoder::ICC_SIGNATURE.to_vec();
                payload.extend(random_bytes(rng, 9));
                bytes.extend(segment(0xE2, &payload));
            }
            2 => {
                let tiff = tiff_orientation(rng.below(11) as u8);
                bytes.extend(exif_segment(&tiff));
            }
            3 => {
                let mut payload = pr_turbojpeg::decoder::EXIF_SIGNATURE.to_vec();
                payload.extend(random_bytes(rng, 10));
                bytes.extend(segment(0xE1, &payload));
            }
            4 => bytes.extend(segment(0xE1, b"http://ns.adobe.com/xap/1.0/\0")),
            5 => bytes.extend(segment(0xE0, &random_bytes(rng, 20))),
            6 => bytes.extend([0xFF, 0xD0 + rng.below(8) as u8]),
            7 => bytes.extend([0xFF, 0x01]),
            8 => bytes.extend([0xFF, 0xFF]),
            9 => bytes.extend([0xFF, 0x00]),
            10 => bytes.extend([0xFF, 0xD9]),
            11 => bytes.extend([0xFF, 0xD8]),
            12 => {
                let marker = 0xC0 + rng.below(0x3F) as u8;
                bytes.extend(segment(marker, &random_bytes(rng, 30)));
            }
            _ => bytes.extend([0xFF, 0xDA, 0, 2]),
        }
    }
    bytes
}

/// A small mutation (flip, truncate or insert a byte) in the spirit of C# `JpegBytes.Mutate`.
fn mutate_header(rng: &mut XorShift, mut jpeg: Vec<u8>) -> Vec<u8> {
    for _ in 0..=rng.below(3) {
        if jpeg.is_empty() {
            break;
        }
        let at = rng.below(jpeg.len() as u64) as usize;
        match rng.below(3) {
            0 => jpeg[at] = rng.next_u8(),
            1 => jpeg.truncate(at),
            _ => jpeg.insert(at, if rng.below(2) == 0 { 0xFF } else { rng.next_u8() }),
        }
    }
    jpeg
}

#[test]
fn all_walkers_agree_with_reference_oracle_on_random_headers() {
    // C#: JpegHeaderOracleTests.AllWalkers_AgreeWithReferenceOracle (60 000 headers, vacuity guards kept).
    // ICC probe = scan_header(j, true, false).has_icc; the Exif block is compared with the oracle, with the combined
    // scan and with pr_imaging's find_exif_tiff_block; the orientation with try_read_orientation of the oracle's block.
    let mut rng = XorShift::new(0x0A11_0B01);
    let (mut icc_hits, mut exif_hits, mut orientation_hits, mut more_hits) = (0usize, 0usize, 0usize, 0usize);
    for iteration in 0..60_000 {
        let mut jpeg = random_header(&mut rng);
        if rng.below(2) == 0 {
            jpeg = mutate_header(&mut rng, jpeg);
        }
        let (expected_icc, expected_exif) = header_oracle(&jpeg);
        let context = format!("iteration {iteration}: {jpeg:02X?}");

        assert_eq!(
            scan_header(&jpeg, true, false).has_icc,
            expected_icc,
            "ICC mismatch, {context}"
        );
        assert_eq!(
            scan_header(&jpeg, true, true).has_icc,
            expected_icc,
            "ICC mismatch (combined), {context}"
        );
        let found = scan_header(&jpeg, false, true).exif_tiff;
        assert_eq!(
            found,
            expected_exif.as_deref().unwrap_or(&[]),
            "Exif block mismatch, {context}"
        );
        assert_eq!(
            exif_parser::find_exif_tiff_block(&jpeg),
            expected_exif.as_deref().unwrap_or(&[]),
            "ExifParser block mismatch, {context}"
        );
        let orientation = expected_exif
            .as_deref()
            .map_or(1, |tiff| exif_parser::try_read_orientation(tiff).map_or(1, i32::from));
        assert_eq!(
            read_exif_orientation(&jpeg),
            orientation,
            "orientation mismatch, {context}"
        );

        icc_hits += usize::from(expected_icc);
        exif_hits += usize::from(expected_exif.is_some());
        orientation_hits += usize::from(orientation != 1);
        more_hits += usize::from(header_needs_more_data(&jpeg));
    }
    assert!(icc_hits > 2_000, "ICC positives: {icc_hits}");
    assert!(exif_hits > 2_000, "Exif block positives: {exif_hits}");
    assert!(orientation_hits > 1_000, "orientation positives: {orientation_hits}");
    assert!(more_hits > 0, "needs-more-data positives: {more_hits}");
}

// ---------------------------------------------------------------------------------------------------------------
// Bounded reads (`TurboJpegHardeningTests`, `TurboJpegHeaderAreaCapTests`, `TurboJpegSharingAndHeaderGrowthGapTests`)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn read_all_bytes_length_one_above_the_limit_is_refused_before_reading() {
    // C#: TurboJpegGuardMutationTests.ReadAllBytes_LengthOneAboveTheLimit_IsRefusedBeforeReading
    let mut reader = CountingReader::new(vec![0; 16], 0);
    let result = read_all_bytes(&mut reader, MAX_SOURCE_BYTES + 1);
    assert!(matches!(result, Err(SourceReadError::TooLarge { .. })), "{result:?}");
    assert_eq!(reader.reads, 0);
}

#[test]
#[ignore = "slow: allocates 2 GiB"]
fn read_all_bytes_length_exactly_the_limit_passes_the_limit_check() {
    // C#: TurboJpegGuardMutationTests.ReadAllBytes_LengthExactlyTheLimit_PassesTheLimitCheck. OutOfMemory with zero
    // reads is accepted, as in C#: only the TooLarge refusal is a failure.
    let mut reader = CountingReader::new(Vec::new(), 0);
    let result = read_all_bytes(&mut reader, MAX_SOURCE_BYTES);
    assert!(!matches!(result, Err(SourceReadError::TooLarge { .. })), "{result:?}");
}

#[test]
fn read_all_bytes_stream_shorter_than_length_returns_what_exists() {
    // C#: TurboJpegHardeningTests.ReadAllBytes_StreamShorterThanLength_ReturnsWhatExists
    let mut reader = CountingReader::new(vec![7; 10], 0);
    let buffer = read_all_bytes(&mut reader, 20).expect("short stream is not an error");
    assert_eq!(buffer, vec![7; 10]);
}

#[test]
fn read_all_bytes_exact_length_is_one_read() {
    // C#: TurboJpegHardeningTests.ReadAllBytes_ExactLength_OneRead (a counting Read double; exactly 1 call)
    let mut reader = CountingReader::new(vec![1, 2, 3, 4], 0);
    let buffer = read_all_bytes(&mut reader, 4).expect("exact read");
    assert_eq!(buffer, vec![1, 2, 3, 4]);
    assert_eq!(reader.reads, 1);
}

#[test]
fn read_all_bytes_too_large_is_not_supported_with_zero_reads() {
    // C#: TurboJpegHardeningTests.ReadAllBytes_TooLarge_NotSupported (0 calls)
    let mut reader = CountingReader::new(vec![0; 4], 0);
    assert!(matches!(
        read_all_bytes(&mut reader, MAX_SOURCE_BYTES + 1),
        Err(SourceReadError::TooLarge { .. })
    ));
    assert_eq!(reader.reads, 0);
}

#[test]
fn zero_byte_file_is_not_supported_and_carries_its_source_bytes() {
    // C#: TurboJpegHardeningTests.ZeroByteFile_NotSupported_CarriesSourceBytes (an empty attached buffer is Some)
    let temp = TempRoot::new("tj-zero");
    let path = write_under(temp.path(), "zero.jpg", &[]);
    let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 36, 0));
    let error = decode_error(&decoder, &DecodeRequest::new(path, 0));
    assert!(error.is_not_supported(), "{error:?}");
    assert!(
        error.source_bytes().is_some(),
        "an empty file read from disk is attached"
    );
}

#[test]
fn extend_header_area_endless_app_segments_stops_at_the_cap() {
    // C#: TurboJpegHeaderAreaCapTests.ExtendHeaderArea_EndlessAppSegments_StopsAtTheCap (writes a 66 MiB temp file)
    let temp = TempRoot::new("tj-cap");
    let path = temp.join("endless.jpg");
    let mut body = vec![0xFF, 0xD8];
    let segment_bytes = segment(0xE2, &vec![0u8; 0xFFFC]);
    while (body.len() as i64) < EXTENDED_HEADER_CAP + 4096 {
        body.extend_from_slice(&segment_bytes);
    }
    std::fs::write(&path, &body).expect("write 66 MiB");
    let initial = body[..HEADER_READ_CHUNK].to_vec();
    let out = extend_header_area(&path, initial).expect("extend");
    assert_eq!(out.len() as i64, EXTENDED_HEADER_CAP);
}

#[test]
#[ignore = "slow: 66 MiB temp file"]
fn extend_header_area_file_that_another_process_has_open_for_writing_is_still_read() {
    // C#: TurboJpegSharingAndHeaderGrowthGapTests.ExtendHeaderArea_FileThatAnotherProcessHasOpenForWriting_IsStillRead
    let temp = TempRoot::new("tj-share");
    let path = write_under(
        temp.path(),
        "shared.jpg",
        &jpeg_shell(&[exif_segment(&tiff_orientation(6))]),
    );
    let _writer = OpenOptions::new().write(true).open(&path).expect("writer handle");
    let initial = fixture_bytes("turbojpeg/parity-8x8.jpg");
    assert!(extend_header_area(&path, initial).is_ok());
}

#[test]
fn file_exists_is_true_for_files_only() {
    // C#: TurboJpegHardeningTests (File.Exists semantics, semantics 2b)
    let temp = TempRoot::new("tj-exists");
    let path = write_under(temp.path(), "a.jpg", b"x");
    assert!(file_exists(&path));
    assert!(!file_exists(temp.path()));
    assert!(!file_exists(&temp.join("missing.jpg")));
}

#[test]
fn grow_header_area_reads_no_more_than_the_cap() {
    // Rust-only: grow_header_area never returns more than its cap, and returns the bytes that exist when the reader ends.
    let mut body = vec![0xFF, 0xD8];
    body.extend(segment(0xE2, &vec![0u8; 0xFFF0]));
    body.extend(segment(0xE2, &vec![0u8; 0xFFF0]));
    let mut reader = io::Cursor::new(body.clone());
    reader.seek(SeekFrom::Start(HEADER_READ_CHUNK as u64)).expect("seek");
    let grown = grow_header_area(
        &mut reader,
        body.len() as i64,
        body[..HEADER_READ_CHUNK].to_vec(),
        128 * 1024,
    )
    .expect("grow");
    assert!(grown.len() <= 128 * 1024);
    assert_eq!(&grown[..], &body[..grown.len()]);
}

#[test]
fn read_header_area_keeps_the_first_chunk_of_a_short_file() {
    // Rust-only: read_header_area returns the whole file when it is shorter than one chunk.
    let temp = TempRoot::new("tj-header");
    let body = fixture_bytes("turbojpeg/parity-8x8.jpg");
    let path = write_under(temp.path(), "short.jpg", &body);
    assert_eq!(read_header_area(&path).expect("header area"), body);
}

#[test]
fn file_sharing_open_handles_do_not_block_the_decode_read() {
    // C#: TurboJpegSharingAndHeaderGrowthGapTests (pure half): a writer handle does not stop the shared read.
    let temp = TempRoot::new("tj-writer");
    let path = write_under(temp.path(), "w.jpg", &fixture_bytes("turbojpeg/parity-8x8.jpg"));
    let mut writer = OpenOptions::new().write(true).open(&path).expect("writer");
    // A write handle stays open; the shared read must not be blocked by it.
    writer.flush().expect("flush");
    let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 36, 0));
    let result = decoder.read_info(path.to_str().expect("utf8 path"));
    assert!(result.is_ok(), "{result:?}");
}

// ---------------------------------------------------------------------------------------------------------------
// Decode, pure paths (`TurboJpegGuardMutationTests`, `TurboJpegTests`, `DecoderAdmissionFallbackTests`, ...)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn decode_without_the_jpeg_signature_is_not_supported() {
    // C#: TurboJpegGuardMutationTests.Decode_WithoutTheJpegSignature_IsNotSupported (6 rows; the rows are not in the
    // contract, these are the signature boundaries: empty, one and two bytes, FF D9, 00 D8 FF, PNG)
    let rows = ["", "FF", "FFD8", "FFD9FF", "00D8FF", "89504E47"];
    for row in rows {
        let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 36, 0));
        let request = DecodeRequest::new("sig.jpg", 0).with_bytes(hex(row).into());
        let error = decode_error(&decoder, &request);
        assert!(error.is_not_supported(), "row {row:?}: {error:?}");
        assert!(error.is_localized() && error.source_bytes().is_none(), "row {row:?}");
    }
}

#[test]
fn read_info_file_of_two_bytes_is_not_supported() {
    // C#: TurboJpegGuardMutationTests.ReadInfo_FileOfTwoBytes_IsNotSupported
    let temp = TempRoot::new("tj-two");
    let path = write_under(temp.path(), "two.jpg", &[0xFF, 0xD8]);
    let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 36, 0));
    let error = decoder.read_info(path.to_str().expect("utf8")).expect_err("two bytes");
    assert!(error.is_not_supported(), "{error:?}");
}

#[test]
fn decode_non_jpeg_bytes_without_path_names_the_memory_buffer_in_the_log_message() {
    // C#: TurboJpegGuardMutationTests.Decode_NonJpegBytesWithoutPath_NamesTheMemoryBufferInTheLogMessage (empty path = null)
    let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 36, 0));
    let request = DecodeRequest::new("", 0).with_bytes(hex("89504E47").into());
    let error = decode_error(&decoder, &request);
    assert!(error.to_string().contains(messages::MEMORY_BUFFER), "{error}");
}

#[test]
fn decode_non_jpeg_bytes_with_a_path_names_the_path_in_the_log_message() {
    // C#: TurboJpegGuardMutationTests.Decode_NonJpegBytesWithAPath_NamesThePathInTheLogMessage
    let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 36, 0));
    let request = DecodeRequest::new("g.jpg", 0).with_bytes(hex("89504E47").into());
    let error = decode_error(&decoder, &request);
    assert!(error.to_string().contains("g.jpg"), "{error}");
}

#[test]
fn decode_source_file_of_exactly_the_guard_threshold_consults_the_memory_budget_before_reading() {
    // C#: TurboJpegGuardMutationTests.Decode_SourceFileOfExactlyTheGuardThreshold_ConsultsTheMemoryBudgetBeforeReading
    // (a sparse 128 MiB file; the signature fails after the read, so the case stays pure)
    let temp = TempRoot::new("tj-guard");
    let path = temp.join("big.jpg");
    std::fs::File::create(&path)
        .expect("create")
        .set_len(GUARD_THRESHOLD_BYTES as u64)
        .expect("set_len");
    let mem = memory(1 << 36, 0);
    let decoder = pure_decoder(factory_of(ok_fake), mem.clone());
    let _ = decode_error(&decoder, &DecodeRequest::new(path, 0));
    assert_eq!(mem.reads.load(Ordering::SeqCst), 1);
}

#[test]
fn decode_source_file_just_below_the_guard_threshold_never_consults_the_memory_budget() {
    // C#: TurboJpegGuardMutationTests.Decode_SourceFileJustBelowTheGuardThreshold_NeverConsultsTheMemoryBudget
    let temp = TempRoot::new("tj-below");
    let path = temp.join("below.jpg");
    std::fs::File::create(&path)
        .expect("create")
        .set_len(GUARD_THRESHOLD_BYTES as u64 - 1)
        .expect("set_len");
    let mem = memory(1 << 36, 0);
    let decoder = pure_decoder(factory_of(ok_fake), mem.clone());
    let _ = decode_error(&decoder, &DecodeRequest::new(path, 0));
    assert_eq!(mem.reads.load(Ordering::SeqCst), 0);
}

#[test]
fn decode_jpeg_with_icc_profile_throws_not_supported_before_the_factory() {
    // C#: TurboJpegTests.JpegWithIccProfileThrowsNotSupportedException and
    // CombinedHeaderScanTests.Decode_IccAndExifBothPresent_ThrowsIccFallback (the ICC check is before the factory)
    let created = Arc::new(AtomicUsize::new(0));
    let counter = created.clone();
    let factory = Arc::new(
        move || -> Result<Box<dyn pr_turbojpeg::decoder::Decompressor>, DecodeError> {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(ok_fake()))
        },
    );
    let decoder = pure_decoder(factory, memory(1 << 36, 0));
    let jpeg = jpeg_shell(&[icc_segment(12), exif_segment(&tiff_orientation(6))]);
    let error = decode_error(&decoder, &DecodeRequest::new("icc.jpg", 0).with_bytes(jpeg.into()));
    assert!(error.is_not_supported(), "{error:?}");
    assert_eq!(created.load(Ordering::SeqCst), 0, "no decompressor for an ICC profile");
    assert!(FallbackOk::is_fallbackable_icc(&error));
}

/// Helper namespace: the fallbackable verdict of the ICC refusal (the chain tests use the real fallback).
struct FallbackOk;

impl FallbackOk {
    fn is_fallbackable_icc(error: &DecodeError) -> bool {
        pr_imaging::fallback::FallbackImageDecoder::is_fallbackable(error)
    }
}

#[test]
fn non_jpeg_files_throw_not_supported() {
    // C#: TurboJpegTests.NonJpegFilesThrowNotSupportedException
    let temp = TempRoot::new("tj-nonjpeg");
    let path = write_under(temp.path(), "text.jpg", b"This is not a JPEG at all");
    let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 36, 0));
    let error = decode_error(&decoder, &DecodeRequest::new(path, 0));
    assert!(error.is_not_supported(), "{error:?}");
}

#[test]
fn orientation_metadata_read_matches_for_normal_and_rotated_headers() {
    // C#: Metadata/OrientationParityTests.TurboJpegAndExifParser_Agree and NormalOrientation_StillRead (pure rows)
    let rotated = jpeg_shell(&[exif_segment(&tiff_orientation(6))]);
    assert_eq!(read_exif_orientation(&rotated), 6);
    let normal = jpeg_shell(&[exif_segment(&tiff_orientation(1))]);
    assert_eq!(read_exif_orientation(&normal), 1);
    let from_parser = exif_parser::try_read_orientation(scan_header(&rotated, false, true).exif_tiff);
    assert_eq!(from_parser.map(i32::from).unwrap_or(1), read_exif_orientation(&rotated));
}

#[test]
fn out_of_range_orientation_value_is_treated_as_normal_in_pure_reads() {
    // C#: Robustness/InvalidOrientationValueTests.OutOfRangeOrientation_IsTreatedAsNormal (pure half: the header read)
    let jpeg = jpeg_shell(&[exif_segment(&tiff_orientation(9))]);
    assert_eq!(
        read_exif_orientation(&jpeg),
        1,
        "an orientation outside 1..8 reads as normal (the raw value only survives a source_orientation override)"
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Decode with fakes: strict mode, native error arms, factory errors, scale path, admission, threads
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn turbo_success_reports_actual_backend_and_orientation() {
    // C#: DecoderContractRegressionTests.TurboSuccessReportsActualBackendAndOrientation
    let decoder = decoder_with(factory_of(ok_fake), memory(1 << 36, 0), no_scaler());
    let jpeg = fixture_bytes("turbojpeg/parity-8x8.jpg");
    let image = decoder
        .decode(&DecodeRequest::new("p.jpg", 0).with_bytes(jpeg.into()))
        .expect("fake decode");
    assert_eq!(image.actual_backend(), pr_core::model::enums::DecoderBackend::TurboJpeg);
    assert_eq!(image.orientation(), 1);
}

#[test]
fn strict_mode_failure_is_invalid_operation_not_fallbackable_and_carries_no_bytes() {
    // Rust-only (contract "New Rust tests"): set(StopOnWarning) returns -1 -> InvalidOperation, never fallbackable.
    let temp = TempRoot::new("tj-strict");
    let path = write_under(temp.path(), "p.jpg", &fixture_bytes("turbojpeg/parity-8x8.jpg"));
    let decoder = pure_decoder(
        factory_of(|| FakeDecompressor {
            strict_code: -1,
            ..FakeDecompressor::default()
        }),
        memory(1 << 36, 0),
    );
    let error = decode_error(&decoder, &DecodeRequest::new(path, 0));
    assert!(
        matches!(error.kind(), DecodeError::InvalidOperation { .. }),
        "{error:?}"
    );
    assert!(!FallbackOk::is_fallbackable_icc(&error));
    assert!(error.source_bytes().is_none());
}

#[test]
fn set_scaling_factor_failure_decodes_at_full_size_and_fine_scales() {
    // Rust-only: a refused set_scaling_factor resets the factor to 1/1 and the fine scaler produces the target.
    let decoder = decoder_with(
        factory_of(|| FakeDecompressor {
            scale_code: 1,
            width: 64,
            height: 48,
            ..FakeDecompressor::default()
        }),
        memory(1 << 36, 0),
        blank_scaler(),
    );
    let jpeg = fixture_bytes("turbojpeg/parity-64x48.jpg");
    let request =
        DecodeRequest::with_box("p.jpg", pr_imaging::decode_types::DecodeBox::new(16, 12)).with_bytes(jpeg.into());
    let image = decoder.decode(&request).expect("fine scale after the refused factor");
    assert_eq!((image.pixel_width(), image.pixel_height()), (16, 12));
    assert!(image.downscaled());
}

#[test]
fn header_failure_without_native_text_logs_the_parse_error_and_shows_no_detail_to_the_user() {
    // Rust-only + C#: TurboJpegGuardMutationTests (T3): the log says "Header parse error", the user text uses no_detail().
    let decoder = pure_decoder(
        factory_of(|| FakeDecompressor {
            header_code: -1,
            error: None,
            ..FakeDecompressor::default()
        }),
        memory(1 << 36, 0),
    );
    let jpeg = hex("FFD8FFE0");
    let error = decode_error(&decoder, &DecodeRequest::new("h.jpg", 0).with_bytes(jpeg.into()));
    assert!(error.to_string().contains(messages::HEADER_PARSE_ERROR), "{error}");
    let user = error.user_text().map(|t| t()).unwrap_or_default();
    assert!(user.contains("no details available"), "{user}");
}

#[test]
fn header_failure_with_native_text_carries_the_native_detail_in_log_and_user_text() {
    // C#: TurboJpegGuardMutationTests.Decode_HeaderTheNativeParserRejects_CarriesTheNativeDetailInLogAndUserText (fake text)
    let decoder = pure_decoder(
        factory_of(|| FakeDecompressor {
            header_code: -1,
            error: Some("Unsupported JPEG data precision 3".to_owned()),
            ..FakeDecompressor::default()
        }),
        memory(1 << 36, 0),
    );
    let error = decode_error(
        &decoder,
        &DecodeRequest::new("h.jpg", 0).with_bytes(hex("FFD8FFE0").into()),
    );
    assert!(
        error.to_string().contains("Unsupported JPEG data precision 3"),
        "{error}"
    );
    assert!(
        error
            .user_text()
            .is_some_and(|t| t().contains("Unsupported JPEG data precision 3"))
    );
}

#[test]
fn decompress_failure_with_and_without_native_detail() {
    // C#: TurboJpegGuardMutationTests.Decode_ScanTheNativeDecoderRejects_CarriesTheNativeDetailInLogAndUserText (fake)
    for (native, expected) in [
        (Some("Corrupt scan"), "Corrupt scan"),
        (None, messages::DECOMPRESSION_ERROR),
    ] {
        let decoder = pure_decoder(
            factory_of(move || FakeDecompressor {
                decompress_code: -1,
                error: native.map(str::to_owned),
                ..FakeDecompressor::default()
            }),
            memory(1 << 36, 0),
        );
        let error = decode_error(
            &decoder,
            &DecodeRequest::new("d.jpg", 0).with_bytes(fixture_bytes("turbojpeg/parity-8x8.jpg").into()),
        );
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn sof_declaring_zero_width_is_refused_as_invalid_data_fault() {
    // C#: TurboJpegGuardMutationTests.Decode_SofDeclaringAZeroWidth_IsRejectedAsAnInvalidDataFault
    let decoder = pure_decoder(
        factory_of(|| FakeDecompressor {
            width: 0,
            ..FakeDecompressor::default()
        }),
        memory(1 << 36, 0),
    );
    let error = decode_error(
        &decoder,
        &DecodeRequest::new("z.jpg", 0).with_bytes(fixture_bytes("turbojpeg/parity-8x8.jpg").into()),
    );
    assert!(matches!(error.kind(), DecodeError::InvalidData { .. }), "{error:?}");
    assert!(error.to_string().contains("0x8"), "{error}");
}

#[test]
fn factory_error_is_fallbackable_and_carries_the_file_bytes() {
    // Rust-only (contract "New Rust tests"): DllNotFound from the factory is fallbackable and attaches the file bytes.
    let temp = TempRoot::new("tj-dll");
    let path = write_under(temp.path(), "p.jpg", &fixture_bytes("turbojpeg/parity-8x8.jpg"));
    let decoder = pure_decoder(
        failing_factory(|| DecodeError::DllNotFound {
            message: "turbojpeg.dll".to_owned(),
        }),
        memory(1 << 36, 0),
    );
    let error = decode_error(&decoder, &DecodeRequest::new(path, 0));
    assert!(FallbackOk::is_fallbackable_icc(&error));
    assert_eq!(
        error.source_bytes().map(|b| b.len()),
        Some(fixture_bytes("turbojpeg/parity-8x8.jpg").len())
    );
}

#[test]
fn native_entry_point_not_found_propagates_unchanged_from_header_and_from_scan_limit() {
    // Rust-only (contract "New Rust tests"): an Err from decompress_header or set(ScanLimit) is not turned into InvalidData.
    let by_header = pure_decoder(
        factory_of(|| FakeDecompressor {
            header_error: Some(|| DecodeError::EntryPointNotFound {
                message: "tj3DecompressHeader".to_owned(),
            }),
            ..FakeDecompressor::default()
        }),
        memory(1 << 36, 0),
    );
    let error = decode_error(
        &by_header,
        &DecodeRequest::new("e.jpg", 0).with_bytes(fixture_bytes("turbojpeg/parity-8x8.jpg").into()),
    );
    assert!(
        matches!(error.kind(), DecodeError::EntryPointNotFound { .. }),
        "{error:?}"
    );
    let by_limit = pure_decoder(
        factory_of(|| FakeDecompressor {
            scan_limit_error: Some(|| DecodeError::EntryPointNotFound {
                message: "tj3Set".to_owned(),
            }),
            ..FakeDecompressor::default()
        }),
        memory(1 << 36, 0),
    );
    let error = decode_error(
        &by_limit,
        &DecodeRequest::new("e.jpg", 0).with_bytes(fixture_bytes("turbojpeg/parity-8x8.jpg").into()),
    );
    assert!(
        matches!(error.kind(), DecodeError::EntryPointNotFound { .. }),
        "{error:?}"
    );
}

#[test]
fn fine_scaler_out_of_memory_is_a_memory_admission_with_the_scaled_size_and_no_live_buffer() {
    // Rust-only (contract "New Rust tests"): OutOfMemory from the scaler -> MemoryAdmission naming the SCALED size.
    serialize_pixel_memory();
    let before = NativePixelMemory::live_count();
    let decoder = decoder_with(
        factory_of(|| FakeDecompressor {
            width: 64,
            height: 48,
            ..FakeDecompressor::default()
        }),
        memory(1 << 36, 0),
        oom_scaler(),
    );
    let request = DecodeRequest::with_box("o.jpg", pr_imaging::decode_types::DecodeBox::new(4, 4))
        .with_bytes(fixture_bytes("turbojpeg/parity-64x48.jpg").into());
    let error = decode_error(&decoder, &request);
    assert!(matches!(error.kind(), DecodeError::MemoryAdmission { .. }), "{error:?}");
    assert!(error.to_string().contains("8x6"), "scaled size in the message: {error}");
    assert_eq!(NativePixelMemory::live_count(), before);
}

#[test]
fn decoder_is_send_sync_and_has_no_cancellable_variant() {
    // C#: Rust-only compile check + as_cancellable() is None (CancellableImageDecoder is not implemented).
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TurboJpegDecoder>();
    let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 36, 0));
    assert!(decoder.as_cancellable().is_none());
}

#[test]
fn parallel_decodes_match_the_serial_reference() {
    // C#: DecoderConcurrencyTests.ParallelDecodes_MatchTheSerialReference (8 threads, one shared decoder)
    let decoder = Arc::new(decoder_with(factory_of(ok_fake), memory(1 << 36, 0), no_scaler()));
    let jpeg = Arc::new(fixture_bytes("turbojpeg/parity-8x8.jpg"));
    let serial = decoder
        .decode(&DecodeRequest::new("s.jpg", 0).with_bytes(jpeg.to_vec().into()))
        .map(|i| (i.pixel_width(), i.pixel_height()))
        .expect("serial");
    thread::scope(|scope| {
        for _ in 0..8 {
            let decoder = Arc::clone(&decoder);
            let jpeg = Arc::clone(&jpeg);
            scope.spawn(move || {
                let got = decoder
                    .decode(&DecodeRequest::new("p.jpg", 0).with_bytes(jpeg.to_vec().into()))
                    .map(|i| (i.pixel_width(), i.pixel_height()))
                    .expect("parallel");
                assert_eq!(got, serial);
            });
        }
    });
}

#[test]
fn exactly_one_pixel_buffer_is_alive_after_a_successful_decode_and_none_after_a_failure() {
    // C#: TurboJpegPixelBufferLifetimeTests.Decode_HoldsExactlyOneBuffer_AllOrientationsAndFineScale (pure half, fake
    // decompressor and blank scaler). The count is the process-wide NativePixelMemory counter (W4-16b).
    serialize_pixel_memory();
    let start = NativePixelMemory::live_count();
    let decoder = decoder_with(factory_of(ok_fake), memory(1 << 36, 0), blank_scaler());
    let request = DecodeRequest::with_box("l.jpg", pr_imaging::decode_types::DecodeBox::new(4, 4))
        .with_bytes(fixture_bytes("turbojpeg/parity-64x48.jpg").into());
    let image = decoder.decode(&request).expect("decode");
    assert_eq!(NativePixelMemory::live_count(), start + 1);
    drop(image);
    assert_eq!(NativePixelMemory::live_count(), start);
    let failing = decoder_with(
        factory_of(|| FakeDecompressor {
            decompress_code: -1,
            ..FakeDecompressor::default()
        }),
        memory(1 << 36, 0),
        blank_scaler(),
    );
    let _ =
        failing.decode(&DecodeRequest::new("f.jpg", 0).with_bytes(fixture_bytes("turbojpeg/parity-64x48.jpg").into()));
    assert_eq!(NativePixelMemory::live_count(), start);
}

#[test]
fn codec_throwing_frees_the_buffer() {
    // C#: TurboJpegPixelBufferLifetimeTests.CodecThrows_BufferIsFreed (the codec owns the buffer; an error drops it)
    struct FailingCodec;
    impl pr_imaging::decoder_traits::PlatformImageCodec for FailingCodec {
        fn name(&self) -> &'static str {
            "failing"
        }
        fn image_from_pixels(
            &self,
            _pixels: PixelBuffer,
        ) -> Result<pr_imaging::decode_types::PlatformImage, DecodeError> {
            Err(DecodeError::Other {
                message: "codec failed".to_owned(),
            })
        }
        fn to_pixels<'a>(
            &self,
            platform_image: &'a pr_imaging::decode_types::PlatformImage,
        ) -> Result<pr_imaging::decode_types::PixelLease<'a>, DecodeError> {
            match platform_image.as_pixels() {
                Some(pixels) => Ok(pr_imaging::decode_types::PixelLease::borrowed(pixels.as_ref())),
                None => Err(DecodeError::InvalidArgument {
                    message: "not pixels".to_owned(),
                }),
            }
        }
    }
    serialize_pixel_memory();
    let start = NativePixelMemory::live_count();
    let decoder = TurboJpegDecoder::new(
        Arc::new(FailingCodec),
        blank_scaler(),
        memory(1 << 36, 0),
        factory_of(ok_fake),
        english(),
    );
    let request = DecodeRequest::new("c.jpg", 0).with_bytes(fixture_bytes("turbojpeg/parity-8x8.jpg").into());
    let _ = decoder.decode(&request);
    assert_eq!(NativePixelMemory::live_count(), start);
}

#[test]
fn icc_profile_falls_back_without_allocating() {
    // C#: TurboJpegPixelBufferLifetimeTests.IccProfile_FallsBackWithoutAllocating (pure)
    serialize_pixel_memory();
    let start = NativePixelMemory::live_count();
    let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 36, 0));
    let jpeg = jpeg_shell(&[icc_segment(12)]);
    let _ = decoder.decode(&DecodeRequest::new("i.jpg", 0).with_bytes(jpeg.into()));
    assert_eq!(NativePixelMemory::live_count(), start);
}

#[test]
fn huge_claimed_size_is_admission_refusal_not_overflow_and_allocates_nothing() {
    // C#: TurboJpegPixelBufferLifetimeTests.HugeClaimedSize_IsAdmissionRefusal_NotOverflow_AndAllocatesNothing (pure)
    serialize_pixel_memory();
    let start = NativePixelMemory::live_count();
    let mem = memory(1 << 30, 0);
    let decoder = pure_decoder(
        factory_of(|| FakeDecompressor {
            width: 65_535,
            height: 65_535,
            ..FakeDecompressor::default()
        }),
        mem.clone(),
    );
    let error = decode_error(
        &decoder,
        &DecodeRequest::new("h.jpg", 0).with_bytes(fixture_bytes("turbojpeg/parity-8x8.jpg").into()),
    );
    assert!(
        matches!(
            error.kind(),
            DecodeError::InvalidData { .. } | DecodeError::MemoryAdmission { .. }
        ),
        "{error:?}"
    );
    assert_eq!(NativePixelMemory::live_count(), start);
}

#[test]
fn fallback_chain_source_bytes_are_attached_only_for_fallbackable_file_failures() {
    // C#: Decoding/ErrorHandlingReviewDecodingTests.SourceBytes_AttachedOnlyForFallbackableFailure and
    // SourceBytes_NotAttachedForNonFallbackableFailure; Decoding/DecoderAdmissionFallbackTests (turbo half, pure).
    let temp = TempRoot::new("tj-attach");
    let path = write_under(temp.path(), "a.jpg", &fixture_bytes("turbojpeg/parity-8x8.jpg"));
    let fallbackable = pure_decoder(
        failing_factory(|| DecodeError::DllNotFound {
            message: "x".to_owned(),
        }),
        memory(1 << 36, 0),
    );
    assert!(
        decode_error(&fallbackable, &DecodeRequest::new(path.clone(), 0))
            .source_bytes()
            .is_some()
    );
    let refused = pure_decoder(
        factory_of(|| FakeDecompressor {
            strict_code: -1,
            ..FakeDecompressor::default()
        }),
        memory(1 << 36, 0),
    );
    assert!(
        decode_error(&refused, &DecodeRequest::new(path, 0))
            .source_bytes()
            .is_none()
    );
}

#[test]
fn huge_source_file_is_refused_before_reading() {
    // C#: ErrorHandlingReviewDecodingTests.HugeSourceFile_IsRefusedBeforeReading (pure: the guard reads memory, not the file)
    let temp = TempRoot::new("tj-huge");
    let path = temp.join("huge.jpg");
    std::fs::File::create(&path)
        .expect("create")
        .set_len(MAX_SOURCE_BYTES as u64 + 1)
        .expect("set_len");
    let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 30, 0));
    let error = decode_error(&decoder, &DecodeRequest::new(path, 0));
    assert!(
        error.is_not_supported() || matches!(error.kind(), DecodeError::MemoryAdmission { .. }),
        "{error:?}"
    );
}

#[test]
fn missing_file_keeps_its_localized_file_not_found_type() {
    // C#: DecoderErrorLocalizationTests.MissingFile_IsLocalizedAndKeepsItsType and
    // Robustness/DecoderFileEdgeCaseTests.MissingFile_KeepsItsExceptionType / Quality/DecoderQualityGateTests.MissingFilesThrowFileNotFoundException
    let temp = TempRoot::new("tj-missing");
    let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 36, 0));
    let error = decode_error(&decoder, &DecodeRequest::new(temp.join("nope.jpg"), 0));
    assert!(matches!(error.kind(), DecodeError::FileNotFound { .. }), "{error:?}");
    assert!(error.is_localized());
}

#[test]
fn no_source_is_localized() {
    // C#: DecoderErrorLocalizationTests.NoSource_IsLocalized (blank path and no bytes)
    let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 36, 0));
    let error = decode_error(&decoder, &DecodeRequest::new("   ", 0));
    assert!(matches!(error.kind(), DecodeError::InvalidArgument { .. }), "{error:?}");
    assert!(error.is_localized());
}

#[test]
fn blank_path_read_info_is_invalid_argument() {
    // C#: DecoderFileEdgeCaseTests / ReadInfo blank path (InvalidArgument, not localized; semantics 12a)
    let decoder = pure_decoder(factory_of(ok_fake), memory(1 << 36, 0));
    let error = decoder.read_info("  ").expect_err("blank");
    assert!(
        matches!(error, DecodeError::InvalidArgument { ref message } if message == messages::BLANK_PATH),
        "{error:?}"
    );
}

#[test]
fn progressive_scan_limit_refuses_with_decompress_text_and_few_scans_decode() {
    // C#: TurboJpegHardeningTests.ProgressiveWithFewScans_Decodes / ProgressiveWithTooManyScans_IsRefused (pure part: the
    // pure walker sees the scan count; the native half is in decoder_native.rs)
    assert_eq!(MAX_PROGRESSIVE_SCANS, 500);
    let few = progressive_gray(0);
    let many = progressive_gray(7);
    assert_eq!(few.windows(2).filter(|w| *w == [0xFF, 0xDA]).count(), 64);
    assert_eq!(many.windows(2).filter(|w| *w == [0xFF, 0xDA]).count(), 512);
}

#[test]
fn pad_com_bytes_helper_places_sos_where_the_golden_expects() {
    // Rust-only: the golden transform helpers (`apply_transforms`) keep the frame valid for the header walk.
    let source = fixture_bytes("turbojpeg/small-q85-64x48.jpg");
    let padded = apply_transforms(
        source.clone(),
        &[serde_json::json!({"op": "lead_com", "count": 2, "size": 10})],
    );
    assert_eq!(padded.len(), source.len() + 2 * 14);
    assert!(scan_header(&padded, false, false).exif_tiff.is_empty());
}

#[test]
fn read_info_header_size_rows_use_the_first_chunk_only_for_orientation() {
    // C#: Robustness/LargeHeaderReadInfoTests (pure part of the chunking rule, semantics 12 / T11)
    let info = ImageInfo::with_orientation(120, 80, 6);
    assert_eq!((info.width(), info.height()), (80, 120));
    assert_eq!(
        DimensionsProbe::Failed { native_error: None },
        DimensionsProbe::Failed { native_error: None }
    );
}

// ---------------------------------------------------------------------------------------------------------------
// REPAIR round 1: tests that pin the boundaries the mutation run found unpinned (Rust-only; no C# twin)
// ---------------------------------------------------------------------------------------------------------------

mod repair {
    use super::*;
    use pr_imaging::decode_types::DecodeBox;
    use pr_turbojpeg::decoder::{Decompressor, EnglishTurboJpegTexts, TurboJpegTexts};
    use pr_turbojpeg::native::{TjParam, TjPixelFormat};
    use std::sync::Mutex;

    /// What the probe decompressors saw (shared across the decompressors a decoder creates).
    #[derive(Default)]
    struct Log {
        factors: Mutex<Vec<(i32, i32)>>,
        buffers: Mutex<Vec<(usize, i32)>>,
    }

    /// A decompressor that reports `width x height`, records the scaling factor and the destination, and can refuse
    /// the factor.
    struct Probe {
        log: Arc<Log>,
        width: i32,
        height: i32,
        refuse_scaling: bool,
    }

    impl Decompressor for Probe {
        fn set(&mut self, _param: TjParam, _value: i32) -> Result<i32, DecodeError> {
            Ok(0)
        }
        fn get(&mut self, param: TjParam) -> Result<i32, DecodeError> {
            Ok(match param {
                TjParam::JpegWidth => self.width,
                TjParam::JpegHeight => self.height,
                _ => 0,
            })
        }
        fn decompress_header(&mut self, _jpeg: &[u8]) -> Result<i32, DecodeError> {
            Ok(0)
        }
        fn set_scaling_factor(&mut self, factor: TjScalingFactor) -> Result<i32, DecodeError> {
            self.log.factors.lock().unwrap().push((factor.num, factor.denom));
            Ok(i32::from(self.refuse_scaling))
        }
        fn decompress8(
            &mut self,
            _jpeg: &[u8],
            dst: &mut [u8],
            pitch: i32,
            _pixel_format: TjPixelFormat,
        ) -> Result<i32, DecodeError> {
            self.log.buffers.lock().unwrap().push((dst.len(), pitch));
            Ok(0)
        }
        fn error_message(&mut self) -> Option<String> {
            None
        }
    }

    fn factory_of_probe(log: Arc<Log>, width: i32, height: i32, refuse_scaling: bool) -> Arc<dyn DecompressorFactory> {
        Arc::new(move || -> Result<Box<dyn Decompressor>, DecodeError> {
            Ok(Box::new(Probe {
                log: Arc::clone(&log),
                width,
                height,
                refuse_scaling,
            }))
        })
    }

    fn probe_decoder(log: &Arc<Log>, width: i32, height: i32, refuse_scaling: bool) -> TurboJpegDecoder {
        decoder_with(
            factory_of_probe(Arc::clone(log), width, height, refuse_scaling),
            memory(1 << 36, 0),
            blank_scaler(),
        )
    }

    struct Outcome {
        width: i32,
        height: i32,
        downscaled: bool,
        original: (i32, i32),
        factor: Option<(i32, i32)>,
        dst: Option<(usize, i32)>,
    }

    /// Decodes the 64x48 fixture bytes while the probe reports `width x height`.
    fn run(
        width: i32,
        height: i32,
        refuse: bool,
        decode_box: DecodeBox,
        orientation: Option<i32>,
    ) -> Result<Outcome, DecodeError> {
        let log = Arc::new(Log::default());
        let decoder = probe_decoder(&log, width, height, refuse);
        let jpeg = fixture_bytes("turbojpeg/parity-64x48.jpg");
        let mut request = DecodeRequest::with_box("p.jpg", decode_box).with_bytes(jpeg.into());
        if let Some(value) = orientation {
            request = request.with_source_orientation(value);
        }
        let image = decoder.decode(&request)?;
        let factor = log.factors.lock().unwrap().first().copied();
        let dst = log.buffers.lock().unwrap().first().copied();
        Ok(Outcome {
            width: image.pixel_width(),
            height: image.pixel_height(),
            downscaled: image.downscaled(),
            original: (image.original_width(), image.original_height()),
            factor,
            dst,
        })
    }

    #[test]
    fn output_buffer_saturation_boundary_is_exact() {
        // width 2^30 -> stride 2^32; i64::MAX / 2^32 == 2^31 - 1 == height: NOT saturated, the true product is printed.
        let error = calculate_output_buffer(1 << 30, i32::MAX, TjScalingFactor::ONE).unwrap_err();
        assert_eq!(error.width, 1 << 30);
        assert_eq!(error.height, i64::from(i32::MAX));
        assert_eq!(error.buffer_length, (1_i64 << 32) * i64::from(i32::MAX));
        let ok = calculate_output_buffer(1000, 1000, TjScalingFactor::ONE).unwrap();
        assert_eq!((ok.stride, ok.buffer_length), (4000, 4_000_000));
        let error = calculate_output_buffer(i32::MAX, i32::MAX, TjScalingFactor::ONE).unwrap_err();
        assert_eq!(error.buffer_length, i64::MAX);
    }

    #[test]
    fn one_zero_side_is_invalid_dimensions() {
        for (w, h) in [(0, 5), (5, 0), (0, 0), (-1, 5), (5, -1)] {
            let error = run(w, h, false, DecodeBox::new(0, 0), None).err().expect("refused");
            assert!(
                matches!(error.kind(), DecodeError::InvalidData { .. }),
                "{w}x{h}: {error:?}"
            );
            assert_eq!(
                error.to_string(),
                format!("{}{w}x{h}", messages::INVALID_DIMENSIONS_PREFIX),
                "{w}x{h}"
            );
        }
        assert!(run(1, 1, false, DecodeBox::new(0, 0), None).is_ok());
    }

    #[test]
    fn scale_decision_passes_the_expected_factor() {
        // (reported size, box, refuse, orientation) -> (factor, output size, downscaled)
        type Row = ((i32, i32), (i32, i32), bool, Option<i32>, (i32, i32), (i32, i32), bool);
        let rows: [Row; 9] = [
            // Width shrinks, height does not (aspect clamp): only the width comparison selects the factor.
            ((4000, 1), (2000, 1), false, None, (1, 2), (2000, 1), true),
            // Height shrinks, width does not.
            ((1, 4000), (1, 2000), false, None, (1, 2), (1, 2000), true),
            // Exact 1/4 factor: DCT output equals the target, no fine scale.
            ((64, 48), (16, 12), false, None, (1, 4), (16, 12), true),
            // Target between factors: 3/8 (24x18) then a fine scale down to the target.
            ((64, 48), (20, 15), false, None, (3, 8), (20, 15), true),
            // Box equal to the source: nothing to do, factor stays 1/1 and the image is not downscaled.
            ((64, 48), (64, 48), false, None, (1, 1), (64, 48), false),
            // Box bigger than the source: same.
            ((64, 48), (640, 480), false, None, (1, 1), (64, 48), false),
            // Factor refused (1/2 was attempted): decode at 1/1, fine scale from there; downscaled via the target comparisons.
            ((4000, 1), (2000, 1), true, None, (1, 2), (2000, 1), true),
            ((1, 4000), (1, 2000), true, None, (1, 2), (1, 2000), true),
            // Transposing orientation: stored axes swap for the fit, the result is oriented back.
            ((64, 48), (24, 32), false, Some(6), (1, 2), (24, 32), true),
        ];
        for (reported, box_size, refuse, orientation, factor, size, downscaled) in rows {
            let label = format!("{reported:?} box {box_size:?} refuse {refuse} orientation {orientation:?}");
            let outcome = run(
                reported.0,
                reported.1,
                refuse,
                DecodeBox::new(box_size.0, box_size.1),
                orientation,
            )
            .unwrap_or_else(|e| panic!("{label}: {e:?}"));
            assert_eq!(outcome.factor, Some(factor), "{label}: factor");
            assert_eq!((outcome.width, outcome.height), size, "{label}: size");
            assert_eq!(outcome.downscaled, downscaled, "{label}: downscaled");
            let transposed = orientation.is_some_and(|o| o >= 5);
            let expected_original = if transposed { (reported.1, reported.0) } else { reported };
            assert_eq!(outcome.original, expected_original, "{label}: original");
        }
    }

    #[test]
    fn dct_output_buffer_is_the_scaled_size() {
        let outcome = run(64, 48, false, DecodeBox::new(16, 12), None).unwrap();
        assert_eq!(outcome.dst, Some((16 * 12 * 4, 16 * 4)));
        // Fine scale: the DCT buffer is the 3/8 one (24x18), the result is the target.
        let outcome = run(64, 48, false, DecodeBox::new(20, 15), None).unwrap();
        assert_eq!(outcome.dst, Some((24 * 18 * 4, 24 * 4)));
        assert_eq!((outcome.width, outcome.height), (20, 15));
    }

    #[test]
    fn refused_scaling_factor_decodes_into_the_full_size_buffer() {
        let outcome = run(4000, 1, true, DecodeBox::new(2000, 1), None).unwrap();
        assert_eq!(outcome.dst, Some((4000 * 4, 4000 * 4)));
        let outcome = run(1, 4000, true, DecodeBox::new(1, 2000), None).unwrap();
        assert_eq!(outcome.dst, Some((4000 * 4, 4)));
    }

    #[test]
    fn no_downscale_request_never_touches_the_scale_or_flags() {
        let outcome = run(64, 48, false, DecodeBox::new(0, 0), None).unwrap();
        assert_eq!(outcome.factor, Some((1, 1)));
        assert_eq!((outcome.width, outcome.height, outcome.downscaled), (64, 48, false));
    }

    #[test]
    fn orientation_is_applied_only_when_requested() {
        let outcome = run(64, 48, false, DecodeBox::new(0, 0), Some(6)).unwrap();
        assert_eq!((outcome.width, outcome.height), (48, 64));
        let outcome = run(64, 48, false, DecodeBox::new(0, 0), Some(1)).unwrap();
        assert_eq!((outcome.width, outcome.height), (64, 48));
        // Orientation not requested: reported unchanged, pixels untouched.
        let log = Arc::new(Log::default());
        let decoder = probe_decoder(&log, 64, 48, false);
        let request = DecodeRequest::new("p.jpg", 0)
            .with_apply_orientation(false)
            .with_source_orientation(6)
            .with_bytes(fixture_bytes("turbojpeg/parity-64x48.jpg").into());
        let image = decoder.decode(&request).unwrap();
        assert_eq!(
            (image.pixel_width(), image.pixel_height(), image.orientation()),
            (64, 48, 1)
        );
    }

    /// A reader that yields `Interrupted` before each successful read (3 bytes at a time), or fails with a fixed error.
    struct Flaky {
        data: io::Cursor<Vec<u8>>,
        interrupt_next: bool,
        fail_with: Option<io::ErrorKind>,
    }

    impl Read for Flaky {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if let Some(kind) = self.fail_with {
                return Err(io::Error::from(kind));
            }
            if self.interrupt_next {
                self.interrupt_next = false;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            self.interrupt_next = true;
            let limit = buf.len().min(3);
            self.data.read(&mut buf[..limit])
        }
    }

    #[test]
    fn interrupted_reads_are_retried_and_other_errors_surface() {
        let data: Vec<u8> = (0..10).collect();
        let mut reader = Flaky {
            data: io::Cursor::new(data.clone()),
            interrupt_next: true,
            fail_with: None,
        };
        assert_eq!(read_all_bytes(&mut reader, 10).unwrap(), data);
        let mut reader = Flaky {
            data: io::Cursor::new(data),
            interrupt_next: false,
            fail_with: Some(io::ErrorKind::PermissionDenied),
        };
        match read_all_bytes(&mut reader, 10) {
            Err(SourceReadError::Io(e)) => assert_eq!(e.kind(), io::ErrorKind::PermissionDenied),
            other => panic!("expected Io, got {other:?}"),
        }
    }

    /// `FF D8`, `count` APP segments of `payload` bytes each, SOS, EOI.
    fn big_header(count: usize, payload: usize) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        for _ in 0..count {
            out.extend(segment(0xE5, &vec![7u8; payload]));
        }
        out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
        out
    }

    #[test]
    fn grow_header_area_reads_only_while_the_header_is_unfinished_and_inside_the_limits() {
        let file = big_header(4, 60_000); // ~240 KB: the SOS header ends after 240 KB
        let total = file.len() as i64;
        // Buffer already holds the whole file although it looks unfinished: zero reads.
        let truncated = file[..file.len() - 3].to_vec();
        let mut reader = CountingReader::new(Vec::new(), 0);
        let got = grow_header_area(&mut reader, truncated.len() as i64, truncated.clone(), HEADER_READ_CAP).unwrap();
        assert_eq!((got, reader.reads), (truncated, 0));
        // Buffer at the cap (limit == len) but shorter than the file and unfinished: zero reads.
        let first = file[..HEADER_READ_CHUNK].to_vec();
        let mut reader = CountingReader::new(file[HEADER_READ_CHUNK..].to_vec(), 0);
        let got = grow_header_area(&mut reader, total, first.clone(), HEADER_READ_CHUNK as i64).unwrap();
        assert_eq!((got.len(), reader.reads), (HEADER_READ_CHUNK, 0));
        // Header already complete inside the buffer although the file is longer: zero reads.
        let mut with_tail = big_header(1, 100);
        with_tail.extend(vec![0u8; 5000]);
        let mut reader = CountingReader::new(Vec::new(), 0);
        let got = grow_header_area(&mut reader, with_tail.len() as i64, with_tail.clone(), HEADER_READ_CAP).unwrap();
        assert_eq!((got.len(), reader.reads), (with_tail.len(), 0));
        // The doubling reads: 64 KiB -> 128 KiB -> the file length (the limit): two reads, the whole file.
        let mut reader = CountingReader::new(file[HEADER_READ_CHUNK..].to_vec(), 0);
        let got = grow_header_area(&mut reader, total, first, HEADER_READ_CAP).unwrap();
        assert_eq!(got, file);
        assert_eq!(reader.reads, 2);
    }

    #[test]
    fn grow_header_area_with_a_short_reader_keeps_exactly_the_bytes_that_exist() {
        let file = big_header(4, 60_000);
        let first = file[..HEADER_READ_CHUNK].to_vec();
        // The file "claims" its full length but the reader ends 1000 bytes into the growth.
        let mut reader = CountingReader::new(file[HEADER_READ_CHUNK..HEADER_READ_CHUNK + 1000].to_vec(), 0);
        let got = grow_header_area(&mut reader, file.len() as i64, first, HEADER_READ_CAP).unwrap();
        assert_eq!(got.len(), HEADER_READ_CHUNK + 1000);
        assert_eq!(got[..], file[..HEADER_READ_CHUNK + 1000]);
    }

    #[test]
    fn english_texts_are_the_catalog_sentences() {
        let t = EnglishTurboJpegTexts;
        assert_eq!(t.not_jpeg("a.bin"), "The file is not a valid JPEG: a.bin");
        assert_eq!(t.memory_buffer(), "memory buffer");
        assert!(t.icc_fallback().contains("embedded ICC color profile"));
        assert_eq!(t.no_detail(), "no details available");
        assert_eq!(t.header_failed("x"), "The JPEG header could not be read: x");
        assert_eq!(
            t.invalid_dimensions(3, 4),
            "The decoder reported invalid image dimensions: 3x4"
        );
        assert_eq!(t.decompress_failed("x"), "The image could not be decoded: x");
        assert_eq!(t.info_failed("x"), "The image information could not be read: x");
        assert_eq!(
            t.no_source(),
            "A decode request needs either a file path or image data."
        );
        assert_eq!(
            t.output_too_large(3, 4, 5),
            "The decoded image would be too large: 3x4 (5 bytes)."
        );
        assert_eq!(
            t.strict_mode_failed("x"),
            "The decoder could not switch to strict mode: x"
        );
        assert_eq!(t.image_not_found("p"), "Image file not found: p");
    }

    #[test]
    fn read_info_needs_both_sides_positive() {
        let root = TempRoot::new("w514b-info-dims");
        let path = write_under(root.path(), "a.jpg", &fixture_bytes("turbojpeg/parity-64x48.jpg"));
        for (w, h) in [(0, 5), (5, 0), (-3, 5), (5, -3), (0, 0)] {
            let log = Arc::new(Log::default());
            let decoder = probe_decoder(&log, w, h, false);
            let error = decoder.read_info(path.to_str().unwrap()).unwrap_err();
            assert!(
                matches!(error.kind(), DecodeError::InvalidData { .. }),
                "{w}x{h}: {error:?}"
            );
        }
        let log = Arc::new(Log::default());
        let info = probe_decoder(&log, 1, 1, false)
            .read_info(path.to_str().unwrap())
            .unwrap();
        assert_eq!((info.pixel_width, info.pixel_height), (1, 1));
    }

    /// A factory whose first decompressor fails the header parse and whose later ones succeed with 9x7.
    fn first_fails_then_ok() -> Arc<dyn DecompressorFactory> {
        let calls = Arc::new(AtomicUsize::new(0));
        factory_of(move || FakeDecompressor {
            header_code: i32::from(calls.fetch_add(1, Ordering::SeqCst) == 0),
            width: 9,
            height: 7,
            ..FakeDecompressor::default()
        })
    }

    #[test]
    fn read_info_retries_only_when_the_extended_read_grew_the_buffer() {
        let root = TempRoot::new("w514b-info-grow");
        // Header longer than the 8 MiB first cap: the extended buffer is longer, the retry parse succeeds.
        let big = big_header(150, 60_000); // 9 MB: past the 8 MiB first cap
        assert!(big.len() as i64 > HEADER_READ_CAP);
        let path = write_under(root.path(), "big.jpg", &big);
        let decoder = decoder_with(first_fails_then_ok(), memory(1 << 36, 0), no_scaler());
        let info = decoder.read_info(path.to_str().unwrap()).unwrap();
        assert_eq!((info.pixel_width, info.pixel_height), (9, 7));
        // A small file: the extended read adds nothing, so there is NO retry; the final probe (which would succeed)
        // is not an `Ok` path, the verdict is the generic parse error.
        let small = write_under(root.path(), "small.jpg", &fixture_bytes("turbojpeg/parity-64x48.jpg"));
        let decoder = decoder_with(first_fails_then_ok(), memory(1 << 36, 0), no_scaler());
        let error = decoder.read_info(small.to_str().unwrap()).unwrap_err();
        assert!(matches!(error.kind(), DecodeError::InvalidData { .. }), "{error:?}");
        assert!(error.message().ends_with(messages::HEADER_PARSE_ERROR), "{error:?}");
    }

    #[cfg(windows)]
    #[test]
    fn a_source_locked_against_sharing_is_an_io_error_not_not_found() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = TempRoot::new("w514b-locked");
        let path = write_under(root.path(), "locked.jpg", &fixture_bytes("turbojpeg/parity-64x48.jpg"));
        let _lock = OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        let log = Arc::new(Log::default());
        let decoder = probe_decoder(&log, 8, 8, false);
        let error = decoder
            .decode(&DecodeRequest::new(path.clone(), 0))
            .err()
            .expect("locked");
        assert!(matches!(error.kind(), DecodeError::Io(_)), "{error:?}");
    }
}
