//! M5 killers (mutation close-out 2026-11-04) for `resampler.rs::box_footprint`. The mutant `i * source / target` ->
//! `i / source / target` (43:19) makes every footprint start at 0, which only slows the large golden tests down until the
//! harness times out; a timeout is not a kill. This binary is small and fast and sorts (and is listed) before the big
//! `rgb_bgra_resampler` binary, so the mutant fails here first.

use pr_core::sync::CancelToken;
use pr_libraw::resampler::resize;

// 8x8 gray -> 2x2 box average. Source value = row * 20 + column * 3; the expected values are block averages of the
// 4x4 blocks computed outside the code under test (half-up integer rounding, as C# BoxAverage).
#[test]
fn box_average_8x8_to_2x2_averages_each_own_block() {
    let rgb: Vec<u8> = (0..8u8).flat_map(|y| (0..8u8).map(move |x| y * 20 + x * 3)).collect();
    let mut bgra = vec![0u8; 2 * 2 * 4];

    resize(&rgb, 8, 8, &mut bgra, 2, 2, &CancelToken::none(), 1).expect("resize");

    let expected = [35u8, 47, 115, 127];
    for (pixel, value) in expected.iter().enumerate() {
        assert_eq!(
            &bgra[pixel * 4..pixel * 4 + 4],
            &[*value, *value, *value, 255],
            "target pixel {pixel}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------
// `convert` loop (resampler.rs 97:17 `<` -> `<=` / `==`, 98:40 `+` -> `*`, 99:33 `*` -> `/`).
//
// With these mutants the block loop never ends (the cursor does not advance, or it spins at `first == pixel_count`), so cargo-mutants
// can only report a TIMEOUT. The conversion runs on a worker thread; a watchdog cancels the token when the loop is stuck (the loop
// calls `cancel.check()` once per block), which turns the hang into `Err(Canceled)` and fails the assertion. The watchdog waits on a
// channel and is released the moment the conversion returns: no sleep and no fixed delay on the passing path.
// ---------------------------------------------------------------------------------------------------------------

use std::sync::mpsc;
use std::time::Duration;

use pr_libraw::resampler::ResampleError;

const WATCHDOG: Duration = Duration::from_secs(20);

/// `Convert` of `rgb` (equal sizes `width` x `height`) into a fresh BGRA buffer of `bgra_len` bytes, or the error.
fn convert_bounded(
    rgb: Vec<u8>,
    width: i32,
    height: i32,
    channels: i32,
    bgra_len: usize,
) -> (Result<(), ResampleError>, Vec<u8>) {
    let token = CancelToken::new();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let watchdog_token = token.clone();
    let watchdog = std::thread::Builder::new()
        .name("convert-watchdog".to_owned())
        .spawn(move || {
            if done_rx.recv_timeout(WATCHDOG).is_err() {
                watchdog_token.cancel();
            }
        })
        .expect("spawn watchdog");
    let mut bgra = vec![7u8; bgra_len];
    let result = resize(&rgb, width, height, &mut bgra, width, height, &token, channels);
    let _ = done_tx.send(());
    watchdog.join().expect("join watchdog");
    (result, bgra)
}

#[test]
fn convert_gray_pixels_beyond_the_first_check_block_are_all_swapped() {
    // 0x40000 + 5 pixels: the loop runs two blocks (the second starts at the stride), the empty tail is not a third block.
    let pixels = 0x4_0000usize + 5;
    let rgb: Vec<u8> = (0..pixels).map(|i| (i % 251) as u8).collect();
    let (result, bgra) = convert_bounded(rgb.clone(), pixels as i32, 1, 1, pixels * 4);
    assert_eq!(
        result,
        Ok(()),
        "convert must finish and not be canceled by the watchdog"
    );
    for i in [0usize, 1, 0x3_FFFF, 0x4_0000, 0x4_0001, pixels - 1] {
        let v = rgb[i];
        assert_eq!(&bgra[i * 4..i * 4 + 4], &[v, v, v, 255], "pixel {i}");
    }
}

#[test]
fn convert_rgb_pixels_in_the_second_check_block_use_their_own_source_bytes() {
    // channels = 3 so that `first * channels` != `first / channels` for the second block (first = 0x40000).
    let pixels = 0x4_0000usize + 3;
    let rgb: Vec<u8> = (0..pixels * 3).map(|i| (i % 253) as u8).collect();
    let (result, bgra) = convert_bounded(rgb.clone(), pixels as i32, 1, 3, pixels * 4);
    assert_eq!(
        result,
        Ok(()),
        "convert must finish and not be canceled by the watchdog"
    );
    for i in [0usize, 0x3_FFFF, 0x4_0000, 0x4_0001, pixels - 1] {
        let (r, g, b) = (rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2]);
        assert_eq!(&bgra[i * 4..i * 4 + 4], &[b, g, r, 255], "pixel {i}");
    }
}

#[test]
fn convert_small_image_finishes_and_writes_every_pixel() {
    let (result, bgra) = convert_bounded(vec![1, 2, 3, 4, 5, 6], 2, 1, 3, 8);
    assert_eq!(result, Ok(()));
    assert_eq!(bgra, [3, 2, 1, 255, 6, 5, 4, 255]);
}

#[test]
fn convert_empty_source_finishes_without_writing() {
    let (result, bgra) = convert_bounded(Vec::new(), 2, 2, 1, 16);
    assert_eq!(result, Ok(()));
    assert_eq!(bgra, vec![7u8; 16]);
}

// ---------------------------------------------------------------------------------------------------------------
// Dimension guards (resampler.rs 336:48 and 336:69 `||` -> `&&` in `resize`, 392:26 in `resize_to_buffer`): ONE non-positive
// size, all others valid, must give exactly the guard's message (and not whichever error a later stage would raise).
// ---------------------------------------------------------------------------------------------------------------

fn guard_message(sw: i32, sh: i32, tw: i32, th: i32) -> String {
    format!("Invalid RAW resize dimensions {sw}x{sh} -> {tw}x{th}.")
}

#[test]
fn resize_with_a_single_non_positive_size_reports_the_dimension_guard() {
    let rgb = vec![0u8; 4 * 4];
    for (sw, sh, tw, th) in [
        (0, 4, 2, 2),
        (-1, 4, 2, 2),
        (4, 0, 2, 2),
        (4, -1, 2, 2),
        (4, 4, 0, 2),
        (4, 4, -1, 2),
        (4, 4, 2, 0),
        (4, 4, 2, -1),
    ] {
        let mut bgra = vec![0u8; 64];
        let result = resize(&rgb, sw, sh, &mut bgra, tw, th, &CancelToken::none(), 1);
        assert_eq!(
            result,
            Err(ResampleError::InvalidData(guard_message(sw, sh, tw, th))),
            "resize {sw}x{sh} -> {tw}x{th}"
        );
    }
}

#[test]
fn resize_to_buffer_with_a_single_non_positive_source_size_reports_the_dimension_guard_before_allocating() {
    // The target is too large for a BGRA buffer: if the guard did not fire first, the allocation error would come out instead.
    for (sw, sh) in [(0, 1), (-1, 1), (1, 0), (1, -1)] {
        let result = pr_libraw::resampler::resize_to_buffer(&[], sw, sh, 50_000, 50_000, 1, &CancelToken::none());
        assert_eq!(
            result.err(),
            Some(ResampleError::InvalidData(guard_message(sw, sh, 50_000, 50_000))),
            "resize_to_buffer {sw}x{sh} -> 50000x50000"
        );
    }
}
