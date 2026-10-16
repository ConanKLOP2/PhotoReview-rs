//! Red tests for W5-11b `PreviewSelector` (`preview_selector.rs`, C# baseline d92d6aae `src/PhotoReview.Imaging.Raw/PreviewSelector.cs`).
//!
//! Ports every test named in `contracts/W5-11b.md`:
//! * `Raw/PreviewSelectorMutationGapTests.cs` (all), `Raw/PreviewSelectorUnknownSizeTests.cs` (all 7),
//!   `Raw/PreviewSelectorJpegFrameTests.cs` (all but the RawDecoder RAF test, W5-12),
//!   `Raw/PreviewSelectorColorSpaceTests.cs` (the non-corpus tests),
//! * the `PreviewSelector` tests of `RawContainerGapRound2Tests`, `RawHeaderHostileInputTests`, `RawErrorHandlingReviewTests`,
//!   `RawJpegFrameAndHeaderSourceGapTests` (the `TryReadJpegFrame_*` ones), `Tiff/JpegProbeAndDngChainTests`
//!   (`PreviewSelector_FillBytesBeforeFrameHeader_StillResolvesDimensions`), `RawDecoderResolvedInfoCacheTests`,
//!   `RawDecoderPreviewHardeningTests`, `RawDecoderTests` (`PreviewSelector_SelectsSmallestMatchingBox`),
//! * the `PreviewSelector` half of `Robustness/BinaryReaderFuzzTests.Fuzz_JpegMarkerWalkers_FailOnlyCleanly`,
//! * golden tests over `tests/golden/preview-selector-select.json` (160 cases), `preview-selector-jpeg-frame.json` (142)
//!   and `adobe-rgb-exif.json` (61). The golden files carry their inputs; the bytes are rebuilt from `parts`.
//!
//! Documented skips (not ported here): `SelectPreview_NoJpegPreviewKind_FallsBackToTheFirstPreview` (the Rust
//! `EmbeddedPreviewKind` has one variant, contract deviation (c)); the three `Corpus_*` tests, which go to
//! `tests/preview_selector_corpus.rs` (contract Open question 1: the lead hands them to the last of W5-11a / W5-12 to merge);
//! `RawDecoder_ReadInfoOnRafWithSofPastSixtyFourKiB_ReturnsTheJpegDimensions` (W5-12).
//!
//! Rust-only tests (no C# twin, contract "New Rust tests") are marked as such.
//!
//! Stub bodies are `todo!()`, so every test that reaches the selector fails until the porter fills them in.

use std::io;

use pr_imaging::decode_types::DecodeBox;
use pr_imaging_raw::formats::limits::MAX_HEADER_BYTES;
use pr_imaging_raw::preview_selector::{
    is_adobe_rgb_exif, select_preview, select_preview_resolved, try_read_jpeg_frame, walk_jpeg_header,
};
use pr_imaging_raw::{
    EmbeddedPreview, EmbeddedPreviewKind, InMemoryRawHeaderSource, PreviewColorSpace, RAW_HEADER_BLOCK_SIZE,
    RawHeaderError, RawHeaderSource,
};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

const SOI: [u8; 2] = [0xFF, 0xD8];

// ---------------------------------------------------------------------------------------------- byte builders

/// C# `PreviewSelectorMutationGapTests.Sof(marker, width, height)`: a 13-byte SOFn frame header, 8-bit, 1 component.
fn sof(marker: u8, width: u16, height: u16) -> Vec<u8> {
    let [width_high, width_low] = width.to_be_bytes();
    let [height_high, height_low] = height.to_be_bytes();
    vec![
        0xFF,
        marker,
        0x00,
        0x0B,
        0x08,
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

/// C# `PreviewSelectorJpegFrameTests`/`RawJpegFrameAndHeaderSourceGapTests.Sof(marker, declaredLength, payload)`: the bytes
/// after the marker are written verbatim, the declared length is whatever the test says.
fn sof_raw(marker: u8, declared_length: u16, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xFF, marker];
    bytes.extend_from_slice(&declared_length.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

/// A segment with a zeroed payload of `payload_len` bytes (C# `Segment(marker, payloadLength)`).
fn segment(marker: u8, payload_len: usize) -> Vec<u8> {
    segment_with_payload(marker, &vec![0u8; payload_len])
}

/// A segment with `payload` after its 16-bit length field (the length counts the field itself).
fn segment_with_payload(marker: u8, payload: &[u8]) -> Vec<u8> {
    let length = u16::try_from(payload.len() + 2).expect("segment length fits u16");
    let mut bytes = vec![0xFF, marker];
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

/// C# `PreviewSelectorJpegFrameTests.AppSegment(marker, payloadLength)`: filled with 0x11.
fn app_segment(marker: u8, payload_len: usize) -> Vec<u8> {
    segment_with_payload(marker, &vec![0x11u8; payload_len])
}

/// A minimal lossy JPEG: SOI, APP0 JFIF, SOF0 `width` x `height`, SOS, EOI (C# `SyntheticRawBuilder.CreateMinimalJpeg`).
fn minimal_jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut jpeg = SOI.to_vec();
    jpeg.extend(segment_with_payload(
        0xE0,
        &[
            b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00,
        ],
    ));
    jpeg.extend(sof(0xC0, width, height));
    jpeg.extend(segment_with_payload(0xDA, &[0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]));
    jpeg.extend_from_slice(&[0xFF, 0xD9]);
    jpeg
}

/// C# `TiffBytes.AsLosslessJpeg`: the first SOF0 becomes SOF3 (lossless, not decodable by the selector).
fn as_lossless_jpeg(jpeg: &[u8]) -> Vec<u8> {
    let mut out = jpeg.to_vec();
    let at = out
        .windows(2)
        .position(|pair| pair == [0xFF, 0xC0])
        .expect("the minimal JPEG has a baseline SOF");
    out[at + 1] = 0xC3;
    out
}

/// `minimal_jpeg` with `count` fill bytes (0xFF) right after SOI, i.e. before every other segment.
fn with_fill_bytes(jpeg: &[u8], count: usize) -> Vec<u8> {
    let mut out = SOI.to_vec();
    out.extend(std::iter::repeat_n(0xFF, count));
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// `minimal_jpeg` with `segments` inserted right after SOI (C# `JpegWithLeadingSegments`).
fn jpeg_with_leading_segments(width: u16, height: u16, segments: &[Vec<u8>]) -> Vec<u8> {
    let jpeg = minimal_jpeg(width, height);
    let mut out = SOI.to_vec();
    for segment in segments {
        out.extend_from_slice(segment);
    }
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// EXIF-sized APP1 (65450 bytes) plus an APP2 (20000 bytes): the SOF ends up past 85 KB (C# `FujiShapedJpeg`).
fn fuji_shaped_jpeg(width: u16, height: u16) -> Vec<u8> {
    jpeg_with_leading_segments(width, height, &[app_segment(0xE1, 65_450), app_segment(0xE2, 20_000)])
}

/// Offset of the first `FF C0` (C# `SofOffset`).
fn sof_offset(jpeg: &[u8]) -> usize {
    jpeg.windows(2)
        .position(|pair| pair == [0xFF, 0xC0])
        .expect("the JPEG has a baseline SOF")
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn len_i64(bytes: &[u8]) -> i64 {
    i64::try_from(bytes.len()).expect("test input length fits i64")
}

/// C# `ExifWithInteropIndex`: `"Exif\0\0"` + TIFF with IFD0 (0x8769 -> 26), Exif IFD (0xA005 -> 44) and Interop IFD
/// (0x0001 ASCII x4 inline = `interop` padded with NUL), in the requested byte order, followed by `extra_tail`.
fn exif_with_interop_index(interop: &str, little: bool, extra_tail: &[u8]) -> Vec<u8> {
    struct Writer {
        little: bool,
        out: Vec<u8>,
    }
    impl Writer {
        fn u16(&mut self, value: u16) {
            let bytes = if self.little {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            };
            self.out.extend_from_slice(&bytes);
        }
        fn u32(&mut self, value: u32) {
            let bytes = if self.little {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            };
            self.out.extend_from_slice(&bytes);
        }
    }

    let mut w = Writer {
        little,
        out: b"Exif\0\0".to_vec(),
    };
    w.out.extend_from_slice(if little { b"II" } else { b"MM" });
    w.u16(42);
    w.u32(8);
    // IFD0 at 8: one entry, Exif IFD pointer -> 26.
    w.u16(1);
    w.u16(0x8769);
    w.u16(4);
    w.u32(1);
    w.u32(26);
    w.u32(0);
    // Exif IFD at 26: one entry, Interop IFD pointer -> 44.
    w.u16(1);
    w.u16(0xA005);
    w.u16(4);
    w.u32(1);
    w.u32(44);
    w.u32(0);
    // Interop IFD at 44: InteropIndex, ASCII x4 inline.
    w.u16(1);
    w.u16(0x0001);
    w.u16(2);
    w.u32(4);
    let mut value = [0u8; 4];
    for (slot, byte) in value.iter_mut().zip(interop.bytes().take(3)) {
        *slot = byte;
    }
    w.out.extend_from_slice(&value);
    w.u32(0);
    w.out.extend_from_slice(extra_tail);
    w.out
}

/// C# `JpegWithExif`: `minimal_jpeg(width, height)` with an APP1 carrying `exif_payload` right after SOI.
fn jpeg_with_exif(exif_payload: &[u8], width: u16, height: u16) -> Vec<u8> {
    let jpeg = minimal_jpeg(width, height);
    let length = u16::try_from(exif_payload.len() + 2).expect("APP1 length fits u16");
    let mut out = SOI.to_vec();
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(exif_payload);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// The Adobe RGB EXIF payload used by most colour-space tests.
fn adobe_exif(little: bool) -> Vec<u8> {
    exif_with_interop_index("R03", little, &[])
}

// ---------------------------------------------------------------------------------------------- sources

/// Which failure a [`TestSource`] reports on every read.
enum Backing {
    Memory(InMemoryRawHeaderSource),
    /// C# `throw new InvalidDataException("budget exhausted")` (no inner exception: swallowed by the selector).
    Invalid(i64),
    /// C# `InvalidDataException("read failed", IOException("disk"))`: an I/O failure, propagated by the selector.
    Io(i64),
}

/// A counting header source: `reads()` counts every `read` call (failing ones included, like the goldens), and
/// `bytes_charged()` is the budget charged by the in-memory backing (0 for the throwing ones).
struct TestSource {
    backing: Backing,
    reads: usize,
}

impl TestSource {
    fn memory(bytes: Vec<u8>) -> Self {
        Self {
            backing: Backing::Memory(InMemoryRawHeaderSource::new(bytes)),
            reads: 0,
        }
    }

    /// A memory source whose budget was already spent by a read of `[offset, offset + count)` (not counted as a read).
    fn memory_preburned(bytes: Vec<u8>, offset: i64, count: i32) -> Self {
        let mut inner = InMemoryRawHeaderSource::new(bytes);
        inner.read(offset, count).expect("preburn read");
        Self {
            backing: Backing::Memory(inner),
            reads: 0,
        }
    }

    fn invalid(length: i64) -> Self {
        Self {
            backing: Backing::Invalid(length),
            reads: 0,
        }
    }

    fn io(length: i64) -> Self {
        Self {
            backing: Backing::Io(length),
            reads: 0,
        }
    }

    fn reads(&self) -> usize {
        self.reads
    }

    fn bytes_charged(&self) -> i64 {
        match &self.backing {
            Backing::Memory(inner) => inner.total_bytes_read(),
            Backing::Invalid(_) | Backing::Io(_) => 0,
        }
    }
}

impl RawHeaderSource for TestSource {
    fn length(&self) -> i64 {
        match &self.backing {
            Backing::Memory(inner) => inner.length(),
            Backing::Invalid(length) | Backing::Io(length) => *length,
        }
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.reads += 1;
        match &mut self.backing {
            Backing::Memory(inner) => inner.read(offset, count),
            Backing::Invalid(_) => Err(RawHeaderError::invalid_data("budget exhausted")),
            Backing::Io(_) => Err(RawHeaderError::invalid_data_io("read failed", io::Error::other("disk"))),
        }
    }
}

/// Every read returns fewer bytes than asked for (a test double violating the trait; contract Rust-only test, deviation (b)).
struct ShortReadSource {
    data: Vec<u8>,
}

impl RawHeaderSource for ShortReadSource {
    fn length(&self) -> i64 {
        len_i64(&self.data)
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let start = usize::try_from(offset).expect("non-negative offset");
        let wanted = usize::try_from(count).expect("non-negative count");
        let got = wanted / 2;
        Ok(&self.data[start..start + got])
    }
}

fn zeros() -> InMemoryRawHeaderSource {
    InMemoryRawHeaderSource::new(vec![0u8; 256])
}

fn known(index: i32, width: i32, height: i32) -> EmbeddedPreview {
    EmbeddedPreview::new(
        index,
        0,
        100,
        EmbeddedPreviewKind::Jpeg,
        width,
        height,
        PreviewColorSpace::Unknown,
    )
}

fn chosen_index(previews: &[EmbeddedPreview], request_box: DecodeBox, orientation: i32) -> i32 {
    let mut source = zeros();
    select_preview(&mut source, previews, request_box, orientation)
        .expect("no error")
        .expect("a preview is chosen")
        .index
}

// ---------------------------------------------------------------------------------------------- selection: request box and ranking

#[test]
fn select_preview_request_box_picks_the_smallest_preview_whose_both_sides_fit() {
    // C#: PreviewSelectorMutationGapTests.SelectPreview_RequestBox_PicksTheSmallestPreviewWhoseBothSidesFit (21 rows)
    const ROWS: [(i32, i32, i32, i32); 21] = [
        (0, 0, 1, 2),
        (-5, -5, 1, 2),
        (0, 1000, 1, 1),
        (1000, 0, 1, 1),
        (1000, 1000, 1, 1),
        (160, 120, 1, 0),
        (160, 0, 1, 0),
        (0, 120, 1, 0),
        (1620, 100, 1, 1),
        (100, 1080, 1, 1),
        (1621, 100, 1, 2),
        (100, 1081, 1, 2),
        (2000, 100, 1, 2),
        (100, 1100, 1, 2),
        (0, 1100, 1, 2),
        (1000, 1500, 1, 2),
        (1000, 1500, 6, 1),
        (1000, 1500, 5, 1),
        (1000, 1500, 8, 1),
        (1000, 1500, 4, 2),
        (20000, 20000, 1, 2),
    ];
    let previews = [known(0, 160, 120), known(1, 1620, 1080), known(2, 6000, 4000)];

    for (box_width, box_height, orientation, expected) in ROWS {
        let got = chosen_index(&previews, DecodeBox::new(box_width, box_height), orientation);
        assert_eq!(got, expected, "box {box_width}x{box_height}, orientation {orientation}");
    }
}

#[test]
fn select_preview_long_side_of_exactly_one_thousand_is_viewable_and_outranks_a_larger_thumbnail() {
    // C#: PreviewSelectorMutationGapTests.SelectPreview_LongSideOfExactlyOneThousand_IsViewableAndOutranksALargerThumbnail
    let previews = [known(0, 1000, 500), known(1, 999, 999)];

    assert_eq!(chosen_index(&previews, DecodeBox::UNBOUNDED, 1), 0);
}

#[test]
fn select_preview_partially_known_size_that_cannot_be_resolved_is_ranked_by_its_byte_length_not_by_an_area_of_zero() {
    // C#: PreviewSelectorMutationGapTests.SelectPreview_PartiallyKnownSizeThatCannotBeResolved_IsRankedByItsByteLengthNotByAnAreaOfZero
    for (width, height) in [(0, 500), (500, 0)] {
        let mut source = TestSource::memory(vec![0u8; 12_000]);
        let previews = [
            EmbeddedPreview::new(
                0,
                0,
                10_000,
                EmbeddedPreviewKind::Jpeg,
                width,
                height,
                PreviewColorSpace::Unknown,
            ),
            EmbeddedPreview::new(
                1,
                10_000,
                100,
                EmbeddedPreviewKind::Jpeg,
                160,
                120,
                PreviewColorSpace::Unknown,
            ),
        ];

        let chosen = select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1)
            .expect("no error")
            .expect("a preview is chosen");

        assert_eq!(chosen.index, 0, "partial size ({width}, {height})");
    }
}

#[test]
fn select_preview_partially_known_size_of_a_real_jpeg_is_resolved_from_its_frame_header() {
    // C#: PreviewSelectorMutationGapTests.SelectPreview_PartiallyKnownSizeOfARealJpeg_IsResolvedFromItsFrameHeader
    for (width, height) in [(0, 500), (500, 0)] {
        let jpeg = minimal_jpeg(2000, 1500);
        let mut file = jpeg.clone();
        file.extend(std::iter::repeat_n(0u8, 1000));
        let previews = [
            EmbeddedPreview::new(
                0,
                0,
                len_i64(&jpeg),
                EmbeddedPreviewKind::Jpeg,
                width,
                height,
                PreviewColorSpace::Unknown,
            ),
            EmbeddedPreview::new(
                1,
                len_i64(&jpeg),
                100,
                EmbeddedPreviewKind::Jpeg,
                160,
                120,
                PreviewColorSpace::Unknown,
            ),
        ];
        let mut source = TestSource::memory(file);

        let chosen = select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1)
            .expect("no error")
            .expect("a preview is chosen");

        assert_eq!(
            (chosen.index, chosen.width, chosen.height),
            (0, 2000, 1500),
            "partial size ({width}, {height})"
        );
    }
}

#[test]
fn select_preview_unknown_size_jpeg_of_eight_thousand_bytes_outranks_a_160x120_thumbnail() {
    // C#: PreviewSelectorMutationGapTests.SelectPreview_UnknownSizeJpegOfEightThousandBytes_OutranksA160x120Thumbnail
    // Estimated at 4 pixels per byte: 32,000 against the thumbnail's 19,200.
    let mut source = TestSource::memory(vec![0u8; 8_100]);
    let previews = [
        EmbeddedPreview::new(0, 0, 8_000, EmbeddedPreviewKind::Jpeg, 0, 0, PreviewColorSpace::Unknown),
        EmbeddedPreview::new(
            1,
            8_000,
            100,
            EmbeddedPreviewKind::Jpeg,
            160,
            120,
            PreviewColorSpace::Unknown,
        ),
    ];

    let chosen = select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!(chosen.index, 0);
}

#[test]
fn select_preview_single_unknown_size_preview_of_a_real_jpeg_is_resolved() {
    // C#: PreviewSelectorMutationGapTests.SelectPreview_SingleUnknownSizePreviewOfARealJpeg_IsResolved
    let jpeg = minimal_jpeg(640, 480);
    let previews = [EmbeddedPreview::new(
        0,
        0,
        len_i64(&jpeg),
        EmbeddedPreviewKind::Jpeg,
        0,
        0,
        PreviewColorSpace::Unknown,
    )];
    let mut source = TestSource::memory(jpeg);

    let chosen = select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!((chosen.width, chosen.height), (640, 480));
}

#[test]
fn select_preview_unknown_size_preview_outside_the_source_is_left_unwalked() {
    // C#: PreviewSelectorMutationGapTests.SelectPreview_UnknownSizePreviewOutsideTheSource_IsLeftUnwalked (2 rows)
    for offset in [256i64, -1] {
        let previews = [EmbeddedPreview::new(
            0,
            offset,
            100,
            EmbeddedPreviewKind::Jpeg,
            0,
            0,
            PreviewColorSpace::Unknown,
        )];
        let selection = select_preview_resolved(&mut zeros(), &previews, DecodeBox::UNBOUNDED, 1).expect("no error");

        assert_eq!(selection.resolved_previews.len(), 1);
        assert!(!selection.resolved_previews[0].header_resolved, "offset {offset}");
    }
}

#[test]
fn select_preview_known_size_preview_colour_space_walk_happens_only_for_a_readable_range() {
    // C#: PreviewSelectorMutationGapTests.SelectPreview_KnownSizePreview_ColourSpaceWalkHappensOnlyForAReadableRange (5 rows)
    const ROWS: [(i64, i64, bool); 5] = [
        (256, 100, false),
        (-1, 100, false),
        (0, 4, true),
        (0, 3, false),
        (0, 100, true),
    ];

    for (offset, length, expected_resolved) in ROWS {
        let previews = [EmbeddedPreview::new(
            0,
            offset,
            length,
            EmbeddedPreviewKind::Jpeg,
            1920,
            1280,
            PreviewColorSpace::Unknown,
        )];
        let selection = select_preview_resolved(&mut zeros(), &previews, DecodeBox::UNBOUNDED, 1).expect("no error");

        assert_eq!(
            selection.resolved_previews[0].header_resolved, expected_resolved,
            "offset {offset}, length {length}"
        );
    }
}

// ---------------------------------------------------------------------------------------------- JPEG marker walk

/// C# `PreviewSelectorMutationGapTests.Walk`: `try_read_jpeg_frame` over an in-memory source of the whole buffer.
fn walk(jpeg: &[u8]) -> Option<(i32, i32, PreviewColorSpace)> {
    let mut source = InMemoryRawHeaderSource::new(jpeg.to_vec());
    try_read_jpeg_frame(&mut source, 0, len_i64(jpeg))
        .expect("the in-memory source cannot fail")
        .map(|frame| (frame.width, frame.height, frame.color_space))
}

#[test]
fn try_read_jpeg_frame_frame_header_ending_exactly_at_the_end_of_the_range_is_read() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_FrameHeaderEndingExactlyAtTheEndOfTheRange_IsRead
    let (width, height, _) = walk(&cat(&[&SOI, &sof(0xC0, 640, 480)])).expect("frame found");

    assert_eq!((width, height), (640, 480));
}

#[test]
fn try_read_jpeg_frame_frame_header_cut_off_by_one_byte_is_rejected() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_FrameHeaderCutOffByOneByte_IsRejected
    let jpeg = cat(&[&SOI, &sof(0xC0, 640, 480)]);

    assert!(walk(&jpeg[..jpeg.len() - 1]).is_none());
}

#[test]
fn try_read_jpeg_frame_frame_header_with_minimum_five_byte_payload_is_read() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_FrameHeaderWithMinimumFiveBytePayload_IsRead
    let frame: [u8; 9] = [0xFF, 0xC0, 0x00, 0x07, 0x08, 0x01, 0xE0, 0x02, 0x80];

    let (width, height, _) = walk(&cat(&[&SOI, &frame])).expect("frame found");

    assert_eq!((width, height), (640, 480));
}

#[test]
fn try_read_jpeg_frame_frame_header_with_four_byte_payload_is_rejected() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_FrameHeaderWithFourBytePayload_IsRejected
    let frame: [u8; 9] = [0xFF, 0xC0, 0x00, 0x06, 0x08, 0x01, 0xE0, 0x02, 0x80];

    assert!(walk(&cat(&[&SOI, &frame])).is_none());
}

#[test]
fn try_read_jpeg_frame_empty_segment_before_the_frame_is_skipped() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_EmptySegmentBeforeTheFrame_IsSkipped
    let jpeg = cat(&[&SOI, &[0xFF, 0xE0, 0x00, 0x02], &sof(0xC0, 640, 480)]);

    let (width, _, _) = walk(&jpeg).expect("frame found");

    assert_eq!(width, 640);
}

#[test]
fn try_read_jpeg_frame_table_or_reserved_segment_inside_the_sof_range_is_skipped_not_treated_as_a_frame() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_TableOrReservedSegmentInsideTheSofRange_IsSkippedNotTreatedAsAFrame
    for marker in [0xC4u8, 0xC8, 0xCC] {
        let jpeg = cat(&[&SOI, &segment(marker, 20), &sof(0xC0, 640, 480)]);

        let (width, height, _) = walk(&jpeg).expect("frame found after the skipped segment");

        assert_eq!((width, height), (640, 480), "marker {marker:#04x}");
    }
}

#[test]
fn try_read_jpeg_frame_first_frame_marker_only_baseline_extended_and_progressive_are_accepted() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_FirstFrameMarker_OnlyBaselineExtendedAndProgressiveAreAccepted (8 rows)
    const ROWS: [(u8, bool); 8] = [
        (0xC0, true),
        (0xC1, true),
        (0xC2, true),
        (0xC3, false),
        (0xC5, false),
        (0xC9, false),
        (0xCA, false),
        (0xCF, false),
    ];
    for (marker, expected) in ROWS {
        // A valid baseline frame follows, so a walk that wrongly skipped the first marker would succeed.
        let jpeg = cat(&[&SOI, &sof(marker, 1000, 700), &sof(0xC0, 111, 222)]);

        let got = walk(&jpeg);

        assert_eq!(got.is_some(), expected, "marker {marker:#04x}");
        if expected {
            assert_eq!(got.map(|(w, h, _)| (w, h)), Some((1000, 700)), "marker {marker:#04x}");
        }
    }
}

#[test]
fn try_read_jpeg_frame_zero_width_or_height_is_rejected() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_ZeroWidthOrHeight_IsRejected (2 rows)
    for (width, height) in [(0u16, 480u16), (640, 0)] {
        assert!(
            walk(&cat(&[&SOI, &sof(0xC0, width, height)])).is_none(),
            "size {width}x{height}"
        );
    }
}

#[test]
fn try_read_jpeg_frame_entropy_data_or_end_marker_before_the_frame_ends_the_walk() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_EntropyDataOrEndMarkerBeforeTheFrame_EndsTheWalk (3 rows)
    for marker in [0x00u8, 0xDA, 0xD9] {
        let jpeg = cat(&[&SOI, &[0xFF, marker, 0x00, 0x02], &sof(0xC0, 640, 480)]);

        assert!(walk(&jpeg).is_none(), "marker {marker:#04x}");
    }
}

#[test]
fn try_read_jpeg_frame_standalone_marker_before_the_frame_is_skipped_without_a_length() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_StandaloneMarkerBeforeTheFrame_IsSkippedWithoutALength (4 rows)
    for marker in [0x01u8, 0xD0, 0xD4, 0xD8] {
        let jpeg = cat(&[&SOI, &[0xFF, marker], &sof(0xC0, 640, 480)]);

        let (width, height, _) = walk(&jpeg).expect("frame found after the standalone marker");

        assert_eq!((width, height), (640, 480), "marker {marker:#04x}");
    }
}

#[test]
fn try_read_jpeg_frame_frame_header_after_many_segments_is_found_only_within_the_segment_cap() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_FrameHeaderAfterManySegments_IsFoundOnlyWithinTheSegmentCap (511 / 512)
    for (segments, expected) in [(511usize, true), (512, false)] {
        let mut parts: Vec<Vec<u8>> = vec![SOI.to_vec()];
        parts.extend(std::iter::repeat_n(vec![0xFF, 0xE0, 0x00, 0x02], segments));
        parts.push(sof(0xC0, 640, 480));
        let jpeg: Vec<u8> = parts.concat();

        assert_eq!(walk(&jpeg).is_some(), expected, "{segments} segments before the frame");
    }
}

#[test]
fn try_read_jpeg_frame_run_of_fill_bytes_before_the_frame_is_tolerated_up_to_the_cap() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_RunOfFillBytesBeforeTheFrame_IsToleratedUpToTheCap (65 536 / 65 537)
    for (fill_bytes, expected) in [(65_536usize, true), (65_537, false)] {
        let jpeg = cat(&[&SOI, &vec![0xFFu8; fill_bytes], &sof(0xC0, 640, 480)]);

        assert_eq!(walk(&jpeg).is_some(), expected, "{fill_bytes} fill bytes");
    }
}

#[test]
fn try_read_jpeg_frame_no_soi_is_rejected_even_when_a_frame_header_follows() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_NoSoi_IsRejectedEvenWhenAFrameHeaderFollows
    assert!(walk(&cat(&[&[0x00, 0x00], &sof(0xC0, 640, 480)])).is_none());
}

#[test]
fn try_read_jpeg_frame_range_that_starts_outside_or_at_the_very_end_returns_false_without_throwing() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_RangeThatStartsOutsideOrAtTheVeryEnd_ReturnsFalseWithoutThrowing (3 rows)
    for offset in [-1i64, 1, 2] {
        let mut source = InMemoryRawHeaderSource::new(vec![0xFF, 0xD8]);

        let frame = try_read_jpeg_frame(&mut source, offset, 100).expect("no error");

        assert!(frame.is_none(), "offset {offset}");
    }
}

#[test]
fn try_read_jpeg_frame_exif_in_an_app2_segment_is_not_used_for_the_colour_space() {
    // C#: PreviewSelectorMutationGapTests.TryReadJpegFrame_ExifInAnApp2Segment_IsNotUsedForTheColourSpace
    let exif = adobe_exif(true);
    let length = u16::try_from(exif.len() + 2).expect("length fits u16");
    let app1 = cat(&[&[0xFF, 0xE1], &length.to_be_bytes(), &exif]);
    let app2 = cat(&[&[0xFF, 0xE2], &length.to_be_bytes(), &exif]);

    let (_, _, in_app1) = walk(&cat(&[&SOI, &app1, &sof(0xC0, 640, 480)])).expect("frame found");
    let (_, _, in_app2) = walk(&cat(&[&SOI, &app2, &sof(0xC0, 640, 480)])).expect("frame found");

    assert_eq!(in_app1, PreviewColorSpace::AdobeRgb);
    assert_eq!(in_app2, PreviewColorSpace::Unknown);
}

#[test]
fn walk_jpeg_header_reports_the_colour_space_of_a_walk_that_found_no_frame() {
    // Rust-only (contract "New Rust tests"): an APP1 with R03 before a lossless SOF3 gives no frame but AdobeRgb.
    let jpeg = as_lossless_jpeg(&jpeg_with_exif(&adobe_exif(true), 640, 480));
    let mut source = InMemoryRawHeaderSource::new(jpeg.clone());

    let walked = walk_jpeg_header(&mut source, 0, len_i64(&jpeg), true).expect("no error");

    assert_eq!(walked.frame, None);
    assert_eq!(walked.color_space, PreviewColorSpace::AdobeRgb);
}

#[test]
fn try_read_jpeg_frame_offset_i64_max_and_length_i64_min_read_nothing() {
    // Rust-only (contract "New Rust tests"): `offset + length` must never overflow, and both are rejected before any read.
    let bytes = minimal_jpeg(640, 480);
    let mut source = TestSource::memory(bytes.clone());
    assert!(
        try_read_jpeg_frame(&mut source, i64::MAX, 100)
            .expect("no error")
            .is_none()
    );
    assert_eq!(source.reads(), 0, "offset i64::MAX must not read");

    let mut source = TestSource::memory(bytes);
    assert!(
        try_read_jpeg_frame(&mut source, 0, i64::MIN)
            .expect("no error")
            .is_none()
    );
    assert_eq!(source.reads(), 0, "length i64::MIN must not read");
}

#[test]
fn select_preview_resolved_returns_a_list_of_the_input_length_in_the_input_order() {
    // Rust-only (contract "New Rust tests"): the resolved list mirrors the input slot by slot, and the caller's slice is untouched.
    let jpeg = minimal_jpeg(320, 240);
    let previews = [
        EmbeddedPreview::new(
            7,
            0,
            100,
            EmbeddedPreviewKind::Jpeg,
            160,
            120,
            PreviewColorSpace::Unknown,
        ),
        EmbeddedPreview::new(
            3,
            0,
            len_i64(&jpeg),
            EmbeddedPreviewKind::Jpeg,
            0,
            0,
            PreviewColorSpace::Unknown,
        ),
        EmbeddedPreview::new(9, 0, 50, EmbeddedPreviewKind::Jpeg, 0, 0, PreviewColorSpace::Unknown),
    ];
    let original = previews.clone();
    let mut source = TestSource::memory(jpeg);

    let selection = select_preview_resolved(&mut source, &previews, DecodeBox::UNBOUNDED, 1).expect("no error");

    assert_eq!(selection.resolved_previews.len(), previews.len());
    let indices: Vec<i32> = selection.resolved_previews.iter().map(|p| p.index).collect();
    assert_eq!(indices, vec![7, 3, 9]);
    assert_eq!(
        previews.as_slice(),
        original.as_slice(),
        "the caller's list must not change"
    );
}

#[test]
fn walk_a_source_that_returns_fewer_bytes_than_requested_ends_the_walk_instead_of_panicking() {
    // Rust-only (contract "New Rust tests", deviation (b)): C# would throw IndexOutOfRangeException here.
    let jpeg = cat(&[&SOI, &sof(0xC0, 640, 480)]);
    let mut source = ShortReadSource { data: jpeg.clone() };

    let frame = try_read_jpeg_frame(&mut source, 0, len_i64(&jpeg)).expect("no error");

    assert!(frame.is_none());
    let previews = [EmbeddedPreview::new(
        0,
        0,
        len_i64(&jpeg),
        EmbeddedPreviewKind::Jpeg,
        0,
        0,
        PreviewColorSpace::Unknown,
    )];
    let mut source = ShortReadSource { data: jpeg };
    assert!(select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1).is_ok());
}

// ---------------------------------------------------------------------------------------------- JPEG frame (large headers)

#[test]
fn try_read_jpeg_frame_sof_past_sixty_four_kib_is_found() {
    // C#: PreviewSelectorJpegFrameTests.TryReadJpegFrame_SofPastSixtyFourKiB_IsFound
    let jpeg = fuji_shaped_jpeg(6000, 4000);
    assert!(sof_offset(&jpeg) > 64 * 1024);

    let (width, height, _) = walk(&jpeg).expect("frame found");

    assert_eq!((width, height), (6000, 4000));
}

#[test]
fn try_read_jpeg_frame_reads_only_segment_headers_and_the_frame_not_the_whole_range() {
    // C#: PreviewSelectorJpegFrameTests.TryReadJpegFrame_ReadsOnlySegmentHeadersAndTheFrameNotTheWholeRange
    // 300 KB of trailing scan data after the headers: the walk must never touch it.
    let mut jpeg = fuji_shaped_jpeg(6000, 4000);
    jpeg.extend(std::iter::repeat_n(0u8, 300_000));
    let mut source = TestSource::memory(jpeg.clone());

    assert!(
        try_read_jpeg_frame(&mut source, 0, len_i64(&jpeg))
            .expect("no error")
            .is_some()
    );

    // The SOF sits just past 64 KiB, so at most the first two 64 KB blocks are charged (the budget counts blocks).
    let block = i64::try_from(RAW_HEADER_BLOCK_SIZE).expect("block size fits i64");
    assert!(
        source.bytes_charged() <= 2 * block,
        "read {} bytes",
        source.bytes_charged()
    );
}

#[test]
fn try_read_jpeg_frame_preview_inside_larger_file_honours_offset() {
    // C#: PreviewSelectorJpegFrameTests.TryReadJpegFrame_PreviewInsideLargerFile_HonoursOffset
    let jpeg = fuji_shaped_jpeg(4896, 3264);
    let mut file = vec![0u8; 160];
    file.extend_from_slice(&jpeg);
    file.extend(std::iter::repeat_n(0u8, 500));
    let mut source = InMemoryRawHeaderSource::new(file);

    let frame = try_read_jpeg_frame(&mut source, 160, len_i64(&jpeg))
        .expect("no error")
        .expect("frame found");

    assert_eq!((frame.width, frame.height), (4896, 3264));
}

#[test]
fn try_read_jpeg_frame_range_ends_before_the_frame_header_reports_unknown() {
    // C#: PreviewSelectorJpegFrameTests.TryReadJpegFrame_RangeEndsBeforeTheFrameHeader_ReportsUnknown
    let jpeg = fuji_shaped_jpeg(6000, 4000);
    let mut source = InMemoryRawHeaderSource::new(jpeg);

    let frame = try_read_jpeg_frame(&mut source, 0, 70_000).expect("no error");

    assert!(frame.is_none());
}

#[test]
fn try_read_jpeg_frame_range_claimed_past_end_of_file_is_clamped_not_thrown() {
    // C#: PreviewSelectorJpegFrameTests.TryReadJpegFrame_RangeClaimedPastEndOfFile_IsClampedNotThrown
    let jpeg = fuji_shaped_jpeg(6000, 4000);
    let mut source = InMemoryRawHeaderSource::new(jpeg);

    let frame = try_read_jpeg_frame(&mut source, 0, i64::MAX / 2)
        .expect("no error")
        .expect("frame found");

    assert_eq!(frame.width, 6000);
}

#[test]
fn try_read_jpeg_frame_undecodable_frame_type_reports_unknown() {
    // C#: PreviewSelectorJpegFrameTests.TryReadJpegFrame_UndecodableFrameType_ReportsUnknown (4 rows)
    for (sof_marker, description) in [
        (0xC3u8, "lossless"),
        (0xC9, "arithmetic sequential"),
        (0xCA, "arithmetic progressive"),
        (0xC5, "hierarchical"),
    ] {
        let mut jpeg = fuji_shaped_jpeg(6000, 4000);
        let at = sof_offset(&jpeg);
        jpeg[at + 1] = sof_marker;

        assert!(walk(&jpeg).is_none(), "{description}");
    }
}

#[test]
fn try_read_jpeg_frame_extended_sequential_and_progressive_are_accepted() {
    // C#: PreviewSelectorJpegFrameTests.TryReadJpegFrame_ExtendedSequentialAndProgressive_AreAccepted (2 rows)
    for sof_marker in [0xC1u8, 0xC2] {
        let mut jpeg = fuji_shaped_jpeg(6000, 4000);
        let at = sof_offset(&jpeg);
        jpeg[at + 1] = sof_marker;

        let (width, _, _) = walk(&jpeg).expect("frame found");

        assert_eq!(width, 6000, "marker {sof_marker:#04x}");
    }
}

#[test]
fn try_read_jpeg_frame_more_segments_than_the_cap_reports_unknown_after_bounded_work() {
    // C#: PreviewSelectorJpegFrameTests.TryReadJpegFrame_MoreSegmentsThanTheCap_ReportsUnknownAfterBoundedWork
    let mut parts: Vec<Vec<u8>> = vec![SOI.to_vec()];
    parts.extend((0..600).map(|_| app_segment(0xE2, 4)));
    parts.push(minimal_jpeg(640, 480)[2..].to_vec());
    let jpeg: Vec<u8> = parts.concat();
    let mut source = TestSource::memory(jpeg.clone());

    let frame = try_read_jpeg_frame(&mut source, 0, len_i64(&jpeg)).expect("no error");

    assert!(frame.is_none());
    // One 4-byte header read per visited segment: the walk stops at the segment cap instead of following all 600.
    assert!(source.reads() <= 520, "{} reads", source.reads());
}

#[test]
fn try_read_jpeg_frame_malformed_segments_report_unknown_without_throwing() {
    // C#: PreviewSelectorJpegFrameTests.TryReadJpegFrame_MalformedSegments_ReportUnknownWithoutThrowing (5 rows)
    let rows: [&[u8]; 5] = [
        // segment length below its own 2-byte field
        &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x01, 0x00, 0x00],
        // segment runs past the range
        &[0xFF, 0xD8, 0xFF, 0xE0, 0xFF, 0xF0, 0x00, 0x00],
        // not a marker
        &[0xFF, 0xD8, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC],
        // SOS before any frame header
        &[0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02, 0x00, 0x00],
        // no SOI
        &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    ];
    for bytes in rows {
        assert!(walk(bytes).is_none(), "bytes {bytes:02x?}");
    }
}

#[test]
fn try_read_jpeg_frame_adobe_rgb_marker_in_exif_is_reported() {
    // C#: PreviewSelectorJpegFrameTests.TryReadJpegFrame_AdobeRgbMarkerInExif_IsReported
    let jpeg = jpeg_with_exif(&exif_with_interop_index("R03", true, &[]), 640, 480);

    let (_, _, colour_space) = walk(&jpeg).expect("frame found");

    assert_eq!(colour_space, PreviewColorSpace::AdobeRgb);
}

#[test]
fn select_preview_single_jpeg_with_sof_past_sixty_four_kib_resolves_dimensions() {
    // C#: PreviewSelectorJpegFrameTests.SelectPreview_SingleJpegWithSofPastSixtyFourKiB_ResolvesDimensions
    let jpeg = fuji_shaped_jpeg(6000, 4000);
    let previews = [EmbeddedPreview::new(
        0,
        0,
        len_i64(&jpeg),
        EmbeddedPreviewKind::Jpeg,
        0,
        0,
        PreviewColorSpace::Unknown,
    )];
    let mut source = TestSource::memory(jpeg);

    let chosen = select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!((chosen.width, chosen.height), (6000, 4000));
}

// ---------------------------------------------------------------------------------------------- frame gap tests

/// C# `RawJpegFrameAndHeaderSourceGapTests.Frame640x480`: the SOF payload (precision, size, components) without the marker.
const FRAME_640X480: [u8; 15] = [8, 0x01, 0xE0, 0x02, 0x80, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1];

#[test]
fn try_read_jpeg_frame_lossy_sof_frame_reports_the_dimensions_control() {
    // C#: RawJpegFrameAndHeaderSourceGapTests.TryReadJpegFrame_LossySofFrame_ReportsTheDimensions_Control (3 rows)
    for marker in [0xC0u8, 0xC1, 0xC2] {
        let jpeg = cat(&[&SOI, &sof_raw(marker, 2 + 15, &FRAME_640X480), &[0xFF, 0xD9]]);

        let (width, height, _) = walk(&jpeg).expect("frame found");

        assert_eq!((width, height), (640, 480), "marker {marker:#04x}");
    }
}

#[test]
fn try_read_jpeg_frame_sof_frame_type_that_is_not_baseline_or_huffman_lossy_returns_false() {
    // C#: RawJpegFrameAndHeaderSourceGapTests.TryReadJpegFrame_SofFrameTypeThatIsNotBaselineOrHuffmanLossy_ReturnsFalse (5 rows)
    for marker in [0xC3u8, 0xC5, 0xC9, 0xCA, 0xCB] {
        let jpeg = cat(&[&SOI, &sof_raw(marker, 2 + 15, &FRAME_640X480), &[0xFF, 0xD9]]);

        assert!(walk(&jpeg).is_none(), "marker {marker:#04x}");
    }
}

#[test]
fn try_read_jpeg_frame_sof_segment_runs_past_the_buffer_returns_false_without_throwing() {
    // C#: RawJpegFrameAndHeaderSourceGapTests.TryReadJpegFrame_SofSegmentRunsPastTheBuffer_ReturnsFalseWithoutThrowing
    // Declares a 19-byte segment but the buffer ends after the first 8 payload bytes.
    let jpeg = cat(&[&SOI, &sof_raw(0xC0, 2 + 15, &FRAME_640X480[..8])]);

    assert!(walk(&jpeg).is_none());
}

#[test]
fn try_read_jpeg_frame_sof_segment_length_far_beyond_the_buffer_returns_false() {
    // C#: RawJpegFrameAndHeaderSourceGapTests.TryReadJpegFrame_SofSegmentLengthFarBeyondTheBuffer_ReturnsFalse
    let jpeg = cat(&[&SOI, &sof_raw(0xC0, 0xFFFF, &FRAME_640X480)]);

    assert!(walk(&jpeg).is_none());
}

#[test]
fn try_read_jpeg_frame_sof_with_too_small_payload_does_not_read_the_dimensions_from_the_following_segment() {
    // C#: RawJpegFrameAndHeaderSourceGapTests.TryReadJpegFrame_SofWithTooSmallPayload_DoesNotReadTheDimensionsFromTheFollowingSegment
    // The SOF says it has a 2-byte payload; the 4 bytes after it belong to the next segment (0x01E0 / 0x0280 look like a size).
    let jpeg = cat(&[
        &SOI,
        &sof_raw(0xC0, 4, &[8, 0x01]),
        &[0x01, 0xE0, 0x02, 0x80, 0xFF, 0xD9],
    ]);

    assert!(walk(&jpeg).is_none());
}

#[test]
fn try_read_jpeg_frame_buffer_ends_inside_the_first_segment_header_returns_false() {
    // C#: RawJpegFrameAndHeaderSourceGapTests.TryReadJpegFrame_BufferEndsInsideTheFirstSegmentHeader_ReturnsFalse (3 rows)
    let rows: [&[u8]; 3] = [&[0xFF, 0xD8], &[0xFF, 0xD8, 0xFF], &[0xFF, 0xD8, 0xFF, 0xC0, 0x00]];
    for jpeg in rows {
        assert!(walk(jpeg).is_none(), "bytes {jpeg:02x?}");
    }
}

#[test]
fn select_preview_fill_bytes_before_frame_header_still_resolves_dimensions() {
    // C#: Tiff/JpegProbeAndDngChainTests.PreviewSelector_FillBytesBeforeFrameHeader_StillResolvesDimensions (600 fill bytes)
    let jpeg = with_fill_bytes(&minimal_jpeg(320, 240), 600);
    let previews = [EmbeddedPreview::new(
        0,
        0,
        len_i64(&jpeg),
        EmbeddedPreviewKind::Jpeg,
        0,
        0,
        PreviewColorSpace::Unknown,
    )];
    let mut source = TestSource::memory(jpeg);

    let chosen = select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!((chosen.width, chosen.height), (320, 240));
}

// ---------------------------------------------------------------------------------------------- unknown-size selection

/// C# `PreviewSelectorUnknownSizeTests.Build`: a small known JPEG followed by an unresolvable (SOF3) JPEG range of
/// `unknown_length` bytes.
fn build_known_and_unknown(
    known_width: u16,
    known_height: u16,
    unknown_length: usize,
) -> (TestSource, EmbeddedPreview, EmbeddedPreview) {
    let known = minimal_jpeg(known_width, known_height);
    let lossless = as_lossless_jpeg(&minimal_jpeg(6000, 4000));
    let mut unknown_bytes = vec![0u8; unknown_length];
    unknown_bytes[..lossless.len()].copy_from_slice(&lossless);
    let known_length = len_i64(&known);

    let mut file = known;
    file.extend_from_slice(&unknown_bytes);
    let known_preview = EmbeddedPreview::new(
        0,
        0,
        known_length,
        EmbeddedPreviewKind::Jpeg,
        i32::from(known_width),
        i32::from(known_height),
        PreviewColorSpace::Srgb,
    );
    let unknown_preview = EmbeddedPreview::new(
        1,
        known_length,
        i64::try_from(unknown_length).expect("length fits i64"),
        EmbeddedPreviewKind::Jpeg,
        0,
        0,
        PreviewColorSpace::Srgb,
    );
    (TestSource::memory(file), known_preview, unknown_preview)
}

#[test]
fn select_preview_unbounded_unknown_size_but_by_far_largest_jpeg_beats_a_tiny_known_thumbnail() {
    // C#: PreviewSelectorUnknownSizeTests.SelectPreview_Unbounded_UnknownSizeButByFarLargestJpeg_BeatsATinyKnownThumbnail
    let (mut source, known, unknown) = build_known_and_unknown(160, 120, 300_000);

    let chosen = select_preview(&mut source, &[known, unknown.clone()], DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!(chosen.offset, unknown.offset);
}

#[test]
fn select_preview_unbounded_small_unknown_size_jpeg_does_not_beat_a_large_known_preview() {
    // C#: PreviewSelectorUnknownSizeTests.SelectPreview_Unbounded_SmallUnknownSizeJpeg_DoesNotBeatALargeKnownPreview
    let (mut source, known, unknown) = build_known_and_unknown(1620, 1080, 2_000);

    let chosen = select_preview(&mut source, &[known.clone(), unknown], DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!(chosen.offset, known.offset);
}

#[test]
fn select_preview_box_smaller_than_a_known_preview_still_picks_the_smallest_known_match() {
    // C#: PreviewSelectorUnknownSizeTests.SelectPreview_BoxSmallerThanAKnownPreview_StillPicksTheSmallestKnownMatch
    let (mut source, known, unknown) = build_known_and_unknown(1620, 1080, 300_000);

    let chosen = select_preview(&mut source, &[known.clone(), unknown], DecodeBox::new(800, 600), 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!(chosen.offset, known.offset);
}

#[test]
fn select_preview_box_larger_than_every_known_preview_falls_back_to_the_largest_including_unknown_size() {
    // C#: PreviewSelectorUnknownSizeTests.SelectPreview_BoxLargerThanEveryKnownPreview_FallsBackToTheLargestIncludingUnknownSize
    let (mut source, known, unknown) = build_known_and_unknown(160, 120, 300_000);

    let chosen = select_preview(&mut source, &[known, unknown.clone()], DecodeBox::new(4000, 3000), 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!(chosen.offset, unknown.offset);
}

#[test]
fn select_preview_unbounded_huge_unknown_size_jpeg_never_outranks_a_known_preview_of_at_least_1000_pixels() {
    // C#: PreviewSelectorUnknownSizeTests.SelectPreview_Unbounded_HugeUnknownSizeJpeg_NeverOutranksAKnownPreviewOfAtLeast1000Pixels
    // 7 MB unknown scored 28M (bytes * 4) against 1620x1080 = 1.7M: a known viewable preview always ranks first.
    let (mut source, known, unknown) = build_known_and_unknown(1620, 1080, 7_000_000);

    let chosen = select_preview(&mut source, &[known.clone(), unknown], DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!(chosen.offset, known.offset);
}

#[test]
fn select_preview_two_unknown_size_jpegs_picks_the_larger_by_bytes() {
    // C#: PreviewSelectorUnknownSizeTests.SelectPreview_TwoUnknownSizeJpegs_PicksTheLargerByBytes
    let (mut source, _, unknown) = build_known_and_unknown(160, 120, 300_000);
    let smaller = EmbeddedPreview {
        index: 2,
        length: 50_000,
        ..unknown.clone()
    };

    let chosen = select_preview(&mut source, &[smaller, unknown], DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!(chosen.length, 300_000);
}

#[test]
fn select_preview_three_unknown_size_previews_resolves_all_so_the_choice_uses_every_pixel_size() {
    // C#: PreviewSelectorUnknownSizeTests.SelectPreview_ThreeUnknownSizePreviews_ResolvesAllSoTheChoiceUsesEveryPixelSize
    // The byte-largest candidate is NOT the pixel-largest one (heavier compression): every unknown-size candidate is
    // resolved and reported as resolved for the cache.
    let jpegs: Vec<Vec<u8>> = vec![
        cat(&[&minimal_jpeg(2000, 1500), &vec![0u8; 5000]]),
        cat(&[&minimal_jpeg(6000, 4000), &vec![0u8; 1000]]),
        minimal_jpeg(160, 120),
    ];
    let data: Vec<u8> = jpegs.concat();
    let mut offset = 0i64;
    let mut previews = Vec::new();
    for jpeg in &jpegs {
        let index = i32::try_from(previews.len()).expect("index fits i32");
        previews.push(EmbeddedPreview::new(
            index,
            offset,
            len_i64(jpeg),
            EmbeddedPreviewKind::Jpeg,
            0,
            0,
            PreviewColorSpace::Srgb,
        ));
        offset += len_i64(jpeg);
    }
    let mut source = TestSource::memory(data);

    let selection = select_preview_resolved(&mut source, &previews, DecodeBox::UNBOUNDED, 1).expect("no error");

    let chosen = selection.chosen.expect("a preview is chosen");
    assert_eq!((chosen.width, chosen.height), (6000, 4000));
    for resolved in &selection.resolved_previews {
        assert!(
            resolved.header_resolved && resolved.width > 0 && resolved.height > 0,
            "{resolved:?}"
        );
    }
}

#[test]
fn select_preview_unknown_size_preview_claiming_to_extend_past_eof_resolves_from_available_bytes() {
    // C#: RawHeaderHostileInputTests.SelectPreview_UnknownSizePreviewClaimingToExtendPastEof_ResolvesFromAvailableBytes
    let jpeg = minimal_jpeg(640, 480);
    let mut source = TestSource::memory(jpeg.clone());
    let preview = EmbeddedPreview::new(
        0,
        0,
        len_i64(&jpeg) + 1_000_000,
        EmbeddedPreviewKind::Jpeg,
        0,
        0,
        PreviewColorSpace::Unknown,
    );

    let chosen = select_preview(&mut source, &[preview], DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!((chosen.width, chosen.height), (640, 480));
}

#[test]
fn select_preview_unknown_size_preview_starting_near_eof_and_overlong_does_not_throw() {
    // C#: RawHeaderHostileInputTests.SelectPreview_UnknownSizePreviewStartingNearEofAndOverlong_DoesNotThrow
    let mut source = TestSource::memory(vec![0u8; 100]);
    let preview = EmbeddedPreview::new(
        0,
        98,
        10_000,
        EmbeddedPreviewKind::Jpeg,
        0,
        0,
        PreviewColorSpace::Unknown,
    );

    let chosen = select_preview(&mut source, std::slice::from_ref(&preview), DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    // The walk found no frame header, so the size stays unknown; the preview comes back marked as looked-up.
    assert_eq!(
        chosen,
        EmbeddedPreview {
            header_resolved: true,
            ..preview
        }
    );
}

#[test]
fn select_preview_four_byte_unknown_size_preview_is_marked_resolved_without_a_walk() {
    // C#: Raw/RawContainerGapRound2Tests.SelectPreview_FourByteUnknownSizePreview_IsMarkedResolvedWithoutAWalk
    let previews = [EmbeddedPreview::new(
        0,
        0,
        4,
        EmbeddedPreviewKind::Jpeg,
        0,
        0,
        PreviewColorSpace::Unknown,
    )];
    let mut source = InMemoryRawHeaderSource::new(vec![0xFF, 0xD8, 0xFF, 0xD9, 0, 0, 0, 0]);

    let selection = select_preview_resolved(&mut source, &previews, DecodeBox::UNBOUNDED, 1).expect("no error");

    assert!(selection.chosen.as_ref().expect("a preview is chosen").header_resolved);
    assert!(selection.resolved_previews[0].header_resolved);
}

#[test]
fn select_preview_io_failure_while_walking_jpeg_header_propagates() {
    // C#: RawErrorHandlingReviewTests.SelectPreview_IoFailureWhileWalkingJpegHeader_Propagates (2 rows)
    for (width, height) in [(0, 0), (4000, 3000)] {
        let mut source = TestSource::io(1_000_000);
        let previews = [EmbeddedPreview::new(
            0,
            1000,
            5000,
            EmbeddedPreviewKind::Jpeg,
            width,
            height,
            PreviewColorSpace::Unknown,
        )];

        let error =
            select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1).expect_err("an I/O failure is propagated");

        assert!(error.is_io_failure(), "size {width}x{height}: {error}");
    }
}

#[test]
fn select_preview_non_io_failure_while_walking_jpeg_header_stays_unresolved() {
    // C#: RawErrorHandlingReviewTests.SelectPreview_NonIoFailureWhileWalkingJpegHeader_StaysUnresolved (2 rows)
    for (width, height) in [(0, 0), (4000, 3000)] {
        let mut source = TestSource::invalid(1_000_000);
        let previews = [EmbeddedPreview::new(
            0,
            1000,
            5000,
            EmbeddedPreviewKind::Jpeg,
            width,
            height,
            PreviewColorSpace::Unknown,
        )];

        let selection = select_preview_resolved(&mut source, &previews, DecodeBox::UNBOUNDED, 1).expect("no error");

        assert!(selection.chosen.is_some(), "size {width}x{height}");
        assert!(!selection.resolved_previews[0].header_resolved, "size {width}x{height}");
    }
}

#[test]
fn select_preview_header_budget_exhausted_treats_dimensions_as_unknown_instead_of_throwing() {
    // C#: RawDecoderPreviewHardeningTests.SelectPreview_HeaderBudgetExhausted_TreatsDimensionsAsUnknownInsteadOfThrowing
    let budget = usize::try_from(MAX_HEADER_BYTES).expect("budget fits usize");
    let preview_at = budget + 1000;
    let mut file = vec![0u8; budget + 100_000];
    file[preview_at..preview_at + minimal_jpeg(320, 240).len()].copy_from_slice(&minimal_jpeg(320, 240));
    let mut source = InMemoryRawHeaderSource::new(file);
    // Burns the whole header budget before the selection.
    source
        .read(0, MAX_HEADER_BYTES)
        .expect("the first read fits the budget");
    let previews = [
        EmbeddedPreview::new(
            0,
            preview_at as i64,
            200,
            EmbeddedPreviewKind::Jpeg,
            0,
            0,
            PreviewColorSpace::Unknown,
        ),
        EmbeddedPreview::new(
            1,
            preview_at as i64,
            100,
            EmbeddedPreviewKind::Jpeg,
            0,
            0,
            PreviewColorSpace::Unknown,
        ),
    ];

    let chosen =
        select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1).expect("budget exhaustion is not an error");

    assert!(chosen.is_some());
}

#[test]
fn select_preview_selects_smallest_matching_box() {
    // C#: RawDecoderTests.PreviewSelector_SelectsSmallestMatchingBox
    let p_small = EmbeddedPreview::new(
        100,
        0,
        1000,
        EmbeddedPreviewKind::Jpeg,
        320,
        240,
        PreviewColorSpace::Srgb,
    );
    let p_medium = EmbeddedPreview::new(
        200,
        0,
        2000,
        EmbeddedPreviewKind::Jpeg,
        1024,
        768,
        PreviewColorSpace::Srgb,
    );
    let p_large = EmbeddedPreview::new(
        300,
        0,
        5000,
        EmbeddedPreviewKind::Jpeg,
        4000,
        3000,
        PreviewColorSpace::Srgb,
    );
    let previews = [p_small, p_large, p_medium];
    let mut source = InMemoryRawHeaderSource::new(vec![0u8; 64]);

    // Request box 800x600 -> should pick pMedium (1024x768).
    let selected = select_preview(&mut source, &previews, DecodeBox::new(800, 600), 1)
        .expect("no error")
        .expect("a preview is chosen");
    assert_eq!((selected.width, selected.height), (1024, 768));

    // Request unbounded -> should pick largest (4000x3000).
    let selected = select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");
    assert_eq!((selected.width, selected.height), (4000, 3000));
}

// ---------------------------------------------------------------------------------------------- caching (walk once)

#[test]
fn select_preview_unknown_dimensions_and_colour_space_walks_the_jpeg_once() {
    // C#: RawDecoderResolvedInfoCacheTests.SelectPreview_UnknownDimensionsAndColourSpace_WalksTheJpegOnce
    let jpeg = minimal_jpeg(320, 240);
    let preview = EmbeddedPreview::new(
        0,
        0,
        len_i64(&jpeg),
        EmbeddedPreviewKind::Jpeg,
        0,
        0,
        PreviewColorSpace::Unknown,
    );

    let mut walk_only = TestSource::memory(jpeg.clone());
    assert!(
        try_read_jpeg_frame(&mut walk_only, 0, len_i64(&jpeg))
            .expect("no error")
            .is_some()
    );

    let mut selecting = TestSource::memory(jpeg);
    let chosen = select_preview(&mut selecting, &[preview], DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!((chosen.width, chosen.height), (320, 240));
    assert_eq!(walk_only.reads(), selecting.reads());
}

#[test]
fn select_preview_resolved_list_is_reusable_without_any_further_read() {
    // C#: RawDecoderResolvedInfoCacheTests.SelectPreview_ResolvedList_IsReusableWithoutAnyFurtherRead
    let jpeg = minimal_jpeg(320, 240);
    let preview = EmbeddedPreview::new(
        0,
        0,
        len_i64(&jpeg),
        EmbeddedPreviewKind::Jpeg,
        0,
        0,
        PreviewColorSpace::Unknown,
    );
    let mut source = TestSource::memory(jpeg);

    let first = select_preview_resolved(&mut source, std::slice::from_ref(&preview), DecodeBox::UNBOUNDED, 1)
        .expect("no error");
    let reads_after_first = source.reads();
    let again = select_preview(&mut source, &first.resolved_previews, DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert!(reads_after_first > 0);
    assert_eq!(source.reads(), reads_after_first);
    assert_eq!((again.width, again.height), (320, 240));
}

// ---------------------------------------------------------------------------------------------- Adobe RGB and colour space

#[test]
fn select_preview_declared_size_with_interop_r03_is_adobe_rgb() {
    // C#: PreviewSelectorColorSpaceTests.SelectPreview_DeclaredSizeWithInteropR03_IsAdobeRgb (LE, BE)
    for little in [true, false] {
        let jpeg = jpeg_with_exif(&exif_with_interop_index("R03", little, &[]), 640, 480);
        let previews = [EmbeddedPreview::new(
            0,
            0,
            len_i64(&jpeg),
            EmbeddedPreviewKind::Jpeg,
            1920,
            1280,
            PreviewColorSpace::Unknown,
        )];
        let mut source = TestSource::memory(jpeg);

        let chosen = select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1)
            .expect("no error")
            .expect("a preview is chosen");

        assert_eq!((chosen.width, chosen.height), (1920, 1280), "little endian: {little}");
        assert_eq!(
            chosen.color_space,
            PreviewColorSpace::AdobeRgb,
            "little endian: {little}"
        );
    }
}

#[test]
fn select_preview_declared_size_with_interop_r98_stays_unknown() {
    // C#: PreviewSelectorColorSpaceTests.SelectPreview_DeclaredSizeWithInteropR98_StaysUnknown
    let jpeg = jpeg_with_exif(&exif_with_interop_index("R98", true, &[]), 640, 480);
    let previews = [EmbeddedPreview::new(
        0,
        0,
        len_i64(&jpeg),
        EmbeddedPreviewKind::Jpeg,
        1920,
        1280,
        PreviewColorSpace::Unknown,
    )];
    let mut source = TestSource::memory(jpeg);

    let chosen = select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!(chosen.color_space, PreviewColorSpace::Unknown);
}

#[test]
fn select_preview_two_previews_resolves_the_colour_space_of_the_chosen_one() {
    // C#: PreviewSelectorColorSpaceTests.SelectPreview_TwoPreviews_ResolvesTheColourSpaceOfTheChosenOne
    let adobe = jpeg_with_exif(&exif_with_interop_index("R03", true, &[]), 1620, 1080);
    let small = minimal_jpeg(160, 120);
    let mut file = adobe.clone();
    file.extend_from_slice(&small);
    // The C# test builds its second preview as (index = adobe.Length, offset = 0): copied as is, it does not change the outcome.
    let previews = [
        EmbeddedPreview::new(
            0,
            0,
            len_i64(&adobe),
            EmbeddedPreviewKind::Jpeg,
            1620,
            1080,
            PreviewColorSpace::Unknown,
        ),
        EmbeddedPreview::new(
            i32::try_from(adobe.len()).expect("length fits i32"),
            0,
            len_i64(&small),
            EmbeddedPreviewKind::Jpeg,
            160,
            120,
            PreviewColorSpace::Unknown,
        ),
    ];
    let mut source = TestSource::memory(file);

    let chosen = select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!(chosen.width, 1620);
    assert_eq!(chosen.color_space, PreviewColorSpace::AdobeRgb);
}

#[test]
fn select_preview_declared_size_reads_only_headers_and_the_exif_segment_not_the_whole_jpeg() {
    // C#: PreviewSelectorColorSpaceTests.SelectPreview_DeclaredSize_ReadsOnlyHeadersAndTheExifSegmentNotTheWholeJpeg
    let jpeg = jpeg_with_exif(&exif_with_interop_index("R03", true, &[]), 640, 480);
    let mut padded = jpeg.clone();
    padded.extend(std::iter::repeat_n(0u8, 200_000));
    let len = len_i64(&padded);
    let mut source = InMemoryRawHeaderSource::new(padded);

    let previews = [EmbeddedPreview::new(
        0,
        0,
        len,
        EmbeddedPreviewKind::Jpeg,
        1920,
        1280,
        PreviewColorSpace::Unknown,
    )];
    select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1).expect("no error");

    // Budget is charged per 64 KB block like production: only the first block (headers + EXIF), not the 200 KB of padding.
    let block = i64::try_from(RAW_HEADER_BLOCK_SIZE).expect("block size fits i64");
    assert_eq!(source.total_bytes_read(), block);
}

#[test]
fn select_preview_r03_or_adobe_rgb_text_outside_the_interop_entry_is_not_adobe_rgb() {
    // C#: PreviewSelectorColorSpaceTests.SelectPreview_R03OrAdobeRgbTextOutsideTheInteropEntry_IsNotAdobeRgb
    // MakerNote-like binary that happens to contain both byte sequences: the old 64 KB substring scan flagged this.
    let noise = b"xxR03xx Adobe RGB xx";
    let jpeg = jpeg_with_exif(&exif_with_interop_index("R98", true, noise), 640, 480);
    let previews = [EmbeddedPreview::new(
        0,
        0,
        len_i64(&jpeg),
        EmbeddedPreviewKind::Jpeg,
        1920,
        1280,
        PreviewColorSpace::Unknown,
    )];
    let mut source = TestSource::memory(jpeg);

    let chosen = select_preview(&mut source, &previews, DecodeBox::UNBOUNDED, 1)
        .expect("no error")
        .expect("a preview is chosen");

    assert_eq!(chosen.color_space, PreviewColorSpace::Unknown);
}

#[test]
fn try_read_jpeg_frame_interop_r03_is_reported_together_with_the_frame() {
    // C#: PreviewSelectorColorSpaceTests.TryReadJpegFrame_InteropR03_IsReportedTogetherWithTheFrame
    let jpeg = jpeg_with_exif(&exif_with_interop_index("R03", true, &[]), 800, 600);

    let (width, height, colour_space) = walk(&jpeg).expect("frame found");

    assert_eq!((width, height), (800, 600));
    assert_eq!(colour_space, PreviewColorSpace::AdobeRgb);
}

#[test]
fn is_adobe_rgb_exif_malformed_payload_returns_false_without_throwing() {
    // C#: PreviewSelectorColorSpaceTests.IsAdobeRgbExif_MalformedPayload_ReturnsFalseWithoutThrowing (3 rows)
    let rows: [Vec<u8>; 3] = [
        vec![],
        // IFD0 offset past the payload
        vec![
            0x45, 0x78, 0x69, 0x66, 0, 0, 0x49, 0x49, 0x2A, 0, 0xFF, 0xFF, 0xFF, 0x7F,
        ],
        // wrong TIFF magic
        vec![0x45, 0x78, 0x69, 0x66, 0, 0, 0x4D, 0x4D, 0, 0x2B, 0, 0, 0, 8],
    ];
    for payload in rows {
        assert!(!is_adobe_rgb_exif(&payload), "payload {payload:02x?}");
    }
}

// ---------------------------------------------------------------------------------------------- Adobe RGB interoperability (IsAdobeRgbExif)

#[test]
fn is_adobe_rgb_exif_reference_payload_is_adobe_rgb() {
    // C#: PreviewSelectorMutationGapTests.IsAdobeRgbExif_ReferencePayload_IsAdobeRgb
    assert!(is_adobe_rgb_exif(&adobe_exif(true)));
    assert!(is_adobe_rgb_exif(&adobe_exif(false)));
}

#[test]
fn is_adobe_rgb_exif_interop_entry_ending_exactly_at_the_end_of_the_block_is_still_recognised() {
    // C#: PreviewSelectorMutationGapTests.IsAdobeRgbExif_InteropEntryEndingExactlyAtTheEndOfTheBlock_IsStillRecognised
    let payload = adobe_exif(true);

    assert!(is_adobe_rgb_exif(&payload[..payload.len() - 4]));
}

#[test]
fn is_adobe_rgb_exif_wrong_entry_type_or_count_is_not_adobe_rgb() {
    // C#: PreviewSelectorMutationGapTests.IsAdobeRgbExif_WrongEntryTypeOrCount_IsNotAdobeRgb (4 rows)
    for (payload_index, value) in [(54usize, 7u8), (56, 3), (20, 2), (18, 3)] {
        let mut payload = adobe_exif(true);
        payload[payload_index] = value;

        assert!(!is_adobe_rgb_exif(&payload), "byte {payload_index} = {value}");
    }
}

#[test]
fn is_adobe_rgb_exif_byte_order_mark_with_only_one_matching_letter_is_rejected() {
    // C#: PreviewSelectorMutationGapTests.IsAdobeRgbExif_ByteOrderMarkWithOnlyOneMatchingLetter_IsRejected
    let mut little_endian_body = adobe_exif(true);
    little_endian_body[7] = b'M'; // "IM"
    let mut big_endian_body = adobe_exif(false);
    big_endian_body[7] = b'I'; // "MI"

    assert!(!is_adobe_rgb_exif(&little_endian_body));
    assert!(!is_adobe_rgb_exif(&big_endian_body));
}

/// C# `PutEntry`: a little-endian IFD entry (tag, type, count, inline value) at `at`.
fn put_entry(tiff: &mut [u8], at: usize, tag: u16, entry_type: u16, count: u32, value: &[u8]) {
    tiff[at..at + 2].copy_from_slice(&tag.to_le_bytes());
    tiff[at + 2..at + 4].copy_from_slice(&entry_type.to_le_bytes());
    tiff[at + 4..at + 8].copy_from_slice(&count.to_le_bytes());
    tiff[at + 8..at + 8 + value.len()].copy_from_slice(value);
}

#[test]
fn is_adobe_rgb_exif_first_ifd_offset_inside_the_tiff_header_is_rejected() {
    // C#: PreviewSelectorMutationGapTests.IsAdobeRgbExif_FirstIfdOffsetInsideTheTiffHeader_IsRejected
    // IFD0 offset 2 reads its count from the magic ("*\0" = 42 entries) and its entries from the offset field onwards;
    // entry 1 (at tiff offset 16) is the Exif IFD pointer, the Exif and Interop IFDs are well formed further on.
    let mut tiff = vec![0u8; 200];
    tiff[..4].copy_from_slice(b"II*\0");
    tiff[4..8].copy_from_slice(&2u32.to_le_bytes());
    put_entry(&mut tiff, 16, 0x8769, 4, 1, &100u32.to_le_bytes());
    tiff[100..102].copy_from_slice(&1u16.to_le_bytes());
    put_entry(&mut tiff, 102, 0xA005, 4, 1, &130u32.to_le_bytes());
    tiff[130..132].copy_from_slice(&1u16.to_le_bytes());
    put_entry(&mut tiff, 132, 0x0001, 2, 4, b"R03\0");

    assert!(!is_adobe_rgb_exif(&cat(&[b"Exif\0\0", &tiff])));
}

#[test]
fn is_adobe_rgb_exif_tag_found_only_as_the_second_entry_of_each_ifd_is_found() {
    // C#: PreviewSelectorMutationGapTests.IsAdobeRgbExif_TagFoundOnlyAsTheSecondEntryOfEachIfd_IsFound
    // Every IFD gets a leading decoy entry; the real entry is the second one.
    fn decoy(tag: u16) -> Vec<u8> {
        let [tag_low, tag_high] = tag.to_le_bytes();
        vec![tag_low, tag_high, 4, 0, 1, 0, 0, 0, 0, 0, 0, 0]
    }
    fn entry(tag: u16, entry_type: u16, count: u32, value: &[u8]) -> Vec<u8> {
        let mut bytes = tag.to_le_bytes().to_vec();
        bytes.extend_from_slice(&entry_type.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(value);
        bytes
    }
    fn ifd(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = vec![u8::try_from(entries.len()).expect("entry count fits u8"), 0];
        bytes.extend(entries.concat());
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes
    }

    let mut tiff = b"II*\0".to_vec();
    tiff.extend_from_slice(&8u32.to_le_bytes());
    let ifd0_size = 2 + (2 * 12) + 4;
    let exif_at = u32::try_from(8 + ifd0_size).expect("offset fits u32");
    let exif_size = 2 + (2 * 12) + 4;
    let interop_at = exif_at + u32::try_from(exif_size).expect("size fits u32");
    let body = cat(&[
        &tiff,
        &ifd(&[decoy(0x0100), entry(0x8769, 4, 1, &exif_at.to_le_bytes())]),
        &ifd(&[decoy(0x0100), entry(0xA005, 4, 1, &interop_at.to_le_bytes())]),
        &ifd(&[decoy(0x0002), entry(0x0001, 2, 4, b"R03\0")]),
    ]);

    assert!(is_adobe_rgb_exif(&cat(&[b"Exif\0\0", &body])));
}

#[test]
fn is_adobe_rgb_exif_interop_entry_that_only_appears_after_the_counted_entries_is_not_read() {
    // C#: PreviewSelectorMutationGapTests.IsAdobeRgbExif_InteropEntryThatOnlyAppearsAfterTheCountedEntries_IsNotRead
    // The Interop IFD declares one entry (tag 2); the bytes after its counted entry happen to spell a valid "R03" entry.
    let payload = adobe_exif(true);
    let fake: [u8; 12] = [0x01, 0x00, 0x02, 0x00, 0x04, 0x00, 0x00, 0x00, b'R', b'0', b'3', 0];
    let mut longer = vec![0u8; payload.len() + 8];
    longer[..payload.len()].copy_from_slice(&payload);
    longer[6 + 46] = 0x02; // entry 0 of the Interop IFD (tiff offset 46) becomes tag 0x0002
    longer[6 + 58..6 + 58 + fake.len()].copy_from_slice(&fake);

    assert!(!is_adobe_rgb_exif(&longer));
}

// ---------------------------------------------------------------------------------------------- goldens

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

/// An integer field that the goldens write either as a JSON number or as a decimal string (for `i64::MAX`).
fn golden_i64(case: &Value, key: &str) -> i64 {
    match &case[key] {
        Value::Number(number) => number
            .as_i64()
            .unwrap_or_else(|| panic!("case key '{key}' is not an i64: {number}")),
        Value::String(text) => text
            .parse()
            .unwrap_or_else(|error| panic!("case key '{key}' = {text:?}: {error}")),
        other => panic!("case has no integer '{key}': {other}"),
    }
}

fn golden_i32(case: &Value, key: &str) -> i32 {
    i32::try_from(golden_i64(case, key)).expect("golden value fits i32")
}

/// Golden colour space codes: C# `PreviewColorSpace` 0 Unknown, 1 Srgb, 2 AdobeRgb.
fn color_space_from_code(code: i64) -> PreviewColorSpace {
    match code {
        0 => PreviewColorSpace::Unknown,
        1 => PreviewColorSpace::Srgb,
        2 => PreviewColorSpace::AdobeRgb,
        other => panic!("unknown colour space code {other}"),
    }
}

fn color_space_code(colour_space: PreviewColorSpace) -> i64 {
    match colour_space {
        PreviewColorSpace::Unknown => 0,
        PreviewColorSpace::Srgb => 1,
        PreviewColorSpace::AdobeRgb => 2,
    }
}

/// The source of a golden case: `memory` (the rebuilt bytes, with the `preburn` read applied first), or a source whose every
/// read fails (`throw-invalid` / `throw-io`, length `source_length`).
fn golden_source(case: &Value) -> TestSource {
    let name = str_of(case, "name");
    match str_of(case, "source") {
        "memory" => {
            let bytes = golden_bytes(case);
            assert_eq!(
                len_i64(&bytes),
                golden_i64(case, "source_length"),
                "{name}: rebuilt bytes do not match source_length"
            );
            match case.get("preburn") {
                Some(preburn) => TestSource::memory_preburned(
                    bytes,
                    golden_i64(preburn, "offset"),
                    i32::try_from(golden_i64(preburn, "count")).expect("preburn count fits i32"),
                ),
                None => TestSource::memory(bytes),
            }
        }
        "throw-invalid" => TestSource::invalid(golden_i64(case, "source_length")),
        "throw-io" => TestSource::io(golden_i64(case, "source_length")),
        other => panic!("{name}: unknown source kind {other:?}"),
    }
}

/// The previews of a select golden case (`index`, `offset`, `length`, `width`, `height`, `color_space`, `header_resolved`).
fn golden_previews(case: &Value) -> Vec<EmbeddedPreview> {
    case["previews"]
        .as_array()
        .expect("previews array")
        .iter()
        .map(|preview| {
            let mut resolved = EmbeddedPreview::new(
                golden_i32(preview, "index"),
                golden_i64(preview, "offset"),
                golden_i64(preview, "length"),
                EmbeddedPreviewKind::Jpeg,
                golden_i32(preview, "width"),
                golden_i32(preview, "height"),
                color_space_from_code(i64_of(preview, "color_space")),
            );
            resolved.header_resolved = preview.get("header_resolved").and_then(Value::as_bool).unwrap_or(false);
            resolved
        })
        .collect()
}

fn golden_box(case: &Value) -> DecodeBox {
    let values = case["box"].as_array().expect("box array");
    let width = i32::try_from(values[0].as_i64().expect("box width")).expect("box width fits i32");
    let height = i32::try_from(values[1].as_i64().expect("box height")).expect("box height fits i32");
    DecodeBox::new(width, height)
}

/// A preview of the golden output (`chosen` / `resolved` entries) against the selection's preview.
fn assert_preview_matches(name: &str, what: &str, actual: &EmbeddedPreview, want: &Value) {
    assert_eq!(i64::from(actual.index), i64_of(want, "index"), "{name}: {what} index");
    assert_eq!(actual.offset, golden_i64(want, "offset"), "{name}: {what} offset");
    assert_eq!(actual.length, golden_i64(want, "length"), "{name}: {what} length");
    assert_eq!(i64::from(actual.width), i64_of(want, "width"), "{name}: {what} width");
    assert_eq!(
        i64::from(actual.height),
        i64_of(want, "height"),
        "{name}: {what} height"
    );
    assert_eq!(
        color_space_code(actual.color_space),
        i64_of(want, "color_space"),
        "{name}: {what} colour space"
    );
    assert_eq!(
        actual.header_resolved,
        bool_of(want, "header_resolved"),
        "{name}: {what} header_resolved"
    );
}

#[test]
fn golden_preview_selector_select() {
    // tests/golden/preview-selector-select.json (oracle-cs preview-selector-select at d92d6aae): 160 cases. Reads are
    // compared exactly (read_count of the selection itself, bytes_charged in total, preburn included).
    let golden = golden::load("preview-selector-select");
    assert_eq!(golden.cases.len(), 160, "golden case count");

    for case in &golden.cases {
        let name = str_of(case, "name");
        let mut source = golden_source(case);
        let previews = golden_previews(case);
        let request_box = golden_box(case);
        let orientation = golden_i32(case, "orientation");

        let outcome = select_preview_resolved(&mut source, &previews, request_box, orientation);

        assert_eq!(
            i64::try_from(source.reads()).expect("read count fits i64"),
            golden_i64(case, "read_count"),
            "{name}: read_count"
        );
        assert_eq!(
            source.bytes_charged(),
            golden_i64(case, "bytes_charged"),
            "{name}: bytes_charged"
        );
        match str_of(case, "error") {
            "io-failure" => {
                let error = outcome.expect_err(&format!("{name}: an I/O failure must propagate"));
                assert!(error.is_io_failure(), "{name}: {error}");
            }
            "none" => {
                let selection = outcome.unwrap_or_else(|error| panic!("{name}: unexpected error {error}"));
                let want_chosen = case.get("chosen").unwrap_or_else(|| panic!("{name}: no chosen"));
                match (want_chosen, selection.chosen.as_ref()) {
                    (Value::Null, None) => {}
                    (want, Some(actual)) => assert_preview_matches(name, "chosen", actual, want),
                    (want, None) => panic!("{name}: expected chosen {want}, got none"),
                }
                let want_resolved = case["resolved"]
                    .as_array()
                    .unwrap_or_else(|| panic!("{name}: no resolved"));
                assert_eq!(
                    selection.resolved_previews.len(),
                    want_resolved.len(),
                    "{name}: resolved length"
                );
                for (position, (actual, want)) in selection.resolved_previews.iter().zip(want_resolved).enumerate() {
                    assert_preview_matches(name, &format!("resolved[{position}]"), actual, want);
                }
            }
            other => panic!("{name}: unknown error kind {other:?}"),
        }
    }
}

#[test]
fn golden_preview_selector_jpeg_frame() {
    // tests/golden/preview-selector-jpeg-frame.json (oracle-cs preview-selector-jpeg-frame at d92d6aae): 142 cases. The
    // colour space is the `out` value of the walk even when no frame was found (walk_jpeg_header keeps it).
    let golden = golden::load("preview-selector-jpeg-frame");
    assert_eq!(golden.cases.len(), 142, "golden case count");

    for case in &golden.cases {
        let name = str_of(case, "name");
        let offset = golden_i64(case, "offset");
        let length = golden_i64(case, "length");
        let mut source = golden_source(case);

        let outcome = walk_jpeg_header(&mut source, offset, length, true);

        assert_eq!(
            i64::try_from(source.reads()).expect("read count fits i64"),
            golden_i64(case, "read_count"),
            "{name}: read_count"
        );
        assert_eq!(
            source.bytes_charged(),
            golden_i64(case, "bytes_charged"),
            "{name}: bytes_charged"
        );
        match str_of(case, "error") {
            "none" => {
                let walk = outcome.unwrap_or_else(|error| panic!("{name}: unexpected error {error}"));
                assert_eq!(walk.frame.is_some(), bool_of(case, "ok"), "{name}: ok");
                assert_eq!(
                    color_space_code(walk.color_space),
                    i64_of(case, "color_space"),
                    "{name}: colour space"
                );
                if let Some((width, height)) = walk.frame {
                    assert_eq!(
                        (i64::from(width), i64::from(height)),
                        (i64_of(case, "width"), i64_of(case, "height")),
                        "{name}: dimensions"
                    );
                }
                // The public wrapper agrees (a fresh source, so its reads are not part of the golden count).
                let wrapped = try_read_jpeg_frame(&mut golden_source(case), offset, length)
                    .unwrap_or_else(|error| panic!("{name}: wrapper error {error}"));
                assert_eq!(wrapped.is_some(), bool_of(case, "ok"), "{name}: try_read_jpeg_frame ok");
            }
            "invalid-data" => {
                let error = outcome.expect_err(&format!("{name}: budget exhaustion is an error here"));
                assert!(!error.is_io_failure(), "{name}: {error}");
            }
            "io-failure" => {
                let error = outcome.expect_err(&format!("{name}: an I/O failure must propagate"));
                assert!(error.is_io_failure(), "{name}: {error}");
            }
            other => panic!("{name}: unknown error kind {other:?}"),
        }
    }
}

#[test]
fn golden_adobe_rgb_exif() {
    // tests/golden/adobe-rgb-exif.json (oracle-cs adobe-rgb-exif at d92d6aae): 61 cases.
    let golden = golden::load("adobe-rgb-exif");
    assert_eq!(golden.cases.len(), 61, "golden case count");

    for case in &golden.cases {
        let name = str_of(case, "name");
        let payload = decode_hex(str_of(case, "hex"));
        assert_eq!(
            len_i64(&payload),
            i64_of(case, "payload_length"),
            "{name}: payload_length"
        );

        assert_eq!(is_adobe_rgb_exif(&payload), bool_of(case, "result"), "{name}: result");
    }
}

// ---------------------------------------------------------------------------------------------- fuzz

/// Deterministic xorshift64 for the mutants (no `rand` crate; the same generator as W5-02b).
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

/// The second fuzz seed: the minimal JPEG with an Exif APP1 and an ICC APP2 segment after SOI (the W5-02b seed).
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

#[test]
fn fuzz_preview_selector_never_panics() {
    // C#: BinaryReaderFuzzTests.Fuzz_JpegMarkerWalkers_FailOnlyCleanly (the PreviewSelector half; the JpegMarkerProbe half is
    // W5-02b). A panic is the failure (no catch_unwind).
    let seeds = [minimal_jpeg(64, 48), jpeg_with_exif_and_icc()];
    for (index, seed) in seeds.iter().enumerate() {
        let mut rng = XorShift64(4500 + u64::try_from(index).expect("seed index fits u64"));
        for mutant in fuzz_corpus(seed, &mut rng) {
            let length = len_i64(&mutant);
            // Frame walk over the whole buffer: a range that the buffer can honour never fails here.
            let mut source = InMemoryRawHeaderSource::new(mutant.clone());
            let frame = try_read_jpeg_frame(&mut source, 0, length);
            assert!(frame.is_ok(), "frame walk must not fail: {frame:?}");
            if let Ok(Some(found)) = frame {
                assert!(
                    found.width > 0 && found.height > 0,
                    "frame accepted with a non-positive size: {found:?}"
                );
            }
            let _ = is_adobe_rgb_exif(&mutant);
            // Lengths and offsets the container would claim but the file cannot honour.
            let previews = [
                EmbeddedPreview::new(
                    0,
                    0,
                    length,
                    EmbeddedPreviewKind::Jpeg,
                    0,
                    0,
                    PreviewColorSpace::Unknown,
                ),
                EmbeddedPreview::new(
                    1,
                    length / 2,
                    i64::MAX / 2,
                    EmbeddedPreviewKind::Jpeg,
                    0,
                    0,
                    PreviewColorSpace::Unknown,
                ),
                EmbeddedPreview::new(2, 2, 3, EmbeddedPreviewKind::Jpeg, 640, 480, PreviewColorSpace::Unknown),
            ];
            let mut source = InMemoryRawHeaderSource::new(mutant);
            assert!(select_preview(&mut source, &previews, DecodeBox::new(32, 32), 1).is_ok());
        }
    }
}
