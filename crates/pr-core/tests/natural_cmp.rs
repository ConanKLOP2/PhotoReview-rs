//! Port of the C# `ManagedNaturalComparer` tests (`tests/PhotoReview.Core.Tests/Catalog/`) plus golden checks against
//! `tests/golden/natural-cmp.json` and `tests/golden/natural-key.json` (generated from .NET by `tools/oracle-cs`).
//!
//! Port of `PhotoReview.Core/Catalog/ManagedNaturalComparer.cs` (C# baseline e441d67a).

use std::cmp::Ordering;

use pr_core::abstractions::NaturalComparer;
use pr_core::catalog::natural_cmp::{
    ManagedNaturalComparer, build_natural_key, compare_natural, is_dotnet_digit, lower_invariant,
};
use pr_core::text::cmp_ignore_case;
use pr_test_support::golden::{self, i64_of, str_of};
use serde_json::Value;

fn sign(o: Ordering) -> i64 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// Deterministic xorshift64 RNG (no `rand` crate). The seed is mixed so that small seeds never reach the all-zero state.
struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform-ish value in `0..n` (`n > 0`).
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// The original (pre-optimisation) natural-key builder, the oracle the optimised one must match
/// (C# `NaturalKeyReference.BuildNaturalKey`). It uses the same digit and lower-casing primitives as the port, which are
/// pinned separately by the golden files; only the structure (builder vs. buffer) is under test here.
fn reference_build_natural_key(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if is_dotnet_digit(chars[i]) {
            let start = i;
            while i < chars.len() && is_dotnet_digit(chars[i]) {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            let digits = run.trim_start_matches('0');
            out.push_str(&format!("{:010}", digits.chars().count()));
            out.push_str(digits);
            out.push('\0');
        } else {
            out.push(lower_invariant(chars[i]));
            i += 1;
        }
    }
    out
}

/// The original comparer (C# `NaturalKeyReference.Compare`), for non-null names.
fn reference_compare(x: &str, y: &str) -> Ordering {
    if x == y {
        return Ordering::Equal;
    }
    let c = cmp_ignore_case(&reference_build_natural_key(x), &reference_build_natural_key(y));
    if c != Ordering::Equal {
        return c;
    }
    let c = cmp_ignore_case(x, y);
    if c != Ordering::Equal {
        return c;
    }
    x.cmp(y)
}

/// Digits, letters in both cases, separators, Arabic-Indic and fullwidth digits, a non-BMP character
/// (the C# alphabet has a lone surrogate pair here; a Rust `&str` cannot hold one, so U+1F600 stands in).
const EQUIVALENCE_ALPHABET: &[char] = &[
    '0',
    '0',
    '0',
    '1',
    '2',
    '9',
    '٣',
    '٠',
    '１',
    'a',
    'A',
    'b',
    'Z',
    'i',
    'I',
    ' ',
    '(',
    ')',
    '.',
    '_',
    'É',
    'é',
    '\u{1F600}',
];

fn random_name(rng: &mut XorShift, max_length: usize) -> String {
    let len = rng.below(max_length + 1);
    (0..len)
        .map(|_| EQUIVALENCE_ALPHABET[rng.below(EQUIVALENCE_ALPHABET.len())])
        .collect()
}

/// Alphabet of the C# `NaturalSortPropertyTests` (multi-char entries included).
const PROPERTY_ALPHABET: &[&str] = &[
    "a",
    "B",
    "c",
    "İ",
    "ı",
    "i",
    "I",
    "ß",
    "SS",
    "é",
    "e\u{301}",
    "日",
    "本",
    "م",
    "ر",
    "0",
    "00",
    "1",
    "2",
    "9",
    "10",
    "007",
    "٣",
    "３",
    "_",
    "-",
    " ",
    "(",
    ")",
    ".",
    "\u{1F600}",
    "Z",
    "z",
    "~",
];

fn random_property_name(rng: &mut XorShift) -> String {
    let parts = rng.below(6) + 1;
    let mut s: String = (0..parts)
        .map(|_| PROPERTY_ALPHABET[rng.below(PROPERTY_ALPHABET.len())])
        .collect();
    match rng.below(3) {
        0 => s.push_str(".jpg"),
        1 if rng.below(2) == 0 => s.push_str(".JPG"),
        _ => {}
    }
    s
}

fn opt_str(v: &Value) -> Option<&str> {
    if v.is_null() {
        None
    } else {
        Some(v.as_str().unwrap_or_else(|| panic!("not a string or null: {v}")))
    }
}

// C#: ManagedNaturalComparerTests.CompareCorrectlyOrdersNumericSequences (theory, 9 rows)
#[test]
fn compare_correctly_orders_numeric_sequences() {
    let rows: &[(&str, &str, i64)] = &[
        ("a2", "a10", -1),
        ("a10", "a2", 1),
        ("a2", "a2", 0),
        ("img1.jpg", "img2.jpg", -1),
        ("img2.jpg", "img10.jpg", -1),
        ("img10.jpg", "img100.jpg", -1),
        ("img2.jpg", "img1000000000000.jpg", -1),
        ("a (1)", "a (2)", -1),
        ("a (2)", "a (10)", -1),
    ];
    for &(x, y, expected) in rows {
        assert_eq!(
            sign(ManagedNaturalComparer::INSTANCE.compare(Some(x), Some(y))),
            expected,
            "compare {x:?} {y:?}"
        );
    }
}

// C#: ManagedNaturalComparerTests.CompareHandlesCaseInsensitivity
#[test]
fn compare_handles_case_insensitivity() {
    let c = ManagedNaturalComparer::INSTANCE;
    assert_eq!(c.compare(Some("photo.jpg"), Some("photo.jpg")), Ordering::Equal);
    // Case-difference should sort stably
    assert_ne!(c.compare(Some("PHOTO.jpg"), Some("photo.jpg")), Ordering::Equal);
}

// C#: ManagedNaturalComparerTests.CompareHandlesNulls
#[test]
fn compare_handles_nulls() {
    let c = ManagedNaturalComparer::INSTANCE;
    assert_eq!(c.compare(None, None), Ordering::Equal);
    assert_eq!(c.compare(None, Some("any")), Ordering::Less);
    assert_eq!(c.compare(Some("any"), None), Ordering::Greater);
}

// C#: ManagedNaturalComparerTests.SortListMatchesNaturalOrder
#[test]
fn sort_list_matches_natural_order() {
    let mut input = vec!["img10.jpg", "img2.jpg", "img1.jpg", "img100.jpg", "img20.jpg"];
    input.sort_by(|x, y| compare_natural(x, y));
    let expected = vec!["img1.jpg", "img2.jpg", "img10.jpg", "img20.jpg", "img100.jpg"];
    assert_eq!(input, expected);
}

// C#: ManagedNaturalComparerEquivalenceTests.MatchesOriginal_OnRandomInput
#[test]
fn matches_original_on_random_input() {
    let mut rng = XorShift::new(20_260_926);
    for i in 0..20_000 {
        // long names exercise the pooled-buffer path of the C# code (> 256 key chars)
        let max_length = if i % 50 == 0 { 400 } else { 24 };
        let a = random_name(&mut rng, max_length);
        let b = if i % 7 == 0 {
            a.to_uppercase()
        } else {
            random_name(&mut rng, max_length)
        };

        assert_eq!(reference_build_natural_key(&a), build_natural_key(&a), "key of {a:?}");
        assert_eq!(
            sign(compare_natural(&a, &b)),
            sign(reference_compare(&a, &b)),
            "compare {a:?} {b:?}"
        );
    }
}

// C#: ManagedNaturalComparerEquivalenceTests.EdgeKeys_MatchOriginal (theory, 6 rows)
#[test]
fn edge_keys_match_original() {
    let names: &[&str] = &["", "0", "000", "a0b00c", "007.jpg", "12345678901234567890"];
    for name in names {
        assert_eq!(
            reference_build_natural_key(name),
            build_natural_key(name),
            "key of {name:?}"
        );
    }
}

/// A strict total order: antisymmetric, transitive, zero only for identical strings.
fn comparer_is_total_order(seed: u64) {
    let mut rng = XorShift::new(seed);
    let names: Vec<String> = (0..220).map(|_| random_property_name(&mut rng)).collect();

    for a in &names {
        assert_eq!(compare_natural(a, a), Ordering::Equal);
        for b in &names {
            let ab = sign(compare_natural(a, b));
            assert_eq!(-ab, sign(compare_natural(b, a)), "antisymmetry {a:?} {b:?}");
            if ab == 0 {
                assert_eq!(a, b);
            }
        }
    }

    for _ in 0..20_000 {
        let a = &names[rng.below(names.len())];
        let b = &names[rng.below(names.len())];
        let d = &names[rng.below(names.len())];
        if compare_natural(a, b) != Ordering::Greater && compare_natural(b, d) != Ordering::Greater {
            assert!(
                compare_natural(a, d) != Ordering::Greater,
                "{a} <= {b} <= {d} but {a} > {d}"
            );
        }
    }
}

// C#: NaturalSortPropertyTests.Comparer_IsTotalOrder (theory, seed 1)
#[test]
fn comparer_is_total_order_seed_1() {
    comparer_is_total_order(1);
}

// C#: NaturalSortPropertyTests.Comparer_IsTotalOrder (theory, seed 2)
#[test]
fn comparer_is_total_order_seed_2() {
    comparer_is_total_order(2);
}

// C#: NaturalSortPropertyTests.Comparer_IsTotalOrder (theory, seed 3)
#[test]
fn comparer_is_total_order_seed_3() {
    comparer_is_total_order(3);
}

// C#: NaturalSortPropertyTests.Comparer_IsTotalOrder (theory, seed 4)
#[test]
fn comparer_is_total_order_seed_4() {
    comparer_is_total_order(4);
}

// C#: NaturalSortPropertyTests.Comparer_TurkishI_Consistent
// (no culture switch in Rust: the C# assertions are kept as they are)
#[test]
fn comparer_turkish_i_consistent() {
    let c = ManagedNaturalComparer::INSTANCE;
    let turkish_sample = ["i2.jpg", "I10.jpg", "İ 1.jpg", "ı 3.jpg", "i1.jpg"];

    assert_eq!(c.compare(Some("photo.jpg"), Some("photo.jpg")), Ordering::Equal);
    assert_ne!(c.compare(Some("PHOTO.jpg"), Some("photo.jpg")), Ordering::Equal);
    assert_eq!(
        sign(c.compare(Some("I.jpg"), Some("\u{131}.jpg"))),
        -sign(c.compare(Some("\u{131}.jpg"), Some("I.jpg")))
    );
    assert_ne!(c.compare(Some("\u{130}.jpg"), Some("i.jpg")), Ordering::Equal);

    let mut sorted: Vec<&str> = turkish_sample.to_vec();
    sorted.sort_by(|x, y| c.compare(Some(x), Some(y)));
    let i_names: Vec<&str> = sorted
        .into_iter()
        .filter(|x| matches!(x.chars().next(), Some('i') | Some('I')))
        .collect();
    assert_eq!(i_names, vec!["i1.jpg", "i2.jpg", "I10.jpg"]);
}

// C#: NaturalSortPropertyTests.Comparer_LeadingZeros
#[test]
fn comparer_leading_zeros() {
    let c = ManagedNaturalComparer::INSTANCE;
    let zero_sample = ["a1", "a01", "a001", "a0", "a00", "a2", "a10", "a010"];
    let mut list: Vec<&str> = zero_sample.to_vec();
    list.sort_by(|x, y| c.compare(Some(x), Some(y)));

    let mut again = list.clone();
    again.sort_by(|x, y| c.compare(Some(x), Some(y)));
    assert_eq!(list, again);

    let pos = |s: &str| {
        list.iter()
            .position(|x| *x == s)
            .unwrap_or_else(|| panic!("{s} missing"))
    };
    assert!(pos("a0") < pos("a1") && pos("a00") < pos("a1"));
    assert!(pos("a2") < pos("a10") && pos("a2") < pos("a010"));
}

// C#: NaturalSortPropertyTests.Comparer_HugeDigitRuns
#[test]
fn comparer_huge_digit_runs() {
    let c = ManagedNaturalComparer::INSTANCE;
    let big = "9".repeat(5000);
    let bigger = format!("1{}", "0".repeat(5000));

    assert_eq!(
        compare_natural(&format!("f{big}.jpg"), &format!("f{bigger}.jpg")),
        Ordering::Less
    );
    assert_eq!(compare_natural(&big, &bigger), Ordering::Less);
    assert_eq!(compare_natural("", ""), Ordering::Equal);
    assert_eq!(compare_natural("", "a"), Ordering::Less);
    assert_eq!(c.compare(Some(""), Some("")), Ordering::Equal);
}

// C#: SortAndPairMutationGapTests.Compare_NamesDifferingOnlyInCaseAndLeadingZeros_OrderByTheCaseInsensitiveTextBeforeTheOrdinalTie
#[test]
fn compare_names_differing_only_in_case_and_leading_zeros_order_by_the_case_insensitive_text_before_the_ordinal_tie() {
    let c = ManagedNaturalComparer::INSTANCE;

    // "a01" and "A1" have the same natural key; the case-insensitive text ("a01" < "a1") decides, not the ordinal 'A' < 'a'.
    assert_eq!(c.compare(Some("a01"), Some("A1")), Ordering::Less);
    assert_eq!(c.compare(Some("A1"), Some("a01")), Ordering::Greater);
    // Same text apart from case: the ordinal tie-break puts the upper-case spelling first.
    assert_eq!(c.compare(Some("A1"), Some("a1")), Ordering::Less);
    assert_eq!(c.compare(Some("a1"), Some("a1")), Ordering::Equal);
}

// C#: SortAndPairMutationGapTests.BuildNaturalKey_NullOrEmpty_IsEmpty (only the "" half: null does not exist in Rust)
#[test]
fn build_natural_key_null_or_empty_is_empty() {
    assert_eq!(build_natural_key(""), "");
}

// C#: ImageSortServiceTests.NaturalKeyBuildsExpectedOrder
#[test]
fn natural_key_builds_expected_order() {
    let key1 = build_natural_key("photo2.jpg");
    let key2 = build_natural_key("photo10.jpg");
    assert_eq!(cmp_ignore_case(&key1, &key2), Ordering::Less);
}

// C#: ManagedNaturalComparer.Compare / ManagedNaturalComparer.BuildNaturalKey, pinned by tests/golden/natural-cmp.json
// (94 pair cases with null handling, 7 sort cases).
#[test]
fn golden_natural_cmp_pairs_and_sorts_match_dotnet() {
    let g = golden::load("natural-cmp");
    let c = ManagedNaturalComparer::INSTANCE;
    for case in &g.cases {
        match str_of(case, "kind") {
            "pair" => {
                let (a, b) = (opt_str(&case["a"]), opt_str(&case["b"]));
                assert_eq!(sign(c.compare(a, b)), i64_of(case, "compare"), "pair {a:?} {b:?}");
            }
            "sort" => {
                let input: Vec<&str> = case["input"]
                    .as_array()
                    .unwrap_or_else(|| panic!("sort case has no input: {case}"))
                    .iter()
                    .map(|v| opt_str(v).unwrap_or_else(|| panic!("null in sort input: {case}")))
                    .collect();
                let expected: Vec<&str> = case["sorted"]
                    .as_array()
                    .unwrap_or_else(|| panic!("sort case has no sorted: {case}"))
                    .iter()
                    .map(|v| opt_str(v).unwrap_or_else(|| panic!("null in sorted: {case}")))
                    .collect();
                let mut sorted = input.clone();
                sorted.sort_by(|x, y| compare_natural(x, y));
                assert_eq!(sorted, expected, "sort of {input:?}");
            }
            other => panic!("unknown natural-cmp case kind {other:?}"),
        }
    }
}

// C#: ManagedNaturalComparer.BuildNaturalKey, pinned by tests/golden/natural-key.json (33 exact key strings).
#[test]
fn golden_natural_key_matches_dotnet() {
    let g = golden::load("natural-key");
    for case in &g.cases {
        let name = str_of(case, "name");
        assert_eq!(build_natural_key(name), str_of(case, "key"), "key of {name:?}");
    }
}
