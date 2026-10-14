//! Port of `PhotoReview.Core/Localization/LanguageLoader.cs` (C# baseline d92d6aae, 144 lines): finds and loads
//! translation files from the shipped folder (`<app>\Languages`) and the user folder
//! (`%LOCALAPPDATA%\PhotoReviewRs\Languages`, docs/adr/R0002; the C# app uses `...\PhotoReview\Languages`).
//! Files are untrusted: a broken or oversized file is skipped with a warning, never a panic.
//!
//! Unit W2-04. Stub: bodies are `todo!()`; data definitions are complete. Contract: `contracts/W2-04.md`.

// Stub-only (docs/MULTI-AGENT-PLAN D11): removed by the porter in the commit that removes the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::cmp::Ordering;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use pr_core::abstractions::FileSystem;

use crate::localizer::Localizer;

/// Setting value that follows the Windows UI language. C# `LanguageLoader.AutoCode`.
pub const AUTO_CODE: &str = "auto";

/// A language the user can pick: found in the shipped folder, the user folder, or built in.
/// C# `record LanguageInfo(Code, Name, NativeName, IsUserProvided)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageInfo {
    pub code: String,
    pub name: String,
    pub native_name: String,
    pub is_user_provided: bool,
}

/// The two facts of a `CultureInfo` that `LanguageLoader.Resolve` reads: `Name` (`"vi-VN"`, `"pt-BR"`, `""` for the
/// invariant culture) and `TwoLetterISOLanguageName` (`"vi"`, `"pt"`, `"fil"`, `"iv"` for the invariant culture).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiCulture {
    /// `CultureInfo.Name`.
    pub name: String,
    /// `CultureInfo.TwoLetterISOLanguageName`.
    pub two_letter_iso_language_name: String,
}

impl UiCulture {
    /// `CultureInfo.InvariantCulture`: `Name == ""`, two-letter name `"iv"`.
    pub fn invariant() -> Self {
        UiCulture {
            name: String::new(),
            two_letter_iso_language_name: "iv".to_owned(),
        }
    }

    /// Builds the culture from its name: `two_letter_iso_language_name` is the text before the first `-`, lower-cased
    /// (the empty name is the invariant culture). Exact for every culture name the golden `l10n-loader` probes; the
    /// platform layer may construct the struct directly from `GetLocaleInfoEx` instead.
    pub fn from_name(name: &str) -> Self {
        todo!("see contracts/W2-04.md section 'UiCulture'")
    }
}

/// The UI language of the process (C# `CultureInfo.CurrentUICulture`), used when `load`/`resolve` get no culture.
/// Defaults to [`UiCulture::invariant`]; the app sets it at startup from the Windows UI language.
static CURRENT_UI_CULTURE: RwLock<Option<UiCulture>> = RwLock::new(None);

/// C# `CultureInfo.CurrentUICulture`.
pub fn current_ui_culture() -> UiCulture {
    todo!("read CURRENT_UI_CULTURE, recover a poisoned lock with PoisonError::into_inner")
}

/// Sets the ambient UI culture (the C# `CurrentUICulture` setter).
pub fn set_current_ui_culture(culture: UiCulture) {
    todo!()
}

/// Orders two native names for the language picker. C# uses `StringComparer.CurrentCultureIgnoreCase`; Rust has no
/// culture-aware collation, so the default is `pr_core::text::cmp_ignore_case` and the platform layer may inject a
/// `CompareStringEx`-based comparer later (docs: contracts/W2-04.md open question 3).
pub type NativeNameComparer = Arc<dyn Fn(&str, &str) -> Ordering + Send + Sync>;

/// Finds and loads translation files. C# `LanguageLoader`.
pub struct LanguageLoader {
    file_system: Arc<dyn FileSystem>,
    shipped_dir: PathBuf,
    user_dir: Option<PathBuf>,
    native_name_comparer: NativeNameComparer,
}

impl std::fmt::Debug for LanguageLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanguageLoader")
            .field("shipped_dir", &self.shipped_dir)
            .field("user_dir", &self.user_dir)
            .finish_non_exhaustive()
    }
}

impl LanguageLoader {
    /// `user_dir == None` means "no user folder" (C# `null`).
    pub fn new(file_system: Arc<dyn FileSystem>, shipped_dir: impl Into<PathBuf>, user_dir: Option<PathBuf>) -> Self {
        todo!("store the arguments; the default comparer is pr_core::text::cmp_ignore_case")
    }

    /// Replaces the native-name collation (default: ordinal ignore case).
    pub fn with_native_name_comparer(self, comparer: NativeNameComparer) -> Self {
        todo!()
    }

    /// English first, then every other valid language by native name (a code found in both folders is listed once,
    /// the shipped entry wins). Broken files are skipped silently here; [`load`](Self::load) reports them.
    pub fn discover_languages(&self) -> Vec<LanguageInfo> {
        todo!("ReadAll with a throw-away warnings list, then Discover")
    }

    /// Builds the localizer for `code` (`None`, blank and `"auto"` resolve through `ui_culture`, `None` culture =
    /// [`current_ui_culture`]). Unknown languages fall back to English. Never fails: problems are warnings in
    /// `Localizer::warnings`.
    pub fn load(&self, code: Option<&str>, ui_culture: Option<&UiCulture>) -> Localizer {
        todo!("see contracts/W2-04.md section 'LanguageLoader::load'")
    }

    /// Maps a setting value to a language code that has a catalog; `"en"` when nothing matches.
    pub fn resolve(&self, code: Option<&str>, ui_culture: Option<&UiCulture>) -> String {
        todo!()
    }
}
