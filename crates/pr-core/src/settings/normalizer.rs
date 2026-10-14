//! Repair of freshly loaded settings, shortcut validation, and the start-up repair text.
//!
//! Port of `PhotoReview.Core/Settings/SettingsNormalizer.cs`, `SettingsValidator.cs`, `SettingsLoadRepairText.cs`
//! and `SettingsParseResult.cs` (baseline d92d6aae; `DisableConflictingOptionalShortcuts` is new since e441d67a).
//! Contract: `contracts/W2-02/contract.md`.
//!
//! Mapping of the C# static classes: `SettingsNormalizer.Normalize` is [`normalize`],
//! `SettingsNormalizer.DisableConflictingOptionalShortcuts` is [`disable_conflicting_optional_shortcuts`],
//! `SettingsLoadRepairText.Build` is [`build_repair_text`], `SettingsValidator` is [`SettingsValidator`].
//!
//! The C# code reflects over the `ShortcutMappings` properties; here use `ShortcutMappings::PROPERTY_NAMES`,
//! `OPTIONAL_NAMES`, `is_optional`, `get` and `get_mut` (module `settings::shortcuts`).
#![allow(unused_variables, dead_code)] // D11: stub with todo!() bodies; removed by the porter in the commit that removes the last todo!().

use std::fmt;
use std::sync::Arc;

use crate::abstractions::KeyNameValidator;
use crate::settings::app_settings::{AppSettings, SettingsJsonError, SettingsTexts};

/// Prefix of the repair entries that name a TURNED OFF optional shortcut (`Shortcuts.<Name>`), as opposed to a reset
/// setting. `SettingsParseResult.Repairs` adds it; `SettingsLoadRepairText` splits on it (ordinal, case-sensitive).
pub const SHORTCUT_REPAIR_PREFIX: &str = "Shortcuts.";

/// `SettingsNormalizer.Normalize`: repairs `settings` in place; returns the names of the settings that were reset or
/// clamped (empty when all valid), in the order of the checks (the contract lists the order; the golden
/// `settings-normalize` pins it). Entries can also be `Actions[<i>].Shortcut` (a user action lost the key that a
/// blank mandatory shortcut got back). Also clears `settings.read_notes` (a dropped `null` action counts as the
/// `Actions` repair, once).
///
/// Calls `AppSettings::canonicalize_all` in the middle of the sequence (before the mouse/zoom checks) and uses
/// `context_menu::parse_hidden` / `to_stored` for the hidden-menu-items list.
pub fn normalize(settings: &mut AppSettings) -> Vec<String> {
    todo!("W2-02")
}

/// `SettingsNormalizer.DisableConflictingOptionalShortcuts`: turns off (sets to `""`) every optional shortcut whose
/// canonical key is already taken, and canonicalises (trims, resolves aliases, null becomes `""`) the optional ones
/// that stay. Returns the property names it disabled, in `ShortcutMappings::OPTIONAL_NAMES` order.
///
/// A key is taken by the 13 mandatory shortcuts (not the legacy `MoveToFolder2`), by every action shortcut, and by an
/// optional shortcut handled earlier in `OPTIONAL_NAMES` order. Keys compare ignoring case
/// (`pr_core::text::ignore_case_key`).
pub fn disable_conflicting_optional_shortcuts(settings: &mut AppSettings) -> Vec<String> {
    todo!("W2-02")
}

/// `SettingsLoadRepairText.Build(repairs, forImport)`: the start-up dialog text for `SettingsStore.LastLoadRepairs`.
///
/// Repairs not starting with [`SHORTCUT_REPAIR_PREFIX`] are the "reset" ones: if any, ONE first line
/// `texts.dialog_import_settings_repaired(items)` (`for_import`) or `texts.settings_load_repaired(items)`, with
/// `items` = those names joined with `", "`. Then one line `texts.settings_load_shortcut_disabled(name)` per prefixed
/// repair, in order, `name` = the text after the prefix. Lines are joined with CRLF (`Environment.NewLine`).
/// No repairs gives `""`.
pub fn build_repair_text(repairs: &[String], for_import: bool, texts: &dyn SettingsTexts) -> String {
    todo!("W2-02")
}

/// `SettingsValidator`: checks that every shortcut is a valid key name and that no key is bound twice.
pub struct SettingsValidator {
    key_validator: Arc<dyn KeyNameValidator>,
    texts: Arc<dyn SettingsTexts>,
}

impl fmt::Debug for SettingsValidator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsValidator").finish_non_exhaustive()
    }
}

impl SettingsValidator {
    /// `new SettingsValidator(keyValidator)` (C# also takes the ambient texts; here they are explicit).
    pub fn new(key_validator: Arc<dyn KeyNameValidator>, texts: Arc<dyn SettingsTexts>) -> Self {
        Self { key_validator, texts }
    }

    /// `ValidateShortcuts(settings)`: `None` when valid, otherwise the first problem as localized text.
    ///
    /// Order: the 28 shortcut properties in `ShortcutMappings::PROPERTY_NAMES` order (skipping `MoveToFolder2`): blank
    /// optional ones are skipped, a blank mandatory one or one the key validator rejects gives
    /// `texts.core_settings_shortcut_invalid(property)`. Then every action: a blank name, blank shortcut or a shortcut
    /// the validator rejects gives `texts.core_settings_action_invalid()`. Finally the first duplicate group (see the contract) gives
    /// `texts.core_settings_shortcut_duplicate(key, names)`.
    pub fn validate_shortcuts(&self, settings: &AppSettings) -> Option<String> {
        todo!("W2-02")
    }
}

/// `SettingsParseResult`: the outcome of `SettingsStore::parse_text`.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsParseResult {
    /// The parsed, migrated and normalized settings.
    pub settings: AppSettings,
    /// The properties whose value could not be read (reset to default).
    pub salvaged: Vec<String>,
    /// `salvaged` plus the names `normalize` reset (each name once).
    pub reset_settings: Vec<String>,
    /// Optional shortcuts turned off for a key collision (property names).
    pub disabled_shortcuts: Vec<String>,
    /// The `JsonException` that made the reader salvage property by property; `None` when the whole text read fine.
    pub salvage_cause: Option<SettingsJsonError>,
    /// The text was the JSON literal `null`.
    pub is_json_null: bool,
}

impl SettingsParseResult {
    /// `Repairs`: what the user is told: `reset_settings` first, then `"Shortcuts." + name` for each disabled shortcut
    /// (the order `SettingsStore.LastLoadRepairs` uses).
    pub fn repairs(&self) -> Vec<String> {
        todo!("W2-02")
    }
}
