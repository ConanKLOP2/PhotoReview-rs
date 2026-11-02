//! Killers of the SEC-01 guard `is_path_unreachable(&err)` in `final_link_target` (physical.rs). The OS answer of
//! `fs::canonicalize` is injected through `final_link_target_for_tests`, so no disk is touched: an "unreachable" error
//! (dangling link) makes the resolver follow the chain by hand, any other error is returned as-is (fail closed, RV-S06).
#![cfg(windows)]

use std::io;
use std::path::PathBuf;

use pr_core::io::physical::final_link_target_for_tests;

/// Codes of .NET `FileSystem.IsPathUnreachableError`.
const UNREACHABLE: [i32; 12] = [2, 3, 21, 123, 161, 53, 67, 87, 1231, 65, 6, 206];
/// Not in the list: access denied, sharing, lock, a loop (1921), invalid function, directory, bad reparse data.
const NOT_UNREACHABLE: [i32; 9] = [5, 32, 33, 1, 267, 1921, 1920, 4390, 50];

/// Returns (result, number of manual steps taken).
fn resolve(code: i32) -> (io::Result<String>, usize) {
    let mut steps = 0usize;
    let result = final_link_target_for_tests(
        r"C:\l\link",
        &mut |_| Err(io::Error::from_raw_os_error(code)),
        &mut |path| {
            steps += 1;
            (path == r"C:\l\link").then(|| r"C:\l\gone".to_owned())
        },
    );
    (result, steps)
}

#[test]
fn unreachable_os_errors_follow_the_chain_by_hand() {
    for code in UNREACHABLE {
        let (result, steps) = resolve(code);
        assert_eq!(
            result.unwrap_or_else(|e| panic!("code {code}: {e}")),
            r"C:\l\gone",
            "code {code}"
        );
        assert_eq!(steps, 2, "code {code}: link, then its target");
    }
}

#[test]
fn other_os_errors_are_returned_without_walking() {
    for code in NOT_UNREACHABLE {
        let (result, steps) = resolve(code);
        let err = result.expect_err("a non-unreachable OS error must fail closed");
        assert_eq!(err.raw_os_error(), Some(code));
        assert_eq!(steps, 0, "code {code}: the manual walk must not run");
    }
}

#[test]
fn error_without_os_code_is_returned_without_walking() {
    let mut steps = 0usize;
    let err = final_link_target_for_tests(
        r"C:\l\link",
        &mut |_| Err(io::Error::new(io::ErrorKind::NotFound, "no code")),
        &mut |_| {
            steps += 1;
            None
        },
    )
    .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert_eq!(steps, 0);
}

#[test]
fn canonical_path_is_trimmed_of_the_extended_prefix() {
    let ok = |link: &str| {
        final_link_target_for_tests(link, &mut |_| Ok(PathBuf::from(r"\\?\C:\real\x")), &mut |_| {
            panic!("no walk")
        })
    };
    assert_eq!(ok(r"C:\l\link").unwrap(), r"C:\real\x");
    assert_eq!(ok(r"\\?\C:\l\link").unwrap(), r"\\?\C:\real\x");
}
