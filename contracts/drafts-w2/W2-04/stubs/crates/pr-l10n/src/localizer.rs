//! Port of `PhotoReview.Core/Localization/Localizer.cs` (156 lines), `BuiltInCatalog.cs` (26 lines) and the
//! `CreatePseudo` / `CreateShowKeys` plumbing of `TranslatorModes.cs` (63 lines), C# baseline d92d6aae.
//!
//! [`Localizer`] is the immutable merged view of one UI language. Lookup order per key: user file, shipped file,
//! built-in English, the key itself. The ambient instance ([`Localizer::current`], used by the generated `tr` API)
//! is replaced atomically and announces the change ([`on_current_changed`]).
//!
//! Number formatting: C# formats numbers with the ambient `CurrentCulture` (Windows regional settings, not the UI
//! language). Rust keeps that ambient value in [`number_culture`] / [`set_number_culture`]; `format_with` and
//! `format_plural_with` take the culture explicitly (tests use them).
//!
//! Unit W2-04. Stub: bodies are `todo!()`; data definitions are complete. Contract: `contracts/W2-04.md`.

// Stub-only (docs/MULTI-AGENT-PLAN D11): removed by the porter in the commit that removes the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::catalog::{LanguageCatalog, PluralRule};
use crate::template::{LocArg, LocTemplate, NumberCulture};

/// The language code of the built-in catalog. C# `Localizer.EnglishCode`.
pub const ENGLISH_CODE: &str = "en";

/// Immutable, merged view of one UI language.
#[derive(Debug)]
pub struct Localizer {
    code: String,
    native_name: String,
    plural: PluralRule,
    templates: HashMap<String, LocTemplate>,
    /// `baseKey -> (.one, .other)`, resolved once so `format_plural` needs one lookup.
    plurals: HashMap<String, (Option<LocTemplate>, LocTemplate)>,
    warnings: Vec<String>,
}

impl Localizer {
    /// Builds a localizer from English plus zero or more overlay catalogs of one language, applied in order (later
    /// overlays win: pass shipped first, user last). Overlay entries are validated against English: unknown keys are
    /// ignored (warning), a template with bad braces or a placeholder English does not have is rejected (the value
    /// from an earlier overlay, else English, stays; warning), a missing placeholder only warns.
    /// `prior_warnings` come first in [`warnings`](Self::warnings). C# `Localizer.Create`.
    pub fn create(english: &LanguageCatalog, overlays: &[LanguageCatalog], prior_warnings: &[String]) -> Localizer {
        todo!("see contracts/W2-04.md section 'Localizer::create'")
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn native_name(&self) -> &str {
        &self.native_name
    }

    pub fn plural(&self) -> PluralRule {
        self.plural
    }

    /// Problems found while building this localizer (rejected keys, unknown keys, broken files).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// All keys this localizer can resolve (the English key set), in no particular order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.templates.keys().map(String::as_str)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.templates.contains_key(key)
    }

    /// The text of `key` with no arguments (placeholders stay visible as `{name}`), or `key` itself when unknown.
    /// C# `Localizer.Get`.
    pub fn get(&self, key: &str) -> String {
        todo!()
    }

    /// `Format(key, args)` with the ambient [`number_culture`]. Unknown key -> the key.
    pub fn format(&self, key: &str, args: &[LocArg]) -> String {
        todo!()
    }

    /// `Format(key, args)` with an explicit culture.
    pub fn format_with(&self, key: &str, args: &[LocArg], culture: &NumberCulture) -> String {
        todo!()
    }

    /// `FormatPlural(baseKey, count, args)` with the ambient [`number_culture`].
    pub fn format_plural(&self, base_key: &str, count: i64, args: &[LocArg]) -> String {
        todo!()
    }

    /// `FormatPlural` with an explicit culture: picks `baseKey.one` or `baseKey.other` by [`plural`](Self::plural),
    /// then formats it. See the contract for the exact fall-backs.
    pub fn format_plural_with(&self, base_key: &str, count: i64, args: &[LocArg], culture: &NumberCulture) -> String {
        todo!()
    }

    /// A copy with every template rewritten (translator modes). Warnings are carried over.
    pub fn transform(
        &self,
        code: &str,
        native_name: &str,
        plural: PluralRule,
        transform: impl FnMut(&str, &LocTemplate) -> LocTemplate,
    ) -> Localizer {
        todo!()
    }

    /// Ambient localizer. Defaults to [`built_in_english_localizer`] until [`set_current`](Self::set_current) runs.
    pub fn current() -> Arc<Localizer> {
        todo!("read CURRENT; install built-in English if empty (compare-exchange semantics)")
    }

    /// Replaces the ambient localizer atomically, then runs the [`on_current_changed`] handlers (outside every lock,
    /// after the new value is visible to `current()`).
    pub fn set_current(localizer: Arc<Localizer>) {
        todo!()
    }

    /// Test seam for C# `LocalizerCurrentTests.Current_NotReplaced_IsBuiltInEnglish` (reflection on `s_current`):
    /// clears the ambient value so [`current`](Self::current) falls back to built-in English again.
    #[doc(hidden)]
    pub fn clear_current_for_tests() {
        todo!()
    }
}

/// Process-wide ambient localizer storage.
static CURRENT: RwLock<Option<Arc<Localizer>>> = RwLock::new(None);

/// Keeps an [`on_current_changed`] handler registered; dropping it unregisters the handler
/// (C# `CurrentChanged -= handler`).
#[derive(Debug)]
pub struct CurrentChangedSubscription {
    id: u64,
}

impl Drop for CurrentChangedSubscription {
    fn drop(&mut self) {
        todo!("remove the handler with this id")
    }
}

/// C# `Localizer.CurrentChanged += handler`: called on whichever thread calls `set_current`, once per call.
pub fn on_current_changed(handler: impl Fn() + Send + Sync + 'static) -> CurrentChangedSubscription {
    todo!()
}

/// The number format of the user's regional settings (C# `CultureInfo.CurrentCulture`); default invariant. The app
/// sets it at startup and whenever the Windows regional settings change.
pub fn number_culture() -> Arc<NumberCulture> {
    todo!()
}

/// Sets the ambient number culture.
pub fn set_number_culture(culture: NumberCulture) {
    todo!()
}

/// `TranslatorModes.CreatePseudo`: every text becomes `[Accented text ~~~]` (see `template::pseudo_template`), code
/// `qps-ploc`, native name `"Pseudo"`, plural rule `OneOther`.
pub fn create_pseudo(source: &Localizer) -> Localizer {
    todo!()
}

/// `TranslatorModes.CreateShowKeys`: every text becomes `[key]`; code, native name and plural rule are kept.
pub fn create_show_keys(source: &Localizer) -> Localizer {
    todo!()
}

/// `BuiltInCatalog.English`: the English catalog embedded from `assets/languages/en.json`, always complete, the final
/// fallback for every key. Parsed once. Panics only if the embedded asset is invalid (a build-time bug).
pub fn built_in_english() -> &'static LanguageCatalog {
    todo!("OnceLock + LanguageCatalog::try_parse(EMBEDDED_ENGLISH, \"en (built-in)\", ..)")
}

/// `BuiltInCatalog.EnglishLocalizer`: `Localizer::create(built_in_english(), &[], &[])`, built once.
pub fn built_in_english_localizer() -> Arc<Localizer> {
    todo!()
}

/// The embedded English catalog text (C# embedded resource `PhotoReview.Core.Localization.en.json`).
pub const EMBEDDED_ENGLISH_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/languages/en.json"));
