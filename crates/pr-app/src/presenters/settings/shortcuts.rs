//! Port of `PhotoReview.App.Shared/Presenters/SettingsPresenter.cs` (C# baseline `d92d6aae`), W8-10e, shortcut helpers of the
//! Shortcuts page: `ZoomShortcutSummary` (lines 207 to 221) and `CaptureShortcut` (lines 223 to 229). Contract: `contracts/W8-10e.md`.
//!
//! Both are STATIC in C#, so they are associated functions without `self`. (The live duplicate warning is in `validate.rs`;
//! the form <-> `ShortcutMappings` conversions are `presenters::settings_field_map`, B1.)

use std::collections::BTreeMap;

use pr_core::settings::shortcuts::canonicalize;

use super::load::SettingsPresenter;
use crate::input::key_id::KeyId;
use crate::presenters::settings_field_map::FieldContext;
use crate::services::shortcut_key_name::is_reserved;

/// The eight zoom shortcuts in display order: label key, `ShortcutMappings` property name.
const ZOOM_LINES: [(&str, &str); 8] = [
    ("settings.shortcut.clickZoom", "ClickZoom"),
    ("settings.shortcut.toggleFit", "ToggleFit"),
    ("settings.shortcut.zoomActualSize", "ZoomActualSize"),
    ("settings.shortcut.zoomIn", "ZoomIn"),
    ("settings.shortcut.zoomOut", "ZoomOut"),
    ("settings.shortcut.fitWidth", "FitWidth"),
    ("settings.shortcut.fitWidth2", "FitWidth2"),
    ("settings.shortcut.fitHeight", "FitHeight"),
];

impl SettingsPresenter {
    /// C# static `ZoomShortcutSummary(shortcutTexts)`: the zoom card's read-only list of the eight zoom shortcuts as typed. Eight
    /// lines joined with `"\r\n"` (`Environment.NewLine` on Windows, which is the only target), each `"{label}: {key}"` with the
    /// label from `ctx.texts.get(..)` and `key` = the typed text TRIMMED, or `settings.zoomShortcuts.none` when the property has no
    /// entry or its text is blank (`trim().is_empty()`). Order: ClickZoom, ToggleFit, ZoomActualSize, ZoomIn, ZoomOut, FitWidth,
    /// FitWidth2, FitHeight.
    pub fn zoom_shortcut_summary(shortcut_texts: &BTreeMap<String, String>, ctx: &FieldContext<'_>) -> String {
        let key = |property: &str| match shortcut_texts.get(property) {
            Some(text) if !text.trim().is_empty() => text.trim().to_owned(),
            _ => ctx.texts.get("settings.zoomShortcuts.none"),
        };
        ZOOM_LINES
            .iter()
            .map(|(label, property)| format!("{}: {}", ctx.texts.get(label), key(property)))
            .collect::<Vec<_>>()
            .join("\r\n")
    }

    /// C# static `CaptureShortcut(key, keyName)`: the shortcut capture box. `None` when the key must not be swallowed
    /// (`services::shortcut_key_name::is_reserved(key)`: Tab, Escape, the modifiers, IME and dead-char markers keep focus movement and
    /// Esc-cancel); else `Some(pr_core::settings::shortcuts::canonicalize(Some(key_name)))`. `key_name` is the platform's name for the
    /// key AFTER an IME's `ImeProcessed` was replaced by the real key (the caller's job).
    pub fn capture_shortcut(key: KeyId, key_name: &str) -> Option<String> {
        if is_reserved(key) {
            None
        } else {
            Some(canonicalize(Some(key_name)))
        }
    }
}
