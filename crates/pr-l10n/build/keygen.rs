//! Port of `PhotoReview.Localization.Generator` (C# baseline d92d6aae): `TrGenerator.cs` (456 lines),
//! `FlatJsonReader.cs` (345), `LocDiagnostics.cs` (39). Reads the English catalog (`assets/languages/en.json`) and
//! plans one constant per key (module `keys`) and one typed accessor per key or plural group (module `tr`), so a
//! wrong key name or a wrong number of arguments is a compile error (docs/adr ADR 0006 point 4).
//!
//! This file has NO dependencies besides `std` because it is compiled twice: by `build.rs` (`mod keygen;` through
//! `#[path]`) and by `tests/key_generator.rs` (`#[path = "../build/keygen.rs"] mod keygen;`). Keep it free of `crate::`
//! paths. Unit W2-04. Contract: `contracts/W2-04.md`.

use std::collections::{HashMap, HashSet};

/// One `"key": "text"` pair of a flat catalog, with the key's position in the file.
/// C# `JsonEntry`. Offsets are BYTE offsets into the text (C# uses UTF-16 offsets; only diagnostics use them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonEntry {
    pub key: String,
    pub value: String,
    /// Byte offset of the key's opening quote.
    pub key_start: usize,
    /// Byte length of the key token including its quotes.
    pub key_length: usize,
}

/// C# `FlatJsonException`: the catalog is not valid JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatJsonError {
    pub message: String,
    /// Byte offset of the problem.
    pub position: usize,
}

/// Maximum nesting skipped inside an underscore key (C# `FlatJsonReader.MaxDepth`; C# compares `depth > 64` with the
/// depth starting at 0 for the value of the key, so 65 nested arrays are still accepted: keep it).
pub const MAX_DEPTH: usize = 64;

/// C# `FlatJsonReader.Read`: a flat JSON object of string values. Keys starting with `_` may hold any JSON value and
/// are skipped. Accepts `//` and `/* */` comments and trailing commas like the runtime parser; a leading U+FEFF is
/// skipped; an unpaired surrogate escape in a key or value is an error (`RejectUnpairedSurrogate`).
pub fn read_flat_json(text: &str) -> Result<Vec<JsonEntry>, FlatJsonError> {
    FlatJsonReader {
        text,
        bytes: text.as_bytes(),
        pos: 0,
    }
    .read_root()
}

/// Port of the private `FlatJsonReader` class. Works on UTF-8 bytes; every structural character is ASCII, so a byte
/// scan sees the same structure as the C# UTF-16 scan.
struct FlatJsonReader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

type ReadResult<T> = Result<T, FlatJsonError>;

fn error_at(message: impl Into<String>, position: usize) -> FlatJsonError {
    FlatJsonError {
        message: message.into(),
        position,
    }
}

/// A decoded JSON string. `lone_surrogate`: an unpaired surrogate escape was seen (C# keeps it in the string; a Rust
/// `String` cannot, so the text holds U+FFFD there).
struct FlatString {
    text: String,
    lone_surrogate: bool,
}

impl FlatString {
    fn from_units(units: &[u16]) -> Self {
        let mut text = String::with_capacity(units.len());
        let mut lone_surrogate = false;
        for decoded in char::decode_utf16(units.iter().copied()) {
            match decoded {
                Ok(c) => text.push(c),
                Err(_) => {
                    lone_surrogate = true;
                    text.push('\u{FFFD}');
                }
            }
        }
        Self { text, lone_surrogate }
    }
}

/// An unpaired surrogate (a `\ud800` escape) is valid JSON text but System.Text.Json cannot turn it into a string, so
/// the runtime rejects the catalog; only the strings the runtime reads (keys and catalog values) are checked.
fn reject_unpaired_surrogate(value: &FlatString, position: usize) -> ReadResult<()> {
    if value.lone_surrogate {
        return Err(error_at("unpaired surrogate in string", position));
    }
    Ok(())
}

impl FlatJsonReader<'_> {
    fn error(&self, message: impl Into<String>) -> FlatJsonError {
        error_at(message, self.pos)
    }

    /// C# `Peek()`: the current byte, `0` (NUL) at the end of the text.
    fn peek(&self) -> u8 {
        self.bytes.get(self.pos).copied().unwrap_or(0)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn try_consume(&mut self, c: u8) -> bool {
        if self.at_end() || self.bytes[self.pos] != c {
            return false;
        }
        self.pos += 1;
        true
    }

    fn expect(&mut self, c: u8) -> ReadResult<()> {
        if self.at_end() {
            return Err(self.error(format!("unexpected end of file, expected '{}'", c as char)));
        }
        if self.bytes[self.pos] != c {
            return Err(self.error(format!("expected '{}'", c as char)));
        }
        self.pos += 1;
        Ok(())
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }

    fn skip_trivia(&mut self) -> ReadResult<()> {
        while self.pos < self.bytes.len() {
            let c = self.bytes[self.pos];
            let next = self.bytes.get(self.pos + 1).copied();
            if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
                self.pos += 1;
            } else if c == b'/' && next == Some(b'/') {
                // System.Text.Json ends a // comment at LF, CR, U+2028 or U+2029 (the terminator stays for the next loop).
                while self.pos < self.bytes.len() {
                    let rest = &self.bytes[self.pos..];
                    if rest[0] == b'\n'
                        || rest[0] == b'\r'
                        || rest.starts_with("\u{2028}".as_bytes())
                        || rest.starts_with("\u{2029}".as_bytes())
                    {
                        break;
                    }
                    self.pos += 1;
                }
            } else if c == b'/' && next == Some(b'*') {
                match self.text[self.pos + 2..].find("*/") {
                    Some(end) => self.pos = self.pos + 2 + end + 2,
                    None => return Err(self.error("unterminated comment")),
                }
            } else {
                return Ok(());
            }
        }
        Ok(())
    }

    fn read_root(&mut self) -> ReadResult<Vec<JsonEntry>> {
        let mut entries = Vec::new();
        if self.text.starts_with('\u{FEFF}') {
            self.pos = '\u{FEFF}'.len_utf8();
        }
        self.skip_trivia()?;
        self.expect(b'{')?;
        self.skip_trivia()?;
        if self.try_consume(b'}') {
            self.ensure_end()?;
            return Ok(entries);
        }
        loop {
            self.skip_trivia()?;
            if self.try_consume(b'}') {
                break; // trailing comma
            }
            let key_start = self.pos;
            let key = self.read_string()?;
            let key_length = self.pos - key_start;
            reject_unpaired_surrogate(&key, key_start)?;
            self.skip_whitespace(); // System.Text.Json allows no comment between a name and its ':'
            self.expect(b':')?;
            self.skip_trivia()?;
            if key.text.starts_with('_') {
                self.skip_value(0)?;
            } else {
                if self.peek() != b'"' || self.at_end() {
                    return Err(self.error(format!("value of '{}' must be a string", key.text)));
                }
                let value_start = self.pos;
                let value = self.read_string()?;
                reject_unpaired_surrogate(&value, value_start)?;
                entries.push(JsonEntry {
                    key: key.text,
                    value: value.text,
                    key_start,
                    key_length,
                });
            }
            self.skip_trivia()?;
            if self.try_consume(b',') {
                continue;
            }
            if self.try_consume(b'}') {
                break;
            }
            return Err(self.error("expected ',' or '}'"));
        }
        self.ensure_end()?;
        Ok(entries)
    }

    fn ensure_end(&mut self) -> ReadResult<()> {
        self.skip_trivia()?;
        if !self.at_end() {
            return Err(self.error("unexpected content after the root object"));
        }
        Ok(())
    }

    fn skip_value(&mut self, depth: usize) -> ReadResult<()> {
        if depth > MAX_DEPTH {
            return Err(self.error("nesting too deep"));
        }
        if self.at_end() {
            return Err(self.error("expected a JSON value"));
        }
        match self.bytes[self.pos] {
            b'"' => {
                self.read_string()?;
                Ok(())
            }
            b'{' => {
                self.pos += 1;
                self.skip_container(b'}', depth, true)
            }
            b'[' => {
                self.pos += 1;
                self.skip_container(b']', depth, false)
            }
            b't' => self.expect_word("true"),
            b'f' => self.expect_word("false"),
            b'n' => self.expect_word("null"),
            b'-' | b'0'..=b'9' => self.skip_number(),
            _ => Err(self.error("expected a JSON value")),
        }
    }

    fn skip_container(&mut self, close: u8, depth: usize, is_object: bool) -> ReadResult<()> {
        self.skip_trivia()?;
        if self.try_consume(close) {
            return Ok(());
        }
        loop {
            self.skip_trivia()?;
            if self.try_consume(close) {
                return Ok(()); // trailing comma
            }
            if is_object {
                self.read_string()?;
                self.skip_whitespace();
                self.expect(b':')?;
                self.skip_trivia()?;
            }
            self.skip_value(depth + 1)?;
            self.skip_trivia()?;
            if self.try_consume(b',') {
                continue;
            }
            if self.try_consume(close) {
                return Ok(());
            }
            return Err(self.error(format!("expected ',' or '{}'", close as char)));
        }
    }

    fn skip_number(&mut self) -> ReadResult<()> {
        let start = self.pos;
        if self.peek() == b'-' {
            self.pos += 1;
        }
        let mut digits = 0;
        let leading_zero = self.peek() == b'0';
        while self.peek().is_ascii_digit() {
            self.pos += 1;
            digits += 1;
        }
        if digits == 0 || (leading_zero && digits > 1) {
            return Err(error_at("invalid number", start)); // JSON forbids leading zeros
        }
        if self.peek() == b'.' {
            self.pos += 1;
            if !self.peek().is_ascii_digit() {
                return Err(error_at("invalid number", start));
            }
            while self.peek().is_ascii_digit() {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), b'e' | b'E') {
            self.pos += 1;
            if matches!(self.peek(), b'+' | b'-') {
                self.pos += 1;
            }
            if !self.peek().is_ascii_digit() {
                return Err(error_at("invalid number", start));
            }
            while self.peek().is_ascii_digit() {
                self.pos += 1;
            }
        }
        Ok(())
    }

    fn expect_word(&mut self, word: &str) -> ReadResult<()> {
        if !self.bytes[self.pos..].starts_with(word.as_bytes()) {
            return Err(self.error("expected a JSON value"));
        }
        self.pos += word.len();
        Ok(())
    }

    fn read_string(&mut self) -> ReadResult<FlatString> {
        let start = self.pos;
        self.expect(b'"')?;
        let mut units: Vec<u16> = Vec::new();
        loop {
            if self.at_end() {
                return Err(error_at("unterminated string", start));
            }
            let b = self.bytes[self.pos];
            if b == b'"' {
                self.pos += 1;
                return Ok(FlatString::from_units(&units));
            }
            if b < b' ' {
                return Err(error_at("control character in string (escape it)", self.pos));
            }
            if b != b'\\' {
                // Copy one whole char (non-ASCII bytes never match a structural byte).
                let Some(ch) = self.text[self.pos..].chars().next() else {
                    return Err(error_at("unterminated string", start));
                };
                let mut buf = [0_u16; 2];
                units.extend_from_slice(ch.encode_utf16(&mut buf));
                self.pos += ch.len_utf8();
                continue;
            }
            self.pos += 1; // the backslash
            if self.at_end() {
                return Err(error_at("unterminated string", start));
            }
            let Some(e) = self.text[self.pos..].chars().next() else {
                return Err(error_at("unterminated string", start));
            };
            let escape_start = self.pos - 1;
            self.pos += e.len_utf8();
            match e {
                '"' => units.push(u16::from(b'"')),
                '\\' => units.push(u16::from(b'\\')),
                '/' => units.push(u16::from(b'/')),
                'b' => units.push(0x08),
                'f' => units.push(0x0C),
                'n' => units.push(0x0A),
                'r' => units.push(0x0D),
                't' => units.push(0x09),
                'u' => {
                    // Exactly four hex digits: int.TryParse would also accept a trailing NUL, System.Text.Json does not.
                    let mut code: u16 = 0;
                    for k in 0..4 {
                        let digit = self.bytes.get(self.pos + k).and_then(|&h| char::from(h).to_digit(16));
                        let Some(digit) = digit else {
                            return Err(error_at("invalid \\u escape", escape_start));
                        };
                        code = code * 16 + digit as u16;
                    }
                    units.push(code);
                    self.pos += 4;
                }
                other => {
                    return Err(error_at(format!("invalid escape '\\{other}'"), escape_start));
                }
            }
        }
    }
}

/// C# `LocDiagnostics` ids. All are build errors.
pub const PRLOC001_INVALID_JSON: &str = "PRLOC001";
pub const PRLOC002_INVALID_KEY: &str = "PRLOC002";
pub const PRLOC003_IDENTIFIER_COLLISION: &str = "PRLOC003";
pub const PRLOC004_INVALID_PLACEHOLDER: &str = "PRLOC004";
pub const PRLOC005_PLURAL_WITHOUT_OTHER: &str = "PRLOC005";
pub const PRLOC006_DUPLICATE_KEY: &str = "PRLOC006";
pub const PRLOC007_MULTIPLE_CATALOGS: &str = "PRLOC007";

/// One build error. `message` is the C# message text (`LocDiagnostics` format strings with the arguments filled in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocDiagnostic {
    pub id: &'static str,
    pub message: String,
}

/// An English catalog handed to the generator (C# `AdditionalText` named `en.json`).
#[derive(Debug, Clone)]
pub struct CatalogInput {
    /// Full path, used for ordering (ordinal) and in the PRLOC007 message.
    pub path: String,
    /// `None` = the file could not be read.
    pub content: Option<String>,
}

/// A `TrKeys` constant: C# identifier (PascalCase) and the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstEntry {
    /// The C# identifier from `ToIdentifier` (collision detection and golden files use this form).
    pub ident: String,
    pub key: String,
}

/// A `Tr` member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberKind {
    /// One key. `key` is the full catalog key.
    Simple { key: String },
    /// A `base.one` / `base.other` group. `base` is the key without the suffix.
    Plural { base: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberEntry {
    /// The C# identifier from `ToIdentifier`.
    pub ident: String,
    pub kind: MemberKind,
    /// Placeholder names in order of first appearance, deduplicated; for a plural group: `other`'s then `one`'s,
    /// without `count`.
    pub params: Vec<String>,
    /// English text(s) for the doc comment: `[other]` or `[one, other]` for a group, `[text]` otherwise.
    pub english: Vec<String>,
}

/// Result of analysing the English catalog (what C# `Execute` computes before it renders).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Diagnostics in the order C# reports them.
    pub diagnostics: Vec<LocDiagnostic>,
    /// `false` when C# adds no `Tr.g.cs` (no `en.json` at all, unreadable file, invalid JSON).
    pub generated: bool,
    pub consts: Vec<ConstEntry>,
    pub members: Vec<MemberEntry>,
}

const ONE_SUFFIX: &str = ".one";
const OTHER_SUFFIX: &str = ".other";
const COUNT_NAME: &str = "count";

/// Members a static class inherits from `object` in C# (the golden is keyed by the C# identifiers; keep the list).
const RESERVED: [&str; 9] = [
    "Tr",
    "TrKeys",
    "Equals",
    "ReferenceEquals",
    "GetHashCode",
    "GetType",
    "ToString",
    "MemberwiseClone",
    "Finalize",
];

/// Rust-only: an identifier whose snake_case form cannot be a (raw) function name.
const RUST_UNNAMEABLE: [&str; 3] = ["Self", "Crate", "Super"];

/// Strict, reserved and edition 2018/2024 keywords (`Self` included).
const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
    "trait", "true", "type", "unsafe", "use", "where", "while", "async", "await", "dyn", "abstract", "become", "box",
    "do", "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen",
];

struct KeyInfo {
    key: String,
    text: String,
    ident: String,
    placeholders: Vec<String>,
    is_valid_text: bool,
}

fn utf16_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn diag(id: &'static str, message: String) -> LocDiagnostic {
    LocDiagnostic { id, message }
}

/// C# `TryClaim`: first owner wins; reserved names are never free.
fn try_claim(
    diagnostics: &mut Vec<LocDiagnostic>,
    owners: &mut HashMap<String, String>,
    ident: &str,
    key: &str,
    type_name: &str,
) -> bool {
    if let Some(owner) = owners.get(ident) {
        diagnostics.push(diag(
            PRLOC003_IDENTIFIER_COLLISION,
            format!("Key '{key}' generates identifier '{type_name}.{ident}', which is already used by '{owner}'"),
        ));
        return false;
    }
    let reserved = RESERVED.contains(&ident) || (type_name == "Tr" && RUST_UNNAMEABLE.contains(&ident));
    if reserved {
        diagnostics.push(diag(
            PRLOC003_IDENTIFIER_COLLISION,
            format!(
                "Key '{key}' generates identifier '{type_name}.{ident}', which is already used by a built-in member"
            ),
        ));
        return false;
    }
    owners.insert(ident.to_owned(), key.to_owned());
    true
}

fn plural_base(key: &str) -> Option<&str> {
    for suffix in [ONE_SUFFIX, OTHER_SUFFIX] {
        if key.len() > suffix.len() && key.ends_with(suffix) {
            return Some(&key[..key.len() - suffix.len()]);
        }
    }
    None
}

/// C# `TrGenerator.Initialize` + `Execute` analysis: picks the first catalog by ordinal path (the others get
/// PRLOC007), reads it, validates keys, claims identifiers, groups plurals. Only files whose name equals `en.json`
/// ignoring case are considered: filter BEFORE calling (the generator does `Path.GetFileName(f.Path)`).
pub fn plan(inputs: &[CatalogInput]) -> Plan {
    let mut plan = Plan::default();
    if inputs.is_empty() {
        return plan;
    }
    // RV-P07: one Tr per compilation; the first by path order is used, every other one is reported.
    let mut ordered: Vec<&CatalogInput> = inputs.iter().collect();
    ordered.sort_by(|a, b| utf16_cmp(&a.path, &b.path));
    for extra in &ordered[1..] {
        plan.diagnostics.push(diag(
            PRLOC007_MULTIPLE_CATALOGS,
            format!(
                "'{}' is an extra en.json: only '{}' (first by path) is used to generate Tr",
                extra.path, ordered[0].path
            ),
        ));
    }
    let input = ordered[0];
    let Some(content) = &input.content else {
        plan.diagnostics.push(diag(
            PRLOC001_INVALID_JSON,
            "en.json is not a valid flat catalog: file cannot be read".to_owned(),
        ));
        return plan;
    };
    let entries = match read_flat_json(content) {
        Ok(entries) => entries,
        Err(error) => {
            plan.diagnostics.push(diag(
                PRLOC001_INVALID_JSON,
                format!("en.json is not a valid flat catalog: {}", error.message),
            ));
            return plan;
        }
    };

    // 1. Valid, unique keys (first occurrence wins; the runtime would take the last, so a duplicate is an error).
    let mut keys: Vec<KeyInfo> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for entry in entries {
        if !is_valid_key(&entry.key) {
            plan.diagnostics.push(diag(
                PRLOC002_INVALID_KEY,
                format!(
                    "Key '{}' is invalid: use ASCII letters, digits and '.', start with a letter, no empty segment",
                    entry.key
                ),
            ));
            continue;
        }
        if by_key.contains_key(&entry.key) {
            plan.diagnostics.push(diag(
                PRLOC006_DUPLICATE_KEY,
                format!("Key '{}' appears more than once", entry.key),
            ));
            continue;
        }
        let placeholders = try_get_placeholders(&entry.value);
        if placeholders.is_none() {
            plan.diagnostics.push(diag(
                PRLOC004_INVALID_PLACEHOLDER,
                format!(
                    "Text of '{}' has unbalanced braces or an invalid placeholder (use {{name}}, and {{{{ }}}} for literal braces)",
                    entry.key
                ),
            ));
        }
        by_key.insert(entry.key.clone(), keys.len());
        keys.push(KeyInfo {
            ident: to_identifier(&entry.key),
            is_valid_text: placeholders.is_some(),
            placeholders: placeholders.unwrap_or_default(),
            key: entry.key,
            text: entry.value,
        });
    }

    // 2. TrKeys: one const per key.
    let mut const_owners: HashMap<String, String> = HashMap::new();
    for key in &keys {
        if try_claim(&mut plan.diagnostics, &mut const_owners, &key.ident, &key.key, "TrKeys") {
            plan.consts.push(ConstEntry {
                ident: key.ident.clone(),
                key: key.key.clone(),
            });
        }
    }

    // 3. Tr: plural groups (base.one / base.other) become one method; every other key one member.
    let mut member_owners: HashMap<String, String> = HashMap::new();
    let mut plural_done: HashSet<&str> = HashSet::new();
    for key in &keys {
        let Some(base) = plural_base(&key.key) else {
            if !key.is_valid_text {
                continue;
            }
            if try_claim(&mut plan.diagnostics, &mut member_owners, &key.ident, &key.key, "Tr") {
                plan.members.push(MemberEntry {
                    ident: key.ident.clone(),
                    kind: MemberKind::Simple { key: key.key.clone() },
                    params: key.placeholders.clone(),
                    english: vec![key.text.clone()],
                });
            }
            continue;
        };
        if !plural_done.insert(base) {
            continue;
        }
        let one = by_key.get(&format!("{base}{ONE_SUFFIX}")).map(|&i| &keys[i]);
        let other = by_key.get(&format!("{base}{OTHER_SUFFIX}")).map(|&i| &keys[i]);
        let Some(other) = other else {
            plan.diagnostics.push(diag(
                PRLOC005_PLURAL_WITHOUT_OTHER,
                format!("Plural group '{base}' has '{base}.one' but no '{base}.other'"),
            ));
            continue;
        };
        if one.is_some_and(|o| !o.is_valid_text) || !other.is_valid_text {
            continue;
        }
        let mut params: Vec<String> = Vec::new();
        for name in other
            .placeholders
            .iter()
            .chain(one.iter().flat_map(|o| o.placeholders.iter()))
        {
            if name != COUNT_NAME && !params.contains(name) {
                params.push(name.clone());
            }
        }
        let ident = to_identifier(base);
        if try_claim(&mut plan.diagnostics, &mut member_owners, &ident, base, "Tr") {
            let english = match one {
                Some(one) => vec![one.text.clone(), other.text.clone()],
                None => vec![other.text.clone()],
            };
            plan.members.push(MemberEntry {
                ident,
                kind: MemberKind::Plural { base: base.to_owned() },
                params,
                english,
            });
        }
    }
    plan.generated = true;
    plan
}

fn is_ascii_letter(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// C# `IsValidKey`: ASCII letters/digits/'.'; first char a letter; no '.' at the end; no empty segment.
pub fn is_valid_key(key: &str) -> bool {
    let chars: Vec<char> = key.chars().collect();
    if chars.is_empty() || !is_ascii_letter(chars[0]) || chars[chars.len() - 1] == '.' {
        return false;
    }
    for (i, &c) in chars.iter().enumerate() {
        if c == '.' {
            if chars[i - 1] == '.' {
                return false;
            }
            let next = chars[i + 1];
            if !is_ascii_letter(next) && !next.is_ascii_digit() {
                return false;
            }
        } else if !is_ascii_letter(c) && !c.is_ascii_digit() {
            return false;
        }
    }
    true
}

/// C# `ToIdentifier`: split on '.', drop empty segments, upper-case each segment's first char (`char.ToUpperInvariant`
/// on an ASCII letter or digit), keep the rest as is. `"status.batchDone"` -> `"StatusBatchDone"`.
pub fn to_identifier(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for segment in key.split('.') {
        let mut chars = segment.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

fn is_valid_placeholder_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if is_ascii_letter(first) => chars.all(|c| c.is_ascii_alphanumeric()),
        _ => false,
    }
}

/// C# `TryGetPlaceholders`: names in order of first appearance, deduplicated; `None` when the text is invalid
/// (same grammar as `pr_l10n::template::LocTemplate::try_parse`).
pub fn try_get_placeholders(text: &str) -> Option<Vec<String>> {
    let mut names: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                if bytes.get(i + 1) == Some(&b'{') {
                    i += 2;
                    continue;
                }
                let end = i + 1 + text[i + 1..].find('}')?;
                let name = &text[i + 1..end];
                if !is_valid_placeholder_name(name) {
                    return None;
                }
                if seen.insert(name) {
                    names.push(name.to_owned());
                }
                i = end + 1;
            }
            b'}' => {
                if bytes.get(i + 1) == Some(&b'}') {
                    i += 2;
                    continue;
                }
                return None;
            }
            _ => i += 1,
        }
    }
    Some(names)
}

/// Splits a PascalCase identifier into snake_case words: an `_` before every upper-case letter except the first char.
fn snake_case(ident: &str) -> String {
    let mut out = String::with_capacity(ident.len() + 4);
    for (i, c) in ident.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn is_rust_keyword(name: &str) -> bool {
    RUST_KEYWORDS.contains(&name)
}

/// Rust name of a `keys` constant: SCREAMING_SNAKE of the C# identifier (`StatusBatchDone` -> `STATUS_BATCH_DONE`,
/// an `_` before every upper-case letter except the first char). Injective on identifiers.
pub fn const_name(ident: &str) -> String {
    snake_case(ident).to_ascii_uppercase()
}

/// Rust name of a `tr` function: snake_case of the C# identifier by the same rule (`status_batch_done`). A Rust
/// keyword becomes a raw identifier (`r#type`); `self`/`crate`/`super` cannot be raw identifiers: [`plan`] reports
/// PRLOC003 ("a built-in member") for them like it does for the C# reserved names.
pub fn fn_name(ident: &str) -> String {
    let name = snake_case(ident);
    if matches!(name.as_str(), "self" | "crate" | "super") || !is_rust_keyword(&name) {
        name
    } else {
        format!("r#{name}")
    }
}

/// Rust parameter name for a placeholder: the placeholder verbatim, `r#`-escaped when it is a Rust keyword, and
/// `name_` for `self`/`Self`/`crate`/`super`.
pub fn param_name(placeholder: &str) -> String {
    if matches!(placeholder, "self" | "Self" | "crate" | "super") {
        format!("{placeholder}_")
    } else if is_rust_keyword(placeholder) {
        format!("r#{placeholder}")
    } else {
        placeholder.to_owned()
    }
}

/// Source of module `keys` (`pub const STATUS_BATCH_DONE: &str = "status.batchDone";` per constant, ending with a
/// newline) written to `$OUT_DIR/tr_keys.rs` and included as `pr_l10n::keys`.
pub fn render_keys(plan: &Plan) -> String {
    let mut out = String::new();
    out.push_str("// <auto-generated/> by pr-l10n build.rs from assets/languages/en.json. Do not edit.\n");
    for entry in &plan.consts {
        out.push_str(&format!(
            "pub const {}: &str = {};\n",
            const_name(&entry.ident),
            rust_literal(&entry.key)
        ));
    }
    out
}

/// A Rust string literal for ASCII-or-not text (escapes `\`, `"` and control characters).
fn rust_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The doc-comment form of English text (C# `XmlEscape` minus the XML escapes): `\n` shown as the two characters
/// backslash and `n`, `\r` dropped, other control characters and U+0085/U+2028/U+2029 replaced by a space.
fn doc_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\r' => {}
            '\n' => out.push_str("\\n"),
            c if c < ' ' || matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}') => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Source of module `tr` (`pub fn status_batch_done(count: impl Into<LocValue>, ...) -> String`, plural groups as
/// `pub fn test_items(count: i64, ...)`) written to `$OUT_DIR/tr_fns.rs` and included as `pr_l10n::tr`.
pub fn render_tr(plan: &Plan) -> String {
    let mut out = String::new();
    out.push_str("// <auto-generated/> by pr-l10n build.rs from assets/languages/en.json. Do not edit.\n");
    out.push_str("// Allowed: an empty catalog generates no member that uses these imports.\n");
    out.push_str("#[allow(unused_imports)]\n");
    out.push_str("use crate::localizer::Localizer;\n");
    out.push_str("#[allow(unused_imports)]\n");
    out.push_str("use crate::template::{LocArg, LocValue};\n");
    for member in &plan.members {
        out.push('\n');
        let fn_ident = fn_name(&member.ident);
        match &member.kind {
            MemberKind::Simple { key } => {
                let english = member.english.first().map_or("", String::as_str);
                out.push_str(&format!("/// English: \"{}\"\n", doc_text(english)));
                let const_path = format!("crate::keys::{}", const_name(&to_identifier(key)));
                if member.params.is_empty() {
                    out.push_str(&format!(
                        "pub fn {fn_ident}() -> String {{\n    Localizer::current().get({const_path})\n}}\n"
                    ));
                } else {
                    let params: Vec<String> = member
                        .params
                        .iter()
                        .map(|p| format!("{}: impl Into<LocValue>", param_name(p)))
                        .collect();
                    let args: Vec<String> = member
                        .params
                        .iter()
                        .map(|p| format!("LocArg::new({}, {})", rust_literal(p), param_name(p)))
                        .collect();
                    out.push_str(&format!(
                        "pub fn {fn_ident}({}) -> String {{\n    Localizer::current().format({const_path}, &[{}])\n}}\n",
                        params.join(", "),
                        args.join(", ")
                    ));
                }
            }
            MemberKind::Plural { base } => {
                let summary = match member.english.as_slice() {
                    [one, other] => format!("English: \"{}\" / \"{}\"", doc_text(one), doc_text(other)),
                    [other] => format!("English: \"{}\"", doc_text(other)),
                    _ => String::new(),
                };
                out.push_str(&format!("/// {summary}\n"));
                let mut params = vec!["count: i64".to_owned()];
                let mut args = vec![format!("LocArg::new({}, count)", rust_literal(COUNT_NAME))];
                for p in &member.params {
                    params.push(format!("{}: impl Into<LocValue>", param_name(p)));
                    args.push(format!("LocArg::new({}, {})", rust_literal(p), param_name(p)));
                }
                out.push_str(&format!(
                    "pub fn {fn_ident}({}) -> String {{\n    Localizer::current().format_plural({}, count, &[{}])\n}}\n",
                    params.join(", "),
                    rust_literal(base),
                    args.join(", ")
                ));
            }
        }
    }
    out
}
