//! Production localized text adapters (NOT in the plan row of W8-09b; found while writing `contracts/W8-09b.md`, decision D-W8B6-150).
//!
//! C# reads every UI sentence of the core and imaging libraries from the ambient `Tr.*` (`Localizer.Current`). In Rust each of those libraries takes an injected
//! `*Texts` trait (D13/D41: `pr-core`, `pr-imaging`, `pr-imaging-raw`, `pr-turbojpeg` do not see `pr-l10n`) and ships only an English implementation. The composition root
//! owes the localized one. [`LocalizedTexts`] implements ALL of them over `Localizer::current()` / `number_culture()`, evaluated on EVERY call (a language switch applies at once,
//! D-W8-4): each method is `Localizer::current().format_with(KEY, &[LocArg::new(NAME, value)], &number_culture())` with the key named in the doc of the trait method
//! (`English*Texts` of the same trait lists the same keys and is the oracle: a test compares every method of `LocalizedTexts` under the English catalog with `English*Texts`).
//! Contract: `contracts/W8-09b.md`, slice S1. C# source: the generated `Tr.*` members of `PhotoReview.Core.Localization` (baseline `d92d6aae`).

use std::sync::Arc;

use pr_app::coordinators::file_actions::recycle::TextSource;
use pr_app::misc::file_hash::FileHashTexts;
use pr_core::file_actions::journal::JournalTexts;
use pr_core::file_actions::recovery_retry::RecoveryRetryTexts;
use pr_core::file_actions::service::FileActionTexts;
use pr_core::file_actions::undo::UndoTexts;
use pr_core::model::FileOperationType;
use pr_core::settings::app_settings::SettingsTexts;
use pr_imaging::cache::source_bytes::SourceBytesTexts;
use pr_imaging::webp_heic::WebpHeicTexts;
use pr_imaging_raw::decoder::RawDecoderTexts;
use pr_imaging_raw::routing::RawRoutingTexts;
use pr_l10n::keys;
use pr_l10n::localizer::{Localizer, built_in_english_localizer, number_culture};
use pr_l10n::template::{LocArg, NumberCulture};
use pr_turbojpeg::decoder::TurboJpegTexts;

/// `Localizer::current().get(key)` (C# `Tr.X` without placeholders).
fn ambient_get(key: &str) -> String {
    Localizer::current().get(key)
}

/// `Localizer::current().format_with(key, args, &number_culture())` (C# `Tr.X(args)`).
fn ambient_format(key: &str, args: &[LocArg]) -> String {
    Localizer::current().format_with(key, args, &number_culture())
}

/// A count as the catalog argument (C# `int`): saturates instead of wrapping for a count that does not fit.
fn count_arg(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

/// The `{operation}` placeholder of `core.fileAction.unsupportedOperation` is the C# enum member name.
fn operation_name(operation: FileOperationType) -> &'static str {
    match operation {
        FileOperationType::Move => "Move",
        FileOperationType::Copy => "Copy",
        FileOperationType::Recycle => "Recycle",
    }
}

/// Zero-sized; `Clone + Copy`. One value serves every `*Texts` trait below and [`TextSource`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalizedTexts;

impl TextSource for LocalizedTexts {
    /// `Localizer::current()`.
    fn localizer(&self) -> Arc<Localizer> {
        Localizer::current()
    }

    /// `pr_l10n::localizer::number_culture()`.
    fn numbers(&self) -> Arc<NumberCulture> {
        number_culture()
    }
}

impl JournalTexts for LocalizedTexts {
    /// C# `BuiltInCatalog.EnglishLocalizer.Get(key)`: the invariant text stored in the journal, whatever the UI language is.
    fn english(&self, key: &str) -> String {
        built_in_english_localizer().get(key)
    }

    /// C# `Localizer.Current.Get(key)`.
    fn localized(&self, key: &str) -> String {
        ambient_get(key)
    }
}

impl FileActionTexts for LocalizedTexts {
    fn core_file_action_busy(&self) -> String {
        ambient_get(keys::CORE_FILE_ACTION_BUSY)
    }

    fn core_file_action_group_needs_two_paths(&self) -> String {
        ambient_get(keys::CORE_FILE_ACTION_GROUP_NEEDS_TWO_PATHS)
    }

    fn core_file_action_unsupported_operation(&self, operation: FileOperationType) -> String {
        ambient_format(
            keys::CORE_FILE_ACTION_UNSUPPORTED_OPERATION,
            &[LocArg::new("operation", operation_name(operation))],
        )
    }

    fn core_file_action_no_destination(&self) -> String {
        ambient_get(keys::CORE_FILE_ACTION_NO_DESTINATION)
    }

    fn core_file_action_same_folder(&self) -> String {
        ambient_get(keys::CORE_FILE_ACTION_SAME_FOLDER)
    }

    fn core_file_action_destination_exists(&self, path: &str) -> String {
        ambient_format(keys::CORE_FILE_ACTION_DESTINATION_EXISTS, &[LocArg::new("path", path)])
    }

    fn core_file_action_source_missing(&self, path: &str) -> String {
        ambient_format(keys::CORE_FILE_ACTION_SOURCE_MISSING, &[LocArg::new("path", path)])
    }

    fn core_recycle_unsupported_drive(&self, name: &str) -> String {
        ambient_format(keys::CORE_RECYCLE_UNSUPPORTED_DRIVE, &[LocArg::new("name", name)])
    }

    fn core_recycle_bin_cannot_hold(&self, name: &str) -> String {
        ambient_format(keys::CORE_RECYCLE_BIN_CANNOT_HOLD, &[LocArg::new("name", name)])
    }

    fn core_group_action_rollback_failed(&self, message: &str, count: usize) -> String {
        ambient_format(
            keys::CORE_GROUP_ACTION_ROLLBACK_FAILED,
            &[LocArg::new("message", message), LocArg::new("count", count_arg(count))],
        )
    }
}

impl UndoTexts for LocalizedTexts {
    fn core_undo_busy(&self) -> String {
        ambient_get(keys::CORE_UNDO_BUSY)
    }

    fn core_undo_no_move_to_undo(&self) -> String {
        ambient_get(keys::CORE_UNDO_NO_MOVE_TO_UNDO)
    }

    fn core_undo_nothing_to_undo(&self) -> String {
        ambient_get(keys::CORE_UNDO_NOTHING_TO_UNDO)
    }

    fn core_undo_source_or_destination_changed(&self) -> String {
        ambient_get(keys::CORE_UNDO_SOURCE_OR_DESTINATION_CHANGED)
    }

    fn core_undo_fingerprint_missing(&self) -> String {
        ambient_get(keys::CORE_UNDO_FINGERPRINT_MISSING)
    }

    fn core_undo_destination_changed_after_move(&self) -> String {
        ambient_get(keys::CORE_UNDO_DESTINATION_CHANGED_AFTER_MOVE)
    }

    fn core_undo_failed(&self, message: &str) -> String {
        ambient_format(keys::CORE_UNDO_FAILED, &[LocArg::new("message", message)])
    }

    fn core_undo_permanently_deleted(&self, file_name: &str) -> String {
        ambient_format(
            keys::CORE_UNDO_PERMANENTLY_DELETED,
            &[LocArg::new("fileName", file_name)],
        )
    }

    fn core_undo_recycle_target_exists(&self, file_name: &str) -> String {
        ambient_format(
            keys::CORE_UNDO_RECYCLE_TARGET_EXISTS,
            &[LocArg::new("fileName", file_name)],
        )
    }

    fn core_undo_recycle_restore_failed(&self, file_name: &str) -> String {
        ambient_format(
            keys::CORE_UNDO_RECYCLE_RESTORE_FAILED,
            &[LocArg::new("fileName", file_name)],
        )
    }

    fn core_undo_unsupported(&self) -> String {
        ambient_get(keys::CORE_UNDO_UNSUPPORTED)
    }

    fn core_undo_group_partially_restored(&self, restored: usize, total: usize, names: &str) -> String {
        ambient_format(
            keys::CORE_UNDO_GROUP_PARTIALLY_RESTORED,
            &[
                LocArg::new("restored", count_arg(restored)),
                LocArg::new("total", count_arg(total)),
                LocArg::new("names", names),
            ],
        )
    }

    fn core_recovery_already_handled(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_ALREADY_HANDLED)
    }
}

impl RecoveryRetryTexts for LocalizedTexts {
    fn core_recovery_busy(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_BUSY)
    }

    fn core_recovery_cancelled_needs_confirm(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_CANCELLED_NEEDS_CONFIRM)
    }

    fn core_recovery_only_move_copy(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_ONLY_MOVE_COPY)
    }

    fn core_recovery_no_destination(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_NO_DESTINATION)
    }

    fn core_recovery_source_missing(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_SOURCE_MISSING)
    }

    fn core_recovery_source_changed(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_SOURCE_CHANGED)
    }

    fn core_recovery_destination_exists(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_DESTINATION_EXISTS)
    }

    fn core_recovery_conflict(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_CONFLICT)
    }

    fn core_recovery_destination_changed(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_DESTINATION_CHANGED)
    }

    fn core_recovery_already_handled(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_ALREADY_HANDLED)
    }

    fn core_recovery_succeeded(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_SUCCEEDED)
    }

    fn core_recovery_succeeded_journal_failed(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_SUCCEEDED_JOURNAL_FAILED)
    }

    fn core_recovery_completed_failure_not_journaled(&self) -> String {
        ambient_get(keys::CORE_RECOVERY_COMPLETED_FAILURE_NOT_JOURNALED)
    }

    fn core_recycle_unsupported_drive(&self, name: &str) -> String {
        ambient_format(keys::CORE_RECYCLE_UNSUPPORTED_DRIVE, &[LocArg::new("name", name)])
    }

    fn core_recycle_bin_cannot_hold(&self, name: &str) -> String {
        ambient_format(keys::CORE_RECYCLE_BIN_CANNOT_HOLD, &[LocArg::new("name", name)])
    }

    fn core_undo_recycle_target_exists(&self, file_name: &str) -> String {
        ambient_format(
            keys::CORE_UNDO_RECYCLE_TARGET_EXISTS,
            &[LocArg::new("fileName", file_name)],
        )
    }

    fn core_undo_recycle_restore_failed(&self, file_name: &str) -> String {
        ambient_format(
            keys::CORE_UNDO_RECYCLE_RESTORE_FAILED,
            &[LocArg::new("fileName", file_name)],
        )
    }
}

impl FileHashTexts for LocalizedTexts {
    fn file_changed_while_hashing(&self, path: &str) -> String {
        ambient_format(keys::ERR_IO_FILE_CHANGED_WHILE_HASHING, &[LocArg::new("path", path)])
    }
}

impl SourceBytesTexts for LocalizedTexts {
    fn file_changed_while_reading(&self, path: &str) -> String {
        ambient_format(keys::ERR_IO_FILE_CHANGED_WHILE_READING, &[LocArg::new("path", path)])
    }

    fn unexpected_eof(&self, path: &str) -> String {
        ambient_format(keys::ERR_IO_UNEXPECTED_EOF, &[LocArg::new("path", path)])
    }
}

impl WebpHeicTexts for LocalizedTexts {
    fn webp_heic_disabled(&self, file_name: &str) -> String {
        ambient_format(
            keys::IMAGE_ERROR_WEBP_HEIC_DISABLED,
            &[LocArg::new("fileName", file_name)],
        )
    }

    fn webp_codec_missing(&self, file_name: &str) -> String {
        ambient_format(
            keys::IMAGE_ERROR_WEBP_CODEC_MISSING,
            &[LocArg::new("fileName", file_name)],
        )
    }

    fn heif_codec_missing(&self, file_name: &str) -> String {
        ambient_format(
            keys::IMAGE_ERROR_HEIF_CODEC_MISSING,
            &[LocArg::new("fileName", file_name)],
        )
    }
}

impl RawDecoderTexts for LocalizedTexts {
    fn raw_corrupt(&self) -> String {
        ambient_get(keys::IMAGE_ERROR_RAW_CORRUPT)
    }

    fn raw_unsupported(&self) -> String {
        ambient_get(keys::IMAGE_ERROR_RAW_UNSUPPORTED)
    }

    fn raw_no_preview(&self) -> String {
        ambient_get(keys::IMAGE_ERROR_RAW_NO_PREVIEW)
    }

    fn file_changed_while_reading(&self, path: &str) -> String {
        ambient_format(keys::ERR_IO_FILE_CHANGED_WHILE_READING, &[LocArg::new("path", path)])
    }
}

impl RawRoutingTexts for LocalizedTexts {
    fn raw_disabled(&self, file_name: &str) -> String {
        ambient_format(keys::IMAGE_ERROR_RAW_DISABLED, &[LocArg::new("fileName", file_name)])
    }
}

impl TurboJpegTexts for LocalizedTexts {
    fn not_jpeg(&self, source: &str) -> String {
        ambient_format(keys::ERR_DECODER_NOT_JPEG, &[LocArg::new("source", source)])
    }

    fn memory_buffer(&self) -> String {
        ambient_get(keys::ERR_DECODER_MEMORY_BUFFER)
    }

    fn icc_fallback(&self) -> String {
        ambient_get(keys::ERR_DECODER_ICC_FALLBACK)
    }

    fn no_detail(&self) -> String {
        ambient_get(keys::ERR_DECODER_NO_DETAIL)
    }

    fn header_failed(&self, detail: &str) -> String {
        ambient_format(keys::ERR_DECODER_HEADER_FAILED, &[LocArg::new("detail", detail)])
    }

    fn invalid_dimensions(&self, width: i64, height: i64) -> String {
        ambient_format(
            keys::ERR_DECODER_INVALID_DIMENSIONS,
            &[LocArg::new("width", width), LocArg::new("height", height)],
        )
    }

    fn decompress_failed(&self, detail: &str) -> String {
        ambient_format(keys::ERR_DECODER_DECOMPRESS_FAILED, &[LocArg::new("detail", detail)])
    }

    fn info_failed(&self, detail: &str) -> String {
        ambient_format(keys::ERR_DECODER_INFO_FAILED, &[LocArg::new("detail", detail)])
    }

    fn no_source(&self) -> String {
        ambient_get(keys::ERR_DECODER_NO_SOURCE)
    }

    fn output_too_large(&self, width: i64, height: i64, bytes: i64) -> String {
        ambient_format(
            keys::ERR_DECODER_OUTPUT_TOO_LARGE,
            &[
                LocArg::new("width", width),
                LocArg::new("height", height),
                LocArg::new("bytes", bytes),
            ],
        )
    }

    fn strict_mode_failed(&self, detail: &str) -> String {
        ambient_format(keys::ERR_DECODER_STRICT_MODE_FAILED, &[LocArg::new("detail", detail)])
    }

    fn image_not_found(&self, path: &str) -> String {
        ambient_format(keys::ERR_IO_IMAGE_NOT_FOUND, &[LocArg::new("path", path)])
    }
}

impl SettingsTexts for LocalizedTexts {
    fn action_default_name(&self, n: u32) -> String {
        ambient_format(keys::ACTION_DEFAULT_NAME, &[LocArg::new("n", n)])
    }

    fn action_default_backup(&self) -> String {
        ambient_get(keys::ACTION_DEFAULT_BACKUP)
    }

    fn core_settings_shortcut_invalid(&self, name: &str) -> String {
        ambient_format(keys::CORE_SETTINGS_SHORTCUT_INVALID, &[LocArg::new("name", name)])
    }

    fn core_settings_action_invalid(&self) -> String {
        ambient_get(keys::CORE_SETTINGS_ACTION_INVALID)
    }

    fn core_settings_action_binding_name(&self, name: &str) -> String {
        ambient_format(keys::CORE_SETTINGS_ACTION_BINDING_NAME, &[LocArg::new("name", name)])
    }

    fn core_settings_shortcut_duplicate(&self, key: &str, names: &str) -> String {
        ambient_format(
            keys::CORE_SETTINGS_SHORTCUT_DUPLICATE,
            &[LocArg::new("key", key), LocArg::new("names", names)],
        )
    }

    fn settings_load_repaired(&self, items: &str) -> String {
        ambient_format(keys::SETTINGS_LOAD_REPAIRED, &[LocArg::new("items", items)])
    }

    fn dialog_import_settings_repaired(&self, items: &str) -> String {
        ambient_format(keys::DIALOG_IMPORT_SETTINGS_REPAIRED, &[LocArg::new("items", items)])
    }

    fn settings_load_shortcut_disabled(&self, name: &str) -> String {
        ambient_format(keys::SETTINGS_LOAD_SHORTCUT_DISABLED, &[LocArg::new("name", name)])
    }
}
