//! W8-10d tests of `presenters::settings::load` (pages, `load_fields`, RAM slider, hints, defaults) and the three
//! field-map round trips the plan moved here from W8-10b. Kind F: hand-written expectations from the C# tests
//! (baseline d92d6aae). Each test names its C# twin in a `// C#:` comment; tests without a twin say `// New test`.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use pr_app::presenters::settings::SettingsPresenter;
use pr_app::presenters::settings::SettingsPresenterOptions;
use pr_app::presenters::settings::load::PAGES;
use pr_app::presenters::settings_field_map::{
    FieldContext, covered_properties, shortcut_get, shortcut_properties, shortcut_set,
};
use pr_app::presenters::settings_form_state::{SettingsFormState, SettingsSaveOutcome};
use pr_core::abstractions::{ShellIntegration, ShellMenuState};
use pr_core::model::enums::{
    DecoderBackend, FitWidthAnchor, ImageSortMode, ImageTransition, InitialViewMode, InstanceMode, JournalDurability,
    KeyboardZoomAnchor, KineticGlideSmoothing, LoadingMode, MiddleClickAction, MouseWheelAction, RawFullDecode,
    RawPairMode, ScalingQuality,
};
use pr_core::model::fields::{ExifInfoFields, TitleBarFields};
use pr_core::settings::app_settings::{
    AppSettings, DEFAULT_CLICK_ZOOM_PERCENT, EnglishSettingsTexts, MAX_ARROW_PAN_STEP_PERCENT, MAX_CLICK_ZOOM_PERCENT,
    MAX_IMAGE_TRANSITION_MS, MAX_INFO_OVERLAY_AUTO_HIDE_DELAY_MS, MAX_INFO_OVERLAY_FONT_SIZE,
    MAX_KEYBOARD_ZOOM_STEP_PERCENT, MAX_TOOLBAR_AUTO_HIDE_DELAY_MS, MAX_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE,
    MIN_KEYBOARD_ZOOM_STEP_PERCENT, MIN_TOOLBAR_OPACITY_PERCENT,
};
use pr_core::settings::context_menu::{ContextMenuItemId, ITEMS, apply_hidden, effective_hidden, item_info};
use pr_core::settings::perf_options::{
    MAX_IMAGE_CACHE_RAM_PERCENT, MAX_PRELOAD_COUNT, MIN_PRELOAD_BACKWARD_COUNT, PRELOAD_FORWARD_COUNT,
};
use pr_core::settings::shortcuts::{ReviewAction, ShortcutMappings};
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, ImageDecoderFactory};
use pr_imaging::webp_heic::WicCodecSupport;
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_l10n::template::NumberCulture;

const GIB: i64 = 1024 * 1024 * 1024;
const PHYSICAL_MEMORY: i64 = 32 * GIB;

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
}

fn options(physical_memory_bytes: i64) -> SettingsPresenterOptions {
    SettingsPresenterOptions::new(physical_memory_bytes, Arc::new(EnglishSettingsTexts), Arc::new(NoShell))
}

fn presenter_with(settings: &AppSettings, physical_memory_bytes: i64) -> SettingsPresenter {
    SettingsPresenter::new(settings, options(physical_memory_bytes))
}

/// C# `NewPresenter(settings)`: no store, no localization, 32 GiB.
fn new_presenter(settings: &AppSettings) -> SettingsPresenter {
    presenter_with(settings, PHYSICAL_MEMORY)
}

fn default_settings() -> AppSettings {
    AppSettings::new(&EnglishSettingsTexts)
}

fn json_of(settings: &AppSettings) -> String {
    settings
        .to_json()
        .unwrap_or_else(|error| format!("to_json failed: {error:?}"))
}

fn saved(presenter: &mut SettingsPresenter, state: &SettingsFormState, env: &Env) -> bool {
    presenter.try_save(state, &env.ctx()).outcome == SettingsSaveOutcome::Saved
}

// ---------------------------------------------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn next_page_index_wraps() {
    // C#: SettingsPresenterTests.NextPageIndex_Wraps
    assert_eq!(SettingsPresenter::next_page_index(0, 7, true), 1);
    assert_eq!(SettingsPresenter::next_page_index(6, 7, true), 0);
    assert_eq!(SettingsPresenter::next_page_index(0, 7, false), 6);
    assert_eq!(PAGES.len(), 7);
}

#[test]
fn page_titles_are_localized() {
    // C#: SettingsPresenterTests.PageTitles_AreLocalized
    let env = Env::new();
    for page in PAGES {
        let title = SettingsPresenter::page_title(page.key, &env.ctx());
        assert!(!title.trim().is_empty(), "{}", page.key);
        assert_ne!(
            title, page.title_key,
            "{} must be translated, not echo its key",
            page.key
        );
    }
    assert_eq!(SettingsPresenter::page_title("NoSuchPage", &env.ctx()), "");
}

/// The page memory is process-wide; tests that touch it take this lock and restore the original value.
static PAGE_MEMORY: Mutex<()> = Mutex::new(());

struct RestorePage {
    original: String,
}

impl Drop for RestorePage {
    fn drop(&mut self) {
        SettingsPresenter::remember_page(&self.original);
    }
}

#[test]
fn initial_page_uses_remembered_known_key() {
    // C#: SettingsPresenterTests.InitialPage_UsesRememberedKnownKey
    let _lock = PAGE_MEMORY.lock().unwrap_or_else(PoisonError::into_inner);
    let _restore = RestorePage {
        original: SettingsPresenter::remembered_page_key(),
    };
    SettingsPresenter::remember_page("Files");
    assert_eq!(SettingsPresenter::initial_page_key(), "Files");
    SettingsPresenter::remember_page("Gone");
    assert_eq!(SettingsPresenter::initial_page_key(), "General");
    // The remembered text is stored as it is, even when it is not a page.
    assert_eq!(SettingsPresenter::remembered_page_key(), "Gone");
}

// ---------------------------------------------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn defaults_resets_listed_settings_and_keeps_the_rest() {
    // C#: SettingsPresenterTests.Defaults_ResetsListedSettings_AndKeepsTheRest
    let mut custom = default_settings();
    custom.click_zoom_percent = 300;
    custom.logging_enabled = true;
    custom.initial_view_mode = InitialViewMode::Percent100;
    custom.ui_language = "vi".to_owned();
    let mut keep = ReviewAction::new("Keep");
    keep.shortcut = "F7".to_owned();
    keep.destination = "Out".to_owned();
    custom.actions = vec![keep];
    custom.preload_forward_count = MAX_PRELOAD_COUNT;
    custom.show_folder_info = true;
    custom.shortcuts.next = "N".to_owned();
    let mut presenter = new_presenter(&custom);

    let state = presenter.apply_defaults();

    let settings = presenter.settings();
    assert_eq!(settings.click_zoom_percent, DEFAULT_CLICK_ZOOM_PERCENT);
    assert!(!settings.logging_enabled);
    assert_eq!(settings.initial_view_mode, InitialViewMode::Fit);
    assert_eq!(settings.preload_forward_count, PRELOAD_FORWARD_COUNT);
    assert_eq!(settings.shortcuts.next, ShortcutMappings::default().next);
    assert_eq!(settings.ui_language, "vi");
    let names: Vec<&str> = settings.actions.iter().map(|action| action.name.as_str()).collect();
    assert_eq!(names, ["Keep"]);
    assert_eq!(state.click_zoom_percent_text, DEFAULT_CLICK_ZOOM_PERCENT.to_string());
    assert!(!state.show_folder_info);
}

#[test]
fn defaults_clears_indicator_and_editor_path() {
    // C#: SettingsPresenterTests.Defaults_ClearsIndicatorAndEditorPath
    let mut custom = default_settings();
    custom.show_zoom_indicator = true;
    custom.external_editor_path = r"C:\Tools\editor.exe".to_owned();
    let mut presenter = new_presenter(&custom);

    let state = presenter.apply_defaults();

    assert!(!presenter.settings().show_zoom_indicator);
    assert_eq!(presenter.settings().external_editor_path, "");
    assert!(!state.show_zoom_indicator);
    assert_eq!(state.external_editor_path, "");
}

#[test]
fn defaults_touch_only_the_listed_properties() {
    // New test: the 65 assignments of `ApplyDefaults` bring every property except the ones C# leaves alone back to
    // `AppSettings::new`. A settings object with every property moved ends equal to the defaults on the reset ones.
    let baseline = default_settings();
    let mut custom = default_settings();
    for row in property_table() {
        (row.mutate)(&mut custom);
    }
    custom.ui_language = "vi".to_owned();
    custom.last_move_to_folder = Some("move".to_owned());
    custom.last_copy_to_folder = Some("copy".to_owned());
    let mut presenter = new_presenter(&custom);

    presenter.apply_defaults();
    let after = presenter.settings();

    // Not touched by Defaults: language, actions, last folders, config version.
    assert_eq!(after.ui_language, "vi");
    assert_eq!(after.last_move_to_folder.as_deref(), Some("move"));
    assert_eq!(after.last_copy_to_folder.as_deref(), Some("copy"));
    assert_eq!(after.config_version, baseline.config_version);
    // Reset: every mapped scalar property listed in the table is back to its default (the table has them all).
    for row in property_table() {
        assert!((row.same)(after, &baseline), "{} was not reset by Defaults", row.name);
    }
    // Performance internals are reset too.
    assert_eq!(after.image_cache_capacity_bytes, baseline.image_cache_capacity_bytes);
    assert_eq!(after.memory_reserve_bytes, baseline.memory_reserve_bytes);
    assert_eq!(after.preload_worker_count, baseline.preload_worker_count);
    assert_eq!(after.preload_memory_load_limit, baseline.preload_memory_load_limit);
    assert_eq!(
        after.set_zoom_also_sets_click_level,
        baseline.set_zoom_also_sets_click_level
    );
    assert_eq!(after.hidden_context_menu_items, None);
}

// ---------------------------------------------------------------------------------------------------------------
// RAM slider
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn ram_range_follows_typed_preload_window() {
    // C#: SettingsPresenterTests.RamRange_FollowsTypedPreloadWindow (8 GiB: a bigger window visibly raises the floor)
    let presenter = presenter_with(&default_settings(), 8 * GIB);
    let max = MAX_PRELOAD_COUNT.to_string();
    let small = presenter.ram_cache_range(Some("1"), Some("1"));
    let large = presenter.ram_cache_range(Some(&max), Some(&max));
    let saved_forward = presenter.settings().preload_forward_count.to_string();
    let saved_backward = presenter.settings().preload_backward_count.to_string();
    let saved = presenter.ram_cache_range(Some(&saved_forward), Some(&saved_backward));

    assert_eq!(small.1, MAX_IMAGE_CACHE_RAM_PERCENT);
    assert!(
        large.0 > small.0,
        "large window floor {} should exceed small {}",
        large.0,
        small.0
    );
    assert_eq!(presenter.ram_cache_range(Some("garbage"), None), saved);
    assert_eq!(presenter.ram_cache_range(Some("-5"), Some("99999")), saved);
    assert_ne!(saved, large);

    // Each box counts on its own: changing only the forward box (backward = saved) moves the floor too.
    let forward_only = presenter.ram_cache_range(Some(&max), Some(&saved_backward));
    let forward_small = presenter.ram_cache_range(Some("1"), Some(&saved_backward));
    assert!(
        forward_only.0 > forward_small.0,
        "{} vs {}",
        forward_only.0,
        forward_small.0
    );
    let backward_only = presenter.ram_cache_range(Some(&saved_forward), Some(&max));
    let backward_small = presenter.ram_cache_range(Some(&saved_forward), Some("0"));
    assert!(
        backward_only.0 > backward_small.0,
        "{} vs {}",
        backward_only.0,
        backward_small.0
    );
}

#[test]
fn ram_range_falls_back_per_box_to_the_saved_value_clamped_into_that_boxs_range() {
    // New test (trap T3): a bad box does not change the other box, and a saved value outside the range is clamped.
    let mut settings = default_settings();
    settings.preload_forward_count = 9_999; // outside [1, 500]
    settings.preload_backward_count = -3; // outside [0, 500]
    // 128 GiB: the floors of a 500-image and of a 9999-image window differ (on 8 GiB both hit the 90 % cap).
    let presenter = presenter_with(&settings, 128 * GIB);
    let clamped = presenter.ram_cache_range(
        Some(&MAX_PRELOAD_COUNT.to_string()),
        Some(&MIN_PRELOAD_BACKWARD_COUNT.to_string()),
    );
    // Bad forward box: saved 9999 -> 500; bad backward box: saved -3 -> 0.
    assert_eq!(presenter.ram_cache_range(Some("x"), Some("y")), clamped);
    assert_eq!(presenter.ram_cache_range(None, None), clamped);
    // A non-ASCII (NBSP, full-width digit) or '+'-only text is refused like any other junk.
    assert_eq!(presenter.ram_cache_range(Some("\u{a0}5"), Some("+")), clamped);
    assert_eq!(presenter.ram_cache_range(Some("\u{ff15}"), Some("")), clamped);
    // Surrounding ASCII whitespace and a plus sign are accepted.
    assert_eq!(
        presenter.ram_cache_range(Some(" +500\t"), Some("0 ")),
        clamped,
        "' +500\\t' and '0 ' parse to the same window"
    );
    // A bad box does not leak into the other one: forward typed 1, backward junk behaves like backward 0 (saved -3 clamped).
    let forward_one = presenter.ram_cache_range(Some("1"), Some("junk"));
    let forward_one_backward_zero = presenter.ram_cache_range(Some("1"), Some("0"));
    assert_eq!(forward_one, forward_one_backward_zero);
}

#[test]
fn ram_load_clamps_into_range() {
    // C#: SettingsPresenterTests.RamLoad_ClampsIntoRange
    let env = Env::new();
    let mut settings = default_settings();
    settings.image_cache_ram_percent = 1;
    let presenter = new_presenter(&settings);
    let state = presenter.load_fields();
    let (min, max) = presenter.ram_cache_range(
        Some(state.preload_forward_text.as_str()),
        Some(state.preload_backward_text.as_str()),
    );

    assert_eq!(state.ram_cache_percent, f64::from(min));
    assert!(state.ram_cache_percent >= f64::from(min) && state.ram_cache_percent <= f64::from(max));
    assert!(presenter.ram_cache_value_text(100, &env.ctx()).contains("32.0"));
}

#[test]
fn load_fields_does_not_modify_the_working_copy() {
    // New test: the RAM clamp is display only.
    let mut settings = default_settings();
    settings.image_cache_ram_percent = 1;
    let presenter = new_presenter(&settings);
    let before = json_of(presenter.settings());
    let state = presenter.load_fields();
    assert!(state.ram_cache_percent > 1.0);
    assert_eq!(json_of(presenter.settings()), before);
    assert_eq!(presenter.settings().image_cache_ram_percent, 1);
    // A saved value above the maximum is clamped down on display as well.
    let mut high = default_settings();
    high.image_cache_ram_percent = 100;
    assert_eq!(
        new_presenter(&high).load_fields().ram_cache_percent,
        f64::from(MAX_IMAGE_CACHE_RAM_PERCENT)
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Hints and texts
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn hints_use_the_localizer() {
    // New test: the five hint functions against the en.json placeholders.
    let env = Env::new();
    let ctx = env.ctx();
    let presenter = presenter_with(&default_settings(), 8 * GIB);

    let ram = presenter.ram_cache_hint_text(7, 90, &ctx);
    assert!(ram.contains("this device's 8.0 GB of RAM"), "{ram}");
    assert!(ram.contains("Allowed: 7% to 90%."), "{ram}");

    let value = presenter.ram_cache_value_text(50, &ctx);
    assert_eq!(value, "50% of RAM \u{2248} 4.0 GB");

    let window = SettingsPresenter::preload_window_hint(&ctx);
    assert!(
        window.contains(&format!(
            "Allowed {MIN_PRELOAD_BACKWARD_COUNT}\u{2013}{MAX_PRELOAD_COUNT}."
        )),
        "{window}"
    );

    let opacity = SettingsPresenter::toolbar_opacity_hint(&ctx);
    assert!(
        opacity.contains(&format!("below {MIN_TOOLBAR_OPACITY_PERCENT}%")),
        "{opacity}"
    );

    let zoom = SettingsPresenter::keyboard_zoom_step_percent_hint(&ctx);
    assert!(
        zoom.contains(&format!(
            "({MIN_KEYBOARD_ZOOM_STEP_PERCENT}-{MAX_KEYBOARD_ZOOM_STEP_PERCENT}, default 10)"
        )),
        "{zoom}"
    );
}

#[test]
fn toolbar_opacity_value_text_rounds_ties_to_even() {
    // New test (trap T5): C# `(int)Math.Round(v)` is ties-to-even.
    let env = Env::new();
    let rows: [(f64, &str); 8] = [
        (0.5, "0 %"),
        (1.5, "2 %"),
        (2.5, "2 %"),
        (99.5, "100 %"),
        (100.0, "100 %"),
        (41.6, "42 %"),
        (-0.5, "0 %"),
        (-1.5, "-2 %"),
    ];
    for (value, expected) in rows {
        assert_eq!(
            SettingsPresenter::toolbar_opacity_value_text(value, &env.ctx()),
            expected,
            "{value}"
        );
    }
    // The negative sign comes from the culture.
    let mut numbers = NumberCulture::invariant();
    numbers.negative_sign = "\u{2212}".to_owned();
    let ctx = FieldContext {
        texts: &env.texts,
        numbers: &numbers,
    };
    assert_eq!(SettingsPresenter::toolbar_opacity_value_text(-1.5, &ctx), "\u{2212}2 %");
}

#[test]
fn format_gb_uses_one_decimal_and_question_mark_when_unknown() {
    // New test (trap T6): GiB with ONE decimal, half away from zero, the culture's decimal separator, "?" when unknown.
    let env = Env::new();
    let hint_total = |bytes: i64, ctx: &FieldContext<'_>| {
        let text = presenter_with(&default_settings(), bytes).ram_cache_hint_text(1, 90, ctx);
        let start = text.find("device's ").map(|at| at + "device's ".len()).unwrap_or(0);
        let end = text.find(" GB of RAM").unwrap_or(text.len());
        text[start..end].to_owned()
    };
    assert_eq!(hint_total(32 * GIB, &env.ctx()), "32.0");
    assert_eq!(hint_total(GIB + GIB / 2, &env.ctx()), "1.5");
    assert_eq!(hint_total(GIB + GIB / 4, &env.ctx()), "1.3"); // 1.25 rounds away from zero
    assert_eq!(hint_total(0, &env.ctx()), "?");
    assert_eq!(hint_total(-1, &env.ctx()), "?");

    let mut numbers = NumberCulture::invariant();
    numbers.decimal_separator = ",".to_owned();
    let comma = FieldContext {
        texts: &env.texts,
        numbers: &numbers,
    };
    assert_eq!(hint_total(GIB + GIB / 2, &comma), "1,5");
}

#[test]
fn webp_heic_status_covers_all_codec_combinations() {
    // New test: webp x {available, missing}, heif x {ok, container only, none}.
    let env = Env::new();
    let ctx = env.ctx();
    let available = env.texts.get("settings.webpHeicSupportEnabled.codecAvailable");
    let webp_missing = env.texts.get("settings.webpHeicSupportEnabled.webpMissing");
    let heif_missing = env.texts.get("settings.webpHeicSupportEnabled.heifMissing");
    let hevc_missing = env.texts.get("settings.webpHeicSupportEnabled.hevcMissing");
    let codecs = |webp: bool, heif_container: bool, hevc_decoder: bool| WicCodecSupport {
        webp,
        heif_container,
        hevc_decoder,
        detail: String::new(),
    };
    let status = |webp: &str, heif: &str| format!("On this PC: WebP {webp}; HEIC/HEIF {heif}.");

    let rows = [
        (codecs(true, true, true), status(&available, &available)),
        (codecs(true, true, false), status(&available, &hevc_missing)),
        (codecs(true, false, true), status(&available, &heif_missing)),
        (codecs(true, false, false), status(&available, &heif_missing)),
        (codecs(false, true, true), status(&webp_missing, &available)),
        (codecs(false, true, false), status(&webp_missing, &hevc_missing)),
        (codecs(false, false, false), status(&webp_missing, &heif_missing)),
        (
            WicCodecSupport::none("probe failed"),
            status(&webp_missing, &heif_missing),
        ),
    ];
    for (support, expected) in rows {
        assert_eq!(
            SettingsPresenter::format_webp_heic_codec_status(&support, &ctx),
            expected,
            "{support:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------
// TurboJPEG
// ---------------------------------------------------------------------------------------------------------------

struct NoDecoder;

impl ImageDecoder for NoDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Err(DecodeError::Canceled)
    }
    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Err(DecodeError::Canceled)
    }
}

struct FakeFactory {
    turbo_registered: bool,
}

impl ImageDecoderFactory for FakeFactory {
    fn create(&self, _backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        Arc::new(NoDecoder)
    }
    fn is_registered(&self, backend: DecoderBackend) -> bool {
        backend != DecoderBackend::TurboJpeg || self.turbo_registered
    }
}

#[test]
fn turbo_jpeg_availability() {
    // C#: SettingsPresenterTests.TurboJpegAvailability (the C# test only checks `null`; the factory rows are new)
    assert!(SettingsPresenter::is_turbo_jpeg_available(None));
    assert!(SettingsPresenter::is_turbo_jpeg_available(Some(&FakeFactory {
        turbo_registered: true
    })));
    assert!(!SettingsPresenter::is_turbo_jpeg_available(Some(&FakeFactory {
        turbo_registered: false
    })));
}

// ---------------------------------------------------------------------------------------------------------------
// Round trips (moved here from W8-10b)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn shortcuts_round_trip_each_property() {
    // C#: SettingsFieldMapTests.Shortcuts_RoundTripEachProperty
    let env = Env::new();
    // Distinct valid key names for the 29 boxes (a duplicate would be refused by the validator).
    let keys = [
        "A", "B", "C", "D", "E", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W",
        "X", "Y", "Z", "F1", "F2", "F3", "F4",
    ];
    let properties = shortcut_properties();
    assert_eq!(properties.len(), keys.len());
    let mut source = default_settings();
    source.actions = Vec::new();
    for (property, key) in properties.iter().zip(keys) {
        shortcut_set(&mut source.shortcuts, property, key.to_owned());
    }
    source.shortcuts.move_to_folder2 = "F9".to_owned();

    // The working copy of a presenter built from the keyed settings; Save must write every box back, so the copy the
    // presenter saves into starts as one where all 27 are blank.
    let loader = new_presenter(&source);
    let state = loader.load_fields();
    for (property, key) in properties.iter().zip(keys) {
        assert_eq!(
            state.shortcut_texts.get(*property).map(String::as_str),
            Some(key),
            "{property}"
        );
    }

    let mut blank = source.clone();
    for property in properties {
        shortcut_set(&mut blank.shortcuts, property, String::new());
    }
    let mut presenter = new_presenter(&blank);
    let result = presenter.try_save(&state, &env.ctx());
    assert_eq!(result.outcome, SettingsSaveOutcome::Saved, "{:?}", result.message);
    for (property, key) in properties.iter().zip(keys) {
        assert_eq!(
            shortcut_get(&presenter.settings().shortcuts, property),
            key,
            "{property}"
        );
    }
    assert_eq!(presenter.settings().shortcuts.move_to_folder2, "F9");
}

/// One row of the property table: a valid, non-default value and the comparison for it.
struct PropertyRow {
    name: &'static str,
    mutate: fn(&mut AppSettings),
    same: fn(&AppSettings, &AppSettings) -> bool,
}

macro_rules! row {
    ($name:literal, $field:ident, $value:expr) => {
        PropertyRow {
            name: $name,
            mutate: |s| s.$field = $value,
            same: |a, b| a.$field == b.$field,
        }
    };
}

/// Every mapped property except `Shortcuts`, `Actions`, `UiLanguage` and `HiddenContextMenuItems` (dedicated tests), each
/// with the same "valid, not default" value as the C# `MutateValid`. Rust has no reflection, so the table is explicit and
/// a first assertion in the round trip compares its names with `covered_properties()`.
fn property_table() -> Vec<PropertyRow> {
    vec![
        row!("InitialViewMode", initial_view_mode, InitialViewMode::Percent100),
        row!("FitWidthAnchor", fit_width_anchor, FitWidthAnchor::TopThird),
        row!("FitWidthAnchor2", fit_width_anchor2, Some(FitWidthAnchor::TopThird)),
        row!("MiddleClickAction", middle_click_action, Some(MiddleClickAction::Fit)),
        row!("KeepZoomAcrossImages", keep_zoom_across_images, true),
        row!("ImageSortMode", image_sort_mode, Some(ImageSortMode::SizeAscending)),
        row!("ScalingQuality", scaling_quality, Some(ScalingQuality::Linear)),
        row!("DecoderBackend", decoder_backend, Some(DecoderBackend::Wpf)),
        row!("LoadingMode", loading_mode, Some(LoadingMode::Fast)),
        row!("InstanceMode", instance_mode, InstanceMode::PerFolder),
        row!("CompareHashEnabled", compare_hash_enabled, false),
        row!("CompareSizeEnabled", compare_size_enabled, false),
        row!("RawSupportEnabled", raw_support_enabled, false),
        row!("WebpHeicSupportEnabled", webp_heic_support_enabled, false),
        row!("RawFullDecode", raw_full_decode, RawFullDecode::OnZoom),
        row!("RawPairMode", raw_pair_mode, RawPairMode::PreferJpeg),
        row!("LoggingEnabled", logging_enabled, true),
        row!(
            "AllowPermanentDeleteWithoutRecycleBin",
            allow_permanent_delete_without_recycle_bin,
            true
        ),
        row!("ConfirmBeforeDelete", confirm_before_delete, true),
        row!(
            "ExternalEditorPath",
            external_editor_path,
            "probe-ExternalEditorPath".to_owned()
        ),
        row!(
            "ExternalEditors",
            external_editors,
            vec![pr_core::settings::external_editors::ExternalEditor {
                id: "probe-editor".to_owned(),
                name: "Probe".to_owned(),
                path: "C:\\probe.exe".to_owned(),
                arguments: "\"{file}\"".to_owned(),
            }]
        ),
        row!(
            "DefaultExternalEditorId",
            default_external_editor_id,
            "probe-editor".to_owned()
        ),
        row!(
            "ExternalEditorSendsRawOriginal",
            external_editor_sends_raw_original,
            false
        ),
        row!(
            "JournalDurability",
            journal_durability,
            JournalDurability::PowerLossSafe
        ),
        row!(
            "ImageCacheRamPercent",
            image_cache_ram_percent,
            MAX_IMAGE_CACHE_RAM_PERCENT
        ),
        row!("ShowInfoOverlay", show_info_overlay, false),
        row!("ShowFileInfo", show_file_info, false),
        row!("ShowFolderInfo", show_folder_info, true),
        row!("MouseWheelAction", mouse_wheel_action, MouseWheelAction::Navigate),
        row!("ClickToZoomEnabled", click_to_zoom_enabled, true),
        row!("KineticPanEnabled", kinetic_pan_enabled, true),
        row!(
            "KineticGlideSmoothing",
            kinetic_glide_smoothing,
            Some(KineticGlideSmoothing::Off)
        ),
        row!("ArrowKeyNavigatesAtZoomEdge", arrow_key_navigates_at_zoom_edge, true),
        row!("ShowFolderMenuItems", show_folder_menu_items, false),
        row!("ShowZoomMenuItems", show_zoom_menu_items, true),
        row!("ShowRecycleMenuItem", show_recycle_menu_item, true),
        row!("ClickZoomKeyTogglesFit", click_zoom_key_toggles_fit, true),
        row!(
            "KeyboardZoomAnchor",
            keyboard_zoom_anchor,
            Some(KeyboardZoomAnchor::Pointer)
        ),
        row!("MoveCopyReuseLastFolder", move_copy_reuse_last_folder, true),
        row!("ShowExifInfo", show_exif_info, true),
        row!("ExifInfoFields", exif_info_fields, ExifInfoFields::LENS),
        row!("TitleBarFields", title_bar_fields, TitleBarFields::FILE_NAME),
        row!("ClickZoomPercent", click_zoom_percent, MAX_CLICK_ZOOM_PERCENT),
        row!(
            "ArrowPanStepPercent",
            arrow_pan_step_percent,
            MAX_ARROW_PAN_STEP_PERCENT
        ),
        row!(
            "KeyboardZoomStepPercent",
            keyboard_zoom_step_percent,
            MAX_KEYBOARD_ZOOM_STEP_PERCENT
        ),
        row!("ShowZoomIndicator", show_zoom_indicator, true),
        row!("ImageTransition", image_transition, ImageTransition::Fade),
        row!("ImageTransitionMs", image_transition_ms, MAX_IMAGE_TRANSITION_MS),
        row!("TouchpadSwipeEnabled", touchpad_swipe_enabled, false),
        row!(
            "TouchpadSwipeDistancePerImage",
            touchpad_swipe_distance_per_image,
            MAX_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE
        ),
        row!("PreloadForwardCount", preload_forward_count, MAX_PRELOAD_COUNT),
        row!("PreloadBackwardCount", preload_backward_count, MAX_PRELOAD_COUNT),
        row!("ToolbarAutoHide", toolbar_auto_hide, true),
        row!(
            "ToolbarAutoHideDelayMs",
            toolbar_auto_hide_delay_ms,
            MAX_TOOLBAR_AUTO_HIDE_DELAY_MS
        ),
        row!("InfoOverlayAutoHide", info_overlay_auto_hide, true),
        row!(
            "InfoOverlayAutoHideDelayMs",
            info_overlay_auto_hide_delay_ms,
            MAX_INFO_OVERLAY_AUTO_HIDE_DELAY_MS
        ),
        row!(
            "ToolbarOpacityPercent",
            toolbar_opacity_percent,
            MIN_TOOLBAR_OPACITY_PERCENT
        ),
        row!(
            "InfoOverlayFontSize",
            info_overlay_font_size,
            MAX_INFO_OVERLAY_FONT_SIZE
        ),
    ]
}

#[test]
fn mapped_properties_round_trip_through_load_and_save() {
    // C#: SettingsFieldMapTests.MappedProperties_RoundTripThroughLoadAndSave
    let env = Env::new();
    let table = property_table();

    // A new property in the field map fails here until it has a mutator.
    let special: BTreeSet<&str> = ["Shortcuts", "Actions", "UiLanguage", "HiddenContextMenuItems"].into();
    let expected: BTreeSet<&str> = covered_properties().difference(&special).copied().collect();
    let names: BTreeSet<&str> = table.iter().map(|row| row.name).collect();
    assert_eq!(table.len(), names.len(), "duplicate row names");
    assert_eq!(names, expected);

    let defaults = default_settings();
    let mut failures = Vec::new();
    for row in &table {
        let mut baseline = default_settings();
        (row.mutate)(&mut baseline);
        assert!(
            !(row.same)(&baseline, &defaults),
            "{}: the mutation must differ from the default",
            row.name
        );

        let state = new_presenter(&baseline).load_fields();
        // Save the loaded form into a working copy that has only this property back at its default: only the field row
        // writing the loaded control back can restore it.
        let mut presenter = new_presenter(&defaults);
        let result = presenter.try_save(&state, &env.ctx());
        if result.outcome != SettingsSaveOutcome::Saved {
            failures.push(format!("{}: Save refused: {:?}", row.name, result.message));
        } else if !(row.same)(presenter.settings(), &baseline) {
            failures.push(format!("{}: Save did not restore the loaded value", row.name));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn context_menu_round_trips_hidden_set_and_keeps_locked_items_shown() {
    // C#: SettingsFieldMapTests.ContextMenu_RoundTripsHiddenSet_AndKeepsLockedItemsShown
    let env = Env::new();
    let all: Vec<ContextMenuItemId> = ITEMS.iter().map(|info| info.id).collect();
    let hidden: Vec<ContextMenuItemId> = all
        .iter()
        .copied()
        .filter(|id| !item_info(*id).locked)
        .take(3)
        .collect();
    assert_eq!(hidden.len(), 3);
    let mut source = default_settings();
    apply_hidden(&mut source, hidden.iter().copied());

    let loader = new_presenter(&source);
    let state = loader.load_fields();
    assert_eq!(state.context_menu_shown.len(), all.len() - hidden.len());
    for id in &hidden {
        assert!(!state.context_menu_shown.contains(id), "{id:?}");
    }
    for id in all.iter().filter(|id| item_info(**id).locked) {
        assert!(state.context_menu_shown.contains(id), "{id:?}");
    }

    // Save into a working copy whose list is cleared and whose legacy flag differs: only the row can restore the set.
    let mut start = source.clone();
    start.hidden_context_menu_items = None;
    start.show_folder_menu_items = !source.show_folder_menu_items;
    let mut presenter = new_presenter(&start);
    assert!(saved(&mut presenter, &state, &env));
    let sorted = |set: std::collections::HashSet<ContextMenuItemId>| {
        let mut list: Vec<ContextMenuItemId> = set.into_iter().collect();
        list.sort();
        list
    };
    let mut want = hidden.clone();
    want.sort();
    assert_eq!(sorted(effective_hidden(presenter.settings())), want);

    // Un-ticking one more item in the form saves it as hidden too.
    let extra = all
        .iter()
        .copied()
        .find(|id| !item_info(*id).locked && !hidden.contains(id))
        .unwrap_or(ContextMenuItemId::Undo);
    let mut state = state;
    state.context_menu_shown.remove(&extra);
    assert!(saved(&mut presenter, &state, &env));
    assert!(effective_hidden(presenter.settings()).contains(&extra));
}
