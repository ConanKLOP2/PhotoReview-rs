//! W8-08d: `MainViewModel` settings dialog entry and the reloads it triggers, window close, instance label, title bar, folder line and the
//! live language switch (`contracts/W8-08d.md`, C# `MainViewModel.cs` lines 855 to 1085). Each test names the C# test it ports
//! (`// C#: Class.Method`) or is marked `// New test`. The window is the whole `MainViewModel` (`support/main_env.rs`); the Settings dialog
//! is a fake that edits the settings store when "opened", like the real dialog saves.
//!
//! Not ported here, with the reason: `HotPath/TitleBarAndExifLineCostTests` (a perf harness) and `OpenFolderAfterCloseSession_IsIgnored`
//! (already ported as `an_open_on_a_closed_window_is_ignored` in `viewmodels_main_navigation.rs`, where `close_session` is first used).

mod file_action_fakes;
mod support {
    pub mod main_env;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pr_app::viewmodels::main::MainProperty;
use pr_core::abstractions::SettingsTarget;
use pr_core::instance::ownership::FolderOwnershipError;
use pr_core::model::{
    DecoderBackend, ImageSortMode, LoadingMode, RawFullDecode, RawPairMode, ScalingQuality, TitleBarFields,
};
use pr_core::session::store::SessionState;
use pr_core::settings::app_settings::AppSettings;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_l10n::template::LocArg;
use support::main_env::{EnvOptions, MainEnv, RecordingOwnership, en_with};
use support::preview_fakes::{Gate, lock, sized_image};

// ---- helpers ----------------------------------------------------------------------------------------------------------------

/// The Settings dialog saves: "opening" it applies `change` to the settings store and answers "saved".
fn dialog_saves(env: &MainEnv, change: impl Fn(&mut AppSettings) + Send + 'static) {
    let store = Arc::clone(&env.store);
    env.dialog.on_settings(move |_target| {
        support::main_env::update_settings(&store, &change);
        true
    });
}

/// C# `OpenRawFolderAsync`: `1.png` and `2.cr2` (two separate photos) opened with RAW support on / off and the given pair mode.
fn raw_env(pair_mode: RawPairMode, raw_support: bool) -> (MainEnv, String, Vec<String>) {
    let mut env = MainEnv::with_options(EnvOptions {
        settings: Box::new(move |settings| {
            settings.raw_support_enabled = raw_support;
            settings.raw_pair_mode = pair_mode;
        }),
        ..EnvOptions::default()
    });
    let (folder, files) = env.images("raw_folder", &["1.png", "2.cr2"]);
    env.open(&folder);
    (env, folder, files)
}

/// A folder with `a.jpg + a.cr2` and RAW support on (`PreferJpeg`), opened.
fn pair_env(extra: impl FnOnce(&mut AppSettings) + 'static) -> (MainEnv, Vec<String>) {
    let mut env = MainEnv::with_options(EnvOptions {
        settings: Box::new(move |settings| {
            settings.raw_support_enabled = true;
            settings.raw_pair_mode = RawPairMode::PreferJpeg;
            extra(settings);
        }),
        ..EnvOptions::default()
    });
    let (folder, files) = env.images("pair_folder", &["a.jpg", "a.cr2"]);
    env.open(&folder);
    (env, files)
}

// ---- the dialog: what a changed setting resets -------------------------------------------------------------------------------

#[test]
fn show_settings_when_the_loading_mode_changed_cancels_preload_and_presents_again() {
    // C#: AdvancedTests.ShowSettings_WhenLoadingModeChanged_CancelsPreloadAndPresentsAgain
    let mut env = MainEnv::new();
    let (folder, _) = env.images("settings_folder", &["1.png"]);
    env.open(&folder);
    env.preload.clear_events();
    let presentations = lock(&env.sink_seen).presented.len();
    dialog_saves(&env, |settings| settings.loading_mode = Some(LoadingMode::Original));

    env.vm.show_settings();
    env.settle();

    assert_eq!(env.preload.count_of("cancel"), 1);
    assert!(
        lock(&env.sink_seen).presented.len() > presentations,
        "the current image was presented again"
    );
}

#[test]
fn show_settings_when_only_the_decoder_backend_changed_cancels_preload_and_drops_the_preloaded_keys() {
    // C#: MutationGaps.ShowSettings_WhenOnlyTheDecoderBackendChanged_CancelsPreloadAndDropsThePreloadedKeys
    let mut env = MainEnv::new();
    let folder = env.folder("settings_backend_only");
    env.add_image(&folder, "1.png");
    env.open(&folder);
    env.preload.clear_events();
    dialog_saves(&env, |settings| settings.decoder_backend = Some(DecoderBackend::Wpf));

    env.vm.show_settings();

    assert_eq!(env.preload.count_of("cancel"), 1);
    assert_eq!(env.preload.count_of("clear"), 1);
    assert_eq!(
        env.runner.count_of("preview-disk-clear"),
        1,
        "R2-F-16: the disk cache is cleared on a worker, never on the UI thread"
    );
    env.settle();
    assert_eq!(env.runner.count_of("preview-disk-clear"), 0);
}

#[test]
fn show_settings_when_only_the_loading_mode_changed_keeps_the_preloaded_keys() {
    // C#: MutationGaps.ShowSettings_WhenOnlyTheLoadingModeChanged_KeepsThePreloadedKeys
    let mut env = MainEnv::new();
    let folder = env.folder("settings_mode_only");
    env.add_image(&folder, "1.png");
    env.open(&folder);
    env.preload.clear_events();
    dialog_saves(&env, |settings| settings.loading_mode = Some(LoadingMode::Original));

    env.vm.show_settings();
    env.settle();

    assert_eq!(env.preload.count_of("cancel"), 1);
    assert_eq!(env.preload.count_of("clear"), 0);
    assert_eq!(env.runner.count_of("preview-disk-clear"), 0);
}

#[test]
fn show_settings_when_the_zoom_step_changed_applies_it_to_the_viewer() {
    // C#: MutationGaps.ShowSettings_WhenTheZoomStepChanged_AppliesItToTheViewer (Q-R41)
    let mut env = MainEnv::new();
    dialog_saves(&env, |settings| settings.keyboard_zoom_step_percent = 50);

    let changes = env.vm.show_settings();

    assert!((env.viewer.borrow().zoom_step() - 0.5).abs() < 1e-9);
    assert!(
        changes.main.contains(MainProperty::IsExifLineVisible),
        "the EXIF line is refreshed"
    );
}

#[test]
fn show_settings_when_the_dialog_reports_no_change_does_nothing() {
    // New test (the dialog was cancelled: `false` means nothing was saved)
    let mut env = MainEnv::new();
    let folder = env.folder("settings_cancel");
    env.add_image(&folder, "1.png");
    env.open(&folder);
    env.preload.clear_events();
    let title = env.vm.folder_title().to_owned();
    env.dialog.on_settings(|_| false);

    let changes = env.vm.show_settings();

    assert!(changes.is_empty());
    assert!(env.preload.events().is_empty());
    assert_eq!(env.vm.folder_title(), title);
    assert!(env.vm.is_idle());
    assert_eq!(
        lock(&env.dialog.settings_targets).clone(),
        vec![SettingsTarget::Default]
    );
}

#[test]
fn show_settings_with_no_folder_open_does_not_re_present_anything() {
    // C#: AppGaps.ShowSettings_WithNoFolderOpen_DoesNotRePresentAnything
    let mut env = MainEnv::new();
    dialog_saves(&env, |settings| settings.loading_mode = Some(LoadingMode::Original));

    env.vm.show_settings();

    assert_eq!(env.runner.pending(), 0, "nothing was scheduled");
    assert!(lock(&env.sink_seen).presented.is_empty());
    assert!(env.vm.is_idle());
}

#[test]
fn show_settings_clears_a_provisional_override() {
    // New test (T3: an override left by a failed save must not hide the values the dialog saves)
    let mut env = MainEnv::new();
    let config = env.paths.config_file.clone();
    let _ = std::fs::remove_file(&config);
    std::fs::create_dir_all(&config).unwrap_or_else(|error| panic!("block the settings file: {error}"));
    env.vm.toggle_info_overlay(); // the save fails: the toggle stays as a provisional override
    assert!(env.settings.has_override());
    std::fs::remove_dir_all(&config).unwrap_or_else(|error| panic!("unblock the settings file: {error}"));
    dialog_saves(&env, |settings| settings.show_info_overlay = true);

    env.vm.show_settings();

    assert!(
        !env.settings.has_override(),
        "a changed dialog clears the provisional override"
    );
    assert!(
        env.vm.settings().show_info_overlay,
        "the view model now reads the saved value"
    );
}

#[test]
fn show_settings_compares_the_store_values_not_the_override() {
    // New test (T1: the "previous" snapshot comes from the store; an installed override would hide the change)
    let mut env = MainEnv::new();
    env.update_settings(|settings| settings.raw_support_enabled = false);
    let folder = env.folder("settings_store_vs_override");
    env.add_image(&folder, "1.png");
    env.add_image(&folder, "2.cr2");
    env.open(&folder);
    assert_eq!(env.vm.total_files(), 1, "RAW support is off in the store");
    env.vm.set_settings(Arc::new(AppSettings {
        raw_support_enabled: true, // the override already says "on"
        ..(*env.vm.settings()).clone()
    }));
    dialog_saves(&env, |settings| settings.raw_support_enabled = true);

    env.vm.show_settings();
    env.settle();

    assert_eq!(
        env.vm.total_files(),
        2,
        "store: off -> on is a change, so the folder was reloaded"
    );
}

#[test]
fn show_settings_reports_the_capture_pair_tooltip() {
    // C#: RawPair.CapturePairBadgeToolTip_AfterSettingsAssignTheKey_IsRefreshed
    let mut env = MainEnv::new();
    let before = env.vm.capture_pair_badge_tool_tip();
    dialog_saves(&env, |settings| {
        settings.shortcuts.toggle_capture_member = "F4".to_owned()
    });

    let changes = env.vm.show_settings();

    assert!(changes.main.contains(MainProperty::CapturePairBadgeToolTip));
    assert_ne!(env.vm.capture_pair_badge_tool_tip(), before);
    assert_eq!(
        env.vm.capture_pair_badge_tool_tip(),
        en_with("main.capturePair.badge.tooltipBound", &[LocArg::new("key", "F4")])
    );
}

#[test]
fn open_in_external_editor_with_no_editor_configured_opens_settings_on_the_external_editor_field() {
    // C#: AdvancedTests.OpenInExternalEditor_NoEditorConfigured_OpensSettingsOnExternalEditorField (listed with W8-08c, needs `show_settings_at`)
    let mut env = MainEnv::new();
    let folder = env.folder("editor_not_configured");
    env.add_image(&folder, "photo.png");
    env.update_settings(|settings| settings.external_editor_path = String::new());
    env.open(&folder);

    env.vm
        .open_in_external_editor()
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(lock(&env.dialog.errors).is_empty());
    assert_eq!(
        lock(&env.dialog.settings_targets).clone(),
        vec![SettingsTarget::ExternalEditor]
    );
}

// ---- reloads ----------------------------------------------------------------------------------------------------------------

#[test]
fn show_settings_when_raw_support_turned_off_reloads_the_folder_so_no_raw_entry_stays_listed() {
    // C#: AdvancedTests.ShowSettings_WhenRawSupportTurnedOff_ReloadsTheFolderSoNoRawEntryStaysListed
    let (mut env, _, _) = raw_env(RawPairMode::Separate, true);
    assert_eq!(env.vm.total_files(), 2);
    env.preload.clear_events();
    dialog_saves(&env, |settings| settings.raw_support_enabled = false);

    env.vm.show_settings();
    env.settle();

    assert_eq!(
        env.vm.total_files(),
        1,
        "reloaded with RAW hidden; without the reload the RAW entry would stay listed"
    );
    assert!(env.preload.count_of("cancel") >= 1);
    assert!(
        env.preload.count_of("clear") >= 1,
        "the preloaded keys of the old listing are dropped"
    );
}

#[test]
fn show_settings_when_raw_support_unchanged_does_not_reload_the_folder() {
    // C#: AdvancedTests.ShowSettings_WhenRawSupportUnchanged_DoesNotReloadTheFolder
    let (mut env, _, _) = raw_env(RawPairMode::Separate, true);
    let epoch = env.vm.folder_load_epoch();
    dialog_saves(&env, |_| {});

    env.vm.show_settings();

    assert_eq!(env.vm.folder_load_epoch(), epoch);
    assert_eq!(env.runner.pending(), 0);
}

fn pair_mode_change_reloads(before: RawPairMode, after: RawPairMode) {
    let (mut env, _, _) = raw_env(before, true);
    let epoch = env.vm.folder_load_epoch();
    dialog_saves(&env, move |settings| settings.raw_pair_mode = after);

    env.vm.show_settings();

    assert_eq!(env.vm.folder_load_epoch(), epoch + 1, "the folder is reloaded");
    env.settle();
}

#[test]
fn show_settings_when_the_raw_pair_mode_changed_from_separate_to_prefer_jpeg_while_raw_is_on_reloads_the_folder() {
    // C#: RawSettings.ShowSettings_WhenRawPairModeChangedWhileRawIsOn_ReloadsTheFolder (Separate -> PreferJpeg)
    pair_mode_change_reloads(RawPairMode::Separate, RawPairMode::PreferJpeg);
}

#[test]
fn show_settings_when_the_raw_pair_mode_changed_from_prefer_jpeg_to_separate_while_raw_is_on_reloads_the_folder() {
    // C#: RawSettings.ShowSettings_WhenRawPairModeChangedWhileRawIsOn_ReloadsTheFolder (PreferJpeg -> Separate)
    pair_mode_change_reloads(RawPairMode::PreferJpeg, RawPairMode::Separate);
}

#[test]
fn show_settings_changing_raw_support_with_no_folder_open_resets_nothing() {
    // New test (the reload needs a session folder: without one a RAW switch has nothing to reload and no cache to drop)
    let mut env = MainEnv::new();
    env.update_settings(|settings| settings.raw_support_enabled = true);
    env.preload.clear_events();
    dialog_saves(&env, |settings| settings.raw_support_enabled = false);

    env.vm.show_settings();

    assert!(env.preload.events().is_empty(), "{:?}", env.preload.events());
    assert_eq!(env.runner.pending(), 0);
    assert_eq!(env.vm.folder_load_epoch(), 0);
}

#[test]
fn show_settings_when_the_raw_pair_mode_changed_while_raw_is_off_does_not_reload() {
    // C#: RawSettings.ShowSettings_WhenRawPairModeChangedWhileRawIsOff_DoesNotReload
    let (mut env, _, _) = raw_env(RawPairMode::Separate, false);
    let epoch = env.vm.folder_load_epoch();
    dialog_saves(&env, |settings| settings.raw_pair_mode = RawPairMode::PreferJpeg);

    env.vm.show_settings();

    assert_eq!(env.vm.folder_load_epoch(), epoch);
    assert_eq!(env.runner.pending(), 0);
}

#[test]
fn show_settings_when_the_image_sort_mode_changed_reloads_the_folder_at_the_current_image() {
    // C#: SortMode.ShowSettings_ImageSortModeChanged_ReloadsTheFolderAtTheCurrentImage
    let (mut env, folder, _) = raw_env(RawPairMode::Separate, false);
    let current = env.vm.presenter().current_presented_path().map(str::to_owned);
    assert!(current.is_some());
    let epoch = env.vm.folder_load_epoch();
    let ownership = RecordingOwnership::new();
    env.vm.set_folder_ownership(Some(ownership.clone()));
    dialog_saves(&env, |settings| {
        settings.image_sort_mode = Some(ImageSortMode::SizeAscending)
    });

    env.vm.show_settings();
    env.settle();

    assert_eq!(
        env.vm.folder_load_epoch(),
        epoch + 1,
        "reloaded so the new order applies to the open folder"
    );
    assert_eq!(lock(&ownership.before_opens).clone(), vec![folder]);
    assert_eq!(
        lock(&ownership.initial_paths).clone(),
        vec![current],
        "the current image stays the one shown"
    );
}

fn webp_heic_switch(enabled_before: bool, expected_after: usize) {
    let mut env = MainEnv::with_options(EnvOptions {
        settings: Box::new(move |settings| settings.webp_heic_support_enabled = enabled_before),
        ..EnvOptions::default()
    });
    let (folder, _) = env.images(
        &format!("webp_heic_toggle_{enabled_before}"),
        &["1.png", "2.webp", "3.heic"],
    );
    env.open(&folder);
    assert_eq!(env.vm.total_files(), if enabled_before { 3 } else { 1 });
    dialog_saves(&env, move |settings| {
        settings.webp_heic_support_enabled = !enabled_before
    });

    env.vm.show_settings();
    env.settle();

    assert_eq!(env.vm.total_files(), expected_after);
}

#[test]
fn show_settings_when_the_webp_heic_switch_is_turned_off_reloads_the_folder_listing() {
    // C#: WebpHeic.ShowSettings_WhenWebpHeicSwitchChanges_ReloadsTheFolderListing (true, 1)
    webp_heic_switch(true, 1);
}

#[test]
fn show_settings_when_the_webp_heic_switch_is_turned_on_reloads_the_folder_listing() {
    // C#: WebpHeic.ShowSettings_WhenWebpHeicSwitchChanges_ReloadsTheFolderListing (false, 3)
    webp_heic_switch(false, 3);
}

#[test]
fn show_settings_folder_reload_opens_at_the_currently_presented_capture_member() {
    // C#: RawSettings.ShowSettings_FolderReload_OpensAtTheCurrentlyPresentedCaptureMember
    let (mut env, files) = pair_env(|_| {});
    env.vm.toggle_capture_group_member();
    env.settle();
    assert_eq!(env.vm.presenter().current_presented_path(), Some(files[1].as_str()));
    let ownership = RecordingOwnership::new();
    env.vm.set_folder_ownership(Some(ownership.clone()));
    dialog_saves(&env, |settings| settings.raw_pair_mode = RawPairMode::Separate);

    env.vm.show_settings();
    env.settle();

    assert_eq!(
        lock(&ownership.initial_paths).clone(),
        vec![Some(files[1].clone())],
        "not the entry's representative JPEG"
    );
}

#[test]
fn show_settings_raw_support_turned_off_while_a_raw_member_is_shown_reloads_at_the_jpeg_member() {
    // C#: RawSettings.ShowSettings_RawSupportTurnedOffWhileRawMemberShown_ReloadsAtTheJpegMember
    let (mut env, files) = pair_env(|_| {});
    env.vm.toggle_capture_group_member();
    env.settle();
    assert_eq!(env.vm.presenter().current_presented_path(), Some(files[1].as_str()));
    let ownership = RecordingOwnership::new();
    env.vm.set_folder_ownership(Some(ownership.clone()));
    dialog_saves(&env, |settings| settings.raw_support_enabled = false);

    env.vm.show_settings();
    env.settle();

    assert_eq!(
        lock(&ownership.initial_paths).clone(),
        vec![Some(files[0].clone())],
        "the RAW member is no longer listed after the reload"
    );
}

#[test]
fn show_settings_raw_support_toggled_with_other_changes_still_applies_the_shared_settings() {
    // C#: RawSettings.ShowSettings_WhenRawSupportToggledWithOtherChanges_StillAppliesTheSharedSettings
    let (mut env, _, _) = raw_env(RawPairMode::Separate, true);
    assert_ne!(env.viewer.borrow().scaling_quality(), ScalingQuality::Linear);
    dialog_saves(&env, |settings| {
        settings.raw_support_enabled = false;
        settings.scaling_quality = Some(ScalingQuality::Linear);
    });

    env.vm.show_settings();
    env.settle();

    assert_eq!(
        env.viewer.borrow().scaling_quality(),
        ScalingQuality::Linear,
        "was skipped by the early return"
    );
    assert_eq!(env.vm.total_files(), 1);
}

fn raw_full_decode_changed_keeps_member_and_compare(compare_open: bool) {
    let (mut env, files) = pair_env(|settings| settings.raw_full_decode = RawFullDecode::Never);
    if compare_open {
        env.vm.toggle_compare();
        env.settle();
    } else {
        env.vm.toggle_capture_group_member();
        env.settle();
        assert_eq!(env.vm.presenter().current_presented_path(), Some(files[1].as_str()));
    }
    let generation_before = env.clock.current_navigation();
    dialog_saves(&env, |settings| settings.raw_full_decode = RawFullDecode::OnZoom);

    let changes = env.vm.show_settings();
    env.settle();

    // A refresh that does nothing would also leave the member / compare state "kept": prove the image was presented again.
    assert!(env.clock.current_navigation() > generation_before);
    if compare_open {
        assert!(env.vm.compare().is_visible());
        assert_eq!(env.vm.compare().left_path(), Some(files[0].as_str()));
        assert_eq!(env.vm.compare().right_path(), Some(files[1].as_str()));
    } else {
        assert_eq!(env.vm.presenter().current_presented_path(), Some(files[1].as_str()));
        assert_eq!(env.vm.capture_pair_badge(), env.text("main.capturePair.badge.raw"));
    }
    assert!(
        changes.main.contains(MainProperty::CapturePairBadge)
            || env.changes.main.contains(MainProperty::CapturePairBadge)
    );
}

#[test]
fn show_settings_raw_full_decode_changed_keeps_the_toggled_capture_member() {
    // C#: RawSettings.ShowSettings_RawFullDecodeChanged_KeepsTheToggledCaptureMemberAndCompareState (compareOpen false)
    raw_full_decode_changed_keeps_member_and_compare(false);
}

#[test]
fn show_settings_raw_full_decode_changed_keeps_the_compare_state() {
    // C#: RawSettings.ShowSettings_RawFullDecodeChanged_KeepsTheToggledCaptureMemberAndCompareState (compareOpen true)
    raw_full_decode_changed_keeps_member_and_compare(true);
}

#[test]
fn show_settings_reload_while_a_file_action_is_in_flight_waits_for_the_action_then_reloads() {
    // C#: RawSettings.ShowSettings_ReloadWhileAFileActionIsInFlight_WaitsForTheActionThenReloads
    let (mut env, _, _) = raw_env(RawPairMode::Separate, true);
    env.vm.recycle(); // the current photo goes to the bin on its worker (held)
    env.settle_holding("the recycle to be in flight", &["FileAction"], |_| false);
    let epoch = env.vm.folder_load_epoch();
    dialog_saves(&env, |settings| settings.raw_pair_mode = RawPairMode::PreferJpeg);

    env.vm.show_settings();

    assert_eq!(
        env.vm.folder_load_epoch(),
        epoch,
        "no reload while the action runs (it would make the action late)"
    );
    assert!(!env.vm.is_idle());
    env.settle();
    assert_eq!(env.vm.folder_load_epoch(), epoch + 1);
    assert_eq!(
        env.vm.total_files(),
        1,
        "reloaded from disk after the recycle: only the RAW is left"
    );
}

#[test]
fn show_settings_reload_failure_is_observed_and_does_not_fault_the_refresh_task() {
    // C#: RawSettings.ShowSettings_ReloadFailure_IsObservedAndDoesNotFaultTheRefreshTask
    let (mut env, _, _) = raw_env(RawPairMode::Separate, true);
    let ownership = RecordingOwnership::new();
    *lock(&ownership.fail_with) = Some(FolderOwnershipError::InvalidFolder);
    env.vm.set_folder_ownership(Some(ownership.clone()));
    dialog_saves(&env, |settings| settings.raw_pair_mode = RawPairMode::PreferJpeg);

    env.vm.show_settings();
    env.settle(); // would hang if the failure left the open outstanding

    assert!(env.vm.is_idle());
    assert!(
        env.log
            .contains(pr_test_support::LogLevel::Error, "The folder ownership check failed"),
        "{:?}",
        env.log.entries()
    );
    assert_eq!(env.vm.total_files(), 2, "the open failed: the old listing stays");
}

#[test]
fn two_reloads_waiting_for_the_gate_each_open_once() {
    // New test (note 4: each waiting reload opens once)
    let (mut env, folder, _) = raw_env(RawPairMode::Separate, true);
    env.vm.recycle();
    env.settle_holding("the recycle to be in flight", &["FileAction"], |_| false);
    let ownership = RecordingOwnership::new();
    env.vm.set_folder_ownership(Some(ownership.clone()));
    dialog_saves(&env, |settings| settings.raw_pair_mode = RawPairMode::PreferJpeg);
    env.vm.show_settings();
    dialog_saves(&env, |settings| settings.raw_pair_mode = RawPairMode::PreferRaw);
    env.vm.show_settings();
    assert!(
        lock(&ownership.before_opens).is_empty(),
        "both reloads wait for the action"
    );

    env.settle();

    assert_eq!(lock(&ownership.before_opens).clone(), vec![folder.clone(), folder]);
    assert!(env.vm.is_idle());
}

// ---- RAW full decode (needs the presenter and the zoom loader) --------------------------------------------------------------

/// A RAW's ordinary decode: always the 2000 px embedded JPEG of a 6000 x 4000 sensor, whatever the box (C# `RawEmbeddedDecoder`).
#[derive(Default)]
struct RawEmbedded {
    decodes: AtomicUsize,
}

impl ImageDecoder for RawEmbedded {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.decodes.fetch_add(1, Ordering::SeqCst);
        Ok(sized_image(2000, 1333, 6000, 4000, true))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(6000, 4000))
    }
}

/// The RAW full decoder: 6016 x 4016 (the LibRaw sensor area), every decode held on a gate (C# `RawFullDecoderFake`).
struct RawFull {
    decodes: AtomicUsize,
    gate: Arc<Gate>,
}

impl ImageDecoder for RawFull {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        if !self.gate.wait() {
            return Err(DecodeError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the full decode gate was never opened",
            )));
        }
        self.decodes.fetch_add(1, Ordering::SeqCst);
        Ok(sized_image(6016, 4016, 6016, 4016, false))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(6000, 4000))
    }
}

fn raw_zoom_env(mode: LoadingMode, setting: RawFullDecode) -> (MainEnv, Arc<RawEmbedded>, Arc<RawFull>) {
    let embedded = Arc::new(RawEmbedded::default());
    let full = Arc::new(RawFull {
        decodes: AtomicUsize::new(0),
        gate: Gate::new(),
    });
    let mut env = MainEnv::with_options(EnvOptions {
        decoder: embedded.clone(),
        raw_full_decoder: Some(full.clone()),
        settings: Box::new(move |settings| {
            settings.loading_mode = Some(mode);
            settings.raw_full_decode = setting;
        }),
        catalog_builder: Some(Box::new(|photos: &Path| {
            let folder = photos.join("raw_zoom");
            std::fs::create_dir_all(&folder).unwrap_or_else(|error| panic!("create: {error}"));
            let path = folder.join("raw.cr2");
            std::fs::write(&path, [0_u8; 64]).unwrap_or_else(|error| panic!("write: {error}"));
            let mut catalog = pr_core::catalog::review_catalog::ReviewCatalog::new();
            catalog.reset(
                vec![
                    pr_core::catalog::types::CatalogEntry::new(path.to_string_lossy().into_owned())
                        .unwrap_or_else(|error| panic!("{error:?}")),
                ],
                RawPairMode::Separate,
                None,
            );
            catalog
        })),
        ..EnvOptions::default()
    });
    env.vm.first();
    env.settle();
    (env, embedded, full)
}

#[test]
fn show_settings_raw_full_decode_never_to_on_zoom_in_original_mode_starts_one_full_decode() {
    // C#: ZoomSwapTests.ShowSettings_RawFullDecodeNeverToOnZoom_OriginalMode_ZoomStartsOneFullDecode
    let (mut env, _, full) = raw_zoom_env(LoadingMode::Original, RawFullDecode::Never);
    env.vm.zoom_actual_size();
    assert!(!env.vm.presenter().zoom_detail().pending_load(), "Never: nothing armed");

    dialog_saves(&env, |settings| settings.raw_full_decode = RawFullDecode::OnZoom);
    env.vm.show_settings();
    // The view is still zoomed to 100 %, so the re-armed target starts its (gated) decode at once; the C# test awaits only the present.
    env.settle_until("the re-present to arm the zoom target", |env| {
        env.vm.presenter().zoom_detail().pending_load()
    });
    env.vm.toggle_fit();
    env.vm.zoom_actual_size();
    assert!(env.vm.presenter().zoom_detail().pending_load());
    full.gate.open();
    env.settle();

    assert_eq!(full.decodes.load(Ordering::SeqCst), 1);
    assert!(env.vm.presenter().zoom_detail().held_original().is_some());
}

#[test]
fn show_settings_raw_full_decode_on_zoom_to_never_drops_the_held_original_and_never_decodes_or_swaps_again() {
    // C#: ZoomSwapTests.ShowSettings_RawFullDecodeOnZoomToNever_DropsHeldOriginalAndNeverDecodesOrSwapsAgain
    let (mut env, _, full) = raw_zoom_env(LoadingMode::Preview, RawFullDecode::OnZoom);
    env.vm.zoom_actual_size();
    assert!(env.vm.presenter().zoom_detail().pending_load());
    full.gate.open();
    env.settle();
    assert!(env.vm.presenter().zoom_detail().held_original().is_some());
    assert_eq!(full.decodes.load(Ordering::SeqCst), 1);

    dialog_saves(&env, |settings| settings.raw_full_decode = RawFullDecode::Never);
    env.vm.show_settings();
    env.settle();
    let swaps = std::rc::Rc::new(std::cell::Cell::new(0));
    let counter = std::rc::Rc::clone(&swaps);
    env.bridge
        .set_source_size_swapping_hook(Some(Box::new(move |_| counter.set(counter.get() + 1))));
    env.vm.zoom_actual_size();
    env.vm.zoom_in();
    env.settle();

    assert!(env.vm.presenter().zoom_detail().held_original().is_none());
    assert!(!env.vm.presenter().zoom_detail().is_showing_original());
    assert!(!env.vm.presenter().zoom_detail().pending_load());
    assert_eq!(full.decodes.load(Ordering::SeqCst), 1);
    assert_eq!(swaps.get(), 0);
}

#[test]
fn show_settings_raw_full_decode_on_zoom_to_never_while_the_target_is_armed_but_not_decoded_drops_the_target() {
    // C#: ZoomSwapTests.ShowSettings_RawFullDecodeOnZoomToNever_WhileTargetArmedButNotDecoded_DropsTheTarget
    let (mut env, _, full) = raw_zoom_env(LoadingMode::Preview, RawFullDecode::OnZoom); // target armed, still at Fit

    dialog_saves(&env, |settings| settings.raw_full_decode = RawFullDecode::Never);
    env.vm.show_settings();
    env.settle();
    env.vm.zoom_actual_size();

    assert!(!env.vm.presenter().zoom_detail().pending_load());
    assert_eq!(full.decodes.load(Ordering::SeqCst), 0);
}

// ---- close ------------------------------------------------------------------------------------------------------------------

#[test]
fn close_session_writes_pending_state_and_disposes_the_writer() {
    // C#: MainViewModelFileActionTests.CloseSession_WritesPendingState_AndDisposesWriter (R7-3)
    let mut env = MainEnv::new();
    let folder = env.folder("close_session");
    let a = Path::new(&folder).join("a.jpg").to_string_lossy().into_owned();
    let b = Path::new(&folder).join("b.jpg").to_string_lossy().into_owned();
    let mut state = SessionState::new(folder.clone());
    state.current_path = Some(a.clone());
    env.session_writer
        .update(&state)
        .unwrap_or_else(|error| panic!("update: {error}"));

    env.vm.close_session();

    let saved = env
        .sessions
        .load(&folder)
        .unwrap_or_else(|error| panic!("load: {error}"));
    assert_eq!(saved.current_path, Some(a.clone()));
    // Disposed (Q-R5 bounded path), not just flushed: a late update after close is not written.
    let mut late = SessionState::new(folder.clone());
    late.current_path = Some(b);
    env.session_writer
        .update(&late)
        .unwrap_or_else(|error| panic!("update: {error}"));
    env.session_writer.flush();
    let after = env
        .sessions
        .load(&folder)
        .unwrap_or_else(|error| panic!("load: {error}"));
    assert_eq!(after.current_path, Some(a));
}

#[test]
fn close_session_disposes_the_folder_coordinator_so_no_further_load_starts() {
    // C#: MainViewModelFileActionTests.CloseSession_DisposesFolderCoordinator_SoNoFurtherLoadStarts
    let mut env = MainEnv::new();
    let folder = env.folder("close_coordinator");
    env.add_image(&folder, "a.jpg");

    env.vm.close_session();
    env.vm
        .open_folder(&folder, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle();

    // The folder holds a photo, so a load that did start would report one file: 0 proves it never started.
    assert_eq!(env.vm.total_files(), 0);
    assert!(env.vm.session().is_none());
    assert!(env.vm.is_closed());

    // Positive control: the same folder opens normally on a window that was not closed.
    let mut open = MainEnv::new();
    let other = open.folder("close_coordinator");
    open.add_image(&other, "a.jpg");
    open.open(&other);
    assert_eq!(open.vm.total_files(), 1);
}

#[test]
fn close_session_finishes_a_pending_open_once() {
    // New test (note 5: a load the disposed coordinator drops can never finish by itself; exactly one AfterOpen per started open)
    let mut env = MainEnv::new();
    let folder = env.folder("close_pending");
    env.add_image(&folder, "a.jpg");
    let ownership = RecordingOwnership::new();
    env.vm.set_folder_ownership(Some(ownership.clone()));
    env.vm
        .open_folder(&folder, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    assert!(env.runner.run_tagged("folder-ownership"));
    env.pump(); // granted: the load starts (its scan job is queued, not run)
    assert!(!env.vm.is_idle());

    env.vm.close_session();
    env.settle();

    assert_eq!(lock(&ownership.after_opens).clone(), vec![(folder, None)]);
    assert!(env.vm.is_idle());
    assert_eq!(env.vm.total_files(), 0);
}

#[test]
fn close_session_releases_the_parked_commands() {
    // New test (C# `Dispose` completes the order gate: the parked navigation resumes, O1)
    let mut env = MainEnv::new();
    let (folder, files) = env.images("close_parked", &["1.png", "2.png", "3.png"]);
    env.explorer.set_order(files.clone());
    env.vm
        .open_folder(&folder, Some(&files[1]))
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("the opened file to be presented", &["Explorer"], |_| false);
    env.vm.last();
    assert_eq!(env.vm.current_index(), 1, "parked behind the Explorer order");

    env.vm.close_session();

    assert_eq!(
        env.vm.current_index(),
        2,
        "the dispose released the gate and the parked command ran"
    );
}

// ---- title and language -----------------------------------------------------------------------------------------------------

fn title_env(fields: TitleBarFields, logging: bool) -> (MainEnv, String) {
    let mut env = MainEnv::new();
    let (folder, _) = env.images("title_fields", &["a.jpg", "b.jpg"]);
    env.vm.set_settings(Arc::new(AppSettings {
        loading_mode: Some(LoadingMode::Preview),
        logging_enabled: logging,
        title_bar_fields: fields,
        ..AppSettings::default()
    }));
    env.open(&folder);
    (env, folder)
}

#[test]
fn title_bar_fields_control_the_title_content_and_follow_navigation() {
    // C#: NavigationTests.TitleBarFields_ControlsTitleContent_AndFollowsNavigation
    let (mut env, folder) = title_env(TitleBarFields::INDEX_COUNT | TitleBarFields::FILE_NAME, false);

    let title = env.vm.folder_title().to_owned();
    assert!(title.contains("1/2"), "{title}");
    assert!(title.contains("a.jpg"), "{title}");
    assert!(
        !title.contains(&folder),
        "FolderPath / FolderName are both off: {title}"
    );

    env.vm.next();
    env.settle();

    let title = env.vm.folder_title().to_owned();
    assert!(title.contains("2/2") && title.contains("b.jpg"), "{title}");
    assert!(!title.contains("1/2") && !title.contains("a.jpg"), "{title}");
}

#[test]
fn title_bar_fields_keep_the_logging_marker() {
    // C#: NavigationTests.TitleBarFields_KeepsTheLoggingMarker
    let (env, _) = title_env(TitleBarFields::FILE_NAME, true);

    let title = env.vm.folder_title().to_owned();
    assert!(title.contains("a.jpg"), "{title}");
    assert!(title.contains("LOG"), "{title}");
}

#[test]
fn title_bar_with_nothing_selected_falls_back_to_the_folder_name() {
    // C#: NavigationTests.TitleBarFields_NoneSelected_FallsBackToFolderName
    let (env, _) = title_env(TitleBarFields::NONE, false);

    assert!(
        env.vm.folder_title().contains("title_fields"),
        "{}",
        env.vm.folder_title()
    );
}

#[test]
fn title_bar_with_the_dimensions_field_shows_the_presented_photos_size() {
    // C#: MutationGaps.TitleBar_WithTheDimensionsField_ShowsThePresentedPhotosSize
    let (mut env, _) = title_env(TitleBarFields::DIMENSIONS, false);

    let changes = env.vm.notify_presentation_changed();

    assert!(changes.main.contains(MainProperty::FolderTitle) || !env.vm.folder_title().is_empty());
    let title = env.vm.folder_title().to_owned();
    // The decoder reports 6000 x 4000; the text is the catalog's `exif.dimensions` with the invariant digits.
    assert!(title.contains("6000") && title.contains("4000"), "{title}");
}

#[test]
fn instance_label_prefixes_the_title_and_unset_leaves_it_unchanged() {
    // C#: NavigationTests.InstanceLabel_PrefixesTitle_UnsetLeavesTitleUnchanged
    let mut env = MainEnv::new();
    let (folder, _) = env.images("instance_label", &["a.jpg"]);

    // Unset (default, the normal user launch): today's exact title, before and after a folder loads.
    assert_eq!(env.vm.folder_title(), env.text("app.title"));
    env.open(&folder);
    let loaded_unset = env.vm.folder_title().to_owned();
    assert!(!loaded_unset.contains('['), "{loaded_unset}");

    // No folder open: the label still applies to the plain app title.
    let mut empty = MainEnv::new();
    let changes = empty.vm.set_instance_label(Some("LABEL".to_owned()));
    assert!(changes.main.contains(MainProperty::FolderTitle));
    assert_eq!(empty.vm.folder_title(), format!("[LABEL] {}", empty.text("app.title")));

    // Folder loaded: same prefix, same folder content as the unset case.
    env.vm.set_instance_label(Some("LABEL".to_owned()));
    assert!(env.vm.folder_title().starts_with("[LABEL] "));
    assert_eq!(env.vm.folder_title(), format!("[LABEL] {loaded_unset}"));
    assert_eq!(env.vm.instance_label(), Some("LABEL"));

    // Clearing it restores exactly today's behaviour again.
    env.vm.set_instance_label(None);
    assert_eq!(env.vm.folder_title(), loaded_unset);
    assert_eq!(env.vm.instance_label(), None);
}

#[test]
fn the_title_is_reported_only_when_it_changed() {
    // New test (C# `FolderTitle` is a `SetProperty`)
    let mut env = MainEnv::new();

    assert!(
        env.vm
            .set_instance_label(Some("X".to_owned()))
            .main
            .contains(MainProperty::FolderTitle)
    );
    assert!(
        !env.vm
            .set_instance_label(Some("X".to_owned()))
            .main
            .contains(MainProperty::FolderTitle)
    );
    assert!(
        env.vm
            .set_instance_label(Some(String::new()))
            .main
            .contains(MainProperty::FolderTitle),
        "an empty label is no label"
    );
    assert_eq!(env.vm.folder_title(), env.text("app.title"));
}

#[test]
fn refresh_localized_text_after_a_switch_to_english_rerenders_the_folder_text() {
    // C#: MainViewModelLanguageSwitchTests.RefreshLocalizedText_AfterSwitchToEnglish_RerendersFolderText (I18N, ADR 0006)
    let mut env = MainEnv::with_options(EnvOptions {
        vietnamese: true,
        ..EnvOptions::default()
    });
    let folder = env.folder("lang_switch");
    let session = SessionState::new(folder.clone());
    env.vm.replay_sink_calls_for_tests(|sink| {
        use pr_app::coordinators::traits::IFolderLoadSink;
        sink.on_catalog_ready(&folder, 3, &session);
        sink.on_order_applied(3, 0, false);
    });
    let vietnamese = format!("{folder}  (3 ảnh) · Explorer");
    assert_eq!(env.vm.folder_text(), vietnamese);

    env.format.use_language(false);
    let changes = env.vm.refresh_localized_text();

    assert!(!env.vm.folder_text().contains("ảnh"), "{}", env.vm.folder_text());
    assert!(env.vm.folder_text().starts_with(&folder));
    assert!(env.vm.is_explorer_order_applied());
    assert!(changes.main.contains(MainProperty::FolderText));
    assert!(
        !changes.main.contains(MainProperty::IsExplorerOrderApplied),
        "the order flag did not change, so it is not reported"
    );
    assert!(changes.main.contains(MainProperty::SkippedWarningText));
    assert!(changes.main.contains(MainProperty::CapturePairBadge));
    assert!(changes.main.contains(MainProperty::CapturePairBadgeToolTip));

    env.format.use_language(true);
    env.vm.refresh_localized_text();
    assert_eq!(env.vm.folder_text(), vietnamese);
}

#[test]
fn refresh_localized_text_keeps_the_event_status_text() {
    // New test (the event status text switches language with the next update, as C#)
    let mut env = MainEnv::new();
    env.vm.set_status_text("Action failed: boom");

    env.vm.refresh_localized_text();

    assert_eq!(env.vm.status_text(), "Action failed: boom");
}
