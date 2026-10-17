//! Mutation-killing regression tests for W5-16c decoder.
mod support;

use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use pr_core::abstractions::SourceReadPriority;
use pr_core::sync::CancelToken;
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_libraw::decoder::{LibRawDecoder, MAX_QUEUED_PRELOAD_DECODES, find_end_of_image};
use pr_libraw::full_decode_gate::FullDecodeGate;
use pr_libraw::memory_guard::{RawBufferFamily, RawStructure, estimate_peak_bytes};
use support::{FakeApi, FakeCodec, Log, Script, WAIT_LIMIT, global_gate_lock, memory_source, request};

const MOSAIC: u32 = 0x9494_9494;

fn decode_with(script: Script, total: i64) -> Result<(), DecodeError> {
    let log = Log::default();
    let codec = Arc::new(FakeCodec {
        log: log.clone(),
        fail: false,
    });
    let decoder = LibRawDecoder::new(codec, FakeApi::arc(script, &log), memory_source(total, 0))
        .with_gate(FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES));
    decoder.decode(&request("C:\\x.raw")).map(|_| ())
}

fn base() -> Script {
    Script {
        width: 100,
        height: 100,
        raw_width: 100,
        raw_height: 100,
        ..Script::default()
    }
}

fn est_a() -> i64 {
    estimate_peak_bytes(100, 100, 100, 100, 100, 100, RawBufferFamily::Bayer)
}

#[test]
fn probe_forwards_raw_width_and_height() {
    assert!(decode_with(base(), est_a()).is_ok());
    let big = Script {
        raw_width: 1000,
        raw_height: 100,
        ..base()
    };
    assert!(decode_with(big, est_a()).is_err(), "raw_width must reach the estimate");
    let big = Script {
        raw_width: 100,
        raw_height: 1000,
        ..base()
    };
    assert!(decode_with(big, est_a()).is_err(), "raw_height must reach the estimate");
}

#[test]
fn probe_forwards_decoder_name() {
    let s = Script {
        decoder_name: Some("uncompressed_fp_dng_load_raw()".into()),
        ..base()
    };
    assert!(decode_with(s, est_a()).is_err(), "float DNG name must reach classify");
}

#[test]
fn probe_forwards_structure() {
    let s = Script {
        decoder_name: Some("lossless_dng_load_raw()".into()),
        structure: Some(RawStructure::new(3, MOSAIC)),
        ..base()
    };
    assert!(decode_with(s, est_a()).is_ok(), "bayer structure must reach classify");
}

#[test]
fn as_cancellable_debug_and_thumbnail_method() {
    let log = Log::default();
    let codec = Arc::new(FakeCodec {
        log: log.clone(),
        fail: false,
    });
    let d = LibRawDecoder::new(codec, FakeApi::arc(base(), &log), memory_source(0, 0));
    assert!(ImageDecoder::as_cancellable(&d).is_some());
    assert!(format!("{d:?}").contains("LibRawDecoder"));
    assert_eq!(
        d.read_jpeg_thumbnail("C:\\x.orf", &CancelToken::none()).unwrap(),
        vec![0xFF, 0xD8, 0xFF, 0xD9]
    );
}

#[test]
fn trim_fast_path_wins_over_structure() {
    // C#: strip trailing 00, ends with FF D9 => keep 7 (structure walk alone would stop at 4).
    let b = [0xFF, 0xD8, 0xFF, 0xD9, 0x12, 0xFF, 0xD9, 0x00, 0x00];
    assert_eq!(find_end_of_image(&b).unwrap(), 7);
    let b = [0xFF, 0xD8, 0xFF, 0xD9, 0x12, 0xFF, 0xD9, 0xFF, 0x00];
    assert_eq!(find_end_of_image(&b).unwrap(), 7);
}

#[test]
fn global_queued_waiters_counts_both_lanes() {
    let _guard = global_gate_lock();
    let log = Log::default();
    let mk = |core: pr_libraw::decoder::CoreOverride| {
        let codec = Arc::new(FakeCodec {
            log: log.clone(),
            fail: false,
        });
        LibRawDecoder::new(codec, FakeApi::arc(base(), &log), memory_source(0, 0)).with_core_override(core)
    };
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = std::sync::Mutex::new(release_rx);
    let holder = mk(Arc::new(move |_r, _l, _c| {
        entered_tx.send(()).unwrap();
        let _ = release_rx.lock().unwrap().recv_timeout(WAIT_LIMIT);
        Err(DecodeError::InvalidOperation {
            message: "holder".into(),
        })
    }));
    let h = thread::spawn(move || holder.decode(&request("C:\\a.raw")).map(|_| ()));
    entered_rx.recv_timeout(WAIT_LIMIT).unwrap();
    let mut waiters = Vec::new();
    for priority in [SourceReadPriority::Viewer, SourceReadPriority::Preload] {
        let d = mk(Arc::new(|_r, _l, _c| {
            Err(DecodeError::InvalidOperation { message: "w".into() })
        }));
        let mut req = request("C:\\b.raw");
        req.priority = priority;
        waiters.push(thread::spawn(move || d.decode(&req).map(|_| ())));
    }
    let start = Instant::now();
    let mut seen = 0;
    while start.elapsed() < WAIT_LIMIT {
        seen = LibRawDecoder::full_decode_queued_waiters();
        if seen == 2 {
            break;
        }
        thread::yield_now();
    }
    release_tx.send(()).unwrap();
    let _ = h.join();
    for w in waiters {
        let _ = w.join();
    }
    assert_eq!(seen, 2);
}
