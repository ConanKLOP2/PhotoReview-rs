//! Port of `PhotoReview.Core/Localization/LanguageLoader.cs` (C# baseline d92d6aae, 144 lines): finds and loads
//! translation files from the shipped folder (`<app>\Languages`) and the user folder
//! (`%LOCALAPPDATA%\PhotoReviewRs\Languages`, docs/adr/R0002; the C# app uses `...\PhotoReview\Languages`).
//! Files are untrusted: a broken or oversized file is skipped with a warning, never a panic.
//!
//! Unit W2-04. Contract: `contracts/W2-04.md`.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use pr_core::abstractions::FileSystem;
use pr_core::text::dotnet_path::get_file_name_without_extension;
use pr_core::text::{cmp_ignore_case, eq_ignore_case, ignore_case_key};

use crate::catalog::{LanguageCatalog, MAX_FILE_BYTES};
use crate::localizer::{ENGLISH_CODE, Localizer, built_in_english};

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
        if name.is_empty() {
            return Self::invariant();
        }
        let language = name.split('-').next().unwrap_or(name);
        UiCulture {
            name: name.to_owned(),
            two_letter_iso_language_name: language.to_lowercase(),
        }
    }
}

/// The UI language of the process (C# `CultureInfo.CurrentUICulture`), used when `load`/`resolve` get no culture.
/// Defaults to [`UiCulture::invariant`]; the app sets it at startup from the Windows UI language.
static CURRENT_UI_CULTURE: RwLock<Option<UiCulture>> = RwLock::new(None);

/// C# `CultureInfo.CurrentUICulture`.
pub fn current_ui_culture() -> UiCulture {
    CURRENT_UI_CULTURE
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .unwrap_or_else(UiCulture::invariant)
}

/// Sets the ambient UI culture (the C# `CurrentUICulture` setter).
pub fn set_current_ui_culture(culture: UiCulture) {
    *CURRENT_UI_CULTURE.write().unwrap_or_else(PoisonError::into_inner) = Some(culture);
}

/// Orders two native names for the language picker. C# uses `StringComparer.CurrentCultureIgnoreCase`; Rust has no
/// culture-aware collation, so the default is `pr_core::text::cmp_ignore_case` and the platform layer may inject a
/// `CompareStringEx`-based comparer later (docs: contracts/W2-04.md open question 3).
pub type NativeNameComparer = Arc<dyn Fn(&str, &str) -> Ordering + Send + Sync>;

const LATIN1_BASE: &str = "AAAAAAACEEEEIIIIDNOOOOO*OUUUUYTsaaaaaaaceeeeiiiidnooooo/ouuuuyty";
const LATIN_EXT_A_BASE: &str = "AaAaAaCcCcCcCcDdDdEeEeEeEeEeGgGgGgGgHhHhIiIiIiIiIiIiJjKkkLlLlLlLlLlNnNnNnnNnOoOoOoOoRrRrRrSsSsSsSsTtTtTtUuUuUuUuUuUuWwYyYZzZzZzs";

/// Base letter of a precomposed Latin letter (Latin-1, Latin Extended-A, Vietnamese), the char itself otherwise.
fn fold_accent(c: char) -> char {
    let code = c as u32;
    let base = |table: &str, offset: u32| table.chars().nth(offset as usize).filter(|b| *b != '*' && *b != '/');
    match code {
        0xC0..=0xFF => base(LATIN1_BASE, code - 0xC0),
        0x100..=0x17F => base(LATIN_EXT_A_BASE, code - 0x100),
        0x1A0 => Some('O'),
        0x1A1 => Some('o'),
        0x1AF => Some('U'),
        0x1B0 => Some('u'),
        0x1EA0..=0x1EF9 => {
            let group = match code {
                0x1EA0..=0x1EB7 => 'A',
                0x1EB8..=0x1EC7 => 'E',
                0x1EC8..=0x1ECB => 'I',
                0x1ECC..=0x1EE3 => 'O',
                0x1EE4..=0x1EF1 => 'U',
                _ => 'Y',
            };
            Some(if code % 2 == 0 {
                group
            } else {
                group.to_ascii_lowercase()
            })
        }
        _ => None,
    }
    .unwrap_or(c)
}

/// Default native-name order: compares by base letter ignoring case (so an accented initial sorts with its plain
/// letter, like a culture-aware collation), ties are broken by the accented spelling.
/// Approximates C# `StringComparer.CurrentCultureIgnoreCase`; inject a platform comparer for exact parity.
fn default_native_name_cmp(a: &str, b: &str) -> Ordering {
    let primary = |s: &str| -> String {
        s.chars()
            .filter(|c| !('\u{300}'..='\u{36F}').contains(c))
            .map(fold_accent)
            .collect()
    };
    cmp_ignore_case(&primary(a), &primary(b)).then_with(|| cmp_ignore_case(a, b))
}

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

/// A catalog read from disk and whether it came from the user folder.
struct ReadCatalog {
    catalog: LanguageCatalog,
    is_user: bool,
}

impl LanguageLoader {
    /// `user_dir == None` means "no user folder" (C# `null`).
    pub fn new(file_system: Arc<dyn FileSystem>, shipped_dir: impl Into<PathBuf>, user_dir: Option<PathBuf>) -> Self {
        LanguageLoader {
            file_system,
            shipped_dir: shipped_dir.into(),
            user_dir,
            native_name_comparer: Arc::new(default_native_name_cmp),
        }
    }

    /// Replaces the native-name collation (default: an accent-folding approximation of the C# culture order).
    pub fn with_native_name_comparer(mut self, comparer: NativeNameComparer) -> Self {
        self.native_name_comparer = comparer;
        self
    }

    /// English first, then every other valid language by native name (a code found in both folders is listed once,
    /// the shipped entry wins). Broken files are skipped silently here; [`load`](Self::load) reports them.
    pub fn discover_languages(&self) -> Vec<LanguageInfo> {
        let mut scratch = Vec::new();
        self.discover(&self.read_all(&mut scratch))
    }

    fn discover(&self, catalogs: &[ReadCatalog]) -> Vec<LanguageInfo> {
        let english = built_in_english();
        let mut found: Vec<LanguageInfo> = vec![LanguageInfo {
            code: ENGLISH_CODE.to_owned(),
            name: english.name().to_owned(),
            native_name: english.native_name().to_owned(),
            is_user_provided: false,
        }];
        let mut seen = HashSet::new();
        seen.insert(ignore_case_key(ENGLISH_CODE));
        for read in catalogs {
            let catalog = &read.catalog;
            if seen.insert(ignore_case_key(catalog.code())) {
                found.push(LanguageInfo {
                    code: catalog.code().to_owned(),
                    name: catalog.name().to_owned(),
                    native_name: catalog.native_name().to_owned(),
                    is_user_provided: read.is_user,
                });
            }
        }
        // English first, then a stable sort by native name (equal names keep discovery order).
        let mut rest = found.split_off(1);
        rest.sort_by(|a, b| (self.native_name_comparer)(&a.native_name, &b.native_name));
        found.extend(rest);
        found
    }

    /// Builds the localizer for `code` (`None`, blank and `"auto"` resolve through `ui_culture`, `None` culture =
    /// [`current_ui_culture`]). Unknown languages fall back to English. Never fails: problems are warnings in
    /// `Localizer::warnings`.
    pub fn load(&self, code: Option<&str>, ui_culture: Option<&UiCulture>) -> Localizer {
        let mut warnings = Vec::new();
        let all = self.read_all(&mut warnings); // one pass over the folders
        let resolved = Self::resolve_in(&self.discover(&all), code, ui_culture);
        let mut overlays: Vec<LanguageCatalog> = Vec::new();
        if !eq_ignore_case(&resolved, ENGLISH_CODE) {
            // Shipped first, user last: user entries override shipped ones key by key.
            for is_user in [false, true] {
                overlays.extend(
                    all.iter()
                        .filter(|x| x.is_user == is_user && eq_ignore_case(x.catalog.code(), &resolved))
                        .map(|x| x.catalog.clone()),
                );
            }
        } else {
            // A user file may also override English wording.
            overlays.extend(
                all.iter()
                    .filter(|x| x.is_user && eq_ignore_case(x.catalog.code(), ENGLISH_CODE))
                    .map(|x| x.catalog.clone()),
            );
        }
        Localizer::create(built_in_english(), &overlays, &warnings)
    }

    /// Maps a setting value to a language code that has a catalog; `"en"` when nothing matches.
    pub fn resolve(&self, code: Option<&str>, ui_culture: Option<&UiCulture>) -> String {
        Self::resolve_in(&self.discover_languages(), code, ui_culture)
    }

    fn resolve_in(languages: &[LanguageInfo], code: Option<&str>, ui_culture: Option<&UiCulture>) -> String {
        let find = |wanted: &str| {
            languages
                .iter()
                .map(|l| l.code.as_str())
                .find(|c| eq_ignore_case(c, wanted))
        };
        if let Some(code) = code
            && !code.trim().is_empty()
            && !eq_ignore_case(code, AUTO_CODE)
        {
            return find(code).unwrap_or(ENGLISH_CODE).to_owned();
        }
        let culture = ui_culture.cloned().unwrap_or_else(current_ui_culture);
        find(&culture.name)
            .or_else(|| find(&culture.two_letter_iso_language_name))
            .unwrap_or(ENGLISH_CODE)
            .to_owned()
    }

    /// C# `ReadAll`: the shipped folder, then the user folder; warnings of every folder and file are appended in order.
    fn read_all(&self, warnings: &mut Vec<String>) -> Vec<ReadCatalog> {
        let mut all = Vec::new();
        for catalog in self.read_folder(&self.shipped_dir, warnings) {
            all.push(ReadCatalog {
                catalog,
                is_user: false,
            });
        }
        if let Some(user_dir) = &self.user_dir {
            for catalog in self.read_folder(user_dir, warnings) {
                all.push(ReadCatalog { catalog, is_user: true });
            }
        }
        all
    }

    fn read_folder(&self, dir: &Path, warnings: &mut Vec<String>) -> Vec<LanguageCatalog> {
        let mut catalogs = Vec::new();
        if !self.file_system.directory_exists(dir) {
            return catalogs;
        }
        let files = match self.file_system.enumerate_files(dir, "*.json") {
            Ok(files) => files,
            Err(err) => {
                warnings.push(format!("{}: cannot list files ({err})", dir.display()));
                return catalogs;
            }
        };
        let mut files: Vec<(String, PathBuf)> = files
            .into_iter()
            .map(|f| (f.to_string_lossy().into_owned(), f))
            .collect();
        files.sort_by(|a, b| cmp_ignore_case(&a.0, &b.0));

        for (name, file) in files {
            // Translator notes (en.notes.json) and other dotted names are not catalogs.
            if get_file_name_without_extension(&name).contains('.') {
                continue;
            }
            if let Some(stat) = self.file_system.get_file_stat(&file)
                && stat.length > MAX_FILE_BYTES
            {
                warnings.push(format!("{name}: larger than {MAX_FILE_BYTES} bytes, skipped"));
                continue;
            }
            let json = match self.file_system.read_all_text(&file) {
                Ok(json) => json,
                Err(err) => {
                    warnings.push(format!("{name}: cannot read ({err})"));
                    continue;
                }
            };
            // The stat above can be missing or stale (the file grew after it): its UTF-8 byte count is the last line
            // of defence.
            if json.len() as u64 > MAX_FILE_BYTES {
                warnings.push(format!("{name}: larger than {MAX_FILE_BYTES} bytes, skipped"));
                continue;
            }
            if let Some(catalog) = LanguageCatalog::try_parse(&json, &name, warnings) {
                catalogs.push(catalog);
            }
        }
        catalogs
    }
}
