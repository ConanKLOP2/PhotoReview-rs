//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 262-328 and 388-391 (baseline `7eaee2a2`): the settings and language load, what is done with the result,
//! the "settings repaired" notice (C-28) and the `ShellStartupContext` of C-16 (lines 300-303 and 520-523). Unit W8-17d. Contract: `contracts/W8-17d.md`.
//!
//! The C# runs `store.Load()` and `localization.Load(..)` in ONE pool task, then continues on the UI thread: `localization.Apply`, mark `settingsLoaded`, the log switch.
//! Rust: [`load_settings_and_language`] is the worker body (`Send`-safe pieces only), [`apply_loaded_settings`] is the UI-thread part; the hop between them is
//! `startup::ui_hop`. Nothing here touches a window.

use std::fmt;
use std::io;
use std::sync::Arc;
use std::sync::PoisonError;
use std::time::Instant;

use pr_core::diagnostics::diag_options::DiagOptions;
use pr_core::settings::app_settings::{AppSettings, SettingsTexts};
use pr_core::settings::store::SettingsStore;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;

use crate::presenters::settings::load::SharedLocalization;
use crate::startup::context::ShellStartupContext;
use crate::startup::launch::ShellLaunch;
use crate::windowing::contracts::PrefetchHandle;

/// What the worker produced: C# `(AppSettings Settings, Localizer Localizer)`.
#[derive(Clone)]
pub struct LoadedSettings {
    /// `store.Load()`.
    pub settings: Arc<AppSettings>,
    /// `localization.Load(settings.UiLanguage)`.
    pub localizer: Arc<Localizer>,
}

impl fmt::Debug for LoadedSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedSettings").finish_non_exhaustive()
    }
}

/// Why the load step failed (C# `await settingsLoad` throwing): the start-up reports it through the "start-up failed" box.
#[derive(Debug)]
pub enum StartupLoadError {
    /// `SettingsStore::load` returned `Err` (a `Changed` handler failed; `store.current()` already holds the loaded settings, but the C# `Load()` threw).
    Settings(io::Error),
    /// The worker panicked (C# a faulted task) or could not be started; the text is the panic message or `HopLost`.
    Worker(String),
}

impl fmt::Display for StartupLoadError {
    /// `Settings(e)`: `e`; `Worker(m)`: `m`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Settings(e) => e.fmt(f),
            Self::Worker(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for StartupLoadError {}

/// The worker body (C# lines 285-299). In this order: `store.load(None)` (`Err` -> `Settings`); `mark("settingsFileLoaded")`; `after_file_loaded(&settings)` (the shell starts the
/// early-decode preparation there when the launch has an initial FILE, C# lines 291-294: it must return at once); `localization.lock().load(Some(&settings.ui_language))`
/// (a poisoned mutex is recovered with `PoisonError::into_inner`; the guard is held for the load only); `mark("languageLoaded")`; `Ok(LoadedSettings)`.
pub fn load_settings_and_language(
    store: &SettingsStore,
    localization: &SharedLocalization,
    mark: &dyn Fn(&str),
    after_file_loaded: &dyn Fn(&Arc<AppSettings>),
) -> Result<LoadedSettings, StartupLoadError> {
    let settings = store.load(None).map_err(StartupLoadError::Settings)?;
    mark("settingsFileLoaded");
    after_file_loaded(&settings);
    let localizer = Arc::new(
        localization
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .load(Some(&settings.ui_language)),
    );
    mark("languageLoaded");
    Ok(LoadedSettings { settings, localizer })
}

/// The UI-thread part (C# lines 304-306): `localization.lock().apply(Some(&settings.ui_language), localizer.clone())` (publishes the localizer and the UI culture), then
/// `mark("settingsLoaded")`.
pub fn apply_loaded_settings(localization: &SharedLocalization, loaded: &LoadedSettings, mark: &dyn Fn(&str)) {
    localization
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .apply(Some(&loaded.settings.ui_language), loaded.localizer.clone());
    mark("settingsLoaded");
}

/// C# `AppLog.Enabled = settings.LoggingEnabled || DiagOptions.ForceLog` (lines 281 and 307): the value to give `AppLog::set_enabled`.
pub fn logging_enabled(settings: &AppSettings, diag: &DiagOptions) -> bool {
    settings.logging_enabled || diag.force_log
}

/// The two start-up log lines (C# lines 308-316): `"Startup (shell) args=" + args joined by " | "` (written only when logging is enabled) and, when
/// `diag.any_enabled()`, `"DIAG MODE: " + diag.describe()` (written even when logging is off, through `AppLog::write_forced`; the second element says so).
/// Pure: the caller does the writing. Returns `(startup_line, diag_line)`; `diag_line` is `None` when no DIAG variable is in effect.
pub fn startup_log_lines(args: &[String], diag: &DiagOptions) -> (String, Option<String>) {
    (
        format!("Startup (shell) args={}", args.join(" | ")),
        diag.any_enabled().then(|| format!("DIAG MODE: {}", diag.describe())),
    )
}

/// C# lines 388-391: `Some(build_repair_text(&store.last_load_repairs(), false, texts))` when the load repaired anything, else `None` (a valid file shows no box).
pub fn repair_notice(store: &SettingsStore, texts: &dyn SettingsTexts) -> Option<String> {
    let repairs = store.last_load_repairs();
    (!repairs.is_empty()).then(|| pr_core::settings::normalizer::build_repair_text(&repairs, false, texts))
}

/// `SettingsTexts` over the CURRENT localizer (C# `Tr.*`): the nine members read the keys `action.default.name` (`{n}`), `action.default.backup`,
/// `core.settings.shortcutInvalid` (`{name}`), `core.settings.actionInvalid`, `core.settings.actionBindingName` (`{name}`), `core.settings.shortcutDuplicate`
/// (`{key}`, `{names}`), `settings.load.repaired` (`{items}`), `dialog.importSettings.repaired` (`{items}`), `settings.load.shortcutDisabled` (`{name}`) of the held
/// localizer. The store of the shell is built with these texts (09b) and the repair notice reads them AFTER the language was applied.
#[derive(Clone)]
pub struct LocalizerSettingsTexts {
    localizer: Arc<Localizer>,
}

impl fmt::Debug for LocalizerSettingsTexts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalizerSettingsTexts").finish_non_exhaustive()
    }
}

impl LocalizerSettingsTexts {
    /// Texts of `localizer`.
    pub fn new(localizer: Arc<Localizer>) -> Self {
        Self { localizer }
    }
}

impl SettingsTexts for LocalizerSettingsTexts {
    fn action_default_name(&self, n: u32) -> String {
        self.localizer.format("action.default.name", &[LocArg::new("n", n)])
    }
    fn action_default_backup(&self) -> String {
        self.localizer.get("action.default.backup")
    }
    fn core_settings_shortcut_invalid(&self, name: &str) -> String {
        self.localizer
            .format("core.settings.shortcutInvalid", &[LocArg::new("name", name)])
    }
    fn core_settings_action_invalid(&self) -> String {
        self.localizer.get("core.settings.actionInvalid")
    }
    fn core_settings_action_binding_name(&self, name: &str) -> String {
        self.localizer
            .format("core.settings.actionBindingName", &[LocArg::new("name", name)])
    }
    fn core_settings_shortcut_duplicate(&self, key: &str, names: &str) -> String {
        self.localizer.format(
            "core.settings.shortcutDuplicate",
            &[LocArg::new("key", key), LocArg::new("names", names)],
        )
    }
    fn settings_load_repaired(&self, items: &str) -> String {
        self.localizer
            .format("settings.load.repaired", &[LocArg::new("items", items)])
    }
    fn dialog_import_settings_repaired(&self, items: &str) -> String {
        self.localizer
            .format("dialog.importSettings.repaired", &[LocArg::new("items", items)])
    }
    fn settings_load_shortcut_disabled(&self, name: &str) -> String {
        self.localizer
            .format("settings.load.shortcutDisabled", &[LocArg::new("name", name)])
    }
}

/// C# `Context = new ShellStartupContext(_launch.Arguments, _launch.InitialPath, _launch.LaunchFolder, ReadSettingsAsync(settingsLoad), placementLoad, _startTimestamp)` (C-16).
/// Built by 17d AFTER the load result is at hand (D-W8B6-22): `settings` is `StartupTask::spawn("startup-context-settings", move || (*settings).clone())` (a task that is already
/// ready), `placement` is the real prefetch handle when `Some`, else an EMPTY handle (`PrefetchHandle { handle: None }`: the early-decode worker consumed the single-use handle).
/// `process_start` is the `Instant` taken at the entry of `run`. No code reads the context yet.
pub fn build_startup_context(
    launch: &ShellLaunch,
    settings: &Arc<AppSettings>,
    placement: Option<PrefetchHandle>,
    process_start: Instant,
) -> ShellStartupContext {
    let settings = settings.clone();
    ShellStartupContext {
        arguments: launch.arguments.clone(),
        initial_path: launch.initial_path().map(str::to_owned),
        launch_folder: launch.launch_folder.clone(),
        settings: crate::startup::context::StartupTask::spawn("startup-context-settings", move || (*settings).clone()),
        placement: placement.unwrap_or(PrefetchHandle { handle: None }),
        process_start,
    }
}
