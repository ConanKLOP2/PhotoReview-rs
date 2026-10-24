use pr_core::model::enums::FileOperationType;
use pr_core::settings::shortcuts::ReviewAction;
use pr_platform_win::dialogs::action_profiles as ap;
use pr_platform_win::dialogs::action_profiles::{
    APPLY_ID, CANCEL_ID, DETAILS_HEADER_ID, FileFilterText, LIST_HEADER_ID, field_height, layout_plan, left_width,
    margin, minimum_client_size, preferred_client_size,
};
use pr_platform_win::dialogs::native::dialog::DialogContent;
use pr_platform_win::dialogs::zoom_prompt::Placement;

#[path = "common/dialogs_secondary_fixture.rs"]
mod dialogs_secondary_fixture;
use dialogs_secondary_fixture::{FakeMeasure, LanguageGuard};

fn measure() -> FakeMeasure {
    let mut m = FakeMeasure {
        avg_char_width: 6,
        line_height: 15,
        default_item_height: 18,
        text_height: 32,
        button_width_override: Some(75),
        ..Default::default()
    };
    m.item_heights.insert(LIST_HEADER_ID, 20);
    m.item_heights.insert(DETAILS_HEADER_ID, 20);
    m
}

#[test]
fn file_filter_parse_matches_the_csharp_golden() {
    // C#: ActionProfilesDialogTests.FileFilterText_Parse
    let cases = [
        ("", vec![]),
        ("A", vec![]),
        ("A|*.a", vec![("A", "*.a")]),
        ("A|*.a|B|*.b;*.c", vec![("A", "*.a"), ("B", "*.b;*.c")]),
        ("A|*.a|B", vec![("A", "*.a")]),
        ("|", vec![("", "")]),
        ("||", vec![("", "")]),
        ("A|*.a|", vec![("A", "*.a")]),
        (" A | *.a ", vec![(" A ", " *.a ")]),
        ("JSON (*.json)|*.json", vec![("JSON (*.json)", "*.json")]),
        (
            "JSON (*.json)|*.json|All files (*.*)|*.*",
            vec![("JSON (*.json)", "*.json"), ("All files (*.*)", "*.*")],
        ),
        (
            "Executable (*.exe)|*.exe|All files (*.*)|*.*",
            vec![("Executable (*.exe)", "*.exe"), ("All files (*.*)", "*.*")],
        ),
        ("A|B|C|D|E|F", vec![("A", "B"), ("C", "D"), ("E", "F")]),
        ("A||B|", vec![("A", ""), ("B", "")]),
    ];
    for (input, expected) in cases {
        let actual = FileFilterText::parse(input);
        let pairs: Vec<_> = actual.iter().map(|f| (f.name.as_str(), f.pattern.as_str())).collect();
        assert_eq!(pairs, expected, "{input:?}");
    }
}

#[test]
fn layout_plan_matches_the_worked_example() {
    // C#: ActionProfilesDialogTests.Layout_DoesNotOverlapOrCutAnything_AtOneAndOneAndAHalfScale
    // Geometry mirrors ActionProfilesDialog.LayoutControls in the C# oracle.
    let m = measure();
    let plan = layout_plan(&m, 660, 431);
    assert_eq!(preferred_client_size(&m), (660, 431));
    assert_eq!(minimum_client_size(&m), (510, 394));
    assert_eq!(margin(&m), 15);
    assert_eq!(field_height(&m), 26);
    assert_eq!(left_width(&m, 660), 220);
    assert_eq!(plan.list_item_height, 24);
    assert_eq!(plan.button_row.ids, [CANCEL_ID, APPLY_ID]);
    assert_eq!(plan.placements.len(), 16);
    assert_eq!(
        plan.placements,
        vec![
            Placement::new(100, 15, 15, 220, 20),
            Placement::new(101, 15, 43, 220, 264),
            Placement::new(102, 15, 315, 107, 26),
            Placement::new(103, 128, 315, 107, 26),
            Placement::new(104, 15, 349, 107, 26),
            Placement::new(105, 128, 349, 107, 26),
            Placement::new(106, 250, 15, 395, 20),
            Placement::new(110, 250, 50, 395, 18),
            Placement::new(111, 250, 72, 395, 26),
            Placement::new(112, 250, 113, 395, 18),
            Placement::new(113, 250, 135, 395, 26),
            Placement::new(114, 250, 176, 395, 18),
            Placement::new(115, 250, 198, 395, 102),
            Placement::new(116, 250, 239, 395, 18),
            Placement::new(117, 250, 261, 395, 26),
            Placement::new(118, 250, 306, 395, 32),
        ]
    );
}

#[test]
fn layout_minimum_width_and_metric_boundaries_match_contract() {
    // C#: ActionProfilesDialogTests.LayoutMetrics_UsesMinimumAndMaximumLeftWidths
    let m = measure();
    assert_eq!(left_width(&m, 0), 150);
    assert_eq!(left_width(&m, 300), 150);
    assert_eq!(left_width(&m, 510), 170);
    assert_eq!(left_width(&m, 810), 255);
}

#[test]
fn filter_and_geometry_cases_match_the_csharp_golden() {
    // C#: ActionProfilesDialogTests.GoldenCases
    let _language = LanguageGuard::english();
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/dialog-action-profiles.json")).unwrap();
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 51);
    let number = |case: &serde_json::Value, key: &str| -> i32 { case[key].as_i64().unwrap_or(0) as i32 };
    let mut compared = 0;
    for case in cases {
        let m = FakeMeasure {
            avg_char_width: number(case, "avg"),
            line_height: number(case, "line"),
            ..Default::default()
        };
        match case["kind"].as_str().unwrap() {
            "file_filter_parse" => {
                let parsed = FileFilterText::parse(case["text"].as_str().unwrap());
                let actual: Vec<_> = parsed
                    .iter()
                    .map(|f| serde_json::json!({"name": f.name, "pattern": f.pattern}))
                    .collect();
                assert_eq!(actual, case["filters"].as_array().unwrap().clone(), "{}", case["name"]);
                compared += 1;
            }
            "metrics" => {
                assert_eq!(margin(&m), number(case, "margin"), "{}", case["name"]);
                assert_eq!(field_height(&m), number(case, "field_height"), "{}", case["name"]);
                assert_eq!(field_height(&m), number(case, "button_height"), "{}", case["name"]);
                assert_eq!(
                    left_width(&m, number(case, "client_width")),
                    number(case, "left_width"),
                    "{}",
                    case["name"]
                );
                compared += 1;
            }
            "preferred" => {
                assert_eq!(
                    preferred_client_size(&m),
                    (number(case, "preferred_width"), number(case, "preferred_height")),
                    "{}",
                    case["name"]
                );
                assert_eq!(
                    minimum_client_size(&m),
                    (number(case, "minimum_width"), number(case, "minimum_height")),
                    "{}",
                    case["name"]
                );
                compared += 1;
            }
            "row_text" => {
                let actions: Vec<_> = case["actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| ReviewAction {
                        name: a["name"].as_str().unwrap().to_owned(),
                        shortcut: a["shortcut"].as_str().unwrap().to_owned(),
                        operation: match a["operation"].as_str().unwrap() {
                            "Move" => FileOperationType::Move,
                            "Copy" => FileOperationType::Copy,
                            "Recycle" => FileOperationType::Recycle,
                            other => panic!("unexpected operation {other}"),
                        },
                        destination: a["destination"].as_str().unwrap().to_owned(),
                        confirm: a["confirm"].as_bool().unwrap(),
                    })
                    .collect();
                let row = ap::row_text_of(&actions, number(case, "index")).unwrap();
                assert_eq!(row.text, case["text"].as_str().unwrap(), "{}", case["name"]);
                assert_eq!(row.badge.as_deref(), case["badge"].as_str(), "{}", case["name"]);
                compared += 1;
            }
            "dialog" => {
                let content = ap::ActionProfilesContent::new(&[], None, None);
                assert_eq!(content.title(), case["title"].as_str().unwrap(), "{}", case["name"]);
                assert_eq!(content.initial_focus_id(), number(case, "initial_focus_id"));
                assert_eq!(content.default_button_id(), number(case, "default_button_id"));
                assert_eq!(content.header_control_ids(), vec![LIST_HEADER_ID, DETAILS_HEADER_ID]);
                assert_eq!(content.resizable(), case["resizable"].as_bool().unwrap());
                assert_eq!(content.uses_modal_frame(), case["uses_modal_frame"].as_bool().unwrap());
                assert_eq!(content.extra_window_style(), number(case, "extra_window_style") as u32);
                let expected = case["items"].as_array().unwrap();
                let items = content.items();
                assert_eq!(items.len(), expected.len());
                for (actual, wanted) in items.iter().zip(expected) {
                    assert_eq!(format!("{:?}", actual.class), wanted["class"].as_str().unwrap());
                    assert_eq!(actual.id, number(wanted, "id"));
                    assert_eq!(actual.text, wanted["text"].as_str().unwrap());
                    assert_eq!(actual.style, number(wanted, "style") as u32);
                    assert_eq!(actual.ex_style, number(wanted, "ex_style") as u32);
                }
                compared += 1;
            }
            "constants" => {
                let ids = [
                    ("list_header_id", ap::LIST_HEADER_ID),
                    ("list_id", ap::LIST_ID),
                    ("add_id", ap::ADD_ID),
                    ("remove_id", ap::REMOVE_ID),
                    ("import_id", ap::IMPORT_ID),
                    ("export_id", ap::EXPORT_ID),
                    ("details_header_id", ap::DETAILS_HEADER_ID),
                    ("name_label_id", ap::NAME_LABEL_ID),
                    ("name_id", ap::NAME_ID),
                    ("shortcut_label_id", ap::SHORTCUT_LABEL_ID),
                    ("shortcut_id", ap::SHORTCUT_ID),
                    ("operation_label_id", ap::OPERATION_LABEL_ID),
                    ("operation_id", ap::OPERATION_ID),
                    ("destination_label_id", ap::DESTINATION_LABEL_ID),
                    ("destination_id", ap::DESTINATION_ID),
                    ("confirm_id", ap::CONFIRM_ID),
                    ("cancel_id", ap::CANCEL_ID),
                    ("apply_id", ap::APPLY_ID),
                ];
                for (key, value) in ids {
                    assert_eq!(value, number(case, key), "{key}");
                }
                compared += 1;
            }
            _ => {}
        }
    }
    assert_eq!(compared, 51);
}
