//! W9-R1: C# RenameNamePolicyTests and oracle at7eaee2a2. Pure strings; no user files.
use pr_core::file_actions::rename::{RenameNameCheck, RenameNamePolicy as Policy, RenameNameProblem as Problem};
use pr_test_support::golden;
use serde_json::{Value, json};

fn verdict(check: &RenameNameCheck) -> Value {
    json!({"problem":format!("{:?}",check.problem),"new_file_name":check.new_file_name,
        "invalid_character":check.invalid_character.map(|c|String::from_utf16_lossy(&[c])),
        "unchanged":check.is_unchanged,"case_only":check.differs_only_by_case})
}

// Oracle W9-R1.cs; input remains staged by the generator contract.
#[test]
fn golden_single_rename_policy_matches_all_33_cases() {
    let input: Value = serde_json::from_str(include_str!(
        "../../../tests/golden-inputs/staged/w9-r1-rename-policy.json"
    ))
    .unwrap();
    let golden = golden::load("w9-r1-rename-policy");
    let cases = golden
        .cases
        .iter()
        .filter(|c| c["kind"] == "single")
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 33);
    for (case, spec) in cases.into_iter().zip(input["single"].as_array().unwrap()) {
        let actual = Policy::check(
            case["typed"].as_str(),
            case["path"].as_str().unwrap(),
            spec["long_path_aware"].as_bool().unwrap_or(false),
        );
        let mut expected = (*case).clone();
        let object = expected.as_object_mut().unwrap();
        object.remove("kind");
        object.remove("path");
        object.remove("typed");
        assert_eq!(verdict(&actual), expected, "{case}");
    }
}

// C#: RenameNamePolicyTests.Check_EmptyOrOnlyExtension_ReturnsEmpty; Check_NullStem_ReturnsEmpty.
#[test]
fn check_empty_or_only_extension_returns_empty() {
    for typed in [None, Some(""), Some(" \t\r\n"), Some(".jpg"), Some(" .JPG ")] {
        let check = Policy::check(typed, r"C:\photos\a.jpg", false);
        assert_eq!(check.problem, Problem::Empty);
        assert!(check.new_file_name.is_empty());
        assert!(!check.is_valid());
    }
}

// C#: RenameNamePolicyTests.Check_LoneSurrogate_ReturnsInvalidCharacter.
#[test]
fn check_lone_surrogate_returns_invalid_character() {
    for typed in [vec![0xD800], vec![0xDC00], vec![b'a' as u16, 0xD800, b'b' as u16]] {
        let check = Policy::check_utf16(Some(&typed), r"C:\photos\a.jpg", false);
        assert_eq!(check.problem, Problem::InvalidCharacter);
        assert_eq!(
            check.invalid_character,
            Some(if typed[0] == b'a' as u16 { 0xD800 } else { typed[0] })
        );
    }
}

// C#: RenameNamePolicyTests.Check_ValidSurrogatePair_IsAccepted.
#[test]
fn check_valid_surrogate_pair_is_accepted() {
    let check = Policy::check_utf16(Some(&[0xD83D, 0xDE00]), r"C:\photos\a.JPG", false);
    assert!(check.is_valid());
    assert_eq!(check.new_file_name, "😀.JPG");
}

// C#: RenameNamePolicyTests.Check_SeveralForbiddenCharacters_ReportsFirstOne.
#[test]
fn check_several_forbidden_characters_reports_first_one() {
    for (typed, bad) in [("a?b:c", b'?' as u16), ("a:b?c", b':' as u16), ("a\u{1f}b?", 0x1f)] {
        let check = Policy::check(Some(typed), r"C:\photos\a.jpg", false);
        assert_eq!(check.problem, Problem::InvalidCharacter);
        assert_eq!(check.invalid_character, Some(bad));
    }
}

// C#: RenameNamePolicyTests.Check_NfdTypedForNfcCurrent_IsDifferentNameAndNotNormalized.
#[test]
fn check_nfd_typed_for_nfc_current_is_different_name_and_not_normalized() {
    let check = Policy::check(Some("cafe\u{301}"), "C:\\photos\\café.jpg", false);
    assert!(check.is_valid());
    assert!(!check.is_unchanged && !check.differs_only_by_case);
    assert_eq!(check.new_file_name, "cafe\u{301}.jpg");
}

// C#: RenameNamePolicyTests.Check_OnlyCaseDiffers_IsCaseOnly; Check_SameNameWithTypedExtension_IsUnchanged.
#[test]
fn check_only_case_differs_is_case_only() {
    let check = Policy::check(Some("A.jpg"), r"C:\photos\a.JPG", false);
    assert!(check.is_valid() && check.differs_only_by_case && !check.is_unchanged);
    assert_eq!(check.new_file_name, "A.JPG");
    let same = Policy::check(Some("a.jpg"), r"C:\photos\a.JPG", false);
    assert!(same.is_unchanged && !same.differs_only_by_case);
}

const CURRENT: &str = r"C:\Photos\IMG_0001.jpg";

// C#: RenameNamePolicyTests.Check_TypedStem_AppendsOriginalExtensionOnce.
#[test]
fn check_typed_stem_appends_original_extension_once() {
    for (typed, name) in [
        ("a", "a.jpg"),
        ("  a  ", "a.jpg"),
        ("\ta\t", "a.jpg"),
        ("a.jpg", "a.jpg"),
        ("A.JPG", "A.jpg"),
        ("a .jpg", "a.jpg"),
        ("a.v2", "a.v2.jpg"),
        ("a.jpg.jpg", "a.jpg.jpg"),
        ("a b", "a b.jpg"),
        ("a.png", "a.png.jpg"),
    ] {
        let check = Policy::check(Some(typed), CURRENT, false);
        assert!(check.is_valid(), "{typed}");
        assert_eq!(check.new_file_name, name);
        assert_eq!(check.invalid_character, None);
    }
}

// C#: RenameNamePolicyTests.Check_ForbiddenCharacter_ReturnsInvalidCharacterWithThatChar.
#[test]
fn check_forbidden_character_returns_invalid_character_with_that_char() {
    for bad in ['<', '>', ':', '"', '/', '\\', '|', '?', '*'] {
        let check = Policy::check(Some(&format!("a{bad}b")), CURRENT, false);
        assert_eq!(check.problem, Problem::InvalidCharacter);
        assert_eq!(check.invalid_character, Some(bad as u16));
        assert!(check.new_file_name.is_empty());
    }
}

// C#: RenameNamePolicyTests.Check_ControlCharacterInside_ReturnsInvalidCharacter.
#[test]
fn check_control_character_inside_returns_invalid_character() {
    for code in [0, 1, 8, 9, 10, 13, 27, 31] {
        let check = Policy::check_utf16(Some(&[97, code, 98]), CURRENT, false);
        assert_eq!(check.problem, Problem::InvalidCharacter);
        assert_eq!(check.invalid_character, Some(code));
    }
    // C# rejects only below U+0020; DEL and C1 controls remain legal components.
    assert!(Policy::check(Some("a\u{7f}\u{80}"), CURRENT, false).is_valid());
}

// C#: RenameNamePolicyTests.Check_TrailingDot_ReturnsTrailingDotOrSpace.
#[test]
fn check_trailing_dot_returns_trailing_dot_or_space() {
    for typed in ["a.", "a..", "a. "] {
        let check = Policy::check(Some(typed), CURRENT, false);
        assert_eq!(check.problem, Problem::TrailingDotOrSpace);
        assert!(check.new_file_name.is_empty());
    }
}

const DEVICES: &[&str] = &[
    "CON", "con", "Prn", "AUX", "nul", "COM0", "COM1", "com9", "LPT0", "lpt5", "LPT9", "COM¹", "COM²", "LPT³",
    "CONIN$", "conout$", "CON.txt", "con .txt", "NUL.v2",
];
const ORDINARY: &[&str] = &["COM10", "COM", "CONSOLE", "a.CON", "xCON", "LPT10", "COMA"];

// C#: RenameNamePolicyTests.Check_DeviceName_ReturnsReservedDeviceName.
#[test]
fn check_device_name_returns_reserved_device_name() {
    for typed in DEVICES {
        let check = Policy::check(Some(typed), CURRENT, false);
        assert_eq!(check.problem, Problem::ReservedDeviceName, "{typed}");
        assert!(check.new_file_name.is_empty());
    }
}

// C#: RenameNamePolicyTests.Check_NotADeviceName_IsAccepted.
#[test]
fn check_not_a_device_name_is_accepted() {
    for typed in ORDINARY {
        assert!(Policy::check(Some(typed), CURRENT, false).is_valid(), "{typed}");
    }
}

// C#: RenameNamePolicyTests.IsReservedDeviceName_Table.
#[test]
fn is_reserved_device_name_table() {
    for name in DEVICES
        .iter()
        .copied()
        .chain(["PRN.jpg", "com7.jpg", "COM².txt", "CONOUT$.jpg"])
    {
        assert!(Policy::is_reserved_device_name(name), "{name}");
    }
    for name in ORDINARY.iter().copied().chain([""]) {
        assert!(!Policy::is_reserved_device_name(name), "{name}");
    }
}

// C#: RenameNamePolicyTests.CheckFileName_Table.
#[test]
fn check_file_name_table() {
    for (name, problem) in [
        (Some("a.jpg"), Problem::None),
        (None, Problem::Empty),
        (Some("  "), Problem::Empty),
        (Some("a|b.jpg"), Problem::InvalidCharacter),
        (Some("a\\b.jpg"), Problem::InvalidCharacter),
        (Some("a."), Problem::TrailingDotOrSpace),
        (Some("a "), Problem::TrailingDotOrSpace),
        (Some("CON.jpg"), Problem::ReservedDeviceName),
    ] {
        assert_eq!(Policy::check_file_name(name), problem);
    }
    assert_eq!(Policy::check_file_name(Some(&"x".repeat(255))), Problem::None);
    assert_eq!(Policy::check_file_name(Some(&"x".repeat(256))), Problem::NameTooLong);
}

fn long_current(stem_length: usize) -> String {
    format!("C:\\{}\\{}.jpg", "d".repeat(197), "y".repeat(stem_length))
}

macro_rules! policy_case {
    ($name:ident, $source:literal, $typed:expr, $path:expr, $aware:expr, $problem:expr) => {
        #[doc = concat!("C#: RenameNamePolicyTests.", $source)]
        #[test]
        fn $name() {
            let check = Policy::check(Some(&$typed), &$path, $aware);
            assert_eq!(check.problem, $problem);
            if check.is_valid() {
                assert!(!check.new_file_name.is_empty());
            } else {
                assert!(check.new_file_name.is_empty());
            }
        }
    };
}
policy_case!(
    check_file_name_of255_is_accepted,
    "Check_FileNameOf255_IsAccepted",
    "x".repeat(251),
    CURRENT,
    true,
    Problem::None
);
policy_case!(
    check_file_name_of256_returns_name_too_long,
    "Check_FileNameOf256_ReturnsNameTooLong",
    "x".repeat(252),
    CURRENT,
    false,
    Problem::NameTooLong
);
policy_case!(
    check_new_path_reaches260_and_is_longer_returns_path_too_long,
    "Check_NewPathReaches260AndIsLonger_ReturnsPathTooLong",
    "x".repeat(56),
    long_current(1),
    false,
    Problem::PathTooLong
);
policy_case!(
    check_new_path_exactly259_is_accepted,
    "Check_NewPathExactly259_IsAccepted",
    "x".repeat(54),
    long_current(1),
    false,
    Problem::None
);
policy_case!(
    check_new_path_exactly260_returns_path_too_long,
    "Check_NewPathExactly260_ReturnsPathTooLong",
    "x".repeat(55),
    long_current(1),
    false,
    Problem::PathTooLong
);
policy_case!(
    check_long_current_path_and_shorter_new_name_is_accepted,
    "Check_LongCurrentPathAndShorterNewName_IsAccepted",
    "x".repeat(80),
    long_current(96),
    false,
    Problem::None
);
policy_case!(
    check_long_current_path_and_same_length_name_is_accepted,
    "Check_LongCurrentPathAndSameLengthName_IsAccepted",
    "x".repeat(96),
    long_current(96),
    false,
    Problem::None
);
policy_case!(
    check_long_path_aware_skips_path_budget,
    "Check_LongPathAware_SkipsPathBudget",
    "x".repeat(56),
    long_current(1),
    true,
    Problem::None
);

// C#: RenameNamePolicyTests.ExceedsPathBudget_Table.
#[test]
fn exceeds_path_budget_table() {
    let current = long_current(1);
    assert!(Policy::exceeds_path_budget(&current, &"x".repeat(60), false));
    assert!(!Policy::exceeds_path_budget(&current, &"x".repeat(58), false));
    assert!(!Policy::exceeds_path_budget(&current, &"x".repeat(60), true));
    assert!(!Policy::exceeds_path_budget(&current, "a.jpg", false));
    assert!(!Policy::exceeds_path_budget("a.jpg", &"x".repeat(260), false));
}

macro_rules! spelling_case {
    ($test:ident, $source:literal, $typed:literal, $path:expr, $name:literal, $unchanged:literal, $case_only:literal) => {
        #[doc = concat!("C#: RenameNamePolicyTests.", $source)]
        #[test]
        fn $test() {
            let check = Policy::check(Some($typed), $path, false);
            assert!(check.is_valid());
            assert_eq!(
                (
                    check.new_file_name.as_str(),
                    check.is_unchanged,
                    check.differs_only_by_case
                ),
                ($name, $unchanged, $case_only)
            );
        }
    };
}
spelling_case!(
    check_same_name_is_unchanged,
    "Check_SameName_IsUnchanged",
    "IMG_0001",
    CURRENT,
    "IMG_0001.jpg",
    true,
    false
);
spelling_case!(
    check_same_name_with_typed_extension_is_unchanged,
    "Check_SameNameWithTypedExtension_IsUnchanged",
    "IMG_0001.jpg",
    CURRENT,
    "IMG_0001.jpg",
    true,
    false
);
spelling_case!(
    check_extension_case_of_current_file_is_kept,
    "Check_ExtensionCaseOfCurrentFile_IsKept",
    "a.jpg",
    r"C:\Photos\IMG.JPG",
    "a.JPG",
    false,
    false
);
spelling_case!(
    check_lower_to_upper_case_only_of_a_is_case_only,
    "Check_LowerToUpperCaseOnlyOfA_IsCaseOnly",
    "A",
    r"C:\Photos\a.jpg",
    "A.jpg",
    false,
    true
);
spelling_case!(
    check_different_name_is_neither_unchanged_nor_case_only,
    "Check_DifferentName_IsNeitherUnchangedNorCaseOnly",
    "other",
    CURRENT,
    "other.jpg",
    false,
    false
);
spelling_case!(
    check_nfc_typed_for_nfd_current_is_kept_as_typed,
    "Check_NfcTypedForNfdCurrent_IsKeptAsTyped",
    "é",
    "C:\\Photos\\e\u{301}.jpg",
    "é.jpg",
    false,
    false
);

// C#: RenameNamePolicyTests.Check_NullStem_ReturnsEmpty.
#[test]
fn check_null_stem_returns_empty() {
    assert_eq!(Policy::check(None, CURRENT, false).problem, Problem::Empty);
}

#[test]
fn supplementary_characters_count_as_two_utf16_units() {
    let accepted = format!("{}x", "😀".repeat(125));
    assert_eq!(
        Policy::check(Some(&accepted), CURRENT, true)
            .new_file_name
            .encode_utf16()
            .count(),
        255
    );
    assert_eq!(
        Policy::check(Some(&"😀".repeat(126)), CURRENT, true).problem,
        Problem::NameTooLong
    );
    let current = format!("C:\\{}\\a.jpg", "😀".repeat(100));
    assert_eq!(
        Policy::check(Some(&"x".repeat(56)), &current, false).problem,
        Problem::PathTooLong
    );
}
// C#: RenameNamePolicyTests.Check_BlankCurrentPath_Throws; D-W9R-12 maps exception to verdict.
#[test]
fn check_blank_current_path_returns_empty() {
    for path in ["", " ", "\t\n"] {
        for aware in [false, true] {
            assert_eq!(Policy::check(Some("a"), path, aware).problem, Problem::Empty);
            assert!(Policy::exceeds_path_budget(path, "a.jpg", aware));
        }
    }
}

#[test]
fn path_key_map_retains_first_spelling_and_ordinal_ignore_case_semantics() {
    use pr_core::file_actions::rename::PathKeyMap;
    let mut map = PathKeyMap::new();
    assert!(map.is_empty());
    assert_eq!(map.insert(r"C:\Photos\a.JPG".into(), "first.jpg".into()), None);
    assert_eq!(
        map.insert(r"c:\PHOTOS\A.jpg".into(), "second.jpg".into()),
        Some("first.jpg".into())
    );
    assert_eq!(map.len(), 1);
    assert!(map.contains_key(r"C:\PHOTOS\a.jpg"));
    assert_eq!(map.iter().next().unwrap().0, r"C:\Photos\a.JPG");
    let other = [(r"c:\photos\A.jpg".into(), "second.jpg".into())]
        .into_iter()
        .collect::<PathKeyMap>();
    assert_eq!(map, other);
    map.insert("ß".into(), "sharp".into());
    assert!(!map.contains_key("SS"));
    map.insert("é".into(), "nfc".into());
    assert!(!map.contains_key("e\u{301}"));
}
