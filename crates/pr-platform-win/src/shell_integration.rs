//! Port of `PhotoReview.Platform.Windows/WindowsShellIntegration.cs` (77 lines, baseline `d92d6aae`): `ShellIntegration` over `HKEY_CURRENT_USER\Software\Classes`
//! (W8-09b slice S2, `contracts/W8-09b.md`, decision D-W8B6-151; no earlier unit owned it: `pr_core::abstractions::shell_integration` holds only the trait and the pure
//! `shell_menu_command` values).
//!
//! Writes only `Directory\shell\<VERB>` and `Directory\Background\shell\<VERB>` below the classes root, where `<VERB>` is `shell_menu_command::VERB_KEY_NAME` (`PhotoReviewRs`,
//! R0002: never the C# app's `PhotoReview` key). The root is a constructor argument so tests use a PRIVATE key (`Software\PhotoReviewRsTests\<guid>\Classes`) instead of the
//! user's real one. Per user, no administrator rights. UI thread or worker, no thread affinity.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::fmt;
use std::io;

use pr_core::abstractions::{ShellIntegration, ShellMenuState};

/// C# `DefaultClassesKey`: the real per-user classes root, relative to HKCU.
pub const DEFAULT_CLASSES_KEY: &str = r"Software\Classes";

/// C# `ArgumentException("A registry key path is required.")`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlankClassesKey;

impl fmt::Display for BlankClassesKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("A registry key path is required.")
    }
}

impl std::error::Error for BlankClassesKey {}

/// C# `WindowsShellIntegration`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsShellIntegration {
    classes_key: String,
}

impl WindowsShellIntegration {
    /// C# `new WindowsShellIntegration()`: [`DEFAULT_CLASSES_KEY`].
    pub fn new() -> Self {
        todo!()
    }

    /// C# `new WindowsShellIntegration(classesKey)`: `Err(BlankClassesKey)` for a blank key; trailing `\` characters are trimmed.
    pub fn with_classes_key(classes_key: &str) -> Result<Self, BlankClassesKey> {
        todo!()
    }
}

impl Default for WindowsShellIntegration {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellIntegration for WindowsShellIntegration {
    /// C# `GetState`: `shell_menu_command::evaluate(read_command(FOLDER_VERB), read_command(BACKGROUND_VERB), executable_path)` where `read_command` is the default value of
    /// `<classes>\<verb>\command` (absent key or non-string value = `None`). A blank `executable_path` is C# `ArgumentException`: here `NotRegistered` (the trait has no error
    /// channel; the Settings page only passes `current_exe`).
    fn get_state(&self, executable_path: &str) -> ShellMenuState {
        todo!()
    }

    /// C# `Register`: `is_valid_executable_path(Some(path))` false -> `Err(io::ErrorKind::InvalidInput)` and NOTHING written; else `write_verb` for the folder verb then the background verb
    /// (`CreateSubKey`: default value = `MENU_TEXT`, value `Icon` = `icon(exe)`, subkey `command` default value = `for_folder(exe)` / `for_background(exe)`, all `REG_SZ`). A failure of
    /// either write calls `unregister` (best effort, errors swallowed) and returns the original error: never half of the pair.
    fn register(&self, executable_path: &str) -> io::Result<()> {
        todo!()
    }

    /// C# `Unregister`: opens the classes key for writing; absent = `Ok(())`; `DeleteSubKeyTree` of both verb paths, a missing one is not an error. Nothing else is touched.
    fn unregister(&self) -> io::Result<()> {
        todo!()
    }
}
