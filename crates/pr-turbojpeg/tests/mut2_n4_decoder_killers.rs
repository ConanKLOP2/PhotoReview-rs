//! N4 (2026-11-05): killers and proofs for the survivors and timeouts of the `pr_turbojpeg::decoder` mutation run
//! (`decoder.rs` 172:9, 178:9, 382:27, 492:22, 519:46, 580:23, 580:55, 611:9, 661:27, 992:9). Hand-written, no golden file, no
//! sleep on the passing path. Loops that a mutant turns into endless loops run on a bounded thread, so such a mutant FAILS in
//! seconds instead of timing the harness out.

use std::io::{self, Read};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::PixelBuffer;
use pr_test_support::golden;
use pr_turbojpeg::decoder::{
    EXIF_SIGNATURE, EnglishTurboJpegTexts, FineScaleError, ICC_SIGNATURE, NativeDecompressors, SegmentStep,
    SourceReadError, TurboJpegDecoder, calculate_output_buffer, grow_header_area, header_needs_more_data, scan_header,
    try_read_segment,
};
use pr_turbojpeg::native::{DLL_NAME, TjScalingFactor, TurboJpegLibrary};

/// Runs `work` on its own thread; the test fails when it has not finished within `seconds`.
fn bounded<T: Send + 'static>(seconds: u64, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(work());
    });
    receiver
        .recv_timeout(Duration::from_secs(seconds))
        .unwrap_or_else(|_| panic!("did not finish within {seconds} s (an endless loop)"))
}

// ------------------------------------------------------------------------------------------ calculate_output_buffer

/// decoder.rs:386 (`height > i64::MAX / stride.max(1)`) sits exactly on a reachable boundary: width 2^31-1 gives stride 2^33-4
/// and `i64::MAX / stride == 2^30`. At height 2^30 the refusal must print the exact product (2^63 - 2^32), one row more
/// saturates to i64::MAX, one row less is the plain product. (The mutation run already caught the operators of that line; this
/// pins the boundary itself.)
#[test]
fn output_buffer_length_at_the_exact_overflow_boundary_is_the_product_not_the_saturated_value() {
    let width = i32::MAX;
    let stride = i128::from(width) * 4;
    let boundary = i128::from(i64::MAX) / stride;
    assert_eq!(boundary, 1 << 30, "the premise of the test: the boundary row count");
    for (height, expected) in [
        (i32::try_from(boundary).expect("fits") - 1, stride * (boundary - 1)),
        (i32::try_from(boundary).expect("fits"), stride * boundary),
        (i32::try_from(boundary).expect("fits") + 1, i128::from(i64::MAX)),
    ] {
        let refusal =
            calculate_output_buffer(width, height, TjScalingFactor::ONE).expect_err("far above i32::MAX bytes");
        assert_eq!(i128::from(refusal.buffer_length), expected, "height {height}");
        assert_eq!(refusal.width, i64::from(width));
        assert_eq!(refusal.height, i64::from(height));
    }
}

/// decoder.rs:382:27 `width > i64::MAX / 4` -> `>=` / `==`: the operators differ only when the scaled width is EXACTLY
/// `T = i64::MAX / 4 = 2^61 - 1`. The scaled width is `(dim * num + denom - 1) / denom` for `i32` `dim`, `num`, `denom`.
///  * `denom >= 3`: the largest possible numerator `(2^31-1)^2` divided by 3 is already below `T` (checked here for the
///    extreme corner; the value only falls as `denom` grows and as `dim`/`num` shrink).
///  * `denom <= 2`: `dim * num` would have to equal `T` (denom 1), `2T - 1` or `2T` (denom 2); every divisor `a <= 2^31 - 1` of
///    each target is tried below and the cofactor never fits an `i32`.
///
/// The 2^31 trial divisions per target are slow in a debug build, so this test is `#[ignore]`d; run it with `--release`.
#[test]
#[ignore = "slow: 3 x 2^31 trial divisions, run with --release"]
fn no_i32_dimension_and_factor_scale_to_exactly_the_saturation_threshold() {
    let t = i64::MAX / 4;
    for denom in [3, 4, 5, 1000, i32::MAX] {
        let corner = TjScalingFactor::new(i32::MAX, denom)
            .scaled(i32::MAX)
            .expect("denominator is positive");
        assert!(corner < t, "denom {denom}: {corner}");
    }
    assert_eq!(
        TjScalingFactor::new(i32::MAX, 1).scaled(i32::MAX),
        Some(i64::from(i32::MAX) * i64::from(i32::MAX))
    );
    for target in [t, 2 * t - 1, 2 * t] {
        for a in 1..=i64::from(i32::MAX) {
            if target % a == 0 {
                assert!(
                    target / a > i64::from(i32::MAX),
                    "target {target} = {a} * {}",
                    target / a
                );
            }
        }
    }
}

// ------------------------------------------------------------------------------------------ NativeDecompressors

/// decoder.rs:172:9 `NativeDecompressors::shared -> Self` replaced by `Default::default()`: EQUIVALENT. `shared()` is
/// `Self { library: None }` and the derived `Default` is `library: None`; the struct has this one field and `Option` has one
/// `None` value, so the domain is a single value. Proven by run on the observable (Debug) form, which shows the field.
#[test]
fn shared_equals_default_and_holds_no_library() {
    let shared = format!("{:?}", NativeDecompressors::shared());
    assert_eq!(shared, format!("{:?}", NativeDecompressors::default()));
    assert!(shared.contains("library: None"), "{shared}");
}

/// decoder.rs:178:9 `NativeDecompressors::with_library -> Self` replaced by `Default::default()`. The existing native tests
/// survive that mutant when `turbojpeg.dll` also sits next to the test executable (the process-wide fallback loads the same
/// DLL), so only the stored library tells the two apart: it must be `Some`.
#[test]
#[ignore = "native: needs native/x64/turbojpeg.dll"]
fn with_library_keeps_the_given_library() {
    let path = golden::repo_root().join("native").join("x64").join(DLL_NAME);
    assert!(path.is_file(), "missing {}: stage the pinned DLL first", path.display());
    let library = TurboJpegLibrary::load(&path).unwrap_or_else(|e| panic!("load {}: {e}", path.display()));
    let text = format!("{:?}", NativeDecompressors::with_library(library));
    assert!(text.contains("library: Some("), "{text}");
}

// ------------------------------------------------------------------------------------------ header walk

fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let length = u16::try_from(payload.len() + 2).expect("segment fits");
    let mut bytes = vec![0xFF, marker];
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

/// decoder.rs:519:46 (`be16`: `(hi << 8) | lo`, `|` -> `^`): the operands occupy disjoint bits, so the operators agree. Proven
/// over EVERY 16-bit segment length through the real walk: an APP0 segment of that length is skipped exactly (the Exif block
/// behind it is found), and a length below 2 ends the walk.
#[test]
fn every_sixteen_bit_segment_length_is_skipped_exactly() {
    bounded(60, || {
        let exif = segment(0xE1, &[b"Exif\0\0".as_slice(), &[0x49, 0x49]].concat());
        for length in 0..=u16::MAX {
            let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
            jpeg.extend_from_slice(&length.to_be_bytes());
            if length >= 2 {
                jpeg.resize(4 + usize::from(length), 0x55);
            }
            jpeg.extend_from_slice(&exif);
            let found = scan_header(&jpeg, false, true).exif_tiff;
            if length >= 2 {
                assert_eq!(found, &[0x49, 0x49], "segment length {length}");
            } else {
                assert!(found.is_empty(), "segment length {length}");
            }
        }
    });
}

/// decoder.rs:492:22 (`*offset += 2 + length` -> `2 - length`): a segment of length exactly 2 (no payload) would not advance
/// the walk and loop forever. The walk over empty segments ends with the documented answer, within a bound.
#[test]
fn empty_segments_advance_the_walk() {
    let jpeg: Vec<u8> = [
        vec![0xFF, 0xD8],
        segment(0xE0, &[]),
        segment(0xE3, &[]),
        segment(0xE1, &[b"Exif\0\0".as_slice(), &[0x4D, 0x4D]].concat()),
    ]
    .concat();
    let found = bounded(10, move || scan_header(&jpeg, true, true).exif_tiff.to_vec());
    assert_eq!(found, vec![0x4D, 0x4D]);
}

/// The segment kinds of the sequences below.
fn kinds() -> Vec<Vec<u8>> {
    vec![
        segment(0xE0, b"JFIF\0"),
        segment(0xE2, &[ICC_SIGNATURE.as_slice(), &[1, 1]].concat()),
        segment(0xE2, b"other"),
        segment(0xE1, &[EXIF_SIGNATURE.as_slice(), &[0x49, 0x49, 0x2A, 0x00]].concat()),
        segment(0xE1, &EXIF_SIGNATURE),
        segment(0xE1, b"xmp"),
        segment(0xDB, &[0; 4]),
    ]
}

/// What `scan_header` must return, computed by walking EVERY segment with the public `try_read_segment` and no early exit:
/// the first ICC-signed APP2 sets `has_icc`; the first Exif-signed APP1 gives its TIFF part.
fn walk_everything(jpeg: &[u8], want_icc: bool, want_exif: bool) -> (bool, Vec<u8>) {
    let mut offset = 2;
    let (mut icc, mut exif, mut found) = (false, Vec::new(), false);
    while let SegmentStep::Segment { marker, payload } = try_read_segment(jpeg, &mut offset) {
        if want_icc && !icc && marker == 0xE2 && payload.starts_with(&ICC_SIGNATURE) {
            icc = true;
        } else if want_exif && !found && marker == 0xE1 && payload.starts_with(&EXIF_SIGNATURE) {
            exif = payload[EXIF_SIGNATURE.len()..].to_vec();
            found = true;
        }
    }
    (icc, exif)
}

/// decoder.rs:580:23 and 580:55 (`||` -> `&&` in the stop condition of `scan_header`). The stop is an optimisation: the loop
/// body never changes an answer that is already known (`!has_icc` / `!found_exif` guard it), so scanning on gives the same
/// result. Proven over EVERY sequence of 0..=6 segments from 7 kinds (about 137 000 files) for all four `want_*` pairs: the
/// result equals the full no-early-exit walk. The work done differs (not observable through a `&[u8]` API).
#[test]
fn scan_header_result_equals_the_full_walk_for_every_short_segment_sequence() {
    bounded(60, || {
        let kinds = kinds();
        for count in 0..=6u32 {
            for n in 0..kinds.len().pow(count) {
                let mut jpeg = vec![0xFF, 0xD8];
                let mut rest = n;
                for _ in 0..count {
                    jpeg.extend_from_slice(&kinds[rest % kinds.len()]);
                    rest /= kinds.len();
                }
                jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
                for (want_icc, want_exif) in [(false, false), (true, false), (false, true), (true, true)] {
                    let scan = scan_header(&jpeg, want_icc, want_exif);
                    let (icc, exif) = walk_everything(&jpeg, want_icc, want_exif);
                    assert_eq!(
                        (scan.has_icc, scan.exif_tiff.to_vec()),
                        (icc, exif),
                        "{jpeg:02X?} {want_icc} {want_exif}"
                    );
                }
            }
        }
    });
}

// ------------------------------------------------------------------------------------------ errors and readers

/// decoder.rs:611:9 `Display for SourceReadError` -> `Ok(Default::default())`: each variant prints its message.
#[test]
fn source_read_error_display_names_the_cause() {
    assert_eq!(
        SourceReadError::TooLarge { length: 123 }.to_string(),
        "JPEG source is too large to read into memory: 123 bytes."
    );
    assert_eq!(
        SourceReadError::OutOfMemory { bytes: 9 }.to_string(),
        "Insufficient memory to continue the execution of the program."
    );
    let io_error = io::Error::other("disk on fire");
    assert_eq!(SourceReadError::Io(io_error).to_string(), "disk on fire");
}

/// A JPEG head that always needs more data (SOI, then one APP0 segment, no frame header yet).
fn jpeg_head() -> Vec<u8> {
    [vec![0xFF, 0xD8], segment(0xE0, b"JFIF\0")].concat()
}

struct Failing;

impl Read for Failing {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("boom"))
    }
}

/// Interrupts once, then serves zeros.
struct InterruptedOnce(bool);

impl Read for InterruptedOnce {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if !self.0 {
            self.0 = true;
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        buffer.fill(0);
        Ok(buffer.len())
    }
}

/// decoder.rs:661:27 (`fill_buffer`: the `Interrupted` retry guard -> `true`): only `Interrupted` is retried. A reader that
/// fails for another reason surfaces as `Io` at once (the mutant retried it forever), and a single `Interrupted` is retried.
#[test]
fn growing_the_header_area_retries_only_interrupted_reads() {
    let head = jpeg_head();
    let length = 4096i64;
    let failed = bounded(10, {
        let head = head.clone();
        move || grow_header_area(&mut Failing, length, head, length)
    });
    match failed {
        Err(SourceReadError::Io(error)) => assert_eq!(error.to_string(), "boom"),
        other => panic!("expected the read error, got {:?}", other.map(|b| b.len())),
    }
    let grown = bounded(10, move || {
        grow_header_area(&mut InterruptedOnce(false), length, head, length).expect("the interrupted read is retried")
    });
    assert!(grown.len() > jpeg_head().len(), "the buffer grew: {}", grown.len());
}

/// decoder.rs:992:9 `Debug for TurboJpegDecoder` -> `Ok(Default::default())`: the output names the type.
#[test]
fn turbojpeg_decoder_debug_names_the_type() {
    let memory: Arc<dyn MemoryInfoSource> = Arc::new(|| MemoryInfo {
        total_available: 1 << 30,
        load: 0,
    });
    let decoder = TurboJpegDecoder::native(
        PixelBufferImageCodec::shared(),
        Arc::new(
            |_: &PixelBuffer, _: i32, _: i32| -> Result<PixelBuffer, FineScaleError> {
                Err(FineScaleError::Failed(DecodeError::Other {
                    message: "unused".to_owned(),
                }))
            },
        ),
        memory,
        Arc::new(EnglishTurboJpegTexts),
    );
    let text = format!("{decoder:?}");
    assert!(text.starts_with("TurboJpegDecoder"), "{text}");
}

// ------------------------------------------------------------------------------------------ grow_header_area

/// decoder.rs:698:65 (`&&` -> `||` between `len < limit` and `header_needs_more_data`): a header that is already complete (the
/// frame header sits inside the buffer) must not be grown, and the reader must not be touched. With `||` the loop grows the
/// buffer anyway and the failing reader surfaces as an error.
#[test]
fn a_complete_header_is_returned_unchanged_without_reading() {
    // SOI, a baseline SOF0 segment and a complete SOS header: nothing more is needed to parse the frame.
    let mut head = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x30, 0x00, 0x40];
    head.extend_from_slice(&[0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
    head.extend_from_slice(&[
        0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
    ]);
    assert!(!header_needs_more_data(&head), "the premise: the header is complete");
    let length = 1_000_000i64;
    let grown = bounded(10, {
        let head = head.clone();
        move || grow_header_area(&mut Failing, length, head, length)
    });
    assert_eq!(grown.expect("nothing is read"), head);
}

/// A reader over `data`, served from its start.
struct Serve<'a>(&'a [u8]);

impl Read for Serve<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let n = buffer.len().min(self.0.len());
        buffer[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

/// The documented loop of `GrowHeaderArea`, written without looking at the Rust body.
fn reference_grow(mut buffer: Vec<u8>, mut tail: &[u8], length: i64, cap: i64) -> Vec<u8> {
    let limit = length.min(cap).min(pr_turbojpeg::decoder::MAX_SOURCE_BYTES);
    while (buffer.len() as i64) < length && (buffer.len() as i64) < limit && header_needs_more_data(&buffer) {
        let old = buffer.len();
        let new = limit.min(2 * old as i64) as usize;
        if new <= old {
            break;
        }
        let take = (new - old).min(tail.len());
        buffer.extend_from_slice(&tail[..take]);
        tail = &tail[take..];
        if take < new - old {
            return buffer;
        }
    }
    buffer
}

/// decoder.rs:698:28 and 698:57 (`<` -> `<=`) and 698:37 (`&&` -> `||` between the first two terms): `limit <= length`, so
/// `len < length` is implied by `len < limit`, and when `len` has reached `limit` the next step cannot grow (`new <= old` ends
/// the loop). Proven by a differential run against the documented loop over every `length` 0..=40, 17 caps, every start size
/// 0..=min(length + 2, 26), start contents that need more data and ones that do not, and a file that is complete or cut short.
#[test]
fn growing_the_header_area_equals_the_documented_loop_on_a_small_exhaustive_domain() {
    let head = jpeg_head();
    let mut file = head.clone();
    file.extend((0..200u32).map(|i| (i % 7) as u8));
    let caps = [0i64, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 16, 17, 20, 33, 41, 100];
    let mut cases = 0usize;
    for length in 0..=40i64 {
        for &cap in &caps {
            for start in 0..=usize::try_from(length + 2).unwrap().min(26) {
                for pattern in 0..2 {
                    let mut buffer: Vec<u8> = file.iter().copied().take(start).collect();
                    buffer.resize(start, 0);
                    if pattern == 1 {
                        buffer.iter_mut().skip(2).for_each(|b| *b = 0x55);
                    }
                    for complete in [true, false] {
                        let remaining = usize::try_from((length - start as i64).max(0)).unwrap();
                        let served = if complete { remaining } else { remaining / 2 };
                        let tail: Vec<u8> = (0..served).map(|i| (i % 11) as u8).collect();
                        let got =
                            grow_header_area(&mut Serve(&tail), length, buffer.clone(), cap).expect("in-memory reads");
                        let want = reference_grow(buffer.clone(), &tail, length, cap);
                        assert_eq!(
                            got, want,
                            "length {length} cap {cap} start {start} pattern {pattern} complete {complete}"
                        );
                        cases += 1;
                    }
                }
            }
        }
    }
    assert!(cases > 10_000, "{cases}");
}
