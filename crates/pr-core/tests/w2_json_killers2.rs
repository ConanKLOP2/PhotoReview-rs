//! System.Text.Json reader/writer contracts, C# baseline 7eaee2a2 (W2-01).
use pr_core::settings::app_settings::json::{self, JsonValue};

#[test]
fn json_string_escapes_decode_exactly_and_reject_incomplete_input() {
    // app_settings.rs:1277..1339 cursor += replacements and deleted escape match arms.
    // C#: JsonDocument string decoding, W2-01 syntax and escape tables.
    assert_eq!(
        json::parse(r#""a\"\\\/\b\f\n\r\t\u0041é😀""#).unwrap(),
        JsonValue::String(Some("a\"\\/\u{8}\u{c}\n\r\tAé😀".into()))
    );
    for invalid in ["\"", "\"abc", "\"\\", r#""\q""#, "\"\u{1f}\""] {
        assert!(json::parse(invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn json_writer_preserves_escape_boundaries_and_number_notation() {
    // app_settings.rs:1617 < -> <=, || -> &&; :1620 ASCII guard replacements.
    // C#: Utf8JsonWriter default encoder, W2-01 escape table.
    assert_eq!(
        json::escape_string(" \u{1f}\u{7f}é😀"),
        r#"" \u001F\u007F\u00E9\uD83D\uDE00""#
    );
    // app_settings.rs:1664 > -> >=, < -> <=; :1682 > -> >=; :1687 < -> <=.
    // C#: Utf8JsonWriter.WriteNumberValue(double), W2-01 notation boundaries.
    for (value, expected) in [
        (1e-5, "1E-05"),
        (1e-4, "0.0001"),
        (1e16, "10000000000000000"),
        (1e17, "1E+17"),
        (1.25e18, "1.25E+18"),
        (-0.0, "-0"),
        (0.0, "0"),
        (12.5, "12.5"),
        (-1.25e-6, "-1.25E-06"),
    ] {
        assert_eq!(json::format_f64(value).as_deref(), Some(expected), "{value}");
    }
}

#[test]
fn json_decimal_strings_require_complete_ascii_grammar() {
    // app_settings.rs:1493..1522 is_decimal_literal comparisons, arithmetic and booleans.
    // C#: JsonNumberHandling.AllowReadingFromString, W2-01 floating string grammar.
    for (token, expected) in [
        (".5", 0.5),
        ("1.", 1.0),
        ("+1.25e-2", 0.0125),
        ("-12E+2", -1200.0),
        ("0", 0.0),
        ("12.34", 12.34),
    ] {
        assert_eq!(
            json::to_f64(&JsonValue::String(Some(token.into()))),
            Some(expected),
            "{token}"
        );
    }
    for token in [
        "", "+", "-", ".", "+.", "e1", "1e", "1e+", "1e-", "1x", "1.2x", "1e2x", " 1", "1 ", "１", "1e999", "inf",
        "infinity",
    ] {
        assert_eq!(json::to_f64(&JsonValue::String(Some(token.into()))), None, "{token}");
    }
}

#[test]
fn json_reader_preserves_tokens_comments_and_container_boundaries() {
    // app_settings.rs:1148..1425 Parser cursor arithmetic, delimiter and depth comparisons.
    // C#: JsonDocumentOptions AllowTrailingCommas and CommentHandling.Skip (W2-01).
    let doc = json::parse(" \t\r\n/*é*/{\"a\" \t: //x\r\n[true,false,null,-12.34e+5,],} //end").unwrap();
    assert_eq!(
        doc.property("a"),
        Some(&JsonValue::Array(vec![
            JsonValue::Bool(true),
            JsonValue::Bool(false),
            JsonValue::Null,
            JsonValue::Number("-12.34e+5".into())
        ]))
    );
    assert_eq!(json::parse("[]").unwrap(), JsonValue::Array(vec![]));
    assert_eq!(json::parse("{}").unwrap(), JsonValue::Object(vec![]));
    // app_settings.rs:1203 at_delimiter -> true: preserve the public diagnostic,
    // rather than allowing the root's trailing-data check to hide this change.
    assert_eq!(json::parse("truex").unwrap_err().message, "invalid literal");
    assert_eq!(
        json::parse("1x").unwrap_err().message,
        "invalid character after a number"
    );
    for token in ["0", "-0", "12", "0.5", "1E-2"] {
        assert_eq!(json::parse(token).unwrap(), JsonValue::Number(token.into()));
    }
    for invalid in [
        "",
        "/",
        "/*",
        "//\u{2028}",
        "//\u{2029}",
        "truex",
        "false0",
        "null_",
        "01",
        "-",
        "1.",
        "1e",
        "1e+",
        "1x",
        "[1 2]",
        "[1,,]",
        "{a:1}",
        "{\"a\" 1}",
        "{\"a\"/*x*/:1}",
        "{\"a\":1 \"b\":2}",
        "true false",
    ] {
        assert!(json::parse(invalid).is_err(), "{invalid:?}");
    }
    for open in ['[', '{'] {
        let nested = |depth| {
            if open == '[' {
                format!("{}0{}", "[".repeat(depth), "]".repeat(depth))
            } else {
                format!("{}0{}", "{\"a\":".repeat(depth), "}".repeat(depth))
            }
        };
        assert!(json::parse(&nested(64)).is_ok());
        assert!(json::parse(&nested(65)).is_err());
    }
}
