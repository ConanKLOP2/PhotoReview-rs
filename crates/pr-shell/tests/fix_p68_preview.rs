//! P-68 (lane fix-p68-preview): `LazyPrewarmPreviews::start_viewer_preview` must not panic when the preview service cannot be built.
//! New tests, no C# twin. The seam is `AppPaths::preview_cache_dir` = blank, which makes `PreviewImageService::new` fail.
#![cfg(windows)]

// This existing fixture exports seams for several registry test binaries.
#[allow(dead_code)]
#[path = "common/composition_fixture.rs"]
mod composition_fixture;

use std::time::Duration;

use pr_app::services::prewarm::PrewarmPreviews;
use pr_core::abstractions::FileStat;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache_key::{ImageCacheKey, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::service::PreviewError;
use pr_shell::composition::adapters::LazyPrewarmPreviews;
use pr_test_support::LogLevel;

fn key() -> ImageCacheKey {
    let stat = FileStat {
        length: 4,
        last_write_utc: DateTimeUtc::parse_json("2026-01-01T00:00:00Z")
            .expect("fixed instant")
            .0,
    };
    ImageCacheKey::create_from_stat(
        r"C:\p68\a.jpg",
        Some(stat),
        false,
        DecodeBox::new(8, 8),
        KeyOptions::default(),
    )
    .expect("key from a stat")
}

#[test]
fn the_seam_makes_preview_service_fail() {
    let (services, _root, _log) = composition_fixture::shared_services_with_failing_preview_service();
    assert!(services.preview_service().is_err());
}

#[test]
fn start_viewer_preview_answers_a_failed_handle_when_the_service_cannot_be_built() {
    let (services, _root, log) = composition_fixture::shared_services_with_failing_preview_service();
    let previews = LazyPrewarmPreviews::new(services);
    let handle = previews.start_viewer_preview(r"C:\p68\a.jpg", &key());
    assert!(handle.is_ready());
    let result = handle
        .wait_timeout(Duration::from_secs(30))
        .unwrap_or_else(|_| panic!("a failed handle must not block"));
    match result {
        Err(PreviewError::Spawn(error)) => assert!(error.to_string().contains("preview_service"), "{error}"),
        Err(other) => panic!("expected PreviewError::Spawn, got {other:?}"),
        Ok(_) => panic!("a preview came out of a service that cannot be built"),
    }
    assert!(log.contains(LogLevel::Error, "preview service could not be created"));
}

#[test]
fn current_cache_key_still_answers_source_no_longer_exists_when_the_service_cannot_be_built() {
    let (services, _root, log) = composition_fixture::shared_services_with_failing_preview_service();
    let previews = LazyPrewarmPreviews::new(services);
    assert!(previews.current_cache_key(r"C:\p68\a.jpg", None).is_err());
    assert!(log.contains(LogLevel::Error, "preview service could not be created"));
}
