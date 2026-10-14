//! Port of the `PhotoReview.Core/Settings` normalizer, validator and repair-text tests (baseline d92d6aae).
//! Goldens: `tests/golden/settings-normalize.json` (294), `settings-validate.json` (84), `settings-repair-text.json` (22).

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use pr_core::abstractions::KeyNameValidator;
use pr_core::model::enums::{FileOperationType, JournalDurability, KineticGlideSmoothing, RawFullDecode, RawPairMode};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts, SettingsJsonError, SettingsTexts};
use pr_core::settings::normalizer::{
    SHORTCUT_REPAIR_PREFIX, SettingsParseResult, SettingsValidator, build_repair_text,
    disable_conflicting_optional_shortcuts, normalize,
};
use pr_core::settings::perf_options as perf;
use pr_core::settings::shortcuts::{ReviewAction, ShortcutMappings, canonicalize};
use pr_test_support::golden::{self, bool_of, str_of};
use serde_json::Value;

const EN: EnglishSettingsTexts = EnglishSettingsTexts;

fn read(text: &str) -> AppSettings {
    AppSettings::from_json(text, &EN)
        .unwrap_or_else(|e| panic!("read failed: {e}"))
        .unwrap_or_else(|| panic!("json null"))
}

fn defaults() -> AppSettings {
    AppSettings::new(&EN)
}

fn mandatory_names() -> Vec<&'static str> {
    ShortcutMappings::PROPERTY_NAMES
        .iter()
        .copied()
        .filter(|n| !ShortcutMappings::is_optional(n) && *n != "MoveToFolder2")
        .collect()
}

fn set(s: &mut AppSettings, name: &str, value: &str) {
    *s.shortcuts.get_mut(name).expect("property") = value.to_owned();
}

fn get<'a>(s: &'a AppSettings, name: &str) -> &'a str {
    s.shortcuts.get(name).expect("property")
}

fn action(name: &str, shortcut: &str) -> ReviewAction {
    let mut a = ReviewAction::new(name);
    a.shortcut = shortcut.to_owned();
    a
}

// ---- validators used by the goldens and tests ----

struct NonBlank;
impl KeyNameValidator for NonBlank {
    fn is_valid_key_name(&self, key_name: &str) -> bool {
        !key_name.trim().is_empty()
    }
}

struct LettersDigits;
impl KeyNameValidator for LettersDigits {
    fn is_valid_key_name(&self, key_name: &str) -> bool {
        !key_name.is_empty() && key_name.chars().all(|c| c.is_ascii_alphanumeric())
    }
}

struct RejectAll;
impl KeyNameValidator for RejectAll {
    fn is_valid_key_name(&self, _key_name: &str) -> bool {
        false
    }
}

struct Known(HashSet<String>);
impl Known {
    fn new<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        Self(names.into_iter().map(pr_core::text::ignore_case_key).collect())
    }
}
impl KeyNameValidator for Known {
    fn is_valid_key_name(&self, key_name: &str) -> bool {
        let t = key_name.trim();
        !t.is_empty() && self.0.contains(&pr_core::text::ignore_case_key(t))
    }
}

const DEFAULT_KNOWN: [&str; 32] = [
    "Right", "Left", "Enter", "Delete", "C", "PageDown", "PageUp", "Home", "Add", "Subtract", "F", "Space", "Z", "F11",
    "F3", "F4", "F5", "F6", "T", "D1", "D2", "D3", "D4", "M", "Y", "End", "I", "W", "H", "K", "O", "R",
];

fn validator(v: Arc<dyn KeyNameValidator>) -> SettingsValidator {
    SettingsValidator::new(v, Arc::new(EN))
}

fn known_validator() -> SettingsValidator {
    validator(Arc::new(Known::new(DEFAULT_KNOWN)))
}

fn make_validator(spec: &Value) -> Arc<dyn KeyNameValidator> {
    match str_of(spec, "kind") {
        "non_blank" => Arc::new(NonBlank),
        "letters_digits" => Arc::new(LettersDigits),
        "reject_all" => Arc::new(RejectAll),
        "known" => {
            let names: Vec<&str> = spec["names"]
                .as_array()
                .expect("names")
                .iter()
                .map(|n| n.as_str().expect("name"))
                .collect();
            Arc::new(Known::new(names))
        }
        other => panic!("validator kind {other}"),
    }
}

// ---- golden: settings-normalize ----

/// Splits a written document into `(property name, segment)` pairs the way the oracle's `W202Segments` does.
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
    let default = segments(&defaults().to_json().expect("default writes"));
    let now = segments(&settings.to_json()?);
    let mut map = serde_json::Map::new();
    for (name, seg) in now {
        if default.get(&name) != Some(&seg) {
            map.insert(name, Value::String(seg));
        }
    }
    Ok(Value::Object(map))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|s| s.as_str().expect("string").to_owned())
        .collect()
}

#[test]
fn golden_settings_normalize() {
    let g = golden::load("settings-normalize");
    assert!(g.csharp_commit.starts_with("d92d6aae"), "baseline {}", g.csharp_commit);
    assert_eq!(g.cases.len(), 294);
    for case in &g.cases {
        let name = str_of(case, "name");
        let text = str_of(case, "text");
        let mut s = read(text);
        let fixed = normalize(&mut s);
        let disabled = disable_conflicting_optional_shortcuts(&mut s);
        let fixed_again = normalize(&mut s);
        let disabled_again = disable_conflicting_optional_shortcuts(&mut s);
        assert_eq!(fixed, strings(&case["fixed"]), "{name}: fixed");
        assert_eq!(disabled, strings(&case["disabled"]), "{name}: disabled");
        assert_eq!(fixed_again, strings(&case["fixed_again"]), "{name}: fixed_again");
        assert_eq!(
            disabled_again,
            strings(&case["disabled_again"]),
            "{name}: disabled_again"
        );
        let changed = changed_from_default(&s).unwrap_or_else(|e| panic!("{name}: write failed {e}"));
        assert_eq!(changed, case["changed"], "{name}: changed");
    }
}

// ---- golden: settings-validate ----

#[test]
fn golden_settings_validate() {
    let g = golden::load("settings-validate");
    assert!(g.csharp_commit.starts_with("d92d6aae"), "baseline {}", g.csharp_commit);
    assert_eq!(g.cases.len(), 84);
    for case in &g.cases {
        let name = str_of(case, "name");
        let s = read(str_of(case, "text"));
        let v = validator(make_validator(&case["validator"]));
        let expected = case["message"].as_str().map(str::to_owned);
        assert_eq!(v.validate_shortcuts(&s), expected, "{name}");
    }
}

// ---- golden: settings-repair-text ----

#[test]
fn golden_settings_repair_text() {
    let g = golden::load("settings-repair-text");
    assert!(g.csharp_commit.starts_with("d92d6aae"), "baseline {}", g.csharp_commit);
    assert_eq!(g.cases.len(), 22);
    for case in &g.cases {
        let name = str_of(case, "name");
        let repairs = strings(&case["repairs"]);
        let text = build_repair_text(&repairs, bool_of(case, "for_import"), &EN);
        assert_eq!(text, str_of(case, "text"), "{name}");
    }
}

// ---- SettingsNormalizerMutationGapTests ----

#[test]
fn normalize_default_settings_reports_nothing() {
    // C#: SettingsNormalizerMutationGapTests.Normalize_DefaultSettings_ReportsNothing
    assert!(normalize(&mut defaults()).is_empty());
}

#[test]
fn normalize_source_bytes_capacity_must_be_positive() {
    // C#: SettingsNormalizerMutationGapTests.Normalize_SourceBytesCapacity_MustBePositive
    for (value, reset) in [(0_i64, true), (-1, true), (1, false)] {
        let mut s = defaults();
        s.source_bytes_capacity_bytes = value;
        let repaired = normalize(&mut s);
        assert_eq!(
            repaired.iter().any(|n| n == "SourceBytesCapacityBytes"),
            reset,
            "{value}"
        );
        let expected = if reset {
            perf::SOURCE_BYTES_CAPACITY_BYTES
        } else {
            value
        };
        assert_eq!(s.source_bytes_capacity_bytes, expected, "{value}");
    }
}

#[test]
fn normalize_memory_reserve_may_be_zero_but_not_negative() {
    // C#: SettingsNormalizerMutationGapTests.Normalize_MemoryReserve_MayBeZeroButNotNegative
    for (value, reset) in [(-1_i64, true), (0, false), (1, false)] {
        let mut s = defaults();
        s.memory_reserve_bytes = value;
        let repaired = normalize(&mut s);
        assert_eq!(repaired.iter().any(|n| n == "MemoryReserveBytes"), reset, "{value}");
        let expected = if reset { perf::MEMORY_RESERVE_BYTES } else { value };
        assert_eq!(s.memory_reserve_bytes, expected, "{value}");
    }
}

#[test]
fn normalize_preload_memory_load_limit_must_be_in_zero_exclusive_to_one_inclusive() {
    // C#: SettingsNormalizerMutationGapTests.Normalize_PreloadMemoryLoadLimit_MustBeInZeroExclusiveToOneInclusive
    let cases = [
        (0.0, true),
        (-0.5, true),
        (0.01, false),
        (1.0, false),
        (1.0001, true),
        (f64::NAN, true),
    ];
    for (value, reset) in cases {
        let mut s = defaults();
        s.preload_memory_load_limit = value;
        let repaired = normalize(&mut s);
        assert_eq!(repaired.iter().any(|n| n == "PreloadMemoryLoadLimit"), reset, "{value}");
        let expected = if reset { perf::PRELOAD_MEMORY_LOAD_LIMIT } else { value };
        assert_eq!(s.preload_memory_load_limit.to_bits(), expected.to_bits(), "{value}");
    }
}

#[test]
fn normalize_undefined_journal_durability_is_reset_to_fast() {
    // C#: SettingsNormalizerMutationGapTests.Normalize_UndefinedJournalDurability_IsResetToFast
    // Rust cannot hold (JournalDurability)99: the lenient reader maps it to value 0 and nothing is reported.
    let mut s = read("{\"JournalDurability\": 99}");
    let repaired = normalize(&mut s);
    assert!(!repaired.iter().any(|n| n == "JournalDurability"));
    assert_eq!(s.journal_durability, JournalDurability::Fast);
}

#[test]
fn normalize_undefined_raw_full_decode_is_reset_to_its_default() {
    // C#: SettingsNormalizerMutationGapTests.Normalize_UndefinedRawFullDecode_IsResetToItsDefault
    let mut s = read("{\"RawFullDecode\": 99}");
    let repaired = normalize(&mut s);
    assert!(!repaired.iter().any(|n| n == "RawFullDecode"));
    assert_eq!(s.raw_full_decode, defaults().raw_full_decode);
    assert_eq!(s.raw_full_decode, RawFullDecode::Never);
}

#[test]
fn normalize_undefined_raw_pair_mode_is_reset_to_its_default() {
    // C#: SettingsNormalizerMutationGapTests.Normalize_UndefinedRawPairMode_IsResetToItsDefault
    let mut s = read("{\"RawPairMode\": 99}");
    let repaired = normalize(&mut s);
    assert!(!repaired.iter().any(|n| n == "RawPairMode"));
    assert_eq!(s.raw_pair_mode, defaults().raw_pair_mode);
    assert_eq!(s.raw_pair_mode, RawPairMode::Separate);
}

#[test]
fn normalize_undefined_option_enums_are_reset_and_reported() {
    // C#: SettingsNormalizerMutationGapTests (undefined enum facts, `Option<..>` sentinel fields)
    let mut s = defaults();
    s.loading_mode = None;
    s.image_sort_mode = None;
    s.scaling_quality = None;
    s.decoder_backend = None;
    s.keyboard_zoom_anchor = None;
    s.fit_width_anchor2 = None;
    s.middle_click_action = None;
    s.kinetic_glide_smoothing = None;
    let repaired = normalize(&mut s);
    assert_eq!(
        repaired,
        [
            "LoadingMode",
            "ImageSortMode",
            "ScalingQuality",
            "DecoderBackend",
            "KeyboardZoomAnchor",
            "FitWidthAnchor2",
            "MiddleClickAction",
            "KineticGlideSmoothing",
        ]
    );
    let d = defaults();
    assert_eq!(s.loading_mode, d.loading_mode);
    assert_eq!(s.decoder_backend, d.decoder_backend);
    assert_eq!(s.kinetic_glide_smoothing, Some(KineticGlideSmoothing::Predict));
}

#[test]
fn normalize_action_with_undefined_operation_becomes_move_and_is_reported() {
    // C#: SettingsNormalizerMutationGapTests.Normalize_ActionWithUndefinedOperation_BecomesMoveAndIsReported
    // Rust cannot hold (FileOperationType)99: the reader maps an odd operation to Move silently (golden
    // `actions: odd operations are silently Move`).
    let mut s =
        read("{\"Actions\":[{\"Name\":\"A\",\"Shortcut\":\"J\",\"Destination\":\"C:\\\\x\",\"Operation\":99}]}");
    let repaired = normalize(&mut s);
    assert!(!repaired.iter().any(|n| n == "Actions"));
    assert_eq!(s.actions.len(), 1);
    assert_eq!(s.actions[0].operation, FileOperationType::Move);
}

#[test]
fn normalize_dropped_null_action_is_reported_once() {
    // C#: SettingsNormalizer `Actions.RemoveAll(a => a is null)` (the reader dropped the nulls: read_notes)
    let mut s = defaults();
    s.read_notes.null_actions = 3;
    let repaired = normalize(&mut s);
    assert_eq!(repaired, ["Actions"]);
    assert!(normalize(&mut s).is_empty());
}

#[test]
fn normalize_valid_actions_are_not_reported() {
    // C#: SettingsNormalizerMutationGapTests.Normalize_ValidActions_AreNotReported
    let mut s = defaults();
    s.actions.clear();
    let mut a = action("A", "J");
    a.destination = "C:\\x".to_owned();
    a.operation = FileOperationType::Copy;
    s.actions.push(a);
    let repaired = normalize(&mut s);
    assert!(!repaired.iter().any(|n| n == "Actions"));
    assert_eq!(s.actions.len(), 1);
    assert_eq!(s.actions[0].operation, FileOperationType::Copy);
}

#[test]
fn disable_conflicting_optional_shortcuts_no_actions_list_does_not_throw() {
    // C#: SettingsNormalizerMutationGapTests.DisableConflictingOptionalShortcuts_NoActionsList_DoesNotThrow
    let mut s = defaults();
    s.actions.clear();
    assert!(disable_conflicting_optional_shortcuts(&mut s).is_empty());
}

#[test]
fn canonicalize_all_no_actions_list_still_canonicalises_the_shortcuts() {
    // C#: SettingsNormalizerMutationGapTests.CanonicalizeAll_NoActionsList_StillCanonicalisesTheShortcuts
    let mut s = defaults();
    s.actions.clear();
    s.shortcuts.next = "Return".to_owned();
    s.canonicalize_all();
    assert_eq!(s.shortcuts.next, canonicalize(Some("Return")));
}

// ---- SettingsNormalizerMandatoryShortcutConflictTests ----

#[test]
fn mandatory_shortcut_set_is_the_thirteen_validated_bindings() {
    // C#: SettingsNormalizerMandatoryShortcutConflictTests.MandatoryShortcutSet_IsTheThirteenValidatedBindings
    assert_eq!(mandatory_names().len(), 13);
}

#[test]
fn optional_shortcut_sharing_a_mandatory_key_is_disabled_and_settings_stay_valid() {
    // C#: SettingsNormalizerMandatoryShortcutConflictTests.OptionalShortcut_SharingAMandatoryKey_IsDisabledAndSettingsStayValid
    let default_shortcuts = ShortcutMappings::default();
    let v = validator(Arc::new(NonBlank));
    let mut pairs = 0;
    for mandatory in mandatory_names() {
        for optional in ShortcutMappings::OPTIONAL_NAMES {
            let mut s = defaults();
            s.actions.clear();
            s.shortcuts = ShortcutMappings::default();
            // C# starts from `new ShortcutMappings()` (the defaults); an empty optional default uses a key no default uses.
            let key = match default_shortcuts.get(optional) {
                Some(k) if !k.is_empty() => k.to_owned(),
                _ => "J".to_owned(),
            };
            set(&mut s, mandatory, &key);
            set(&mut s, optional, &key);

            let disabled = disable_conflicting_optional_shortcuts(&mut s);

            assert!(disabled.iter().any(|n| n == optional), "{mandatory}/{optional}");
            assert_eq!(get(&s, optional), "", "{mandatory}/{optional}");
            assert_eq!(get(&s, mandatory), key, "{mandatory}/{optional}");
            assert_eq!(v.validate_shortcuts(&s), None, "{mandatory}/{optional}");
            pairs += 1;
        }
    }
    assert_eq!(pairs, 13 * 14);
}

// ---- SettingsValidatorTests ----

#[test]
fn default_settings_are_valid() {
    // C#: SettingsValidatorTests.DefaultSettings_AreValid
    assert_eq!(known_validator().validate_shortcuts(&defaults()), None);
}

#[test]
fn invalid_shortcut_key_returns_error() {
    // C#: SettingsValidatorTests.InvalidShortcutKey_ReturnsError
    for (property, key) in [("Next", "InvalidKey"), ("Previous", "   ")] {
        let mut s = defaults();
        set(&mut s, property, key);
        let error = known_validator().validate_shortcuts(&s);
        assert_eq!(error, Some(EN.core_settings_shortcut_invalid(property)), "{property}");
    }
}

#[test]
fn invalid_action_returns_error() {
    // C#: SettingsValidatorTests.InvalidAction_ReturnsError
    for (name, shortcut) in [("", "Enter"), ("TestAction", "UnknownKey123")] {
        let mut s = defaults();
        s.actions = vec![action(name, shortcut)];
        let error = known_validator().validate_shortcuts(&s);
        assert_eq!(error, Some(EN.core_settings_action_invalid()), "{name}");
    }
}

#[test]
fn duplicate_keys_returns_conflict_message() {
    // C#: SettingsValidatorTests.DuplicateKeys_ReturnsConflictMessage
    let mut s = defaults();
    s.shortcuts.previous = "Right".to_owned();
    let error = known_validator().validate_shortcuts(&s).expect("conflict");
    assert!(error.contains("Key Right is used more than once by:"), "{error}");
    assert!(error.contains("Next"));
    assert!(error.contains("Previous"));
}

#[test]
fn action_and_shortcut_conflict_returns_conflict_message() {
    // C#: SettingsValidatorTests.ActionAndShortcutConflict_ReturnsConflictMessage
    let mut s = defaults();
    s.actions = vec![action("DuplicateAction", "Right")];
    let error = known_validator().validate_shortcuts(&s).expect("conflict");
    assert!(error.contains("Key Right is used more than once by:"), "{error}");
    assert!(error.contains("Next"));
    assert!(error.contains("Action: DuplicateAction"));
}

// ---- SettingsValidatorRobustnessTests ----

fn letters_validator() -> SettingsValidator {
    validator(Arc::new(LettersDigits))
}

#[test]
fn blank_mandatory_names_the_property() {
    // C#: SettingsValidatorRobustnessTests.BlankMandatory_NamesTheProperty
    for property in mandatory_names() {
        let mut s = defaults();
        set(&mut s, property, "  ");
        assert_eq!(
            letters_validator().validate_shortcuts(&s),
            Some(EN.core_settings_shortcut_invalid(property)),
            "{property}"
        );
    }
}

#[test]
fn optional_blank_ok_duplicate_still_conflicts() {
    // C#: SettingsValidatorRobustnessTests.Optional_BlankOk_DuplicateStillConflicts
    let mut ok = defaults();
    for name in [
        "LastImage",
        "ZoomActualSize",
        "ToggleInfoOverlay",
        "MoveToFolder",
        "CopyToFolder",
    ] {
        set(&mut ok, name, "");
    }
    assert_eq!(letters_validator().validate_shortcuts(&ok), None);

    let mut clash = defaults();
    clash.shortcuts.copy_to_folder = clash.shortcuts.move_to_folder.clone();
    let expected = EN.core_settings_shortcut_duplicate(&clash.shortcuts.move_to_folder, "MoveToFolder, CopyToFolder");
    assert_eq!(letters_validator().validate_shortcuts(&clash), Some(expected));
}

#[test]
fn duplicate_ignores_case_and_padding() {
    // C#: SettingsValidatorRobustnessTests.Duplicate_IgnoresCaseAndPadding
    let mut s = defaults();
    s.actions = vec![action("A", " f7 "), action("B", "F7")];
    let error = letters_validator().validate_shortcuts(&s).expect("conflict");
    assert!(error.to_ascii_uppercase().contains("F7"), "{error}");
    assert!(error.contains(&EN.core_settings_action_binding_name("A")));
    assert!(error.contains(&EN.core_settings_action_binding_name("B")));
}

/// Deterministic xorshift64 (no `rand` dependency).
struct XorShift(u64);
impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[test]
fn fuzz_conflict_oracle() {
    // C#: SettingsValidatorRobustnessTests.Fuzz_ConflictOracle (seeds 1-3, 500 rounds)
    let pool = [
        "A", "B", "C", "D", "E", "F1", "F2", "F3", "F4", "F5", "G", "H", "J", "K", "L", "N", "O", "P", "Q", "R", "S",
    ];
    for seed in 1_u64..=3 {
        let mut r = XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for round in 0..500 {
            let mut s = defaults();
            s.actions.clear();
            let mut used: Vec<String> = Vec::new();
            let mut pick = |r: &mut XorShift| {
                let mut key = if r.below(6) == 0 && !used.is_empty() {
                    used[r.below(used.len() as u64) as usize].clone()
                } else {
                    format!("{}{}", pool[r.below(pool.len() as u64) as usize], r.below(1000))
                };
                if r.below(4) == 0 {
                    key = key.to_ascii_lowercase();
                }
                used.push(key.clone());
                key
            };
            for name in ShortcutMappings::PROPERTY_NAMES {
                if name == "MoveToFolder2" {
                    continue;
                }
                let value = if r.below(8) == 0 && ShortcutMappings::is_optional(name) {
                    String::new()
                } else {
                    pick(&mut r)
                };
                set(&mut s, name, &value);
            }
            for i in (1..=r.below(4)).rev() {
                let key = pick(&mut r);
                s.actions.push(action(&format!("Act{i}"), &key));
            }

            let mut keys: Vec<String> = ShortcutMappings::PROPERTY_NAMES
                .iter()
                .filter(|n| **n != "MoveToFolder2")
                .map(|n| get(&s, n).to_owned())
                .chain(s.actions.iter().map(|a| a.shortcut.clone()))
                .filter(|k| !k.is_empty())
                .map(|k| k.to_ascii_uppercase())
                .collect();
            let total = keys.len();
            keys.sort();
            keys.dedup();
            let has_conflict = keys.len() != total;

            let error = letters_validator().validate_shortcuts(&s);
            assert_eq!(has_conflict, error.is_some(), "seed {seed} round {round}");
        }
    }
}

// ---- hand-written: SettingsParseResult.Repairs ----

#[test]
fn parse_result_repairs_lists_resets_then_prefixed_disabled_shortcuts() {
    // no C# twin: `SettingsParseResult.Repairs`
    let result = SettingsParseResult {
        settings: defaults(),
        salvaged: vec!["Salvaged".to_owned()],
        reset_settings: vec!["Salvaged".to_owned(), "PreloadWorkerCount".to_owned()],
        disabled_shortcuts: vec!["Refresh".to_owned(), "FitWidth".to_owned()],
        salvage_cause: None,
        is_json_null: false,
    };
    assert_eq!(
        result.repairs(),
        [
            "Salvaged",
            "PreloadWorkerCount",
            &format!("{SHORTCUT_REPAIR_PREFIX}Refresh"),
            "Shortcuts.FitWidth"
        ]
    );
}

// ---- repair round 1: mutation-killing probes ----

#[test]
fn validate_passes_the_trimmed_shortcut_text_to_the_key_validator() {
    // C#: SettingsValidator trims the property text before IsValidKeyName (padded key must be accepted).
    let mut s = defaults();
    s.actions.clear();
    set(&mut s, "Next", " Right ");
    let v = validator(Arc::new(LettersDigits));
    assert_eq!(v.validate_shortcuts(&s), None);
}

#[test]
fn validate_passes_the_trimmed_action_shortcut_to_the_key_validator() {
    let mut s = defaults();
    s.actions.clear();
    s.actions.push(action("Act", " Q "));
    let v = validator(Arc::new(LettersDigits));
    assert_eq!(v.validate_shortcuts(&s), None);
}

#[test]
fn legacy_move_to_folder2_key_does_not_count_as_taken_for_optional_shortcuts() {
    // C#: DisableConflictingOptionalShortcuts.Take() lists only the 13 mandatory names (not MoveToFolder2).
    let mut s = defaults();
    s.actions.clear();
    set(&mut s, "MoveToFolder2", "Q");
    set(&mut s, "LastImage", "Q");
    let disabled = disable_conflicting_optional_shortcuts(&mut s);
    assert!(disabled.is_empty());
    assert_eq!(get(&s, "LastImage"), "Q");
}
