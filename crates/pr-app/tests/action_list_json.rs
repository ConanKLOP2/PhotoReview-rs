//! Red tests for `pr_app::presenters::action_list_json` (W8-10a, contract `contracts/W8-10a.md`): the bodies are `todo!()`.
//! Golden `action-list-json` (26 cases) plus hand-written tests for the rules and traps T8 to T12.
//! There is no C# test of `ActionListJson` by name; every test here is `// new:`.

mod presenters_support;

use pr_app::presenters::action_list_json::{
    ActionListJsonError, deserialize, deserialize_or_null, serialize, serialize_for_export, throw_if_invalid,
};
use pr_core::model::enums::FileOperationType;
use pr_core::settings::shortcuts::ReviewAction;
use presenters_support::{act, actions_of, golden_pairs, mv, rows};

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
                let want = expected["text"].as_str().unwrap().to_owned();
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
    // new: rule 16, two-space indent, CRLF line ends, order Name, Shortcut, Operation, Destination, Confirm.
    let text = serialize(&[act("Copy it", "F6", FileOperationType::Copy, "Backup", true)]);
    let want = "[\r\n  {\r\n    \"Name\": \"Copy it\",\r\n    \"Shortcut\": \"F6\",\r\n    \"Operation\": \"Copy\",\r\n    \"Destination\": \"Backup\",\r\n    \"Confirm\": true\r\n  }\r\n]";
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

fn one_operation(operation: &str) -> String {
    format!("[{{\"Name\":\"A\",\"Shortcut\":\"F6\",\"Operation\":{operation}}}]")
}

// C#: ActionListJsonTests.Deserialize_UnknownOperation_Throws (5 InlineData rows, both entry points)
#[test]
fn deserialize_unknown_operation_throws() {
    for operation in ["\"Teleport\"", "99", "\"\"", "null", "true"] {
        let text = one_operation(operation);
        assert!(
            matches!(deserialize(&text, DEFAULT_NAME), Err(ActionListJsonError::Malformed(_))),
            "deserialize {operation}"
        );
        assert!(
            matches!(
                deserialize_or_null(&text, DEFAULT_NAME),
                Err(ActionListJsonError::Malformed(_))
            ),
            "deserialize_or_null {operation}"
        );
    }
}

// C#: ActionListJsonTests.Deserialize_KnownOperation_Parses (6 InlineData rows: names, case variants, the alias, numbers, a numeric string)
#[test]
fn deserialize_known_operation_parses() {
    for (operation, expected) in [
        ("\"Move\"", FileOperationType::Move),
        ("\"copy\"", FileOperationType::Copy),
        ("\"Delete\"", FileOperationType::Recycle),
        ("\"Recycle\"", FileOperationType::Recycle),
        ("1", FileOperationType::Copy),
        ("\"2\"", FileOperationType::Recycle),
    ] {
        let list =
            deserialize(&one_operation(operation), DEFAULT_NAME).unwrap_or_else(|e| panic!("{operation}: {e:?}"));
        assert_eq!(list.len(), 1, "{operation}");
        assert_eq!(list[0].operation, expected, "{operation}");
    }
}

// C#: ActionListJsonTests.Deserialize_MissingOperation_UsesDefault
#[test]
fn deserialize_missing_operation_uses_default() {
    let list = deserialize("[{\"Name\":\"A\",\"Shortcut\":\"F6\"}]", DEFAULT_NAME).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].operation, FileOperationType::default());
    assert_eq!(deserialize("null", DEFAULT_NAME), Ok(vec![]));
    assert_eq!(deserialize_or_null("null", DEFAULT_NAME), Ok(None));
}

// new (found by the port of ActionListJsonTests.Deserialize_KnownOperation_Parses): C# `LenientEnumConverter.TryReadStrict` trims the
// text, matches names/aliases ignoring case and accepts int.TryParse text that names a defined member, so `" Move "`, `"+1"` and
// `"\t2 "` are NOT errors (the former Rust reader rejected every string that was not exactly Move/Copy/Recycle/Delete).
#[test]
fn deserialize_operation_text_is_trimmed_and_numeric_text_is_accepted() {
    for (operation, expected) in [
        ("\" Move \"", FileOperationType::Move),
        ("\"\\tCOPY\\r\\n\"", FileOperationType::Copy),
        ("\"+1\"", FileOperationType::Copy),
        ("\"0\"", FileOperationType::Move),
        ("\"\\t2 \"", FileOperationType::Recycle),
        ("\" delete\"", FileOperationType::Recycle),
    ] {
        let text = one_operation(operation);
        for result in [
            deserialize(&text, DEFAULT_NAME),
            deserialize_or_null(&text, DEFAULT_NAME).map(Option::unwrap),
        ] {
            let list = result.unwrap_or_else(|e| panic!("{operation}: {e:?}"));
            assert_eq!(list[0].operation, expected, "{operation}");
        }
    }
}

// new: the same strict reader rejects blank text, undefined numeric text, fractional/exponent/out-of-range numbers.
#[test]
fn deserialize_operation_blank_undefined_or_fractional_values_throw() {
    for operation in [
        "\" \"",
        "\"3\"",
        "\"-1\"",
        "\"1.0\"",
        "1.0",
        "1e0",
        "2147483648",
        "-1",
        "[]",
        "{}",
        "false",
    ] {
        let text = one_operation(operation);
        assert!(
            matches!(deserialize(&text, DEFAULT_NAME), Err(ActionListJsonError::Malformed(_))),
            "{operation}"
        );
        assert!(
            matches!(
                deserialize_or_null(&text, DEFAULT_NAME),
                Err(ActionListJsonError::Malformed(_))
            ),
            "or_null {operation}"
        );
    }
}

// new: C# `ThrowIfUnknownOperation` finds the property with OrdinalIgnoreCase, so a bad `operation`/`OPERATION` value throws even
// though the serializer itself (case-sensitive names) would have ignored that property.
#[test]
fn deserialize_unknown_operation_under_any_property_case_throws_and_a_known_one_is_ignored() {
    for property in ["operation", "OPERATION", "oPeRaTiOn"] {
        let bad = format!("[{{\"Name\":\"A\",\"Shortcut\":\"F6\",\"{property}\":\"Teleport\"}}]");
        assert!(
            matches!(deserialize(&bad, DEFAULT_NAME), Err(ActionListJsonError::Malformed(_))),
            "{property}"
        );
        let good = format!("[{{\"Name\":\"A\",\"Shortcut\":\"F6\",\"{property}\":\"Copy\"}}]");
        let list = deserialize(&good, DEFAULT_NAME).unwrap();
        assert_eq!(
            list[0].operation,
            FileOperationType::Move,
            "{property}: only the exact name `Operation` is read"
        );
    }
}

// new (mutation killer, `json_string` match arm `'\r'` deleted survived: the generic `\u000D` form was written instead of `\r`).
#[test]
fn serialize_escapes_a_carriage_return_with_its_short_form_in_both_modes() {
    let list = [mv("a\rb", "F8")];
    assert!(serialize(&list).contains("\"Name\": \"a\rb\""), "{}", serialize(&list));
    assert!(
        serialize_for_export(&list).contains("\"Name\": \"a\rb\""),
        "{}",
        serialize_for_export(&list)
    );
}
