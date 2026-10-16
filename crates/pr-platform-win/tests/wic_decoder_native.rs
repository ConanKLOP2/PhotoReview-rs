#![cfg(windows)]
//! Red ports of the WIC-dependent tests of `WicDirectDecoder` (C# baseline d92d6aae). Every test here runs real WIC and is
//! `#[ignore = "native: needs WIC"]`; run them on `windows-latest` with `-- --ignored`. The golden runners
//! `golden_wic_direct_decode_matches_csharp` and `golden_wic_direct_read_info_matches_csharp` check every row of
//! `tests/golden/wic-direct-decode.json` (149 rows) and `wic-direct-read-info.json` (26 rows); the named tests select the rows
//! that the C# test they port exercised, so each named test says which C# test it covers.
//!
//! Pixel rule (contract T11): `pixels_sha256` is compared only when `PHOTOREVIEW_RS_WIC_VERSION` holds the running
//! `windowscodecs.dll` version recorded in the golden `environment` row; otherwise the five samples are compared with a per-channel
//! tolerance of 3. Every other field is exact. Goldens are never regenerated here.
//!
//! Fixture substitutions (documented per test): a few C# tests build their images with WPF encoders; their committed fixture
//! of the same shape stands in (`// Fixture substitution:`).
//!
//! DOCUMENTED SKIPS (non-portable, recorded in the PR): `Decode_ArgumentNullException_PropagatesUnchanged` (Rust has no null;
//! `// DIVERGENCE: no null in Rust`), `WicDirectDecoderPixelParityTests` (WPF / pre-WP-03 reference, no Rust counterpart; facts
//! pinned by the `wic-direct-decode` golden), `TryGetNativeReducedSize_CodecThrowsComException_IsNotUseful` (needs a `#[implement]`
//! COM fake, contract OQ4; the filter is covered by `is_color_metadata_fault` in `wic_decoder.rs`), `ReadColorContexts_Succeeds_ReturnsTheContexts`
//! (covered by the native ICC goldens `icc-p3-jpeg` and `icc-p3-png-alpha`, as the contract says).

use std::fs;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use pr_core::abstractions::{PhysicalSourceReader, ReadSeek, SourceReadPriority, SourceReader};
use pr_core::session::store::sha256;
use pr_imaging::decode_types::{DecodeBox, DecodeRequest, DecodedImageView, PixelLease, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, PlatformImageCodec};
use pr_imaging::exif_summary::codec::encode as encode_exif;
use pr_imaging::fallback::FallbackImageDecoder;
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::wic::decoder::WicDirectDecoder;
use pr_platform_win::wic::interop::outstanding_on_this_thread;
use pr_test_support::{TempRoot, golden};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------

/// Records every `open_source` call (file name, priority, buffer size) like the C# `ISourceReader` double.
#[derive(Default)]
struct RecordingReader {
    calls: Mutex<Vec<ReadCall>>,
    /// Streams whose `read` fails with `ErrorKind::InvalidInput` (the C# `ArgumentException` of `Stream.Read`, OQ3).
    fail_reads: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadCall {
    file: String,
    priority: String,
    buffer_size: usize,
}

impl RecordingReader {
    fn failing() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            fail_reads: true,
        }
    }

    fn calls(&self) -> Vec<ReadCall> {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// A stream whose every read is an `InvalidInput` failure.
struct FailingReads(File);

impl Read for FailingReads {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "stream read rejected"))
    }
}

impl Seek for FailingReads {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}

impl SourceReader for RecordingReader {
    fn open_source(
        &self,
        path: &Path,
        priority: SourceReadPriority,
        buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(ReadCall {
                file: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                priority: format!("{priority:?}"),
                buffer_size,
            });
        let file = File::open(path)?;
        if self.fail_reads {
            Ok(Box::new(FailingReads(file)))
        } else {
            Ok(Box::new(file))
        }
    }
}

/// A codec double that fails every hand-over with the given error.
struct FailingCodec(fn() -> DecodeError);

impl PlatformImageCodec for FailingCodec {
    fn name(&self) -> &'static str {
        "pixels"
    }

    fn image_from_pixels(&self, _pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
        Err((self.0)())
    }

    fn to_pixels<'a>(&self, _platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
        Err(DecodeError::Disposed {
            message: "not used by the decoder".to_owned(),
        })
    }
}

fn memory() -> Arc<dyn MemoryInfoSource> {
    Arc::new(|| MemoryInfo {
        total_available: 64 * 1024 * 1024 * 1024,
        load: 0,
    })
}

fn decoder(reader: Arc<RecordingReader>) -> WicDirectDecoder {
    WicDirectDecoder::new(
        PixelBufferImageCodec::shared(),
        Some(reader as Arc<dyn SourceReader>),
        memory(),
    )
}

fn physical_decoder() -> WicDirectDecoder {
    WicDirectDecoder::new(
        PixelBufferImageCodec::shared(),
        Some(Arc::new(PhysicalSourceReader) as Arc<dyn SourceReader>),
        memory(),
    )
}

fn fixtures() -> PathBuf {
    golden::repo_root().join("tests").join("fixtures").join("wic-direct")
}

fn fixture(name: &str) -> PathBuf {
    fixtures().join(name)
}

fn golden_row(file: &str, name: &str) -> Value {
    golden::load(file)
        .cases
        .into_iter()
        .find(|c| c["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("{file}: no row {name}"))
}

fn decode_row(name: &str) -> Value {
    golden_row("wic-direct-decode", name)
}

fn read_info_row(name: &str) -> Value {
    golden_row("wic-direct-read-info", name)
}

/// The path a row names: `file`, `missing`, `missing_dir`, a literal `path`, or the `label` of a bytes request.
fn request_path(case: &Value) -> PathBuf {
    let root = fixtures();
    if let Some(label) = case.get("label").and_then(Value::as_str) {
        PathBuf::from(label)
    } else if let Some(path) = case.get("path").and_then(Value::as_str) {
        PathBuf::from(path)
    } else if let Some(missing) = case.get("missing").and_then(Value::as_str) {
        root.join(missing)
    } else if let Some(missing) = case.get("missing_dir").and_then(Value::as_str) {
        root.join("no-such-dir").join(missing)
    } else {
        root.join(golden::str_of(case, "file"))
    }
}

fn build_request(case: &Value) -> DecodeRequest {
    let mut request = DecodeRequest::with_box(
        request_path(case),
        DecodeBox::new(
            case.get("box_w").and_then(Value::as_i64).unwrap_or(0) as i32,
            case.get("box_h").and_then(Value::as_i64).unwrap_or(0) as i32,
        ),
    );
    if let Some(apply) = case.get("apply_orientation").and_then(Value::as_bool) {
        request = request.with_apply_orientation(apply);
    }
    if let Some(orientation) = case.get("source_orientation").and_then(Value::as_i64) {
        request = request.with_source_orientation(orientation as i32);
    }
    if case.get("priority").and_then(Value::as_str) == Some("Preload") {
        request = request.with_priority(SourceReadPriority::Preload);
    }
    if case.get("from_bytes").and_then(Value::as_bool) == Some(true) {
        let bytes = fs::read(fixture(golden::str_of(case, "file"))).expect("fixture bytes");
        request = request.with_bytes(Arc::from(bytes));
    }
    request
}

fn expected_reader_calls(case: &Value) -> Vec<ReadCall> {
    case["reader_calls"]
        .as_array()
        .expect("reader_calls")
        .iter()
        .map(|call| ReadCall {
            file: golden::str_of(call, "file").to_owned(),
            priority: golden::str_of(call, "priority").to_owned(),
            buffer_size: golden::i64_of(call, "buffer_size") as usize,
        })
        .collect()
}

/// The .NET exception type of a golden row, as a `DecodeError` variant (one table per test file).
fn variant_matches(dotnet_type: &str, error: &DecodeError) -> bool {
    let kind = error.kind();
    match dotnet_type {
        "PhotoReview.Imaging.Decoding.DecoderMemoryAdmissionException" => {
            matches!(kind, DecodeError::MemoryAdmission { .. })
        }
        "System.IO.InvalidDataException" => matches!(kind, DecodeError::InvalidData { .. }),
        "System.NotSupportedException" => matches!(kind, DecodeError::NotSupported { .. }),
        "System.Runtime.InteropServices.COMException" => matches!(kind, DecodeError::Com { .. }),
        "System.ArgumentException" => matches!(kind, DecodeError::InvalidArgument { .. }),
        "System.InvalidOperationException" => matches!(kind, DecodeError::InvalidOperation { .. }),
        "System.OutOfMemoryException" => matches!(kind, DecodeError::OutOfMemory { .. }),
        "System.InvalidCastException" => matches!(kind, DecodeError::InvalidCast { .. }),
        "System.IO.FileNotFoundException" => matches!(kind, DecodeError::FileNotFound { .. }),
        // T5: a missing directory is `io::ErrorKind::NotFound` in Rust (FileNotFound); only the behaviour is compared.
        "System.IO.DirectoryNotFoundException" => {
            !FallbackImageDecoder::is_fallbackable(error) && error.is_io_exception()
        }
        other => panic!("golden exception type {other} has no DecodeError variant in the table"),
    }
}

fn check_error(name: &str, expected: &Value, actual: &DecodeError) {
    let dotnet = golden::str_of(expected, "type");
    assert!(
        variant_matches(dotnet, actual),
        "{name}: expected a {dotnet} equivalent, got {actual}"
    );
    assert_eq!(
        FallbackImageDecoder::is_fallbackable(actual),
        expected["fallbackable"].as_bool().expect("golden fallbackable"),
        "{name}: INV-12 fallback verdict"
    );
    if let DecodeError::Com { hresult, .. } = actual.kind() {
        assert_eq!(
            i64::from(*hresult),
            golden::i64_of(expected, "hresult"),
            "{name}: HRESULT"
        );
    }
    if matches!(
        dotnet,
        "PhotoReview.Imaging.Decoding.DecoderMemoryAdmissionException"
            | "System.IO.InvalidDataException"
            | "System.NotSupportedException"
    ) {
        assert_eq!(
            actual.message(),
            golden::str_of(expected, "message"),
            "{name}: message built by this unit"
        );
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// T11: the exact pixel hash is compared only on the WIC build the golden was recorded on.
fn same_wic_build() -> bool {
    let env = decode_row("environment");
    std::env::var("PHOTOREVIEW_RS_WIC_VERSION")
        .map(|v| Some(v.as_str()) == env["windowscodecs_version"].as_str())
        .unwrap_or(false)
}

fn sample_at(bytes: &[u8], stride: usize, x: usize, y: usize) -> [u8; 4] {
    let i = y * stride + x * 4;
    [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
}

fn parse_sample(text: &str) -> [u8; 4] {
    let mut out = [0u8; 4];
    for (slot, part) in out.iter_mut().zip(text.split(',')) {
        *slot = part.parse().expect("sample channel");
    }
    out
}

/// Compares the five golden samples (tl, tr, bl, br, center) of a decoded view.
fn check_samples(name: &str, expected: &Value, bytes: &[u8], stride: usize, width: usize, height: usize) {
    let exact = same_wic_build();
    let points = [
        ("tl", 0, 0),
        ("tr", width - 1, 0),
        ("bl", 0, height - 1),
        ("br", width - 1, height - 1),
        ("center", width / 2, height / 2),
    ];
    for (key, x, y) in points {
        let want = parse_sample(golden::str_of(&expected["samples"], key));
        let got = sample_at(bytes, stride, x, y);
        if exact {
            assert_eq!(got, want, "{name}: sample {key}");
        } else {
            for channel in 0..4 {
                assert!(
                    got[channel].abs_diff(want[channel]) <= 3,
                    "{name}: sample {key} channel {channel}: got {got:?}, want {want:?}"
                );
            }
        }
    }
}

/// The EXIF summary encoded the way the golden `exif_encoded` column records it.
fn exif_hex(view: &dyn DecodedImageView) -> Option<String> {
    view.exif().map(|summary| hex(&encode_exif(Some(summary))))
}

/// Checks a successful decode against the `ok` object of a golden row.
fn check_view(name: &str, ok: &Value, view: &dyn DecodedImageView) {
    assert_eq!(
        i64::from(view.pixel_width()),
        golden::i64_of(ok, "pixel_width"),
        "{name}: pixel_width"
    );
    assert_eq!(
        i64::from(view.pixel_height()),
        golden::i64_of(ok, "pixel_height"),
        "{name}: pixel_height"
    );
    assert_eq!(
        view.downscaled(),
        golden::bool_of(ok, "downscaled"),
        "{name}: downscaled"
    );
    assert_eq!(
        i64::from(view.orientation()),
        golden::i64_of(ok, "orientation"),
        "{name}: orientation"
    );
    assert_eq!(
        i64::from(view.original_width()),
        golden::i64_of(ok, "original_width"),
        "{name}: original_width"
    );
    assert_eq!(
        i64::from(view.original_height()),
        golden::i64_of(ok, "original_height"),
        "{name}: original_height"
    );
    assert_eq!(
        view.estimated_bytes(),
        golden::i64_of(ok, "estimated_bytes"),
        "{name}: estimated_bytes"
    );
    assert_eq!(
        format!("{:?}", view.actual_backend()),
        golden::str_of(ok, "backend"),
        "{name}: backend"
    );
    match ok["exif_encoded"].as_str() {
        Some(want) => assert_eq!(exif_hex(view).as_deref(), Some(want), "{name}: exif_encoded"),
        None => assert!(view.exif().is_none(), "{name}: no EXIF expected"),
    }
    let camera_make = view.exif().and_then(|e| e.camera_make.clone());
    assert_eq!(
        camera_make.as_deref(),
        ok["camera_make"].as_str(),
        "{name}: camera_make"
    );

    let pixels = view
        .platform_image()
        .as_pixels()
        .unwrap_or_else(|| panic!("{name}: WicDirect hands over pixel buffers"));
    let layout = match pixels.layout() {
        PixelLayout::Bgr32 => "Bgr32",
        PixelLayout::Pbgra32 => "Pbgra32",
    };
    assert_eq!(layout, golden::str_of(ok, "layout"), "{name}: layout");
    assert_eq!(
        i64::from(pixels.stride()),
        golden::i64_of(ok, "stride"),
        "{name}: stride"
    );
    let bytes = pixels.bytes().expect("live buffer");
    if same_wic_build() {
        assert_eq!(
            hex(&sha256(bytes)),
            golden::str_of(ok, "pixels_sha256"),
            "{name}: pixels_sha256"
        );
    }
    check_samples(
        name,
        ok,
        bytes,
        pixels.stride() as usize,
        pixels.width() as usize,
        pixels.height() as usize,
    );
}

fn assert_reader_calls(name: &str, case: &Value, reader: &RecordingReader) {
    assert_eq!(reader.calls(), expected_reader_calls(case), "{name}: source opens");
}

/// One golden decode row: request, reader calls, and the result (view or error).
fn check_decode(case: &Value) {
    let name = golden::str_of(case, "name");
    let reader = Arc::new(RecordingReader::default());
    let result = decoder(reader.clone()).decode(&build_request(case));
    assert_reader_calls(name, case, &reader);
    match case.get("ok") {
        Some(ok) => {
            let view = result.unwrap_or_else(|e| panic!("{name}: expected ok, got {e}"));
            check_view(name, ok, view.as_ref());
        }
        None => match result {
            Ok(_) => panic!("{name}: expected an error"),
            Err(e) => check_error(name, &case["error"], &e),
        },
    }
}

/// One golden read-info row.
fn check_read_info(case: &Value) {
    let name = golden::str_of(case, "name");
    let reader = Arc::new(RecordingReader::default());
    let path = request_path(case);
    let result = decoder(reader.clone()).read_info(&path.to_string_lossy());
    assert_reader_calls(name, case, &reader);
    match case.get("ok") {
        Some(ok) => {
            let info = result.unwrap_or_else(|e| panic!("{name}: expected ok, got {e}"));
            assert_eq!(
                i64::from(info.pixel_width),
                golden::i64_of(ok, "pixel_width"),
                "{name}: pixel_width"
            );
            assert_eq!(
                i64::from(info.pixel_height),
                golden::i64_of(ok, "pixel_height"),
                "{name}: pixel_height"
            );
            assert_eq!(
                i64::from(info.orientation),
                golden::i64_of(ok, "orientation"),
                "{name}: orientation"
            );
            assert_eq!(i64::from(info.width()), golden::i64_of(ok, "width"), "{name}: width");
            assert_eq!(i64::from(info.height()), golden::i64_of(ok, "height"), "{name}: height");
        }
        None => match result {
            Ok(_) => panic!("{name}: expected an error"),
            Err(e) => check_error(name, &case["error"], &e),
        },
    }
}

fn decode_bytes(
    decoder: &WicDirectDecoder,
    label: &Path,
    bytes: Vec<u8>,
) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
    decoder.decode(&DecodeRequest::new(label, 0).with_bytes(Arc::from(bytes)))
}

// ---------------------------------------------------------------------------------------------------------------
// Golden runners (every row)
// ---------------------------------------------------------------------------------------------------------------

/// Every `wic-direct-decode` row (149, the `environment` row carries the WIC build only).
#[test]
#[ignore = "native: needs WIC"]
fn golden_wic_direct_decode_matches_csharp() {
    let golden = golden::load("wic-direct-decode");
    for case in golden
        .cases
        .iter()
        .filter(|c| c["name"].as_str() != Some("environment"))
    {
        check_decode(case);
    }
}

/// Every `wic-direct-read-info` row (26).
#[test]
#[ignore = "native: needs WIC"]
fn golden_wic_direct_read_info_matches_csharp() {
    let golden = golden::load("wic-direct-read-info");
    for case in golden
        .cases
        .iter()
        .filter(|c| c["name"].as_str() != Some("environment"))
    {
        check_read_info(case);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// C# WicDirectTests
// ---------------------------------------------------------------------------------------------------------------

// C#: WicDirectTests.PngDecodesWithZeroLoss
// Fixture substitution: the C# test builds a 64x48 Bgra32 gradient in code; the committed `gradient-64x48-bgra32.png` and its
// golden samples and hash (`format-png-bgra32`) stand in for it.
#[test]
#[ignore = "native: needs WIC"]
fn png_decodes_with_zero_loss() {
    check_decode(&decode_row("format-png-bgra32"));
}

// C#: WicDirectTests.ReadInfoDetectsOrientation
#[test]
#[ignore = "native: needs WIC"]
fn read_info_detects_orientation() {
    for orientation in 1..=8 {
        check_read_info(&read_info_row(&format!("orient-{orientation}")));
    }
}

// C#: WicDirectTests.DecodeAppliesOrientationCorners
#[test]
#[ignore = "native: needs WIC"]
fn decode_applies_orientation_corners() {
    for orientation in 1..=8 {
        check_decode(&decode_row(&format!("orient-{orientation}-apply")));
    }
}

// C#: WicDirectTests.DownscaleProducesTargetWidth
#[test]
#[ignore = "native: needs WIC"]
fn downscale_produces_target_width() {
    check_decode(&decode_row("dct-400x300-to-100"));
}

// C#: WicDirectTests.DecodesFromMemoryBuffer (bytes decode after the file is deleted)
#[test]
#[ignore = "native: needs WIC"]
fn decodes_from_memory_buffer() {
    let temp = TempRoot::new("wic-memory-buffer");
    let path = temp.join("plain.jpg");
    fs::copy(fixture("plain-64x48.jpg"), &path).expect("copy fixture");
    let bytes = fs::read(&path).expect("read bytes");
    fs::remove_file(&path).expect("delete the file");
    let view = decode_bytes(&physical_decoder(), &path, bytes).expect("bytes decode without the file");
    assert_eq!((view.pixel_width(), view.pixel_height()), (64, 48));
}

// ---------------------------------------------------------------------------------------------------------------
// C# DecodeSizeDecisionMutationTests (WicDirect rows, through the golden `wic-direct-decode`)
// ---------------------------------------------------------------------------------------------------------------

// C#: DecodeSizeDecisionMutationTests.Decode_BoxAgainstStoredSize_ExactSizeDownscaledFlagAndOriginal
#[test]
#[ignore = "native: needs WIC"]
fn decode_box_against_stored_size_exact_size_downscaled_flag_and_original() {
    check_decode(&decode_row("size-plain-32x24"));
}

// C#: DecodeSizeDecisionMutationTests.Decode_LegacyWidthHeightRequest_BehavesLikeTheBox
#[test]
#[ignore = "native: needs WIC"]
fn decode_legacy_width_height_request_behaves_like_the_box() {
    let row = decode_row("size-plain-40x0");
    let view = physical_decoder()
        .decode(&DecodeRequest::new(fixture("plain-64x48.jpg"), 40))
        .expect("legacy width-only request decodes");
    check_view("size-plain-40x0", &row["ok"], view.as_ref());
}

// C#: DecodeSizeDecisionMutationTests.Decode_FromPreReadBytes_SameDecisionsAsFromTheFile
#[test]
#[ignore = "native: needs WIC"]
fn decode_from_pre_read_bytes_same_decisions_as_from_the_file() {
    check_decode(&decode_row("dct-800x600-to-100-bytes"));
    check_decode(&decode_row("dct-800x600-to-100"));
}

// C#: DecodeSizeDecisionMutationTests.Decode_RotatedSource_BoxAppliesToDisplayedSizeAndOriginalIsSwapped
#[test]
#[ignore = "native: needs WIC"]
fn decode_rotated_source_box_applies_to_displayed_size_and_original_is_swapped() {
    check_decode(&decode_row("rotated-rot6-24x32"));
}

// C#: DecodeSizeDecisionMutationTests.Decode_RotatedSourceWithoutApplyOrientation_KeepsStoredSizeAndReportsOrientationOne
#[test]
#[ignore = "native: needs WIC"]
fn decode_rotated_source_without_apply_orientation_keeps_stored_size_and_reports_orientation_one() {
    check_decode(&decode_row("rot6-no-apply-keeps-stored"));
}

// C#: DecodeSizeDecisionMutationTests.Decode_RotatedSourceWithBoxButWithoutApplyOrientation_BoxFitsTheStoredGrid
#[test]
#[ignore = "native: needs WIC"]
fn decode_rotated_source_with_box_but_without_apply_orientation_box_fits_the_stored_grid() {
    check_decode(&decode_row("rot6-no-apply-box-fits-stored-grid"));
}

// C#: DecodeSizeDecisionMutationTests.Decode_SourceOrientationWithoutApplyOrientation_IsIgnored
#[test]
#[ignore = "native: needs WIC"]
fn decode_source_orientation_without_apply_orientation_is_ignored() {
    check_decode(&decode_row("plain-source6-no-apply-ignored"));
}

// C#: DecodeSizeDecisionMutationTests.Decode_SourceOrientationWithApplyOrientation_RotatesAndSwapsTheOriginal
#[test]
#[ignore = "native: needs WIC"]
fn decode_source_orientation_with_apply_orientation_rotates_and_swaps_the_original() {
    check_decode(&decode_row("plain-source6-apply-rotates"));
}

// ---------------------------------------------------------------------------------------------------------------
// C# SourceReaderSeamMutationTests
// ---------------------------------------------------------------------------------------------------------------

// C#: SourceReaderSeamMutationTests.WicDecode_InjectedReader_IsUsedWithOneMegabyteBufferAndTheRequestPriority
#[test]
#[ignore = "native: needs WIC"]
fn wic_decode_injected_reader_is_used_with_one_megabyte_buffer_and_the_request_priority() {
    let reader = Arc::new(RecordingReader::default());
    let request = DecodeRequest::new(fixture("plain-64x48.jpg"), 0).with_priority(SourceReadPriority::Preload);
    decoder(reader.clone())
        .decode(&request)
        .expect("decodes through the injected reader");
    assert_eq!(
        reader.calls(),
        vec![ReadCall {
            file: "plain-64x48.jpg".to_owned(),
            priority: "Preload".to_owned(),
            buffer_size: 1024 * 1024,
        }]
    );
}

// C#: SourceReaderSeamMutationTests.WicReadInfo_InjectedReader_IsUsedWithSixtyFourKilobyteBufferOnTheViewerLane
#[test]
#[ignore = "native: needs WIC"]
fn wic_read_info_injected_reader_is_used_with_sixty_four_kilobyte_buffer_on_the_viewer_lane() {
    let reader = Arc::new(RecordingReader::default());
    let path = fixture("plain-64x48.jpg");
    decoder(reader.clone())
        .read_info(&path.to_string_lossy())
        .expect("reads through the injected reader");
    assert_eq!(
        reader.calls(),
        vec![ReadCall {
            file: "plain-64x48.jpg".to_owned(),
            priority: "Viewer".to_owned(),
            buffer_size: 64 * 1024,
        }]
    );
}

// ---------------------------------------------------------------------------------------------------------------
// C# WicDirectInternalsMutationTests: the native reduction COM paths (private, reached through decode)
// ---------------------------------------------------------------------------------------------------------------

// C#: WicDirectInternalsMutationTests.TryGetNativeReducedSize_FrameWithoutTransformInterface_IsNotUseful
// `try_get_native_reduced_size` is private: a PNG frame has no IWICBitmapSourceTransform, so the 64x48 PNG decoded to width 32
// must come out through the high-quality scaler (downscaled, 32x24, never a DCT size).
#[test]
#[ignore = "native: needs WIC"]
fn try_get_native_reduced_size_frame_without_transform_interface_is_not_useful() {
    let view = physical_decoder()
        .decode(&DecodeRequest::new(fixture("gradient-64x48-bgra32.png"), 32))
        .expect("PNG decodes");
    assert_eq!((view.pixel_width(), view.pixel_height()), (32, 24));
    assert!(view.downscaled());
}

// DOCUMENTED SKIP: `TryGetNativeReducedSize_CodecThrowsComException_IsNotUseful` needs a managed COM fake of
// IWICBitmapFrameDecode + IWICBitmapSourceTransform (contract OQ4); its filter, `Com` -> not useful, is ported as the
// `Com`-kind branch of `try_get_native_reduced_size` and covered by `is_color_metadata_fault` / `decode_error_from_wic` tests.

// C#: WicDirectInternalsMutationTests.Decode_TiffWithExifMake_ReadsItThroughTheTiffIfdRoot
#[test]
#[ignore = "native: needs WIC"]
fn decode_tiff_with_exif_make_reads_it_through_the_tiff_ifd_root() {
    check_decode(&decode_row("exif-tiff-make"));
}

// C#: WicDirectInternalsMutationTests.Decode_JpegWithExifMake_ReadsItThroughTheJpegIfdRoot
#[test]
#[ignore = "native: needs WIC"]
fn decode_jpeg_with_exif_make_reads_it_through_the_jpeg_ifd_root() {
    check_decode(&decode_row("exif-jpeg-make"));
}

// C#: WicDirectInternalsMutationTests.Decode_PngWithoutExifContainer_HasNoExif
#[test]
#[ignore = "native: needs WIC"]
fn decode_png_without_exif_container_has_no_exif() {
    check_decode(&decode_row("exif-png-none"));
}

// C#: WicDirectInternalsMutationTests.Decode_ExifAdobeRgbColorSpace_IsTransformedToSrgb_WhileSrgbAndUntaggedAreNot
#[test]
#[ignore = "native: needs WIC"]
fn decode_exif_adobe_rgb_color_space_is_transformed_to_srgb_while_srgb_and_untagged_are_not() {
    check_decode(&decode_row("colorspace-adobe"));
    check_decode(&decode_row("colorspace-srgb"));
    check_decode(&decode_row("colorspace-none"));
}

// ---------------------------------------------------------------------------------------------------------------
// C# WicDirectDecoderWp03ReviewTests (decode-level rows; the buffer rows are in wic_decoder_buffers.rs)
// ---------------------------------------------------------------------------------------------------------------

// C#: WicDirectDecoderWp03ReviewTests.Decode_PlainArgumentException_IsInvalidData
#[test]
#[ignore = "native: needs WIC"]
fn decode_plain_argument_exception_is_invalid_data() {
    let decoder = WicDirectDecoder::new(
        Arc::new(FailingCodec(|| DecodeError::InvalidArgument {
            message: "codec rejected the pixels".to_owned(),
        })),
        Some(Arc::new(PhysicalSourceReader) as Arc<dyn SourceReader>),
        memory(),
    );
    match decoder.decode(&DecodeRequest::new(fixture("plain-64x48.jpg"), 0)) {
        Ok(_) => panic!("the codec refuses the pixels"),
        Err(e) => assert!(matches!(e.kind(), DecodeError::InvalidData { .. }), "{e}"),
    }
}

// C#: WicDirectDecoderWp03ReviewTests.Decode_OutOfMemory_Propagates
#[test]
#[ignore = "native: needs WIC"]
fn decode_out_of_memory_propagates() {
    let decoder = WicDirectDecoder::new(
        Arc::new(FailingCodec(|| DecodeError::OutOfMemory {
            message: "codec ran out of memory".to_owned(),
        })),
        Some(Arc::new(PhysicalSourceReader) as Arc<dyn SourceReader>),
        memory(),
    );
    match decoder.decode(&DecodeRequest::new(fixture("plain-64x48.jpg"), 0)) {
        Ok(_) => panic!("the codec runs out of memory"),
        Err(e) => assert!(matches!(e.kind(), DecodeError::OutOfMemory { .. }), "{e}"),
    }
}

// DOCUMENTED SKIP: `Decode_ArgumentNullException_PropagatesUnchanged` has no Rust twin: there is no null argument in Rust.
// DIVERGENCE: no null in Rust (contract trap T4); the relabel rule is ported in `wic_decoder.rs`.

// ---------------------------------------------------------------------------------------------------------------
// C# WicDirectFaultInjectionTests (native stream fault)
// ---------------------------------------------------------------------------------------------------------------

// C#: WicDirectFaultInjectionTests.Decode_StreamReportsInvalidArgument_IsInvalidDataNotArgumentException
#[test]
#[ignore = "native: needs WIC"]
fn decode_stream_reports_invalid_argument_is_invalid_data_not_argument_exception() {
    let reader = Arc::new(RecordingReader::failing());
    match decoder(reader).decode(&DecodeRequest::new(fixture("plain-64x48.jpg"), 0)) {
        Ok(_) => panic!("the stream fails every read"),
        Err(e) => assert!(matches!(e.kind(), DecodeError::InvalidData { .. }), "{e}"),
    }
}

// C#: WicDirectFaultInjectionTests.ReadInfo_StreamReportsInvalidArgument_IsInvalidDataNotArgumentException
#[test]
#[ignore = "native: needs WIC"]
fn read_info_stream_reports_invalid_argument_is_invalid_data_not_argument_exception() {
    let reader = Arc::new(RecordingReader::failing());
    match decoder(reader).read_info(&fixture("plain-64x48.jpg").to_string_lossy()) {
        Ok(_) => panic!("the stream fails every read"),
        Err(e) => assert!(matches!(e.kind(), DecodeError::InvalidData { .. }), "{e}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// C# ErrorHandlingReviewDecodingTests, DecoderOutputFormatTests, ExifOrientationPureTests, MemoryBufferDecodeTests,
// SourceOrientationOverrideTests
// ---------------------------------------------------------------------------------------------------------------

// C#: ErrorHandlingReviewDecodingTests.WicDirect_HappyPath_Unchanged
#[test]
#[ignore = "native: needs WIC"]
fn wic_direct_happy_path_unchanged() {
    check_decode(&decode_row("size-plain-64x48"));
}

// C#: DecoderOutputFormatTests.WicDirectJpegOutputsMaterializedBgr32 (4 rows)
#[test]
#[ignore = "native: needs WIC"]
fn wic_direct_jpeg_outputs_materialized_bgr32() {
    for name in ["full-400x300", "exif-jpeg-make", "colorspace-srgb", "icc-p3-jpeg"] {
        check_decode(&decode_row(name));
    }
}

// C#: DecoderOutputFormatTests.WicDirectPngWithAlphaOutputsPremultipliedPbgra32
// Fixture substitution: the C# test uses a 16x8 half-transparent PNG that is not committed; `icc-p3-alpha-64x48.png` (Pbgra32)
// stands in, and the premultiplied rule is checked on every pixel (colour channels never exceed alpha).
#[test]
#[ignore = "native: needs WIC"]
fn wic_direct_png_with_alpha_outputs_premultiplied_pbgra32() {
    let row = decode_row("icc-p3-png-alpha");
    check_decode(&row);
    let view = physical_decoder()
        .decode(&DecodeRequest::new(fixture("icc-p3-alpha-64x48.png"), 0))
        .expect("alpha PNG decodes");
    let pixels = view.platform_image().as_pixels().expect("pixels");
    assert_eq!(pixels.layout(), PixelLayout::Pbgra32);
    let bytes = pixels.bytes().expect("live buffer");
    for pixel in bytes.chunks_exact(4) {
        let alpha = pixel[3];
        assert!(
            pixel[..3].iter().all(|c| *c <= alpha),
            "premultiplied: colour <= alpha, got {pixel:?}"
        );
    }
}

// C#: ExifOrientationPureTests.WicDecode_OrientationAndExifSummary_EqualTheWpfMetadataRead
// Rust has no WPF: the orientation and the encoded EXIF equal the golden values.
#[test]
#[ignore = "native: needs WIC"]
fn wic_decode_orientation_and_exif_summary_equal_the_wpf_metadata_read() {
    check_decode(&decode_row("exif-jpeg-make"));
}

// C#: MemoryBufferDecodeTests.BytesWin_OverThePathLabel
#[test]
#[ignore = "native: needs WIC"]
fn bytes_win_over_the_path_label() {
    check_decode(&decode_row("bytes-win-over-path-label"));
}

// C#: SourceOrientationOverrideTests.AllDecoders_SourceOrientationOverride_WinsOverEmbeddedExif (WicDirect rows)
#[test]
#[ignore = "native: needs WIC"]
fn all_decoders_source_orientation_override_wins_over_embedded_exif() {
    check_decode(&decode_row("rot6-source1-overrides-exif"));
    check_decode(&decode_row("rot6-source3-overrides-exif"));
}

// C#: DecoderConcurrencyTests.ParallelDecodes_MatchTheSerialReference (8 threads, per-thread COM setup, trap T7)
#[test]
#[ignore = "native: needs WIC"]
fn parallel_decodes_match_the_serial_reference() {
    let decoder = Arc::new(physical_decoder());
    let path = fixture("plain-64x48.jpg");
    let decode_hash = |decoder: &WicDirectDecoder| -> [u8; 32] {
        let view = decoder
            .decode(&DecodeRequest::new(path.clone(), 0))
            .expect("plain JPEG decodes");
        sha256(
            view.platform_image()
                .as_pixels()
                .expect("pixels")
                .bytes()
                .expect("live buffer"),
        )
    };
    let serial = decode_hash(&decoder);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let decoder = Arc::clone(&decoder);
                let decode_hash = &decode_hash;
                scope.spawn(move || decode_hash(&decoder))
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().expect("worker thread"), serial);
        }
    });
}

// ---------------------------------------------------------------------------------------------------------------
// C# Metadata/DecoderExifTests (WicDirect rows)
// ---------------------------------------------------------------------------------------------------------------

fn assert_jpeg_make_exif(view: &dyn DecodedImageView, expected_hex: &str) {
    let exif = view.exif().expect("EXIF present");
    assert_eq!(exif.camera_make.as_deref(), Some("JpegMaker"));
    assert_eq!(exif_hex(view).as_deref(), Some(expected_hex));
}

// C#: DecoderExifTests.PreviewDecodeFromFileCarriesExif
#[test]
#[ignore = "native: needs WIC"]
fn preview_decode_from_file_carries_exif() {
    let expected = decode_row("exif-jpeg-make");
    let view = physical_decoder()
        .decode(&DecodeRequest::with_box(
            fixture("jpeg-make-32x24.jpg"),
            DecodeBox::new(16, 0),
        ))
        .expect("preview decode");
    assert_jpeg_make_exif(view.as_ref(), golden::str_of(&expected["ok"], "exif_encoded"));
}

// C#: DecoderExifTests.DecodeFromBytesCarriesExif
#[test]
#[ignore = "native: needs WIC"]
fn decode_from_bytes_carries_exif() {
    let expected = decode_row("exif-jpeg-make");
    let bytes = fs::read(fixture("jpeg-make-32x24.jpg")).expect("fixture");
    let view = decode_bytes(&physical_decoder(), &fixture("jpeg-make-32x24.jpg"), bytes).expect("bytes decode");
    assert_jpeg_make_exif(view.as_ref(), golden::str_of(&expected["ok"], "exif_encoded"));
}

// C#: DecoderExifTests.FullDecodeCarriesExif
#[test]
#[ignore = "native: needs WIC"]
fn full_decode_carries_exif() {
    check_decode(&decode_row("exif-jpeg-make"));
}

// C#: DecoderExifTests.NoExifGivesNull
#[test]
#[ignore = "native: needs WIC"]
fn no_exif_gives_null() {
    check_decode(&decode_row("exif-png-none"));
}

// C#: DecoderExifTests.PartialExifKeepsPresentTags
#[test]
#[ignore = "native: needs WIC"]
fn partial_exif_keeps_present_tags() {
    // C# `ExifTestData.EncodeJpegWithPartialExif`: only Make (IFD0) and ISO (EXIF IFD) are written; every other tag
    // is absent (the per-tag "not found" path). Rust has no WPF encoder, so the same two tags go into an APP1 segment
    // spliced into a committed baseline JPEG.
    let view = physical_decoder()
        .decode(
            &DecodeRequest::with_box("p.jpg", DecodeBox::new(32, 32)).with_bytes(Arc::from(jpeg_with_partial_exif())),
        )
        .expect("decodes");
    let exif = view.exif().expect("EXIF present");
    assert_eq!(exif.camera_make.as_deref(), Some("FUJIFILM"));
    assert_eq!(exif.iso, Some(3200));
    assert!(exif.camera_model.is_none(), "absent model stays absent");
    assert!(exif.lens_model.is_none(), "absent lens stays absent");
    assert!(exif.exposure_time.is_none(), "absent exposure stays absent");
}

/// `plain-64x48.jpg` with an APP1 EXIF segment (big-endian TIFF) holding only IFD0 Make = "FUJIFILM" and EXIF IFD
/// ISOSpeedRatings = 3200, inserted after the JFIF APP0 segment.
fn jpeg_with_partial_exif() -> Vec<u8> {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"MM\x00\x2A\x00\x00\x00\x08");
    // IFD0 at 8: 2 entries (Make -> offset 38, ExifIFD pointer -> 48), next IFD 0; ends at 38.
    tiff.extend_from_slice(&2u16.to_be_bytes());
    tiff.extend_from_slice(&[0x01, 0x0F, 0x00, 0x02, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 38]);
    tiff.extend_from_slice(&[0x87, 0x69, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 48]);
    tiff.extend_from_slice(&0u32.to_be_bytes());
    tiff.extend_from_slice(b"FUJIFILM\x00\x00");
    assert_eq!(tiff.len(), 48);
    // EXIF IFD at 48: ISOSpeedRatings (0x8827, SHORT) = 3200, next IFD 0.
    tiff.extend_from_slice(&1u16.to_be_bytes());
    tiff.extend_from_slice(&[0x88, 0x27, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x0C, 0x80, 0x00, 0x00]);
    tiff.extend_from_slice(&0u32.to_be_bytes());

    let mut payload = b"Exif\x00\x00".to_vec();
    payload.extend_from_slice(&tiff);
    let base = fs::read(fixture("plain-64x48.jpg")).expect("fixture");
    assert_eq!(&base[..2], &[0xFF, 0xD8], "SOI");
    let insert_at = if base[2..4] == [0xFF, 0xE0] {
        4 + usize::from(u16::from_be_bytes([base[4], base[5]]))
    } else {
        2
    };
    let mut jpeg = base[..insert_at].to_vec();
    jpeg.extend_from_slice(&[0xFF, 0xE1]);
    jpeg.extend_from_slice(&u16::try_from(payload.len() + 2).expect("segment length").to_be_bytes());
    jpeg.extend_from_slice(&payload);
    jpeg.extend_from_slice(&base[insert_at..]);
    jpeg
}

// C#: DecoderExifTests.PngHasNoExif
#[test]
#[ignore = "native: needs WIC"]
fn png_has_no_exif() {
    check_decode(&decode_row("exif-png-none"));
}

// ---------------------------------------------------------------------------------------------------------------
// C# WicLifetimeBalanceTests (outstanding COM objects on this thread return to the start value)
// ---------------------------------------------------------------------------------------------------------------

// C#: WicLifetimeBalanceTests.Decode_AllPipelineShapes_Balanced
#[test]
#[ignore = "native: needs WIC"]
fn decode_all_pipeline_shapes_balanced() {
    let decoder = physical_decoder();
    for name in [
        "size-plain-32x24",
        "dct-400x300-to-100",
        "format-png-bgra32",
        "format-tiff-cmyk32",
        "rot6-no-apply-keeps-stored",
        "orient-6-apply",
        "icc-p3-jpeg",
        "icc-p3-png-alpha",
        "exif-jpeg-make",
        "format-png-indexed8",
    ] {
        let row = decode_row(name);
        let before = outstanding_on_this_thread();
        let _ = decoder.decode(&build_request(&row));
        assert_eq!(
            outstanding_on_this_thread(),
            before,
            "{name}: every COM object is released"
        );
    }
}

// C#: WicLifetimeBalanceTests.DecodeAndReadInfo_DamagedFiles_Balanced
#[test]
#[ignore = "native: needs WIC"]
fn decode_and_read_info_damaged_files_balanced() {
    let decoder = physical_decoder();
    for name in ["fail-text", "fail-empty-file", "fail-truncated-30"] {
        let row = decode_row(name);
        let before = outstanding_on_this_thread();
        let _ = decoder.decode(&build_request(&row));
        assert_eq!(outstanding_on_this_thread(), before, "decode {name}");
    }
    for name in ["fail-text", "fail-empty-file", "fail-header-garbage"] {
        let row = read_info_row(name);
        let before = outstanding_on_this_thread();
        let _ = decoder.read_info(&request_path(&row).to_string_lossy());
        assert_eq!(outstanding_on_this_thread(), before, "read_info {name}");
    }
}

// C#: WicLifetimeBalanceTests.ReadInfo_Balanced
#[test]
#[ignore = "native: needs WIC"]
fn read_info_balanced() {
    let decoder = physical_decoder();
    for name in ["plain", "rot6", "png-bgra32", "tiff-make"] {
        let row = read_info_row(name);
        let before = outstanding_on_this_thread();
        let _ = decoder.read_info(&request_path(&row).to_string_lossy());
        assert_eq!(outstanding_on_this_thread(), before, "read_info {name}");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// C# DecodeBoxDecoderTests (WicDirect rows; sizes only, the fixtures stand in for the WPF-encoded images)
// ---------------------------------------------------------------------------------------------------------------

fn decode_boxed(file: &str, width: i32, height: i32) -> Arc<dyn DecodedImageView> {
    physical_decoder()
        .decode(&DecodeRequest::with_box(fixture(file), DecodeBox::new(width, height)))
        .unwrap_or_else(|e| panic!("{file}: {e}"))
}

// C#: DecodeBoxDecoderTests.Decode_LandscapeJpeg_FitsBoxHeight
#[test]
#[ignore = "native: needs WIC"]
fn decode_landscape_jpeg_fits_box_height() {
    let view = decode_boxed("gradient-800x600.jpg", 0, 150);
    assert_eq!((view.pixel_width(), view.pixel_height()), (200, 150));
}

// C#: DecodeBoxDecoderTests.Decode_PortraitJpeg_FitsBoxHeight
#[test]
#[ignore = "native: needs WIC"]
fn decode_portrait_jpeg_fits_box_height() {
    let view = physical_decoder()
        .decode(&DecodeRequest::with_box(
            fixture("rot6-64x48.jpg"),
            DecodeBox::new(0, 32),
        ))
        .expect("portrait decodes");
    assert_eq!((view.pixel_width(), view.pixel_height()), (24, 32));
}

// C#: DecodeBoxDecoderTests.Decode_ExifRotatedJpeg_BoxAppliedAfterOrientation (rows 6 and 8)
#[test]
#[ignore = "native: needs WIC"]
fn decode_exif_rotated_jpeg_box_applied_after_orientation() {
    check_decode(&decode_row("orient-6-box-50x50"));
    check_decode(&decode_row("orient-8-box-50x50"));
}

// C#: DecodeBoxDecoderTests.Decode_PanoramaJpeg_FitsBoxWidth
#[test]
#[ignore = "native: needs WIC"]
fn decode_panorama_jpeg_fits_box_width() {
    let view = decode_boxed("short-64x3.jpg", 16, 0);
    assert_eq!(view.pixel_width(), 16);
    assert!((1..=3).contains(&view.pixel_height()), "height keeps the aspect ratio");
}

// C#: DecodeBoxDecoderTests.Decode_TinyJpeg_NotUpscaled
#[test]
#[ignore = "native: needs WIC"]
fn decode_tiny_jpeg_not_upscaled() {
    check_decode(&decode_row("size-plain-640x480"));
}

// C#: DecodeBoxDecoderTests.Decode_Png_FitsBox
#[test]
#[ignore = "native: needs WIC"]
fn decode_png_fits_box() {
    let view = decode_boxed("gradient-64x48-bgra32.png", 16, 16);
    assert_eq!((view.pixel_width(), view.pixel_height()), (16, 12));
}

// C#: DecodeBoxDecoderTests.Decode_FromBytes_FitsBox
#[test]
#[ignore = "native: needs WIC"]
fn decode_from_bytes_fits_box() {
    let bytes = fs::read(fixture("gradient-64x48-bgra32.png")).expect("fixture");
    let view = physical_decoder()
        .decode(
            &DecodeRequest::with_box(fixture("gradient-64x48-bgra32.png"), DecodeBox::new(16, 16))
                .with_bytes(Arc::from(bytes)),
        )
        .expect("bytes decode");
    assert_eq!((view.pixel_width(), view.pixel_height()), (16, 12));
}

// C#: DecodeBoxDecoderTests.Decode_UnboundedBox_FullSize
#[test]
#[ignore = "native: needs WIC"]
fn decode_unbounded_box_full_size() {
    let view = decode_boxed("gradient-800x600.jpg", 0, 0);
    assert_eq!((view.pixel_width(), view.pixel_height()), (800, 600));
    assert!(!view.downscaled());
}

// ---------------------------------------------------------------------------------------------------------------
// C# Quality/DecoderQualityGateTests, Quality/WicColorProfileTests
// ---------------------------------------------------------------------------------------------------------------

// C#: DecoderQualityGateTests.FileHandlesReleasedImmediatelyAfterDecode (INV-8, production reader)
#[test]
#[ignore = "native: needs WIC"]
fn file_handles_released_immediately_after_decode() {
    let temp = TempRoot::new("wic-handles");
    let path = temp.join("plain.jpg");
    fs::copy(fixture("plain-64x48.jpg"), &path).expect("copy fixture");
    let view = physical_decoder()
        .decode(&DecodeRequest::new(path.clone(), 0))
        .expect("decodes");
    drop(view);
    fs::remove_file(&path).expect("the decode released its file handle (INV-8)");
}

// C#: DecoderQualityGateTests.MissingFilesThrowFileNotFoundException (decode and read_info)
#[test]
#[ignore = "native: needs WIC"]
fn missing_files_throw_file_not_found_exception() {
    check_decode(&decode_row("fail-missing-file"));
    check_read_info(&read_info_row("fail-missing-file"));
}

// C#: DecoderQualityGateTests.CorruptFilesFailSafelyWithoutCrash
#[test]
#[ignore = "native: needs WIC"]
fn corrupt_files_fail_safely_without_crash() {
    for name in [
        "fail-text",
        "fail-empty-file",
        "fail-header-garbage",
        "fail-truncated-30",
    ] {
        check_decode(&decode_row(name));
    }
}

// C#: DecoderQualityGateTests.EmbeddedIccProfilesHandledSafely (WicDirect rows)
#[test]
#[ignore = "native: needs WIC"]
fn embedded_icc_profiles_handled_safely() {
    for name in ["icc-p3-jpeg", "icc-p3-png-alpha", "icc-p3-jpeg-box"] {
        check_decode(&decode_row(name));
    }
}

// C#: WicColorProfileTests.DirectWicDecodesProfiledJpegForPathAndMemory
#[test]
#[ignore = "native: needs WIC"]
fn direct_wic_decodes_profiled_jpeg_for_path_and_memory() {
    check_decode(&decode_row("icc-p3-jpeg"));
    check_decode(&decode_row("icc-p3-jpeg-bytes"));
}

// C#: WicColorProfileTests.DirectWicTransformsDisplayP3PixelsToSrgbLikeColorManagedWpf
// Compared against the golden samples (recorded through the same pipeline on the oracle), not against WPF.
#[test]
#[ignore = "native: needs WIC"]
fn direct_wic_transforms_display_p3_pixels_to_srgb_like_color_managed_wpf() {
    check_decode(&decode_row("icc-p3-jpeg"));
}

// C#: WicColorProfileTests.ProfiledPngWithAlphaDecodesPremultipliedAndKeepsAlpha
#[test]
#[ignore = "native: needs WIC"]
fn profiled_png_with_alpha_decodes_premultiplied_and_keeps_alpha() {
    check_decode(&decode_row("icc-p3-png-alpha"));
}

// ---------------------------------------------------------------------------------------------------------------
// C# Robustness/*
// ---------------------------------------------------------------------------------------------------------------

// C#: DecoderFileEdgeCaseTests.NotAnImage_FailsCleanly
#[test]
#[ignore = "native: needs WIC"]
fn not_an_image_fails_cleanly() {
    check_decode(&decode_row("fail-text"));
}

// C#: DecoderFileEdgeCaseTests.MissingFile_KeepsItsExceptionType
#[test]
#[ignore = "native: needs WIC"]
fn missing_file_keeps_its_exception_type() {
    check_decode(&decode_row("fail-missing-file"));
}

// C#: DecoderFileEdgeCaseTests.ReadInfo_OnMutatedFiles_IsWellBehaved
#[test]
#[ignore = "native: needs WIC"]
fn read_info_on_mutated_files_is_well_behaved() {
    for name in ["fail-text", "fail-empty-file", "fail-header-garbage", "truncated-30"] {
        check_read_info(&read_info_row(name));
    }
}

// C#: InvalidOrientationValueTests.OutOfRangeOrientation_IsTreatedAsNormal
#[test]
#[ignore = "native: needs WIC"]
fn out_of_range_orientation_is_treated_as_normal() {
    check_decode(&decode_row("orient-9-apply"));
    check_read_info(&read_info_row("orient-9"));
}

// C#: IccMismatchTests.ProfileColourSpaceMismatch_StillDecodes
// Fixture substitution: the committed CMYK TIFF stands in for the profile whose colour space does not match the pixels.
#[test]
#[ignore = "native: needs WIC"]
fn profile_colour_space_mismatch_still_decodes() {
    check_decode(&decode_row("format-tiff-cmyk32"));
}

// C#: LargeHeaderReadInfoTests.ReadInfo_AgreesWithDecode_WhenExifIsBeyond64Kb (WicDirect row)
// Builds padded JPEGs from `orient-6-120x80.jpg` with 0, 1, 2, 3 and 20 leading 60 KB COM segments.
#[test]
#[ignore = "native: needs WIC"]
fn read_info_agrees_with_decode_when_exif_is_beyond_64_kb() {
    let original = fs::read(fixture("orient-6-120x80.jpg")).expect("fixture");
    let temp = TempRoot::new("wic-large-header");
    let decoder = physical_decoder();
    for segments in [0usize, 1, 2, 3, 20] {
        let mut padded = original[..2].to_vec();
        for _ in 0..segments {
            let payload = 60_000usize;
            padded.extend_from_slice(&[0xFF, 0xFE]);
            padded.extend_from_slice(&((payload + 2) as u16).to_be_bytes());
            padded.extend(vec![0x20u8; payload]);
        }
        padded.extend_from_slice(&original[2..]);
        let path = temp.join(format!("padded-{segments}.jpg"));
        fs::write(&path, &padded).expect("write padded JPEG");
        let info = decoder
            .read_info(&path.to_string_lossy())
            .unwrap_or_else(|e| panic!("{segments} segments: read_info {e}"));
        let view = decoder
            .decode(&DecodeRequest::new(path.clone(), 0))
            .unwrap_or_else(|e| panic!("{segments} segments: decode {e}"));
        assert_eq!(view.orientation(), 6, "{segments} segments: decode orientation");
        assert_eq!(info.orientation, view.orientation(), "{segments} segments: orientation");
        // C#: (decoded.OriginalWidth, OriginalHeight) == (info.Width, info.Height), both in display orientation.
        assert_eq!(
            (view.original_width(), view.original_height()),
            (info.width(), info.height()),
            "{segments} segments"
        );
        assert_eq!(
            (info.width(), info.height()),
            (80, 120),
            "{segments} segments: displayed size"
        );
        assert_eq!(
            (info.pixel_width, info.pixel_height),
            (120, 80),
            "{segments} segments: stored size"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------
// C# DecoderMutationFuzzTests (deterministic xorshift mutations of committed fixtures, no `rand`)
// ---------------------------------------------------------------------------------------------------------------

struct XorShift(u64);

impl XorShift {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

fn mutate(bytes: &[u8], rng: &mut XorShift, edits: usize, window: std::ops::Range<usize>) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let window = window.start.min(out.len())..window.end.min(out.len());
    if window.is_empty() {
        return out;
    }
    for _ in 0..edits {
        let i = window.start + rng.below(window.len());
        out[i] = rng.next_u64() as u8;
    }
    out
}

/// The failures a damaged file may produce: anything else is a bug in the decoder.
fn assert_understood(name: &str, result: &Result<Arc<dyn DecodedImageView>, DecodeError>) {
    if let Err(e) = result {
        let understood = matches!(
            e.kind(),
            DecodeError::Com { .. }
                | DecodeError::InvalidData { .. }
                | DecodeError::NotSupported { .. }
                | DecodeError::InvalidArgument { .. }
                | DecodeError::MemoryAdmission { .. }
                | DecodeError::InvalidCast { .. }
                | DecodeError::Overflow { .. }
                | DecodeError::Format { .. }
        );
        assert!(understood, "{name}: unexpected failure {e}");
    }
}

fn run_fuzz(seed: u64, iterations: usize, files: &[&str]) {
    let decoder = physical_decoder();
    let mut rng = XorShift(seed);
    for file in files {
        let original = fs::read(fixture(file)).expect("fixture");
        for _ in 0..iterations {
            let edits = 1 + rng.below(8);
            let damaged = mutate(&original, &mut rng, edits, 0..original.len());
            let result = decode_bytes(&decoder, &fixture(file), damaged);
            assert_understood(file, &result);
        }
    }
}

// C#: DecoderMutationFuzzTests.MutatedImages_FailOnlyWithUnderstoodExceptions
#[test]
#[ignore = "native: needs WIC"]
fn mutated_images_fail_only_with_understood_exceptions() {
    run_fuzz(
        0x9E37_79B9_7F4A_7C15,
        200,
        &["plain-64x48.jpg", "orient-6-120x80.jpg", "jpeg-make-32x24.jpg"],
    );
}

// C#: DecoderMutationFuzzTests.CorruptExifMetadata_NeverBreaksTheDecoders
#[test]
#[ignore = "native: needs WIC"]
fn corrupt_exif_metadata_never_breaks_the_decoders() {
    let original = fs::read(fixture("jpeg-make-32x24.jpg")).expect("fixture");
    let exif_at = original
        .windows(6)
        .position(|w| w == b"Exif\0\0")
        .expect("the fixture carries an Exif segment");
    let decoder = physical_decoder();
    let mut rng = XorShift(0xC0FF_EE00_1234_5678);
    for _ in 0..200 {
        let edits = 1 + rng.below(4);
        let damaged = mutate(&original, &mut rng, edits, exif_at + 6..exif_at + 200);
        let result = decode_bytes(&decoder, &fixture("jpeg-make-32x24.jpg"), damaged);
        assert_understood("exif-corrupt", &result);
    }
}

// C#: DecoderMutationFuzzTests.DamagedExifTiffHeader_StillDecodes
#[test]
#[ignore = "native: needs WIC"]
fn damaged_exif_tiff_header_still_decodes() {
    let mut damaged = fs::read(fixture("jpeg-make-32x24.jpg")).expect("fixture");
    let exif_at = damaged
        .windows(6)
        .position(|w| w == b"Exif\0\0")
        .expect("the fixture carries an Exif segment");
    // TIFF header: byte order (2), magic 42 (2), offset of the first IFD (4). Point the IFD far outside the segment.
    let tiff = exif_at + 6;
    damaged[tiff + 4..tiff + 8].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
    let view =
        decode_bytes(&physical_decoder(), &fixture("jpeg-make-32x24.jpg"), damaged).expect("the pixels still decode");
    assert_eq!((view.pixel_width(), view.pixel_height()), (32, 24));
}

// C#: DecoderMutationFuzzTests.OrientationTagWithTwoValues_IsReadConsistently
#[test]
#[ignore = "native: needs WIC"]
fn orientation_tag_with_two_values_is_read_consistently() {
    let decoder = physical_decoder();
    let path = fixture("orient-6-120x80.jpg");
    let info = decoder.read_info(&path.to_string_lossy()).expect("read_info");
    let view = decoder.decode(&DecodeRequest::new(path, 0)).expect("decode");
    assert_eq!(
        info.orientation,
        view.orientation(),
        "read_info and decode report the same orientation"
    );
}

// C#: DecoderMutationFuzzTests.MutatedImages_DeepRun (3000 iterations per seed; PHOTOREVIEW_FUZZ_SEED overrides the seed)
#[test]
#[ignore = "slow: deep fuzz"]
fn mutated_images_deep_run() {
    let seed = std::env::var("PHOTOREVIEW_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0x5151_5151);
    run_fuzz(
        seed,
        3000,
        &["plain-64x48.jpg", "orient-6-120x80.jpg", "jpeg-make-32x24.jpg"],
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Repair round 1: disk reads (AGENTS priority 1, read each file once)
// ---------------------------------------------------------------------------------------------------------------

/// `File` that counts the bytes it reads from disk.
struct CountingFile(File, Arc<std::sync::atomic::AtomicU64>);

impl Read for CountingFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.0.read(buf)?;
        self.1.fetch_add(read as u64, std::sync::atomic::Ordering::SeqCst);
        Ok(read)
    }
}

impl Seek for CountingFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}

/// The `PhysicalSourceReader` shape (a `BufReader` of the requested size over the file) with a disk-byte counter.
#[derive(Default)]
struct CountingReader(Arc<std::sync::atomic::AtomicU64>);

impl SourceReader for CountingReader {
    fn open_source(
        &self,
        path: &Path,
        _priority: SourceReadPriority,
        buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        Ok(Box::new(io::BufReader::with_capacity(
            buffer_size.max(1),
            CountingFile(File::open(path)?, Arc::clone(&self.0)),
        )))
    }
}

// Verifier finding: the .NET oracle (GetProcessIoCounters ReadTransferCount at d92d6aae, PhysicalSourceReader) reads
// exactly the file length on Decode and on ReadInfo for these fixtures (45921, 4174, 468 and 828 bytes).
#[test]
#[ignore = "native: needs WIC"]
fn decode_and_read_info_read_each_source_byte_from_disk_once() {
    for name in [
        "gradient-800x600.jpg",
        "orient-6-120x80.jpg",
        "gradient-64x48-bgra32.png",
        "tiff-make-32x24.tif",
    ] {
        let path = fixture(name);
        let length = fs::metadata(&path).expect("fixture").len();
        let reader = Arc::new(CountingReader::default());
        let decoder = WicDirectDecoder::new(
            PixelBufferImageCodec::shared(),
            Some(Arc::clone(&reader) as Arc<dyn SourceReader>),
            memory(),
        );
        decoder.decode(&DecodeRequest::new(path.clone(), 0)).expect("decodes");
        let decoded = reader.0.swap(0, std::sync::atomic::Ordering::SeqCst);
        decoder.read_info(&path.to_string_lossy()).expect("read_info");
        let read_info = reader.0.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(decoded, length, "{name}: decode disk bytes");
        assert_eq!(read_info, length, "{name}: read_info disk bytes");
    }
}
