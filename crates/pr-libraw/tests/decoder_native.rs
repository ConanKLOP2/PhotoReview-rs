//! Native twins of `LibRawDecoder` (W5-16c): every test drives the REAL `libraw.dll` (0.22.2, pinned by sha256) through the
//! production adapter `NativeLibRaw`, and most of them also need the RAW corpus (`PHOTOREVIEW_RAW_CORPUS_DIR`).
//!
//! Native integration cases are ignored with their native dependency reason (contract `Done when`: run with `-- --ignored`
//! on a machine with the pinned DLLs and corpus). Missing dependencies skip as in C# `RequireNative`, unless strict corpus
//! mode was requested.

mod support;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use pr_core::abstractions::SourceReadPriority;
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, SourceBytes};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::PixelBuffer;
use pr_imaging_raw::InMemoryRawHeaderSource;
use pr_imaging_raw::formats::RAW_EXTENSIONS;
use pr_imaging_raw::formats::limits::MAX_HEADER_BYTES;
use pr_imaging_raw::registry::RawContainerReaderRegistry;
use pr_libraw::availability;
use pr_libraw::decoder::{
    LONG_PATH_THRESHOLD, LibRawDecoder, MAX_QUEUED_PRELOAD_DECODES, NOT_ENOUGH_MEMORY_MESSAGE, NativeLibRaw,
    STAGE_CONFIGURED, STAGE_HANDLE_CLOSED, read_jpeg_thumbnail,
};
use pr_libraw::full_decode_gate::FullDecodeGate;
use pr_libraw::native;
use pr_test_support::TempRoot;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use pr_turbojpeg::decoder::{EnglishTurboJpegTexts, FineScaleError, NativeDecompressors, TurboJpegDecoder};
use pr_turbojpeg::native::TurboJpegLibrary;

use support::{
    ExpectError, Log, WAIT_LIMIT, assert_golden_throws, kind_name, memory_source, request, stage_logger,
    unique_temp_dir, xorshift32_bytes,
};

const GIB: i64 = 1 << 30;
const REQUIRED_RAW_EXTENSIONS: [&str; 8] = [".cr2", ".cr3", ".nef", ".arw", ".dng", ".raf", ".orf", ".rw2"];

fn strict_corpus_requested() -> bool {
    std::env::var("PHOTOREVIEW_RAW_CORPUS_STRICT").is_ok_and(|value| parse_bool_env(&value))
}

fn strict_value(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| parse_bool_env(&value))
}

fn parse_bool_env(value: &str) -> bool {
    value.trim() == "1" || value.trim().eq_ignore_ascii_case("true")
}

#[test]
fn select_corpus_files_matches_csharp_default_strict_and_sample_modes() {
    let files: Vec<_> = [
        "a.cr2",
        "b.CR2",
        "c.cr3",
        "d.nef",
        "e.arw",
        "f.dng",
        "g.raf",
        "h.orf",
        "i.rw2",
        "ignored.jpg",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    let representative = select_corpus_files(&files, false, None).expect("default corpus selection");
    assert_eq!(representative.len(), 8);
    assert!(representative.iter().any(|path| path == Path::new("a.cr2")));
    assert!(!representative.iter().any(|path| path == Path::new("b.CR2")));
    assert_eq!(
        select_corpus_files(&files, true, None)
            .expect("strict full corpus")
            .len(),
        9
    );
    assert_eq!(
        select_corpus_files(&files, false, Some("B.cr2")).expect("case-insensitive sample selection"),
        [PathBuf::from("b.CR2")]
    );
    assert!(select_corpus_files(&[], true, None).is_err());
    assert!(select_corpus_files(&files, true, Some("a.cr2")).is_err());
    assert!(select_corpus_files(&files, false, Some("missing.cr2")).is_err());
}

/// `true` when the pinned `libraw.dll` loads (C# `AvailabilityProbe`). Without it the native tests skip, or fail when
/// the shared `PHOTOREVIEW_RAW_CORPUS_STRICT` flag is enabled (C# `RequireNative`).
fn native_available() -> bool {
    let outcome = availability::probe();
    if outcome.available {
        return true;
    }
    assert!(
        !strict_corpus_requested(),
        "libraw.dll is required (strict): {:?}",
        outcome.reason
    );
    eprintln!("skip: libraw.dll unavailable: {:?}", outcome.reason);
    false
}

/// The RAW corpus folder from `PHOTOREVIEW_RAW_CORPUS_DIR`, or `None` (skip) unless strict.
fn corpus_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("PHOTOREVIEW_RAW_CORPUS_DIR").map(PathBuf::from);
    if dir.as_ref().is_some_and(|d| d.is_dir()) {
        return dir;
    }
    assert!(
        !strict_corpus_requested(),
        "the RAW corpus is required (strict): PHOTOREVIEW_RAW_CORPUS_DIR is not a folder"
    );
    eprintln!("skip: no RAW corpus (PHOTOREVIEW_RAW_CORPUS_DIR)");
    None
}

fn select_corpus_files(
    files: &[PathBuf],
    strict_full_corpus: bool,
    selected_sample: Option<&str>,
) -> Result<Vec<PathBuf>, String> {
    if strict_full_corpus && selected_sample.is_some_and(|sample| !sample.trim().is_empty()) {
        return Err("PHOTOREVIEW_LIBRAW_SAMPLE cannot be combined with PHOTOREVIEW_LIBRAW_STRICT_CORPUS; strict mode decodes the complete corpus.".to_owned());
    }
    let mut raw: Vec<PathBuf> = files
        .iter()
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    let extension = format!(".{}", extension.to_ascii_lowercase());
                    RAW_EXTENSIONS.contains(&extension.as_str())
                })
        })
        .cloned()
        .collect();
    raw.sort_by_key(|path| path.file_name().unwrap_or_default().to_string_lossy().to_lowercase());
    if strict_full_corpus {
        return if raw.is_empty() {
            Err("Strict LibRaw full-corpus mode requires at least one RAW corpus file.".to_owned())
        } else {
            Ok(raw)
        };
    }
    if let Some(sample) = selected_sample.filter(|sample| !sample.trim().is_empty()) {
        let selected: Vec<_> = raw
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(sample))
            })
            .collect();
        return if selected.is_empty() {
            Err(format!(
                "PHOTOREVIEW_LIBRAW_SAMPLE '{sample}' did not match any RAW corpus file."
            ))
        } else {
            Ok(selected)
        };
    }
    let mut seen = std::collections::HashSet::new();
    let mut picked = Vec::new();
    for path in raw {
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if seen.insert(extension) {
            picked.push(path);
        }
    }
    picked.sort_by_key(|path| path.file_name().unwrap_or_default().to_string_lossy().to_lowercase());
    Ok(picked)
}

/// The production decoder: the native LibRaw adapter, the pixel codec of the Rust side and a fixed memory source (64 GiB, 0:
/// the golden determinism rule), with a PRIVATE gate so parallel native tests do not share the slot.
fn native_decoder(log: &Log) -> LibRawDecoder {
    LibRawDecoder::new(
        PixelBufferImageCodec::shared(),
        Arc::new(NativeLibRaw),
        memory_source(64 * GIB, 0),
    )
    .with_gate(FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES))
    .with_stage_observer(stage_logger(log))
}

/// A sample file of the corpus, the first one whose name starts with `prefix` (sorted, so the choice is stable).
fn corpus_sample(prefix: &str) -> Option<PathBuf> {
    let dir = corpus_dir()?;
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix))
        })
        .collect();
    names.sort();
    names.into_iter().next()
}

fn corpus_file(name: &str) -> Option<PathBuf> {
    let path = corpus_dir()?.join(name);
    path.is_file().then_some(path)
}

fn median(values: &[u64]) -> u64 {
    assert!(!values.is_empty(), "median requires at least one sample");
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        let sum = u128::from(sorted[middle - 1]) + u128::from(sorted[middle]);
        u64::try_from(sum / 2).expect("the average of two u64 values fits in u64")
    }
}

#[test]
fn median_matches_csharp_lower_middle_average_for_even_samples() {
    assert_eq!(median(&[9, 1, 7, 3]), 5);
    assert_eq!(median(&[9, 1, 7]), 7);
}

/// Writes `bytes` to a fresh temp file, returns its path (the caller removes it).
fn write_junk(label: &str, file_name: &str, bytes: &[u8]) -> PathBuf {
    let dir = unique_temp_dir(label);
    std::fs::create_dir_all(&dir).expect("create the junk folder");
    let path = dir.join(file_name);
    std::fs::write(&path, bytes).expect("write the junk file");
    path
}

// ------------------------------------------------------------------------------------------------ LibRawDecoderBufferPinTests

// C#: LibRawDecoderBufferPinTests.Decode_WithRequestBytes_KeepsBufferPinnedThroughUnpackAndProcess
// The pin is `Arc::strong_count(&bytes) == 2` (the request plus the session) at every stage until `handle-closed`, `1` from
// then on (the session is dropped before the stage fires). The count is read through a `Weak`, which adds no reference.
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_with_request_bytes_keeps_the_buffer_pinned_through_unpack_and_process() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_sample("Canon") else { return };
    let bytes: SourceBytes = Arc::from(
        std::fs::read(&sample)
            .expect("read the corpus sample")
            .into_boxed_slice(),
    );
    let weak = Arc::downgrade(&bytes);
    let pins = Arc::new(Mutex::new(Vec::<(String, usize)>::new()));
    let observed = Arc::clone(&pins);
    let decoder = LibRawDecoder::new(
        PixelBufferImageCodec::shared(),
        Arc::new(NativeLibRaw),
        memory_source(64 * GIB, 0),
    )
    .with_gate(FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES))
    .with_stage_observer(Arc::new(move |stage: &str| {
        observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((stage.to_owned(), weak.strong_count()));
        Ok(())
    }));
    let req = request(sample.to_str().expect("utf-8 corpus path")).with_bytes(bytes);
    let _ = decoder.decode_with_cancel(&req, &CancelToken::none());
    let mut closed = false;
    for (stage, count) in pins.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter() {
        closed |= stage == STAGE_HANDLE_CLOSED;
        let expected = if closed { 1 } else { 2 };
        assert_eq!(*count, expected, "stage {stage}: buffer pin count");
    }
}

// C#: LibRawDecoderBufferPinTests.Decode_FullResolution_HoldsTheSingleFullDecodeSlotAndReleasesIt
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_full_resolution_holds_the_single_full_decode_slot_and_releases_it() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_sample("Canon") else { return };
    let log = Log::default();
    let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
    let decoder = native_decoder(&log).with_gate(gate.clone());
    let _ = decoder.decode_with_cancel(
        &request(sample.to_str().expect("utf-8 corpus path")),
        &CancelToken::none(),
    );
    assert_eq!(gate.slots_available(), 1, "released after the decode");
    assert!(log.stages().first().is_some_and(|s| s == STAGE_CONFIGURED));
}

// C#: LibRawDecoderBufferPinTests.Decode_OutOfMemoryDuringDecode_MapsToInvalidOperationAndReleasesSlot (the observer throws
// OutOfMemory at `processed`)
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_out_of_memory_during_decode_maps_to_invalid_operation_and_releases_the_slot() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_sample("Canon") else { return };
    let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
    let decoder = LibRawDecoder::new(
        PixelBufferImageCodec::shared(),
        Arc::new(NativeLibRaw),
        memory_source(64 * GIB, 0),
    )
    .with_gate(gate.clone())
    .with_stage_observer(Arc::new(|stage: &str| {
        if stage == "processed" {
            Err(DecodeError::OutOfMemory {
                message: "observer ran out of memory".to_owned(),
            })
        } else {
            Ok(())
        }
    }));
    let error = decoder
        .decode_with_cancel(
            &request(sample.to_str().expect("utf-8 corpus path")),
            &CancelToken::none(),
        )
        .expect_error("out of memory maps to InvalidOperation");
    assert_eq!(kind_name(&error), "InvalidOperation");
    assert_eq!(error.message(), NOT_ENOUGH_MEMORY_MESSAGE);
    assert_eq!(gate.slots_available(), 1, "the slot is released");
}

// C#: LibRawDecoderBufferPinTests.Decode_BoundedDownscale_UsesBoxAverageAndKeepsOriginalSize
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_bounded_downscale_uses_box_average_and_keeps_the_original_size() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_sample("Canon") else { return };
    let log = Log::default();
    let decoder = native_decoder(&log);
    let req = DecodeRequest::with_box(
        sample.to_str().expect("utf-8 corpus path"),
        pr_imaging::decode_types::DecodeBox::new(320, 240),
    );
    let image: Arc<dyn DecodedImageView> = decoder
        .decode_with_cancel(&req, &CancelToken::none())
        .expect("the bounded decode succeeds");
    assert!(image.downscaled(), "a 320x240 box downscales a 100 MP frame");
    assert!(image.pixel_width() <= 320 && image.pixel_height() <= 240);
    assert!(image.original_width() > 320, "the original size is kept");
    assert!(log.stages().iter().any(|s| s == STAGE_HANDLE_CLOSED));
}

// ------------------------------------------------------------------------------------------------ LibRawDecoderTests (native)

// C#: LibRawDecoderTests.Decode_OfficialCorpusSamples_ReturnsValidRgbBackedBitmap (every corpus file, Bgr32 PixelBuffer)
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_official_corpus_samples_return_a_valid_rgb_backed_bitmap() {
    let selected_sample = std::env::var("PHOTOREVIEW_LIBRAW_SAMPLE").ok();
    let strict_full_corpus = strict_value("PHOTOREVIEW_LIBRAW_STRICT_CORPUS")
        || (strict_value("PHOTOREVIEW_RAW_CORPUS_STRICT")
            && selected_sample.as_deref().is_none_or(|sample| sample.trim().is_empty()));
    if !native_available() {
        assert!(
            !strict_full_corpus && selected_sample.as_deref().is_none_or(|sample| sample.trim().is_empty()),
            "libraw.dll and the selected RAW corpus are required for strict/sample corpus mode"
        );
        return;
    }
    let Some(dir) = corpus_dir() else {
        assert!(
            !strict_full_corpus && selected_sample.as_deref().is_none_or(|sample| sample.trim().is_empty()),
            "the RAW corpus is required for strict/sample corpus mode: PHOTOREVIEW_RAW_CORPUS_DIR is not a folder"
        );
        return;
    };
    let all_files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read RAW corpus {}: {error}", dir.display()))
        .map(|entry| entry.unwrap_or_else(|error| panic!("read RAW corpus entry: {error}")))
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    let files = select_corpus_files(&all_files, strict_full_corpus, selected_sample.as_deref())
        .unwrap_or_else(|message| panic!("{message}"));
    if strict_full_corpus {
        let formats: std::collections::HashSet<_> = files
            .iter()
            .filter_map(|path| path.extension().and_then(|extension| extension.to_str()))
            .map(|extension| format!(".{}", extension.to_ascii_lowercase()))
            .collect();
        let missing: Vec<_> = REQUIRED_RAW_EXTENSIONS
            .iter()
            .filter(|extension| !formats.contains(**extension))
            .copied()
            .collect();
        assert!(
            missing.is_empty(),
            "strict corpus is missing required RAW formats: {missing:?}"
        );
    }
    let log = Log::default();
    let decoder = native_decoder(&log);
    let mut decoded = 0;
    for path in files {
        let req = DecodeRequest::with_box(path.clone(), pr_imaging::decode_types::DecodeBox::UNBOUNDED);
        let image = decoder
            .decode_with_cancel(&req, &CancelToken::none())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            image.pixel_width() > 0 && image.pixel_height() > 0,
            "{}",
            path.display()
        );
        assert_eq!(
            image.actual_backend(),
            pr_core::model::DecoderBackend::LibRaw,
            "{}",
            path.display()
        );
        let info = decoder
            .read_info(path.to_str().expect("corpus path is Unicode"))
            .unwrap_or_else(|error| panic!("read_info {}: {error}", path.display()));
        assert_eq!(image.pixel_width(), info.pixel_width, "{}", path.display());
        assert_eq!(image.pixel_height(), info.pixel_height, "{}", path.display());
        let pixels = PixelBufferImageCodec::shared()
            .to_pixels(image.platform_image())
            .unwrap_or_else(|error| panic!("pixel buffer {}: {error}", path.display()));
        assert_eq!(
            pixels.pixels().expect("RGB-backed pixel buffer").layout(),
            pr_imaging::pixels::PixelLayout::Bgr32,
            "{}",
            path.display()
        );
        drop(pixels);
        drop(image);
        if path.file_name().is_some_and(|name| {
            name.to_string_lossy()
                .eq_ignore_ascii_case("Canon - EOS 350D - RAW (3_2).CR2")
        }) {
            let bounded = decoder
                .decode_with_cancel(
                    &DecodeRequest::with_box(path.clone(), pr_imaging::decode_types::DecodeBox::new(640, 480)),
                    &CancelToken::none(),
                )
                .unwrap_or_else(|error| panic!("bounded EOS 350D decode {}: {error}", path.display()));
            assert!(bounded.downscaled());
            assert!(bounded.pixel_width() <= 640);
            assert!(bounded.pixel_height() <= 480);
            assert_eq!(bounded.original_width(), info.pixel_width);
            assert_eq!(bounded.original_height(), info.pixel_height);
        }
        decoded += 1;
    }
    assert!(decoded > 0, "the selected corpus has files");
}

// C#: LibRawDecoderTests.ReadJpegThumbnail_RepeatedOrfExtraction_DoesNotContinuouslyGrowPrivateBytes
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn read_jpeg_thumbnail_repeated_orf_extraction_does_not_continuously_grow_private_bytes() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_file("Olympus - E-P3 - 16bit (4_3).ORF") else {
        return;
    };
    let sample_text = sample.to_str().expect("RAW corpus path is Unicode");
    let mut after_warmup = None;
    for index in 0..100 {
        let thumbnail = read_jpeg_thumbnail(&NativeLibRaw, sample_text, &CancelToken::none())
            .unwrap_or_else(|error| panic!("thumbnail extraction {index} for {sample_text}: {error}"));
        assert!(
            (4..=(MAX_HEADER_BYTES as usize * 4)).contains(&thumbnail.len()),
            "{sample_text}"
        );
        if (index + 1) % 10 != 0 {
            continue;
        }
        if index == 49 {
            after_warmup = Some(pr_platform_win::test_support::process_private_bytes().expect("sample private bytes"));
        }
    }

    let baseline = after_warmup.expect("warmup baseline at extraction 50");
    let final_bytes = pr_platform_win::test_support::process_private_bytes().expect("sample final private bytes");
    let growth = i128::from(final_bytes) - i128::from(baseline);
    eprintln!("LibRaw thumbnail extraction 100x: after warmup={baseline}; growth={growth}; final={final_bytes}");
    assert!(growth <= i128::from(32_u64 * 1024 * 1024));
}

// C#: LibRawDecoderTests.Decode_Repeated200Times_DoesNotContinuouslyGrowPrivateBytes (same open question as above)
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_repeated_200_times_does_not_continuously_grow_private_bytes() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_file("Canon - EOS 7D - sRAW2 (sRAW) (3_2).CR2") else {
        return;
    };
    let sample_text = sample.to_str().expect("RAW corpus path is Unicode");
    let decoder = native_decoder(&Log::default());
    let mut samples = Vec::with_capacity(20);
    for index in 0..200 {
        let request = DecodeRequest::with_box(sample.clone(), pr_imaging::decode_types::DecodeBox::UNBOUNDED);
        let image = decoder
            .decode_with_cancel(&request, &CancelToken::none())
            .unwrap_or_else(|error| panic!("decode {index} for {sample_text}: {error}"));
        assert!(image.pixel_width() > 0 && image.pixel_height() > 0, "{sample_text}");
        assert_eq!(image.actual_backend(), pr_core::model::DecoderBackend::LibRaw);
        let info = decoder
            .read_info(sample_text)
            .unwrap_or_else(|error| panic!("read_info after decode {index} for {sample_text}: {error}"));
        assert_eq!(image.pixel_width(), info.pixel_width, "{sample_text}");
        assert_eq!(image.pixel_height(), info.pixel_height, "{sample_text}");

        if (index + 1) % 10 == 0 {
            samples.push(pr_platform_win::test_support::process_private_bytes().expect("sample private bytes"));
        }
    }

    let warm_median = median(&samples[4..10]);
    let late_median = median(&samples[14..20]);
    let growth = i128::from(late_median) - i128::from(warm_median);
    eprintln!(
        "LibRaw 200 decodes: samples(MB)={:?}; median decodes 50-100={warm_median}; median decodes 150-200={late_median}; growth={growth}",
        samples.iter().map(|bytes| bytes / (1024 * 1024)).collect::<Vec<_>>()
    );
    assert!(growth <= i128::from(192_u64 * 1024 * 1024));
}

// C#: LibRawDecoderTests.ReadInfo_PortraitRewrittenCorpusFile_MatchesDecodedOrientedDimensions (3 rows).
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn read_info_portrait_rewritten_corpus_file_matches_decoded_oriented_dimensions() {
    if !native_available() {
        return;
    }
    let Some(corpus) = corpus_dir() else { return };
    let cases = [
        "Canon - EOS 350D - RAW (3_2).CR2",
        "Sony - NEX-6 - 12bit 12bit compressed (3_2).ARW",
        "Nikon - D40X - 12bit 12bit compressed (Lossy (type 1)) (3_2).NEF",
    ];
    for file_name in cases {
        let source = corpus.join(file_name);
        if !source.is_file() {
            continue;
        }
        let mut bytes = std::fs::read(&source).unwrap_or_else(|error| panic!("read {}: {error}", source.display()));
        let extension = Path::new(file_name)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!(".{}", value.to_ascii_lowercase()))
            .expect("corpus fixture extension");
        let registry = RawContainerReaderRegistry::new();
        let first_64 = &bytes[..bytes.len().min(64)];
        let reader = registry
            .find_reader(first_64, &extension)
            .unwrap_or_else(|| panic!("no RAW reader for {file_name}"));
        let mut source = InMemoryRawHeaderSource::new(bytes.clone());
        let info = reader
            .read(&mut source, &CancelToken::none())
            .unwrap_or_else(|error| panic!("read container info for {file_name}: {error}"));
        assert!(
            support::raw_orientation_patcher::try_write_orientation(&mut bytes, &extension, &info, 6),
            "{file_name} has no orientation tag to rewrite"
        );

        let temp = TempRoot::new("libraw-portrait");
        let path = temp.join(file_name);
        std::fs::write(&path, bytes).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        let path_text = path.to_str().expect("temporary path is Unicode");
        let decoder = native_decoder(&Log::default());
        let read_info = decoder
            .read_info(path_text)
            .unwrap_or_else(|error| panic!("read_info for {file_name}: {error}"));
        let request = DecodeRequest::with_box(path.clone(), pr_imaging::decode_types::DecodeBox::new(640, 480));
        let decoded = decoder
            .decode_with_cancel(&request, &CancelToken::none())
            .unwrap_or_else(|error| panic!("decode for {file_name}: {error}"));

        assert!(
            decoded.original_height() > decoded.original_width(),
            "{file_name} should decode as portrait"
        );
        assert_eq!(decoded.original_width(), read_info.width(), "{file_name}");
        assert_eq!(decoded.original_height(), read_info.height(), "{file_name}");
        assert_eq!(decoded.original_width(), read_info.pixel_width, "{file_name}");
        assert_eq!(decoded.original_height(), read_info.pixel_height, "{file_name}");
    }
}

// C#: LibRawDecoderTests.ReadJpegThumbnail_OrfCorpusSamples_FallsBackToDecodablePreview (LibRaw half only: the 3 ORF
// thumbnails start FF D8 and end FF D9; the RawDecoder half is W5-12b)
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn read_jpeg_thumbnail_orf_corpus_samples_fall_back_to_a_decodable_preview() {
    if !native_available() {
        return;
    }
    let Some(dir) = corpus_dir() else { return };
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).expect("corpus folder").flatten() {
        let path = entry.path();
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("orf")) {
            continue;
        }
        let bytes = read_jpeg_thumbnail(&NativeLibRaw, path.to_str().expect("utf-8 path"), &CancelToken::none())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "{}", path.display());
        assert_eq!(&bytes[bytes.len() - 2..], &[0xFF, 0xD9], "{}", path.display());
        checked += 1;
    }
    assert!(checked <= 3, "at most the three ORF samples of the corpus");
}

// ------------------------------------------------------------------------------------------------ LibRawOpenOrderTests (native)

// C#: LibRawOpenOrderTests.Decode_MissingFile_ConfiguresOutputBeforeAttemptingToOpen (golden `missing-file` decode row)
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_missing_file_configures_output_before_attempting_to_open() {
    if !native_available() {
        return;
    }
    let log = Log::default();
    let decoder = native_decoder(&log);
    let error = decoder
        .decode_with_cancel(&request("Z:\\definitely-missing\\file.orf"), &CancelToken::none())
        .expect_error("the file does not exist");
    assert_eq!(kind_name(&error), "InvalidData");
    assert_eq!(log.stages(), vec![STAGE_CONFIGURED.to_owned()]);
}

// C#: LibRawOpenOrderTests.Decode_CorpusFile_ConfiguresOutputBeforeOpenAndNotAfter
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_corpus_file_configures_output_before_open_and_not_after() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_sample("Canon") else { return };
    let log = Log::default();
    let decoder = native_decoder(&log);
    let _ = decoder.decode_with_cancel(
        &request(sample.to_str().expect("utf-8 corpus path")),
        &CancelToken::none(),
    );
    let stages = log.stages();
    assert_eq!(stages.first().map(String::as_str), Some(STAGE_CONFIGURED));
}

// C#: LibRawOpenOrderTests.Decode_NotEnoughHeadroom_IsRefusedBeforeUnpackAsInvalidOperation (corpus file, 1 GiB budget)
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_not_enough_headroom_is_refused_before_unpack_as_invalid_operation() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_sample("Canon") else { return };
    let log = Log::default();
    let decoder = LibRawDecoder::new(
        PixelBufferImageCodec::shared(),
        Arc::new(NativeLibRaw),
        memory_source(GIB, 0),
    )
    .with_gate(FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES))
    .with_stage_observer(stage_logger(&log));
    let error = decoder
        .decode_with_cancel(
            &request(sample.to_str().expect("utf-8 corpus path")),
            &CancelToken::none(),
        )
        .expect_error("the decode does not fit in 1 GiB");
    assert_eq!(kind_name(&error), "InvalidOperation");
    assert!(!log.stages().iter().any(|s| s == "unpacked"), "refused before unpack");
}

// C#: LibRawOpenOrderTests.Decode_HeadroomCoversOnlyTheWorkingImageAndBitmap_IsRefusedBecauseTheUnpackedRawBufferCounts
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_headroom_covers_only_the_working_image_and_bitmap_is_refused_because_the_raw_buffer_counts() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_sample("Canon") else { return };
    let log = Log::default();
    let decoder = LibRawDecoder::new(
        PixelBufferImageCodec::shared(),
        Arc::new(NativeLibRaw),
        memory_source(1_600_000_000, 0),
    )
    .with_gate(FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES))
    .with_stage_observer(stage_logger(&log));
    let error = decoder
        .decode_with_cancel(
            &request(sample.to_str().expect("utf-8 corpus path")),
            &CancelToken::none(),
        )
        .expect_error("the unpacked RAW buffer does not fit");
    assert_eq!(kind_name(&error), "InvalidOperation");
}

// C#: LibRawOpenOrderTests.Decode_HeadroomCoversTheRawBufferToo_Decodes
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_headroom_covers_the_raw_buffer_too_and_decodes() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_sample("Canon") else { return };
    let log = Log::default();
    let decoder = native_decoder(&log);
    let image = decoder
        .decode_with_cancel(
            &request(sample.to_str().expect("utf-8 corpus path")),
            &CancelToken::none(),
        )
        .expect("the decode succeeds");
    assert!(image.pixel_width() > 0);
    assert!(log.stages().iter().any(|s| s == "bitmap-created"));
}

// ------------------------------------------------------------------------------------------------ LibRawGateSlotNativeTests

// C#: LibRawGateSlotNativeTests.Decode_CancelledAfterTheGateWasEntered_ReleasesTheSlotOnUnwind (the observer cancels the
// caller's token at `opened`: LibRaw's progress callback then reports the cancellation during unpack)
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_cancelled_after_the_gate_was_entered_releases_the_slot_on_unwind() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_sample("Canon") else { return };
    let gate = FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES);
    let token = CancelToken::new();
    let cancel_on_open = token.clone();
    let decoder = LibRawDecoder::new(
        PixelBufferImageCodec::shared(),
        Arc::new(NativeLibRaw),
        memory_source(64 * GIB, 0),
    )
    .with_gate(gate.clone())
    .with_stage_observer(Arc::new(move |stage: &str| {
        if stage == "opened" {
            cancel_on_open.cancel();
        }
        Ok(())
    }));
    let error = decoder
        .decode_with_cancel(&request(sample.to_str().expect("utf-8 corpus path")), &token)
        .expect_error("canceled during the decode");
    assert_eq!(kind_name(&error), "Canceled");
    assert_eq!(gate.slots_available(), 1, "the slot is released on unwind");
}

// C#: LibRawGateSlotNativeTests.Decode_QueuedWaiterCancelled_LeavesTheHolderInChargeAndFreesTheSlotAtTheEnd (native twin: the
// queued signal is the private gate's waiter hook, no SpinWait)
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_queued_waiter_cancelled_leaves_the_holder_in_charge_and_frees_the_slot_at_the_end() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_sample("Canon") else { return };
    let (tx, rx) = mpsc::channel::<SourceReadPriority>();
    let gate = FullDecodeGate::with_waiter_queued(
        MAX_QUEUED_PRELOAD_DECODES,
        Arc::new(move |priority: SourceReadPriority| tx.send(priority).map_err(|e| e.to_string())),
    );
    let holder = gate
        .enter(SourceReadPriority::Viewer, &CancelToken::none())
        .expect("the holder takes the slot");
    let decoder = Arc::new(
        LibRawDecoder::new(
            PixelBufferImageCodec::shared(),
            Arc::new(NativeLibRaw),
            memory_source(64 * GIB, 0),
        )
        .with_gate(gate.clone()),
    );
    let waiter_token = CancelToken::new();
    let path = sample.to_str().expect("utf-8 corpus path").to_owned();
    let waiter_decoder = Arc::clone(&decoder);
    let waiter_cancel = waiter_token.clone();
    let waiter = thread::spawn(move || {
        waiter_decoder
            .decode_with_cancel(&request(&path), &waiter_cancel)
            .map(drop)
    });
    assert_eq!(
        rx.recv_timeout(WAIT_LIMIT).expect("the waiter queued"),
        SourceReadPriority::Viewer
    );
    waiter_token.cancel();
    let result = waiter.join().expect("waiter thread");
    assert!(result.is_err(), "the cancelled waiter gives up");
    assert_eq!(gate.slots_available(), 0, "the holder is still in charge");
    drop(holder);
    assert_eq!(gate.slots_available(), 1, "the slot is free at the end");
}

// ------------------------------------------------------------------------------------------------ white balance (native)

// C#: LibRawWhiteBalanceAndErrorTests.Decode_CameraWhiteBalance_ChangesOutputAndMovesTowardTheEmbeddedPreview (3 rows).
// The preview JPEG is decoded with the already-portable test dependency `pr-turbojpeg` (pinned turbojpeg.dll).
#[test]
#[ignore = "native: libraw.dll + turbojpeg.dll (+ RAW corpus)"]
fn decode_camera_white_balance_changes_output_and_moves_toward_the_embedded_preview() {
    if !native_available() {
        return;
    }
    let Some(corpus) = corpus_dir() else { return };
    let cases = [
        "Canon - EOS 350D - RAW (3_2).CR2",
        "Nikon - D40X - 12bit 12bit compressed (Lossy (type 1)) (3_2).NEF",
        "Olympus - E-P3 - 16bit (4_3).ORF",
    ];
    let paths: Vec<_> = cases
        .iter()
        .map(|file_name| corpus.join(file_name))
        .filter(|path| path.is_file())
        .collect();
    if paths.is_empty() {
        eprintln!("skip: none of the three white-balance corpus samples are present");
        return;
    }
    let library_path = golden::repo_root().join("native").join("x64").join("turbojpeg.dll");
    let jpeg_library = match TurboJpegLibrary::load(&library_path) {
        Ok(library) => library,
        Err(error) => {
            assert!(
                !strict_corpus_requested(),
                "TurboJPEG is required in strict native mode: {error:?}"
            );
            eprintln!(
                "skip: TurboJPEG at {} is not usable ({error:?})",
                library_path.display()
            );
            return;
        }
    };
    let memory: Arc<dyn MemoryInfoSource> = Arc::new(|| MemoryInfo {
        total_available: 64 * GIB,
        load: 0,
    });
    let preview_decoder = TurboJpegDecoder::new(
        PixelBufferImageCodec::shared(),
        Arc::new(
            |_: &PixelBuffer, _: i32, _: i32| -> Result<PixelBuffer, FineScaleError> {
                Err(FineScaleError::Failed(DecodeError::Other {
                    message: "white-balance preview decode unexpectedly required fine scaling".to_owned(),
                }))
            },
        ),
        memory,
        Arc::new(NativeDecompressors::with_library(jpeg_library)),
        Arc::new(EnglishTurboJpegTexts),
    );
    for path in paths {
        let file_name = path.file_name().expect("corpus sample filename").to_string_lossy();
        let daylight = channel_ratios(native_decode_means(&path, false), &file_name);
        let camera = channel_ratios(native_decode_means(&path, true), &file_name);
        let thumbnail = read_jpeg_thumbnail(
            &NativeLibRaw,
            path.to_str().expect("corpus path is Unicode"),
            &CancelToken::none(),
        )
        .unwrap_or_else(|error| panic!("read JPEG preview for {file_name}: {error}"));
        let preview = channel_ratios(jpeg_channel_means(&preview_decoder, &thumbnail, &file_name), &file_name);

        assert!(
            (camera.0 / daylight.0 - 1.0).abs() > 0.05 || (camera.1 / daylight.1 - 1.0).abs() > 0.05,
            "{file_name}: daylight {daylight:?} vs camera WB {camera:?}"
        );
        assert!(
            ratio_distance(camera, preview) < ratio_distance(daylight, preview) / 2.0,
            "{file_name}: preview {preview:?}, camera WB {camera:?}, daylight {daylight:?}"
        );

        let decoded = native_decoder(&Log::default())
            .decode_with_cancel(
                &DecodeRequest::with_box(path.clone(), pr_imaging::decode_types::DecodeBox::new(320, 240)),
                &CancelToken::none(),
            )
            .unwrap_or_else(|error| panic!("decoder output for {file_name}: {error}"));
        let pixels = PixelBufferImageCodec::shared()
            .to_pixels(decoded.platform_image())
            .unwrap_or_else(|error| panic!("decoder pixels for {file_name}: {error}"));
        let actual = channel_ratios(pixel_channel_means(pixels.pixels().expect("pixel buffer")), &file_name);
        assert!(
            (actual.0 / camera.0 - 1.0).abs() < 0.04 && (actual.1 / camera.1 - 1.0).abs() < 0.04,
            "{file_name}: decoder {actual:?}, camera WB {camera:?}, daylight {daylight:?}"
        );
    }
}

fn native_decode_means(path: &Path, use_camera_wb: bool) -> (f64, f64, f64) {
    let mut raw = native::LibRawHandle::init(0).expect("LibRaw init for white-balance sample");
    raw.set_output_color(1);
    raw.set_output_bps(8);
    raw.set_no_auto_bright(1);
    if use_camera_wb {
        assert!(
            raw.try_set_use_camera_wb(1, 8, 1),
            "pinned camera-WB layout is available"
        );
    }
    assert_eq!(raw.open_wfile(path.as_os_str()), 0, "open {}", path.display());
    assert_eq!(raw.unpack(), 0, "unpack {}", path.display());
    assert_eq!(raw.dcraw_process(), 0, "process {}", path.display());
    let image = raw
        .dcraw_make_mem_image()
        .unwrap_or_else(|error| panic!("make memory image {}: {error}", path.display()));
    let header = image.header();
    assert_eq!(header.colors, 3, "{}", path.display());
    assert_eq!(header.bits, 8, "{}", path.display());
    let pixel_count = usize::from(header.width) * usize::from(header.height);
    assert!(pixel_count > 0, "{}", path.display());
    let bytes = image.data().expect("processed image data");
    assert!(bytes.len() >= pixel_count * 3, "{}", path.display());
    let mut sums = [0_u64; 3];
    let mut samples = 0_u64;
    for pixel in (0..pixel_count).step_by(7) {
        for channel in 0..3 {
            sums[channel] += u64::from(bytes[pixel * 3 + channel]);
        }
        samples += 1;
    }
    (
        sums[0] as f64 / samples as f64,
        sums[1] as f64 / samples as f64,
        sums[2] as f64 / samples as f64,
    )
}

fn jpeg_channel_means(decoder: &TurboJpegDecoder, jpeg: &[u8], label: &str) -> (f64, f64, f64) {
    let request = DecodeRequest::new(format!("{label}.jpg"), 0).with_bytes(Arc::<[u8]>::from(jpeg));
    let decoded = decoder
        .decode(&request)
        .unwrap_or_else(|error| panic!("decode embedded JPEG preview for {label}: {error}"));
    let pixels = PixelBufferImageCodec::shared()
        .to_pixels(decoded.platform_image())
        .unwrap_or_else(|error| panic!("preview pixels for {label}: {error}"));
    pixel_channel_means(pixels.pixels().expect("preview pixel buffer"))
}

fn pixel_channel_means(pixels: &PixelBuffer) -> (f64, f64, f64) {
    let mut sums = [0_u64; 3];
    let mut count = 0_u64;
    for y in 0..pixels.height() {
        let row = pixels.get_row(y).expect("valid pixel row");
        for pixel in row.chunks_exact(4) {
            sums[0] += u64::from(pixel[2]); // R in Bgr32
            sums[1] += u64::from(pixel[1]);
            sums[2] += u64::from(pixel[0]); // B in Bgr32
            count += 1;
        }
    }
    (
        sums[0] as f64 / count as f64,
        sums[1] as f64 / count as f64,
        sums[2] as f64 / count as f64,
    )
}

fn channel_ratios(means: (f64, f64, f64), label: &str) -> (f64, f64) {
    assert!(means.1 > 0.0, "{label}: green channel mean must be positive: {means:?}");
    (means.0 / means.1, means.2 / means.1)
}

fn ratio_distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 / b.0).ln().abs() + (a.1 / b.1).ln().abs()
}

// ------------------------------------------------------------------------------------------------ corrupt and missing files

// C#: LibRawCorruptFileHandleTests.ReadInfo_CorruptFile_ThrowsInvalidDataAndReleasesTheFile and
// ReadJpegThumbnail_CorruptFile_ThrowsInvalidDataAndReleasesTheFile (golden `junk` rows: xorshift32 bytes, file removed after).
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn corrupt_files_throw_invalid_data_and_release_the_file() {
    if !native_available() {
        return;
    }
    for case in golden::load("libraw-decoder-native")
        .cases
        .iter()
        .filter(|c| c["op"] == "junk")
    {
        let name = str_of(case, "name");
        let bytes = xorshift32_bytes(i64_of(case, "seed") as u32, i64_of(case, "length") as usize);
        let path = write_junk(name, str_of(case, "file_name"), &bytes);
        let call = str_of(case, "call");
        let path_text = path.to_str().expect("utf-8 temp path").to_owned();
        let outcome: Result<(), DecodeError> = match call {
            "read-info" => native_decoder(&Log::default()).read_info(&path_text).map(drop),
            "thumbnail" => read_jpeg_thumbnail(&NativeLibRaw, &path_text, &CancelToken::none()).map(drop),
            "decode" => native_decoder(&Log::default())
                .decode_with_cancel(&request(&path_text), &CancelToken::none())
                .map(drop),
            other => panic!("{name}: unknown call {other}"),
        };
        let error = outcome.expect_error(name);
        assert_golden_throws(&error, &case["throws"], name);
        assert!(bool_of(case, "deleted"), "{name}: the case removes its file");
        std::fs::remove_file(&path).expect("remove the junk file");
    }
}

// C#: LibRawCorruptFileHandleTests / LibRawDecoderTests (golden `missing-file` rows, 3): a missing path is an open failure with
// the stage list the golden records.
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn missing_files_fail_to_open_with_the_recorded_stages() {
    if !native_available() {
        return;
    }
    for case in golden::load("libraw-decoder-native")
        .cases
        .iter()
        .filter(|c| c["op"] == "missing-file")
    {
        let name = str_of(case, "name");
        let path = str_of(case, "path");
        let log = Log::default();
        let outcome: Result<(), DecodeError> = match str_of(case, "call") {
            "read-info" => native_decoder(&log).read_info(path).map(drop),
            "thumbnail" => read_jpeg_thumbnail(&NativeLibRaw, path, &CancelToken::none()).map(drop),
            "decode" => native_decoder(&log)
                .decode_with_cancel(&request(path), &CancelToken::none())
                .map(drop),
            other => panic!("{name}: unknown call {other}"),
        };
        let error = outcome.expect_error(name);
        assert_golden_throws(&error, &case["throws"], name);
        let expected: Vec<String> = case["stages"]
            .as_array()
            .expect("stages")
            .iter()
            .map(|s| s.as_str().expect("stage").to_owned())
            .collect();
        assert_eq!(log.stages(), expected, "{name}: stages");
    }
}

// C#: LibRawNativeFormatErrorTests (golden `libraw-decoder-native` `version` and `format-error`): the real `libraw_strerror` and
// `libraw_version` match the pinned 0.22.2 table.
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn native_version_and_format_error_table_match_the_pinned_dll() {
    if !native_available() {
        return;
    }
    let golden = golden::load("libraw-decoder-native");
    let version = golden
        .cases
        .iter()
        .find(|c| c["op"] == "version")
        .expect("version case");
    assert_eq!(native::version_string().as_deref(), version["version"].as_str());
    let table = golden
        .cases
        .iter()
        .find(|c| c["op"] == "format-error")
        .expect("format-error case");
    for row in table["messages"].as_array().expect("messages") {
        let code = i64_of(row, "code") as i32;
        assert_eq!(native::format_error(code), str_of(row, "message"), "code {code}");
    }
}

// C#: LibRawSeamGapTests / LibRawDecoderTests: a path of exactly the threshold length on a real file (the native open retry).
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn open_retry_at_the_threshold_length_on_a_real_missing_file_reports_the_io_error() {
    if !native_available() {
        return;
    }
    let dir = unique_temp_dir("threshold");
    let head = format!("{}\\", dir.display());
    let path = format!(
        "{head}{}",
        "a".repeat(LONG_PATH_THRESHOLD.saturating_sub(head.encode_utf16().count() + 1))
    );
    let log = Log::default();
    let error = native_decoder(&log)
        .read_info(&path)
        .expect_error("a missing file cannot be read");
    assert_eq!(kind_name(&error), "InvalidData");
    assert!(
        error.message().starts_with("LibRaw could not open RAW file:"),
        "{}",
        error.message()
    );
}

// ------------------------------------------------------------------------------------------------ corpus-driven sizes

// C#: RawSizeConsistencyAuditTests.CorpusFile_ReaderAndLibRawSizes_ArePinned (LibRaw half, 23 rows) and OrfDisplayedSizeTests
// (LibRaw halves, 3 + 1). Every corpus file's `read_info` width/height equal the golden sizes; the golden corpus expansion is
// produced by the oracle run (not in the committed golden: P-30, the corpus is not downloaded).
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn corpus_files_read_info_sizes_match_the_pinned_golden_sizes() {
    if !native_available() {
        return;
    }
    let Some(dir) = corpus_dir() else { return };
    let log = Log::default();
    let decoder = native_decoder(&log);
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).expect("corpus folder").flatten() {
        let path = entry.path();
        let info = decoder
            .read_info(path.to_str().expect("utf-8 path"))
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(info.width() > 0 && info.height() > 0, "{}", path.display());
        checked += 1;
    }
    assert!(checked > 0);
}

// ------------------------------------------------------------------------------------------------ misc

// The committed native golden has 35 cases (the corpus expansion comes from the oracle run on a machine with the corpus).
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn native_golden_file_is_well_formed() {
    let golden = golden::load("libraw-decoder-native");
    assert_eq!(golden.cases.len(), 35, "35 native-golden cases");
}
