//! Tests of `presenters::settings_field_map` (C# `PhotoReview.App.Shared/Presenters/SettingsFieldMap.cs`, baseline
//! `d92d6aae`). The C# tests are `tests/PhotoReview.App.Tests/Presenters/SettingsFieldMapTests.cs`; each test names the one
//! it ports in a `// C#:` comment, and `// new:` marks a test the C# suite does not have. Contract: `contracts/W8-10b.md`.
//!
//! Golden-driven tests read `tests/golden/settings-field-map.json` (outputs) joined with
//! `tests/golden-inputs/settings-field-map.json` (inputs) by case id. Every test here is RED while the bodies of
//! `settings_field_map.rs` are `todo!()`.
//!
//! Documented skips (moved, not dropped; the owner is named in the contract's C# test table):
//! - `Shortcuts_RoundTripEachProperty` and `MappedProperties_RoundTripThroughLoadAndSave` need `SettingsPresenter`: W8-10d.
//! - The presenter half of `ContextMenu_RoundTripsHiddenSet_AndKeepsLockedItemsShown`: W8-10d.

use std::collections::{BTreeMap, BTreeSet};

use pr_app::presenters::action_list_json;
use pr_app::presenters::settings_field_map::{self as map, FieldContext};
use pr_app::presenters::settings_form_state::{SettingsFieldError, SettingsFormState};
use pr_core::model::enums::{FitWidthAnchor, InitialViewMode};
use pr_core::model::fields::{ExifInfoFields, TitleBarFields};
use pr_core::model::serde_ext::LenientEnum;
use pr_core::settings::app_settings::{json as settings_json, AppSettings, EnglishSettingsTexts};
use pr_core::settings::context_menu::{self, ContextMenuItemId};
use pr_core::settings::shortcuts::ShortcutMappings;
use pr_l10n::localizer::{built_in_english_localizer, Localizer};
use pr_l10n::template::NumberCulture;
use pr_test_support::golden::repo_root;
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------------------------------------------------
// Golden access
// ---------------------------------------------------------------------------------------------------------------------

/// One golden case: the output (`expected`) and the input it was produced from (`input`), joined by id.
struct Case {
    id: String,
    kind: String,
    input: Value,
    expected: Value,
}

fn read_json(relative: &str) -> Value {
    let path = repo_root().join(relative);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// Every golden case, joined with its input. A case without an input, or with a different kind, is a broken package.
fn golden_cases() -> Vec<Case> {
    let golden = read_json("tests/golden/settings-field-map.json");
    let inputs = read_json("tests/golden-inputs/settings-field-map.json");
    let mut input_by_id: BTreeMap<String, Value> = BTreeMap::new();
    for input in inputs["cases"].as_array().expect("golden-inputs cases") {
        let id = input["id"].as_str().expect("input id").to_owned();
        assert!(
            input_by_id.insert(id.clone(), input.clone()).is_none(),
            "duplicate input id {id}"
        );
    }
    golden["cases"]
        .as_array()
        .expect("golden cases")
        .iter()
        .map(|case| {
            let id = case["id"].as_str().expect("golden id").to_owned();
            let input = input_by_id
                .get(&id)
                .unwrap_or_else(|| panic!("no input for golden case {id}"))
                .clone();
            let kind = case["kind"].as_str().expect("golden kind").to_owned();
            assert_eq!(input["kind"].as_str(), Some(kind.as_str()), "kind of case {id}");
            Case {
                id,
                kind,
                input,
                expected: case["result"].clone(),
            }
        })
        .collect()
}

/// The cases of one kind, with the count the contract fixes for it (617 cases in total).
fn cases_of(kind: &str, count: usize) -> Vec<Case> {
    let cases: Vec<Case> = golden_cases().into_iter().filter(|c| c.kind == kind).collect();
    assert_eq!(cases.len(), count, "golden cases of kind {kind}");
    cases
}

/// Runs every case of one kind and fails with the mismatches (first 20 printed).
fn run_kind(kind: &str, count: usize) {
    let cases = cases_of(kind, count);
    let mismatches: Vec<String> = cases
        .iter()
        .filter_map(|case| check_case(case).err().map(|e| format!("{}: {e}", case.id)))
        .collect();
    assert!(
        mismatches.is_empty(),
        "{} of {} {kind} cases differ from the C# golden:\n{}",
        mismatches.len(),
        cases.len(),
        mismatches.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// Runs the cases of one kind whose id satisfies `keep`.
fn run_selected(kind: &str, count: usize, keep: impl Fn(&str) -> bool) -> usize {
    let cases: Vec<Case> = cases_of(kind, count).into_iter().filter(|c| keep(&c.id)).collect();
    let mismatches: Vec<String> = cases
        .iter()
        .filter_map(|case| check_case(case).err().map(|e| format!("{}: {e}", case.id)))
        .collect();
    assert!(mismatches.is_empty(), "golden mismatches:\n{}", mismatches.join("\n"));
    cases.len()
}

fn check_case(case: &Case) -> Result<(), String> {
    match case.kind.as_str() {
        "tables" => check_tables(&case.expected),
        "load" => check_load(case),
        "apply_row" => check_apply_row(case),
        "apply_all" => check_apply_all(case),
        "shortcuts_read" => check_shortcuts_read(case),
        "shortcuts_canonicalize" => check_shortcuts_canonicalize(case),
        "anchor_to_index" => check_anchor_to_index(case),
        "index_to_anchor" => check_index_to_anchor(case),
        "view_mode_to_index" => check_view_mode_to_index(case),
        "index_to_view_mode" => check_index_to_view_mode(case),
        "try_int" => check_try_int(case),
        "try_double" => check_try_double(case),
        "double_text" => check_double_text(case),
        other => Err(format!("unknown golden kind {other}")),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Fixtures: settings and form built from golden inputs
// ---------------------------------------------------------------------------------------------------------------------

fn with_ctx<R>(run: impl FnOnce(&FieldContext<'_>) -> R) -> R {
    let localizer: std::sync::Arc<Localizer> = built_in_english_localizer();
    let numbers = NumberCulture::invariant();
    run(&FieldContext {
        texts: &localizer,
        numbers: &numbers,
    })
}

/// `new AppSettings()` with the golden's `settings` object applied in order. `ActionsText` is the action list JSON
/// text (parsed as the `Actions` property); every other key is a C# property name read by `apply_json_property`.
fn settings_from_input(input: &Value) -> AppSettings {
    let texts = EnglishSettingsTexts;
    let mut settings = AppSettings::new(&texts);
    let Some(entries) = input.get("settings").and_then(Value::as_object) else {
        return settings;
    };
    for (name, value) in entries {
        let (property, text) = if name == "ActionsText" {
            ("Actions", value.as_str().expect("ActionsText is a string").to_owned())
        } else {
            (name.as_str(), value.to_string())
        };
        let parsed = settings_json::parse(&text).unwrap_or_else(|e| panic!("settings {name}: {}", e.message));
        settings
            .apply_json_property(property, &parsed, &texts)
            .unwrap_or_else(|e| panic!("settings {name}: {}", e.message));
    }
    settings
}

/// The golden's `form` overrides, camelCase keys (`shortcutTexts` replaces the whole map).
fn apply_form_overrides(form: &mut SettingsFormState, input: &Value) {
    let Some(entries) = input.get("form").and_then(Value::as_object) else {
        return;
    };
    for (key, value) in entries {
        set_form_field(form, key, value);
    }
}

/// The form a Save starts from: every row loaded from `settings` (C# `new SettingsFormState()` plus `Load` of each
/// binding), then the golden overrides on top.
fn form_for_apply(input: &Value, settings: &AppSettings) -> SettingsFormState {
    let mut form = SettingsFormState::default();
    for row in map::bindings() {
        (row.load)(settings, &mut form);
    }
    apply_form_overrides(&mut form, input);
    form
}

fn member<T: LenientEnum>(name: &str) -> T {
    T::MEMBERS
        .iter()
        .copied()
        .find(|member| member.name() == name)
        .unwrap_or_else(|| panic!("no {} member {name}", T::TYPE_NAME))
}

fn set_form_field(form: &mut SettingsFormState, key: &str, value: &Value) {
    let int = |v: &Value| i32::try_from(v.as_i64().expect("integer form value")).expect("form index fits i32");
    let flag = |v: &Value| v.as_bool().expect("boolean form value");
    let text = |v: &Value| v.as_str().expect("string form value").to_owned();
    let float = |v: &Value| v.as_f64().expect("number form value");
    match key {
        "viewModeIndex" => form.view_mode_index = int(value),
        "fitWidthAnchorIndex" => form.fit_width_anchor_index = int(value),
        "fitWidthAnchor2Index" => form.fit_width_anchor2_index = int(value),
        "middleClickActionIndex" => form.middle_click_action_index = int(value),
        "keepZoomAcrossImages" => form.keep_zoom_across_images = flag(value),
        "loadingModeIndex" => form.loading_mode_index = int(value),
        "sortModeTag" => form.sort_mode_tag = value.as_str().map(str::to_owned),
        "scalingQualityIndex" => form.scaling_quality_index = int(value),
        "decoderBackendIndex" => form.decoder_backend_index = int(value),
        "instanceModeIndex" => form.instance_mode_index = int(value),
        "compareHashEnabled" => form.compare_hash_enabled = flag(value),
        "compareSizeEnabled" => form.compare_size_enabled = flag(value),
        "rawSupportEnabled" => form.raw_support_enabled = flag(value),
        "webpHeicSupportEnabled" => form.webp_heic_support_enabled = flag(value),
        "rawFullDecodeIndex" => form.raw_full_decode_index = int(value),
        "rawPairModeIndex" => form.raw_pair_mode_index = int(value),
        "loggingEnabled" => form.logging_enabled = flag(value),
        "allowPermanentDelete" => form.allow_permanent_delete = flag(value),
        "confirmBeforeDelete" => form.confirm_before_delete = flag(value),
        "externalEditorPath" => form.external_editor_path = text(value),
        "journalSafe" => form.journal_safe = flag(value),
        "moveCopyReuseLastFolder" => form.move_copy_reuse_last_folder = flag(value),
        "showInfoOverlay" => form.show_info_overlay = flag(value),
        "showFileInfo" => form.show_file_info = flag(value),
        "showFolderInfo" => form.show_folder_info = flag(value),
        "infoOverlayFontSizeText" => form.info_overlay_font_size_text = text(value),
        "showZoomIndicator" => form.show_zoom_indicator = flag(value),
        "toolbarAutoHide" => form.toolbar_auto_hide = flag(value),
        "toolbarAutoHideDelayText" => form.toolbar_auto_hide_delay_text = text(value),
        "infoOverlayAutoHide" => form.info_overlay_auto_hide = flag(value),
        "infoOverlayAutoHideDelayText" => form.info_overlay_auto_hide_delay_text = text(value),
        "toolbarOpacityValue" => form.toolbar_opacity_value = float(value),
        "showExifInfo" => form.show_exif_info = flag(value),
        "exifInfoFields" => form.exif_info_fields = ExifInfoFields::from_bits_truncate(int(value) as u32),
        "titleBarFields" => form.title_bar_fields = TitleBarFields::from_bits_truncate(int(value) as u32),
        "mouseWheelActionIndex" => form.mouse_wheel_action_index = int(value),
        "clickToZoomEnabled" => form.click_to_zoom_enabled = flag(value),
        "clickZoomPercentText" => form.click_zoom_percent_text = text(value),
        "kineticPanEnabled" => form.kinetic_pan_enabled = flag(value),
        "kineticGlideSmoothingIndex" => form.kinetic_glide_smoothing_index = int(value),
        "arrowKeyNavigatesAtZoomEdge" => form.arrow_key_navigates_at_zoom_edge = flag(value),
        "clickZoomKeyTogglesFit" => form.click_zoom_key_toggles_fit = flag(value),
        "arrowPanStepText" => form.arrow_pan_step_text = text(value),
        "keyboardZoomStepPercentText" => form.keyboard_zoom_step_percent_text = text(value),
        "keyboardZoomAnchorIndex" => form.keyboard_zoom_anchor_index = int(value),
        "imageTransitionIndex" => form.image_transition_index = int(value),
        "imageTransitionMsText" => form.image_transition_ms_text = text(value),
        "touchpadSwipeEnabled" => form.touchpad_swipe_enabled = flag(value),
        "touchpadSwipeDistanceText" => form.touchpad_swipe_distance_text = text(value),
        "preloadForwardText" => form.preload_forward_text = text(value),
        "preloadBackwardText" => form.preload_backward_text = text(value),
        "ramCachePercent" => form.ram_cache_percent = float(value),
        "actionsJson" => form.actions_json = text(value),
        "shortcutTexts" => {
            let entries = value.as_object().expect("shortcutTexts object");
            form.shortcut_texts = entries
                .iter()
                .map(|(name, typed)| (name.clone(), typed.as_str().unwrap_or_default().to_owned()))
                .collect();
        }
        "contextMenuShown" => form.context_menu_shown = shown_from_input(value),
        other => panic!("golden form key {other} has no SettingsFormState field"),
    }
}

/// The golden's `contextMenuShown`: a list of names, `ALL`, or `ALL_BUT:<Name>`.
fn shown_from_input(value: &Value) -> BTreeSet<ContextMenuItemId> {
    match value {
        Value::Array(names) => {
            let names: Vec<&str> = names.iter().map(|n| n.as_str().expect("item name")).collect();
            ContextMenuItemId::ALL
                .iter()
                .copied()
                .filter(|id| names.contains(&id.name()))
                .collect()
        }
        Value::String(marker) if marker == "ALL" => ContextMenuItemId::ALL.iter().copied().collect(),
        Value::String(marker) => {
            let except = marker
                .strip_prefix("ALL_BUT:")
                .unwrap_or_else(|| panic!("bad marker {marker}"));
            ContextMenuItemId::ALL
                .iter()
                .copied()
                .filter(|id| id.name() != except)
                .collect()
        }
        other => panic!("bad contextMenuShown {other}"),
    }
}

/// A shortcut map from a golden dictionary: absent boxes are empty (`MoveToFolder2` keeps the default when absent).
fn shortcuts_from_dict(dict: &Value) -> ShortcutMappings {
    let given = dict.as_object().cloned().unwrap_or_default();
    let mut full = Map::new();
    for name in ShortcutMappings::PROPERTY_NAMES {
        match given.get(name) {
            Some(value) => {
                full.insert(name.to_owned(), value.clone());
            }
            None if name != "MoveToFolder2" => {
                full.insert(name.to_owned(), Value::String(String::new()));
            }
            None => {}
        }
    }
    serde_json::from_value(Value::Object(full)).expect("shortcut dictionary")
}

// ---------------------------------------------------------------------------------------------------------------------
// Projections: the golden shapes of settings, form, refusals and shortcuts
// ---------------------------------------------------------------------------------------------------------------------

/// The 59 settings keys the golden lists. Everything comes from `AppSettings::to_json` (C# names) except `Actions`,
/// which is the action list text, and `EffectiveHiddenContextMenuItems`, the sorted effective hidden set.
fn settings_projection(settings: &AppSettings) -> Value {
    let text = settings.to_json().expect("settings serialize");
    let mut object: Map<String, Value> = match serde_json::from_str::<Value>(&text).expect("settings json") {
        Value::Object(object) => object,
        other => panic!("settings json is not an object: {other}"),
    };
    object.insert(
        "Actions".to_owned(),
        Value::String(action_list_json::serialize(&settings.actions)),
    );
    let mut hidden: Vec<String> = context_menu::effective_hidden(settings)
        .into_iter()
        .map(|id| id.name().to_owned())
        .collect();
    hidden.sort();
    object.insert("EffectiveHiddenContextMenuItems".to_owned(), serde_json::json!(hidden));
    Value::Object(object)
}

/// The form keys the golden lists (camelCase). Doubles are text in R round-trip form, like the golden.
fn form_projection(form: &SettingsFormState) -> Value {
    let mut out = Map::new();
    let mut put = |key: &str, value: Value| {
        out.insert(key.to_owned(), value);
    };
    put("viewModeIndex", form.view_mode_index.into());
    put("fitWidthAnchorIndex", form.fit_width_anchor_index.into());
    put("fitWidthAnchor2Index", form.fit_width_anchor2_index.into());
    put("middleClickActionIndex", form.middle_click_action_index.into());
    put("keepZoomAcrossImages", form.keep_zoom_across_images.into());
    put("loadingModeIndex", form.loading_mode_index.into());
    put(
        "sortModeTag",
        form.sort_mode_tag.clone().map_or(Value::Null, Value::from),
    );
    put("scalingQualityIndex", form.scaling_quality_index.into());
    put("decoderBackendIndex", form.decoder_backend_index.into());
    put("instanceModeIndex", form.instance_mode_index.into());
    put("compareHashEnabled", form.compare_hash_enabled.into());
    put("compareSizeEnabled", form.compare_size_enabled.into());
    put("rawSupportEnabled", form.raw_support_enabled.into());
    put("webpHeicSupportEnabled", form.webp_heic_support_enabled.into());
    put("rawFullDecodeIndex", form.raw_full_decode_index.into());
    put("rawPairModeIndex", form.raw_pair_mode_index.into());
    put("loggingEnabled", form.logging_enabled.into());
    put("allowPermanentDelete", form.allow_permanent_delete.into());
    put("confirmBeforeDelete", form.confirm_before_delete.into());
    put("externalEditorPath", form.external_editor_path.clone().into());
    put("journalSafe", form.journal_safe.into());
    put("moveCopyReuseLastFolder", form.move_copy_reuse_last_folder.into());
    put("showInfoOverlay", form.show_info_overlay.into());
    put("showFileInfo", form.show_file_info.into());
    put("showFolderInfo", form.show_folder_info.into());
    put(
        "infoOverlayFontSizeText",
        form.info_overlay_font_size_text.clone().into(),
    );
    put("showZoomIndicator", form.show_zoom_indicator.into());
    put("toolbarAutoHide", form.toolbar_auto_hide.into());
    put(
        "toolbarAutoHideDelayText",
        form.toolbar_auto_hide_delay_text.clone().into(),
    );
    put("infoOverlayAutoHide", form.info_overlay_auto_hide.into());
    put(
        "infoOverlayAutoHideDelayText",
        form.info_overlay_auto_hide_delay_text.clone().into(),
    );
    put("toolbarOpacityValue", form.toolbar_opacity_value.to_string().into());
    put("showExifInfo", form.show_exif_info.into());
    put("exifInfoFields", form.exif_info_fields.bits().into());
    put("titleBarFields", form.title_bar_fields.bits().into());
    put("mouseWheelActionIndex", form.mouse_wheel_action_index.into());
    put("clickToZoomEnabled", form.click_to_zoom_enabled.into());
    put("clickZoomPercentText", form.click_zoom_percent_text.clone().into());
    put("kineticPanEnabled", form.kinetic_pan_enabled.into());
    put("kineticGlideSmoothingIndex", form.kinetic_glide_smoothing_index.into());
    put(
        "arrowKeyNavigatesAtZoomEdge",
        form.arrow_key_navigates_at_zoom_edge.into(),
    );
    put("clickZoomKeyTogglesFit", form.click_zoom_key_toggles_fit.into());
    put("arrowPanStepText", form.arrow_pan_step_text.clone().into());
    put(
        "keyboardZoomStepPercentText",
        form.keyboard_zoom_step_percent_text.clone().into(),
    );
    put("keyboardZoomAnchorIndex", form.keyboard_zoom_anchor_index.into());
    put("imageTransitionIndex", form.image_transition_index.into());
    put("imageTransitionMsText", form.image_transition_ms_text.clone().into());
    put("touchpadSwipeEnabled", form.touchpad_swipe_enabled.into());
    put(
        "touchpadSwipeDistanceText",
        form.touchpad_swipe_distance_text.clone().into(),
    );
    put("preloadForwardText", form.preload_forward_text.clone().into());
    put("preloadBackwardText", form.preload_backward_text.clone().into());
    put("ramCachePercent", form.ram_cache_percent.to_string().into());
    put("actionsJson", form.actions_json.clone().into());
    put(
        "shortcutTexts",
        serde_json::to_value(&form.shortcut_texts).expect("shortcut texts"),
    );
    // Sorted by C# name (ordinal), the golden order; the set itself is ordered by enum value.
    let mut shown: Vec<&str> = form.context_menu_shown.iter().map(|id| id.name()).collect();
    shown.sort_unstable();
    put("contextMenuShown", serde_json::json!(shown));
    Value::Object(out)
}

/// `refused` of an apply: `null`, or the message and the focus (C# `SettingsFocusField` name).
fn refusal_projection(refused: Option<&SettingsFieldError>) -> Value {
    match refused {
        None => Value::Null,
        Some(error) => serde_json::json!({ "message": error.message, "focus": format!("{:?}", error.focus) }),
    }
}

fn shortcuts_projection(shortcuts: &ShortcutMappings) -> Value {
    Value::Object(
        shortcuts
            .entries()
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.into()))
            .collect(),
    )
}

/// Golden comparison: every key of `expected` must be present in `actual` and equal. Numbers compare by value (the
/// golden writes `12.0` and `12` alike). Extra keys in `actual` are not a difference.
fn diff_json(expected: &Value, actual: &Value) -> Result<(), String> {
    let mut differences = Vec::new();
    collect_diff("", expected, actual, &mut differences);
    if differences.is_empty() {
        Ok(())
    } else {
        Err(differences.join("; "))
    }
}

fn collect_diff(path: &str, expected: &Value, actual: &Value, out: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Object(want), Value::Object(got)) => {
            for (key, value) in want {
                match got.get(key) {
                    Some(found) => collect_diff(&format!("{path}.{key}"), value, found, out),
                    None => out.push(format!("{path}.{key} missing")),
                }
            }
        }
        (Value::Array(want), Value::Array(got)) => {
            if want.len() != got.len() {
                out.push(format!("{path} has {} items, want {}", got.len(), want.len()));
            }
            for (i, (w, g)) in want.iter().zip(got.iter()).enumerate() {
                collect_diff(&format!("{path}[{i}]"), w, g, out);
            }
        }
        (Value::Number(want), Value::Number(got)) => {
            if want.as_f64() != got.as_f64() {
                out.push(format!("{path}: {got} want {want}"));
            }
        }
        (want, got) => {
            if want != got {
                out.push(format!("{path}: {got} want {want}"));
            }
        }
    }
}

/// Golden doubles are text: `"NaN"`, `"Infinity"`, `"-Infinity"` or a decimal number.
fn golden_f64(text: &str) -> f64 {
    match text {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        other => other.parse().unwrap_or_else(|e| panic!("golden double {other}: {e}")),
    }
}

fn same_f64(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a == b
}

// ---------------------------------------------------------------------------------------------------------------------
// Per-kind golden checks
// ---------------------------------------------------------------------------------------------------------------------

fn check_tables(expected: &Value) -> Result<(), String> {
    let covered: Vec<String> = map::covered_properties().into_iter().map(str::to_owned).collect();
    let covered_want: Vec<String> = string_list(&expected["covered_sorted"]);
    if covered != covered_want {
        return Err(format!("covered_properties {covered:?} want {covered_want:?}"));
    }
    let not_edited: Vec<Vec<String>> = map::not_edited_in_dialog()
        .iter()
        .map(|(name, reason)| vec![(*name).to_owned(), (*reason).to_owned()])
        .collect();
    let not_edited_want: Vec<Vec<String>> = expected["not_edited_in_order"]
        .as_array()
        .expect("not_edited_in_order")
        .iter()
        .map(string_list)
        .collect();
    if not_edited != not_edited_want {
        return Err("not_edited_in_dialog differs from the C# insertion order".to_owned());
    }
    let rows: Vec<Vec<String>> = map::bindings()
        .iter()
        .map(|row| row.properties.iter().map(|p| (*p).to_owned()).collect())
        .collect();
    let rows_want: Vec<Vec<String>> = expected["bindings_in_order"]
        .as_array()
        .expect("bindings_in_order")
        .iter()
        .map(string_list)
        .collect();
    if rows != rows_want {
        return Err("bindings() property lists differ from the Save order".to_owned());
    }
    let shortcuts: Vec<String> = map::shortcut_properties().iter().map(|p| (*p).to_owned()).collect();
    let shortcuts_want = string_list(&expected["shortcut_names_in_order"]);
    if shortcuts != shortcuts_want {
        return Err("shortcut_properties() differs from the Shortcuts page order".to_owned());
    }
    // settable = covered union not-edited names, each once (the golden lists 70 names, sorted).
    let mut union: Vec<String> = covered.clone();
    union.extend(not_edited_want.iter().map(|pair| pair[0].clone()));
    union.sort();
    let settable_want = string_list(&expected["settable"]);
    if union != settable_want {
        return Err(format!(
            "covered + not-edited {} names differ from settable {}",
            union.len(),
            settable_want.len()
        ));
    }
    Ok(())
}

fn check_load(case: &Case) -> Result<(), String> {
    let settings = settings_from_input(&case.input);
    let mut form = SettingsFormState::default();
    for row in map::bindings() {
        (row.load)(&settings, &mut form);
    }
    diff_json(&case.expected, &form_projection(&form))
}

fn check_apply_row(case: &Case) -> Result<(), String> {
    let row = case.input["row"].as_u64().expect("row index") as usize;
    let settings = settings_from_input(&case.input);
    let form = form_for_apply(&case.input, &settings);
    let binding = map::bindings()[row];
    let mut out = settings.deep_clone();
    let refused = with_ctx(|ctx| (binding.apply)(&form, &mut out, ctx));
    let actual = serde_json::json!({
        "refused": refusal_projection(refused.as_ref()),
        "settings": settings_projection(&out),
    });
    diff_json(&case.expected, &actual)
}

fn check_apply_all(case: &Case) -> Result<(), String> {
    let settings = settings_from_input(&case.input);
    let form = form_for_apply(&case.input, &settings);
    let mut out = settings.deep_clone();
    let mut refused_row = None;
    let mut refused = None;
    with_ctx(|ctx| {
        for (index, row) in map::bindings().iter().enumerate() {
            if let Some(error) = (row.apply)(&form, &mut out, ctx) {
                refused_row = Some(index);
                refused = Some(error);
                break;
            }
        }
    });
    let actual = serde_json::json!({
        "refused_row": refused_row.map_or(Value::Null, Value::from),
        "refused": refusal_projection(refused.as_ref()),
        "settings": settings_projection(&out),
    });
    diff_json(&case.expected, &actual)
}

fn check_shortcuts_read(case: &Case) -> Result<(), String> {
    let mut form = SettingsFormState::default();
    apply_form_overrides(&mut form, &case.input);
    diff_json(&case.expected, &shortcuts_projection(&map::read_shortcuts(&form)))
}

fn check_shortcuts_canonicalize(case: &Case) -> Result<(), String> {
    let raw = shortcuts_from_dict(&case.input["raw"]);
    let existing = shortcuts_from_dict(&case.input["existing"]);
    diff_json(
        &case.expected,
        &shortcuts_projection(&map::canonicalize_shortcuts(&raw, &existing)),
    )
}

fn check_anchor_to_index(case: &Case) -> Result<(), String> {
    let anchor: FitWidthAnchor = member(case.input["anchor"].as_str().expect("anchor"));
    diff_json(
        &case.expected,
        &serde_json::json!({ "index": map::anchor_to_index(anchor) }),
    )
}

fn check_index_to_anchor(case: &Case) -> Result<(), String> {
    let index = case.input["index"].as_i64().expect("index") as i32;
    let fallback: FitWidthAnchor = member(case.input["fallback"].as_str().expect("fallback"));
    let got = map::index_to_anchor(index, fallback);
    diff_json(&case.expected, &serde_json::json!({ "anchor": got.name() }))
}

fn check_view_mode_to_index(case: &Case) -> Result<(), String> {
    let mode: InitialViewMode = member(case.input["mode"].as_str().expect("mode"));
    diff_json(
        &case.expected,
        &serde_json::json!({ "index": map::view_mode_to_index(mode) }),
    )
}

fn check_index_to_view_mode(case: &Case) -> Result<(), String> {
    let index = case.input["index"].as_i64().expect("index") as i32;
    let mode = map::index_to_view_mode(index);
    diff_json(&case.expected, &serde_json::json!({ "mode": mode.name() }))
}

fn check_try_int(case: &Case) -> Result<(), String> {
    let text = case.input["text"].as_str().expect("text");
    let min = case.input["min"].as_i64().expect("min") as i32;
    let max = case.input["max"].as_i64().expect("max") as i32;
    let got = map::try_int(text, min, max);
    let want_ok = case.expected["ok"].as_bool().expect("ok");
    match (want_ok, got) {
        (false, None) => Ok(()),
        (true, Some(value)) if Some(value) == case.expected["value"].as_i64().map(|v| v as i32) => Ok(()),
        (_, got) => Err(format!(
            "try_int({text:?}, {min}, {max}) = {got:?}, golden {}",
            case.expected
        )),
    }
}

fn check_try_double(case: &Case) -> Result<(), String> {
    let text = case.input["text"].as_str().expect("text");
    let want_ok = case.expected["ok"].as_bool().expect("ok");
    match (want_ok, map::try_double(text)) {
        (false, None) => Ok(()),
        (true, Some(value)) => {
            let want = golden_f64(case.expected["value"].as_str().expect("value text"));
            let want_finite = case.expected["finite"].as_bool().expect("finite");
            if !same_f64(value, want) {
                return Err(format!("try_double({text:?}) = {value}, golden {want}"));
            }
            if value.is_finite() != want_finite {
                return Err(format!(
                    "try_double({text:?}) finite = {}, golden {want_finite}",
                    value.is_finite()
                ));
            }
            Ok(())
        }
        (_, got) => Err(format!("try_double({text:?}) = {got:?}, golden {}", case.expected)),
    }
}

fn check_double_text(case: &Case) -> Result<(), String> {
    let value = golden_f64(case.input["value"].as_str().expect("value text"));
    let got = map::dotnet_double_text(value);
    let want = case.expected["text"].as_str().expect("text");
    if got == want {
        Ok(())
    } else {
        Err(format!("dotnet_double_text({value:?}) = {got:?}, golden {want:?}"))
    }
}

fn string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|v| v.as_str().expect("string").to_owned())
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden-driven tests (one per golden kind, plus the contract's named ones)
// ---------------------------------------------------------------------------------------------------------------------

// C#: SettingsFieldMapTests.EveryProperty_IsMappedOrExplicitlyNotEditable (line 22). golden `tables.settable`.
#[test]
fn every_settable_property_is_mapped_or_excluded() {
    run_kind("tables", 1);
}

// C#: SettingsFieldMapTests.Lists_NameOnlyRealPropertiesAndDoNotOverlap (line 31). golden `tables`, non-blank reasons.
#[test]
fn covered_and_not_edited_name_real_properties_disjointly() {
    let covered = map::covered_properties();
    let not_edited = map::not_edited_in_dialog();
    let names: BTreeSet<&str> = not_edited.iter().map(|(name, _)| *name).collect();
    assert_eq!(names.len(), not_edited.len(), "a not-edited name is listed twice");
    assert!(covered.is_disjoint(&names), "covered and not-edited names overlap");
    assert!(
        not_edited.iter().all(|(_, reason)| !reason.trim().is_empty()),
        "a not-edited reason is blank"
    );
    let real: BTreeSet<&str> = pr_core::settings::app_settings::JSON_KEYS.iter().copied().collect();
    let all: BTreeSet<&str> = covered.iter().copied().chain(names.iter().copied()).collect();
    assert_eq!(
        all, real,
        "covered and not-edited names must be the settable AppSettings properties"
    );
    run_kind("tables", 1);
}

// C#: SettingsFieldMapTests.Bindings_OwnDistinctProperties (line 41). Pure: 47 rows, 58 names, no name owned twice.
#[test]
fn bindings_are_nonempty_and_own_distinct_properties() {
    let rows = map::bindings();
    assert_eq!(rows.len(), 47, "row count");
    let mut owned: BTreeSet<&str> = BTreeSet::new();
    let mut total = 0;
    for row in rows {
        assert!(!row.properties.is_empty(), "a row owns no property");
        for property in row.properties {
            total += 1;
            assert!(owned.insert(property), "{property} is owned by two rows");
        }
    }
    assert_eq!(total, 58, "rows own 58 names; UiLanguage has no row");
    let mut covered = map::covered_properties();
    assert!(covered.remove("UiLanguage"), "UiLanguage is covered without a row");
    assert_eq!(covered, owned);
}

// C#: SettingsFieldMapTests.ShortcutAccessors_CoverEveryShortcutProperty (line 50).
// The 27 boxes are every ShortcutMappings name except MoveToFolder2 (owned by ReviewAction), in the page order.
#[test]
fn shortcut_properties_are_every_box_except_move_to_folder2() {
    let boxes = map::shortcut_properties();
    let want: Vec<&str> = ShortcutMappings::PROPERTY_NAMES
        .iter()
        .copied()
        .filter(|name| *name != "MoveToFolder2")
        .collect();
    assert_eq!(boxes.len(), 27, "box count");
    assert_eq!(boxes, want.as_slice());
    let unique: BTreeSet<&str> = boxes.iter().copied().collect();
    assert_eq!(unique.len(), boxes.len(), "a shortcut name is listed twice");
}

// C#: SettingsFieldMapTests.ShortcutAccessors_ReadAndWriteTheirOwnProperty (line 61).
#[test]
fn shortcut_get_and_set_touch_only_their_property() {
    for name in map::shortcut_properties() {
        let mut shortcuts = ShortcutMappings::default();
        let before: BTreeMap<String, String> = shortcuts
            .entries()
            .into_iter()
            .map(|(n, v)| (n.to_owned(), v.to_owned()))
            .collect();
        map::shortcut_set(&mut shortcuts, name, "KEY-X".to_owned());
        assert_eq!(map::shortcut_get(&shortcuts, name), "KEY-X", "{name} reads back");
        for (other, value) in shortcuts.entries() {
            if other != *name {
                assert_eq!(
                    Some(value),
                    before.get(other).map(String::as_str),
                    "{name} changed {other}"
                );
            }
        }
    }
}

// C#: SettingsFieldMapTests.Shortcuts_RoundTripEachProperty (line 75). Pure half; the presenter half is W8-10d.
#[test]
fn shortcut_round_trip_load_read() {
    let mut source = ShortcutMappings::default();
    for (index, name) in map::shortcut_properties().iter().enumerate() {
        map::shortcut_set(&mut source, name, format!("K{index}"));
    }
    let mut form = SettingsFormState::default();
    map::load_shortcuts(&source, &mut form);
    for name in map::shortcut_properties() {
        assert_eq!(
            map::shortcut_text(&form, name),
            map::shortcut_get(&source, name),
            "{name} loads as typed"
        );
    }
    assert_eq!(
        map::read_shortcuts(&form),
        source,
        "read after load gives the source back"
    );
}

// C#: SettingsFieldMapTests.MappedProperties_RoundTripThroughLoadAndSave (line 142). Pure half: each row's marker.
#[test]
fn every_row_roundtrips_its_marker_through_load_and_apply() {
    let ran = run_selected("apply_row", 243, |id| {
        id.starts_with("apply-row-") && id.ends_with("roundtrip-marker")
    });
    assert_eq!(ran, 47, "one marker case per row");
}

// C#: SettingsFieldMapTests.ContextMenu_RoundTripsHiddenSet_AndKeepsLockedItemsShown (line 176). Pure half; presenter half is W8-10d.
#[test]
fn context_menu_rows_roundtrip_hidden_set_and_keep_locked_shown() {
    let applied = run_selected("apply_row", 243, |id| id.starts_with("apply-context-"));
    let loaded = run_selected("load", 117, |id| id.starts_with("load-context-"));
    assert!(applied > 0 && loaded > 0, "context-menu golden cases are missing");
}

// C#: SettingsFieldMapTests.FieldFlags_RoundTrip (line 202). Rows 35 (ExifInfoFields) and 36 (TitleBarFields).
#[test]
fn flags_roundtrip_through_load_and_apply_rows() {
    let rows = map::bindings();
    let exif_row = rows[35];
    let title_row = rows[36];
    assert_eq!(exif_row.properties, ["ExifInfoFields"]);
    assert_eq!(title_row.properties, ["TitleBarFields"]);
    let exif_samples: [u32; 11] = [0, 1, 2, 4, 8, 16, 32, 64, 128, 256, 506];
    let title_samples: [u32; 6] = [0, 1, 2, 4, 1024, TitleBarFields::ALL.bits()];
    with_ctx(|ctx| {
        let settings_texts = EnglishSettingsTexts;
        for bits in exif_samples {
            let mut source = AppSettings::new(&settings_texts);
            source.exif_info_fields = ExifInfoFields::from_bits_truncate(bits);
            let mut form = SettingsFormState::default();
            (exif_row.load)(&source, &mut form);
            let mut out = source.deep_clone();
            assert_eq!((exif_row.apply)(&form, &mut out, ctx), None, "exif {bits} refused");
            assert_eq!(out.exif_info_fields, source.exif_info_fields, "exif {bits} round trip");
        }
        for bits in title_samples {
            let mut source = AppSettings::new(&settings_texts);
            source.title_bar_fields = TitleBarFields::from_bits_truncate(bits);
            let mut form = SettingsFormState::default();
            (title_row.load)(&source, &mut form);
            let mut out = source.deep_clone();
            assert_eq!((title_row.apply)(&form, &mut out, ctx), None, "title {bits} refused");
            assert_eq!(out.title_bar_fields, source.title_bar_fields, "title {bits} round trip");
        }
    });
}

// new: semantics 1 (T7): the pass stops at the first refusal and the rows before it stay written in the clone.
#[test]
fn apply_all_stops_at_first_refusal_and_keeps_earlier_rows() {
    let ran = run_selected("apply_all", 6, |id| id.contains("refused"));
    assert!(ran >= 4, "refused apply_all golden cases are missing");
    run_kind("apply_all", 6);
}

#[test]
fn golden_settings_field_map_tables() {
    run_kind("tables", 1);
}

#[test]
fn golden_settings_field_map_load() {
    run_kind("load", 117);
}

#[test]
fn golden_settings_field_map_apply_row() {
    run_kind("apply_row", 243);
}

#[test]
fn golden_settings_field_map_apply_all() {
    run_kind("apply_all", 6);
}

#[test]
fn golden_settings_field_map_shortcuts_read() {
    run_kind("shortcuts_read", 3);
}

#[test]
fn golden_settings_field_map_shortcuts_canonicalize() {
    run_kind("shortcuts_canonicalize", 2);
}

#[test]
fn golden_settings_field_map_anchor_to_index() {
    run_kind("anchor_to_index", 3);
}

#[test]
fn golden_settings_field_map_index_to_anchor() {
    run_kind("index_to_anchor", 14);
}

#[test]
fn golden_settings_field_map_view_mode_to_index() {
    run_kind("view_mode_to_index", 7);
}

#[test]
fn golden_settings_field_map_index_to_view_mode() {
    run_kind("index_to_view_mode", 9);
}

#[test]
fn golden_settings_field_map_try_int() {
    run_kind("try_int", 165);
}

#[test]
fn golden_settings_field_map_try_double() {
    run_kind("try_double", 31);
}

#[test]
fn golden_settings_field_map_double_text() {
    run_kind("double_text", 16);
}

// new: T3 `int.TryParse(NumberStyles.Integer)`: .NET whitespace (U+0009..U+000D, U+0020) only, one sign, digits.
#[test]
fn try_int_accepts_dotnet_integer_text() {
    assert_eq!(map::try_int("12", 10, 800), Some(12));
    assert_eq!(map::try_int(" 12\t", 10, 800), Some(12));
    assert_eq!(map::try_int("+5", 0, 10), Some(5));
    assert_eq!(map::try_int("-5", -10, 10), Some(-5));
    assert_eq!(map::try_int("800", 10, 800), Some(800), "upper bound is inclusive");
}

// new: T3 and T21: no decimals, no exponent, no thousands separator, no NBSP or ZWSP, no i32 overflow.
#[test]
fn try_int_refuses_non_integer_text_and_overflow() {
    for text in [
        "12.0",
        "1e2",
        "1,000",
        "\u{00A0}12",
        "\u{200B}12",
        "",
        " ",
        "9",
        "801",
        "2147483648",
    ] {
        assert_eq!(map::try_int(text, 10, 800), None, "{text:?} must be refused");
    }
    assert_eq!(
        map::try_int("2147483648", i32::MIN, i32::MAX),
        None,
        "i32 overflow is refused"
    );
}

// new: T4 `double.TryParse(NumberStyles.Float, Invariant)`: a trailing or leading point, exponent, NaN, Infinity.
#[test]
fn try_double_accepts_dotnet_float_forms() {
    assert_eq!(map::try_double("8."), Some(8.0));
    assert_eq!(map::try_double(".5"), Some(0.5));
    assert_eq!(map::try_double(" 12 "), Some(12.0));
    assert_eq!(map::try_double("1e1"), Some(10.0));
    assert!(
        map::try_double("NaN").is_some_and(f64::is_nan),
        "NaN parses, the caller refuses it"
    );
    assert_eq!(map::try_double("Infinity"), Some(f64::INFINITY));
    assert_eq!(
        map::try_double("1e400"),
        Some(f64::INFINITY),
        "overflow parses to infinity"
    );
}

// new: T4: invariant culture only; a comma decimal and non-numeric text are refused.
#[test]
fn try_double_refuses_comma_decimal_and_words() {
    for text in ["12,5", "abc", "", "\u{00A0}12", "1 2"] {
        assert_eq!(map::try_double(text), None, "{text:?} must be refused");
    }
}

// new: T13: `double.ToString(Invariant)` writes whole numbers without a point and keeps the fraction.
#[test]
fn dotnet_double_text_whole_numbers_have_no_point_and_fractions_are_kept() {
    assert_eq!(map::dotnet_double_text(12.0), "12");
    assert_eq!(map::dotnet_double_text(12.5), "12.5");
}

// The exponent thresholds of `dotnet_double_text` are golden-checked by `golden_settings_field_map_double_text`.
#[test]
fn golden_double_text_thresholds_are_checked() {
    run_kind("double_text", 16);
}
