//! Mutation killers of lane cl-settingsui for `dialogs/settings/builder/{groups,rows,inputs}.rs`, `context.rs`, `dialog/{host,frame,surface}.rs`.
//! Pure logic on a fake surface with non-degenerate scale factors (dx = 2n, dy = 3n): no window, no GUI, no sleeps.
//! Each test names the mutant (file:line, operator) it kills. Evidence: docs/reviews/CL-SETTINGSUI-2026-10-30.md.
#![cfg(windows)]

use pr_platform_win::dialogs::settings::builder::groups::{CheckElement, CheckFlowElement, GroupElement, RadioElement};
use pr_platform_win::dialogs::settings::builder::rows::{
    ButtonsElement, RowElement, RowKind, SettingsLayoutElement, SettingsLayoutSurface, SliderElement, TextElement,
    label_width,
};
use std::collections::HashSet;

type Call = (i32, i32, i32, i32, i32);
const HIDE: i32 = i32::MIN;

struct Fake {
    hidden: HashSet<i32>,
    empty: HashSet<i32>,
    mult: i32,
    calls: Vec<Call>,
}
impl Fake {
    fn new() -> Self {
        Self {
            hidden: HashSet::new(),
            empty: HashSet::new(),
            mult: 7,
            calls: Vec::new(),
        }
    }
}
impl SettingsLayoutSurface for Fake {
    fn dx(&self, n: i32) -> i32 {
        n * 2
    }
    fn dy(&self, n: i32) -> i32 {
        n * 3
    }
    fn line_height(&self) -> i32 {
        17
    }
    fn field_height(&self) -> i32 {
        23
    }
    fn text_of(&self, id: i32) -> String {
        if self.empty.contains(&id) {
            String::new()
        } else {
            "a".repeat((id % 7 + 1) as usize)
        }
    }
    fn text_height(&self, id: i32, width: i32) -> i32 {
        10 + id + width / 50
    }
    fn text_width(&self, text: &str) -> i32 {
        text.len() as i32 * self.mult
    }
    fn is_hidden(&self, id: i32) -> bool {
        self.hidden.contains(&id)
    }
    fn combo_drop_height(&self, id: i32) -> i32 {
        100 + id
    }
    fn place(&mut self, id: i32, x: i32, y: i32, w: i32, h: i32) {
        self.calls.push((id, x, y, w, h));
    }
    fn hide(&mut self, id: i32) {
        self.calls.push((id, HIDE, 0, 0, 0));
    }
}

fn run(e: &dyn SettingsLayoutElement, s: &mut Fake, x: i32, y: i32, w: i32) -> (i32, Vec<Call>) {
    let r = e.place(s, x, y, w);
    (r, std::mem::take(&mut s.calls))
}

/// Runs the named scenario on a fresh fake surface.
fn scenario(name: &str) -> (i32, Vec<Call>) {
    let mut s = Fake::new();
    match name {
        "group" => run(
            &GroupElement::new(
                1,
                vec![
                    Box::new(CheckElement::new(2, 3)),
                    Box::new(RadioElement::new(4, Some(5), 3)),
                ],
            ),
            &mut s,
            10,
            20,
            400,
        ),
        "group_narrow" => run(
            &GroupElement::new(
                1,
                vec![
                    Box::new(CheckElement::new(2, 3)),
                    Box::new(RadioElement::new(4, Some(5), 3)),
                ],
            ),
            &mut s,
            10,
            20,
            20,
        ),
        "check" => run(&CheckElement::new(2, 3), &mut s, 10, 20, 400),
        "check_narrow" => run(&CheckElement::new(2, 3), &mut s, 10, 20, 30),
        "radio" => run(&RadioElement::new(4, Some(5), 3), &mut s, 10, 20, 400),
        "radio_narrow" => run(&RadioElement::new(4, Some(5), 3), &mut s, 10, 20, 40),
        "radio_nohint" => run(&RadioElement::new(4, None, 3), &mut s, 10, 20, 400),
        "radio_hidden" => {
            s.hidden.insert(4);
            run(&RadioElement::new(4, Some(5), 3), &mut s, 10, 20, 400)
        }
        "radio_hidden_nohint" => {
            s.hidden.insert(4);
            run(&RadioElement::new(4, None, 3), &mut s, 10, 20, 400)
        }
        "check_hidden" => {
            s.hidden.insert(4);
            run(&CheckElement::new(4, 3), &mut s, 10, 20, 400)
        }
        "flow" => run(&CheckFlowElement::new(vec![1, 2, 3, 4, 5], 3), &mut s, 10, 20, 200),
        "flow_wide" => run(&CheckFlowElement::new(vec![1, 2, 3], 3), &mut s, 10, 20, 400),
        "flow_tiny" => run(&CheckFlowElement::new(vec![6, 6], 3), &mut s, 10, 20, 30),
        "flow_mult" => {
            s.mult = 20;
            run(&CheckFlowElement::new(vec![1, 2, 3, 4, 5], 3), &mut s, 10, 20, 300)
        }
        "row_combo" => run(&RowElement::new(1, 2, RowKind::Combo, 0), &mut s, 10, 20, 400),
        "row_edit" => run(&RowElement::new(1, 2, RowKind::Edit, 0), &mut s, 10, 20, 400),
        "row_width" => run(&RowElement::new(1, 2, RowKind::Edit, 50), &mut s, 10, 20, 400),
        "row_width_big" => run(&RowElement::new(1, 2, RowKind::Combo, 300), &mut s, 10, 20, 400),
        "row_narrow" => run(&RowElement::new(1, 2, RowKind::Edit, 0), &mut s, 10, 20, 60),
        "row_tall" => run(&RowElement::new(40, 2, RowKind::Edit, 0), &mut s, 10, 20, 400),
        "row_hidden" => {
            s.hidden.insert(2);
            run(&RowElement::new(1, 2, RowKind::Edit, 0), &mut s, 10, 20, 400)
        }
        "slider" => run(&SliderElement::new(1, 2, 3), &mut s, 10, 20, 400),
        "slider_narrow" => run(&SliderElement::new(1, 2, 3), &mut s, 10, 20, 150),
        "slider_tall" => run(&SliderElement::new(50, 2, 3), &mut s, 10, 20, 400),
        "slider_wide_value" => {
            s.mult = 20;
            run(&SliderElement::new(1, 2, 6), &mut s, 10, 20, 700)
        }
        "slider_wide_hundred" => {
            s.mult = 20;
            run(&SliderElement::new(1, 2, 3), &mut s, 10, 20, 700)
        }
        "text_aligned" => run(&TextElement::new(8, true), &mut s, 10, 20, 400),
        "text_aligned_narrow" => run(&TextElement::new(8, true), &mut s, 10, 20, 40),
        "text_full" => run(&TextElement::new(8, false), &mut s, 10, 20, 400),
        "text_full_narrow" => run(&TextElement::new(8, false), &mut s, 10, 20, 40),
        "text_empty" => {
            s.empty.insert(8);
            run(&TextElement::new(8, true), &mut s, 10, 20, 400)
        }
        "text_hidden" => {
            s.hidden.insert(8);
            run(&TextElement::new(8, true), &mut s, 10, 20, 400)
        }
        "buttons_aligned" => run(&ButtonsElement::new(vec![1, 2, 3, 4], true), &mut s, 10, 20, 300),
        "buttons_full" => run(&ButtonsElement::new(vec![1, 2, 3, 4], false), &mut s, 10, 20, 200),
        "buttons_tiny" => run(&ButtonsElement::new(vec![1], false), &mut s, 10, 20, 10),
        "buttons_mult" => {
            s.mult = 20;
            run(&ButtonsElement::new(vec![6, 6, 1], false), &mut s, 10, 20, 500)
        }
        "buttons_hidden_one" => {
            s.hidden.insert(2);
            run(&ButtonsElement::new(vec![1, 2, 3], false), &mut s, 10, 20, 400)
        }
        "buttons_all_hidden" => {
            s.hidden.extend([1, 2, 3]);
            run(&ButtonsElement::new(vec![1, 2, 3], false), &mut s, 10, 20, 400)
        }
        other => panic!("unknown scenario {other}"),
    }
}

/// Pinned placements (name, returned height, recorded `place`/`hide` calls), each value checked by hand against the C# arithmetic (dx = 2n, dy = 3n).
const EXPECTED: &[(&str, i32, &[Call])] = &[
    (
        "group",
        165,
        &[
            (2, 32, 52, 362, 23),
            (4, 32, 87, 362, 23),
            (5, 56, 116, 338, 21),
            (1, 10, 20, 400, 141),
        ],
    ),
    (
        "group_narrow",
        160,
        &[
            (2, 32, 52, 74, 23),
            (4, 32, 87, 74, 23),
            (5, 56, 116, 60, 16),
            (1, 10, 20, 20, 136),
        ],
    ),
    ("check", 35, &[(2, 16, 20, 394, 23)]),
    ("check_narrow", 35, &[(2, 16, 20, 24, 23)]),
    ("radio", 70, &[(4, 16, 20, 394, 24), (5, 40, 50, 370, 22)]),
    ("radio_narrow", 63, &[(4, 16, 20, 34, 23), (5, 40, 49, 60, 16)]),
    ("radio_nohint", 42, &[(4, 16, 20, 394, 24)]),
    ("radio_hidden", 0, &[(4, HIDE, 0, 0, 0), (5, HIDE, 0, 0, 0)]),
    ("radio_hidden_nohint", 0, &[(4, HIDE, 0, 0, 0)]),
    ("check_hidden", 0, &[(4, HIDE, 0, 0, 0)]),
    (
        "flow",
        108,
        &[
            (1, 16, 20, 46, 23),
            (2, 82, 20, 53, 23),
            (3, 16, 55, 60, 23),
            (4, 96, 55, 67, 23),
            (5, 16, 90, 74, 23),
        ],
    ),
    (
        "flow_wide",
        38,
        &[(1, 16, 20, 46, 23), (2, 82, 20, 53, 23), (3, 155, 20, 60, 23)],
    ),
    ("flow_tiny", 73, &[(6, 16, 20, 24, 23), (6, 16, 55, 24, 23)]),
    (
        "flow_mult",
        108,
        &[
            (1, 16, 20, 72, 23),
            (2, 108, 20, 92, 23),
            (3, 16, 55, 112, 23),
            (4, 148, 55, 132, 23),
            (5, 16, 90, 152, 23),
        ],
    ),
    ("row_combo", 38, &[(1, 10, 24, 160, 14), (2, 182, 20, 228, 125)]),
    ("row_edit", 38, &[(1, 10, 24, 160, 14), (2, 182, 20, 228, 23)]),
    ("row_width", 38, &[(1, 10, 24, 160, 14), (2, 182, 20, 100, 23)]),
    ("row_width_big", 38, &[(1, 10, 24, 160, 14), (2, 182, 20, 228, 125)]),
    ("row_narrow", 38, &[(1, 10, 24, 160, 14), (2, 182, 20, 60, 23)]),
    ("row_tall", 68, &[(40, 10, 20, 160, 53), (2, 182, 35, 228, 23)]),
    ("row_hidden", 0, &[(1, HIDE, 0, 0, 0), (2, HIDE, 0, 0, 0)]),
    (
        "slider",
        56,
        &[(1, 10, 33, 160, 14), (2, 182, 20, 148, 41), (3, 342, 33, 68, 14)],
    ),
    (
        "slider_narrow",
        56,
        &[(1, 10, 33, 160, 14), (2, 182, 20, 120, 41), (3, 314, 33, 68, 14)],
    ),
    (
        "slider_tall",
        78,
        &[(50, 10, 20, 160, 63), (2, 182, 31, 148, 41), (3, 342, 44, 68, 14)],
    ),
    (
        "slider_wide_value",
        56,
        &[(1, 10, 32, 266, 16), (2, 288, 20, 258, 41), (6, 558, 31, 152, 19)],
    ),
    (
        "slider_wide_hundred",
        56,
        &[(1, 10, 32, 266, 16), (2, 288, 20, 298, 41), (3, 598, 33, 112, 15)],
    ),
    ("text_aligned", 37, &[(8, 182, 20, 228, 22)]),
    ("text_aligned_narrow", 34, &[(8, 182, 20, 60, 19)]),
    ("text_full", 41, &[(8, 10, 20, 400, 26)]),
    ("text_full_narrow", 34, &[(8, 10, 20, 60, 19)]),
    ("text_empty", 0, &[(8, HIDE, 0, 0, 0)]),
    ("text_hidden", 0, &[(8, HIDE, 0, 0, 0)]),
    (
        "buttons_aligned",
        143,
        &[
            (1, 182, 20, 100, 23),
            (2, 182, 55, 100, 23),
            (3, 182, 90, 100, 23),
            (4, 182, 125, 100, 23),
        ],
    ),
    (
        "buttons_full",
        143,
        &[
            (1, 10, 20, 100, 23),
            (2, 10, 55, 100, 23),
            (3, 10, 90, 100, 23),
            (4, 10, 125, 100, 23),
        ],
    ),
    ("buttons_tiny", 38, &[(1, 10, 20, 100, 23)]),
    (
        "buttons_mult",
        38,
        &[(6, 10, 20, 172, 23), (6, 194, 20, 172, 23), (1, 378, 20, 100, 23)],
    ),
    (
        "buttons_hidden_one",
        38,
        &[(1, 10, 20, 100, 23), (2, HIDE, 0, 0, 0), (3, 122, 20, 100, 23)],
    ),
    (
        "buttons_all_hidden",
        0,
        &[(1, HIDE, 0, 0, 0), (2, HIDE, 0, 0, 0), (3, HIDE, 0, 0, 0)],
    ),
];

fn check_family(prefix: &str) {
    let mut seen = 0;
    for (name, height, calls) in EXPECTED.iter().filter(|e| e.0.starts_with(prefix)) {
        let (h, c) = scenario(name);
        assert_eq!((h, c.as_slice()), (*height, *calls), "{name}");
        seen += 1;
    }
    assert!(seen > 0);
}

// groups.rs:10,13,16 `add`/`sub`/`mul` -> 0/1/-1, groups.rs:67-80 `place_visible` of GroupElement (every + - * operator and constant).
#[test]
fn group_element_places_children_and_frame_exactly() {
    check_family("group");
}
// groups.rs:117-126 CheckElement::place_visible operators and hidden branch.
#[test]
fn check_element_places_exactly() {
    check_family("check");
}
// groups.rs RadioElement::place_visible operators, hint placement and hidden branches.
#[test]
fn radio_element_places_exactly() {
    check_family("radio");
}
// groups.rs CheckFlowElement::place_visible wrap logic (`>`, `&&`) and sums.
#[test]
fn check_flow_element_wraps_exactly() {
    check_family("flow");
}
// rows.rs RowElement::place_visible (combo drop height, width_dlu `>`, centering `/ 2`).
#[test]
fn row_element_places_exactly() {
    check_family("row");
}
// rows.rs SliderElement::place_visible (vw/sw clamp, value text width).
#[test]
fn slider_element_places_exactly() {
    check_family("slider");
}
// rows.rs TextElement::place_visible (aligned indent, empty and hidden early return).
#[test]
fn text_element_places_exactly() {
    check_family("text");
}
// rows.rs ButtonsElement::place_visible (wrap, hidden skip, `any`).
#[test]
fn buttons_element_places_exactly() {
    check_family("buttons");
}

// rows.rs `clamp`, `label_width` (`* 38 / 100`, dx(80)/dx(140) bounds) and `mul`.
#[test]
fn label_width_clamps_between_scaled_bounds() {
    let s = Fake::new();
    assert_eq!(label_width(&s, 100), 160);
    assert_eq!(label_width(&s, 600), 228);
    assert_eq!(label_width(&s, 1000), 280);
    assert_eq!(label_width(&s, 700), 266);
}

// groups.rs:28 `<Debug for GroupElement>::fmt -> Ok(Default::default())`, groups.rs:44 `title_id`, :49 `children`, `children_mut`.
#[test]
fn group_element_accessors_and_debug() {
    let mut g = GroupElement::new(
        7,
        vec![Box::new(CheckElement::new(2, 0)), Box::new(CheckElement::new(3, 0))],
    );
    let text = format!("{g:?}");
    assert!(
        text.contains("GroupElement") && text.contains("title_id: 7") && text.contains("children: 2"),
        "{text}"
    );
    assert_eq!(g.title_id(), 7);
    assert_eq!(g.children().len(), 2);
    assert_eq!(g.children_mut().len(), 2);
    assert!(g.visible());
    assert_eq!(g.control_ids(), vec![7, 2, 3]);
    assert!(g.as_group().is_some());
}

// groups.rs/rows.rs `control_ids` and `visible`/`set_visible` of every element.
#[test]
fn every_element_reports_ids_and_toggles_visibility() {
    let mut els: Vec<(Box<dyn SettingsLayoutElement>, Vec<i32>)> = vec![
        (Box::new(CheckElement::new(1, 0)), vec![1]),
        (Box::new(RadioElement::new(2, Some(3), 0)), vec![2, 3]),
        (Box::new(RadioElement::new(2, None, 0)), vec![2]),
        (Box::new(CheckFlowElement::new(vec![4, 5], 0)), vec![4, 5]),
        (Box::new(RowElement::new(6, 7, RowKind::Edit, 0)), vec![6, 7]),
        (Box::new(SliderElement::new(8, 9, 10)), vec![8, 9, 10]),
        (Box::new(TextElement::new(11, false)), vec![11]),
        (Box::new(ButtonsElement::new(vec![12, 13], false)), vec![12, 13]),
    ];
    for (e, ids) in &mut els {
        assert_eq!(&e.control_ids(), ids);
        assert!(e.visible());
        e.set_visible(false);
        assert!(!e.visible());
        let mut s = Fake::new();
        let (h, calls) = run(e.as_ref(), &mut s, 0, 0, 300);
        assert_eq!(h, 0);
        assert_eq!(calls.iter().map(|c| c.0).collect::<Vec<_>>(), *ids);
        assert!(calls.iter().all(|c| c.1 == HIDE));
        e.set_visible(true);
        assert!(e.visible());
    }
}
