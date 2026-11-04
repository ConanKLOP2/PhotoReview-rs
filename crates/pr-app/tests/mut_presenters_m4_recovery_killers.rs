//! M4 mutation killers for `presenters/recovery_presentation.rs` (hand-written, no golden data).
use pr_app::presenters::recovery_presentation::{RecoveryTexts, RecoveryTimeFormatter};
use pr_core::file_actions::recovery_check::{RecoveryPathStatus, RecoveryVerdict};
use pr_core::time::DateTimeUtc;
use pr_l10n::localizer::built_in_english_localizer;

struct FakeTime;

impl RecoveryTimeFormatter for FakeTime {
    fn format_local_general(&self, utc: DateTimeUtc) -> String {
        format!("<{}>", utc.ticks())
    }
}

const VERDICTS: [(RecoveryVerdict, &str); 11] = [
    (RecoveryVerdict::CanRetry, "canRetry"),
    (RecoveryVerdict::AlreadyDone, "alreadyDone"),
    (RecoveryVerdict::Conflict, "conflict"),
    (RecoveryVerdict::SourceChanged, "sourceChanged"),
    (RecoveryVerdict::DestinationChanged, "destinationChanged"),
    (RecoveryVerdict::Lost, "lost"),
    (RecoveryVerdict::Unknown, "unknown"),
    (RecoveryVerdict::RecycleUnverifiable, "recycleUnverifiable"),
    (RecoveryVerdict::NotRecycled, "notRecycled"),
    (RecoveryVerdict::PermanentlyDeleted, "permanentlyDeleted"),
    (
        RecoveryVerdict::PartiallyPermanentlyDeleted,
        "partiallyPermanentlyDeleted",
    ),
];

// C# `VerdictText` / `ExplainText` / `ActionText` / `PathStatusText`: each member maps to the catalog entry of its own name.
#[test]
fn every_verdict_and_path_status_maps_to_its_own_catalog_entry() {
    let loc = built_in_english_localizer();
    let t = RecoveryTexts {
        loc: &loc,
        time: &FakeTime,
        group_separator: ",",
    };
    for (verdict, name) in VERDICTS {
        for (kind, actual) in [
            ("verdict", t.verdict_text(verdict)),
            ("explain", t.explain_text(verdict)),
            ("action", t.action_text(verdict)),
        ] {
            let key = format!("recovery.{kind}.{name}");
            let expected = loc.format(&key, &[]);
            assert_ne!(expected, key, "{key} is missing from the English catalog");
            assert_eq!(actual, expected, "{key}");
        }
    }
    for (status, name) in [
        (RecoveryPathStatus::Exists, "exists"),
        (RecoveryPathStatus::Missing, "missing"),
        (RecoveryPathStatus::Changed, "changed"),
        (RecoveryPathStatus::Unreadable, "unreadable"),
    ] {
        let key = format!("recovery.pathStatus.{name}");
        let expected = loc.format(&key, &[]);
        assert_ne!(expected, key, "{key} is missing from the English catalog");
        assert_eq!(t.path_status_text(status), expected, "{key}");
    }
}

fn texts<'a>(loc: &'a pr_l10n::localizer::Localizer, sep: &'a str) -> RecoveryTexts<'a> {
    RecoveryTexts {
        loc,
        time: &FakeTime,
        group_separator: sep,
    }
}

// C# `FormatSize` = `N0` + the "{size} bytes" text: groups of three digits from the right, never a leading separator.
#[test]
fn format_size_groups_digits_by_three_without_a_leading_separator() {
    let loc = built_in_english_localizer();
    let t = texts(&loc, ",");
    for (bytes, grouped) in [
        (0_i64, "0"),
        (7, "7"),
        (12, "12"),
        (123, "123"),
        (1234, "1,234"),
        (12345, "12,345"),
        (123456, "123,456"),
        (1234567, "1,234,567"),
        (123456789, "123,456,789"),
        (-123, "-123"),
        (-1234, "-1,234"),
        (-123456, "-123,456"),
        (i64::MAX, "9,223,372,036,854,775,807"),
    ] {
        assert_eq!(t.format_size(bytes), format!("{grouped} bytes"), "{bytes}");
    }
    assert_eq!(texts(&loc, "'").format_size(1234567), "1'234'567 bytes");
}

// C# `GroupStatusText`: the state word by verdict; with a destination the two path statuses follow.
#[test]
fn group_status_text_maps_each_verdict_to_its_state_word_and_appends_path_statuses() {
    use pr_core::file_actions::journal_line::{JournalDateTime, JournalGroupMember};
    use pr_core::file_actions::recovery_check::RecoveryPathCheck;
    let loc = built_in_english_localizer();
    let t = texts(&loc, ",");
    let member = |destination: Option<&str>| JournalGroupMember {
        source: "s".to_owned(),
        destination: destination.map(str::to_owned),
        size: 1,
        last_write_utc: JournalDateTime::default(),
        permanent: false,
    };
    let check = |status| RecoveryPathCheck::new("p", status, None, None, true);
    let word = |key: &str| {
        let text = loc.format(key, &[]);
        assert_ne!(text, key, "{key} missing from the English catalog");
        text
    };
    let expected = [
        (RecoveryVerdict::AlreadyDone, word("recovery.group.exists")),
        (RecoveryVerdict::CanRetry, word("recovery.group.sourceStatus")),
        (RecoveryVerdict::NotRecycled, word("recovery.group.onDisk")),
        (RecoveryVerdict::RecycleUnverifiable, word("recovery.group.missing")),
        (RecoveryVerdict::SourceChanged, word("recovery.group.changed")),
        (RecoveryVerdict::DestinationChanged, word("recovery.group.changed")),
        (RecoveryVerdict::Conflict, word("recovery.group.conflict")),
        (RecoveryVerdict::Lost, word("recovery.group.lost")),
        (RecoveryVerdict::Unknown, word("recovery.group.unreadable")),
        (
            RecoveryVerdict::PermanentlyDeleted,
            t.verdict_text(RecoveryVerdict::PermanentlyDeleted),
        ),
        (
            RecoveryVerdict::PartiallyPermanentlyDeleted,
            t.verdict_text(RecoveryVerdict::PartiallyPermanentlyDeleted),
        ),
    ];
    let src = check(RecoveryPathStatus::Exists);
    for (verdict, text) in &expected {
        assert_eq!(
            &t.group_status_text(*verdict, &member(None), &src, None),
            text,
            "{verdict:?} without destination"
        );
    }
    let dst = check(RecoveryPathStatus::Changed);
    let src_word = loc.format("recovery.detail.source", &[]);
    let dst_word = loc.format("recovery.detail.destination", &[]);
    let exists = t.path_status_text(RecoveryPathStatus::Exists);
    let changed = t.path_status_text(RecoveryPathStatus::Changed);
    let missing = t.path_status_text(RecoveryPathStatus::Missing);
    for (verdict, text) in &expected {
        assert_eq!(
            t.group_status_text(*verdict, &member(Some("d")), &src, Some(&dst)),
            format!("{text} \u{b7} {src_word}: {exists} \u{b7} {dst_word}: {changed}"),
            "{verdict:?} with destination"
        );
    }
    // No destination check: the destination status is Missing.
    assert_eq!(
        t.group_status_text(RecoveryVerdict::Lost, &member(Some("d")), &src, None),
        format!(
            "{} \u{b7} {src_word}: {exists} \u{b7} {dst_word}: {missing}",
            word("recovery.group.lost")
        )
    );
}

// --- retry_confirm_text, nearest_existing_folder, Debug ------------------------------------------------------------------

fn cancelled_delete_group(permanent: &[bool]) -> pr_core::file_actions::journal_line::JournalEntry {
    use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
    use pr_core::model::{FileOperationType, JournalState};
    let stamp = JournalDateTime::utc(DateTimeUtc::from_ticks(639_264_384_000_000_000).unwrap());
    JournalEntry {
        id: "g".to_owned(),
        op_type: FileOperationType::Recycle,
        state: JournalState::Failed,
        source: r"C:\a\1.jpg".to_owned(),
        destination: None,
        size: 1,
        last_write_utc: stamp,
        timestamp_utc: stamp,
        error: Some("cancelled".to_owned()),
        error_code: Some(pr_core::file_actions::errors::CANCELLED_BY_USER.to_owned()),
        permanent: None,
        undo: None,
        group_id: Some("capture".to_owned()),
        group_members: Some(
            permanent
                .iter()
                .enumerate()
                .map(|(i, p)| JournalGroupMember {
                    source: format!(r"C:\a\{i}.jpg"),
                    destination: None,
                    size: 1,
                    last_write_utc: stamp,
                    permanent: *p,
                })
                .collect(),
        ),
    }
}

// C# `RetryConfirmText`, cancelled branch: the PERMANENT text needs at least one permanent member; none gives the plain one.
#[test]
fn cancelled_delete_confirmation_mentions_permanent_deletion_only_when_a_member_is_permanent() {
    let loc = built_in_english_localizer();
    let t = texts(&loc, ",");

    let with_permanent = t.retry_confirm_text(&cancelled_delete_group(&[true, false, true]), None, true);
    assert_eq!(
        with_permanent,
        "You cancelled this Move to Recycle Bin earlier. Retrying will FINISH it for 3 file(s); 2 of them are on drives without a Recycle Bin and will be deleted PERMANENTLY. This cannot be undone. Continue?"
    );

    let none_permanent = t.retry_confirm_text(&cancelled_delete_group(&[false, false]), None, true);
    assert_eq!(
        none_permanent,
        "You cancelled this Move to Recycle Bin earlier. Retrying will FINISH it for 2 file(s). Continue?"
    );

    let not_allowed = t.retry_confirm_text(&cancelled_delete_group(&[true]), None, false);
    assert_eq!(
        not_allowed,
        "You cancelled this Move to Recycle Bin earlier. Retrying will FINISH it for 1 file(s). Continue?"
    );
}

// C# `NearestExistingFolder`: the first existing ANCESTOR (never the path itself), None when nothing exists or the path has no folder.
#[test]
fn nearest_existing_folder_walks_up_to_the_first_existing_ancestor() {
    use pr_app::presenters::recovery_presentation::nearest_existing_folder;
    let exists = |p: &str| p == r"C:\a" || p == r"C:\a\b\c\d.jpg";
    assert_eq!(
        nearest_existing_folder(r"C:\a\b\c\d.jpg", &exists),
        Some(r"C:\a".to_owned()),
        "the file path itself is not tested, only its ancestors"
    );
    assert_eq!(
        nearest_existing_folder(r"C:\a\x.jpg", &exists),
        Some(r"C:\a".to_owned())
    );
    assert_eq!(nearest_existing_folder(r"C:\z\y\x.jpg", &|_| false), None);
    assert_eq!(nearest_existing_folder("x.jpg", &|_| true), None, "no folder part");
}

// `impl Debug for RecoveryTexts` names the type and the separator, not the localizer.
#[test]
fn recovery_texts_debug_shows_the_group_separator() {
    let loc = built_in_english_localizer();
    let shown = format!("{:?}", texts(&loc, " | "));
    assert!(shown.starts_with("RecoveryTexts"), "{shown}");
    assert!(shown.contains("group_separator: \" | \""), "{shown}");
}
