use pr_platform_win::dialogs::settings::builder::groups::{CheckElement, CheckFlowElement, GroupElement, RadioElement};
use pr_platform_win::dialogs::settings::builder::rows::{
    ButtonsElement, LayoutElement, RowElement, RowKind, SettingsLayoutElement, SettingsLayoutSurface, SettingsRow,
    SettingsSliderRow, SliderElement, TextElement, label_width,
};
use std::collections::HashSet;

struct Surface {
    hidden: HashSet<i32>,
    calls: Vec<(i32, i32, i32, i32, i32)>,
}
impl Surface {
    fn new() -> Self {
        Self {
            hidden: HashSet::new(),
            calls: Vec::new(),
        }
    }
}
impl SettingsLayoutSurface for Surface {
    fn dx(&self, n: i32) -> i32 {
        n
    }
    fn dy(&self, n: i32) -> i32 {
        n
    }
    fn line_height(&self) -> i32 {
        16
    }
    fn field_height(&self) -> i32 {
        23
    }
    fn text_of(&self, id: i32) -> String {
        format!("text{id}")
    }
    fn text_height(&self, _id: i32, _width: i32) -> i32 {
        16
    }
    fn text_width(&self, text: &str) -> i32 {
        text.encode_utf16().count() as i32 * 7
    }
    fn is_hidden(&self, id: i32) -> bool {
        self.hidden.contains(&id)
    }
    fn combo_drop_height(&self, _id: i32) -> i32 {
        80
    }
    fn place(&mut self, id: i32, x: i32, y: i32, w: i32, h: i32) {
        self.calls.push((id, x, y, w, h));
    }
    fn hide(&mut self, id: i32) {
        self.calls.push((id, i32::MIN, 0, 0, 0));
    }
}

// C#: SettingsLayoutElement.LabelWidth
#[test]
fn golden_label_width_rows() {
    let s = Surface::new();
    assert_eq!(label_width(&s, 100), 80);
    assert_eq!(label_width(&s, 300), 114);
    assert_eq!(label_width(&s, 1000), 140);
}
// C#: SettingsRowElement.ControlIds
#[test]
fn row_control_ids_keep_label_then_control_order() {
    let e = RowElement::new(1, 2, RowKind::Combo, 0);
    assert_eq!(e.control_ids(), vec![1, 2]);
}
// C#: SettingsSliderElement.ControlIds
#[test]
fn slider_control_ids_keep_label_slider_value_order() {
    let e = SliderElement::new(1, 2, 3);
    assert_eq!(e.control_ids(), vec![1, 2, 3]);
}
// C#: SettingsTextElement.ControlIds
#[test]
fn text_control_ids_contain_its_id() {
    let e = TextElement::new(4, false);
    assert_eq!(e.control_ids(), vec![4]);
}
// C#: SettingsLayoutElement.Place
#[test]
fn invisible_element_hides_every_id_in_control_ids_order_and_returns_zero() {
    let mut e = RowElement::new(1, 2, RowKind::Edit, 0);
    e.set_visible(false);
    let mut s = Surface::new();
    assert_eq!(e.place(&mut s, 0, 0, 200), 0);
    assert_eq!(s.calls, vec![(1, i32::MIN, 0, 0, 0), (2, i32::MIN, 0, 0, 0)]);
}
// C#: SettingsGroupElement.ControlIds
#[test]
fn group_control_ids_start_with_the_title_id() {
    let child: LayoutElement = Box::new(CheckElement::new(2, 0));
    let g = GroupElement::new(1, vec![child]);
    assert_eq!(g.control_ids(), vec![1, 2]);
}
// C#: SettingsGroupElement.PlaceVisible
#[test]
fn group_place_honours_a_hidden_child_and_a_hidden_nested_group() {
    let mut nested = GroupElement::new(3, vec![Box::new(CheckElement::new(4, 0))]);
    nested.set_visible(false);
    let g = GroupElement::new(1, vec![Box::new(CheckElement::new(2, 0)), Box::new(nested)]);
    let mut s = Surface::new();
    let used = g.place(&mut s, 0, 0, 200);
    // Expected values: the real C# SettingsGroupElement/SettingsCheckElement code (SettingsPageBuilder.cs, identical at e7c2fcd8 and 7eaee2a2)
    // run verbatim in a scratch console project with this file's fake surface (dx = dy = identity,
    // line height 16, text height 16); the check is 18 high at (8, 21) inside the 8 px padded group, the hidden nested
    // group and its child are hidden (id order 3, 4) and the title spans 45 px.
    assert_eq!(used, 53);
    assert_eq!(
        s.calls,
        vec![
            (2, 8, 21, 184, 18),
            (3, i32::MIN, 0, 0, 0),
            (4, i32::MIN, 0, 0, 0),
            (1, 0, 0, 200, 45)
        ]
    );
}
// C#: SettingsLayoutElement.AsGroup
#[test]
fn as_group_and_as_group_mut_find_nested_groups_and_toggle_visible() {
    let mut g = GroupElement::new(1, vec![Box::new(GroupElement::new(2, vec![]))]);
    assert!(g.as_group().is_some());
    let child = g.children_mut().first_mut().expect("nested child");
    child.as_group_mut().expect("nested group").set_visible(false);
    assert!(!child.visible());
}
// C#: SettingsCheckFlowElement.PlaceVisible
#[test]
fn check_flow_does_not_skip_hidden_items() {
    let mut s = Surface::new();
    s.hidden.insert(2);
    let e = CheckFlowElement::new(vec![1, 2], 0);
    let used = e.place(&mut s, 0, 0, 200);
    // C# SettingsCheckFlowElement.PlaceVisible run verbatim (see the group test): the label "text1" is 5 * 7 + 16 = 51 wide,
    // the second item sits after the 10 px gap, and the hidden id 2 is still placed (no Hide call).
    assert_eq!(used, 23);
    assert_eq!(s.calls, vec![(1, 0, 0, 51, 18), (2, 61, 0, 51, 18)]);
}
// C#: SettingsRadioElement.PlaceVisible
#[test]
fn radio_hidden_hint_alone_is_still_placed() {
    let mut s = Surface::new();
    s.hidden.insert(9);
    let e = RadioElement::new(1, Some(9), 0);
    let used = e.place(&mut s, 0, 0, 200);
    // C# SettingsRadioElement.PlaceVisible run verbatim (see the group test): only the radio's own hidden state hides the
    // pair, so the hidden hint is placed 12 px in, under the radio.
    assert_eq!(used, 42);
    assert_eq!(s.calls, vec![(1, 0, 0, 200, 18), (9, 12, 20, 188, 16)]);
}
// C#: SettingsLayoutElement.LabelWidth
#[test]
fn label_width_does_not_panic_when_the_scale_inverts_the_clamp() {
    struct Negative;
    impl SettingsLayoutSurface for Negative {
        fn dx(&self, n: i32) -> i32 {
            -n
        }
        fn dy(&self, n: i32) -> i32 {
            n
        }
        fn line_height(&self) -> i32 {
            1
        }
        fn field_height(&self) -> i32 {
            1
        }
        fn text_of(&self, _: i32) -> String {
            String::new()
        }
        fn text_height(&self, _: i32, _: i32) -> i32 {
            1
        }
        fn text_width(&self, _: &str) -> i32 {
            1
        }
        fn is_hidden(&self, _: i32) -> bool {
            false
        }
        fn combo_drop_height(&self, _: i32) -> i32 {
            0
        }
        fn place(&mut self, _: i32, _: i32, _: i32, _: i32, _: i32) {}
        fn hide(&mut self, _: i32) {}
    }
    let _ = label_width(&Negative, 100);
}
// C#: SettingsLayoutElement.PlaceVisible
#[test]
fn layout_arithmetic_saturates_at_i32_extremes() {
    let mut s = Surface::new();
    let e = CheckElement::new(1, i32::MAX);
    let used = e.place(&mut s, i32::MAX, i32::MIN, i32::MAX);
    // Expected values are the ones written down in docs/reviews/test-strengthening-plan.md (tp-pr-platform-win T8), taken from the
    // C# rule (SettingsCheckElement.PlaceVisible) before any run; the saturation at the i32 extremes is the Rust-only part.
    assert_eq!(used, 22);
    assert_eq!(s.calls, vec![(1, i32::MAX, i32::MIN, 0, 18)]);
}
// C#: SettingsRow and SettingsSliderRow
#[test]
fn rows_records_are_copy_and_comparable() {
    let a = SettingsRow { label: 1, control: 2 };
    let b = a;
    assert_eq!(a, b);
    let c = SettingsSliderRow {
        label: 1,
        slider: 2,
        value: 3,
    };
    let d = c;
    assert_eq!(c, d);
}
// C#: SettingsLayoutElement.Place
#[test]
fn layout_elements_are_object_safe() {
    let mut elements: Vec<LayoutElement> = vec![
        Box::new(CheckElement::new(1, 0)),
        Box::new(RowElement::new(2, 3, RowKind::Edit, 0)),
        Box::new(SliderElement::new(4, 5, 6)),
        Box::new(TextElement::new(7, false)),
        Box::new(ButtonsElement::new(vec![8], false)),
        Box::new(RadioElement::new(9, None, 0)),
        Box::new(CheckFlowElement::new(vec![10], 0)),
        Box::new(GroupElement::new(11, vec![])),
    ];
    let mut s = Surface::new();
    let used: Vec<i32> = elements.iter_mut().map(|e| e.place(&mut s, 0, 0, 300)).collect();
    // C# run verbatim (see the group test): used heights in declaration order, and the placed rectangles of all 8 elements
    // (the elements share one surface and one origin, so the rectangles are each element's own, not stacked).
    assert_eq!(used, vec![22, 28, 29, 21, 28, 24, 23, 31]);
    assert_eq!(
        s.calls,
        vec![
            (1, 0, 0, 300, 18),
            (2, 0, 3, 114, 16),
            (3, 120, 0, 180, 23),
            (4, 0, 4, 114, 16),
            (5, 120, 0, 133, 24),
            (6, 259, 4, 41, 16),
            (7, 0, 0, 300, 16),
            (8, 0, 0, 51, 23),
            (9, 0, 0, 300, 18),
            (10, 0, 0, 58, 18),
            (11, 0, 0, 300, 23),
        ]
    );
}
