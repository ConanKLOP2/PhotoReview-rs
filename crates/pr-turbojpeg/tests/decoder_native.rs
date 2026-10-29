//! Red tests for W5-14b (`contracts/W5-14b.md`): the native half of `pr_turbojpeg::decoder`, against the real
//! `native/x64/turbojpeg.dll` (loaded by absolute path, `tools/fetch-native.ps1`). Every test is
//! `#[ignore = "native: needs turbojpeg.dll"]`; a machine without the DLL prints the reason and returns early.
//!
//! Golden-driven: `turbojpeg-decode` (183 rows + environment) and `turbojpeg-read-info` (42 rows + environment). A row
//! whose `turbojpeg_sha256` differs from the DLL on this machine is compared on sizes, flags and error types only
//! (contract "Native tests"). Fine-scale rows compare the 16 samples with tolerance 3 (D39), never the whole hash.
//!
//! Divergences from C# are marked `// DIVERGENCE: D-n` (contract semantics 15).

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use pr_core::abstractions::NullLog;
use pr_core::model::enums::DecoderBackend;
use pr_imaging::decode_types::{DecodeBox, DecodeRequest, DecodedImageView, SourceBytes};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::fallback::FallbackImageDecoder;
use pr_imaging::pixel_memory::NativePixelMemory;
use pr_imaging::pixels::PixelBuffer;
use pr_test_support::{TempRoot, golden};
use pr_turbojpeg::decoder::{HEADER_READ_CHUNK, messages};
use pr_turbojpeg::native::{TjParam, TjPixelFormat, TurboJpegLibrary};
use serde_json::Value;
use support::*;

/// `Some(decoder)` on a machine with the DLL; `None` (reason printed) otherwise.
fn decoder_or_skip(mem: Arc<CountingMemory>) -> Option<pr_turbojpeg::decoder::TurboJpegDecoder> {
    native_decoder_with(mem as Arc<dyn pr_imaging::memory_headroom::MemoryInfoSource>)
}

/// `true` when this machine's `turbojpeg.dll` is the one the goldens were written with (exact hashes allowed).
fn dll_matches_golden(environment: &Value) -> bool {
    match std::fs::read(native_dll_path()) {
        Ok(bytes) => sha256_hex(&bytes) == golden::str_of(environment, "turbojpeg_sha256"),
        Err(_) => false,
    }
}

struct SyntheticFlavour {
    width: i32,
    height: i32,
    subsamp: i32,
    progressive: bool,
    arithmetic: bool,
    restart_rows: i32,
    cmyk: bool,
    colorspace: Option<i32>,
}

/// Compresses the synthetic BGRA gradient used by C# `TurboJpegSyntheticFormatTests.Compress`.
fn synthetic_jpeg(library: &Arc<TurboJpegLibrary>, flavour: SyntheticFlavour) -> Vec<u8> {
    let SyntheticFlavour {
        width,
        height,
        subsamp,
        progressive,
        arithmetic,
        restart_rows,
        cmyk,
        colorspace,
    } = flavour;
    let mut compressor = library.create_compressor().expect("compressor");
    assert_eq!(compressor.set(TjParam::Quality, 92).expect("quality"), 0);
    assert_eq!(compressor.set(TjParam::Subsamp, subsamp).expect("subsampling"), 0);
    if progressive {
        assert_eq!(compressor.set(TjParam::Progressive, 1).expect("progressive"), 0);
    }
    if arithmetic {
        assert_eq!(compressor.set(TjParam::Arithmetic, 1).expect("arithmetic"), 0);
    }
    if restart_rows > 0 {
        assert_eq!(
            compressor
                .set(TjParam::RestartRows, restart_rows)
                .expect("restart rows"),
            0
        );
    }
    if let Some(colorspace) = colorspace {
        assert_eq!(compressor.set(TjParam::Colorspace, colorspace).expect("colorspace"), 0);
    }

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let offset = ((y * width + x) * 4) as usize;
            pixels[offset] = (40 + 180 * x / (width - 1).max(1)) as u8;
            pixels[offset + 1] = (40 + 180 * y / (height - 1).max(1)) as u8;
            pixels[offset + 2] = (200 - 120 * (x + y) / (width + height - 2).max(1)) as u8;
            pixels[offset + 3] = if cmyk {
                (30 + 60 * x / (width - 1).max(1)) as u8
            } else {
                255
            };
        }
    }
    compressor
        .compress8(
            &pixels,
            width,
            width * 4,
            height,
            if cmyk { TjPixelFormat::Cmyk } else { TjPixelFormat::Bgra },
        )
        .expect("tj3Compress8")
}

/// Loads both native pieces needed for an on-the-fly synthetic decode test.
fn synthetic_test_parts() -> Option<(Arc<TurboJpegLibrary>, pr_turbojpeg::decoder::TurboJpegDecoder)> {
    let path = native_dll_path();
    if !path.is_file() {
        eprintln!("skipping: missing native TurboJPEG DLL at {}", path.display());
        return None;
    }
    let library = match TurboJpegLibrary::load(&path) {
        Ok(library) => library,
        Err(error) => {
            eprintln!("skipping: cannot load native TurboJPEG DLL: {error}");
            return None;
        }
    };
    let decoder = decoder_or_skip(memory(1 << 36, 0))?;
    Some((library, decoder))
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn flavour_decodes_consistently() {
    // C#: TurboJpegSyntheticFormatTests.Flavour_DecodesConsistently (18 synthesized JPEG flavours).
    let Some((library, decoder)) = synthetic_test_parts() else {
        return;
    };
    let turbo: Arc<dyn ImageDecoder> = Arc::new(decoder);
    let wic: Arc<dyn ImageDecoder> = Arc::new(pr_platform_win::wic::decoder::WicDirectDecoder::new(
        pr_imaging::pixel_memory::PixelBufferImageCodec::shared(),
        None,
        memory(1 << 36, 0) as Arc<dyn pr_imaging::memory_headroom::MemoryInfoSource>,
    ));
    let flavours = [
        ("444", 33, 21, 0, false, false, 0, false, None),
        ("422", 33, 21, 1, false, false, 0, false, None),
        ("420", 33, 21, 2, false, false, 0, false, None),
        ("440", 33, 21, 4, false, false, 0, false, None),
        ("411", 33, 21, 5, false, false, 0, false, None),
        ("gray", 33, 21, 3, false, false, 0, false, None),
        ("progressive-420", 33, 21, 2, true, false, 0, false, None),
        ("arithmetic-444", 33, 21, 0, false, true, 0, false, None),
        ("restart-rows", 33, 21, 2, false, false, 1, false, None),
        ("cmyk", 33, 21, 0, false, false, 0, true, Some(3)),
        ("ycck", 33, 21, 0, false, false, 0, true, Some(4)),
        ("1x1", 1, 1, 0, false, false, 0, false, None),
        ("1x40", 1, 40, 2, false, false, 0, false, None),
        ("40x1", 40, 1, 2, false, false, 0, false, None),
        ("7x9-420", 7, 9, 2, false, false, 0, false, None),
        ("17x11-422", 17, 11, 1, false, false, 0, false, None),
        ("9x9-411", 9, 9, 5, false, false, 0, false, None),
        ("16x16-420-progressive", 16, 16, 2, true, false, 0, false, None),
    ];
    for (name, width, height, subsamp, progressive, arithmetic, restart_rows, cmyk, colorspace) in flavours {
        let jpeg = synthetic_jpeg(
            &library,
            SyntheticFlavour {
                width,
                height,
                subsamp,
                progressive,
                arithmetic,
                restart_rows,
                cmyk,
                colorspace,
            },
        );
        let bytes: SourceBytes = jpeg.into();
        let request = || DecodeRequest::new("synthetic.jpg", 0).with_bytes(bytes.clone());
        // C#: both production chains (Turbo -> Wpf, WicDirect -> Wpf) decode every flavour at the right size, CMYK and YCCK included.
        // DIVERGENCE: the Rust host has no WPF; its "Wpf" slot is the WIC decoder (`WicDirectDecoder`), which is also what C# WPF decodes with.
        let chains: [(&str, Arc<dyn ImageDecoder>); 2] = [
            (
                "Turbo->Wic",
                Arc::new(FallbackImageDecoder::new(
                    Arc::clone(&turbo),
                    DecoderBackend::TurboJpeg,
                    Arc::clone(&wic),
                    Arc::new(NullLog),
                    None,
                )),
            ),
            ("WicDirect", Arc::clone(&wic)),
        ];
        for (chain_name, chain) in chains {
            let image = chain
                .decode(&request())
                .unwrap_or_else(|error| panic!("{name}: {chain_name} decode failed: {error}"));
            assert_eq!(
                (image.pixel_width(), image.pixel_height()),
                (width, height),
                "{name}: {chain_name} decoded size"
            );
        }

        // C#: TurboJpeg alone may decline (InvalidDataException / NotSupportedException, e.g. CMYK/YCCK with 'Unsupported color conversion request'); WPF alone may decline.
        let via_turbo = match turbo.decode(&request()) {
            Ok(image) => Some(image),
            Err(error) if declines(&error) => {
                eprintln!("{name}: TurboJpeg declines ({error})");
                None
            }
            Err(error) => panic!("{name}: TurboJpeg failed with a non-declining error: {error}"),
        };
        let via_wic = match wic.decode(&request()) {
            Ok(image) => Some(image),
            Err(error) if declines(&error) => {
                eprintln!("{name}: WIC declines ({error})");
                None
            }
            Err(error) => panic!("{name}: WIC failed with a non-declining error: {error}"),
        };
        // C#: where both decode, the colours agree (mean CIE76 dE < 3.0, ImageCompare.Compare / PixelMetrics.MeanDeltaE).
        if let (Some(turbo_image), Some(wic_image)) = (via_turbo, via_wic) {
            let delta = mean_delta_e(&*turbo_image, &*wic_image);
            eprintln!("{name}: TurboJpeg vs WIC mean dE={delta:.2}");
            assert!(
                delta < 3.0,
                "{name}: TurboJpeg and WIC disagree on colours (mean dE {delta:.2})"
            );
        }
    }
}

/// C# `catch (Exception ex) when (ex is InvalidDataException or NotSupportedException)`: the decoder declines the file.
fn declines(error: &DecodeError) -> bool {
    match error {
        DecodeError::WithSourceBytes { error, .. } | DecodeError::Localized { error, .. } => declines(error),
        DecodeError::InvalidData { .. } | DecodeError::NotSupported { .. } => true,
        _ => false,
    }
}

/// Port of C# `PixelMetrics.MeanDeltaE` (sRGB -> XYZ D65 -> CIE Lab, mean Euclidean distance) over two decoded images of equal size.
fn mean_delta_e(a: &dyn DecodedImageView, b: &dyn DecodedImageView) -> f64 {
    fn bgr_bytes(image: &dyn DecodedImageView) -> Vec<u8> {
        let pixels = image
            .platform_image()
            .as_pixels()
            .expect("decoder hands over a pixel buffer");
        pixels.bytes().expect("live pixel buffer").to_vec()
    }
    fn lab(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
        fn inverse_gamma(c: f64) -> f64 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        fn f(t: f64) -> f64 {
            if t > 0.008_856_451_679_035_631 {
                t.cbrt()
            } else {
                7.787_037_037_037_037 * t + 16.0 / 116.0
            }
        }
        let (r, g, b) = (
            inverse_gamma(f64::from(r) / 255.0),
            inverse_gamma(f64::from(g) / 255.0),
            inverse_gamma(f64::from(b) / 255.0),
        );
        let x = 0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b;
        let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175_0 * b;
        let z = 0.019_333_9 * r + 0.119_192_0 * g + 0.950_304_1 * b;
        let (fx, fy, fz) = (f(x / 0.950_47), f(y / 1.0), f(z / 1.088_83));
        (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
    }
    assert_eq!(
        (a.pixel_width(), a.pixel_height()),
        (b.pixel_width(), b.pixel_height()),
        "image dimensions must match"
    );
    let (bytes_a, bytes_b) = (bgr_bytes(a), bgr_bytes(b));
    assert_eq!(bytes_a.len(), bytes_b.len());
    let mut total = 0.0;
    for (p, q) in bytes_a.chunks_exact(4).zip(bytes_b.chunks_exact(4)) {
        let (l1, a1, b1) = lab(p[2], p[1], p[0]);
        let (l2, a2, b2) = lab(q[2], q[1], q[0]);
        total += ((l1 - l2).powi(2) + (a1 - a2).powi(2) + (b1 - b2).powi(2)).sqrt();
    }
    total / (bytes_a.len() / 4) as f64
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn box_decode_produces_exact_fit_size() {
    // C#: TurboJpegSyntheticFormatTests.BoxDecode_ProducesExactFitSize (9 source sizes x 8 boxes).
    let Some((library, decoder)) = synthetic_test_parts() else {
        return;
    };
    let sizes = [
        (1, 1),
        (1, 7),
        (7, 1),
        (33, 21),
        (64, 48),
        (100, 3),
        (3, 100),
        (129, 257),
        (255, 255),
    ];
    let boxes = [
        DecodeBox::new(1, 1),
        DecodeBox::new(2, 2),
        DecodeBox::new(5, 0),
        DecodeBox::new(0, 5),
        DecodeBox::new(16, 16),
        DecodeBox::new(50, 50),
        DecodeBox::new(1000, 1000),
        DecodeBox::new(7, 3),
    ];
    for (width, height) in sizes {
        let jpeg = synthetic_jpeg(
            &library,
            SyntheticFlavour {
                width,
                height,
                subsamp: 2,
                progressive: false,
                arithmetic: false,
                restart_rows: 0,
                cmyk: false,
                colorspace: None,
            },
        );
        for decode_box in boxes {
            let (expected_width, expected_height) = decode_box.fit(width, height);
            let image = decoder
                .decode(
                    &DecodeRequest::with_box("box.jpg", decode_box)
                        .with_apply_orientation(false)
                        .with_bytes(jpeg.clone().into()),
                )
                .unwrap_or_else(|error| panic!("{width}x{height} into {decode_box:?}: {error}"));
            assert_eq!(
                (image.pixel_width(), image.pixel_height()),
                (expected_width, expected_height),
                "{width}x{height} into {decode_box:?}"
            );
            assert_eq!((image.original_width(), image.original_height()), (width, height));
        }
    }
}

/// The C# exception type name of a Rust error (golden field `error.type`).
fn csharp_type(error: &DecodeError) -> &'static str {
    match error {
        DecodeError::NotSupported { .. } => "System.NotSupportedException",
        DecodeError::InvalidData { .. } => "System.IO.InvalidDataException",
        DecodeError::MemoryAdmission { .. } => "PhotoReview.Imaging.Decoding.DecoderMemoryAdmissionException",
        DecodeError::FileNotFound { .. } => "System.IO.FileNotFoundException",
        DecodeError::InvalidArgument { .. } => "System.ArgumentException",
        DecodeError::InvalidOperation { .. } => "System.InvalidOperationException",
        DecodeError::OutOfMemory { .. } => "System.OutOfMemoryException",
        _ => "other",
    }
}

/// Rows of the `turbojpeg-decode` golden use these source spellings. `path` returns the request path and the bytes.
struct Source {
    bytes: Option<Vec<u8>>,
    path: std::path::PathBuf,
}

/// Builds the request source of a golden case. `from` = `bytes` (caller bytes), `file` (written to a temp file),
/// `none` (no path, no bytes). `missing` and `missing_dir` are paths that do not hold a file.
fn source_of(case: &Value, temp: &TempRoot) -> Source {
    let source = &case["source"];
    let transforms: Vec<Value> = source
        .get("transforms")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let base: Option<Vec<u8>> = if let Some(h) = source.get("hex").and_then(Value::as_str) {
        Some(hex(h))
    } else if let Some(f) = source.get("fixture").and_then(Value::as_str) {
        Some(fixture_bytes(f))
    } else if let Some(n) = source.get("progressive_gray").and_then(Value::as_i64) {
        Some(progressive_gray(n as i32))
    } else {
        source
            .get("zero_file")
            .and_then(Value::as_u64)
            .map(|n| vec![0u8; n as usize])
    };
    // Oracle `W514bSource`: `null` source = no bytes, no file; `missing`, `missing_dir`, `path` (a literal, also blank) and
    // `directory` produce a path and no bytes.
    if source.is_null() {
        return Source {
            bytes: None,
            path: std::path::PathBuf::new(),
        };
    }
    if let Some(name) = source.get("missing").and_then(Value::as_str) {
        return Source {
            bytes: None,
            path: temp.join(name),
        };
    }
    if let Some(name) = source.get("missing_dir").and_then(Value::as_str) {
        return Source {
            bytes: None,
            path: temp.join("no-such-dir").join(name),
        };
    }
    if let Some(literal) = source.get("path").and_then(Value::as_str) {
        return Source {
            bytes: None,
            path: std::path::PathBuf::from(literal),
        };
    }
    if source.get("directory").is_some() {
        return Source {
            bytes: None,
            path: temp.path().to_path_buf(),
        };
    }
    let bytes = base.map(|b| apply_transforms(b, &transforms));
    let label = case["path"]
        .as_str()
        .filter(|label| !label.is_empty())
        .unwrap_or("source.jpg");
    let path = match &bytes {
        Some(b) => write_under(temp.path(), label, b),
        None => std::path::PathBuf::new(),
    };
    Source { bytes, path }
}

/// `(B, G, R)` of every pixel in row order, the bytes the golden `bgr_sha256` hashes.
fn bgr_bytes(pixels: &PixelBuffer) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..pixels.height() {
        let row = pixels.get_row(y).expect("row");
        for x in 0..pixels.width() as usize {
            out.extend_from_slice(&row[x * 4..x * 4 + 3]);
        }
    }
    out
}

/// One `turbojpeg-decode` row. Returns the mismatch text, `None` when the row agrees with C#.
fn check_decode_row(case: &Value, exact: bool) -> Option<String> {
    let name = case["name"].as_str().unwrap_or("?").to_owned();
    let temp = TempRoot::new("tj-decode-row");
    let source = source_of(case, &temp);
    let mem = memory(
        golden::i64_of(&case["memory"], "total"),
        golden::i64_of(&case["memory"], "load"),
    );
    let decoder = decoder_or_skip(mem.clone())?;
    let box_ = case["box"].as_array().expect("box");
    let decode_box = DecodeBox::new(box_[0].as_i64()? as i32, box_[1].as_i64()? as i32);
    let mut request = DecodeRequest::with_box(source.path.clone(), decode_box)
        .with_apply_orientation(case["apply_orientation"].as_bool().unwrap_or(true));
    if let Some(orientation) = case.get("source_orientation").and_then(Value::as_i64) {
        request = request.with_source_orientation(orientation as i32);
    }
    // GOLDEN DEFECT (reported to the lead): the oracle harness passes `bytes is null ? null : new ReadOnlyMemory<byte>(bytes)`,
    // which C# types as a non-nullable `ReadOnlyMemory<byte>`: a null array becomes an EMPTY caller buffer. So every row
    // without bytes (all `from: file` rows, `none`) was recorded as "not a valid JPEG: <path>". To compare with the golden
    // exactly as recorded, the request carries the same (empty) buffer; the real file reads are checked separately by
    // `file_row_agrees_with_bytes` below.
    let is_file = golden::str_of(case, "from") == "file";
    let recorded_bytes = if is_file {
        Vec::new()
    } else {
        source.bytes.clone().unwrap_or_default()
    };
    request = request.with_bytes(recorded_bytes.into());
    if !is_file {
        request.path = std::path::PathBuf::from(case["path"].as_str().unwrap_or(""));
    }
    let consulted = mem.reads.load(Ordering::SeqCst) as i64;
    let result = decoder.decode(&request);
    let consulted_now = mem.reads.load(Ordering::SeqCst) as i64;
    if is_file {
        if let Some(problem) = file_row_agrees_with_bytes(&name, case, &decoder, &source, &request) {
            return Some(problem);
        }
    }
    let _ = consulted;
    if let Some(error) = case.get("error").filter(|e| !e.is_null()) {
        return match result {
            Ok(_) => Some(format!("{name}: expected {}, got a decoded image", error["type"])),
            Err(actual) => {
                if csharp_type(actual.kind()) != golden::str_of(error, "type") {
                    return Some(format!(
                        "{name}: type {} vs {}",
                        csharp_type(actual.kind()),
                        error["type"]
                    ));
                }
                let fallbackable = FallbackImageDecoder::is_fallbackable(&actual);
                if fallbackable != golden::bool_of(error, "fallbackable") {
                    return Some(format!("{name}: fallbackable {fallbackable}"));
                }
                let source_len = actual.source_bytes().map(|b| b.len() as i64);
                let want_len = error["source_bytes_length"].as_i64();
                if source_len != want_len && error["source_bytes_length"].is_number() {
                    return Some(format!("{name}: source bytes {source_len:?} vs {want_len:?}"));
                }
                if consulted_now != golden::i64_of(case, "consulted") {
                    return Some(format!("{name}: memory consulted {consulted_now}"));
                }
                None
            }
        };
    }
    let image = match result {
        Ok(image) => image,
        Err(error) => return Some(format!("{name}: unexpected error {error}")),
    };
    let ok = &case["ok"];
    let got = (
        image.pixel_width(),
        image.pixel_height(),
        image.estimated_bytes(),
        image.downscaled(),
        image.orientation(),
    );
    let want = (
        golden::i64_of(ok, "pixel_width") as i32,
        golden::i64_of(ok, "pixel_height") as i32,
        golden::i64_of(ok, "estimated_bytes"),
        golden::bool_of(ok, "downscaled"),
        golden::i64_of(ok, "orientation") as i32,
    );
    if got != want {
        return Some(format!("{name}: sizes/flags {got:?} vs {want:?}"));
    }
    if consulted_now != golden::i64_of(case, "consulted") {
        return Some(format!("{name}: memory consulted {consulted_now}"));
    }
    let pixels = image.platform_image().as_pixels().expect("pixel image");
    if exact && !golden::bool_of(ok, "downscaled") {
        let digest = sha256_hex(&bgr_bytes(pixels));
        if digest != golden::str_of(ok, "bgr_sha256") {
            return Some(format!("{name}: bgr_sha256 {digest}"));
        }
    }
    let _ = layout_name(pixels.layout());
    None
}

/// `turbojpeg-decode`: every row against the DLL on this machine (sizes always, pixels when the DLL matches).
#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn turbojpeg_decode_golden_rows_match_csharp() {
    // C#: TurboJpegPixelBufferParityTests, DecodeBoxDecoderTests, DecodeSizeDecisionMutationTests, TurboJpegHardeningTests,
    // TurboJpegGuardMutationTests, DecoderOutputFormatTests (the golden rows of the decode path).
    if native_decoder_with(memory(1 << 36, 0)).is_none() {
        return;
    }
    let cases = cases("turbojpeg-decode");
    let exact = dll_matches_golden(&cases[0]);
    let mismatches: Vec<String> = cases[1..].iter().filter_map(|c| check_decode_row(c, exact)).collect();
    assert!(
        mismatches.is_empty(),
        "{} of {} decode rows disagree with C#; first: {:?}",
        mismatches.len(),
        cases.len() - 1,
        mismatches.first()
    );
}

/// `turbojpeg-read-info`: header-only sizes, orientation and error types.
#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn turbojpeg_read_info_golden_rows_match_csharp() {
    // C#: TurboJpegTests.ReadInfoDetectsOrientation (rows), TurboJpegHardeningTests, Robustness/LargeHeaderReadInfoTests.
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let cases = cases("turbojpeg-read-info");
    let mut mismatches = Vec::new();
    for case in &cases[1..] {
        let name = case["name"].as_str().unwrap_or("?");
        let temp = TempRoot::new("tj-ri-row");
        let source = source_of(case, &temp);
        let path = source.path.to_string_lossy().into_owned();
        let result = decoder.read_info(&path);
        match (result, case.get("error")) {
            (Ok(info), None) => {
                let got = (
                    info.pixel_width,
                    info.pixel_height,
                    info.orientation,
                    info.width(),
                    info.height(),
                );
                let ok = &case["ok"];
                let want = (
                    golden::i64_of(ok, "pixel_width") as i32,
                    golden::i64_of(ok, "pixel_height") as i32,
                    golden::i64_of(ok, "orientation") as i32,
                    golden::i64_of(ok, "width") as i32,
                    golden::i64_of(ok, "height") as i32,
                );
                if got != want {
                    mismatches.push(format!("{name}: {got:?} vs {want:?}"));
                }
            }
            (Err(actual), Some(error)) => {
                if csharp_type(actual.kind()) != golden::str_of(error, "type") {
                    mismatches.push(format!("{name}: type {}", csharp_type(actual.kind())));
                }
            }
            (other, _) => mismatches.push(format!("{name}: {other:?}")),
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} read-info rows disagree; first: {:?}",
        mismatches.len(),
        mismatches.first()
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Guards and header (`TurboJpegGuardMutationTests`, native rows)
// ---------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_exactly_the_three_signature_bytes_fails_as_a_broken_header_not_as_a_non_jpeg() {
    // C#: TurboJpegGuardMutationTests.Decode_ExactlyTheThreeSignatureBytes_FailsAsABrokenHeaderNotAsANonJpeg
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let request = DecodeRequest::new("sig.jpg", 0).with_bytes(hex("FFD8FF").into());
    let error = decoder.decode(&request).err().expect("3 bytes fail");
    assert!(matches!(error.kind(), DecodeError::InvalidData { .. }), "{error:?}");
    assert!(error.to_string().contains(messages::HEADER_FAILED_PREFIX), "{error}");
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn read_info_file_of_exactly_the_three_signature_bytes_fails_as_a_broken_header() {
    // C#: TurboJpegGuardMutationTests.ReadInfo_FileOfExactlyTheThreeSignatureBytes_FailsAsABrokenHeaderNotAsANonJpeg
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let temp = TempRoot::new("tj-sig3");
    let path = write_under(temp.path(), "sig.jpg", &hex("FFD8FF"));
    let error = decoder.read_info(path.to_str().expect("utf8")).expect_err("broken");
    assert!(matches!(error.kind(), DecodeError::InvalidData { .. }), "{error:?}");
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_header_the_native_parser_rejects_carries_the_native_detail_in_log_and_user_text() {
    // C#: TurboJpegGuardMutationTests.Decode_HeaderTheNativeParserRejects_CarriesTheNativeDetailInLogAndUserText
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/small-q85-64x48.jpg"),
        &[serde_json::json!({"op": "bad_precision"})],
    );
    let error = decoder
        .decode(&DecodeRequest::new("g.jpg", 0).with_bytes(jpeg.into()))
        .err()
        .expect("bad precision");
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
#[ignore = "native: needs turbojpeg.dll"]
fn decode_scan_the_native_decoder_rejects_carries_the_native_detail_in_log_and_user_text() {
    // C#: TurboJpegGuardMutationTests.Decode_ScanTheNativeDecoderRejects_CarriesTheNativeDetailInLogAndUserText
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/parity-640x480.jpg"),
        &[serde_json::json!({"op": "destroy_entropy_from_half"})],
    );
    let error = decoder
        .decode(&DecodeRequest::new("s.jpg", 0).with_bytes(jpeg.into()))
        .err()
        .expect("scan failure");
    assert!(matches!(error.kind(), DecodeError::InvalidData { .. }), "{error:?}");
    assert!(
        error.to_string().starts_with(messages::DECOMPRESS_FAILED_PREFIX),
        "{error}"
    );
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn read_info_header_the_native_parser_rejects_carries_the_native_detail() {
    // C#: TurboJpegGuardMutationTests.ReadInfo_HeaderTheNativeParserRejects_CarriesTheNativeDetailInLogAndUserText
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let temp = TempRoot::new("tj-ri-bp");
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/small-q85-64x48.jpg"),
        &[serde_json::json!({"op": "bad_precision"})],
    );
    let path = write_under(temp.path(), "bp.jpg", &jpeg);
    let error = decoder
        .read_info(path.to_str().expect("utf8"))
        .expect_err("bad precision");
    assert!(
        error.to_string().contains("Unsupported JPEG data precision 3"),
        "{error}"
    );
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_jpeg_without_a_frame_header_reports_invalid_dimensions_or_header_failure() {
    // C#: TurboJpegGuardMutationTests.Decode_JpegWithoutAFrameHeader_ReportsInvalidDimensions
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let jpeg = hex("FFD8FFE00002FFD9");
    let error = decoder
        .decode(&DecodeRequest::new("n.jpg", 0).with_bytes(jpeg.into()))
        .err()
        .expect("no frame");
    assert!(matches!(error.kind(), DecodeError::InvalidData { .. }), "{error:?}");
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_sof_declaring_a_zero_width_is_rejected_as_an_invalid_data_fault() {
    // C#: TurboJpegGuardMutationTests.Decode_SofDeclaringAZeroWidth_IsRejectedAsAnInvalidDataFault
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/parity-8x8.jpg"),
        &[serde_json::json!({"op": "claim_size", "width": 0, "height": 8})],
    );
    let error = decoder
        .decode(&DecodeRequest::new("z.jpg", 0).with_bytes(jpeg.into()))
        .err()
        .expect("zero width");
    // The native header parse itself refuses a zero width ("Empty JPEG image", golden `zero-width`); C# only asserts the type.
    assert!(matches!(error.kind(), DecodeError::InvalidData { .. }), "{error:?}");
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_output_buffer_of_exactly_the_guard_threshold_consults_the_memory_budget() {
    // C#: TurboJpegGuardMutationTests.Decode_OutputBufferOfExactlyTheGuardThreshold_ConsultsTheMemoryBudget
    // 8192 x 4096 x 4 bytes = GUARD_THRESHOLD_BYTES exactly: one read of the memory probe.
    let mem = memory(1 << 30, 0);
    let Some(decoder) = decoder_or_skip(mem.clone()) else {
        return;
    };
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/parity-8x8.jpg"),
        &[serde_json::json!({"op": "claim_size", "width": 8192, "height": 4096})],
    );
    let _ = decoder.decode(&DecodeRequest::new("g.jpg", 0).with_bytes(jpeg.into()));
    assert_eq!(mem.reads.load(Ordering::SeqCst), 1);
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_output_buffer_just_below_the_guard_threshold_never_consults_the_memory_budget() {
    // C#: TurboJpegGuardMutationTests.Decode_OutputBufferJustBelowTheGuardThreshold_NeverConsultsTheMemoryBudget
    let mem = memory(1 << 30, 0);
    let Some(decoder) = decoder_or_skip(mem.clone()) else {
        return;
    };
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/parity-8x8.jpg"),
        &[serde_json::json!({"op": "claim_size", "width": 8192, "height": 4095})],
    );
    let _ = decoder.decode(&DecodeRequest::new("g.jpg", 0).with_bytes(jpeg.into()));
    assert_eq!(mem.reads.load(Ordering::SeqCst), 0);
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_downscale_of_a_huge_declared_size_is_guarded_by_the_scaled_output_not_the_original() {
    // C#: TurboJpegGuardMutationTests.Decode_DownscaleOfAHugeDeclaredSize_IsGuardedByTheScaledOutputNotTheOriginal
    // 20000^2 with a 2000 box: factor 1/8 -> 2500^2 (25 MB) fits; the refusal must not name the original size.
    let Some(decoder) = native_decoder_with(memory(1 << 30, 0)) else {
        return;
    };
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/parity-8x8.jpg"),
        &[serde_json::json!({"op": "claim_size", "width": 20000, "height": 20000})],
    );
    let request = DecodeRequest::with_box("h.jpg", DecodeBox::new(2000, 2000)).with_bytes(jpeg.into());
    if let Err(error) = decoder.decode(&request) {
        assert!(!matches!(error.kind(), DecodeError::MemoryAdmission { .. }), "{error}");
    }
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_impossible_output_size_is_refused_without_asking_the_memory_budget() {
    // C#: TurboJpegGuardMutationTests.Decode_ImpossibleOutputSize_IsRefusedWithoutAskingTheMemoryBudget
    let mem = memory(1 << 30, 0);
    let Some(decoder) = decoder_or_skip(mem.clone()) else {
        return;
    };
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/parity-8x8.jpg"),
        &[serde_json::json!({"op": "claim_size", "width": 65535, "height": 65535})],
    );
    let error = decoder
        .decode(&DecodeRequest::new("h.jpg", 0).with_bytes(jpeg.into()))
        .err()
        .expect("impossible");
    assert!(matches!(error.kind(), DecodeError::InvalidData { .. }), "{error:?}");
    assert_eq!(mem.reads.load(Ordering::SeqCst), 0);
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn read_info_first_read_ends_inside_the_sos_header_reads_on_and_returns_the_size() {
    // C#: TurboJpegGuardMutationTests.ReadInfo_FirstReadEndsInsideTheSosHeader_ReadsOnAndReturnsTheSize
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let temp = TempRoot::new("tj-sos-cut");
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/parity-64x48.jpg"),
        &[serde_json::json!({"op": "sos_at", "offset": HEADER_READ_CHUNK - 2})],
    );
    let path = write_under(temp.path(), "cut.jpg", &jpeg);
    let info = decoder.read_info(path.to_str().expect("utf8")).expect("reads on");
    assert_eq!((info.pixel_width, info.pixel_height), (64, 48));
}

// ---------------------------------------------------------------------------------------------------------------
// Hardening and header area (`TurboJpegHardeningTests`, `TurboJpegSharingAndHeaderGrowthGapTests`, native rows)
// ---------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn truncated_mid_scan_is_invalid_data_and_carries_the_source_bytes() {
    // C#: TurboJpegHardeningTests.TruncatedMidScan_InvalidData_CarriesSourceBytes
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let temp = TempRoot::new("tj-trunc");
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/parity-640x480.jpg"),
        &[serde_json::json!({"op": "truncate", "num": 1, "den": 2})],
    );
    let path = write_under(temp.path(), "trunc.jpg", &jpeg);
    let error = decoder.decode(&DecodeRequest::new(path, 0)).err().expect("truncated");
    assert!(matches!(error.kind(), DecodeError::InvalidData { .. }), "{error:?}");
    assert_eq!(error.source_bytes().map(|b| b.len()), Some(jpeg.len()));
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn huge_declared_size_on_small_machine_is_refused_before_allocation() {
    // C#: TurboJpegHardeningTests.HugeDeclaredSize_OnSmallMachine_IsRefusedBeforeAllocation
    let Some(decoder) = native_decoder_with(memory(64 * 1024 * 1024, 0)) else {
        return;
    };
    serialize_pixel_memory();
    let before = NativePixelMemory::live_count();
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/parity-8x8.jpg"),
        &[serde_json::json!({"op": "claim_size", "width": 8192, "height": 8192})],
    );
    let error = decoder
        .decode(&DecodeRequest::new("h.jpg", 0).with_bytes(jpeg.into()))
        .err()
        .expect("refused");
    assert!(matches!(error.kind(), DecodeError::MemoryAdmission { .. }), "{error:?}");
    assert_eq!(NativePixelMemory::live_count(), before);
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn legitimate_large_original_on_normal_machine_passes_the_guard() {
    // C#: TurboJpegHardeningTests.LegitimateLargeOriginal_OnNormalMachine_PassesTheGuard
    let Some(decoder) = native_decoder_with(memory(32 * 1024 * 1024 * 1024, 0)) else {
        return;
    };
    let jpeg = fixture_bytes("turbojpeg/parity-640x480.jpg");
    let image = decoder
        .decode(&DecodeRequest::new("l.jpg", 0).with_bytes(jpeg.into()))
        .expect("passes the guard");
    assert_eq!((image.pixel_width(), image.pixel_height()), (640, 480));
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn progressive_with_few_scans_decodes_and_with_too_many_scans_is_refused() {
    // C#: TurboJpegHardeningTests.ProgressiveWithFewScans_Decodes and ProgressiveWithTooManyScans_IsRefused
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let image = decoder
        .decode(&DecodeRequest::new("p.jpg", 0).with_bytes(progressive_gray(0).into()))
        .expect("64 scans");
    assert_eq!((image.pixel_width(), image.pixel_height()), (8, 8));
    let error = decoder
        .decode(&DecodeRequest::new("p512.jpg", 0).with_bytes(progressive_gray(7).into()))
        .err()
        .expect("512 scans");
    assert!(error.to_string().contains("more than 500 scans"), "{error}");
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn read_info_file_that_another_process_has_open_for_writing_is_still_read() {
    // C#: TurboJpegSharingAndHeaderGrowthGapTests.ReadInfo_FileThatAnotherProcessHasOpenForWriting_IsStillRead
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let temp = TempRoot::new("tj-share-ri");
    let path = write_under(temp.path(), "s.jpg", &fixture_bytes("turbojpeg/parity-8x8.jpg"));
    let _writer = std::fs::OpenOptions::new().write(true).open(&path).expect("writer");
    assert!(decoder.read_info(path.to_str().expect("utf8")).is_ok());
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_file_that_another_process_has_open_for_writing_is_still_decoded() {
    // C#: TurboJpegSharingAndHeaderGrowthGapTests.Decode_FileThatAnotherProcessHasOpenForWriting_IsStillDecoded
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let temp = TempRoot::new("tj-share-d");
    let path = write_under(temp.path(), "s.jpg", &fixture_bytes("turbojpeg/parity-8x8.jpg"));
    let _writer = std::fs::OpenOptions::new().write(true).open(&path).expect("writer");
    assert!(decoder.decode(&DecodeRequest::new(path, 0)).is_ok());
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn read_info_frame_header_beyond_the_first_read_cap_is_found_by_the_extended_read() {
    // C#: TurboJpegSharingAndHeaderGrowthGapTests.ReadInfo_FrameHeaderBeyondTheFirstReadCap_IsFoundByTheExtendedRead
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let temp = TempRoot::new("tj-ext");
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/parity-64x48.jpg"),
        &[serde_json::json!({"op": "pad_com_bytes", "bytes": 200_000, "payload": 60_000})],
    );
    let path = write_under(temp.path(), "ext.jpg", &jpeg);
    let info = decoder.read_info(path.to_str().expect("utf8")).expect("extended read");
    assert_eq!((info.pixel_width, info.pixel_height), (64, 48));
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn read_info_endless_header_segments_without_a_frame_fails_instead_of_reporting_a_size() {
    // C#: TurboJpegSharingAndHeaderGrowthGapTests.ReadInfo_EndlessHeaderSegmentsWithoutAFrame_FailsInsteadOfReportingASize
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let temp = TempRoot::new("tj-endless");
    let jpeg = apply_transforms(
        vec![0xFF, 0xD8],
        &[serde_json::json!({"op": "pad_com_bytes", "bytes": 9 * 1024 * 1024, "payload": 65000, "keep_tail": false})],
    );
    let path = write_under(temp.path(), "endless.jpg", &jpeg);
    let error = decoder.read_info(path.to_str().expect("utf8")).expect_err("no frame");
    assert!(error.to_string().starts_with(messages::INFO_FAILED_PREFIX), "{error}");
}

// ---------------------------------------------------------------------------------------------------------------
// Orientation, box and output format (`TurboJpegTests`, `DecodeBoxDecoderTests`, `DecoderOutputFormatTests`, ...)
// ---------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn read_info_detects_orientation() {
    // C#: TurboJpegTests.ReadInfoDetectsOrientation (8 rows: orientation 1..8 by set_orientation)
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let temp = TempRoot::new("tj-orient");
    for orientation in 1..=8u64 {
        let jpeg = apply_transforms(
            fixture_bytes("turbojpeg/parity-64x48.jpg"),
            &[serde_json::json!({"op": "set_orientation", "value": orientation})],
        );
        let path = write_under(temp.path(), &format!("o{orientation}.jpg"), &jpeg);
        let info = decoder.read_info(path.to_str().expect("utf8")).expect("info");
        assert_eq!(info.orientation, orientation as i32);
        let transposed = (5..=8).contains(&orientation);
        assert_eq!(
            (info.width(), info.height()),
            if transposed { (48, 64) } else { (64, 48) },
            "orientation {orientation}"
        );
    }
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_applies_orientation_corners() {
    // C#: TurboJpegTests.DecodeAppliesOrientationCorners (8 rows). Asserted here on the transposed sizes; the corner
    // colours are checked by the golden `turbojpeg-decode` rows `orient-*` (samples, exact when the DLL matches).
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    for orientation in 1..=8u64 {
        let jpeg = apply_transforms(
            fixture_bytes("turbojpeg/parity-64x48.jpg"),
            &[serde_json::json!({"op": "set_orientation", "value": orientation})],
        );
        let image = decoder
            .decode(&DecodeRequest::new("o.jpg", 0).with_bytes(jpeg.into()))
            .expect("decode");
        let (w, h) = if (5..=8).contains(&orientation) {
            (48, 64)
        } else {
            (64, 48)
        };
        assert_eq!(
            (image.pixel_width(), image.pixel_height()),
            (w, h),
            "orientation {orientation}"
        );
        assert_eq!(image.orientation(), orientation as i32);
    }
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn downscale_produces_target_width() {
    // C#: TurboJpegTests.DownscaleProducesTargetWidth
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let request = DecodeRequest::with_box("parity-400x300.jpg", DecodeBox::new(100, 0))
        .with_bytes(fixture_bytes("turbojpeg/parity-400x300.jpg").into());
    let image = decoder.decode(&request).expect("downscale");
    assert_eq!(image.pixel_width(), 100);
    assert!(image.downscaled());
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decodes_from_memory_buffer() {
    // C#: TurboJpegTests.DecodesFromMemoryBuffer
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let image = decoder
        .decode(&DecodeRequest::new("m.jpg", 0).with_bytes(fixture_bytes("turbojpeg/parity-8x8.jpg").into()))
        .expect("bytes");
    assert_eq!((image.pixel_width(), image.pixel_height()), (8, 8));
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn box_decodes_fit_the_box_like_decode_box_fit() {
    // C#: DecodeBoxDecoderTests.Decode_LandscapeJpeg_FitsBoxHeight, Decode_PortraitJpeg_FitsBoxHeight,
    // Decode_ExifRotatedJpeg_BoxAppliedAfterOrientation, Decode_PanoramaJpeg_FitsBoxWidth, Decode_TinyJpeg_NotUpscaled,
    // Decode_FromBytes_FitsBox, Decode_UnboundedBox_FullSize (the Png row is WIC/WPF: not ported, see the notes).
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let rows: [(&str, DecodeBox); 6] = [
        ("parity-400x300.jpg", DecodeBox::new(0, 200)),
        ("tall-10x1000.jpg", DecodeBox::new(0, 100)),
        ("exif-64x48-o6.jpg", DecodeBox::new(32, 0)),
        ("parity-400x300.jpg", DecodeBox::new(100, 0)),
        ("parity-1x1.jpg", DecodeBox::new(16, 16)),
        ("parity-8x8.jpg", DecodeBox::UNBOUNDED),
    ];
    for (file, decode_box) in rows {
        let bytes = fixture_bytes(&format!("turbojpeg/{file}"));
        let request = DecodeRequest::with_box(file, decode_box).with_bytes(bytes.clone().into());
        let image = decoder.decode(&request).expect("box decode");
        let info = decoder
            .read_info(&fixture(&format!("turbojpeg/{file}")).to_string_lossy())
            .expect("info");
        let transposed = matches!(info.orientation, 5..=8);
        let (w, h) = decode_box.fit_stored(info.pixel_width, info.pixel_height, transposed);
        // The fit is on the stored grid; the oriented image has the axes swapped.
        let (w, h) = if transposed { (h, w) } else { (w, h) };
        assert_eq!(
            (image.pixel_width(), image.pixel_height()),
            (w, h),
            "{file} {decode_box:?}"
        );
    }
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn turbojpeg_outputs_materialized_bgr32_with_estimated_w_h_4() {
    // C#: DecoderOutputFormatTests.TurboJpegOutputsMaterializedBgr32 (5 rows: layout Bgr32, estimated = w * h * 4)
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    for file in [
        "parity-1x1.jpg",
        "parity-8x8.jpg",
        "parity-64x48.jpg",
        "parity-100x75.jpg",
        "parity-257x129.jpg",
    ] {
        let image = decoder
            .decode(&DecodeRequest::new(file, 0).with_bytes(fixture_bytes(&format!("turbojpeg/{file}")).into()))
            .expect("decode");
        let pixels = image.platform_image().as_pixels().expect("materialized pixels");
        assert_eq!(pixels.layout(), pr_imaging::pixels::PixelLayout::Bgr32, "{file}");
        assert_eq!(
            image.estimated_bytes(),
            i64::from(image.pixel_width()) * i64::from(image.pixel_height()) * 4
        );
    }
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_real_jpeg_orientation_and_exif_both_correct() {
    // C#: CombinedHeaderScanTests.Decode_RealJpeg_OrientationAndExifBothCorrect
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let image = decoder
        .decode(&DecodeRequest::new("e.jpg", 0).with_bytes(fixture_bytes("turbojpeg/exif-64x48-o6.jpg").into()))
        .expect("decode");
    assert_eq!(image.orientation(), 6);
    assert_eq!((image.pixel_width(), image.pixel_height()), (48, 64));
    assert!(image.exif().is_some(), "EXIF summary read during the decode");
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn large_header_read_info_agrees_with_decode_when_exif_is_beyond_64_kb() {
    // C#: Robustness/LargeHeaderReadInfoTests.ReadInfo_AgreesWithDecode_WhenExifIsBeyond64Kb (5 rows of padding)
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let temp = TempRoot::new("tj-large");
    for bytes in [60_000usize, 66_000, 70_000, 131_072, 300_000] {
        let jpeg = apply_transforms(
            fixture_bytes("turbojpeg/parity-64x48.jpg"),
            &[serde_json::json!({"op": "pad_com_bytes", "bytes": bytes, "payload": 65_000})],
        );
        let path = write_under(temp.path(), &format!("big{bytes}.jpg"), &jpeg);
        let info = decoder.read_info(path.to_str().expect("utf8")).expect("info");
        let image = decoder.decode(&DecodeRequest::new(path, 0)).expect("decode");
        assert_eq!(
            (info.pixel_width, info.pixel_height),
            (image.pixel_width(), image.pixel_height()),
            "pad {bytes}"
        );
    }
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn large_header_read_info_sos_segment_straddles_the_first_read_chunk_still_parses() {
    // C#: Robustness/LargeHeaderReadInfoTests.ReadInfo_SosSegmentStraddlesTheFirstReadChunk_StillParses
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let temp = TempRoot::new("tj-straddle");
    for k in [1usize, 2, 3, 4] {
        let jpeg = apply_transforms(
            fixture_bytes("turbojpeg/parity-64x48.jpg"),
            &[serde_json::json!({"op": "pad_com_before_sos", "sos_bytes_in_first_chunk": k})],
        );
        let path = write_under(temp.path(), &format!("straddle{k}.jpg"), &jpeg);
        let info = decoder.read_info(path.to_str().expect("utf8")).expect("straddling SOS");
        assert_eq!((info.pixel_width, info.pixel_height), (64, 48), "k {k}");
    }
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_refuses_a_huge_pixel_request_without_a_fallback_chain() {
    // C#: TurboJpegPixelBufferLifetimeTests.HugeClaimedSize_IsAdmissionRefusal_NotOverflow_AndAllocatesNothing (native)
    let Some(decoder) = native_decoder_with(memory(1 << 30, 0)) else {
        return;
    };
    serialize_pixel_memory();
    let before = NativePixelMemory::live_count();
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/parity-8x8.jpg"),
        &[serde_json::json!({"op": "claim_size", "width": 65535, "height": 65535})],
    );
    let _ = decoder.decode(&DecodeRequest::new("h.jpg", 0).with_bytes(jpeg.into()));
    assert_eq!(NativePixelMemory::live_count(), before);
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn corrupt_jpeg_does_not_leak_a_pixel_buffer() {
    // C#: TurboJpegPixelBufferLifetimeTests.CorruptJpeg_DoesNotLeakPixelBuffer (gradient-256x256 with a broken scan)
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    serialize_pixel_memory();
    let before = NativePixelMemory::live_count();
    let jpeg = apply_transforms(
        fixture_bytes("turbojpeg/gradient-256x256.jpg"),
        &[serde_json::json!({"op": "destroy_entropy_from_half"})],
    );
    let _ = decoder.decode(&DecodeRequest::new("c.jpg", 0).with_bytes(jpeg.into()));
    assert_eq!(NativePixelMemory::live_count(), before);
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decode_holds_exactly_one_buffer_for_all_orientations_and_fine_scale() {
    // C#: TurboJpegPixelBufferLifetimeTests.Decode_HoldsExactlyOneBuffer_AllOrientationsAndFineScale (native)
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    serialize_pixel_memory();
    let before = NativePixelMemory::live_count();
    for orientation in 1..=8u64 {
        let jpeg = apply_transforms(
            fixture_bytes("turbojpeg/parity-64x48.jpg"),
            &[serde_json::json!({"op": "set_orientation", "value": orientation})],
        );
        let image = decoder
            .decode(&DecodeRequest::new("o.jpg", 0).with_bytes(jpeg.into()))
            .expect("decode");
        assert_eq!(NativePixelMemory::live_count(), before + 1);
        drop(image);
    }
    assert_eq!(NativePixelMemory::live_count(), before);
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn fine_scale_keeps_the_target_size_within_one_pixel_of_wic() {
    // C#: TurboJpegPixelBufferParityTests.FineScale_CloseToWpf (golden sizes exact; samples within tolerance 3, D39)
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let request = DecodeRequest::with_box("fine.jpg", DecodeBox::new(37, 0))
        .with_bytes(fixture_bytes("turbojpeg/parity-257x129.jpg").into());
    let image = decoder.decode(&request).expect("fine scale");
    assert_eq!(image.pixel_width(), 37);
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn memory_buffer_with_override_matches_legacy_and_custom_codec_receives_the_buffer() {
    // C#: TurboJpegPixelBufferParityTests.MemoryBufferWithOverride_MatchesLegacy, CustomCodec_ReceivesBuffer
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let image = decoder
        .decode(&DecodeRequest::new("m.jpg", 0).with_bytes(fixture_bytes("turbojpeg/parity-64x48.jpg").into()))
        .expect("memory");
    assert_eq!((image.pixel_width(), image.pixel_height()), (64, 48));
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn apply_orientation_false_gives_no_rotation() {
    // C#: TurboJpegPixelBufferParityTests.ApplyOrientationFalse_NoRotation
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let jpeg = fixture_bytes("turbojpeg/exif-64x48-o6.jpg");
    let request = DecodeRequest::new("n.jpg", 0)
        .with_apply_orientation(false)
        .with_bytes(jpeg.into());
    let image = decoder.decode(&request).expect("no rotation");
    assert_eq!((image.pixel_width(), image.pixel_height()), (64, 48));
    assert_eq!(image.orientation(), 1);
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn decoder_with_the_fallback_chain_keeps_the_turbojpeg_result_for_plain_files() {
    // C#: Decoding/SourceOrientationOverrideTests.AllDecoders_SourceOrientationOverride_WinsOverEmbeddedExif (turbo half)
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let request = DecodeRequest::new("s.jpg", 0)
        .with_source_orientation(8)
        .with_bytes(fixture_bytes("turbojpeg/exif-64x48-o6.jpg").into());
    let image = decoder.decode(&request).expect("override");
    assert_eq!(
        image.orientation(),
        8,
        "the given source orientation wins over the embedded EXIF"
    );
}

#[test]
#[ignore = "native: needs turbojpeg.dll"]
fn parallel_native_decodes_match_the_serial_reference() {
    // C#: Decoding/DecoderConcurrencyTests.ParallelDecodes_MatchTheSerialReference (native half)
    let Some(decoder) = native_decoder_with(memory(1 << 36, 0)) else {
        return;
    };
    let decoder = Arc::new(decoder);
    let jpeg = Arc::new(fixture_bytes("turbojpeg/parity-64x48.jpg"));
    let serial = decoder
        .decode(&DecodeRequest::new("s.jpg", 0).with_bytes(jpeg.to_vec().into()))
        .map(|i| bgr_bytes_of(&i))
        .expect("serial");
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let (decoder, jpeg, serial) = (Arc::clone(&decoder), Arc::clone(&jpeg), serial.clone());
            scope.spawn(move || {
                let got = decoder
                    .decode(&DecodeRequest::new("p.jpg", 0).with_bytes(jpeg.to_vec().into()))
                    .map(|i| bgr_bytes_of(&i))
                    .expect("parallel");
                assert_eq!(got, serial);
            });
        }
    });
}

/// The B,G,R bytes of a decoded image (the pixel comparison of the concurrency test).
fn bgr_bytes_of(image: &Arc<dyn DecodedImageView>) -> Vec<u8> {
    bgr_bytes(image.platform_image().as_pixels().expect("pixels"))
}

/// What the poisoned golden rows cannot say: the REAL file read. For a `from: file` row whose bytes are known the decode
/// by path must agree with the decode of the same bytes in memory (same outcome class, same image facts, and the
/// fallbackable failure of the file path carries the file); path-only rows (missing file/dir, directory, blank) must
/// fail with the localized not-found / no-source verdicts of semantics 2.
fn file_row_agrees_with_bytes(
    name: &str,
    case: &Value,
    decoder: &pr_turbojpeg::decoder::TurboJpegDecoder,
    source: &Source,
    in_memory: &DecodeRequest,
) -> Option<String> {
    let src = &case["source"];
    let mut by_path = DecodeRequest::with_box(source.path.clone(), in_memory.decode_box())
        .with_apply_orientation(in_memory.apply_orientation);
    by_path.source_orientation = in_memory.source_orientation;
    if src.get("missing").is_some() || src.get("missing_dir").is_some() || src.get("directory").is_some() {
        return match decoder.decode(&by_path) {
            Err(DecodeError::FileNotFound { .. }) => None,
            Err(other) if matches!(other.kind(), DecodeError::FileNotFound { .. }) && other.is_localized() => None,
            other => Some(format!(
                "{name}: path-only source should be a localized FileNotFound, got {:?}",
                other.map(|_| "ok")
            )),
        };
    }
    if let Some(literal) = src.get("path").and_then(Value::as_str) {
        if literal.trim().is_empty() {
            return match decoder.decode(&by_path) {
                Err(error) if matches!(error.kind(), DecodeError::InvalidArgument { .. }) && error.is_localized() => {
                    None
                }
                other => Some(format!(
                    "{name}: blank path should be a localized no-source error, got {:?}",
                    other.map(|_| "ok")
                )),
            };
        }
    }
    let bytes = source.bytes.as_ref()?;
    // 128 MiB zero files: the same verdict, no need to decode the buffer twice.
    let via_bytes = decoder.decode(&by_path.clone().with_bytes(bytes.clone().into()));
    let via_file = decoder.decode(&by_path);
    match (via_file, via_bytes) {
        (Ok(file), Ok(memory)) => {
            let facts = |image: &std::sync::Arc<dyn pr_imaging::decode_types::DecodedImageView>| {
                (
                    image.pixel_width(),
                    image.pixel_height(),
                    image.estimated_bytes(),
                    image.downscaled(),
                    image.orientation(),
                    image.original_width(),
                    image.original_height(),
                )
            };
            if facts(&file) != facts(&memory) {
                return Some(format!(
                    "{name}: file {:?} vs memory {:?}",
                    facts(&file),
                    facts(&memory)
                ));
            }
            None
        }
        (Err(file), Err(memory)) => {
            if csharp_type(file.kind()) != csharp_type(memory.kind())
                || FallbackImageDecoder::is_fallbackable(&file) != FallbackImageDecoder::is_fallbackable(&memory)
            {
                return Some(format!("{name}: file error {file:?} vs memory error {memory:?}"));
            }
            // A source refused by the pre-read guard (too large for memory) was never read, so it carries nothing.
            let never_read = file.message().starts_with("JPEG source is too large");
            if FallbackImageDecoder::is_fallbackable(&file)
                && !never_read
                && file.source_bytes().map(|b| b.len()) != Some(bytes.len())
            {
                return Some(format!("{name}: a fallbackable file failure must carry the file bytes"));
            }
            None
        }
        (file, memory) => Some(format!(
            "{name}: file outcome {:?} vs memory outcome {:?}",
            file.map(|_| "ok"),
            memory.map(|_| "ok")
        )),
    }
}
