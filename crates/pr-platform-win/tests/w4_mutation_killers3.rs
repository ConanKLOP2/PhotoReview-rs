//! CX-W4REST deterministic decoder and cache mutation regressions.
#![cfg(windows)]

use pr_core::abstractions::{ReadSeek, SourceReadPriority, SourceReader};
use pr_imaging::decode_types::DecodeRequest;
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::memory_headroom::MemoryInfo;
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_platform_win::wic::decoder::WicDirectDecoder;
use pr_test_support::TempRoot;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct RefusingReader(AtomicUsize);
impl SourceReader for RefusingReader {
    fn open_source(
        &self,
        _: &std::path::Path,
        _: SourceReadPriority,
        buffer_size: usize,
    ) -> std::io::Result<Box<dyn ReadSeek>> {
        assert_eq!(buffer_size, 1_048_576);
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(std::io::Error::other("w4rest source open sentinel"))
    }
}

#[test]
fn decode_blank_path_rejects_before_open_and_nonblank_path_reaches_source_reader() {
    // Mutants wic/decoder.rs:390:5, replace is_blank_path with true / false.
    // C#: WicDirectDecoder.Decode rejects blank request.Path before opening the source.
    let reader = Arc::new(RefusingReader(AtomicUsize::new(0)));
    let decoder = WicDirectDecoder::new(
        PixelBufferImageCodec::shared(),
        Some(reader.clone()),
        Arc::new(|| MemoryInfo {
            total_available: 32 * 1024 * 1024 * 1024,
            load: 0,
        }),
    );
    assert!(format!("{decoder:?}").starts_with("WicDirectDecoder"));
    for blank in ["", " ", "\t\r\n", "\u{2003}"] {
        let error = decoder
            .decode(&DecodeRequest::new(blank, 0))
            .err()
            .expect("blank path must fail");
        assert!(matches!(error, DecodeError::InvalidArgument { .. }));
        assert!(error.message().contains("request.Path"));
        assert_eq!(reader.0.load(Ordering::SeqCst), 0);
    }
    let root = TempRoot::new("w4rest-nonblank");
    let error = decoder
        .decode(&DecodeRequest::new(root.path().join("fake.jpg"), 0))
        .err()
        .expect("source reader must fail");
    assert_eq!(error.message(), "w4rest source open sentinel");
    assert_eq!(reader.0.load(Ordering::SeqCst), 1);
}

#[test]
fn wic_argument_hresult_preserves_invalid_argument_before_outer_relabel() {
    // Mutant wic/decoder.rs:828:9 deletes Argument / ArgumentOutOfRange mapping.
    // C#: Marshal.GetExceptionForHR retains these exception types before Decode's outer catch.
    for hresult in [0x8007_0057_u32, 0x8013_1502, 0x8007_0459] {
        let mapped =
            pr_platform_win::wic::decoder::decode_error_from_wic(&pr_platform_win::wic::interop::WicError::Com {
                hresult: hresult as i32,
            });
        assert!(
            matches!(mapped, DecodeError::InvalidArgument { .. }),
            "{hresult:08X}: {mapped:?}"
        );
        assert!(!mapped.message().is_empty());
        let relabeled = pr_platform_win::wic::decoder::relabel_invalid_argument(mapped, "WicDirect");
        assert!(matches!(relabeled, DecodeError::InvalidData { .. }));
    }
}

#[test]
fn cache_diagnostics_retain_hresult_geometry_and_underlying_error() {
    // Mutants wic/cache_image.rs:219:9, 352:9, 404:9: Display body -> Ok(Default::default()).
    // C#: cache codec exceptions preserve failure details for diagnostics.
    use pr_platform_win::wic::cache_image::{CacheImageError, DestinationTooShort, EncodeError};
    assert_eq!(
        CacheImageError::InvalidData("invalid payload").to_string(),
        "invalid payload"
    );
    assert_eq!(
        CacheImageError::Com {
            hresult: 0x8898_2F71_u32 as i32
        }
        .to_string(),
        "WIC call failed (0x88982F71)."
    );
    assert_eq!(
        DestinationTooShort { required: 9, actual: 8 }.to_string(),
        "Destination is too short."
    );
    assert_eq!(
        EncodeError::QualityOutOfRange { quality: -7 }.to_string(),
        "JPEG quality must be between 1 and 100 (was -7)."
    );
    assert_eq!(
        EncodeError::TooLarge {
            byte_count: 4_294_967_296
        }
        .to_string(),
        "Pixel buffer is too large to encode (4294967296 bytes)."
    );
    assert_eq!(
        EncodeError::Io(std::io::Error::other("fake write failure")).to_string(),
        "Writing the encoded image failed: fake write failure"
    );
}

#[test]
fn cache_band_budget_is_exactly_256_kib() {
    // Mutants wic/cache_image.rs:113:35, replace * with + or /.
    // C#: WicImageEncoder uses 256 * 1024 bytes for BGR conversion bands.
    assert_eq!(pr_platform_win::wic::cache_image::BAND_BYTES, 262_144);
}

#[test]
fn source_stream_preserves_initial_cursor_across_length_and_seek_and_is_read_only() {
    // Mutants wic/decoder.rs:64/67 (* -> + or /), 450 (position -> 0/1),
    // 470 (move_inner_to -> Ok / != -> ==), 563 (write_all -> Ok),
    // 329/529 (Debug -> Ok) and 605 (into_any -> default).
    // C#: source FileStream buffering preserves its logical cursor across Length and Seek.
    use pr_platform_win::wic::decoder::{DECODE_SOURCE_BUFFER_SIZE, READ_INFO_SOURCE_BUFFER_SIZE, SourceStreamBackend};
    use pr_platform_win::wic::interop::StreamBackend;
    use std::io::{Cursor, SeekFrom};
    assert_eq!(DECODE_SOURCE_BUFFER_SIZE, 1_048_576);
    assert_eq!(READ_INFO_SOURCE_BUFFER_SIZE, 65_536);
    let mut cursor = Cursor::new(vec![10, 11, 12, 13, 14, 15]);
    cursor.set_position(2);
    let mut stream = SourceStreamBackend::with_buffer_size(Box::new(cursor), 2);
    assert!(format!("{stream:?}").starts_with("SourceStreamBackend"));
    assert_eq!(stream.seek(SeekFrom::Current(0)).unwrap(), 2);
    assert_eq!(stream.len().unwrap(), 6);
    assert_eq!(stream.seek(SeekFrom::Current(0)).unwrap(), 2);
    let mut byte = [0];
    assert_eq!(stream.read(&mut byte).unwrap(), 1);
    assert_eq!(byte, [12]);
    stream.seek(SeekFrom::Start(0)).unwrap();
    assert_eq!(stream.read(&mut byte).unwrap(), 1);
    assert_eq!(byte, [10]);
    assert_eq!(
        stream.write_all(&[99]).unwrap_err().kind(),
        std::io::ErrorKind::Unsupported
    );
    assert!(Box::new(stream).into_any().is::<SourceStreamBackend>());
}

#[test]
fn cache_entry_failure_accepts_each_dotnet_base_class_and_rejects_resource_failures() {
    // Mutants wic/cache_image.rs:195:5 (return true), 202-205 (|| -> &&).
    // C#: DiskCacheStore.IsCacheEntryFailure accepts Argument/IO/NotSupported/InvalidOperation subclasses.
    use pr_platform_win::wic::cache_image::com_is_entry_failure;
    for hresult in [
        0x8007_0057_u32,
        0x8007_0002,
        0x8013_1515,
        0x8013_1509,
        0x8013_1537,
        0x8000_4002,
    ] {
        assert!(com_is_entry_failure(hresult as i32), "entry failure {hresult:08X}");
    }
    for hresult in [0x8007_000E_u32, 0x8000_4001, 0x8000_4003, 0x8007_0216] {
        assert!(
            !com_is_entry_failure(hresult as i32),
            "resource/programming failure {hresult:08X}"
        );
    }
}
