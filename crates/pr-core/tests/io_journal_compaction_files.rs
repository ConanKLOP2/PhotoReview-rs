//! Port of the C# native tests of the journal compaction Windows primitives (C# baseline d92d6aae,
//! `tests/PhotoReview.Core.Tests/IO/`): `PhysicalJournalCompactionFilesTests` (the rename and lock primitives) and the two
//! compaction facts of `PhysicalIoMutationGapTests`. Port of `PhotoReview.Core/IO/PhysicalJournalCompactionFiles.cs`
//! (`pr_core::io::journal_compaction_files`). Contract: `contracts/W3-02.md`.
//!
//! Every test is `#[ignore = "native"]` and runs only on Windows, inside its own `TempRoot`, with
//! `cargo test -p pr-core --test io_journal_compaction_files -- --ignored`. They touch only files they create; the Recycle
//! Bin is never involved.
#![cfg(windows)]

use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use pr_core::abstractions::JournalCompactionFiles;
use pr_core::file_actions::errors::ERROR_FILE_EXISTS;
use pr_core::io::journal_compaction_files::{PhysicalJournalCompactionFiles, os_error_code};
use pr_test_support::TempRoot;

const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_SHARE_DELETE: u32 = 0x4;
const SHARE_READ_WRITE_DELETE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// `ERROR_SHARING_VIOLATION`: the HRESULT 0x80070020 of the C# tests.
const ERROR_SHARING_VIOLATION: i32 = 32;

/// C# `TargetPath`: `operations.jsonl` in the test folder.
fn target_path(root: &TempRoot) -> PathBuf {
    root.join("operations.jsonl")
}

/// C# `NewTempPath()`: a fresh `operations.jsonl.<32 hex>.compact.tmp` path that does not exist yet.
fn new_temp_path(root: &TempRoot) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    root.join(format!(
        "operations.jsonl.{:016x}{:016x}.compact.tmp",
        std::process::id(),
        n
    ))
}

/// Clears the read-only attribute again on drop, so `TempRoot` can remove the folder even when a test fails.
struct ReadOnlyGuard(PathBuf);

impl ReadOnlyGuard {
    fn set(path: &Path) -> Self {
        let mut permissions = fs::metadata(path).expect("the leftover exists").permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions).expect("set the read-only attribute");
        Self(path.to_path_buf())
    }
}

impl Drop for ReadOnlyGuard {
    // Windows only: `set_readonly(false)` clears FILE_ATTRIBUTE_READONLY, the clippy lint is about Unix mode bits.
    #[allow(clippy::permissions_set_readonly_false)]
    fn drop(&mut self) {
        if let Ok(metadata) = fs::metadata(&self.0) {
            let mut permissions = metadata.permissions();
            permissions.set_readonly(false);
            let _ = fs::set_permissions(&self.0, permissions);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// PhysicalJournalCompactionFilesTests
// ---------------------------------------------------------------------------------------------------------------

// C#: PhysicalJournalCompactionFilesTests.ReplaceAtomically_TargetOpenByCompatibleReader_SucceedsAndReaderSeesOldContent
#[test]
#[ignore = "native"]
fn replace_atomically_target_open_by_compatible_reader_succeeds_and_reader_sees_old_content() {
    let root = TempRoot::new("journal-compaction-files");
    let target = target_path(&root);
    fs::write(&target, b"old-content").expect("write the target");
    let mut reader = OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ_WRITE_DELETE)
        .open(&target)
        .expect("a compatible reader opens the target");
    let files = PhysicalJournalCompactionFiles;

    {
        let mut replacement = files
            .create_staged_replacement(&new_temp_path(&root))
            .expect("create the staged file");
        replacement.write(b"new-content").expect("write");
        replacement.flush_to_disk().expect("flush");
        replacement
            .replace_atomically(&target)
            .expect("the rename succeeds under a compatible reader");
        // The staged-replacement handle itself only shares Read (matching the real writer): it is dropped at the end of
        // this scope, before anything else opens the (now renamed) target for reading.
    }

    let mut old_bytes = vec![0u8; "old-content".len()];
    reader
        .read_exact(&mut old_bytes)
        .expect("the reader still holds the old content");
    assert_eq!(old_bytes, b"old-content");
    assert_eq!(fs::read(&target).expect("read the target"), b"new-content");
}

// C#: PhysicalJournalCompactionFilesTests.ReplaceAtomically_TargetLockedByOpenReadDenyWriters_Succeeds
#[test]
#[ignore = "native"]
fn replace_atomically_target_locked_by_open_read_deny_writers_succeeds() {
    let root = TempRoot::new("journal-compaction-files");
    let target = target_path(&root);
    fs::write(&target, b"old-content").expect("write the target");
    let files = PhysicalJournalCompactionFiles;
    let mut locked = files
        .open_read_deny_writers(&target)
        .expect("the deny-writers lock opens");

    {
        let mut replacement = files
            .create_staged_replacement(&new_temp_path(&root))
            .expect("create the staged file");
        replacement.write(b"new-content").expect("write");
        replacement.flush_to_disk().expect("flush");
        replacement
            .replace_atomically(&target)
            .expect("the rename succeeds while the target is locked");
    }

    let mut old_bytes = vec![0u8; "old-content".len()];
    locked
        .read_exact(&mut old_bytes)
        .expect("the lock holder still reads the old content");
    assert_eq!(old_bytes, b"old-content");
    assert_eq!(fs::read(&target).expect("read the target"), b"new-content");
}

// C#: PhysicalJournalCompactionFilesTests.ReplaceAtomically_TargetHeldByAppender_ThrowsTargetUnchangedTempDeleted
#[test]
#[ignore = "native"]
fn replace_atomically_target_held_by_appender_throws_target_unchanged_temp_deleted() {
    let root = TempRoot::new("journal-compaction-files");
    let target = target_path(&root);
    fs::write(&target, b"old-content").expect("write the target");
    let appender = OpenOptions::new()
        .append(true)
        .share_mode(FILE_SHARE_READ)
        .open(&target)
        .expect("the appender opens the target");
    let temp = new_temp_path(&root);
    let files = PhysicalJournalCompactionFiles;

    let mut replacement = files.create_staged_replacement(&temp).expect("create the staged file");
    replacement.write(b"new-content").expect("write");
    replacement.flush_to_disk().expect("flush");

    let err = replacement
        .replace_atomically(&target)
        .expect_err("the appender holds the target");
    assert_eq!(os_error_code(&err), Some(ERROR_SHARING_VIOLATION));
    drop(replacement);
    drop(appender); // the appender eventually releases; a Write-sharing handle blocks even reads until then

    assert_eq!(fs::read(&target).expect("read the target"), b"old-content");
    assert!(
        !temp.exists(),
        "the temp file is deleted when the staged replacement is dropped"
    );
}

// C#: PhysicalJournalCompactionFilesTests.Dispose_WithoutReplace_DeletesTempFile
#[test]
#[ignore = "native"]
fn dispose_without_replace_deletes_temp_file() {
    let root = TempRoot::new("journal-compaction-files");
    let temp = new_temp_path(&root);
    let files = PhysicalJournalCompactionFiles;
    let mut replacement = files.create_staged_replacement(&temp).expect("create the staged file");
    replacement.write(b"content").expect("write");
    replacement.flush_to_disk().expect("flush");
    assert!(temp.exists());

    drop(replacement);

    assert!(!temp.exists());
}

// C#: PhysicalJournalCompactionFilesTests.OpenReadDenyWriters_BlocksAppendOpens_AllowsCompatibleReaders
#[test]
#[ignore = "native"]
fn open_read_deny_writers_blocks_append_opens_allows_compatible_readers() {
    let root = TempRoot::new("journal-compaction-files");
    let target = target_path(&root);
    fs::write(&target, b"content").expect("write the target");
    let files = PhysicalJournalCompactionFiles;
    let _locked = files
        .open_read_deny_writers(&target)
        .expect("the deny-writers lock opens");

    let err = OpenOptions::new()
        .append(true)
        .share_mode(FILE_SHARE_READ)
        .open(&target)
        .expect_err("the lock refuses an appender");
    assert_eq!(os_error_code(&err), Some(ERROR_SHARING_VIOLATION));

    let mut reader = OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ_WRITE_DELETE)
        .open(&target)
        .expect("a compatible reader opens the target");
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).expect("read");
    assert_eq!(bytes, b"content");
}

// ---------------------------------------------------------------------------------------------------------------
// PhysicalIoMutationGapTests: the two compaction facts.
// ---------------------------------------------------------------------------------------------------------------

// C#: PhysicalIoMutationGapTests.CreateStagedReplacement_FileAlreadyExists_ThrowsIOExceptionWithFileExistsAndKeepsTheFile
#[test]
#[ignore = "native"]
fn create_staged_replacement_file_already_exists_throws_io_error_with_file_exists_and_keeps_the_file() {
    let root = TempRoot::new("physical-io-gap");
    let temp = root.join("staged.tmp");
    fs::write(&temp, [1u8, 2, 3]).expect("write the existing file");
    let files = PhysicalJournalCompactionFiles;

    // `Result::err` rather than `expect_err`: the Ok type (a boxed trait object) is not Debug.
    let err = files
        .create_staged_replacement(&temp)
        .err()
        .expect("CREATE_NEW never reuses an existing file");

    assert_eq!(os_error_code(&err), Some(ERROR_FILE_EXISTS)); // ERROR_FILE_EXISTS: CREATE_NEW never reuses or truncates
    assert_eq!(fs::read(&temp).expect("read the file"), vec![1u8, 2, 3]);
}

// C#: PhysicalIoMutationGapTests.StagedReplacementDispose_LeftoverTempThatCannotBeDeleted_DoesNotThrow
#[test]
#[ignore = "native"]
fn staged_replacement_dispose_leftover_temp_that_cannot_be_deleted_does_not_throw() {
    let root = TempRoot::new("physical-io-gap");
    let temp = root.join("staged.tmp");
    let files = PhysicalJournalCompactionFiles;
    let mut staged = files.create_staged_replacement(&temp).expect("create the staged file");
    staged.write(b"x").expect("write");
    // File::remove_file would delete a read-only file; DeleteFileW (the C# File.Delete) refuses it, and Drop must
    // leave it behind without panicking.
    let _reset = ReadOnlyGuard::set(&temp);

    let outcome = panic::catch_unwind(AssertUnwindSafe(move || drop(staged)));

    assert!(outcome.is_ok(), "dropping a staged replacement must not panic");
    assert!(
        temp.exists(),
        "the read-only leftover stays for the next compaction to remove"
    );
}

// ---------------------------------------------------------------------------------------------------------------
// Rust-only parity tests (no C# twin): the root-path error mapping, the flush before the rename, and paths of 260 UTF-16
// units or more (the `\\?\` form .NET adds itself). Same rules as above: own `TempRoot`, never the Recycle Bin.
// ---------------------------------------------------------------------------------------------------------------

/// `ERROR_ACCESS_DENIED`: what .NET's `FileStream` reports (`UnauthorizedAccessException`, 0x80070005) for a bare root.
const ERROR_ACCESS_DENIED: i32 = 5;

/// A compatible reader of a file that a staged replacement still holds (GENERIC_WRITE | DELETE, sharing Read only).
fn read_through_compatible_reader(path: &Path) -> Vec<u8> {
    let mut reader = OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ_WRITE_DELETE)
        .open(path)
        .expect("a compatible reader opens the file");
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).expect("read");
    bytes
}

/// A path under `root` of exactly `units` UTF-16 units ending in `file_name`, its parent folders created (std creates
/// long folders itself).
fn path_of_length(root: &TempRoot, units: usize, file_name: &str) -> PathBuf {
    let base = root.path().to_str().expect("a Unicode temp root").to_owned();
    assert!(
        base.len() + 1 + file_name.len() + 2 <= units,
        "the temp root is too long for this test"
    );
    let mut folder = PathBuf::from(&base);
    let mut used = base.len();
    loop {
        let left = units - used - 1 - file_name.len(); // what remains before `\<file_name>`
        if left == 0 {
            break;
        }
        // Each folder costs `\` plus its name; keep names <= 200 and never leave a remainder of exactly 1.
        let name_len = if left <= 201 { left - 1 } else { (left - 2).min(200) };
        folder.push("d".repeat(name_len));
        used += 1 + name_len;
    }
    fs::create_dir_all(&folder).expect("create the long folders");
    let path = folder.join(file_name);
    assert_eq!(path.as_os_str().encode_wide().count(), units);
    path
}

/// C# `new FileStream("C:\\", ...)` throws `UnauthorizedAccessException` (0x80070005), not `DirectoryNotFoundException`:
/// `SafeFileHandle.Open` maps ERROR_PATH_NOT_FOUND on a bare root to access denied.
#[test]
#[ignore = "native"]
fn open_read_deny_writers_on_a_root_is_access_denied_like_file_stream() {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
    let files = PhysicalJournalCompactionFiles;
    for root in [format!("{drive}\\"), format!("{drive}/")] {
        let err = files
            .open_read_deny_writers(Path::new(&root))
            .err()
            .expect("a root is not a file");
        assert_eq!(os_error_code(&err), Some(ERROR_ACCESS_DENIED), "{root}");
    }
}

/// `flush_to_disk` pushes the buffered bytes into the file: another handle sees them while the staged replacement is
/// still open.
#[test]
#[ignore = "native"]
fn flush_to_disk_makes_the_buffered_bytes_visible_before_the_handle_closes() {
    let root = TempRoot::new("journal-compaction-files");
    let temp = new_temp_path(&root);
    let files = PhysicalJournalCompactionFiles;
    let mut replacement = files.create_staged_replacement(&temp).expect("create the staged file");
    // Just under the C# 64 KiB FileStream buffer: nothing reaches the file before the flush.
    let payload: Vec<u8> = (0..65_535usize).map(|i| b'a' + (i % 26) as u8).collect();
    replacement.write(&payload).expect("write");
    assert_eq!(read_through_compatible_reader(&temp), b"", "still in the 64 KiB buffer");

    replacement.flush_to_disk().expect("flush");

    assert_eq!(read_through_compatible_reader(&temp), payload);
}

/// `replace_atomically` flushes BEFORE renaming (C# `Flush(flushToDisk: true)` first): the renamed journal already holds
/// every byte while the staged replacement is still alive, with no explicit `flush_to_disk` call by the caller.
#[test]
#[ignore = "native"]
fn replace_atomically_flushes_the_buffered_bytes_before_the_rename() {
    let root = TempRoot::new("journal-compaction-files");
    let target = target_path(&root);
    fs::write(&target, b"old-content").expect("write the target");
    let files = PhysicalJournalCompactionFiles;
    let mut replacement = files
        .create_staged_replacement(&new_temp_path(&root))
        .expect("create the staged file");
    replacement.write(b"new-content").expect("write");

    replacement.replace_atomically(&target).expect("rename");

    assert_eq!(read_through_compatible_reader(&target), b"new-content");
    drop(replacement);
    assert_eq!(fs::read(&target).expect("read the target"), b"new-content");
}

/// .NET opens a path of 260 UTF-16 units or more in the `\\?\` form; the lock opens and reads it.
#[test]
#[ignore = "native"]
fn open_read_deny_writers_opens_paths_of_260_units_or_more() {
    let root = TempRoot::new("jcf-long");
    for units in [260, 300] {
        let target = path_of_length(&root, units, &format!("operations-{units}.jsonl"));
        fs::write(&target, b"long-content").expect("write the long target");
        let files = PhysicalJournalCompactionFiles;

        let mut locked = files
            .open_read_deny_writers(&target)
            .expect("the lock opens a long path");

        let mut bytes = Vec::new();
        locked.read_to_end(&mut bytes).expect("read");
        assert_eq!(bytes, b"long-content", "{units} units");
    }
}

/// `Drop` deletes a staged replacement at 260 units or more like `File.Delete` (`DeleteFileW` on the `\\?\` form), also
/// when the caller passed the `\\?\` form itself (a device path is never prefixed twice).
#[test]
#[ignore = "native"]
fn dispose_without_replace_deletes_temp_files_of_260_units_or_more() {
    let root = TempRoot::new("jcf-long");
    let files = PhysicalJournalCompactionFiles;
    let mut cases = Vec::new();
    for units in [260, 300] {
        let temp = path_of_length(&root, units, &format!("staged-{units}.compact.tmp"));
        cases.push((temp.clone(), temp));
    }
    let plain = path_of_length(&root, 280, "staged-device.compact.tmp");
    let device = PathBuf::from(format!(r"\\?\{}", plain.display()));
    cases.push((device, plain));

    for (given, on_disk) in cases {
        let mut replacement = files
            .create_staged_replacement(&given)
            .expect("create the long staged file");
        replacement.write(b"content").expect("write");
        assert!(on_disk.exists(), "{}", given.display());

        drop(replacement);

        assert!(!on_disk.exists(), "deleted: {}", given.display());
    }
}

/// C# `CreateFileW(.., FILE_SHARE_READ, ..)` for the staged replacement: while it is open, every other writer is turned
/// away with a sharing violation (readers are still let in).
#[test]
#[ignore = "native"]
fn staged_replacement_denies_other_writers_and_admits_readers() {
    let root = TempRoot::new("jcf-share");
    let temp = new_temp_path(&root);
    let files = PhysicalJournalCompactionFiles;
    let staged = files.create_staged_replacement(&temp).expect("create");

    let err = OpenOptions::new()
        .write(true)
        .share_mode(SHARE_READ_WRITE_DELETE)
        .open(&temp)
        .expect_err("a writer must be refused");
    assert_eq!(os_error_code(&err), Some(ERROR_SHARING_VIOLATION));
    OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ_WRITE_DELETE)
        .open(&temp)
        .expect("a compatible reader is admitted");

    drop(staged);
    assert!(!temp.exists());
}

/// C# `ReplaceAtomically`: `ArgumentException.ThrowIfNullOrWhiteSpace(destination)` runs first, before the flush and
/// `Path.GetFullPath`; nothing is renamed and the temp file is still deleted on drop.
#[test]
#[ignore = "native"]
fn replace_atomically_blank_destination_is_the_whitespace_argument_error() {
    let root = TempRoot::new("jcf-blank");
    let temp = new_temp_path(&root);
    let files = PhysicalJournalCompactionFiles;
    let mut staged = files.create_staged_replacement(&temp).expect("create");
    staged.write(b"content").expect("write");
    for blank in ["", " ", "\t", " \t\r\n"] {
        let err = staged.replace_atomically(Path::new(blank)).expect_err("blank");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{blank:?}");
        assert_eq!(
            err.to_string(),
            "The value cannot be an empty string or composed entirely of whitespace.",
            "{blank:?}"
        );
    }
    assert!(!root.join("\t").exists());
    assert!(temp.exists());
    drop(staged);
    assert!(!temp.exists());
}
