//! Lead seams of Wave 3 on `PhysicalFileSystem` (docs/PROPOSED-DECISIONS.md P-11, P-12; contracts/README.md D26).
//!
//! Every test touches the real disk in a private `TempRoot` and is `#[ignore = "native: real disk"]`; run with
//! `cargo test -p pr-core --test io_physical_seams -- --ignored` on Windows.
#![cfg(windows)]

use std::io::Write;

use pr_core::abstractions::FileSystem;
use pr_core::io::physical::PhysicalFileSystem;
use pr_test_support::TempRoot;

// P-11 (W3-01 open question 1): the physical append stream IS the real file (C# `stream is FileStream fs`), so the
// PowerLossSafe journal can flush it to disk; flushing through it must persist what was written.
#[test]
#[ignore = "native: real disk"]
fn open_append_physical_stream_exposes_the_real_file() {
    let root = TempRoot::new("seam-append");
    let path = root.join("journal.jsonl");
    for durable in [false, true] {
        let mut stream = PhysicalFileSystem.open_append(&path, durable).unwrap();
        stream.write_all(b"line\r\n").unwrap();
        let file = stream.as_file_mut().expect("a physical append stream is a real file");
        file.sync_all().unwrap();
        assert_eq!(file.metadata().unwrap().len(), if durable { 12 } else { 6 });
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"line\r\nline\r\n");
}
