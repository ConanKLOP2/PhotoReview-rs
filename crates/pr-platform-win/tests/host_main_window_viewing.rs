//! Tests of W8-13j: `ShellMainWindow.Viewing.cs` (image changes with crossfade, the hub queue, composer and prefetcher lifetime, GPU cache clear, compare
//! hit-testing). The pure functions of the unit are tested without a window. The window-level tests are `#[ignore = "native: needs desktop"]`: a real UI
//! thread, a hidden (never shown, never activated) shell window and a WARP offscreen surface; every wait is bounded, no sleeps.
//!
//! The C# `ShellViewingTests` run through the whole startup (W8-17e); the window-level twins are named in `// C#:` comments. `render_now` (the first-image draw)
//! belongs to 13i: the tests here check the state the viewing glue produces, not the frame.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/main_window_fixture.rs"]
mod main_window_fixture;
#[path = "common/main_window_viewing_fixture.rs"]
mod main_window_viewing_fixture;
#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use main_window_fixture::{built, finish, run, start_ui, wait_until};
use main_window_viewing_fixture::{built_with_renderer, draw_one_frame, solid};
use pr_app::input::primitives::PointD;
use pr_app::viewmodels::main::{ChangeSet, ImageChangingEventArgs};
use pr_app::viewport::render_geometry::ImageInterpolation;
use pr_core::abstractions::{UiDispatcher, UiPriority};
use pr_core::model::enums::ImageTransition;
use pr_core::settings::app_settings::{AppSettings, MAX_IMAGE_TRANSITION_MS, MIN_IMAGE_TRANSITION_MS};
use pr_imaging::decode_types::PlatformImage;
use pr_platform_win::host::main_window::viewing::{
    DEFER_UPLOAD_BYTES, compare_retained_ids, crossfade_duration, frame_contains_assigned_image,
};
use pr_platform_win::shell_viewing::compare::layout;
use pr_platform_win::shell_viewing::composer::ComposedFrame;

// ---- pure ---------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn defer_upload_threshold_is_16_mib() {
    assert_eq!(DEFER_UPLOAD_BYTES, 16_777_216);
}

// C#: ShellViewingTests.ImageChanging_ThatIsNotAFileChange_NeverStartsACrossfade (the transition-off half) and the clamp of OnImageChanging
#[test]
fn crossfade_duration_is_none_when_the_transition_is_off_and_clamped_otherwise() {
    assert_eq!(crossfade_duration(ImageTransition::None, 200), None);
    assert_eq!(crossfade_duration(ImageTransition::None, 0), None);
    assert_eq!(
        crossfade_duration(ImageTransition::Fade, 200),
        Some(Duration::from_millis(200))
    );
    assert_eq!(
        crossfade_duration(ImageTransition::Fade, MIN_IMAGE_TRANSITION_MS),
        Some(Duration::from_millis(40))
    );
    assert_eq!(
        crossfade_duration(ImageTransition::Fade, MAX_IMAGE_TRANSITION_MS),
        Some(Duration::from_millis(400))
    );
    assert_eq!(
        crossfade_duration(ImageTransition::Fade, 0),
        Some(Duration::from_millis(40))
    );
    assert_eq!(
        crossfade_duration(ImageTransition::Fade, 39),
        Some(Duration::from_millis(40))
    );
    assert_eq!(
        crossfade_duration(ImageTransition::Fade, 401),
        Some(Duration::from_millis(400))
    );
    assert_eq!(
        crossfade_duration(ImageTransition::Fade, i32::MAX),
        Some(Duration::from_millis(400))
    );
    assert_eq!(
        crossfade_duration(ImageTransition::Fade, i32::MIN),
        Some(Duration::from_millis(40))
    );
}

fn frame(main_drawn: bool, compare_images: i32) -> ComposedFrame {
    ComposedFrame {
        main_drawn,
        crossfade_drawn: false,
        compare_images,
        interpolation: ImageInterpolation::Linear,
        degraded: false,
    }
}

// C#: ShellViewingTests.FrameDrawnWhileAnUpgradeIsStillUploading_DoesNotCountAsTheNewImagePresented
#[test]
fn a_frame_counts_for_the_assigned_image_only_when_the_drawn_image_is_that_buffer() {
    let current = solid(4, 1);
    let upgrade = solid(4, 2);
    // The presenter assigned `upgrade`, but the frame still has `current`: it does not count.
    assert!(!frame_contains_assigned_image(
        &frame(true, 0),
        Some(&current),
        Some(&upgrade)
    ));
    // The very same buffer: it counts.
    assert!(frame_contains_assigned_image(
        &frame(true, 0),
        Some(&current),
        Some(&current)
    ));
    // A distinct buffer with the same pixels is not the same image (reference equality).
    let copy = solid(4, 1);
    assert!(!frame_contains_assigned_image(
        &frame(true, 0),
        Some(&current),
        Some(&copy)
    ));
    // Nothing was drawn.
    assert!(!frame_contains_assigned_image(
        &frame(false, 0),
        Some(&current),
        Some(&current)
    ));
    // No composer image and no hub image: only a drawn main image makes it count.
    assert!(!frame_contains_assigned_image(&frame(false, 0), None, None));
    assert!(frame_contains_assigned_image(&frame(true, 0), None, None));
    // One side missing.
    assert!(!frame_contains_assigned_image(&frame(true, 0), Some(&current), None));
    assert!(!frame_contains_assigned_image(&frame(true, 0), None, Some(&current)));
    // Compare images count even when the main image does not.
    assert!(frame_contains_assigned_image(
        &frame(false, 1),
        Some(&current),
        Some(&upgrade)
    ));
    assert!(frame_contains_assigned_image(
        &frame(true, 2),
        Some(&current),
        Some(&upgrade)
    ));
}

// C#: UpdateRetained (the compare part): ids of the bitmaps that exist and are not disposed
#[test]
fn compare_retained_ids_skip_missing_and_disposed_images() {
    let left = solid(4, 1);
    let right = solid(4, 2);
    assert_eq!(
        compare_retained_ids([Some(&left), Some(&right)]),
        [left.id(), right.id()]
    );
    assert_eq!(compare_retained_ids([Some(&left), None]), [left.id()]);
    assert_eq!(compare_retained_ids([None, Some(&right)]), [right.id()]);
    assert!(compare_retained_ids([None, None]).is_empty());
}

// equivalent/native gap: `MainWindowPrefetchHost::path_at` requires a live MainWindowInner and a non-empty
// ReviewCatalog. The current main_window_fixture builds an empty MainViewModel catalog; fabricating a
// parallel catalog here would not exercise the real host method, so the surviving path_at mutant needs
// a fixture extension that can seed the real view model before construction.

// ---- native -------------------------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs desktop"]
fn composer_and_prefetcher_exist_between_attach_renderer_and_dispose() {
    let ui = start_ui();
    let (before, after_attach, after_dispose) = run(&ui, || {
        let b = main_window_fixture::build(main_window_fixture::BuildOptions::default());
        let before = (b.window.composer().is_some(), b.window.prefetcher().is_some());
        let (w, h) = b.shell.client_size_pixels();
        b.window
            .attach_renderer(Some(&main_window_fixture::WarpHook::new(w, h)))
            .unwrap();
        let after_attach = (b.window.composer().is_some(), b.window.prefetcher().is_some());
        b.window.dispose();
        let after_dispose = (b.window.composer().is_some(), b.window.prefetcher().is_some());
        (before, after_attach, after_dispose)
    });
    assert_eq!(before, (false, false));
    assert_eq!(after_attach, (true, true));
    assert_eq!(after_dispose, (false, false));
    finish(ui);
}

// Trap T1: the hub handler runs inside a view-model call (the view model is borrowed): it only queues.
#[test]
#[ignore = "native: needs desktop"]
fn hub_callback_during_a_view_model_borrow_only_queues() {
    let ui = start_ui();
    let (current_during, current_after_drain, image_id, composer_id) = run(&ui, || {
        let b = built_with_renderer();
        let image = solid(8, 7);
        let platform = PlatformImage::Pixels(Arc::clone(&image));
        let during = {
            let _borrow = b.window.view_model().borrow_mut();
            b.hub.raise(Some(&platform), true);
            b.window.composer().unwrap().current().map(|c| c.id())
        };
        b.window.apply_changes(&ChangeSet::default());
        let after = b.window.composer().unwrap().current().map(|c| c.id());
        (during, after, image.id(), after)
    });
    assert_eq!(
        current_during, None,
        "nothing is handled while the view model is borrowed"
    );
    assert_eq!(current_after_drain, Some(image_id));
    assert_eq!(composer_id, Some(image_id));
    finish(ui);
}

// Trap T2: images are handled FIFO, after the borrow ended; the last one wins.
#[test]
#[ignore = "native: needs desktop"]
fn images_are_handled_fifo_and_the_last_one_is_current() {
    let ui = start_ui();
    let (current, second_id) = run(&ui, || {
        let b = built_with_renderer();
        let first = PlatformImage::Pixels(solid(8, 1));
        let second_buffer = solid(8, 2);
        let second = PlatformImage::Pixels(Arc::clone(&second_buffer));
        b.hub.raise(Some(&first), true);
        b.hub.raise(Some(&second), true);
        b.window.apply_changes(&ChangeSet::default());
        (
            b.window.composer().unwrap().current().map(|c| c.id()),
            second_buffer.id(),
        )
    });
    assert_eq!(current, Some(second_id));
    finish(ui);
}

// C#: HandleImageChanged: an empty folder / compare mode reports no image
#[test]
#[ignore = "native: needs desktop"]
fn a_hub_report_without_an_image_clears_the_current_image() {
    let ui = start_ui();
    let current = run(&ui, || {
        let b = built_with_renderer();
        let first = PlatformImage::Pixels(solid(8, 1));
        b.hub.raise(Some(&first), true);
        b.window.apply_changes(&ChangeSet::default());
        b.hub.raise(None, true);
        b.window.apply_changes(&ChangeSet::default());
        b.window.composer().unwrap().current().map(|c| c.id())
    });
    assert_eq!(current, None);
    finish(ui);
}

// C#: ShellViewingTests.ImageChanging_ThatIsNotAFileChange_NeverStartsACrossfade
#[test]
#[ignore = "native: needs desktop"]
fn image_changing_that_is_not_a_file_change_never_starts_a_crossfade() {
    let ui = start_ui();
    let starts = run(&ui, || {
        let b = built_with_renderer();
        let settings = AppSettings {
            image_transition: ImageTransition::Fade,
            image_transition_ms: 200,
            ..AppSettings::default()
        };
        b.window.view_model().borrow_mut().set_settings(Arc::new(settings));
        let composer = b.window.composer().unwrap();
        composer.set(Some(solid(8, 1)));
        draw_one_frame(&b, &composer);
        b.window.apply_changes(&ChangeSet {
            image_changing: Some(ImageChangingEventArgs { is_file_change: false }),
            ..ChangeSet::default()
        });
        composer.crossfade().start_count()
    });
    assert_eq!(starts, 0);
    finish(ui);
}

// C#: ShellViewingTests.Crossfade_Enabled_StartsOnEveryFileChange_AndFinishesByItself (the start half; the fade itself is 16c/16d)
#[test]
#[ignore = "native: needs desktop"]
fn image_changing_for_a_file_change_starts_a_crossfade_only_when_the_transition_is_on() {
    let ui = start_ui();
    let (off, on_active, on_starts) = run(&ui, || {
        let b = built_with_renderer();
        let composer = b.window.composer().unwrap();
        composer.set(Some(solid(8, 1)));
        draw_one_frame(&b, &composer);
        let changing = ChangeSet {
            image_changing: Some(ImageChangingEventArgs { is_file_change: true }),
            ..ChangeSet::default()
        };
        // Default settings: `ImageTransition::None`.
        b.window.apply_changes(&changing);
        let off = composer.crossfade().start_count();
        let settings = AppSettings {
            image_transition: ImageTransition::Fade,
            image_transition_ms: 200,
            ..AppSettings::default()
        };
        b.window.view_model().borrow_mut().set_settings(Arc::new(settings));
        b.window.apply_changes(&changing);
        (
            off,
            composer.crossfade().is_active(),
            composer.crossfade().start_count(),
        )
    });
    assert_eq!(off, 0, "the transition is off by default");
    assert!(on_active);
    assert_eq!(on_starts, 1);
    finish(ui);
}

// C#: ShellViewingTests.ClearCache_DropsEveryUnretainedGpuBitmap_NotOnlyTheCounter / ClearCache_AlsoClearsTheGpuCache_AndTheImageIsRedrawn (INV-2)
#[test]
#[ignore = "native: needs desktop"]
fn clearing_the_preview_cache_counts_clears_the_gpu_cache_and_cancels_the_fade() {
    let ui = start_ui();
    run(&ui, || {
        let b = built_with_renderer();
        let composer = b.window.composer().unwrap();
        composer.set(Some(solid(8, 1)));
        draw_one_frame(&b, &composer);
        let settings = AppSettings {
            image_transition: ImageTransition::Fade,
            image_transition_ms: 400,
            ..AppSettings::default()
        };
        b.window.view_model().borrow_mut().set_settings(Arc::new(settings));
        b.window.apply_changes(&ChangeSet {
            image_changing: Some(ImageChangingEventArgs { is_file_change: true }),
            ..ChangeSet::default()
        });
        assert!(composer.crossfade().is_active());
        // The listener may run on any thread: here a worker thread calls `clear_cache` like the memory-pressure path does.
        let service = Arc::clone(b.window.view_model().borrow().preview_service());
        std::thread::spawn(move || service.clear_cache()).join().unwrap();
    });
    wait_until(&ui, "the cache-cleared job ran on the UI thread", || {
        built().window.gpu_cache_clear_count() == 1
    });
    run(&ui, || {
        let b = built();
        assert!(
            !b.window.composer().unwrap().crossfade().is_active(),
            "the old fade layer stopped"
        );
        assert_eq!(b.window.gpu_cache_clear_count(), 1);
    });
    finish(ui);
}

// Trap T4: after dispose the posted clear finds no window.
#[test]
#[ignore = "native: needs desktop"]
fn clear_gpu_cache_after_dispose_is_a_no_op() {
    let ui = start_ui();
    let flag = Arc::new(AtomicBool::new(false));
    let sentinel = Arc::clone(&flag);
    run(&ui, move || {
        let b = built_with_renderer();
        let service = Arc::clone(b.window.view_model().borrow().preview_service());
        b.window.dispose();
        service.clear_cache();
        // A Background job runs after the Normal one that `clear_cache` posted (the listener was dropped by `dispose`, so none was).
        b.window.dispatcher().post_with_priority(
            Box::new(move || sentinel.store(true, Ordering::SeqCst)),
            UiPriority::Background,
        );
    });
    let probe = Arc::clone(&flag);
    wait_until(&ui, "the sentinel job", move || probe.load(Ordering::SeqCst));
    run(&ui, || assert_eq!(built().window.gpu_cache_clear_count(), 0));
    finish(ui);
}

// C#: ShellViewingTests.Compare_TogglesShowsBothImages_AndClicksSelectTheCellUnderThePoint (the click half; client-origin points at 25 % / 75 % / (2, 2))
#[test]
#[ignore = "native: needs desktop"]
fn try_select_compare_at_maps_left_right_and_margin() {
    let ui = start_ui();
    run(&ui, || {
        let b = built_with_renderer();
        {
            let mut view_model = b.window.view_model().borrow_mut();
            let compare = view_model.compare_mut_for_tests();
            compare.set_left_path(Some("left.jpg".to_owned()));
            compare.set_right_path(Some("right.jpg".to_owned()));
        }
        let client = pr_platform_win::host::contracts::ShellWindowApi::client_size_dip(&*b.shell);
        let left = PointD {
            x: client.width * 0.25,
            y: client.height * 0.5,
        };
        let right = PointD {
            x: client.width * 0.75,
            y: client.height * 0.5,
        };
        let margin = PointD { x: 2.0, y: 2.0 };
        // Compare hidden: every point is refused and nothing is selected.
        assert!(!b.window.try_select_compare_at(left));
        assert_eq!(b.window.view_model().borrow().compare().selected_path(), None);

        b.window
            .view_model()
            .borrow_mut()
            .compare_mut_for_tests()
            .set_visible(true);
        assert!(
            layout::hit_test(client, left).is_some(),
            "the 25 % point lies on the left cell"
        );
        assert!(b.window.try_select_compare_at(left));
        assert_eq!(
            b.window.view_model().borrow().compare().selected_path(),
            Some("left.jpg")
        );
        assert!(b.window.try_select_compare_at(right));
        assert_eq!(
            b.window.view_model().borrow().compare().selected_path(),
            Some("right.jpg")
        );
        assert!(
            !b.window.try_select_compare_at(margin),
            "the margin is outside the panel"
        );
        assert_eq!(
            b.window.view_model().borrow().compare().selected_path(),
            Some("right.jpg"),
            "a miss keeps the selection"
        );
        b.window.dispose();
        assert!(!b.window.try_select_compare_at(left), "disposed");
    });
    finish(ui);
}

// C#: Stats / Pacer properties
#[test]
#[ignore = "native: needs desktop"]
fn stats_and_pacer_start_empty() {
    let ui = start_ui();
    run(&ui, || {
        let b = built_with_renderer();
        assert!(!b.window.with_pacer(|pacer| pacer.is_degraded()));
        assert_eq!(b.window.with_pacer(|pacer| pacer.deferred_total()), 0);
        assert_eq!(b.window.gpu_cache_clear_count(), 0);
        assert_eq!(b.window.with_stats(|stats| stats.count()), 0);
    });
    finish(ui);
}
