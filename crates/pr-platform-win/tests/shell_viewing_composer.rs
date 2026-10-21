//! Tests of `PresentationComposer` (W8-16c, kind F): ports of `PresentationComposerTests.cs` and `PresentationStaleOrderTests.cs` (baseline
//! `d92d6aae`) plus the trap tests of `contracts/W8-16c.md`. The composer runs on a probe GPU cache whose background stripes the test finishes by hand,
//! a fake frame clock, a manual deadline scheduler and a settable timestamp (1 tick = 1 ms). No GPU, no sleeps.
#![cfg(windows)]

#[path = "common/viewing_composer_fixture.rs"]
mod fixture;
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use pr_app::viewport::render_geometry::ImageInterpolation;
use pr_core::model::enums::ScalingQuality;
use pr_imaging::pixels::PixelBuffer;
use pr_platform_win::host::frame_clock::base::qpc_frequency;
use pr_platform_win::shell_viewing::compare::{self, CompareFrame, CompareSide, layout};
use pr_platform_win::shell_viewing::composer::{
    ComposerError, ComposerOptions, MAX_DEFER_MS, PresentationComposer, PresentationInputs,
};

use fixture::{Call, Rec, Rig, buffer, client, disposed_buffer, inputs, rect, the_box};

fn same(actual: &Option<Arc<PixelBuffer>>, expected: &Arc<PixelBuffer>) -> bool {
    actual.as_ref().is_some_and(|actual| Arc::ptr_eq(actual, expected))
}

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// Frame inputs with a compare panel.
fn compare_inputs<'a>(frame: CompareFrame<'a>, quality: ScalingQuality) -> PresentationInputs<'a> {
    PresentationInputs {
        viewport: client(),
        main_destination: the_box(),
        effective_quality: quality,
        compare_quality: quality,
        dpi_scale: 1.0,
        compare_visible: true,
        compare: frame,
    }
}

// ---- layers and order ---------------------------------------------------------------------------------------------------------------------

// C#: PresentationComposerTests.Draw_MainImage_UsesTheLayoutBoxUnderAViewportClip
#[test]
fn draw_main_image_uses_the_layout_box_under_a_viewport_clip() {
    let rig = Rig::new();
    let image = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&image)));

    let (frame, rec) = rig.draw(&inputs(the_box()));

    assert!(frame.main_drawn);
    assert!(!frame.crossfade_drawn);
    assert_eq!(rec.names(), ["push", "image", "pop"]);
    assert_eq!(rec.pushed(), [rect(0.0, 0.0, 1000.0, 700.0)]);
    assert_eq!(
        rec.images(),
        [(image.id(), the_box(), ImageInterpolation::HighQualityCubic, 1.0)]
    );
    assert_eq!(rec.depth, 0);
    assert_eq!(frame.interpolation, ImageInterpolation::HighQualityCubic);
    assert!(!frame.degraded);
}

// C#: PresentationComposerTests.Draw_UsesTheEffectiveScalingQuality_ForTheMainImage
#[test]
fn draw_uses_the_effective_scaling_quality_for_the_main_image() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(100, 100)));
    let mut input = inputs(the_box());
    input.effective_quality = ScalingQuality::Linear;
    input.compare_quality = ScalingQuality::HighQuality;

    let (frame, rec) = rig.draw(&input);

    assert_eq!(rec.images()[0].2, ImageInterpolation::Linear);
    assert_eq!(frame.interpolation, ImageInterpolation::Linear);
}

// C#: PresentationComposerTests.Draw_WithNothingToShow_DrawsNothing
#[test]
fn draw_with_nothing_to_show_draws_nothing() {
    let rig = Rig::new();

    let (frame, rec) = rig.draw(&inputs(the_box()));

    assert!(!frame.main_drawn);
    assert!(!frame.crossfade_drawn);
    assert_eq!(frame.compare_images, 0);
    assert!(rec.calls.is_empty());
}

// C#: PresentationComposerTests.Draw_DisposedCurrent_DrawsNothingAndDoesNotThrow (a buffer disposed BEFORE `set`)
#[test]
fn draw_disposed_current_draws_nothing_and_does_not_fail() {
    let rig = Rig::new();
    rig.composer.set(Some(disposed_buffer()));

    let (frame, rec) = rig.draw(&inputs(the_box()));

    assert!(!frame.main_drawn);
    assert!(rec.calls.is_empty());
    assert!(rig.composer.last_drawn().is_none());
}

// C#: PresentationComposerTests.LayersAreDrawnInXamlOrder_MainThenOutgoingThenComparePanel
#[test]
fn layers_are_drawn_in_xaml_order_main_then_outgoing_then_compare_panel() {
    let rig = Rig::new();
    let outgoing = buffer(100, 100);
    let incoming = buffer(100, 100);
    let left = buffer(100, 100);
    let right = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&outgoing)));
    rig.draw(&inputs(the_box())); // the frame the user saw
    assert_eq!(rig.composer.begin_crossfade(ms(200)), Ok(true));
    assert_eq!(
        rig.composer.set_incoming(Some(Arc::clone(&incoming)), true, 0),
        Ok(true)
    );
    let frame = CompareFrame {
        left: Some(&left),
        right: Some(&right),
        left_selected: true,
        right_selected: false,
    };

    let (composed, rec) = rig.draw(&compare_inputs(frame, ScalingQuality::HighQuality));

    assert!(composed.main_drawn);
    assert!(composed.crossfade_drawn);
    assert_eq!(composed.compare_images, 2);
    let ids: Vec<i64> = rec.images().iter().map(|image| image.0).collect();
    assert_eq!(ids, [incoming.id(), outgoing.id(), left.id(), right.id()]);
    assert_eq!(rec.images()[1].3, 1.0); // the outgoing layer starts at 1
    let first_fill = rec
        .calls
        .iter()
        .position(|call| matches!(call, Call::Fill(..)))
        .unwrap();
    let outgoing_at = rec
        .calls
        .iter()
        .rposition(|call| matches!(call, Call::Image { id, .. } if *id == outgoing.id()))
        .unwrap();
    assert!(
        first_fill > outgoing_at,
        "the compare panel must be drawn AFTER the main image and the outgoing layer"
    );
    assert_eq!(rec.depth, 0);
}

// C#: PresentationComposerTests.Compare_DrawsCanvasPanel_ThenSelectionBorders_ThenImagesInsideTheirCells
#[test]
fn compare_draws_canvas_panel_then_selection_borders_then_images_inside_their_cells() {
    let rig = Rig::new();
    let left = buffer(200, 100);
    let right = buffer(100, 200);
    let frame = CompareFrame {
        left: Some(&left),
        right: Some(&right),
        left_selected: false,
        right_selected: true,
    };

    let (composed, rec) = rig.draw(&compare_inputs(frame, ScalingQuality::Linear));

    assert_eq!(composed.compare_images, 2);
    let fills = rec.fills();
    assert_eq!(fills[0], (layout::panel(client()), compare::canvas()));
    assert!(fills[1..5].iter().all(|fill| fill.1 == compare::unselected_color()));
    assert!(fills[5..9].iter().all(|fill| fill.1 == compare::selected_color()));
    let images = rec.images();
    assert_eq!(images[0].1, layout::image(client(), CompareSide::Left, 200, 100));
    assert_eq!(images[1].1, layout::image(client(), CompareSide::Right, 100, 200));
    // The two cells use `compare_quality`, not the main image's.
    assert!(images.iter().all(|image| image.2 == ImageInterpolation::Linear));
}

#[test]
fn compare_cells_use_the_compare_quality_not_the_effective_quality() {
    let rig = Rig::new();
    let left = buffer(10, 10);
    let frame = CompareFrame {
        left: Some(&left),
        right: None,
        left_selected: false,
        right_selected: false,
    };
    let mut input = compare_inputs(frame, ScalingQuality::HighQuality);
    input.compare_quality = ScalingQuality::Linear;

    let (_, rec) = rig.draw(&input);

    assert_eq!(rec.images()[0].2, ImageInterpolation::Linear);
}

// C#: PresentationComposerTests.Compare_NotVisible_DrawsNoPanel
#[test]
fn compare_not_visible_draws_no_panel() {
    let rig = Rig::new();
    let left = buffer(10, 10);
    rig.composer.set(Some(buffer(10, 10)));
    let frame = CompareFrame {
        left: Some(&left),
        right: Some(&left),
        left_selected: true,
        right_selected: false,
    };
    let mut input = compare_inputs(frame, ScalingQuality::HighQuality);
    input.compare_visible = false;

    let (composed, rec) = rig.draw(&input);

    assert_eq!(composed.compare_images, 0);
    assert!(rec.fills().is_empty());
}

// C#: PresentationComposerTests.Compare_MissingOrDisposedImage_StillDrawsPanelAndBorders
#[test]
fn compare_missing_or_disposed_image_still_draws_panel_and_borders() {
    let rig = Rig::new();
    let disposed = disposed_buffer();
    let frame = CompareFrame {
        left: None,
        right: Some(&disposed),
        left_selected: true,
        right_selected: false,
    };

    let (composed, rec) = rig.draw(&compare_inputs(frame, ScalingQuality::HighQuality));

    assert_eq!(composed.compare_images, 0);
    assert_eq!(rec.fills().len(), 9); // panel + 2 x 4 border sides
    assert!(rec.images().is_empty());
}

// ---- same-image upgrade (ADR 0008) ---------------------------------------------------------------------------------------------------------

// C#: PresentationComposerTests.Upgrade_KeepsTheOldBitmapOnScreenUntilTheNewOneIsUploaded_ThenDrawsItInTheSameBox
#[test]
fn upgrade_keeps_the_old_bitmap_on_screen_until_the_new_one_is_uploaded_then_draws_it_in_the_same_box() {
    let rig = Rig::new();
    let preview = buffer(100, 100);
    let full = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&preview)));
    rig.draw(&inputs(the_box()));
    let changed = Rc::new(Cell::new(0));
    let counter = Rc::clone(&changed);
    rig.composer
        .subscribe_current_changed(Rc::new(move || counter.set(counter.get() + 1)));

    let replaced_now = rig.composer.set_incoming(Some(Arc::clone(&full)), false, 1);

    assert_eq!(replaced_now, Ok(false));
    assert!(same(&rig.composer.current(), &preview));
    assert!(same(&rig.composer.pending(), &full));
    assert_eq!(rig.probe.prefetch_calls.borrow().as_slice(), [full.id()]);
    let (_, before) = rig.draw(&inputs(the_box()));
    assert_eq!(before.images().len(), 1);
    assert_eq!(before.images()[0].0, preview.id());
    let invalidations = rig.invalidations.get();

    assert!(rig.probe.finish_next()); // stripes run; only the last one completes the bitmap
    assert!(same(&rig.composer.current(), &preview));
    rig.probe.finish_all();

    assert!(same(&rig.composer.current(), &full));
    assert!(rig.composer.pending().is_none());
    assert_eq!(changed.get(), 1);
    assert!(rig.probe.uploaded.borrow().contains(&full.id()));
    let (_, after) = rig.draw(&inputs(the_box()));
    let drawn = after.images();
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].0, full.id());
    assert_eq!(drawn[0].1, before.images()[0].1); // no layout jump
    assert_eq!(rig.invalidations.get(), invalidations + 1); // the next frame is requested on promotion
    assert_eq!(rig.composer.promotion_count(), 1);
}

// C#: PresentationComposerTests.Upgrade_PrefetchedAlready_ReplacesImmediately
#[test]
fn upgrade_prefetched_already_replaces_immediately() {
    let rig = Rig::new();
    let preview = buffer(100, 100);
    let full = buffer(100, 100);
    rig.composer.set(Some(preview));
    rig.probe.preload(full.id()); // already fully on the GPU
    let changed = Rc::new(Cell::new(0));
    let counter = Rc::clone(&changed);
    rig.composer
        .subscribe_current_changed(Rc::new(move || counter.set(counter.get() + 1)));

    assert_eq!(rig.composer.set_incoming(Some(Arc::clone(&full)), false, 1), Ok(true));

    assert!(same(&rig.composer.current(), &full));
    assert!(rig.composer.pending().is_none());
    assert_eq!(
        changed.get(),
        0,
        "the caller sees `true`; CurrentChanged is only for promotions"
    );
    assert_eq!(rig.composer.promotion_count(), 0);
    assert!(rig.scheduled.borrow().is_empty());
}

// C#: PresentationComposerTests.Upgrade_BelowTheDeferThreshold_ReplacesImmediately
#[test]
fn upgrade_below_the_defer_threshold_replaces_immediately() {
    let rig = Rig::new();
    let thumbnail = buffer(10, 10);
    let preview = buffer(100, 100); // 40 000 bytes
    rig.composer.set(Some(thumbnail));

    assert_eq!(
        rig.composer.set_incoming(Some(Arc::clone(&preview)), false, 40_001),
        Ok(true)
    );
    assert!(same(&rig.composer.current(), &preview));

    let larger = buffer(100, 101); // 40 400 bytes: equal to the threshold is NOT below it
    assert_eq!(
        rig.composer.set_incoming(Some(Arc::clone(&larger)), false, 40_400),
        Ok(false)
    );
    assert!(same(&rig.composer.current(), &preview));
    assert!(same(&rig.composer.pending(), &larger));
}

// C#: PresentationComposerTests.FileChange_ReplacesImmediately_AndDropsAPendingUpgrade
#[test]
fn file_change_replaces_immediately_and_drops_a_pending_upgrade() {
    let rig = Rig::new();
    let first = buffer(100, 100);
    let upgrade = buffer(100, 100);
    let next = buffer(100, 100);
    rig.composer.set(Some(first));
    rig.composer.set_incoming(Some(Arc::clone(&upgrade)), false, 1).unwrap();
    assert!(same(&rig.composer.pending(), &upgrade));

    assert_eq!(rig.composer.set_incoming(Some(Arc::clone(&next)), true, 1), Ok(true));
    rig.probe.finish_all(); // the stripes of the dropped upgrade change nothing

    assert!(same(&rig.composer.current(), &next));
    assert!(rig.composer.pending().is_none());
    assert_eq!(rig.composer.promotion_count(), 0);
}

// C#: PresentationComposerTests.ClearingTheImage_ReplacesImmediately_EvenWithAPendingUpgrade
#[test]
fn clearing_the_image_replaces_immediately_even_with_a_pending_upgrade() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(100, 100)));
    rig.composer.set_incoming(Some(buffer(100, 100)), false, 1).unwrap();
    assert!(rig.composer.pending().is_some());

    assert_eq!(rig.composer.set_incoming(None, false, 1), Ok(true));

    assert!(rig.composer.current().is_none());
    assert!(rig.composer.pending().is_none());
}

// C#: PresentationComposerTests.FirstImage_WithNothingToUpgradeFrom_ReplacesImmediately
#[test]
fn first_image_with_nothing_to_upgrade_from_replaces_immediately() {
    let rig = Rig::new();

    assert_eq!(rig.composer.set_incoming(Some(buffer(100, 100)), false, 1), Ok(true));

    assert!(rig.composer.current().is_some());
    assert!(rig.probe.prefetch_calls.borrow().is_empty());
}

#[test]
fn an_incoming_disposed_image_replaces_immediately_without_a_prefetch() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(100, 100)));
    let dead = disposed_buffer();

    assert_eq!(rig.composer.set_incoming(Some(Arc::clone(&dead)), false, 1), Ok(true));

    assert!(same(&rig.composer.current(), &dead));
    assert!(rig.probe.prefetch_calls.borrow().is_empty());
    assert!(rig.composer.pending().is_none());
}

#[test]
fn set_drops_a_pending_upgrade_and_replaces_current() {
    let rig = Rig::new();
    let other = buffer(100, 100);
    rig.composer.set(Some(buffer(100, 100)));
    rig.composer.set_incoming(Some(buffer(100, 100)), false, 1).unwrap();

    rig.composer.set(Some(Arc::clone(&other)));

    assert!(same(&rig.composer.current(), &other));
    assert!(rig.composer.pending().is_none());
    rig.composer.set(None);
    assert!(rig.composer.current().is_none());
}

// C#: PresentationComposerTests.PendingUpgrade_AfterTheDeadline_IsPromotedEvenIfTheUploadNeverCompletes
#[test]
fn pending_upgrade_after_the_deadline_is_promoted_even_if_the_upload_never_completes() {
    let rig = Rig::new();
    let preview = buffer(100, 100);
    let full = buffer(100, 100);
    rig.composer.set(Some(preview));
    rig.composer.set_incoming(Some(Arc::clone(&full)), false, 1).unwrap();
    assert_eq!(rig.scheduled.borrow().len(), 1);
    assert_eq!(rig.scheduled.borrow()[0].delay, ms(MAX_DEFER_MS as u64));

    rig.advance(500);
    assert!(!rig.composer.try_promote_pending()); // not due yet
    rig.advance(600);
    rig.fire(0); // the deadline (starved cache)

    assert!(same(&rig.composer.current(), &full));
    assert_eq!(rig.composer.promotion_count(), 1);
    assert_eq!(rig.invalidations.get(), 1);
}

#[test]
fn promotion_by_time_happens_exactly_at_max_defer_ms() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(100, 100)));
    let full = buffer(100, 100);
    rig.composer.set_incoming(Some(Arc::clone(&full)), false, 1).unwrap();

    rig.advance(MAX_DEFER_MS - 1);
    assert!(!rig.composer.try_promote_pending());
    assert!(rig.composer.pending().is_some());
    rig.advance(1);
    assert!(rig.composer.try_promote_pending()); // waited == limit is due
    assert!(same(&rig.composer.current(), &full));
    assert!(!rig.composer.try_promote_pending()); // nothing pending any more
}

#[test]
fn ticks_per_second_below_one_uses_the_qpc_frequency() {
    let rig = Rig::with(120, 0);
    let frequency = qpc_frequency();
    rig.composer.set(Some(buffer(100, 100)));
    rig.composer.set_incoming(Some(buffer(100, 100)), false, 1).unwrap();

    rig.advance(frequency - 1);
    assert!(!rig.composer.try_promote_pending());
    rig.advance(1);
    assert!(rig.composer.try_promote_pending());
}

#[test]
fn a_custom_tick_rate_scales_the_deadline() {
    let rig = Rig::with(120, 10); // 10 ticks per second: 1000 ms = 10 ticks
    rig.composer.set(Some(buffer(100, 100)));
    rig.composer.set_incoming(Some(buffer(100, 100)), false, 1).unwrap();

    rig.advance(9);
    assert!(!rig.composer.try_promote_pending());
    rig.advance(1);
    assert!(rig.composer.try_promote_pending());
}

// C#: PresentationComposerTests.StaleDeadline_OfASupersededUpgrade_DoesNothing
#[test]
fn stale_deadline_of_a_superseded_upgrade_does_nothing() {
    let rig = Rig::new();
    let preview = buffer(100, 100);
    let first = buffer(100, 100);
    let second = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&preview)));
    rig.composer.set_incoming(Some(first), false, 1).unwrap();
    rig.composer.set_incoming(Some(Arc::clone(&second)), false, 1).unwrap();

    rig.advance(2000);
    rig.fire(0); // the deadline of `first`

    assert!(same(&rig.composer.current(), &preview));
    assert!(same(&rig.composer.pending(), &second));
    assert_eq!(rig.invalidations.get(), 0);
    rig.fire(1); // the live one promotes
    assert!(same(&rig.composer.current(), &second));
}

// New (T1): the token guards a re-armed pending of the SAME buffer.
#[test]
fn deadline_token_guards_a_rearmed_pending_of_the_same_buffer() {
    let rig = Rig::new();
    let preview = buffer(100, 100);
    let full = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&preview)));
    rig.composer.set_incoming(Some(Arc::clone(&full)), false, 1).unwrap();
    rig.composer.set_incoming(Some(Arc::clone(&full)), false, 1).unwrap(); // same buffer again: a new token
    assert_eq!(rig.scheduled.borrow().len(), 2);

    rig.advance(2000);
    rig.fire(0); // the first deadline carries the old token

    assert!(same(&rig.composer.current(), &preview));
    assert!(same(&rig.composer.pending(), &full));
    rig.fire(1);
    assert!(same(&rig.composer.current(), &full));
    assert_eq!(rig.composer.promotion_count(), 1);
}

// New (T1): the id check of UploadCompleted. The pending upgrade is overdue, so only the id can stop an unrelated completion from promoting it.
#[test]
fn upload_completed_of_another_buffer_is_ignored() {
    let rig = Rig::new();
    let preview = buffer(100, 100);
    let full = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&preview)));
    rig.composer.set_incoming(Some(Arc::clone(&full)), false, 1).unwrap();
    rig.advance(5000);

    rig.probe.raise(full.id() + 1_000_000);

    assert!(same(&rig.composer.current(), &preview));
    assert!(same(&rig.composer.pending(), &full));
    assert_eq!(rig.invalidations.get(), 0);
    rig.probe.raise(full.id()); // its own completion promotes (overdue anyway)
    assert!(same(&rig.composer.current(), &full));
    assert_eq!(rig.invalidations.get(), 1);
}

#[test]
fn upload_completed_without_a_pending_upgrade_is_ignored() {
    let rig = Rig::new();
    let current = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&current)));

    rig.probe.raise(current.id());

    assert!(same(&rig.composer.current(), &current));
    assert_eq!(rig.invalidations.get(), 0);
    assert_eq!(rig.composer.promotion_count(), 0);
}

// New (T2): `prefetch` raises UploadCompleted from inside the call, while `set_incoming` is mid-way.
#[test]
fn set_incoming_with_a_prefetch_that_completes_synchronously_is_not_lost() {
    let rig = Rig::new();
    let preview = buffer(4, 4);
    let tiny = buffer(4, 4);
    rig.composer.set(Some(preview));
    rig.probe.complete_on_prefetch.set(true);
    let changed = Rc::new(Cell::new(0));
    let counter = Rc::clone(&changed);
    rig.composer
        .subscribe_current_changed(Rc::new(move || counter.set(counter.get() + 1)));

    let replaced_now = rig.composer.set_incoming(Some(Arc::clone(&tiny)), false, 1);

    assert_eq!(replaced_now, Ok(true));
    assert!(same(&rig.composer.current(), &tiny));
    assert!(rig.composer.pending().is_none());
    assert_eq!(rig.composer.promotion_count(), 0); // the event inside prefetch saw no pending and was ignored
    assert_eq!(changed.get(), 0);
    assert_eq!(rig.invalidations.get(), 0);
    assert!(rig.scheduled.borrow().is_empty());
}

#[test]
fn a_failing_prefetch_is_reported_and_leaves_only_pending_cleared() {
    let rig = Rig::new();
    let preview = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&preview)));
    rig.composer.set_incoming(Some(buffer(100, 100)), false, 1).unwrap();
    assert!(rig.composer.pending().is_some());
    rig.probe.fail_prefetch.set(true);

    let result = rig.composer.set_incoming(Some(buffer(100, 100)), false, 1);

    assert!(matches!(result, Err(ComposerError::Render(_))));
    assert!(same(&rig.composer.current(), &preview));
    assert!(rig.composer.pending().is_none());
    assert_eq!(rig.scheduled.borrow().len(), 1); // no new deadline for the failed one
}

// C#: PresentationComposerTests.RandomNavigation_AlwaysEndsOnTheLastAssignedImage (seeds 1-6)
#[test]
fn random_navigation_always_ends_on_the_last_assigned_image() {
    for seed in 1..=6_u64 {
        // A deterministic LCG replaces `System.Random` (the SEQUENCE need not match C#).
        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut next = move |bound: u64| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % bound
        };
        let rig = Rig::new();
        let mut keep: Vec<Arc<PixelBuffer>> = Vec::new();
        let mut last: Option<Arc<PixelBuffer>> = None;
        for _ in 0..60 {
            match next(5) {
                0 => {
                    let image = buffer(100, 100);
                    keep.push(Arc::clone(&image));
                    last = Some(Arc::clone(&image));
                    rig.composer.set_incoming(Some(image), true, 1).unwrap();
                }
                1 => {
                    if last.is_some() {
                        let image = buffer(100, 100);
                        keep.push(Arc::clone(&image));
                        last = Some(Arc::clone(&image));
                        rig.composer.set_incoming(Some(image), false, 1).unwrap();
                    }
                }
                2 => {
                    for _ in 0..=next(5) {
                        rig.probe.finish_next();
                    }
                }
                3 => {
                    rig.advance(i64::try_from(next(700)).unwrap());
                    rig.composer.try_promote_pending();
                }
                _ => {
                    rig.draw(&inputs(the_box()));
                    rig.composer.begin_crossfade(ms(120)).unwrap();
                }
            }
            if let Some(current) = rig.composer.current() {
                assert!(!current.is_disposed());
            }
        }

        rig.probe.finish_all();
        rig.advance(5000);
        let deadlines = rig.scheduled.borrow().len();
        for index in 0..deadlines {
            rig.fire(index);
        }
        rig.composer.try_promote_pending();

        match &last {
            Some(last) => assert!(same(&rig.composer.current(), last), "seed {seed}"),
            None => assert!(rig.composer.current().is_none(), "seed {seed}"),
        }
        assert!(rig.composer.pending().is_none(), "seed {seed}");
        assert!(!keep.is_empty() || last.is_none());
    }
}

// ---- seen frame, crossfade, retained ids --------------------------------------------------------------------------------------------------

// C#: PresentationComposerTests.BeginCrossfade_FreezesTheFrameThatWasActuallyDrawn
#[test]
fn begin_crossfade_freezes_the_frame_that_was_actually_drawn() {
    let rig = Rig::new();
    let first = buffer(100, 100);
    let second = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&first)));
    assert_eq!(rig.composer.begin_crossfade(ms(100)), Ok(false)); // nothing drawn yet: nothing to fade
    let old_box = rect(30.0, 20.0, 500.0, 400.0);
    let mut input = inputs(old_box);
    input.effective_quality = ScalingQuality::Linear;
    rig.draw(&input);

    assert_eq!(rig.composer.begin_crossfade(ms(100)), Ok(true));
    rig.composer.set_incoming(Some(Arc::clone(&second)), true, 0).unwrap();
    let (_, rec) = rig.draw(&inputs(rect(0.0, 0.0, 10.0, 10.0))); // the new layout must not move the old layer

    assert!(same(&rig.composer.crossfade().outgoing(), &first));
    let outgoing = rec.images().into_iter().find(|image| image.0 == first.id()).unwrap();
    assert_eq!((outgoing.1, outgoing.2), (old_box, ImageInterpolation::Linear));
}

#[test]
fn begin_crossfade_with_a_zero_duration_is_refused_by_the_layer() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(100, 100)));
    rig.draw(&inputs(the_box()));

    assert_eq!(rig.composer.begin_crossfade(Duration::ZERO), Ok(false));
    assert!(!rig.composer.crossfade().is_active());
}

// C#: PresentationStaleOrderTests.Composer_CrossfadeRequestedBeforeTheNewImageWasDrawn_FreezesNothingNew
#[test]
fn crossfade_requested_before_the_new_image_was_drawn_freezes_nothing_new() {
    let rig = Rig::new();
    let seen = buffer(8, 8);
    rig.composer.set_incoming(Some(Arc::clone(&seen)), true, 1).unwrap();
    rig.draw(&inputs(the_box())); // file 0 drawn
    assert_eq!(rig.composer.begin_crossfade(ms(100)), Ok(true)); // fade away from file 0
    rig.composer.set_incoming(Some(buffer(8, 8)), true, 1).unwrap(); // file 1 assigned, NOT drawn yet

    let begun = rig.composer.begin_crossfade(ms(100)); // a third navigation asks for a fade

    assert_eq!(begun, Ok(false)); // file 1 was never on screen
    assert!(rig.composer.last_drawn().is_none());
    assert!(same(&rig.composer.crossfade().outgoing(), &seen)); // the running fade keeps what the user really saw
    assert_eq!(rig.composer.crossfade().start_count(), 1);
}

#[test]
fn cancel_crossfade_releases_the_old_image_and_the_clock() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(100, 100)));
    rig.draw(&inputs(the_box()));
    rig.composer.begin_crossfade(ms(100)).unwrap();
    assert!(rig.composer.crossfade().is_active());
    assert_eq!(rig.clock.subscribers(), 1);

    rig.composer.cancel_crossfade();

    assert!(!rig.composer.crossfade().is_active());
    assert_eq!(rig.clock.subscribers(), 0);
    let mut ids = BTreeSet::new();
    rig.composer.collect_retained_ids(&mut ids);
    assert_eq!(ids.len(), 1); // only `current`
}

// New: `last_drawn` is set by a draw that really drew.
#[test]
fn last_drawn_is_none_until_a_draw_succeeded() {
    let rig = Rig::new();
    let image = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&image)));
    assert!(rig.composer.last_drawn().is_none()); // never drawn

    rig.probe.fail_get.set(true);
    let mut rec = Rec::default();
    assert!(rig.composer.draw(&mut rec, &inputs(the_box())).is_err());
    assert!(rig.composer.last_drawn().is_none()); // the failed draw does not count
    rig.probe.fail_get.set(false);

    let (frame, _) = rig.draw(&inputs(rect(f64::NAN, 0.0, 10.0, 10.0)));
    assert!(!frame.main_drawn); // try_draw refuses a NaN box
    assert!(rig.composer.last_drawn().is_none());

    rig.draw(&inputs(the_box()));
    let (buffer_drawn, frame_box, interpolation) = rig.composer.last_drawn().unwrap();
    assert!(Arc::ptr_eq(&buffer_drawn, &image));
    assert_eq!(
        (frame_box, interpolation),
        (the_box(), ImageInterpolation::HighQualityCubic)
    );

    rig.composer.set(Some(buffer(100, 100))); // another buffer is current now: the drawn frame is not the current one
    assert!(rig.composer.last_drawn().is_none());
}

// C#: PresentationComposerTests.CollectRetainedIds_CoversCurrentPendingAndOutgoing
#[test]
fn collect_retained_ids_covers_current_pending_and_outgoing() {
    let rig = Rig::new();
    let current = buffer(100, 100);
    let pending = buffer(100, 100);
    let outgoing = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&outgoing)));
    rig.draw(&inputs(the_box()));
    rig.composer.begin_crossfade(ms(200)).unwrap();
    rig.composer.set_incoming(Some(Arc::clone(&current)), true, 0).unwrap();
    rig.composer.set_incoming(Some(Arc::clone(&pending)), false, 1).unwrap();

    let mut ids = BTreeSet::new();
    rig.composer.collect_retained_ids(&mut ids);

    assert_eq!(ids, BTreeSet::from([current.id(), pending.id(), outgoing.id()]));
}

// New: a set has no duplicates; a disposed current is not retained.
#[test]
fn collect_retained_ids_has_no_duplicates_and_skips_disposed_buffers() {
    let rig = Rig::new();
    let image = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&image)));
    rig.draw(&inputs(the_box()));
    rig.composer.begin_crossfade(ms(200)).unwrap();
    rig.composer.set(Some(Arc::clone(&image))); // outgoing == current

    let mut ids = BTreeSet::new();
    rig.composer.collect_retained_ids(&mut ids);
    assert_eq!(ids, BTreeSet::from([image.id()]));

    rig.composer.set(Some(disposed_buffer()));
    let mut again = BTreeSet::new();
    rig.composer.collect_retained_ids(&mut again);
    assert_eq!(again, BTreeSet::from([image.id()])); // only the outgoing one
}

// ---- filter by motion ---------------------------------------------------------------------------------------------------------------------

// C#: PresentationComposerTests.HeavyDownscale_WhileMoving_UsesLinear_ThenHighQualityOnceSettled
#[test]
fn heavy_downscale_while_moving_uses_linear_then_high_quality_once_settled() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(1000, 667))); // 0.67 MP into a ~100x67 box: x100 downscale
    let box_a = rect(10.0, 10.0, 100.0, 66.7);
    let box_b = rect(40.0, 10.0, 100.0, 66.7);

    let (rest, rest_rec) = rig.draw(&inputs(box_a)); // a fresh image is not motion
    let (moved, moved_rec) = rig.draw(&inputs(box_b)); // same image, new box: motion
    let (still_moving, _) = rig.draw(&inputs(box_b)); // same box but SettleMs has not passed

    assert!(!rest.degraded);
    assert_eq!(rest_rec.images()[0].2, ImageInterpolation::HighQualityCubic);
    assert!(moved.degraded);
    assert_eq!(moved.interpolation, ImageInterpolation::Linear);
    assert_eq!(moved_rec.images()[0].2, ImageInterpolation::Linear);
    assert!(still_moving.degraded);
    assert!(rig.composer.is_settle_armed());
    assert_eq!(rig.clock.subscribers(), 1);
    assert_eq!(rig.clock.requests.get(), 1); // arming asked for a frame

    let invalidations = rig.invalidations.get();
    rig.advance(50);
    rig.clock.tick(rig.now.get()); // under 120 ms: keep waiting
    assert_eq!(rig.invalidations.get(), invalidations);
    assert_eq!(rig.clock.requests.get(), 2); // and keep polling
    rig.advance(100);
    rig.clock.tick(rig.now.get()); // still long enough: ask for one HQ frame
    assert_eq!(rig.invalidations.get(), invalidations + 1);
    assert_eq!(rig.clock.requests.get(), 2);

    let (settled, settled_rec) = rig.draw(&inputs(box_b));
    assert!(!settled.degraded);
    assert_eq!(settled_rec.images()[0].2, ImageInterpolation::HighQualityCubic);
    assert!(!rig.composer.is_settle_armed());
    assert_eq!(rig.clock.subscribers(), 0);
}

// C#: PresentationComposerTests.SettleClock_AsksForTheHighQualityFrame_ExactlyWhenSettleMsHaveElapsed (119 ms no, 120 ms yes)
#[test]
fn settle_clock_asks_for_the_high_quality_frame_exactly_when_settle_ms_have_elapsed() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(1000, 667)));
    rig.draw(&inputs(rect(10.0, 10.0, 100.0, 66.7)));
    rig.draw(&inputs(rect(40.0, 10.0, 100.0, 66.7))); // motion at `now`
    assert!(rig.composer.is_settle_armed());
    let before = rig.invalidations.get();

    rig.advance(119);
    rig.clock.tick(rig.now.get()); // 1 ms left: still moving
    assert_eq!(rig.invalidations.get(), before);

    rig.advance(1); // exactly 120 ms = SettleMs: still
    rig.clock.tick(rig.now.get());
    assert_eq!(rig.invalidations.get(), before + 1);

    let (settled, rec) = rig.draw(&inputs(rect(40.0, 10.0, 100.0, 66.7)));
    assert!(!settled.degraded); // the same boundary as `draw`: no longer "in motion"
    assert_eq!(rec.images()[0].2, ImageInterpolation::HighQualityCubic);
}

#[test]
fn a_draw_one_tick_before_the_settle_boundary_is_still_degraded() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(1000, 667)));
    rig.draw(&inputs(rect(10.0, 10.0, 100.0, 66.7)));
    rig.draw(&inputs(rect(40.0, 10.0, 100.0, 66.7)));

    rig.advance(119);
    let (frame, _) = rig.draw(&inputs(rect(40.0, 10.0, 100.0, 66.7)));

    assert!(frame.degraded);
}

// C#: PresentationComposerTests.LightDownscaleWhileMoving_KeepsHighQuality_AndNeverArmsTheSettleClock
#[test]
fn light_downscale_while_moving_keeps_high_quality_and_never_arms_the_settle_clock() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(200, 133)));
    rig.draw(&inputs(rect(0.0, 0.0, 150.0, 100.0)));

    let (moved, rec) = rig.draw(&inputs(rect(30.0, 0.0, 150.0, 100.0)));

    assert!(!moved.degraded);
    assert_eq!(rec.images()[0].2, ImageInterpolation::HighQualityCubic);
    assert!(!rig.composer.is_settle_armed());
    assert_eq!(rig.clock.subscribers(), 0);
}

// C#: PresentationComposerTests.DpiScale_CountsPhysicalPixelsWhenJudgingTheDownscale
#[test]
fn dpi_scale_counts_physical_pixels_when_judging_the_downscale() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(400, 300)));
    rig.draw(&inputs(rect(0.0, 0.0, 100.0, 75.0)));

    // 100x75 DIP is x16 area at 100 %, but only x4 at 200 % (not heavy).
    let mut input = inputs(rect(10.0, 0.0, 100.0, 75.0));
    input.dpi_scale = 2.0;
    let (moved, rec) = rig.draw(&input);

    assert!(!moved.degraded);
    assert_eq!(rec.images()[0].2, ImageInterpolation::HighQualityCubic);
}

// New: `NaN > 0` is false, so a NaN, zero or negative DPI counts as 1.0 (x100 downscale: degraded).
#[test]
fn dpi_nan_zero_or_negative_counts_as_one() {
    for dpi in [f64::NAN, 0.0, -2.0] {
        let rig = Rig::new();
        rig.composer.set(Some(buffer(1000, 667)));
        rig.draw(&inputs(rect(10.0, 10.0, 100.0, 66.7)));
        let mut input = inputs(rect(40.0, 10.0, 100.0, 66.7));
        input.dpi_scale = dpi;

        let (moved, _) = rig.draw(&input);

        assert!(moved.degraded, "dpi {dpi}");
    }
}

// New: a new image at another box is not motion (the `moved` test needs the same buffer id).
#[test]
fn a_different_image_at_another_box_is_not_motion() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(1000, 667)));
    rig.draw(&inputs(rect(10.0, 10.0, 100.0, 66.7)));
    rig.composer.set(Some(buffer(1000, 667)));

    let (frame, _) = rig.draw(&inputs(rect(40.0, 10.0, 100.0, 66.7)));

    assert!(!frame.degraded);
    assert!(!rig.composer.is_settle_armed());
}

// New: the same box twice is not motion; NaN-aware rect equality (a NaN box equal to the stored NaN box is not a move).
#[test]
fn the_same_box_is_not_motion() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(1000, 667)));
    rig.draw(&inputs(rect(10.0, 10.0, 100.0, 66.7)));

    let (frame, _) = rig.draw(&inputs(rect(10.0, 10.0, 100.0, 66.7)));

    assert!(!frame.degraded);
}

// New (contract: `set_settle_armed(degraded)` also when nothing is drawn: unarms).
#[test]
fn settle_clock_is_unarmed_when_nothing_is_drawn() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(1000, 667)));
    rig.draw(&inputs(rect(10.0, 10.0, 100.0, 66.7)));
    rig.draw(&inputs(rect(40.0, 10.0, 100.0, 66.7)));
    assert!(rig.composer.is_settle_armed());
    assert_eq!(rig.clock.subscribers(), 1);

    rig.composer.set(None);
    let (frame, _) = rig.draw(&inputs(rect(40.0, 10.0, 100.0, 66.7)));

    assert!(!frame.main_drawn);
    assert!(!frame.degraded);
    assert!(!rig.composer.is_settle_armed());
    assert_eq!(rig.clock.subscribers(), 0);
}

// New: `settle_ms <= 0` gives zero settle ticks, so nothing is ever "in motion".
#[test]
fn settle_ticks_for_negative_or_zero_settle_ms_are_zero() {
    for settle_ms in [-5, 0] {
        let rig = Rig::with(settle_ms, 1_000);
        rig.composer.set(Some(buffer(1000, 667)));
        rig.draw(&inputs(rect(10.0, 10.0, 100.0, 66.7)));

        let (moved, _) = rig.draw(&inputs(rect(40.0, 10.0, 100.0, 66.7)));

        assert!(!moved.degraded, "settle_ms {settle_ms}");
        assert!(!rig.composer.is_settle_armed());
    }
}

// New: a failing draw pops its clips, skips the rest and leaves the settle state alone.
#[test]
fn draw_error_pops_the_clip_and_skips_the_rest() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(1000, 667)));
    rig.draw(&inputs(rect(10.0, 10.0, 100.0, 66.7)));
    rig.draw(&inputs(rect(40.0, 10.0, 100.0, 66.7)));
    assert!(rig.composer.is_settle_armed()); // armed by the previous (successful) draw
    let left = buffer(10, 10);
    let frame = CompareFrame {
        left: Some(&left),
        right: None,
        left_selected: false,
        right_selected: false,
    };
    rig.probe.fail_get.set(true);
    let mut rec = Rec::default();

    // A request that would NOT degrade (Linear requested) so that an updated settle state would show.
    let mut input = compare_inputs(frame, ScalingQuality::Linear);
    input.main_destination = rect(70.0, 10.0, 100.0, 66.7);
    let result = rig.composer.draw(&mut rec, &input);

    assert!(result.is_err());
    assert_eq!(rec.depth, 0);
    assert_eq!(rec.names(), ["push", "pop"]); // no crossfade, no compare panel
    assert!(rec.fills().is_empty());
    assert!(
        rig.composer.is_settle_armed(),
        "the settle state is not updated by a failed frame"
    );
}

#[test]
fn a_failing_compare_image_is_reported_after_the_main_image_was_drawn() {
    let rig = Rig::new();
    let image = buffer(100, 100);
    rig.composer.set(Some(Arc::clone(&image)));
    let left = buffer(10, 10);
    rig.probe.preload(image.id());
    let frame = CompareFrame {
        left: Some(&left),
        right: None,
        left_selected: false,
        right_selected: false,
    };
    let mut rec = Rec::default();
    // The probe fails every get_or_upload: the main image fails first.
    rig.probe.fail_get.set(true);

    let result = rig
        .composer
        .draw(&mut rec, &compare_inputs(frame, ScalingQuality::HighQuality));

    assert!(result.is_err());
    assert_eq!(rec.depth, 0);
}

// C#: PresentationComposerTests.Dispose_StopsListeningToTheClockAndTheCache
#[test]
fn dispose_stops_listening_to_the_clock_and_the_cache() {
    let rig = Rig::new();
    let full = buffer(1000, 667);
    rig.composer.set(Some(Arc::clone(&full)));
    rig.draw(&inputs(rect(0.0, 0.0, 100.0, 67.0)));
    rig.draw(&inputs(rect(10.0, 0.0, 100.0, 67.0)));
    assert_eq!(rig.clock.subscribers(), 1);
    assert_eq!(rig.probe.handler_count(), 1);

    rig.composer.dispose();

    assert_eq!(rig.clock.subscribers(), 0);
    assert_eq!(rig.probe.handler_count(), 0);
    assert!(!rig.composer.is_settle_armed());
    assert!(rig.composer.current().is_none());
    assert!(rig.composer.pending().is_none());
    assert_eq!(
        rig.composer.set_incoming(Some(buffer(10, 10)), false, 0),
        Err(ComposerError::Disposed)
    );
    assert_eq!(rig.composer.begin_crossfade(ms(100)), Err(ComposerError::Disposed));
    // draw still works and draws nothing; it does not re-arm the settle clock
    let (frame, rec) = rig.draw(&inputs(the_box()));
    assert!(!frame.main_drawn);
    assert!(rec.calls.is_empty());
    assert_eq!(rig.clock.subscribers(), 0);
}

// New
#[test]
fn dispose_twice_is_a_no_op() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(100, 100)));
    rig.draw(&inputs(the_box()));
    rig.composer.begin_crossfade(ms(100)).unwrap();

    rig.composer.dispose();
    rig.composer.dispose();

    assert_eq!(rig.clock.subscribers(), 0);
    assert_eq!(rig.probe.handler_count(), 0);
    assert!(!rig.composer.crossfade().is_active());
}

#[test]
fn dispose_drops_a_pending_upgrade_and_its_deadline_does_nothing() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(100, 100)));
    rig.composer.set_incoming(Some(buffer(100, 100)), false, 1).unwrap();
    rig.composer.dispose();
    rig.advance(5000);

    rig.fire(0);

    assert!(rig.composer.current().is_none());
    assert_eq!(rig.composer.promotion_count(), 0);
    assert_eq!(rig.invalidations.get(), 0);
}

#[test]
fn dropping_the_composer_disposes_it() {
    let rig = Rig::new();
    rig.composer.set(Some(buffer(1000, 667)));
    rig.draw(&inputs(rect(0.0, 0.0, 100.0, 67.0)));
    rig.draw(&inputs(rect(10.0, 0.0, 100.0, 67.0)));
    assert_eq!(rig.clock.subscribers(), 1);
    let Rig {
        probe, clock, composer, ..
    } = rig;

    drop(composer);

    assert_eq!(clock.subscribers(), 0);
    assert_eq!(probe.handler_count(), 0);
}

// New: handlers run on a snapshot, in subscription order; a handler may (un)subscribe while the event is raised.
#[test]
fn current_changed_handlers_may_resubscribe() {
    let rig = Rig::new();
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let ids: Rc<RefCell<BTreeMap<&'static str, u64>>> = Rc::new(RefCell::new(BTreeMap::new()));
    let weak: Weak<PresentationComposer> = Rc::downgrade(&rig.composer);

    let first = {
        let (log, ids, weak) = (Rc::clone(&log), Rc::clone(&ids), weak.clone());
        rig.composer.subscribe_current_changed(Rc::new(move || {
            log.borrow_mut().push("first");
            let composer = weak.upgrade().unwrap();
            composer.unsubscribe_current_changed(ids.borrow()["second"]); // removed during the raise: still called this time
            let third_log = Rc::clone(&log);
            let third = composer.subscribe_current_changed(Rc::new(move || third_log.borrow_mut().push("third")));
            ids.borrow_mut().insert("third", third);
        }))
    };
    let second = {
        let log = Rc::clone(&log);
        rig.composer
            .subscribe_current_changed(Rc::new(move || log.borrow_mut().push("second")))
    };
    ids.borrow_mut().insert("first", first);
    ids.borrow_mut().insert("second", second);
    rig.composer.unsubscribe_current_changed(9_999); // unknown id: no-op

    rig.composer.set(Some(buffer(100, 100)));
    rig.composer.set_incoming(Some(buffer(100, 100)), false, 1).unwrap();
    rig.probe.finish_all();
    assert_eq!(*log.borrow(), ["first", "second"]);

    rig.composer.set_incoming(Some(buffer(100, 100)), false, 1).unwrap();
    rig.probe.finish_all();
    assert_eq!(*log.borrow(), ["first", "second", "first", "third"]);
}

#[test]
fn default_options_use_the_qpc_clock_and_the_scaling_policy_settle() {
    let options = ComposerOptions::default();
    assert!(options.timestamp.is_none());
    assert_eq!(options.ticks_per_second, 0);
    assert_eq!(options.settle_ms, pr_app::viewing::scaling_policy::SETTLE_MS);
    assert!(format!("{options:?}").contains("settle_ms"));

    // With the default clock an upgrade that was just set is not due yet.
    let probe = fixture::Probe::new();
    let composer = PresentationComposer::new(
        Rc::clone(&probe) as Rc<dyn pr_app::services::render::gpu_image::PrefetchStatus>,
        Rc::new(viewing_fakes::FakeClock::default()),
        Rc::new(|| {}),
        Rc::new(|_, _| {}),
        options,
    );
    composer.set(Some(buffer(100, 100)));
    composer.set_incoming(Some(buffer(100, 100)), false, 1).unwrap();
    assert!(!composer.try_promote_pending());
    assert!(composer.pending().is_some());
}

// ---- PresentationStaleOrderTests ----------------------------------------------------------------------------------------------------------

/// One composer with the file / quality of every buffer it was given (C# `Rig`).
struct Order {
    rig: Rig,
    file: i32,
    quality: i32,
    origin: BTreeMap<i64, (i32, i32)>,
    keep: Vec<Arc<PixelBuffer>>,
    last_drawn_file: i32,
    best_quality_shown: BTreeMap<i32, i32>,
}

impl Order {
    fn new() -> Order {
        Order {
            rig: Rig::new(),
            file: -1,
            quality: 0,
            origin: BTreeMap::new(),
            keep: Vec::new(),
            last_drawn_file: -1,
            best_quality_shown: BTreeMap::new(),
        }
    }

    fn new_buffer(&mut self, file: i32, quality: i32) -> Arc<PixelBuffer> {
        let image = buffer(8, 8);
        self.origin.insert(image.id(), (file, quality));
        self.keep.push(Arc::clone(&image));
        image
    }

    fn apply(&mut self, op: usize) {
        match op {
            0 => {
                // the first bitmap of another file (a file change: replaces at once)
                self.file += 1;
                self.quality = 0;
                let image = self.new_buffer(self.file, 0);
                self.rig.composer.set_incoming(Some(image), true, 1).unwrap();
            }
            1 => {
                // a better bitmap of the file on screen (preview -> full)
                if self.file < 0 {
                    return;
                }
                self.quality += 1;
                let image = self.new_buffer(self.file, self.quality);
                self.rig.composer.set_incoming(Some(image), false, 1).unwrap();
            }
            2 => {
                self.rig.probe.finish_next(); // one background upload stripe
            }
            3 => {
                // the newest upgrade deadline fires, long after it was armed
                self.rig.advance(2000);
                let count = self.rig.scheduled.borrow().len();
                if count > 0 {
                    self.rig.fire(count - 1);
                }
            }
            4 => {
                // the OLDEST deadline fires late (it must not drag a newer state)
                self.rig.advance(2000);
                self.rig.fire(0);
            }
            5 => self.draw(),
            _ => {
                // a frame is drawn and the crossfade freezes it, as ImageChanging does before a file change
                self.draw();
                self.rig.composer.begin_crossfade(ms(100)).unwrap();
            }
        }
    }

    fn draw(&mut self) {
        let (frame, rec) = self.rig.draw(&inputs(the_box()));
        if !frame.main_drawn {
            return;
        }
        let main = rec.images()[0].0;
        let (file, quality) = self.origin[&main];
        assert!(
            file >= self.last_drawn_file,
            "frame drew a bitmap of file {file} after one of file {}",
            self.last_drawn_file
        );
        self.last_drawn_file = file;
        assert_eq!(
            self.file, file,
            "what is drawn as THE image is always the latest file's"
        );
        let best = self.best_quality_shown.get(&file).copied().unwrap_or(0);
        assert!(
            quality >= best,
            "frame drew quality {quality} of file {file} after quality {best} was already drawn"
        );
        self.best_quality_shown.insert(file, quality);
    }

    fn assert_invariants(&self, path: &str) {
        match self.rig.composer.current() {
            Some(current) => {
                let (file, quality) = self.origin[&current.id()];
                assert!(
                    file == self.file,
                    "{path}: Current belongs to file {file} but file {} was assigned last",
                    self.file
                );
                if let Some(pending) = self.rig.composer.pending() {
                    let (pending_file, pending_quality) = self.origin[&pending.id()];
                    assert!(
                        pending_file == file,
                        "{path}: Pending is of file {pending_file}, Current of file {file}"
                    );
                    assert!(
                        pending_quality > quality,
                        "{path}: Pending quality {pending_quality} is not newer than Current {quality}"
                    );
                }
                assert!(!current.is_disposed());
            }
            None => assert!(self.rig.composer.pending().is_none()),
        }
    }
}

// C#: PresentationStaleOrderTests.Composer_EverySequenceOfFiveEvents_NeverShowsAnOlderFileOrAWorseBitmap (7 event kinds ^ 5 positions)
#[test]
fn composer_every_sequence_of_five_events_never_shows_an_older_file_or_a_worse_bitmap() {
    const ALPHABET: usize = 7;
    const DEPTH: usize = 5;
    let total = ALPHABET.pow(DEPTH as u32);
    for n in 0..total {
        let mut code = n;
        let mut sequence = [0_usize; DEPTH];
        for slot in &mut sequence {
            *slot = code % ALPHABET;
            code /= ALPHABET;
        }
        let mut order = Order::new();
        let path = format!("{sequence:?}");
        for (step, op) in sequence.into_iter().enumerate() {
            order.apply(op);
            order.assert_invariants(&format!("{path} after step {step}"));
        }

        // Whatever is still queued (late stripes, late deadlines) finishes: the end state is the latest file at its best.
        order.rig.probe.finish_all();
        order.apply(3);
        order.rig.composer.try_promote_pending();
        order.assert_invariants(&format!("{path} settled"));
        assert!(order.rig.composer.pending().is_none(), "{path}");
    }
    assert_eq!(total, 16_807);
}

// C#: PresentationStaleOrderTests.Composer_OlderUpgradeUploadingFirst_IsNeverPromotedOverTheNewerOne
#[test]
fn composer_older_upgrade_uploading_first_is_never_promoted_over_the_newer_one() {
    let mut order = Order::new();
    order.apply(0);
    let before = order.rig.composer.current().unwrap();
    let first = order.new_buffer(0, 1);
    let second = order.new_buffer(0, 2);
    order
        .rig
        .composer
        .set_incoming(Some(Arc::clone(&first)), false, 1)
        .unwrap();
    order
        .rig
        .composer
        .set_incoming(Some(Arc::clone(&second)), false, 1)
        .unwrap();
    let shown: Rc<RefCell<Vec<Option<Arc<PixelBuffer>>>>> = Rc::new(RefCell::new(vec![order.rig.composer.current()]));
    let weak = Rc::downgrade(&order.rig.composer);
    let sink = Rc::clone(&shown);
    order.rig.composer.subscribe_current_changed(Rc::new(move || {
        sink.borrow_mut().push(weak.upgrade().unwrap().current())
    }));

    order.rig.probe.finish_all(); // the FIRST upgrade's stripes run (and complete) before the second's

    assert!(same(&order.rig.composer.current(), &second));
    assert!(!shown.borrow().iter().any(|image| same(image, &first)));
    assert_eq!(order.rig.composer.promotion_count(), 1);
    assert!(!same(&order.rig.composer.current(), &before));
}

// C#: PresentationStaleOrderTests.Composer_DeviceLostDuringPendingUpgrade_DeadlinePromotesAndDrawsTheUpgrade
#[test]
fn composer_device_lost_during_pending_upgrade_deadline_promotes_and_draws_the_upgrade() {
    let mut order = Order::new();
    order.apply(0);
    order.apply(5); // the first image is on screen
    let original = order.rig.composer.current().unwrap();
    order.apply(1); // upgrade, uploading in the background
    let upgrade = order.rig.composer.pending().unwrap();
    order.rig.probe.finish_next();

    order.rig.probe.device_lost();
    order.rig.probe.finish_all();

    assert!(same(&order.rig.composer.current(), &original));
    assert!(same(&order.rig.composer.pending(), &upgrade));
    assert_eq!(order.rig.open_deadlines(), 1);
    order.apply(3); // the deadline fires
    assert!(same(&order.rig.composer.current(), &upgrade));
    assert!(order.rig.composer.pending().is_none());
    let (frame, rec) = order.rig.draw(&inputs(the_box()));
    assert!(frame.main_drawn);
    assert_eq!(rec.images().len(), 1);
    assert_eq!(rec.images()[0].0, upgrade.id());
    assert_eq!(order.rig.composer.promotion_count(), 1);
}
