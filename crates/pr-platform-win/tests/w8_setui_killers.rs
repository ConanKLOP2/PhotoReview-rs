//! CX-SETUI: pure Settings builder mutation checks; no desktop, user data or clock.
#![cfg(windows)]

// Keep the established placement assertions in this ONE integration binary.
#[path = "w8_settingsui_killers.rs"]
mod established_layout;

use pr_platform_win::dialogs::settings::builder::groups::CheckFlowElement;
use pr_platform_win::dialogs::settings::builder::inputs::SettingsPageBuilder;
use pr_platform_win::dialogs::settings::builder::rows::{SettingsLayoutElement, SettingsLayoutSurface};
use pr_platform_win::dialogs::settings::page_view::TextFn;
use std::rc::Rc;

fn text(value: &'static str) -> TextFn {
    Rc::new(move || value.to_owned())
}

#[test]
fn nested_group_visibility_search_preserves_siblings_and_rejects_unknown_ids() {
    // C# SettingsPageBuilder Flatten / frame group lookup.
    // inputs.rs:164,174,543,544,554,557: return true/false/None/Some and == -> !=.
    // groups.rs:90,93: as_group/as_group_mut -> None.
    let mut builder = SettingsPageBuilder::new(7000);
    let before = builder.check(text("before"), 0);
    let mut nested = 0;
    let outer = builder.group(text("outer"), |builder| {
        builder.check(text("child before"), 0);
        nested = builder.group(text("nested"), |builder| {
            builder.check(text("leaf"), 0);
        });
        builder.check(text("child after"), 0);
    });
    let sibling = builder.group(text("sibling"), |_| {});
    assert_eq!(builder.group_visible(before), None);
    assert_eq!(builder.group_visible(7999), None);
    assert!(!builder.set_group_visible(before, false));
    assert!(!builder.set_group_visible(7999, false));
    assert_eq!(builder.group_visible(nested), Some(true));
    assert!(builder.set_group_visible(nested, false));
    assert_eq!(builder.group_visible(nested), Some(false));
    assert_eq!(builder.group_visible(outer), Some(true));
    assert_eq!(builder.group_visible(sibling), Some(true));
    assert!(builder.set_group_visible(outer, false));
    assert_eq!(builder.group_visible(outer), Some(false));
    assert!(builder.set_group_visible(nested, true));
    assert_eq!(builder.group_visible(nested), Some(true));
    assert_eq!(builder.group_visible(outer), Some(false));
    assert_eq!(builder.group_visible(sibling), Some(true));
}

#[test]
fn flatten_visits_every_descendant_in_preorder_even_when_parent_hidden() {
    // C# SettingsPageBuilder Flatten: parents before descendants, hidden included.
    // inputs.rs:181,534: flatten -> vec![] / delete flatten_element.
    let mut builder = SettingsPageBuilder::new(4000);
    builder.check(text("first"), 0);
    let outer = builder.group(text("outer"), |builder| {
        builder.check(text("second"), 0);
        builder.group(text("inner"), |builder| {
            builder.check(text("third"), 0);
        });
        builder.check(text("fourth"), 0);
    });
    builder.check(text("last"), 0);
    assert!(builder.set_group_visible(outer, false));
    let ids: Vec<Vec<i32>> = builder.flatten().iter().map(|element| element.control_ids()).collect();
    assert_eq!(
        ids,
        vec![
            vec![4001],
            vec![4002, 4003, 4004, 4005, 4006],
            vec![4003],
            vec![4004, 4005],
            vec![4005],
            vec![4006],
            vec![4007],
        ]
    );
}

#[derive(Default)]
struct WrapSurface {
    placements: Vec<(i32, i32, i32, i32, i32)>,
}

impl SettingsLayoutSurface for WrapSurface {
    fn dx(&self, n: i32) -> i32 {
        n
    }
    fn dy(&self, n: i32) -> i32 {
        n
    }
    fn line_height(&self) -> i32 {
        10
    }
    fn field_height(&self) -> i32 {
        12
    }
    fn text_of(&self, _id: i32) -> String {
        "four".to_owned()
    }
    fn text_width(&self, text: &str) -> i32 {
        text.len() as i32
    }
    fn text_height(&self, _id: i32, _width: i32) -> i32 {
        10
    }
    fn is_hidden(&self, _id: i32) -> bool {
        false
    }
    fn combo_drop_height(&self, _id: i32) -> i32 {
        20
    }
    fn place(&mut self, id: i32, x: i32, y: i32, width: i32, height: i32) {
        self.placements.push((id, x, y, width, height));
    }
    fn hide(&mut self, _id: i32) {
        panic!("visible flow must place every checkbox");
    }
}

#[test]
fn check_flow_exact_right_edge_stays_on_row_and_one_pixel_less_wraps() {
    // C# SettingsCheckFlowElement.PlaceVisible: cursorX + itemWidth > right.
    // groups.rs:234:41 `>` -> `>=` was MISSED; pin equality and both sides.
    let flow = CheckFlowElement::new(vec![1, 2], 0);
    for width in [50, 51] {
        let mut surface = WrapSurface::default();
        assert_eq!(flow.place(&mut surface, 7, 11, width), 17);
        assert_eq!(surface.placements, vec![(1, 7, 11, 20, 12), (2, 37, 11, 20, 12)]);
    }
    let mut surface = WrapSurface::default();
    assert_eq!(flow.place(&mut surface, 7, 11, 49), 33);
    assert_eq!(surface.placements, vec![(1, 7, 11, 20, 12), (2, 7, 27, 20, 12)]);
}
