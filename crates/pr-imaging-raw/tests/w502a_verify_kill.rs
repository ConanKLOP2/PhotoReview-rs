//! Verification-only mutant-killing tests for W5-02a (NOT part of the commit).
use pr_imaging_raw::tiff_nav::{
    TiffEntry, compute_exif_block, has_square_default_scale, read_ifd_entries, read_tag_unsigned_array,
    try_read_exif_pixel_dimensions,
};
use pr_imaging_raw::{RawHeaderError, RawHeaderSource};

const LE: bool = true;

struct Shorty {
    data: Vec<u8>,
    for_count: i32,
    drop: usize,
}

impl RawHeaderSource for Shorty {
    fn length(&self) -> i64 {
        self.data.len() as i64
    }
    fn read(&mut self, offset: i64, count: i32) -> Result<&[u8], RawHeaderError> {
        let o = offset as usize;
        let mut n = count as usize;
        if count == self.for_count {
            n -= self.drop;
        }
        Ok(&self.data[o..o + n])
    }
}

struct Failing(i64);
impl RawHeaderSource for Failing {
    fn length(&self) -> i64 {
        self.0
    }
    fn read(&mut self, _o: i64, _c: i32) -> Result<&[u8], RawHeaderError> {
        Err(RawHeaderError::invalid_data("budget exhausted"))
    }
}

fn entry(tag: u16, ty: u16, count: u32, val: u32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&tag.to_le_bytes());
    b.extend_from_slice(&ty.to_le_bytes());
    b.extend_from_slice(&count.to_le_bytes());
    b.extend_from_slice(&val.to_le_bytes());
    b
}

#[test]
fn k66_count_read_at_len_minus_two_reaches_the_source() {
    let mut s = Failing(1_000_000);
    assert!(read_ifd_entries(&mut s, 999_998, LE).is_err());
}

#[test]
fn k71_short_count_read_ends_chain() {
    let mut data = vec![0u8; 32];
    data[10..14].copy_from_slice(&16u32.to_le_bytes());
    let mut s = Shorty {
        data,
        for_count: 2,
        drop: 1,
    };
    let r = read_ifd_entries(&mut s, 8, LE).unwrap();
    assert!(r.entries.is_empty());
    assert_eq!(r.next_ifd_offset, 0);
}

#[test]
fn k78_overshoot_by_less_than_five_bytes_is_clamped() {
    let mut data = vec![0u8; 32];
    data[8..10].copy_from_slice(&2u16.to_le_bytes());
    data[10..22].copy_from_slice(&entry(0x0100, 3, 1, 5));
    let mut s = Shorty {
        data,
        for_count: -1,
        drop: 0,
    };
    let r = read_ifd_entries(&mut s, 8, LE).unwrap();
    assert_eq!(r.entries.len(), 1);
}

#[test]
fn k94_short_entries_read_drops_partial_entry() {
    let mut data = vec![0u8; 64];
    data[8..10].copy_from_slice(&1u16.to_le_bytes());
    data[10..22].copy_from_slice(&entry(0x0100, 3, 1, 5));
    let mut s = Shorty {
        data,
        for_count: 12,
        drop: 1,
    };
    let r = read_ifd_entries(&mut s, 8, LE).unwrap();
    assert!(r.entries.is_empty());
}

#[test]
fn k172_max_items_zero_never_reads() {
    let e = TiffEntry {
        tag: 1,
        field_type: 3,
        count: 10,
        value_or_offset: 100,
    };
    let mut s = Failing(1_000_000);
    assert_eq!(read_tag_unsigned_array(&mut s, &e, LE, 0).unwrap(), Vec::<i64>::new());
}

#[test]
fn k215_extent_exactly_at_eof_beyond_default_block_grows() {
    let len = 200_000usize;
    let mut data = vec![0u8; len];
    data[8..10].copy_from_slice(&1u16.to_le_bytes());
    let n = 100u32;
    data[10..22].copy_from_slice(&entry(0x010F, 2, n, (len as u32) - n));
    let mut s = Shorty {
        data,
        for_count: -1,
        drop: 0,
    };
    let b = compute_exif_block(&mut s, LE, 8).unwrap();
    assert_eq!(b.length, 200_000);
}

#[test]
fn k293_dimension_of_one_is_accepted() {
    let mut data = vec![0u8; 64];
    data[8..10].copy_from_slice(&2u16.to_le_bytes());
    data[10..22].copy_from_slice(&entry(0xA002, 3, 1, 1));
    data[22..34].copy_from_slice(&entry(0xA003, 3, 1, 1));
    let mut s = Shorty {
        data,
        for_count: -1,
        drop: 0,
    };
    assert_eq!(try_read_exif_pixel_dimensions(&mut s, 8, LE).unwrap(), Some((1, 1)));
}

#[test]
fn k361_zero_pair_is_not_square() {
    for ratio in [[0u32, 0, 1, 1], [0, 1, 0, 1], [1, 0, 1, 0], [1, 1, 0, 0]] {
        let mut data = vec![0u8; 64];
        for (i, v) in ratio.iter().enumerate() {
            data[16 + 4 * i..20 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        let entries = vec![TiffEntry {
            tag: 0xC61E,
            field_type: 5,
            count: 2,
            value_or_offset: 16,
        }];
        let mut s = Shorty {
            data,
            for_count: -1,
            drop: 0,
        };
        assert!(!has_square_default_scale(&mut s, &entries, LE).unwrap(), "{ratio:?}");
    }
}

#[test]
fn k512_short_array_read_stops_at_whole_items() {
    let mut data = vec![0u8; 64];
    for i in 0..4u16 {
        data[16 + 2 * i as usize..18 + 2 * i as usize].copy_from_slice(&(i + 1).to_le_bytes());
    }
    let e = TiffEntry {
        tag: 1,
        field_type: 3,
        count: 4,
        value_or_offset: 16,
    };
    let mut s = Shorty {
        data,
        for_count: 8,
        drop: 6,
    };
    assert_eq!(read_tag_unsigned_array(&mut s, &e, LE, 64).unwrap(), vec![1]);
}
