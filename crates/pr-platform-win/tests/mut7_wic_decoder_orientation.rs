//! MUT7 (2026-11-06): `wic/decoder.rs:232:38`, `request.apply_orientation && orientation > 1` -> `||`.
//!
//! The mutant differs from the original in two cases only:
//! (a) `apply_orientation == true`, `orientation <= 1`: `map_to_transform_options` gives `Rotate0` for every such value (proven over all 2^32 - 7 `i32`
//!     values by `mut3_wic_decoder_proofs`), and no rotator is created for `Rotate0`;
//! (b) `apply_orientation == false`, `orientation > 1`: with `apply_orientation == false` the orientation of `decode_from_stream` comes from
//!     `read_frame_metadata(.., false, ..)` (a caller-given `source_orientation` is used only together with `apply_orientation == true`), and that reads
//!     no tag, so it is 1 for every file and every request.
//! Case (b) cannot be reached with any input, so no test can tell the mutant from the original (EQUIVALENT). These tests run the real WIC decode over every
//! orientation fixture (EXIF tag 0 to 9) and every `source_orientation` class (none, in range, out of range, negative, `i32::MAX`) with
//! `apply_orientation == false` and assert the observable consequence of case (b) not existing: stored size, orientation 1, no swap of the original
//! size. They also assert that the tags ARE in the files (the same fixtures rotate with `apply_orientation == true`).
#![cfg(windows)]

use std::path::PathBuf;
use std::sync::Arc;

use pr_core::abstractions::SourceReader;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView};
use pr_imaging::memory_headroom::{MemoryInfo, MemoryInfoSource};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_platform_win::wic::decoder::WicDirectDecoder;
use pr_test_support::golden;

fn fixture(name: &str) -> PathBuf {
    golden::repo_root()
        .join("tests")
        .join("fixtures")
        .join("wic-direct")
        .join(name)
}

fn decode(name: &str, request: impl FnOnce(DecodeRequest) -> DecodeRequest) -> Arc<dyn DecodedImageView> {
    let memory: Arc<dyn MemoryInfoSource> = Arc::new(|| MemoryInfo {
        total_available: 64 * 1024 * 1024 * 1024,
        load: 0,
    });
    let decoder = WicDirectDecoder::new(PixelBufferImageCodec::shared(), None::<Arc<dyn SourceReader>>, memory);
    let bytes = std::fs::read(fixture(name)).expect("fixture bytes");
    let request = request(DecodeRequest::new(PathBuf::from(name), 0).with_bytes(Arc::from(bytes)));
    pr_imaging::decoder_traits::ImageDecoder::decode(&decoder, &request).expect("decode")
}

#[test]
#[ignore = "native: needs WIC"]
fn without_apply_orientation_no_tag_and_no_given_orientation_ever_rotates_or_is_reported() {
    for tag in 0..=9 {
        let name = format!("orient-{tag}-120x80.jpg");
        // Control: the tag is really in the file (2..=8 are tags a rotator acts on; 5..=8 swap the sides).
        let applied = decode(&name, |r| r.with_apply_orientation(true));
        let transposed = (5..=8).contains(&tag);
        let expected_applied = if transposed { (80, 120) } else { (120, 80) };
        assert_eq!(
            (applied.pixel_width(), applied.pixel_height()),
            expected_applied,
            "{name}: applied"
        );
        for given in [
            None,
            Some(0),
            Some(1),
            Some(2),
            Some(6),
            Some(8),
            Some(9),
            Some(-1),
            Some(i32::MIN),
            Some(i32::MAX),
        ] {
            let view = decode(&name, |r| {
                let r = r.with_apply_orientation(false);
                match given {
                    Some(value) => r.with_source_orientation(value),
                    None => r,
                }
            });
            assert_eq!(
                (view.pixel_width(), view.pixel_height()),
                (120, 80),
                "{name} given {given:?}: stored size"
            );
            assert_eq!(view.orientation(), 1, "{name} given {given:?}: orientation");
            assert_eq!(
                (view.original_width(), view.original_height()),
                (120, 80),
                "{name} given {given:?}: original"
            );
            assert!(!view.downscaled(), "{name} given {given:?}: downscaled");
        }
    }
}
