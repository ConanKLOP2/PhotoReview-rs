//! Red F tests for `pr_app::presenters::recovery_presentation` (W8-10f, contract `contracts/W8-10f.md`): the bodies are
//! `todo!()`. The time-dependent members use a fake `RecoveryTimeFormatter` (D-10f-2/3); the real English catalog is the
//! `Localizer`. Golden-covered cases are in `presenters_recovery_golden.rs`.

use pr_app::presenters::recovery_filter::{RecoveryFilter, RecoveryTone};
use pr_app::presenters::recovery_presentation::{
    RecoveryTexts, RecoveryTimeFormatter, allows_retry, matches, path_status_tone, rgb, verdict_tone,
};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_check::{
    RecoveryCheckResult, RecoveryGroupMemberCheck, RecoveryPathCheck, RecoveryPathStatus, RecoveryVerdict,
};
use pr_core::model::FileOperationType;
use pr_core::time::DateTimeUtc;
use pr_l10n::localizer::{Localizer, built_in_english_localizer};

/// Returns `<ticks>`: proves the UTC value is handed over unchanged (no offset, no zone).
struct FakeTime;

impl RecoveryTimeFormatter for FakeTime {
    fn format_local_general(&self, utc: DateTimeUtc) -> String {
        format!("<{}>", utc.ticks())
    }
}

const ALL_VERDICTS: [RecoveryVerdict; 11] = [
    RecoveryVerdict::CanRetry,
    RecoveryVerdict::AlreadyDone,
    RecoveryVerdict::Conflict,
    RecoveryVerdict::SourceChanged,
    RecoveryVerdict::DestinationChanged,
    RecoveryVerdict::Lost,
    RecoveryVerdict::Unknown,
    RecoveryVerdict::RecycleUnverifiable,
    RecoveryVerdict::NotRecycled,
    RecoveryVerdict::PermanentlyDeleted,
    RecoveryVerdict::PartiallyPermanentlyDeleted,
];

fn ticks(n: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(n).expect("valid ticks")
}

fn entry(op: FileOperationType, source: &str, size: i64) -> JournalEntry {
    JournalEntry {
        id: "0".repeat(32),
        op_type: op,
        state: pr_core::model::JournalState::Failed,
        source: source.to_owned(),
        destination: None,
        size,
        last_write_utc: JournalDateTime::utc(ticks(5)),
        timestamp_utc: JournalDateTime::default(),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

fn member(source: &str, permanent: bool) -> JournalGroupMember {
    JournalGroupMember {
        source: source.to_owned(),
        destination: None,
        size: 1,
        last_write_utc: JournalDateTime::default(),
        permanent,
    }
}

fn path_check(path: &str, status: RecoveryPathStatus, size: Option<i64>, time: Option<i64>) -> RecoveryPathCheck {
    RecoveryPathCheck::new(path, status, size, time.map(ticks), true)
}

fn result(entry: JournalEntry, verdict: RecoveryVerdict) -> RecoveryCheckResult {
    let source = path_check(&entry.source, RecoveryPathStatus::Exists, Some(entry.size), None);
    RecoveryCheckResult {
        entry,
        source,
        destination: None,
        verdict,
        group_members: None,
    }
}

fn group_result(op: FileOperationType, verdict: RecoveryVerdict) -> RecoveryCheckResult {
    let single = result(entry(op, "C:\\p\\a.jpg", 1), verdict);
    let mut group = single.clone();
    group.entry.group_members = Some(vec![member("C:\\p\\a.jpg", false)]);
    group.group_members = Some(vec![RecoveryGroupMemberCheck {
        member: member("C:\\p\\a.jpg", false),
        check: single,
    }]);
    group
}

fn with_texts<R>(group_separator: &str, f: impl FnOnce(&RecoveryTexts<'_>) -> R) -> R {
    let english: std::sync::Arc<Localizer> = built_in_english_localizer();
    let time = FakeTime;
    let texts = RecoveryTexts {
        loc: &english,
        time: &time,
        group_separator,
    };
    f(&texts)
}

#[test]
fn every_verdict_has_text_explanation_and_action_and_keys_are_not_shown_raw() {
    // C#: RecoveryPresenterTests.EveryVerdict_HasTextExplanationAndAction_AndKeysAreNotShownRaw
    with_texts(",", |t| {
        for verdict in ALL_VERDICTS {
            for (what, text) in [
                ("verdict", t.verdict_text(verdict)),
                ("explain", t.explain_text(verdict)),
                ("action", t.action_text(verdict)),
            ] {
                assert!(!text.trim().is_empty(), "{verdict:?} {what}");
                assert!(
                    !text.starts_with("recovery."),
                    "{verdict:?} {what} shows a raw key: {text}"
                );
            }
        }
    });
}

#[test]
fn verdicts_are_distinguishable_without_color() {
    // C#: RecoveryPresenterTests.Verdicts_AreDistinguishableWithoutColor
    with_texts(",", |t| {
        let texts: Vec<String> = ALL_VERDICTS.iter().map(|v| t.verdict_text(*v)).collect();
        for (i, a) in texts.iter().enumerate() {
            for b in &texts[i + 1..] {
                assert_ne!(a, b);
            }
        }
    });
}

#[test]
fn allows_retry_only_for_can_retry_move_copy() {
    // C#: RecoveryPresenterTests.AllowsRetry_OnlyForCanRetryMoveCopy (theory verdict x type; also golden allows-retry-*)
    for verdict in ALL_VERDICTS {
        for op in [
            FileOperationType::Move,
            FileOperationType::Copy,
            FileOperationType::Recycle,
        ] {
            let single = result(entry(op, "C:\\p\\a.jpg", 1), verdict);
            let want = verdict == RecoveryVerdict::CanRetry && op != FileOperationType::Recycle;
            assert_eq!(allows_retry(Some(&single)), want, "{verdict:?}/{op:?}");
        }
    }
}

#[test]
fn allows_retry_while_still_checking_is_false() {
    // C#: RecoveryPresenterTests.AllowsRetry_WhileStillChecking_IsFalse
    assert!(!allows_retry(None));
}

#[test]
fn allows_retry_for_a_group_recycle_entry_is_true_when_it_can_retry() {
    // C#: RecoveryPresenterMutationGapTests.AllowsRetry_ForAGroupRecycleEntry_IsTrueWhenItCanRetry
    let group = group_result(FileOperationType::Recycle, RecoveryVerdict::CanRetry);
    assert!(allows_retry(Some(&group)));
    let not_retry = group_result(FileOperationType::Recycle, RecoveryVerdict::AlreadyDone);
    assert!(!allows_retry(Some(&not_retry)));
}

#[test]
fn matches_follows_filter() {
    // C#: RecoveryPresenterTests.Matches_FollowsFilter (also golden matches-matrix)
    assert!(matches(RecoveryFilter::All, None));
    assert!(!matches(RecoveryFilter::Problems, None));
    assert!(matches(RecoveryFilter::Problems, Some(RecoveryVerdict::Lost)));
    assert!(!matches(RecoveryFilter::Problems, Some(RecoveryVerdict::CanRetry)));
    assert!(!matches(RecoveryFilter::Problems, Some(RecoveryVerdict::AlreadyDone)));
    assert!(matches(RecoveryFilter::CanRetry, Some(RecoveryVerdict::CanRetry)));
    assert!(matches(RecoveryFilter::AlreadyDone, Some(RecoveryVerdict::AlreadyDone)));
}

#[test]
fn verdict_brush_covers_every_verdict() {
    // C#: RecoveryPresenterMutationGapTests.VerdictBrush_CoversEveryVerdict (`verdict_tone` is total)
    for verdict in ALL_VERDICTS {
        let _ = verdict_tone(verdict);
    }
    assert_eq!(verdict_tone(RecoveryVerdict::CanRetry), RecoveryTone::Good);
    assert_eq!(verdict_tone(RecoveryVerdict::Lost), RecoveryTone::Bad);
    assert_eq!(verdict_tone(RecoveryVerdict::Unknown), RecoveryTone::Neutral);
}

#[test]
fn neutral_brush_is_the_gray_used_for_uncolored_states() {
    // C#: RecoveryPresenterCoverageTests.NeutralBrush_IsTheFrozenGrayUsedForUncoloredStates ("frozen" is WPF only)
    assert_eq!(rgb(RecoveryTone::Neutral), (0xB0, 0xB0, 0xB0));
}

#[test]
fn path_view_shows_path_status_sizes_and_folder_note() {
    // C#: RecoveryPresenterTests.PathView_ShowsPathStatusSizesAndFolderNote
    with_texts(",", |t| {
        let journal = entry(FileOperationType::Move, "C:\\p\\a.jpg", 2048);
        let check = path_check("C:\\p\\a.jpg", RecoveryPathStatus::Changed, Some(1234), Some(77));
        let view = t.path_view("Source", &check, &journal, true);
        assert_eq!(view.title, "Source");
        assert_eq!(view.path, "C:\\p\\a.jpg");
        assert_eq!(view.status_text, "Status: Changed since the journal entry");
        assert_eq!(view.status_tone, RecoveryTone::Warn);
        assert_eq!(view.now_text, "Now: 1,234 bytes \u{b7} modified <77>");
        assert_eq!(view.journal_text, "Journal recorded: 2,048 bytes \u{b7} modified <5>");
        assert_eq!(view.note.as_deref(), Some("The destination folder no longer exists."));
    });
}

#[test]
fn path_view_colours_and_words_the_status_of_the_path() {
    // C#: RecoveryPresenterCoverageTests.PathView_ColoursAndWordsTheStatusOfThePath (theory: status, r, g, b)
    with_texts(",", |t| {
        let journal = entry(FileOperationType::Move, "C:\\p\\a.jpg", 1);
        for (status, word, colour) in [
            (RecoveryPathStatus::Exists, "Exists", (0x6C, 0xCB, 0x7A)),
            (RecoveryPathStatus::Missing, "Missing", (0xB0, 0xB0, 0xB0)),
            (
                RecoveryPathStatus::Changed,
                "Changed since the journal entry",
                (0xF0, 0xB0, 0x40),
            ),
            (RecoveryPathStatus::Unreadable, "Cannot be read", (0xFF, 0x6B, 0x6B)),
        ] {
            let view = t.path_view("T", &path_check("C:\\p\\a.jpg", status, None, None), &journal, false);
            assert_eq!(view.status_text, format!("Status: {word}"), "{status:?}");
            assert_eq!(rgb(view.status_tone), colour, "{status:?}");
            assert_eq!(rgb(path_status_tone(status)), colour, "{status:?}");
        }
    });
}

#[test]
fn path_view_with_the_folder_missing_note_carries_the_folder_missing_text() {
    // C#: RecoveryPresenterCoverageTests.PathView_WithTheFolderMissingNote_CarriesTheFolderMissingText
    with_texts(",", |t| {
        let journal = entry(FileOperationType::Copy, "C:\\p\\a.jpg", 1);
        let check = path_check("C:\\out\\a.jpg", RecoveryPathStatus::Missing, None, None);
        let with = t.path_view("Destination", &check, &journal, true);
        assert_eq!(with.note, Some(t.loc.get("recovery.detail.folderMissing")));
        assert_eq!(t.path_view("Destination", &check, &journal, false).note, None);
    });
}

#[test]
fn now_text_for_an_unreadable_file_says_it_could_not_be_read_and_a_changed_one_still_shows_its_size_and_time() {
    // C#: RecoveryPresenterCoverageTests.NowText_ForAnUnreadableFile_SaysItCouldNotBeRead_AndAChangedOneStillShowsItsSizeAndTime
    with_texts(",", |t| {
        let unreadable = path_check("C:\\p\\a.jpg", RecoveryPathStatus::Unreadable, None, None);
        assert_eq!(t.now_text(&unreadable), t.loc.get("recovery.detail.nowUnreadable"));
        assert_eq!(t.now_text(&unreadable), "Now: cannot be read");

        let missing = path_check("C:\\p\\a.jpg", RecoveryPathStatus::Missing, None, None);
        assert_eq!(t.now_text(&missing), "Now: no file at this path");

        let changed = path_check("C:\\p\\a.jpg", RecoveryPathStatus::Changed, Some(4096), Some(99));
        assert_eq!(t.now_text(&changed), "Now: 4,096 bytes \u{b7} modified <99>");
    });
}

#[test]
fn now_text_for_a_present_file_shows_its_current_size_and_time() {
    // C#: RecoveryPresenterMutationGapTests.NowText_ForAPresentFile_ShowsItsCurrentSizeAndTime
    with_texts(",", |t| {
        let present = path_check(
            "C:\\p\\a.jpg",
            RecoveryPathStatus::Exists,
            Some(1_234_567),
            Some(123_456_789),
        );
        assert_eq!(t.now_text(&present), "Now: 1,234,567 bytes \u{b7} modified <123456789>");
        // Rule 8: a missing size and time read as 0 and the default time (ticks 0).
        let bare = path_check("C:\\p\\a.jpg", RecoveryPathStatus::Exists, None, None);
        assert_eq!(t.now_text(&bare), "Now: 0 bytes \u{b7} modified <0>");
    });
}

#[test]
fn format_time_treats_the_stamp_as_utc_and_shows_local_time() {
    // C#: RecoveryPresenterCoverageTests.FormatTime_TreatsTheStampAsUtcAndShowsLocalTime (D-10f-2: the trait gets the UTC
    // value unchanged; the real local-time text is a native check)
    with_texts(",", |t| {
        assert_eq!(t.format_time(ticks(638_000_000_000_000_000)), "<638000000000000000>");
        assert_eq!(t.format_time(DateTimeUtc::MIN), "<0>");
    });
}

#[test]
fn journal_text_uses_the_entry_size_and_utc_time() {
    // new: rule 15, `{size}` is `format_size(entry.size)` and `{time}` comes from the formatter with `.value`.
    with_texts(",", |t| {
        let mut journal = entry(FileOperationType::Move, "C:\\p\\a.jpg", 1000);
        journal.last_write_utc = JournalDateTime::unspecified(ticks(42));
        assert_eq!(
            t.journal_text(&journal),
            "Journal recorded: 1,000 bytes \u{b7} modified <42>"
        );
    });
}

#[test]
fn format_size_uses_the_group_separator_it_is_given() {
    // new: D-10f-4, the separator is a parameter (the golden only uses ",").
    with_texts(".", |t| {
        assert_eq!(t.format_size(1_234_567), "1.234.567 bytes");
        assert_eq!(t.format_size(999), "999 bytes");
        assert_eq!(t.format_size(-1000), "-1.000 bytes");
    });
}

#[test]
fn retry_confirm_text_permanent_member_already_gone_does_not_warn_about_permanent_deletion() {
    // C#: RecoveryPresenterTests.RetryConfirmText_PermanentMemberAlreadyGone_DoesNotWarnAboutPermanentDeletion (also F for
    // the golden `...counts-only-notrecycled`)
    with_texts(",", |t| {
        let mut journal = entry(FileOperationType::Recycle, "C:\\photos\\a.jpg", 1);
        journal.group_members = Some(vec![
            member("C:\\photos\\a.jpg", true),
            member("C:\\photos\\b.jpg", false),
        ]);
        let mut check = result(journal.clone(), RecoveryVerdict::NotRecycled);
        check.group_members = Some(vec![
            RecoveryGroupMemberCheck {
                // the permanent member is already recycled
                member: member("C:\\photos\\a.jpg", true),
                check: result(
                    entry(FileOperationType::Recycle, "C:\\photos\\a.jpg", 1),
                    RecoveryVerdict::RecycleUnverifiable,
                ),
            },
            RecoveryGroupMemberCheck {
                member: member("C:\\photos\\b.jpg", false),
                check: result(
                    entry(FileOperationType::Recycle, "C:\\photos\\b.jpg", 1),
                    RecoveryVerdict::NotRecycled,
                ),
            },
        ]);
        assert_eq!(
            t.retry_confirm_text(&journal, Some(&check), true),
            "Retry Move to Recycle Bin for 2 capture-group members? Completed members will be skipped."
        );
    });
}

#[test]
fn retry_confirm_text_cancelled_group_delete_without_permanent_members_keeps_the_cancelled_wording() {
    // C#: RecoveryPresenterTests.RetryConfirmText_CancelledGroupDeleteWithoutPermanentMembers_KeepsTheCancelledWording
    with_texts(",", |t| {
        let mut journal = entry(FileOperationType::Recycle, "C:\\photos\\a.jpg", 1);
        journal.error_code = Some(pr_core::file_actions::errors::CANCELLED_BY_USER.to_owned());
        journal.group_members = Some(vec![
            member("C:\\photos\\a.jpg", false),
            member("C:\\photos\\b.jpg", false),
        ]);
        assert_eq!(
            t.retry_confirm_text(&journal, None, true),
            "You cancelled this Move to Recycle Bin earlier. Retrying will FINISH it for 2 file(s). Continue?"
        );
    });
}

#[test]
fn retry_confirm_text_counts_one_file_when_the_member_list_is_empty() {
    // new: rule 11b, `Some(vec![])` counts as 1 (C# `Count > 0`).
    with_texts(",", |t| {
        let mut journal = entry(FileOperationType::Move, "C:\\photos\\a.jpg", 1);
        journal.error_code = Some(pr_core::file_actions::errors::CANCELLED_BY_USER.to_owned());
        journal.group_members = Some(vec![]);
        assert_eq!(
            t.retry_confirm_text(&journal, None, false),
            "You cancelled this Move earlier. Retrying will FINISH it for 1 file(s). Continue?"
        );
    });
}

#[test]
fn retry_confirm_text_names_only_the_file_name_after_the_last_separator() {
    // new: rule 11c, `get_file_name` takes the part after the last `\` or `/`.
    with_texts(",", |t| {
        for source in ["C:\\photos\\IMG_1.jpg", "C:/photos/IMG_1.jpg"] {
            let journal = entry(FileOperationType::Copy, source, 1);
            assert_eq!(
                t.retry_confirm_text(&journal, None, false),
                "Retry Copy for IMG_1.jpg?",
                "{source}"
            );
        }
    });
}

#[test]
fn retry_confirm_text_undo_wording_is_only_for_undo_true() {
    // new: rule 11a, `undo` is `Option<bool>`: `Some(false)` and `None` are not an undo.
    with_texts(",", |t| {
        let mut journal = entry(FileOperationType::Recycle, "C:\\photos\\a.jpg", 1);
        journal.group_members = Some(vec![member("C:\\photos\\a.jpg", false)]);
        journal.undo = Some(false);
        let plain = "Retry Move to Recycle Bin for 1 capture-group members? Completed members will be skipped.";
        assert_eq!(t.retry_confirm_text(&journal, None, true), plain);
        journal.undo = None;
        assert_eq!(t.retry_confirm_text(&journal, None, true), plain);
        journal.undo = Some(true);
        assert_eq!(
            t.retry_confirm_text(&journal, None, true),
            "Restore capture-group members from the Recycle Bin (up to 1)? Members that are already back will be skipped."
        );
    });
}

#[test]
fn state_text_names_dismissed_with_the_literal() {
    // new: rule 13 (golden state-text-all-includes-dismissed)
    with_texts(",", |t| {
        assert_eq!(t.state_text(pr_core::model::JournalState::Dismissed), "Dismissed");
        assert_eq!(t.operation_text(FileOperationType::Recycle), "Move to Recycle Bin");
    });
}
