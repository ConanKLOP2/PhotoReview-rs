//! Port of the WPF-free orientation tests of `PhotoReview.Imaging.Tests` (C# baseline d92d6aae) plus golden checks against
//! `tests/golden/orientation-values.json`, `orientation-read-wic.json` and `orientation-pixels.json` (generated from .NET by
//! `tools/oracle-cs`). Port of `PhotoReview.Imaging/Decoding/ExifOrientation.cs` and the transform table of
//! `PhotoReview.Imaging.Wpf/WpfExifOrientation.cs`.
//!
//! Written red in the W4-02a test pass and green once the bodies landed. Do not weaken an assertion to make one pass;
//! a golden that disagrees is a behaviour difference.
//!
//! Documented skips (no Rust twin, not ported here):
//! - `ExifOrientationPureTests.ReadFromWic_ThrowingQuery_IsOne_ButOutOfMemoryPropagates`: only the `InvalidCastException`
//!   assert is ported (as `Err(QueryFailed)` => 1). The OOM assert has no twin: Rust aborts on allocation failure.
//!   The `ArgumentNullException` assert has no twin: Rust has no null closure.
//! - `ExifOrientationPureTests.WicDecode_OrientationAndExifSummary_EqualTheWpfMetadataRead`: needs the WIC decoder (W4-11c/d).
//! - `ExifOrientationTests.Apply_OrientationOutsideTwoToEight_ReturnsTheSameFrozenInstanceUntouched`: W4-16a
//!   (`apply_orientation` returns the source); frozen-ness has no twin.
//! - `PixelOpsOrientationTests.ApplyOrientation_ThenInverse_IsIdentity_Property` and
//!   `ApplyOrientation_ComposesLikeTheDihedralGroup_Property`: W4-16a.
//! - `OrientationTests.ImageInfoDimensionSwap`: W4-01b (`ImageInfo`).

use pr_imaging::orientation::{
    EXIF_ORIENTATION_QUERY, OrientationTransform, QueryFailed, WINDOWS_ORIENTATION_QUERY, create_transform,
    is_transposed, map_pixel, normalize, oriented_size, read_from_wic,
};
use pr_test_support::golden::{self, i64_of, str_of};
use serde_json::Value;

/// Applies a transform to the pixel `(x, y)` of a `w` x `h` image the way WPF composed it: the mirrors first (dimensions
/// unchanged), then the clockwise rotation in a y-down plane. Used only by the tests, as the independent reading of the table.
fn apply_transform(t: OrientationTransform, x: u32, y: u32, w: u32, h: u32) -> (u32, u32) {
    let (x, y) = (
        if t.flip_horizontal { w - 1 - x } else { x },
        if t.flip_vertical { h - 1 - y } else { y },
    );
    match t.rotate_clockwise_degrees {
        0 => (x, y),
        90 => (h - 1 - y, x),
        180 => (w - 1 - x, h - 1 - y),
        270 => (y, w - 1 - x),
        other => panic!("unexpected rotation {other}"),
    }
}

/// Every orientation the goldens and the EXIF definition care about: 1..8, every invalid class and the `int` extremes.
const ALL_ORIENTATIONS: [i32; 16] = [
    i32::MIN,
    -3,
    -1,
    0,
    1,
    2,
    3,
    4,
    5,
    6,
    7,
    8,
    9,
    65535,
    i32::MAX,
    i32::MIN + 1,
];

// C#: ExifOrientationPureTests.Normalize_KeepsValidValues_AndMapsTheRestToOne
#[test]
fn normalize_keeps_valid_values_and_maps_the_rest_to_one() {
    let rows: [(i32, i32); 9] = [
        (1, 1),
        (2, 2),
        (5, 5),
        (8, 8),
        (0, 1),
        (9, 1),
        (-3, 1),
        (i32::MAX, 1),
        (i32::MIN, 1),
    ];
    for (orientation, expected) in rows {
        assert_eq!(normalize(orientation), expected, "normalize({orientation})");
    }
}

// C#: ExifOrientationPureTests.IsTransposed_IsFiveToEight
#[test]
fn is_transposed_is_five_to_eight() {
    let rows: [(i32, bool); 4] = [(4, false), (5, true), (8, true), (9, false)];
    for (orientation, expected) in rows {
        assert_eq!(is_transposed(orientation), expected, "is_transposed({orientation})");
    }
}

// C#: ExifOrientationPureTests.ReadFromWic_ExifTagFirst_ThenTheWindowsPolicy
// (`(ushort)n` becomes `Ok(Some(n))`, `null` becomes `Ok(None)`.)
#[test]
fn read_from_wic_exif_tag_first_then_the_windows_policy() {
    let rows: [(Option<i64>, Option<i64>, i32); 7] = [
        (Some(6), None, 6),
        (Some(1), Some(8), 1),
        (Some(0), Some(3), 3),
        (Some(9), Some(7), 7),
        (None, Some(2), 2),
        (None, Some(9), 1),
        (None, None, 1),
    ];
    for (exif, windows, expected) in rows {
        let got = read_from_wic(|name| match name {
            EXIF_ORIENTATION_QUERY => Ok(exif),
            WINDOWS_ORIENTATION_QUERY => Ok(windows),
            other => panic!("unexpected query {other}"),
        });
        assert_eq!(got, expected, "exif={exif:?} windows={windows:?}");
    }
}

// C#: ExifOrientationPureTests.ReadFromWic_ArrayValue_UsesTheFirstElement
// The array rule belongs to W4-04b `as_integer`; here the closure already gives `Ok(Some(5))` (first element of `[5, 2]`),
// and the test keeps the C# name so the row stays traceable.
#[test]
fn read_from_wic_array_value_uses_the_first_element() {
    let got = read_from_wic(|name| match name {
        EXIF_ORIENTATION_QUERY => Ok(Some(5)),
        _ => Ok(None),
    });
    assert_eq!(got, 5);
}

// C#: ExifOrientationPureTests.ReadFromWic_ThrowingQuery_IsOne_ButOutOfMemoryPropagates
// Only the first assert has a Rust twin (see the module doc for the skipped OOM and null asserts).
#[test]
fn read_from_wic_throwing_query_is_one() {
    assert_eq!(read_from_wic(|_| Err(QueryFailed)), 1);
}

// C#: ExifOrientationPureTests.ReadFromWic_TheWindowsQueryIsOnlyAskedWhenTheExifTagIsUnusable
#[test]
fn read_from_wic_the_windows_query_is_only_asked_when_the_exif_tag_is_unusable() {
    let mut asked: Vec<String> = Vec::new();
    let got = read_from_wic(|name| {
        asked.push(name.to_owned());
        Ok(Some(6))
    });
    assert_eq!(got, 6);
    assert_eq!(asked, vec![EXIF_ORIENTATION_QUERY.to_owned()]);
}

// C#: ExifOrientationTests.Apply_NonSquareImage_PutsEverySourcePixelWhereTheExifOrientationDefinesIt
// Port: the 3x2 Bgra32 image is replaced by the coordinate arithmetic, which is what `apply_orientation` (W4-16a) moves pixels by.
// Every source pixel must land where the EXIF definition puts it, and the output size must be swapped for 5..=8.
#[test]
fn apply_non_square_image_puts_every_source_pixel_where_the_exif_orientation_defines_it() {
    const W: u32 = 3;
    const H: u32 = 2;
    for orientation in 2..=8 {
        let (out_w, out_h) = oriented_size(orientation, W, H);
        let transposed = orientation >= 5;
        assert_eq!(
            (out_w, out_h),
            if transposed { (H, W) } else { (W, H) },
            "oriented_size({orientation})"
        );
        let mut seen = vec![false; (out_w * out_h) as usize];
        for y in 0..H {
            for x in 0..W {
                let (dx, dy) = map_pixel(orientation, x, y, W, H);
                assert!(
                    dx < out_w && dy < out_h,
                    "orientation {orientation}: ({x},{y}) -> ({dx},{dy}) out of bounds"
                );
                let slot = (dy * out_w + dx) as usize;
                assert!(!seen[slot], "orientation {orientation}: two pixels land on ({dx},{dy})");
                seen[slot] = true;
            }
        }
        assert!(
            seen.iter().all(|&s| s),
            "orientation {orientation}: some output pixel has no source"
        );
    }
}

// C#: ExifOrientationTests.CreateTransform_OrientationOutsideOneToEight_IsTheIdentity
#[test]
fn create_transform_orientation_outside_one_to_eight_is_the_identity() {
    for orientation in [0, 9, 65535, -3] {
        assert_eq!(
            create_transform(orientation),
            OrientationTransform::IDENTITY,
            "create_transform({orientation})"
        );
    }
}

// C#: PixelOpsOrientationTests.ApplyOrientation_MovesKnownPixel
// Port against `map_pixel` (W4-16a ports the same rows against the buffer loop). Source 3x2, row = (orientation, srcX, srcY, dstX, dstY).
#[test]
fn apply_orientation_moves_known_pixel() {
    let rows: [(i32, u32, u32, u32, u32); 7] = [
        (2, 0, 0, 2, 0),
        (3, 0, 0, 2, 1),
        (4, 0, 0, 0, 1),
        (5, 2, 0, 0, 2),
        (6, 0, 0, 1, 0),
        (7, 0, 0, 1, 2),
        (8, 0, 0, 0, 2),
    ];
    for (orientation, src_x, src_y, dst_x, dst_y) in rows {
        assert_eq!(
            map_pixel(orientation, src_x, src_y, 3, 2),
            (dst_x, dst_y),
            "map_pixel({orientation}, {src_x}, {src_y}, 3, 2)"
        );
    }
}

// Rust-only: the closed-form `map_pixel` and the flips-then-rotate table of `create_transform` must agree on every pixel
// of every shape the goldens use, for every orientation including the invalid ones (identity).
#[test]
fn create_transform_agrees_with_map_pixel() {
    let shapes: [(u32, u32); 6] = [(3, 2), (2, 3), (1, 1), (4, 1), (1, 4), (5, 3)];
    for orientation in ALL_ORIENTATIONS {
        let transform = create_transform(orientation);
        for (w, h) in shapes {
            for y in 0..h {
                for x in 0..w {
                    assert_eq!(
                        apply_transform(transform, x, y, w, h),
                        map_pixel(orientation, x, y, w, h),
                        "orientation {orientation}, shape {w}x{h}, pixel ({x},{y})"
                    );
                }
            }
        }
    }
}

// Golden: tests/golden/orientation-values.json (oracle-cs orientation-values)
#[test]
fn golden_orientation_values_match_csharp() {
    let gold = golden::load("orientation-values");
    assert_eq!(gold.csharp_commit, "d92d6aae639a40eede232948f9bce97ecfd02c10");
    for case in &gold.cases {
        let name = str_of(case, "name");
        let values = int_list(case, "values");
        let normalized = int_list(case, "normalize");
        let transposed = bool_list(case, "transposed");
        assert_eq!(values.len(), normalized.len(), "{name}: normalize length");
        assert_eq!(values.len(), transposed.len(), "{name}: transposed length");
        for (i, &value) in values.iter().enumerate() {
            let value = i32::try_from(value).expect("golden value fits i32");
            assert_eq!(normalize(value), normalized[i] as i32, "{name}: normalize({value})");
            assert_eq!(is_transposed(value), transposed[i], "{name}: is_transposed({value})");
        }
        assert_eq!(
            str_of(case, "exif_query"),
            EXIF_ORIENTATION_QUERY,
            "{name}: exif query string"
        );
        assert_eq!(
            str_of(case, "windows_query"),
            WINDOWS_ORIENTATION_QUERY,
            "{name}: windows query string"
        );
    }
}

// Golden: tests/golden/orientation-read-wic.json (oracle-cs orientation-read-wic)
// The oracle goes through the real C# `AsInteger`. The Rust closure receives the already-reduced integer (contract
// semantics note 4), so the case's managed type is mapped to the `as_integer` result here: byte/short/ushort/int/uint
// give the value, `ushort[]` its first element (`None` when empty), `string` and `long` are not integers (`None`),
// `null` is absent, and `throw` is `Err(QueryFailed)`.
#[test]
fn golden_orientation_read_wic_matches_csharp() {
    let gold = golden::load("orientation-read-wic");
    assert_eq!(gold.csharp_commit, "d92d6aae639a40eede232948f9bce97ecfd02c10");
    for case in &gold.cases {
        let name = str_of(case, "name");
        let exif = &case["exif"];
        let windows = &case["windows"];
        let expected = i32::try_from(i64_of(case, "orientation")).expect("golden orientation fits i32");
        let expected_asked: Vec<&str> = case["asked"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: missing 'asked'"))
            .iter()
            .map(|v| {
                v.as_str()
                    .unwrap_or_else(|| panic!("{name}: asked entry is not a string"))
            })
            .collect();

        let mut asked: Vec<&str> = Vec::new();
        let got = read_from_wic(|query| match query {
            EXIF_ORIENTATION_QUERY => {
                asked.push("exif");
                as_integer_case(exif)
            }
            WINDOWS_ORIENTATION_QUERY => {
                asked.push("windows");
                as_integer_case(windows)
            }
            other => panic!("{name}: unexpected query {other}"),
        });

        assert_eq!(got, expected, "{name}: orientation");
        assert_eq!(asked, expected_asked, "{name}: asked queries");
    }
}

// Golden: tests/golden/orientation-pixels.json (oracle-cs orientation-pixels, through the real C# `PixelOps.ApplyOrientation`)
// Checks, per case: the output size, the `same_instance` flag (identity exactly for the no-op orientations), the closed-form
// destination of every source pixel, and that the output buffer built from `map_pixel` equals the golden `source_ids`.
#[test]
fn golden_orientation_pixels_match_csharp() {
    let gold = golden::load("orientation-pixels");
    assert_eq!(gold.csharp_commit, "d92d6aae639a40eede232948f9bce97ecfd02c10");
    for case in &gold.cases {
        let name = str_of(case, "name");
        let orientation = i32::try_from(i64_of(case, "orientation")).expect("golden orientation fits i32");
        let w = u32::try_from(i64_of(case, "width")).expect("width");
        let h = u32::try_from(i64_of(case, "height")).expect("height");
        let out_w = u32::try_from(i64_of(case, "out_width")).expect("out_width");
        let out_h = u32::try_from(i64_of(case, "out_height")).expect("out_height");
        let same_instance = case["same_instance"]
            .as_bool()
            .unwrap_or_else(|| panic!("{name}: same_instance"));
        let source_ids = int_list(case, "source_ids");
        assert_eq!(source_ids.len(), (w * h) as usize, "{name}: source_ids length");

        assert_eq!(
            oriented_size(orientation, w, h),
            (out_w, out_h),
            "{name}: oriented size"
        );
        assert_eq!(
            same_instance,
            create_transform(orientation) == OrientationTransform::IDENTITY,
            "{name}: same_instance vs identity transform"
        );

        let mut out = vec![-1_i64; (out_w * out_h) as usize];
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = map_pixel(orientation, x, y, w, h);
                assert!(
                    dx < out_w && dy < out_h,
                    "{name}: ({x},{y}) -> ({dx},{dy}) out of bounds"
                );
                out[(dy * out_w + dx) as usize] = i64::from(y * w + x);
            }
        }
        assert_eq!(out, source_ids, "{name}: output source ids");
    }
}

/// The `as_integer` result the Rust closure is given for one golden case value (see the read-wic golden test).
fn as_integer_case(value: &Value) -> Result<Option<i64>, QueryFailed> {
    if value.is_null() {
        return Ok(None);
    }
    match value["type"].as_str().expect("case value has a type") {
        "throw" => Err(QueryFailed),
        "byte" | "short" | "ushort" | "int" | "uint" => Ok(Some(i64_of(value, "value"))),
        "ushort[]" => Ok(value["values"]
            .as_array()
            .and_then(|values| values.first())
            .map(|first| first.as_i64().expect("array element is an integer"))),
        "string" | "long" => Ok(None),
        other => panic!("unknown golden value type {other}"),
    }
}

fn int_list(case: &Value, key: &str) -> Vec<i64> {
    case[key]
        .as_array()
        .unwrap_or_else(|| panic!("case has no array '{key}': {case}"))
        .iter()
        .map(|v| {
            v.as_i64()
                .unwrap_or_else(|| panic!("'{key}' entry is not an integer: {v}"))
        })
        .collect()
}

fn bool_list(case: &Value, key: &str) -> Vec<bool> {
    case[key]
        .as_array()
        .unwrap_or_else(|| panic!("case has no array '{key}': {case}"))
        .iter()
        .map(|v| {
            v.as_bool()
                .unwrap_or_else(|| panic!("'{key}' entry is not a bool: {v}"))
        })
        .collect()
}
