//! CL-MUT-SEL2 killers for `preview_selector.rs` (shard 1/2). In-memory bytes only: no golden files, no RAW corpus.

use pr_imaging_raw::InMemoryRawHeaderSource;
use pr_imaging_raw::preview_selector::{is_adobe_rgb_exif, walk_jpeg_header};

/// SOI, `restarts` standalone `FF D0` markers (each consumes one segment iteration), then a baseline SOF0 segment.
fn jpeg_with_standalone_markers(restarts: usize, width: u16, height: u16) -> Vec<u8> {
    let mut jpeg = vec![0xFF, 0xD8];
    for _ in 0..restarts {
        jpeg.extend_from_slice(&[0xFF, 0xD0]);
    }
    jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]);
    jpeg.extend_from_slice(&height.to_be_bytes());
    jpeg.extend_from_slice(&width.to_be_bytes());
    jpeg.extend_from_slice(&[0x01, 0x01, 0x11, 0x00, 0xFF, 0xD9]);
    jpeg
}

// preview_selector.rs:333:21 `+=` -> `-=` / `*=` on `segment` in `walk_jpeg_header`: the walk visits at most
// MAX_JPEG_MARKER_SEGMENTS (512) marker segments, so a frame header behind 600 standalone markers is not found.
#[test]
fn a_frame_behind_more_than_the_segment_limit_is_not_found() {
    let jpeg = jpeg_with_standalone_markers(600, 640, 480);
    let len = jpeg.len() as i64;
    let mut source = InMemoryRawHeaderSource::new(jpeg);
    let walk = walk_jpeg_header(&mut source, 0, len, true).unwrap();
    assert_eq!(walk.frame, None, "more than 512 marker segments must stop the walk");
}

#[test]
fn a_frame_behind_fewer_than_the_segment_limit_is_found() {
    let jpeg = jpeg_with_standalone_markers(400, 640, 480);
    let len = jpeg.len() as i64;
    let mut source = InMemoryRawHeaderSource::new(jpeg);
    let walk = walk_jpeg_header(&mut source, 0, len, true).unwrap();
    assert_eq!(walk.frame, Some((640, 480)));
}

// preview_selector.rs:376:27 `||` -> `&&` and 376:22 `<` -> `==` in `is_adobe_rgb_exif`: a payload shorter than 14 bytes is
// refused before the TIFF header is indexed (a bare "Exif\0\0" prefix has no TIFF bytes at all; the mutants would index it).
#[test]
fn a_payload_shorter_than_fourteen_bytes_is_refused_without_panicking() {
    assert!(!is_adobe_rgb_exif(b"Exif\0\0"));
    assert!(!is_adobe_rgb_exif(b"Exif\0\0I"));
    assert!(!is_adobe_rgb_exif(b"Exif\0\0II*\0\x08\0\0"));
}
