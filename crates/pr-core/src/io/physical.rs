//! Port of `PhotoReview.Core/IO/PhysicalFileSystem.cs` (C# baseline d92d6aae): the real-disk [`FileSystem`], with the
//! Windows semantics the file-action safety story rests on (never overwrite on move/copy, create-new copy with an
//! ownership proof, atomic replace through a temp file, shared reads that never block other apps, reparse-point
//! resolution). Every behaviour below is pinned by the goldens `physical-fs-ops`, `physical-fs-pattern`,
//! `physical-fs-read-text` and `physical-fs-resolve` (contracts/W2-14.md).
//!
//! .NET quirks this port reproduces on purpose (do not "fix" them): `File.Delete` fails on read-only files and succeeds on
//! a missing file; `File.Move` of a directory is "file not found"; `File.Exists` is false for a trailing separator;
//! `File.ReadAllText` shares only `Read` (fails while another handle has the file open for writing); directory listings
//! use DOS wildcard rules; `Directory.EnumerateDirectories` includes junctions.
//!
//! The three Win32 primitives std has no equivalent for (`MoveFileExW` without/with `MOVEFILE_REPLACE_EXISTING`,
//! `CopyFileExW` with `COPY_FILE_FAIL_IF_EXISTS`, `DeleteFileW`) are called through a private `extern "system"` module
//! (contract W2-14 open question 1: std's `rename`/`copy` overwrite and `remove_file` deletes read-only files).

use std::collections::hash_map::RandomState;
use std::ffi::OsString;
use std::fs::{self, File, Metadata, OpenOptions};
use std::hash::BuildHasher;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::abstractions::{AppendStream, CopyCreationProof, FileStat, FileSystem, ReadSeek, SkippedEntry, SkippedKind};
use crate::file_actions::errors;
use crate::text::dotnet_path::{self, PathError};
use crate::text::ordinal_upper;
use crate::time::DateTimeUtc;

/// C# `ReplaceAttempts`: the final rename of an atomic write is tried at most 8 times.
pub const REPLACE_ATTEMPTS: u32 = 8;

/// The file system of the machine. Stateless: every call goes to the OS.
#[derive(Debug, Default, Clone, Copy)]
pub struct PhysicalFileSystem;

/// One directory entry as a listing delivers it (C# `FileInfo` from `DirectoryInfo.EnumerateFiles`): full path, length and
/// last write time taken from the directory entry, no per-file open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    /// `Path::new(directory)` made absolute, joined with the entry name (C# `FileInfo.FullName`).
    pub full_name: PathBuf,
    pub length: u64,
    pub last_write_utc: DateTimeUtc,
}

/// Test seam (C# `openListing` of the internal `EnumerateFilesWithStat`): opens a directory listing. `Err` from the opener
/// is a failure before the first entry; an `Err` item is a failure after `n` entries.
pub type OpenListing<'a> = &'a dyn Fn(&Path) -> io::Result<Box<dyn Iterator<Item = io::Result<ListedFile>>>>;

impl PhysicalFileSystem {
    /// C# internal `EnumerateFilesWithStat(directory, include, onSkipped, openListing)`: the logic of the trait method
    /// `enumerate_files_with_stat` over an injected listing. An `Err` item after at least one entry was read keeps what was
    /// read and reports one `SkippedEntry { path: directory, reason: err.to_string(), kind: ListingInterrupted }`; an `Err`
    /// before the first entry (opener or first item) is returned. Any other failure kind is not special-cased.
    pub fn enumerate_files_with_stat_from(
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
        open_listing: OpenListing<'_>,
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        if is_blank(directory) {
            return Err(blank_path_error());
        }
        // `using var enumerator`: the iterator is dropped on every exit path (return, `?`, break).
        let mut listing = open_listing(directory)?;
        let mut listed = 0usize;
        let mut files = Vec::new();
        loop {
            let info = match listing.next() {
                None => break,
                Some(Ok(info)) => {
                    listed += 1;
                    info
                }
                // C# `catch (...) when (listed > 0 && ex is IOException or UnauthorizedAccessException)`: every io::Error
                // is one of those two. A failure before the first entry means the folder itself is unreadable: it
                // propagates so the load fails loudly instead of showing an empty catalog.
                Some(Err(err)) if listed > 0 => {
                    on_skipped(SkippedEntry {
                        path: directory.to_path_buf(),
                        reason: err.to_string(),
                        kind: SkippedKind::ListingInterrupted,
                    });
                    break;
                }
                Some(Err(err)) => return Err(err),
            };
            if !include(&info.full_name) {
                continue;
            }
            let stat = FileStat {
                length: info.length,
                last_write_utc: info.last_write_utc,
            };
            files.push((info.full_name, Some(stat)));
        }
        Ok(files)
    }
}

/// C# `ReplaceWithRetry(move, sleep)` (internal): runs `move_op`; a transient failure (`PermissionDenied`, or raw OS error
/// 32 sharing violation, or 33 lock violation) is retried after `sleep(attempt * 3)` milliseconds, at most
/// [`REPLACE_ATTEMPTS`] attempts in total (sleeps 3, 6, 9 ... 21 = at most 84 ms); any other error, and the last attempt's
/// error, is returned at once without sleeping.
pub fn replace_with_retry(mut move_op: impl FnMut() -> io::Result<()>, mut sleep: impl FnMut(u32)) -> io::Result<()> {
    let mut attempt = 1u32;
    loop {
        match move_op() {
            Ok(()) => return Ok(()),
            Err(err) if attempt < REPLACE_ATTEMPTS && is_transient_replace_failure(&err) => sleep(attempt * 3),
            Err(err) => return Err(err),
        }
        attempt += 1;
    }
}

/// C# `IsTransientReplaceFailure`: `UnauthorizedAccessException` (which .NET raises only for ERROR_ACCESS_DENIED, so a
/// `PermissionDenied` with another raw code such as 1314 is NOT transient), or an `IOException` with HRESULT
/// 0x80070020 (sharing violation) / 0x80070021 (lock violation).
fn is_transient_replace_failure(err: &io::Error) -> bool {
    match err.raw_os_error() {
        Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION | ERROR_ACCESS_DENIED) => true,
        Some(_) => false,
        None => err.kind() == io::ErrorKind::PermissionDenied,
    }
}

/// C# `IsSwallowedAsDestinationExists` (internal): the failures of the no-overwrite copy that mean "the destination
/// already exists" (`TryCopyNew` returns false): exactly [`errors::is_destination_exists`](crate::file_actions::errors::is_destination_exists).
pub fn is_swallowed_as_destination_exists(err: &io::Error) -> bool {
    errors::is_destination_exists(err)
}

/// `FileSystemName.MatchesWin32Expression` semantics of `Directory.EnumerateFiles(dir, pattern)` (legacy overload):
/// DOS wildcards, case-insensitive with [`text::ordinal_upper`](crate::text::ordinal_upper). See the contract for the
/// translation and the matcher; pinned by golden `physical-fs-pattern`.
pub fn matches_win32_expression(expression: &str, name: &str) -> bool {
    let expression: Vec<u16> = expression.encode_utf16().collect();
    let name: Vec<u16> = name.encode_utf16().collect();
    matches_win32_expression_units(&expression, &name)
}

// .NET strings are UTF-16: `FileSystemName` walks UTF-16 code units, so `?` (DOS_QM) consumes ONE surrogate of an astral
// character, not the whole character. The matcher therefore works on `u16` units, never on Rust `char`s.
const U_STAR: u16 = b'*' as u16;
const U_QM: u16 = b'?' as u16;
const U_DOT: u16 = b'.' as u16;
const U_DOS_STAR: u16 = b'<' as u16;
const U_DOS_QM: u16 = b'>' as u16;
const U_DOS_DOT: u16 = b'"' as u16;
const U_ESCAPE: u16 = b'\\' as u16;

/// [`matches_win32_expression`] over UTF-16 code units (a listed name may hold a lone surrogate).
fn matches_win32_expression_units(expression: &[u16], name: &[u16]) -> bool {
    // `FileSystemEnumerableFactory.NormalizeInputs`: "", "." and "*.*" historically mean "*".
    let is_star_star = expression == [U_STAR, U_DOT, U_STAR];
    let translated: Vec<u16> =
        if expression.is_empty() || expression == [U_DOT] || expression == [U_STAR] || is_star_star {
            vec![U_STAR]
        } else {
            translate_win32_expression(expression)
        };
    match_pattern(&translated, name)
}

/// `char.ToUpperInvariant` of one UTF-16 unit: a surrogate is returned unchanged.
fn upper_unit(unit: u16) -> u16 {
    match char::from_u32(u32::from(unit)) {
        Some(c) => {
            let upper = ordinal_upper(c);
            // `ordinal_upper` of a BMP char stays in the BMP (simple case mapping).
            u16::try_from(u32::from(upper)).unwrap_or(unit)
        }
        None => unit,
    }
}

/// `MemoryExtensions.EndsWith(name, end, StringComparison.OrdinalIgnoreCase)` over UTF-16 units: a well-formed
/// surrogate pair is compared by the upper case of its scalar value, anything else unit by unit.
fn units_eq_ignore_case(a: &[u16], b: &[u16]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut left = char::decode_utf16(a.iter().copied());
    let mut right = char::decode_utf16(b.iter().copied());
    loop {
        match (left.next(), right.next()) {
            (None, None) => return true,
            (Some(Ok(x)), Some(Ok(y))) => {
                if ordinal_upper(x) != ordinal_upper(y) {
                    return false;
                }
            }
            (Some(Err(x)), Some(Err(y))) => {
                if x.unpaired_surrogate() != y.unpaired_surrogate() {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// `FileSystemName.TranslateWin32Expression`: `?` => `>` (DOS_QM); a `.` followed by `?` or `*` => `"` (DOS_DOT); a
/// trailing `.` right after a `*` turns that `*` into `<` (DOS_STAR) and is dropped; everything else unchanged.
fn translate_win32_expression(chars: &[u16]) -> Vec<u16> {
    let length = chars.len();
    let mut out: Vec<u16> = Vec::with_capacity(length);
    for (i, &c) in chars.iter().enumerate() {
        match c {
            U_DOT => {
                if i >= 1 && i == length - 1 && chars[i - 1] == U_STAR {
                    if let Some(last) = out.last_mut() {
                        *last = U_DOS_STAR;
                    }
                } else if i < length - 1 && (chars[i + 1] == U_QM || chars[i + 1] == U_STAR) {
                    out.push(U_DOS_DOT);
                } else {
                    out.push(U_DOT);
                }
            }
            U_QM => out.push(U_DOS_QM),
            _ => out.push(c),
        }
    }
    out
}

/// `FileSystemName.MatchPattern(expression, name, ignoreCase: true, useExtendedWildcards: true)`: the FsRtl NFA over
/// UTF-16 units. State `2 * offset` sits before expression unit `offset`; `*` and `<` also have the state
/// `2 * offset + 1`; `2 * len` is "expression exhausted".
fn match_pattern(expression: &[u16], name: &[u16]) -> bool {
    if expression.is_empty() || name.is_empty() {
        return false;
    }
    if expression[0] == U_STAR {
        if expression.len() == 1 {
            return true;
        }
        let end = &expression[1..];
        if !end
            .iter()
            .any(|&c| matches!(c, U_DOS_DOT | U_STAR | U_DOS_STAR | U_DOS_QM | U_QM))
        {
            // A single leading `*` then literals: "ends with" (OrdinalIgnoreCase).
            if name.len() < end.len() {
                return false;
            }
            return units_eq_ignore_case(&name[name.len() - end.len()..], end);
        }
    }

    let len = expression.len();
    let max_state = len * 2;
    let mut prior: Vec<usize> = vec![0];
    let mut current: Vec<usize> = Vec::new();
    let mut name_offset = 0usize;
    let mut name_char: u16 = 0;
    let mut name_finished = false;

    while !name_finished {
        if name_offset < name.len() {
            name_char = name[name_offset];
            name_offset += 1;
        } else {
            if prior.last() == Some(&max_state) {
                break;
            }
            name_finished = true;
        }

        current.clear();
        for &prior_state in &prior {
            let mut offset = prior_state.div_ceil(2);
            while offset < len {
                let mut state = offset * 2;
                let mut c = expression[offset];
                // What follows the match attempt of this expression char.
                let zero_or_more;
                if c == U_STAR {
                    zero_or_more = true;
                } else if c == U_DOS_STAR {
                    // DOS_STAR: any character zero or more times, but a '.' only when it is not the last one.
                    let mut not_last_period = false;
                    if !name_finished && name_char == U_DOT {
                        not_last_period = name[name_offset..].contains(&U_DOT);
                    }
                    zero_or_more = name_finished || name_char != U_DOT || not_last_period;
                } else {
                    state += 2;
                    if c == U_DOS_QM {
                        // DOS_QM: zero characters at the end of the name or before a '.', else one character.
                        if name_finished || name_char == U_DOT {
                            offset += 1;
                            if offset == len {
                                current.push(max_state);
                            }
                            continue;
                        }
                        current.push(state);
                        break;
                    } else if c == U_DOS_DOT {
                        // DOS_DOT: a '.', or zero characters beyond the end of the name.
                        if name_finished {
                            offset += 1;
                            if offset == len {
                                current.push(max_state);
                            }
                            continue;
                        }
                        if name_char == U_DOT {
                            current.push(state);
                        }
                        break;
                    } else {
                        if c == U_ESCAPE {
                            // Escape: match the next expression char literally.
                            offset += 1;
                            if offset == len {
                                current.push(max_state);
                                break;
                            }
                            state = offset * 2 + 2;
                            c = expression[offset];
                        }
                        if name_finished {
                            break;
                        }
                        if c == U_QM || upper_unit(c) == upper_unit(name_char) {
                            current.push(state);
                        }
                        break;
                    }
                }
                if zero_or_more {
                    current.push(state);
                }
                current.push(state + 1);
                offset += 1;
                if offset == len {
                    current.push(max_state);
                }
            }
        }

        if current.is_empty() {
            return false;
        }
        current.sort_unstable();
        current.dedup();
        std::mem::swap(&mut prior, &mut current);
    }

    prior.last() == Some(&max_state)
}

/// `File.ReadAllText(path)` decoding: a UTF-8 / UTF-16 LE / UTF-16 BE / UTF-32 LE / UTF-32 BE byte-order mark selects the
/// encoding and is dropped; otherwise UTF-8. Invalid input becomes U+FFFD, never an error. Pinned by golden
/// `physical-fs-read-text`.
pub fn decode_text_like_dotnet(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE, 0x00, 0x00]) {
        decode_utf32(rest, u32::from_le_bytes)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        decode_utf16(rest, u16::from_le_bytes)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        decode_utf16(rest, u16::from_be_bytes)
    } else if let Some(rest) = bytes.strip_prefix(&[0x00, 0x00, 0xFE, 0xFF]) {
        decode_utf32(rest, u32::from_be_bytes)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// UTF-16 with .NET replacement: a lone surrogate is one U+FFFD, an odd trailing byte is one U+FFFD.
fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let chunks = bytes.chunks_exact(2);
    let odd = !chunks.remainder().is_empty();
    let units: Vec<u16> = chunks.map(|c| unit([c[0], c[1]])).collect();
    let mut text = String::from_utf16_lossy(&units);
    if odd {
        text.push('\u{FFFD}');
    }
    text
}

/// UTF-32 with .NET replacement: a code point above U+10FFFF or in the surrogate range is one U+FFFD, a 1-3 byte tail
/// is one U+FFFD.
fn decode_utf32(bytes: &[u8], unit: fn([u8; 4]) -> u32) -> String {
    let chunks = bytes.chunks_exact(4);
    let tail = !chunks.remainder().is_empty();
    let mut text: String = chunks
        .map(|c| char::from_u32(unit([c[0], c[1], c[2], c[3]])).unwrap_or('\u{FFFD}'))
        .collect();
    if tail {
        text.push('\u{FFFD}');
    }
    text
}

/// `Guid.NewGuid().ToString("N")`: 32 lowercase hex digits, unique per call across threads and processes (it names the
/// temp file of an atomic write). Not required to be cryptographically random.
pub fn new_guid_n() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    static PROCESS_KEY: OnceLock<u64> = OnceLock::new();
    let pid = std::process::id();
    let key = *PROCESS_KEY.get_or_init(|| RandomState::new().hash_one((pid, now_nanos())));
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // High half: pid, time and fresh hasher keys (distinct across processes). Low half: the process counter under a
    // per-process mask (injective in `n`, so never repeated within the process).
    let high = RandomState::new().hash_one((pid, now_nanos(), n, key));
    let low = n ^ key;
    format!("{high:016x}{low:016x}")
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------------------------
// Win32 constants and small helpers
// ---------------------------------------------------------------------------------------------------------------

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PATH_NOT_FOUND: i32 = 3;
const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_SHARING_VIOLATION: i32 = 32;
const ERROR_LOCK_VIOLATION: i32 = 33;
const ERROR_INVALID_PARAMETER: i32 = 87;
const ERROR_ALREADY_EXISTS: i32 = 183;
const ERROR_FILENAME_EXCED_RANGE: i32 = 206;
const ERROR_CANT_RESOLVE_FILENAME: i32 = 1921;

const FILE_LIST_DIRECTORY: u32 = 0x1;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

const FILE_SHARE_NONE: u32 = 0;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_SHARE_DELETE: u32 = 0x4;
const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
const FILE_FLAG_WRITE_THROUGH: u32 = 0x8000_0000;

const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// .NET `MaxFollowedLinks`: a chain longer than this is treated as a loop.
const MAX_FOLLOWED_LINKS: usize = 63;

/// `ArgumentException.ThrowIfNullOrWhiteSpace`.
fn is_blank(path: &Path) -> bool {
    path.as_os_str().to_string_lossy().trim().is_empty()
}

fn blank_path_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "The value cannot be an empty string or composed entirely of whitespace.",
    )
}

/// The path as text for the .NET `Path.*` helpers. A path that is not valid Unicode (lone surrogate) cannot be passed
/// through them: `InvalidInput`.
fn path_str(path: &Path) -> io::Result<&str> {
    path.to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "The path is not valid Unicode."))
}

fn path_error(err: PathError) -> io::Error {
    match err {
        PathError::Argument => io::Error::new(io::ErrorKind::InvalidInput, err.to_string()),
        PathError::TooLong => io::Error::from_raw_os_error(ERROR_FILENAME_EXCED_RANGE),
    }
}

/// `Path.GetFullPath` as an `io::Result`.
fn full_path(path: &Path) -> io::Result<String> {
    dotnet_path::get_full_path(path_str(path)?).map_err(path_error)
}

/// `PathInternal.IsDevice`: `\\?\`, `\??\` or `\\.\` (either separator).
fn is_device(path: &str) -> bool {
    let b = path.as_bytes();
    let sep = |c: u8| c == b'\\' || c == b'/';
    if b.len() < 4 {
        return false;
    }
    (sep(b[0]) && sep(b[1]) && (b[2] == b'?' || b[2] == b'.') && sep(b[3]))
        || (b[0] == b'\\' && b[1] == b'?' && b[2] == b'?' && b[3] == b'\\')
}

/// `PathInternal.EnsureExtendedPrefix`: `\\?\` (or `\\?\UNC\`) in front of a fully qualified, non-device path.
fn ensure_extended_prefix(path: &str) -> String {
    if !dotnet_path::is_path_fully_qualified(path) || is_device(path) {
        return path.to_owned();
    }
    match path.strip_prefix(r"\\") {
        Some(unc) => format!(r"\\?\UNC\{unc}"),
        None => format!(r"\\?\{path}"),
    }
}

/// `PathInternal.EnsureExtendedPrefixIfNeeded`: the extended form when the path is 260 UTF-16 units or longer, or ends
/// with a period or a space (which Win32 would otherwise silently trim).
fn ensure_extended_prefix_if_needed(path: &str) -> String {
    if path.encode_utf16().count() >= 260 || path.ends_with(['.', ' ']) {
        ensure_extended_prefix(path)
    } else {
        path.to_owned()
    }
}

fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

/// The UTF-16 units of a listed name (a Windows name may hold a lone surrogate, which `to_string_lossy` would lose).
#[cfg(windows)]
fn os_units(name: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    name.encode_wide().collect()
}

#[cfg(not(windows))]
fn os_units(name: &std::ffi::OsStr) -> Vec<u16> {
    name.to_string_lossy().encode_utf16().collect()
}

/// `FileSystemEntry.ToSpecifiedFullPath` (`Path.Join(originalRoot, name)`): one `\` between them unless `directory`
/// already ends with a separator or is a bare drive-relative spec `X:` (observed with .NET 10: `Directory.EnumerateFiles("C:")`
/// yields `C:a.txt`, the current directory of drive C:, while `C:sub` yields `C:sub\b.txt`).
fn join_as_spelled(directory: &Path, name: &std::ffi::OsStr) -> PathBuf {
    let mut joined: OsString = directory.as_os_str().to_owned();
    let spelled = directory.as_os_str().to_string_lossy();
    let ends_with_sep = spelled.ends_with(is_sep);
    let bare_drive = {
        let b = spelled.as_bytes();
        b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
    };
    if !directory.as_os_str().is_empty() && !ends_with_sep && !bare_drive {
        joined.push("\\");
    }
    joined.push(name);
    PathBuf::from(joined)
}

/// The attributes of the entry ITSELF (a link is not followed; a dangling junction still has attributes).
fn entry_attributes(path: &str) -> Option<u32> {
    fs::symlink_metadata(path).ok().map(|m| attributes_of(&m))
}

#[cfg(windows)]
fn attributes_of(metadata: &Metadata) -> u32 {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes()
}

#[cfg(not(windows))]
fn attributes_of(metadata: &Metadata) -> u32 {
    let mut attributes = 0;
    if metadata.is_dir() {
        attributes |= FILE_ATTRIBUTE_DIRECTORY;
    }
    if metadata.file_type().is_symlink() {
        attributes |= FILE_ATTRIBUTE_REPARSE_POINT;
    }
    attributes
}

/// Last write time of the entry, from its FILETIME (100 ns precision, UTC).
#[cfg(windows)]
fn last_write_of(metadata: &Metadata) -> DateTimeUtc {
    use std::os::windows::fs::MetadataExt;
    DateTimeUtc::from_filetime(metadata.last_write_time()).unwrap_or(DateTimeUtc::MIN)
}

#[cfg(not(windows))]
fn last_write_of(metadata: &Metadata) -> DateTimeUtc {
    metadata
        .modified()
        .ok()
        .and_then(DateTimeUtc::from_system_time)
        .unwrap_or(DateTimeUtc::MIN)
}

fn stat_of(metadata: &Metadata) -> FileStat {
    FileStat {
        length: metadata.len(),
        last_write_utc: last_write_of(metadata),
    }
}

/// `OpenOptions` with an explicit Win32 share mode and flags (std's default share mode is ReadWrite|Delete, which is
/// wrong for every .NET call that shares less).
fn options(share: u32, flags: u32) -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(share);
        options.custom_flags(flags);
    }
    #[cfg(not(windows))]
    let _ = (share, flags);
    options
}

/// .NET `File.Exists`-style probe on an already full path: the entry exists and is not a directory. Like .NET
/// (`FillAttributeInfo`), a name left ending with a space or a dot after the trailing separator is trimmed is queried in
/// the extended form, so `<dir>\b.txt \` is NOT `<dir>\b.txt`.
fn file_exists_full(full: &str) -> bool {
    win32_file_exists(full)
}

/// .NET `Directory.Exists`-style probe on an already full path (`<dir>\sub \` is not `<dir>\sub`, see [`file_exists_full`]).
fn directory_exists_full(full: &str) -> bool {
    win32_directory_exists(full).0
}

/// `new FileStream(path, ...)` (also `File.ReadAllText`): .NET opens `Path.GetFullPath(path)`, so `<dir>\sub/` becomes
/// `<dir>\sub\` and a folder spelled that way is "path not found" (raw 3), not "access denied". `SafeFileHandle.Open`
/// reports "path not found" on a bare root as access denied (raw 5).
fn open_like_file_stream(options: &OpenOptions, path: &Path) -> io::Result<File> {
    let full = full_path(path)?;
    match options.open(ensure_extended_prefix_if_needed(&full)) {
        Err(err)
            if err.raw_os_error() == Some(ERROR_PATH_NOT_FOUND) && full.len() == dotnet_path::root_length(&full) =>
        {
            Err(io::Error::from_raw_os_error(ERROR_ACCESS_DENIED))
        }
        other => other,
    }
}

/// .NET `FileSystem.FillAttributeInfo` + attribute test: the attributes of the entry itself (trailing separator trimmed,
/// long or dot/space-ending names in the extended form), or the OS error.
fn win32_attributes(path: &str) -> io::Result<u32> {
    let trimmed = dotnet_path::trim_ending_directory_separator(path);
    fs::symlink_metadata(ensure_extended_prefix_if_needed(trimmed)).map(|m| attributes_of(&m))
}

/// .NET `FileSystem.DirectoryExists(fullPath, out lastError)`.
fn win32_directory_exists(path: &str) -> (bool, Option<i32>) {
    match win32_attributes(path) {
        Ok(attributes) => (attributes & FILE_ATTRIBUTE_DIRECTORY != 0, None),
        Err(err) => (false, err.raw_os_error()),
    }
}

/// .NET `FileSystem.FileExists(fullPath)`.
fn win32_file_exists(path: &str) -> bool {
    win32_attributes(path).is_ok_and(|attributes| attributes & FILE_ATTRIBUTE_DIRECTORY == 0)
}

/// `Directory.CreateDirectory(path)` (.NET `FileSystem.CreateDirectory` on Windows), not `create_dir_all`: an existing
/// entry with the DIRECTORY attribute is enough (a dangling junction or a junction loop is `Ok`); the missing folders
/// are collected walking back from the end and created root-first with `CreateDirectoryW` on the extended path; a
/// failure is reported only when the LAST (wanted) folder failed, and "already exists" only when a file sits there
/// (raw 183) or the existence check was denied (raw 5). Nothing to create and no existing ancestor: the root must exist,
/// else raw 3 (this is also how a device name such as `<dir>\nul`, which `GetFullPath` turns into `\\.\nul`, fails).
fn create_directory_impl(path: &Path) -> io::Result<()> {
    let full = full_path(path)?;
    if win32_directory_exists(&full).0 {
        return Ok(());
    }

    let bytes = full.as_bytes();
    let mut length = full.len();
    // Trim the trailing separator or the walk would create the same folder twice.
    if length >= 2 && full.ends_with(is_sep) {
        length -= 1;
    }
    let root_length = dotnet_path::root_length(&full);
    let mut missing: Vec<&str> = Vec::new();
    let mut some_path_exists = false;
    if length > root_length {
        // Separators are ASCII, so every slice below ends on a char boundary.
        let mut i = length as isize - 1;
        while i >= root_length as isize && !some_path_exists {
            let dir = &full[..i as usize + 1];
            if win32_directory_exists(dir).0 {
                some_path_exists = true;
            } else {
                missing.push(dir);
            }
            while i > root_length as isize && !is_sep(char::from(bytes[i as usize])) {
                i -= 1;
            }
            i -= 1;
        }
    }

    let count = missing.len();
    let mut last_ok = true;
    let mut first_error: Option<io::Error> = None;
    while let Some(name) = missing.pop() {
        let created = fs::create_dir(ensure_extended_prefix(name));
        last_ok = created.is_ok();
        if let Err(err) = created
            && first_error.is_none()
        {
            if err.raw_os_error() != Some(ERROR_ALREADY_EXISTS) {
                first_error = Some(err);
            } else if win32_file_exists(name) {
                // A file in this folder's place.
                first_error = Some(err);
            } else if let (false, Some(ERROR_ACCESS_DENIED)) = win32_directory_exists(name) {
                first_error = Some(io::Error::from_raw_os_error(ERROR_ACCESS_DENIED));
            }
        }
    }

    // Masks OS differences for CreateDirectory("X:\") when X: does not exist (and for network roots).
    if count == 0 && !some_path_exists {
        let root = &full[..root_length];
        if !win32_directory_exists(root).0 {
            return Err(io::Error::from_raw_os_error(ERROR_PATH_NOT_FOUND));
        }
        return Ok(());
    }
    // Only creating the exact folder that was asked for counts.
    match first_error {
        Some(err) if !last_ok => Err(err),
        _ => Ok(()),
    }
}

/// `File.Move(source, destination, overwrite)`: full paths, the source must be a FILE (else raw 2, also for a
/// directory), then `MoveFileExW(MOVEFILE_COPY_ALLOWED [| MOVEFILE_REPLACE_EXISTING])`.
fn move_impl(source: &Path, destination: &Path, overwrite: bool) -> io::Result<()> {
    let source_full = full_path(source)?;
    let destination_full = full_path(destination)?;
    if !file_exists_full(&source_full) {
        return Err(io::Error::from_raw_os_error(ERROR_FILE_NOT_FOUND));
    }
    win32::move_file(&win32::wide(&source_full), &win32::wide(&destination_full), overwrite)
}

// ---------------------------------------------------------------------------------------------------------------
// Win32 FFI (kernel32 is linked by std already; no new crate)
// ---------------------------------------------------------------------------------------------------------------

#[cfg(windows)]
mod win32 {
    use std::ffi::{OsStr, c_void};
    use std::io;
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_COPY_ALLOWED: u32 = 0x2;
    const COPY_FILE_FAIL_IF_EXISTS: u32 = 0x1;
    /// .NET `PathInternal.MaxShortPath`: from this length on a path needs the `\\?\` form.
    const MAX_SHORT_PATH: usize = 260;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
        fn CopyFileExW(
            existing: *const u16,
            new: *const u16,
            progress: *const c_void,
            data: *const c_void,
            cancel: *mut i32,
            flags: u32,
        ) -> i32;
        fn DeleteFileW(path: *const u16) -> i32;
    }

    /// A NUL-terminated UTF-16 buffer of a FULL path (no NUL inside: `get_full_path` rejects it), in the `\\?\` form
    /// when it is 260 UTF-16 units or longer (.NET `PathInternal.EnsureExtendedPrefixIfNeeded`; std does this only
    /// inside std).
    pub(super) fn wide(full: &str) -> Vec<u16> {
        let units = full.encode_utf16().count();
        let extended;
        let text = if units >= MAX_SHORT_PATH {
            extended = super::ensure_extended_prefix(full);
            extended.as_str()
        } else {
            full
        };
        OsStr::new(text).encode_wide().chain(std::iter::once(0)).collect()
    }

    fn check(result: i32) -> io::Result<()> {
        if result != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(super) fn move_file(source: &[u16], destination: &[u16], overwrite: bool) -> io::Result<()> {
        debug_assert!(source.last() == Some(&0) && destination.last() == Some(&0));
        let flags = MOVEFILE_COPY_ALLOWED | if overwrite { MOVEFILE_REPLACE_EXISTING } else { 0 };
        // SAFETY: both buffers are NUL-terminated UTF-16 strings built by `wide` that outlive the call; MoveFileExW
        // only reads them.
        check(unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) })
    }

    /// `CopyFileExW(.., COPY_FILE_FAIL_IF_EXISTS)`: never overwrites; keeps attributes and last write time.
    pub(super) fn copy_file_no_overwrite(source: &[u16], destination: &[u16]) -> io::Result<()> {
        debug_assert!(source.last() == Some(&0) && destination.last() == Some(&0));
        let mut cancel: i32 = 0;
        // SAFETY: both buffers are NUL-terminated UTF-16 strings that outlive the call; no progress routine (null) and
        // no callback data; `cancel` is a valid i32 that lives for the whole synchronous call.
        check(unsafe {
            CopyFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                &mut cancel,
                COPY_FILE_FAIL_IF_EXISTS,
            )
        })
    }

    pub(super) fn delete_file(path: &[u16]) -> io::Result<()> {
        debug_assert!(path.last() == Some(&0));
        // SAFETY: `path` is a NUL-terminated UTF-16 string that outlives the call; DeleteFileW only reads it.
        check(unsafe { DeleteFileW(path.as_ptr()) })
    }
}

/// Non-Windows hosts: the crate still builds (CI lints on Windows only), but the Win32 primitives are unavailable.
#[cfg(not(windows))]
mod win32 {
    use std::io;

    pub(super) fn wide(full: &str) -> Vec<u16> {
        full.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn unsupported() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "PhysicalFileSystem needs Windows")
    }

    pub(super) fn move_file(_: &[u16], _: &[u16], _: bool) -> io::Result<()> {
        Err(unsupported())
    }

    pub(super) fn copy_file_no_overwrite(_: &[u16], _: &[u16]) -> io::Result<()> {
        Err(unsupported())
    }

    pub(super) fn delete_file(_: &[u16]) -> io::Result<()> {
        Err(unsupported())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Directory listings
// ---------------------------------------------------------------------------------------------------------------

/// The folder a .NET `FileSystemEnumerator` actually lists: `TrimEndingDirectorySeparator(GetFullPath(directory))`
/// (so `<dir>\sub ` and `<dir>\sub\ ` list `<dir>\sub`), checked the way its `CreateDirectoryHandle` opens it.
fn listing_root(directory: &Path) -> io::Result<String> {
    let full = full_path(directory)?;
    let root = dotnet_path::trim_ending_directory_separator(&full).to_owned();
    open_directory_handle(&root)?;
    Ok(root)
}

/// `FileSystemEnumerator.CreateDirectoryHandle`: `CreateFile(FILE_LIST_DIRECTORY, share ReadWrite|Delete,
/// FILE_FLAG_BACKUP_SEMANTICS)`, following links. "File not found" is reported as "path not found" (raw 3, .NET
/// historically throws `DirectoryNotFoundException`); a FILE opens fine and then fails the directory query with
/// ERROR_INVALID_PARAMETER (raw 87), not std's ERROR_DIRECTORY (267).
#[cfg(windows)]
fn open_directory_handle(root: &str) -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    let opened = OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(ensure_extended_prefix_if_needed(root));
    let handle = match opened {
        Ok(handle) => handle,
        Err(err) if err.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => {
            return Err(io::Error::from_raw_os_error(ERROR_PATH_NOT_FOUND));
        }
        Err(err) => return Err(err),
    };
    if attributes_of(&handle.metadata()?) & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(io::Error::from_raw_os_error(ERROR_INVALID_PARAMETER));
    }
    Ok(())
}

#[cfg(not(windows))]
fn open_directory_handle(_root: &str) -> io::Result<()> {
    Ok(())
}

/// `Directory.EnumerateFiles/EnumerateDirectories(directory)` over the OS listing: entries whose DIRECTORY attribute
/// equals `directories` (Hidden/System included, `EnumerationOptions.Compatible`), in directory order, joined to the
/// directory as spelled.
fn list_entries(directory: &Path, directories: bool, keep: impl Fn(&[u16]) -> bool) -> io::Result<Vec<PathBuf>> {
    let root = listing_root(directory)?;
    let mut found = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let attributes = attributes_of(&entry.metadata()?);
        if (attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directories {
            continue;
        }
        let name = entry.file_name();
        if keep(&os_units(&name)) {
            found.push(join_as_spelled(directory, &name));
        }
    }
    Ok(found)
}

/// The production listing of `enumerate_files_with_stat` (C# `new DirectoryInfo(dir).EnumerateFiles("*")`): files of
/// `get_full_path(directory)` with the stat of their directory entry (no per-file open).
fn open_physical_listing(directory: &Path) -> io::Result<Box<dyn Iterator<Item = io::Result<ListedFile>>>> {
    let full = PathBuf::from(listing_root(directory)?);
    let entries = fs::read_dir(&full)?;
    Ok(Box::new(entries.filter_map(move |entry| {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => return Some(Err(err)),
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(err) => return Some(Err(err)),
        };
        if attributes_of(&metadata) & FILE_ATTRIBUTE_DIRECTORY != 0 {
            return None;
        }
        let stat = stat_of(&metadata);
        Some(Ok(ListedFile {
            full_name: join_as_spelled(&full, &entry.file_name()),
            length: stat.length,
            last_write_utc: stat.last_write_utc,
        }))
    })))
}

// ---------------------------------------------------------------------------------------------------------------
// Reparse points (SEC-01)
// ---------------------------------------------------------------------------------------------------------------

/// `ResolveIfReparsePoint`: the final target of `candidate` when it exists (by the entry itself) and is a symlink or
/// junction; otherwise `candidate` unchanged.
fn resolve_if_reparse_point(candidate: String) -> io::Result<String> {
    let Some(attributes) = entry_attributes(&candidate) else {
        return Ok(candidate); // doesn't exist yet: nothing to resolve
    };
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return Ok(candidate);
    }
    // `LinkTarget` is non-null only for a reparse point .NET understands (symlink or junction): `read_link` fails for
    // other tags (cloud placeholders, dedup ...).
    if fs::read_link(&candidate).is_err() {
        return Ok(candidate);
    }
    final_link_target(&candidate)
}

/// `FileSystem.GetFinalLinkTarget`: open the link following it and ask the OS for the final path; when the target is
/// unreachable (dangling link) follow the chain by hand and return the last target, even if it does not exist.
fn final_link_target(link: &str) -> io::Result<String> {
    match fs::canonicalize(link) {
        Ok(final_path) => {
            let text = final_path.to_string_lossy().into_owned();
            // .NET trims the `\\?\` of GetFinalPathNameByHandle unless the link path itself was extended, then makes a
            // FileInfo of it (FullName).
            let trimmed = if is_extended(link) {
                text.as_str()
            } else {
                text.get(4..).unwrap_or(&text)
            };
            dotnet_path::get_full_path(trimmed).map_err(path_error)
        }
        Err(err) if is_path_unreachable(&err) => final_link_target_slow(link),
        // A loop (`a -> b -> a`, ERROR_CANT_RESOLVE_FILENAME) and every other failure: the caller fails closed (RV-S06).
        Err(err) => Err(err),
    }
}

/// `GetFinalLinkTargetSlow`: follow immediate targets until one is not a link, at most [`MAX_FOLLOWED_LINKS`] hops.
fn final_link_target_slow(link: &str) -> io::Result<String> {
    let mut previous = None;
    let mut current = immediate_link_target(link);
    let mut hops = 0usize;
    while let Some(target) = current {
        hops += 1;
        if hops > MAX_FOLLOWED_LINKS {
            // .NET throws an IOException with ERROR_CANT_RESOLVE_FILENAME (1921) here.
            return Err(io::Error::from_raw_os_error(ERROR_CANT_RESOLVE_FILENAME));
        }
        current = immediate_link_target(&target);
        previous = Some(target);
    }
    previous.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("Cannot resolve the link '{link}'.")))
}

/// `GetImmediateLinkTarget(path, returnFullPath: true, throwOnError: false)`: the target of one link as a full path
/// (relative targets resolve against the link's folder, the NT `\??\` prefix removed), `None` when `path` is not a
/// readable link.
fn immediate_link_target(path: &str) -> Option<String> {
    let attributes = entry_attributes(path)?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return None;
    }
    let target = fs::read_link(path).ok()?;
    let target = target.to_string_lossy();
    let target = strip_nt_prefix(&target);
    let combined = if dotnet_path::is_path_rooted(&target) {
        target
    } else {
        let folder = dotnet_path::get_directory_name_raw(path);
        if folder.is_empty() {
            target
        } else if folder.ends_with(is_sep) {
            format!("{folder}{target}")
        } else {
            format!("{folder}\\{target}")
        }
    };
    dotnet_path::get_full_path(&combined).ok()
}

/// `\??\C:\x` and `\\?\C:\x` => `C:\x`; `\??\UNC\s\x` and `\\?\UNC\s\x` => `\\s\x`.
fn strip_nt_prefix(target: &str) -> String {
    for prefix in [r"\??\", r"\\?\"] {
        if let Some(rest) = target.strip_prefix(prefix) {
            if let Some(unc) = rest.strip_prefix(r"UNC\") {
                return format!(r"\\{unc}");
            }
            return rest.to_owned();
        }
    }
    target.to_owned()
}

/// `PathInternal.IsExtended`: `\\?\` (exactly, backslashes only).
fn is_extended(path: &str) -> bool {
    path.starts_with(r"\\?\")
}

/// .NET `FileSystem.IsPathUnreachableError`.
fn is_path_unreachable(err: &io::Error) -> bool {
    matches!(
        err.raw_os_error(),
        Some(
            2      // ERROR_FILE_NOT_FOUND
            | 3    // ERROR_PATH_NOT_FOUND
            | 21   // ERROR_NOT_READY
            | 123  // ERROR_INVALID_NAME
            | 161  // ERROR_BAD_PATHNAME
            | 53   // ERROR_BAD_NETPATH
            | 67   // ERROR_BAD_NET_NAME
            | 87   // ERROR_INVALID_PARAMETER
            | 1231 // ERROR_NETWORK_UNREACHABLE
            | 65   // ERROR_NETWORK_ACCESS_DENIED
            | 6    // ERROR_INVALID_HANDLE
            | 206 // ERROR_FILENAME_EXCED_RANGE
        )
    )
}

/// P-12 classification of a failed metadata query: `true` = "nothing there" (`Ok(None)`), `false` = "cannot be
/// inspected" (`Err`). Missing = not found, every .NET "path unreachable" code ([`is_path_unreachable`]: missing
/// file/folder, bad name, missing network host or share, drive not ready, too long), ERROR_INVALID_FUNCTION (1, a
/// device such as `CON` that has no file information) and ERROR_DIRECTORY (267, a file used as a folder).
/// Access denied (5), sharing (32) and lock (33) violations and anything else are NOT missing.
pub fn is_stat_missing(err: &io::Error) -> bool {
    const ERROR_INVALID_FUNCTION: i32 = 1;
    const ERROR_DIRECTORY: i32 = 267;
    match err.raw_os_error() {
        Some(ERROR_INVALID_FUNCTION | ERROR_DIRECTORY) => true,
        Some(ERROR_NETWORK_ACCESS_DENIED) => false,
        Some(_) => is_path_unreachable(err),
        None => err.kind() == io::ErrorKind::NotFound,
    }
}

/// ERROR_NETWORK_ACCESS_DENIED: in .NET's "unreachable" list, but for a stat it is a denial, not an absence.
const ERROR_NETWORK_ACCESS_DENIED: i32 = 65;

// ---------------------------------------------------------------------------------------------------------------
// FileSystem
// ---------------------------------------------------------------------------------------------------------------

impl FileSystem for PhysicalFileSystem {
    // DIVERGENCE: C# throws ArgumentException for a blank path; the bool/Option methods return false/None instead.
    fn file_exists(&self, path: &Path) -> bool {
        if is_blank(path) {
            return false;
        }
        // File.Exists: full path; a trailing separator is false; any failure is false.
        match full_path(path) {
            Ok(full) => !full.ends_with(is_sep) && file_exists_full(&full),
            Err(_) => false,
        }
    }

    fn directory_exists(&self, path: &Path) -> bool {
        if is_blank(path) {
            return false;
        }
        full_path(path).is_ok_and(|full| directory_exists_full(&full))
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        if is_blank(path) {
            return None;
        }
        // new FileInfo(path): full path with ONE trailing separator trimmed (unlike File.Exists), attributes of the entry,
        // queried in the extended form when the trimmed name ends with a space or a dot (FillAttributeInfo): `b.txt \` is
        // `b.txt ` (missing), not `b.txt`.
        let full = full_path(path).ok()?;
        let trimmed = dotnet_path::trim_ending_directory_separator(&full);
        let metadata = fs::symlink_metadata(ensure_extended_prefix_if_needed(trimmed)).ok()?;
        if attributes_of(&metadata) & FILE_ATTRIBUTE_DIRECTORY != 0 {
            return None;
        }
        Some(stat_of(&metadata))
    }

    /// P-12: the same lookup as [`get_file_stat`](Self::get_file_stat), but a path that cannot be inspected is an `Err`.
    /// C# `GetFileStat` throws `ArgumentException` for a blank path and (from `Path.GetFullPath`) for a NUL: both are
    /// `Err(InvalidInput)` here. A name that cannot exist (wildcards, `|`, a stream, a device such as `CON`, a missing
    /// folder, drive or UNC host, a component too long) is `Ok(None)`, as in C# (`FileInfo.Exists` false; goldens
    /// `recovery-check` `p-*`). A whole path longer than the OS limit makes `Path.GetFullPath` throw
    /// `PathTooLongException` (an `IOException`), so it is an `Err` with raw OS error 206 (golden
    /// `p-path-40000-chars`: source Unreadable). Access denied, sharing/lock violations and other I/O failures are `Err` (the deliberate
    /// P-12 rule: "cannot tell" is never reported as "gone").
    fn try_get_file_stat(&self, path: &Path) -> io::Result<Option<FileStat>> {
        if is_blank(path) {
            return Err(blank_path_error());
        }
        // `new FileInfo(path)` runs Path.GetFullPath, which throws `ArgumentException` (blank/NUL) and
        // `PathTooLongException` (an `IOException`) when the resolved path is longer than the OS limit (golden
        // `recovery-check` `p-path-40000-chars`: source Unreadable). Both are `Err` here, never "not there".
        let full = full_path(path)?;
        let trimmed = dotnet_path::trim_ending_directory_separator(&full);
        match fs::symlink_metadata(ensure_extended_prefix_if_needed(trimmed)) {
            Ok(metadata) if attributes_of(&metadata) & FILE_ATTRIBUTE_DIRECTORY != 0 => Ok(None),
            Ok(metadata) => Ok(Some(stat_of(&metadata))),
            Err(err) if is_stat_missing(&err) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        if is_blank(source) || is_blank(destination) {
            return Err(blank_path_error());
        }
        move_impl(source, destination, false)
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        if is_blank(source) || is_blank(destination) {
            return Err(blank_path_error());
        }
        let source_full = full_path(source)?;
        let destination_full = full_path(destination)?;
        match win32::copy_file_no_overwrite(&win32::wide(&source_full), &win32::wide(&destination_full)) {
            Ok(()) => Ok(()),
            // FileSystem.CopyFile: access denied onto an existing directory is rethrown as a plain IOException (HResult
            // 5, not a Win32 HRESULT: golden "copy: onto an existing directory" has win32 0).
            Err(err) if err.raw_os_error() == Some(ERROR_ACCESS_DENIED) && directory_exists_full(&destination_full) => {
                Err(io::Error::new(
                    io::ErrorKind::IsADirectory,
                    format!("The target file '{destination_full}' is a directory, not a file."),
                ))
            }
            Err(err) => Err(err),
        }
    }

    fn try_copy_new_with_proof(
        &self,
        source: &Path,
        destination: &Path,
        proof: &CopyCreationProof,
    ) -> io::Result<bool> {
        if is_blank(source) || is_blank(destination) {
            return Err(blank_path_error());
        }
        match self.copy_file(source, destination) {
            Ok(()) => {
                // R01: raised only once the copy returned; a failed copy is never claimed as ours.
                proof.mark_created();
                Ok(true)
            }
            // A directory at the destination is not "a file already exists" whatever code the OS build reports.
            Err(err) if is_swallowed_as_destination_exists(&err) && !self.directory_exists(destination) => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        if is_blank(path) {
            return Err(blank_path_error());
        }
        let full = full_path(path)?;
        match win32::delete_file(&win32::wide(&full)) {
            Err(err) if err.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => Ok(()),
            other => other,
        }
    }

    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        if is_blank(path) {
            return Err(blank_path_error());
        }
        let mut shared = options(
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_FLAG_SEQUENTIAL_SCAN,
        );
        shared.read(true);
        let file = open_like_file_stream(&shared, path)?;
        if buffer_size > 1 {
            Ok(Box::new(BufReader::with_capacity(buffer_size, file)))
        } else {
            Ok(Box::new(file))
        }
    }

    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        if is_blank(path) {
            return Err(blank_path_error());
        }
        // FileMode.Append, FileAccess.Write, FileShare.Read.
        // DIVERGENCE: the C# FileStream buffers 4096 bytes (data and some errors surface at Flush/Dispose); the raw File
        // writes through to the OS at every call. Callers write whole lines, so the file content is the same.
        // P-11: the boxed `File` answers `as_file_mut()` with itself (C# `stream is FileStream`), so the journal can
        // flush it to disk in PowerLossSafe mode.
        let mut append = options(FILE_SHARE_READ, if durable { FILE_FLAG_WRITE_THROUGH } else { 0 });
        append.append(true).create(true);
        let file = open_like_file_stream(&append, path)?;
        Ok(Box::new(file))
    }

    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        if is_blank(path) {
            return Err(blank_path_error());
        }
        let target = path_str(path)?;
        let dir = dotnet_path::get_directory_name_raw(target);
        if !dir.is_empty() {
            // Directory.CreateDirectory(dir), not create_dir_all: a dangling junction counts as an existing folder.
            create_directory_impl(Path::new(dir))?;
        }

        let temp = format!("{target}.{}.tmp", new_guid_n());
        let temp_path = Path::new(&temp);
        let result = (|| {
            // new FileStream(tempPath, ...) opens Path.GetFullPath(tempPath).
            let mut file = options(FILE_SHARE_NONE, if durable { FILE_FLAG_WRITE_THROUGH } else { 0 })
                .write(true)
                .create_new(true)
                .open(full_path(temp_path)?)?;
            file.write_all(text.as_bytes())?;
            if durable {
                file.sync_all()?;
            }
            drop(file);
            // File.Move(temp, path, overwrite: true), retried on transient contention.
            replace_with_retry(
                || move_impl(temp_path, path, true),
                |ms| std::thread::sleep(Duration::from_millis(u64::from(ms))),
            )
        })();

        // finally: best-effort temp cleanup; the original failure is reported.
        if self.file_exists(temp_path) {
            let _ = self.delete(temp_path);
        }
        result
    }

    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        if is_blank(path) {
            return Err(blank_path_error());
        }
        // File.ReadAllText shares READ only: it fails while any other handle has the file open for writing.
        let mut read = options(FILE_SHARE_READ, 0);
        read.read(true);
        let mut file = open_like_file_stream(&read, path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(decode_text_like_dotnet(&bytes))
    }

    // DIVERGENCE: C# returns a lazy IEnumerable (a missing folder throws at the first MoveNext); this returns a Vec, so
    // the error comes from the call itself. Callers enumerate immediately.
    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        if is_blank(directory) {
            return Err(blank_path_error());
        }
        // A pattern with a separator (C# splits it into folder + pattern, UNPINNED), a rooted pattern or a NUL is out of
        // scope: InvalidInput.
        let mut chars = pattern.chars();
        let rooted = matches!((chars.next(), chars.next()), (Some(d), Some(':')) if d.is_ascii_alphabetic());
        if pattern.contains(is_sep) || pattern.contains('\0') || rooted {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("The search pattern '{pattern}' is not supported."),
            ));
        }
        let expression: Vec<u16> = pattern.encode_utf16().collect();
        list_entries(directory, false, |name| {
            matches_win32_expression_units(&expression, name)
        })
    }

    fn enumerate_files_with_stat(
        &self,
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        Self::enumerate_files_with_stat_from(directory, include, on_skipped, &open_physical_listing)
    }

    fn try_probe_readable(&self, path: &Path) -> Result<(), String> {
        if is_blank(path) {
            return Err(blank_path_error().to_string());
        }
        // AR16: one read-open sharing ReadWrite|Delete, no bytes read.
        let mut probe = options(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, 0);
        probe.read(true);
        match open_like_file_stream(&probe, path) {
            Ok(_) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        if is_blank(directory) {
            return Err(blank_path_error());
        }
        list_entries(directory, true, |_| true)
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        if is_blank(path) {
            return Err(blank_path_error());
        }
        create_directory_impl(path)
    }

    fn try_delete_empty_directory(&self, path: &Path) -> bool {
        if is_blank(path) {
            return false;
        }
        // Non-recursive RemoveDirectory refuses a non-empty directory atomically (no check-then-delete race); a junction
        // is removed as a link only.
        fs::remove_dir(path).is_ok()
    }

    fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> {
        if is_blank(path) {
            return Err(blank_path_error());
        }
        let full = full_path(path)?;
        let root = &full[..dotnet_path::root_length(&full)];
        if root.is_empty() {
            return Ok(PathBuf::from(full));
        }
        let segments: Vec<&str> = full[root.len()..].split(is_sep).filter(|s| !s.is_empty()).collect();
        // A bare root ("C:\") has nothing to resolve; trimming it would hand back the drive-relative "C:" instead.
        if segments.is_empty() {
            return Ok(PathBuf::from(root));
        }
        let mut current = root.trim_end_matches(is_sep).to_owned();
        for segment in segments {
            let candidate = if current.is_empty() {
                segment.to_owned()
            } else {
                format!("{current}\\{segment}")
            };
            current = resolve_if_reparse_point(candidate)?;
        }
        Ok(PathBuf::from(current))
    }
}
