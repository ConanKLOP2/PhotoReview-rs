//! Port of `PhotoReview.App.Shared/Presenters/SettingsPresenter.cs` (C# baseline `d92d6aae`), W8-10e: the result records
//! (lines 12 to 36) and the dialog actions that are not form validation (lines 330 to 636): import / export of the whole settings
//! file, the actions-editor hand-off, the context-menu rows, the language picker, the Explorer integration and the manual update
//! check. Contract: `contracts/W8-10e.md`.
//!
//! `impl SettingsPresenter` blocks; the struct lives in `load.rs`. Everything is synchronous and UI-thread (D-W8-2) except the
//! update check, which is split in three steps so the network call runs on a worker and its result comes back through
//! `UiScheduler::post`: [`SettingsPresenter::begin_update_check`] (UI thread) -> [`UpdateCheckJob::run`] (worker, blocking) ->
//! [`SettingsPresenter::finish_update_check`] (UI thread). No future, no runtime.

use std::fmt;
use std::os::windows::process::CommandExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use pr_core::abstractions::ShellMenuState;
use pr_core::abstractions::shell_menu_command::is_valid_executable_path;
use pr_core::settings::app_settings::SettingsTexts;
use pr_core::settings::context_menu::{ContextMenuItemId, ITEMS};
use pr_core::settings::normalizer::build_repair_text;
use pr_core::settings::shortcuts::ReviewAction;
use pr_core::settings::store::SettingsStore;
use pr_core::sync::{CancelToken, Canceled};
use pr_core::text::eq_ignore_case;
use pr_core::updates::checker::UpdateCheck;
use pr_core::updates::version::{AppVersion, UpdateCheckResult, UpdateCheckStatus, UpdateFailure, validate_update_url};
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;

use super::load::SettingsPresenter;
use crate::localization::language_options::{self, LanguageOption};
use crate::localization::translation_problems;
use crate::misc::app_log::AppLog;
use crate::presenters::action_list_json::{self, ActionListJsonError};
use crate::presenters::settings_field_map::FieldContext;

/// `ctx.texts.format_with(key, {name: value})`, the shape of nearly every message of this file.
fn text_with(ctx: &FieldContext<'_>, key: &str, name: &str, value: impl Into<String>) -> String {
    ctx.texts
        .format_with(key, &[LocArg::new(name, value.into())], ctx.numbers)
}

/// C# `SettingsExplorerRequest(Arguments, ErrorMessage)`: the arguments to start Explorer with, or the reason it cannot be done.
/// `(None, None)` = "nothing to do" (no localization service).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsExplorerRequest {
    pub arguments: Option<String>,
    pub error_message: Option<String>,
}

/// C# `ShellIntegrationStatus(Text, AddEnabled, RemoveEnabled, AddLabel)`. `add_label` `None` = leave the caption as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellIntegrationStatus {
    pub text: String,
    pub add_enabled: bool,
    pub remove_enabled: bool,
    pub add_label: Option<String>,
}

/// C# `UpdateCheckDisplay(StatusText, DownloadUrl)`: what the update-check line shows once a check finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheckDisplay {
    pub status_text: String,
    /// The validated download page, or `None` when no page can be offered.
    pub download_url: Option<String>,
}

impl UpdateCheckDisplay {
    /// C# `ShowOpenPage`: `download_url` is `Some`.
    pub fn show_open_page(&self) -> bool {
        self.download_url.is_some()
    }
}

/// C# `TranslationReloadResult(Message, HasProblems)`: the outcome of the "reload translations" button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationReloadResult {
    pub message: String,
    pub has_problems: bool,
}

/// C# `SettingsImportResult(Error, RepairsNotice)`: a refusal, or the (optional) "these values were reset" notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsImportResult {
    pub error: Option<String>,
    pub repairs_notice: Option<String>,
}

/// C# `ContextMenuRow(Id, Label, AutomationName, IsChild, Locked, LockedTooltip)`: one tick-box row of the right-click menu page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenuRow {
    pub id: ContextMenuItemId,
    pub label: String,
    pub automation_name: String,
    pub is_child: bool,
    pub locked: bool,
    pub locked_tooltip: Option<String>,
}

/// The view's "reload the controls from the working copy" callback (C# `Action refreshView`). It gets the presenter (the view calls
/// `load_fields()` on it) and may fail with the message of the C# exception.
pub type RefreshView<'a> = &'a mut dyn FnMut(&SettingsPresenter) -> Result<(), String>;

/// What a finished check produced on the worker (C# `UpdateCheckResult`, a thrown exception, or a cancellation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheckOutcome {
    /// The checker returned a result.
    Result(UpdateCheckResult),
    /// The checker returned `Err(Canceled)` (C# `OperationCanceledException`).
    Canceled,
    /// The checker panicked (C# any other exception except `OutOfMemoryException`): shown as `BadResponse`.
    Failed,
}

/// A started check, ready to run on a worker. `Send`: it owns an `Arc` checker, the version text and the cancel token.
pub struct UpdateCheckJob {
    checker: Option<Arc<dyn UpdateCheck>>,
    version: Option<String>,
    cancel: CancelToken,
}

impl fmt::Debug for UpdateCheckJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateCheckJob")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl UpdateCheckJob {
    /// BLOCKING, worker thread: `checker.check(version, &cancel)`. No checker configured => `Result(UpdateCheckResult::failed(BadResponse))`.
    /// `Err(Canceled)` => `Canceled`. A panic of the checker is caught (`catch_unwind`) => `Failed`. Never panics.
    pub fn run(self) -> UpdateCheckCompleted {
        let outcome = match &self.checker {
            None => UpdateCheckOutcome::Result(UpdateCheckResult::failed(UpdateFailure::BadResponse)),
            Some(checker) => {
                let version = self.version.as_deref();
                let cancel = &self.cancel;
                match catch_unwind(AssertUnwindSafe(|| checker.check(version, cancel))) {
                    Ok(Ok(result)) => UpdateCheckOutcome::Result(result),
                    Ok(Err(Canceled)) => UpdateCheckOutcome::Canceled,
                    Err(_) => UpdateCheckOutcome::Failed,
                }
            }
        };
        UpdateCheckCompleted {
            outcome,
            cancel: self.cancel,
            version: self.version,
        }
    }
}

/// The outcome plus the token of the check that produced it, handed back to the UI thread.
#[derive(Debug, Clone)]
pub struct UpdateCheckCompleted {
    pub outcome: UpdateCheckOutcome,
    pub cancel: CancelToken,
    /// The version text the check was started with (the dialog footer's version).
    pub version: Option<String>,
}

impl SettingsPresenter {
    // ---- Import / export of the whole settings file (Q-R50) ----

    /// C# `ExportSettingsTo(path)`: writes `settings.to_json()` (the config.json contract) with `std::fs::write` (UTF-8, no BOM, like
    /// `File.WriteAllText`). `None` on success; on an `io::Error` (C# `IOException` / `UnauthorizedAccessException`) the localized
    /// `dialog.exportSettings.failed` with `{message}` = the error text. A non-finite number makes `to_json` fail: that is NOT in the C#
    /// catch list (`ArgumentException` escapes), so it is returned as the same failure text instead of a panic (Rust has no unwinding contract here).
    pub fn export_settings_to(&self, path: &Path, ctx: &FieldContext<'_>) -> Option<String> {
        let written = self
            .settings
            .to_json()
            .map_err(|error| error.message)
            .and_then(|json| std::fs::write(path, json).map_err(|error| error.to_string()));
        match written {
            Ok(()) => None,
            Err(message) => Some(text_with(ctx, "dialog.exportSettings.failed", "message", message)),
        }
    }

    /// C# `ImportSettingsFrom(path, refreshView)`: reads the file (`std::fs::read_to_string`; an `io::Error` gives
    /// `SettingsImportResult { error: Some(dialog.importSettings.failed {message}), repairs_notice: None }` and `refresh_view` is NOT
    /// called), then [`Self::try_apply_imported_json`]: `Ok(notice)` => `{ error: None, repairs_notice: notice }`, `Err(message)` =>
    /// `{ error: Some(message), repairs_notice: None }`.
    pub fn import_settings_from(
        &mut self,
        path: &Path,
        refresh_view: RefreshView<'_>,
        ctx: &FieldContext<'_>,
    ) -> SettingsImportResult {
        let json = match std::fs::read_to_string(path) {
            Ok(json) => json,
            Err(error) => {
                return SettingsImportResult {
                    error: Some(text_with(
                        ctx,
                        "dialog.importSettings.failed",
                        "message",
                        error.to_string(),
                    )),
                    repairs_notice: None,
                };
            }
        };
        match self.try_apply_imported_json(&json, refresh_view, ctx) {
            Ok(notice) => SettingsImportResult {
                error: None,
                repairs_notice: notice,
            },
            Err(message) => SettingsImportResult {
                error: Some(message),
                repairs_notice: None,
            },
        }
    }

    /// C# `TryApplyImportedJson(json, refreshView, out message)`: replaces the working copy with imported text through the start-up
    /// pipeline. `Ok(notice)`: `notice` is `Some(build_repair_text(repairs, true, ..))` when anything was reset, else `None`. `Err(text)`:
    /// the file is unusable and the working copy is what it was. Steps:
    ///
    /// 1. `previous = self.settings.clone()`;
    /// 2. `SettingsStore::parse_text(json, &*self.settings_texts)`; an `Err(SettingsJsonError)` or `is_json_null` (error text
    ///    `"The file does not contain a settings object."`) goes to the failure arm;
    /// 3. `self.settings = parsed.settings.clone()`; `refresh_view(self)`; the notice is built from `parsed.repairs()` only when
    ///    `repairs` is non-empty;
    /// 4. failure arm (any error of steps 2 and 3, including `refresh_view` returning `Err`): log, `self.settings = previous`, call
    ///    `refresh_view` AGAIN (an `Err` there is only logged), and return `Err(dialog.importSettings.failed {message})`.
    pub fn try_apply_imported_json(
        &mut self,
        json: &str,
        refresh_view: RefreshView<'_>,
        ctx: &FieldContext<'_>,
    ) -> Result<Option<String>, String> {
        let previous = self.settings.clone();
        match self.apply_imported_json(json, refresh_view) {
            Ok(notice) => Ok(notice),
            Err(message) => {
                AppLog::global().error(
                    &format!("Settings import: the file could not be applied: {message}"),
                    None,
                );
                self.settings = previous;
                if let Err(reload_message) = refresh_view(self) {
                    AppLog::global().error(
                        &format!("Settings import: could not restore the previous fields: {reload_message}"),
                        None,
                    );
                }
                Err(text_with(ctx, "dialog.importSettings.failed", "message", message))
            }
        }
    }

    /// Steps 2 and 3 of [`Self::try_apply_imported_json`]; the caller restores the working copy on `Err`.
    fn apply_imported_json(&mut self, json: &str, refresh_view: RefreshView<'_>) -> Result<Option<String>, String> {
        let parsed = SettingsStore::parse_text(json, &*self.settings_texts).map_err(|error| error.message)?;
        if parsed.is_json_null {
            return Err("The file does not contain a settings object.".to_owned());
        }
        let repairs = parsed.repairs();
        self.settings = parsed.settings.clone();
        refresh_view(self)?;
        Ok(if repairs.is_empty() {
            None
        } else {
            Some(build_repair_text(&repairs, true, &*self.settings_texts))
        })
    }

    /// C# `SettingsFileDialogDirectory`: the directory of the running executable (`AppContext.BaseDirectory`).
    pub fn settings_file_dialog_directory() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_default()
    }

    // ---- Actions editor hand-off ----

    /// C# static `ActionsForEditor(actionsJson)`: `action_list_json::deserialize_or_null(text, default_name)` with `Ok(None)` (the
    /// literal `null`) replaced by `ReviewAction::defaults(..)` (same arguments as `pr_core::settings::app_settings::AppSettings::new`
    /// uses). Unusable JSON is the `Err`.
    pub fn actions_for_editor(
        actions_json: &str,
        settings_texts: &dyn SettingsTexts,
    ) -> Result<Vec<ReviewAction>, ActionListJsonError> {
        let default_name = settings_texts.action_default_name(2);
        Ok(
            match action_list_json::deserialize_or_null(actions_json, &default_name)? {
                Some(actions) => actions,
                None => ReviewAction::defaults(
                    &|n| settings_texts.action_default_name(n),
                    &settings_texts.action_default_backup(),
                ),
            },
        )
    }

    /// C# static `ActionsToText(actions)`: `action_list_json::serialize(actions)` (relaxed escaping: Vietnamese stays readable).
    pub fn actions_to_text(actions: &[ReviewAction]) -> String {
        action_list_json::serialize(actions)
    }

    // ---- Context menu page ----

    /// C# static `ContextMenuRows()`: one row per entry of `pr_core::settings::context_menu::ITEMS`, in table order: `label` =
    /// [`Self::context_menu_item_label`], `automation_name` = `settings.contextMenu.item.automationName` with `{name}` = the label,
    /// `is_child` = `parent.is_some()`, `locked` = `info.locked`, `locked_tooltip` = `Some(settings.contextMenu.settingsLocked.tooltip)`
    /// exactly when locked.
    pub fn context_menu_rows(ctx: &FieldContext<'_>) -> Vec<ContextMenuRow> {
        ITEMS
            .iter()
            .map(|item| {
                let label = Self::context_menu_item_label(item.id, ctx);
                ContextMenuRow {
                    id: item.id,
                    automation_name: text_with(ctx, "settings.contextMenu.item.automationName", "name", label.clone()),
                    label,
                    is_child: item.parent.is_some(),
                    locked: item.locked,
                    locked_tooltip: item
                        .locked
                        .then(|| ctx.texts.get("settings.contextMenu.settingsLocked.tooltip")),
                }
            })
            .collect()
    }

    /// C# static `ContextMenuItemLabel(id)`: `settings.contextMenu.item.<x>` for all 18 ids (the C# `_ => id.ToString()` arm has
    /// no input in Rust).
    pub fn context_menu_item_label(id: ContextMenuItemId, ctx: &FieldContext<'_>) -> String {
        let name = match id {
            ContextMenuItemId::Undo => "undo",
            ContextMenuItemId::MoveToRecycleBin => "moveToRecycleBin",
            ContextMenuItemId::Fit => "fit",
            ContextMenuItemId::ZoomToLevel => "zoomToLevel",
            ContextMenuItemId::ZoomSubmenu => "zoomSubmenu",
            ContextMenuItemId::OpenFolder => "openFolder",
            ContextMenuItemId::NextFolder => "nextFolder",
            ContextMenuItemId::PreviousFolder => "previousFolder",
            ContextMenuItemId::ExternalEditor => "externalEditor",
            ContextMenuItemId::CopyFileName => "copyFileName",
            ContextMenuItemId::CopyFullPath => "copyFullPath",
            ContextMenuItemId::Settings => "settings",
            ContextMenuItemId::ZoomFitWidth => "zoomFitWidth",
            ContextMenuItemId::ZoomFitWidth2 => "zoomFitWidth2",
            ContextMenuItemId::ZoomFitHeight => "zoomFitHeight",
            ContextMenuItemId::ZoomPresets => "zoomPresets",
            ContextMenuItemId::ZoomLevelOptions => "zoomLevelOptions",
            ContextMenuItemId::Refresh => "refresh",
        };
        ctx.texts.get(&format!("settings.contextMenu.item.{name}"))
    }

    // ---- Language picker (I18N L08) ----

    /// C# `LoadLanguages(currentCode)`: `None` without a localization service. Else `selected = current_code or settings.ui_language`,
    /// `options = language_options::build(&service.discover_languages(), ctx.texts)`, index = `language_options::index_of(&options,
    /// Some(selected))` (`None` is the C# `-1`).
    pub fn load_languages(
        &self,
        current_code: Option<&str>,
        ctx: &FieldContext<'_>,
    ) -> Option<(Vec<LanguageOption>, Option<usize>)> {
        let localization = self.localization.as_ref()?;
        let languages = localization
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .discover_languages();
        let selected = current_code.unwrap_or(self.settings.ui_language.as_str());
        let options = language_options::build(&languages, ctx.texts);
        let index = language_options::index_of(&options, Some(selected));
        Some((options, index))
    }

    /// C# `PrepareLanguagesFolder()`: `(None, None)` without a service; else `std::fs::create_dir_all(user_languages_dir)`; an
    /// `io::Error` gives `(None, Some(error text))`; else `arguments = "\"{dir}\""` (the dir in double quotes).
    pub fn prepare_languages_folder(&self) -> SettingsExplorerRequest {
        let Some(localization) = self.localization.as_ref() else {
            return SettingsExplorerRequest {
                arguments: None,
                error_message: None,
            };
        };
        let dir = localization
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .user_languages_dir()
            .to_path_buf();
        match std::fs::create_dir_all(&dir) {
            Ok(()) => SettingsExplorerRequest {
                arguments: Some(format!("\"{}\"", dir.display())),
                error_message: None,
            },
            Err(error) => SettingsExplorerRequest {
                arguments: None,
                error_message: Some(error.to_string()),
            },
        }
    }

    /// C# `ReloadTranslations()`: `None` without a service. Else `warnings = service.reload()` (publishes a new localizer), log
    /// `"Translations reloaded"`, and the result is built with `localization::translation_problems::{message, has_problems}`. The texts
    /// for `message` are `Localizer::current()` read AFTER the reload (the language files just changed; this is the one place of the
    /// unit that reads the ambient localizer, because the `ctx` the caller holds is the pre-reload one).
    pub fn reload_translations(&self) -> Option<TranslationReloadResult> {
        let localization = self.localization.as_ref()?;
        let warnings = localization.lock().unwrap_or_else(PoisonError::into_inner).reload();
        AppLog::global().info("Translations reloaded");
        let texts = Localizer::current();
        Some(TranslationReloadResult {
            message: translation_problems::message(&warnings, &texts),
            has_problems: translation_problems::has_problems(&warnings),
        })
    }

    /// C# `ExportTranslation(selected)`: `(None, None)` without a service. Else `language = language_options::to_setting(selected,
    /// Some(&settings.ui_language))`, `path = service.export_todo(Some(&language))`, and on success `(Some(SettingsExplorerRequest {
    /// arguments: "/select,\"{path}\"", .. }), None)`; an export error gives `(None, Some(error text))`.
    ///
    /// DIFFERENT UNTIL W8-04a lands `LocalizationService::export_todo` (contract O1: a B1 file this unit does not own, and the shipped
    /// languages directory is private to the service): with a service this returns `(None, Some(<fixed English text>))` instead of
    /// writing a file.
    pub fn export_translation(
        &self,
        selected: Option<&LanguageOption>,
    ) -> (Option<SettingsExplorerRequest>, Option<String>) {
        if self.localization.is_none() {
            return (None, None);
        }
        let _language = language_options::to_setting(selected, Some(self.settings.ui_language.as_str()));
        (
            None,
            Some("Translation export is not available in this build.".to_owned()),
        )
    }

    /// C# static `PrepareLogLocation()`: `directory = parent of AppLog::global().file_path()`; `create_dir_all` failure gives an
    /// `error_message`; else `arguments = "/select,\"{file path}\""`.
    pub fn prepare_log_location() -> SettingsExplorerRequest {
        let file = AppLog::global().file_path();
        if let Some(directory) = file.parent()
            && let Err(error) = std::fs::create_dir_all(directory)
        {
            return SettingsExplorerRequest {
                arguments: None,
                error_message: Some(error.to_string()),
            };
        }
        SettingsExplorerRequest {
            arguments: Some(format!("/select,\"{}\"", file.display())),
            error_message: None,
        }
    }

    /// C# static `TryStartExplorer(arguments)`: starts `explorer.exe` with the arguments passed RAW (Explorer parses its own quotes).
    /// `None` on success, else the error text. No test starts a process (C# has none either).
    pub fn try_start_explorer(arguments: &str) -> Option<String> {
        match std::process::Command::new("explorer.exe").raw_arg(arguments).spawn() {
            // The child is Explorer itself; there is nothing to wait for.
            Ok(child) => {
                drop(child);
                None
            }
            Err(error) => Some(error.to_string()),
        }
    }

    // ---- Explorer integration ----

    /// C# `RefreshShellStatus()`: the registration state and which buttons make sense.
    /// - unavailable (`settings.shell.status.unavailable`, both buttons off, no label change) when `executable_path` is not valid
    ///   (`shell_menu_command::is_valid_executable_path`) or its file name without extension equals `dotnet` (`OrdinalIgnoreCase`);
    /// - else `state = shell_integration.get_state(exe)`: text `settings.shell.status.{registered | elsewhere | notRegistered}`,
    ///   `add_enabled = state != Registered`, `remove_enabled = state != NotRegistered`, `add_label` = `Some(settings.shell.update)` for
    ///   `RegisteredElsewhere`, else `Some(settings.shell.add)`.
    ///
    /// The C# catch arm for a throwing `GetState` has no Rust twin (`ShellIntegration::get_state` is infallible), see the contract.
    pub fn refresh_shell_status(&self, ctx: &FieldContext<'_>) -> ShellIntegrationStatus {
        let exe = self.executable_path.as_deref();
        let is_dotnet = exe
            .and_then(|path| Path::new(path).file_stem())
            .is_some_and(|stem| eq_ignore_case(&stem.to_string_lossy(), "dotnet"));
        let exe = match exe {
            Some(path) if is_valid_executable_path(Some(path)) && !is_dotnet => path,
            _ => {
                return ShellIntegrationStatus {
                    text: ctx.texts.get("settings.shell.status.unavailable"),
                    add_enabled: false,
                    remove_enabled: false,
                    add_label: None,
                };
            }
        };
        let state = self.shell_integration.get_state(exe);
        let text = match state {
            ShellMenuState::Registered => ctx.texts.get("settings.shell.status.registered"),
            ShellMenuState::RegisteredElsewhere => ctx.texts.get("settings.shell.status.elsewhere"),
            ShellMenuState::NotRegistered => ctx.texts.get("settings.shell.status.notRegistered"),
        };
        let label = if state == ShellMenuState::RegisteredElsewhere {
            ctx.texts.get("settings.shell.update")
        } else {
            ctx.texts.get("settings.shell.add")
        };
        ShellIntegrationStatus {
            text,
            add_enabled: state != ShellMenuState::Registered,
            remove_enabled: state != ShellMenuState::NotRegistered,
            add_label: Some(label),
        }
    }

    /// C# `ChangeShellRegistration(register)`: `register(exe)` or `unregister()`; the status afterwards. An `io::Error` logs
    /// `"Explorer integration change failed"` and returns `refresh_shell_status()` with `text` replaced by `settings.shell.status.error`
    /// (`{message}` = the error text). `register` with no executable path is a programming error of the view (C# `ExecutablePath!`):
    /// treated as the same failure text instead of panicking.
    pub fn change_shell_registration(&self, register: bool, ctx: &FieldContext<'_>) -> ShellIntegrationStatus {
        let changed = if register {
            match self.executable_path.as_deref() {
                Some(exe) => self.shell_integration.register(exe),
                None => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "The executable path is not available.",
                )),
            }
        } else {
            self.shell_integration.unregister()
        };
        match changed {
            Ok(()) => self.refresh_shell_status(ctx),
            Err(error) => {
                AppLog::global().error("Explorer integration change failed", Some(&error));
                let mut status = self.refresh_shell_status(ctx);
                status.text = text_with(ctx, "settings.shell.status.error", "message", error.to_string());
                status
            }
        }
    }

    // ---- Manual update check: the only network use of the app, only on click ----

    /// First step of C# `CheckForUpdateAsync(version)`, UI thread: `update_url = None`; a new `CancelToken` is stored in
    /// `update_cancel` (replacing a previous one without cancelling it, like C# `_updateCts = cts`); returns the job for a worker.
    pub fn begin_update_check(&mut self, version: Option<&str>) -> UpdateCheckJob {
        self.update_url = None;
        let cancel = CancelToken::new();
        self.update_cancel = Some(cancel.clone());
        UpdateCheckJob {
            checker: self.update_checker.clone(),
            version: version.map(str::to_owned),
            cancel,
        }
    }

    /// Last step, UI thread. `update_cancel = None` (C# `finally`). Returns `None` when the check was cancelled: the outcome is
    /// `Canceled`, or `completed.cancel.is_canceled()` is true (the dialog closed while the answer was in flight): the view must not
    /// touch its controls then (INV-9 style stale-completion guard). Else:
    /// - `Result` with status `UpToDate`: `update.status.upToDate` with `{version}` = latest version (or `""`), no URL;
    /// - `UpdateAvailable`: `update_url = validate_update_url(download_url)`; `current` = `AppVersion::try_parse(version)` rendered by its
    ///   `Display`, else the raw `version` text (or `""`); text `update.status.available` with `{version}` and `{current}`; the display
    ///   carries `update_url` (so a foreign URL is not offered);
    /// - any other status, and `Failed`: `update.status.failed.<offline | timeout | rateLimited | invalidVersion>`, any other failure
    ///   (`BadResponse`, `None`) `update.status.failed.badResponse`; no URL.
    pub fn finish_update_check(
        &mut self,
        completed: UpdateCheckCompleted,
        ctx: &FieldContext<'_>,
    ) -> Option<UpdateCheckDisplay> {
        self.update_cancel = None;
        // The token of the check that produced this answer, not `update_cancel`: a newer check may have replaced it.
        if completed.cancel.is_canceled() {
            return None;
        }
        let failed_text = |failure: UpdateFailure| {
            let key = match failure {
                UpdateFailure::Offline => "update.status.failed.offline",
                UpdateFailure::Timeout => "update.status.failed.timeout",
                UpdateFailure::RateLimited => "update.status.failed.rateLimited",
                UpdateFailure::InvalidVersion => "update.status.failed.invalidVersion",
                UpdateFailure::BadResponse | UpdateFailure::None => "update.status.failed.badResponse",
            };
            UpdateCheckDisplay {
                status_text: ctx.texts.get(key),
                download_url: None,
            }
        };
        let result = match completed.outcome {
            UpdateCheckOutcome::Canceled => return None,
            UpdateCheckOutcome::Failed => return Some(failed_text(UpdateFailure::BadResponse)),
            UpdateCheckOutcome::Result(result) => result,
        };
        let latest = result.latest_version().unwrap_or("");
        match result.status() {
            UpdateCheckStatus::UpToDate => Some(UpdateCheckDisplay {
                status_text: text_with(ctx, "update.status.upToDate", "version", latest),
                download_url: None,
            }),
            UpdateCheckStatus::UpdateAvailable => {
                self.update_url = validate_update_url(result.download_url());
                let version = completed.version.as_deref();
                let current = AppVersion::try_parse(version)
                    .map(|parsed| parsed.to_string())
                    .unwrap_or_else(|| version.unwrap_or("").to_owned());
                Some(UpdateCheckDisplay {
                    status_text: ctx.texts.format_with(
                        "update.status.available",
                        &[LocArg::new("version", latest), LocArg::new("current", current)],
                        ctx.numbers,
                    ),
                    download_url: self.update_url.clone(),
                })
            }
            UpdateCheckStatus::Failed => Some(failed_text(result.failure())),
        }
    }

    /// Convenience for tests and CLI tools: `begin_update_check` + `job.run()` + `finish_update_check` on the calling thread. NEVER
    /// call it on the UI thread with a real network checker.
    pub fn check_for_update_blocking(
        &mut self,
        version: Option<&str>,
        ctx: &FieldContext<'_>,
    ) -> Option<UpdateCheckDisplay> {
        let completed = self.begin_update_check(version).run();
        self.finish_update_check(completed, ctx)
    }

    /// C# `OpenUpdatePage(openUrl)`: `url = validate_update_url(self.update_url)` (RE-validated on every click); `None` => returns `None`
    /// without calling `open_url`. `open_url` returning `Err` gives `Some(update.openPage.failed)`; else `None`.
    pub fn open_update_page(
        &self,
        open_url: &mut dyn FnMut(&str) -> Result<(), String>,
        ctx: &FieldContext<'_>,
    ) -> Option<String> {
        let url = validate_update_url(self.update_url.as_deref())?;
        match open_url(&url) {
            Ok(()) => None,
            Err(_) => Some(ctx.texts.get("update.openPage.failed")),
        }
    }

    /// C# `internal UpdateUrl { get; set; }`: test seam to store a foreign URL and prove `open_update_page` re-validates.
    pub fn update_url(&self) -> Option<&str> {
        self.update_url.as_deref()
    }

    /// C# `UpdateUrl` setter (test seam).
    pub fn set_update_url(&mut self, url: Option<String>) {
        self.update_url = url;
    }

    /// C# `CancelUpdateCheck()`: cancels the running check's token (dialog closed). Harmless when none runs.
    pub fn cancel_update_check(&self) {
        if let Some(cancel) = &self.update_cancel {
            cancel.cancel();
        }
    }
}
