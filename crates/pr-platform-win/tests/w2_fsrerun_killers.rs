//! cl-fsrerun: kills the share-mode mutants of `PhysicalFileSystem::open_read_shared_file` that the flush killers
//! (`w2_fs_flush_killers`) do not cover (`FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`, `|` -> `&`).
//! INV-8: while a stream from `open_read_shared` is open, another handle can read, write, rename and delete the file.
//! Real disk in a private `TempRoot`; run with `-- --include-ignored --test-threads=1`.
#![cfg(windows)]

use std::fs::OpenOptions;
use std::io::{Read, Write};

use pr_core::io::physical::PhysicalFileSystem;
use pr_test_support::TempRoot;

#[test]
#[ignore = "native: real disk"]
fn open_read_shared_handle_allows_read_write_rename_and_delete_by_others() {
    let root = TempRoot::new("fsrerun-share");
    let path = root.join("a.txt");
    std::fs::write(&path, "hello").unwrap();

    let mut held = PhysicalFileSystem.open_read_shared_file(&path).unwrap();

    // FILE_SHARE_READ: another reader.
    let mut other = OpenOptions::new()
        .read(true)
        .open(&path)
        .expect("second reader must be allowed");
    let mut text = String::new();
    other.read_to_string(&mut text).unwrap();
    assert_eq!(text, "hello");
    drop(other);

    // FILE_SHARE_WRITE: another writer.
    let mut writer = OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("writer must be allowed (FILE_SHARE_WRITE)");
    writer.write_all(b"J").unwrap();
    drop(writer);

    // FILE_SHARE_DELETE: rename, then delete, while the stream stays open and readable.
    let renamed = root.join("b.txt");
    std::fs::rename(&path, &renamed).expect("rename must be allowed (FILE_SHARE_DELETE)");
    std::fs::remove_file(&renamed).expect("delete must be allowed (FILE_SHARE_DELETE)");

    let mut rest = String::new();
    held.read_to_string(&mut rest).unwrap();
    assert_eq!(rest.len(), 5);
}
