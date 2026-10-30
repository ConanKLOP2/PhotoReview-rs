//! C# JsonDocument config grammar, baseline 7eaee2a2; X-stj-reader continuation.
use pr_core::settings::app_settings::json::{self, JsonValue};

#[test]
fn json_string_cursor_preserves_each_escape_and_utf8_character() {
    // app_settings.rs:1277,1287,1291,1295,1339 += -> -=/*=; :1297..1305 delete match arms.
    // C#: JsonDocument string decoding (W2-01 syntax/escape contract).
    for (input, expected) in [
        (r#""a""#, "a"),
        (r#""""#, ""),
        // :1291 += -> -= reads the preceding 'a' as an invalid escape and fails promptly.
        (r#""a\n""#, "a\n"),
        (r#""\"""#, "\""),
        (r#""\\""#, "\\"),
        (r#""\/""#, "/"),
        (r#""\b""#, "\u{8}"),
        (r#""\f""#, "\u{c}"),
        (r#""\n""#, "\n"),
        (r#""\r""#, "\r"),
        (r#""\t""#, "\t"),
        (r#""\u0041""#, "A"),
        ("\"\u{e9}\u{1f600}\"", "\u{e9}\u{1f600}"),
    ] {
        assert_eq!(
            json::parse(input).unwrap(),
            JsonValue::String(Some(expected.into())),
            "{input}"
        );
    }
    for input in ["\"", "\"a", "\"\\", r#""\q""#] {
        assert!(json::parse(input).is_err(), "{input}");
    }
}

#[test]
fn json_whitespace_and_comments_preserve_the_following_value() {
    // app_settings.rs:1145..1183 skip_plain_ws/skip_ws return, arithmetic, match-arm mutations.
    // C#: JsonDocumentOptions CommentHandling.Skip; only four JSON whitespace characters.
    for prefix in [
        " ",
        "\t",
        "\r",
        "\n",
        "/**/",
        "/*a*b/ */",
        "/*\u{e9}*/",
        "//a\n",
        "//\u{e9}\r",
        "//",
        "//a",
    ] {
        let input = if prefix.starts_with("//") && !prefix.ends_with(['\n', '\r']) {
            format!("true{prefix}")
        } else {
            format!("{prefix}true")
        };
        assert_eq!(json::parse(&input).unwrap(), JsonValue::Bool(true), "{input:?}");
    }
    for input in ["/", "/x", "/*", "/*a", "//\u{2028}", "//\u{2029}", "\u{a0}true"] {
        assert!(json::parse(input).is_err(), "{input:?}");
    }
}

#[test]
fn json_number_cursor_keeps_the_entire_token_and_requires_digits() {
    // app_settings.rs:1221..1260 parse_number arithmetic, booleans, return/match-arm replacements.
    // C#: JsonDocument rejects non-JSON number grammar, preserves raw numeric tokens.
    for input in ["0", "-0", "12", "-12", "0.25", "1e2", "1E+23", "-12.34e-56"] {
        assert_eq!(json::parse(input).unwrap(), JsonValue::Number(input.into()), "{input}");
    }
    for input in ["-", "-x", "01", "1.", "1.x", "1e", "1E+", "1e-", "1e+x"] {
        assert!(json::parse(input).is_err(), "{input}");
    }
    assert_eq!(
        json::parse("1x").unwrap_err().message,
        "invalid character after a number"
    );
}

#[test]
fn json_object_requires_colons_separators_and_exact_depth() {
    // app_settings.rs:1377..1420 parse_object >, !=, += and deleted match arms/return replacements.
    // C#: JsonDocumentOptions AllowTrailingCommas=true, MaxDepth=64 (W2-01).
    assert_eq!(json::parse("{}").unwrap(), JsonValue::Object(vec![]));
    assert_eq!(
        json::parse("{\"a\" \t\r\n:/*x*/1,\"b\":false,}").unwrap(),
        JsonValue::Object(vec![
            (Some("a".into()), JsonValue::Number("1".into())),
            (Some("b".into()), JsonValue::Bool(false)),
        ])
    );
    for input in [
        "{",
        "{a:1}",
        "{\"a\" 1}",
        "{\"a\"/*x*/:1}",
        "{\"a\":}",
        "{\"a\":1 \"b\":2}",
        "{\"a\":1,,}",
        "{\"a\":1",
    ] {
        assert!(json::parse(input).is_err(), "{input}");
    }
    for (depth, valid) in [(63, true), (64, true), (65, false)] {
        let input = format!("{}0{}", "{\"a\":".repeat(depth), "}".repeat(depth));
        assert_eq!(json::parse(&input).is_ok(), valid, "depth {depth}");
    }
}
