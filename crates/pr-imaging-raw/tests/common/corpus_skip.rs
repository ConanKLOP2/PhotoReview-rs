//! Shared by the RAW corpus tests (`#[path = "common/corpus_skip.rs"] mod corpus_skip;`): a skipped corpus test must not look
//! like a plain pass. The line goes to the real stderr (not the libtest output capture), so it shows without `--nocapture`.
//! CI has no `tests/fixtures/raw-corpus`; with the corpus present, set `PHOTOREVIEW_RAW_CORPUS_STRICT=1` (or
//! `PHOTOREVIEW_RS_RAW_CORPUS_STRICT=1` where a file uses that name) to turn every skip into a failure.
#![allow(dead_code)]

use std::io::Write;

/// Prints `SKIPPED (no RAW corpus: <test>) <why>`; `<test>` is the libtest thread name (`?` when run on the main thread).
pub fn skipped(why: &str) {
    let thread = std::thread::current();
    let test = thread.name().unwrap_or("?");
    let line = format!("SKIPPED (no RAW corpus: {test}) {why}\n");
    let _ = std::io::stderr().write_all(line.as_bytes());
}
