//! Adversarial verification of W8-04b: kill tests for mutants that survived the committed tests.
use pr_app::localization::export::{export, read_notes};

fn tmp(label: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("verify-w8-04b-f-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

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
