//! Red tests for `pr_app::presenters::action_list_json` (W8-10a, contract `contracts/W8-10a.md`): the bodies are `todo!()`.
//! Golden `action-list-json` (26 cases) plus hand-written tests for the rules and traps T8 to T12.
//! There is no C# test of `ActionListJson` by name; every test here is `// new:`.

mod presenters_support;

use pr_app::presenters::action_list_json::{
    ActionListJsonError, deserialize, deserialize_or_null, serialize, serialize_for_export, throw_if_invalid,
};
use pr_core::model::enums::FileOperationType;
use pr_core::settings::shortcuts::ReviewAction;
use presenters_support::{act, actions_of, golden_pairs, lf, mv, rows};

/// `EnglishSettingsTexts::action_default_name(2)`, the default the golden (language `en`) uses (open question 5).
const DEFAULT_NAME: &str = "Action 2";

#[test]
fn golden_action_list_json() {
    // new: golden `action-list-json` (oracle W8-10a.cs), all kinds.
    let mut failures = Vec::new();
    for (input, expected) in golden_pairs("action-list-json") {
        let name = expected["name"].as_str().unwrap().to_owned();
        let kind = expected["kind"].as_str().unwrap();
        let outcome: Result<(), String> = match kind {
            "serialize" | "serialize_export" => {
                let actions = actions_of(&input["actions"]);
                let got = if kind == "serialize" {
                    serialize(&actions)
                } else {
                    serialize_for_export(&actions)
                };
                let want = lf(expected["text"].as_str().unwrap());
                if got == want {
                    Ok(())
                } else {
                    Err(format!("got {got:?}, want {want:?}"))
                }
            }
            "deserialize" => {
                let got = deserialize(input["text"].as_str().unwrap(), DEFAULT_NAME);
                check_read(&expected["result"], got.map(Some))
            }
            "deserialize_or_null" => {
                let got = deserialize_or_null(input["text"].as_str().unwrap(), DEFAULT_NAME);
                check_read(&expected["result"], got)
            }
            "throw_if_invalid" => {
                let got = throw_if_invalid(&actions_of(&input["actions"]));
                if expected["ok"].as_bool() == Some(true) {
                    if got == Ok(()) {
                        Ok(())
                    } else {
                        Err(format!("want Ok, got {got:?}"))
                    }
                } else {
                    let want = expected["message"].as_str().unwrap();
                    match got {
                        Err(ActionListJsonError::Invalid(m)) if m == want => Ok(()),
                        other => Err(format!("want Invalid({want:?}), got {other:?}")),
                    }
                }
            }
            other => panic!("unknown golden kind {other}"),
        };
        if let Err(why) = outcome {
            failures.push(format!("{name}: {why}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} golden failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Compares a reader result with a golden `{value}` / `{throws}` object.
fn check_read(
    expected: &serde_json::Value,
    got: Result<Option<Vec<ReviewAction>>, ActionListJsonError>,
) -> Result<(), String> {
    if expected.get("throws").is_some() {
        return match got {
            Err(ActionListJsonError::Malformed(_)) => Ok(()),
            other => Err(format!("want Malformed, got {other:?}")),
        };
    }
    match (&expected["value"], got) {
        (serde_json::Value::Null, Ok(None)) => Ok(()),
        (value, Ok(Some(list))) if value.is_array() => {
            let want = actions_of(value);
            if rows(&list) == rows(&want) {
                Ok(())
            } else {
                Err(format!("got {list:?}, want {want:?}"))
            }
        }
        (_, other) => Err(format!("unexpected result {other:?} for {expected}")),
    }
}

#[test]
fn serialize_empty_is_brackets() {
    // new: an empty list is `[]` in both writers (also golden `serialize-empty`, `export-empty`).
    assert_eq!(serialize(&[]), "[]");
    assert_eq!(serialize_for_export(&[]), "[]");
}

#[test]
fn serialize_writes_indented_json_in_property_order_with_operation_by_name() {
    // new: rule 16, two-space indent, `\n` line ends, order Name, Shortcut, Operation, Destination, Confirm.
    let text = serialize(&[act("Copy it", "F6", FileOperationType::Copy, "Backup", true)]);
    let want = "[\n  {\n    \"Name\": \"Copy it\",\n    \"Shortcut\": \"F6\",\n    \"Operation\": \"Copy\",\n    \"Destination\": \"Backup\",\n    \"Confirm\": true\n  }\n]";
    assert_eq!(text, want);
}

#[test]
fn serialize_relaxed_keeps_non_ascii_and_html_characters() {
    // new: T8, relaxed escaping writes non-ASCII and `< > & ' +` as they are; only `"`, `\` and controls are escaped.
    let text = serialize(&[act(
        "Vi\u{1ec7}t <a>&'+ \"q\" b\\s",
        "F6",
        FileOperationType::Move,
        "Sao l\u{01b0}u",
        false,
    )]);
    assert!(
        text.contains("\"Name\": \"Vi\u{1ec7}t <a>&'+ \\\"q\\\" b\\\\s\""),
        "{text}"
    );
    assert!(text.contains("\"Destination\": \"Sao l\u{01b0}u\""), "{text}");
}

#[test]
fn serialize_escapes_control_characters() {
    // new: T8, a tab and a newline use short escapes, other controls use `\u00XX`.
    let text = serialize(&[mv("tab\tnl\nctl\u{1}", "F8")]);
    assert!(text.contains("\"Name\": \"tab\\tnl\\nctl\\u0001\""), "{text}");
}

#[test]
fn export_escaping_matches_default_encoder() {
    // new: T8 (golden `export-vi-default-escaping`, `export-html-sensitive-and-plus`): every non-ASCII character and
    // `< > & ' " + \`` are `\uXXXX` with UPPERCASE hex, as .NET writes them; the backslash stays `\\`.
    let text = serialize_for_export(&[act("Vi\u{1ec7}t", "F6", FileOperationType::Copy, "Backup", false)]);
    assert!(text.contains("\"Name\": \"Vi\\u1EC7t\""), "{text}");
    let text = serialize_for_export(&[act("<a>&'\"+`\\", "F9", FileOperationType::Move, "x", true)]);
    assert!(
        text.contains("\"Name\": \"\\u003Ca\\u003E\\u0026\\u0027\\u0022\\u002B\\u0060\\\\\""),
        "{text}"
    );
}

#[test]
fn serialize_then_deserialize_round_trips() {
    // new: both writers produce text that the reader reads back unchanged.
    let list = vec![
        act("Vi\u{1ec7}t <a>", "F6", FileOperationType::Copy, "Sao l\u{01b0}u", true),
        act("Bin", "F7", FileOperationType::Recycle, "", false),
    ];
    assert_eq!(
        rows(&deserialize(&serialize(&list), DEFAULT_NAME).unwrap()),
        rows(&list)
    );
    assert_eq!(
        rows(&deserialize(&serialize_for_export(&list), DEFAULT_NAME).unwrap()),
        rows(&list)
    );
}

#[test]
fn deserialize_missing_properties_take_initialisers() {
    // new: T9 / rule 18 (golden `deserialize-empty-object-takes-defaults`).
    let list = deserialize("[{}]", DEFAULT_NAME).unwrap();
    assert_eq!(
        rows(&list),
        rows(&[act(DEFAULT_NAME, "Enter", FileOperationType::Move, "Group-2", false)])
    );
}

#[test]
fn deserialize_property_names_are_case_sensitive() {
    // new: T9 (golden `deserialize-property-names-are-case-sensitive`): `name` does not match `Name`.
    let list = deserialize(
        "[{\"name\":\"x\",\"shortcut\":\"F6\",\"operation\":\"Copy\"}]",
        DEFAULT_NAME,
    )
    .unwrap();
    assert_eq!(
        rows(&list),
        rows(&[act(DEFAULT_NAME, "Enter", FileOperationType::Move, "Group-2", false)])
    );
}

#[test]
fn deserialize_unknown_property_is_ignored() {
    // new: rule 18 (golden `deserialize-unknown-property-ignored`).
    let text = "[{\"Name\":\"A\",\"Shortcut\":\"F6\",\"Extra\":123,\"Operation\":\"Copy\"}]";
    let list = deserialize(text, DEFAULT_NAME).unwrap();
    assert_eq!(
        rows(&list),
        rows(&[act("A", "F6", FileOperationType::Copy, "Group-2", false)])
    );
}

#[test]
fn deserialize_operation_is_lenient() {
    // new: T11, names, numbers and the alias `Delete` (golden `legacy-delete-alias`, `numeric-operation`).
    for (json, want) in [
        ("\"Delete\"", FileOperationType::Recycle),
        ("\"Recycle\"", FileOperationType::Recycle),
        ("1", FileOperationType::Copy),
        ("0", FileOperationType::Move),
    ] {
        let text = format!("[{{\"Name\":\"A\",\"Shortcut\":\"F6\",\"Operation\":{json}}}]");
        let list = deserialize(&text, DEFAULT_NAME).unwrap();
        assert_eq!(list[0].operation, want, "{json}");
    }
}

#[test]
fn deserialize_literal_null_is_empty_and_or_null_gives_none() {
    // new: rules 18 and 19 (golden `deserialize-literal-null-is-empty`, `deserialize-or-null-literal-null`).
    assert_eq!(deserialize("null", DEFAULT_NAME), Ok(vec![]));
    assert_eq!(deserialize_or_null("null", DEFAULT_NAME), Ok(None));
    assert_eq!(deserialize_or_null("[]", DEFAULT_NAME), Ok(Some(vec![])));
}

#[test]
fn deserialize_rejects_malformed_text_and_wrong_shapes() {
    // new: rules 18 and 19 (golden `malformed-text`, `object-instead-of-list`, `or-null-malformed`).
    for text in ["[{\"Name\":", "{\"Name\":\"A\"}", "not json", "[1]", "\"x\""] {
        assert!(
            matches!(deserialize(text, DEFAULT_NAME), Err(ActionListJsonError::Malformed(_))),
            "{text}"
        );
        assert!(
            matches!(
                deserialize_or_null(text, DEFAULT_NAME),
                Err(ActionListJsonError::Malformed(_))
            ),
            "{text}"
        );
    }
}

#[test]
fn deserialize_null_element_is_malformed() {
    // new: T10, a null element is `Malformed` in Rust (C# keeps it and `TryImport` rejects it later).
    assert!(matches!(
        deserialize("[null]", DEFAULT_NAME),
        Err(ActionListJsonError::Malformed(_))
    ));
}

#[test]
fn deserialize_undefined_operation_number_is_malformed() {
    // new: T12, `99` cannot be held by `FileOperationType`.
    let text = "[{\"Name\":\"A\",\"Shortcut\":\"F6\",\"Operation\":99}]";
    assert!(matches!(
        deserialize(text, DEFAULT_NAME),
        Err(ActionListJsonError::Malformed(_))
    ));
}

#[test]
fn deserialize_unknown_operation_name_is_malformed() {
    // new: contract header of `ActionListJsonError::Malformed`: an unknown operation name is `Malformed`.
    let text = "[{\"Name\":\"A\",\"Shortcut\":\"F6\",\"Operation\":\"Teleport\"}]";
    assert!(matches!(
        deserialize(text, DEFAULT_NAME),
        Err(ActionListJsonError::Malformed(_))
    ));
}

#[test]
fn throw_if_invalid_checks_names_shortcuts_and_duplicate_text() {
    // new: rule 20 and T1 (golden `throw-*`).
    const FIRST: &str = "An action has no name, an invalid shortcut or an invalid operation.";
    const SECOND: &str = "Two actions use the same shortcut.";
    assert_eq!(throw_if_invalid(&[]), Ok(()));
    assert_eq!(throw_if_invalid(&[mv("A", "F6"), mv("B", "F7")]), Ok(()));
    assert_eq!(
        throw_if_invalid(&[mv(" ", "F6")]),
        Err(ActionListJsonError::Invalid(FIRST))
    );
    assert_eq!(
        throw_if_invalid(&[mv("A", "Nope")]),
        Err(ActionListJsonError::Invalid(FIRST))
    );
    assert_eq!(
        throw_if_invalid(&[mv("A", "  ")]),
        Err(ActionListJsonError::Invalid(FIRST))
    );
    assert_eq!(
        throw_if_invalid(&[mv("A", "F6"), mv("B", "f6")]),
        Err(ActionListJsonError::Invalid(SECOND))
    );
    assert_eq!(
        throw_if_invalid(&[mv("A", "Return"), mv("B", "Enter")]),
        Err(ActionListJsonError::Invalid(SECOND))
    );
}
