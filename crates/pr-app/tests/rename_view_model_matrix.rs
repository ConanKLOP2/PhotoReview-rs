//! W9-R10: the `MainViewModelRenameTests` of C# 7eaee2a2 that R6/R7 do not already cover, in the REAL `MainViewModel` over real
//! coordinators, a real presenter over a fake decoder and real tiny files under a `TempRoot` (the `MainEnv` fixture).
//!
//! Already ported (R6 `presenter_rename.rs`): `RekeysTheSessionAndPersistsIt`, `RekeysTheSkippedList`,
//! `WhileAnExplorerOrderIsPending_WaitsForItThenRenames`. Already ported (R7 `rename_prompt_presenter.rs`):
//! `PromptRenameCurrentAsync_TheViewerMovedOnWhileTheBoxWasOpen_RenamesNothing` and the single-file policy part of
//! `PromptRenameCurrentAsync_ValidateFollowsThePolicy_AndTypingNeverTouchesTheDisk` (this file adds the full row list through the
//! view model's own prepared prompt).
//!
//! Deviations of the Rust shape (R0003): `RenameCurrentAsync` returns a `ChangeSet`, so the C# `FileRenameResult` is observed through
//! the files, the catalog, the status line and the sinks; the C# `Task` waits are `MainEnv::settle` pumps (bounded, no sleeps); the C#
//! `SetZoom(2.0)` is `zoom_actual_size()` (the viewer handle has no zoom setter that reaches the presenter, and 100 percent of the
//! 6000 x 4000 original is still far beyond the 600 px preview).

mod file_action_fakes;
mod support {
    pub mod main_env;
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::path::Path;
use std::sync::{Arc, Mutex};

use pr_app::coordinators::presenter::state::ImageChange;
use pr_app::coordinators::zoom_detail::same_platform_image;
use pr_app::presenters::rename_prompt::PreparedRenamePrompt;
use pr_core::abstractions::{FileSystem, RenamePromptRequest};
use pr_core::catalog::types::{CaptureGroup, CatalogEntry};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::RawPairMode;
use pr_core::model::enums::InitialViewMode;
use pr_core::text::eq_ignore_case;
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use support::main_env::{EnvOptions, MainEnv};
use support::preview_fakes::{SizedDecoder, lock};

/// The prompt texts come from the process-wide current localizer: serialize the tests that read it.
static LOCALIZER_LOCK: Mutex<()> = Mutex::new(());

fn english() {
    Localizer::set_current(built_in_english_localizer());
}

fn join(folder: &str, name: &str) -> String {
    Path::new(folder).join(name).to_string_lossy().into_owned()
}

fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

fn env_with(decoder: Arc<SizedDecoder>) -> MainEnv {
    MainEnv::with_options(EnvOptions {
        decoder,
        ..EnvOptions::default()
    })
}

fn presented_count(env: &MainEnv) -> usize {
    lock(&env.sink_seen).presented.len()
}

fn image_count(env: &MainEnv) -> usize {
    lock(&env.sink_seen).images
}

/// C# `ApplyFitWidth()`.
fn apply_fit_width(env: &MainEnv) {
    let mut viewer = env.viewer.borrow_mut();
    viewer.set_dpi_scale(1.0);
    viewer.apply_initial_view_mode(InitialViewMode::FitWidth, 1200.0, 800.0, 100, false);
}

/// Runs the rename to its end.
fn rename(env: &mut MainEnv, name: &str) {
    env.vm.rename_current(name);
    env.settle();
}

// C#: MainViewModelRenameTests.RenameCurrentAsync_ShownPhoto_KeepsIndexZoomAndPresentsNothing
#[test]
fn rename_current_async_shown_photo_keeps_index_zoom_and_presents_nothing() {
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_keep", &["a.jpg", "b.jpg", "c.jpg"]);
    env.open_at(&folder, &files[1]);
    assert_eq!(env.vm.catalog().current_index(), 1);
    apply_fit_width(&env);
    let zoom = env.viewer.borrow().zoom();
    let width = env.viewer.borrow().source_pixel_width();
    let presented = presented_count(&env);
    let images = image_count(&env);
    let current = env.vm.presenter().current_image().cloned();
    assert!(current.is_some(), "an image is shown");

    rename(&mut env, "renamed");

    let renamed = join(&folder, "renamed.jpg");
    assert!(exists(&renamed));
    assert!(!exists(&files[1]));
    assert_eq!(
        env.vm.catalog().paths().to_vec(),
        [files[0].clone(), renamed.clone(), files[2].clone()]
    );
    assert_eq!(env.vm.catalog().current_index(), 1);
    assert_eq!(presented_count(&env), presented, "present was not run again");
    assert_eq!(image_count(&env), images, "the bitmap was not touched");
    assert!(
        current
            .as_ref()
            .zip(env.vm.presenter().current_image())
            .is_some_and(|(before, after)| same_platform_image(before, after))
    );
    assert_eq!(env.vm.presenter().current_presented_path(), Some(renamed.as_str()));
    assert_eq!(
        env.vm
            .presenter()
            .current_photo_info()
            .map(|info| info.file_name.as_str()),
        Some("renamed.jpg")
    );
    assert_eq!(env.viewer.borrow().zoom(), zoom);
    assert_eq!(env.viewer.borrow().source_pixel_width(), width);
}

// C#: MainViewModelRenameTests.RenameCurrentAsync_ThenNotifyAndZoomSwap_TheViewerStillSeesTheSameImage
#[test]
fn rename_current_async_then_notify_and_zoom_swap_the_viewer_still_sees_the_same_image() {
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_same", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    apply_fit_width(&env);
    assert!((env.viewer.borrow().zoom() - 1200.0 / 6000.0).abs() < 1e-6);

    rename(&mut env, "z");
    let renamed = join(&folder, "z.jpg");
    // A zoom swap's SetCurrentImage: the same (renamed) file, so NOT a new image.
    env.bridge.on_set_current_image(
        &ImageChange {
            presented_path: Some(&renamed),
            original_width: 6000,
            original_height: 4000,
            is_same_source_swap: false,
        },
        false,
    );
    env.viewer.borrow_mut().swap_source_size(6016, 4016, &mut |_| {});

    // A same-image swap keeps the Fit axis: the width fit is re-applied to the new size. A "new image" forgets the axis, which
    // would leave the old zoom factor.
    assert!((env.viewer.borrow().zoom() - 1200.0 / 6016.0).abs() < 1e-6);
}

// C#: MainViewModelRenameTests.RenameCurrentAsync_ZoomedWithHeldOriginal_ZoomStaysAndTheOriginalIsNotDecodedAgain
#[test]
fn rename_current_async_zoomed_with_held_original_zoom_stays_and_the_original_is_not_decoded_again() {
    let decoder = SizedDecoder::new();
    let mut env = env_with(decoder.clone());
    let (folder, files) = env.images("rename_zoomed", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    env.vm.zoom_actual_size();
    assert!(env.vm.presenter().zoom_detail().pending_load());
    env.settle();
    let held = env
        .vm
        .presenter()
        .zoom_detail()
        .held_original()
        .cloned()
        .unwrap_or_else(|| panic!("a held original"));
    let zoom = env.viewer.borrow().zoom();
    let decodes = decoder.total_original_decodes();
    let images = image_count(&env);

    rename(&mut env, "z");

    assert!(exists(&join(&folder, "z.jpg")));
    assert_eq!(env.viewer.borrow().zoom(), zoom);
    let held_now = env.vm.presenter().zoom_detail().held_original().expect("still held");
    assert!(same_platform_image(held.platform_image(), held_now.platform_image()));
    assert!(env.vm.presenter().zoom_detail().is_showing_original());
    assert_eq!(decoder.total_original_decodes(), decodes);
    assert_eq!(image_count(&env), images);
}

// C#: MainViewModelRenameTests.RenameCurrentAsync_ZoomedAndTheOriginalNotYetDecoded_DecodesItViaTheNewName
#[test]
fn rename_current_async_zoomed_and_the_original_not_yet_decoded_decodes_it_via_the_new_name() {
    let decoder = SizedDecoder::new();
    let old_gate = decoder.gate("a.jpg");
    let new_gate = decoder.gate("z.jpg");
    let mut env = env_with(decoder.clone());
    let (folder, files) = env.images("rename_zoomlate", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    env.vm.zoom_actual_size();
    assert!(env.vm.presenter().zoom_detail().pending_load(), "decoding a.jpg, gated");
    let renamed = join(&folder, "z.jpg");

    // The gated decode runs on its own thread, so the rename does not wait for it.
    env.vm.rename_current("z");
    env.settle_until("the rename to re-key the presenter", |env| {
        env.vm.presenter().current_presented_path() == Some(renamed.as_str())
    });

    assert!(exists(&renamed));
    assert!(
        env.vm.presenter().zoom_detail().pending_load(),
        "the decode restarted under the new name"
    );
    old_gate.open();
    new_gate.open();
    env.settle();

    assert_eq!(decoder.original_decodes("z.jpg"), 1, "decoded via the new name");
    assert!(env.vm.presenter().zoom_detail().held_original().is_some());
}

// C#: MainViewModelRenameTests.RenameCurrentAsync_ANewViewModelOverTheSameFolder_StartsAtTheRenamedPhotoNotTheFirst
#[test]
fn rename_current_async_a_new_view_model_over_the_same_folder_starts_at_the_renamed_photo_not_the_first() {
    // MainEnv builds exactly one view model, so the folder is opened again in it: the real session store (flushed to disk) is
    // what decides where the reopened folder starts.
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_restart", &["a.jpg", "b.jpg", "c.jpg"]);
    env.open_at(&folder, &files[2]);
    rename(&mut env, "zz");
    env.session_writer.flush();

    env.open(&folder);

    let renamed = join(&folder, "zz.jpg");
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path()),
        Some(renamed.as_str())
    );
    assert_eq!(
        env.vm.session().and_then(|session| session.current_path.as_deref()),
        Some(renamed.as_str())
    );
    assert_ne!(env.vm.catalog().current_index(), 0);
}

// C#: MainViewModelRenameTests.RenameCurrentAsync_QueuedWhileAnotherFolderGetsOpened_IsDroppedAndRenamesNothing
#[test]
fn rename_current_async_queued_while_another_folder_gets_opened_is_dropped_and_renames_nothing() {
    let mut env = MainEnv::new();
    let (folder_a, files_a) = env.images("rename_dropA", &["a.jpg", "b.jpg"]);
    let (folder_b, _) = env.images("rename_dropB", &["x.jpg", "y.jpg"]);
    env.explorer.set_order(files_a.clone());
    env.vm
        .open_folder(&folder_a, Some(&files_a[0]))
        .unwrap_or_else(|error| panic!("open A: {error}"));
    env.settle_holding("present before the Explorer order", &["Explorer"], |_| false);

    env.vm.rename_current("renamed"); // queued behind the pending order of folder A
    env.settle_holding("rename remains behind the Explorer order", &["Explorer"], |_| false);
    assert!(exists(&files_a[0]));
    assert!(
        !env.vm.is_idle(),
        "the rename is parked behind the Explorer order, not lost"
    );
    env.vm
        .open_folder(&folder_b, None)
        .unwrap_or_else(|error| panic!("open B: {error}")); // another folder is opened meanwhile
    while env.runner.run_tagged("Explorer") {}
    env.settle();

    assert!(exists(&files_a[0]));
    assert!(!exists(&join(&folder_a, "renamed.jpg")));
    assert!(!exists(&join(&folder_b, "renamed.jpg")));
    assert!(exists(&join(&folder_b, "x.jpg")));
}

// C#: MainViewModelRenameTests.RenameCurrentAsync_CompareVisible_ClearsCompareAndPresentsTheRenamedEntryAgain
#[test]
fn rename_current_async_compare_visible_clears_compare_and_presents_the_renamed_entry_again() {
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_compare", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    env.vm.compare_mut_for_tests().set_visible(true);
    let presented = presented_count(&env);

    rename(&mut env, "z");

    let renamed = join(&folder, "z.jpg");
    assert!(exists(&renamed));
    assert!(!env.vm.presenter().is_compare_visible());
    assert_eq!(presented_count(&env), presented + 1);
    assert_eq!(lock(&env.sink_seen).presented.last(), Some(&renamed));
    assert_eq!(env.vm.presenter().current_presented_path(), Some(renamed.as_str()));
}

// C#: MainViewModelRenameTests.RenameCurrentAsync_EvictsThePreviewCacheOfTheOldName
#[test]
fn rename_current_async_evicts_the_preview_cache_of_the_old_name() {
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_cache", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    assert!(
        env.preview_service.cache_count() >= 1,
        "the shown preview is cached under a.jpg"
    );

    rename(&mut env, "z");

    assert_eq!(
        env.preview_service.cache_count(),
        0,
        "nothing is left under the old name (the kept image is not re-cached either)"
    );
}

// C#: MainViewModelRenameTests.RenameCurrentAsync_CancelsPreloadThenWarmsAroundTheCurrentIndex
#[test]
fn rename_current_async_cancels_preload_then_warms_around_the_current_index() {
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_preload", &["a.jpg", "b.jpg", "c.jpg"]);
    env.open_at(&folder, &files[1]);
    env.preload.clear_events();

    rename(&mut env, "z");

    // The Rust preload fake also logs the per-path eviction (`evict:<path>`); the C# recorder does not.
    let events: Vec<String> = env
        .preload
        .events()
        .into_iter()
        .filter(|event| !event.starts_with("evict:"))
        .collect();
    assert_eq!(events, ["cancel", "preload:1"]);
}

// C#: MainViewModelRenameTests.RenameCurrentAsync_InvalidName_ReportsInvalidAndChangesNothing
#[test]
fn rename_current_async_invalid_name_reports_invalid_and_changes_nothing() {
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_invalid", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    let presented = presented_count(&env);

    rename(&mut env, "a?b");

    assert!(exists(&files[0]));
    assert_eq!(presented_count(&env), presented);
    assert_eq!(env.vm.presenter().current_presented_path(), Some(files[0].as_str()));
    assert_eq!(env.vm.catalog().paths().to_vec(), files);
    let expected = env.text_with(
        "core.fileAction.renameInvalidName",
        &[pr_l10n::template::LocArg::new("name", "a?b")],
    );
    assert_eq!(env.vm.status_text(), expected, "the refusal names the typed name");
}

// C#: MainViewModelRenameTests.RenameCurrentAsync_NoFolderOpen_IsNothingToRename
#[test]
fn rename_current_async_no_folder_open_is_nothing_to_rename() {
    let mut env = MainEnv::new();
    let status = env.vm.status_text();

    rename(&mut env, "x");

    assert_eq!(env.vm.catalog().count(), 0);
    assert_eq!(env.vm.status_text(), status, "nothing to rename: no status, no job");
    assert_eq!(env.runner.pending(), 0);
    assert!(env.vm.is_idle());
}

// C#: MainViewModelRenameTests.UndoAsync_AfterRename_PutsTheOldNameBackWithoutPresentingAndRekeysTheSession
#[test]
fn undo_async_after_rename_puts_the_old_name_back_without_presenting_and_rekeys_the_session() {
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_undo", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    rename(&mut env, "z");
    let presented = presented_count(&env);

    env.vm.undo();
    env.settle();

    assert!(exists(&files[0]));
    assert!(!exists(&join(&folder, "z.jpg")));
    assert_eq!(env.vm.presenter().current_presented_path(), Some(files[0].as_str()));
    assert_eq!(
        env.vm.session().and_then(|session| session.current_path.as_deref()),
        Some(files[0].as_str())
    );
    assert_eq!(presented_count(&env), presented);
    assert_eq!(
        env.vm
            .presenter()
            .current_photo_info()
            .map(|info| info.file_name.as_str()),
        Some("a.jpg")
    );
}

// ---- FEAT-RENAME PR3: the name prompt (PromptRenameCurrentAsync) ---------------------------------------------------------

/// What the shell does around the modal box: queue the plan, release the view-model borrow, build the request with a live catalog
/// lookup and the real disk, let `answer` act as the user (it may type into `validate`), then finish with the answer.
/// Returns whether the box was shown.
fn prompt(env: &mut MainEnv, answer: impl FnOnce(&RenamePromptRequest) -> Option<String>) -> bool {
    env.vm.prompt_rename_current();
    let Some(plan) = env.vm.take_rename_prompt() else {
        return false;
    };
    let listed = listing_of(env, &plan);
    let fs: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem);
    let request = plan
        .request(listed, fs)
        .unwrap_or_else(|error| panic!("request: {error:?}"));
    let typed = answer(&request);
    let source = plan.source().to_owned();
    env.vm.finish_rename_prompt(&source, typed.as_deref());
    env.settle();
    true
}

fn listing_of(env: &MainEnv, _plan: &PreparedRenamePrompt) -> Arc<dyn Fn(&str) -> bool> {
    let paths: Vec<String> = env.vm.catalog().paths().to_vec();
    #[allow(clippy::arc_with_non_send_sync)] // matches the dialog seam: consumed synchronously on the UI thread
    let listed: Arc<dyn Fn(&str) -> bool> =
        Arc::new(move |candidate: &str| paths.iter().any(|p| eq_ignore_case(p, candidate)));
    listed
}

// C#: MainViewModelRenameTests.PromptRenameCurrentAsync_AsksWithTheStemAndTheFixedExtension_ThenRenamesAndUndoPutsItBack
#[test]
fn prompt_rename_current_async_asks_with_the_stem_and_the_fixed_extension_then_renames_and_undo_puts_it_back() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    english();
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_prompt", &["a.jpg", "b.JPG", "c.jpg"]);
    env.open_at(&folder, &files[1]);

    let shown = prompt(&mut env, |request| {
        assert_eq!(request.initial_name, "b");
        assert_eq!(
            request.extension, ".JPG",
            "the extension is fixed, in the case it has on disk"
        );
        Some("holiday 01".to_owned())
    });

    let renamed = join(&folder, "holiday 01.JPG");
    assert!(shown);
    assert!(exists(&renamed));
    assert!(!exists(&files[1]));
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path()),
        Some(renamed.as_str())
    );
    assert_eq!(env.vm.catalog().current_index(), 1);

    env.vm.undo();
    env.settle();

    assert!(exists(&files[1]));
    assert!(!exists(&renamed));
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path()),
        Some(files[1].as_str())
    );
}

// C#: MainViewModelRenameTests.PromptRenameCurrentAsync_Cancelled_ChangesNothingAndIsSilent
#[test]
fn prompt_rename_current_async_cancelled_changes_nothing_and_is_silent() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    english();
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_promptcancel", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    let presented = presented_count(&env);
    let status = env.vm.status_text();
    let prompts = Mutex::new(0);

    let shown = prompt(&mut env, |_| {
        *prompts.lock().unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
        None
    });

    assert!(shown);
    assert_eq!(*prompts.lock().unwrap_or_else(std::sync::PoisonError::into_inner), 1);
    assert!(exists(&files[0]));
    assert_eq!(presented_count(&env), presented);
    assert_eq!(env.vm.catalog().paths().to_vec(), files);
    assert_eq!(env.vm.status_text(), status, "silent");
    assert_eq!(env.runner.pending(), 0, "no rename was queued");
}

// C#: MainViewModelRenameTests.PromptRenameCurrentAsync_TheSameNameTyped_IsNoChange_NothingTouched
#[test]
fn prompt_rename_current_async_the_same_name_typed_is_no_change_nothing_touched() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    english();
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_promptsame", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);

    prompt(&mut env, |_| Some("a".to_owned()));

    assert!(exists(&files[0]));
    assert_eq!(env.vm.catalog().paths().to_vec(), files);
    assert_eq!(
        env.vm.status_text(),
        env.text_with(
            "status.renameUnchanged",
            &[pr_l10n::template::LocArg::new("fileName", "a.jpg")]
        )
    );
}

// C#: MainViewModelRenameTests.PromptRenameCurrentAsync_ValidateFollowsThePolicy_AndTypingNeverTouchesTheDisk
#[test]
fn prompt_rename_current_async_validate_follows_the_policy_and_typing_never_touches_the_disk() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    english();
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_promptvalidate", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    env.add_image(&folder, "late.jpg"); // on disk, not in the listing
    let seen: Mutex<Vec<(String, bool, Option<String>)>> = Mutex::new(Vec::new());

    prompt(&mut env, |request| {
        let long = "x".repeat(300);
        let rows: [(&str, bool); 11] = [
            ("", false),
            ("a?", false),
            ("a.", false),
            ("CON", false),
            (long.as_str(), false),
            ("b", false),
            ("a", false),
            ("A", false),
            ("late", false),
            ("late", true),
            ("fresh", true),
        ];
        for (typed, commit) in rows {
            let error = (request.validate)(typed, commit);
            seen.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((typed.to_owned(), commit, error));
        }
        None
    });

    let seen = seen.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
    let error = |typed: &str, commit: bool| -> Option<String> {
        let mut rows = seen.iter().filter(|row| row.0 == typed && row.1 == commit);
        let row = rows.next().unwrap_or_else(|| panic!("no row for {typed:?}"));
        assert!(rows.next().is_none(), "one row per typed text");
        row.2.clone()
    };
    let text = |key: &str| Some(Localizer::current().get(key));
    assert_eq!(error("", false), text("dialog.rename.error.empty"));
    assert_eq!(error("a?", false), text("dialog.rename.error.invalidCharacter"));
    assert_eq!(error("a.", false), text("dialog.rename.error.trailingDot"));
    assert_eq!(error("CON", false), text("dialog.rename.error.reservedName"));
    assert_eq!(error(&"x".repeat(300), false), text("dialog.rename.error.tooLong"));
    let exists_text = |name: &str| {
        Some(Localizer::current().format(
            "dialog.rename.error.exists",
            &[pr_l10n::template::LocArg::new("name", name)],
        ))
    };
    assert_eq!(error("b", false), exists_text("b.jpg"), "another listed photo");
    assert_eq!(error("a", false), None, "the file itself");
    assert_eq!(
        error("A", false),
        None,
        "a change of letter case only is the file itself, not a clash"
    );
    assert_eq!(error("late", false), None, "typing does not ask the disk ...");
    assert_eq!(
        error("late", true),
        exists_text("late.jpg"),
        "... the confirming call does"
    );
    assert_eq!(error("fresh", true), None);
}

// C#: MainViewModelRenameTests.PromptRenameCurrentAsync_NoFolderOpen_NeverShowsTheBox
#[test]
fn prompt_rename_current_async_no_folder_open_never_shows_the_box() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    english();
    let mut env = MainEnv::new();

    let shown = prompt(&mut env, |_| Some("x".to_owned()));

    assert!(!shown, "no prepared prompt, so the shell never opens the box");
    assert_eq!(env.runner.pending(), 0);
}

// C#: MainViewModelRenameTests.PromptRenameCurrentAsync_ACaptureGroup_IsValidatedWithTheGroupsPlan_NotTheJpegAlone
#[test]
fn prompt_rename_current_async_a_capture_group_is_validated_with_the_groups_plan_not_the_jpeg_alone() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    english();
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_promptgroup", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    // The sidecar is not named after the photo, so the group cannot be re-spelled together.
    let group = CaptureGroup::new(
        files[0].clone(),
        join(&folder, "a.cr2"),
        Some(join(&folder, "unrelated.xmp")),
    )
    .unwrap_or_else(|error| panic!("group: {error:?}"));
    let mut first = CatalogEntry::new(files[0].clone()).unwrap_or_else(|error| panic!("entry: {error:?}"));
    first.capture_group = Some(group);
    let second = CatalogEntry::new(files[1].clone()).unwrap_or_else(|error| panic!("entry: {error:?}"));
    env.vm
        .catalog_mut_for_tests()
        .reset(vec![first, second], RawPairMode::Separate, None);
    env.vm.catalog_mut_for_tests().set_current(0);
    let error: Mutex<Option<String>> = Mutex::new(None);

    prompt(&mut env, |request| {
        *error.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = (request.validate)("fresh", false);
        None
    });

    assert_eq!(
        error.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner),
        Some(Localizer::current().get("dialog.rename.error.sidecarNotMappable"))
    );
}
