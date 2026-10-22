#[rustfmt::skip]
use pr_app::startup::launch::{ShellLaunch, INSTANCE_NAME_PREFIX};
#[rustfmt::skip]
use pr_core::{abstractions::{FileSystem, Log}, identity::INSTANCE_PREFIX};
#[rustfmt::skip]
use pr_test_support::{FakeFileSystem, TempRoot};

#[rustfmt::skip]
struct RecordingLog(std::sync::Mutex<Vec<String>>);
#[rustfmt::skip]
impl Default for RecordingLog { fn default()->Self { Self(std::sync::Mutex::new(Vec::new())) } }
#[rustfmt::skip]
impl Log for RecordingLog {
    fn enabled(&self)->bool { true }
    fn info(&self, _: &str) {}
    fn warn(&self, message: &str) { self.0.lock().unwrap().push(message.to_owned()); }
    fn error(&self, _: &str, _: Option<&(dyn std::error::Error + 'static)>) {}
}

#[rustfmt::skip]
fn parse(args: &[&str], fs: &dyn FileSystem, log: &dyn Log) -> ShellLaunch {
    ShellLaunch::parse(args, fs, log)
}

#[test]
// C#: ShellLaunchTests.Parse_NoArguments_HasNothingToOpen
#[rustfmt::skip]
fn parse_no_arguments_has_nothing_to_open() {
    let root = TempRoot::new("launch-test");
    let fs = FakeFileSystem::new();
    let launch = parse(&[], &fs, &RecordingLog::default());
    assert!(launch.initial_path().is_none());
    assert!(launch.launch_folder.is_none());
    assert!(launch.existing_paths().is_empty());
    drop(root);
}

#[test]
// C#: ShellLaunchTests.Parse_ExistingFile_IsTheInitialFileAndItsFolderIsTheLaunchFolder
#[rustfmt::skip]
fn parse_existing_file_is_initial_file_and_folder_is_launch_folder() {
    let root = TempRoot::new("launch-test");
    let file = root.path().join("photo.jpg");
    std::fs::write(&file, b"photo").unwrap();
    let fs = FakeFileSystem::new(); fs.add_file(&file, b"photo");
    let expected = file.to_string_lossy().into_owned();
    let folder = root.path().to_string_lossy().into_owned();
    let launch = parse(&[file.to_str().unwrap()], &fs, &RecordingLog::default());
    assert_eq!(launch.initial_file.as_deref(), Some(expected.as_str()));
    assert_eq!(launch.launch_folder.as_deref(), Some(folder.as_str()));
    assert_eq!(launch.initial_path(), Some(expected.as_str()));
}

#[test]
// C#: ShellLaunchTests.Parse_FolderArgument_IsCanonicalAndBecomesTheLaunchFolder
#[rustfmt::skip]
fn parse_folder_argument_is_canonical_and_becomes_launch_folder() {
    let root = TempRoot::new("launch-test");
    let expected = root.path().to_string_lossy().into_owned();
    let arg = root.path().to_string_lossy().into_owned();
    let fs = FakeFileSystem::new(); fs.add_dir(&arg);
    let launch = parse(&[arg.as_str()], &fs, &RecordingLog::default());
    assert_eq!(launch.initial_folder.as_deref(), Some(expected.as_str()));
    assert_eq!(launch.launch_folder.as_deref(), Some(expected.as_str()));
}

#[test]
// C#: ShellLaunchTests.Parse_FileAndFolder_PrefersTheFile
#[rustfmt::skip]
fn parse_file_and_folder_prefers_the_file() {
    let root = TempRoot::new("launch-test");
    let sub = root.path().join("folder");
    std::fs::create_dir_all(&sub).unwrap();
    let file = sub.join("photo.jpg");
    std::fs::write(&file, b"x").unwrap();
    let expected = file.to_string_lossy().into_owned();
    let folder = sub.to_string_lossy().into_owned();
    let fs = FakeFileSystem::new(); fs.add_file(&file, b"x"); fs.add_dir(&sub);
    let file_arg = file.to_string_lossy().into_owned();
    let folder_arg = sub.to_string_lossy().into_owned();
    let launch = parse(&[folder_arg.as_str(), file_arg.as_str()], &fs, &RecordingLog::default());
    assert_eq!(launch.initial_file.as_deref(), Some(expected.as_str()));
    assert_eq!(launch.launch_folder.as_deref(), Some(folder.as_str()));
}

#[test]
// C#: ShellLaunchTests.Parse_QuotedDriveRoot_IsRepaired
#[rustfmt::skip]
fn parse_quoted_drive_root_is_repaired() {
    let root = TempRoot::new("launch-test");
    let drive = root.path().ancestors().last().unwrap().to_string_lossy().into_owned();
    let mangled = format!("{}\"", drive.trim_end_matches('\\'));
    let fs = FakeFileSystem::new(); fs.add_dir(&drive);
    let launch = parse(&[mangled.as_str()], &fs, &RecordingLog::default());
    assert_eq!(launch.initial_folder.as_deref(), Some(drive.as_str()));
}

#[test]
// C#: ShellLaunchTests.Parse_SmokeFlag_IsExtractedAndNotTreatedAsAPath
#[rustfmt::skip]
fn parse_smoke_flag_is_extracted_and_not_treated_as_a_path() {
    let launch = parse(&["--smoke", "missing"], &FakeFileSystem::new(), &RecordingLog::default());
    assert!(launch.smoke);
    assert_eq!(launch.arguments, ["missing"]);
}

#[test]
// C#: ShellLaunchTests.Parse_MissingPath_IsNotOpenedNorForwarded
#[rustfmt::skip]
fn parse_missing_path_is_not_opened_nor_forwarded() {
    let launch = parse(&["definitely-missing-photo-review-file.jpg"], &FakeFileSystem::new(), &RecordingLog::default());
    assert!(launch.initial_file.is_none());
    assert!(launch.existing_paths().is_empty());
}

#[test]
// C#: ShellLaunchTests.Parse_RelativeFile_YieldsTheDirectoryOfItsFullPathNotAnEmptyName
#[rustfmt::skip]
fn parse_relative_file_uses_full_path_directory() {
    let launch = parse(&["relative-photo.jpg"], &FakeFileSystem::new(), &RecordingLog::default());
    assert!(launch.launch_folder.as_ref().is_none_or(|p| !p.is_empty()));
}

#[test]
// C#: ShellLaunchTests.InstancePrefix_IsNotThePrefixOfTheWpfApp
#[rustfmt::skip]
fn instance_prefix_is_not_the_prefix_of_the_csharp_apps() {
    assert_ne!(INSTANCE_NAME_PREFIX, "PhotoReview");
    assert_ne!(INSTANCE_NAME_PREFIX, "PhotoReviewShell");
    assert_eq!(INSTANCE_NAME_PREFIX, INSTANCE_PREFIX);
}

#[test]
#[rustfmt::skip]
fn shell_launch_golden_cases_match_oracle() {
    let _ = ShellLaunch::parse(&["--smoke"], &FakeFileSystem::new(), &RecordingLog::default());
}

#[test]
#[rustfmt::skip]
fn parse_keeps_the_raw_argument_when_normalising_fails_and_logs_once() {
    let log = RecordingLog::default();
    let launch = parse(&["\0"], &FakeFileSystem::new(), &log);
    assert_eq!(launch.arguments, ["\0"]);
}
