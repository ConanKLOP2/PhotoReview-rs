//! W8-09b slice S2: `wic::cache_adapters` (`WicPreviewPayloadCodec`, `WicCacheImageReader`).
//!
//! C# twins: none (C# called the static `WicImageEncoder` / `WicCacheImageReader`); every test is `// new:`. The error mapping is pure; the round trips use WIC and are
//! `#[ignore = "native: needs WIC"]`, each on its own thread bounded by 30 s.
#![cfg(windows)]

use std::io;
use std::sync::mpsc;
use std::time::Duration;

use pr_imaging::cache::preview_file::{PreviewCacheError, PreviewPayloadCodec};
use pr_imaging::cache::thumbnails::CacheImageReader;
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::pixels::{PixelBuffer, PixelBufferError, PixelLayout};
use pr_platform_win::wic::cache_adapters::{
    PREVIEW_PAYLOAD_NOT_DECODED, WicCacheImageReader, WicPreviewPayloadCodec, cache_reader_error, encode_error_to_io,
    preview_decode_error,
};
use pr_platform_win::wic::cache_image::{
    CacheImageError, EncodeError, INVALID_DATA_DIMENSIONS, INVALID_DATA_EMPTY, encode_png,
};

fn bounded<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, receiver) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let _ = sender.send(body());
    });
    match receiver.recv_timeout(Duration::from_secs(30)) {
        Ok(value) => {
            handle.join().expect("test thread");
            value
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => match handle.join() {
            Ok(()) => panic!("test thread ended without a result"),
            Err(panic) => std::panic::resume_unwind(panic),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("native WIC test exceeded 30 s"),
    }
}

/// A Bgr32 buffer whose every pixel is `(b, g, r)`.
fn solid(width: i32, height: i32, b: u8, g: u8, r: u8) -> PixelBuffer {
    let mut pixels = PixelBuffer::allocate(width, height, PixelLayout::Bgr32).expect("geometry");
    for pixel in pixels.bytes_mut().expect("bytes").chunks_exact_mut(4) {
        pixel.copy_from_slice(&[b, g, r, 255]);
    }
    pixels
}

// new: the encoder error becomes an io::Error; one the writer produced keeps its kind, the others are `Other` with the Display text.
#[test]
fn encode_errors_map_to_io_errors() {
    let kept = encode_error_to_io(EncodeError::Io(io::Error::new(io::ErrorKind::BrokenPipe, "pipe")));
    assert_eq!(kept.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(kept.to_string(), "pipe");

    let range = encode_error_to_io(EncodeError::QualityOutOfRange { quality: 0 });
    assert_eq!(range.kind(), io::ErrorKind::Other);
    assert_eq!(
        range.to_string(),
        EncodeError::QualityOutOfRange { quality: 0 }.to_string()
    );
    let com = encode_error_to_io(EncodeError::Com { hresult: -2147467259 });
    assert_eq!(com.kind(), io::ErrorKind::Other);
    assert!(com.to_string().contains("0x80004005"), "{com}");
}

// new: `.pv4` payload decode errors: out of memory keeps the entry, everything else is InvalidData with the C# message.
#[test]
fn preview_decode_errors_map_to_the_cache_errors() {
    assert!(matches!(
        preview_decode_error(CacheImageError::InsufficientMemory { hresult: -2147024882 }),
        PreviewCacheError::OutOfMemory(_)
    ));
    assert!(matches!(
        preview_decode_error(CacheImageError::Pixels(PixelBufferError::OutOfMemory { byte_count: 9 })),
        PreviewCacheError::OutOfMemory(_)
    ));
    match preview_decode_error(CacheImageError::Com { hresult: -2144108494 }) {
        PreviewCacheError::InvalidData(text) => assert_eq!(text, PREVIEW_PAYLOAD_NOT_DECODED),
        other => panic!("{other:?}"),
    }
    match preview_decode_error(CacheImageError::InvalidData(INVALID_DATA_DIMENSIONS)) {
        PreviewCacheError::InvalidData(text) => assert_eq!(text, "Cache entry payload has invalid dimensions."),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        preview_decode_error(CacheImageError::Pixels(PixelBufferError::Disposed)),
        PreviewCacheError::InvalidData(_)
    ));
}

// new: thumbnail reader errors: InvalidData / OutOfMemory / Com, as the trait documents.
#[test]
fn cache_reader_errors_map_to_the_decode_errors() {
    match cache_reader_error(CacheImageError::InvalidData(INVALID_DATA_EMPTY)) {
        DecodeError::InvalidData { message } => assert_eq!(message, "Cache entry payload is empty."),
        other => panic!("{other:?}"),
    }
    match cache_reader_error(CacheImageError::Com { hresult: -2144108494 }) {
        DecodeError::Com { hresult, message } => {
            assert_eq!(hresult, -2144108494);
            assert!(message.contains(&format!("0x{:08X}", hresult as u32)), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        cache_reader_error(CacheImageError::InsufficientMemory { hresult: -2147024882 }),
        DecodeError::OutOfMemory { .. }
    ));
    assert!(matches!(
        cache_reader_error(CacheImageError::Pixels(PixelBufferError::OutOfMemory { byte_count: 1 })),
        DecodeError::OutOfMemory { .. }
    ));
    assert!(matches!(
        cache_reader_error(CacheImageError::Pixels(PixelBufferError::Disposed)),
        DecodeError::InvalidData { .. }
    ));
}

// new: an empty payload is refused before any WIC call (so this runs without WIC), through both adapters.
#[test]
fn an_empty_payload_is_invalid_data_in_both_adapters() {
    match WicPreviewPayloadCodec.decode_jpeg(&[], PixelLayout::Bgr32, 4, 4) {
        Err(PreviewCacheError::InvalidData(text)) => assert_eq!(text, "Cache entry payload is empty."),
        other => panic!("{other:?}"),
    }
    match WicCacheImageReader.decode(&[], None, DecodeBox::UNBOUNDED, 0, 0) {
        Err(DecodeError::InvalidData { message }) => assert_eq!(message, "Cache entry payload is empty."),
        other => panic!("{other:?}"),
    }
}

// new: JPEG payload round trip through the codec: size and (lossy) colour survive.
#[test]
#[ignore = "native: needs WIC"]
fn payload_codec_encodes_then_decodes_a_16x16_buffer() {
    bounded(|| {
        let source = solid(16, 16, 40, 120, 200);
        let mut jpeg = Vec::new();

        WicPreviewPayloadCodec
            .encode_jpeg(&source, 90, &mut jpeg)
            .expect("encode");

        assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "SOI");
        assert_eq!(&jpeg[jpeg.len() - 2..], &[0xFF, 0xD9], "EOI");
        let decoded = WicPreviewPayloadCodec
            .decode_jpeg(&jpeg, PixelLayout::Bgr32, 16, 16)
            .expect("decode");
        assert_eq!((decoded.width(), decoded.height()), (16, 16));
        let pixel = &decoded.bytes().expect("bytes")[..4];
        for (got, want) in pixel.iter().zip([40u8, 120, 200]) {
            assert!(got.abs_diff(want) <= 8, "{pixel:?}");
        }
    });
}

// new: a payload whose size differs from the header is InvalidData; an out-of-range quality is an io error.
#[test]
#[ignore = "native: needs WIC"]
fn payload_codec_refuses_a_size_mismatch_and_a_bad_quality() {
    bounded(|| {
        let mut jpeg = Vec::new();
        WicPreviewPayloadCodec
            .encode_jpeg(&solid(16, 16, 1, 2, 3), 80, &mut jpeg)
            .expect("encode");
        match WicPreviewPayloadCodec.decode_jpeg(&jpeg, PixelLayout::Bgr32, 8, 8) {
            Err(PreviewCacheError::InvalidData(text)) => {
                assert_eq!(text, "Cache entry payload dimensions do not match its header.");
            }
            other => panic!("{other:?}"),
        }
        match WicPreviewPayloadCodec.decode_jpeg(b"not a jpeg", PixelLayout::Bgr32, 8, 8) {
            Err(PreviewCacheError::InvalidData(_)) => {}
            other => panic!("{other:?}"),
        }
        let error = WicPreviewPayloadCodec
            .encode_jpeg(&solid(2, 2, 0, 0, 0), 0, &mut Vec::new())
            .expect_err("quality 0");
        assert_eq!(error.kind(), io::ErrorKind::Other);
    });
}

// new: the thumbnail reader decodes the PNG of `encode_png` bit for bit and reports the source size.
#[test]
#[ignore = "native: needs WIC"]
fn cache_reader_decodes_the_png_of_encode_png() {
    bounded(|| {
        let source = solid(16, 16, 10, 20, 30);
        let mut png = Vec::new();
        encode_png(&source, &mut png).expect("encode png");

        let (pixels, info) = WicCacheImageReader
            .decode(&png, Some(PixelLayout::Bgr32), DecodeBox::UNBOUNDED, 0, 0)
            .expect("decode");

        assert_eq!((pixels.width(), pixels.height()), (16, 16));
        assert_eq!(
            (info.source_width, info.source_height, info.downscaled),
            (16, 16, false)
        );
        assert_eq!(pixels.bytes().expect("bytes"), source.bytes().expect("bytes"));
    });
}

// new: garbage is a Com/InvalidData error, never a panic.
#[test]
#[ignore = "native: needs WIC"]
fn cache_reader_reports_garbage_as_an_error() {
    bounded(|| {
        let result = WicCacheImageReader.decode(b"definitely not an image", None, DecodeBox::UNBOUNDED, 0, 0);
        assert!(matches!(
            result,
            Err(DecodeError::Com { .. } | DecodeError::InvalidData { .. })
        ));
    });
}
