//! RAW-GUARDS (2026-11-11): differential proofs for the no-behaviour-change refactors behind the run-supported survivors of
//! `docs/reviews/P1-MUT-2026-11-06.md`: `tiff_nav::read_tag_unsigned_array` (172:18, 513:15) and `preview_selector`
//! `is_adobe_rgb_exif` (400:18) / `try_find_entry` (421:37, 421:41, 427:18).
//!
//! Each test carries the formula as it was BEFORE the refactor (the C# text) as a reference function and compares it with the
//! production function over a whole class domain, including the call sequence the source sees. Inline bytes only.
//! Evidence: `docs/reviews/RAW-GUARDS-MUT-2026-11-11.md`.

use std::cell::RefCell;

use pr_imaging::tiff::{read_u16, read_u32, read_unsigned, type_size};
use pr_imaging_raw::preview_selector::{is_adobe_rgb_exif, try_find_entry};
use pr_imaging_raw::tiff_nav::{TiffEntry, read_tag_unsigned_array};
use pr_imaging_raw::{RawHeaderError, RawHeaderSource};

// ---------------------------------------------------------------------------------------------------------------------
// tiff_nav: reference = the code before the refactor.
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Answer {
    /// Exactly the bytes asked for.
    Exact,
    /// `n` bytes fewer than asked (a source that answers short).
    Short(usize),
    /// `n` bytes more than asked (when the file has them).
    Generous(usize),
}

struct LoggingSource {
    data: Vec<u8>,
    answer: Answer,
    log: RefCell<Vec<String>>,
}

impl LoggingSource {
    fn new(data: &[u8], answer: Answer) -> Self {
        Self {
            data: data.to_vec(),
            answer,
            log: RefCell::new(Vec::new()),
        }
    }
}

impl RawHeaderSource for LoggingSource {
    fn length(&self) -> i64 {
        self.log.borrow_mut().push("L".to_owned());
        i64::try_from(self.data.len()).unwrap_or(i64::MAX)
    }
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        self.log.borrow_mut().push(format!("R {offset} {count}"));
        let start = usize::try_from(offset).map_err(|_| RawHeaderError::invalid_data("negative offset"))?;
        let wanted = usize::try_from(count).map_err(|_| RawHeaderError::invalid_data("negative count"))?;
        if start > self.data.len() || wanted > self.data.len() - start {
            return Err(RawHeaderError::invalid_data("past end"));
        }
        let given = match self.answer {
            Answer::Exact => wanted,
            Answer::Short(n) => wanted.saturating_sub(n),
            Answer::Generous(n) => wanted + n,
        };
        let end = (start + given).min(self.data.len());
        Ok(&self.data[start..end])
    }
}

fn inline_bytes(value: u32, little_endian: bool) -> [u8; 4] {
    if little_endian {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    }
}

/// `collect_items` as it was (guard `(i + 1) * size > span_len`, item = rest of the span).
fn old_collect_items(span: &[u8], n: i64, size: i64, field_type: u16, little_endian: bool, out: &mut Vec<i64>) {
    let span_len = i64::try_from(span.len()).unwrap_or(i64::MAX);
    for i in 0..n {
        if (i + 1) * size > span_len {
            break;
        }
        let Ok(start) = usize::try_from(i * size) else { break };
        let item = &span[start..];
        if field_type == 5 {
            if item.len() < 8 {
                continue;
            }
            let num = read_u32(item, 0, little_endian);
            let den = read_u32(item, 4, little_endian);
            if den == 0 {
                continue;
            }
            out.push((f64::from(num) / f64::from(den)).round() as i64);
        } else if let Some(v) = read_unsigned(item, field_type, little_endian) {
            out.push(v);
        }
    }
}

/// `read_tag_unsigned_array` as it was (guard `size == 0 || count == 0 || max == 0`).
fn old_read_tag_unsigned_array(
    source: &mut dyn RawHeaderSource,
    entry: &TiffEntry,
    little_endian: bool,
    max_items: i32,
) -> Result<Vec<i64>, RawHeaderError> {
    let size = i64::from(type_size(entry.field_type));
    let Ok(max_u32) = u32::try_from(max_items) else {
        return Ok(Vec::new());
    };
    if size == 0 || entry.count == 0 || max_u32 == 0 {
        return Ok(Vec::new());
    }
    let n = i64::from(entry.count.min(max_u32));
    let total = size * i64::from(entry.count);
    let mut result = Vec::new();
    if total <= 4 {
        let buf = inline_bytes(entry.value_or_offset, little_endian);
        old_collect_items(&buf, n, size, entry.field_type, little_endian, &mut result);
        return Ok(result);
    }
    let len = source.length();
    let offset = i64::from(entry.value_or_offset);
    let want = n * size;
    if offset + want > len {
        return Ok(result);
    }
    let Ok(want_i32) = i32::try_from(want) else {
        return Ok(result);
    };
    let span = source.read(offset, want_i32)?;
    old_collect_items(span, n, size, entry.field_type, little_endian, &mut result);
    Ok(result)
}

fn fills() -> Vec<Vec<u8>> {
    let pattern: Vec<u8> = (0..96u32).map(|i| ((i * 37 + 11) & 0xFF) as u8).collect();
    // Rationals with a zero denominator in some slots.
    let gappy: Vec<u8> = (0..96usize)
        .map(|i| {
            if i % 8 >= 4 && i % 16 >= 8 {
                0
            } else {
                (i as u8).wrapping_mul(3).wrapping_add(1)
            }
        })
        .collect();
    vec![pattern, vec![0u8; 96], vec![0xFFu8; 96], gappy]
}

/// Every field type 0..=20 plus the unknown extremes, both byte orders, counts at every class edge, `max_items` at every class
/// edge, value words inline / in range / past the end / `u32::MAX`, 4 fills, source behaviours exact, generous and short by
/// 1..=9: the new function gives the same list (or the same error) and the source sees the same calls as the old one.
#[test]
fn read_tag_unsigned_array_equals_the_pre_refactor_formula_with_the_same_source_calls() {
    let types: Vec<u16> = (0..=20u16).chain([255, 256, 0x8000, u16::MAX - 1, u16::MAX]).collect();
    let counts = [
        0u32,
        1,
        2,
        3,
        4,
        5,
        6,
        7,
        8,
        11,
        12,
        13,
        64,
        65,
        70_000,
        0x7FFF_FFFF,
        0x8000_0000,
        u32::MAX,
    ];
    let maxes = [i32::MIN, -1, 0, 1, 2, 3, 4, 5, 64, 65, 1 << 20, i32::MAX];
    let words = [0u32, 1, 4, 5, 8, 16, 33, 90, 95, 96, 97, 0x0102_0304, u32::MAX];
    let answers: Vec<Answer> = [Answer::Exact, Answer::Generous(3)]
        .into_iter()
        .chain((1..=9).map(Answer::Short))
        .collect();
    let mut checked = 0u64;
    let mut non_empty = 0u64;
    for fill in fills() {
        for &field_type in &types {
            for little in [true, false] {
                for &count in &counts {
                    for &value_or_offset in &words {
                        let entry = TiffEntry {
                            tag: 0x1234,
                            field_type,
                            count,
                            value_or_offset,
                        };
                        for &max in &maxes {
                            for &answer in &answers {
                                let mut old_src = LoggingSource::new(&fill, answer);
                                let mut new_src = LoggingSource::new(&fill, answer);
                                let old = old_read_tag_unsigned_array(&mut old_src, &entry, little, max);
                                let new = read_tag_unsigned_array(&mut new_src, &entry, little, max);
                                let ctx = format!("{entry:?} little={little} max={max} {answer:?}");
                                assert_eq!(format!("{old:?}"), format!("{new:?}"), "result differs for {ctx}");
                                assert_eq!(
                                    *old_src.log.borrow(),
                                    *new_src.log.borrow(),
                                    "source calls differ for {ctx}"
                                );
                                checked += 1;
                                if matches!(&new, Ok(v) if !v.is_empty()) {
                                    non_empty += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(checked > 500_000, "the class run shrank: {checked}");
    assert!(
        non_empty > 10_000,
        "too few non-empty answers to be a meaningful run: {non_empty}"
    );
}

/// The ways to ask for nothing never touch the source (no `length()`, no `read()`), for every type, also a type whose size
/// would otherwise make the read go out of line.
#[test]
fn asking_for_nothing_never_touches_the_source() {
    for field_type in 0..=20u16 {
        for (count, max) in [
            (0u32, 64i32),
            (u32::MAX, 0),
            (u32::MAX, -1),
            (u32::MAX, i32::MIN),
            (0, 0),
        ] {
            let entry = TiffEntry {
                tag: 1,
                field_type,
                count,
                value_or_offset: 8,
            };
            let mut source = LoggingSource::new(&[7u8; 64], Answer::Exact);
            let list = read_tag_unsigned_array(&mut source, &entry, true, max).expect("no read, no error");
            assert!(list.is_empty());
            assert!(
                source.log.borrow().is_empty(),
                "type {field_type} count {count} max {max} touched the source"
            );
        }
    }
    // An unknown type with a real count and room never touches the source either.
    for field_type in [0u16, 14, 15, 99, u16::MAX] {
        let entry = TiffEntry {
            tag: 1,
            field_type,
            count: 70_000,
            value_or_offset: 8,
        };
        let mut source = LoggingSource::new(&[7u8; 64], Answer::Exact);
        assert!(
            read_tag_unsigned_array(&mut source, &entry, false, 64)
                .expect("empty")
                .is_empty()
        );
        assert!(source.log.borrow().is_empty());
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// preview_selector: reference = the C# text before the refactor.
// ---------------------------------------------------------------------------------------------------------------------

fn u16_at(data: &[u8], offset: i64, little: bool) -> u16 {
    read_u16(data, i32::try_from(offset).unwrap_or(i32::MAX), little)
}

fn u32_at(data: &[u8], offset: i64, little: bool) -> u32 {
    read_u32(data, i32::try_from(offset).unwrap_or(i32::MAX), little)
}

/// `try_find_entry` with its early `ifd_offset + 2 > len` guard (C# `TryFindEntry`).
fn old_try_find_entry(tiff: &[u8], ifd_offset: i64, tag: u16, little: bool) -> Option<i64> {
    let len = tiff.len() as i64;
    if ifd_offset < 8 || ifd_offset + 2 > len {
        return None;
    }
    let count = u16_at(tiff, ifd_offset, little);
    for i in 0..i64::from(count) {
        let entry = ifd_offset + 2 + 12 * i;
        if entry + 12 > len {
            return None;
        }
        if u16_at(tiff, entry, little) == tag {
            return Some(entry);
        }
    }
    None
}

fn old_try_find_long_entry(tiff: &[u8], ifd_offset: i64, tag: u16, little: bool) -> Option<i64> {
    let entry = old_try_find_entry(tiff, ifd_offset, tag, little)?;
    if u16_at(tiff, entry + 2, little) != 4 || u32_at(tiff, entry + 4, little) != 1 {
        return None;
    }
    Some(i64::from(u32_at(tiff, entry + 8, little)))
}

/// `is_adobe_rgb_exif` with the trailing `entry + 12 <= len` conjunct (C# `IsAdobeRgbExif`).
fn old_is_adobe_rgb_exif(payload: &[u8]) -> bool {
    if payload.len() < 14 || !payload.starts_with(b"Exif\0\0") {
        return false;
    }
    let tiff = &payload[6..];
    let little = match (tiff[0], tiff[1]) {
        (b'I', b'I') => true,
        (b'M', b'M') => false,
        _ => return false,
    };
    if u16_at(tiff, 2, little) != 42 {
        return false;
    }
    let ifd0 = i64::from(u32_at(tiff, 4, little));
    let Some(exif_ifd) = old_try_find_long_entry(tiff, ifd0, 0x8769, little) else {
        return false;
    };
    let Some(interop_ifd) = old_try_find_long_entry(tiff, exif_ifd, 0xA005, little) else {
        return false;
    };
    let Some(entry) = old_try_find_entry(tiff, interop_ifd, 0x0001, little) else {
        return false;
    };
    u16_at(tiff, entry + 2, little) == 2
        && u32_at(tiff, entry + 4, little) == 4
        && entry + 12 <= tiff.len() as i64
        && usize::try_from(entry + 8)
            .ok()
            .and_then(|s| tiff.get(s..s + 4))
            .is_some_and(|b| b == b"R03\0")
}

fn put16(out: &mut Vec<u8>, v: u16, little: bool) {
    out.extend_from_slice(&if little { v.to_le_bytes() } else { v.to_be_bytes() });
}

fn put32(out: &mut Vec<u8>, v: u32, little: bool) {
    out.extend_from_slice(&inline_bytes(v, little));
}

/// "Exif\0\0" + TIFF with IFD0 (8) -> Exif IFD (26) -> Interop IFD (44) holding InteropIndex "R03\0"; each IFD is
/// count + one entry + next pointer, the TIFF is 62 bytes.
fn adobe_payload(little: bool) -> Vec<u8> {
    let mut p = b"Exif\0\0".to_vec();
    p.extend_from_slice(if little { b"II" } else { b"MM" });
    put16(&mut p, 42, little);
    put32(&mut p, 8, little);
    for (tag, ty, count, value) in [(0x8769u16, 4u16, 1u32, 26u32), (0xA005, 4, 1, 44)] {
        put16(&mut p, 1, little);
        put16(&mut p, tag, little);
        put16(&mut p, ty, little);
        put32(&mut p, count, little);
        put32(&mut p, value, little);
        put32(&mut p, 0, little);
    }
    put16(&mut p, 1, little);
    put16(&mut p, 1, little);
    put16(&mut p, 2, little);
    put32(&mut p, 4, little);
    p.extend_from_slice(b"R03\0");
    put32(&mut p, 0, little);
    p
}

#[test]
fn is_adobe_rgb_exif_equals_the_pre_refactor_formula_for_every_truncation_and_byte_edit() {
    let mut accepted = 0;
    for little in [true, false] {
        let full = adobe_payload(little);
        assert_eq!(full.len(), 6 + 62);
        assert!(is_adobe_rgb_exif(&full), "the fixture must be accepted");
        // The interop entry ends at TIFF byte 58: every shorter payload is refused, 58 and longer accepted.
        for len in 0..=full.len() {
            let cut = &full[..len];
            assert_eq!(
                is_adobe_rgb_exif(cut),
                old_is_adobe_rgb_exif(cut),
                "truncation {len} little={little}"
            );
            assert_eq!(
                is_adobe_rgb_exif(cut),
                len >= 6 + 58,
                "truncation {len} little={little}"
            );
            accepted += usize::from(is_adobe_rgb_exif(cut));
        }
        // Every single-byte edit (class values), full and cut just after / before the end of the interop entry.
        for pos in 0..full.len() {
            for value in [
                0u8,
                1,
                2,
                4,
                8,
                12,
                26,
                44,
                57,
                58,
                62,
                0x7F,
                0xFF,
                full[pos].wrapping_add(1),
            ] {
                for len in [full.len(), 6 + 58, 6 + 57] {
                    let mut edited = full[..len].to_vec();
                    edited[pos.min(len - 1)] = value;
                    assert_eq!(
                        is_adobe_rgb_exif(&edited),
                        old_is_adobe_rgb_exif(&edited),
                        "edit pos {pos} value {value} len {len} little={little}"
                    );
                }
            }
        }
        // Every IFD pointer of the chain (IFD0 pointer, Exif pointer, Interop pointer) over the whole short range and the
        // u32 edges, with several truncations of the payload.
        for pointer_at in [6 + 4, 6 + 8 + 2 + 8, 6 + 26 + 2 + 8] {
            for pointer in (0u32..=80).chain([0x7FFF_FFFE, 0x7FFF_FFFF, 0x8000_0000, u32::MAX - 1, u32::MAX]) {
                for len in [full.len(), 6 + 62 - 4, 6 + 58, 6 + 50, 6 + 45, 6 + 44, 6 + 30] {
                    let mut edited = full[..len].to_vec();
                    for (k, b) in inline_bytes(pointer, little).iter().enumerate() {
                        if pointer_at + k < edited.len() {
                            edited[pointer_at + k] = *b;
                        }
                    }
                    assert_eq!(
                        is_adobe_rgb_exif(&edited),
                        old_is_adobe_rgb_exif(&edited),
                        "pointer at {pointer_at} = {pointer} len {len} little={little}"
                    );
                }
            }
        }
    }
    assert!(accepted > 0);
}

/// Seeded xorshift so the run is reproducible.
fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn compare_entry(tiff: &[u8], ifd: i64, tag: u16, little: bool) -> bool {
    let new = try_find_entry(tiff, ifd, tag, little);
    assert_eq!(
        new,
        old_try_find_entry(tiff, ifd, tag, little),
        "len {} ifd {ifd} tag {tag:#x} little={little}",
        tiff.len()
    );
    new.is_some()
}

/// `try_find_entry` over every length class x every IFD offset class (negative, 0..=len + 20, the i32 / u32 edges: the reachable
/// domain is `u32` widened, `i64::MAX` would overflow the C# `long` arithmetic too) x both byte orders x tags, with structured
/// entries (the wanted tag in the LAST, possibly partial, entry; counts 0, 1, 2, 0xFFFF) and random bytes.
#[test]
fn try_find_entry_equals_the_pre_refactor_formula_over_every_truncation_and_pointer_class() {
    let tags = [0x8769u16, 0xA005, 0x0001, 0x0000, 0xFFFF];
    let edge_ifds = [
        i64::MIN,
        -1_000_000,
        -1,
        0x7FFF_FFFE,
        0x7FFF_FFFF,
        0x8000_0000,
        0x8000_0001,
        0xFFFF_FFFE,
        0xFFFF_FFFF,
        0x1_0000_0000,
        i64::MAX / 4,
    ];
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut found = 0u64;
    let mut checked = 0u64;
    for little in [true, false] {
        // Structured: IFD at 8, `k` whole entries then `r` bytes of one more entry (len = 10 + 12k + r), the tag in the last
        // entry that fits (whole or partial) or in every entry; counts 0, 1, 2, 3, k, k + 1, 0xFFFF.
        for k in 0..=4usize {
            for r in 0..=13usize {
                for &tag in &tags[..3] {
                    for count in [0u16, 1, 2, 3, k as u16, k as u16 + 1, 0xFFFF] {
                        for only_last in [false, true] {
                            let len = 10 + 12 * k + r;
                            let mut tiff = vec![0xEEu8; len];
                            let mut head = Vec::new();
                            put16(&mut head, count, little);
                            tiff[8..10].copy_from_slice(&head);
                            let mut tag_bytes = Vec::new();
                            put16(&mut tag_bytes, tag, little);
                            for e in 0..=k {
                                if only_last && e != k {
                                    continue;
                                }
                                for (i, b) in tag_bytes.iter().enumerate() {
                                    let at = 10 + 12 * e + i;
                                    if at < tiff.len() {
                                        tiff[at] = *b;
                                    }
                                }
                            }
                            for ifd in (-2i64..=(len as i64 + 20)).chain(edge_ifds) {
                                found += u64::from(compare_entry(&tiff, ifd, tag, little));
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        // Random content over every short length.
        for len in 0..=60usize {
            for _ in 0..40 {
                let tiff: Vec<u8> = (0..len)
                    .map(|_| match next(&mut state) % 6 {
                        0 => 0,
                        1 => 1,
                        2 => 0xFF,
                        3 => 0xA0,
                        4 => 0x05,
                        _ => (next(&mut state) & 0xFF) as u8,
                    })
                    .collect();
                for ifd in (-2i64..=(len as i64 + 20)).chain(edge_ifds) {
                    for &tag in &tags {
                        found += u64::from(compare_entry(&tiff, ifd, tag, little));
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 100_000, "the class run shrank: {checked}");
    assert!(found > 500, "too few hits to be meaningful: {found}");
}
