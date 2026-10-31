//! Port of `PhotoReview.Core/Settings/ShortcutMappings.cs`, `ShortcutKeyCanonical.cs` and `ReviewAction.cs`
//! (baseline 7eaee2a2) and the C# tests that cover them. Golden: `tests/golden/shortcut-canonical.json`.

use pr_core::model::enums::FileOperationType;
use pr_core::settings::shortcuts::{ALIAS_PAIRS, ReviewAction, ShortcutMappings, canonicalize, canonicalize_all};
use pr_test_support::golden::{self, bool_of, str_of};
use serde_json::Value;

/// The golden cases of one kind, in file order. Also checks the header: the file must come from the pinned baseline.
fn kind_cases(kind: &str) -> Vec<Value> {
    let g = golden::load("shortcut-canonical");
    assert!(
        g.csharp_commit.starts_with("7eaee2a2"),
        "golden baseline {}",
        g.csharp_commit
    );
    g.cases.into_iter().filter(|case| case["kind"] == kind).collect()
}

/// Deterministic generator for the idempotence test (the C# test uses `new Random(25)`; the sequence does not matter).
struct Lcg(u64);

impl Lcg {
    fn next_below(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % bound
    }
}

// C#: ShortcutKeyCanonicalTests.Canonicalize_Alias_ReturnsCanonical (theory, 7 rows x 4 asserts).
#[test]
fn canonicalize_alias_returns_canonical() {
    let rows = [
        ("Return", "Enter"),
        ("Prior", "PageUp"),
        ("Next", "PageDown"),
        ("Capital", "CapsLock"),
        ("Snapshot", "PrintScreen"),
        ("Oem1", "OemSemicolon"),
        ("Oem102", "OemBackslash"),
    ];
    for (alias, canonical) in rows {
        assert_eq!(canonicalize(Some(alias)), canonical, "alias {alias}");
        assert_eq!(
            canonicalize(Some(&alias.to_ascii_uppercase())),
            canonical,
            "upper {alias}"
        );
        assert_eq!(
            canonicalize(Some(&format!("  {} ", alias.to_ascii_lowercase()))),
            canonical,
            "padded lower {alias}"
        );
        assert_eq!(
            canonicalize(Some(&canonical.to_ascii_lowercase())),
            canonical,
            "canonical lower {canonical}"
        );
    }
}

// C#: ShortcutKeyCanonicalTests.AliasPairs_AllSameKey.
#[test]
fn alias_pairs_all_same_key() {
    for (alias, canonical) in ALIAS_PAIRS {
        assert_eq!(canonicalize(Some(alias)), canonical, "alias {alias}");
    }
}

// C#: ShortcutKeyCanonicalTests.Canonicalize_NonAliases_Unchanged.
#[test]
fn canonicalize_non_aliases_unchanged() {
    assert_eq!(canonicalize(Some("Add")), "Add");
    assert_eq!(canonicalize(None), "");
    assert_eq!(canonicalize(Some(" Bogus ")), "Bogus");
    assert_eq!(canonicalize(Some("f11")), "f11");
}

// C#: ShortcutKeyCanonicalTests.Canonicalize_IsIdempotent (500 random strings plus every table name).
#[test]
fn canonicalize_is_idempotent() {
    let alphabet: &[u8] = b"aA1 ,+-ReturnPriorNext";
    let mut rng = Lcg(25);
    let mut inputs: Vec<String> = ALIAS_PAIRS
        .iter()
        .flat_map(|&(alias, canonical)| [alias.to_owned(), canonical.to_owned()])
        .collect();
    for _ in 0..500 {
        let len = rng.next_below(12);
        let text: String = (0..len)
            .map(|_| alphabet[rng.next_below(alphabet.len())] as char)
            .collect();
        inputs.push(text);
    }
    for input in &inputs {
        let once = canonicalize(Some(input));
        assert_eq!(canonicalize(Some(&once)), once, "idempotent {input:?}");
        assert_eq!(canonicalize(Some(&format!(" {once} "))), once, "padded {input:?}");
    }
}

// C#: AppSettingsTests.OpenFolderAndCustomZoomShortcuts_HaveDistinctDefaults (the ShortcutMappings part).
#[test]
fn open_folder_and_custom_zoom_shortcuts_have_distinct_defaults() {
    let shortcuts = ShortcutMappings::default();
    assert_eq!(shortcuts.open_folder, "O");
    assert_eq!(shortcuts.custom_zoom, "D3");
    assert!(ShortcutMappings::is_optional("OpenFolder"));
    assert!(ShortcutMappings::is_optional("CustomZoom"));
}

// C#: ReviewActionDefaultsTests.Defaults_NamesAreLocalized_FoldersAreNeutral. The catalog lookup is replaced by a
// closure; the Vietnamese one carries the strings of `assets/languages/vi.json`.
#[test]
fn defaults_names_are_localized_folders_are_neutral() {
    let english = ReviewAction::defaults(&|n: u32| format!("Action {n}"), "Backup");
    let vietnamese = ReviewAction::defaults(&|n: u32| format!("Loại {n}"), "Sao lưu");

    let names = |actions: &[ReviewAction]| actions.iter().map(|a| a.name.clone()).collect::<Vec<_>>();
    let destinations = |actions: &[ReviewAction]| actions.iter().map(|a| a.destination.clone()).collect::<Vec<_>>();
    let operations: Vec<FileOperationType> = english.iter().map(|a| a.operation).collect();

    assert_eq!(names(&english), ["Action 2", "Action 3", "Action 4", "Backup"]);
    assert_eq!(names(&vietnamese), ["Loại 2", "Loại 3", "Loại 4", "Sao lưu"]);
    assert_eq!(destinations(&english), ["Group-2", "Group-3", "Group-4", "Backup"]);
    assert_eq!(destinations(&english), destinations(&vietnamese));
    assert_eq!(
        operations,
        [
            FileOperationType::Move,
            FileOperationType::Move,
            FileOperationType::Move,
            FileOperationType::Copy
        ]
    );
}

// C#: ShortcutKeyCanonicalTests.OldConfig_LoadsAndSavesCanonical, canonicalisation part only.
#[test]
fn canonicalize_all_canonicalises_every_property_and_action_shortcut() {
    let mut shortcuts = ShortcutMappings {
        next: " Next ".to_owned(),
        previous_folder: "Prior".to_owned(),
        last_image: String::new(),
        ..ShortcutMappings::default()
    };
    let mut actions = vec![ReviewAction::new("A")];
    actions[0].shortcut = "RETURN".to_owned();
    actions[0].destination = "x".to_owned();

    canonicalize_all(&mut shortcuts, &mut actions);

    assert_eq!(shortcuts.next, "PageDown");
    assert_eq!(shortcuts.previous_folder, "PageUp");
    assert_eq!(shortcuts.last_image, "");
    assert_eq!(shortcuts.fullscreen, "F11", "non-alias unchanged");
    assert_eq!(actions[0].shortcut, "Enter");
    assert_eq!(actions[0].name, "A", "name is not touched");
    assert_eq!(actions[0].destination, "x", "destination is not touched");
}

// Every one of the 30 properties is canonicalised, not only the ones named in the test above.
// C#: ShortcutMappings properties reached by reflection in CanonicalizeAll.
#[test]
fn canonicalize_all_handles_every_property() {
    for name in ShortcutMappings::PROPERTY_NAMES {
        let mut shortcuts = ShortcutMappings::default();
        *shortcuts.get_mut(name).expect("known property") = " prior ".to_owned();
        canonicalize_all(&mut shortcuts, &mut []);
        assert_eq!(shortcuts.get(name), Some("PageUp"), "property {name}");
    }
}

// C#: the property list reflected by ShortcutMappings (GetProperties), with its accessors.
#[test]
fn get_get_mut_entries_agree_with_property_names() {
    let mut shortcuts = ShortcutMappings::default();
    assert_eq!(shortcuts.entries().len(), 30);
    for (i, name) in ShortcutMappings::PROPERTY_NAMES.iter().enumerate() {
        let (entry_name, entry_value) = shortcuts.entries()[i];
        assert_eq!(entry_name, *name);
        assert_eq!(shortcuts.get(name), Some(entry_value), "get {name}");

        let written = format!("Z{i}");
        *shortcuts.get_mut(name).expect("known property") = written.clone();
        assert_eq!(
            shortcuts.entries()[i],
            (*name, written.as_str()),
            "write-through {name}"
        );
    }
    assert!(shortcuts.get("").is_none());
    assert!(shortcuts.get("next").is_none());
    assert!(shortcuts.get("OptionalNames").is_none());
    assert!(shortcuts.get_mut("Bogus").is_none());
}

// C#: ReviewAction JSON (D-09): PascalCase names in declaration order, Operation written as the enum name.
#[test]
fn serialize_review_action_uses_pascal_case_names() {
    let action = ReviewAction::new("Action 2");
    assert_eq!(
        serde_json::to_string(&action).expect("serialize"),
        r#"{"Name":"Action 2","Shortcut":"Enter","Operation":"Move","Destination":"Group-2","Confirm":false}"#
    );
    let mut copy = ReviewAction::new("Backup");
    copy.operation = FileOperationType::Copy;
    copy.shortcut = "F5".to_owned();
    copy.destination = "Backup".to_owned();
    assert_eq!(
        serde_json::to_string(&copy).expect("serialize"),
        r#"{"Name":"Backup","Shortcut":"F5","Operation":"Copy","Destination":"Backup","Confirm":false}"#
    );
}

// Declaration order is part of the config file: the text form must list the properties in PROPERTY_NAMES order.
#[test]
fn serialize_shortcut_mappings_keeps_declaration_order() {
    let text = serde_json::to_string(&ShortcutMappings::default()).expect("serialize");
    let mut last = 0;
    for name in ShortcutMappings::PROPERTY_NAMES {
        let at = text
            .find(&format!("\"{name}\":"))
            .unwrap_or_else(|| panic!("missing {name} in {text}"));
        assert!(at > last, "{name} out of order in {text}");
        last = at;
    }
}

// Serde: a missing property keeps its default, an unknown property is ignored (`#[serde(default)]`).
#[test]
fn shortcut_mappings_missing_property_keeps_default_unknown_ignored() {
    let shortcuts: ShortcutMappings =
        serde_json::from_str(r#"{"Next":"N","Bogus":"x","FitWidth":"Q"}"#).expect("deserialize");
    assert_eq!(shortcuts.next, "N");
    assert_eq!(shortcuts.previous, "Left");
    assert_eq!(shortcuts.fit_width, "Q");
    assert_eq!(shortcuts.toggle_capture_member, "");
    assert_eq!(shortcuts.fit_width2, "D4");
    assert_eq!(shortcuts.refresh, "R");
}

// C#: ShortcutKeyCanonicalTests (golden twin): every alias pair, in table order.
// Golden kind "alias-pair".
#[test]
fn alias_pairs_match_golden() {
    let cases = kind_cases("alias-pair");
    assert_eq!(cases.len(), 29);
    assert_eq!(ALIAS_PAIRS.len(), 29);
    for (case, &(alias, canonical)) in cases.iter().zip(ALIAS_PAIRS.iter()) {
        assert_eq!(str_of(case, "alias"), alias);
        assert_eq!(str_of(case, "canonical"), canonical);
    }
}

// C#: ShortcutKeyCanonical.Canonicalize, including null, blanks, Unicode whitespace and non-ASCII look-alikes.
// Golden kind "canonicalize"; the name may be JSON null.
#[test]
fn canonicalize_matches_golden() {
    let cases = kind_cases("canonicalize");
    assert!(!cases.is_empty());
    for case in &cases {
        assert!(case.get("name").is_some(), "case without name: {case}");
        let name = case["name"].as_str();
        assert_eq!(canonicalize(name), str_of(case, "canonical"), "canonicalize({name:?})");
    }
}

// C#: ShortcutMappings.IsOptional. Golden kind "is-optional".
#[test]
fn is_optional_matches_golden() {
    let cases = kind_cases("is-optional");
    assert!(!cases.is_empty());
    for case in &cases {
        let name = str_of(case, "name");
        assert_eq!(
            ShortcutMappings::is_optional(name),
            bool_of(case, "optional"),
            "is_optional({name:?})"
        );
    }
}

// C#: ShortcutMappings defaults, OptionalNames and the JSON STJ writes for new ShortcutMappings().
// Golden kind "shortcut-defaults".
#[test]
fn shortcut_defaults_match_golden() {
    let cases = kind_cases("shortcut-defaults");
    assert_eq!(cases.len(), 1);
    let case = &cases[0];
    let defaults = ShortcutMappings::default();

    let properties = case["properties"].as_array().expect("properties array");
    assert_eq!(properties.len(), ShortcutMappings::PROPERTY_NAMES.len());
    for (i, property) in properties.iter().enumerate() {
        let name = str_of(property, "name");
        assert_eq!(name, ShortcutMappings::PROPERTY_NAMES[i], "property order");
        assert_eq!(
            defaults.get(name),
            Some(str_of(property, "default")),
            "default of {name}"
        );
        assert_eq!(
            ShortcutMappings::is_optional(name),
            bool_of(property, "optional"),
            "optional flag of {name}"
        );
    }

    let optional: Vec<&str> = case["optional_names"]
        .as_array()
        .expect("optional_names array")
        .iter()
        .map(|v| v.as_str().expect("string"))
        .collect();
    assert_eq!(optional, ShortcutMappings::OPTIONAL_NAMES);

    let written = serde_json::to_value(&defaults).expect("serialize");
    assert_eq!(written, case["json"], "JSON of new ShortcutMappings()");
    let back: ShortcutMappings = serde_json::from_value(case["json"].clone()).expect("deserialize golden JSON");
    assert_eq!(back, defaults, "JSON round trip");
}

// C#: ReviewAction.Defaults() without the localized Name. Golden kind "action-default".
#[test]
fn action_defaults_match_golden() {
    let cases = kind_cases("action-default");
    let actions = ReviewAction::defaults(&|n: u32| format!("Action {n}"), "Backup");
    assert_eq!(actions.len(), cases.len());
    for (action, case) in actions.iter().zip(&cases) {
        assert_eq!(action.shortcut, str_of(case, "shortcut"));
        // The variant name (Debug), not serde: the JSON of FileOperationType belongs to W1-02.
        assert_eq!(format!("{:?}", action.operation), str_of(case, "operation"));
        assert_eq!(action.destination, str_of(case, "destination"));
        assert_eq!(action.confirm, bool_of(case, "confirm"));
    }
}
