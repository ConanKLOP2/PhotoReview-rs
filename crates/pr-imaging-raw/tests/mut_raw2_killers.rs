//! Synthetic-byte regressions for CX-RAWMUT1; no RAW corpus or golden files.

use pr_imaging_raw::registry::{adobe_rgb_profile, ensure_adobe_rgb_profile, has_embedded_icc};

// registry.rs:135:19, `<` -> `<=`: a bare SOI must receive the complete ICC segment.
#[test]
fn ensure_adobe_rgb_profile_bare_soi_receives_complete_profile() {
    let result = ensure_adobe_rgb_profile(vec![0xff, 0xd8]).expect("bare SOI is accepted");
    assert_eq!(&result[..6], &[0xff, 0xd8, 0xff, 0xe2, 0x01, 0x86]);
    assert_eq!(&result[20..], adobe_rgb_profile());
}

fn icc_segment(marker: u8, length: u16, identifier: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xff, marker];
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(identifier);
    bytes
}

// registry.rs:204:27, `||` -> `&&`: bytes after SOS/EOI must never be scanned as APP2.
#[test]
fn has_embedded_icc_does_not_scan_length_shaped_data_after_sos_or_eoi() {
    for marker in [0xda, 0xd9] {
        let mut bytes = vec![0xff, 0xd8, 0xff, marker, 0, 2];
        bytes.extend_from_slice(&icc_segment(0xe2, 14, b"ICC_PROFILE\0"));
        assert!(!has_embedded_icc(&bytes), "stop at marker {marker:x}");
    }
}

// registry.rs:196-227: progress arithmetic, standalone-marker ||/==, segment bounds,
// APP2 &&/==/>= and identifier equality. Inline bytes keep these checks independent of goldens.
#[test]
fn has_embedded_icc_walks_standalone_markers_and_validates_segment_bounds() {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let icc = icc_segment(0xe2, 14, b"ICC_PROFILE\0");
        for marker in [0xd8, 0x01, 0xd0, 0xd7] {
            let mut bytes = vec![0xff, 0xd8, 0xff, 0xff, marker];
            bytes.extend_from_slice(&icc);
            assert!(has_embedded_icc(&bytes), "standalone marker {marker:x}");
        }
        for marker in [0xda, 0xd9] {
            let mut bytes = vec![0xff, 0xd8, 0xff, marker];
            bytes.extend_from_slice(&icc);
            assert!(!has_embedded_icc(&bytes), "stop at {marker:x}");
        }
        for length in [2_u16, 3, 16, 256] {
            let mut bytes = vec![0xff, 0xd8];
            bytes.extend_from_slice(&icc_segment(0xe1, length, &vec![0; usize::from(length) - 2]));
            bytes.extend_from_slice(&icc);
            assert!(has_embedded_icc(&bytes), "skip segment length {length}");
        }
        for (marker, length, identifier, expected) in [
            (0xe2, 14, &b"ICC_PROFILE\0"[..], true),
            (0xe2, 15, &b"ICC_PROFILE\0X"[..], true),
            (0xe2, 13, &b"ICC_PROFILE"[..], false),
            (0xe2, 14, &b"ICC_PROFILEX"[..], false),
            (0xe1, 14, &b"ICC_PROFILE\0"[..], false),
            (0xe2, 15, &b"ICC_PROFILE\0"[..], false),
            (0xe2, 0, &b"ICC_PROFILE\0"[..], false),
            (0xe2, 1, &b"ICC_PROFILE\0"[..], false),
        ] {
            let mut bytes = vec![0xff, 0xd8];
            bytes.extend_from_slice(&icc_segment(marker, length, identifier));
            assert_eq!(has_embedded_icc(&bytes), expected, "marker {marker:x}, length {length}");
        }
        send.send(()).expect("receiver remains live");
    });
    receive
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("ICC walker must terminate and all byte cases must pass");
}

// readers/cr2.rs:123:34, `>` -> `>=`: zero ends the chain before cancellation is checked.
#[test]
fn cr2_zero_first_ifd_offset_ends_the_chain_before_cancellation_check() {
    use pr_core::sync::CancelToken;
    use pr_imaging_raw::readers::cr2::Cr2ContainerReader;
    use pr_imaging_raw::{InMemoryRawHeaderSource, RawContainerReader, RawFormat};
    let mut bytes = vec![0; 16];
    bytes[..4].copy_from_slice(b"II*\0");
    bytes[8..10].copy_from_slice(b"CR");
    let mut source = InMemoryRawHeaderSource::new(bytes);
    let token = CancelToken::new();
    token.cancel();
    let info = Cr2ContainerReader::new()
        .read(&mut source, &token)
        .expect("zero IFD pointer terminates the chain");
    assert_eq!(info.format, RawFormat::Cr2);
    assert_eq!((info.sensor_width, info.sensor_height, info.orientation), (0, 0, 1));
    assert!(info.previews.is_empty());
}
