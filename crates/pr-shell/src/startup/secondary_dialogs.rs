//! The PRODUCTION `SecondaryWindowDialogs` (plan edge P-43: the unplanned W8-18w, folded into W8-17c). Contract: `contracts/W8-17c.md`.
//!
//! C# source: the dialog-building bodies of `Win32SecondaryWindowPort` (baseline `d92d6aae`, `Dialogs/Native/Win32SecondaryWindowPort.cs`: `ShowSettings` 90-115,
//! `ShowRecovery` 123-160, `TryReadRecoveryEntries` 163-175, `ShowBenchmark` 181-228, `ShowActionProfiles` 234-239, `ShowDiagnostics` 241-252, `ShowSkippedFiles`
//! 254-258, `ShowBatchReview` 260-264, `PromptCustomZoom` 266-270) and the `BenchmarkDialogSeams` record (21-27). The routing, the open list, the nested modal run and
//! `dispose` are `pr_platform_win::dialogs::secondary_window_port` (W8-18o); this type only builds each dialog from the services and says how to read its result.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::coordinators::zoom_detail::BackgroundRunner;
use pr_app::presenters::benchmark::{BenchmarkFileScan, BenchmarkSessionFactory, BenchmarkSummarySave};
use pr_app::presenters::recovery_presentation::RecoveryTimeFormatter;
use pr_app::presenters::secondary::NumberFormatting;
use pr_app::presenters::settings::load::SharedLocalization;
use pr_app::services::contracts::ClipboardService;
use pr_core::abstractions::{BatchReviewItem, FileSystem, Log, SettingsTarget, ShellIntegration, SkippedEntry};
use pr_core::catalog::types::LatestExplorerSnapshot;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::recovery_retry::RecoveryRetryService;
use pr_core::settings::app_settings::SettingsTexts;
use pr_core::settings::shortcuts::ReviewAction;
use pr_core::settings::store::SettingsStore;
use pr_core::updates::checker::UpdateCheck;
use pr_imaging::decoder_traits::ImageDecoderFactory;
use pr_l10n::localizer::Localizer;
use pr_platform_win::dialogs::action_profiles::DialogWarningSink;
use pr_platform_win::dialogs::benchmark::state::{OpenInExplorerFn, PickFolderFn};
use pr_platform_win::dialogs::file_dialog::ShellFileDialog;
use pr_platform_win::dialogs::folder_dialog::ShellFolderDialog;
use pr_platform_win::dialogs::native::chrome::NativeDialogChrome;
use pr_platform_win::dialogs::secondary_window_port::{ActionProfilesOpener, PreparedDialog, SecondaryWindowDialogs};
use pr_platform_win::dialogs::task_dialog::TaskDialogPort;
use pr_platform_win::win32::structs::Hwnd;

/// The services the C# port asked its `IServiceProvider` for with `GetService<T>()` (an absent one is `None`, exactly the C# `null` returns) and
/// `GetRequiredService<T>()` (fields that are not `Option`). Built from the shared graph (W8-09b registry) by W8-17e and handed to
/// [`ProductionSecondaryWindowDialogs::new`]; the settings of the current UI language are read through `localization` at use time.
pub struct DialogServices {
    /// `GetService<SettingsStore>()`: `None` = Settings opens nothing (`settings` answers `None`).
    pub settings_store: Option<Arc<SettingsStore>>,
    /// `GetService<LocalizationService>()`.
    pub localization: Option<SharedLocalization>,
    /// `GetService<IUpdateChecker>()`.
    pub update_checker: Option<Arc<dyn UpdateCheck>>,
    /// `GetService<IImageDecoderFactory>()`.
    pub decoder_factory: Option<Arc<dyn ImageDecoderFactory>>,
    /// `GetService<OperationJournal>()`: `None` = Recovery opens nothing.
    pub journal: Option<Arc<OperationJournal>>,
    /// `GetService<RecoveryRetryService>()`: `None` = the Retry button is never enabled.
    pub recovery_retry: Option<Arc<RecoveryRetryService>>,
    /// `GetService<IFileSystem>()` (always registered in production; the checker of the Recovery window is `RecoveryFileCheck::new(file_system)`).
    pub file_system: Arc<dyn FileSystem>,
    /// `GetService<ReviewMetrics>()`.
    pub metrics: Option<Arc<ReviewMetrics>>,
    /// `GetService<LatestExplorerSnapshot>()`.
    pub explorer_snapshot: Option<Arc<LatestExplorerSnapshot>>,
    /// The texts of `pr_core::settings` (`SettingsPresenterOptions::settings_texts`).
    pub settings_texts: Arc<dyn SettingsTexts>,
    /// `WindowsShellIntegration` (`SettingsPresenterOptions::shell_integration`).
    pub shell_integration: Arc<dyn ShellIntegration>,
    /// `RamBudgetPolicy.GetPhysicalMemoryBytes()` (`SettingsPresenterOptions::physical_memory_bytes`).
    pub physical_memory_bytes: i64,
    /// Runs the Recovery check/retry jobs and the Benchmark scan/profile/save jobs off the UI thread (`ThreadRunner` in production).
    pub runner: Arc<dyn BackgroundRunner>,
    /// `BenchmarkProfileSession.Factory(PixelBufferImageCodec.Instance, () => new WicDirectDecoder(..))`.
    pub benchmark_sessions: BenchmarkSessionFactory,
}

impl std::fmt::Debug for DialogServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogServices").finish_non_exhaustive()
    }
}

/// What the host (the composition root) gives the factory besides the graph services.
pub struct DialogHostParts {
    /// `GetService<IClipboardService>()`: the Copy buttons of Recovery and Diagnostics.
    pub clipboard: Rc<dyn ClipboardService>,
    /// `GetService<IShellFileDialog>()`: Import/Export, the exe picker, Action profiles files.
    pub file_dialog: Rc<dyn ShellFileDialog>,
    /// `GetService<IShellFolderDialog>()`: the Benchmark folder button (through `FolderPicker.UsableInitialFolder`).
    pub folder_dialog: Rc<dyn ShellFolderDialog>,
    /// Logs `"Folder dialog failed"` style errors and the RV-A14 journal read error.
    pub log: Arc<dyn Log>,
    /// The version text of Settings (`SettingsDialogOptions::version`).
    pub version: Option<String>,
    /// The catalog of the CURRENT language, evaluated per dialog (C# `Tr`): `Localizer::current` in production, a fixed catalog in tests.
    pub localizer: Box<dyn Fn() -> Arc<Localizer>>,
    /// The user-locale number formatting (`DiagnosticsPresenter.Build`, `ListDialogContent::for_batch_review`): `WindowsNumberFormatting` in production.
    pub numbers: Rc<dyn NumberFormatting>,
    /// The local time text of the Recovery window (`RecoveryTextSource`): `WindowsRecoveryTimeFormatter` in production.
    pub time: Rc<dyn RecoveryTimeFormatter>,
    /// The digit-group separator of the .NET `N0` format (`RecoveryTextSource`): `locale::group_separator()` in production.
    pub group_separator: String,
    /// C# `DateTime.Now.ToString("T")` for the "done" line of the Recovery window (`RecoveryWindowOptions::current_time_text`): `locale::local_long_time_text` in production.
    pub current_time_text: Arc<dyn Fn() -> String + Send + Sync>,
}

impl std::fmt::Debug for DialogHostParts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogHostParts").finish_non_exhaustive()
    }
}

/// C# `BenchmarkDialogSeams` (record, 21-27): the replaceable parts of the Benchmark window; `None` = the real one.
#[derive(Default, Clone)]
pub struct BenchmarkDialogSeams {
    /// C# `Sessions`.
    pub sessions: Option<BenchmarkSessionFactory>,
    /// C# `Scan`.
    pub scan: Option<BenchmarkFileScan>,
    /// C# `Save`.
    pub save: Option<BenchmarkSummarySave>,
    /// C# `PickFolder`.
    pub pick_folder: Option<PickFolderFn>,
    /// C# `OpenInExplorer`.
    pub open_in_explorer: Option<OpenInExplorerFn>,
}

impl std::fmt::Debug for BenchmarkDialogSeams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BenchmarkDialogSeams").finish_non_exhaustive()
    }
}

/// The other constructor arguments of the C# port that belong to the dialogs (`chrome`, `warnings`, `benchmark`, `recovery`). `Default` = the real ones.
#[derive(Default)]
pub struct DialogFactorySeams {
    /// C# `chrome`: `None` = `default_chrome()` of the native base.
    pub chrome: Option<Rc<dyn NativeDialogChrome>>,
    /// C# `warnings`: `None` = `TaskDialogWarningSink::instance()`.
    pub warnings: Option<Rc<dyn DialogWarningSink>>,
    /// C# `benchmark`.
    pub benchmark: BenchmarkDialogSeams,
    /// C# `RecoveryDialogSeams.Messages`.
    pub recovery_messages: Option<Rc<dyn TaskDialogPort>>,
    /// C# `RecoveryDialogSeams.Explorer`.
    pub recovery_explorer: Option<pr_platform_win::dialogs::recovery::ExplorerSeam>,
    /// C# `RecoveryDialogSeams.AutoCheck` negated: `true` = do not start the live check (tests drive each step).
    pub recovery_no_auto_check: bool,
}

impl std::fmt::Debug for DialogFactorySeams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogFactorySeams").finish_non_exhaustive()
    }
}

/// The production implementation of `SecondaryWindowDialogs` (D-W8B5-413). UI thread only (`Rc`).
pub struct ProductionSecondaryWindowDialogs {
    services: Rc<DialogServices>,
    host: DialogHostParts,
    seams: DialogFactorySeams,
}

impl std::fmt::Debug for ProductionSecondaryWindowDialogs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProductionSecondaryWindowDialogs")
            .finish_non_exhaustive()
    }
}

impl ProductionSecondaryWindowDialogs {
    /// The graph services, the host parts and the seams.
    pub fn new(services: Rc<DialogServices>, host: DialogHostParts, seams: DialogFactorySeams) -> Self {
        todo!()
    }
}

impl SecondaryWindowDialogs for ProductionSecondaryWindowDialogs {
    /// C# `ShowSettings` 97-115 up to the `new SettingsDialog(..)`: `None` without a store; `SettingsDialogArgs { current: store.current(), presenter:
    /// SettingsPresenterOptions { store, localization, update_checker, shell_integration, executable_path, .. }, decoder_factory, options: SettingsDialogOptions {
    /// files, warnings, target, show_action_profiles, version, .. }, chrome }`; `finish(_)` = `dialog.saved()`.
    fn settings(
        &self,
        target: SettingsTarget,
        show_action_profiles: ActionProfilesOpener,
    ) -> Option<PreparedDialog<bool>> {
        todo!()
    }

    /// C# `ShowRecovery` 131-158 up to the `new RecoveryDialog(..)`.
    fn recovery(&self, owner: Hwnd) -> Option<PreparedDialog<()>> {
        todo!()
    }

    /// C# `ShowBenchmark` 195-208 up to the `new BenchmarkDialog(..)`: the dialog, not shown.
    fn benchmark(&self, folder: Option<&Path>) -> Option<pr_platform_win::dialogs::native::dialog::NativeDialog> {
        todo!()
    }

    /// C# `ShowActionProfiles` 237.
    fn action_profiles(&self, actions: &[ReviewAction]) -> Option<PreparedDialog<Option<Vec<ReviewAction>>>> {
        todo!()
    }

    /// C# `ShowDiagnostics` 241-252.
    fn diagnostics(&self) -> Option<PreparedDialog<()>> {
        todo!()
    }

    /// C# `ShowSkippedFiles` 254-258.
    fn skipped_files(&self, entries: &[SkippedEntry]) -> Option<PreparedDialog<()>> {
        todo!()
    }

    /// C# `ShowBatchReview` 260-264.
    fn batch_review(&self, items: &[BatchReviewItem]) -> Option<PreparedDialog<bool>> {
        todo!()
    }

    /// C# `PromptCustomZoom` 266-270.
    fn custom_zoom(&self, current_percent: i32) -> Option<PreparedDialog<Option<i32>>> {
        todo!()
    }
}
