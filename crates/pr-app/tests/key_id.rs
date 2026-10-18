//! Port of `PhotoReview.App.Shared/Input/KeyId.cs` tests: the mirror of WPF `Key` (`ContractMirrorTests.Enum_MirrorsWpfEnum`,
//! `KeyNameGoldenTests`), the golden checks over `tests/golden/key-id.json` (oracle `oracle/W8-05b.cs`, command `key-id`)
//! and the WPF fixture `tests/fixtures/golden/key-names.v1.json`. Unit W8-05b, contract `contracts/W8-05b.md`.
//!
//! Red stage: `KeyId::from_value`, `name`, `from_name` and `from_name_ignore_case` are `todo!()` in the stub, so the golden
//! `canonical`/`from_value`/`from_name` cases and the hand-written lookup tests fail until the porter implements them.
//! `declared` and the fixture-order and mirror tests read the already-generated `KEY_ID_NAMES` table and pass on the stub.
//!
//! Not ported here: `ContractMirrorTests` rows for `PointerButton` and `KeyModifiers` (other units),
//! `KeyNameGoldenTests.RecordKeyNames_WritesTheGoldenFromTheWpfBuild` (Manual, writes the fixture), and the
//! `ShortcutKeyName`, `KeyIdMapping` and `ShortcutRouter` tests of W8-04c, W8-05c and W8-05d.

use pr_app::input::key_id::{KEY_ID_NAMES, KeyId};
use pr_core::settings::shortcuts::ShortcutMappings;
use pr_test_support::golden::{self, i64_of, str_of};
use serde_json::Value;

/// `tests/fixtures/golden/key-names.v1.json` `items`: `(name, keyValue)` in the fixture's order (value, then ordinal name).
fn wpf_fixture() -> Vec<(String, i64)> {
    let path = golden::repo_root()
        .join("tests")
        .join("fixtures")
        .join("golden")
        .join("key-names.v1.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["items"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'items'", path.display()))
        .iter()
        .map(|item| {
            let name = item["name"].as_str().expect("fixture item has a name").to_owned();
            let value = item["keyValue"].as_i64().expect("fixture item has a keyValue");
            (name, value)
        })
        .collect()
}

// ---- Golden (tests/golden/key-id.json) --------------------------------------------------------------------------------

#[test]
fn golden_key_id_declared() {
    // C#: ContractMirrorTests.Enum_MirrorsWpfEnum (declaration order pinned by the golden; the table is generated from C#).
    let golden = golden::load("key-id");
    let declared: Vec<&Value> = golden.cases.iter().filter(|c| c["kind"] == "declared").collect();
    assert_eq!(declared.len(), 202, "golden declares 202 names");
    assert_eq!(KEY_ID_NAMES.len(), 202, "KEY_ID_NAMES holds 202 rows");
    for case in declared {
        let index = usize::try_from(i64_of(case, "index")).expect("index is non-negative");
        let (name, value) = KEY_ID_NAMES[index];
        assert_eq!(name, str_of(case, "name"), "declared row {index}: name");
        assert_eq!(i64::from(value), i64_of(case, "value"), "declared row {index}: value");
    }
}

#[test]
fn golden_key_id_canonical() {
    // C#: Enum.GetName(KeyId) for each of the 173 distinct values; the golden pins the alias answer (semantic note 4).
    let golden = golden::load("key-id");
    let canonical: Vec<&Value> = golden.cases.iter().filter(|c| c["kind"] == "canonical").collect();
    assert_eq!(
        canonical.len(),
        173,
        "golden holds one canonical name per distinct value"
    );
    for case in canonical {
        let value = i32::try_from(i64_of(case, "value")).expect("value fits i32");
        let key = KeyId::from_value(value).unwrap_or_else(|| panic!("value {value} is defined"));
        assert_eq!(key.name(), str_of(case, "name"), "canonical name for value {value}");
    }
}

#[test]
fn golden_key_id_from_value() {
    // C#: Enum.IsDefined((KeyId)value) and Enum.GetName for the defined ones.
    let golden = golden::load("key-id");
    let cases: Vec<&Value> = golden.cases.iter().filter(|c| c["kind"] == "from_value").collect();
    assert_eq!(cases.len(), 11, "golden holds 11 from_value probes");
    for case in cases {
        let input = i32::try_from(i64_of(case, "input")).expect("probe fits i32");
        let defined = case["defined"].as_bool().expect("defined is a bool");
        let got = KeyId::from_value(input);
        assert_eq!(got.is_some(), defined, "from_value({input}) is defined");
        if defined {
            assert_eq!(
                got.expect("defined").name(),
                str_of(case, "name"),
                "from_value({input}) name"
            );
        }
    }
}

#[test]
fn golden_key_id_from_name() {
    // C#: Enum.TryParse<KeyId>(name, ignoreCase, out key) plus Enum.IsDefined; names only, exact or OrdinalIgnoreCase.
    let golden = golden::load("key-id");
    let cases: Vec<&Value> = golden.cases.iter().filter(|c| c["kind"] == "from_name").collect();
    assert_eq!(cases.len(), 500, "golden holds 500 from_name probes");
    for case in cases {
        let input = str_of(case, "input");
        let ignore_case = case["ignore_case"].as_bool().expect("ignore_case is a bool");
        let found = case["found"].as_bool().expect("found is a bool");
        let got = if ignore_case {
            KeyId::from_name_ignore_case(input)
        } else {
            KeyId::from_name(input)
        };
        assert_eq!(
            got.is_some(),
            found,
            "from_name({input:?}, ignore_case={ignore_case}) found"
        );
        if found {
            assert_eq!(
                i64::from(got.expect("found").value()),
                i64_of(case, "value"),
                "from_name({input:?}, ignore_case={ignore_case}) value"
            );
        }
    }
}

// ---- WPF fixture (tests/fixtures/golden/key-names.v1.json) ------------------------------------------------------------

#[test]
fn key_id_mirrors_wpf_key_names_and_values() {
    // C#: ContractMirrorTests.Enum_MirrorsWpfEnum[KeyId, Key]: sorted (name, value) lists are equal; aliases included.
    let mut ours: Vec<(String, i64)> = KEY_ID_NAMES
        .iter()
        .map(|(n, v)| ((*n).to_owned(), i64::from(*v)))
        .collect();
    let mut wpf = wpf_fixture();
    ours.sort();
    wpf.sort();
    assert_eq!(ours.len(), 202, "202 declared names");
    assert_eq!(ours, wpf);
}

#[test]
fn key_id_names_match_wpf_key_fixture_in_value_then_name_order() {
    // C#: KeyNameGoldenTests.KeyNames_CommittedGolden_MatchesTheWpfKeyEnumeration (value order, then ordinal name).
    let mut ours: Vec<(String, i64)> = KEY_ID_NAMES
        .iter()
        .map(|(n, v)| ((*n).to_owned(), i64::from(*v)))
        .collect();
    ours.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    assert_eq!(ours, wpf_fixture());
}

#[test]
fn key_id_default_shortcut_names_parse() {
    // C#: KeyNameGoldenTests.KeyNames_EveryShortcutNameParsesToItsGoldenValue. Empty strings mean "no shortcut" and are skipped,
    // as the C# test does with IsNullOrEmpty.
    let fixture = wpf_fixture();
    for (property, name) in ShortcutMappings::default().entries() {
        if name.is_empty() {
            continue;
        }
        let entry = fixture
            .iter()
            .find(|(golden_name, _)| golden_name == name)
            .unwrap_or_else(|| panic!("{property}={name} is not a golden key name"));
        let parsed = KeyId::from_name(name).unwrap_or_else(|| panic!("{property}={name} parses"));
        assert_eq!(
            i64::from(parsed.value()),
            entry.1,
            "{property}={name} parses to its golden value"
        );
    }
}

// ---- Hand-written (new in the Rust port) ------------------------------------------------------------------------------

#[test]
fn aliases_share_one_value() {
    // Declared aliases are equal values, so `Enter` and `Return` are one key (semantic note 3).
    assert_eq!(KeyId::ENTER, KeyId::RETURN);
    assert_eq!(KeyId::CAPS_LOCK, KeyId::CAPITAL);
    assert_eq!(KeyId::HANGUL_MODE, KeyId::KANA_MODE);
    assert_eq!(KeyId::KANJI_MODE, KeyId::HANJA_MODE);
    assert_eq!(KeyId::PAGE_UP, KeyId::PRIOR);
    assert_eq!(KeyId::PAGE_DOWN, KeyId::NEXT);
    assert_eq!(KeyId::PRINT_SCREEN, KeyId::SNAPSHOT);
    assert_eq!(KeyId::OEM_SEMICOLON, KeyId::OEM1);
}

#[test]
fn value_is_the_raw_wpf_value() {
    // C#: (int)key. The table maximum 172 is DeadCharProcessed; NoName is 169.
    assert_eq!(KeyId::NONE.value(), 0);
    assert_eq!(KeyId::ESCAPE.value(), 13);
    assert_eq!(KeyId::NO_NAME.value(), 169);
    assert_eq!(KeyId::DEAD_CHAR_PROCESSED.value(), 172);
}

#[test]
fn from_value_rejects_out_of_range() {
    // C#: Enum.IsDefined((KeyId)value) is false outside 0..=172.
    assert_eq!(KeyId::from_value(-1), None);
    assert_eq!(KeyId::from_value(173), None);
    assert_eq!(KeyId::from_value(i32::MAX), None);
    assert_eq!(KeyId::from_value(0), Some(KeyId::NONE));
    assert_eq!(KeyId::from_value(172), Some(KeyId::DEAD_CHAR_PROCESSED));
}

#[test]
fn from_name_does_not_parse_numbers() {
    // Semantic note 5: names only. C# Enum.TryParse("6") gives 6; the caller (ShortcutKeyName, KeyIdMapping) decides that.
    assert_eq!(KeyId::from_name("6"), None);
    assert_eq!(KeyId::from_name("-6"), None);
}

#[test]
fn from_name_is_case_sensitive() {
    // C#: Enum.TryParse(name, false, ...) is ordinal and case-sensitive.
    assert_eq!(KeyId::from_name("tab"), None);
    assert_eq!(KeyId::from_name("Tab"), Some(KeyId::TAB));
}

#[test]
fn from_name_ignore_case_uses_ordinal_casing() {
    // C#: OrdinalIgnoreCase. Kelvin sign, long s and dotless or dotted i fold to no ASCII letter; plain `k` matches `K`.
    assert_eq!(KeyId::from_name_ignore_case("\u{212a}"), None);
    assert_eq!(KeyId::from_name_ignore_case("k"), Some(KeyId::K));
    assert_eq!(KeyId::from_name_ignore_case("\u{17f}pace"), None);
    assert_eq!(KeyId::from_name_ignore_case("cap\u{131}tal"), None);
    assert_eq!(KeyId::from_name_ignore_case("CAP\u{130}TAL"), None);
}

#[test]
fn from_name_trims_surrounding_whitespace_but_not_nul() {
    // Semantic note 5: str::trim() as Enum.TryParse does; NUL is not trimmed; empty and blank strings are None.
    assert_eq!(KeyId::from_name("\u{a0}F1"), Some(KeyId::F1));
    assert_eq!(KeyId::from_name("Tab\u{0000}"), None);
    assert_eq!(KeyId::from_name(""), None);
    assert_eq!(KeyId::from_name(" \t "), None);
}
