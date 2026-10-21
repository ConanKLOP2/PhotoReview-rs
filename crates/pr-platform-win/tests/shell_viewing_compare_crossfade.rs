#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::viewmodels::compare::CompareViewModel;
use pr_app::viewmodels::format::context::{DateText, FormatContext};
use pr_app::viewport::render_geometry::{ImageInterpolation, dark_canvas};
use pr_core::time::DateTimeUtc;
use pr_imaging::decode_types::{DecodedImage, DecodedImageOptions, PlatformImage};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_l10n::localizer::Localizer;
use pr_l10n::template::NumberCulture;
use pr_platform_win::shell_viewing::compare::{self, CompareFrame, CompareSide, layout};
use pr_platform_win::shell_viewing::crossfade::{CrossfadeError, CrossfadeLayer};
use viewing_fakes::{FakeCache, FakeClock, RecordingContext};

fn client() -> SizeD {
    SizeD {
        width: 1200.0,
        height: 800.0,
    }
}
fn frame() -> RectD {
    RectD {
        x: 1.0,
        y: 2.0,
        width: 60.0,
        height: 50.0,
    }
}
fn pixels() -> Arc<PixelBuffer> {
    Arc::new(PixelBuffer::allocate(8, 8, PixelLayout::Pbgra32).unwrap())
}
fn layer(clock: Rc<FakeClock>, changes: Rc<Cell<usize>>) -> CrossfadeLayer {
    CrossfadeLayer::new(clock, Rc::new(move || changes.set(changes.get() + 1)), 1_000)
}

#[derive(Debug)]
struct Dates;
impl DateText for Dates {
    fn general(&self, _: DateTimeUtc) -> Option<String> {
        Some(String::new())
    }
    fn invariant_general(&self, _: DateTimeUtc) -> String {
        String::new()
    }
}

fn decoded(image: PlatformImage) -> Arc<DecodedImage> {
    Arc::new(DecodedImage::new(image, 8, 8, 256, DecodedImageOptions::default()).unwrap())
}

// C#: CompareLayoutTests.Panel_IsTheClientInsetBy8OnEverySide
#[test]
fn compare_layout_panel_is_the_client_inset_by_8_on_every_side() {
    assert_eq!(
        layout::panel(client()),
        RectD {
            x: 8.0,
            y: 8.0,
            width: 1184.0,
            height: 784.0
        }
    );
}

// C#: CompareLayoutTests.Borders_AreEqualColumnsInsetBy4
#[test]
fn compare_layout_borders_are_equal_columns_inset_by_4() {
    assert_eq!(
        layout::border(client(), CompareSide::Left),
        RectD {
            x: 12.0,
            y: 12.0,
            width: 584.0,
            height: 776.0
        }
    );
    assert_eq!(
        layout::border(client(), CompareSide::Right),
        RectD {
            x: 604.0,
            y: 12.0,
            width: 584.0,
            height: 776.0
        }
    );
}

// C#: CompareLayoutTests.Content_IsInsideTheTwoDipBorder
#[test]
fn compare_layout_content_is_inside_the_two_dip_border() {
    assert_eq!(
        layout::content(client(), CompareSide::Left),
        RectD {
            x: 14.0,
            y: 14.0,
            width: 580.0,
            height: 772.0
        }
    );
    assert_eq!(
        layout::content(client(), CompareSide::Right),
        RectD {
            x: 606.0,
            y: 14.0,
            width: 580.0,
            height: 772.0
        }
    );
}

// C#: CompareLayoutTests.Image_Landscape_IsUniformFitToWidthAndCenteredVertically
#[test]
fn compare_layout_image_landscape_is_uniform_fit_to_width_and_centered_vertically() {
    let image = layout::image(client(), CompareSide::Left, 4000, 3000);
    assert_eq!(image.x, 14.0);
    assert!((image.y - 182.5).abs() < 1e-10);
    assert_eq!(image.width, 580.0);
    assert!((image.height - 435.0).abs() < 1e-10);
}

// C#: CompareLayoutTests.Image_Portrait_IsUniformFitToHeightAndCenteredHorizontally
#[test]
fn compare_layout_image_portrait_is_uniform_fit_to_height_and_centered_horizontally() {
    assert_eq!(
        layout::image(client(), CompareSide::Right, 3000, 4000),
        RectD {
            x: 606.5,
            y: 14.0,
            width: 579.0,
            height: 772.0
        }
    );
}

// C#: CompareLayoutTests.Image_SmallerThanTheCell_IsEnlargedToFit
#[test]
fn compare_layout_image_smaller_than_the_cell_is_enlarged_to_fit() {
    assert_eq!(
        layout::image(client(), CompareSide::Left, 58, 77),
        RectD {
            x: 14.0,
            y: 15.0,
            width: 580.0,
            height: 770.0
        }
    );
}

// C#: CompareLayoutTests.Image_EmptyBitmapOrEmptyCell_IsEmpty
#[test]
fn compare_layout_image_empty_bitmap_or_empty_cell_is_empty() {
    assert_eq!(layout::image(client(), CompareSide::Left, 0, 20).width, 0.0);
    assert_eq!(layout::image(SizeD::default(), CompareSide::Left, 20, 20).height, 0.0);
}

// C#: CompareLayoutTests.HitTest_BordersCountOnTheGapAndMarginDoNot
#[test]
fn compare_layout_hit_test_borders_count_on_the_gap_and_margin_do_not() {
    for (x, y, expected) in [
        (12.0, 12.0, Some(CompareSide::Left)),
        (300.0, 400.0, Some(CompareSide::Left)),
        (700.0, 400.0, Some(CompareSide::Right)),
        (1187.0, 787.0, Some(CompareSide::Right)),
        (1188.0, 400.0, Some(CompareSide::Right)),
        (600.0, 400.0, None),
        (5.0, 5.0, None),
        (1195.0, 400.0, None),
    ] {
        assert_eq!(layout::hit_test(client(), PointD { x, y }), expected);
    }
}

// C#: CompareLayoutTests.HitTest_TinyClient_NeverHits
#[test]
fn compare_layout_hit_test_tiny_client_never_hits() {
    assert_eq!(
        layout::hit_test(
            SizeD {
                width: 1.0,
                height: 1.0
            },
            PointD { x: 12.0, y: 12.0 }
        ),
        None
    );
    assert_eq!(
        layout::hit_test(
            SizeD {
                width: f64::NAN,
                height: 100.0
            },
            PointD::default()
        ),
        None
    );
}

#[test]
fn compare_colors_match_the_xaml() {
    assert_eq!(compare::canvas(), dark_canvas());
    assert_eq!(compare::selected_color().g, 0xCD as f32 / 255.0);
    assert_eq!(compare::unselected_color().r, 0x55 as f32 / 255.0);
}

#[test]
fn from_view_model_borrows_pixels_and_selection_flags() {
    let texts = Localizer::current();
    let numbers = NumberCulture::invariant();
    let ctx = FormatContext {
        texts: &texts,
        numbers: &numbers,
        group_separator: ",",
        dates: &Dates,
    };
    let mut model = CompareViewModel::new(&ctx);
    let left = pixels();
    let right = pixels();
    model.set_left_path(Some("left".into()));
    model.set_right_path(Some("right".into()));
    model.select_left();
    model.set_left_image(Some(decoded(PlatformImage::Pixels(left.clone()))));
    model.set_right_image(Some(decoded(PlatformImage::Pixels(right.clone()))));
    let frame = CompareFrame::from_view_model(&model);
    assert_eq!(frame.left.map(PixelBuffer::id), Some(left.id()));
    assert_eq!(frame.right.map(PixelBuffer::id), Some(right.id()));
    assert!(frame.left_selected);
    assert!(!frame.right_selected);
    model.set_right_image(Some(decoded(PlatformImage::Opaque(Arc::new(42_i32)))));
    assert!(CompareFrame::from_view_model(&model).right.is_none());
}

#[test]
fn draw_empty_panel_draws_nothing() {
    let mut context = RecordingContext::default();
    assert_eq!(
        compare::draw(
            &FakeCache::default(),
            &mut context,
            SizeD::default(),
            &CompareFrame::empty(),
            ImageInterpolation::Linear
        )
        .unwrap(),
        0
    );
    assert!(context.calls.is_empty());
}

#[test]
fn draw_calls_in_order_and_returns_the_count_of_images() {
    let image = pixels();
    let mut context = RecordingContext::default();
    let frame = CompareFrame {
        left: Some(&image),
        right: Some(&image),
        left_selected: true,
        right_selected: false,
    };
    assert_eq!(
        compare::draw(
            &FakeCache::default(),
            &mut context,
            client(),
            &frame,
            ImageInterpolation::Linear
        )
        .unwrap(),
        2
    );
    assert_eq!(context.depth, 0);
    assert_eq!(context.calls.iter().filter(|call| call.starts_with("image")).count(), 2);
    assert_eq!(context.calls.iter().filter(|call| *call == "push").count(), 2);
    assert_eq!(
        context.calls.first().unwrap(),
        &format!("fill:{:?}", layout::panel(client()))
    );
}

#[test]
fn draw_missing_or_disposed_buffer_keeps_the_border_only() {
    let mut context = RecordingContext::default();
    assert_eq!(
        compare::draw(
            &FakeCache::default(),
            &mut context,
            client(),
            &CompareFrame::empty(),
            ImageInterpolation::Linear
        )
        .unwrap(),
        0
    );
    assert_eq!(context.calls.len(), 9);
}

#[test]
fn draw_disposed_buffer_keeps_the_border_only() {
    let mut disposed = PixelBuffer::allocate(8, 8, PixelLayout::Pbgra32).unwrap();
    disposed.dispose();
    let frame = CompareFrame {
        left: Some(&disposed),
        ..CompareFrame::empty()
    };
    let mut context = RecordingContext::default();
    assert_eq!(
        compare::draw(
            &FakeCache::default(),
            &mut context,
            client(),
            &frame,
            ImageInterpolation::Linear
        )
        .unwrap(),
        0
    );
    assert_eq!(context.calls.len(), 9);
}

#[test]
fn draw_nan_panel_draws_nothing() {
    let mut context = RecordingContext::default();
    assert_eq!(
        compare::draw(
            &FakeCache::default(),
            &mut context,
            SizeD {
                width: f64::NAN,
                height: 800.0
            },
            &CompareFrame::empty(),
            ImageInterpolation::Linear
        )
        .unwrap(),
        0
    );
    assert!(context.calls.is_empty());
}

#[test]
fn border_thickness_is_clamped_for_tiny_cells() {
    let client = SizeD {
        width: 35.0,
        height: 27.0,
    };
    let border = layout::border(client, CompareSide::Left);
    assert_eq!(border.width, 1.5);
    let mut context = RecordingContext::default();
    compare::draw(
        &FakeCache::default(),
        &mut context,
        client,
        &CompareFrame::empty(),
        ImageInterpolation::Linear,
    )
    .unwrap();
    assert!(context.calls.contains(&format!(
        "fill:{:?}",
        RectD {
            x: border.x,
            y: border.y,
            width: border.width,
            height: 0.75
        }
    )));
}

#[test]
fn draw_pops_the_clip_when_the_cache_fails() {
    let image = pixels();
    let mut context = RecordingContext::default();
    let frame = CompareFrame {
        left: Some(&image),
        ..CompareFrame::empty()
    };
    assert!(
        compare::draw(
            &FakeCache { fail: true },
            &mut context,
            client(),
            &frame,
            ImageInterpolation::Linear
        )
        .is_err()
    );
    assert_eq!(context.depth, 0);
    assert_eq!(context.calls.last().unwrap(), "pop");
}

// C#: CrossfadeLayerTests.Opacity_FollowsQuadraticOutOverTheDuration
#[test]
fn opacity_follows_quadratic_out_over_the_duration() {
    let clock = Rc::new(FakeClock::default());
    let changes = Rc::new(Cell::new(0));
    let layer = layer(clock.clone(), changes.clone());
    assert_eq!(
        layer.start(
            pixels(),
            frame(),
            ImageInterpolation::Linear,
            Duration::from_millis(200)
        ),
        Ok(true)
    );
    for (time, expected) in [(0, 1.0), (50, 0.5625), (100, 0.25), (150, 0.0625)] {
        clock.tick(time);
        assert!((layer.opacity() - expected).abs() < 0.0001);
    }
    clock.tick(200);
    assert!(!layer.is_active());
    assert_eq!(clock.subscribers(), 0);
    assert_eq!(changes.get(), 4);
}

// C#: CrossfadeLayerTests.Start_WithNothingToFade_ReturnsFalseAndSubscribesNothing
#[test]
fn start_guards_table() {
    let clock = Rc::new(FakeClock::default());
    let layer = layer(clock.clone(), Rc::new(Cell::new(0)));
    for (rect, duration) in [
        (frame(), Duration::ZERO),
        (frame(), Duration::from_nanos(50)),
        (RectD { width: 0.0, ..frame() }, Duration::from_millis(1)),
        (
            RectD {
                height: f64::NAN,
                ..frame()
            },
            Duration::from_millis(1),
        ),
        (
            RectD {
                x: f64::INFINITY,
                ..frame()
            },
            Duration::from_millis(1),
        ),
    ] {
        assert_eq!(
            layer.start(pixels(), rect, ImageInterpolation::Linear, duration),
            Ok(false)
        );
    }
    assert_eq!(layer.start_count(), 0);
    assert_eq!(clock.subscribers(), 0);
}

// C#: CrossfadeLayerTests.NewNavigationMidFade_RestartsFromOpacityOneWithTheNewOutgoing_NotQueued
#[test]
fn restart_mid_fade_cancels_the_old_tween() {
    let clock = Rc::new(FakeClock::default());
    let layer = layer(clock.clone(), Rc::new(Cell::new(0)));
    let first = pixels();
    let second = pixels();
    layer
        .start(first, frame(), ImageInterpolation::Linear, Duration::from_millis(200))
        .unwrap();
    clock.tick(0);
    clock.tick(100);
    layer
        .start(
            second.clone(),
            frame(),
            ImageInterpolation::Linear,
            Duration::from_millis(200),
        )
        .unwrap();
    assert_eq!(layer.opacity(), 1.0);
    assert_eq!(layer.outgoing_buffer_id(), Some(second.id()));
    clock.tick(100);
    clock.tick(150);
    assert!((layer.opacity() - 0.5625).abs() < 0.0001);
    assert_eq!(clock.subscribers(), 1);
}

// C#: CrossfadeLayerTests.Cancel_ReleasesTheOutgoingImageAndTheClock
#[test]
fn cancel_releases_the_outgoing_image_and_the_clock() {
    let clock = Rc::new(FakeClock::default());
    let layer = layer(clock.clone(), Rc::new(Cell::new(0)));
    layer
        .start(
            pixels(),
            frame(),
            ImageInterpolation::Linear,
            Duration::from_millis(200),
        )
        .unwrap();
    layer.cancel();
    assert!(!layer.is_active());
    assert_eq!(layer.opacity(), 0.0);
    assert_eq!(clock.subscribers(), 0);
}

// C#: CrossfadeLayerTests.Dispose_ReleasesEverythingAndRejectsNewFades
#[test]
fn dispose_releases_everything_and_rejects_new_fades() {
    let clock = Rc::new(FakeClock::default());
    let layer = layer(clock.clone(), Rc::new(Cell::new(0)));
    layer
        .start(
            pixels(),
            frame(),
            ImageInterpolation::Linear,
            Duration::from_millis(200),
        )
        .unwrap();
    layer.dispose();
    layer.dispose();
    assert_eq!(
        layer.start(
            pixels(),
            frame(),
            ImageInterpolation::Linear,
            Duration::from_millis(200)
        ),
        Err(CrossfadeError::Disposed)
    );
    assert_eq!(clock.subscribers(), 0);
}

#[test]
fn layer_is_not_kept_alive_by_the_clock() {
    let clock = Rc::new(FakeClock::default());
    {
        let layer = layer(clock.clone(), Rc::new(Cell::new(0)));
        layer
            .start(
                pixels(),
                frame(),
                ImageInterpolation::Linear,
                Duration::from_millis(200),
            )
            .unwrap();
    }
    assert_eq!(clock.subscribers(), 0);
    clock.tick(300);
}

// C#: CrossfadeLayerTests.Start_FreezesTheOutgoingFrame_AtOpacityOne_UnderAViewportClip
#[test]
fn start_freezes_the_outgoing_frame_at_opacity_one_under_a_viewport_clip() {
    let clock = Rc::new(FakeClock::default());
    let layer = layer(clock, Rc::new(Cell::new(0)));
    let image = pixels();
    layer
        .start(
            image.clone(),
            frame(),
            ImageInterpolation::Linear,
            Duration::from_millis(200),
        )
        .unwrap();
    assert_eq!(layer.frame(), frame());
    assert_eq!(layer.opacity(), 1.0);
    assert!(Arc::ptr_eq(&layer.outgoing().unwrap(), &image));
    let mut context = RecordingContext::default();
    assert!(layer.draw(&FakeCache::default(), &mut context, frame()).unwrap());
    assert_eq!(context.depth, 0);
    assert_eq!(context.calls, ["push", "image:1", "pop"]);
}

// C#: CrossfadeLayerTests.OutgoingBufferId_IsReportedWhileFading_SoTheCacheKeepsItsBitmap
#[test]
fn outgoing_buffer_id_is_reported_while_fading_so_the_cache_keeps_its_bitmap() {
    let clock = Rc::new(FakeClock::default());
    let layer = layer(clock, Rc::new(Cell::new(0)));
    let image = pixels();
    layer
        .start(
            image.clone(),
            frame(),
            ImageInterpolation::Linear,
            Duration::from_millis(1),
        )
        .unwrap();
    assert_eq!(layer.outgoing_buffer_id(), Some(image.id()));
    layer.cancel();
    assert_eq!(layer.outgoing_buffer_id(), None);
}

// C#: CrossfadeLayerTests.EveryChangedFrame_AsksTheHostToRedraw
#[test]
fn every_changed_frame_asks_the_host_to_redraw() {
    let clock = Rc::new(FakeClock::default());
    let changes = Rc::new(Cell::new(0));
    let layer = layer(clock.clone(), changes.clone());
    layer
        .start(
            pixels(),
            frame(),
            ImageInterpolation::Linear,
            Duration::from_millis(200),
        )
        .unwrap();
    assert_eq!(clock.requests.get(), 1);
    clock.tick(0);
    assert_eq!(changes.get(), 0);
    clock.tick(50);
    assert_eq!(changes.get(), 1);
    clock.tick(50);
    assert_eq!(changes.get(), 1);
    clock.tick(200);
    assert_eq!(changes.get(), 2);
}

#[test]
fn crossfade_draw_pops_clip_on_cache_error() {
    let clock = Rc::new(FakeClock::default());
    let layer = layer(clock, Rc::new(Cell::new(0)));
    layer
        .start(
            pixels(),
            frame(),
            ImageInterpolation::Linear,
            Duration::from_millis(200),
        )
        .unwrap();
    let mut context = RecordingContext::default();
    assert!(layer.draw(&FakeCache { fail: true }, &mut context, frame()).is_err());
    assert_eq!(context.depth, 0);
    assert!(layer.is_active());
}

#[test]
fn invalidate_runs_without_a_borrow() {
    let clock = Rc::new(FakeClock::default());
    let target: Rc<std::cell::RefCell<Option<Rc<CrossfadeLayer>>>> = Rc::new(std::cell::RefCell::new(None));
    let target_for_callback = target.clone();
    let layer = Rc::new(CrossfadeLayer::new(
        clock.clone(),
        Rc::new(move || {
            if let Some(layer) = target_for_callback.borrow().as_ref() {
                assert!(layer.outgoing().is_some());
                layer.cancel();
            }
        }),
        1_000,
    ));
    *target.borrow_mut() = Some(layer.clone());
    layer
        .start(
            pixels(),
            frame(),
            ImageInterpolation::Linear,
            Duration::from_millis(200),
        )
        .unwrap();
    clock.tick(0);
    clock.tick(50);
    assert!(!layer.is_active());
    assert_eq!(clock.subscribers(), 0);
    target.borrow_mut().take();
}
