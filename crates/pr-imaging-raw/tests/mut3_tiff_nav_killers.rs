//! MUT3 / P1 (2026-11-06): killers and class runs for the `tiff_nav.rs` survivors of the whole-file mutation run:
//! `read_ifd_entries` 78:38, `read_tag_unsigned_array` 172:18, `compute_exif_block` 215:25, `has_square_default_scale` 355:19 and
//! 362:16 / 362:27 / 362:38, `clamp_to_int` 494:22, `collect_items` 513:15.
//! Inline bytes only: no golden file, no corpus, no sleep, no production change. Evidence: `docs/reviews/P1-MUT-2026-11-06.md`.

use std::cell::RefCell;
use std::rc::Rc;

use pr_imaging_raw::tiff_nav::{
    TiffEntry, clamp_to_int, compute_exif_block, has_square_default_scale, read_ifd_entries, read_tag_unsigned_array,
};
use pr_imaging_raw::{InMemoryRawHeaderSource, RawHeaderError, RawHeaderSource};
use pr_test_support::raw::{Entry, TiffBytes, at, long, short};

fn default_scale_entry(offset: u32) -> TiffEntry {
    TiffEntry {
        tag: 0xC61E,
        field_type: 5,
        count: 2,
        value_or_offset: offset,
    }
}

fn scale_bytes(little: bool, hn: u32, hd: u32, vn: u32, vd: u32) -> Vec<u8> {
    let mut data = vec![0xEEu8; 16];
    for value in [hn, hd, vn, vd] {
        data.extend_from_slice(&if little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        });
    }
    data.extend_from_slice(&[0xEE; 16]);
    data
}

/// Answers every read with `count + 8` bytes (as many as the file has): a source may hand back more than it was asked for, the
/// reader must use the first `count` bytes and nothing else.
struct GenerousSource {
    data: Vec<u8>,
}

impl RawHeaderSource for GenerousSource {
    fn length(&self) -> i64 {
        i64::try_from(self.data.len()).unwrap_or(i64::MAX)
    }
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let start = usize::try_from(offset).map_err(|_| RawHeaderError::invalid_data("negative offset"))?;
        let wanted = usize::try_from(count).map_err(|_| RawHeaderError::invalid_data("negative count"))?;
        if start > self.data.len() || wanted > self.data.len() - start {
            return Err(RawHeaderError::invalid_data("past end"));
        }
        let end = (start + wanted + 8).min(self.data.len());
        Ok(&self.data[start..end])
    }
}

/// tiff_nav.rs:355:19 `span.len() < 16` -> `> 16`. A 16-byte read is answered with exactly 16 bytes by the two in-crate sources,
/// so for them the condition is false in both versions; the mutant differs for a source that answers with MORE bytes (it would
/// reject a perfectly good 1:1 scale) and, for a SHORTER answer, both versions end in `false` (the reads past the end give 0).
/// Original: the first 16 bytes decide.
#[test]
fn a_default_scale_read_answered_with_extra_bytes_is_judged_on_its_first_sixteen() {
    for little in [true, false] {
        let square = scale_bytes(little, 3, 2, 6, 4);
        let mut generous = GenerousSource { data: square.clone() };
        assert!(
            has_square_default_scale(&mut generous, &[default_scale_entry(16)], little).expect("read"),
            "little {little}"
        );
        let mut exact = InMemoryRawHeaderSource::new(square);
        assert!(has_square_default_scale(&mut exact, &[default_scale_entry(16)], little).expect("read"));

        let skewed = scale_bytes(little, 3, 2, 5, 4);
        let mut generous = GenerousSource { data: skewed };
        assert!(!has_square_default_scale(&mut generous, &[default_scale_entry(16)], little).expect("read"));
    }
}

/// tiff_nav.rs:362:16, 362:27, 362:38 (`hn == 0 || hd == 0 || vn == 0 || vd == 0`, each `||` -> `&&`). The result for four
/// values depends on them only through "is it zero" (the 16 patterns) as long as one is zero: the equation
/// `hn * vd == vn * hd` is then decided by the pattern alone (`hn * vd` is 0 exactly when `hn == 0 || vd == 0`, `vn * hd` exactly
/// when `vn == 0 || hd == 0`), and with no zero all four mutants and the original are the same code path. Run: all 12^4 =
/// 20 736 combinations of {0..=7, 2^16, 2^31 - 1, 2^31, 2^32 - 1} (every zero pattern with twelve magnitudes per
/// non-zero slot) in both byte orders against the contract "any zero is not square, else cross-multiplied equality in u64".
/// 362:38 is killed by the 0/0 and 0/x-and-0/y rows; 362:16 and 362:27 pass on all of them.
#[test]
fn a_default_scale_with_a_zero_numerator_or_denominator_is_never_square() {
    let values: Vec<u32> = (0..=7).chain([1 << 16, (1 << 31) - 1, 1 << 31, u32::MAX]).collect();
    let mut checked = 0usize;
    for little in [true, false] {
        for &hn in &values {
            for &hd in &values {
                for &vn in &values {
                    for &vd in &values {
                        let mut source = InMemoryRawHeaderSource::new(scale_bytes(little, hn, hd, vn, vd));
                        let got =
                            has_square_default_scale(&mut source, &[default_scale_entry(16)], little).expect("read");
                        let any_zero = hn == 0 || hd == 0 || vn == 0 || vd == 0;
                        let expected = !any_zero && u64::from(hn) * u64::from(vd) == u64::from(vn) * u64::from(hd);
                        assert_eq!(got, expected, "little {little} h {hn}/{hd} v {vn}/{vd}");
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 2 * 12usize.pow(4));
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Length,
    Read(i64, i32),
}

struct Tripwire {
    inner: InMemoryRawHeaderSource,
    log: Rc<RefCell<Vec<Call>>>,
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

/// tiff_nav.rs:78:38 `ifd_offset + 2 + clamped * 12 > len` -> `>=`. The two differ only when the entries end EXACTLY at the end
/// of the source. For `entry_count >= 1` the clamp `(len - ifd - 2) / 12` returns the same count, so the result is the same
/// (run: every IFD offset 8..=40 and every count 1..=300 plus 4096 and 4097, both byte orders). For `entry_count == 0` the
/// original goes on and asks for the empty entry span (C# `source.Read(ifdOffset + 2, 0)`, then finds no room for the next
/// pointer), the mutant returns early without that call: the results are equal (no entries, no next IFD), the call sequence is
/// not, and the sequence is what this test pins.
#[test]
fn an_ifd_that_ends_exactly_at_the_end_of_the_source_is_read_as_the_original_reads_it() {
    for little in [true, false] {
        for ifd in 8..=40usize {
            for entry_count in (1..=300u16).chain([4096, 4097]) {
                let clamped = usize::from(entry_count.min(4096));
                let len = ifd + 2 + clamped * 12;
                let mut data = vec![0u8; len];
                data[ifd..ifd + 2].copy_from_slice(&if little {
                    entry_count.to_le_bytes()
                } else {
                    entry_count.to_be_bytes()
                });
                for i in 0..clamped {
                    let base = ifd + 2 + i * 12;
                    let tag = u16::try_from(i % 60_000).expect("fits") + 1;
                    data[base..base + 2].copy_from_slice(&if little { tag.to_le_bytes() } else { tag.to_be_bytes() });
                }
                let mut source = InMemoryRawHeaderSource::new(data);
                let read = read_ifd_entries(&mut source, i64::try_from(ifd).expect("fits"), little).expect("read");
                assert_eq!(
                    read.entries.len(),
                    clamped,
                    "ifd {ifd} count {entry_count} little {little}"
                );
                assert_eq!(read.next_ifd_offset, 0);
            }
        }
    }
}

#[test]
fn an_empty_ifd_whose_count_word_ends_the_source_still_asks_for_the_empty_entry_span() {
    for little in [true, false] {
        for ifd in [8usize, 9, 16, 100] {
            let mut data = vec![0u8; ifd + 2];
            data[ifd..ifd + 2].copy_from_slice(&0u16.to_le_bytes());
            let log = Rc::new(RefCell::new(Vec::new()));
            let mut source = Tripwire {
                inner: InMemoryRawHeaderSource::new(data),
                log: Rc::clone(&log),
            };
            let ifd = i64::try_from(ifd).expect("fits");
            let read = read_ifd_entries(&mut source, ifd, little).expect("read");
            assert!(read.entries.is_empty());
            assert_eq!(read.next_ifd_offset, 0);
            assert_eq!(
                *log.borrow(),
                vec![Call::Length, Call::Read(ifd, 2), Call::Read(ifd + 2, 0)],
                "ifd {ifd} little {little}"
            );
        }
    }
}

// ============================================================================ tiff_nav.rs 172:18

fn array_entry(field_type: u16, count: u32, value: u32) -> TiffEntry {
    TiffEntry {
        tag: 0x014A,
        field_type,
        count,
        value_or_offset: value,
    }
}

/// tiff_nav.rs:172:18 `size == 0 || entry.count == 0 || max_u32 == 0` -> `(size == 0 && entry.count == 0) || max_u32 == 0`.
/// The two differ for `size == 0` with a non-zero count, and for a non-zero `size` with `count == 0` (and `max_items > 0`).
/// Case 1: `size == 0` is a field type unknown to `type_size`; `collect_items` then runs its loop with `size == 0`, every
/// iteration looks at the same `span[0..]` and `read_unsigned` answers `None` for every such type, so no item is ever produced.
/// Run: EVERY u16 field type with `type_size == 0` (65 523 of them) x both byte orders x counts 1, 2, 7, 70 000 x four value
/// words: the list is empty. Case 2: a known type with count 0 has `n == 0` and `total == 0` (inline, zero iterations): every
/// known type 1..=13 x both byte orders x five value words x three `max_items`: empty.
#[test]
fn an_array_entry_of_unknown_type_or_of_zero_count_reads_as_an_empty_list() {
    use pr_imaging::tiff::type_size;
    let values = [0u32, 1, 0x0102_0304, u32::MAX];
    let mut unknown_types = 0usize;
    for field_type in 0..=u16::MAX {
        if type_size(field_type) != 0 {
            continue;
        }
        unknown_types += 1;
        for little in [true, false] {
            for count in [1u32, 2, 7, 70_000] {
                for value in values {
                    let mut source = InMemoryRawHeaderSource::new(vec![0xAB; 64]);
                    let items =
                        read_tag_unsigned_array(&mut source, &array_entry(field_type, count, value), little, 64)
                            .expect("no read happens");
                    assert!(
                        items.is_empty(),
                        "type {field_type} count {count} value {value:#x} little {little}"
                    );
                }
            }
        }
    }
    assert_eq!(unknown_types, 65_523);

    for field_type in 1..=13u16 {
        if type_size(field_type) == 0 {
            continue;
        }
        for little in [true, false] {
            for value in [0u32, 1, 0x0102_0304, 16, u32::MAX] {
                for max_items in [1, 64, i32::MAX] {
                    let mut source = InMemoryRawHeaderSource::new(vec![0xAB; 64]);
                    let items =
                        read_tag_unsigned_array(&mut source, &array_entry(field_type, 0, value), little, max_items)
                            .expect("no read happens");
                    assert!(
                        items.is_empty(),
                        "type {field_type} value {value:#x} max {max_items} little {little}"
                    );
                }
            }
        }
    }
}

// ============================================================================ tiff_nav.rs 494:22

/// tiff_nav.rs:494:22 `Some(v) if v > 0` -> `v >= 0` (`clamp_to_int`). The two guards differ only for `v == 0`, one value; the
/// mutant then answers `i32::try_from(0)`, which is 0, the same as the fallback arm. The whole divergent domain is run, with the
/// neighbours and the saturation edges.
#[test]
fn clamp_to_int_of_zero_is_zero_and_the_neighbours_keep_their_meaning() {
    assert_eq!(clamp_to_int(Some(0)), 0);
    assert_eq!(clamp_to_int(None), 0);
    assert_eq!(clamp_to_int(Some(-1)), 0);
    assert_eq!(clamp_to_int(Some(i64::MIN)), 0);
    assert_eq!(clamp_to_int(Some(1)), 1);
    assert_eq!(clamp_to_int(Some(i64::from(i32::MAX))), i32::MAX);
    assert_eq!(clamp_to_int(Some(i64::from(i32::MAX) + 1)), i32::MAX);
    assert_eq!(clamp_to_int(Some(i64::MAX)), i32::MAX);
}

// ============================================================================ tiff_nav.rs 215:25

/// A file of `len` bytes with IFD0 at 16 holding the given entries; everything else zero.
fn exif_block_file(little: bool, len: usize, entries: &[Entry]) -> Vec<u8> {
    TiffBytes::new(little, len).header(16).ifd(16, 0, entries).into_vec()
}

/// tiff_nav.rs:215:25 `exif_pointer > 0` -> `>= 0` (`compute_exif_block`). The mutant differs only for an Exif pointer of 0 (an
/// entry 0x8769 whose value is 0, or no such entry), where it runs one more `exif_summary_extent(source, 0, ..)`. That call asks
/// `read_ifd_entries` for offset 0, which reads nothing and answers "no entries" whatever the source (run with a panicking
/// source and the i64 length edges in `mut3_raw_selector_proofs`), so it contributes `(0, 0)` and `needed.max(0)` keeps `needed`.
/// Run: the block length for pointer 0 / no pointer in both byte orders and four sizes of `needed` (nothing, below the 128 KiB
/// default, above it, above `min(len, 4 MiB)`), against the documented rule: `max(min(len, 128 KiB), needed)` when `needed` fits
/// `min(len, 4 MiB)`, else `min(len, 128 KiB)`.
#[test]
fn an_exif_pointer_of_zero_adds_nothing_to_the_exif_block_length() {
    const DEFAULT: i64 = 128 * 1024;
    let make = at(0x010F, 2, 20, 200_000); // IFD0 "Make": 20 ASCII bytes out of line at 200 000
    let model = at(0x0110, 2, 20, 5_000_000); // beyond min(len, 4 MiB)
    for little in [true, false] {
        for pointer in [None, Some(0u32)] {
            let with_pointer = |mut entries: Vec<Entry>| {
                if let Some(value) = pointer {
                    entries.push(long(0x8769, value));
                }
                entries
            };
            // (entries, file length, expected block length)
            let cases: Vec<(Vec<Entry>, usize, i64)> = vec![
                (with_pointer(vec![short(0x0100, 10)]), 600, 600),
                (with_pointer(vec![short(0x0100, 10)]), 300_000, DEFAULT),
                (with_pointer(vec![short(0x0100, 10), make]), 300_000, 200_020),
                (with_pointer(vec![short(0x0100, 10), model]), 6_000_000, DEFAULT),
            ];
            for (entries, len, expected) in cases {
                let mut source = InMemoryRawHeaderSource::new(exif_block_file(little, len, &entries));
                let block = compute_exif_block(&mut source, little, 16).expect("compute");
                assert_eq!(
                    (block.offset, block.length),
                    (0, expected),
                    "len {len} pointer {pointer:?} little {little}"
                );
            }
        }
    }
}

// ============================================================================ tiff_nav.rs 513:15

/// Answers every read with only the first `keep` bytes it was asked for (a source may hand back less than it was asked for).
struct ShortSource {
    data: Vec<u8>,
    keep: usize,
}

impl RawHeaderSource for ShortSource {
    fn length(&self) -> i64 {
        i64::try_from(self.data.len()).unwrap_or(i64::MAX)
    }
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let start = usize::try_from(offset).map_err(|_| RawHeaderError::invalid_data("negative offset"))?;
        let wanted = usize::try_from(count).map_err(|_| RawHeaderError::invalid_data("negative count"))?;
        if start > self.data.len() || wanted > self.data.len() - start {
            return Err(RawHeaderError::invalid_data("past end"));
        }
        Ok(&self.data[start..start + wanted.min(self.keep)])
    }
}

/// tiff_nav.rs:513:15 `(i + 1) * size > span_len` -> `(i * 1) * size > span_len` (`collect_items`). With a source that answers
/// complete spans (both in-crate sources, and the inline buffer) the loop never reaches the condition with an item that does
/// not fit, so the two are the same there. They differ only when the span is SHORTER than `n * size` (a short answer): the
/// original stops before the first partial item, the mutant steps once into it, and the partial item decodes to nothing for
/// every known type (`read_unsigned` and the RATIONAL branch need the whole item). Run: all 13 known types x both byte orders x
/// counts 1..=6 x every short answer 0..=count*size bytes of a 0xA5-filled and a 0xFF-filled array: the decoded list is the
/// whole items that exist. The contents of the partial item are two fills, not every value, hence "run-supported" in the report.
#[test]
fn a_short_answer_yields_exactly_the_whole_items_it_contains() {
    use pr_imaging::tiff::type_size;
    for field_type in 1..=13u16 {
        let size = usize::try_from(type_size(field_type)).expect("non-negative");
        for little in [true, false] {
            for count in 1..=6u32 {
                let want = size * usize::try_from(count).expect("small");
                if want <= 4 {
                    continue; // inline: the span is the 4-byte value field, no source read
                }
                for keep in 0..=want {
                    for fill in [0xA5u8, 0xFF] {
                        let mut source = ShortSource {
                            data: vec![fill; 64],
                            keep,
                        };
                        let items =
                            read_tag_unsigned_array(&mut source, &array_entry(field_type, count, 8), little, 64)
                                .expect("read");
                        // Types 1, 3, 4, 5, 8, 9, 13 decode; 2, 6, 7, 10, 11, 12 have a size but no unsigned reading.
                        let decodable = matches!(field_type, 1 | 3 | 4 | 5 | 8 | 9 | 13);
                        let expected = if decodable { keep / size } else { 0 };
                        assert_eq!(
                            items.len(),
                            expected,
                            "type {field_type} count {count} keep {keep} fill {fill:#x} little {little}"
                        );
                    }
                }
            }
        }
    }
}
