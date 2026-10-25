use std::rc::Rc;
use std::sync::Arc;

#[path = "common/dialogs_secondary_fixture.rs"]
mod fixture;

use pr_app::presenters::benchmark::BenchmarkSessionFactory;
use pr_app::presenters::benchmark_presentation::BenchmarkProfileItem;
use pr_bench::profiles;
use pr_l10n::localizer::Localizer;
use pr_platform_win::dialogs::benchmark::results::{column_click_sub_item, profile_list_text, row_color_key};
use pr_platform_win::dialogs::benchmark::run::{enabled_flags, focus_target, status_line_height};
use pr_platform_win::dialogs::benchmark::state::{BenchmarkContent, BenchmarkOptions};
use pr_platform_win::dialogs::native::chrome::DialogColors;
use pr_platform_win::dialogs::native::dialog::{DialogContent, NativeDialog};
use pr_platform_win::dialogs::zoom_prompt::DialogMeasure;

#[test]
fn row_color_key_table() {
    assert_eq!(row_color_key(true, false, 0), "Dark.SuccessBackground");
    assert_eq!(row_color_key(false, true, 0), "Dark.ErrorBackground");
    assert_eq!(row_color_key(false, false, 0), "Dark.Panel");
    assert_eq!(row_color_key(false, false, 1), "Dark.RowAlt");
    assert_eq!(row_color_key(true, true, 1), "Dark.SuccessBackground");
}

// C#: BenchmarkDialog golden cases from W8-18s oracle.
#[test]
fn every_benchmark_results_case_matches_the_csharp_golden() {
    let _language = fixture::LanguageGuard::english();
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/dialog-benchmark-results.json")).unwrap();
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 30);
    let texts = Localizer::current();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        match case["kind"].as_str().unwrap() {
            "palette" => {
                assert_eq!(
                    DialogColors::from_key(case["key"].as_str().unwrap()),
                    Some(case["value"].as_u64().unwrap() as u32),
                    "{name}"
                );
            }
            "profile_list" => {
                let items: Vec<_> = profiles::all().iter().cloned().map(BenchmarkProfileItem::new).collect();
                let expected = case["items"].as_array().unwrap();
                assert_eq!(items.len(), expected.len(), "{name}");
                for (index, expected) in expected.iter().enumerate() {
                    assert_eq!(items[index].profile.id, expected["id"].as_str().unwrap(), "{name}");
                    let actual = profile_list_text(&items, index as i32, &texts).unwrap();
                    assert_eq!(actual.text, expected["text"].as_str().unwrap(), "{name}");
                    assert_eq!(actual.badge.as_deref(), expected["badge"].as_str(), "{name}");
                }
                assert!(profile_list_text(&items, -1, &texts).is_none(), "{name}");
                assert!(
                    profile_list_text(&items, items.len() as i32, &texts).is_none(),
                    "{name}"
                );
            }
            "row_color" => {
                let displayed = case["displayed_row"].as_i64().unwrap() as i32;
                let source = case["source"].as_i64().unwrap();
                let row = usize::try_from(source)
                    .ok()
                    .and_then(|i| case["rows"].as_array().unwrap().get(i));
                let is_best = row.and_then(|r| r["is_best"].as_bool()).unwrap_or(false);
                let is_fail = row.is_some_and(|r| r["status"] == "Fail");
                let key = row_color_key(is_best, is_fail, displayed);
                assert_eq!(
                    DialogColors::from_key(key),
                    Some(case["back"].as_u64().unwrap() as u32),
                    "{name}"
                );
                assert_eq!(DialogColors::text(), case["text"].as_u64().unwrap() as u32, "{name}");
            }
            "static_color" => {
                let factory: BenchmarkSessionFactory = Arc::new(|_, _| panic!("factory is not used by static colour"));
                let mut options = BenchmarkOptions {
                    pictures_folder: Some("pictures-fixture".to_owned()),
                    ..Default::default()
                };
                let content = Rc::new(BenchmarkContent::new(None, factory, &mut options));
                let dialog = NativeDialog::new(content.clone());
                assert_eq!(
                    content.static_text_color(&dialog, case["id"].as_i64().unwrap() as i32),
                    case["value"].as_u64().unwrap() as u32,
                    "{name}"
                );
            }
            kind => panic!("unhandled golden case: {kind}"),
        }
    }
}

#[test]
fn column_click_sub_item_of_zero_is_none() {
    assert_eq!(column_click_sub_item(0), None);
}

#[test]
fn nmlistview_isubitem_is_at_offset_28() {
    #[repr(C)]
    struct NmlistviewLayout {
        hwnd_from: isize,
        id_from: usize,
        code: u32,
        padding: u32,
        item: i32,
        sub_item: i32,
    }
    assert_eq!(std::mem::offset_of!(NmlistviewLayout, sub_item), 28);
}

#[test]
fn enabled_flags_follow_the_csharp_order_and_polarity() {
    assert_eq!(
        enabled_flags(true),
        [
            (110, false),
            (111, false),
            (112, true),
            (103, false),
            (109, false),
            (105, false),
            (102, false)
        ]
    );
    assert_eq!(
        enabled_flags(false),
        [
            (110, true),
            (111, true),
            (112, false),
            (103, true),
            (109, true),
            (105, true),
            (102, true)
        ]
    );
}

#[test]
fn focus_target_rescues_a_disabled_focus() {
    assert_eq!(focus_target(true, true), Some(112));
    assert_eq!(focus_target(false, true), Some(110));
    assert_eq!(focus_target(true, false), None);
    assert_eq!(focus_target(false, false), None);
}

struct Measure(std::cell::Cell<i32>);
impl DialogMeasure for Measure {
    fn dx(&self, v: i32) -> i32 {
        v
    }
    fn dy(&self, v: i32) -> i32 {
        v
    }
    fn line_height(&self) -> i32 {
        1
    }
    fn measure_item_height(&self, _: i32, _: i32) -> i32 {
        0
    }
    fn measure_text_width(&self, _: &str) -> i32 {
        0
    }
    fn measure_text_height(&self, _: &str, width: i32) -> i32 {
        self.0.set(width);
        7
    }
    fn item_text(&self, _: i32) -> String {
        String::new()
    }
    fn button_width(&self, _: &[i32]) -> i32 {
        0
    }
    fn button_row_width(&self, _: &[i32]) -> i32 {
        0
    }
}

#[test]
fn status_line_height_is_zero_for_empty_text_and_uses_the_client_width_minus_two_margins() {
    let m = Measure(std::cell::Cell::new(-1));
    assert_eq!(status_line_height(&m, "", 100, 10), 0);
    assert_eq!(m.0.get(), -1);
    assert_eq!(status_line_height(&m, "status", 100, 10), 7);
    assert_eq!(m.0.get(), 80);
    let _ = status_line_height(&m, "status", 1, 10);
    assert_eq!(m.0.get(), 1);
}
