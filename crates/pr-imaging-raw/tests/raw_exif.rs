//! Red tests for W5-11c (`raw_exif.rs`): `RawExif.TryReadExif`, the EXIF summary merged from the EXIF blocks a RAW container
//! reader found (C# baseline d92d6aae, `src/PhotoReview.Imaging.Raw/RawExif.cs`). Ports `RawExifTests`, `Cr3ExifMergeTests`,
//! `RawExifMergeGapTests`, `OrfExifMagicTests` and the `RawExif` section of `RawHeaderHostileInputTests`, plus the golden
//! drivers over `tests/golden/raw-exif.json` (82 cases) and `tests/golden/raw-exif-corpus.json`.
//!
//! Every ported test has a `// C#: <Class>.<Method>` comment naming the C# test. Stub bodies are `todo!()`, so every test
//! that reaches `try_read_exif` fails until the implementation lands.
//!
//! Documented skips (contract W5-11c):
//! - `Cr3ExifMergeTests.TryParseTiffBlock_IfdIsExif_ReadsExposureFromIfd0` (W4-03b `ExifParser`), and
//!   `OrfExifMagicTests.TryParseJpeg_ExifBlockWithOrfMagic_StaysStrict` (W4-03b): not this unit. The golden case
//!   `orf-magic-inside-jpeg-block-stays-strict` covers the same bytes through `RawExif`.
//! - The `InMemoryRawHeaderSource_*`, `SourceRawHeaderSource_*`, `TiffStructure_*` and `SelectPreview_*` tests of
//!   `RawHeaderHostileInputTests` (W5-01b, W5-12, W4-03a, W5-11b).
//! - Corpus tests (`CorpusSamples_ExifSummaryMatchesSurvey`, `CorpusCr3_*`, `CorpusOrfFiles_ExposeExif`) are not `#[ignore]`:
//!   they return early when `tests/fixtures/raw-corpus` is absent (P-30: the RAW corpus is not downloaded).
//! - `mutation_checks` (contract item 5) is documented at the end of this file, not run in CI.
//!
//! Local helpers: the C# `ExifTestData` builder is ported in the `exif_data` module below (the `pr-imaging` test builder is
//! private to its own test file). The TIFF byte builder and the synthetic RAW builders come from `pr_test_support::raw`.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use pr_core::sync::CancelToken;
use pr_imaging::exif_parser as ep;
use pr_imaging::exif_summary::{ExifRational, ExifSummary};
use pr_imaging_raw::header_source::SourceRawHeaderSource;
use pr_imaging_raw::raw_exif::{RawExifRead, try_read_exif};
use pr_imaging_raw::readers::cr3::Cr3ContainerReader;
use pr_imaging_raw::readers::orf::OrfContainerReader;
use pr_imaging_raw::registry::RawContainerReaderRegistry;
use pr_imaging_raw::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, InMemoryRawHeaderSource, PreviewColorSpace, RawContainerInfo,
    RawContainerReader, RawFormat, RawHeaderError, RawHeaderSource, is_raw_extension,
};
use pr_test_support::golden::{self, bool_of, i64_of, repo_root, str_of};
use pr_test_support::raw::{TiffBytes, XorShift64, at, build_tiff, create_minimal_jpeg, from_hex, short};
use serde_json::{Value, json};

// ---------------------------------------------------------------- C# ExifTestData (local port)

/// C# `ExifTestData`: hand-built EXIF/TIFF blocks in either byte order. Mirrors the `pr-imaging` builder that
/// `tests/exif_parser.rs` owns (it is private to that test file).
mod exif_data {
    use super::{ExifRational, ExifSummary, ep};

    /// One IFD entry with its value bytes (already in the file's byte order).
    #[derive(Clone, Debug)]
    pub struct Entry {
        pub tag: u16,
        pub typ: u16,
        pub count: u32,
        pub value: Vec<u8>,
    }

    pub fn ascii(tag: u16, text: &str) -> Entry {
        let mut value = text.as_bytes().to_vec();
        value.push(0);
        Entry {
            tag,
            typ: 2,
            count: u32::try_from(value.len()).expect("short test string"),
            value,
        }
    }

    pub fn short_le(tag: u16, value: u16, little: bool) -> Entry {
        Entry {
            tag,
            typ: 3,
            count: 1,
            value: u16_bytes(value, little),
        }
    }

    pub fn rational(tag: u16, numerator: u32, denominator: u32, little: bool) -> Entry {
        let mut value = u32_bytes(numerator, little);
        value.extend(u32_bytes(denominator, little));
        Entry {
            tag,
            typ: 5,
            count: 1,
            value,
        }
    }

    fn u16_bytes(value: u16, little: bool) -> Vec<u8> {
        if little {
            value.to_le_bytes().to_vec()
        } else {
            value.to_be_bytes().to_vec()
        }
    }

    fn u32_bytes(value: u32, little: bool) -> Vec<u8> {
        if little {
            value.to_le_bytes().to_vec()
        } else {
            value.to_be_bytes().to_vec()
        }
    }

    /// TIFF block: header, IFD0 (plus an Exif-IFD pointer when `exif` is non-empty), the Exif IFD, then the out-of-line
    /// values. Offsets are relative to the block start, as in a real APP1 segment.
    pub fn tiff(little: bool, ifd0: &[Entry], exif: &[Entry]) -> Vec<u8> {
        let mut ifd0_entries: Vec<Entry> = ifd0.to_vec();
        let has_exif = !exif.is_empty();
        if has_exif {
            ifd0_entries.push(Entry {
                tag: ep::TAG_EXIF_IFD,
                typ: 4,
                count: 1,
                value: vec![0; 4],
            });
        }
        let ifd0_offset = 8usize;
        let ifd0_size = 2 + ifd0_entries.len() * 12 + 4;
        let exif_offset = ifd0_offset + ifd0_size;
        let exif_size = if has_exif { 2 + exif.len() * 12 + 4 } else { 0 };
        let data_offset = exif_offset + exif_size;

        let mut output: Vec<u8> = Vec::new();
        let mut data: Vec<u8> = Vec::new();
        output.extend_from_slice(if little { b"II" } else { b"MM" });
        output.extend(u16_bytes(42, little));
        output.extend(u32_bytes(u32::try_from(ifd0_offset).expect("offset"), little));
        write_ifd(&mut output, &mut data, &ifd0_entries, little, exif_offset, data_offset);
        if has_exif {
            write_ifd(&mut output, &mut data, exif, little, exif_offset, data_offset);
        }
        output.extend_from_slice(&data);
        output
    }

    fn write_ifd(
        output: &mut Vec<u8>,
        data: &mut Vec<u8>,
        entries: &[Entry],
        little: bool,
        exif_offset: usize,
        data_offset: usize,
    ) {
        output.extend(u16_bytes(u16::try_from(entries.len()).expect("entries"), little));
        for entry in entries {
            let value = if entry.tag == ep::TAG_EXIF_IFD {
                u32_bytes(u32::try_from(exif_offset).expect("offset"), little)
            } else {
                entry.value.clone()
            };
            output.extend(u16_bytes(entry.tag, little));
            output.extend(u16_bytes(entry.typ, little));
            output.extend(u32_bytes(entry.count, little));
            if value.len() <= 4 {
                output.extend_from_slice(&value);
                output.resize(output.len() + 4 - value.len(), 0);
            } else {
                let at = u32::try_from(data_offset + data.len()).expect("offset");
                output.extend(u32_bytes(at, little));
                data.extend_from_slice(&value);
                if data.len() % 2 == 1 {
                    data.push(0);
                }
            }
        }
        output.extend(u32_bytes(0, little)); // no next IFD
    }

    /// C# `ExifTestData.FullCamera`: the camera the "full" fixtures describe.
    pub fn full_camera(little: bool) -> (Vec<Entry>, Vec<Entry>) {
        (
            vec![
                ascii(ep::TAG_MAKE, "Canon"),
                ascii(ep::TAG_MODEL, "Canon EOS R5"),
                ascii(ep::TAG_DATE_TIME, "2024:06:30 09:00:00"),
            ],
            vec![
                rational(ep::TAG_EXPOSURE_TIME, 1, 250, little),
                rational(ep::TAG_F_NUMBER, 28, 10, little),
                short_le(ep::TAG_ISO, 400, little),
                ascii(ep::TAG_DATE_TIME_ORIGINAL, "2024:05:01 14:03:22"),
                rational(ep::TAG_FOCAL_LENGTH, 50, 1, little),
                ascii(ep::TAG_LENS_MODEL, "RF24-70mm F2.8 L IS USM"),
            ],
        )
    }

    /// `date_taken` as the `yyyy-MM-ddTHH:mm:ss` text the oracle prints (the `Z` of `to_json` removed).
    pub fn date_text(summary: &ExifSummary) -> Option<String> {
        summary
            .date_taken
            .map(|date| date.to_json().trim_end_matches('Z').to_owned())
    }

    /// C# `ExifTestData.AssertFullCamera`.
    pub fn assert_full_camera(summary: Option<&ExifSummary>) {
        let summary = summary.expect("the full camera EXIF must merge into a summary");
        assert_eq!(summary.camera_make.as_deref(), Some("Canon"));
        assert_eq!(summary.camera_model.as_deref(), Some("Canon EOS R5"));
        // DateTimeOriginal wins over DateTime
        assert_eq!(date_text(summary).as_deref(), Some("2024-05-01T14:03:22"));
        assert_eq!(summary.lens_model.as_deref(), Some("RF24-70mm F2.8 L IS USM"));
        assert_eq!(summary.iso, Some(400));
        assert_eq!(summary.focal_length, Some(ExifRational::new(50, 1)));
        assert_eq!(summary.f_number, Some(ExifRational::new(28, 10)));
        assert_eq!(summary.exposure_time, Some(ExifRational::new(1, 250)));
    }
}

// ---------------------------------------------------------------- shared helpers

/// `RawContainerInfo` with the given format and EXIF blocks (no previews), the shape every RawExif test passes.
fn info_with(format: RawFormat, blocks: Vec<ExifBlock>) -> RawContainerInfo {
    RawContainerInfo {
        format,
        sensor_width: 100,
        sensor_height: 100,
        orientation: 1,
        previews: Vec::new(),
        exif_blocks: blocks,
    }
}

fn cr3_info(blocks: Vec<ExifBlock>) -> RawContainerInfo {
    info_with(RawFormat::Cr3, blocks)
}

/// `new ExifBlock(offset, length, isTiffHeader)`.
fn tiff_block(offset: i64, length: i64) -> ExifBlock {
    ExifBlock::new(offset, length, true)
}

/// `new ExifBlock(offset, length, isTiffHeader: true, ifdIsExif)`.
fn exif_ifd_block(offset: i64, length: i64, ifd_is_exif: bool) -> ExifBlock {
    ExifBlock::with_ifd_is_exif(offset, length, true, ifd_is_exif)
}

fn len_i64(bytes: &[u8]) -> i64 {
    i64::try_from(bytes.len()).expect("test buffer fits i64")
}

/// `TryReadExif` with a fresh `complete` flag, the call every RawExif test makes.
fn read_exif(source: &mut dyn RawHeaderSource, info: &RawContainerInfo) -> RawExifRead {
    try_read_exif(source, info)
}

/// `date_taken` text of a summary, as the golden files write it.
fn date_text(summary: &ExifSummary) -> Option<String> {
    exif_data::date_text(summary)
}

/// C# `TiffBytes` camera block: IFD0 { Make "Canon", Model "Canon EOS", DateTime 2019:05:06 07:08:09 }.
fn camera_tiff() -> Vec<u8> {
    TiffBytes::new(true, 256)
        .header(8)
        .ifd(
            8,
            0,
            &[at(0x010F, 2, 6, 100), at(0x0110, 2, 9, 110), at(0x0132, 2, 20, 130)],
        )
        .put(100, b"Canon\0")
        .put(110, b"Canon EOS\0")
        .put(130, b"2019:05:06 07:08:09\0")
        .into_vec()
}

/// C# `Cr3ExifMergeTests.BuildExposureTiff`: 512-byte TIFF; IFD0 at 8 with ISO 800, f/2.8 (28/10), 1/250, focal 50,
/// lens name, DateTimeOriginal 2020:01:02 03:04:05. IFD0 is read as the Exif IFD in the CR3 "CMT2" block.
fn exposure_tiff() -> Vec<u8> {
    TiffBytes::new(true, 512)
        .header(8)
        .ifd(
            8,
            0,
            &[
                at(0x829A, 5, 1, 200),
                at(0x829D, 5, 1, 208),
                short(0x8827, 800),
                at(0x9003, 2, 20, 216),
                at(0x920A, 5, 1, 240),
                at(0xA434, 2, 8, 250),
            ],
        )
        .u32(200, 1)
        .u32(204, 250)
        .u32(208, 28)
        .u32(212, 10)
        .u32(240, 50)
        .u32(244, 1)
        .put(216, b"2020:01:02 03:04:05\0")
        .put(250, b"RF50mm\0\0")
        .into_vec()
}

/// C# `Cr3ExifMergeTests.BuildDateOnlyExifTiff`: IFD0 holds only DateTimeOriginal, read as the Exif IFD.
fn date_only_exif_tiff(date: &str) -> Vec<u8> {
    TiffBytes::new(true, 128)
        .header(8)
        .ifd(8, 0, &[at(0x9003, 2, 20, 100)])
        .put(100, format!("{date}\0").as_bytes())
        .into_vec()
}

/// C# `Cr3ExifMergeTests.Layout`: CMT1 (camera) followed by CMT2 (exposure); the second block is flagged `ifd_is_exif`
/// when `cmt2_is_exif` is set.
fn cr3_layout(cmt2_is_exif: bool) -> (InMemoryRawHeaderSource, RawContainerInfo) {
    let cmt1 = camera_tiff();
    let cmt2 = exposure_tiff();
    let file: Vec<u8> = [cmt1.as_slice(), cmt2.as_slice()].concat();
    let info = cr3_info(vec![
        exif_ifd_block(0, len_i64(&cmt1), false),
        exif_ifd_block(len_i64(&cmt1), len_i64(&cmt2), cmt2_is_exif),
    ]);
    (InMemoryRawHeaderSource::new(file), info)
}

/// C# `RawExifMergeGapTests.FullCameraTiff`: the full camera with only the entries `keep` accepts.
fn full_camera_tiff(keep: impl Fn(u16) -> bool) -> Vec<u8> {
    let (ifd0, exif) = exif_data::full_camera(true);
    let ifd0: Vec<exif_data::Entry> = ifd0.into_iter().filter(|e| keep(e.tag)).collect();
    let exif: Vec<exif_data::Entry> = exif.into_iter().filter(|e| keep(e.tag)).collect();
    exif_data::tiff(true, &ifd0, &exif)
}

/// C# `RawExifMergeGapTests.Blocks`: two blocks back to back, the second optionally flagged `ifd_is_exif`.
fn two_blocks(first: &[u8], second: &[u8], second_is_exif: bool) -> (InMemoryRawHeaderSource, RawContainerInfo) {
    let file: Vec<u8> = [first, second].concat();
    let info = cr3_info(vec![
        tiff_block(0, len_i64(first)),
        exif_ifd_block(len_i64(first), len_i64(second), second_is_exif),
    ]);
    (InMemoryRawHeaderSource::new(file), info)
}

/// C# `RawExifMergeGapTests` field-to-tag mapping (`TagsOf`).
fn field_tags() -> [(&'static str, Vec<u16>); 8] {
    [
        ("date", vec![ep::TAG_DATE_TIME, ep::TAG_DATE_TIME_ORIGINAL]),
        ("make", vec![ep::TAG_MAKE]),
        ("model", vec![ep::TAG_MODEL]),
        ("lens", vec![ep::TAG_LENS_MODEL]),
        ("iso", vec![ep::TAG_ISO]),
        ("focal", vec![ep::TAG_FOCAL_LENGTH]),
        ("fnumber", vec![ep::TAG_F_NUMBER]),
        ("exposure", vec![ep::TAG_EXPOSURE_TIME]),
    ]
}

/// C# `RawHeaderHostileInputTests.MakeTiff`: minimal TIFF whose IFD0 holds one inline-or-out-of-line Make.
fn make_tiff(make: &str) -> Vec<u8> {
    exif_data::tiff(true, &[exif_data::ascii(ep::TAG_MAKE, make)], &[])
}

/// Source whose every read fails (C# `ThrowingSource`: length 1024, `InvalidDataException` on each read).
struct ThrowingSource;

impl RawHeaderSource for ThrowingSource {
    fn length(&self) -> i64 {
        1024
    }

    fn read(&mut self, _offset: i64, _count: i32) -> Result<&[u8], RawHeaderError> {
        Err(RawHeaderError::invalid_data("Header read exceeded hard limit."))
    }
}

/// C# `FailFirstReadSource`: the first read fails, later reads delegate to the inner source.
struct FailFirstReadSource<S: RawHeaderSource> {
    inner: S,
    failed: bool,
}

impl<S: RawHeaderSource> RawHeaderSource for FailFirstReadSource<S> {
    fn length(&self) -> i64 {
        self.inner.length()
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        if !self.failed {
            self.failed = true;
            return Err(RawHeaderError::invalid_data("Simulated failure."));
        }
        self.inner.read(offset, count)
    }
}

/// Golden driver source: wraps an `InMemoryRawHeaderSource`, logs every `(offset, count)` call in order (failed calls
/// included) and fails the 0-based call indexes in `fail_reads` without touching the inner source.
struct RecordingSource {
    inner: InMemoryRawHeaderSource,
    reads: Vec<(i64, i32)>,
    fail_reads: Vec<usize>,
}

impl RawHeaderSource for RecordingSource {
    fn length(&self) -> i64 {
        self.inner.length()
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let index = self.reads.len();
        self.reads.push((offset, count));
        if self.fail_reads.contains(&index) {
            return Err(RawHeaderError::invalid_data("golden fail_reads"));
        }
        self.inner.read(offset, count)
    }
}

/// Parses a golden JSON file that is not a `pr_test_support::golden::load` file (`load` rejects empty `cases`).
fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// `tests/fixtures/raw-corpus` when present; `None` skips the corpus tests (P-30: the corpus is not downloaded).
fn raw_corpus_dir() -> Option<PathBuf> {
    let dir = repo_root().join("tests").join("fixtures").join("raw-corpus");
    dir.is_dir().then_some(dir)
}

/// Every corpus file with a RAW extension, sorted; empty when the corpus is absent.
fn raw_corpus_files() -> Vec<PathBuf> {
    let Some(dir) = raw_corpus_dir() else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| is_raw_extension(path.to_str()))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// `Path.GetExtension(file).ToLowerInvariant()`: the registry takes the extension with its dot.
fn ext_of(path: &Path) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    format!(".{}", ext.to_ascii_lowercase())
}

/// A `SourceRawHeaderSource` over an in-memory file (the production file-backed source, over a `Cursor`).
fn source_of_bytes(bytes: Vec<u8>) -> SourceRawHeaderSource {
    SourceRawHeaderSource::from_stream(Box::new(Cursor::new(bytes))).expect("header source over an in-memory stream")
}

/// `RawExifRead` summary as the golden files write it (`date_taken` as `yyyy-MM-ddTHH:mm:ss`, rationals as objects).
fn summary_json(summary: Option<&ExifSummary>) -> Value {
    fn rational_json(r: &ExifRational) -> Value {
        json!({ "numerator": r.numerator, "denominator": r.denominator })
    }
    match summary {
        None => Value::Null,
        Some(s) => json!({
            "date_taken": date_text(s),
            "camera_make": s.camera_make,
            "camera_model": s.camera_model,
            "lens_model": s.lens_model,
            "iso": s.iso,
            "focal_length": s.focal_length.as_ref().map(rational_json),
            "f_number": s.f_number.as_ref().map(rational_json),
            "exposure_time": s.exposure_time.as_ref().map(rational_json),
        }),
    }
}

// ---------------------------------------------------------------- RawExifTests

#[test]
fn synthetic_tiff_block_yields_exif_summary() {
    // C#: RawExifTests.SyntheticTiffBlock_YieldsExifSummary
    let jpeg = create_minimal_jpeg(640, 480);
    let tiff = build_tiff(true, Some(&jpeg), 1, 42);
    let mut source = InMemoryRawHeaderSource::new(tiff.clone());
    let info = RawContainerInfo {
        format: RawFormat::Cr2,
        sensor_width: 640,
        sensor_height: 480,
        orientation: 1,
        previews: vec![EmbeddedPreview::new(
            0,
            0,
            len_i64(&jpeg),
            EmbeddedPreviewKind::Jpeg,
            640,
            480,
            PreviewColorSpace::Unknown,
        )],
        exif_blocks: vec![tiff_block(0, len_i64(&tiff))],
    };

    let read = read_exif(&mut source, &info);

    // Synthetic builder does not add Make/Model strings; parsing must succeed without throwing.
    assert!(read.summary.as_ref().is_none_or(|s| !s.is_empty()));
}

#[test]
fn corpus_samples_exif_summary_matches_survey() {
    // C#: RawExifTests.CorpusSamples_ExifSummaryMatchesSurvey ([Trait Native]; the corpus is required)
    if raw_corpus_dir().is_none() {
        return;
    }
    let registry = RawContainerReaderRegistry::new();
    let mut exif_count = 0usize;

    for path in raw_corpus_files() {
        let bytes = std::fs::read(&path).expect("read corpus file");
        let ext = ext_of(&path);
        let head = &bytes[..bytes.len().min(64)];
        let Some(reader) = registry.find_reader(head, &ext) else {
            continue;
        };
        let mut source = source_of_bytes(bytes.clone());
        let info = reader
            .read(&mut source, &CancelToken::new())
            .expect("reader reads a corpus file");
        let read = read_exif(&mut source, &info);

        if let Some(exif) = read.summary {
            exif_count += 1;
            if let Some(make) = &exif.camera_make {
                assert!(!make.trim().is_empty(), "{}: blank camera make", path.display());
            }
            if let Some(model) = &exif.camera_model {
                assert!(!model.trim().is_empty(), "{}: blank camera model", path.display());
            }
        }
    }

    assert!(
        exif_count >= 10,
        "expected at least 10 corpus files with EXIF, found {exif_count}"
    );
}

// ---------------------------------------------------------------- Cr3ExifMergeTests

#[test]
fn try_read_exif_cr3_style_cmt1_plus_cmt2_merges_camera_and_exposure() {
    // C#: Cr3ExifMergeTests.TryReadExif_Cr3StyleCmt1PlusCmt2_MergesCameraAndExposure
    let (mut source, info) = cr3_layout(true);

    let read = read_exif(&mut source, &info);
    let exif = read.summary.expect("CMT1 + CMT2 must merge into a summary");

    assert_eq!(exif.camera_make.as_deref(), Some("Canon"));
    assert_eq!(exif.camera_model.as_deref(), Some("Canon EOS"));
    assert_eq!(exif.iso, Some(800));
    assert_eq!(exif.f_number, Some(ExifRational::new(28, 10)));
    assert_eq!(exif.exposure_time, Some(ExifRational::new(1, 250)));
    assert_eq!(exif.focal_length, Some(ExifRational::new(50, 1)));
    assert_eq!(exif.lens_model.as_deref(), Some("RF50mm"));
    assert!(exif.date_taken.is_some());
}

#[test]
fn try_read_exif_cr3_where_cmt1_and_cmt2_dates_differ_prefers_the_capture_time_of_cmt2() {
    // C#: Cr3ExifMergeTests.TryReadExif_Cr3WhereCmt1AndCmt2DatesDiffer_PrefersTheCaptureTimeOfCmt2
    // CMT1 DateTime is 2019:05:06 (0x0132); CMT2 DateTimeOriginal is 2020:01:02 (0x9003).
    let (mut source, info) = cr3_layout(true);

    let read = read_exif(&mut source, &info);
    let exif = read.summary.expect("CMT1 + CMT2 must merge into a summary");

    assert_eq!(date_text(&exif).as_deref(), Some("2020-01-02T03:04:05"));
}

#[test]
fn try_read_exif_cmt2_without_date_keeps_the_cmt1_date() {
    // C#: Cr3ExifMergeTests.TryReadExif_Cmt2WithoutDate_KeepsTheCmt1Date
    let cmt1 = camera_tiff();
    let cmt2 = TiffBytes::new(true, 128)
        .header(8)
        .ifd(8, 0, &[short(0x8827, 100)])
        .into_vec();
    let mut source = InMemoryRawHeaderSource::new([cmt1.as_slice(), cmt2.as_slice()].concat());
    let info = cr3_info(vec![
        exif_ifd_block(0, len_i64(&cmt1), false),
        exif_ifd_block(len_i64(&cmt1), len_i64(&cmt2), true),
    ]);

    let read = read_exif(&mut source, &info);
    let exif = read.summary.expect("CMT1 must still yield a summary");

    assert_eq!(date_text(&exif).as_deref(), Some("2019-05-06T07:08:09"));
}

#[test]
fn try_read_exif_cmt2_without_ifd_is_exif_flag_does_not_read_exposure() {
    // C#: Cr3ExifMergeTests.TryReadExif_Cmt2WithoutIfdIsExifFlag_DoesNotReadExposure
    let (mut source, info) = cr3_layout(false);

    let read = read_exif(&mut source, &info);
    let exif = read.summary.expect("CMT1 must still yield a summary");

    assert_eq!(exif.camera_make.as_deref(), Some("Canon"));
    assert_eq!(exif.iso, None);
}

#[test]
fn try_read_exif_first_block_wins_per_field() {
    // C#: Cr3ExifMergeTests.TryReadExif_FirstBlockWinsPerField
    let a = exposure_tiff();
    let b = TiffBytes::new(true, 128)
        .header(8)
        .ifd(8, 0, &[short(0x8827, 100)])
        .into_vec();
    let mut source = InMemoryRawHeaderSource::new([a.as_slice(), b.as_slice()].concat());
    let info = cr3_info(vec![
        exif_ifd_block(0, len_i64(&a), true),
        exif_ifd_block(len_i64(&a), len_i64(&b), true),
    ]);

    let read = read_exif(&mut source, &info);

    assert_eq!(read.summary.expect("summary").iso, Some(800));
}

#[test]
fn try_read_exif_two_ifd_is_exif_blocks_first_with_date_the_first_date_wins() {
    // C#: Cr3ExifMergeTests.TryReadExif_TwoIfdIsExifBlocksFirstWithDate_TheFirstDateWins
    let a = date_only_exif_tiff("2020:01:02 03:04:05");
    let b = date_only_exif_tiff("2021:06:07 08:09:10");
    let mut source = InMemoryRawHeaderSource::new([a.as_slice(), b.as_slice()].concat());
    let info = cr3_info(vec![
        exif_ifd_block(0, len_i64(&a), true),
        exif_ifd_block(len_i64(&a), len_i64(&b), true),
    ]);

    let read = read_exif(&mut source, &info);

    assert_eq!(
        date_text(&read.summary.expect("summary")).as_deref(),
        Some("2020-01-02T03:04:05")
    );
}

#[test]
fn try_read_exif_dateless_first_block_then_two_ifd_is_exif_blocks_with_dates_the_first_exif_date_wins() {
    // C#: Cr3ExifMergeTests.TryReadExif_DatelessFirstBlockThenTwoIfdIsExifBlocksWithDates_TheFirstExifDateWins
    let cam = exif_data::tiff(true, &[exif_data::ascii(ep::TAG_MAKE, "Canon")], &[]);
    let a = date_only_exif_tiff("2020:01:02 03:04:05");
    let b = date_only_exif_tiff("2021:06:07 08:09:10");
    let file = [cam.as_slice(), a.as_slice(), b.as_slice()].concat();
    let mut source = InMemoryRawHeaderSource::new(file);
    let info = cr3_info(vec![
        tiff_block(0, len_i64(&cam)),
        exif_ifd_block(len_i64(&cam), len_i64(&a), true),
        exif_ifd_block(len_i64(&cam) + len_i64(&a), len_i64(&b), true),
    ]);

    let read = read_exif(&mut source, &info);
    let exif = read.summary.expect("summary");

    assert_eq!(exif.camera_make.as_deref(), Some("Canon"));
    assert_eq!(date_text(&exif).as_deref(), Some("2020-01-02T03:04:05"));
}

#[test]
fn try_read_exif_hostile_second_block_keeps_first_summary() {
    // C#: Cr3ExifMergeTests.TryReadExif_HostileSecondBlock_KeepsFirstSummary
    let cmt1 = camera_tiff();
    let mut file = cmt1.clone();
    file.extend_from_slice(&[0u8; 64]);
    let mut source = InMemoryRawHeaderSource::new(file);
    let info = cr3_info(vec![
        tiff_block(0, len_i64(&cmt1)),
        exif_ifd_block(len_i64(&cmt1), 64, true),
    ]);

    let read = read_exif(&mut source, &info);

    assert_eq!(read.summary.expect("summary").camera_make.as_deref(), Some("Canon"));
}

/// C# `Cr3ExifMergeTests.CorpusCr3_ExifSummary_HasExposureFieldsFromCmt2` (theory row 1). Native: returns early when the
/// corpus file is absent.
#[test]
fn corpus_cr3_eos_m50_exif_summary_has_exposure_fields_from_cmt2() {
    // C#: Cr3ExifMergeTests.CorpusCr3_ExifSummary_HasExposureFieldsFromCmt2 (row "Canon - EOS M50 - CRAW (3_2).CR3")
    corpus_cr3_exif_summary(
        "Canon - EOS M50 - CRAW (3_2).CR3",
        "Canon EOS M50",
        1000,
        (4, 1),
        (1, 60),
        15,
        "EF-M15-45mm f/3.5-6.3 IS STM",
    );
}

/// C# `Cr3ExifMergeTests.CorpusCr3_ExifSummary_HasExposureFieldsFromCmt2` (theory row 2).
#[test]
fn corpus_cr3_eos_r6_exif_summary_has_exposure_fields_from_cmt2() {
    // C#: Cr3ExifMergeTests.CorpusCr3_ExifSummary_HasExposureFieldsFromCmt2 (row "Canon - EOS R6 - 3_2.CR3")
    corpus_cr3_exif_summary(
        "Canon - EOS R6 - 3_2.CR3",
        "Canon EOS R6",
        100,
        (25, 1),
        (1, 4),
        200,
        "RF70-200mm F2.8 L IS USM",
    );
}

/// Shared body of the two CR3 corpus rows; returns early when the corpus file is absent (C# `RawCorpus.TryGetFile`).
fn corpus_cr3_exif_summary(
    file: &str,
    model: &str,
    iso: i32,
    f_number: (u32, u32),
    exposure: (u32, u32),
    focal: u32,
    lens: &str,
) {
    let Some(path) = raw_corpus_dir().map(|dir| dir.join(file)) else {
        return;
    };
    if !path.is_file() {
        return;
    }
    let bytes = std::fs::read(&path).expect("read CR3 corpus file");
    let mut source = source_of_bytes(bytes);
    let info = Cr3ContainerReader
        .read(&mut source, &CancelToken::new())
        .expect("CR3 reader reads the corpus file");

    let read = read_exif(&mut source, &info);
    let exif = read.summary.expect("CR3 corpus file must yield EXIF");

    assert_eq!(exif.camera_make.as_deref(), Some("Canon"));
    assert_eq!(exif.camera_model.as_deref(), Some(model));
    assert_eq!(exif.iso, Some(iso));
    assert_eq!(exif.f_number, Some(ExifRational::new(f_number.0, f_number.1)));
    assert_eq!(exif.exposure_time, Some(ExifRational::new(exposure.0, exposure.1)));
    assert_eq!(exif.focal_length, Some(ExifRational::new(focal, 1)));
    assert_eq!(exif.lens_model.as_deref(), Some(lens));
    assert!(exif.date_taken.is_some());
}

// ---------------------------------------------------------------- RawExifMergeGapTests

#[test]
fn try_read_exif_field_missing_from_the_first_block_is_taken_from_the_second() {
    // C#: RawExifMergeGapTests.TryReadExif_FieldMissingFromTheFirstBlock_IsTakenFromTheSecond (theory, 8 rows)
    for (field, tags) in field_tags() {
        let first = full_camera_tiff(|tag| !tags.contains(&tag));
        let second = full_camera_tiff(|tag| tags.contains(&tag));
        let (mut source, info) = two_blocks(&first, &second, false);

        let read = read_exif(&mut source, &info);

        assert!(read.complete, "field {field}: complete must stay true");
        exif_data::assert_full_camera(read.summary.as_ref());
    }
}

#[test]
fn try_read_exif_both_blocks_have_every_field_but_one_missing_first_block_wins_every_field_it_has() {
    // C#: RawExifMergeGapTests.TryReadExif_BothBlocksHaveEveryFieldButOneMissing_FirstBlockWinsEveryFieldItHas
    // The first block lacks only the focal length (so the second is read); the second disagrees on everything.
    let other = exif_data::tiff(
        true,
        &[
            exif_data::ascii(ep::TAG_MAKE, "Nikon"),
            exif_data::ascii(ep::TAG_MODEL, "D850"),
            exif_data::ascii(ep::TAG_DATE_TIME, "2000:01:01 00:00:00"),
        ],
        &[
            exif_data::rational(ep::TAG_EXPOSURE_TIME, 1, 1000, true),
            exif_data::rational(ep::TAG_F_NUMBER, 56, 10, true),
            exif_data::short_le(ep::TAG_ISO, 100, true),
            exif_data::ascii(ep::TAG_DATE_TIME_ORIGINAL, "2000:01:02 00:00:00"),
            exif_data::rational(ep::TAG_FOCAL_LENGTH, 85, 1, true),
            exif_data::ascii(ep::TAG_LENS_MODEL, "other lens"),
        ],
    );
    let (mut source, info) = two_blocks(&full_camera_tiff(|tag| tag != ep::TAG_FOCAL_LENGTH), &other, false);

    let read = read_exif(&mut source, &info);
    let exif = read.summary.expect("summary");

    assert_eq!(exif.camera_make.as_deref(), Some("Canon"));
    assert_eq!(exif.camera_model.as_deref(), Some("Canon EOS R5"));
    assert_eq!(date_text(&exif).as_deref(), Some("2024-05-01T14:03:22"));
    assert_eq!(exif.lens_model.as_deref(), Some("RF24-70mm F2.8 L IS USM"));
    assert_eq!(exif.iso, Some(400));
    assert_eq!(exif.f_number, Some(ExifRational::new(28, 10)));
    assert_eq!(exif.exposure_time, Some(ExifRational::new(1, 250)));
    assert_eq!(exif.focal_length, Some(ExifRational::new(85, 1)));
}

#[test]
fn try_read_exif_first_block_already_has_every_field_later_blocks_are_not_read() {
    // C#: RawExifMergeGapTests.TryReadExif_FirstBlockAlreadyHasEveryField_LaterBlocksAreNotRead
    // A later Exif-IFD block would normally replace the date: it must not even be looked at.
    let later = exif_data::tiff(
        true,
        &[exif_data::ascii(ep::TAG_DATE_TIME_ORIGINAL, "2000:01:01 00:00:00")],
        &[],
    );
    let (mut source, info) = two_blocks(&full_camera_tiff(|_| true), &later, true);

    let read = read_exif(&mut source, &info);

    exif_data::assert_full_camera(read.summary.as_ref());
}

#[test]
fn try_read_exif_block_starting_past_the_end_of_the_file_is_skipped() {
    // C#: RawExifMergeGapTests.TryReadExif_BlockStartingPastTheEndOfTheFile_IsSkipped
    let tiff = full_camera_tiff(|_| true);
    let mut source = InMemoryRawHeaderSource::new(tiff.clone());
    let info = cr3_info(vec![tiff_block(len_i64(&tiff) + 10, 64), tiff_block(0, len_i64(&tiff))]);

    let read = read_exif(&mut source, &info);

    exif_data::assert_full_camera(read.summary.as_ref());
    assert!(read.complete);
}

#[test]
fn try_read_exif_block_with_negative_offset_is_skipped() {
    // C#: RawExifMergeGapTests.TryReadExif_BlockWithNegativeOffset_IsSkipped
    let tiff = full_camera_tiff(|_| true);
    let mut source = InMemoryRawHeaderSource::new(tiff.clone());
    let info = cr3_info(vec![tiff_block(-5, 64), tiff_block(0, len_i64(&tiff))]);

    let read = read_exif(&mut source, &info);

    exif_data::assert_full_camera(read.summary.as_ref());
    assert!(read.complete);
}

#[test]
fn try_read_exif_block_claiming_to_extend_past_the_end_of_the_file_is_read_up_to_the_end() {
    // C#: RawExifMergeGapTests.TryReadExif_BlockClaimingToExtendPastTheEndOfTheFile_IsReadUpToTheEnd
    let tiff = full_camera_tiff(|_| true);
    let mut source = InMemoryRawHeaderSource::new(tiff.clone());
    let info = cr3_info(vec![tiff_block(0, len_i64(&tiff) + 1000)]);

    let read = read_exif(&mut source, &info);

    exif_data::assert_full_camera(read.summary.as_ref());
    assert!(read.complete);
}

#[test]
fn try_read_exif_zero_length_block_is_skipped_and_nothing_is_reported() {
    // C#: RawExifMergeGapTests.TryReadExif_ZeroLengthBlock_IsSkippedAndNothingIsReported
    let tiff = full_camera_tiff(|_| true);
    let mut source = InMemoryRawHeaderSource::new(tiff);
    let info = cr3_info(vec![tiff_block(0, 0)]);

    let read = read_exif(&mut source, &info);

    assert_eq!(read.summary, None);
    assert!(read.complete);
}

#[test]
fn try_read_exif_both_blocks_have_the_focal_length_first_block_wins() {
    // C#: RawExifMergeGapTests.TryReadExif_BothBlocksHaveTheFocalLength_FirstBlockWins
    let other = exif_data::tiff(true, &[], &[exif_data::rational(ep::TAG_FOCAL_LENGTH, 85, 1, true)]);
    let (mut source, info) = two_blocks(&full_camera_tiff(|tag| tag != ep::TAG_LENS_MODEL), &other, false);

    let read = read_exif(&mut source, &info);

    assert_eq!(
        read.summary.expect("summary").focal_length,
        Some(ExifRational::new(50, 1))
    );
}

#[test]
fn try_read_exif_second_block_claiming_to_extend_past_the_end_of_the_file_is_read_up_to_the_end() {
    // C#: RawExifMergeGapTests.TryReadExif_SecondBlockClaimingToExtendPastTheEndOfTheFile_IsReadUpToTheEnd
    let first = full_camera_tiff(|tag| tag != ep::TAG_LENS_MODEL);
    let second = exif_data::tiff(true, &[], &[exif_data::ascii(ep::TAG_LENS_MODEL, "Lens X")]);
    let file = [first.as_slice(), second.as_slice()].concat();
    let mut source = InMemoryRawHeaderSource::new(file);
    let info = cr3_info(vec![
        tiff_block(0, len_i64(&first)),
        tiff_block(len_i64(&first), len_i64(&second) + 1000),
    ]);

    let read = read_exif(&mut source, &info);

    assert_eq!(read.summary.expect("summary").lens_model.as_deref(), Some("Lens X"));
    assert!(read.complete);
}

// ---------------------------------------------------------------- OrfExifMagicTests

/// C# `OrfExifMagicTests.BuildOrf`: whole-file TIFF-shaped block, IFD0 { Make "OLYMPUS", Model "E-M1" }, with the
/// signature bytes (`II`/`MM` + 2 magic bytes) written literally.
fn build_orf(little: bool, signature: &[u8; 4]) -> Vec<u8> {
    TiffBytes::new(little, 200)
        .put(0, signature)
        .u32(4, 8)
        .ifd(8, 0, &[at(0x010F, 2, 8, 100), at(0x0110, 2, 5, 120)])
        .put(100, b"OLYMPUS\0")
        .put(120, b"E-M1\0")
        .into_vec()
}

fn read_through_raw_exif(orf: &[u8]) -> Option<ExifSummary> {
    let info = info_with(RawFormat::Orf, vec![tiff_block(0, len_i64(orf))]);
    let mut source = InMemoryRawHeaderSource::new(orf.to_vec());
    read_exif(&mut source, &info).summary
}

#[test]
fn try_read_exif_iiro_block_yields_make_and_model() {
    // C#: OrfExifMagicTests.TryReadExif_IiroBlock_YieldsMakeAndModel
    let exif = read_through_raw_exif(&build_orf(true, b"IIRO")).expect("IIRO block must parse");

    assert_eq!(exif.camera_make.as_deref(), Some("OLYMPUS"));
    assert_eq!(exif.camera_model.as_deref(), Some("E-M1"));
}

#[test]
fn try_read_exif_iirs_block_yields_make_and_model() {
    // C#: OrfExifMagicTests.TryReadExif_IirsBlock_YieldsMakeAndModel
    let exif = read_through_raw_exif(&build_orf(true, b"IIRS")).expect("IIRS block must parse");

    assert_eq!(exif.camera_make.as_deref(), Some("OLYMPUS"));
}

#[test]
fn try_read_exif_big_endian_mmor_block_yields_make_and_model() {
    // C#: OrfExifMagicTests.TryReadExif_BigEndianMmorBlock_YieldsMakeAndModel
    let exif = read_through_raw_exif(&build_orf(false, b"MMOR")).expect("MMOR block must parse");

    assert_eq!(exif.camera_model.as_deref(), Some("E-M1"));
}

/// C# `OrfExifMagicTests.TryReadExif_UnknownMagic_IsRejected` (theory, 4 rows: 0x0000, 0x002B BigTIFF, 0x4F53, 0xFFFF).
#[test]
fn try_read_exif_unknown_magic_is_rejected() {
    // C#: OrfExifMagicTests.TryReadExif_UnknownMagic_IsRejected
    for magic in [0x0000u16, 0x002B, 0x4F53, 0xFFFF] {
        let signature = [b'I', b'I', (magic & 0xFF) as u8, (magic >> 8) as u8];
        let block = build_orf(true, &signature);

        assert_eq!(
            read_through_raw_exif(&block),
            None,
            "magic {magic:#06x} must be rejected"
        );
    }
}

#[test]
fn try_read_exif_hostile_orf_block_does_not_throw() {
    // C#: OrfExifMagicTests.TryReadExif_HostileOrfBlock_DoesNotThrow
    let mut block = build_orf(true, b"IIRO");
    // IFD0 offset far outside the block and an entry count larger than the data.
    block[4..8].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0x7F]);
    assert_eq!(read_through_raw_exif(&block), None);

    let mut counted = build_orf(true, b"IIRO");
    counted[8] = 0xFF;
    counted[9] = 0xFF;
    let _ = read_through_raw_exif(&counted);
}

#[test]
fn corpus_orf_files_expose_exif() {
    // C#: OrfExifMagicTests.CorpusOrfFiles_ExposeExif ([Trait Native]; the corpus is required)
    if raw_corpus_dir().is_none() {
        return;
    }
    for path in raw_corpus_files() {
        if ext_of(&path) != ".orf" {
            continue;
        }
        let bytes = std::fs::read(&path).expect("read ORF corpus file");
        let mut source = source_of_bytes(bytes);
        let info = OrfContainerReader
            .read(&mut source, &CancelToken::new())
            .expect("ORF reader reads the corpus file");

        let read = read_exif(&mut source, &info);
        let exif = read
            .summary
            .unwrap_or_else(|| panic!("{}: no EXIF read from the ORF", path.display()));
        assert!(
            exif.camera_make.as_deref().is_some_and(|make| !make.is_empty()),
            "{}: no camera make",
            path.display()
        );
    }
}

// ---------------------------------------------------------------- RawHeaderHostileInputTests (RawExif section)

#[test]
fn try_read_exif_tiff_block_claiming_to_extend_past_eof_parses_the_available_bytes() {
    // C#: RawHeaderHostileInputTests.TryReadExif_TiffBlockClaimingToExtendPastEof_ParsesTheAvailableBytes
    let tiff = make_tiff("CamA");
    let mut source = InMemoryRawHeaderSource::new(tiff.clone());
    let info = info_with(RawFormat::Cr2, vec![tiff_block(0, len_i64(&tiff) + 100_000)]);

    let read = read_exif(&mut source, &info);

    assert_eq!(read.summary.expect("summary").camera_make.as_deref(), Some("CamA"));
}

#[test]
fn try_read_exif_block_starting_near_eof_and_overlong_returns_none_without_throwing() {
    // C#: RawHeaderHostileInputTests.TryReadExif_BlockStartingNearEofAndOverlong_ReturnsNullWithoutThrowing
    let mut source = InMemoryRawHeaderSource::new(vec![0u8; 100]);
    let info = info_with(
        RawFormat::Cr2,
        vec![tiff_block(96, 5_000), ExifBlock::new(99, i64::MAX, false)],
    );

    let read = read_exif(&mut source, &info);

    assert_eq!(read.summary, None);
}

#[test]
fn try_read_exif_source_throws_invalid_data_returns_none() {
    // C#: RawHeaderHostileInputTests.TryReadExif_SourceThrowsInvalidData_ReturnsNull
    let info = info_with(RawFormat::Cr2, vec![tiff_block(0, 64)]);

    let read = read_exif(&mut ThrowingSource, &info);

    assert_eq!(read.summary, None);
    assert!(!read.complete);
}

#[test]
fn try_read_exif_failure_on_first_block_still_tries_later_blocks() {
    // C#: RawHeaderHostileInputTests.TryReadExif_FailureOnFirstBlock_StillTriesLaterBlocks
    let tiff = make_tiff("CamB");
    let mut source = FailFirstReadSource {
        inner: InMemoryRawHeaderSource::new(tiff.clone()),
        failed: false,
    };
    let info = info_with(
        RawFormat::Cr2,
        vec![tiff_block(0, len_i64(&tiff)), tiff_block(0, len_i64(&tiff))],
    );

    let read = read_exif(&mut source, &info);

    assert_eq!(read.summary.expect("summary").camera_make.as_deref(), Some("CamB"));
    assert!(
        !read.complete,
        "a failed read must clear complete even when a later block succeeds"
    );
}

// ---------------------------------------------------------------- golden drivers

/// Source bytes of one golden input: `length` zero bytes with each `parts` entry copied in at its `at`.
fn build_source_bytes(source: &Value) -> Vec<u8> {
    let length = usize::try_from(i64_of(source, "length")).expect("golden length fits usize");
    let mut bytes = vec![0u8; length];
    for part in source["parts"].as_array().expect("golden source parts") {
        let at = usize::try_from(i64_of(part, "at")).expect("golden part offset fits usize");
        let data = from_hex(str_of(part, "hex"));
        bytes[at..at + data.len()].copy_from_slice(&data);
    }
    bytes
}

fn format_of(name: &str) -> RawFormat {
    match name {
        "Cr2" => RawFormat::Cr2,
        "Cr3" => RawFormat::Cr3,
        "Nef" => RawFormat::Nef,
        "Arw" => RawFormat::Arw,
        "Dng" => RawFormat::Dng,
        "Raf" => RawFormat::Raf,
        "Orf" => RawFormat::Orf,
        "Rw2" => RawFormat::Rw2,
        other => panic!("golden format {other} is not a RawFormat"),
    }
}

/// Golden i64 that may be a JSON number or, for values beyond 2^53 (`i64::MAX`, `i64::MIN`), a decimal string.
fn wide_i64_of(case: &Value, key: &str) -> i64 {
    match case[key].as_str() {
        Some(text) => text
            .parse()
            .unwrap_or_else(|_| panic!("golden '{key}' is not an i64: {case}")),
        None => i64_of(case, key),
    }
}

fn blocks_of(blocks: &Value) -> Vec<ExifBlock> {
    blocks
        .as_array()
        .expect("golden blocks")
        .iter()
        .map(|b| {
            ExifBlock::with_ifd_is_exif(
                wide_i64_of(b, "offset"),
                wide_i64_of(b, "length"),
                bool_of(b, "is_tiff"),
                bool_of(b, "ifd_is_exif"),
            )
        })
        .collect()
}

fn fail_reads_of(input: &Value) -> Vec<usize> {
    input["fail_reads"].as_array().map_or_else(Vec::new, |indexes| {
        indexes
            .iter()
            .map(|v| usize::try_from(v.as_u64().expect("fail_reads index")).expect("index fits usize"))
            .collect()
    })
}

fn reads_of(expected: &Value) -> Vec<(i64, i32)> {
    expected["reads"]
        .as_array()
        .expect("golden reads")
        .iter()
        .map(|pair| {
            (
                pair[0].as_i64().expect("read offset"),
                i32::try_from(pair[1].as_i64().expect("read count")).expect("read count fits i32"),
            )
        })
        .collect()
}

/// Runs one `raw-exif` golden case and compares summary, complete flag, the exact read log and bytes charged.
fn run_golden_case(name: &str, input: &Value, expected: &Value) {
    assert!(
        !bool_of(expected, "threw"),
        "{name}: the C# oracle threw, which RawExif must never do"
    );
    let mut source = RecordingSource {
        inner: InMemoryRawHeaderSource::new(build_source_bytes(&input["source"])),
        reads: Vec::new(),
        fail_reads: fail_reads_of(input),
    };
    let info = info_with(format_of(str_of(input, "format")), blocks_of(&input["blocks"]));

    let read = read_exif(&mut source, &info);

    assert_eq!(
        summary_json(read.summary.as_ref()),
        expected["summary"],
        "{name}: summary"
    );
    assert_eq!(read.complete, bool_of(expected, "complete"), "{name}: complete");
    assert_eq!(source.reads, reads_of(expected), "{name}: read log");
    assert_eq!(
        source.inner.total_bytes_read(),
        i64_of(expected, "total_bytes_read"),
        "{name}: total_bytes_read"
    );
}

/// `tests/golden/raw-exif.json` (82 cases) with the inputs of `tests/golden-inputs/raw-exif.json`, matched by name.
#[test]
fn golden_raw_exif() {
    // C#: RawExif.TryReadExif (oracle command raw-exif)
    let golden = golden::load("raw-exif");
    let inputs = read_json(&repo_root().join("tests").join("golden-inputs").join("raw-exif.json"));
    let inputs = inputs["cases"].as_array().expect("golden-inputs raw-exif cases");
    assert_eq!(inputs.len(), golden.cases.len(), "input and golden case counts differ");

    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = inputs
            .iter()
            .find(|input| str_of(input, "name") == name)
            .unwrap_or_else(|| panic!("no golden input named {name}"));
        run_golden_case(name, input, case);
    }
}

/// `tests/golden/raw-exif-corpus.json`: the oracle enumerates the corpus itself. With zero entries (the wiring machine
/// had no corpus) this machine must find no RAW file either; with entries each one runs registry -> reader ->
/// `try_read_exif` on the same bytes. Entry keys (`file`, `summary`, `complete`, `bytes_read`) follow the oracle output
/// (Q4 of the contract: the strict-corpus switch is not wired yet).
#[test]
fn golden_raw_exif_corpus() {
    // C#: RawExif.TryReadExif (oracle command raw-exif-corpus)
    let doc = read_json(&repo_root().join("tests").join("golden").join("raw-exif-corpus.json"));
    let cases = doc["cases"].as_array().expect("golden raw-exif-corpus cases");
    let files = raw_corpus_files();

    if cases.is_empty() {
        assert!(
            files.is_empty(),
            "the wiring machine had no RAW corpus but this machine has {} RAW file(s)",
            files.len()
        );
        return;
    }

    let registry = RawContainerReaderRegistry::new();
    for case in cases {
        let file = str_of(case, "file");
        let path = raw_corpus_dir()
            .expect("the golden corpus needs tests/fixtures/raw-corpus")
            .join(file);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let head = &bytes[..bytes.len().min(64)];
        let reader = registry
            .find_reader(head, &ext_of(&path))
            .unwrap_or_else(|| panic!("{file}: no container reader"));
        let mut source = source_of_bytes(bytes);
        let info = reader
            .read(&mut source, &CancelToken::new())
            .unwrap_or_else(|e| panic!("{file}: reader failed: {e}"));

        let read = read_exif(&mut source, &info);

        assert_eq!(summary_json(read.summary.as_ref()), case["summary"], "{file}: summary");
        assert_eq!(read.complete, bool_of(case, "complete"), "{file}: complete");
        assert_eq!(
            source.total_bytes_read(),
            i64_of(case, "bytes_read"),
            "{file}: bytes_read"
        );
    }
}

// ---------------------------------------------------------------- never panics

/// Contract test 4: 2000 deterministic random block lists over a valid TIFF plus random bytes. Asserts only that every
/// call returns (no panic, no hang, no overflow): offsets and lengths come from the boundary set of the golden cases.
#[test]
fn never_panics_on_random_blocks() {
    // Port note: no C# twin (contract W5-11c, test 4).
    let mut rng = XorShift64::new(0x5EED_0011_C0DE);
    let tiff = camera_tiff();
    let size = 2048usize;

    for _ in 0..2000 {
        let mut bytes = tiff.clone();
        bytes.resize(size, 0);
        for byte in bytes.iter_mut().skip(tiff.len()) {
            *byte = u8::try_from(rng.next_u64() & 0xFF).expect("masked to a byte");
        }
        let len = i64::try_from(size).expect("size fits i64");

        let block_count = 1 + rng.below(4);
        let blocks: Vec<ExifBlock> = (0..block_count)
            .map(|_| {
                let offset = match rng.below(10) {
                    0 => -1,
                    1 => 0,
                    2 => 1,
                    3 => len - 1,
                    4 => len,
                    5 => len + 1,
                    6 => i64::from(i32::MAX),
                    7 => i64::MAX,
                    8 => i64::MIN,
                    _ => i64::try_from(rng.below(4096)).expect("small offset"),
                };
                let length = match rng.below(10) {
                    0 => -1,
                    1 => 0,
                    2 => 1,
                    3 => len - 1,
                    4 => len,
                    5 => len + 1,
                    6 => i64::from(i32::MAX),
                    7 => i64::MAX,
                    8 => i64::MIN,
                    _ => i64::try_from(rng.below(8192)).expect("small length"),
                };
                ExifBlock::with_ifd_is_exif(offset, length, rng.below(2) == 0, rng.below(2) == 0)
            })
            .collect();

        let mut source = InMemoryRawHeaderSource::new(bytes);
        let _ = read_exif(&mut source, &info_with(RawFormat::Cr3, blocks));
    }
}

// ---------------------------------------------------------------- mutation checks (documented, not run in CI)
//
// AGENTS.md "Tests": the porter must show the golden fails when the guarded code is broken. The checks, run by hand once
// the implementation lands, each change one line of `raw_exif.rs` and must make `golden_raw_exif` fail:
// (a) `prefer_later_date` ignores `date_from_exif_ifd` (exif-block-without-date-then-plain-date-then-exif-date-exif-date-replaces
//     and exif-date-first-then-plain-date-keeps-exif-date must change);
// (b) `merge` uses `later.or(first)` (every field-first-wins case must change);
// (c) the early stop after completeness is dropped (first-block-complete-later-exif-block-not-read: reads gets a second entry);
// (d) `complete` is reset to true by a later successful read (failure-on-first-block-second-succeeds-complete-stays-false);
// (e) the skip test uses `>` instead of `>=` (block-offset-equals-length-is-skipped reads at `offset == length`);
// (f) `to_read` uses `max` instead of `min` (block-length-clamped-to-4-mib and block-4-mib-plus-one-clamped change).
