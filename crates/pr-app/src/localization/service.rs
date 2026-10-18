//! Port of `PhotoReview.App.Shared/Localization/LocalizationService.cs` (C# baseline d92d6aae, 121 lines) and of the
//! `TranslatorMode` enum in the same file (I18N L09). Owns the UI language (ADR 0006): loads catalogs through
//! `pr_l10n::loader::LanguageLoader`, publishes the result as the ambient `Localizer`, and switches the ambient
//! UI culture. Unit W8-04a. Contract: `contracts/W8-04a.md`.
//!
//! Deliberate differences from C# (each becomes a `Different` row in PARITY.md):
//! - `CultureInfo.GetCultureInfo` (throws for unknown names) becomes [`CultureLookup::lookup`] (`None` = not found);
//!   the production lookup lives in `pr-platform-win` (`GetLocaleInfoEx`).
//! - `CultureInfo.InstalledUICulture` is passed in as [`LocalizationService::new`]'s `installed_ui_culture`.
//! - `CultureInfo.DefaultThreadCurrentUICulture` has no Rust equivalent (threads do not inherit the ambient
//!   culture): only `pr_l10n::loader::set_current_ui_culture` is set.
//! - `ShippedLanguagesDir` (`AppContext.BaseDirectory`) and `UserLanguagesDir` (`<config dir>\Languages`) are values
//!   in [`LocalizationPaths`] supplied by the App.

#![allow(unused_variables, dead_code)] // D11: todo!() bodies; the porter removes this line with the last todo!()

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_core::abstractions::{FileSystem, Log};
use pr_l10n::loader::{LanguageInfo, LanguageLoader, UiCulture};
use pr_l10n::localizer::Localizer;

/// C# `TranslatorMode`: translator aids applied on top of the chosen language (command line `--i18n-pseudo` /
/// `--i18n-keys`, I18N L09).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TranslatorMode {
    /// No aid.
    #[default]
    None,
    /// Accented, longer, bracketed text: finds hard-coded strings and clipped layouts (`pr_l10n::localizer::create_pseudo`).
    Pseudo,
    /// Each text shows its catalog key (`pr_l10n::localizer::create_show_keys`).
    Keys,
}

/// C# `GetCultureInfo(name)` without the exception: `Some` for a culture .NET knows, `None` otherwise.
/// Production: `pr-platform-win` (`GetLocaleInfoEx`). Tests: a fixed table of the culture names the goldens use.
pub trait CultureLookup: Send + Sync {
    /// The culture for a language code, or `None` when .NET would throw `CultureNotFoundException`.
    fn lookup(&self, name: &str) -> Option<UiCulture>;
}

/// The two folders the service reads. C# `IAppPaths.UserLanguagesDir` is `<dir of ConfigFile>\Languages`; the App
/// computes it and passes it here. `shipped_dir` is C# `ShippedLanguagesDir` (`<app>\Languages`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalizationPaths {
    pub shipped_dir: PathBuf,
    pub user_languages_dir: PathBuf,
}

/// C# `LocalizationService`. Not `Sync`-shared: owned by the UI thread. `load` only reads files (worker-safe);
/// `apply` and `switch` publish and must run on the UI thread.
pub struct LocalizationService {
    loader: LanguageLoader,
    user_languages_dir: PathBuf,
    installed_ui_culture: UiCulture,
    cultures: Arc<dyn CultureLookup>,
    log: Arc<dyn Log>,
    mode: TranslatorMode,
    requested_language: String,
}

impl fmt::Debug for LocalizationService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalizationService")
            .field("user_languages_dir", &self.user_languages_dir)
            .field("installed_ui_culture", &self.installed_ui_culture)
            .field("mode", &self.mode)
            .field("requested_language", &self.requested_language)
            .finish_non_exhaustive()
    }
}

impl LocalizationService {
    /// C# public constructor `LocalizationService(IAppPaths, IFileSystem, ILog)`, with the loader built from
    /// `paths` (`LanguageLoader::new(file_system, shipped_dir, Some(user_languages_dir))`). `requested_language` starts
    /// as `auto`. Nothing is read here.
    pub fn new(
        file_system: Arc<dyn FileSystem>,
        paths: LocalizationPaths,
        installed_ui_culture: UiCulture,
        cultures: Arc<dyn CultureLookup>,
        log: Arc<dyn Log>,
    ) -> Self {
        todo!()
    }

    /// C# `UserLanguagesDir`.
    pub fn user_languages_dir(&self) -> &Path {
        todo!()
    }

    /// C# `Mode` (get).
    pub fn mode(&self) -> TranslatorMode {
        todo!()
    }

    /// C# `Mode` (set): takes effect at the next `load`.
    pub fn set_mode(&mut self, mode: TranslatorMode) {
        todo!()
    }

    /// C# `RequestedLanguage`: the setting value last applied (`auto` or a code, as given; blank becomes `auto`).
    pub fn requested_language(&self) -> &str {
        todo!()
    }

    /// C# `DiscoverLanguages()` (delegates to `LanguageLoader::discover_languages`).
    pub fn discover_languages(&self) -> Vec<LanguageInfo> {
        todo!()
    }

    /// C# `Load(language)`: `loader.load(language, Some(installed_ui_culture))` (never the ambient culture), then
    /// `Pseudo` -> `create_pseudo`, `Keys` -> `create_show_keys`, `None` -> unchanged. Reads files, touches no global
    /// state (the `Localizer` is returned, not published).
    pub fn load(&self, language: Option<&str>) -> Localizer {
        todo!()
    }

    /// C# `LocalizationService.ParseMode(args)`: `--i18n-keys` wins over `--i18n-pseudo`; both are matched element by
    /// element with `OrdinalIgnoreCase`; near misses (`--i18n`, `i18n-keys`) do not match.
    pub fn parse_mode<S: AsRef<str>>(args: &[S]) -> TranslatorMode {
        todo!()
    }

    /// C# `Apply(language, localizer)`. In this order:
    /// 1. `requested_language` = `auto` when `language` is `None` or blank, else `language` as given;
    /// 2. each `localizer.warnings()` entry is logged with `warn` as `"i18n: " + warning`;
    /// 3. the UI culture is set to `cultures.lookup(code)` where `code` is `en` in `Pseudo` mode and
    ///    `localizer.code()` otherwise, through `pr_l10n::loader::set_current_ui_culture`; `None` from the lookup keeps
    ///    the culture unchanged (C# `CultureNotFoundException`);
    /// 4. `localizer` is published with `pr_l10n::localizer::Localizer::set_current` (same `Arc`, so `Arc::ptr_eq` holds).
    ///
    /// Must run on the UI thread.
    pub fn apply(&mut self, language: Option<&str>, localizer: Arc<Localizer>) {
        todo!()
    }

    /// C# `Switch(language)` = `Apply(language, Load(language))`.
    pub fn switch(&mut self, language: Option<&str>) {
        todo!()
    }

    /// C# `Reload()`: re-reads `requested_language`, applies it, returns the warnings of the new localizer
    /// (skipped files, entries that fall back to English).
    pub fn reload(&mut self) -> Vec<String> {
        todo!()
    }

    /// C# `ResolveCode(language)`: `loader.resolve(language, Some(installed_ui_culture))`; unknown codes give `en`.
    pub fn resolve_code(&self, language: Option<&str>) -> String {
        todo!()
    }
}
