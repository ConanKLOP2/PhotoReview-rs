//! Port of `PhotoReview.App.Tests/Input/KeyIdMappingTests.cs` (baseline `d92d6aae`, 8 methods) and the golden checks over
//! `tests/golden/key-id-mapping.json` (oracle `oracle/W8-05c.cs`, command `key-id-mapping`). Unit W8-05c, contract
//! `contracts/W8-05c.md`.
//!
//! Red stage: `from_virtual_key`, `to_virtual_key` and `try_parse` are `todo!()` in the stub, so every test that calls them
//! fails until the porter lands the bodies. `golden_enum_fields_match_the_key_names_table` reads the already-generated
//! `KEY_NAMES` table and passes on the stub (data only, no logic), like the W8-05b `declared` test.
//!
//! Documented skips (not ported as assertions):
//! - C# `ToVirtualKey_MatchesKeyInterop_ForEveryKey` and `FromVirtualKey_MatchesKeyInterop_ForEveryCode` compare with WPF
//!   `KeyInterop`, which the Rust port cannot run. The golden `from-vk` and `to-vk` rows pin the same tables
//!   (contract open question 4); the golden facts below replace them.
//! - Out-of-range KEY values (`-2`, `-1`, `173..=177`, `1000`, `i32::MAX`, `i32::MIN`) cannot be built as `KeyId`: the type is a
//!   `u8` newtype over 0..=172 (W8-05b, `KeyId::from_value` is `None` for them). The golden `to-vk` rows for those 10 values
//!   are counted and skipped in `golden_to_vk_matches_the_oracle`; the expected answer (`0`) is still checked through
//!   `KeyId::from_value` returning `None`. Open question for the lead (see the report): does `to_virtual_key` need an
//!   `i32` input to keep trap 5 testable?
//! - C# `TryParse_AgreesWithShortcutKeyName_ForEveryKeyName` is deferred (contract open question 5): it needs
//!   `services::shortcut_key_name` (W8-04c).

use pr_app::input::key_id::{KEY_ID_NAMES, KeyId};
use pr_app::input::key_mapping::{KEY_NAMES, from_virtual_key, to_virtual_key, try_parse};
use pr_test_support::golden::{self, i64_of, str_of};
use serde_json::Value;

/// `VkControl` / `VkMenu` in the C# source, and the generic Shift code used by the golden.
const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12;

/// Virtual-key codes outside `0..=255` that the golden pins (trap 1: they must give `KeyId::NONE`, never panic).
const OUT_OF_RANGE_VK: [i32; 5] = [-1, 256, 0x1000, i32::MAX, i32::MIN];

/// Golden cases of one `kind` from `tests/golden/key-id-mapping.json`.
fn golden_cases(kind: &str) -> Vec<Value> {
    golden::load("key-id-mapping")
        .cases
        .into_iter()
        .filter(|case| case["kind"] == kind)
        .collect()
}

/// The golden `key` of a `from-vk` row as a `KeyId` (every expected key in the golden is a defined value).
fn expected_key(case: &Value) -> KeyId {
    let value = i32::try_from(i64_of(case, "key")).expect("golden key fits i32");
    KeyId::from_value(value).unwrap_or_else(|| panic!("golden key {value} is defined"))
}

/// `from_virtual_key(vk, extended)` as pinned by the golden `from-vk` rows (`rows` is the parsed `from-vk` case list).
fn golden_from_vk(rows: &[Value], vk: i32, extended: bool) -> KeyId {
    rows.iter()
        .find(|case| i64::from(vk) == i64_of(case, "vk") && case["extended"] == extended)
        .map(expected_key)
        .unwrap_or_else(|| panic!("golden has no from-vk row for vk {vk} extended {extended}"))
}

/// The 202 declared field names, in declaration order, from `tests/golden-inputs/key-id-mapping.json` (`names`).
fn golden_input_names() -> Vec<String> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("key-id-mapping.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["names"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'names'", path.display()))
        .iter()
        .map(|name| name.as_str().expect("input name is a string").to_owned())
        .collect()
}

// ---- Facts ported from KeyIdMappingTests.cs -------------------------------------------------------------------------

#[test]
fn from_virtual_key_matches_the_golden_for_every_code_and_outside_it() {
    // C#: KeyIdMappingTests.FromVirtualKey_MatchesKeyInterop_ForEveryCode (WPF KeyInterop; the golden `from-vk` rows replace it).
    // Codes 0..=255 and the out-of-range codes, extended flag false.
    let rows: Vec<Value> = golden_cases("from-vk")
        .into_iter()
        .filter(|case| case["extended"] == false)
        .collect();
    assert_eq!(
        rows.len(),
        256 + OUT_OF_RANGE_VK.len(),
        "golden has 261 plain from-vk rows"
    );
    for case in &rows {
        let vk = i32::try_from(i64_of(case, "vk")).expect("golden vk fits i32");
        assert_eq!(from_virtual_key(vk, false), expected_key(case), "vk 0x{vk:X}");
    }
    for vk in OUT_OF_RANGE_VK {
        assert_eq!(from_virtual_key(vk, false), KeyId::NONE, "out-of-range vk {vk}");
    }
}

#[test]
fn from_virtual_key_extended_changes_only_control_and_menu() {
    // C#: KeyIdMappingTests.FromVirtualKey_Extended_OnlyControlAndAltBecomeRight
    let rows = golden_cases("from-vk");
    for vk in 0..=255 {
        let expected = match vk {
            VK_CONTROL => KeyId::RIGHT_CTRL,
            VK_MENU => KeyId::RIGHT_ALT,
            _ => golden_from_vk(&rows, vk, false),
        };
        assert_eq!(from_virtual_key(vk, true), expected, "vk 0x{vk:X}");
    }
    // Written out (trap 2): the generic codes and the flag, and no RightShift from 0x10.
    assert_eq!(from_virtual_key(VK_SHIFT, false), KeyId::LEFT_SHIFT);
    assert_eq!(from_virtual_key(VK_SHIFT, true), KeyId::LEFT_SHIFT);
    assert_eq!(from_virtual_key(VK_CONTROL, false), KeyId::LEFT_CTRL);
    assert_eq!(from_virtual_key(VK_CONTROL, true), KeyId::RIGHT_CTRL);
    assert_eq!(from_virtual_key(VK_MENU, false), KeyId::LEFT_ALT);
    assert_eq!(from_virtual_key(VK_MENU, true), KeyId::RIGHT_ALT);
}

#[test]
fn to_virtual_key_matches_the_golden_for_every_key_value() {
    // C#: KeyIdMappingTests.ToVirtualKey_MatchesKeyInterop_ForEveryKey (WPF KeyInterop; the golden `to-vk` rows replace it).
    // The 173 defined values are checked; the 10 values a KeyId cannot hold are skipped (see the file header).
    for case in golden_cases("to-vk") {
        let key = i64_of(&case, "key");
        let Ok(key) = i32::try_from(key) else { continue };
        let Some(key_id) = KeyId::from_value(key) else { continue };
        let expected = i32::try_from(i64_of(&case, "vk")).expect("golden vk fits i32");
        assert_eq!(to_virtual_key(key_id), expected, "key {key}");
    }
}

#[test]
fn to_virtual_key_then_from_virtual_key_keeps_the_code() {
    // C#: KeyIdMappingTests.ToVirtualKey_ThenFromVirtualKey_KeepsTheCode (loop over the 173 defined values, not Enum.GetValues
    // with its aliases: an alias has the same code as its value, so the loop over values covers the same pairs).
    for value in 0..=172 {
        let key = KeyId::from_value(value).expect("0..=172 are defined");
        let vk = to_virtual_key(key);
        if vk == 0 {
            continue;
        }
        assert_eq!(
            to_virtual_key(from_virtual_key(vk, false)),
            vk,
            "key {value} vk 0x{vk:X}"
        );
    }
}

#[test]
fn to_virtual_key_is_zero_only_for_none_line_feed_system_and_dead_char() {
    // C#: KeyIdMappingTests.ToVirtualKey_ZeroOnlyForKeysWithoutACode (expected values written out: None, LineFeed, System,
    // DeadCharProcessed = 0, 4, 156, 172; trap 5).
    let without_code: Vec<i32> = (0..=172)
        .filter(|&value| to_virtual_key(KeyId::from_value(value).expect("0..=172 are defined")) == 0)
        .collect();
    assert_eq!(without_code, vec![0, 4, 156, 172]);
    assert_eq!(to_virtual_key(KeyId::NONE), 0);
    assert_eq!(to_virtual_key(KeyId::LINE_FEED), 0);
    assert_eq!(to_virtual_key(KeyId::SYSTEM), 0);
    assert_eq!(to_virtual_key(KeyId::DEAD_CHAR_PROCESSED), 0);
}

#[test]
fn try_parse_valid_names_parse_with_aliases_canonicalised() {
    // C#: KeyIdMappingTests.TryParse_ValidName_Parses (9 rows; `Return` and `Prior` are aliases, `enter` needs canonicalise).
    let rows = [
        ("Enter", KeyId::ENTER),
        ("Return", KeyId::ENTER),
        ("enter", KeyId::ENTER),
        ("PageUp", KeyId::PAGE_UP),
        ("Prior", KeyId::PAGE_UP),
        ("F11", KeyId::F11),
        ("A", KeyId::A),
        ("D2", KeyId::D2),
        (" Delete ", KeyId::DELETE),
    ];
    for (name, expected) in rows {
        assert_eq!(try_parse(name), Some(expected), "name {name:?}");
    }
}

#[test]
fn try_parse_numbers_lists_unknown_wrong_case_and_blanks_are_rejected() {
    // C#: KeyIdMappingTests.TryParse_Invalid_Rejected (10 rows; `None` is the C# `false` with key = None).
    let rows = ["999", "1", "172", "-1", "+44", "A,B", "NotAKey", "a", "", "  "];
    for name in rows {
        assert_eq!(try_parse(name), None, "name {name:?}");
    }
}

// C#: KeyIdMappingTests.TryParse_AgreesWithShortcutKeyName_ForEveryKeyName is DEFERRED (contract open question 5): it needs
// `services::shortcut_key_name` (W8-04c). Port it in that unit's test file once both are merged.

// ---- Golden (tests/golden/key-id-mapping.json, no C# method) --------------------------------------------------------

#[test]
fn golden_from_vk_matches_the_oracle() {
    // Every `from-vk` row: codes 0..=255 and the out-of-range codes, extended false and true (522 rows).
    let rows = golden_cases("from-vk");
    assert_eq!(rows.len(), 522, "golden holds 522 from-vk rows");
    for case in &rows {
        let vk = i32::try_from(i64_of(case, "vk")).expect("golden vk fits i32");
        let extended = case["extended"] == true;
        assert_eq!(
            from_virtual_key(vk, extended),
            expected_key(case),
            "vk 0x{vk:X} extended {extended}"
        );
    }
}

#[test]
fn golden_to_vk_matches_the_oracle() {
    // Every `to-vk` row (183). The 173 defined keys must match exactly; the 10 values a KeyId cannot hold are skipped and
    // counted (see the file header), and the skipped set is pinned so a change in the type shows up here.
    let rows = golden_cases("to-vk");
    assert_eq!(rows.len(), 183, "golden holds 183 to-vk rows");
    let mut checked = 0;
    let mut skipped = 0;
    for case in &rows {
        let key = i32::try_from(i64_of(case, "key")).ok();
        match key.and_then(KeyId::from_value) {
            Some(key_id) => {
                let expected = i32::try_from(i64_of(case, "vk")).expect("golden vk fits i32");
                assert_eq!(to_virtual_key(key_id), expected, "key {key_id:?}");
                checked += 1;
            }
            None => {
                // Not representable as KeyId: the golden answer must be 0, the only value this type can express.
                assert_eq!(i64_of(case, "vk"), 0, "unrepresentable key {}", i64_of(case, "key"));
                skipped += 1;
            }
        }
    }
    assert_eq!(checked, 173, "every defined key 0..=172 is checked");
    assert_eq!(
        skipped, 10,
        "the 10 unrepresentable golden keys are the documented skips"
    );
}

#[test]
fn golden_try_parse_matches_the_oracle() {
    // Every `try-parse` row (56): `ok` and, when ok, the key value. Covers trims, non-ASCII probes, casing, digits and signs.
    let rows = golden_cases("try-parse");
    assert_eq!(rows.len(), 56, "golden holds 56 try-parse rows");
    for case in &rows {
        let name = str_of(case, "name");
        let ok = case["ok"] == true;
        let parsed = try_parse(name);
        assert_eq!(parsed.is_some(), ok, "ok for name {name:?}");
        if ok {
            let key = i64_of(case, "key");
            assert_eq!(
                i64::from(parsed.expect("checked above").value()),
                key,
                "key for name {name:?}"
            );
        }
    }
}

#[test]
fn golden_enum_fields_match_the_key_names_table() {
    // The golden `enum-fields` row: 202 declared names, checked by the oracle against reflection (`names_equal`). The Rust
    // side: `KEY_NAMES` holds the same names, in the same order, with the value `KEY_ID_NAMES` records for each one.
    let rows = golden_cases("enum-fields");
    assert_eq!(rows.len(), 1, "golden holds one enum-fields row");
    assert_eq!(i64_of(&rows[0], "count"), 202);
    assert_eq!(rows[0]["names_equal"], true);

    let names = golden_input_names();
    assert_eq!(names.len(), 202, "input holds the 202 declared names");
    assert_eq!(KEY_NAMES.len(), 202, "KEY_NAMES holds 202 rows");
    assert_eq!(KEY_ID_NAMES.len(), 202);
    for (index, (name, value)) in KEY_NAMES.iter().enumerate() {
        assert_eq!(*name, names[index], "row {index}: name");
        assert_eq!(*value, i32::from(KEY_ID_NAMES[index].1), "row {index}: value of {name}");
    }
}
