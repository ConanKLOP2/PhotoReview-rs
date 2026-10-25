use pr_app::presenters::recovery_filter::RecoveryTone;
use pr_app::presenters::recovery_presentation::{RecoveryGroupMemberView, RecoveryPathView};
use pr_app::presenters::recovery_window::RecoveryDetails;
use pr_platform_win::dialogs::recovery::layout::{DetailsLayout, RecoveryLayoutBase};
use pr_platform_win::dialogs::recovery::path_panel::{
    SOURCE_GROUP, build_details_text, explorer_arguments, layout_details, place_path_group,
};
use pr_platform_win::dialogs::recovery::{RecoveryContent, RecoveryDialogSeams, layout::*, path_panel};
use pr_test_support::TempRoot;
use std::cell::RefCell;
use std::rc::Rc;
#[path = "common/dialogs_recovery_fixture.rs"]
mod fixture;

#[test]
fn build_details_text_has_crlf_action_line() {
    // C#: RecoveryDialog.BuildDetailsText
    let details = RecoveryDetails {
        explain_text: "Explanation".to_owned(),
        action_text: "Restore it".to_owned(),
        source_visible: false,
        source: None,
        destination_visible: false,
        destination: None,
        group_members: None,
        group_heading: None,
        error_visible: false,
        error_text: String::new(),
        no_destination_visible: false,
        verdict_tone: RecoveryTone::Neutral,
        verdict_text: String::new(),
    };
    assert_eq!(
        build_details_text(&details, "Action"),
        "Explanation\r\n\r\nAction: Restore it"
    );
}

#[test]
fn build_details_text_omits_empty_action_line() {
    // C#: RecoveryDialogGapTests.ActionLine_IsLeftOutWhenTheVerdictHasNoActionText
    let details = RecoveryDetails {
        explain_text: "Explanation".to_owned(),
        action_text: String::new(),
        source_visible: false,
        source: None,
        destination_visible: false,
        destination: None,
        group_members: None,
        group_heading: None,
        error_visible: false,
        error_text: String::new(),
        no_destination_visible: false,
        verdict_tone: RecoveryTone::Neutral,
        verdict_text: String::new(),
    };
    assert_eq!(build_details_text(&details, "Action"), "Explanation");
}

#[test]
fn explorer_arguments_quotes_existing_file_or_folder() {
    // C#: RecoveryDialog.ShowInExplorer
    let file = |p: &str| p == r"C:\photos\a b.jpg";
    let dir = |p: &str| p == r"C:\photos";
    assert_eq!(
        explorer_arguments(r"C:\photos\a b.jpg", &file, &dir),
        Some(r#"/select,"C:\photos\a b.jpg""#.to_owned())
    );
    assert_eq!(
        explorer_arguments(r"C:\photos\missing.jpg", &file, &dir),
        Some(r#""C:\photos""#.to_owned())
    );
    assert_eq!(explorer_arguments("  ", &file, &dir), None);
}

type Placement = (i32, i32, i32, i32, i32);
#[derive(Default)]
struct Layout(RefCell<Vec<Placement>>, i32);
impl RecoveryLayoutBase for Layout {
    fn dx(&self, value: i32) -> i32 {
        value
    }
    fn dy(&self, value: i32) -> i32 {
        value
    }
    fn measure_text_height(&self, _text: &str, _width: i32) -> i32 {
        if self.1 == 0 { 10 } else { self.1 }
    }
    fn line_height(&self) -> i32 {
        15
    }
    fn measure_item_height(&self, _id: i32, _width: i32) -> i32 {
        15
    }
    fn measure_text_width(&self, text: &str) -> i32 {
        text.len() as i32
    }
    fn get_item_text(&self, _id: i32) -> String {
        String::new()
    }
    fn place(&self, id: i32, x: i32, y: i32, width: i32, height: i32) {
        self.0.borrow_mut().push((id, x, y, width, height));
    }
    fn place_button_row(&self, _client_width: i32, _right_margin: i32, _top: i32, _height: i32, _ids: &[i32]) -> i32 {
        0
    }
    fn button_row_width(&self, _ids: &[i32]) -> i32 {
        0
    }
    fn button_width(&self, _ids: &[i32]) -> i32 {
        10
    }
    fn set_list_item_height(&self, _id: i32, _height: i32) {}
}

#[test]
fn layout_details_with_no_details_places_empty_message() {
    // C#: RecoveryDialog.LayoutDetails
    let layout = Layout::default();
    layout_details(&layout, DetailsLayout::default(), 0, 0, 100, 100);
    assert_eq!(&*layout.0.borrow(), &[(DETAILS_EMPTY_ID, 0, 42, 100, 15)]);
}

#[test]
fn place_path_group_returns_a_later_y() {
    // C#: RecoveryDialog.PlacePathGroup
    let layout = Layout::default();
    assert_eq!(place_path_group(&layout, 0, 10, 100, SOURCE_GROUP), 57);
    assert_eq!(
        &*layout.0.borrow(),
        &[
            (SOURCE_TITLE_ID, 0, 10, 2, 15),
            (SOURCE_STATUS_ID, 10, 10, 90, 15),
            (SOURCE_PATH_ID, 0, 28, 72, 21),
            (COPY_SOURCE_ID, 76, 28, 10, 21),
            (SHOW_SOURCE_ID, 90, 28, 10, 21)
        ]
    );
}

// C#: RecoveryDialog.PlacePathGroup keeps both buttons at ButtonHeight even for a six-line path.
#[test]
fn long_path_keeps_both_path_buttons_at_button_height() {
    let layout = Layout(RefCell::new(Vec::new()), 90);
    assert_eq!(place_path_group(&layout, 0, 10, 100, SOURCE_GROUP), 130);
    let placements = layout.0.borrow();
    assert_eq!(placements[2], (SOURCE_PATH_ID, 0, 28, 72, 94));
    assert_eq!(placements[3], (COPY_SOURCE_ID, 76, 28, 10, 21));
    assert_eq!(placements[4], (SHOW_SOURCE_ID, 90, 28, 10, 21));
}

fn tone(value: &serde_json::Value) -> RecoveryTone {
    match value.as_str().unwrap() {
        "Neutral" => RecoveryTone::Neutral,
        "Good" => RecoveryTone::Good,
        "Info" => RecoveryTone::Info,
        "Warn" => RecoveryTone::Warn,
        "Bad" => RecoveryTone::Bad,
        other => panic!("unknown tone {other}"),
    }
}
fn path(value: &serde_json::Value) -> Option<RecoveryPathView> {
    if value.is_null() {
        return None;
    }
    Some(RecoveryPathView {
        title: value["title"].as_str().unwrap().to_owned(),
        path: value["path"].as_str().unwrap().to_owned(),
        status_text: value["status_text"].as_str().unwrap().to_owned(),
        status_tone: tone(&value["status_tone"]),
        now_text: value["now"].as_str().unwrap().to_owned(),
        journal_text: value["journal"].as_str().unwrap().to_owned(),
        note: value["note"].as_str().map(str::to_owned),
    })
}

// C#: RecoveryDialog.BuildDetailsText / TextColorOf / Copy / ShowInExplorer oracle (W8-18u).
#[test]
fn every_recovery_details_case_matches_the_csharp_golden() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/recovery-dialog-details.json")).unwrap();
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 58);
    let root = TempRoot::new("recovery-details-oracle");
    let root_text = root.path().to_string_lossy().into_owned();
    let files = [root.join("a.jpg"), root.join("with space.jpg"), root.join("sub/b.jpg")];
    let directories = [
        root.path().to_path_buf(),
        root.join("sub"),
        root.path().parent().unwrap().to_path_buf(),
    ];
    for case in cases {
        let name = case["name"].as_str().unwrap();
        match case["kind"].as_str().unwrap() {
            "details_text" => {
                let details = RecoveryDetails {
                    explain_text: case["explain"].as_str().unwrap().to_owned(),
                    action_text: case["action"].as_str().unwrap().to_owned(),
                    source: path(&case["source"]),
                    destination: path(&case["destination"]),
                    source_visible: case["source_visible"].as_bool().unwrap(),
                    destination_visible: case["destination_visible"].as_bool().unwrap(),
                    group_heading: case["group_heading"].as_str().map(str::to_owned),
                    group_members: case["group_members"].as_array().map(|items| {
                        items
                            .iter()
                            .map(|m| RecoveryGroupMemberView {
                                status: m["status"].as_str().unwrap().to_owned(),
                                source: m["source"].as_str().unwrap().to_owned(),
                                destination: m["destination"].as_str().unwrap().to_owned(),
                            })
                            .collect()
                    }),
                    error_visible: false,
                    error_text: String::new(),
                    no_destination_visible: false,
                    verdict_tone: RecoveryTone::Neutral,
                    verdict_text: String::new(),
                };
                assert_eq!(
                    build_details_text(&details, case["action_label"].as_str().unwrap()),
                    case["text"].as_str().unwrap(),
                    "{name}"
                );
            }
            "text_color" => {
                let id = match case["id_name"].as_str().unwrap() {
                    "VerdictBadgeId" => VERDICT_BADGE_ID,
                    "SourceStatusId" => SOURCE_STATUS_ID,
                    "DestinationStatusId" => DESTINATION_STATUS_ID,
                    "ErrorId" => ERROR_ID,
                    "SummaryId" => SUMMARY_ID,
                    "CheckStatusId" => CHECK_STATUS_ID,
                    "VerdictLabelId" => VERDICT_LABEL_ID,
                    "FilterEmptyId" => FILTER_EMPTY_ID,
                    "DetailsEmptyId" => DETAILS_EMPTY_ID,
                    "NoDestinationId" => NO_DESTINATION_ID,
                    "SourcePathId" => SOURCE_PATH_ID,
                    "DestinationPathId" => DESTINATION_PATH_ID,
                    "DetailsId" => DETAILS_ID,
                    "HeaderId" => HEADER_ID,
                    "ListId" => LIST_ID,
                    "CloseId" => CLOSE_ID,
                    other => panic!("unknown id {other}"),
                };
                assert_eq!(
                    path_panel::text_color_for(
                        id,
                        tone(&case["verdict_tone"]),
                        tone(&case["source_tone"]),
                        tone(&case["destination_tone"])
                    ),
                    case["color"].as_u64().unwrap() as u32,
                    "{name}"
                );
            }
            "explorer_args" => {
                let raw = case["path"].as_str().unwrap();
                let input = raw.replace("$ROOT", &root_text);
                let actual =
                    explorer_arguments(&input, &|p| files.iter().any(|f| f == std::path::Path::new(p)), &|p| {
                        directories.iter().any(|d| d == std::path::Path::new(p))
                    })
                    .into_iter()
                    .collect::<Vec<_>>();
                let expected = if raw == "$ROOT" {
                    // The oracle normalized its root but retained its machine's temp parent. Bind that parent to this test's TempRoot.
                    assert_eq!(case["args"].as_array().unwrap().len(), 1);
                    vec![format!("\"{}\"", root.path().parent().unwrap().display())]
                } else {
                    case["args"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_str().unwrap().replace("$ROOT", &root_text))
                        .collect()
                };
                assert_eq!(actual, expected, "{name}");
            }
            "copy" => {
                let calls = Rc::new(RefCell::new(Vec::new()));
                let recorded = calls.clone();
                let content = RecoveryContent::new(
                    Vec::new(),
                    fixture::options(),
                    fixture::services(),
                    RecoveryDialogSeams {
                        copy: Some(Rc::new(move |p| {
                            recorded.borrow_mut().push(p.to_owned());
                            false
                        })),
                        auto_check: false,
                        ..Default::default()
                    },
                );
                path_panel::copy(&content, case["path"].as_str().unwrap());
                assert_eq!(serde_json::json!(&*calls.borrow()), case["calls"], "{name}");
            }
            other => panic!("unhandled golden kind {other}"),
        }
    }
}
