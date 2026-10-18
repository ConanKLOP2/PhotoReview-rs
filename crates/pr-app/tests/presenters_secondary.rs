//! Red tests (W8-10c) for `presenters::secondary`: the click zoom prompt, diagnostics, batch review and skipped files.
//!
//! Port of `PhotoReview.App.Tests/Presenters/SecondaryDialogPresenterTests.cs`, the presenter parts of
//! `Localization/DiagnosticsUnitsLeftoversTests.cs` and `MainWindowSupportTests.ExplorerStatusText_*` (baseline `d92d6aae`).
//! Each C# twin is named in a `// C#:` comment; tests written from the doc comments carry `// new:`.
//!
//! English expectations are the catalog texts of `assets/languages/en.json`; Vietnamese expectations are the texts of
//! `assets/languages/vi.json`, loaded at test time with `LanguageCatalog::try_parse` (decision OQ-2). Number formatting
//! uses the hand-written en-US [`EnUs`] (decision D-W8-10c-2). Every test calls a stub that is still `todo!()`.
//!
//! DIVERGENCE: the C# `StatusText((ExplorerOrderStatus)42)` and `DiagnosticsPresenter.StatusText` of an unknown status
//! (C# `"99"`) have no Rust form: `ExplorerOrderStatus` is a closed enum with seven variants (contracts/README.md D5,
//! trap T13), so only the seven texts are tested.

use std::path::PathBuf;
use std::sync::Arc;

use pr_app::presenters::secondary::{
    BatchReviewPresenter, ClickZoomCustomPresenter, DiagnosticsPresenter, NumberFormatting, SkippedFilesPresenter,
    file_name,
};
use pr_core::abstractions::{BatchReviewItem, SkippedEntry, SkippedKind};
use pr_core::catalog::explorer_reason::{self, ALL_CODES, COM_CALL_FAILED, EMPTY_VIEW, TIMEOUT};
use pr_core::catalog::explorer_types::{ExplorerGroupState, ExplorerOrderStatus, ExplorerViewSnapshot};
use pr_core::diagnostics::metrics::{PresentLatencyBucket, ReviewMetricsSnapshot, SourceOpenEntry};
use pr_core::settings::app_settings::{MAX_CLICK_ZOOM_PERCENT, MIN_CLICK_ZOOM_PERCENT};
use pr_core::time::DateTimeUtc;
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::{Localizer, built_in_english, built_in_english_localizer};
use pr_l10n::template::LocArg;

/// The en-US number formats (C# `CultureInfo.GetCultureInfo("en-US")`): group separator `,`, `N0` rounding half away from
/// zero, and the ICU percent pattern `#,##0%` (no space: `0.75` gives `75.0%`, confirmed in PowerShell on en-US, OQ-5).
#[derive(Debug)]
struct EnUs;

fn group_digits(digits: &str) -> String {
    let mut out = String::new();
    let len = digits.len();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl NumberFormatting for EnUs {
    fn grouped_i64(&self, value: i64) -> String {
        let body = group_digits(&value.unsigned_abs().to_string());
        if value < 0 { format!("-{body}") } else { body }
    }

    fn grouped_f64(&self, value: f64) -> String {
        // f64::round rounds half away from zero (2.5 -> 3), the N0 rule of the W8-01a format_fixed.
        self.grouped_i64(value.round() as i64)
    }

    fn percent_one_decimal(&self, ratio: f64) -> String {
        format!("{:.1}%", ratio * 100.0)
    }
}

/// The English localizer (the built-in catalog, no overlays).
fn english() -> Arc<Localizer> {
    built_in_english_localizer()
}

/// The Vietnamese localizer: `assets/languages/vi.json` as an overlay on the built-in English catalog (OQ-2).
fn vietnamese() -> Localizer {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("languages")
        .join("vi.json");
    let text = std::fs::read_to_string(&path).expect("assets/languages/vi.json");
    let mut warnings = Vec::new();
    let vi = LanguageCatalog::try_parse(&text, &path.to_string_lossy(), &mut warnings).expect("valid catalog");
    assert!(warnings.is_empty(), "{warnings:?}");
    Localizer::create(built_in_english(), &[vi], &[])
}

/// An Explorer snapshot with the given status and reason; the other fields are empty.
fn explorer(status: ExplorerOrderStatus, reason: Option<String>, ordered: &[&str]) -> ExplorerViewSnapshot {
    ExplorerViewSnapshot {
        folder: "C:\\p".to_string(),
        ordered_paths: ordered.iter().map(|s| s.to_string()).collect(),
        sort_columns: Vec::new(),
        group_state: ExplorerGroupState::Active,
        status,
        reason,
        captured_utc: DateTimeUtc::MIN,
    }
}

// ---- Click zoom custom ----

#[test]
fn click_zoom_custom_accepts_range_only() {
    // C#: SecondaryDialogPresenterTests.ClickZoomCustom_AcceptsRangeOnly (line 180)
    let l = english();
    let mut presenter = ClickZoomCustomPresenter::new(100);
    assert_eq!(presenter.initial_text(), "100");

    let min_text = MIN_CLICK_ZOOM_PERCENT.to_string();
    let max_text = MAX_CLICK_ZOOM_PERCENT.to_string();
    let below_text = (MIN_CLICK_ZOOM_PERCENT - 1).to_string();
    let above_text = (MAX_CLICK_ZOOM_PERCENT + 1).to_string();

    assert_eq!(presenter.try_accept(&l, Some(min_text.as_str())), Ok(()));
    assert_eq!(presenter.value(), MIN_CLICK_ZOOM_PERCENT);
    assert_eq!(presenter.try_accept(&l, Some(max_text.as_str())), Ok(()));
    assert_eq!(presenter.value(), MAX_CLICK_ZOOM_PERCENT);

    let message = l.format(
        "dialog.settingsInvalid.clickZoomPercent",
        &[
            LocArg::new("min", MIN_CLICK_ZOOM_PERCENT),
            LocArg::new("max", MAX_CLICK_ZOOM_PERCENT),
        ],
    );
    assert_eq!(message, "Click zoom level must be a number between 10 and 800.");
    assert_eq!(
        presenter.try_accept(&l, Some(below_text.as_str())),
        Err(message.clone())
    );
    assert_eq!(
        presenter.try_accept(&l, Some(above_text.as_str())),
        Err(message.clone())
    );
    assert_eq!(presenter.try_accept(&l, Some("12x")), Err(message.clone()));
    assert_eq!(presenter.try_accept(&l, None), Err(message));
    assert_eq!(presenter.value(), MAX_CLICK_ZOOM_PERCENT); // a refusal keeps the last accepted value
}

#[test]
fn click_zoom_custom_initial_text_is_invariant_digits() {
    // new: from semantics 4 (the box starts with the invariant decimal digits of the current value)
    assert_eq!(ClickZoomCustomPresenter::new(150).initial_text(), "150");
    assert_eq!(ClickZoomCustomPresenter::new(-5).initial_text(), "-5");
}

#[test]
fn click_zoom_custom_parsing_follows_number_styles_integer() {
    // new: T1 (.NET int.TryParse with NumberStyles.Integer and the invariant culture). Accepted: white space made of
    // U+0009..U+000D and U+0020 around the number, and one leading sign. Refused: anything else, U+00A0 included.
    let l = english();
    for accepted in [" 150 ", "+150", "\t800\r\n"] {
        let mut presenter = ClickZoomCustomPresenter::new(100);
        assert_eq!(
            presenter.try_accept(&l, Some(accepted)),
            Ok(()),
            "accepted: {accepted:?}"
        );
    }
    for refused in ["1 50", "1,000", "12.0", "1e2", "\u{a0}150", "2147483648", "", "   "] {
        let mut presenter = ClickZoomCustomPresenter::new(100);
        assert!(presenter.try_accept(&l, Some(refused)).is_err(), "refused: {refused:?}");
    }
}

#[test]
fn click_zoom_custom_value_is_zero_before_the_first_accept() {
    // new: T2 (`value()` changes only on Ok; before any accepted text it is 0, not the initial text)
    let l = english();
    let mut presenter = ClickZoomCustomPresenter::new(150);
    assert_eq!(presenter.value(), 0);
    assert!(presenter.try_accept(&l, Some("12x")).is_err());
    assert_eq!(presenter.value(), 0);
}

// ---- Diagnostics ----

#[test]
fn diagnostics_empty_snapshot() {
    // C#: SecondaryDialogPresenterTests.Diagnostics_EmptySnapshot (line 209)
    let l = english();
    let display = DiagnosticsPresenter::build(&ReviewMetricsSnapshot::default(), None, &l, &EnUs);

    assert_eq!(display.hit_rate, "N/A");
    assert_eq!(display.present_latency, "N/A");
    assert_eq!(display.present_histogram, "N/A");
    assert_eq!(display.explorer_metadata, "N/A");
    assert_eq!(display.explorer_order, "Not queried yet");
    assert_eq!(display.source_opens, "0");
    assert_eq!(display.cache_hits, "0");
}

#[test]
fn diagnostics_formats_with_current_culture() {
    // C#: SecondaryDialogPresenterTests.Diagnostics_FormatsWithCurrentCulture (line 223, en-US)
    let snapshot = ReviewMetricsSnapshot {
        cache_hits: 3000,
        cache_misses: 1000,
        source_bytes_read: 1_234_567,
        source_reads: 2,
        decode_milliseconds: 4500,
        presented_images: 4,
        present_milliseconds: 80,
        stat_count: 7,
        session_write_count: 8,
        cross_thread_present_count: 9,
        source_open_count: 2,
        top_source_opens: vec![SourceOpenEntry {
            path: "C:\\photos\\a.jpg".to_string(),
            count: 2,
        }],
        present_histogram: vec![PresentLatencyBucket {
            label: "<16ms".to_string(),
            count: 3,
        }],
        ..Default::default()
    };

    let display = DiagnosticsPresenter::build(&snapshot, None, &english(), &EnUs);

    assert_eq!(display.cache_hits, "3,000");
    assert_eq!(display.cache_misses, "1,000");
    assert_eq!(display.hit_rate, "75.0%");
    assert_eq!(display.source_bytes, "1,234,567 bytes");
    assert_eq!(display.source_reads, "2");
    assert_eq!(display.decode_time, "4,500 ms");
    assert_eq!(display.present_latency, "20 ms/image (4)");
    assert_eq!(display.decoder_fallbacks, "0");
    assert_eq!(display.stat_count, "7");
    assert_eq!(display.session_writes, "8");
    assert_eq!(display.cross_thread_presents, "9");
    assert!(display.source_opens.contains("a.jpg"));
    assert!(!display.source_opens.contains("photos"));
    assert!(display.present_histogram.contains("<16ms"));
}

#[test]
fn diagnostics_hit_rate_is_zero_percent_when_only_misses_exist() {
    // new: T3 (the zero test uses hits + misses; zero hits with some misses is 0.0%, not N/A)
    let snapshot = ReviewMetricsSnapshot {
        cache_hits: 0,
        cache_misses: 5,
        ..Default::default()
    };
    assert_eq!(
        DiagnosticsPresenter::build(&snapshot, None, &english(), &EnUs).hit_rate,
        "0.0%"
    );
}

#[test]
fn diagnostics_explorer_snapshot() {
    // C#: SecondaryDialogPresenterTests.Diagnostics_ExplorerSnapshot (line 250)
    let l = english();
    let snapshot = ReviewMetricsSnapshot::default();
    let ok = explorer(ExplorerOrderStatus::Available, None, &["a", "b", "c"]);
    let failed = explorer(
        ExplorerOrderStatus::TimedOut,
        Some(explorer_reason::format(TIMEOUT, &[])),
        &[],
    );

    let ok_display = DiagnosticsPresenter::build(&snapshot, Some(&ok), &l, &EnUs);
    let failed_display = DiagnosticsPresenter::build(&snapshot, Some(&failed), &l, &EnUs);

    assert_eq!(ok_display.explorer_order, "Available: 3 items");
    assert_eq!(
        failed_display.explorer_order,
        "TimedOut: The Explorer query took too long."
    );
    assert_eq!(ok_display.explorer_metadata, "0 sort column(s), group=Active");
}

#[test]
fn diagnostics_unknown_reason() {
    // C#: SecondaryDialogPresenterTests.Diagnostics_UnknownReason (line 265). The C# `StatusText(99)` half is a
    // DIVERGENCE (closed enum, trap T13): only the reason half is checked here.
    let l = english();
    assert_eq!(
        DiagnosticsPresenter::reason_text("weird thing", &l),
        "Unknown reason: weird thing"
    );
}

#[test]
fn diagnostics_source_opens_zero_is_literal_zero_and_count_is_not_grouped() {
    // new: T6 and T10. A zero count is the literal "0" (not localized); the total is grouped, the per-entry count is raw;
    // an empty top list keeps the separator, so the summary is "1,234 · ".
    let l = english();
    let zero = ReviewMetricsSnapshot::default();
    assert_eq!(DiagnosticsPresenter::build(&zero, None, &l, &EnUs).source_opens, "0");

    let empty_top = ReviewMetricsSnapshot {
        source_open_count: 1234,
        ..Default::default()
    };
    assert_eq!(
        DiagnosticsPresenter::build(&empty_top, None, &l, &EnUs).source_opens,
        "1,234 · "
    );

    let one_entry = ReviewMetricsSnapshot {
        source_open_count: 1234,
        top_source_opens: vec![SourceOpenEntry {
            path: "C:\\p\\a.jpg".to_string(),
            count: 1234,
        }],
        ..Default::default()
    };
    assert_eq!(
        DiagnosticsPresenter::build(&one_entry, None, &l, &EnUs).source_opens,
        "1,234 · a.jpg×1234"
    );
}

#[test]
fn diagnostics_present_latency_uses_presented_images_and_rounds_half_away() {
    // new: T4 and T11. The average divides as f64 by `presented_images`; 5 ms over 2 images is 2.5, which rounds to 3 with
    // the en-US N0 rule. With no presented image the text is N/A, even when `present_milliseconds` is non-zero.
    let l = english();
    let two = ReviewMetricsSnapshot {
        presented_images: 2,
        present_milliseconds: 5,
        ..Default::default()
    };
    assert_eq!(
        DiagnosticsPresenter::build(&two, None, &l, &EnUs).present_latency,
        "3 ms/image (2)"
    );

    let none = ReviewMetricsSnapshot {
        presented_images: 0,
        present_milliseconds: 5,
        ..Default::default()
    };
    assert_eq!(
        DiagnosticsPresenter::build(&none, None, &l, &EnUs).present_latency,
        "N/A"
    );
}

#[test]
fn diagnostics_explorer_none_is_not_queried_but_metadata_is_not_available() {
    // new: T7. `explorer: None` means two different texts: the order says "not queried", the metadata says "N/A".
    let display = DiagnosticsPresenter::build(&ReviewMetricsSnapshot::default(), None, &english(), &EnUs);
    assert_eq!(display.explorer_order, "Not queried yet");
    assert_eq!(display.explorer_metadata, "N/A");
}

#[test]
fn diagnostics_explorer_some_empty_reason_is_unknown_not_absent() {
    // new: T8. `reason: Some("")` is a reason: it goes through reason_text(""), which gives the unknown text with an empty tail.
    let l = english();
    let snapshot = explorer(ExplorerOrderStatus::TimedOut, Some(String::new()), &[]);
    let display = DiagnosticsPresenter::build(&ReviewMetricsSnapshot::default(), Some(&snapshot), &l, &EnUs);
    assert_eq!(display.explorer_order, "TimedOut: Unknown reason: ");
}

#[test]
fn status_text_maps_every_status_to_its_own_translation() {
    // C#: MainWindowSupportTests.ExplorerStatusText_MapsEveryStatusToItsOwnTranslation_AndUnknownToItsName (line 139).
    // The C# `(ExplorerOrderStatus)42` line is a DIVERGENCE (closed enum, trap T13): seven texts only.
    let l = english();
    let expected = [
        (ExplorerOrderStatus::Available, "Available"),
        (ExplorerOrderStatus::NoMatchingWindow, "NoMatchingWindow"),
        (ExplorerOrderStatus::NativeViewUnavailable, "NativeViewUnavailable"),
        (ExplorerOrderStatus::InvalidSnapshot, "InvalidSnapshot"),
        (ExplorerOrderStatus::TimedOut, "TimedOut"),
        (ExplorerOrderStatus::Canceled, "Canceled"),
        (ExplorerOrderStatus::Failed, "Failed"),
    ];
    for (status, text) in expected {
        assert_eq!(DiagnosticsPresenter::status_text(status, &l), text, "{status:?}");
    }
    let mut distinct: Vec<&str> = expected.iter().map(|(_, text)| *text).collect();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 7); // seven different texts: a swapped arm cannot hide

    let vi = vietnamese();
    assert_eq!(
        DiagnosticsPresenter::status_text(ExplorerOrderStatus::TimedOut, &vi),
        "Hết thời gian chờ"
    );
    assert_eq!(
        DiagnosticsPresenter::status_text(ExplorerOrderStatus::Available, &vi),
        "Sẵn sàng"
    );
}

#[test]
fn every_explorer_reason_code_maps_to_catalog_text_in_both_languages() {
    // C#: DiagnosticsUnitsLeftoversTests.EveryExplorerReasonCode_MapsToCatalogText_InBothLanguages (line 26).
    // The C# reflection over ExplorerReason literals becomes a loop over ALL_CODES (T17: twelve codes).
    assert_eq!(ALL_CODES.len(), 12);
    let en = english();
    let vi = vietnamese();
    for l in [en.as_ref(), &vi] {
        for code in ALL_CODES {
            let reason = explorer_reason::format(code, &["d0", "d1"]);
            let text = DiagnosticsPresenter::reason_text(&reason, l);
            let unknown = l.format("diag.explorerReason.unknown", &[LocArg::new("reason", reason.clone())]);
            assert_ne!(unknown, text, "{code} is not translated");
            assert_ne!(code, text);
            assert!(!text.contains('|'), "{code}: {text}");
        }
    }
}

#[test]
fn reason_text_inserts_technical_details_verbatim_and_translates_the_sentence() {
    // C#: DiagnosticsUnitsLeftoversTests.ReasonText_InsertsTechnicalDetailsVerbatim_AndTranslatesTheSentence (line 43)
    let reason = explorer_reason::format(COM_CALL_FAILED, &["IFolderView2.GetItem(3)", "0x80004005"]);
    assert_eq!(
        DiagnosticsPresenter::reason_text(&reason, &english()),
        "Explorer call IFolderView2.GetItem(3) failed (HRESULT 0x80004005)."
    );
    assert_eq!(
        DiagnosticsPresenter::reason_text(&reason, &vietnamese()),
        "Lệnh gọi Explorer IFolderView2.GetItem(3) thất bại (HRESULT 0x80004005)."
    );
}

#[test]
fn reason_text_unknown_reason_is_wrapped_not_dropped() {
    // C#: DiagnosticsUnitsLeftoversTests.ReasonText_UnknownReason_IsWrappedNotDropped (line 55)
    assert_eq!(
        DiagnosticsPresenter::reason_text("from a newer build", &english()),
        "Unknown reason: from a newer build"
    );
}

#[test]
fn reason_text_argument_order_follows_placeholder_names() {
    // new: semantics 7 table. The detail index follows the placeholder order: `empty-view` takes hresult first, then count.
    let reason = explorer_reason::format(EMPTY_VIEW, &["0x80004005", "3"]);
    assert_eq!(
        DiagnosticsPresenter::reason_text(&reason, &english()),
        "The Explorer view is empty or its item count could not be read (HRESULT 0x80004005, 3 items)."
    );
    assert_eq!(
        DiagnosticsPresenter::reason_text("empty-view|0x80004005|3", &english()),
        "The Explorer view is empty or its item count could not be read (HRESULT 0x80004005, 3 items)."
    );
}

#[test]
fn group_state_name_is_the_english_member_name() {
    // new: semantics 9 and T9. The enum member name is used in both languages (the C# enum is passed unformatted).
    assert_eq!(DiagnosticsPresenter::group_state_name(ExplorerGroupState::None), "None");
    assert_eq!(
        DiagnosticsPresenter::group_state_name(ExplorerGroupState::Active),
        "Active"
    );
    assert_eq!(
        DiagnosticsPresenter::group_state_name(ExplorerGroupState::Unknown),
        "Unknown"
    );

    let snapshot = explorer(ExplorerOrderStatus::Available, None, &[]);
    let display = DiagnosticsPresenter::build(&ReviewMetricsSnapshot::default(), Some(&snapshot), &vietnamese(), &EnUs);
    assert_eq!(display.explorer_metadata, "0 cột sắp xếp, nhóm=Active");
}

// ---- Batch review ----

#[test]
fn batch_review_lines() {
    // C#: SecondaryDialogPresenterTests.BatchReview_Lines (line 274, en-US)
    let l = english();
    let items = [
        BatchReviewItem {
            path: PathBuf::from("C:\\a\\one.jpg"),
            length: Some(1_234_567),
        },
        BatchReviewItem {
            path: PathBuf::from("C:\\a\\gone.jpg"),
            length: None,
        },
    ];
    let lines = BatchReviewPresenter::build_lines(&items, &l, &EnUs);

    assert_eq!(lines[0], "one.jpg    (1,234,567 bytes)    C:\\a\\one.jpg");
    assert_eq!(lines[1], "gone.jpg    (no longer exists)    C:\\a\\gone.jpg");
    assert_eq!(
        BatchReviewPresenter::summary(2, &l),
        "2 files will be moved to the Recycle Bin. Check the list before confirming."
    );
}

#[test]
fn batch_review_path_only_form() {
    // C#: SecondaryDialogPresenterTests.BatchReview_PathOnlyForm (line 285)
    let items = BatchReviewPresenter::from_paths(&[PathBuf::from("C:\\x\\a.jpg")]);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].length, None);
    let lines = BatchReviewPresenter::build_lines(&items, &english(), &EnUs);
    assert_eq!(lines, ["a.jpg    (no longer exists)    C:\\x\\a.jpg"]);
}

#[test]
fn batch_review_zero_length_is_a_size_not_missing() {
    // new: T12 (`Some(0)` is a known size and shows 0; only `None` is the missing line)
    let item = BatchReviewItem {
        path: PathBuf::from("C:\\x\\empty.jpg"),
        length: Some(0),
    };
    let line = BatchReviewPresenter::build_item(&item, &english(), &EnUs);
    assert_eq!(line, "empty.jpg    (0 bytes)    C:\\x\\empty.jpg");
    assert!(!line.contains("no longer exists"));
}

#[test]
fn batch_review_summary_is_plural_by_count() {
    // new: semantics 12. `.one` for exactly 1; `.other` for 0, 2 and -1.
    let l = english();
    assert_eq!(
        BatchReviewPresenter::summary(1, &l),
        "1 file will be moved to the Recycle Bin. Check the list before confirming."
    );
    assert_eq!(
        BatchReviewPresenter::summary(0, &l),
        "0 files will be moved to the Recycle Bin. Check the list before confirming."
    );
    assert_eq!(
        BatchReviewPresenter::summary(2, &l),
        "2 files will be moved to the Recycle Bin. Check the list before confirming."
    );
    assert!(
        BatchReviewPresenter::summary(-1, &l)
            .ends_with("files will be moved to the Recycle Bin. Check the list before confirming.")
    );
}

#[test]
fn file_name_takes_text_after_last_separator() {
    // new: T5 (C# Path.GetFileName is the text after the last separator, not std::path::Path::file_name)
    assert_eq!(file_name("C:\\a\\one.jpg"), "one.jpg");
    assert_eq!(file_name("C:\\a\\"), "");
    assert_eq!(file_name("a/b"), "b");
    assert_eq!(file_name("C:\\a\\.."), "..");
    assert_eq!(file_name("one.jpg"), "one.jpg");
}

// ---- Skipped files ----

#[test]
fn skipped_files_lines() {
    // C#: SecondaryDialogPresenterTests.SkippedFiles_Lines (line 294)
    let l = english();
    let entries = [
        SkippedEntry {
            path: PathBuf::from("C:\\a\\bad.jpg"),
            reason: "Access to the path is denied".to_string(),
            kind: SkippedKind::FileUnreadable,
        },
        SkippedEntry {
            path: PathBuf::from("C:\\a"),
            reason: "The network path was not found".to_string(),
            kind: SkippedKind::ListingInterrupted,
        },
    ];
    let lines = SkippedFilesPresenter::build_lines(&entries, &l);

    assert_eq!(
        lines[0],
        "C:\\a\\bad.jpg  (cannot be read: Access to the path is denied)"
    );
    assert_eq!(
        lines[1],
        "C:\\a  (folder listing stopped early: The network path was not found)"
    );
    assert_eq!(SkippedFilesPresenter::title(&l), "Skipped files");
    assert_eq!(
        SkippedFilesPresenter::intro(&l),
        "These files could not be read (access denied, locked or I/O error), so they are not in the image list. The list may be incomplete."
    );
    assert_eq!(SkippedFilesPresenter::close_text(&l), "Close");
}

#[test]
fn skipped_entry_wraps_os_message_in_translated_category() {
    // C#: DiagnosticsUnitsLeftoversTests.SkippedEntry_WrapsTheOsMessageInATranslatedCategory (line 74)
    let file = SkippedEntry {
        path: PathBuf::from("C:\\p\\a.jpg"),
        reason: "OS says no".to_string(),
        kind: SkippedKind::FileUnreadable,
    };
    let listing = SkippedEntry {
        path: PathBuf::from("C:\\p"),
        reason: "disk gone".to_string(),
        kind: SkippedKind::ListingInterrupted,
    };

    let en = english();
    assert_eq!(
        SkippedFilesPresenter::format_entry(&file, &en),
        "C:\\p\\a.jpg  (cannot be read: OS says no)"
    );
    assert_eq!(
        SkippedFilesPresenter::format_entry(&listing, &en),
        "C:\\p  (folder listing stopped early: disk gone)"
    );

    let vi = vietnamese();
    assert_eq!(
        SkippedFilesPresenter::format_entry(&file, &vi),
        "C:\\p\\a.jpg  (không đọc được: OS says no)"
    );
    assert_eq!(
        SkippedFilesPresenter::format_entry(&listing, &vi),
        "C:\\p  (việc liệt kê thư mục dừng giữa chừng: disk gone)"
    );
}

#[test]
fn skipped_files_titles_are_translated_in_both_languages() {
    // new: the `main.skipped.window.*` texts differ from the English text in Vietnamese
    let en = english();
    let vi = vietnamese();
    assert_eq!(SkippedFilesPresenter::title(&vi), "Tệp bị bỏ qua");
    assert_eq!(SkippedFilesPresenter::close_text(&vi), "Đóng");
    for (english_text, vietnamese_text) in [
        (SkippedFilesPresenter::title(&en), SkippedFilesPresenter::title(&vi)),
        (SkippedFilesPresenter::intro(&en), SkippedFilesPresenter::intro(&vi)),
        (
            SkippedFilesPresenter::close_text(&en),
            SkippedFilesPresenter::close_text(&vi),
        ),
    ] {
        assert!(!vietnamese_text.is_empty());
        assert_ne!(english_text, vietnamese_text);
    }
}

#[test]
fn from_paths_has_no_sizes() {
    // new: semantics 11 (`from_paths` gives `length: None` for every path)
    let paths = [PathBuf::from("C:\\x\\a.jpg"), PathBuf::from("C:\\x\\b.jpg")];
    let items = BatchReviewPresenter::from_paths(&paths);
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|item| item.length.is_none()));
    assert_eq!(items[1].path, PathBuf::from("C:\\x\\b.jpg"));
}
