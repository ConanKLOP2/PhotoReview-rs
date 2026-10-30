//! Killers of lane cl-sec01 (SEC-01 / F-05 audit, 2026-10-30): the `MAX_FOLLOWED_LINKS` cap of the manual link walk
//! (`GetFinalLinkTargetSlow`) in `PhysicalFileSystem`. No disk is touched: the walk runs over an in-memory link table
//! through the `follow_link_chain_for_tests` seam, because the OS refuses a chain of more than 63 reparse points (1921)
//! before the manual walk is reached, so no junction chain can exercise the cap.
#![cfg(windows)]

use std::collections::HashMap;
use std::io;

use pr_core::io::physical::follow_link_chain_for_tests;

const ERROR_CANT_RESOLVE_FILENAME: i32 = 1921;

/// `p0 -> p1 -> ... -> p<links>`; `p<links>` is not a link. Returns the table and the head.
fn chain(links: usize) -> (HashMap<String, String>, String) {
    let table = (0..links).map(|i| (format!("p{i}"), format!("p{}", i + 1))).collect();
    (table, "p0".to_owned())
}

fn walk(table: &HashMap<String, String>, head: &str) -> (io::Result<String>, usize) {
    let mut steps = 0usize;
    let result = follow_link_chain_for_tests(head, &mut |path| {
        steps += 1;
        table.get(path).cloned()
    });
    (result, steps)
}

// physical.rs:995 `hops += 1` -> `-= 1` (usize underflow panic) / `*= 1` (the counter never rises), :996 `>` -> `>=`
// (63 hops would fail) and `>` -> `<` (the first hop would fail). C#: GetFinalLinkTargetSlow follows at most
// MaxFollowedLinks (63) hops: a 63-link chain still resolves to its last target, which does not need to exist.
#[test]
fn resolve_chain_of_exactly_63_links_resolves_to_its_last_target() {
    let (table, head) = chain(63);

    let (result, steps) = walk(&table, &head);

    assert_eq!(result.unwrap(), "p63");
    assert_eq!(
        steps, 64,
        "one step per link plus the step that finds the end of the chain"
    );
}

// physical.rs:995 `hops += 1` -> `*= 1` and :996 `>` -> `>=`/`<`. One link more is "too many levels of links"
// (IOException, win32 1921); the walk stops right there instead of following the rest.
#[test]
fn resolve_chain_of_64_links_fails_with_1921_after_the_64th_hop() {
    let (table, head) = chain(64);

    let (result, steps) = walk(&table, &head);

    assert_eq!(result.unwrap_err().raw_os_error(), Some(ERROR_CANT_RESOLVE_FILENAME));
    assert_eq!(
        steps, 64,
        "the cap stops the walk at the 64th target without asking for the next one"
    );
}

// physical.rs:995 `hops += 1` -> `*= 1`: with a counter that never rises a cycle would never end (a hang that
// cargo-mutants reports as a timeout); here the cycle must fail with 1921 after a bounded number of steps.
#[test]
fn resolve_link_cycle_fails_with_1921_after_a_bounded_walk() {
    let table: HashMap<String, String> = [("a", "b"), ("b", "c"), ("c", "a")]
        .into_iter()
        .map(|(from, to)| (from.to_owned(), to.to_owned()))
        .collect();
    let mut steps = 0usize;

    let result = follow_link_chain_for_tests("a", &mut |path| {
        steps += 1;
        assert!(steps <= 1000, "the walk must stop by itself");
        table.get(path).cloned()
    });

    assert_eq!(result.unwrap_err().raw_os_error(), Some(ERROR_CANT_RESOLVE_FILENAME));
    assert_eq!(steps, 64);
}

// physical.rs:991 (`final_link_target_slow` -> Ok(..)): a path that is not a link has no target, the walk reports
// NotFound, and a chain returns its last target, not the first.
#[test]
fn resolve_path_that_is_not_a_link_has_no_final_target_and_a_short_chain_returns_its_last() {
    let (table, _) = chain(0);
    let (result, steps) = walk(&table, "plain");
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    assert_eq!(steps, 1);

    let (table, head) = chain(1);
    assert_eq!(walk(&table, &head).0.unwrap(), "p1");
    let (table, head) = chain(2);
    assert_eq!(walk(&table, &head).0.unwrap(), "p2");
}
