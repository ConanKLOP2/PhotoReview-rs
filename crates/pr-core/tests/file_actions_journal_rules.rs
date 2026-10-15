//! Port of the C# tail-reader line predicate tests (C# baseline d92d6aae, `tests/PhotoReview.Core.Tests/FileActions/`):
//! `JournalTailCommittedMoveTests` (the predicate must accept exactly what the full reader accepts),
//! `JournalGroupSchemaTests.CommittedGroupMove_TailReaderKeepsEveryMemberOnTheReturnedEntry`, and the golden check against
//! `tests/golden/journal-tail-committed-move.json` (24 cases).
//!
//! Port of `PhotoReview.Core/FileActions/OperationJournal.cs` lines 115, 384-402 and 429-438 (`file_actions::journal_rules`,
//! complete in the stub: these tests pass against it and guard it against regressions).

use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember, try_parse};
use pr_core::file_actions::journal_rules::try_parse_tail_committed_move;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_test_support::golden::{self, bool_of, str_of};
use serde_json::Value;

const STAMP_TEXT: &str = "2026-09-29T00:00:00Z";

fn stamp() -> JournalDateTime {
    let (value, _) = DateTimeUtc::parse_json(STAMP_TEXT).expect("stamp text parses");
    JournalDateTime::utc(value)
}

/// C# `JournalGroupMember(source, destination, size, Stamp)`.
fn member(source: &str, destination: &str, size: i64) -> JournalGroupMember {
    JournalGroupMember {
        source: source.to_owned(),
        destination: Some(destination.to_owned()),
        size,
        last_write_utc: stamp(),
        permanent: false,
    }
}

/// C# `CommittedMove(params JournalGroupMember[] members)`: Id `m`, group id `g` only when members are given.
fn committed_move(members: Vec<JournalGroupMember>) -> JournalEntry {
    let group = !members.is_empty();
    JournalEntry {
        id: "m".to_owned(),
        op_type: FileOperationType::Move,
        state: JournalState::Committed,
        source: r"C:\a.jpg".to_owned(),
        destination: Some(r"C:\s\a.jpg".to_owned()),
        size: 1,
        last_write_utc: stamp(),
        timestamp_utc: stamp(),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: group.then(|| "g".to_owned()),
        group_members: group.then_some(members),
    }
}

// C#: JournalTailCommittedMoveTests.TryParseTailCommittedMove_ValidCommittedGroupMove_IsReturned
#[test]
fn try_parse_tail_committed_move_valid_committed_group_move_is_returned() {
    let entry = committed_move(vec![member(r"C:\a.jpg", r"C:\s\a.jpg", 1)]);
    let line = entry.to_line();

    let parsed = try_parse_tail_committed_move(line.as_bytes()).expect("a committed Move is returned");

    assert_eq!(parsed.group_members.as_ref().map(Vec::len), Some(1));
}

/// C# `TryParseTailCommittedMove_LineTheFullReaderRejects_IsQuarantinedHereToo`: the reference rule first, then the tail
/// predicate on the same bytes.
fn assert_quarantined_here_too(json: &str) {
    assert!(
        try_parse(json).is_none(),
        "the full reader must reject this line (reference rule)"
    );
    assert!(
        try_parse_tail_committed_move(json.as_bytes()).is_none(),
        "the tail predicate must reject it too"
    );
}

// C#: JournalTailCommittedMoveTests.TryParseTailCommittedMove_LineTheFullReaderRejects_IsQuarantinedHereToo (InlineData 1)
#[test]
fn try_parse_tail_committed_move_line_the_full_reader_rejects_empty_member_source_is_quarantined_here_too() {
    assert_quarantined_here_too(
        r#"{"Id":"m","Type":"Move","State":"Committed","Source":"C:\\a.jpg","Destination":"C:\\s\\a.jpg","Size":1,"LastWriteUtc":"2026-09-29T00:00:00Z","TimestampUtc":"2026-09-29T00:00:00Z","GroupId":"g","GroupMembers":[{"Source":"","Destination":"C:\\s\\a.jpg","Size":1,"LastWriteUtc":"2026-09-29T00:00:00Z"}]}"#,
    );
}

// C#: JournalTailCommittedMoveTests.TryParseTailCommittedMove_LineTheFullReaderRejects_IsQuarantinedHereToo (InlineData 2)
#[test]
fn try_parse_tail_committed_move_line_the_full_reader_rejects_null_member_is_quarantined_here_too() {
    assert_quarantined_here_too(
        r#"{"Id":"m","Type":"Move","State":"Committed","Source":"C:\\a.jpg","Destination":"C:\\s\\a.jpg","Size":1,"LastWriteUtc":"2026-09-29T00:00:00Z","TimestampUtc":"2026-09-29T00:00:00Z","GroupId":"g","GroupMembers":[null]}"#,
    );
}

// C#: JournalTailCommittedMoveTests.TryParseTailCommittedMove_LineTheFullReaderRejects_IsQuarantinedHereToo (InlineData 3)
#[test]
fn try_parse_tail_committed_move_line_the_full_reader_rejects_empty_id_is_quarantined_here_too() {
    assert_quarantined_here_too(
        r#"{"Id":"","Type":"Move","State":"Committed","Source":"C:\\a.jpg","Destination":"C:\\s\\a.jpg","Size":1,"LastWriteUtc":"2026-09-29T00:00:00Z","TimestampUtc":"2026-09-29T00:00:00Z"}"#,
    );
}

// C#: JournalGroupSchemaTests.CommittedGroupMove_TailReaderKeepsEveryMemberOnTheReturnedEntry
#[test]
fn committed_group_move_tail_reader_keeps_every_member_on_the_returned_entry() {
    let entry = JournalEntry {
        id: "group".to_owned(),
        op_type: FileOperationType::Move,
        state: JournalState::Committed,
        source: r"C:\photos\a.jpg".to_owned(),
        destination: Some(r"C:\photos\selected\a.jpg".to_owned()),
        size: 12,
        last_write_utc: stamp(),
        timestamp_utc: stamp(),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: Some("capture-a".to_owned()),
        group_members: Some(vec![
            member(r"C:\photos\a.jpg", r"C:\photos\selected\a.jpg", 12),
            member(r"C:\photos\a.cr2", r"C:\photos\selected\a.cr2", 120),
        ]),
    };
    let line = entry.to_line();

    let parsed = try_parse_tail_committed_move(line.as_bytes()).expect("a committed group Move is returned");

    assert_eq!(parsed, entry);
    assert_eq!(parsed.group_members.as_ref().map(Vec::len), Some(2));
}

// ---------------------------------------------------------------------------------------------------------------
// Golden check: tests/golden/journal-tail-committed-move.json.
// ---------------------------------------------------------------------------------------------------------------

fn decode_hex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "hex text has an odd length");
    text.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("hex is ASCII"), 16).expect("hex digits"))
        .collect()
}

/// A case's line bytes: `line_hex` when present, else `line_text` as UTF-8.
fn case_line(case: &Value) -> Vec<u8> {
    match case.get("line_hex").and_then(Value::as_str) {
        Some(hex) => decode_hex(hex),
        None => str_of(case, "line_text").as_bytes().to_vec(),
    }
}

#[test]
fn golden_tail_committed_move_matches_csharp_for_every_case() {
    let golden = golden::load("journal-tail-committed-move");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let line = case_line(case);

        let parsed = try_parse_tail_committed_move(&line);

        assert_eq!(
            parsed.is_some(),
            bool_of(case, "committed_move"),
            "case {name}: committed_move"
        );
        let expected_id = case["id"].as_str().map(str::to_owned);
        assert_eq!(parsed.map(|entry| entry.id), expected_id, "case {name}: id");
    }
}
