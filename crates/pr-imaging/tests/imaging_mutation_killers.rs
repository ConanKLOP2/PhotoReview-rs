//! Deterministic regressions for the postmerge Wave 4/5 pure imaging mutation debt.
mod support {
    pub mod preview_service;
}
use pr_core::time::DateTimeUtc;
use pr_imaging::adaptive_preview::{calculate_target_decode_box, quantize};
use pr_imaging::cache::thumbnails::{build_key, build_stamp};
use pr_imaging::cache_key::{ImageCacheKey, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::memory_headroom::{self as memory, MemoryInfo};
use pr_test_support::{FakeFileSystem, TempRoot};

#[test]
fn service_persists_only_downscaled_non_degraded_previews() {
    // Mutants: service/decode.rs:448/462-464, delete ! and && -> || in publish/persist policy.
    use pr_imaging::service::PreviewImageService;
    use pr_imaging::service::disk::disk_cache_path;
    use support::preview_service::{FakeDecoder, Script, TIMEOUT, config};
    for (downscaled, degraded) in [(false, false), (true, true), (false, true)] {
        let root = TempRoot::new("imaging-mutation-persist-policy");
        let path = root.path().join("source.jpg");
        std::fs::write(&path, [1, 2, 3]).unwrap();
        let decoder = FakeDecoder::new();
        decoder.script_default(Script {
            downscaled,
            degraded,
            ..Script::default()
        });
        let dir = root.path().join("cache");
        let mut cfg = config(&dir, decoder);
        cfg.disable_disk_cache_override = Some(false);
        let service = PreviewImageService::new(cfg).unwrap();
        let key = service.get_current_cache_key(&path.to_string_lossy()).unwrap();
        assert!(
            service
                .get_preview_path(&path.to_string_lossy())
                .wait_timeout(TIMEOUT)
                .unwrap()
                .is_ok()
        );
        assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
        assert!(!disk_cache_path(&dir, &key).exists());
        assert_eq!(service.cache_count(), if degraded { 0 } else { 1 });
    }
}

#[test]
fn service_disk_hit_avoids_decode_and_corrupt_entry_falls_back() {
    // Mutants: service/decode.rs:420-465, disk branch source_read assignment, !degraded and persist predicates.
    use pr_imaging::service::PreviewImageService;
    use pr_imaging::service::disk::disk_cache_path;
    use std::sync::Arc;
    use support::preview_service::{FakeDecoder, Script, TIMEOUT, config};
    let root = TempRoot::new("imaging-mutation-disk");
    let path = root.path().join("source.jpg");
    std::fs::write(&path, [1, 2, 3, 4, 5, 6, 7]).unwrap();
    let dir = root.path().join("cache");
    let make = || {
        let decoder = FakeDecoder::new();
        decoder.script_default(Script {
            downscaled: true,
            ..Script::default()
        });
        let mut cfg = config(&dir, decoder.clone());
        cfg.disable_disk_cache_override = Some(false);
        let metrics = Arc::clone(&cfg.metrics);
        (PreviewImageService::new(cfg).unwrap(), decoder, metrics)
    };
    let (writer, decoder, metrics) = make();
    let key = writer.get_current_cache_key(&path.to_string_lossy()).unwrap();
    assert!(
        writer
            .get_preview_path(&path.to_string_lossy())
            .wait_timeout(TIMEOUT)
            .unwrap()
            .is_ok()
    );
    assert_eq!(decoder.calls(), 1);
    assert_eq!(metrics.snapshot().source_reads, 1);
    assert!(writer.shutdown_persist_workers_timeout(TIMEOUT));
    let entry = disk_cache_path(&dir, &key);
    assert!(entry.is_file());
    drop(writer);
    let (reader, decoder, metrics) = make();
    assert!(
        reader
            .get_preview_path(&path.to_string_lossy())
            .wait_timeout(TIMEOUT)
            .unwrap()
            .is_ok()
    );
    assert_eq!(decoder.calls(), 0);
    assert_eq!(metrics.snapshot().source_reads, 0);
    assert_eq!(metrics.snapshot().disk_cache_hits, 1);
    assert!(reader.shutdown_persist_workers_timeout(TIMEOUT));
    drop(reader);
    // Mutant: service/decode.rs:420:31, cache-entry-failure match guard -> true must not swallow OOM.
    {
        use pr_imaging::cache::preview_file::{PreviewCacheError, PreviewPayloadCodec};
        use pr_imaging::pixels::{PixelBuffer, PixelLayout};
        use pr_imaging::service::PreviewError;
        struct OutOfMemoryCodec;
        impl PreviewPayloadCodec for OutOfMemoryCodec {
            fn encode_jpeg(&self, _: &PixelBuffer, _: i32, _: &mut dyn std::io::Write) -> std::io::Result<()> {
                panic!("a failed disk read must not queue a write")
            }
            fn decode_jpeg(&self, _: &[u8], _: PixelLayout, _: i32, _: i32) -> Result<PixelBuffer, PreviewCacheError> {
                Err(PreviewCacheError::OutOfMemory("fake memory exhaustion".into()))
            }
        }
        let original_entry = std::fs::read(&entry).unwrap();
        let decoder = FakeDecoder::new();
        let mut cfg = config(&dir, decoder.clone());
        cfg.disable_disk_cache_override = Some(false);
        cfg.payload_codec = Arc::new(OutOfMemoryCodec);
        let reader = PreviewImageService::new(cfg).unwrap();
        let result = reader
            .get_preview_path(&path.to_string_lossy())
            .wait_timeout(TIMEOUT)
            .unwrap();
        assert!(matches!(result, Err(PreviewError::DiskEntry(_))));
        assert_eq!(decoder.calls(), 0);
        assert!(reader.shutdown_persist_workers_timeout(TIMEOUT));
        assert_eq!(std::fs::read(&entry).unwrap(), original_entry);
    }
    std::fs::write(&entry, [1, 2, 3, 4]).unwrap();
    let (reader, decoder, metrics) = make();
    assert!(
        reader
            .get_preview_path(&path.to_string_lossy())
            .wait_timeout(TIMEOUT)
            .unwrap()
            .is_ok()
    );
    assert_eq!(decoder.calls(), 1);
    assert_eq!(metrics.snapshot().source_reads, 1);
    assert_eq!(metrics.snapshot().disk_cache_hits, 0);
    assert!(reader.shutdown_persist_workers_timeout(TIMEOUT));
}

#[test]
fn thumbnail_disk_decode_preserves_metadata_and_cache_path() {
    // Mutants: cache/thumbnails.rs:686 PathBuf default; :732-736 option field deletions.
    use pr_core::model::DecoderBackend;
    use pr_imaging::cache::thumbnails::{CacheImageInfo, CacheImageReader, cache_file_path, decode_disk_thumbnail};
    use pr_imaging::decode_types::DecodedImageView;
    use pr_imaging::decoder_traits::DecodeError;
    use pr_imaging::pixels::{PixelBuffer, PixelLayout};
    struct Reader;
    impl CacheImageReader for Reader {
        fn decode(
            &self,
            encoded: &[u8],
            layout: Option<PixelLayout>,
            decode_box: DecodeBox,
            width: i32,
            height: i32,
        ) -> Result<(PixelBuffer, CacheImageInfo), DecodeError> {
            assert_eq!(encoded, [1, 2, 3]);
            assert_eq!(layout, None);
            assert_eq!(decode_box, DecodeBox::new(800, 0));
            assert_eq!((width, height), (0, 0));
            Ok((
                PixelBuffer::allocate(2, 3, PixelLayout::Bgr32).unwrap(),
                CacheImageInfo {
                    source_width: 4000,
                    source_height: 6000,
                    downscaled: true,
                },
            ))
        }
    }
    let root = TempRoot::new("imaging-mutation-thumbnail");
    let path = cache_file_path(root.path(), "abc");
    assert_eq!(path, root.path().join("abc.png"));
    let fs = FakeFileSystem::new();
    fs.add_file_at(
        &path,
        &[1, 2, 3],
        DateTimeUtc::from_ticks(638_000_000_000_000_000).unwrap(),
    );
    let image = decode_disk_thumbnail(&path, &fs, &Reader, &support::preview_service::PixelsCodec).unwrap();
    assert!(image.downscaled());
    assert_eq!(image.orientation(), 1);
    assert_eq!(image.actual_backend(), DecoderBackend::WicDirect);
    assert_eq!((image.original_width(), image.original_height()), (4000, 6000));
}

#[test]
fn service_source_byte_counter_falls_back_to_key_length() {
    // Mutants: service/decode.rs:648, source_bytes_read -> 0, 1, -1.
    use pr_imaging::service::PreviewImageService;
    use std::sync::Arc;
    use support::preview_service::{FakeDecoder, TIMEOUT, config};
    let root = TempRoot::new("imaging-mutation-service");
    let path = root.path().join("source.jpg");
    std::fs::write(&path, [1, 2, 3, 4, 5, 6, 7]).unwrap();
    let decoder = FakeDecoder::new();
    let cfg = config(&root.path().join("cache"), decoder);
    let metrics = Arc::clone(&cfg.metrics);
    let service = PreviewImageService::new(cfg).unwrap();
    let handle = service.get_preview_path(&path.to_string_lossy());
    assert!(handle.wait_timeout(TIMEOUT).unwrap().is_ok());
    assert_eq!(metrics.snapshot().source_bytes_read, 7);
}

#[test]
fn adaptive_quantization_preserves_epsilon_caps_and_aspect() {
    // Mutants: adaptive_preview.rs:57-86, scale arithmetic, quantize division/subtraction/multiplication.
    for (side, expected) in [
        (0.0, 128),
        (128.0, 128),
        (128.0 + 1e-8, 128),
        (128.0 + 1e-6, 256),
        (3990.0, 4000),
        (f64::INFINITY, 4000),
        (f64::NAN, 0),
    ] {
        assert_eq!(quantize(side), expected);
    }
    assert_eq!(
        calculate_target_decode_box(300.0, 600.0, 1.0, 1.0).unwrap(),
        DecodeBox::new(640, 1280)
    );
    assert_eq!(
        calculate_target_decode_box(900.0, 600.0, 2.0, 1.5).unwrap(),
        DecodeBox::new(2816, 1920)
    );
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        for index in 0..4 {
            let mut args = [1.0; 4];
            args[index] = bad;
            let err = calculate_target_decode_box(args[0], args[1], args[2], args[3]).unwrap_err();
            assert_eq!(
                err.param_name,
                ["viewportWidth", "viewportHeight", "dpiScale", "qualityMultiplier"][index]
            );
        }
    }
}

#[test]
fn thumbnail_stamp_and_digest_pin_every_identity_field() {
    // Mutants: cache/thumbnails.rs:667-680, string replacement and nibble shifts/masks.
    assert_eq!(build_stamp("photo.jpg", 17, -23), "photo.jpg|17|-23|800|orient=1");
    let stamp = build_stamp("photo.jpg", 17, -23);
    let expected: String = pr_core::session::store::sha256(stamp.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(build_key("photo.jpg", 17, -23), expected);
    for (path, length, ticks) in [("Photo.jpg", 17, -23), ("photo.jpg", 18, -23), ("photo.jpg", 17, -22)] {
        assert_ne!(build_key(path, length, ticks), expected);
    }
}

#[test]
fn cache_identity_detects_length_only_time_only_and_missing_source() {
    // Mutants: cache_key.rs:310-319, bool replacement, == inversion and && -> ||.
    let root = TempRoot::new("imaging-mutation-identity");
    let path = root.path().join("identity.jpg").to_string_lossy().into_owned();
    let fs = FakeFileSystem::new();
    let time = DateTimeUtc::from_ticks(638_000_000_000_000_000).unwrap();
    fs.add_file_at(&path, &[1, 2, 3], time);
    let key = ImageCacheKey::create(&fs, &path, false, DecodeBox::new(512, 384), KeyOptions::default()).unwrap();
    // Mutants: cache_key.rs:120/:286-298, incidental struct field deletions emitted despite -F.
    let moved_path = root.path().join("moved.jpg").to_string_lossy().into_owned();
    let moved = key.with_path(&moved_path).unwrap();
    assert_ne!(moved.path(), key.path());
    assert_eq!(moved.length(), key.length());
    let original = ImageCacheKey::create_original(&key);
    assert!(original.is_original());
    assert_eq!(original.target_box(), DecodeBox::UNBOUNDED);
    let raw = ImageCacheKey::create_original_with_kind(&key, 2);
    assert_eq!(raw.source_kind(), 2);
    assert!(key.matches_current_source(&fs));
    fs.add_file_at(&path, &[1, 2, 3, 4], time);
    assert!(!key.matches_current_source(&fs));
    fs.add_file_at(&path, &[1, 2, 3], DateTimeUtc::from_ticks(time.ticks() + 1).unwrap());
    assert!(!key.matches_current_source(&fs));
    assert!(!key.matches_current_source(&FakeFileSystem::new()));
}

#[test]
fn memory_budget_preserves_exact_fit_unknown_total_and_negative_load() {
    // Mutants: memory_headroom.rs:65/69/70, bool replacement, <= inversion, - -> + or /.
    for (estimate, total, load, expected) in [
        (60, 100, 40, true),
        (61, 100, 40, false),
        (100, 100, -10, true),
        (101, 100, -10, false),
        (0, 100, 101, false),
        (i64::MAX, 0, 10, true),
        (i64::MAX, -1, 10, true),
    ] {
        assert_eq!(memory::has_headroom(estimate, total, load), expected);
        let source = || MemoryInfo {
            total_available: total,
            load,
        };
        assert_eq!(memory::has_headroom_from(&source, estimate), expected);
    }
}

#[test]
fn output_peak_doubles_and_rejects_overflow_with_known_total() {
    // Mutants: memory_headroom.rs:51-55, ! removal, / -> % or *, * -> + or /, bool replacement.
    for (buffer, total, load, expected) in [
        (30, 100, 40, true),
        (31, 100, 40, false),
        (-1, 100, 0, false),
        (-1, 0, 0, true),
        (i64::MAX / 2, i64::MAX, 0, true),
        (i64::MAX / 2 + 1, i64::MAX, 0, false),
        (i64::MAX, -1, 0, true),
    ] {
        assert_eq!(memory::output_has_headroom(buffer, total, load), expected);
        let source = || MemoryInfo {
            total_available: total,
            load,
        };
        assert_eq!(memory::output_has_headroom_from(&source, buffer), expected);
    }
}

#[test]
fn memory_guard_threshold_is_128_mib() {
    // Mutants: memory_headroom.rs:12, constant multiplication -> addition or division.
    assert_eq!(memory::GUARD_THRESHOLD_BYTES, 134_217_728);
}
