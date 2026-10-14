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
//! Unit W2-04. Contract: `contracts/W2-04.md`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};

use crate::catalog::{LanguageCatalog, PluralRule};
use crate::template::{LocArg, LocTemplate, NumberCulture, PSEUDO_CODE, pseudo_template, show_key_template};

/// The language code of the built-in catalog. C# `Localizer.EnglishCode`.
pub const ENGLISH_CODE: &str = "en";

const OTHER_SUFFIX: &str = ".other";
const ONE_SUFFIX: &str = ".one";

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

/// C# constructor: resolves the `(.one, .other)` pairs once.
fn build_plurals(templates: &HashMap<String, LocTemplate>) -> HashMap<String, (Option<LocTemplate>, LocTemplate)> {
    let mut plurals = HashMap::new();
    for (key, template) in templates {
        let Some(base) = key.strip_suffix(OTHER_SUFFIX) else {
            continue;
        };
        let one = templates.get(&format!("{base}{ONE_SUFFIX}")).cloned();
        plurals.insert(base.to_owned(), (one, template.clone()));
    }
    plurals
}

impl Localizer {
    fn new(
        code: String,
        native_name: String,
        plural: PluralRule,
        templates: HashMap<String, LocTemplate>,
        warnings: Vec<String>,
    ) -> Self {
        let plurals = build_plurals(&templates);
        Localizer {
            code,
            native_name,
            plural,
            templates,
            plurals,
            warnings,
        }
    }

    /// Builds a localizer from English plus zero or more overlay catalogs of one language, applied in order (later
    /// overlays win: pass shipped first, user last). Overlay entries are validated against English: unknown keys are
    /// ignored (warning), a template with bad braces or a placeholder English does not have is rejected (the value
    /// from an earlier overlay, else English, stays; warning), a missing placeholder only warns.
    /// `prior_warnings` come first in [`warnings`](Self::warnings). C# `Localizer.Create`.
    pub fn create(english: &LanguageCatalog, overlays: &[LanguageCatalog], prior_warnings: &[String]) -> Localizer {
        let mut warnings: Vec<String> = prior_warnings.to_vec();
        let mut templates: HashMap<String, LocTemplate> = HashMap::new();
        for (key, text) in english.entries() {
            match LocTemplate::try_parse(text) {
                Some(t) => {
                    templates.insert(key.clone(), t);
                }
                None => {
                    warnings.push(format!("en: '{key}' has invalid placeholders"));
                    templates.insert(key.clone(), LocTemplate::literal(text));
                }
            }
        }

        let english_templates = templates.clone();
        let mut code = english.code().to_owned();
        let mut native_name = english.native_name().to_owned();
        let mut plural = english.plural();
        for overlay in overlays {
            // A partial override of the same language (docs/TRANSLATING.md) must not reset the shipped nativeName /
            // plural rule to defaults; a different language starts from its own values.
            let same_language = overlay.code() == code;
            code = overlay.code().to_owned();
            if overlay.native_name_declared() || !same_language {
                native_name = overlay.native_name().to_owned();
            }
            if overlay.plural_declared() || !same_language {
                plural = overlay.plural();
            }
            for (key, text) in overlay.entries() {
                let Some(source) = english_templates.get(key) else {
                    warnings.push(format!(
                        "{}: unknown key '{}' ignored",
                        overlay.code(),
                        LanguageCatalog::safe(Some(key))
                    ));
                    continue;
                };
                let Some(translated) = LocTemplate::try_parse(text) else {
                    warnings.push(format!(
                        "{}: '{}' has unbalanced braces or an invalid placeholder; English used",
                        overlay.code(),
                        LanguageCatalog::safe(Some(key))
                    ));
                    continue;
                };
                let allowed = source.placeholder_names();
                let used = translated.placeholder_names();
                if let Some(unknown) = used.iter().find(|n| !allowed.contains(n)) {
                    warnings.push(format!(
                        "{}: '{}' uses unknown placeholder {{{unknown}}}; English used",
                        overlay.code(),
                        LanguageCatalog::safe(Some(key))
                    ));
                    continue;
                }
                if let Some(missing) = allowed.iter().find(|n| !used.contains(n)) {
                    warnings.push(format!(
                        "{}: '{}' does not use placeholder {{{missing}}}",
                        overlay.code(),
                        LanguageCatalog::safe(Some(key))
                    ));
                }
                templates.insert(key.clone(), translated);
            }
        }
        Localizer::new(code, native_name, plural, templates, warnings)
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
        match self.templates.get(key) {
            Some(t) => t.render(&[], &number_culture()),
            None => key.to_owned(),
        }
    }

    /// `Format(key, args)` with the ambient [`number_culture`]. Unknown key -> the key.
    pub fn format(&self, key: &str, args: &[LocArg]) -> String {
        self.format_with(key, args, &number_culture())
    }

    /// `Format(key, args)` with an explicit culture.
    pub fn format_with(&self, key: &str, args: &[LocArg], culture: &NumberCulture) -> String {
        match self.templates.get(key) {
            Some(t) => t.render(args, culture),
            None => key.to_owned(),
        }
    }

    /// `FormatPlural(baseKey, count, args)` with the ambient [`number_culture`].
    pub fn format_plural(&self, base_key: &str, count: i64, args: &[LocArg]) -> String {
        self.format_plural_with(base_key, count, args, &number_culture())
    }

    /// `FormatPlural` with an explicit culture: picks `baseKey.one` or `baseKey.other` by [`plural`](Self::plural),
    /// then formats it. See the contract for the exact fall-backs.
    pub fn format_plural_with(&self, base_key: &str, count: i64, args: &[LocArg], culture: &NumberCulture) -> String {
        let use_one = self.plural == PluralRule::OneOther && count == 1;
        if let Some((one, other)) = self.plurals.get(base_key) {
            let chosen = match one {
                Some(one) if use_one => one,
                _ => other,
            };
            return chosen.render(args, culture);
        }
        let mut key = if use_one {
            format!("{base_key}{ONE_SUFFIX}")
        } else {
            format!("{base_key}{OTHER_SUFFIX}")
        };
        if !self.templates.contains_key(&key) {
            key = format!("{base_key}{OTHER_SUFFIX}");
        }
        self.format_with(&key, args, culture)
    }

    /// A copy with every template rewritten (translator modes). Warnings are carried over.
    pub fn transform(
        &self,
        code: &str,
        native_name: &str,
        plural: PluralRule,
        mut transform: impl FnMut(&str, &LocTemplate) -> LocTemplate,
    ) -> Localizer {
        let templates: HashMap<String, LocTemplate> = self
            .templates
            .iter()
            .map(|(key, template)| (key.clone(), transform(key, template)))
            .collect();
        Localizer::new(
            code.to_owned(),
            native_name.to_owned(),
            plural,
            templates,
            self.warnings.clone(),
        )
    }

    /// Ambient localizer. Defaults to [`built_in_english_localizer`] until [`set_current`](Self::set_current) runs.
    pub fn current() -> Arc<Localizer> {
        if let Some(current) = CURRENT.read().unwrap_or_else(PoisonError::into_inner).as_ref() {
            return Arc::clone(current);
        }
        // Compare-exchange semantics: whoever gets the write lock first installs English, the rest see it.
        let mut guard = CURRENT.write().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(guard.get_or_insert_with(built_in_english_localizer))
    }

    /// Replaces the ambient localizer atomically, then runs the [`on_current_changed`] handlers (outside every lock,
    /// after the new value is visible to `current()`).
    pub fn set_current(localizer: Arc<Localizer>) {
        *CURRENT.write().unwrap_or_else(PoisonError::into_inner) = Some(localizer);
        // Copy the list first so a handler may unsubscribe or call `set_current` itself.
        let handlers: Vec<Handler> = HANDLERS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(_, h)| Arc::clone(h))
            .collect();
        for handler in handlers {
            handler();
        }
    }

    /// Test seam for C# `LocalizerCurrentTests.Current_NotReplaced_IsBuiltInEnglish` (reflection on `s_current`):
    /// clears the ambient value so [`current`](Self::current) falls back to built-in English again.
    #[doc(hidden)]
    pub fn clear_current_for_tests() {
        *CURRENT.write().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// Process-wide ambient localizer storage.
static CURRENT: RwLock<Option<Arc<Localizer>>> = RwLock::new(None);

type Handler = Arc<dyn Fn() + Send + Sync>;

static HANDLERS: Mutex<Vec<(u64, Handler)>> = Mutex::new(Vec::new());
static NEXT_HANDLER_ID: AtomicU64 = AtomicU64::new(1);

/// Keeps an [`on_current_changed`] handler registered; dropping it unregisters the handler
/// (C# `CurrentChanged -= handler`).
#[derive(Debug)]
pub struct CurrentChangedSubscription {
    id: u64,
}

impl Drop for CurrentChangedSubscription {
    fn drop(&mut self) {
        HANDLERS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(id, _)| *id != self.id);
    }
}

/// C# `Localizer.CurrentChanged += handler`: called on whichever thread calls `set_current`, once per call.
pub fn on_current_changed(handler: impl Fn() + Send + Sync + 'static) -> CurrentChangedSubscription {
    let id = NEXT_HANDLER_ID.fetch_add(1, Ordering::Relaxed);
    HANDLERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((id, Arc::new(handler)));
    CurrentChangedSubscription { id }
}

static NUMBER_CULTURE: RwLock<Option<Arc<NumberCulture>>> = RwLock::new(None);

/// The number format of the user's regional settings (C# `CultureInfo.CurrentCulture`); default invariant. The app
/// sets it at startup and whenever the Windows regional settings change.
pub fn number_culture() -> Arc<NumberCulture> {
    match NUMBER_CULTURE.read().unwrap_or_else(PoisonError::into_inner).as_ref() {
        Some(culture) => Arc::clone(culture),
        None => Arc::new(NumberCulture::invariant()),
    }
}

/// Sets the ambient number culture.
pub fn set_number_culture(culture: NumberCulture) {
    *NUMBER_CULTURE.write().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(culture));
}

/// `TranslatorModes.CreatePseudo`: every text becomes `[Accented text ~~~]` (see `template::pseudo_template`), code
/// `qps-ploc`, native name `"Pseudo"`, plural rule `OneOther`.
pub fn create_pseudo(source: &Localizer) -> Localizer {
    source.transform(PSEUDO_CODE, "Pseudo", PluralRule::OneOther, |_, t| pseudo_template(t))
}

/// `TranslatorModes.CreateShowKeys`: every text becomes `[key]`; code, native name and plural rule are kept.
pub fn create_show_keys(source: &Localizer) -> Localizer {
    source.transform(source.code(), source.native_name(), source.plural(), |key, _| {
        show_key_template(key)
    })
}

static BUILT_IN_ENGLISH: OnceLock<LanguageCatalog> = OnceLock::new();
static BUILT_IN_ENGLISH_LOCALIZER: OnceLock<Arc<Localizer>> = OnceLock::new();

/// `BuiltInCatalog.English`: the English catalog embedded from `assets/languages/en.json`, always complete, the final
/// fallback for every key. Parsed once. Panics only if the embedded asset is invalid (a build-time bug).
pub fn built_in_english() -> &'static LanguageCatalog {
    BUILT_IN_ENGLISH.get_or_init(|| {
        let mut warnings = Vec::new();
        match LanguageCatalog::try_parse(EMBEDDED_ENGLISH_JSON, "en (built-in)", &mut warnings) {
            Some(catalog) => catalog,
            None => panic!("Built-in English catalog is invalid: {}", warnings.join("; ")),
        }
    })
}

/// `BuiltInCatalog.EnglishLocalizer`: `Localizer::create(built_in_english(), &[], &[])`, built once.
pub fn built_in_english_localizer() -> Arc<Localizer> {
    Arc::clone(BUILT_IN_ENGLISH_LOCALIZER.get_or_init(|| Arc::new(Localizer::create(built_in_english(), &[], &[]))))
}

/// The embedded English catalog text (C# embedded resource `PhotoReview.Core.Localization.en.json`).
pub const EMBEDDED_ENGLISH_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/languages/en.json"));
