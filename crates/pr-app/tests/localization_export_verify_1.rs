//! Adversarial verification of W8-04b (oracle tests run only when the .NET oracle env vars are set).
use pr_app::localization::export::build_json_with_help;
use pr_l10n::catalog::LanguageCatalog;
use std::collections::BTreeMap;

fn cat(json: &str) -> LanguageCatalog {
    let mut w = Vec::new();
    LanguageCatalog::try_parse(json, "t", &mut w).expect("parse")
}

#[test]
fn escape_set_matches_dotnet_unsafe_relaxed_for_every_bmp_char() {
    let Ok(path) = std::env::var("ESCAPED_TXT") else {
        return; // needs the .NET oracle dump
    };
    let text = std::fs::read_to_string(path).unwrap();
    let mut expected = std::collections::BTreeSet::new();
    for line in text.lines() {
        let (hex, _) = line.split_once(' ').unwrap();
        expected.insert(u32::from_str_radix(hex, 16).unwrap());
    }
    let english = cat(r#"{"_meta":{"code":"en","name":"E","nativeName":"E"},"k":"v"}"#);
    let mut diffs = Vec::new();
    for c in 0u32..=0xFFFF {
        let Some(ch) = char::from_u32(c) else { continue };
        let value = ch.to_string();
        let json = format!(
            "{{\"_meta\":{{\"code\":\"xx\",\"name\":\"X\",\"nativeName\":\"X\"}},\"k\":{}}}",
            serde_json::to_string(&value).unwrap()
        );
        let tr = cat(&json);
        let out = build_json_with_help(&english, "xx", &[tr], &BTreeMap::new(), "h");
        let line = out.lines().find(|l| l.starts_with("  \"k\": ")).unwrap();
        let rust_escaped = line.chars().nth(8) == Some(92u8 as char);
        // text of the entry may be normalized by the catalog parser; only compare when the value round-trips
        if !line.contains(&value) && !rust_escaped {
            continue;
        }
        if rust_escaped != expected.contains(&c) {
            diffs.push(format!(
                "U+{c:04X} rust_escaped={rust_escaped} dotnet_escaped={}",
                expected.contains(&c)
            ));
        }
    }
    assert!(
        diffs.is_empty(),
        "{} differences, first: {:?}",
        diffs.len(),
        &diffs[..diffs.len().min(15)]
    );
}

#[test]
fn read_notes_matches_dotnet_on_battery() {
    let Ok(dir) = std::env::var("NOTES_DIR") else {
        return; // needs the .NET oracle battery
    };
    let dir = std::path::PathBuf::from(dir);
    let mut diffs = Vec::new();
    let mut i = 0;
    while dir.join(format!("{i}.bin")).exists() {
        let notes = pr_app::localization::export::read_notes(&dir.join(format!("{i}.bin")));
        let rust = serde_json::to_string(&notes).unwrap();
        let cs = std::fs::read_to_string(dir.join(format!("{i}.out"))).unwrap();
        let bytes = std::fs::read(dir.join(format!("{i}.bin"))).unwrap();
        if rust != cs {
            diffs.push(format!(
                "case {i}: {:?} rust={rust} cs={cs}",
                String::from_utf8_lossy(&bytes[..bytes.len().min(70)])
            ));
        }
        i += 1;
    }
    assert!(diffs.is_empty(), "{diffs:#?}");
}

fn notes_of(text: &[u8]) -> BTreeMap<String, String> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("verify-w8-04b-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("n{}.json", text.len()));
    std::fs::write(&p, text).unwrap();
    let r = pr_app::localization::export::read_notes(&p);
    let _ = std::fs::remove_dir_all(&dir);
    r
}

#[test]
fn diff_nbsp_is_escaped_by_dotnet_unsafe_relaxed() {
    let english = cat(r#"{"_meta":{"code":"en","name":"E","nativeName":"E"},"k":"v"}"#);
    let tr =
        cat("{\"_meta\":{\"code\":\"xx\",\"name\":\"X\",\"nativeName\":\"X\"},\"k\":\"a\u{a0}b\u{3000}c\u{e000}\"}");
    let out = build_json_with_help(&english, "xx", &[tr], &BTreeMap::new(), "h");
    assert!(out.contains(r#""a\u00A0b\u3000c\uE000""#), "{out}");
}

#[test]
fn diff_line_comment_ends_at_cr() {
    assert_eq!(notes_of(b"// c\r{\"a\":\"1\"}").get("a").map(String::as_str), Some("1"));
}

#[test]
fn diff_comment_between_name_and_colon_is_rejected() {
    assert!(notes_of(b"{\"a\" /*c*/ : \"1\"}").is_empty());
}
