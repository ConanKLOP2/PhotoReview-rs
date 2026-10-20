//! Tests of the `ImagePresenter` state slice (W8-07h): constructor, properties, `ClearPresentation`, `EvictCachedPath`, the zoom-detail glue
//! (C# `ImagePresenter.cs` lines 1 to 232, baseline d92d6aae). Contract: `contracts/W8-07h.md`, kind F (fakes, no oracle, no golden).
//!
//! The C# tests that need a successful `PresentAsync` are only complete after W8-07j; they are written here with `#[ignore = "needs W8-07j"]`
//! (the 07j porter removes the ignores). The zoom swap itself (`show_zoom_detail_image`) is exercised by in-crate tests in `presenter/zoom.rs`.

mod support {
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::sync::Arc;

use pr_app::coordinators::presenter::state::{PresentRequest, PresenterChanges};
use pr_app::coordinators::zoom_detail::{IndicatorDelay, ZoomDetailRefreshResult};
use pr_core::abstractions::UiScheduler;
use pr_imaging::decoder_traits::ImageDecoder;
use pr_imaging::service::PreviewError;
use support::presenter_fakes::{
    CountingRunner, EmbeddedInfoRawDecoder, FixtureOptions, FullForNamesDecoder, GatedScheduler, PresenterFixture,
};
use support::preview_fakes::{ServiceOptions, SizedDecoder, lock};

fn fixture(names: &[&str]) -> PresenterFixture {
    PresenterFixture::new(SizedDecoder::new(), names)
}

// ---- [self] ----------------------------------------------------------------------------------------------------------

// New test: no C# twin (the C# constructor is covered by every test that builds a presenter).
#[test]
fn new_starts_no_work_and_reads_no_file() {
    let runner = CountingRunner::new();
    let scheduler = GatedScheduler::new();
    let f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["a.jpg"],
        FixtureOptions {
            runner: Some(runner.clone()),
            scheduler: Some(scheduler.clone() as Arc<dyn UiScheduler>),
            ..FixtureOptions::default()
        },
    );

    assert_eq!(lock(&f.sink_log).call_count(), 0);
    assert_eq!(f.preload.total_calls(), 0);
    assert_eq!(runner.jobs(), 0);
    assert_eq!(scheduler.posted_count(), 0);
    assert_eq!(f.wakes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(f.presenter.is_idle());
    assert!(f.presenter.current_image().is_none());
    assert_eq!(f.presenter.status_text(), "");
    assert!(!f.presenter.status_needs_attention());
    assert!(f.presenter.current_presented_path().is_none());
    assert!(f.presenter.current_photo_info().is_none());
    assert_eq!(f.presenter.current_original_width(), 0);
    assert_eq!(f.presenter.current_original_height(), 0);
    assert!(!f.presenter.is_same_source_swap());
    assert!(!f.presenter.last_present_started_from_ram());
    assert!(!f.presenter.is_compare_visible());
    assert!(!f.presenter.zoom_detail().is_showing_original());
    assert!(!f.presenter.zoom_detail().pending_load());
}

// New test: no C# twin.
#[test]
fn defer_next_preload_kick_flag_roundtrips() {
    let mut f = fixture(&[]);
    assert!(!f.presenter.defer_next_preload_kick());
    f.presenter.set_defer_next_preload_kick(true);
    assert!(f.presenter.defer_next_preload_kick());
    f.presenter.set_defer_next_preload_kick(false);
    assert!(!f.presenter.defer_next_preload_kick());
}

// New test: no C# twin.
#[test]
fn evict_cached_path_forwards_the_normalized_path_to_the_preload_controller() {
    let f = fixture(&["a.jpg"]);
    let path = f.path("a.jpg");

    f.presenter.evict_cached_path(&path).expect("a valid path evicts");

    assert_eq!(f.preload.removed_for_path(), vec![path.to_uppercase()]);
}

// New test: no C# twin.
#[test]
fn evict_cached_path_with_an_invalid_path_returns_the_error_and_removes_nothing() {
    let f = fixture(&["a.jpg"]);

    let result = f.presenter.evict_cached_path("");

    assert!(matches!(result, Err(PreviewError::InvalidPath(_))), "got {result:?}");
    assert!(f.preload.removed_for_path().is_empty());
}

// C#: ZoomDetailLoaderGapTests.Refresh_WithoutATarget_ReportsFullResolution (through the presenter)
#[test]
fn refresh_zoom_detail_without_a_target_reports_full_resolution() {
    let mut f = fixture(&[]);

    let (result, changes) = f.presenter.refresh_zoom_detail(1.0);

    assert_eq!(result, ZoomDetailRefreshResult::AlreadyFullResolution);
    assert!(changes.is_empty());
    assert_eq!(lock(&f.sink_log).call_count(), 0);
}

// New test: no C# twin.
#[test]
fn set_viewer_zoom_without_a_target_returns_no_changes() {
    let mut f = fixture(&[]);

    assert!(f.presenter.set_viewer_zoom(Some(2.0)).is_empty());
    assert!(f.presenter.set_viewer_zoom(None).is_empty());
    assert!(f.presenter.reset_zoom_detail().is_empty());
    assert!(f.presenter.is_idle());
}

// New test: no C# twin.
#[test]
fn clear_presentation_on_a_fresh_presenter_clears_compare_and_reports_changes() {
    let mut f = fixture(&[]);

    let mut env = support::presenter_fakes::env(&mut f.catalog, &f.settings, &f.format);
    let changes = f.presenter.clear_presentation(&mut env);

    assert!(changes.contains(PresenterChanges::CURRENT_IMAGE));
    assert!(f.presenter.current_image().is_none());
    assert!(!f.presenter.is_compare_visible());
    assert!(f.presenter.compare().left_image().is_none());
    assert!(f.presenter.compare().right_image().is_none());
    let log = lock(&f.sink_log);
    assert_eq!(log.images.len(), 1, "exactly one set_current_image");
    let call = &log.images[0];
    assert!(call.image.is_none());
    assert!(!call.is_file_change);
    assert_eq!((call.original_width, call.original_height), (0, 0));
    assert!(!call.is_same_source_swap);
    assert!(call.presented_path.is_none());
}

// New test: no C# twin.
#[test]
fn presenter_changes_bits_combine_and_test() {
    let both = PresenterChanges::CURRENT_IMAGE | PresenterChanges::STATUS;
    assert!(both.contains(PresenterChanges::CURRENT_IMAGE));
    assert!(both.contains(PresenterChanges::STATUS));
    assert!(!both.contains(PresenterChanges::COMPARE));
    assert!(!both.contains(PresenterChanges::CURRENT_IMAGE | PresenterChanges::COMPARE));
    assert!(PresenterChanges::NONE.is_empty());
    assert!(PresenterChanges::default().is_empty());
    assert!(!both.is_empty());

    let mut all = PresenterChanges::NONE;
    for bit in [
        PresenterChanges::CURRENT_IMAGE,
        PresenterChanges::STATUS,
        PresenterChanges::PHOTO_INFO,
        PresenterChanges::COMPARE,
        PresenterChanges::RAW_DECODE_INDICATOR,
        PresenterChanges::CURRENT_PATH,
    ] {
        assert!(!all.contains(bit), "the bits are distinct");
        all |= bit;
    }
    assert_eq!(all.0, 1 | 2 | 4 | 8 | 16 | 32);
}

// ---- [needs 07j] -----------------------------------------------------------------------------------------------------

fn raw_service_options(full_decoder: Arc<dyn ImageDecoder>, original_mode: bool) -> ServiceOptions {
    ServiceOptions {
        original_loading_mode: original_mode,
        raw_full_decoder: Some(full_decoder),
        raw_full_decode_enabled: true,
    }
}

fn instant_indicator() -> IndicatorDelay {
    // Never reached by these tests (they do not wait for the RAW indicator): an immediate delay is the safe stand-in.
    Arc::new(|_delay, _cancel| true)
}

fn present_index(f: &mut PresenterFixture, index: i32) {
    let mut env = support::presenter_fakes::env(&mut f.catalog, &f.settings, &f.format);
    f.presenter
        .present(&mut env, PresentRequest::new(index))
        .unwrap_or_else(|error| panic!("present {index}: {error}"));
    f.pump_until_idle();
}

fn clear(f: &mut PresenterFixture) {
    let mut env = support::presenter_fakes::env(&mut f.catalog, &f.settings, &f.format);
    f.presenter.clear_presentation(&mut env);
}

// C#: ImagePresenterTests.ClearPresentation_ClearsCurrentImageAndCompareState
#[test]
fn clear_presentation_clears_current_image_and_compare_state() {
    let mut f = fixture(&["photo.jpg", "photo (1).jpg"]);
    present_index(&mut f, 0);

    clear(&mut f);

    assert!(f.presenter.current_image().is_none());
    assert!(!f.presenter.is_compare_visible());
    assert!(f.presenter.compare().left_image().is_none());
    assert!(f.presenter.compare().right_image().is_none());
    assert!(lock(&f.sink_log).images.last().is_some_and(|call| call.image.is_none()));
}

// C#: ZoomDetailTests.NavigatingAfterTheOriginalSwap_StillRequestsTheFileChangeTransition
#[test]
fn navigating_after_the_original_swap_still_requests_the_file_change_transition() {
    let mut f = fixture(&["a.jpg", "b.jpg"]);
    present_index(&mut f, 0);
    f.presenter.set_viewer_zoom(Some(2.0));
    f.pump_until(|f| f.presenter.zoom_detail().is_showing_original());
    let shown_before = lock(&f.sink_log).images.len();

    present_index(&mut f, 1);

    let flags = lock(&f.sink_log).file_change_flags();
    assert!(flags.len() > shown_before);
    assert!(flags[shown_before], "the first bitmap of the different file fades");
}

// C#: ZoomDetailTests.RawPreviewSize_FollowsWhichImageIsDisplayed
#[test]
fn raw_preview_size_follows_which_image_is_displayed() {
    let mut f = PresenterFixture::with_options(
        Arc::new(EmbeddedInfoRawDecoder),
        &["info.cr2"],
        FixtureOptions {
            service: raw_service_options(SizedDecoder::new(), true),
            indicator_delay: Some(instant_indicator()),
            ..FixtureOptions::default()
        },
    );
    present_index(&mut f, 0);
    let info = f
        .presenter
        .current_photo_info()
        .cloned()
        .expect("photo info after the present");
    assert_eq!((info.raw_preview_width, info.raw_preview_height), (2000, 1333));
    assert_eq!((info.width, info.height), (6000, 4000), "sensor size stays");

    f.presenter.set_viewer_zoom(Some(1.0));
    f.pump_until(|f| f.presenter.zoom_detail().is_showing_original());
    let info = f
        .presenter
        .current_photo_info()
        .cloned()
        .expect("photo info while the original shows");
    assert_eq!((info.raw_preview_width, info.raw_preview_height), (0, 0));
    assert_eq!((info.width, info.height), (6000, 4000));

    f.presenter.set_viewer_zoom(None);
    assert!(!f.presenter.zoom_detail().is_showing_original());
    let info = f
        .presenter
        .current_photo_info()
        .cloned()
        .expect("photo info back on the preview");
    assert_eq!((info.raw_preview_width, info.raw_preview_height), (2000, 1333));
}

// C#: ZoomDetailTests.RawPreviewSize_WhenTheHeldOriginalIsStillTheEmbeddedJpeg_StaysInThePhotoInfo
#[test]
fn raw_preview_size_when_the_held_original_is_still_the_embedded_jpeg_stays_in_the_photo_info() {
    // The "full" decoder fell back to the embedded JPEG: the held original still is the RAW preview.
    let mut f = PresenterFixture::with_options(
        Arc::new(EmbeddedInfoRawDecoder),
        &["info-embedded.cr2"],
        FixtureOptions {
            service: raw_service_options(Arc::new(EmbeddedInfoRawDecoder), false),
            indicator_delay: Some(instant_indicator()),
            ..FixtureOptions::default()
        },
    );
    present_index(&mut f, 0);

    f.presenter.set_viewer_zoom(Some(1.0));
    f.pump_until(|f| f.presenter.zoom_detail().is_showing_original());

    let info = f.presenter.current_photo_info().cloned().expect("photo info");
    assert_eq!((info.raw_preview_width, info.raw_preview_height), (2000, 1333));
}

// C#: ZoomDetailTests.RawPreviewInfo_OfAnEarlierPhoto_IsNeverShownForTheNextPhotoWhoseFullDecodeFellBackToTheEmbeddedJpeg
#[test]
fn raw_preview_info_of_an_earlier_photo_is_never_shown_for_the_next_photo_whose_full_decode_fell_back_to_the_embedded_jpeg()
 {
    // A gets a true full decode; B's "full" decode falls back to its embedded JPEG (still a RAW preview).
    let mut f = PresenterFixture::with_options(
        Arc::new(EmbeddedInfoRawDecoder),
        &["a.cr2", "b.cr2"],
        FixtureOptions {
            service: raw_service_options(
                Arc::new(FullForNamesDecoder {
                    full_names: vec!["a.cr2".to_owned()],
                }),
                true,
            ),
            indicator_delay: Some(instant_indicator()),
            ..FixtureOptions::default()
        },
    );
    present_index(&mut f, 0);
    f.presenter.set_viewer_zoom(Some(1.0));
    f.pump_until(|f| f.presenter.zoom_detail().is_showing_original());
    let info = f.presenter.current_photo_info().cloned().expect("photo info of A");
    assert_eq!(info.file_name, "a.cr2");
    assert_eq!(info.raw_preview_width, 0, "the full decode of A is on screen");

    present_index(&mut f, 1);
    f.presenter.set_viewer_zoom(None);
    f.presenter.set_viewer_zoom(Some(1.0));
    f.pump_until(|f| f.presenter.zoom_detail().is_showing_original());

    let info = f.presenter.current_photo_info().cloned().expect("photo info of B");
    assert_eq!(info.file_name, "b.cr2");
    assert_eq!(info.raw_preview_width, 2000, "B's held original is the embedded JPEG");
}

// C#: ZoomDetailTests.NonRawPhoto_NeverHasARawPreviewSize
#[test]
fn non_raw_photo_never_has_a_raw_preview_size() {
    let mut f = fixture(&["a.jpg"]);
    present_index(&mut f, 0);
    assert_eq!(
        f.presenter.current_photo_info().map(|info| info.raw_preview_width),
        Some(0)
    );

    f.presenter.set_viewer_zoom(Some(2.0));
    f.pump_until(|f| f.presenter.zoom_detail().is_showing_original());
    assert_eq!(
        f.presenter.current_photo_info().map(|info| info.raw_preview_width),
        Some(0)
    );

    f.presenter.set_viewer_zoom(None);
    assert_eq!(
        f.presenter.current_photo_info().map(|info| info.raw_preview_width),
        Some(0)
    );
}

// The sink's zoom change is how the C# `OnApplyInitialViewMode` test hook reaches the loader (`take_viewer_zoom_change` after `apply_initial_view_mode`).
#[test]
fn the_zoom_of_apply_initial_view_mode_reaches_the_loader_before_the_target_is_armed() {
    let mut f = fixture(&["a.jpg"]);
    lock(&f.sink_log).zoom_on_apply_initial_view_mode = Some(Some(2.0));

    present_index(&mut f, 0);
    f.pump_until(|f| f.presenter.zoom_detail().is_showing_original());

    assert!(f.presenter.zoom_detail().is_showing_original());
}
