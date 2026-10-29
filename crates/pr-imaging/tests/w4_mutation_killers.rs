//! Deterministic follow-up for Wave 4 mutation gaps; no native codec or user data.
mod support {
    pub mod preview_service;
}

use std::sync::Arc;

use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::pixels::fits_in_span;
use pr_imaging::preload::target::PreloadTarget;
use pr_imaging::service::PreviewImageService;
use pr_test_support::TempRoot;
use support::preview_service::{FakeDecoder, Script, TIMEOUT, config};

#[test]
fn thumbnail_size_guard_rejects_invalid_sizes_before_any_allocation() {
    // Mutants embedded_thumbnail.rs:36: body -> true (initial timeout), positivity
    // comparisons, && -> ||, product * -> / or +, and <= -> < or ==.
    // C#: EmbeddedThumbnailReader.IsAcceptableSize; test only the predicate.
    use pr_imaging::embedded_thumbnail::is_acceptable_size;
    for (width, height, expected) in [
        (1, 1, true),
        (2048, 2048, true),
        (2048, 2049, false),
        (0, 1, false),
        (1, 0, false),
        (-1, 1, false),
        (1, -1, false),
        (-1, -1, false),
        (100_000, 100_000, false),
        (i32::MAX, i32::MAX, false),
    ] {
        assert_eq!(is_acceptable_size(width, height), expected, "{width}x{height}");
    }
}

#[test]
fn span_boundary_accepts_exact_maximum_without_allocation() {
    // Mutant pixels.rs:141: <= -> <, >, ==; fits_in_span -> true/false.
    assert!(fits_in_span(i64::from(i32::MAX)));
    assert!(fits_in_span(0));
    assert!(!fits_in_span(i64::from(i32::MAX) + 1));
}

#[test]
fn seeded_preload_adapter_observes_ram_and_disk_and_cancellation() {
    // Mutants service/mod.rs:1507-1554: cached lookup -> false/true, bytes -> None,
    // preload cancellation delete/negate, has_disk_cached_preview -> false/true.
    let root = TempRoot::new("w4-seeded-preload");
    let source = root.path().join("source.jpg");
    std::fs::write(&source, [1, 2, 3, 4]).unwrap();
    let decoder = FakeDecoder::new();
    decoder.script_default(Script {
        downscaled: true,
        ..Script::default()
    });
    let mut cfg = config(&root.path().join("cache"), decoder.clone());
    cfg.disable_disk_cache_override = Some(false);
    let service = PreviewImageService::new(cfg).unwrap();
    let path = source.to_str().unwrap();
    let key = service.get_current_cache_key(path).unwrap();
    let target: &dyn PreloadTarget = &service;
    assert!(!target.try_get_cached_preview_for_path(path));
    assert!(!target.try_get_cached_preview(&key));
    assert_eq!(target.cached_preview_bytes(&key), None);
    assert!(!target.has_disk_cached_preview(&key));
    assert_eq!(target.cache_count(), 0);
    assert_eq!(target.cache_bytes(), 0);
    assert_eq!(target.active_viewer_decodes(), 0);
    assert!(service.get_preview_path(path).wait_timeout(TIMEOUT).unwrap().is_ok());
    assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
    assert!(target.try_get_cached_preview_for_path(path));
    assert!(target.try_get_cached_preview(&key));
    assert_eq!(target.cached_preview_bytes(&key), Some(4 * 3 * 4));
    assert!(target.has_disk_cached_preview(&key));
    assert_eq!(target.cache_count(), 1);
    assert_eq!(target.cache_bytes(), 4 * 3 * 4);
    assert_eq!(target.active_viewer_decodes(), 0);
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(matches!(target.preload(path, &cancel), Err(DecodeError::Canceled)));
    assert_eq!(decoder.calls(), 1);
}

struct LocalizedFailure;

#[test]
fn preload_adapter_reports_active_viewer_while_decoder_is_blocked() {
    // MISSED mutant service/mod.rs:1546:9: active_viewer_decodes -> 0.
    // C#: IPreloadTarget.ActiveViewerDecodes; a fake gate observes the active
    // state without elapsed-time assertions, sleep, or polling.
    let root = TempRoot::new("w4-active-viewer-adapter");
    let source = root.path().join("viewer.jpg");
    std::fs::write(&source, [1, 2, 3]).unwrap();
    let decoder = FakeDecoder::new();
    decoder.block();
    let service = PreviewImageService::new(config(&root.path().join("cache"), decoder.clone())).unwrap();
    let path = source.to_str().unwrap();
    let key = service.get_current_cache_key(path).unwrap();
    let pending = service.get_viewer_preview(path, &key, &CancelToken::new(), std::time::Duration::ZERO);
    let started = decoder.wait_started("viewer.jpg");
    let active = PreloadTarget::active_viewer_decodes(&service);
    decoder.release();
    assert!(pending.wait_timeout(TIMEOUT).unwrap().is_ok());
    assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
    assert!(started);
    assert_eq!(active, 1);
    assert_eq!(PreloadTarget::active_viewer_decodes(&service), 0);
}

impl ImageDecoder for LocalizedFailure {
    fn decode(&self, _: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Err(DecodeError::InvalidData {
            message: "invariant diagnostic".into(),
        }
        .with_user_text(Arc::new(|| "localized sentence".into()))
        .attach_source_bytes(Arc::from([1_u8, 2, 3])))
    }

    fn read_info(&self, _: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(64, 48))
    }
}

#[test]
fn preload_rebuild_preserves_localized_sentence_and_kind_but_discards_bytes() {
    // Mutant service/mod.rs:1563: rebuild_decode_error loses user_text; no C# twin
    // for rebuilding the Rust non-Clone enum (C# propagates the exception object).
    let root = TempRoot::new("w4-localized-rebuild");
    let source = root.path().join("source.jpg");
    std::fs::write(&source, [1, 2, 3]).unwrap();
    let service =
        Arc::new(PreviewImageService::new(config(&root.path().join("cache"), Arc::new(LocalizedFailure))).unwrap());
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let worker_service = service.clone();
    let worker = std::thread::spawn(move || {
        let error =
            PreloadTarget::preload(&*worker_service, source.to_str().unwrap(), &CancelToken::new()).unwrap_err();
        tx.send(error).unwrap();
    });
    let error = rx.recv_timeout(TIMEOUT).unwrap();
    worker.join().unwrap();
    assert!(matches!(error.kind(), DecodeError::InvalidData { message } if message == "invariant diagnostic"));
    assert_eq!(error.describe(), "localized sentence");
    assert_eq!(error.message(), "invariant diagnostic");
    assert!(error.source_bytes().is_none());
    assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
}
