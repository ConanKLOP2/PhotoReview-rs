//! W9-E1: external editor records and the pure rules of the declared-editors list.
//!
//! Port of `PhotoReview.Core/Settings/ExternalEditor.cs` (classes `ExternalEditor` and the static `ExternalEditors`), baseline `7eaee2a2`.
//! Contract: `contracts/W9-E1.md`.
//! `ExternalEditorsEdit` (the editor dialog result) is NOT here: in C# it is internal to `Shell.Win32` (W9-E2).

use super::app_settings::AppSettings;
use crate::catalog::types::{RAW_EXTENSIONS, extension_in, get_extension};
use crate::text::{cmp_ignore_case, dotnet_path};
use std::collections::HashSet;

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
        crate::io::physical::new_guid_n()
    }

    /// C# `new ExternalEditor()`: a fresh `new_id()`, empty `name` and `path`, `arguments` = [`ExternalEditor::DEFAULT_ARGUMENTS`].
    pub fn new() -> Self {
        Self {
            id: Self::new_id(),
            name: String::new(),
            path: String::new(),
            arguments: Self::DEFAULT_ARGUMENTS.to_owned(),
        }
    }

    /// C# `DisplayName`: `name.trim()` when `name` is not blank, else `ExternalEditors::name_from_path(path)`.
    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            ExternalEditors::name_from_path(Some(&self.path))
        } else {
            self.name.trim().to_owned()
        }
    }
}

impl Default for ExternalEditor {
    fn default() -> Self {
        Self::new()
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
        let path = path.unwrap_or("").trim();
        let name = dotnet_path::get_file_name_without_extension(path);
        if name.trim().is_empty() {
            path.to_owned()
        } else {
            name.to_owned()
        }
    }

    /// C# `Effective(settings)`: the list when it is not empty, else (legacy path only) ONE implicit editor `{ id: LEGACY_EDITOR_ID, name: name_from_path(legacy), path: legacy.trim(), arguments: default }`, else empty. C# returns the live list; Rust returns a copy.
    pub fn effective(settings: &AppSettings) -> Vec<ExternalEditor> {
        if !settings.external_editors.is_empty() {
            return settings.external_editors.clone();
        }
        let path = settings.external_editor_path.trim();
        if path.is_empty() {
            Vec::new()
        } else {
            vec![ExternalEditor {
                id: Self::LEGACY_EDITOR_ID.to_owned(),
                name: Self::name_from_path(Some(path)),
                path: path.to_owned(),
                arguments: ExternalEditor::DEFAULT_ARGUMENTS.to_owned(),
            }]
        }
    }

    /// C# `EffectiveDefault(settings)`: the editor of [`ExternalEditors::effective`] whose id equals `default_external_editor_id` (ordinal), else the first, else `None`.
    pub fn effective_default(settings: &AppSettings) -> Option<ExternalEditor> {
        let editors = Self::effective(settings);
        editors
            .iter()
            .find(|editor| editor.id == settings.default_external_editor_id)
            .or_else(|| editors.first())
            .cloned()
    }

    /// C# `Default(settings)`: like `effective_default` but over `settings.external_editors` only (NO legacy implicit editor).
    pub fn default(settings: &AppSettings) -> Option<ExternalEditor> {
        Self::find(settings, Some(&settings.default_external_editor_id))
            .or_else(|| settings.external_editors.first().cloned())
    }

    /// C# `Find(settings, id)`: the editor of `settings.external_editors` with this id (ordinal); `None` id or no match -> `None`.
    pub fn find(settings: &AppSettings, id: Option<&str>) -> Option<ExternalEditor> {
        let id = id?;
        settings.external_editors.iter().find(|editor| editor.id == id).cloned()
    }

    /// C# `Reconcile(settings)`: legacy migration, repair, default fix, `external_editor_path` mirror. Returns true when an EXISTING list was repaired (the silent legacy migration and the mirror do not count).
    pub fn reconcile(settings: &mut AppSettings) -> bool {
        // Keep C# repair reporting without nullable entries in the public Rust list.
        let mut repaired = std::mem::take(&mut settings.read_notes.null_external_editors) != 0;
        let mut clean = Vec::new();
        let mut ids = HashSet::new();
        for mut editor in std::mem::take(&mut settings.external_editors) {
            if editor.path.trim().is_empty() || clean.len() >= Self::MAX_EDITORS {
                repaired = true;
                continue;
            }
            if editor.id.trim().is_empty() || !ids.insert(editor.id.clone()) {
                loop {
                    editor.id = ExternalEditor::new_id();
                    if ids.insert(editor.id.clone()) {
                        break;
                    }
                }
                repaired = true;
            }
            if editor.arguments.trim().is_empty() {
                editor.arguments = ExternalEditor::DEFAULT_ARGUMENTS.to_owned();
            }
            if editor.arguments.encode_utf16().count() > Self::MAX_ARGUMENTS_LENGTH {
                editor.arguments = ExternalEditor::DEFAULT_ARGUMENTS.to_owned();
                repaired = true;
            }
            editor.path = editor.path.trim().to_owned();
            editor.name = editor.name.trim().to_owned();
            clean.push(editor);
        }
        let legacy = settings.external_editor_path.trim();
        if clean.is_empty() && !legacy.is_empty() {
            let editor = ExternalEditor {
                name: Self::name_from_path(Some(legacy)),
                path: legacy.to_owned(),
                ..ExternalEditor::new()
            };
            settings.default_external_editor_id = editor.id.clone();
            clean.push(editor);
        }
        if clean.is_empty() {
            repaired |= !settings.default_external_editor_id.is_empty();
            settings.default_external_editor_id.clear();
        } else if !clean
            .iter()
            .any(|editor| editor.id == settings.default_external_editor_id)
        {
            repaired |= !settings.default_external_editor_id.is_empty();
            settings.default_external_editor_id = clean[0].id.clone();
        }
        settings.external_editors = clean;
        settings.external_editor_path = Self::default(settings).map_or_else(String::new, |editor| editor.path);
        repaired
    }

    /// C# `ApplyLegacyPath(settings, legacyPath)`: the WPF-window hand-off (changed path moves/removes/creates the default editor, then `reconcile`).
    pub fn apply_legacy_path(settings: &mut AppSettings, legacy_path: Option<&str>) {
        let path = legacy_path.unwrap_or("").trim();
        let current = Self::default(settings);
        if current.as_ref().map_or("", |editor| editor.path.as_str()) == path {
            return;
        }
        if let Some(current) = current {
            if path.is_empty() {
                if let Some(index) = settings
                    .external_editors
                    .iter()
                    .position(|editor| editor.id == current.id)
                {
                    settings.external_editors.remove(index);
                }
            } else if let Some(editor) = settings
                .external_editors
                .iter_mut()
                .find(|editor| editor.id == current.id)
            {
                if editor.name == Self::name_from_path(Some(&editor.path)) {
                    editor.name = Self::name_from_path(Some(path));
                }
                editor.path = path.to_owned();
            }
        } else if !path.is_empty() {
            let editor = ExternalEditor {
                name: Self::name_from_path(Some(path)),
                path: path.to_owned(),
                ..ExternalEditor::new()
            };
            settings.default_external_editor_id = editor.id.clone();
            settings.external_editors.push(editor);
        }
        Self::reconcile(settings);
    }

    /// C# `BuildArguments(template, filePath)`. `None` = C# `ArgumentException.ThrowIfNullOrEmpty(filePath)` (empty `file_path`; a caller bug, never user text, so not a panic).
    pub fn build_arguments(template: Option<&str>, file_path: &str) -> Option<String> {
        if file_path.is_empty() {
            return None;
        }
        let template = template.unwrap_or("").trim();
        let mut remaining = if template.is_empty() {
            ExternalEditor::DEFAULT_ARGUMENTS
        } else {
            template
        };
        let mut escaped = String::new();
        let mut backslashes = 0;
        for ch in file_path.chars() {
            if ch == '\\' {
                escaped.push(ch);
                backslashes += 1;
                continue;
            }
            if ch == '"' {
                escaped.extend(std::iter::repeat_n('\\', backslashes + 1));
            }
            escaped.push(ch);
            backslashes = 0;
        }
        escaped.extend(std::iter::repeat_n('\\', backslashes));
        let quoted = format!("\"{escaped}\"");
        let mut result = String::new();
        let mut in_quotes = false;
        let mut replaced = false;
        while !remaining.is_empty() {
            if let Some(rest) = remaining.strip_prefix("{file}") {
                result.push_str(if in_quotes { &escaped } else { &quoted });
                remaining = rest;
                replaced = true;
            } else if let Some(ch) = remaining.chars().next() {
                result.push(ch);
                if ch == '"' {
                    in_quotes = !in_quotes;
                }
                remaining = &remaining[ch.len_utf8()..];
            }
        }
        if !replaced {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(&quoted);
        }
        Some(result)
    }

    /// C# `ResolveFileToSend(path, pairedRawPath, sendRawOriginal, fileExists)`.
    pub fn resolve_file_to_send(
        path: &str,
        paired_raw_path: Option<&str>,
        send_raw_original: bool,
        file_exists: &dyn Fn(&str) -> bool,
    ) -> String {
        if !send_raw_original || extension_in(RAW_EXTENSIONS, get_extension(path)) {
            return path.to_owned();
        }
        if let Some(paired) = paired_raw_path.filter(|paired| !paired.trim().is_empty()) {
            if file_exists(paired) {
                return paired.to_owned();
            }
        }
        let Some(folder) = dotnet_path::get_directory_name(path).filter(|folder| !folder.is_empty()) else {
            return path.to_owned();
        };
        let stem = dotnet_path::get_file_name_without_extension(path);
        if stem.trim().is_empty() {
            return path.to_owned();
        }
        let mut extensions = RAW_EXTENSIONS.to_vec();
        extensions.sort_by(|left, right| cmp_ignore_case(left, right));
        let separator = if folder.ends_with(['\\', '/', ':']) { "" } else { "\\" };
        for extension in extensions {
            let candidate = format!("{folder}{separator}{stem}{extension}");
            if file_exists(&candidate) {
                return candidate;
            }
        }
        path.to_owned()
    }
}
