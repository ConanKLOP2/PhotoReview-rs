//! G18 (docs/reviews/pr-164.md L2): the 300 UTF-16 unit cut of `error_log::format` with real surrogate pairs at the boundary, and the exact
//! boundary for both the message and the inner message. The older `startup_error_log.rs` test uses two ASCII `?` and cannot reach the
//! `from_utf16_lossy` branch. C# cuts with `Substring(0, 300)` and writes UTF-8 (the encoder turns a lone high surrogate into U+FFFD), so the bytes agree.
use pr_app::startup::error_log::{self, ErrorDetails, InnerErrorDetails};
use pr_core::time::DateTimeUtc;

const EMOJI: &str = "\u{1F600}";

fn when() -> DateTimeUtc {
    DateTimeUtc::parse_json("2026-05-06T07:08:09.123Z").unwrap().0
}

fn first_line(details: &ErrorDetails) -> String {
    let text = error_log::format(when(), "ctx", details);
    text.split("\r\n").next().unwrap().to_owned()
}

fn message_of(line: &str) -> &str {
    line.split_once(": ").expect("header has ': '").1
}

#[test]
fn a_pair_split_at_unit_300_becomes_one_replacement_character() {
    let d = ErrorDetails::new("Error", 0, format!("{}{EMOJI}tail", "z".repeat(299)));
    let line = first_line(&d);
    assert_eq!(message_of(&line), format!("{}\u{FFFD}", "z".repeat(299)));
    assert!(!line.contains("tail") && !line.contains(EMOJI));
}

#[test]
fn a_pair_ending_exactly_at_unit_300_is_kept_whole() {
    let d = ErrorDetails::new("Error", 0, format!("{}{EMOJI}tail", "z".repeat(298)));
    let line = first_line(&d);
    assert_eq!(message_of(&line), format!("{}{EMOJI}", "z".repeat(298)));
}

#[test]
fn a_message_of_exactly_300_units_is_not_cut_and_301_loses_one_unit() {
    let exact = first_line(&ErrorDetails::new("Error", 0, "a".repeat(300)));
    assert_eq!(message_of(&exact), "a".repeat(300));
    let over = first_line(&ErrorDetails::new("Error", 0, format!("{}b", "a".repeat(300))));
    assert_eq!(message_of(&over), "a".repeat(300));
    let short = first_line(&ErrorDetails::new("Error", 0, format!("{}b", "a".repeat(299))));
    assert_eq!(message_of(&short), format!("{}b", "a".repeat(299)));
}

#[test]
fn the_inner_message_is_cut_at_300_units_with_the_same_pair_rule() {
    let mut d = ErrorDetails::new("Error", 0, "outer");
    d.inner = Some(InnerErrorDetails {
        type_name: "Inner".into(),
        message: format!("{}{EMOJI}tail", "q".repeat(299)),
    });
    let text = error_log::format(when(), "ctx", &d);
    let inner = text
        .split("\r\n")
        .find(|l| l.starts_with("  inner: "))
        .expect("inner line");
    assert_eq!(inner, format!("  inner: Inner: {}\u{FFFD}", "q".repeat(299)));
}

#[test]
fn line_breaks_are_replaced_before_the_cut_so_crlf_counts_as_one_unit() {
    // "\r\n" becomes one space, so 299 'k' + the space are exactly 300 units and "rest" is cut.
    let d = ErrorDetails::new("Error", 0, format!("{}\r\nrest", "k".repeat(299)));
    let line = first_line(&d);
    assert_eq!(message_of(&line), format!("{} ", "k".repeat(299)));
}
