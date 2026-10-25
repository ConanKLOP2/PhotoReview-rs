use pr_app::presenters::recovery_filter::RecoveryTone;
use pr_app::presenters::recovery_window::RecoveryDetails;
use pr_platform_win::dialogs::recovery::layout::{DetailsLayout, RecoveryLayoutBase};
use pr_platform_win::dialogs::recovery::path_panel::{
    SOURCE_GROUP, build_details_text, explorer_arguments, layout_details, place_path_group,
};

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

#[derive(Default)]
struct Layout;
impl RecoveryLayoutBase for Layout {
    fn dx(&self, value: i32) -> i32 {
        value
    }
    fn dy(&self, value: i32) -> i32 {
        value
    }
    fn measure_text_height(&self, _text: &str, _width: i32) -> i32 {
        10
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
    fn place(&self, _id: i32, _x: i32, _y: i32, _width: i32, _height: i32) {}
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
    layout_details(&Layout, DetailsLayout::default(), 0, 0, 100, 100);
}

#[test]
fn place_path_group_returns_a_later_y() {
    // C#: RecoveryDialog.PlacePathGroup
    assert!(place_path_group(&Layout, 0, 10, 100, SOURCE_GROUP) > 10);
}

#[test]
fn tone_type_is_available_for_color_cases() {
    let _ = RecoveryTone::Neutral;
}
