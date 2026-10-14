//! Port of `PhotoReview.Core/Localization/LanguageCatalog.cs` (C# baseline d92d6aae, 213 lines): one translation file,
//! parsed but not yet validated against English. Translation files are untrusted: [`LanguageCatalog::try_parse`]
//! never panics and reports every problem as a warning string.
//!
//! The JSON layer is hand-written here (NOT `serde_json`): `System.Text.Json` with `CommentHandling.Skip` and
//! `AllowTrailingCommas`, maximum depth 64, duplicate names kept in order, strings materialised lazily (a lone
//! surrogate escape is only an error when the text is actually read). Contract: `contracts/W2-04.md`.
//!
//! Unit W2-04.

use std::collections::HashMap;

use pr_core::text::eq_ignore_case;

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
        self.index.get(key).map(|&i| self.entries[i].1.as_str())
    }

    /// C# `LanguageCatalog.TryParse(json, source, out catalog, warnings)`. `None` is the `false` result; every
    /// problem (also on success) is appended to `warnings` in the order C# adds it. Never panics.
    pub fn try_parse(json: &str, source: &str, warnings: &mut Vec<String>) -> Option<LanguageCatalog> {
        let root = match parse_document(json) {
            Ok(root) => root,
            Err(message) => {
                warnings.push(format!(
                    "{}: invalid JSON ({})",
                    Self::safe(Some(source)),
                    Self::safe(Some(&message))
                ));
                return None;
            }
        };
        match read_document(&root, source, warnings) {
            Ok(catalog) => catalog,
            Err(message) => {
                // Well-formed JSON can still hold text that cannot be read (a lone "\ud800" surrogate escape).
                warnings.push(format!(
                    "{}: unreadable text ({})",
                    Self::safe(Some(source)),
                    Self::safe(Some(&message))
                ));
                None
            }
        }
    }

    /// BCP-47-ish: ASCII letters, digits and `-`, 2..=16 chars (UTF-16 length = char count here), first char an ASCII
    /// letter. C# `IsValidCode`.
    pub fn is_valid_code(code: &str) -> bool {
        let count = code.chars().count();
        (2..=16).contains(&count)
            && code.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }

    /// Makes untrusted text safe to embed in a one-line warning: control characters (`char::is_control`) become
    /// U+FFFD and text longer than 120 UTF-16 units is cut and gets `"..."` appended. `None` is `""`.
    /// C# `LanguageCatalog.Safe` (internal; `pub` here because `Localizer` and `LanguageLoader` use it).
    pub fn safe(value: Option<&str>) -> String {
        let Some(value) = value else {
            return String::new();
        };
        let cleaned = strip_control(value, '\u{FFFD}');
        if utf16_len(&cleaned) > MAX_WARNING_FRAGMENT_LENGTH {
            let mut cut = cut_utf16(&cleaned, MAX_WARNING_FRAGMENT_LENGTH);
            cut.push_str("...");
            cut
        } else {
            cleaned
        }
    }
}

fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// The first `units` UTF-16 units of `text`. C# keeps a lone high surrogate when the cut splits a pair; a Rust
/// `String` cannot, and the oracle serialises it as U+FFFD, so the split pair becomes one U+FFFD.
fn cut_utf16(text: &str, units: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let width = c.len_utf16();
        if used + width > units {
            if used < units {
                out.push('\u{FFFD}');
            }
            break;
        }
        out.push(c);
        used += width;
    }
    out
}

fn strip_control(value: &str, replacement: char) -> String {
    if !value.chars().any(char::is_control) {
        return value.to_owned();
    }
    value
        .chars()
        .map(|c| if c.is_control() { replacement } else { c })
        .collect()
}

/// C# `CleanDisplayName`: control characters become spaces, trimmed, capped at 64 UTF-16 units (then trimmed
/// again); `None` when nothing visible is left.
fn clean_display_name(value: &str) -> Option<String> {
    let stripped = strip_control(value, ' ');
    let trimmed = stripped.trim();
    let cleaned = if utf16_len(trimmed) > MAX_DISPLAY_NAME_LENGTH {
        cut_utf16(trimmed, MAX_DISPLAY_NAME_LENGTH).trim_end().to_owned()
    } else {
        trimmed.to_owned()
    };
    if cleaned.trim().is_empty() { None } else { Some(cleaned) }
}

// ---------------------------------------------------------------------------------------------------------------
// JSON layer (System.Text.Json: CommentHandling.Skip, AllowTrailingCommas, MaxDepth 64)
// ---------------------------------------------------------------------------------------------------------------

/// Maximum nesting depth; the root object is depth 1.
const MAX_JSON_DEPTH: usize = 64;

/// A string token. `bad` marks a lone surrogate escape: valid JSON, but reading the text fails (C# `GetString()`).
#[derive(Debug)]
struct JsonStr {
    text: String,
    bad: bool,
}

impl JsonStr {
    /// C# reading `property.Name` / `GetString()`: fails for a lone surrogate.
    fn read(&self) -> Result<&str, String> {
        if self.bad {
            Err("Cannot transcode invalid UTF-16 string to UTF-8: the string contains an unpaired surrogate".to_owned())
        } else {
            Ok(&self.text)
        }
    }
}

#[derive(Debug)]
enum JsonValue {
    Object(Vec<(JsonStr, JsonValue)>),
    Array(Vec<JsonValue>),
    String(JsonStr),
    /// Number, `true`, `false` or `null`: only "not a string / object / array" matters.
    Other,
}

fn parse_document(json: &str) -> Result<JsonValue, String> {
    let mut parser = Parser {
        text: json,
        bytes: json.as_bytes(),
        pos: 0,
    };
    parser.skip_trivia()?;
    let value = parser.parse_value(0)?;
    parser.skip_trivia()?;
    if parser.pos < parser.bytes.len() {
        return Err(format!(
            "'{}' is invalid after a single JSON value",
            parser.describe_here()
        ));
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn describe_here(&self) -> String {
        self.text[self.pos..]
            .chars()
            .next()
            .map(String::from)
            .unwrap_or_default()
    }

    fn fail<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{what}. Path: $ | BytePositionInLine: {}.", self.pos))
    }

    fn skip_trivia(&mut self) -> Result<(), String> {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\r' | b'\n' => self.pos += 1,
                b'/' => match self.bytes.get(self.pos + 1) {
                    Some(b'/') => {
                        self.pos += 2;
                        while self.pos < self.bytes.len() {
                            let rest = &self.bytes[self.pos..];
                            if rest[0] == b'\n' || rest[0] == b'\r' {
                                self.pos += 1;
                                break;
                            }
                            if rest.starts_with("\u{2028}".as_bytes()) || rest.starts_with("\u{2029}".as_bytes()) {
                                // System.Text.Json: a line separator inside a // comment is invalid.
                                return self.fail("Invalid line separator within a single-line comment");
                            }
                            self.pos += 1;
                        }
                    }
                    Some(b'*') => match self.text[self.pos + 2..].find("*/") {
                        Some(end) => self.pos += 2 + end + 2,
                        None => return self.fail("Expected end of comment, but instead reached end of data"),
                    },
                    _ => return self.fail("'/' is an invalid start of a value"),
                },
                _ => return Ok(()),
            }
        }
        Ok(())
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<JsonValue, String> {
        let Some(&c) = self.bytes.get(self.pos) else {
            return self.fail("Expected a JSON value, but instead reached end of data");
        };
        match c {
            b'{' => self.parse_object(depth + 1),
            b'[' => self.parse_array(depth + 1),
            b'"' => Ok(JsonValue::String(self.parse_string()?)),
            b't' => self.literal("true"),
            b'f' => self.literal("false"),
            b'n' => self.literal("null"),
            b'-' | b'0'..=b'9' => self.number(),
            _ => self.fail(&format!("'{}' is an invalid start of a value", self.describe_here())),
        }
    }

    fn literal(&mut self, word: &str) -> Result<JsonValue, String> {
        if !self.bytes[self.pos..].starts_with(word.as_bytes()) {
            return self.fail(&format!("'{word}' literal expected"));
        }
        self.pos += word.len();
        Ok(JsonValue::Other)
    }

    fn number(&mut self) -> Result<JsonValue, String> {
        let digits = |p: &Self, from: usize| {
            let mut i = from;
            while p.bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            i
        };
        let mut i = self.pos;
        if self.bytes.get(i) == Some(&b'-') {
            i += 1;
        }
        let int_start = i;
        i = digits(self, i);
        if i == int_start {
            return self.fail("Expected a digit");
        }
        if self.bytes[int_start] == b'0' && i - int_start > 1 {
            return self.fail("Leading zeros are not allowed");
        }
        if self.bytes.get(i) == Some(&b'.') {
            let frac = digits(self, i + 1);
            if frac == i + 1 {
                return self.fail("Expected a digit after the decimal point");
            }
            i = frac;
        }
        if matches!(self.bytes.get(i), Some(b'e' | b'E')) {
            let mut j = i + 1;
            if matches!(self.bytes.get(j), Some(b'+' | b'-')) {
                j += 1;
            }
            let exp = digits(self, j);
            if exp == j {
                return self.fail("Expected a digit in the exponent");
            }
            i = exp;
        }
        self.pos = i;
        Ok(JsonValue::Other)
    }

    fn parse_object(&mut self, depth: usize) -> Result<JsonValue, String> {
        if depth > MAX_JSON_DEPTH {
            return self.fail("The maximum configured depth of 64 has been exceeded");
        }
        self.pos += 1; // '{'
        let mut properties = Vec::new();
        self.skip_trivia()?;
        if self.bytes.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            return Ok(JsonValue::Object(properties));
        }
        loop {
            // After a comma a trailing comma may directly close the object (checked below), else a name follows.
            if self.bytes.get(self.pos) != Some(&b'"') {
                return self.fail("Expected start of a property name");
            }
            let name = self.parse_string()?;
            self.skip_whitespace(); // no comment is allowed between a name and its ':'
            if self.bytes.get(self.pos) != Some(&b':') {
                return self.fail("Expected a ':' after a property name");
            }
            self.pos += 1;
            self.skip_trivia()?;
            let value = self.parse_value(depth)?;
            properties.push((name, value));
            self.skip_trivia()?;
            match self.bytes.get(self.pos) {
                Some(b',') => {
                    self.pos += 1;
                    self.skip_trivia()?;
                    if self.bytes.get(self.pos) == Some(&b'}') {
                        self.pos += 1;
                        return Ok(JsonValue::Object(properties));
                    }
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(JsonValue::Object(properties));
                }
                _ => return self.fail("Expected ',' or '}' after a property value"),
            }
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<JsonValue, String> {
        if depth > MAX_JSON_DEPTH {
            return self.fail("The maximum configured depth of 64 has been exceeded");
        }
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_trivia()?;
        if self.bytes.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            items.push(self.parse_value(depth)?);
            self.skip_trivia()?;
            match self.bytes.get(self.pos) {
                Some(b',') => {
                    self.pos += 1;
                    self.skip_trivia()?;
                    if self.bytes.get(self.pos) == Some(&b']') {
                        self.pos += 1;
                        return Ok(JsonValue::Array(items));
                    }
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(JsonValue::Array(items));
                }
                _ => return self.fail("Expected ',' or ']' after an array element"),
            }
        }
    }

    fn hex4(&self, at: usize) -> Option<u32> {
        let digits = self.bytes.get(at..at + 4)?;
        let mut value = 0_u32;
        for &b in digits {
            value = value * 16 + char::from(b).to_digit(16)?;
        }
        Some(value)
    }

    fn parse_string(&mut self) -> Result<JsonStr, String> {
        self.pos += 1; // opening quote
        let mut text = String::new();
        let mut bad = false;
        loop {
            let Some(&b) = self.bytes.get(self.pos) else {
                return self.fail("Expected end of string, but instead reached end of data");
            };
            match b {
                b'"' => {
                    self.pos += 1;
                    return Ok(JsonStr { text, bad });
                }
                0..=0x1F => return self.fail("Invalid control character within a string"),
                b'\\' => {
                    let Some(&e) = self.bytes.get(self.pos + 1) else {
                        return self.fail("Expected end of string, but instead reached end of data");
                    };
                    self.pos += 2;
                    match e {
                        b'"' => text.push('"'),
                        b'\\' => text.push('\\'),
                        b'/' => text.push('/'),
                        b'b' => text.push('\u{8}'),
                        b'f' => text.push('\u{C}'),
                        b'n' => text.push('\n'),
                        b'r' => text.push('\r'),
                        b't' => text.push('\t'),
                        b'u' => {
                            let Some(unit) = self.hex4(self.pos) else {
                                return self.fail("'\\u' must be followed by four hex digits");
                            };
                            self.pos += 4;
                            if (0xD800..0xDC00).contains(&unit) {
                                // A pair when `\uDC00..\uDFFF` follows right away.
                                let low = if self.bytes[self.pos..].starts_with(b"\\u") {
                                    self.hex4(self.pos + 2).filter(|l| (0xDC00..0xE000).contains(l))
                                } else {
                                    None
                                };
                                match low {
                                    Some(low) => {
                                        self.pos += 6;
                                        let cp = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                                        text.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                                    }
                                    None => {
                                        bad = true;
                                        text.push('\u{FFFD}');
                                    }
                                }
                            } else if (0xDC00..0xE000).contains(&unit) {
                                bad = true;
                                text.push('\u{FFFD}');
                            } else {
                                text.push(char::from_u32(unit).unwrap_or('\u{FFFD}'));
                            }
                        }
                        _ => return self.fail("'\\' is an invalid escapable character within a JSON string"),
                    }
                }
                _ => {
                    // Copy one whole UTF-8 sequence.
                    let Some(ch) = self.text[self.pos..].chars().next() else {
                        return self.fail("Expected end of string, but instead reached end of data");
                    };
                    text.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Catalog layer
// ---------------------------------------------------------------------------------------------------------------

#[derive(Default)]
struct MetaState {
    code: Option<String>,
    name: Option<String>,
    native_name: Option<String>,
    plural: PluralRule,
    plural_declared: bool,
    authors: Vec<String>,
}

/// C# `ParseDocument`: `Err` is an "unreadable text" failure (the first lone-surrogate read aborts the parse).
fn read_document(
    root: &JsonValue,
    source: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<LanguageCatalog>, String> {
    let JsonValue::Object(properties) = root else {
        warnings.push(format!(
            "{}: root must be a JSON object",
            LanguageCatalog::safe(Some(source))
        ));
        return Ok(None);
    };
    let src = LanguageCatalog::safe(Some(source));
    let mut meta = MetaState::default();
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for (name, value) in properties {
        // NameEquals compares the unescaped name: a name with a lone surrogate never equals "_meta".
        if !name.bad && name.text == META_KEY {
            read_meta(value, &src, warnings, &mut meta)?;
            continue;
        }
        let name = name.read()?;
        if name.starts_with('_') {
            continue;
        }
        let JsonValue::String(text) = value else {
            warnings.push(format!(
                "{src}: '{}' is not a string, ignored",
                LanguageCatalog::safe(Some(name))
            ));
            continue;
        };
        let text = text.read()?.to_owned();
        match index.get(name) {
            Some(&i) => {
                warnings.push(format!(
                    "{src}: duplicate key '{}', the last value is used",
                    LanguageCatalog::safe(Some(name))
                ));
                entries[i].1 = text;
            }
            None => {
                index.insert(name.to_owned(), entries.len());
                entries.push((name.to_owned(), text));
            }
        }
    }

    let code = match meta.code {
        Some(code) if !code.trim().is_empty() && LanguageCatalog::is_valid_code(&code) => code,
        _ => {
            warnings.push(format!("{src}: _meta.code is missing or invalid"));
            return Ok(None);
        }
    };
    let code = code.to_ascii_lowercase();
    let native_name_declared = meta.native_name.is_some() || meta.name.is_some();
    let name = meta.name.unwrap_or_else(|| code.clone());
    let native_name = meta.native_name.unwrap_or_else(|| name.clone());
    Ok(Some(LanguageCatalog {
        code,
        name,
        native_name,
        plural: meta.plural,
        native_name_declared,
        plural_declared: meta.plural_declared,
        authors: meta.authors,
        entries,
        index,
    }))
}

/// C# `ReadMeta`: known keys only when the value has the right JSON type, everything else is silently ignored.
fn read_meta(meta: &JsonValue, src: &str, warnings: &mut Vec<String>, state: &mut MetaState) -> Result<(), String> {
    let JsonValue::Object(properties) = meta else {
        warnings.push(format!("{src}: _meta must be an object"));
        return Ok(());
    };
    for (name, value) in properties {
        match (name.read()?, value) {
            ("code", JsonValue::String(s)) => state.code = Some(s.read()?.to_owned()),
            ("name", JsonValue::String(s)) => state.name = clean_display_name(s.read()?),
            ("nativeName", JsonValue::String(s)) => state.native_name = clean_display_name(s.read()?),
            ("plural", JsonValue::String(s)) => {
                let rule = s.read()?;
                state.plural_declared = true;
                if eq_ignore_case(rule, "none") {
                    state.plural = PluralRule::None;
                } else {
                    state.plural = PluralRule::OneOther;
                    if !eq_ignore_case(rule, "one-other") {
                        warnings.push(format!(
                            "{src}: unknown _meta.plural '{}', using one-other (valid: one-other, none)",
                            LanguageCatalog::safe(Some(rule))
                        ));
                    }
                }
            }
            ("authors", JsonValue::Array(items)) => {
                for item in items {
                    if let JsonValue::String(s) = item {
                        state.authors.push(s.read()?.to_owned());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
