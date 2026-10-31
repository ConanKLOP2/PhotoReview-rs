//! CX-SETUI2: one binary for deterministic Settings builder mutation checks.
#![cfg(windows)]

#[path = "settings_builder_inputs.rs"]
mod builder_inputs;
#[path = "w8_setui_killers.rs"]
mod prior_killers;

#[test]
fn builder_diagnostics_preserve_base_item_count_and_all_error_messages() {
    // C# SettingsPageBuilder programmer-error messages and IdBase.
    // inputs.rs:46,75: fmt -> Ok(Default::default()); :104: id_base -> 0/1/-1.
    use pr_platform_win::dialogs::settings::builder::inputs::{SettingsBuilderError, SettingsPageBuilder};
    let mut builder = SettingsPageBuilder::new(8123);
    assert_eq!(builder.id_base(), 8123);
    assert_eq!(
        format!("{builder:?}"),
        "SettingsPageBuilder { id_base: 8123, items: 0, errors: [], .. }"
    );
    builder.radio(&[], None, 0);
    assert_eq!(
        format!("{builder:?}"),
        "SettingsPageBuilder { id_base: 8123, items: 0, errors: [RadioNeedsTwoOptions], .. }"
    );
    for (error, message) in [
        (
            SettingsBuilderError::TooManyControls,
            "A settings page can declare at most 999 controls.",
        ),
        (
            SettingsBuilderError::RadioNeedsTwoOptions,
            "A radio group needs at least two options.",
        ),
        (
            SettingsBuilderError::RadioHintCountMismatch,
            "There must be one hint (or null) per radio option.",
        ),
    ] {
        assert_eq!(error.to_string(), message);
    }
}

#[path = "settings_framework.rs"]
mod framework;
