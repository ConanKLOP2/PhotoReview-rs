//! Red tests (W8-15c, test porter) for `pr_app::overlay::text::TextElement`.
//!
//! Port of the text part of `PhotoReview.Shell.Tests/Overlay/OverlayLayoutTests.cs` (baseline `d92d6aae`, the C# repo at
//! `C:/MyProjects/PhotoReview`) plus F tests (`// new:`) for the traps T2, T4, T5, T6 of `contracts/W8-15c.md`.
//! The fakes are in `overlay_fakes.rs` (`FakeTextRenderer`: width = characters x size / 2, height = size x 1.5, ellipsis cut
//! to whole characters; `RecordingDrawContext`).
//!
//! The C# text-reuse test drives the element through a `PanelElement` (W8-15d). The Rust port calls `measure` directly with
//! the same offered widths in the same order, so the request and live counts are the same. `layout()` is `pub(crate)`, so
//! the lease is observed through the fake's live count only.
//!
//! Every test calls a body that still ends in `todo!()`, so the tests fail until the porter fills `overlay::text`.

#[path = "overlay_fakes.rs"]
mod overlay_fakes;

use overlay_fakes::{FakeTextRenderer, RecordingDrawContext};
use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::overlay::contracts::{OverlayElement, OverlayLayoutContext};
use pr_app::overlay::element::{ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment};
use pr_app::overlay::palette;
use pr_app::overlay::text::TextElement;
use pr_app::services::render::text::{TextStyle, TextTrimming};

/// Text style of the given size, font "Segoe UI", not bold (C# `new TextStyle("Segoe UI", size)`).
fn style(size: f64) -> TextStyle {
    TextStyle {
        font_family: "Segoe UI".to_owned(),
        font_size_dip: size,
        bold: false,
    }
}

/// Layout context of an 800 x 600 client (C# `Context(fake)` default).
fn context(text: &FakeTextRenderer) -> OverlayLayoutContext<'_> {
    OverlayLayoutContext::new(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        1.0,
        text,
    )
}

fn offered(width: f64) -> SizeD {
    SizeD { width, height: 600.0 }
}

fn assert_rect(actual: RectD, x: f64, y: f64, width: f64, height: f64) {
    assert_eq!((actual.x, actual.y, actual.width, actual.height), (x, y, width, height));
}

#[test]
fn text_element_is_click_through_by_default() {
    // C#: OverlayLayoutTests.TextElement_IsClickThroughByDefault
    let fake = FakeTextRenderer::new();
    let mut text = TextElement::new("t").expect("non-empty id");
    text.set_text("hello");

    text.arrange(&context(&fake));

    assert!(!ElementNode::base(&text).is_hit_test_visible);
    assert!(!text.hit_test(PointD { x: 1.0, y: 1.0 }));
    assert!(text.find_at(PointD { x: 1.0, y: 1.0 }).is_none());
}

#[test]
fn text_element_remeasures_with_a_new_renderer_and_releases_the_old_layout() {
    // C#: OverlayLayoutTests.TextElement_RemeasuresWithANewRenderer_AndReleasesTheOldLayout
    let first = FakeTextRenderer::new();
    let second = FakeTextRenderer::new();
    let mut text = TextElement::new("a").expect("non-empty id");
    text.set_text("hello");

    text.measure(offered(800.0), &context(&first));
    assert_eq!(first.live(), 1);

    text.measure(offered(800.0), &context(&second));
    assert_eq!(first.live(), 0);
    assert_eq!(second.request_count(), 1);
    assert_eq!(second.live(), 1);
}

#[test]
fn text_element_reuses_layout_until_style_text_or_width_changes_and_releases_on_dispose() {
    // C#: OverlayLayoutTests.TextElement_ReusesLayout_UntilStyleTextOrWidthChanges_AndReleasesOnDispose
    // The C# test measures through a PanelElement (W8-15d); the same offered widths are used here, in the same order.
    let fake = FakeTextRenderer::new();
    let mut text = TextElement::new("a").expect("non-empty id");
    text.set_text("hello");
    text.set_style(style(10.0));

    text.measure(offered(800.0), &context(&fake));
    text.measure(offered(800.0), &context(&fake));
    assert_eq!(fake.request_count(), 1);
    assert_eq!(fake.live(), 1);

    text.set_text("hello!");
    assert_eq!(fake.live(), 0);
    text.measure(offered(800.0), &context(&fake));
    assert_eq!(fake.request_count(), 2);

    text.measure(offered(400.0), &context(&fake));
    assert_eq!(fake.request_count(), 3);
    assert_eq!(fake.live(), 1);

    text.set_text("hello!");
    text.measure(offered(400.0), &context(&fake));
    assert_eq!(fake.request_count(), 3);

    text.release_resources();
    assert_eq!(fake.live(), 0);
}

#[test]
fn text_element_style_with_nan_size_does_not_invalidate() {
    // new: T4. A style whose size is NaN equals itself (C# record equality, `double.Equals`), so setting it again keeps the layout.
    let fake = FakeTextRenderer::new();
    let mut text = TextElement::new("a").expect("non-empty id");
    text.set_text("hello");

    text.set_style(style(f64::NAN));
    text.measure(offered(800.0), &context(&fake));
    assert_eq!(fake.live(), 1);

    text.set_style(style(f64::NAN));
    assert_eq!(fake.live(), 1);
    text.measure(offered(800.0), &context(&fake));
    assert_eq!(fake.request_count(), 1);
}

#[test]
fn text_element_same_text_different_case_invalidates() {
    // new: T6. Ordinal equality: "Hello" and "hello" are different texts, so the layout is dropped at once.
    let fake = FakeTextRenderer::new();
    let mut text = TextElement::new("a").expect("non-empty id");
    text.set_text("Hello");
    text.measure(offered(800.0), &context(&fake));
    assert_eq!(fake.live(), 1);

    text.set_text("hello");

    assert_eq!(fake.live(), 0);
}

#[test]
fn text_element_trimming_passes_max_width_and_trimming_to_the_renderer() {
    // new: the offered width and the trimming reach the renderer. Char width 5 (size 10): full width 50 > 30, so the
    // ellipsis cut gives floor(30 / 5) * 5 = 30. Without trimming the full 50 is kept.
    let fake = FakeTextRenderer::new();
    let mut text = TextElement::new("a").expect("non-empty id");
    text.set_text("ABCDEFGHIJ");
    text.set_style(style(10.0));
    text.set_trimming(TextTrimming::CharacterEllipsis);

    let trimmed = text.measure(offered(30.0), &context(&fake));

    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, "ABCDEFGHIJ");
    assert_eq!(requests[0].2, 30.0);
    assert_eq!(requests[0].3, TextTrimming::CharacterEllipsis);
    assert_eq!((trimmed.width, trimmed.height), (30.0, 15.0));

    text.set_trimming(TextTrimming::None);
    let untrimmed = text.measure(offered(30.0), &context(&fake));

    let requests = fake.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].3, TextTrimming::None);
    assert_eq!((untrimmed.width, untrimmed.height), (50.0, 15.0));
}

#[test]
fn text_element_empty_text_creates_a_layout_but_renders_nothing() {
    // new: an empty text still asks for one layout (C# `_text` is measured), and draws nothing: no text, no clip.
    let fake = FakeTextRenderer::new();
    let mut text = TextElement::new("a").expect("non-empty id");
    text.set_style(style(10.0));
    text.arrange(&context(&fake));
    assert_eq!(fake.request_count(), 1);

    let mut dc = RecordingDrawContext::new();
    text.render(&mut dc);

    assert!(dc.texts.is_empty());
    assert!(dc.clips.is_empty());
    assert!(!dc.calls.contains(&"draw_text"));
}

#[test]
fn text_element_renders_clip_then_text_at_bounds_origin() {
    // new: one push_clip(bounds), one draw_text at (bounds.x, bounds.y), one pop_clip. No centring: the layout sits at the top-left.
    let fake = FakeTextRenderer::new();
    let mut text = TextElement::new("a").expect("non-empty id");
    text.set_text("hi");
    text.set_style(style(10.0));
    ElementNode::base_mut(&mut text).horizontal_alignment = OverlayHorizontalAlignment::Left;
    ElementNode::base_mut(&mut text).vertical_alignment = OverlayVerticalAlignment::Top;
    ElementNode::base_mut(&mut text).margin = OverlayThickness::new(10.0, 20.0, 0.0, 0.0);
    text.arrange(&context(&fake));
    let bounds = text.bounds();

    let mut dc = RecordingDrawContext::new();
    text.render(&mut dc);

    // The base `render` wraps the core in push/pop opacity; the core itself is clip, text, clip pop.
    assert_eq!(
        dc.calls,
        vec!["push_opacity", "push_clip", "draw_text", "pop_clip", "pop_opacity"]
    );
    assert_eq!(dc.clips, vec![bounds]);
    assert_eq!(dc.texts.len(), 1);
    assert_eq!(dc.texts[0].0, "hi");
    assert_eq!((dc.texts[0].1.x, dc.texts[0].1.y), (bounds.x, bounds.y));
    assert_rect(bounds, 10.0, 20.0, 10.0, 15.0);
}

#[test]
fn text_element_failed_measure_retries_on_the_next_measure() {
    // new: T2 for the text. A failed `create_layout` gives back the old lease first; the next measure creates a new one.
    let fake = FakeTextRenderer::new();
    let mut text = TextElement::new("a").expect("non-empty id");
    text.set_text("hello");
    text.set_style(style(10.0));
    text.measure(offered(800.0), &context(&fake));
    assert_eq!(fake.live(), 1);

    fake.fail.set(true);
    text.measure(offered(400.0), &context(&fake));
    assert_eq!(fake.request_count(), 2);
    assert_eq!(fake.live(), 0);

    fake.fail.set(false);
    text.measure(offered(400.0), &context(&fake));
    assert_eq!(fake.request_count(), 3);
    assert_eq!(fake.live(), 1);
}

#[test]
fn text_element_default_colour_is_text_bright() {
    // new: the default colour is TEXT_BRIGHT (C# `DarkPalette.TextBright`).
    let text = TextElement::new("a").expect("non-empty id");

    assert_eq!(text.color, palette::text_bright());
}

#[test]
fn text_element_release_resources_invalidates_and_forgets_the_owner() {
    // new: after `release_resources` the same renderer is asked again (the owner is forgotten, the width is NaN).
    let fake = FakeTextRenderer::new();
    let mut text = TextElement::new("a").expect("non-empty id");
    text.set_text("hello");
    text.set_style(style(10.0));
    text.measure(offered(800.0), &context(&fake));
    assert_eq!(fake.live(), 1);

    text.release_resources();
    assert_eq!(fake.live(), 0);

    text.measure(offered(800.0), &context(&fake));
    assert_eq!(fake.request_count(), 2);
    assert_eq!(fake.live(), 1);
}

#[test]
fn text_element_measure_nan_width_re_measures_every_time() {
    // new: T5. The width test is IEEE `!=`: a NaN offered width never equals the stored one, so each measure asks again.
    // (With `f64_same`, NaN would equal NaN and the second measure would reuse the layout.)
    let fake = FakeTextRenderer::new();
    let mut text = TextElement::new("a").expect("non-empty id");
    text.set_text("hello");
    text.set_style(style(10.0));

    text.measure(offered(f64::NAN), &context(&fake));
    text.measure(offered(f64::NAN), &context(&fake));

    assert_eq!(fake.request_count(), 2);
}

fn context_with_id(text: &FakeTextRenderer, id: u64) -> OverlayLayoutContext<'_> {
    OverlayLayoutContext::with_renderer_id(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        1.0,
        text,
        id,
    )
}

#[test]
fn text_remeasure_follows_the_renderer_id_not_the_address() {
    // new: O2 (INV-1). The same renderer under a new id re-measures and releases the old lease; the same id keeps the layout.
    let fake = FakeTextRenderer::new();
    let mut text = TextElement::new("t").expect("non-empty id");
    text.set_text("x");
    text.measure(offered(100.0), &context_with_id(&fake, 1));
    text.measure(offered(100.0), &context_with_id(&fake, 1));
    assert_eq!(fake.request_count(), 1);
    text.measure(offered(100.0), &context_with_id(&fake, 2));
    assert_eq!(fake.request_count(), 2);
    assert_eq!(fake.live(), 1);
}

#[test]
fn text_replacement_renderer_at_the_same_address_with_a_new_id_remeasures() {
    // new: O2. 50 renderers allocated and dropped in turn (the allocator reuses addresses), each with a fresh id: every one is asked.
    let mut text = TextElement::new("t").expect("non-empty id");
    text.set_text("x");
    let mut asked = 0;
    for id in 0..50_u64 {
        let renderer = Box::new(FakeTextRenderer::new());
        text.measure(offered(100.0), &context_with_id(&renderer, id));
        asked += renderer.request_count();
    }
    assert_eq!(asked, 50);
}

#[test]
fn text_getters_find_and_find_at_report_the_state() {
    // new: kills the getter, find and find_at mutants.
    let fake = FakeTextRenderer::new();
    let mut text = TextElement::new("t").expect("non-empty id");
    text.set_text("hello");
    text.set_style(style(10.0));
    assert_eq!(text.text(), "hello");
    assert_eq!(text.style().font_size_dip, 10.0);
    assert_eq!(text.style().font_family, "Segoe UI");
    assert_eq!(text.trimming(), TextTrimming::None);

    ElementNode::base_mut(&mut text).is_hit_test_visible = true;
    ElementNode::base_mut(&mut text).horizontal_alignment = OverlayHorizontalAlignment::Left;
    ElementNode::base_mut(&mut text).vertical_alignment = OverlayVerticalAlignment::Top;
    text.arrange(&context(&fake));
    let found = text.find_at(PointD { x: 1.0, y: 1.0 }).expect("hit-testable text");
    assert_eq!(found.base().id(), "t");
    assert!(text.find_at(PointD { x: 700.0, y: 500.0 }).is_none());
    assert_eq!(text.find("t").expect("same id").base().id(), "t");
    assert!(text.find("other").is_none());
}

#[test]
fn text_debug_names_the_element_and_its_text() {
    // new: kills `Debug::fmt -> Ok(())`.
    let text = TextElement::new("t").expect("non-empty id");
    let shown = format!("{text:?}");
    assert!(shown.contains("TextElement"), "{shown}");
    assert!(shown.contains("has_layout"), "{shown}");
}
