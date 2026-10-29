//! CX-IMAGING3 deterministic cache identity mutation regressions.
use pr_imaging::cache::thumbnails::{build_key, build_stamp, cache_file_path};
use pr_test_support::TempRoot;

#[test]
fn thumbnail_key_preserves_utf8_case_and_invariant_stamp() {
    // Mutants cache/thumbnails.rs:674:5 (return empty/xyzzy), 678:50 (>> to <<),
    // 679:50 (& to | / ^): assert directly without worker gates or disk I/O.
    // C# ThumbnailCache.BuildKey: SHA-256 of the invariant UTF-8 stamp.
    assert_eq!(
        build_stamp("photo-\u{e9}.JPG", u64::MAX, -17),
        "photo-\u{e9}.JPG|18446744073709551615|-17|800|orient=1"
    );
    assert_eq!(
        build_key("photo-\u{e9}.JPG", u64::MAX, -17),
        "0eb8d4f4695e203daadf008aac80247b0acb1b440bc9f1a6044906ddf42b38fd"
    );
    assert_ne!(
        build_key("photo-\u{e9}.JPG", u64::MAX, -17),
        build_key("photo-\u{e9}.jpg", u64::MAX, -17)
    );
}

#[test]
fn thumbnail_cache_file_path_retains_directory_key_and_png_suffix() {
    // Mutant cache/thumbnails.rs:686:5, replace cache_file_path with Default::default().
    // C# ThumbnailCache: Path.Combine(diskDirectory, key + ".png").
    let root = TempRoot::new("cx3-key-path");
    assert_eq!(cache_file_path(root.path(), "abc012"), root.path().join("abc012.png"));
}

// The platform test binary expands this macro. Keeping tokens here avoids a
// pr-imaging -> pr-platform-win dev-dependency cycle while keeping killers together.
#[macro_export]
macro_rules! wic_cache_boundary_killers {
    () => {
        #[test]
        fn cache_decode_plan_height_only_reduction_and_byte_limit_are_exact() {
            // Mutants wic/cache_image.rs:282:61, < to >; 292:32/50, * to + or /.
            // C# WicCacheImageReader.Decode: final dimensions and w*h*4 byte budget.
            use pr_imaging::decode_types::DecodeBox;
            use pr_platform_win::wic::cache_image::{
                CacheImageError, INVALID_DATA_TOO_LARGE, MAX_DECODED_BYTES, plan_decode,
            };
            let height_only = plan_decode(1, 4, DecodeBox::new(0, 2), 0, 0).unwrap();
            assert_eq!(
                (height_only.width, height_only.height, height_only.downscaled),
                (1, 2, true)
            );
            assert_eq!((height_only.source_width, height_only.source_height), (1, 4));
            // Just below and just above the byte boundary, without allocating any pixels.
            let accepted_width = (MAX_DECODED_BYTES / 4) as u32;
            let accepted = plan_decode(accepted_width, 1, DecodeBox::UNBOUNDED, 0, 0).unwrap();
            assert_eq!(
                (accepted.width, accepted.height, accepted.downscaled),
                (accepted_width as i32, 1, false)
            );
            for (w, h) in [(accepted_width + 1, 1), (32768, 16384), (16384, 32768)] {
                assert_eq!(
                    plan_decode(w, h, DecodeBox::UNBOUNDED, 0, 0),
                    Err(CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE)),
                    "{w}x{h}"
                );
            }
            let shrunk = plan_decode(32768, 16384, DecodeBox::new(2, 1), 0, 0).unwrap();
            assert_eq!((shrunk.width, shrunk.height, shrunk.downscaled), (2, 1, true));
        }
        #[test]
        fn cache_payload_length_accepts_max_and_rejects_oversize_without_allocation() {
            // Mutant wic/cache_image.rs:247:19, > to >= (and == / <); no buffer allocation.
            // C# WicCacheImageReader.Decode: guard the managed stream length before WIC.
            use pr_platform_win::wic::cache_image::{CacheImageError, INVALID_DATA_TOO_LARGE, check_payload_len};
            assert_eq!(check_payload_len(0), Ok(()));
            assert_eq!(check_payload_len(u32::MAX as usize), Ok(()));
            assert_eq!(
                check_payload_len(u32::MAX as usize + 1),
                Err(CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE))
            );
            assert_eq!(
                check_payload_len(usize::MAX),
                Err(CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE))
            );
        }
    };
}
