//! Mutation-gap tests (W5-11b): the lower bound of accepted IFD offsets in `try_find_entry` (`ifd_offset < 8`).
use pr_imaging_raw::preview_selector::is_adobe_rgb_exif;

fn entry(tag: u16, typ: u16, count: u32, value: [u8; 4]) -> Vec<u8> {
    let mut e = Vec::new();
    e.extend_from_slice(&tag.to_le_bytes());
    e.extend_from_slice(&typ.to_le_bytes());
    e.extend_from_slice(&count.to_le_bytes());
    e.extend_from_slice(&value);
    e
}

#[test]
fn ifd0_at_offset_eight_is_accepted() {
    let mut p = b"Exif\0\0".to_vec();
    p.extend_from_slice(&[b'I', b'I', 42, 0, 8, 0, 0, 0]);
    p.extend_from_slice(&1u16.to_le_bytes());
    p.extend(entry(0x8769, 4, 1, 26u32.to_le_bytes()));
    p.extend_from_slice(&[0; 4]);
    p.extend_from_slice(&1u16.to_le_bytes());
    p.extend(entry(0xA005, 4, 1, 44u32.to_le_bytes()));
    p.extend_from_slice(&[0; 4]);
    p.extend_from_slice(&1u16.to_le_bytes());
    p.extend(entry(0x0001, 2, 4, *b"R03\0"));
    assert!(is_adobe_rgb_exif(&p));
}

#[test]
fn ifd0_at_offset_seven_overlapping_the_header_is_rejected() {
    let mut p = b"Exif\0\0".to_vec();
    p.extend_from_slice(&[b'I', b'I', 42, 0, 7, 0, 0, 0]);
    p.push(1); // tiff[8]: with tiff[7] == 0 the entry count read at offset 7 is 0x0100
    p.extend(entry(0x8769, 4, 1, 24u32.to_le_bytes())); // tiff[9..21]
    p.extend_from_slice(&[0; 3]); // tiff[21..24]
    p.extend_from_slice(&1u16.to_le_bytes()); // exif IFD at 24
    p.extend(entry(0xA005, 4, 1, 42u32.to_le_bytes()));
    p.extend_from_slice(&[0; 4]); // tiff[38..42]
    p.extend_from_slice(&1u16.to_le_bytes()); // interop IFD at 42
    p.extend(entry(0x0001, 2, 4, *b"R03\0"));
    assert!(!is_adobe_rgb_exif(&p));
}
