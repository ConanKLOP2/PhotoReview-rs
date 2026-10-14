//! Keyboard shortcut settings: the mappings, their canonical key spelling, and the review actions.
//!
//! Port of `PhotoReview.Core/Settings/ShortcutMappings.cs`, `ShortcutKeyCanonical.cs` and `ReviewAction.cs`
//! (baseline e441d67a). Contract: `contracts/W1-04/contract.md`.
//!
//! Core knows shortcuts only as strings (WPF `Key` names). Core never validates that a name is a real key (the App
//! layer does that through `IKeyNameValidator`).
//!
//! C# code reflects over the properties of `ShortcutMappings` (`GetProperties()`): the validator, the normalizer and
//! `CanonicalizeAll`. Rust has no reflection, so [`ShortcutMappings::PROPERTY_NAMES`], [`ShortcutMappings::entries`],
//! [`ShortcutMappings::get`] and [`ShortcutMappings::get_mut`] expose the same property list, in declaration order.

// Stub: bodies are `todo!()`. The porter deletes this line together with the `todo!()`s.
#![allow(unused_variables)]

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
}

impl ShortcutMappings {
    /// The C# property names (the JSON names too), in declaration order: what `GetProperties()` returns.
    pub const PROPERTY_NAMES: [&'static str; 26] = [
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
    ];

    /// `ShortcutMappings.OptionalNames`: the shortcuts that may be empty (= feature disabled). The older ones are
    /// mandatory: an empty value is invalid. NOT part of [`ShortcutMappings::PROPERTY_NAMES`] (in C# it is a field).
    pub const OPTIONAL_NAMES: [&'static str; 12] = [
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
    ];

    /// `ShortcutMappings.IsOptional`: true when `property_name` is one of [`ShortcutMappings::OPTIONAL_NAMES`].
    /// Ordinal and case-sensitive (`StringComparer.Ordinal`): `"lastimage"` and `" LastImage"` are not optional.
    pub fn is_optional(property_name: &str) -> bool {
        todo!()
    }

    /// The value of the property called `property_name` (a C# property name, case-sensitive), or `None` when there is
    /// no such property.
    pub fn get(&self, property_name: &str) -> Option<&str> {
        todo!()
    }

    /// Mutable access to the property called `property_name`, or `None` when there is no such property.
    pub fn get_mut(&mut self, property_name: &str) -> Option<&mut String> {
        todo!()
    }

    /// Every `(C# property name, value)` pair in declaration order; always 26 entries.
    pub fn entries(&self) -> Vec<(&'static str, &str)> {
        todo!()
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
    todo!()
}

/// `ShortcutKeyCanonical.CanonicalizeAll` for the parts of `AppSettings` that exist in this unit: canonicalises every
/// one of the 26 shortcut strings of `shortcuts` and the `shortcut` of every action, in place. (The `AppSettings`
/// wrapper that also handles a missing `Shortcuts`/`Actions` is owned by the settings unit.)
pub fn canonicalize_all(shortcuts: &mut ShortcutMappings, actions: &mut [ReviewAction]) {
    todo!()
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
        todo!()
    }
}
