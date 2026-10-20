//! F tests for `pr_app::presenters::recovery_window` (W8-10g, contract `contracts/W8-10g.md`): the Recovery window's behaviour
//! without a window. C# oracle: `RecoveryWindowPresenterTests` (baseline `d92d6aae`). The view is a recording fake, the retry
//! delegate is a fake (no real retry, no Recycle Bin), the live check runs with an inline or a collecting `post` (no thread, no
//! sleep). Only `run_checks_reports_every_row_and_writes_nothing` touches the disk, and only reads a temp folder.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pr_app::presenters::recovery_filter::{RecoveryFilter, RecoveryTone};
use pr_app::presenters::recovery_presentation::{RecoveryTexts, RecoveryTimeFormatter, rgb};
use pr_app::presenters::recovery_window::{
    DismissFn, RecoveryCheckEvent, RecoveryCheckMessage, RecoveryCheckView, RecoveryChecker, RecoveryRowModel,
    RecoveryWindowOptions, RecoveryWindowPresenter, RetryFn,
};
use pr_core::abstractions::FileSystem;
use pr_core::file_actions::errors::PENDING_UNCONFIRMED;
use pr_core::file_actions::journal::DismissOutcome;
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_check::{
    RecoveryCheckResult, RecoveryFileCheck, RecoveryGroupMemberCheck, RecoveryPathCheck, RecoveryPathStatus,
    RecoveryVerdict,
};
use pr_core::file_actions::recovery_retry::{AllowPermanentDelete, RecoveryRetryResult};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_l10n::keys;
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_l10n::template::LocArg;
use pr_test_support::TempRoot;

// ---- helpers ----

struct FakeTime;

impl RecoveryTimeFormatter for FakeTime {
    fn format_local_general(&self, utc: DateTimeUtc) -> String {
        format!("<{}>", utc.ticks())
    }
}

fn ticks(n: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(n).expect("valid ticks")
}

fn stamp() -> JournalDateTime {
    JournalDateTime::utc(ticks(1_000))
}

fn loc() -> Arc<Localizer> {
    built_in_english_localizer()
}

fn with_texts<R>(f: impl FnOnce(&RecoveryTexts<'_>) -> R) -> R {
    let english = loc();
    let time = FakeTime;
    let texts = RecoveryTexts {
        loc: &english,
        time: &time,
        group_separator: ",",
    };
    f(&texts)
}

fn entry(id: &str) -> JournalEntry {
    entry_with(id, FileOperationType::Move, None, None)
}

fn entry_with(id: &str, op: FileOperationType, source: Option<&str>, destination: Option<&str>) -> JournalEntry {
    JournalEntry {
        id: id.to_owned(),
        op_type: op,
        state: JournalState::Failed,
        source: source.map_or_else(|| format!("wp35g0-missing/{id}.jpg"), str::to_owned),
        destination: destination.map(str::to_owned),
        size: 10,
        last_write_utc: stamp(),
        timestamp_utc: stamp(),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

fn member(source: &str, destination: Option<&str>, permanent: bool) -> JournalGroupMember {
    JournalGroupMember {
        source: source.to_owned(),
        destination: destination.map(str::to_owned),
        size: 1,
        last_write_utc: stamp(),
        permanent,
    }
}

fn check(entry: &JournalEntry, verdict: RecoveryVerdict) -> RecoveryCheckResult {
    check_with(entry, verdict, None, None)
}

fn check_with(
    entry: &JournalEntry,
    verdict: RecoveryVerdict,
    destination: Option<RecoveryPathCheck>,
    members: Option<Vec<RecoveryGroupMemberCheck>>,
) -> RecoveryCheckResult {
    RecoveryCheckResult {
        entry: entry.clone(),
        source: RecoveryPathCheck::new(
            entry.source.clone(),
            RecoveryPathStatus::Exists,
            Some(entry.size),
            None,
            true,
        ),
        destination,
        verdict,
        group_members: members,
    }
}

fn ok_result() -> RecoveryRetryResult {
    RecoveryRetryResult::new(true, "done", None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Status(String),
    Recheck(bool),
    Completed,
    Row(usize),
}

#[derive(Default)]
struct FakeView {
    calls: Vec<Call>,
}

impl FakeView {
    fn status(&self) -> Vec<String> {
        self.calls
            .iter()
            .filter_map(|c| if let Call::Status(s) = c { Some(s.clone()) } else { None })
            .collect()
    }
    fn recheck(&self) -> Vec<bool> {
        self.calls
            .iter()
            .filter_map(|c| if let Call::Recheck(b) = c { Some(*b) } else { None })
            .collect()
    }
    fn completed(&self) -> usize {
        self.calls.iter().filter(|c| **c == Call::Completed).count()
    }
    fn rows_changed(&self) -> Vec<usize> {
        self.calls
            .iter()
            .filter_map(|c| if let Call::Row(i) = c { Some(*i) } else { None })
            .collect()
    }
}

impl RecoveryCheckView for FakeView {
    fn set_check_status(&mut self, text: &str) {
        self.calls.push(Call::Status(text.to_owned()));
    }
    fn set_recheck_enabled(&mut self, enabled: bool) {
        self.calls.push(Call::Recheck(enabled));
    }
    fn check_completed(&mut self) {
        self.calls.push(Call::Completed);
    }
    fn row_changed(&mut self, row_index: usize) {
        self.calls.push(Call::Row(row_index));
    }
}

/// Checker that gives a fixed verdict for every entry and counts its calls.
struct FakeChecker {
    calls: AtomicUsize,
    verdict: RecoveryVerdict,
    fail_on_call: Option<usize>,
    panic_on_call: Option<usize>,
}

impl FakeChecker {
    fn new(verdict: RecoveryVerdict) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            verdict,
            fail_on_call: None,
            panic_on_call: None,
        }
    }
}

impl RecoveryChecker for FakeChecker {
    fn check(&self, entry: &JournalEntry) -> Result<RecoveryCheckResult, String> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_on_call == Some(n) {
            return Err("share went away".to_owned());
        }
        if self.panic_on_call == Some(n) {
            panic!("checker blew up");
        }
        Ok(check(entry, self.verdict))
    }
}

fn options(checker: Arc<dyn RecoveryChecker>) -> RecoveryWindowOptions {
    RecoveryWindowOptions {
        retry: None,
        dismiss: None,
        checker,
        allow_permanent_delete: None,
        current_time_text: Arc::new(|| "12:34:56".to_owned()),
    }
}

fn plain_options() -> RecoveryWindowOptions {
    options(Arc::new(FakeChecker::new(RecoveryVerdict::CanRetry)))
}

fn presenter(entries: Vec<JournalEntry>) -> RecoveryWindowPresenter {
    RecoveryWindowPresenter::new(entries, plain_options())
}

fn retry_fn(f: impl Fn(&JournalEntry) -> Result<RecoveryRetryResult, String> + Send + Sync + 'static) -> RetryFn {
    Arc::new(f)
}

fn dismiss_all() -> DismissFn {
    Box::new(|entries| {
        Ok(DismissOutcome {
            dismissed: entries.to_vec(),
            skipped: Vec::new(),
        })
    })
}

fn ids(p: &RecoveryWindowPresenter) -> Vec<String> {
    p.rows().iter().map(|r| r.entry().id.clone()).collect()
}

/// A presenter whose rows already carry the given verdicts (a checker that answers by entry Id, then a blocking run).
fn presenter_with_verdicts(
    items: Vec<(JournalEntry, RecoveryVerdict)>,
    mut opts: RecoveryWindowOptions,
) -> RecoveryWindowPresenter {
    struct ById(Vec<(String, RecoveryVerdict)>);
    impl RecoveryChecker for ById {
        fn check(&self, entry: &JournalEntry) -> Result<RecoveryCheckResult, String> {
            let verdict = self
                .0
                .iter()
                .find(|(id, _)| *id == entry.id)
                .map(|(_, v)| *v)
                .ok_or("unknown id")?;
            Ok(check(entry, verdict))
        }
    }
    let map = items.iter().map(|(e, v)| (e.id.clone(), *v)).collect();
    opts.checker = Arc::new(ById(map));
    let entries = items.into_iter().map(|(e, _)| e).collect();
    let mut p = RecoveryWindowPresenter::new(entries, opts);
    let mut view = FakeView::default();
    with_texts(|t| p.run_checks_blocking(&mut view, t));
    p
}

// ---- list ----

#[test]
fn list_unchecked_rows_match_all_only() {
    // C#: RecoveryWindowPresenterTests.List_UncheckedRows_MatchAllOnly
    let p = presenter(vec![entry("1"), entry("2")]);
    let english = loc();
    assert_eq!(p.shown(RecoveryFilter::All).len(), 2);
    assert!(p.shown(RecoveryFilter::CanRetry).is_empty());
    assert!(p.shown(RecoveryFilter::Problems).is_empty());
    assert!(p.shown(RecoveryFilter::AlreadyDone).is_empty());
    assert_eq!(
        p.summary_text(&english),
        english.format_plural("recovery.summary.count", 2, &[LocArg::new("count", 2)])
    );
    assert!(p.filter_empty_visible(0));
    assert!(!p.filter_empty_visible(2));
    let empty = presenter(vec![]);
    assert!(!empty.filter_empty_visible(0));
    assert_eq!(
        empty.summary_text(&english),
        english.format(keys::RECOVERY_SUMMARY_EMPTY, &[])
    );
}

#[test]
fn list_filter_follows_verdicts() {
    // C#: RecoveryWindowPresenterTests.List_FilterFollowsVerdicts
    let p = presenter_with_verdicts(
        vec![
            (entry("1"), RecoveryVerdict::CanRetry),
            (entry("2"), RecoveryVerdict::AlreadyDone),
            (entry("3"), RecoveryVerdict::Lost),
        ],
        plain_options(),
    );
    assert_eq!(p.shown(RecoveryFilter::CanRetry), vec![0]);
    assert_eq!(p.shown(RecoveryFilter::AlreadyDone), vec![1]);
    assert_eq!(p.shown(RecoveryFilter::Problems), vec![2]);
    assert_eq!(p.shown(RecoveryFilter::All), vec![0, 1, 2]);
}

#[test]
fn row_badge_follows_result() {
    // C#: RecoveryWindowPresenterTests.Row_BadgeFollowsResult (the ResultChanged count is checked via row_changed in the run tests)
    let e = entry("1");
    let mut row = RecoveryRowModel::new(e.clone());
    with_texts(|t| {
        assert_eq!(row.badge_text(t), t.loc.format(keys::RECOVERY_ROW_CHECKING, &[]));
        assert_eq!(row.badge_tone(), RecoveryTone::Neutral);
        row.set_result(Some(check(&e, RecoveryVerdict::CanRetry)));
        assert_eq!(row.badge_text(t), t.verdict_text(RecoveryVerdict::CanRetry));
        assert_eq!(row.badge_tone(), RecoveryTone::Good);
        row.set_result(None);
        assert!(row.check().is_none());
    });
}

#[test]
fn row_titles() {
    // C#: RecoveryWindowPresenterTests.Row_Titles
    with_texts(|t| {
        let plain = RecoveryRowModel::new(entry_with("1", FileOperationType::Move, Some(r"C:\a\one.jpg"), None));
        assert_eq!(plain.title(t), "one.jpg");

        let mut grouped = entry_with("2", FileOperationType::Move, Some(r"C:\a\two.jpg"), None);
        grouped.group_members = Some(vec![
            member(r"C:\a\two.jpg", None, false),
            member(r"C:\a\two.raw", None, false),
        ]);
        let group = RecoveryRowModel::new(grouped);
        assert_eq!(
            group.title(t),
            t.loc.format(
                keys::RECOVERY_GROUP_TITLE,
                &[LocArg::new("fileName", "two.jpg"), LocArg::new("count", 2i64)]
            )
        );
        assert_eq!(
            plain.subtitle(t),
            t.loc.format(
                keys::RECOVERY_ROW_SUBTITLE,
                &[
                    LocArg::new("state", t.state_text(JournalState::Failed)),
                    LocArg::new("operation", t.operation_text(FileOperationType::Move)),
                ]
            )
        );
    });
}

// ---- buttons ----

#[test]
fn buttons_truth_table() {
    // C#: RecoveryWindowPresenterTests.Buttons_TruthTable
    let none = presenter_with_verdicts(vec![(entry("1"), RecoveryVerdict::CanRetry)], plain_options());
    let states = none.button_states(Some(0), 1);
    assert_eq!(
        (states.retry, states.clear_selected, states.clear_all),
        (false, false, false)
    );

    let mut opts = plain_options();
    opts.retry = Some(retry_fn(|_| Ok(ok_result())));
    opts.dismiss = Some(dismiss_all());
    let mut p = presenter_with_verdicts(
        vec![
            (entry("1"), RecoveryVerdict::CanRetry),
            (entry("2"), RecoveryVerdict::Conflict),
        ],
        opts,
    );
    let t = |s: pr_app::presenters::recovery_window::RecoveryButtonStates| (s.retry, s.clear_selected, s.clear_all);
    assert_eq!(t(p.button_states(Some(0), 1)), (true, true, true));
    assert_eq!(t(p.button_states(Some(1), 1)), (false, true, true)); // not a retry verdict
    assert_eq!(t(p.button_states(None, 0)), (false, false, true)); // nothing selected
    assert_eq!(t(p.button_states(None, 2)), (false, true, true)); // several selected: no single row

    let e = p.rows()[0].entry().clone();
    let busy = RefCell::new(Vec::new());
    let job = p
        .begin_retry(&e, &mut |b| busy.borrow_mut().push(b))
        .expect("retry available");
    assert!(p.is_retrying());
    assert!(!p.can_close());
    assert_eq!(t(p.button_states(Some(0), 1)), (false, false, false));
    assert!(!p.can_start_retry(Some(0)));

    let outcome = job.run();
    let result = p.finish_retry(&e.id, outcome, &mut |b| busy.borrow_mut().push(b), &mut || {});
    assert!(result.is_ok());
    assert!(!p.is_retrying());
    assert!(p.can_close());
    assert!(p.can_start_retry(Some(0)));
    assert_eq!(*busy.borrow(), vec![true, false]);
}

#[test]
fn buttons_clear_all_needs_rows() {
    // C#: RecoveryWindowPresenterTests.Buttons_ClearAllNeedsRows
    let mut opts = plain_options();
    opts.dismiss = Some(dismiss_all());
    let p = RecoveryWindowPresenter::new(vec![], opts);
    assert!(!p.button_states(None, 0).clear_all);
}

// ---- details ----

#[test]
fn details_unchecked() {
    // C#: RecoveryWindowPresenterTests.Details_Unchecked
    let mut e = entry_with("1", FileOperationType::Move, Some(r"C:\a\x.jpg"), Some(r"D:\b\x.jpg"));
    e.error_code = Some(PENDING_UNCONFIRMED.to_owned());
    let row = RecoveryRowModel::new(e);
    with_texts(|t| {
        let d = RecoveryWindowPresenter::details(&row, t);
        assert_eq!(d.verdict_text, t.loc.format(keys::RECOVERY_ROW_CHECKING, &[]));
        assert_eq!(d.verdict_tone, RecoveryTone::Neutral);
        assert_eq!(d.explain_text, t.loc.format(keys::RECOVERY_DETAIL_CHECKING, &[]));
        assert_eq!(d.action_text, "");
        assert!(d.source_visible);
        assert_eq!(d.source.as_ref().expect("source").path, r"C:\a\x.jpg");
        assert!(d.destination_visible);
        assert_eq!(d.destination.as_ref().expect("destination").path, r"D:\b\x.jpg");
        assert!(!d.no_destination_visible);
        assert!(d.group_members.is_none());
        assert!(d.group_heading.is_none());
        assert!(d.error_visible);
        assert!(!d.error_text.trim().is_empty());
    });
}

#[test]
fn details_unchecked_without_destination() {
    // C#: RecoveryWindowPresenterTests.Details_UncheckedWithoutDestination
    let row = RecoveryRowModel::new(entry_with("1", FileOperationType::Recycle, None, None));
    with_texts(|t| {
        let d = RecoveryWindowPresenter::details(&row, t);
        assert!(!d.destination_visible);
        assert!(d.destination.is_none());
        assert!(d.no_destination_visible);
    });
}

#[test]
fn details_checked_plain_row() {
    // C#: RecoveryWindowPresenterTests.Details_CheckedPlainRow
    let e = entry_with(
        "1",
        FileOperationType::Move,
        Some(r"C:\a\x.jpg"),
        Some(r"D:\gone\x.jpg"),
    );
    let dest = RecoveryPathCheck::new(r"D:\gone\x.jpg", RecoveryPathStatus::Missing, None, None, false);
    let mut row = RecoveryRowModel::new(e.clone());
    row.set_result(Some(check_with(&e, RecoveryVerdict::CanRetry, Some(dest), None)));
    with_texts(|t| {
        let d = RecoveryWindowPresenter::details(&row, t);
        assert_eq!(d.verdict_text, t.verdict_text(RecoveryVerdict::CanRetry));
        assert_eq!(d.verdict_tone, RecoveryTone::Good);
        assert_eq!(d.explain_text, t.explain_text(RecoveryVerdict::CanRetry));
        assert_eq!(d.action_text, t.action_text(RecoveryVerdict::CanRetry));
        assert_eq!(
            d.destination.as_ref().expect("destination").note.as_deref(),
            Some(t.loc.format(keys::RECOVERY_DETAIL_FOLDER_MISSING, &[]).as_str())
        );
        assert!(d.source.as_ref().expect("source").note.is_none());
        assert!(d.source_visible);
        assert!(d.destination_visible);
        assert!(!d.no_destination_visible);
    });
}

#[test]
fn details_checked_without_destination() {
    // C#: RecoveryWindowPresenterTests.Details_CheckedWithoutDestination
    let e = entry_with("1", FileOperationType::Recycle, None, None);
    let mut row = RecoveryRowModel::new(e.clone());
    row.set_result(Some(check(&e, RecoveryVerdict::NotRecycled)));
    with_texts(|t| {
        let d = RecoveryWindowPresenter::details(&row, t);
        assert!(!d.destination_visible);
        assert!(d.no_destination_visible);
        assert_eq!(d.verdict_tone, RecoveryTone::Warn);
    });
}

#[test]
fn details_checked_group() {
    // C#: RecoveryWindowPresenterTests.Details_CheckedGroup
    let m1 = member(r"C:\a\1.jpg", Some(r"D:\b\1.jpg"), false);
    let m2 = member(r"C:\a\2.jpg", None, false);
    let mut e = entry_with("g", FileOperationType::Move, Some(r"C:\a\1.jpg"), Some(r"D:\b\1.jpg"));
    e.group_members = Some(vec![m1.clone(), m2.clone()]);
    let dest1 = RecoveryPathCheck::new(r"D:\b\1.jpg", RecoveryPathStatus::Missing, None, None, true);
    let checks = vec![
        RecoveryGroupMemberCheck {
            member: m1.clone(),
            check: check_with(&e, RecoveryVerdict::CanRetry, Some(dest1), None),
        },
        RecoveryGroupMemberCheck {
            member: m2.clone(),
            check: check(&e, RecoveryVerdict::AlreadyDone),
        },
    ];
    let mut row = RecoveryRowModel::new(e.clone());
    row.set_result(Some(check_with(&e, RecoveryVerdict::CanRetry, None, Some(checks))));
    with_texts(|t| {
        let d = RecoveryWindowPresenter::details(&row, t);
        assert_eq!(
            d.group_heading.as_deref(),
            Some(
                t.loc
                    .format(keys::RECOVERY_GROUP_MEMBERS, &[LocArg::new("count", 2i64)])
                    .as_str()
            )
        );
        let members = d.group_members.expect("members");
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].source, r"C:\a\1.jpg");
        assert_eq!(members[0].destination, r"D:\b\1.jpg");
        assert_eq!(
            members[1].destination,
            t.loc.format(keys::RECOVERY_DETAIL_NO_DESTINATION, &[])
        );
        assert!(!d.source_visible);
        assert!(!d.destination_visible);
        assert!(!d.no_destination_visible);
    });
}

// ---- live check ----

#[test]
fn run_checks_reports_every_row_and_writes_nothing() {
    // C#: RecoveryWindowPresenterTests.RunChecks_ReportsEveryRow_AndWritesNothing
    let temp = TempRoot::new("wp35g0-recovery");
    let shots = temp.join("shots");
    std::fs::create_dir_all(&shots).expect("mkdir");
    let keep = temp.join("keep");
    std::fs::create_dir_all(&keep).expect("mkdir");
    let source = shots.join("retry.jpg");
    std::fs::write(&source, [1u8, 2, 3]).expect("write");
    let listing = |root: &std::path::Path| {
        let mut all = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for item in std::fs::read_dir(&dir).expect("read_dir") {
                let path = item.expect("item").path();
                if path.is_dir() {
                    stack.push(path.clone());
                }
                all.push(path);
            }
        }
        all.sort();
        all
    };
    let before = listing(temp.path());
    let stat = PhysicalFileSystem.get_file_stat(&source).expect("stat");
    let mut first = entry_with(
        "1",
        FileOperationType::Move,
        Some(&source.to_string_lossy()),
        Some(&keep.join("retry.jpg").to_string_lossy()),
    );
    first.size = 3;
    first.last_write_utc = JournalDateTime::utc(stat.last_write_utc);
    let second = entry_with(
        "2",
        FileOperationType::Move,
        Some(&temp.join("nothing.jpg").to_string_lossy()),
        Some(&keep.join("nothing.jpg").to_string_lossy()),
    );
    let checker: Arc<dyn RecoveryChecker> = Arc::new(RecoveryFileCheck::new(Arc::new(PhysicalFileSystem)));
    let mut p = RecoveryWindowPresenter::new(vec![first, second], options(checker));
    let mut view = FakeView::default();

    with_texts(|t| p.run_checks_blocking(&mut view, t));

    assert_eq!(p.rows()[0].check().expect("checked").verdict, RecoveryVerdict::CanRetry);
    assert!(p.rows()[1].check().is_some());
    assert_eq!(view.recheck(), vec![false, true]);
    let english = loc();
    let progress = |done: i64| {
        english.format(
            keys::RECOVERY_CHECK_PROGRESS,
            &[LocArg::new("done", done), LocArg::new("total", 2i64)],
        )
    };
    let status = view.status();
    assert_eq!(status.len(), 4);
    assert_eq!(status[0], progress(0));
    assert_eq!(status[1], progress(1));
    assert_eq!(status[2], progress(2));
    assert_eq!(
        status[3],
        english.format(
            keys::RECOVERY_CHECK_DONE,
            &[LocArg::new("total", 2i64), LocArg::new("time", "12:34:56")]
        )
    );
    assert_eq!(view.completed(), 1);
    assert_eq!(listing(temp.path()), before);
}

#[test]
fn run_checks_no_rows() {
    // C#: RecoveryWindowPresenterTests.RunChecks_NoRows
    let mut p = presenter(vec![]);
    let mut view = FakeView::default();
    with_texts(|t| p.run_checks_blocking(&mut view, t));
    assert_eq!(view.calls, vec![Call::Status(String::new())]);
}

#[test]
fn run_checks_resets_rows_first() {
    // C#: RecoveryWindowPresenterTests.RunChecks_ResetsRowsFirst
    let mut p = presenter_with_verdicts(vec![(entry("1"), RecoveryVerdict::Lost)], plain_options());
    assert!(p.rows()[0].check().is_some());
    let mut view = FakeView::default();
    let run = with_texts(|t| p.begin_checks(&mut view, t)).expect("run");
    // The reset happened before the worker even started: the view was told and the row reads "checking".
    assert_eq!(view.rows_changed(), vec![0]);
    assert!(p.rows()[0].check().is_none());
    assert_eq!(view.calls[0], Call::Row(0));
    drop(run);
}

#[test]
fn run_checks_cancelled_run_failure_does_not_touch_the_view() {
    // C#: RecoveryWindowPresenterTests.RunChecks_CancelledRunFailure_DoesNotTouchTheView
    let mut checker = FakeChecker::new(RecoveryVerdict::CanRetry);
    checker.fail_on_call = Some(1);
    let mut p = RecoveryWindowPresenter::new(vec![entry("1")], options(Arc::new(checker)));
    let mut view = FakeView::default();
    let run = with_texts(|t| p.begin_checks(&mut view, t)).expect("run");
    let queue = RefCell::new(Vec::new());
    run.run(&|m| queue.borrow_mut().push(m));
    assert!(matches!(queue.borrow()[0].event, RecoveryCheckEvent::Failed { .. }));
    p.cancel_check();

    let before = view.calls.clone();
    for m in queue.into_inner() {
        with_texts(|t| p.on_check_message(m, &mut view, t));
    }

    assert_eq!(view.calls, before);
    assert_eq!(view.recheck(), vec![false]);
    assert!(!view.status().contains(&String::new()));
    assert_eq!(view.completed(), 0);
}

#[test]
fn run_checks_late_result_of_cancelled_run_is_ignored() {
    // C#: RecoveryWindowPresenterTests.RunChecks_LateResultOfCancelledRun_IsIgnored
    let mut p = presenter(vec![entry("1"), entry("2")]);
    let mut view = FakeView::default();
    let run = with_texts(|t| p.begin_checks(&mut view, t)).expect("run");
    let queue = RefCell::new(Vec::new());
    run.run(&|m| queue.borrow_mut().push(m)); // the worker has posted both results and Finished; none delivered yet
    assert_eq!(queue.borrow().len(), 3);
    p.cancel_check();

    for m in queue.into_inner() {
        with_texts(|t| p.on_check_message(m, &mut view, t));
    }

    assert!(p.rows().iter().all(|r| r.check().is_none()));
    assert_eq!(view.status().len(), 1); // only the initial "0/2" line
    assert_eq!(view.completed(), 0);
    assert_eq!(view.recheck(), vec![false]);
}

// ---- dismiss ----

#[test]
fn dismiss_prompt_needs_delegate_and_rows() {
    // C#: RecoveryWindowPresenterTests.Dismiss_PromptNeedsDelegateAndRows
    let english = loc();
    let without = presenter(vec![entry("1")]);
    assert_eq!(without.dismiss_prompt(&[0], &english), None);

    let mut opts = plain_options();
    opts.dismiss = Some(dismiss_all());
    let p = RecoveryWindowPresenter::new(vec![entry("1"), entry("2")], opts);
    assert_eq!(p.dismiss_prompt(&[], &english), None);
    assert_eq!(
        p.dismiss_prompt(&[0, 1], &english),
        Some(english.format(keys::RECOVERY_DISMISS_CONFIRM_MESSAGE, &[LocArg::new("count", 2i64)]))
    );
}

#[test]
fn dismiss_removes_dismissed_and_stale() {
    // C#: RecoveryWindowPresenterTests.Dismiss_RemovesDismissedAndStale
    let received: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let sink = received.clone();
    let mut opts = plain_options();
    opts.dismiss = Some(Box::new(move |entries| {
        *sink.lock().expect("lock") = entries.iter().map(|e| e.id.clone()).collect();
        Ok(DismissOutcome {
            dismissed: vec![entries[0].clone()],
            skipped: vec![entries[1].clone()],
        })
    }));
    let mut p = RecoveryWindowPresenter::new(vec![entry("1"), entry("2"), entry("3")], opts);

    let result = p.dismiss(&[0, 1]);

    assert_eq!(result.error, None);
    assert_eq!(result.skipped_count, 1);
    assert_eq!(*received.lock().expect("lock"), vec!["1".to_owned(), "2".to_owned()]);
    assert_eq!(ids(&p), vec!["3".to_owned()]);
}

#[test]
fn dismiss_throws_removes_nothing() {
    // C#: RecoveryWindowPresenterTests.Dismiss_Throws_RemovesNothing
    let mut opts = plain_options();
    opts.dismiss = Some(Box::new(|_| Err("journal locked".to_owned())));
    let mut p = RecoveryWindowPresenter::new(vec![entry("1")], opts);
    let result = p.dismiss(&[0]);
    assert_eq!(result.error.as_deref(), Some("journal locked"));
    assert_eq!(result.skipped_count, 0);
    assert_eq!(p.rows().len(), 1);
}

#[test]
fn dismiss_keeps_rows_not_requested() {
    // New test
    let called = Arc::new(AtomicUsize::new(0));
    let counter = called.clone();
    let mut opts = plain_options();
    opts.dismiss = Some(Box::new(move |entries| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(DismissOutcome {
            dismissed: entries.to_vec(),
            skipped: vec![],
        })
    }));
    let mut p = RecoveryWindowPresenter::new(vec![entry("1"), entry("2")], opts);
    // No rows requested: the delegate is not called at all.
    let none = p.dismiss(&[]);
    assert_eq!((none.error, none.skipped_count), (None, 0));
    assert_eq!(called.load(Ordering::SeqCst), 0);
    // Only the requested row goes.
    p.dismiss(&[1]);
    assert_eq!(ids(&p), vec!["1".to_owned()]);
    // Ids are compared ordinally: a differently-cased id is a different row.
    let mut opts = plain_options();
    opts.dismiss = Some(dismiss_all());
    let mut q = RecoveryWindowPresenter::new(vec![entry("ab"), entry("AB")], opts);
    q.dismiss(&[0]);
    assert_eq!(ids(&q), vec!["AB".to_owned()]);
}

// ---- retry ----

#[test]
fn retry_without_delegate_throws() {
    // C#: RecoveryWindowPresenterTests.Retry_WithoutDelegate_Throws
    let mut p = presenter(vec![entry("1")]);
    let e = p.rows()[0].entry().clone();
    let busy = RefCell::new(Vec::new());
    let outcome = p.begin_retry(&e, &mut |b| busy.borrow_mut().push(b));
    assert!(outcome.is_err());
    assert!(!p.is_retrying());
    assert!(busy.borrow().is_empty());
}

#[test]
fn retry_throws_releases_busy_state() {
    // C#: RecoveryWindowPresenterTests.Retry_Throws_ReleasesBusyState
    let mut opts = plain_options();
    opts.retry = Some(retry_fn(|_| Err("nope".to_owned())));
    let mut p = RecoveryWindowPresenter::new(vec![entry("1")], opts);
    let e = p.rows()[0].entry().clone();
    let busy = RefCell::new(Vec::new());
    let job = p.begin_retry(&e, &mut |b| busy.borrow_mut().push(b)).expect("job");
    let outcome = job.run();
    assert_eq!(outcome, Err("nope".to_owned()));

    let result = p.finish_retry(&e.id, outcome, &mut |b| busy.borrow_mut().push(b), &mut || {});

    assert_eq!(result, Err("nope".to_owned()));
    assert!(!p.is_retrying());
    assert_eq!(*busy.borrow(), vec![true, false]);
    assert_eq!(p.rows().len(), 1);
}

#[test]
fn retry_superseded_drops_the_row() {
    // C#: RecoveryWindowPresenterTests.Retry_Superseded_DropsTheRow
    let mut opts = plain_options();
    opts.retry = Some(retry_fn(|_| {
        let mut r = RecoveryRetryResult::new(false, "someone else did it", None);
        r.superseded = true;
        Ok(r)
    }));
    let mut p = RecoveryWindowPresenter::new(vec![entry("1"), entry("2")], opts);
    let e = p.rows()[0].entry().clone();
    let changed = RefCell::new(0);
    let job = p.begin_retry(&e, &mut |_| {}).expect("job");
    let result = p
        .finish_retry(&e.id, job.run(), &mut |_| {}, &mut || *changed.borrow_mut() += 1)
        .expect("result");
    assert!(result.superseded);
    assert_eq!(*changed.borrow(), 1);
    assert_eq!(ids(&p), vec!["2".to_owned()]);

    let mut opts = plain_options();
    opts.retry = Some(retry_fn(|_| Ok(ok_result())));
    let mut normal = RecoveryWindowPresenter::new(vec![entry("1")], opts);
    let e = normal.rows()[0].entry().clone();
    *changed.borrow_mut() = 0;
    let job = normal.begin_retry(&e, &mut |_| {}).expect("job");
    normal
        .finish_retry(&e.id, job.run(), &mut |_| {}, &mut || *changed.borrow_mut() += 1)
        .expect("result");
    assert_eq!(*changed.borrow(), 0);
    assert_eq!(normal.rows().len(), 1);
}

#[test]
fn describe_retry_maps_every_outcome() {
    // C#: RecoveryWindowPresenterTests.DescribeRetry_MapsEveryOutcome
    let english = loc();
    let title = |key: &str| english.format(key, &[]);

    let success = RecoveryWindowPresenter::describe_retry(&ok_result(), &english);
    assert_eq!(success.title, title(keys::DIALOG_RETRY_SUCCEEDED_TITLE));
    assert!(!success.is_warning);
    assert!(success.close_window);
    assert!(!success.recheck);

    let journal_warning = RecoveryWindowPresenter::describe_retry(
        &RecoveryRetryResult::not_journaled(true, "moved", None, "disk full"),
        &english,
    );
    assert_eq!(
        journal_warning.title,
        title(keys::DIALOG_RETRY_DONE_JOURNAL_FAILED_TITLE)
    );
    assert!(journal_warning.is_warning);
    assert!(journal_warning.close_window);

    let refused = RecoveryWindowPresenter::describe_retry(&RecoveryRetryResult::new(false, "conflict", None), &english);
    assert_eq!(refused.title, title(keys::DIALOG_RETRY_REJECTED_TITLE));
    assert!(refused.is_warning);
    assert!(!refused.close_window);
    assert!(refused.recheck);

    let mut gone = RecoveryRetryResult::new(false, "gone", None);
    gone.superseded = true;
    let stale = RecoveryWindowPresenter::describe_retry(&gone, &english);
    assert_eq!(stale.title, title(keys::RECOVERY_RETRY_STALE_TITLE));
    assert!(!stale.is_warning);
    assert!(!stale.close_window);
    assert!(!stale.recheck);
    assert_eq!(stale.message, "gone");
}

#[test]
fn retry_confirm_reads_setting_lazily() {
    // C#: RecoveryWindowPresenterTests.RetryConfirm_ReadsSettingLazily
    let allow = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = allow.clone();
    let setting: AllowPermanentDelete = Arc::new(move || flag.load(Ordering::SeqCst));
    let mut e = entry_with("g", FileOperationType::Recycle, Some(r"C:\a\1.jpg"), None);
    e.state = JournalState::Prepared;
    e.group_members = Some(vec![member(r"C:\a\1.jpg", None, true)]);
    let mut opts = plain_options();
    opts.allow_permanent_delete = Some(setting);
    let p = RecoveryWindowPresenter::new(vec![e], opts);

    with_texts(|t| {
        let before = p.retry_confirm_text(0, t);
        allow.store(true, Ordering::SeqCst);
        let after = p.retry_confirm_text(0, t);
        let op = t.operation_text(FileOperationType::Recycle);
        assert_eq!(
            before,
            t.loc.format(
                keys::RECOVERY_GROUP_RETRY_CONFIRM,
                &[LocArg::new("operation", op.clone()), LocArg::new("count", 1i64)]
            )
        );
        assert_eq!(
            after,
            t.loc.format(
                keys::RECOVERY_GROUP_RETRY_CONFIRM_PERMANENT,
                &[
                    LocArg::new("operation", op),
                    LocArg::new("count", 1i64),
                    LocArg::new("permanentCount", 1i64)
                ]
            )
        );
    });
}

#[test]
fn retry_confirm_cancelled_undo_group_has_no_permanent_warning() {
    // C#: RecoveryWindowPresenterTests.RetryConfirm_CancelledUndoGroup_HasNoPermanentWarning
    let mut e = entry_with("g", FileOperationType::Recycle, Some(r"C:\a\1.jpg"), None);
    e.error_code = Some(pr_core::file_actions::errors::CANCELLED_BY_USER.to_owned());
    e.undo = Some(true);
    e.group_members = Some(vec![member(r"C:\a\1.jpg", None, true)]);
    with_texts(|t| {
        let text = t.retry_confirm_text(&e, None, true);
        assert_eq!(
            text,
            t.loc.format(
                keys::RECOVERY_RETRY_CONFIRM_CANCELLED,
                &[
                    LocArg::new("operation", t.operation_text(FileOperationType::Recycle)),
                    LocArg::new("count", 1i64)
                ]
            )
        );
    });
}

#[test]
fn tones_have_distinct_documented_colours() {
    // C#: RecoveryWindowPresenterTests.Tones_HaveDistinctDocumentedColours
    assert_eq!(rgb(RecoveryTone::Good), (0x6C, 0xCB, 0x7A));
    assert_eq!(rgb(RecoveryTone::Neutral), (0xB0, 0xB0, 0xB0));
    let all = [
        RecoveryTone::Neutral,
        RecoveryTone::Good,
        RecoveryTone::Info,
        RecoveryTone::Warn,
        RecoveryTone::Bad,
    ];
    let colours: std::collections::HashSet<_> = all.iter().map(|t| rgb(*t)).collect();
    assert_eq!(colours.len(), all.len());
}

// ---- new tests ----

#[test]
fn late_result_of_an_earlier_run_is_dropped_during_a_new_run() {
    // New test (T2): the token of the MESSAGE decides, not the presenter's current one.
    let mut p = presenter(vec![entry("1")]);
    let mut view = FakeView::default();
    let run1 = with_texts(|t| p.begin_checks(&mut view, t)).expect("run 1");
    let old = RefCell::new(Vec::new());
    run1.run(&|m| old.borrow_mut().push(m));
    let _run2 = with_texts(|t| p.begin_checks(&mut view, t)).expect("run 2");
    let before = view.calls.clone();

    for m in old.into_inner() {
        with_texts(|t| p.on_check_message(m, &mut view, t));
    }

    assert_eq!(view.calls, before);
    assert!(p.rows()[0].check().is_none());
}

#[test]
fn new_run_cancels_the_previous_one() {
    // New test
    let mut p = presenter(vec![entry("1"), entry("2")]);
    let mut view = FakeView::default();
    let run1 = with_texts(|t| p.begin_checks(&mut view, t)).expect("run 1");
    let run2 = with_texts(|t| p.begin_checks(&mut view, t)).expect("run 2");
    // A cancelled run posts nothing at all.
    let posted = RefCell::new(0usize);
    run1.run(&|_| *posted.borrow_mut() += 1);
    assert_eq!(*posted.borrow(), 0);
    // The newer run is live and completes.
    let queue = RefCell::new(Vec::new());
    run2.run(&|m| queue.borrow_mut().push(m));
    for m in queue.into_inner() {
        with_texts(|t| p.on_check_message(m, &mut view, t));
    }
    assert_eq!(view.completed(), 1);
    assert!(p.rows().iter().all(|r| r.check().is_some()));
}

#[test]
fn result_for_a_cleared_row_still_counts_progress() {
    // New test (T1): results are keyed by Id; a row cleared meanwhile is skipped but still counts.
    let mut opts = plain_options();
    opts.dismiss = Some(dismiss_all());
    let mut p = RecoveryWindowPresenter::new(vec![entry("1"), entry("2")], opts);
    let mut view = FakeView::default();
    let run = with_texts(|t| p.begin_checks(&mut view, t)).expect("run");
    let queue = RefCell::new(Vec::new());
    run.run(&|m| queue.borrow_mut().push(m));
    p.dismiss(&[0]);
    for m in queue.into_inner() {
        with_texts(|t| p.on_check_message(m, &mut view, t));
    }
    assert_eq!(ids(&p), vec!["2".to_owned()]);
    assert!(p.rows()[0].check().is_some());
    let english = loc();
    assert_eq!(
        view.status()[2],
        english.format(
            keys::RECOVERY_CHECK_PROGRESS,
            &[LocArg::new("done", 2i64), LocArg::new("total", 2i64)]
        )
    );
    assert_eq!(view.completed(), 1);
}

#[test]
fn checker_panic_becomes_failed_and_reenables_recheck() {
    // New test (T7)
    let mut checker = FakeChecker::new(RecoveryVerdict::CanRetry);
    checker.panic_on_call = Some(1);
    let mut p = RecoveryWindowPresenter::new(vec![entry("1"), entry("2")], options(Arc::new(checker)));
    let mut view = FakeView::default();
    with_texts(|t| p.run_checks_blocking(&mut view, t));
    assert_eq!(view.recheck(), vec![false, true]);
    assert_eq!(view.status().last().map(String::as_str), Some(""));
    assert_eq!(view.completed(), 0);
    assert!(p.rows().iter().all(|r| r.check().is_none()));
}

#[test]
fn retry_job_panic_becomes_err_and_busy_is_released() {
    // New test (T7)
    let mut opts = plain_options();
    opts.retry = Some(retry_fn(|_| panic!("retry blew up")));
    let mut p = RecoveryWindowPresenter::new(vec![entry("1")], opts);
    let e = p.rows()[0].entry().clone();
    let busy = RefCell::new(Vec::new());
    let job = p.begin_retry(&e, &mut |b| busy.borrow_mut().push(b)).expect("job");
    let outcome = job.run();
    assert!(outcome.as_ref().is_err_and(|m| m.contains("retry blew up")));
    let result = p.finish_retry(&e.id, outcome, &mut |b| busy.borrow_mut().push(b), &mut || {});
    assert!(result.is_err());
    assert!(!p.is_retrying());
    assert_eq!(*busy.borrow(), vec![true, false]);
}

#[test]
fn summary_text_uses_plural_forms() {
    // New test (T11)
    let english = loc();
    let one = presenter(vec![entry("1")]).summary_text(&english);
    let two = presenter(vec![entry("1"), entry("2")]).summary_text(&english);
    assert!(one.starts_with("1 pending/failed operation."), "{one}");
    assert!(two.starts_with("2 pending/failed operations."), "{two}");
}

#[test]
fn check_message_event_shapes() {
    // New test: the worker reports Result per entry then Finished, in order.
    let mut p = presenter(vec![entry("1")]);
    let mut view = FakeView::default();
    let run = with_texts(|t| p.begin_checks(&mut view, t)).expect("run");
    let queue: RefCell<Vec<RecoveryCheckMessage>> = RefCell::new(Vec::new());
    run.run(&|m| queue.borrow_mut().push(m));
    let events: Vec<RecoveryCheckEvent> = queue.into_inner().into_iter().map(|m| m.event).collect();
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], RecoveryCheckEvent::Result { id, .. } if id == "1"));
    assert_eq!(events[1], RecoveryCheckEvent::Finished);
}
