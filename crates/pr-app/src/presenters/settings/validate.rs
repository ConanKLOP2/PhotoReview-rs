//! Port of `PhotoReview.App.Shared/Presenters/SettingsPresenter.cs` (C# baseline `d92d6aae`), W8-10d, second half: `TrySave`
//! (lines 124 to 179: validate, write into the working copy, persist, switch language) and the live shortcut-conflict preview
//! (lines 181 to 205). Contract: `contracts/W8-10d.md`.
//!
//! `impl SettingsPresenter` block; the struct lives in `load.rs`. Synchronous and UI-thread only (D-W8-2): the store write is a
//! blocking call, exactly as in C#.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, PoisonError};

use pr_core::settings::app_settings::AppSettings;
use pr_core::settings::normalizer::SettingsValidator;
use pr_core::settings::shortcuts::{ReviewAction, ShortcutMappings, canonicalize};
use pr_core::text::{eq_ignore_case, ignore_case_key};

use super::load::SettingsPresenter;
use crate::localization::language_options::{LanguageOption, to_setting};
use crate::misc::app_log::AppLog;
use crate::presenters::action_list_json::deserialize;
use crate::presenters::action_profiles::find_destination_problem;
use crate::presenters::settings_field_map::{
    FieldContext, bindings, read_shortcuts, shortcut_properties, shortcut_text,
};
use crate::presenters::settings_form_state::{
    SettingsFocusField, SettingsFormState, SettingsSaveOutcome, SettingsSaveResult,
};
use crate::services::shortcut_key_name::{KeyIdNameValidator, try_parse};

impl SettingsPresenter {
    /// C# `TrySave(state)`: validates `state`, writes it into the working copy, persists it and applies the language switch.
    /// A refusal keeps everything as the user typed it; nothing is persisted unless the result is `Saved`. Steps, in this order
    /// (each `Invalid` ends the call; rows already written stay written, the save is NOT atomic):
    /// 1. log `"Settings save requested"` (info);
    /// 2. MANDATORY shortcuts = the entries of `settings_field_map::shortcut_properties()` that are not
    ///    `ShortcutMappings::is_optional`, read with `settings_field_map::shortcut_text(state, property)`. Invalid
    ///    (`texts.get("dialog.settingsInvalid.shortcuts")`) when any text is refused by `services::shortcut_key_name::try_parse`, OR
    ///    when the canonical forms (`pr_core::settings::shortcuts::canonicalize`) are not pairwise distinct under
    ///    `OrdinalIgnoreCase` (`pr_core::text::ignore_case_key`);
    /// 3. OPTIONAL shortcuts: a text that is not blank (`trim().is_empty()`) and not accepted by `try_parse` gives the same message;
    /// 4. every row of `settings_field_map::bindings()` in table order: `(row.apply)(state, &mut self.settings, ctx)`; the first
    ///    `Some(error)` gives `Invalid(error.message, error.focus)`;
    /// 5. `validate_shortcuts(&self.settings)` (store validator, else the fallback one): `Some(message)` gives `Invalid(message, None)`;
    /// 6. `presenters::action_profiles::find_destination_problem(&settings.actions, ctx.texts)`: `Some(problem)` gives `Invalid(problem, None)`;
    /// 7. with a localization service: `settings.ui_language = language_options::to_setting(selected, Some(&settings.ui_language))`
    ///    where `selected` is the entry of `state.selected_language_code` (this is the C# `state.SelectedLanguage`; the B1 form
    ///    carries the CODE, decision D-W8-10c-1: build a `LanguageOption { code, display_name: String::new() }` from it, or pass
    ///    `None` for no selection);
    /// 8. `store.save(self.settings.clone())` when a store exists. ANY `SettingsSaveError` (C# catches `IOException`,
    ///    `UnauthorizedAccessException`, `NotSupportedException`, `ArgumentException`) logs `"Settings save failed"` (error) and gives
    ///    `SaveFailed` with `error.to_string()`, focus `None`. On `Ok(saved)` the working copy becomes `(*saved).clone()`: C#
    ///    `Persist` canonicalizes the shortcuts and stamps `ConfigVersion` ON the object it is given and keeps that same object as
    ///    `Current`, and the dialog's `Settings` IS that object; Rust `save` takes ownership, so the presenter re-reads the result;
    /// 9. log `"Settings saved"`; with a localization service whose `requested_language()` differs (`OrdinalIgnoreCase`,
    ///    `pr_core::text::eq_ignore_case`) from `settings.ui_language`: `service.switch(Some(&settings.ui_language))` (UI thread);
    /// 10. `Saved`.
    pub fn try_save(&mut self, state: &SettingsFormState, ctx: &FieldContext<'_>) -> SettingsSaveResult {
        let log = AppLog::global();
        log.info("Settings save requested");
        let required_message = || SettingsSaveResult {
            outcome: SettingsSaveOutcome::Invalid,
            message: Some(ctx.texts.get("dialog.settingsInvalid.shortcuts")),
            focus: SettingsFocusField::None,
        };
        let required: Vec<&str> = shortcut_properties()
            .iter()
            .filter(|property| !ShortcutMappings::is_optional(property))
            .map(|property| shortcut_text(state, property))
            .collect();
        let canonical_keys: BTreeSet<String> = required
            .iter()
            .map(|text| ignore_case_key(&canonicalize(Some(text))))
            .collect();
        if required.iter().any(|text| try_parse(Some(text)).is_none()) || canonical_keys.len() != required.len() {
            return required_message();
        }
        // Optional shortcuts may be empty (= feature disabled); non-empty ones still have to be a real key name.
        let optional_junk = shortcut_properties()
            .iter()
            .filter(|property| ShortcutMappings::is_optional(property))
            .map(|property| shortcut_text(state, property))
            .any(|text| !text.trim().is_empty() && try_parse(Some(text)).is_none());
        if optional_junk {
            return required_message();
        }

        for binding in bindings() {
            if let Some(error) = (binding.apply)(state, &mut self.settings, ctx) {
                return Self::invalid(error.message, error.focus);
            }
        }

        let current = self.settings.clone();
        if let Some(message) = self.validate_shortcuts(&current) {
            return Self::invalid(message, SettingsFocusField::None);
        }
        // Destinations typed in the raw actions JSON get the same check as the Action Profiles editor.
        if let Some(problem) = find_destination_problem(&self.settings.actions, ctx.texts) {
            return Self::invalid(problem, SettingsFocusField::None);
        }
        if self.localization.is_some() {
            let selected = state.selected_language_code.as_ref().map(|code| LanguageOption {
                code: code.clone(),
                display_name: String::new(),
            });
            self.settings.ui_language = to_setting(selected.as_ref(), Some(&self.settings.ui_language));
        }
        if let Some(store) = &self.store {
            // A dialog built without a store (tests) has nothing to persist to; the working copy already reflects what was validated.
            match store.save(self.settings.clone()) {
                Ok(saved) => self.settings = (*saved).clone(),
                Err(error) => {
                    log.error("Settings save failed", Some(&error));
                    return SettingsSaveResult {
                        outcome: SettingsSaveOutcome::SaveFailed,
                        message: Some(error.to_string()),
                        focus: SettingsFocusField::None,
                    };
                }
            }
        }
        log.info("Settings saved");
        // The new language applies live (UI thread); every localized text follows the switch.
        if let Some(localization) = &self.localization {
            let mut service = localization.lock().unwrap_or_else(PoisonError::into_inner);
            if !eq_ignore_case(service.requested_language(), &self.settings.ui_language) {
                service.switch(Some(&self.settings.ui_language));
            }
        }
        SettingsSaveResult::saved()
    }

    /// C# private `Invalid(message, focus = None)`.
    fn invalid(message: String, focus: SettingsFocusField) -> SettingsSaveResult {
        SettingsSaveResult {
            outcome: SettingsSaveOutcome::Invalid,
            message: Some(message),
            focus,
        }
    }

    /// C# private `ValidateShortcuts(settings)`: `store.validate_shortcuts(settings)` when a store exists, else a lazily created
    /// `SettingsValidator::new(Arc::new(KeyIdNameValidator), self.settings_texts.clone())` kept in `fallback_validator`.
    fn validate_shortcuts(&mut self, settings: &AppSettings) -> Option<String> {
        match &self.store {
            Some(store) => store.validate_shortcuts(settings),
            None => {
                let texts = Arc::clone(&self.settings_texts);
                self.fallback_validator
                    .get_or_insert_with(|| SettingsValidator::new(Arc::new(KeyIdNameValidator), texts))
                    .validate_shortcuts(settings)
            }
        }
    }

    /// C# `DuplicateShortcutWarning(shortcutTexts, actionsJson)`: the live "two actions use the same key" message for the text
    /// currently typed, or `None`. Builds a probe `AppSettings::new(&*self.settings_texts)` whose `shortcuts` are
    /// `settings_field_map::read_shortcuts(&form)` (a `SettingsFormState::default()` whose `shortcut_texts` are a COPY of the argument,
    /// NOT canonicalized) and whose `actions` are `parsed_actions_for_probe(actions_json)`, then `validate_shortcuts(&probe)`. Runs on
    /// every keystroke, never throws: broken actions JSON means no actions.
    pub fn duplicate_shortcut_warning(
        &mut self,
        shortcut_texts: &BTreeMap<String, String>,
        actions_json: &str,
    ) -> Option<String> {
        let mut form = SettingsFormState::default();
        for (property, text) in shortcut_texts {
            form.shortcut_texts.insert(property.clone(), text.clone());
        }
        let mut probe = AppSettings::new(&*self.settings_texts);
        probe.shortcuts = read_shortcuts(&form);
        probe.actions = self.parsed_actions_for_probe(actions_json);
        self.validate_shortcuts(&probe)
    }

    /// C# private `ParsedActionsForProbe(text)`: the cached list when `text` equals the cached text (compared as a string), else
    /// `action_list_json::deserialize(text, &settings_texts.action_default_name(2))`; an `Err` gives an EMPTY list; either way the
    /// cache (text and list) is replaced, so a broken text is not re-parsed on the next keystroke either.
    fn parsed_actions_for_probe(&mut self, text: &str) -> Vec<ReviewAction> {
        if self.cached_probe_actions_json.as_deref() == Some(text) {
            return self.cached_probe_actions.clone();
        }
        // Invalid JSON while typing: Save's own check reports that separately.
        self.cached_probe_actions = deserialize(text, &self.settings_texts.action_default_name(2)).unwrap_or_default();
        self.cached_probe_actions_json = Some(text.to_owned());
        self.cached_probe_actions.clone()
    }
}
