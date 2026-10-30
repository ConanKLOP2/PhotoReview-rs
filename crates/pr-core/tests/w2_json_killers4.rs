//! Port of System.Text.Json comment grammar, C# baseline 7eaee2a2; X-stj-reader.
use pr_core::settings::app_settings::json::{self, JsonValue};

#[test]
fn json_line_comments_preserve_utf8_and_stop_at_json_newlines() {
    // app_settings.rs:1157 + -> -/*; :1159 += -> -=/*=; :1166 += -> -=/*=.
    // Put UTF-8 first: cursor multiplication must fail promptly, before ASCII loops.
    assert_eq!(json::parse("true//éé").unwrap(), JsonValue::Bool(true));
    assert_eq!(json::parse("true//a").unwrap(), JsonValue::Bool(true));
    // app_settings.rs:1164 delete newline match arm; C#: CommentHandling.Skip.
    for newline in ['\n', '\r'] {
        assert_eq!(
            json::parse(&format!("//comment{newline}true")).unwrap(),
            JsonValue::Bool(true)
        );
    }
    // app_settings.rs:1165 delete Unicode line-separator rejection match arm.
    for separator in ['\u{2028}', '\u{2029}'] {
        assert!(json::parse(&format!("true//{separator}")).is_err());
    }
}

#[test]
fn json_block_comments_require_the_exact_closing_delimiter() {
    // app_settings.rs:1171 += -> -=/*=; :1175 guard true/false, == -> !=, + -> -/*.
    // app_settings.rs:1176 += -> -=/*=; :1179 += -> -=/*=.
    // :1176 -= returns into whitespace, then exposes '*' instead of looping on '/'.
    assert_eq!(json::parse("/*  */true").unwrap(), JsonValue::Bool(true));
    assert_eq!(json::parse("/**/true").unwrap(), JsonValue::Bool(true));
    assert_eq!(json::parse("true/**/").unwrap(), JsonValue::Bool(true));
    assert_eq!(json::parse("/*a*b/ */true").unwrap(), JsonValue::Bool(true));
    assert!(json::parse("true/*/").is_err());
    assert!(json::parse("true/*a").is_err());
}

#[test]
fn json_decimal_strings_keep_the_public_finite_conversion_boundary() {
    // app_settings.rs:1487 return true; :1496 - -> + (CX-JSON2 equivalents).
    // C#: AllowReadingFromString with named non-finite values handled separately.
    for (text, expected) in [
        ("+.5", Some(0.5)),
        ("-1.e2", Some(-100.0)),
        ("+.", None),
        ("-e2", None),
        ("+", None),
        ("1e9999", None),
        ("inf", None),
        ("nan", None),
        (" 1", None),
    ] {
        assert_eq!(
            json::to_f64(&JsonValue::String(Some(text.into()))),
            expected,
            "{text:?}"
        );
    }
}

#[test]
fn json_string_cursor_advances_over_unicode_and_hex_escapes() {
    // app_settings.rs:1295 += -> -=; :1339 += -> *= (CX-JSON3 timeouts).
    // Hex escape first avoids the decrement revisiting an ordinary escape forever.
    assert_eq!(json::parse(r#""\u0041""#).unwrap(), JsonValue::String(Some("A".into())));
    // Multiplication lands inside the UTF-8 character, rather than spinning on ASCII.
    assert_eq!(json::parse("\"é\"").unwrap(), JsonValue::String(Some("é".into())));
}
