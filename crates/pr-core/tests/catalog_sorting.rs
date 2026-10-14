//! Tests for `pr_core::catalog::sorting` (C# `ImageSortService` and `SiblingFolderService`, baseline d92d6aae), plus the
//! golden replays of `tests/golden/{image-sort-entries,image-sort-paths,sibling-folders}.json`.

use std::cmp::Ordering;
use std::path::Path;

use pr_core::abstractions::NaturalComparer;
use pr_core::catalog::natural_cmp::ManagedNaturalComparer;
use pr_core::catalog::sorting::{
    get_sorted_siblings, parse_sort_mode, sort_entries, sort_paths, sort_paths_by_mode_name, sort_sibling_folders,
};
use pr_core::catalog::types::CatalogEntry;
use pr_core::model::ImageSortMode;
use pr_core::text::dotnet_path::{get_file_name, get_full_path, trim_ending_directory_separator};
use pr_core::time::DateTimeUtc;
use pr_test_support::golden::{self, str_of};
use pr_test_support::{FakeFileSystem, LogLevel, RecordingLog, TempRoot};
use serde_json::Value;

const ALL_MODES: [ImageSortMode; 6] = [
    ImageSortMode::Name,
    ImageSortMode::SizeDescending,
    ImageSortMode::SizeAscending,
    ImageSortMode::Default,
    ImageSortMode::NameAscending,
    ImageSortMode::NameDescending,
];

/// C# `ReverseOrdinalComparer`: `string.CompareOrdinal(y, x)`.
struct ReverseOrdinalComparer;

impl NaturalComparer for ReverseOrdinalComparer {
    fn compare(&self, a: Option<&str>, b: Option<&str>) -> Ordering {
        b.cmp(&a)
    }
}

/// C# `OpaqueComparer`: the managed comparison behind a different type, so the generic path is taken.
struct OpaqueComparer;

impl NaturalComparer for OpaqueComparer {
    fn compare(&self, a: Option<&str>, b: Option<&str>) -> Ordering {
        ManagedNaturalComparer::INSTANCE.compare(a, b)
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

    /// Value in `0..n` (`n > 0`).
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

const ALPHABET: [&str; 34] = [
    "a",
    "B",
    "c",
    "\u{130}",
    "\u{131}",
    "i",
    "I",
    "\u{DF}",
    "SS",
    "\u{E9}",
    "e\u{301}",
    "\u{65E5}",
    "\u{672C}",
    "\u{645}",
    "\u{631}",
    "0",
    "00",
    "1",
    "2",
    "9",
    "10",
    "007",
    "\u{663}",
    "\u{FF13}",
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

/// C# `NaturalSortPropertyTests.RandomName`.
fn random_name(r: &mut XorShift) -> String {
    let parts = 1 + r.below(6);
    let mut s = String::new();
    for _ in 0..parts {
        s.push_str(ALPHABET[r.below(ALPHABET.len())]);
    }
    if r.below(3) == 0 {
        s.push_str(".jpg");
    } else if r.below(2) == 0 {
        s.push_str(".JPG");
    }
    s
}

fn entry(path: &str, length: Option<i64>) -> CatalogEntry {
    let mut e = CatalogEntry::new(path).expect("non-blank test path");
    e.length = length;
    e
}

fn paths_of(entries: &[CatalogEntry]) -> Vec<String> {
    entries.iter().map(|e| e.path().to_owned()).collect()
}

fn names_of(paths: &[String]) -> Vec<String> {
    paths.iter().map(|p| get_file_name(p).to_owned()).collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A fake file system holding `C:\f\<name>` files of the given sizes.
fn fixture() -> (FakeFileSystem, Vec<String>) {
    let fs = FakeFileSystem::new();
    let mut files = Vec::new();
    for (name, size) in [("img10.jpg", 300), ("img2.jpg", 100), ("img1.jpg", 200)] {
        let path = format!("C:\\f\\{name}");
        fs.add_file(&path, &vec![0u8; size]);
        files.push(path);
    }
    (fs, files)
}

fn out_of_order_entries() -> Vec<CatalogEntry> {
    vec![
        entry("C:\\f\\img10.jpg", Some(50)),
        entry("C:\\f\\IMG2.jpg", Some(300)),
        entry("C:\\f\\b.jpg", Some(10)),
        entry("C:\\f\\img2.jpg", Some(300)),
        entry("C:\\f\\A.jpg", Some(999)),
        entry("C:\\f\\img10.jpg", Some(60)),
    ]
}

// ---------------------------------------------------------------- ImageSortServiceTests

#[test]
fn natural_filename_sort_orders_numeric_suffixes() {
    // C#: ImageSortServiceTests.NaturalFilenameSortOrdersNumericSuffixes
    let (fs, files) = fixture();
    let sorted = sort_paths_by_mode_name(&files, Some("Name"), None, &fs, None);
    assert_eq!(names_of(&sorted), ["img1.jpg", "img2.jpg", "img10.jpg"]);
}

#[test]
fn natural_filename_sort_handles_long_numeric_runs() {
    // C#: ImageSortServiceTests.NaturalFilenameSortHandlesLongNumericRuns
    let fs = FakeFileSystem::new();
    let files = strings(&["img1000000000000.jpg", "img2.jpg", "img10.jpg"]);
    let sorted = sort_paths_by_mode_name(&files, Some("Name"), None, &fs, None);
    assert_eq!(names_of(&sorted), ["img2.jpg", "img10.jpg", "img1000000000000.jpg"]);
}

#[test]
fn size_sort_orders_files_by_descending_bytes() {
    // C#: ImageSortServiceTests.SizeSortOrdersFilesByDescendingBytes
    let (fs, files) = fixture();
    let sorted = sort_paths_by_mode_name(&files, Some("Size"), None, &fs, None);
    assert_eq!(names_of(&sorted), ["img10.jpg", "img1.jpg", "img2.jpg"]);
}

#[test]
fn size_sort_orders_files_by_ascending_bytes() {
    // C#: ImageSortServiceTests.SizeSortOrdersFilesByAscendingBytes
    let (fs, files) = fixture();
    let sorted = sort_paths_by_mode_name(&files, Some("SizeAscending"), None, &fs, None);
    assert_eq!(names_of(&sorted), ["img2.jpg", "img1.jpg", "img10.jpg"]);
}

#[test]
fn sort_entries_uses_scanned_metadata_and_keeps_entry_metadata_attached() {
    // C#: ImageSortServiceTests.SortEntries_UsesScannedMetadataAndKeepsEntryMetadataAttached
    let (_, files) = fixture();
    let day = |d: i64| DateTimeUtc::from_ticks(d * 864_000_000_000).expect("valid ticks");
    let mut entries: Vec<CatalogEntry> = files.iter().map(|p| entry(p, None)).collect();
    entries[0].length = Some(300);
    entries[0].last_write_utc = Some(day(1));
    entries[1].length = Some(100);
    entries[1].last_write_utc = Some(day(2));
    entries[2].length = Some(200);
    entries[2].last_write_utc = Some(day(3));

    let sorted = sort_entries(&entries, ImageSortMode::SizeAscending, None);

    assert_eq!(
        paths_of(&sorted),
        [files[1].clone(), files[2].clone(), files[0].clone()]
    );
    assert_eq!(sorted[0].last_write_utc, Some(day(2)));
    assert_eq!(sorted[0].length, Some(100));
}

#[test]
fn default_sort_entries_returns_input_order_including_duplicates() {
    // C#: ImageSortServiceTests.Default_SortEntries_ReturnsInputOrderIncludingDuplicates
    let input = out_of_order_entries();
    let result = sort_entries(&input, ImageSortMode::Default, None);
    assert_eq!(paths_of(&result), paths_of(&input));
    assert_eq!(result, input);
    assert!(sort_entries(&[], ImageSortMode::Default, None).is_empty());
}

#[test]
fn default_legacy_sort_returns_input_order() {
    // C#: ImageSortServiceTests.Default_LegacySort_ReturnsInputOrder
    let fs = FakeFileSystem::new();
    let input = strings(&[
        "C:\\f\\img10.jpg",
        "C:\\f\\B.jpg",
        "C:\\f\\img2.jpg",
        "C:\\f\\a.jpg",
        "C:\\f\\img2.jpg",
    ]);
    assert_eq!(sort_paths(&input, ImageSortMode::Default, None, &fs, None), input);
    assert_eq!(sort_paths_by_mode_name(&input, Some("Default"), None, &fs, None), input);
    assert!(sort_paths(&[], ImageSortMode::Default, None, &fs, None).is_empty());
}

#[test]
fn name_ascending_equals_name_mode_and_is_natural_case_insensitive() {
    // C#: ImageSortServiceTests.NameAscending_EqualsNameMode_AndIsNaturalCaseInsensitive
    let fs = FakeFileSystem::new();
    let input = out_of_order_entries();
    let name = paths_of(&sort_entries(&input, ImageSortMode::Name, None));
    assert_eq!(
        name,
        paths_of(&sort_entries(&input, ImageSortMode::NameAscending, None))
    );
    assert_eq!(get_file_name(&name[0]), "A.jpg");
    assert_eq!(get_file_name(&name[1]), "b.jpg");
    assert_eq!(get_file_name(&name[name.len() - 1]), "img10.jpg");
    let paths = paths_of(&input);
    assert_eq!(
        sort_paths(&paths, ImageSortMode::Name, None, &fs, None),
        sort_paths(&paths, ImageSortMode::NameAscending, None, &fs, None)
    );
}

#[test]
fn name_descending_is_exact_reverse_of_ascending_including_ties() {
    // C#: ImageSortServiceTests.NameDescending_IsExactReverseOfAscending_IncludingTies
    let fs = FakeFileSystem::new();
    let input = out_of_order_entries();
    let asc = sort_entries(&input, ImageSortMode::NameAscending, None);
    let desc = sort_entries(&input, ImageSortMode::NameDescending, None);
    let mut reversed = asc.clone();
    reversed.reverse();
    // Whole entries: the two img10.jpg entries differ by length, so the tie reversal is visible.
    assert_eq!(reversed, desc);
    assert_eq!(get_file_name(desc[0].path()), "img10.jpg");

    let paths = paths_of(&input);
    let mut asc_paths = sort_paths(&paths, ImageSortMode::NameAscending, None, &fs, None);
    asc_paths.reverse();
    assert_eq!(
        asc_paths,
        sort_paths(&paths, ImageSortMode::NameDescending, None, &fs, None)
    );
    assert_eq!(
        asc_paths,
        sort_paths_by_mode_name(&paths, Some("NameDescending"), None, &fs, None)
    );
    assert!(sort_entries(&[], ImageSortMode::NameDescending, None).is_empty());
}

// ---------------------------------------------------------------- NaturalSortPropertyTests

#[test]
fn sort_is_shuffle_invariant() {
    // C#: NaturalSortPropertyTests.Sort_IsShuffleAndCultureInvariant (one run: Rust has no culture)
    let fs = FakeFileSystem::new();
    let mut r = XorShift::new(5);
    let mut names: Vec<String> = Vec::new();
    for _ in 0..400 {
        let n = random_name(&mut r);
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let full = |n: &String| format!("C:\\f\\{n}");
    let input: Vec<String> = names.iter().map(full).collect();
    let reference = sort_paths(&input, ImageSortMode::Name, None, &fs, None);
    for _ in 0..5 {
        let mut shuffled = input.clone();
        for i in (1..shuffled.len()).rev() {
            let j = r.below(i + 1);
            shuffled.swap(i, j);
        }
        assert_eq!(sort_paths(&shuffled, ImageSortMode::Name, None, &fs, None), reference);
    }
}

#[test]
fn sort_fast_path_matches_generic_comparer_path() {
    // C#: NaturalSortPropertyTests.Sort_FastPathMatchesGenericComparerPath
    let fs = FakeFileSystem::new();
    for seed in 1..=3u64 {
        let mut r = XorShift::new(seed);
        let entries: Vec<CatalogEntry> = (0..600)
            .map(|_| {
                let folder = r.below(3);
                let name = random_name(&mut r);
                let len = r.below(6) as i64;
                entry(&format!("C:\\f{folder}\\{name}"), Some(len))
            })
            .collect();
        let paths = paths_of(&entries);
        for mode in ALL_MODES {
            assert_eq!(
                sort_entries(&entries, mode, Some(&OpaqueComparer)),
                sort_entries(&entries, mode, None),
                "seed {seed} mode {mode:?}"
            );
            if mode == ImageSortMode::Name {
                assert_eq!(
                    sort_paths(&paths, mode, Some(&OpaqueComparer), &fs, None),
                    sort_paths(&paths, mode, None, &fs, None)
                );
            }
        }
    }
}

#[test]
fn sort_numeric_runs_by_value() {
    // C#: NaturalSortPropertyTests.Sort_NumericRunsByValue (BigInteger replaced by (digit count, digit text))
    let fs = FakeFileSystem::new();
    for seed in 1..=3u64 {
        let mut r = XorShift::new(seed);
        let mut numbers: Vec<String> = Vec::new();
        for _ in 0..300 {
            let mut text = (1 + r.next_u64() % (i64::MAX as u64)).to_string();
            if r.below(4) == 0 {
                text.push_str(&"0".repeat(1 + r.below(29)));
            }
            if !numbers.contains(&text) {
                numbers.push(text);
            }
        }
        let mut shuffled: Vec<String> = numbers.iter().map(|n| format!("C:\\f\\IMG_{n}.jpg")).collect();
        for i in (1..shuffled.len()).rev() {
            let j = r.below(i + 1);
            shuffled.swap(i, j);
        }
        let sorted = sort_paths(&shuffled, ImageSortMode::Name, None, &fs, None);
        let got: Vec<String> = sorted
            .iter()
            .map(|p| {
                get_file_name(p)
                    .trim_start_matches("IMG_")
                    .trim_end_matches(".jpg")
                    .to_owned()
            })
            .collect();
        let mut expected = numbers.clone();
        expected.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
        assert_eq!(got, expected, "seed {seed}");
    }
}

#[test]
fn sort_entries_size_modes_consistent() {
    // C#: NaturalSortPropertyTests.SortEntries_SizeModes_Consistent
    let sized = vec![
        entry("C:\\f\\b10.jpg", Some(100)),
        entry("C:\\f\\b2.jpg", Some(100)),
        entry("C:\\f\\a.jpg", Some(5)),
        entry("C:\\f\\unknown.jpg", None),
    ];
    let names = |mode| names_of(&paths_of(&sort_entries(&sized, mode, None)));
    assert_eq!(
        names(ImageSortMode::SizeAscending),
        ["unknown.jpg", "a.jpg", "b2.jpg", "b10.jpg"]
    );
    assert_eq!(
        names(ImageSortMode::SizeDescending),
        ["b2.jpg", "b10.jpg", "a.jpg", "unknown.jpg"]
    );
}

// ---------------------------------------------------------------- SortAndPairMutationGapTests

#[test]
fn sort_entries_injected_comparer_is_used_for_name_order() {
    // C#: SortAndPairMutationGapTests.SortEntries_InjectedComparer_IsUsedForNameOrder
    let entries = vec![
        entry("C:\\a.jpg", None),
        entry("C:\\c.jpg", None),
        entry("C:\\b.jpg", None),
    ];
    let sorted = sort_entries(&entries, ImageSortMode::Name, Some(&ReverseOrdinalComparer));
    assert_eq!(paths_of(&sorted), ["C:\\c.jpg", "C:\\b.jpg", "C:\\a.jpg"]);
}

#[test]
fn sort_paths_injected_comparer_is_used_for_name_order() {
    // C#: SortAndPairMutationGapTests.SortPaths_InjectedComparer_IsUsedForNameOrder
    let fs = FakeFileSystem::new();
    let files = strings(&["C:\\a.jpg", "C:\\c.jpg", "C:\\b.jpg"]);
    let sorted = sort_paths(&files, ImageSortMode::Name, Some(&ReverseOrdinalComparer), &fs, None);
    assert_eq!(sorted, ["C:\\c.jpg", "C:\\b.jpg", "C:\\a.jpg"]);
}

#[test]
fn sort_paths_unreadable_size_is_logged_and_sorts_as_smallest() {
    // C#: SortAndPairMutationGapTests.SortPaths_UnreadableSize_IsLoggedAndSortsAsSmallest
    let fs = FakeFileSystem::new();
    let log = RecordingLog::new();
    let big = "C:\\sort-gap\\big.jpg".to_owned();
    let missing = "C:\\sort-gap\\missing.jpg".to_owned();
    fs.add_file(&big, &[1, 2, 3, 4, 5]);

    let descending = sort_paths(
        &[missing.clone(), big.clone()],
        ImageSortMode::SizeDescending,
        None,
        &fs,
        Some(&log),
    );
    let ascending = sort_paths(
        &[big.clone(), missing.clone()],
        ImageSortMode::SizeAscending,
        None,
        &fs,
        Some(&log),
    );

    assert_eq!(descending, [big.clone(), missing.clone()]);
    assert_eq!(ascending, [missing.clone(), big]);
    let entries = log.entries();
    assert_eq!(entries.len(), 2);
    for (level, message) in entries {
        assert_eq!(level, LogLevel::Error);
        assert!(message.contains(&missing), "{message}");
        assert!(message.starts_with("Failed to read file size for "), "{message}");
    }
}

#[test]
fn sizes_are_not_read_for_default_and_name_modes() {
    // Spec note 3 of contracts/W2-05.md (C# reads sizes only in the two size modes).
    let fs = FakeFileSystem::new();
    let log = RecordingLog::new();
    let files = strings(&["C:\\x\\missing.jpg", "C:\\x\\a.jpg"]);
    for mode in [
        ImageSortMode::Default,
        ImageSortMode::Name,
        ImageSortMode::NameAscending,
        ImageSortMode::NameDescending,
    ] {
        sort_paths(&files, mode, None, &fs, Some(&log));
    }
    assert!(log.entries().is_empty());
}

#[test]
fn parse_sort_mode_follows_to_upper_invariant() {
    // Spec note 5 of contracts/W2-05.md (quirk verified with .NET 10).
    assert_eq!(parse_sort_mode(Some("\u{17F}ize")), ImageSortMode::SizeDescending);
    assert_eq!(parse_sort_mode(Some("s\u{131}ze")), ImageSortMode::Name);
    assert_eq!(parse_sort_mode(Some(" size")), ImageSortMode::Name);
    assert_eq!(parse_sort_mode(Some("Size")), ImageSortMode::SizeDescending);
    assert_eq!(parse_sort_mode(None), ImageSortMode::Name);
    assert_eq!(parse_sort_mode(Some("PortraitFirst")), ImageSortMode::Name);
    assert_eq!(parse_sort_mode(Some("nameDescending")), ImageSortMode::NameDescending);
}

// ---------------------------------------------------------------- goldens: ImageSortService

fn mode_from_name(name: &str) -> ImageSortMode {
    match name {
        "Name" => ImageSortMode::Name,
        "SizeDescending" => ImageSortMode::SizeDescending,
        "SizeAscending" => ImageSortMode::SizeAscending,
        "Default" => ImageSortMode::Default,
        "NameAscending" => ImageSortMode::NameAscending,
        "NameDescending" => ImageSortMode::NameDescending,
        other => panic!("unknown mode {other}"),
    }
}

fn comparer_named(name: Option<&str>) -> Option<Box<dyn NaturalComparer>> {
    match name {
        None => None,
        Some("reverse-ordinal") => Some(Box::new(ReverseOrdinalComparer)),
        Some("opaque") => Some(Box::new(OpaqueComparer)),
        Some(other) => panic!("unknown comparer {other}"),
    }
}

#[test]
fn golden_image_sort_entries() {
    let g = golden::load("image-sort-entries");
    assert_eq!(g.cases.len(), 21);
    for case in &g.cases {
        let name = str_of(case, "name");
        let mode = mode_from_name(str_of(case, "mode"));
        let comparer = comparer_named(case["comparer"].as_str());
        let read = |key: &str| -> Vec<CatalogEntry> {
            case[key]
                .as_array()
                .unwrap_or_else(|| panic!("{name}: no {key}"))
                .iter()
                .map(|e| entry(str_of(e, "path"), e["length"].as_i64()))
                .collect()
        };
        let entries = read("entries");
        let expected = read("sorted");
        let sorted = sort_entries(&entries, mode, comparer.as_deref());
        assert_eq!(sorted, expected, "case {name}");
    }
}

#[test]
fn golden_image_sort_paths() {
    let g = golden::load("image-sort-paths");
    assert_eq!(g.cases.len(), 34);
    let inputs_path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("image-sort-paths.json");
    let text = std::fs::read_to_string(&inputs_path).expect("read golden inputs");
    let inputs: Value = serde_json::from_str(&text).expect("parse golden inputs");
    let input_cases = inputs["cases"].as_array().expect("input cases");
    assert_eq!(input_cases.len(), g.cases.len());

    for (case, input) in g.cases.iter().zip(input_cases) {
        let name = str_of(case, "name");
        assert_eq!(str_of(input, "name"), name);
        let root = "C:\\oracle-sort";
        let fs = FakeFileSystem::new();
        let mut files = Vec::new();
        for f in input["files"].as_array().expect("files") {
            let fname = str_of(f, "name");
            let path = format!("{root}\\{fname}");
            if let Some(size) = f["size"].as_u64() {
                fs.add_file(&path, &vec![0u8; size as usize]);
            }
            files.push(path);
        }
        let comparer = comparer_named(input["comparer"].as_str());
        let log = RecordingLog::new();
        let sorted = if input.get("mode_text").is_some() {
            sort_paths_by_mode_name(
                &files,
                input["mode_text"].as_str(),
                comparer.as_deref(),
                &fs,
                Some(&log),
            )
        } else {
            sort_paths(
                &files,
                mode_from_name(str_of(input, "mode")),
                comparer.as_deref(),
                &fs,
                Some(&log),
            )
        };
        let expected: Vec<&str> = case["sorted"]
            .as_array()
            .expect("sorted")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let got: Vec<&str> = sorted.iter().map(|p| get_file_name(p)).collect();
        assert_eq!(got, expected, "case {name}");

        let failed: Vec<&str> = case["failed_size_reads"]
            .as_array()
            .expect("failed")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let entries = log.entries();
        assert_eq!(entries.len(), failed.len(), "case {name}: logged errors");
        for ((level, message), fname) in entries.iter().zip(&failed) {
            assert_eq!(*level, LogLevel::Error, "case {name}");
            assert!(message.contains(&format!("{root}\\{fname}")), "case {name}: {message}");
            assert!(message.starts_with("Failed to read file size for "), "case {name}");
        }
    }
}

// ---------------------------------------------------------------- SiblingFolderService

fn sibling_names(paths: &[String]) -> Vec<String> {
    names_of(paths)
}

/// Pure twin of the three native C# `SiblingFolderServiceTests` and of the golden cases that list folders.
fn pure_siblings(root: &str, visible: &[&str], current: &str) -> Vec<String> {
    let listed: Vec<String> = visible.iter().map(|n| format!("{root}\\{n}")).collect();
    let full = get_full_path(&format!("{root}\\{current}")).expect("valid path");
    sort_sibling_folders(trim_ending_directory_separator(&full), listed)
}

#[test]
fn get_sorted_returns_siblings_in_natural_order_pure() {
    // C#: SiblingFolderServiceTests.GetSortedReturnsSiblingsInNaturalOrder (pure twin)
    let result = pure_siblings("C:\\root", &["Album 10", "Album 1", "Album 2"], "Album 1");
    assert_eq!(sibling_names(&result), ["Album 1", "Album 2", "Album 10"]);
    assert_eq!(result[0], "C:\\root\\Album 1");
}

#[test]
fn get_sorted_hidden_and_system_siblings_are_not_returned_pure() {
    // C#: SiblingFolderServiceTests.GetSorted_HiddenAndSystemSiblings_AreNotReturned (pure twin: the listing passed
    // to the pure step is already filtered, the filter itself is covered by the native test)
    let result = pure_siblings("C:\\root", &["Album 1", "Album 2", "Album 10"], "Album 1");
    assert_eq!(sibling_names(&result), ["Album 1", "Album 2", "Album 10"]);
}

#[test]
fn get_sorted_current_folder_is_hidden_still_returned_among_visible_siblings_pure() {
    // C#: SiblingFolderServiceTests.GetSorted_CurrentFolderIsHidden_StillReturnedAmongVisibleSiblings (pure twin)
    let result = pure_siblings("C:\\root", &["Album 1", "Album 2", "Album 10"], "Album 5 hidden");
    assert_eq!(
        sibling_names(&result),
        ["Album 1", "Album 2", "Album 5 hidden", "Album 10"]
    );
}

fn attrib(flag: &str, path: &Path) {
    let status = std::process::Command::new("attrib")
        .arg(flag)
        .arg(path)
        .status()
        .expect("run attrib");
    assert!(status.success(), "attrib {flag} {}", path.display());
}

fn make_dirs(root: &TempRoot, names: &[&str]) {
    for n in names {
        std::fs::create_dir_all(root.join(n)).expect("create dir");
    }
}

#[test]
#[ignore = "native: real directory listing and Hidden/System attributes"]
fn get_sorted_returns_siblings_in_natural_order() {
    // C#: SiblingFolderServiceTests.GetSortedReturnsSiblingsInNaturalOrder
    let root = TempRoot::new("sibling");
    make_dirs(&root, &["Album 1", "Album 2", "Album 10"]);
    let current = root.join("Album 1");
    let sorted = get_sorted_siblings(&current.to_string_lossy(), None).expect("path");
    assert_eq!(sibling_names(&sorted), ["Album 1", "Album 2", "Album 10"]);
}

#[test]
#[ignore = "native: real directory listing and Hidden/System attributes"]
fn get_sorted_hidden_and_system_siblings_are_not_returned() {
    // C#: SiblingFolderServiceTests.GetSorted_HiddenAndSystemSiblings_AreNotReturned
    let root = TempRoot::new("sibling");
    make_dirs(
        &root,
        &["Album 1", "Album 2", "Album 10", ".thumbnails", "Album 3 system"],
    );
    attrib("+h", &root.join(".thumbnails"));
    attrib("+s", &root.join("Album 3 system"));
    let current = root.join("Album 1");
    let sorted = get_sorted_siblings(&current.to_string_lossy(), None).expect("path");
    assert_eq!(sibling_names(&sorted), ["Album 1", "Album 2", "Album 10"]);
}

#[test]
#[ignore = "native: real directory listing and Hidden/System attributes"]
fn get_sorted_current_folder_is_hidden_still_returned_among_visible_siblings() {
    // C#: SiblingFolderServiceTests.GetSorted_CurrentFolderIsHidden_StillReturnedAmongVisibleSiblings
    let root = TempRoot::new("sibling");
    make_dirs(
        &root,
        &["Album 1", "Album 2", "Album 10", "Album 5 hidden", "Album 6 hidden"],
    );
    attrib("+h", &root.join("Album 5 hidden"));
    attrib("+h", &root.join("Album 6 hidden"));
    let current = root.join("Album 5 hidden");
    let sorted = get_sorted_siblings(&current.to_string_lossy(), None).expect("path");
    assert_eq!(
        sibling_names(&sorted),
        ["Album 1", "Album 2", "Album 5 hidden", "Album 10"]
    );
}

#[test]
fn sibling_folders_parent_cannot_be_listed_returns_empty_and_logs_the_error() {
    // C#: CatalogMutationGapTests.SiblingFolders_ParentCannotBeListed_ReturnsEmptyAndLogsTheError
    let root = TempRoot::new("sibling-gap");
    let log = RecordingLog::new();
    let parent = root.join("missing-parent");
    let current = parent.join("child");

    let siblings = get_sorted_siblings(&current.to_string_lossy(), Some(&log)).expect("path");

    assert!(siblings.is_empty());
    let entries = log.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, LogLevel::Error);
    // The temp root may be spelled with an 8.3 short name (CI: RUNNER~1) while the log shows the long one: compare the
    // tail that is stable under that expansion.
    let tail = format!(
        "{}\\missing-parent",
        root.path().file_name().expect("temp root name").to_string_lossy()
    );
    assert!(entries[0].1.contains(&tail), "{}", entries[0].1);
    assert!(
        entries[0].1.starts_with("Failed to enumerate sibling folders of "),
        "{}",
        entries[0].1
    );
}

#[test]
fn sibling_folders_parent_cannot_be_listed_and_no_log_given_still_returns_empty() {
    // C#: CatalogMutationGapTests.SiblingFolders_ParentCannotBeListedAndNoLogGiven_StillReturnsEmpty
    let root = TempRoot::new("sibling-gap");
    let current = root.join("missing-parent").join("child");
    let siblings = get_sorted_siblings(&current.to_string_lossy(), None).expect("path");
    assert!(siblings.is_empty());
}

#[test]
fn sibling_folders_of_a_drive_root_are_empty() {
    // C# `Directory.GetParent` is null for a root: no listing, no log.
    let log = RecordingLog::new();
    let siblings = get_sorted_siblings("C:\\", Some(&log)).expect("path");
    assert!(siblings.is_empty());
    assert!(log.entries().is_empty());
}

// ---------------------------------------------------------------- golden: SiblingFolderService

fn golden_folders(case: &Value) -> Vec<(String, Option<String>)> {
    case["folders"]
        .as_array()
        .expect("folders")
        .iter()
        .map(|f| (str_of(f, "name").to_owned(), f["attr"].as_str().map(str::to_owned)))
        .collect()
}

fn expected_siblings(case: &Value) -> Vec<String> {
    case["siblings"]
        .as_array()
        .expect("siblings")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

#[test]
fn golden_sibling_folders_pure() {
    let g = golden::load("sibling-folders");
    assert_eq!(g.cases.len(), 12);
    for case in &g.cases {
        let name = str_of(case, "name");
        if case["logged_errors"].as_i64() != Some(0) || name.starts_with("missing-parent") {
            continue; // listing failures: run against the real file system below
        }
        let folders = golden_folders(case);
        let visible: Vec<&str> = folders
            .iter()
            .filter(|(_, attr)| attr.is_none())
            .map(|(n, _)| n.as_str())
            .collect();
        let result = pure_siblings("C:\\root", &visible, str_of(case, "current"));
        assert_eq!(sibling_names(&result), expected_siblings(case), "case {name}");
    }
}

#[test]
fn golden_sibling_folders_listing_failures() {
    let g = golden::load("sibling-folders");
    let mut seen = 0;
    for case in &g.cases {
        let name = str_of(case, "name");
        if !name.starts_with("missing-parent") {
            continue;
        }
        seen += 1;
        let root = TempRoot::new("sibling-golden");
        let current = root.join(str_of(case, "current"));
        let with_log = case["logged_errors"].as_i64() == Some(1);
        let log = RecordingLog::new();
        let result =
            get_sorted_siblings(&current.to_string_lossy(), if with_log { Some(&log) } else { None }).expect("path");
        assert!(result.is_empty(), "case {name}");
        assert_eq!(log.entries().len(), usize::from(with_log), "case {name}");
    }
    assert_eq!(seen, 2);
}

#[test]
#[ignore = "native"]
fn golden_sibling_folders_native() {
    let g = golden::load("sibling-folders");
    for case in &g.cases {
        let name = str_of(case, "name");
        let root = TempRoot::new("sibling-golden");
        for (folder, attr) in golden_folders(case) {
            let path = root.join(&folder);
            std::fs::create_dir_all(&path).expect("create dir");
            match attr.as_deref() {
                Some("hidden") => attrib("+h", &path),
                Some("system") => attrib("+s", &path),
                _ => {}
            }
        }
        let current = root.join(str_of(case, "current"));
        let expect_error = case["logged_errors"].as_i64() == Some(1);
        let log = RecordingLog::new();
        let log_arg: Option<&dyn pr_core::abstractions::Log> = if name.contains("without-a-log") {
            None
        } else {
            Some(&log)
        };
        let result = get_sorted_siblings(&current.to_string_lossy(), log_arg).expect("path");
        assert_eq!(sibling_names(&result), expected_siblings(case), "case {name}");
        assert_eq!(log.entries().len(), usize::from(expect_error), "case {name}");
    }
}
