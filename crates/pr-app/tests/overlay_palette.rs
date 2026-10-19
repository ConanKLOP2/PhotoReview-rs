//! Red tests (W8-15b, test porter) for `pr_app::overlay::palette`: the dark palette of the Direct2D overlay.
//!
//! Port of `PhotoReview.Shell.Tests/Overlay/DarkPaletteTests.cs` (4 facts), plus the golden
//! `tests/golden/overlay-palette.json` (17 cases: 1 `table`, 9 `get`, 7 `from_argb`) and the XAML key list in
//! `tests/golden-inputs/overlay-palette.json` (`xaml_brushes`, 35 entries; the WPF XAML itself is retired, contract O7).
//! Every test here fails through `todo!()` until the porter fills the bodies of `overlay::palette`.

use pr_app::overlay::palette::{
    border_strong, button, button_hover, button_pressed, from_argb, get, overlay, text_bright, text_secondary, tooltip,
    warning, TABLE,
};
use pr_app::viewport::render_geometry::ColorF;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// The XAML brushes of `DarkPalette.xaml` as the golden input carries them: `(key, hex)`, hex is 6 or 8 digits.
fn xaml_brushes() -> Vec<(String, u32)> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("overlay-palette.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["xaml_brushes"]
        .as_array()
        .expect("xaml_brushes")
        .iter()
        .map(|brush| {
            let key = str_of(brush, "key").to_owned();
            let hex = str_of(brush, "hex");
            let value = u32::from_str_radix(hex, 16).unwrap_or_else(|_| panic!("bad hex {hex} for {key}"));
            // A 6-digit colour has full alpha (contract O7).
            let argb = if hex.len() == 6 { 0xFF00_0000 | value } else { value };
            (key, argb)
        })
        .collect()
}

/// A golden number: a JSON number, or one of the oracle strings.
fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or_else(|| panic!("not a float: {value}")),
        Value::String(text) => match text.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            "-0" => -0.0,
            other => other.parse::<f64>().unwrap_or_else(|_| panic!("not a number: {other}")),
        },
        other => panic!("not a number: {other}"),
    }
}

/// Golden channels are the f64 widening of the C# float: compare bit for bit after widening the Rust f32.
fn assert_color_matches(actual: ColorF, case: &Value, step: &str) {
    assert_eq!(f64::from(actual.r), num(&case["r"]), "{step}: r");
    assert_eq!(f64::from(actual.g), num(&case["g"]), "{step}: g");
    assert_eq!(f64::from(actual.b), num(&case["b"]), "{step}: b");
    assert_eq!(f64::from(actual.a), num(&case["a"]), "{step}: a");
}

// C#: DarkPaletteTests.EveryXamlKey_IsInTheTable_WithTheSameColor
#[test]
fn every_xaml_key_is_in_the_table_with_the_same_color() {
    let xaml = xaml_brushes();

    assert!(xaml.len() >= 30, "parsed only {} brushes from the XAML", xaml.len());
    for (key, argb) in &xaml {
        assert_eq!(
            get(key),
            Some(from_argb(*argb)),
            "{key} missing from the palette or has another colour"
        );
    }
}

// C#: DarkPaletteTests.TableHasNoKeyThatTheXamlLacks
#[test]
fn table_has_no_key_that_the_xaml_lacks() {
    let xaml = xaml_brushes();

    for (key, _) in TABLE {
        assert!(
            xaml.iter().any(|(k, _)| k.as_str() == *key),
            "{key} is not in DarkPalette.xaml"
        );
    }
    assert_eq!(
        TABLE.len(),
        xaml.len(),
        "table and XAML must have the same number of keys"
    );
}

// C#: DarkPaletteTests.ArgbConversion_UsesStraightAlpha
#[test]
fn argb_conversion_uses_straight_alpha() {
    // Dark.Overlay = #B0181818: A = 0xB0, RGB = 0x18.
    let overlay = overlay();
    assert!((overlay.a - 0xB0 as f32 / 255.0).abs() <= 1e-5, "alpha of Dark.Overlay");
    assert!((overlay.r - 0x18 as f32 / 255.0).abs() <= 1e-5, "red of Dark.Overlay");
    assert_eq!(overlay.r, overlay.g);
    assert_eq!(overlay.r, overlay.b);

    let c = from_argb(0x33FF_0080);
    assert!((c.a - 0x33 as f32 / 255.0).abs() <= 1e-5, "alpha");
    assert!((c.r - 1.0).abs() <= 1e-5, "red");
    assert!(c.g.abs() <= 1e-5, "green");
    assert!((c.b - 0x80 as f32 / 255.0).abs() <= 1e-5, "blue");
}

// C#: DarkPaletteTests.Get_UnknownKey_Throws (the port returns None instead of throwing, contract O4)
#[test]
fn get_unknown_key_is_none() {
    assert_eq!(get("Dark.DoesNotExist"), None);
}

// new: `get` is an exact lookup, so the table keys must be unique (a duplicate would hide a wrong colour).
#[test]
fn table_keys_are_unique() {
    let mut keys: Vec<&str> = TABLE.iter().map(|(key, _)| *key).collect();
    keys.sort_unstable();
    let before = keys.len();
    keys.dedup();

    assert_eq!(keys.len(), before, "duplicate key in the palette table");
    assert_eq!(TABLE.len(), 35, "the C# table has 35 rows");
}

// new: each named accessor is exactly `get` of its table key.
#[test]
fn named_accessors_equal_get_of_their_key() {
    let pairs = [
        (overlay(), "Dark.Overlay"),
        (text_bright(), "Dark.TextBright"),
        (text_secondary(), "Dark.TextSecondary"),
        (warning(), "Dark.Warning"),
        (button(), "Dark.Button"),
        (button_hover(), "Dark.ButtonHover"),
        (button_pressed(), "Dark.ButtonPressed"),
        (tooltip(), "Dark.Tooltip"),
        (border_strong(), "Dark.BorderStrong"),
    ];
    for (accessor, key) in pairs {
        assert_eq!(Some(accessor), get(key), "accessor for {key}");
    }
}

// new: lookup is ordinal, untrimmed and case-sensitive (contract T8).
#[test]
fn get_is_ordinal_and_untrimmed() {
    assert_eq!(get("dark.overlay"), None, "lower case");
    assert_eq!(get("Dark.Overlay "), None, "trailing space");
    assert_eq!(get(""), None, "empty key");
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden: tests/golden/overlay-palette.json (`oracle-cs overlay-palette`, C# d92d6aae).
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn golden_overlay_palette() {
    let golden = golden::load("overlay-palette");
    assert_eq!(golden.generator, "oracle-cs overlay-palette");
    assert_eq!(golden.cases.len(), 17, "overlay-palette golden has 17 cases");

    for case in &golden.cases {
        let name = str_of(case, "name");
        match str_of(case, "kind") {
            "table" => {
                let entries = case["entries"].as_array().expect("entries");
                assert_eq!(entries.len(), TABLE.len(), "{name}: row count");
                for (index, (entry, (key, argb))) in entries.iter().zip(TABLE).enumerate() {
                    let step = format!("{name} row #{index} ({key})");
                    assert_eq!(str_of(entry, "key"), *key, "{step}: key in declaration order");
                    assert_eq!(i64_of(entry, "argb") as u32, *argb, "{step}: argb");
                    assert_color_matches(from_argb(*argb), entry, &step);
                }
            }
            "get" => {
                let key = str_of(case, "key");
                if bool_of(case, "found") {
                    let color = get(key).unwrap_or_else(|| panic!("{name}: {key} not found"));
                    assert_color_matches(color, case, name);
                } else {
                    assert_eq!(get(key), None, "{name}: {key} must not be found");
                    assert_eq!(case["threw"].as_str(), Some("KeyNotFoundException"), "{name}");
                }
            }
            "from_argb" => {
                let argb = i64_of(case, "argb") as u32;
                assert_color_matches(from_argb(argb), case, name);
            }
            other => panic!("{name}: unknown kind {other}"),
        }
    }
}
