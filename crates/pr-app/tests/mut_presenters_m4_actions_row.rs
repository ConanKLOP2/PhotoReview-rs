//! M4 mutation killers for the `Actions` row of `presenters/settings_field_map.rs` (load text of the action list and the
//! apply validation). Hand-written, no golden data.
use pr_app::presenters::settings_field_map::{FieldContext, bindings};
use pr_app::presenters::settings_form_state::{SettingsFieldError, SettingsFocusField, SettingsFormState};
use pr_core::model::enums::FileOperationType;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::shortcuts::ReviewAction;
use pr_l10n::localizer::built_in_english_localizer;
use pr_l10n::template::NumberCulture;

const INVALID: &str = "The action profiles JSON is not valid.";

fn settings() -> AppSettings {
    AppSettings::new(&EnglishSettingsTexts)
}

fn row() -> &'static pr_app::presenters::settings_field_map::SettingsFieldBinding {
    bindings()
        .iter()
        .find(|b| b.properties.contains(&"Actions"))
        .expect("Actions row")
}

fn load_text(actions: Vec<ReviewAction>) -> String {
    let mut s = settings();
    s.actions = actions;
    let mut form = SettingsFormState::default();
    (row().load)(&s, &mut form);
    form.actions_json
}

fn apply(json: &str) -> (Option<SettingsFieldError>, AppSettings) {
    let form = SettingsFormState {
        actions_json: json.to_owned(),
        ..SettingsFormState::default()
    };
    let texts = built_in_english_localizer();
    let numbers = NumberCulture::invariant();
    let ctx = FieldContext {
        texts: &texts,
        numbers: &numbers,
    };
    let mut target = settings();
    target.actions = vec![ReviewAction::new("Before")];
    let error = (row().apply)(&form, &mut target, &ctx);
    (error, target)
}

fn action(name: &str, shortcut: &str, operation: FileOperationType, destination: &str, confirm: bool) -> ReviewAction {
    let mut a = ReviewAction::new(name);
    a.shortcut = shortcut.to_owned();
    a.operation = operation;
    a.destination = destination.to_owned();
    a.confirm = confirm;
    a
}

#[test]
fn load_writes_the_indented_json_with_crlf_and_escapes_every_control_character() {
    assert_eq!(load_text(Vec::new()), "[]");
    let text = load_text(vec![
        action("Keep", "K", FileOperationType::Move, "Group-1", false),
        action("Copy it", "C", FileOperationType::Copy, "D:\\out", true),
        action("Bin", "Delete", FileOperationType::Recycle, "", false),
    ]);
    let expected = "[\r\n  {\r\n    \"Name\": \"Keep\",\r\n    \"Shortcut\": \"K\",\r\n    \"Operation\": \"Move\",\r\n    \"Destination\": \"Group-1\",\r\n    \"Confirm\": false\r\n  },\r\n  {\r\n    \"Name\": \"Copy it\",\r\n    \"Shortcut\": \"C\",\r\n    \"Operation\": \"Copy\",\r\n    \"Destination\": \"D:\\\\out\",\r\n    \"Confirm\": true\r\n  },\r\n  {\r\n    \"Name\": \"Bin\",\r\n    \"Shortcut\": \"Delete\",\r\n    \"Operation\": \"Recycle\",\r\n    \"Destination\": \"\",\r\n    \"Confirm\": false\r\n  }\r\n]";
    assert_eq!(text, expected);
    let odd = load_text(vec![action(
        "q\"b\\n\nr\rt\tc\u{1}u\u{1f}s\u{20}e\u{7f}é",
        "K",
        FileOperationType::Move,
        "x",
        false,
    )]);
    assert!(
        odd.contains("\"Name\": \"q\\\"b\\\\n\\nr\\rt\\tc\\u0001u\\u001fs e\u{7f}é\""),
        "{odd}"
    );
}

fn one(extra: &str) -> String {
    format!("[{{\"Name\":\"N\",\"Shortcut\":\"K\"{extra}}}]")
}

#[test]
fn apply_reads_null_empty_and_complete_lists() {
    let (error, target) = apply("null");
    assert_eq!(error, None);
    assert!(target.actions.is_empty());
    let (error, target) = apply("[]");
    assert_eq!(error, None);
    assert!(target.actions.is_empty());
    let (error, target) = apply(
        "[{\"Name\":\"A\",\"Shortcut\":\"K\",\"Operation\":\"Copy\",\"Destination\":\"D\",\"Confirm\":true},\
          {\"Name\":\"B\",\"Shortcut\":\"L\",\"Operation\":1,\"Destination\":\"E\"}]",
    );
    assert_eq!(error, None);
    assert_eq!(target.actions.len(), 2);
    assert_eq!(target.actions[0], action("A", "K", FileOperationType::Copy, "D", true));
    assert_eq!(target.actions[1], action("B", "L", FileOperationType::Copy, "E", false));
}

#[test]
fn apply_accepts_the_defaults_for_missing_members_and_ignores_unknown_ones() {
    let (error, target) = apply(&one(",\"Extra\":[1,2]"));
    assert_eq!(error, None);
    assert_eq!(
        target.actions,
        vec![action("N", "K", FileOperationType::Move, "Group-2", false)]
    );
}

#[test]
fn apply_reads_every_operation_spelling() {
    for (text, expected) in [
        ("\"Move\"", FileOperationType::Move),
        ("\"Copy\"", FileOperationType::Copy),
        ("\"Recycle\"", FileOperationType::Recycle),
        ("\"Delete\"", FileOperationType::Recycle),
        ("0", FileOperationType::Move),
        ("1", FileOperationType::Copy),
        ("2", FileOperationType::Recycle),
    ] {
        let (error, target) = apply(&one(&format!(",\"Operation\":{text}")));
        assert_eq!(error, None, "{text}");
        assert_eq!(target.actions[0].operation, expected, "{text}");
    }
    for text in ["\"Nope\"", "3", "-1", "true", "null", "[]", "1.5"] {
        let (error, target) = apply(&one(&format!(",\"Operation\":{text}")));
        let error = error.unwrap_or_else(|| panic!("{text} must be refused"));
        assert_eq!(
            (error.message.as_str(), error.focus),
            (INVALID, SettingsFocusField::None),
            "{text}"
        );
        assert_eq!(
            target.actions,
            vec![ReviewAction::new("Before")],
            "{text}: unparsable list is not stored"
        );
    }
}

#[test]
fn apply_refuses_members_of_the_wrong_type_and_non_list_documents() {
    for json in [
        "[{\"Name\":1,\"Shortcut\":\"K\"}]",
        "[{\"Name\":null,\"Shortcut\":\"K\"}]",
        "[{\"Name\":\"N\",\"Shortcut\":2}]",
        "[{\"Name\":\"N\",\"Shortcut\":\"K\",\"Destination\":3}]",
        "[{\"Name\":\"N\",\"Shortcut\":\"K\",\"Confirm\":\"yes\"}]",
        "[{\"Name\":\"N\",\"Shortcut\":\"K\",\"Confirm\":1}]",
        "[1]",
        "[null]",
        "{}",
        "\"x\"",
        "5",
        "true",
        "[",
        "",
    ] {
        let (error, target) = apply(json);
        let error = error.unwrap_or_else(|| panic!("{json:?} must be refused"));
        assert_eq!(
            (error.message.as_str(), error.focus),
            (INVALID, SettingsFocusField::None),
            "{json:?}"
        );
        assert_eq!(target.actions, vec![ReviewAction::new("Before")], "{json:?}");
    }
}

#[test]
fn apply_validates_names_and_shortcuts_after_storing_the_parsed_list() {
    // C# parses, assigns, then validates: a refused list is still the stored one.
    for json in [
        "[{\"Name\":\"\",\"Shortcut\":\"K\"}]",
        "[{\"Name\":\"   \",\"Shortcut\":\"K\"}]",
        "[{\"Name\":\"N\",\"Shortcut\":\"\"}]",
        "[{\"Name\":\"A\",\"Shortcut\":\"K\"},{\"Name\":\"B\",\"Shortcut\":\"k\"}]",
        "[{\"Name\":\"A\",\"Shortcut\":\"K\"},{\"Name\":\"B\",\"Shortcut\":\"K\"}]",
    ] {
        let (error, target) = apply(json);
        let error = error.unwrap_or_else(|| panic!("{json:?} must be refused"));
        assert_eq!(
            (error.message.as_str(), error.focus),
            (INVALID, SettingsFocusField::None),
            "{json:?}"
        );
        assert_ne!(
            target.actions,
            vec![ReviewAction::new("Before")],
            "{json:?}: parsed list is stored first"
        );
    }
    let (error, target) = apply("[{\"Name\":\"A\",\"Shortcut\":\"K\"},{\"Name\":\"B\",\"Shortcut\":\"L\"}]");
    assert_eq!(error, None);
    assert_eq!(target.actions.len(), 2);
}

// The key-length rule: a canonical shortcut that is not a known shortcut property name and is longer than 128 chars is refused;
// exactly 128 chars is accepted. (A property name is never that long, so the known-name test only matters through the `!`.)
#[test]
fn apply_refuses_an_unknown_shortcut_key_longer_than_128_chars_and_accepts_exactly_128() {
    let with_key = |len: usize| apply(&format!("[{{\"Name\":\"N\",\"Shortcut\":\"{}\"}}]", "k".repeat(len)));
    for len in [1, 127, 128] {
        let (error, target) = with_key(len);
        assert_eq!(error, None, "{len} chars");
        assert_eq!(target.actions.len(), 1, "{len} chars");
    }
    for len in [129, 130, 400] {
        let (error, _) = with_key(len);
        let error = error.unwrap_or_else(|| panic!("{len} chars must be refused"));
        assert_eq!(
            (error.message.as_str(), error.focus),
            (INVALID, SettingsFocusField::None),
            "{len} chars"
        );
    }
}
