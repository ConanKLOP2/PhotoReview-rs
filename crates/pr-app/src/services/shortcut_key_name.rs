//! Port of `Services/ShortcutKeyName.cs` (28 lines) and the retired `Services/WpfKeyNameValidator.cs` (9 lines, replaced by
//! [`KeyIdNameValidator`]). Contract: `contracts/W8-04c.md`, rules R-SK-*.
//!
//! The one place that turns a configured shortcut string into a [`KeyId`] (the WPF-free mirror of `System.Windows.Input.Key`, C-06).
//! Name lookup is by the enum's own names (aliases included), never by number: `Enum.TryParse` alone accepts "999" and "A,B", and
//! names that can never be pressed as a command (Escape, Tab, modifiers) would silently never fire.

#![allow(unused_variables, dead_code)] // D11: stub bodies are todo!(); removed with the last todo!() of this file.

use pr_core::abstractions::KeyNameValidator;

use crate::input::key_id::KeyId; // W8-05b: needs `KeyId::ALL` (every variant, aliases included) and `KeyId::name(self) -> &'static str`

/// C# `ShortcutKeyName.TryParse(string? name, out KeyId key)`. `Some(key)` = usable as a shortcut; `None` = refused (C# `false`).
///
/// Order (R-SK-1): trim; canonicalise through `pr_core::settings::shortcuts::canonicalize` (only when the trimmed text is not empty);
/// refuse empty, a comma, a leading digit, a leading `-` or `+`; look the name up case-insensitively among `KeyId::ALL`; refuse
/// reserved keys ([`is_reserved`]).
pub fn try_parse(name: Option<&str>) -> Option<KeyId> {
    todo!()
}

/// C# `ShortcutKeyName.IsReserved(KeyId)`: keys the capture box must let through (focus, cancel) and that can never be a
/// command: `None`, `Escape`, `Tab`, `ImeProcessed`, `DeadCharProcessed`, `System`, `LeftCtrl`, `RightCtrl`, `LeftAlt`, `RightAlt`,
/// `LeftShift`, `RightShift`, `LWin`, `RWin` (R-SK-2). Also used by `SettingsPresenter` (K-32 validation).
pub fn is_reserved(key: KeyId) -> bool {
    todo!()
}

/// Replaces the retired C# `WpfKeyNameValidator` (9 lines, `IKeyNameValidator` backed by `ShortcutKeyName.TryParse`). The composition root
/// (W8-09a/b) registers it wherever `SettingsStore` takes a `KeyNameValidator`. Behaviour: `is_valid_key_name(s)` = `try_parse(Some(s))`
/// is `Some` (R-SK-5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyIdNameValidator;

impl KeyNameValidator for KeyIdNameValidator {
    fn is_valid_key_name(&self, key_name: &str) -> bool {
        todo!()
    }
}
