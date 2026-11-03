//! G18 (docs/reviews/pr-164.md L3): `ShellLaunch::parse` logs a normalisation failure once (with the count), and logs nothing when every
//! argument normalises. The older `parse_keeps_the_raw_argument_when_normalising_fails_and_logs_once` never read the log.
use pr_app::startup::launch::ShellLaunch;
use pr_core::abstractions::Log;
use pr_test_support::FakeFileSystem;
use std::sync::Mutex;

#[derive(Default)]
struct Recording(Mutex<Vec<String>>);

impl Log for Recording {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, _: &str) {}
    fn warn(&self, message: &str) {
        self.0.lock().unwrap().push(message.to_owned());
    }
    fn error(&self, _: &str, _: Option<&(dyn std::error::Error + 'static)>) {}
}

#[test]
fn normalisation_failures_are_logged_once_with_their_count() {
    let log = Recording::default();
    // Only an argument naming an existing directory goes through get_full_path, so the fake lists both bad names as directories.
    let fs = FakeFileSystem::new();
    fs.add_dir("\0");
    fs.add_dir("a\0b");
    let launch = ShellLaunch::parse(&["\0", "--smoke", "a\0b"], &fs, &log);
    assert!(launch.smoke);
    assert_eq!(launch.arguments, ["\0", "a\0b"]);
    assert_eq!(
        *log.0.lock().unwrap(),
        ["Launch arguments could not be normalised and were kept as given: 2"]
    );
}

#[test]
fn nothing_is_logged_when_every_argument_normalises() {
    let log = Recording::default();
    let _ = ShellLaunch::parse(&["missing-photo.jpg", "--smoke"], &FakeFileSystem::new(), &log);
    assert!(log.0.lock().unwrap().is_empty());
}
