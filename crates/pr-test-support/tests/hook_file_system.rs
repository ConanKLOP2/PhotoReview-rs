//! `HookFileSystem` (P-14): port of `tests/PhotoReview.Core.Tests/IO/InMemoryFileSystemHookTests.cs` (C# baseline
//! d92d6aae) plus the hooks those tests do not cover.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_core::abstractions::{CopyCreationProof, FileSystem};
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeFileSystem, FsOp, HookFileSystem};

const TEST_FILE: &str = r"C:\Testing\sample.txt";
const DEST: &str = r"C:\Testing\dest.txt";

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn setup() -> (Arc<FakeFileSystem>, HookFileSystem) {
    let disk = Arc::new(FakeFileSystem::new());
    let fs = HookFileSystem::new(Arc::clone(&disk));
    fs.create_directory(&p(r"C:\Testing")).unwrap();
    (disk, fs)
}

fn fail_with(message: &'static str) -> pr_test_support::PathHook {
    Arc::new(move |_| Some(io::Error::other(message)))
}

fn fail_pair(message: &'static str) -> pr_test_support::PairHook {
    Arc::new(move |_, _| Some(io::Error::other(message)))
}

// C#: InMemoryFileSystemHookTests.OpenReadHookInterceptsReadOperations
#[test]
fn open_read_hook_intercepts_read_operations() {
    let (_, fs) = setup();
    fs.write_all_text_atomic(&p(TEST_FILE), "data", true).unwrap();
    fs.set_open_read_hook(Some(fail_with("Simulated disk read error")));

    let err = fs.open_read_shared(&p(TEST_FILE), 65536).err().unwrap();
    assert_eq!(err.to_string(), "Simulated disk read error");
    assert_eq!(
        fs.read_all_text(&p(TEST_FILE)).unwrap_err().to_string(),
        "Simulated disk read error"
    );
    assert_eq!(
        fs.try_probe_readable(&p(TEST_FILE)).unwrap_err(),
        "Simulated disk read error"
    );
}

// C#: InMemoryFileSystemHookTests.WriteHookInterceptsWriteAllTextAtomic
#[test]
fn write_hook_intercepts_write_all_text_atomic() {
    let (disk, fs) = setup();
    fs.set_write_hook(Some(Arc::new(|_| {
        Some(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Simulated permission denied",
        ))
    })));

    let err = fs.write_all_text_atomic(&p(TEST_FILE), "content", true).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(err.to_string(), "Simulated permission denied");
    assert!(!disk.file_exists(&p(TEST_FILE)));
}

// C#: InMemoryFileSystemHookTests.DeleteHookInterceptsDeleteOperation
#[test]
fn delete_hook_intercepts_delete_operation() {
    let (disk, fs) = setup();
    fs.write_all_text_atomic(&p(TEST_FILE), "data", true).unwrap();
    fs.set_delete_hook(Some(fail_with("Simulated file in use")));

    assert_eq!(
        fs.delete(&p(TEST_FILE)).unwrap_err().to_string(),
        "Simulated file in use"
    );
    assert!(disk.file_exists(&p(TEST_FILE)));
}

// C#: InMemoryFileSystemHookTests.MoveHookInterceptsMoveOperation
#[test]
fn move_hook_intercepts_move_operation() {
    let (disk, fs) = setup();
    fs.write_all_text_atomic(&p(TEST_FILE), "data", true).unwrap();
    fs.set_move_hook(Some(fail_pair("Simulated cross-device link")));

    let err = fs.move_file(&p(TEST_FILE), &p(DEST)).unwrap_err();
    assert_eq!(err.to_string(), "Simulated cross-device link");
    assert!(disk.file_exists(&p(TEST_FILE)));
    assert!(!disk.file_exists(&p(DEST)));
}

// C#: InMemoryFileSystemHookTests.CopyHookInterceptsCopyOperation
#[test]
fn copy_hook_intercepts_copy_operation() {
    let (disk, fs) = setup();
    fs.write_all_text_atomic(&p(TEST_FILE), "data", true).unwrap();
    fs.set_copy_hook(Some(fail_pair("Simulated out of disk space")));

    assert_eq!(
        fs.copy_file(&p(TEST_FILE), &p(DEST)).unwrap_err().to_string(),
        "Simulated out of disk space"
    );
    assert!(fs.try_copy_new(&p(TEST_FILE), &p(DEST)).is_err());
    assert!(!disk.file_exists(&p(DEST)));
}

// C#: InMemoryFileSystemHookTests.StampOnMoveRewritesTheDestinationWriteTime
#[test]
fn stamp_on_move_rewrites_the_destination_write_time() {
    let (disk, fs) = setup();
    let stamp = DateTimeUtc::from_ticks(639_029_198_450_000_000).unwrap();
    disk.add_file_at(TEST_FILE, b"data", stamp);
    let dst = p(DEST);
    fs.set_stamp_on_move(Some(Arc::new(move |destination, original| {
        if destination == dst {
            DateTimeUtc::from_ticks(original.ticks() - 10_000_000).unwrap()
        } else {
            original
        }
    })));

    fs.move_file(&p(TEST_FILE), &p(DEST)).unwrap();

    assert_eq!(
        fs.get_file_stat(&p(DEST)).unwrap().last_write_utc.ticks(),
        stamp.ticks() - 10_000_000
    );
}

// C#: InMemoryFileSystemHookTests.CopyFailsAfterBytesLeavesAPartialDestinationAndThrows
#[test]
fn copy_fails_after_bytes_leaves_a_partial_destination_and_throws() {
    let (disk, fs) = setup();
    fs.write_all_text_atomic(&p(TEST_FILE), "0123456789", true).unwrap();
    fs.set_copy_fails_after_bytes(Some(3));

    assert!(fs.copy_file(&p(TEST_FILE), &p(DEST)).is_err());
    let proof = CopyCreationProof::new();
    assert!(
        fs.try_copy_new_with_proof(&p(TEST_FILE), &p(r"C:\Testing\dest.txt.2"), &proof)
            .is_err()
    );
    assert!(
        !proof.destination_created(),
        "a partial copy is never claimed (D-01 (c))"
    );

    assert_eq!(disk.file_content(DEST).as_deref(), Some(&b"012"[..]));
    assert_eq!(
        disk.file_content(r"C:\Testing\dest.txt.2").as_deref(),
        Some(&b"012"[..])
    );
}

// P-14 (no C# twin): CopyPartialHook picks the pairs it cuts short; others copy normally and raise the proof.
#[test]
fn copy_partial_hook_cuts_only_the_chosen_copy_and_never_raises_the_proof() {
    let (disk, fs) = setup();
    disk.add_file(TEST_FILE, b"0123456789");
    fs.set_copy_partial_hook(Some(Arc::new(|_, destination: &Path| {
        destination
            .ends_with("cut.txt")
            .then(|| (4, io::Error::other("cut short")))
    })));

    let proof = CopyCreationProof::new();
    let err = fs
        .try_copy_new_with_proof(&p(TEST_FILE), &p(r"C:\Testing\cut.txt"), &proof)
        .unwrap_err();
    assert_eq!(err.to_string(), "cut short");
    assert!(!proof.destination_created());
    assert_eq!(disk.file_content(r"C:\Testing\cut.txt").as_deref(), Some(&b"0123"[..]));

    // The destination exists now: "create new" says false without touching it.
    assert!(!fs.try_copy_new(&p(TEST_FILE), &p(r"C:\Testing\cut.txt")).unwrap());

    let proof = CopyCreationProof::new();
    assert!(
        fs.try_copy_new_with_proof(&p(TEST_FILE), &p(r"C:\Testing\whole.txt"), &proof)
            .unwrap()
    );
    assert!(proof.destination_created());
    assert_eq!(
        disk.file_content(r"C:\Testing\whole.txt").as_deref(),
        Some(&b"0123456789"[..])
    );
}

// P-14: MoveLeavesSource = the move copies and keeps the source (cross-volume move that could not delete).
#[test]
fn move_leaves_source_keeps_both_files() {
    let (disk, fs) = setup();
    disk.add_file(TEST_FILE, b"data");
    fs.set_move_leaves_source(true);

    fs.move_file(&p(TEST_FILE), &p(DEST)).unwrap();

    assert!(disk.file_exists(&p(TEST_FILE)));
    assert_eq!(disk.file_content(DEST).as_deref(), Some(&b"data"[..]));
}

// P-12 x P-14: the C# StatHook throws from GetFileStat; the Rust fallible stat returns it, get_file_stat says None.
#[test]
fn stat_hook_fails_the_fallible_stat_only_for_non_blank_paths() {
    let (disk, fs) = setup();
    disk.add_file(TEST_FILE, b"data");
    fs.set_stat_hook(Some(Arc::new(|path| {
        path.ends_with("sample.txt")
            .then(|| io::Error::from(io::ErrorKind::PermissionDenied))
    })));

    let err = fs.try_get_file_stat(&p(TEST_FILE)).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(fs.get_file_stat(&p(TEST_FILE)), None);
    assert_eq!(disk.count(FsOp::Stat), 0, "a hooked stat never reaches the disk");

    let blank = fs.try_get_file_stat(&p(" ")).unwrap_err();
    assert_eq!(
        blank.kind(),
        io::ErrorKind::InvalidInput,
        "blank is checked before the hook"
    );
    assert_eq!(disk.count(FsOp::Stat), 1);
}

// P-14: OpenAppendHook + the Events trace (kind, durable flag, path, thread) of IO03.
#[test]
fn open_append_hook_and_events_trace_every_append_and_move() {
    let (_, fs) = setup();
    {
        let mut stream = fs.open_append(&p(TEST_FILE), true).unwrap();
        stream.write_all(b"line\r\n").unwrap();
        assert!(stream.as_file_mut().is_none());
    }
    fs.move_file(&p(TEST_FILE), &p(DEST)).unwrap();
    fs.set_open_append_hook(Some(fail_with("journal locked")));
    assert_eq!(
        fs.open_append(&p(TEST_FILE), false).err().unwrap().to_string(),
        "journal locked"
    );

    let events = fs.events();
    let summary: Vec<(&str, bool, PathBuf)> = events.iter().map(|e| (e.kind, e.durable, e.path.clone())).collect();
    assert_eq!(
        summary,
        vec![
            ("append", true, p(TEST_FILE)),
            ("move", false, p(TEST_FILE)),
            ("append", false, p(TEST_FILE)),
        ]
    );
    assert!(events.iter().all(|e| e.thread == std::thread::current().id()));
    fs.clear_events();
    assert!(fs.events().is_empty());
}

// P-14: a hook may call back into the same file system (the C# MoveHook of the undo tests reads the journal).
#[test]
fn a_hook_can_call_back_into_the_file_system() {
    let disk = Arc::new(FakeFileSystem::new());
    disk.add_file(TEST_FILE, b"journal line");
    let fs = Arc::new(HookFileSystem::new(Arc::clone(&disk)));
    let inner = Arc::downgrade(&fs);
    fs.set_move_hook(Some(Arc::new(move |source, _| {
        let fs = inner.upgrade()?;
        let mut text = String::new();
        fs.open_read_shared(source, 16).ok()?.read_to_string(&mut text).ok()?;
        fs.events(); // takes the hooks lock again: must not deadlock
        (text == "journal line").then(|| io::Error::other("seen"))
    })));
    assert_eq!(fs.move_file(&p(TEST_FILE), &p(DEST)).unwrap_err().to_string(), "seen");
}

// P-14: EnumerateFilesHook / DeleteDirectoryHook.
#[test]
fn enumerate_and_delete_directory_hooks() {
    let (disk, fs) = setup();
    disk.add_dir(r"C:\Testing\empty");
    fs.set_enumerate_files_hook(Some(fail_with("listing denied")));
    assert_eq!(
        fs.enumerate_files(&p(r"C:\Testing"), "*").unwrap_err().to_string(),
        "listing denied"
    );
    fs.set_delete_directory_hook(Some(fail_with("in use")));
    assert!(!fs.try_delete_empty_directory(&p(r"C:\Testing\empty")));
    assert!(disk.directory_exists(&p(r"C:\Testing\empty")));
    fs.set_delete_directory_hook(None);
    assert!(fs.try_delete_empty_directory(&p(r"C:\Testing\empty")));
}

// P-14 (C# AddReparsePoint / ResolveRealPath / ResolveRealPathHook, SEC-01 and RV-S06).
#[test]
fn reparse_points_are_resolved_segment_by_segment() {
    let (disk, fs) = setup();
    fs.add_reparse_point(r"C:\Testing\link", r"D:\Elsewhere\");
    assert!(disk.directory_exists(&p(r"C:\Testing\link")));

    assert_eq!(
        fs.resolve_real_path(&p(r"C:\testing\LINK\sub\a.jpg")).unwrap(),
        p(r"D:\Elsewhere\sub\a.jpg")
    );
    assert_eq!(
        fs.resolve_real_path(&p(r"C:\Testing\a.jpg")).unwrap(),
        p(r"C:\Testing\a.jpg")
    );
    assert_eq!(
        fs.resolve_real_path(&p(r"C:\")).unwrap(),
        p(r"C:\"),
        "a bare root keeps its separator"
    );

    fs.set_resolve_real_path_hook(Some(Arc::new(|_| Some(io::Error::from_raw_os_error(1921)))));
    assert_eq!(
        fs.resolve_real_path(&p(r"C:\Testing\link")).unwrap_err().raw_os_error(),
        Some(1921)
    );
}
