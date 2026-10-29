//! System.Text.Json helper contracts from W2-01, C# baseline `7eaee2a2`.
//! Direct DOM assertions complement the typed AppSettings golden cases.

use pr_core::settings::app_settings::json::{self, JsonValue};

#[test]
fn json_property_duplicate_names_return_the_last_ordinal_match() {
    // app_settings.rs:1117 replace property with None; :1118 delete Object arm; :1121 == -> !=.
    // C#: JsonElement.TryGetProperty (SettingsStore raw-document inspection).
    let value = json::parse(r#"{"name":1,"Name":2,"name":3,"\ud800":4}"#).unwrap();
    assert_eq!(value.property("name"), Some(&JsonValue::Number("3".into())));
    assert_eq!(value.property("Name"), Some(&JsonValue::Number("2".into())));
    assert_eq!(value.property("absent"), None);
    assert_eq!(JsonValue::Null.property("name"), None);
}

#[test]
fn json_integer_strings_keep_sign_and_reject_values_outside_i64() {
    // app_settings.rs:1447 || -> &&, delete !; :1453 > -> <; :1457 delete -.
    // C#: System.Text.Json JsonNumberHandling.AllowReadingFromString, W2-01 integer grammar.
    for (token, expected) in [
        ("+42", Some(42)),
        ("-42", Some(-42)),
        ("-9223372036854775808", Some(i64::MIN)),
        ("9223372036854775807", Some(i64::MAX)),
        ("9223372036854775808", None),
        ("-9223372036854775809", None),
        ("18446744073709551616", None),
        ("999999999999999999999999999999999999999999999999999999", None),
        ("", None),
        ("+", None),
        ("-", None),
        ("1x", None),
        (" 42", None),
    ] {
        assert_eq!(
            json::to_i64(&JsonValue::String(Some(token.into()))),
            expected,
            "{token:?}"
        );
    }
    // app_settings.rs:1444:21 allow_plus -> true: '+' is allowed only inside a JSON string.
    // The public DOM can be constructed directly; a number token still obeys the number grammar.
    assert_eq!(json::to_i64(&JsonValue::Number("+42".into())), None);
}

#[test]
fn json_serde_adapter_preserves_unsigned_numbers_and_rejects_unpaired_strings() {
    // app_settings.rs:1568 replace to_serde_value with Ok(Default::default()).
    // C#: enum converters inspect the JsonElement token kind; W2-01 adapter contract.
    for token in ["-9223372036854775808", "18446744073709551615", "1.25", "1e2"] {
        let value = json::parse(token).unwrap();
        assert_eq!(
            json::to_serde_value(&value).unwrap(),
            serde_json::from_str::<serde_json::Value>(token).unwrap()
        );
    }
    assert_eq!(
        json::to_serde_value(&JsonValue::Bool(true)).unwrap(),
        serde_json::Value::Bool(true)
    );
    assert_eq!(
        json::to_serde_value(&JsonValue::String(Some("text".into()))).unwrap(),
        "text"
    );
    assert!(json::to_serde_value(&JsonValue::String(None)).is_err());
    assert_eq!(
        json::to_serde_value(&json::parse("[1]").unwrap()).unwrap(),
        serde_json::json!([])
    );
    assert_eq!(
        json::to_serde_value(&json::parse(r#"{"a":1}"#).unwrap()).unwrap(),
        serde_json::json!({})
    );
    assert_eq!(
        json::to_serde_value(&json::parse("1e999").unwrap()).unwrap(),
        serde_json::Value::Null
    );
}
