//! Loading, repairing, backing up and saving `config.json`.
//!
//! Port of `PhotoReview.Core/Settings/SettingsStore.cs` (baseline d92d6aae) plus ONE Rust-only function, the
//! first-run import of the C# app's `config.json` (docs/adr/R0002). Contract: `contracts/W2-03/contract.md`.
//!
//! The Rust app reads and writes ITS OWN `config.json` under `%LOCALAPPDATA%\PhotoReviewRs` (`AppPaths`); it never
//! writes anything into the C# app's folder, not even a backup of a broken C# file.
//!
//! Mapping from C#: exceptions that C# catches as `IOException`/`UnauthorizedAccessException` are every `io::Error`;
//! `JsonException` is [`SettingsJsonError`]; the `Changed` event is a list of handlers that return `io::Result` (an
//! `Err` is the C# "handler threw"; it is returned from `load`/`save` and is never mistaken for a failed write).
#![allow(unused_variables, dead_code)] // D11: stub with todo!() bodies; removed by the porter in the commit that removes the last todo!().

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::abstractions::{Clock, FileSystem, KeyNameValidator, Log};
use crate::app_paths::AppPaths;
use crate::settings::app_settings::{AppSettings, SettingsJsonError, SettingsTexts, json};
use crate::settings::normalizer::{SettingsParseResult, SettingsValidator};
use crate::time::DateTimeUtc;

/// `Action<string, Exception>? onStartupError`: called instead of `ILog.Error` for the start-up problems.
pub type StartupErrorCallback = Box<dyn Fn(&str, &(dyn Error + 'static)) + Send + Sync>;

/// A `Changed` subscriber. Returning `Err` stands for a handler that threw: the remaining handlers are skipped
/// and the error is returned to the caller of `load`/`save`.
pub type ChangedHandler = Box<dyn Fn(&Arc<AppSettings>) -> io::Result<()> + Send + Sync>;

/// What `Thread.Sleep` is in C#: injected so tests do not wait. The default sleeps for real.
pub type RetrySleeper = Arc<dyn Fn(Duration) + Send + Sync>;

/// `SimpleKeyNameValidator`: the permissive fallback when no platform validator is wired (any non-blank name).
#[derive(Debug, Default, Clone, Copy)]
pub struct SimpleKeyNameValidator;

impl KeyNameValidator for SimpleKeyNameValidator {
    fn is_valid_key_name(&self, key_name: &str) -> bool {
        !key_name.trim().is_empty()
    }
}

/// Why `save` failed: the C# `Save` throws the IO exception of the write, or `ArgumentException` when a double
/// cannot be written (NaN / infinity), which Load's IO catch blocks do NOT swallow.
#[derive(Debug)]
pub enum SettingsSaveError {
    /// Creating the folder, the backup excepted, or the atomic write failed.
    Io(io::Error),
    /// `AppSettings::to_json` failed (non-finite number).
    Json(SettingsJsonError),
}

impl fmt::Display for SettingsSaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Json(e) => write!(f, "{e}"),
        }
    }
}

impl Error for SettingsSaveError {}

impl From<io::Error> for SettingsSaveError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<SettingsJsonError> for SettingsSaveError {
    fn from(e: SettingsJsonError) -> Self {
        Self::Json(e)
    }
}

#[derive(Debug)]
struct State {
    current: Arc<AppSettings>,
    /// The last load saw a config.json stamped by a NEWER build: the next write backs it up first.
    newer_file_on_disk: bool,
    /// The corrupt/unreadable config.json could not be backed up (or was only locked): never overwrite it this session.
    keep_corrupt_file: bool,
    last_load_repairs: Vec<String>,
}

/// `SettingsStore`: owns the current settings and the config file. Thread-safe; share it in an `Arc`.
pub struct SettingsStore {
    app_paths: AppPaths,
    file_system: Arc<dyn FileSystem>,
    log: Arc<dyn Log>,
    clock: Arc<dyn Clock>,
    texts: Arc<dyn SettingsTexts>,
    on_startup_error: Option<StartupErrorCallback>,
    key_names: Arc<dyn KeyNameValidator>,
    validator: SettingsValidator,
    sleeper: RetrySleeper,
    state: Mutex<State>,
    handlers: Mutex<Vec<(usize, Arc<ChangedHandler>)>>,
    next_handler_id: AtomicUsize,
}

impl fmt::Debug for SettingsStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsStore").field("app_paths", &self.app_paths).finish_non_exhaustive()
    }
}

/// The folder-less file name of the C# app's settings file; see [`csharp_config_path`].
pub const CONFIG_FILE_NAME: &str = "config.json";

/// Rust-only. `<local_app_data>\PhotoReview\config.json`: where the C# app keeps its settings (R0002: read-only,
/// first-run import only). `identity::CSHARP_APP_DIR_NAME` is the folder name.
pub fn csharp_config_path(local_app_data: &Path) -> PathBuf {
    todo!("W2-03")
}

impl SettingsStore {
    /// `MaxNewerBackups`: how many `config.json.newer-*` backups are kept.
    pub const MAX_NEWER_BACKUPS: usize = 3;

    /// `ReadRetryDelaysMs`: the pauses before retry 1..=4 of a failed read.
    pub const READ_RETRY_DELAYS_MS: [u64; 4] = [50, 100, 200, 400];

    /// `new SettingsStore(appPaths, fileSystem, log, onStartupError, keyNameValidator)`. `clock` stands for
    /// `DateTime.UtcNow` (backup stamps), `texts` for the ambient `Tr`. `Current` starts as `AppSettings::new(texts)`
    /// (so `hidden_context_menu_items` is `None`, as in C#). Key names default to [`SimpleKeyNameValidator`], the
    /// read-retry sleeper to `std::thread::sleep`, no startup-error callback (errors go to `log.error`).
    pub fn new(
        app_paths: AppPaths,
        file_system: Arc<dyn FileSystem>,
        log: Arc<dyn Log>,
        clock: Arc<dyn Clock>,
        texts: Arc<dyn SettingsTexts>,
    ) -> Self {
        todo!("W2-03")
    }

    /// Sets the `onStartupError` callback.
    pub fn with_startup_error_callback(self, callback: StartupErrorCallback) -> Self {
        todo!("W2-03")
    }

    /// Sets the key-name validator (`KeyNames`) and rebuilds the `SettingsValidator` that uses it.
    pub fn with_key_name_validator(self, key_names: Arc<dyn KeyNameValidator>) -> Self {
        todo!("W2-03")
    }

    /// Replaces the sleeper used between read retries (tests record the delays instead of waiting).
    pub fn with_read_retry_sleeper(self, sleeper: RetrySleeper) -> Self {
        todo!("W2-03")
    }

    /// `Current`.
    pub fn current(&self) -> Arc<AppSettings> {
        todo!("W2-03")
    }

    /// `KeyNames`.
    pub fn key_names(&self) -> Arc<dyn KeyNameValidator> {
        todo!("W2-03")
    }

    /// `LastLoadRepairs`: what the last `load` had to repair (reset names, `Shortcuts.<Name>` entries,
    /// `Actions[<i>].Shortcut`); empty when the file was valid. Reset to empty at the start of every `load`.
    pub fn last_load_repairs(&self) -> Vec<String> {
        todo!("W2-03")
    }

    /// `Changed += handler`; returns an id for [`SettingsStore::unsubscribe_changed`].
    pub fn subscribe_changed(&self, handler: ChangedHandler) -> usize {
        todo!("W2-03")
    }

    /// `Changed -= handler`; false when the id is unknown.
    pub fn unsubscribe_changed(&self, id: usize) -> bool {
        todo!("W2-03")
    }

    /// `ValidateShortcuts(settings)`: `SettingsValidator::validate_shortcuts` with this store's key-name validator.
    pub fn validate_shortcuts(&self, settings: &AppSettings) -> Option<String> {
        todo!("W2-03")
    }

    /// `Load(path)`: `path` `None` = the default config file (`app_paths.config_file`), otherwise that file (nothing
    /// is ever written for an explicit path except the backup of a broken explicit file next to it).
    ///
    /// The full state machine (messages, order, which flag is set where) is in the contract. In short: read with
    /// retry; `parse_text`; salvage -> startup error + `.corrupt-` backup; `JsonException` -> startup error + backup +
    /// defaults written; IO failure -> in-memory defaults and `keep_corrupt_file`; repairs of the default file are
    /// written back once (after a `.repaired-` backup unless a salvage backup was made), never for a file from a
    /// newer build; `Changed` is raised exactly once, after all file work.
    ///
    /// `Err` only when a `Changed` handler returned `Err`; `current()` already holds the loaded settings then.
    pub fn load(&self, path: Option<&Path>) -> io::Result<Arc<AppSettings>> {
        todo!("W2-03")
    }

    /// `Save(settings)`: persists `settings` (canonicalised, stamped with the current version, written atomically and
    /// durably to `app_paths.config_file`, folder created first), makes them current and raises `Changed` once.
    /// On a write failure `Err` is returned, the previous file and `current()` are untouched and `Changed` is not raised.
    /// After a load of a newer-build file the first save backs that file up as `config.json.newer-<stamp>` first.
    pub fn save(&self, settings: AppSettings) -> Result<Arc<AppSettings>, SettingsSaveError> {
        todo!("W2-03")
    }

    /// `ParseText(json)`: the ONE parse pipeline for config.json-shaped text, shared by `load` and Settings > Import.
    /// No disk access. `Err` when the text is not parseable JSON or not a JSON object (the JSON text `null` is `Ok` with `is_json_null` and defaults).
    ///
    /// Steps: parse; read the whole document, or on failure salvage property by property ([`SettingsStore::try_salvage`]);
    /// mark an unversioned file as legacy; migrate; `normalizer::normalize` (the result minus `salvaged` is appended to
    /// `salvaged` to form `reset_settings`); `disable_conflicting_optional_shortcuts`.
    pub fn parse_text(text: &str, texts: &dyn SettingsTexts) -> Result<SettingsParseResult, SettingsJsonError> {
        todo!("W2-03")
    }

    /// `Parse(json)` (internal test seam in C#): read the whole document and run only the post-processing
    /// (`FinishDeserialize`: legacy mark, migrate), no normalize. `Err` like `parse_text`.
    pub fn parse(text: &str, texts: &dyn SettingsTexts) -> Result<AppSettings, SettingsJsonError> {
        todo!("W2-03")
    }

    /// `Migrate(settings)` (public static): with no raw text to inspect, a blank or `auto` (ASCII-case-insensitive
    /// `OrdinalIgnoreCase`) `ui_language` counts as not chosen. Version below 3 and not chosen: `ui_language` = `"vi"`.
    /// Then the version is raised to the current one, a blank language becomes `auto`, and a missing
    /// `hidden_context_menu_items` is materialised from the legacy flags (`context_menu::to_stored(legacy_hidden(..))`).
    pub fn migrate(settings: &mut AppSettings) {
        todo!("W2-03")
    }

    /// `TrySalvage`: reads `root` (an already parsed document) property by property with
    /// `AppSettings::apply_json_property`, starting from `AppSettings::new(texts)`. `None` when `root` is not an object
    /// (or has a property name with an unpaired surrogate); otherwise the settings and the names that could not be read
    /// (in file order, a name once per failing entry; unknown names are skipped).
    pub fn try_salvage(root: &json::JsonValue, texts: &dyn SettingsTexts) -> Option<(AppSettings, Vec<String>)> {
        todo!("W2-03")
    }

    /// Rust-only (R0002). First start of the Rust app: if the Rust `config.json` does not exist yet and the C#
    /// app's does, import it READ-ONLY.
    ///
    /// * Own config exists, or the C# file does not exist: exactly `load(None)`.
    /// * Otherwise read the C# file (same retry as `load`), `parse_text` it, make the result current, log an
    ///   info line, set `last_load_repairs` from the parse result, write the result to the Rust `config.json`
    ///   (canonicalised, version 3) and raise `Changed` once.
    /// * The C# file is never written, copied, renamed or deleted: no `.corrupt-` backup next to it, whatever it
    ///   contains. If it cannot be read or is not a JSON object, log a warning and continue as `load(None)`
    ///   (defaults are written).
    pub fn load_first_run_with_import(&self, csharp_config: &Path) -> io::Result<Arc<AppSettings>> {
        todo!("W2-03")
    }

    // ---- private helpers (signatures are suggestions; the contract describes the behaviour) ----

    /// `ReadAllTextWithRetry`: up to 4 retries (pauses [`SettingsStore::READ_RETRY_DELAYS_MS`]) on any `io::Error`;
    /// before each retry `log.info("config.json read failed (<IOException|UnauthorizedAccessException>), retrying: <message>")`.
    fn read_all_text_with_retry(&self, file_path: &Path) -> io::Result<String> {
        todo!("W2-03")
    }

    /// `Persist`: see `save`; also the keep-corrupt-file guard and the newer-file backup.
    fn persist(&self, settings: AppSettings) -> Result<(), SettingsSaveError> {
        todo!("W2-03")
    }

    /// `TryPersistRepairs`.
    fn try_persist_repairs(&self, repaired: AppSettings) -> bool {
        todo!("W2-03")
    }

    /// `BackupNewerConfig`.
    fn backup_newer_config(&self, file_path: &Path) {
        todo!("W2-03")
    }

    /// `NewerBackupPath`.
    fn newer_backup_path(&self, file_path: &Path) -> PathBuf {
        todo!("W2-03")
    }

    /// `NewerBackupSortKey`: `"yyyyMMddHHmmss"` or `"yyyyMMddHHmmss-N"` to `(stamp, N)`.
    fn newer_backup_sort_key(suffix: &str) -> (String, i32) {
        todo!("W2-03")
    }

    /// `UniqueBackupPath(filePath, kind)`: `<file>.<kind>-<stamp>`, then `-2` ... `-99` while the name exists.
    fn unique_backup_path(&self, file_path: &Path, kind: &str) -> PathBuf {
        todo!("W2-03")
    }

    /// `LogStartupError`: the callback when set, else `log.error(message, Some(error))`.
    fn log_startup_error(&self, message: &str, error: &(dyn Error + 'static)) {
        todo!("W2-03")
    }

    /// `Changed?.Invoke(this, settings)`: handlers in subscription order from a snapshot of the list taken without
    /// holding any lock; the first `Err` stops the loop and is returned.
    fn raise_changed(&self, settings: &Arc<AppSettings>) -> io::Result<()> {
        todo!("W2-03")
    }

    /// `DateTime.UtcNow.ToString("yyyyMMddHHmmss", InvariantCulture)` from `clock.utc_now()` (take the first 19
    /// characters of `DateTimeUtc::to_json()` and keep the digits).
    fn stamp(now: DateTimeUtc) -> String {
        todo!("W2-03")
    }

    /// `FinishDeserialize`: `mark_unversioned_as_legacy`, `migrate_core`, defaults for a missing `shortcuts`/`actions`
    /// (nothing to do in Rust).
    fn finish_deserialize(loaded: &mut AppSettings, root: &json::JsonValue) {
        todo!("W2-03")
    }

    /// `MarkUnversionedAsLegacy`: an object root without a `ConfigVersion` property gets `config_version` 1.
    fn mark_unversioned_as_legacy(loaded: &mut AppSettings, root: &json::JsonValue) {
        todo!("W2-03")
    }

    /// `UiLanguageDeclared`: the root is an object with a `UiLanguage` property (whatever its value).
    fn ui_language_declared(root: &json::JsonValue) -> bool {
        todo!("W2-03")
    }

    /// Private `Migrate(settings, uiLanguageDeclared)`.
    fn migrate_core(settings: &mut AppSettings, ui_language_declared: bool) {
        todo!("W2-03")
    }
}
