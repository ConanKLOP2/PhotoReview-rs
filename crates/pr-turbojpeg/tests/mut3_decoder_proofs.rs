//! MUT3 (2026-11-06), task P2: killers and whole-domain proofs for the pr-turbojpeg `decoder.rs` survivors that N4 left open
//! (382:27, 580:23, 580:55, 698:28, 698:57, 1126:47, 1126:62). No `turbojpeg.dll`, no golden file, no sleep. One test is
//! `#[ignore]`d because it reserves 2 GiB of address space (lazily zeroed pages).
//!
//! Vocabulary used in the evidence file: a mutant is PROVEN when the test passes under it and the divergent state is enumerated
//! (value by value, or class by class where the code only branches on the class), KILLED when the test fails under it.

use std::io::{self, Read};
use std::sync::{Arc, Mutex};
use std::thread;

use pr_imaging::decode_types::{DecodeBox, DecodeRequest};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::{PixelBuffer, PixelLayout, apply_orientation};
use pr_test_support::golden;
use pr_turbojpeg::decoder::{
    Decompressor, DecompressorFactory, EXIF_SIGNATURE, EnglishTurboJpegTexts, FineScaleError, FineScaler,
    ICC_SIGNATURE, MAX_SOURCE_BYTES, SUPPORTED_SCALING_FACTORS, SegmentStep, TurboJpegDecoder, calculate_output_buffer,
    grow_header_area, header_needs_more_data, scaled_size, scan_header, try_read_segment,
};
use pr_turbojpeg::native::{TjParam, TjPixelFormat, TjScalingFactor};

// ------------------------------------------------------------------------------------------------ 382:27

/// `T = i64::MAX / 4 = 2^61 - 1`, the saturation threshold of `calculate_output_buffer`.
const T: i64 = i64::MAX / 4;

/// Lucas-Lehmer test for the Mersenne number `2^p - 1` (p an odd prime, p <= 63 so every square fits `u128`).
fn lucas_lehmer(p: u32) -> bool {
    let m: u128 = (1u128 << p) - 1;
    let mut s: u128 = 4;
    for _ in 0..p - 2 {
        s = (s * s % m + m - 2) % m;
    }
    s == 0
}

/// decoder.rs:382:27 `width > i64::MAX / 4` -> `==`.
///
/// For a width ABOVE `T` the original saturates the stride to `i64::MAX`; the mutant lets `width * 4` wrap. The refusal prints
/// the buffer length, which differs: with the original it is `i64::MAX` (height 2^32 - 2 is above `i64::MAX / i64::MAX == 1`), with
/// the wrapped (negative) stride it is some other product. A factor `i32::MAX / 1` on a `i32::MAX` side gives width `2^62 - 2^32 + 1`.
#[test]
fn a_scaled_width_above_the_saturation_threshold_saturates_the_stride_and_the_length() {
    let factor = TjScalingFactor::new(i32::MAX, 1);
    let refusal = calculate_output_buffer(i32::MAX, 2, factor).expect_err("far above i32::MAX");
    assert_eq!(refusal.width, i64::from(i32::MAX) * i64::from(i32::MAX));
    assert!(refusal.width > T);
    assert_eq!(refusal.height, 2 * i64::from(i32::MAX));
    assert_eq!(refusal.buffer_length, i64::MAX);
    // One row: stride saturated to i64::MAX, 1 * i64::MAX.
    let one_row = calculate_output_buffer(i32::MAX, 1, factor).expect_err("far above i32::MAX");
    assert_eq!(one_row.buffer_length, i64::MAX);
}

/// decoder.rs:382:27 `width > i64::MAX / 4` -> `>=`: PROVEN equivalent.
///
/// `>` and `>=` differ only for a scaled width of EXACTLY `T`. The scaled width is `(dim * num + denom - 1) / denom` (truncating
/// division) for `i32` `dim`, `num`, `denom`, and `denom <= 0` gives 0. Let `p = dim * num`; `|p| <= 2^62`. A positive result needs
/// `p > 0` (for `p <= 0` the numerator is below `denom`, so the quotient is 0 or negative), and the result is `T` exactly when
/// `(T - 1) * denom < p <= T * denom`.
///  * `denom >= 2`: `p >= (T - 1) * 2 + 1 = 2^62 - 3`, but `p <= (2^31 - 1)^2 = 2^62 - 2^32 + 1` for positive operands, and for two
///    negative operands `|p| <= 2^31 * 2^31 = 2^62`, which would need `dim = num = -2^31`, giving `p = 2^62 >= 2^62 - 3`:
///    that corner is run explicitly below (it gives `(2^62 + denom - 1) / denom`, a value other than `T`). Larger `denom` only raises the lower bound.
///  * `denom == 1`: `p == T` exactly; `T = 2^61 - 1` is a Mersenne PRIME (Lucas-Lehmer, executed here), so a factorisation has a factor
///    `>= T > 2^31`, which no `i32` operand has (magnitudes are at most 2^31).
#[test]
fn no_i32_dimension_and_factor_scale_to_exactly_the_saturation_threshold_by_proof() {
    assert_eq!(T, (1i64 << 61) - 1);
    // The Lucas-Lehmer routine itself, checked against the known exponents (2^11-1 and 2^23-1 are composite).
    for (p, prime) in [
        (3u32, true),
        (5, true),
        (7, true),
        (11, false),
        (13, true),
        (17, true),
        (19, true),
        (23, false),
        (31, true),
        (61, true),
    ] {
        assert_eq!(lucas_lehmer(p), prime, "2^{p} - 1");
    }
    const _: () = assert!(T > 1 << 31, "a cofactor of T above 2^31 does not fit an i32 magnitude");

    let max_positive = i128::from(i32::MAX) * i128::from(i32::MAX);
    let two_t_lower = (i128::from(T) - 1) * 2 + 1;
    assert!(
        two_t_lower > max_positive,
        "denom >= 2 cannot be reached with positive operands"
    );
    // Two negative operands: the largest product is (-2^31) * (-2^31) = 2^62.
    let corner = TjScalingFactor::new(i32::MIN, 2)
        .scaled(i32::MIN)
        .expect("positive denominator");
    assert_ne!(corner, T, "the only product above the positive bound gives {corner}");
    for denom in [2, 3, 4, 1000, i32::MAX] {
        for (dim, num) in [
            (i32::MIN, i32::MIN),
            (i32::MAX, i32::MAX),
            (i32::MIN, i32::MAX),
            (1, i32::MAX),
            (i32::MAX, 1),
        ] {
            let scaled = TjScalingFactor::new(num, denom)
                .scaled(dim)
                .expect("positive denominator");
            assert_ne!(scaled, T, "{dim} * {num} / {denom}");
        }
    }
    // denom == 1: the extremes, and the prime factor argument above.
    for (dim, num) in [
        (i32::MAX, i32::MAX),
        (1, i32::MAX),
        (i32::MAX, 1),
        (i32::MIN, i32::MIN),
        (i32::MIN, i32::MAX),
    ] {
        let scaled = TjScalingFactor::new(num, 1).scaled(dim).expect("positive denominator");
        assert_ne!(scaled, T, "{dim} * {num} / 1");
    }
}

// ------------------------------------------------------------------------------------------------ 580:23, 580:55

fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let length = u16::try_from(payload.len() + 2).expect("segment fits");
    let mut bytes = vec![0xFF, marker];
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

/// A `scan_header` stream: SOI, the given segments, then a SOS marker with an empty body (the walk ends there).
fn stream(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut jpeg = vec![0xFF, 0xD8];
    for part in parts {
        jpeg.extend_from_slice(part);
    }
    jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
    jpeg
}

/// What a full walk with no early exit answers (the documented rules: the first ICC-signed APP2, the first Exif-signed APP1).
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

/// decoder.rs:580:23 and 580:55 (`||` -> `&&` in the stop condition of `scan_header`): PROVEN equivalent.
///
/// The loop state is `(has_icc, found_exif)`; the offset does not enter any decision. A mutant that stops later (never, for an unwanted
/// answer) can only change the result if a segment read AFTER the original's stop point changes the state. The stop point is a state
/// in which every wanted answer is already known, and the two branches of the body are guarded by `!has_icc` and `!found_exif`,
/// so in a stopped state neither branch can fire for ANY segment. A segment is decided by `(marker, payload prefix)` only:
/// `marker == E2 && starts_with(ICC)` and `marker == E1 && starts_with(EXIF)`; all 256 markers are run here with each of the four
/// payload classes (ICC signature, Exif signature with a TIFF part, Exif signature alone, neither).
///
/// For every `want` pair, every stopped state is reached by a prefix (all prefixes of 0..=2 of the 7 kinds below that stop the
/// original) and then EVERY (marker, payload class) segment, twice in a row, is appended; the answer must stay the answer of the
/// prefix, and equal the full walk. 4 x (prefixes) x 1 024 segments x 2 repeats.
#[test]
fn a_stopped_scan_header_state_is_not_changed_by_any_later_segment() {
    let kinds: Vec<Vec<u8>> = vec![
        segment(0xE0, b"JFIF\0"),
        segment(0xE2, &[ICC_SIGNATURE.as_slice(), &[1, 1]].concat()),
        segment(0xE2, b"other"),
        segment(0xE1, &[EXIF_SIGNATURE.as_slice(), &[0x49, 0x49, 0x2A, 0x00]].concat()),
        segment(0xE1, &EXIF_SIGNATURE),
        segment(0xE1, b"xmp"),
        segment(0xDB, &[0; 4]),
    ];
    let payloads: [Vec<u8>; 4] = [
        [ICC_SIGNATURE.as_slice(), &[9, 9]].concat(),
        [EXIF_SIGNATURE.as_slice(), &[0x4D, 0x4D, 0x00, 0x2A]].concat(),
        EXIF_SIGNATURE.to_vec(),
        b"neither of them".to_vec(),
    ];
    let mut prefixes: Vec<Vec<Vec<u8>>> = vec![vec![]];
    for first in &kinds {
        prefixes.push(vec![first.clone()]);
        for second in &kinds {
            prefixes.push(vec![first.clone(), second.clone()]);
        }
    }
    let mut stopped_runs = 0usize;
    for (want_icc, want_exif) in [(false, false), (true, false), (false, true), (true, true)] {
        for prefix in &prefixes {
            let base_stream = stream(prefix);
            let base = scan_header(&base_stream, want_icc, want_exif);
            let base_answer = (base.has_icc, base.exif_tiff.to_vec());
            // The original stops when every wanted answer is known (with both unwanted it stops after the first segment).
            let stopped =
                (!want_icc || base.has_icc) && (!want_exif || !base.exif_tiff.is_empty() || exif_found(prefix));
            if !stopped {
                continue;
            }
            for marker in 0..=255u8 {
                for payload in &payloads {
                    let extra = segment(marker, payload);
                    let mut parts = prefix.clone();
                    parts.push(extra.clone());
                    parts.push(extra);
                    let jpeg = stream(&parts);
                    let scan = scan_header(&jpeg, want_icc, want_exif);
                    assert_eq!(
                        (scan.has_icc, scan.exif_tiff.to_vec()),
                        base_answer,
                        "marker {marker:02X} want {want_icc} {want_exif} prefix {}",
                        prefix.len()
                    );
                    assert_eq!(walk_everything(&jpeg, want_icc, want_exif), base_answer);
                    stopped_runs += 1;
                }
            }
        }
    }
    assert!(stopped_runs > 4 * 1024, "the lemma was exercised: {stopped_runs}");
}

/// True when the prefix contains an Exif-signed APP1 (even one without a TIFF part, which is still "found").
fn exif_found(prefix: &[Vec<u8>]) -> bool {
    prefix
        .iter()
        .any(|part| part[1] == 0xE1 && part[4..].starts_with(&EXIF_SIGNATURE))
}

// ------------------------------------------------------------------------------------------------ 698:28, 698:57

/// A reader that fails the test when it is asked for a single byte.
struct MustNotRead(Arc<Mutex<usize>>);

impl Read for MustNotRead {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        *self.0.lock().unwrap() += 1;
        Err(io::Error::other("the reader must not be touched"))
    }
}

/// A header that always needs more data: SOI, then an APP1 segment that claims 65 535 bytes.
fn hungry_header(total: usize) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF];
    bytes.resize(total.max(6), 0);
    bytes
}

fn expect_unchanged(buffer: Vec<u8>, length: i64, cap: i64) {
    let reads = Arc::new(Mutex::new(0usize));
    let before = buffer.clone();
    assert!(
        header_needs_more_data(&buffer),
        "the premise: the header needs more data"
    );
    let after = grow_header_area(&mut MustNotRead(Arc::clone(&reads)), length, buffer, cap)
        .unwrap_or_else(|e| panic!("length {length} cap {cap}: {e}"));
    assert_eq!(after, before, "length {length} cap {cap}");
    assert_eq!(*reads.lock().unwrap(), 0, "length {length} cap {cap}");
}

/// decoder.rs:698:28 (`len < length` -> `<=`) and 698:57 (`len < limit` -> `<=`) in the `grow_header_area` loop condition: PROVEN
/// equivalent (class enumeration).
///
/// `limit = min(length, cap, MAX_SOURCE_BYTES)`. The operators differ only on the equalities `len == length` and `len == limit`
/// (everything else is a strict order, where `<` and `<=` agree).
/// * `len == length`: then `limit <= length == len`, so the second conjunct `len < limit` is false and the loop does not run,
///   with either operator.
/// * `len == limit` with the mutant `<=`: the loop body starts, `new_len = min(limit, 2 * len) == len`, and the guard
///   `new_len <= old_len` breaks out before anything is reserved, resized or read.
///
/// `limit` has exactly three sources (`length`, `cap`, `MAX_SOURCE_BYTES`) and the equality states are enumerated for each: for
/// every buffer length 0..=64 (a hungry header, so the third conjunct is true and the reader would be touched by any growth), and
/// every `cap` / `length` pairing that makes that source the limit, including negative and extreme values. The buffer must come back
/// unchanged and the reader must never be read. The `MAX_SOURCE_BYTES` source needs a 2 GiB buffer: its own `#[ignore]`d test.
#[test]
fn a_buffer_exactly_at_its_limit_is_returned_unchanged_whatever_the_limit_came_from() {
    let extremes = [
        0i64,
        1,
        6,
        63,
        64,
        65,
        1000,
        i64::from(i32::MAX),
        i64::MAX - 1,
        i64::MAX,
    ];
    let mut states = 0usize;
    for len in 6..=64usize {
        let n = len as i64;
        // limit == length (len == length), cap anywhere at or above it.
        for cap in [n, n + 1, 1000, i64::from(i32::MAX), i64::MAX] {
            expect_unchanged(hungry_header(len), n, cap);
            states += 1;
        }
        // limit == cap < length (len == cap), length anywhere above it.
        for length in [n + 1, n + 2, 1000, i64::from(i32::MAX), i64::MAX] {
            expect_unchanged(hungry_header(len), length, n);
            states += 1;
        }
        // limit == length == cap (all three equal).
        expect_unchanged(hungry_header(len), n, n);
        states += 1;
    }
    // A cap or length below the buffer (len > limit) and the extremes: also unchanged, no read.
    for len in [6usize, 7, 64] {
        for length in extremes {
            for cap in extremes.iter().copied().chain([-1, i64::MIN]) {
                let limit = length.min(cap).min(MAX_SOURCE_BYTES);
                if (len as i64) < limit && (len as i64) < length {
                    continue; // growth is legitimate there (the reader would be read)
                }
                expect_unchanged(hungry_header(len), length, cap);
                states += 1;
            }
        }
    }
    assert!(states > 400, "{states}");
}

/// The third source of `limit`: `len == MAX_SOURCE_BYTES` (2 147 483 583 bytes), with `length` and `cap` above it. Reserves the
/// buffer with `vec![0; n]`, which Windows backs with lazily zeroed pages (only the first page is touched).
#[test]
#[ignore = "large: reserves 2 GiB of address space (lazily zeroed pages)"]
fn a_two_gibibyte_buffer_exactly_at_the_source_limit_is_returned_unchanged() {
    let size = usize::try_from(MAX_SOURCE_BYTES).expect("64-bit");
    let mut buffer = vec![0u8; size];
    buffer[..2].copy_from_slice(&[0xFF, 0xD8]);
    // A chain of APP1 segments of 65 535 declared bytes each (65 537 with the marker), so the walk runs through the whole
    // buffer and the last segment overruns its end: the header "needs more data" however large the buffer is.
    let mut offset = 2usize;
    while offset + 4 <= size {
        buffer[offset..offset + 4].copy_from_slice(&[0xFF, 0xE1, 0xFF, 0xFF]);
        offset += 65_537;
    }
    assert!(header_needs_more_data(&buffer));
    let reads = Arc::new(Mutex::new(0usize));
    let length = MAX_SOURCE_BYTES + 1000;
    let after =
        grow_header_area(&mut MustNotRead(Arc::clone(&reads)), length, buffer, i64::MAX).expect("no growth, no read");
    assert_eq!(after.len(), size);
    assert_eq!(*reads.lock().unwrap(), 0);
}

// ------------------------------------------------------------------------------------------------ 1126:47, 1126:62

/// decoder.rs:1126:62 (`orientation > 1` -> `>= 1`) and the apply-side half of 1126:47 (`&&` -> `||`).
///
/// The mutants make `FinishPlan::orient` true where the original has it false, i.e. for `apply_orientation == true` with an
/// orientation of 1 (and below), and `finish_with_codec` then calls `apply_orientation(pixels, orientation)`. That call answers
/// `Ok(src)` unchanged (same buffer, same id) for every orientation outside 2..=8. ALL 2^32 - 7 such `i32` values are run here,
/// on four threads, against a live buffer.
#[test]
fn apply_orientation_is_the_identity_for_every_i32_outside_two_to_eight() {
    fn sweep(low: i32, high: i32) {
        let mut buffer = PixelBuffer::allocate(1, 1, PixelLayout::Bgr32).expect("1x1 buffer");
        let id = buffer.id();
        let mut value = i64::from(low);
        while value <= i64::from(high) {
            let orientation = value as i32;
            if !(2..=8).contains(&orientation) {
                buffer = match apply_orientation(buffer, orientation) {
                    Ok(same) => same,
                    Err(e) => panic!("orientation {orientation}: {:?}", e.error),
                };
                assert_eq!(buffer.id(), id, "orientation {orientation} replaced the buffer");
            }
            value += 1;
        }
    }
    thread::scope(|scope| {
        scope.spawn(|| sweep(i32::MIN, -(1 << 30) - 1));
        scope.spawn(|| sweep(-(1 << 30), -1));
        scope.spawn(|| sweep(0, (1 << 30) - 1));
        scope.spawn(|| sweep(1 << 30, i32::MAX));
    });
}

// A fake native decompressor reporting a fixed size, so `decode_bytes` runs without a DLL.
struct Probe {
    width: i32,
    height: i32,
}

impl Decompressor for Probe {
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
        _dst: &mut [u8],
        _pitch: i32,
        _pixel_format: TjPixelFormat,
    ) -> Result<i32, DecodeError> {
        Ok(0)
    }
    fn error_message(&mut self) -> Option<String> {
        None
    }
}

struct NoScale;

impl FineScaler for NoScale {
    fn resize(&self, _source: &PixelBuffer, width: i32, height: i32) -> Result<PixelBuffer, FineScaleError> {
        PixelBuffer::allocate(width, height, PixelLayout::Bgr32)
            .map_err(|e| FineScaleError::Failed(DecodeError::Other { message: e.to_string() }))
    }
}

fn decoder() -> TurboJpegDecoder {
    let factory: Arc<dyn DecompressorFactory> =
        Arc::new(|| -> Result<Box<dyn Decompressor>, DecodeError> { Ok(Box::new(Probe { width: 64, height: 48 })) });
    let memory: Arc<dyn MemoryInfoSource> = Arc::new(|| MemoryInfo {
        total_available: 1 << 36,
        load: 0,
    });
    TurboJpegDecoder::new(
        PixelBufferImageCodec::shared(),
        Arc::new(NoScale),
        memory,
        factory,
        Arc::new(EnglishTurboJpegTexts),
    )
}

/// The fixture with an Exif APP1 carrying `orientation` (both byte orders) spliced in right after SOI.
fn jpeg_with_exif_orientation(orientation: u16, big_endian: bool) -> Vec<u8> {
    let fixture =
        std::fs::read(golden::repo_root().join("tests/fixtures/turbojpeg/parity-64x48.jpg")).expect("fixture");
    let word = |value: u16| {
        if big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    };
    let mut tiff = if big_endian { b"MM".to_vec() } else { b"II".to_vec() };
    tiff.extend_from_slice(&word(42));
    tiff.extend_from_slice(&if big_endian {
        8u32.to_be_bytes()
    } else {
        8u32.to_le_bytes()
    });
    tiff.extend_from_slice(&word(1)); // one entry
    tiff.extend_from_slice(&word(0x0112)); // Orientation
    tiff.extend_from_slice(&word(3)); // SHORT
    tiff.extend_from_slice(&if big_endian {
        1u32.to_be_bytes()
    } else {
        1u32.to_le_bytes()
    });
    tiff.extend_from_slice(&word(orientation));
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&[0, 0, 0, 0]); // next IFD
    let mut jpeg = vec![0xFF, 0xD8];
    jpeg.extend_from_slice(&segment(0xE1, &[EXIF_SIGNATURE.as_slice(), &tiff].concat()));
    jpeg.extend_from_slice(&fixture[2..]);
    jpeg
}

/// decoder.rs:1126:47 (`apply_orientation && orientation > 1` -> `||`), the not-applied half: PROVEN equivalent (injection
/// enumeration).
///
/// With `apply_orientation == false` the original has `orient == false`; the mutant has `orient == (orientation > 1)`. They can only
/// differ in an execution in which the local `orientation` is above 1 although it is not applied, AND the value is one that
/// `apply_orientation` acts on (2..=8; every other value is the identity, see the sweep above). The local starts at 1 and can receive
/// a value from exactly two sources, `request.source_orientation` and the Exif block of the source; so the divergent set is covered
/// by injecting each of 2..=8 (and the neighbours 0, 1, 9, 255, 65 535, `i32::MIN`, `i32::MAX`) through both sources, the Exif one
/// in both byte orders. In every run the decode reports orientation 1, the unrotated size and the same pixels size as a request
/// without any orientation source. (With `apply_orientation == true` the same injections report the injected value and swap the
/// axes for 5..=8, so the test would see a leak.)
#[test]
fn an_orientation_that_is_not_applied_never_reaches_the_plan() {
    let decoder = decoder();
    let plain = std::fs::read(golden::repo_root().join("tests/fixtures/turbojpeg/parity-64x48.jpg")).expect("fixture");
    let baseline = decoder
        .decode(
            &DecodeRequest::with_box("p.jpg", DecodeBox::new(0, 0))
                .with_bytes(plain.into())
                .with_apply_orientation(false),
        )
        .expect("baseline");
    assert_eq!(
        (baseline.pixel_width(), baseline.pixel_height(), baseline.orientation()),
        (64, 48, 1)
    );

    let values: Vec<i32> = (0..=9).chain([255, 65_535, i32::MIN, i32::MAX]).collect();
    for &value in &values {
        let request = |bytes: Vec<u8>, apply: bool, source: Option<i32>| {
            let mut request = DecodeRequest::with_box("p.jpg", DecodeBox::new(0, 0))
                .with_bytes(bytes.into())
                .with_apply_orientation(apply);
            if let Some(value) = source {
                request = request.with_source_orientation(value);
            }
            request
        };
        let fixture =
            std::fs::read(golden::repo_root().join("tests/fixtures/turbojpeg/parity-64x48.jpg")).expect("fixture");
        // Source A: the request's source orientation, not applied.
        let image = decoder.decode(&request(fixture, false, Some(value))).expect("decode");
        assert_eq!(
            (image.pixel_width(), image.pixel_height(), image.orientation()),
            (64, 48, 1),
            "source {value}"
        );
        // Source B: the Exif block, not applied (value range of a SHORT), both byte orders.
        if let Ok(short) = u16::try_from(value) {
            for big_endian in [false, true] {
                let image = decoder
                    .decode(&request(jpeg_with_exif_orientation(short, big_endian), false, None))
                    .expect("decode");
                assert_eq!(
                    (image.pixel_width(), image.pixel_height(), image.orientation()),
                    (64, 48, 1),
                    "exif {short} big_endian {big_endian}"
                );
                // Control: applied, the same block is honoured (so the injection really reaches the decoder).
                let applied = decoder
                    .decode(&request(jpeg_with_exif_orientation(short, big_endian), true, None))
                    .expect("decode");
                if (1..=8).contains(&short) {
                    assert_eq!(applied.orientation(), i32::from(short), "applied exif {short}");
                }
                let swapped = (5..=8).contains(&short);
                assert_eq!(
                    (applied.pixel_width(), applied.pixel_height()),
                    if swapped { (48, 64) } else { (64, 48) },
                    "applied exif {short}"
                );
            }
        }
        // Control for source A: applied, the injected value is reported as given.
        let fixture =
            std::fs::read(golden::repo_root().join("tests/fixtures/turbojpeg/parity-64x48.jpg")).expect("fixture");
        let applied = decoder.decode(&request(fixture, true, Some(value))).expect("decode");
        assert_eq!(applied.orientation(), value, "applied source {value}");
    }
}

/// decoder.rs:1126:62 (`orientation > 1` -> `>= 1`): PROVEN equivalent. The divergent set is `apply_orientation && orientation == 1`;
/// there the mutant orients, which is `apply_orientation(pixels, 1)`, the identity proven above for every `i32` outside 2..=8. The
/// decode of that exact set is run end to end (both Exif byte orders and the request source orientation 1) and equals the decode
/// that applies nothing.
#[test]
fn applying_orientation_one_changes_nothing() {
    let decoder = decoder();
    let plain = std::fs::read(golden::repo_root().join("tests/fixtures/turbojpeg/parity-64x48.jpg")).expect("fixture");
    let reference = decoder
        .decode(
            &DecodeRequest::with_box("p.jpg", DecodeBox::new(0, 0))
                .with_bytes(plain.clone().into())
                .with_apply_orientation(false),
        )
        .expect("reference");
    let mut runs = vec![
        decoder.decode(
            &DecodeRequest::with_box("p.jpg", DecodeBox::new(0, 0))
                .with_bytes(plain.into())
                .with_source_orientation(1),
        ),
    ];
    for big_endian in [false, true] {
        runs.push(
            decoder.decode(
                &DecodeRequest::with_box("p.jpg", DecodeBox::new(0, 0))
                    .with_bytes(jpeg_with_exif_orientation(1, big_endian).into()),
            ),
        );
    }
    for run in runs {
        let image = run.expect("decode");
        assert_eq!(
            (
                image.pixel_width(),
                image.pixel_height(),
                image.orientation(),
                image.downscaled()
            ),
            (
                reference.pixel_width(),
                reference.pixel_height(),
                1,
                reference.downscaled()
            )
        );
    }
}

/// Premise of the 1129:34 reading, kept as a run: no supported factor scales up, so `scaled_width > orig_width` never holds
/// (this is the mutant's replacement for `scaled_width < orig_width`); the unscaled factor keeps the size. The other half
/// (a factor other than 1/1 is only chosen when a target side is below the source side) is control flow and stays OPEN.
#[test]
fn no_supported_factor_makes_a_side_larger() {
    for factor in SUPPORTED_SCALING_FACTORS {
        assert!(factor.num > 0 && factor.num <= factor.denom, "{factor:?}");
        for side in [1, 2, 3, 7, 8, 9, 63, 64, 65, 4000, 65_535, i32::MAX - 1, i32::MAX] {
            let scaled = scaled_size(side, factor);
            assert!(scaled <= i64::from(side), "{side} * {factor:?} = {scaled}");
            assert!(scaled >= 1, "{side} * {factor:?} = {scaled}");
        }
    }
    for side in [1, 2, 3, 64, i32::MAX] {
        assert_eq!(scaled_size(side, TjScalingFactor::ONE), i64::from(side));
    }
}
