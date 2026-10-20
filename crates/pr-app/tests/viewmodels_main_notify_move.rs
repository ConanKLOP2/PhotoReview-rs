//! W8-08e: `MainViewModel` change notification, the sink recorders and their replay (D-08e-1), Move to / Copy to, the remembered
//! Move/Copy folder and the photo information (EXIF) line (`contracts/W8-08e.md`, C# `MainViewModel.cs` lines 1086 to 1385). Each test
//! names the C# test it ports (`// C#: Class.Method`) or is marked `// New test`. The window is the whole `MainViewModel` over real
//! coordinators and real files of a temp folder (`support/main_env.rs`); the sink members the C# tests call directly are recorded on
//! the view model's `SinkLog` and replayed with `replay_sink_calls_for_tests`, exactly as after a coordinator step.
//!
//! Not ported, with the reason (no silent skips):
//! - `MoveTo_StateChangedWhileDialogOpen_AbortsWithStatus`, case `image`: the C# hook moves the catalog's current index while the picker
//!   is open. The view model owns the catalog and is mutably borrowed by the call, so nothing can change it there (contract note 8); the
//!   controller twin is `file_action_move_copy.rs::move_to_selection_changes_while_picker_open_*`. The `folder` and `busy` cases are ported.
//! - `HotPath/TitleBarAndExifLineCostTests`, `HotPath/WarmNavigationReadBoundsTests`: perf harnesses (contract).
//! - `OnUnreadableRemovedAsync` is not `async` here: the present is a request (contract note 4), so the C# `await` has no twin.

mod file_action_fakes;
mod support {
    pub mod main_env;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::path::Path;
use std::sync::Arc;

use pr_app::coordinators::traits::{IFileActionSink, IFolderLoadSink, ISiblingNavigatorSink};
use pr_app::viewmodels::format::status;
use pr_app::viewmodels::main::{MainProperty, SinkLog, remember_move_copy_folder};
use pr_core::abstractions::{SkippedEntry, SkippedKind};
use pr_core::model::FileOperationType;
use pr_core::session::store::SessionState;
use pr_l10n::template::LocArg;
use pr_test_support::LogLevel;
use support::main_env::{EnvOptions, MainEnv, en_with, update_settings};
use support::presenter_fakes::FormatFixture;
use support::preview_fakes::lock;

// ---- helpers ----------------------------------------------------------------------------------------------------------------

fn album(env: &mut MainEnv, name: &str, files: &[&str]) -> (String, Vec<String>) {
    let (folder, paths) = env.images(name, files);
    env.open(&folder);
    (folder, paths)
}

fn skipped(path: &Path) -> SkippedEntry {
    SkippedEntry {
        path: path.to_path_buf(),
        reason: "locked".to_owned(),
        kind: SkippedKind::FileUnreadable,
    }
}

/// The folder line of the C# `FolderText` for `count` images (`explorer` adds the Explorer suffix).
fn folder_line(folder: &str, count: i32, explorer: bool) -> String {
    let fixture = FormatFixture::new();
    let ctx = fixture.ctx();
    let key = if explorer {
        "main.folderText.imageCountExplorer"
    } else {
        "main.folderText.imageCount"
    };
    let args = [LocArg::new("count", i64::from(count)), LocArg::new("folder", folder)];
    ctx.texts.format_plural_with(key, i64::from(count), &args, ctx.numbers)
}

fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

fn preload_index_calls(env: &MainEnv) -> usize {
    env.preload.preload_centers().len()
}

/// `evict:` events of the preload for a file name (case-insensitive suffix).
fn evict_calls_for(env: &MainEnv, file_name: &str) -> usize {
    let wanted = file_name.to_ascii_lowercase();
    env.preload
        .events()
        .iter()
        .filter(|event| event.starts_with("evict:") && event.to_ascii_lowercase().ends_with(&wanted))
        .count()
}

// ---- folder sink ------------------------------------------------------------------------------------------------------------

#[test]
fn on_catalog_ready_and_on_empty_adopt_the_session_passed_in_without_reloading() {
    // C#: MainViewModelAdvancedTests.Sink_OnCatalogReadyAndOnEmpty_AdoptTheSessionPassedInWithoutReloading
    let mut env = MainEnv::new();
    let folder = env.folder("session_adopt");
    // The store holds a different session for this folder: a sink that re-read it would return that one.
    env.sessions
        .save(&SessionState {
            current_path: Some("from-disk".to_owned()),
            ..SessionState::new(folder.as_str())
        })
        .unwrap_or_else(|error| panic!("save: {error:?}"));

    let passed = SessionState {
        current_path: Some("from-coordinator".to_owned()),
        ..SessionState::new(folder.as_str())
    };
    let ready = passed.clone();
    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_catalog_ready(&folder, 3, &ready));
    assert_eq!(env.vm.session(), Some(&passed));

    let passed_empty = SessionState {
        current_path: Some("from-coordinator-empty".to_owned()),
        ..SessionState::new(folder.as_str())
    };
    let empty = passed_empty.clone();
    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_empty(&folder, &empty));
    assert_eq!(env.vm.session(), Some(&passed_empty));
}

#[test]
fn folder_text_explorer_order_is_tracked_by_the_state_flag_not_by_the_text() {
    // C#: MainViewModelAdvancedTests.FolderText_ExplorerOrder_IsTrackedByStateFlagNotByText
    let mut env = MainEnv::new();
    let folder = env.folder("explorer_flag");
    let session = SessionState::new(folder.as_str());
    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_catalog_ready(&folder, 3, &session));
    assert!(!env.vm.is_explorer_order_applied());
    assert_eq!(env.vm.folder_text(), folder_line(&folder, 3, false));

    let changes = env
        .vm
        .replay_sink_calls_for_tests(|sink| sink.on_order_applied(3, 0, false));
    assert!(env.vm.is_explorer_order_applied());
    assert_eq!(env.vm.folder_text(), folder_line(&folder, 3, true));
    assert!(changes.main.contains(MainProperty::IsExplorerOrderApplied));

    // Applying again does not stack the suffix.
    let again = env
        .vm
        .replay_sink_calls_for_tests(|sink| sink.on_order_applied(3, 0, false));
    assert_eq!(env.vm.folder_text(), folder_line(&folder, 3, true));
    assert!(!again.main.contains(MainProperty::IsExplorerOrderApplied));

    // A new catalog starts in natural order again.
    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_empty(&folder, &session));
    assert!(!env.vm.is_explorer_order_applied());
    assert_eq!(env.vm.folder_text(), folder_line(&folder, 0, false));
}

#[test]
fn on_empty_with_subfolders_sets_the_subfolders_status_text() {
    // C#: MainViewModelAdvancedTests.Sink_OnEmptyWithSubfolders_SetsSubfoldersStatusText
    let mut env = MainEnv::new();
    let folder = env.folder("empty_with_subfolders");
    let session = SessionState::new(folder.as_str());

    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_empty_with_subfolders(&folder, &session, 3));

    let fixture = FormatFixture::new();
    assert_eq!(
        env.vm.status_text(),
        status::no_supported_images_but_subfolders(3, &fixture.ctx())
    );
    assert_ne!(env.vm.status_text(), status::no_supported_images(&fixture.ctx()));
}

#[test]
fn on_empty_shows_the_no_images_status_clears_the_presentation_and_reports_the_catalog() {
    // New test (C# `OnEmpty`, lines 1132 to 1145)
    let mut env = MainEnv::new();
    let (_, _) = album(&mut env, "empty_after_images", &["1.jpg"]);
    let folder = env.folder("empty_next");
    let session = SessionState::new(folder.as_str());

    let changes = env
        .vm
        .replay_sink_calls_for_tests(|sink| sink.on_empty(&folder, &session));

    let fixture = FormatFixture::new();
    assert_eq!(env.vm.status_text(), status::no_supported_images(&fixture.ctx()));
    assert!(changes.catalog_changed);
    assert!(changes.main.contains(MainProperty::StatusText));
    assert!(changes.main.contains(MainProperty::FolderText));
    assert!(env.preload.events().contains(&"cancel".to_owned()));
}

#[test]
fn opening_a_folder_runs_the_reset_caches_action() {
    // C#: MainViewModelAdvancedTests.AppGaps.OpeningAFolder_RunsTheResetCachesAction
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let mut env = MainEnv::with_options(EnvOptions {
        reset_caches: Some(Box::new(move || {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })),
        ..EnvOptions::default()
    });

    album(&mut env, "reset_caches", &["1.png"]);

    assert!(calls.load(std::sync::atomic::Ordering::SeqCst) >= 1);
}

#[test]
fn opening_a_folder_with_nothing_skipped_does_not_report_the_skipped_entries() {
    // C#: MainViewModelAdvancedTests.AppGaps.OpeningAFolder_WithNothingSkipped_DoesNotRaiseTheSkippedEntriesChange
    let mut env = MainEnv::new();
    let (folder, _) = env.images("no_skipped_notify", &["1.png"]);

    let changes = env.open(&folder);

    assert!(!changes.main.contains(MainProperty::SkippedEntries));
    assert!(!env.changes.main.contains(MainProperty::SkippedEntries));
}

#[test]
fn opening_another_folder_clears_the_skipped_files_warning_of_the_previous_one() {
    // C#: MainViewModelAdvancedTests.MutationGaps.OpeningAnotherFolder_ClearsTheSkippedFilesWarningOfThePreviousOne
    let mut env = MainEnv::new();
    album(&mut env, "skipped_first", &["1.png"]);
    let entry = skipped(&env.photos.join("x.jpg"));
    let shown = entry.clone();
    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_files_skipped("folder", std::slice::from_ref(&shown)));
    assert!(env.vm.has_skipped_entries());
    assert!(!env.vm.skipped_warning_text().is_empty());
    let (other, _) = env.images("skipped_second", &["2.png"]);

    env.open(&other);

    assert!(!env.vm.has_skipped_entries());
    assert_eq!(env.vm.skipped_warning_text(), "");
}

#[test]
fn status_needs_attention_is_raised_by_skipped_entries() {
    // C#: MainViewModelAdvancedTests.MutationGaps.StatusNeedsAttention_IsRaisedByEachReasonOnItsOwn (the skipped leg; the rest is W8-08a)
    let mut env = MainEnv::new();
    album(&mut env, "attention", &["attention.jpg"]);
    assert!(
        !env.vm.status_needs_attention(),
        "the plain index/size/name line is not something the user must read"
    );

    let entry = skipped(&env.photos.join("x.jpg"));
    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_files_skipped("folder", std::slice::from_ref(&entry)));

    assert!(env.vm.status_needs_attention());
}

#[test]
fn files_skipped_does_not_raise_the_catalog_change() {
    // New test (T6: `FilesSkipped` is not a catalog event)
    let mut env = MainEnv::new();
    let entry = skipped(&env.photos.join("x.jpg"));

    let changes = env
        .vm
        .replay_sink_calls_for_tests(|sink| sink.on_files_skipped("folder", std::slice::from_ref(&entry)));

    assert!(!changes.catalog_changed);
    assert!(changes.main.contains(MainProperty::SkippedEntries));
    assert!(changes.main.contains(MainProperty::HasSkippedEntries));
}

/// A failure whose `Display` is the sentence the user must read.
#[derive(Debug)]
struct LocalizedFailure(&'static str);

impl std::fmt::Display for LocalizedFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for LocalizedFailure {}

#[test]
fn on_failed_uses_the_localized_sentence_of_the_error() {
    // C#: MainViewModelNavigationTests.OnFailed_UsesTheLocalizedSentenceOfTheException
    let mut env = MainEnv::new();
    let folder = env.folder("failed");
    let error = LocalizedFailure("localized sentence");

    let changes = env
        .vm
        .replay_sink_calls_for_tests(|sink| sink.on_failed(&folder, &error));

    assert!(
        env.vm.status_text().contains("localized sentence"),
        "{}",
        env.vm.status_text()
    );
    assert!(!env.vm.status_text().contains("raw english message"));
    assert!(changes.main.contains(MainProperty::StatusText));
    assert!(
        changes.main.contains(MainProperty::CurrentIndex),
        "the navigation state is reported too"
    );
    assert!(!changes.catalog_changed);
}

#[test]
fn on_order_applied_recentres_the_preload_only_when_the_current_image_was_kept() {
    // C#: MainViewModelNavigationTests.MutationGaps.OnOrderApplied_RecentresThePreloadOnlyWhenTheCurrentImageWasKept
    for (kept, index, expect_preload) in [(true, 1, true), (false, 1, false), (true, -1, false)] {
        let mut env = MainEnv::new();
        album(&mut env, &format!("order_applied_{kept}_{index}"), &["a.png", "b.png"]);
        let before = preload_index_calls(&env);

        env.vm
            .replay_sink_calls_for_tests(|sink| sink.on_order_applied(2, index, kept));

        assert_eq!(
            preload_index_calls(&env) > before,
            expect_preload,
            "kept {kept}, index {index}"
        );
    }
}

#[test]
fn explorer_order_applied_cancels_the_preload_before_recentering() {
    // C#: MainViewModelFileActionTests.ExplorerOrderApplied_CancelsPreloadBeforeRecentering
    let mut env = MainEnv::new();
    album(&mut env, "order_album", &["1.jpg", "2.jpg"]);
    env.preload.clear_events();

    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_order_applied(2, 0, true));

    assert_eq!(env.preload.events(), vec!["cancel".to_owned(), "preload:0".to_owned()]);
}

#[test]
fn opening_a_folder_cancels_the_running_preload_before_preloading_the_new_catalog() {
    // C#: MainViewModelFileActionTests.OpenFolder_CancelsRunningPreloadBeforePreloadingNewCatalog
    let mut env = MainEnv::new();
    album(&mut env, "preload_a", &["a1.jpg", "a2.jpg"]);
    env.preload.clear_events();

    album(&mut env, "preload_b", &["b1.jpg", "b2.jpg"]);

    // A loop started for folder A holds A's entries; it must be stopped before B is preloaded (R2-F-01).
    let events = env.preload.events();
    let cancel_at = events.iter().position(|event| event == "cancel");
    let preload_at = events.iter().position(|event| event.starts_with("preload:"));
    assert!(
        cancel_at.is_some(),
        "Opening another folder did not cancel the running preload: {events:?}"
    );
    assert!(
        preload_at.is_none() || cancel_at < preload_at,
        "Preload for the new folder started before the old loop was cancelled: {events:?}"
    );
}

#[test]
fn on_order_applied_without_a_session_keeps_the_folder_line_and_only_sets_the_order_flag() {
    // New test (contract note 3: no session -> the shown folder and count are kept, only the flag is set)
    let mut env = MainEnv::new();
    assert!(env.vm.session().is_none());

    let changes = env
        .vm
        .replay_sink_calls_for_tests(|sink| sink.on_order_applied(5, 0, false));

    // Nothing was shown yet: no folder line to re-render, and no flag change either.
    assert_eq!(env.vm.folder_text(), "");
    assert!(!env.vm.is_explorer_order_applied());
    assert!(changes.catalog_changed);
}

#[test]
fn on_order_applied_re_renders_the_folder_line_with_the_new_count_of_the_session_folder() {
    // New test (contract note 3: with a session folder the line is re-rendered with the count of the event)
    let mut env = MainEnv::new();
    let (folder, _) = album(&mut env, "order_count", &["1.jpg", "2.jpg"]);

    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_order_applied(2, 0, false));

    assert_eq!(env.vm.folder_text(), folder_line(&folder, 2, true));
}

// ---- unreadable files -------------------------------------------------------------------------------------------------------

/// Holds a file open with no sharing at all, so the readability probe of the folder load cannot read it (C# `FileShare.None`).
#[cfg(windows)]
fn lock_exclusively(path: &str) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(path)
        .unwrap_or_else(|error| panic!("lock {path}: {error}"))
}

#[cfg(windows)]
#[test]
fn unreadable_probe_non_current_is_removed_from_catalog_preload_and_count() {
    // C#: MainViewModelNavigationTests.UnreadableProbe.UnreadableProbe_NonCurrent_RemovedFromCatalogPreloadAndCount (AR16)
    let mut env = MainEnv::new();
    let (folder, files) = env.images("probe_non_current", &["a.png", "b.png", "c.png"]);
    {
        let _held = lock_exclusively(&files[1]);
        env.open(&folder);
    }

    assert_eq!(
        env.vm.catalog().paths().to_vec(),
        vec![files[0].clone(), files[2].clone()]
    );
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files[0].clone())
    );
    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(env.vm.skipped_entries().len(), 1);
    assert_eq!(env.vm.skipped_entries()[0].path, Path::new(&files[1]));
    assert!(env.vm.has_skipped_entries());

    // The preload loop is stopped and the removed file's keys dropped before preload restarts on the new catalog.
    let events = env.preload.events();
    let evict = events
        .iter()
        .position(|event| event.starts_with("evict:") && event.to_ascii_lowercase().ends_with("b.png"))
        .unwrap_or_else(|| panic!("the removed file's preload keys were not dropped: {events:?}"));
    assert!(evict > 0);
    assert_eq!(events[evict - 1], "cancel");
    assert_eq!(events.last().map(String::as_str), Some("preload:0"));
}

#[cfg(windows)]
#[test]
fn unreadable_probe_current_advances_to_next_and_updates_the_session() {
    // C#: MainViewModelNavigationTests.UnreadableProbe.UnreadableProbe_Current_AdvancesToNextAndUpdatesSession (AR16)
    let mut env = MainEnv::new();
    let (folder, files) = env.images("probe_current", &["a.png", "b.png", "c.png"]);
    {
        let _held = lock_exclusively(&files[0]);
        env.open(&folder);
    }

    assert_eq!(
        env.vm.catalog().paths().to_vec(),
        vec![files[1].clone(), files[2].clone()]
    );
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files[1].clone())
    );
    assert_eq!(
        env.vm.session().and_then(|session| session.current_path.clone()),
        Some(files[1].clone())
    );
    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(env.vm.skipped_entries().len(), 1);
    assert_eq!(env.vm.skipped_entries()[0].path, Path::new(&files[0]));
    let events = env.preload.events();
    let evict = events
        .iter()
        .position(|event| event.starts_with("evict:") && event.to_ascii_lowercase().ends_with("a.png"))
        .unwrap_or_else(|| panic!("the removed file's preload keys were not dropped: {events:?}"));
    assert!(evict > 0);
    assert_eq!(events[evict - 1], "cancel");
}

#[test]
fn on_unreadable_removed_when_the_current_image_was_the_last_one_left_shows_the_empty_state() {
    // C#: MainViewModelAdvancedTests.MutationGaps.OnUnreadableRemoved_WhenTheCurrentImageWasTheLastOneLeft_ShowsTheEmptyState
    let mut env = MainEnv::new();

    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_unreadable_removed(&[], true));

    let fixture = FormatFixture::new();
    assert_eq!(env.vm.status_text(), status::no_images_remaining(&fixture.ctx()));
}

#[test]
fn on_unreadable_removed_while_the_current_image_stays_recentres_the_preload_and_evicts_each_path() {
    // New test (contract note 4 and T4/T5: evict per path, cancel before the re-centre)
    let mut env = MainEnv::new();
    let (_, files) = album(&mut env, "unreadable_stays", &["a.png", "b.png", "c.png"]);
    env.preload.clear_events();

    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_unreadable_removed(&files[1..], false));

    let events = env.preload.events();
    assert_eq!(evict_calls_for(&env, "b.png"), 1, "{events:?}");
    assert_eq!(evict_calls_for(&env, "c.png"), 1, "{events:?}");
    let cancel = events.iter().position(|event| event == "cancel").unwrap_or(usize::MAX);
    let around = events
        .iter()
        .position(|event| event.starts_with("preload:"))
        .unwrap_or_else(|| panic!("no re-centre: {events:?}"));
    assert!(cancel < around, "{events:?}");
}

#[test]
fn on_unreadable_removed_re_renders_the_folder_line_with_the_remaining_count_and_clears_compare() {
    // New test (contract note 4: the folder line is re-rendered with `catalog.count()`; the compare state is cleared for a removed current image)
    let mut env = MainEnv::new();
    let (folder, files) = album(&mut env, "unreadable_line", &["a.png", "b.png", "c.png"]);
    assert_eq!(env.vm.folder_text(), folder_line(&folder, 3, false));
    env.vm.compare_mut_for_tests().select(Some(files[1].clone()));
    assert_eq!(env.vm.compare().selected_path(), Some(files[1].as_str()));
    env.vm.catalog_mut_for_tests().remove_paths(&files[1..]);

    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_unreadable_removed(&files[1..], true));
    env.settle();

    assert_eq!(env.vm.folder_text(), folder_line(&folder, 1, false));
    assert_eq!(env.vm.compare().selected_path(), None);
}

// ---- file-action and navigator sinks ----------------------------------------------------------------------------------------

#[test]
fn update_session_path_records_the_current_path_on_the_session() {
    // C#: MainViewModelAdvancedTests.MutationGaps.UpdateSessionPath_RecordsTheCurrentPathOnTheSession
    let mut env = MainEnv::new();
    let (folder, _) = album(&mut env, "session_path", &["1.png"]);
    let path = Path::new(&folder).join("elsewhere.png").to_string_lossy().into_owned();
    let before = env.vm.session().map(|session| session.updated_utc);

    env.vm
        .replay_sink_calls_for_tests(|sink| IFileActionSink::update_session_path(sink, &path));

    let session = env.vm.session().unwrap_or_else(|| panic!("no session"));
    assert_eq!(session.current_path.as_deref(), Some(path.as_str()));
    assert_ne!(Some(session.updated_utc), before, "the update time is recorded");
    // The session is persisted (debounced writer; a flush writes it now).
    env.session_writer.flush();
    let stored = env
        .sessions
        .load(&folder)
        .unwrap_or_else(|error| panic!("load: {error:?}"));
    assert_eq!(stored.current_path.as_deref(), Some(path.as_str()));
}

#[test]
fn update_session_path_without_a_session_does_nothing() {
    // New test (contract note 6: no session -> nothing)
    let mut env = MainEnv::new();

    env.vm
        .replay_sink_calls_for_tests(|sink| IFileActionSink::update_session_path(sink, "x.png"));

    assert!(env.vm.session().is_none());
}

#[test]
fn notify_navigation_state_from_a_controller_refreshes_the_navigation_flags() {
    // C#: MainViewModelAdvancedTests.MutationGaps.NotifyNavigationStateChanged_FromAController_RefreshesTheNavigationFlags
    let mut env = MainEnv::new();
    album(&mut env, "controller_notify", &["1.png"]);

    let changes = env
        .vm
        .replay_sink_calls_for_tests(IFileActionSink::notify_navigation_state_changed);

    for property in [
        MainProperty::CanNavigateNext,
        MainProperty::HasImages,
        MainProperty::TotalFiles,
        MainProperty::CanNavigatePrevious,
        MainProperty::CurrentIndex,
        MainProperty::CurrentImage,
        MainProperty::StatusText,
        MainProperty::CurrentHasCapturePair,
        MainProperty::CapturePairBadge,
    ] {
        assert!(changes.main.contains(property), "{property:?}");
    }
    // As C#: the attention flag is deliberately not part of the navigation notification.
    assert!(!changes.main.contains(MainProperty::StatusNeedsAttention));
    assert!(!changes.catalog_changed);
}

#[test]
fn notify_presentation_changed_is_the_navigation_notification() {
    // New test (contract: `notify_presentation_changed` is the C# `SetStatusText` sink target)
    let mut env = MainEnv::new();
    album(&mut env, "presentation_changed", &["1.png", "2.png"]);

    let changes = env.vm.notify_presentation_changed();

    assert!(changes.main.contains(MainProperty::CurrentIndex));
    assert!(changes.main.contains(MainProperty::TotalFiles));
    assert!(changes.main.contains(MainProperty::ExifText) || changes.main.contains(MainProperty::IsExifLineVisible));
}

#[test]
fn evict_cached_paths_from_a_controller_drops_each_path_from_the_preloaded_keys() {
    // C#: MainViewModelNavigationTests.MutationGaps.EvictCachedPaths_FromAController_DropsEachPathFromThePreloadedKeys
    let mut env = MainEnv::new();
    let (_, files) = album(&mut env, "evict_paths", &["a.png", "b.png"]);

    env.vm
        .replay_sink_calls_for_tests(|sink| sink.evict_cached_paths(&files));

    assert_eq!(evict_calls_for(&env, "a.png"), 1);
    assert_eq!(evict_calls_for(&env, "b.png"), 1);
}

#[test]
fn on_catalog_changed_with_a_removed_path_evicts_only_that_path() {
    // C#: MainViewModelNavigationTests.MutationGaps.OnCatalogChanged_WithARemovedPath_EvictsOnlyThatPath
    let mut env = MainEnv::new();
    let (_, files) = album(&mut env, "catalog_changed", &["a.png", "b.png"]);

    let first = env.vm.replay_sink_calls_for_tests(|sink| sink.on_catalog_changed(None));
    assert!(first.catalog_changed);
    assert_eq!(evict_calls_for(&env, "a.png"), 0);

    let second = env
        .vm
        .replay_sink_calls_for_tests(|sink| sink.on_catalog_changed(Some(&files[0])));
    assert!(second.catalog_changed);
    assert_eq!(evict_calls_for(&env, "a.png"), 1);
    assert_eq!(evict_calls_for(&env, "b.png"), 0);
}

#[test]
fn late_status_is_dropped_unless_the_event_status_is_empty() {
    // C#: FolderChange.LateCompletionHint_DoesNotOverwriteNewFolderStatus, plus new cases
    let mut env = MainEnv::new();
    let folder = env.folder("late_status");
    let session = SessionState::new(folder.as_str());
    // The new folder's own message ("no images") is the event status.
    env.vm
        .replay_sink_calls_for_tests(|sink| sink.on_empty(&folder, &session));
    let own = env.vm.status_text();
    assert!(!own.is_empty());

    let dropped = env
        .vm
        .replay_sink_calls_for_tests(|sink| sink.show_late_action_status("late hint"));

    assert_eq!(
        env.vm.status_text(),
        own,
        "a late hint must not replace the folder's own text"
    );
    assert!(!dropped.main.contains(MainProperty::StatusText));

    // With no event text the hint is shown.
    env.vm
        .replay_sink_calls_for_tests(|sink| IFileActionSink::set_status_text(sink, ""));
    assert_eq!(env.vm.status_text(), "");
    let shown = env
        .vm
        .replay_sink_calls_for_tests(|sink| sink.show_late_action_status("late hint"));
    assert_eq!(env.vm.status_text(), "late hint");
    assert!(shown.main.contains(MainProperty::StatusText));
    assert!(!shown.catalog_changed);
}

// ---- recorder behaviour -----------------------------------------------------------------------------------------------------

#[test]
fn recorded_calls_replay_in_call_order() {
    // New test (T1: the order of the recorded calls is preserved)
    let mut env = MainEnv::new();
    let folder = env.folder("replay_order");
    let session = SessionState::new(folder.as_str());

    env.vm.replay_sink_calls_for_tests(|sink| {
        IFileActionSink::set_status_text(sink, "first");
        sink.on_catalog_ready(&folder, 2, &session);
        IFileActionSink::set_status_text(sink, "last");
    });

    // `on_catalog_ready` assigns "1/2"-style index text; the status calls around it win in call order.
    assert_eq!(env.vm.status_text(), "last");

    env.vm.replay_sink_calls_for_tests(|sink| {
        IFileActionSink::set_status_text(sink, "first");
        sink.on_empty(&folder, &session);
    });
    let fixture = FormatFixture::new();
    assert_eq!(env.vm.status_text(), status::no_supported_images(&fixture.ctx()));
}

#[test]
fn folder_present_clears_the_event_status_but_action_present_does_not() {
    // New test (T2: `IFolderLoadSink.PresentAsync` sets `_statusText = ""`, `IFileActionSink.PresentAsync` does not)
    let mut env = MainEnv::new();
    album(&mut env, "present_status", &["1.png", "2.png"]);

    env.vm
        .replay_sink_calls_for_tests(|sink| IFileActionSink::set_status_text(sink, "event text"));
    env.vm
        .replay_sink_calls_for_tests(|sink| IFolderLoadSink::present(sink, 1, 0));
    env.settle();
    assert!(
        !env.vm.status_text().contains("event text"),
        "the folder-load present cleared the event text: {}",
        env.vm.status_text()
    );

    env.vm
        .replay_sink_calls_for_tests(|sink| IFileActionSink::set_status_text(sink, "event text"));
    env.vm
        .replay_sink_calls_for_tests(|sink| IFileActionSink::present(sink, 0));
    assert!(
        env.vm.status_text().contains("event text"),
        "the file-action present kept the event text: {}",
        env.vm.status_text()
    );
}

#[test]
fn a_replayed_open_folder_records_and_replays_its_own_sink_calls() {
    // New test (D-08e-1: `apply_sink_actions` repeats until the log stays empty)
    let mut env = MainEnv::new();
    let (first, _) = album(&mut env, "replay_open_a", &["a.png"]);
    let (second, files) = env.images("replay_open_b", &["b1.png", "b2.png"]);

    env.vm
        .replay_sink_calls_for_tests(|sink| ISiblingNavigatorSink::open_folder(sink, &second, None));
    env.settle();

    assert_ne!(first, second);
    assert_eq!(env.vm.catalog().paths().to_vec(), files);
    assert_eq!(env.vm.folder_text(), folder_line(&second, 2, false));
    assert_eq!(env.vm.session().map(|session| session.folder.clone()), Some(second));
}

#[test]
fn catalog_changed_is_reported_only_for_the_callbacks_that_raised_the_event() {
    // New test (T6)
    let mut env = MainEnv::new();
    let (folder, files) = album(&mut env, "catalog_flag", &["1.png", "2.png"]);
    let session = SessionState::new(folder.as_str());
    let entry = skipped(&env.photos.join("x.jpg"));

    let raising = [
        env.vm
            .replay_sink_calls_for_tests(|sink| sink.on_catalog_ready(&folder, 2, &session)),
        env.vm
            .replay_sink_calls_for_tests(|sink| sink.on_empty(&folder, &session)),
        env.vm
            .replay_sink_calls_for_tests(|sink| sink.on_empty_with_subfolders(&folder, &session, 1)),
        env.vm
            .replay_sink_calls_for_tests(|sink| sink.on_order_applied(2, 0, true)),
        env.vm.replay_sink_calls_for_tests(|sink| sink.on_catalog_changed(None)),
        env.vm
            .replay_sink_calls_for_tests(|sink| sink.on_unreadable_removed(&files[1..], false)),
    ];
    for (position, changes) in raising.iter().enumerate() {
        assert!(changes.catalog_changed, "callback {position}");
    }

    let quiet = [
        env.vm
            .replay_sink_calls_for_tests(|sink| IFileActionSink::set_status_text(sink, "text")),
        env.vm
            .replay_sink_calls_for_tests(|sink| sink.show_late_action_status("late")),
        env.vm
            .replay_sink_calls_for_tests(|sink| sink.on_files_skipped(&folder, std::slice::from_ref(&entry))),
        env.vm
            .replay_sink_calls_for_tests(|sink| sink.evict_cached_paths(&files)),
        env.vm
            .replay_sink_calls_for_tests(IFileActionSink::notify_navigation_state_changed),
    ];
    for (position, changes) in quiet.iter().enumerate() {
        assert!(!changes.catalog_changed, "callback {position}");
    }
}

#[test]
fn on_catalog_ready_assigns_the_index_status_without_a_property_report_of_its_own_and_reports_the_navigation_state() {
    // New test (contract note 1: the event status of `CatalogReady` is assigned silently; the navigation notification reports it)
    let mut env = MainEnv::new();
    let folder = env.folder("ready_status");
    let session = SessionState::new(folder.as_str());

    let changes = env
        .vm
        .replay_sink_calls_for_tests(|sink| sink.on_catalog_ready(&folder, 4, &session));

    let fixture = FormatFixture::new();
    assert_eq!(env.vm.status_text(), status::index_only(0, 4, &fixture.ctx()));
    assert!(changes.main.contains(MainProperty::StatusText));
    assert!(changes.main.contains(MainProperty::FolderText));
    assert!(changes.catalog_changed);
}

// ---- Move to / Copy to ------------------------------------------------------------------------------------------------------

struct MoveAlbum {
    env: MainEnv,
    files: Vec<String>,
    folder: String,
    dest: String,
}

fn move_album_with(options: EnvOptions) -> MoveAlbum {
    let mut env = MainEnv::with_options(options);
    let (folder, files) = env.images("photos_album", &["1.jpg", "2.jpg"]);
    let dest = env.folder("dest");
    env.open(&folder);
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files[0].clone())
    );
    MoveAlbum {
        env,
        files,
        folder,
        dest,
    }
}

fn move_album() -> MoveAlbum {
    move_album_with(EnvOptions::default())
}

fn answer(env: &MainEnv, folder: Option<&str>) {
    *lock(&env.picker.answer) = folder.map(str::to_owned);
}

fn picker_calls(env: &MainEnv) -> Vec<(String, Option<String>)> {
    lock(&env.picker.calls).clone()
}

fn undo_len(env: &MainEnv) -> usize {
    lock(&env.undo).move_history().len()
}

fn photos_root(env: &MainEnv) -> String {
    env.photos.to_string_lossy().into_owned()
}

/// A fresh store over the same config file: what a restart would read back.
fn reloaded_settings(env: &MainEnv) -> Arc<pr_core::settings::app_settings::AppSettings> {
    use pr_core::abstractions::{FileSystem, Log, SystemClock};
    use pr_core::io::physical::PhysicalFileSystem;
    use pr_core::settings::app_settings::EnglishSettingsTexts;
    use pr_core::settings::store::SettingsStore;
    let fs: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem);
    let log: Arc<dyn Log> = Arc::new(pr_test_support::RecordingLog::new());
    let store = SettingsStore::new(
        env.paths.clone(),
        fs,
        log,
        Arc::new(SystemClock),
        Arc::new(EnglishSettingsTexts),
    );
    store
        .load(None)
        .unwrap_or_else(|error| panic!("reload the settings: {error}"))
}

#[test]
fn move_to_picked_folder_moves_advances_remembers_the_folder_and_undo_restores() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_PickedFolder_MovesAdvancesRemembersFolderAndUndoRestores
    let MoveAlbum {
        mut env, files, dest, ..
    } = move_album();
    answer(&env, Some(&dest));

    env.vm.move_to_folder(false);
    env.settle();

    let moved = Path::new(&dest).join("1.jpg");
    assert!(moved.exists());
    assert!(!exists(&files[0]));
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files[1].clone())
    );
    assert_eq!(env.vm.total_files(), 1);
    // The picker started at the photo folder's parent (no last folder yet) and was titled for Move.
    let calls = picker_calls(&env);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, env.text("dialog.moveToFolder.title"));
    assert_eq!(calls[0].1, Some(photos_root(&env)));
    assert_eq!(
        env.vm.status_text(),
        en_with(
            "status.movedToFolder",
            &[LocArg::new("fileName", "1.jpg"), LocArg::new("folder", dest.as_str())]
        )
    );
    // Remembered and persisted: a fresh store reads it back from config.json.
    assert_eq!(
        reloaded_settings(&env).last_move_to_folder.as_deref(),
        Some(dest.as_str())
    );
    assert_eq!(env.settings.current().last_copy_to_folder, None);

    assert_eq!(undo_len(&env), 1);
    env.vm.undo();
    env.settle();
    assert!(exists(&files[0]));
    assert!(!moved.exists());
    assert_eq!(env.vm.total_files(), 2);
}

#[test]
fn move_to_when_remembering_the_folder_fails_still_moves_and_keeps_the_folder_in_memory() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_WhenRememberingTheFolderFails_StillMovesAndKeepsTheFolderInMemory
    let MoveAlbum {
        mut env, files, dest, ..
    } = move_album();
    answer(&env, Some(&dest));
    let config = env.paths.config_file.clone();
    if config.exists() {
        std::fs::remove_file(&config).unwrap_or_else(|error| panic!("remove config: {error}"));
    }
    std::fs::create_dir_all(&config).unwrap_or_else(|error| panic!("config as a folder: {error}")); // config.json can no longer be written

    env.vm.move_to_folder(false);
    env.settle();

    assert!(Path::new(&dest).join("1.jpg").exists());
    assert!(!exists(&files[0]));
    assert_eq!(
        env.settings.current().last_move_to_folder.as_deref(),
        Some(dest.as_str())
    );
    assert!(
        env.log.entries().iter().any(|(level, text)| *level == LogLevel::Warn
            && text.starts_with("Could not save the last Move-to/Copy-to folder:")),
        "{:?}",
        env.log.entries()
    );
}

#[test]
fn copy_to_picked_folder_copies_keeps_the_source_and_registers_no_undo() {
    // C#: MainViewModelMoveCopyToTests.CopyTo_PickedFolder_CopiesKeepsSourceAndDoesNotRegisterUndo
    let MoveAlbum {
        mut env, files, dest, ..
    } = move_album();
    answer(&env, Some(&dest));

    env.vm.copy_to_folder(false);
    env.settle();

    assert!(Path::new(&dest).join("1.jpg").exists());
    assert!(exists(&files[0]));
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files[0].clone())
    );
    assert_eq!(env.vm.total_files(), 2);
    let calls = picker_calls(&env);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, env.text("dialog.copyToFolder.title"));
    assert_eq!(
        env.vm.status_text(),
        en_with(
            "status.copiedToFolder",
            &[LocArg::new("fileName", "1.jpg"), LocArg::new("folder", dest.as_str())]
        )
    );
    assert_eq!(
        env.settings.current().last_copy_to_folder.as_deref(),
        Some(dest.as_str())
    );
    assert_eq!(env.settings.current().last_move_to_folder, None);
    assert_eq!(undo_len(&env), 0);
}

#[test]
fn move_to_cancelled_is_a_no_op() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_Cancelled_IsNoOp
    let MoveAlbum { mut env, files, .. } = move_album();
    answer(&env, None);
    let status_before = env.vm.status_text();

    env.vm.move_to_folder(false);
    env.settle();

    assert_eq!(picker_calls(&env).len(), 1);
    assert!(exists(&files[0]));
    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files[0].clone())
    );
    assert_eq!(env.vm.status_text(), status_before);
    assert_eq!(env.settings.current().last_move_to_folder, None);
    assert!(!env.vm.is_file_action_in_progress());
    assert!(!env.service.is_busy());
}

#[test]
fn move_to_reuse_last_on_skips_the_picker_and_moves_to_the_last_folder() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_ReuseLastOn_SkipsPickerAndMovesToLastFolder
    let MoveAlbum {
        mut env, files, dest, ..
    } = move_album();
    update_settings(&env.store, |settings| {
        settings.move_copy_reuse_last_folder = true;
        settings.last_move_to_folder = Some(dest.clone());
    });

    env.vm.move_to_folder(false);
    env.settle();

    assert!(picker_calls(&env).is_empty());
    assert!(Path::new(&dest).join("1.jpg").exists());
    assert!(!exists(&files[0]));
    assert_eq!(
        env.vm.status_text(),
        en_with(
            "status.movedToFolder",
            &[LocArg::new("fileName", "1.jpg"), LocArg::new("folder", dest.as_str())]
        )
    );
}

#[test]
fn move_to_reuse_last_on_but_forced_opens_the_picker_at_the_last_folder() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_ReuseLastOnButForced_OpensPickerAtLastFolder
    let MoveAlbum {
        mut env, files, dest, ..
    } = move_album();
    let other = env.folder("other");
    update_settings(&env.store, |settings| {
        settings.move_copy_reuse_last_folder = true;
        settings.last_move_to_folder = Some(dest.clone());
    });
    answer(&env, Some(&other));

    env.vm.move_to_folder(true);
    env.settle();

    let calls = picker_calls(&env);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1.as_deref(), Some(dest.as_str()));
    assert!(Path::new(&other).join("1.jpg").exists());
    assert!(!Path::new(&dest).join("1.jpg").exists());
    assert!(!exists(&files[0]));
    assert_eq!(
        env.settings.current().last_move_to_folder.as_deref(),
        Some(other.as_str())
    );
}

#[test]
fn move_to_reuse_last_off_opens_the_picker_at_the_last_folder() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_ReuseLastOff_OpensPickerAtLastFolder
    let MoveAlbum { mut env, dest, .. } = move_album();
    update_settings(&env.store, |settings| settings.last_move_to_folder = Some(dest.clone()));
    answer(&env, None);

    env.vm.move_to_folder(false);
    env.settle();

    let calls = picker_calls(&env);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1.as_deref(), Some(dest.as_str()));
}

#[test]
fn move_to_reuse_last_but_the_folder_was_deleted_opens_the_picker_at_the_photo_parent() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_ReuseLastButFolderDeleted_OpensPickerAtPhotoParent
    let MoveAlbum { mut env, files, .. } = move_album();
    let gone = env.photos.join("gone").to_string_lossy().into_owned();
    update_settings(&env.store, |settings| {
        settings.move_copy_reuse_last_folder = true;
        settings.last_move_to_folder = Some(gone.clone());
    });
    answer(&env, None);

    env.vm.move_to_folder(false);
    env.settle();

    let calls = picker_calls(&env);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, Some(photos_root(&env)));
    assert!(exists(&files[0]));
}

#[test]
fn move_to_state_changed_while_the_dialog_is_open_aborts_with_status() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_StateChangedWhileDialogOpen_AbortsWithStatus (cases `folder` and `busy`)
    for change in ["folder", "busy"] {
        let MoveAlbum {
            mut env, files, dest, ..
        } = move_album();
        answer(&env, Some(&dest));
        let clock = Arc::clone(&env.clock);
        let service = Arc::clone(&env.service);
        let held_busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let held = Arc::clone(&held_busy);
        *lock(&env.picker.while_open) = Some(Box::new(move || match change {
            "folder" => {
                clock.next_folder(); // a forwarded open started loading another folder
            }
            _ => held.store(service.try_begin(), std::sync::atomic::Ordering::SeqCst),
        }));

        env.vm.move_to_folder(false);
        env.settle();
        if held_busy.load(std::sync::atomic::Ordering::SeqCst) {
            env.service.end();
        }

        assert!(exists(&files[0]), "{change}");
        assert!(exists(&files[1]), "{change}");
        assert_eq!(std::fs::read_dir(&dest).map_or(0, Iterator::count), 0, "{change}");
        assert_eq!(env.vm.total_files(), 2, "{change}");
        assert_eq!(undo_len(&env), 0, "{change}");
        assert_eq!(
            env.vm.status_text(),
            en_with(
                "status.moveCopyTo.stateChanged",
                &[LocArg::new("actionName", env.text("action.moveToFolder.name").as_str())]
            ),
            "{change}"
        );
        assert_eq!(env.settings.current().last_move_to_folder, None, "{change}");
        assert!(!env.vm.is_file_action_in_progress(), "{change}");
    }
}

#[test]
fn move_to_the_photo_folder_itself_is_refused() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_PhotoFolderItself_IsRefused
    let MoveAlbum {
        mut env, files, folder, ..
    } = move_album();
    answer(&env, Some(&format!("{folder}{}", std::path::MAIN_SEPARATOR)));

    env.vm.move_to_folder(false);
    env.settle();

    assert!(exists(&files[0]));
    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(
        env.vm.status_text(),
        en_with(
            "status.moveCopyTo.sameFolder",
            &[LocArg::new("actionName", env.text("action.moveToFolder.name").as_str())]
        )
    );
}

#[test]
fn copy_to_a_missing_or_relative_folder_is_refused() {
    // C#: MainViewModelMoveCopyToTests.CopyTo_MissingOrRelativeFolder_IsRefused
    let MoveAlbum {
        mut env, files, folder, ..
    } = move_album();
    let missing = env.photos.join("missing").to_string_lossy().into_owned();
    let name = env.text("action.copyToFolder.name");

    answer(&env, Some(&missing));
    env.vm.copy_to_folder(false);
    env.settle();
    assert_eq!(
        env.vm.status_text(),
        en_with(
            "status.moveCopyTo.folderMissing",
            &[
                LocArg::new("actionName", name.as_str()),
                LocArg::new("folder", missing.as_str())
            ]
        )
    );
    assert!(
        !Path::new(&missing).exists(),
        "the pipeline would create it: it must never be reached"
    );

    answer(&env, Some("relative"));
    env.vm.copy_to_folder(false);
    env.settle();
    assert_eq!(
        env.vm.status_text(),
        en_with(
            "status.moveCopyTo.notAbsolute",
            &[LocArg::new("actionName", name.as_str())]
        )
    );
    assert!(!Path::new(&folder).join("relative").exists());
    assert!(exists(&files[0]));
}

#[test]
fn move_to_name_collision_follows_the_action_policy_and_keeps_both_files() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_NameCollision_FollowsActionPolicy_RefusesAndKeepsBothFiles
    let MoveAlbum {
        mut env, files, dest, ..
    } = move_album();
    let existing = Path::new(&dest).join("1.jpg");
    std::fs::write(&existing, [1_u8, 2, 3]).unwrap_or_else(|error| panic!("write: {error}"));
    answer(&env, Some(&dest));

    env.vm.move_to_folder(false);
    env.settle();

    assert!(exists(&files[0]));
    assert_eq!(
        std::fs::read(&existing).unwrap_or_default(),
        vec![1_u8, 2, 3],
        "never overwritten"
    );
    assert_eq!(env.vm.total_files(), 2, "INV-5: the source went back into the catalog");
    assert_eq!(env.vm.catalog().paths()[0], files[0]);
    assert_eq!(undo_len(&env), 0);
    let fixture = FormatFixture::new();
    let reason = en_with(
        "core.fileAction.destinationExists",
        &[LocArg::new("path", existing.to_string_lossy().as_ref())],
    );
    assert_eq!(
        env.vm.status_text(),
        status::action_failed(&env.text("action.moveToFolder.name"), Some(&reason), &fixture.ctx())
    );
    assert_eq!(
        env.settings.current().last_move_to_folder,
        None,
        "only a successful operation is remembered"
    );
}

fn size_changed_move() -> pr_core::file_actions::undo::MoveOverride {
    Arc::new(|source: &Path, destination: &Path| {
        std::fs::rename(source, destination)?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(destination)?;
        file.write_all(b"grew while moving")
    })
}

fn move_to_size_changed_during_the_move(vietnamese: bool) {
    let MoveAlbum {
        mut env, files, dest, ..
    } = move_album_with(EnvOptions {
        move_override: Some(size_changed_move()),
        vietnamese,
        ..EnvOptions::default()
    });
    answer(&env, Some(&dest));

    env.vm.move_to_folder(false);
    env.settle();

    let moved = Path::new(&dest).join("1.jpg");
    assert!(!exists(&files[0]));
    assert!(moved.exists());
    assert_eq!(env.vm.total_files(), 1);
    assert!(!env.vm.catalog().paths().contains(&files[0]));
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files[1].clone())
    );
    let journal = pr_core::file_actions::journal::OperationJournal::new(
        &env.paths,
        Arc::new(pr_core::io::physical::PhysicalFileSystem),
        Arc::new(pr_core::abstractions::SystemClock),
    );
    let failed = journal
        .read_pending_and_failed_operations()
        .unwrap_or_else(|error| panic!("journal: {error}"));
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].error_code.as_deref(), Some("VerifySizeChanged"));
    assert_eq!(failed[0].source, files[0]);
    assert_eq!(undo_len(&env), 0);
    assert_eq!(
        env.settings.current().last_move_to_folder,
        None,
        "not a success: the folder is not remembered"
    );
    let fixture = if vietnamese {
        FormatFixture::vietnamese()
    } else {
        FormatFixture::new()
    };
    assert_eq!(
        env.vm.status_text(),
        fixture.ctx().texts.format_with(
            "status.moveUnverified",
            &[LocArg::new("fileName", "1.jpg")],
            fixture.ctx().numbers
        )
    );
    assert!(env.vm.status_text().contains(if vietnamese {
        "không xác minh được"
    } else {
        "could not be verified"
    }));
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn move_to_size_changed_during_the_move_keeps_the_source_out_of_the_catalog_warns_and_journals_failed_en() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_SizeChangedDuringMove_SourceStaysOutOfCatalog_WarnsAndJournalsFailed (language en)
    move_to_size_changed_during_the_move(false);
}

#[test]
fn move_to_size_changed_during_the_move_keeps_the_source_out_of_the_catalog_warns_and_journals_failed_vi() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_SizeChangedDuringMove_SourceStaysOutOfCatalog_WarnsAndJournalsFailed (language vi)
    move_to_size_changed_during_the_move(true);
}

#[test]
fn move_to_queued_in_folder_a_whose_folder_changes_before_its_turn_does_not_move_the_new_folder_photo() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_QueuedInFolderA_FolderChangesBeforeItsTurn_DoesNotMoveNewFolderPhoto (R02)
    let MoveAlbum {
        mut env, files, dest, ..
    } = move_album();
    let (other, other_files) = env.images("other", &["x.jpg"]);
    answer(&env, Some(&dest));

    env.vm.move_to_folder(false);
    assert!(env.vm.is_file_action_in_progress());
    env.vm.move_to_folder(false); // queued behind the first
    env.settle_holding("everything but the file operation", &["FileAction"], |_| false);
    env.vm
        .open_folder(&other, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle();

    assert!(
        exists(&other_files[0]),
        "A Move-to queued in folder A moved folder B's photo."
    );
    assert!(!Path::new(&dest).join("x.jpg").exists());
    assert_eq!(env.vm.catalog().paths().to_vec(), other_files);
    assert!(!exists(&files[0]), "the command that had started in A completed there");
    assert!(exists(&files[1]));
    assert_eq!(undo_len(&env), 1);
    assert_eq!(
        picker_calls(&env).len(),
        1,
        "the stale queued command never even asked for a folder"
    );
}

#[test]
fn move_to_source_still_existing_after_a_failed_move_keeps_the_source_in_the_catalog() {
    // C#: MainViewModelMoveCopyToTests.MoveTo_SourceStillExistsAfterFailedMove_KeepsSourceInCatalog
    let copy_only: pr_core::file_actions::undo::MoveOverride = Arc::new(|source: &Path, destination: &Path| {
        std::fs::copy(source, destination).map(|_| ()) // cross-volume style: copied, but the source could not be deleted
    });
    let MoveAlbum {
        mut env, files, dest, ..
    } = move_album_with(EnvOptions {
        move_override: Some(copy_only),
        ..EnvOptions::default()
    });
    answer(&env, Some(&dest));

    env.vm.move_to_folder(false);
    env.settle();

    assert!(exists(&files[0]));
    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(env.vm.catalog().paths()[0], files[0]);
    let fixture = FormatFixture::new();
    let reason = env.text("core.fileAction.moveSourceNotRemoved");
    assert_eq!(
        env.vm.status_text(),
        status::action_failed(&env.text("action.moveToFolder.name"), Some(&reason), &fixture.ctx())
    );
}

#[test]
fn move_to_and_copy_to_are_no_ops_for_an_empty_catalog() {
    // New test (controller early return: empty catalog -> `Done(false)`, no picker, gate released)
    let mut env = MainEnv::new();
    answer(&env, Some(&photos_root(&env)));

    env.vm.move_to_folder(false);
    env.vm.copy_to_folder(true);
    env.settle();

    assert!(picker_calls(&env).is_empty());
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn move_to_with_a_compare_selection_moves_the_selected_photo() {
    // New test (contract note 8: `get_source` is the snapshot `(compare selected path, catalog current path)`)
    let MoveAlbum {
        mut env, files, dest, ..
    } = move_album();
    env.vm.compare_mut_for_tests().select(Some(files[1].clone()));
    answer(&env, Some(&dest));

    env.vm.move_to_folder(false);
    env.settle();

    assert!(Path::new(&dest).join("2.jpg").exists());
    assert!(exists(&files[0]));
    assert!(!exists(&files[1]));
}

#[test]
fn move_to_waits_for_the_previous_action_and_then_runs_on_the_then_current_photo() {
    // New test (same gate and pipeline as `run_action`, INV-4)
    let MoveAlbum {
        mut env, files, dest, ..
    } = move_album();
    answer(&env, Some(&dest));

    env.vm.run_action(0); // Move to "Sorted"
    env.vm.move_to_folder(false); // queued behind it
    env.settle();

    assert!(!exists(&files[0]));
    assert!(!exists(&files[1]));
    assert!(
        Path::new(&dest).join("2.jpg").exists(),
        "the queued command ran on the then-current photo"
    );
    assert_eq!(undo_len(&env), 2);
    assert!(!env.vm.is_file_action_in_progress());
}

// ---- remembered Move/Copy folder --------------------------------------------------------------------------------------------

#[test]
fn remember_move_copy_folder_sets_the_move_or_the_copy_setting_and_saves() {
    // New test (C# `RememberMoveCopyFolder`)
    let env = MainEnv::new();

    remember_move_copy_folder(&env.settings, env.log.as_ref(), FileOperationType::Move, "C:\\moves");
    assert_eq!(env.settings.current().last_move_to_folder.as_deref(), Some("C:\\moves"));
    assert_eq!(env.settings.current().last_copy_to_folder, None);

    remember_move_copy_folder(&env.settings, env.log.as_ref(), FileOperationType::Copy, "C:\\copies");
    assert_eq!(
        env.settings.current().last_copy_to_folder.as_deref(),
        Some("C:\\copies")
    );
    assert_eq!(env.settings.current().last_move_to_folder.as_deref(), Some("C:\\moves"));

    let reloaded = reloaded_settings(&env);
    assert_eq!(reloaded.last_move_to_folder.as_deref(), Some("C:\\moves"));
    assert_eq!(reloaded.last_copy_to_folder.as_deref(), Some("C:\\copies"));
}

#[test]
fn remember_move_copy_folder_logs_a_warning_when_the_save_fails_and_keeps_the_value() {
    // New test (the text is `Could not save the last Move-to/Copy-to folder: <message>`)
    let env = MainEnv::new();
    let config = env.paths.config_file.clone();
    if config.exists() {
        std::fs::remove_file(&config).unwrap_or_else(|error| panic!("remove config: {error}"));
    }
    std::fs::create_dir_all(&config).unwrap_or_else(|error| panic!("config as a folder: {error}"));

    remember_move_copy_folder(&env.settings, env.log.as_ref(), FileOperationType::Move, "C:\\moves");

    assert_eq!(env.settings.current().last_move_to_folder.as_deref(), Some("C:\\moves"));
    let warnings: Vec<String> = env
        .log
        .entries()
        .into_iter()
        .filter(|(level, _)| *level == LogLevel::Warn)
        .map(|(_, text)| text)
        .collect();
    assert!(
        warnings
            .iter()
            .any(|text| text.starts_with("Could not save the last Move-to/Copy-to folder: ")),
        "{warnings:?}"
    );
}

#[test]
fn remember_move_copy_folder_keeps_other_settings_changed_since() {
    // New test (T8: it clones the CURRENT settings at the time of the call)
    let env = MainEnv::new();
    update_settings(&env.store, |settings| settings.keyboard_zoom_step_percent = 37);

    remember_move_copy_folder(&env.settings, env.log.as_ref(), FileOperationType::Copy, "C:\\copies");

    assert_eq!(env.settings.current().keyboard_zoom_step_percent, 37);
    assert_eq!(
        env.settings.current().last_copy_to_folder.as_deref(),
        Some("C:\\copies")
    );
    assert_eq!(reloaded_settings(&env).keyboard_zoom_step_percent, 37);
}

#[test]
fn sink_log_records_every_trait_in_call_order() {
    // New test (D-08e-1: the recorder is the sink of all four coordinators; `Debug` is derived, T10)
    let mut log = SinkLog::new();
    IFileActionSink::set_status_text(&mut log, "a");
    log.on_catalog_changed(None);
    let text = format!("{log:?}");
    assert!(text.contains("SetStatus") && text.contains("CatalogChanged"), "{text}");
}

// ---- photo information (EXIF) line ------------------------------------------------------------------------------------------

mod exif_line {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use pr_app::viewmodels::main::MainProperty;
    use pr_core::model::ExifInfoFields;
    use pr_imaging::decode_types::{
        DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo, PlatformImage,
    };
    use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
    use pr_imaging::exif_summary::{ExifRational, ExifSummary};

    use super::support::main_env::{EnvOptions, MainEnv, update_settings};
    use super::support::preview_fakes::{Gate, lock};

    /// A decoded image that carries EXIF and, for a RAW, the size of its embedded JPEG (`IRawPreviewInfo`).
    #[derive(Debug)]
    struct ExifView {
        inner: DecodedImage,
        embedded: (i32, i32),
    }

    impl DecodedImageView for ExifView {
        fn pixel_width(&self) -> i32 {
            self.inner.pixel_width()
        }
        fn pixel_height(&self) -> i32 {
            self.inner.pixel_height()
        }
        fn downscaled(&self) -> bool {
            self.inner.downscaled()
        }
        fn orientation(&self) -> i32 {
            1
        }
        fn estimated_bytes(&self) -> i64 {
            self.inner.estimated_bytes()
        }
        fn platform_image(&self) -> &PlatformImage {
            self.inner.platform_image()
        }
        fn original_width(&self) -> i32 {
            self.inner.original_width()
        }
        fn original_height(&self) -> i32 {
            self.inner.original_height()
        }
        fn exif(&self) -> Option<&ExifSummary> {
            self.inner.exif()
        }
        fn embedded_preview_size(&self) -> Option<(i32, i32)> {
            (self.embedded.0 > 0).then_some(self.embedded)
        }
    }

    /// C# `ExifDecoder`: a 4 x 3 decode of a 6000 x 4000 original whose EXIF and embedded preview size are scripted per file name.
    /// `held` blocks the decode of a file until its gate opens.
    struct ExifDecoder {
        exif: HashMap<String, ExifSummary>,
        preview: HashMap<String, (i32, i32)>,
        held: Mutex<HashMap<String, Arc<Gate>>>,
    }

    impl std::fmt::Debug for ExifDecoder {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ExifDecoder").finish_non_exhaustive()
        }
    }

    impl ImageDecoder for ExifDecoder {
        fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
            let name = request
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let gate = lock(&self.held).get(&name).cloned();
            if let Some(gate) = gate {
                assert!(gate.wait(), "the held decode of {name} was never released");
            }
            let inner = DecodedImage::new(
                PlatformImage::Opaque(Arc::new(0_u8)),
                4,
                3,
                48,
                DecodedImageOptions {
                    downscaled: true,
                    original_width: 6000,
                    original_height: 4000,
                    exif: self.exif.get(&name).cloned(),
                    ..DecodedImageOptions::default()
                },
            )
            .unwrap_or_else(|error| panic!("test image: {error}"));
            Ok(Arc::new(ExifView {
                inner,
                embedded: self.preview.get(&name).copied().unwrap_or((0, 0)),
            }))
        }

        fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
            Ok(ImageInfo::new(6000, 4000))
        }
    }

    fn canon() -> ExifSummary {
        ExifSummary {
            camera_make: Some("Canon".to_owned()),
            camera_model: Some("Canon EOS R5".to_owned()),
            iso: Some(400),
            exposure_time: Some(ExifRational::new(1, 250)),
            ..ExifSummary::default()
        }
    }

    fn env_with(decoder: ExifDecoder, fields: ExifInfoFields) -> MainEnv {
        MainEnv::with_options(EnvOptions {
            decoder: Arc::new(decoder),
            settings: Box::new(move |settings| {
                settings.show_exif_info = true;
                settings.exif_info_fields = fields;
            }),
            ..EnvOptions::default()
        })
    }

    #[test]
    fn exif_line_follows_the_presented_image_and_settings() {
        // C#: ExifLineViewModelTests.LineFollowsPresentedImageAndSettings
        let b_gate = Gate::new();
        let mut held = HashMap::new();
        held.insert("b.jpg".to_owned(), Arc::clone(&b_gate));
        let mut exif = HashMap::new();
        for name in ["a.jpg", "c.jpg", "c (1).jpg"] {
            exif.insert(name.to_owned(), canon());
        }
        let decoder = ExifDecoder {
            exif,
            preview: HashMap::new(),
            held: Mutex::new(held),
        };
        // Default ExifInfoFields excludes FileName/Dimensions: the status line above already shows those.
        let mut env = env_with(decoder, ExifInfoFields::DEFAULT);
        let (folder, _) = env.images("exif_album", &["a.jpg", "b.jpg", "c.jpg", "c (1).jpg"]); // "c.jpg" + "c (1).jpg" = a compare pair
        let changes = env.open(&folder);

        let line = "Canon EOS R5 · ISO 400 · 1/250 s";
        assert_eq!(env.vm.exif_text(), line);
        assert!(env.vm.is_exif_line_visible());
        assert!(changes.main.contains(MainProperty::ExifText) || env.changes.main.contains(MainProperty::ExifText));
        assert_eq!(
            env.vm.exif_automation_name(),
            env.text_with(
                "main.exif.automationName",
                &[pr_l10n::template::LocArg::new("details", line)]
            )
        );

        // Integration: file info off keeps the panel for the EXIF line; the master switch (key I) hides the EXIF line too.
        update_settings(&env.store, |settings| settings.show_file_info = false);
        assert!(env.vm.is_exif_line_visible());
        assert!(env.vm.is_status_panel_visible());
        let toggled = env.vm.toggle_info_overlay();
        assert!(!env.vm.settings().show_info_overlay);
        assert!(!env.vm.is_exif_line_visible());
        assert!(!env.vm.is_status_panel_visible());
        assert!(toggled.main.contains(MainProperty::IsStatusPanelVisible));
        env.vm.toggle_info_overlay();
        update_settings(&env.store, |settings| settings.show_file_info = true);
        assert!(env.vm.is_exif_line_visible());

        update_settings(&env.store, |settings| {
            settings.exif_info_fields = ExifInfoFields::CAMERA
                .union(ExifInfoFields::ISO)
                .union(ExifInfoFields::SHUTTER_SPEED);
        });
        assert_eq!(env.vm.exif_text(), line);

        update_settings(&env.store, |settings| settings.show_exif_info = false);
        assert!(!env.vm.is_exif_line_visible());
        assert!(!env.vm.exif_text().is_empty());
        // ModifiedDate excluded here: it is the (real, wall-clock) file's last-write time, not fixed test data, and this step asserts an
        // exact string equal to "b.jpg · 6000×4000" below.
        update_settings(&env.store, |settings| {
            settings.show_exif_info = true;
            settings.exif_info_fields = ExifInfoFields::ALL.difference(ExifInfoFields::MODIFIED_DATE);
        });

        env.vm.next(); // b.jpg: decode held until released, then decodes without EXIF
        assert!(env.vm.presenter().current_photo_info().is_none());
        assert_eq!(
            env.vm.exif_text(),
            "",
            "while b loads, a's EXIF is not shown under b's status"
        );
        b_gate.open();
        env.settle();

        assert_eq!(env.vm.exif_text(), "b.jpg · 6000×4000");
        assert!(!env.vm.exif_text().contains("Canon"));

        update_settings(&env.store, |settings| {
            settings.exif_info_fields = ExifInfoFields::CAMERA.union(ExifInfoFields::LENS);
        });
        assert_eq!(env.vm.exif_text(), "");
        assert!(
            !env.vm.is_exif_line_visible(),
            "nothing to show: hidden even though ShowExifInfo is on"
        );
        update_settings(&env.store, |settings| settings.exif_info_fields = ExifInfoFields::ALL);

        env.vm.next(); // a compare pair: two images, the compare status describes them, no single-image line
        env.settle();

        assert!(env.vm.compare().is_visible(), "next did not reach the compare pair");
        let shown = env
            .vm
            .compare()
            .left_path()
            .or_else(|| env.vm.compare().right_path())
            .unwrap_or_default()
            .to_owned();
        assert!(shown.ends_with("c.jpg") || shown.ends_with("c (1).jpg"), "{shown}");
        assert!(env.vm.compare().left_path().is_some());
        assert!(env.vm.compare().right_path().is_some());
        assert_eq!(env.vm.exif_text(), "");
        assert!(!env.vm.is_exif_line_visible());
    }

    #[test]
    fn exif_line_shows_the_raw_preview_size_only_for_an_embedded_preview_image() {
        // C#: ExifLineViewModelTests.LineShowsRawPreviewSize_OnlyForAnEmbeddedPreviewImage (Q-RAW-03)
        let mut preview = HashMap::new();
        preview.insert("p.jpg".to_owned(), (1620, 1080)); // decodes to an embedded RAW preview
        let decoder = ExifDecoder {
            exif: HashMap::new(),
            preview,
            held: Mutex::new(HashMap::new()),
        };
        let mut env = env_with(decoder, ExifInfoFields::FILE_NAME.union(ExifInfoFields::DIMENSIONS));
        let (folder, _) = env.images("raw_album", &["n.jpg", "p.jpg"]);
        env.open(&folder);
        assert_eq!(env.vm.exif_text(), "n.jpg · 6000×4000");

        env.vm.next();
        env.settle();
        assert_eq!(env.vm.exif_text(), "p.jpg · 6000×4000 · RAW preview 1620×1080");
        assert!(env.vm.is_exif_line_visible());

        update_settings(&env.store, |settings| settings.show_exif_info = false); // settings unchanged: the line (and so the label) is hidden with it
        assert!(!env.vm.is_exif_line_visible());
    }

    #[test]
    fn exif_text_is_empty_while_nothing_is_presented() {
        // New test (contract note 9)
        let env = env_with(
            ExifDecoder {
                exif: HashMap::new(),
                preview: HashMap::new(),
                held: Mutex::new(HashMap::new()),
            },
            ExifInfoFields::ALL,
        );

        assert_eq!(env.vm.exif_text(), "");
        assert!(!env.vm.is_exif_line_visible());
    }

    #[test]
    fn is_exif_line_visible_needs_the_master_switch_and_the_exif_switch_and_text() {
        // New test (the `||` chain of C# `IsExifLineVisible`: every term alone hides the line)
        let mut exif = HashMap::new();
        exif.insert("a.jpg".to_owned(), canon());
        let mut env = env_with(
            ExifDecoder {
                exif,
                preview: HashMap::new(),
                held: Mutex::new(HashMap::new()),
            },
            ExifInfoFields::CAMERA,
        );
        let (folder, _) = env.images("visible_album", &["a.jpg"]);
        env.open(&folder);
        assert!(env.vm.is_exif_line_visible());

        update_settings(&env.store, |settings| settings.show_info_overlay = false);
        assert!(!env.vm.is_exif_line_visible(), "master switch off");
        update_settings(&env.store, |settings| {
            settings.show_info_overlay = true;
            settings.show_exif_info = false;
        });
        assert!(!env.vm.is_exif_line_visible(), "exif switch off");
        update_settings(&env.store, |settings| {
            settings.show_exif_info = true;
            settings.exif_info_fields = ExifInfoFields::NONE;
        });
        assert!(!env.vm.is_exif_line_visible(), "no text");
        update_settings(&env.store, |settings| {
            settings.exif_info_fields = ExifInfoFields::CAMERA
        });
        assert!(env.vm.is_exif_line_visible());
    }

    #[test]
    fn exif_line_shows_the_catalog_entrys_modified_time_without_reading_the_disk() {
        // New test (contract note 9: ModifiedDate = the catalog entry's last write, converted with `FormatContextSource::to_local_time`)
        let mut env = env_with(
            ExifDecoder {
                exif: HashMap::new(),
                preview: HashMap::new(),
                held: Mutex::new(HashMap::new()),
            },
            ExifInfoFields::MODIFIED_DATE,
        );
        let (folder, _) = env.images("modified_album", &["a.jpg"]);
        env.open(&folder);

        assert!(
            env.vm
                .catalog()
                .current()
                .is_some_and(|entry| entry.last_write_utc.is_some()),
            "the scan records the last write"
        );
        let text = env.vm.exif_text();

        // The fixture's date formatter yields an empty date text: the label is what proves the time was passed on.
        assert!(text.starts_with("Modified:"), "the modified time is shown: {text:?}");
    }

    #[test]
    fn exif_automation_name_wraps_the_text() {
        // New test (contract note 9: `main.exif.automationName` with `{details}`)
        let mut exif = HashMap::new();
        exif.insert("a.jpg".to_owned(), canon());
        let mut env = env_with(
            ExifDecoder {
                exif,
                preview: HashMap::new(),
                held: Mutex::new(HashMap::new()),
            },
            ExifInfoFields::CAMERA,
        );
        let (folder, _) = env.images("automation_album", &["a.jpg"]);
        env.open(&folder);

        let name = env.vm.exif_automation_name();

        assert!(name.contains("Canon EOS R5"), "{name}");
        assert_ne!(name, "Canon EOS R5");
    }

    #[test]
    fn notify_exif_line_changed_reports_the_four_properties() {
        // New test (`notify_exif_line_changed` is the part of the navigation notification that reports the EXIF line)
        let mut env = MainEnv::new();

        let changes = env.vm.notify_presentation_changed();

        for property in [
            MainProperty::ExifText,
            MainProperty::IsExifLineVisible,
            MainProperty::ExifAutomationName,
            MainProperty::IsStatusPanelVisible,
        ] {
            assert!(changes.main.contains(property), "{property:?}");
        }
        let _ = Path::new("");
    }
}
