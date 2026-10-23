use std::cmp::Ordering;

use pr_l10n::localizer::Localizer;
use pr_platform_win::dialogs::benchmark::layout::*;
use pr_platform_win::dialogs::benchmark::state::{
    column_headers, compare_f64_dotnet, compare_rows, culture_compare, display_order, next_sort,
};
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
