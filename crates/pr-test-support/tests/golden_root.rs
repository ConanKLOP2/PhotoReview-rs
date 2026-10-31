//! `golden::repo_root()` must find the repo-root `tests/golden` from the RUNTIME manifest dir, because under cargo-mutants this crate may be
//! reused from a previous temp copy whose compile-time `CARGO_MANIFEST_DIR` no longer exists (CL-MUT-GOLDEN-INFRA).

use pr_test_support::golden;

#[test]
fn repo_root_holds_tests_golden() {
    let root = golden::repo_root();
    assert!(
        root.join("tests").join("golden").is_dir(),
        "{} has no tests/golden",
        root.display()
    );
}

#[test]
fn load_reads_a_golden_file_through_the_resolved_root() {
    let g = golden::load("lru-trace");
    assert!(!g.cases.is_empty());
}
