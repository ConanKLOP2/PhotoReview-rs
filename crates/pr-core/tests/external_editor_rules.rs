//! C# ExternalEditorsTests, all 77 external-editor-rules oracle cases (W9-E1).
use pr_core::settings::{
    app_settings::{AppSettings, EnglishSettingsTexts},
    external_editors::{ExternalEditor, ExternalEditors},
};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;

// C#: ExternalEditorsTests.Defaults_NothingDeclared_RawOn_ShortcutEmptyAndOptional.
#[test]
fn defaults_nothing_declared_raw_on_shortcut_empty_and_optional() {
    let settings = AppSettings::new(&EnglishSettingsTexts);
    assert!(settings.external_editors.is_empty());
    assert!(settings.external_editor_sends_raw_original);
    assert!(settings.shortcuts.open_in_default_editor.is_empty());
    assert!(pr_core::settings::shortcuts::ShortcutMappings::is_optional(
        "OpenInDefaultEditor"
    ));
}

struct AlwaysValid;

// C#: ExternalEditors.Reconcile uses string.Length (UTF-16), not UTF-8 bytes or scalar count.
#[test]
fn reconcile_argument_limit_counts_utf16_and_preserves_valid_nonblank_text() {
    let mut settings = AppSettings::new(&EnglishSettingsTexts);
    settings.external_editors.push(ExternalEditor {
        path: "editor.exe".to_owned(),
        arguments: "😀".repeat(2000),
        ..ExternalEditor::new()
    });
    assert!(!ExternalEditors::reconcile(&mut settings));
    assert_eq!(settings.external_editors[0].arguments, "😀".repeat(2000));
    settings.external_editors[0].arguments.push('x');
    assert!(ExternalEditors::reconcile(&mut settings));
    assert_eq!(
        settings.external_editors[0].arguments,
        ExternalEditor::DEFAULT_ARGUMENTS
    );
    settings.external_editors[0].arguments = "  --open {file}  ".to_owned();
    assert!(!ExternalEditors::reconcile(&mut settings));
    assert_eq!(settings.external_editors[0].arguments, "  --open {file}  ");
}

// C#: ExternalEditor property initializers versus explicit null strings in config JSON.
#[test]
fn read_editors_missing_fields_use_defaults_and_null_strings_become_empty() {
    let settings = AppSettings::from_json(
        r#"{"ExternalEditors":[{"Path":"a.exe"},{"Id":null,"Name":null,"Path":null,"Arguments":null}]}"#,
        &EnglishSettingsTexts,
    )
    .unwrap()
    .unwrap();
    let first = &settings.external_editors[0];
    assert_eq!(first.id.len(), 32);
    assert!(
        first
            .id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert!(first.name.is_empty());
    assert_eq!(first.arguments, ExternalEditor::DEFAULT_ARGUMENTS);
    let second = &settings.external_editors[1];
    assert!(second.id.is_empty() && second.name.is_empty() && second.path.is_empty() && second.arguments.is_empty());
    let text = settings.to_json().unwrap();
    let editors = text.split_once("\"ExternalEditors\"").unwrap().1;
    let positions: Vec<_> = ["\"Id\"", "\"Name\"", "\"Path\"", "\"Arguments\""]
        .iter()
        .map(|key| editors.find(key).unwrap())
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(!text.contains("DisplayName"));
}
impl pr_core::abstractions::KeyNameValidator for AlwaysValid {
    fn is_valid_key_name(&self, _: &str) -> bool {
        true
    }
}

// C#: ExternalEditorsTests.ValidateShortcuts_DefaultEditorKeyOnAnActionKey_IsADuplicate_AndEmptyIsAllowed.
#[test]
fn validate_shortcuts_default_editor_key_on_an_action_key_is_a_duplicate_and_empty_is_allowed() {
    let validator =
        pr_core::settings::normalizer::SettingsValidator::new(Arc::new(AlwaysValid), Arc::new(EnglishSettingsTexts));
    let mut settings = AppSettings::new(&EnglishSettingsTexts);
    assert!(validator.validate_shortcuts(&settings).is_none());
    settings.shortcuts.open_in_default_editor = settings.actions[0].shortcut.clone();
    assert!(validator.validate_shortcuts(&settings).is_some());
    settings.shortcuts.open_in_default_editor = "F9".to_owned();
    assert!(validator.validate_shortcuts(&settings).is_none());
}

// C#: SettingsNormalizer trims optional keys; ExternalEditors reconciliation is the final repair.
#[test]
fn normalize_editor_key_trims_and_conflicting_optional_key_is_disabled() {
    let mut settings = AppSettings::new(&EnglishSettingsTexts);
    settings.shortcuts.open_in_default_editor = " F6 ".to_owned();
    pr_core::settings::normalizer::normalize(&mut settings);
    assert_eq!(settings.shortcuts.open_in_default_editor, "F6");
    settings.shortcuts.open_in_default_editor = settings.actions[0].shortcut.clone();
    let disabled = pr_core::settings::normalizer::disable_conflicting_optional_shortcuts(&mut settings);
    assert!(disabled.iter().any(|name| name == "OpenInDefaultEditor"));
    assert!(settings.shortcuts.open_in_default_editor.is_empty());
}

fn settings(value: &Value, known: &mut HashSet<String>) -> AppSettings {
    let mut settings = AppSettings::new(&EnglishSettingsTexts);
    if let Some(editors) = value["editors"].as_array() {
        settings.external_editors = editors
            .iter()
            .map(|value| {
                // C# can hold null slots; the Rust fixture represents one as an invalid blank-path slot.
                // Reconcile must discard it and report a repair. JSON reader coverage is separate.
                if value.is_null() {
                    return ExternalEditor {
                        id: String::new(),
                        name: String::new(),
                        path: String::new(),
                        arguments: String::new(),
                    };
                }
                let mut editor = ExternalEditor::new();
                for (key, target) in [
                    ("id", &mut editor.id),
                    ("name", &mut editor.name),
                    ("path", &mut editor.path),
                    ("arguments", &mut editor.arguments),
                ] {
                    if let Some(value) = value.get(key) {
                        *target = value.as_str().unwrap_or("").to_owned();
                    }
                }
                if !editor.id.is_empty() {
                    known.insert(editor.id.clone());
                }
                editor
            })
            .collect();
    }
    if let Some(id) = value.get("default_id") {
        settings.default_external_editor_id = id.as_str().unwrap_or("").to_owned();
    }
    if let Some(path) = value.get("legacy_path") {
        settings.external_editor_path = path.as_str().unwrap_or("").to_owned();
    }
    settings
}
fn editor(value: &ExternalEditor, known: &HashSet<String>) -> Value {
    json!({"id":if known.contains(&value.id)||value.id==ExternalEditors::LEGACY_EDITOR_ID{&value.id}else{"<fresh>"},"name":value.name,"path":value.path,"arguments":value.arguments})
}
fn index(list: &[ExternalEditor], editor: Option<ExternalEditor>) -> i32 {
    editor.map_or(-1, |editor| {
        list.iter().position(|e| e.id == editor.id).map_or(-2, |i| i as i32)
    })
}
fn dump(settings: &AppSettings, known: &HashSet<String>) -> Value {
    json!({"editors":settings.external_editors.iter().map(|e|editor(e,known)).collect::<Vec<_>>(),"default_index":settings.external_editors.iter().position(|e|e.id==settings.default_external_editor_id).map_or(-1,|i|i as i32),"default_id_blank":settings.default_external_editor_id.is_empty(),"legacy_path":settings.external_editor_path})
}
#[test]
fn every_external_editor_rule_matches_the_csharp_golden() {
    let inputs: Value =
        serde_json::from_str(include_str!("../../../tests/golden-inputs/external-editor-rules.json")).unwrap();
    let golden: Value = serde_json::from_str(include_str!("../../../tests/golden/external-editor-rules.json")).unwrap();
    let inputs = inputs["cases"].as_array().unwrap();
    let expected = golden["cases"].as_array().unwrap();
    assert_eq!(inputs.len(), 77);
    assert_eq!(expected.len(), inputs.len());
    for (input, expected) in inputs.iter().zip(expected) {
        let kind = input["kind"].as_str().unwrap();
        let mut actual = json!({"kind":kind,"name":input["name"]});
        match kind {
            "constants" => {
                actual["max_editors"] = json!(ExternalEditors::MAX_EDITORS);
                actual["default_arguments"] = json!(ExternalEditor::DEFAULT_ARGUMENTS);
                actual["legacy_editor_id"] = json!(ExternalEditors::LEGACY_EDITOR_ID);
            }
            "name" => actual["value"] = json!(ExternalEditors::name_from_path(input["path"].as_str())),
            "display_name" => {
                actual["value"] = json!(
                    ExternalEditor {
                        id: "test".to_owned(),
                        name: input["editor_name"].as_str().unwrap_or("").to_owned(),
                        path: input["path"].as_str().unwrap_or("").to_owned(),
                        arguments: ExternalEditor::DEFAULT_ARGUMENTS.to_owned()
                    }
                    .display_name()
                )
            }
            "arguments" => {
                match ExternalEditors::build_arguments(input["template"].as_str(), input["path"].as_str().unwrap_or(""))
                {
                    Some(value) => actual["value"] = json!(value),
                    None => actual["error"] = json!("ArgumentException"),
                }
            }
            "effective" => {
                let mut known = HashSet::new();
                let settings = settings(&input["settings"], &mut known);
                let effective = ExternalEditors::effective(&settings);
                actual["effective"] = json!(effective.iter().map(|e| editor(e, &known)).collect::<Vec<_>>());
                let chosen = ExternalEditors::effective_default(&settings);
                // Each C# Effective call creates a distinct implicit legacy object; Rust returns value copies.
                actual["effective_default_index"] =
                    json!(if settings.external_editors.is_empty() && chosen.is_some() {
                        -2
                    } else {
                        index(&effective, chosen)
                    });
                actual["default_index"] = json!(index(&settings.external_editors, ExternalEditors::default(&settings)));
                let mut finds = serde_json::Map::new();
                if let Some(ids) = input["find_ids"].as_array() {
                    for id in ids {
                        finds.insert(
                            id.as_str().unwrap_or("<null>").to_owned(),
                            json!(index(
                                &settings.external_editors,
                                ExternalEditors::find(&settings, id.as_str())
                            )),
                        );
                    }
                }
                actual["find"] = Value::Object(finds);
            }
            "reconcile" | "apply_legacy" => {
                let mut known = HashSet::new();
                let mut settings = settings(&input["settings"], &mut known);
                if kind == "reconcile" {
                    actual["repaired"] = json!(ExternalEditors::reconcile(&mut settings));
                } else {
                    ExternalEditors::apply_legacy_path(&mut settings, input["legacy_path"].as_str());
                }
                actual["settings"] = dump(&settings, &known);
            }
            "resolve" => {
                let existing = input["existing"].as_array().unwrap();
                actual["value"] = json!(ExternalEditors::resolve_file_to_send(
                    input["path"].as_str().unwrap(),
                    input["paired_raw"].as_str(),
                    input["send_raw"].as_bool().unwrap(),
                    &|path| existing
                        .iter()
                        .any(|p| pr_core::text::eq_ignore_case(p.as_str().unwrap(), path))
                ));
            }
            other => panic!("unhandled oracle kind {other}"),
        }
        assert_eq!(&actual, expected, "{}", input["name"]);
    }
}
