//! Port of `PhotoReview.Core/FileActions/JournalLineParser.cs` plus the on-disk shape of the `JournalEntry` /
//! `JournalGroupMember` records it reads (C# baseline e441d67a, `OperationJournal.cs` and `JournalGroupMember.cs`).
//!
//! One journal line is one compact JSON object written by `System.Text.Json` with its default options. This module
//! reads such a line ([`try_parse`], [`try_parse_utf8`]: accept/reject rules identical to C#, including every legacy
//! shape the parser tolerates) and writes it back byte for byte ([`JournalEntry::to_line`]). The writer appends
//! `Environment.NewLine` (CRLF) itself; that terminator, the file handling and the journal logic are NOT in this unit.
//!
//! Dependencies the lead adds to `pr-core`: `serde` (derive) and `serde_json` with the `raw_value` feature as REGULAR
//! dependencies. `crate::model::{FileOperationType, JournalState}` come from W1-01.
//!
//! Deliberate differences from C# (no golden, see contract.md): a timestamp text with a UTC offset
//! (`2026-01-01T07:00:00+07:00`) is converted to UTC instead of becoming a machine-dependent `DateTimeKind.Local`;
//! invalid UTF-8 inside a property NAME rejects the line (C# tolerates it in an unknown name).

use std::fmt;
use std::io;

use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::ser::{CharEscape, Formatter};
use serde_json::value::RawValue;

use crate::model::serde_ext::LenientEnum;
use crate::text::eq_ignore_case;
use crate::time::{DateTimeUtc, SourceKind};

use crate::model::{FileOperationType, JournalState};

/// A C# `DateTime` as the journal stores it: the ticks plus whether the text carried a `Z` (`DateTimeKind.Utc`) or
/// no zone at all (`DateTimeKind.Unspecified`, which also is `default(DateTime)`).
///
/// The kind decides the text written back: Utc -> `2026-09-30T08:15:42.1234567Z`, Unspecified -> the same without the
/// `Z`. Equality and hashing compare the ticks only, like `DateTime ==` (the kind is ignored).
#[derive(Debug, Clone, Copy)]
pub struct JournalDateTime {
    /// The instant in .NET ticks (for an Unspecified text, the ticks of the text taken as they are).
    pub value: DateTimeUtc,
    /// `true` for `DateTimeKind.Unspecified`.
    pub unspecified: bool,
}

impl JournalDateTime {
    /// `DateTimeKind.Utc`.
    pub fn utc(value: DateTimeUtc) -> Self {
        Self {
            value,
            unspecified: false,
        }
    }

    /// `DateTimeKind.Unspecified`.
    pub fn unspecified(value: DateTimeUtc) -> Self {
        Self {
            value,
            unspecified: true,
        }
    }

    /// System.Text.Json text of this value: [`DateTimeUtc::to_json`], without the trailing `Z` when `unspecified`.
    pub fn to_json(self) -> String {
        let text = self.value.to_json();
        if self.unspecified && text.ends_with('Z') {
            text[..text.len() - 1].to_owned()
        } else {
            text
        }
    }

    /// System.Text.Json reading of a `DateTime` text via [`DateTimeUtc::parse_json`]: `Z` -> utc, no zone ->
    /// unspecified, `+hh:mm` offset -> converted to UTC, utc (deliberate difference). `None` = the C# reader throws.
    pub fn parse_json(text: &str) -> Option<Self> {
        if !stj_shape_ok(text.as_bytes()) {
            return None;
        }
        let (value, kind) = DateTimeUtc::parse_json(text)?;
        Some(match kind {
            SourceKind::Unspecified => Self::unspecified(value),
            SourceKind::Utc | SourceKind::Offset => Self::utc(value),
        })
    }
}

/// The parts of System.Text.Json's `DateTime` grammar (`JsonHelpers.TryParseAsISO`) that [`DateTimeUtc::parse_json`]
/// is more lenient about:
/// * a date-only text is exactly `yyyy-MM-dd`: anything longer needs `T` at index 10 (`2026-01-01Z` and
///   `2026-01-01+07:00` are rejected);
/// * the `.` fraction separator needs at least one more character after it (`…:42.` is rejected, while `…:42.Z` and
///   `…:42.+07:00`, a dot with no digits before the zone, are accepted);
/// * at most 16 fraction digits are read; a 17th digit is not a zone designator, so the text is rejected.
fn stj_shape_ok(b: &[u8]) -> bool {
    if b.len() > 10 && b.get(10) != Some(&b'T') {
        return false;
    }
    match b.iter().position(|&c| c == b'.') {
        None => true,
        Some(dot) => {
            let rest = b.get(dot + 1..).unwrap_or_default();
            !rest.is_empty() && rest.iter().take_while(|c| c.is_ascii_digit()).count() <= 16
        }
    }
}

impl Default for JournalDateTime {
    /// `default(DateTime)`: 0001-01-01T00:00:00, Unspecified. This is what a MISSING timestamp member becomes.
    fn default() -> Self {
        Self {
            value: DateTimeUtc::MIN,
            unspecified: true,
        }
    }
}

impl PartialEq for JournalDateTime {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for JournalDateTime {}

impl std::hash::Hash for JournalDateTime {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl Serialize for JournalDateTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_json())
    }
}

/// C# `JournalGroupMember`: one member's identity in a journaled capture-group operation. Property order on disk:
/// Source, Destination (written even when null), Size, LastWriteUtc, Permanent (always written).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct JournalGroupMember {
    pub source: String,
    pub destination: Option<String>,
    pub size: i64,
    pub last_write_utc: JournalDateTime,
    pub permanent: bool,
}

/// C# `JournalEntry`: one journal line. Property order on disk is the declaration order below. `Destination` and
/// `Error` are written even when null; the five trailing options are omitted when `None`.
///
/// Equality is structural (the C# record overrides `Equals` to compare `GroupMembers` element-wise, which derived
/// `PartialEq` on `Vec` does): `None` and `Some(vec![])` are different.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct JournalEntry {
    /// 32 hex digits when written by the app; the parser only requires it to be non-empty.
    pub id: String,
    /// C# `Type`.
    #[serde(rename = "Type", serialize_with = "serialize_operation_type")]
    pub op_type: FileOperationType,
    #[serde(serialize_with = "serialize_state")]
    pub state: JournalState,
    pub source: String,
    pub destination: Option<String>,
    pub size: i64,
    pub last_write_utc: JournalDateTime,
    pub timestamp_utc: JournalDateTime,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permanent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_members: Option<Vec<JournalGroupMember>>,
}

/// Enum text as `LenientEnumConverter<FileOperationType>.Write` emits it (`value.ToString()`).
fn serialize_operation_type<S: Serializer>(value: &FileOperationType, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(match value {
        FileOperationType::Move => "Move",
        FileOperationType::Copy => "Copy",
        FileOperationType::Recycle => "Recycle",
    })
}

/// Enum text as `LenientEnumConverter<JournalState>.Write` emits it.
fn serialize_state<S: Serializer>(value: &JournalState, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(match value {
        JournalState::Prepared => "Prepared",
        JournalState::Committed => "Committed",
        JournalState::Failed => "Failed",
        JournalState::Dismissed => "Dismissed",
    })
}

impl JournalEntry {
    /// C# `JsonSerializer.Serialize(entry)`: the compact JSON line WITHOUT a line terminator, byte-identical to C#
    /// (string escaping of the default `JavaScriptEncoder`, see [`DotNetJsonFormatter`]). Implement with
    /// `serde_json::Serializer::with_formatter(Vec<u8>, DotNetJsonFormatter)`; serializing into a `Vec` cannot fail.
    pub fn to_line(&self) -> String {
        let mut out = Vec::new();
        let written = {
            let mut ser = serde_json::Serializer::with_formatter(&mut out, DotNetJsonFormatter);
            Serialize::serialize(self, &mut ser).is_ok()
        };
        if !written {
            return String::new();
        }
        String::from_utf8(out).unwrap_or_default()
    }
}

/// Appends `unit` as `\uXXXX` with uppercase hex digits (the .NET spelling).
fn push_unit(out: &mut String, unit: u16) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push_str("\\u");
    for shift in [12u32, 8, 4, 0] {
        out.push(char::from(HEX[usize::from((unit >> shift) & 0xF)]));
    }
}

/// `serde_json` formatter that reproduces the string escaping of `System.Text.Json`'s default encoder:
/// * `"` -> `"`, `\` -> `\\`, backspace/form feed/newline/CR/tab -> `\b \f \n \r \t`, every other control char
///   `0x00..=0x1F` -> `\u00XX` (UPPERCASE hex);
/// * `& ' + < > `` ` `` and DEL (0x7F) -> `\u00XX` (uppercase hex);
/// * every non-ASCII char -> `\uXXXX` (uppercase hex) per UTF-16 unit (an astral char becomes a surrogate pair
///   `Ã°Å¸Ëœâ‚¬`);
/// * everything else (including `/`) is written as is. Numbers, `null`, braces, commas stay compact (default trait
///   methods).
#[derive(Debug, Clone, Copy, Default)]
pub struct DotNetJsonFormatter;

impl Formatter for DotNetJsonFormatter {
    /// Called for runs of characters `serde_json` itself does not escape: apply the `& ' + < > `` DEL` and
    /// non-ASCII rules here.
    fn write_string_fragment<W: ?Sized + io::Write>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()> {
        let mut out = String::with_capacity(fragment.len());
        for c in fragment.chars() {
            if c.is_ascii() {
                if matches!(c, '&' | '\'' | '+' | '<' | '>' | '`' | '\u{7F}') {
                    push_unit(&mut out, c as u16);
                } else {
                    out.push(c);
                }
            } else {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units).iter() {
                    push_unit(&mut out, *unit);
                }
            }
        }
        writer.write_all(out.as_bytes())
    }

    /// Called for `"`, `\` and the control characters `serde_json` escapes: emit the .NET spelling listed above.
    fn write_char_escape<W: ?Sized + io::Write>(&mut self, writer: &mut W, char_escape: CharEscape) -> io::Result<()> {
        let text: &str = match char_escape {
            CharEscape::Quote => "\\u0022",
            CharEscape::ReverseSolidus => "\\\\",
            CharEscape::Solidus => "/",
            CharEscape::Backspace => "\\b",
            CharEscape::FormFeed => "\\f",
            CharEscape::LineFeed => "\\n",
            CharEscape::CarriageReturn => "\\r",
            CharEscape::Tab => "\\t",
            CharEscape::AsciiControl(byte) => {
                let mut out = String::with_capacity(6);
                push_unit(&mut out, u16::from(byte));
                return writer.write_all(out.as_bytes());
            }
        };
        writer.write_all(text.as_bytes())
    }
}

/// C# `JournalLineParser.TryParse(string)`: the entry when `line` is accepted, `None` when it is rejected (malformed
/// JSON, wrong member types, unrecognised Type/State, empty Id, unusable Source or group member, ...). Never panics.
pub fn try_parse(line: &str) -> Option<JournalEntry> {
    try_parse_utf8(line.as_bytes())
}

/// C# `JournalLineParser.TryParse(ReadOnlySpan<byte>)`: same as [`try_parse`] for a UTF-8 line without terminator.
/// A UTF-8 BOM, NUL bytes, UTF-16 text and invalid UTF-8 inside a string value that is read are rejected.
pub fn try_parse_utf8(line: &[u8]) -> Option<JournalEntry> {
    if nests_too_deep(line) {
        return None;
    }
    let mut de = serde_json::Deserializer::from_slice(line);
    let acc = de.deserialize_map(EntryVisitor).ok()?;
    de.end().ok()?;
    acc.finish()
}

// ---------------------------------------------------------------------------------------------------------------
// Reader internals. Every leaf value is read as raw JSON text and decoded here (see "Why RawValue" in the contract):
// serde's derive would accept `-0`/`1.0` differently and could not express per-occurrence validation.
// ---------------------------------------------------------------------------------------------------------------

/// Maximum nesting of containers in a line (an object plus 63 nested arrays is the deepest accepted line).
const MAX_NESTING: usize = 64;

/// `true` when some container nests deeper than [`MAX_NESTING`]. Counts `{` and `[` outside strings; never panics.
fn nests_too_deep(line: &[u8]) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for &b in line {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else {
            match b {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > MAX_NESTING {
                        return true;
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    false
}

/// Any failure inside the visitor: the caller maps the whole line to `None`.
fn reject<E: de::Error>() -> E {
    E::custom("journal line rejected")
}

/// Reads one member value as raw text and decodes it. Decode failure rejects the line.
fn field<'de, A, T>(map: &mut A, decode: fn(&str) -> Option<T>) -> Result<T, A::Error>
where
    A: MapAccess<'de>,
{
    let raw = map.next_value::<Box<RawValue>>()?;
    decode(raw.get()).ok_or_else(reject::<A::Error>)
}

/// A JSON string, or `null` (= `None`). Outer `None`: any other JSON type.
fn decode_nullable_string(raw: &str) -> Option<Option<String>> {
    match raw.as_bytes().first()? {
        b'n' => Some(None),
        b'"' => serde_json::from_str::<String>(raw).ok().map(Some),
        _ => None,
    }
}

/// `true`, `false` or `null`. Outer `None`: any other JSON type.
fn decode_nullable_bool(raw: &str) -> Option<Option<bool>> {
    match raw.as_bytes().first()? {
        b'n' => Some(None),
        b't' => Some(Some(true)),
        b'f' => Some(Some(false)),
        _ => None,
    }
}

/// `true` or `false` only (a group member's `Permanent`).
fn decode_bool(raw: &str) -> Option<bool> {
    match raw.as_bytes().first()? {
        b't' => Some(true),
        b'f' => Some(false),
        _ => None,
    }
}

/// An integer JSON token: no `.`, `e` or `E`, and it fits i64. `-0` is 0.
fn integer_token(raw: &str) -> Option<i64> {
    match raw.as_bytes().first()? {
        b'-' | b'0'..=b'9' if !raw.bytes().any(|b| matches!(b, b'.' | b'e' | b'E')) => raw.parse::<i64>().ok(),
        _ => None,
    }
}

/// A timestamp: a JSON string that [`JournalDateTime::parse_json`] accepts.
fn decode_datetime(raw: &str) -> Option<JournalDateTime> {
    let text = serde_json::from_str::<String>(raw).ok()?;
    JournalDateTime::parse_json(&text)
}

/// Step 7 of the reader: `Some(Some(v))` recognized, `Some(None)` present but not recognized, `None` malformed.
fn recognize<T: LenientEnum>(raw: &str) -> Option<Option<T>> {
    match raw.as_bytes().first()? {
        b'"' => {
            let text = serde_json::from_str::<String>(raw).ok()?;
            if text.contains(',') {
                return Some(None);
            }
            Some(member_from_text::<T>(text.trim()))
        }
        b'-' | b'0'..=b'9' => Some(integer_token(raw).and_then(member_from_number::<T>)),
        _ => Some(None),
    }
}

/// Name or alias (ASCII case-insensitive), or a signed decimal number that is a defined value.
fn member_from_text<T: LenientEnum>(text: &str) -> Option<T> {
    let named = T::MEMBERS.iter().copied().find(|member| {
        eq_ignore_case(text, member.name()) || member.aliases().iter().any(|alias| eq_ignore_case(text, alias))
    });
    if named.is_some() {
        return named;
    }
    // .NET integer parsing (Enum.TryParse, int.TryParse with NumberStyles.Integer): optional sign, ASCII digits, then
    // optional ASCII white space (TAB, LF, VT, FF, CR, space) and then any number of trailing NUL chars, which .NET
    // ignores ("1\0" is 1, "1 \0" is 1, "1\0x" and "1\u{A0}\0" are not numbers).
    let unsigned = text
        .strip_prefix('+')
        .or_else(|| text.strip_prefix('-'))
        .unwrap_or(text);
    let digit_len = unsigned.bytes().take_while(u8::is_ascii_digit).count();
    let (digits, tail) = unsigned.split_at(digit_len);
    if digits.is_empty()
        || !tail
            .trim_start_matches(['\t', '\n', '\u{B}', '\u{C}', '\r', ' '])
            .chars()
            .all(|c| c == '\0')
    {
        return None;
    }
    let number = digits.parse::<i64>().ok()?;
    member_from_number::<T>(if text.starts_with('-') {
        number.checked_neg()?
    } else {
        number
    })
}

/// A defined member whose value is `n` (must fit i32).
fn member_from_number<T: LenientEnum>(n: i64) -> Option<T> {
    let n = i32::try_from(n).ok()?;
    T::MEMBERS.iter().copied().find(|member| member.value() == n)
}

/// `Source`, `Destination`, `Id`, `GroupMembers` text: not empty, not whitespace only, no NUL.
fn is_usable(text: &str) -> bool {
    !text.contains('\0') && !text.chars().all(char::is_whitespace)
}

/// The top-level members, as they were read. Duplicates overwrite the value (last one wins); a malformed value has
/// already rejected the line while reading.
#[derive(Default)]
struct EntryAcc {
    id: Option<String>,
    op_type: Option<FileOperationType>,
    state: Option<JournalState>,
    source: Option<String>,
    destination: Option<String>,
    size: i64,
    last_write: JournalDateTime,
    timestamp: JournalDateTime,
    error: Option<String>,
    error_code: Option<String>,
    permanent: Option<bool>,
    undo: Option<bool>,
    group_id: Option<String>,
    /// `None` for a missing or `null` `GroupMembers`; a `null` element is kept (rejected in [`EntryAcc::finish`]).
    group_members: Option<Vec<MemberSlot>>,
}

/// One group member object, or a `null` element.
enum MemberSlot {
    Null,
    Member(MemberAcc),
}

#[derive(Default)]
struct MemberAcc {
    source: Option<String>,
    destination: Option<String>,
    size: i64,
    last_write: JournalDateTime,
    permanent: bool,
}

impl EntryAcc {
    /// Step 8 (acceptance) and step 9 (destination normalisation).
    fn finish(self) -> Option<JournalEntry> {
        let group_members = match self.group_members {
            None => None,
            Some(slots) => Some(slots.into_iter().map(MemberSlot::finish).collect::<Option<Vec<_>>>()?),
        };
        let op_type = self.op_type?;
        let state = self.state?;
        let id = self.id.filter(|id| !id.is_empty())?;
        let source = self.source.filter(|source| is_usable(source))?;
        Some(JournalEntry {
            id,
            op_type,
            state,
            source,
            destination: self.destination.filter(|destination| is_usable(destination)),
            size: self.size,
            last_write_utc: self.last_write,
            timestamp_utc: self.timestamp,
            error: self.error,
            error_code: self.error_code,
            permanent: self.permanent,
            undo: self.undo,
            group_id: self.group_id,
            group_members,
        })
    }
}

impl MemberSlot {
    fn finish(self) -> Option<JournalGroupMember> {
        match self {
            Self::Null => None,
            Self::Member(member) => member.finish(),
        }
    }
}

impl MemberAcc {
    fn finish(self) -> Option<JournalGroupMember> {
        let source = self.source.filter(|source| is_usable(source))?;
        if self
            .destination
            .as_deref()
            .is_some_and(|destination| destination.contains('\0'))
        {
            return None;
        }
        Some(JournalGroupMember {
            source,
            destination: self.destination,
            size: self.size,
            last_write_utc: self.last_write,
            permanent: self.permanent,
        })
    }
}

/// Reads the members of the top-level object in file order.
fn read_entry<'de, A: MapAccess<'de>>(mut map: A) -> Result<EntryAcc, A::Error> {
    let mut acc = EntryAcc::default();
    while let Some(key) = map.next_key::<String>()? {
        match key.as_str() {
            "Id" => acc.id = field(&mut map, decode_nullable_string)?,
            "Type" => acc.op_type = field(&mut map, recognize::<FileOperationType>)?,
            "State" => acc.state = field(&mut map, recognize::<JournalState>)?,
            "Source" => acc.source = field(&mut map, decode_nullable_string)?,
            "Destination" => acc.destination = field(&mut map, decode_nullable_string)?,
            "Size" => acc.size = field(&mut map, integer_token)?,
            "LastWriteUtc" => acc.last_write = field(&mut map, decode_datetime)?,
            "TimestampUtc" => acc.timestamp = field(&mut map, decode_datetime)?,
            "Error" => acc.error = field(&mut map, decode_nullable_string)?,
            "ErrorCode" => acc.error_code = field(&mut map, decode_nullable_string)?,
            "Permanent" => acc.permanent = field(&mut map, decode_nullable_bool)?,
            "Undo" => acc.undo = field(&mut map, decode_nullable_bool)?,
            "GroupId" => acc.group_id = field(&mut map, decode_nullable_string)?,
            "GroupMembers" => acc.group_members = map.next_value::<GroupMembers>()?.0,
            _ => {
                map.next_value::<IgnoredAny>()?;
            }
        }
    }
    Ok(acc)
}

/// Reads the members of one group member object.
fn read_member<'de, A: MapAccess<'de>>(mut map: A) -> Result<MemberAcc, A::Error> {
    let mut acc = MemberAcc::default();
    while let Some(key) = map.next_key::<String>()? {
        match key.as_str() {
            "Source" => acc.source = field(&mut map, decode_nullable_string)?,
            "Destination" => acc.destination = field(&mut map, decode_nullable_string)?,
            "Size" => acc.size = field(&mut map, integer_token)?,
            "LastWriteUtc" => acc.last_write = field(&mut map, decode_datetime)?,
            "Permanent" => acc.permanent = field(&mut map, decode_bool)?,
            _ => {
                map.next_value::<IgnoredAny>()?;
            }
        }
    }
    Ok(acc)
}

struct EntryVisitor;

impl<'de> Visitor<'de> for EntryVisitor {
    type Value = EntryAcc;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a journal line object")
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<EntryAcc, A::Error> {
        read_entry(map)
    }
}

/// `GroupMembers`: `null` gives `None`, an array gives its slots. Any other JSON type is an error.
struct GroupMembers(Option<Vec<MemberSlot>>);

impl<'de> Deserialize<'de> for GroupMembers {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(GroupMembersVisitor).map(GroupMembers)
    }
}

struct GroupMembersVisitor;

impl<'de> Visitor<'de> for GroupMembersVisitor {
    type Value = Option<Vec<MemberSlot>>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an array of group members or null")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
        let mut slots = Vec::new();
        while let Some(slot) = seq.next_element::<MemberSlot>()? {
            slots.push(slot);
        }
        Ok(Some(slots))
    }
}

impl<'de> Deserialize<'de> for MemberSlot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(MemberSlotVisitor)
    }
}

struct MemberSlotVisitor;

impl<'de> Visitor<'de> for MemberSlotVisitor {
    type Value = MemberSlot;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a group member object or null")
    }

    fn visit_unit<E: de::Error>(self) -> Result<MemberSlot, E> {
        Ok(MemberSlot::Null)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<MemberSlot, A::Error> {
        read_member(map).map(MemberSlot::Member)
    }
}
