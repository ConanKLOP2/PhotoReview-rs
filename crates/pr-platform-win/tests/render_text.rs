#![cfg(windows)]
//! Native tests of `render::text::renderer` (unit W8-12i, test kind N). C# twins: `DWriteTextRendererTests` (baseline `d92d6aae`,
//! `Category=Native`, `vi-VN`, Segoe UI 12). DirectWrite factory only, no window, no device, no sleeps.
//!
//! NOT written here (they need the Direct2D draw context of W8-12e, which is not merged): `Vietnamese_DrawsGlyphsInsideTheMeasuredBox`,
//! `Ellipsis_DrawsInkOnlyWithinMaxWidth_AndDiffersFromPlainClip`, `Layout_SizeDoesNotDependOnDpi_InkScalesWithIt`,
//! `Layouts_SurviveDeviceRecreation`, `ColourEmoji_DrawnWithColour_NotMonochrome`, `PlainText_StaysMonochrome_WhenColourFontsAreEnabled`
//! and the draw half of `EmptyText_HasZeroWidthAndDrawsNothing`.

use pr_app::services::render::{RenderError, TextRenderer, TextStyle};
use pr_platform_win::gfx::dwrite::layout_metrics;
use pr_platform_win::render::text::layout_cache::TextTrimming;
use pr_platform_win::render::text::renderer::{
    DWriteTextLayoutLease, DWriteTextRenderer, RENDERER_OBJECT_NAME, TextError,
};

const VIETNAMESE: &str = "Trường Đại học Bách khoa – Hà Nội, ảnh đẹp ấm áp nhất";
const NO_LIMIT: f64 = f64::INFINITY;

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
