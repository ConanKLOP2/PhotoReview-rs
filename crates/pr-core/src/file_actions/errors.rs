//! Port of `PhotoReview.Core/FileActions/JournalErrors.cs`, `PhotoReview.Core/IO/FileSystemErrors.cs` and
//! `PhotoReview.Core/FileActions/RecycleBinCapacity.cs` (C# baseline e441d67a).
//!
//! * Stable journal error codes (ADR 0006, decision Q-L3). The journal is machine data: a record stores the code plus
//!   invariant ENGLISH text, never the UI language. The UI localizes by code. Codes are persisted: never rename or
//!   reuse one.
//! * Classification of "the destination already exists" file-system failures.
//! * The Recycle Bin capacity rule for group Recycle (F-WIN-2).
//!
//! Localization: `pr-l10n` does not exist yet, so every function that needs text takes a lookup closure
//! `&dyn Fn(&str) -> String` mapping a catalog key (see [`CODE_KEYS`]) to text in the language the caller wants
//! (English catalog for the journal, the current UI language for display).
// STUB (contract writer): the bodies are `todo!()`; the porter removes this allow together with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::fmt;
use std::io;

use super::types::JournalGroupMember;
use crate::abstractions::RecycleBin;

// ---------------------------------------------------------------------------------------------------------------
// Journal error codes
// ---------------------------------------------------------------------------------------------------------------

/// A pending Recycle Bin operation found its source still present during startup reconcile.
pub const SOURCE_STILL_EXISTS_AFTER_RECOVERY: &str = "SourceStillExistsAfterRecovery";
/// A pending Move/Copy could not be confirmed during startup reconcile; it is not replayed.
pub const PENDING_UNCONFIRMED: &str = "PendingUnconfirmed";
/// After Move/Copy the destination size did not match the source.
pub const VERIFY_SIZE_CHANGED: &str = "VerifySizeChanged";
/// After a recovery retry the destination check failed.
pub const RETRY_VERIFY_FAILED: &str = "RetryVerifyFailed";
/// A relative Move/Copy destination resolved outside the source folder; rejected before anything is journaled.
pub const DESTINATION_OUTSIDE_SOURCE: &str = "DestinationOutsideSource";
/// A Move copied the file (cross-volume) but could not delete the source: both copies exist, so the Move did not
/// happen from the user's point of view.
pub const MOVE_SOURCE_NOT_REMOVED: &str = "MoveSourceNotRemoved";
/// The user cancelled the operation and its compensation could not restore everything, so the entry stays as a
/// Failed Recovery item. A retry would FINISH what the user cancelled, so Recovery asks for confirmation first.
pub const CANCELLED_BY_USER: &str = "CancelledByUser";
/// A path holds an unpaired UTF-16 surrogate (legal in a Windows file name): JSON cannot carry it, so the journal line
/// would name a different file. Refused before anything is journaled or mutated.
pub const UNREPRESENTABLE_PATH: &str = "UnrepresentablePath";

/// Every code above, in the C# declaration order.
pub const ALL_CODES: [&str; 8] = [
    SOURCE_STILL_EXISTS_AFTER_RECOVERY,
    PENDING_UNCONFIRMED,
    VERIFY_SIZE_CHANGED,
    RETRY_VERIFY_FAILED,
    DESTINATION_OUTSIDE_SOURCE,
    MOVE_SOURCE_NOT_REMOVED,
    CANCELLED_BY_USER,
    UNREPRESENTABLE_PATH,
];

/// `code -> language-catalog key` (C# `TrKeys.*`; the key strings are the keys in `en.json`). Same order as
/// [`ALL_CODES`].
pub const CODE_KEYS: [(&str, &str); 8] = [
    (SOURCE_STILL_EXISTS_AFTER_RECOVERY, "core.journal.sourceStillExists"),
    (PENDING_UNCONFIRMED, "core.journal.pendingUnconfirmed"),
    (VERIFY_SIZE_CHANGED, "core.fileAction.verifyFailedSizeChanged"),
    (RETRY_VERIFY_FAILED, "core.recovery.verifyFailed"),
    (DESTINATION_OUTSIDE_SOURCE, "core.fileAction.destinationOutsideSource"),
    (MOVE_SOURCE_NOT_REMOVED, "core.fileAction.moveSourceNotRemoved"),
    (CANCELLED_BY_USER, "core.journal.cancelledByUser"),
    (UNREPRESENTABLE_PATH, "core.journal.unrepresentablePath"),
];

/// `JournalErrors.Text(...)` was asked about a code this build does not know (C# `ArgumentException`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownJournalCode(pub String);

impl fmt::Display for UnknownJournalCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unknown journal error code '{}'.", self.0)
    }
}

impl std::error::Error for UnknownJournalCode {}

/// The catalog key for `code`, or `None` for an unknown code. Exact, case-sensitive match (C# `StringComparer.Ordinal`).
pub fn catalog_key(code: &str) -> Option<&'static str> {
    todo!()
}

/// `JournalErrors.IsKnown`: true when `code` is one of [`ALL_CODES`] (exact match; `None` is false).
pub fn is_known(code: Option<&str>) -> bool {
    todo!()
}

/// `JournalErrors.EnglishText` / `LocalizedText`: `localize(key)` for the key of `code`. The caller passes the
/// English catalog for `EnglishText` (the text stored in the journal) and the current UI language for
/// `LocalizedText`. Unknown code => `Err(UnknownJournalCode)`.
pub fn text(code: &str, localize: &dyn Fn(&str) -> String) -> Result<String, UnknownJournalCode> {
    todo!()
}

/// `JournalErrors.Describe(code, storedText)`: UI text for a journal error. A known `code` gives its localized text
/// (`localize`); otherwise (`code` None, an unknown code from a newer build, an old record without a code) the
/// `stored_text` exactly as written (which may itself be `None`).
pub fn describe(code: Option<&str>, stored_text: Option<&str>, localize: &dyn Fn(&str) -> String) -> Option<String> {
    todo!()
}

/// An IO failure with a stable journal code (C# `JournalCodedException : IOException`). `message` is the UI text
/// (current language, shown in the status bar / Recovery window); the journal stores `code` plus English text instead.
/// Convert to a returnable error with [`into_io_error`](Self::into_io_error); [`for_journal`] recognizes it again.
#[derive(Debug)]
pub struct JournalCodedError {
    /// One of [`ALL_CODES`] (not a message); empty for the two "plain" C# constructors.
    pub code: String,
    pub message: String,
    pub inner: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl JournalCodedError {
    /// C# `JournalCodedException(string code)`: the caller passes the localized text it computed with [`text`].
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        todo!()
    }

    /// C# `JournalCodedException(string message, Exception innerException)`: `code` is the empty string.
    pub fn with_inner(message: impl Into<String>, inner: Box<dyn std::error::Error + Send + Sync>) -> Self {
        todo!()
    }

    /// `io::Error::other(self)` (C# `IOException`: kind `Other`, no OS code).
    pub fn into_io_error(self) -> io::Error {
        todo!()
    }
}

impl fmt::Display for JournalCodedError {
    /// The `message`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl std::error::Error for JournalCodedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        todo!()
    }
}

/// `JournalErrors.ForJournal(Exception)`: the `(code, text)` pair to persist for a failure.
///
/// In this order:
/// 1. `err` is a [`JournalCodedError`] (directly, or inside an `io::Error` -- look through `io::Error::get_ref()`)
///    whose `code` [`is_known`]: `(Some(code), text(code, english)?)`.
/// 2. `err` is [`crate::sync::Canceled`] (directly or inside an `io::Error`): `(Some(CANCELLED_BY_USER),
///    text(CANCELLED_BY_USER, english))`.
/// 3. Anything else (including a coded error whose code is unknown): `(None, err.to_string())` -- the failure's own
///    message with no code.
///
/// `english` looks keys up in the English catalog. The returned code is the `&'static str` from [`ALL_CODES`].
pub fn for_journal(
    err: &(dyn std::error::Error + 'static),
    english: &dyn Fn(&str) -> String,
) -> (Option<&'static str>, String) {
    todo!()
}

// ---------------------------------------------------------------------------------------------------------------
// FileSystemErrors
// ---------------------------------------------------------------------------------------------------------------

/// Win32 `ERROR_FILE_EXISTS`.
pub const ERROR_FILE_EXISTS: i32 = 80;
/// Win32 `ERROR_ALREADY_EXISTS`.
pub const ERROR_ALREADY_EXISTS: i32 = 183;

/// `FileSystemErrors.IsDestinationExists`: true when `err` is the Windows "destination already exists" failure. A
/// no-overwrite Copy/Move raises it BEFORE touching anything, so whatever is at the destination then was not created
/// by that call and must never be deleted by a compensation.
///
/// True when `err.raw_os_error()` is `Some(80)` or `Some(183)`, OR when `err.kind() == io::ErrorKind::AlreadyExists`
/// (fakes build the error from a kind only). Every other error -- access denied (5), sharing violation (32), disk
/// full (112), no OS code -- is false.
pub fn is_destination_exists(err: &io::Error) -> bool {
    todo!()
}

/// The pure HRESULT rule behind C# `IsDestinationExists` (for an `IOException`): true when the low 16 bits are 80 or
/// 183 AND `(hresult & 0x7FFF_0000) == 0x0007_0000` (the Win32 facility, with or without the severity bit).
/// `0x80070050` and `0x800700B7` and `0x000700B7` are true; `0x00000050` (low word 80 but not a Win32 HRESULT),
/// `0x800B0050` (another facility) and `0x80070070` (low word 0x70) are false. (`hresult as u32` for the masks.)
pub fn hresult_is_destination_exists(hresult: i32) -> bool {
    todo!()
}

// ---------------------------------------------------------------------------------------------------------------
// RecycleBinCapacity (C# `internal static class`)
// ---------------------------------------------------------------------------------------------------------------

/// .NET `Path.GetPathRoot(path)` on Windows; `None` where .NET returns null (empty string or only spaces).
/// The result is a new string because .NET normalizes the root's separators. Rules (pinned by golden `path-root`,
/// which decides any disagreement):
///
/// 1. `path` empty or only `' '` chars => `None`.
/// 2. `root_len` (UTF-16 == char count here; all separators and prefixes are ASCII):
///    * `device` = path starts with `\\?\`, `\??\`, or (`\\.\` / `//?/` style: two separators, then `'.'` or `'?'`,
///      then a separator; `'/'` counts as a separator everywhere); `device_unc` = `device` and `path.len() >= 8` and
///      `path[4..7] == "UNC"` (UPPER case, exact) and `path[7]` is a separator.
///    * If (`!device` or `device_unc`) and `path[0]` is a separator: if `device_unc` or `path[1]` is a separator:
///      UNC: `i = if device_unc { 8 } else { 2 }`; then `n = 2`; `while i < len && (!is_sep(path[i]) || { n -= 1; n > 0 })`
///      `i += 1` (stops at the second separator after the server/share, or the end); `root_len = i`. Else
///      (`"\foo"`) `root_len = 1`.
///    * Else if `device`: `i = 4`; advance while `i < len` and `path[i]` is not a separator; if `i < len` and `i > 4`
///      and `path[i]` is a separator, `i += 1`; `root_len = i`.
///    * Else if `len >= 2` and `path[1] == ':'` and `path[0]` is an ASCII letter: `i = 2`; if `len > 2` and `path[2]`
///      is a separator, `i = 3`; `root_len = i`.
///    * Else `root_len = 0`.
/// 3. `root_len == 0` => `Some("")` (NOT `None`).
/// 4. Normalize `path[..root_len]` (C# `NormalizeDirectorySeparators`): if the first char is a separator emit `'\\'`
///    and start at index 1; for each following char: a separator that is immediately followed by another separator
///    is dropped, any other separator is emitted as `'\\'`, other chars are copied. So `"C:/"` => `"C:\"`,
///    `"//srv/sh"` => `"\\srv\sh"`, `"\\\srv"` => `"\\srv"`.
///
/// Examples: `"C:\a\b.jpg"` => `"C:\"`; `"c:/a"` => `"c:\"`; `"C:a"` => `"C:"`; `"\\srv\sh\x"` => `"\\srv\sh"`;
/// `"\\?\C:\a"` => `"\\?\C:\"`; `"\\?\UNC\srv\sh\a"` => `"\\?\UNC\srv\sh"`; `"a\b"` => `""`; `""` => `None`.
pub fn get_path_root(path: &str) -> Option<String> {
    todo!()
}

/// `RecycleBinCapacity.FirstOverflow` (F-WIN-2): the bin of a volume must hold ALL of that volume's non-permanent
/// members together, not just each one alone, otherwise the shell deletes the overflow permanently while the journal
/// says "recycle".
///
/// 1. Skip members with `permanent == true`.
/// 2. Group the rest by volume = `get_path_root(&member.source).unwrap_or_default()`, compared OrdinalIgnoreCase
///    (`crate::text::ignore_case_key` as the map key). Groups are visited in order of FIRST appearance (use a `Vec`
///    of groups plus an index map, not a plain `HashMap` iteration).
/// 3. For each group, in that order: `first` = the first member of the group; `total` = sum of the group's `size`s
///    (`saturating_add`; C# would throw on overflow, which cannot happen for real file sizes); ask
///    `recycle_bin.fits_in_recycle_bin(Path::new(&first.source), total)` with `total` converted to `u64`
///    (`u64::try_from(total).unwrap_or(0)`). The first group that does NOT fit returns `Some(first)`.
/// 4. All groups fit (or no non-permanent member): `None`.
pub fn first_overflow<'a>(
    recycle_bin: &dyn RecycleBin,
    members: &'a [JournalGroupMember],
) -> Option<&'a JournalGroupMember> {
    todo!()
}
