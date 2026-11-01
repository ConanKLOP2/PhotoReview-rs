//! INV-1 ("an OLD preview never overwrites the NEW one") at the sink level, second half of the port of
//! `PhotoReview.App.Tests/Coordinators/StalePreviewOverwriteTests.cs` (baseline 7eaee2a2; lane cx-testgaps-stale2), next to `g19_stale_preview.rs`:
//! ClearCache during decodes, a superseded running decode that is cached, a late decode failure, the zoom original finishing late / of the current image /
//! released at navigation start, and the compare cases (partner of a superseded pair, partner failing late, pair A overwritten by pair B).
//!
//! The order of completion is the order in which the test opens the gates and pumps; "the worker has finished" is a probe in the decoder, not a sleep.
//! Fakes only: the folder is a temp root with tiny files; nothing touches the Recycle Bin.

mod support {
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use pr_app::coordinators::zoom_detail::same_platform_image;
use pr_core::model::LoadingMode;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use support::present_helpers::{images_shown, presented_paths, start, statuses};
use support::presenter_fakes::PresenterFixture;
use support::preview_fakes::{Gate, SizedDecoder, lock, sized_image};

const THREE: [&str; 3] = ["g19b-a.jpg", "g19b-b.jpg", "g19b-c.jpg"];
const PAIRS: [&str; 5] = ["a.jpg", "a (1).jpg", "b.jpg", "b (1).jpg", "c.jpg"];

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Every decode of a file blocks on that file's gate (opened by the test); a file can be made to fail; "started" and "finished" are observable.
#[derive(Debug, Default)]
struct NameGatedDecoder {
    gates: Mutex<HashMap<String, Arc<Gate>>>,
    fail: Mutex<HashSet<String>>,
    started: Mutex<Vec<String>>,
    started_changed: Condvar,
    finished: Mutex<Vec<String>>,
    timed_out: Mutex<Vec<String>>,
}

impl NameGatedDecoder {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn gate(&self, name: &str) -> Arc<Gate> {
        Arc::clone(lock(&self.gates).entry(name.to_lowercase()).or_default())
    }

    fn open(&self, name: &str) {
        self.gate(name).open();
    }

    fn fail(&self, name: &str) {
        lock(&self.fail).insert(name.to_lowercase());
    }

    fn has_started(&self, name: &str) -> bool {
        lock(&self.started).contains(&name.to_lowercase())
    }

    fn finished_count(&self, name: &str) -> usize {
        let key = name.to_lowercase();
        lock(&self.finished).iter().filter(|done| **done == key).count()
    }

    fn started_count(&self, name: &str) -> usize {
        let key = name.to_lowercase();
        lock(&self.started).iter().filter(|done| **done == key).count()
    }

    fn timed_out(&self) -> Vec<String> {
        lock(&self.timed_out).clone()
    }
}

impl ImageDecoder for NameGatedDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let name = name_of(&request.path);
        lock(&self.started).push(name.clone());
        self.started_changed.notify_all();
        let opened = self.gate(&name).wait();
        let result = if !opened {
            lock(&self.timed_out).push(name.clone());
            Err(DecodeError::InvalidData {
                message: format!("gate of {name} was never opened"),
            })
        } else if lock(&self.fail).contains(&name) {
            Err(DecodeError::InvalidData {
                message: "late decode failure".to_owned(),
            })
        } else {
            Ok(sized_image(1, 1, 1, 1, false))
        };
        lock(&self.finished).push(name);
        result
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(1, 1))
    }
}

/// `SizedDecoder` (only the full-resolution decode is gated) plus a probe: how many full-resolution decodes have returned.
#[derive(Debug)]
struct OriginalProbe {
    inner: Arc<SizedDecoder>,
    finished: AtomicUsize,
}

impl OriginalProbe {
    fn new() -> (Arc<Self>, Arc<SizedDecoder>) {
        let inner = SizedDecoder::new();
        (
            Arc::new(Self {
                inner: Arc::clone(&inner),
                finished: AtomicUsize::new(0),
            }),
            inner,
        )
    }
}

impl ImageDecoder for OriginalProbe {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let result = self.inner.decode(request);
        if request.decode_box().is_unbounded() {
            self.finished.fetch_add(1, Ordering::SeqCst);
        }
        result
    }

    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        self.inner.read_info(path)
    }
}

/// Starts navigation `index` and pumps until its decode really entered the decoder (the stat came back and the decode was requested).
fn navigate_until_decoding(f: &mut PresenterFixture, decoder: &NameGatedDecoder, index: i32, name: &str) {
    start(f, index);
    f.pump_until(|_| decoder.has_started(name));
}

fn fast_fixture(decoder: &Arc<NameGatedDecoder>, names: &[&str]) -> PresenterFixture {
    let mut f = PresenterFixture::new(Arc::clone(decoder) as _, names);
    f.settings.loading_mode = Some(LoadingMode::Fast); // no thumbnail event: only the PREVIEW events meet the superseded tokens
    f
}

// C#: StalePreviewOverwriteTests.ClearCacheDuringDecodes_CurrentNavigationStillPresents_AndNothingStaleIsCached
#[test]
fn clear_cache_during_decodes_current_navigation_still_presents_and_nothing_stale_is_cached() {
    let decoder = NameGatedDecoder::new();
    let mut f = fast_fixture(&decoder, &THREE);
    let b = f.path(THREE[1]);
    navigate_until_decoding(&mut f, &decoder, 0, THREE[0]);
    navigate_until_decoding(&mut f, &decoder, 1, THREE[1]);

    f.service.clear_cache(); // the epoch moves on while both decodes are running
    decoder.open(THREE[0]);
    decoder.open(THREE[1]);
    f.pump_until(|_| decoder.finished_count(THREE[0]) == 1 && decoder.finished_count(THREE[1]) == 1);
    f.pump_until_idle();

    assert!(decoder.timed_out().is_empty(), "{:?}", decoder.timed_out());
    assert_eq!(presented_paths(&f), vec![b], "the current navigation still presents");
    assert_eq!(images_shown(&f), 1);
    assert_eq!(f.catalog.current_index(), 1);
    assert_eq!(
        f.service.cache_count(),
        0,
        "both finished after the epoch moved on: neither is published"
    );
}

// C#: StalePreviewOverwriteTests.SupersededRunningDecode_FinishingLate_IsCachedForALaterVisitAndNeverShown
#[test]
fn superseded_running_decode_finishing_late_is_cached_for_a_later_visit_and_never_shown() {
    let decoder = NameGatedDecoder::new();
    let mut f = fast_fixture(&decoder, &THREE);
    let (a, b) = (f.path(THREE[0]), f.path(THREE[1]));
    navigate_until_decoding(&mut f, &decoder, 0, THREE[0]);
    navigate_until_decoding(&mut f, &decoder, 1, THREE[1]);
    decoder.open(THREE[1]);
    f.pump_until(|f| presented_paths(f).contains(&b));
    let shown = images_shown(&f);

    decoder.open(THREE[0]); // the superseded decode finishes after the new image is on screen
    f.pump_until(|_| decoder.finished_count(THREE[0]) == 1);
    f.pump_until_idle();

    assert_eq!(images_shown(&f), shown, "nothing of a reached the sink");
    assert_eq!(presented_paths(&f), vec![b.clone()]);
    let cached_a = f
        .service
        .try_get_cached_preview_path(&a)
        .expect("a's decode is in the RAM cache under a's key");
    let cached_b = f
        .service
        .try_get_cached_preview_path(&b)
        .expect("b's decode is in the RAM cache under b's key");
    assert!(!same_platform_image(
        cached_a.platform_image(),
        cached_b.platform_image()
    ));

    // Returning to a shows exactly that bitmap, from RAM (no second decode).
    start(&mut f, 0);
    f.pump_until_idle();
    assert_eq!(decoder.started_count(THREE[0]), 1, "no second decode of a");
    let on_screen = f.presenter.current_image().expect("a is shown");
    assert!(same_platform_image(on_screen, cached_a.platform_image()));
}

// C#: StalePreviewOverwriteTests.LateDecodeFailureOfAnOlderNavigation_NeitherShowsAnErrorNorTouchesTheCatalog
#[test]
fn late_decode_failure_of_an_older_navigation_neither_shows_an_error_nor_touches_the_catalog() {
    let decoder = NameGatedDecoder::new();
    let mut f = fast_fixture(&decoder, &THREE);
    let b = f.path(THREE[1]);
    decoder.fail(THREE[0]);
    navigate_until_decoding(&mut f, &decoder, 0, THREE[0]);
    navigate_until_decoding(&mut f, &decoder, 1, THREE[1]);
    decoder.open(THREE[1]);
    f.pump_until(|f| presented_paths(f).contains(&b));
    f.pump_until_idle();
    let (statuses_before, shown_before) = (statuses(&f), images_shown(&f));

    decoder.open(THREE[0]); // a's decode now fails, after the user left
    f.pump_until(|_| decoder.finished_count(THREE[0]) == 1);
    f.pump_until_idle();

    assert_eq!(statuses(&f), statuses_before, "no error status of the older navigation");
    assert_eq!(images_shown(&f), shown_before);
    assert_eq!(presented_paths(&f), vec![b]);
    assert_eq!(f.catalog.count(), 3, "nothing removed from the catalog");
    assert_eq!(f.catalog.current_index(), 1);
}

/// A fixture whose previews are downscaled (so a zoom arms a full-resolution decode) and whose full-resolution decodes are gated per file.
fn zoom_fixture() -> (PresenterFixture, Arc<OriginalProbe>, Arc<SizedDecoder>) {
    let (probe, sized) = OriginalProbe::new();
    let f = PresenterFixture::new(Arc::clone(&probe) as _, &THREE);
    (f, probe, sized)
}

/// Presents file 0 (its preview is not gated), zooms in so its original decode is requested, and waits until that decode entered the decoder.
fn present_zoomed_first(f: &mut PresenterFixture, sized: &SizedDecoder) {
    start(f, 0);
    f.pump_until(|f| presented_paths(f).len() == 1);
    f.pump_until_idle();
    f.presenter.set_viewer_zoom(Some(2.0)); // arms the original decode of file 0
    assert!(
        f.presenter.zoom_detail().pending_load(),
        "the original is really in flight"
    );
    assert!(
        sized.wait_started(THREE[0]),
        "the original decode of file 0 never started"
    );
}

// C#: StalePreviewOverwriteTests.LateZoomOriginal_AfterNavigatingAway_ChangesNothingOnScreen
#[test]
fn late_zoom_original_after_navigating_away_changes_nothing_on_screen() {
    let (mut f, probe, sized) = zoom_fixture();
    let gate = sized.gate(THREE[0]);
    let b = f.path(THREE[1]);
    present_zoomed_first(&mut f, &sized);
    start(&mut f, 1);
    f.pump_until(|f| presented_paths(f).contains(&b));

    gate.open(); // the original of file 0 finishes late (file 1's own original is not gated: two originals in all)
    f.pump_until(|_| probe.finished.load(Ordering::SeqCst) == 2);
    f.pump_until_idle();

    let calls = lock(&f.sink_log).images.clone();
    let first_b = calls
        .iter()
        .position(|call| {
            call.presented_path
                .as_deref()
                .is_some_and(|p| p.eq_ignore_ascii_case(&b))
        })
        .expect("b was shown");
    assert!(
        calls[first_b..].iter().all(|call| call
            .presented_path
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case(&b))),
        "an image of a reached the sink after b was shown"
    );
    assert_eq!(presented_paths(&f).last(), Some(&b));
    assert_eq!(f.catalog.current_index(), 1);
    if let (Some(held), Some(on_screen)) = (f.presenter.zoom_detail().held_original(), f.presenter.current_image()) {
        assert!(
            same_platform_image(on_screen, held.platform_image()),
            "the held original is the one on screen"
        );
    }
}

// C#: StalePreviewOverwriteTests.ZoomOriginal_OfTheCurrentImage_ReplacesThePreview_ThenNavigationReleasesIt
#[test]
fn zoom_original_of_the_current_image_replaces_the_preview_then_navigation_releases_it() {
    let (mut f, _probe, sized) = zoom_fixture();
    let (a, b) = (f.path(THREE[0]), f.path(THREE[1]));
    present_zoomed_first(&mut f, &sized); // the gate of file 0 is not closed: the original completes by itself
    f.pump_until(|f| f.presenter.zoom_detail().held_original().is_some());
    f.pump_until_idle();

    assert!(f.presenter.zoom_detail().is_showing_original());
    let last = lock(&f.sink_log).images.last().cloned().expect("an image call");
    assert!(last.is_same_source_swap, "same file, original stage");
    assert_eq!(
        last.presented_path.as_deref().map(str::to_lowercase),
        Some(a.to_lowercase())
    );
    let held = f.presenter.zoom_detail().held_original().cloned().expect("held");
    assert!(same_platform_image(
        f.presenter.current_image().expect("shown"),
        held.platform_image()
    ));

    start(&mut f, 1);
    assert!(
        f.presenter.zoom_detail().held_original().is_none(),
        "released at navigation start"
    );
    f.pump_until(|f| presented_paths(f).contains(&b));
    f.pump_until_idle();
    // The zoom is still 2.0, so b arms (and finishes) an original of its own: it must not be a's.
    if let Some(now) = f.presenter.zoom_detail().held_original() {
        assert!(!same_platform_image(now.platform_image(), held.platform_image()));
    }
}

// C#: StalePreviewOverwriteTests.NavigationStart_ReleasesThePreviousZoomOriginalImmediately
#[test]
fn navigation_start_releases_the_previous_zoom_original_at_once() {
    let (mut f, _probe, sized) = zoom_fixture();
    present_zoomed_first(&mut f, &sized);
    f.pump_until(|f| f.presenter.zoom_detail().held_original().is_some());
    assert!(f.presenter.zoom_detail().is_showing_original());

    start(&mut f, 1); // nothing pumped: the next image has not even been decoded

    assert!(f.presenter.zoom_detail().held_original().is_none());
    assert!(!f.presenter.zoom_detail().is_showing_original());
    assert!(!f.presenter.zoom_detail().pending_load());
    f.pump_until_idle();
}

/// Presents the pair starting at `first` far enough that the partner's decode is in the decoder: the main image's gate is opened, the partner's is not.
fn open_pair_until_partner_decoding(
    f: &mut PresenterFixture,
    decoder: &NameGatedDecoder,
    first: i32,
    main: &str,
    partner: &str,
) {
    navigate_until_decoding(f, decoder, first, main);
    decoder.open(main);
    f.pump_until(|_| decoder.has_started(partner));
}

/// What a stale compare partner must leave untouched once the user is on `c.jpg` (the status text and the sink name `c`, nothing of the pair).
fn assert_settled_on_c_without_compare(f: &PresenterFixture, c: &str) {
    assert!(!f.presenter.compare().is_visible());
    assert!(f.presenter.compare().left_image().is_none());
    assert!(f.presenter.compare().right_image().is_none());
    assert!(f.presenter.compare().left_path().is_none());
    assert!(f.presenter.compare().right_path().is_none());
    assert_eq!(f.catalog.current_index(), 4);
    assert_eq!(presented_paths(f).last().map(String::as_str), Some(c));
    assert!(
        presented_paths(f).iter().all(|p| !p.to_lowercase().contains("a (1)")),
        "{:?}",
        presented_paths(f)
    );
    let all = statuses(f);
    assert!(all.last().is_some_and(|last| last.contains("c.jpg")), "{all:?}");
    assert!(all.iter().all(|s| !s.contains("a (1)")), "{all:?}");
    assert!(
        f.presenter.status_text().contains("c.jpg"),
        "{}",
        f.presenter.status_text()
    );
}

// C#: StalePreviewOverwriteTests.ComparePartnerOfASupersededPair_FinishingLate_ChangesNothing
#[test]
fn compare_partner_of_a_superseded_pair_finishing_late_changes_nothing() {
    let decoder = NameGatedDecoder::new();
    let mut f = fast_fixture(&decoder, &PAIRS);
    let c = f.path(PAIRS[4]);
    open_pair_until_partner_decoding(&mut f, &decoder, 0, PAIRS[0], PAIRS[1]);
    navigate_until_decoding(&mut f, &decoder, 4, PAIRS[4]); // c.jpg is in no pair
    decoder.open(PAIRS[4]);
    f.pump_until(|f| presented_paths(f).contains(&c));

    decoder.open(PAIRS[1]); // the partner arrives after the user left
    f.pump_until(|_| decoder.finished_count(PAIRS[1]) == 1);
    f.pump_until_idle();

    assert!(decoder.timed_out().is_empty(), "{:?}", decoder.timed_out());
    assert_settled_on_c_without_compare(&f, &c);
}

// C#: StalePreviewOverwriteTests.ComparePartnerFailingLate_OfASupersededNavigation_ChangesNothing
#[test]
fn compare_partner_failing_late_of_a_superseded_navigation_changes_nothing() {
    let decoder = NameGatedDecoder::new();
    let mut f = fast_fixture(&decoder, &PAIRS);
    let c = f.path(PAIRS[4]);
    decoder.fail(PAIRS[1]);
    open_pair_until_partner_decoding(&mut f, &decoder, 0, PAIRS[0], PAIRS[1]);
    navigate_until_decoding(&mut f, &decoder, 4, PAIRS[4]);
    decoder.open(PAIRS[4]);
    f.pump_until(|f| presented_paths(f).contains(&c));

    decoder.open(PAIRS[1]); // the partner decode now fails, after the user left
    f.pump_until(|_| decoder.finished_count(PAIRS[1]) == 1);
    f.pump_until_idle();

    assert_settled_on_c_without_compare(&f, &c);
    assert!(statuses(&f).iter().all(|s| !s.contains("late decode failure")));
    assert_eq!(f.catalog.count(), 5);
}

// C#: StalePreviewOverwriteTests.ComparePairOverwrittenByANewerPair_LateCellOfTheOldPair_IsDropped
#[test]
fn compare_pair_overwritten_by_a_newer_pair_late_cell_of_the_old_pair_is_dropped() {
    let decoder = NameGatedDecoder::new();
    let mut f = fast_fixture(&decoder, &PAIRS);
    let (b, b1) = (f.path(PAIRS[2]), f.path(PAIRS[3]));
    open_pair_until_partner_decoding(&mut f, &decoder, 0, PAIRS[0], PAIRS[1]); // pair A, its partner pending
    open_pair_until_partner_decoding(&mut f, &decoder, 2, PAIRS[2], PAIRS[3]); // pair B supersedes it

    decoder.open(PAIRS[3]); // B completes first
    f.pump_until(|f| f.presenter.compare().is_visible());
    assert_eq!(f.presenter.compare().left_path(), Some(b.as_str()));
    assert_eq!(f.presenter.compare().right_path(), Some(b1.as_str()));

    decoder.open(PAIRS[1]); // A's cell arrives afterwards
    f.pump_until(|_| decoder.finished_count(PAIRS[1]) == 1);
    f.pump_until_idle();

    assert!(decoder.timed_out().is_empty(), "{:?}", decoder.timed_out());
    assert!(f.presenter.compare().is_visible());
    assert_eq!(f.presenter.compare().left_path(), Some(b.as_str()));
    assert_eq!(f.presenter.compare().right_path(), Some(b1.as_str()));
    assert!(f.presenter.compare().left_image().is_some());
    assert!(f.presenter.compare().right_image().is_some());
    assert_eq!(f.catalog.current_index(), 2);
    assert!(
        f.presenter.status_text().contains("b (1).jpg"),
        "{}",
        f.presenter.status_text()
    );
    assert!(statuses(&f).iter().all(|s| !s.contains("a (1)")), "{:?}", statuses(&f));
}
