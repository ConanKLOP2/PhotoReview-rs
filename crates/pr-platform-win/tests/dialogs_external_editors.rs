//! All committed W9-E2 dialog-external-editors oracle cases; no window created.
#[path = "common/dialogs_secondary_fixture.rs"]
mod fixture;
use fixture::{FakeMeasure, LanguageGuard};
use pr_core::settings::external_editors::ExternalEditor;
use pr_platform_win::dialogs::native::dialog::DialogContent;
use pr_platform_win::dialogs::native::external_editors_dialog as ed;
use serde_json::{Value, json};

// Source-derived F: C# wrapped text and wide Browse text, with independently worked pixel coordinates.
#[test]
fn layout_plan_keeps_measured_labels_and_wide_browse_in_csharp_place_order() {
    use pr_platform_win::dialogs::zoom_prompt::Placement as P;
    let mut m = FakeMeasure {
        avg_char_width: 4,
        line_height: 8,
        text_width: 123,
        ..Default::default()
    };
    m.item_heights.extend([
        (ed::LIST_HEADER_ID, 11),
        (ed::DETAILS_HEADER_ID, 13),
        (ed::NAME_LABEL_ID, 17),
        (ed::PATH_LABEL_ID, 19),
        (ed::ARGUMENTS_LABEL_ID, 23),
        (ed::HINT_ID, 29),
    ]);
    let plan = ed::layout_plan(&m, 600, 400);
    assert_eq!(
        plan.placements,
        vec![
            P::new(ed::LIST_HEADER_ID, 10, 10, 170, 11),
            P::new(ed::LIST_ID, 10, 25, 170, 307),
            P::new(ed::ADD_ID, 10, 336, 83, 14),
            P::new(ed::REMOVE_ID, 97, 336, 83, 14),
            P::new(ed::SET_DEFAULT_ID, 10, 354, 170, 14),
            P::new(ed::DETAILS_HEADER_ID, 190, 10, 400, 13),
            P::new(ed::NAME_LABEL_ID, 190, 31, 400, 17),
            P::new(ed::NAME_ID, 190, 50, 400, 14),
            P::new(ed::PATH_LABEL_ID, 190, 72, 400, 19),
            P::new(ed::PATH_ID, 190, 93, 259, 14),
            P::new(ed::BROWSE_ID, 453, 93, 137, 14),
            P::new(ed::ARGUMENTS_LABEL_ID, 190, 115, 400, 23),
            P::new(ed::ARGUMENTS_ID, 190, 140, 400, 14),
            P::new(ed::HINT_ID, 190, 158, 400, 29),
        ]
    );
    assert_eq!(plan.list_item_height, 13);
    assert_eq!(
        (
            plan.button_row.client_width,
            plan.button_row.right_margin,
            plan.button_row.top,
            plan.button_row.height
        ),
        (600, 10, 376, 14)
    );
    assert_eq!(plan.button_row.ids, [ed::CANCEL_ID, ed::APPLY_ID]);
}

// C#: IndexOfDefault marks only its first matching row, even for unreconciled duplicate IDs.
#[test]
fn duplicate_default_ids_only_badge_the_first_row_and_invalid_indices_have_no_row() {
    let editors = vec![
        ExternalEditor {
            id: "same".to_owned(),
            name: "First".to_owned(),
            path: "first.exe".to_owned(),
            arguments: String::new(),
        },
        ExternalEditor {
            id: "same".to_owned(),
            name: "Second".to_owned(),
            path: "second.exe".to_owned(),
            arguments: String::new(),
        },
    ];
    assert!(ed::row_text_of(&editors, "same", 0).unwrap().badge.is_some());
    assert!(ed::row_text_of(&editors, "same", 1).unwrap().badge.is_none());
    assert!(ed::row_text_of(&editors, "same", -1).is_none());
    assert!(ed::row_text_of(&editors, "same", 2).is_none());
}

fn editors(case: &Value) -> Vec<ExternalEditor> {
    case["editors"].as_array().map_or_else(Vec::new, |items| {
        items
            .iter()
            .map(|value| ExternalEditor {
                id: value["id"].as_str().unwrap_or("").to_owned(),
                name: value["name"].as_str().unwrap_or("").to_owned(),
                path: value["path"].as_str().unwrap_or("").to_owned(),
                arguments: value["arguments"]
                    .as_str()
                    .unwrap_or(ExternalEditor::DEFAULT_ARGUMENTS)
                    .to_owned(),
            })
            .collect()
    })
}

#[test]
fn all_external_editors_dialog_cases_match_the_csharp_golden() {
    let _language = LanguageGuard::english();
    let inputs: Value = serde_json::from_str(include_str!(
        "../../../tests/golden-inputs/dialog-external-editors.json"
    ))
    .unwrap();
    let golden: Value =
        serde_json::from_str(include_str!("../../../tests/golden/dialog-external-editors.json")).unwrap();
    let inputs = inputs["cases"].as_array().unwrap();
    let expected = golden["cases"].as_array().unwrap();
    assert_eq!(inputs.len(), 14);
    assert_eq!(inputs.len(), expected.len());
    for (input, expected) in inputs.iter().zip(expected) {
        let mut actual = input.clone();
        let kind = input["kind"].as_str().unwrap();
        let number = |key: &str| input[key].as_i64().unwrap_or(0) as i32;
        let measure = FakeMeasure {
            avg_char_width: number("avg"),
            line_height: number("line"),
            ..Default::default()
        };
        match kind {
            "dialog" => {
                let content =
                    ed::ExternalEditorsContent::new(&editors(input), input["default_id"].as_str(), None, None, None);
                actual["title"] = json!(content.title());
                actual["items"]=json!(content.items().iter().map(|item| json!({"class":format!("{:?}",item.class),"id":item.id,"text":item.text,"style":item.style,"ex_style":item.ex_style})).collect::<Vec<_>>());
                actual["initial_focus_id"] = json!(content.initial_focus_id());
                actual["default_button_id"] = json!(content.default_button_id());
                actual["resizable"] = json!(content.resizable());
                actual["header_ids"] = json!(content.header_control_ids());
                actual["uses_modal_frame"] = json!(content.uses_modal_frame());
                actual["extra_window_style"] = json!(content.extra_window_style());
            }
            "row_text" => {
                // The oracle calls RowText on a constructed dialog without a window;
                // this also checks the base fallback for an empty list.
                let dialog = ed::ExternalEditorsDialog::new(
                    &editors(input),
                    input["default_id"].as_str(),
                    None,
                    None,
                    None,
                    None,
                );
                let row = dialog.row_text(number("index"));
                actual["text"] = json!(row.text);
                actual["badge"] = json!(row.badge);
            }
            "preferred" => {
                let (w, h) = ed::preferred_client_size(&measure);
                actual["preferred_width"] = json!(w);
                actual["preferred_height"] = json!(h);
                let (w, h) = ed::minimum_client_size(&measure);
                actual["minimum_width"] = json!(w);
                actual["minimum_height"] = json!(h);
            }
            "metrics" => {
                actual["margin"] = json!(ed::margin(&measure));
                actual["field_height"] = json!(ed::field_height(&measure));
                actual["button_height"] = json!(ed::field_height(&measure));
                actual["left_width"] = json!(ed::left_width(&measure, number("client_width")));
            }
            "effective_default" => {
                let editors = editors(input);
                let content = ed::ExternalEditorsContent::new(&editors, input["default_id"].as_str(), None, None, None);
                let id = content.outcome().default_id;
                actual["effective_default_id"] = json!(id);
                actual["index_of_default"] = json!(
                    editors
                        .iter()
                        .position(|editor| editor.id == id)
                        .map_or(-1, |i| i as i32)
                );
            }
            "constants" => {
                actual["values"] = json!({"list_header_id":ed::LIST_HEADER_ID,"list_id":ed::LIST_ID,"add_id":ed::ADD_ID,"remove_id":ed::REMOVE_ID,"set_default_id":ed::SET_DEFAULT_ID,"details_header_id":ed::DETAILS_HEADER_ID,"name_label_id":ed::NAME_LABEL_ID,"name_id":ed::NAME_ID,"path_label_id":ed::PATH_LABEL_ID,"path_id":ed::PATH_ID,"browse_id":ed::BROWSE_ID,"arguments_label_id":ed::ARGUMENTS_LABEL_ID,"arguments_id":ed::ARGUMENTS_ID,"hint_id":ed::HINT_ID,"cancel_id":ed::CANCEL_ID,"apply_id":ed::APPLY_ID})
            }
            other => panic!("unknown oracle kind {other}"),
        }
        assert_eq!(&actual, expected, "{}", input["name"]);
    }
}
