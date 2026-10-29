//! SYNTHETIC RAW evidence (2026-10-29, machine A, lane `synthetic-raw`). Rust rows of docs/PARITY.md cannot be proved with a
//! camera corpus on a machine that has none, so `tools/dev/make-synthetic-raw.py` writes minimal valid DNG files (16 bit
//! uncompressed RGGB CFA, optional baseline JPEG preview) and these tests push them through the repo's own readers.
//!
//! THESE ARE NOT A CAMERA CORPUS. They show "a valid DNG of this shape is accepted, sized and routed as expected"; they say
//! nothing about vendor maker notes, compressed sensor data, lossless JPEG, X-Trans, white balance tables or the C# corpus
//! goldens. The corpus rows of docs/PARITY.md stay Partial.
//!
//! No C# twin: Rust-only evidence tests. The generator is embedded at compile time (`include_str!`) and run with
//! `python -I`; when python is not on PATH the tests print a skip note and return (set `PHOTOREVIEW_RS_SYNTHETIC_STRICT=1`
//! to turn the skip into a failure; CI runners have python). The native tests are `#[ignore = "native: needs libraw.dll"]`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeBox, DecodeRequest};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_imaging_raw::decoder::RawDecoder;
use pr_imaging_raw::readers::dng::DngContainerReader;
use pr_imaging_raw::{InMemoryRawHeaderSource, RawContainerInfo, RawContainerReader};
use pr_libraw::decoder::{LibRawDecoder, MAX_QUEUED_PRELOAD_DECODES, NativeLibRaw, read_jpeg_thumbnail};
use pr_libraw::full_decode_gate::FullDecodeGate;
use pr_test_support::TempRoot;
use pr_turbojpeg::decoder::{EnglishTurboJpegTexts, FineScaleError, NativeDecompressors, TurboJpegDecoder};
use pr_turbojpeg::native::TurboJpegLibrary;

/// The generator, embedded so no repo-relative path is needed at run time.
const GENERATOR: &str = include_str!("../../../tools/dev/make-synthetic-raw.py");
const WIDTH: i32 = 160;
const HEIGHT: i32 = 96;
const GIB: i64 = 1 << 30;

fn strict() -> bool {
    matches!(
        std::env::var("PHOTOREVIEW_RS_SYNTHETIC_STRICT").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// Generates the file set into `root` and returns the folder, or `None` (skip) when no python runs the generator.
fn generate(root: &TempRoot) -> Option<PathBuf> {
    let script = root.join("make-synthetic-raw.py");
    std::fs::write(&script, GENERATOR).expect("write the generator into the temp root");
    let out = root.join("out");
    for python in ["python", "python3", "py"] {
        let run = Command::new(python)
            .arg("-I")
            .arg(&script)
            .arg(&out)
            .arg("--size")
            .arg(format!("{WIDTH}x{HEIGHT}"))
            .output();
        match run {
            Ok(output) if output.status.success() => return Some(out),
            Ok(output) => panic!(
                "{python} could not run the generator: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
            Err(_) => continue,
        }
    }
    assert!(!strict(), "python is required in strict synthetic-RAW mode");
    eprintln!("skipped: no python on PATH, the synthetic RAW generator cannot run");
    None
}

fn read_container(path: &Path) -> RawContainerInfo {
    let bytes = std::fs::read(path).expect("read generated DNG");
    let mut source = InMemoryRawHeaderSource::new(bytes);
    DngContainerReader::new()
        .read(&mut source, &CancelToken::new())
        .unwrap_or_else(|error| panic!("DNG container reader rejected {}: {error:?}", path.display()))
}

fn preview_sizes(info: &RawContainerInfo) -> Vec<(i32, i32)> {
    info.previews.iter().map(|p| (p.width, p.height)).collect()
}

// Rust-only: the container reader sees the sensor size, the preview and the orientation the generator wrote.
#[test]
fn synthetic_dngs_have_the_sensor_size_previews_and_orientation_the_generator_wrote() {
    let root = TempRoot::new("synthetic-raw-container");
    let Some(dir) = generate(&root) else { return };
    let plain = read_container(&dir.join("plain.dng"));
    assert_eq!((plain.sensor_width, plain.sensor_height), (WIDTH, HEIGHT));
    assert!(plain.previews.is_empty(), "plain.dng carries no preview");
    assert_eq!(plain.orientation, 1);

    let preview = read_container(&dir.join("preview.dng"));
    assert_eq!((preview.sensor_width, preview.sensor_height), (WIDTH, HEIGHT));
    assert_eq!(preview_sizes(&preview), vec![(WIDTH / 2, HEIGHT / 2)]);

    let subifd = read_container(&dir.join("subifd.dng"));
    assert_eq!((subifd.sensor_width, subifd.sensor_height), (WIDTH, HEIGHT));
    assert_eq!(preview_sizes(&subifd), vec![(WIDTH / 2, HEIGHT / 2)]);

    // The swap file: the preview is smaller than the sensor, so a viewer shows it first and swaps to the full decode.
    let swap = read_container(&dir.join("swap.dng"));
    assert_eq!((swap.sensor_width, swap.sensor_height), (WIDTH, HEIGHT));
    assert_eq!(preview_sizes(&swap), vec![(WIDTH / 4, HEIGHT / 4)]);

    assert_eq!(read_container(&dir.join("orient6.dng")).orientation, 6);
    assert_eq!(read_container(&dir.join("orient3.dng")).orientation, 3);
    assert!(dir.join("pair.jpg").is_file() && dir.join("pair.dng").is_file());
}

fn native_ready() -> bool {
    let outcome = pr_libraw::availability::probe();
    if !outcome.available {
        assert!(!strict(), "libraw.dll is required in strict mode: {:?}", outcome.reason);
        eprintln!(
            "skipped native test: {}",
            outcome.reason.as_deref().unwrap_or("libraw.dll unavailable")
        );
    }
    outcome.available
}

fn libraw_decoder() -> LibRawDecoder {
    let memory: Arc<dyn MemoryInfoSource> = Arc::new(|| MemoryInfo {
        total_available: 64 * GIB,
        load: 0,
    });
    LibRawDecoder::new(PixelBufferImageCodec::shared(), Arc::new(NativeLibRaw), memory)
        .with_gate(FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES))
}

/// BGR(A) of the decoded pixel at (x, y).
fn pixel_at(image: &dyn pr_imaging::decode_types::DecodedImageView, x: i32, y: i32) -> [u8; 3] {
    let lease = PixelBufferImageCodec::shared()
        .to_pixels(image.platform_image())
        .expect("pixels of the decoded image");
    let buffer: &PixelBuffer = lease.pixels().expect("pixel-backed image");
    assert!(matches!(buffer.layout(), PixelLayout::Bgr32 | PixelLayout::Pbgra32));
    let row = buffer.get_row(y).expect("row");
    let at = usize::try_from(x).expect("x") * 4;
    [row[at], row[at + 1], row[at + 2]]
}

fn near(actual: [u8; 3], expected_bgr: [u8; 3], tolerance: i32) -> bool {
    actual
        .iter()
        .zip(expected_bgr)
        .all(|(a, e)| (i32::from(*a) - i32::from(e)).abs() <= tolerance)
}

// Rust-only (native): LibRaw accepts the plain CFA DNG, reports the sensor size and demosaics the colour bars (bar 1 yellow,
// bar 5 red, last bar black, the marker black on the white first bar).
#[test]
#[ignore = "native: needs libraw.dll"]
fn libraw_decodes_a_synthetic_cfa_dng_with_the_right_size_and_plausible_colours() {
    if !native_ready() {
        return;
    }
    let root = TempRoot::new("synthetic-raw-libraw");
    let Some(dir) = generate(&root) else { return };
    let path = dir.join("plain.dng");
    let path_text = path.to_str().expect("utf-8 temp path");
    let decoder = libraw_decoder();
    let info = decoder
        .read_info(path_text)
        .expect("LibRaw read_info of the synthetic DNG");
    assert_eq!((info.pixel_width, info.pixel_height), (WIDTH, HEIGHT));

    let image = decoder
        .decode(&DecodeRequest::new(path_text, 0))
        .expect("LibRaw full decode of the synthetic DNG");
    assert_eq!((image.pixel_width(), image.pixel_height()), (WIDTH, HEIGHT));
    let bar = WIDTH / 8;
    let at_bar = |n: i32| pixel_at(image.as_ref(), bar * n + bar / 2, HEIGHT / 3);
    // BGR order. Interpolation and the sRGB curve make saturated bars approximate: a wide tolerance, but the hue is decisive.
    assert!(near(at_bar(0), [255, 255, 255], 40), "white bar {:?}", at_bar(0));
    assert!(near(at_bar(1), [0, 255, 255], 60), "yellow bar {:?}", at_bar(1));
    assert!(near(at_bar(5), [0, 0, 255], 60), "red bar {:?}", at_bar(5));
    assert!(near(at_bar(7), [0, 0, 0], 40), "black bar {:?}", at_bar(7));
    assert!(
        near(pixel_at(image.as_ref(), 4, 4), [0, 0, 0], 40),
        "corner marker {:?}",
        pixel_at(image.as_ref(), 4, 4)
    );
}

// Rust-only (native): an EXIF orientation 6 (rotate 90 cw) makes LibRaw report the flipped size, as the C# ReadInfo comment says.
#[test]
#[ignore = "native: needs libraw.dll"]
fn libraw_reports_the_flipped_size_for_a_portrait_oriented_synthetic_dng() {
    if !native_ready() {
        return;
    }
    let root = TempRoot::new("synthetic-raw-orient");
    let Some(dir) = generate(&root) else { return };
    let path = dir.join("orient6.dng");
    let info = libraw_decoder()
        .read_info(path.to_str().expect("utf-8 temp path"))
        .expect("read_info");
    assert_eq!((info.pixel_width, info.pixel_height), (HEIGHT, WIDTH));
}

fn jpeg_size(bytes: &[u8]) -> (i32, i32) {
    let sof = bytes
        .windows(2)
        .position(|pair| pair == [0xFF, 0xC0])
        .expect("SOF0 marker in the preview");
    let height = i32::from(u16::from_be_bytes([bytes[sof + 5], bytes[sof + 6]]));
    let width = i32::from(u16::from_be_bytes([bytes[sof + 7], bytes[sof + 8]]));
    (width, height)
}

// Rust-only (native): the preview-only path (LibRaw thumbnail) returns the embedded JPEG of the DNG, and fails for the one
// without a preview.
#[test]
#[ignore = "native: needs libraw.dll"]
fn libraw_thumbnail_path_returns_the_embedded_preview_of_a_synthetic_dng() {
    if !native_ready() {
        return;
    }
    let root = TempRoot::new("synthetic-raw-thumb");
    let Some(dir) = generate(&root) else { return };
    let preview = dir.join("preview.dng");
    let jpeg = read_jpeg_thumbnail(
        &NativeLibRaw,
        preview.to_str().expect("utf-8 temp path"),
        &CancelToken::none(),
    )
    .expect("embedded preview of preview.dng");
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
    assert_eq!(jpeg_size(&jpeg), (WIDTH / 2, HEIGHT / 2));

    let plain = dir.join("plain.dng");
    let result = read_jpeg_thumbnail(
        &NativeLibRaw,
        plain.to_str().expect("utf-8 temp path"),
        &CancelToken::none(),
    );
    assert!(result.is_err(), "plain.dng has no thumbnail: {result:?}");
}

// Rust-only (native): the production RAW decoder (container reader + TurboJPEG preview) returns the 1/2-size preview for a
// small box while ReadInfo reports the sensor size: exactly the situation of the viewer's preview-to-full swap. The swap file
// (preview 1/4 size) and the pair behave the same way.
#[test]
#[ignore = "native: needs libraw.dll and turbojpeg.dll"]
fn raw_decoder_serves_the_embedded_preview_of_a_synthetic_dng_smaller_than_the_sensor() {
    let library_path = pr_test_support::golden::repo_root()
        .join("native")
        .join("x64")
        .join("turbojpeg.dll");
    let jpeg_library = match TurboJpegLibrary::load(&library_path) {
        Ok(library) => library,
        Err(error) => {
            assert!(!strict(), "turbojpeg.dll is required in strict mode: {error:?}");
            eprintln!("skipped native test: turbojpeg.dll unusable ({error:?})");
            return;
        }
    };
    let root = TempRoot::new("synthetic-raw-rawdecoder");
    let Some(dir) = generate(&root) else { return };
    let memory: Arc<dyn MemoryInfoSource> = Arc::new(|| MemoryInfo {
        total_available: 64 * GIB,
        load: 0,
    });
    let jpeg = TurboJpegDecoder::new(
        PixelBufferImageCodec::shared(),
        Arc::new(
            |_: &PixelBuffer, _: i32, _: i32| -> Result<PixelBuffer, FineScaleError> {
                Err(FineScaleError::Failed(DecodeError::Other {
                    message: "synthetic preview decode unexpectedly required fine scaling".to_owned(),
                }))
            },
        ),
        memory,
        Arc::new(NativeDecompressors::with_library(jpeg_library)),
        Arc::new(EnglishTurboJpegTexts),
    );
    let raw = RawDecoder::new(Arc::new(jpeg));
    for (name, preview) in [
        ("preview.dng", (WIDTH / 2, HEIGHT / 2)),
        ("subifd.dng", (WIDTH / 2, HEIGHT / 2)),
        ("swap.dng", (WIDTH / 4, HEIGHT / 4)),
    ] {
        let path = dir.join(name);
        let text = path.to_str().expect("utf-8 temp path");
        let info = raw
            .read_info(text)
            .unwrap_or_else(|error| panic!("{name} read_info: {error}"));
        assert_eq!(
            (info.pixel_width, info.pixel_height),
            (WIDTH, HEIGHT),
            "{name} ReadInfo is the sensor size"
        );
        let image = raw
            .decode(&DecodeRequest::with_box(
                text,
                DecodeBox {
                    width: WIDTH,
                    height: HEIGHT,
                },
            ))
            .unwrap_or_else(|error| panic!("{name} decode: {error}"));
        assert_eq!(image.embedded_preview_size(), Some(preview), "{name} embedded preview");
        assert_eq!(
            (image.pixel_width(), image.pixel_height()),
            preview,
            "{name} pixel size is the preview size"
        );
        assert_eq!(
            (image.original_width(), image.original_height()),
            (WIDTH, HEIGHT),
            "{name} original size"
        );
        assert!(
            image.downscaled(),
            "{name}: a preview smaller than the sensor is downscaled"
        );
        // The preview carries the same scene: yellow in the second bar (BGR).
        let bar = preview.0 / 8;
        assert!(
            near(
                pixel_at(image.as_ref(), bar + bar / 2, preview.1 / 3),
                [0, 255, 255],
                60
            ),
            "{name} yellow bar"
        );
    }
}
