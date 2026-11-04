//! N1 run-proofs (mutation close-out 2026-11-05) for the five `journal_line.rs` survivors that M5 classified as equivalent
//! by reading only. Each section is an exhaustive run over a finite input domain and is meant to PASS with the original
//! code and with the hand-applied mutant (the N1 evidence file lists the commands):
//!
//! * `380:31` guard `!raw.bytes().any(..)` -> `true` in `integer_token`: every token over an 8-symbol alphabet up to a
//!   fixed length is put into `Size` (top level up to length 6, group member up to length 4) and compared with an
//!   independent oracle (JSON integer grammar + `i64` range). The tokens with `.`, `e`, `E` are all in the domain.
//! * `596:9`, `619:9`, `647:9` `expecting` -> `Ok(Default::default())`: the text only reaches serde's type-mismatch errors,
//!   which `try_parse` maps to `None` without looking at them. Every JSON value type (6) at every position that has a
//!   visitor (the entry, `GroupMembers`, one member slot) is run and the outcome compared with the expected one.
//! * `623:9` `GroupMembersVisitor::visit_unit` -> `Ok(Default::default())`: `Option<Vec<_>>::default()` is `None`, which is
//!   what the original returns; run for `null` and compared with an absent `GroupMembers`.

use pr_core::file_actions::journal_line::{JournalEntry, try_parse};

const SAMPLE_HEAD: &str =
    r#"{"Id":"op","Type":"Move","State":"Prepared","Source":"C:\\photos\\a.jpg","Destination":"C:\\sel\\a.jpg","#;
const SAMPLE_TAIL: &str = r#""LastWriteUtc":"2026-09-25T00:00:00Z","TimestampUtc":"2026-09-25T00:00:00Z""#;
const MEMBER_TAIL: &str = r#""LastWriteUtc":"2026-09-25T00:00:00Z","Permanent":false"#;

fn line_with_size(token: &str) -> String {
    format!("{SAMPLE_HEAD}\"Size\":{token},{SAMPLE_TAIL}}}")
}

fn line_with_group(members_json: &str) -> String {
    format!("{SAMPLE_HEAD}\"Size\":5,{SAMPLE_TAIL},\"GroupId\":\"g\",\"GroupMembers\":{members_json}}}")
}

fn line_with_member_size(token: &str) -> String {
    line_with_group(&format!(
        "[{{\"Source\":\"C:\\\\a.jpg\",\"Size\":{token},{MEMBER_TAIL}}}]"
    ))
}

/// Independent oracle: the JSON integer grammar `-?(0|[1-9][0-9]*)` without fraction or exponent, in the `i64` range,
/// `-0` is 0. Written without `parse` on the token, so it does not share the code under test's shortcut.
fn oracle(token: &str) -> Option<i64> {
    let (negative, digits) = match token.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, token),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    let mut value: i128 = 0;
    for b in digits.bytes() {
        value = value * 10 + i128::from(b - b'0');
        if value > i128::from(i64::MAX) + 1 {
            return None;
        }
    }
    let signed = if negative { -value } else { value };
    i64::try_from(signed).ok()
}

fn rec(alphabet: &[u8], max_len: usize, current: &mut Vec<u8>, count: &mut usize, visit: &mut dyn FnMut(&str)) {
    if !current.is_empty() {
        visit(std::str::from_utf8(current).expect("ascii"));
        *count += 1;
    }
    if current.len() == max_len {
        return;
    }
    for &symbol in alphabet {
        current.push(symbol);
        rec(alphabet, max_len, current, count, visit);
        current.pop();
    }
}

/// Calls `visit` with every string of length 1..=`max_len` over `alphabet`; returns how many there were.
fn for_every_token(alphabet: &[u8], max_len: usize, mut visit: impl FnMut(&str)) -> usize {
    let mut count = 0;
    rec(alphabet, max_len, &mut Vec::new(), &mut count, &mut visit);
    count
}

#[test]
fn the_sample_line_parses_with_the_expected_size() {
    let entry = try_parse(&line_with_size("5")).expect("sample parses");
    assert_eq!(entry.size, 5);
    let entry = try_parse(&line_with_member_size("7")).expect("group sample parses");
    assert_eq!(entry.group_members.expect("members")[0].size, 7);
}

#[test]
fn every_short_size_token_over_digits_signs_dot_and_exponent_matches_the_integer_oracle() {
    // 8^1 + .. + 8^6 = 299_592 tokens; all with '.', 'e' or 'E' are in here (the mutated guard's whole input class).
    let mut with_marker = 0usize;
    let mut accepted = 0usize;
    let total = for_every_token(b"019-+.eE", 6, |token| {
        let expected = oracle(token);
        let actual = try_parse(&line_with_size(token)).map(|entry: JournalEntry| entry.size);
        assert_eq!(actual, expected, "Size token {token:?}");
        if token.bytes().any(|b| matches!(b, b'.' | b'e' | b'E')) {
            with_marker += 1;
            assert_eq!(actual, None, "a token with '.', 'e' or 'E' must be rejected: {token:?}");
        }
        if actual.is_some() {
            accepted += 1;
        }
    });
    assert_eq!(total, 299_592);
    assert!(
        with_marker > 150_000,
        "the dot/exponent class is covered: {with_marker}"
    );
    assert!(
        accepted > 100,
        "accepted tokens exist, so the oracle is not vacuous: {accepted}"
    );
}

#[test]
fn every_short_member_size_token_matches_the_integer_oracle() {
    let total = for_every_token(b"019-+.eE", 4, |token| {
        let expected = oracle(token);
        let actual =
            try_parse(&line_with_member_size(token)).map(|entry| entry.group_members.expect("members")[0].size);
        assert_eq!(actual, expected, "member Size token {token:?}");
    });
    assert_eq!(total, 4_680);
}

#[test]
fn size_tokens_at_the_i64_edges_and_with_realistic_exponents_match_the_oracle() {
    for token in [
        "9223372036854775807",
        "9223372036854775808",
        "-9223372036854775808",
        "-9223372036854775809",
        "-0",
        "0",
        "00",
        "1.0",
        "1.5",
        "1e3",
        "1E3",
        "1e+3",
        "-1.5e-3",
        "0.0",
        "5.",
        ".5",
    ] {
        assert_eq!(
            try_parse(&line_with_size(token)).map(|entry| entry.size),
            oracle(token),
            "Size token {token:?}"
        );
    }
}

/// One JSON value of every type: null, boolean, number, string, array, object.
const JSON_TYPES: [(&str, &str); 6] = [
    ("null", "null"),
    ("boolean", "true"),
    ("number", "12"),
    ("string", "\"text\""),
    ("array", "[]"),
    ("object", "{}"),
];

#[test]
fn entry_visitor_every_json_type_at_the_top_level_is_rejected() {
    for (name, value) in JSON_TYPES {
        assert!(try_parse(value).is_none(), "top-level {name} {value}");
    }
    assert!(try_parse(&line_with_size("5")).is_some(), "a valid object is accepted");
}

#[test]
fn group_members_visitor_every_json_type_gives_the_documented_outcome() {
    for (name, value) in JSON_TYPES {
        let parsed = try_parse(&line_with_group(value));
        match name {
            // null: `visit_unit` -> no members.
            "null" => {
                let entry = parsed.expect("null GroupMembers is accepted");
                assert!(entry.group_members.is_none());
            }
            "array" => assert_eq!(parsed.expect("empty array").group_members, Some(Vec::new())),
            _ => assert!(parsed.is_none(), "GroupMembers as {name} must be rejected"),
        }
    }
    let one = format!("[{{\"Source\":\"C:\\\\a.jpg\",\"Size\":1,{MEMBER_TAIL}}}]");
    assert_eq!(
        try_parse(&line_with_group(&one))
            .expect("one member")
            .group_members
            .expect("members")
            .len(),
        1
    );
}

#[test]
fn group_members_null_gives_the_same_entry_as_the_default_option() {
    // `visit_unit` must produce `None`: `Option<Vec<_>>::default()`, the value the mutant returns, is also `None`.
    assert!(Option::<Vec<u8>>::default().is_none());
    let with_null = try_parse(&line_with_group("null")).expect("null accepted");
    assert!(with_null.group_members.is_none());
}

#[test]
fn member_slot_visitor_every_json_type_gives_the_documented_outcome() {
    let valid = format!("{{\"Source\":\"C:\\\\a.jpg\",\"Size\":1,{MEMBER_TAIL}}}");
    for (name, value) in JSON_TYPES {
        let parsed = try_parse(&line_with_group(&format!("[{valid},{value}]")));
        let members = parsed.map(|entry| entry.group_members.map(|members| members.len()));
        // Every slot type is rejected with the whole line: `null` is `MemberSlot::Null` (`visit_unit`, which finishes to
        // `None`), `{}` has no usable Source, boolean, number, string and array are type errors.
        assert_eq!(members, None, "slot as {name}");
    }
}
