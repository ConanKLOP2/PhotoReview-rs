//! Tests of `pr_shell::composition::texts::LocalizedTexts` (W8-09b slice S1, `contracts/W8-09b.md`, decision D-W8B6-150).
//!
//! C# has no twin: the C# libraries read the ambient `Tr.*` directly, so the Rust injected `*Texts` traits need a production implementation and these tests are all `new:`.
//! The oracle is the `English*Texts` of each trait (they list the same keys and the texts of `assets/languages/en.json`); a second test replaces the ambient localizer by the
//! "show keys" one (every text becomes `[key]`) so that each method is pinned to its catalog key even where two keys have the same English text.
//!
//! `Localizer::current()` and `number_culture()` are process-wide: every test holds [`ambient`], which also restores English and the invariant culture afterwards.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_app::coordinators::file_actions::recycle::TextSource;
use pr_app::misc::file_hash::{EnglishFileHashTexts, FileHashTexts};
use pr_core::file_actions::journal::{ENGLISH_JOURNAL_TEXTS, EnglishJournalTexts, JournalTexts};
use pr_core::file_actions::recovery_retry::{EnglishRecoveryRetryTexts, RecoveryRetryTexts};
use pr_core::file_actions::service::{EnglishFileActionTexts, FileActionTexts};
use pr_core::file_actions::undo::{EnglishUndoTexts, UndoTexts};
use pr_core::model::FileOperationType;
use pr_core::settings::app_settings::{EnglishSettingsTexts, SettingsTexts};
use pr_imaging::cache::source_bytes::{EnglishSourceBytesTexts, SourceBytesTexts};
use pr_imaging::webp_heic::WebpHeicTexts;
use pr_imaging_raw::decoder::{EnglishRawDecoderTexts, RawDecoderTexts};
use pr_imaging_raw::routing::RawRoutingTexts;
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::{
    Localizer, built_in_english, built_in_english_localizer, create_show_keys, number_culture, set_number_culture,
};
use pr_l10n::template::NumberCulture;
use pr_shell::composition::texts::LocalizedTexts;
use pr_turbojpeg::decoder::{EnglishTurboJpegTexts, TurboJpegTexts};

static GLOBAL: Mutex<()> = Mutex::new(());

/// Holds the process-wide lock and puts the ambient state back to English / invariant on drop (also when a test panics).
struct Ambient {
    _lock: MutexGuard<'static, ()>,
}

impl Drop for Ambient {
    fn drop(&mut self) {
        Localizer::set_current(built_in_english_localizer());
        set_number_culture(NumberCulture::invariant());
    }
}

/// Takes the lock and starts from the English catalog and the invariant culture.
fn ambient() -> Ambient {
    let lock = GLOBAL.lock().unwrap_or_else(PoisonError::into_inner);
    Localizer::set_current(built_in_english_localizer());
    set_number_culture(NumberCulture::invariant());
    Ambient { _lock: lock }
}

/// What every text becomes under the "show keys" localizer.
fn key(name: &str) -> String {
    format!("[{name}]")
}

// A path-like value with braces: it must be copied into the text, never be read as a template.
const PATH: &str = "C:\\photos\\{raw}\\a b.jpg";

#[test]
fn localized_texts_equal_the_english_journal_texts_under_the_english_catalog() {
    // new: LocalizedTexts / JournalTexts against EnglishJournalTexts
    let _ambient = ambient();
    let localized = LocalizedTexts;
    let english = EnglishJournalTexts;
    for (code_key, text) in ENGLISH_JOURNAL_TEXTS {
        assert_eq!(JournalTexts::english(&localized, code_key), text, "english({code_key})");
        assert_eq!(
            JournalTexts::localized(&localized, code_key),
            text,
            "localized({code_key})"
        );
        assert_eq!(
            JournalTexts::localized(&localized, code_key),
            JournalTexts::localized(&english, code_key)
        );
    }
    // An unknown key answers itself (C# `Localizer.Get`), like `EnglishJournalTexts`.
    assert_eq!(JournalTexts::english(&localized, "no.such.key"), "no.such.key");
    assert_eq!(JournalTexts::localized(&localized, "no.such.key"), "no.such.key");
}

#[test]
fn localized_texts_equal_the_english_file_action_texts_under_the_english_catalog() {
    // new: LocalizedTexts / FileActionTexts against EnglishFileActionTexts
    let _ambient = ambient();
    let l = LocalizedTexts;
    let e = EnglishFileActionTexts;
    assert_eq!(l.core_file_action_busy(), e.core_file_action_busy());
    assert_eq!(
        l.core_file_action_group_needs_two_paths(),
        e.core_file_action_group_needs_two_paths()
    );
    for operation in [
        FileOperationType::Move,
        FileOperationType::Copy,
        FileOperationType::Recycle,
    ] {
        assert_eq!(
            l.core_file_action_unsupported_operation(operation),
            e.core_file_action_unsupported_operation(operation),
            "{operation:?}"
        );
    }
    assert_eq!(l.core_file_action_no_destination(), e.core_file_action_no_destination());
    assert_eq!(l.core_file_action_same_folder(), e.core_file_action_same_folder());
    assert_eq!(
        l.core_file_action_destination_exists(PATH),
        e.core_file_action_destination_exists(PATH)
    );
    assert_eq!(
        l.core_file_action_source_missing(PATH),
        e.core_file_action_source_missing(PATH)
    );
    assert_eq!(
        FileActionTexts::core_recycle_unsupported_drive(&l, "E:\\x{y}.jpg"),
        FileActionTexts::core_recycle_unsupported_drive(&e, "E:\\x{y}.jpg")
    );
    assert_eq!(
        FileActionTexts::core_recycle_bin_cannot_hold(&l, "E:\\x{y}.jpg"),
        FileActionTexts::core_recycle_bin_cannot_hold(&e, "E:\\x{y}.jpg")
    );
    assert_eq!(
        l.core_group_action_rollback_failed("It failed.", 7),
        e.core_group_action_rollback_failed("It failed.", 7)
    );
}

#[test]
fn localized_texts_equal_the_english_undo_texts_under_the_english_catalog() {
    // new: LocalizedTexts / UndoTexts against EnglishUndoTexts
    let _ambient = ambient();
    let l = LocalizedTexts;
    let e = EnglishUndoTexts;
    assert_eq!(l.core_undo_busy(), e.core_undo_busy());
    assert_eq!(l.core_undo_no_move_to_undo(), e.core_undo_no_move_to_undo());
    assert_eq!(l.core_undo_nothing_to_undo(), e.core_undo_nothing_to_undo());
    assert_eq!(
        l.core_undo_source_or_destination_changed(),
        e.core_undo_source_or_destination_changed()
    );
    assert_eq!(l.core_undo_fingerprint_missing(), e.core_undo_fingerprint_missing());
    assert_eq!(
        l.core_undo_destination_changed_after_move(),
        e.core_undo_destination_changed_after_move()
    );
    assert_eq!(l.core_undo_failed("boom {1}"), e.core_undo_failed("boom {1}"));
    assert_eq!(
        l.core_undo_permanently_deleted("a{b}.jpg"),
        e.core_undo_permanently_deleted("a{b}.jpg")
    );
    assert_eq!(
        UndoTexts::core_undo_recycle_target_exists(&l, "a{b}.jpg"),
        UndoTexts::core_undo_recycle_target_exists(&e, "a{b}.jpg")
    );
    assert_eq!(
        UndoTexts::core_undo_recycle_restore_failed(&l, "a{b}.jpg"),
        UndoTexts::core_undo_recycle_restore_failed(&e, "a{b}.jpg")
    );
    assert_eq!(l.core_undo_unsupported(), e.core_undo_unsupported());
    assert_eq!(
        l.core_undo_group_partially_restored(2, 5, "x.jpg, y.jpg"),
        e.core_undo_group_partially_restored(2, 5, "x.jpg, y.jpg")
    );
    assert_eq!(
        UndoTexts::core_recovery_already_handled(&l),
        UndoTexts::core_recovery_already_handled(&e)
    );
}

#[test]
fn localized_texts_equal_the_english_recovery_retry_texts_under_the_english_catalog() {
    // new: LocalizedTexts / RecoveryRetryTexts against EnglishRecoveryRetryTexts
    let _ambient = ambient();
    let l = LocalizedTexts;
    let e = EnglishRecoveryRetryTexts;
    assert_eq!(l.core_recovery_busy(), e.core_recovery_busy());
    assert_eq!(
        l.core_recovery_cancelled_needs_confirm(),
        e.core_recovery_cancelled_needs_confirm()
    );
    assert_eq!(l.core_recovery_only_move_copy(), e.core_recovery_only_move_copy());
    assert_eq!(l.core_recovery_no_destination(), e.core_recovery_no_destination());
    assert_eq!(l.core_recovery_source_missing(), e.core_recovery_source_missing());
    assert_eq!(l.core_recovery_source_changed(), e.core_recovery_source_changed());
    assert_eq!(
        l.core_recovery_destination_exists(),
        e.core_recovery_destination_exists()
    );
    assert_eq!(l.core_recovery_conflict(), e.core_recovery_conflict());
    assert_eq!(
        l.core_recovery_destination_changed(),
        e.core_recovery_destination_changed()
    );
    assert_eq!(
        RecoveryRetryTexts::core_recovery_already_handled(&l),
        RecoveryRetryTexts::core_recovery_already_handled(&e)
    );
    assert_eq!(l.core_recovery_succeeded(), e.core_recovery_succeeded());
    assert_eq!(
        l.core_recovery_succeeded_journal_failed(),
        e.core_recovery_succeeded_journal_failed()
    );
    assert_eq!(
        l.core_recovery_completed_failure_not_journaled(),
        e.core_recovery_completed_failure_not_journaled()
    );
    assert_eq!(
        RecoveryRetryTexts::core_recycle_unsupported_drive(&l, "E:\\x{y}.jpg"),
        RecoveryRetryTexts::core_recycle_unsupported_drive(&e, "E:\\x{y}.jpg")
    );
    assert_eq!(
        RecoveryRetryTexts::core_recycle_bin_cannot_hold(&l, "E:\\x{y}.jpg"),
        RecoveryRetryTexts::core_recycle_bin_cannot_hold(&e, "E:\\x{y}.jpg")
    );
    assert_eq!(
        RecoveryRetryTexts::core_undo_recycle_target_exists(&l, "a{b}.jpg"),
        RecoveryRetryTexts::core_undo_recycle_target_exists(&e, "a{b}.jpg")
    );
    assert_eq!(
        RecoveryRetryTexts::core_undo_recycle_restore_failed(&l, "a{b}.jpg"),
        RecoveryRetryTexts::core_undo_recycle_restore_failed(&e, "a{b}.jpg")
    );
}

#[test]
fn localized_texts_equal_the_english_io_texts_under_the_english_catalog() {
    // new: LocalizedTexts / FileHashTexts, SourceBytesTexts, RawDecoderTexts against their English* twins
    let _ambient = ambient();
    let l = LocalizedTexts;
    assert_eq!(
        l.file_changed_while_hashing(PATH),
        EnglishFileHashTexts.file_changed_while_hashing(PATH)
    );
    assert_eq!(
        SourceBytesTexts::file_changed_while_reading(&l, PATH),
        SourceBytesTexts::file_changed_while_reading(&EnglishSourceBytesTexts, PATH)
    );
    assert_eq!(l.unexpected_eof(PATH), EnglishSourceBytesTexts.unexpected_eof(PATH));
    let raw = EnglishRawDecoderTexts;
    assert_eq!(l.raw_corrupt(), raw.raw_corrupt());
    assert_eq!(l.raw_unsupported(), raw.raw_unsupported());
    assert_eq!(l.raw_no_preview(), raw.raw_no_preview());
    assert_eq!(
        RawDecoderTexts::file_changed_while_reading(&l, PATH),
        RawDecoderTexts::file_changed_while_reading(&raw, PATH)
    );
}

#[test]
fn localized_texts_render_the_webp_heic_and_raw_routing_sentences_with_the_file_name() {
    // new: WebpHeicTexts and RawRoutingTexts have no English* twin in the libraries; the expected sentences are the en.json ones.
    let _ambient = ambient();
    let l = LocalizedTexts;
    assert_eq!(
        l.webp_heic_disabled("a{b}.webp"),
        "WebP/HEIC support is turned off in Settings, so a{b}.webp cannot be shown."
    );
    assert_eq!(
        l.raw_disabled("a{b}.cr2"),
        "RAW support is turned off in Settings, so a{b}.cr2 cannot be shown."
    );
    assert!(
        l.webp_codec_missing("a{b}.webp")
            .starts_with("a{b}.webp is a WebP image, but Windows on this PC has no WebP decoder."),
        "{}",
        l.webp_codec_missing("a{b}.webp")
    );
    assert!(
        l.heif_codec_missing("a{b}.heic")
            .starts_with("a{b}.heic is a HEIC/HEIF photo"),
        "{}",
        l.heif_codec_missing("a{b}.heic")
    );
    // The three sentences are different texts (a swapped key would show here).
    assert_ne!(l.webp_codec_missing("x"), l.heif_codec_missing("x"));
    assert_ne!(l.webp_codec_missing("x"), l.webp_heic_disabled("x"));
}

#[test]
fn localized_texts_equal_the_english_turbo_jpeg_texts_under_the_english_catalog() {
    // new: LocalizedTexts / TurboJpegTexts against EnglishTurboJpegTexts
    let _ambient = ambient();
    let l = LocalizedTexts;
    let e = EnglishTurboJpegTexts;
    assert_eq!(l.not_jpeg(PATH), e.not_jpeg(PATH));
    assert_eq!(l.memory_buffer(), e.memory_buffer());
    assert_eq!(l.icc_fallback(), e.icc_fallback());
    assert_eq!(l.no_detail(), e.no_detail());
    assert_eq!(l.header_failed("d {1}"), e.header_failed("d {1}"));
    assert_eq!(l.invalid_dimensions(-3, 1_048_576), e.invalid_dimensions(-3, 1_048_576));
    assert_eq!(l.decompress_failed("d {1}"), e.decompress_failed("d {1}"));
    assert_eq!(l.info_failed("d {1}"), e.info_failed("d {1}"));
    assert_eq!(l.no_source(), e.no_source());
    assert_eq!(
        l.output_too_large(70_000, 80_000, 22_400_000_000),
        e.output_too_large(70_000, 80_000, 22_400_000_000)
    );
    assert_eq!(l.strict_mode_failed("d {1}"), e.strict_mode_failed("d {1}"));
    assert_eq!(l.image_not_found(PATH), e.image_not_found(PATH));
}

#[test]
fn localized_texts_equal_the_english_settings_texts_under_the_english_catalog() {
    // new: LocalizedTexts / SettingsTexts against EnglishSettingsTexts
    let _ambient = ambient();
    let l = LocalizedTexts;
    let e = EnglishSettingsTexts;
    assert_eq!(l.action_default_name(12), e.action_default_name(12));
    assert_eq!(l.action_default_backup(), e.action_default_backup());
    assert_eq!(
        l.core_settings_shortcut_invalid("Ctrl+{x}"),
        e.core_settings_shortcut_invalid("Ctrl+{x}")
    );
    assert_eq!(l.core_settings_action_invalid(), e.core_settings_action_invalid());
    assert_eq!(
        l.core_settings_action_binding_name("Keep {1}"),
        e.core_settings_action_binding_name("Keep {1}")
    );
    assert_eq!(
        l.core_settings_shortcut_duplicate("F5", "Keep, Move"),
        e.core_settings_shortcut_duplicate("F5", "Keep, Move")
    );
    assert_eq!(l.settings_load_repaired("a, b"), e.settings_load_repaired("a, b"));
    assert_eq!(
        l.dialog_import_settings_repaired("a, b"),
        e.dialog_import_settings_repaired("a, b")
    );
    assert_eq!(
        l.settings_load_shortcut_disabled("Keep {1}"),
        e.settings_load_shortcut_disabled("Keep {1}")
    );
}

#[test]
fn localized_texts_read_the_catalog_key_named_in_the_doc_of_each_method() {
    // new: under the "show keys" localizer every text is `[key]`, so each method is pinned to its own key.
    let _ambient = ambient();
    Localizer::set_current(Arc::new(create_show_keys(&built_in_english_localizer())));
    let l = LocalizedTexts;

    // JournalTexts: `english` stays on the built-in English catalog, `localized` follows the ambient one.
    assert_eq!(
        JournalTexts::english(&l, "core.journal.cancelledByUser"),
        "The operation was cancelled before it finished."
    );
    assert_eq!(
        JournalTexts::localized(&l, "core.journal.cancelledByUser"),
        key("core.journal.cancelledByUser")
    );

    // FileActionTexts
    assert_eq!(l.core_file_action_busy(), key("core.fileAction.busy"));
    assert_eq!(
        l.core_file_action_group_needs_two_paths(),
        key("core.fileAction.groupNeedsTwoPaths")
    );
    assert_eq!(
        l.core_file_action_unsupported_operation(FileOperationType::Copy),
        key("core.fileAction.unsupportedOperation")
    );
    assert_eq!(
        l.core_file_action_no_destination(),
        key("core.fileAction.noDestination")
    );
    assert_eq!(l.core_file_action_same_folder(), key("core.fileAction.sameFolder"));
    assert_eq!(
        l.core_file_action_destination_exists("p"),
        key("core.fileAction.destinationExists")
    );
    assert_eq!(
        l.core_file_action_source_missing("p"),
        key("core.fileAction.sourceMissing")
    );
    assert_eq!(
        FileActionTexts::core_recycle_unsupported_drive(&l, "n"),
        key("core.recycle.unsupportedDrive")
    );
    assert_eq!(
        FileActionTexts::core_recycle_bin_cannot_hold(&l, "n"),
        key("core.recycle.binCannotHold")
    );
    assert_eq!(
        l.core_group_action_rollback_failed("m", 1),
        key("core.groupAction.rollbackFailed")
    );

    // UndoTexts
    assert_eq!(l.core_undo_busy(), key("core.undo.busy"));
    assert_eq!(l.core_undo_no_move_to_undo(), key("core.undo.noMoveToUndo"));
    assert_eq!(l.core_undo_nothing_to_undo(), key("core.undo.nothingToUndo"));
    assert_eq!(
        l.core_undo_source_or_destination_changed(),
        key("core.undo.sourceOrDestinationChanged")
    );
    assert_eq!(l.core_undo_fingerprint_missing(), key("core.undo.fingerprintMissing"));
    assert_eq!(
        l.core_undo_destination_changed_after_move(),
        key("core.undo.destinationChangedAfterMove")
    );
    assert_eq!(l.core_undo_failed("m"), key("core.undo.failed"));
    assert_eq!(
        l.core_undo_permanently_deleted("f"),
        key("core.undo.permanentlyDeleted")
    );
    assert_eq!(
        UndoTexts::core_undo_recycle_target_exists(&l, "f"),
        key("core.undo.recycleTargetExists")
    );
    assert_eq!(
        UndoTexts::core_undo_recycle_restore_failed(&l, "f"),
        key("core.undo.recycleRestoreFailed")
    );
    assert_eq!(l.core_undo_unsupported(), key("core.undo.unsupported"));
    assert_eq!(
        l.core_undo_group_partially_restored(1, 2, "n"),
        key("core.undo.groupPartiallyRestored")
    );
    assert_eq!(
        UndoTexts::core_recovery_already_handled(&l),
        key("core.recovery.alreadyHandled")
    );

    // RecoveryRetryTexts
    assert_eq!(l.core_recovery_busy(), key("core.recovery.busy"));
    assert_eq!(
        l.core_recovery_cancelled_needs_confirm(),
        key("core.recovery.cancelledNeedsConfirm")
    );
    assert_eq!(l.core_recovery_only_move_copy(), key("core.recovery.onlyMoveCopy"));
    assert_eq!(l.core_recovery_no_destination(), key("core.recovery.noDestination"));
    assert_eq!(l.core_recovery_source_missing(), key("core.recovery.sourceMissing"));
    assert_eq!(l.core_recovery_source_changed(), key("core.recovery.sourceChanged"));
    assert_eq!(
        l.core_recovery_destination_exists(),
        key("core.recovery.destinationExists")
    );
    assert_eq!(l.core_recovery_conflict(), key("core.recovery.conflict"));
    assert_eq!(
        l.core_recovery_destination_changed(),
        key("core.recovery.destinationChanged")
    );
    assert_eq!(
        RecoveryRetryTexts::core_recovery_already_handled(&l),
        key("core.recovery.alreadyHandled")
    );
    assert_eq!(l.core_recovery_succeeded(), key("core.recovery.succeeded"));
    assert_eq!(
        l.core_recovery_succeeded_journal_failed(),
        key("core.recovery.succeededJournalFailed")
    );
    assert_eq!(
        l.core_recovery_completed_failure_not_journaled(),
        key("core.recovery.completedFailureNotJournaled")
    );
    assert_eq!(
        RecoveryRetryTexts::core_recycle_unsupported_drive(&l, "n"),
        key("core.recycle.unsupportedDrive")
    );
    assert_eq!(
        RecoveryRetryTexts::core_recycle_bin_cannot_hold(&l, "n"),
        key("core.recycle.binCannotHold")
    );
    assert_eq!(
        RecoveryRetryTexts::core_undo_recycle_target_exists(&l, "f"),
        key("core.undo.recycleTargetExists")
    );
    assert_eq!(
        RecoveryRetryTexts::core_undo_recycle_restore_failed(&l, "f"),
        key("core.undo.recycleRestoreFailed")
    );

    // FileHashTexts, SourceBytesTexts
    assert_eq!(l.file_changed_while_hashing("p"), key("err.io.fileChangedWhileHashing"));
    assert_eq!(
        SourceBytesTexts::file_changed_while_reading(&l, "p"),
        key("err.io.fileChangedWhileReading")
    );
    assert_eq!(l.unexpected_eof("p"), key("err.io.unexpectedEof"));

    // WebpHeicTexts, RawDecoderTexts, RawRoutingTexts
    assert_eq!(l.webp_heic_disabled("f"), key("image.error.webpHeicDisabled"));
    assert_eq!(l.webp_codec_missing("f"), key("image.error.webpCodecMissing"));
    assert_eq!(l.heif_codec_missing("f"), key("image.error.heifCodecMissing"));
    assert_eq!(l.raw_corrupt(), key("image.error.rawCorrupt"));
    assert_eq!(l.raw_unsupported(), key("image.error.rawUnsupported"));
    assert_eq!(l.raw_no_preview(), key("image.error.rawNoPreview"));
    assert_eq!(
        RawDecoderTexts::file_changed_while_reading(&l, "p"),
        key("err.io.fileChangedWhileReading")
    );
    assert_eq!(l.raw_disabled("f"), key("image.error.rawDisabled"));

    // TurboJpegTexts
    assert_eq!(l.not_jpeg("s"), key("err.decoder.notJpeg"));
    assert_eq!(l.memory_buffer(), key("err.decoder.memoryBuffer"));
    assert_eq!(l.icc_fallback(), key("err.decoder.iccFallback"));
    assert_eq!(l.no_detail(), key("err.decoder.noDetail"));
    assert_eq!(l.header_failed("d"), key("err.decoder.headerFailed"));
    assert_eq!(l.invalid_dimensions(1, 2), key("err.decoder.invalidDimensions"));
    assert_eq!(l.decompress_failed("d"), key("err.decoder.decompressFailed"));
    assert_eq!(l.info_failed("d"), key("err.decoder.infoFailed"));
    assert_eq!(l.no_source(), key("err.decoder.noSource"));
    assert_eq!(l.output_too_large(1, 2, 3), key("err.decoder.outputTooLarge"));
    assert_eq!(l.strict_mode_failed("d"), key("err.decoder.strictModeFailed"));
    assert_eq!(l.image_not_found("p"), key("err.io.imageNotFound"));

    // SettingsTexts
    assert_eq!(l.action_default_name(1), key("action.default.name"));
    assert_eq!(l.action_default_backup(), key("action.default.backup"));
    assert_eq!(
        l.core_settings_shortcut_invalid("n"),
        key("core.settings.shortcutInvalid")
    );
    assert_eq!(l.core_settings_action_invalid(), key("core.settings.actionInvalid"));
    assert_eq!(
        l.core_settings_action_binding_name("n"),
        key("core.settings.actionBindingName")
    );
    assert_eq!(
        l.core_settings_shortcut_duplicate("k", "n"),
        key("core.settings.shortcutDuplicate")
    );
    assert_eq!(l.settings_load_repaired("i"), key("settings.load.repaired"));
    assert_eq!(
        l.dialog_import_settings_repaired("i"),
        key("dialog.importSettings.repaired")
    );
    assert_eq!(
        l.settings_load_shortcut_disabled("n"),
        key("settings.load.shortcutDisabled")
    );
}

/// A one-language overlay on top of the built-in English: only the keys below change.
const OVERLAY_JSON: &str = r#"{
  "_meta": { "code": "xx", "name": "Test", "nativeName": "Test", "plural": "none" },
  "core.fileAction.busy": "XX busy",
  "core.fileAction.destinationExists": "XX exists: {path}",
  "core.undo.failed": "XX undo: {message}",
  "core.journal.cancelledByUser": "XX cancelled",
  "image.error.rawCorrupt": "XX corrupt"
}"#;

fn overlay_localizer() -> Arc<Localizer> {
    let mut warnings = Vec::new();
    let overlay = LanguageCatalog::try_parse(OVERLAY_JSON, "overlay", &mut warnings)
        .unwrap_or_else(|| panic!("overlay catalog is invalid: {}", warnings.join("; ")));
    Arc::new(Localizer::create(built_in_english(), &[overlay], &warnings))
}

#[test]
fn localized_texts_follow_the_ambient_language_at_every_call() {
    // new: a language switch between two calls of the SAME value changes the text (C# `Tr.*` reads `Localizer.Current` each time)
    let _ambient = ambient();
    let l = LocalizedTexts;
    assert_eq!(l.core_file_action_busy(), "A previous operation is still running.");
    assert_eq!(l.core_undo_failed("m"), "Could not undo: m");

    Localizer::set_current(overlay_localizer());
    assert_eq!(l.core_file_action_busy(), "XX busy");
    assert_eq!(l.core_file_action_destination_exists("D:\\a"), "XX exists: D:\\a");
    assert_eq!(l.core_undo_failed("m"), "XX undo: m");
    assert_eq!(l.raw_corrupt(), "XX corrupt");
    // A key the overlay does not translate falls back to English.
    assert_eq!(l.raw_unsupported(), "Unsupported RAW container format.");
    // The journal stores the INVARIANT English text whatever the UI language is; only `localized` follows the switch.
    assert_eq!(
        JournalTexts::english(&l, "core.journal.cancelledByUser"),
        "The operation was cancelled before it finished."
    );
    assert_eq!(
        JournalTexts::localized(&l, "core.journal.cancelledByUser"),
        "XX cancelled"
    );

    Localizer::set_current(built_in_english_localizer());
    assert_eq!(l.core_file_action_busy(), "A previous operation is still running.");
    assert_eq!(
        JournalTexts::localized(&l, "core.journal.cancelledByUser"),
        JournalTexts::english(&l, "core.journal.cancelledByUser")
    );
}

#[test]
fn text_source_follows_the_ambient_localizer_and_formats_with_it() {
    // new: TextSource::localizer is the ambient localizer (same Arc), not a copy taken at construction
    let _ambient = ambient();
    let l = LocalizedTexts;
    assert!(Arc::ptr_eq(&l.localizer(), &Localizer::current()));
    let overlay = overlay_localizer();
    Localizer::set_current(Arc::clone(&overlay));
    assert!(Arc::ptr_eq(&l.localizer(), &overlay));
    // The provided methods of the trait go through the same localizer.
    assert_eq!(TextSource::get(&l, "core.fileAction.busy"), "XX busy");
    assert_eq!(
        TextSource::format(
            &l,
            "core.undo.failed",
            &[pr_l10n::template::LocArg::new("message", "m")]
        ),
        "XX undo: m"
    );
}

#[test]
fn text_source_numbers_are_the_ambient_number_culture() {
    // new: TextSource::numbers is `number_culture()` of NOW, and it is the culture the numeric placeholders are written with
    let _ambient = ambient();
    let l = LocalizedTexts;
    assert_eq!(*l.numbers(), NumberCulture::invariant());
    assert_eq!(
        l.invalid_dimensions(-5, -6),
        "The decoder reported invalid image dimensions: -5x-6"
    );

    let culture = NumberCulture {
        negative_sign: "\u{2212}".to_owned(),
        ..NumberCulture::invariant()
    };
    set_number_culture(culture.clone());
    assert_eq!(*l.numbers(), culture);
    assert!(Arc::ptr_eq(&l.numbers(), &number_culture()));
    assert_eq!(
        l.invalid_dimensions(-5, -6),
        "The decoder reported invalid image dimensions: \u{2212}5x\u{2212}6"
    );
}

#[test]
fn counts_larger_than_the_catalog_integer_saturate_instead_of_wrapping() {
    // new: `usize` counts become the catalog's signed 64-bit integer; a value that does not fit saturates (never a negative number)
    let _ambient = ambient();
    let l = LocalizedTexts;
    assert!(
        l.core_undo_group_partially_restored(usize::MAX, 3, "n")
            .starts_with("Restored 9223372036854775807 of 3 files.")
    );
    assert!(
        l.core_group_action_rollback_failed("m", usize::MAX)
            .contains("9223372036854775807")
    );
}
