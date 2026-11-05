//! MUT3 (2026-11-06), task P2: run support for the `presenters/benchmark.rs` survivors N4 left open (390:51, 832:14, 833:59, 833:17).
//! Hand-written, no golden file, no sleep. None of these mutants can be closed by a run (see the doc comments): the tests pin the
//! behaviour that stays equal under them, so a later seam can turn them into closures.

use pr_app::presenters::benchmark::{BenchmarkPresenter, BenchmarkScanError, enumerate_image_files};
use pr_core::sync::CancelToken;
use pr_test_support::TempRoot;

/// benchmark.rs:390:51 `files.len() > limit` -> `>=`. The branches differ only when `files.len() == limit`, where `files[..limit]` IS
/// the whole slice, so the copy is the same list either way. Run for every length `1..=3000` with the limit EQUAL to the length,
/// and for the neighbours `limit = len - 1` and `len + 1`, on distinct strings (a swap or a drop would show).
/// RUN-SUPPORTED, not a closure: the domain is every length up to `i32::MAX`; only a bounded part can be allocated and run.
#[test]
fn a_limit_equal_to_the_list_length_returns_the_whole_list() {
    for len in 1..=3000usize {
        let files: Vec<String> = (0..len).map(|i| format!("folder/img-{i:05}.jpg")).collect();
        let limit = i32::try_from(len).expect("small");
        assert_eq!(
            BenchmarkPresenter::apply_image_limit(&files, limit),
            files,
            "limit == len == {len}"
        );
        assert_eq!(
            BenchmarkPresenter::apply_image_limit(&files, limit + 1),
            files,
            "limit == len + 1, len {len}"
        );
        assert_eq!(
            BenchmarkPresenter::apply_image_limit(&files, limit - 1),
            if len == 1 {
                files.clone()
            } else {
                files[..len - 1].to_vec()
            },
            "limit == len - 1 (a zero limit means no limit), len {len}"
        );
    }
}

/// benchmark.rs:832:14 `seen += 1` -> `*= 1`, 833:59 `== 0` -> `!= 0`, 833:17 `%` -> `/` and `%` -> `+` change how often the token is
/// polled inside the loop and nothing else: the poll is a read of an atomic flag. Once canceled a token stays canceled
/// (`CancelToken` has no reset), so an in-loop `Canceled` implies the poll after the loop answers `Canceled` too: the result is the
/// same whatever the polling pattern. The one thing that would tell the patterns apart is an I/O error from a directory entry
/// AFTER the token was canceled (the original keeps reading, a mutant that polled earlier returns `Canceled` first); the standard
/// library gives no way to provoke such an entry error from a test, so this stays OPEN. This test pins what is observable:
/// a token canceled before the call, a token never canceled, and the stickiness of the flag.
#[test]
fn a_canceled_token_stays_canceled_and_decides_the_result_of_a_scan() {
    let token = CancelToken::new();
    assert!(token.check().is_ok());
    token.cancel();
    for _ in 0..1000 {
        assert!(token.check().is_err(), "a canceled token must stay canceled");
    }
    let root = TempRoot::new("mut3-enumerate");
    let folder = root.join("photos");
    std::fs::create_dir(&folder).expect("folder");
    for i in 0..600 {
        std::fs::write(folder.join(format!("p{i}.jpg")), b"x").expect("file");
    }
    let text = folder.to_string_lossy().into_owned();
    assert_eq!(
        enumerate_image_files(&text, 0, &token),
        Err(BenchmarkScanError::Canceled)
    );
    let listed = enumerate_image_files(&text, 0, &CancelToken::new()).expect("not canceled");
    assert_eq!(listed.len(), 600);
}
