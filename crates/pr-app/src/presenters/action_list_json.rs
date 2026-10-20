//! Port of `PhotoReview.App.Shared/Presenters/ActionListJson.cs` (baseline d92d6aae, 53 lines): the JSON form of an action
//! list. The Settings raw-JSON box uses [`serialize`] (relaxed escaping, Vietnamese stays readable); the Action Profiles
//! export file uses [`serialize_for_export`] (default .NET escaping). Contract: `contracts/W8-10a.md`.
use pr_core::model::enums::FileOperationType;
use pr_core::settings::shortcuts::ReviewAction;
use std::collections::HashSet;

/// Failure of the JSON readers and of [`throw_if_invalid`] (C# `JsonException`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionListJsonError {
    /// Malformed JSON, a wrong JSON shape, a null list element or an unknown operation name. The text is the parser's
    /// message; it is never shown to the user.
    Malformed(String),
    /// A Save-time check failed. The text is the English C# message (`An action has no name, an invalid shortcut or an
    /// invalid operation.` or `Two actions use the same shortcut.`); it is never shown to the user.
    Invalid(&'static str),
}

/// C# `Serialize(actions)`: indented (two spaces, `\n` line ends), relaxed escaping (`UnsafeRelaxedJsonEscaping`):
/// non-ASCII text is written as is. Property names in C# order `Name, Shortcut, Operation, Destination, Confirm`;
/// `Operation` is written by name (`Move`, `Copy`, `Recycle`). An empty list is `[]`.
pub fn serialize(actions: &[ReviewAction]) -> String {
    write_actions(actions, false)
}

/// C# `SerializeForExport(actions)`: the same shape as [`serialize`] with DEFAULT escaping (`JavaScriptEncoder.Default`):
/// non-ASCII and the HTML/JS-sensitive ASCII characters are written as `\uXXXX`. The golden fixes the exact set.
pub fn serialize_for_export(actions: &[ReviewAction]) -> String {
    write_actions(actions, true)
}

/// C# `Deserialize(text)`: parses a list. The literal `null` gives an empty list (C# `?? []`). Missing properties take the
/// C# initialiser values (`Name` = `default_name`, which the caller passes as the localized `Tr.ActionDefaultName(2)`,
/// `Shortcut` `"Enter"`, `Operation` `Move`, `Destination` `"Group-2"`, `Confirm` false). Property names match
/// case-sensitively, unknown properties are ignored, `Operation` is read leniently (names, numbers, the alias `Delete`
/// for `Recycle`). Any malformed text is `Err(Malformed)`.
pub fn deserialize(text: &str, default_name: &str) -> Result<Vec<ReviewAction>, ActionListJsonError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| ActionListJsonError::Malformed(error.to_string()))?;
    if value.is_null() {
        return Ok(Vec::new());
    }
    parse_actions(value, default_name)
}

/// C# `DeserializeOrNull(text)`: like [`deserialize`] but the literal `null` gives `Ok(None)` (the importer rejects it as
/// "invalid file"). Everything else as [`deserialize`].
pub fn deserialize_or_null(text: &str, default_name: &str) -> Result<Option<Vec<ReviewAction>>, ActionListJsonError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| ActionListJsonError::Malformed(error.to_string()))?;
    if value.is_null() {
        return Ok(None);
    }
    parse_actions(value, default_name).map(Some)
}

/// C# `ThrowIfInvalid(actions)`, the Save-time checks. Fails with `Invalid` when an action has a blank name, a blank
/// shortcut, or a shortcut that does not parse; and when two actions canonicalize to the same shortcut text under
/// ordinal-ignore-case comparison (this check compares TEXT, unlike `has_invalid_actions`, which compares keys).
pub fn throw_if_invalid(actions: &[ReviewAction]) -> Result<(), ActionListJsonError> {
    let mut seen = HashSet::new();
    for action in actions {
        if action.name.trim().is_empty()
            || action.shortcut.trim().is_empty()
            || super::action_profiles::parse_shortcut(&action.shortcut).is_none()
        {
            return Err(ActionListJsonError::Invalid(
                "An action has no name, an invalid shortcut or an invalid operation.",
            ));
        }
        let key = pr_core::settings::shortcuts::canonicalize(Some(&action.shortcut));
        let key = pr_core::text::ignore_case_key(&key);
        if !seen.insert(key) {
            return Err(ActionListJsonError::Invalid("Two actions use the same shortcut."));
        }
    }
    Ok(())
}

fn parse_actions(value: serde_json::Value, default_name: &str) -> Result<Vec<ReviewAction>, ActionListJsonError> {
    let items = value
        .as_array()
        .ok_or_else(|| ActionListJsonError::Malformed("expected array".to_owned()))?;
    items
        .iter()
        .map(|item| {
            let object = item
                .as_object()
                .ok_or_else(|| ActionListJsonError::Malformed("expected object".to_owned()))?;
            let string = |name: &str, default: &str| -> Result<String, ActionListJsonError> {
                match object.get(name) {
                    None => Ok(default.to_owned()),
                    Some(serde_json::Value::Null) => Ok(String::new()),
                    Some(value) => value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| ActionListJsonError::Malformed(format!("{name} must be a string"))),
                }
            };
            let name = string("Name", default_name)?;
            let shortcut = string("Shortcut", "Enter")?;
            let destination = string("Destination", "Group-2")?;
            let confirm = match object.get("Confirm") {
                None => false,
                Some(value) => value
                    .as_bool()
                    .ok_or_else(|| ActionListJsonError::Malformed("Confirm must be boolean".to_owned()))?,
            };
            let operation = match object.get("Operation") {
                None => FileOperationType::Move,
                Some(serde_json::Value::String(s)) if s.eq_ignore_ascii_case("Move") => FileOperationType::Move,
                Some(serde_json::Value::String(s)) if s.eq_ignore_ascii_case("Copy") => FileOperationType::Copy,
                Some(serde_json::Value::String(s))
                    if s.eq_ignore_ascii_case("Recycle") || s.eq_ignore_ascii_case("Delete") =>
                {
                    FileOperationType::Recycle
                }
                Some(serde_json::Value::Number(n)) => match n.as_i64() {
                    Some(0) => FileOperationType::Move,
                    Some(1) => FileOperationType::Copy,
                    Some(2) => FileOperationType::Recycle,
                    _ => return Err(ActionListJsonError::Malformed("invalid operation".to_owned())),
                },
                Some(_) => return Err(ActionListJsonError::Malformed("invalid operation".to_owned())),
            };
            Ok(ReviewAction {
                name,
                shortcut,
                operation,
                destination,
                confirm,
            })
        })
        .collect()
}

fn write_actions(actions: &[ReviewAction], export: bool) -> String {
    if actions.is_empty() {
        return "[]".to_owned();
    }
    let rows: Vec<String> = actions.iter().map(|a| format!("  {{\r\n    \"Name\": {},\r\n    \"Shortcut\": {},\r\n    \"Operation\": {},\r\n    \"Destination\": {},\r\n    \"Confirm\": {}\r\n  }}", json_string(&a.name, export), json_string(&a.shortcut, export), json_string(match a.operation { FileOperationType::Move => "Move", FileOperationType::Copy => "Copy", FileOperationType::Recycle => "Recycle" }, export), json_string(&a.destination, export), a.confirm)).collect();
    format!("[\r\n{}\r\n]", rows.join(",\r\n"))
}

fn json_string(value: &str, export: bool) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        let code = ch as u32;
        let escaped = match ch {
            '"' if export => Some(format!("\\u{code:04X}")),
            '"' => Some("\\\"".to_owned()),
            '\\' => Some("\\\\".to_owned()),
            '\n' => Some("\\n".to_owned()),
            '\r' => Some("\\r".to_owned()),
            '\t' => Some("\\t".to_owned()),
            c if (c as u32) < 0x20 => Some(format!("\\u{code:04X}")),
            c if export && (!c.is_ascii() || "<>+'&`".contains(c)) => Some(format!("\\u{code:04X}")),
            _ => None,
        };
        if let Some(escaped) = escaped {
            out.push_str(&escaped);
        } else {
            out.push(ch);
        }
    }
    out.push('"');
    out
}
