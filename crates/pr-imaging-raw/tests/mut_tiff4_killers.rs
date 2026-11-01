//! CL-MUT-TIFF4 mutant killers for `tiff_nav.rs` (inline bytes, no golden, no corpus; see
//! `docs/reviews/CL-MUT-TIFF4-2026-11-01.md`).

use pr_imaging_raw::InMemoryRawHeaderSource;
use pr_imaging_raw::tiff_nav::{
    TiffEntry, clamp_to_int, read_image_size, read_tag_unsigned_array, read_tag_value, starts_with_soi,
    try_read_jpeg_interchange, try_read_single_strip,
};

fn te(tag: u16, field_type: u16, count: u32, value_or_offset: u32) -> TiffEntry {
    TiffEntry {
        tag,
        field_type,
        count,
        value_or_offset,
    }
}

// tiff_nav.rs:176:22 `size * count` -> `size + count`: a LONG x 1 is exactly 4 bytes, so it is INLINE; with `+` it would be
// 5 and the value would be misread as an offset into the source.
#[test]
fn unsigned_array_long_count_one_is_inline_not_an_offset() {
    for little_endian in [true, false] {
        let mut source = InMemoryRawHeaderSource::new(vec![0xEE; 64]);
        let items = read_tag_unsigned_array(&mut source, &te(0x0100, 4, 1, 2), little_endian, 64).expect("read");
        assert_eq!(items, vec![2]);
    }
}

// tiff_nav.rs:176:22: also a BYTE x 4 (product 4, sum 8) stays inline.
#[test]
fn unsigned_array_four_inline_bytes_are_read_from_the_value_field() {
    let mut source = InMemoryRawHeaderSource::new(vec![0xEE; 64]);
    let items = read_tag_unsigned_array(&mut source, &te(0x0100, 1, 4, 0x0403_0201), true, 64).expect("read");
    assert_eq!(items, vec![1, 2, 3, 4]);
}

// tiff_nav.rs:258:27 `offsets.count != 1 || counts.count != 1` -> `&&`: a multi-strip image is refused when EITHER count is not 1.
#[test]
fn single_strip_is_refused_when_only_the_offsets_count_is_not_one() {
    let mut source = InMemoryRawHeaderSource::new(vec![0u8; 64]);
    let entries = [te(0x0111, 3, 2, 8), te(0x0117, 4, 1, 16)];
    assert_eq!(try_read_single_strip(&mut source, &entries, true).expect("read"), None);
}

#[test]
fn single_strip_is_refused_when_only_the_byte_counts_count_is_not_one() {
    let mut source = InMemoryRawHeaderSource::new(vec![0u8; 64]);
    let entries = [te(0x0111, 4, 1, 8), te(0x0117, 3, 2, 16)];
    assert_eq!(try_read_single_strip(&mut source, &entries, true).expect("read"), None);
}

#[test]
fn single_strip_with_both_counts_one_is_read() {
    let mut source = InMemoryRawHeaderSource::new(vec![0u8; 64]);
    let entries = [te(0x0111, 4, 1, 8), te(0x0117, 4, 1, 16)];
    assert_eq!(
        try_read_single_strip(&mut source, &entries, true).expect("read"),
        Some((8, 16))
    );
}

// tiff_nav.rs:400 `read_tag_value` (whole-function mutants; the C# parity cases for it live only in the golden test).
#[test]
fn read_tag_value_returns_the_first_entry_with_the_tag_and_none_when_absent() {
    let mut source = InMemoryRawHeaderSource::new(vec![0u8; 64]);
    let entries = [te(0x0101, 4, 1, 5), te(0x0100, 4, 1, 777), te(0x0100, 4, 1, 999)];
    assert_eq!(
        read_tag_value(&mut source, &entries, 0x0100, true).expect("read"),
        Some(777)
    );
    assert_eq!(
        read_tag_value(&mut source, &entries, 0x0101, true).expect("read"),
        Some(5)
    );
    assert_eq!(read_tag_value(&mut source, &entries, 0x0102, true).expect("read"), None);
}

// tiff_nav.rs:416 `read_image_size`: width from 0x0100, height from 0x0101, clamped, missing is 0.
#[test]
fn read_image_size_reads_width_and_height_from_their_own_tags() {
    let mut source = InMemoryRawHeaderSource::new(vec![0u8; 64]);
    let entries = [te(0x0100, 4, 1, 6000), te(0x0101, 4, 1, 4000)];
    assert_eq!(
        read_image_size(&mut source, &entries, true).expect("read"),
        (6000, 4000)
    );
    assert_eq!(
        read_image_size(&mut source, &entries[..1], true).expect("read"),
        (6000, 0)
    );
    assert_eq!(
        read_image_size(&mut source, &entries[1..], true).expect("read"),
        (0, 4000)
    );
    let huge = [te(0x0100, 4, 1, u32::MAX), te(0x0101, 9, 1, u32::MAX)];
    assert_eq!(read_image_size(&mut source, &huge, true).expect("read"), (i32::MAX, 0));
}

/// 64 zero bytes with a JPEG SOI (`FF D8`) at offset 16.
fn jpeg_source(with_soi: bool) -> InMemoryRawHeaderSource {
    let mut bytes = vec![0u8; 64];
    if with_soi {
        bytes[16] = 0xFF;
        bytes[17] = 0xD8;
    }
    InMemoryRawHeaderSource::new(bytes)
}

// tiff_nav.rs:431 `try_read_jpeg_interchange`.
#[test]
fn jpeg_interchange_with_a_soi_inside_the_file_is_returned() {
    let entries = [te(0x0201, 4, 1, 16), te(0x0202, 4, 1, 20)];
    assert_eq!(
        try_read_jpeg_interchange(&mut jpeg_source(true), &entries, true).expect("read"),
        Some((16, 20))
    );
}

#[test]
fn jpeg_interchange_without_a_soi_is_refused() {
    let entries = [te(0x0201, 4, 1, 16), te(0x0202, 4, 1, 20)];
    assert_eq!(
        try_read_jpeg_interchange(&mut jpeg_source(false), &entries, true).expect("read"),
        None
    );
}

#[test]
fn jpeg_interchange_missing_either_tag_is_refused() {
    let offset = te(0x0201, 4, 1, 16);
    let length = te(0x0202, 4, 1, 20);
    assert_eq!(
        try_read_jpeg_interchange(&mut jpeg_source(true), &[offset], true).expect("read"),
        None
    );
    assert_eq!(
        try_read_jpeg_interchange(&mut jpeg_source(true), &[length], true).expect("read"),
        None
    );
}

#[test]
fn jpeg_interchange_range_past_the_end_or_shorter_than_four_bytes_is_refused() {
    let past_end = [te(0x0201, 4, 1, 16), te(0x0202, 4, 1, 49)];
    assert_eq!(
        try_read_jpeg_interchange(&mut jpeg_source(true), &past_end, true).expect("read"),
        None
    );
    let exactly_to_end = [te(0x0201, 4, 1, 16), te(0x0202, 4, 1, 48)];
    assert_eq!(
        try_read_jpeg_interchange(&mut jpeg_source(true), &exactly_to_end, true).expect("read"),
        Some((16, 48))
    );
    let too_short = [te(0x0201, 4, 1, 16), te(0x0202, 4, 1, 3)];
    assert_eq!(
        try_read_jpeg_interchange(&mut jpeg_source(true), &too_short, true).expect("read"),
        None
    );
    let at_four = [te(0x0201, 4, 1, 16), te(0x0202, 4, 1, 4)];
    assert_eq!(
        try_read_jpeg_interchange(&mut jpeg_source(true), &at_four, true).expect("read"),
        Some((16, 4))
    );
}

// tiff_nav.rs:448/452 `starts_with_soi`: at least 4 bytes, inside the file, first two bytes exactly FF D8.
#[test]
fn starts_with_soi_needs_four_bytes_inside_the_file_and_the_exact_marker() {
    assert!(starts_with_soi(&mut jpeg_source(true), 16, 4).expect("read"));
    assert!(starts_with_soi(&mut jpeg_source(true), 16, 48).expect("read"));
    assert!(!starts_with_soi(&mut jpeg_source(true), 16, 3).expect("read"));
    assert!(!starts_with_soi(&mut jpeg_source(true), 16, 49).expect("read"));
    assert!(!starts_with_soi(&mut jpeg_source(true), 0, 20).expect("read"));
    assert!(!starts_with_soi(&mut jpeg_source(false), 16, 20).expect("read"));
    for (first, second) in [(0xFF, 0xD9), (0x00, 0xD8), (0xFE, 0xD8), (0xFF, 0xD7)] {
        let mut bytes = vec![0u8; 64];
        bytes[16] = first;
        bytes[17] = second;
        assert!(!starts_with_soi(&mut InMemoryRawHeaderSource::new(bytes), 16, 20).expect("read"));
    }
}

// tiff_nav.rs:493 `clamp_to_int`.
#[test]
fn clamp_to_int_keeps_positive_values_and_saturates() {
    assert_eq!(clamp_to_int(Some(5)), 5);
    assert_eq!(clamp_to_int(Some(i64::from(i32::MAX))), i32::MAX);
    assert_eq!(clamp_to_int(Some(i64::from(i32::MAX) + 1)), i32::MAX);
    assert_eq!(clamp_to_int(Some(0)), 0);
    assert_eq!(clamp_to_int(Some(-3)), 0);
    assert_eq!(clamp_to_int(None), 0);
}

// tiff_nav.rs:176:22 `size * count` -> `size / count`: SHORT x 4 is 8 bytes, out of line, read from the offset.
#[test]
fn unsigned_array_eight_bytes_are_out_of_line_and_read_from_the_offset() {
    let mut bytes = vec![0xEE; 64];
    bytes[16..24].copy_from_slice(&[1, 0, 2, 0, 3, 0, 4, 0]);
    let mut source = InMemoryRawHeaderSource::new(bytes);
    let items = read_tag_unsigned_array(&mut source, &te(0x0100, 3, 4, 16), true, 64).expect("read");
    assert_eq!(items, vec![1, 2, 3, 4]);
}
