//! Production localized text adapters (NOT in the plan row of W8-09b; found while writing `contracts/W8-09b.md`, decision D-W8B6-150).
//!
//! C# reads every UI sentence of the core and imaging libraries from the ambient `Tr.*` (`Localizer.Current`). In Rust each of those libraries takes an injected
//! `*Texts` trait (D13/D41: `pr-core`, `pr-imaging`, `pr-imaging-raw`, `pr-turbojpeg` do not see `pr-l10n`) and ships only an English implementation. The composition root
//! owes the localized one. [`LocalizedTexts`] implements ALL of them over `Localizer::current()` / `number_culture()`, evaluated on EVERY call (a language switch applies at once,
//! D-W8-4): each method is `Localizer::current().format_with(KEY, &[LocArg::new(NAME, value)], &number_culture())` with the key named in the doc of the trait method
//! (`English*Texts` of the same trait lists the same keys and is the oracle: a test compares every method of `LocalizedTexts` under the English catalog with `English*Texts`).
//! Contract: `contracts/W8-09b.md`, slice S1.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

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
use pr_l10n::localizer::Localizer;
use pr_l10n::template::NumberCulture;
use pr_turbojpeg::decoder::TurboJpegTexts;

/// Zero-sized; `Clone + Copy`. One value serves every `*Texts` trait below and [`TextSource`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalizedTexts;

impl TextSource for LocalizedTexts {
    /// `Localizer::current()`.
    fn localizer(&self) -> Arc<Localizer> {
        todo!()
    }

    /// `pr_l10n::localizer::number_culture()`.
    fn numbers(&self) -> Arc<NumberCulture> {
        todo!()
    }
}

impl JournalTexts for LocalizedTexts {
    fn english(&self, key: &str) -> String {
        todo!()
    }

    fn localized(&self, key: &str) -> String {
        todo!()
    }
}

impl FileActionTexts for LocalizedTexts {
    fn core_file_action_busy(&self) -> String {
        todo!()
    }

    fn core_file_action_group_needs_two_paths(&self) -> String {
        todo!()
    }

    fn core_file_action_unsupported_operation(&self, operation: FileOperationType) -> String {
        todo!()
    }

    fn core_file_action_no_destination(&self) -> String {
        todo!()
    }

    fn core_file_action_same_folder(&self) -> String {
        todo!()
    }

    fn core_file_action_destination_exists(&self, path: &str) -> String {
        todo!()
    }

    fn core_file_action_source_missing(&self, path: &str) -> String {
        todo!()
    }

    fn core_recycle_unsupported_drive(&self, name: &str) -> String {
        todo!()
    }

    fn core_recycle_bin_cannot_hold(&self, name: &str) -> String {
        todo!()
    }

    fn core_group_action_rollback_failed(&self, message: &str, count: usize) -> String {
        todo!()
    }
}

impl UndoTexts for LocalizedTexts {
    fn core_undo_busy(&self) -> String {
        todo!()
    }

    fn core_undo_no_move_to_undo(&self) -> String {
        todo!()
    }

    fn core_undo_nothing_to_undo(&self) -> String {
        todo!()
    }

    fn core_undo_source_or_destination_changed(&self) -> String {
        todo!()
    }

    fn core_undo_fingerprint_missing(&self) -> String {
        todo!()
    }

    fn core_undo_destination_changed_after_move(&self) -> String {
        todo!()
    }

    fn core_undo_failed(&self, message: &str) -> String {
        todo!()
    }

    fn core_undo_permanently_deleted(&self, file_name: &str) -> String {
        todo!()
    }

    fn core_undo_recycle_target_exists(&self, file_name: &str) -> String {
        todo!()
    }

    fn core_undo_recycle_restore_failed(&self, file_name: &str) -> String {
        todo!()
    }

    fn core_undo_unsupported(&self) -> String {
        todo!()
    }

    fn core_undo_group_partially_restored(&self, restored: usize, total: usize, names: &str) -> String {
        todo!()
    }

    fn core_recovery_already_handled(&self) -> String {
        todo!()
    }
}

impl RecoveryRetryTexts for LocalizedTexts {
    fn core_recovery_busy(&self) -> String {
        todo!()
    }

    fn core_recovery_cancelled_needs_confirm(&self) -> String {
        todo!()
    }

    fn core_recovery_only_move_copy(&self) -> String {
        todo!()
    }

    fn core_recovery_no_destination(&self) -> String {
        todo!()
    }

    fn core_recovery_source_missing(&self) -> String {
        todo!()
    }

    fn core_recovery_source_changed(&self) -> String {
        todo!()
    }

    fn core_recovery_destination_exists(&self) -> String {
        todo!()
    }

    fn core_recovery_conflict(&self) -> String {
        todo!()
    }

    fn core_recovery_destination_changed(&self) -> String {
        todo!()
    }

    fn core_recovery_already_handled(&self) -> String {
        todo!()
    }

    fn core_recovery_succeeded(&self) -> String {
        todo!()
    }

    fn core_recovery_succeeded_journal_failed(&self) -> String {
        todo!()
    }

    fn core_recovery_completed_failure_not_journaled(&self) -> String {
        todo!()
    }

    fn core_recycle_unsupported_drive(&self, name: &str) -> String {
        todo!()
    }

    fn core_recycle_bin_cannot_hold(&self, name: &str) -> String {
        todo!()
    }

    fn core_undo_recycle_target_exists(&self, file_name: &str) -> String {
        todo!()
    }

    fn core_undo_recycle_restore_failed(&self, file_name: &str) -> String {
        todo!()
    }
}

impl FileHashTexts for LocalizedTexts {
    fn file_changed_while_hashing(&self, path: &str) -> String {
        todo!()
    }
}

impl SourceBytesTexts for LocalizedTexts {
    fn file_changed_while_reading(&self, path: &str) -> String {
        todo!()
    }

    fn unexpected_eof(&self, path: &str) -> String {
        todo!()
    }
}

impl WebpHeicTexts for LocalizedTexts {
    fn webp_heic_disabled(&self, file_name: &str) -> String {
        todo!()
    }

    fn webp_codec_missing(&self, file_name: &str) -> String {
        todo!()
    }

    fn heif_codec_missing(&self, file_name: &str) -> String {
        todo!()
    }
}

impl RawDecoderTexts for LocalizedTexts {
    fn raw_corrupt(&self) -> String {
        todo!()
    }

    fn raw_unsupported(&self) -> String {
        todo!()
    }

    fn raw_no_preview(&self) -> String {
        todo!()
    }

    fn file_changed_while_reading(&self, path: &str) -> String {
        todo!()
    }
}

impl RawRoutingTexts for LocalizedTexts {
    fn raw_disabled(&self, file_name: &str) -> String {
        todo!()
    }
}

impl TurboJpegTexts for LocalizedTexts {
    fn not_jpeg(&self, source: &str) -> String {
        todo!()
    }

    fn memory_buffer(&self) -> String {
        todo!()
    }

    fn icc_fallback(&self) -> String {
        todo!()
    }

    fn no_detail(&self) -> String {
        todo!()
    }

    fn header_failed(&self, detail: &str) -> String {
        todo!()
    }

    fn invalid_dimensions(&self, width: i64, height: i64) -> String {
        todo!()
    }

    fn decompress_failed(&self, detail: &str) -> String {
        todo!()
    }

    fn info_failed(&self, detail: &str) -> String {
        todo!()
    }

    fn no_source(&self) -> String {
        todo!()
    }

    fn output_too_large(&self, width: i64, height: i64, bytes: i64) -> String {
        todo!()
    }

    fn strict_mode_failed(&self, detail: &str) -> String {
        todo!()
    }

    fn image_not_found(&self, path: &str) -> String {
        todo!()
    }
}

impl SettingsTexts for LocalizedTexts {
    fn action_default_name(&self, n: u32) -> String {
        todo!()
    }

    fn action_default_backup(&self) -> String {
        todo!()
    }

    fn core_settings_shortcut_invalid(&self, name: &str) -> String {
        todo!()
    }

    fn core_settings_action_invalid(&self) -> String {
        todo!()
    }

    fn core_settings_action_binding_name(&self, name: &str) -> String {
        todo!()
    }

    fn core_settings_shortcut_duplicate(&self, key: &str, names: &str) -> String {
        todo!()
    }

    fn settings_load_repaired(&self, items: &str) -> String {
        todo!()
    }

    fn dialog_import_settings_repaired(&self, items: &str) -> String {
        todo!()
    }

    fn settings_load_shortcut_disabled(&self, name: &str) -> String {
        todo!()
    }
}
