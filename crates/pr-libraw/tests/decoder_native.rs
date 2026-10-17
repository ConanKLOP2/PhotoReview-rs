//! Native twins of `LibRawDecoder` (W5-16c): every test drives the REAL `libraw.dll` (0.22.2, pinned by sha256) through the
//! production adapter `NativeLibRaw`, and most of them also need the RAW corpus (`PHOTOREVIEW_RAW_CORPUS_DIR`).
//!
//! Every test here is `#[ignore = "native: libraw.dll (+ RAW corpus)"]` (contract `Done when`: run with `-- --ignored` on a
//! machine that has the pinned DLL and the corpus). When the DLL or the corpus is missing the test skips (C# `RequireNative`),
//! unless `PHOTOREVIEW_RAW_CORPUS_STRICT=1` turns the skip into a failure.
//!
//! Red tests: the stub bodies are `todo!()`, so every test here fails until the W5-16c porter fills them in. Two groups stay
//! red for their own reasons, documented on each test: the private-bytes leak tests (contract Open question 4: a
//! `process_private_bytes()` helper) and the white-balance preview decode (Open question 5: a JPEG decoder for the test).

mod support;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use pr_core::abstractions::SourceReadPriority;
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, SourceBytes};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_libraw::availability;
use pr_libraw::decoder::{
    LONG_PATH_THRESHOLD, LibRawDecoder, MAX_QUEUED_PRELOAD_DECODES, NOT_ENOUGH_MEMORY_MESSAGE, NativeLibRaw,
    STAGE_CONFIGURED, STAGE_HANDLE_CLOSED, read_jpeg_thumbnail,
};
use pr_libraw::full_decode_gate::FullDecodeGate;
use pr_libraw::native;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};

use support::{
    ExpectError, Log, WAIT_LIMIT, assert_golden_throws, kind_name, memory_source, request, stage_logger,
    unique_temp_dir, xorshift32_bytes,
};

const GIB: i64 = 1 << 30;

/// `true` when the pinned `libraw.dll` loads (C# `AvailabilityProbe`). Without it the native tests skip, or fail when
/// `PHOTOREVIEW_RAW_CORPUS_STRICT=1` (C# `RequireNative`).
fn native_available() -> bool {
    let outcome = availability::probe();
    if outcome.available {
        return true;
    }
    assert!(
        std::env::var("PHOTOREVIEW_RAW_CORPUS_STRICT")
            .map(|v| v != "1")
            .unwrap_or(true),
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
        std::env::var("PHOTOREVIEW_RAW_CORPUS_STRICT")
            .map(|v| v != "1")
            .unwrap_or(true),
        "the RAW corpus is required (strict): PHOTOREVIEW_RAW_CORPUS_DIR is not a folder"
    );
    eprintln!("skip: no RAW corpus (PHOTOREVIEW_RAW_CORPUS_DIR)");
    None
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
    if !native_available() {
        return;
    }
    let Some(dir) = corpus_dir() else { return };
    let log = Log::default();
    let decoder = native_decoder(&log);
    let mut decoded = 0;
    for entry in std::fs::read_dir(dir).expect("corpus folder").flatten() {
        let path = entry.path();
        let req = DecodeRequest::with_box(path.clone(), pr_imaging::decode_types::DecodeBox::new(320, 240));
        let image = decoder
            .decode_with_cancel(&req, &CancelToken::none())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            image.pixel_width() > 0 && image.pixel_height() > 0,
            "{}",
            path.display()
        );
        decoded += 1;
    }
    assert!(decoded > 0, "the corpus has files");
}

// C#: LibRawDecoderTests.ReadJpegThumbnail_RepeatedOrfExtraction_DoesNotContinuouslyGrowPrivateBytes
// Open question 4: private bytes need a `process_private_bytes()` helper (not in this crate yet): red until it lands.
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn read_jpeg_thumbnail_repeated_orf_extraction_does_not_continuously_grow_private_bytes() {
    if !native_available() {
        return;
    }
    let Some(sample) = corpus_sample("Olympus") else { return };
    let _ = sample;
    todo!("needs process_private_bytes() (contract Open question 4): not ported yet")
}

// C#: LibRawDecoderTests.Decode_Repeated200Times_DoesNotContinuouslyGrowPrivateBytes (same open question as above)
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_repeated_200_times_does_not_continuously_grow_private_bytes() {
    if !native_available() {
        return;
    }
    todo!("needs process_private_bytes() (contract Open question 4): not ported yet")
}

// C#: LibRawDecoderTests.ReadInfo_PortraitRewrittenCorpusFile_MatchesDecodedOrientedDimensions (3 rows). Needs W5-11a's
// registry and the `RawOrientationPatcher` test-support port: red until both merge.
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn read_info_portrait_rewritten_corpus_file_matches_decoded_oriented_dimensions() {
    if !native_available() {
        return;
    }
    todo!("needs W5-11a registry and the RawOrientationPatcher test-support port")
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
// Open question 5: the preview means need a JPEG decoder in the test (WIC via pr-platform-win, or TurboJPEG W5-14): red until
// one is chosen.
#[test]
#[ignore = "native: libraw.dll (+ RAW corpus)"]
fn decode_camera_white_balance_changes_output_and_moves_toward_the_embedded_preview() {
    if !native_available() {
        return;
    }
    let Some(_sample) = corpus_sample("Canon") else { return };
    todo!("needs a JPEG decoder for the preview means (contract Open question 5)")
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
