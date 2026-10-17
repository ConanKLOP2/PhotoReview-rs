//! Verifier-only adversarial tests for W5-17 (NOT committed).
use pr_core::sync::CancelToken;
use pr_imaging::pixel_memory::NativePixelMemory;
use pr_libraw::resampler::{ResampleError, resize, resize_to_buffer};
use std::sync::{Mutex, PoisonError};

static GUARD: Mutex<()> = Mutex::new(());

fn bytes(len: usize) -> Vec<u8> {
    let mut s = 0x1234_5678u32;
    (0..len)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s >> 8) as u8
        })
        .collect()
}

fn convert_check(pixels: usize, channels: usize) {
    let rgb = bytes(pixels * channels);
    let mut bgra = vec![0u8; pixels * 4];
    resize(
        &rgb,
        pixels as i32,
        1,
        &mut bgra,
        pixels as i32,
        1,
        &CancelToken::none(),
        channels as i32,
    )
    .unwrap();
    for i in 0..pixels {
        let (g, b) = if channels == 3 { (1, 2) } else { (0, 0) };
        let s = i * channels;
        assert_eq!(
            &bgra[i * 4..i * 4 + 4],
            &[rgb[s + b], rgb[s + g], rgb[s], 255],
            "pixel {i}"
        );
    }
}

#[test]
fn convert_rgb_crosses_block_boundaries() {
    for n in [262_143usize, 262_144, 262_145, 524_289, 600_000] {
        convert_check(n, 3);
        convert_check(n, 1);
    }
}

fn expect_invalid(r: Result<(), ResampleError>) {
    assert!(matches!(r, Err(ResampleError::InvalidData(_))), "{r:?}");
}

#[test]
fn resize_each_single_non_positive_size_is_invalid_data() {
    let rgb = vec![7u8; 10 * 10 * 3];
    for (sw, sh, tw, th) in [
        (0, 10, 5, 5),
        (10, 0, 5, 5),
        (10, 10, 0, 5),
        (10, 10, 5, 0),
        (-1, 10, 5, 5),
        (10, -1, 5, 5),
        (10, 10, -1, 5),
        (10, 10, 5, -1),
        (10, 10, 12, 0),
        (10, 10, 0, 12),
        (10, 0, 12, 12),
        (0, 10, 12, 12),
    ] {
        let mut out = vec![0u8; 400];
        expect_invalid(resize(&rgb, sw, sh, &mut out, tw, th, &CancelToken::none(), 3));
    }
}

#[test]
fn resize_to_buffer_guard_runs_before_any_allocation() {
    let _g = GUARD.lock().unwrap_or_else(PoisonError::into_inner);
    let before = NativePixelMemory::live_count();
    for (sw, sh) in [(0, 10), (10, 0), (-3, 10), (10, -3)] {
        let r = resize_to_buffer(&[0u8; 300], sw, sh, 40000, 40000, 3, &CancelToken::none());
        assert!(
            matches!(r, Err(ResampleError::InvalidData(_))),
            "{sw}x{sh}: {:?}",
            r.err()
        );
    }
    for (tw, th) in [(0, 40000), (40000, 0), (-1, 40000), (40000, -1)] {
        let r = resize_to_buffer(&[0u8; 300], 10, 10, tw, th, 3, &CancelToken::none());
        assert!(
            matches!(r, Err(ResampleError::InvalidData(_))),
            "{tw}x{th}: {:?}",
            r.err()
        );
    }
    assert_eq!(NativePixelMemory::live_count(), before);
}
