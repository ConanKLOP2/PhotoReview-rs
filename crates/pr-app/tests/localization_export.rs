//! Port tests for W8-04b: `TranslationExport` (I18N L08 "Export strings to translate"), C# `PhotoReview.App/Localization/
//! TranslationExport.cs` at baseline d92d6aae. C# twins are marked `// C#: TranslationExportTests.<Method>`, the tests with
//! no C# twin are marked `// new:`. The golden-driven test replays `tests/golden/translation-export.json` (written by the
//! oracle, never edited here). Red tests: the port panics via `todo!()` until the bodies are implemented.
//!
//! `free_backup_path` and `read_catalogs` are `pub(crate)`, so the backup numbering and the catalog file rules are
//! checked through [`export`], which is their only caller.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_app::localization::export::{
    BACKUP_SUFFIX, MISSING_KEY, NOTES_FILE_NAME, TranslationExportError, build_json_with_help, export, read_notes,
};
use pr_core::io::physical::PhysicalFileSystem;
use pr_l10n::catalog::{LanguageCatalog, MAX_FILE_BYTES};
use pr_l10n::keys;
use pr_l10n::loader::LanguageLoader;
use pr_l10n::localizer::{Localizer, built_in_english, built_in_english_localizer};
use pr_test_support::{TempRoot, golden};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------------------------------

/// `TranslationExportTests.Catalog(json)`: `LanguageCatalog.TryParse` must succeed.
fn parse_catalog(json: &str) -> LanguageCatalog {
    let mut warnings = Vec::new();
    LanguageCatalog::try_parse(json, "test", &mut warnings)
        .unwrap_or_else(|| panic!("catalog did not parse: {warnings:?}"))
}

/// `Tr.ExportHelp` in English: the tests pass it to `build_json_with_help` so they do not depend on the ambient language.
fn english_help() -> String {
    built_in_english_localizer().get(keys::EXPORT_HELP)
}

/// `TranslationExport.BuildJson(BuiltInCatalog.English, code, translations, notes)` with the English help text.
fn build(translations: &[LanguageCatalog], code: &str, notes: &BTreeMap<String, String>) -> String {
    build_json_with_help(built_in_english(), code, translations, notes, &english_help())
}

/// `JsonDocument.Parse(text).RootElement` of the export output.
fn parse_root(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("export output is not JSON: {e}\n{text}"))
}

/// Top-level member names in DOCUMENT order. The writer puts one top-level member per line, indented by two spaces;
/// `serde_json::Value` would sort them, so the order checks read the text.
fn top_level_keys(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  \"")?;
            let end = rest.find('"')?;
            Some(rest[..end].to_owned())
        })
        .collect()
}

/// Text keys (members not starting with `_`) in document order: `JsonElement.EnumerateObject` without `_` names.
fn text_keys(text: &str) -> Vec<String> {
    top_level_keys(text)
        .into_iter()
        .filter(|k| !k.starts_with('_'))
        .collect()
}

/// Sorted by UTF-16 code units (T2: not `String` byte order).
fn utf16_sorted(mut keys: Vec<String>) -> Vec<String> {
    keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    keys
}

/// `BuiltInCatalog.English.Entries.Keys` filtered and sorted (`StringComparer.Ordinal`).
fn english_keys_where(pred: impl Fn(&str) -> bool) -> Vec<String> {
    utf16_sorted(
        built_in_english()
            .entries()
            .iter()
            .map(|(k, _)| k.clone())
            .filter(|k| pred(k.as_str()))
            .collect(),
    )
}

fn english_has(key: &str) -> bool {
    built_in_english().entry(key).is_some()
}

fn english_text(key: &str) -> String {
    built_in_english()
        .entry(key)
        .unwrap_or_else(|| panic!("no English key {key}"))
        .to_owned()
}

fn english_count() -> usize {
    built_in_english().entries().len()
}

/// `root[_missing]` as strings, in order.
fn missing_list(root: &Value) -> Vec<String> {
    root[MISSING_KEY]
        .as_array()
        .expect("_missing is an array")
        .iter()
        .map(|v| v.as_str().expect("_missing entry is a string").to_owned())
        .collect()
}

/// `root[key]` as a string.
fn str_at<'a>(root: &'a Value, key: &str) -> &'a str {
    root[key].as_str().unwrap_or_else(|| panic!("no string member {key}"))
}

fn write_text(path: &Path, text: &str) {
    fs::write(path, text).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// `path + suffix` on the OS string (`xx.todo.json` + `.bak` = `xx.todo.json.bak`).
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Writes a folder from a golden map: a string is raw file text, any other value is serialized JSON, `null` is skipped.
fn write_files(dir: &Path, files: &Value) {
    fs::create_dir_all(dir).unwrap_or_else(|e| panic!("create {}: {e}", dir.display()));
    let Some(map) = files.as_object() else {
        return;
    };
    for (name, body) in map {
        let text = match body {
            Value::Null => continue,
            Value::String(raw) => raw.clone(),
            other => other.to_string(),
        };
        write_text(&dir.join(name), &text);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// build_json (C# TranslationExportTests, BuildJson_*)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
// C#: TranslationExportTests.BuildJson_ListsEveryKey_MissingFirstInEnglish_ThenCurrentTranslations_MetaAndNotes
fn build_json_lists_every_key_missing_first_in_english_then_current_translations_meta_and_notes() {
    let partial = parse_catalog(
        r#"{ "_meta": { "code": "xx", "name": "Test", "nativeName": "Testish", "plural": "one-other", "authors": ["A"] },
          "app.title": "Tst", "common.cancel": "Nope" }"#,
    );
    let mut notes = BTreeMap::new();
    notes.insert("common.save".to_owned(), "Save button.".to_owned());
    notes.insert("app.title".to_owned(), "Product name.".to_owned());

    let text = build(&[partial], "xx", &notes);
    let root = parse_root(&text);

    let meta = &root["_meta"];
    assert_eq!(meta["code"].as_str(), Some("xx"));
    assert_eq!(meta["nativeName"].as_str(), Some("Testish"));
    assert_eq!(meta["plural"].as_str(), Some("one-other"));
    assert_eq!(meta["authors"].as_array().map(Vec::len), Some(1));
    assert_eq!(meta["authors"][0].as_str(), Some("A"));

    // Every English key once: the untranslated ones (sorted) first, then the translated ones (sorted).
    let missing = english_keys_where(|k| k != "app.title" && k != "common.cancel");
    let mut expected_order = missing.clone();
    expected_order.push("app.title".to_owned());
    expected_order.push("common.cancel".to_owned());
    assert_eq!(text_keys(&text), expected_order);
    assert_eq!(missing_list(&root), missing);
    assert_eq!(str_at(&root, "common.save"), english_text("common.save"));
    assert_eq!(str_at(&root, "app.title"), "Tst");
    assert_eq!(str_at(&root, "common.cancel"), "Nope");

    // Notes for translated keys too, not only for missing ones.
    let exported = root["_notes"].as_object().expect("_notes is an object");
    assert_eq!(exported["common.save"].as_str(), Some("Save button."));
    assert_eq!(exported["app.title"].as_str(), Some("Product name."));
    assert_eq!(exported.len(), 2);
    assert!(top_level_keys(&text).iter().any(|k| k == "_help"));
}

#[test]
// C#: TranslationExportTests.BuildJson_English_ListsEveryKeyWithItsText_NothingMissing
fn build_json_english_lists_every_key_with_its_text_nothing_missing() {
    let text = build(&[], "en", &BTreeMap::new());
    let root = parse_root(&text);

    assert_eq!(text_keys(&text), english_keys_where(|_| true));
    assert!(missing_list(&root).is_empty());
    assert_eq!(str_at(&root, "common.save"), english_text("common.save"));
    assert_eq!(root["_meta"]["code"].as_str(), Some("en"));
}

#[test]
// C#: TranslationExportTests.BuildJson_English_UserOverrideReplacesBuiltInText
fn build_json_english_user_override_replaces_built_in_text() {
    let user = parse_catalog(r#"{ "_meta": { "code": "en" }, "common.save": "Keep" }"#);

    let text = build(&[user], "en", &BTreeMap::new());
    let root = parse_root(&text);

    assert_eq!(str_at(&root, "common.save"), "Keep");
    assert_eq!(text_keys(&text).len(), english_count());
}

#[test]
// C#: TranslationExportTests.BuildJson_PluralNone_SkipsUntranslatedOneForms_KeepsTranslatedOnes
fn build_json_plural_none_skips_untranslated_one_forms_keeps_translated_ones() {
    let one_keys = english_keys_where(|k| {
        k.strip_suffix(".one")
            .is_some_and(|base| english_has(&format!("{base}.other")))
    });
    assert!(one_keys.len() >= 2, "English must have plural pairs for this test");
    let kept = one_keys[0].clone();
    let translation_json = serde_json::json!({
        "_meta": { "code": "xx", "plural": "none" },
        (kept.as_str()): "One"
    })
    .to_string();
    let translation = parse_catalog(&translation_json);

    let text = build(&[translation], "xx", &BTreeMap::new());
    let root = parse_root(&text);

    let keys = text_keys(&text);
    for skipped in &one_keys[1..] {
        assert!(
            !keys.contains(skipped),
            "untranslated one-form {skipped} must be skipped"
        );
    }
    assert_eq!(str_at(&root, &kept), "One");
    assert!(!missing_list(&root).contains(&kept));
    // The `.other` form next to the skipped `.one` is still work to do.
    let second_base = one_keys[1].strip_suffix(".one").expect("one key");
    assert!(missing_list(&root).contains(&format!("{second_base}.other")));
}

#[test]
// C#: TranslationExportTests.BuildJson_PluralOneOther_ListsUntranslatedOneFormsAsMissing
fn build_json_plural_one_other_lists_untranslated_one_forms_as_missing() {
    let one_key = english_keys_where(|k| k.ends_with(".one"))[0].clone();
    let translation = parse_catalog(r#"{ "_meta": { "code": "xx", "plural": "one-other" } }"#);

    let text = build(&[translation], "xx", &BTreeMap::new());
    let root = parse_root(&text);

    assert!(missing_list(&root).contains(&one_key));
}

#[test]
// C#: TranslationExportTests.BuildJson_DropsKeysEnglishDoesNotKnow
fn build_json_drops_keys_english_does_not_know() {
    let translation = parse_catalog(r#"{ "_meta": { "code": "xx" }, "no.such.key": "x", "app.title": "T" }"#);

    let text = build(&[translation], "xx", &BTreeMap::new());
    let root = parse_root(&text);

    assert!(!text_keys(&text).iter().any(|k| k == "no.such.key"));
    assert_eq!(str_at(&root, "app.title"), "T");
}

#[test]
// C#: TranslationExportTests.BuildJson_OutputIsAValidCatalogForTheSameLanguage
fn build_json_output_is_a_valid_catalog_for_the_same_language() {
    let translation = parse_catalog(r#"{ "_meta": { "code": "xx" }, "app.title": "T" }"#);
    let json = build(&[translation], "xx", &BTreeMap::new());

    // Renaming the file to xx.json must load cleanly.
    let catalog = parse_catalog(&json);
    assert_eq!(catalog.code(), "xx");
    let localizer = Localizer::create(built_in_english(), std::slice::from_ref(&catalog), &[]);
    assert!(localizer.warnings().is_empty(), "warnings: {:?}", localizer.warnings());
    assert_eq!(catalog.entry("app.title"), Some("T")); // the existing translation survives the round trip
    assert_eq!(catalog.entries().len(), english_count());
}

// ---------------------------------------------------------------------------------------------------------------------
// export (C# TranslationExportTests, Export_*)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
// C#: TranslationExportTests.Export_MergesShippedAndUserFiles_WritesTodoFileTheLoaderIgnores
fn export_merges_shipped_and_user_files_writes_todo_file_the_loader_ignores() {
    let temp = TempRoot::new("translation-export");
    let shipped = temp.join("shipped");
    let user = temp.join("user").join("Languages"); // does not exist yet
    fs::create_dir_all(&shipped).expect("create shipped");
    write_text(
        &shipped.join("xx.json"),
        r#"{ "_meta": { "code": "xx", "nativeName": "Shipped" }, "app.title": "T" }"#,
    );
    write_text(
        &shipped.join(NOTES_FILE_NAME),
        r#"{ "_meta": {}, "common.save": "Save button." }"#,
    );
    write_text(
        &shipped.join("yy.json"),
        r#"{ "_meta": { "code": "yy" }, "common.save": "other language" }"#,
    );
    fs::create_dir_all(&user).expect("create user");
    write_text(
        &user.join("xx.json"),
        r#"{ "_meta": { "code": "xx", "nativeName": "Mine" }, "common.cancel": "C", "app.title": "U" }"#,
    );

    let path = export("xx", &shipped, &user).expect("export");

    assert_eq!(path, user.join("xx.todo.json"));
    let bytes = fs::read(&path).expect("read export");
    assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "UTF-8 without BOM");
    let text = String::from_utf8(bytes).expect("export is UTF-8");
    let root = parse_root(&text);
    let missing = missing_list(&root);
    assert_eq!(text_keys(&text).len(), english_count());
    assert!(!missing.contains(&"app.title".to_owned())); // translated in both files
    assert!(!missing.contains(&"common.cancel".to_owned())); // translated in the user file
    assert!(missing.contains(&"common.save".to_owned())); // a translation of another language does not count
    assert_eq!(str_at(&root, "app.title"), "U"); // user text wins over shipped
    assert_eq!(str_at(&root, "common.cancel"), "C");
    assert_eq!(str_at(&root, "common.save"), english_text("common.save"));
    assert_eq!(root["_meta"]["nativeName"].as_str(), Some("Mine")); // user file wins
    assert_eq!(root["_notes"]["common.save"].as_str(), Some("Save button."));

    // The dotted name keeps the half-done file out of the language list and out of the loaded text.
    let loader = LanguageLoader::new(Arc::new(PhysicalFileSystem), shipped.clone(), Some(user.clone()));
    let xx_count = loader.discover_languages().iter().filter(|l| l.code == "xx").count();
    assert_eq!(xx_count, 1);
    let loaded = loader.load(Some("xx"), None);
    assert_eq!(loaded.get("app.title"), "U");
    assert_eq!(loaded.get("common.save"), english_text("common.save"));
}

#[test]
// C#: TranslationExportTests.Export_OverwritesAnEarlierExport
fn export_overwrites_an_earlier_export() {
    let temp = TempRoot::new("translation-export");
    let shipped = temp.join("shipped2");
    let user = temp.join("user2");
    fs::create_dir_all(&user).expect("create user");
    write_text(&user.join("xx.todo.json"), "stale");

    let path = export("xx", &shipped, &user).expect("export");

    assert!(read_text(&path).starts_with('{'));
}

#[test]
// C#: TranslationExportTests.Export_KeepsTheEarlierExportAsBackup_SoHandEditsAreNotLost
fn export_keeps_the_earlier_export_as_backup_so_hand_edits_are_not_lost() {
    let temp = TempRoot::new("translation-export");
    let shipped = temp.join("shipped4");
    let user = temp.join("user4");
    fs::create_dir_all(&user).expect("create user");
    let existing = user.join("xx.todo.json");
    write_text(&existing, "my half-done translation");

    let path = export("xx", &shipped, &user).expect("export");

    assert_eq!(path, existing);
    assert_eq!(
        read_text(&with_suffix(&existing, BACKUP_SUFFIX)),
        "my half-done translation"
    );
    assert!(read_text(&path).starts_with('{'));
}

#[test]
// C#: TranslationExportTests.Export_SecondExport_KeepsHandEditsInAnotherBackup
fn export_second_export_keeps_hand_edits_in_another_backup() {
    let temp = TempRoot::new("translation-export");
    let shipped = temp.join("shipped5");
    let user = temp.join("user5");
    fs::create_dir_all(&user).expect("create user");
    let existing = user.join("xx.todo.json");
    write_text(&existing, "first hand edit");
    export("xx", &shipped, &user).expect("first export");
    write_text(&existing, "second hand edit");

    export("xx", &shipped, &user).expect("second export");

    assert_eq!(read_text(&with_suffix(&existing, BACKUP_SUFFIX)), "first hand edit");
    assert_eq!(
        read_text(&with_suffix(&existing, &format!("{BACKUP_SUFFIX}2"))),
        "second hand edit"
    );
}

#[test]
// C#: TranslationExportTests.Export_UnchangedFile_NeedsNoBackup
fn export_unchanged_file_needs_no_backup() {
    let temp = TempRoot::new("translation-export");
    let shipped = temp.join("shipped6");
    let user = temp.join("user6");
    let path = export("xx", &shipped, &user).expect("first export");

    export("xx", &shipped, &user).expect("second export");

    assert!(!with_suffix(&path, BACKUP_SUFFIX).exists());
}

#[test]
// C#: TranslationExportTests.Export_RejectsInvalidCode
fn export_rejects_invalid_code() {
    let temp = TempRoot::new("translation-export");

    let result = export("..\\evil", &temp.join("s3"), &temp.join("u3"));

    match result {
        Err(TranslationExportError::InvalidCode(code)) => assert_eq!(code, "..\\evil"),
        other => panic!("expected InvalidCode, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// new tests (no C# twin)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
// new: the file is UTF-8 without BOM, LF only, exactly one trailing newline (rule T12).
fn build_json_ends_with_one_newline_and_no_bom() {
    let text = build(&[], "xx", &BTreeMap::new());

    assert!(
        text.ends_with("}\n"),
        "the text must end with a newline after the closing brace"
    );
    assert!(!text.ends_with("}\n\n"), "exactly one trailing newline");
    assert!(!text.as_bytes().starts_with(&[0xEF, 0xBB, 0xBF]), "no BOM");
    assert!(!text.contains('\r'), "LF newlines only");
}

#[test]
// new: T1 and T11. The backup name appends `.bak` to the whole file name (`xx.todo.json.bak`, never `xx.todo.bak` or
// `xx.bak`) and the next free name is `.bak2`, `.bak3`, ... (there is no `.bak1`). Checked through `export`, since
// `free_backup_path` is `pub(crate)`.
fn free_backup_path_appends_suffix_and_never_replaces_json() {
    let temp = TempRoot::new("translation-export");
    let shipped = temp.join("shipped");
    let user = temp.join("user");
    fs::create_dir_all(&user).expect("create user");
    let existing = user.join("xx.todo.json");

    write_text(&existing, "one");
    export("xx", &shipped, &user).expect("export 1");
    write_text(&existing, "two");
    export("xx", &shipped, &user).expect("export 2");
    write_text(&existing, "three");
    export("xx", &shipped, &user).expect("export 3");

    assert_eq!(read_text(&user.join("xx.todo.json.bak")), "one");
    assert_eq!(read_text(&user.join("xx.todo.json.bak2")), "two");
    assert_eq!(read_text(&user.join("xx.todo.json.bak3")), "three");
    assert!(
        !user.join("xx.todo.bak").exists(),
        "the .json suffix must not be replaced"
    );
    assert!(!user.join("xx.bak").exists(), "the .json suffix must not be replaced");
}

#[test]
// new: T13. Blank `code` (empty or whitespace) and blank `user_dir` are errors, checked before any file access.
fn export_blank_arguments_are_errors_and_write_nothing() {
    let temp = TempRoot::new("translation-export");
    let shipped = temp.join("shipped");
    let user = temp.join("user");

    assert!(matches!(
        export("", &shipped, &user),
        Err(TranslationExportError::BlankArgument("code"))
    ));
    assert!(matches!(
        export("   ", &shipped, &user),
        Err(TranslationExportError::BlankArgument("code"))
    ));
    assert!(matches!(
        export("xx", &shipped, Path::new("   ")),
        Err(TranslationExportError::BlankArgument("user_dir"))
    ));
    assert!(matches!(
        export("xx", &shipped, Path::new("")),
        Err(TranslationExportError::BlankArgument("user_dir"))
    ));
    assert!(!user.exists());
    assert_eq!(
        fs::read_dir(temp.path()).expect("list temp").count(),
        0,
        "nothing written"
    );
}

#[test]
// new: T13. An invalid code is refused before the user folder is created.
fn export_invalid_code_creates_no_folder() {
    let temp = TempRoot::new("translation-export");
    let shipped = temp.join("shipped");
    let user = temp.join("user");

    match export("1ab", &shipped, &user) {
        Err(TranslationExportError::InvalidCode(code)) => assert_eq!(code, "1ab"),
        other => panic!("expected InvalidCode, got {other:?}"),
    }
    assert!(!user.exists(), "user folder must not be created for an invalid code");
}

#[test]
// new: T7. A user folder that is a file is an `Io` error, and nothing else is written.
fn export_fails_with_io_when_user_dir_is_a_file() {
    let temp = TempRoot::new("translation-export");
    let shipped = temp.join("shipped");
    let user = temp.join("user");
    write_text(&user, "not a folder");

    let result = export("xx", &shipped, &user);

    assert!(
        matches!(result, Err(TranslationExportError::Io(_))),
        "expected Io, got {result:?}"
    );
    assert_eq!(read_text(&user), "not a folder");
    assert_eq!(
        fs::read_dir(temp.path()).expect("list temp").count(),
        1,
        "only the user file exists"
    );
}

#[test]
// new: items 8 to 10 of the contract (`read_catalogs`), checked through `export`: a dotted name, a broken file and an
// oversized file are skipped, a file of another code is skipped, and the one valid `xx.json` is read.
fn read_catalogs_skips_dotted_broken_and_oversized_files() {
    let temp = TempRoot::new("translation-export");
    let shipped = temp.join("shipped");
    let user = temp.join("user");
    fs::create_dir_all(&shipped).expect("create shipped");
    fs::create_dir_all(&user).expect("create user");
    write_text(
        &shipped.join("xx.json"),
        r#"{ "_meta": { "code": "xx" }, "app.title": "T" }"#,
    );
    write_text(
        &shipped.join("xx.draft.json"),
        r#"{ "_meta": { "code": "xx" }, "common.save": "DRAFT" }"#,
    );
    write_text(
        &shipped.join("aa.json"),
        r#"{ "_meta": { "code": "xx" }, "app.title": "BROKEN""#,
    );
    write_text(
        &shipped.join("yy.json"),
        r#"{ "_meta": { "code": "yy" }, "common.cancel": "YY" }"#,
    );
    let mut oversized = br#"{ "_meta": { "code": "xx" }, "common.cancel": "BIG" }"#.to_vec();
    oversized.resize((MAX_FILE_BYTES + 1) as usize, b' ');
    fs::write(shipped.join("big.json"), oversized).expect("write oversized catalog");

    let path = export("xx", &shipped, &user).expect("a broken or oversized file must not fail the export");

    let root = parse_root(&read_text(&path));
    assert_eq!(str_at(&root, "app.title"), "T");
    assert_eq!(
        str_at(&root, "common.save"),
        english_text("common.save"),
        "dotted draft is not read"
    );
    assert_eq!(
        str_at(&root, "common.cancel"),
        english_text("common.cancel"),
        "oversized and other-code files are not read"
    );
    assert!(missing_list(&root).contains(&"common.cancel".to_owned()));
}

#[test]
// new: item 11 of the contract (`read_notes`): a missing file, a file longer than MAX_FILE_BYTES and a folder named
// like the notes file all give an EMPTY map, never an error.
fn read_notes_missing_and_oversized_files_are_empty() {
    let temp = TempRoot::new("translation-export");

    assert!(read_notes(&temp.join(NOTES_FILE_NAME)).is_empty(), "missing file");

    let big_dir = temp.join("big");
    fs::create_dir_all(&big_dir).expect("create big");
    let mut oversized = br#"{ "common.save": "Save button." }"#.to_vec();
    oversized.resize((MAX_FILE_BYTES + 1) as usize, b' ');
    fs::write(big_dir.join(NOTES_FILE_NAME), oversized).expect("write oversized notes");
    assert!(read_notes(&big_dir.join(NOTES_FILE_NAME)).is_empty(), "oversized file");

    let folder = temp.join("folder");
    fs::create_dir_all(folder.join(NOTES_FILE_NAME)).expect("create folder named like the notes file");
    assert!(
        read_notes(&folder.join(NOTES_FILE_NAME)).is_empty(),
        "not a regular file"
    );
}

#[test]
// new: an identical rewrite keeps the same bytes and creates no backup (the byte comparison of the twin).
fn export_identical_rewrite_creates_no_backup() {
    let temp = TempRoot::new("translation-export");
    let shipped = temp.join("shipped");
    let user = temp.join("user");

    let path = export("xx", &shipped, &user).expect("first export");
    let first = fs::read(&path).expect("read first export");
    let again = export("xx", &shipped, &user).expect("second export");
    let second = fs::read(&again).expect("read second export");

    assert_eq!(again, path);
    assert_eq!(first, second, "identical rewrite keeps the same bytes");
    assert!(
        !with_suffix(&path, BACKUP_SUFFIX).exists(),
        "no backup for an unchanged text"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// golden: tests/golden/translation-export.json (39 cases: 14 build, 7 export, 12 read_notes, 6 invalid_code)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
// new: golden. Every case of the oracle run is replayed byte for byte.
fn golden_translation_export() {
    let golden = golden::load("translation-export");
    assert_eq!(golden.csharp_commit, "d92d6aae639a40eede232948f9bce97ecfd02c10");
    assert_eq!(golden.cases.len(), 39);

    for case in &golden.cases {
        let id = golden::str_of(case, "id");
        match golden::str_of(&case["input"], "kind") {
            "build" => check_build_case(case),
            "export" => check_export_case(case),
            "read_notes" => check_read_notes_case(case),
            "invalid_code" => check_invalid_code_case(case),
            other => panic!("{id}: unknown kind {other}"),
        }
    }
}

fn check_build_case(case: &Value) {
    let id = golden::str_of(case, "id");
    let input = &case["input"];

    let english_owned = if input["english"].is_null() {
        None
    } else {
        Some(parse_catalog(&input["english"].to_string()))
    };
    let english: &LanguageCatalog = english_owned.as_ref().unwrap_or_else(|| built_in_english());

    // A translation that is a string is raw text; one that does not parse is dropped, as in C#.
    let mut translations = Vec::new();
    for entry in input["translations"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let json = match entry {
            Value::String(raw) => raw.clone(),
            other => other.to_string(),
        };
        let mut warnings = Vec::new();
        if let Some(catalog) = LanguageCatalog::try_parse(&json, "golden", &mut warnings) {
            translations.push(catalog);
        }
    }

    let notes: BTreeMap<String, String> = input["notes"]
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(k, v)| (k.clone(), v.as_str().expect("note text").to_owned()))
                .collect()
        })
        .unwrap_or_default();

    let code = golden::str_of(input, "code");
    let actual = build_json_with_help(english, code, &translations, &notes, &english_help());
    assert_eq!(actual, golden::str_of(case, "output"), "{id}");
}

fn check_export_case(case: &Value) {
    let id = golden::str_of(case, "id");
    let input = &case["input"];
    let temp = TempRoot::new("translation-export");
    let shipped = temp.join("shipped");
    let user = temp.join("user");

    write_files(&shipped, &input["shipped"]);
    if !input["user_missing"].as_bool().unwrap_or(false) {
        write_files(&user, &input["user"]);
    }

    let code = golden::str_of(input, "code");
    let runs = input["runs"].as_u64().unwrap_or(1);
    let mut last_path = None;
    for run in 0..runs {
        if run > 0 && !input["between_runs"].is_null() {
            write_files(&user, &input["between_runs"]);
        }
        let path = export(code, &shipped, &user).unwrap_or_else(|e| panic!("{id}: export failed: {e}"));
        last_path = Some(path);
    }
    let path = last_path.expect("runs >= 1");

    let relative = path
        .strip_prefix(&user)
        .unwrap_or_else(|_| panic!("{id}: {} is not inside the user folder", path.display()))
        .to_string_lossy()
        .into_owned();
    assert_eq!(relative, golden::str_of(case, "path"), "{id}");

    let mut files = BTreeMap::new();
    for entry in fs::read_dir(&user).expect("list user folder") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        files.insert(name, read_text(&entry.path()));
    }
    let expected: BTreeMap<String, String> = case["files"]
        .as_object()
        .expect("files object")
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().expect("file text").to_owned()))
        .collect();
    assert_eq!(files, expected, "{id}");
}

fn check_read_notes_case(case: &Value) {
    let id = golden::str_of(case, "id");
    let input = &case["input"];
    let temp = TempRoot::new("translation-export");
    let path = temp.join(NOTES_FILE_NAME);

    if !input["missing"].as_bool().unwrap_or(false) {
        let mut bytes = golden::str_of(input, "text").as_bytes().to_vec();
        if let Some(size) = input["pad_to_bytes"].as_u64() {
            if (bytes.len() as u64) < size {
                bytes.resize(size as usize, b' ');
            }
        }
        fs::write(&path, bytes).expect("write notes file");
    }

    let notes = read_notes(&path);
    let expected: BTreeMap<String, String> = case["notes"]
        .as_object()
        .expect("notes object")
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().expect("note text").to_owned()))
        .collect();
    assert_eq!(notes, expected, "{id}");
}

fn check_invalid_code_case(case: &Value) {
    let id = golden::str_of(case, "id");
    let input = &case["input"];
    assert_eq!(golden::str_of(case, "threw"), "ArgumentException", "{id}");
    let temp = TempRoot::new("translation-export");
    let user = temp.join("user");

    let result = export(golden::str_of(input, "code"), &temp.join("shipped"), &user);

    assert!(
        matches!(
            &result,
            Err(TranslationExportError::BlankArgument(_) | TranslationExportError::InvalidCode(_))
        ),
        "{id}: expected an argument error, got {result:?}"
    );
    let expected_created = case["user_dir_created"].as_bool().expect("user_dir_created bool");
    assert_eq!(user.exists(), expected_created, "{id}");
}
