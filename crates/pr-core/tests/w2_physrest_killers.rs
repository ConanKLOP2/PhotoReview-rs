//! Port of PhotoReview.Core/IO/PhysicalFileSystem.cs: remaining listing mutation guards.
#![cfg(windows)]

use pr_core::abstractions::FileSystem;
use pr_core::io::physical::PhysicalFileSystem;
use pr_test_support::TempRoot;
use std::io;

// C#: Directory.EnumerateFiles/EnumerateDirectories and DirectoryInfo.EnumerateFiles.
// physical.rs:865/905/924/1371/1396/1413 body replacements; :910/:935 directory-bit operators.
#[test]
#[ignore = "native: private TempRoot listing only"]
fn listing_separates_files_directories_and_preserves_entry_stats() {
    let root = TempRoot::new("physrest-list");
    let fs = PhysicalFileSystem;
    let a = root.join("a.jpg");
    let b = root.join("b.txt");
    let dir = root.join("child");
    std::fs::write(&a, b"abc").unwrap();
    std::fs::write(&b, b"12345").unwrap();
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("nested.jpg"), b"nested").unwrap();
    let mut files = fs.enumerate_files(root.path(), "*").unwrap();
    files.sort();
    assert_eq!(files, [a.clone(), b.clone()]);
    assert_eq!(fs.enumerate_files(root.path(), "*.jpg").unwrap(), [a.as_path()]);
    assert_eq!(fs.enumerate_directories(root.path()).unwrap(), [dir]);
    let mut skipped = Vec::new();
    let mut entries = fs
        .enumerate_files_with_stat(root.path(), &|_| true, &mut |e| skipped.push(e))
        .unwrap();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(entries.len(), 2);
    for ((path, stat), (expected, length)) in entries.iter().zip([(&a, 3), (&b, 5)]) {
        assert_eq!(path, expected);
        let stat = stat.as_ref().expect("directory entry has stat");
        assert_eq!(stat.length, length);
        // Compare the same stable entry, never a wall-clock timestamp.
        assert_eq!(Some(*stat), fs.get_file_stat(path));
    }
    assert!(skipped.is_empty());
    let filtered = fs
        .enumerate_files_with_stat(root.path(), &|p| p.extension().is_some_and(|e| e == "txt"), &mut |_| {
            panic!("unexpected skip")
        })
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].0, b);
}

// C#: the adapter rejects unsupported search patterns before asking the OS.
// physical.rs:1378 two || -> && mutants (separator, NUL, rooted alternatives independently).
#[test]
#[ignore = "native: private TempRoot listing only"]
fn listing_rejects_each_unsupported_pattern_independently() {
    let root = TempRoot::new("physrest-pattern");
    for pattern in ["sub/*.jpg", "sub\\*.jpg", "a\0b", "Q:*.jpg"] {
        assert_eq!(
            PhysicalFileSystem
                .enumerate_files(root.path(), pattern)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput,
            "{pattern:?}"
        );
    }
}
