//! W8 (2026-11): fail-fast killers and equivalence proofs for the pr-turbojpeg `decoder.rs` survivors of master
//! (661:27 hung `decoder_pure` under mutation and is now bounded there; 492:22 timed out in the full run, cause unconfirmed:
//! the debug build panics on the `usize` underflow first, so the timeout may come from a build without overflow checks;
//! 1187:47 and 1187:62 are equivalent). No `turbojpeg.dll`, no golden file, no sleep.
//!
//! Every call that a mutant can turn into an endless loop runs on a worker thread and is awaited with `recv_timeout`; the readers
//! below additionally stop themselves after a call budget, so a mutated build ends the loop (with a wrong answer) instead of
//! leaving a spinning thread behind.
//!
//! Equivalence arguments (no test can kill these, the reachable domain is enumerated):
//!  * 172:9 `NativeDecompressors::shared -> Default::default()`: single-valued domain (see `mut2_n4_decoder_killers.rs`).
//!  * 382:27 `width > i64::MAX / 4` -> `>=`: width is exactly `i64::MAX / 4 = 2^61 - 1` for no `i32` triple (Mersenne prime,
//!    see `mut3_decoder_proofs.rs`).
//!  * 519:46 `be16` `|` -> `^`: `(hi << 8)` has its low 8 bits clear and `lo < 256`, the operands are bit-disjoint.
//!  * 580:23 / 580:55 `scan_header` stop condition `||` -> `&&`: the mutant never stops early (the first clause needs
//!    `!want_icc && has_icc`, impossible because `has_icc` is only set when `want_icc`; likewise for Exif), and the loop body
//!    is guarded by `!has_icc` / `!found_exif` and the ICC and Exif branches test disjoint markers (E2 / E1), so a walk that
//!    continues past the stop point can never change an answer. Only the work differs.
//!  * 698:28 `len < length` -> `<=`: `limit = min(length, ..) <= length`, so `len < limit` already implies `len < length`; the
//!    conjunct is redundant and `<=` only matters where `len == length`, where `len < limit` is false anyway.
//!  * 698:57 `len < limit` -> `<=`: at `len == limit` (with `len < length`) the body computes `new_len = min(limit, 2 * len)
//!    = limit = len` (`len >= 1` because `header_needs_more_data` is false for an empty buffer) and breaks on `new_len <=
//!    old_len` before any read or growth: same result, no read.
//!  * 1187:47 / 1187:62 `orient: apply && orientation > 1` (`&&` -> `||`, `>` -> `>=`): `plan.orient` is only consumed by
//!    `apply_orientation`, which is the identity (same buffer) for every orientation outside 2..=8; the mutants only differ
//!    for `orientation <= 1` (where `orientation` is forced to 1 when `apply_orientation` is false). Pinned below.

use std::io::{self, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use pr_imaging::decode_types::{DecodeBox, DecodeRequest, PixelLease, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, PlatformImageCodec};
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::PixelBuffer;
use pr_test_support::golden;
use pr_turbojpeg::decoder::{
    Decompressor, DecompressorFactory, EnglishTurboJpegTexts, FineScaleError, FineScaler, SegmentStep, SourceReadError,
    TurboJpegDecoder, grow_header_area, header_needs_more_data, read_all_bytes, scan_header, try_read_segment,
};
use pr_turbojpeg::native::{TjParam, TjPixelFormat, TjScalingFactor};

/// Runs `work` on a worker thread and fails (instead of hanging the test binary) when it does not finish in 20 s. A panic of
/// the worker (for example a `usize` underflow of a mutated walk) drops the sender and fails at once.
fn within<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(work());
    });
    match receiver.recv_timeout(Duration::from_secs(20)) {
        Ok(value) => value,
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("did not finish within 20 s (an endless loop)"),
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the worker panicked"),
    }
}

fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let length = u16::try_from(payload.len() + 2).expect("segment fits");
    let mut bytes = vec![0xFF, marker];
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

// ------------------------------------------------------------------------------------------------ 492:22

/// decoder.rs:492:22 (`*offset += 2 + length` -> `2 - length`): ONE call of `try_read_segment` must move the offset past the
/// whole segment. With the mutant a segment of length 2 (no payload) leaves the offset where it is (the callers loop for
/// ever), and a longer one underflows `usize` in a debug build (panic) or wraps to a huge offset in release. Single step, so
/// the check itself cannot loop.
#[test]
fn one_step_of_the_segment_walk_advances_past_the_whole_segment() {
    for payload_len in [0usize, 1, 2, 3, 14, 250] {
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend(segment(0xE0, &vec![0x5A; payload_len]));
        let end = jpeg.len();
        jpeg.extend(segment(0xE3, &[1]));
        let (offset, payload) = within(move || {
            let mut offset = 2usize;
            let step = try_read_segment(&jpeg, &mut offset);
            match step {
                SegmentStep::Segment { marker, payload } => {
                    assert_eq!(marker, 0xE0);
                    (offset, payload.to_vec())
                }
                SegmentStep::End { .. } => panic!("the first segment is complete"),
            }
        });
        assert_eq!(offset, end, "payload length {payload_len}");
        assert_eq!(payload, vec![0x5A; payload_len], "payload length {payload_len}");
    }
}

/// The same property through the loops that consume it: a run of empty (length 2) and short segments in front of the Exif
/// block is walked to its end in finite time and the block is found; the header of such a file is complete.
#[test]
fn a_run_of_empty_segments_is_walked_to_the_exif_block_in_bounded_time() {
    let mut jpeg = vec![0xFF, 0xD8];
    for _ in 0..8 {
        jpeg.extend(segment(0xE0, &[]));
        jpeg.extend(segment(0xE4, &[7, 7, 7]));
    }
    jpeg.extend(segment(0xE1, &[b"Exif\0\0".as_slice(), &[0x4D, 0x4D]].concat()));
    jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
    let (tiff, needs_more) = within(move || {
        (
            scan_header(&jpeg, true, true).exif_tiff.to_vec(),
            header_needs_more_data(&jpeg),
        )
    });
    assert_eq!(tiff, vec![0x4D, 0x4D]);
    assert!(!needs_more, "SOS closes the header");
}

// ------------------------------------------------------------------------------------------------ 661:27

/// A reader that fails with `kind` on EVERY call. After `budget` calls it reports end of stream so that a mutated retry loop
/// terminates by itself (with a wrong answer) rather than spinning for ever.
struct AlwaysFails {
    kind: io::ErrorKind,
    calls: Arc<AtomicUsize>,
    budget: usize,
}

impl Read for AlwaysFails {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        if self.calls.fetch_add(1, Ordering::SeqCst) >= self.budget {
            return Ok(0);
        }
        Err(io::Error::new(self.kind, "persistent failure"))
    }
}

/// `n` times `Interrupted`, then serves 0x7E bytes.
struct InterruptedThenData {
    interrupts: usize,
    calls: Arc<AtomicUsize>,
}

impl Read for InterruptedThenData {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.interrupts > 0 {
            self.interrupts -= 1;
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        buffer.fill(0x7E);
        Ok(buffer.len())
    }
}

/// decoder.rs:661:27 (`fill_buffer`: `error.kind() == Interrupted` -> `true`): a persistent error that is not `Interrupted` is
/// returned at the FIRST failing read as `Io` with its kind (the mutant retries it until the stream "ends" and then reports
/// success with 0 bytes). Run through `read_all_bytes` and through `grow_header_area` for several kinds.
#[test]
fn a_non_interrupted_read_error_is_returned_at_once_and_never_retried() {
    for kind in [
        io::ErrorKind::Other,
        io::ErrorKind::UnexpectedEof,
        io::ErrorKind::BrokenPipe,
        io::ErrorKind::WouldBlock,
        io::ErrorKind::PermissionDenied,
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let result = within({
            let calls = Arc::clone(&calls);
            move || {
                read_all_bytes(
                    &mut AlwaysFails {
                        kind,
                        calls,
                        budget: 1000,
                    },
                    64,
                )
            }
        });
        match result {
            Err(SourceReadError::Io(error)) => assert_eq!(error.kind(), kind),
            other => panic!("{kind:?}: expected Io, got {other:?}"),
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "{kind:?}: read_all_bytes retried a hard error"
        );

        // A header that needs more data, so the loop reads from the failing reader.
        let head = [vec![0xFF, 0xD8], segment(0xE0, b"JFIF\0")].concat();
        let calls = Arc::new(AtomicUsize::new(0));
        let result = within({
            let calls = Arc::clone(&calls);
            move || {
                grow_header_area(
                    &mut AlwaysFails {
                        kind,
                        calls,
                        budget: 1000,
                    },
                    4096,
                    head,
                    4096,
                )
            }
        });
        match result {
            Err(SourceReadError::Io(error)) => assert_eq!(error.kind(), kind),
            other => panic!("{kind:?}: expected Io, got {:?}", other.map(|b| b.len())),
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "{kind:?}: grow_header_area retried a hard error"
        );
    }
}

/// The retry arm itself stays: `Interrupted` any number of times is retried and the data is delivered complete.
#[test]
fn interrupted_reads_are_retried_until_the_data_arrives() {
    for interrupts in [1usize, 2, 7, 50] {
        let calls = Arc::new(AtomicUsize::new(0));
        let bytes = within({
            let calls = Arc::clone(&calls);
            move || read_all_bytes(&mut InterruptedThenData { interrupts, calls }, 16).expect("retried")
        });
        assert_eq!(bytes, vec![0x7E; 16], "{interrupts} interrupts");
        assert_eq!(calls.load(Ordering::SeqCst), interrupts + 1, "{interrupts} interrupts");
    }
}

// ------------------------------------------------------------------------------------------------ 1187:47 / 1187:62

/// `(width, height, id, bytes)` of one buffer handed to the codec.
type Handed = (i32, i32, i64, Vec<u8>);

#[derive(Default)]
struct Written {
    /// `(width, height, id, bytes)` of every buffer handed to the codec.
    handed: Mutex<Vec<Handed>>,
}

/// Records what the decoder hands to the codec, then delegates to the real pixel codec.
struct RecordingCodec {
    inner: Arc<dyn PlatformImageCodec>,
    written: Arc<Written>,
}

impl PlatformImageCodec for RecordingCodec {
    fn name(&self) -> &'static str {
        "recording"
    }
    fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
        self.written.handed.lock().unwrap().push((
            pixels.width(),
            pixels.height(),
            pixels.id(),
            pixels.bytes().expect("live buffer").to_vec(),
        ));
        self.inner.image_from_pixels(pixels)
    }
    fn to_pixels<'a>(&self, platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
        self.inner.to_pixels(platform_image)
    }
}

/// Reports `width x height`, accepts every call and fills the destination with a position-dependent pattern.
struct PatternProbe {
    width: i32,
    height: i32,
}

impl Decompressor for PatternProbe {
    fn set(&mut self, _param: TjParam, _value: i32) -> Result<i32, DecodeError> {
        Ok(0)
    }
    fn get(&mut self, param: TjParam) -> Result<i32, DecodeError> {
        Ok(match param {
            TjParam::JpegWidth => self.width,
            TjParam::JpegHeight => self.height,
            _ => 0,
        })
    }
    fn decompress_header(&mut self, _jpeg: &[u8]) -> Result<i32, DecodeError> {
        Ok(0)
    }
    fn set_scaling_factor(&mut self, _factor: TjScalingFactor) -> Result<i32, DecodeError> {
        Ok(0)
    }
    fn decompress8(
        &mut self,
        _jpeg: &[u8],
        dst: &mut [u8],
        _pitch: i32,
        _pixel_format: TjPixelFormat,
    ) -> Result<i32, DecodeError> {
        for (index, byte) in dst.iter_mut().enumerate() {
            *byte = u8::try_from(index % 251).expect("below 251");
        }
        Ok(0)
    }
    fn error_message(&mut self) -> Option<String> {
        None
    }
}

struct NoFineScale;

impl FineScaler for NoFineScale {
    fn resize(&self, _source: &PixelBuffer, _width: i32, _height: i32) -> Result<PixelBuffer, FineScaleError> {
        Err(FineScaleError::Failed(DecodeError::Other {
            message: "no fine scale expected".to_owned(),
        }))
    }
}

/// Decodes the 64 x 48 fixture (reported size) with `apply_orientation` and `orientation` given; returns what reached the
/// codec: `(width, height, bytes)` and the orientation on the decoded image.
fn decode(apply_orientation: bool, orientation: Option<i32>) -> (i32, i32, Vec<u8>, i32) {
    let path = golden::repo_root().join("tests/fixtures/turbojpeg/parity-64x48.jpg");
    let jpeg = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let written = Arc::new(Written::default());
    let factory: Arc<dyn DecompressorFactory> = Arc::new(|| -> Result<Box<dyn Decompressor>, DecodeError> {
        Ok(Box::new(PatternProbe { width: 64, height: 48 }))
    });
    let memory: Arc<dyn MemoryInfoSource> = Arc::new(|| MemoryInfo {
        total_available: 1 << 36,
        load: 0,
    });
    let decoder = TurboJpegDecoder::new(
        Arc::new(RecordingCodec {
            inner: PixelBufferImageCodec::shared(),
            written: Arc::clone(&written),
        }),
        Arc::new(NoFineScale),
        memory,
        factory,
        Arc::new(EnglishTurboJpegTexts),
    );
    let mut request = DecodeRequest::with_box("p.jpg", DecodeBox::new(0, 0))
        .with_bytes(jpeg.into())
        .with_apply_orientation(apply_orientation);
    if let Some(value) = orientation {
        request = request.with_source_orientation(value);
    }
    let image = decoder.decode(&request).expect("decodes");
    let handed = written.handed.lock().unwrap();
    assert_eq!(handed.len(), 1);
    let (width, height, _id, bytes) = handed[0].clone();
    assert_eq!((image.pixel_width(), image.pixel_height()), (width, height));
    (width, height, bytes, image.orientation())
}

/// decoder.rs:1187:47 (`&&` -> `||`) and 1187:62 (`>` -> `>=`) in `FinishPlan { orient: apply && orientation > 1, .. }`: both
/// EQUIVALENT. They differ only for `orientation <= 1` with `apply_orientation`, where `apply_orientation()` returns the very
/// buffer it was given. Pinned end to end: for orientation 0, 1, 9, -1 and `i32::MAX` the pixels that reach the codec are the
/// untouched decoder output, with and without `apply_orientation`; a real orientation (6) transposes the buffer, 3 reverses
/// it, and with `apply_orientation == false` nothing is applied whatever orientation the caller reports.
#[test]
fn orientation_outside_two_to_eight_hands_the_decoded_pixels_to_the_codec_untouched() {
    let (_, _, untouched, _) = decode(false, None);
    assert_eq!(untouched.len(), 64 * 48 * 4);
    for orientation in [0, 1, 9, -1, i32::MAX] {
        let (width, height, bytes, reported) = decode(true, Some(orientation));
        assert_eq!((width, height), (64, 48), "orientation {orientation}");
        assert_eq!(bytes, untouched, "orientation {orientation}");
        assert_eq!(reported, orientation, "orientation {orientation}");
    }
    // Not applied: the caller's orientation is ignored (reported as 1).
    for orientation in [1, 6] {
        let (width, height, bytes, reported) = decode(false, Some(orientation));
        assert_eq!((width, height, &bytes, reported), (64, 48, &untouched, 1));
    }
    // Applied: 6 swaps the axes and moves pixels, 3 keeps the size and reverses the pixels.
    let (width, height, bytes, reported) = decode(true, Some(6));
    assert_eq!((width, height, reported), (48, 64, 6));
    assert_ne!(bytes, untouched);
    let (width, height, bytes, reported) = decode(true, Some(3));
    assert_eq!((width, height, reported), (64, 48, 3));
    assert_ne!(bytes, untouched);
}
