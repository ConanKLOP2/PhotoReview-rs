use std::cmp::Ordering;
use std::sync::Arc;

#[path = "common/dialogs_secondary_fixture.rs"]
mod fixture;

use pr_app::presenters::benchmark::BenchmarkSessionFactory;
use pr_app::presenters::benchmark_presentation::BenchmarkResultRow;
use pr_bench::models::{BenchmarkPhaseResult, BenchmarkResultStatus, BenchmarkWorkload};
use pr_bench::profiles;
use pr_l10n::localizer::Localizer;
use pr_platform_win::dialogs::benchmark::layout::*;
use pr_platform_win::dialogs::benchmark::state::{BenchmarkContent, BenchmarkOptions};
use pr_platform_win::dialogs::benchmark::state::{
    column_headers, compare_f64_dotnet, compare_rows, culture_compare, display_order, next_sort,
};
use pr_platform_win::dialogs::native::dialog::DialogContent;
use pr_platform_win::dialogs::zoom_prompt::{ButtonRowPlan, DialogMeasure};

#[rustfmt::skip]
struct Measure { status: String }
#[rustfmt::skip]
impl DialogMeasure for Measure {
    fn dx(&self, u: i32) -> i32 { (u * 6 + 2) / 4 }
    fn dy(&self, u: i32) -> i32 { (u * 15 + 4) / 8 }
    fn line_height(&self) -> i32 { 15 }
    fn measure_item_height(&self, id: i32, _width: i32) -> i32 {
        match id { HEADER_ID => 20, DESCRIPTION_ID => 30, PROFILES_LABEL_ID => 18, STATUS_ID if !self.status.is_empty() => 40, _ => 0 }
    }
    fn measure_text_width(&self, _text: &str) -> i32 { 60 }
    fn measure_text_height(&self, text: &str, _width: i32) -> i32 { if text.is_empty() { 0 } else { 40 } }
    fn item_text(&self, id: i32) -> String { if id == LIMIT_LABEL_ID { "Image limit".into() } else if id == STATUS_ID { self.status.clone() } else { String::new() } }
    fn button_width(&self, _ids: &[i32]) -> i32 { 75 }
    fn button_row_width(&self, ids: &[i32]) -> i32 { ids.len() as i32 * 75 }
}

#[test]
#[rustfmt::skip]
fn layout_plan_matches_the_worked_example() {
    let m = Measure { status: String::new() };
    assert_eq!(preferred_client_size(&m), (780, 619));
    assert_eq!(minimum_client_size(&m), (600, 525));
    assert_eq!(margin(&m), 15);
    assert_eq!(field_height(&m), 26);
    let p = layout_plan(&m, 780, 619);
    assert_eq!(p.profiles_item_height, 24);
    assert_eq!(p.status_height, 0);
    assert_eq!(p.button_row, ButtonRowPlan { client_width: 780, right_margin: 15, top: 578, height: 26, ids: vec![OPEN_REPORT_ID, OPEN_FOLDER_ID, CLOSE_ID] });
    assert_eq!(p.placements.last().map(|x| (x.id, x.x, x.y, x.width, x.height)), Some((RESULTS_ID, 15, 390, 750, 173)));
    assert_eq!(p.column_widths, [24, 178, 81, 93, 48, 54, 54, 54, 54, 89]);
}

#[test]
#[rustfmt::skip]
fn layout_plan_with_status_and_tiny_client_respects_contract_minimums() {
    let m = Measure { status: "wrapped status".into() };
    let normal = layout_plan(&m, 780, 619);
    assert_eq!(normal.status_height, 40);
    assert_eq!(normal.placements.last().map(|p| p.y), Some(430));
    let tiny = layout_plan(&m, 1, 1);
    assert!(tiny.placements.last().is_some_and(|p| p.height >= m.dy(30)));
    assert!(tiny.placements.iter().find(|p| p.id == FOLDER_ID).is_some_and(|p| p.width >= m.dx(40)));
}

#[test]
#[rustfmt::skip]
fn sort_and_numeric_helpers_follow_dotnet_and_csharp_rules() {
    assert_eq!(display_order(4, -1, 1, &|a,b| a.cmp(&b)), vec![0,1,2,3]);
    assert_eq!(display_order(4, 1, 1, &|a,b| (a % 2).cmp(&(b % 2))), vec![0,2,1,3]);
    assert_eq!(display_order(4, 1, 2, &|a,b| (a % 2).cmp(&(b % 2))), vec![3,1,2,0]);
    assert_eq!(next_sort((-1, 0), 6), (6, 1));
    assert_eq!(next_sort((6, 1), 6), (6, 2));
    assert_eq!(next_sort((6, 2), 6), (6, 1));
    assert_eq!(compare_f64_dotnet(f64::NAN, -1.0), Ordering::Less);
    assert_eq!(compare_f64_dotnet(f64::NAN, f64::NAN), Ordering::Equal);
    assert_eq!(compare_f64_dotnet(-0.0, 0.0), Ordering::Equal);
}

#[test]
fn language_headers_and_culture_compare_are_available() {
    let headers = column_headers(&Localizer::current());
    assert_eq!(headers.len(), COLUMN_COUNT);
    assert_eq!(culture_compare("Alpha", "Beta"), Ordering::Less);
    let _ = compare_rows;
}

fn golden() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../tests/golden/dialog-benchmark-layout.json")).unwrap()
}

fn number(case: &serde_json::Value, key: &str) -> i32 {
    case[key].as_i64().unwrap() as i32
}

fn row(value: &serde_json::Value) -> BenchmarkResultRow {
    let id = value["profile_id"].as_str().unwrap();
    let profile = profiles::find(id).unwrap().clone();
    let workload = BenchmarkWorkload::from_name(value["workload"].as_str().unwrap()).unwrap();
    let status = BenchmarkResultStatus::from_name(value["status"].as_str().unwrap()).unwrap();
    let samples = value["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_f64().unwrap())
        .collect();
    let mut result = BenchmarkResultRow::new(
        profile,
        BenchmarkPhaseResult::new(id, workload, samples, status, None, None),
        None,
    );
    result.is_best = value["is_best"].as_bool().unwrap_or(false);
    result
}

// C#: BenchmarkDialog golden cases from W8-18r oracle.
#[test]
fn every_benchmark_layout_case_matches_the_csharp_golden() {
    let _language = fixture::LanguageGuard::english();
    let golden = golden();
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 45);
    let texts = Localizer::current();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        match case["kind"].as_str().unwrap() {
            "dialog" => {
                let factory: BenchmarkSessionFactory = Arc::new(|_, _| panic!("factory is not used by layout"));
                let mut options = BenchmarkOptions {
                    pictures_folder: Some("pictures-fixture".to_owned()),
                    ..Default::default()
                };
                let content = BenchmarkContent::new(case["initial_folder"].as_str(), factory, &mut options);
                assert_eq!(content.title(), case["title"].as_str().unwrap(), "{name}");
                assert_eq!(content.resizable(), case["resizable"].as_bool().unwrap(), "{name}");
                assert_eq!(
                    content.uses_modal_frame(),
                    case["uses_modal_frame"].as_bool().unwrap(),
                    "{name}"
                );
                assert_eq!(
                    content.extra_window_style(),
                    case["extra_window_style"].as_u64().unwrap() as u32,
                    "{name}"
                );
                assert_eq!(content.initial_focus_id(), number(case, "initial_focus_id"), "{name}");
                assert_eq!(content.default_button_id(), number(case, "default_button_id"), "{name}");
                assert_eq!(content.header_control_ids(), vec![HEADER_ID], "{name}");
                let items = content.items();
                let expected = case["items"].as_array().unwrap();
                assert_eq!(items.len(), expected.len(), "{name}");
                for (actual, expected) in items.iter().zip(expected) {
                    assert_eq!(
                        format!("{:?}", actual.class),
                        expected["class"].as_str().unwrap(),
                        "{name}"
                    );
                    assert_eq!(actual.id, number(expected, "id"), "{name}");
                    assert_eq!(actual.text, expected["text"].as_str().unwrap(), "{name}");
                    assert_eq!(actual.style, expected["style"].as_u64().unwrap() as u32, "{name}");
                    assert_eq!(actual.ex_style, expected["ex_style"].as_u64().unwrap() as u32, "{name}");
                }
            }
            "column_headers" => {
                let actual = column_headers(&texts);
                let expected = case["headers"].as_array().unwrap();
                assert_eq!(actual.len(), expected.len(), "{name}");
                for (actual, expected) in actual.iter().zip(expected) {
                    assert_eq!(actual, expected.as_str().unwrap(), "{name}");
                }
            }
            "compare_rows" => {
                let left = row(&case["left"]);
                let right = row(&case["right"]);
                let actual = compare_rows(&left, &right, number(case, "column"), &texts, &|a, b| a.cmp(b));
                assert_eq!(actual as i32, number(case, "sign"), "{name}");
            }
            "sort_clicks" => {
                let mut state = (-1, 0);
                for (click, expected) in case["clicks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(case["states"].as_array().unwrap())
                {
                    state = next_sort(state, click.as_i64().unwrap() as i32);
                    assert_eq!(
                        state,
                        (
                            expected[0].as_i64().unwrap() as i32,
                            expected[1].as_i64().unwrap() as i32
                        ),
                        "{name}"
                    );
                }
            }
            "preferred" | "metrics" => {
                let m = fixture::FakeMeasure {
                    avg_char_width: number(case, "avg"),
                    line_height: number(case, "line"),
                    ..Default::default()
                };
                if case["kind"] == "preferred" {
                    assert_eq!(
                        preferred_client_size(&m),
                        (number(case, "preferred_width"), number(case, "preferred_height")),
                        "{name}"
                    );
                    assert_eq!(
                        minimum_client_size(&m),
                        (number(case, "minimum_width"), number(case, "minimum_height")),
                        "{name}"
                    );
                } else {
                    assert_eq!(margin(&m), number(case, "margin"), "{name}");
                    assert_eq!(field_height(&m), number(case, "field_height"), "{name}");
                    assert_eq!(field_height(&m), number(case, "button_height"), "{name}");
                }
            }
            "constants" => {
                for (key, actual) in [
                    ("header_id", HEADER_ID),
                    ("description_id", DESCRIPTION_ID),
                    ("folder_id", FOLDER_ID),
                    ("browse_id", BROWSE_ID),
                    ("limit_label_id", LIMIT_LABEL_ID),
                    ("limit_id", LIMIT_ID),
                    ("profiles_label_id", PROFILES_LABEL_ID),
                    ("select_all_id", SELECT_ALL_ID),
                    ("select_none_id", SELECT_NONE_ID),
                    ("profiles_id", PROFILES_ID),
                    ("run_id", RUN_ID),
                    ("quick_check_id", QUICK_CHECK_ID),
                    ("cancel_run_id", CANCEL_RUN_ID),
                    ("progress_id", PROGRESS_ID),
                    ("status_id", STATUS_ID),
                    ("results_id", RESULTS_ID),
                    ("open_report_id", OPEN_REPORT_ID),
                    ("open_folder_id", OPEN_FOLDER_ID),
                    ("close_id", CLOSE_ID),
                ] {
                    assert_eq!(actual, number(case, key), "{name}: {key}");
                }
                assert_eq!(COLUMN_COUNT as i32, number(case, "column_count"), "{name}");
            }
            kind => panic!("unhandled golden case: {kind}"),
        }
    }
}
