//! Port of `PhotoReview.App.Shared/Presenters/RecoveryPresentation.cs` (237 lines): the pure mapping from
//! `RecoveryCheckResult` / `JournalEntry` to localized text and colour tones of the Recovery window. No UI type.
//!
//! Texts: `pr_l10n::keys::*` constants through `Localizer::format` (D-W8-4; never `tr::` accessors, which read a
//! global localizer). Time: `RecoveryTimeFormatter`, implemented by `pr_platform_win` (contract D-10f-3).
//! Digit grouping of `N0`: `RecoveryTexts::group_separator` (contract D-10f-4).
//! Contract: `contracts/W8-10f.md`. Lead-owned `presenters/mod.rs` declares this module.
#![allow(unused_variables, dead_code)] // D11: todo!() bodies; the porter removes this line with the last todo!()

use pr_core::file_actions::journal_line::{JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_check::{
    RecoveryCheckResult, RecoveryPathCheck, RecoveryPathStatus, RecoveryVerdict,
};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_l10n::localizer::Localizer;

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
        todo!()
    }

    /// C# `ExplainText`: `RECOVERY_EXPLAIN_*`; unknown -> `RECOVERY_EXPLAIN_UNKNOWN`.
    pub fn explain_text(&self, verdict: RecoveryVerdict) -> String {
        todo!()
    }

    /// C# `ActionText`: `RECOVERY_ACTION_*`; unknown -> `RECOVERY_ACTION_UNKNOWN`.
    pub fn action_text(&self, verdict: RecoveryVerdict) -> String {
        todo!()
    }

    /// C# `PathStatusText`: `RECOVERY_PATH_STATUS_*`; `Unreadable` and any other value -> `RECOVERY_PATH_STATUS_UNREADABLE`.
    pub fn path_status_text(&self, status: RecoveryPathStatus) -> String {
        todo!()
    }

    /// C# `FormatSize`: `RECOVERY_DETAIL_BYTES` with `{size}` = the decimal digits of `bytes` grouped by
    /// `group_separator` every three digits (`N0`), the minus sign of the culture (`-`) in front when negative.
    pub fn format_size(&self, bytes: i64) -> String {
        todo!()
    }

    /// C# `FormatTime`: `self.time.format_local_general(utc)`.
    pub fn format_time(&self, utc: DateTimeUtc) -> String {
        todo!()
    }

    /// C# `NowText`: missing -> `RECOVERY_DETAIL_NOW_MISSING`; unreadable -> `RECOVERY_DETAIL_NOW_UNREADABLE`;
    /// otherwise `RECOVERY_DETAIL_NOW_PRESENT` with the current size (0 when absent) and the current write time
    /// (`DateTimeUtc::default()` when absent, C# `?? default`).
    pub fn now_text(&self, check: &RecoveryPathCheck) -> String {
        todo!()
    }

    /// C# `JournalText`: `RECOVERY_DETAIL_JOURNAL` with the journaled size and `last_write_utc.value`.
    pub fn journal_text(&self, entry: &JournalEntry) -> String {
        todo!()
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
        todo!()
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
        todo!()
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
        todo!()
    }

    /// C# `OperationText`: `ENUM_FILE_OPERATION_MOVE` / `_COPY` / `_RECYCLE`.
    pub fn operation_text(&self, op: FileOperationType) -> String {
        todo!()
    }

    /// C# `StateText`: `ENUM_JOURNAL_STATE_PREPARED` / `_COMMITTED` / `_FAILED`; `Dismissed` (no case in C#, the
    /// default `ToString()`) -> the literal `"Dismissed"`.
    pub fn state_text(&self, state: JournalState) -> String {
        todo!()
    }
}

/// C# `Rgb`: the documented hint colour (0xRRGGBB) of a tone. `Neutral` and any other value -> (0xB0, 0xB0, 0xB0).
pub fn rgb(tone: RecoveryTone) -> (u8, u8, u8) {
    todo!()
}

/// C# `VerdictTone`. `CanRetry` -> Good; `AlreadyDone` -> Info; `Conflict`, `SourceChanged`, `DestinationChanged`,
/// `NotRecycled`, `PartiallyPermanentlyDeleted` -> Warn; `PermanentlyDeleted`, `Lost` -> Bad; else Neutral.
pub fn verdict_tone(verdict: RecoveryVerdict) -> RecoveryTone {
    todo!()
}

/// C# `PathStatusTone`. `Exists` -> Good; `Changed` -> Warn; `Missing` -> Neutral; `Unreadable` and others -> Bad.
pub fn path_status_tone(status: RecoveryPathStatus) -> RecoveryTone {
    todo!()
}

/// C# `AllowsRetry`: `result` is `CanRetry` AND (`entry.op_type` is `Move` or `Copy`, OR (`result.is_group()` AND
/// `entry.op_type` is `Recycle`)). `None` (still checking) -> false.
pub fn allows_retry(result: Option<&RecoveryCheckResult>) -> bool {
    todo!()
}

/// C# `Matches`. `All` -> true; `CanRetry` -> `verdict == Some(CanRetry)`; `AlreadyDone` -> `verdict == Some(AlreadyDone)`;
/// `Problems` -> `verdict` is `Some(v)` with `v` other than `CanRetry` and `AlreadyDone`.
pub fn matches(filter: RecoveryFilter, verdict: Option<RecoveryVerdict>) -> bool {
    todo!()
}

/// C# `NearestExistingFolder`: walk up from `path` with `get_directory_name` and return the first ancestor for which
/// `directory_exists` is true; `None` when the path cannot be parsed (`get_directory_name` gives `None`) or no
/// ancestor exists. The walk does not test `path` itself.
pub fn nearest_existing_folder(path: &str, directory_exists: &dyn Fn(&str) -> bool) -> Option<String> {
    todo!()
}

impl std::fmt::Debug for RecoveryTexts<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryTexts")
            .field("group_separator", &self.group_separator)
            .finish_non_exhaustive()
    }
}
