//! Port of the C# `DuplicateFinder` tests (`tests/PhotoReview.Core.Tests/FileActions/`) plus the golden check against
//! `tests/golden/duplicates.json` (generated from .NET by `tools/oracle-cs`).
//!
//! Port of `PhotoReview.Core/FileActions/DuplicateFinder.cs` (C# baseline d92d6aae).

use std::cell::{Cell, RefCell};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use pr_core::abstractions::{FileStat, FileSystem, ReadSeek};
use pr_core::file_actions::duplicates::{HashFailure, find, has_numbered_suffix};
use pr_core::sync::{CancelToken, Canceled};
use pr_test_support::golden;
use pr_test_support::{FakeFileSystem, FsOp};
use serde_json::Value;

fn s(items: &[&str]) -> Vec<String> {
    items.iter().map(|x| (*x).to_owned()).collect()
}

fn fs_with(files: &[(&str, &str)]) -> FakeFileSystem {
    let fs = FakeFileSystem::new();
    for (path, text) in files {
        fs.add_file(path, text.as_bytes());
    }
    fs
}

fn run(
    files: &[&str],
    remove_numbered: bool,
    fs: &FakeFileSystem,
    mut hash: impl FnMut(&str) -> Result<String, HashFailure>,
) -> Result<Vec<String>, Canceled> {
    find(
        &s(files),
        remove_numbered,
        &mut |p: &str, _c: &CancelToken| hash(p),
        fs,
        &CancelToken::none(),
    )
}

fn constant(h: &'static str) -> impl FnMut(&str) -> Result<String, HashFailure> {
    move |_| Ok(h.to_owned())
}

// C#: DuplicateFinderTests.FindAsync_WhenFilesEmpty_ReturnsEmpty
#[test]
fn find_when_files_empty_returns_empty() {
    let fs = FakeFileSystem::new();
    assert_eq!(run(&[], true, &fs, constant("dummy")).unwrap(), Vec::<String>::new());
}

// C#: DuplicateFinderTests.FindAsync_HashThrowsOperationCanceledWithoutTokenCancellation_PropagatesIt
#[test]
fn find_hash_canceled_without_token_cancellation_propagates_it() {
    let fs = fs_with(&[(r"C:\photos\img.jpg", "content"), (r"C:\photos\img (1).jpg", "content")]);
    let r = run(&[r"C:\photos\img.jpg", r"C:\photos\img (1).jpg"], true, &fs, |_| {
        Err(HashFailure::Canceled)
    });
    assert_eq!(r, Err(Canceled));
}

// C#: DuplicateFinderTests.FindAsync_SamePathListedTwiceOrRecased_IsNotItsOwnDuplicate
#[test]
fn find_same_path_listed_twice_or_recased_is_not_its_own_duplicate() {
    let fs = fs_with(&[(r"C:\photos\img (1).jpg", "content")]);
    let f = r"C:\photos\img (1).jpg";
    let upper = f.to_uppercase();
    let r = run(&[f, f, &upper], true, &fs, constant("same")).unwrap();
    assert!(r.is_empty());
}

// C#: DuplicateFinderTests.FindAsync_RemoveNumbered_SelectsOnlyNumberedCopies
#[test]
fn find_remove_numbered_selects_only_numbered_copies() {
    let names = [r"C:\photos\img.jpg", r"C:\photos\img (1).jpg", r"C:\photos\img (2).jpg"];
    let fs = fs_with(&names.map(|n| (n, "identical-content")));
    let r = run(&names, true, &fs, constant("hash-abc")).unwrap();
    assert_eq!(r, s(&[names[1], names[2]]));
}

// C#: DuplicateFinderTests.FindAsync_RemoveOriginal_SelectsOnlyNonNumberedOriginals
#[test]
fn find_remove_original_selects_only_non_numbered_originals() {
    let names = [r"C:\photos\img.jpg", r"C:\photos\img (1).jpg"];
    let fs = fs_with(&names.map(|n| (n, "identical-content")));
    let r = run(&names, false, &fs, constant("hash-abc")).unwrap();
    assert_eq!(r, s(&[names[0]]));
}

// C#: DuplicateFinderTests.FindAsync_RemoveNumbered_WhenGroupContainsOnlyNumberedCopies_KeepsOneSurvivor
#[test]
fn find_remove_numbered_when_group_contains_only_numbered_copies_keeps_one_survivor() {
    let names = [
        r"C:\photos\img (1).jpg",
        r"C:\photos\img (2).jpg",
        r"C:\photos\img (3).jpg",
    ];
    let fs = fs_with(&names.map(|n| (n, "identical-content")));
    let r = run(&names, true, &fs, constant("hash-abc")).unwrap();
    assert_eq!(r, s(&[names[1], names[2]]));
}

// C#: DuplicateFinderTests.FindAsync_RemoveOriginal_WhenGroupContainsOnlyOriginals_KeepsOneSurvivor
#[test]
fn find_remove_original_when_group_contains_only_originals_keeps_one_survivor() {
    let names = [r"C:\photos\img-a.jpg", r"C:\photos\img-b.jpg", r"C:\photos\img-c.jpg"];
    let fs = fs_with(&names.map(|n| (n, "identical-content")));
    let r = run(&names, false, &fs, constant("hash-abc")).unwrap();
    assert_eq!(r, s(&[names[1], names[2]]));
}

// C#: DuplicateFinderTests.FindAsync_DifferentSizes_SkipsHashComputation
#[test]
fn find_different_sizes_skips_hash_computation() {
    let fs = fs_with(&[
        (r"C:\photos\small.jpg", "small"),
        (r"C:\photos\large.jpg", "large large large"),
    ]);
    let called = Cell::new(false);
    let r = run(&[r"C:\photos\small.jpg", r"C:\photos\large.jpg"], true, &fs, |_| {
        called.set(true);
        Ok("hash".into())
    })
    .unwrap();
    assert!(!called.get());
    assert!(r.is_empty());
}

// C#: DuplicateFinderTests.FindAsync_SameSizeDifferentHash_NotTreatedAsDuplicate
#[test]
fn find_same_size_different_hash_not_treated_as_duplicate() {
    let fs = fs_with(&[(r"C:\photos\a.jpg", "12345"), (r"C:\photos\a (1).jpg", "abcde")]);
    let r = run(&[r"C:\photos\a.jpg", r"C:\photos\a (1).jpg"], true, &fs, |p| {
        Ok(if p.contains("(1)") { "hash-b" } else { "hash-a" }.to_owned())
    })
    .unwrap();
    assert!(r.is_empty());
}

// C#: DuplicateFinderTests.FindAsync_FileErrorDuringHash_IgnoresAndContinues
#[test]
fn find_file_error_during_hash_ignores_and_continues() {
    let names = [r"C:\photos\a.jpg", r"C:\photos\a (1).jpg", r"C:\photos\a (2).jpg"];
    let fs = fs_with(&names.map(|n| (n, "content")));
    let r = run(&names, true, &fs, |p| {
        if p.to_lowercase().ends_with("a (1).jpg") {
            Err(HashFailure::Unreadable(io::Error::other("File disappeared")))
        } else {
            Ok("hash-same".into())
        }
    })
    .unwrap();
    assert_eq!(r, s(&[names[2]]));
}

// C#: DuplicateFinderTests.FindAsync_WhenCancelled_ThrowsOperationCanceledException
#[test]
fn find_when_cancelled_returns_canceled() {
    let fs = fs_with(&[(r"C:\photos\a.jpg", "content"), (r"C:\photos\a (1).jpg", "content")]);
    let token = CancelToken::new();
    token.cancel();
    let r = find(
        &s(&[r"C:\photos\a.jpg", r"C:\photos\a (1).jpg"]),
        true,
        &mut |_p: &str, _c: &CancelToken| Ok("hash".to_owned()),
        &fs,
        &token,
    );
    assert_eq!(r, Err(Canceled));
}

// C#: DuplicateFinderTests.FindAsync_HashThrowsArgumentException_SkipsThatFileAndKeepsTheRest
#[test]
fn find_hash_invalid_path_skips_that_file_and_keeps_the_rest() {
    let names = [r"C:\photos\img.jpg", r"C:\photos\img (1).jpg", r"C:\photos\bad (1).jpg"];
    let fs = fs_with(&names.map(|n| (n, "content")));
    let r = run(&names, true, &fs, |p| {
        if p == names[2] {
            Err(HashFailure::Unreadable(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid path",
            )))
        } else {
            Ok("same".into())
        }
    })
    .unwrap();
    assert_eq!(r, s(&[names[1]]));
}

// C#: DuplicateFinderTests.FindAsync_StatThrowsNotSupported_SkipsThatFileAndKeepsTheRest
#[test]
fn find_stat_not_supported_skips_that_file_and_keeps_the_rest() {
    let names = [
        r"C:\photos\img.jpg",
        r"C:\photos\img (1).jpg",
        r"C:\photos\weird (1).jpg",
    ];
    let fs = fs_with(&names.map(|n| (n, "content")));
    fs.fail(FsOp::Stat, names[2], io::ErrorKind::Unsupported);
    let r = run(&names, true, &fs, constant("same")).unwrap();
    assert_eq!(r, s(&[names[1]]));
}

// C#: DuplicateFinderSkipsUnremovableGroupsTests.FindAsync_GroupWithoutMatchingMember_IsNeverHashed (theory, 2 rows)
#[test]
fn find_group_without_matching_member_is_never_hashed() {
    let rows: [(bool, [&str; 4]); 2] = [
        (
            true,
            [r"C:\p\a.raw", r"C:\p\b.raw", r"C:\p\c (1).jpg", r"C:\p\d (1).jpg"],
        ),
        (
            false,
            [r"C:\p\a (1).raw", r"C:\p\b (1).raw", r"C:\p\c.jpg", r"C:\p\d.jpg"],
        ),
    ];
    for (remove_numbered, [skip_a, skip_b, keep_a, keep_b]) in rows {
        let fs = fs_with(&[
            (skip_a, "1234567890"),
            (skip_b, "abcdefghij"),
            (keep_a, "xy"),
            (keep_b, "xy"),
        ]);
        let hashed = RefCell::new(Vec::new());
        let r = run(&[skip_a, skip_b, keep_a, keep_b], remove_numbered, &fs, |p| {
            hashed.borrow_mut().push(p.to_owned());
            Ok("same".into())
        })
        .unwrap();
        let hashed = hashed.into_inner();
        assert!(!hashed.iter().any(|h| h == skip_a || h == skip_b));
        assert_eq!(hashed.len(), 2);
        assert_eq!(r.len(), 1); // the pair matches entirely, so the first one is kept as survivor
    }
}

/// Test-local `FileSystem` whose only working method is `get_file_stat`: it cancels the token on the first call.
struct CancelOnFirstStat {
    token: CancelToken,
    calls: AtomicUsize,
}

impl FileSystem for CancelOnFirstStat {
    fn file_exists(&self, _: &Path) -> bool {
        unimplemented!()
    }
    fn directory_exists(&self, _: &Path) -> bool {
        unimplemented!()
    }
    fn get_file_stat(&self, _: &Path) -> Option<FileStat> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.token.cancel();
        }
        None
    }
    fn move_file(&self, _: &Path, _: &Path) -> io::Result<()> {
        unimplemented!()
    }
    fn copy_file(&self, _: &Path, _: &Path) -> io::Result<()> {
        unimplemented!()
    }
    fn delete(&self, _: &Path) -> io::Result<()> {
        unimplemented!()
    }
    fn open_read_shared(&self, _: &Path, _: usize) -> io::Result<Box<dyn ReadSeek>> {
        unimplemented!()
    }
    fn open_append(&self, _: &Path, _: bool) -> io::Result<Box<dyn io::Write + Send>> {
        unimplemented!()
    }
    fn write_all_text_atomic(&self, _: &Path, _: &str, _: bool) -> io::Result<()> {
        unimplemented!()
    }
    fn read_all_text(&self, _: &Path) -> io::Result<String> {
        unimplemented!()
    }
    fn enumerate_files(&self, _: &Path, _: &str) -> io::Result<Vec<PathBuf>> {
        unimplemented!()
    }
    fn enumerate_directories(&self, _: &Path) -> io::Result<Vec<PathBuf>> {
        unimplemented!()
    }
    fn create_directory(&self, _: &Path) -> io::Result<()> {
        unimplemented!()
    }
}

// C#: CopyOwnershipFollowupsTests.FindAsync_CancelledDuringSizeScan_StopsAtTheNextPath
#[test]
fn find_cancelled_during_size_scan_stops_at_the_next_path() {
    let token = CancelToken::new();
    let fs = CancelOnFirstStat {
        token: token.clone(),
        calls: AtomicUsize::new(0),
    };
    let files = s(&[r"C:\p\a.jpg", r"C:\p\b.jpg", r"C:\p\c.jpg", r"C:\p\d.jpg"]);
    let r = find(
        &files,
        true,
        &mut |_p: &str, _c: &CancelToken| Ok("h".to_owned()),
        &fs,
        &token,
    );
    assert_eq!(r, Err(Canceled));
    assert_eq!(fs.calls.load(Ordering::SeqCst), 1); // the scan must not keep measuring the remaining files after cancellation
}

// C#: FileActionSafetyEdgeTests.DuplicateFinder_SamePathTwice_KeepsTheFile
#[test]
fn same_path_twice_keeps_the_file() {
    let fs = fs_with(&[(r"C:\photos\a (1).jpg", "same")]);
    let r = run(
        &[r"C:\photos\a (1).jpg", r"C:\photos\A (1).JPG"],
        true,
        &fs,
        constant("h"),
    )
    .unwrap();
    assert!(r.is_empty());
}

// C#: FileActionSafetyEdgeTests.DuplicateFinder_HashCollisionAcrossSizes_KeepsASurvivorPerSizeGroup
#[test]
fn hash_collision_across_sizes_keeps_a_survivor_per_size_group() {
    let short = [r"C:\photos\x (1).jpg", r"C:\photos\y (1).jpg"];
    let long = [r"C:\photos\p (1).jpg", r"C:\photos\q (1).jpg"];
    let fs = FakeFileSystem::new();
    for p in short {
        fs.add_file(p, b"1234");
    }
    for p in long {
        fs.add_file(p, b"12345");
    }
    let all = [short[0], short[1], long[0], long[1]];
    let r = run(&all, true, &fs, constant("collision")).unwrap();
    assert!(short.iter().any(|p| !r.iter().any(|x| x == p)));
    assert!(long.iter().any(|p| !r.iter().any(|x| x == p)));
}

// C#: FileActionsSmallMutationGapTests.FindAsync_TwoEmptyFiles_AreDuplicatesOfEachOther
#[test]
fn two_empty_files_are_duplicates_of_each_other() {
    let fs = fs_with(&[(r"C:\photos\img.jpg", ""), (r"C:\photos\img (1).jpg", "")]);
    let r = run(
        &[r"C:\photos\img.jpg", r"C:\photos\img (1).jpg"],
        true,
        &fs,
        constant("same"),
    )
    .unwrap();
    assert_eq!(r, s(&[r"C:\photos\img (1).jpg"]));
}

// C#: FileActionsSmallMutationGapTests.FindAsync_FilesWithNoSameSizePartner_AreNeverHashed
#[test]
fn files_with_no_same_size_partner_are_never_hashed() {
    let fs = fs_with(&[(r"C:\photos\a (1).jpg", "123"), (r"C:\photos\b (1).jpg", "12345")]);
    let hashed = RefCell::new(Vec::new());
    let r = run(&[r"C:\photos\a (1).jpg", r"C:\photos\b (1).jpg"], true, &fs, |p| {
        hashed.borrow_mut().push(p.to_owned());
        Ok("same".into())
    })
    .unwrap();
    assert!(r.is_empty());
    assert!(hashed.borrow().is_empty());
}

// C#: FileActionsSmallMutationGapTests.FindAsync_PathWhoseStatFails_IsSkippedAndTheOthersAreStillCompared (theory io / unauthorized / argument / notsupported)
#[test]
fn path_whose_stat_fails_is_skipped_and_the_others_are_still_compared() {
    for kind in [
        io::ErrorKind::Other,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::InvalidInput,
        io::ErrorKind::Unsupported,
    ] {
        let fs = fs_with(&[
            (r"C:\photos\img.jpg", "same"),
            (r"C:\photos\img (1).jpg", "same"),
            (r"C:\photos\locked.jpg", "same"),
        ]);
        fs.fail(FsOp::Stat, r"C:\photos\locked.jpg", kind);
        let r = run(
            &[r"C:\photos\img.jpg", r"C:\photos\locked.jpg", r"C:\photos\img (1).jpg"],
            true,
            &fs,
            constant("same"),
        )
        .unwrap();
        assert_eq!(r, s(&[r"C:\photos\img (1).jpg"]), "{kind:?}");
    }
}

#[test]
fn has_numbered_suffix_examples() {
    assert!(has_numbered_suffix("img (1)"));
    assert!(has_numbered_suffix("a\nb (1)"));
    assert!(has_numbered_suffix(" (1)"));
    assert!(has_numbered_suffix("img (１)"));
    assert!(!has_numbered_suffix("img(1)"));
    assert!(!has_numbered_suffix("img (1) "));
    assert!(!has_numbered_suffix("img (𝟏)"));
}

// ---- golden ----

#[test]
fn duplicates_golden() {
    let g = golden::load("duplicates");
    let inputs_path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("duplicates.json");
    let inputs: Value = serde_json::from_str(&std::fs::read_to_string(&inputs_path).unwrap()).unwrap();
    let inputs = inputs["cases"].as_array().unwrap();
    assert_eq!(g.cases.len(), 27);
    assert_eq!(inputs.len(), g.cases.len());
    let root = r"C:\golden-dup";

    for (expected, input) in g.cases.iter().zip(inputs) {
        let name = golden::str_of(expected, "name");
        assert_eq!(golden::str_of(input, "name"), name);
        let fs = FakeFileSystem::new();
        fs.add_dir(root);
        let mut created: Vec<String> = Vec::new();
        for f in input["files"].as_array().unwrap() {
            let n = golden::str_of(f, "name");
            fs.add_file(format!(r"{root}\{n}"), golden::str_of(f, "content").as_bytes());
            created.push(n.to_owned());
        }
        if let Some(dirs) = input.get("directories").and_then(Value::as_array) {
            for d in dirs {
                fs.add_dir(format!(r"{root}\{}", d.as_str().unwrap()));
            }
        }
        let names_of = |v: &Value| -> Vec<String> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_owned())
                .collect()
        };
        let listed = match input.get("listed") {
            Some(l) => names_of(l),
            None => created.clone(),
        };
        let io_errors: Vec<String> = input.get("io_error").map(names_of).unwrap_or_default();
        let hash_mode = &input["hash"];
        let file_name = |p: &str| p.rsplit('\\').next().unwrap().to_owned();
        let hashed = RefCell::new(Vec::<String>::new());

        let files: Vec<String> = listed.iter().map(|n| format!(r"{root}\{n}")).collect();
        let result = find(
            &files,
            input["remove_numbered"].as_bool().unwrap(),
            &mut |p: &str, _c: &CancelToken| {
                let n = file_name(p);
                hashed.borrow_mut().push(n.clone());
                if io_errors.iter().any(|e| e.eq_ignore_ascii_case(&n)) {
                    return Err(HashFailure::Unreadable(io::Error::other("injected")));
                }
                match hash_mode.as_str() {
                    Some("content") => {
                        let bytes = fs.file_content(p).unwrap_or_default();
                        Ok(String::from_utf8_lossy(&bytes).into_owned())
                    }
                    Some(_) => Ok("constant".to_owned()),
                    None => Ok(hash_mode["by_name"][&n].as_str().unwrap().to_owned()),
                }
            },
            &fs,
            &CancelToken::none(),
        )
        .unwrap();

        let removed: Vec<String> = result.iter().map(|p| file_name(p)).collect();
        assert_eq!(removed, names_of(&expected["removed"]), "{name}: removed");
        assert_eq!(hashed.into_inner(), names_of(&expected["hashed"]), "{name}: hashed");
    }
}
