//! INV-1 ("an OLD preview never overwrites the NEW one") at the sink level, port of the missing part of
//! `PhotoReview.App.Tests/Coordinators/StalePreviewOverwriteTests.cs` (baseline 7eaee2a2; lane cx-testgaps-g19):
//! `Burst_ThreeFiles_EveryPreviewCompletionOrder_...`, `Burst_RevisitingTheFirstFile_...` and `LateThumbnailOfAnOlderNavigation_...`.
//!
//! Every decode and thumbnail read is a gate the test opens; waits are bounded and gate-driven (no sleeps). The decode-completion order is driven by the order
//! in which the gates are opened: the invariants asserted (only the last navigation is ever presented, nothing of an older navigation reaches the sink) must
//! hold for every order, so the test runs all of them. Fakes only: the folder is a temp root with empty files; nothing touches the Recycle Bin.

mod support {
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pr_app::coordinators::zoom_detail::same_platform_image;
use pr_core::model::LoadingMode;
use pr_core::sync::CancelToken;
use pr_imaging::cache::thumbnails::EmbeddedThumbnailReaderFn;
use pr_imaging::service::PreviewImage;
use support::present_helpers::{PerPathGatedDecoder, images_shown, presented_paths, pump_one, start, statuses};
use support::presenter_fakes::{FixtureOptions, PresenterFixture, build_thumbnail_cache_with};
use support::preview_fakes::{Gate, lock, sized_image};

const NAMES: [&str; 3] = ["g19-a.jpg", "g19-b.jpg", "g19-c.jpg"];

/// Every order of `items` (small slices only).
fn permutations(items: &[usize]) -> Vec<Vec<usize>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut result = Vec::new();
    for (i, first) in items.iter().enumerate() {
        let mut rest = items.to_vec();
        rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, *first);
            result.push(tail);
        }
    }
    result
}

fn index_of(file: usize) -> i32 {
    i32::try_from(file).unwrap_or_else(|error| panic!("index {file}: {error}"))
}

/// Navigates through `targets` (every navigation but the last has its stat answered and its viewer decode really inside the decoder when it is superseded),
/// then opens the decode gates of `release_order` one by one and checks that only the last navigation was ever presented.
fn run_burst(targets: &[usize], release_order: &[usize]) {
    let decoder = PerPathGatedDecoder::new();
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &NAMES);
    f.settings.loading_mode = Some(LoadingMode::Fast); // no thumbnail event: the PREVIEW events are the ones that meet the superseded tokens
    let paths: Vec<String> = NAMES.iter().map(|name| f.path(name)).collect();
    let scenario = format!("targets {targets:?}, gates opened in order {release_order:?}");

    let (last, earlier) = targets
        .split_last()
        .unwrap_or_else(|| panic!("{scenario}: no navigation"));
    for target in earlier {
        start(&mut f, index_of(*target));
        pump_one(&mut f); // the stat came back: the loading status shows and the viewer decode is requested
        assert!(
            decoder.wait_started(&paths[*target]),
            "{scenario}: decode of {target} never started"
        );
    }
    start(&mut f, index_of(*last)); // supersedes every decode above

    for file in release_order {
        decoder.open(&paths[*file]);
        f.pump(); // whatever finished meanwhile is applied; the order of the gates is the order of completion
    }
    f.pump_until_idle();

    assert!(
        decoder.timed_out().is_empty(),
        "{scenario}: a gate was never opened: {:?}",
        decoder.timed_out()
    );
    assert_eq!(presented_paths(&f), vec![paths[*last].clone()], "{scenario}");
    assert_eq!(images_shown(&f), 1, "{scenario}: exactly the last navigation's image");
    assert!(f.presenter.current_image().is_some(), "{scenario}");
    assert_eq!(f.catalog.current_index(), index_of(*last), "{scenario}");
    for (file, name) in NAMES.iter().enumerate() {
        if file != *last {
            assert!(
                statuses(&f).iter().all(|status| !status.contains(name)),
                "{scenario}: a status of the older navigation {name} reached the sink: {:?}",
                statuses(&f)
            );
        }
    }
}

// C#: StalePreviewOverwriteTests.Burst_ThreeFiles_EveryPreviewCompletionOrder_NeverShowsAnOlderImage (3! = 6 gate orders; the sink never sees an older image)
#[test]
fn burst_three_files_every_decode_completion_order_never_shows_an_older_image() {
    let orders = permutations(&[0, 1, 2]);
    assert_eq!(orders.len(), 6, "guards the enumerator itself");
    for order in orders {
        run_burst(&[0, 1, 2], &order);
    }
}

// C#: StalePreviewOverwriteTests.Burst_RevisitingTheFirstFile_EveryPreviewCompletionOrder_NeverShowsAnOlderImage (0, 1, then 0 again)
#[test]
fn burst_revisiting_the_first_file_every_decode_completion_order_never_shows_the_other_image() {
    for order in permutations(&[0, 1]) {
        run_burst(&[0, 1, 0], &order);
    }
}

// C#: StalePreviewOverwriteTests.LateThumbnailOfAnOlderNavigation_IsDroppedNotFlashedOverTheNewImage
// File a's thumbnail read and decode are both held; the user moves to b, which presents; THEN a's thumbnail (and decode) finish: the sink must not get them.
#[test]
fn late_thumbnail_of_an_older_navigation_is_dropped_not_flashed_over_the_new_image() {
    let decoder = PerPathGatedDecoder::new();
    let read_started = Gate::new();
    let release_thumbnail = Gate::new();
    let thumbnail_returned = Arc::new(AtomicBool::new(false));
    let old_thumbnail: PreviewImage = sized_image(160, 160, 160, 160, false);
    let reader: EmbeddedThumbnailReaderFn = {
        let (read_started, release_thumbnail, returned) = (
            Arc::clone(&read_started),
            Arc::clone(&release_thumbnail),
            Arc::clone(&thumbnail_returned),
        );
        let old_thumbnail = Arc::clone(&old_thumbnail);
        Arc::new(move |path: &str, _cancel: &CancelToken| {
            if !path.to_lowercase().ends_with("g19-a.jpg") {
                return Ok(None); // b has no embedded thumbnail
            }
            read_started.open();
            assert!(release_thumbnail.wait(), "the thumbnail gate of a was never opened");
            returned.store(true, Ordering::SeqCst);
            Ok(Some(Arc::clone(&old_thumbnail)))
        })
    };
    let scratch = pr_test_support::TempRoot::new("g19-stale-thumbs");
    let cache = build_thumbnail_cache_with(&scratch.join("thumbs"), reader);
    let mut f = PresenterFixture::with_options(
        Arc::clone(&decoder) as _,
        &NAMES[..2],
        FixtureOptions {
            thumbnails: Some(cache),
            ..FixtureOptions::default()
        },
    );
    f.settings.loading_mode = Some(LoadingMode::Preview);
    let (a, b) = (f.path(NAMES[0]), f.path(NAMES[1]));

    start(&mut f, 0);
    pump_one(&mut f); // a's stat: its thumbnail read and its preview decode are requested
    assert!(read_started.wait(), "a's thumbnail read never started");
    start(&mut f, 1); // the user moves on while a's thumbnail is still being read
    pump_one(&mut f); // b's stat: its decode is requested
    assert!(decoder.wait_started(&b));
    decoder.open(&b);
    f.pump_until(|f| presented_paths(f).contains(&b));
    let shown_for_b = images_shown(&f);
    assert_eq!(shown_for_b, 1, "b's preview is on screen");

    release_thumbnail.open(); // a's thumbnail arrives LATE
    decoder.open(&a); // and so does a's preview
    f.pump_until(|_| thumbnail_returned.load(Ordering::SeqCst));
    f.pump_until_idle();

    assert_eq!(
        images_shown(&f),
        shown_for_b,
        "nothing of a reached the sink after b was presented"
    );
    assert_eq!(presented_paths(&f), vec![b]);
    let on_screen = f.presenter.current_image().expect("an image is shown");
    assert!(
        !same_platform_image(on_screen, old_thumbnail.platform_image()),
        "the OLD thumbnail was flashed over the new image"
    );
    assert!(lock(&f.sink_log).images.iter().all(|call| {
        call.image
            .as_ref()
            .is_none_or(|shown| !same_platform_image(shown, old_thumbnail.platform_image()))
    }));
}
