//! Physical-filesystem mutation killers (lane CL-PHYS3). Pure error classification; no disk, no sleeps.
//! Port of PhotoReview.Core/IO/PhysicalFileSystem.cs stat-failure classification (P-12).

use std::io;

use pr_core::io::physical::is_stat_missing;

// physical.rs:1091 `is_stat_missing` -> true / false, :1097 `==` -> `!=` (kind NotFound arm).
#[test]
fn is_stat_missing_absence_codes_are_missing() {
    // 1 = ERROR_INVALID_FUNCTION (device), 267 = ERROR_DIRECTORY, 2/3 not found, 123 bad name, 53 bad net path.
    for code in [1, 267, 2, 3, 123, 53, 21, 206] {
        assert!(
            is_stat_missing(&io::Error::from_raw_os_error(code)),
            "code {code} must be missing"
        );
    }
    assert!(is_stat_missing(&io::Error::new(io::ErrorKind::NotFound, "gone")));
}

#[test]
fn is_stat_missing_denials_and_unknown_failures_are_not_missing() {
    // 5 access denied, 32 sharing, 33 lock, 65 network access denied (in the unreachable list but a denial), 1314 privilege.
    for code in [5, 32, 33, 65, 1314] {
        assert!(
            !is_stat_missing(&io::Error::from_raw_os_error(code)),
            "code {code} must not be missing"
        );
    }
    assert!(!is_stat_missing(&io::Error::new(
        io::ErrorKind::PermissionDenied,
        "denied"
    )));
    assert!(!is_stat_missing(&io::Error::other("boom")));
}

// physical.rs:1130 `+` -> `*` / `-` in `flush_temp_file`: the per-thread flush counter must advance by exactly one per
// durable write and stay put for a non-durable one (counter starts at 0, so `*` never moves it and `-` underflows).
#[test]
fn durable_atomic_write_counts_exactly_one_flush_per_write_and_non_durable_none() {
    use pr_core::abstractions::FileSystem;
    use pr_core::io::physical::{PhysicalFileSystem, temp_file_flushes_on_this_thread};

    let root = pr_test_support::TempRoot::new("phys3-flush");
    let target = root.path().join("a.txt");
    let fs = PhysicalFileSystem;
    let start = temp_file_flushes_on_this_thread();
    fs.write_all_text_atomic(&target, "one", true).unwrap();
    assert_eq!(temp_file_flushes_on_this_thread(), start + 1);
    fs.write_all_text_atomic(&target, "two", true).unwrap();
    assert_eq!(temp_file_flushes_on_this_thread(), start + 2);
    fs.write_all_text_atomic(&target, "three", false).unwrap();
    assert_eq!(temp_file_flushes_on_this_thread(), start + 2);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "three");
}

// physical.rs:1258:29 match guard (`attributes_of(..) & FILE_ATTRIBUTE_DIRECTORY != 0`) -> true / false in `try_get_file_stat`.
#[test]
fn try_get_file_stat_reports_a_file_but_none_for_a_directory_or_missing() {
    use pr_core::abstractions::FileSystem;
    use pr_core::io::physical::PhysicalFileSystem;

    let root = pr_test_support::TempRoot::new("phys3-stat");
    let file = root.path().join("f.bin");
    std::fs::write(&file, [1u8, 2, 3, 4, 5]).unwrap();
    let dir = root.path().join("d");
    std::fs::create_dir(&dir).unwrap();
    let fs = PhysicalFileSystem;
    let stat = fs.try_get_file_stat(&file).unwrap().expect("file has stat");
    assert_eq!(stat.length, 5);
    assert!(
        fs.try_get_file_stat(&dir).unwrap().is_none(),
        "a directory is not a file stat"
    );
    assert!(fs.try_get_file_stat(&root.path().join("nope")).unwrap().is_none());
}

// physical.rs:1266:29 and :1273:29 `||` -> `&&` in `move_file` / `copy_file`: ONE blank side must already be rejected with the
// blank-path error (the `&&` form lets a half blank pair reach the OS call), and nothing on disk changes.
#[test]
fn move_and_copy_with_exactly_one_blank_side_fail_with_the_blank_path_error_and_change_nothing() {
    use pr_core::abstractions::FileSystem;
    use pr_core::io::physical::PhysicalFileSystem;
    use std::path::Path;

    let root = pr_test_support::TempRoot::new("phys3-blank");
    let file = root.path().join("keep.txt");
    std::fs::write(&file, "data").unwrap();
    let fs = PhysicalFileSystem;
    let blank = Path::new("  ");
    let other = root.path().join("other.txt");
    for (src, dst) in [(blank, other.as_path()), (file.as_path(), blank)] {
        for result in [fs.move_file(src, dst), fs.copy_file(src, dst)] {
            let err = result.expect_err("half blank pair must fail");
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
            assert!(
                err.to_string()
                    .contains("empty string or composed entirely of whitespace"),
                "{err}"
            );
        }
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "data");
    assert!(!other.exists());
}
