//! Slice S4 of unit W8-20b (`contracts/W8-20b.md`): the six whole-shell tests of C# `ShellOverlayIntegrationTests` that `W8-15j` defers ("Deferred to W8-20b: the other six"). Baseline `d92d6aae`,
//! `tests/PhotoReview.Shell.Integration.Tests/Startup/ShellOverlayIntegrationTests.cs` (the two others, the real click on Fit and the Tools anchor, are ported by 15j).
//!
//! Native (`#[ignore = "native: needs desktop"]`). The tests that switch the language or the settings serialise on one `static Mutex` and restore `Localizer::set_current` in a guard (the C#
//! "GlobalState" collection). STUB: the bodies are `todo!()`.
#![cfg(windows)]
#![allow(dead_code, unused_imports)] // stub

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{OpenTarget, SmokeConfig, UiSmokeRun, vk};

/// C# `ToolbarIsDrawnFromTheFirstFrame_AtTheTopLeft_AndGoneAfterClose` (line 62).
#[test]
#[ignore = "native: needs desktop"]
fn toolbar_is_drawn_from_the_first_frame_at_the_top_left_and_gone_after_close() {
    todo!("W8-20b S4: contracts/W8-20b.md")
}

/// C# `SettingsSave_ReevaluatesAutoHide_AndTheToolbarFollowsTheMouse` (line 109).
#[test]
#[ignore = "native: needs desktop"]
fn settings_save_reevaluates_auto_hide_and_the_toolbar_follows_the_mouse() {
    todo!("W8-20b S4: contracts/W8-20b.md")
}

/// C# `LanguageSwitch_UpdatesToolbarTextOnTheUiThread` (line 133).
#[test]
#[ignore = "native: needs desktop"]
fn language_switch_updates_toolbar_text_on_the_ui_thread() {
    todo!("W8-20b S4: contracts/W8-20b.md")
}

/// C# `Close_UnsubscribesTheOverlayFromTheStaticLocalizerEvent` (line 159).
#[test]
#[ignore = "native: needs desktop"]
fn close_unsubscribes_the_overlay_from_the_static_localizer_event() {
    todo!("W8-20b S4: contracts/W8-20b.md")
}

/// C# `Close_UnsubscribesFromTheSettingsStoreAndTheViewModel` (line 188).
#[test]
#[ignore = "native: needs desktop"]
fn close_unsubscribes_from_the_settings_store_and_the_view_model() {
    todo!("W8-20b S4: contracts/W8-20b.md")
}

/// C# `LanguageSwitch_RefreshesTheViewModelTextTheOverlayShows` (line 202).
#[test]
#[ignore = "native: needs desktop"]
fn language_switch_refreshes_the_view_model_text_the_overlay_shows() {
    todo!("W8-20b S4: contracts/W8-20b.md")
}
