//! Port of `INaturalComparer.cs` and `IKeyNameValidator.cs`.

use std::cmp::Ordering;

/// Natural ("a2" < "a10") file-name order like Explorer. C# `IComparer<string?>`: `None` is a null string.
pub trait NaturalComparer: Send + Sync {
    fn compare(&self, a: Option<&str>, b: Option<&str>) -> Ordering;
}

/// Whether a key name is valid on the UI platform (shortcut settings).
pub trait KeyNameValidator: Send + Sync {
    fn is_valid_key_name(&self, key_name: &str) -> bool;
}
