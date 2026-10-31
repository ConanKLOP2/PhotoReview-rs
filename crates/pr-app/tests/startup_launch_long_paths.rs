//! Review L2 of `ST-SHELL-REVIEW-2026-10-31`: `ShellLaunch::parse` with a `\\?\` path, a UNC path and non-ASCII names. New tests, no C# twin.
#![cfg(windows)]
use pr_app::startup::launch::ShellLaunch;
use pr_test_support::{FakeFileSystem, RecordingLog};

#[test]
fn parse_extended_length_path_with_unicode_name_is_kept_verbatim() {
    let path = r"\\?\C:\Ảnh\x.jpg";
    let fs = FakeFileSystem::new();
    fs.add_file(path, b"x");
    let launch = ShellLaunch::parse(&[path], &fs, &RecordingLog::new());
    assert_eq!(launch.initial_file.as_deref(), Some(path));
    assert_eq!(launch.launch_folder.as_deref(), Some(r"\\?\C:\Ảnh"));
    assert_eq!(launch.existing_paths(), &[path.to_owned()]);
}

#[test]
fn parse_unc_path_with_unicode_folder_is_kept_verbatim() {
    let path = r"\\server\share\Ảnh\x.jpg";
    let fs = FakeFileSystem::new();
    fs.add_file(path, b"x");
    let launch = ShellLaunch::parse(&[path], &fs, &RecordingLog::new());
    assert_eq!(launch.initial_file.as_deref(), Some(path));
    assert_eq!(launch.launch_folder.as_deref(), Some(r"\\server\share\Ảnh"));
}

#[test]
fn parse_unc_folder_argument_is_the_launch_folder() {
    let path = r"\\server\share\Ảnh";
    let fs = FakeFileSystem::new();
    fs.add_dir(path);
    let launch = ShellLaunch::parse(&[path], &fs, &RecordingLog::new());
    assert_eq!(launch.initial_folder.as_deref(), Some(path));
    assert_eq!(launch.launch_folder.as_deref(), Some(path));
}
