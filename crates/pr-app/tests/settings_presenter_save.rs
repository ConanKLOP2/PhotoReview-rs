//! W8-10d tests of `presenters::settings::validate` (`try_save`, the live duplicate-shortcut preview) and the Save
//! half of the C# `SettingsPresenterTests`. Kind F: hand-written expectations from the C# tests (baseline d92d6aae).
//! Each test names its C# twin in a `// C#:` comment; tests without a twin say `// New test`.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_app::localization::service::{CultureLookup, LocalizationPaths, LocalizationService};
use pr_app::presenters::action_list_json;
use pr_app::presenters::settings::{SettingsPresenter, SettingsPresenterOptions};
use pr_app::presenters::settings_field_map::FieldContext;
use pr_app::presenters::settings_form_state::{
    SettingsFocusField, SettingsFormState, SettingsSaveOutcome, SettingsSaveResult,
};
use pr_core::abstractions::{Clock, FileSystem, Log, ShellIntegration, ShellMenuState};
use pr_core::app_paths::AppPaths;
use pr_core::model::enums::{
    DecoderBackend, FitWidthAnchor, ImageSortMode, InitialViewMode, MiddleClickAction, RawPairMode,
};
use pr_core::settings::app_settings::{
    AppSettings, EnglishSettingsTexts, MAX_CLICK_ZOOM_PERCENT, MAX_TOOLBAR_AUTO_HIDE_DELAY_MS,
    MAX_TOOLBAR_OPACITY_PERCENT, MIN_CLICK_ZOOM_PERCENT,
};
use pr_core::settings::perf_options::MAX_PRELOAD_COUNT;
use pr_core::settings::shortcuts::{ReviewAction, canonicalize};
use pr_core::settings::store::SettingsStore;
use pr_l10n::loader::{UiCulture, current_ui_culture, set_current_ui_culture};
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_l10n::template::{LocArg, NumberCulture};
use pr_test_support::{FakeClock, FakeFileSystem, FsOp, RecordingLog};

const PHYSICAL_MEMORY: i64 = 32 * 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------------------------------------------

struct NoShell;

impl ShellIntegration for NoShell {
    fn get_state(&self, _executable_path: &str) -> ShellMenuState {
        ShellMenuState::NotRegistered
    }
    fn register(&self, _executable_path: &str) -> std::io::Result<()> {
        Ok(())
    }
    fn unregister(&self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The English localizer and the invariant culture, owned for the whole test.
struct Env {
    texts: Arc<Localizer>,
    numbers: NumberCulture,
}

impl Env {
    fn new() -> Self {
        Self {
            texts: built_in_english_localizer(),
            numbers: NumberCulture::invariant(),
        }
    }

    fn ctx(&self) -> FieldContext<'_> {
        FieldContext {
            texts: &self.texts,
            numbers: &self.numbers,
        }
    }

    fn get(&self, key: &str) -> String {
        self.texts.get(key)
    }
}

fn options() -> SettingsPresenterOptions {
    SettingsPresenterOptions::new(PHYSICAL_MEMORY, Arc::new(EnglishSettingsTexts), Arc::new(NoShell))
}

/// C# `NewPresenter(settings)`: no store, no localization, 32 GiB.
fn new_presenter(settings: &AppSettings) -> SettingsPresenter {
    SettingsPresenter::new(settings, options())
}

fn default_settings() -> AppSettings {
    AppSettings::new(&EnglishSettingsTexts)
}

fn ok(presenter: &mut SettingsPresenter, state: &SettingsFormState, env: &Env) -> bool {
    presenter.try_save(state, &env.ctx()).outcome == SettingsSaveOutcome::Saved
}

fn json_of(settings: &AppSettings) -> String {
    settings
        .to_json()
        .unwrap_or_else(|error| format!("to_json failed: {error:?}"))
}

fn text_of(result: &SettingsSaveResult) -> String {
    result.message.clone().unwrap_or_default()
}

/// A store over a fake disk, with the paths the tests assert on.
struct StoreRig {
    fs: Arc<FakeFileSystem>,
    paths: AppPaths,
    store: Arc<SettingsStore>,
}

impl StoreRig {
    fn new() -> Self {
        let fs = Arc::new(FakeFileSystem::new());
        let paths = AppPaths::new(Path::new(r"C:\Fake\LocalAppData"), None, false);
        let store = Arc::new(SettingsStore::new(
            paths.clone(),
            Arc::clone(&fs) as Arc<dyn FileSystem>,
            Arc::new(RecordingLog::new()) as Arc<dyn Log>,
            Arc::new(FakeClock::new()) as Arc<dyn Clock>,
            Arc::new(EnglishSettingsTexts),
        ));
        Self { fs, paths, store }
    }

    fn presenter(&self, settings: &AppSettings) -> SettingsPresenter {
        let mut opts = options();
        opts.store = Some(Arc::clone(&self.store));
        SettingsPresenter::new(settings, opts)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Save: validation and order
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn save_untouched_form_succeeds_and_is_stable() {
    // C#: SettingsPresenterTests.Save_UntouchedForm_Succeeds_AndIsStable (the JSON context is `AppSettings::to_json`)
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let state = presenter.load_fields();
    let first = presenter.try_save(&state, &env.ctx());
    assert_eq!(first.outcome, SettingsSaveOutcome::Saved);
    assert_eq!(first.message, None);

    // The first Save materializes the legacy-derived context-menu list; from then on the form is a fixed point.
    let after_first = json_of(presenter.settings());
    let state = presenter.load_fields();
    assert!(ok(&mut presenter, &state, &env));
    assert_eq!(json_of(presenter.settings()), after_first);
}

#[test]
fn save_bad_number_is_refused_with_focus() {
    // C#: SettingsPresenterTests.Save_BadNumber_IsRefusedWithFocus (10 rows)
    type Setter = fn(&mut SettingsFormState, String);
    let rows: [(&str, Setter, SettingsFocusField); 10] = [
        (
            "ClickZoomPercentText",
            |s, v| s.click_zoom_percent_text = v,
            SettingsFocusField::None,
        ),
        (
            "ArrowPanStepText",
            |s, v| s.arrow_pan_step_text = v,
            SettingsFocusField::ArrowPanStep,
        ),
        (
            "KeyboardZoomStepPercentText",
            |s, v| s.keyboard_zoom_step_percent_text = v,
            SettingsFocusField::KeyboardZoomStep,
        ),
        (
            "ImageTransitionMsText",
            |s, v| s.image_transition_ms_text = v,
            SettingsFocusField::ImageTransitionMs,
        ),
        (
            "TouchpadSwipeDistanceText",
            |s, v| s.touchpad_swipe_distance_text = v,
            SettingsFocusField::TouchpadSwipeDistance,
        ),
        (
            "PreloadForwardText",
            |s, v| s.preload_forward_text = v,
            SettingsFocusField::PreloadForward,
        ),
        (
            "PreloadBackwardText",
            |s, v| s.preload_backward_text = v,
            SettingsFocusField::PreloadBackward,
        ),
        (
            "ToolbarAutoHideDelayText",
            |s, v| s.toolbar_auto_hide_delay_text = v,
            SettingsFocusField::None,
        ),
        (
            "InfoOverlayAutoHideDelayText",
            |s, v| s.info_overlay_auto_hide_delay_text = v,
            SettingsFocusField::InfoOverlayAutoHideDelay,
        ),
        (
            "InfoOverlayFontSizeText",
            |s, v| s.info_overlay_font_size_text = v,
            SettingsFocusField::None,
        ),
    ];
    let env = Env::new();
    for (field, set, focus) in rows {
        let mut presenter = new_presenter(&default_settings());
        let mut state = presenter.load_fields();
        set(&mut state, "not-a-number".to_owned());

        let result = presenter.try_save(&state, &env.ctx());

        assert_eq!(result.outcome, SettingsSaveOutcome::Invalid, "{field}");
        assert!(!text_of(&result).trim().is_empty(), "{field}: message");
        assert_eq!(result.focus, focus, "{field}: focus");
    }
}

#[test]
fn save_number_outside_range_is_refused() {
    // C#: SettingsPresenterTests.Save_NumberOutsideRange_IsRefused (3 rows)
    type Setter = fn(&mut SettingsFormState, String);
    let rows: [(&str, Setter, i32); 3] = [
        ("ClickZoomPercentText", |s, v| s.click_zoom_percent_text = v, 9),
        ("ClickZoomPercentText", |s, v| s.click_zoom_percent_text = v, 801),
        ("PreloadForwardText", |s, v| s.preload_forward_text = v, -1),
    ];
    let env = Env::new();
    for (field, set, value) in rows {
        let mut presenter = new_presenter(&default_settings());
        let mut state = presenter.load_fields();
        set(&mut state, value.to_string());
        assert_eq!(
            presenter.try_save(&state, &env.ctx()).outcome,
            SettingsSaveOutcome::Invalid,
            "{field} = {value}"
        );
    }
}

#[test]
fn save_click_zoom_range_ends_are_accepted() {
    // C#: SettingsPresenterTests.Save_ClickZoomRangeEnds_AreAccepted
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let mut state = presenter.load_fields();
    for value in [MIN_CLICK_ZOOM_PERCENT, MAX_CLICK_ZOOM_PERCENT] {
        state.click_zoom_percent_text = value.to_string();
        assert!(ok(&mut presenter, &state, &env), "{value}");
        assert_eq!(presenter.settings().click_zoom_percent, value);
    }
}

#[test]
fn save_first_refusal_wins_and_earlier_rows_are_written() {
    // C#: SettingsPresenterTests.Save_FirstRefusalWins_AndEarlierRowsAreWritten
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let mut state = presenter.load_fields();
    state.view_mode_index = 4; // written before the numbers are validated
    state.click_zoom_percent_text = "x".to_owned(); // first refusal
    state.arrow_pan_step_text = "y".to_owned(); // would also be refused, but later
    state.preload_forward_text = "z".to_owned();

    let result = presenter.try_save(&state, &env.ctx());

    assert_eq!(result.outcome, SettingsSaveOutcome::Invalid);
    let expected = env.texts.format_with(
        "dialog.settingsInvalid.clickZoomPercent",
        &[
            LocArg::new("min", MIN_CLICK_ZOOM_PERCENT),
            LocArg::new("max", MAX_CLICK_ZOOM_PERCENT),
        ],
        &env.numbers,
    );
    assert_eq!(result.message, Some(expected));
    assert_eq!(result.focus, SettingsFocusField::None);
    assert_eq!(presenter.settings().initial_view_mode, InitialViewMode::Percent100);
}

#[test]
fn save_preload_counts_are_all_or_nothing() {
    // C#: SettingsPresenterTests.Save_PreloadCounts_AreAllOrNothing
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let original = presenter.settings().preload_forward_count;
    let mut state = presenter.load_fields();
    state.preload_forward_text = MAX_PRELOAD_COUNT.to_string();
    state.preload_backward_text = "bad".to_owned();

    let result = presenter.try_save(&state, &env.ctx());

    assert_eq!(result.outcome, SettingsSaveOutcome::Invalid);
    assert_eq!(result.focus, SettingsFocusField::PreloadBackward);
    assert_eq!(presenter.settings().preload_forward_count, original);
}

#[test]
fn save_auto_hide_group_is_all_or_nothing() {
    // C#: SettingsPresenterTests.Save_AutoHideGroup_IsAllOrNothing
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let original_delay = presenter.settings().toolbar_auto_hide_delay_ms;
    let mut state = presenter.load_fields();
    state.toolbar_auto_hide = true;
    state.toolbar_auto_hide_delay_text = MAX_TOOLBAR_AUTO_HIDE_DELAY_MS.to_string();
    state.info_overlay_font_size_text = "NaN".to_owned();

    assert_eq!(
        presenter.try_save(&state, &env.ctx()).outcome,
        SettingsSaveOutcome::Invalid
    );
    assert!(!presenter.settings().toolbar_auto_hide);
    assert_eq!(presenter.settings().toolbar_auto_hide_delay_ms, original_delay);
}

#[test]
fn save_font_size_uses_invariant_culture_and_refuses_infinity() {
    // C#: SettingsPresenterTests.Save_FontSize_UsesInvariantCultureAndRefusesInfinity
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let mut state = presenter.load_fields();
    state.info_overlay_font_size_text = "Infinity".to_owned();
    assert_eq!(
        presenter.try_save(&state, &env.ctx()).outcome,
        SettingsSaveOutcome::Invalid
    );

    state.info_overlay_font_size_text = "14.5".to_owned();
    assert!(ok(&mut presenter, &state, &env));
    assert_eq!(presenter.settings().info_overlay_font_size, 14.5);
}

#[test]
fn save_unknown_combo_indexes_fall_back() {
    // C#: SettingsPresenterTests.Save_UnknownComboIndexes_FallBack
    let env = Env::new();
    let mut settings = default_settings();
    settings.image_sort_mode = Some(ImageSortMode::SizeAscending);
    let mut presenter = new_presenter(&settings);
    let mut state = presenter.load_fields();
    state.view_mode_index = -1;
    state.fit_width_anchor_index = 99;
    state.middle_click_action_index = 99;
    state.sort_mode_tag = None;
    state.decoder_backend_index = 7;
    state.raw_pair_mode_index = 7;

    assert!(ok(&mut presenter, &state, &env));
    let saved = presenter.settings();
    assert_eq!(saved.initial_view_mode, InitialViewMode::Fit);
    assert_eq!(saved.fit_width_anchor, FitWidthAnchor::Centre);
    assert_eq!(saved.middle_click_action_or_default(), MiddleClickAction::ActualSize);
    assert_eq!(saved.image_sort_mode_or_default(), ImageSortMode::Name);
    assert_eq!(saved.decoder_backend_or_default(), DecoderBackend::Wpf);
    assert_eq!(saved.raw_pair_mode, RawPairMode::Separate);
}

#[test]
fn load_percent400_lands_on_200_and_path_is_trimmed() {
    // C#: SettingsPresenterTests.Load_Percent400_LandsOn200_AndPathIsTrimmed
    let env = Env::new();
    let mut settings = default_settings();
    settings.initial_view_mode = InitialViewMode::Percent400;
    let mut presenter = new_presenter(&settings);
    let mut state = presenter.load_fields();
    assert_eq!(state.view_mode_index, 5);

    state.external_editor_path = r"  C:\Tools\editor.exe  ".to_owned();
    assert!(ok(&mut presenter, &state, &env));
    assert_eq!(presenter.settings().initial_view_mode, InitialViewMode::Percent200);
    assert_eq!(presenter.settings().external_editor_path, r"C:\Tools\editor.exe");
}

// ---------------------------------------------------------------------------------------------------------------
// Shortcuts
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn save_duplicate_mandatory_shortcut_is_refused() {
    // C#: SettingsPresenterTests.Save_DuplicateMandatoryShortcut_IsRefused
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let mut state = presenter.load_fields();
    state.shortcut_texts.insert("Next".to_owned(), "Right".to_owned());
    state.shortcut_texts.insert("Previous".to_owned(), "right".to_owned());

    let result = presenter.try_save(&state, &env.ctx());

    assert_eq!(result.outcome, SettingsSaveOutcome::Invalid);
    assert_eq!(result.message, Some(env.get("dialog.settingsInvalid.shortcuts")));
}

#[test]
fn save_mandatory_needs_a_key_optional_may_be_empty() {
    // C#: SettingsPresenterTests.Save_MandatoryNeedsAKey_OptionalMayBeEmpty
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let mut state = presenter.load_fields();
    state.shortcut_texts.insert("Next".to_owned(), "NotAKey".to_owned());
    assert_eq!(
        presenter.try_save(&state, &env.ctx()).message,
        Some(env.get("dialog.settingsInvalid.shortcuts"))
    );

    state.shortcut_texts.insert("Next".to_owned(), "Right".to_owned());
    state.shortcut_texts.insert("LastImage".to_owned(), String::new());
    assert!(ok(&mut presenter, &state, &env));
    assert_eq!(presenter.settings().shortcuts.get("LastImage"), Some(""));
}

#[test]
fn save_optional_shortcut_with_junk_is_refused() {
    // C#: SettingsPresenterTests.Save_OptionalShortcutWithJunk_IsRefused
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let mut state = presenter.load_fields();
    state.shortcut_texts.insert("ClickZoom".to_owned(), "Nope".to_owned());

    let result = presenter.try_save(&state, &env.ctx());

    assert_eq!(result.outcome, SettingsSaveOutcome::Invalid);
    assert_eq!(result.message, Some(env.get("dialog.settingsInvalid.shortcuts")));
}

#[test]
fn save_optional_conflicting_with_mandatory_is_refused_by_validator() {
    // C#: SettingsPresenterTests.Save_OptionalConflictingWithMandatory_IsRefusedByValidator
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let mut state = presenter.load_fields();
    let next = state.shortcut_texts.get("Next").cloned().unwrap_or_default();
    assert!(!next.is_empty());
    state.shortcut_texts.insert("MoveToFolder".to_owned(), next);

    let result = presenter.try_save(&state, &env.ctx());

    assert_eq!(result.outcome, SettingsSaveOutcome::Invalid);
    assert_ne!(result.message, Some(env.get("dialog.settingsInvalid.shortcuts")));
    assert!(!text_of(&result).trim().is_empty());
}

#[test]
fn save_shortcuts_are_canonicalized() {
    // C#: SettingsPresenterTests.Save_ShortcutsAreCanonicalized
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let mut state = presenter.load_fields();
    state.shortcut_texts.insert("Next".to_owned(), " right ".to_owned());

    assert!(ok(&mut presenter, &state, &env));
    assert_eq!(
        presenter.settings().shortcuts.get("Next"),
        Some(canonicalize(Some(" right ")).as_str())
    );
}

#[test]
fn duplicate_warning_follows_typed_text() {
    // C#: SettingsPresenterTests.DuplicateWarning_FollowsTypedText
    let mut presenter = new_presenter(&default_settings());
    let state = presenter.load_fields();
    assert_eq!(
        presenter.duplicate_shortcut_warning(&state.shortcut_texts, &state.actions_json),
        None
    );

    let mut typed: BTreeMap<String, String> = state.shortcut_texts.clone();
    typed.insert("Next".to_owned(), "Right".to_owned());
    typed.insert("Previous".to_owned(), "Right".to_owned());
    let warning = presenter.duplicate_shortcut_warning(&typed, &state.actions_json);
    assert!(warning.is_some_and(|text| !text.trim().is_empty()));
    // Broken actions JSON never throws: it means "no actions".
    let warning = presenter.duplicate_shortcut_warning(&typed, "{ not json");
    assert!(warning.is_some_and(|text| !text.trim().is_empty()));
}

#[test]
fn duplicate_warning_sees_action_shortcuts() {
    // C#: SettingsPresenterTests.DuplicateWarning_SeesActionShortcuts
    let mut action = ReviewAction::new("A");
    action.shortcut = "F8".to_owned();
    action.destination = "Out".to_owned();
    let mut settings = default_settings();
    settings.actions = vec![action];
    let mut presenter = new_presenter(&settings);
    let state = presenter.load_fields();
    let mut typed = state.shortcut_texts.clone();
    typed.insert("Next".to_owned(), "F8".to_owned());

    let first = presenter.duplicate_shortcut_warning(&typed, &state.actions_json);
    assert!(first.as_deref().is_some_and(|text| !text.trim().is_empty()));
    // cached parse of the same text gives the same answer
    assert_eq!(presenter.duplicate_shortcut_warning(&typed, &state.actions_json), first);
    assert_eq!(
        presenter.duplicate_shortcut_warning(&state.shortcut_texts, &state.actions_json),
        None
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Actions JSON box
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn save_actions_json_is_validated() {
    // C#: SettingsPresenterTests.Save_ActionsJson_IsValidated
    // DIVERGENCE: C# writes `"Operation":0`; the merged W8-10b Actions row refuses a numeric Operation (only names),
    // so the rows use `"Operation":"Move"`. Reported to the lead as a W8-10b defect.
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let mut state = presenter.load_fields();
    let refused = Some(env.get("dialog.settingsInvalid.actionsJson"));

    state.actions_json = "{ not json".to_owned();
    assert_eq!(presenter.try_save(&state, &env.ctx()).message, refused);

    state.actions_json = r#"[{"Name":"","Shortcut":"F7","Operation":"Move","Destination":"Out"}]"#.to_owned();
    assert_eq!(presenter.try_save(&state, &env.ctx()).message, refused);

    state.actions_json = r#"[{"Name":"A","Shortcut":"F7","Operation":"Move","Destination":"Out"},{"Name":"B","Shortcut":"f7","Operation":"Move","Destination":"Out2"}]"#
        .to_owned();
    assert_eq!(presenter.try_save(&state, &env.ctx()).message, refused);

    state.actions_json = r#"[{"Name":"A","Shortcut":"F7","Operation":"Move","Destination":"Out"}]"#.to_owned();
    assert!(ok(&mut presenter, &state, &env));
    let names: Vec<&str> = presenter.settings().actions.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["A"]);
}

#[test]
fn save_action_destination_problem_is_refused() {
    // C#: SettingsPresenterTests.Save_ActionDestinationProblem_IsRefused (`"Operation":"Move"`: see the DIVERGENCE note above)
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let mut state = presenter.load_fields();
    state.actions_json =
        r#"[{"Name":"Esc","Shortcut":"F7","Operation":"Move","Destination":"..\\Elsewhere"}]"#.to_owned();

    let result = presenter.try_save(&state, &env.ctx());

    assert_eq!(result.outcome, SettingsSaveOutcome::Invalid);
    let expected = env.texts.format_with(
        "actionProfiles.error.destinationEscapes",
        &[LocArg::new("name", "Esc")],
        &env.numbers,
    );
    assert_eq!(result.message, Some(expected));
}

#[test]
fn actions_text_keeps_non_ascii_readable() {
    // C#: SettingsPresenterTests.ActionsText_KeepsNonAsciiReadable (the `ActionsForEditor` half is W8-10e; here the
    // JSON box text is parsed with `action_list_json`, which is what the editor hand-off does)
    let env = Env::new();
    let mut action = ReviewAction::new("Tuyệt vời");
    action.shortcut = "F7".to_owned();
    action.destination = "Out".to_owned();
    let mut settings = default_settings();
    settings.actions = vec![action];
    let mut presenter = new_presenter(&settings);
    let state = presenter.load_fields();
    assert!(state.actions_json.contains("Tuyệt vời"), "{}", state.actions_json);
    let parsed = action_list_json::deserialize(&state.actions_json, "Action 2");
    assert_eq!(parsed.map(|list| list.len()).ok(), Some(1));
    assert!(ok(&mut presenter, &state, &env));
}

// ---------------------------------------------------------------------------------------------------------------
// Sliders
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn ram_save_rounds_the_slider_value() {
    // C#: SettingsPresenterTests.RamSave_RoundsTheSliderValue
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let mut state = presenter.load_fields();
    state.ram_cache_percent = 41.6;
    assert!(ok(&mut presenter, &state, &env));
    assert_eq!(presenter.settings().image_cache_ram_percent, 42);
}

#[test]
fn opacity_save_is_clamped() {
    // C#: SettingsPresenterTests.OpacitySave_IsClamped
    let env = Env::new();
    let mut presenter = new_presenter(&default_settings());
    let mut state = presenter.load_fields();
    state.toolbar_opacity_value = 5000.0;
    assert!(ok(&mut presenter, &state, &env));
    assert_eq!(
        presenter.settings().toolbar_opacity_percent,
        MAX_TOOLBAR_OPACITY_PERCENT
    );
}

// ---------------------------------------------------------------------------------------------------------------
// New tests: store, language, no writes after a refusal
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn save_with_store_persists_and_replaces_the_working_copy_with_the_canonical_one() {
    // New test
    let env = Env::new();
    let rig = StoreRig::new();
    let mut presenter = rig.presenter(&default_settings());
    let mut state = presenter.load_fields();
    state.shortcut_texts.insert("Next".to_owned(), " right ".to_owned());
    state.click_zoom_percent_text = "250".to_owned();

    assert!(ok(&mut presenter, &state, &env));

    // The working copy is the canonical object the store kept (shortcuts canonical, version stamped).
    let canonical = canonicalize(Some(" right "));
    assert_eq!(canonical, "right"); // trimmed; only alias spellings change case
    assert_eq!(presenter.settings().shortcuts.get("Next"), Some(canonical.as_str()));
    assert_eq!(presenter.settings().click_zoom_percent, 250);
    assert_eq!(json_of(presenter.settings()), json_of(&rig.store.current()));
    let on_disk = rig.fs.file_content(&rig.paths.config_file).unwrap_or_default();
    let text = String::from_utf8_lossy(&on_disk).into_owned();
    assert!(text.contains("\"ConfigVersion\""), "{text}");
    assert!(text.contains("\"Next\": \"right\""), "{text}");
}

#[test]
fn save_store_failure_is_save_failed_and_keeps_the_dialog_state() {
    // New test
    let env = Env::new();
    let rig = StoreRig::new();
    rig.fs
        .fail(FsOp::WriteAtomic, &rig.paths.config_file, ErrorKind::PermissionDenied);
    let mut presenter = rig.presenter(&default_settings());
    let mut state = presenter.load_fields();
    state.click_zoom_percent_text = "250".to_owned();

    let result = presenter.try_save(&state, &env.ctx());

    assert_eq!(result.outcome, SettingsSaveOutcome::SaveFailed);
    assert!(!text_of(&result).trim().is_empty());
    assert_eq!(result.focus, SettingsFocusField::None);
    // The dialog keeps what the user typed so the edit is not lost; the store did not change.
    assert_eq!(presenter.settings().click_zoom_percent, 250);
    assert_eq!(rig.store.current().click_zoom_percent, 100);
    assert_eq!(rig.fs.file_content(&rig.paths.config_file), None);
}

#[test]
fn save_does_not_write_anything_after_a_refusal() {
    // New test
    let env = Env::new();
    let rig = StoreRig::new();
    let mut presenter = rig.presenter(&default_settings());

    let mut state = presenter.load_fields();
    state.shortcut_texts.insert("Next".to_owned(), "NotAKey".to_owned());
    assert_eq!(
        presenter.try_save(&state, &env.ctx()).outcome,
        SettingsSaveOutcome::Invalid
    );

    let mut state = presenter.load_fields();
    state.shortcut_texts.insert("Next".to_owned(), "Right".to_owned());
    state.arrow_pan_step_text = "bad".to_owned();
    assert_eq!(
        presenter.try_save(&state, &env.ctx()).outcome,
        SettingsSaveOutcome::Invalid
    );

    let mut state = presenter.load_fields();
    let next = state.shortcut_texts.get("Next").cloned().unwrap_or_default();
    state.shortcut_texts.insert("MoveToFolder".to_owned(), next);
    assert_eq!(
        presenter.try_save(&state, &env.ctx()).outcome,
        SettingsSaveOutcome::Invalid
    );

    assert_eq!(rig.fs.count(FsOp::WriteAtomic), 0);
    assert_eq!(rig.fs.file_content(&rig.paths.config_file), None);
}

// ---- language switch (a real LocalizationService over a fake disk) ----

const SHIPPED: &str = r"C:\App\Languages";
const USER: &str = r"C:\Users\u\AppData\Local\PhotoReviewRs\Languages";
const VI_CATALOG: &str = include_str!("../../../assets/languages/vi.json");

struct KnownCultures;

impl CultureLookup for KnownCultures {
    fn lookup(&self, name: &str) -> Option<UiCulture> {
        matches!(name, "en" | "vi").then(|| UiCulture::from_name(name))
    }
}

/// Process-wide lock for the test that publishes the ambient localizer / UI culture.
static SERIAL: Mutex<()> = Mutex::new(());

struct Restore {
    _lock: MutexGuard<'static, ()>,
    localizer: Arc<Localizer>,
    culture: UiCulture,
}

impl Restore {
    fn new() -> Self {
        let lock = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        Self {
            _lock: lock,
            localizer: Localizer::current(),
            culture: current_ui_culture(),
        }
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        Localizer::set_current(Arc::clone(&self.localizer));
        set_current_ui_culture(self.culture.clone());
    }
}

#[test]
fn save_language_switch_runs_only_when_the_language_changed() {
    // New test
    let _restore = Restore::new();
    let env = Env::new();
    let disk = Arc::new(FakeFileSystem::new());
    disk.add_dir(SHIPPED);
    disk.add_file(Path::new(SHIPPED).join("vi.json"), VI_CATALOG.as_bytes());
    disk.add_dir(USER);
    let service = LocalizationService::new(
        Arc::clone(&disk) as Arc<dyn FileSystem>,
        LocalizationPaths {
            shipped_dir: PathBuf::from(SHIPPED),
            user_languages_dir: PathBuf::from(USER),
        },
        UiCulture::from_name("en"),
        Arc::new(KnownCultures),
        Arc::new(RecordingLog::new()) as Arc<dyn Log>,
    );
    let shared = Arc::new(Mutex::new(service));
    let mut opts = options();
    opts.localization = Some(Arc::clone(&shared));
    let mut presenter = SettingsPresenter::new(&default_settings(), opts);
    assert!(presenter.has_localization());

    // auto -> vi: the requested language differs, so the service switches.
    let mut state = presenter.load_fields();
    state.selected_language_code = Some("vi".to_owned());
    assert!(ok(&mut presenter, &state, &env));
    assert_eq!(presenter.settings().ui_language, "vi");
    assert_eq!(
        shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .requested_language(),
        "vi"
    );
    let published = Localizer::current();

    // "VI" equals "vi" under OrdinalIgnoreCase: no switch, so nothing new is published.
    state.selected_language_code = Some("VI".to_owned());
    assert!(ok(&mut presenter, &state, &env));
    assert_eq!(presenter.settings().ui_language, "VI");
    assert!(Arc::ptr_eq(&published, &Localizer::current()));

    // No selection keeps the saved language.
    state.selected_language_code = None;
    assert!(ok(&mut presenter, &state, &env));
    assert_eq!(presenter.settings().ui_language, "VI");
    assert!(Arc::ptr_eq(&published, &Localizer::current()));

    // A real change switches again.
    state.selected_language_code = Some("auto".to_owned());
    assert!(ok(&mut presenter, &state, &env));
    assert_eq!(presenter.settings().ui_language, "auto");
    assert_eq!(
        shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .requested_language(),
        "auto"
    );
    assert!(!Arc::ptr_eq(&published, &Localizer::current()));
}
