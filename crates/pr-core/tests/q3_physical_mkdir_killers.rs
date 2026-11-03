//! X-physical-fs (Q3 2026-11-04): killers for survivors of `create_directory_impl` (`io/physical.rs`) that no earlier killer reached.
//! C#: `Directory.CreateDirectory` creates exactly the missing levels, root first, and nothing else; on a drive that does not exist it
//! fails with `DirectoryNotFoundException` (raw 3). Real disk under a private `TempRoot` (native), no sleeps; the walk runs on a
//! worker thread with a bounded wait so a mutant that stops the walk from ending fails the test instead of hanging the run.
use pr_core::abstractions::FileSystem;
use pr_core::io::physical::PhysicalFileSystem;
use pr_test_support::TempRoot;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

const Q: &str = "\u{5c}";

fn bounded<T: Send + 'static>(what: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx.recv_timeout(Duration::from_secs(20))
        .unwrap_or_else(|e| panic!("{what}: create_directory did not finish ({e})"))
}

fn names(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

// physical.rs:693:21 (`i > root_length` of the walk back to the previous separator): when the inner loop does not stop at the
// separator, every character position becomes a "missing folder" and `create_directory` makes prefix folders such as `aa`, `aaa`.
#[test]
#[ignore = "native: real disk, private TempRoot"]
fn create_directory_creates_the_missing_levels_and_no_prefix_folders() {
    let root = TempRoot::new("q3-mkdir-exact");
    let base = root.path().to_path_buf();
    let wanted = base.join("aaaa").join("bbbb").join("cccc");
    let target = wanted.clone();
    bounded("nested", move || PhysicalFileSystem.create_directory(&target)).unwrap();
    assert!(wanted.is_dir());
    assert_eq!(names(&base), ["aaaa"]);
    assert_eq!(names(&base.join("aaaa")), ["bbbb"]);
    assert_eq!(names(&base.join("aaaa").join("bbbb")), ["cccc"]);
}

#[test]
#[ignore = "native: real disk, private TempRoot"]
fn create_directory_below_an_existing_folder_creates_only_the_levels_under_it() {
    let root = TempRoot::new("q3-mkdir-below");
    let base = root.path().to_path_buf();
    std::fs::create_dir(base.join("keep")).unwrap();
    let target = base.join("keep").join("xxxxxx").join("yyyyyy");
    let worker = target.clone();
    bounded("below", move || PhysicalFileSystem.create_directory(&worker)).unwrap();
    assert!(target.is_dir());
    assert_eq!(names(&base), ["keep"]);
    assert_eq!(names(&base.join("keep")), ["xxxxxx"]);
}

#[test]
#[ignore = "native: real disk, private TempRoot"]
fn create_directory_with_a_trailing_separator_creates_the_same_folders_and_no_prefix_folders() {
    let root = TempRoot::new("q3-mkdir-trailing");
    let base = root.path().to_path_buf();
    let spelled = format!("{}{Q}pppp{Q}qqqq{Q}", base.display());
    bounded("trailing", move || {
        PhysicalFileSystem.create_directory(Path::new(&spelled))
    })
    .unwrap();
    assert_eq!(names(&base), ["pppp"]);
    assert_eq!(names(&base.join("pppp")), ["qqqq"]);
}

// physical.rs:686:41 (`i >= root_length && !some_path_exists`): with `||` the walk back keeps going past the root of a drive
// that does not exist and slices below index 0 (a panic); C# reports DirectoryNotFoundException (raw 3).
#[test]
#[ignore = "native: needs a drive letter that has no volume"]
fn create_directory_on_a_missing_drive_fails_with_path_not_found() {
    let letter = ('D'..='Z').find(|c| !Path::new(&format!("{c}:{Q}")).exists());
    let Some(letter) = letter else { return };
    for spelled in [
        format!("{letter}:{Q}a{Q}b"),
        format!("{letter}:{Q}a"),
        format!("{letter}:{Q}a{Q}b{Q}"),
    ] {
        let path = PathBuf::from(&spelled);
        let err = bounded("missing drive", move || PhysicalFileSystem.create_directory(&path)).unwrap_err();
        assert_eq!(err.raw_os_error(), Some(3), "{spelled}");
    }
}
