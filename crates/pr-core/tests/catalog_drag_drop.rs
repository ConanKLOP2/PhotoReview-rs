//! Tests for `pr_core::catalog::drag_drop` (port of `DragDropInputService.cs`, C# baseline d92d6aae).
//! Each test names the C# test it ports in a `// C#:` comment. Real temp folders become `FakeFileSystem` entries under
//! [`ROOT`]; the golden file `tests/golden/drag-drop.json` is replayed in [`golden_drag_drop_cases_match_csharp`].

use pr_core::abstractions::FileSystem;
use pr_core::catalog::drag_drop::{DragDropInputKind, DragDropInputResult, DragDropWarning, parse};
use pr_core::text::dotnet_path::get_full_path;
use pr_test_support::FakeFileSystem;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// Fixed fake root for every test (Windows-style, as the C# tests ran on Windows).
const ROOT: &str = r"C:\fake-root";

fn root(relative: &str) -> String {
    format!("{ROOT}\\{relative}")
}

/// A fake file system holding `dirs` and `files` (1-byte files) below [`ROOT`]; parents are created with the files.
fn fake_fs(dirs: &[&str], files: &[&str]) -> FakeFileSystem {
    let fs = FakeFileSystem::new();
    for dir in dirs {
        fs.add_dir(root(dir));
    }
    for file in files {
        fs.add_file(root(file), b"x");
    }
    fs
}

fn parse_strs<S: AsRef<str>>(paths: &[S], fs: &dyn FileSystem, raw: bool, webp_heic: bool) -> DragDropInputResult {
    let owned: Vec<String> = paths.iter().map(|p| p.as_ref().to_owned()).collect();
    parse(Some(&owned), raw, webp_heic, fs)
}

fn warning_name(warning: Option<DragDropWarning>) -> Option<&'static str> {
    warning.map(|w| match w {
        DragDropWarning::NoValidInput => "NoValidInput",
        DragDropWarning::OnlyFirstFolder => "OnlyFirstFolder",
        DragDropWarning::NoSupportedImages => "NoSupportedImages",
        DragDropWarning::FolderUnknown => "FolderUnknown",
        DragDropWarning::MixedFolders => "MixedFolders",
        DragDropWarning::UnsupportedIgnored => "UnsupportedIgnored",
    })
}

fn kind_name(kind: DragDropInputKind) -> &'static str {
    match kind {
        DragDropInputKind::Invalid => "Invalid",
        DragDropInputKind::Folder => "Folder",
        DragDropInputKind::Image => "Image",
    }
}

const ALL_WARNINGS: [DragDropWarning; 6] = [
    DragDropWarning::NoValidInput,
    DragDropWarning::OnlyFirstFolder,
    DragDropWarning::NoSupportedImages,
    DragDropWarning::FolderUnknown,
    DragDropWarning::MixedFolders,
    DragDropWarning::UnsupportedIgnored,
];

// C#: DragDropInputServiceTests.ParseEmptyOrNullReturnsInvalid
#[test]
fn parse_empty_or_null_returns_invalid() {
    let fs = FakeFileSystem::new();

    let result_null = parse(None, false, false, &fs);
    assert!(!result_null.is_valid());
    assert_eq!(result_null.kind, DragDropInputKind::Invalid);

    let result_empty = parse(Some(&[]), false, false, &fs);
    assert!(!result_empty.is_valid());
}

// C#: DragDropInputServiceTests.ParseExistingDirectoryReturnsFolderKind
// The C# test uses Path.GetTempPath(), which ends with a separator: the fake folder keeps that spelling.
#[test]
fn parse_existing_directory_returns_folder_kind() {
    let fs = FakeFileSystem::new();
    fs.add_dir(r"C:\fake-temp");
    let temp_dir = r"C:\fake-temp\";

    let result = parse_strs(&[temp_dir], &fs, false, false);

    assert!(result.is_valid());
    assert_eq!(result.kind, DragDropInputKind::Folder);
    assert_eq!(result.folder_path, Some(get_full_path(temp_dir).expect("full path")));
}

// C#: DragDropInputServiceTests.ParseImageFileReturnsImageKindWithInitialImage
#[test]
fn parse_image_file_returns_image_kind_with_initial_image() {
    let fs = fake_fs(&[], &["drag-folder\\first.JPG"]);

    let parsed = parse_strs(&[&root("drag-folder\\first.JPG")], &fs, false, false);

    assert_eq!(parsed.kind, DragDropInputKind::Image);
    assert_eq!(parsed.folder_path, Some(root("drag-folder")));
    assert_eq!(parsed.initial_image_path, Some(root("drag-folder\\first.JPG")));
}

// C#: DragDropInputServiceTests.ParseHeicFile_FollowsTheWebpHeicSwitch (InlineData true)
#[test]
fn parse_heic_file_with_webp_heic_switch_on_is_valid() {
    let fs = fake_fs(&["iphone"], &["iphone\\IMG_0001.HEIC"]);
    let image = root("iphone\\IMG_0001.HEIC");

    let parsed = parse_strs(&[&image], &fs, false, true);

    assert!(parsed.is_valid());
    assert_eq!(parsed.initial_image_path, Some(image));
}

// C#: DragDropInputServiceTests.ParseHeicFile_FollowsTheWebpHeicSwitch (InlineData false)
#[test]
fn parse_heic_file_with_webp_heic_switch_off_is_invalid() {
    let fs = fake_fs(&["iphone"], &["iphone\\IMG_0001.HEIC"]);
    let image = root("iphone\\IMG_0001.HEIC");

    let parsed = parse_strs(&[&image], &fs, false, false);

    assert!(!parsed.is_valid());
    assert_eq!(parsed.initial_image_path, None);
}

// C#: DragDropInputServiceTests.ParseUnsupportedFileIsInvalid
#[test]
fn parse_unsupported_file_is_invalid() {
    let fs = FakeFileSystem::new();

    assert!(!parse_strs(&[&root("notes.txt")], &fs, false, false).is_valid());
}

// C#: DragDropInputServiceTests.ParseSameFolderWithTrailingSeparator_IsNotIgnored (W2-CC-06)
#[test]
fn parse_same_folder_with_trailing_separator_is_not_ignored() {
    let fs = fake_fs(&["same-folder"], &[]);
    let folder = root("same-folder");
    let with_separator = format!("{folder}\\");
    let uppercase = folder.to_uppercase();

    let parsed = parse_strs(&[&folder, &with_separator, &uppercase], &fs, false, false);

    assert_eq!(parsed.kind, DragDropInputKind::Folder);
    assert_eq!(parsed.ignored_path_count, 0);
    assert_eq!(parsed.warning, None);
}

// C#: CatalogMutationGapTests.DragDropResult_IsValid_NeedsAKindOtherThanInvalidAndAFolder (6 InlineData rows)
#[test]
fn drag_drop_result_is_valid_needs_a_kind_other_than_invalid_and_a_folder() {
    let cases = [
        (DragDropInputKind::Folder, Some(r"C:\x"), true),
        (DragDropInputKind::Image, Some(r"C:\x"), true),
        (DragDropInputKind::Folder, None, false),
        (DragDropInputKind::Image, None, false),
        (DragDropInputKind::Invalid, Some(r"C:\x"), false),
        (DragDropInputKind::Invalid, None, false),
    ];
    for (kind, folder, expected) in cases {
        let result = DragDropInputResult {
            kind,
            folder_path: folder.map(str::to_owned),
            initial_image_path: None,
            ignored_path_count: 0,
            warning: None,
        };
        assert_eq!(result.is_valid(), expected, "kind {kind:?}, folder {folder:?}");
    }
}

// C#: CatalogMutationGapTests.DragDropParse_SingleFolder_HasNoWarningAndNothingIgnored
#[test]
fn parse_single_folder_has_no_warning_and_nothing_ignored() {
    let fs = fake_fs(&["only"], &[]);

    let result = parse_strs(&[&root("only")], &fs, false, false);

    assert_eq!(result.kind, DragDropInputKind::Folder);
    assert_eq!(result.ignored_path_count, 0);
    assert_eq!(result.warning, None);
}

// C#: CatalogMutationGapTests.DragDropParse_SameFolderTwiceInDifferentCase_IsNotCountedAsIgnored
#[test]
fn parse_same_folder_twice_in_different_case_is_not_counted_as_ignored() {
    let fs = fake_fs(&["Photos"], &[]);
    let folder = root("Photos");

    let result = parse_strs(&[&folder, &folder.to_uppercase()], &fs, false, false);

    assert_eq!(result.ignored_path_count, 0);
    assert_eq!(result.warning, None);
}

// C#: CatalogMutationGapTests.DragDropParse_FolderPlusOtherPaths_IgnoresThemAndWarnsOnlyTheFirstFolderIsUsed
#[test]
fn parse_folder_plus_other_paths_ignores_them_and_warns_only_first_folder_is_used() {
    let fs = fake_fs(&["first", "second"], &["first\\a.jpg"]);

    let result = parse_strs(
        &[&root("first"), &root("second"), &root("first\\a.jpg")],
        &fs,
        false,
        false,
    );

    assert_eq!(result.kind, DragDropInputKind::Folder);
    assert_eq!(result.folder_path, Some(root("first")));
    assert_eq!(result.ignored_path_count, 2);
    assert_eq!(result.warning, Some(DragDropWarning::OnlyFirstFolder));
}

// C#: CatalogMutationGapTests.DragDropParse_SeveralImagesOfOneFolder_AcceptsThemAllWithoutWarning
#[test]
fn parse_several_images_of_one_folder_accepts_them_all_without_warning() {
    let fs = fake_fs(&[], &["shots\\a.jpg", "shots\\b.png"]);

    let result = parse_strs(&[&root("shots\\a.jpg"), &root("shots\\b.png")], &fs, false, false);

    assert_eq!(result.kind, DragDropInputKind::Image);
    assert_eq!(result.initial_image_path, Some(root("shots\\a.jpg")));
    assert_eq!(result.ignored_path_count, 0);
    assert_eq!(result.warning, None);
}

// C#: CatalogMutationGapTests.DragDropParse_ImageAndUnsupportedFile_CountsTheUnsupportedOneAsIgnoredAndWarns
#[test]
fn parse_image_and_unsupported_file_counts_unsupported_as_ignored_and_warns() {
    let fs = fake_fs(&[], &["shots\\a.jpg", "shots\\notes.txt"]);

    let result = parse_strs(
        &[
            &root("shots\\a.jpg"),
            &root("shots\\notes.txt"),
            &root("shots\\gone.jpg"),
        ],
        &fs,
        false,
        false,
    );

    assert_eq!(result.kind, DragDropInputKind::Image);
    assert_eq!(result.ignored_path_count, 2);
    assert_eq!(result.warning, Some(DragDropWarning::UnsupportedIgnored));
}

// C#: CatalogMutationGapTests.DragDropParse_ImagesFromTwoFolders_KeepsTheFirstFolderAndWarnsAboutMixedFolders
#[test]
fn parse_images_from_two_folders_keeps_first_folder_and_warns_mixed_folders() {
    let fs = fake_fs(&[], &["one\\a.jpg", "two\\b.jpg"]);

    let result = parse_strs(&[&root("one\\a.jpg"), &root("two\\b.jpg")], &fs, false, false);

    assert_eq!(result.kind, DragDropInputKind::Image);
    assert_eq!(result.folder_path, Some(root("one")));
    assert_eq!(result.ignored_path_count, 1);
    assert_eq!(result.warning, Some(DragDropWarning::MixedFolders));
}

// C#: CatalogServicesRobustnessTests.DragDrop_InvalidPaths_AreInvalidNotExceptions
// DIVERGENCE: a path that `get_full_path` rejects (NUL, 5000 characters) counts as "does not exist" instead of throwing.
#[test]
fn parse_invalid_paths_are_invalid_not_exceptions() {
    let fs = FakeFileSystem::new();
    let long_name = "x".repeat(5000);
    let paths = ["<>|?*", "C:\\a\0b.jpg", long_name.as_str(), "   "];

    let result = parse_strs(&paths, &fs, false, false);

    assert!(!result.is_valid());
    assert_eq!(result.kind, DragDropInputKind::Invalid);
    assert_eq!(result.folder_path, None);
    assert_eq!(result.ignored_path_count, 3);
    assert_eq!(result.warning, Some(DragDropWarning::NoSupportedImages));
}

// C#: CatalogServicesRobustnessTests.DragDrop_NullAndEmpty (the Warning is not null)
#[test]
fn parse_null_and_empty_warns_no_valid_input() {
    let fs = FakeFileSystem::new();

    assert!(!parse(None, false, false, &fs).is_valid());
    let empty = parse(Some(&[]), false, false, &fs);
    assert!(!empty.is_valid());
    assert_eq!(empty.warning, Some(DragDropWarning::NoValidInput));
}

// C#: JournalErrorCodeTests.CoreMessages_English (line 171): "No valid file or folder." is the English text. The
// localizer is another unit, so this checks the catalog keys exist in assets/languages/en.json with non-empty text.
#[test]
fn warning_catalog_keys_exist_in_en_json() {
    let path = golden::repo_root().join("assets").join("languages").join("en.json");
    let text = std::fs::read_to_string(&path).expect("read en.json");
    let doc: Value = serde_json::from_str(&text).expect("parse en.json");

    for warning in ALL_WARNINGS {
        let key = warning.catalog_key();
        let value = doc[key].as_str().unwrap_or_else(|| panic!("en.json has no key {key}"));
        assert!(!value.trim().is_empty(), "en.json {key} is empty");
    }
    assert_eq!(
        doc[DragDropWarning::NoValidInput.catalog_key()].as_str(),
        Some("No valid file or folder.")
    );
}

/// `case[key]` as a list of strings with `{root}` replaced by [`ROOT`]; empty when absent.
fn strings_of(case: &Value, key: &str) -> Vec<String> {
    case[key]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|s| s.replace("{root}", ROOT)))
                .collect()
        })
        .unwrap_or_default()
}

/// `case[key]` as an optional string with `{root}` replaced by [`ROOT`].
fn optional_path_of(case: &Value, key: &str) -> Option<String> {
    case[key].as_str().map(|s| s.replace("{root}", ROOT))
}

// Golden: tests/golden/drag-drop.json (oracle command `drag-drop`, 35 cases). Each case builds its tree in a fake file
// system under ROOT, replays `Parse(paths, raw, webp_heic)` and compares every result field with the C# output.
#[test]
fn golden_drag_drop_cases_match_csharp() {
    let golden = golden::load("drag-drop");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let dirs = strings_of(case, "dirs");
        let files = strings_of(case, "files");
        let fs = FakeFileSystem::new();
        for dir in &dirs {
            fs.add_dir(format!("{ROOT}\\{dir}"));
        }
        for file in &files {
            fs.add_file(format!("{ROOT}\\{file}"), b"x");
        }

        let paths: Option<Vec<String>> = if case["paths"].is_null() {
            None
        } else {
            Some(strings_of(case, "paths"))
        };
        let raw = bool_of(case, "raw");
        let webp_heic = bool_of(case, "webp_heic");
        let result = parse(paths.as_deref(), raw, webp_heic, &fs);

        assert_eq!(kind_name(result.kind), str_of(case, "kind"), "{name}: kind");
        assert_eq!(result.is_valid(), bool_of(case, "is_valid"), "{name}: is_valid");
        assert_eq!(
            result.folder_path,
            optional_path_of(case, "folder_path"),
            "{name}: folder_path"
        );
        assert_eq!(
            result.initial_image_path,
            optional_path_of(case, "initial_image_path"),
            "{name}: initial_image_path"
        );
        assert_eq!(
            result.ignored_path_count as i64,
            i64_of(case, "ignored_path_count"),
            "{name}: ignored_path_count"
        );
        assert_eq!(
            warning_name(result.warning),
            case["warning"].as_str(),
            "{name}: warning"
        );
    }
}
