//! ST-TEST-HYGIENE F1: report (never fail) whether the RAW corpus is present, so CI logs show that the corpus-gated tests of
//! `raw_exif`, `raw_reader_arw`, `readers_cr2`, `readers_rw2`, `cr3_reader`, `w5_18_integration` and `raw_nef` ran or skipped.
//! Each of them prints `SKIPPED (no RAW corpus: <test>)` to stderr when its sample is missing.
//! With the corpus present, set `PHOTOREVIEW_RAW_CORPUS_STRICT=1` (`PHOTOREVIEW_RS_RAW_CORPUS_STRICT=1` for rw2/nef) to make a skip a failure.

use std::io::Write;
use std::path::PathBuf;

const GATED_FILES: [&str; 7] = [
    "raw_exif.rs",
    "raw_reader_arw.rs",
    "readers_cr2.rs",
    "readers_rw2.rs",
    "cr3_reader.rs",
    "w5_18_integration.rs",
    "raw_nef.rs",
];

fn corpus_dir() -> PathBuf {
    std::env::var_os("PHOTOREVIEW_RAW_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("tests")
                .join("fixtures")
                .join("raw-corpus")
        })
}

#[test]
fn raw_corpus_presence_report() {
    let dir = corpus_dir();
    let files = std::fs::read_dir(&dir).map_or(0, |entries| {
        entries.filter_map(Result::ok).filter(|e| e.path().is_file()).count()
    });
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let skip_sites: usize = GATED_FILES
        .iter()
        .filter_map(|name| std::fs::read_to_string(tests_dir.join(name)).ok())
        .map(|text| text.matches("corpus_skip::skipped(").count())
        .sum();
    let strict = ["PHOTOREVIEW_RAW_CORPUS_STRICT", "PHOTOREVIEW_RS_RAW_CORPUS_STRICT"]
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|v| matches!(v.trim(), "1" | "true")));
    let line = if files == 0 {
        format!(
            "RAW CORPUS REPORT: ABSENT at {} -> corpus-gated tests in {} files skip ({skip_sites} skip sites); strict={strict}\n",
            dir.display(),
            GATED_FILES.len()
        )
    } else {
        format!(
            "RAW CORPUS REPORT: present at {} ({files} files); corpus-gated tests run; strict={strict}\n",
            dir.display()
        )
    };
    // Real stderr: visible without `--nocapture`.
    let _ = std::io::stderr().write_all(line.as_bytes());
}
