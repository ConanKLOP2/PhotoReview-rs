//! N4 (2026-11-05): run-based equivalence proofs for the nine survivors of `preview_selector.rs` shard 1/2:
//! `walk_jpeg_header` 336:49, 347:51, 348:50 (`|` -> `^`), `is_adobe_rgb_exif` 376:22 (`<` -> `<=`), 400:18 (`+` -> `-`),
//! `try_find_entry` 421:37 (`+` -> `-`), 421:41 (`>` -> `==` and `>=`), 427:18 (`+` -> `-`).
//! Inline bytes only: no golden file, no corpus, no sleep. Each test passes on the original and under the mutant it names.
//!
//! Domains: the three `|` -> `^` mutants are proven over their WHOLE domain (all 65 536 high/low byte pairs, through the real
//! walk). The six comparison mutants are proven against an independent reference implementation over a structured
//! enumeration (every truncation length, every pointer value 0..=len+4 and the u32 edges, every entry count 0..=8, seeded
//! random edits); the full byte domain of an arbitrary payload is not enumerable, so those six are "run-supported" only.

use pr_imaging_raw::InMemoryRawHeaderSource;
use pr_imaging_raw::preview_selector::{is_adobe_rgb_exif, walk_jpeg_header};
use pr_test_support::raw::XorShift64;

/// SOI + one SOF0 segment (length 17: precision, height, width, 3 components) with the given size bytes.
fn sof0_jpeg(height: u16, width: u16) -> Vec<u8> {
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
    jpeg.extend_from_slice(&height.to_be_bytes());
    jpeg.extend_from_slice(&width.to_be_bytes());
    jpeg.extend_from_slice(&[0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
    jpeg
}

fn frame_of(jpeg: Vec<u8>) -> Option<(i32, i32)> {
    let length = jpeg.len() as i64;
    let mut source = InMemoryRawHeaderSource::new(jpeg);
    walk_jpeg_header(&mut source, 0, length, true)
        .expect("in-memory source")
        .frame
}

/// preview_selector.rs:347:51 and 348:50 (`(h1 << 8) | h0`, `(w1 << 8) | w0`, `|` -> `^`): the operands occupy disjoint bits, so
/// the two operators agree. Proven over every 16-bit height (width fixed) and every 16-bit width (height fixed) through the
/// real frame walk: the frame size is exactly the written size, and a zero side gives no frame.
#[test]
fn every_sixteen_bit_frame_height_and_width_is_read_back_exactly() {
    for height in 0..=u16::MAX {
        let expected = if height == 0 {
            None
        } else {
            Some((640, i32::from(height)))
        };
        assert_eq!(frame_of(sof0_jpeg(height, 640)), expected, "height {height}");
    }
    for width in 0..=u16::MAX {
        let expected = if width == 0 {
            None
        } else {
            Some((i32::from(width), 480))
        };
        assert_eq!(frame_of(sof0_jpeg(480, width)), expected, "width {width}");
    }
}

/// preview_selector.rs:336:49 (`(head[2] << 8) | head[3]`, `|` -> `^`): disjoint bits again. Proven over every segment length
/// 0..=65 535 of an APP0 segment that precedes the SOF0: the frame is found exactly when the segment length is at least 2, at
/// the position the length says.
#[test]
fn every_sixteen_bit_segment_length_is_followed_to_the_next_marker() {
    let tail = sof0_jpeg(480, 640);
    let sof = &tail[2..];
    for length in 0..=u16::MAX {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpeg.extend_from_slice(&length.to_be_bytes());
        if length >= 2 {
            jpeg.resize(4 + usize::from(length), 0);
            jpeg.extend_from_slice(sof);
        }
        let expected = if length >= 2 { Some((640, 480)) } else { None };
        assert_eq!(frame_of(jpeg), expected, "segment length {length}");
    }
}

// ---------------------------------------------------------------------------------------------- is_adobe_rgb_exif

fn rd16(t: &[u8], o: i64, le: bool) -> u16 {
    match usize::try_from(o).ok().and_then(|s| t.get(s..s.checked_add(2)?)) {
        Some(b) if le => u16::from_le_bytes([b[0], b[1]]),
        Some(b) => u16::from_be_bytes([b[0], b[1]]),
        None => 0,
    }
}

fn rd32(t: &[u8], o: i64, le: bool) -> u32 {
    match usize::try_from(o).ok().and_then(|s| t.get(s..s.checked_add(4)?)) {
        Some(b) if le => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        Some(b) => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        None => 0,
    }
}

/// First entry with `tag` in the IFD at `ifd`, written from the C# `TryFindEntry` contract.
fn ref_find(t: &[u8], ifd: i64, tag: u16, le: bool) -> Option<i64> {
    let len = t.len() as i64;
    if ifd < 8 || ifd + 2 > len {
        return None;
    }
    for i in 0..i64::from(rd16(t, ifd, le)) {
        let e = ifd + 2 + 12 * i;
        if e + 12 > len {
            return None;
        }
        if rd16(t, e, le) == tag {
            return Some(e);
        }
    }
    None
}

fn ref_long(t: &[u8], ifd: i64, tag: u16, le: bool) -> Option<i64> {
    let e = ref_find(t, ifd, tag, le)?;
    (rd16(t, e + 2, le) == 4 && rd32(t, e + 4, le) == 1).then(|| i64::from(rd32(t, e + 8, le)))
}

fn reference(p: &[u8]) -> bool {
    if p.len() < 14 || !p.starts_with(b"Exif\0\0") {
        return false;
    }
    let t = &p[6..];
    let le = match (t[0], t[1]) {
        (b'I', b'I') => true,
        (b'M', b'M') => false,
        _ => return false,
    };
    if rd16(t, 2, le) != 42 {
        return false;
    }
    let Some(exif) = ref_long(t, i64::from(rd32(t, 4, le)), 0x8769, le) else {
        return false;
    };
    let Some(interop) = ref_long(t, exif, 0xA005, le) else {
        return false;
    };
    let Some(e) = ref_find(t, interop, 0x0001, le) else {
        return false;
    };
    rd16(t, e + 2, le) == 2
        && rd32(t, e + 4, le) == 4
        && e + 12 <= t.len() as i64
        && usize::try_from(e + 8)
            .ok()
            .and_then(|s| t.get(s..s + 4))
            .is_some_and(|b| b == b"R03\0")
}

/// A genuine Adobe RGB payload: IFD0 at 8 -> Exif IFD at 26 -> Interop IFD at 44 -> entry 0x0001 ASCII x4 "R03\0".
fn adobe_payload(le: bool) -> Vec<u8> {
    let u16b = |v: u16| if le { v.to_le_bytes() } else { v.to_be_bytes() };
    let u32b = |v: u32| if le { v.to_le_bytes() } else { v.to_be_bytes() };
    let mut t = vec![if le { b'I' } else { b'M' }; 2];
    t.extend_from_slice(&u16b(42));
    t.extend_from_slice(&u32b(8));
    for (tag, ty, count, value) in [(0x8769u16, 4u16, 1u32, 26u32), (0xA005, 4, 1, 44)] {
        t.extend_from_slice(&u16b(1));
        t.extend_from_slice(&u16b(tag));
        t.extend_from_slice(&u16b(ty));
        t.extend_from_slice(&u32b(count));
        t.extend_from_slice(&u32b(value));
        t.extend_from_slice(&u32b(0));
    }
    t.extend_from_slice(&u16b(1));
    t.extend_from_slice(&u16b(1));
    t.extend_from_slice(&u16b(2));
    t.extend_from_slice(&u32b(4));
    t.extend_from_slice(b"R03\0");
    t.extend_from_slice(&u32b(0));
    let mut p = b"Exif\0\0".to_vec();
    p.extend_from_slice(&t);
    p
}

fn agree(p: &[u8], what: &str) {
    assert_eq!(
        is_adobe_rgb_exif(p),
        reference(p),
        "{what}, {} bytes: {p:02X?}",
        p.len()
    );
}

#[test]
fn the_reference_accepts_the_genuine_payloads_so_the_differential_below_is_not_vacuous() {
    for le in [true, false] {
        let p = adobe_payload(le);
        assert!(reference(&p));
        assert!(is_adobe_rgb_exif(&p));
    }
}

/// 376:22 `payload.len() < 14` -> `<= 14`. At exactly 14 bytes the TIFF part is the 8-byte header; every IFD pointer then lies at
/// `>= 8`, and `ifd + 2 > 8` ends `try_find_entry`, so the original is `false` too. Run for both byte orders over every IFD0
/// pointer 0..=2^20 and the u32 edges (the whole 2^32 pointer range is not enumerated).
#[test]
fn a_fourteen_byte_payload_is_never_adobe_rgb() {
    for le in [true, false] {
        let pointers = (0..=(1u32 << 20)).chain([1 << 24, 1 << 31, u32::MAX - 1, u32::MAX]);
        for ifd0 in pointers {
            let mut p = b"Exif\0\0".to_vec();
            p.extend_from_slice(if le { b"II" } else { b"MM" });
            p.extend_from_slice(&if le { 42u16.to_le_bytes() } else { 42u16.to_be_bytes() });
            p.extend_from_slice(&if le { ifd0.to_le_bytes() } else { ifd0.to_be_bytes() });
            assert_eq!(p.len(), 14);
            assert!(!is_adobe_rgb_exif(&p), "ifd0 {ifd0} little {le}");
        }
    }
}

/// 400:18, 421:37, 421:41, 427:18: every truncation length of the genuine payloads.
#[test]
fn every_truncation_of_a_genuine_payload_agrees_with_the_reference() {
    for le in [true, false] {
        let full = adobe_payload(le);
        for cut in 0..=full.len() {
            agree(&full[..cut], "truncation");
        }
    }
}

/// 421:37, 421:41: every IFD pointer (IFD0 at tiff 4, Exif pointer at 8+2+8, interop pointer at 26+2+8) set to every value in
/// 0..=len+4 and to the u32 edges, for every truncation length of the TIFF tail past the pointer.
#[test]
fn every_ifd_pointer_value_near_the_end_of_the_payload_agrees_with_the_reference() {
    for le in [true, false] {
        let base = adobe_payload(le);
        let pointer_positions = [6 + 4, 6 + 8 + 2 + 8, 6 + 26 + 2 + 8];
        for position in pointer_positions {
            for cut in (14..=base.len()).rev() {
                let mut candidates: Vec<u32> = (0..=(base.len() as u32 + 4)).collect();
                candidates.extend([1 << 16, 1 << 24, 1 << 31, u32::MAX - 1, u32::MAX]);
                for value in candidates {
                    let mut p = base[..cut].to_vec();
                    if position + 4 > p.len() {
                        continue;
                    }
                    let bytes = if le { value.to_le_bytes() } else { value.to_be_bytes() };
                    p[position..position + 4].copy_from_slice(&bytes);
                    agree(&p, "pointer edit");
                }
            }
        }
    }
}

/// 400:18, 427:18: every entry count 0..=8 and 0xFFFF of the three IFDs, at every truncation length.
#[test]
fn every_small_entry_count_agrees_with_the_reference() {
    for le in [true, false] {
        let base = adobe_payload(le);
        for position in [6 + 8, 6 + 26, 6 + 44] {
            for cut in 14..=base.len() {
                for count in (0..=8u16).chain([u16::MAX]) {
                    let mut p = base[..cut].to_vec();
                    if position + 2 > p.len() {
                        continue;
                    }
                    let bytes = if le { count.to_le_bytes() } else { count.to_be_bytes() };
                    p[position..position + 2].copy_from_slice(&bytes);
                    agree(&p, "count edit");
                }
            }
        }
    }
}

/// All mutants of this group: 200 000 seeded random byte edits (1 to 3 bytes) of the genuine payloads, cut at a random length.
#[test]
fn seeded_random_edits_of_a_genuine_payload_agree_with_the_reference() {
    let mut rng = XorShift64::new(0x4E34_5E1E);
    for round in 0..200_000usize {
        let le = round % 2 == 0;
        let mut p = adobe_payload(le);
        for _ in 0..=rng.below(3) {
            let index = rng.below(p.len());
            p[index] = (rng.next_u64() & 0xFF) as u8;
        }
        let cut = 14 + rng.below(p.len() - 13);
        p.truncate(cut);
        agree(&p, "random edit");
    }
}
