//! Mutation killers for `JournalDateTime::eq` (survivor of the JOURNAL-LINE-MUTATION-2026-11-03 run).
//! C# `DateTime ==` compares the ticks only: equal ticks are equal whatever the kind, different ticks are never equal.

use pr_core::file_actions::journal_line::JournalDateTime;

fn dt(text: &str) -> JournalDateTime {
    JournalDateTime::parse_json(text).expect("text parses")
}

#[test]
fn different_ticks_are_not_equal() {
    assert_ne!(dt("2026-09-29T00:00:00Z"), dt("2026-09-29T00:00:01Z"));
    assert_ne!(dt("2026-09-29T00:00:00.0000001Z"), dt("2026-09-29T00:00:00Z"));
}

#[test]
fn same_ticks_are_equal_regardless_of_kind() {
    assert_eq!(dt("2026-09-29T00:00:00Z"), dt("2026-09-29T00:00:00"));
    assert_eq!(dt("2026-09-29T00:00:00Z"), dt("2026-09-29T00:00:00Z"));
}

// ---------------------------------------------------------------------------------------------------------------
// `nests_too_deep` survivors (lines 315, 320, 327): depth is counted outside strings only, and closers lower it.
// ---------------------------------------------------------------------------------------------------------------

use pr_core::file_actions::journal_line::{try_parse, try_parse_utf8};

fn line_with_extra(extra: &str, error: &str) -> String {
    format!(
        "{{\"Id\":\"id1\",\"Type\":\"Move\",\"State\":\"Prepared\",\"Source\":\"a.jpg\",\"Destination\":null,\"Size\":1,\"LastWriteUtc\":\"2026-01-01T00:00:00Z\",\"TimestampUtc\":\"2026-01-01T00:00:00Z\",\"Error\":\"{error}\",\"Extra\":{extra}}}"
    )
}

#[test]
fn brackets_inside_a_string_value_do_not_count_as_nesting() {
    let brackets = "[".repeat(100) + &"{".repeat(100);
    let line = line_with_extra("[]", &brackets);
    assert!(try_parse(&line).is_some(), "100+100 brackets inside a string");
    assert!(try_parse_utf8(line.as_bytes()).is_some());
}

#[test]
fn closed_containers_lower_the_depth_again() {
    // 100 sibling arrays at depth 2: never deeper than 2, so accepted.
    let extra = format!("[{}]", vec!["[]"; 100].join(","));
    let line = line_with_extra(&extra, "e");
    assert!(try_parse(&line).is_some(), "100 sibling arrays accepted");
    assert!(try_parse_utf8(line.as_bytes()).is_some());
    // And the same with objects as siblings.
    let extra = format!("[{}]", vec!["{}"; 100].join(","));
    let line = line_with_extra(&extra, "e");
    assert!(try_parse(&line).is_some(), "100 sibling objects accepted");
}
