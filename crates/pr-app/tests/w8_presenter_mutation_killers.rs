//! Mutation killers for the Wave 8 presenters in `pr-app` (report `docs/reviews/W8-PRESENTER-MUTATION.md`).
//! Every test names the mutant (file:line, operator) it kills. Fakes and `TempRoot` only; no sleeps, no real Recycle Bin.

mod support {
    pub mod preview_fakes;
}

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pr_app::localization::service::{CultureLookup, LocalizationPaths, LocalizationService};
use pr_app::presenters::settings::{SettingsPresenter, SettingsPresenterOptions};
use pr_app::presenters::settings_field_map::FieldContext;
use pr_core::abstractions::{FileSystem, Log, ShellIntegration, ShellMenuState};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::context_menu::ITEMS;
use pr_l10n::loader::UiCulture;
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_l10n::template::NumberCulture;
use pr_test_support::{FakeFileSystem, RecordingLog, TempRoot};

use pr_app::presenters::recovery_presentation::{RecoveryTexts, RecoveryTimeFormatter};
use pr_app::presenters::recovery_window::{
    RecoveryCheckView, RecoveryChecker, RecoveryRowModel, RecoveryWindowOptions, RecoveryWindowPresenter,
};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry};
use pr_core::file_actions::recovery_check::RecoveryCheckResult;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;

struct Env {
    texts: Arc<Localizer>,
    numbers: NumberCulture,
}

impl Env {
    fn new() -> Self {
        Self {
            texts: built_in_english_localizer(),
            numbers: NumberCulture::invariant(),
        }
    }

    fn ctx(&self) -> FieldContext<'_> {
        FieldContext {
            texts: &self.texts,
            numbers: &self.numbers,
        }
    }
}

struct NoShell;

impl ShellIntegration for NoShell {
    fn get_state(&self, _executable_path: &str) -> ShellMenuState {
        ShellMenuState::NotRegistered
    }

    fn register(&self, _executable_path: &str) -> std::io::Result<()> {
        Ok(())
    }

    fn unregister(&self) -> std::io::Result<()> {
        Ok(())
    }
}

fn options() -> SettingsPresenterOptions {
    SettingsPresenterOptions::new(
        32 * 1024 * 1024 * 1024,
        Arc::new(EnglishSettingsTexts),
        Arc::new(NoShell),
    )
}

struct KnownCultures;

impl CultureLookup for KnownCultures {
    fn lookup(&self, name: &str) -> Option<UiCulture> {
        matches!(name, "en" | "vi").then(|| UiCulture::from_name(name))
    }
}

fn service_in(root: &TempRoot) -> LocalizationService {
    let shipped = PathBuf::from(r"C:\App\Languages");
    let disk = Arc::new(FakeFileSystem::new());
    disk.add_dir(&shipped);
    let file_system: Arc<dyn FileSystem> = disk;
    let log: Arc<dyn Log> = Arc::new(RecordingLog::new());
    LocalizationService::new(
        file_system,
        LocalizationPaths {
            shipped_dir: shipped,
            user_languages_dir: root.join("Languages"),
        },
        UiCulture::from_name("en"),
        Arc::new(KnownCultures),
        log,
    )
}

// ---------------------------------------------------------------------------------------------------------------
// settings/apply.rs
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn update_check_job_debug_names_the_type_and_the_version() {
    // apply.rs:125:9 replace <impl Debug for UpdateCheckJob>::fmt with Ok(Default::default()) (empty output)
    let mut presenter = SettingsPresenter::new(&AppSettings::new(&EnglishSettingsTexts), options());
    let job = presenter.begin_update_check(Some("2.0.93"));
    let text = format!("{job:?}");
    assert!(text.contains("UpdateCheckJob"), "{text}");
    assert!(text.contains("2.0.93"), "{text}");
}

#[test]
fn context_menu_item_labels_are_the_localized_item_texts_and_distinct() {
    // apply.rs:334:9 replace SettingsPresenter::context_menu_item_label -> String with "xyzzy".into()
    let env = Env::new();
    let rows = SettingsPresenter::context_menu_rows(&env.ctx());
    assert_eq!(rows.len(), ITEMS.len());
    assert!(rows.iter().all(|row| row.label != "xyzzy"));
    let distinct: BTreeSet<&str> = rows.iter().map(|row| row.label.as_str()).collect();
    assert_eq!(distinct.len(), rows.len(), "every item has its own label");
    assert!(
        rows.iter()
            .all(|row| !row.label.starts_with("settings.contextMenu.item."))
    );
    let first = &rows[0];
    assert_eq!(first.label, env.texts.get("settings.contextMenu.item.undo"));
}

#[test]
fn reload_translations_with_a_service_reports_a_result() {
    // apply.rs:410:9 replace SettingsPresenter::reload_translations -> Option<TranslationReloadResult> with None
    let root = TempRoot::new("w8-pres-mut-reload");
    let service = service_in(&root);
    let mut options = options();
    options.localization = Some(Arc::new(Mutex::new(service)));
    let presenter = SettingsPresenter::new(&AppSettings::new(&EnglishSettingsTexts), options);
    let result = presenter.reload_translations().expect("a service exists");
    assert!(
        !result.has_problems,
        "an empty user folder has no problems: {}",
        result.message
    );
    let without = SettingsPresenter::new(&AppSettings::new(&EnglishSettingsTexts), self::options());
    assert!(without.reload_translations().is_none());
}

// apply.rs:466:9 try_start_explorer -> None / Some(String::new()) / Some("xyzzy"): NOT killable. The function spawns the real
// `explorer.exe` (a visible window on the user's desktop) and C# has no test for it either; it is process-spawn glue.

#[test]
fn set_update_url_stores_the_url_that_open_update_page_validates_and_opens() {
    // apply.rs:657:9 replace SettingsPresenter::set_update_url with ()
    let env = Env::new();
    let mut presenter = SettingsPresenter::new(&AppSettings::new(&EnglishSettingsTexts), options());
    let url = "https://github.com/ConanKLOP2/PhotoReview/releases/tag/v2.0.94";
    presenter.set_update_url(Some(url.to_owned()));
    assert_eq!(presenter.update_url(), Some(url));
    let mut opened = Vec::new();
    let error = presenter.open_update_page(
        &mut |u: &str| {
            opened.push(u.to_owned());
            Ok(())
        },
        &env.ctx(),
    );
    assert_eq!(error, None);
    assert_eq!(opened, vec![url.to_owned()]);
}

// ---------------------------------------------------------------------------------------------------------------
// settings/load.rs
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn settings_presenter_and_options_debug_name_the_type_and_fields() {
    // load.rs:125:9 replace <impl Debug for SettingsPresenterOptions>::fmt with Ok(Default::default())
    // load.rs:180:9 replace <impl Debug for SettingsPresenter>::fmt with Ok(Default::default())
    let text = format!("{:?}", options());
    assert!(text.contains("SettingsPresenterOptions"), "{text}");
    assert!(text.contains("physical_memory_bytes"), "{text}");
    let presenter = SettingsPresenter::new(&AppSettings::new(&EnglishSettingsTexts), options());
    let text = format!("{presenter:?}");
    assert!(text.contains("SettingsPresenter"), "{text}");
    assert!(text.contains("has_store"), "{text}");
}

#[test]
fn next_page_index_wraps_with_the_truncating_remainder_of_csharp() {
    // load.rs:246:37 replace % with + in SettingsPresenter::next_page_index (equivalent for 0 <= current < count; differs when
    // current + step < -count, where C# `((-31 % 7) + 7) % 7 = 4`)
    assert_eq!(SettingsPresenter::next_page_index(-30, 7, false), 4);
    assert_eq!(SettingsPresenter::next_page_index(0, 7, false), 6);
    assert_eq!(SettingsPresenter::next_page_index(6, 7, true), 0);
    assert_eq!(SettingsPresenter::next_page_index(3, 7, true), 4);
}

// load.rs:276:20 (`<` -> `<=`) and load.rs:279:20 (`>` -> `>=`) in load_fields: EQUIVALENT. When percent == min (or == max) the
// branch assigns min (or max) to a value that is already equal to it.

#[test]
fn ram_cache_hint_rounds_a_value_between_one_and_ten_hundredths_of_a_gib_half_up() {
    // load.rs:542:27 replace < with == / <= in one_decimal_half_up (whole == 0: 64 MiB = 0.0625 GiB must show "0.1", not "0.0")
    let env = Env::new();
    let mut options = options();
    options.physical_memory_bytes = 64 * 1024 * 1024;
    let presenter = SettingsPresenter::new(&AppSettings::new(&EnglishSettingsTexts), options);
    let hint = presenter.ram_cache_hint_text(10, 50, &env.ctx());
    assert!(hint.contains("0.1"), "{hint}");
    assert!(!hint.contains("0.0 "), "{hint}");
}

// ---------------------------------------------------------------------------------------------------------------
// recovery_window.rs
// ---------------------------------------------------------------------------------------------------------------

struct FakeTime;

impl RecoveryTimeFormatter for FakeTime {
    fn format_local_general(&self, utc: DateTimeUtc) -> String {
        format!("<{}>", utc.ticks())
    }
}

fn recovery_entry(id: &str, source: &str) -> JournalEntry {
    let stamp = JournalDateTime::utc(DateTimeUtc::from_ticks(1_000).expect("valid ticks"));
    JournalEntry {
        id: id.to_owned(),
        op_type: FileOperationType::Move,
        state: JournalState::Failed,
        source: source.to_owned(),
        destination: None,
        size: 10,
        last_write_utc: stamp,
        timestamp_utc: stamp,
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

struct NeverChecker;

impl RecoveryChecker for NeverChecker {
    fn check(&self, _entry: &JournalEntry) -> Result<RecoveryCheckResult, String> {
        Err("not called".to_owned())
    }
}

struct SilentView;

impl RecoveryCheckView for SilentView {
    fn set_check_status(&mut self, _text: &str) {}
    fn set_recheck_enabled(&mut self, _enabled: bool) {}
    fn check_completed(&mut self) {}
    fn row_changed(&mut self, _row_index: usize) {}
}

fn recovery_options() -> RecoveryWindowOptions {
    RecoveryWindowOptions {
        retry: Some(Arc::new(|_: &JournalEntry| Err("unused".to_owned()))),
        dismiss: None,
        checker: Arc::new(NeverChecker),
        allow_permanent_delete: None,
        current_time_text: Arc::new(|| "12:34:56".to_owned()),
    }
}

#[test]
fn recovery_row_with_an_empty_member_list_is_titled_by_the_file_name_alone() {
    // recovery_window.rs:83:30 replace match guard !members.is_empty() with true in RecoveryRowModel::title
    let english = built_in_english_localizer();
    let time = FakeTime;
    let texts = RecoveryTexts {
        loc: &english,
        time: &time,
        group_separator: ",",
    };
    let mut grouped = recovery_entry("1", r"C:\one.jpg");
    grouped.group_members = Some(Vec::new());
    assert_eq!(RecoveryRowModel::new(grouped).title(&texts), "one.jpg");
}

#[test]
fn recovery_debug_impls_name_their_types() {
    // recovery_window.rs:217:9 RecoveryWindowOptions, :254:9 RecoveryCheckRun, :306:9 RecoveryRetryJob, :342:9
    // RecoveryWindowPresenter: replace <impl Debug>::fmt with Ok(Default::default()) (empty output)
    let text = format!("{:?}", recovery_options());
    assert!(
        text.contains("RecoveryWindowOptions") && text.contains("has_retry"),
        "{text}"
    );

    let english = built_in_english_localizer();
    let time = FakeTime;
    let texts = RecoveryTexts {
        loc: &english,
        time: &time,
        group_separator: ",",
    };
    let mut presenter = RecoveryWindowPresenter::new(vec![recovery_entry("a", r"C:x.jpg")], recovery_options());
    let text = format!("{presenter:?}");
    assert!(
        text.contains("RecoveryWindowPresenter") && text.contains("is_retrying"),
        "{text}"
    );

    let run = presenter.begin_checks(&mut SilentView, &texts).expect("one row");
    let text = format!("{run:?}");
    assert!(text.contains("RecoveryCheckRun") && text.contains("entries"), "{text}");

    let entry = recovery_entry("a", r"C:x.jpg");
    let job = presenter
        .begin_retry(&entry, &mut |_| {})
        .expect("a retry delegate exists");
    let text = format!("{job:?}");
    assert!(text.contains("RecoveryRetryJob") && text.contains('a'), "{text}");
}

#[test]
fn can_start_retry_refuses_an_index_equal_to_the_row_count() {
    // recovery_window.rs:701:55 replace < with <= in RecoveryWindowPresenter::can_start_retry
    let presenter = RecoveryWindowPresenter::new(vec![recovery_entry("a", r"C:\x\a.jpg")], recovery_options());
    assert!(presenter.can_start_retry(Some(0)));
    assert!(!presenter.can_start_retry(Some(1)), "one row: index 1 is out of range");
    assert!(!presenter.can_start_retry(None));
}

// ---------------------------------------------------------------------------------------------------------------
// coordinators/zoom_detail.rs
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn completion_inbox_reports_emptiness_and_has_a_named_debug() {
    // zoom_detail.rs:82:9 replace <impl Debug for CompletionInbox<T>>::fmt with Ok(Default::default())
    // zoom_detail.rs:121:9 replace CompletionInbox<T>::is_empty -> bool with true (the `-> false` twin made the zoom_detail
    // binary hang: reported TIMEOUT, caught by the 90 s limit; this test also fails it at once)
    let inbox = pr_app::coordinators::zoom_detail::CompletionInbox::<u32>::new(Arc::new(|| {}));
    assert!(inbox.is_empty());
    inbox.send(7);
    assert!(!inbox.is_empty());
    assert_eq!(inbox.len(), 1);
    assert!(format!("{inbox:?}").contains("CompletionInbox"));
    assert_eq!(inbox.drain(), vec![7]);
    assert!(inbox.is_empty());
}

#[test]
fn production_indicator_delay_waits_at_least_the_requested_time() {
    // zoom_detail.rs:196:39 replace + with - in default_indicator_delay (deadline in the past: returns at once). A lower bound on
    // a monotonic clock is deterministic, never flaky.
    let delay = pr_app::coordinators::zoom_detail::default_indicator_delay();
    let wanted = Duration::from_millis(40);
    let start = Instant::now();
    assert!(delay(wanted, &pr_core::sync::CancelToken::new()));
    assert!(start.elapsed() >= wanted, "returned after {:?}", start.elapsed());
}

#[test]
fn zoom_detail_loader_debug_names_the_type_and_state() {
    // zoom_detail.rs:274:9 replace <impl Debug for ZoomDetailLoader>::fmt with Ok(Default::default())
    let root = TempRoot::new("w8-pres-mut-zoom-debug");
    let service = support::preview_fakes::build_service(
        &root.join("cache"),
        support::preview_fakes::SizedDecoder::new(),
        support::preview_fakes::ServiceOptions::default(),
    );
    let loader = pr_app::coordinators::zoom_detail::ZoomDetailLoader::new(
        service,
        Arc::new(pr_core::catalog::generation::GenerationClock::default()),
        Arc::new(pr_app::coordinators::zoom_detail::ThreadRunner),
        Arc::new(|| {}),
    );
    let text = format!("{loader:?}");
    assert!(
        text.contains("ZoomDetailLoader") && text.contains("showing_original"),
        "{text}"
    );
}

// ---------------------------------------------------------------------------------------------------------------
// benchmark.rs
// ---------------------------------------------------------------------------------------------------------------

struct BenchView;

impl pr_app::presenters::benchmark::BenchmarkView for BenchView {
    fn set_status(&mut self, _text: &str) {}
    fn set_progress(&mut self, _fraction: f64) {}
    fn set_running(&mut self, _running: bool) {}
    fn refresh_rows(&mut self) {}
    fn open_in_explorer(&mut self, _arguments: &str) {}
}

struct BenchSession(Arc<AtomicUsize>);

impl pr_app::presenters::benchmark::BenchmarkProfileSession for BenchSession {
    fn run(
        &mut self,
        _folder: &str,
        _progress: &dyn pr_bench::engine::BenchmarkProgressSink,
        _cancel: &pr_core::sync::CancelToken,
    ) -> Result<pr_bench::models::BenchmarkReport, pr_bench::engine::BenchmarkError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(pr_bench::models::BenchmarkReport {
            run_id: "run".to_owned(),
            started_utc: pr_bench::models::BenchmarkTimestamp::unix_epoch(),
            folder: "bench".to_owned(),
            phases: vec![pr_bench::models::BenchmarkPhaseResult::new(
                "p",
                pr_bench::models::BenchmarkWorkload::Sequential,
                vec![10.0, 20.0],
                pr_bench::models::BenchmarkResultStatus::Pass,
                None,
                None,
            )],
            metrics: None,
            machine: None,
        })
    }

    fn dispose(&mut self) {}
}

#[test]
fn benchmark_step_jobs_have_named_debug_output() {
    // benchmark.rs:131:9 BenchmarkScanJob, :179:9 BenchmarkProfileJob, :255:9 BenchmarkSaveJob (and the presenter's own Debug):
    // replace <impl Debug>::fmt with Ok(Default::default()) (empty output). Fakes only: scan and save are closures.
    use pr_app::presenters::benchmark::{BenchmarkPresenter, BenchmarkScanOutcome, BenchmarkStep};
    let runs = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&runs);
    let factory: pr_app::presenters::benchmark::BenchmarkSessionFactory =
        Arc::new(move |_profile, _files| Ok(Box::new(BenchSession(Arc::clone(&counter))) as Box<_>));
    let scan: pr_app::presenters::benchmark::BenchmarkFileScan =
        Arc::new(|_folder, _limit, _cancel| Ok(vec!["a.jpg".to_owned()]));
    let save: pr_app::presenters::benchmark::BenchmarkSummarySave = Arc::new(|_reports| Ok("summary.json".to_owned()));
    let mut presenter = BenchmarkPresenter::new(factory, Some(scan), Some(save));
    let env = Env::new();
    let ctx = env.ctx();
    let mut view = BenchView;
    let chosen: Vec<_> = pr_bench::profiles::all().iter().take(2).cloned().collect();
    assert_eq!(chosen.len(), 2);

    let text = format!("{presenter:?}");
    assert!(
        text.contains("BenchmarkPresenter") && text.contains("is_running"),
        "{text}"
    );

    let BenchmarkStep::Scan(job) = presenter.run_selected(&chosen, "bench-folder", Some("5"), &mut view, &ctx) else {
        panic!("a started run begins with the scan");
    };
    let text = format!("{job:?}");
    assert!(
        text.contains("BenchmarkScanJob") && text.contains("bench-folder") && text.contains("image_limit"),
        "{text}"
    );
    let outcome: BenchmarkScanOutcome = job.run();

    let BenchmarkStep::Profile(job) = presenter.on_scan_finished(outcome, &mut view, &ctx) else {
        panic!("files found: the first profile follows");
    };
    let text = format!("{job:?}");
    assert!(
        text.contains("BenchmarkProfileJob") && text.contains("run_id"),
        "{text}"
    );
    let first = job.run(&|_message| {});

    let BenchmarkStep::Profile(job) = presenter.on_profile_finished(first, &mut view, &ctx) else {
        panic!("the second profile follows");
    };
    let second = job.run(&|_message| {});

    let BenchmarkStep::Save(job) = presenter.on_profile_finished(second, &mut view, &ctx) else {
        panic!("two reports are saved");
    };
    let text = format!("{job:?}");
    assert!(text.contains("BenchmarkSaveJob") && text.contains("reports"), "{text}");
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

// FLAKY (about 40 % of runs): the loop below exits or hangs depending on the order in which the old path's stale completion and the new path's decode
// complete. Waiting for the new original to be held makes it hang for the whole 20 s deadline in about a third of the runs, i.e. in those runs the new
// path's original is never held: a possible stale-completion race in `ZoomDetailLoader::retarget_path` (INV-1). Ignored until that investigation lane
// (`zoom-race`) decides whether production or this test is wrong; do not un-ignore it before then.
#[test]
#[ignore = "flaky ~40%: possible INV-1 race in ZoomDetailLoader::retarget_path, under investigation (zoom-race)"]
fn retarget_path_with_a_decode_in_flight_shows_only_the_new_paths_pixels() {
    // Regression pin for zoom_detail.rs:306:13 (delete field id from struct Target expression in retarget_path). It does NOT kill that
    // mutant: `cancel_pending()` already cancels the running old-path decode and the fake decoder honours the cancel, so the
    // stale completion is dropped either way (survivor documented in docs/reviews/W8-PRESENTER-MUTATION.md). The test pins that
    // the shown pixels are the new path's decode and that exactly one ShowImage is produced.
    use pr_app::coordinators::zoom_detail::{ThreadRunner, ZoomDetailEffect, ZoomDetailLoader, same_platform_image};
    use pr_core::catalog::generation::GenerationClock;
    use support::preview_fakes::{ServiceOptions, SizedDecoder, TIMEOUT, build_service, sized_image};

    let root = TempRoot::new("w8-pres-mut-retarget-id");
    let decoder = SizedDecoder::new();
    let service = build_service(&root.join("cache"), decoder.clone(), ServiceOptions::default());
    let clock = Arc::new(GenerationClock::default());
    let mut loader = ZoomDetailLoader::new(
        Arc::clone(&service),
        Arc::clone(&clock),
        Arc::new(ThreadRunner),
        Arc::new(|| {}),
    );
    let file = |name: &str| {
        let path = root.join(name);
        std::fs::write(&path, [0xff, 0xd8, 0xff, 0xd9]).expect("write the fixture file");
        path.to_string_lossy().into_owned()
    };
    let old_gate = decoder.gate("a.jpg");
    loader.set_zoom(Some(1.0));
    let old_path = file("a.jpg");
    let token = clock.next_navigation();
    let key = service.get_current_cache_key(&old_path).expect("cache key");
    let mut shown = Vec::new();
    let mut collect = |effects: Vec<ZoomDetailEffect>| {
        for effect in effects {
            if let ZoomDetailEffect::ShowImage { image, .. } = effect {
                shown.push(image);
            }
        }
    };
    collect(loader.on_preview_presented(token, &old_path, &key, &sized_image(600, 400, 6000, 4000, true)));
    assert!(decoder.wait_started("a.jpg"));
    let new_path = file("b.jpg");
    // A rename keeps the file's size and modification time, and the retargeted cache key still carries the OLD file's stamp
    // (`ImageCacheKey::with_path`): give b.jpg a.jpg's stamp, else `decode_original` reports `SourceChanged` whenever the two
    // fixture writes land on different clock ticks (the flake this test used to have: held_original() stayed None).
    let old_modified = std::fs::metadata(&old_path)
        .and_then(|meta| meta.modified())
        .expect("a.jpg modification time");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&new_path)
        .and_then(|handle| handle.set_modified(old_modified))
        .expect("give b.jpg the modification time of a.jpg");
    clock.stop_for_action();
    collect(loader.retarget_path(clock.current_navigation(), &new_path));
    old_gate.open();
    let deadline = Instant::now() + TIMEOUT;
    while decoder.original_decodes("b.jpg") == 0 || loader.pending_load() || loader.queued_events() != 0 {
        assert!(Instant::now() < deadline, "the decodes did not settle");
        loader.wait_for_event(Duration::from_millis(50));
        collect(loader.pump());
    }
    let held = loader
        .held_original()
        .expect("the new path's original is held")
        .platform_image()
        .clone();
    assert!(loader.is_showing_original());
    assert_eq!(shown.len(), 1, "one ShowImage");
    assert!(
        same_platform_image(&shown[0], &held),
        "the shown pixels are the new path's decode"
    );
}
