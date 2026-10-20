//! Red tests (W8-15c, test porter) for `pr_app::overlay::button::ButtonElement`.
//!
//! Port of the button part of `PhotoReview.Shell.Tests/Overlay/OverlayLayoutTests.cs` and `OverlayPartsTests.cs` (baseline
//! `d92d6aae`, the C# repo at `C:/MyProjects/PhotoReview`), plus F tests (`// new:`) for the traps T2 to T14 of
//! `contracts/W8-15c.md`. The fakes are in `overlay_fakes.rs` (`FakeTextRenderer`: width = characters x size / 2, height =
//! size x 1.5; `RecordingDrawContext`).
//!
//! Every test calls a body that still ends in `todo!()` (directly or through `measure`, `arrange`, `render`, `click`), so the
//! tests fail until the porter fills `overlay::button`. They compile against the stub's public API; `layout()` is not used
//! (it is `pub(crate)` on `TextElement` only, and the button has no accessor), so the lease is observed through the fake's
//! live count.

#[path = "overlay_fakes.rs"]
mod overlay_fakes;

use overlay_fakes::{FakeTextRenderer, RecordingDrawContext};
use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::overlay::button::ButtonElement;
use pr_app::overlay::contracts::{OpacityTarget, OverlayElement, OverlayLayoutContext};
use pr_app::overlay::element::{ElementError, ElementNode, OverlayHorizontalAlignment, OverlayVerticalAlignment};
use pr_app::overlay::palette;
use pr_app::services::render::ColorF;
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

/// Left/top alignment, so the element keeps its desired size (C# tests set `HorizontalAlignment = Left`, `VerticalAlignment = Top`).
fn align_top_left(button: &mut ButtonElement) {
    let base = ElementNode::base_mut(button);
    base.horizontal_alignment = OverlayHorizontalAlignment::Left;
    base.vertical_alignment = OverlayVerticalAlignment::Top;
}

fn assert_rect(actual: RectD, x: f64, y: f64, width: f64, height: f64) {
    assert_eq!((actual.x, actual.y, actual.width, actual.height), (x, y, width, height));
}

/// Palette colour by its C# key (`DarkPalette.Get`), for the keys that have no named function.
fn palette_color(key: &str) -> ColorF {
    palette::get(key).expect("palette key exists")
}

#[test]
fn button_size_includes_padding_and_border_and_hit_test_follows_visibility() {
    // C#: OverlayLayoutTests.Button_SizeIncludesPaddingAndBorder_AndHitTestFollowsVisibility
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("Fit").expect("non-empty id");
    button.set_content("Fit");
    button.set_style(style(10.0));
    align_top_left(&mut button);

    button.arrange(&context(&fake));

    // "Fit" = 15 x 15; + padding 8,4 + border 1 on each side.
    assert_rect(button.bounds(), 0.0, 0.0, 15.0 + 16.0 + 2.0, 15.0 + 8.0 + 2.0);
    assert!(button.hit_test(PointD { x: 5.0, y: 5.0 }));
    assert!(!button.hit_test(PointD { x: 50.0, y: 5.0 }));

    let mut clicked: Vec<String> = Vec::new();
    assert!(button.click(&mut |id: &str| clicked.push(id.to_owned())));
    button.is_enabled = false;
    assert!(!button.click(&mut |id: &str| clicked.push(id.to_owned())));
    button.is_enabled = true;
    button.set_opacity(0.0);
    assert!(!button.hit_test(PointD { x: 5.0, y: 5.0 }));
    assert!(!button.click(&mut |id: &str| clicked.push(id.to_owned())));
    assert_eq!(clicked, vec!["Fit".to_owned()]);
}

#[test]
fn button_render_picks_background_by_state() {
    // C#: OverlayLayoutTests.Button_Render_PicksBackgroundByState
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content("OK");
    button.set_style(style(10.0));
    button.arrange(&context(&fake));

    // (hot, pressed, enabled) -> background of the first fill.
    let cases = [
        (false, false, true, palette::button()),
        (true, false, true, palette::button_hover()),
        (true, true, true, palette::button_pressed()),
        (true, true, false, palette::button()),
    ];
    for (hot, pressed, enabled, expected) in cases {
        button.is_hot = hot;
        button.is_pressed = pressed;
        button.is_enabled = enabled;
        let mut dc = RecordingDrawContext::new();
        button.render(&mut dc);
        assert_eq!(
            dc.fills[0].color, expected,
            "hot={hot} pressed={pressed} enabled={enabled}"
        );
    }
}

#[test]
fn button_fixed_size_wins_over_content_and_focus_and_checked_change_chrome() {
    // C#: OverlayPartsTests.Button_FixedSizeWinsOverContent_AndFocusAndCheckedChangeTheChrome
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("T").expect("non-empty id");
    button.set_content("xxxxxxxxxxxxxxxxxxxxxxxx");
    button.set_style(style(18.0));
    button.fixed_width = 34.0;
    button.fixed_height = 32.0;
    align_top_left(&mut button);

    button.arrange(&context(&fake));
    assert_eq!(button.bounds().width, 34.0);
    assert_eq!(button.bounds().height, 32.0);

    let mut plain = RecordingDrawContext::new();
    button.render(&mut plain);
    assert_eq!(plain.fills.len(), 1);
    assert_eq!(plain.fills[0].color, palette::button());

    button.is_checked = true;
    let mut pressed = RecordingDrawContext::new();
    button.render(&mut pressed);
    assert_eq!(pressed.fills.len(), 1);
    assert_eq!(pressed.fills[0].color, palette::button_pressed());
}

#[test]
fn button_measure_is_one_line_and_ignores_the_offered_width() {
    // C#: OverlayLayoutTests.MaxWidth_CapsTheDesiredWidth_EvenWhenAChildCannotShrink (button part).
    // 40 x "x" at size 10 = 200 DIP (char width 5); + padding 16 + border 2 = 218, whatever the offered width.
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content(&"x".repeat(40));
    button.set_style(style(10.0));

    let desired = button.measure(
        SizeD {
            width: 100.0,
            height: 300.0,
        },
        &context(&fake),
    );

    assert_eq!(desired.width, 218.0);
    assert_eq!(desired.height, 15.0 + 8.0 + 2.0);
}

#[test]
fn button_text_is_requested_with_infinite_width_and_no_trimming() {
    // new: T3. The button text is one line: the layout is asked for an infinite width and no trimming, whatever the offered width.
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content(&"x".repeat(40));
    button.set_style(style(10.0));

    button.measure(
        SizeD {
            width: 100.0,
            height: 300.0,
        },
        &context(&fake),
    );

    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].2, f64::INFINITY);
    assert_eq!(requests[0].3, TextTrimming::None);
}

#[test]
fn button_remeasures_with_a_new_renderer_and_releases_the_old_layout() {
    // C#: OverlayLayoutTests.TextElement_RemeasuresWithANewRenderer_AndReleasesTheOldLayout (button twin)
    let first = FakeTextRenderer::new();
    let second = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content("OK");
    button.set_style(style(10.0));

    button.measure(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        &context(&first),
    );
    assert_eq!(first.live(), 1);

    button.measure(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        &context(&second),
    );
    assert_eq!(first.live(), 0);
    assert_eq!(second.request_count(), 1);
    assert_eq!(second.live(), 1);
}

#[test]
fn button_measure_failure_leaves_no_layout() {
    // new: T2. The old lease is given back before `create_layout` runs; when it fails no lease is kept, and the next measure retries.
    let good = FakeTextRenderer::new();
    let bad = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content("OK");
    button.set_style(style(10.0));

    button.measure(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        &context(&good),
    );
    assert_eq!(good.live(), 1);

    bad.fail.set(true);
    button.measure(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        &context(&bad),
    );
    assert_eq!(good.live(), 0);
    assert_eq!(bad.request_count(), 1);
    assert_eq!(bad.live(), 0);

    bad.fail.set(false);
    button.measure(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        &context(&bad),
    );
    assert_eq!(bad.request_count(), 2);
    assert_eq!(bad.live(), 1);
}

#[test]
fn button_empty_content_still_measures_one_layout_and_draws_no_text() {
    // new: T12. An empty content still asks the renderer once (the request count matches C#), and draws no text.
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    align_top_left(&mut button);

    button.arrange(&context(&fake));
    assert_eq!(fake.request_count(), 1);

    let mut dc = RecordingDrawContext::new();
    button.render(&mut dc);
    assert!(dc.texts.is_empty());
    assert!(dc.clips.is_empty());
    assert_eq!(dc.clip_depth(), 0);
}

#[test]
fn button_content_change_drops_layout_only_when_different() {
    // new: an equal content keeps the layout; a different one (also a case change, T6) drops it at once.
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content("OK");
    button.set_style(style(10.0));
    button.measure(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        &context(&fake),
    );
    assert_eq!(fake.live(), 1);

    button.set_content("OK");
    assert_eq!(fake.live(), 1);

    button.set_content("Ok");
    assert_eq!(fake.live(), 0);
}

#[test]
fn button_style_with_nan_size_does_not_drop_layout() {
    // new: T4 for the button. A style with a NaN size equals itself (C# record equality), so setting it again keeps the layout.
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content("OK");

    button.set_style(style(f64::NAN));
    button.measure(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        &context(&fake),
    );
    assert_eq!(fake.live(), 1);

    button.set_style(style(f64::NAN));
    assert_eq!(fake.live(), 1);
    button.measure(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        &context(&fake),
    );
    assert_eq!(fake.request_count(), 1);
}

#[test]
fn button_focus_border_uses_accent_hover() {
    // new: a focused button strokes its border in ACCENT_HOVER (C# `DarkPalette.Get("Dark.AccentHover")`).
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content("OK");
    button.set_style(style(10.0));
    button.arrange(&context(&fake));
    button.is_focused = true;

    let mut dc = RecordingDrawContext::new();
    button.render(&mut dc);

    assert_eq!(dc.strokes.len(), 1);
    assert_eq!(dc.strokes[0].1, palette_color("Dark.AccentHover"));
}

#[test]
fn button_disabled_text_uses_text_muted() {
    // new: a disabled button draws its text in TEXT_MUTED (C# `DarkPalette.Get("Dark.TextMuted")`).
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content("OK");
    button.set_style(style(10.0));
    button.arrange(&context(&fake));
    button.is_enabled = false;

    let mut dc = RecordingDrawContext::new();
    button.render(&mut dc);

    assert_eq!(dc.texts.len(), 1);
    assert_eq!(dc.texts[0].2, palette_color("Dark.TextMuted"));
}

#[test]
fn button_zero_corner_radius_fills_a_plain_rectangle() {
    // new: T9. A corner radius of zero (not greater than zero) fills a plain rectangle, never a rounded one.
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content("OK");
    button.set_style(style(10.0));
    button.corner_radius = 0.0;
    button.arrange(&context(&fake));

    let mut dc = RecordingDrawContext::new();
    button.render(&mut dc);

    assert!(dc.calls.contains(&"fill_rectangle"));
    assert!(!dc.calls.contains(&"fill_rounded_rectangle"));
}

#[test]
fn button_click_sink_receives_the_element_id_and_is_not_called_when_rejected() {
    // new: T7 and decision O3. The sink gets the element id once per accepted click; a rejected click never calls it.
    let mut button = ButtonElement::new("Tool").expect("non-empty id");
    button.set_content("T");
    let mut ids: Vec<String> = Vec::new();

    assert!(button.click(&mut |id: &str| ids.push(id.to_owned())));

    button.is_enabled = false;
    assert!(!button.click(&mut |id: &str| ids.push(id.to_owned())));
    button.is_enabled = true;

    button.set_opacity(0.0);
    assert!(!button.click(&mut |id: &str| ids.push(id.to_owned())));
    button.set_opacity(1.0);

    ElementNode::base_mut(&mut button).is_visible = false;
    assert!(!button.click(&mut |id: &str| ids.push(id.to_owned())));

    assert_eq!(ids, vec!["Tool".to_owned()]);
}

#[test]
fn button_new_rejects_an_empty_id() {
    // new: `ElementError::EmptyId` replaces the C# `ArgumentException` of the constructor.
    assert_eq!(ButtonElement::new("").err(), Some(ElementError::EmptyId));
}

#[test]
fn button_release_resources_drops_the_layout() {
    // new: semantics 13. `release_resources` gives the lease back at once (live 0).
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content("OK");
    button.set_style(style(10.0));
    button.measure(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        &context(&fake),
    );
    assert_eq!(fake.live(), 1);

    button.release_resources();

    assert_eq!(fake.live(), 0);
}

#[test]
fn button_debug_does_not_require_layout_debug() {
    // new: T14. `Debug` is hand-written (a `Box<dyn TextLayout>` has no `Debug`); it names the element.
    let button = ButtonElement::new("Debug-me").expect("non-empty id");

    let text = format!("{button:?}");

    assert!(text.contains("Debug-me"));
}

/// Context with an explicit renderer id (decision O2).
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

/// A 34 x 32 button with "OK" (10 x 15) placed at slot origin (10, 20), border `border`, corner radius `radius`.
fn placed_button(border: f64, radius: f64) -> (ButtonElement, FakeTextRenderer) {
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content("OK");
    button.set_style(style(10.0));
    button.fixed_width = 34.0;
    button.fixed_height = 32.0;
    button.border_thickness = border;
    button.corner_radius = radius;
    align_top_left(&mut button);
    button.measure(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        &context(&fake),
    );
    button.arrange_in(
        RectD {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 100.0,
        },
        &context(&fake),
    );
    (button, fake)
}

#[test]
fn button_remeasure_follows_the_renderer_id_not_the_address() {
    // new: O2 (INV-1). Layout reuse keys on the renderer id: the same renderer under a new id re-measures, and a different
    // renderer (or a replacement at the same address) under the old id keeps the layout only if the ids are equal.
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content("x");
    let size = SizeD {
        width: 100.0,
        height: 100.0,
    };
    button.measure(size, &context_with_id(&fake, 1));
    button.measure(size, &context_with_id(&fake, 1));
    assert_eq!(fake.request_count(), 1);
    button.measure(size, &context_with_id(&fake, 2));
    assert_eq!(fake.request_count(), 2);
    assert_eq!(fake.live(), 1);
}

#[test]
fn button_boxed_zero_sized_renderers_with_distinct_ids_remeasure() {
    // new: O2. Two boxed zero-sized renderers share one dangling address; distinct ids still re-measure (C# ReferenceEquals).
    struct ZstLayout;
    impl pr_app::services::render::text::TextLayout for ZstLayout {
        fn size(&self) -> SizeD {
            SizeD {
                width: 1.0,
                height: 1.0,
            }
        }
        fn text(&self) -> &str {
            ""
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    struct ZstRenderer;
    thread_local! { static CALLS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }
    impl pr_app::services::render::text::TextRenderer for ZstRenderer {
        fn create_layout(
            &self,
            _text: &str,
            _style: &TextStyle,
            _max_width: f64,
            _trimming: TextTrimming,
        ) -> Result<Box<dyn pr_app::services::render::text::TextLayout>, pr_app::services::render::error::RenderError>
        {
            CALLS.with(|c| c.set(c.get() + 1));
            Ok(Box::new(ZstLayout))
        }
    }
    let size = SizeD {
        width: 100.0,
        height: 100.0,
    };
    let client = SizeD {
        width: 800.0,
        height: 600.0,
    };
    let a = Box::new(ZstRenderer);
    let b = Box::new(ZstRenderer);
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content("x");
    button.measure(size, &OverlayLayoutContext::with_renderer_id(client, 1.0, &*a, 1));
    button.measure(size, &OverlayLayoutContext::with_renderer_id(client, 1.0, &*b, 2));
    assert_eq!(CALLS.with(std::cell::Cell::get), 2);
}

#[test]
fn button_border_thickness_two_counts_twice_in_the_size() {
    // new: kills `2.0 * border` -> `2.0 / border` (equal only at 1). "Fit" 15 x 15 + padding 16 x 8 + chrome 4.
    let fake = FakeTextRenderer::new();
    let mut button = ButtonElement::new("Fit").expect("non-empty id");
    button.set_content("Fit");
    button.set_style(style(10.0));
    button.border_thickness = 2.0;
    let desired = button.measure(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        &context(&fake),
    );
    assert_eq!((desired.width, desired.height), (15.0 + 16.0 + 4.0, 15.0 + 8.0 + 4.0));
}

#[test]
fn button_border_and_corner_gates_follow_the_sign() {
    // new: kills `>` -> `<` / `>=` in the border and corner radius gates.
    let (zero, fake) = placed_button(0.0, 0.0);
    let mut dc = RecordingDrawContext::new();
    zero.render(&mut dc);
    assert!(dc.strokes.is_empty(), "no border at thickness 0");
    assert!(!dc.fills[0].rounded, "plain fill at radius 0");
    drop(fake);

    let (some, _fake) = placed_button(1.0, 2.0);
    let mut dc = RecordingDrawContext::new();
    some.render(&mut dc);
    assert_eq!(dc.strokes.len(), 1);
    assert!(dc.fills[0].rounded);
}

#[test]
fn button_stroke_rect_is_inset_by_half_the_border() {
    // new: border 2 at bounds (10, 20, 34, 32): half = 1, stroke (11, 21, 32, 30), width 2.
    let (button, _fake) = placed_button(2.0, 2.0);
    let mut dc = RecordingDrawContext::new();
    button.render(&mut dc);
    let (rect, color, width) = dc.strokes[0];
    assert_rect(rect, 11.0, 21.0, 32.0, 30.0);
    assert_eq!(color, palette::border_strong());
    assert_eq!(width, 2.0);

    // Odd thickness 3: half = 1.5, stroke (11.5, 21.5, 31, 29).
    let (odd, _fake) = placed_button(3.0, 2.0);
    let mut dc = RecordingDrawContext::new();
    odd.render(&mut dc);
    assert_rect(dc.strokes[0].0, 11.5, 21.5, 31.0, 29.0);
}

#[test]
fn button_text_is_centred_in_the_bounds() {
    // new: "OK" is 10 x 15 in bounds (10, 20, 34, 32): origin (10 + 12, 20 + 8.5).
    let (button, _fake) = placed_button(1.0, 2.0);
    let mut dc = RecordingDrawContext::new();
    button.render(&mut dc);
    assert_eq!(dc.texts.len(), 1);
    assert_eq!(dc.texts[0].1, PointD { x: 22.0, y: 28.5 });
    assert_eq!(dc.clips[0], button.bounds());
}

#[test]
fn button_getters_find_and_find_at_report_the_state() {
    // new: kills the getter, find and find_at mutants.
    let (mut button, _fake) = placed_button(1.0, 2.0);
    assert_eq!(button.content(), "OK");
    assert_eq!(button.style().font_size_dip, 10.0);
    assert_eq!(button.style().font_family, "Segoe UI");
    let found = button.find_at(PointD { x: 12.0, y: 22.0 }).expect("inside the button");
    assert_eq!(found.base().id(), "B");
    assert!(button.find_at(PointD { x: 500.0, y: 500.0 }).is_none());
    assert_eq!(button.find("B").expect("same id").base().id(), "B");
    assert!(button.find("other").is_none());
    button.set_content("Different");
    assert_eq!(button.content(), "Different");
}
