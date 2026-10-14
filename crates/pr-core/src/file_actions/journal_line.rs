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

use serde::Serialize;
use serde::Serializer;
use serde_json::ser::{CharEscape, Formatter};

use crate::time::DateTimeUtc;

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
        todo!("port DateTime write: utc -> DateTimeUtc::to_json, unspecified -> same without the final Z")
    }

    /// System.Text.Json reading of a `DateTime` text via [`DateTimeUtc::parse_json`]: `Z` -> utc, no zone ->
    /// unspecified, `+hh:mm` offset -> converted to UTC, utc (deliberate difference). `None` = the C# reader throws.
    pub fn parse_json(text: &str) -> Option<Self> {
        let _ = text;
        todo!("port DateTime read")
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
        todo!("serialize with DotNetJsonFormatter")
    }
}

/// `serde_json` formatter that reproduces the string escaping of `System.Text.Json`'s default encoder:
/// * `"` -> `"`, `\` -> `\\`, backspace/form feed/newline/CR/tab -> `\b \f \n \r \t`, every other control char
///   `0x00..=0x1F` -> `\u00XX` (UPPERCASE hex);
/// * `& ' + < > `` ` `` and DEL (0x7F) -> `\u00XX` (uppercase hex);
/// * every non-ASCII char -> `\uXXXX` (uppercase hex) per UTF-16 unit (an astral char becomes a surrogate pair
///   `😀`);
/// * everything else (including `/`) is written as is. Numbers, `null`, braces, commas stay compact (default trait
///   methods).
#[derive(Debug, Clone, Copy, Default)]
pub struct DotNetJsonFormatter;

impl Formatter for DotNetJsonFormatter {
    /// Called for runs of characters `serde_json` itself does not escape: apply the `& ' + < > `` DEL` and
    /// non-ASCII rules here.
    fn write_string_fragment<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> std::io::Result<()> {
        let _ = (writer, fragment);
        todo!("escape & ' + < > ` DEL and non-ASCII as \\uXXXX")
    }

    /// Called for `"`, `\` and the control characters `serde_json` escapes: emit the .NET spelling listed above.
    fn write_char_escape<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        char_escape: CharEscape,
    ) -> std::io::Result<()> {
        let _ = (writer, char_escape);
        todo!("emit .NET spelling of quote, backslash and control characters")
    }
}

/// C# `JournalLineParser.TryParse(string)`: the entry when `line` is accepted, `None` when it is rejected (malformed
/// JSON, wrong member types, unrecognised Type/State, empty Id, unusable Source or group member, ...). Never panics.
pub fn try_parse(line: &str) -> Option<JournalEntry> {
    let _ = line;
    todo!("port JournalLineParser.TryParse(string)")
}

/// C# `JournalLineParser.TryParse(ReadOnlySpan<byte>)`: same as [`try_parse`] for a UTF-8 line without terminator.
/// A UTF-8 BOM, NUL bytes, UTF-16 text and invalid UTF-8 inside a string value that is read are rejected.
pub fn try_parse_utf8(line: &[u8]) -> Option<JournalEntry> {
    let _ = line;
    todo!("port JournalLineParser.TryParse(ReadOnlySpan<byte>)")
}
