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
//! Unit W2-19. Stub: bodies are `todo!()`; data definitions are complete. Contract: `contracts/W2-19.md`.

// Stub-only (docs/MULTI-AGENT-PLAN D11): removed by the porter in the commit that removes the last `todo!()`.
#![allow(unused_variables, dead_code)]

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
    todo!("see contracts/W2-19.md section 'encode'")
}

/// `IsComplete(message)`: the message ends with the empty terminator line, i.e. it has at least 2 bytes and the last
/// two bytes are both `\n`.
pub fn is_complete(message: &[u8]) -> bool {
    todo!()
}

/// `TryDecode(message, pathExists, out paths)`: `Some(paths)` when the message is complete, strictly valid UTF-8,
/// has the right header, at most [`MAX_PATHS`] paths, and every path is acceptable AND `path_exists` returns true
/// for it. `path_exists` is called in order and the decode stops at the first path that fails either check. An empty
/// path list is `Some(vec![])`. `None` is the C# `false` (C# leaves `paths` empty).
pub fn try_decode(message: &[u8], path_exists: impl FnMut(&str) -> bool) -> Option<Vec<String>> {
    todo!("see contracts/W2-19.md section 'try_decode'")
}

/// C# private `IsAcceptablePath`: non-empty, at most [`MAX_PATH_CHARS`] UTF-16 units, no control character
/// (`char::is_control`), not a device/extended/NT-object prefix (any of `\\?\`, `\\.\`, `\??\` and their `/`
/// spellings), no `..` segment (split on `\` and `/`), and [`is_path_fully_qualified`].
pub fn is_acceptable_path(path: &str) -> bool {
    todo!()
}

/// .NET `Path.IsPathFullyQualified` on Windows (the rules of `PathInternal.IsPartiallyQualified` negated), for UTF-16
/// strings: false for fewer than 2 chars; when the first char is `\` or `/`: true unless the second char is neither
/// a separator nor `?` (so `\\x`, `//x`, `\?x`, `\??\...` are fully qualified; `\x` and `/x` are not); otherwise true
/// only for `<ASCII letter>:<\ or />...` (length >= 3). Hoist candidate for a shared `paths` module (README D8).
pub fn is_path_fully_qualified(path: &str) -> bool {
    todo!()
}
