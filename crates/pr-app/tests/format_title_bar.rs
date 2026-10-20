//! Port of `PhotoReview.App.Shared/ViewModels/TitleBarFormatter.cs` tests: `tests/PhotoReview.App.Tests/ViewModels/TitleBarFormatterTests.cs`
//! (C# baseline d92d6aae), plus the golden checks over `tests/golden/format-title-bar.json` (oracle `oracle/W8-02b.cs`,
//! command `format-title-bar`).
//!
//! Unit W8-02b, contract `contracts/W8-02b.md`. Red stage: every stub body is `todo!()`, so each test here fails until the
//! porter implements the formatter. `TestLocalization.Use(English)` becomes an explicit English `Localizer`; C#
//! `ToLocalTime().ToString("g")` becomes the recorded local wall clock (contract Semantics 9).

mod format_support;

use format_support::{
    Env, date, english, env_for_case, exif_of, fixture_dates, i32_of, invariant_numbers, opt_i32, opt_i64, opt_text,
    text_of,
};
use pr_app::viewmodels::format::exif::SEPARATOR;
use pr_app::viewmodels::format::status::format_file_size;
use pr_app::viewmodels::format::title_bar::{TitleBarLine, format_title_bar};
use pr_core::model::TitleBarFields;
use pr_imaging::exif_summary::{ExifRational, ExifSummary};
use pr_test_support::golden;

const FOLDER: &str = "C:\\Photos\\Vacation 2024";
const FOLDER_NAME: &str = "Vacation 2024";

/// The English environment with the modified time of the C# fixture (2024-06-10 08:00, "g" = "06/10/2024 08:00").
fn env() -> Env {
    Env::new(
        english(),
        invariant_numbers(),
        ",",
        fixture_dates(&[("2024-06-10 08:00:00", "06/10/2024 08:00")]),
    )
}

/// The C# `TitleBarFormatterTests.Exif` fixture.
fn fixture_exif() -> ExifSummary {
    ExifSummary {
        date_taken: Some(date("2024-05-01 14:03:22")),
        camera_make: Some("Canon".to_owned()),
        camera_model: Some("Canon EOS R5".to_owned()),
        lens_model: Some("RF24-70mm F2.8".to_owned()),
        iso: Some(400),
        focal_length: Some(ExifRational::new(50, 1)),
        f_number: Some(ExifRational::new(28, 10)),
        exposure_time: Some(ExifRational::new(1, 250)),
    }
}

/// `TitleBarFormatter.Format(fields, Folder, 11, 340, "IMG_1234.jpg", 2500000, 6000, 4000, modified, Exif)`.
fn full_line<'a>(exif: Option<&'a ExifSummary>) -> TitleBarLine<'a> {
    TitleBarLine {
        folder_path: FOLDER,
        index: Some(11),
        count: Some(340),
        file_name: Some("IMG_1234.jpg"),
        length: Some(2_500_000),
        width: 6000,
        height: 4000,
        modified_local: Some(date("2024-06-10 08:00:00")),
        exif,
    }
}

#[test]
fn title_bar_default_is_folder_name_only() {
    // C#: TitleBarFormatterTests.Default_IsFolderNameOnly
    let exif = fixture_exif();
    let env = env();
    assert_eq!(
        format_title_bar(TitleBarFields::DEFAULT, &full_line(Some(&exif)), &env.ctx()),
        FOLDER_NAME
    );
}

#[test]
fn title_bar_folder_path_shows_full_path() {
    // C#: TitleBarFormatterTests.FolderPath_ShowsFullPath
    let exif = fixture_exif();
    let env = env();
    assert_eq!(
        format_title_bar(TitleBarFields::FOLDER_PATH, &full_line(Some(&exif)), &env.ctx()),
        FOLDER
    );
}

#[test]
fn title_bar_all_fields_render_in_order() {
    // C#: TitleBarFormatterTests.AllFields_RenderInOrder (English catalog; the golden all-vi row covers Vietnamese)
    let exif = fixture_exif();
    let env = env();
    let expected = [
        FOLDER_NAME,
        FOLDER,
        "12/340",
        "IMG_1234.jpg",
        "2.38 MB",
        "6000×4000",
        "Modified: 06/10/2024 08:00",
        "Taken: 05/01/2024 14:03",
        "Canon EOS R5",
        "RF24-70mm F2.8",
        "ISO 400",
        "50 mm",
        "f/2.8",
        "1/250 s",
    ]
    .join(SEPARATOR);
    assert_eq!(
        format_title_bar(TitleBarFields::ALL, &full_line(Some(&exif)), &env.ctx()),
        expected
    );
}

#[test]
fn title_bar_index_count_is_one_based() {
    // C#: TitleBarFormatterTests.IndexCount_Is1BasedPositionOverCount
    let env = env();
    let line = TitleBarLine {
        index: Some(0),
        count: Some(5),
        ..blank_line()
    };
    assert_eq!(format_title_bar(TitleBarFields::INDEX_COUNT, &line, &env.ctx()), "1/5");
}

/// A line with no folder-part values: the folder path set, everything else absent.
fn blank_line() -> TitleBarLine<'static> {
    TitleBarLine {
        folder_path: FOLDER,
        index: None,
        count: None,
        file_name: None,
        length: None,
        width: 0,
        height: 0,
        modified_local: None,
        exif: None,
    }
}

#[test]
fn title_bar_missing_values_are_skipped() {
    // C#: TitleBarFormatterTests.MissingValues_AreSkipped (7 InlineData rows)
    let env = env();
    let rows = [
        TitleBarFields::INDEX_COUNT,
        TitleBarFields::FILE_NAME,
        TitleBarFields::FILE_SIZE,
        TitleBarFields::DIMENSIONS,
        TitleBarFields::MODIFIED_DATE,
        TitleBarFields::DATE_TAKEN,
        TitleBarFields::CAMERA,
    ];
    for field in rows {
        let text = format_title_bar(TitleBarFields::FOLDER_NAME | field, &blank_line(), &env.ctx());
        assert_eq!(text, FOLDER_NAME, "field {:#x}", field.bits());
    }
}

#[test]
fn title_bar_drive_root_folder_name_shows_root() {
    // C#: TitleBarFormatterTests.DriveRoot_FolderName_ShowsRoot
    let env = env();
    let line = TitleBarLine {
        folder_path: "D:\\",
        index: Some(0),
        count: Some(40),
        file_name: Some("IMG_1.jpg"),
        ..blank_line()
    };
    let fields = TitleBarFields::FOLDER_NAME | TitleBarFields::INDEX_COUNT | TitleBarFields::FILE_NAME;
    assert_eq!(format_title_bar(fields, &line, &env.ctx()), "D:\\ · 1/40 · IMG_1.jpg");
}

#[test]
fn title_bar_no_fields_produces_empty() {
    // C#: TitleBarFormatterTests.NoFieldsSelected_ProducesEmpty
    let exif = fixture_exif();
    let env = env();
    assert_eq!(
        format_title_bar(TitleBarFields::NONE, &full_line(Some(&exif)), &env.ctx()),
        ""
    );
}

#[test]
fn title_bar_file_size_uses_status_units() {
    // C#: TitleBarFormatterTests.FileSize_UsesSharedUnitFormatting
    let env = env();
    let text = format_title_bar(TitleBarFields::FILE_SIZE, &full_line(None), &env.ctx());
    assert_eq!(text, "2.38 MB");
    assert_eq!(text, format_file_size(2_500_000, &env.ctx()));
}

#[test]
fn title_bar_modified_date_formats_like_date_taken() {
    // C#: TitleBarFormatterTests.ModifiedDate_IsLocalTimeFormattedLikeDateTaken
    let env = env();
    assert_eq!(
        format_title_bar(TitleBarFields::MODIFIED_DATE, &full_line(None), &env.ctx()),
        "Modified: 06/10/2024 08:00"
    );
}

#[test]
fn title_bar_camera_reuses_camera_text() {
    // C#: TitleBarFormatterTests.Camera_ReusesExifFormatterCameraText
    let exif = fixture_exif();
    let env = env();
    assert_eq!(
        format_title_bar(TitleBarFields::CAMERA, &full_line(Some(&exif)), &env.ctx()),
        "Canon EOS R5"
    );
}

#[test]
fn title_bar_turning_one_field_off_removes_exactly_that_part() {
    // C#: TitleBarFormatterTests.TurningOneFieldOff_RemovesExactlyThatPart
    let exif = fixture_exif();
    let env = env();
    let all = format_title_bar(TitleBarFields::ALL, &full_line(Some(&exif)), &env.ctx());
    let without = format_title_bar(
        TitleBarFields::ALL.difference(TitleBarFields::MODIFIED_DATE),
        &full_line(Some(&exif)),
        &env.ctx(),
    );
    assert!(all.contains("06/10/2024 08:00"), "{all}");
    assert!(!without.contains("06/10/2024 08:00"), "{without}");
    assert_eq!(all.split(SEPARATOR).count() - 1, without.split(SEPARATOR).count());
}

// ---- Golden checks over tests/golden/format-title-bar.json ----

/// One golden row through `format_title_bar`.
fn run_title_bar_case(case: &serde_json::Value) -> String {
    let exif = exif_of(&case["exif"]);
    let file_name = opt_text(case, "file_name");
    let modified_local = opt_text(case, "modified_local").map(|text| date(&text));
    let folder = text_of(case, "folder_path");
    let line = TitleBarLine {
        folder_path: &folder,
        index: opt_i32(case, "index"),
        count: opt_i32(case, "count"),
        file_name: file_name.as_deref(),
        length: opt_i64(case, "length"),
        width: i32_of(case, "width"),
        height: i32_of(case, "height"),
        modified_local,
        exif: exif.as_ref(),
    };
    let fields = TitleBarFields::from_bits_truncate(golden_fields(case));
    let env = env_for_case(case);
    format_title_bar(fields, &line, &env.ctx())
}

fn golden_fields(case: &serde_json::Value) -> u32 {
    let bits = golden::i64_of(case, "fields");
    u32::try_from(bits).unwrap_or_else(|_| panic!("fields out of range: {bits}"))
}

#[test]
fn golden_format_title_bar() {
    // new: golden-driven, every case of tests/golden/format-title-bar.json (oracle format-title-bar, C# TitleBarFormatter); collects all differences
    let doc = golden::load("format-title-bar");
    let mut mismatches = Vec::new();
    for case in &doc.cases {
        let expected = text_of(case, "text");
        let actual = run_title_bar_case(case);
        if actual != expected {
            mismatches.push(format!(
                "{}: expected {expected:?}, got {actual:?}",
                golden::str_of(case, "name")
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} format-title-bar cases differ from the C# oracle:\n{}",
        mismatches.len(),
        doc.cases.len(),
        mismatches.join("\n")
    );
}
