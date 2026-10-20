//! Red tests (W8-10b, test porter) for `pr_app::presenters::settings_field_map` (C# `Presenters/SettingsFieldMap.cs`,
//! baseline `d92d6aae`). Ported from `tests/PhotoReview.App.Tests/Presenters/SettingsFieldMapTests.cs` (the pure halves; the
//! presenter halves move to W8-10d) plus the golden `settings-field-map` (13 kinds, 617 cases).
//!
//! Every test fails through the `todo!()` of the stub until the port lands (the golden replays call `bindings()` or one of
//! the other stubs before they compare anything).

use std::collections::{BTreeMap, BTreeSet, HashSet};

use pr_app::presenters::action_list_json;
use pr_app::presenters::settings_field_map::{
    FieldContext, anchor_to_index, bindings, canonicalize_shortcuts, covered_properties, dotnet_double_text,
    index_to_anchor, index_to_view_mode, load_shortcuts, not_edited_in_dialog, read_shortcuts, shortcut_get,
    shortcut_properties, shortcut_set, shortcut_text, try_double, try_int, view_mode_to_index,
};
use pr_app::presenters::settings_form_state::{SettingsFieldError, SettingsFocusField, SettingsFormState};
use pr_core::model::enums::{FitWidthAnchor, InitialViewMode};
use pr_core::model::fields::{ExifInfoFields, TitleBarFields};
use pr_core::model::serde_ext::LenientEnum;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts, json};
use pr_core::settings::context_menu::{ContextMenuItemId, effective_hidden};
use pr_core::settings::shortcuts::ShortcutMappings;
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_l10n::template::NumberCulture;
use pr_test_support::golden;
use serde_json::{Map, Value, json};
use std::sync::Arc;

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Owns what `FieldContext` borrows: the English localizer and the invariant number culture.
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

fn default_settings() -> AppSettings {
    AppSettings::new(&EnglishSettingsTexts)
}

/// Runs every binding's `load` in table order (C# `foreach (var binding in Bindings) binding.Load(source, state)`).
fn load_all(source: &AppSettings) -> SettingsFormState {
    let mut state = SettingsFormState::default();
    for binding in bindings() {
        (binding.load)(source, &mut state);
    }
    state
}

fn names_to_ids(names: &[&str]) -> Vec<ContextMenuItemId> {
    names.iter().map(|n| id_by_name(n)).collect()
}

fn id_by_name(name: &str) -> ContextMenuItemId {
    ContextMenuItemId::ALL
        .iter()
        .copied()
        .find(|i| i.name() == name)
        .unwrap_or_else(|| panic!("unknown context menu id {name}"))
}

// ---------------------------------------------------------------------------------------------------------------------
// Form <-> JSON (the golden's camelCase keys)
// ---------------------------------------------------------------------------------------------------------------------

macro_rules! scalar_codec {
    (
        str: [$($sk:literal => $sf:ident),* $(,)?];
        int: [$($ik:literal => $if_:ident),* $(,)?];
        bool: [$($bk:literal => $bf:ident),* $(,)?];
        f64: [$($fk:literal => $ff:ident),* $(,)?];
    ) => {
        fn scalars_to_json(s: &SettingsFormState, m: &mut Map<String, Value>) {
            $( m.insert($sk.to_owned(), json!(s.$sf)); )*
            $( m.insert($ik.to_owned(), json!(s.$if_)); )*
            $( m.insert($bk.to_owned(), json!(s.$bf)); )*
            $( m.insert($fk.to_owned(), json!(s.$ff)); )*
        }

        fn scalars_from_json(v: &Value, s: &mut SettingsFormState) {
            $( s.$sf = v[$sk].as_str().unwrap_or_else(|| panic!("form key {} is not a string: {v}", $sk)).to_owned(); )*
            $( s.$if_ = i32::try_from(v[$ik].as_i64().unwrap_or_else(|| panic!("form key {} is not an integer", $ik)))
                .expect("i32"); )*
            $( s.$bf = v[$bk].as_bool().unwrap_or_else(|| panic!("form key {} is not a bool", $bk)); )*
            $( s.$ff = number_of(&v[$fk]).unwrap_or_else(|| panic!("form key {} is not a number", $fk)); )*
        }
    };
}

/// A JSON number or a string holding a number ("R" form, as the oracle writes doubles).
fn number_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

scalar_codec! {
    str: [
        "actionsJson" => actions_json,
        "externalEditorPath" => external_editor_path,
        "infoOverlayFontSizeText" => info_overlay_font_size_text,
        "toolbarAutoHideDelayText" => toolbar_auto_hide_delay_text,
        "infoOverlayAutoHideDelayText" => info_overlay_auto_hide_delay_text,
        "clickZoomPercentText" => click_zoom_percent_text,
        "arrowPanStepText" => arrow_pan_step_text,
        "keyboardZoomStepPercentText" => keyboard_zoom_step_percent_text,
        "imageTransitionMsText" => image_transition_ms_text,
        "touchpadSwipeDistanceText" => touchpad_swipe_distance_text,
        "preloadForwardText" => preload_forward_text,
        "preloadBackwardText" => preload_backward_text,
    ];
    int: [
        "viewModeIndex" => view_mode_index,
        "fitWidthAnchorIndex" => fit_width_anchor_index,
        "fitWidthAnchor2Index" => fit_width_anchor2_index,
        "middleClickActionIndex" => middle_click_action_index,
        "loadingModeIndex" => loading_mode_index,
        "scalingQualityIndex" => scaling_quality_index,
        "decoderBackendIndex" => decoder_backend_index,
        "instanceModeIndex" => instance_mode_index,
        "rawFullDecodeIndex" => raw_full_decode_index,
        "rawPairModeIndex" => raw_pair_mode_index,
        "mouseWheelActionIndex" => mouse_wheel_action_index,
        "kineticGlideSmoothingIndex" => kinetic_glide_smoothing_index,
        "keyboardZoomAnchorIndex" => keyboard_zoom_anchor_index,
        "imageTransitionIndex" => image_transition_index,
    ];
    bool: [
        "keepZoomAcrossImages" => keep_zoom_across_images,
        "compareHashEnabled" => compare_hash_enabled,
        "compareSizeEnabled" => compare_size_enabled,
        "rawSupportEnabled" => raw_support_enabled,
        "webpHeicSupportEnabled" => webp_heic_support_enabled,
        "loggingEnabled" => logging_enabled,
        "allowPermanentDelete" => allow_permanent_delete,
        "confirmBeforeDelete" => confirm_before_delete,
        "journalSafe" => journal_safe,
        "moveCopyReuseLastFolder" => move_copy_reuse_last_folder,
        "showInfoOverlay" => show_info_overlay,
        "showFileInfo" => show_file_info,
        "showFolderInfo" => show_folder_info,
        "showZoomIndicator" => show_zoom_indicator,
        "toolbarAutoHide" => toolbar_auto_hide,
        "infoOverlayAutoHide" => info_overlay_auto_hide,
        "showExifInfo" => show_exif_info,
        "clickToZoomEnabled" => click_to_zoom_enabled,
        "kineticPanEnabled" => kinetic_pan_enabled,
        "arrowKeyNavigatesAtZoomEdge" => arrow_key_navigates_at_zoom_edge,
        "clickZoomKeyTogglesFit" => click_zoom_key_toggles_fit,
        "touchpadSwipeEnabled" => touchpad_swipe_enabled,
    ];
    f64: [
        "toolbarOpacityValue" => toolbar_opacity_value,
        "ramCachePercent" => ram_cache_percent,
    ];
}

/// Every `SettingsFormState` field except `selected_language_code` (W8-10c's), under the oracle's camelCase names.
fn form_json(s: &SettingsFormState) -> Value {
    let mut m = Map::new();
    scalars_to_json(s, &mut m);
    m.insert("sortModeTag".to_owned(), json!(s.sort_mode_tag));
    m.insert("exifInfoFields".to_owned(), json!(s.exif_info_fields.bits()));
    m.insert("titleBarFields".to_owned(), json!(s.title_bar_fields.bits()));
    m.insert("shortcutTexts".to_owned(), json!(s.shortcut_texts));
    let mut shown: Vec<&str> = s.context_menu_shown.iter().map(|i| i.name()).collect();
    shown.sort_unstable();
    m.insert("contextMenuShown".to_owned(), json!(shown));
    Value::Object(m)
}

fn form_from_json(v: &Value) -> SettingsFormState {
    let mut s = SettingsFormState::default();
    scalars_from_json(v, &mut s);
    s.sort_mode_tag = v["sortModeTag"].as_str().map(str::to_owned);
    s.exif_info_fields =
        ExifInfoFields::from_bits_truncate(u32::try_from(v["exifInfoFields"].as_u64().expect("exif")).expect("u32"));
    s.title_bar_fields =
        TitleBarFields::from_bits_truncate(u32::try_from(v["titleBarFields"].as_u64().expect("title")).expect("u32"));
    s.shortcut_texts = v["shortcutTexts"]
        .as_object()
        .expect("shortcutTexts")
        .iter()
        .map(|(k, t)| (k.clone(), t.as_str().expect("shortcut text").to_owned()))
        .collect::<BTreeMap<_, _>>();
    s.context_menu_shown = v["contextMenuShown"]
        .as_array()
        .expect("contextMenuShown")
        .iter()
        .map(|n| id_by_name(n.as_str().expect("name")))
        .collect();
    s
}

/// `contextMenuShown`: a list of names, "ALL" or "ALL_BUT:<Name>" (see the golden input `note`).
fn expand_shown(v: &Value) -> Value {
    match v {
        Value::Array(_) => v.clone(),
        Value::String(marker) => {
            let skip = marker.strip_prefix("ALL_BUT:").map(id_by_name);
            let names: Vec<&str> = ContextMenuItemId::ALL
                .iter()
                .filter(|i| Some(**i) != skip)
                .map(|i| i.name())
                .collect();
            json!(names)
        }
        other => panic!("bad contextMenuShown {other}"),
    }
}

/// C# `FormFrom`: every binding's load on `source`, then the overrides (a `shortcutTexts` override replaces the whole map).
fn form_from(source: &AppSettings, overrides: Option<&Value>) -> SettingsFormState {
    let loaded = load_all(source);
    let Some(overrides) = overrides.and_then(Value::as_object) else {
        return loaded;
    };
    let mut merged = form_json(&loaded);
    let map = merged.as_object_mut().expect("object");
    for (key, value) in overrides {
        let value = if key == "contextMenuShown" {
            expand_shown(value)
        } else {
            value.clone()
        };
        assert!(map.contains_key(key), "unknown form key {key}");
        map.insert(key.clone(), value);
    }
    form_from_json(&merged)
}

// ---------------------------------------------------------------------------------------------------------------------
// Settings <-> JSON (C# property names)
// ---------------------------------------------------------------------------------------------------------------------

/// `new AppSettings()` with the case's keys set (`ActionsText` is the action list JSON text, `Shortcuts` an object).
fn settings_from(values: Option<&Value>) -> AppSettings {
    let mut settings = default_settings();
    let Some(values) = values.and_then(Value::as_object) else {
        return settings;
    };
    for (key, value) in values {
        let (name, text) = match key.as_str() {
            "ActionsText" => ("Actions", value.as_str().expect("ActionsText").to_owned()),
            other => (other, serde_json::to_string(value).expect("json")),
        };
        let parsed = json::parse(&text).unwrap_or_else(|e| panic!("settings key {key}: {e:?}"));
        let known = settings
            .apply_json_property(name, &parsed, &EnglishSettingsTexts)
            .unwrap_or_else(|e| panic!("settings key {key}: {e:?}"));
        assert!(known, "unknown settings key {key}");
    }
    settings
}

fn shortcuts_from(values: &Value) -> ShortcutMappings {
    let mut shortcuts = ShortcutMappings::default();
    for (name, value) in values.as_object().expect("shortcuts object") {
        *shortcuts.get_mut(name).unwrap_or_else(|| panic!("no shortcut {name}")) =
            value.as_str().expect("text").to_owned();
    }
    shortcuts
}

fn shortcuts_json(shortcuts: &ShortcutMappings) -> Value {
    let mut m = Map::new();
    for name in ShortcutMappings::PROPERTY_NAMES {
        m.insert(name.to_owned(), json!(shortcuts.get(name).expect("property")));
    }
    Value::Object(m)
}

/// The covered `AppSettings` values after an apply, in the oracle's `SettingsSnapshot` form.
fn settings_snapshot(settings: &AppSettings) -> Value {
    let written: Value = serde_json::from_str(&settings.to_json().expect("to_json")).expect("settings json");
    let mut m = Map::new();
    for &name in SETTINGS_NAMES {
        let value = match name {
            // Flags are written as text by `to_json`; the oracle writes the integer value.
            "ExifInfoFields" => json!(settings.exif_info_fields.bits()),
            "TitleBarFields" => json!(settings.title_bar_fields.bits()),
            // `to_json` writes "100%" for Percent100; the oracle writes the member name.
            "InitialViewMode" => json!(settings.initial_view_mode.name()),
            _ => written[name].clone(),
        };
        assert!(
            !value.is_null() || name == "HiddenContextMenuItems",
            "settings key {name} missing from to_json"
        );
        m.insert(name.to_owned(), value);
    }
    m.insert("Shortcuts".to_owned(), shortcuts_json(&settings.shortcuts));
    m.insert(
        "Actions".to_owned(),
        json!(action_list_json::serialize(&settings.actions)),
    );
    let mut hidden: Vec<&str> = effective_hidden(settings).into_iter().map(|i| i.name()).collect();
    hidden.sort_unstable();
    m.insert("EffectiveHiddenContextMenuItems".to_owned(), json!(hidden));
    Value::Object(m)
}

const SETTINGS_NAMES: &[&str] = &[
    "InitialViewMode",
    "FitWidthAnchor",
    "FitWidthAnchor2",
    "MiddleClickAction",
    "KeepZoomAcrossImages",
    "ImageSortMode",
    "ScalingQuality",
    "DecoderBackend",
    "LoadingMode",
    "InstanceMode",
    "CompareHashEnabled",
    "CompareSizeEnabled",
    "RawSupportEnabled",
    "WebpHeicSupportEnabled",
    "RawFullDecode",
    "RawPairMode",
    "LoggingEnabled",
    "AllowPermanentDeleteWithoutRecycleBin",
    "ConfirmBeforeDelete",
    "ExternalEditorPath",
    "JournalDurability",
    "ImageCacheRamPercent",
    "ShowInfoOverlay",
    "ShowFileInfo",
    "ShowFolderInfo",
    "MouseWheelAction",
    "ClickToZoomEnabled",
    "KineticPanEnabled",
    "KineticGlideSmoothing",
    "ArrowKeyNavigatesAtZoomEdge",
    "ClickZoomKeyTogglesFit",
    "KeyboardZoomAnchor",
    "MoveCopyReuseLastFolder",
    "ShowExifInfo",
    "ExifInfoFields",
    "TitleBarFields",
    "ClickZoomPercent",
    "ArrowPanStepPercent",
    "KeyboardZoomStepPercent",
    "ImageTransition",
    "ImageTransitionMs",
    "TouchpadSwipeEnabled",
    "TouchpadSwipeDistancePerImage",
    "PreloadForwardCount",
    "PreloadBackwardCount",
    "ToolbarAutoHide",
    "ToolbarAutoHideDelayMs",
    "InfoOverlayAutoHide",
    "InfoOverlayAutoHideDelayMs",
    "ToolbarOpacityPercent",
    "InfoOverlayFontSize",
    "ShowZoomIndicator",
    "ShowFolderMenuItems",
    "ShowZoomMenuItems",
    "ShowRecycleMenuItem",
    "UiLanguage",
];

fn refusal_json(refusal: &Option<SettingsFieldError>) -> Value {
    match refusal {
        None => Value::Null,
        Some(e) => json!({ "message": e.message, "focus": format!("{:?}", e.focus) }),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden access and comparison
// ---------------------------------------------------------------------------------------------------------------------

/// One golden case: its input (from `tests/golden-inputs`) and the oracle's `result`.
struct Case {
    id: String,
    input: Value,
    result: Value,
}

fn golden_cases(kind: &str) -> Vec<Case> {
    let golden = golden::load("settings-field-map");
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("settings-field-map.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let inputs: Value = serde_json::from_str(&text).expect("parse inputs");
    let by_id: BTreeMap<&str, &Value> = inputs["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|c| (c["id"].as_str().expect("id"), c))
        .collect();
    let cases: Vec<Case> = golden
        .cases
        .iter()
        .filter(|c| c["kind"] == kind)
        .map(|c| {
            let id = c["id"].as_str().expect("id").to_owned();
            let input = (*by_id.get(id.as_str()).unwrap_or_else(|| panic!("no input for {id}"))).clone();
            Case {
                id,
                input,
                result: c["result"].clone(),
            }
        })
        .collect();
    assert!(!cases.is_empty(), "no golden cases of kind {kind}");
    cases
}

/// Structural equality; a number and a string holding the same number are equal (the oracle writes doubles as "R" strings).
fn assert_same(expected: &Value, actual: &Value, path: &str) {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            let ek: BTreeSet<&String> = e.keys().collect();
            let ak: BTreeSet<&String> = a.keys().collect();
            assert_eq!(ek, ak, "{path}: keys");
            for (k, ev) in e {
                assert_same(ev, &a[k], &format!("{path}.{k}"));
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            assert_eq!(e.len(), a.len(), "{path}: length");
            for (i, (ev, av)) in e.iter().zip(a).enumerate() {
                assert_same(ev, av, &format!("{path}[{i}]"));
            }
        }
        (Value::String(e), Value::String(a)) => assert_eq!(e, a, "{path}"),
        (Value::Number(_), Value::Number(_) | Value::String(_)) | (Value::String(_), Value::Number(_)) => {
            let (en, an) = (number_of(expected), number_of(actual));
            assert!(
                en.is_some() && an.is_some(),
                "{path}: expected {expected}, got {actual}"
            );
            assert_eq!(en, an, "{path}");
        }
        _ => assert_eq!(expected, actual, "{path}"),
    }
}

fn replay_load(case: &Case) {
    let state = load_all(&settings_from(case.input.get("settings")));
    assert_same(&case.result, &form_json(&state), &case.id);
}

fn replay_apply_row(env: &Env, case: &Case) {
    let row = usize::try_from(case.input["row"].as_u64().expect("row")).expect("usize");
    let source = settings_from(case.input.get("settings"));
    let form = form_from(&source, case.input.get("form"));
    let mut target = settings_from(case.input.get("settings"));
    let refusal = (bindings()[row].apply)(&form, &mut target, &env.ctx());
    let actual = json!({ "refused": refusal_json(&refusal), "settings": settings_snapshot(&target) });
    assert_same(&case.result, &actual, &case.id);
}

fn apply_all(
    env: &Env,
    form: &SettingsFormState,
    target: &mut AppSettings,
) -> (Option<usize>, Option<SettingsFieldError>) {
    for (row, binding) in bindings().iter().enumerate() {
        if let Some(refusal) = (binding.apply)(form, target, &env.ctx()) {
            return (Some(row), Some(refusal));
        }
    }
    (None, None)
}

fn replay_apply_all(env: &Env, case: &Case) {
    let source = settings_from(case.input.get("settings"));
    let form = form_from(&source, case.input.get("form"));
    let mut target = settings_from(case.input.get("settings"));
    let (refused_row, refusal) = apply_all(env, &form, &mut target);
    let actual = json!({
        "refused_row": refused_row,
        "refused": refusal_json(&refusal),
        "settings": settings_snapshot(&target),
    });
    assert_same(&case.result, &actual, &case.id);
}

fn anchor_named(name: &str) -> FitWidthAnchor {
    FitWidthAnchor::MEMBERS
        .iter()
        .copied()
        .find(|m| m.name() == name)
        .unwrap_or_else(|| panic!("anchor {name}"))
}

fn view_mode_named(name: &str) -> InitialViewMode {
    InitialViewMode::MEMBERS
        .iter()
        .copied()
        .find(|m| m.name() == name)
        .unwrap_or_else(|| panic!("view mode {name}"))
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported C# tests
// ---------------------------------------------------------------------------------------------------------------------

// C#: SettingsFieldMapTests.EveryProperty_IsMappedOrExplicitlyNotEditable
// Golden `tables.settable` (the reflected settable properties) = covered union not-edited, each once.
#[test]
fn every_settable_property_is_mapped_or_excluded() {
    let tables = &golden_cases("tables")[0].result;
    let covered = covered_properties();
    let not_edited: Vec<&str> = not_edited_in_dialog().iter().map(|(name, _)| *name).collect();
    let settable: Vec<&str> = tables["settable"]
        .as_array()
        .expect("settable")
        .iter()
        .map(|v| v.as_str().expect("name"))
        .collect();

    let unaccounted: Vec<&&str> = settable
        .iter()
        .filter(|n| !covered.contains(**n) && !not_edited.contains(n))
        .collect();
    assert!(
        unaccounted.is_empty(),
        "Add a field row (or a reasoned exclusion) for: {unaccounted:?}"
    );

    let mut union: Vec<&str> = covered.iter().copied().chain(not_edited.iter().copied()).collect();
    let total = union.len();
    union.sort_unstable();
    union.dedup();
    assert_eq!(
        total,
        union.len(),
        "a property is both mapped and excluded, or listed twice"
    );
    let mut sorted_settable = settable.clone();
    sorted_settable.sort_unstable();
    assert_eq!(union, sorted_settable);
}

// C#: SettingsFieldMapTests.Lists_NameOnlyRealPropertiesAndDoNotOverlap
#[test]
fn covered_and_not_edited_name_real_properties_disjointly() {
    let tables = &golden_cases("tables")[0].result;
    let all: HashSet<&str> = tables["settable"]
        .as_array()
        .expect("settable")
        .iter()
        .map(|v| v.as_str().expect("name"))
        .collect();
    let covered = covered_properties();
    assert!(
        covered.iter().all(|n| all.contains(n)),
        "covered names a property that does not exist"
    );
    let not_edited = not_edited_in_dialog();
    assert!(
        not_edited.iter().all(|(n, _)| all.contains(n)),
        "not-edited names a property that does not exist"
    );
    assert!(
        not_edited.iter().all(|(n, _)| !covered.contains(n)),
        "covered and not-edited overlap"
    );
    assert!(
        not_edited.iter().all(|(_, reason)| !reason.trim().is_empty()),
        "blank reason"
    );
}

// C#: SettingsFieldMapTests.Bindings_OwnDistinctProperties
#[test]
fn bindings_are_nonempty_and_own_distinct_properties() {
    let rows = bindings();
    assert_eq!(rows.len(), 47);
    assert!(rows.iter().all(|b| !b.properties.is_empty()));
    let owned: Vec<&str> = rows.iter().flat_map(|b| b.properties.iter().copied()).collect();
    let distinct: HashSet<&str> = owned.iter().copied().collect();
    assert_eq!(owned.len(), distinct.len());
    assert_eq!(
        owned.len(),
        58,
        "58 row properties plus UiLanguage make the 59 covered names"
    );
}

// C#: SettingsFieldMapTests.ShortcutAccessors_CoverEveryShortcutProperty
#[test]
fn shortcut_properties_are_every_box_except_move_to_folder2() {
    let mapped = shortcut_properties();
    assert_eq!(mapped.len(), 27);
    let distinct: HashSet<&str> = mapped.iter().copied().collect();
    assert_eq!(mapped.len(), distinct.len(), "duplicate box");
    let mut expected: Vec<&str> = ShortcutMappings::PROPERTY_NAMES
        .iter()
        .copied()
        .filter(|n| *n != "MoveToFolder2")
        .collect();
    expected.sort_unstable();
    let mut got = mapped.to_vec();
    got.sort_unstable();
    assert_eq!(expected, got);
}

// C#: SettingsFieldMapTests.ShortcutAccessors_ReadAndWriteTheirOwnProperty
#[test]
fn shortcut_get_and_set_touch_only_their_property() {
    for property in shortcut_properties() {
        let mut shortcuts = ShortcutMappings::default();
        let before = shortcuts.clone();
        shortcut_set(&mut shortcuts, property, format!("probe-{property}"));
        assert_eq!(
            shortcuts.get(property),
            Some(format!("probe-{property}").as_str()),
            "{property}: set"
        );
        for other in ShortcutMappings::PROPERTY_NAMES.iter().filter(|n| *n != property) {
            assert_eq!(
                shortcuts.get(other),
                before.get(other),
                "{property}: set changed {other}"
            );
        }
        *shortcuts.get_mut(property).expect("property") = format!("other-{property}");
        assert_eq!(
            shortcut_get(&shortcuts, property),
            format!("other-{property}"),
            "{property}: get"
        );
    }
}

// C#: SettingsFieldMapTests.Shortcuts_RoundTripEachProperty (pure half; the presenter half moves to W8-10d)
// Distinct valid key names for the 27 boxes; MoveToFolder2 is carried over, not shown.
#[test]
fn shortcut_round_trip_load_read() {
    let keys = [
        "A", "B", "C", "D", "E", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W",
        "X", "Y", "Z", "F1", "F2",
    ];
    let properties = shortcut_properties();
    assert_eq!(properties.len(), keys.len());
    let mut source = ShortcutMappings::default();
    for (property, key) in properties.iter().zip(keys) {
        shortcut_set(&mut source, property, key.to_owned());
    }
    source.move_to_folder2 = "F9".to_owned();

    let mut state = SettingsFormState::default();
    load_shortcuts(&source, &mut state);
    for (property, key) in properties.iter().zip(keys) {
        assert_eq!(shortcut_text(&state, property), key, "{property}: loaded text");
    }

    let raw = read_shortcuts(&state);
    for (property, key) in properties.iter().zip(keys) {
        assert_eq!(shortcut_get(&raw, property), key, "{property}: read");
    }
    assert_eq!(
        raw.move_to_folder2,
        ShortcutMappings::default().move_to_folder2,
        "MoveToFolder2 keeps the default when read"
    );

    let saved = canonicalize_shortcuts(&raw, &source);
    for (property, key) in properties.iter().zip(keys) {
        assert_eq!(shortcut_get(&saved, property), key, "{property}: canonical");
    }
    assert_eq!(
        saved.move_to_folder2, "F9",
        "MoveToFolder2 is copied from the existing settings"
    );

    // Row 45 (Shortcuts): Save writes every box back even when the settings were cleared.
    let env = Env::new();
    let mut target = default_settings();
    for property in properties {
        shortcut_set(&mut target.shortcuts, property, String::new());
    }
    target.shortcuts.move_to_folder2 = "F9".to_owned();
    assert_eq!(bindings()[45].properties, ["Shortcuts"]);
    assert_eq!((bindings()[45].apply)(&state, &mut target, &env.ctx()), None);
    for (property, key) in properties.iter().zip(keys) {
        assert_eq!(shortcut_get(&target.shortcuts, property), key, "{property}: applied");
    }
    assert_eq!(target.shortcuts.move_to_folder2, "F9");
}

// C#: SettingsFieldMapTests.MappedProperties_RoundTripThroughLoadAndSave (pure half; the presenter half moves to W8-10d)
// Golden `apply-row-NN-roundtrip-marker`: every row writes its loaded marker values back.
#[test]
fn every_row_roundtrips_its_marker_through_load_and_apply() {
    let env = Env::new();
    let cases: Vec<Case> = golden_cases("apply_row")
        .into_iter()
        .filter(|c| c.id.contains("roundtrip-marker"))
        .collect();
    assert_eq!(cases.len(), 47, "one marker case per row");
    for case in &cases {
        replay_apply_row(&env, case);
    }
}

// C#: SettingsFieldMapTests.ContextMenu_RoundTripsHiddenSet_AndKeepsLockedItemsShown (pure half; presenter half moves to W8-10d)
// Golden `load-context-*` and `apply-context-*`.
#[test]
fn context_menu_rows_roundtrip_hidden_set_and_keep_locked_shown() {
    let env = Env::new();
    let loads: Vec<Case> = golden_cases("load")
        .into_iter()
        .filter(|c| c.id.starts_with("load-context-"))
        .collect();
    let applies: Vec<Case> = golden_cases("apply_row")
        .into_iter()
        .filter(|c| c.id.starts_with("apply-context-"))
        .collect();
    assert_eq!(loads.len(), 4);
    assert_eq!(applies.len(), 3);
    for case in &loads {
        replay_load(case);
    }
    for case in &applies {
        replay_apply_row(&env, case);
    }

    // Hand-written twin of the C# test: three items hidden, the locked Settings item always shown, one more unticked.
    let hidden = names_to_ids(&["Undo", "MoveToRecycleBin", "Fit"]);
    let mut source = default_settings();
    pr_core::settings::context_menu::apply_hidden(&mut source, hidden.iter().copied());
    let mut state = load_all(&source);
    assert_eq!(
        state.context_menu_shown.len(),
        ContextMenuItemId::ALL.len() - hidden.len()
    );
    assert!(hidden.iter().all(|id| !state.context_menu_shown.contains(id)));
    assert!(state.context_menu_shown.contains(&ContextMenuItemId::Settings));

    let mut target = default_settings();
    target.hidden_context_menu_items = None;
    assert_eq!((bindings()[30].apply)(&state, &mut target, &env.ctx()), None);
    let expected: HashSet<ContextMenuItemId> = hidden.into_iter().collect();
    assert_eq!(effective_hidden(&target), expected);

    state.context_menu_shown.remove(&ContextMenuItemId::OpenFolder);
    state.context_menu_shown.remove(&ContextMenuItemId::Settings); // locked: can never be hidden
    assert_eq!((bindings()[30].apply)(&state, &mut target, &env.ctx()), None);
    let hidden_now = effective_hidden(&target);
    assert!(hidden_now.contains(&ContextMenuItemId::OpenFolder));
    assert!(!hidden_now.contains(&ContextMenuItemId::Settings));
}

// C#: SettingsFieldMapTests.FieldFlags_RoundTrip (rows 35 and 36)
#[test]
fn flags_roundtrip_through_load_and_apply_rows() {
    let env = Env::new();
    let mut source = default_settings();
    source.exif_info_fields = ExifInfoFields::CAMERA | ExifInfoFields::ISO;
    source.title_bar_fields = TitleBarFields::FILE_NAME | TitleBarFields::FOLDER_PATH;

    let mut state = load_all(&source);
    assert_eq!(state.exif_info_fields, ExifInfoFields::CAMERA | ExifInfoFields::ISO);
    assert_eq!(
        state.title_bar_fields,
        TitleBarFields::FILE_NAME | TitleBarFields::FOLDER_PATH
    );

    state.exif_info_fields = ExifInfoFields::LENS;
    state.title_bar_fields = TitleBarFields::NONE;
    assert_eq!(bindings()[35].properties, ["ExifInfoFields"]);
    assert_eq!(bindings()[36].properties, ["TitleBarFields"]);
    assert_eq!((bindings()[35].apply)(&state, &mut source, &env.ctx()), None);
    assert_eq!((bindings()[36].apply)(&state, &mut source, &env.ctx()), None);
    assert_eq!(source.exif_info_fields, ExifInfoFields::LENS);
    assert_eq!(source.title_bar_fields, TitleBarFields::NONE);
}

// ---------------------------------------------------------------------------------------------------------------------
// New tests: hand-written boundary cases
// ---------------------------------------------------------------------------------------------------------------------

// new: try_int boundaries (T3, T21).
#[test]
fn try_int_accepts_digits_sign_and_dotnet_whitespace() {
    assert_eq!(try_int("12", 10, 800), Some(12));
    assert_eq!(try_int("  12\t", 10, 800), Some(12));
    assert_eq!(try_int("\u{000B}\u{000C}12\r\n", 10, 800), Some(12));
    assert_eq!(try_int("+12", 10, 800), Some(12));
    assert_eq!(try_int("-5", -10, 5), Some(-5));
    assert_eq!(try_int("10", 10, 800), Some(10), "lower bound included");
    assert_eq!(try_int("800", 10, 800), Some(800), "upper bound included");
    assert_eq!(try_int("-2147483648", i32::MIN, i32::MAX), Some(i32::MIN));
    assert_eq!(try_int("2147483647", i32::MIN, i32::MAX), Some(i32::MAX));
}

// new: try_int refusals (T3, T21).
#[test]
fn try_int_refuses_what_dotnet_refuses() {
    assert_eq!(try_int("9", 10, 800), None, "below min");
    assert_eq!(try_int("801", 10, 800), None, "above max");
    assert_eq!(try_int("", 10, 800), None);
    assert_eq!(try_int("   ", 10, 800), None);
    assert_eq!(try_int("+", 10, 800), None);
    assert_eq!(try_int("12.0", 10, 800), None, "no fractional part, not even zeros");
    assert_eq!(try_int("1,000", 10, 8000), None, "no thousands separator");
    assert_eq!(try_int("1e2", 10, 800), None, "no exponent");
    assert_eq!(try_int("1 2", 10, 800), None);
    assert_eq!(try_int("--12", 10, 800), None);
    assert_eq!(
        try_int("\u{00A0}12", 10, 800),
        None,
        "NBSP is not .NET integer whitespace (trim() would accept it)"
    );
    assert_eq!(try_int("12\u{00A0}", 10, 800), None);
    assert_eq!(try_int("\u{200B}12", 10, 800), None);
    assert_eq!(try_int("\u{FF11}\u{FF12}", 10, 800), None, "full-width digits");
    assert_eq!(try_int("2147483648", i32::MIN, i32::MAX), None, "i32 overflow");
    assert_eq!(try_int("-2147483649", i32::MIN, i32::MAX), None);
}

// new: try_double accepts (T4).
#[test]
fn try_double_accepts_float_styles() {
    assert_eq!(try_double("12"), Some(12.0));
    assert_eq!(try_double(" 12 "), Some(12.0));
    assert_eq!(try_double("12.5"), Some(12.5));
    assert_eq!(try_double("8."), Some(8.0));
    assert_eq!(try_double(".5"), Some(0.5));
    assert_eq!(try_double("-1.5"), Some(-1.5));
    assert_eq!(try_double("+2"), Some(2.0));
    assert_eq!(try_double("1e2"), Some(100.0));
    assert_eq!(try_double("1E+2"), Some(100.0));
    assert_eq!(try_double("25e-1"), Some(2.5));
}

// new: try_double symbols and overflow parse, the caller refuses them (T4).
#[test]
fn try_double_parses_symbols_and_overflow_to_non_finite() {
    assert!(try_double("NaN").is_some_and(f64::is_nan));
    assert_eq!(try_double("Infinity"), Some(f64::INFINITY));
    assert_eq!(try_double("-Infinity"), Some(f64::NEG_INFINITY));
    assert_eq!(try_double("1e400"), Some(f64::INFINITY));
}

// new: try_double refusals (T4).
#[test]
fn try_double_refuses_what_dotnet_refuses() {
    assert_eq!(try_double(""), None);
    assert_eq!(try_double("   "), None);
    assert_eq!(try_double("12,5"), None);
    assert_eq!(try_double("1_0"), None);
    assert_eq!(try_double("0x10"), None);
    assert_eq!(try_double("1e"), None);
    assert_eq!(try_double("."), None);
    assert_eq!(try_double("\u{00A0}1"), None);
    assert_eq!(try_double("1\u{200B}"), None);
    assert_eq!(try_double("1 2"), None);
}

// new: dotnet_double_text in the usual range (T13).
#[test]
fn dotnet_double_text_uses_shortest_digits_without_trailing_zero() {
    assert_eq!(dotnet_double_text(8.0), "8");
    assert_eq!(dotnet_double_text(12.0), "12");
    assert_eq!(dotnet_double_text(12.5), "12.5");
    assert_eq!(dotnet_double_text(24.0), "24");
    assert_eq!(dotnet_double_text(0.1), "0.1");
    assert_eq!(dotnet_double_text(-3.25), "-3.25");
    assert_eq!(dotnet_double_text(0.0), "0");
    assert_eq!(dotnet_double_text(123_456_789_012_345.0), "123456789012345");
}

// new: dotnet_double_text symbols and exponent form (T13).
#[test]
fn dotnet_double_text_writes_symbols_and_exponents() {
    assert_eq!(dotnet_double_text(f64::NAN), "NaN");
    assert_eq!(dotnet_double_text(f64::INFINITY), "Infinity");
    assert_eq!(dotnet_double_text(f64::NEG_INFINITY), "-Infinity");
    assert_eq!(dotnet_double_text(1e15), "1000000000000000"); // measured on .NET 10 ("R"): fixed notation below 1e17
    assert_eq!(dotnet_double_text(1e-5), "1E-05");
}

// new: index tables (semantics 2, 3, T17).
#[test]
fn view_mode_and_anchor_index_tables() {
    let modes = [
        (InitialViewMode::Fit, 0),
        (InitialViewMode::FitWidth, 1),
        (InitialViewMode::FitHeight, 2),
        (InitialViewMode::ClickZoomLevel, 3),
        (InitialViewMode::Percent100, 4),
        (InitialViewMode::Percent200, 5),
        (InitialViewMode::Percent400, 5),
    ];
    for (mode, index) in modes {
        assert_eq!(view_mode_to_index(mode), index, "{mode:?}");
    }
    assert_eq!(
        index_to_view_mode(5),
        InitialViewMode::Percent200,
        "a stray 400 % lands on the 200 % item"
    );
    assert_eq!(index_to_view_mode(-1), InitialViewMode::Fit);
    assert_eq!(index_to_view_mode(6), InitialViewMode::Fit);

    assert_eq!(anchor_to_index(FitWidthAnchor::Centre), 0);
    assert_eq!(anchor_to_index(FitWidthAnchor::TopThird), 1);
    assert_eq!(anchor_to_index(FitWidthAnchor::BottomThird), 2);
    assert_eq!(index_to_anchor(1, FitWidthAnchor::Centre), FitWidthAnchor::TopThird);
    assert_eq!(
        index_to_anchor(-1, FitWidthAnchor::BottomThird),
        FitWidthAnchor::BottomThird
    );
    assert_eq!(index_to_anchor(3, FitWidthAnchor::Centre), FitWidthAnchor::Centre);
}

// new: apply_all stops at the first refusal and keeps the rows before it (semantics 1, T7).
#[test]
fn apply_all_stops_at_first_refusal_and_keeps_earlier_rows() {
    let env = Env::new();
    let source = default_settings();
    let mut form = load_all(&source);
    form.click_zoom_percent_text = "300".to_owned(); // row 37: valid, written
    form.arrow_pan_step_text = "0".to_owned(); // row 38: refused
    form.keyboard_zoom_step_percent_text = "55".to_owned(); // row 39: never reached

    let mut target = default_settings();
    let (row, refusal) = apply_all(&env, &form, &mut target);

    assert_eq!(row, Some(38));
    assert_eq!(
        refusal,
        Some(SettingsFieldError::new(
            "Arrow key step must be a number between 1 and 100.",
            SettingsFocusField::ArrowPanStep
        ))
    );
    assert_eq!(target.click_zoom_percent, 300, "the earlier row stays written");
    assert_eq!(
        target.keyboard_zoom_step_percent, source.keyboard_zoom_step_percent,
        "the later row is not applied"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// New tests: one per golden kind
// ---------------------------------------------------------------------------------------------------------------------

// new: golden `tables`.
#[test]
fn golden_settings_field_map_tables() {
    let tables = golden_cases("tables").remove(0).result;
    let strings = |key: &str| -> Vec<String> {
        tables[key]
            .as_array()
            .expect(key)
            .iter()
            .map(|v| v.as_str().expect("name").to_owned())
            .collect()
    };

    let covered: Vec<String> = covered_properties().into_iter().map(str::to_owned).collect();
    assert_eq!(covered, strings("covered_sorted"));
    assert_eq!(covered.len(), 59);

    let shortcut_names: Vec<String> = shortcut_properties().iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(shortcut_names, strings("shortcut_names_in_order"));

    let not_edited: Vec<Value> = not_edited_in_dialog()
        .iter()
        .map(|(name, reason)| json!([name, reason]))
        .collect();
    assert_eq!(Value::Array(not_edited), tables["not_edited_in_order"]);

    let rows: Vec<Value> = bindings().iter().map(|b| json!(b.properties)).collect();
    assert_eq!(
        Value::Array(rows),
        tables["bindings_in_order"],
        "row order is the Save order (T10)"
    );
}

// new: golden `load` (117 cases).
#[test]
fn golden_settings_field_map_load() {
    for case in golden_cases("load") {
        replay_load(&case);
    }
}

// new: golden `apply_row` (243 cases).
#[test]
fn golden_settings_field_map_apply_row() {
    let env = Env::new();
    for case in golden_cases("apply_row") {
        replay_apply_row(&env, &case);
    }
}

// new: golden `apply_all` (6 cases): refused row, message, focus and the partial state.
#[test]
fn golden_settings_field_map_apply_all() {
    let env = Env::new();
    for case in golden_cases("apply_all") {
        replay_apply_all(&env, &case);
    }
}

// new: golden `shortcuts_read`.
#[test]
fn golden_settings_field_map_shortcuts_read() {
    for case in golden_cases("shortcuts_read") {
        let mut state = SettingsFormState::default();
        for (name, value) in case.input["form"]["shortcutTexts"].as_object().expect("shortcutTexts") {
            state
                .shortcut_texts
                .insert(name.clone(), value.as_str().expect("text").to_owned());
        }
        assert_same(&case.result, &shortcuts_json(&read_shortcuts(&state)), &case.id);
    }
}

// new: golden `shortcuts_canonicalize`.
#[test]
fn golden_settings_field_map_shortcuts_canonicalize() {
    for case in golden_cases("shortcuts_canonicalize") {
        let raw = shortcuts_from(&case.input["raw"]);
        let existing = shortcuts_from(&case.input["existing"]);
        assert_same(
            &case.result,
            &shortcuts_json(&canonicalize_shortcuts(&raw, &existing)),
            &case.id,
        );
    }
}

// new: golden `anchor_to_index`.
#[test]
fn golden_settings_field_map_anchor_to_index() {
    for case in golden_cases("anchor_to_index") {
        let anchor = anchor_named(case.input["anchor"].as_str().expect("anchor"));
        assert_eq!(json!({ "index": anchor_to_index(anchor) }), case.result, "{}", case.id);
    }
}

// new: golden `index_to_anchor`.
#[test]
fn golden_settings_field_map_index_to_anchor() {
    for case in golden_cases("index_to_anchor") {
        let index = i32::try_from(case.input["index"].as_i64().expect("index")).expect("i32");
        let fallback = anchor_named(case.input["fallback"].as_str().expect("fallback"));
        assert_eq!(
            json!({ "anchor": index_to_anchor(index, fallback).name() }),
            case.result,
            "{}",
            case.id
        );
    }
}

// new: golden `view_mode_to_index`.
#[test]
fn golden_settings_field_map_view_mode_to_index() {
    for case in golden_cases("view_mode_to_index") {
        let mode = view_mode_named(case.input["mode"].as_str().expect("mode"));
        assert_eq!(json!({ "index": view_mode_to_index(mode) }), case.result, "{}", case.id);
    }
}

// new: golden `index_to_view_mode`.
#[test]
fn golden_settings_field_map_index_to_view_mode() {
    for case in golden_cases("index_to_view_mode") {
        let index = i32::try_from(case.input["index"].as_i64().expect("index")).expect("i32");
        assert_eq!(
            json!({ "mode": index_to_view_mode(index).name() }),
            case.result,
            "{}",
            case.id
        );
    }
}

// new: golden `try_int` (165 cases: 33 texts x 5 bounds).
#[test]
fn golden_settings_field_map_try_int() {
    for case in golden_cases("try_int") {
        let text = case.input["text"].as_str().expect("text");
        let min = i32::try_from(case.input["min"].as_i64().expect("min")).expect("i32");
        let max = i32::try_from(case.input["max"].as_i64().expect("max")).expect("i32");
        let actual = try_int(text, min, max);
        let expected = json!({ "ok": actual.is_some(), "value": actual });
        assert_eq!(expected, case.result, "{} ({text:?}, {min}..{max})", case.id);
    }
}

// new: golden `try_double` (31 cases).
#[test]
fn golden_settings_field_map_try_double() {
    for case in golden_cases("try_double") {
        let text = case.input["text"].as_str().expect("text");
        let actual = try_double(text);
        let ok = case.result["ok"].as_bool().expect("ok");
        assert_eq!(actual.is_some(), ok, "{} ({text:?})", case.id);
        if let Some(value) = actual {
            let expected: f64 = case.result["value"].as_str().expect("value").parse().expect("R text");
            let same = if expected.is_nan() {
                value.is_nan()
            } else {
                value == expected
            };
            assert!(same, "{} ({text:?}): expected {expected}, got {value}", case.id);
            assert_eq!(
                value.is_finite(),
                case.result["finite"].as_bool().expect("finite"),
                "{}",
                case.id
            );
        }
    }
}

// new: golden `double_text` (16 cases).
#[test]
fn golden_settings_field_map_double_text() {
    for case in golden_cases("double_text") {
        let value: f64 = parse_r(case.input["value"].as_str().expect("value"));
        assert_eq!(
            json!({ "text": dotnet_double_text(value) }),
            case.result,
            "{} ({value:e})",
            case.id
        );
    }
}

fn parse_r(text: &str) -> f64 {
    text.trim()
        .parse::<f64>()
        .unwrap_or_else(|e| panic!("bad double {text:?}: {e}"))
}
