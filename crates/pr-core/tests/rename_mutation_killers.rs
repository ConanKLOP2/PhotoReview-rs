//! W9-RENAME-MUTATION-2 killers for cargo-mutants survivors in `file_actions/rename.rs` (`PathKeyMap`) and
//! `file_actions/case_only_rename.rs`. Each test names the mutant it kills. Fakes only; paths come from `TempRoot`.
use std::path::Path;

use pr_core::abstractions::FileSystem;
use pr_core::file_actions::case_only_rename::CaseOnlyRename;
use pr_core::file_actions::rename::PathKeyMap;
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeFileSystem, TempRoot};

fn map(pairs: &[(&str, &str)]) -> PathKeyMap {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

// Mutant: rename.rs:22 `PathKeyMap::is_empty -> true`.
#[test]
fn path_key_map_is_empty_only_before_the_first_insert() {
    let mut keys = PathKeyMap::new();
    assert!(keys.is_empty());
    keys.insert("a".into(), "b".into());
    assert!(!keys.is_empty());
    assert_eq!(keys.len(), 1);
}

// Mutants: rename.rs:63 `<impl PartialEq for PathKeyMap>::eq -> true` and `&& -> ||` (length and content both matter).
#[test]
fn path_key_map_equality_needs_equal_length_and_equal_entries() {
    let one = map(&[("a", "1")]);
    let two = map(&[("a", "1"), ("b", "2")]);
    assert_ne!(one, two, "a subset is not equal (length differs)");
    assert_ne!(two, one);
    assert_ne!(map(&[("a", "1")]), map(&[("a", "2")]), "same key, different value");
    assert_ne!(map(&[("a", "1")]), map(&[("b", "1")]), "same length, different key");
    assert_eq!(map(&[("A", "1")]), map(&[("a", "1")]), "keys compare ignoring case");
    assert_eq!(PathKeyMap::new(), PathKeyMap::new());
}

fn rig(name: &str) -> (TempRoot, FakeFileSystem, String) {
    let root = TempRoot::new(name);
    let fs = FakeFileSystem::new();
    let path = root.join("a.jpg").to_string_lossy().into_owned();
    fs.add_file_at(&path, b"x", DateTimeUtc::MIN);
    (root, fs, path)
}

// Mutant: case_only_rename.rs:24 `stat_as`: `stat.is_none() || !is_case_only_pair` -> `&&`. For a stored file with no
// case-only twin the stat is returned without asking for the stored spelling (an empty listing must not hide it).
#[test]
fn stat_as_without_a_case_only_twin_returns_the_stat_without_consulting_the_listing() {
    let (_root, fs, path) = rig("killer-stat-as");
    fs.set_stored_file_names_override(Some(Vec::new()));
    let stat = CaseOnlyRename::stat_as(&fs, &path, None).expect("stat");
    assert!(stat.is_some(), "no twin: the stat stands");
    let stat = CaseOnlyRename::stat_as(&fs, &path, Some("unrelated.png")).expect("stat");
    assert!(stat.is_some(), "an unrelated twin is not a case-only pair");
}

// Mutant: case_only_rename.rs:75 `is_stored_as`: `stored.len() > 1` -> `<`. Two stored spellings (a case-sensitive
// folder) mean both names exist, so the exact spelling is present.
#[test]
fn stat_as_with_two_stored_spellings_keeps_the_stat_for_either_name() {
    let (root, fs, path) = rig("killer-two-spellings");
    let twin = root.join("A.jpg").to_string_lossy().into_owned();
    fs.set_stored_file_names_override(Some(vec!["A.jpg".into(), "a.jpg".into()]));
    assert!(
        CaseOnlyRename::stat_as(&fs, &path, Some(&twin))
            .expect("stat")
            .is_some()
    );
    assert!(CaseOnlyRename::exists_as(&fs, &path, Some(&twin)).expect("exists"));
    assert!(fs.file_exists(Path::new(&path)));
}

// Mutant: rename.rs:271 `CaptureGroupRenamePlan::build`: `owner == jpeg stem || owner == raw stem` -> `&&`.
// A sidecar owned by only ONE of the two stems must still map to the new stem.
#[test]
fn capture_group_plan_maps_a_sidecar_owned_by_either_stem_alone() {
    use pr_core::catalog::types::CaptureGroup;
    use pr_core::file_actions::rename::{CaptureGroupRenamePlan, RenameNameProblem};

    // The sidecar belongs to the JPEG stem only (the RAW has another stem).
    let jpeg_owned = CaptureGroup::new(r"C:\p\a.jpg", r"C:\p\b.cr2", Some(r"C:\p\a.xmp".to_owned())).expect("group");
    let plan = CaptureGroupRenamePlan::build(&jpeg_owned, Some("pic"), true);
    assert_eq!(plan.check.problem, RenameNameProblem::None);
    let names = plan.file_names.expect("names");
    assert_eq!(names.get(r"C:\p\a.xmp").map(String::as_str), Some("pic.xmp"));

    // The sidecar belongs to the RAW stem only.
    let raw_owned = CaptureGroup::new(r"C:\p\a.jpg", r"C:\p\b.cr2", Some(r"C:\p\b.xmp".to_owned())).expect("group");
    let plan = CaptureGroupRenamePlan::build(&raw_owned, Some("pic"), true);
    assert_eq!(plan.check.problem, RenameNameProblem::None);
    let names = plan.file_names.expect("names");
    assert_eq!(names.get(r"C:\p\b.xmp").map(String::as_str), Some("pic.xmp"));
}
