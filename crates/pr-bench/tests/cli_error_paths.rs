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
