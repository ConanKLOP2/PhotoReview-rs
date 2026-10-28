use pr_platform_win::dialogs::settings::pages::files::initial_folder;
use pr_test_support::TempRoot;
use std::fs;

#[test]
fn initial_folder_returns_parent_for_existing_file() {
    // C#: FilesSettingsPage.InitialFolder
    // Own TempRoot per test: the old nanosecond-stamped dirs collided when the two tests ran in parallel.
    let root = TempRoot::new("files-page-parent");
    let directory = root.path().to_path_buf();
    let file = directory.join("editor.exe");
    fs::write(&file, []).unwrap();

    let result = initial_folder(&file.to_string_lossy());

    assert_eq!(
        result.map(|path| path.to_lowercase()),
        Some(directory.to_string_lossy().replace('/', "\\").to_lowercase())
    );
}

#[test]
fn initial_folder_returns_none_for_blank_missing_or_directory_paths() {
    // C#: FilesSettingsPage.InitialFolder
    let root = TempRoot::new("files-page-none");
    let directory = root.path().to_path_buf();

    let blank = initial_folder("   ");
    let missing = initial_folder(&directory.join("missing.exe").to_string_lossy());
    let directory_result = initial_folder(&directory.to_string_lossy());

    assert_eq!(blank, None);
    assert_eq!(missing, None);
    assert_eq!(directory_result, None);
}
