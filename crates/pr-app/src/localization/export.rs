//! Port of `PhotoReview.App/Localization/TranslationExport.cs` (C# baseline d92d6aae, 188 lines): "Export strings to
//! translate" (I18N L08). Writes `<code>.todo.json` for a translator: `_meta` of the language, `_help`, `_missing`
//! (English keys still untranslated), `_notes` (translator context from `en.notes.json`), then every English key once
//! (untranslated first, English text; then translated, current text).
//!
//! Unit W8-04b. Contract: `contracts/W8-04b.md` (after merge: `contracts/W8-04b.md`).
//! Files are written under `%LOCALAPPDATA%\PhotoReviewRs\Languages` (R0002), never into the C# app's folders.
//! No `unsafe`, no `windows` crate (pure module of `pr_app`).

use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt::{self, Write as _};
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use pr_core::text::{cmp_ignore_case, eq_ignore_case};
use pr_l10n::catalog::{LanguageCatalog, MAX_FILE_BYTES, META_KEY, PluralRule};
use pr_l10n::keys;
use pr_l10n::localizer::{ENGLISH_CODE, Localizer, built_in_english};

/// `TranslationExport.FileSuffix`. The dotted name keeps the loader from reading the half-done file as a catalog.
pub const FILE_SUFFIX: &str = ".todo.json";

/// `TranslationExport.BackupSuffix`. Appended to the previous export when it is replaced.
pub const BACKUP_SUFFIX: &str = ".bak";

/// `TranslationExport.NotesFileName`.
pub const NOTES_FILE_NAME: &str = "en.notes.json";

/// `TranslationExport.MissingKey`: the array of keys whose value is still English.
pub const MISSING_KEY: &str = "_missing";

/// Errors of [`export`]. C# throws `ArgumentException` for the first two (golden: `"threw": "ArgumentException"`) and
/// lets `IOException` / `UnauthorizedAccessException` escape (`Io`). Messages are not compared with C#.
#[derive(Debug)]
pub enum TranslationExportError {
    /// `code` or `user_dir` is empty or whitespace only (`ArgumentException.ThrowIfNullOrWhiteSpace`). The payload is
    /// the parameter name: `"code"` or `"user_dir"`.
    BlankArgument(&'static str),
    /// `LanguageCatalog.IsValidCode(code)` is false (C# "Invalid language code '{code}'."). Nothing is touched on disk.
    InvalidCode(String),
    /// Creating the folder, reading an existing export, copying the backup or writing the file failed.
    Io(io::Error),
}

impl fmt::Display for TranslationExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TranslationExportError::BlankArgument(name) => write!(f, "argument '{name}' must not be blank"),
            TranslationExportError::InvalidCode(code) => write!(f, "Invalid language code '{code}'."),
            TranslationExportError::Io(error) => write!(f, "translation export failed: {error}"),
        }
    }
}

impl Error for TranslationExportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            TranslationExportError::Io(error) => Some(error),
            TranslationExportError::BlankArgument(_) | TranslationExportError::InvalidCode(_) => None,
        }
    }
}

impl From<io::Error> for TranslationExportError {
    fn from(error: io::Error) -> Self {
        TranslationExportError::Io(error)
    }
}

/// `TranslationExport.HelpText` = `Tr.ExportHelp` in the current UI language: the generated accessor
/// `pr_l10n::keys::EXPORT_HELP` (key `export.help`) read through `Localizer::current()`.
pub fn help_text() -> String {
    Localizer::current().get(keys::EXPORT_HELP)
}

/// `TranslationExport.Export(code, shippedDir, userDir)`. Checks, in this order: `code` not blank, `user_dir` not blank,
/// `code` valid (`LanguageCatalog::is_valid_code`). Then reads the catalogs of `code` from `shipped_dir` and then from
/// `user_dir` (user last), reads `en.notes.json` from `shipped_dir`, builds the text with [`build_json`], creates
/// `user_dir`, and writes `<code lower>.todo.json` into it as UTF-8 without BOM. If the file exists and its text differs,
/// the old text is first copied to a free backup name (`.bak`, `.bak2`, `.bak3`, ...). Returns the written path.
pub fn export(code: &str, shipped_dir: &Path, user_dir: &Path) -> Result<PathBuf, TranslationExportError> {
    if code.trim().is_empty() {
        return Err(TranslationExportError::BlankArgument("code"));
    }
    if user_dir.to_string_lossy().trim().is_empty() {
        return Err(TranslationExportError::BlankArgument("user_dir"));
    }
    if !LanguageCatalog::is_valid_code(code) {
        return Err(TranslationExportError::InvalidCode(code.to_owned()));
    }

    let mut catalogs = read_catalogs(shipped_dir, code)?;
    catalogs.extend(read_catalogs(user_dir, code)?);
    let notes = read_notes(&shipped_dir.join(NOTES_FILE_NAME));
    let text = build_json(built_in_english(), code, &catalogs, &notes);

    fs::create_dir_all(user_dir)?;
    let path = user_dir.join(format!("{}{FILE_SUFFIX}", code.to_lowercase()));

    // The export is what a translator edits by hand: a second Export must not silently destroy that work. An export
    // that would replace different content keeps its own backup; an identical file needs none.
    if path.is_file() {
        let old_bytes = fs::read(&path)?;
        if decode_text(&old_bytes) != text {
            write_new_file(&free_backup_path(&path), &old_bytes)?;
        }
    }
    fs::write(&path, text.as_bytes())?;
    Ok(path)
}

/// `TranslationExport.BuildJson(english, code, translations, notes)` with [`help_text`] as `_help`.
pub fn build_json(
    english: &LanguageCatalog,
    code: &str,
    translations: &[LanguageCatalog],
    notes: &BTreeMap<String, String>,
) -> String {
    build_json_with_help(english, code, translations, notes, &help_text())
}

/// [`build_json`] with the `_help` text given by the caller (the golden and the tests pass the English help text, so
/// they do not depend on the ambient `Localizer::current()`). The text ends with exactly one `\n`.
pub fn build_json_with_help(
    english: &LanguageCatalog,
    code: &str,
    translations: &[LanguageCatalog],
    notes: &BTreeMap<String, String>,
    help: &str,
) -> String {
    let is_english = eq_ignore_case(code, ENGLISH_CODE);
    // The last translation supplies `_meta`; an English export without translations uses English itself.
    let meta: Option<&LanguageCatalog> = match translations.last() {
        Some(last) => Some(last),
        None if is_english => Some(english),
        None => None,
    };
    let plural = meta.map_or(PluralRule::OneOther, LanguageCatalog::plural);

    // Current text per English key. English is complete by definition; for English the translations override its
    // texts. Keys English does not know are dropped. The last source wins.
    let mut current: HashMap<&str, &str> = HashMap::new();
    let sources: Vec<&LanguageCatalog> = if is_english {
        std::iter::once(english).chain(translations.iter()).collect()
    } else {
        translations.iter().collect()
    };
    for catalog in sources {
        for (key, text) in catalog.entries() {
            if english.entry(key).is_some() {
                current.insert(key.as_str(), text.as_str());
            }
        }
    }

    // A language without plural forms never shows key.one, so an untranslated one is not work to do.
    let mut missing: Vec<(String, String)> = english
        .entries()
        .iter()
        .filter(|(key, _)| !current.contains_key(key.as_str()))
        .filter(|(key, _)| !(plural == PluralRule::None && is_unused_one_form(key, english)))
        .cloned()
        .collect();
    missing.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));

    let mut done: Vec<(String, String)> = current
        .iter()
        .map(|(key, text)| ((*key).to_owned(), (*text).to_owned()))
        .collect();
    done.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));

    // Untranslated keys first (English text), then every translated key with its current text.
    let entries: Vec<(String, String)> = missing.iter().cloned().chain(done).collect();

    let meta_members = vec![
        (
            "code".to_owned(),
            Json::Str(meta.map_or_else(|| code.to_lowercase(), |m| m.code().to_owned())),
        ),
        (
            "name".to_owned(),
            Json::Str(meta.map_or_else(|| code.to_owned(), |m| m.name().to_owned())),
        ),
        (
            "nativeName".to_owned(),
            Json::Str(meta.map_or_else(|| code.to_owned(), |m| m.native_name().to_owned())),
        ),
        (
            "plural".to_owned(),
            Json::Str(
                if plural == PluralRule::None {
                    "none"
                } else {
                    "one-other"
                }
                .to_owned(),
            ),
        ),
        (
            "authors".to_owned(),
            Json::Strs(meta.map_or_else(Vec::new, |m| m.authors().to_vec())),
        ),
    ];
    let missing_keys: Vec<String> = missing.iter().map(|(key, _)| key.clone()).collect();
    let note_members: Vec<(String, Json)> = entries
        .iter()
        .filter_map(|(key, _)| notes.get(key).map(|note| (key.clone(), Json::Str(note.clone()))))
        .collect();

    let mut members: Vec<(String, Json)> = vec![
        (META_KEY.to_owned(), Json::Object(meta_members)),
        ("_help".to_owned(), Json::Str(help.to_owned())),
        (MISSING_KEY.to_owned(), Json::Strs(missing_keys)),
        ("_notes".to_owned(), Json::Object(note_members)),
    ];
    members.extend(entries.into_iter().map(|(key, text)| (key, Json::Str(text))));

    let mut out = String::new();
    write_object(&mut out, &members, 0);
    out.push('\n');
    out
}

/// `TranslationExport.ReadNotes(path)`: key to translator note from a JSON object. Keys starting with `_` and non-string
/// values are ignored; a duplicate key keeps the last value. A missing file, a file longer than
/// `LanguageCatalog::MAX_FILE_BYTES`, unreadable bytes, a parse error or a root that is not an object give an EMPTY map
/// (never an error). Syntax: comments and trailing commas are accepted (C# `CommentHandling.Skip`,
/// `AllowTrailingCommas`); a UTF-8 BOM is skipped. See open question 1 for the parser.
pub fn read_notes(path: &Path) -> BTreeMap<String, String> {
    let mut notes = BTreeMap::new();
    let Ok(metadata) = fs::metadata(path) else {
        return notes;
    };
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return notes;
    }
    let Ok(bytes) = fs::read(path) else {
        return notes;
    };
    let chars: Vec<char> = decode_text(&bytes).chars().collect();
    let Ok(Node::Object(members)) = parse_lenient_document(&chars) else {
        return notes;
    };
    for (name, value) in members {
        if name.starts_with('_') {
            continue;
        }
        if let Node::Str(text) = value {
            notes.insert(name, text);
        }
    }
    notes
}

/// `FreeBackupPath(path)`: `path + ".bak"`, or `path + ".bak" + n` for the first `n >= 2` whose file does not exist.
/// Appends to the OS string; never `Path::with_extension` (that would replace `.json`).
pub(crate) fn free_backup_path(path: &Path) -> PathBuf {
    let mut candidate = append_suffix(path, BACKUP_SUFFIX);
    let mut n = 2u32;
    while candidate.is_file() {
        candidate = append_suffix(path, &format!("{BACKUP_SUFFIX}{n}"));
        n += 1;
    }
    candidate
}

/// `ReadCatalogs(dir, code)`: parsed catalogs of `code` in `dir`. A missing folder gives an empty list. Files are taken
/// in OrdinalIgnoreCase order of their full path; only `*.json` files (ASCII case-insensitive suffix) whose file name
/// has no further `.` before the extension (`en.notes.json`, `xx.todo.json` are skipped), at most `MAX_FILE_BYTES`,
/// that parse with `LanguageCatalog::try_parse` and whose code equals `code` (OrdinalIgnoreCase). A broken or
/// unreadable single file is skipped. A failure to LIST the folder is an error (C# does not catch it).
pub(crate) fn read_catalogs(dir: &Path, code: &str) -> io::Result<Vec<LanguageCatalog>> {
    let mut found = Vec::new();
    if !dir.is_dir() {
        return Ok(found);
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if has_json_suffix(&name) {
            candidates.push(entry.path());
        }
    }
    candidates.sort_by(|a, b| cmp_ignore_case(&a.to_string_lossy(), &b.to_string_lossy()));

    for path in candidates {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Same rule as the loader: dotted names (en.notes.json, *.todo.json) are not catalogs.
        let stem = &name[..name.len() - ".json".len()];
        if stem.contains('.') {
            continue;
        }
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let text = decode_text(&bytes);
        let mut warnings = Vec::new();
        let parsed = LanguageCatalog::try_parse(&text, &path.to_string_lossy(), &mut warnings);
        if let Some(catalog) = parsed {
            if eq_ignore_case(catalog.code(), code) {
                found.push(catalog);
            }
        }
    }
    Ok(found)
}

// ---------------------------------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------------------------------

/// True for an English key `X.one` whose `X.other` English also has (C# `IsUnusedOneForm`).
fn is_unused_one_form(key: &str, english: &LanguageCatalog) -> bool {
    key.strip_suffix(".one")
        .is_some_and(|base| english.entry(&format!("{base}.other")).is_some())
}

/// `name` ends with `.json`, ASCII case-insensitively (the Windows `*.json` pattern).
fn has_json_suffix(name: &str) -> bool {
    let len = name.len();
    len >= ".json".len()
        && name.is_char_boundary(len - ".json".len())
        && name[len - ".json".len()..].eq_ignore_ascii_case(".json")
}

/// `path + suffix` on the OS string (`xx.todo.json` + `.bak` = `xx.todo.json.bak`).
fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Creates a new file with `bytes`; fails when the file exists, so a backup is never overwritten.
fn write_new_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)
}

/// Text of file bytes as `File.ReadAllText` sees them for UTF-8: one BOM is removed, invalid bytes become U+FFFD.
fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

// ---------------------------------------------------------------------------------------------------------------------
// writer (T3: the order of members is written by hand, `serde_json::Map` sorts keys)
// ---------------------------------------------------------------------------------------------------------------------

/// The shapes the export writes: a string, an array of strings, or an object (members in order).
enum Json {
    Str(String),
    Strs(Vec<String>),
    Object(Vec<(String, Json)>),
}

fn write_value(out: &mut String, value: &Json, depth: usize) {
    match value {
        Json::Str(text) => out.push_str(&json_string(text)),
        Json::Strs(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                push_indent(out, depth + 1);
                out.push_str(&json_string(item));
                if index + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, depth);
            out.push(']');
        }
        Json::Object(members) => write_object(out, members, depth),
    }
}

fn write_object(out: &mut String, members: &[(String, Json)], depth: usize) {
    if members.is_empty() {
        out.push_str("{}");
        return;
    }
    out.push_str("{\n");
    for (index, (name, value)) in members.iter().enumerate() {
        push_indent(out, depth + 1);
        out.push_str(&json_string(name));
        out.push_str(": ");
        write_value(out, value, depth + 1);
        if index + 1 < members.len() {
            out.push(',');
        }
        out.push('\n');
    }
    push_indent(out, depth);
    out.push('}');
}

fn push_indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// A JSON string literal with the escape rule of `JavaScriptEncoder.UnsafeRelaxedJsonEscaping` as the golden shows it:
/// `"`, `\` and the short escapes; other C0 controls, DEL, C1 controls, U+2028, U+2029 and every non-BMP character as
/// `\uXXXX` (uppercase hex, surrogate pairs for non-BMP), plus the code points of `ESCAPED_BMP_RANGES` (NBSP, unassigned, ...).
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if needs_unicode_escape(c) => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{unit:04X}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn needs_unicode_escape(c: char) -> bool {
    let code = c as u32;
    if code >= 0x1_0000 {
        return true;
    }
    code < 0x20
        || code == 0x7F
        || ESCAPED_BMP_RANGES
            .binary_search_by(|&(lo, hi)| {
                if code < lo {
                    std::cmp::Ordering::Greater
                } else if code > hi {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
}

/// BMP code points >= U+0080 that .NET `UnsafeRelaxedJsonEscaping` writes as `\uXXXX` (C1 controls, NBSP, unassigned,
/// space separators, line/paragraph separators, private use, noncharacters). Dumped from System.Text.Json on net10.
/// Sorted, inclusive ranges.
const ESCAPED_BMP_RANGES: &[(u32, u32)] = &[
    (0x0080, 0x00A0),
    (0x0378, 0x0379),
    (0x0380, 0x0383),
    (0x038B, 0x038B),
    (0x038D, 0x038D),
    (0x03A2, 0x03A2),
    (0x0530, 0x0530),
    (0x0557, 0x0558),
    (0x058B, 0x058C),
    (0x0590, 0x0590),
    (0x05C8, 0x05CF),
    (0x05EB, 0x05EE),
    (0x05F5, 0x05FF),
    (0x070E, 0x070E),
    (0x074B, 0x074C),
    (0x07B2, 0x07BF),
    (0x07FB, 0x07FC),
    (0x082E, 0x082F),
    (0x083F, 0x083F),
    (0x085C, 0x085D),
    (0x085F, 0x085F),
    (0x086B, 0x086F),
    (0x088F, 0x088F),
    (0x0892, 0x0896),
    (0x0984, 0x0984),
    (0x098D, 0x098E),
    (0x0991, 0x0992),
    (0x09A9, 0x09A9),
    (0x09B1, 0x09B1),
    (0x09B3, 0x09B5),
    (0x09BA, 0x09BB),
    (0x09C5, 0x09C6),
    (0x09C9, 0x09CA),
    (0x09CF, 0x09D6),
    (0x09D8, 0x09DB),
    (0x09DE, 0x09DE),
    (0x09E4, 0x09E5),
    (0x09FF, 0x0A00),
    (0x0A04, 0x0A04),
    (0x0A0B, 0x0A0E),
    (0x0A11, 0x0A12),
    (0x0A29, 0x0A29),
    (0x0A31, 0x0A31),
    (0x0A34, 0x0A34),
    (0x0A37, 0x0A37),
    (0x0A3A, 0x0A3B),
    (0x0A3D, 0x0A3D),
    (0x0A43, 0x0A46),
    (0x0A49, 0x0A4A),
    (0x0A4E, 0x0A50),
    (0x0A52, 0x0A58),
    (0x0A5D, 0x0A5D),
    (0x0A5F, 0x0A65),
    (0x0A77, 0x0A80),
    (0x0A84, 0x0A84),
    (0x0A8E, 0x0A8E),
    (0x0A92, 0x0A92),
    (0x0AA9, 0x0AA9),
    (0x0AB1, 0x0AB1),
    (0x0AB4, 0x0AB4),
    (0x0ABA, 0x0ABB),
    (0x0AC6, 0x0AC6),
    (0x0ACA, 0x0ACA),
    (0x0ACE, 0x0ACF),
    (0x0AD1, 0x0ADF),
    (0x0AE4, 0x0AE5),
    (0x0AF2, 0x0AF8),
    (0x0B00, 0x0B00),
    (0x0B04, 0x0B04),
    (0x0B0D, 0x0B0E),
    (0x0B11, 0x0B12),
    (0x0B29, 0x0B29),
    (0x0B31, 0x0B31),
    (0x0B34, 0x0B34),
    (0x0B3A, 0x0B3B),
    (0x0B45, 0x0B46),
    (0x0B49, 0x0B4A),
    (0x0B4E, 0x0B54),
    (0x0B58, 0x0B5B),
    (0x0B5E, 0x0B5E),
    (0x0B64, 0x0B65),
    (0x0B78, 0x0B81),
    (0x0B84, 0x0B84),
    (0x0B8B, 0x0B8D),
    (0x0B91, 0x0B91),
    (0x0B96, 0x0B98),
    (0x0B9B, 0x0B9B),
    (0x0B9D, 0x0B9D),
    (0x0BA0, 0x0BA2),
    (0x0BA5, 0x0BA7),
    (0x0BAB, 0x0BAD),
    (0x0BBA, 0x0BBD),
    (0x0BC3, 0x0BC5),
    (0x0BC9, 0x0BC9),
    (0x0BCE, 0x0BCF),
    (0x0BD1, 0x0BD6),
    (0x0BD8, 0x0BE5),
    (0x0BFB, 0x0BFF),
    (0x0C0D, 0x0C0D),
    (0x0C11, 0x0C11),
    (0x0C29, 0x0C29),
    (0x0C3A, 0x0C3B),
    (0x0C45, 0x0C45),
    (0x0C49, 0x0C49),
    (0x0C4E, 0x0C54),
    (0x0C57, 0x0C57),
    (0x0C5B, 0x0C5C),
    (0x0C5E, 0x0C5F),
    (0x0C64, 0x0C65),
    (0x0C70, 0x0C76),
    (0x0C8D, 0x0C8D),
    (0x0C91, 0x0C91),
    (0x0CA9, 0x0CA9),
    (0x0CB4, 0x0CB4),
    (0x0CBA, 0x0CBB),
    (0x0CC5, 0x0CC5),
    (0x0CC9, 0x0CC9),
    (0x0CCE, 0x0CD4),
    (0x0CD7, 0x0CDC),
    (0x0CDF, 0x0CDF),
    (0x0CE4, 0x0CE5),
    (0x0CF0, 0x0CF0),
    (0x0CF4, 0x0CFF),
    (0x0D0D, 0x0D0D),
    (0x0D11, 0x0D11),
    (0x0D45, 0x0D45),
    (0x0D49, 0x0D49),
    (0x0D50, 0x0D53),
    (0x0D64, 0x0D65),
    (0x0D80, 0x0D80),
    (0x0D84, 0x0D84),
    (0x0D97, 0x0D99),
    (0x0DB2, 0x0DB2),
    (0x0DBC, 0x0DBC),
    (0x0DBE, 0x0DBF),
    (0x0DC7, 0x0DC9),
    (0x0DCB, 0x0DCE),
    (0x0DD5, 0x0DD5),
    (0x0DD7, 0x0DD7),
    (0x0DE0, 0x0DE5),
    (0x0DF0, 0x0DF1),
    (0x0DF5, 0x0E00),
    (0x0E3B, 0x0E3E),
    (0x0E5C, 0x0E80),
    (0x0E83, 0x0E83),
    (0x0E85, 0x0E85),
    (0x0E8B, 0x0E8B),
    (0x0EA4, 0x0EA4),
    (0x0EA6, 0x0EA6),
    (0x0EBE, 0x0EBF),
    (0x0EC5, 0x0EC5),
    (0x0EC7, 0x0EC7),
    (0x0ECF, 0x0ECF),
    (0x0EDA, 0x0EDB),
    (0x0EE0, 0x0EFF),
    (0x0F48, 0x0F48),
    (0x0F6D, 0x0F70),
    (0x0F98, 0x0F98),
    (0x0FBD, 0x0FBD),
    (0x0FCD, 0x0FCD),
    (0x0FDB, 0x0FFF),
    (0x10C6, 0x10C6),
    (0x10C8, 0x10CC),
    (0x10CE, 0x10CF),
    (0x1249, 0x1249),
    (0x124E, 0x124F),
    (0x1257, 0x1257),
    (0x1259, 0x1259),
    (0x125E, 0x125F),
    (0x1289, 0x1289),
    (0x128E, 0x128F),
    (0x12B1, 0x12B1),
    (0x12B6, 0x12B7),
    (0x12BF, 0x12BF),
    (0x12C1, 0x12C1),
    (0x12C6, 0x12C7),
    (0x12D7, 0x12D7),
    (0x1311, 0x1311),
    (0x1316, 0x1317),
    (0x135B, 0x135C),
    (0x137D, 0x137F),
    (0x139A, 0x139F),
    (0x13F6, 0x13F7),
    (0x13FE, 0x13FF),
    (0x1680, 0x1680),
    (0x169D, 0x169F),
    (0x16F9, 0x16FF),
    (0x1716, 0x171E),
    (0x1737, 0x173F),
    (0x1754, 0x175F),
    (0x176D, 0x176D),
    (0x1771, 0x1771),
    (0x1774, 0x177F),
    (0x17DE, 0x17DF),
    (0x17EA, 0x17EF),
    (0x17FA, 0x17FF),
    (0x181A, 0x181F),
    (0x1879, 0x187F),
    (0x18AB, 0x18AF),
    (0x18F6, 0x18FF),
    (0x191F, 0x191F),
    (0x192C, 0x192F),
    (0x193C, 0x193F),
    (0x1941, 0x1943),
    (0x196E, 0x196F),
    (0x1975, 0x197F),
    (0x19AC, 0x19AF),
    (0x19CA, 0x19CF),
    (0x19DB, 0x19DD),
    (0x1A1C, 0x1A1D),
    (0x1A5F, 0x1A5F),
    (0x1A7D, 0x1A7E),
    (0x1A8A, 0x1A8F),
    (0x1A9A, 0x1A9F),
    (0x1AAE, 0x1AAF),
    (0x1ACF, 0x1AFF),
    (0x1B4D, 0x1B4D),
    (0x1BF4, 0x1BFB),
    (0x1C38, 0x1C3A),
    (0x1C4A, 0x1C4C),
    (0x1C8B, 0x1C8F),
    (0x1CBB, 0x1CBC),
    (0x1CC8, 0x1CCF),
    (0x1CFB, 0x1CFF),
    (0x1F16, 0x1F17),
    (0x1F1E, 0x1F1F),
    (0x1F46, 0x1F47),
    (0x1F4E, 0x1F4F),
    (0x1F58, 0x1F58),
    (0x1F5A, 0x1F5A),
    (0x1F5C, 0x1F5C),
    (0x1F5E, 0x1F5E),
    (0x1F7E, 0x1F7F),
    (0x1FB5, 0x1FB5),
    (0x1FC5, 0x1FC5),
    (0x1FD4, 0x1FD5),
    (0x1FDC, 0x1FDC),
    (0x1FF0, 0x1FF1),
    (0x1FF5, 0x1FF5),
    (0x1FFF, 0x200A),
    (0x2028, 0x2029),
    (0x202F, 0x202F),
    (0x205F, 0x205F),
    (0x2065, 0x2065),
    (0x2072, 0x2073),
    (0x208F, 0x208F),
    (0x209D, 0x209F),
    (0x20C1, 0x20CF),
    (0x20F1, 0x20FF),
    (0x218C, 0x218F),
    (0x242A, 0x243F),
    (0x244B, 0x245F),
    (0x2B74, 0x2B75),
    (0x2B96, 0x2B96),
    (0x2CF4, 0x2CF8),
    (0x2D26, 0x2D26),
    (0x2D28, 0x2D2C),
    (0x2D2E, 0x2D2F),
    (0x2D68, 0x2D6E),
    (0x2D71, 0x2D7E),
    (0x2D97, 0x2D9F),
    (0x2DA7, 0x2DA7),
    (0x2DAF, 0x2DAF),
    (0x2DB7, 0x2DB7),
    (0x2DBF, 0x2DBF),
    (0x2DC7, 0x2DC7),
    (0x2DCF, 0x2DCF),
    (0x2DD7, 0x2DD7),
    (0x2DDF, 0x2DDF),
    (0x2E5E, 0x2E7F),
    (0x2E9A, 0x2E9A),
    (0x2EF4, 0x2EFF),
    (0x2FD6, 0x2FEF),
    (0x3000, 0x3000),
    (0x3040, 0x3040),
    (0x3097, 0x3098),
    (0x3100, 0x3104),
    (0x3130, 0x3130),
    (0x318F, 0x318F),
    (0x31E6, 0x31EE),
    (0x321F, 0x321F),
    (0xA48D, 0xA48F),
    (0xA4C7, 0xA4CF),
    (0xA62C, 0xA63F),
    (0xA6F8, 0xA6FF),
    (0xA7CE, 0xA7CF),
    (0xA7D2, 0xA7D2),
    (0xA7D4, 0xA7D4),
    (0xA7DD, 0xA7F1),
    (0xA82D, 0xA82F),
    (0xA83A, 0xA83F),
    (0xA878, 0xA87F),
    (0xA8C6, 0xA8CD),
    (0xA8DA, 0xA8DF),
    (0xA954, 0xA95E),
    (0xA97D, 0xA97F),
    (0xA9CE, 0xA9CE),
    (0xA9DA, 0xA9DD),
    (0xA9FF, 0xA9FF),
    (0xAA37, 0xAA3F),
    (0xAA4E, 0xAA4F),
    (0xAA5A, 0xAA5B),
    (0xAAC3, 0xAADA),
    (0xAAF7, 0xAB00),
    (0xAB07, 0xAB08),
    (0xAB0F, 0xAB10),
    (0xAB17, 0xAB1F),
    (0xAB27, 0xAB27),
    (0xAB2F, 0xAB2F),
    (0xAB6C, 0xAB6F),
    (0xABEE, 0xABEF),
    (0xABFA, 0xABFF),
    (0xD7A4, 0xD7AF),
    (0xD7C7, 0xD7CA),
    (0xD7FC, 0xD7FF),
    (0xE000, 0xF8FF),
    (0xFA6E, 0xFA6F),
    (0xFADA, 0xFAFF),
    (0xFB07, 0xFB12),
    (0xFB18, 0xFB1C),
    (0xFB37, 0xFB37),
    (0xFB3D, 0xFB3D),
    (0xFB3F, 0xFB3F),
    (0xFB42, 0xFB42),
    (0xFB45, 0xFB45),
    (0xFBC3, 0xFBD2),
    (0xFD90, 0xFD91),
    (0xFDC8, 0xFDCE),
    (0xFDD0, 0xFDEF),
    (0xFE1A, 0xFE1F),
    (0xFE53, 0xFE53),
    (0xFE67, 0xFE67),
    (0xFE6C, 0xFE6F),
    (0xFE75, 0xFE75),
    (0xFEFD, 0xFF00),
    (0xFFBF, 0xFFC1),
    (0xFFC8, 0xFFC9),
    (0xFFD0, 0xFFD1),
    (0xFFD8, 0xFFD9),
    (0xFFDD, 0xFFDF),
    (0xFFE7, 0xFFE7),
    (0xFFEF, 0xFFF8),
    (0xFFFE, 0xFFFF),
];

// ---------------------------------------------------------------------------------------------------------------------
// lenient reader for en.notes.json (T4, open question 1: comments and trailing commas as `JsonDocument.Parse` with
// `CommentHandling.Skip` and `AllowTrailingCommas`)
// ---------------------------------------------------------------------------------------------------------------------

/// `JsonDocumentOptions.MaxDepth` default.
const MAX_NOTES_DEPTH: usize = 64;

/// A parsed value. Only what [`read_notes`] needs is kept: strings and object members. Arrays, numbers and literals
/// are checked for syntax and then dropped.
enum Node {
    Str(String),
    Object(Vec<(String, Node)>),
    Other,
}

type ParseResult<T> = Result<T, ()>;

/// Parses the whole document; trailing non-whitespace after the root is an error (C# `JsonDocument.Parse`).
fn parse_lenient_document(chars: &[char]) -> ParseResult<Node> {
    let mut parser = LenientParser {
        chars,
        pos: 0,
        depth: 0,
    };
    parser.skip_trivia()?;
    let root = parser.value()?;
    parser.skip_trivia()?;
    if parser.pos != chars.len() {
        return Err(());
    }
    Ok(root)
}

struct LenientParser<'a> {
    chars: &'a [char],
    pos: usize,
    depth: usize,
}

impl LenientParser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Whitespace, `//` line comments and `/* */` block comments.
    fn skip_trivia(&mut self) -> ParseResult<()> {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r' | '\n') => self.pos += 1,
                Some('/') => match self.chars.get(self.pos + 1) {
                    Some('/') => {
                        self.pos += 2;
                        while let Some(c) = self.peek() {
                            self.pos += 1;
                            if c == '\n' || c == '\r' {
                                break;
                            }
                        }
                    }
                    Some('*') => {
                        self.pos += 2;
                        loop {
                            match self.peek() {
                                None => return Err(()),
                                Some('*') if self.chars.get(self.pos + 1) == Some(&'/') => {
                                    self.pos += 2;
                                    break;
                                }
                                Some(_) => self.pos += 1,
                            }
                        }
                    }
                    _ => return Err(()),
                },
                _ => return Ok(()),
            }
        }
    }

    fn enter(&mut self) -> ParseResult<()> {
        self.depth += 1;
        if self.depth > MAX_NOTES_DEPTH { Err(()) } else { Ok(()) }
    }

    fn value(&mut self) -> ParseResult<Node> {
        match self.peek().ok_or(())? {
            '{' => self.object(),
            '[' => {
                self.array()?;
                Ok(Node::Other)
            }
            '"' => Ok(Node::Str(self.string()?)),
            't' => self.literal("true"),
            'f' => self.literal("false"),
            'n' => self.literal("null"),
            '-' | '0'..='9' => {
                self.number()?;
                Ok(Node::Other)
            }
            _ => Err(()),
        }
    }

    fn object(&mut self) -> ParseResult<Node> {
        self.enter()?;
        self.pos += 1;
        let mut members = Vec::new();
        loop {
            self.skip_trivia()?;
            match self.peek() {
                Some('}') => {
                    self.pos += 1;
                    break;
                }
                Some('"') => {}
                _ => return Err(()),
            }
            let name = self.string()?;
            // .NET rejects a comment between the name and the colon: whitespace only here.
            while matches!(self.peek(), Some(' ' | '\t' | '\r' | '\n')) {
                self.pos += 1;
            }
            if self.peek() != Some(':') {
                return Err(());
            }
            self.pos += 1;
            self.skip_trivia()?;
            let value = self.value()?;
            members.push((name, value));
            self.skip_trivia()?;
            match self.peek() {
                Some(',') => self.pos += 1,
                Some('}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(()),
            }
        }
        self.depth -= 1;
        Ok(Node::Object(members))
    }

    fn array(&mut self) -> ParseResult<()> {
        self.enter()?;
        self.pos += 1;
        loop {
            self.skip_trivia()?;
            if self.peek() == Some(']') {
                self.pos += 1;
                break;
            }
            self.value()?;
            self.skip_trivia()?;
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(']') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(()),
            }
        }
        self.depth -= 1;
        Ok(())
    }

    fn string(&mut self) -> ParseResult<String> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or(())?;
            self.pos += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let escape = self.peek().ok_or(())?;
                    self.pos += 1;
                    match escape {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{C}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => out.push(self.unicode_escape()?),
                        _ => return Err(()),
                    }
                }
                c if (c as u32) < 0x20 => return Err(()),
                c => out.push(c),
            }
        }
    }

    /// The character of a `\uXXXX` escape (the `\u` is consumed). A surrogate must come as a pair; a lone one is an
    /// error (the notes reader then gives no notes, as for any unreadable file).
    fn unicode_escape(&mut self) -> ParseResult<char> {
        let unit = self.hex4()?;
        let scalar = if (0xD800..=0xDBFF).contains(&unit) {
            if self.peek() != Some('\\') || self.chars.get(self.pos + 1) != Some(&'u') {
                return Err(());
            }
            self.pos += 2;
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(());
            }
            0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
        } else if (0xDC00..=0xDFFF).contains(&unit) {
            return Err(());
        } else {
            unit
        };
        char::from_u32(scalar).ok_or(())
    }

    fn hex4(&mut self) -> ParseResult<u32> {
        let mut value = 0u32;
        for _ in 0..4 {
            let digit = self.peek().and_then(|c| c.to_digit(16)).ok_or(())?;
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn number(&mut self) -> ParseResult<()> {
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        match self.peek() {
            Some('0') => self.pos += 1,
            Some('1'..='9') => self.digits()?,
            _ => return Err(()),
        }
        if self.peek() == Some('.') {
            self.pos += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            self.digits()?;
        }
        Ok(())
    }

    /// One or more ASCII digits.
    fn digits(&mut self) -> ParseResult<()> {
        let start = self.pos;
        while matches!(self.peek(), Some('0'..='9')) {
            self.pos += 1;
        }
        if self.pos == start { Err(()) } else { Ok(()) }
    }

    fn literal(&mut self, word: &str) -> ParseResult<Node> {
        for expected in word.chars() {
            if self.peek() != Some(expected) {
                return Err(());
            }
            self.pos += 1;
        }
        Ok(Node::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_string_escapes_controls_and_leaves_text_raw() {
        assert_eq!(json_string("a\"b\\c\n\u{1}"), "\"a\\\"b\\\\c\\n\\u0001\"");
        assert_eq!(json_string("é ✓ </b> & 'q' +"), "\"é ✓ </b> & 'q' +\"");
    }

    #[test]
    fn json_string_escapes_del_line_separators_and_surrogate_pairs() {
        assert_eq!(json_string("\u{7F}\u{2028}"), "\"\\u007F\\u2028\"");
        assert_eq!(json_string("\u{1F4C1}"), "\"\\uD83D\\uDCC1\"");
    }

    #[test]
    fn lenient_parser_accepts_comments_and_trailing_commas() {
        let chars: Vec<char> = "// c\n{ /* c */ \"a\": \"S\", }".chars().collect();
        let Ok(Node::Object(members)) = parse_lenient_document(&chars) else {
            panic!("should parse");
        };
        assert_eq!(members.len(), 1);
    }

    #[test]
    fn lenient_parser_rejects_trailing_garbage_and_lone_surrogate() {
        let garbage: Vec<char> = "{\"a\":\"1\"} x".chars().collect();
        assert!(parse_lenient_document(&garbage).is_err());
        let surrogate: Vec<char> = "{\"a\":\"\\ud800\"}".chars().collect();
        assert!(parse_lenient_document(&surrogate).is_err());
    }
}
