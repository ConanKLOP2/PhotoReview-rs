//! CX-MUTW4 deterministic pure-boundary mutation regressions.
#![cfg(windows)]

use pr_imaging::decode_types::DecodeBox;
use pr_platform_win::wic::cache_image::{
    CacheImageError, INVALID_DATA_TOO_LARGE, MAX_DECODED_BYTES, check_payload_len, plan_decode,
};

#[test]
fn cache_payload_and_final_dimensions_accept_exact_limits() {
    // Mutants wic/cache_image.rs:247:19 (> to >=), 282:61 (< to >),
    // 292:32/50 (* to + or /). C#: WicCacheImageReader.Decode size guards.
    assert_eq!(check_payload_len(u32::MAX as usize), Ok(()));
    assert_eq!(
        check_payload_len(u32::MAX as usize + 1),
        Err(CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE))
    );
    let width = (MAX_DECODED_BYTES / 4) as u32;
    let accepted = plan_decode(width, 1, DecodeBox::UNBOUNDED, 0, 0).unwrap();
    assert_eq!(
        (accepted.width, accepted.height, accepted.downscaled),
        (width as i32, 1, false)
    );
    for (w, h) in [(width + 1, 1), (32768, 16384), (16384, 32768)] {
        assert_eq!(
            plan_decode(w, h, DecodeBox::UNBOUNDED, 0, 0),
            Err(CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE))
        );
    }
    for (w, h, bw, bh, expected) in [(1, 4, 0, 2, (1, 2)), (4, 1, 2, 0, (2, 1))] {
        let plan = plan_decode(w, h, DecodeBox::new(bw, bh), 0, 0).unwrap();
        assert_eq!((plan.width, plan.height), expected);
        assert_eq!((plan.source_width, plan.source_height), (w as i32, h as i32));
        assert!(plan.downscaled);
    }
}

#[test]
fn jpeg_quality_uses_fractional_single_precision_value() {
    // Mutants wic/cache_image.rs:340:5 (return 0/1/-1), 340:20 (/ to % or *).
    // C#: WicImageEncoder.Encode uses quality / 100f for ImageQuality.
    use pr_platform_win::wic::cache_image::image_quality_value;
    for (quality, expected) in [(1, 0.01_f32), (37, 0.37_f32), (100, 1.0_f32)] {
        assert_eq!(image_quality_value(quality).to_bits(), expected.to_bits());
    }
}

#[test]
#[ignore = "native: needs WIC"]
fn jpeg_option_bag_keeps_image_quality_observable_in_encoded_bytes() {
    // Incidental mutants wic/cache_image.rs:679-681 delete PROPBAG2 fields.
    // C#: WicImageEncoder writes ImageQuality before frame.Initialize.
    use pr_imaging::pixels::{PixelBuffer, PixelLayout};
    use pr_platform_win::wic::cache_image::encode_jpeg;
    let mut pixels = PixelBuffer::allocate(16, 8, PixelLayout::Bgr32).unwrap();
    for (index, byte) in pixels.bytes_mut().unwrap().iter_mut().enumerate() {
        *byte = ((index * 131 + index / 13) % 256) as u8;
    }
    let mut low = Vec::new();
    let mut high = Vec::new();
    encode_jpeg(&pixels, &mut low, 37).unwrap();
    encode_jpeg(&pixels, &mut high, 83).unwrap();
    assert!(low.starts_with(&[0xff, 0xd8]));
    assert!(high.starts_with(&[0xff, 0xd8]));
    assert_ne!(low, high);
}

#[test]
fn output_failure_is_swallowed_only_for_jpeg() {
    // Mutants wic/cache_image.rs:452:5 (return Ok), 454:19 (guard true/false).
    // C#: WicCacheImageIoTests.Encode_ThrowingOutputStream reflects JPEG/PNG asymmetry.
    use pr_platform_win::wic::cache_image::{EncodeError, WINCODEC_ERR_STREAMWRITE, finish_output};
    for jpeg in [false, true] {
        assert!(finish_output(jpeg, Ok(())).is_ok());
        let failed = finish_output(jpeg, Err(std::io::Error::other("fake output failure")));
        if jpeg {
            assert!(failed.is_ok());
        } else {
            assert!(matches!(failed, Err(EncodeError::Com { hresult }) if hresult == WINCODEC_ERR_STREAMWRITE));
        }
    }
}

#[test]
fn source_stream_cached_reads_advance_position_and_cross_window_edges() {
    // Mutants wic/decoder.rs:545:39 (+ to - or *); also read cursor/fetch arithmetic.
    // C#: FileStream-like read window must preserve the position across consecutive reads.
    use pr_platform_win::wic::decoder::SourceStreamBackend;
    use pr_platform_win::wic::interop::StreamBackend;
    use std::io::{Cursor, SeekFrom};
    let mut stream = SourceStreamBackend::with_buffer_size(Box::new(Cursor::new((1_u8..=12).collect::<Vec<_>>())), 4);
    for (expected, position) in [
        (&[1, 2][..], 2),
        (&[3][..], 3),
        (&[4, 5, 6, 7][..], 7),
        (&[8, 9, 10, 11][..], 11),
        (&[12][..], 12),
    ] {
        let mut output = vec![0; expected.len()];
        assert_eq!(stream.read(&mut output).unwrap(), expected.len());
        assert_eq!(output, expected);
        assert_eq!(stream.seek(SeekFrom::Current(0)).unwrap(), position);
    }
    assert_eq!(stream.len().unwrap(), 12);
    assert_eq!(stream.seek(SeekFrom::Start(1)).unwrap(), 1);
    let mut reread = [0; 3];
    assert_eq!(stream.read(&mut reread).unwrap(), 3);
    assert_eq!(reread, [2, 3, 4]);
}
