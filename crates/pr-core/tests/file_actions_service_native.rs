//! Native and slow tests of `pr_core::file_actions::service` (every `#[ignore]`d test of the W3-04 unit). Port of
//! `PhotoReview.Core.Tests/FileActions/FileActionConcurrencyTests.cs`, `SymlinkDestinationEscapeNativeTests.cs`,
//! `CaptureGroupActionNativeTests.cs`, `CaptureGroupActionSlowTests.cs`, `JournalDurabilityLatencyManualTests.cs`,
//! `Services/InterleavedFileActionSequenceTests.cs` and `BenchmarkScenarioTests.cs` (baseline d92d6aae).
//!
//! Run them on Windows with `cargo test -p pr-core --test file_actions_service_native -- --include-ignored`. They use
//! [`PhysicalFileSystem`] under a [`TempRoot`] and [`DeletingTempBin`], which deletes the temp file it is given and never
//! touches the user's Recycle Bin (AGENTS.md, C# 2026-09-24 incident). Nothing here runs in the default test pass.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use pr_core::abstractions::{Clock, FileSystem, RecycleBin, SystemClock};
use pr_core::app_paths::AppPaths;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::{CaptureGroup, CatalogEntry};
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::types::{CaptureGroupActionRequest, FileActionRequest};
use pr_core::file_actions::undo::{EnglishUndoTexts, UndoService};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::{FileOperationType, RawPairMode};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_test_support::TempRoot;

/// A recycle bin for native tests: "sending to the bin" deletes the given temp file, and nothing else is ever touched.
/// C# `PhysicalActionHarness` uses the same rule. It is never the real Recycle Bin.
#[derive(Debug, Default)]
struct DeletingTempBin;

impl RecycleBin for DeletingTempBin {
    fn send_to_recycle_bin(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }

    fn delete_permanently(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }

    fn try_restore(
        &self,
        _original_path: &Path,
        _expected_size: u64,
        _expected_last_write_utc: DateTimeUtc,
    ) -> std::io::Result<bool> {
        Ok(true)
    }

    fn fits_in_recycle_bin(&self, _path: &Path, _file_size: u64) -> bool {
        true
    }
}

/// A real-disk service over a temp root: physical file system, journal under the root, [`DeletingTempBin`].
struct Native {
    root: TempRoot,
    fs: Arc<dyn FileSystem>,
    journal: Arc<OperationJournal>,
    service: FileActionService,
}

impl Native {
    fn new(label: &str) -> Self {
        let root = TempRoot::new(label);
        let fs: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem);
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let paths = AppPaths::new(root.path(), None, false);
        let journal = Arc::new(OperationJournal::new(&paths, fs.clone(), clock.clone()));
        let bin: Arc<dyn RecycleBin> = Arc::new(DeletingTempBin);
        let service = FileActionService::new(journal.clone(), fs.clone(), clock, bin);
        Self {
            root,
            fs,
            journal,
            service,
        }
    }

    /// A file under the temp root, with `bytes` as its content.
    fn file(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create folder");
        std::fs::write(&path, bytes).expect("write file");
        path
    }
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read file")
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionConcurrencyTests.cs: the physical harness (native: real files)
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionConcurrencyTests.MoveSucceedsWhileReadIsOpen
#[test]
#[ignore = "native: real files"]
fn move_succeeds_while_read_is_open() {
    let native = Native::new("move-while-read");
    let source = native.file("photos/a.jpg", b"photo");
    let mut reader = native
        .fs
        .open_read_shared(&source, 4096)
        .expect("a shared read is open");
    let result = native.service.execute(
        &FileActionRequest {
            destination: Some("sel".to_owned()),
            ..FileActionRequest::new(source.to_string_lossy().into_owned(), FileOperationType::Move)
        },
        &CancelToken::none(),
    );
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer).expect("the open read still works");
    assert!(result.succeeded, "{:?}", result.error);
    assert_eq!(buffer, b"photo");
}

// C#: FileActionConcurrencyTests.DeleteSucceedsWhileReadIsOpen
#[test]
#[ignore = "native: real files"]
fn delete_succeeds_while_read_is_open() {
    let native = Native::new("delete-while-read");
    let source = native.file("photos/a.jpg", b"photo");
    let _reader = native
        .fs
        .open_read_shared(&source, 4096)
        .expect("a shared read is open");
    let result = native.service.execute(
        &FileActionRequest::new(source.to_string_lossy().into_owned(), FileOperationType::Recycle),
        &CancelToken::none(),
    );
    assert!(result.succeeded, "{:?}", result.error);
    assert!(!source.exists());
}

// C#: FileActionConcurrencyTests.CopyPreservesSourceAndBytesWhileReadIsOpen
#[test]
#[ignore = "native: real files"]
fn copy_preserves_source_and_bytes_while_read_is_open() {
    let native = Native::new("copy-while-read");
    let source = native.file("photos/a.jpg", b"photo bytes");
    let _reader = native
        .fs
        .open_read_shared(&source, 4096)
        .expect("a shared read is open");
    let result = native.service.execute(
        &FileActionRequest {
            destination: Some("sel".to_owned()),
            ..FileActionRequest::new(source.to_string_lossy().into_owned(), FileOperationType::Copy)
        },
        &CancelToken::none(),
    );
    assert!(result.succeeded, "{:?}", result.error);
    assert_eq!(text(&source), "photo bytes");
    assert_eq!(text(&native.root.join("photos/sel/a.jpg")), "photo bytes");
}

// C#: FileActionConcurrencyTests.InterleavedSequenceLeavesDeterministicState
#[test]
#[ignore = "native: real files"]
fn interleaved_sequence_leaves_deterministic_state() {
    let native = Native::new("interleaved");
    let a = native.file("photos/a.jpg", b"a");
    let b = native.file("photos/b.jpg", b"b");
    let c = native.file("photos/c.jpg", b"c");
    let run = |source: &Path, op: FileOperationType, destination: Option<&str>| {
        native.service.execute(
            &FileActionRequest {
                destination: destination.map(str::to_owned),
                ..FileActionRequest::new(source.to_string_lossy().into_owned(), op)
            },
            &CancelToken::none(),
        )
    };
    assert!(run(&a, FileOperationType::Move, Some("sel")).succeeded);
    assert!(run(&b, FileOperationType::Copy, Some("sel")).succeeded);
    assert!(run(&c, FileOperationType::Recycle, None).succeeded);
    assert!(native.root.join("photos/sel/a.jpg").exists());
    assert!(b.exists() && native.root.join("photos/sel/b.jpg").exists());
    assert!(!c.exists());
}

// ---------------------------------------------------------------------------------------------------------------
// SymlinkDestinationEscapeNativeTests.cs: a real junction (`mklink /J`), the bin is DeletingTempBin
// ---------------------------------------------------------------------------------------------------------------

fn make_junction(link: &Path, target: &Path) {
    std::fs::create_dir_all(target).expect("junction target");
    // `cmd` reads a `/` inside a path as a switch ("Invalid switch - link"): pass both paths with backslashes only.
    let windows = |path: &Path| path.to_string_lossy().replace('/', "\\");
    let status = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(windows(link))
        .arg(windows(target))
        .status()
        .expect("mklink runs");
    assert!(status.success(), "mklink /J failed");
}

// C#: SymlinkDestinationEscapeNativeTests.ExecuteAsync_RealSymlinkEscapesFolder_Rejected
#[test]
#[ignore = "native: real junction"]
fn execute_async_real_symlink_escapes_folder_rejected() {
    let native = Native::new("junction-escape");
    let source = native.file("photos/a.jpg", b"photo");
    make_junction(&native.root.join("photos/link"), &native.root.join("outside"));
    let result = native.service.execute(
        &FileActionRequest {
            destination: Some(r"link\processed".to_owned()),
            ..FileActionRequest::new(source.to_string_lossy().into_owned(), FileOperationType::Move)
        },
        &CancelToken::none(),
    );
    assert!(!result.succeeded);
    assert!(source.exists());
    assert!(!native.root.join("outside/processed/a.jpg").exists());
}

// C#: SymlinkDestinationEscapeNativeTests.ExecuteAsync_RealSymlinkStaysInsideFolder_Succeeds
#[test]
#[ignore = "native: real junction"]
fn execute_async_real_symlink_stays_inside_folder_succeeds() {
    let native = Native::new("junction-inside");
    let source = native.file("photos/a.jpg", b"photo");
    make_junction(&native.root.join("photos/link"), &native.root.join("photos/real"));
    let result = native.service.execute(
        &FileActionRequest {
            destination: Some(r"link\processed".to_owned()),
            ..FileActionRequest::new(source.to_string_lossy().into_owned(), FileOperationType::Move)
        },
        &CancelToken::none(),
    );
    assert!(result.succeeded, "{:?}", result.error);
}

// C#: SymlinkDestinationEscapeNativeTests.ExecuteAsync_OrdinaryDestination_Unaffected
#[test]
#[ignore = "native: real junction"]
fn execute_async_ordinary_destination_unaffected() {
    let native = Native::new("ordinary-destination");
    let source = native.file("photos/a.jpg", b"photo");
    let result = native.service.execute(
        &FileActionRequest {
            destination: Some("sel".to_owned()),
            ..FileActionRequest::new(source.to_string_lossy().into_owned(), FileOperationType::Move)
        },
        &CancelToken::none(),
    );
    assert!(result.succeeded, "{:?}", result.error);
    assert!(native.root.join("photos/sel/a.jpg").exists());
}

// ---------------------------------------------------------------------------------------------------------------
// CaptureGroupActionNativeTests.cs and CaptureGroupActionSlowTests.cs: group move + undo on real files
// ---------------------------------------------------------------------------------------------------------------

/// Group move of a JPEG + RAW pair on real files, then undo; the pair is restored. The C# fixture uses a copied RAW from
/// the corpus, which the Rust port does not ship: a synthetic RAW of the same shape stands in.
// C#: CaptureGroupActionNativeTests.GroupMoveAndUndo_CopiedRawCorpusMember_PreservesOriginalAndRestoresPair
#[test]
#[ignore = "native: real files + RAW corpus"]
fn group_move_and_undo_copied_raw_corpus_member_preserves_original_and_restores_pair() {
    let native = Native::new("group-undo-raw");
    let jpeg = native.file("photos/IMG_0001.JPG", b"jpeg");
    let raw = native.file("photos/IMG_0001.CR3", b"raw bytes");
    let capture = CaptureGroup::new(jpeg.to_string_lossy(), raw.to_string_lossy(), None).expect("group");
    let mut request = CaptureGroupActionRequest::new(capture, FileOperationType::Move);
    request.destination = Some("sel".to_owned());
    let result = native.service.execute_group(&request, &CancelToken::none());
    assert!(result.succeeded, "{:?}", result.error);
    let mut undo = UndoService::new(
        native.journal.clone(),
        native.fs.clone(),
        Arc::new(DeletingTempBin),
        Arc::new(EnglishUndoTexts),
    );
    undo.register_group(&result);
    let outcome = undo.undo_last();
    assert!(outcome.succeeded, "{:?}", outcome.error_message);
    assert_eq!(text(&raw), "raw bytes");
    assert!(jpeg.exists());
}

// C#: CaptureGroupActionSlowTests.GroupMoveAndUndo_UsesRealFilesystemAndKeepsBothMembersRecoverable
#[test]
#[ignore = "slow: real files"]
fn group_move_and_undo_uses_real_filesystem_and_keeps_both_members_recoverable() {
    let native = Native::new("group-undo-slow");
    let jpeg = native.file("photos/IMG_0002.JPG", &vec![7u8; 4096]);
    let raw = native.file("photos/IMG_0002.CR3", &vec![9u8; 8192]);
    let capture = CaptureGroup::new(jpeg.to_string_lossy(), raw.to_string_lossy(), None).expect("group");
    let mut request = CaptureGroupActionRequest::new(capture, FileOperationType::Move);
    request.destination = Some("sel".to_owned());
    let result = native.service.execute_group(&request, &CancelToken::none());
    assert!(result.succeeded, "{:?}", result.error);
    let mut undo = UndoService::new(
        native.journal.clone(),
        native.fs.clone(),
        Arc::new(DeletingTempBin),
        Arc::new(EnglishUndoTexts),
    );
    undo.register_group(&result);
    assert!(undo.undo_last().succeeded);
    assert_eq!(std::fs::metadata(&raw).expect("raw back").len(), 8192);
    assert_eq!(std::fs::metadata(&jpeg).expect("jpeg back").len(), 4096);
}

// ---------------------------------------------------------------------------------------------------------------
// JournalDurabilityLatencyManualTests.cs (manual) and the Services interleaved and benchmark scenarios (native)
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalDurabilityLatencyManualTests.MoveLatency
#[test]
#[ignore = "manual: latency measurement"]
fn move_latency() {
    let native = Native::new("move-latency");
    let source = native.file("photos/a.jpg", b"photo");
    let started = std::time::Instant::now();
    let result = native.service.execute(
        &FileActionRequest {
            destination: Some("sel".to_owned()),
            ..FileActionRequest::new(source.to_string_lossy().into_owned(), FileOperationType::Move)
        },
        &CancelToken::none(),
    );
    assert!(result.succeeded, "{:?}", result.error);
    eprintln!("move latency: {:?}", started.elapsed());
}

/// The three paths of a small review catalog, as the App builds it (C# `ReviewCatalog.Reset`).
fn catalog_of(native: &Native, names: &[&str]) -> ReviewCatalog {
    let entries: Vec<CatalogEntry> = names
        .iter()
        .map(|name| {
            let path = native.file(&format!("photos/{name}"), b"x");
            CatalogEntry::new(path.to_string_lossy().into_owned()).expect("catalog entry")
        })
        .collect();
    let mut catalog = ReviewCatalog::new();
    catalog.reset(entries, RawPairMode::default(), None);
    catalog
}

// C#: Services/InterleavedFileActionSequenceTests.TC08a_MoveKeepsNextImageWithoutSkipping
#[test]
#[ignore = "native: real files"]
fn tc08a_move_keeps_next_image_without_skipping() {
    let native = Native::new("tc08a");
    let mut catalog = catalog_of(&native, &["a.jpg", "b.jpg", "c.jpg"]);
    let current = catalog.current().expect("a current image").path().to_owned();
    let result = native.service.execute(
        &FileActionRequest {
            destination: Some("sel".to_owned()),
            ..FileActionRequest::new(current.clone(), FileOperationType::Move)
        },
        &CancelToken::none(),
    );
    assert!(result.succeeded, "{:?}", result.error);
    catalog.remove(&current);
    assert!(catalog.current().expect("next image").path().ends_with("b.jpg"));
}

// C#: Services/InterleavedFileActionSequenceTests.TC08b_DeleteKeepsNextImageWithoutSkipping
#[test]
#[ignore = "native: real files"]
fn tc08b_delete_keeps_next_image_without_skipping() {
    let native = Native::new("tc08b");
    let mut catalog = catalog_of(&native, &["a.jpg", "b.jpg", "c.jpg"]);
    assert!(catalog.set_current(1));
    let current = catalog.current().expect("b").path().to_owned();
    let result = native.service.execute(
        &FileActionRequest::new(current.clone(), FileOperationType::Recycle),
        &CancelToken::none(),
    );
    assert!(result.succeeded, "{:?}", result.error);
    catalog.remove(&current);
    assert!(catalog.current().expect("next image").path().ends_with("c.jpg"));
}

// C#: Services/InterleavedFileActionSequenceTests.TC08c_DeleteAtEndSelectsPriorSurvivingSlot
#[test]
#[ignore = "native: real files"]
fn tc08c_delete_at_end_selects_prior_surviving_slot() {
    let native = Native::new("tc08c");
    let mut catalog = catalog_of(&native, &["a.jpg", "b.jpg", "c.jpg"]);
    assert!(catalog.set_current(2));
    let current = catalog.current().expect("c").path().to_owned();
    let result = native.service.execute(
        &FileActionRequest::new(current.clone(), FileOperationType::Recycle),
        &CancelToken::none(),
    );
    assert!(result.succeeded, "{:?}", result.error);
    catalog.remove(&current);
    assert!(catalog.current().expect("prior image").path().ends_with("b.jpg"));
}

// C#: BenchmarkScenarioTests.AdvancesExactlyOnceBeforeMove
#[test]
#[ignore = "native: real files"]
fn advances_exactly_once_before_move() {
    let native = Native::new("bench-move");
    let mut catalog = catalog_of(&native, &["a.jpg", "b.jpg", "c.jpg"]);
    let current = catalog.current().expect("a").path().to_owned();
    let before = catalog.current_index();
    let result = native.service.execute(
        &FileActionRequest {
            destination: Some("sel".to_owned()),
            ..FileActionRequest::new(current.clone(), FileOperationType::Move)
        },
        &CancelToken::none(),
    );
    assert!(result.succeeded, "{:?}", result.error);
    catalog.remove(&current);
    assert_eq!(
        catalog.current_index(),
        before,
        "one advance: the slot now holds the next image"
    );
}

// C#: BenchmarkScenarioTests.AdvancesExactlyOnceBeforeDelete
#[test]
#[ignore = "native: real files"]
fn advances_exactly_once_before_delete() {
    let native = Native::new("bench-delete");
    let mut catalog = catalog_of(&native, &["a.jpg", "b.jpg", "c.jpg"]);
    let current = catalog.current().expect("a").path().to_owned();
    let before = catalog.current_index();
    let result = native.service.execute(
        &FileActionRequest::new(current.clone(), FileOperationType::Recycle),
        &CancelToken::none(),
    );
    assert!(result.succeeded, "{:?}", result.error);
    catalog.remove(&current);
    assert_eq!(catalog.current_index(), before);
}

// C#: BenchmarkScenarioTests.AdvancesExactlyOnceBeforeCopy
#[test]
#[ignore = "native: real files"]
fn advances_exactly_once_before_copy() {
    let native = Native::new("bench-copy");
    let catalog = catalog_of(&native, &["a.jpg", "b.jpg", "c.jpg"]);
    let current = catalog.current().expect("a").path().to_owned();
    let result = native.service.execute(
        &FileActionRequest {
            destination: Some("sel".to_owned()),
            ..FileActionRequest::new(current.clone(), FileOperationType::Copy)
        },
        &CancelToken::none(),
    );
    assert!(result.succeeded, "{:?}", result.error);
    assert!(Path::new(&current).exists(), "a copy keeps the source in the catalog");
}

// C#: BenchmarkScenarioTests.InterleavedSequenceHasNoDoubleNavigation
#[test]
#[ignore = "native: real files"]
fn interleaved_sequence_has_no_double_navigation() {
    let native = Native::new("bench-interleaved");
    let mut catalog = catalog_of(&native, &["a.jpg", "b.jpg", "c.jpg", "d.jpg"]);
    for _ in 0..2 {
        let current = catalog.current().expect("image").path().to_owned();
        let result = native.service.execute(
            &FileActionRequest {
                destination: Some("sel".to_owned()),
                ..FileActionRequest::new(current.clone(), FileOperationType::Move)
            },
            &CancelToken::none(),
        );
        assert!(result.succeeded, "{:?}", result.error);
        catalog.remove(&current);
    }
    assert_eq!(catalog.count(), 2, "two removals, two navigations, no more");
}

// C#: BenchmarkScenarioTests.PerformsNoRetrySideEffect
#[test]
#[ignore = "native: real files"]
fn performs_no_retry_side_effect() {
    let native = Native::new("bench-no-retry");
    let source = native.file("photos/a.jpg", b"photo");
    let request = FileActionRequest {
        destination: Some("sel".to_owned()),
        ..FileActionRequest::new(source.to_string_lossy().into_owned(), FileOperationType::Move)
    };
    let first = native.service.execute(&request, &CancelToken::none());
    assert!(first.succeeded, "{:?}", first.error);
    let second = native.service.execute(&request, &CancelToken::none());
    assert!(
        !second.succeeded,
        "the second run finds no source: no retry side effect"
    );
}
