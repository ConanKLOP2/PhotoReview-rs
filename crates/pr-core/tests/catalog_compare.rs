//! Port of the C# `ComparePairService` tests (`tests/PhotoReview.Core.Tests/Catalog/`) plus golden checks against
//! `tests/golden/compare-pair.json` and `tests/golden/numbered-pattern.json` (generated from .NET by `tools/oracle-cs`).
//!
//! Port of `PhotoReview.Core/Catalog/ComparePairService.cs` (C# baseline d92d6aae).

use std::collections::HashSet;

use pr_core::catalog::compare::{CompareIndex, build_index, find, numbered_stem_base, numbered_suffix};
use pr_core::catalog::types::get_extension;
use pr_core::text::dotnet_path::{PathError, get_directory_name};
use pr_core::text::{eq_ignore_case, ignore_case_key};
use pr_test_support::golden::{self, str_of};
use serde_json::Value;

type Pair = (String, String);

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn pair(l: &str, r: &str) -> Pair {
    (l.to_owned(), r.to_owned())
}

fn found(files: &[String], path: &str) -> Option<Pair> {
    find(files, path).expect("find")
}

fn indexed(index: &CompareIndex, path: &str) -> Option<Pair> {
    index.get(path).cloned()
}

// C#: ComparePairServiceTests.FindMatchesNumberedVariantWithOriginal
#[test]
fn find_matches_numbered_variant_with_original() {
    let files = strings(&[
        r"C:\Photos\DSC0001.JPG",
        r"C:\Photos\DSC0001 (1).JPG",
        r"C:\Photos\DSC0002.JPG",
    ]);
    let expected = pair(r"C:\Photos\DSC0001.JPG", r"C:\Photos\DSC0001 (1).JPG");
    assert_eq!(found(&files, r"C:\Photos\DSC0001.JPG"), Some(expected.clone()));
    assert_eq!(found(&files, r"C:\Photos\DSC0001 (1).JPG"), Some(expected));
}

// C#: ComparePairServiceTests.FindReturnsNullWhenNoPairExists
#[test]
fn find_returns_none_when_no_pair_exists() {
    let files = strings(&[r"C:\Photos\DSC0001.JPG", r"C:\Photos\DSC0002.JPG"]);
    assert_eq!(found(&files, r"C:\Photos\DSC0001.JPG"), None);
}

// C#: ComparePairServiceTests.FindStaysWithinSelectedFolder
#[test]
fn find_stays_within_selected_folder() {
    let files = strings(&[
        r"C:\Photos\DSC0001.JPG",
        r"C:\Photos\DSC0001 (1).JPG",
        r"C:\Photos\Other\DSC0001.JPG",
    ]);
    assert_eq!(found(&files, r"C:\Photos\Other\DSC0001.JPG"), None);
}

// C#: ComparePairServiceTests.FindRejectsSingleFileList
#[test]
fn find_rejects_single_file_list() {
    let files = strings(&[r"C:\Photos\DSC0001.JPG"]);
    assert_eq!(found(&files, r"C:\Photos\DSC0001.JPG"), None);
}

// C#: ComparePairServiceTests.BuildIndexMatchesFindForEveryPath
#[test]
fn build_index_matches_find_for_every_path() {
    let files = strings(&[
        r"C:\Photos\DSC0001.JPG",
        r"C:\Photos\DSC0001 (1).JPG",
        r"C:\Photos\DSC0001 (2).JPG",
        r"C:\Photos\DSC0002.JPG",
        r"C:\Photos\Other\DSC0001.JPG",
        r"C:\Photos\DSC0003.PNG",
        r"C:\Photos\DSC0003 (1).PNG",
    ]);
    let index = build_index(&files).unwrap();
    for path in &files {
        assert_eq!(indexed(&index, path), found(&files, path), "{path}");
    }
    assert!(!index.is_empty());
}

// C#: ComparePairServiceTests.BuildIndexPairsOriginalWithSmallestNumbered
#[test]
fn build_index_pairs_original_with_smallest_numbered() {
    let files = strings(&[
        r"C:\Photos\DSC0001.JPG",
        r"C:\Photos\DSC0001 (2).JPG",
        r"C:\Photos\DSC0001 (1).JPG",
    ]);
    let index = build_index(&files).unwrap();
    assert_eq!(
        index.get(r"C:\Photos\DSC0001.JPG").unwrap().1,
        r"C:\Photos\DSC0001 (1).JPG"
    );
    let from_numbered = index.get(r"C:\Photos\DSC0001 (2).JPG").unwrap();
    assert_eq!(from_numbered.0, r"C:\Photos\DSC0001.JPG");
    assert_eq!(from_numbered.1, r"C:\Photos\DSC0001 (2).JPG");
}

// C#: ComparePairServiceTests.BuildIndexOmitsUnpairedOriginal
#[test]
fn build_index_omits_unpaired_original() {
    let index = build_index(&strings(&[r"C:\Photos\DSC0001.JPG", r"C:\Photos\DSC0002.JPG"])).unwrap();
    assert!(index.is_empty());
    assert_eq!(index.len(), 0);
}

// C#: ComparePairServiceTests.BuildIndexMatchesFindWithMixedCase (theory, 2 rows)
#[test]
fn build_index_matches_find_with_mixed_case() {
    for (original, variant) in [
        (r"C:\Photos\DSC0001.JPG", r"C:\Photos\DSC0001 (1).jpg"),
        (r"C:\Photos\DSC0001.JPG", r"c:\photos\DSC0001 (1).JPG"),
    ] {
        let files = strings(&[original, variant]);
        let expected = found(&files, original);
        assert!(expected.is_some(), "{variant}");
        let index = build_index(&files).unwrap();
        assert_eq!(indexed(&index, original), expected, "{variant}");
    }
}

/// Deterministic xorshift64 RNG (no `rand` crate).
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

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() >> 11) as usize % n
    }
}

fn fuzz_one(seed: u64) {
    const STEMS: [&str; 12] = [
        "IMG",
        "img",
        "DSC0001",
        "photo",
        "a (1)",
        "x (2) (3)",
        "日本",
        "İ",
        "ı",
        "(1)",
        " (1)",
        "b",
    ];
    const SUFFIXES: [&str; 11] = [
        "",
        "",
        " (1)",
        " (2)",
        " (10)",
        " (0)",
        " (99999999999)",
        "(3)",
        " ( 1)",
        " (１)",
        " (٣)",
    ];
    const EXTENSIONS: [&str; 6] = [".jpg", ".JPG", ".jpeg", ".png", ".Png", ""];
    const FOLDERS: [&str; 4] = [r"C:\Photos", r"c:\photos", r"C:\Photos\Sub", r"D:\Other"];

    let mut r = XorShift::new(seed);
    for _round in 0..60 {
        let count = 1 + r.below(39);
        let mut seen = HashSet::new();
        let mut files = Vec::new();
        for _ in 0..count {
            let f = format!(
                "{}\\{}{}{}",
                FOLDERS[r.below(FOLDERS.len())],
                STEMS[r.below(STEMS.len())],
                SUFFIXES[r.below(SUFFIXES.len())],
                EXTENSIONS[r.below(EXTENSIONS.len())]
            );
            if seen.insert(ignore_case_key(&f)) {
                files.push(f);
            }
        }
        let index = build_index(&files).unwrap();
        for file in &files {
            let f = found(&files, file);
            let i = indexed(&index, file);
            assert_eq!(f, i, "{file}\n{}", files.join("\n"));
            if let Some((left, right)) = f {
                let dir = |p: &str| get_directory_name(p).map(|d| ignore_case_key(&d));
                assert_eq!(dir(&left), dir(&right));
                assert_eq!(
                    ignore_case_key(get_extension(&left)),
                    ignore_case_key(get_extension(&right))
                );
                assert!(files.contains(&left));
                assert!(files.contains(&right));
                assert!(!eq_ignore_case(&left, &right));
            }
        }
    }
}

// C#: ComparePairFuzzTests.BuildIndex_MatchesFind (theory seeds 1-5)
#[test]
fn build_index_matches_find_fuzz() {
    for seed in 1..=5 {
        fuzz_one(seed);
    }
}

// C#: ComparePairTieOrderTests.BuildIndex_SpellingsOfOneFullPath_KeepInputOrder
#[test]
fn build_index_spellings_of_one_full_path_keep_input_order() {
    let mut files = Vec::new();
    let mut first_spellings = Vec::new();
    for i in 0..40 {
        let plain = format!(r"C:\Photos\s{i:02}.jpg");
        let dotted = format!(r"C:\Photos\.\s{i:02}.jpg"); // GetFullPath turns it into the same full path
        let (first, second) = if i % 2 == 0 { (plain, dotted) } else { (dotted, plain) };
        files.push(first.clone());
        files.push(second);
        files.push(format!(r"C:\Photos\s{i:02} (1).jpg"));
        first_spellings.push(first);
    }
    let index = build_index(&files).unwrap();
    for (i, first) in first_spellings.iter().enumerate() {
        let numbered = format!(r"C:\Photos\s{i:02} (1).jpg");
        let got = indexed(&index, &numbered).unwrap_or_else(|| panic!("{numbered}"));
        assert_eq!(got, (first.clone(), numbered.clone()));
        assert_eq!(found(&files, &numbered), Some(got));
    }
}

// C#: ComparePairTieOrderTests.BuildIndex_PathsDifferingOnlyByCase_OrdinalOrderPicksTheOriginal (theory)
#[test]
fn build_index_paths_differing_only_by_case_ordinal_order_picks_the_original() {
    for lower_case_listed_first in [false, true] {
        let lower = r"C:\Photos\shot.jpg";
        let upper = r"C:\Photos\SHOT.jpg";
        let numbered = r"C:\Photos\shot (1).jpg";
        let files = if lower_case_listed_first {
            strings(&[lower, upper, numbered])
        } else {
            strings(&[upper, lower, numbered])
        };
        let index = build_index(&files).unwrap();
        let got = indexed(&index, numbered).unwrap();
        assert_eq!(got, pair(upper, numbered)); // "S" sorts before "s" ordinally
        assert_eq!(found(&files, numbered), Some(got));
    }
}

// C#: SortAndPairMutationGapTests.BuildIndex_NumberedVariantsInPathOrderDifferentFromNumericOrder_PicksTheLowestNumber
#[test]
fn build_index_numbered_variants_in_path_order_different_from_numeric_order_picks_the_lowest() {
    let files = strings(&[r"C:\p\a.jpg", r"C:\p\a (10).jpg", r"C:\p\a (2).jpg", r"C:\p\a (3).jpg"]);
    let index = build_index(&files).unwrap();
    let from_find = found(&files, r"C:\p\a.jpg");
    assert_eq!(
        indexed(&index, r"C:\p\a.jpg"),
        Some(pair(r"C:\p\a.jpg", r"C:\p\a (2).jpg"))
    );
    assert_eq!(from_find, indexed(&index, r"C:\p\a.jpg"));
}

// C#: SortAndPairMutationGapTests.BuildIndex_VariantsWithTheSameNumber_KeepTheirPathOrder
#[test]
fn build_index_variants_with_the_same_number_keep_their_path_order() {
    let files = strings(&[r"C:\p\a.jpg", r"C:\p\a (1).jpg", r"C:\p\a (01).jpg"]);
    let index = build_index(&files).unwrap();
    assert_eq!(index.get(r"C:\p\a.jpg").unwrap().1, r"C:\p\a (01).jpg");
    assert_eq!(found(&files, r"C:\p\a.jpg"), indexed(&index, r"C:\p\a.jpg"));
}

// C#: SortAndPairMutationGapTests.BuildIndex_ADriveRootEntry_DoesNotThrow
#[test]
fn build_index_a_drive_root_entry_does_not_throw() {
    assert!(build_index(&strings(&[r"C:\", r"C:\a.jpg"])).is_ok());
}

// C#: CompareAndCaptureGapTests.BuildIndex_NestedNumbering_EqualsFindForEveryPath
#[test]
fn build_index_nested_numbering_equals_find_for_every_path() {
    let files = strings(&[
        r"C:\Photos\a.JPG",
        r"C:\Photos\a (1).JPG",
        r"C:\Photos\a (1) (2).JPG",
        r"C:\Photos\a (2).JPG",
    ]);
    let index = build_index(&files).unwrap();
    for path in &files {
        assert_eq!(indexed(&index, path), found(&files, path), "{path}");
    }
    // "a (1)" is the original of "a (1) (2)"
    assert_eq!(index.get(r"C:\Photos\a (1) (2).JPG").unwrap().0, r"C:\Photos\a (1).JPG");
}

#[test]
fn index_keys_are_ordinal_ignore_case() {
    let index = build_index(&strings(&[r"C:\Photos\DSC0001.JPG", r"C:\Photos\DSC0001 (1).JPG"])).unwrap();
    assert!(index.get(r"c:\photos\dsc0001.jpg").is_some());
    assert_eq!(index.iter().count(), index.len());
}

#[test]
fn find_reports_invalid_path() {
    let files = strings(&[r"C:\p\a.jpg", "C:\\p\\a\0.jpg"]);
    assert_eq!(find(&files, r"C:\p\a.jpg"), Err(PathError::Argument));
    assert_eq!(build_index(&files), Err(PathError::Argument));
}

// ---- goldens ----

fn pair_of(v: &Value) -> Result<Option<Pair>, String> {
    if v.is_null() {
        return Ok(None);
    }
    if let Some(t) = v.get("throws") {
        return Err(t.as_str().unwrap().to_owned());
    }
    let a = v.as_array().expect("pair array");
    Ok(Some((
        a[0].as_str().unwrap().to_owned(),
        a[1].as_str().unwrap().to_owned(),
    )))
}

fn files_of(case: &Value) -> Vec<String> {
    case["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn compare_pair_golden() {
    let g = golden::load("compare-pair");
    assert_eq!(g.cases.len(), 34);
    for case in &g.cases {
        let name = str_of(case, "name");
        let files = files_of(case);
        for f in case["finds"].as_array().unwrap() {
            let query = str_of(f, "query");
            let got = find(&files, query);
            match pair_of(&f["pair"]) {
                Ok(expected) => assert_eq!(got.unwrap(), expected, "{name}: {query}"),
                Err(_) => assert!(got.is_err(), "{name}: {query} should throw"),
            }
        }
        let index = build_index(&files);
        if let Some(t) = case["index"].get("throws") {
            assert!(index.is_err(), "{name}: index should throw {t}");
            continue;
        }
        let index = index.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let entries = case["index"].as_array().unwrap();
        assert_eq!(index.len(), entries.len(), "{name}: index.len()");
        for e in entries {
            let key = str_of(e, "key");
            let got = index.get(key).unwrap_or_else(|| panic!("{name}: missing key {key}"));
            assert_eq!(got.0, str_of(e, "left"), "{name}: {key}");
            assert_eq!(got.1, str_of(e, "right"), "{name}: {key}");
        }
        for (key, _) in index.iter() {
            assert!(
                entries.iter().any(|e| str_of(e, "key") == key),
                "{name}: unexpected key {key}"
            );
        }
        if let Some(agrees) = case["index_find_agrees"].as_array() {
            for a in agrees {
                let file = str_of(a, "file");
                let agree = indexed(&index, file) == find(&files, file).unwrap();
                assert_eq!(agree, a["agrees"].as_bool().unwrap(), "{name}: {file}");
            }
        }
    }
}

#[test]
fn numbered_pattern_golden_stems() {
    let g = golden::load("numbered-pattern");
    let mut stems = 0;
    for case in &g.cases {
        let Some(stem) = case.get("stem").and_then(Value::as_str) else {
            continue;
        };
        stems += 1;
        let base = numbered_stem_base(stem);
        assert_eq!(base, case["stem_base"].as_str(), "stem_base of {stem:?}");
        assert_eq!(
            i64::from(numbered_suffix(stem)),
            case["suffix"].as_i64().unwrap(),
            "suffix of {stem:?}"
        );
        assert_eq!(
            pr_core::file_actions::duplicates::has_numbered_suffix(stem),
            case["duplicate_pattern"].as_bool().unwrap(),
            "duplicate pattern of {stem:?}"
        );
    }
    assert_eq!(stems, 43);
}

#[test]
fn numbered_stem_base_examples() {
    assert_eq!(numbered_stem_base("a (1)"), Some("a"));
    assert_eq!(numbered_stem_base(" (1)"), Some(""));
    assert_eq!(numbered_stem_base("a (1) (2)"), Some("a (1)"));
    assert_eq!(numbered_stem_base("a (𝟏)"), None);
    assert_eq!(numbered_suffix("a (2147483647)"), i32::MAX);
    assert_eq!(numbered_suffix("a(1)"), 1);
}
