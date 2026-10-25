//! C# RecoveryDialog layout golden and stable-selection regressions (W8-18t).
use std::collections::HashSet;

#[path = "common/dialogs_recovery_fixture.rs"]
mod fixture;

use pr_app::presenters::recovery_filter::{RecoveryFilter, RecoveryTone};
use pr_platform_win::dialogs::recovery::{layout::*, list::*, restore::*};

fn number(value: &serde_json::Value, key: &str) -> i32 {
    value[key].as_i64().unwrap() as i32
}

// C#: RecoveryDialog.BuildItems / PreferredClientSize / MinimumClientSize and ToColorRef oracle.
#[test]
fn every_recovery_layout_case_matches_the_csharp_golden() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/recovery-dialog-layout.json")).unwrap();
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 17);
    let texts = fixture::texts();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        match case["kind"].as_str().unwrap() {
            "constants" => {
                let names = [
                    "header_id",
                    "summary_id",
                    "filter_label_id",
                    "filter_id",
                    "recheck_id",
                    "check_status_id",
                    "list_id",
                    "filter_empty_id",
                    "details_empty_id",
                    "verdict_label_id",
                    "verdict_badge_id",
                    "error_id",
                    "source_title_id",
                    "source_status_id",
                    "source_path_id",
                    "copy_source_id",
                    "show_source_id",
                    "destination_title_id",
                    "destination_status_id",
                    "destination_path_id",
                    "copy_destination_id",
                    "show_destination_id",
                    "no_destination_id",
                    "details_id",
                    "clear_selected_id",
                    "clear_all_id",
                    "retry_id",
                    "close_id",
                ];
                assert_eq!(case["ids"].as_object().unwrap().len(), ALL_CONTROL_IDS.len());
                for (key, id) in names.iter().zip(ALL_CONTROL_IDS) {
                    assert_eq!(id, number(&case["ids"], key), "{name}: {key}");
                }
                assert_eq!(SS_CENTER as i32, number(case, "ss_center"));
                assert_eq!(CURSOR_ARROW, number(case, "cursor_arrow"));
                assert_eq!(CURSOR_WAIT, number(case, "cursor_wait"));
            }
            "items" => {
                let items = build_items(&texts);
                let expected = case["items"].as_array().unwrap();
                assert_eq!(items.len(), expected.len());
                for (actual, expected) in items.iter().zip(expected) {
                    assert_eq!(
                        format!("{:?}", actual.class),
                        expected["class"].as_str().unwrap(),
                        "{name}"
                    );
                    assert_eq!(actual.id, number(expected, "id"), "{name}");
                    assert_eq!(
                        actual.text,
                        expected["text"].as_str().unwrap(),
                        "{name}: id {}",
                        actual.id
                    );
                    assert_eq!(
                        actual.style,
                        expected["style"].as_u64().unwrap() as u32,
                        "{name}: id {}",
                        actual.id
                    );
                    assert_eq!(
                        actual.ex_style,
                        expected["ex_style"].as_u64().unwrap() as u32,
                        "{name}: id {}",
                        actual.id
                    );
                }
            }
            "color_ref" => {
                let tone = match case["tone"].as_str().unwrap() {
                    "Neutral" => RecoveryTone::Neutral,
                    "Good" => RecoveryTone::Good,
                    "Info" => RecoveryTone::Info,
                    "Warn" => RecoveryTone::Warn,
                    "Bad" => RecoveryTone::Bad,
                    other => panic!("unknown tone {other}"),
                };
                assert_eq!(to_color_ref(tone), case["color_ref"].as_u64().unwrap() as u32, "{name}");
            }
            "sizes" => {
                let m = fixture::Metrics {
                    avg: number(case, "avg"),
                    line: number(case, "line"),
                };
                assert_eq!(margin(&m), number(case, "margin"), "{name}");
                assert_eq!(field_height(&m), number(case, "field_height"), "{name}");
                assert_eq!(button_height(&m), number(case, "button_height"), "{name}");
                assert_eq!(
                    preferred_client_size(&m),
                    (number(case, "preferred_w"), number(case, "preferred_h")),
                    "{name}"
                );
                assert_eq!(
                    minimum_client_size(&m),
                    (number(case, "minimum_w"), number(case, "minimum_h")),
                    "{name}"
                );
            }
            other => panic!("unhandled golden kind {other}"),
        }
    }
}

// C#: RecoveryDialogTests.ClearSelected_RemovesRowsWithoutSelectingNeighbours
#[test]
fn selection_after_a_removal_does_not_move_to_the_neighbours() {
    let selected = HashSet::from(["removed".to_owned(), "kept".to_owned()]);
    let remaining = vec!["next-to-removed".to_owned(), "kept".to_owned(), "other".to_owned()];
    assert_eq!(carried_selection(&selected, &remaining), vec![1]);
    assert!(carried_selection(&HashSet::from(["removed".to_owned()]), &remaining).is_empty());
}

// C#: RecoveryDialog.RefreshEntries preserves scrolling only for an existing positive index.
#[test]
fn top_index_and_filters_follow_the_csharp_boundaries() {
    for top in [-2, -1, 0, 3, 4, i32::MAX] {
        assert_eq!(top_index_to_restore(top, 3), None);
    }
    assert_eq!(top_index_to_restore(1, 3), Some(1));
    assert_eq!(top_index_to_restore(2, 3), Some(2));
    assert_eq!(top_index_to_restore(1, 0), None);
    for (index, expected) in [
        (-1, RecoveryFilter::All),
        (0, RecoveryFilter::All),
        (1, RecoveryFilter::CanRetry),
        (2, RecoveryFilter::AlreadyDone),
        (3, RecoveryFilter::Problems),
        (4, RecoveryFilter::All),
    ] {
        assert_eq!(filter_from_index(index), expected);
    }
}
