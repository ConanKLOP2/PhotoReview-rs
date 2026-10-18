//! F tests for `pr_app::presenters::benchmark_presentation` (W8-10h, red: the bodies are `todo!()`).
//!
//! Port of `tests/PhotoReview.App.Tests/Localization/BenchmarkRowTextTests.cs`, `BenchmarkTextTests.cs` and the
//! benchmark part of `DiagnosticsUnitsLeftoversTests.cs` (baseline `d92d6aae`). Each test names its C# twin in a
//! `// C#:` comment; tests with no C# twin are marked `// new:`. The English localizer is the built-in catalog, the
//! Vietnamese one is the English catalog plus `assets/languages/vi.json`. Non-portable C# assertions (`null`,
//! `(LoadingMode)99`, `(BenchmarkWorkload)99`, `PropertyChanged`) are documented skips, see the contract.

use std::path::Path;
use std::sync::Arc;

use pr_app::presenters::benchmark_presentation::{
    BEST_SYMBOL, BenchmarkProfileItem, BenchmarkResultRow, KEY_PROGRESS_CORRECTNESS_FAILED, KEY_PROGRESS_OK,
    PROFILE_KEY_PREFIX, RowError, describe_error, format_milliseconds_f0, loading_mode_text, milliseconds_text,
    profile_description, profile_key, profile_name, progress_message, workload_text,
};
use pr_bench::engine::{PROGRESS_CORRECTNESS_FAILED, PROGRESS_OK, RESULT_CORRECTNESS_FAILED};
use pr_bench::models::{
    BenchmarkPhaseResult, BenchmarkProfile, BenchmarkProfileError, BenchmarkProfileProblem, BenchmarkResultStatus,
    BenchmarkWorkload,
};
use pr_bench::profiles;
use pr_core::model::enums::LoadingMode;
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::{Localizer, built_in_english, built_in_english_localizer};

/// The English localizer (built-in catalog).
fn english() -> Arc<Localizer> {
    built_in_english_localizer()
}

/// The Vietnamese localizer: the English catalog plus the shipped `assets/languages/vi.json`.
fn vietnamese() -> Localizer {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("languages")
        .join("vi.json");
    let text = std::fs::read_to_string(&path).expect("assets/languages/vi.json");
    let mut warnings = Vec::new();
    let vi = LanguageCatalog::try_parse(&text, &path.to_string_lossy(), &mut warnings).expect("valid vi.json");
    Localizer::create(built_in_english(), &[vi], &[])
}

/// A registry profile by id (the C# `BenchmarkProfiles.Find(id)!`).
fn registry(id: &str) -> BenchmarkProfile {
    profiles::find(id)
        .unwrap_or_else(|| panic!("registry has no profile '{id}'"))
        .clone()
}

/// C# `Profile => BenchmarkProfiles.All[0]`.
fn first_profile() -> BenchmarkProfile {
    profiles::all()[0].clone()
}

/// C# `Phase(status, message, samples, workload)` with the profile id of the row.
fn phase(
    profile_id: &str,
    workload: BenchmarkWorkload,
    samples: Vec<f64>,
    status: BenchmarkResultStatus,
    message: Option<&str>,
) -> BenchmarkPhaseResult {
    BenchmarkPhaseResult::new(profile_id, workload, samples, status, message.map(str::to_owned), None)
}

/// The default test phase: four samples, Sequential workload (C# `Phase` helper defaults).
fn default_phase(profile_id: &str, status: BenchmarkResultStatus, message: Option<&str>) -> BenchmarkPhaseResult {
    phase(
        profile_id,
        BenchmarkWorkload::Sequential,
        vec![10.0, 20.0, 30.0, 40.0],
        status,
        message,
    )
}

// ---------------------------------------------------------------------------------------------------------------------
// BenchmarkRowTextTests.cs
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn loading_mode_text_maps_every_mode_to_its_own_text() {
    // C#: BenchmarkRowTextTests.LoadingModeText_MapsEveryModeToItsOwnText_AndUnknownToItsName
    // (the `(LoadingMode)99` assertion is N/A: the enum is closed, decision D-10h-4)
    let en = english();
    assert_eq!(loading_mode_text(LoadingMode::Fast, &en), "Fast");
    assert_eq!(loading_mode_text(LoadingMode::Preview, &en), "Preview");
    assert_eq!(loading_mode_text(LoadingMode::Original, &en), "Original");
    let distinct: std::collections::HashSet<String> = [LoadingMode::Fast, LoadingMode::Preview, LoadingMode::Original]
        .into_iter()
        .map(|mode| loading_mode_text(mode, &en))
        .collect();
    assert_eq!(distinct.len(), 3);
}

#[test]
fn workload_text_maps_every_workload_to_its_own_text() {
    // C#: BenchmarkRowTextTests.WorkloadText_MapsEveryWorkloadToItsOwnText_AndUnknownToItsName
    // (the `(BenchmarkWorkload)99` assertion is N/A: the enum is closed, decision D-10h-4)
    let en = english();
    let expected = [
        (BenchmarkWorkload::FirstFrame, "FirstFrame"),
        (BenchmarkWorkload::Sequential, "Sequential"),
        (BenchmarkWorkload::Random, "Random"),
        (BenchmarkWorkload::WarmNext, "WarmNext"),
        (BenchmarkWorkload::Preload, "Preload"),
        (BenchmarkWorkload::FileAction, "FileAction"),
        (BenchmarkWorkload::Correctness, "Correctness"),
    ];
    for (workload, text) in expected {
        assert_eq!(workload_text(workload, &en), text);
    }
    let distinct: std::collections::HashSet<String> = BenchmarkWorkload::ALL
        .into_iter()
        .map(|w| workload_text(w, &en))
        .collect();
    assert_eq!(distinct.len(), 7);
}

#[test]
fn describe_known_profile_problems_map_to_translated_sentences() {
    // C#: BenchmarkRowTextTests.Describe_KnownProfileProblems_MapToTranslatedSentences
    // (the `Describe(null)` assertion is N/A: there is no null in Rust)
    let en = english();
    let known = first_profile();

    let not_implemented_known = RowError::Profile(BenchmarkProfileError::new(
        BenchmarkProfileProblem::NotImplemented,
        known.id.clone(),
        "english",
    ));
    assert_eq!(
        describe_error(&not_implemented_known, &en),
        format!(
            "{} has no real check yet, so it was not run (a decode-only stand-in would give misleading results).",
            profile_name(&known, &en)
        )
    );

    let not_implemented_unknown = RowError::Profile(BenchmarkProfileError::new(
        BenchmarkProfileProblem::NotImplemented,
        "no-such-profile",
        "english",
    ));
    assert_eq!(
        describe_error(&not_implemented_unknown, &en),
        "no-such-profile has no real check yet, so it was not run (a decode-only stand-in would give misleading results)."
    );

    let missing = RowError::Profile(BenchmarkProfileError::new(
        BenchmarkProfileProblem::MissingIdOrMode,
        "",
        "english",
    ));
    assert_eq!(
        describe_error(&missing, &en),
        "The configuration has no id or loading mode."
    );

    let invalid = RowError::Profile(BenchmarkProfileError::new(
        BenchmarkProfileProblem::InvalidSettings,
        "p1",
        "english",
    ));
    assert_eq!(
        describe_error(&invalid, &en),
        "The settings of configuration 'p1' are invalid."
    );

    // C#: IOException("disk on fire") is the technical message of an other error.
    assert_eq!(
        describe_error(&RowError::Other("disk on fire".to_owned()), &en),
        "Unexpected error: disk on fire"
    );
}

#[test]
fn row_exposes_profile_and_phase_texts() {
    // C#: BenchmarkRowTextTests.Row_ExposesTheProfileAndPhaseWithTranslatedTexts
    // C#: BenchmarkTextTests.LibraryMessages_MapToCatalogText_OtherTextPassesThrough (lines 70-74: the row texts
    // `passed.StatusText` = `Đạt`, `WorkloadText`, `LoadingModeText`, `LoadingMode`)
    let en = english();
    let vi = vietnamese();
    let profile = first_profile();
    let row = BenchmarkResultRow::new(
        profile.clone(),
        phase(
            &profile.id,
            BenchmarkWorkload::Random,
            vec![10.0, 20.0, 30.0, 40.0],
            BenchmarkResultStatus::Pass,
            None,
        ),
        None,
    );

    assert_eq!(row.profile, profile);
    assert_eq!(row.profile_name(&en), profile_name(&profile, &en));
    assert_eq!(row.loading_mode(), profile.loading_mode);
    assert_eq!(row.loading_mode_text(&en), loading_mode_text(profile.loading_mode, &en));
    assert_eq!(row.workload(), BenchmarkWorkload::Random);
    assert_eq!(row.workload_text(&en), workload_text(BenchmarkWorkload::Random, &en));
    assert_eq!(row.count(), 4);
    assert_eq!(row.status(), BenchmarkResultStatus::Pass);
    assert_eq!(row.error, None);

    // The Vietnamese texts of a passing row (C# `Đạt`, the catalog value of enum.benchmarkResultStatus.pass).
    let passed = BenchmarkResultRow::new(
        registry("fast-sequential"),
        phase(
            "fast-sequential",
            BenchmarkWorkload::Sequential,
            vec![1.0],
            BenchmarkResultStatus::Pass,
            None,
        ),
        None,
    );
    assert_eq!(passed.status_text(&vi), "Đạt");
    assert_eq!(passed.workload_text(&vi), "Sequential");
    assert_eq!(passed.loading_mode_text(&vi), "Nhanh");
    assert_eq!(passed.loading_mode(), LoadingMode::Fast);
}

#[test]
fn row_best_marker_is_the_trophy_only_for_the_best_row() {
    // C#: BenchmarkRowTextTests.Row_BestMarker_IsTheTrophyOnlyForTheBestRow
    let profile = first_profile();
    let mut row = BenchmarkResultRow::new(
        profile.clone(),
        default_phase(&profile.id, BenchmarkResultStatus::Pass, None),
        None,
    );
    assert_eq!(row.best_marker(), "");
    row.is_best = true;
    assert_eq!(row.best_marker(), BEST_SYMBOL);
    assert!(!BEST_SYMBOL.is_empty());
}

#[test]
fn row_percentiles_are_whole_milliseconds_and_no_value_without_samples() {
    // C#: BenchmarkRowTextTests.Row_Percentiles_AreWholeMilliseconds_AndNoValueWithoutSamples
    let en = english();
    let profile = first_profile();
    let samples: Vec<f64> = (1..=100_i32).map(f64::from).collect();
    let full = phase(
        &profile.id,
        BenchmarkWorkload::Sequential,
        samples,
        BenchmarkResultStatus::Pass,
        None,
    );
    let row = BenchmarkResultRow::new(profile.clone(), full.clone(), None);

    assert_eq!(row.p50_text(&en), format!("{} ms", format_milliseconds_f0(full.p50())));
    assert_eq!(row.p95_text(&en), format!("{} ms", format_milliseconds_f0(full.p95())));
    assert_eq!(row.p99_text(&en), format!("{} ms", format_milliseconds_f0(full.p99())));
    assert_eq!(row.max_text(&en), "100 ms");
    let distinct: std::collections::HashSet<String> = [
        row.p50_text(&en),
        row.p95_text(&en),
        row.p99_text(&en),
        row.max_text(&en),
    ]
    .into_iter()
    .collect();
    assert_eq!(distinct.len(), 4, "each column shows its own statistic");

    let empty = BenchmarkResultRow::new(
        profile.clone(),
        phase(
            &profile.id,
            BenchmarkWorkload::Sequential,
            vec![],
            BenchmarkResultStatus::InsufficientData,
            None,
        ),
        None,
    );
    for text in [
        empty.p50_text(&en),
        empty.p95_text(&en),
        empty.p99_text(&en),
        empty.max_text(&en),
    ] {
        assert_eq!(text, "—");
    }
}

#[test]
fn row_status_text_maps_each_status() {
    // C#: BenchmarkRowTextTests.Row_StatusText_MapsEachStatus
    let en = english();
    let profile = first_profile();
    let status_text = |status| {
        BenchmarkResultRow::new(profile.clone(), default_phase(&profile.id, status, None), None).status_text(&en)
    };
    assert_eq!(status_text(BenchmarkResultStatus::Pass), "Pass");
    assert_eq!(status_text(BenchmarkResultStatus::Warn), "Warning");
    assert_eq!(
        status_text(BenchmarkResultStatus::InsufficientData),
        "Insufficient data"
    );
}

#[test]
fn row_status_text_for_a_failure_depends_on_message_and_error() {
    // C#: BenchmarkRowTextTests.Row_StatusText_ForAFailure_DependsOnTheMessageAndTheError
    // C#: BenchmarkTextTests.LibraryMessages_MapToCatalogText_OtherTextPassesThrough (lines 70-72: the correctness
    // failure row shows `benchmark.result.correctnessFailed`)
    let en = english();
    let profile = first_profile();
    let failure = |message: Option<&str>, error: Option<RowError>| {
        BenchmarkResultRow::new(
            profile.clone(),
            default_phase(&profile.id, BenchmarkResultStatus::Fail, message),
            error,
        )
        .status_text(&en)
    };

    // No message: plain Fail.
    assert_eq!(failure(None, None), "Error");
    // The engine's correctness message: the correctness text.
    assert_eq!(
        failure(Some(RESULT_CORRECTNESS_FAILED), None),
        "One or more samples failed correctness"
    );
    // A library message without an error is shown as given, inside the translated wrapper.
    assert_eq!(failure(Some("library said so"), None), "Failed: library said so");
    // With the error behind it, the translated description replaces the English message.
    let error = RowError::Other("disk on fire".to_owned());
    assert_eq!(
        failure(Some("english message"), Some(error)),
        "Failed: Unexpected error: disk on fire"
    );
}

#[test]
fn profile_item_shows_catalog_texts() {
    // C#: BenchmarkRowTextTests.ProfileItem_ShowsCatalogTexts_AndRaisesAnAllPropertiesChangeOnRefresh
    // (the PropertyChanged half is N/A: decision D-10h-2, the window re-reads the texts)
    let en = english();
    let profile = first_profile();
    let item = BenchmarkProfileItem::new(profile.clone());

    assert_eq!(item.profile, profile);
    assert_eq!(item.name(&en), profile_name(&profile, &en));
    assert_eq!(item.description(&en), profile_description(&profile, &en));
}

// ---------------------------------------------------------------------------------------------------------------------
// BenchmarkTextTests.cs
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn profile_key_camel_cases_the_id() {
    // C#: BenchmarkTextTests.ProfileKey_CamelCasesTheId (three [InlineData] rows)
    assert_eq!(
        profile_key("fast-sequential", "name"),
        "benchmark.profile.fastSequential.name"
    );
    assert_eq!(
        profile_key("preview-high-quality", "description"),
        "benchmark.profile.previewHighQuality.description"
    );
    assert_eq!(
        profile_key("ssd-throughput", "name"),
        "benchmark.profile.ssdThroughput.name"
    );
    assert_eq!(PROFILE_KEY_PREFIX, "benchmark.profile.");
}

#[test]
fn every_profile_has_catalog_keys_and_english_matches_library_text() {
    // C#: BenchmarkTextTests.EveryProfile_HasCatalogKeys_AndEnglishMatchesTheLibraryText
    let en = english();
    let all = profiles::all();
    assert_eq!(all.len(), 30, "registry size at baseline d92d6aae");
    for profile in all {
        let name_key = profile_key(&profile.id, "name");
        let description_key = profile_key(&profile.id, "description");
        assert!(en.contains(&name_key), "en.json is missing {name_key}");
        assert!(en.contains(&description_key), "en.json is missing {description_key}");
        assert_eq!(en.get(&name_key), profile.name, "{name_key}");
        assert_eq!(en.get(&description_key), profile.description, "{description_key}");
    }
}

#[test]
fn profile_texts_follow_the_current_language() {
    // C#: BenchmarkTextTests.ProfileTexts_FollowTheCurrentLanguage
    let vi = vietnamese();
    let en = english();
    let profile = registry("fast-sequential");

    assert_eq!(profile_description(&profile, &vi), "Liên tục chuyển sang ảnh tiếp theo");
    assert_eq!(profile_name(&profile, &vi), "Fast Sequential");
    assert_eq!(profile_description(&profile, &en), "Continuous Next");
}

#[test]
fn profile_without_catalog_key_falls_back_to_the_library_text() {
    // C#: BenchmarkTextTests.ProfileWithoutCatalogKey_FallsBackToTheLibraryText
    let en = english();
    let mut profile = first_profile();
    profile.id = "custom-probe".to_owned();
    profile.name = "Custom Probe".to_owned();
    profile.description = "Library text".to_owned();

    assert_eq!(profile_name(&profile, &en), "Custom Probe");
    assert_eq!(profile_description(&profile, &en), "Library text");
}

#[test]
fn library_messages_map_to_catalog_text_other_text_passes_through() {
    // C#: BenchmarkTextTests.LibraryMessages_MapToCatalogText_OtherTextPassesThrough (lines 61-68)
    let vi = vietnamese();
    assert_eq!(PROGRESS_OK, "OK");
    assert_eq!(progress_message(PROGRESS_OK, &vi), vi.get(KEY_PROGRESS_OK));
    assert_eq!(
        progress_message(PROGRESS_CORRECTNESS_FAILED, &vi),
        vi.get(KEY_PROGRESS_CORRECTNESS_FAILED)
    );
    assert_eq!(progress_message("boom", &vi), "boom");
}

#[test]
fn describe_maps_library_problems_to_catalog_text_in_both_languages() {
    // C#: DiagnosticsUnitsLeftoversTests.BenchmarkDescribe_MapsLibraryProblemsToCatalogText_OtherErrorsKeepTheirMessage
    let vi = vietnamese();
    let en = english();
    let not_implemented = RowError::Profile(BenchmarkProfileError::new(
        BenchmarkProfileProblem::NotImplemented,
        "cache-recovery",
        "English library text",
    ));

    let vi_text = describe_error(&not_implemented, &vi);
    assert!(vi_text.contains("chưa có bước kiểm tra thật"), "{vi_text}");
    assert!(!vi_text.contains("English library text"), "{vi_text}");
    assert_eq!(
        describe_error(&RowError::Other("boom".to_owned()), &vi),
        "Lỗi không mong muốn: boom"
    );

    let en_text = describe_error(&not_implemented, &en);
    assert!(en_text.contains("has no real check yet"), "{en_text}");
    assert_eq!(
        describe_error(&RowError::Other("boom".to_owned()), &en),
        "Unexpected error: boom"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// New F tests (no C# twin; written from the contract semantics)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn cell_texts_are_ten_cells_in_column_order() {
    // new: cell_texts_are_ten_cells_in_column_order (contract "Test plan"; golden row-best-pass)
    let en = english();
    let profile = registry("fast-sequential");
    let mut row = BenchmarkResultRow::new(
        profile.clone(),
        default_phase("fast-sequential", BenchmarkResultStatus::Pass, None),
        None,
    );
    row.is_best = true;

    let cells = row.cell_texts(&en);
    assert_eq!(cells.len(), 10);
    assert_eq!(
        cells,
        vec![
            "🏆",
            "Fast Sequential",
            "Fast",
            "Sequential",
            "4",
            "25 ms",
            "38 ms",
            "40 ms",
            "40 ms",
            "Pass",
        ]
    );
}

#[test]
fn format_milliseconds_f0_prints_non_finite_values_like_invariant_culture() {
    // new: format_milliseconds_f0_prints_non_finite_values_like_invariant_culture (trap T4)
    assert_eq!(format_milliseconds_f0(f64::NAN), "NaN");
    assert_eq!(format_milliseconds_f0(f64::INFINITY), "Infinity");
    assert_eq!(format_milliseconds_f0(f64::NEG_INFINITY), "-Infinity");
    assert_eq!(format_milliseconds_f0(-0.4), "-0");
    assert_eq!(format_milliseconds_f0(0.5), "0");
    assert_eq!(format_milliseconds_f0(1.5), "2");
    assert_eq!(format_milliseconds_f0(2.5), "2");
}

#[test]
fn status_text_fail_without_message_ignores_the_error() {
    // new: status_text_fail_without_message_ignores_the_error (golden row-fail-no-message-with-error-shows-plain-fail)
    let en = english();
    let profile = registry("preview-light");
    let row = BenchmarkResultRow::new(
        profile,
        phase(
            "preview-light",
            BenchmarkWorkload::Sequential,
            vec![5.0],
            BenchmarkResultStatus::Fail,
            None,
        ),
        Some(RowError::Other("boom".to_owned())),
    );
    assert_eq!(row.status_text(&en), "Error");
}

#[test]
fn profile_key_case_mapping_matches_the_golden() {
    // new: profile_key_case_mapping_matches_the_golden (traps T1 and T2; values from tests/golden/benchmark-presentation.json)
    assert_eq!(profile_key("a-ß", "name"), "benchmark.profile.aß.name");
    assert_eq!(profile_key("a-ÿ", "name"), "benchmark.profile.aŸ.name");
    assert_eq!(profile_key("a-été", "name"), "benchmark.profile.aÉté.name");
    assert_eq!(profile_key("a-ſ", "name"), "benchmark.profile.aS.name");
    // The golden keeps the dotless i (`key-dotless-i`), which Rust `char::to_uppercase` would turn into `I`:
    // the porter must add an exception (contract open question 1).
    assert_eq!(profile_key("a-ı", "name"), "benchmark.profile.aı.name");
    assert_eq!(
        profile_key("a-\u{1F600}x", "name"),
        "benchmark.profile.a\u{1F600}x.name"
    );
}

#[test]
fn best_marker_empty_when_not_best() {
    // new: best_marker_empty_when_not_best
    let profile = first_profile();
    let row = BenchmarkResultRow::new(
        profile.clone(),
        default_phase(&profile.id, BenchmarkResultStatus::Warn, Some("ignored")),
        None,
    );
    assert_eq!(row.best_marker(), "");
    assert!(!row.is_best);
}

#[test]
fn milliseconds_text_uses_no_value_without_samples_and_the_unit_otherwise() {
    // new: contract semantics rule 14 (count == 0 ignores the value; otherwise unit.milliseconds with F0 value)
    let en = english();
    assert_eq!(milliseconds_text(42.0, 0, &en), "—");
    assert_eq!(milliseconds_text(12.6, 1, &en), "13 ms");
}

#[test]
fn progress_message_passes_unknown_text_through_unchanged() {
    // new: contract semantics rule 7 (exact, case-sensitive match; empty string passes through)
    let en = english();
    assert_eq!(progress_message("ok", &en), "ok");
    assert_eq!(progress_message("", &en), "");
}
