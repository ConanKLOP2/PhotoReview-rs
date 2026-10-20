// Mutation-kill tests: pin C# d92d6aae boundary behaviour (tiny custom-fixed values, positive-only dimensions, zero ISO / zero-denominator rationals).
mod format_support;
use format_support::*;
use pr_app::viewmodels::format::context::format_custom_fixed;
use pr_app::viewmodels::format::exif::{ExifLine, format_exif};
use pr_core::model::ExifInfoFields;
use pr_imaging::exif_summary::{ExifRational, ExifSummary};

fn line(w: i32, h: i32, rw: i32, rh: i32) -> ExifLine<'static> {
    ExifLine {
        file_name: None,
        width: w,
        height: h,
        exif: None,
        modified_local: None,
        raw_preview_width: rw,
        raw_preview_height: rh,
    }
}

// C#: (0.005).ToString("0.#") == "0", (0.05).ToString("0.#") == "0.1", (0.0005).ToString("0.##") == "0", (0.005).ToString("0.##") == "0.01"
#[test]
fn custom_fixed_tiny_values() {
    let n = invariant_numbers();
    assert_eq!(format_custom_fixed(0.005, 1, &n), "0");
    assert_eq!(format_custom_fixed(0.05, 1, &n), "0.1");
    assert_eq!(format_custom_fixed(0.0005, 2, &n), "0");
    assert_eq!(format_custom_fixed(0.0006, 2, &n), "0");
    assert_eq!(format_custom_fixed(0.005, 2, &n), "0.01");
    assert_eq!(format_custom_fixed(0.0049, 2, &n), "0");
    assert_eq!(format_custom_fixed(0.0009, 1, &n), "0");
    assert_eq!(format_custom_fixed(0.6, 0, &n), "1");
    assert_eq!(format_custom_fixed(0.4, 0, &n), "0");
    assert_eq!(format_custom_fixed(0.05, 0, &n), "0");
    assert_eq!(format_custom_fixed(0.5, 0, &n), "1");
}

#[test]
fn focal_length_tiny() {
    let env = en_env();
    let ctx = env.ctx();
    let mk = |n, d| ExifSummary {
        focal_length: Some(ExifRational::new(n, d)),
        ..Default::default()
    };
    for (n, d, want) in [(5, 1000, "0 mm"), (1, 20, "0.1 mm"), (6, 10000, "0 mm")] {
        let e = mk(n, d);
        let mut l = line(0, 0, 0, 0);
        l.exif = Some(&e);
        assert_eq!(format_exif(ExifInfoFields::FOCAL_LENGTH, &l, &ctx), want, "{n}/{d}");
    }
}

// C#: width > 0 && height > 0 (each side alone, 0 or negative)
#[test]
fn dimensions_need_both_sides_positive() {
    let env = en_env();
    let ctx = env.ctx();
    for (w, h) in [(0, 5), (5, 0), (0, 0), (-1, 5), (5, -1)] {
        assert_eq!(
            format_exif(ExifInfoFields::DIMENSIONS, &line(w, h, 0, 0), &ctx),
            "",
            "{w}x{h}"
        );
        assert_eq!(
            format_exif(ExifInfoFields::DIMENSIONS, &line(0, 0, w, h), &ctx),
            "",
            "raw {w}x{h}"
        );
    }
}

mod title {
    use super::*;
    use pr_app::viewmodels::format::title_bar::{TitleBarLine, format_title_bar};
    use pr_core::model::TitleBarFields;

    fn tl(w: i32, h: i32, exif: Option<&ExifSummary>) -> TitleBarLine<'_> {
        TitleBarLine {
            folder_path: "/x",
            index: None,
            count: None,
            file_name: None,
            length: None,
            width: w,
            height: h,
            modified_local: None,
            exif,
        }
    }

    // C#: width > 0 && height > 0
    #[test]
    fn title_dimensions_need_both_sides_positive() {
        let env = en_env();
        let ctx = env.ctx();
        for (w, h) in [(0, 5), (5, 0), (0, 0), (-1, 5), (5, -1)] {
            assert_eq!(
                format_title_bar(TitleBarFields::DIMENSIONS, &tl(w, h, None), &ctx),
                "",
                "{w}x{h}"
            );
        }
    }

    // C#: Iso is > 0; FocalLength.Value > 0 (Value is 0 when the denominator is 0); same for FNumber
    #[test]
    fn title_zero_iso_and_zero_denominator_rationals_are_skipped() {
        let env = en_env();
        let ctx = env.ctx();
        let e = ExifSummary {
            iso: Some(0),
            focal_length: Some(ExifRational::new(5, 0)),
            f_number: Some(ExifRational::new(5, 0)),
            ..Default::default()
        };
        let f = TitleBarFields::ISO | TitleBarFields::FOCAL_LENGTH | TitleBarFields::APERTURE;
        assert_eq!(format_title_bar(f, &tl(0, 0, Some(&e)), &ctx), "");
        let e = ExifSummary {
            iso: Some(-5),
            focal_length: Some(ExifRational::new(0, 5)),
            f_number: Some(ExifRational::new(0, 5)),
            ..Default::default()
        };
        assert_eq!(format_title_bar(f, &tl(0, 0, Some(&e)), &ctx), "");
    }
}
