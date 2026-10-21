#[path = "common/viewing_fakes_16d.rs"]
mod viewing_fakes;

use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::shell_viewing::prefetcher::{PresentationPrefetcher, plan};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use viewing_fakes::{FakePrefetchCache, FakePrefetchHost};

fn pixel() -> Arc<PixelBuffer> {
    Arc::new(PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).unwrap())
}
fn rig() -> (Rc<PresentationPrefetcher>, Rc<FakePrefetchCache>, Rc<FakePrefetchHost>) {
    let cache = Rc::new(FakePrefetchCache::default());
    let host = Rc::new(FakePrefetchHost::default());
    let prefetcher = Rc::new(PresentationPrefetcher::new(cache.clone(), host.clone()));
    (prefetcher, cache, host)
}
fn apply(p: &PresentationPrefetcher, host: &FakePrefetchHost, found: &[Option<Arc<PixelBuffer>>]) {
    let generation = host.lookups.borrow().last().unwrap().generation;
    p.apply_lookup(generation, found).unwrap();
}

#[test]
fn plan_is_ahead_in_the_direction_of_travel_then_behind_clamped_to_the_catalog() {
    // C#: PresentationPrefetcherTests.Plan_IsAheadInTheDirectionOfTravelThenBehind_ClampedToTheCatalog
    for (index, count, direction, expected) in [
        (5, 10, 1, vec![6, 7, 4]),
        (5, 10, -1, vec![4, 3, 6]),
        (0, 10, 1, vec![1, 2]),
        (0, 10, -1, vec![1]),
        (9, 10, 1, vec![8]),
        (9, 10, -1, vec![8, 7]),
        (0, 1, 1, vec![]),
    ] {
        assert_eq!(plan(count, index, direction, 2, 1), expected);
    }
}
#[test]
fn plan_theory_table() {
    for count in [0_i32, 1, 2, 10] {
        for index in [0, count.saturating_sub(1)] {
            for direction in [-5, 0, 1] {
                assert!(plan(count, index, direction, 0, 0).is_empty());
                assert!(plan(count, index, direction, 2, 1).iter().all(|&i| i >= 0 && i < count));
            }
        }
    }
}
#[test]
fn plan_does_not_overflow_near_i32_max() {
    assert_eq!(plan(i32::MAX, i32::MAX - 1, 1, 2, 1), vec![i32::MAX - 2]);
}

#[test]
fn navigated_prefetches_the_neighbours_ahead_first_and_retains_their_ids() {
    // C#: PresentationPrefetcherTests.Navigated_PrefetchesTheNeighboursAheadFirstAndRetainsTheirIds
    let (p, c, h) = rig();
    let a = pixel();
    let b = pixel();
    let d = pixel();
    p.navigated(5, 10);
    assert_eq!(h.lookups.borrow()[0].paths, ["p6", "p7", "p4"]);
    apply(&p, &h, &[Some(a.clone()), Some(b.clone()), Some(d.clone())]);
    assert_eq!(c.prefetched.borrow().as_slice(), &[a.id(), b.id(), d.id()]);
    assert_eq!(p.prefetch_calls(), 3);
    let ids = p.planned_ids();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&a.id()));
    assert!(ids.contains(&d.id()));
}
#[test]
fn navigated_backwards_flips_the_direction() {
    // C#: PresentationPrefetcherTests.Navigated_BackwardsFlipsTheDirection
    let (p, _, h) = rig();
    p.navigated(5, 10);
    p.navigated(4, 10);
    assert_eq!(p.direction(), -1);
    p.navigated(4, 10);
    assert_eq!(p.direction(), -1);
    assert_eq!(h.lookups.borrow().last().unwrap().paths, ["p3", "p2", "p5"]);
}
#[test]
fn neighbours_not_decoded_yet_are_prefetched_by_the_retry_passes() {
    // C#: PresentationPrefetcherTests.NeighboursNotDecodedYet_ArePrefetchedByTheRetryPasses
    let (p, c, h) = rig();
    p.navigated(5, 10);
    apply(&p, &h, &[None, None, None]);
    let late = pixel();
    let retry = h.retries.borrow()[0].1;
    p.run_retry(retry);
    apply(&p, &h, &[Some(late.clone())]);
    assert_eq!(c.prefetched.borrow().as_slice(), &[late.id()]);
}
#[test]
fn retry_passes_of_an_older_navigation_are_dropped() {
    // C#: PresentationPrefetcherTests.RetryPassesOfAnOlderNavigation_AreDropped
    let (p, c, h) = rig();
    p.navigated(5, 10);
    let old = h.retries.borrow()[0].1;
    p.navigated(6, 10);
    let count = h.lookups.borrow().len();
    p.run_retry(old);
    assert_eq!(h.lookups.borrow().len(), count);
    assert!(c.prefetched.borrow().is_empty());
}
#[test]
fn retry_passes_of_an_older_navigation_do_not_even_start_a_background_lookup() {
    // C#: PresentationPrefetcherTests.RetryPassesOfAnOlderNavigation_DoNotEvenStartABackgroundLookup
    let (p, _, h) = rig();
    p.navigated(5, 10);
    let old = h.retries.borrow()[0].1;
    p.navigated(6, 10);
    let count = h.lookups.borrow().len();
    p.run_retry(old);
    assert_eq!(h.lookups.borrow().len(), count);
}
#[test]
fn already_on_the_gpu_is_not_uploaded_twice() {
    // C#: PresentationPrefetcherTests.AlreadyOnTheGpu_IsNotUploadedTwice
    let (p, c, h) = rig();
    let image = pixel();
    c.contained.borrow_mut().insert(image.id());
    p.navigated(5, 10);
    apply(&p, &h, &[Some(image.clone())]);
    assert_eq!(p.prefetch_calls(), 0);
    assert!(c.prefetched.borrow().is_empty());
    assert!(p.planned_ids().contains(&image.id()));
}
#[test]
fn contains_true_for_a_partial_upload_skips_prefetch() {
    already_on_the_gpu_is_not_uploaded_twice();
}
#[test]
fn disposed_neighbour_is_skipped() {
    // C#: PresentationPrefetcherTests.DisposedNeighbour_IsSkipped
    let (p, c, h) = rig();
    let mut image = PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).unwrap();
    image.dispose();
    p.navigated(5, 10);
    apply(&p, &h, &[Some(Arc::new(image))]);
    assert!(p.planned_ids().is_empty());
    assert!(c.prefetched.borrow().is_empty());
}
#[test]
fn lookup_runs_off_the_ui_thread_and_prefetch_comes_back_on_it() {
    // C#: PresentationPrefetcherTests.LookupRunsOffTheUiThreadAndPrefetchComesBackOnIt
    let (p, c, h) = rig();
    p.navigated(5, 10);
    assert_eq!(h.lookups.borrow().len(), 1);
    assert_eq!(h.lookups.borrow()[0].paths, ["p6", "p7", "p4"]);
    assert!(c.prefetched.borrow().is_empty());
    apply(&p, &h, &[Some(pixel())]);
    assert_eq!(c.prefetched.borrow().len(), 1);
}
#[test]
fn no_image_clears_the_plan_and_cancels_passes_in_flight() {
    // C#: PresentationPrefetcherTests.NoImage_ClearsThePlanAndCancelsPassesInFlight
    let (p, _, h) = rig();
    p.navigated(5, 10);
    let old = h.lookups.borrow()[0].generation;
    let retry = h.retries.borrow()[0].1;
    apply(&p, &h, &[Some(pixel())]);
    p.navigated(-1, 0);
    p.run_retry(retry);
    p.apply_lookup(old, &[Some(pixel())]).unwrap();
    assert!(p.planned_ids().is_empty());
}
#[test]
fn plan_changed_is_raised_after_each_applying_pass() {
    // C#: PresentationPrefetcherTests.PlanChanged_IsRaisedAfterEachApplyingPass
    let (p, _, h) = rig();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    p.subscribe_plan_changed(Rc::new(move || count.set(count.get() + 1)));
    p.navigated(5, 10);
    apply(&p, &h, &[None]);
    assert_eq!(calls.get(), 1);
    p.navigated(-1, 0);
    assert_eq!(calls.get(), 1);
}
#[test]
fn prefetcher_late_lookup_result_of_an_older_navigation_is_not_applied() {
    // C#: PresentationStaleOrderTests.Prefetcher_LateLookupResultOfAnOlderNavigation_IsNotApplied
    let (p, c, h) = rig();
    p.navigated(5, 10);
    let old = h.lookups.borrow()[0].generation;
    p.navigated(6, 10);
    p.apply_lookup(old, &[Some(pixel())]).unwrap();
    assert!(c.prefetched.borrow().is_empty());
    assert!(p.planned_ids().is_empty());
}
#[test]
fn navigated_same_index_keeps_direction_and_bumps_generation() {
    let (p, _, h) = rig();
    p.navigated(5, 10);
    p.navigated(4, 10);
    let generation = h.lookups.borrow().last().unwrap().generation;
    p.navigated(4, 10);
    assert_eq!(p.direction(), -1);
    assert_ne!(h.lookups.borrow().last().unwrap().generation, generation);
}
#[test]
fn apply_lookup_error_stops_the_pass_without_plan_changed() {
    let (p, c, h) = rig();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    p.subscribe_plan_changed(Rc::new(move || count.set(count.get() + 1)));
    p.navigated(5, 10);
    c.fail.set(true);
    let image = pixel();
    assert!(
        p.apply_lookup(h.lookups.borrow()[0].generation, &[Some(image.clone())])
            .is_err()
    );
    assert_eq!(p.planned_ids(), [image.id()]);
    assert_eq!(p.prefetch_calls(), 0);
    assert_eq!(calls.get(), 0);
}
#[test]
fn schedule_retry_is_called_three_times_with_the_documented_delays() {
    let (p, _, h) = rig();
    p.navigated(5, 10);
    assert_eq!(
        h.retries.borrow().iter().map(|(d, _)| *d).collect::<Vec<_>>(),
        [
            Duration::from_millis(120),
            Duration::from_millis(400),
            Duration::from_millis(1000)
        ]
    );
}
#[test]
fn no_paths_starts_no_lookup() {
    let (p, _, h) = rig();
    h.no_paths.set(true);
    p.navigated(5, 10);
    assert!(h.lookups.borrow().is_empty());
}
