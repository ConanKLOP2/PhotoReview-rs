//! Adversarial verification tests (committed). Each asserts the C# d92d6aae behaviour.
use pr_core::sync::CancelToken;
use pr_libraw::resampler::resize;

/// C# Resize(rgb, 0,0, bgra, 0,0): sizes equal -> Convert, which ignores geometry and converts rgb.Length/channels pixels.
#[test]
fn convert_equal_zero_sizes_converts_like_csharp() {
    let rgb = [1u8, 2, 3];
    let mut bgra = [0u8; 4];
    let r = resize(&rgb, 0, 0, &mut bgra, 0, 0, &CancelToken::none(), 3);
    assert!(r.is_ok(), "C# converts 1 pixel; got {r:?}");
    assert_eq!(bgra, [3, 2, 1, 255]);
}

#[test]
fn convert_equal_negative_sizes_converts_like_csharp() {
    let rgb = [1u8, 2, 3];
    let mut bgra = [0u8; 4];
    let r = resize(&rgb, -1, -1, &mut bgra, -1, -1, &CancelToken::none(), 3);
    assert!(r.is_ok(), "C# converts 1 pixel; got {r:?}");
    assert_eq!(bgra, [3, 2, 1, 255]);
}

/// C# Convert writes pixel 0 and THEN checks the token (index 0 & 0x3FFFF == 0).
#[test]
fn convert_cancelled_writes_pixel_zero_before_throwing() {
    let token = CancelToken::new();
    token.cancel();
    let rgb = [1u8, 2, 3, 4, 5, 6];
    let mut bgra = [0u8; 8];
    let r = resize(&rgb, 2, 1, &mut bgra, 2, 1, &token, 3);
    assert!(r.is_err());
    assert_eq!(&bgra[..4], &[3, 2, 1, 255]);
}

use pr_libraw::resampler::{ResampleError, resize_to_buffer};

/// Kills mutant 93:33 (`first * channels` -> `first / channels`): RGB convert of more than 262144 pixels.
#[test]
fn convert_rgb_larger_than_one_check_block_is_exact() {
    let n = 0x4_0000 + 5usize;
    let rgb: Vec<u8> = (0..n * 3).map(|i| (i % 251) as u8).collect();
    let mut bgra = vec![0u8; n * 4];
    resize(&rgb, n as i32, 1, &mut bgra, n as i32, 1, &CancelToken::none(), 3).unwrap();
    for p in [0usize, 1, 0x3_FFFF, 0x4_0000, 0x4_0001, n - 1] {
        assert_eq!(
            &bgra[p * 4..p * 4 + 4],
            &[rgb[p * 3 + 2], rgb[p * 3 + 1], rgb[p * 3], 255],
            "pixel {p}"
        );
    }
}

/// Kills mutants 322:48 and 322:69: each single non-positive size alone must be InvalidData from `resize`.
#[test]
fn resize_each_single_non_positive_size_is_invalid_data() {
    let rgb = vec![0u8; 4 * 4 * 3];
    for (sw, sh, tw, th) in [
        (0, 4, 2, 2),
        (4, 0, 2, 2),
        (4, 4, 0, 2),
        (4, 4, 2, 0),
        (4, 4, -1, 2),
        (4, 4, 2, -1),
    ] {
        let mut bgra = vec![0u8; 64];
        let r = resize(&rgb, sw, sh, &mut bgra, tw, th, &CancelToken::none(), 3);
        assert!(
            matches!(r, Err(ResampleError::InvalidData(_))),
            "{sw}x{sh}->{tw}x{th}: {r:?}"
        );
    }
}

/// Kills mutant 385:26: sw<=0 alone must be rejected by the guard BEFORE the allocation (huge target would overflow).
#[test]
fn resize_to_buffer_zero_source_width_rejected_before_allocation() {
    for (sw, sh) in [(0, 1), (1, 0), (-1, 1), (1, -1)] {
        let r = resize_to_buffer(&[0u8; 3], sw, sh, i32::MAX, 2, 3, &CancelToken::none());
        assert!(
            matches!(r, Err(ResampleError::InvalidData(_))),
            "{sw}x{sh}: {:?}",
            r.err()
        );
    }
}
