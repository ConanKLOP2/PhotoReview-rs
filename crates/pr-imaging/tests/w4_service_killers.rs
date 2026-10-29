//! CX-IMAGING2 mutation regressions. TempRoot and injected codecs only.
mod support {
    pub mod preview_service;
}

use std::sync::Arc;

use pr_imaging::cache::preview_file::{PreviewCacheError, PreviewPayloadCodec};
use pr_imaging::pixels::{PixelBuffer, PixelBufferError, PixelLayout};
use pr_imaging::service::{PreviewImageService, disk::disk_cache_path};
use pr_test_support::{RecordingLog, TempRoot};
use support::preview_service::{FakeDecoder, Script, TIMEOUT, config};

struct LengthStream(u64);

impl std::io::Read for LengthStream {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        panic!("length guard test must stop before allocating or reading")
    }
}

impl std::io::Seek for LengthStream {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        match position {
            std::io::SeekFrom::End(0) => Ok(self.0),
            std::io::SeekFrom::Start(0) => Err(std::io::Error::other("CX2 accepted length")),
            _ => panic!("unexpected seek"),
        }
    }
}

struct LengthFileSystem(u64);

impl pr_core::abstractions::FileSystem for LengthFileSystem {
    fn open_read_shared(
        &self,
        _: &std::path::Path,
        buffer_size: usize,
    ) -> std::io::Result<Box<dyn pr_core::abstractions::ReadSeek>> {
        assert_eq!(buffer_size, 1);
        Ok(Box::new(LengthStream(self.0)))
    }
    fn file_exists(&self, _: &std::path::Path) -> bool {
        panic!("unexpected filesystem operation")
    }
    fn directory_exists(&self, _: &std::path::Path) -> bool {
        panic!("unexpected filesystem operation")
    }
    fn get_file_stat(&self, _: &std::path::Path) -> Option<pr_core::abstractions::FileStat> {
        panic!("unexpected filesystem operation")
    }
    fn move_file(&self, _: &std::path::Path, _: &std::path::Path) -> std::io::Result<()> {
        panic!("unexpected filesystem operation")
    }
    fn copy_file(&self, _: &std::path::Path, _: &std::path::Path) -> std::io::Result<()> {
        panic!("unexpected filesystem operation")
    }
    fn delete(&self, _: &std::path::Path) -> std::io::Result<()> {
        panic!("unexpected filesystem operation")
    }
    fn open_append(
        &self,
        _: &std::path::Path,
        _: bool,
    ) -> std::io::Result<Box<dyn pr_core::abstractions::AppendStream>> {
        panic!("unexpected filesystem operation")
    }
    fn write_all_text_atomic(&self, _: &std::path::Path, _: &str, _: bool) -> std::io::Result<()> {
        panic!("unexpected filesystem operation")
    }
    fn read_all_text(&self, _: &std::path::Path) -> std::io::Result<String> {
        panic!("unexpected filesystem operation")
    }
    fn enumerate_files(&self, _: &std::path::Path, _: &str) -> std::io::Result<Vec<std::path::PathBuf>> {
        panic!("unexpected filesystem operation")
    }
    fn enumerate_directories(&self, _: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
        panic!("unexpected filesystem operation")
    }
    fn create_directory(&self, _: &std::path::Path) -> std::io::Result<()> {
        panic!("unexpected filesystem operation")
    }
}

struct UnusedCacheReader;

impl pr_imaging::cache::thumbnails::CacheImageReader for UnusedCacheReader {
    fn decode(
        &self,
        _: &[u8],
        _: Option<PixelLayout>,
        _: pr_imaging::decode_types::DecodeBox,
        _: i32,
        _: i32,
    ) -> Result<(PixelBuffer, pr_imaging::cache::thumbnails::CacheImageInfo), pr_imaging::decoder_traits::DecodeError>
    {
        panic!("length guard test must stop before decode")
    }
}

#[test]
fn disk_thumbnail_length_guard_accepts_exact_max_and_rejects_larger_before_allocation() {
    // MISSED cache/thumbnails.rs:709:34: > -> == and > -> >=.
    // C#: ThumbnailCache.DecodeDiskThumbnail length guard. Seeking back is a
    // sentinel error: observe acceptance without a 2 GiB allocation or read.
    use pr_imaging::cache::thumbnails::{MAX_DISK_ENTRY_BYTES, decode_disk_thumbnail};
    use pr_imaging::decoder_traits::DecodeError;
    let root = TempRoot::new("cx2-thumbnail-length");
    for (length, accepted) in [
        (0, false),
        (MAX_DISK_ENTRY_BYTES, true),
        (MAX_DISK_ENTRY_BYTES + 1, false),
        (MAX_DISK_ENTRY_BYTES + 2, false),
    ] {
        let error = decode_disk_thumbnail(
            &root.join("virtual.png"),
            &LengthFileSystem(length),
            &UnusedCacheReader,
            &pr_imaging::pixel_memory::PixelBufferImageCodec::INSTANCE,
        )
        .unwrap_err();
        if accepted {
            assert!(matches!(error, DecodeError::Io(ref inner) if inner.to_string() == "CX2 accepted length"));
        } else {
            assert!(
                matches!(error, DecodeError::InvalidData { ref message } if message == "Thumbnail cache entry has an invalid length.")
            );
        }
    }
}

struct FailingDecoder(std::sync::Mutex<Option<pr_imaging::decoder_traits::DecodeError>>);

impl pr_imaging::decoder_traits::ImageDecoder for FailingDecoder {
    fn decode(
        &self,
        _: &pr_imaging::decode_types::DecodeRequest,
    ) -> Result<Arc<dyn pr_imaging::decode_types::DecodedImageView>, pr_imaging::decoder_traits::DecodeError> {
        Err(self.0.lock().unwrap().take().expect("one decode per test case"))
    }

    fn read_info(
        &self,
        _: &str,
    ) -> Result<pr_imaging::decode_types::ImageInfo, pr_imaging::decoder_traits::DecodeError> {
        panic!("preload does not need read_info")
    }
}

#[test]
fn preload_rebuild_preserves_each_error_kind_message_and_localized_text() {
    // Mutants service/mod.rs:1563,1572: rebuild_decode_error[_kind] body
    // replacement. C# propagates the exception object; Rust reconstructs it.
    use pr_imaging::decoder_traits::DecodeError;
    use pr_imaging::preload::target::PreloadTarget;
    let message = || "CX2 diagnostic".to_owned();
    let errors = [
        DecodeError::MemoryAdmission { message: message() },
        DecodeError::MissingCodec {
            message: message(),
            user_text: None,
        },
        DecodeError::FileNotFound { message: message() },
        DecodeError::DirectoryNotFound { message: message() },
        DecodeError::OutOfMemory { message: message() },
        DecodeError::NotSupported {
            message: message(),
            user_text: None,
            inner_hresult: Some(-17),
        },
        DecodeError::InvalidCast { message: message() },
        DecodeError::Overflow { message: message() },
        DecodeError::Format { message: message() },
        DecodeError::InvalidData { message: message() },
        DecodeError::Com {
            message: message(),
            hresult: -19,
        },
        DecodeError::DllNotFound { message: message() },
        DecodeError::BadImageFormat { message: message() },
        DecodeError::EntryPointNotFound { message: message() },
        DecodeError::InvalidOperation { message: message() },
        DecodeError::InvalidArgument { message: message() },
        DecodeError::Disposed { message: message() },
        DecodeError::Io(std::io::Error::new(std::io::ErrorKind::PermissionDenied, message())),
        DecodeError::Other { message: message() },
    ];
    for error in errors {
        let kind = std::mem::discriminant(error.kind());
        let root = TempRoot::new("cx2-rebuild-kind");
        let source = root.join("source.jpg");
        std::fs::write(&source, [1, 2, 3]).unwrap();
        let error = error
            .with_user_text(Arc::new(|| "CX2 localized".to_owned()))
            .attach_source_bytes(Arc::from([1_u8, 2, 3]));
        let decoder = Arc::new(FailingDecoder(std::sync::Mutex::new(Some(error))));
        let service = Arc::new(PreviewImageService::new(config(&root.join("cache"), decoder)).unwrap());
        let worker_service = service.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = PreloadTarget::preload(
                &*worker_service,
                source.to_str().unwrap(),
                &pr_core::sync::CancelToken::new(),
            );
            tx.send(result).unwrap();
        });
        let error = rx.recv_timeout(TIMEOUT).unwrap().unwrap_err();
        worker.join().unwrap();
        assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
        assert_eq!(std::mem::discriminant(error.kind()), kind);
        assert_eq!(error.message(), message());
        assert_eq!(error.describe(), "CX2 localized");
        assert!(error.source_bytes().is_none());
        match error.kind() {
            DecodeError::Com { hresult, .. } => assert_eq!(*hresult, -19),
            DecodeError::NotSupported { inner_hresult, .. } => assert_eq!(*inner_hresult, Some(-17)),
            DecodeError::Io(inner) => assert_eq!(inner.kind(), std::io::ErrorKind::PermissionDenied),
            _ => {}
        }
    }
}

#[test]
fn pixel_error_display_respects_public_variant_values() {
    // Mutant pixels.rs:76:27: < -> <= in Display. Public error variants can be
    // constructed directly, even when Allocate would not produce this value.
    assert_eq!(
        PixelBufferError::WidthOutOfRange { width: 1 }.to_string(),
        "width ('1') must be less than or equal to '536870911'. (Parameter 'width')\nActual value was 1."
    );
    // Also kills < -> > in the same branch with the killers binary alone.
    assert_eq!(
        PixelBufferError::WidthOutOfRange { width: 0 }.to_string(),
        "width ('0') must be greater than or equal to '1'. (Parameter 'width')\nActual value was 0."
    );
    // Mutant pixels.rs:105:23: < -> <= in Display.
    assert_eq!(
        PixelBufferError::RowOutOfRange { y: 0, height: 0 }.to_string(),
        "y ('0') must be less than '0'. (Parameter 'y')\nActual value was 0."
    );
    assert_eq!(
        PixelBufferError::RowOutOfRange { y: -1, height: 0 }.to_string(),
        "y ('-1') must be a non-negative value. (Parameter 'y')\nActual value was -1."
    );
}

struct BrokenEncoder;

struct DisposedPixelCodec;

impl pr_imaging::decoder_traits::PlatformImageCodec for DisposedPixelCodec {
    fn name(&self) -> &'static str {
        "cx2-disposed"
    }

    fn image_from_pixels(
        &self,
        pixels: PixelBuffer,
    ) -> Result<pr_imaging::decode_types::PlatformImage, pr_imaging::decoder_traits::DecodeError> {
        Ok(pr_imaging::decode_types::PlatformImage::Pixels(Arc::new(pixels)))
    }

    fn to_pixels<'a>(
        &self,
        _: &'a pr_imaging::decode_types::PlatformImage,
    ) -> Result<pr_imaging::decode_types::PixelLease<'a>, pr_imaging::decoder_traits::DecodeError> {
        Err(pr_imaging::decoder_traits::DecodeError::Disposed {
            message: "CX2 pixel conversion disposed".into(),
        })
    }
}

#[test]
fn non_argument_pixel_conversion_error_is_logged_instead_of_silently_skipped() {
    // MISSED service/disk.rs:480:27: match guard -> true in persist_one.
    // C# catches ArgumentException only; ObjectDisposedException is logged.
    let root = TempRoot::new("cx2-pixel-conversion-failure");
    let path = root.join("source.jpg");
    std::fs::write(&path, [1, 2, 3]).unwrap();
    let decoder = FakeDecoder::new();
    decoder.script_default(Script {
        downscaled: true,
        ..Script::default()
    });
    let dir = root.join("cache");
    let log = Arc::new(RecordingLog::new());
    let mut cfg = config(&dir, decoder);
    cfg.disable_disk_cache_override = Some(false);
    cfg.platform_codec = Arc::new(DisposedPixelCodec);
    cfg.log = Some(log.clone());
    let service = PreviewImageService::new(cfg).unwrap();
    let path = path.to_str().unwrap();
    let key = service.get_current_cache_key(path).unwrap();
    assert!(service.get_preview_path(path).wait_timeout(TIMEOUT).unwrap().is_ok());
    assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
    assert!(service.wait_for_prune(TIMEOUT));
    assert!(service.try_get_cached_preview(&key).is_some());
    assert!(!disk_cache_path(&dir, &key).exists());
    assert!(
        log.entries()
            .iter()
            .any(|entry| entry.1.contains("CX2 pixel conversion disposed"))
    );
}

impl PreviewPayloadCodec for BrokenEncoder {
    fn encode_jpeg(&self, _: &PixelBuffer, _: i32, out: &mut dyn std::io::Write) -> std::io::Result<()> {
        out.write_all(b"partial jpeg")?;
        Err(std::io::Error::other("CX2 broken encoder"))
    }

    fn decode_jpeg(&self, _: &[u8], _: PixelLayout, _: i32, _: i32) -> Result<PixelBuffer, PreviewCacheError> {
        panic!("no persisted entry may be read after the encoder failed")
    }
}

#[test]
fn broken_encoder_keeps_ram_result_logs_failure_and_leaves_no_entry_or_temp() {
    // Mutant service/disk.rs:470: persist_one -> Ok(()), error propagation /
    // logging removed in persist worker. C#: PreviewImageServiceFaultTests;
    // encoder writes a partial payload before failing, never touches user data.
    let root = TempRoot::new("cx2-broken-encoder");
    let path = root.join("source.jpg");
    std::fs::write(&path, [1, 2, 3]).unwrap();
    let decoder = FakeDecoder::new();
    decoder.script_default(Script {
        downscaled: true,
        ..Script::default()
    });
    let dir = root.join("cache");
    let log = Arc::new(RecordingLog::new());
    let mut cfg = config(&dir, decoder.clone());
    cfg.disable_disk_cache_override = Some(false);
    cfg.payload_codec = Arc::new(BrokenEncoder);
    cfg.log = Some(log.clone());
    let service = PreviewImageService::new(cfg).unwrap();
    let path = path.to_str().unwrap();
    let key = service.get_current_cache_key(path).unwrap();
    assert!(service.get_preview_path(path).wait_timeout(TIMEOUT).unwrap().is_ok());
    assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
    assert!(service.wait_for_prune(TIMEOUT));
    assert_eq!(decoder.calls(), 1);
    assert!(service.try_get_cached_preview(&key).is_some());
    assert!(!disk_cache_path(&dir, &key).exists());
    assert!(!service.has_disk_cached_preview(&key));
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
    assert!(log.entries().iter().any(|entry| entry.1.contains("CX2 broken encoder")));
}
