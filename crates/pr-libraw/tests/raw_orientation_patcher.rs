//! Synthetic, native-independent tests for the W5-16c `RawOrientationPatcher` test helper.

mod support;

use pr_imaging_raw::{EmbeddedPreview, EmbeddedPreviewKind, PreviewColorSpace, RawContainerInfo, RawFormat};
use support::raw_orientation_patcher::{
    try_find_cr3_cmt1_tiff, try_rename_ifd0_tag, try_write_orientation, try_write_tiff_orientation,
};

fn empty_info() -> RawContainerInfo {
    RawContainerInfo {
        format: RawFormat::Unknown,
        sensor_width: 0,
        sensor_height: 0,
        orientation: 1,
        previews: Vec::new(),
        exif_blocks: Vec::new(),
    }
}

fn info_with_preview(offset: i64) -> RawContainerInfo {
    RawContainerInfo {
        previews: vec![EmbeddedPreview::new(
            0,
            offset,
            0,
            EmbeddedPreviewKind::Jpeg,
            0,
            0,
            PreviewColorSpace::Unknown,
        )],
        ..empty_info()
    }
}

fn tiff(little: bool, orientation: u16) -> Vec<u8> {
    let mut bytes = vec![0u8; 26];
    bytes[..2].copy_from_slice(if little { b"II" } else { b"MM" });
    put_u16(&mut bytes, 2, 42, little);
    put_u32(&mut bytes, 4, 8, little);
    put_u16(&mut bytes, 8, 1, little);
    put_u16(&mut bytes, 10, 0x0112, little);
    put_u16(&mut bytes, 12, 3, little); // SHORT
    put_u32(&mut bytes, 14, 1, little);
    put_u16(&mut bytes, 18, orientation, little);
    put_u32(&mut bytes, 22, 0, little); // no next IFD
    bytes
}

fn jpeg_with_exif(little: bool, orientation: u16) -> Vec<u8> {
    let tiff = tiff(little, orientation);
    let segment_len = u16::try_from(2 + 6 + tiff.len()).expect("synthetic APP1 length fits u16");
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
    jpeg.extend_from_slice(&segment_len.to_be_bytes());
    jpeg.extend_from_slice(b"Exif\0\0");
    jpeg.extend_from_slice(&tiff);
    jpeg.extend_from_slice(&[0xFF, 0xD9]);
    jpeg
}

fn read_orientation(file: &[u8], tiff_start: usize, little: bool) -> u16 {
    let at = tiff_start + 18;
    let pair = [file[at], file[at + 1]];
    if little {
        u16::from_le_bytes(pair)
    } else {
        u16::from_be_bytes(pair)
    }
}

fn put_u16(bytes: &mut [u8], at: usize, value: u16, little: bool) {
    let pair = if little {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    };
    bytes[at..at + 2].copy_from_slice(&pair);
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32, little: bool) {
    let quad = if little {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    };
    bytes[at..at + 4].copy_from_slice(&quad);
}

#[test]
// C#: LibRawDecoderTests.ReadInfo_PortraitRewrittenCorpusFile_MatchesDecodedOrientedDimensions (TIFF-at-zero route).
fn try_write_orientation_rewrites_tiff_at_offset_zero() {
    let mut bytes = tiff(true, 1);
    assert!(try_write_orientation(&mut bytes, ".nef", &empty_info(), 6));
    assert_eq!(read_orientation(&bytes, 0, true), 6);
}

#[test]
// C#: LibRawDecoderTests.ReadInfo_PortraitRewrittenCorpusFile_MatchesDecodedOrientedDimensions (RAF embedded-EXIF route).
fn try_write_orientation_rewrites_raf_embedded_jpeg_exif() {
    let mut bytes = vec![0xA5; 13];
    let jpeg_start = bytes.len();
    bytes.extend_from_slice(&jpeg_with_exif(false, 1));
    let info = info_with_preview(jpeg_start as i64);

    assert!(try_write_orientation(&mut bytes, ".raf", &info, 8));
    assert_eq!(read_orientation(&bytes, jpeg_start + 12, false), 8);
}

#[test]
// C#: LibRawDecoderTests.ReadInfo_PortraitRewrittenCorpusFile_MatchesDecodedOrientedDimensions (RW2 EXIF fallback route).
fn try_write_orientation_rw2_falls_back_to_embedded_jpeg_exif_when_tiff_has_no_tag() {
    let mut bytes = tiff(true, 1);
    assert!(try_rename_ifd0_tag(&mut bytes, 0, 0x0112, 0xC001));
    let jpeg_start = bytes.len();
    bytes.extend_from_slice(&jpeg_with_exif(true, 1));
    let info = info_with_preview(jpeg_start as i64);

    assert!(try_write_orientation(&mut bytes, ".rw2", &info, 3));
    assert_eq!(read_orientation(&bytes, jpeg_start + 12, true), 3);
    assert!(!try_write_tiff_orientation(&mut bytes, 0, 4));
}

#[test]
// C#: LibRawDecoderTests.ReadInfo_PortraitRewrittenCorpusFile_MatchesDecodedOrientedDimensions (CR3 CMT1 route).
fn try_write_orientation_cr3_uses_cmt1_tiff_payload() {
    let mut bytes = vec![0, 0, 0, 30]; // preceding ISO-BMFF box bytes; CMT1 begins at offset 4
    bytes.extend_from_slice(b"CMT1");
    let tiff_start = bytes.len();
    bytes.extend_from_slice(&tiff(false, 1));
    let info = empty_info();

    assert_eq!(try_find_cr3_cmt1_tiff(&bytes), Some(tiff_start));
    assert!(try_write_orientation(&mut bytes, ".cr3", &info, 6));
    assert_eq!(read_orientation(&bytes, tiff_start, false), 6);
}

#[test]
// C#: LibRawDecoderTests.ReadInfo_PortraitRewrittenCorpusFile_MatchesDecodedOrientedDimensions (invalid CMT1 guard).
fn try_find_cr3_cmt1_tiff_rejects_missing_or_invalid_marker_context() {
    assert_eq!(try_find_cr3_cmt1_tiff(b"no CMT1 block"), None);

    let mut invalid_endian = vec![0; 4];
    invalid_endian.extend_from_slice(b"CMT1XX");
    invalid_endian.extend_from_slice(&[0; 6]);
    assert_eq!(try_find_cr3_cmt1_tiff(&invalid_endian), None);
}

#[test]
fn try_write_orientation_fails_closed_for_truncated_exif_and_invalid_preview_offsets() {
    // C#'s AsSpan(offset + 4, 6) throws on this truncated APP1 payload; this test helper returns false (D60).
    let truncated_app1 = [0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x08, b'E'];
    assert!(!try_write_orientation(
        &mut truncated_app1.to_vec(),
        ".raf",
        &info_with_preview(0),
        6
    ));

    let mut bytes = jpeg_with_exif(true, 1);
    assert!(!try_write_orientation(&mut bytes, ".raf", &info_with_preview(-1), 6));
    assert!(!try_write_orientation(
        &mut bytes,
        ".raf",
        &info_with_preview(i64::MAX),
        6
    ));
}
