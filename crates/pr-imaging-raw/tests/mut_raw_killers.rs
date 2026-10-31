//! Killers for mutants that survived the `cl-mut-raw` run (synthetic data only, no corpus, no sleeps).

use pr_imaging_raw::registry::RawContainerReaderRegistry;

/// `registry.rs` `Debug::fmt` replaced by `Ok(Default::default())` printed nothing: pin the type name and reader count.
#[test]
fn registry_debug_prints_type_name_and_reader_count() {
    let text = format!("{:?}", RawContainerReaderRegistry::new());
    assert_eq!(text, "RawContainerReaderRegistry { reader_count: 8 }");
    let empty = format!("{:?}", RawContainerReaderRegistry::with_readers([]));
    assert_eq!(empty, "RawContainerReaderRegistry { reader_count: 0 }");
}

use pr_imaging_raw::registry::{ICC_IDENTIFIER, ensure_adobe_rgb_profile, has_embedded_icc};

/// A JPEG-like buffer: `soi`, then an APP2 segment carrying the ICC identifier (valid walk for `has_embedded_icc`).
fn icc_tagged(soi: [u8; 2]) -> Vec<u8> {
    let mut bytes = soi.to_vec();
    bytes.extend_from_slice(&[0xFF, 0xE2, 0x00, 0x10]);
    bytes.extend_from_slice(&ICC_IDENTIFIER);
    bytes.extend_from_slice(&[1, 1]);
    bytes
}

/// `has_embedded_icc`: the SOI guard must reject a wrong first byte OR a wrong second byte on its own
/// (`||` -> `&&` mutants accepted either half alone and walked into the tagged APP2 segment).
#[test]
fn has_embedded_icc_requires_both_soi_bytes() {
    assert!(
        has_embedded_icc(&icc_tagged([0xFF, 0xD8])),
        "control: a real SOI is tagged"
    );
    assert!(!has_embedded_icc(&icc_tagged([0x00, 0xD8])), "wrong first SOI byte");
    assert!(!has_embedded_icc(&icc_tagged([0xFF, 0x00])), "wrong second SOI byte");
}

/// `ensure_adobe_rgb_profile`: same guard, but an untagged non-JPEG must be an error, not a rewritten stream.
#[test]
fn ensure_adobe_rgb_profile_rejects_each_wrong_soi_byte() {
    for soi in [[0x00_u8, 0xD8], [0xFF, 0x00], [0x00, 0x00]] {
        let mut jpeg = soi.to_vec();
        jpeg.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        let err = ensure_adobe_rgb_profile(jpeg).expect_err("not a JPEG");
        assert!(err.to_string().contains("not a valid JPEG"), "{err}");
    }
    assert!(ensure_adobe_rgb_profile(vec![0xFF]).is_err());
    assert!(ensure_adobe_rgb_profile(vec![0xFF, 0xD8, 0xFF, 0xD9]).is_ok());
    // Exactly two bytes (bare SOI) is a valid stream: kills `len < 2` -> `len <= 2`.
    let two = ensure_adobe_rgb_profile(vec![0xFF, 0xD8]).expect("bare SOI is accepted");
    assert_eq!(&two[..4], &[0xFF, 0xD8, 0xFF, 0xE2]);
}

/// `has_embedded_icc` skips a run of `FF` fill bytes before the marker. The `+=` -> `-=` mutant never leaves the run
/// (an endless loop, a timeout rather than a failure), so the walk runs on a worker thread and a bounded wait turns the
/// hang into an assertion failure. The normal path returns at once.
#[test]
fn has_embedded_icc_skips_ff_fill_bytes_before_the_marker() {
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xFF, 0xFF, 0xE2, 0x00, 0x10];
    jpeg.extend_from_slice(&ICC_IDENTIFIER);
    jpeg.extend_from_slice(&[1, 1]);
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(has_embedded_icc(&jpeg));
    });
    let tagged = receiver
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("has_embedded_icc must return (fill-byte skip terminates)");
    assert!(tagged, "APP2 ICC segment after FF fill bytes is found");
}
