//! Port of `PhotoReview.Core/Settings/AppSettings.cs` and `AppSettingsJsonContext.cs` tests (baseline d92d6aae).
//! Golden: `tests/golden/settings-json.json` (785 cases).

use std::collections::BTreeMap;

use pr_core::model::enums::{FitWidthAnchor, ImageSortMode, InitialViewMode, LoadingMode, RawFullDecode, RawPairMode};
use pr_core::settings::app_settings::{
    self as app, AppSettings, EnglishSettingsTexts, JSON_KEYS, SettingsJsonError, json,
};
use pr_test_support::golden::{self, str_of};
use serde_json::Value;

const GIB: i64 = 1024 * 1024 * 1024;

fn read(text: &str) -> Result<Option<AppSettings>, SettingsJsonError> {
    AppSettings::from_json(text, &EnglishSettingsTexts)
}

/// Splits a written document into `(property name, segment)` pairs the way the oracle's `W201Segments` does.
fn segments(text: &str) -> BTreeMap<String, String> {
    let lines: Vec<&str> = text.split("\r\n").collect();
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in &lines[1..lines.len() - 1] {
        if let Some(rest) = line.strip_prefix("  \"") {
            if !rest.starts_with(' ') {
                let (name, value) = rest.split_once("\": ").expect("property line");
                out.push((name.to_owned(), vec![value.to_owned()]));
                continue;
            }
        }
        out.last_mut().expect("continuation").1.push((*line).to_owned());
    }
    out.into_iter()
        .map(|(name, parts)| {
            let joined = parts.join("\r\n");
            let trimmed = joined.strip_suffix(',').unwrap_or(&joined).to_owned();
            (name, trimmed)
        })
        .collect()
}

fn changed_from_default(settings: &AppSettings) -> Result<Value, SettingsJsonError> {
    let default = segments(&AppSettings::default().to_json().expect("default writes"));
    let now = segments(&settings.to_json()?);
    let mut map = serde_json::Map::new();
    for (name, seg) in now {
        if default.get(&name) != Some(&seg) {
            map.insert(name, Value::String(seg));
        }
    }
    Ok(Value::Object(map))
}

fn check_read_case(case: &Value) {
    let name = str_of(case, "name");
    let text = case
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let property = str_of(case, "property");
            format!("{{\"{property}\":{}}}", str_of(case, "token"))
        });
    let outcome = str_of(case, "outcome");
    let parsed = read(&text);
    match outcome {
        "error" => {
            let write_failed = case.get("error_type").and_then(Value::as_str) == Some("ArgumentException");
            match parsed {
                Err(e) => assert!(!write_failed, "{name}: read failed but C# only failed the write: {e}"),
                Ok(Some(s)) => {
                    assert!(write_failed, "{name}: expected a read error");
                    assert!(s.to_json().is_err(), "{name}: write must fail");
                    assert!(case.get("null_actions").is_some(), "{name}");
                }
                Ok(None) => panic!("{name}: expected error"),
            }
        }
        "null" => assert!(matches!(parsed, Ok(None)), "{name}: expected null"),
        "ok" => {
            let s = parsed
                .unwrap_or_else(|e| panic!("{name}: unexpected error {e}"))
                .unwrap_or_else(|| panic!("{name}: unexpected null"));
            assert_eq!(
                i64::from(s.read_notes.null_actions),
                case["null_actions"].as_i64().expect("null_actions"),
                "{name}: null_actions"
            );
            let changed = changed_from_default(&s).unwrap_or_else(|e| panic!("{name}: write failed {e}"));
            assert_eq!(changed, case["changed"], "{name}: changed segments");
        }
        other => panic!("unknown outcome {other}"),
    }
}

fn golden_cases(kind: &str) -> Vec<Value> {
    let g = golden::load("settings-json");
    assert!(g.csharp_commit.starts_with("d92d6aae"), "baseline {}", g.csharp_commit);
    g.cases.into_iter().filter(|c| c["kind"] == kind).collect()
}

#[test]
fn golden_settings_json() {
    // defaults
    let defaults = &golden_cases("defaults")[0];
    assert_eq!(
        AppSettings::default().to_json().expect("writes"),
        str_of(defaults, "text")
    );
    let keys: Vec<&str> = defaults["keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap())
        .collect();
    assert_eq!(JSON_KEYS.to_vec(), keys);
    let consts = defaults["consts"].as_object().unwrap();
    let expected: [(&str, f64); 28] = [
        ("CurrentConfigVersion", f64::from(app::CURRENT_CONFIG_VERSION)),
        ("MinClickZoomPercent", f64::from(app::MIN_CLICK_ZOOM_PERCENT)),
        ("MaxClickZoomPercent", f64::from(app::MAX_CLICK_ZOOM_PERCENT)),
        ("MinArrowPanStepPercent", f64::from(app::MIN_ARROW_PAN_STEP_PERCENT)),
        ("MaxArrowPanStepPercent", f64::from(app::MAX_ARROW_PAN_STEP_PERCENT)),
        (
            "DefaultArrowPanStepPercent",
            f64::from(app::DEFAULT_ARROW_PAN_STEP_PERCENT),
        ),
        ("DefaultClickZoomPercent", f64::from(app::DEFAULT_CLICK_ZOOM_PERCENT)),
        (
            "MinToolbarAutoHideDelayMs",
            f64::from(app::MIN_TOOLBAR_AUTO_HIDE_DELAY_MS),
        ),
        (
            "MaxToolbarAutoHideDelayMs",
            f64::from(app::MAX_TOOLBAR_AUTO_HIDE_DELAY_MS),
        ),
        (
            "DefaultToolbarAutoHideDelayMs",
            f64::from(app::DEFAULT_TOOLBAR_AUTO_HIDE_DELAY_MS),
        ),
        (
            "MinInfoOverlayAutoHideDelayMs",
            f64::from(app::MIN_INFO_OVERLAY_AUTO_HIDE_DELAY_MS),
        ),
        (
            "MaxInfoOverlayAutoHideDelayMs",
            f64::from(app::MAX_INFO_OVERLAY_AUTO_HIDE_DELAY_MS),
        ),
        (
            "DefaultInfoOverlayAutoHideDelayMs",
            f64::from(app::DEFAULT_INFO_OVERLAY_AUTO_HIDE_DELAY_MS),
        ),
        ("MinToolbarOpacityPercent", f64::from(app::MIN_TOOLBAR_OPACITY_PERCENT)),
        ("MaxToolbarOpacityPercent", f64::from(app::MAX_TOOLBAR_OPACITY_PERCENT)),
        (
            "DefaultToolbarOpacityPercent",
            f64::from(app::DEFAULT_TOOLBAR_OPACITY_PERCENT),
        ),
        ("MinInfoOverlayFontSize", app::MIN_INFO_OVERLAY_FONT_SIZE),
        ("MaxInfoOverlayFontSize", app::MAX_INFO_OVERLAY_FONT_SIZE),
        ("DefaultInfoOverlayFontSize", app::DEFAULT_INFO_OVERLAY_FONT_SIZE),
        (
            "MinKeyboardZoomStepPercent",
            f64::from(app::MIN_KEYBOARD_ZOOM_STEP_PERCENT),
        ),
        (
            "MaxKeyboardZoomStepPercent",
            f64::from(app::MAX_KEYBOARD_ZOOM_STEP_PERCENT),
        ),
        (
            "DefaultKeyboardZoomStepPercent",
            f64::from(app::DEFAULT_KEYBOARD_ZOOM_STEP_PERCENT),
        ),
        ("MinImageTransitionMs", f64::from(app::MIN_IMAGE_TRANSITION_MS)),
        ("MaxImageTransitionMs", f64::from(app::MAX_IMAGE_TRANSITION_MS)),
        ("DefaultImageTransitionMs", f64::from(app::DEFAULT_IMAGE_TRANSITION_MS)),
        (
            "MinTouchpadSwipeDistancePerImage",
            f64::from(app::MIN_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE),
        ),
        (
            "MaxTouchpadSwipeDistancePerImage",
            f64::from(app::MAX_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE),
        ),
        (
            "DefaultTouchpadSwipeDistancePerImage",
            f64::from(app::DEFAULT_TOUCHPAD_SWIPE_DISTANCE_PER_IMAGE),
        ),
    ];
    assert_eq!(consts.len(), expected.len());
    for (name, value) in expected {
        assert_eq!(consts[name].as_f64(), Some(value), "const {name}");
    }

    // read + scalar
    let reads = golden_cases("read");
    assert_eq!(reads.len(), 104);
    for case in &reads {
        check_read_case(case);
    }
    let scalars = golden_cases("scalar");
    assert_eq!(scalars.len(), 475);
    for case in &scalars {
        check_read_case(case);
    }

    // roundtrip
    let rts = golden_cases("roundtrip");
    assert_eq!(rts.len(), 13);
    for case in &rts {
        let name = str_of(case, "name");
        let s = read(str_of(case, "text"))
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .unwrap_or_else(|| panic!("{name}: null"));
        assert_eq!(s.to_json().expect("writes"), str_of(case, "text_out"), "{name}");
    }

    // doubles
    let doubles = golden_cases("double");
    assert_eq!(doubles.len(), 42);
    for case in &doubles {
        let bits = u64::from_str_radix(str_of(case, "bits").trim_start_matches("0x"), 16).unwrap();
        let v = f64::from_bits(bits);
        let mut s = AppSettings::new(&EnglishSettingsTexts);
        s.info_overlay_font_size = v;
        s.preload_memory_load_limit = v;
        if case.get("error_type").is_some() {
            assert!(s.to_json().is_err(), "bits {bits:#x}");
            assert!(json::format_f64(v).is_none());
            continue;
        }
        let segs = segments(&s.to_json().expect("writes"));
        assert_eq!(segs["InfoOverlayFontSize"], str_of(case, "text"), "bits {bits:#x}");
        assert_eq!(
            segs["PreloadMemoryLoadLimit"],
            str_of(case, "text_limit"),
            "bits {bits:#x}"
        );
    }

    // escape
    let escapes = golden_cases("escape");
    assert_eq!(escapes.len(), 150);
    for case in &escapes {
        let cp = u32::try_from(case["codepoint"].as_i64().unwrap()).unwrap();
        let c = char::from_u32(cp).expect("scalar value");
        let mut s = AppSettings::new(&EnglishSettingsTexts);
        s.last_move_to_folder = Some(format!("a{c}b"));
        let segs = segments(&s.to_json().expect("writes"));
        assert_eq!(segs["LastMoveToFolder"], str_of(case, "json"), "code point {cp}");
    }
}

// C#: AppSettingsPocoTests.AppSettingsDefaults
#[test]
fn app_settings_defaults() {
    let s = AppSettings::default();
    assert_eq!(s.config_version, 3);
    assert_eq!(s.ui_language, "auto");
    assert_eq!(s.initial_view_mode, InitialViewMode::Fit);
    assert_eq!(s.loading_mode, Some(LoadingMode::Preview));
    assert!(!s.logging_enabled);
    assert_eq!(s.image_sort_mode, Some(ImageSortMode::Default));
    assert!(s.compare_hash_enabled);
    assert!(s.compare_size_enabled);
    assert!(!s.actions.is_empty());
    assert!(s.raw_support_enabled);
    assert_eq!(s.raw_full_decode, RawFullDecode::Never);
    assert_eq!(s.raw_pair_mode, RawPairMode::Separate);
    assert!(s.webp_heic_support_enabled);
}

// C#: AppSettingsPocoTests.AppSettingsDeserialization
#[test]
fn app_settings_deserialization() {
    let text = "{\n \"LoadingMode\": \"fast\",\n \"ImageSortMode\": \"Size\",\n \"InitialViewMode\": \"100%\"\n}";
    let s = read(text).expect("reads").expect("not null");
    assert_eq!(s.loading_mode, Some(LoadingMode::Fast));
    assert_eq!(s.image_sort_mode, Some(ImageSortMode::SizeDescending));
    assert_eq!(s.initial_view_mode, InitialViewMode::Percent100);
    assert!(s.raw_support_enabled);
    assert_eq!(s.raw_pair_mode, RawPairMode::Separate);
    assert!(s.webp_heic_support_enabled);
}

// C#: ConfigKeyContractTests.EveryPublishedKey_IsStillWritten
// C#: ConfigKeyContractTests.NewKeys_AreAddedToTheFrozenListOnPurpose
#[test]
fn written_default_document_has_exactly_the_frozen_keys_in_order() {
    let written = AppSettings::default().to_json().expect("writes");
    let keys: Vec<String> = segments(&written).into_keys().collect();
    let mut sorted_frozen: Vec<String> = JSON_KEYS.iter().map(|k| (*k).to_owned()).collect();
    sorted_frozen.sort();
    assert_eq!(keys, sorted_frozen);
    // Order: scan the document for each key in JSON_KEYS order.
    let mut from = 0;
    for key in JSON_KEYS {
        let needle = format!("\r\n  \"{key}\": ");
        let at = written[from..]
            .find(&needle)
            .unwrap_or_else(|| panic!("{key} missing or out of order"));
        from += at + needle.len();
    }
}

// C#: ConfigKeyContractTests.FrozenKeys_HaveNoDuplicates
#[test]
fn frozen_keys_have_no_duplicates() {
    let mut keys = JSON_KEYS.to_vec();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), JSON_KEYS.len());
    assert_eq!(JSON_KEYS.len(), 70);
}

// C#: SettingsDefaultsLiteralTests.PerformanceDefaults_AreTheShippedLiterals
#[test]
fn performance_defaults_are_the_shipped_literals() {
    let d = AppSettings::default();
    assert_eq!(d.image_cache_capacity_bytes, 16 * GIB);
    assert_eq!(d.image_cache_ram_percent, 50);
    assert_eq!(d.memory_reserve_bytes, 2 * GIB);
    assert_eq!(d.preload_worker_count, 8);
    assert_eq!(d.preload_memory_load_limit, 0.9);
    assert_eq!(d.preview_disk_cache_capacity_bytes, 4 * GIB);
    assert!(!d.use_source_bytes_cache);
    assert_eq!(d.source_bytes_capacity_bytes, 16 * GIB);
    assert_eq!(d.preload_forward_count, 32);
    assert_eq!(d.preload_backward_count, 8);
}

// C#: SettingsDefaultsLiteralTests.ViewingDefaults_AreTheShippedLiterals
#[test]
fn viewing_defaults_are_the_shipped_literals() {
    let d = AppSettings::default();
    assert_eq!(d.keyboard_zoom_step_percent, 10);
    assert_eq!(d.arrow_pan_step_percent, 10);
    assert_eq!(d.click_zoom_percent, 100);
    assert!(!d.click_to_zoom_enabled);
    assert!(!d.click_zoom_key_toggles_fit);
    assert!(!d.kinetic_pan_enabled);
    assert!(!d.keep_zoom_across_images);
    assert!(!d.confirm_before_delete);
    assert!(!d.allow_permanent_delete_without_recycle_bin);
    assert!(d.raw_support_enabled);
    assert!(d.webp_heic_support_enabled);
    assert!(d.touchpad_swipe_enabled);
    assert_eq!(d.touchpad_swipe_distance_per_image, 200);
    assert_eq!(d.fit_width_anchor, FitWidthAnchor::Centre);
    assert_eq!(d.fit_width_anchor2, Some(FitWidthAnchor::BottomThird));
}

// C#: ContextMenuLayoutTests.Clone_and_json_export_keep_the_list_and_an_empty_list_stays_empty
#[test]
fn clone_and_json_export_keep_the_list_and_an_empty_list_stays_empty() {
    let mut settings = AppSettings::new(&EnglishSettingsTexts);
    settings.hidden_context_menu_items = Some(vec![]);
    let text = settings.to_json().expect("writes");
    let parsed = read(&text).expect("reads").expect("not null");
    assert_eq!(parsed.hidden_context_menu_items, Some(vec![]));
    assert_eq!(settings.deep_clone().hidden_context_menu_items, Some(vec![]));
}

fn custom_settings() -> AppSettings {
    let mut s = AppSettings::new(&EnglishSettingsTexts);
    s.loading_mode = Some(LoadingMode::Fast);
    s.image_sort_mode = Some(ImageSortMode::SizeDescending);
    s.initial_view_mode = InitialViewMode::Percent100;
    s.logging_enabled = true;
    s.click_zoom_percent = 250;
    s.image_cache_capacity_bytes = 123_456_789_012;
    s.preload_memory_load_limit = 0.75;
    s.info_overlay_font_size = 13.5;
    s.ui_language = "vi".to_owned();
    s.external_editor_path = "C:\\Tools\\Ed & <x>.exe".to_owned();
    s.last_move_to_folder = Some("D:\\Out".to_owned());
    s.hidden_context_menu_items = Some(vec![Some("Fit".to_owned()), None]);
    s.shortcuts.next = "N".to_owned();
    s.actions[0].name = "Mine".to_owned();
    s.actions[0].confirm = true;
    s
}

// C#: SettingsRobustnessTests.EveryPublicSettingRoundTripsThroughClone (in spirit)
#[test]
fn every_public_setting_round_trips_through_clone_and_json() {
    let source = custom_settings();
    assert_eq!(source.deep_clone(), source);
    let text = source.to_json().expect("writes");
    let back = read(&text).expect("reads").expect("not null");
    assert_eq!(back, source);
}

// Contract "Done when": apply_json_property salvage.
#[test]
fn apply_json_property_salvage_keeps_other_properties_intact() {
    let doc = json::parse(
        r#"{"ConfigVersion":3,"LoggingEnabled":"not-a-bool","ClickZoomPercent":300,
            "Actions":[{"Name":"Mine","Shortcut":"F9","Operation":"Copy","Destination":"D"}],
            "Shortcuts":{"Next":"N"}}"#,
    )
    .expect("parses");
    let json::JsonValue::Object(entries) = &doc else {
        panic!("object")
    };
    let mut s = AppSettings::new(&EnglishSettingsTexts);
    let mut failed = Vec::new();
    for (name, value) in entries {
        let name = name.as_deref().expect("name");
        if s.apply_json_property(name, value, &EnglishSettingsTexts).is_err() {
            failed.push(name.to_owned());
        }
    }
    assert_eq!(failed, vec!["LoggingEnabled".to_owned()]);
    assert!(!s.logging_enabled);
    assert_eq!(s.click_zoom_percent, 300);
    assert_eq!(s.actions.len(), 1);
    assert_eq!(s.actions[0].name, "Mine");
    assert_eq!(s.shortcuts.next, "N");
}
