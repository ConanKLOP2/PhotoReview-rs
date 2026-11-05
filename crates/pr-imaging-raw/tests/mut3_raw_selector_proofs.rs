//! MUT3 / P1 (2026-11-06): proofs and killers for the survivors that N4 left "run-supported, open":
//! `preview_selector.rs` 376:22 and `readers/arw.rs` 124:32, 172:21, 243:15, `readers/cr2.rs` 145:32.
//! Inline bytes only: no golden file, no corpus, no sleep, no production change. Evidence: `docs/reviews/P1-MUT-2026-11-06.md`.
//!
//! Rule B (strict): a mutant is closed only by a run over the WHOLE domain in which mutant and original differ, or over
//! every input class. Where the divergence is the value of one comparison, the test says which values form the divergent set
//! and runs all of them.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use pr_core::sync::CancelToken;
use pr_imaging::decode_types::DecodeBox;
use pr_imaging_raw::preview_selector::{is_adobe_rgb_exif, select_preview};
use pr_imaging_raw::readers::arw::ArwContainerReader;
use pr_imaging_raw::readers::cr2::Cr2ContainerReader;
use pr_imaging_raw::tiff_nav::{read_ifd_entries, try_read_exif_pixel_dimensions};
use pr_imaging_raw::{
    EmbeddedPreview, EmbeddedPreviewKind, InMemoryRawHeaderSource, PreviewColorSpace, RawContainerInfo,
    RawContainerReader, RawHeaderError, RawHeaderSource,
};
use pr_test_support::raw::{TiffBytes, at, long, short};

// ============================================================================ preview_selector.rs 376:22

const PREFIX: &[u8; 6] = b"Exif\0\0";

/// Layout of a 14-byte payload: 6 prefix bytes, `b0 b1` (byte order), `m0 m1` (magic), 4 IFD0 pointer bytes.
fn payload14(prefix: &[u8; 6], order: [u8; 2], magic: [u8; 2], ifd0: [u8; 4]) -> [u8; 14] {
    let mut p = [0u8; 14];
    p[..6].copy_from_slice(prefix);
    p[6..8].copy_from_slice(&order);
    p[8..10].copy_from_slice(&magic);
    p[10..].copy_from_slice(&ifd0);
    p
}

/// `payload.len() < 14` -> `<= 14` (376:22). The two differ ONLY for payloads of exactly 14 bytes. The original answers `false`
/// for all of them, in three classes that together are every 14-byte payload:
/// P1 the prefix is not `Exif\0\0` (second operand of the `||`), P2 the prefix matches and the TIFF header is invalid, P3 the
/// prefix matches and the header is `II*\0` / `MM\0*` (only then does the IFD0 pointer matter).
/// Fast part: P1 by every single-byte deviation of the prefix (6 x 255) and the extreme prefixes; P2 and P3 by edges.
#[test]
fn a_fourteen_byte_payload_is_never_adobe_rgb_fast_classes() {
    for position in 0..6 {
        for byte in 0..=u8::MAX {
            if byte == PREFIX[position] {
                continue;
            }
            let mut prefix = *PREFIX;
            prefix[position] = byte;
            let p = payload14(&prefix, *b"II", [42, 0], 8u32.to_le_bytes());
            assert!(!is_adobe_rgb_exif(&p), "prefix deviation at {position}: {byte:#04x}");
        }
    }
    for prefix in [[0u8; 6], [0xFFu8; 6]] {
        assert!(!is_adobe_rgb_exif(&payload14(
            &prefix,
            *b"II",
            [42, 0],
            8u32.to_le_bytes()
        )));
    }
    for ifd0 in (0..=70_000u32).chain([1 << 20, 1 << 24, 1 << 31, u32::MAX - 1, u32::MAX]) {
        assert!(!is_adobe_rgb_exif(&payload14(
            PREFIX,
            *b"II",
            [42, 0],
            ifd0.to_le_bytes()
        )));
        assert!(!is_adobe_rgb_exif(&payload14(
            PREFIX,
            *b"MM",
            [0, 42],
            ifd0.to_be_bytes()
        )));
    }
}

/// The same two classes P2 and P3 over their WHOLE domains (the prefix class P1 is decided by the first operand of the `||` and
/// is covered above): P3 = 2 byte orders x all 2^32 IFD0 pointers, P2 = every `(b0, b1, m0, m1)` of 2^32 header words except the
/// two valid ones (IFD0 pointer 8). Spread over worker threads; about 13e9 calls of the real function. Run explicitly:
/// `cargo test -p pr-imaging-raw --test mut3_raw_selector_proofs -- --include-ignored`.
#[test]
#[ignore = "slow: exhaustive 2^33 + 2^32 calls of is_adobe_rgb_exif (about 10 min in a debug build on a loaded machine); run it with --include-ignored"]
fn every_fourteen_byte_payload_with_the_exif_prefix_is_not_adobe_rgb() {
    let workers: u32 =
        std::thread::available_parallelism().map_or(4, |n| u32::try_from(n.get()).unwrap_or(4).clamp(1, 8));
    let slice = u32::MAX / workers + 1;
    std::thread::scope(|scope| {
        for w in 0..workers {
            scope.spawn(move || {
                let lo = w.saturating_mul(slice);
                let hi = lo.saturating_add(slice - 1);
                // P3: both byte orders, every IFD0 pointer of this slice.
                let mut p = payload14(PREFIX, *b"II", [42, 0], [0; 4]);
                let mut q = payload14(PREFIX, *b"MM", [0, 42], [0; 4]);
                let mut value = lo;
                loop {
                    p[10..].copy_from_slice(&value.to_le_bytes());
                    q[10..].copy_from_slice(&value.to_be_bytes());
                    assert!(!is_adobe_rgb_exif(&p), "II ifd0 {value}");
                    assert!(!is_adobe_rgb_exif(&q), "MM ifd0 {value}");
                    if value == hi {
                        break;
                    }
                    value += 1;
                }
                // P2: this slice of the 2^32 header words (order bytes, magic bytes), IFD0 pointer 8.
                let mut p = payload14(PREFIX, [0; 2], [0; 2], 8u32.to_le_bytes());
                let mut word = lo;
                loop {
                    let b = word.to_le_bytes();
                    let valid = (b[..2] == *b"II" && b[2..] == [42, 0]) || (b[..2] == *b"MM" && b[2..] == [0, 42]);
                    if !valid {
                        p[6..10].copy_from_slice(&b);
                        assert!(!is_adobe_rgb_exif(&p), "header word {word:#010x}");
                    }
                    if word == hi {
                        break;
                    }
                    word += 1;
                }
            });
        }
    });
}

// ============================================================================ sources used below

/// Source whose `read` must never be called: any read panics, so a passing run proves the result does not depend on content.
/// `length` is a plain number the test sweeps.
struct NoReadSource {
    length: i64,
}

impl RawHeaderSource for NoReadSource {
    fn length(&self) -> i64 {
        self.length
    }
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        panic!("unexpected read({offset}, {count}) of a source that must not be read");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Length,
    Read(i64, i32),
}

/// Forwards to an in-memory source and logs every call (length queries and reads) in order.
struct Tripwire {
    inner: InMemoryRawHeaderSource,
    log: Rc<RefCell<Vec<Call>>>,
}

impl Tripwire {
    fn new(data: Vec<u8>) -> (Self, Rc<RefCell<Vec<Call>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        (
            Self {
                inner: InMemoryRawHeaderSource::new(data),
                log: Rc::clone(&log),
            },
            log,
        )
    }
}

impl RawHeaderSource for Tripwire {
    fn length(&self) -> i64 {
        self.log.borrow_mut().push(Call::Length);
        self.inner.length()
    }
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.log.borrow_mut().push(Call::Read(offset, count));
        self.inner.read(offset, count)
    }
}

fn run_arw(data: Vec<u8>) -> (RawContainerInfo, Vec<Call>) {
    let (mut source, log) = Tripwire::new(data);
    let info = ArwContainerReader
        .read(&mut source, &CancelToken::new())
        .expect("synthetic ARW parses");
    let calls = log.borrow().clone();
    (info, calls)
}

// ============================================================================ arw.rs 124:32

/// `exif_offset > 0` -> `>= 0` (arw.rs 124:32, same shape at cr2.rs 145:32 for the Exif pixel size part). Mutant and original
/// differ ONLY for `exif_offset == 0`; the mutant then runs `read_ifd_entries(source, 0, ..)` and
/// `try_read_exif_pixel_dimensions(source, 0, ..)`, and loops over their (empty) results. The two functions are run here with a
/// source whose `read` panics and whose `length` sweeps the i64 edge classes: they give "no entries, next 0" and `None` without
/// reading anything, so for EVERY source (the only channel through which content could matter is `read`) the mutant block is a
/// no-op. `length` is consulted by no branch at offset 0 (`ifd_offset < 8` short-circuits before `len - 2`).
#[test]
fn ifd_reads_at_offset_zero_touch_no_source_content_whatever_the_length() {
    let lengths = [
        i64::MIN,
        i64::MIN + 1,
        -1,
        0,
        1,
        2,
        7,
        8,
        9,
        15,
        16,
        17,
        1000,
        i64::from(u32::MAX),
        i64::MAX - 1,
        i64::MAX,
    ];
    for length in lengths {
        for little in [true, false] {
            let mut source = NoReadSource { length };
            let read = read_ifd_entries(&mut source, 0, little).expect("offset 0 reads nothing");
            assert!(read.entries.is_empty(), "length {length}");
            assert_eq!(read.next_ifd_offset, 0);
            assert_eq!(
                try_read_exif_pixel_dimensions(&mut source, 0, little).expect("offset 0 reads nothing"),
                None,
                "length {length}"
            );
        }
    }
}

// ============================================================================ arw.rs 172:21

/// A synthetic ARW: IFD0 at 100 holds the raw size and an Exif pointer to 200; the Exif IFD holds one MakerNote entry
/// (`count`, offset `note`); a Sony IFD with a 0x2001 PreviewImage entry sits at 8 and a JPEG at 300.
fn arw_with_maker_note(little: bool, count: u32, note: u32) -> Vec<u8> {
    let mut jpeg = vec![0xFF, 0xD8];
    jpeg.resize(100, 0);
    TiffBytes::new(little, 600)
        .header(100)
        .ifd(100, 0, &[short(0x0100, 50), short(0x0101, 40), long(0x8769, 200)])
        .ifd(200, 0, &[at(0x927C, 7, count, note)])
        .ifd(8, 0, &[at(0x2001, 7, 100, 300)])
        .put(300, &jpeg)
        .into_vec()
}

/// `note > 0` -> `>= 0` (arw.rs 172:21). The mutant differs only for a latched maker note offset 0, where it calls
/// `parse_sony_maker_note_preview(source, 0, ..)`. A function that can only observe the source through `length()` and `read()`
/// is independent of the source when it makes no such call. The call log of the file with a latched note offset 0 (count 10) is
/// compared with the log of the same file whose MakerNote entry is NOT latched (count 4, the filter needs `count > 4`): they
/// are equal, so the call at offset 0 made no source call, listed no preview, and changed nothing. The control (note offset 8,
/// the Sony IFD) shows the tripwire does see the call of the parse: more calls, one more preview.
#[test]
fn a_latched_maker_note_offset_zero_makes_no_source_call_and_lists_nothing() {
    for little in [true, false] {
        let (latched_zero, calls_zero) = run_arw(arw_with_maker_note(little, 10, 0));
        let (not_latched, calls_not_latched) = run_arw(arw_with_maker_note(little, 4, 0));
        assert_eq!(calls_zero, calls_not_latched, "little {little}");
        assert_eq!(latched_zero.previews, not_latched.previews);
        assert!(latched_zero.previews.is_empty());
        assert_eq!(latched_zero.exif_blocks, not_latched.exif_blocks);
        assert_eq!(
            (latched_zero.sensor_width, latched_zero.sensor_height),
            (not_latched.sensor_width, not_latched.sensor_height)
        );

        // Control: a note offset the parse accepts (8: the Sony IFD) is visible in the log and lists the preview.
        let (listed, calls_listed) = run_arw(arw_with_maker_note(little, 10, 8));
        assert!(
            calls_listed.len() > calls_zero.len(),
            "the tripwire must see a real parse"
        );
        assert_eq!(listed.previews.len(), 1);
    }
}

// ============================================================================ arw.rs 243:15

/// `length < 4` -> `== 4` / `<= 4` (arw.rs 243:15). `length` is the entry `count`, and line 238 has already returned for
/// `count <= 4`. The mutated and the original condition differ only for `length <= 4` (`<= 4` at 4; `== 4` at 0..=4) and every
/// such count is refused two statements earlier; for `count >= 5` all three conditions are false. Run: every count in the whole
/// divergent set 0..=4, both byte orders, with a valid JPEG waiting at the entry's offset. Nothing is listed AND the log has no
/// read at the JPEG offset (the SOI read only comes after line 243, so the mutated line is never reached). Control: count 5 and
/// 100 do reach it.
#[test]
fn sony_preview_counts_zero_to_four_never_reach_the_length_check() {
    let build = |little: bool, count: u32| {
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.resize(100, 0);
        TiffBytes::new(little, 600)
            .header(100)
            .ifd(100, 0, &[long(0x8769, 200)])
            .ifd(200, 0, &[at(0x927C, 7, 100, 8)])
            .ifd(8, 0, &[at(0x2001, 7, count, 300)])
            .put(300, &jpeg)
            .into_vec()
    };
    for little in [true, false] {
        for count in 0..=4u32 {
            let (info, calls) = run_arw(build(little, count));
            assert!(info.previews.is_empty(), "count {count} little {little}");
            assert!(
                !calls.iter().any(|c| matches!(c, Call::Read(300, _))),
                "count {count} little {little}: the SOI at the preview offset must not be read"
            );
        }
        for count in [5u32, 100] {
            let (info, calls) = run_arw(build(little, count));
            assert_eq!(info.previews.len(), 1, "count {count} little {little}");
            assert!(
                calls.contains(&Call::Read(300, 2)),
                "count {count}: the SOI read happens"
            );
        }
    }
}

// ============================================================================ cr2.rs 145:32

/// A source with a block budget: bytes in memory, `block` bytes per block, at most `max_blocks` distinct blocks may ever be
/// touched; a read that would touch one more fails with `InvalidData` (no I/O cause), the contract of every
/// `RawHeaderSource` ("budget exhausted"). Blocks already touched are free, like in the production sources.
struct BlockBudgetSource {
    data: Vec<u8>,
    block: usize,
    max_blocks: usize,
    touched: BTreeSet<usize>,
}

impl RawHeaderSource for BlockBudgetSource {
    fn length(&self) -> i64 {
        i64::try_from(self.data.len()).unwrap_or(i64::MAX)
    }
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let (Ok(start), Ok(count)) = (usize::try_from(offset), usize::try_from(count)) else {
            return Err(RawHeaderError::ArgumentOutOfRange {
                param: "offset",
                message: "negative".to_owned(),
            });
        };
        let Some(end) = start.checked_add(count).filter(|&e| e <= self.data.len()) else {
            return Err(RawHeaderError::invalid_data("past end"));
        };
        if count > 0 {
            let blocks: Vec<usize> = (start / self.block..=(end - 1) / self.block).collect();
            let fresh = blocks.iter().filter(|b| !self.touched.contains(b)).count();
            if self.touched.len() + fresh > self.max_blocks {
                return Err(RawHeaderError::invalid_data("header budget exhausted"));
            }
            self.touched.extend(blocks);
        }
        Ok(&self.data[start..end])
    }
}

/// CR2 whose IFD0 carries an Exif pointer entry with COUNT 0 (so `read_tag_value` is `None` and no block is made in the loop,
/// but `compute_exif_block` still sees the pointer, offset 2560 in a far block), IFD1 carries Exif pointer value 0, IFD2 follows.
fn cr2_with_zero_exif_pointer_in_ifd1(little: bool) -> Vec<u8> {
    TiffBytes::new(little, 3000)
        .header(16)
        .put(8, b"CR\x02\0\0\0\0\0")
        .ifd(
            16,
            256,
            &[short(0x0100, 100), short(0x0101, 60), at(0x8769, 4, 0, 2560)],
        )
        .ifd(256, 512, &[short(0x0100, 50), short(0x0101, 40), long(0x8769, 0)])
        .ifd(512, 0, &[short(0x0100, 120), short(0x0101, 80)])
        .ifd(2560, 0, &[short(0xA002, 120), short(0xA003, 80)])
        .into_vec()
}

/// `exif_offset > 0` -> `>= 0` (cr2.rs 145:32). N4 called this equivalent ("same call, only the time differs"); the time matters
/// when the source has a budget. With an Exif pointer of 0 in IFD1 the original builds the Exif block AFTER the loop (C#
/// `Cr2ContainerReader.cs` line 135), the mutant inside it, at IFD1. `compute_exif_block` reads the Exif IFD that IFD0's pointer
/// names (far block), so the mutant spends the last budget block there and the read of IFD2 fails, while the original finishes
/// the loop and only the (swallowed) Exif block growth hits the budget. Budget 3 blocks of 256 bytes: header+IFD0, IFD1, IFD2.
#[test]
fn an_exif_pointer_of_zero_in_a_later_ifd_builds_the_exif_block_after_the_loop() {
    for little in [true, false] {
        let mut source = BlockBudgetSource {
            data: cr2_with_zero_exif_pointer_in_ifd1(little),
            block: 256,
            max_blocks: 3,
            touched: BTreeSet::new(),
        };
        let info = Cr2ContainerReader::new()
            .read(&mut source, &CancelToken::new())
            .expect("the loop fits the budget; only the Exif block growth runs out and keeps the default block");
        assert_eq!((info.sensor_width, info.sensor_height), (120, 80), "little {little}");
        assert_eq!(info.exif_blocks.len(), 1);
        assert_eq!((info.exif_blocks[0].offset, info.exif_blocks[0].length), (0, 3000));
        assert!(info.previews.is_empty());
    }
}

/// Control for the test above: with a generous budget the same file reads the same size, so the budget (not the structure)
/// is what the test exercises.
#[test]
fn the_same_file_with_a_generous_budget_reads_the_same_sensor_size() {
    for little in [true, false] {
        let mut source = BlockBudgetSource {
            data: cr2_with_zero_exif_pointer_in_ifd1(little),
            block: 256,
            max_blocks: 100,
            touched: BTreeSet::new(),
        };
        let info = Cr2ContainerReader::new()
            .read(&mut source, &CancelToken::new())
            .expect("generous budget");
        assert_eq!((info.sensor_width, info.sensor_height), (120, 80));
        assert_eq!(info.exif_blocks.len(), 1);
    }
}

// ============================================================================ preview_selector.rs 160:25

fn known_size_jpeg(index: i32, length: i64, width: i32, height: i32) -> EmbeddedPreview {
    let mut preview = EmbeddedPreview::new(
        index,
        0,
        length,
        EmbeddedPreviewKind::Jpeg,
        width,
        height,
        PreviewColorSpace::Unknown,
    );
    preview.header_resolved = true;
    preview
}

fn selector_fixture() -> [EmbeddedPreview; 3] {
    // Sizes are known and resolved, so the selector never touches the (empty) source.
    [
        known_size_jpeg(0, 90_000, 1616, 1080),
        known_size_jpeg(1, 30_000, 640, 424),
        known_size_jpeg(2, 500_000, 6000, 4000),
    ]
}

fn select_in(
    source: &mut InMemoryRawHeaderSource,
    previews: &[EmbeddedPreview],
    box_width: i32,
    box_height: i32,
    orientation: i32,
) -> Option<EmbeddedPreview> {
    select_preview(
        source,
        previews,
        DecodeBox {
            width: box_width,
            height: box_height,
        },
        orientation,
    )
    .expect("known sizes never read")
}

fn select_with(box_width: i32, box_height: i32, orientation: i32) -> Option<EmbeddedPreview> {
    let mut source = InMemoryRawHeaderSource::new(Vec::new());
    select_in(&mut source, &selector_fixture(), box_width, box_height, orientation)
}

/// `else if req_h > 0` -> `>= 0` (preview_selector.rs 160:25, the `fits` chain for the partial box). The branch is reached for
/// `req_w <= 0` only, and an unbounded box (`width <= 0 && height <= 0`) returns before the loop, so `req_h` is positive there
/// and the `else { true }` arm is unreachable; the mutant differs from the original only for `req_w <= 0 && req_h == 0`, which is
/// the unbounded box. Fast part: the whole square -300..=300 x -300..=300 plus the i32 edges, both orientations.
#[test]
fn a_box_with_a_zero_side_and_no_positive_side_selects_like_the_unbounded_box() {
    for orientation in [1, 6] {
        let unbounded = select_with(0, 0, orientation);
        let sides = (-300..=300).chain([i32::MIN, i32::MIN + 1, i32::MAX - 1, i32::MAX]);
        for width in sides.clone() {
            for height in [0, -1, i32::MIN] {
                if width <= 0 {
                    assert_eq!(
                        select_with(width, height, orientation),
                        unbounded,
                        "box {width}x{height}"
                    );
                }
            }
        }
        // A positive height with a non-positive width is the partial box the branch is for: 1080 high, any width.
        for width in [0, -1, i32::MIN] {
            let chosen = select_with(width, 1000, orientation).expect("some preview");
            assert!(chosen.width.max(chosen.height) >= 1000);
        }
    }
}

/// The whole divergent domain of 160:25: every `req_w` in `i32::MIN..=0` with `req_h == 0` (2^31 + 1 boxes) through the real
/// selector; each must select exactly what the unbounded box selects. The orientation does not matter on this path (the
/// unbounded return comes before it is read); orientation 6 is covered by the fast test. Run explicitly with `--include-ignored`.
#[test]
#[ignore = "slow: 2^31 selector runs; run it with --include-ignored"]
fn every_box_with_a_non_positive_width_and_zero_height_selects_like_the_unbounded_box() {
    let workers: i64 =
        std::thread::available_parallelism().map_or(4, |n| i64::try_from(n.get()).unwrap_or(4).clamp(1, 8));
    let total: i64 = 1 << 31; // widths i32::MIN..=-1 are 2^31 values; width 0 is run after the loop.
    let unbounded = select_with(0, 0, 1);
    std::thread::scope(|scope| {
        for w in 0..workers {
            let unbounded = unbounded.clone();
            scope.spawn(move || {
                let previews = selector_fixture();
                let mut source = InMemoryRawHeaderSource::new(Vec::new());
                let mut k = w;
                while k < total {
                    let width = i32::try_from(i64::from(i32::MIN) + k).expect("in range");
                    assert_eq!(
                        select_in(&mut source, &previews, width, 0, 1),
                        unbounded,
                        "box {width}x0"
                    );
                    k += workers;
                }
            });
        }
    });
    assert_eq!(select_with(0, 0, 1), unbounded);
}
