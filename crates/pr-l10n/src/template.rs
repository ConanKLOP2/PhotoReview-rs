//! Port of `PhotoReview.Core/Localization/{LocTemplate,LocArg,UserFacingError,TranslatorModes}.cs`
//! (C# baseline e441d67a).
//!
//! * [`LocTemplate`]: a translation string parsed once into literal and `{name}` placeholder segments
//!   (`{{`/`}}` are literal braces). Rendering never fails: a placeholder without a usable argument is written back
//!   as `{name}`.
//! * [`LocArg`] / [`LocValue`]: a named value for a placeholder. C# passes `object?` and formats it with
//!   `CultureInfo.CurrentCulture`; Rust passes an explicit [`NumberCulture`] to [`LocTemplate::render`] instead of an
//!   ambient culture (docs/adr/R0003-concurrency.md: no hidden global state).
//! * [`LocalizedError`] and the free functions [`localized_io`], [`localize_io`], [`is_localized`], [`describe`]:
//!   the Rust form of `UserFacingError` (a localized UI sentence attached to an error without changing its type).
//! * Translator aids from `TranslatorModes`: [`accent`], [`pseudo_padding`], [`pseudo_template`],
//!   [`show_key_template`]. The two `Localizer`-level entry points (`CreatePseudo`, `CreateShowKeys`) belong to the
//!   Localizer work unit and are built on these.

use std::error::Error;
use std::fmt;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

// ---------------------------------------------------------------------------------------------------------------
// Culture
// ---------------------------------------------------------------------------------------------------------------

/// The parts of a .NET `NumberFormatInfo` that `ToString(null, culture)` of an integer or a floating-point number
/// uses (general "G" format). Built by the caller (production: from the Windows user locale; tests: from the facts
/// stored in the golden file). `Default` is the invariant culture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberCulture {
    /// `NumberFormatInfo.NegativeSign` (`"-"`, U+2212 in sv-SE/nb-NO, with RTL marks in ar/fa/he). May be several chars.
    pub negative_sign: String,
    /// `NumberFormatInfo.PositiveSign`, used for the exponent of scientific notation (`E+17`).
    pub positive_sign: String,
    /// `NumberFormatInfo.NumberDecimalSeparator`.
    pub decimal_separator: String,
    /// `NumberFormatInfo.NaNSymbol`.
    pub nan_symbol: String,
    /// `NumberFormatInfo.PositiveInfinitySymbol` (invariant: `"Infinity"`, most cultures: `"∞"`).
    pub positive_infinity_symbol: String,
    /// `NumberFormatInfo.NegativeInfinitySymbol` (invariant: `"-Infinity"`).
    pub negative_infinity_symbol: String,
}

impl NumberCulture {
    /// `CultureInfo.InvariantCulture`.
    pub fn invariant() -> Self {
        Self {
            negative_sign: "-".to_owned(),
            positive_sign: "+".to_owned(),
            decimal_separator: ".".to_owned(),
            nan_symbol: "NaN".to_owned(),
            positive_infinity_symbol: "Infinity".to_owned(),
            negative_infinity_symbol: "-Infinity".to_owned(),
        }
    }
}

impl Default for NumberCulture {
    fn default() -> Self {
        Self::invariant()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------------------------------------------

/// Formatter closure of [`LocValue::Lazy`]: gets the culture, returns the text or `None` when it "threw".
pub type LazyFormatter = Arc<dyn Fn(&NumberCulture) -> Option<String> + Send + Sync>;

/// A value for a placeholder (the closed set of C# `object?` values the app passes; see contract.md for the
/// mapping). Anything else (dates, enums, `decimal`) is pre-formatted by the caller into [`LocValue::Str`].
#[derive(Clone)]
pub enum LocValue {
    /// C# `null`: renders as the empty string.
    Null,
    /// C# `string`: inserted verbatim.
    Str(String),
    /// Any signed C# integer (`int`, `long`, `short`, ...): sign (culture negative sign) + ASCII digits.
    Int(i64),
    /// Any unsigned C# integer (`uint`, `ulong`, `byte`, ...).
    UInt(u64),
    /// C# `double`: "G" shortest round-trip format in the given culture.
    F64(f64),
    /// C# `float`: "G" shortest round-trip format (single precision digits) in the given culture.
    F32(f32),
    /// C# `bool`: `"True"` / `"False"` (not culture dependent).
    Bool(bool),
    /// C# `char`: the character.
    Char(char),
    /// An argument whose own formatting code can fail (C# object whose `ToString`/`IFormattable.ToString` throws,
    /// R21). `None` (or a panic inside the closure) means "it threw": the placeholder is written back as `{name}`.
    /// `Some(text)` is inserted verbatim. The closure may itself render another template (nested render).
    Lazy(LazyFormatter),
}

impl fmt::Debug for LocValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("Null"),
            Self::Str(s) => f.debug_tuple("Str").field(s).finish(),
            Self::Int(v) => f.debug_tuple("Int").field(v).finish(),
            Self::UInt(v) => f.debug_tuple("UInt").field(v).finish(),
            Self::F64(v) => f.debug_tuple("F64").field(v).finish(),
            Self::F32(v) => f.debug_tuple("F32").field(v).finish(),
            Self::Bool(v) => f.debug_tuple("Bool").field(v).finish(),
            Self::Char(v) => f.debug_tuple("Char").field(v).finish(),
            Self::Lazy(_) => f.write_str("Lazy(..)"),
        }
    }
}

impl From<&str> for LocValue {
    fn from(v: &str) -> Self {
        Self::Str(v.to_owned())
    }
}
impl From<String> for LocValue {
    fn from(v: String) -> Self {
        Self::Str(v)
    }
}
impl From<i32> for LocValue {
    fn from(v: i32) -> Self {
        Self::Int(i64::from(v))
    }
}
impl From<i64> for LocValue {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}
impl From<u32> for LocValue {
    fn from(v: u32) -> Self {
        Self::UInt(u64::from(v))
    }
}
impl From<u64> for LocValue {
    fn from(v: u64) -> Self {
        Self::UInt(v)
    }
}
impl From<f64> for LocValue {
    fn from(v: f64) -> Self {
        Self::F64(v)
    }
}
impl From<f32> for LocValue {
    fn from(v: f32) -> Self {
        Self::F32(v)
    }
}
impl From<bool> for LocValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl From<char> for LocValue {
    fn from(v: char) -> Self {
        Self::Char(v)
    }
}

/// C# `readonly record struct LocArg(string Name, object? Value)`: one named value for a `{name}` placeholder.
#[derive(Debug, Clone)]
pub struct LocArg {
    /// Placeholder name; matched case-sensitively (ordinal) against the template's names.
    pub name: String,
    pub value: LocValue,
}

impl LocArg {
    pub fn new(name: impl Into<String>, value: impl Into<LocValue>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// C# `LocTemplate.FormatValue` + `TryFormatValue`: the text one argument contributes, or `None` when its
/// formatter failed (the caller then writes the placeholder back).
///
/// `Null` -> `""`; `Str` verbatim; `Int`/`UInt`/`F64`/`F32` as `ToString(null, culture)`; `Bool` -> `"True"`/`"False"`;
/// `Char` -> the char; `Lazy` -> its closure's result (a panic inside it is caught and gives `None`).
pub fn format_value(value: &LocValue, culture: &NumberCulture) -> Option<String> {
    Some(match value {
        LocValue::Null => String::new(),
        LocValue::Str(s) => s.clone(),
        LocValue::Int(v) => {
            let mut out = String::new();
            if *v < 0 {
                out.push_str(&culture.negative_sign);
            }
            out.push_str(&v.unsigned_abs().to_string());
            out
        }
        LocValue::UInt(v) => v.to_string(),
        LocValue::F64(v) => format_f64(*v, culture),
        LocValue::F32(v) => format_f32(*v, culture),
        LocValue::Bool(b) => String::from(if *b { "True" } else { "False" }),
        LocValue::Char(c) => c.to_string(),
        LocValue::Lazy(formatter) => {
            let call: &dyn Fn(&NumberCulture) -> Option<String> = &**formatter;
            // R21: a formatter that panics counts as one that failed.
            return catch_unwind(AssertUnwindSafe(|| call(culture))).ok().flatten();
        }
    })
}

/// .NET `double.ToString(null, culture)` (general format, shortest round-trip digits). See contract.md for the
/// exact layout rules (scientific from decimal exponent >= 17 or <= -5, `E+17` / `E-05`, `-0`, NaN, infinities).
pub fn format_f64(value: f64, culture: &NumberCulture) -> String {
    if value.is_nan() {
        return culture.nan_symbol.clone();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            culture.positive_infinity_symbol.clone()
        } else {
            culture.negative_infinity_symbol.clone()
        };
    }
    let (digits, exponent) = shortest_digits(&format!("{:e}", value.abs()));
    general_format(
        value.is_sign_negative(),
        &digits,
        exponent,
        F64_SCIENTIFIC_EXPONENT,
        culture,
    )
}

/// .NET `float.ToString(null, culture)`: same as [`format_f64`] but with single-precision shortest digits and
/// scientific notation from decimal exponent >= 9 (or <= -5).
pub fn format_f32(value: f32, culture: &NumberCulture) -> String {
    if value.is_nan() {
        return culture.nan_symbol.clone();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            culture.positive_infinity_symbol.clone()
        } else {
            culture.negative_infinity_symbol.clone()
        };
    }
    // Format the f32 itself so the shortest digits are the single-precision ones.
    let (digits, exponent) = shortest_digits(&format!("{:e}", value.abs()));
    general_format(
        value.is_sign_negative(),
        &digits,
        exponent,
        F32_SCIENTIFIC_EXPONENT,
        culture,
    )
}

/// Decimal exponent from which `double` uses scientific notation (.NET "G" default for double).
const F64_SCIENTIFIC_EXPONENT: i32 = 17;
/// Decimal exponent from which `float` uses scientific notation (.NET "G" default for single).
const F32_SCIENTIFIC_EXPONENT: i32 = 9;

/// Splits Rust's `{:e}` output (`d.ddde<exp>`, shortest round-trip digits of a non-negative finite number) into the
/// significant digits (no trailing zeros, at least one digit) and the decimal exponent.
fn shortest_digits(exp_form: &str) -> (String, i32) {
    let (mantissa, exponent) = exp_form.split_once('e').unwrap_or((exp_form, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let trimmed = digits.trim_end_matches('0');
    if trimmed.is_empty() {
        // Zero: `{:e}` gives "0e0", the digit string of zero is "0" with exponent 0.
        return ("0".to_owned(), 0);
    }
    (trimmed.to_owned(), exponent)
}

/// Lays out shortest digits `digits` (value `d[0].d[1..] x 10^exponent`) like .NET "G": sign, then fixed notation
/// unless `exponent >= scientific_from` or `exponent <= -5`.
fn general_format(
    negative: bool,
    digits: &str,
    exponent: i32,
    scientific_from: i32,
    culture: &NumberCulture,
) -> String {
    let mut out = String::new();
    if negative {
        out.push_str(&culture.negative_sign);
    }
    if exponent >= scientific_from || exponent <= -5 {
        // Scientific: d[.ddd] E sign exponent (at least two digits).
        if let Some(first) = digits.get(..1) {
            out.push_str(first);
        }
        if digits.len() > 1 {
            out.push_str(&culture.decimal_separator);
            out.push_str(&digits[1..]);
        }
        out.push('E');
        if exponent >= 0 {
            out.push_str(&culture.positive_sign);
        } else {
            out.push_str(&culture.negative_sign);
        }
        out.push_str(&format!("{:02}", exponent.unsigned_abs()));
    } else {
        let scale = exponent + 1;
        if scale <= 0 {
            out.push('0');
            out.push_str(&culture.decimal_separator);
            for _ in 0..scale.unsigned_abs() {
                out.push('0');
            }
            out.push_str(digits);
        } else {
            let scale = usize::try_from(scale).unwrap_or(0);
            if digits.len() <= scale {
                out.push_str(digits);
                for _ in digits.len()..scale {
                    out.push('0');
                }
            } else {
                out.push_str(&digits[..scale]);
                out.push_str(&culture.decimal_separator);
                out.push_str(&digits[scale..]);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// LocTemplate
// ---------------------------------------------------------------------------------------------------------------

/// A translation string parsed once. Invariant: `literals.len() == names.len() + 1`.
///
/// Fields are private; the porter chooses the representation (suggested: exactly the C# fields `text`, `literals`,
/// `names`). Equality compares all three.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocTemplate {
    text: String,
    literals: Vec<String>,
    names: Vec<String>,
}

impl LocTemplate {
    /// C# `LocTemplate.TryParse(string, out LocTemplate?)`. `None` for an unbalanced brace or an invalid
    /// placeholder name (the two causes are NOT distinguished, as in C#).
    pub fn try_parse(text: &str) -> Option<LocTemplate> {
        let mut literals: Vec<String> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut rest = 0_usize;
        while rest < text.len() {
            // Copy the run of plain text up to the next brace in one slice (braces are ASCII, so the cut is a char
            // boundary and multi-byte text is copied intact).
            let next = text[rest..].find(['{', '}']).map_or(text.len(), |p| rest + p);
            current.push_str(&text[rest..next]);
            rest = next;
            let Some(brace) = text[rest..].chars().next() else {
                break;
            };
            if brace == '{' {
                if text[rest + 1..].starts_with('{') {
                    current.push('{');
                    rest += 2;
                    continue;
                }
                // C# `text.IndexOf('}', i + 1)`: the FIRST closing brace after this one decides the name.
                let close = text[rest + 1..].find('}').map(|p| rest + 1 + p)?;
                let name = &text[rest + 1..close];
                if !is_valid_placeholder_name(name) {
                    return None;
                }
                literals.push(std::mem::take(&mut current));
                names.push(name.to_owned());
                rest = close + 1;
            } else {
                // A lone `}` (not followed by another `}`) is invalid.
                if text[rest + 1..].starts_with('}') {
                    current.push('}');
                    rest += 2;
                    continue;
                }
                return None;
            }
        }
        literals.push(current);
        Some(LocTemplate {
            text: text.to_owned(),
            literals,
            names,
        })
    }

    /// C# `LocTemplate.Literal`: text shown verbatim (no placeholders); `text()` returns `text` unchanged, even if it
    /// contains braces.
    pub fn literal(text: &str) -> LocTemplate {
        LocTemplate {
            text: text.to_owned(),
            literals: vec![text.to_owned()],
            names: Vec::new(),
        }
    }

    /// C# `Text`: the original, unparsed translation text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// C# `PlaceholderNames`: names in order of appearance (may repeat).
    pub fn placeholder_names(&self) -> &[String] {
        &self.names
    }

    /// C# `TransformLiterals`: same placeholders, each literal segment replaced by `literal(segment)`.
    /// `literal` is called once per segment, in order, first segment first (it may keep state: `FnMut`).
    pub fn transform_literals(&self, mut literal: impl FnMut(&str) -> String) -> LocTemplate {
        let mut literals: Vec<String> = Vec::with_capacity(self.literals.len());
        for segment in &self.literals {
            literals.push(literal(segment));
        }
        let mut text = escape_braces(literals.first().map_or("", String::as_str));
        for (index, name) in self.names.iter().enumerate() {
            text.push('{');
            text.push_str(name);
            text.push('}');
            if let Some(segment) = literals.get(index + 1) {
                text.push_str(&escape_braces(segment));
            }
        }
        LocTemplate {
            text,
            literals,
            names: self.names.clone(),
        }
    }

    /// C# `AppendLiteral`: same template with `suffix` appended to the last literal segment.
    pub fn append_literal(&self, suffix: &str) -> LocTemplate {
        let mut literals = self.literals.clone();
        match literals.last_mut() {
            Some(last) => last.push_str(suffix),
            None => literals.push(suffix.to_owned()),
        }
        LocTemplate {
            text: format!("{}{}", self.text, escape_braces(suffix)),
            literals,
            names: self.names.clone(),
        }
    }

    /// C# `Render(ReadOnlySpan<LocArg>)`, never fails. `culture` replaces C# `CultureInfo.CurrentCulture`.
    pub fn render(&self, args: &[LocArg], culture: &NumberCulture) -> String {
        let mut out = String::new();
        for (index, name) in self.names.iter().enumerate() {
            if let Some(segment) = self.literals.get(index) {
                out.push_str(segment);
            }
            // The FIRST argument with this exact name decides. A failed formatter does not fall through to a later
            // duplicate (C# `found = true; break;`).
            match args.iter().find(|arg| arg.name == *name) {
                Some(arg) => match format_value(&arg.value, culture) {
                    Some(text) => out.push_str(&text),
                    None => push_placeholder(&mut out, name),
                },
                None => push_placeholder(&mut out, name),
            }
        }
        if let Some(last) = self.literals.get(self.names.len()) {
            out.push_str(last);
        }
        out
    }
}

/// Doubles every brace (C# `Replace("{", "{{").Replace("}", "}}")`).
fn escape_braces(text: &str) -> String {
    text.replace('{', "{{").replace('}', "}}")
}

/// Writes `{name}` back (an unresolved placeholder).
fn push_placeholder(out: &mut String, name: &str) {
    out.push('{');
    out.push_str(name);
    out.push('}');
}

/// C# `LocTemplate.IsValidName`: non-empty, first char an ASCII letter, every char an ASCII letter or digit.
pub fn is_valid_placeholder_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => chars.all(|c| c.is_ascii_alphanumeric()),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// UserFacingError
// ---------------------------------------------------------------------------------------------------------------

/// The localized sentence attached to an error (C# `Func<string>` stored in `Exception.Data`). Evaluated every time
/// the error is displayed, so it follows the language current at display time.
pub type LocalizedText = Arc<dyn Fn() -> String + Send + Sync>;

/// Error payload that carries a [`LocalizedText`] (the Rust stand-in for `exception.Data["PhotoReview.UserFacingText"]`).
/// Its `Display` is the English `message` (for logs); the UI shows [`describe`]. Put it inside an [`io::Error`]
/// with [`localized_io`] / [`localize_io`] so callers keep matching on `io::ErrorKind`.
pub struct LocalizedError {
    message: String,
    source: Option<Box<dyn Error + Send + Sync>>,
    text: LocalizedText,
}

impl LocalizedError {
    /// A new payload with English `message` and no inner error.
    pub fn new(message: impl Into<String>, text: LocalizedText) -> Self {
        Self {
            message: message.into(),
            source: None,
            text,
        }
    }

    /// The localized sentence factory.
    pub fn text(&self) -> &LocalizedText {
        &self.text
    }
}

impl fmt::Debug for LocalizedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalizedError")
            .field("message", &self.message)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for LocalizedError {
    /// The English message (C# `Exception.Message`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for LocalizedError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|e| e as &(dyn Error + 'static))
    }
}

/// C# `UserFacingError.Localized(new IOException(english), text)` (also for `EndOfStreamException`,
/// `InvalidDataException`, `NotSupportedException`, `FileNotFoundException`: pick the matching `kind`).
/// The returned error has `kind()` = `kind` and `to_string()` = `english`.
pub fn localized_io(
    kind: io::ErrorKind,
    english: impl Into<String>,
    text: impl Fn() -> String + Send + Sync + 'static,
) -> io::Error {
    let payload = LocalizedError::new(english, Arc::new(text) as LocalizedText);
    io::Error::new(kind, payload)
}

/// C# `UserFacingError.Localized(existingException, text)`: attach a sentence to an error that already exists.
/// The result keeps `err.kind()` and `err.to_string()` (the original error becomes the `source` of the payload).
/// Idempotent: an error that is already localized is returned unchanged (the C# call sites guard with
/// `!IsLocalized(ex)`; this makes the guard unnecessary).
pub fn localize_io(err: io::Error, text: impl Fn() -> String + Send + Sync + 'static) -> io::Error {
    if is_localized(&err) {
        return err;
    }
    let kind = err.kind();
    let payload = LocalizedError {
        message: err.to_string(),
        source: Some(Box::new(err)),
        text: Arc::new(text) as LocalizedText,
    };
    io::Error::new(kind, payload)
}

/// The [`LocalizedError`] payload of `err` itself: `err` is one, or an io::Error whose inner error is one.
fn localized_payload<'a>(err: &'a (dyn Error + 'static)) -> Option<&'a LocalizedError> {
    if let Some(payload) = err.downcast_ref::<LocalizedError>() {
        return Some(payload);
    }
    err.downcast_ref::<io::Error>()
        .and_then(io::Error::get_ref)
        .and_then(|inner| inner.downcast_ref::<LocalizedError>())
}

/// C# `UserFacingError.IsLocalized`: true when `err` itself (an io::Error holding a [`LocalizedError`], or a
/// [`LocalizedError`]) carries a sentence. The `source()` chain is NOT searched (C# only looks at the exception itself).
pub fn is_localized(err: &(dyn Error + 'static)) -> bool {
    localized_payload(err).is_some()
}

/// C# `UserFacingError.Describe`: the localized sentence when attached, otherwise the error's own message
/// (`err.to_string()`; for an `io::Error` from the OS with `raw_os_error() == Some(code)`, without the trailing
/// `" (os error {code})"` that Rust appends, so it equals the .NET `Exception.Message`). When evaluating the
/// sentence panics, the same message fallback is returned (C#: any exception except OutOfMemory is swallowed).
pub fn describe(err: &(dyn Error + 'static)) -> String {
    let Some(payload) = localized_payload(err) else {
        return fallback_message(err);
    };
    let sentence: &(dyn Fn() -> String + Send + Sync) = &**payload.text();
    match catch_unwind(AssertUnwindSafe(sentence)) {
        Ok(text) => text,
        Err(_) => fallback_message(err),
    }
}

/// The English message of `err` as .NET `Exception.Message` would show it (see [`describe`]).
fn fallback_message(err: &(dyn Error + 'static)) -> String {
    if let Some(io_err) = err.downcast_ref::<io::Error>() {
        if let Some(code) = io_err.raw_os_error() {
            return strip_os_suffix(io_err.to_string(), code);
        }
        if let Some(payload) = io_err
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<LocalizedError>())
        {
            return payload_message(payload);
        }
        return io_err.to_string();
    }
    if let Some(payload) = err.downcast_ref::<LocalizedError>() {
        return payload_message(payload);
    }
    err.to_string()
}

/// The English message of a payload; an OS error stored as its source loses the Rust `(os error N)` suffix.
fn payload_message(payload: &LocalizedError) -> String {
    let os_code = payload
        .source
        .as_deref()
        .and_then(|source| source.downcast_ref::<io::Error>())
        .and_then(io::Error::raw_os_error);
    match os_code {
        Some(code) => strip_os_suffix(payload.message.clone(), code),
        None => payload.message.clone(),
    }
}

/// Removes the exact `" (os error {code})"` suffix Rust appends to OS error text.
fn strip_os_suffix(message: String, code: i32) -> String {
    match message.strip_suffix(&format!(" (os error {code})")) {
        Some(stripped) => stripped.to_owned(),
        None => message,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// TranslatorModes
// ---------------------------------------------------------------------------------------------------------------

/// C# `TranslatorModes.PseudoCode`.
pub const PSEUDO_CODE: &str = "qps-ploc";

/// C# `TranslatorModes.Accent`: maps each ASCII letter to its accented look-alike (table in contract.md), everything
/// else unchanged. One output char per input char.
pub fn accent(text: &str) -> String {
    text.chars()
        .map(|c| match ascii_letter_index(c) {
            Some(index) => char::from_u32(ACCENT_CODE_POINTS[index]).unwrap_or(c),
            None => c,
        })
        .collect()
}

/// Index of an ASCII letter in `abc...zABC...Z` (the order of [`ACCENT_CODE_POINTS`]).
fn ascii_letter_index(c: char) -> Option<usize> {
    match c {
        'a'..='z' => Some(usize::from(c as u8 - b'a')),
        'A'..='Z' => Some(26 + usize::from(c as u8 - b'A')),
        _ => None,
    }
}

/// Accented look-alikes of `abc...zABC...Z`, as code points (the C# source has no accented letters; keep it so).
const ACCENT_CODE_POINTS: [u32; 52] = [
    0x00E1, 0x0180, 0x0107, 0x0111, 0x00E9, 0x0192, 0x011D, 0x0125, 0x00ED, 0x0135, 0x0137, 0x013A, 0x1E3F, //
    0x0144, 0x00F3, 0x1E55, 0x02A0, 0x0155, 0x015B, 0x0165, 0x00FA, 0x1E7D, 0x0175, 0x1E8B, 0x00FD, 0x017A, //
    0x00C1, 0x0181, 0x0106, 0x0110, 0x00C9, 0x0191, 0x011C, 0x0124, 0x00CD, 0x0134, 0x0136, 0x0139, 0x1E3E, //
    0x0143, 0x00D3, 0x1E54, 0x024A, 0x0154, 0x015A, 0x0164, 0x00DA, 0x1E7C, 0x0174, 0x1E8A, 0x00DD, 0x0179,
];

/// C# `TranslatorModes.Padding(int length)`, where `length` is the UTF-16 length of the ORIGINAL template text
/// (`t.Text.Length`): `""` for 0, otherwise `" "` followed by `max(1, ceil(length as f64 * 0.35))` tildes.
pub fn pseudo_padding(utf16_length: usize) -> String {
    if utf16_length == 0 {
        return String::new();
    }
    // Keep the f64 expression: integer arithmetic differs where `n * 0.35` is just above an integer.
    let tildes = (utf16_length as f64 * 0.35).ceil() as usize;
    format!(" {}", "~".repeat(tildes.max(1)))
}

/// The per-template transform of C# `TranslatorModes.CreatePseudo`: accent every literal segment, put `[` before the
/// first segment, append `pseudo_padding(t.Text.Length) + "]"`; placeholders are kept.
pub fn pseudo_template(t: &LocTemplate) -> LocTemplate {
    let mut first = true;
    let accented = t.transform_literals(|literal| {
        let text = accent(literal);
        if first {
            first = false;
            format!("[{text}")
        } else {
            text
        }
    });
    let utf16_length = t.text().encode_utf16().count();
    accented.append_literal(&format!("{}]", pseudo_padding(utf16_length)))
}

/// The per-template transform of C# `TranslatorModes.CreateShowKeys`: `LocTemplate.Literal("[" + key + "]")`.
pub fn show_key_template(key: &str) -> LocTemplate {
    LocTemplate::literal(&format!("[{key}]"))
}
