//! Red tests for W5-10a (`bmff.rs`): `BmffBoxNavigator` (C# baseline d92d6aae, `src/PhotoReview.Imaging.Raw/Bmff/BmffBoxNavigator.cs`).
//! Each ported test carries `// C#: <Class>.<Method>` naming its C# twin. Golden: `tests/golden/bmff-navigator.json` (127 cases,
//! inputs in `tests/golden-inputs/bmff-navigator.json`), plus a deterministic fuzz of the walker over the two synthetic seeds.
//! Stub bodies are `todo!()`, so every test that reaches the navigator fails until the porter fills it in.
//!
//! Documented skips: none. Test-only helpers: `KnobSource` is the Rust twin of the C# `ShortReadSource` plus the golden
//! `fail` / `fail_after` / `short_at` knobs (C# `InMemoryRawHeaderSource` cannot fail on demand).

use std::io;

use pr_imaging_raw::bmff::{BmffBox, MAX_CHILD_BOXES, read_child_boxes, try_read_box};
use pr_imaging_raw::{InMemoryRawHeaderSource, RawHeaderError, RawHeaderSource};
use pr_test_support::golden;
use serde_json::{Value, json};

const CANON_UUID: [u8; 16] = [
    0x85, 0xC0, 0xB6, 0x87, 0x82, 0x0F, 0x11, 0xE0, 0x81, 0x11, 0xF4, 0xCE, 0x46, 0x2B, 0x6A, 0x48,
];

/// Visit budget of the C# fuzz walker (`4_096 * 8`).
const WALK_VISIT_LIMIT: usize = 4_096 * 8;

// ------------------------------------------------------------------ builders (C# `Box`, `U32`, `U64`, `Cat`, `Ftyp`)

fn u32be(value: u32) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn u64be(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// C# `Box(type, parts...)`: a regular 32-bit size header followed by the payload.
fn bx(box_type: &str, payload: &[u8]) -> Vec<u8> {
    let size = u32::try_from(8 + payload.len()).expect("test box fits u32");
    cat(&[&u32be(size), box_type.as_bytes(), payload])
}

fn ftyp() -> Vec<u8> {
    bx("ftyp", &cat(&[b"crx ", &u32be(1), b"crx "]))
}

fn src(data: Vec<u8>) -> InMemoryRawHeaderSource {
    InMemoryRawHeaderSource::new(data)
}

/// `TryReadBox` with the header source assumed healthy (the failing paths have their own tests).
fn opt(source: &mut InMemoryRawHeaderSource, offset: i64, container_end: Option<i64>) -> Option<BmffBox> {
    try_read_box(source, offset, container_end).expect("healthy in-memory source does not fail")
}

fn children(source: &mut InMemoryRawHeaderSource, parent: &BmffBox, skip: i32) -> Vec<BmffBox> {
    read_child_boxes(source, parent, skip).expect("healthy in-memory source does not fail")
}

fn parsed(source: &mut InMemoryRawHeaderSource, offset: i64) -> BmffBox {
    opt(source, offset, None).expect("box parses")
}

// ------------------------------------------------------------------ fault and short-read sources

#[derive(Clone, Copy)]
enum Failure {
    /// `RawHeaderError::invalid_data`: a malformed-source failure, not an I/O one.
    Data,
    /// `RawHeaderError::invalid_data_io` wrapping an `io::Error`: a disk or share failure.
    Io,
}

/// An in-memory source with the golden knobs: `fail` from read number `fail_after` on (reads are counted from 0), and
/// `short_at`: a read at that offset returns one byte fewer than asked. C# `ShortReadSource` is the `short_at` knob;
/// it deliberately breaks the trait contract.
struct KnobSource {
    data: Vec<u8>,
    fail: Option<Failure>,
    fail_after: usize,
    short_at: Option<i64>,
    reads: usize,
}

impl KnobSource {
    fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            fail: None,
            fail_after: 0,
            short_at: None,
            reads: 0,
        }
    }

    fn failing(mut self, kind: Failure, fail_after: usize) -> Self {
        self.fail = Some(kind);
        self.fail_after = fail_after;
        self
    }

    fn short_at(mut self, offset: i64) -> Self {
        self.short_at = Some(offset);
        self
    }
}

impl RawHeaderSource for KnobSource {
    fn length(&self) -> i64 {
        i64::try_from(self.data.len()).expect("length fits i64")
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let index = self.reads;
        self.reads += 1;
        if index >= self.fail_after {
            match self.fail {
                Some(Failure::Data) => return Err(RawHeaderError::invalid_data("fake source failed")),
                Some(Failure::Io) => {
                    return Err(RawHeaderError::invalid_data_io(
                        "fake source failed",
                        io::Error::other("fake disk failure"),
                    ));
                }
                None => {}
            }
        }
        let mut len = usize::try_from(count).map_err(|_| RawHeaderError::invalid_data("negative read count"))?;
        if self.short_at == Some(offset) {
            len -= 1;
        }
        let start = usize::try_from(offset).map_err(|_| RawHeaderError::invalid_data("negative read offset"))?;
        let end = start
            .checked_add(len)
            .ok_or_else(|| RawHeaderError::invalid_data("read out of range"))?;
        self.data
            .get(start..end)
            .ok_or_else(|| RawHeaderError::invalid_data("read out of range"))
    }
}

/// Records every header read the navigator issues (INV-8 probe).
struct RecordingSource {
    inner: InMemoryRawHeaderSource,
    reads: Vec<(i64, i32)>,
}

impl RawHeaderSource for RecordingSource {
    fn length(&self) -> i64 {
        self.inner.length()
    }

    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.reads.push((offset, count));
        self.inner.read(offset, count)
    }
}

// ------------------------------------------------------------------ BmffBoxNavigator.TryReadBox

// C#: BmffCr3MutationGapTests.TryReadBox_EightByteBoxAtVeryEndOfFile_IsRead
#[test]
fn try_read_box_eight_byte_box_at_very_end_of_file_is_read() {
    let ftyp = ftyp();
    let file = cat(&[&ftyp, &bx("free", &[])]);
    let mut source = src(file.clone());

    let box_ = opt(&mut source, ftyp.len() as i64, None).expect("8-byte box at EOF is read");

    assert_eq!(box_.box_type, "free");
    assert_eq!(box_.total_size, 8);
    assert_eq!(box_.payload_size, 0);
    assert_eq!(box_.payload_offset, file.len() as i64);
}

// C#: BmffCr3MutationGapTests.TryReadBox_HeaderTruncatedByEndOfFile_ReturnsFalseWithoutThrowing (theory 7, 1)
#[test]
fn try_read_box_header_truncated_by_end_of_file_returns_none_without_error() {
    for bytes_left in [7usize, 1] {
        let file = cat(&[&ftyp(), &vec![0u8; bytes_left]]);
        let mut source = src(file.clone());

        let got = opt(&mut source, (file.len() - bytes_left) as i64, None);

        assert!(got.is_none(), "{bytes_left} bytes left must not parse");
    }
}

// C#: BmffCr3MutationGapTests.TryReadBox_NegativeOffset_ReturnsFalseWithoutThrowing
#[test]
fn try_read_box_negative_offset_returns_none_without_error() {
    let mut source = src(cat(&[&ftyp(), &bx("free", &[])]));

    assert!(opt(&mut source, -1, None).is_none());
}

// C#: BmffCr3MutationGapTests.TryReadBox_LargesizeBoxWithPayloadFillingTheRestOfTheFile_ReadsSixteenByteHeader
#[test]
fn try_read_box_largesize_box_with_payload_filling_the_rest_of_the_file_reads_sixteen_byte_header() {
    let ftyp = ftyp();
    let large = cat(&[&u32be(1), b"free", &u64be(20), &[1u8, 2, 3, 4]]);
    let mut source = src(cat(&[&ftyp, &large]));

    let box_ = opt(&mut source, ftyp.len() as i64, None).expect("largesize box is read");

    assert_eq!(box_.total_size, 20);
    assert_eq!(box_.payload_offset, ftyp.len() as i64 + 16);
    assert_eq!(box_.payload_size, 4);
}

// C#: BmffCr3MutationGapTests.TryReadBox_LargesizeBoxThatIsExactlySixteenBytesAtEndOfFile_HasEmptyPayload
#[test]
fn try_read_box_largesize_box_exactly_sixteen_bytes_at_end_of_file_has_empty_payload() {
    let ftyp = ftyp();
    let mut source = src(cat(&[&ftyp, &u32be(1), b"free", &u64be(16)]));

    let box_ = opt(&mut source, ftyp.len() as i64, None).expect("16-byte largesize box is read");

    assert_eq!(box_.total_size, 16);
    assert_eq!(box_.payload_size, 0);
}

// C#: BmffCr3MutationGapTests.TryReadBox_LargesizeHeaderCutOffByEndOfFile_ReturnsFalseWithoutThrowing
#[test]
fn try_read_box_largesize_header_cut_off_by_end_of_file_returns_none_without_error() {
    // size==1 announces a 16-byte header but only 12 bytes exist.
    let ftyp = ftyp();
    let mut source = src(cat(&[&ftyp, &u32be(1), b"free", &[0u8; 4]]));

    assert!(opt(&mut source, ftyp.len() as i64, None).is_none());
}

// C#: BmffCr3MutationGapTests.TryReadBox_SizeZeroWithContainerEndZero_ReturnsFalse
#[test]
fn try_read_box_size_zero_with_container_end_zero_returns_none() {
    // containerEnd 0 is a real (empty) container end, not the "unspecified" default: the box would end before it starts.
    let mut source = src(cat(&[&u32be(0), b"free", &[0u8; 24]]));

    assert!(opt(&mut source, 0, Some(0)).is_none());
}

// C#: BmffCr3MutationGapTests.TryReadBox_SizeZeroExtendsToContainerEndOrEndOfFile
#[test]
fn try_read_box_size_zero_extends_to_container_end_or_end_of_file() {
    let mut source = src(cat(&[&u32be(0), b"free", &[0u8; 28]]));

    let in_container = opt(&mut source, 0, Some(20)).expect("size-0 box inside a container");
    assert_eq!(in_container.total_size, 20);
    let top_level = opt(&mut source, 0, None).expect("size-0 top-level box");
    assert_eq!(top_level.total_size, 36);
}

// C#: BmffChildBoxScopeTests.TryReadBox_TopLevelSizeZero_StillExtendsToEndOfFile
#[test]
fn try_read_box_top_level_size_zero_still_extends_to_end_of_file() {
    let data = cat(&[&u32be(0), b"mdat", &[0u8; 50]]);
    let len = data.len() as i64;
    let mut source = src(data);

    let box_ = opt(&mut source, 0, None).expect("top-level size-0 box parses");

    assert_eq!(box_.total_size, len);
}

// C#: Cr3BmffHardeningTests.TryReadBox_LargesizeThatOverflowsWhenAddedToOffset_ReturnsFalse
#[test]
fn try_read_box_largesize_that_overflows_when_added_to_offset_returns_none() {
    // Box at offset 20 declares largesize long.MaxValue - 8: offset + size would wrap to a negative number.
    let ftyp = ftyp();
    let big = i64::MAX - 8;
    let mut source = src(cat(&[&ftyp, &u32be(1), b"free", &u64be(big as u64), &[0u8; 16]]));

    assert!(opt(&mut source, 20, None).is_none());
}

// C#: Cr3BmffHardeningTests.TryReadBox_LargesizeAboveLongMax_ReturnsFalse
#[test]
fn try_read_box_largesize_above_long_max_returns_none() {
    let ftyp = ftyp();
    let mut source = src(cat(&[&ftyp, &u32be(1), b"free", &u64be(u64::MAX), &[0u8; 16]]));

    assert!(opt(&mut source, 20, None).is_none());
}

// C#: Cr3BmffHardeningTests.TryReadBox_UuidBoxTooSmallForExtendedType_ReturnsFalse (theory 8, 20, 23)
#[test]
fn try_read_box_uuid_box_too_small_for_extended_type_returns_none() {
    for size32 in [8u32, 20, 23] {
        let mut source = src(cat(&[&u32be(size32), b"uuid", &[0u8; 32]]));

        assert!(
            opt(&mut source, 0, None).is_none(),
            "uuid box of size {size32} must not parse"
        );
    }
}

// C#: Cr3BmffHardeningTests.TryReadBox_UuidBoxExactlyHeaderPlusExtendedType_HasEmptyPayload
#[test]
fn try_read_box_uuid_box_exactly_header_plus_extended_type_has_empty_payload() {
    let mut source = src(cat(&[&u32be(24), b"uuid", &[0u8; 16]]));

    let box_ = opt(&mut source, 0, None).expect("uuid box parses");

    assert_eq!(box_.payload_size, 0);
    assert_eq!(box_.payload_offset, 24);
}

// C#: RawContainerGapRound2Tests.TryReadBox_ShortHeaderRead_ReturnsFalse
#[test]
fn try_read_box_short_header_read_returns_none() {
    let file = cat(&[&bx("free", &[0u8; 8]), &bx("free", &[0u8; 8])]);
    let mut source = KnobSource::new(file).short_at(0);

    assert!(
        try_read_box(&mut source, 0, None)
            .expect("short read is malformed, not an error")
            .is_none()
    );
}

// C#: RawContainerGapRound2Tests.TryReadBox_ShortLargeSizeRead_ReturnsFalse
#[test]
fn try_read_box_short_large_size_read_returns_none() {
    let file = cat(&[&u32be(1), b"free", &u64be(24), &[0u8; 8]]);
    let mut source = KnobSource::new(file).short_at(8);

    assert!(
        try_read_box(&mut source, 0, None)
            .expect("short read is malformed, not an error")
            .is_none()
    );
}

// C#: RawContainerGapRound2Tests.TryReadBox_ShortUuidRead_ReturnsFalse
#[test]
fn try_read_box_short_uuid_read_returns_none() {
    let file = bx("uuid", &cat(&[&CANON_UUID, &[0u8; 8]]));
    let mut source = KnobSource::new(file).short_at(8);

    assert!(
        try_read_box(&mut source, 0, None)
            .expect("short read is malformed, not an error")
            .is_none()
    );
}

// No C# twin: pins the C# rule "`containerEnd >= 0`" (Traps 8): a negative container end is the unspecified default.
#[test]
fn try_read_box_negative_container_end_behaves_as_unspecified() {
    let file = cat(&[&u32be(0), b"free", &[0u8; 28]]);
    let mut source = src(file);

    let unspecified = opt(&mut source, 0, None).expect("size-0 box parses");
    let negative = opt(&mut source, 0, Some(-5)).expect("size-0 box parses");

    assert_eq!(negative, unspecified);
    assert_eq!(negative.total_size, 36);
}

// No C# twin: pins `Encoding.ASCII.GetString` for the type bytes (every byte above 0x7F becomes '?').
#[test]
fn try_read_box_non_ascii_type_bytes_map_to_question_marks() {
    let mut source = src(cat(&[&u32be(8), &[0xC3, 0xA9, b'x', b'y']]));

    let box_ = opt(&mut source, 0, None).expect("box with a non-ASCII type parses");

    assert_eq!(box_.box_type, "??xy");
}

// No C# twin: a `uuid` box found through a 64-bit largesize carries its extended type (C# `uuid` with largesize header).
#[test]
fn try_read_box_uuid_reached_through_largesize_carries_its_uuid() {
    let uuid: [u8; 16] = core::array::from_fn(|i| 0x11 + i as u8);
    let mut source = src(cat(&[&u32be(1), b"uuid", &u64be(32), &uuid]));

    let box_ = opt(&mut source, 0, None).expect("largesize uuid box parses");

    assert_eq!(box_.uuid, Some(uuid));
    assert_eq!(box_.payload_offset, 32);
    assert_eq!(box_.payload_size, 0);
}

// No C# twin: pins INV-8 (PARITY): header reads only, at most three per header (8, 8, 16 bytes), never payload bytes.
#[test]
fn try_read_box_issues_only_header_reads() {
    let file = bx("uuid", &cat(&[&CANON_UUID, &[0x33u8; 1000]]));
    let mut source = RecordingSource {
        inner: src(file),
        reads: Vec::new(),
    };

    let box_ = try_read_box(&mut source, 0, None)
        .expect("healthy source")
        .expect("uuid box parses");

    assert_eq!(box_.uuid, Some(CANON_UUID));
    assert!(
        source.reads.len() <= 3,
        "at most three reads per header, got {:?}",
        source.reads
    );
    assert!(
        source
            .reads
            .iter()
            .all(|&(offset, count)| (count == 8 || count == 16) && offset + i64::from(count) <= 24),
        "reads must stay inside the 24-byte uuid header: {:?}",
        source.reads
    );
}

// ------------------------------------------------------------------ BmffBoxNavigator.ReadChildBoxes

// C#: BmffChildBoxScopeTests.ReadChildBoxes_SizeZeroChild_ExtendsToTheEndOfItsParentNotTheFile
#[test]
fn read_child_boxes_size_zero_child_extends_to_the_end_of_its_parent_not_the_file() {
    let first = bx("free", &[0u8; 8]);
    let size_zero = cat(&[&u32be(0), b"last", &[0u8; 20]]);
    let parent = bx("moov", &cat(&[&first, &size_zero]));
    let trailer = bx("free", &[0u8; 100]); // more file after the parent: EOF is NOT the parent end
    let mut source = src(cat(&[&parent, &trailer]));
    let moov = parsed(&mut source, 0);

    let kids = children(&mut source, &moov, 0);

    assert_eq!(kids.len(), 2);
    assert_eq!(kids[1].box_type, "last");
    assert_eq!(kids[1].offset + kids[1].total_size, parent.len() as i64);
}

// C#: BmffCr3MutationGapTests.ReadChildBoxes_ParentPayloadStartingAtFileStart_ReturnsItsChildren
#[test]
fn read_child_boxes_parent_payload_starting_at_file_start_returns_its_children() {
    let file = cat(&[&bx("aaaa", &[1]), &bx("bbbb", &[2, 3])]);
    let mut source = src(file.clone());
    let parent = BmffBox::new("root", 0, file.len() as i64, 0, file.len() as i64);

    let kids = children(&mut source, &parent, 0);

    let types: Vec<&str> = kids.iter().map(|k| k.box_type.as_str()).collect();
    let offsets: Vec<i64> = kids.iter().map(|k| k.offset).collect();
    assert_eq!(types, ["aaaa", "bbbb"]);
    assert_eq!(offsets, [0, 9]);
}

// C#: BmffCr3MutationGapTests.ReadChildBoxes_NegativePayloadSkip_ReturnsEmpty
#[test]
fn read_child_boxes_negative_payload_skip_returns_empty() {
    let mut source = src(bx("moov", &bx("free", &[])));
    let moov = parsed(&mut source, 0);

    assert!(children(&mut source, &moov, -8).is_empty());
}

// C#: BmffCr3MutationGapTests.ReadChildBoxes_EightByteChildFillingExactlyTheLastEightPayloadBytes_IsIncluded
#[test]
fn read_child_boxes_eight_byte_child_filling_exactly_the_last_eight_payload_bytes_is_included() {
    let mut source = src(bx("moov", &bx("free", &[])));
    let moov = parsed(&mut source, 0);

    let kids = children(&mut source, &moov, 0);

    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0].box_type, "free");
    assert_eq!(kids[0].offset, 8);
}

// C#: BmffCr3MutationGapTests.ReadChildBoxes_PayloadSkipLeavingExactlyOneEmptyBox_ReturnsIt
#[test]
fn read_child_boxes_payload_skip_leaving_exactly_one_empty_box_returns_it() {
    let mut source = src(bx("uuid", &cat(&[&[0u8; 16], &[0u8; 8], &bx("free", &[])])));
    let uuid = parsed(&mut source, 0);

    let kids = children(&mut source, &uuid, 8);

    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0].box_type, "free");
}

// C#: Cr3BmffHardeningTests.ReadChildBoxes_ParentPayloadSizeOverflowingLimit_IsClampedToSource
#[test]
fn read_child_boxes_parent_payload_size_overflowing_limit_is_clamped_to_source() {
    // Parent payload starts at offset 8, so PayloadOffset + long.MaxValue wraps negative if not clamped.
    let file = cat(&[&bx("skip", &[]), &bx("free", &[0u8; 8]), &bx("skip", &[0u8; 8])]);
    let mut source = src(file);
    let hostile_parent = BmffBox::new("moov", 0, i64::MAX, 8, i64::MAX);

    let kids = children(&mut source, &hostile_parent, 0);

    let types: Vec<&str> = kids.iter().map(|k| k.box_type.as_str()).collect();
    assert_eq!(types, ["free", "skip"]);
}

// C#: Cr3BmffHardeningTests.ReadChildBoxes_NegativePayloadSize_ReturnsEmptyWithoutThrowing
#[test]
fn read_child_boxes_negative_payload_size_returns_empty_without_error() {
    let mut source = src(bx("free", &[0u8; 8]));
    let hostile_parent = BmffBox::new("moov", 0, 0, 0, -4);

    assert!(children(&mut source, &hostile_parent, 0).is_empty());
}

// C#: Cr3BmffHardeningTests.ReadChildBoxes_ChildLargerThanParentPayload_StopsWithoutThrowing
#[test]
fn read_child_boxes_child_larger_than_parent_payload_stops_without_error() {
    // Child claims 0x7FFFFFF0 bytes but the file (and parent) are tiny.
    let inner = cat(&[&u32be(0x7FFF_FFF0), b"free", &[0u8; 8]]);
    let mut source = src(bx("moov", &inner));
    let moov = parsed(&mut source, 0);

    assert!(children(&mut source, &moov, 0).is_empty());
}

// C#: Cr3BmffHardeningTests.ReadChildBoxes_MoreThanMaxChildBoxes_ReturnsExactlyTheCapWithoutThrowing
#[test]
fn read_child_boxes_more_than_max_child_boxes_returns_exactly_the_cap_without_error() {
    let kids: Vec<Vec<u8>> = (0..MAX_CHILD_BOXES + 44).map(|_| bx("free", &[])).collect();
    let refs: Vec<&[u8]> = kids.iter().map(Vec::as_slice).collect();
    let mut source = src(bx("moov", &cat(&refs)));
    let moov = parsed(&mut source, 0);

    assert_eq!(children(&mut source, &moov, 0).len(), MAX_CHILD_BOXES);
}

// C#: Cr3BmffHardeningTests.ReadChildBoxes_PayloadSkipBeyondPayload_ReturnsEmpty
#[test]
fn read_child_boxes_payload_skip_beyond_payload_returns_empty() {
    let mut source = src(bx("moov", &bx("free", &[])));
    let moov = parsed(&mut source, 0);

    assert!(children(&mut source, &moov, i32::MAX).is_empty());
}

// No C# twin: pins `MaxChildBoxes == 256` (C# `BmffBoxNavigator.MaxChildBoxes`) and the cap boundary, 255 / 256 / 257 children.
#[test]
fn read_child_boxes_cap_boundary_255_256_257_children() {
    assert_eq!(MAX_CHILD_BOXES, 256);
    for (count, expected) in [(255usize, 255usize), (256, 256), (257, 256)] {
        let kids: Vec<Vec<u8>> = (0..count).map(|_| bx("free", &[])).collect();
        let refs: Vec<&[u8]> = kids.iter().map(Vec::as_slice).collect();
        let mut source = src(bx("moov", &cat(&refs)));
        let moov = parsed(&mut source, 0);

        assert_eq!(children(&mut source, &moov, 0).len(), expected, "{count} children");
    }
}

// No C# twin: a 4 GiB-claiming parent is clamped to the source and its children are pushed, never pre-allocated
// (`Vec::new()` then push, never `with_capacity(payload_size)`).
#[test]
fn read_child_boxes_claimed_4_gib_payload_does_not_preallocate() {
    let kids: Vec<Vec<u8>> = (0..300).map(|_| bx("free", &[])).collect();
    let refs: Vec<&[u8]> = kids.iter().map(Vec::as_slice).collect();
    let mut source = src(bx("moov", &cat(&refs)));
    let claimed = i64::from(u32::MAX);
    let hostile_parent = BmffBox::new("moov", 0, claimed, 8, claimed);

    let got = children(&mut source, &hostile_parent, 0);

    assert_eq!(got.len(), MAX_CHILD_BOXES);
    assert!(
        got.capacity() <= 1024,
        "capacity {} suggests a pre-allocation",
        got.capacity()
    );
}

// No C# twin: `BmffBox::new` is the C# constructor with its `uuid = null` default.
#[test]
fn bmff_box_new_leaves_uuid_none() {
    let box_ = BmffBox::new("moov", 0, 8, 8, 0);

    assert_eq!(box_.box_type, "moov");
    assert_eq!(box_.uuid, None);
}

// No C# twin: source errors propagate unchanged from both functions; `is_io_failure()` tells the two kinds apart
// (W5-10b relies on it to keep disk errors visible).
#[test]
fn source_errors_propagate_and_io_failures_are_distinguishable() {
    let file = bx("moov", &bx("free", &[]));

    let mut data_failure = KnobSource::new(file.clone()).failing(Failure::Data, 0);
    let err = try_read_box(&mut data_failure, 0, None).expect_err("a failing source is an error, not a box");
    assert!(!err.is_io_failure(), "invalid data must not look like an I/O failure");

    let mut io_failure = KnobSource::new(file.clone()).failing(Failure::Io, 0);
    let err = try_read_box(&mut io_failure, 0, None).expect_err("a failing source is an error, not a box");
    assert!(
        err.is_io_failure(),
        "a disk failure must stay visible as an I/O failure"
    );

    let moov = BmffBox::new("moov", 0, file.len() as i64, 8, file.len() as i64 - 8);
    let mut child_failure = KnobSource::new(file.clone()).failing(Failure::Io, 0);
    let err = read_child_boxes(&mut child_failure, &moov, 0).expect_err("child read failure propagates");
    assert!(err.is_io_failure());
    let mut child_data_failure = KnobSource::new(file).failing(Failure::Data, 0);
    let err = read_child_boxes(&mut child_data_failure, &moov, 0).expect_err("child read failure propagates");
    assert!(!err.is_io_failure());
}

// ------------------------------------------------------------------ golden and fuzz

fn golden_inputs() -> Value {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("bmff-navigator.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn hex_bytes(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex digit pair"))
        .collect()
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn case_bytes(input: &Value) -> Vec<u8> {
    hex_bytes(input["data"]["hex"].as_str().expect("case data hex"))
}

/// Golden knobs: `fail` ("data" | "io") from read `fail_after` on, and `short_at`.
fn knob_source(data: Vec<u8>, input: &Value) -> KnobSource {
    let mut source = KnobSource::new(data);
    if let Some(kind) = input.get("fail").and_then(Value::as_str) {
        let after = usize::try_from(input.get("fail_after").and_then(Value::as_u64).unwrap_or(0))
            .expect("fail_after fits usize");
        let kind = match kind {
            "data" => Failure::Data,
            "io" => Failure::Io,
            other => panic!("unknown fail kind {other}"),
        };
        source = source.failing(kind, after);
    }
    if let Some(at) = input.get("short_at").and_then(Value::as_i64) {
        source = source.short_at(at);
    }
    source
}

fn error_kind(err: &RawHeaderError) -> &'static str {
    if err.is_io_failure() { "io" } else { "data" }
}

/// Golden box object: `type`, `offset`, `total_size`, `payload_offset`, `payload_size`, `uuid` (hex or null).
fn box_value(box_: &BmffBox) -> Value {
    json!({
        "type": box_.box_type,
        "offset": box_.offset,
        "total_size": box_.total_size,
        "payload_offset": box_.payload_offset,
        "payload_size": box_.payload_size,
        "uuid": box_.uuid.map(|u| hex_string(&u)),
    })
}

/// Golden inputs write values beyond 2^53 as decimal strings.
fn json_i64(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn parent_from_json(parent: &Value) -> BmffBox {
    BmffBox {
        box_type: parent["type"].as_str().expect("parent type").to_owned(),
        offset: json_i64(&parent["offset"]).expect("parent offset"),
        total_size: json_i64(&parent["total_size"]).expect("parent total_size"),
        payload_offset: json_i64(&parent["payload_offset"]).expect("parent payload_offset"),
        payload_size: json_i64(&parent["payload_size"]).expect("parent payload_size"),
        uuid: parent
            .get("uuid")
            .and_then(Value::as_str)
            .map(|h| hex_bytes(h).try_into().expect("uuid is 16 bytes")),
    }
}

fn check_try_read_box(name: &str, case: &Value, input: &Value) {
    let mut source = knob_source(case_bytes(input), input);
    let offset = input["offset"].as_i64().expect("offset");
    let container_end = input.get("container_end").and_then(Value::as_i64);

    let got = try_read_box(&mut source, offset, container_end);
    match case["error"].as_str() {
        Some(kind) => {
            let err = got.expect_err(name);
            assert_eq!(error_kind(&err), kind, "{name}");
        }
        None => {
            let got = got.unwrap_or_else(|e| panic!("{name}: unexpected source error {e}"));
            assert_eq!(got.is_some(), case["ok"].as_bool().expect("ok"), "{name}");
            match got {
                Some(box_) => assert_eq!(box_value(&box_), case["box"], "{name}"),
                None => assert!(case["box"].is_null(), "{name}"),
            }
        }
    }
}

fn check_child_boxes(name: &str, case: &Value, input: &Value) {
    let data = case_bytes(input);
    let parent = match input.get("parent_at").and_then(Value::as_i64) {
        Some(at) => {
            let mut plain = src(data.clone());
            opt(&mut plain, at, None).unwrap_or_else(|| panic!("{name}: parent_at does not parse"))
        }
        None => parent_from_json(&input["parent"]),
    };
    let skip =
        i32::try_from(input.get("payload_skip").and_then(Value::as_i64).unwrap_or(0)).expect("payload_skip fits i32");
    let mut source = knob_source(data, input);

    let got = read_child_boxes(&mut source, &parent, skip);
    match case["error"].as_str() {
        Some(kind) => {
            let err = got.expect_err(name);
            assert_eq!(error_kind(&err), kind, "{name}");
        }
        None => {
            let kids = got.unwrap_or_else(|e| panic!("{name}: unexpected source error {e}"));
            assert_eq!(json!(kids.len()), case["count"], "{name}");
            let values: Vec<Value> = kids.iter().map(box_value).collect();
            assert_eq!(Value::Array(values), case["children"], "{name}");
        }
    }
}

/// C# `WalkBoxes` without the recursion (the golden `walk` op): top-level boxes capped at 256 iterations; each
/// `moov/trak/mdia/minf/stbl/uuid/meta` box gets one `read_child_boxes(.., 0)` whose children are listed at depth 1, never walked.
struct Walk {
    any: bool,
    visited: usize,
    boxes: Vec<(usize, BmffBox)>,
}

fn walk(source: &mut dyn RawHeaderSource) -> Result<Walk, RawHeaderError> {
    let end = source.length();
    let mut offset = 0i64;
    let mut out = Walk {
        any: false,
        visited: 0,
        boxes: Vec::new(),
    };
    for _ in 0..MAX_CHILD_BOXES {
        if offset >= end {
            break;
        }
        let Some(box_) = try_read_box(source, offset, Some(end))? else {
            break;
        };
        // A parsed box must stay inside the container and make forward progress.
        assert_eq!(box_.offset, offset, "box offset equals the requested one");
        assert!(
            box_.total_size >= 8 && box_.offset + box_.total_size <= end,
            "box {} escapes",
            box_.box_type
        );
        out.any = true;
        out.visited += 1;
        out.boxes.push((0, box_.clone()));
        if out.visited > WALK_VISIT_LIMIT {
            break;
        }
        if matches!(
            box_.box_type.as_str(),
            "moov" | "trak" | "mdia" | "minf" | "stbl" | "uuid" | "meta"
        ) {
            for child in read_child_boxes(source, &box_, 0)? {
                assert!(
                    child.offset >= box_.payload_offset
                        && child.offset + child.total_size <= box_.offset + box_.total_size,
                    "child {} escapes its parent {}",
                    child.box_type,
                    box_.box_type
                );
                out.visited += 1;
                out.boxes.push((1, child));
            }
        }
        offset = box_.offset + box_.total_size;
    }
    Ok(out)
}

fn check_walk(name: &str, case: &Value, input: &Value) {
    assert!(case["error"].is_null(), "{name}: walk has no failing knob");
    let mut source = src(case_bytes(input));

    let walked = walk(&mut source).unwrap_or_else(|e| panic!("{name}: unexpected source error {e}"));

    assert_eq!(json!(walked.any), case["any"], "{name}");
    assert_eq!(json!(walked.visited), case["visited"], "{name}");
    let boxes: Vec<Value> = walked
        .boxes
        .iter()
        .map(|(depth, box_)| {
            let mut value = box_value(box_);
            value["depth"] = json!(depth);
            value
        })
        .collect();
    assert_eq!(Value::Array(boxes), case["boxes"], "{name}");
}

// C#: BinaryReaderFuzzTests (golden for the oracle `bmff-navigator` command; 127 cases)
#[test]
fn golden_bmff_navigator_cases() {
    let golden_doc = golden::load("bmff-navigator");
    let inputs = golden_inputs();
    let input_cases = inputs["cases"].as_array().expect("input cases");
    assert_eq!(
        golden_doc.cases.len(),
        127,
        "golden bmff-navigator must carry 127 cases"
    );
    assert_eq!(input_cases.len(), golden_doc.cases.len());

    for (case, input) in golden_doc.cases.iter().zip(input_cases) {
        let name = golden::str_of(case, "name");
        assert_eq!(name, golden::str_of(input, "name"), "golden and input order differ");
        match golden::str_of(case, "op") {
            "try-read-box" => check_try_read_box(name, case, input),
            "child-boxes" => check_child_boxes(name, case, input),
            "walk" => check_walk(name, case, input),
            other => panic!("{name}: unknown op {other}"),
        }
    }
}

/// Deterministic xorshift64 (no `rand` crate); the seed is never zero.
struct XorShift(u64);

impl XorShift {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform-ish value in `0..n`; `n` must be non-zero.
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next_u64() % u64::try_from(n).expect("n fits u64")).expect("index fits usize")
    }
}

/// Port of the C# mutant families: flip 1-4 random bytes, truncate, append random bytes, or overwrite a 4-byte window with
/// `00 00 00 00`, `FF FF FF FF`, `00 00 00 01` or `7F FF FF F0` (the length-field values of `BinaryFuzz`).
fn mutant(seed: &[u8], rng: &mut XorShift) -> Vec<u8> {
    const PATTERNS: [[u8; 4]; 4] = [
        [0, 0, 0, 0],
        [0xFF, 0xFF, 0xFF, 0xFF],
        [0, 0, 0, 1],
        [0x7F, 0xFF, 0xFF, 0xF0],
    ];
    let mut out = seed.to_vec();
    match rng.below(4) {
        0 => {
            for _ in 0..=rng.below(4) {
                if !out.is_empty() {
                    let at = rng.below(out.len());
                    out[at] = rng.next_u64() as u8;
                }
            }
        }
        1 => {
            let keep = rng.below(out.len() + 1);
            out.truncate(keep);
        }
        2 => {
            for _ in 0..=rng.below(32) {
                out.push(rng.next_u64() as u8);
            }
        }
        _ => {
            if out.len() >= 4 {
                let at = rng.below(out.len() - 3);
                let pattern = PATTERNS[rng.below(PATTERNS.len())];
                out[at..at + 4].copy_from_slice(&pattern);
            }
        }
    }
    out
}

/// The walk golden seed, read from the oracle inputs so the fuzz starts from the exact bytes the C# harness used.
fn walk_seed(name: &str) -> Vec<u8> {
    let inputs = golden_inputs();
    let case = inputs["cases"]
        .as_array()
        .expect("input cases")
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("golden input {name} missing"));
    case_bytes(case)
}

// C#: BinaryReaderFuzzTests.Fuzz_BmffBoxNavigator_FailsOnlyCleanly (CR3 and plain ISO-BMFF seeds, 4300 mutants each)
#[test]
fn fuzz_bmff_box_navigator_fails_only_cleanly() {
    let seeds = [walk_seed("walk-synthetic-cr3"), walk_seed("walk-plain-iso-bmff-meta")];
    for (index, seed) in seeds.iter().enumerate() {
        let mut rng = XorShift(4300 + index as u64);
        for mutant_index in 0..4300 {
            let bytes = mutant(seed, &mut rng);
            let mut source = src(bytes);

            let walked = walk(&mut source)
                .unwrap_or_else(|e| panic!("seed {index} mutant {mutant_index}: walk must not fail on memory: {e}"));

            assert_eq!(walked.visited, walked.boxes.len());
            assert!(
                walked.visited <= WALK_VISIT_LIMIT,
                "walk visited {} boxes",
                walked.visited
            );
            let mut parent: Option<&BmffBox> = None;
            for (depth, box_) in &walked.boxes {
                if *depth == 0 {
                    parent = Some(box_);
                } else {
                    let owner = parent.expect("a child always follows its parent");
                    assert!(
                        box_.offset >= owner.payload_offset
                            && box_.offset + box_.total_size <= owner.offset + owner.total_size,
                        "seed {index} mutant {mutant_index}: child {} escapes {}",
                        box_.box_type,
                        owner.box_type
                    );
                }
            }
        }
    }
}
