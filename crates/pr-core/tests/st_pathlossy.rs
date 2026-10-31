//! ST-PATHLOSSY: what happens to a file name that holds an unpaired UTF-16 surrogate (legal on Windows) when it crosses
//! from `OsStr`/`PathBuf` to the `String` that every file-action request and journal line carries.
//!
//! C# `OperationJournal.ThrowIfUnrepresentable` (W2-FA-03) refuses such a path with `JournalErrors.UnrepresentablePath`
//! before anything is journaled or mutated. Rust's `String` cannot hold a lone surrogate, so the journal gate "cannot
//! fire" (journal.rs `append_lines`) and the real gate would have to sit at the `OsStr -> String` boundary, which does
//! not exist: `UNREPRESENTABLE_PATH` is never emitted. These tests pin down the present behaviour on real files under a
//! `TempRoot` (a recycle bin that only deletes the temp file; the real Recycle Bin is never touched).
//!
//! Windows only. The `#[ignore]`d test is RED on purpose: it states the invariant that is violated today.

#![cfg(windows)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_core::abstractions::{Clock, FileSystem, RecycleBin, SystemClock};
use pr_core::app_paths::AppPaths;
use pr_core::catalog::sorting::list_visible_directories;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::types::{FileActionRequest, FileActionResult};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::FileOperationType;
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_test_support::TempRoot;

/// "Sending to the bin" deletes the given temp file. Never the real Recycle Bin.
#[derive(Debug, Default)]
struct DeletingTempBin;

impl RecycleBin for DeletingTempBin {
    fn send_to_recycle_bin(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }
    fn delete_permanently(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }
    fn try_restore(&self, _: &Path, _: u64, _: DateTimeUtc) -> std::io::Result<bool> {
        Ok(true)
    }
    fn fits_in_recycle_bin(&self, _: &Path, _: u64) -> bool {
        true
    }
}

struct Harness {
    root: TempRoot,
    journal_file: PathBuf,
    service: FileActionService,
}

impl Harness {
    fn new(label: &str) -> Self {
        let root = TempRoot::new(label);
        let fs: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem);
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let paths = AppPaths::new(root.path(), None, false);
        let journal = Arc::new(OperationJournal::new(&paths, fs.clone(), clock.clone()));
        let bin: Arc<dyn RecycleBin> = Arc::new(DeletingTempBin);
        let service = FileActionService::new(journal, fs, clock, bin);
        Self {
            root,
            journal_file: paths.journal_file,
            service,
        }
    }

    fn journal_text(&self) -> String {
        std::fs::read_to_string(&self.journal_file).unwrap_or_default()
    }
}

/// `<stem><lone high surrogate>.jpg` as an `OsString` (UTF-16 units `... 0xD800 ...`).
fn surrogate_name(stem: &str) -> OsString {
    let mut units: Vec<u16> = stem.encode_utf16().collect();
    units.push(0xD800);
    units.extend(".jpg".encode_utf16());
    OsString::from_wide(&units)
}

/// A folder holding the file `<stem><U+D800>.jpg` (and, when `decoy`, the VALID file `<stem><U+FFFD>.jpg`).
fn make_files(harness: &Harness, decoy: bool) -> (PathBuf, PathBuf, Option<PathBuf>) {
    let folder = harness.root.join("photos");
    std::fs::create_dir_all(&folder).expect("folder");
    let lone = folder.join(surrogate_name("a"));
    std::fs::write(&lone, b"surrogate").expect("a file with a lone surrogate in its name is creatable on NTFS");
    let decoy = decoy.then(|| {
        let path = folder.join("a\u{FFFD}.jpg");
        std::fs::write(&path, b"decoy").expect("decoy");
        path
    });
    (folder, lone, decoy)
}

/// The boundary conversion the app uses today (`folder_load/load.rs`: `path.to_string_lossy().into_owned()`).
fn lossy(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn recycle(harness: &Harness, source: String) -> FileActionResult {
    harness.service.execute(
        &FileActionRequest::new(source, FileOperationType::Recycle),
        &CancelToken::none(),
    )
}

// Control: the listing itself is lossless (a PathBuf), so the loss happens only at the later String conversion.
#[test]
fn listing_returns_the_lone_surrogate_path_losslessly() {
    let h = Harness::new("pathlossy-list");
    let (folder, lone, _) = make_files(&h, false);
    let listed = PhysicalFileSystem.enumerate_files(&folder, "*").expect("enumerate");
    assert_eq!(listed, vec![lone.clone()]);
    assert!(listed[0].exists());
    assert!(listed[0].to_str().is_none(), "the path is not valid Unicode");
}

// The lossy String names a DIFFERENT (here: non-existent) file.
#[test]
fn lossy_string_of_a_lone_surrogate_path_names_another_file() {
    let h = Harness::new("pathlossy-string");
    let (_, lone, _) = make_files(&h, false);
    let text = lossy(&lone);
    assert!(text.contains('\u{FFFD}'));
    assert!(lone.exists());
    assert!(
        !Path::new(&text).exists(),
        "the String names a file that does not exist"
    );
}

// No decoy: the action on the wrong name fails safe (the real file is untouched), but no `UnrepresentablePath` is ever
// produced (C# refuses before journaling).
#[test]
fn lossy_source_without_a_decoy_fails_safe_but_is_not_refused_as_unrepresentable() {
    let h = Harness::new("pathlossy-failsafe");
    let (_, lone, _) = make_files(&h, false);
    let result = recycle(&h, lossy(&lone));
    assert!(!result.succeeded);
    assert!(lone.exists(), "the real file is untouched");
    assert_ne!(result.error.as_deref(), Some("UnrepresentablePath"));
    assert!(!h.journal_text().contains("UnrepresentablePath"));
}

// With a decoy whose name really holds U+FFFD, the lossy String of the lone-surrogate file IS the decoy's path: the
// action runs on the WRONG file. Passing = the hazard is present today (C# would refuse with UnrepresentablePath, and
// neither file would be touched).
#[test]
fn lossy_source_with_a_decoy_acts_on_the_wrong_file() {
    let h = Harness::new("pathlossy-decoy");
    let (_, lone, decoy) = make_files(&h, true);
    let decoy = decoy.expect("decoy");
    let result = recycle(&h, lossy(&lone));
    assert!(result.succeeded, "{:?}", result.error);
    assert!(!decoy.exists(), "the decoy (a different file) was recycled");
    assert!(lone.exists(), "the file the user selected is untouched");
    assert!(!h.journal_text().contains("UnrepresentablePath"));
}

// RED, intentionally ignored. Invariant: a path text returned for an entry must name that entry. `list_visible_directories`
// (sibling-folder navigation) returns `entry.path().to_string_lossy()`, so a folder with a lone surrogate comes back as a
// path that does not exist. Fix options (proposal): skip/err for non-Unicode entries or carry `PathBuf` end to end.
#[test]
#[ignore = "ST-PATHLOSSY red: list_visible_directories returns a lossy path for a non-Unicode folder name (see docs/reviews/ST-PATHLOSSY-2026-10-31.md)"]
fn listed_directory_text_must_name_the_listed_directory() {
    let h = Harness::new("pathlossy-dirs");
    let parent = h.root.join("parent");
    let dir = parent.join(surrogate_name("d"));
    std::fs::create_dir_all(&dir).expect("dir with a lone surrogate");
    let listed = list_visible_directories(&parent).expect("list");
    assert!(!listed.is_empty());
    for text in &listed {
        assert!(Path::new(text).exists(), "{text:?} does not name an existing directory");
    }
}
