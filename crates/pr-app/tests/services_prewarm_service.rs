//! Tests of `pr_app::services::prewarm` against the real `PreviewImageService` with a counting decoder (contract `contracts/W8-04c.md`,
//! Test plan, "Needs the real PreviewImageService"). RED until W4-14 merges: the service's `current_cache_key` and
//! `get_viewer_preview` are stubs, so the prewarm thread panics and `wait` reports it.
//!
//! Port of the three C# service tests of `InitialImagePrewarmTests.cs`. The fakes (`FakeDecoder`, `PixelsCodec`, the payload codec)
//! come from the W4-14a support file, included by path so the decoder fake is not copied (the contract allows a local fake instead).
//! Every wait is bounded by the prewarm's own join (no sleep, no timing).

#[path = "../../pr-imaging/tests/support/preview_service.rs"]
mod preview_service;

use std::path::Path;
use std::sync::{Arc, Mutex};

use pr_app::services::prewarm::{PrewarmPreviews, start};
use pr_core::settings::app_settings::AppSettings;
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::service::PreviewImageService;
use pr_test_support::TempRoot;

use preview_service::{FakeDecoder, box_source, config};

/// The predicted launch box (C# `Predicted`).
const PREDICTED: DecodeBox = DecodeBox::new(1280, 768);
/// The service's own viewport box (C# `Viewport`).
const VIEWPORT: DecodeBox = DecodeBox::new(1920, 1152);

/// A real preview service whose viewport box is `VIEWPORT`, decoding through `decoder`, with the disk cache off.
fn service(dir: &Path, decoder: Arc<FakeDecoder>) -> Arc<PreviewImageService> {
    let mut cfg = config(dir, decoder);
    cfg.target_decode_box = box_source(Arc::new(Mutex::new(VIEWPORT)));
    Arc::new(PreviewImageService::new(cfg).expect("the preview service builds"))
}

/// Coerces the real service to the trait object `start` takes.
fn previews_of(service: &Arc<PreviewImageService>) -> Option<Arc<dyn PrewarmPreviews>> {
    let previews: Arc<dyn PrewarmPreviews> = service.clone();
    Some(previews)
}

/// Writes a small file into `dir` and returns its absolute path (C# `CreateFile`).
fn write_file(dir: &Path, name: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, [1u8, 2, 3, 4]).expect("write the test file");
    path.to_string_lossy().into_owned()
}

// C#: InitialImagePrewarmTests.Start_WebpFile_FollowsTheWebpHeicSetting
// WebP is prewarmed only when the WebP/HEIC setting is on.
#[test]
fn webp_file_follows_the_webp_heic_setting() {
    let root = TempRoot::new("prewarm_webp");
    let path = write_file(root.path(), "pic.webp");
    let decoder = FakeDecoder::new();
    let svc = service(root.path(), decoder.clone());

    let off = AppSettings {
        webp_heic_support_enabled: false,
        ..AppSettings::default()
    };
    assert!(start(previews_of(&svc), &off, Some(path.as_str()), None).is_none());

    let on = AppSettings {
        webp_heic_support_enabled: true,
        ..AppSettings::default()
    };
    let task = start(previews_of(&svc), &on, Some(path.as_str()), None).expect("WebP is prewarmed when enabled");
    task.wait().expect("the prewarm thread ends normally");
    assert_eq!(decoder.calls(), 1);
}

// C#: InitialImagePrewarmTests.Start_WithExplicitBox_DecodesAndCachesUnderThatBox
// An explicit box keys the decode: the entry is cached under that box and not under the viewport box.
#[test]
fn explicit_box_decodes_and_caches_under_that_box() {
    let root = TempRoot::new("prewarm_explicit");
    let path = write_file(root.path(), "launch.jpg");
    let decoder = FakeDecoder::new();
    let svc = service(root.path(), decoder.clone());

    let task = start(
        previews_of(&svc),
        &AppSettings::default(),
        Some(path.as_str()),
        Some(PREDICTED),
    )
    .expect("the launch file is prewarmed");
    task.wait().expect("the prewarm thread ends normally");

    assert_eq!(decoder.calls(), 1);
    let last = decoder
        .requests()
        .last()
        .map(|request| DecodeBox::new(request.target_width, request.target_height));
    assert_eq!(last, Some(PREDICTED), "the decode requested the predicted box");

    let predicted_key = svc
        .get_current_cache_key_for_box(&path, PREDICTED)
        .expect("key for the predicted box");
    assert!(
        svc.try_get_cached_preview(&predicted_key).is_some(),
        "cached under the predicted box"
    );

    let viewport_key = svc.get_current_cache_key(&path).expect("key for the viewport box");
    assert!(
        svc.try_get_cached_preview(&viewport_key).is_none(),
        "not cached under the viewport box"
    );
}

// C#: InitialImagePrewarmTests.Start_WithoutBox_DecodesAndCachesUnderTheViewportBox
// Without a box the decode is keyed with the service's current viewport box.
#[test]
fn without_box_decodes_and_caches_under_the_viewport_box() {
    let root = TempRoot::new("prewarm_no_box");
    let path = write_file(root.path(), "launch.jpg");
    let decoder = FakeDecoder::new();
    let svc = service(root.path(), decoder.clone());

    let task = start(previews_of(&svc), &AppSettings::default(), Some(path.as_str()), None)
        .expect("the launch file is prewarmed");
    task.wait().expect("the prewarm thread ends normally");

    assert_eq!(decoder.calls(), 1);
    let viewport_key = svc.get_current_cache_key(&path).expect("key for the viewport box");
    assert!(
        svc.try_get_cached_preview(&viewport_key).is_some(),
        "cached under the viewport box"
    );

    let predicted_key = svc
        .get_current_cache_key_for_box(&path, PREDICTED)
        .expect("key for the predicted box");
    assert!(
        svc.try_get_cached_preview(&predicted_key).is_none(),
        "not cached under the predicted box"
    );
}
