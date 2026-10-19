//! Tests of `pr_app::services::shortcut_key_name` (contract `contracts/W8-04c.md`, rules R-SK-1..5, traps T-1, T-2, T-8).
//!
//! Port of `ShortcutKeyNameTests.cs` and `ShortcutKeyCanonicalTableTests.cs`, plus the golden fixture test over
//! `tests/fixtures/golden/key-names.v1.json` (no oracle run; the fixture is synced from C#).
//!
//! Adaptation (documented): the contract assumed `KeyId::ALL` and `KeyId::wpf_value()`. The merged W8-05b shape is a `u8` newtype
//! with `KeyId::from_name`, `KeyId::from_name_ignore_case`, `KeyId::value()` and `KEY_ID_NAMES`; these tests use that shape.
//! `AliasTable_CoversAllEnumAliases` is owned by W8-05c and is not ported here.

use pr_app::input::key_id::KeyId;
use pr_app::services::shortcut_key_name::{KeyIdNameValidator, is_reserved, try_parse};
use pr_core::abstractions::KeyNameValidator;
use pr_core::settings::shortcuts::ALIAS_PAIRS;

/// The 14 keys of R-SK-2, by the WPF `Key` name.
const RESERVED_NAMES: [&str; 14] = [
    "None",
    "Escape",
    "Tab",
    "ImeProcessed",
    "DeadCharProcessed",
    "System",
    "LeftCtrl",
    "RightCtrl",
    "LeftAlt",
    "RightAlt",
    "LeftShift",
    "RightShift",
    "LWin",
    "RWin",
];

// C#: ShortcutKeyNameTests.TryParse_ValidKey_Parses (Theory, 4 cases: `Right`, ` delete ` (trim, case), `F11`, `Space`)
#[test]
fn valid_key_parses_case_insensitively_and_trimmed() {
    let cases = [
        ("Right", KeyId::RIGHT),
        (" delete ", KeyId::DELETE),
        ("F11", KeyId::F11),
        ("Space", KeyId::SPACE),
    ];
    for (name, expected) in cases {
        assert_eq!(try_parse(Some(name)), Some(expected), "name {name:?}");
    }
}

// C#: ShortcutKeyNameTests.TryParse_Unusable_Rejected (Theory, 12 cases). `try_parse(None)` is the C# `null` case.
// T-2: `""` and `"   "` are refused through different steps; both are checked.
#[test]
fn unusable_names_are_rejected() {
    let names = [
        "999",
        "-1",
        "+44",
        "A,B",
        "NotAKey",
        "",
        "   ",
        "Escape",
        "Tab",
        "ImeProcessed",
        "None",
        "LeftCtrl",
    ];
    for name in names {
        assert_eq!(try_parse(Some(name)), None, "name {name:?} must be refused");
    }
    assert_eq!(try_parse(None), None, "C# null");
}

// C#: ShortcutKeyCanonicalTableTests.TryParse_Alias_SameKey (Theory, 3 cases)
#[test]
fn alias_parses_to_same_key_as_canonical() {
    let cases = [("Return", "Enter"), ("Prior", "PageUp"), ("next", "PageDown")];
    for (alias, canonical) in cases {
        let parsed_alias = try_parse(Some(alias));
        let parsed_canonical = try_parse(Some(canonical));
        assert!(parsed_alias.is_some(), "alias {alias:?} must parse");
        assert_eq!(parsed_alias, parsed_canonical, "{alias:?} and {canonical:?}");
    }
}

// C#: ShortcutKeyCanonicalTableTests.AliasTable_MatchesKeyEnum (Core `ALIAS_PAIRS`, 29 pairs; every pair is one key on both sides)
#[test]
fn every_alias_pair_parses_to_one_key() {
    assert_eq!(ALIAS_PAIRS.len(), 29);
    for (alias, canonical) in ALIAS_PAIRS {
        assert!(
            KeyId::from_name(alias).is_some(),
            "alias {alias:?} is a declared key name"
        );
        assert_eq!(
            try_parse(Some(alias)),
            try_parse(Some(canonical)),
            "{alias:?} and {canonical:?}"
        );
    }
}

// Contract golden test (G-fixture): every item of `key-names.v1.json` (recorded from `System.Windows.Input.Key`) must agree with
// `KeyId`: the value matches the WPF value, and a reserved key is refused while every other key parses to the same value.
#[test]
fn key_names_fixture_every_item_matches_the_wpf_value() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/golden/key-names.v1.json"
    );
    let text = std::fs::read_to_string(path).expect("key-names.v1.json is committed");
    let document: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let items = document["items"].as_array().expect("items array");

    let mut failures = Vec::new();
    for item in items {
        let name = item["name"].as_str().expect("name");
        let key_value = item["keyValue"].as_i64().expect("keyValue");

        // A golden name missing from the KeyId vocabulary fails the test and is named.
        let Some(key) = KeyId::from_name(name) else {
            failures.push(format!("{name}: missing from KeyId"));
            continue;
        };
        if i64::from(key.value()) != key_value {
            failures.push(format!("{name}: KeyId value {} != golden {key_value}", key.value()));
        }

        let parsed = try_parse(Some(name));
        if RESERVED_NAMES.contains(&name) {
            if parsed.is_some() {
                failures.push(format!("{name}: reserved key must be refused"));
            }
        } else {
            match parsed {
                Some(parsed_key) if i64::from(parsed_key.value()) == key_value => {}
                other => failures.push(format!("{name}: try_parse gave {other:?}, golden value {key_value}")),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} golden mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// T-1: the Kelvin sign (U+212A) is not the letter K under OrdinalIgnoreCase (its upper case is itself).
#[test]
fn kelvin_sign_does_not_match_k() {
    assert!(KeyId::from_name_ignore_case("K").is_some(), "the K key exists");
    assert_eq!(try_parse(Some("\u{212A}")), None);
}

// T-1: the dotless i (U+0131) folds to `I` only when the generated OrdinalIgnoreCase table says so (Q9). The test asserts the
// consistency with `eq_ignore_case`, not a fixed answer.
#[test]
fn dotless_i_follows_the_ordinal_table() {
    let folds = pr_core::text::eq_ignore_case("\u{131}nsert", "Insert");
    let parsed = try_parse(Some("\u{131}nsert"));
    assert_eq!(
        parsed == Some(KeyId::INSERT),
        folds,
        "parse of U+0131 must agree with eq_ignore_case"
    );
}

// Contract test (R-SK-2): `is_reserved` is true for exactly the 14 keys and false for an ordinary key.
#[test]
fn is_reserved_is_exactly_the_fourteen_keys() {
    for name in RESERVED_NAMES {
        let key = KeyId::from_name(name).expect("reserved names are declared keys");
        assert!(is_reserved(key), "{name} is reserved");
    }
    assert!(!is_reserved(KeyId::RIGHT));
    assert!(!is_reserved(KeyId::F11));
}

// Contract test (R-SK-5): the validator trait object agrees with `try_parse`.
#[test]
fn keyid_name_validator_matches_try_parse() {
    let validator = KeyIdNameValidator;
    let dynamic: &dyn KeyNameValidator = &validator;
    for name in ["Right", " delete ", "Return", "999", "Escape", "", "NotAKey"] {
        assert_eq!(
            dynamic.is_valid_key_name(name),
            try_parse(Some(name)).is_some(),
            "name {name:?}"
        );
    }
}
