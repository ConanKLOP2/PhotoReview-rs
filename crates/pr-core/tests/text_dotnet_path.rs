//! pr_core::text::dotnet_path against golden `dotnet-path` (.NET `System.IO.Path`, W2-00).

use pr_core::text::dotnet_path::*;
use pr_test_support::golden::{self, str_of};
use serde_json::Value;

fn opt(case: &Value, key: &str) -> Option<String> {
    case[key].as_str().map(str::to_owned)
}

fn flag(case: &Value, key: &str) -> bool {
    case[key].as_bool().unwrap_or(false)
}

// C#: Path.GetDirectoryName / GetFileName / GetFileNameWithoutExtension / TrimEndingDirectorySeparator / IsPathRooted /
// IsPathFullyQualified (oracle command dotnet-path).
#[test]
fn path_helpers_match_dotnet_golden() {
    let g = golden::load("dotnet-path");
    let mut n = 0;
    for case in g.cases.iter().filter(|c| c["kind"] == "path") {
        let p = str_of(case, "path");
        assert_eq!(
            get_directory_name(p),
            opt(case, "directory_name"),
            "directory_name {p:?}"
        );
        assert_eq!(
            get_directory_name_raw(p),
            str_of(case, "directory_name_raw"),
            "directory_name_raw {p:?}"
        );
        assert_eq!(get_file_name(p), str_of(case, "file_name"), "file_name {p:?}");
        assert_eq!(get_file_name_without_extension(p), str_of(case, "stem"), "stem {p:?}");
        assert_eq!(
            ends_in_directory_separator(p),
            flag(case, "ends_in_separator"),
            "ends_in_separator {p:?}"
        );
        assert_eq!(
            trim_ending_directory_separator(p),
            str_of(case, "trim_ending"),
            "trim_ending {p:?}"
        );
        assert_eq!(is_path_rooted(p), flag(case, "rooted"), "rooted {p:?}");
        assert_eq!(
            is_path_fully_qualified(p),
            flag(case, "fully_qualified"),
            "fully_qualified {p:?}"
        );
        n += 1;
    }
    assert_eq!(n, 72);
}

// C#: Path.GetFullPath (oracle command dotnet-path).
#[test]
fn get_full_path_matches_dotnet_golden() {
    let g = golden::load("dotnet-path");
    let mut n = 0;
    for case in g.cases.iter().filter(|c| c["kind"] == "full") {
        let p = str_of(case, "path");
        let got = get_full_path(p);
        match case["full"].as_str() {
            Some(expected) => assert_eq!(got.as_deref(), Ok(expected), "full {p:?}"),
            None => {
                assert_eq!(case["full"]["throws"], "ArgumentException", "{p:?}");
                assert_eq!(got, Err(PathError::Argument), "full {p:?}");
            }
        }
        n += 1;
    }
    assert_eq!(n, 39);
}

// C#: Path.GetInvalidPathChars().
#[test]
fn invalid_path_chars_match_dotnet_golden() {
    let g = golden::load("dotnet-path");
    let case = g
        .cases
        .iter()
        .find(|c| c["kind"] == "invalid_path_chars")
        .expect("invalid_path_chars case");
    let mut expected: Vec<u32> = case["code_points"]
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|v| v.as_u64().map(|n| n as u32))
        .collect();
    expected.sort_unstable();
    let actual: Vec<u32> = (0..=0x10FFFF_u32)
        .filter_map(char::from_u32)
        .filter(|&c| is_invalid_path_char(c))
        .map(|c| c as u32)
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn root_length_examples() {
    for (p, n) in [
        ("C:\\a", 3),
        ("C:a", 2),
        ("\\a", 1),
        ("\\\\srv\\share\\x", 11),
        ("\\\\srv\\", 6),
        ("\\\\?\\C:\\a", 7),
        ("\\\\?\\x", 5),
        ("1:\\a", 0),
        ("a\\b", 0),
    ] {
        assert_eq!(root_length(p), n, "{p:?}");
    }
}

#[test]
fn get_full_path_relative_uses_current_directory() {
    let cwd = std::env::current_dir().expect("cwd");
    let expected = cwd.join("y").to_string_lossy().into_owned();
    assert_eq!(get_full_path("x\\..\\y"), Ok(expected));
}

#[test]
fn get_full_path_very_long_does_not_panic() {
    let long = format!("C:\\{}", "a".repeat(40_000));
    match get_full_path(&long) {
        Ok(_) | Err(PathError::TooLong) => {}
        Err(e) => panic!("unexpected {e:?}"),
    }
}

// Differential vs .NET 10: IsExtended covers the NT prefix; nothing is normalized or trimmed.
#[test]
fn get_full_path_nt_prefix_is_unchanged() {
    for p in [r"\??\C:\a", r"\??\a.b ", r"\??\C:/a/../b"] {
        assert_eq!(get_full_path(p), Ok(p.to_owned()), "{p:?}");
    }
}

// Differential vs .NET 10: PathHelper.Normalize expands existing 8.3 names.
#[cfg(windows)]
#[test]
fn get_full_path_expands_existing_short_names() {
    if std::path::Path::new(r"C:\PROGRA~1").exists() {
        let got = get_full_path(r"C:\PROGRA~1").expect("full path");
        assert!(got.eq_ignore_ascii_case(r"C:\Program Files"), "{got:?}");
    }
    assert_eq!(get_full_path(r"C:\NOSUCH~1\x"), Ok(r"C:\NOSUCH~1\x".to_owned()));
}

// Kills the `i > 4` guard mutation: a bare device prefix followed by a separator has root length 4.
#[test]
fn bare_device_prefix_with_separator() {
    for p in ["\x5c\x5c?\x5c\x5c", "\x5c\x5c.\x5c\x5c", "\x5c??\x5c\x5c"] {
        assert_eq!(root_length(p), 4, "{p:?}");
        assert_eq!(get_directory_name(p), Some(p[..4].to_owned()), "{p:?}");
    }
    assert_eq!(
        get_directory_name("\x5c\x5c?\x5c\x5cx"),
        Some("\x5c\x5c?\x5c".to_owned())
    );
}

// Kills the run-collapse mutation of NormalizeDirectorySeparators.
#[test]
fn directory_name_collapses_separator_runs() {
    assert_eq!(
        get_directory_name("C:\x5ca\x5c\x5cb\x5cc.jpg"),
        Some("C:\x5ca\x5cb".to_owned())
    );
    assert_eq!(get_directory_name("a\x5c\x5cb\x5cc.jpg"), Some("a\x5cb".to_owned()));
    assert_eq!(get_directory_name("a//b/c.jpg"), Some("a\x5cb".to_owned()));
    assert_eq!(
        get_directory_name("\x5c??\x5c\x5cx\x5cy"),
        Some("\x5c??\x5cx".to_owned())
    );
}

// Differential vs .NET 10 (PathHelper.TryExpandShortFileName): when the walk-back for a non-existent path reaches the
// root, every index it visited is restored as `\` instead of the original char.
#[cfg(windows)]
#[test]
fn get_full_path_garbles_like_dotnet_when_expansion_fails() {
    let bs = '\x5c';
    let s = |n: usize| bs.to_string().repeat(n);
    // Drive that does not exist, last segment exactly `~` (trailing dots/spaces are trimmed first).
    if !std::path::Path::new("Z:\x5c").exists() {
        for p in ["Z:\x5c~", "Z:~", "Z:/~", "Z:\x5c~.", "Z:\x5c~ "] {
            assert_eq!(get_full_path(p), Ok(format!("{}~", s(3))), "{p:?}");
        }
        // Segment longer than `~` is not garbled.
        assert_eq!(get_full_path("Z:\x5c~a"), Ok("Z:\x5c~a".to_owned()));
    }
    // Device paths: the text before the last char (after the `\.\` prefix) is replaced.
    assert_eq!(
        get_full_path("\x5c\x5c.\x5cPROGRA~1"),
        Ok(format!("\x5c\x5c.\x5c{}1", s(7)))
    );
    assert_eq!(get_full_path("\x5c\x5c.\x5ca~"), Ok(format!("\x5c\x5c.\x5c{}~", s(1))));
    // `//?/` is not an extended prefix for .NET: it is normalized, then expanded like a device path.
    assert_eq!(get_full_path("//?/x~1\x5cy"), Ok(format!("\x5c\x5c?\x5c{}y", s(4))));
    // A real extended path is returned unchanged.
    assert_eq!(
        get_full_path("\x5c\x5c?\x5cx~1\x5cy"),
        Ok("\x5c\x5c?\x5cx~1\x5cy".to_owned())
    );
}

// Kills `all(' ')` -> `trim().is_empty()`: only U+0020 makes the path empty, a tab or NBSP is a relative name.
#[test]
fn get_full_path_tab_or_nbsp_only_is_not_empty() {
    for p in ["\t", "\u{a0}", " \t "] {
        assert!(get_full_path(p).is_ok(), "{p:?}");
    }
    assert_eq!(get_full_path("   "), Err(PathError::Argument));
}

// Hand-written cases from the .NET `PathInternal.GetRootLength` / `Path.GetDirectoryNameOffset` algorithms
// (mutation-kill tests; not in the golden). `@` stands for a backslash.
fn bs(s: &str) -> String {
    s.replace('@', "\\")
}

#[test]
fn directory_name_trims_separator_run_after_unc_root() {
    // The run of separators between the UNC root and the file name is trimmed back to the root, not left as `\`.
    assert_eq!(get_directory_name(&bs("@@srv@share@@x")), Some(bs("@@srv@share")));
    assert_eq!(get_directory_name_raw(&bs("@@srv@share@@x")), bs("@@srv@share"));
    assert_eq!(get_directory_name_raw(&bs("@@srv@share@@@x")), bs("@@srv@share"));
    assert_eq!(get_directory_name_raw(&bs("@@srv@share/@x")), bs("@@srv@share"));
    assert_eq!(
        get_directory_name_raw(&bs("@@?@UNC@srv@share@@x")),
        bs("@@?@UNC@srv@share")
    );
}

#[test]
fn root_length_nt_prefix_needs_separator_after_question_marks() {
    // `\??` without the fourth char being a separator is not a device prefix: root is the single leading `\`.
    for (p, n) in [("@??x", 1), ("@??", 1), ("@??a@b", 1), ("@??@", 4), ("@??@C:@a", 7)] {
        assert_eq!(root_length(&bs(p)), n, "{p:?}");
    }
}

#[test]
fn root_length_device_unc_skips_the_unc_separator() {
    // Scan starts after `\?\UNC\`: the separator at index 7 must not count towards the two that end the share.
    for (p, n) in [
        ("@@?@UNC@srv@share@x", 17),
        ("@@?@UNC@srv@share", 17),
        ("@@?@UNC@srv@", 12),
        ("@@?@UNC@srv", 11),
        ("@@.@UNC/srv/share/x", 17),
    ] {
        assert_eq!(root_length(&bs(p)), n, "{p:?}");
    }
    assert_eq!(
        get_directory_name(&bs("@@?@UNC@srv@share@x")),
        Some(bs("@@?@UNC@srv@share"))
    );
    assert_eq!(get_directory_name(&bs("@@?@UNC@srv@share")), None);
    assert_eq!(get_file_name(&bs("@@?@UNC@srv@share@x.jpg")), "x.jpg");
}
