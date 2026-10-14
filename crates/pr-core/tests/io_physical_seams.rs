//! Lead seams of Wave 3 on `PhysicalFileSystem` (docs/PROPOSED-DECISIONS.md P-11, P-12; contracts/README.md D26).
//!
//! Every test touches the real disk in a private `TempRoot` and is `#[ignore = "native: real disk"]`; run with
//! `cargo test -p pr-core --test io_physical_seams -- --ignored` on Windows.
#![cfg(windows)]

use std::io::{self, Write};
use std::path::Path;

use pr_core::abstractions::FileSystem;
use pr_core::io::physical::{PhysicalFileSystem, is_stat_missing};
use pr_test_support::TempRoot;

// P-11 (W3-01 open question 1): the physical append stream IS the real file (C# `stream is FileStream fs`), so the
// PowerLossSafe journal can flush it to disk; flushing through it must persist what was written.
#[test]
#[ignore = "native: real disk"]
fn open_append_physical_stream_exposes_the_real_file() {
    let root = TempRoot::new("seam-append");
    let path = root.join("journal.jsonl");
    for durable in [false, true] {
        let mut stream = PhysicalFileSystem.open_append(&path, durable).unwrap();
        stream.write_all(b"line\r\n").unwrap();
        let file = stream.as_file_mut().expect("a physical append stream is a real file");
        file.sync_all().unwrap();
        assert_eq!(file.metadata().unwrap().len(), if durable { 12 } else { 6 });
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"line\r\nline\r\n");
}

// P-12: which failed metadata queries mean "nothing there" (Ok(None)) and which mean "cannot be inspected" (Err).
#[test]
fn is_stat_missing_separates_absent_from_denied() {
    for code in [1, 2, 3, 6, 21, 53, 67, 87, 123, 161, 206, 267, 1231] {
        assert!(
            is_stat_missing(&io::Error::from_raw_os_error(code)),
            "raw {code} means nothing is there"
        );
    }
    for code in [5, 32, 33, 65, 1117] {
        assert!(
            !is_stat_missing(&io::Error::from_raw_os_error(code)),
            "raw {code} means the path cannot be inspected"
        );
    }
    assert!(is_stat_missing(&io::Error::from(io::ErrorKind::NotFound)));
    assert!(!is_stat_missing(&io::Error::from(io::ErrorKind::PermissionDenied)));
    assert!(!is_stat_missing(&io::Error::other("boom")));
}

// P-12 on the real disk, following the physical cases of golden `recovery-check` (C# PhysicalFileSystem.GetFileStat at
// d92d6aae): only blank and NUL paths throw there; every other odd name is "not there".
#[test]
#[ignore = "native: real disk"]
fn try_get_file_stat_physical_matches_the_recovery_check_goldens() {
    let root = TempRoot::new("seam-stat");
    let file = root.join("a.jpg");
    std::fs::write(&file, b"0123456789").unwrap();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let fs = PhysicalFileSystem;
    let dir = root.path().to_string_lossy().into_owned();

    let stat = fs.try_get_file_stat(&file).unwrap().expect("p-exists");
    assert_eq!(stat.length, 10);
    assert_eq!(Some(stat), fs.get_file_stat(&file), "same lookup as get_file_stat");
    // p-trailing-separator-source-exists: FileInfo trims ONE trailing separator.
    assert!(
        fs.try_get_file_stat(Path::new(&format!(r"{dir}\a.jpg\")))
            .unwrap()
            .is_some()
    );
    // p-extended-length-prefix-source
    assert!(
        fs.try_get_file_stat(Path::new(&format!(r"\\?\{dir}\a.jpg")))
            .unwrap()
            .is_some()
    );

    for (name, path) in [
        ("p-directory-at-source-path", format!(r"{dir}\sub")),
        ("p-missing", format!(r"{dir}\gone.jpg")),
        ("p-dest-folder-missing", format!(r"{dir}\gone\a.jpg")),
        ("p-file-as-folder", format!(r"{dir}\a.jpg\b.jpg")),
        ("p-wildcard-in-source", format!(r"{dir}\a*.jpg")),
        ("p-question-mark-in-source", format!(r"{dir}\a?.jpg")),
        ("p-less-than-in-source", format!(r"{dir}\a<b.jpg")),
        ("p-pipe-in-source", format!(r"{dir}\a|b.jpg")),
        ("p-colon-stream-in-source", format!(r"{dir}\a.jpg:s")),
        (
            "p-relative-source-missing",
            "pr-oracle-no-such-file-w307.jpg".to_owned(),
        ),
        ("p-component-300-chars", format!(r"{dir}\{}.jpg", "a".repeat(300))),
        ("p-path-40000-chars", format!(r"{dir}\{}", r"aaaaaaaaa\".repeat(4000))),
        ("p-reserved-device-name", format!(r"{dir}\CON")),
        (
            "p-unc-missing-server",
            r"\\pr-oracle-no-such-host-w307\s\a.jpg".to_owned(),
        ),
    ] {
        match fs.try_get_file_stat(Path::new(&path)) {
            Ok(None) => {}
            other => panic!("{name}: expected Ok(None), got {other:?}"),
        }
    }

    for (name, path) in [
        ("p-empty-source", String::new()),
        ("p-whitespace-source", "  ".to_owned()),
        ("p-nul-in-source", format!("{dir}\\a\0b.jpg")),
    ] {
        let err = fs.try_get_file_stat(Path::new(&path)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{name}");
        assert_eq!(
            fs.get_file_stat(Path::new(&path)),
            None,
            "{name}: get_file_stat unchanged"
        );
    }
}
