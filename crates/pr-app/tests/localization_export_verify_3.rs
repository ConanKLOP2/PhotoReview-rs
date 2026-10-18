//! Adversarial verification of W8-04b: escape table vs .NET, and the differences found.
use pr_app::localization::export::{build_json_with_help, export, read_notes};
use pr_l10n::catalog::LanguageCatalog;
use std::collections::BTreeMap;

fn cat(json: &str) -> LanguageCatalog {
    let mut w = Vec::new();
    LanguageCatalog::try_parse(json, "t", &mut w).expect("parse")
}

fn tmp(label: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("verify-w8-04b-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn escape_of_every_scalar_matches_dotnet_exactly() {
    let Ok(path) = std::env::var("ESCAPED_ALL") else {
        return; // needs the .NET oracle dump
    };
    let text = std::fs::read_to_string(path).unwrap();
    let mut expected: BTreeMap<u32, String> = BTreeMap::new();
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        let (hex, body) = line.split_once(' ').unwrap();
        expected.insert(u32::from_str_radix(hex, 16).unwrap(), body.to_owned());
    }
    let english = cat(r#"{"_meta":{"code":"en","name":"E","nativeName":"E"},"k":"v"}"#);
    let mut diffs = Vec::new();
    // Batch characters: one _help per char, line-based.
    for c in 0u32..=0x10FFFF {
        let Some(ch) = char::from_u32(c) else { continue };
        let help = ch.to_string();
        let out = build_json_with_help(&english, "en", &[], &BTreeMap::new(), &help);
        let line = out.split('\n').find(|l| l.starts_with("  \"_help\": ")).unwrap_or("");
        // a raw newline never occurs (always escaped), so the line is complete
        let body = &line["  \"_help\": \"".len()..line.len() - 2];
        let want = expected.get(&c).cloned().unwrap_or_else(|| help.clone());
        if body != want {
            diffs.push(format!("U+{c:04X} rust={body:?} dotnet={want:?}"));
        }
    }
    assert!(
        diffs.is_empty(),
        "{} differences, first: {:#?}",
        diffs.len(),
        &diffs[..diffs.len().min(15)]
    );
}

#[test]
fn diff_line_comment_with_u2028_is_rejected_by_dotnet() {
    let d = tmp("c2028");
    let p = d.join("en.notes.json");
    std::fs::write(&p, "{\"a\":\"1\"}//x\u{2028}y").unwrap();
    // .NET JsonDocument.Parse throws JsonException (comment contains a line separator): no notes.
    assert!(read_notes(&p).is_empty(), "U+2028 in a // comment");
    std::fs::write(&p, "{\"a\":\"1\"}//x\u{2029}").unwrap();
    assert!(read_notes(&p).is_empty(), "U+2029 in a // comment");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn smoke_export_same_user_and_shipped_dir() {
    let d = tmp("same");
    std::fs::write(
        d.join("xx.json"),
        r#"{"_meta":{"code":"xx","name":"X","nativeName":"X"},"app.title":"T"}"#,
    )
    .unwrap();
    let p = export("xx", &d, &d).unwrap();
    assert!(p.exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn diff_utf16_bom_notes_file_is_decoded_by_file_read_all_text() {
    // C# File.ReadAllText detects a UTF-16 LE BOM; decoded like ReadAllText.
    let d = tmp("u16");
    let p = d.join("en.notes.json");
    let mut bytes = vec![0xFF, 0xFE];
    for u in "{\"a\":\"1\"}".encode_utf16() {
        bytes.extend(u.to_le_bytes());
    }
    std::fs::write(&p, bytes).unwrap();
    assert_eq!(read_notes(&p).get("a").map(String::as_str), Some("1"));
    let _ = std::fs::remove_dir_all(&d);
}

// ---- kill tests for the mutants that survived the committed tests ----

fn xx_catalog(tag: &str) -> String {
    format!("{{\"_meta\":{{\"code\":\"xx\",\"name\":\"N\",\"nativeName\":\"NN\"}},\"app.title\":\"{tag}\"}}")
}

fn export_text(shipped: &std::path::Path, user: &std::path::Path) -> String {
    let p = export("xx", shipped, user).unwrap();
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn kill_has_json_suffix_variants() {
    let d = tmp("suffix");
    let shipped = d.join("shipped");
    let user = d.join("user");
    std::fs::create_dir_all(&shipped).unwrap();
    // short names must not panic; a non-.json file with a valid catalog must be ignored
    std::fs::write(shipped.join("a"), xx_catalog("tagshortq")).unwrap();
    std::fs::write(shipped.join("ab"), xx_catalog("tagshort2q")).unwrap();
    std::fs::write(shipped.join("xx.txt"), xx_catalog("tagtxtq")).unwrap();
    std::fs::write(shipped.join("xx.json5"), xx_catalog("tagjson5q")).unwrap();
    std::fs::write(shipped.join("json"), xx_catalog("tagbareq")).unwrap();
    let text = export_text(&shipped, &user);
    for bad in ["tagshortq", "tagtxtq", "tagjson5q", "tagbareq"] {
        assert!(!text.contains(bad), "{bad} must be ignored");
    }
    // upper-case extension and a multi-byte name are catalogs
    std::fs::write(shipped.join("MULTI.JSON"), xx_catalog("tagupperq")).unwrap();
    let text = export_text(&shipped, &user);
    assert!(text.contains("tagupperq"));
    std::fs::remove_file(shipped.join("MULTI.JSON")).unwrap();
    std::fs::write(
        shipped.join("\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}.json"),
        xx_catalog("tagmultiq"),
    )
    .unwrap();
    let text = export_text(&shipped, &user);
    assert!(text.contains("tagmultiq"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn kill_backup_numbering_reaches_bak3_and_bak4() {
    let d = tmp("bak");
    let shipped = d.join("shipped");
    let user = d.join("user");
    std::fs::create_dir_all(&shipped).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    let todo = user.join("xx.todo.json");
    for (i, name) in [
        "xx.todo.json.bak",
        "xx.todo.json.bak2",
        "xx.todo.json.bak3",
        "xx.todo.json.bak4",
    ]
    .iter()
    .enumerate()
    {
        std::fs::write(&todo, format!("hand edit {i}")).unwrap();
        export("xx", &shipped, &user).unwrap();
        assert_eq!(
            std::fs::read_to_string(user.join(name)).unwrap(),
            format!("hand edit {i}"),
            "{name}"
        );
    }
    assert!(!user.join("xx.todo.json.bak5").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn kill_read_notes_exact_max_size_is_accepted() {
    let d = tmp("max");
    let p = d.join("en.notes.json");
    let body = "{\"a\":\"1\"}";
    let mut t = body.to_owned();
    t.push_str(&" ".repeat(1024 * 1024 - body.len()));
    assert_eq!(t.len() as u64, pr_l10n::catalog::MAX_FILE_BYTES);
    std::fs::write(&p, &t).unwrap();
    assert_eq!(
        read_notes(&p).len(),
        1,
        "exactly MAX_FILE_BYTES is accepted (C# uses >)"
    );
    t.push(' ');
    std::fs::write(&p, &t).unwrap();
    assert!(read_notes(&p).is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn kill_error_display_and_source() {
    use pr_app::localization::export::TranslationExportError as E;
    use std::error::Error;
    assert!(E::BlankArgument("code").to_string().contains("code"));
    assert!(E::InvalidCode("1ab".into()).to_string().contains("1ab"));
    let io = E::Io(std::io::Error::other("boom"));
    assert!(io.to_string().contains("boom"));
    assert!(io.source().is_some());
    assert!(E::InvalidCode("x".into()).source().is_none());
}

#[test]
fn diff_lone_surrogate_escape_in_skipped_values_does_not_invalidate_the_file() {
    // .NET: JsonDocument.Parse accepts an unpaired \uD800 escape; GetString (never called for skipped values) is the
    // only place that throws. So these files give {a: 1} in C#; Rust returns an empty map.
    let d = tmp("surr");
    let p = d.join("en.notes.json");
    for text in [
        r#"{"a":"1","b":["\ud800"]}"#,
        r#"{"a":"1","_b":"\ud800"}"#,
        r#"{"a":"1","b":{"c":"\ud800"}}"#,
        r#"{"a":"1","b":3,"_c":"\udc00"}"#,
    ] {
        std::fs::write(&p, text).unwrap();
        assert_eq!(read_notes(&p).get("a").map(String::as_str), Some("1"), "{text}");
    }
    let _ = std::fs::remove_dir_all(&d);
}
