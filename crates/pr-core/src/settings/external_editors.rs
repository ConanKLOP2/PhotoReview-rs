//! W9-E1: external editor records and the pure rules of the declared-editors list.
//!
//! Port of `PhotoReview.Core/Settings/ExternalEditor.cs` (classes `ExternalEditor` and the static `ExternalEditors`), baseline `7eaee2a2`.
//! Contract: `contracts/W9-E1.md`. Stub: every body is `todo!()`; the signatures are the contract.
//! `ExternalEditorsEdit` (the editor dialog result) is NOT here: in C# it is internal to `Shell.Win32` (W9-E2).

#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use super::app_settings::AppSettings;

/// C# `ExternalEditor`: one declared external image editor. JSON property names (config.json): `Id`, `Name`, `Path`, `Arguments`
/// (`DisplayName` is `[JsonIgnore]`, not persisted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalEditor {
    /// C# `Id`: stable identifier, `Guid.NewGuid().ToString("N")` (32 lower-case hex digits); lets the default survive a rename or a reorder.
    pub id: String,
    /// C# `Name`: menu text; blank = the executable's file name (see [`ExternalEditor::display_name`]).
    pub name: String,
    /// C# `Path`: full path of the editor's executable.
    pub path: String,
    /// C# `Arguments`: command-line template, `{file}` is the photo path. Default [`ExternalEditor::DEFAULT_ARGUMENTS`].
    pub arguments: String,
}

impl ExternalEditor {
    /// C# `DefaultArguments`: `"{file}"` including the two double quotes.
    pub const DEFAULT_ARGUMENTS: &'static str = "\"{file}\"";

    /// C# `ExternalEditor.NewId()`: `crate::io::physical::new_guid_n()`.
    pub fn new_id() -> String {
        todo!()
    }

    /// C# `new ExternalEditor()`: a fresh `new_id()`, empty `name` and `path`, `arguments` = [`ExternalEditor::DEFAULT_ARGUMENTS`].
    pub fn new() -> Self {
        todo!()
    }

    /// C# `DisplayName`: `name.trim()` when `name` is not blank, else `ExternalEditors::name_from_path(path)`.
    pub fn display_name(&self) -> String {
        todo!()
    }
}

/// C# static class `ExternalEditors` (the pure rules). C# `Clone()` is `Clone::clone`.
#[derive(Debug)]
pub struct ExternalEditors;

impl ExternalEditors {
    /// C# `MaxEditors`: hard cap on the list.
    pub const MAX_EDITORS: usize = 32;
    /// C# private `MaxArgumentsLength`: an `arguments` text longer than this (in UTF-16 units, as C# `string.Length`) is replaced by the default and counts as a repair.
    pub const MAX_ARGUMENTS_LENGTH: usize = 4000;
    /// C# `LegacyEditorId`: id of the implicit editor made from a legacy `external_editor_path` set without a list.
    pub const LEGACY_EDITOR_ID: &'static str = "legacy";

    /// C# `NameFromPath(string? path)`: `"Photoshop.exe"` -> `"Photoshop"`; blank path -> `""`; a file name without a stem -> the trimmed path (also for an invalid path: C# `catch ArgumentException`).
    pub fn name_from_path(path: Option<&str>) -> String {
        todo!()
    }

    /// C# `Effective(settings)`: the list when it is not empty, else (legacy path only) ONE implicit editor `{ id: LEGACY_EDITOR_ID, name: name_from_path(legacy), path: legacy.trim(), arguments: default }`, else empty. C# returns the live list; Rust returns a copy.
    pub fn effective(settings: &AppSettings) -> Vec<ExternalEditor> {
        todo!()
    }

    /// C# `EffectiveDefault(settings)`: the editor of [`ExternalEditors::effective`] whose id equals `default_external_editor_id` (ordinal), else the first, else `None`.
    pub fn effective_default(settings: &AppSettings) -> Option<ExternalEditor> {
        todo!()
    }

    /// C# `Default(settings)`: like `effective_default` but over `settings.external_editors` only (NO legacy implicit editor).
    pub fn default(settings: &AppSettings) -> Option<ExternalEditor> {
        todo!()
    }

    /// C# `Find(settings, id)`: the editor of `settings.external_editors` with this id (ordinal); `None` id or no match -> `None`.
    pub fn find(settings: &AppSettings, id: Option<&str>) -> Option<ExternalEditor> {
        todo!()
    }

    /// C# `Reconcile(settings)`: legacy migration, repair, default fix, `external_editor_path` mirror. Returns true when an EXISTING list was repaired (the silent legacy migration and the mirror do not count).
    pub fn reconcile(settings: &mut AppSettings) -> bool {
        todo!()
    }

    /// C# `ApplyLegacyPath(settings, legacyPath)`: the WPF-window hand-off (changed path moves/removes/creates the default editor, then `reconcile`).
    pub fn apply_legacy_path(settings: &mut AppSettings, legacy_path: Option<&str>) {
        todo!()
    }

    /// C# `BuildArguments(template, filePath)`. `None` = C# `ArgumentException.ThrowIfNullOrEmpty(filePath)` (empty `file_path`; a caller bug, never user text, so not a panic).
    pub fn build_arguments(template: Option<&str>, file_path: &str) -> Option<String> {
        todo!()
    }

    /// C# `ResolveFileToSend(path, pairedRawPath, sendRawOriginal, fileExists)`.
    pub fn resolve_file_to_send(
        path: &str,
        paired_raw_path: Option<&str>,
        send_raw_original: bool,
        file_exists: &dyn Fn(&str) -> bool,
    ) -> String {
        todo!()
    }
}
