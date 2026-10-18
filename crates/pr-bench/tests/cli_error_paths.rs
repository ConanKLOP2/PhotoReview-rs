//! W7-03d: pins the locked-leaf report write, ToolError::from_io AlreadyExists text and Display (mutation-test gaps).
use pr_bench::cli::backend::ToolError;
#[cfg(windows)]
use pr_bench::cli::tool_path_guard::write_report_file;
#[cfg(windows)]
use std::fs::OpenOptions;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;

/// .NET 10 probe: File.Move(temp, leaf, true) with the leaf held open (FileShare None/Read/ReadWrite/Delete) is always
/// UnauthorizedAccessException("Access to the path is denied.") (no path).
#[cfg(windows)]
#[test]
fn locked_leaf_any_share_is_access_denied_without_path() {
    for share in [0u32, 1, 3, 7] {
        let root = std::env::temp_dir().join(format!("w703d-kill-{}", &pr_core::io::physical::new_guid_n()[..6]));
        std::fs::create_dir_all(&root).unwrap();
        let leaf = root.join("l.json");
        std::fs::write(&leaf, "x").unwrap();
        let h = OpenOptions::new().read(true).share_mode(share).open(&leaf).unwrap();
        let r = write_report_file(&leaf.to_string_lossy(), "y");
        drop(h);
        let _ = std::fs::remove_dir_all(&root);
        let e = r.unwrap_err();
        assert_eq!(
            (e.type_name.as_str(), e.message.as_str()),
            ("UnauthorizedAccessException", "Access to the path is denied."),
            "share {share}"
        );
    }
}

/// .NET Win32Marshal ERROR_ALREADY_EXISTS (183).
#[test]
fn from_io_already_exists_text() {
    let e = ToolError::from_io(
        &std::io::Error::from(std::io::ErrorKind::AlreadyExists),
        r"C:\x\y",
        "FileNotFoundException",
    );
    assert_eq!(e.type_name, "IOException");
    assert_eq!(
        e.message,
        r"Cannot create 'C:\x\y' because a file or directory with the same name already exists."
    );
}

#[test]
fn tool_error_display_is_the_message() {
    assert_eq!(ToolError::argument("boom").to_string(), "boom");
}

// ---- W7 repair: .NET messages and the empty cache directory (probe results on .NET 10) -------------------------------------

use pr_bench::cli::tool_path_guard::{claim_cache_directory, real};
use pr_core::io::physical::PhysicalFileSystem;

fn pair(e: &ToolError) -> (&str, &str) {
    (e.type_name.as_str(), e.message.as_str())
}

/// C#: ToolPathGuard.ClaimCacheDirectory("") -> Directory.CreateDirectory("") = ArgumentException (nothing is created).
#[test]
fn claim_cache_directory_empty_is_argument_exception_and_writes_no_marker() {
    let marker = std::path::Path::new(pr_bench::cli::tool_path_guard::CACHE_MARKER);
    let _ = std::fs::remove_file(marker); // only a leftover of the bug this test guards
    let e = claim_cache_directory("").expect_err("empty path");
    assert_eq!(
        pair(&e),
        (
            "ArgumentException",
            "The value cannot be an empty string. (Parameter 'path')"
        )
    );
    assert!(!marker.exists(), "marker leaked into the current directory");
}

#[test]
fn claim_cache_directory_blank_is_argument_exception() {
    let e = claim_cache_directory(" ").expect_err("blank path");
    assert_eq!(pair(&e), ("ArgumentException", "The path is empty. (Parameter 'path')"));
    let e = claim_cache_directory("a\0b").expect_err("NUL path");
    assert_eq!(
        pair(&e),
        ("ArgumentException", "Null character in path. (Parameter 'path')")
    );
}

#[test]
fn claim_cache_directory_invalid_name_io_exception_has_path_suffix() {
    let root = pr_test_support::TempRoot::new("claim-bad");
    let bad = format!("{}\\a?b", root.path().display());
    let e = claim_cache_directory(&bad).expect_err("invalid name");
    assert_eq!(
        pair(&e),
        (
            "IOException",
            format!("The filename, directory name, or volume label syntax is incorrect. : '{bad}'.").as_str()
        )
    );
}

#[test]
fn claim_cache_directory_over_a_file_is_already_exists_io_exception() {
    let root = pr_test_support::TempRoot::new("claim-file");
    let file = root.path().join("f.txt");
    std::fs::write(&file, "x").unwrap();
    let text = file.to_string_lossy().into_owned();
    let expected = format!("Cannot create '{text}' because a file or directory with the same name already exists.");
    assert_eq!(
        pair(&claim_cache_directory(&text).unwrap_err()),
        ("IOException", expected.as_str())
    );
    let under = format!("{text}\\sub");
    assert_eq!(
        pair(&claim_cache_directory(&under).unwrap_err()),
        ("IOException", expected.as_str())
    );
}

#[test]
fn from_io_generic_io_exception_has_path_suffix() {
    let e = ToolError::from_io(
        &std::io::Error::from_raw_os_error(123),
        r"C:\x\y",
        "FileNotFoundException",
    );
    assert_eq!(
        pair(&e),
        (
            "IOException",
            r"The filename, directory name, or volume label syntax is incorrect. : 'C:\x\y'."
        )
    );
}

/// C#: PhysicalFileSystem.ResolveRealPath -> ArgumentException.ThrowIfNullOrWhiteSpace(path) -> "... (Parameter 'path')".
#[test]
fn real_blank_and_nul_messages_have_the_parameter_suffix() {
    let fs = PhysicalFileSystem;
    for blank in ["", " "] {
        let e = real(&fs, blank).expect_err("blank");
        assert_eq!(
            pair(&e),
            (
                "ArgumentException",
                "The value cannot be an empty string or composed entirely of whitespace. (Parameter 'path')"
            )
        );
    }
    let e = real(&fs, "a\0b").expect_err("nul");
    assert_eq!(
        pair(&e),
        ("ArgumentException", "Null character in path. (Parameter 'path')")
    );
}
