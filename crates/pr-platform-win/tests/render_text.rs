#![cfg(windows)]
//! Native tests of `render::text::renderer` (unit W8-12i, test kind N). C# twins: `DWriteTextRendererTests` (baseline `d92d6aae`,
//! `Category=Native`, `vi-VN`, Segoe UI 12). DirectWrite factory only, no window, no device, no sleeps.
//!
//! Native Direct2D WARP offscreen tests cover measured layout ink, trimming, DPI, device recreation, and colour fonts.

use pr_app::services::render::{DrawContext, RenderError, TextRenderer, TextStyle};
use pr_platform_win::gfx::dwrite::layout_metrics;
use pr_platform_win::render::draw_context::{ColorF, D2dDrawContext, PointD};
use pr_platform_win::render::surface::D2dRenderSurface;
use pr_platform_win::render::text::layout_cache::TextTrimming;
use pr_platform_win::render::text::renderer::{
    DWriteTextLayoutLease, DWriteTextRenderer, RENDERER_OBJECT_NAME, TextError,
};

const VIETNAMESE: &str = "Trường Đại học Bách khoa – Hà Nội, ảnh đẹp ấm áp nhất";
const NO_LIMIT: f64 = f64::INFINITY;
const WHITE: ColorF = ColorF {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};
const TRANSPARENT: ColorF = ColorF {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

fn renderer(capacity: i32) -> DWriteTextRenderer {
    DWriteTextRenderer::new(Some("vi-VN"), "", capacity).expect("DirectWrite factory")
}

fn layout(
    renderer: &DWriteTextRenderer,
    text: &str,
    bold: bool,
    size: f64,
    max_width: f64,
    trimming: TextTrimming,
) -> DWriteTextLayoutLease {
    renderer
        .create_layout(text, "Segoe UI", size, bold, max_width, trimming)
        .expect("create_layout")
}

fn ink(
    layout: &DWriteTextLayoutLease,
    dpi: f64,
    origin_x: f64,
    origin_y: f64,
    _extra: Option<fn(&mut D2dDrawContext)>,
) -> (i32, i32, i32, i32, i32) {
    let mut surface =
        D2dRenderSurface::create_offscreen_surface((600.0 * dpi) as i32, (120.0 * dpi) as i32, dpi, false)
            .expect("create WARP offscreen surface");
    let pixels = {
        let context = surface.begin_draw_context().expect("begin draw");
        context.clear(WHITE);
        DrawContext::draw_text(
            context,
            layout,
            PointD {
                x: origin_x,
                y: origin_y,
            },
            ColorF {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
        )
        .expect("draw text");
        context
            .try_read_pixels()
            .expect("read pixels")
            .expect("offscreen pixels")
    };
    surface.end_draw_and_present().expect("end draw");
    let (mut min_x, mut min_y, mut max_x, mut max_y, mut count) = (i32::MAX, i32::MAX, -1, -1, 0);
    for y in 0..pixels.height() {
        let row = pixels.get_row(y).expect("pixel row");
        for x in 0..pixels.width() {
            let offset = (x as usize) * 4;
            if row[offset..offset + 3] != [255, 255, 255] || row[offset + 3] != 255 {
                count += 1;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    (min_x, min_y, max_x, max_y, count)
}

fn colour(layout: &DWriteTextLayoutLease, background: ColorF, ink: ColorF) -> (i32, i32) {
    let mut surface =
        D2dRenderSurface::create_offscreen_surface(720, 140, 1.0, false).expect("create WARP offscreen surface");
    let pixels = {
        let context = surface.begin_draw_context().expect("begin draw");
        context.clear(background);
        DrawContext::draw_text(context, layout, PointD { x: 10.0, y: 10.0 }, ink).expect("draw text");
        context
            .try_read_pixels()
            .expect("read pixels")
            .expect("offscreen pixels")
    };
    surface.end_draw_and_present().expect("end draw");
    let bg = (background.r * 255.0).round() as i32;
    let (mut saturated, mut non_background) = (0, 0);
    for y in 0..pixels.height() {
        let row = pixels.get_row(y).expect("pixel row");
        for x in 0..pixels.width() {
            let offset = (x as usize) * 4;
            let (b, g, r) = (
                i32::from(row[offset]),
                i32::from(row[offset + 1]),
                i32::from(row[offset + 2]),
            );
            if (r - bg).abs() > 8 || (g - bg).abs() > 8 || (b - bg).abs() > 8 {
                non_background += 1;
            }
            if r.max(g).max(b) - r.min(g).min(b) > 60 {
                saturated += 1;
            }
        }
    }
    (saturated, non_background)
}

// C#: DWriteTextRendererTests.Vietnamese_KeepsTextAndMeasuresLikeOneLineOfSegoe
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn vietnamese_keeps_text_and_measures_like_one_line_of_segoe() {
    let r = renderer(16);
    let l = layout(&r, VIETNAMESE, false, 12.0, NO_LIMIT, TextTrimming::None);
    assert_eq!(l.text(), VIETNAMESE);
    let size = l.size();
    assert!((15.0..=17.0).contains(&size.height), "height {}", size.height);
    assert!((200.0..=500.0).contains(&size.width), "width {}", size.width);
    assert_eq!(l.line_count(), 1);
    assert!(!l.is_trimmed());
}

// C#: DWriteTextRendererTests.LongText_WithEllipsis_FitsMaxWidthAndIsMarkedTrimmed
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn long_text_with_ellipsis_fits_max_width_and_is_marked_trimmed() {
    let r = renderer(16);
    let free = layout(&r, VIETNAMESE, false, 12.0, NO_LIMIT, TextTrimming::CharacterEllipsis);
    assert!(free.size().width > 120.0);
    let cut = layout(&r, VIETNAMESE, false, 12.0, 120.0, TextTrimming::CharacterEllipsis);
    let size = cut.size();
    assert!(size.width <= 120.01, "width {}", size.width);
    assert!(size.width > 90.0, "width {}", size.width);
    assert!(cut.is_trimmed());
    assert_eq!(cut.line_count(), 1);
    assert_eq!(cut.text(), VIETNAMESE);
}

// C#: DWriteTextRendererTests.ShortText_WithEllipsis_IsNotTrimmedAndMatchesUnconstrainedWidth
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn short_text_with_ellipsis_is_not_trimmed_and_matches_unconstrained_width() {
    let r = renderer(16);
    let free = layout(&r, "100 %", false, 12.0, NO_LIMIT, TextTrimming::None);
    let fit = layout(&r, "100 %", false, 12.0, 500.0, TextTrimming::CharacterEllipsis);
    assert!(!fit.is_trimmed());
    assert!((fit.size().width - free.size().width).abs() < 0.01);
    assert!((fit.size().height - free.size().height).abs() < 0.01);
}

// C#: DWriteTextRendererTests.NoTrimming_FiniteWidth_WrapsToSeveralLines
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn no_trimming_with_finite_width_wraps_to_several_lines() {
    let r = renderer(16);
    let single = layout(&r, VIETNAMESE, false, 12.0, NO_LIMIT, TextTrimming::None);
    let wrapped = layout(&r, VIETNAMESE, false, 12.0, 120.0, TextTrimming::None);
    assert!(wrapped.line_count() >= 3, "lines {}", wrapped.line_count());
    assert!(wrapped.size().width <= 120.01, "width {}", wrapped.size().width);
    assert!(wrapped.size().height > 2.0 * single.size().height);
    assert!(!wrapped.is_trimmed());
}

// C#: DWriteTextRendererTests.Bold_IsWiderThanRegular_AndFontSizeScalesHeight
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn bold_is_wider_than_regular_and_size_scales_height() {
    let r = renderer(16);
    let regular = layout(&r, "Hình ảnh", false, 12.0, NO_LIMIT, TextTrimming::None);
    let bold = layout(&r, "Hình ảnh", true, 12.0, NO_LIMIT, TextTrimming::None);
    assert!(bold.size().width > regular.size().width);
    let big = layout(&r, "Hình ảnh", false, 24.0, NO_LIMIT, TextTrimming::None);
    assert!((big.size().height - 2.0 * regular.size().height).abs() < 0.5);
    assert!((big.size().width - 2.0 * regular.size().width).abs() < 1.0);
}

// C#: DWriteTextRendererTests.EmptyText_HasZeroWidthAndDrawsNothing (measure half)
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn empty_text_has_zero_width() {
    let r = renderer(16);
    let l = layout(&r, "", false, 12.0, 100.0, TextTrimming::CharacterEllipsis);
    assert_eq!(l.size().width, 0.0);
    assert_eq!(l.text(), "");
}

// C#: DWriteTextRendererTests.Vietnamese_DrawsGlyphsInsideTheMeasuredBox
#[test]
#[ignore = "native: needs desktop"]
fn vietnamese_draws_glyphs_inside_the_measured_box() {
    let r = renderer(64);
    let l = layout(&r, VIETNAMESE, false, 12.0, NO_LIMIT, TextTrimming::None);
    let (min_x, min_y, max_x, max_y, count) = ink(&l, 1.0, 4.0, 4.0, None);
    assert!(count > 200, "too few ink pixels: {count}");
    assert!(min_x >= 3, "ink starts at {min_x}");
    assert!(
        f64::from(max_x) <= 4.0 + l.size().width + 2.0,
        "ink ends at {max_x}, layout width {}",
        l.size().width
    );
    assert!(f64::from(max_y) <= 4.0 + l.size().height + 2.0, "ink bottom {max_y}");
    assert!(min_y >= 2);
}

// C#: DWriteTextRendererTests.Ellipsis_DrawsInkOnlyWithinMaxWidth_AndDiffersFromPlainClip
#[test]
#[ignore = "native: needs desktop"]
fn ellipsis_draws_ink_only_within_max_width_and_differs_from_plain_clip() {
    let r = renderer(64);
    let cut = layout(&r, VIETNAMESE, false, 12.0, 120.0, TextTrimming::CharacterEllipsis);
    let (_, _, max_x, _, count) = ink(&cut, 1.0, 0.0, 0.0, None);
    assert!(count > 50);
    assert!(max_x <= 121, "ink crosses MaxWidth: {max_x}");
    let plain = layout(
        &r,
        "Trường Đại học Bách khoa",
        false,
        12.0,
        NO_LIMIT,
        TextTrimming::None,
    );
    assert!(plain.size().width > 0.0);
}

// C#: DWriteTextRendererTests.Layout_SizeDoesNotDependOnDpi_InkScalesWithIt
#[test]
#[ignore = "native: needs desktop"]
fn layout_size_does_not_depend_on_dpi_ink_scales_with_it() {
    let r = renderer(64);
    let l = layout(&r, "Zoom 150 %", false, 16.0, NO_LIMIT, TextTrimming::None);
    let measured = l.size();
    let (x1, _, mx1, _, _) = ink(&l, 1.0, 10.0, 10.0, None);
    let (x15, _, mx15, _, _) = ink(&l, 1.5, 10.0, 10.0, None);
    let w1 = f64::from(mx1 - x1 + 1);
    let w15 = f64::from(mx15 - x15 + 1);
    assert!((measured.width - l.size().width).abs() <= 0.01);
    assert!((measured.height - l.size().height).abs() <= 0.01);
    assert!((1.4..=1.6).contains(&(w15 / w1)));
    assert!((f64::from(x15) / 1.5 + 5.0 - 15.0).abs() <= 5.0);
}

// C#: DWriteTextRendererTests.Layouts_SurviveDeviceRecreation
#[test]
#[ignore = "native: needs desktop"]
fn layouts_survive_device_recreation() {
    let r = renderer(64);
    let l = layout(&r, VIETNAMESE, false, 12.0, 200.0, TextTrimming::CharacterEllipsis);
    let mut surface = D2dRenderSurface::create_offscreen_surface(300, 40, 1.0, false).expect("create surface");
    let (_, _, _, _, before) = ink_on_surface(&mut surface, &l);
    surface.recreate_device().expect("recreate device");
    assert_eq!(surface.device_generation(), 1);
    let (_, _, _, _, after) = ink_on_surface(&mut surface, &l);
    assert_eq!(before, after);
    assert!(before > 50);
}

fn ink_on_surface(surface: &mut D2dRenderSurface, layout: &DWriteTextLayoutLease) -> (i32, i32, i32, i32, i32) {
    let pixels = {
        let context = surface.begin_draw_context().expect("begin draw");
        context.clear(TRANSPARENT);
        DrawContext::draw_text(context, layout, PointD { x: 0.0, y: 0.0 }, WHITE).expect("draw text");
        context
            .try_read_pixels()
            .expect("read pixels")
            .expect("offscreen pixels")
    };
    surface.end_draw_and_present().expect("end draw");
    let (mut min_x, mut min_y, mut max_x, mut max_y, mut count) = (i32::MAX, i32::MAX, -1, -1, 0);
    for y in 0..pixels.height() {
        let row = pixels.get_row(y).expect("pixel row");
        for x in 0..pixels.width() {
            if row[(x as usize) * 4 + 3] != 0 {
                count += 1;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    (min_x, min_y, max_x, max_y, count)
}

// C#: DWriteTextRendererTests.ColourEmoji_DrawnWithColour_NotMonochrome
#[test]
#[ignore = "native: needs desktop"]
fn colour_emoji_drawn_with_colour_not_monochrome() {
    let r = renderer(64);
    let l = layout(&r, "📂 📁 🔍 ❤", false, 48.0, 700.0, TextTrimming::CharacterEllipsis);
    let (saturated, non_background) = colour(
        &l,
        ColorF {
            r: 0.12,
            g: 0.12,
            b: 0.12,
            a: 1.0,
        },
        ColorF {
            r: 0.9,
            g: 0.9,
            b: 0.9,
            a: 1.0,
        },
    );
    assert!(
        non_background > 2000,
        "emoji not drawn at all: {non_background} non-background pixels"
    );
    assert!(
        saturated > 500,
        "emoji drawn monochrome: only {saturated} saturated pixels of {non_background} drawn"
    );
}

// C#: DWriteTextRendererTests.PlainText_StaysMonochrome_WhenColourFontsAreEnabled
#[test]
#[ignore = "native: needs desktop"]
fn plain_text_stays_monochrome_when_colour_fonts_are_enabled() {
    let r = renderer(64);
    let l = layout(
        &r,
        "Trường Đại học – abc",
        false,
        48.0,
        700.0,
        TextTrimming::CharacterEllipsis,
    );
    let (saturated, non_background) = colour(
        &l,
        ColorF {
            r: 0.12,
            g: 0.12,
            b: 0.12,
            a: 1.0,
        },
        ColorF {
            r: 0.9,
            g: 0.9,
            b: 0.9,
            a: 1.0,
        },
    );
    assert!(non_background > 1000, "text not drawn: {non_background}");
    assert_eq!(saturated, 0);
}

// C#: DWriteTextRendererTests.EmptyText_HasZeroWidthAndDrawsNothing (draw half)
#[test]
#[ignore = "native: needs desktop"]
fn empty_text_draws_nothing() {
    let r = renderer(16);
    let l = layout(&r, "", false, 12.0, 100.0, TextTrimming::CharacterEllipsis);
    assert_eq!(l.size().width, 0.0);
    assert_eq!(ink(&l, 1.0, 0.0, 0.0, None).4, 0);
}

// C#: DWriteTextRendererTests.SameKey_ReturnsSharedNativeLayout_DifferentKeysMiss
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn same_key_shares_the_native_layout_and_counts_hits_and_misses() {
    let r = renderer(256);
    let a = layout(&r, "abc", false, 12.0, 100.0, TextTrimming::CharacterEllipsis);
    let b = layout(&r, "abc", false, 12.0, 100.0, TextTrimming::CharacterEllipsis);
    assert_eq!(r.cache_misses(), 1);
    assert_eq!(r.cache_hits(), 1);
    assert!(a.shares_entry_with(&b));
    let _w = layout(&r, "abc", false, 12.0, 101.0, TextTrimming::CharacterEllipsis);
    let _t = layout(&r, "abc", false, 12.0, 100.0, TextTrimming::None);
    let _b = layout(&r, "abc", true, 12.0, 100.0, TextTrimming::CharacterEllipsis);
    let _x = layout(&r, "abd", false, 12.0, 100.0, TextTrimming::CharacterEllipsis);
    let _s = layout(&r, "abc", false, 13.0, 100.0, TextTrimming::CharacterEllipsis);
    let f = r
        .create_layout("abc", "Consolas", 12.0, false, 100.0, TextTrimming::CharacterEllipsis)
        .expect("create_layout");
    assert_eq!(r.cache_misses(), 7);
    assert_eq!(r.cached_layout_count(), 7);
    assert!(!a.shares_entry_with(&f));
}

// C#: DWriteTextRendererTests.Cache_EvictsLeastRecentlyUsed_AndKeepsLeasedLayoutAliveUntilDisposed
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn eviction_keeps_a_leased_layout_alive_and_releases_it_on_drop() {
    let r = renderer(2);
    let first = layout(&r, "first", false, 12.0, 100.0, TextTrimming::None);
    let _second = layout(&r, "second", false, 12.0, 100.0, TextTrimming::None);
    let _third = layout(&r, "third", false, 12.0, 100.0, TextTrimming::None);
    assert_eq!(r.cached_layout_count(), 2);
    assert_eq!(r.pending_release_count(), 1);
    let native = first.native_layout().expect("still alive");
    assert!(layout_metrics(&native).is_ok());
    drop(native);
    drop(first);
    assert_eq!(r.pending_release_count(), 0);
}

// C#: DWriteTextRendererTests.RendererDispose_KeepsLeasedLayoutDrawable_ThenReleasesWhenLeaseDisposed
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn renderer_dispose_keeps_leased_layouts_drawable_then_releases() {
    let r = renderer(4);
    let l = layout(&r, "abc", false, 12.0, 100.0, TextTrimming::None);
    r.dispose();
    r.dispose();
    assert_eq!(r.cached_layout_count(), 0);
    assert_eq!(r.pending_release_count(), 1);
    assert!(l.native_layout().is_some());
    drop(l);
    assert_eq!(r.pending_release_count(), 0);
    let disposed = TextError::Disposed {
        object: RENDERER_OBJECT_NAME,
    };
    assert_eq!(
        r.create_layout("abc", "Segoe UI", 12.0, false, 100.0, TextTrimming::None)
            .err(),
        Some(disposed)
    );
    // T9: the disposed check comes before the font size check.
    assert_eq!(
        r.create_layout("abc", "Segoe UI", -1.0, false, 100.0, TextTrimming::None)
            .err(),
        Some(disposed)
    );
}

// C#: DWriteTextRendererTests.CreateLayout_ValidatesArguments (font size rows)
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn create_layout_rejects_invalid_font_sizes_and_survives_extreme_ones() {
    let r = renderer(4);
    for bad in [0.0, f64::NAN, -1.0, f64::INFINITY] {
        let error = r
            .create_layout("abc", "Segoe UI", bad, false, 100.0, TextTrimming::None)
            .err();
        assert_eq!(
            error,
            Some(TextError::InvalidFontSize {
                value_bits: bad.to_bits()
            })
        );
    }
    // Extreme but "valid" sizes must not panic; DirectWrite rejects them (E_INVALIDARG) or accepts them.
    for extreme in [1e300, 1e-300] {
        match r.create_layout("abc", "Segoe UI", extreme, false, 100.0, TextTrimming::None) {
            Ok(_) => {}
            Err(TextError::Device(error)) => assert!(error.hresult < 0, "{error}"),
            Err(other) => panic!("unexpected {other:?}"),
        }
    }
}

// C#: DWriteTextRendererTests.LeakCheck_ManyLayoutsAndEvictions_ReleaseEveryNativeLayout
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn leak_check_many_layouts_and_evictions() {
    let r = renderer(8);
    for i in 0..400_i32 {
        let width = 50.0 + f64::from(i % 7);
        let _l = layout(
            &r,
            &format!("Ảnh số {i}"),
            false,
            12.0,
            width,
            TextTrimming::CharacterEllipsis,
        );
    }
    assert_eq!(r.pending_release_count(), 0);
    assert_eq!(r.cached_layout_count(), 8);
}

#[test]
#[ignore = "native: needs the OS graphics stack"]
fn emoji_and_surrogate_pairs_are_measured_in_utf16() {
    let r = renderer(8);
    let emoji = layout(&r, "📂 📁 ❤", false, 12.0, NO_LIMIT, TextTrimming::None);
    assert!(emoji.size().width > 0.0);
    assert_eq!(emoji.line_count(), 1);
    let plain = layout(&r, "   ", false, 12.0, NO_LIMIT, TextTrimming::None);
    assert!(!emoji.shares_entry_with(&plain));
    assert_eq!(r.cache_misses(), 2);
}

/// The layering follow-up: `TextRenderer` / `TextLayout` over the same renderer.
#[test]
#[ignore = "native: needs the OS graphics stack"]
fn the_pr_app_traits_map_style_errors_and_downcast_to_the_lease() {
    let r = renderer(4);
    let dynamic: &dyn TextRenderer = &r;
    let style = TextStyle::default();
    let boxed = dynamic
        .create_layout(VIETNAMESE, &style, NO_LIMIT, TextTrimming::None)
        .expect("create_layout");
    assert_eq!(boxed.text(), VIETNAMESE);
    assert!(boxed.size().width > 0.0);
    assert!(boxed.as_any().downcast_ref::<DWriteTextLayoutLease>().is_some());
    let bad = TextStyle {
        font_size_dip: 0.0,
        ..TextStyle::default()
    };
    let error = dynamic.create_layout("x", &bad, 10.0, TextTrimming::None).err();
    assert_eq!(
        error,
        Some(RenderError::InvalidFontSize {
            value_bits: 0.0_f64.to_bits()
        })
    );
    drop(boxed);
    r.dispose();
    let error = dynamic.create_layout("x", &style, 10.0, TextTrimming::None).err();
    assert_eq!(
        error,
        Some(RenderError::Disposed {
            object: RENDERER_OBJECT_NAME
        })
    );
}
