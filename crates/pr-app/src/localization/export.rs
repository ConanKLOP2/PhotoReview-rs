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
/// `\uXXXX` (uppercase hex, surrogate pairs for non-BMP). Other non-ASCII text stays raw.
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
    code < 0x20 || (0x7F..=0x9F).contains(&code) || code == 0x2028 || code == 0x2029 || code > 0xFFFF
}

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
                            if c == '\n' {
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
            self.skip_trivia()?;
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
