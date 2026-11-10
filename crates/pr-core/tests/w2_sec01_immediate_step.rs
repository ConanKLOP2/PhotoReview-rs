//! Killers of the one-link step of the SEC-01 manual link walk (`immediate_link_target`, port of
//! `FileSystem.GetImmediateLinkTarget(path, returnFullPath: true, throwOnError: false)`), task T4 of the 2026-11 parity
//! sweep. The attribute query and `read_link` are injected through `immediate_link_target_for_tests`, so no disk (and no
//! privilege for a file symlink) is needed. The `&` of `attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0` is NOT
//! equivalent to `|` or `^`: `|` never takes the shortcut (a non-reparse entry with a readable target is reported as a
//! link) and `^` takes it for exactly 0x400 (a file symlink without the archive bit is reported as "not a link").
#![cfg(windows)]

use std::path::PathBuf;

use pr_core::io::physical::immediate_link_target_for_tests;

const DIRECTORY: u32 = 0x10;
const ARCHIVE: u32 = 0x20;
const REPARSE_POINT: u32 = 0x400;

fn step(path: &str, attributes: Option<u32>, target: Option<&str>) -> Option<String> {
    immediate_link_target_for_tests(path, &mut |_| attributes, &mut |_| target.map(PathBuf::from))
}

// GetImmediateLinkTarget on a file symlink without any other attribute bit (exactly 0x400): kills `&` -> `^`
// (0x400 ^ 0x400 == 0 would return None).
#[test]
fn immediate_step_file_symlink_with_only_reparse_bit_is_followed() {
    assert_eq!(
        step(r"C:\d\link", Some(REPARSE_POINT), Some(r"C:\d\real")),
        Some(r"C:\d\real".to_owned())
    );
}

// A junction (DIRECTORY | REPARSE_POINT) and a symlink with the archive bit are followed too.
#[test]
fn immediate_step_junction_and_archive_symlink_are_followed() {
    assert_eq!(
        step(r"C:\d\j", Some(DIRECTORY | REPARSE_POINT), Some(r"C:\d\t")),
        Some(r"C:\d\t".to_owned())
    );
    assert_eq!(
        step(r"C:\d\s", Some(ARCHIVE | REPARSE_POINT), Some(r"C:\d\t")),
        Some(r"C:\d\t".to_owned())
    );
}

// An entry without the reparse bit is not a link even when read_link would answer: kills `&` -> `|` and `==` -> `!=`.
#[test]
fn immediate_step_entry_without_reparse_bit_is_not_a_link() {
    for attributes in [0, ARCHIVE, DIRECTORY, DIRECTORY | ARCHIVE, 0x1 | 0x2 | 0x4] {
        let mut asked = false;
        let result = immediate_link_target_for_tests(r"C:\d\plain", &mut |_| Some(attributes), &mut |_| {
            asked = true;
            Some(PathBuf::from(r"C:\d\t"))
        });
        assert_eq!(result, None, "attributes {attributes:#x}");
        assert!(!asked, "read_link must not be asked for attributes {attributes:#x}");
    }
}

// A missing entry or an unreadable link (other reparse tag) is `None`, not a path.
#[test]
fn immediate_step_missing_entry_or_unreadable_link_is_none() {
    assert_eq!(step(r"C:\d\gone", None, Some(r"C:\d\t")), None);
    assert_eq!(step(r"C:\d\cloud", Some(REPARSE_POINT), None), None);
}

// The NT prefixes of a junction target are removed (kills the strip_nt_prefix body mutants through the real caller).
#[test]
fn immediate_step_strips_the_nt_prefix_of_the_target() {
    assert_eq!(
        step(r"C:\d\j", Some(REPARSE_POINT), Some(r"\??\C:\real\x")),
        Some(r"C:\real\x".to_owned())
    );
    assert_eq!(
        step(r"C:\d\j", Some(REPARSE_POINT), Some(r"\\?\C:\real\y")),
        Some(r"C:\real\y".to_owned())
    );
    assert_eq!(
        step(r"C:\d\j", Some(REPARSE_POINT), Some(r"\??\UNC\srv\share\x")),
        Some(r"\\srv\share\x".to_owned())
    );
    assert_eq!(
        step(r"C:\d\j", Some(REPARSE_POINT), Some(r"\\?\UNC\srv\share\y")),
        Some(r"\\srv\share\y".to_owned())
    );
}

// A relative target resolves against the link's folder (the root keeps its separator).
#[test]
fn immediate_step_relative_target_resolves_against_the_link_folder() {
    assert_eq!(
        step(r"C:\d\link", Some(REPARSE_POINT), Some("t")),
        Some(r"C:\d\t".to_owned())
    );
    assert_eq!(
        step(r"C:\d\link", Some(REPARSE_POINT), Some(r"..\t")),
        Some(r"C:\t".to_owned())
    );
    assert_eq!(
        step(r"C:\link", Some(REPARSE_POINT), Some("t")),
        Some(r"C:\t".to_owned())
    );
}
