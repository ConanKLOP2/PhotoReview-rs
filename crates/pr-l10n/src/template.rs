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
    let _ = (value, culture);
    todo!("port LocTemplate.FormatValue / TryFormatValue")
}

/// .NET `double.ToString(null, culture)` (general format, shortest round-trip digits). See contract.md for the
/// exact layout rules (scientific from decimal exponent >= 17 or <= -5, `E+17` / `E-05`, `-0`, NaN, infinities).
pub fn format_f64(value: f64, culture: &NumberCulture) -> String {
    let _ = (value, culture);
    todo!("port double.ToString(null, culture)")
}

/// .NET `float.ToString(null, culture)`: same as [`format_f64`] but with single-precision shortest digits and
/// scientific notation from decimal exponent >= 9 (or <= -5).
pub fn format_f32(value: f32, culture: &NumberCulture) -> String {
    let _ = (value, culture);
    todo!("port float.ToString(null, culture)")
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
    #[allow(dead_code)] // read by render/transform_literals once ported; remove this attribute then
    literals: Vec<String>,
    names: Vec<String>,
}

impl LocTemplate {
    /// C# `LocTemplate.TryParse(string, out LocTemplate?)`. `None` for an unbalanced brace or an invalid
    /// placeholder name (the two causes are NOT distinguished, as in C#).
    pub fn try_parse(text: &str) -> Option<LocTemplate> {
        let _ = text;
        todo!("port LocTemplate.TryParse")
    }

    /// C# `LocTemplate.Literal`: text shown verbatim (no placeholders); `text()` returns `text` unchanged, even if it
    /// contains braces.
    pub fn literal(text: &str) -> LocTemplate {
        let _ = text;
        todo!("port LocTemplate.Literal")
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
    pub fn transform_literals(&self, literal: impl FnMut(&str) -> String) -> LocTemplate {
        let _ = literal;
        todo!("port LocTemplate.TransformLiterals")
    }

    /// C# `AppendLiteral`: same template with `suffix` appended to the last literal segment.
    pub fn append_literal(&self, suffix: &str) -> LocTemplate {
        let _ = suffix;
        todo!("port LocTemplate.AppendLiteral")
    }

    /// C# `Render(ReadOnlySpan<LocArg>)`, never fails. `culture` replaces C# `CultureInfo.CurrentCulture`.
    pub fn render(&self, args: &[LocArg], culture: &NumberCulture) -> String {
        let _ = (args, culture);
        todo!("port LocTemplate.Render")
    }
}

/// C# `LocTemplate.IsValidName`: non-empty, first char an ASCII letter, every char an ASCII letter or digit.
pub fn is_valid_placeholder_name(name: &str) -> bool {
    let _ = name;
    todo!("port LocTemplate.IsValidName")
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
    let _: String = english.into();
    let _ = (kind, text);
    todo!("port UserFacingError.Localized for a new error")
}

/// C# `UserFacingError.Localized(existingException, text)`: attach a sentence to an error that already exists.
/// The result keeps `err.kind()` and `err.to_string()` (the original error becomes the `source` of the payload).
pub fn localize_io(err: io::Error, text: impl Fn() -> String + Send + Sync + 'static) -> io::Error {
    let _ = (err, text);
    todo!("port UserFacingError.Localized for an existing error")
}

/// C# `UserFacingError.IsLocalized`: true when `err` itself (an io::Error holding a [`LocalizedError`], or a
/// [`LocalizedError`]) carries a sentence. The `source()` chain is NOT searched (C# only looks at the exception itself).
pub fn is_localized(err: &(dyn Error + 'static)) -> bool {
    let _ = err;
    todo!("port UserFacingError.IsLocalized")
}

/// C# `UserFacingError.Describe`: the localized sentence when attached, otherwise the error's own message
/// (`err.to_string()`; for an `io::Error` from the OS with `raw_os_error() == Some(code)`, without the trailing
/// `" (os error {code})"` that Rust appends, so it equals the .NET `Exception.Message`). When evaluating the
/// sentence panics, the same message fallback is returned (C#: any exception except OutOfMemory is swallowed).
pub fn describe(err: &(dyn Error + 'static)) -> String {
    let _ = err;
    todo!("port UserFacingError.Describe")
}

// ---------------------------------------------------------------------------------------------------------------
// TranslatorModes
// ---------------------------------------------------------------------------------------------------------------

/// C# `TranslatorModes.PseudoCode`.
pub const PSEUDO_CODE: &str = "qps-ploc";

/// C# `TranslatorModes.Accent`: maps each ASCII letter to its accented look-alike (table in contract.md), everything
/// else unchanged. One output char per input char.
pub fn accent(text: &str) -> String {
    let _ = text;
    todo!("port TranslatorModes.Accent")
}

/// C# `TranslatorModes.Padding(int length)`, where `length` is the UTF-16 length of the ORIGINAL template text
/// (`t.Text.Length`): `""` for 0, otherwise `" "` followed by `max(1, ceil(length as f64 * 0.35))` tildes.
pub fn pseudo_padding(utf16_length: usize) -> String {
    let _ = utf16_length;
    todo!("port TranslatorModes.Padding")
}

/// The per-template transform of C# `TranslatorModes.CreatePseudo`: accent every literal segment, put `[` before the
/// first segment, append `pseudo_padding(t.Text.Length) + "]"`; placeholders are kept.
pub fn pseudo_template(t: &LocTemplate) -> LocTemplate {
    let _ = t;
    todo!("port the CreatePseudo transform")
}

/// The per-template transform of C# `TranslatorModes.CreateShowKeys`: `LocTemplate.Literal("[" + key + "]")`.
pub fn show_key_template(key: &str) -> LocTemplate {
    let _ = key;
    todo!("port the CreateShowKeys transform")
}
