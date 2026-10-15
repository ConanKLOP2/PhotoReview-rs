//! `FaultInjectionFileSystem` (P-14): every `FaultKind` at every kind of mutating call, the dead-process rule, the
//! counting and `applies`. Port of the behaviour of `tests/PhotoReview.Core.Tests/Fakes/FaultInjectionFileSystem.cs`
//! (C# baseline d92d6aae; the C# fake has no tests of its own, the matrices exercise it).

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pr_core::abstractions::{CopyCreationProof, FileSystem};
use pr_test_support::{
    DISK_FULL_MESSAGE, FakeFileSystem, FaultInjectionFileSystem, FaultKind, FaultPlan, is_process_crash,
};

const SRC: &str = r"C:\photos\a.jpg";
const DST: &str = r"C:\out\a.jpg";
const JOURNAL: &str = r"C:\data\operations.jsonl";

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn setup() -> (Arc<FakeFileSystem>, FaultInjectionFileSystem) {
    let disk = Arc::new(FakeFileSystem::new());
    disk.add_file(SRC, b"0123456789");
    disk.add_dir(r"C:\out");
    disk.add_dir(r"C:\data");
    let fs = FaultInjectionFileSystem::new(Arc::clone(&disk));
    (disk, fs)
}

fn plan(at: usize, kind: FaultKind) -> Option<FaultPlan> {
    Some(FaultPlan { at, kind })
}

#[test]
fn healthy_run_counts_every_mutation_in_order() {
    let (disk, fs) = setup();
    fs.arm(None);
    fs.create_directory(&p(r"C:\new")).unwrap();
    fs.copy_file(&p(SRC), &p(r"C:\out\copy.jpg")).unwrap();
    assert!(fs.try_copy_new(&p(SRC), &p(r"C:\out\new.jpg")).unwrap());
    fs.move_file(&p(SRC), &p(DST)).unwrap();
    fs.open_append(&p(JOURNAL), true).unwrap().write_all(b"x\r\n").unwrap();
    fs.write_all_text_atomic(&p(r"C:\data\s.json"), "{}", false).unwrap();
    fs.delete(&p(DST)).unwrap();
    // Reads and the empty-folder cleanup are not numbered (C#: TryDeleteEmptyDirectory is forwarded as is).
    fs.get_file_stat(&p(SRC));
    fs.read_all_text(&p(JOURNAL)).unwrap();
    assert!(fs.try_delete_empty_directory(&p(r"C:\new")));

    assert_eq!(
        fs.kinds(),
        ["mkdir", "copy", "copy", "move", "append", "write", "delete"]
    );
    assert_eq!(fs.mutations(), 7);
    assert!(!fs.dead());
    assert_eq!(disk.file_content(JOURNAL).as_deref(), Some(&b"x\r\n"[..]));
}

#[test]
fn crash_before_has_no_effect_and_kills_every_later_call() {
    let (disk, fs) = setup();
    fs.arm(plan(1, FaultKind::CrashBefore));

    let err = fs.move_file(&p(SRC), &p(DST)).unwrap_err();
    assert!(is_process_crash(&err));
    assert_eq!(err.to_string(), "simulated process death");
    assert!(fs.dead());
    assert!(disk.file_exists(&p(SRC)) && !disk.file_exists(&p(DST)));

    // Every later call fails (or answers "nothing") and the disk never changes again.
    assert!(is_process_crash(&fs.delete(&p(SRC)).unwrap_err()));
    assert!(is_process_crash(&fs.read_all_text(&p(SRC)).unwrap_err()));
    assert!(is_process_crash(&fs.try_get_file_stat(&p(SRC)).unwrap_err()));
    assert!(is_process_crash(
        &fs.enumerate_files(&p(r"C:\photos"), "*").unwrap_err()
    ));
    assert!(fs.open_read_shared(&p(SRC), 16).is_err());
    assert!(fs.try_probe_readable(&p(SRC)).is_err());
    assert!(!fs.file_exists(&p(SRC)));
    assert!(fs.get_file_stat(&p(SRC)).is_none());
    assert!(disk.file_exists(&p(SRC)));
    assert_eq!(fs.mutations(), 1, "a dead process numbers nothing");

    // A restart (re-arm) revives it over the same disk.
    fs.arm(None);
    assert!(!fs.dead());
    assert!(fs.file_exists(&p(SRC)));
}

#[test]
fn crash_after_applies_the_effect_then_dies() {
    let (disk, fs) = setup();
    fs.arm(plan(2, FaultKind::CrashAfter));
    fs.create_directory(&p(r"C:\new")).unwrap();
    assert!(is_process_crash(&fs.move_file(&p(SRC), &p(DST)).unwrap_err()));
    assert!(!disk.file_exists(&p(SRC)) && disk.file_exists(&p(DST)));
    assert!(fs.dead());
}

#[test]
fn crash_partial_copy_and_move_leave_a_truncated_destination_and_the_source() {
    for kind in ["copy", "move", "try_copy_new"] {
        let (disk, fs) = setup();
        fs.arm(plan(1, FaultKind::CrashPartial));
        let proof = CopyCreationProof::new();
        let err = match kind {
            "copy" => fs.copy_file(&p(SRC), &p(DST)).unwrap_err(),
            "move" => fs.move_file(&p(SRC), &p(DST)).unwrap_err(),
            _ => fs.try_copy_new_with_proof(&p(SRC), &p(DST), &proof).unwrap_err(),
        };
        assert!(is_process_crash(&err), "{kind}");
        assert_eq!(disk.file_content(DST).as_deref(), Some(&b"01234"[..]), "{kind}");
        assert_eq!(
            disk.peek_stat(DST).map(|s| s.last_write_utc),
            disk.peek_stat(SRC).map(|s| s.last_write_utc)
        );
        assert!(disk.file_exists(&p(SRC)), "{kind}: the source is untouched");
        assert!(!proof.destination_created(), "{kind}: a partial copy is unclaimed");
        assert!(fs.dead());
    }
}

#[test]
fn partial_copy_never_overwrites_an_existing_destination() {
    let (disk, fs) = setup();
    disk.add_file(DST, b"foreign");
    fs.arm(plan(1, FaultKind::FailPartial));
    let err = fs.copy_file(&p(SRC), &p(DST)).unwrap_err();
    assert_eq!(err.to_string(), DISK_FULL_MESSAGE);
    assert_eq!(disk.file_content(DST).as_deref(), Some(&b"foreign"[..]));
    assert!(!fs.dead(), "a fail kind keeps the process alive");
}

#[test]
fn crash_move_copied_not_deleted_leaves_both_files() {
    let (disk, fs) = setup();
    fs.arm(plan(1, FaultKind::CrashMoveCopiedNotDeleted));
    assert!(is_process_crash(&fs.move_file(&p(SRC), &p(DST)).unwrap_err()));
    assert_eq!(disk.file_content(DST).as_deref(), Some(&b"0123456789"[..]));
    assert!(disk.file_exists(&p(SRC)));
}

#[test]
fn fail_once_and_fail_unauthorized_have_no_effect_and_the_process_lives_on() {
    let (disk, fs) = setup();
    fs.arm(plan(1, FaultKind::FailOnce));
    let err = fs.delete(&p(SRC)).unwrap_err();
    assert!(!is_process_crash(&err));
    assert_eq!(err.kind(), io::ErrorKind::Other);
    assert!(disk.file_exists(&p(SRC)));
    fs.delete(&p(SRC)).unwrap();
    assert!(!disk.file_exists(&p(SRC)));

    fs.arm(plan(1, FaultKind::FailUnauthorized));
    let err = fs.write_all_text_atomic(&p(r"C:\data\s.json"), "{}", true).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(!disk.file_exists(&p(r"C:\data\s.json")));
    assert!(!fs.dead());
}

#[test]
fn cancel_after_applies_the_effect_then_runs_the_cancel_hook() {
    let (disk, fs) = setup();
    let cancels = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&cancels);
    fs.set_cancel_hook(Some(Arc::new(move || {
        seen.fetch_add(1, Ordering::SeqCst);
    })));
    fs.arm(plan(1, FaultKind::CancelAfter));
    fs.move_file(&p(SRC), &p(DST)).unwrap();
    assert!(disk.file_exists(&p(DST)));
    assert_eq!(cancels.load(Ordering::SeqCst), 1);
    fs.copy_file(&p(DST), &p(r"C:\out\b.jpg")).unwrap();
    assert_eq!(cancels.load(Ordering::SeqCst), 1, "only at the planned call");
}

#[test]
fn partial_append_opens_the_file_then_fails_without_writing() {
    // C# OpenAppend: Mutate runs the partial open and then THROWS for both partial kinds, so the caller never gets the
    // (torn) stream and no byte is written: pinned exactly as the oracle behaves.
    for (kind, dies) in [(FaultKind::CrashPartial, true), (FaultKind::FailPartial, false)] {
        let (disk, fs) = setup();
        disk.add_file(JOURNAL, b"old\r\n");
        fs.arm(plan(1, kind));
        let err = fs.open_append(&p(JOURNAL), true).err().unwrap();
        assert_eq!(is_process_crash(&err), dies, "{kind:?}");
        if !dies {
            assert_eq!(err.to_string(), DISK_FULL_MESSAGE);
        }
        assert_eq!(fs.dead(), dies, "{kind:?}");
        assert_eq!(disk.file_content(JOURNAL).as_deref(), Some(&b"old\r\n"[..]), "{kind:?}");
    }
}

#[test]
fn healthy_append_stream_is_the_disks_stream() {
    let (disk, fs) = setup();
    fs.arm(plan(2, FaultKind::FailOnce));
    let mut stream = fs.open_append(&p(JOURNAL), false).unwrap();
    assert!(stream.as_file_mut().is_none());
    stream.write_all(b"line\r\n").unwrap();
    assert_eq!(disk.file_content(JOURNAL).as_deref(), Some(&b"line\r\n"[..]));
    assert!(
        fs.open_append(&p(JOURNAL), false).is_err(),
        "the second append is the planned failure"
    );
}

#[test]
fn applies_matches_the_csharp_table() {
    let kinds = ["mkdir", "move", "copy", "delete", "append", "write", "bin.recycle"];
    for kind in FaultKind::ALL {
        for mutation in kinds {
            let expected = match kind {
                FaultKind::CrashPartial | FaultKind::FailPartial => matches!(mutation, "copy" | "move" | "append"),
                FaultKind::CrashMoveCopiedNotDeleted => mutation == "move",
                FaultKind::CrashAfter => mutation != "append",
                _ => true,
            };
            assert_eq!(kind.applies(mutation), expected, "{kind:?} at {mutation}");
        }
    }
}

#[test]
fn mutate_numbers_external_steps_of_a_fake_bin() {
    let (disk, fs) = setup();
    fs.arm(plan(2, FaultKind::CrashBefore));
    fs.mutate("bin.recycle", &mut || disk.delete(Path::new(SRC)), None, None)
        .unwrap();
    let err = fs.mutate("bin.restore", &mut || Ok(()), None, None).unwrap_err();
    assert!(is_process_crash(&err));
    assert_eq!(fs.kinds(), ["bin.recycle", "bin.restore"]);
}

#[test]
fn snapshot_lists_direct_files_of_each_folder() {
    let (disk, fs) = setup();
    disk.add_file(r"C:\photos\sub\deep.jpg", b"x");
    let snapshot = fs.snapshot(&[Path::new(r"C:\photos"), Path::new(r"C:\missing")]);
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot.values().next().map(Vec::as_slice), Some(&b"0123456789"[..]));
}
