//! Port of `PhotoReview.App.Shared/Presenters/RecoveryPresentation.cs` (237 lines): the pure mapping from
//! `RecoveryCheckResult` / `JournalEntry` to localized text and colour tones of the Recovery window. No UI type.
//!
//! Texts: `pr_l10n::keys::*` constants through `Localizer::format` (D-W8-4; never `tr::` accessors, which read a
//! global localizer). Time: `RecoveryTimeFormatter`, implemented by `pr_platform_win` (contract D-10f-3).
//! Digit grouping of `N0`: `RecoveryTexts::group_separator` (contract D-10f-4).
//! Contract: `contracts/W8-10f.md`. Lead-owned `presenters/mod.rs` declares this module.
use pr_core::file_actions::errors::CANCELLED_BY_USER;
use pr_core::file_actions::journal_line::{JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_check::{
    RecoveryCheckResult, RecoveryPathCheck, RecoveryPathStatus, RecoveryVerdict,
};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::text::dotnet_path::{get_directory_name, get_file_name};
use pr_core::time::DateTimeUtc;
use pr_l10n::keys;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;

use super::recovery_filter::{RecoveryFilter, RecoveryTone};

/// C# `RecoveryPathView`: what the details panel shows for one path (source or destination).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryPathView {
    pub title: String,
    pub path: String,
    pub status_text: String,
    pub status_tone: RecoveryTone,
    pub now_text: String,
    pub journal_text: String,
    /// C# `string? Note`: the folder-missing text, or `None`.
    pub note: Option<String>,
}

/// C# `RecoveryGroupMemberView`: one capture-group member line of the details panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryGroupMemberView {
    pub status: String,
    pub source: String,
    pub destination: String,
}

/// Local-time text of a stamp: C# `DateTime.SpecifyKind(utc, Utc).ToLocalTime().ToString("g", CurrentCulture)`.
/// The implementation owns the time zone and the culture (Windows); tests use a fake that returns a fixed text.
pub trait RecoveryTimeFormatter {
    fn format_local_general(&self, utc: DateTimeUtc) -> String;
}

/// The inputs every text function needs: the catalog, the time formatter and the digit-group separator of `N0`.
pub struct RecoveryTexts<'a> {
    pub loc: &'a Localizer,
    pub time: &'a dyn RecoveryTimeFormatter,
    pub group_separator: &'a str,
}

impl RecoveryTexts<'_> {
    /// C# `VerdictText`: `pr_l10n::keys::RECOVERY_VERDICT_*`; unknown -> `RECOVERY_VERDICT_UNKNOWN`.
    pub fn verdict_text(&self, verdict: RecoveryVerdict) -> String {
        let key = match verdict {
            RecoveryVerdict::CanRetry => keys::RECOVERY_VERDICT_CAN_RETRY,
            RecoveryVerdict::AlreadyDone => keys::RECOVERY_VERDICT_ALREADY_DONE,
            RecoveryVerdict::Conflict => keys::RECOVERY_VERDICT_CONFLICT,
            RecoveryVerdict::SourceChanged => keys::RECOVERY_VERDICT_SOURCE_CHANGED,
            RecoveryVerdict::DestinationChanged => keys::RECOVERY_VERDICT_DESTINATION_CHANGED,
            RecoveryVerdict::Lost => keys::RECOVERY_VERDICT_LOST,
            RecoveryVerdict::RecycleUnverifiable => keys::RECOVERY_VERDICT_RECYCLE_UNVERIFIABLE,
            RecoveryVerdict::NotRecycled => keys::RECOVERY_VERDICT_NOT_RECYCLED,
            RecoveryVerdict::PermanentlyDeleted => keys::RECOVERY_VERDICT_PERMANENTLY_DELETED,
            RecoveryVerdict::PartiallyPermanentlyDeleted => keys::RECOVERY_VERDICT_PARTIALLY_PERMANENTLY_DELETED,
            RecoveryVerdict::Unknown => keys::RECOVERY_VERDICT_UNKNOWN,
        };
        self.loc.format(key, &[])
    }

    /// C# `ExplainText`: `RECOVERY_EXPLAIN_*`; unknown -> `RECOVERY_EXPLAIN_UNKNOWN`.
    pub fn explain_text(&self, verdict: RecoveryVerdict) -> String {
        let key = match verdict {
            RecoveryVerdict::CanRetry => keys::RECOVERY_EXPLAIN_CAN_RETRY,
            RecoveryVerdict::AlreadyDone => keys::RECOVERY_EXPLAIN_ALREADY_DONE,
            RecoveryVerdict::Conflict => keys::RECOVERY_EXPLAIN_CONFLICT,
            RecoveryVerdict::SourceChanged => keys::RECOVERY_EXPLAIN_SOURCE_CHANGED,
            RecoveryVerdict::DestinationChanged => keys::RECOVERY_EXPLAIN_DESTINATION_CHANGED,
            RecoveryVerdict::Lost => keys::RECOVERY_EXPLAIN_LOST,
            RecoveryVerdict::RecycleUnverifiable => keys::RECOVERY_EXPLAIN_RECYCLE_UNVERIFIABLE,
            RecoveryVerdict::NotRecycled => keys::RECOVERY_EXPLAIN_NOT_RECYCLED,
            RecoveryVerdict::PermanentlyDeleted => keys::RECOVERY_EXPLAIN_PERMANENTLY_DELETED,
            RecoveryVerdict::PartiallyPermanentlyDeleted => keys::RECOVERY_EXPLAIN_PARTIALLY_PERMANENTLY_DELETED,
            RecoveryVerdict::Unknown => keys::RECOVERY_EXPLAIN_UNKNOWN,
        };
        self.loc.format(key, &[])
    }

    /// C# `ActionText`: `RECOVERY_ACTION_*`; unknown -> `RECOVERY_ACTION_UNKNOWN`.
    pub fn action_text(&self, verdict: RecoveryVerdict) -> String {
        let key = match verdict {
            RecoveryVerdict::CanRetry => keys::RECOVERY_ACTION_CAN_RETRY,
            RecoveryVerdict::AlreadyDone => keys::RECOVERY_ACTION_ALREADY_DONE,
            RecoveryVerdict::Conflict => keys::RECOVERY_ACTION_CONFLICT,
            RecoveryVerdict::SourceChanged => keys::RECOVERY_ACTION_SOURCE_CHANGED,
            RecoveryVerdict::DestinationChanged => keys::RECOVERY_ACTION_DESTINATION_CHANGED,
            RecoveryVerdict::Lost => keys::RECOVERY_ACTION_LOST,
            RecoveryVerdict::RecycleUnverifiable => keys::RECOVERY_ACTION_RECYCLE_UNVERIFIABLE,
            RecoveryVerdict::NotRecycled => keys::RECOVERY_ACTION_NOT_RECYCLED,
            RecoveryVerdict::PermanentlyDeleted => keys::RECOVERY_ACTION_PERMANENTLY_DELETED,
            RecoveryVerdict::PartiallyPermanentlyDeleted => keys::RECOVERY_ACTION_PARTIALLY_PERMANENTLY_DELETED,
            RecoveryVerdict::Unknown => keys::RECOVERY_ACTION_UNKNOWN,
        };
        self.loc.format(key, &[])
    }

    /// C# `PathStatusText`: `RECOVERY_PATH_STATUS_*`; `Unreadable` and any other value -> `RECOVERY_PATH_STATUS_UNREADABLE`.
    pub fn path_status_text(&self, status: RecoveryPathStatus) -> String {
        let key = match status {
            RecoveryPathStatus::Exists => keys::RECOVERY_PATH_STATUS_EXISTS,
            RecoveryPathStatus::Missing => keys::RECOVERY_PATH_STATUS_MISSING,
            RecoveryPathStatus::Changed => keys::RECOVERY_PATH_STATUS_CHANGED,
            RecoveryPathStatus::Unreadable => keys::RECOVERY_PATH_STATUS_UNREADABLE,
        };
        self.loc.format(key, &[])
    }

    /// C# `FormatSize`: `RECOVERY_DETAIL_BYTES` with `{size}` = the decimal digits of `bytes` grouped by
    /// `group_separator` every three digits (`N0`), the minus sign of the culture (`-`) in front when negative.
    pub fn format_size(&self, bytes: i64) -> String {
        let digits = bytes.unsigned_abs().to_string();
        let first = digits.len() % 3;
        let mut grouped = String::new();
        if bytes < 0 {
            grouped.push('-');
        }
        for (index, ch) in digits.chars().enumerate() {
            if index > 0 && (index == first || (index > first && (index - first) % 3 == 0)) {
                grouped.push_str(self.group_separator);
            }
            grouped.push(ch);
        }
        self.loc
            .format(keys::RECOVERY_DETAIL_BYTES, &[LocArg::new("size", grouped)])
    }

    /// C# `FormatTime`: `self.time.format_local_general(utc)`.
    pub fn format_time(&self, utc: DateTimeUtc) -> String {
        self.time.format_local_general(utc)
    }

    /// C# `NowText`: missing -> `RECOVERY_DETAIL_NOW_MISSING`; unreadable -> `RECOVERY_DETAIL_NOW_UNREADABLE`;
    /// otherwise `RECOVERY_DETAIL_NOW_PRESENT` with the current size (0 when absent) and the current write time
    /// (`DateTimeUtc::default()` when absent, C# `?? default`).
    pub fn now_text(&self, check: &RecoveryPathCheck) -> String {
        match check.status {
            RecoveryPathStatus::Missing => self.loc.format(keys::RECOVERY_DETAIL_NOW_MISSING, &[]),
            RecoveryPathStatus::Unreadable => self.loc.format(keys::RECOVERY_DETAIL_NOW_UNREADABLE, &[]),
            _ => self.loc.format(
                keys::RECOVERY_DETAIL_NOW_PRESENT,
                &[
                    LocArg::new("size", self.format_size(check.current_size.unwrap_or(0))),
                    LocArg::new(
                        "time",
                        self.format_time(check.current_last_write_utc.unwrap_or_default()),
                    ),
                ],
            ),
        }
    }

    /// C# `JournalText`: `RECOVERY_DETAIL_JOURNAL` with the journaled size and `last_write_utc.value`.
    pub fn journal_text(&self, entry: &JournalEntry) -> String {
        self.loc.format(
            keys::RECOVERY_DETAIL_JOURNAL,
            &[
                LocArg::new("size", self.format_size(entry.size)),
                LocArg::new("time", self.format_time(entry.last_write_utc.value)),
            ],
        )
    }

    /// C# `PathView`. `folder_missing_note` -> `note = Some(RECOVERY_DETAIL_FOLDER_MISSING)`.
    /// `status_text` = `RECOVERY_DETAIL_STATUS` with `{status}` = `path_status_text`.
    pub fn path_view(
        &self,
        title: &str,
        check: &RecoveryPathCheck,
        entry: &JournalEntry,
        folder_missing_note: bool,
    ) -> RecoveryPathView {
        RecoveryPathView {
            title: title.to_owned(),
            path: check.path.clone(),
            status_text: self.loc.format(
                keys::RECOVERY_DETAIL_STATUS,
                &[LocArg::new("status", self.path_status_text(check.status))],
            ),
            status_tone: path_status_tone(check.status),
            now_text: self.now_text(check),
            journal_text: self.journal_text(entry),
            note: folder_missing_note.then(|| self.loc.format(keys::RECOVERY_DETAIL_FOLDER_MISSING, &[])),
        }
    }

    /// C# `GroupStatusText`: the state word by verdict (`RECOVERY_GROUP_*`, other verdicts -> `verdict_text`), then,
    /// only when `member.destination` is `Some`, ` · {RECOVERY_DETAIL_SOURCE}: {source status}` and
    /// ` · {RECOVERY_DETAIL_DESTINATION}: {destination status or Missing}`.
    pub fn group_status_text(
        &self,
        verdict: RecoveryVerdict,
        member: &JournalGroupMember,
        source: &RecoveryPathCheck,
        destination: Option<&RecoveryPathCheck>,
    ) -> String {
        let mut text = match verdict {
            RecoveryVerdict::AlreadyDone => self.loc.format(keys::RECOVERY_GROUP_EXISTS, &[]),
            RecoveryVerdict::CanRetry => self.loc.format(keys::RECOVERY_GROUP_SOURCE_STATUS, &[]),
            RecoveryVerdict::NotRecycled => self.loc.format(keys::RECOVERY_GROUP_ON_DISK, &[]),
            RecoveryVerdict::RecycleUnverifiable => self.loc.format(keys::RECOVERY_GROUP_MISSING, &[]),
            RecoveryVerdict::SourceChanged | RecoveryVerdict::DestinationChanged => {
                self.loc.format(keys::RECOVERY_GROUP_CHANGED, &[])
            }
            RecoveryVerdict::Conflict => self.loc.format(keys::RECOVERY_GROUP_CONFLICT, &[]),
            RecoveryVerdict::Lost => self.loc.format(keys::RECOVERY_GROUP_LOST, &[]),
            RecoveryVerdict::Unknown => self.loc.format(keys::RECOVERY_GROUP_UNREADABLE, &[]),
            _ => self.verdict_text(verdict),
        };
        if member.destination.is_some() {
            let src = self.loc.format(keys::RECOVERY_DETAIL_SOURCE, &[]);
            let dst = self.loc.format(keys::RECOVERY_DETAIL_DESTINATION, &[]);
            text.push_str(&format!(
                " · {src}: {} · {dst}: {}",
                self.path_status_text(source.status),
                self.path_status_text(destination.map_or(RecoveryPathStatus::Missing, |v| v.status))
            ));
        }
        text
    }

    /// C# `RetryConfirmText` (the Retry button confirmation). Branch order, first match wins:
    /// 1. `error_code == CANCELLED_BY_USER` (ordinal): count = members.len() or 1; if the entry is a group Delete
    ///    (`Recycle`, `undo != Some(true)`), `allow_permanent_delete`, and `pending_permanent_count > 0`, the
    ///    `RECOVERY_RETRY_CONFIRM_CANCELLED_PERMANENT` text; else `RECOVERY_RETRY_CONFIRM_CANCELLED`.
    /// 2. no non-empty group members: `DIALOG_CONFIRM_RETRY_MESSAGE` with `{fileName}` = `get_file_name(source)`.
    /// 3. group `Recycle` with `undo == Some(true)`: `RECOVERY_GROUP_RETRY_CONFIRM_RESTORE` ({count}).
    /// 4. group `Recycle` and `allow_permanent_delete` and `pending_permanent_count > 0`:
    ///    `RECOVERY_GROUP_RETRY_CONFIRM_PERMANENT` ({operation}, {count}, {permanentCount}).
    /// 5. `RECOVERY_GROUP_RETRY_CONFIRM` ({operation}, {count}).
    pub fn retry_confirm_text(
        &self,
        entry: &JournalEntry,
        check: Option<&RecoveryCheckResult>,
        allow_permanent_delete: bool,
    ) -> String {
        let members = entry.group_members.as_deref().unwrap_or(&[]);
        let count = if members.is_empty() { 1 } else { members.len() } as i64;
        let candidates: Vec<&JournalGroupMember> = check
            .and_then(|c| c.group_members.as_ref())
            .map(|items| {
                items
                    .iter()
                    .filter(|x| x.check.verdict == RecoveryVerdict::NotRecycled)
                    .map(|x| &x.member)
                    .collect()
            })
            .unwrap_or_else(|| members.iter().collect());
        let permanent = candidates.iter().filter(|m| m.permanent).count() as i64;
        let is_delete = entry.op_type == FileOperationType::Recycle && entry.undo != Some(true);
        if entry.error_code.as_deref() == Some(CANCELLED_BY_USER) {
            return if is_delete && allow_permanent_delete && permanent > 0 {
                self.loc.format(
                    keys::RECOVERY_RETRY_CONFIRM_CANCELLED_PERMANENT,
                    &[
                        LocArg::new("operation", self.operation_text(entry.op_type)),
                        LocArg::new("count", count),
                        LocArg::new("permanentCount", permanent),
                    ],
                )
            } else {
                self.loc.format(
                    keys::RECOVERY_RETRY_CONFIRM_CANCELLED,
                    &[
                        LocArg::new("operation", self.operation_text(entry.op_type)),
                        LocArg::new("count", count),
                    ],
                )
            };
        }
        if members.is_empty() {
            return self.loc.format(
                keys::DIALOG_CONFIRM_RETRY_MESSAGE,
                &[
                    LocArg::new("operation", self.operation_text(entry.op_type)),
                    LocArg::new("fileName", get_file_name(&entry.source)),
                ],
            );
        }
        if entry.op_type == FileOperationType::Recycle && entry.undo == Some(true) {
            return self.loc.format(
                keys::RECOVERY_GROUP_RETRY_CONFIRM_RESTORE,
                &[LocArg::new("count", count)],
            );
        }
        if is_delete && allow_permanent_delete && permanent > 0 {
            return self.loc.format(
                keys::RECOVERY_GROUP_RETRY_CONFIRM_PERMANENT,
                &[
                    LocArg::new("operation", self.operation_text(entry.op_type)),
                    LocArg::new("count", count),
                    LocArg::new("permanentCount", permanent),
                ],
            );
        }
        self.loc.format(
            keys::RECOVERY_GROUP_RETRY_CONFIRM,
            &[
                LocArg::new("operation", self.operation_text(entry.op_type)),
                LocArg::new("count", count),
            ],
        )
    }

    /// C# `OperationText`: `ENUM_FILE_OPERATION_MOVE` / `_COPY` / `_RECYCLE`.
    pub fn operation_text(&self, op: FileOperationType) -> String {
        self.loc.format(
            match op {
                FileOperationType::Move => keys::ENUM_FILE_OPERATION_MOVE,
                FileOperationType::Copy => keys::ENUM_FILE_OPERATION_COPY,
                FileOperationType::Recycle => keys::ENUM_FILE_OPERATION_RECYCLE,
            },
            &[],
        )
    }

    /// C# `StateText`: `ENUM_JOURNAL_STATE_PREPARED` / `_COMMITTED` / `_FAILED`; `Dismissed` (no case in C#, the
    /// default `ToString()`) -> the literal `"Dismissed"`.
    pub fn state_text(&self, state: JournalState) -> String {
        match state {
            JournalState::Prepared => self.loc.format(keys::ENUM_JOURNAL_STATE_PREPARED, &[]),
            JournalState::Committed => self.loc.format(keys::ENUM_JOURNAL_STATE_COMMITTED, &[]),
            JournalState::Failed => self.loc.format(keys::ENUM_JOURNAL_STATE_FAILED, &[]),
            JournalState::Dismissed => "Dismissed".to_owned(),
        }
    }
}

/// C# `Rgb`: the documented hint colour (0xRRGGBB) of a tone. `Neutral` and any other value -> (0xB0, 0xB0, 0xB0).
pub fn rgb(tone: RecoveryTone) -> (u8, u8, u8) {
    match tone {
        RecoveryTone::Good => (0x6c, 0xcb, 0x7a),
        RecoveryTone::Info => (0x6f, 0xb1, 0xff),
        RecoveryTone::Warn => (0xf0, 0xb0, 0x40),
        RecoveryTone::Bad => (0xff, 0x6b, 0x6b),
        RecoveryTone::Neutral => (0xb0, 0xb0, 0xb0),
    }
}

/// C# `VerdictTone`. `CanRetry` -> Good; `AlreadyDone` -> Info; `Conflict`, `SourceChanged`, `DestinationChanged`,
/// `NotRecycled`, `PartiallyPermanentlyDeleted` -> Warn; `PermanentlyDeleted`, `Lost` -> Bad; else Neutral.
pub fn verdict_tone(verdict: RecoveryVerdict) -> RecoveryTone {
    match verdict {
        RecoveryVerdict::CanRetry => RecoveryTone::Good,
        RecoveryVerdict::AlreadyDone => RecoveryTone::Info,
        RecoveryVerdict::Conflict
        | RecoveryVerdict::SourceChanged
        | RecoveryVerdict::DestinationChanged
        | RecoveryVerdict::NotRecycled
        | RecoveryVerdict::PartiallyPermanentlyDeleted => RecoveryTone::Warn,
        RecoveryVerdict::PermanentlyDeleted | RecoveryVerdict::Lost => RecoveryTone::Bad,
        RecoveryVerdict::RecycleUnverifiable | RecoveryVerdict::Unknown => RecoveryTone::Neutral,
    }
}

/// C# `PathStatusTone`. `Exists` -> Good; `Changed` -> Warn; `Missing` -> Neutral; `Unreadable` and others -> Bad.
pub fn path_status_tone(status: RecoveryPathStatus) -> RecoveryTone {
    match status {
        RecoveryPathStatus::Exists => RecoveryTone::Good,
        RecoveryPathStatus::Changed => RecoveryTone::Warn,
        RecoveryPathStatus::Missing => RecoveryTone::Neutral,
        RecoveryPathStatus::Unreadable => RecoveryTone::Bad,
    }
}

/// C# `AllowsRetry`: `result` is `CanRetry` AND (`entry.op_type` is `Move` or `Copy`, OR (`result.is_group()` AND
/// `entry.op_type` is `Recycle`)). `None` (still checking) -> false.
pub fn allows_retry(result: Option<&RecoveryCheckResult>) -> bool {
    result.is_some_and(|r| {
        r.verdict == RecoveryVerdict::CanRetry
            && (matches!(r.entry.op_type, FileOperationType::Move | FileOperationType::Copy)
                || (r.is_group() && r.entry.op_type == FileOperationType::Recycle))
    })
}

/// C# `Matches`. `All` -> true; `CanRetry` -> `verdict == Some(CanRetry)`; `AlreadyDone` -> `verdict == Some(AlreadyDone)`;
/// `Problems` -> `verdict` is `Some(v)` with `v` other than `CanRetry` and `AlreadyDone`.
pub fn matches(filter: RecoveryFilter, verdict: Option<RecoveryVerdict>) -> bool {
    match filter {
        RecoveryFilter::All => true,
        RecoveryFilter::CanRetry => verdict == Some(RecoveryVerdict::CanRetry),
        RecoveryFilter::AlreadyDone => verdict == Some(RecoveryVerdict::AlreadyDone),
        RecoveryFilter::Problems => {
            verdict.is_some_and(|v| !matches!(v, RecoveryVerdict::CanRetry | RecoveryVerdict::AlreadyDone))
        }
    }
}

/// C# `NearestExistingFolder`: walk up from `path` with `get_directory_name` and return the first ancestor for which
/// `directory_exists` is true; `None` when the path cannot be parsed (`get_directory_name` gives `None`) or no
/// ancestor exists. The walk does not test `path` itself.
pub fn nearest_existing_folder(path: &str, directory_exists: &dyn Fn(&str) -> bool) -> Option<String> {
    let mut current = get_directory_name(path);
    while let Some(folder) = current {
        if folder.is_empty() {
            return None;
        }
        if directory_exists(&folder) {
            return Some(folder);
        }
        current = get_directory_name(&folder);
    }
    None
}

impl std::fmt::Debug for RecoveryTexts<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryTexts")
            .field("group_separator", &self.group_separator)
            .finish_non_exhaustive()
    }
}
