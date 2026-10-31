//! DV-24 (INV-8): a RAW file must be renamable and deletable WHILE LibRaw decodes it. `libraw_open_wfile` opens the file
//! without `FILE_SHARE_DELETE` and holds the handle until `libraw_close` (lane `fix-raw-share-delete`; observed with Python
//! ctypes in `docs/reviews/FIX-IMAGING-CACHE-2026-10-31.md` M4), so Delete/Move of the open RAW fails with sharing violation 32.
//! `LibRawDecoder::with_shared_file_read` reads the file once through a Read|Write|Delete share handle and decodes the bytes
//! (`libraw_open_buffer`).
//!
//! No C# twin: Rust-only evidence. The fake-session tests run everywhere; the `native` tests need `libraw.dll` next to the test
//! binary and `python` (the synthetic DNG generator `tools/dev/make-synthetic-raw.py`, embedded at compile time) and are
//! `#[ignore]`d. These are synthetic DNGs, not a camera corpus.

mod support;

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};

use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeBox, DecodeRequest, DecodedImageView};
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_libraw::decoder::{
    LibRawDecoder, MAX_QUEUED_PRELOAD_DECODES, NativeLibRaw, SHARED_READ_CHUNK_BYTES, STAGE_OPENED, STAGE_UNPACKED,
    SharedReadError, read_file_shared,
};
use pr_libraw::full_decode_gate::FullDecodeGate;
use pr_test_support::TempRoot;
use support::{ExpectError, FakeApi, FakeCodec, Log, Script, kind_name, memory_source, pixel_memory_shared};

const GIB: i64 = 1 << 30;
const GENERATOR: &str = include_str!("../../../tools/dev/make-synthetic-raw.py");
const WIDTH: i32 = 160;
const HEIGHT: i32 = 96;
/// Windows `ERROR_SHARING_VIOLATION`.
const ERROR_SHARING_VIOLATION: i32 = 32;

fn plenty() -> Arc<dyn MemoryInfoSource> {
    memory_source(64 * GIB, 0)
}

fn strict() -> bool {
    matches!(
        std::env::var("PHOTOREVIEW_RS_SYNTHETIC_STRICT").as_deref(),
        Ok("1") | Ok("true")
    )
}

// ------------------------------------------------------------------------------------------------ read_file_shared (pure)

// Rust-only: more than one chunk comes back byte for byte, and the file is not held afterwards.
#[test]
fn read_file_shared_returns_every_byte_of_a_multi_chunk_file_and_releases_the_file() {
    let root = TempRoot::new("raw-share-read-all");
    let path = root.join("big.raw");
    let content: Vec<u8> = (0..(SHARED_READ_CHUNK_BYTES * 2 + 12_345))
        .map(|i| (i % 251) as u8)
        .collect();
    std::fs::write(&path, &content).expect("write fixture");
    let bytes = read_file_shared(&path, &CancelToken::none(), plenty().as_ref()).expect("read");
    assert_eq!(&*bytes, content.as_slice());
    std::fs::rename(&path, root.join("moved.raw")).expect("the file is not held after the read");
}

// Rust-only: an empty file is an empty buffer (LibRaw then rejects it with its own error, as for the path open).
#[test]
fn read_file_shared_of_an_empty_file_is_an_empty_buffer() {
    let root = TempRoot::new("raw-share-read-empty");
    let path = root.join("empty.raw");
    std::fs::write(&path, b"").expect("write fixture");
    let bytes = read_file_shared(&path, &CancelToken::none(), plenty().as_ref()).expect("read");
    assert!(bytes.is_empty());
}

// Rust-only: a missing file is an I/O error (mapped to LIBRAW_IO_ERROR by the decoder).
#[test]
fn read_file_shared_of_a_missing_file_is_an_io_error() {
    let root = TempRoot::new("raw-share-read-missing");
    let error =
        read_file_shared(&root.join("none.raw"), &CancelToken::none(), plenty().as_ref()).expect_err("missing file");
    assert!(matches!(error, SharedReadError::Io(_)), "{error:?}");
}

// Rust-only: a token canceled before the read never opens the file.
#[test]
fn read_file_shared_with_a_canceled_token_reads_nothing() {
    let root = TempRoot::new("raw-share-read-canceled");
    let path = root.join("a.raw");
    std::fs::write(&path, b"abc").expect("write fixture");
    let token = CancelToken::new();
    token.cancel();
    let error = read_file_shared(&path, &token, plenty().as_ref()).expect_err("canceled");
    assert!(matches!(error, SharedReadError::Canceled), "{error:?}");
}

// Rust-only: a cancel that arrives after the open (here: raised by the memory probe, which runs between the length and the first
// chunk) stops the read before any chunk, deterministically and without a sleep.
#[test]
fn read_file_shared_stops_between_the_open_and_the_first_chunk_when_canceled() {
    let root = TempRoot::new("raw-share-read-cancel-mid");
    let path = root.join("a.raw");
    std::fs::write(&path, vec![7u8; SHARED_READ_CHUNK_BYTES + 1]).expect("write fixture");
    let token = CancelToken::new();
    let raiser = token.clone();
    let probe: Arc<dyn MemoryInfoSource> = Arc::new(move || {
        raiser.cancel();
        MemoryInfo {
            total_available: 64 * GIB,
            load: 0,
        }
    });
    let error = read_file_shared(&path, &token, probe.as_ref()).expect_err("canceled");
    assert!(matches!(error, SharedReadError::Canceled), "{error:?}");
    std::fs::rename(&path, root.join("b.raw")).expect("the file is released after a canceled read");
}

// Rust-only: the file must fit twice (Vec plus the shared copy) in the headroom; the read is refused before any allocation.
#[test]
fn read_file_shared_is_refused_when_twice_the_length_exceeds_the_headroom() {
    let root = TempRoot::new("raw-share-read-headroom");
    let path = root.join("a.raw");
    std::fs::write(&path, vec![1u8; 1000]).expect("write fixture");
    let tight = memory_source(1999, 0);
    let refused = read_file_shared(&path, &CancelToken::none(), tight.as_ref()).expect_err("refused");
    assert!(matches!(refused, SharedReadError::OutOfMemory), "{refused:?}");
    let exact = memory_source(2000, 0);
    assert!(
        read_file_shared(&path, &CancelToken::none(), exact.as_ref()).is_ok(),
        "an exact fit passes"
    );
    let loaded = memory_source(2000, 1);
    assert!(
        matches!(
            read_file_shared(&path, &CancelToken::none(), loaded.as_ref()),
            Err(SharedReadError::OutOfMemory)
        ),
        "the memory load is part of the budget"
    );
}

// ------------------------------------------------------------------------------------------------ decoder over fakes

fn fake_decoder(log: &Log, shared: bool, memory: Arc<dyn MemoryInfoSource>) -> LibRawDecoder {
    let codec = Arc::new(FakeCodec {
        log: log.clone(),
        fail: false,
    });
    let decoder = LibRawDecoder::new(codec, FakeApi::arc(Script::default(), log), memory)
        .with_gate(FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES));
    if shared {
        decoder.with_shared_file_read()
    } else {
        decoder
    }
}

// Rust-only: with the flag, a path request reaches LibRaw as ONE open_buffer over the file bytes and never as open_wfile.
#[test]
fn shared_file_read_opens_libraw_on_the_bytes_and_not_on_the_path() {
    let _pixels = pixel_memory_shared();
    let root = TempRoot::new("raw-share-fake-buffer");
    let path = root.join("photo.cr3");
    std::fs::write(&path, vec![9u8; 4096]).expect("write fixture");
    let log = Log::default();
    let decoder = fake_decoder(&log, true, plenty());
    let _ = decoder.decode_with_cancel(
        &DecodeRequest::new(path.to_str().expect("utf-8"), 0),
        &CancelToken::none(),
    );
    let lines = log.lines();
    assert!(lines.iter().any(|l| l == "open_buffer:4096"), "{lines:?}");
    assert!(!lines.iter().any(|l| l.starts_with("open_wfile:")), "{lines:?}");
}

// Rust-only: without the flag the C# path open is unchanged (the oracle tests depend on it).
#[test]
fn without_the_flag_the_path_open_is_unchanged() {
    let _pixels = pixel_memory_shared();
    let log = Log::default();
    let decoder = fake_decoder(&log, false, plenty());
    let _ = decoder.decode_with_cancel(&DecodeRequest::new("Z:\\no\\such.cr3", 0), &CancelToken::none());
    let lines = log.lines();
    assert!(lines.iter().any(|l| l.starts_with("open_wfile:")), "{lines:?}");
    assert!(!lines.iter().any(|l| l.starts_with("open_buffer:")), "{lines:?}");
}

// Rust-only: a missing file fails like the C# path open (InvalidData, "LibRaw could not open RAW file"), before any unpack, so
// the fallback chain classifies it the same way.
#[test]
fn shared_file_read_of_a_missing_file_fails_with_the_same_text_as_the_path_open() {
    let _pixels = pixel_memory_shared();
    let root = TempRoot::new("raw-share-fake-missing");
    let missing = root.join("gone.cr3");
    let log = Log::default();
    let error = fake_decoder(&log, true, plenty())
        .decode_with_cancel(
            &DecodeRequest::new(missing.to_str().expect("utf-8"), 0),
            &CancelToken::none(),
        )
        .expect_error("the file does not exist");
    assert_eq!(kind_name(&error), "InvalidData");
    assert!(error.to_string().contains("LibRaw could not open RAW file:"), "{error}");
    assert!(!log.lines().iter().any(|l| l == "unpack"), "{:?}", log.lines());
}

// Rust-only: a file that does not fit twice in the headroom is "Not enough memory", like LibRaw's own allocation failure.
#[test]
fn shared_file_read_refused_by_the_headroom_is_the_not_enough_memory_error() {
    let _pixels = pixel_memory_shared();
    let root = TempRoot::new("raw-share-fake-headroom");
    let path = root.join("photo.cr3");
    std::fs::write(&path, vec![9u8; 4096]).expect("write fixture");
    let log = Log::default();
    let error = fake_decoder(&log, true, memory_source(8000, 0))
        .decode_with_cancel(
            &DecodeRequest::new(path.to_str().expect("utf-8"), 0),
            &CancelToken::none(),
        )
        .expect_error("headroom");
    assert_eq!(error.to_string(), pr_libraw::decoder::NOT_ENOUGH_MEMORY_MESSAGE);
}

// Rust-only: cancel wins over the I/O error code, as for check_result on the path open.
#[test]
fn shared_file_read_with_a_canceled_token_is_canceled() {
    let _pixels = pixel_memory_shared();
    let root = TempRoot::new("raw-share-fake-cancel");
    let path = root.join("photo.cr3");
    std::fs::write(&path, vec![9u8; 4096]).expect("write fixture");
    let log = Log::default();
    // `decode_with_cancel` checks the token before the gate, so the cancel is raised by the memory probe, which the shared read
    // consults after the open and before the first chunk.
    let live = CancelToken::new();
    let raiser = live.clone();
    let probe_live: Arc<dyn MemoryInfoSource> = Arc::new(move || {
        raiser.cancel();
        MemoryInfo {
            total_available: 64 * GIB,
            load: 0,
        }
    });
    let error = fake_decoder(&log, true, probe_live)
        .decode_with_cancel(&DecodeRequest::new(path.to_str().expect("utf-8"), 0), &live)
        .expect_error("canceled while reading");
    assert!(matches!(error.kind(), DecodeError::Canceled), "{error:?}");
}

// ------------------------------------------------------------------------------------------------ native (real libraw.dll)

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

fn native_decoder(shared: bool) -> LibRawDecoder {
    let decoder = LibRawDecoder::new(PixelBufferImageCodec::shared(), Arc::new(NativeLibRaw), plenty())
        .with_gate(FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES));
    if shared {
        decoder.with_shared_file_read()
    } else {
        decoder
    }
}

/// What the observer found out at one stage: the OS error of a rename / delete attempted while the decode is in flight.
#[derive(Default)]
struct Probe {
    renamed: Mutex<Vec<(String, Option<i32>)>>,
    deleted: Mutex<Vec<(String, Option<i32>)>>,
    current: Mutex<Option<PathBuf>>,
}

/// Observer: at `rename_at` renames the file to `<name>.moved`, at `delete_at` deletes it (wherever it is now). Both record the
/// raw OS error (`None` = success).
fn mover(probe: &Arc<Probe>, rename_at: &'static str, delete_at: &'static str) -> pr_libraw::decoder::StageObserver {
    let probe = Arc::clone(probe);
    Arc::new(move |stage: &str| {
        let mut current = probe.current.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(path) = current.clone() else { return Ok(()) };
        if stage == rename_at {
            let target = path.with_extension("moved");
            let outcome = std::fs::rename(&path, &target);
            probe.renamed.lock().unwrap_or_else(PoisonError::into_inner).push((
                stage.to_owned(),
                outcome.as_ref().err().and_then(std::io::Error::raw_os_error),
            ));
            if outcome.is_ok() {
                *current = Some(target);
            }
        } else if stage == delete_at {
            let outcome = std::fs::remove_file(&path);
            probe.deleted.lock().unwrap_or_else(PoisonError::into_inner).push((
                stage.to_owned(),
                outcome.as_ref().err().and_then(std::io::Error::raw_os_error),
            ));
        }
        Ok(())
    })
}

fn bgra_of(image: &dyn DecodedImageView) -> (i32, i32, Vec<u8>) {
    let lease = PixelBufferImageCodec::shared()
        .to_pixels(image.platform_image())
        .expect("pixels of the decoded image");
    let buffer = lease.pixels().expect("pixel-backed image");
    let mut all = Vec::new();
    for y in 0..buffer.height() {
        all.extend_from_slice(buffer.get_row(y).expect("row"));
    }
    (buffer.width(), buffer.height(), all)
}

// Rust-only (native): BASELINE. The C# path (`libraw_open_wfile`) holds the file without FILE_SHARE_DELETE for the whole decode:
// renaming it at the `opened` stage fails with sharing violation 32. This is the behaviour DV-24 removes.
#[test]
#[ignore = "native: needs libraw.dll and python (synthetic DNG)"]
fn native_path_open_blocks_rename_while_libraw_decodes() {
    if !native_ready() {
        return;
    }
    let root = TempRoot::new("raw-share-native-baseline");
    let Some(dir) = generate(&root) else { return };
    let path = dir.join("plain.dng");
    let probe = Arc::new(Probe::default());
    *probe.current.lock().unwrap() = Some(path.clone());
    let decoder = native_decoder(false).with_stage_observer(mover(&probe, STAGE_OPENED, "never"));
    let image = decoder
        .decode_with_cancel(
            &DecodeRequest::new(path.to_str().expect("utf-8"), 0),
            &CancelToken::none(),
        )
        .expect("decode");
    assert_eq!((image.pixel_width(), image.pixel_height()), (WIDTH, HEIGHT));
    let renamed = probe.renamed.lock().unwrap().clone();
    assert_eq!(
        renamed,
        vec![(STAGE_OPENED.to_owned(), Some(ERROR_SHARING_VIOLATION))],
        "libraw_open_wfile keeps the file open without FILE_SHARE_DELETE"
    );
    assert!(path.is_file(), "the file stayed in place");
}

// Rust-only (native): the goal. With the shared read, renaming the file at the `opened` stage and deleting it at the `unpacked`
// stage (LibRaw is still reading its buffer and demosaics afterwards) both succeed, and the decode still returns the image.
#[test]
#[ignore = "native: needs libraw.dll and python (synthetic DNG)"]
fn native_shared_file_read_lets_the_file_be_renamed_and_deleted_while_libraw_decodes() {
    if !native_ready() {
        return;
    }
    let root = TempRoot::new("raw-share-native-goal");
    let Some(dir) = generate(&root) else { return };
    let path = dir.join("plain.dng");
    let probe = Arc::new(Probe::default());
    *probe.current.lock().unwrap() = Some(path.clone());
    let decoder = native_decoder(true).with_stage_observer(mover(&probe, STAGE_OPENED, STAGE_UNPACKED));
    let image = decoder
        .decode_with_cancel(
            &DecodeRequest::new(path.to_str().expect("utf-8"), 0),
            &CancelToken::none(),
        )
        .expect("decode");
    assert_eq!((image.pixel_width(), image.pixel_height()), (WIDTH, HEIGHT));
    assert_eq!(
        probe.renamed.lock().unwrap().clone(),
        vec![(STAGE_OPENED.to_owned(), None)],
        "rename during decode"
    );
    assert_eq!(
        probe.deleted.lock().unwrap().clone(),
        vec![(STAGE_UNPACKED.to_owned(), None)],
        "delete during decode"
    );
    assert!(
        !path.exists() && !path.with_extension("moved").exists(),
        "the file is really gone"
    );
}

// Rust-only (native): the shared path decodes every synthetic DNG to the SAME pixels, size and flags as the C# path open
// (full decode and a small decode box). This is the oracle for "libraw_open_buffer behaves like libraw_open_wfile" on these files.
#[test]
#[ignore = "native: needs libraw.dll and python (synthetic DNG)"]
fn native_shared_file_read_decodes_the_same_pixels_as_the_path_open() {
    if !native_ready() {
        return;
    }
    let root = TempRoot::new("raw-share-native-pixels");
    let Some(dir) = generate(&root) else { return };
    let mut compared = 0;
    for name in [
        "plain.dng",
        "preview.dng",
        "subifd.dng",
        "swap.dng",
        "orient6.dng",
        "orient3.dng",
        "pair.dng",
    ] {
        let path = dir.join(name);
        let text = path.to_str().expect("utf-8");
        for decode_box in [DecodeBox::UNBOUNDED, DecodeBox::new(64, 64)] {
            let request = DecodeRequest::with_box(text, decode_box);
            let by_path = native_decoder(false)
                .decode_with_cancel(&request, &CancelToken::none())
                .expect("path");
            let by_bytes = native_decoder(true)
                .decode_with_cancel(&request, &CancelToken::none())
                .expect("shared");
            assert_eq!(
                bgra_of(by_path.as_ref()),
                bgra_of(by_bytes.as_ref()),
                "{name} {decode_box:?} pixels"
            );
            assert_eq!(by_path.downscaled(), by_bytes.downscaled(), "{name} downscaled");
            assert_eq!(
                by_path.original_width(),
                by_bytes.original_width(),
                "{name} original width"
            );
            assert_eq!(
                by_path.original_height(),
                by_bytes.original_height(),
                "{name} original height"
            );
            assert_eq!(by_path.orientation(), by_bytes.orientation(), "{name} orientation");
            compared += 1;
        }
    }
    assert_eq!(compared, 14);
}

// Rust-only (native): both opens fail with the same text for a file LibRaw cannot parse and for a missing file.
#[test]
#[ignore = "native: needs libraw.dll"]
fn native_shared_file_read_reports_the_same_errors_as_the_path_open() {
    if !native_ready() {
        return;
    }
    let root = TempRoot::new("raw-share-native-errors");
    let junk = root.join("junk.cr2");
    std::fs::write(&junk, vec![0x5Au8; 8192]).expect("write fixture");
    let missing = root.join("missing.cr2");
    for path in [&junk, &missing] {
        let request = DecodeRequest::new(path.to_str().expect("utf-8"), 0);
        let by_path = native_decoder(false)
            .decode_with_cancel(&request, &CancelToken::none())
            .expect_error("path");
        let by_bytes = native_decoder(true)
            .decode_with_cancel(&request, &CancelToken::none())
            .expect_error("shared");
        assert_eq!(kind_name(&by_path), kind_name(&by_bytes), "{}", path.display());
        assert_eq!(by_path.to_string(), by_bytes.to_string(), "{}", path.display());
    }
}
