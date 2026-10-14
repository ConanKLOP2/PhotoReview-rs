//! Port of `PhotoReview.Core/Instance/ForwardedPathProtocol.cs` (C# baseline d92d6aae, 89 lines): the Q-R10 wire
//! format a second launch uses to hand its file paths to the running instance. UTF-8 lines `PHOTOREVIEW-OPEN 1`,
//! one absolute path per line, then an empty line.
//!
//! The receiver treats every byte as untrusted: size, count, encoding, shape and existence are all checked before
//! any path is used. [`encode`] applies the exact same acceptability contract as [`try_decode`], so a message that
//! `encode` produces can never be rejected by the owner (SEC-02).
//!
//! The pipe itself (named pipe, ACL, `PhotoReviewRs` names per docs/adr/R0002) is Wave 6 (`pr-platform-win`).
//!
//! Unit W2-19. Contract: `contracts/W2-19.md`.

use std::fmt;

/// First line of every message.
pub const HEADER: &str = "PHOTOREVIEW-OPEN 1";

/// Largest message in bytes (64 KiB), inclusive.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

/// Most paths in one message, inclusive.
pub const MAX_PATHS: usize = 16;

/// Longest path in UTF-16 code units, inclusive (C# `string.Length`; an astral character counts 2).
pub const MAX_PATH_CHARS: usize = 32767;

/// Why [`encode`] refused. C#: `ArgumentException` with `ParamName == "paths"` in every case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// More than [`MAX_PATHS`] paths ("Too many paths.").
    TooManyPaths,
    /// A path that [`is_acceptable_path`] rejects ("Path is not acceptable for forwarding: '{path}'.").
    PathNotAcceptable(String),
    /// The encoded message is longer than [`MAX_MESSAGE_BYTES`] ("Message too large.").
    MessageTooLarge,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::TooManyPaths => f.write_str("Too many paths."),
            EncodeError::PathNotAcceptable(path) => write!(f, "Path is not acceptable for forwarding: '{path}'."),
            EncodeError::MessageTooLarge => f.write_str("Message too large."),
        }
    }
}

impl std::error::Error for EncodeError {}

/// `Encode(paths)`: `HEADER\n` + each path + `\n` + a final `\n`, as UTF-8. Checks, in this order: more than
/// [`MAX_PATHS`] paths; each path in turn with [`is_acceptable_path`]; total size. An empty list is valid (a
/// plain "bring to front" request).
pub fn encode(paths: &[String]) -> Result<Vec<u8>, EncodeError> {
    if paths.len() > MAX_PATHS {
        return Err(EncodeError::TooManyPaths);
    }
    let mut text = String::from(HEADER);
    text.push('\n');
    for path in paths {
        // SEC-02: the exact acceptability contract of the receiver.
        if !is_acceptable_path(path) {
            return Err(EncodeError::PathNotAcceptable(path.clone()));
        }
        text.push_str(path);
        text.push('\n');
    }
    text.push('\n');
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(EncodeError::MessageTooLarge);
    }
    Ok(text.into_bytes())
}

/// `IsComplete(message)`: the message ends with the empty terminator line, i.e. it has at least 2 bytes and the last
/// two bytes are both `\n`.
pub fn is_complete(message: &[u8]) -> bool {
    message.len() >= 2 && message[message.len() - 1] == b'\n' && message[message.len() - 2] == b'\n'
}

/// `TryDecode(message, pathExists, out paths)`: `Some(paths)` when the message is complete, strictly valid UTF-8,
/// has the right header, at most [`MAX_PATHS`] paths, and every path is acceptable AND `path_exists` returns true
/// for it. `path_exists` is called in order and the decode stops at the first path that fails either check. An empty
/// path list is `Some(vec![])`. `None` is the C# `false` (C# leaves `paths` empty).
pub fn try_decode(message: &[u8], mut path_exists: impl FnMut(&str) -> bool) -> Option<Vec<String>> {
    if message.is_empty() || message.len() > MAX_MESSAGE_BYTES || !is_complete(message) {
        return None;
    }
    let text = std::str::from_utf8(message).ok()?;
    // The message ends with two '\n' (ASCII), so cutting two bytes is on a char boundary.
    let body = &text[..text.len() - 2];
    let lines: Vec<&str> = body.split('\n').collect();
    if lines[0] != HEADER || lines.len() - 1 > MAX_PATHS {
        return None;
    }
    let mut result = Vec::with_capacity(lines.len() - 1);
    for line in &lines[1..] {
        if !is_acceptable_path(line) || !path_exists(line) {
            return None;
        }
        result.push((*line).to_owned());
    }
    Some(result)
}

/// C# private `IsAcceptablePath`: non-empty, at most [`MAX_PATH_CHARS`] UTF-16 units, no control character
/// (`char::is_control`), not a device/extended/NT-object prefix (any of `\\?\`, `\\.\`, `\??\` and their `/`
/// spellings), no `..` segment (split on `\` and `/`), and [`is_path_fully_qualified`].
///
/// RV-S05 (lone surrogates) cannot occur in a `&str`; the byte-level equivalent is the strict UTF-8 decode.
pub fn is_acceptable_path(path: &str) -> bool {
    if path.is_empty() || path.encode_utf16().count() > MAX_PATH_CHARS {
        return false;
    }
    if path.chars().any(char::is_control) {
        return false;
    }
    let sep = |c: char| c == '\\' || c == '/';
    let mut chars = path.chars();
    if let (Some(c0), Some(c1), Some(c2)) = (chars.next(), chars.next(), chars.next())
        && sep(c0)
        && (sep(c1) || c1 == '?')
        && (c2 == '?' || c2 == '.')
    {
        return false;
    }
    if path.split(['\\', '/']).any(|segment| segment == "..") {
        return false;
    }
    is_path_fully_qualified(path)
}

/// .NET `Path.IsPathFullyQualified` on Windows; delegates to the shared W2-00 implementation
/// (`text::dotnet_path`), which has the same rules.
pub fn is_path_fully_qualified(path: &str) -> bool {
    crate::text::dotnet_path::is_path_fully_qualified(path)
}
