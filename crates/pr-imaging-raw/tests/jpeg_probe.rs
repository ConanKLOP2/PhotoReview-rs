//! Red tests for W5-02b `JpegMarkerProbe` (`jpeg_probe.rs`, C# baseline d92d6aae).
//!
//! Ports every method of `tests/PhotoReview.Imaging.Tests/Raw/Tiff/JpegMarkerProbeMutationGapTests.cs` (22 methods: 15 facts
//! and 7 theories with 22 rows), the `TryReadLossyFrame_JpegExhaustingHeaderBudget_*` case of `TiffPreviewHardeningTests.cs`,
//! the JPEG half of `Robustness/BinaryReaderFuzzTests.cs` (`Fuzz_JpegMarkerWalkers_FailOnlyCleanly`), and golden
//! `jpeg-marker-probe` (86 cases). Stub bodies are `todo!()`, so every test that reaches the probe fails until it is filled in.

use pr_imaging_raw::jpeg_probe::{LossyFrame, try_read_lossy_frame};
use pr_imaging_raw::{InMemoryRawHeaderSource, RawHeaderError, RawHeaderSource};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

const SOI: [u8; 2] = [0xFF, 0xD8];

/// C# `JpegMarkerProbeMutationGapTests.Sof`: a 13-byte SOFn frame header (3 components) with the given size and precision.
fn sof(marker: u8, width: u16, height: u16, precision: u8) -> Vec<u8> {
    let [width_high, width_low] = width.to_be_bytes();
    let [height_high, height_low] = height.to_be_bytes();
    vec![
        0xFF,
        marker,
        0x00,
        0x0B,
        precision,
        height_high,
        height_low,
        width_high,
        width_low,
        0x01,
        0x01,
        0x11,
        0x00,
    ]
}

/// The default frame of the C# tests: 640 x 480, 8-bit, SOF0.
fn sof_default() -> Vec<u8> {
    sof(0xC0, 640, 480, 8)
}

/// A segment with `payload` after its 16-bit length field (the length counts the field itself).
fn segment_with_payload(marker: u8, payload: &[u8]) -> Vec<u8> {
    let length = u16::try_from(payload.len() + 2).expect("segment length fits u16");
    let mut bytes = vec![0xFF, marker];
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

/// C# `JpegMarkerProbeMutationGapTests.Segment(marker, payloadLength)`: zeroed payload.
fn segment(marker: u8, payload_len: usize) -> Vec<u8> {
    segment_with_payload(marker, &vec![0u8; payload_len])
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn len_i64(bytes: &[u8]) -> i64 {
    i64::try_from(bytes.len()).expect("test input length fits i64")
}

/// C# `Probe(bytes, out w, out h, offset, length)`: the range defaults to the rest of the source.
fn probe(bytes: &[u8], offset: i64, length: Option<i64>) -> Result<Option<LossyFrame>, RawHeaderError> {
    let mut source = InMemoryRawHeaderSource::new(bytes.to_vec());
    let length = length.unwrap_or(len_i64(bytes) - offset);
    try_read_lossy_frame(&mut source, offset, length)
}

/// [`probe`] on the in-memory source, which cannot fail: an `Err` here is itself a failure.
fn probe_frame(bytes: &[u8], offset: i64, length: Option<i64>) -> Option<LossyFrame> {
    probe(bytes, offset, length).expect("in-memory source cannot fail")
}

#[derive(Clone, Copy, Debug)]
enum FailKind {
    InvalidData,
    Io,
}

/// C# `JpegMarkerProbeMutationGapTests.ThrowingSource`: length 64, every read fails with the injected error.
struct FailingSource {
    kind: FailKind,
}

impl RawHeaderSource for FailingSource {
    fn length(&self) -> i64 {
        64
    }

    fn read(&mut self, _offset: i64, _count: i32) -> Result<&[u8], RawHeaderError> {
        Err(match self.kind {
            FailKind::InvalidData => RawHeaderError::invalid_data("budget exhausted"),
            FailKind::Io => RawHeaderError::invalid_data_io("read failed", std::io::Error::other("disk")),
        })
    }
}

#[test]
fn try_read_lossy_frame_range_ending_exactly_at_end_of_source_is_probed() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_RangeEndingExactlyAtTheEndOfTheSource_IsProbed
    let bytes = cat(&[&SOI, &sof_default()]);

    assert_eq!(
        probe_frame(&bytes, 0, None),
        Some(LossyFrame {
            width: 640,
            height: 480
        })
    );
}

#[test]
fn try_read_lossy_frame_range_starting_inside_source_is_probed_from_that_offset() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_RangeStartingInsideTheSource_IsProbedFromThatOffset
    let bytes = cat(&[&[1, 2, 3], &SOI, &sof_default()]);

    assert_eq!(
        probe_frame(&bytes, 3, None),
        Some(LossyFrame {
            width: 640,
            height: 480
        })
    );
}

#[test]
fn try_read_lossy_frame_declared_range_longer_than_source_is_rejected_even_when_frame_is_inside() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_DeclaredRangeLongerThanTheSource_IsRejectedEvenWhenTheFrameIsInside
    let bytes = cat(&[&SOI, &sof_default()]);
    let declared = len_i64(&bytes) + 10;

    assert_eq!(probe_frame(&bytes, 0, Some(declared)), None);
}

#[test]
fn try_read_lossy_frame_negative_offset_returns_none_without_panicking() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_NegativeOffset_ReturnsFalseWithoutThrowing
    let bytes = cat(&[&SOI, &sof_default()]);

    assert_eq!(probe_frame(&bytes, -1, Some(10)), None);
}

#[test]
fn try_read_lossy_frame_frame_header_beyond_end_of_range_is_not_read() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_FrameHeaderBeyondTheEndOfTheRange_IsNotRead
    // The range is SOI + an APP0 segment; a valid frame header follows it in the source but outside the range.
    let range = cat(&[&SOI, &segment(0xE0, 4)]);
    let bytes = cat(&[&range, &sof_default()]);

    assert_eq!(probe_frame(&bytes, 0, Some(len_i64(&range))), None);
}

#[test]
fn try_read_lossy_frame_frame_header_cut_off_by_end_of_range_is_not_read() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_FrameHeaderCutOffByTheEndOfTheRange_IsNotRead
    let bytes = cat(&[&SOI, &sof_default()]);

    // The frame data would end at byte 11; the range ends at 10.
    assert_eq!(probe_frame(&bytes, 0, Some(10)), None);
}

#[test]
fn try_read_lossy_frame_minimal_frame_header_of_eight_bytes_is_read() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_MinimalFrameHeaderOfEightBytes_IsRead
    // Segment length 8 (6 payload bytes), the shortest accepted; the range ends with the width.
    let sof: [u8; 9] = [0xFF, 0xC0, 0x00, 0x08, 0x08, 0x01, 0xE0, 0x02, 0x80];
    let bytes = cat(&[&SOI, &sof]);

    assert_eq!(
        probe_frame(&bytes, 0, None),
        Some(LossyFrame {
            width: 640,
            height: 480
        })
    );
}

#[test]
fn try_read_lossy_frame_frame_header_segment_of_seven_bytes_is_rejected() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_FrameHeaderSegmentOfSevenBytes_IsRejected
    let sof: [u8; 9] = [0xFF, 0xC0, 0x00, 0x07, 0x08, 0x01, 0xE0, 0x02, 0x80];
    let bytes = cat(&[&SOI, &sof, &[0u8; 4]]);

    assert_eq!(probe_frame(&bytes, 0, None), None);
}

#[test]
fn try_read_lossy_frame_first_frame_marker_only_lossy_sequential_and_progressive_are_accepted() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_FirstFrameMarker_OnlyLossySequentialAndProgressiveAreAccepted
    // Rows: C0, C1, C2 accepted; C3, C5, CF rejected. The first frame wins: a later valid SOF0 (111 x 222) is never looked at.
    let rows: [(u8, bool); 6] = [
        (0xC0, true),
        (0xC1, true),
        (0xC2, true),
        (0xC3, false),
        (0xC5, false),
        (0xCF, false),
    ];
    for (marker, expected) in rows {
        let bytes = cat(&[&SOI, &sof(marker, 1000, 700, 8), &sof(0xC0, 111, 222, 8)]);
        let want = expected.then_some(LossyFrame {
            width: 1000,
            height: 700,
        });

        assert_eq!(probe_frame(&bytes, 0, None), want, "frame marker 0x{marker:02X}");
    }
}

#[test]
fn try_read_lossy_frame_zero_width_or_height_is_rejected() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_ZeroWidthOrHeight_IsRejected (2 rows: (0, 480), (640, 0))
    let rows: [(u16, u16); 2] = [(0, 480), (640, 0)];
    for (width, height) in rows {
        let bytes = cat(&[&SOI, &sof(0xC0, width, height, 8)]);

        assert_eq!(probe_frame(&bytes, 0, None), None, "width {width}, height {height}");
    }
}

#[test]
fn try_read_lossy_frame_twelve_bit_precision_is_rejected() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_TwelveBitPrecision_IsRejected
    let bytes = cat(&[&SOI, &sof(0xC1, 640, 480, 12)]);

    assert_eq!(probe_frame(&bytes, 0, None), None);
}

#[test]
fn try_read_lossy_frame_table_or_reserved_segment_inside_sof_range_is_skipped() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_TableOrReservedSegmentInsideTheSofRange_IsSkipped
    // Rows: DHT (C4), JPG (C8), DAC (CC) are ordinary segments, skipped by their length.
    for marker in [0xC4u8, 0xC8, 0xCC] {
        let bytes = cat(&[&SOI, &segment(marker, 20), &sof_default()]);

        assert_eq!(
            probe_frame(&bytes, 0, None),
            Some(LossyFrame {
                width: 640,
                height: 480
            }),
            "marker 0x{marker:02X}"
        );
    }
}

#[test]
fn try_read_lossy_frame_segment_longer_than_two_hundred_fifty_six_bytes_is_skipped_with_its_full_length() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_SegmentLongerThanTwoFiftySixBytes_IsSkippedWithItsFullLength
    let bytes = cat(&[&SOI, &segment(0xE0, 300), &sof_default()]);

    assert_eq!(
        probe_frame(&bytes, 0, None),
        Some(LossyFrame {
            width: 640,
            height: 480
        })
    );
}

#[test]
fn try_read_lossy_frame_empty_segment_before_frame_is_skipped() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_EmptySegmentBeforeTheFrame_IsSkipped
    let bytes = cat(&[&SOI, &[0xFF, 0xE0, 0x00, 0x02], &sof_default()]);

    assert_eq!(
        probe_frame(&bytes, 0, None),
        Some(LossyFrame {
            width: 640,
            height: 480
        })
    );
}

#[test]
fn try_read_lossy_frame_segment_length_below_two_ends_the_walk() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_SegmentLengthBelowTwo_EndsTheWalk
    let bytes = cat(&[&SOI, &[0xFF, 0xE0, 0x00, 0x01], &sof_default()]);

    assert_eq!(probe_frame(&bytes, 0, None), None);
}

#[test]
fn try_read_lossy_frame_entropy_or_end_marker_before_frame_ends_the_walk() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_EntropyDataOrEndMarkerBeforeTheFrame_EndsTheWalk
    // Rows: stuffed 00, SOS DA, EOI D9.
    for marker in [0x00u8, 0xDA, 0xD9] {
        let bytes = cat(&[&SOI, &[0xFF, marker, 0x00, 0x02], &sof_default()]);

        assert_eq!(probe_frame(&bytes, 0, None), None, "marker 0x{marker:02X}");
    }
}

#[test]
fn try_read_lossy_frame_standalone_marker_before_frame_is_skipped_without_a_length() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_StandaloneMarkerBeforeTheFrame_IsSkippedWithoutALength
    // Rows: TEM (01), RST0 (D0), RST4 (D4), SOI-inside-walk (D8).
    for marker in [0x01u8, 0xD0, 0xD4, 0xD8] {
        let bytes = cat(&[&SOI, &[0xFF, marker], &sof_default()]);

        assert_eq!(
            probe_frame(&bytes, 0, None),
            Some(LossyFrame {
                width: 640,
                height: 480
            }),
            "marker 0x{marker:02X}"
        );
    }
}

#[test]
fn try_read_lossy_frame_frame_header_after_many_segments_is_found_only_within_the_segment_cap() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_FrameHeaderAfterManySegments_IsFoundOnlyWithinTheSegmentCap
    // Rows: 511 segments then the frame is found; 512 is past MaxJpegMarkerSegments (512) and is not.
    for (segments, expected) in [(511usize, true), (512, false)] {
        let mut bytes = SOI.to_vec();
        for _ in 0..segments {
            bytes.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x02]);
        }
        bytes.extend(sof_default());
        let want = expected.then_some(LossyFrame {
            width: 640,
            height: 480,
        });

        assert_eq!(
            probe_frame(&bytes, 0, None),
            want,
            "{segments} segments before the frame"
        );
    }
}

#[test]
fn try_read_lossy_frame_run_of_fill_bytes_before_frame_is_tolerated_up_to_the_cap() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_RunOfFillBytesBeforeTheFrame_IsToleratedUpToTheCap
    // Rows: 65 536 fill bytes (MaxJpegFillBytes) accepted; 65 537 rejected.
    for (fill_bytes, expected) in [(65_536usize, true), (65_537, false)] {
        let mut bytes = SOI.to_vec();
        bytes.resize(bytes.len() + fill_bytes, 0xFF);
        bytes.extend(sof_default());
        let want = expected.then_some(LossyFrame {
            width: 640,
            height: 480,
        });

        assert_eq!(
            probe_frame(&bytes, 0, None),
            want,
            "{fill_bytes} fill bytes before the frame"
        );
    }
}

#[test]
fn try_read_lossy_frame_no_soi_is_rejected() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_NoSoi_IsRejected
    let bytes = cat(&[&[0x00, 0x00], &sof_default()]);

    assert_eq!(probe_frame(&bytes, 0, None), None);
}

#[test]
fn try_read_lossy_frame_source_failing_with_invalid_data_returns_none_and_no_error() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_SourceThatFailsWithInvalidData_ReturnsFalseAndZeroSize
    // Rust: `Ok(None)` (the "zero size" half has no twin: there are no out values).
    let mut source = FailingSource {
        kind: FailKind::InvalidData,
    };

    let got = try_read_lossy_frame(&mut source, 0, 64);

    assert!(
        matches!(got, Ok(None)),
        "a non-I/O source failure must map to Ok(None), got {got:?}"
    );
}

#[test]
fn try_read_lossy_frame_source_failing_with_io_error_propagates() {
    // C#: JpegMarkerProbeMutationGapTests.TryReadLossyFrame_SourceThatFailsWithAnIoError_Propagates
    // Rust: `Err(e)` with `e.is_io_failure()` (C# `InvalidDataException` with an `IOException` inner).
    let mut source = FailingSource { kind: FailKind::Io };

    let got = try_read_lossy_frame(&mut source, 0, 64);

    match got {
        Err(error) => assert!(
            error.is_io_failure(),
            "I/O inner error must stay an I/O failure: {error}"
        ),
        other => panic!("an I/O failure must propagate as Err, got {other:?}"),
    }
}

/// C# `TiffPreviewHardeningTests.BudgetExhaustingJpeg`: SOI, 140 APP0 segments of declared length 65 535 laid out
/// 65 537 bytes apart, then a 320 x 240 SOF0. The budget-charging source runs out before the SOF.
fn budget_exhausting_jpeg() -> Vec<u8> {
    let mut jpeg = SOI.to_vec();
    for _ in 0..140 {
        jpeg.extend_from_slice(&[0xFF, 0xE0, 0xFF, 0xFF]);
        jpeg.resize(jpeg.len() + 65_533, 0);
    }
    jpeg.extend(sof(0xC0, 320, 240, 8));
    jpeg
}

#[test]
fn try_read_lossy_frame_jpeg_exhausting_header_budget_returns_none_instead_of_throwing() {
    // C#: TiffPreviewHardeningTests.TryReadLossyFrame_JpegExhaustingHeaderBudget_ReturnsFalseInsteadOfThrowing
    let jpeg = budget_exhausting_jpeg();

    let got = probe(&jpeg, 0, None);

    assert!(
        matches!(got, Ok(None)),
        "an exhausted header budget is not a preview, got {got:?}"
    );
}

/// A minimal lossy JPEG: SOI, APP0 JFIF, SOF0 `width` x `height`, SOS, EOI.
fn minimal_jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut jpeg = SOI.to_vec();
    jpeg.extend(segment_with_payload(
        0xE0,
        &[
            b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00,
        ],
    ));
    jpeg.extend(sof(0xC0, width, height, 8));
    jpeg.extend(segment_with_payload(0xDA, &[0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]));
    jpeg.extend_from_slice(&[0xFF, 0xD9]);
    jpeg
}

/// The second fuzz seed: the minimal JPEG with an Exif APP1 and an ICC APP2 segment after SOI.
fn jpeg_with_exif_and_icc() -> Vec<u8> {
    let tail = minimal_jpeg(64, 48);
    let mut jpeg = SOI.to_vec();
    let mut exif = b"Exif\0\0".to_vec();
    exif.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00]);
    jpeg.extend(segment_with_payload(0xE1, &exif));
    let mut icc = b"ICC_PROFILE\0".to_vec();
    icc.extend_from_slice(&[0x01, 0x01]);
    icc.extend(vec![0u8; 40]);
    jpeg.extend(segment_with_payload(0xE2, &icc));
    jpeg.extend_from_slice(&tail[2..]);
    jpeg
}

/// Deterministic xorshift64 for the mutants (no `rand` crate, contract W5-02b).
struct XorShift64(u64);

impl XorShift64 {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Index in `0..bound`; `bound` must be non-zero.
    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).expect("bound fits u64");
        usize::try_from(self.next_u64() % bound).expect("index fits usize")
    }
}

/// `seed` with `encoded` written at `at`, or `None` when it does not fit.
fn overwrite_at(seed: &[u8], at: usize, encoded: &[u8]) -> Option<Vec<u8>> {
    let end = at.checked_add(encoded.len())?;
    if end > seed.len() {
        return None;
    }
    let mut out = seed.to_vec();
    out[at..end].copy_from_slice(encoded);
    Some(out)
}

/// The corpus of one seed: the seed, every truncation (including the empty source), u16/u32 extreme overwrites at every
/// position in both byte orders, and 1500 seeded bit-flip, splice and byte mutants.
fn fuzz_corpus(seed: &[u8], rng: &mut XorShift64) -> Vec<Vec<u8>> {
    assert!(!seed.is_empty(), "fuzz seed must not be empty");
    const U16_VALUES: [u16; 4] = [0, 0x7FFF, 0xFFFF, 0x8000];
    const U32_VALUES: [u32; 4] = [0, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF];

    let mut corpus = vec![seed.to_vec()];
    for cut in 0..seed.len() {
        corpus.push(seed[..cut].to_vec());
    }
    for at in 0..seed.len() {
        for value in U16_VALUES {
            for encoded in [value.to_be_bytes(), value.to_le_bytes()] {
                corpus.extend(overwrite_at(seed, at, &encoded));
            }
        }
        for value in U32_VALUES {
            for encoded in [value.to_be_bytes(), value.to_le_bytes()] {
                corpus.extend(overwrite_at(seed, at, &encoded));
            }
        }
    }
    for _ in 0..1500 {
        let mut mutant = seed.to_vec();
        match rng.below(3) {
            0 => {
                let at = rng.below(mutant.len());
                mutant[at] ^= 1u8 << rng.below(8);
            }
            1 => {
                let from = rng.below(seed.len());
                let to = rng.below(seed.len());
                let run = (1 + rng.below(16)).min(seed.len() - from).min(seed.len() - to);
                mutant[to..to + run].copy_from_slice(&seed[from..from + run]);
            }
            _ => {
                let at = rng.below(mutant.len());
                mutant[at] = rng.next_u64().to_le_bytes()[0];
            }
        }
        corpus.push(mutant);
    }
    corpus
}

/// One fuzz mutant: a frame is only ever accepted with a positive size, and the extreme range calls are rejected by the
/// range check before any read. A panic is the failure (no `catch_unwind`).
fn check_mutant(bytes: &[u8]) {
    let length = len_i64(bytes);
    let mut source = InMemoryRawHeaderSource::new(bytes.to_vec());
    match try_read_lossy_frame(&mut source, 0, length) {
        Ok(Some(frame)) => assert!(
            frame.width > 0 && frame.height > 0,
            "frame accepted with a non-positive size: {frame:?}"
        ),
        Ok(None) => {}
        Err(error) => panic!("the in-memory source cannot fail: {error}"),
    }
    let mut source = InMemoryRawHeaderSource::new(bytes.to_vec());
    assert!(
        matches!(try_read_lossy_frame(&mut source, 0, i64::MAX), Ok(None)),
        "a length of i64::MAX must be rejected by the range check"
    );
    let mut source = InMemoryRawHeaderSource::new(bytes.to_vec());
    assert!(
        matches!(try_read_lossy_frame(&mut source, length / 2, length), Ok(None)),
        "an offset of len/2 with length len must be rejected by the range check"
    );
}

#[test]
fn fuzz_try_read_lossy_frame_never_panics() {
    // C#: BinaryReaderFuzzTests.Fuzz_JpegMarkerWalkers_FailOnlyCleanly (JpegMarkerProbe half; the PreviewSelector half is W5-11)
    let seeds = [minimal_jpeg(64, 48), jpeg_with_exif_and_icc()];
    for (index, seed) in seeds.iter().enumerate() {
        let mut rng = XorShift64(4500 + u64::try_from(index).expect("seed index fits u64"));
        for mutant in fuzz_corpus(seed, &mut rng) {
            check_mutant(&mutant);
        }
    }
}

/// Rebuilds the bytes of a golden case: each part is its hex, zero-extended to `pad_to` bytes, repeated `repeat` times.
fn golden_bytes(case: &Value) -> Vec<u8> {
    let name = str_of(case, "name");
    let parts = case["parts"]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: no parts array"));
    let mut out = Vec::new();
    for part in parts {
        let mut piece = decode_hex(str_of(part, "hex"));
        if let Some(pad_to) = part.get("pad_to").and_then(Value::as_u64) {
            let pad_to = usize::try_from(pad_to).expect("pad_to fits usize");
            piece.resize(pad_to.max(piece.len()), 0);
        }
        let repeat = part.get("repeat").and_then(Value::as_u64).unwrap_or(1);
        for _ in 0..repeat {
            out.extend_from_slice(&piece);
        }
    }
    out
}

fn decode_hex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "odd hex length in {text:?}");
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).expect("ASCII hex digits");
            u8::from_str_radix(digits, 16).unwrap_or_else(|error| panic!("bad hex {digits:?}: {error}"))
        })
        .collect()
}

/// The resolved `length` of a golden case: a decimal string (for `i64::MAX`) or a number.
fn golden_length(case: &Value) -> i64 {
    match &case["length"] {
        Value::String(text) => text.parse().unwrap_or_else(|error| panic!("length {text:?}: {error}")),
        Value::Number(number) => number.as_i64().expect("integer length"),
        other => panic!("case has no length: {other}"),
    }
}

fn golden_dim(case: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(case, key)).expect("golden dimension fits i32")
}

#[test]
fn golden_jpeg_marker_probe() {
    // tests/golden/jpeg-marker-probe.json (oracle-cs jpeg-marker-probe at d92d6aae): 86 cases; the output file also holds
    // the inputs, so the bytes are rebuilt from its parts.
    let golden = golden::load("jpeg-marker-probe");
    assert_eq!(golden.cases.len(), 86, "golden case count");

    let mut mismatches = Vec::new();
    for case in &golden.cases {
        let name = str_of(case, "name");
        let bytes = golden_bytes(case);
        assert_eq!(
            i64_of(case, "total_length"),
            len_i64(&bytes),
            "{name}: rebuilt bytes do not match total_length"
        );
        let offset = i64_of(case, "offset");
        let length = golden_length(case);

        let result = match str_of(case, "source") {
            "memory" => try_read_lossy_frame(&mut InMemoryRawHeaderSource::new(bytes), offset, length),
            "throw-invalid" => try_read_lossy_frame(
                &mut FailingSource {
                    kind: FailKind::InvalidData,
                },
                offset,
                length,
            ),
            "throw-io" => try_read_lossy_frame(&mut FailingSource { kind: FailKind::Io }, offset, length),
            other => panic!("{name}: unknown source kind {other:?}"),
        };
        let got = match &result {
            Ok(Some(frame)) => (true, false, Some((frame.width, frame.height))),
            Ok(None) => (false, false, None),
            Err(error) => (false, error.is_io_failure(), None),
        };

        let expected_ok = bool_of(case, "ok");
        let expected_io = bool_of(case, "io_failure");
        let expected_dims = expected_ok.then(|| (golden_dim(case, "width"), golden_dim(case, "height")));
        let want = (expected_ok, expected_io, expected_dims);
        if got != want {
            mismatches.push(format!("{name}: got {got:?}, want {want:?}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} golden cases differ:\n{}",
        mismatches.len(),
        golden.cases.len(),
        mismatches.join("\n")
    );
}
