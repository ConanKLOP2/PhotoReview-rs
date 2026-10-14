//! Lenient JSON handling of the persisted enums.
//!
//! Port of `PhotoReview.Core/Model/LenientEnumConverter.cs`, `FlagEnumJson.cs`, `SettingsEnumConverter.cs` and
//! `JsonAliasAttribute.cs` (baseline e441d67a). Contract: `contracts/W1-02/contract.md`.
//!
//! C# registers a `System.Text.Json` converter per enum type. Rust has no attribute-driven converters, so:
//!
//! * [`LenientEnum`] is implemented once per plain enum by `model::enums` (name, value, aliases, written text). The
//!   `[JsonAlias]` attribute becomes [`LenientEnum::aliases`].
//! * [`deserialize_lenient`] / [`serialize_lenient`] are what the generated `serde` impls of those enums call.
//! * [`settings_enum`] is the `SettingsEnumConverter<T>` flavour, used per field with `#[serde(with = "...")]`.
//! * [`FlagEnum`] is implemented by `model::fields` for the two `[Flags]` types; [`deserialize_flags`] /
//!   [`serialize_flags`] / [`format_flags`] are `FlagEnumJson`.
//!
//! Nothing here may panic or return an error for bad *content*: every unreadable value maps to the documented
//! fallback. Errors can only come from the underlying deserializer (e.g. malformed JSON, or a lone surrogate escape).

// Stub: bodies are `todo!()`. The porter deletes this line together with the `todo!()`s.
#![allow(unused_variables)]

use serde::{Deserializer, Serializer};

/// A plain (non-flags) enum that is persisted leniently. Implemented by `model::enums` (never by hand elsewhere).
///
/// C# equivalent: any `T : struct, Enum` used with `LenientEnumConverter<T>`.
pub trait LenientEnum: Copy + Eq + std::fmt::Debug + 'static {
    /// C# type name, for diagnostics only (for example `"LoadingMode"`).
    const TYPE_NAME: &'static str;

    /// Every defined member in declaration order (what `typeof(T).GetFields(Public | Static)` returns).
    const MEMBERS: &'static [Self];

    /// The C# identifier of the member, for example `"Percent100"`.
    fn name(self) -> &'static str;

    /// The numeric value (`(int)member`).
    fn value(self) -> i32;

    /// The `[JsonAlias(...)]` strings of the member, in attribute order (empty for most members).
    fn aliases(self) -> &'static [&'static str];

    /// The text written to JSON (`LenientEnumConverter<T>.ToText`). Defaults to [`LenientEnum::name`]; only
    /// `InitialViewMode` overrides it (`Fit`, `100%`, `200%`, `400%`; the other members write their name).
    fn json_text(self) -> &'static str {
        self.name()
    }

    /// C# `default(T)`: the member whose value is 0.
    fn zero() -> Self;
}

/// `LenientEnumConverter<T>.Read` for a string token and for a dictionary key (`ParseText`), without the default.
///
/// Returns `None` when C# would return its `_defaultValue`: `text` is `None`, empty or all whitespace, or it matches
/// no name/alias/number. Rules, in order:
/// 1. blank (`None`, `""`, or only `char::is_whitespace` characters) gives `None`;
/// 2. `trim()` the text (Unicode whitespace, same set as .NET `Trim()`);
/// 3. look it up among the member names and aliases, compared with OrdinalIgnoreCase
///    (`pr_core::text::eq_ignore_case`); aliases are matched after trimming the alias itself;
/// 4. otherwise parse it as an `int` (see the contract: optional sign, ASCII digits, trailing whitespace/NULs) and
///    accept it only when some member has that value.
pub fn parse_lenient_text<T: LenientEnum>(text: Option<&str>) -> Option<T> {
    todo!()
}

/// `LenientEnumConverter<T>.Read` for a JSON number token that is an integer: `Some` only when `n` fits an `i32`
/// and some member has that value (`Enum.IsDefined`). Used by the visitor and available to hand-written parsers
/// (for example the journal line parser).
pub fn lenient_from_number<T: LenientEnum>(n: i64) -> Option<T> {
    todo!()
}

/// `LenientEnumConverter<T>.ToText`: the text written for `value` (also used for dictionary keys).
pub fn lenient_text<T: LenientEnum>(value: T) -> &'static str {
    todo!()
}

/// `LenientEnumConverter<T>.Write` / `WriteAsPropertyName`: writes [`lenient_text`] as a JSON string.
pub fn serialize_lenient<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: LenientEnum,
    S: Serializer,
{
    todo!()
}

/// `new LenientEnumConverter<T>()` (the default is `default(T)`, i.e. [`LenientEnum::zero`]).
///
/// Never fails on content: strings go through [`parse_lenient_text`]; integers through [`lenient_from_number`];
/// `null`, booleans and floating-point numbers give the default; arrays and objects are consumed and give the default.
pub fn deserialize_lenient<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: LenientEnum,
    D: Deserializer<'de>,
{
    todo!()
}

/// `new LenientEnumConverter<T>(defaultValue)`: like [`deserialize_lenient`] with a caller-chosen fallback.
pub fn deserialize_lenient_or<'de, T, D>(deserializer: D, default: T) -> Result<T, D::Error>
where
    T: LenientEnum,
    D: Deserializer<'de>,
{
    todo!()
}

/// `SettingsEnumConverter<T>` / `JsonAliasAttribute`-aware read for settings properties whose default is not the zero
/// member. C# returns an undefined sentinel (`(T)int.MinValue`) for unparsable input; Rust cannot hold an undefined
/// enum, so the result is `None` instead, and the caller (the settings normalizer) resets the field to its default and
/// records the repair. `Some(member)` exactly when [`deserialize_lenient`] would have produced a real member from the
/// input (including the zero member for `0`, `"0"`, `"Name"`...).
pub fn deserialize_settings_enum<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: LenientEnum,
    D: Deserializer<'de>,
{
    todo!()
}

/// `SettingsEnumConverter<T>.Write`: `Some(member)` writes [`lenient_text`]; `None` (the sentinel) writes the string
/// `"-2147483648"`, which is what `value.ToString()` gives for `(T)int.MinValue`.
pub fn serialize_settings_enum<T, S>(value: &Option<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    T: LenientEnum,
    S: Serializer,
{
    todo!()
}

/// Adapter for `#[serde(with = "pr_core::model::serde_ext::settings_enum")]` on an `Option<T>` field.
pub mod settings_enum {
    use super::{LenientEnum, deserialize_settings_enum, serialize_settings_enum};
    use serde::{Deserializer, Serializer};

    /// See [`serialize_settings_enum`].
    pub fn serialize<T, S>(value: &Option<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: LenientEnum,
        S: Serializer,
    {
        serialize_settings_enum(value, serializer)
    }

    /// See [`deserialize_settings_enum`].
    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
    where
        T: LenientEnum,
        D: Deserializer<'de>,
    {
        deserialize_settings_enum(deserializer)
    }
}

/// A `[Flags]` enum persisted as a comma separated list of names (`FlagEnumJson`). Implemented by `model::fields`.
pub trait FlagEnum: Copy + 'static {
    /// C# type name, for diagnostics only.
    const TYPE_NAME: &'static str;

    /// Bit mask of every defined single flag (`All`). Unknown bits are always stripped against it.
    const ALL_BITS: u32;

    /// The `Default` alias value: the fallback for unreadable input.
    const DEFAULT_BITS: u32;

    /// Every defined name with its value, as `Enum.GetNames<T>()` returns them: `None`, the single flags in
    /// ascending bit order, then the aliases `All` and `Default`.
    const NAMES: &'static [(&'static str, u32)];

    /// `(T)Enum.ToObject(typeof(T), bits)` for bits already masked with [`FlagEnum::ALL_BITS`].
    fn from_bits(bits: u32) -> Self;

    /// The raw bits.
    fn bits(self) -> u32;
}

/// `FlagEnumJson.Format`: `"None"` when no known bit is set, otherwise the names of the set single flags in ascending
/// bit order joined with `", "`. Never writes the `None`, `All` or `Default` alias names; unknown bits are ignored.
pub fn format_flags<T: FlagEnum>(value: T) -> String {
    todo!()
}

/// `FlagEnumJson.ParseText` plus its blank check: `None` (also for blank text) gives `T::DEFAULT_BITS`.
///
/// Splits on `,`; every trimmed non-empty token that is a defined name (OrdinalIgnoreCase, including `None`, `All`,
/// `Default`) or an `int` number (see the contract) counts as valid unless it is negative. The valid values are OR-ed
/// and masked with `ALL_BITS`; when no token was valid the result is the default, even if the mask result is 0.
pub fn parse_flags_text<T: FlagEnum>(text: Option<&str>) -> T {
    todo!()
}

/// `FlagEnumJson.Read`: strings go through [`parse_flags_text`]; a non-negative integer that fits `i64` is masked with
/// `ALL_BITS` (an all-unknown-bits value gives `None`, not the default); a negative or out-of-range number, a
/// floating-point number, `null`, a boolean, an array or an object (consumed) gives `T::DEFAULT_BITS`.
pub fn deserialize_flags<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: FlagEnum,
    D: Deserializer<'de>,
{
    todo!()
}

/// `FlagEnumJson.Format` as a JSON string.
pub fn serialize_flags<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: FlagEnum,
    S: Serializer,
{
    todo!()
}
