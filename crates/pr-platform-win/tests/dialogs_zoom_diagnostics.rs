#[path = "common/dialogs_secondary_fixture.rs"]
mod fixture;

use fixture::FakeMeasure;
use fixture::LanguageGuard;
use pr_app::presenters::secondary::DiagnosticsDisplay;
use pr_platform_win::dialogs::native::dialog::DialogContent;
use pr_platform_win::dialogs::{diagnostics, zoom_prompt};

// C#: ZoomPromptDialogTests.ClientWidth_IsTwoHundredDialogUnits_OfTheBodyFont
#[test]
#[rustfmt::skip]
fn zoom_layout_matches_the_worked_example() {
    let mut m = FakeMeasure { avg_char_width: 6, line_height: 15, ..Default::default() };
    m.item_heights.insert(zoom_prompt::LABEL_ID, 30);
    m.item_heights.insert(zoom_prompt::ERROR_ID, 28);
    assert_eq!(zoom_prompt::preferred_client_size(&m, false), (300, 147));
    let p = zoom_prompt::layout_plan(&m, 300, false);
    assert_eq!(p.placements, vec![
        zoom_prompt::Placement::new(100, 18, 19, 264, 30),
        zoom_prompt::Placement::new(101, 18, 57, 264, 26),
    ]);
    assert_eq!(p.button_row, zoom_prompt::ButtonRowPlan { client_width: 300, right_margin: 18, top: 102, height: 26, ids: vec![2, 1] });
    assert_eq!(zoom_prompt::preferred_client_size(&m, true), (300, 183));
    assert_eq!(zoom_prompt::layout_plan(&m, 300, true).button_row.top, 138);
}

// C#: DiagnosticsAndListDialogTests.Diagnostics_ShowsTheFifteenLabelValuePairs_InTheOrderOfTheWpfWindow
#[test]
#[rustfmt::skip]
fn diagnostics_layout_matches_the_worked_example() {
    let mut m = FakeMeasure { avg_char_width: 6, line_height: 15, ..Default::default() };
    m.item_heights.insert(diagnostics::HEADER_ID, 20);
    m.item_heights.insert(diagnostics::NOTE_ID, 40);
    for r in 0..15 { m.item_heights.insert(diagnostics::FIRST_LABEL_ID + r, 15); m.item_heights.insert(diagnostics::FIRST_VALUE_ID + r, 30); }
    assert_eq!(diagnostics::preferred_client_size(&m), (600, 712));
    let p = diagnostics::layout_plan(&m, 600);
    assert_eq!(p.placements[0], zoom_prompt::Placement::new(200, 21, 23, 558, 20));
    assert_eq!(p.placements[1], zoom_prompt::Placement::new(201, 21, 49, 558, 40));
    assert_eq!(p.placements[2], zoom_prompt::Placement::new(210, 21, 108, 188, 30));
    assert_eq!(p.placements[3], zoom_prompt::Placement::new(240, 218, 108, 361, 30));
    assert_eq!(p.button_row.top, 663);
}

#[test]
fn diagnostics_rows_out_of_range_return_none() {
    assert_eq!(diagnostics::label_text(15), None);
}

#[test]
fn empty_diagnostic_value_does_not_pass_an_empty_buffer_to_user32() {
    // An empty diagnostics value is measured while WM_INITDIALOG is running.
    // DrawTextW with an empty Rust slice caused a USER32 access violation.
    let mut rect = pr_platform_win::win32::structs::Rect::default();
    assert_eq!(
        pr_platform_win::win32::dialog_native::user32::draw_text(0, "", &mut rect, 0),
        0
    );
    assert_eq!(rect, pr_platform_win::win32::structs::Rect::default());
}

#[test]
fn preferred_sizes_match_the_csharp_golden() {
    // C#: ZoomPromptDialogTests / DiagnosticsAndListDialogTests preferred sizes.
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/dialog-zoom-diagnostics.json")).unwrap();
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 33);
    let mut compared = 0;
    for case in cases.iter().filter(|c| c["kind"] == "preferred") {
        let m = FakeMeasure {
            avg_char_width: case["avg"].as_i64().unwrap() as i32,
            line_height: case["line"].as_i64().unwrap() as i32,
            ..Default::default()
        };
        let preferred = if case["type"] == "zoom" {
            let error_visible = case["error_visible"].as_bool().unwrap();
            zoom_prompt::preferred_client_size(&m, error_visible)
        } else {
            diagnostics::preferred_client_size(&m)
        };
        let expected_preferred = (
            case["preferred_width"].as_i64().unwrap() as i32,
            case["preferred_height"].as_i64().unwrap() as i32,
        );
        let expected_minimum = (
            case["minimum_width"].as_i64().unwrap() as i32,
            case["minimum_height"].as_i64().unwrap() as i32,
        );
        assert_eq!(preferred, expected_preferred, "{}", case["name"]);
        assert_eq!(preferred, expected_minimum, "{}", case["name"]);
        compared += 1;
    }
    assert_eq!(compared, 15);
}

fn display_from(values: &serde_json::Value) -> DiagnosticsDisplay {
    let v = |i: usize| values[i].as_str().unwrap().to_owned();
    DiagnosticsDisplay {
        cache_hits: v(0),
        cache_misses: v(1),
        hit_rate: v(2),
        source_bytes: v(3),
        source_reads: v(4),
        decode_time: v(5),
        present_latency: v(6),
        explorer_order: v(7),
        explorer_metadata: v(8),
        source_opens: v(9),
        stat_count: v(10),
        session_writes: v(11),
        decoder_fallbacks: v(12),
        present_histogram: v(13),
        cross_thread_presents: v(14),
    }
}

fn assert_items_match(items: &[pr_platform_win::dialogs::native::template::DialogItem], expected: &serde_json::Value) {
    let wanted = expected.as_array().unwrap();
    assert_eq!(items.len(), wanted.len());
    for (actual, expected) in items.iter().zip(wanted) {
        assert_eq!(format!("{:?}", actual.class), expected["class"].as_str().unwrap());
        assert_eq!(actual.id, expected["id"].as_i64().unwrap() as i32);
        assert_eq!(actual.text, expected["text"].as_str().unwrap());
        assert_eq!(actual.style, expected["style"].as_u64().unwrap() as u32);
        assert_eq!(actual.ex_style, expected["ex_style"].as_u64().unwrap() as u32);
    }
}

#[test]
fn dialog_rows_text_and_constants_match_the_csharp_golden() {
    // C#: ZoomPromptDialogTests and DiagnosticsAndListDialogTests oracle cases.
    let _language = LanguageGuard::english();
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/dialog-zoom-diagnostics.json")).unwrap();
    let mut compared = 0;
    for case in golden["cases"].as_array().unwrap() {
        match case["kind"].as_str().unwrap() {
            "zoom_dialog" => {
                let content = zoom_prompt::ZoomPromptContent::new(case["current"].as_i64().unwrap() as i32);
                assert_eq!(content.title(), case["title"].as_str().unwrap());
                assert_items_match(&content.items(), &case["items"]);
                assert_eq!(
                    content.initial_focus_id(),
                    case["initial_focus_id"].as_i64().unwrap() as i32
                );
                assert_eq!(
                    content.default_button_id(),
                    case["default_button_id"].as_i64().unwrap() as i32
                );
                assert_eq!(content.resizable(), case["resizable"].as_bool().unwrap());
                assert_eq!(content.uses_modal_frame(), case["uses_modal_frame"].as_bool().unwrap());
                assert_eq!(
                    content.extra_window_style(),
                    case["extra_window_style"].as_u64().unwrap() as u32
                );
                compared += 1;
            }
            "diagnostics_dialog" => {
                let content = diagnostics::DiagnosticsContent::new(display_from(&case["display"]), None);
                assert_eq!(content.title(), case["title"].as_str().unwrap());
                assert_items_match(&content.items(), &case["items"]);
                assert_eq!(
                    content.initial_focus_id(),
                    case["initial_focus_id"].as_i64().unwrap() as i32
                );
                assert_eq!(
                    content.default_button_id(),
                    case["default_button_id"].as_i64().unwrap() as i32
                );
                assert_eq!(content.header_control_ids(), vec![diagnostics::HEADER_ID]);
                assert_eq!(content.resizable(), case["resizable"].as_bool().unwrap());
                compared += 1;
            }
            "diagnostics_row" => {
                let row = case["row"].as_i64().unwrap();
                let display = display_from(&case["display"]);
                let actual = usize::try_from(row)
                    .ok()
                    .and_then(|r| diagnostics::label_text(r).zip(diagnostics::value_text(&display, r)));
                if case.get("threw").is_some() {
                    assert!(actual.is_none(), "{}", case["name"]);
                } else {
                    let (label, value) = actual.unwrap();
                    assert_eq!(label, case["label"].as_str().unwrap());
                    assert_eq!(value, case["value"].as_str().unwrap());
                }
                compared += 1;
            }
            "diagnostics_text" => {
                let display = display_from(&case["display"]);
                for row in 0..diagnostics::ROW_COUNT {
                    assert_eq!(diagnostics::label_text(row).as_deref(), case["labels"][row].as_str());
                    assert_eq!(diagnostics::value_text(&display, row), case["values"][row].as_str());
                }
                assert_eq!(
                    diagnostics::build_copy_text(&display),
                    case["copy_text"].as_str().unwrap()
                );
                compared += 1;
            }
            "constants" => {
                let ids = [
                    ("zoom_label_id", zoom_prompt::LABEL_ID),
                    ("zoom_edit_id", zoom_prompt::EDIT_ID),
                    ("zoom_error_id", zoom_prompt::ERROR_ID),
                    ("zoom_cancel_id", zoom_prompt::CANCEL_ID),
                    ("zoom_apply_id", zoom_prompt::APPLY_ID),
                    ("diagnostics_header_id", diagnostics::HEADER_ID),
                    ("diagnostics_note_id", diagnostics::NOTE_ID),
                    ("diagnostics_first_label_id", diagnostics::FIRST_LABEL_ID),
                    ("diagnostics_first_value_id", diagnostics::FIRST_VALUE_ID),
                    ("diagnostics_copy_id", diagnostics::COPY_ID),
                    ("diagnostics_close_id", diagnostics::CLOSE_ID),
                    ("diagnostics_row_count", diagnostics::ROW_COUNT as i32),
                ];
                for (key, actual) in ids {
                    assert_eq!(actual, case[key].as_i64().unwrap() as i32, "{key}");
                }
                compared += 1;
            }
            _ => {}
        }
    }
    assert_eq!(compared, 18);
}
