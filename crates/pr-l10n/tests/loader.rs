//! Port of `LanguageLoaderTests` and the loader parts of `LocalizationHardeningTests` (C# baseline d92d6aae).
//! The loader always merges over the embedded English catalog, so these tests rely on its `app.title` key.
//! Each test names its C# twin in a comment. The "real file" variants are covered by the `l10n-loader` golden.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_core::abstractions::{FileStat, FileSystem, ReadSeek};
use pr_l10n::catalog::{MAX_FILE_BYTES, PluralRule};
use pr_l10n::loader::{AUTO_CODE, LanguageLoader, UiCulture, current_ui_culture, set_current_ui_culture};
use pr_l10n::localizer::built_in_english;
use pr_test_support::{FakeFileSystem, FsOp};

const KEY: &str = "app.title";
const SHIPPED_DIR: &str = r"C:\App\Languages";
const USER_DIR: &str = r"C:\Users\u\AppData\Local\PhotoReviewRs\Languages";

fn english_title() -> String {
    built_in_english().entry(KEY).expect("app.title").to_owned()
}

fn has_warning(warnings: &[String], fragment: &str) -> bool {
    warnings.iter().any(|w| w.contains(fragment))
}

struct Fixture {
    fs: Arc<FakeFileSystem>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            fs: Arc::new(FakeFileSystem::new()),
        }
    }

    fn loader_with(&self, user_dir: Option<&str>) -> LanguageLoader {
        let fs: Arc<dyn FileSystem> = self.fs.clone();
        LanguageLoader::new(fs, SHIPPED_DIR, user_dir.map(PathBuf::from))
    }

    fn loader(&self) -> LanguageLoader {
        self.loader_with(Some(USER_DIR))
    }

    fn add_catalog(
        &self,
        dir: &str,
        file_name: &str,
        code: &str,
        native_name: &str,
        title: Option<&str>,
        plural: &str,
    ) {
        let entry = title.map(|t| format!(", \"{KEY}\": \"{t}\"")).unwrap_or_default();
        let json = format!(
            "{{ \"_meta\": {{ \"code\": \"{code}\", \"name\": \"{code}\", \"nativeName\": \"{native_name}\", \"plural\": \"{plural}\" }}{entry} }}"
        );
        self.fs.add_file(Path::new(dir).join(file_name), json.as_bytes());
    }

    fn add_simple(&self, dir: &str, file_name: &str, code: &str, native_name: &str, title: Option<&str>) {
        self.add_catalog(dir, file_name, code, native_name, title, "one-other");
    }
}

fn codes(loader: &LanguageLoader) -> Vec<String> {
    loader.discover_languages().into_iter().map(|l| l.code).collect()
}

fn culture(name: &str) -> UiCulture {
    UiCulture::from_name(name)
}

#[test]
fn discover_languages_missing_folders_returns_english_only() {
    // C#: LanguageLoaderTests.DiscoverLanguages_MissingFolders_ReturnsEnglishOnly
    let languages = Fixture::new().loader().discover_languages();

    assert_eq!(languages.len(), 1);
    assert_eq!(languages[0].code, "en");
    assert!(!languages[0].is_user_provided);
}

#[test]
fn discover_languages_null_user_dir_lists_shipped_only() {
    // C#: LanguageLoaderTests.DiscoverLanguages_NullUserDir_ListsShippedOnly
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", None);
    f.add_simple(USER_DIR, "de.json", "de", "Deutsch", None);

    assert_eq!(codes(&f.loader_with(None)), ["en", "vi"]);
}

#[test]
fn discover_languages_shipped_and_user_english_first_each_code_once_sorted_by_native_name() {
    // C#: LanguageLoaderTests.DiscoverLanguages_ShippedAndUser_EnglishFirstEachCodeOnceSortedByNativeName
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "en.json", "en", "English (shipped)", None);
    f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", None);
    f.add_simple(USER_DIR, "vi.json", "vi", "Tiếng Việt (user)", None);
    f.add_simple(USER_DIR, "de.json", "de", "Deutsch", None);
    f.add_simple(USER_DIR, "en.json", "en", "English (user)", None);

    let languages = f.loader().discover_languages();

    let codes: Vec<&str> = languages.iter().map(|l| l.code.as_str()).collect();
    assert_eq!(codes, ["en", "de", "vi"]);
    assert_eq!(languages[0].native_name, built_in_english().native_name());
    assert!(!languages[0].is_user_provided);
    assert!(languages[1].is_user_provided);
    assert!(!languages[2].is_user_provided); // shipped entry wins the listing
    assert_eq!(languages[2].native_name, "Tiếng Việt");
}

#[test]
fn discover_languages_code_taken_from_meta_not_file_name() {
    // C#: LanguageLoaderTests.DiscoverLanguages_CodeTakenFromMetaNotFileName
    let f = Fixture::new();
    f.add_simple(USER_DIR, "my-translation.json", "fr", "Français", None);

    assert!(
        f.loader()
            .discover_languages()
            .iter()
            .any(|l| l.code == "fr" && l.is_user_provided)
    );
}

#[test]
fn discover_languages_dotted_file_names_skipped() {
    // C#: LanguageLoaderTests.DiscoverLanguages_DottedFileNames_Skipped
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "en.notes.json", "zz", "Notes", None);
    f.add_simple(USER_DIR, "vi.backup.json", "yy", "Backup", None);

    assert_eq!(codes(&f.loader()), ["en"]);
}

#[test]
fn discover_languages_non_json_files_ignored() {
    // C#: LanguageLoaderTests.DiscoverLanguages_NonJsonFiles_Ignored
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "vi.txt", "vi", "Tiếng Việt", None);

    assert_eq!(codes(&f.loader()), ["en"]);
}

#[test]
fn discover_languages_broken_file_skipped_without_throwing() {
    // C#: LanguageLoaderTests.DiscoverLanguages_BrokenFile_SkippedWithoutThrowing
    let f = Fixture::new();
    f.fs.add_file(Path::new(SHIPPED_DIR).join("aa.json"), b"{ not json");
    f.fs.add_file(Path::new(SHIPPED_DIR).join("bb.json"), b"[]");
    f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", None);

    assert_eq!(codes(&f.loader()), ["en", "vi"]);
}

#[test]
fn load_file_larger_than_max_skipped_with_warning() {
    // C#: LanguageLoaderTests.Load_FileLargerThanMax_SkippedWithWarning
    let f = Fixture::new();
    f.fs.add_file(
        Path::new(USER_DIR).join("vi.json"),
        &vec![b' '; MAX_FILE_BYTES as usize + 1],
    );
    let loader = f.loader();

    let localizer = loader.load(Some("vi"), None);

    assert!(!codes(&loader).contains(&"vi".to_owned()));
    assert_eq!(localizer.code(), "en");
    assert!(has_warning(localizer.warnings(), "larger than"));
}

#[test]
fn load_file_exactly_max_is_read() {
    // C#: LanguageLoaderTests.Load_FileExactlyMax_IsRead
    let f = Fixture::new();
    let json = format!(r#"{{ "_meta": {{ "code": "vi" }}, "{KEY}": "Tiêu đề" }}"#);
    let mut padded = vec![b' '; MAX_FILE_BYTES as usize];
    padded[..json.len()].copy_from_slice(json.as_bytes());
    f.fs.add_file(Path::new(USER_DIR).join("vi.json"), &padded);

    let localizer = f.loader().load(Some("vi"), None);

    assert_eq!(localizer.code(), "vi");
    assert!(localizer.warnings().is_empty(), "{:?}", localizer.warnings());
}

#[test]
fn load_broken_file_reported_in_warnings() {
    // C#: LanguageLoaderTests.Load_BrokenFile_ReportedInWarnings
    let f = Fixture::new();
    f.fs.add_file(Path::new(SHIPPED_DIR).join("vi.json"), b"{ \"_meta\": ");
    f.fs.add_file(Path::new(USER_DIR).join("fr.json"), br#"{ "_meta": { "code": "?" } }"#);

    let localizer = f.loader().load(Some("vi"), None);

    assert_eq!(localizer.code(), "en");
    assert_eq!(localizer.get(KEY), english_title());
    assert!(has_warning(localizer.warnings(), "vi.json: invalid JSON"));
    assert!(has_warning(
        localizer.warnings(),
        "fr.json: _meta.code is missing or invalid"
    ));
}

#[test]
fn load_unreadable_file_reported_in_warnings() {
    // C#: LanguageLoaderTests.Load_UnreadableFile_ReportedInWarnings
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", Some("Tiêu đề"));
    f.fs.fail(
        FsOp::ReadText,
        Path::new(SHIPPED_DIR).join("vi.json"),
        io::ErrorKind::PermissionDenied,
    );

    let localizer = f.loader().load(Some("vi"), None);

    assert_eq!(localizer.code(), "en");
    assert!(
        has_warning(localizer.warnings(), "vi.json: cannot read ("),
        "{:?}",
        localizer.warnings()
    );
}

#[test]
fn load_listing_error_is_one_warning_and_the_folder_is_skipped() {
    // C#: ReadFolder `cannot list files` branch (no C# test; the contract lists the exact text)
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", Some("Tiêu đề"));
    f.fs.fail(FsOp::EnumerateFiles, SHIPPED_DIR, io::ErrorKind::PermissionDenied);

    let localizer = f.loader().load(Some("vi"), None);

    assert_eq!(localizer.code(), "en");
    assert_eq!(localizer.warnings().len(), 1);
    assert!(localizer.warnings()[0].starts_with(&format!("{SHIPPED_DIR}: cannot list files (")));
}

#[test]
fn load_shipped_only_uses_shipped_translation() {
    // C#: LanguageLoaderTests.Load_ShippedOnly_UsesShippedTranslation
    let f = Fixture::new();
    f.add_catalog(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", Some("Tiêu đề"), "none");

    let localizer = f.loader().load(Some("vi"), None);

    assert_eq!(localizer.code(), "vi");
    assert_eq!(localizer.native_name(), "Tiếng Việt");
    assert_eq!(localizer.plural(), PluralRule::None);
    assert_eq!(localizer.get(KEY), "Tiêu đề");
    assert!(localizer.warnings().is_empty());
}

#[test]
fn load_shipped_and_user_user_wins_per_key() {
    // C#: LanguageLoaderTests.Load_ShippedAndUser_UserWinsPerKey
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", Some("Shipped title"));
    f.add_simple(USER_DIR, "vi.json", "vi", "Tiếng Việt", Some("User title"));

    assert_eq!(f.loader().load(Some("vi"), None).get(KEY), "User title");
}

#[test]
fn load_user_file_without_key_keeps_shipped_value() {
    // C#: LanguageLoaderTests.Load_UserFileWithoutKey_KeepsShippedValue
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", Some("Shipped title"));
    f.add_simple(USER_DIR, "vi.json", "vi", "Tiếng Việt", None);

    assert_eq!(f.loader().load(Some("vi"), None).get(KEY), "Shipped title");
}

#[test]
fn load_user_only_language_loads() {
    // C#: LanguageLoaderTests.Load_UserOnlyLanguage_Loads
    let f = Fixture::new();
    f.add_simple(USER_DIR, "de.json", "de", "Deutsch", Some("Fotoprüfung"));

    let localizer = f.loader().load(Some("DE"), None);

    assert_eq!(localizer.code(), "de");
    assert_eq!(localizer.get(KEY), "Fotoprüfung");
}

#[test]
fn load_unknown_code_falls_back_to_english() {
    // C#: LanguageLoaderTests.Load_UnknownCode_FallsBackToEnglish
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", Some("Tiêu đề"));

    let localizer = f.loader().load(Some("xx"), None);

    assert_eq!(localizer.code(), "en");
    assert_eq!(localizer.get(KEY), english_title());
}

#[test]
fn load_auto_with_vietnamese_culture_resolves_vietnamese() {
    // C#: LanguageLoaderTests.Load_AutoWithVietnameseCulture_ResolvesVietnamese
    for code in [Some("auto"), Some("AUTO"), None, Some("")] {
        let f = Fixture::new();
        f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", Some("Tiêu đề"));

        let localizer = f.loader().load(code, Some(&culture("vi-VN")));

        assert_eq!(localizer.code(), "vi", "{code:?}");
        assert_eq!(localizer.get(KEY), "Tiêu đề", "{code:?}");
    }
}

#[test]
fn load_auto_with_unavailable_culture_falls_back_to_english() {
    // C#: LanguageLoaderTests.Load_AutoWithUnavailableCulture_FallsBackToEnglish
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", Some("Tiêu đề"));

    let localizer = f.loader().load(Some(AUTO_CODE), Some(&culture("de-DE")));

    assert_eq!(localizer.code(), "en");
    assert_eq!(localizer.get(KEY), english_title());
}

#[test]
fn resolve_auto_with_regional_catalog_prefers_full_culture_name() {
    // C#: LanguageLoaderTests.Resolve_AutoWithRegionalCatalog_PrefersFullCultureName
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "pt.json", "pt", "Português", None);
    f.add_simple(SHIPPED_DIR, "pt-BR.json", "pt-BR", "Português (Brasil)", None);
    let loader = f.loader();

    assert_eq!(loader.resolve(Some(AUTO_CODE), Some(&culture("pt-BR"))), "pt-br");
    assert_eq!(loader.resolve(Some(AUTO_CODE), Some(&culture("pt-PT"))), "pt");
}

#[test]
fn resolve_explicit_code_ignores_culture() {
    // C#: LanguageLoaderTests.Resolve_ExplicitCode_IgnoresCulture
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", None);
    let loader = f.loader();

    assert_eq!(loader.resolve(Some("en"), Some(&culture("vi-VN"))), "en");
    assert_eq!(loader.resolve(Some("vi"), Some(&culture("en-US"))), "vi");
}

#[test]
fn load_missing_folders_returns_english_without_warnings() {
    // C#: LanguageLoaderTests.Load_MissingFolders_ReturnsEnglishWithoutWarnings
    let fs: Arc<dyn FileSystem> = Arc::new(FakeFileSystem::new());
    let loader = LanguageLoader::new(fs, r"C:\Nope\Languages", None);

    let localizer = loader.load(Some("vi"), None);

    assert_eq!(localizer.code(), "en");
    assert!(localizer.warnings().is_empty());
    assert_eq!(localizer.get(KEY), english_title());
}

#[test]
fn load_user_english_override_applies_only_when_english_selected() {
    // C#: LanguageLoaderTests.Load_UserEnglishOverride_AppliesOnlyWhenEnglishSelected
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", None);
    f.add_simple(USER_DIR, "en.json", "en", "English", Some("My Review"));
    let loader = f.loader();

    let english = loader.load(Some("en"), None);
    let vietnamese = loader.load(Some("vi"), None);

    assert_eq!(english.code(), "en");
    assert_eq!(english.get(KEY), "My Review");
    assert_eq!(vietnamese.code(), "vi");
    assert_eq!(vietnamese.get(KEY), english_title());
}

#[test]
fn load_shipped_english_file_not_used_as_overlay() {
    // C#: LanguageLoaderTests.Load_ShippedEnglishFile_NotUsedAsOverlay
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "en.json", "en", "English", Some("Shipped English"));

    assert_eq!(f.loader().load(Some("en"), None).get(KEY), english_title());
}

#[test]
fn load_translation_with_unknown_key_warns_and_still_loads() {
    // C#: LanguageLoaderTests.Load_TranslationWithUnknownKey_WarnsAndStillLoads
    let f = Fixture::new();
    f.fs.add_file(
        Path::new(USER_DIR).join("vi.json"),
        format!(r#"{{ "_meta": {{ "code": "vi" }}, "{KEY}": "Tiêu đề", "no.such.key": "x" }}"#).as_bytes(),
    );

    let localizer = f.loader().load(Some("vi"), None);

    assert_eq!(localizer.get(KEY), "Tiêu đề");
    assert!(has_warning(localizer.warnings(), "unknown key 'no.such.key'"));
}

#[test]
fn load_file_with_utf8_bom_is_read() {
    // Rust addition: the file system strips the BOM (C# StreamReader); the catalog parser itself rejects U+FEFF.
    let f = Fixture::new();
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(format!(r#"{{ "_meta": {{ "code": "vi" }}, "{KEY}": "Tiêu đề" }}"#).as_bytes());
    f.fs.add_file(Path::new(SHIPPED_DIR).join("vi.json"), &bytes);

    let localizer = f.loader().load(Some("vi"), None);

    assert_eq!(localizer.code(), "vi");
    assert!(localizer.warnings().is_empty());
}

#[test]
fn discover_languages_equal_native_names_keep_discovery_order() {
    // Rust addition (contract §17): a stable sort; equal names keep discovery order (shipped files by path first).
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "b.json", "bb", "Same", None);
    f.add_simple(SHIPPED_DIR, "a.json", "aa", "Same", None);
    f.add_simple(USER_DIR, "c.json", "cc", "Same", None);

    assert_eq!(codes(&f.loader()), ["en", "aa", "bb", "cc"]);
}

#[test]
fn native_name_comparer_can_be_replaced() {
    // Rust addition: `with_native_name_comparer` is the platform collation seam (contract open question 3).
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "a.json", "aa", "Alpha", None);
    f.add_simple(SHIPPED_DIR, "b.json", "bb", "Beta", None);
    let loader = f
        .loader()
        .with_native_name_comparer(Arc::new(|a: &str, b: &str| b.cmp(a)));

    assert_eq!(codes(&loader), ["en", "bb", "aa"]);
}

#[test]
fn ui_culture_from_name_takes_the_part_before_the_first_dash() {
    // Rust addition (contract §20).
    assert_eq!(UiCulture::from_name(""), UiCulture::invariant());
    assert_eq!(UiCulture::invariant().two_letter_iso_language_name, "iv");
    let c = UiCulture::from_name("zh-Hant-TW");
    assert_eq!(c.name, "zh-Hant-TW");
    assert_eq!(c.two_letter_iso_language_name, "zh");
    assert_eq!(UiCulture::from_name("PT-br").two_letter_iso_language_name, "pt");
}

#[test]
fn resolve_without_culture_uses_the_current_ui_culture() {
    // C#: `uiCulture ?? CultureInfo.CurrentUICulture`.
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "vi.json", "vi", "Tiếng Việt", None);
    let loader = f.loader();
    assert_eq!(current_ui_culture(), UiCulture::invariant());

    set_current_ui_culture(UiCulture::from_name("vi-VN"));
    let resolved = loader.resolve(None, None);
    set_current_ui_culture(UiCulture::invariant());

    assert_eq!(resolved, "vi");
}

/// File system whose stat is unknown (None), like a share that cannot report sizes.
struct NoStatFileSystem(FakeFileSystem);

impl FileSystem for NoStatFileSystem {
    fn file_exists(&self, path: &Path) -> bool {
        self.0.file_exists(path)
    }
    fn directory_exists(&self, path: &Path) -> bool {
        self.0.directory_exists(path)
    }
    fn get_file_stat(&self, _path: &Path) -> Option<FileStat> {
        None
    }
    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.0.move_file(source, destination)
    }
    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.0.copy_file(source, destination)
    }
    fn delete(&self, path: &Path) -> io::Result<()> {
        self.0.delete(path)
    }
    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.0.open_read_shared(path, buffer_size)
    }
    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn Write + Send>> {
        self.0.open_append(path, durable)
    }
    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.0.write_all_text_atomic(path, text, durable)
    }
    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.0.read_all_text(path)
    }
    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.0.enumerate_files(directory, pattern)
    }
    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.0.enumerate_directories(directory)
    }
    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.0.create_directory(path)
    }
}

#[test]
fn oversized_file_without_stat_skipped() {
    // C#: LocalizationHardeningTests.OversizedFile_WithoutStat_Skipped
    let fs = FakeFileSystem::new();
    let padding = "x".repeat(MAX_FILE_BYTES as usize);
    fs.add_file(
        r"C:\App\Languages\big.json",
        format!(r#"{{ "_meta": {{"code":"bg"}}, "pad": "{padding}" }}"#).as_bytes(),
    );
    fs.add_file(r"C:\App\Languages\ok.json", br#"{ "_meta": {"code":"ok"} }"#);
    let file_system: Arc<dyn FileSystem> = Arc::new(NoStatFileSystem(fs));
    let loader = LanguageLoader::new(file_system, r"C:\App\Languages", None);

    let codes: Vec<String> = loader.discover_languages().into_iter().map(|l| l.code).collect();
    let localizer = loader.load(Some("bg"), None);

    assert!(codes.contains(&"ok".to_owned()));
    assert!(!codes.contains(&"bg".to_owned()));
    assert!(has_warning(localizer.warnings(), "larger than"));
}

#[test]
fn oversized_multibyte_file_without_stat_skipped() {
    // C#: LocalizationHardeningTests.OversizedMultibyteFile_WithoutStat_Skipped
    let fs = FakeFileSystem::new();
    // 2 bytes each: char count < limit < byte count.
    let padding = "é".repeat(MAX_FILE_BYTES as usize / 2 + 1000);
    fs.add_file(
        r"C:\App\Languages\big.json",
        format!(r#"{{ "_meta": {{"code":"bg"}}, "pad": "{padding}" }}"#).as_bytes(),
    );
    let file_system: Arc<dyn FileSystem> = Arc::new(NoStatFileSystem(fs));
    let loader = LanguageLoader::new(file_system, r"C:\App\Languages", None);

    assert!(!loader.discover_languages().iter().any(|l| l.code == "bg"));
    assert!(has_warning(loader.load(Some("bg"), None).warnings(), "larger than"));
}

/// File system that lists files in REVERSE order (a file system is free to list in any order).
struct ReverseListFileSystem(FakeFileSystem);

impl FileSystem for ReverseListFileSystem {
    fn file_exists(&self, path: &Path) -> bool {
        self.0.file_exists(path)
    }
    fn directory_exists(&self, path: &Path) -> bool {
        self.0.directory_exists(path)
    }
    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.0.get_file_stat(path)
    }
    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.0.move_file(source, destination)
    }
    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.0.copy_file(source, destination)
    }
    fn delete(&self, path: &Path) -> io::Result<()> {
        self.0.delete(path)
    }
    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.0.open_read_shared(path, buffer_size)
    }
    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn Write + Send>> {
        self.0.open_append(path, durable)
    }
    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.0.write_all_text_atomic(path, text, durable)
    }
    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.0.read_all_text(path)
    }
    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        let mut files = self.0.enumerate_files(directory, pattern)?;
        files.reverse();
        Ok(files)
    }
    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.0.enumerate_directories(directory)
    }
    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.0.create_directory(path)
    }
}

fn same_code_files() -> FakeFileSystem {
    let fs = FakeFileSystem::new();
    fs.add_file(
        r"C:\App\Languages\a.json",
        br#"{ "_meta": {"code":"xx","nativeName":"First"}, "app.title": "from a" }"#,
    );
    fs.add_file(
        r"C:\App\Languages\b.json",
        br#"{ "_meta": {"code":"xx","nativeName":"Second"}, "app.title": "from b" }"#,
    );
    fs
}

#[test]
fn m10_loader_sorts_files_itself() {
    // Mutation M10: the loader orders files (ordinal ignore case on the full path) itself and does not rely on the
    // order the file system lists them in; two files with the same code resolve the same way either way.
    let sorted: Arc<dyn FileSystem> = Arc::new(same_code_files());
    let reversed: Arc<dyn FileSystem> = Arc::new(ReverseListFileSystem(same_code_files()));

    let a = LanguageLoader::new(sorted, r"C:\App\Languages", None).load(Some("xx"), None);
    let b = LanguageLoader::new(reversed, r"C:\App\Languages", None).load(Some("xx"), None);

    assert_eq!(a.get(KEY), "from b");
    assert_eq!(b.get(KEY), a.get(KEY));
    assert_eq!(b.native_name(), a.native_name());
}

#[test]
fn discover_languages_default_native_name_order_folds_accents() {
    // Differential finding vs C# CurrentCultureIgnoreCase: an accented initial sorts with its base letter
    // (en, cs, de, es, zu); the ordinal comparer gave en, de, es, zu, cs.
    let f = Fixture::new();
    f.add_simple(SHIPPED_DIR, "de.json", "de", "Deutsch", None);
    f.add_simple(SHIPPED_DIR, "cs.json", "cs", "\u{10C}e\u{161}tina", None);
    f.add_simple(SHIPPED_DIR, "es.json", "es", "Espa\u{F1}ol", None);
    f.add_simple(SHIPPED_DIR, "zu.json", "zu", "Zulu", None);

    assert_eq!(codes(&f.loader()), ["en", "cs", "de", "es", "zu"]);
}
