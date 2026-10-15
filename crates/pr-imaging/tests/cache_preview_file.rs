//! Port of the C# `PreviewCacheFile` tests (baseline d92d6aae; PARITY D-06, I-08; INV-1, INV-2):
//! `tests/PhotoReview.Imaging.Tests/Caching/PreviewCacheFileTests.cs`, `Caching/PreviewCacheFileMutationTests.cs`,
//! `Robustness/PreviewCacheFileMutationTests.cs`, `Caching/PreviewCacheExifTests.cs`,
//! `Caching/PreviewCacheCrossVersionTests.cs`, `Caching/PreviewCacheFileConcurrencyTests.cs`, plus golden checks
//! against `tests/golden/preview-entry-read.json` (71 cases) and `tests/golden/preview-entry-write.json` (30 cases).
//!
//! Port of `PhotoReview.Imaging/Caching/PreviewCacheFile.cs` (C# baseline d92d6aae).
//!
//! Red phase (W4-08b): every body in `crates/pr-imaging/src/cache/preview_file.rs` is `todo!()`, so these tests compile
//! against the stub and FAIL until the porter implements the unit. Each test names the C# test it ports in a
//! `// C#:` comment. Native tests are `#[ignore = "native: needs WIC"]` and live with the W4-11e WIC crate tests.
//!
//! The JPEG codec is a deterministic fake ([`FakePayloadCodec`]): its "JPEG" is a frame of `FF D8`, width, height,
//! layout byte, 8 payload bytes and `FF D9`. The fake validates the frame like a decoder would.
//!
//! Documented skips and substitutions (contract W4-08b, "Depends on" and "Traps"):
//! - `IsFullyOpaque_*`, `HasAlpha_*`, `AllAlphaOpaque_*`, `Wp06BridgeSplitTests`: W4-16a `PixelOps`, not this unit.
//! - `DiskHitCarriesExifWithoutSourceRead`: needs `PreviewImageService` (W4-14).
//! - `NewWrite_ByteIdenticalToOldWpfWrite`, `NewWrite_ReadableByOldWpfReader`: WPF-only comparisons, no Rust twin.
//! - `CommittedOldPngThumbnail_ReadsWithNewCode`, `NewPngWrite_MatchesOldPngWrite`, `TranslucentPng_RoundTripsThroughWic`:
//!   thumbnail PNG cache (W4-10b / W4-11e).
//! - `EncodedEntryIsSmallerThanEquivalentPng`, `WriteAtomically_OpaquePbgra32_RoundTripsColours`, and the pixel and JPEG
//!   tail halves of the two cross-version tests: native, need the WIC codec (W4-11e, `pr-platform-win`).
//! - `Robustness/JpegBytes.Mutate` is re-written on a deterministic xorshift RNG (`System.Random(4)` cannot be
//!   reproduced); the C# acceptance bounds are kept.
//! - `PreviewCacheFileWpf.Read` and `LegacyWpfPreviewCache`: WPF-only helpers, replaced by [`read`] with the fake codec.
//! - Golden write rule: the C# exception message carries the .NET `(Parameter '...')` suffix; the comparison uses the
//!   text before it (`contracts/W4-08b.md`, "Golden package").

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

use pr_core::model::DecoderBackend;
use pr_core::model::serde_ext::LenientEnum;
use pr_core::sync::CancelToken;
use pr_imaging::cache::preview_file::{
    CURRENT_VERSION, NO_EXIF_VERSION, PreviewCacheError, PreviewPayloadCodec, backend_from_byte, build_entry_prefix,
    parse_entry, read, read_as_decoded_image, write_atomically, write_decoded_image,
};
use pr_imaging::decode_types::{DecodedImage, DecodedImageOptions, DecodedImageView, PixelLease, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, PlatformImageCodec};
use pr_imaging::exif_summary::{ExifSummary, RawExifTags};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_imaging::tiff::ExifRational;
use pr_test_support::TempRoot;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

// ---- fake JPEG codec -----------------------------------------------------------------------------------------------

/// Payload bytes of the fake frame. Contains no `0xFF`, so no false end marker appears inside a frame.
const FRAME_BODY: [u8; 8] = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];

/// Deterministic stand-in for the WIC JPEG codec: `encode_jpeg` writes a frame, `decode_jpeg` validates one.
#[derive(Debug, Default)]
struct FakePayloadCodec {
    /// Layouts requested from `decode_jpeg`, in call order.
    decoded_layouts: Mutex<Vec<PixelLayout>>,
}

impl FakePayloadCodec {
    /// The bytes `encode_jpeg` writes for a `width x height` buffer of `layout`.
    fn frame(width: i32, height: i32, layout: PixelLayout) -> Vec<u8> {
        let mut frame = vec![0xFF, 0xD8];
        frame.extend_from_slice(&width.to_le_bytes());
        frame.extend_from_slice(&height.to_le_bytes());
        frame.push(u8::try_from(layout.as_i32()).unwrap_or(0));
        frame.extend_from_slice(&FRAME_BODY);
        frame.extend_from_slice(&[0xFF, 0xD9]);
        frame
    }

    fn last_layout(&self) -> Option<PixelLayout> {
        self.decoded_layouts.lock().expect("fake codec lock").last().copied()
    }
}

impl PreviewPayloadCodec for FakePayloadCodec {
    fn encode_jpeg(&self, pixels: &PixelBuffer, _quality: i32, out: &mut dyn Write) -> std::io::Result<()> {
        out.write_all(&FakePayloadCodec::frame(
            pixels.width(),
            pixels.height(),
            pixels.layout(),
        ))
    }

    fn decode_jpeg(
        &self,
        payload: &[u8],
        layout: PixelLayout,
        expected_width: i32,
        expected_height: i32,
    ) -> Result<PixelBuffer, PreviewCacheError> {
        self.decoded_layouts.lock().expect("fake codec lock").push(layout);
        if payload.len() < 13 || !payload.starts_with(&[0xFF, 0xD8]) || !payload.ends_with(&[0xFF, 0xD9]) {
            return Err(PreviewCacheError::InvalidData(
                "Preview cache entry payload could not be decoded.".to_owned(),
            ));
        }
        let width = i32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]);
        let height = i32::from_le_bytes([payload[6], payload[7], payload[8], payload[9]]);
        // The size check runs BEFORE any allocation (contract, semantics note 6).
        if width != expected_width || height != expected_height {
            return Err(PreviewCacheError::InvalidData(
                "Cache entry payload dimensions do not match its header.".to_owned(),
            ));
        }
        PixelBuffer::allocate(expected_width, expected_height, layout)
            .map_err(|e| PreviewCacheError::InvalidData(format!("{e:?}")))
    }
}

/// Platform codec for the decoded-image helpers: the `PlatformImage` is the pixel buffer itself.
struct FakePlatformCodec;

impl PlatformImageCodec for FakePlatformCodec {
    fn name(&self) -> &'static str {
        "pixels"
    }

    fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
        Ok(PlatformImage::Pixels(Arc::new(pixels)))
    }

    fn to_pixels<'a>(&self, platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
        match platform_image.as_pixels() {
            Some(pixels) => Ok(PixelLease::borrowed(pixels)),
            None => Err(DecodeError::InvalidData {
                message: "not a pixel buffer".to_owned(),
            }),
        }
    }
}

// ---- shared helpers ------------------------------------------------------------------------------------------------

/// The shape of a buffer written by a test, plus the header fields the writer receives.
#[derive(Clone, Copy)]
struct Shape {
    width: i32,
    height: i32,
    layout: PixelLayout,
    backend: DecoderBackend,
    orientation: i32,
    original: (i32, i32),
}

impl Shape {
    fn bgr(width: i32, height: i32, backend: DecoderBackend, orientation: i32) -> Self {
        Self {
            width,
            height,
            layout: PixelLayout::Bgr32,
            backend,
            orientation,
            original: (width, height),
        }
    }
}

/// A buffer whose alpha byte is `alpha(x, y)`; colour bytes follow a deterministic pattern.
fn pixels_with_alpha(width: i32, height: i32, layout: PixelLayout, alpha: impl Fn(i32, i32) -> u8) -> PixelBuffer {
    let mut pixels = PixelBuffer::allocate(width, height, layout).expect("allocate test buffer");
    let row_stride = pixels.stride() as usize;
    let bytes = pixels.bytes_mut().expect("test buffer bytes");
    for y in 0..height {
        for x in 0..width {
            let offset = y as usize * row_stride + x as usize * 4;
            let a = alpha(x, y);
            let index = (y * width + x) as u8;
            // Premultiplied layouts need colour <= alpha: a transparent pixel gets zero colour.
            let colour = if a == 0 { 0 } else { index.wrapping_mul(3) };
            bytes[offset] = colour.min(a);
            bytes[offset + 1] = colour.min(a);
            bytes[offset + 2] = colour.min(a);
            bytes[offset + 3] = a;
        }
    }
    pixels
}

fn opaque_pixels(width: i32, height: i32, layout: PixelLayout) -> PixelBuffer {
    pixels_with_alpha(width, height, layout, |_, _| 255)
}

fn decoded_image(shape: Shape, exif: Option<ExifSummary>) -> DecodedImage {
    let pixels = opaque_pixels(shape.width, shape.height, shape.layout);
    let estimated = i64::from(shape.width) * i64::from(shape.height) * 4;
    let options = DecodedImageOptions {
        downscaled: true,
        orientation: shape.orientation,
        actual_backend: shape.backend,
        original_width: shape.original.0,
        original_height: shape.original.1,
        exif,
        is_degraded_fallback: false,
    };
    match DecodedImage::new(
        PlatformImage::Pixels(Arc::new(pixels)),
        shape.width,
        shape.height,
        estimated,
        options,
    ) {
        Ok(image) => image,
        Err(_) => panic!("test decoded image rejected by DecodedImage::new"),
    }
}

/// Writes an entry through the pixel overload of [`write_atomically`].
fn write_shape(path: &Path, shape: Shape, exif: Option<&ExifSummary>) -> Result<(), PreviewCacheError> {
    let pixels = opaque_pixels(shape.width, shape.height, shape.layout);
    write_atomically(
        &pixels,
        shape.backend,
        shape.orientation,
        shape.original.0,
        shape.original.1,
        path,
        false,
        exif,
        &FakePayloadCodec::default(),
        &CancelToken::new(),
    )
}

fn fixture_path(name: &str) -> PathBuf {
    golden::repo_root()
        .join("tests")
        .join("fixtures")
        .join("PreviewCache")
        .join(name)
}

/// The EXIF of the committed fixtures (`FixtureExif` in the C# cross-version tests).
fn fixture_exif() -> ExifSummary {
    ExifSummary::create(&RawExifTags {
        date_taken: Some("2024:05:06 07:08:09"),
        make: Some("Canon"),
        model: Some("EOS R5"),
        lens: Some("RF24-70mm F2.8 L IS USM"),
        iso: Some(400),
        focal_length: Some(ExifRational {
            numerator: 50,
            denominator: 1,
        }),
        f_number: Some(ExifRational {
            numerator: 28,
            denominator: 10,
        }),
        exposure_time: Some(ExifRational {
            numerator: 1,
            denominator: 250,
        }),
    })
    .expect("fixture EXIF summary is not empty")
}

/// The EXIF of the header tests (`Exif` in `PreviewCacheExifTests`).
fn sample_exif() -> ExifSummary {
    ExifSummary::create(&RawExifTags {
        date_taken: Some("2024:05:01 14:03:22"),
        make: Some("Canon"),
        model: Some("Canon EOS R5"),
        lens: Some("RF24-70mm F2.8 L IS USM"),
        iso: Some(400),
        focal_length: Some(ExifRational {
            numerator: 50,
            denominator: 1,
        }),
        f_number: Some(ExifRational {
            numerator: 28,
            denominator: 10,
        }),
        exposure_time: Some(ExifRational {
            numerator: 1,
            denominator: 250,
        }),
    })
    .expect("sample EXIF summary is not empty")
}

/// The 24-byte header, written by hand so the parser tests do not depend on the writer.
#[derive(Clone, Copy)]
struct Hdr {
    version: u8,
    backend: u8,
    orientation: u8,
    alpha: u8,
    width: i32,
    height: i32,
    original_width: i32,
    original_height: i32,
}

impl Hdr {
    /// C# `Header(...)` defaults: v7, backend Wpf (0), orientation 1, alpha 0, original = pixel size.
    fn v7(width: i32, height: i32) -> Self {
        Self {
            version: CURRENT_VERSION,
            backend: 0,
            orientation: 1,
            alpha: 0,
            width,
            height,
            original_width: width,
            original_height: height,
        }
    }

    fn bytes(self) -> Vec<u8> {
        let mut header = Vec::with_capacity(24);
        header.extend_from_slice(b"PRVC");
        header.push(self.version);
        header.push(self.backend);
        header.push(self.orientation);
        header.push(self.alpha);
        header.extend_from_slice(&self.width.to_le_bytes());
        header.extend_from_slice(&self.height.to_le_bytes());
        header.extend_from_slice(&self.original_width.to_le_bytes());
        header.extend_from_slice(&self.original_height.to_le_bytes());
        header
    }
}

/// `u16` EXIF length (little endian) followed by `actual` zero bytes (C# `ExifBlock(declared, actual)`).
fn exif_block(declared: u16, actual: usize) -> Vec<u8> {
    let mut block = declared.to_le_bytes().to_vec();
    block.extend(std::iter::repeat_n(0u8, actual));
    block
}

/// Header, optional EXIF block, optional payload, concatenated (C# `WriteEntry`).
fn entry_bytes(header: Hdr, exif: Option<Vec<u8>>, payload: Option<Vec<u8>>) -> Vec<u8> {
    let mut bytes = header.bytes();
    if let Some(block) = exif {
        bytes.extend(block);
    }
    if let Some(body) = payload {
        bytes.extend(body);
    }
    bytes
}

/// A valid 16x8 payload frame.
fn sample_payload() -> Vec<u8> {
    FakePayloadCodec::frame(16, 8, PixelLayout::Bgr32)
}

fn write_bytes(root: &TempRoot, name: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, bytes).expect("write test entry");
    path
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("golden hex byte"))
        .collect()
}

fn backend_of(value: i64) -> DecoderBackend {
    backend_from_byte(u8::try_from(value).expect("golden backend byte"))
        .unwrap_or_else(|| panic!("golden backend {value} is not defined"))
}

fn count_files(dir: &Path) -> usize {
    fs::read_dir(dir).map(|entries| entries.count()).unwrap_or(0)
}

/// Head of a C# message: the text before the .NET `(Parameter '...')` suffix and its extra lines.
fn message_head(message: &str) -> &str {
    message
        .split(" (Parameter")
        .next()
        .unwrap_or(message)
        .lines()
        .next()
        .unwrap_or(message)
}

// ---- PreviewCacheFileTests -----------------------------------------------------------------------------------------

#[test]
fn write_then_read_round_trips_header_fields() {
    // C#: PreviewCacheFileTests.WriteThenReadRoundTripsHeaderFields (PSNR half is native, see module doc)
    let root = TempRoot::new("pv4-roundtrip");
    let path = root.join("entry.pv4");
    let image = decoded_image(Shape::bgr(220, 160, DecoderBackend::TurboJpeg, 6), None);

    write_decoded_image(
        &image,
        &FakePlatformCodec,
        &path,
        &FakePayloadCodec::default(),
        &CancelToken::new(),
    )
    .expect("write entry");
    let result = read_as_decoded_image(&path, &FakePlatformCodec, &FakePayloadCodec::default()).expect("read entry");

    assert_eq!(result.actual_backend(), DecoderBackend::TurboJpeg);
    assert_eq!(result.orientation(), 6);
    assert_eq!(result.pixel_width(), 220);
    assert_eq!(result.pixel_height(), 160);
    let pixels = result.platform_image().as_pixels().expect("pixel platform image");
    assert_eq!(pixels.layout(), PixelLayout::Bgr32);
}

#[test]
fn write_then_read_round_trips_original_dimensions() {
    // C#: PreviewCacheFileTests.WriteThenReadRoundTripsOriginalDimensions
    let root = TempRoot::new("pv4-original");
    let path = root.join("entry.pv4");
    let shape = Shape {
        original: (4400, 3200),
        ..Shape::bgr(220, 160, DecoderBackend::WicDirect, 1)
    };
    let image = decoded_image(shape, None);

    write_decoded_image(
        &image,
        &FakePlatformCodec,
        &path,
        &FakePayloadCodec::default(),
        &CancelToken::new(),
    )
    .expect("write entry");
    let result = read_as_decoded_image(&path, &FakePlatformCodec, &FakePayloadCodec::default()).expect("read entry");

    assert_eq!(result.pixel_width(), 220);
    assert_eq!(result.pixel_height(), 160);
    assert_eq!(result.original_width(), 4400);
    assert_eq!(result.original_height(), 3200);
}

#[test]
#[ignore = "native: needs WIC"]
fn encoded_entry_is_smaller_than_equivalent_png() {
    // C#: PreviewCacheFileTests.EncodedEntryIsSmallerThanEquivalentPng (PNG half dropped: assert the JPEG entry is
    // under 12.8 MB / 4 for a 2190x1460 gradient). Native twin lands with W4-11e.
    todo!("W4-11e: native twin in crates/pr-platform-win/tests/preview_cache_wic.rs")
}

#[test]
fn mismatched_version_is_rejected() {
    // C#: PreviewCacheFileTests.MismatchedVersionIsRejected
    let root = TempRoot::new("pv4-version");
    let mut bytes = entry_bytes(Hdr::v7(16, 8), Some(exif_block(0, 0)), Some(sample_payload()));
    bytes[4] = CURRENT_VERSION + 1;
    let path = write_bytes(&root, "version.pv4", &bytes);

    match read(&path, &FakePayloadCodec::default()) {
        Err(PreviewCacheError::InvalidData(message)) => {
            assert_eq!(message, "Preview cache entry is version 8, expected 7.");
        }
        other => panic!("version 8 must be InvalidData, got {other:?}"),
    }
}

#[test]
fn legacy_version5_entry_is_rejected() {
    // C#: PreviewCacheFileTests.LegacyVersion5EntryIsRejected
    let root = TempRoot::new("pv4-v5");
    let mut bytes = entry_bytes(Hdr::v7(16, 8), Some(exif_block(0, 0)), Some(sample_payload()));
    bytes[4] = 5;
    let path = write_bytes(&root, "v5.pv4", &bytes);

    assert!(matches!(
        read(&path, &FakePayloadCodec::default()),
        Err(PreviewCacheError::InvalidData(_))
    ));
    const _: () = assert!(CURRENT_VERSION > 5);
}

#[test]
fn bad_magic_is_rejected() {
    // C#: PreviewCacheFileTests.BadMagicIsRejected
    let root = TempRoot::new("pv4-magic");
    let mut bytes = entry_bytes(Hdr::v7(16, 8), Some(exif_block(0, 0)), Some(sample_payload()));
    bytes[0] = b'X';
    let path = write_bytes(&root, "magic.pv4", &bytes);

    assert!(matches!(
        read(&path, &FakePayloadCodec::default()),
        Err(PreviewCacheError::InvalidData(_))
    ));
}

#[test]
fn truncated_payload_is_rejected() {
    // C#: PreviewCacheFileTests.TruncatedPayloadIsRejected (a header-only file: "no payload" in the v7 layout)
    let root = TempRoot::new("pv4-truncated");
    let bytes = entry_bytes(Hdr::v7(16, 8), Some(exif_block(0, 0)), Some(sample_payload()));
    let path = write_bytes(&root, "header-only.pv4", &bytes[..24]);

    assert!(matches!(
        read(&path, &FakePayloadCodec::default()),
        Err(PreviewCacheError::InvalidData(_))
    ));
}

#[test]
fn write_atomically_rejects_alpha_bitmap() {
    // C#: PreviewCacheFileTests.WriteAtomically_RejectsAlphaBitmap (all-zero Pbgra32: transparent pixels)
    let root = TempRoot::new("pv4-alpha");
    let path = root.join("alpha.pv4");
    let pixels = pixels_with_alpha(4, 4, PixelLayout::Pbgra32, |_, _| 0);

    let result = write_atomically(
        &pixels,
        DecoderBackend::Wpf,
        1,
        4,
        4,
        &path,
        false,
        None,
        &FakePayloadCodec::default(),
        &CancelToken::new(),
    );

    assert!(matches!(result, Err(PreviewCacheError::InvalidArgument(_))));
    assert!(!path.exists());
}

#[test]
fn write_atomically_one_transparent_pixel_rejected() {
    // C#: PreviewCacheFileTests.WriteAtomically_OneTransparentPixel_Rejected
    let root = TempRoot::new("pv4-one-transparent");
    let path = root.join("one.pv4");
    let pixels = pixels_with_alpha(
        16,
        16,
        PixelLayout::Pbgra32,
        |x, y| if x == 15 && y == 15 { 0 } else { 255 },
    );

    let result = write_atomically(
        &pixels,
        DecoderBackend::Wpf,
        1,
        16,
        16,
        &path,
        false,
        None,
        &FakePayloadCodec::default(),
        &CancelToken::new(),
    );

    assert!(matches!(result, Err(PreviewCacheError::InvalidArgument(_))));
    assert!(!path.exists());
}

#[test]
fn read_yields_render_native_opaque_pixel_format() {
    // C#: PreviewCacheFileTests.ReadYieldsRenderNativeOpaquePixelFormat (the source is alpha-carrying, JPEG never is)
    let root = TempRoot::new("pv4-bgr32");
    let path = root.join("bgr.pv4");
    write_shape(&path, Shape::bgr(48, 32, DecoderBackend::Wpf, 1), None).expect("write entry");

    let result = read(&path, &FakePayloadCodec::default()).expect("read entry");

    assert_eq!(result.pixels.layout(), PixelLayout::Bgr32);
}

#[test]
fn orientation_round_trips() {
    // C#: PreviewCacheFileTests.OrientationRoundTrips (theory 1..=8, one loop)
    let root = TempRoot::new("pv4-orientation");
    for orientation in 1..=8 {
        let path = root.join(format!("orientation-{orientation}.pv4"));
        write_shape(&path, Shape::bgr(32, 24, DecoderBackend::Wpf, orientation), None).expect("write entry");
        let result = read(&path, &FakePayloadCodec::default()).expect("read entry");
        assert_eq!(result.orientation, orientation, "orientation {orientation}");
    }
}

// ---- PreviewCacheFileMutationTests (Caching) -----------------------------------------------------------------------

#[test]
fn write_orientation_range() {
    // C#: PreviewCacheFileMutationTests.Write_OrientationRange
    let root = TempRoot::new("pv4-write-range");
    write_shape(&root.join("o8.pv4"), Shape::bgr(16, 8, DecoderBackend::Wpf, 8), None).expect("orientation 8");
    write_shape(&root.join("o1.pv4"), Shape::bgr(16, 8, DecoderBackend::Wpf, 1), None).expect("orientation 1");
    assert_eq!(
        read(&root.join("o8.pv4"), &FakePayloadCodec::default())
            .expect("read")
            .orientation,
        8
    );

    for bad in [0, 9] {
        let path = root.join(format!("o{bad}.pv4"));
        let result = write_shape(&path, Shape::bgr(16, 8, DecoderBackend::Wpf, bad), None);
        assert!(
            matches!(result, Err(PreviewCacheError::ArgumentOutOfRange(_))),
            "orientation {bad} must be ArgumentOutOfRange"
        );
        assert!(!path.exists(), "no file for orientation {bad}");
    }
}

#[test]
fn read_orientation_byte_range() {
    // C#: PreviewCacheFileMutationTests.Read_OrientationByteRange
    let root = TempRoot::new("pv4-read-orientation");
    let codec = FakePayloadCodec::default();
    let exif = exif_block(0, 0);
    let payload = sample_payload();

    let ok8 = Hdr {
        orientation: 8,
        ..Hdr::v7(16, 8)
    };
    let path = write_bytes(
        &root,
        "o8.pv4",
        &entry_bytes(ok8, Some(exif.clone()), Some(payload.clone())),
    );
    assert_eq!(read(&path, &codec).expect("orientation 8").orientation, 8);

    for bad in [0u8, 9, 255] {
        let header = Hdr {
            orientation: bad,
            ..Hdr::v7(16, 8)
        };
        let path = write_bytes(
            &root,
            &format!("bad{bad}.pv4"),
            &entry_bytes(header, Some(exif.clone()), Some(payload.clone())),
        );
        match read(&path, &codec) {
            Err(PreviewCacheError::InvalidData(message)) => assert!(message.contains("orientation"), "{message}"),
            other => panic!("orientation {bad} must be InvalidData, got {other:?}"),
        }
    }
}

#[test]
fn read_alpha_flag_selects_output_format() {
    // C#: PreviewCacheFileMutationTests.Read_AlphaFlag_SelectsOutputFormat (asserts the layout the codec received)
    let root = TempRoot::new("pv4-alpha-flag");
    let exif = exif_block(0, 0);
    let payload = sample_payload();

    for (name, alpha, expected) in [
        ("alpha1.pv4", 1u8, PixelLayout::Pbgra32),
        ("alpha255.pv4", 255u8, PixelLayout::Pbgra32),
        ("opaque.pv4", 0u8, PixelLayout::Bgr32),
    ] {
        let header = Hdr {
            alpha,
            ..Hdr::v7(16, 8)
        };
        let path = write_bytes(
            &root,
            name,
            &entry_bytes(header, Some(exif.clone()), Some(payload.clone())),
        );
        let codec = FakePayloadCodec::default();
        let result = read(&path, &codec).expect("read entry");
        assert_eq!(codec.last_layout(), Some(expected), "{name}: codec layout");
        assert_eq!(result.pixels.layout(), expected, "{name}: result layout");
    }
}

#[test]
fn read_non_positive_pixel_dimensions_are_rejected_by_the_header_check() {
    // C#: PreviewCacheFileMutationTests.Read_NonPositivePixelDimensions_AreRejectedByTheHeaderCheck (5 rows)
    for (width, height) in [(0, 8), (16, 0), (-16, 8), (16, -8), (0, 0)] {
        let bytes = entry_bytes(Hdr::v7(width, height), Some(exif_block(0, 0)), Some(sample_payload()));
        match parse_entry(&bytes) {
            Err(PreviewCacheError::InvalidData(message)) => {
                assert!(message.contains("pixel dimensions"), "{width}x{height}: {message}");
            }
            other => panic!("{width}x{height} must be InvalidData, got {other:?}"),
        }
    }
}

#[test]
fn read_payload_dimension_mismatch_is_rejected() {
    // C#: PreviewCacheFileMutationTests.Read_PayloadDimensionMismatch_IsRejected (fake codec: "do not match its header")
    let root = TempRoot::new("pv4-mismatch");
    let bytes = entry_bytes(Hdr::v7(17, 8), Some(exif_block(0, 0)), Some(sample_payload()));
    let path = write_bytes(&root, "mismatch.pv4", &bytes);

    match read(&path, &FakePayloadCodec::default()) {
        Err(PreviewCacheError::InvalidData(message)) => assert!(message.contains("do not match"), "{message}"),
        other => panic!("mismatch must be InvalidData, got {other:?}"),
    }
}

#[test]
fn read_header_plus_one_byte_is_no_payload() {
    // C#: PreviewCacheFileMutationTests.Read_HeaderPlusOneByte_IsNoPayload
    let bytes = entry_bytes(Hdr::v7(16, 8), Some(vec![0]), None);

    match parse_entry(&bytes) {
        Err(PreviewCacheError::InvalidData(message)) => assert!(message.contains("no payload"), "{message}"),
        other => panic!("header + 1 byte must be 'no payload', got {other:?}"),
    }
}

#[test]
fn read_no_payload_is_rejected() {
    // C#: PreviewCacheFileMutationTests.Read_NoPayload_IsRejected
    let v7_empty = entry_bytes(Hdr::v7(16, 8), Some(exif_block(0, 0)), None);
    let v6_empty = entry_bytes(
        Hdr {
            version: NO_EXIF_VERSION,
            ..Hdr::v7(16, 8)
        },
        None,
        None,
    );
    for (name, bytes) in [("v7 empty", v7_empty), ("v6 empty", v6_empty)] {
        match parse_entry(&bytes) {
            Err(PreviewCacheError::InvalidData(message)) => {
                assert!(message.contains("no payload"), "{name}: {message}")
            }
            other => panic!("{name} must be 'no payload', got {other:?}"),
        }
    }
}

#[test]
fn read_exif_block_bounds() {
    // C#: PreviewCacheFileMutationTests.Read_ExifBlockBounds
    let exact = entry_bytes(Hdr::v7(16, 8), Some(exif_block(5, 5)), None);
    let over1 = entry_bytes(Hdr::v7(16, 8), Some(exif_block(5, 4)), None);
    let over5 = entry_bytes(Hdr::v7(16, 8), Some(exif_block(5, 0)), None);

    for (name, bytes, needle) in [
        ("exact", exact, "no payload"),
        ("over1", over1, "oversized"),
        ("over5", over5, "oversized"),
    ] {
        match parse_entry(&bytes) {
            Err(PreviewCacheError::InvalidData(message)) => assert!(message.contains(needle), "{name}: {message}"),
            other => panic!("{name} must be InvalidData({needle}), got {other:?}"),
        }
    }
}

#[test]
fn read_exif_block_at_maximum() {
    // C#: PreviewCacheFileMutationTests.Read_ExifBlockAtMaximum (1024 = codec maximum; one byte more is rejected)
    let root = TempRoot::new("pv4-exif-max");
    let codec = FakePayloadCodec::default();
    let max = u16::try_from(pr_imaging::exif_summary::codec::MAX_ENCODED_LENGTH).expect("codec maximum fits u16");

    let ok = write_bytes(
        &root,
        "max.pv4",
        &entry_bytes(
            Hdr::v7(16, 8),
            Some(exif_block(max, usize::from(max))),
            Some(sample_payload()),
        ),
    );
    let result = read(&ok, &codec).expect("maximum-size EXIF block is accepted");
    assert!(result.exif.is_none(), "an unreadable summary means no EXIF");
    assert_eq!(result.pixels.width(), 16);

    let too_big = write_bytes(
        &root,
        "max1.pv4",
        &entry_bytes(
            Hdr::v7(16, 8),
            Some(exif_block(max + 1, usize::from(max) + 1)),
            Some(sample_payload()),
        ),
    );
    match read(&too_big, &codec) {
        Err(PreviewCacheError::InvalidData(message)) => assert!(message.contains("oversized"), "{message}"),
        other => panic!("one byte over the maximum must be oversized, got {other:?}"),
    }
}

#[test]
fn read_four_byte_payload_ending_in_eoi_is_not_reported_as_truncated() {
    // C#: PreviewCacheFileMutationTests.Read_FourBytePayloadEndingInEoi_IsNotReportedAsTruncated
    let root = TempRoot::new("pv4-four-byte");
    let bytes = entry_bytes(
        Hdr::v7(16, 8),
        Some(exif_block(0, 0)),
        Some(vec![0x00, 0x00, 0xFF, 0xD9]),
    );
    assert!(
        parse_entry(&bytes).is_ok(),
        "a 4-byte payload ending in EOI passes the end-marker check"
    );
    let path = write_bytes(&root, "four.pv4", &bytes);

    match read(&path, &FakePayloadCodec::default()) {
        Ok(_) => {}
        Err(PreviewCacheError::InvalidData(message)) => {
            assert!(!message.contains("truncated"), "must reach the decoder, got: {message}");
        }
        Err(other) => panic!("unexpected failure class: {other:?}"),
    }
}

#[test]
fn read_truncated_payload_is_rejected() {
    // C#: PreviewCacheFileMutationTests.Read_TruncatedPayload_IsRejected
    let payload = sample_payload();
    let cut = entry_bytes(
        Hdr::v7(16, 8),
        Some(exif_block(0, 0)),
        Some(payload[..payload.len() - 3].to_vec()),
    );
    let tiny = entry_bytes(Hdr::v7(16, 8), Some(exif_block(0, 0)), Some(vec![0xFF, 0xD9, 0xFF]));

    for (name, bytes) in [("cut", cut), ("tiny", tiny)] {
        match parse_entry(&bytes) {
            Err(PreviewCacheError::InvalidData(message)) => assert!(message.contains("truncated"), "{name}: {message}"),
            other => panic!("{name} must be 'truncated', got {other:?}"),
        }
    }
}

#[cfg(windows)]
#[test]
fn read_while_another_handle_holds_the_file_open_succeeds() {
    // C#: PreviewCacheFileMutationTests.Read_WhileAnotherHandleHoldsTheFileOpen_Succeeds
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;
    const FILE_SHARE_DELETE: u32 = 0x4;
    const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;

    let root = TempRoot::new("pv4-share");
    let bytes = entry_bytes(Hdr::v7(16, 8), Some(exif_block(0, 0)), Some(sample_payload()));
    let path = write_bytes(&root, "shared.pv4", &bytes);

    {
        let _holder = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
            .open(&path)
            .expect("hold the entry open");
        let result = read(&path, &FakePayloadCodec::default()).expect("read while another handle holds the file");
        assert_eq!(result.pixels.width(), 16);
    }
    assert!(!path.exists(), "the holder deleted the entry on close");
}

// ---- IsFullyOpaque / HasAlpha: not here (W4-16a PixelOps) ----------------------------------------------------------
// C#: PreviewCacheFileMutationTests.HasAlpha_* / IsFullyOpaque_* / AllAlphaOpaque_* are ported with W4-16a.

// ---- Robustness/PreviewCacheFileMutationTests ----------------------------------------------------------------------

/// The seed of the robustness tests: a v7 entry with a 20-byte EXIF block, 16x12, orientation 6, original 4000x3000.
fn robustness_seed() -> Vec<u8> {
    let block_body: Vec<u8> = (0..20u8).map(|i| i.wrapping_mul(7).wrapping_add(1)).collect();
    let mut block = 20u16.to_le_bytes().to_vec();
    block.extend(block_body);
    let header = Hdr {
        backend: 2,
        orientation: 6,
        original_width: 4000,
        original_height: 3000,
        ..Hdr::v7(16, 12)
    };
    entry_bytes(
        header,
        Some(block),
        Some(FakePayloadCodec::frame(16, 12, PixelLayout::Bgr32)),
    )
}

/// Deterministic xorshift64 (no `rand` crate). The seed is mixed so that seed 0 still works.
struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform-ish index in `0..bound` (`bound > 0`).
    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next_u64() % u64::try_from(bound).expect("bound fits u64")).expect("index fits usize")
    }

    fn byte(&mut self) -> u8 {
        u8::try_from(self.next_u64() & 0xFF).expect("masked to a byte")
    }
}

/// Mutation kinds of the C# `JpegBytes.Mutate` (replace byte, flip bit, remove range, insert bytes, truncate, set 0xFF,
/// set 0x00, duplicate slice, write an extreme 16-bit value), applied 1 to 3 times.
fn mutate(rng: &mut XorShift, seed: &[u8]) -> Vec<u8> {
    let mut out = seed.to_vec();
    let rounds = 1 + rng.below(3);
    for _ in 0..rounds {
        if out.is_empty() {
            out.push(rng.byte());
            continue;
        }
        let len = out.len();
        let index = rng.below(len);
        match rng.below(9) {
            0 => out[index] = rng.byte(),
            1 => out[index] ^= 1 << rng.below(8),
            2 => {
                let count = 1 + rng.below(len.min(64));
                let start = rng.below(len - count + 1);
                out.drain(start..start + count);
            }
            3 => {
                let count = 1 + rng.below(16);
                let inserted: Vec<u8> = (0..count).map(|_| rng.byte()).collect();
                let tail = out.split_off(index);
                out.extend(inserted);
                out.extend(tail);
            }
            4 => out.truncate(rng.below(len)),
            5 => out[index] = 0xFF,
            6 => out[index] = 0x00,
            7 => {
                let count = 1 + rng.below(len.min(32));
                let start = rng.below(len - count + 1);
                let slice = out[start..start + count].to_vec();
                let tail = out.split_off(index);
                out.extend(slice);
                out.extend(tail);
            }
            _ => {
                let extremes = [0xFFFFu16, 0x0000, 0x8000, 0x7FFF];
                let value = extremes[rng.below(extremes.len())].to_le_bytes();
                if index + 1 < len {
                    out[index] = value[0];
                    out[index + 1] = value[1];
                }
            }
        }
    }
    out
}

fn run_mutations(iterations: usize, rng_seed: u64) {
    // C#: Robustness.PreviewCacheFileMutationTests.RunMutations (every outcome is Ok or a corrupt-entry failure)
    let root = TempRoot::new("pv4-mutations");
    let seed = robustness_seed();
    let path = root.join("mutant.pv4");
    let mut rng = XorShift::new(rng_seed);
    let codec = FakePayloadCodec::default();
    let mut good = 0usize;
    let mut rejected = 0usize;
    let mut worst = std::time::Duration::ZERO;

    for index in 0..iterations {
        let mutant = mutate(&mut rng, &seed);
        fs::write(&path, &mutant).expect("write mutant");
        let started = std::time::Instant::now();
        match read(&path, &codec) {
            Ok(result) => {
                good += 1;
                assert!(
                    (1..=8).contains(&result.orientation),
                    "mutant {index}: orientation {}",
                    result.orientation
                );
                assert!(
                    result.pixels.width() > 0 && result.pixels.height() > 0,
                    "mutant {index}: pixel size"
                );
                assert!(
                    result.original_width > 0 && result.original_height > 0,
                    "mutant {index}: original size"
                );
                assert_eq!(
                    result.file_bytes,
                    i64::try_from(mutant.len()).expect("length fits i64"),
                    "mutant {index}"
                );
            }
            Err(error) if error.is_cache_entry_failure() => rejected += 1,
            Err(error) => panic!("mutant {index} ({} bytes) escaped as {error:?}", mutant.len()),
        }
        worst = worst.max(started.elapsed());
    }

    assert!(worst < std::time::Duration::from_secs(2), "slowest read {worst:?}");
    assert!(
        good > iterations / 30 && rejected > iterations / 30,
        "corpus is one-sided: {good} good, {rejected} rejected"
    );
}

#[test]
fn mutated_entries_end_in_result_or_corrupt_entry_failure() {
    // C#: Robustness.PreviewCacheFileMutationTests.MutatedEntries_EndInResultOrCorruptEntryFailure (300 iterations, seed 4)
    run_mutations(300, 4);
}

#[test]
#[ignore = "slow"]
fn mutated_entries_deep_run() {
    // C#: Robustness.PreviewCacheFileMutationTests.MutatedEntries_DeepRun (3 000 iterations; seed is a documented
    // stand-in for DecoderMutationFuzzTests.DeepSeed(5))
    run_mutations(3_000, 5);
}

#[test]
fn every_truncation_is_rejected() {
    // C#: Robustness.PreviewCacheFileMutationTests.EveryTruncation_IsRejected (every length 0..len; parse_entry Err)
    let seed = entry_bytes(
        Hdr {
            orientation: 3,
            ..Hdr::v7(8, 8)
        },
        Some(exif_block(0, 0)),
        Some(FakePayloadCodec::frame(8, 8, PixelLayout::Bgr32)),
    );
    for length in 0..seed.len() {
        assert!(
            parse_entry(&seed[..length]).is_err(),
            "an entry truncated to {length}/{} bytes was accepted",
            seed.len()
        );
    }
    assert!(parse_entry(&seed).is_ok(), "the untruncated entry is still good");
}

#[test]
fn extreme_dimension_fields_are_rejected() {
    // C#: Robustness.PreviewCacheFileMutationTests.ExtremeDimensionFields_AreRejected (10 rows; the i32::MAX rows are
    // rejected by the decoder's size check, so the assertion is on read(), not parse_entry)
    let root = TempRoot::new("pv4-extreme");
    let seed = robustness_seed();
    for (offset, value) in [
        (8usize, 0i32),
        (8, -1),
        (8, i32::MAX),
        (12, 0),
        (12, i32::MIN),
        (12, i32::MAX),
        (16, 0),
        (16, -5),
        (20, 0),
        (20, i32::MIN),
    ] {
        let mut bytes = seed.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let path = write_bytes(&root, "dims.pv4", &bytes);
        match read(&path, &FakePayloadCodec::default()) {
            Err(PreviewCacheError::InvalidData(_)) => {}
            other => panic!("offset {offset} value {value} must be InvalidData, got {other:?}"),
        }
    }
}

#[test]
fn invalid_enum_bytes_are_rejected() {
    // C#: Robustness.PreviewCacheFileMutationTests.InvalidEnumBytes_AreRejected (4 rows)
    let root = TempRoot::new("pv4-enum");
    let seed = robustness_seed();
    for (offset, value) in [(6usize, 0u8), (6, 9), (6, 255), (5, 200)] {
        let mut bytes = seed.clone();
        bytes[offset] = value;
        let path = write_bytes(&root, "enum.pv4", &bytes);
        assert!(
            matches!(
                read(&path, &FakePayloadCodec::default()),
                Err(PreviewCacheError::InvalidData(_))
            ),
            "byte {offset} = {value} must be InvalidData"
        );
    }
}

#[test]
fn oversized_exif_length_is_rejected() {
    // C#: Robustness.PreviewCacheFileMutationTests.OversizedExifLength_IsRejected
    let root = TempRoot::new("pv4-exif-length");
    let seed = robustness_seed();
    let length_fields = [1025u16, 0xFFFF, u16::try_from(seed.len()).expect("seed fits u16")];
    for length in length_fields {
        let mut bytes = seed.clone();
        bytes[24..26].copy_from_slice(&length.to_le_bytes());
        let path = write_bytes(&root, "exiflen.pv4", &bytes);
        assert!(
            matches!(
                read(&path, &FakePayloadCodec::default()),
                Err(PreviewCacheError::InvalidData(_))
            ),
            "EXIF length {length} must be rejected"
        );
    }
}

#[test]
fn garbage_exif_block_keeps_pixels() {
    // C#: Robustness.PreviewCacheFileMutationTests.GarbageExifBlock_KeepsPixels
    let root = TempRoot::new("pv4-garbage-exif");
    let mut bytes = robustness_seed();
    for byte in &mut bytes[26..46] {
        *byte = 0xEE;
    }
    let path = write_bytes(&root, "garbage-exif.pv4", &bytes);

    let result = read(&path, &FakePayloadCodec::default()).expect("garbage EXIF keeps the entry readable");

    assert!(result.exif.is_none());
    assert_eq!(result.pixels.width(), 16);
}

#[test]
fn zero_and_one_byte_files_are_rejected() {
    // C#: Robustness.PreviewCacheFileMutationTests.ZeroAndOneByteFiles_AreRejected (end-of-stream is a corrupt entry)
    let root = TempRoot::new("pv4-tiny");
    for (name, bytes) in [("empty.pv4", Vec::new()), ("one.pv4", vec![b'P'])] {
        let path = write_bytes(&root, name, &bytes);
        match read(&path, &FakePayloadCodec::default()) {
            Err(error) => assert!(error.is_cache_entry_failure(), "{name}: {error:?}"),
            Ok(_) => panic!("{name} must be rejected"),
        }
    }
}

// ---- PreviewCacheExifTests -----------------------------------------------------------------------------------------

#[test]
fn entry_round_trips_exif() {
    // C#: PreviewCacheExifTests.EntryRoundTripsExif
    let root = TempRoot::new("pv4-exif-roundtrip");
    let path = root.join("roundtrip.pv4");
    let exif = sample_exif();
    let shape = Shape {
        original: (4000, 6000),
        ..Shape::bgr(8, 6, DecoderBackend::Wpf, 6)
    };
    write_shape(&path, shape, Some(&exif)).expect("write entry");

    let read_back = read(&path, &FakePayloadCodec::default()).expect("read entry");

    assert_eq!(read_back.exif, Some(exif));
    assert_eq!(read_back.orientation, 6);
    assert_eq!((read_back.original_width, read_back.original_height), (4000, 6000));
    assert_eq!((read_back.pixels.width(), read_back.pixels.height()), (8, 6));
}

#[test]
fn entry_without_exif() {
    // C#: PreviewCacheExifTests.EntryWithoutExif
    let root = TempRoot::new("pv4-no-exif");
    let path = root.join("none.pv4");
    write_shape(&path, Shape::bgr(8, 6, DecoderBackend::Wpf, 1), None).expect("write entry");

    let read_back = read(&path, &FakePayloadCodec::default()).expect("read entry");

    assert!(read_back.exif.is_none());
    assert_eq!(fs::read(&path).expect("entry bytes")[4], CURRENT_VERSION);
}

#[test]
fn version6_entry_is_read_without_exif() {
    // C#: PreviewCacheExifTests.Version6EntryIsReadWithoutExif (v6 layout: payload right after the 24-byte header)
    let root = TempRoot::new("pv4-v6");
    let header = Hdr {
        version: NO_EXIF_VERSION,
        original_width: 800,
        original_height: 600,
        ..Hdr::v7(8, 6)
    };
    let bytes = entry_bytes(header, None, Some(FakePayloadCodec::frame(8, 6, PixelLayout::Bgr32)));
    let path = write_bytes(&root, "v6.pv4", &bytes);

    let read_back = read(&path, &FakePayloadCodec::default()).expect("v6 entry is still served");

    assert!(read_back.exif.is_none());
    assert_eq!((read_back.original_width, read_back.original_height), (800, 600));
    assert_eq!((read_back.pixels.width(), read_back.pixels.height()), (8, 6));
}

#[test]
fn corrupt_exif_block_keeps_pixels() {
    // C#: PreviewCacheExifTests.CorruptExifBlockKeepsPixels (EXIF block format version byte set to 99)
    let root = TempRoot::new("pv4-corrupt-exif");
    let exif = sample_exif();
    let path = root.join("corrupt.pv4");
    write_shape(&path, Shape::bgr(8, 6, DecoderBackend::Wpf, 1), Some(&exif)).expect("write entry");
    let mut bytes = fs::read(&path).expect("entry bytes");
    bytes[26] = 99;
    fs::write(&path, &bytes).expect("corrupt the EXIF block");

    let read_back = read(&path, &FakePayloadCodec::default()).expect("pixels survive a corrupt EXIF block");

    assert!(read_back.exif.is_none());
    assert_eq!(read_back.pixels.width(), 8);
}

#[test]
fn exif_oversized_length_is_rejected() {
    // C#: PreviewCacheExifTests.OversizedExifLengthIsRejected (the length field is ushort.MaxValue)
    let root = TempRoot::new("pv4-exif-oversized");
    let path = root.join("oversized.pv4");
    write_shape(&path, Shape::bgr(8, 6, DecoderBackend::Wpf, 1), Some(&sample_exif())).expect("write entry");
    let mut bytes = fs::read(&path).expect("entry bytes");
    bytes[24..26].copy_from_slice(&u16::MAX.to_le_bytes());
    fs::write(&path, &bytes).expect("patch the length");

    assert!(matches!(
        read(&path, &FakePayloadCodec::default()),
        Err(PreviewCacheError::InvalidData(_))
    ));
}

// ---- PreviewCacheCrossVersionTests (pure halves; pixel and JPEG-tail halves are native) ----------------------------

#[test]
fn committed_old_fixture_header_and_exif_match_old_reader() {
    // C#: PreviewCacheCrossVersionTests.CommittedOldFixture_ReadsWithNewCode_IdenticalToOldReader (header/EXIF half;
    // the pixel half is native, see module doc)
    let cases = [
        (
            "v7-exif-bgr32-96x64.pv4",
            DecoderBackend::WicDirect,
            6u8,
            6000,
            4000,
            true,
            2875usize,
            101usize,
        ),
        (
            "v7-noexif-pbgra32-96x64.pv4",
            DecoderBackend::TurboJpeg,
            1u8,
            96,
            64,
            false,
            2800usize,
            26usize,
        ),
    ];
    for (name, backend, orientation, original_width, original_height, has_exif, file_bytes, payload_offset) in cases {
        let bytes = fs::read(fixture_path(name)).expect("committed fixture");
        assert_eq!(bytes.len(), file_bytes, "{name}: fixture size");

        let parsed = parse_entry(&bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));

        assert_eq!(parsed.header.backend, backend, "{name}: backend");
        assert_eq!(parsed.header.orientation, orientation, "{name}: orientation");
        assert_eq!(
            (parsed.header.original_width, parsed.header.original_height),
            (original_width, original_height),
            "{name}: original size"
        );
        assert_eq!(
            (parsed.header.width, parsed.header.height),
            (96, 64),
            "{name}: pixel size"
        );
        assert_eq!(
            parsed.file_bytes,
            i64::try_from(file_bytes).expect("fits"),
            "{name}: file bytes"
        );
        assert_eq!(parsed.payload_offset, payload_offset, "{name}: payload offset");
        assert!(!parsed.header.has_alpha, "{name}: alpha flag is 0 in both fixtures");
        assert_eq!(parsed.exif.is_some(), has_exif, "{name}: EXIF presence");
        if has_exif {
            assert_eq!(parsed.exif, Some(fixture_exif()), "{name}: EXIF content");
        }
    }
}

#[test]
fn new_writer_reproduces_committed_old_fixture_prefix() {
    // C#: PreviewCacheCrossVersionTests.NewWriter_ReproducesCommittedOldFixtureBytes (prefix half: build_entry_prefix
    // equals the first 26 + n bytes of each fixture; the JPEG tail is native)
    let exif_fixture = fs::read(fixture_path("v7-exif-bgr32-96x64.pv4")).expect("committed fixture");
    let exif_prefix = build_entry_prefix(DecoderBackend::WicDirect, 6, 96, 64, 6000, 4000, Some(&fixture_exif()))
        .expect("prefix for the EXIF fixture");
    assert_eq!(exif_prefix.as_slice(), &exif_fixture[..exif_prefix.len()]);
    assert_eq!(exif_prefix.len(), 26 + 75);

    let plain_fixture = fs::read(fixture_path("v7-noexif-pbgra32-96x64.pv4")).expect("committed fixture");
    let plain_prefix =
        build_entry_prefix(DecoderBackend::TurboJpeg, 1, 96, 64, 0, 0, None).expect("prefix for the no-EXIF fixture");
    assert_eq!(plain_prefix.as_slice(), &plain_fixture[..plain_prefix.len()]);
    assert_eq!(plain_prefix.len(), 26);
}

#[test]
fn old_v6_entry_reads_with_new_code() {
    // C#: PreviewCacheCrossVersionTests.OldV6Entry_ReadsWithNewCode (pure half: the v6 layout parses, no EXIF)
    let v7 = fs::read(fixture_path("v7-exif-bgr32-96x64.pv4")).expect("committed fixture");
    let exif_length = usize::from(u16::from_le_bytes([v7[24], v7[25]]));
    let mut v6 = v7[..24].to_vec();
    v6.extend_from_slice(&v7[26 + exif_length..]);
    v6[4] = NO_EXIF_VERSION;

    let parsed = parse_entry(&v6).expect("a v6 entry parses");

    assert!(parsed.exif.is_none());
    assert_eq!(parsed.header.orientation, 6);
    assert_eq!(parsed.payload_offset, 24);
}

#[test]
fn read_as_decoded_image_platform_image_follows_codec() {
    // C#: PreviewCacheCrossVersionTests.ReadAsDecodedImage_PlatformImageFollowsCodec (the pixel-buffer codec branch;
    // the WPF branch is WPF-only)
    let path = fixture_path("v7-exif-bgr32-96x64.pv4");

    let image = read_as_decoded_image(&path, &FakePlatformCodec, &FakePayloadCodec::default())
        .expect("committed fixture reads as a decoded image");

    assert!(image.downscaled());
    assert_eq!(image.estimated_bytes(), 96 * 64 * 4);
    assert_eq!(image.orientation(), 6);
    assert_eq!((image.original_width(), image.original_height()), (6000, 4000));
    assert_eq!(image.exif(), Some(&fixture_exif()));
    let pixels = image.platform_image().as_pixels().expect("pixel platform image");
    assert_eq!(
        (pixels.width(), pixels.height(), pixels.layout()),
        (96, 64, PixelLayout::Bgr32)
    );
}

#[test]
#[ignore = "native: needs WIC"]
fn committed_old_fixture_pixels_match_old_reader() {
    // C#: PreviewCacheCrossVersionTests.CommittedOldFixture_ReadsWithNewCode_IdenticalToOldReader (pixel half; needs the
    // WIC JPEG decoder of W4-11e to compare against the committed WPF pixels)
    todo!("W4-11e: native twin in crates/pr-platform-win/tests/preview_cache_wic.rs")
}

#[test]
#[ignore = "native: needs WIC"]
fn new_writer_reproduces_committed_old_fixture_jpeg_tail() {
    // C#: PreviewCacheCrossVersionTests.NewWriter_ReproducesCommittedOldFixtureBytes (JPEG tail; needs WIC encode)
    todo!("W4-11e: native twin in crates/pr-platform-win/tests/preview_cache_wic.rs")
}

#[test]
#[ignore = "native: needs WIC"]
fn write_atomically_opaque_pbgra32_round_trips_colours() {
    // C#: PreviewCacheFileTests.WriteAtomically_OpaquePbgra32_RoundTripsColours (per-channel error <= 3; needs WIC)
    todo!("W4-11e: native twin in crates/pr-platform-win/tests/preview_cache_wic.rs")
}

// ---- PreviewCacheFileConcurrencyTests ------------------------------------------------------------------------------

#[test]
fn concurrent_replace_and_read_never_yields_a_torn_entry() {
    // C#: PreviewCacheFileConcurrencyTests.ConcurrentReplaceAndRead_NeverYieldsATornEntry
    let root = TempRoot::new("pv4-concurrency");
    let path = root.join("entry.pv4");
    let first = Shape::bgr(8, 6, DecoderBackend::Wpf, 1);
    let second = Shape::bgr(16, 12, DecoderBackend::TurboJpeg, 6);
    write_shape(&path, first, None).expect("seed entry");

    let barrier = Barrier::new(7);
    let stop = AtomicBool::new(false);
    let reads = AtomicUsize::new(0);
    let failures: Mutex<Vec<String>> = Mutex::new(Vec::new());

    std::thread::scope(|scope| {
        let writer_threads: Vec<_> = (0..3usize)
            .map(|writer| {
                let (barrier, path) = (&barrier, path.as_path());
                scope.spawn(move || {
                    barrier.wait();
                    for i in 0..40usize {
                        let shape = if (writer + i) % 2 == 0 { first } else { second };
                        match write_shape(path, shape, None) {
                            Ok(()) | Err(PreviewCacheError::Io(_)) => {}
                            Err(other) => panic!("writer failed: {other:?}"),
                        }
                    }
                })
            })
            .collect();
        for _ in 0..4usize {
            let (barrier, path, stop, reads, failures) = (&barrier, path.as_path(), &stop, &reads, &failures);
            scope.spawn(move || {
                barrier.wait();
                while !stop.load(Ordering::SeqCst) {
                    match read(path, &FakePayloadCodec::default()) {
                        Ok(result) => {
                            reads.fetch_add(1, Ordering::SeqCst);
                            let shape = (
                                result.pixels.width(),
                                result.pixels.height(),
                                result.orientation,
                                result.actual_backend,
                            );
                            let consistent = matches!(
                                shape,
                                (8, 6, 1, DecoderBackend::Wpf) | (16, 12, 6, DecoderBackend::TurboJpeg)
                            );
                            if !consistent {
                                failures
                                    .lock()
                                    .expect("failure list")
                                    .push(format!("torn entry: {shape:?}"));
                            }
                        }
                        Err(PreviewCacheError::Io(_)) => {}
                        Err(other) => failures.lock().expect("failure list").push(format!("{other:?}")),
                    }
                }
            });
        }
        // Readers spin until every writer is done; joining the writers first keeps the readers from running forever.
        let writer_results: Vec<_> = writer_threads.into_iter().map(|handle| handle.join()).collect();
        // Stop the readers before reporting a writer panic, so a failing writer cannot hang the scope.
        stop.store(true, Ordering::SeqCst);
        for result in writer_results {
            result.expect("writer thread panicked");
        }
    });

    assert!(
        failures.lock().expect("failure list").is_empty(),
        "{:?}",
        failures.lock().expect("failure list")
    );
    assert!(reads.load(Ordering::SeqCst) > 0);
    let temp_files = fs::read_dir(root.path())
        .expect("list root")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(temp_files, 0, "no temp file is left behind");
    let final_width = read(&path, &FakePayloadCodec::default())
        .expect("final entry")
        .pixels
        .width();
    assert!(final_width == 8 || final_width == 16);
}

// ---- Golden-driven tests -------------------------------------------------------------------------------------------

/// Applies the golden `ops` (`set`, `delete`, `truncate`, `append`, `append_zeros`, `insert_zeros`, `zero`, `insert`).
fn apply_ops(base: &[u8], ops: &[Value]) -> Vec<u8> {
    let mut bytes = base.to_vec();
    for op in ops {
        match str_of(op, "op") {
            "set" => {
                let offset = usize::try_from(i64_of(op, "offset")).expect("offset");
                let data = hex_decode(str_of(op, "hex"));
                bytes[offset..offset + data.len()].copy_from_slice(&data);
            }
            "delete" => {
                let offset = usize::try_from(i64_of(op, "offset")).expect("offset");
                let count = usize::try_from(i64_of(op, "count")).expect("count");
                bytes.drain(offset..offset + count);
            }
            "truncate" => bytes.truncate(usize::try_from(i64_of(op, "length")).expect("length")),
            "append" => bytes.extend(hex_decode(str_of(op, "hex"))),
            "append_zeros" => {
                let count = usize::try_from(i64_of(op, "count")).expect("count");
                bytes.extend(std::iter::repeat_n(0u8, count));
            }
            "insert_zeros" => {
                let offset = usize::try_from(i64_of(op, "offset")).expect("offset");
                let count = usize::try_from(i64_of(op, "count")).expect("count");
                let tail = bytes.split_off(offset);
                bytes.extend(std::iter::repeat_n(0u8, count));
                bytes.extend(tail);
            }
            "zero" => {
                let offset = usize::try_from(i64_of(op, "offset")).expect("offset");
                let count = usize::try_from(i64_of(op, "count")).expect("count");
                bytes[offset..offset + count].fill(0);
            }
            "insert" => {
                let offset = usize::try_from(i64_of(op, "offset")).expect("offset");
                let data = hex_decode(str_of(op, "hex"));
                let tail = bytes.split_off(offset);
                bytes.extend(data);
                bytes.extend(tail);
            }
            other => panic!("golden op {other} is not known"),
        }
    }
    bytes
}

#[test]
fn preview_entry_read_golden_outcomes_match_csharp() {
    // Golden: tests/golden/preview-entry-read.json (71 cases). Rule: contracts/W4-08b.md, "Golden package".
    let golden = golden::load("preview-entry-read");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let base_file = match str_of(case, "base") {
            "exif" => "v7-exif-bgr32-96x64.pv4",
            "noexif" => "v7-noexif-pbgra32-96x64.pv4",
            other => panic!("{name}: unknown base {other}"),
        };
        let base = fs::read(fixture_path(base_file)).expect("committed fixture");
        let ops = case["ops"].as_array().expect("golden ops array");
        let bytes = apply_ops(&base, ops);
        let outcome = str_of(case, "outcome");
        let parsed = parse_entry(&bytes);

        match outcome {
            "ok" => {
                let entry = parsed.unwrap_or_else(|e| panic!("{name}: expected ok, got {e:?}"));
                assert_eq!(entry.file_bytes, i64_of(case, "file_bytes"), "{name}: file_bytes");
                assert_eq!(
                    i64::from(entry.header.backend.value()),
                    i64_of(case, "backend"),
                    "{name}: backend"
                );
                assert_eq!(
                    i64::from(entry.header.orientation),
                    i64_of(case, "orientation"),
                    "{name}: orientation"
                );
                assert_eq!(i64::from(entry.header.width), i64_of(case, "width"), "{name}: width");
                assert_eq!(i64::from(entry.header.height), i64_of(case, "height"), "{name}: height");
                assert_eq!(
                    i64::from(entry.header.original_width),
                    i64_of(case, "original_width"),
                    "{name}: original width"
                );
                assert_eq!(
                    i64::from(entry.header.original_height),
                    i64_of(case, "original_height"),
                    "{name}: original height"
                );
                let layout = if entry.header.has_alpha { "Pbgra32" } else { "Bgr32" };
                assert_eq!(layout, str_of(case, "layout"), "{name}: layout");
                assert_eq!(entry.exif.is_some(), bool_of(case, "has_exif"), "{name}: has_exif");
                assert_eq!(
                    entry.exif.as_ref().and_then(|e| e.camera_make.as_deref()),
                    case["exif_make"].as_str(),
                    "{name}: exif_make"
                );
                assert_eq!(
                    entry.exif.as_ref().and_then(|e| e.camera_model.as_deref()),
                    case["exif_model"].as_str(),
                    "{name}: exif_model"
                );
                assert_eq!(
                    entry.exif.as_ref().and_then(|e| e.iso).map(i64::from),
                    case["exif_iso"].as_i64(),
                    "{name}: exif_iso"
                );
            }
            // The failure is the JPEG decoder's (W4-11e / native), so only the header stage is checked here.
            "decode-failed" => assert!(parsed.is_ok(), "{name}: header must pass, the decoder fails"),
            "invalid-data" => match parsed {
                Err(PreviewCacheError::InvalidData(message)) => {
                    assert_eq!(message, str_of(case, "message"), "{name}: message");
                }
                other => panic!("{name}: expected InvalidData, got {other:?}"),
            },
            "end-of-stream" => match parsed {
                Err(PreviewCacheError::Io(error)) => {
                    assert_eq!(error.kind(), ErrorKind::UnexpectedEof, "{name}: kind");
                }
                other => panic!("{name}: expected end of stream, got {other:?}"),
            },
            other => panic!("{name}: golden outcome {other} is a golden bug, fix the case"),
        }
    }
}

fn exif_from_golden(value: &Value) -> Option<ExifSummary> {
    if value.is_null() {
        return None;
    }
    let rational = |key: &str| {
        value[key].as_array().map(|pair| ExifRational {
            numerator: u32::try_from(pair[0].as_u64().expect("numerator")).expect("fits u32"),
            denominator: u32::try_from(pair[1].as_u64().expect("denominator")).expect("fits u32"),
        })
    };
    ExifSummary::create(&RawExifTags {
        date_taken: value["date"].as_str(),
        make: value["make"].as_str(),
        model: value["model"].as_str(),
        lens: value["lens"].as_str(),
        iso: value["iso"].as_i64(),
        focal_length: rational("focal"),
        f_number: rational("f_number"),
        exposure_time: rational("exposure"),
    })
}

#[test]
fn preview_entry_write_golden_matches_csharp() {
    // Golden: tests/golden/preview-entry-write.json (30 cases). Rule: contracts/W4-08b.md, "Golden package".
    let golden = golden::load("preview-entry-write");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = &case["case"];
        let outcome = str_of(case, "outcome");
        let root = TempRoot::new("pv4-write-golden");
        let cache_dir = root.join("cache");
        let path = cache_dir.join("entry.pv4");

        let width = i32::try_from(i64_of(input, "width")).expect("width");
        let height = i32::try_from(i64_of(input, "height")).expect("height");
        let layout = match str_of(input, "layout") {
            "bgr32" => PixelLayout::Bgr32,
            "pbgra32" => PixelLayout::Pbgra32,
            other => panic!("{name}: layout {other}"),
        };
        let backend = backend_of(i64_of(input, "backend"));
        let orientation = i32::try_from(i64_of(input, "orientation")).expect("orientation");
        let original_width = i32::try_from(i64_of(input, "original_width")).expect("original width");
        let original_height = i32::try_from(i64_of(input, "original_height")).expect("original height");
        let opacity_verified = bool_of(input, "opacity_verified");
        let transparent = input["transparent_pixel"]
            .as_array()
            .map(|pair| (pair[0].as_i64().expect("x") as i32, pair[1].as_i64().expect("y") as i32));
        let pixels = pixels_with_alpha(
            width,
            height,
            layout,
            |x, y| {
                if transparent == Some((x, y)) { 0 } else { 255 }
            },
        );
        let exif = exif_from_golden(&input["exif"]);

        let result = write_atomically(
            &pixels,
            backend,
            orientation,
            original_width,
            original_height,
            &path,
            opacity_verified,
            exif.as_ref(),
            &FakePayloadCodec::default(),
            &CancelToken::new(),
        );

        match outcome {
            "written" => {
                result.unwrap_or_else(|e| panic!("{name}: expected written, got {e:?}"));
                let prefix = build_entry_prefix(
                    backend,
                    orientation,
                    width,
                    height,
                    original_width,
                    original_height,
                    exif.as_ref(),
                )
                .unwrap_or_else(|e| panic!("{name}: prefix {e:?}"));
                assert_eq!(hex(&prefix), str_of(case, "prefix_hex"), "{name}: prefix bytes");
                assert_eq!(
                    i64::from(u16::from_le_bytes([prefix[24], prefix[25]])),
                    i64_of(case, "exif_length"),
                    "{name}: exif length"
                );
                let file = fs::read(&path).unwrap_or_else(|e| panic!("{name}: entry not written: {e}"));
                assert!(file.starts_with(&prefix), "{name}: file starts with the prefix");
                assert_eq!(
                    file[prefix.len()..].starts_with(&[0xFF, 0xD8]),
                    bool_of(case, "payload_starts_with_soi"),
                    "{name}: SOI"
                );
                assert_eq!(
                    file.ends_with(&[0xFF, 0xD9]),
                    bool_of(case, "payload_ends_with_eoi"),
                    "{name}: EOI"
                );
                assert_eq!(
                    count_files(&cache_dir),
                    usize::try_from(i64_of(case, "file_count_after")).expect("count"),
                    "{name}: files after write (no temp file)"
                );
            }
            "argument" => {
                match result {
                    Err(PreviewCacheError::InvalidArgument(message)) => {
                        assert_eq!(
                            message_head(&message),
                            message_head(str_of(case, "message")),
                            "{name}: message"
                        );
                    }
                    other => panic!("{name}: expected InvalidArgument, got {other:?}"),
                }
                assert_eq!(count_files(&cache_dir), 0, "{name}: refusal leaves no file");
            }
            "out-of-range" => {
                match result {
                    Err(PreviewCacheError::ArgumentOutOfRange(message)) => {
                        assert_eq!(
                            message_head(&message),
                            message_head(str_of(case, "message")),
                            "{name}: message"
                        );
                    }
                    other => panic!("{name}: expected ArgumentOutOfRange, got {other:?}"),
                }
                assert_eq!(count_files(&cache_dir), 0, "{name}: refusal leaves no file");
            }
            other => panic!("{name}: golden outcome {other} is a golden bug, fix the case"),
        }
    }
}
