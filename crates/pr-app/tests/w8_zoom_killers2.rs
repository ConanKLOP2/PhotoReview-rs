//! Deterministic mailbox and zoom-loader regressions for CX-ZOOM's remaining mutation inventory.

#[path = "support/preview_fakes.rs"]
mod preview_fakes;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pr_app::coordinators::zoom_detail::{
    BackgroundJob, BackgroundRunner, CompletionInbox, RAW_INDICATOR_DELAY, ZoomDetailEffect, ZoomDetailLoader,
    same_platform_image,
};
use pr_core::catalog::generation::GenerationClock;
use pr_imaging::decode_types::PlatformImage;
use pr_test_support::TempRoot;

#[test]
fn completion_inbox_preserves_order_shared_identity_and_counts_each_wake() {
    // C#: ZoomDetailLoader UI completion scheduling seam.
    // Mutants: zoom_detail.rs:74 clone -> Default; :100 send -> (); :110 drain -> vec![];
    // :116 len -> 0/1; :121 is_empty -> true/false and == -> !=; :127/:133 wait_ready guards.
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = wakes.clone();
    let inbox = CompletionInbox::new(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    assert!(inbox.is_empty());
    assert_eq!(inbox.len(), 0);
    assert!(!inbox.wait_ready(Duration::ZERO));
    let shared = inbox.clone();
    shared.send(17);
    inbox.send(29);
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    assert_eq!(inbox.len(), 2);
    assert!(!shared.is_empty());
    assert!(shared.wait_ready(Duration::ZERO));
    assert_eq!(shared.drain(), [17, 29]);
    assert!(inbox.drain().is_empty());
    assert!(!inbox.wait_ready(Duration::ZERO));
}

#[test]
fn superseded_raw_indicator_message_cannot_show_the_current_slots_indicator() {
    // C#: ZoomDetailLoader.ShowIndicatorAfterDelayAsync.ShowIfCurrent.
    // Mutants: zoom_detail.rs:510 next_id -> 0/1; :705 && -> ||, == -> !=, delete !.
    let root = TempRoot::new("cx-zoom-stale-indicator");
    let file = root.join("image.nef");
    std::fs::write(&file, [1, 2, 3]).expect("fixture source");
    let path = file.to_string_lossy().into_owned();
    let decoder = preview_fakes::SizedDecoder::new();
    decoder.set_original(60, 40);
    let service = preview_fakes::build_service(
        &root.join("cache"),
        decoder.clone(),
        preview_fakes::ServiceOptions {
            raw_full_decoder: Some(decoder.clone()),
            raw_full_decode_enabled: true,
            ..Default::default()
        },
    );
    let clock = Arc::new(GenerationClock::default());
    let runner = Arc::new(QueuedRunner::default());
    let mut loader = ZoomDetailLoader::new(service.clone(), clock.clone(), runner.clone(), Arc::new(|| {}))
        .with_indicator_delay(Arc::new(|delay, _| {
            assert_eq!(delay, Duration::from_millis(300));
            true
        }));
    let key = service.get_current_cache_key(&path).expect("source identity");
    let preview = preview_fakes::sized_image(6, 4, 60, 40, true);
    loader.on_preview_presented(clock.next_navigation(), &path, &key, &preview);
    loader.set_zoom(Some(1.0));
    runner.next(); // Old timer's event is queued, but not applied.
    loader.on_preview_presented(clock.next_navigation(), &path, &key, &preview);
    assert!(loader.pump().is_empty());
    assert!(!loader.is_raw_decode_indicator_visible());
    assert!(loader.pending_load());
    runner.next(); // Old decode was cancelled before it could read the source.
    assert!(loader.pump().is_empty());
    assert_eq!(decoder.original_decodes("image.nef"), 0);
    assert!(loader.pending_load());
    runner.next(); // Current timer.
    assert!(matches!(
        loader.pump().as_slice(),
        [ZoomDetailEffect::RawDecodeIndicatorChanged { visible: true }]
    ));
    assert!(loader.is_raw_decode_indicator_visible());
    assert!(matches!(
        loader.reset().as_slice(),
        [ZoomDetailEffect::RawDecodeIndicatorChanged { visible: false }]
    ));
    runner.next(); // Cancelled current decode is harmless, too.
    assert!(loader.pump().is_empty());
    assert_eq!(decoder.original_decodes("image.nef"), 0);
}

#[test]
fn platform_image_identity_uses_allocation_for_pixels_and_opaque_images() {
    // C#: ReferenceEquals in ZoomDetailLoader.Update.
    // Mutants: zoom_detail.rs:140 same_platform_image -> true/false; :141/:142 delete match arm.
    let first = preview_fakes::sized_image(2, 3, 2, 3, false).platform_image().clone();
    let second = preview_fakes::sized_image(2, 3, 2, 3, false).platform_image().clone();
    let opaque = PlatformImage::Opaque(Arc::new(17_u32));
    let another = PlatformImage::Opaque(Arc::new(17_u32));
    assert!(same_platform_image(&first, &first.clone()));
    assert!(!same_platform_image(&first, &second));
    assert!(same_platform_image(&opaque, &opaque.clone()));
    assert!(!same_platform_image(&opaque, &another));
    assert!(!same_platform_image(&first, &opaque));
    assert!(!same_platform_image(&opaque, &first));
}

#[derive(Default)]
struct QueuedRunner(Mutex<VecDeque<BackgroundJob>>);

impl BackgroundRunner for QueuedRunner {
    fn run(&self, _: &'static str, job: BackgroundJob) {
        self.0.lock().expect("job queue").push_back(job);
    }
}

impl QueuedRunner {
    fn next(&self) {
        let job = self.0.lock().expect("job queue").pop_front().expect("queued job");
        job();
    }
}

#[test]
fn retarget_discards_a_success_already_in_the_inbox_before_cancellation() {
    // C#: ZoomDetailLoader.RetargetPath / LoadAsync ReferenceEquals(target, _target).
    // Mutants: zoom_detail.rs:306/:307/:308/:309 delete Target id/token/path/key fields.
    // The old service call has ALREADY succeeded when retarget cancels its token: post-decode
    // cancellation checks cannot rewrite the Ok result stored in the loader's inbox.
    let root = TempRoot::new("cx-zoom-retarget-queued-success");
    let old_file = root.join("old.jpg");
    let new_file = root.join("new.jpg");
    std::fs::write(&old_file, [1, 2, 3]).expect("fixture source");
    let old_path = old_file.to_string_lossy().into_owned();
    let new_path = new_file.to_string_lossy().into_owned();
    let decoder = preview_fakes::SizedDecoder::new();
    decoder.set_original(60, 40);
    let service = preview_fakes::build_service(&root.join("cache"), decoder.clone(), Default::default());
    let clock = Arc::new(GenerationClock::default());
    let runner = Arc::new(QueuedRunner::default());
    let mut loader = ZoomDetailLoader::new(service.clone(), clock.clone(), runner.clone(), Arc::new(|| {}));
    let key = service.get_current_cache_key(&old_path).expect("source identity");
    loader.on_preview_presented(
        clock.next_navigation(),
        &old_path,
        &key,
        &preview_fakes::sized_image(6, 4, 60, 40, true),
    );
    loader.set_zoom(Some(1.0));
    runner.next();
    assert_eq!(loader.queued_events(), 1);
    assert_eq!(decoder.original_decodes("old.jpg"), 1);
    std::fs::rename(&old_file, &new_file).expect("rename owned fixture");
    assert!(loader.retarget_path(clock.next_navigation(), &new_path).is_empty());
    assert!(loader.pump().is_empty());
    assert!(
        loader.held_original().is_none(),
        "the queued success belongs to the old target identity"
    );
    assert!(loader.pending_load());
    runner.next();
    let effects = loader.pump();
    assert!(
        matches!(effects.as_slice(), [ZoomDetailEffect::ShowImage { original_width: 60, original_height: 40, .. }, ZoomDetailEffect::OriginalShown { path }] if path == &new_path)
    );
    assert!(loader.held_original().is_some());
    assert!(!loader.pending_load());
    assert_eq!(decoder.original_decodes("new.jpg"), 1);
    // The image can still be shown with the old source key: observe the dimensions cache,
    // which must be seeded under the renamed identity rather than the now-missing old path.
    let renamed_key = service
        .get_current_cache_key(&new_path)
        .expect("renamed source identity");
    assert_eq!(service.try_get_known_original_dimensions(&renamed_key), Some((60, 40)));
}

#[test]
fn raw_indicator_and_decode_completions_have_exact_queue_and_visibility_transitions() {
    // C#: ZoomDetailLoader.RawDecodeIndicatorChanged and Reset.
    // Mutants: zoom_detail.rs:358/:363/:368/:373/:378/:383 getters; :490 pump -> Default;
    // :510 next_id -> 0/1; :663 start_raw_decode_indicator -> (); :684 stop -> ();
    // :693 set_raw_decode_indicator_visible -> () and == -> !=.
    let root = TempRoot::new("cx-zoom-raw-queue");
    let file = root.join("image.nef");
    std::fs::write(&file, [1, 2, 3]).expect("fixture source");
    let path = file.to_string_lossy().into_owned();
    let decoder = preview_fakes::SizedDecoder::new();
    // Keep allocations small while exercising the RAW-specific routing and dimensions.
    decoder.set_original(60, 40);
    let service = preview_fakes::build_service(
        &root.join("cache"),
        decoder.clone(),
        preview_fakes::ServiceOptions {
            raw_full_decoder: Some(decoder),
            raw_full_decode_enabled: true,
            ..Default::default()
        },
    );
    let clock = Arc::new(GenerationClock::default());
    let runner = Arc::new(QueuedRunner::default());
    let delays = Arc::new(AtomicUsize::new(0));
    let observed = delays.clone();
    let mut loader = ZoomDetailLoader::new(service.clone(), clock.clone(), runner.clone(), Arc::new(|| {}))
        .with_indicator_delay(Arc::new(move |delay, cancel| {
            assert_eq!(delay, RAW_INDICATOR_DELAY);
            assert!(!cancel.is_canceled());
            observed.fetch_add(1, Ordering::SeqCst);
            true
        }));
    let key = service.get_current_cache_key(&path).expect("source identity");
    let preview = preview_fakes::sized_image(6, 4, 60, 40, true);
    loader.on_preview_presented(clock.next_navigation(), &path, &key, &preview);
    assert!(loader.held_original().is_none());
    assert!(!loader.pending_load());
    assert!(!loader.is_showing_original());
    assert!(!loader.is_raw_decode_indicator_visible());
    assert_eq!(loader.queued_events(), 0);
    assert!(!loader.wait_for_event(Duration::ZERO));
    assert!(loader.set_zoom(Some(1.0)).is_empty());
    assert!(loader.pending_load());
    runner.next(); // Fake indicator delay, no wall clock.
    assert_eq!(delays.load(Ordering::SeqCst), 1);
    assert_eq!(loader.queued_events(), 1);
    assert!(loader.wait_for_event(Duration::ZERO));
    assert!(matches!(
        loader.pump().as_slice(),
        [ZoomDetailEffect::RawDecodeIndicatorChanged { visible: true }]
    ));
    assert!(loader.is_raw_decode_indicator_visible());
    assert_eq!(loader.queued_events(), 0);
    assert!(loader.pump().is_empty());
    runner.next(); // Decode finishes before the UI pumps it.
    assert_eq!(loader.queued_events(), 1);
    assert!(loader.pending_load());
    let effects = loader.pump();
    assert!(matches!(
        effects.as_slice(),
        [
            ZoomDetailEffect::RawDecodeIndicatorChanged { visible: false },
            ZoomDetailEffect::ShowImage {
                original_width: 60,
                original_height: 40,
                ..
            },
            ZoomDetailEffect::OriginalShown { .. }
        ]
    ));
    assert!(!loader.pending_load());
    assert!(loader.is_showing_original());
    assert!(loader.held_original().is_some());
    assert!(!loader.is_raw_decode_indicator_visible());
    assert!(loader.reset().is_empty());
    assert!(loader.held_original().is_none());
    assert!(!loader.is_showing_original());
    assert!(loader.pump().is_empty());
}
