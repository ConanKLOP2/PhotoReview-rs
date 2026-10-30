//! cl-fsflush: makes the durable half of `PhysicalFileSystem::write_all_text_atomic` (D-05) and the SequentialScan hint of
//! `open_read_shared` (INV-8) observable. The kernel keeps both as file-object mode bits that
//! `NtQueryInformationFile(FileModeInformation)` reports for a handle: `FILE_WRITE_THROUGH` (0x2) for
//! `FILE_FLAG_WRITE_THROUGH` and `FILE_SEQUENTIAL_ONLY` (0x4) for `FILE_FLAG_SEQUENTIAL_SCAN`. The temp file of the
//! atomic write is closed before the call returns, so the handle is read through the
//! `write_all_text_atomic_observed` seam (the P-11 `FlushToDisk` pattern); the flush itself is observed as the
//! `AtomicWriteStep::Flush` step. Real disk in a private `TempRoot`; run with `-- --include-ignored --test-threads=1`.
#![cfg(windows)]

use std::cell::RefCell;
use std::ffi::c_void;
use std::fs::File;
use std::io::Write;
use std::os::windows::io::AsRawHandle;

use pr_core::abstractions::FileSystem;
use pr_core::io::physical::{AtomicWriteStep, PhysicalFileSystem, temp_file_flushes_on_this_thread};
use pr_test_support::TempRoot;

const FILE_WRITE_THROUGH: u32 = 0x2;
const FILE_SEQUENTIAL_ONLY: u32 = 0x4;
const FILE_MODE_INFORMATION: u32 = 16;

#[repr(C)]
struct IoStatusBlock {
    status: usize,
    information: usize,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryInformationFile(
        handle: *mut c_void,
        io_status: *mut IoStatusBlock,
        info: *mut c_void,
        length: u32,
        class: u32,
    ) -> i32;
}

/// The `FILE_MODE_INFORMATION.Mode` bits of an open handle.
fn mode_of(file: &File) -> u32 {
    let mut io = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut mode = 0u32;
    // SAFETY: the handle is open for the whole call (borrowed from `file`); `io` and `mode` are live locals of exactly
    // the sizes the class FileModeInformation (a single ULONG) needs.
    let status = unsafe {
        NtQueryInformationFile(
            file.as_raw_handle(),
            &mut io,
            (&mut mode as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
            FILE_MODE_INFORMATION,
        )
    };
    assert_eq!(
        status, 0,
        "NtQueryInformationFile(FileModeInformation) failed: {status:#x}"
    );
    mode
}

/// Runs `write_all_text_atomic_observed` and returns every `(step, mode of the temp handle)` the observer saw.
fn observe_atomic_write(durable: bool) -> (Vec<(AtomicWriteStep, u32)>, TempRoot) {
    let root = TempRoot::new("fsflush-atomic");
    let target = root.join("out.txt");
    let seen = RefCell::new(Vec::new());
    PhysicalFileSystem
        .write_all_text_atomic_observed(&target, "payload", durable, &mut |step, file| {
            seen.borrow_mut().push((step, mode_of(file)));
            if step == AtomicWriteStep::Flush {
                file.sync_all()?;
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "payload");
    (seen.into_inner(), root)
}

// physical.rs write_all_text_atomic_observed, `if durable { FILE_FLAG_WRITE_THROUGH } else { 0 }` (guard -> true/false,
// constant -> 0): the temp handle of a durable write is write-through, a non-durable one is not.
#[test]
#[ignore = "native: real disk"]
fn durable_atomic_write_opens_the_temp_file_write_through() {
    let (seen, _root) = observe_atomic_write(true);
    assert!(!seen.is_empty());
    for (step, mode) in seen {
        assert_eq!(
            mode & FILE_WRITE_THROUGH,
            FILE_WRITE_THROUGH,
            "{step:?}: temp handle must be write-through"
        );
    }
}

#[test]
#[ignore = "native: real disk"]
fn non_durable_atomic_write_does_not_open_write_through() {
    let (seen, _root) = observe_atomic_write(false);
    assert_eq!(seen.len(), 1, "non-durable: only the Written step");
    assert_eq!(seen[0].1 & FILE_WRITE_THROUGH, 0);
}

// physical.rs write_all_text_atomic_observed, `if durable { observe(Flush) }` (guard -> true/false): a durable write
// flushes exactly once, after the bytes were written; a non-durable write never flushes.
#[test]
#[ignore = "native: real disk"]
fn durable_atomic_write_flushes_once_after_writing_and_non_durable_never() {
    let (seen, _root) = observe_atomic_write(true);
    let steps: Vec<_> = seen.iter().map(|(s, _)| *s).collect();
    assert_eq!(steps, [AtomicWriteStep::Written, AtomicWriteStep::Flush]);
    let (seen, _root) = observe_atomic_write(false);
    assert_eq!(
        seen.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        [AtomicWriteStep::Written]
    );
}

// A flush error must fail the write and leave neither the target nor the temp file behind.
#[test]
#[ignore = "native: real disk"]
fn failed_flush_fails_the_write_and_removes_the_temp_file() {
    let root = TempRoot::new("fsflush-fail");
    let target = root.join("out.txt");
    let err = PhysicalFileSystem
        .write_all_text_atomic_observed(&target, "payload", true, &mut |step, _| match step {
            AtomicWriteStep::Flush => Err(std::io::Error::other("flush failed")),
            AtomicWriteStep::Written => Ok(()),
        })
        .unwrap_err();
    assert_eq!(err.to_string(), "flush failed");
    assert!(!target.exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0, "temp file removed");
}

// The production trait path (durable and not) still produces the complete file.
#[test]
#[ignore = "native: real disk"]
fn trait_write_all_text_atomic_writes_the_complete_file_in_both_modes() {
    let root = TempRoot::new("fsflush-trait");
    let target = root.join("sub").join("out.txt");
    for (durable, text) in [(true, "durable text"), (false, "fast text")] {
        PhysicalFileSystem
            .write_all_text_atomic(&target, text, durable)
            .unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), text);
    }
}

// physical.rs write_all_text_atomic (trait path: match arm `Flush => flush_temp_file(file)` deleted, `flush_temp_file`
// body -> Ok(())): the production flush runs once per durable write on this thread and never for a non-durable one.
#[test]
#[ignore = "native: real disk"]
fn trait_path_flushes_the_temp_file_once_when_durable_and_never_when_not() {
    let root = TempRoot::new("fsflush-count");
    let target = root.join("out.txt");
    let before = temp_file_flushes_on_this_thread();
    PhysicalFileSystem
        .write_all_text_atomic(&target, "fast", false)
        .unwrap();
    assert_eq!(temp_file_flushes_on_this_thread(), before);
    PhysicalFileSystem
        .write_all_text_atomic(&target, "durable", true)
        .unwrap();
    assert_eq!(temp_file_flushes_on_this_thread(), before + 1);
    PhysicalFileSystem
        .write_all_text_atomic(&target, "durable 2", true)
        .unwrap();
    assert_eq!(temp_file_flushes_on_this_thread(), before + 2);
}

// physical.rs open_read_shared_file, `FILE_FLAG_SEQUENTIAL_SCAN` (constant -> 0): the handle is a sequential scan.
#[test]
#[ignore = "native: real disk"]
fn open_read_shared_sets_the_sequential_scan_mode() {
    let root = TempRoot::new("fsflush-seq");
    let path = root.join("a.bin");
    std::fs::write(&path, b"abc").unwrap();
    let file = PhysicalFileSystem.open_read_shared_file(&path).unwrap();
    assert_eq!(mode_of(&file) & FILE_SEQUENTIAL_ONLY, FILE_SEQUENTIAL_ONLY);
    // A plain std open is not sequential: the query really tells the two apart.
    assert_eq!(mode_of(&File::open(&path).unwrap()) & FILE_SEQUENTIAL_ONLY, 0);
}

// open_read_shared still reads the whole file through the wrapper, with and without a buffer.
#[test]
#[ignore = "native: real disk"]
fn open_read_shared_reads_content_with_and_without_buffer() {
    use std::io::Read;
    let root = TempRoot::new("fsflush-read");
    let path = root.join("a.bin");
    std::fs::write(&path, b"hello").unwrap();
    for buffer in [0usize, 1, 2, 4096] {
        let mut text = String::new();
        PhysicalFileSystem
            .open_read_shared(&path, buffer)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "hello", "buffer {buffer}");
    }
}

// physical.rs open_append, `if durable { FILE_FLAG_WRITE_THROUGH } else { 0 }`: the append stream is the real file.
#[test]
#[ignore = "native: real disk"]
fn open_append_durable_is_write_through_and_fast_is_not() {
    let root = TempRoot::new("fsflush-append");
    let path = root.join("j.jsonl");
    for durable in [true, false] {
        let mut stream = PhysicalFileSystem.open_append(&path, durable).unwrap();
        stream.write_all(b"x\r\n").unwrap();
        let mode = mode_of(stream.as_file_mut().expect("real file"));
        assert_eq!(
            mode & FILE_WRITE_THROUGH,
            if durable { FILE_WRITE_THROUGH } else { 0 },
            "durable {durable}"
        );
    }
}
