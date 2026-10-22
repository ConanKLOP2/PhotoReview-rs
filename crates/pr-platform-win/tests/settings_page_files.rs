use pr_platform_win::dialogs::settings::pages::files::initial_folder;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("pr-files-page-{stamp}"))
}

#[test]
fn initial_folder_returns_parent_for_existing_file() {
    // C#: FilesSettingsPage.InitialFolder
    let directory = unique_temp_dir();
    fs::create_dir(&directory).unwrap();
    let file = directory.join("editor.exe");
    fs::write(&file, []).unwrap();

    let result = initial_folder(&file.to_string_lossy());

    let _ = fs::remove_file(&file);
    let _ = fs::remove_dir(&directory);
    assert_eq!(
        result.map(|path| path.to_lowercase()),
        Some(directory.to_string_lossy().replace('/', "\\").to_lowercase())
    );
}

#[test]
fn initial_folder_returns_none_for_blank_missing_or_directory_paths() {
    // C#: FilesSettingsPage.InitialFolder
    let directory = unique_temp_dir();
    fs::create_dir(&directory).unwrap();

    let blank = initial_folder("   ");
    let missing = initial_folder(&directory.join("missing.exe").to_string_lossy());
    let directory_result = initial_folder(&directory.to_string_lossy());

    let _ = fs::remove_dir(&directory);
    assert_eq!(blank, None);
    assert_eq!(missing, None);
    assert_eq!(directory_result, None);
}
