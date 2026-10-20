//! Port of `PhotoReview.App.Shared/ViewModels/ExifFormatter.cs` tests: `tests/PhotoReview.App.Tests/ViewModels/ExifFormatterTests.cs`
//! and `ExifFormatterEdgeTests.cs` (C# baseline d92d6aae), plus the golden checks over `tests/golden/format-exif.json`
//! (inputs echoed per case; oracle `oracle/W8-02b.cs`, command `format-exif`).
//!
//! Unit W8-02b, contract `contracts/W8-02b.md`. Red stage: every stub body is `todo!()`, so each test here fails until the
//! porter implements the formatter. `TestLocalization.Use(...)` becomes an explicit `Localizer` argument; C# `DateTime.ToString("g")`
//! becomes a recorded `DateText` (contract Q3); the C# all-cultures loop is not portable (see the fake-culture test below).
//!
//! Documented skip: `EveryCulture_FormatsEveryPlausibleDate_WithoutThrowing` loops over every .NET culture (needs the Windows
//! culture table); it is ported as a fake-culture test with the same no-throw and fallback contract.

mod format_support;

use format_support::{
    Env, RecordedDates, date, en_env, english, exif_of, fixture_dates, i32_of, invariant_numbers, opt_text,
    rational_of, vi_env, vi_numbers, vietnamese,
};
use pr_app::viewmodels::format::context::{DateText, FormatContext};
use pr_app::viewmodels::format::exif::{ExifLine, SEPARATOR, camera_text, format_date_time, format_exif, shutter_text};
use pr_core::model::ExifInfoFields;
use pr_core::time::DateTimeUtc;
use pr_imaging::exif_summary::{ExifRational, ExifSummary};
use pr_test_support::golden;

/// The EXIF summary of the C# `ExifFormatterTests.Exif` fixture (DateTaken 2024-05-01 14:03:22).
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

/// A line with the file name, the pixel size and an EXIF summary; no modified time, no RAW preview.
fn line<'a>(file: Option<&'a str>, width: i32, height: i32, exif: Option<&'a ExifSummary>) -> ExifLine<'a> {
    ExifLine {
        file_name: file,
        width,
        height,
        exif,
        modified_local: None,
        raw_preview_width: 0,
        raw_preview_height: 0,
    }
}

/// `ExifFormatter.Format(fields, "IMG_1234.jpg", 6000, 4000, Exif, provider: Invariant)`.
fn render_fixture(env: &Env, fields: ExifInfoFields) -> String {
    let exif = fixture_exif();
    format_exif(fields, &line(Some("IMG_1234.jpg"), 6000, 4000, Some(&exif)), &env.ctx())
}

// ---- ExifFormatterTests.cs ----

#[test]
fn format_exif_all_fields_english() {
    // C#: ExifFormatterTests.AllFieldsEnglish
    let env = en_env();
    assert_eq!(
        render_fixture(&env, ExifInfoFields::ALL),
        "IMG_1234.jpg · Taken: 05/01/2024 14:03 · 6000×4000 · Canon EOS R5 · RF24-70mm F2.8 · ISO 400 · 50 mm · f/2.8 · 1/250 s"
    );
}

#[test]
fn format_exif_raw_preview_follows_dimensions_and_is_absent_otherwise() {
    // C#: ExifFormatterTests.RawPreviewSize_FollowsDimensions_AndIsAbsentOtherwise (Q-RAW-03)
    let env = en_env();
    let exif = fixture_exif();
    let plain = render_fixture(&env, ExifInfoFields::ALL);

    let with_preview = format_exif(
        ExifInfoFields::ALL,
        &ExifLine {
            raw_preview_width: 1620,
            raw_preview_height: 1080,
            ..line(Some("IMG_1234.jpg"), 6000, 4000, Some(&exif))
        },
        &env.ctx(),
    );
    assert_eq!(
        with_preview,
        plain.replace("6000×4000", "6000×4000 · RAW preview 1620×1080")
    );

    let zero_preview = format_exif(
        ExifInfoFields::ALL,
        &ExifLine {
            raw_preview_width: 0,
            raw_preview_height: 0,
            ..line(Some("IMG_1234.jpg"), 6000, 4000, Some(&exif))
        },
        &env.ctx(),
    );
    assert_eq!(zero_preview, plain);

    let height_zero = format_exif(
        ExifInfoFields::ALL,
        &ExifLine {
            raw_preview_width: 1620,
            raw_preview_height: 0,
            ..line(Some("IMG_1234.jpg"), 6000, 4000, Some(&exif))
        },
        &env.ctx(),
    );
    assert_eq!(height_zero, plain);

    // Part of the dimensions field: switched off with it.
    let file_only = format_exif(
        ExifInfoFields::FILE_NAME,
        &ExifLine {
            raw_preview_width: 1620,
            raw_preview_height: 1080,
            ..line(Some("IMG_1234.jpg"), 6000, 4000, Some(&exif))
        },
        &env.ctx(),
    );
    assert_eq!(file_only, "IMG_1234.jpg");
}

#[test]
fn format_exif_raw_preview_is_localized_vietnamese() {
    // C#: ExifFormatterTests.RawPreviewSize_IsLocalized
    let env = vi_env();
    let text = format_exif(
        ExifInfoFields::DIMENSIONS,
        &ExifLine {
            raw_preview_width: 1620,
            raw_preview_height: 1080,
            ..line(None, 6000, 4000, None)
        },
        &env.ctx(),
    );
    assert_eq!(text, "6000×4000 · Bản xem trước RAW 1620×1080");
}

#[test]
fn format_exif_units_are_localized_vietnamese() {
    // C#: ExifFormatterTests.UnitsAreLocalized
    let env = vi_env();
    let text = render_fixture(&env, ExifInfoFields::ALL);
    assert!(text.ends_with("ISO 400 · 50 mm · f/2.8 · 1/250 giây"), "{text}");
}

#[test]
fn format_exif_each_field_alone() {
    // C#: ExifFormatterTests.EachFieldAlone (11 InlineData rows)
    let env = en_env();
    let rows: [(ExifInfoFields, &str); 11] = [
        (ExifInfoFields::FILE_NAME, "IMG_1234.jpg"),
        (ExifInfoFields::DATE_TAKEN, "Taken: 05/01/2024 14:03"),
        (ExifInfoFields::DIMENSIONS, "6000×4000"),
        (ExifInfoFields::CAMERA, "Canon EOS R5"),
        (ExifInfoFields::LENS, "RF24-70mm F2.8"),
        (ExifInfoFields::ISO, "ISO 400"),
        (ExifInfoFields::FOCAL_LENGTH, "50 mm"),
        (ExifInfoFields::APERTURE, "f/2.8"),
        (ExifInfoFields::SHUTTER_SPEED, "1/250 s"),
        (
            ExifInfoFields::ISO
                | ExifInfoFields::FOCAL_LENGTH
                | ExifInfoFields::APERTURE
                | ExifInfoFields::SHUTTER_SPEED,
            "ISO 400 · 50 mm · f/2.8 · 1/250 s",
        ),
        (ExifInfoFields::NONE, ""),
    ];
    for (fields, expected) in rows {
        assert_eq!(render_fixture(&env, fields), expected, "fields {:#x}", fields.bits());
    }
}

#[test]
fn format_exif_each_field_off_removes_exactly_that_part() {
    // C#: ExifFormatterTests.EachFieldOff (9 InlineData rows)
    let env = en_env();
    let rows: [(ExifInfoFields, &str); 9] = [
        (ExifInfoFields::FILE_NAME, "IMG_1234.jpg"),
        (ExifInfoFields::DATE_TAKEN, "Taken: 05/01/2024 14:03"),
        (ExifInfoFields::DIMENSIONS, "6000×4000"),
        (ExifInfoFields::CAMERA, "Canon EOS R5"),
        (ExifInfoFields::LENS, "RF24-70mm F2.8"),
        (ExifInfoFields::ISO, "ISO 400"),
        (ExifInfoFields::FOCAL_LENGTH, "50 mm"),
        (ExifInfoFields::APERTURE, "f/2.8"),
        (ExifInfoFields::SHUTTER_SPEED, "1/250 s"),
    ];
    for (field, removed_part) in rows {
        let all = render_fixture(&env, ExifInfoFields::ALL);
        let without = render_fixture(&env, ExifInfoFields::ALL.difference(field));
        assert!(all.contains(removed_part), "{removed_part}: {all}");
        assert!(!without.contains(removed_part), "{removed_part}: {without}");
        assert_eq!(
            all.split(SEPARATOR).count() - 1,
            without.split(SEPARATOR).count(),
            "{removed_part}"
        );
    }
}

#[test]
fn exif_info_fields_are_separate_flags_and_all_is_their_union() {
    // C#: ExifFormatterTests.FlagsAreSeparate
    let fields = [
        ExifInfoFields::FILE_NAME,
        ExifInfoFields::DATE_TAKEN,
        ExifInfoFields::MODIFIED_DATE,
        ExifInfoFields::DIMENSIONS,
        ExifInfoFields::CAMERA,
        ExifInfoFields::LENS,
        ExifInfoFields::ISO,
        ExifInfoFields::FOCAL_LENGTH,
        ExifInfoFields::APERTURE,
        ExifInfoFields::SHUTTER_SPEED,
    ];
    let mut union = ExifInfoFields::NONE;
    for field in fields {
        assert_eq!(field.bits().count_ones(), 1, "{field:?} is not a single flag");
        assert_eq!(union.bits() & field.bits(), 0, "{field:?} overlaps the earlier flags");
        union |= field;
    }
    assert_eq!(union, ExifInfoFields::ALL);

    let env = en_env();
    assert_eq!(render_fixture(&env, ExifInfoFields::ALL).split(SEPARATOR).count(), 9);
}

#[test]
fn format_exif_missing_values_are_skipped() {
    // C#: ExifFormatterTests.MissingValuesAreSkipped
    let env = en_env();
    let no_exif = format_exif(
        ExifInfoFields::ALL,
        &line(Some("IMG_1234.jpg"), 6000, 4000, None),
        &env.ctx(),
    );
    assert_eq!(no_exif, "IMG_1234.jpg · 6000×4000");

    let nothing = format_exif(ExifInfoFields::ALL, &line(Some("IMG_1234.jpg"), 0, 0, None), &env.ctx());
    assert_eq!(nothing, "IMG_1234.jpg");

    let partial = ExifSummary {
        iso: Some(100),
        f_number: Some(ExifRational::new(8, 1)),
        ..Default::default()
    };
    let text = format_exif(
        ExifInfoFields::ALL,
        &line(Some("IMG_1234.jpg"), 0, 0, Some(&partial)),
        &env.ctx(),
    );
    assert_eq!(text, "IMG_1234.jpg · ISO 100 · f/8");
}

#[test]
fn shutter_text_fractions_below_half_second_and_seconds_above() {
    // C#: ExifFormatterTests.ShutterSpeed (9 InlineData rows)
    let env = en_env();
    let rows: [(u32, u32, &str); 9] = [
        (1, 250, "1/250 s"),
        (10, 2500, "1/250 s"),
        (1, 3, "1/3 s"),
        (3, 10, "0.3 s"),
        (4, 10, "0.4 s"),
        (1, 2, "1/2 s"),
        (5, 10, "0.5 s"),
        (13, 10, "1.3 s"),
        (30, 1, "30 s"),
    ];
    for (numerator, denominator, expected) in rows {
        let text = shutter_text(Some(ExifRational::new(numerator, denominator)), &env.ctx());
        assert_eq!(text.as_deref(), Some(expected), "{numerator}/{denominator}");
    }
}

#[test]
fn shutter_text_tenth_bound_is_inclusive_at_point_one() {
    // new: trap T2 (`seconds >= 0.1` lower bound). 0.105 s is above 0.1, so it takes the decimal-seconds branch
    // ("0.11 s" after half-away rounding of the decimals), not the fraction "1/10 s". Kills a bound drift to 0.11.
    let env = en_env();
    let text = shutter_text(Some(ExifRational::new(105, 1000)), &env.ctx());
    assert_eq!(text.as_deref(), Some("0.11 s"));
}

#[test]
fn camera_text_examples() {
    // C#: ExifFormatterTests.CameraText (6 InlineData rows)
    let rows: [(Option<&str>, Option<&str>, Option<&str>); 6] = [
        (Some("Canon"), Some("Canon EOS R5"), Some("Canon EOS R5")),
        (Some("NIKON CORPORATION"), Some("NIKON Z 6_2"), Some("NIKON Z 6_2")),
        (Some("SONY"), Some("ILCE-7M3"), Some("SONY ILCE-7M3")),
        (None, Some("X100V"), Some("X100V")),
        (Some("FUJIFILM"), None, Some("FUJIFILM")),
        (None, None, None),
    ];
    for (make, model, expected) in rows {
        assert_eq!(
            camera_text(make, model).as_deref(),
            expected,
            "make {make:?}, model {model:?}"
        );
    }
}

#[test]
fn format_exif_numbers_use_display_culture() {
    // C#: ExifFormatterTests.DisplayCultureNumbers (Vietnamese decimal comma, English texts)
    let env = Env::new(english(), vi_numbers(), ",", fixture_dates(&[]));
    let exif = ExifSummary {
        focal_length: Some(ExifRational::new(185, 10)),
        f_number: Some(ExifRational::new(28, 10)),
        ..Default::default()
    };
    let text = format_exif(ExifInfoFields::ALL, &line(None, 0, 0, Some(&exif)), &env.ctx());
    assert_eq!(text, "18,5 mm · f/2,8");
}

#[test]
fn exif_info_fields_modified_date_off_by_default() {
    // C#: ExifFormatterTests.ModifiedDate_OffByDefault
    // Note: this checks pr-core constants only, so it passes before the port (documented in the unit report).
    assert!(!ExifInfoFields::DEFAULT.contains(ExifInfoFields::MODIFIED_DATE));
    assert!(ExifInfoFields::ALL.contains(ExifInfoFields::MODIFIED_DATE));
}

#[test]
fn format_exif_modified_date_renders_after_date_taken() {
    // C#: ExifFormatterTests.ModifiedDate_RendersAfterDateTakenInLocalTime
    // The caller converts to local time (contract Semantics 9): the wall clock below is the local value.
    let env = Env::new(
        english(),
        invariant_numbers(),
        ",",
        fixture_dates(&[("2024-06-15 09:30:00", "06/15/2024 09:30")]),
    );
    let exif = fixture_exif();
    let text = format_exif(
        ExifInfoFields::DATE_TAKEN | ExifInfoFields::MODIFIED_DATE,
        &ExifLine {
            modified_local: Some(date("2024-06-15 09:30:00")),
            ..line(Some("a.jpg"), 0, 0, Some(&exif))
        },
        &env.ctx(),
    );
    assert_eq!(text, "Taken: 05/01/2024 14:03 · Modified: 06/15/2024 09:30");
}

#[test]
fn format_exif_modified_date_alone() {
    // C#: ExifFormatterTests.ModifiedDate_Alone
    let env = Env::new(
        english(),
        invariant_numbers(),
        ",",
        fixture_dates(&[("2024-01-02 03:04:05", "01/02/2024 03:04")]),
    );
    let text = format_exif(
        ExifInfoFields::MODIFIED_DATE,
        &ExifLine {
            modified_local: Some(date("2024-01-02 03:04:05")),
            ..line(Some("a.jpg"), 0, 0, None)
        },
        &env.ctx(),
    );
    assert_eq!(text, "Modified: 01/02/2024 03:04");
}

#[test]
fn format_exif_modified_date_null_is_skipped() {
    // C#: ExifFormatterTests.ModifiedDate_NullIsSkipped (not "?")
    let env = en_env();
    let text = format_exif(
        ExifInfoFields::FILE_NAME | ExifInfoFields::MODIFIED_DATE,
        &line(Some("a.jpg"), 0, 0, None),
        &env.ctx(),
    );
    assert_eq!(text, "a.jpg");
}

#[test]
fn format_exif_modified_date_off_removes_exactly_that_part() {
    // C#: ExifFormatterTests.ModifiedDate_TurningOffRemovesExactlyThatPart
    let env = Env::new(
        english(),
        invariant_numbers(),
        ",",
        fixture_dates(&[("2024-03-04 05:06:07", "03/04/2024 05:06")]),
    );
    let exif = fixture_exif();
    let modified = Some(date("2024-03-04 05:06:07"));
    let fields = ExifInfoFields::FILE_NAME | ExifInfoFields::MODIFIED_DATE | ExifInfoFields::CAMERA;

    let with_modified = format_exif(
        fields,
        &ExifLine {
            modified_local: modified,
            ..line(Some("a.jpg"), 0, 0, Some(&exif))
        },
        &env.ctx(),
    );
    let without_modified = format_exif(
        fields.difference(ExifInfoFields::MODIFIED_DATE),
        &ExifLine {
            modified_local: modified,
            ..line(Some("a.jpg"), 0, 0, Some(&exif))
        },
        &env.ctx(),
    );
    assert_eq!(
        with_modified,
        format!("a.jpg{SEPARATOR}Modified: 03/04/2024 05:06{SEPARATOR}Canon EOS R5")
    );
    assert_eq!(without_modified, format!("a.jpg{SEPARATOR}Canon EOS R5"));
}

// ---- ExifFormatterEdgeTests.cs ----

/// Both UI languages, invariant numbers, no recorded dates (the edge rows never show a date unless the test says so).
fn both_languages() -> [(&'static str, Env); 2] {
    [
        (
            "en",
            Env::new(english(), invariant_numbers(), ",", RecordedDates::none()),
        ),
        (
            "vi",
            Env::new(vietnamese(), invariant_numbers(), ",", RecordedDates::none()),
        ),
    ]
}

#[test]
fn format_exif_null_exif_and_no_fields_produce_empty_text() {
    // C#: ExifFormatterEdgeTests.NullExifAndNoFields_ProduceEmptyText_NotPlaceholders (2 languages)
    let iso_only = ExifSummary {
        iso: Some(100),
        ..Default::default()
    };
    let empty = ExifSummary::default();
    for (code, env) in both_languages() {
        let ctx = env.ctx();
        assert_eq!(
            format_exif(
                ExifInfoFields::NONE,
                &line(Some("a.jpg"), 10, 10, Some(&iso_only)),
                &ctx
            ),
            "",
            "{code}"
        );
        assert_eq!(
            format_exif(ExifInfoFields::ALL, &line(None, 0, 0, None), &ctx),
            "",
            "{code}"
        );
        assert_eq!(
            format_exif(ExifInfoFields::ALL, &line(Some("   "), -1, 5, Some(&empty)), &ctx),
            "",
            "{code}"
        );
        assert_eq!(
            format_exif(ExifInfoFields::ALL, &line(Some("a.jpg"), 0, 0, None), &ctx),
            "a.jpg",
            "{code}"
        );
    }
}

#[test]
fn format_exif_huge_values_do_not_panic() {
    // C#: ExifFormatterEdgeTests.HugeAndZeroValues_AreHandledWithoutThrowing (2 languages)
    let huge = ExifSummary {
        iso: Some(i32::MAX),
        focal_length: Some(ExifRational::new(u32::MAX, 1)),
        f_number: Some(ExifRational::new(1, u32::MAX)),
        exposure_time: Some(ExifRational::new(u32::MAX, 1)),
        ..Default::default()
    };
    for (code, env) in both_languages() {
        let text = format_exif(
            ExifInfoFields::ALL,
            &line(Some("x.jpg"), i32::MAX, i32::MAX, Some(&huge)),
            &env.ctx(),
        );
        assert!(text.contains("2147483647×2147483647"), "{code}: {text}");
        assert!(text.contains("2147483647"), "{code}: {text}");
        assert!(!text.contains("NaN"), "{code}: {text}");
        assert!(!text.contains('∞'), "{code}: {text}");
    }
}

#[test]
fn format_exif_zero_or_negative_values_are_skipped() {
    // C#: ExifFormatterEdgeTests.ZeroOrNegativeValues_AreSkipped (2 languages)
    let zeros = ExifSummary {
        iso: Some(0),
        focal_length: Some(ExifRational::new(50, 0)),
        f_number: Some(ExifRational::new(0, 10)),
        exposure_time: Some(ExifRational::new(0, 0)),
        ..Default::default()
    };
    let negative_iso = ExifSummary {
        iso: Some(-100),
        ..Default::default()
    };
    let shown =
        ExifInfoFields::ISO | ExifInfoFields::FOCAL_LENGTH | ExifInfoFields::APERTURE | ExifInfoFields::SHUTTER_SPEED;
    for (code, env) in both_languages() {
        let ctx = env.ctx();
        assert_eq!(
            format_exif(shown, &line(Some("x.jpg"), 1, 1, Some(&zeros)), &ctx),
            "",
            "{code}"
        );
        assert_eq!(
            format_exif(
                ExifInfoFields::ISO,
                &line(Some("x.jpg"), 1, 1, Some(&negative_iso)),
                &ctx
            ),
            "",
            "{code}"
        );
    }
}

#[test]
fn camera_text_unicode_and_rtl_names_pass_through() {
    // C#: ExifFormatterEdgeTests.UnicodeAndRightToLeftCameraNames_PassThroughVerbatim (4 InlineData rows)
    let rows: [(&str, &str, &str); 4] = [
        ("東京カメラ", "東京 α7", "東京カメラ 東京 α7"),
        ("ARABIC", "كاميرا رقمية", "ARABIC كاميرا رقمية"),
        ("שלום", "מצלמה", "שלום מצלמה"),
        ("Ünïcödé Ltd", "Ünïcödé Ltd Ω-1 😀", "Ünïcödé Ltd Ω-1 😀"),
    ];
    for (make, model, expected) in rows {
        assert_eq!(
            camera_text(Some(make), Some(model)).as_deref(),
            Some(expected),
            "make {make:?}"
        );
    }
}

#[test]
fn format_exif_rtl_file_name_stays_inside_the_separated_line() {
    // C#: ExifFormatterEdgeTests.RightToLeftFileName_IsKeptInsideTheSeparatedLine (U+200F after the name)
    let env = en_env();
    let text = format_exif(
        ExifInfoFields::FILE_NAME | ExifInfoFields::DIMENSIONS,
        &line(Some("صورة\u{200F}.jpg"), 640, 480, None),
        &env.ctx(),
    );
    assert_eq!(text, format!("صورة\u{200F}.jpg{SEPARATOR}640×480"));
}

/// A culture whose calendar rejects every date (C# `ArgumentOutOfRangeException` from `ToString("g")`).
#[derive(Debug)]
struct RejectingCulture;

impl DateText for RejectingCulture {
    fn general(&self, _wall_clock: DateTimeUtc) -> Option<String> {
        None
    }

    fn invariant_general(&self, wall_clock: DateTimeUtc) -> String {
        format!("inv {}", wall_clock.ticks())
    }
}

/// A culture that formats every date.
#[derive(Debug)]
struct AcceptingCulture;

impl DateText for AcceptingCulture {
    fn general(&self, _wall_clock: DateTimeUtc) -> Option<String> {
        Some("culture text".to_owned())
    }

    fn invariant_general(&self, _wall_clock: DateTimeUtc) -> String {
        "invariant text".to_owned()
    }
}

#[test]
fn format_date_time_falls_back_when_culture_rejects_date() {
    // C#: ExifFormatterEdgeTests.EveryCulture_FormatsEveryPlausibleDate_WithoutThrowing (documented skip: the all-cultures
    // loop needs the Windows culture table). Ported as a fake culture: a rejected date falls back to the invariant text,
    // an accepted date keeps the culture text, and the extreme .NET dates never throw.
    let texts = english();
    let numbers = invariant_numbers();
    let rejecting = FormatContext {
        texts: texts.as_ref(),
        numbers: &numbers,
        group_separator: ",",
        dates: &RejectingCulture,
    };
    let accepting = FormatContext {
        texts: texts.as_ref(),
        numbers: &numbers,
        group_separator: ",",
        dates: &AcceptingCulture,
    };

    let sample = date("2024-05-01 14:03:22");
    assert_eq!(format_date_time(sample, &rejecting), format!("inv {}", sample.ticks()));
    assert_eq!(format_date_time(sample, &accepting), "culture text");
    assert_eq!(
        format_date_time(DateTimeUtc::MIN, &rejecting),
        format!("inv {}", DateTimeUtc::MIN.ticks())
    );
    assert_eq!(
        format_date_time(DateTimeUtc::MAX, &rejecting),
        format!("inv {}", DateTimeUtc::MAX.ticks())
    );
}

#[test]
fn shutter_text_boundary_values_keep_the_number_part() {
    // C#: ExifFormatterEdgeTests.ShutterSpeed_BoundaryValues (12 InlineData rows)
    let env = en_env();
    let rows: [(u32, u32, &str); 12] = [
        (1, 250, "1/250"),
        (1, 1, "1"),
        (3, 10, "0.3"),
        (4, 10, "0.4"),
        (2, 500, "1/250"),
        (1, 2, "1/2"),
        (5, 10, "0.5"),
        (2, 1, "2"),
        (30, 1, "30"),
        (15, 10, "1.5"),
        (1, 8000, "1/8000"),
        (1, 4_294_967_295, "1/4294967295"),
    ];
    for (numerator, denominator, number_part) in rows {
        let text = shutter_text(Some(ExifRational::new(numerator, denominator)), &env.ctx());
        let text = text.unwrap_or_else(|| panic!("{numerator}/{denominator}: no shutter text"));
        assert!(text.contains(number_part), "{numerator}/{denominator}: {text}");
        assert!(text.ends_with(" s"), "{numerator}/{denominator}: {text}");
    }
}

#[test]
fn shutter_text_decimal_separator_follows_display_culture() {
    // C#: ExifFormatterEdgeTests.ShutterSpeed_DecimalSeparatorFollowsTheDisplayCulture
    let vi = Env::new(vietnamese(), vi_numbers(), ".", RecordedDates::none());
    let invariant = Env::new(english(), invariant_numbers(), ",", RecordedDates::none());
    let exposure = Some(ExifRational::new(15, 10));
    let vi_text = shutter_text(exposure, &vi.ctx()).expect("vi shutter text");
    let invariant_text = shutter_text(exposure, &invariant.ctx()).expect("invariant shutter text");
    assert!(vi_text.contains("1,5"), "{vi_text}");
    assert!(invariant_text.contains("1.5"), "{invariant_text}");
}

#[test]
fn camera_text_blank_parts_produce_nothing() {
    // C#: ExifFormatterEdgeTests.CameraText_BlankPartsProduceNothing (3 InlineData rows)
    let rows: [(Option<&str>, Option<&str>); 3] = [(None, None), (Some(""), Some("")), (Some("  "), Some(" "))];
    for (make, model) in rows {
        assert_eq!(camera_text(make, model), None, "make {make:?}, model {model:?}");
    }
}

#[test]
fn camera_text_does_not_repeat_the_make() {
    // C#: ExifFormatterEdgeTests.CameraText_DoesNotRepeatTheMake
    assert_eq!(
        camera_text(Some("Canon"), Some("Canon EOS R5")).as_deref(),
        Some("Canon EOS R5")
    );
    assert_eq!(
        camera_text(Some("NIKON CORPORATION"), Some("NIKON Z 6")).as_deref(),
        Some("NIKON Z 6")
    );
    assert_eq!(
        camera_text(Some("SONY"), Some("ILCE-7M3")).as_deref(),
        Some("SONY ILCE-7M3")
    );
    assert_eq!(camera_text(Some("Canon"), None).as_deref(), Some("Canon"));
    assert_eq!(camera_text(None, Some("EOS R5")).as_deref(), Some("EOS R5"));
}

// ---- Hand-written extras (`// new:`) ----

#[test]
fn is_blank_empty_and_unicode_whitespace_only_are_blank() {
    // new: T9, C# string.IsNullOrWhiteSpace (empty, spaces, U+3000, U+00A0 are blank; U+200B is not whitespace)
    use pr_app::viewmodels::format::context::is_blank;
    assert!(is_blank(""));
    assert!(is_blank("   "));
    assert!(is_blank("\u{3000}"));
    assert!(is_blank("\u{00A0}"));
    assert!(!is_blank("a"));
    assert!(!is_blank(" a "));
    assert!(!is_blank("\u{200B}"));
}

#[test]
fn starts_with_ignore_case_follows_ordinal_upper_not_lowercase() {
    // new: T8, C# StartsWith(OrdinalIgnoreCase); the ß, dotless i and dotted I rows are pinned by the golden
    // ordinal-ignore-case cases ("ß"/"SS" false, "ı"/"I" false, "i"/"İ" false).
    use pr_app::viewmodels::format::context::starts_with_ignore_case;
    assert!(starts_with_ignore_case("Canon EOS R5", "CANON"));
    assert!(starts_with_ignore_case("abc", ""));
    assert!(starts_with_ignore_case("東京カメラ", "東京"));
    assert!(!starts_with_ignore_case("ß", "SS"));
    assert!(!starts_with_ignore_case("ı", "I"));
    assert!(!starts_with_ignore_case("i", "İ"));
    assert!(!starts_with_ignore_case(" Canon", "Canon"));
}

// ---- Golden checks over tests/golden/format-exif.json ----

fn fields_bits(case: &serde_json::Value) -> u32 {
    let bits = golden::i64_of(case, "fields");
    u32::try_from(bits).unwrap_or_else(|_| panic!("fields out of range: {bits}"))
}

/// One golden case through the formatter that its `kind` names (`format`, `camera`, `shutter`).
fn run_exif_case(case: &serde_json::Value) -> Option<String> {
    let env = format_support::env_for_case(case);
    match golden::str_of(case, "kind") {
        "format" => {
            let exif = exif_of(&case["exif"]);
            let file_name = opt_text(case, "file_name");
            let modified_local = opt_text(case, "modified_local").map(|text| date(&text));
            let line = ExifLine {
                file_name: file_name.as_deref(),
                width: i32_of(case, "width"),
                height: i32_of(case, "height"),
                exif: exif.as_ref(),
                modified_local,
                raw_preview_width: i32_of(case, "raw_preview_width"),
                raw_preview_height: i32_of(case, "raw_preview_height"),
            };
            Some(format_exif(
                ExifInfoFields::from_bits_truncate(fields_bits(case)),
                &line,
                &env.ctx(),
            ))
        }
        "camera" => camera_text(opt_text(case, "make").as_deref(), opt_text(case, "model").as_deref()),
        "shutter" => shutter_text(rational_of(&case["exposure"]), &env.ctx()),
        other => panic!("unknown format-exif kind {other:?}"),
    }
}

#[test]
fn golden_format_exif() {
    // new: golden-driven, every case of tests/golden/format-exif.json (oracle format-exif, C# ExifFormatter); collects all differences
    let doc = golden::load("format-exif");
    let mut mismatches = Vec::new();
    for case in &doc.cases {
        let expected = opt_text(case, "text");
        let actual = run_exif_case(case);
        if actual != expected {
            mismatches.push(format!(
                "{}: expected {expected:?}, got {actual:?}",
                golden::str_of(case, "name")
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} format-exif cases differ from the C# oracle:\n{}",
        mismatches.len(),
        doc.cases.len(),
        mismatches.join("\n")
    );
}
