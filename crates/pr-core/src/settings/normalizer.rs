//! Repair of freshly loaded settings, shortcut validation, and the start-up repair text.
//!
//! Port of `PhotoReview.Core/Settings/SettingsNormalizer.cs`, `SettingsValidator.cs`, `SettingsLoadRepairText.cs`
//! and `SettingsParseResult.cs` (baseline d92d6aae; `DisableConflictingOptionalShortcuts` is new since e441d67a).
//! Contract: `contracts/W2-02.md`.
//!
//! Mapping of the C# static classes: `SettingsNormalizer.Normalize` is [`normalize`],
//! `SettingsNormalizer.DisableConflictingOptionalShortcuts` is [`disable_conflicting_optional_shortcuts`],
//! `SettingsLoadRepairText.Build` is [`build_repair_text`], `SettingsValidator` is [`SettingsValidator`].
//!
//! The C# code reflects over the `ShortcutMappings` properties; here use `ShortcutMappings::PROPERTY_NAMES`,
//! `OPTIONAL_NAMES`, `is_optional`, `get` and `get_mut` (module `settings::shortcuts`).

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use crate::abstractions::KeyNameValidator;
use crate::model::enums::{
    DecoderBackend, FitWidthAnchor, ImageSortMode, InitialViewMode, KeyboardZoomAnchor, KineticGlideSmoothing,
    LoadingMode, MiddleClickAction, ScalingQuality,
};
use crate::settings::app_settings::{
    AppSettings, DEFAULT_INFO_OVERLAY_FONT_SIZE, MAX_ARROW_PAN_STEP_PERCENT, MAX_CLICK_ZOOM_PERCENT,
    MAX_IMAGE_TRANSITION_MS, MAX_INFO_OVERLAY_AUTO_HIDE_DELAY_MS, MAX_INFO_OVERLAY_FONT_SIZE,
    MAX_KEYBOARD_ZOOM_STEP_PERCENT, MAX_TOOLBAR_AUTO_HIDE_DELAY_MS, MAX_TOOLBAR_OPACITY_PERCENT,
    MAX_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE, MIN_ARROW_PAN_STEP_PERCENT, MIN_CLICK_ZOOM_PERCENT, MIN_IMAGE_TRANSITION_MS,
    MIN_INFO_OVERLAY_AUTO_HIDE_DELAY_MS, MIN_INFO_OVERLAY_FONT_SIZE, MIN_KEYBOARD_ZOOM_STEP_PERCENT,
    MIN_TOOLBAR_AUTO_HIDE_DELAY_MS, MIN_TOOLBAR_OPACITY_PERCENT, MIN_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE, ReadNotes,
    SettingsJsonError, SettingsTexts,
};
use crate::settings::context_menu;
use crate::settings::perf_options as perf;
use crate::settings::shortcuts::{ShortcutMappings, canonicalize};
use crate::text::ignore_case_key;

/// Prefix of the repair entries that name a TURNED OFF optional shortcut (`Shortcuts.<Name>`), as opposed to a reset
/// setting. `SettingsParseResult.Repairs` adds it; `SettingsLoadRepairText` splits on it (ordinal, case-sensitive).
pub const SHORTCUT_REPAIR_PREFIX: &str = "Shortcuts.";

fn clamp_i32(value: &mut i32, min: i32, max: i32, name: &str, fixed: &mut Vec<String>) {
    let clamped = (*value).clamp(min, max);
    if clamped != *value {
        *value = clamped;
        fixed.push(name.to_owned());
    }
}

fn reset_if_none<T>(slot: &mut Option<T>, default: T, name: &str, fixed: &mut Vec<String>) {
    if slot.is_none() {
        *slot = Some(default);
        fixed.push(name.to_owned());
    }
}

/// `SettingsNormalizer.Normalize`: repairs `settings` in place; returns the names of the settings that were reset or
/// clamped (empty when all valid), in the order of the checks (the contract lists the order; the golden
/// `settings-normalize` pins it). Entries can also be `Actions[<i>].Shortcut` (a user action lost the key that a
/// blank mandatory shortcut got back). Also clears `settings.read_notes` (a dropped `null` action counts as the
/// `Actions` repair, once).
///
/// Calls `AppSettings::canonicalize_all` in the middle of the sequence (before the mouse/zoom checks) and uses
/// `context_menu::parse_hidden` / `to_stored` for the hidden-menu-items list.
pub fn normalize(settings: &mut AppSettings) -> Vec<String> {
    let mut fixed: Vec<String> = Vec::new();
    let mut pending_reset_keys: HashSet<String> = HashSet::new();

    if settings.image_cache_capacity_bytes <= 0 {
        settings.image_cache_capacity_bytes = perf::IMAGE_CACHE_CAPACITY_BYTES;
        fixed.push("ImageCacheCapacityBytes".to_owned());
    }
    clamp_i32(
        &mut settings.image_cache_ram_percent,
        perf::MIN_IMAGE_CACHE_RAM_PERCENT,
        perf::MAX_IMAGE_CACHE_RAM_PERCENT,
        "ImageCacheRamPercent",
        &mut fixed,
    );
    if settings.source_bytes_capacity_bytes <= 0 {
        settings.source_bytes_capacity_bytes = perf::SOURCE_BYTES_CAPACITY_BYTES;
        fixed.push("SourceBytesCapacityBytes".to_owned());
    }
    if settings.memory_reserve_bytes < 0 {
        settings.memory_reserve_bytes = perf::MEMORY_RESERVE_BYTES;
        fixed.push("MemoryReserveBytes".to_owned());
    }
    if settings.preview_disk_cache_capacity_bytes < 0 {
        settings.preview_disk_cache_capacity_bytes = perf::PREVIEW_DISK_CACHE_CAPACITY_BYTES;
        fixed.push("PreviewDiskCacheCapacityBytes".to_owned());
    }
    if settings.preload_worker_count <= 0 {
        settings.preload_worker_count = perf::PRELOAD_WORKER_COUNT;
        fixed.push("PreloadWorkerCount".to_owned());
    } else if settings.preload_worker_count > perf::MAX_PRELOAD_WORKER_COUNT {
        settings.preload_worker_count = perf::MAX_PRELOAD_WORKER_COUNT;
        fixed.push("PreloadWorkerCount".to_owned());
    }
    let limit = settings.preload_memory_load_limit;
    // NaN fails both comparisons and is reset, like C#.
    if !(limit > 0.0 && limit <= 1.0) {
        settings.preload_memory_load_limit = perf::PRELOAD_MEMORY_LOAD_LIMIT;
        fixed.push("PreloadMemoryLoadLimit".to_owned());
    }
    clamp_i32(
        &mut settings.preload_forward_count,
        perf::MIN_PRELOAD_FORWARD_COUNT,
        perf::MAX_PRELOAD_COUNT,
        "PreloadForwardCount",
        &mut fixed,
    );
    clamp_i32(
        &mut settings.preload_backward_count,
        perf::MIN_PRELOAD_BACKWARD_COUNT,
        perf::MAX_PRELOAD_COUNT,
        "PreloadBackwardCount",
        &mut fixed,
    );

    if settings.initial_view_mode == InitialViewMode::Percent400 {
        settings.initial_view_mode = InitialViewMode::Percent200;
        fixed.push("InitialViewMode".to_owned());
    }
    reset_if_none(
        &mut settings.loading_mode,
        LoadingMode::Preview,
        "LoadingMode",
        &mut fixed,
    );
    reset_if_none(
        &mut settings.image_sort_mode,
        ImageSortMode::Default,
        "ImageSortMode",
        &mut fixed,
    );
    reset_if_none(
        &mut settings.scaling_quality,
        ScalingQuality::HighQuality,
        "ScalingQuality",
        &mut fixed,
    );
    if matches!(settings.decoder_backend, None | Some(DecoderBackend::LibRaw)) {
        settings.decoder_backend = Some(DecoderBackend::WicDirect);
        fixed.push("DecoderBackend".to_owned());
    }
    // JournalDurability, InstanceMode, FitWidthAnchor: plain enums, cannot be undefined in Rust.
    reset_if_none(
        &mut settings.keyboard_zoom_anchor,
        KeyboardZoomAnchor::ViewportCentre,
        "KeyboardZoomAnchor",
        &mut fixed,
    );
    reset_if_none(
        &mut settings.fit_width_anchor2,
        FitWidthAnchor::BottomThird,
        "FitWidthAnchor2",
        &mut fixed,
    );
    reset_if_none(
        &mut settings.middle_click_action,
        MiddleClickAction::ActualSize,
        "MiddleClickAction",
        &mut fixed,
    );

    // Actions: null entries were dropped by the reader (counted in read_notes); an undefined operation cannot occur.
    if settings.read_notes.null_actions > 0 {
        fixed.push("Actions".to_owned());
    }

    // Mandatory shortcuts: a blank value falls back to its default.
    let defaults = ShortcutMappings::default();
    let mut reset_shortcut = false;
    for name in ShortcutMappings::PROPERTY_NAMES {
        if ShortcutMappings::is_optional(name) || name == "MoveToFolder2" {
            continue;
        }
        let blank = settings.shortcuts.get(name).is_none_or(|v| v.trim().is_empty());
        if !blank {
            continue;
        }
        let Some(default_value) = defaults.get(name) else {
            continue;
        };
        if let Some(slot) = settings.shortcuts.get_mut(name) {
            *slot = default_value.to_owned();
            reset_shortcut = true;
            pending_reset_keys.insert(ignore_case_key(&canonicalize(Some(default_value))));
        }
    }
    if reset_shortcut {
        fixed.push("Shortcuts".to_owned());
    }
    for name in ShortcutMappings::OPTIONAL_NAMES {
        if let Some(slot) = settings.shortcuts.get_mut(name) {
            *slot = slot.trim().to_owned();
        }
    }

    settings.canonicalize_all();

    // RV-S03: an action that owns a key restored for a blank mandatory shortcut loses it.
    if !pending_reset_keys.is_empty() {
        for (i, action) in settings.actions.iter_mut().enumerate() {
            let key = canonicalize(Some(&action.shortcut));
            if key.is_empty() || !pending_reset_keys.contains(&ignore_case_key(&key)) {
                continue;
            }
            action.shortcut = String::new();
            fixed.push(format!("Actions[{i}].Shortcut"));
        }
    }

    // MouseWheelAction: plain enum, never undefined in Rust.
    reset_if_none(
        &mut settings.kinetic_glide_smoothing,
        KineticGlideSmoothing::Predict,
        "KineticGlideSmoothing",
        &mut fixed,
    );
    clamp_i32(
        &mut settings.click_zoom_percent,
        MIN_CLICK_ZOOM_PERCENT,
        MAX_CLICK_ZOOM_PERCENT,
        "ClickZoomPercent",
        &mut fixed,
    );
    clamp_i32(
        &mut settings.arrow_pan_step_percent,
        MIN_ARROW_PAN_STEP_PERCENT,
        MAX_ARROW_PAN_STEP_PERCENT,
        "ArrowPanStepPercent",
        &mut fixed,
    );
    clamp_i32(
        &mut settings.keyboard_zoom_step_percent,
        MIN_KEYBOARD_ZOOM_STEP_PERCENT,
        MAX_KEYBOARD_ZOOM_STEP_PERCENT,
        "KeyboardZoomStepPercent",
        &mut fixed,
    );
    clamp_i32(
        &mut settings.toolbar_auto_hide_delay_ms,
        MIN_TOOLBAR_AUTO_HIDE_DELAY_MS,
        MAX_TOOLBAR_AUTO_HIDE_DELAY_MS,
        "ToolbarAutoHideDelayMs",
        &mut fixed,
    );
    clamp_i32(
        &mut settings.info_overlay_auto_hide_delay_ms,
        MIN_INFO_OVERLAY_AUTO_HIDE_DELAY_MS,
        MAX_INFO_OVERLAY_AUTO_HIDE_DELAY_MS,
        "InfoOverlayAutoHideDelayMs",
        &mut fixed,
    );
    clamp_i32(
        &mut settings.toolbar_opacity_percent,
        MIN_TOOLBAR_OPACITY_PERCENT,
        MAX_TOOLBAR_OPACITY_PERCENT,
        "ToolbarOpacityPercent",
        &mut fixed,
    );
    let font = settings.info_overlay_font_size;
    let new_font = if font.is_finite() {
        font.clamp(MIN_INFO_OVERLAY_FONT_SIZE, MAX_INFO_OVERLAY_FONT_SIZE)
    } else {
        DEFAULT_INFO_OVERLAY_FONT_SIZE
    };
    // NaN != NaN is true, so a NaN size is always reported (as in C#).
    if new_font != font {
        settings.info_overlay_font_size = new_font;
        fixed.push("InfoOverlayFontSize".to_owned());
    }
    // ImageTransition: plain enum, never undefined in Rust.
    clamp_i32(
        &mut settings.image_transition_ms,
        MIN_IMAGE_TRANSITION_MS,
        MAX_IMAGE_TRANSITION_MS,
        "ImageTransitionMs",
        &mut fixed,
    );
    // RawFullDecode, RawPairMode: plain enums, never undefined in Rust.
    clamp_i32(
        &mut settings.touchpad_swipe_distance_per_image,
        MIN_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE,
        MAX_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE,
        "TouchpadSwipeDistancePerImage",
        &mut fixed,
    );

    if let Some(list) = &settings.hidden_context_menu_items {
        let canonical = context_menu::to_stored(context_menu::parse_hidden(list));
        let same = canonical.len() == list.len()
            && canonical
                .iter()
                .zip(list.iter())
                .all(|(c, l)| l.as_deref() == Some(c.as_str()));
        if !same {
            settings.hidden_context_menu_items = Some(canonical.into_iter().map(Some).collect());
            fixed.push("HiddenContextMenuItems".to_owned());
        }
    }

    // Not a setting: a second `normalize` must report nothing.
    settings.read_notes = ReadNotes::default();
    fixed
}

/// `SettingsNormalizer.DisableConflictingOptionalShortcuts`: turns off (sets to `""`) every optional shortcut whose
/// canonical key is already taken, and canonicalises (trims, resolves aliases, null becomes `""`) the optional ones
/// that stay. Returns the property names it disabled, in `ShortcutMappings::OPTIONAL_NAMES` order.
///
/// A key is taken by the 13 mandatory shortcuts (not the legacy `MoveToFolder2`), by every action shortcut, and by an
/// optional shortcut handled earlier in `OPTIONAL_NAMES` order. Keys compare ignoring case
/// (`pr_core::text::ignore_case_key`).
pub fn disable_conflicting_optional_shortcuts(settings: &mut AppSettings) -> Vec<String> {
    let mut disabled = Vec::new();
    let mut taken: HashSet<String> = HashSet::new();
    let mut take = |key: &str| {
        if !key.trim().is_empty() {
            taken.insert(ignore_case_key(&canonicalize(Some(key))));
        }
    };
    for name in ShortcutMappings::PROPERTY_NAMES {
        if ShortcutMappings::is_optional(name) || name == "MoveToFolder2" {
            continue;
        }
        if let Some(value) = settings.shortcuts.get(name) {
            take(value);
        }
    }
    for action in &settings.actions {
        take(&action.shortcut);
    }
    for name in ShortcutMappings::OPTIONAL_NAMES {
        let Some(slot) = settings.shortcuts.get_mut(name) else {
            continue;
        };
        let key = canonicalize(Some(slot.as_str()));
        if key.is_empty() {
            slot.clear();
            continue;
        }
        if taken.insert(ignore_case_key(&key)) {
            *slot = key;
        } else {
            disabled.push(name.to_owned());
            slot.clear();
        }
    }
    disabled
}

/// `SettingsLoadRepairText.Build(repairs, forImport)`: the start-up dialog text for `SettingsStore.LastLoadRepairs`.
///
/// Repairs not starting with [`SHORTCUT_REPAIR_PREFIX`] are the "reset" ones: if any, ONE first line
/// `texts.dialog_import_settings_repaired(items)` (`for_import`) or `texts.settings_load_repaired(items)`, with
/// `items` = those names joined with `", "`. Then one line `texts.settings_load_shortcut_disabled(name)` per prefixed
/// repair, in order, `name` = the text after the prefix. Lines are joined with CRLF (`Environment.NewLine`).
/// No repairs gives `""`.
pub fn build_repair_text(repairs: &[String], for_import: bool, texts: &dyn SettingsTexts) -> String {
    let reset: Vec<&str> = repairs
        .iter()
        .map(String::as_str)
        .filter(|r| !r.starts_with(SHORTCUT_REPAIR_PREFIX))
        .collect();
    let mut lines: Vec<String> = Vec::new();
    if !reset.is_empty() {
        let items = reset.join(", ");
        lines.push(if for_import {
            texts.dialog_import_settings_repaired(&items)
        } else {
            texts.settings_load_repaired(&items)
        });
    }
    for repair in repairs {
        if let Some(name) = repair.strip_prefix(SHORTCUT_REPAIR_PREFIX) {
            lines.push(texts.settings_load_shortcut_disabled(name));
        }
    }
    lines.join("\r\n")
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
        let mut bindings: Vec<(String, String)> = Vec::new();
        for name in ShortcutMappings::PROPERTY_NAMES {
            if name == "MoveToFolder2" {
                continue;
            }
            let value = settings.shortcuts.get(name).unwrap_or("").trim();
            let blank = value.is_empty();
            if blank && ShortcutMappings::is_optional(name) {
                continue;
            }
            if blank || !self.key_validator.is_valid_key_name(value) {
                return Some(self.texts.core_settings_shortcut_invalid(name));
            }
            bindings.push((name.to_owned(), canonicalize(Some(value))));
        }
        for action in &settings.actions {
            let shortcut = action.shortcut.trim();
            if action.name.trim().is_empty() || shortcut.is_empty() || !self.key_validator.is_valid_key_name(shortcut) {
                return Some(self.texts.core_settings_action_invalid());
            }
            bindings.push((
                self.texts.core_settings_action_binding_name(&action.name),
                canonicalize(Some(&action.shortcut)),
            ));
        }
        // Group by OrdinalIgnoreCase key in first-occurrence order; the first group with more than one member wins.
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for (i, (_, key)) in bindings.iter().enumerate() {
            let group_key = ignore_case_key(key);
            match groups.iter_mut().find(|(k, _)| *k == group_key) {
                Some((_, members)) => members.push(i),
                None => groups.push((group_key, vec![i])),
            }
        }
        let (_, members) = groups.iter().find(|(_, members)| members.len() > 1)?;
        let first_key = &bindings[members[0]].1;
        let names: Vec<&str> = members.iter().map(|&i| bindings[i].0.as_str()).collect();
        Some(
            self.texts
                .core_settings_shortcut_duplicate(first_key, &names.join(", ")),
        )
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
        let mut out = self.reset_settings.clone();
        out.extend(
            self.disabled_shortcuts
                .iter()
                .map(|name| format!("{SHORTCUT_REPAIR_PREFIX}{name}")),
        );
        out
    }
}
