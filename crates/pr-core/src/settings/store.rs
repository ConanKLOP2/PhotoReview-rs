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

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::abstractions::{Clock, FileSystem, KeyNameValidator, Log};
use crate::app_paths::AppPaths;
use crate::identity::CSHARP_APP_DIR_NAME;
use crate::settings::app_settings::{
    AUTO_LANGUAGE_CODE, AppSettings, CURRENT_CONFIG_VERSION, SettingsJsonError, SettingsTexts, json,
};
use crate::settings::context_menu;
use crate::settings::normalizer::{
    SettingsParseResult, SettingsValidator, disable_conflicting_optional_shortcuts, normalize,
};
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
        f.debug_struct("SettingsStore")
            .field("app_paths", &self.app_paths)
            .finish_non_exhaustive()
    }
}

/// The folder-less file name of the C# app's settings file; see [`csharp_config_path`].
pub const CONFIG_FILE_NAME: &str = "config.json";

/// Rust-only. `<local_app_data>\PhotoReview\config.json`: where the C# app keeps its settings (R0002: read-only,
/// first-run import only). `identity::CSHARP_APP_DIR_NAME` is the folder name.
pub fn csharp_config_path(local_app_data: &Path) -> PathBuf {
    local_app_data.join(CSHARP_APP_DIR_NAME).join(CONFIG_FILE_NAME)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The .NET exception type name `ReadAllTextWithRetry` logs for an IO error.
fn dotnet_io_type_name(e: &io::Error) -> &'static str {
    if e.kind() == io::ErrorKind::PermissionDenied {
        "UnauthorizedAccessException"
    } else {
        "IOException"
    }
}

/// The Rust-only non-finite-number error where the signature only allows `io::Error` (never happens for values
/// that went through `normalize`).
fn json_to_io(e: SettingsJsonError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e)
}

/// `string.IsNullOrWhiteSpace`.
fn is_blank(s: &str) -> bool {
    s.trim().is_empty()
}

fn non_empty_parent(path: &Path) -> Option<&Path> {
    path.parent().filter(|p| !p.as_os_str().is_empty())
}

fn file_name_string(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn append_to_path(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
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
        let key_names: Arc<dyn KeyNameValidator> = Arc::new(SimpleKeyNameValidator);
        let validator = SettingsValidator::new(Arc::clone(&key_names), Arc::clone(&texts));
        let current = Arc::new(AppSettings::new(&*texts));
        Self {
            app_paths,
            file_system,
            log,
            clock,
            texts,
            on_startup_error: None,
            key_names,
            validator,
            sleeper: Arc::new(std::thread::sleep),
            state: Mutex::new(State {
                current,
                newer_file_on_disk: false,
                keep_corrupt_file: false,
                last_load_repairs: Vec::new(),
            }),
            handlers: Mutex::new(Vec::new()),
            next_handler_id: AtomicUsize::new(1),
        }
    }

    /// Sets the `onStartupError` callback.
    pub fn with_startup_error_callback(mut self, callback: StartupErrorCallback) -> Self {
        self.on_startup_error = Some(callback);
        self
    }

    /// Sets the key-name validator (`KeyNames`) and rebuilds the `SettingsValidator` that uses it.
    pub fn with_key_name_validator(mut self, key_names: Arc<dyn KeyNameValidator>) -> Self {
        self.validator = SettingsValidator::new(Arc::clone(&key_names), Arc::clone(&self.texts));
        self.key_names = key_names;
        self
    }

    /// Replaces the sleeper used between read retries (tests record the delays instead of waiting).
    pub fn with_read_retry_sleeper(mut self, sleeper: RetrySleeper) -> Self {
        self.sleeper = sleeper;
        self
    }

    /// `Current`.
    pub fn current(&self) -> Arc<AppSettings> {
        Arc::clone(&lock(&self.state).current)
    }

    /// `KeyNames`.
    pub fn key_names(&self) -> Arc<dyn KeyNameValidator> {
        Arc::clone(&self.key_names)
    }

    /// `LastLoadRepairs`: what the last `load` had to repair (reset names, `Shortcuts.<Name>` entries,
    /// `Actions[<i>].Shortcut`); empty when the file was valid. Reset to empty at the start of every `load`.
    pub fn last_load_repairs(&self) -> Vec<String> {
        lock(&self.state).last_load_repairs.clone()
    }

    /// `Changed += handler`; returns an id for [`SettingsStore::unsubscribe_changed`].
    pub fn subscribe_changed(&self, handler: ChangedHandler) -> usize {
        let id = self.next_handler_id.fetch_add(1, Ordering::Relaxed);
        lock(&self.handlers).push((id, Arc::new(handler)));
        id
    }

    /// How many `Changed` handlers are subscribed right now (C# `Changed.GetInvocationList().Length`): lets a whole-shell test prove that closing the window unsubscribes the overlay.
    pub fn changed_handler_count(&self) -> usize {
        lock(&self.handlers).len()
    }

    /// `Changed -= handler`; false when the id is unknown.
    pub fn unsubscribe_changed(&self, id: usize) -> bool {
        let mut handlers = lock(&self.handlers);
        let before = handlers.len();
        handlers.retain(|(h, _)| *h != id);
        handlers.len() != before
    }

    /// `ValidateShortcuts(settings)`: `SettingsValidator::validate_shortcuts` with this store's key-name validator.
    pub fn validate_shortcuts(&self, settings: &AppSettings) -> Option<String> {
        self.validator.validate_shortcuts(settings)
    }

    fn reset_load_state(&self) {
        let mut st = lock(&self.state);
        st.last_load_repairs = Vec::new();
        st.keep_corrupt_file = false;
        st.newer_file_on_disk = false;
    }

    fn set_keep_corrupt_file(&self, value: bool) {
        lock(&self.state).keep_corrupt_file = value;
    }

    fn set_current(&self, settings: Arc<AppSettings>) {
        lock(&self.state).current = settings;
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
        let default_path = path.is_none();
        let file_path: PathBuf = path.map_or_else(|| self.app_paths.config_file.clone(), Path::to_path_buf);
        self.reset_load_state();
        let mut ready: Option<Arc<AppSettings>> = None;

        if self.file_system.file_exists(&file_path) {
            match self.read_all_text_with_retry(&file_path) {
                Err(e) => {
                    self.log_startup_error(
                        "Config.json inaccessible, using in-memory defaults for this session",
                        &e,
                    );
                    self.set_keep_corrupt_file(default_path);
                    return self.use_in_memory_defaults();
                }
                Ok(text) => match Self::parse_text(&text, &*self.texts) {
                    Err(e) => {
                        self.log_startup_error("Corrupt config.json detected, resetting to defaults", &e);
                        let backup = self.unique_backup_path(&file_path, "corrupt");
                        if let Err(copy_err) = self.file_system.copy_file(&file_path, &backup) {
                            self.log_startup_error(
                                "Could not back up corrupt config.json, using in-memory defaults for this session",
                                &copy_err,
                            );
                            self.set_keep_corrupt_file(default_path);
                            return self.use_in_memory_defaults();
                        }
                    }
                    Ok(parsed) => {
                        ready = Some(self.apply_parsed(&file_path, default_path, parsed)?);
                    }
                },
            }
        }

        if let Some(ready) = ready {
            self.raise_changed(&ready)?;
            return Ok(ready);
        }

        let settings = AppSettings::new(&*self.texts);
        if default_path {
            match self.persist(settings.clone()) {
                Ok(()) => {}
                Err(SettingsSaveError::Io(e)) => {
                    self.log_startup_error(
                        "Could not write default config.json, using in-memory defaults for this session",
                        &e,
                    );
                    self.set_current(Arc::new(settings));
                }
                Err(SettingsSaveError::Json(e)) => return Err(json_to_io(e)),
            }
        } else {
            self.set_current(Arc::new(settings));
        }
        let current = self.current();
        self.raise_changed(&current)?;
        Ok(current)
    }

    /// The two early-return branches of `Load`: defaults in memory, `Changed` once, nothing written.
    fn use_in_memory_defaults(&self) -> io::Result<Arc<AppSettings>> {
        let defaults = Arc::new(AppSettings::new(&*self.texts));
        self.set_current(Arc::clone(&defaults));
        self.raise_changed(&defaults)?;
        Ok(defaults)
    }

    /// Steps a-d of the C# success branch; returns the settings `Changed` is raised with.
    fn apply_parsed(
        &self,
        file_path: &Path,
        default_path: bool,
        parsed: SettingsParseResult,
    ) -> io::Result<Arc<AppSettings>> {
        let mut backed_up = false;
        if let Some(cause) = &parsed.salvage_cause {
            self.log_startup_error(
                &format!(
                    "config.json has unreadable values ({}); the rest was kept",
                    parsed.salvaged.join(", ")
                ),
                cause,
            );
            let backup = self.unique_backup_path(file_path, "corrupt");
            match self.file_system.copy_file(file_path, &backup) {
                Ok(()) => backed_up = true,
                Err(e) => {
                    self.set_keep_corrupt_file(default_path);
                    self.log_startup_error("Could not back up config.json with unreadable values", &e);
                }
            }
        }
        if !parsed.reset_settings.is_empty() {
            self.log.warn(&format!(
                "config.json had invalid values, reset to defaults: {}",
                parsed.reset_settings.join(", ")
            ));
        }
        if !parsed.disabled_shortcuts.is_empty() {
            self.log.info(&format!(
                "Optional shortcuts disabled because their key is already bound: {}",
                parsed.disabled_shortcuts.join(", ")
            ));
        }
        let repairs = parsed.repairs();
        let loaded = Arc::new(parsed.settings);
        let version = loaded.config_version;
        {
            let mut st = lock(&self.state);
            st.last_load_repairs = repairs.clone();
            st.current = Arc::clone(&loaded);
            st.newer_file_on_disk = default_path && version > CURRENT_CONFIG_VERSION;
        }
        if !repairs.is_empty() && default_path && version <= CURRENT_CONFIG_VERSION {
            if !backed_up {
                let backup = self.unique_backup_path(file_path, "repaired");
                if let Err(e) = self.file_system.copy_file(file_path, &backup) {
                    self.set_keep_corrupt_file(true);
                    self.log_startup_error("Could not back up config.json before writing repairs", &e);
                }
            }
            self.try_persist_repairs((*loaded).clone())?;
        }
        Ok(self.current())
    }

    /// `Save(settings)`: persists `settings` (canonicalised, stamped with the current version, written atomically and
    /// durably to `app_paths.config_file`, folder created first), makes them current and raises `Changed` once.
    /// On a write failure `Err` is returned, the previous file and `current()` are untouched and `Changed` is not raised.
    /// After a load of a newer-build file the first save backs that file up as `config.json.newer-<stamp>` first.
    pub fn save(&self, settings: AppSettings) -> Result<Arc<AppSettings>, SettingsSaveError> {
        self.persist(settings)?;
        let current = self.current();
        self.raise_changed(&current)?;
        Ok(current)
    }

    /// `ParseText(json)`: the ONE parse pipeline for config.json-shaped text, shared by `load` and Settings > Import.
    /// No disk access. `Err` when the text is not parseable JSON or not a JSON object (the JSON text `null` is `Ok` with `is_json_null` and defaults).
    ///
    /// Steps: parse; read the whole document, or on failure salvage property by property ([`SettingsStore::try_salvage`]);
    /// mark an unversioned file as legacy; migrate; `normalizer::normalize` (the result minus `salvaged` is appended to
    /// `salvaged` to form `reset_settings`); `disable_conflicting_optional_shortcuts`.
    pub fn parse_text(text: &str, texts: &dyn SettingsTexts) -> Result<SettingsParseResult, SettingsJsonError> {
        let root = json::parse(text)?;
        let mut salvaged: Vec<String> = Vec::new();
        let mut cause: Option<SettingsJsonError> = None;
        let mut is_null = false;
        let mut loaded = match AppSettings::from_json_value(&root, texts) {
            Ok(Some(settings)) => settings,
            Ok(None) => {
                is_null = true;
                AppSettings::new(texts)
            }
            Err(e) => match Self::try_salvage(&root, texts) {
                Some((settings, unusable)) => {
                    salvaged = unusable;
                    cause = Some(e);
                    settings
                }
                None => return Err(e),
            },
        };
        Self::finish_deserialize(&mut loaded, &root);
        let normalized = normalize(&mut loaded);
        // `Enumerable.Except` is a set operation: the normalizer names are distinct and not in `salvaged`.
        let mut reset_settings = salvaged.clone();
        for name in normalized {
            if !salvaged.contains(&name) && !reset_settings.contains(&name) {
                reset_settings.push(name);
            }
        }
        let disabled_shortcuts = disable_conflicting_optional_shortcuts(&mut loaded);
        Ok(SettingsParseResult {
            settings: loaded,
            salvaged,
            reset_settings,
            disabled_shortcuts,
            salvage_cause: cause,
            is_json_null: is_null,
        })
    }

    /// `Parse(json)` (internal test seam in C#): read the whole document and run only the post-processing
    /// (`FinishDeserialize`: legacy mark, migrate), no normalize. `Err` like `parse_text`.
    pub fn parse(text: &str, texts: &dyn SettingsTexts) -> Result<AppSettings, SettingsJsonError> {
        let root = json::parse(text)?;
        let mut loaded = AppSettings::from_json_value(&root, texts)?.unwrap_or_else(|| AppSettings::new(texts));
        Self::finish_deserialize(&mut loaded, &root);
        Ok(loaded)
    }

    /// `Migrate(settings)` (public static): with no raw text to inspect, a blank or `auto` (ASCII-case-insensitive
    /// `OrdinalIgnoreCase`) `ui_language` counts as not chosen. Version below 3 and not chosen: `ui_language` = `"vi"`.
    /// Then the version is raised to the current one, a blank language becomes `auto`, and a missing
    /// `hidden_context_menu_items` is materialised from the legacy flags (`context_menu::to_stored(legacy_hidden(..))`).
    pub fn migrate(settings: &mut AppSettings) {
        let declared =
            !is_blank(&settings.ui_language) && !settings.ui_language.eq_ignore_ascii_case(AUTO_LANGUAGE_CODE);
        Self::migrate_core(settings, declared);
    }

    /// `TrySalvage`: reads `root` (an already parsed document) property by property with
    /// `AppSettings::apply_json_property`, starting from `AppSettings::new(texts)`. `None` when `root` is not an object
    /// (or has a property name with an unpaired surrogate); otherwise the settings and the names that could not be read
    /// (in file order, a name once per failing entry; unknown names are skipped).
    pub fn try_salvage(root: &json::JsonValue, texts: &dyn SettingsTexts) -> Option<(AppSettings, Vec<String>)> {
        let json::JsonValue::Object(entries) = root else {
            return None;
        };
        let mut settings = AppSettings::new(texts);
        let mut bad = Vec::new();
        for (name, value) in entries {
            let name = name.as_deref()?;
            // C# `TrySalvage` deserialises with the property TYPE (type-level `LenientEnumConverter`), so the eight
            // property-level `SettingsEnumConverter` properties fall back to `default(T)` instead of being unusable.
            let applied = match name {
                "LoadingMode" => salvage_enum(value, &mut settings.loading_mode),
                "ImageSortMode" => salvage_enum(value, &mut settings.image_sort_mode),
                "ScalingQuality" => salvage_enum(value, &mut settings.scaling_quality),
                "DecoderBackend" => salvage_enum(value, &mut settings.decoder_backend),
                "KineticGlideSmoothing" => salvage_enum(value, &mut settings.kinetic_glide_smoothing),
                "FitWidthAnchor2" => salvage_enum(value, &mut settings.fit_width_anchor2),
                "MiddleClickAction" => salvage_enum(value, &mut settings.middle_click_action),
                "KeyboardZoomAnchor" => salvage_enum(value, &mut settings.keyboard_zoom_anchor),
                _ => settings.apply_json_property(name, value, texts).is_ok(),
            };
            if !applied {
                bad.push(name.to_owned());
            }
        }
        Some((settings, bad))
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
        if self.file_system.file_exists(&self.app_paths.config_file) || !self.file_system.file_exists(csharp_config) {
            return self.load(None);
        }
        self.reset_load_state();
        let text = match self.read_all_text_with_retry(csharp_config) {
            Ok(t) => t,
            Err(e) => {
                self.log.warn(&format!(
                    "Could not read the C# config.json for the first-run import, starting with defaults: {e}"
                ));
                return self.load(None);
            }
        };
        let parsed = match Self::parse_text(&text, &*self.texts) {
            Ok(p) if p.is_json_null => {
                self.log.warn(
                    "The C# config.json is not a JSON object, so the first-run import is skipped and defaults are used",
                );
                return self.load(None);
            }
            Ok(p) => p,
            Err(e) => {
                self.log.warn(&format!(
                    "The C# config.json is not usable for the first-run import, starting with defaults: {e}"
                ));
                return self.load(None);
            }
        };
        let repairs = parsed.repairs();
        let imported = Arc::new(parsed.settings);
        {
            let mut st = lock(&self.state);
            st.current = Arc::clone(&imported);
            st.last_load_repairs = repairs.clone();
        }
        self.log.info(&format!(
            "Imported the settings of the C# app from {} ({} repaired)",
            csharp_config.display(),
            repairs.len()
        ));
        match self.persist((*imported).clone()) {
            Ok(()) => {}
            Err(SettingsSaveError::Io(e)) => {
                self.log_startup_error(
                    "Could not write default config.json, using in-memory defaults for this session",
                    &e,
                );
                self.set_current(Arc::clone(&imported));
            }
            Err(SettingsSaveError::Json(e)) => return Err(json_to_io(e)),
        }
        let current = self.current();
        self.raise_changed(&current)?;
        Ok(current)
    }

    // ---- private helpers ----

    /// `ReadAllTextWithRetry`: up to 4 retries (pauses [`SettingsStore::READ_RETRY_DELAYS_MS`]) on any `io::Error`;
    /// before each retry `log.info("config.json read failed (<IOException|UnauthorizedAccessException>), retrying: <message>")`.
    fn read_all_text_with_retry(&self, file_path: &Path) -> io::Result<String> {
        let mut attempt = 0;
        loop {
            match self.file_system.read_all_text(file_path) {
                Ok(text) => return Ok(text),
                Err(e) if attempt < Self::READ_RETRY_DELAYS_MS.len() => {
                    self.log.info(&format!(
                        "config.json read failed ({}), retrying: {}",
                        dotnet_io_type_name(&e),
                        e
                    ));
                    (self.sleeper)(Duration::from_millis(Self::READ_RETRY_DELAYS_MS[attempt]));
                    attempt += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// `Persist`: see `save`; also the keep-corrupt-file guard and the newer-file backup.
    fn persist(&self, settings: AppSettings) -> Result<(), SettingsSaveError> {
        let mut settings = settings;
        let keep = lock(&self.state).keep_corrupt_file;
        if keep {
            self.log.warn(
                "Settings kept in memory only: config.json is preserved untouched because it could not be read or backed up",
            );
            self.set_current(Arc::new(settings));
            return Ok(());
        }
        let file_path = self.app_paths.config_file.clone();
        if let Some(dir) = non_empty_parent(&file_path) {
            self.file_system.create_directory(dir)?;
        }
        let newer = lock(&self.state).newer_file_on_disk;
        if newer {
            self.backup_newer_config(&file_path);
            lock(&self.state).newer_file_on_disk = false;
        }
        settings.canonicalize_all();
        super::external_editors::ExternalEditors::reconcile(&mut settings);
        settings.config_version = CURRENT_CONFIG_VERSION;
        let json = settings.to_json()?;
        self.file_system.write_all_text_atomic(&file_path, &json, true)?;
        self.set_current(Arc::new(settings));
        Ok(())
    }

    /// `TryPersistRepairs`: `Ok(false)` when the file cannot be written (the repairs stay in memory only).
    fn try_persist_repairs(&self, repaired: AppSettings) -> io::Result<bool> {
        match self.persist(repaired) {
            Ok(()) => Ok(true),
            Err(SettingsSaveError::Io(e)) => {
                self.log_startup_error(
                    "Could not write the repaired config.json; the repairs apply to this session only",
                    &e,
                );
                Ok(false)
            }
            Err(SettingsSaveError::Json(e)) => Err(json_to_io(e)),
        }
    }

    /// `BackupNewerConfig`.
    fn backup_newer_config(&self, file_path: &Path) {
        let copied: io::Result<bool> = (|| {
            if !self.file_system.file_exists(file_path) {
                return Ok(false);
            }
            let backup = self.newer_backup_path(file_path)?;
            self.file_system.copy_file(file_path, &backup)?;
            self.log.info(&format!(
                "config.json from a newer build backed up before being overwritten: {}",
                backup.display()
            ));
            Ok(true)
        })();
        match copied {
            Ok(false) => return,
            Ok(true) => {}
            Err(e) => {
                self.log.warn(&format!(
                    "Could not back up the newer-build config.json before overwriting it: {e}"
                ));
                return;
            }
        }
        let Some(dir) = non_empty_parent(file_path) else {
            return;
        };
        let name = file_name_string(file_path);
        let prefix = format!("{name}.newer-");
        let files = match self.file_system.enumerate_files(dir, &format!("{name}.newer-*")) {
            Ok(files) => files,
            Err(e) => {
                self.log
                    .warn(&format!("Could not prune old newer-build config backups: {e}"));
                return;
            }
        };
        let mut keyed: Vec<(String, i32, PathBuf)> = files
            .into_iter()
            .filter_map(|f| {
                let file = file_name_string(&f);
                let suffix = file.strip_prefix(&prefix)?;
                let (stamp, counter) = Self::newer_backup_sort_key(suffix);
                Some((stamp, counter, f))
            })
            .collect();
        // Newest first: stamp descending (ordinal, UTF-16 code units), then counter descending. Stable like LINQ.
        keyed.sort_by(|a, b| b.0.encode_utf16().cmp(a.0.encode_utf16()).then_with(|| b.1.cmp(&a.1)));
        for (_, _, old) in keyed.into_iter().skip(Self::MAX_NEWER_BACKUPS) {
            if let Err(e) = self.file_system.delete(&old) {
                self.log
                    .warn(&format!("Could not delete an old newer-build config backup: {e}"));
            }
        }
    }

    /// `NewerBackupPath`.
    fn newer_backup_path(&self, file_path: &Path) -> io::Result<PathBuf> {
        let name = file_name_string(file_path);
        let stamp = Self::stamp(self.clock.utc_now());
        let stem = append_to_path(file_path, &format!(".newer-{stamp}"));
        let Some(dir) = non_empty_parent(file_path) else {
            return Ok(stem);
        };
        let prefix = format!("{name}.newer-");
        let mut max = 0;
        for existing in self.file_system.enumerate_files(dir, &format!("{prefix}{stamp}*"))? {
            let file = file_name_string(&existing);
            let Some(suffix) = file.strip_prefix(&prefix) else {
                continue;
            };
            let (key_stamp, counter) = Self::newer_backup_sort_key(suffix);
            if key_stamp == stamp && counter > max {
                max = counter;
            }
        }
        if max == 0 {
            Ok(stem)
        } else {
            Ok(append_to_path(&stem, &format!("-{}", max.wrapping_add(1))))
        }
    }

    /// `NewerBackupSortKey`: `"yyyyMMddHHmmss"` or `"yyyyMMddHHmmss-N"` to `(stamp, N)`.
    fn newer_backup_sort_key(suffix: &str) -> (String, i32) {
        match suffix.find('-') {
            None => (suffix.to_owned(), 1),
            Some(dash) => {
                let digits = &suffix[dash + 1..];
                // NumberStyles.None: digits only, no sign and no white space.
                let parsed = if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                    digits.parse::<i32>().ok()
                } else {
                    None
                };
                match parsed {
                    Some(n) => (suffix[..dash].to_owned(), n),
                    None => (suffix.to_owned(), 0),
                }
            }
        }
    }

    /// `UniqueBackupPath(filePath, kind)`: `<file>.<kind>-<stamp>`, then `-2` ... `-99` while the name exists.
    fn unique_backup_path(&self, file_path: &Path, kind: &str) -> PathBuf {
        let stamp = Self::stamp(self.clock.utc_now());
        let stem = append_to_path(file_path, &format!(".{kind}-{stamp}"));
        let mut candidate = stem.clone();
        let mut i = 2;
        while i < 100 && self.file_system.file_exists(&candidate) {
            candidate = append_to_path(&stem, &format!("-{i}"));
            i += 1;
        }
        candidate
    }

    /// `LogStartupError`: the callback when set, else `log.error(message, Some(error))`.
    fn log_startup_error(&self, message: &str, error: &(dyn Error + 'static)) {
        match &self.on_startup_error {
            Some(callback) => callback(message, error),
            None => self.log.error(message, Some(error)),
        }
    }

    /// `Changed?.Invoke(this, settings)`: handlers in subscription order from a snapshot of the list taken without
    /// holding any lock; the first `Err` stops the loop and is returned.
    fn raise_changed(&self, settings: &Arc<AppSettings>) -> io::Result<()> {
        let snapshot: Vec<Arc<ChangedHandler>> = lock(&self.handlers).iter().map(|(_, h)| Arc::clone(h)).collect();
        for handler in snapshot {
            handler(settings)?;
        }
        Ok(())
    }

    /// `DateTime.UtcNow.ToString("yyyyMMddHHmmss", InvariantCulture)` from `clock.utc_now()` (take the first 19
    /// characters of `DateTimeUtc::to_json()` and keep the digits).
    fn stamp(now: DateTimeUtc) -> String {
        now.to_json().chars().take(19).filter(char::is_ascii_digit).collect()
    }

    /// `FinishDeserialize`: `mark_unversioned_as_legacy`, `migrate_core`, defaults for a missing `shortcuts`/`actions`
    /// (nothing to do in Rust).
    fn finish_deserialize(loaded: &mut AppSettings, root: &json::JsonValue) {
        Self::mark_unversioned_as_legacy(loaded, root);
        Self::migrate_core(loaded, Self::ui_language_declared(root));
    }

    /// `MarkUnversionedAsLegacy`: an object root without a `ConfigVersion` property gets `config_version` 1.
    fn mark_unversioned_as_legacy(loaded: &mut AppSettings, root: &json::JsonValue) {
        if matches!(root, json::JsonValue::Object(_)) && root.property("ConfigVersion").is_none() {
            loaded.config_version = 1;
        }
    }

    /// `UiLanguageDeclared`: the root is an object with a `UiLanguage` property (whatever its value).
    fn ui_language_declared(root: &json::JsonValue) -> bool {
        matches!(root, json::JsonValue::Object(_)) && root.property("UiLanguage").is_some()
    }

    /// Private `Migrate(settings, uiLanguageDeclared)`.
    fn migrate_core(settings: &mut AppSettings, ui_language_declared: bool) {
        if settings.config_version < 3 && !ui_language_declared {
            settings.ui_language = "vi".to_owned();
        }
        if settings.config_version < 4 {
            super::external_editors::ExternalEditors::reconcile(settings);
        }
        if settings.config_version < CURRENT_CONFIG_VERSION {
            settings.config_version = CURRENT_CONFIG_VERSION;
        }
        if is_blank(&settings.ui_language) {
            settings.ui_language = AUTO_LANGUAGE_CODE.to_owned();
        }
        if settings.hidden_context_menu_items.is_none() {
            let stored = context_menu::to_stored(context_menu::legacy_hidden(settings));
            settings.hidden_context_menu_items = Some(stored.into_iter().map(Some).collect());
        }
    }
}

/// Type-level lenient read used by the salvage path: an unparsable value becomes `default(T)` (never reported).
fn salvage_enum<T: crate::model::serde_ext::LenientEnum>(value: &json::JsonValue, target: &mut Option<T>) -> bool {
    let Ok(v) = json::to_serde_value(value) else {
        return false;
    };
    match crate::model::serde_ext::deserialize_lenient::<T, _>(v) {
        Ok(m) => {
            *target = Some(m);
            true
        }
        Err(_) => false,
    }
}
