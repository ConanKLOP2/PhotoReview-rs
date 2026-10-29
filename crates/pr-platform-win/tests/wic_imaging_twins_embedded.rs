//! Native twins of C# EmbeddedThumbnailReader tests; moved here to avoid a dev-dependency cycle.
#![cfg(windows)]
#[path = "common/imaging_twins_wic.rs"]
mod imaging_twins_wic;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::session::store::sha256;
use pr_imaging::decode_types::{DecodedImage, DecodedImageView};
use pr_imaging::embedded_thumbnail::{EmbeddedThumbnailBackend, try_read};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_test_support::TempRoot;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;
use std::path::{Path, PathBuf};
const FIXTURE_DIR: &str = "tests/fixtures/embedded-thumbnail";
fn fixture_dir() -> PathBuf {
    golden::repo_root().join(FIXTURE_DIR)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let manifest = golden::load("embedded-thumbnail-fixtures");
    let entry = manifest
        .cases
        .iter()
        .find(|c| str_of(c, "file") == name)
        .unwrap_or_else(|| panic!("fixture {name} is not in embedded-thumbnail-fixtures.json"));
    let bytes = std::fs::read(fixture_dir().join(name)).unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
    assert_eq!(bytes.len() as i64, i64_of(entry, "length"), "fixture {name}: length");
    assert_eq!(hex(&sha256(&bytes)), str_of(entry, "sha256"), "fixture {name}: sha256");
    bytes
}

fn truncation_lengths(full_length: usize) -> Vec<usize> {
    let mut lengths = Vec::new();
    let mut length = 0;
    while length < full_length {
        lengths.push(length);
        length += if length < 700 { 1 } else { 9 };
    }
    lengths
}

fn mutate(rng: &mut Lcg, input: &[u8]) -> Vec<u8> {
    let mut data = input.to_vec();
    let rounds = 1 + rng.below(3);
    for _ in 0..rounds {
        if data.is_empty() {
            data.push(rng.byte());
            continue;
        }
        match rng.below(9) {
            0 => {
                let i = rng.below(data.len());
                data[i] = rng.byte();
            }
            1 => {
                let i = rng.below(data.len());
                data[i] ^= 1 << rng.below(8);
            }
            2 => {
                let pos = rng.below(data.len());
                let n = (1 + rng.below(8)).min(data.len() - pos);
                data.drain(pos..pos + n);
            }
            3 => {
                let at = rng.below(data.len() + 1);
                let n = 1 + rng.below(8);
                for k in 0..n {
                    let b = rng.byte();
                    data.insert(at + k, b);
                }
            }
            4 => {
                let keep = rng.below(data.len() + 1);
                data.truncate(keep);
            }
            5 => {
                let i = rng.below(data.len());
                data[i] = 0xFF;
            }
            6 => {
                let i = rng.below(data.len());
                data[i] = 0x00;
            }
            7 => {
                let start = rng.below(data.len());
                let len = (1 + rng.below(23)).min(data.len() - start);
                let slice = data[start..start + len].to_vec();
                let at = rng.below(data.len() + 1);
                for (k, b) in slice.into_iter().enumerate() {
                    data.insert(at + k, b);
                }
            }
            _ => {
                let pos = rng.below(data.len());
                let extreme: [u8; 2] = match rng.below(3) {
                    0 => [0xFF, 0xFF],
                    1 => [0x7F, 0xFF],
                    _ => [0x00, 0x00],
                };
                for (i, b) in extreme.iter().enumerate() {
                    if pos + i < data.len() {
                        data[pos + i] = *b;
                    }
                }
            }
        }
    }
    data
}

fn app1_range(jpeg: &[u8]) -> (usize, usize) {
    let mut at = 2usize;
    while at + 4 <= jpeg.len() && jpeg[at] == 0xFF {
        let len = (usize::from(jpeg[at + 2]) << 8) | usize::from(jpeg[at + 3]);
        if jpeg[at + 1] == 0xE1 {
            return (at, at + 2 + len);
        }
        at += 2 + len;
    }
    (0, 0)
}

fn bgra_at(image: &DecodedImage, x: i32, y: i32) -> [u8; 4] {
    let px = image
        .platform_image()
        .as_pixels()
        .unwrap_or_else(|| panic!("platform image is not a PixelBuffer"));
    assert_eq!(px.layout(), PixelLayout::Pbgra32, "layout");
    let row = px.get_row(y).unwrap_or_else(|_| panic!("row {y} is not readable"));
    let o = x as usize * 4;
    [row[o], row[o + 1], row[o + 2], row[o + 3]]
}
struct Lcg(u64);

impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    /// Uniform in `0..n`; `n` must be positive.
    fn below(&mut self, n: usize) -> usize {
        self.next_u32() as usize % n
    }

    fn byte(&mut self) -> u8 {
        self.next_u32() as u8
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden-driven native read (needs WIC; `-- --ignored` on Windows)
// ---------------------------------------------------------------------------------------------------------------------

/// Real WIC adapter for the pure reader, owned by this test binary.
fn native_backend() -> Box<dyn EmbeddedThumbnailBackend> {
    Box::new(imaging_twins_wic::NativeBackend)
}

/// `try_read` on the real file system with the WIC backend and the pixel codec.
fn read_native(path: &Path) -> Option<DecodedImage> {
    let backend = native_backend();
    let codec = PixelBufferImageCodec::shared();
    try_read(path, &PhysicalFileSystem, backend.as_ref(), codec.as_ref()).unwrap_or_else(|e| panic!("{e}"))
}

/// `"B,G,R,A"` of the pixel at `(x, y)` (the golden's sample format).
fn sample(px: &PixelBuffer, x: i32, y: i32) -> String {
    match px.get_row(y) {
        Ok(row) => {
            let o = x as usize * 4;
            format!("{},{},{},{}", row[o], row[o + 1], row[o + 2], row[o + 3])
        }
        Err(_) => "unreadable".to_owned(),
    }
}

fn check(problems: &mut Vec<String>, name: &str, key: &str, want: String, got: String) {
    if want != got {
        problems.push(format!("{name}: {key} want {want} got {got}"));
    }
}

/// Compares one `embedded-thumbnail-read` case with what the reader returned. Mirrors the oracle's
/// `EmbeddedThumbnailRead` output: the hash covers each full row of the Pbgra32 buffer, in order.
fn compare_read_case(case: &Value, actual: Option<&DecodedImage>) -> Vec<String> {
    let name = str_of(case, "name");
    let expected = case.get("result").filter(|v| !v.is_null());
    let (expected, image) = match (expected, actual) {
        (None, None) => return Vec::new(),
        (None, Some(image)) => {
            return vec![format!(
                "{name}: expected None, got an image {}x{}",
                image.pixel_width(),
                image.pixel_height()
            )];
        }
        (Some(_), None) => return vec![format!("{name}: expected an image, got None")],
        (Some(expected), Some(image)) => (expected, image),
    };
    let mut problems = Vec::new();
    check(
        &mut problems,
        name,
        "width",
        i64_of(expected, "width").to_string(),
        image.pixel_width().to_string(),
    );
    check(
        &mut problems,
        name,
        "height",
        i64_of(expected, "height").to_string(),
        image.pixel_height().to_string(),
    );
    check(
        &mut problems,
        name,
        "estimated_bytes",
        i64_of(expected, "estimated_bytes").to_string(),
        image.estimated_bytes().to_string(),
    );
    check(
        &mut problems,
        name,
        "downscaled",
        bool_of(expected, "downscaled").to_string(),
        image.downscaled().to_string(),
    );
    check(
        &mut problems,
        name,
        "orientation",
        i64_of(expected, "orientation").to_string(),
        image.orientation().to_string(),
    );
    check(
        &mut problems,
        name,
        "original_width",
        i64_of(expected, "original_width").to_string(),
        image.original_width().to_string(),
    );
    check(
        &mut problems,
        name,
        "original_height",
        i64_of(expected, "original_height").to_string(),
        image.original_height().to_string(),
    );
    check(
        &mut problems,
        name,
        "backend",
        str_of(expected, "backend").to_owned(),
        format!("{:?}", image.actual_backend()),
    );

    let Some(px) = image.platform_image().as_pixels() else {
        problems.push(format!("{name}: platform image is not a PixelBuffer"));
        return problems;
    };
    check(
        &mut problems,
        name,
        "layout",
        str_of(expected, "layout").to_owned(),
        format!("{:?}", px.layout()),
    );

    let mut all_rows = Vec::new();
    for y in 0..px.height() {
        match px.get_row(y) {
            Ok(row) => all_rows.extend_from_slice(row),
            Err(_) => {
                problems.push(format!("{name}: row {y} is not readable"));
                return problems;
            }
        }
    }
    check(
        &mut problems,
        name,
        "pixels_sha256",
        str_of(expected, "pixels_sha256").to_owned(),
        hex(&sha256(&all_rows)),
    );

    let (w, h) = (px.width(), px.height());
    let corners = [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)];
    let want_corners = expected["corners"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    for (index, ((x, y), want)) in corners.iter().zip(want_corners).enumerate() {
        check(
            &mut problems,
            name,
            &format!("corner {index}"),
            want.as_str().unwrap_or("").to_owned(),
            sample(px, *x, *y),
        );
    }
    check(
        &mut problems,
        name,
        "center",
        str_of(expected, "center").to_owned(),
        sample(px, w / 2, h / 2),
    );
    problems
}

#[test]
#[ignore = "native: needs WIC"]
fn embedded_thumbnail_read_golden_native() {
    // golden: embedded-thumbnail-read.json (38 cases over tests/fixtures/embedded-thumbnail, native).
    // C#: oracle command embedded-thumbnail-read (EmbeddedThumbnailRead, TryRead with PixelBufferImageCodec).
    let golden = golden::load("embedded-thumbnail-read");
    let mut problems = Vec::new();
    for case in &golden.cases {
        let kind = case.get("kind").and_then(Value::as_str).unwrap_or("file");
        let path = match kind {
            "directory" => fixture_dir(),
            "missing" => fixture_dir().join(format!("does-not-exist-{}", str_of(case, "file"))),
            _ => {
                let file = str_of(case, "file");
                let _ = fixture_bytes(file);
                fixture_dir().join(file)
            }
        };
        let image = read_native(&path);
        problems.extend(compare_read_case(case, image.as_ref()));
    }
    assert!(
        problems.is_empty(),
        "{} golden check(s) differ across {} cases:\n{}",
        problems.len(),
        golden.cases.len(),
        problems.join("\n")
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Native twins on the fixtures (`#[ignore = "native: needs WIC"]`)
// ---------------------------------------------------------------------------------------------------------------------

/// Copies a committed fixture into a scratch folder (native tests never write into `tests/fixtures`).
fn copy_fixture(root: &TempRoot, fixture: &str, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, fixture_bytes(fixture)).unwrap_or_else(|e| panic!("write {name}: {e}"));
    path
}

#[test]
#[ignore = "native: needs WIC"]
#[cfg(windows)]
fn shared_writer_open_still_reads() {
    // C#: EmbeddedThumbnailReaderGapTests.TryRead_SharedWriterOpen_StillReads (holder: ReadWrite | Delete sharing)
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;
    const FILE_SHARE_DELETE: u32 = 0x4;
    let root = TempRoot::new("thumb-shared");
    let path = copy_fixture(&root, "thumb-main480-thumb160.jpg", "shared.jpg");
    let _holder = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&path)
        .unwrap_or_else(|e| panic!("open holder: {e}"));
    let image = read_native(&path).unwrap_or_else(|| panic!("a shared writer must not hide the thumbnail"));
    assert_eq!(image.pixel_width(), 160);
}

#[test]
#[ignore = "native: needs WIC"]
#[cfg(windows)]
fn writer_with_delete_share_still_reads() {
    // C#: EmbeddedThumbnailReaderGapTests.TryRead_WriterWithDeleteShare_StillReads (holder: ReadWrite sharing only)
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;
    let root = TempRoot::new("thumb-shared2");
    let path = copy_fixture(&root, "thumb-main480-thumb160.jpg", "shared2.jpg");
    let _holder = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&path)
        .unwrap_or_else(|e| panic!("open holder: {e}"));
    assert!(
        read_native(&path).is_some(),
        "a writer without delete sharing must not hide the thumbnail"
    );
}

#[test]
#[ignore = "native: needs WIC"]
fn huge_claimed_thumbnail_20000_native_is_none() {
    // C#: EmbeddedThumbnailReaderTests.HugeClaimedThumbnail_ReturnsNull_WithoutBigAllocation (20000 row)
    let root = TempRoot::new("thumb-bomb20");
    let path = copy_fixture(&root, "bomb-claimed-20000.jpg", "bomb-claimed-20000.jpg");
    assert!(read_native(&path).is_none());
}

#[test]
#[ignore = "native: needs WIC"]
fn huge_claimed_thumbnail_65535_native_is_none() {
    // C#: EmbeddedThumbnailReaderTests.HugeClaimedThumbnail_ReturnsNull_WithoutBigAllocation (65535 row)
    let root = TempRoot::new("thumb-bomb65");
    let path = copy_fixture(&root, "bomb-claimed-65535.jpg", "bomb-claimed-65535.jpg");
    assert!(read_native(&path).is_none());
}

#[test]
#[ignore = "native: needs WIC"]
fn oversized_valid_thumbnail_1100_native_is_none() {
    // C#: EmbeddedThumbnailReaderTests.OversizedValidThumbnail_ReturnsNull_WithoutBigAllocation.
    // Native only (contract): the 1100 px thumbnail passes the 4 MP gate, WIC itself refuses it (golden: null).
    let root = TempRoot::new("thumb-1100");
    let path = copy_fixture(&root, "real-thumb-1100.jpg", "real-thumb-1100.jpg");
    assert!(read_native(&path).is_none());
}

#[test]
#[ignore = "native: needs WIC"]
fn hand_built_thumbnail_160_native_is_readable() {
    // C#: EmbeddedThumbnailReaderTests.HandBuiltThumbnailFixture_IsReadable
    let root = TempRoot::new("thumb-160");
    let path = copy_fixture(&root, "real-thumb-160.jpg", "real-thumb-160.jpg");
    let image = read_native(&path).unwrap_or_else(|| panic!("the hand-built fixture must be readable"));
    assert_eq!(image.pixel_width(), 160);
}

#[test]
#[ignore = "native: needs WIC"]
fn normal_thumbnail_native_still_decodes() {
    // C#: EmbeddedThumbnailReaderTests.NormalThumbnail_StillDecodes
    let root = TempRoot::new("thumb-normal");
    let path = copy_fixture(&root, "thumb-main480-thumb160.jpg", "ok.jpg");
    let image = read_native(&path).unwrap_or_else(|| panic!("a 160 px thumbnail must decode"));
    assert_eq!(image.pixel_width(), 160);
}

#[test]
#[ignore = "native: needs WIC"]
fn no_orientation_native_is_pbgra32() {
    // C#: DecodingHelpersMutationTests.TryRead_Thumbnail_IsConvertedToCorrectBgraPixels (native twin)
    let root = TempRoot::new("thumb-noorient");
    let path = copy_fixture(&root, "quad-no-orientation.jpg", "quad-no-orientation.jpg");
    let image = read_native(&path).unwrap_or_else(|| panic!("the quad thumbnail must decode"));
    assert_eq!((image.pixel_width(), image.pixel_height()), (16, 8));
    assert_eq!(bgra_at(&image, 0, 0)[3], 255, "A is 255 for a JPEG thumbnail");
}

#[test]
#[ignore = "native: needs WIC"]
fn rotated_source_native_applies_orientation() {
    // C#: DecodingHelpersMutationTests.TryRead_RotatedSource_AppliesTheOrientationAndSwapsTheOriginalSize (native twin)
    let root = TempRoot::new("thumb-rot");
    let path = copy_fixture(&root, "quad-orientation-6.jpg", "quad-orientation-6.jpg");
    let image = read_native(&path).unwrap_or_else(|| panic!("the orientation-6 thumbnail must decode"));
    assert_eq!((image.pixel_width(), image.pixel_height()), (8, 16));
    assert_eq!(image.orientation(), 6);
    assert_eq!((image.original_width(), image.original_height()), (32, 48));
    assert!(image.downscaled());
}

#[test]
#[ignore = "native: needs WIC"]
fn no_thumbnail_in_plain_jpeg_native_is_none() {
    // C#: DecodingHelpersMutationTests.TryRead_NoThumbnailInTheFile_IsNull (native twin on plain-main64.jpg)
    assert!(read_native(&fixture_dir().join("plain-main64.jpg")).is_none());
}

#[test]
#[ignore = "native: needs WIC"]
fn sources_without_a_thumbnail_native_are_none() {
    // C#: EmbeddedThumbnailReaderWicParityTests.TryRead_SourcesWithoutAThumbnail_AreNullLikeBefore (PNG and plain JPEG)
    for name in ["not-jpeg.png", "plain-main64.jpg"] {
        assert!(read_native(&fixture_dir().join(name)).is_none(), "{name} must be None");
    }
}

#[test]
#[ignore = "native: needs WIC"]
fn missing_file_native_is_none() {
    // C#: DecodingHelpersMutationTests.TryRead_MissingFile_IsNull (native twin)
    let root = TempRoot::new("thumb-missing");
    assert!(read_native(&root.join("missing.jpg")).is_none());
}

#[test]
#[ignore = "native: needs WIC"]
fn path_of_a_directory_native_is_none() {
    // C#: DecodingHelpersMutationTests.TryRead_PathOfADirectory_IsNull (native twin on the fixture folder)
    assert!(read_native(&fixture_dir()).is_none());
}

#[test]
#[ignore = "native: needs WIC"]
fn file_that_is_not_an_image_native_is_none() {
    // C#: DecodingHelpersMutationTests.TryRead_FileThatIsNotAnImage_IsNull (native twin on the fixtures)
    for name in ["garbage.jpg", "text.jpg", "empty.jpg"] {
        assert!(read_native(&fixture_dir().join(name)).is_none(), "{name} must be None");
    }
}

#[test]
#[ignore = "native: needs WIC"]
fn every_truncation_of_a_thumbnail_jpeg_native_never_panics() {
    // C#: DecodingHelpersMutationTests.TryRead_EveryTruncationOfAThumbnailJpeg_NeverThrows (native, on the fixture)
    // Any Ok(_) is acceptable here (length 1452 is still readable in the golden); a panic fails the test.
    let full = fixture_bytes("quad-orientation-6.jpg");
    let root = TempRoot::new("thumb-cut");
    let path = root.join("cut.jpg");
    for length in truncation_lengths(full.len()) {
        std::fs::write(&path, &full[..length]).unwrap_or_else(|e| panic!("write cut: {e}"));
        let image = read_native(&path);
        if length == 0 {
            assert!(image.is_none(), "length 0 must be None");
        }
    }
}

#[test]
#[ignore = "native: needs WIC"]
fn mutated_app1_never_throws_native() {
    // C#: Robustness/EmbeddedThumbnailMutationTests.MutatedApp1_NeverThrows (D42). 250 mutants of the APP1 segment
    // (1..3 byte writes inside it, plus a JpegBytes.Mutate pass every fifth mutant). A panic fails the test.
    let seed = fixture_bytes("quad-orientation-6.jpg");
    let (app1_start, app1_end) = app1_range(&seed);
    assert!(
        app1_end > app1_start,
        "the fixture must carry an APP1 with the thumbnail"
    );
    let root = TempRoot::new("thumb-mutant");
    let path = root.join("t.jpg");
    let mut rng = Lcg(31);
    let mut found = 0;
    for i in 0..250 {
        let mut mutant = seed.clone();
        let flips = 1 + rng.below(3);
        for _ in 0..flips {
            let at = app1_start + 4 + rng.below(app1_end - app1_start - 4);
            mutant[at] = rng.byte();
        }
        if i % 5 == 0 {
            mutant = mutate(&mut rng, &mutant);
        }
        std::fs::write(&path, &mutant).unwrap_or_else(|e| panic!("write mutant {i}: {e}"));
        if let Some(image) = read_native(&path) {
            found += 1;
            assert!(image.pixel_width() > 0 && image.pixel_height() > 0, "mutant {i}");
        }
    }
    assert!(found > 25, "only {found} mutants still yielded a thumbnail");
}
