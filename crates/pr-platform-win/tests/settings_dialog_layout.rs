//! C# SettingsDialog layout/scroll oracle and exact worked placement checks (W8-19m).
use pr_platform_win::dialogs::settings::dialog::{footer::*, page_layout::*, surface::*};
use pr_platform_win::dialogs::zoom_prompt::{DialogMeasure, Placement};
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;

#[test]
fn on_closed_calls_each_page_once_even_when_cleanup_is_requested_twice() {
    use pr_platform_win::dialogs::native::dialog::core::DialogContent;
    use pr_platform_win::dialogs::settings::dialog::SettingsDialog;
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let keys = [
        "General",
        "Display",
        "Mouse",
        "Performance",
        "Shortcuts",
        "Files",
        "Diagnostics",
    ];
    let mut args = fixture::args();
    args.options.pages = Some(
        keys.iter()
            .map(|&key| {
                Box::new(fixture::ProbePage {
                    key,
                    events: events.clone(),
                }) as Box<dyn pr_platform_win::dialogs::settings::page_view::SettingsPageView>
            })
            .collect(),
    );
    let d = SettingsDialog::new(args).unwrap();
    events.borrow_mut().clear();
    DialogContent::on_closed(d.content().as_ref(), d.native());
    DialogContent::on_closed(d.content().as_ref(), d.native());
    assert_eq!(
        *events.borrow(),
        keys.iter().map(|key| format!("close:{key}")).collect::<Vec<_>>()
    );
}

struct Metrics {
    avg: i32,
    line: i32,
}
impl DialogMeasure for Metrics {
    fn dx(&self, n: i32) -> i32 {
        (n * self.avg + 2) / 4
    }
    fn dy(&self, n: i32) -> i32 {
        (n * self.line + 4) / 8
    }
    fn line_height(&self) -> i32 {
        self.line
    }
    fn measure_item_height(&self, _: i32, _: i32) -> i32 {
        0
    }
    fn measure_text_width(&self, _: &str) -> i32 {
        0
    }
    fn measure_text_height(&self, _: &str, _: i32) -> i32 {
        0
    }
    fn item_text(&self, _: i32) -> String {
        String::new()
    }
    fn button_width(&self, _: &[i32]) -> i32 {
        panic!("Settings measures each button")
    }
    fn button_row_width(&self, _: &[i32]) -> i32 {
        panic!("Settings measures each button")
    }
}
fn n(v: &serde_json::Value, k: &str) -> i32 {
    v[k].as_i64().unwrap() as i32
}
#[test]
fn every_layout_and_scroll_case_matches_the_csharp_golden() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/settings-dialog-layout.json")).unwrap();
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 36);
    for row in cases {
        match row["kind"].as_str().unwrap() {
            "metrics" => {
                let m = Metrics {
                    avg: n(row, "avg"),
                    line: n(row, "line"),
                };
                assert_eq!(
                    [margin(&m), field_height(&m), button_height(&m), scroll_bar_width(&m)],
                    [
                        n(row, "margin"),
                        n(row, "field_height"),
                        n(row, "button_height"),
                        n(row, "scroll_bar_width")
                    ],
                    "{}",
                    row["name"]
                );
            }
            "sizes" => {
                let m = Metrics {
                    avg: n(row, "avg"),
                    line: n(row, "line"),
                };
                assert_eq!(
                    preferred_client_size(&m),
                    (n(row, "preferred_width"), n(row, "preferred_height"))
                );
                assert_eq!(
                    minimum_client_size(&m),
                    (n(row, "minimum_width"), n(row, "minimum_height"))
                );
            }
            "layout" => {
                let m = Metrics {
                    avg: n(row, "avg"),
                    line: n(row, "line"),
                };
                let plan = layout_plan(&m, n(row, "width"), n(row, "height"), 7);
                assert_eq!(
                    (plan.viewport_width, plan.viewport_height),
                    (n(row, "viewport_width"), n(row, "viewport_height")),
                    "{}",
                    row["name"]
                );
            }
            "scroll" => {
                let mut offset = n(row, "start");
                let ops = row["ops"].as_array().unwrap();
                let expected = row["offsets"].as_array().unwrap();
                assert_eq!(ops.len(), expected.len());
                for (op, expected) in ops.iter().zip(expected) {
                    let target = match op["op"].as_str().unwrap() {
                        "set" => n(op, "value"),
                        "wheel" => wheel_target(offset, n(op, "delta"), n(row, "line")),
                        "bar" => bar_target(
                            n(op, "code"),
                            offset,
                            bar_line(n(row, "line")),
                            n(row, "viewport"),
                            op["track"].as_i64().unwrap_or(0) as i32,
                        ),
                        other => panic!("unhandled scroll op {other}"),
                    };
                    offset = clamp_scroll(target, n(row, "content"), n(row, "viewport"));
                    assert_eq!(offset, expected.as_i64().unwrap() as i32, "{} op {op}", row["name"]);
                }
            }
            other => panic!("unhandled golden kind {other}"),
        }
    }
}
#[test]
fn layout_plan_preserves_csharp_place_order_and_two_button_rows() {
    let m = Metrics { avg: 8, line: 16 };
    let p = layout_plan(&m, 900, 600, 1);
    assert_eq!(
        p.placements,
        vec![
            Placement::new(2, 556, 552, 100, 28),
            Placement::new(120, 668, 552, 100, 28),
            Placement::new(1, 780, 552, 100, 28),
            Placement::new(121, 20, 552, 100, 28),
            Placement::new(122, 132, 552, 100, 28),
            Placement::new(123, 244, 552, 100, 28),
            Placement::new(124, 356, 552, 100, 28),
            Placement::new(100, 20, 20, 225, 0),
            Placement::new(101, 20, 32, 225, 504),
            Placement::new(102, 20, 536, 225, 0),
            Placement::new(103, 265, 20, 615, 0),
            Placement::new(110, 265, 32, 587, 504),
            Placement::new(104, 860, 32, 20, 504),
        ]
    );
    let p = layout_plan(&m, 700, 500, 1);
    assert_eq!(p.placements[0], Placement::new(2, 356, 452, 100, 28));
    assert_eq!(p.placements[3], Placement::new(121, 20, 416, 100, 28));
    assert_eq!(p.placements[8], Placement::new(101, 20, 32, 180, 368));
    assert_eq!((p.viewport_width, p.viewport_height), (432, 368));
}
#[test]
fn scroll_rounding_boundaries_and_surface_cache_match_csharp() {
    assert_eq!(wheel_target(100, 20, 1), 100); // 0.5 rounds to even 0.
    assert_eq!(wheel_target(100, 60, 1), 98); // 1.5 rounds to even 2.
    assert_eq!(wheel_target(100, -60, 1), 102);
    assert_eq!(ensure_visible_target(-1, 20, 100, 50, 8), Some(91));
    assert_eq!(ensure_visible_target(1, 51, 100, 50, 8), Some(109));
    assert_eq!(ensure_visible_target(0, 50, 100, 50, 8), None);
    assert_eq!(scroll_bar_info(50, 50, 0), None);
    assert_eq!(
        scroll_bar_info(51, 50, 1),
        Some(ScrollBarInfo {
            min: 0,
            max: 50,
            page: 50,
            pos: 1
        })
    );
    assert_eq!(static_text_kind(true, true), StaticTextKind::Muted);
    assert_eq!(static_text_kind(false, true), StaticTextKind::Warning);
    assert_eq!(static_text_kind(false, false), StaticTextKind::Normal);
    assert_eq!(combo_drop_height(0, 0, 16, 4, 8), 28);
    assert_eq!(combo_drop_height(100, 10, 16, 4, 8), 128);
    assert_eq!(placement_change(Some((1, 2, 0, 0)), 1, 2, -5, -2), None);
    assert_eq!(placement_change(None, 1, 2, -5, -2), Some((1, 2, 0, 0)));
}
