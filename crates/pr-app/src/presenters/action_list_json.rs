//! Port of `PhotoReview.App.Shared/Presenters/ActionListJson.cs` (baseline d92d6aae, 53 lines): the JSON form of an action
//! list. The Settings raw-JSON box uses [`serialize`] (relaxed escaping, Vietnamese stays readable); the Action Profiles
//! export file uses [`serialize_for_export`] (default .NET escaping). Contract: `contracts/W8-10a.md`.
#![allow(unused_variables, dead_code)] // D11: stub, every body is todo!()

use pr_core::settings::shortcuts::ReviewAction;

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
    todo!()
}

/// C# `SerializeForExport(actions)`: the same shape as [`serialize`] with DEFAULT escaping (`JavaScriptEncoder.Default`):
/// non-ASCII and the HTML/JS-sensitive ASCII characters are written as `\uXXXX`. The golden fixes the exact set.
pub fn serialize_for_export(actions: &[ReviewAction]) -> String {
    todo!()
}

/// C# `Deserialize(text)`: parses a list. The literal `null` gives an empty list (C# `?? []`). Missing properties take the
/// C# initialiser values (`Name` = `default_name`, which the caller passes as the localized `Tr.ActionDefaultName(2)`,
/// `Shortcut` `"Enter"`, `Operation` `Move`, `Destination` `"Group-2"`, `Confirm` false). Property names match
/// case-sensitively, unknown properties are ignored, `Operation` is read leniently (names, numbers, the alias `Delete`
/// for `Recycle`). Any malformed text is `Err(Malformed)`.
pub fn deserialize(text: &str, default_name: &str) -> Result<Vec<ReviewAction>, ActionListJsonError> {
    todo!()
}

/// C# `DeserializeOrNull(text)`: like [`deserialize`] but the literal `null` gives `Ok(None)` (the importer rejects it as
/// "invalid file"). Everything else as [`deserialize`].
pub fn deserialize_or_null(text: &str, default_name: &str) -> Result<Option<Vec<ReviewAction>>, ActionListJsonError> {
    todo!()
}

/// C# `ThrowIfInvalid(actions)`, the Save-time checks. Fails with `Invalid` when an action has a blank name, a blank
/// shortcut, or a shortcut that does not parse; and when two actions canonicalize to the same shortcut text under
/// ordinal-ignore-case comparison (this check compares TEXT, unlike `has_invalid_actions`, which compares keys).
pub fn throw_if_invalid(actions: &[ReviewAction]) -> Result<(), ActionListJsonError> {
    todo!()
}
