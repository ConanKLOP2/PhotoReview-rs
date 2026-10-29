mod support {
    pub mod main_env;
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

mod file_action_fakes;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use pr_app::coordinators::presenter::state::{ImagePresenter, ImagePresenterDeps};
use pr_app::coordinators::zoom_detail::{ThreadRunner, ZoomDetailEffect, ZoomDetailLoader, same_platform_image};
use pr_core::catalog::generation::GenerationClock;
use pr_core::file_actions::rename::PathKeyMap;
use pr_core::io::navigation_stat::NavigationStatWorker;
use pr_core::model::enums::DecoderBackend;
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodedImage, DecodedImageOptions};
use pr_imaging::service::PreviewImageService;
use pr_test_support::TempRoot;
use support::main_env::MainEnv;
use support::present_helpers::{ScriptedFs, present, start};
use support::presenter_fakes::{
    FakeHasher, FixtureOptions, GatedScheduler, PresenterFixture, RecordingSink, build_thumbnail_cache,
};
use support::preview_fakes::{Gate, ServiceOptions, SizedDecoder, build_service, sized_image};

struct LoaderFixture {
    root: TempRoot,
    clock: Arc<GenerationClock>,
    decoder: Arc<SizedDecoder>,
    service: Arc<PreviewImageService>,
    loader: ZoomDetailLoader,
    shown: Vec<pr_imaging::decode_types::PlatformImage>,
    wakes: Arc<AtomicUsize>,
    /// Signalled after every wake so `finish` can wait on the wake counter itself: a worker queues its completion first and calls the wake callback
    /// afterwards, so draining the queue between the two left `wakes < expected` with nothing queued and `wait_for_event` waited for an event that never came.
    wake_signal: Arc<(Mutex<()>, Condvar)>,
}

impl LoaderFixture {
    fn new() -> Self {
        let root = TempRoot::new("rename-retarget");
        let decoder = SizedDecoder::new();
        let service = build_service(&root.join("cache"), decoder.clone(), ServiceOptions::default());
        let clock = Arc::new(GenerationClock::default());
        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_count = Arc::clone(&wakes);
        let wake_signal = Arc::new((Mutex::new(()), Condvar::new()));
        let signal = Arc::clone(&wake_signal);
        Self {
            root,
            clock: Arc::clone(&clock),
            decoder,
            service: Arc::clone(&service),
            loader: ZoomDetailLoader::new(
                service,
                clock,
                Arc::new(ThreadRunner),
                Arc::new(move || {
                    wake_count.fetch_add(1, Ordering::SeqCst);
                    // Taking the lock before notifying means a waiter that checked the counter cannot miss this wake.
                    let _guard = signal.0.lock().unwrap_or_else(PoisonError::into_inner);
                    signal.1.notify_all();
                }),
            ),
            shown: Vec::new(),
            wakes,
            wake_signal,
        }
    }

    fn file(&self, name: &str) -> String {
        let path = self.root.join(name);
        if !path.exists() {
            std::fs::write(&path, [0xff, 0xd8, 0xff, 0xd9]).unwrap_or_else(|error| panic!("write fixture: {error}"));
        }
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap_or_else(|error| panic!("open fixture: {error}"));
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000))
            .unwrap_or_else(|error| panic!("set fixture timestamp: {error}"));
        path.to_string_lossy().into_owned()
    }

    fn present(&mut self, name: &str) -> (i64, String) {
        let path = self.file(name);
        let token = self.clock.next_navigation();
        let key = self
            .service
            .get_current_cache_key(&path)
            .unwrap_or_else(|error| panic!("cache key: {error:?}"));
        self.loader
            .on_preview_presented(token, &path, &key, &sized_image(600, 400, 6000, 4000, true));
        (token, path)
    }

    fn drain(&mut self) {
        for effect in self.loader.pump() {
            if let ZoomDetailEffect::ShowImage { image, .. } = effect {
                self.shown.push(image);
            }
        }
    }

    fn finish(&mut self, expected_completions: usize) {
        let deadline = Instant::now() + support::preview_fakes::TIMEOUT;
        while self.wakes.load(Ordering::SeqCst) < expected_completions
            || self.loader.pending_load()
            || self.loader.queued_events() != 0
        {
            assert!(
                Instant::now() < deadline,
                "zoom completions did not settle: expected {expected_completions}, current load: {}",
                self.loader.pending_load()
            );
            if self.loader.queued_events() == 0 {
                // Hang guard (AGENTS.md: no fixed delays): wait until a completion was announced or is queued.
                let guard = self.wake_signal.0.lock().unwrap_or_else(PoisonError::into_inner);
                let (_guard, timeout) = self
                    .wake_signal
                    .1
                    .wait_timeout_while(guard, support::preview_fakes::TIMEOUT, |_| {
                        self.wakes.load(Ordering::SeqCst) < expected_completions && self.loader.queued_events() == 0
                    })
                    .unwrap_or_else(PoisonError::into_inner);
                assert!(
                    !timeout.timed_out(),
                    "expected {expected_completions} zoom completions; current load: {}",
                    self.loader.pending_load()
                );
            }
            self.drain();
        }
    }
}

#[test]
fn retarget_path_without_target_is_a_no_op() {
    // C#: ZoomDetailLoaderRetargetTests.RetargetPath_WithoutTarget_IsANoOp
    let mut f = LoaderFixture::new();
    let path = f.file("b.jpg");
    f.loader.retarget_path(f.clock.current_navigation(), &path);
    assert!(!f.loader.pending_load());
    assert_eq!(f.decoder.total_original_decodes(), 0);
}

#[test]
fn retarget_path_at_fit_does_not_decode_until_the_viewer_zooms() {
    // C#: ZoomDetailLoaderRetargetTests.RetargetPath_AtFit_DoesNotDecodeUntilTheViewerZooms
    let mut f = LoaderFixture::new();
    f.present("a.jpg");
    let path = f.file("b.jpg");
    f.clock.stop_for_action();
    f.loader.retarget_path(f.clock.current_navigation(), &path);
    assert!(!f.loader.pending_load());
    assert_eq!(f.decoder.total_original_decodes(), 0);
    f.loader.set_zoom(Some(1.0));
    assert!(f.loader.pending_load());
}

#[test]
fn retarget_path_after_stop_for_action_and_zoom_decodes_the_original_via_the_new_path_only() {
    // C#: ZoomDetailLoaderRetargetTests.RetargetPath_AfterStopForActionAndZoom_DecodesTheOriginalViaTheNewPathOnly
    let mut f = LoaderFixture::new();
    f.present("a.jpg");
    let new_path = f.file("b.jpg");
    f.clock.stop_for_action();
    f.loader.retarget_path(f.clock.current_navigation(), &new_path);
    f.loader.set_zoom(Some(1.0));
    f.finish(1);
    assert_eq!(f.decoder.total_original_decodes(), 1);
    assert_eq!(f.decoder.original_decodes("b.jpg"), 1);
    assert!(f.loader.is_showing_original());
}

#[test]
fn retarget_path_while_zoomed_without_original_restarts_the_decode_under_the_new_path() {
    // C#: ZoomDetailLoaderRetargetTests.RetargetPath_WhileZoomedWithoutOriginal_RestartsTheDecodeUnderTheNewPath
    let mut f = LoaderFixture::new();
    let old_gate = f.decoder.gate("a.jpg");
    f.loader.set_zoom(Some(1.0));
    f.present("a.jpg");
    assert!(f.decoder.wait_started("a.jpg"));
    let new_path = f.file("b.jpg");
    f.clock.stop_for_action();
    f.loader.retarget_path(f.clock.current_navigation(), &new_path);
    old_gate.open();
    assert!(f.decoder.wait_started("b.jpg"));
    f.finish(2);
    assert!(f.decoder.total_original_decodes() >= 1);
    assert!(f.loader.is_showing_original());
}

#[test]
fn retarget_path_decode_in_flight_for_the_old_path_its_pixels_are_never_shown() {
    // C#: ZoomDetailLoaderRetargetTests.RetargetPath_DecodeInFlightForTheOldPath_ItsPixelsAreNeverShown
    let mut f = LoaderFixture::new();
    let old_gate = f.decoder.gate("a.jpg");
    f.loader.set_zoom(Some(1.0));
    f.present("a.jpg");
    assert!(f.decoder.wait_started("a.jpg"));
    let path = f.file("b.jpg");
    f.clock.stop_for_action();
    f.loader.retarget_path(f.clock.current_navigation(), &path);
    old_gate.open();
    f.finish(2);
    assert_eq!(f.decoder.original_decodes("b.jpg"), 1);
    assert!(
        f.loader.is_showing_original(),
        "the new path decode was not accepted: {:#?}",
        f.loader
    );
    assert_eq!(f.shown.len(), 1, "the stale old-path pixels were dropped");
}

#[test]
fn retarget_path_held_original_stays_held_and_shown_without_a_new_decode() {
    // C#: ZoomDetailLoaderRetargetTests.RetargetPath_HeldOriginal_StaysHeldAndShownWithoutANewDecode
    let mut f = LoaderFixture::new();
    f.present("a.jpg");
    f.loader.set_zoom(Some(1.0));
    f.finish(1);
    let held = f.loader.held_original().map(|image| image.platform_image().clone());
    let count = f.decoder.total_original_decodes();
    let path = f.file("b.jpg");
    f.clock.stop_for_action();
    f.loader.retarget_path(f.clock.current_navigation(), &path);
    assert!(
        held.as_ref()
            .zip(f.loader.held_original())
            .is_some_and(|(held, current)| same_platform_image(held, current.platform_image()))
    );
    assert_eq!(count, f.decoder.total_original_decodes());
}

#[test]
fn retarget_path_held_original_zoom_out_and_in_again_still_works_without_decoding() {
    // C#: ZoomDetailLoaderRetargetTests.RetargetPath_HeldOriginal_ZoomOutAndInAgainStillWorksWithoutDecoding
    let mut f = LoaderFixture::new();
    f.present("a.jpg");
    f.loader.set_zoom(Some(1.0));
    f.finish(1);
    let path = f.file("b.jpg");
    f.clock.stop_for_action();
    f.loader.retarget_path(f.clock.current_navigation(), &path);
    f.loader.set_zoom(None);
    assert!(!f.loader.is_showing_original());
    f.loader.set_zoom(Some(1.0));
    assert!(f.loader.is_showing_original());
    assert_eq!(f.decoder.total_original_decodes(), 1);
}

#[test]
fn retarget_path_after_failed_decode_failure_stays_latched_for_the_same_pixels() {
    // C#: ZoomDetailLoaderRetargetTests.RetargetPath_AfterFailedDecode_FailureStaysLatchedForTheSamePixels
    let mut f = LoaderFixture::new();
    f.decoder.fail("a.jpg");
    f.present("a.jpg");
    f.loader.set_zoom(Some(1.0));
    f.finish(1);
    let path = f.file("b.jpg");
    f.clock.stop_for_action();
    f.loader.retarget_path(f.clock.current_navigation(), &path);
    f.loader.set_zoom(Some(2.0));
    assert_eq!(f.decoder.total_original_decodes(), 1);
}

#[test]
fn retarget_path_new_present_afterwards_resets_target_normally() {
    // C#: ZoomDetailLoaderRetargetTests.RetargetPath_NewPresentAfterwards_ResetsTargetNormally
    let mut f = LoaderFixture::new();
    f.present("a.jpg");
    let path = f.file("b.jpg");
    f.clock.stop_for_action();
    f.loader.retarget_path(f.clock.current_navigation(), &path);
    f.present("c.jpg");
    f.loader.set_zoom(Some(1.0));
    f.finish(1);
    assert_eq!(f.decoder.original_decodes("c.jpg"), 1);
}

#[test]
fn stop_for_action_without_retarget_zoom_detail_is_dead_and_nothing_decodes() {
    // C#: ZoomDetailLoaderRetargetTests.StopForAction_WithoutRetarget_ZoomDetailIsDeadAndNothingDecodes
    let mut f = LoaderFixture::new();
    f.present("a.jpg");
    f.clock.stop_for_action();
    f.loader.set_zoom(Some(1.0));
    assert!(!f.loader.pending_load());
    assert_eq!(f.decoder.total_original_decodes(), 0);
}

#[test]
fn retarget_path_shown_file_renamed_rekeys_path_info_and_status_without_presenting_stat_or_decode() {
    // C#: ImagePresenterTests.RetargetPath_ShownFileRenamed_RekeysPathInfoAndStatusWithoutPresentingStatOrDecode
    let decoder = SizedDecoder::new();
    let file_system = ScriptedFs::new();
    let options = FixtureOptions {
        file_system: Some(file_system.clone()),
        ..FixtureOptions::default()
    };
    let mut f = PresenterFixture::with_options(decoder.clone(), &["a.jpg"], options);
    present(&mut f, 0);
    let old = f.presenter.current_presented_path().unwrap_or_default().to_owned();
    let new = f.root.join("renamed.jpg").to_string_lossy().into_owned();
    let before_images = f
        .sink_log
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .images
        .len();
    let before_stats = file_system.stat_count(&old);
    let before_decodes = decoder.total_original_decodes();
    let map = [(old.clone(), new.clone())].into_iter().collect::<PathKeyMap>();
    let ctx = f.format.ctx();
    let (kept, _) = f.presenter.retarget_path(&map, &ctx);
    assert!(kept);
    assert_eq!(f.presenter.current_presented_path(), Some(new.as_str()));
    assert_eq!(
        f.presenter.current_photo_info().map(|info| info.file_name.as_str()),
        Some("renamed.jpg")
    );
    assert!(f.presenter.status_text().ends_with("renamed.jpg"));
    assert_eq!(
        f.sink_log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .images
            .len(),
        before_images
    );
    assert_eq!(file_system.stat_count(&old), before_stats);
    assert_eq!(decoder.total_original_decodes(), before_decodes);
}

#[test]
fn retarget_path_present_in_flight_returns_false_so_the_caller_presents_again() {
    // C#: ImagePresenterTests.RetargetPath_PresentInFlight_ReturnsFalseSoTheCallerPresentsAgain
    let mut f = PresenterFixture::new(SizedDecoder::new(), &["a.jpg", "b.jpg"]);
    start(&mut f, 0);
    assert!(f.presenter.is_present_in_flight());
    let map = PathKeyMap::new();
    let ctx = f.format.ctx();
    let (kept, _) = f.presenter.retarget_path(&map, &ctx);
    assert!(!kept);
}

#[test]
fn retarget_path_superseded_present_stays_in_flight_until_its_frame_continuation_observes_completion() {
    // C#: ImagePresenterTests.RetargetPath_PresentInFlight_ReturnsFalseSoTheCallerPresentsAgain
    let decoder = SizedDecoder::new();
    let scheduler = GatedScheduler::new();
    let mut f = PresenterFixture::with_options(
        decoder,
        &["a.jpg", "b.jpg"],
        FixtureOptions {
            scheduler: Some(scheduler.clone()),
            ..FixtureOptions::default()
        },
    );
    start(&mut f, 0);
    support::present_helpers::pump_one(&mut f); // initial stat starts the preview
    let deadline = std::time::Instant::now() + support::preview_fakes::TIMEOUT;
    while scheduler.posted_count() < 1 {
        assert!(
            std::time::Instant::now() < deadline,
            "old present did not reach PostFrame"
        );
        support::present_helpers::pump_one(&mut f);
    }

    start(&mut f, 1);
    support::present_helpers::pump_one(&mut f);
    let new = f.path("b.jpg");
    while scheduler.posted_count() < 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "new present did not reach PostFrame"
        );
        support::present_helpers::pump_one(&mut f);
    }
    assert!(scheduler.release(1), "release the current navigation frame only");
    while f.presenter.status_text().contains("Loading") {
        assert!(
            std::time::Instant::now() < deadline,
            "new present did not finish its refresh stat"
        );
        support::present_helpers::pump_one(&mut f);
    }
    assert_eq!(
        f.metrics.snapshot().presented_images,
        1,
        "the current present completed before the old frame was released"
    );
    assert!(
        f.presenter.is_present_in_flight(),
        "the superseded present is still awaiting its PostFrame continuation"
    );

    let renamed = f.root.join("renamed.jpg").to_string_lossy().into_owned();
    let map = [(new, renamed)].into_iter().collect::<PathKeyMap>();
    let ctx = f.format.ctx();
    assert!(!f.presenter.retarget_path(&map, &ctx).0);

    assert!(scheduler.release(0), "release the superseded navigation frame");
    support::present_helpers::pump_one(&mut f);
    assert!(
        !f.presenter.is_present_in_flight(),
        "the retired present is complete after its await resumes"
    );
}

#[test]
fn retarget_path_canceled_queued_initial_stat_completes_without_touching_the_filesystem() {
    // C#: ImagePresenterTests.PresentAsync_SupersededWhileStatPending_SuccessfulStatResumesQuietly
    let worker = NavigationStatWorker::new();
    let worker_started = Gate::new();
    let worker_release = Gate::new();
    let started = Arc::clone(&worker_started);
    let release = Arc::clone(&worker_release);
    drop(worker.run(
        move || {
            started.open();
            release.wait()
        },
        &CancelToken::none(),
    ));
    assert!(worker_started.wait(), "the stat worker is held by its first job");

    let file_system = ScriptedFs::new();
    let mut f = PresenterFixture::new(SizedDecoder::new(), &["old.jpg", "current.jpg"]);
    let (sink, sink_log) = RecordingSink::new();
    let counter = Arc::clone(&f.wakes);
    f.sink_log = sink_log;
    f.presenter = ImagePresenter::new(
        ImagePresenterDeps {
            clock: Arc::clone(&f.clock),
            preview_service: Arc::clone(&f.service),
            thumbnail_cache: build_thumbnail_cache(&f.root.join("thumbs-stat-lifecycle")),
            preload: Arc::clone(&f.preload) as _,
            hasher: FakeHasher::new("HASH"),
            metrics: Arc::clone(&f.metrics),
            session_store: None,
            session_writer: None,
            sink: Box::new(sink),
            file_system: Some(file_system.clone()),
            on_presented_hook: None,
            ui_scheduler: None,
            busy_retry_delay: None,
            runner: None,
            stat_worker: Some(worker),
            wake: Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
            indicator_delay: None,
        },
        pr_app::viewmodels::compare::CompareViewModel::new(&f.format.ctx()),
    );
    let old = f.path("old.jpg");
    let current = f.path("current.jpg");
    start(&mut f, 0);
    start(&mut f, 1);
    assert!(f.presenter.is_present_in_flight());

    worker_release.open();
    f.pump_until_idle();
    assert_eq!(
        file_system.stat_count(&old),
        0,
        "the canceled queued stat never touched the file system"
    );
    assert_eq!(
        file_system.stat_count(&current),
        2,
        "the current image gets its initial and refresh stats"
    );
    assert_eq!(f.presenter.current_presented_path(), Some(current.as_str()));
    assert!(f.presenter.current_image().is_some());
    assert_eq!(f.metrics.snapshot().presented_images, 1);
    assert!(
        !f.presenter.is_present_in_flight(),
        "abandonment retires the superseded continuation"
    );
}

#[test]
fn missing_pipeline_started_by_present_counts_until_the_missing_next_stat_resumes() {
    // C#: ImagePresenterTests.PresentAsync_MissingCurrent_RemovesItAndPresentsTheNextEntry
    let file_system = ScriptedFs::new();
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["missing.jpg", "next.jpg"],
        FixtureOptions {
            file_system: Some(file_system.clone()),
            ..FixtureOptions::default()
        },
    );
    let missing = f.path("missing.jpg");
    let next = f.path("next.jpg");
    file_system.script(&missing, |_| Ok(None));
    file_system.hold(&next);
    start(&mut f, 0);
    support::present_helpers::pump_one(&mut f);
    assert!(file_system.wait_started(&next));
    assert!(
        f.presenter.is_present_in_flight(),
        "PresentAsync awaits the missing-removal loop"
    );

    file_system.release(&next);
    f.pump_until_idle();
    assert_eq!(f.presenter.current_presented_path(), Some(next.as_str()));
    assert!(!f.presenter.is_present_in_flight());
}

#[test]
fn direct_missing_removal_does_not_count_as_a_present_until_it_starts_the_next_present() {
    // C#: ImagePresenterTests.RemoveMissingCatalogItemAsync_StandaloneCallDoesNotIncrementPresentAsyncCounter
    let file_system = ScriptedFs::new();
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["removed.jpg", "next.jpg"],
        FixtureOptions {
            file_system: Some(file_system.clone()),
            ..FixtureOptions::default()
        },
    );
    let removed = f.path("removed.jpg");
    let next = f.path("next.jpg");
    file_system.hold(&next);
    let token = f.clock.current_navigation();
    {
        let mut env = pr_app::coordinators::presenter::state::PresenterEnv {
            catalog: &mut f.catalog,
            settings: &f.settings,
            session: f.session.as_mut(),
            format: f.format.ctx(),
        };
        f.presenter.remove_missing_catalog_item(&mut env, &removed, token);
    }
    assert!(file_system.wait_started(&next));
    assert!(
        !f.presenter.is_present_in_flight(),
        "standalone missing removal is not PresentAsync"
    );
    file_system.release(&next);
    f.pump_until_idle();
    assert!(f.presenter.current_image().is_some());
}

#[test]
fn retarget_path_compare_visible_clears_compare_and_returns_false_but_rekeys_file_identity() {
    // C#: ImagePresenterTests.RetargetPath_CompareVisible_ClearsCompareAndReturnsFalseButRekeysFileIdentity
    let mut f = PresenterFixture::new(SizedDecoder::new(), &["a.jpg"]);
    present(&mut f, 0);
    let old = f.presenter.current_presented_path().unwrap_or_default().to_owned();
    let new = f.root.join("renamed.jpg").to_string_lossy().into_owned();
    std::fs::write(&new, [0xff, 0xd8, 0xff, 0xd9]).unwrap_or_else(|error| panic!("write renamed fixture: {error}"));
    let comparison = f.presenter.compare_mut();
    comparison.set_left_path(Some(old.clone()));
    comparison.set_right_path(Some(new.clone()));
    let image = |size| {
        Arc::new(
            DecodedImage::new(
                pr_imaging::decode_types::PlatformImage::Opaque(Arc::new(0_u8)),
                size,
                size,
                i64::from(size) * i64::from(size) * 4,
                DecodedImageOptions {
                    downscaled: false,
                    orientation: 1,
                    actual_backend: DecoderBackend::Wpf,
                    original_width: size,
                    original_height: size,
                    exif: None,
                    is_degraded_fallback: false,
                },
            )
            .unwrap_or_else(|error| panic!("comparison image: {error}")),
        )
    };
    comparison.set_left_image(Some(image(1)));
    comparison.set_right_image(Some(image(2)));
    comparison.set_selected_path(Some(old.clone()));
    comparison.set_left_size_text("left size".to_owned());
    comparison.set_right_size_text("right size".to_owned());
    comparison.set_hash_text("hash result".to_owned());
    comparison.set_status_text("compare status".to_owned());
    comparison.toggle();
    let map = [(old, new)].into_iter().collect::<PathKeyMap>();
    let ctx = f.format.ctx();
    let (kept, _) = f.presenter.retarget_path(&map, &ctx);
    assert!(!kept);
    assert!(!f.presenter.compare().is_visible());
    assert_eq!(f.presenter.compare().left_path(), None);
    assert_eq!(f.presenter.compare().right_path(), None);
    assert!(f.presenter.compare().left_image().is_none());
    assert!(f.presenter.compare().right_image().is_none());
    assert_eq!(f.presenter.compare().selected_path(), None);
    assert!(!f.presenter.compare().is_left_selected());
    assert!(!f.presenter.compare().is_right_selected());
    assert_eq!(f.presenter.compare().left_size_text(), "");
    assert_eq!(f.presenter.compare().right_size_text(), "");
    assert_eq!(f.presenter.compare().status_text(), "");
    assert_ne!(f.presenter.compare().hash_text(), "hash result");
    let new_entry = pr_core::catalog::types::CatalogEntry::new(
        map.iter().next().map(|(_, path)| path.as_str()).unwrap_or_default(),
    )
    .unwrap_or_else(|error| panic!("renamed catalog entry: {error:?}"));
    f.catalog
        .reset(vec![new_entry], pr_core::model::enums::RawPairMode::Separate, None);
    present(&mut f, 0);
    let flags = f
        .sink_log
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .file_change_flags();
    assert_eq!(
        flags.last(),
        Some(&false),
        "the next present is still the same file identity"
    );
}

#[test]
fn rename_current_async_rekeys_the_session_and_persists_it() {
    // C#: MainViewModelRenameTests.RenameCurrentAsync_RekeysTheSessionAndPersistsIt
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_session", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    env.vm.rename_current("renamed");
    env.settle();
    let renamed = std::path::Path::new(&folder)
        .join("renamed.jpg")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        env.vm.session().and_then(|session| session.current_path.as_deref()),
        Some(renamed.as_str())
    );
    env.session_writer.flush();
    let stored = env.sessions.load(&folder).unwrap_or_default();
    assert_eq!(stored.current_path.as_deref(), Some(renamed.as_str()));
}

#[test]
fn rename_current_async_rekeys_the_skipped_list() {
    // C#: MainViewModelRenameTests.RenameCurrentAsync_RekeysTheSkippedList
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_skipped", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    env.vm.skip();
    env.settle();
    env.vm.first();
    env.settle();
    env.vm.rename_current("renamed");
    env.settle();
    let renamed = std::path::Path::new(&folder)
        .join("renamed.jpg")
        .to_string_lossy()
        .into_owned();
    assert!(
        env.vm
            .session()
            .is_some_and(|session| session.skipped.iter().any(|path| path == &renamed))
    );
}

#[test]
fn rename_current_async_while_an_explorer_order_is_pending_waits_for_it_then_renames() {
    // C#: MainViewModelRenameTests.RenameCurrentAsync_WhileAnExplorerOrderIsPending_WaitsForItThenRenames
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_pending_order", &["a.jpg", "b.jpg", "c.jpg"]);
    env.explorer.set_order(files.clone());
    env.vm
        .open_folder(&folder, Some(&files[1]))
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("present before Explorer order", &["Explorer"], |_| false);
    env.vm.rename_current("renamed");
    env.settle_holding("rename remains behind Explorer order", &["Explorer"], |_| false);
    let renamed = std::path::Path::new(&folder).join("renamed.jpg");
    assert!(std::path::Path::new(&files[1]).exists());
    assert!(!renamed.exists());
    assert!(env.runner.run_tagged("Explorer"));
    env.settle();
    assert!(!std::path::Path::new(&files[1]).exists());
    assert!(renamed.exists());
}
