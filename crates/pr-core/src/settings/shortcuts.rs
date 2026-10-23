//! Keyboard shortcut settings: the mappings, their canonical key spelling, and the review actions.
//!
//! Port of `PhotoReview.Core/Settings/ShortcutMappings.cs`, `ShortcutKeyCanonical.cs` and `ReviewAction.cs`
//! (baseline d92d6aae). Contract: `contracts/W1-04/contract.md`.
//!
//! Core knows shortcuts only as strings (WPF `Key` names). Core never validates that a name is a real key (the App
//! layer does that through `IKeyNameValidator`).
//!
//! C# code reflects over the properties of `ShortcutMappings` (`GetProperties()`): the validator, the normalizer and
//! `CanonicalizeAll`. Rust has no reflection, so [`ShortcutMappings::PROPERTY_NAMES`], [`ShortcutMappings::entries`],
//! [`ShortcutMappings::get`] and [`ShortcutMappings::get_mut`] expose the same property list, in declaration order.

use serde::{Deserialize, Serialize};

use crate::model::enums::FileOperationType;

/// The shortcut key for each command (`ShortcutMappings`). Every value is a key NAME such as `"Right"`, `"D1"`,
/// `"F11"`; an empty string means "no shortcut" and is allowed only for the names in [`ShortcutMappings::OPTIONAL_NAMES`].
///
/// JSON: PascalCase property names in declaration order, exactly the C# property names; a missing property keeps its
/// default (`#[serde(default)]`); unknown properties are ignored. Do not reorder the fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ShortcutMappings {
    pub next: String,
    pub previous: String,
    /// Legacy alias of the first action's key; kept for old configs, ignored by the validator.
    pub move_to_folder2: String,
    pub send_to_recycle_bin: String,
    pub compare: String,
    pub next_folder: String,
    pub previous_folder: String,
    pub first_image: String,
    pub zoom_in: String,
    pub zoom_out: String,
    pub toggle_fit: String,
    pub skip: String,
    pub undo: String,
    pub fullscreen: String,
    /// "Move to...": asks for a folder (or reuses the last one) and moves the current photo there. Empty = disabled.
    pub move_to_folder: String,
    /// "Copy to...": same, copying. Empty = disabled.
    pub copy_to_folder: String,
    /// Go to the last image. Empty = disabled.
    pub last_image: String,
    /// Zoom to 100 % = one source pixel per device pixel (ADR 0008). Empty = disabled. `D1` is the "1" key.
    pub zoom_actual_size: String,
    /// Show/hide the on-image info overlays. Empty = disabled.
    pub toggle_info_overlay: String,
    /// Toggles between Fit and the click-zoom percent, anchored at the viewport centre. Empty = disabled. `D2` is the "2" key.
    pub click_zoom: String,
    /// Fits the viewport width exactly. Empty = disabled.
    pub fit_width: String,
    /// Fits the viewport height exactly. Empty = disabled.
    pub fit_height: String,
    /// Toggles keep-zoom-across-images and persists it. Empty = disabled.
    pub toggle_keep_zoom: String,
    /// Q-R42: opens the folder picker; the router requires Ctrl together with this key. Empty = disabled.
    pub open_folder: String,
    /// Q-R43: opens the "Custom zoom..." dialog. Empty = disabled. `D3` is the "3" key.
    pub custom_zoom: String,
    /// Switches between the JPEG and RAW member of the current capture group. Empty = disabled (the default).
    pub toggle_capture_member: String,
    /// A SECOND Fit width command with its own vertical anchor (`AppSettings.FitWidthAnchor2`). Default `D4` (the "4"
    /// key). Empty = disabled. Older configs that already bind `D4` elsewhere get this one disabled by
    /// `SettingsNormalizer.DisableConflictingOptionalShortcuts`.
    pub fit_width2: String,
    /// Q-TOUCHPAD-REFRESH: re-renders the current view at the best quality without moving zoom or scroll. Default `R`
    /// (not `F5`: that is the default key of the "Backup" action). Empty = disabled; a config whose own binding already
    /// owns `R` gets this one disabled by `SettingsNormalizer.DisableConflictingOptionalShortcuts`.
    pub refresh: String,
    /// W9-E1 (JSON `OpenInDefaultEditor`, declared after `Refresh`): sends the photo to the default external editor. Default `""` (disabled, optional).
    pub open_in_default_editor: String,
    /// FEAT-RENAME: opens the rename prompt. Default `F2`; empty disables it.
    pub rename_file: String,
}

impl ShortcutMappings {
    /// The C# property names (the JSON names too), in declaration order: what `GetProperties()` returns.
    pub const PROPERTY_NAMES: [&'static str; 30] = [
        "Next",
        "Previous",
        "MoveToFolder2",
        "SendToRecycleBin",
        "Compare",
        "NextFolder",
        "PreviousFolder",
        "FirstImage",
        "ZoomIn",
        "ZoomOut",
        "ToggleFit",
        "Skip",
        "Undo",
        "Fullscreen",
        "MoveToFolder",
        "CopyToFolder",
        "LastImage",
        "ZoomActualSize",
        "ToggleInfoOverlay",
        "ClickZoom",
        "FitWidth",
        "FitHeight",
        "ToggleKeepZoom",
        "OpenFolder",
        "CustomZoom",
        "ToggleCaptureMember",
        "FitWidth2",
        "Refresh",
        "OpenInDefaultEditor",
        "RenameFile",
    ];

    /// `ShortcutMappings.OptionalNames`: the shortcuts that may be empty (= feature disabled). The older ones are
    /// mandatory: an empty value is invalid. NOT part of [`ShortcutMappings::PROPERTY_NAMES`] (in C# it is a field).
    pub const OPTIONAL_NAMES: [&'static str; 16] = [
        "LastImage",
        "ZoomActualSize",
        "ToggleInfoOverlay",
        "MoveToFolder",
        "CopyToFolder",
        "ClickZoom",
        "FitWidth",
        "FitHeight",
        "ToggleKeepZoom",
        "OpenFolder",
        "CustomZoom",
        "ToggleCaptureMember",
        "FitWidth2",
        "Refresh",
        "OpenInDefaultEditor",
        "RenameFile",
    ];

    /// `ShortcutMappings.IsOptional`: true when `property_name` is one of [`ShortcutMappings::OPTIONAL_NAMES`].
    /// Ordinal and case-sensitive (`StringComparer.Ordinal`): `"lastimage"` and `" LastImage"` are not optional.
    pub fn is_optional(property_name: &str) -> bool {
        Self::OPTIONAL_NAMES.contains(&property_name)
    }

    /// The value of the property called `property_name` (a C# property name, case-sensitive), or `None` when there is
    /// no such property.
    pub fn get(&self, property_name: &str) -> Option<&str> {
        let value = match property_name {
            "Next" => &self.next,
            "Previous" => &self.previous,
            "MoveToFolder2" => &self.move_to_folder2,
            "SendToRecycleBin" => &self.send_to_recycle_bin,
            "Compare" => &self.compare,
            "NextFolder" => &self.next_folder,
            "PreviousFolder" => &self.previous_folder,
            "FirstImage" => &self.first_image,
            "ZoomIn" => &self.zoom_in,
            "ZoomOut" => &self.zoom_out,
            "ToggleFit" => &self.toggle_fit,
            "Skip" => &self.skip,
            "Undo" => &self.undo,
            "Fullscreen" => &self.fullscreen,
            "MoveToFolder" => &self.move_to_folder,
            "CopyToFolder" => &self.copy_to_folder,
            "LastImage" => &self.last_image,
            "ZoomActualSize" => &self.zoom_actual_size,
            "ToggleInfoOverlay" => &self.toggle_info_overlay,
            "ClickZoom" => &self.click_zoom,
            "FitWidth" => &self.fit_width,
            "FitHeight" => &self.fit_height,
            "ToggleKeepZoom" => &self.toggle_keep_zoom,
            "OpenFolder" => &self.open_folder,
            "CustomZoom" => &self.custom_zoom,
            "ToggleCaptureMember" => &self.toggle_capture_member,
            "FitWidth2" => &self.fit_width2,
            "Refresh" => &self.refresh,
            "OpenInDefaultEditor" => &self.open_in_default_editor,
            "RenameFile" => &self.rename_file,
            _ => return None,
        };
        Some(value.as_str())
    }

    /// Mutable access to the property called `property_name`, or `None` when there is no such property.
    pub fn get_mut(&mut self, property_name: &str) -> Option<&mut String> {
        let value = match property_name {
            "Next" => &mut self.next,
            "Previous" => &mut self.previous,
            "MoveToFolder2" => &mut self.move_to_folder2,
            "SendToRecycleBin" => &mut self.send_to_recycle_bin,
            "Compare" => &mut self.compare,
            "NextFolder" => &mut self.next_folder,
            "PreviousFolder" => &mut self.previous_folder,
            "FirstImage" => &mut self.first_image,
            "ZoomIn" => &mut self.zoom_in,
            "ZoomOut" => &mut self.zoom_out,
            "ToggleFit" => &mut self.toggle_fit,
            "Skip" => &mut self.skip,
            "Undo" => &mut self.undo,
            "Fullscreen" => &mut self.fullscreen,
            "MoveToFolder" => &mut self.move_to_folder,
            "CopyToFolder" => &mut self.copy_to_folder,
            "LastImage" => &mut self.last_image,
            "ZoomActualSize" => &mut self.zoom_actual_size,
            "ToggleInfoOverlay" => &mut self.toggle_info_overlay,
            "ClickZoom" => &mut self.click_zoom,
            "FitWidth" => &mut self.fit_width,
            "FitHeight" => &mut self.fit_height,
            "ToggleKeepZoom" => &mut self.toggle_keep_zoom,
            "OpenFolder" => &mut self.open_folder,
            "CustomZoom" => &mut self.custom_zoom,
            "ToggleCaptureMember" => &mut self.toggle_capture_member,
            "FitWidth2" => &mut self.fit_width2,
            "Refresh" => &mut self.refresh,
            "OpenInDefaultEditor" => &mut self.open_in_default_editor,
            "RenameFile" => &mut self.rename_file,
            _ => return None,
        };
        Some(value)
    }

    /// Every `(C# property name, value)` pair in declaration order; always 28 entries.
    pub fn entries(&self) -> Vec<(&'static str, &str)> {
        Self::PROPERTY_NAMES
            .iter()
            .map(|&name| (name, self.get(name).unwrap_or_default()))
            .collect()
    }
}

impl Default for ShortcutMappings {
    /// `ShortcutMappings.Default()` / `new ShortcutMappings()`: the property initialisers.
    fn default() -> Self {
        Self {
            next: "Right".to_owned(),
            previous: "Left".to_owned(),
            move_to_folder2: "Enter".to_owned(),
            send_to_recycle_bin: "Delete".to_owned(),
            compare: "C".to_owned(),
            next_folder: "PageDown".to_owned(),
            previous_folder: "PageUp".to_owned(),
            first_image: "Home".to_owned(),
            zoom_in: "Add".to_owned(),
            zoom_out: "Subtract".to_owned(),
            toggle_fit: "F".to_owned(),
            skip: "Space".to_owned(),
            undo: "Z".to_owned(),
            fullscreen: "F11".to_owned(),
            move_to_folder: "M".to_owned(),
            copy_to_folder: "Y".to_owned(),
            last_image: "End".to_owned(),
            zoom_actual_size: "D1".to_owned(),
            toggle_info_overlay: "I".to_owned(),
            click_zoom: "D2".to_owned(),
            fit_width: "W".to_owned(),
            fit_height: "H".to_owned(),
            toggle_keep_zoom: "K".to_owned(),
            open_folder: "O".to_owned(),
            custom_zoom: "D3".to_owned(),
            toggle_capture_member: String::new(),
            fit_width2: "D4".to_owned(),
            refresh: "R".to_owned(),
            open_in_default_editor: String::new(),
            rename_file: "F2".to_owned(),
        }
    }
}

/// `ShortcutKeyCanonical.AliasPairs`: `(alias, canonical)` for every WPF `Key` name that shares its value with another
/// name. Order and spelling are part of the contract (a golden pins them). Different keys that merely look alike
/// (`Add` and `OemPlus`) are NOT aliases.
pub const ALIAS_PAIRS: [(&str, &str); 29] = [
    ("Return", "Enter"),
    ("Prior", "PageUp"),
    ("Next", "PageDown"),
    ("Capital", "CapsLock"),
    ("Snapshot", "PrintScreen"),
    ("KanaMode", "HangulMode"),
    ("HanjaMode", "KanjiMode"),
    ("Oem1", "OemSemicolon"),
    ("Oem2", "OemQuestion"),
    ("Oem3", "OemTilde"),
    ("Oem4", "OemOpenBrackets"),
    ("Oem5", "OemPipe"),
    ("Oem6", "OemCloseBrackets"),
    ("Oem7", "OemQuotes"),
    ("Oem102", "OemBackslash"),
    ("OemAttn", "DbeAlphanumeric"),
    ("OemFinish", "DbeKatakana"),
    ("OemCopy", "DbeHiragana"),
    ("OemAuto", "DbeSbcsChar"),
    ("OemEnlw", "DbeDbcsChar"),
    ("OemBackTab", "DbeRoman"),
    ("Attn", "DbeNoRoman"),
    ("CrSel", "DbeEnterWordRegisterMode"),
    ("ExSel", "DbeEnterImeConfigureMode"),
    ("EraseEof", "DbeFlushString"),
    ("Play", "DbeCodeInput"),
    ("Zoom", "DbeNoCodeInput"),
    ("NoName", "DbeDetermineString"),
    ("Pa1", "DbeEnterDialogConversionMode"),
];

/// `ShortcutKeyCanonical.Canonicalize`: the one spelling used for a shortcut everywhere.
///
/// `None` and blank text give `""` (= no shortcut). Otherwise the text is trimmed, and when it equals an alias or a
/// canonical name of [`ALIAS_PAIRS`] (ASCII case-insensitive; see the contract about non-ASCII text) the canonical
/// name is returned, which also repairs the casing (`"enter"` gives `"Enter"`). Any other text is returned trimmed and
/// otherwise unchanged (`"f11"` stays `"f11"`). Idempotent.
pub fn canonicalize(name: Option<&str>) -> String {
    let Some(text) = name.map(str::trim) else {
        return String::new();
    };
    if text.is_empty() {
        return String::new();
    }
    // C# looks the text up in an OrdinalIgnoreCase dictionary over ASCII keys: non-ASCII text never matches
    // (golden rows: Kelvin sign, dotless i, fullwidth letters). Compare ASCII-only, not `text::eq_ignore_case`.
    if !text.is_ascii() {
        return text.to_owned();
    }
    for &(alias, canonical) in &ALIAS_PAIRS {
        if alias.eq_ignore_ascii_case(text) || canonical.eq_ignore_ascii_case(text) {
            return canonical.to_owned();
        }
    }
    text.to_owned()
}

/// `ShortcutKeyCanonical.CanonicalizeAll` for the parts of `AppSettings` that exist in this unit: canonicalises every
/// one of the 28 shortcut strings of `shortcuts` and the `shortcut` of every action, in place. (The `AppSettings`
/// wrapper that also handles a missing `Shortcuts`/`Actions` is owned by the settings unit.)
pub fn canonicalize_all(shortcuts: &mut ShortcutMappings, actions: &mut [ReviewAction]) {
    for name in ShortcutMappings::PROPERTY_NAMES {
        if let Some(value) = shortcuts.get_mut(name) {
            *value = canonicalize(Some(value.as_str()));
        }
    }
    for action in actions {
        action.shortcut = canonicalize(Some(action.shortcut.as_str()));
    }
}

/// One user-defined review action: a key that moves, copies or recycles the current photo (`ReviewAction`).
///
/// JSON: `{ "Name", "Shortcut", "Operation", "Destination", "Confirm" }` (PascalCase, this order). Only `Serialize` is
/// derived here: reading needs the localized default `Name`, so the settings unit decides how to deserialize it
/// (see `contract.md`, open questions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReviewAction {
    /// Display name. C# default is the localized `Tr.ActionDefaultName(2)`.
    pub name: String,
    /// Shortcut key name. Default `"Enter"`.
    pub shortcut: String,
    /// Default `Move`.
    pub operation: FileOperationType,
    /// Destination folder (relative names such as `"Group-2"` are language-neutral). Default `"Group-2"`.
    pub destination: String,
    /// Ask before running. Default `false`.
    pub confirm: bool,
}

impl ReviewAction {
    /// A new action with the C# property initialisers: shortcut `"Enter"`, operation `Move`, destination `"Group-2"`,
    /// no confirmation, and the given (already localized) name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            shortcut: "Enter".to_owned(),
            operation: FileOperationType::Move,
            destination: "Group-2".to_owned(),
            confirm: false,
        }
    }

    /// `ReviewAction.Defaults()`: the four default actions, in this order.
    ///
    /// | name                          | shortcut | operation | destination |
    /// |-------------------------------|----------|-----------|-------------|
    /// | `action_default_name(2)`      | `Enter`  | `Move`    | `Group-2`   |
    /// | `action_default_name(3)`      | `F3`     | `Move`    | `Group-3`   |
    /// | `action_default_name(4)`      | `F4`     | `Move`    | `Group-4`   |
    /// | `backup_name`                 | `F5`     | `Copy`    | `Backup`    |
    ///
    /// `confirm` is `false` everywhere. C# reads the names from the language catalog (`Tr.ActionDefaultName(n)`,
    /// `Tr.ActionDefaultBackup`); this crate cannot, so the caller passes them in.
    pub fn defaults(action_default_name: &dyn Fn(u32) -> String, backup_name: &str) -> Vec<ReviewAction> {
        vec![
            ReviewAction {
                name: action_default_name(2),
                shortcut: "Enter".to_owned(),
                operation: FileOperationType::Move,
                destination: "Group-2".to_owned(),
                confirm: false,
            },
            ReviewAction {
                name: action_default_name(3),
                shortcut: "F3".to_owned(),
                operation: FileOperationType::Move,
                destination: "Group-3".to_owned(),
                confirm: false,
            },
            ReviewAction {
                name: action_default_name(4),
                shortcut: "F4".to_owned(),
                operation: FileOperationType::Move,
                destination: "Group-4".to_owned(),
                confirm: false,
            },
            ReviewAction {
                name: backup_name.to_owned(),
                shortcut: "F5".to_owned(),
                operation: FileOperationType::Copy,
                destination: "Backup".to_owned(),
                confirm: false,
            },
        ]
    }
}
