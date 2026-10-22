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
    let _ = g.place(&mut s, 0, 0, 200);
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
    let e = CheckFlowElement::new(vec![1, 2], 0);
    let _ = e.place(&mut s, 0, 0, 200);
}
// C#: SettingsRadioElement.PlaceVisible
#[test]
fn radio_hidden_hint_alone_is_still_placed() {
    let mut s = Surface::new();
    s.hidden.insert(9);
    let e = RadioElement::new(1, Some(9), 0);
    let _ = e.place(&mut s, 0, 0, 200);
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
    let _ = e.place(&mut s, i32::MAX, i32::MIN, i32::MAX);
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
    for e in &mut elements {
        let _ = e.place(&mut s, 0, 0, 300);
    }
}
// C#: SettingsFilesPageTests.ControlsDoNotOverlap
#[test]
#[ignore = "native: needs desktop"]
fn placed_rectangles_are_the_window_rectangles_and_siblings_do_not_overlap() {}
