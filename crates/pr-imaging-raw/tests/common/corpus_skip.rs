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

/// What a corpus test does at its gate.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CorpusGate {
    /// The corpus (or sample) is there: run the test body.
    Run,
    /// Absent and not strict: print `SKIPPED` and return.
    Skip,
    /// Absent and strict: fail the test.
    Fail,
}

/// `PHOTOREVIEW_RAW_CORPUS_STRICT` value is `1` or `true` (trimmed, case-insensitive), as in `cr3_reader.rs`.
pub fn strict_value(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        let v = v.trim();
        v == "1" || v.eq_ignore_ascii_case("true")
    })
}

/// Pure decision over the env value and corpus presence (no process-environment access, so it is testable in parallel).
pub fn gate(strict_env: Option<&str>, corpus_present: bool) -> CorpusGate {
    match (corpus_present, strict_value(strict_env)) {
        (true, _) => CorpusGate::Run,
        (false, false) => CorpusGate::Skip,
        (false, true) => CorpusGate::Fail,
    }
}

/// Absent-corpus gate for a test that returns early: strict mode (`PHOTOREVIEW_RAW_CORPUS_STRICT`) panics, otherwise prints
/// the `SKIPPED` line via [`skipped`].
pub fn skip_or_strict_fail(why: &str) {
    let env = std::env::var("PHOTOREVIEW_RAW_CORPUS_STRICT").ok();
    match gate(env.as_deref(), false) {
        CorpusGate::Fail => panic!("PHOTOREVIEW_RAW_CORPUS_STRICT is set but the RAW corpus is missing: {why}"),
        _ => skipped(why),
    }
}
