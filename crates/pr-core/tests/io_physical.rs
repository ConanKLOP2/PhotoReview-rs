//! W2-14 tests for `pr_core::io::physical` (port of `PhysicalFileSystem.cs`, C# baseline d92d6aae).
//!
//! Pure tests (pattern matcher, text decoding, retry loop, error classification, listing logic over an injected listing,
//! FakeFileSystem contract rows) run in the normal run. Every test that touches the real disk through the OS semantics
//! is `#[ignore = "native"]` (run with `cargo test -p pr-core --test io_physical -- --ignored` on Windows); each one works
//! in a private `TempRoot` and removes its junctions before the root is dropped.
#![cfg(windows)]

use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pr_core::abstractions::{CopyCreationProof, FileSystem, SkippedEntry, SkippedKind};
use pr_core::io::physical::{
    ListedFile, PhysicalFileSystem, REPLACE_ATTEMPTS, decode_text_like_dotnet, is_swallowed_as_destination_exists,
    matches_win32_expression, new_guid_n, replace_with_retry,
};
use pr_core::time::DateTimeUtc;
use pr_test_support::golden::{self, bool_of, str_of};
use pr_test_support::{FakeFileSystem, TempRoot};
use serde_json::Value;

const SHARE_NONE: u32 = 0;
const SHARE_READ: u32 = 1;
const SHARE_WRITE: u32 = 2;
const SHARE_DELETE: u32 = 4;
const SHARE_ALL: u32 = SHARE_READ | SHARE_WRITE | SHARE_DELETE;

// ---------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------

fn phys() -> PhysicalFileSystem {
    PhysicalFileSystem
}

/// C# `TempRoot.File(name, bytes)`: writes the file (creating its folder) and returns its full path.
fn file_in(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, bytes).unwrap();
    path
}

/// C# `TempRoot.Dir(name)`.
fn dir_in(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(&path).unwrap();
    path
}

/// An open handle with an explicit access and share mode (C# `new FileStream(path, FileMode.Open, access, share)`).
fn hold(path: &Path, read: bool, write: bool, share: u32) -> File {
    OpenOptions::new()
        .read(read)
        .write(write)
        .share_mode(share)
        .open(path)
        .unwrap_or_else(|e| panic!("hold {}: {e}", path.display()))
}

/// Junction links made by a test, removed with `remove_dir` (the link only, never the target) when dropped. Declare it
/// AFTER the `TempRoot` so it drops first.
#[derive(Default)]
struct Links(Vec<PathBuf>);

impl Links {
    /// `cmd /c mklink /J link target` (no privilege needed).
    fn junction(&mut self, link: &Path, target: &Path) -> PathBuf {
        let output = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap_or_else(|e| panic!("mklink: {e}"));
        assert!(
            output.status.success(),
            "could not create a junction: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        self.0.push(link.to_path_buf());
        link.to_path_buf()
    }
}

impl Drop for Links {
    fn drop(&mut self) {
        for link in self.0.iter().rev() {
            let _ = fs::remove_dir(link);
        }
    }
}

/// The path as the OS spells it (long names, real case), without the `\\?\` that `canonicalize` adds: a root that
/// junction resolution (which goes through the OS) gives back unchanged.
fn real_root(root: &TempRoot) -> PathBuf {
    let canonical = fs::canonicalize(root.path()).unwrap();
    let text = canonical.to_string_lossy().into_owned();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}

fn eq_ignore_case(a: &Path, b: &Path) -> bool {
    pr_core::text::path_eq_ignore_case(a, b)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn tmp_count(dir: &Path) -> usize {
    match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().to_ascii_lowercase().ends_with(".tmp"))
            .count(),
        Err(_) => 0,
    }
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// ---------------------------------------------------------------------------------------------------------------
// replace_with_retry (C# ReplaceWithRetryTests)
// ---------------------------------------------------------------------------------------------------------------

// C#: ReplaceWithRetryTests.SharingViolation_RetriedThenSucceeds
#[test]
fn sharing_violation_retried_then_succeeds() {
    let mut calls = 0;
    let mut sleeps = Vec::new();

    replace_with_retry(
        || {
            calls += 1;
            if calls < 3 {
                Err(io::Error::from_raw_os_error(32))
            } else {
                Ok(())
            }
        },
        |ms| sleeps.push(ms),
    )
    .unwrap();

    assert_eq!(calls, 3);
    assert_eq!(sleeps, vec![3, 6]);
}

// C#: ReplaceWithRetryTests.AccessDenied_RetriedThenRethrown
#[test]
fn access_denied_retried_then_rethrown() {
    let mut calls = 0u32;
    let mut sleeps = Vec::new();

    let err = replace_with_retry(
        || {
            calls += 1;
            Err(io::Error::from_raw_os_error(5))
        },
        |ms| sleeps.push(ms),
    )
    .unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(calls, 8);
    assert_eq!(calls, REPLACE_ATTEMPTS);
    assert_eq!(sleeps, vec![3, 6, 9, 12, 15, 18, 21]);
}

// C#: ReplaceWithRetryTests.AccessDenied_RetriedThenRethrown (a PermissionDenied without raw code is also transient)
#[test]
fn access_denied_without_code_is_retried() {
    let mut calls = 0;
    let _ = replace_with_retry(
        || {
            calls += 1;
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
        },
        |_| {},
    );
    assert_eq!(calls, 8);
}

// C#: ReplaceWithRetryTests.NonTransient_SurfacesImmediately (DirectoryNotFound, PathTooLong, IOException) plus the lock
// violation (33) as a transient row and PRIVILEGE_NOT_HELD (1314, PermissionDenied in std but IOException in .NET).
#[test]
fn non_transient_surfaces_immediately() {
    let rows: Vec<(io::Error, u32)> = vec![
        (io::Error::from_raw_os_error(3), 1),
        (io::Error::from_raw_os_error(206), 1),
        (io::Error::other("io"), 1),
        (io::Error::from_raw_os_error(1314), 1),
        (io::Error::from_raw_os_error(33), 8),
    ];
    for (template, expected_calls) in rows {
        let mut calls = 0;
        let mut sleeps = 0;
        let make = || match template.raw_os_error() {
            Some(code) => io::Error::from_raw_os_error(code),
            None => io::Error::new(template.kind(), template.to_string()),
        };
        let err = replace_with_retry(
            || {
                calls += 1;
                Err(make())
            },
            |_| sleeps += 1,
        )
        .unwrap_err();
        assert_eq!(err.raw_os_error(), template.raw_os_error());
        assert_eq!(calls, expected_calls, "{template}");
        assert_eq!(sleeps, expected_calls - 1, "{template}");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// is_swallowed_as_destination_exists (C# PhysicalFileSystemTryCopyNewTests, theory rows built from raw OS codes)
// ---------------------------------------------------------------------------------------------------------------

// C#: PhysicalFileSystemTryCopyNewTests.IsSwallowedAsDestinationExists_DestinationExistsHResults_AreSwallowed
// (HResult rows are covered by W1-13 `hresult_is_destination_exists`: an io::Error carries no HResult.)
#[test]
fn is_swallowed_as_destination_exists_destination_exists_codes_are_swallowed() {
    assert!(is_swallowed_as_destination_exists(&io::Error::from_raw_os_error(80)));
    assert!(is_swallowed_as_destination_exists(&io::Error::from_raw_os_error(183)));
    // Rust-only row (fakes): AlreadyExists without a code.
    assert!(is_swallowed_as_destination_exists(&io::Error::new(
        io::ErrorKind::AlreadyExists,
        "exists"
    )));
}

// C#: PhysicalFileSystemTryCopyNewTests.IsSwallowedAsDestinationExists_EveryOtherFailure_IsRethrown
// (The "InvalidOperationException with a matching HResult" row has no twin: not an I/O error.)
#[test]
fn is_swallowed_as_destination_exists_every_other_failure_is_rethrown() {
    for err in [
        io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
        io::Error::from_raw_os_error(5),
        io::Error::from_raw_os_error(3),
        io::Error::from_raw_os_error(32),
        io::Error::from_raw_os_error(112),
        io::Error::other("no code"),
        io::Error::new(io::ErrorKind::IsADirectory, "a directory"),
    ] {
        assert!(!is_swallowed_as_destination_exists(&err), "{err}");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Pattern matching and text decoding (pure goldens)
// ---------------------------------------------------------------------------------------------------------------

fn expected_names(case: &Value) -> Vec<String> {
    case["matched"]
        .as_array()
        .unwrap_or_else(|| panic!("no matched: {case}"))
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

// Golden physical-fs-pattern through the matcher alone (file names; the directories are excluded by the listing).
#[test]
fn golden_physical_fs_pattern_matcher() {
    let golden = golden::load("physical-fs-pattern");
    assert_eq!(golden.cases.len(), 56);
    for case in &golden.cases {
        let pattern = str_of(case, "pattern");
        let mut matched: Vec<String> = case["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .filter(|name| matches_win32_expression(pattern, name))
            .collect();
        matched.sort();
        assert_eq!(matched, expected_names(case), "pattern {pattern:?}");
    }
}

#[test]
fn matches_win32_expression_spot_checks() {
    assert!(matches_win32_expression("*.jpg", "a.b.jpg"));
    assert!(!matches_win32_expression("*.jpg", "x.jpg.tmp"));
    assert!(matches_win32_expression("a?.jpg", "a.jpg"));
    assert!(matches_win32_expression("*.", "noext"));
    assert!(!matches_win32_expression("*.", "a.jpg"));
    assert!(matches_win32_expression("ä.jpg", "Ä.JPG"));
    assert!(!matches_win32_expression("?a", "a"));
    assert!(matches_win32_expression(".", "anything"));
}

// .NET `FileSystemName` walks UTF-16 code units: `?` / `>` consume ONE surrogate of an astral character. Expected values
// observed with .NET 10 `FileSystemName.MatchesWin32Expression`.
#[test]
fn matches_win32_expression_counts_utf16_units_like_dotnet() {
    let e = "\u{1F600}";
    assert!(!matches_win32_expression("a?", &format!("a{e}")));
    assert!(matches_win32_expression("a??", &format!("a{e}")));
    assert!(!matches_win32_expression("a?.b", &format!("a{e}.b")));
    assert!(matches_win32_expression("a??.b", &format!("a{e}.b")));
    assert!(!matches_win32_expression("a>", &format!("a{e}")));
    assert!(!matches_win32_expression("*a?", &format!("xa{e}")));
    assert!(matches_win32_expression("*a??", &format!("xa{e}")));
    assert!(matches_win32_expression(&format!("*{e}"), &format!("b{e}")));
    // Deseret: the "ends with" fast path is OrdinalIgnoreCase over whole scalars; the NFA compares unit by unit.
    assert!(matches_win32_expression("*\u{10428}", "x\u{10400}"));
    assert!(!matches_win32_expression("a\u{10428}", "a\u{10400}"));
}

// `Directory.EnumerateFiles("C:")` keeps the drive-relative spelling (.NET 10: `C:a.txt`, but `C:sub` => `C:sub\b.txt`).
// The test process's current directory (the crate folder) is the current directory of its drive.
#[test]
#[ignore = "native"]
fn enumerate_on_bare_drive_relative_directory_keeps_the_spelling() {
    let cwd = std::env::current_dir().unwrap();
    let text = cwd.to_string_lossy().into_owned();
    let drive = &text[..2];
    assert!(
        drive.ends_with(':'),
        "test needs a drive-letter current directory: {text}"
    );
    let fs = phys();
    let files = fs.enumerate_files(Path::new(drive), "Cargo.toml").unwrap();
    assert_eq!(files, vec![PathBuf::from(format!("{drive}Cargo.toml"))]);
    let dirs = fs.enumerate_directories(Path::new(drive)).unwrap();
    assert!(dirs.contains(&PathBuf::from(format!("{drive}src"))), "{dirs:?}");
    assert!(dirs.iter().all(|d| !d.to_string_lossy().contains('\\')), "{dirs:?}");
    let sub = fs.enumerate_files(Path::new(&format!("{drive}src")), "lib.rs").unwrap();
    assert_eq!(sub, vec![PathBuf::from(format!("{drive}src\\lib.rs"))]);
}

// Golden physical-fs-read-text (38 byte sequences) through the decoder alone.
#[test]
fn golden_physical_fs_read_text_decoder() {
    let golden = golden::load("physical-fs-read-text");
    assert_eq!(golden.cases.len(), 38);
    for case in &golden.cases {
        let bytes = unhex(str_of(case, "hex"));
        let expected: Vec<u16> = case["utf16_units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| u16::try_from(v.as_u64().unwrap()).unwrap())
            .collect();
        let units: Vec<u16> = decode_text_like_dotnet(&bytes).encode_utf16().collect();
        assert_eq!(units, expected, "case {}", str_of(case, "name"));
    }
}

// Golden physical-fs-read-text through read_all_text over a real file.
#[test]
#[ignore = "native"]
fn golden_physical_fs_read_text_from_disk() {
    let root = TempRoot::new("physfs-readtext");
    let path = root.join("t.txt");
    for case in &golden::load("physical-fs-read-text").cases {
        fs::write(&path, unhex(str_of(case, "hex"))).unwrap();
        let expected: Vec<u16> = case["utf16_units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| u16::try_from(v.as_u64().unwrap()).unwrap())
            .collect();
        let units: Vec<u16> = phys().read_all_text(&path).unwrap().encode_utf16().collect();
        assert_eq!(units, expected, "case {}", str_of(case, "name"));
    }
}

#[test]
fn new_guid_n_is_unique_lowercase_hex() {
    let all = Arc::new(Mutex::new(Vec::with_capacity(10_000)));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let all = Arc::clone(&all);
            std::thread::spawn(move || {
                let mine: Vec<String> = (0..1250).map(|_| new_guid_n()).collect();
                all.lock().unwrap().extend(mine);
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let mut all = all.lock().unwrap().clone();
    assert_eq!(all.len(), 10_000);
    for g in &all {
        assert_eq!(g.len(), 32, "{g}");
        assert!(g.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')), "{g}");
    }
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 10_000);
}

// Rust-only: the low 64 bits come from the per-process counter (injective), so they never repeat within a process even
// if two high halves collided; uniqueness does not rest on the hash alone.
#[test]
fn new_guid_n_low_half_never_repeats_within_the_process() {
    let mut lows: Vec<String> = (0..10_000).map(|_| new_guid_n()[16..].to_owned()).collect();
    lows.sort();
    lows.dedup();
    assert_eq!(lows.len(), 10_000);
}

// ---------------------------------------------------------------------------------------------------------------
// Listing interrupted (C# PhysicalFileSystemListingInterruptedTests): injected listing, no OS trick
// ---------------------------------------------------------------------------------------------------------------

/// Yields the entries, then the error; sets `dropped` when dropped (C# `ThrowingListing.Disposed`).
struct ThrowingListing {
    entries: std::vec::IntoIter<ListedFile>,
    then: Option<io::Error>,
    dropped: Rc<Cell<bool>>,
}

impl Iterator for ThrowingListing {
    type Item = io::Result<ListedFile>;
    fn next(&mut self) -> Option<Self::Item> {
        match self.entries.next() {
            Some(entry) => Some(Ok(entry)),
            None => self.then.take().map(Err),
        }
    }
}

impl Drop for ThrowingListing {
    fn drop(&mut self) {
        self.dropped.set(true);
    }
}

/// C# `Files(names)`: real files of length 3 in the temp root, as listing entries.
fn listed_files(root: &TempRoot, names: &[&str]) -> Vec<ListedFile> {
    names
        .iter()
        .map(|name| {
            let path = file_in(root.path(), name, &[1, 2, 3]);
            let metadata = fs::metadata(&path).unwrap();
            ListedFile {
                full_name: path,
                length: metadata.len(),
                last_write_utc: metadata
                    .modified()
                    .ok()
                    .and_then(DateTimeUtc::from_system_time)
                    .unwrap(),
            }
        })
        .collect()
}

type Listed = io::Result<Vec<(PathBuf, Option<pr_core::abstractions::FileStat>)>>;

fn list_through(
    files: Vec<ListedFile>,
    then: io::Error,
    directory: &Path,
    include: &dyn Fn(&Path) -> bool,
    skipped: &mut Vec<SkippedEntry>,
) -> (Listed, Rc<Cell<bool>>) {
    let dropped = Rc::new(Cell::new(false));
    // The trait object must be 'static: the listing is moved into the opener by a one-shot cell.
    let listing = std::cell::RefCell::new(Some(ThrowingListing {
        entries: files.into_iter(),
        then: Some(then),
        dropped: Rc::clone(&dropped),
    }));
    let opener = |_: &Path| -> io::Result<Box<dyn Iterator<Item = io::Result<ListedFile>>>> {
        Ok(Box::new(listing.borrow_mut().take().unwrap()))
    };
    let result =
        PhysicalFileSystem::enumerate_files_with_stat_from(directory, include, &mut |s| skipped.push(s), &opener);
    (result, dropped)
}

// C#: PhysicalFileSystemListingInterruptedTests.EnumerateFilesWithStat_ListingBreaksAfterTwoEntries_KeepsThemAndReportsListingInterrupted
// (theory rows IOException and UnauthorizedAccessException => Other and PermissionDenied)
#[test]
fn enumerate_files_with_stat_listing_breaks_after_two_entries_keeps_them_and_reports_listing_interrupted() {
    for kind in [io::ErrorKind::Other, io::ErrorKind::PermissionDenied] {
        let root = TempRoot::new("listing-interrupted");
        let files = listed_files(&root, &["a.jpg", "b.jpg"]);
        let names: Vec<PathBuf> = files.iter().map(|f| f.full_name.clone()).collect();
        let mut skipped = Vec::new();

        let (listed, dropped) = list_through(
            files,
            io::Error::new(kind, "disk went away"),
            root.path(),
            &|_| true,
            &mut skipped,
        );

        let listed = listed.unwrap();
        assert_eq!(listed.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(), names);
        assert!(listed.iter().all(|(_, s)| s.map(|s| s.length) == Some(3)));
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].kind, SkippedKind::ListingInterrupted);
        assert_eq!(skipped[0].path, root.path());
        assert_eq!(skipped[0].reason, "disk went away");
        assert!(dropped.get());
    }
}

// C#: PhysicalFileSystemListingInterruptedTests.EnumerateFilesWithStat_ListingBreaksBeforeTheFirstEntry_PropagatesAndReportsNothing
#[test]
fn enumerate_files_with_stat_listing_breaks_before_the_first_entry_propagates_and_reports_nothing() {
    let root = TempRoot::new("listing-interrupted");
    let mut skipped = Vec::new();

    let (listed, dropped) = list_through(
        Vec::new(),
        io::Error::other("folder unreadable"),
        root.path(),
        &|_| true,
        &mut skipped,
    );

    assert_eq!(listed.unwrap_err().to_string(), "folder unreadable");
    assert!(skipped.is_empty()); // an unreadable folder must fail the load loudly, not look like an empty catalog
    assert!(dropped.get());
}

// C#: PhysicalFileSystemListingInterruptedTests.EnumerateFilesWithStat_NonIoFailureMidListing_Propagates
// No Rust twin: every listing failure is an io::Error, i.e. an I/O error in the C# sense.

// C#: PhysicalFileSystemListingInterruptedTests.EnumerateFilesWithStat_IncludeFilterRejectsSome_ListsOnlyTheIncludedBeforeTheInterruption
#[test]
fn enumerate_files_with_stat_include_filter_rejects_some_lists_only_the_included_before_the_interruption() {
    let root = TempRoot::new("listing-interrupted");
    let files = listed_files(&root, &["keep.jpg", "drop.txt", "keep2.jpg"]);
    let expected = vec![files[0].full_name.clone(), files[2].full_name.clone()];
    let mut skipped = Vec::new();

    let (listed, _) = list_through(
        files,
        io::Error::other("cut"),
        root.path(),
        &|p| p.to_string_lossy().ends_with(".jpg"),
        &mut skipped,
    );

    assert_eq!(
        listed.unwrap().into_iter().map(|(p, _)| p).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].kind, SkippedKind::ListingInterrupted);
}

// An interruption after entries that the include filter all rejected still counts them as read (C# `listed` counts before
// `include`).
#[test]
fn enumerate_files_with_stat_interruption_after_excluded_entries_is_reported() {
    let root = TempRoot::new("listing-interrupted");
    let files = listed_files(&root, &["drop.txt"]);
    let mut skipped = Vec::new();

    let (listed, _) = list_through(files, io::Error::other("cut"), root.path(), &|_| false, &mut skipped);

    assert!(listed.unwrap().is_empty());
    assert_eq!(skipped.len(), 1);
}

// ---------------------------------------------------------------------------------------------------------------
// FileSystem contract (C# FileSystemContractTests / FileSystemCollisionContractTests): one body per theory, run for the
// physical file system (native) and for FakeFileSystem.
// ---------------------------------------------------------------------------------------------------------------

/// A file system under test plus its root folder.
struct Harness {
    fs: Box<dyn FileSystem>,
    root: PathBuf,
    is_physical: bool,
    _temp: Option<TempRoot>,
}

impl Harness {
    fn physical() -> Self {
        let temp = TempRoot::new("fs-contract");
        Self {
            fs: Box::new(PhysicalFileSystem),
            root: temp.path().to_path_buf(),
            is_physical: true,
            _temp: Some(temp),
        }
    }

    fn fake() -> Self {
        let fake = FakeFileSystem::new();
        let root = PathBuf::from(r"C:\VirtualRoot");
        fake.create_directory(&root).unwrap();
        Self {
            fs: Box::new(fake),
            root,
            is_physical: false,
            _temp: None,
        }
    }

    fn combine(&self, parts: &[&str]) -> PathBuf {
        parts.iter().fold(self.root.clone(), |acc, p| acc.join(p))
    }
}

fn upper(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_uppercase())
}

// C#: FileSystemContractTests.FileExistsReturnsTrueWhenPresentAndFalseWhenMissing
fn file_exists_returns_true_when_present_and_false_when_missing(h: &Harness) {
    let file = h.combine(&["test.txt"]);
    assert!(!h.fs.file_exists(&file));
    h.fs.write_all_text_atomic(&file, "sample content", true).unwrap();
    assert!(h.fs.file_exists(&file));
    assert!(h.fs.file_exists(&upper(&file)));
}

// C#: FileSystemContractTests.DirectoryExistsReturnsTrueWhenPresentAndFalseWhenMissing
fn directory_exists_returns_true_when_present_and_false_when_missing(h: &Harness) {
    let sub = h.combine(&["SubFolder"]);
    assert!(!h.fs.directory_exists(&sub));
    h.fs.create_directory(&sub).unwrap();
    assert!(h.fs.directory_exists(&sub));
    assert!(h.fs.directory_exists(&upper(&sub)));
}

// C#: FileSystemContractTests.TryDeleteEmptyDirectoryRemovesOnlyAnEmptyDirectory
fn try_delete_empty_directory_removes_only_an_empty_directory(h: &Harness) {
    let empty = h.combine(&["Empty"]);
    let full = h.combine(&["Full"]);
    let with_sub = h.combine(&["WithSub"]);
    h.fs.create_directory(&empty).unwrap();
    h.fs.create_directory(&full).unwrap();
    h.fs.write_all_text_atomic(&full.join("keep.txt"), "keep", true)
        .unwrap();
    h.fs.create_directory(&with_sub.join("Nested")).unwrap();

    assert!(!h.fs.try_delete_empty_directory(&h.combine(&["Missing"])));
    assert!(!h.fs.try_delete_empty_directory(&full));
    assert!(!h.fs.try_delete_empty_directory(&with_sub));
    assert!(h.fs.try_delete_empty_directory(&empty));

    assert!(!h.fs.directory_exists(&empty));
    assert!(h.fs.directory_exists(&full));
    assert!(h.fs.file_exists(&full.join("keep.txt")));
    assert!(h.fs.directory_exists(&with_sub.join("Nested")));
}

// C#: FileSystemContractTests.GetFileStatReturnsMetadataForExistingFileAndNullForMissing
fn get_file_stat_returns_metadata_for_existing_file_and_null_for_missing(h: &Harness) {
    let file = h.combine(&["stat-test.txt"]);
    assert!(h.fs.get_file_stat(&file).is_none());
    let content = "Hello UTF-8 World Tiếng Việt";
    h.fs.write_all_text_atomic(&file, content, true).unwrap();
    let stat = h.fs.get_file_stat(&file).unwrap();
    assert_eq!(stat.length, content.len() as u64);
    let five_minutes_ago = DateTimeUtc::now().checked_add_ticks(-5 * 60 * 10_000_000).unwrap();
    assert!(stat.last_write_utc > five_minutes_ago);
}

// C#: FileSystemContractTests.MoveRelocatesFileAndRemovesSource
fn move_relocates_file_and_removes_source(h: &Harness) {
    let source = h.combine(&["source.txt"]);
    let destination = h.combine(&["destination.txt"]);
    h.fs.write_all_text_atomic(&source, "data to move", true).unwrap();
    h.fs.move_file(&source, &destination).unwrap();
    assert!(!h.fs.file_exists(&source));
    assert!(h.fs.file_exists(&destination));
    assert_eq!(h.fs.read_all_text(&destination).unwrap(), "data to move");
}

// C#: FileSystemContractTests.MoveThrowsWhenSourceDoesNotExist
fn move_throws_when_source_does_not_exist(h: &Harness) {
    let err =
        h.fs.move_file(&h.combine(&["non-existent.txt"]), &h.combine(&["dst.txt"]))
            .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    if h.is_physical {
        assert_eq!(err.raw_os_error(), Some(2));
    }
}

// C#: FileSystemContractTests.CopyDuplicatesFilePreservingSource
fn copy_duplicates_file_preserving_source(h: &Harness) {
    let source = h.combine(&["source.txt"]);
    let destination = h.combine(&["copied.txt"]);
    h.fs.write_all_text_atomic(&source, "shared data", true).unwrap();
    h.fs.copy_file(&source, &destination).unwrap();
    assert!(h.fs.file_exists(&source));
    assert!(h.fs.file_exists(&destination));
    assert_eq!(h.fs.read_all_text(&destination).unwrap(), "shared data");
    assert_eq!(h.fs.read_all_text(&source).unwrap(), "shared data");
}

// C#: FileSystemContractTests.DeleteRemovesFileAndIsIdempotentOnMissingFile
fn delete_removes_file_and_is_idempotent_on_missing_file(h: &Harness) {
    let file = h.combine(&["delete-me.txt"]);
    h.fs.write_all_text_atomic(&file, "to be deleted", true).unwrap();
    assert!(h.fs.file_exists(&file));
    h.fs.delete(&file).unwrap();
    assert!(!h.fs.file_exists(&file));
    h.fs.delete(&file).unwrap();
}

// C#: FileSystemContractTests.OpenReadSharedAllowsReadingEntireContent
fn open_read_shared_allows_reading_entire_content(h: &Harness) {
    let file = h.combine(&["shared-read.bin"]);
    let payload = [0x01u8, 0x02, 0x03, 0x04, 0xAA, 0xBB, 0xCC];
    {
        let mut append = h.fs.open_append(&file, true).unwrap();
        append.write_all(&payload).unwrap();
    }
    let mut read = h.fs.open_read_shared(&file, 65536).unwrap();
    let mut bytes = Vec::new();
    read.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, payload);
}

// C#: FileSystemContractTests.OpenAppendAppendsSequentially
fn open_append_appends_sequentially(h: &Harness) {
    let file = h.combine(&["journal.jsonl"]);
    for line in ["Line 1\n", "Line 2\n"] {
        let mut stream = h.fs.open_append(&file, true).unwrap();
        stream.write_all(line.as_bytes()).unwrap();
    }
    let text = h.fs.read_all_text(&file).unwrap();
    let lines: Vec<&str> = text
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| l.trim_end_matches('\r'))
        .collect();
    assert_eq!(lines, vec!["Line 1", "Line 2"]);
}

// C#: FileSystemContractTests.WriteAllTextAtomicAndReadAllTextRoundTrip
fn write_all_text_atomic_and_read_all_text_round_trip(h: &Harness) {
    let file = h.combine(&["nested", "dir", "atomic.txt"]);
    let text = "Dữ liệu thử nghiệm có dấu và ký tự đặc biệt !@#$%^&*()";
    h.fs.write_all_text_atomic(&file, text, true).unwrap();
    assert_eq!(h.fs.read_all_text(&file).unwrap(), text);
}

fn names_of(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

// C#: FileSystemContractTests.EnumerateFilesMatchesPatternAndRespectsTopDirectoryOnly
fn enumerate_files_matches_pattern_and_respects_top_directory_only(h: &Harness) {
    let dir = h.combine(&["Gallery"]);
    h.fs.create_directory(&dir).unwrap();
    let sub = dir.join("Nested");
    h.fs.create_directory(&sub).unwrap();
    for (name, text) in [("photo1.jpg", "1"), ("photo2.png", "2"), ("document.pdf", "3")] {
        h.fs.write_all_text_atomic(&dir.join(name), text, true).unwrap();
    }
    h.fs.write_all_text_atomic(&sub.join("photo3.jpg"), "sub", true)
        .unwrap();

    let mut all = names_of(&h.fs.enumerate_files(&dir, "*").unwrap());
    all.sort();
    assert_eq!(all, vec!["document.pdf", "photo1.jpg", "photo2.png"]);
    assert_eq!(
        names_of(&h.fs.enumerate_files(&dir, "*.jpg").unwrap()),
        vec!["photo1.jpg"]
    );
}

// C#: FileSystemContractTests.EnumerateDirectoriesListsImmediateSubdirectories
fn enumerate_directories_lists_immediate_subdirectories(h: &Harness) {
    let parent = h.combine(&["Parent"]);
    h.fs.create_directory(&parent.join("Child1")).unwrap();
    h.fs.create_directory(&parent.join("Child2")).unwrap();
    let mut dirs = names_of(&h.fs.enumerate_directories(&parent).unwrap());
    dirs.sort();
    assert_eq!(dirs, vec!["Child1", "Child2"]);
}

// C#: FileSystemContractTests.TryProbeReadable_ExistingFileIsReadable_MissingFileReportsAReason
fn try_probe_readable_existing_file_is_readable_missing_file_reports_a_reason(h: &Harness) {
    let present = h.combine(&["present.jpg"]);
    h.fs.write_all_text_atomic(&present, "img", true).unwrap();
    assert_eq!(h.fs.try_probe_readable(&present), Ok(()));
    let reason = h.fs.try_probe_readable(&h.combine(&["missing.jpg"])).unwrap_err();
    assert!(!reason.trim().is_empty());
}

// C#: FileSystemCollisionContractTests.Move_DestinationExists_ThrowsAndKeepsBoth
fn move_destination_exists_throws_and_keeps_both(h: &Harness) {
    let source = h.combine(&["a.txt"]);
    let destination = h.combine(&["b.txt"]);
    h.fs.write_all_text_atomic(&source, "source-bytes", true).unwrap();
    h.fs.write_all_text_atomic(&destination, "other-bytes", true).unwrap();
    assert!(h.fs.move_file(&source, &destination).is_err());
    assert_eq!(h.fs.read_all_text(&source).unwrap(), "source-bytes");
    assert_eq!(h.fs.read_all_text(&destination).unwrap(), "other-bytes");
}

// C#: FileSystemCollisionContractTests.Move_DestinationDiffersByCaseOnly_IsACollision
fn move_destination_differs_by_case_only_is_a_collision(h: &Harness) {
    let source = h.combine(&["a.txt"]);
    h.fs.write_all_text_atomic(&source, "source-bytes", true).unwrap();
    h.fs.write_all_text_atomic(&h.combine(&["Photo.JPG"]), "existing", true)
        .unwrap();
    assert!(h.fs.file_exists(&h.combine(&["photo.jpg"])));
    assert!(h.fs.move_file(&source, &h.combine(&["photo.jpg"])).is_err());
    assert_eq!(h.fs.read_all_text(&h.combine(&["Photo.JPG"])).unwrap(), "existing");
    assert!(h.fs.file_exists(&source));
}

// C#: FileSystemCollisionContractTests.Copy_DestinationExists_ThrowsAndKeepsBoth
fn copy_destination_exists_throws_and_keeps_both(h: &Harness) {
    let source = h.combine(&["a.txt"]);
    let destination = h.combine(&["b.txt"]);
    h.fs.write_all_text_atomic(&source, "source-bytes", true).unwrap();
    h.fs.write_all_text_atomic(&destination, "other-bytes", true).unwrap();
    assert!(h.fs.copy_file(&source, &destination).is_err());
    assert_eq!(h.fs.read_all_text(&destination).unwrap(), "other-bytes");
    assert_eq!(h.fs.read_all_text(&source).unwrap(), "source-bytes");
}

// C#: FileSystemCollisionContractTests.TryCopyNew_DestinationExists_ReturnsFalseAndKeepsBoth
fn try_copy_new_destination_exists_returns_false_and_keeps_both(h: &Harness) {
    let source = h.combine(&["a.txt"]);
    let destination = h.combine(&["b.txt"]);
    h.fs.write_all_text_atomic(&source, "source-bytes", true).unwrap();
    h.fs.write_all_text_atomic(&destination, "other-bytes", true).unwrap();
    assert!(!h.fs.try_copy_new(&source, &destination).unwrap());
    assert_eq!(h.fs.read_all_text(&destination).unwrap(), "other-bytes");
    assert_eq!(h.fs.read_all_text(&source).unwrap(), "source-bytes");
}

// C#: FileSystemCollisionContractTests.TryCopyNew_DestinationFree_CopiesAndReturnsTrue
fn try_copy_new_destination_free_copies_and_returns_true(h: &Harness) {
    let source = h.combine(&["a.txt"]);
    let destination = h.combine(&["b.txt"]);
    h.fs.write_all_text_atomic(&source, "source-bytes", true).unwrap();
    assert!(h.fs.try_copy_new(&source, &destination).unwrap());
    assert_eq!(h.fs.read_all_text(&destination).unwrap(), "source-bytes");
    assert_eq!(h.fs.read_all_text(&source).unwrap(), "source-bytes");
}

// C#: FileSystemCollisionContractTests.Move_MissingDestinationFolder_KeepsSource
fn move_missing_destination_folder_keeps_source(h: &Harness) {
    let source = h.combine(&["a.txt"]);
    h.fs.write_all_text_atomic(&source, "source-bytes", true).unwrap();
    let err = h.fs.move_file(&source, &h.combine(&["nope", "a.txt"])).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    if h.is_physical {
        assert_eq!(err.raw_os_error(), Some(3));
    }
    assert_eq!(h.fs.read_all_text(&source).unwrap(), "source-bytes");
}

// C#: FileSystemCollisionContractTests.WriteAllTextAtomic_Overwrite_ReplacesWholeContent
fn write_all_text_atomic_overwrite_replaces_whole_content(h: &Harness) {
    let path = h.combine(&["s.json"]);
    h.fs.write_all_text_atomic(&path, &"x".repeat(5000), true).unwrap();
    h.fs.write_all_text_atomic(&path, "short", true).unwrap();
    assert_eq!(h.fs.read_all_text(&path).unwrap(), "short");
}

/// Every contract row, by name.
/// A contract row: its name and its body.
type ContractRow = (&'static str, fn(&Harness));

const CONTRACT_ROWS: &[ContractRow] = &[
    (
        "file_exists",
        file_exists_returns_true_when_present_and_false_when_missing,
    ),
    (
        "directory_exists",
        directory_exists_returns_true_when_present_and_false_when_missing,
    ),
    (
        "try_delete_empty_directory",
        try_delete_empty_directory_removes_only_an_empty_directory,
    ),
    (
        "get_file_stat",
        get_file_stat_returns_metadata_for_existing_file_and_null_for_missing,
    ),
    ("move", move_relocates_file_and_removes_source),
    ("move_missing_source", move_throws_when_source_does_not_exist),
    ("copy", copy_duplicates_file_preserving_source),
    ("delete", delete_removes_file_and_is_idempotent_on_missing_file),
    ("open_read_shared", open_read_shared_allows_reading_entire_content),
    ("open_append", open_append_appends_sequentially),
    (
        "write_read_round_trip",
        write_all_text_atomic_and_read_all_text_round_trip,
    ),
    (
        "enumerate_files",
        enumerate_files_matches_pattern_and_respects_top_directory_only,
    ),
    (
        "enumerate_directories",
        enumerate_directories_lists_immediate_subdirectories,
    ),
    (
        "try_probe_readable",
        try_probe_readable_existing_file_is_readable_missing_file_reports_a_reason,
    ),
    ("collision_move", move_destination_exists_throws_and_keeps_both),
    (
        "collision_move_case",
        move_destination_differs_by_case_only_is_a_collision,
    ),
    ("collision_copy", copy_destination_exists_throws_and_keeps_both),
    (
        "collision_try_copy_new",
        try_copy_new_destination_exists_returns_false_and_keeps_both,
    ),
    (
        "try_copy_new_free",
        try_copy_new_destination_free_copies_and_returns_true,
    ),
    ("move_missing_folder", move_missing_destination_folder_keeps_source),
    (
        "atomic_overwrite",
        write_all_text_atomic_overwrite_replaces_whole_content,
    ),
];

/// Rows FakeFileSystem (pr-test-support, lead-owned) does not honour yet; reported to the lead, skipped for the fake only:
/// - `get_file_stat`: the fake stamps files with its FakeClock time, not "now" (C# InMemoryFileSystem uses the real clock);
/// - `write_read_round_trip`: the fake's `write_all_text_atomic` does not create missing parent folders;
/// - `try_probe_readable`: the fake reports a missing file as readable (only `set_unreadable` paths fail).
const FAKE_GAPS: &[&str] = &["get_file_stat", "write_read_round_trip", "try_probe_readable"];

// C#: FileSystemContractTests.* and FileSystemCollisionContractTests.* (theories), PhysicalFileSystem rows.
#[test]
#[ignore = "native"]
fn file_system_contract_physical() {
    for (name, row) in CONTRACT_ROWS {
        eprintln!("contract row (physical): {name}");
        row(&Harness::physical());
    }
}

// C#: FileSystemContractTests.* and FileSystemCollisionContractTests.* (theories), InMemoryFileSystem rows => FakeFileSystem.
#[test]
fn file_system_contract_fake() {
    for (name, row) in CONTRACT_ROWS {
        if FAKE_GAPS.contains(name) {
            continue;
        }
        eprintln!("contract row (fake): {name}");
        row(&Harness::fake());
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Physical-only facts (native)
// ---------------------------------------------------------------------------------------------------------------

// C#: FileSystemCollisionContractTests.LongPath_FullLifecycle
#[test]
#[ignore = "native"]
fn long_path_full_lifecycle() {
    let root = TempRoot::new("fs-collision");
    let fs = phys();
    let deep = (0..8).fold(root.path().to_path_buf(), |acc, _| acc.join("d".repeat(40)));
    fs.create_directory(&deep).unwrap();
    let file = deep.join(format!("{}.jpg", "f".repeat(60)));
    assert!(file.as_os_str().len() > 300);

    fs.write_all_text_atomic(&file, "long", true).unwrap();
    fs.copy_file(&file, &deep.join("copy.jpg")).unwrap();
    fs.move_file(&file, &deep.join("moved.jpg")).unwrap();

    assert_eq!(fs.get_file_stat(&deep.join("moved.jpg")).map(|s| s.length), Some(4));
    assert!(!fs.file_exists(&file));
    fs.delete(&deep.join("copy.jpg")).unwrap();
    assert!(!fs.file_exists(&deep.join("copy.jpg")));
}

// C#: FileSystemCollisionContractTests.Copy_DestinationLocked_FailsAndKeepsSource
#[test]
#[ignore = "native"]
fn copy_destination_locked_fails_and_keeps_source() {
    let root = TempRoot::new("fs-collision");
    let fs = phys();
    let source = file_in(root.path(), "a.txt", &[1, 2, 3]);
    let destination = file_in(root.path(), "b.txt", &[9]);
    let _lock = hold(&destination, true, true, SHARE_NONE);

    let err = fs.copy_file(&source, &destination).unwrap_err();

    assert_eq!(err.raw_os_error(), Some(80)); // existence is checked first
    assert_eq!(fs.get_file_stat(&source).map(|s| s.length), Some(3));
}

/// Clears the read-only flag on drop (C# `finally` of `Move_ReadOnlySource_SameVolume_Works`).
struct ClearReadOnly(Vec<PathBuf>);

impl Drop for ClearReadOnly {
    fn drop(&mut self) {
        for path in &self.0 {
            if let Ok(metadata) = fs::metadata(path) {
                let mut permissions = metadata.permissions();
                #[allow(clippy::permissions_set_readonly_false)] // Windows only: clears FILE_ATTRIBUTE_READONLY
                permissions.set_readonly(false);
                let _ = fs::set_permissions(path, permissions);
            }
        }
    }
}

fn set_readonly(path: &Path) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).unwrap();
}

// C#: FileSystemCollisionContractTests.Move_ReadOnlySource_SameVolume_Works
#[test]
#[ignore = "native"]
fn move_read_only_source_same_volume_works() {
    let root = TempRoot::new("fs-collision");
    let fs = phys();
    let source = file_in(root.path(), "ro.txt", &[1]);
    set_readonly(&source);
    let destination = root.join("moved.txt");
    let _cleanup = ClearReadOnly(vec![source.clone(), destination.clone()]);

    fs.move_file(&source, &destination).unwrap();

    assert!(!fs.file_exists(&source));
    assert!(fs::metadata(&destination).unwrap().permissions().readonly());
}

// C#: FileSystemCollisionContractTests.WriteAllTextAtomic_ConcurrentWriters_LeaveACompleteFile
#[test]
#[ignore = "native"]
fn write_all_text_atomic_concurrent_writers_leave_a_complete_file() {
    let root = TempRoot::new("fs-collision");
    let path = Arc::new(root.join("settings.json"));
    let texts: Vec<String> = (0..4u8)
        .map(|i| char::from(b'a' + i).to_string().repeat(20_000))
        .collect();
    let start = Arc::new(Barrier::new(texts.len()));
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));

    let threads: Vec<_> = texts
        .iter()
        .cloned()
        .map(|text| {
            let (path, start, failures) = (Arc::clone(&path), Arc::clone(&start), Arc::clone(&failures));
            std::thread::spawn(move || {
                start.wait();
                for _ in 0..25 {
                    if let Err(e) = PhysicalFileSystem.write_all_text_atomic(&path, &text, false) {
                        failures.lock().unwrap().push(e.to_string());
                    }
                }
            })
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(30);
    for t in threads {
        while !t.is_finished() {
            assert!(Instant::now() < deadline, "writer thread hung");
            std::thread::yield_now();
        }
        t.join().unwrap();
    }

    assert!(failures.lock().unwrap().is_empty(), "{:?}", failures.lock().unwrap());
    assert!(texts.contains(&phys().read_all_text(&path).unwrap()));
    assert_eq!(tmp_count(root.path()), 0);
}

// C#: PhysicalFileSystemTryCopyNewTests.TryCopyNew_DestinationIsADirectory_ThrowsInsteadOfReportingFalse
#[test]
#[ignore = "native"]
fn try_copy_new_destination_is_a_directory_throws_instead_of_reporting_false() {
    let root = TempRoot::new("trycopynew-dir");
    let source = file_in(root.path(), "a.txt", &[1, 2, 3]);
    let destination = dir_in(root.path(), "b.txt");

    assert!(phys().try_copy_new(&source, &destination).is_err());

    assert!(destination.is_dir());
    assert_eq!(fs::read(&source).unwrap(), vec![1, 2, 3]);
}

// C#: PhysicalFileSystemTryCopyNewTests.TryCopyNew_DestinationFolderDeniesCreatingFiles_ThrowsAccessDeniedAndCreatesNothing
// not ported (ACL): it needs a Deny ACE through a Win32 ACL API. The raw OS error 5 of CopyFileExW is returned as is
// (only 80/183 are swallowed and only when the destination is not a directory).

// C#: PhysicalFileSystemTryCopyNewTests.TryCopyNew_ExistingFileAtDestination_ReturnsFalseAndKeepsItUntouched
#[test]
#[ignore = "native"]
fn try_copy_new_existing_file_at_destination_returns_false_and_keeps_it_untouched() {
    let root = TempRoot::new("trycopynew-exists");
    let source = file_in(root.path(), "a.txt", &[1, 2, 3]);
    let destination = file_in(root.path(), "b.txt", &[9]);

    assert!(!phys().try_copy_new(&source, &destination).unwrap());

    assert_eq!(fs::read(&destination).unwrap(), vec![9]);
}

// C#: PhysicalFileSystemTryCopyNewTests.TryCopyNew_FailsForADirectoryDestination_LeavesNoFileBehind
#[test]
#[ignore = "native"]
fn try_copy_new_fails_for_a_directory_destination_leaves_no_file_behind() {
    let root = TempRoot::new("trycopynew-nofile");
    let source = file_in(root.path(), "a.txt", &[1, 2, 3]);
    let destination = dir_in(root.path(), "b.txt");

    assert!(phys().try_copy_new(&source, &destination).is_err());

    assert!(!phys().file_exists(&destination));
    assert!(file_names(&destination).is_empty());
    assert_eq!(file_names(root.path()), vec!["a.txt", "b.txt"]);
}

// C#: PhysicalFileSystemAtomicWriteTests.WriteAllTextAtomic_RenameFails_LeavesNoTempFile
#[test]
#[ignore = "native"]
fn write_all_text_atomic_rename_fails_leaves_no_temp_file() {
    let root = TempRoot::new("atomic-write");
    let target = dir_in(root.path(), "session.json");

    assert!(phys().write_all_text_atomic(&target, "{}", false).is_err());

    assert_eq!(tmp_count(root.path()), 0);
    assert!(target.is_dir());
}

// C#: SessionWritePolicyTests.PhysicalNonDurableWrite_IsAtomicRoundTripAndLeavesNoTempFiles
#[test]
#[ignore = "native"]
fn physical_non_durable_write_is_atomic_round_trip_and_leaves_no_temp_files() {
    let root = TempRoot::new("session-write-policy");
    let path = root.join("sub").join("state.json");

    phys().write_all_text_atomic(&path, "first", false).unwrap();
    phys().write_all_text_atomic(&path, "second \u{e9}", false).unwrap();

    assert_eq!(phys().read_all_text(&path).unwrap(), "second \u{e9}");
    assert_eq!(file_names(&root.join("sub")), vec!["state.json"]);
}

// C#: SessionWritePolicyTests.PhysicalNonDurableWrite_FailedRename_CleansUpTempFile
// (Measure_SessionSaveLatency_DurableVersusNonDurable is Manual in C#: not ported.)
#[test]
#[ignore = "native"]
fn physical_non_durable_write_failed_rename_cleans_up_temp_file() {
    let root = TempRoot::new("session-write-policy");
    let dir = dir_in(root.path(), "d");
    let target = dir_in(&dir, "target");

    let err = phys().write_all_text_atomic(&target, "x", false).unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(err.raw_os_error(), Some(5));
    assert_eq!(file_names(&dir), vec!["target"]); // no file, only the directory in the way
}

// C#: PhysicalIoMutationGapTests.TryCopyNew_SourceMissing_ThrowsInsteadOfReportingAnExistingDestination
#[test]
#[ignore = "native"]
fn try_copy_new_source_missing_throws_instead_of_reporting_an_existing_destination() {
    let root = TempRoot::new("physical-io-gap");
    let destination = root.join("copy.jpg");

    let err = phys()
        .try_copy_new(&root.join("missing.jpg"), &destination)
        .unwrap_err();

    assert_eq!(err.raw_os_error(), Some(2));
    assert!(!destination.exists());
}

// C#: PhysicalIoMutationGapTests.TryCopyNew_DestinationFolderMissing_Throws
#[test]
#[ignore = "native"]
fn try_copy_new_destination_folder_missing_throws() {
    let root = TempRoot::new("physical-io-gap");
    let source = file_in(root.path(), "a.jpg", &[1]);

    let err = phys()
        .try_copy_new(&source, &root.join("nodir").join("a.jpg"))
        .unwrap_err();

    assert_eq!(err.raw_os_error(), Some(3));
}

// C#: PhysicalIoMutationGapTests.TryCopyNew_DestinationIsADirectory_ThrowsInsteadOfReturningFalse
#[test]
#[ignore = "native"]
fn try_copy_new_destination_is_a_directory_throws_instead_of_returning_false() {
    let root = TempRoot::new("physical-io-gap");
    let source = file_in(root.path(), "a.jpg", &[1]);
    let directory = dir_in(root.path(), "taken");

    assert!(phys().try_copy_new(&source, &directory).is_err());
}

// C#: PhysicalIoMutationGapTests.TryCopyNew_DestinationFileExists_ReturnsFalseAndKeepsItsContent
#[test]
#[ignore = "native"]
fn try_copy_new_destination_file_exists_returns_false_and_keeps_its_content() {
    let root = TempRoot::new("physical-io-gap");
    let source = file_in(root.path(), "a.jpg", &[1]);
    let destination = file_in(root.path(), "b.jpg", &[9, 9]);

    assert!(!phys().try_copy_new(&source, &destination).unwrap());
    assert_eq!(fs::read(&destination).unwrap(), vec![9, 9]);
}

// C#: PhysicalIoMutationGapTests.TryProbeReadable_FileAlsoOpenForWriting_IsReadableBecauseTheProbeSharesReadWriteAndDelete
#[test]
#[ignore = "native"]
fn try_probe_readable_file_also_open_for_writing_is_readable_because_the_probe_shares_read_write_and_delete() {
    let root = TempRoot::new("physical-io-gap");
    let path = file_in(root.path(), "busy.jpg", &[1]);
    let _writer = hold(&path, false, true, SHARE_ALL);

    assert_eq!(phys().try_probe_readable(&path), Ok(()));
}

// C#: PhysicalIoMutationGapTests.TryProbeReadable_FileLockedAgainstReaders_ReportsTheOsMessage
#[test]
#[ignore = "native"]
fn try_probe_readable_file_locked_against_readers_reports_the_os_message() {
    let root = TempRoot::new("physical-io-gap");
    let path = file_in(root.path(), "locked.jpg", &[1]);
    let _exclusive = hold(&path, true, false, SHARE_NONE);

    let reason = phys().try_probe_readable(&path).unwrap_err();
    assert!(!reason.is_empty());
}

// C#: PhysicalIoMutationGapTests.EnumerateReadableFilesWithStat_OneFileLocked_IsSkippedAndReportedWhileTheOthersAreListed
#[test]
#[ignore = "native"]
fn enumerate_readable_files_with_stat_one_file_locked_is_skipped_and_reported_while_the_others_are_listed() {
    let root = TempRoot::new("physical-io-gap");
    let good = file_in(root.path(), "good.jpg", &[1, 2]);
    let locked = file_in(root.path(), "locked.jpg", &[1]);
    let _exclusive = hold(&locked, true, false, SHARE_NONE);
    let mut skipped = Vec::new();

    let listed = phys()
        .enumerate_readable_files_with_stat(root.path(), &|_| true, &mut |s| skipped.push(s))
        .unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].0, good);
    assert_eq!(listed[0].1.map(|s| s.length), Some(2));
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].path, locked);
    assert_eq!(skipped[0].kind, SkippedKind::FileUnreadable);
    assert!(!skipped[0].reason.is_empty());
}

// C#: PhysicalIoMutationGapTests.EnumerateFilesWithStat_FolderDoesNotExist_Propagates
#[test]
#[ignore = "native"]
fn enumerate_files_with_stat_folder_does_not_exist_propagates() {
    let root = TempRoot::new("physical-io-gap");
    let mut skipped = Vec::new();

    let err = phys()
        .enumerate_files_with_stat(&root.join("nope"), &|_| true, &mut |s| skipped.push(s))
        .unwrap_err();

    assert_eq!(err.raw_os_error(), Some(3));
    assert!(skipped.is_empty());
}

// C#: PhysicalIoMutationGapTests.ResolveRealPath_PlainPath_IsReturnedUnchanged
#[test]
#[ignore = "native"]
fn resolve_real_path_plain_path_is_returned_unchanged() {
    let root = TempRoot::new("physical-io-gap");
    let folder = dir_in(root.path(), "plain");
    let file = file_in(root.path(), r"plain\a.jpg", &[1]);

    assert!(eq_ignore_case(&phys().resolve_real_path(&file).unwrap(), &file));
    let future = folder.join("future").join("x.jpg");
    assert!(eq_ignore_case(&phys().resolve_real_path(&future).unwrap(), &future));
}

// C#: PhysicalIoMutationGapTests.ResolveRealPath_BareRoot_KeepsTheTrailingSeparator
#[test]
#[ignore = "native"]
fn resolve_real_path_bare_root_keeps_the_trailing_separator() {
    let resolved = phys().resolve_real_path(Path::new(r"C:\")).unwrap();
    assert_eq!(resolved, PathBuf::from(r"C:\"));
    assert!(resolved.to_string_lossy().ends_with('\\'));
}

// C#: SkippedKindTests.LockedFile_IsReportedAsFileUnreadable_WithTheOsMessageKept
#[test]
#[ignore = "native"]
fn locked_file_is_reported_as_file_unreadable_with_the_os_message_kept() {
    let root = TempRoot::new("skipped-kind");
    let locked = file_in(root.path(), "locked.jpg", &[1, 2, 3]);
    let _hold = hold(&locked, true, true, SHARE_NONE);
    let mut skipped = Vec::new();

    let readable = phys()
        .enumerate_readable_files_with_stat(root.path(), &|_| true, &mut |s| skipped.push(s))
        .unwrap();

    assert!(readable.is_empty());
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].path, locked);
    assert_eq!(skipped[0].kind, SkippedKind::FileUnreadable);
    assert!(!skipped[0].reason.trim().is_empty());
}

// C#: SkippedKindTests.FastListing_DoesNotProbe_LockedFileIsListed_AndTheProbeReportsIt
#[test]
#[ignore = "native"]
fn fast_listing_does_not_probe_locked_file_is_listed_and_the_probe_reports_it() {
    let root = TempRoot::new("skipped-kind");
    let locked = file_in(root.path(), "locked.jpg", &[1, 2, 3]);
    let _hold = hold(&locked, true, true, SHARE_NONE);
    let mut skipped = Vec::new();

    let listed = phys()
        .enumerate_files_with_stat(root.path(), &|_| true, &mut |s| skipped.push(s))
        .unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].0, locked); // AR16: no per-file open on the listing path
    assert!(skipped.is_empty());
    assert!(!phys().try_probe_readable(&locked).unwrap_err().trim().is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// Junctions (C# PhysicalIoJunctionNativeTests, PhysicalFileSystemReparseTests)
// ---------------------------------------------------------------------------------------------------------------

// C#: PhysicalIoJunctionNativeTests.ResolveRealPath_JunctionInsideThePath_ResolvesToTheRealTarget
#[test]
#[ignore = "native"]
fn resolve_real_path_junction_inside_the_path_resolves_to_the_real_target() {
    let root = TempRoot::new("physical-io-junction");
    let base = real_root(&root);
    let mut links = Links::default();
    let target = dir_in(&base, "real");
    let link = links.junction(&base.join("link"), &target);

    let resolved = phys().resolve_real_path(&link.join("child").join("photo.jpg")).unwrap();

    assert!(
        eq_ignore_case(&resolved, &target.join("child").join("photo.jpg")),
        "{}",
        resolved.display()
    );
}

// C#: PhysicalIoJunctionNativeTests.ResolveRealPath_JunctionAsTheFinalSegment_ResolvesToTheRealFolder
#[test]
#[ignore = "native"]
fn resolve_real_path_junction_as_the_final_segment_resolves_to_the_real_folder() {
    let root = TempRoot::new("physical-io-junction");
    let base = real_root(&root);
    let mut links = Links::default();
    let target = dir_in(&base, "real");
    let link = links.junction(&base.join("link"), &target);

    assert!(eq_ignore_case(&phys().resolve_real_path(&link).unwrap(), &target));
}

// C#: PhysicalIoJunctionNativeTests.ResolveRealPath_ChainOfJunctions_ResolvesToTheFinalTarget
#[test]
#[ignore = "native"]
fn resolve_real_path_chain_of_junctions_resolves_to_the_final_target() {
    let root = TempRoot::new("physical-io-junction");
    let base = real_root(&root);
    let mut links = Links::default();
    let target = dir_in(&base, "real");
    let inner = links.junction(&base.join("inner"), &target);
    let outer = links.junction(&base.join("outer"), &inner);

    assert!(eq_ignore_case(&phys().resolve_real_path(&outer).unwrap(), &target));
}

// C#: PhysicalIoJunctionNativeTests.ResolveRealPath_DanglingJunction_StillResolvesToItsMissingTarget
#[test]
#[ignore = "native"]
fn resolve_real_path_dangling_junction_still_resolves_to_its_missing_target() {
    let root = TempRoot::new("physical-io-junction");
    let base = real_root(&root);
    let mut links = Links::default();
    let target = dir_in(&base, "gone");
    let link = links.junction(&base.join("link"), &target);
    fs::remove_dir(&target).unwrap();

    let resolved = phys().resolve_real_path(&link.join("photo.jpg")).unwrap();

    assert!(
        eq_ignore_case(&resolved, &target.join("photo.jpg")),
        "{}",
        resolved.display()
    );
}

// C#: PhysicalIoJunctionNativeTests.ResolveRealPath_RegularFolderAndFileBesideAJunction_AreNotRewritten
#[test]
#[ignore = "native"]
fn resolve_real_path_regular_folder_and_file_beside_a_junction_are_not_rewritten() {
    let root = TempRoot::new("physical-io-junction");
    let base = real_root(&root);
    let mut links = Links::default();
    let folder = dir_in(&base, "plain");
    let file = file_in(&base, r"plain\a.jpg", &[1]);
    let real = dir_in(&base, "real");
    links.junction(&base.join("link"), &real);

    assert!(eq_ignore_case(&phys().resolve_real_path(&folder).unwrap(), &folder));
    assert!(eq_ignore_case(&phys().resolve_real_path(&file).unwrap(), &file));
}

// C#: PhysicalFileSystemReparseTests.ValidateNoEscapeViaReparsePoint_ReparseLoop_FailsClosed and
// ...OrdinaryDestinationBesideLoop_StaysOk (RV-S06). They call W2-08's ActionDestinationPolicy, not merged yet: this ports
// the fact they rest on. A junction loop a -> b -> a makes resolve_real_path return Err (no panic, no hang), and a path
// beside the loop resolves unchanged.
#[test]
#[ignore = "native"]
fn resolve_real_path_reparse_loop_fails_and_a_path_beside_it_resolves() {
    let root = TempRoot::new("rvs06-reparse");
    let base = real_root(&root);
    let mut links = Links::default();
    let photos = dir_in(&base, "photos");
    let a = photos.join("a");
    let b = photos.join("b");
    let staging = dir_in(&photos, "b");
    links.junction(&a, &staging);
    fs::remove_dir(&staging).unwrap();
    links.junction(&b, &a);

    let looped = phys().resolve_real_path(&a.join("processed").join("photo.jpg"));
    assert!(looped.is_err(), "{looped:?}");

    let beside = photos.join("sel").join("photo.jpg");
    assert!(eq_ignore_case(&phys().resolve_real_path(&beside).unwrap(), &beside));
}

// ---------------------------------------------------------------------------------------------------------------
// Golden physical-fs-resolve (pure path math, Windows roots)
// ---------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "needs W2-00 (text::dotnet_path) merged: un-ignore after the D12 merge order lands W2-00"]
fn golden_physical_fs_resolve() {
    let golden = golden::load("physical-fs-resolve");
    assert_eq!(golden.cases.len(), 34);
    for case in &golden.cases {
        let path = str_of(case, "path");
        let result = phys().resolve_real_path(Path::new(path));
        match case["resolved"].as_str() {
            Some(expected) => assert_eq!(
                result.as_ref().map(|p| p.to_string_lossy().into_owned()).ok(),
                Some(expected.to_owned()),
                "path {path:?}: {result:?}"
            ),
            None => {
                assert_eq!(str_of(case, "exception"), "ArgumentException");
                let err = result.unwrap_err();
                assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "path {path:?}");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Golden physical-fs-pattern over real files
// ---------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native"]
fn golden_physical_fs_pattern_on_disk() {
    let golden = golden::load("physical-fs-pattern");
    let root = TempRoot::new("physfs-pattern");
    let first = &golden.cases[0];
    for name in first["names"].as_array().unwrap() {
        file_in(root.path(), name.as_str().unwrap(), b"x");
    }
    for dir in first["dirs"].as_array().unwrap() {
        dir_in(root.path(), dir.as_str().unwrap());
    }
    for case in &golden.cases {
        let pattern = str_of(case, "pattern");
        let mut found = names_of(&phys().enumerate_files(root.path(), pattern).unwrap());
        found.sort();
        assert_eq!(found, expected_names(case), "pattern {pattern:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Golden physical-fs-ops: scenario interpreter over a real folder per case
// ---------------------------------------------------------------------------------------------------------------

/// One golden case run in its own folder. Holders and junctions are released before the folder is removed (C#
/// `PhysFsCleanup`).
struct OpsCase {
    root: String,
    holders: Vec<Box<dyn std::any::Any>>,
    readonly: Vec<PathBuf>,
    links: Links,
    _temp: TempRoot,
}

impl Drop for OpsCase {
    fn drop(&mut self) {
        self.holders.clear();
        drop(ClearReadOnly(std::mem::take(&mut self.readonly)));
        // Links drop next (field order after this body), then the TempRoot.
    }
}

impl OpsCase {
    fn new() -> Self {
        let temp = TempRoot::new("physfs-ops");
        let root = real_root(&temp).to_string_lossy().into_owned();
        Self {
            root,
            holders: Vec::new(),
            readonly: Vec::new(),
            links: Links::default(),
            _temp: temp,
        }
    }

    /// `Path.Combine(root, relative)`.
    fn combine(&self, relative: &str) -> String {
        let rooted = relative.starts_with(['\\', '/']) || (relative.len() >= 2 && relative.as_bytes()[1] == b':');
        if rooted {
            relative.to_owned()
        } else if relative.is_empty() {
            self.root.clone()
        } else {
            format!("{}\\{relative}", self.root)
        }
    }

    /// C# `Arg(i)`.
    fn arg(&self, raw: &str) -> PathBuf {
        if let Some(literal) = raw.strip_prefix('@') {
            return PathBuf::from(literal);
        }
        if raw.contains("{root}") || raw.contains("{rootfwd}") {
            return PathBuf::from(
                raw.replace("{root}", &self.root)
                    .replace("{rootfwd}", &self.root.replace('\\', "/")),
            );
        }
        PathBuf::from(self.combine(raw))
    }

    fn mask(&self, path: &Path) -> String {
        path.to_string_lossy()
            .replace(&self.root, "<root>")
            .replace(&self.root.replace('\\', "/"), "<rootfwd>")
    }

    fn setup(&mut self, items: &[Value]) {
        for item in items {
            if let Some(name) = item["file"].as_str() {
                let path = PathBuf::from(self.combine(name));
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, item["text"].as_str().unwrap_or("x")).unwrap();
                if let Some(utc) = item["lastWriteUtc"].as_str() {
                    let (time, _) = DateTimeUtc::parse_json(utc).unwrap();
                    let file = OpenOptions::new().write(true).open(&path).unwrap();
                    file.set_modified(to_system_time(time)).unwrap();
                }
                if item["readonly"].as_bool() == Some(true) {
                    set_readonly(&path);
                    self.readonly.push(path);
                }
            } else if let Some(name) = item["fileHex"].as_str() {
                let path = PathBuf::from(self.combine(name));
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, unhex(str_of(item, "hex"))).unwrap();
            } else if let Some(name) = item["dir"].as_str() {
                fs::create_dir_all(self.combine(name)).unwrap();
            } else if let Some(name) = item["junction"].as_str() {
                let link = PathBuf::from(self.combine(name));
                let target = PathBuf::from(self.combine(str_of(item, "target")));
                self.links.junction(&link, &target);
            } else if let Some(name) = item["deleteDir"].as_str() {
                fs::remove_dir(self.combine(name)).unwrap();
            } else if let Some(name) = item["hold"].as_str() {
                let (read, write) = match str_of(item, "access") {
                    "read" => (true, false),
                    "write" => (false, true),
                    _ => (true, true),
                };
                let share = match str_of(item, "share") {
                    "none" => SHARE_NONE,
                    "read" => SHARE_READ,
                    "readwrite" => SHARE_READ | SHARE_WRITE,
                    _ => SHARE_ALL,
                };
                let file = hold(Path::new(&self.combine(name)), read, write, share);
                self.holders.push(Box::new(file));
            } else if let Some(name) = item["holdAppend"].as_str() {
                let durable = item["durable"].as_bool().unwrap_or(false);
                let appender = phys().open_append(Path::new(&self.combine(name)), durable).unwrap();
                self.holders.push(Box::new(appender));
            } else {
                panic!("unknown setup item {item}");
            }
        }
    }
}

fn to_system_time(time: DateTimeUtc) -> SystemTime {
    const UNIX_EPOCH_TICKS: i64 = 621_355_968_000_000_000;
    let ticks = time.ticks() - UNIX_EPOCH_TICKS;
    let nanos = u64::try_from(ticks).unwrap() * 100;
    UNIX_EPOCH + Duration::from_nanos(nanos)
}

fn exists_any(path: &Path) -> bool {
    phys().file_exists(path) || phys().directory_exists(path)
}

/// Runs one op; `Ok(value)` or the error; fills `after`.
fn run_op(case: &Value, c: &OpsCase, after: &mut serde_json::Map<String, Value>) -> io::Result<Value> {
    let fs = phys();
    let args: Vec<PathBuf> = case["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| c.arg(a.as_str().unwrap()))
        .collect();
    let a0 = || args[0].as_path();
    let a1 = || args[1].as_path();
    let list = |paths: Vec<PathBuf>| {
        let mut masked: Vec<String> = paths.iter().map(|p| c.mask(p)).collect();
        masked.sort();
        Value::from(masked)
    };
    let op = str_of(case, "op");
    match op {
        "delete" => {
            let r = fs.delete(a0());
            after.insert("exists".into(), fs.file_exists(a0()).into());
            r.map(|()| Value::Null)
        }
        "move" => {
            let r = fs.move_file(a0(), a1());
            after.insert("src_exists".into(), exists_any(a0()).into());
            after.insert("dst_exists".into(), exists_any(a1()).into());
            r.map(|()| Value::Null)
        }
        "copy" => {
            let r = fs.copy_file(a0(), a1());
            after.insert("dst_exists".into(), fs.file_exists(a1()).into());
            r.map(|()| Value::Null)
        }
        "copy-attrs" => {
            fs.copy_file(a0(), a1())?;
            let same =
                fs.get_file_stat(a0()).map(|s| s.last_write_utc) == fs.get_file_stat(a1()).map(|s| s.last_write_utc);
            let readonly = fs::metadata(a1())?.permissions().readonly();
            Ok(serde_json::json!({"same_write_time": same, "readonly": readonly, "write_ticks": null}))
        }
        "trycopynew" => {
            let r = fs.try_copy_new(a0(), a1());
            after.insert("dst_exists".into(), exists_any(a1()).into());
            r.map(Value::from)
        }
        "trycopynew-proof" => {
            let proof = CopyCreationProof::new();
            let r = fs.try_copy_new_with_proof(a0(), a1(), &proof);
            after.insert("proof_created".into(), proof.destination_created().into());
            r.map(Value::from)
        }
        "stat" => Ok(match fs.get_file_stat(a0()) {
            None => Value::Null,
            Some(stat) => {
                // write_ticks is null in the golden (D19); the setup's lastWriteUtc, when given, is checked here.
                if let Some(utc) = case["setup"][0]["lastWriteUtc"].as_str() {
                    assert_eq!(
                        stat.last_write_utc,
                        DateTimeUtc::parse_json(utc).unwrap().0,
                        "{}",
                        str_of(case, "name")
                    );
                }
                serde_json::json!({"length": stat.length, "write_ticks": null, "utc": true})
            }
        }),
        "fileexists" => Ok(fs.file_exists(a0()).into()),
        "direxists" => Ok(fs.directory_exists(a0()).into()),
        "mkdir" => {
            let r = fs.create_directory(a0());
            after.insert("dir_exists".into(), fs.directory_exists(a0()).into());
            r.map(|()| Value::Null)
        }
        "enumdirs" => fs.enumerate_directories(a0()).map(list),
        "enumfiles" => {
            let pattern = case["pattern"].as_str().unwrap_or("*");
            fs.enumerate_files(a0(), pattern).map(list)
        }
        "enumstat" | "enumreadable" => {
            let suffix = case["includeSuffix"].as_str().map(str::to_lowercase);
            let include = |p: &Path| {
                suffix
                    .as_ref()
                    .is_none_or(|s| p.to_string_lossy().to_lowercase().ends_with(s.as_str()))
            };
            let mut skipped = Vec::new();
            let listing = if op == "enumstat" {
                fs.enumerate_files_with_stat(a0(), &include, &mut |s| skipped.push(s))
            } else {
                fs.enumerate_readable_files_with_stat(a0(), &include, &mut |s| skipped.push(s))
            }?;
            let mut files: Vec<Value> = listing
                .iter()
                .map(
                    |(p, s)| serde_json::json!({"path": c.mask(p), "length": s.map(|s| s.length), "write_ticks": null}),
                )
                .collect();
            files.sort_by(|x, y| x["path"].as_str().cmp(&y["path"].as_str()));
            let skipped: Vec<Value> = skipped
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "path": c.mask(&s.path),
                        "kind": format!("{:?}", s.kind),
                        "reason_present": !s.reason.trim().is_empty(),
                    })
                })
                .collect();
            Ok(serde_json::json!({"files": files, "skipped": skipped}))
        }
        "readtext" => fs.read_all_text(a0()).map(Value::from),
        "openread" => {
            let mut stream = fs.open_read_shared(a0(), 65536)?;
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes)?;
            Ok(String::from_utf8_lossy(&bytes).into_owned().into())
        }
        "openread-then-rename" => {
            let mut stream = fs.open_read_shared(a0(), 65536)?;
            fs.move_file(a0(), a1())?;
            fs.delete(a1())?;
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes)?;
            Ok(String::from_utf8_lossy(&bytes).into_owned().into())
        }
        "openappend" => {
            let durable = case["durable"].as_bool().unwrap_or(false);
            for text in case["texts"].as_array().unwrap() {
                let mut stream = fs.open_append(a0(), durable)?;
                stream.write_all(text.as_str().unwrap().as_bytes())?;
                stream.flush()?;
            }
            Ok(fs::read_to_string(a0())?.into())
        }
        "openappend-twice" => {
            let _first = fs.open_append(a0(), false)?;
            let _second = fs.open_append(a0(), false)?;
            Ok(Value::Null)
        }
        "openappend-then-delete" => {
            let _stream = fs.open_append(a0(), false)?;
            fs.delete(a0())?;
            Ok(Value::Null)
        }
        "openappend-then-open-reader" => {
            let _stream = fs.open_append(a0(), true)?;
            let share = if str_of(case, "readerShare") == "read" {
                SHARE_READ
            } else {
                SHARE_ALL
            };
            let _reader = OpenOptions::new().read(true).share_mode(share).open(a0())?;
            Ok(Value::Null)
        }
        "writeatomic" => {
            let durable = case["durable"].as_bool().unwrap_or(true);
            let r = fs.write_all_text_atomic(a0(), str_of(case, "text"), durable);
            after.insert("tmp_files".into(), tmp_count(a0().parent().unwrap()).into());
            r?;
            Ok(hex(&fs::read(a0())?).into())
        }
        "probe" => {
            let r = fs.try_probe_readable(a0());
            Ok(serde_json::json!({
                "readable": r.is_ok(),
                "reason_present": r.as_ref().err().is_some_and(|e| !e.trim().is_empty()),
                "failure_is_null": r.is_ok(),
            }))
        }
        "trydeleteempty" => {
            let value = fs.try_delete_empty_directory(a0());
            after.insert("dir_exists".into(), fs.directory_exists(a0()).into());
            Ok(value.into())
        }
        "resolve" => fs.resolve_real_path(a0()).map(|p| c.mask(&p).into()),
        other => panic!("unknown op {other}"),
    }
}

/// The io::Error shape of a .NET exception class + Win32 code (contract note 1).
fn check_error(name: &str, err: &io::Error, exception: &str, win32: i64) {
    let raw = err.raw_os_error().map(i64::from);
    if win32 != 0 {
        assert_eq!(raw, Some(win32), "{name}: {err} ({err:?})");
    } else {
        assert_eq!(raw, None, "{name}: {err} ({err:?})");
    }
    match exception {
        "FileNotFoundException" | "DirectoryNotFoundException" => {
            assert_eq!(err.kind(), io::ErrorKind::NotFound, "{name}")
        }
        "UnauthorizedAccessException" => assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "{name}"),
        "ArgumentException" => assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{name}"),
        "IOException" => assert!(
            !matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied | io::ErrorKind::InvalidInput
            ),
            "{name}: {err:?}"
        ),
        other => panic!("{name}: unexpected exception class {other}"),
    }
}

#[test]
#[ignore = "native"]
fn golden_physical_fs_ops() {
    let golden = golden::load("physical-fs-ops");
    assert_eq!(golden.cases.len(), 145);
    let mut failures = Vec::new();
    for case in &golden.cases {
        let name = str_of(case, "name").to_owned();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut c = OpsCase::new();
            c.setup(case["setup"].as_array().unwrap());
            let mut after = serde_json::Map::new();
            let result = run_op(case, &c, &mut after);
            if bool_of(case, "ok") {
                let value = result.unwrap_or_else(|e| panic!("{name}: expected Ok, got {e} ({e:?})"));
                assert_eq!(value, case["value"], "{name}");
            } else {
                let err = result.err().unwrap_or_else(|| panic!("{name}: expected an error"));
                check_error(&name, &err, str_of(case, "exception"), case["win32"].as_i64().unwrap());
            }
            let expected_after = case["after"].as_object().cloned().unwrap_or_default();
            assert_eq!(after, expected_after, "{name}: after");
        }));
        if let Err(panic) = outcome {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default();
            failures.push(message);
        }
    }
    assert!(
        failures.is_empty(),
        "{} failing cases:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------------------------------------------
// .NET path normalisation and Directory.CreateDirectory (verifier differences, checked against .NET 10)
// ---------------------------------------------------------------------------------------------------------------

/// `<root>\sub\c.txt` and `<root>\a.txt`.
fn spelled_root(label: &str) -> TempRoot {
    let root = TempRoot::new(label);
    file_in(root.path(), r"sub\c.txt", b"ccc");
    file_in(root.path(), "a.txt", b"x");
    root
}

fn spelled(root: &TempRoot, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", root.path().display()))
}

// C#: Directory.EnumerateFiles(@"<root>\sub ") lists the folder Path.GetFullPath trims it to.
#[test]
#[ignore = "native"]
fn enumerate_files_trailing_space_directory_is_normalised() {
    let root = spelled_root("dotnet-norm");
    let got = phys().enumerate_files(&spelled(&root, r"\sub "), "*").unwrap();
    assert_eq!(names_of(&got), vec!["c.txt"]);
    // The entries are joined to the directory as spelled (FileSystemEntry.ToSpecifiedFullPath).
    assert_eq!(got[0], spelled(&root, r"\sub \c.txt"));
}

// C#: same for a trailing dot.
#[test]
#[ignore = "native"]
fn enumerate_files_trailing_dot_directory_is_normalised() {
    let root = spelled_root("dotnet-norm");
    let got = phys().enumerate_files(&spelled(&root, r"\sub."), "*").unwrap();
    assert_eq!(names_of(&got), vec!["c.txt"]);
}

// C#: Directory.EnumerateDirectories(@"<root>\sub\ ") (blank last segment) lists <root>\sub itself.
#[test]
#[ignore = "native"]
fn enumerate_directories_trailing_space_directory_is_normalised() {
    let root = spelled_root("dotnet-norm");
    dir_in(root.path(), r"sub\inner");
    let got = phys().enumerate_directories(&spelled(&root, r"\sub\ ")).unwrap();
    assert_eq!(names_of(&got), vec!["inner"]);
    let blank = phys().enumerate_directories(&spelled(&root, r"\ ")).unwrap();
    assert_eq!(names_of(&blank), vec!["sub"]);
}

// C#: Directory.EnumerateFiles(<a FILE>) is an IOException with ERROR_INVALID_PARAMETER (87), not ERROR_DIRECTORY
// (267); a stream name under a file is DirectoryNotFound (3).
#[test]
#[ignore = "native"]
fn enumerate_files_on_a_file_path_raw_code() {
    let root = spelled_root("dotnet-norm");
    let file = root.path().join("a.txt");
    assert_eq!(phys().enumerate_files(&file, "*").unwrap_err().raw_os_error(), Some(87));
    assert_eq!(
        phys().enumerate_directories(&file).unwrap_err().raw_os_error(),
        Some(87)
    );
    let with_stat = phys().enumerate_files_with_stat(&file, &|_| true, &mut |_| {});
    assert_eq!(with_stat.unwrap_err().raw_os_error(), Some(87));
    let stream = spelled(&root, r"\a.txt:s");
    assert_eq!(
        phys().enumerate_files(&stream, "*").unwrap_err().raw_os_error(),
        Some(3)
    );
    let missing = root.path().join("missing");
    assert_eq!(
        phys().enumerate_files(&missing, "*").unwrap_err().raw_os_error(),
        Some(3)
    );
}

// C#: Directory.CreateDirectory(@"<root>\nul") -> DirectoryNotFoundException raw 3.
#[test]
#[ignore = "native"]
fn create_directory_device_name_fails() {
    let root = spelled_root("dotnet-norm");
    let got = phys().create_directory(&root.path().join("nul"));
    assert_eq!(got.unwrap_err().raw_os_error(), Some(3));
}

// C#: new FileStream(@"<root>\sub/") -> DirectoryNotFoundException raw 3 (GetFullPath makes it <root>\sub\); also
// File.ReadAllText and the append stream, and a blank last segment.
#[test]
#[ignore = "native"]
fn open_read_shared_directory_with_forward_slash_is_not_found() {
    let root = spelled_root("dotnet-norm");
    for suffix in [r"\sub/", r"\sub\ "] {
        let path = spelled(&root, suffix);
        let read = phys().open_read_shared(&path, 4096);
        assert_eq!(read.err().and_then(|e| e.raw_os_error()), Some(3), "{suffix}");
        let append = phys().open_append(&path, false);
        assert_eq!(append.err().and_then(|e| e.raw_os_error()), Some(3), "{suffix}");
        assert_eq!(
            phys().read_all_text(&path).unwrap_err().raw_os_error(),
            Some(3),
            "{suffix}"
        );
    }
    // The folder spelled without a separator stays access denied (as in .NET).
    let plain = phys().open_read_shared(&root.path().join("sub"), 4096);
    assert_eq!(plain.err().and_then(|e| e.raw_os_error()), Some(5));
}

// C#: Directory.CreateDirectory(<dangling junction>) returns (the entry has the DIRECTORY attribute); a folder under it
// is DirectoryNotFound raw 3, and an atomic write through it fails the same way.
#[test]
#[ignore = "native"]
fn create_directory_on_a_dangling_junction_is_ok() {
    let root = TempRoot::new("dotnet-norm");
    let mut links = Links::default();
    let gone = dir_in(root.path(), "gone");
    let jd = links.junction(&root.path().join("jd"), &gone);
    fs::remove_dir(&gone).unwrap();

    phys().create_directory(&jd).unwrap();
    let under = phys().create_directory(&jd.join("x"));
    assert_eq!(under.unwrap_err().raw_os_error(), Some(3));
    let write = phys().write_all_text_atomic(&jd.join("x").join("f.json"), "{}", false);
    assert_eq!(write.unwrap_err().raw_os_error(), Some(3));
}

// C#: Directory.CreateDirectory creates every missing level, is Ok on an existing folder, and reports a FILE in the
// wanted place as raw 183; a file in a middle level is also raw 183 (the FIRST failure, at the file's level, is
// reported because creating the wanted folder failed too).
#[test]
#[ignore = "native"]
fn create_directory_creates_missing_levels_and_rejects_a_file() {
    let root = spelled_root("dotnet-norm");
    let deep = root.path().join("n1").join("n2").join("n3");
    phys().create_directory(&deep).unwrap();
    assert!(deep.is_dir());
    phys().create_directory(&deep).unwrap();
    phys().create_directory(&spelled(&root, r"\n1\n2\")).unwrap();
    let on_file = phys().create_directory(&root.path().join("a.txt"));
    assert_eq!(on_file.unwrap_err().raw_os_error(), Some(183));
    let under_file = phys().create_directory(&root.path().join("a.txt").join("x"));
    assert_eq!(under_file.unwrap_err().raw_os_error(), Some(183));
}
