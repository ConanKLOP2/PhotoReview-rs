//! Port of `PhotoReview.Platform.Windows/WindowsRecycleBin.cs`: recycle-item candidate matching.
//!
//! Matching is deliberately metadata-only. Callers must require exactly one matching item before restoring it.

use std::path::Path;

use pr_core::{text::eq_ignore_case, time::DateTimeUtc};

const SHELL_TIMESTAMP_TOLERANCE_TICKS: u64 = 10_000_000;

/// Metadata exposed by a Shell Recycle Bin item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecycleCandidate {
    pub deleted_from: Option<String>,
    pub name: Option<String>,
    pub size: i64,
    pub last_write_utc: DateTimeUtc,
    pub alternate_last_write_utc: Option<DateTimeUtc>,
}

impl RecycleCandidate {
    pub fn new(
        deleted_from: Option<String>,
        name: Option<String>,
        size: i64,
        last_write_utc: DateTimeUtc,
        alternate_last_write_utc: Option<DateTimeUtc>,
    ) -> Self {
        Self {
            deleted_from,
            name,
            size,
            last_write_utc,
            alternate_last_write_utc,
        }
    }
}

/// Per-item failures that C# skips while continuing to inspect other Recycle Bin entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecycleItemFailureKind {
    Com,
    InvalidCast,
    Format,
    Other,
}

/// C# `RecycleItemFailure.IsPerItem`: unrelated failures abort enumeration.
pub const fn is_per_item_failure(kind: RecycleItemFailureKind) -> bool {
    matches!(
        kind,
        RecycleItemFailureKind::Com | RecycleItemFailureKind::InvalidCast | RecycleItemFailureKind::Format
    )
}

/// C# `RecycleCandidateSelector.IsMatch`; callers enforce uniqueness over all matching candidates.
pub fn is_match(
    candidate: &RecycleCandidate,
    original_path: &Path,
    expected_size: i64,
    expected_last_write_utc: DateTimeUtc,
) -> bool {
    let Some(deleted_from) = candidate.deleted_from.as_deref().filter(|s| !s.is_empty()) else {
        return false;
    };
    let Some(name) = candidate.name.as_deref().filter(|s| !s.is_empty()) else {
        return false;
    };
    let Some(original_folder) = original_path.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return false;
    };
    let Some(file_name) = original_path.file_name() else {
        return false;
    };
    let file_name = file_name.to_string_lossy();

    if !eq_ignore_case(
        trim_ending_separator(deleted_from),
        trim_ending_separator(&original_folder.to_string_lossy()),
    ) || !name_matches(name, &file_name)
        || candidate.size != expected_size
    {
        return false;
    }

    timestamp_matches(candidate.last_write_utc, expected_last_write_utc)
        || candidate
            .alternate_last_write_utc
            .is_some_and(|alternate| timestamp_matches(alternate, expected_last_write_utc))
}

fn trim_ending_separator(path: &str) -> &str {
    // Preserve a root such as `C:\` while removing redundant trailing separators elsewhere.
    let bytes = path.as_bytes();
    let mut end = bytes.len();
    while end > 1 && matches!(bytes[end - 1], b'\\' | b'/') {
        if end == 3 && bytes[1] == b':' {
            break;
        }
        end -= 1;
    }
    &path[..end]
}

fn name_matches(display_name: &str, file_name: &str) -> bool {
    if eq_ignore_case(display_name, file_name) {
        return true;
    }
    let Some(dot) = file_name.rfind('.').filter(|dot| *dot > 0) else {
        return false;
    };
    eq_ignore_case(display_name, &file_name[..dot])
}

fn timestamp_matches(shell: DateTimeUtc, expected: DateTimeUtc) -> bool {
    shell.ticks().abs_diff(expected.ticks()) < SHELL_TIMESTAMP_TOLERANCE_TICKS
}
