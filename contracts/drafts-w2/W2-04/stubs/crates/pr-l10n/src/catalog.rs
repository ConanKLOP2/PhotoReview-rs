//! Port of `PhotoReview.Core/Localization/LanguageCatalog.cs` (C# baseline d92d6aae, 213 lines): one translation file,
//! parsed but not yet validated against English. Translation files are untrusted: [`LanguageCatalog::try_parse`]
//! never panics and reports every problem as a warning string.
//!
//! The JSON layer is hand-written here (NOT `serde_json`): `System.Text.Json` with `CommentHandling.Skip` and
//! `AllowTrailingCommas`, maximum depth 64, duplicate names kept in order, strings materialised lazily (a lone
//! surrogate escape is only an error when the text is actually read). Contract: `contracts/W2-04.md`.
//!
//! Unit W2-04. Stub: bodies are `todo!()`; data definitions are complete.

// Stub-only (docs/MULTI-AGENT-PLAN D11): removed by the porter in the commit that removes the last `todo!()`.
#![allow(unused_variables, dead_code, clippy::ptr_arg)] // ptr_arg: `warnings` is pushed to once implemented

use std::collections::HashMap;

/// How a language picks between `key.one` and `key.other` (C# `PluralRule`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PluralRule {
    /// `.one` when the count is exactly 1, otherwise `.other` (English and most European languages).
    #[default]
    OneOther,
    /// Always `.other` (Vietnamese, Japanese, Chinese, ...).
    None,
}

impl PluralRule {
    /// The C# enum member name (`"OneOther"` / `"None"`), used by golden files.
    pub fn name(self) -> &'static str {
        match self {
            PluralRule::OneOther => "OneOther",
            PluralRule::None => "None",
        }
    }
}

/// Largest translation file that is read (a full catalog is ~30 KB). C# `LanguageCatalog.MaxFileBytes`.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The reserved top-level key holding the metadata object. C# `LanguageCatalog.MetaKey`.
pub const META_KEY: &str = "_meta";

/// Longest language display name kept (picker rows are one line). C# `MaxDisplayNameLength`.
pub const MAX_DISPLAY_NAME_LENGTH: usize = 64;

/// Longest untrusted fragment echoed into a warning. C# `MaxWarningFragmentLength`.
pub const MAX_WARNING_FRAGMENT_LENGTH: usize = 120;

/// One parsed translation file.
#[derive(Debug, Clone)]
pub struct LanguageCatalog {
    code: String,
    name: String,
    native_name: String,
    plural: PluralRule,
    native_name_declared: bool,
    plural_declared: bool,
    authors: Vec<String>,
    /// Insertion order of first occurrence; a duplicate key replaces the value in place.
    entries: Vec<(String, String)>,
    index: HashMap<String, usize>,
}

impl LanguageCatalog {
    /// Language code from `_meta.code`, lower-case (`vi`, `pt-br`).
    pub fn code(&self) -> &str {
        &self.code
    }

    /// English name of the language (falls back to the code).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Name in the language itself; shown in the language picker (falls back to `name`, then the code).
    pub fn native_name(&self) -> &str {
        &self.native_name
    }

    pub fn plural(&self) -> PluralRule {
        self.plural
    }

    /// True when `_meta` spelled out a (cleaned, non-blank) `name` or `nativeName`.
    pub fn native_name_declared(&self) -> bool {
        self.native_name_declared
    }

    /// True when `_meta.plural` was a string (valid or not).
    pub fn plural_declared(&self) -> bool {
        self.plural_declared
    }

    pub fn authors(&self) -> &[String] {
        &self.authors
    }

    /// Raw key/text pairs in first-occurrence order; keys starting with `_` are already removed.
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    /// Text of `key` (exact, case-sensitive), `None` when absent.
    pub fn entry(&self, key: &str) -> Option<&str> {
        todo!("lookup through the index")
    }

    /// C# `LanguageCatalog.TryParse(json, source, out catalog, warnings)`. `None` is the `false` result; every
    /// problem (also on success) is appended to `warnings` in the order C# adds it. Never panics.
    pub fn try_parse(json: &str, source: &str, warnings: &mut Vec<String>) -> Option<LanguageCatalog> {
        todo!("see contracts/W2-04.md section 'LanguageCatalog::try_parse'")
    }

    /// BCP-47-ish: ASCII letters, digits and `-`, 2..=16 chars (UTF-16 length = char count here), first char an ASCII
    /// letter. C# `IsValidCode`.
    pub fn is_valid_code(code: &str) -> bool {
        todo!()
    }

    /// Makes untrusted text safe to embed in a one-line warning: control characters (`char::is_control`) become
    /// U+FFFD and text longer than 120 UTF-16 units is cut and gets `"..."` appended. `None` is `""`.
    /// C# `LanguageCatalog.Safe` (internal; `pub` here because `Localizer` and `LanguageLoader` use it).
    pub fn safe(value: Option<&str>) -> String {
        todo!()
    }
}
