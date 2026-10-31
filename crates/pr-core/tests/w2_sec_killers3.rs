//! Fail-closed killers of the SEC-01 manual link walk (`GetFinalLinkTargetSlow`) in `PhysicalFileSystem`, lane
//! st-http-sec-killers (2026-10-31). No disk is touched: the walk runs over an in-memory link table through the
//! `follow_link_chain_for_tests` seam. The resolver must never answer `Ok` with a path it did not prove: an empty
//! chain, a cycle or an over-long chain is an error (the caller then refuses the destination).
//!
//! Not reachable through this seam: the guard `is_path_unreachable(&err)` of `final_link_target` (physical.rs, the
//! `fs::canonicalize` branch) runs before the walk and needs an OS error that is neither a loop nor unreachable.
#![cfg(windows)]

use std::collections::HashMap;
use std::io;

use pr_core::io::physical::follow_link_chain_for_tests;

const ERROR_CANT_RESOLVE_FILENAME: i32 = 1921;

fn table(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
}

fn walk(table: &HashMap<String, String>, head: &str) -> (io::Result<String>, usize) {
    let mut steps = 0usize;
    let result = follow_link_chain_for_tests(head, &mut |path| {
        steps += 1;
        assert!(steps <= 1000, "the walk must stop by itself");
        table.get(path).cloned()
    });
    (result, steps)
}

// A link that points at itself is the shortest cycle: fail closed with 1921, never `Ok("a")`.
#[test]
fn resolve_self_link_fails_closed_with_1921() {
    let (result, steps) = walk(&table(&[("a", "a")]), "a");

    assert_eq!(result.unwrap_err().raw_os_error(), Some(ERROR_CANT_RESOLVE_FILENAME));
    assert_eq!(steps, 64);
}

// A two-link loop a -> b -> a also fails with 1921 (kills a walk that stops on the first repeated path with Ok).
#[test]
fn resolve_two_link_loop_fails_closed_with_1921() {
    let (result, _) = walk(&table(&[("a", "b"), ("b", "a")]), "a");

    assert_eq!(result.unwrap_err().raw_os_error(), Some(ERROR_CANT_RESOLVE_FILENAME));
}

// A chain that enters a loop after a tail (x -> a -> b -> a ...) never reaches a non-link: error, not the last seen.
#[test]
fn resolve_chain_that_enters_a_loop_fails_closed_instead_of_returning_the_last_target() {
    let (result, _) = walk(&table(&[("x", "a"), ("a", "b"), ("b", "a")]), "x");

    assert_eq!(result.unwrap_err().raw_os_error(), Some(ERROR_CANT_RESOLVE_FILENAME));
}

// The error of a failed walk is the 1921 OS error, not a NotFound that a caller could treat as "does not exist yet".
#[test]
fn resolve_cap_error_is_not_a_not_found() {
    let (result, _) = walk(&table(&[("a", "a")]), "a");

    assert_ne!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
}

// A head that is not a link is NotFound (the C# "cannot resolve") and the step is asked exactly once.
#[test]
fn resolve_non_link_head_is_not_found_after_one_step() {
    let (result, steps) = walk(&table(&[]), "plain");

    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    assert_eq!(steps, 1);
}

// A dangling last link (target does not exist, so its step is None) resolves to that target, even though it is missing.
#[test]
fn resolve_dangling_chain_returns_the_missing_last_target() {
    let (result, steps) = walk(&table(&[("a", "b"), ("b", "missing")]), "a");

    assert_eq!(result.unwrap(), "missing");
    assert_eq!(steps, 3);
}
