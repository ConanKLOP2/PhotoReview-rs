//! Red tests for `pr_app::misc::launch_args` (contract W8-04d). Port of `PhotoReview.App.Shared/LaunchArguments.cs` (C#
//! baseline d92d6aae) and `PhotoReview.App.Tests/LaunchArgumentsTests.cs` (all six tests).
//!
//! Folders are real temp folders ([`TempRoot`]) on the [`PhysicalFileSystem`]. Expected paths come from
//! `get_full_path(temp.path())`, never from the string the test built by hand (AGENTS.md CI note: 8.3 short names such as
//! `RUNNER~1` are not expanded by `get_full_path`, trap 11).

use std::fs;
use std::path::Path;

use pr_app::misc::launch_args::{normalize, normalize_one};
use pr_core::file_actions::errors::get_path_root;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::text::dotnet_path::get_full_path;
use pr_test_support::{FakeFileSystem, TempRoot};

/// The expected canonical spelling of an existing folder: `get_full_path` of its path (trap 11).
fn full(path: &Path) -> String {
    get_full_path(path.to_str().expect("the temp path is valid UTF-8")).expect("the temp path is absolute")
}

#[test]
fn an_existing_folder_loses_a_trailing_separator() {
    // C#: LaunchArgumentsTests.An_existing_folder_loses_a_trailing_separator
    let temp = TempRoot::new("launch-args");
    let with_separator = format!("{}\\", temp.path().display());

    let result = normalize_one(&PhysicalFileSystem, &with_separator).expect("the folder path is valid");

    assert_eq!(result, full(temp.path()));
}

#[test]
fn a_folder_with_a_dot_segment_is_made_canonical() {
    // C#: LaunchArgumentsTests.A_folder_with_a_dot_segment_is_made_canonical
    let temp = TempRoot::new("launch-args");
    let with_dot = format!("{}\\.", temp.path().display());

    let result = normalize_one(&PhysicalFileSystem, &with_dot).expect("the folder path is valid");

    assert_eq!(result, full(temp.path()));
}

#[test]
fn a_quoted_folder_ending_in_a_backslash_is_repaired() {
    // C#: LaunchArgumentsTests.A_quoted_folder_ending_in_a_backslash_is_repaired
    // "C:\x\" reaches the process as C:\x" (the backslash escapes the closing quote).
    let temp = TempRoot::new("launch-args");
    let quoted = format!("{}\"", temp.path().display());

    let result = normalize_one(&PhysicalFileSystem, &quoted).expect("the folder path is valid");

    assert_eq!(result, full(temp.path()));
}

#[test]
fn a_drive_root_keeps_its_separator_and_a_quoted_root_is_repaired() {
    // C#: LaunchArgumentsTests.A_drive_root_keeps_its_separator_and_a_quoted_root_is_repaired
    let temp = TempRoot::new("launch-args");
    let root = get_path_root(&full(temp.path())).expect("an absolute temp path has a root");

    assert_eq!(
        normalize_one(&PhysicalFileSystem, &root).expect("the root is valid"),
        root
    );
    let quoted_root = format!("{}\"", root.trim_end_matches('\\'));
    assert_eq!(
        normalize_one(&PhysicalFileSystem, &quoted_root).expect("the root is valid"),
        root
    );
}

#[test]
fn a_file_and_a_missing_path_are_returned_unchanged() {
    // C#: LaunchArgumentsTests.A_file_and_a_missing_path_are_returned_unchanged (the empty string too)
    let temp = TempRoot::new("launch-args");
    let file = temp.join("a.jpg");
    fs::write(&file, b"x").expect("write the test file");
    let file_text = file.display().to_string();
    let missing = format!("{}\\", temp.join("missing").display());

    assert_eq!(
        normalize_one(&PhysicalFileSystem, &file_text).expect("a file is valid"),
        file_text
    );
    assert_eq!(
        normalize_one(&PhysicalFileSystem, &missing).expect("a missing path is valid"),
        missing
    );
    assert_eq!(
        normalize_one(&PhysicalFileSystem, "").expect("the empty string is valid"),
        ""
    );
}

#[test]
fn normalize_maps_every_argument_and_keeps_the_order() {
    // C#: LaunchArgumentsTests.Normalize_maps_every_argument_and_keeps_the_order
    let temp = TempRoot::new("launch-args");
    let file = temp.join("a.jpg");
    fs::write(&file, b"x").expect("write the test file");
    let file_text = file.display().to_string();
    let folder_with_separator = format!("{}\\", temp.path().display());

    let result = normalize(
        &PhysicalFileSystem,
        &[file_text.as_str(), folder_with_separator.as_str(), "--i18n-keys"],
    )
    .expect("every argument is valid");

    assert_eq!(result, vec![file_text, full(temp.path()), "--i18n-keys".to_owned()]);
}

#[test]
fn quote_repair_without_an_existing_folder_returns_the_original_text() {
    // Trap 10 (new): a quoted path whose repaired form is missing returns the ORIGINAL text, quote included.
    let temp = TempRoot::new("launch-args");
    let quoted_missing = format!("{}\"", temp.join("missing").display());

    let result = normalize_one(&PhysicalFileSystem, &quoted_missing).expect("the argument is valid");

    assert_eq!(result, quoted_missing);
}

#[test]
fn all_trailing_separators_are_removed() {
    // Rule 16 (new): C# TrimEnd('\\', '/') removes ALL trailing separators, not one.
    let temp = TempRoot::new("launch-args");
    let two_separators = format!("{}\\\\", temp.path().display());

    let result = normalize_one(&PhysicalFileSystem, &two_separators).expect("the folder path is valid");

    assert_eq!(result, full(temp.path()));
}

#[test]
fn a_lone_quote_is_not_repaired() {
    // Rule 15 and trap 17 (new): a lone '"' (length 1) is not a quote repair; it is returned unchanged.
    assert_eq!(
        normalize_one(&PhysicalFileSystem, "\"").expect("a lone quote is valid"),
        "\""
    );
}

#[test]
fn whitespace_only_argument_is_returned_unchanged() {
    // Rule 14 step 1 (new): whitespace-only input is returned unchanged.
    assert_eq!(
        normalize_one(&PhysicalFileSystem, "   ").expect("whitespace is valid"),
        "   "
    );
}

#[test]
fn unc_root_keeps_its_trailing_separator() {
    // Trap 17 (new): a UNC root keeps its trailing separator; its length comes from get_path_root.
    let fake = FakeFileSystem::new();
    fake.add_dir("\\\\server\\share\\");
    let unc_root = "\\\\server\\share\\";
    let expected = get_path_root(unc_root).expect("a UNC root has a root");

    let result = normalize_one(&fake, unc_root).expect("the UNC root is valid");

    assert_eq!(result, expected);
    assert!(result.ends_with('\\'));
}

#[test]
fn nt_prefixed_folder_keeps_its_extended_prefix() {
    // Trap 17 (new): the NT prefix \\?\ survives normalization; the trailing separator is removed.
    let temp = TempRoot::new("launch-args");
    let nt_prefixed = format!("\\\\?\\{}\\", temp.path().display());
    // Expected: get_full_path of the folder without its trailing separator (the separator rule applied to it).
    let expected = get_full_path(nt_prefixed.trim_end_matches('\\')).expect("the NT-prefixed path is valid");

    let result = normalize_one(&PhysicalFileSystem, &nt_prefixed).expect("the NT-prefixed folder is valid");

    assert!(result.starts_with("\\\\?\\"), "the NT prefix must survive: {result}");
    assert_eq!(result, expected);
}
