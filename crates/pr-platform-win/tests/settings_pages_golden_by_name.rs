//! Lane `st-painter-tests` (ST-GAPMAP2 section 6, task 9): loads the golden of EACH Settings page BY NAME (`settings-pages-diag-perf`, `settings-page-display`,
//! `-files`, `-general`, `-mouse`, `-shortcuts`) and compares the page declaration of the Rust page with the C# dump, case by case. The older `golden_*` tests in
//! `settings_pages_diag_perf.rs` / `settings_page_*.rs` build the page and call `dump` but never read the JSON, so a drift in ids, styles or texts passed unseen.
//!
//! Pure: no window, no GDI. Page declaration only needs a `SettingsPageBuilder`.

use std::collections::BTreeSet;

use pr_platform_win::dialogs::settings::builder::inputs::SettingsPageBuilder;
use pr_platform_win::dialogs::settings::page_view::SettingsPageView;
use pr_platform_win::dialogs::settings::pages::{
    diagnostics::DiagnosticsSettingsPage, display::DisplaySettingsPage, files::FilesSettingsPage,
    general::GeneralSettingsPage, mouse::MouseSettingsPage, performance::PerformanceSettingsPage,
    shortcuts::ShortcutsSettingsPage,
};
use pr_test_support::golden;
use serde_json::Value;

/// The only declaration case of the golden `name`.
fn declare_case(name: &str) -> Value {
    let golden = golden::load(name);
    assert_eq!(golden.csharp_commit.len(), 40, "{name}: baseline commit");
    let declared: Vec<&Value> = golden.cases.iter().filter(|c| c["kind"] == "declare").collect();
    assert_eq!(declared.len(), 1, "{name}: exactly one declare case");
    declared[0].clone()
}

/// The declaration case of page `page` in the golden `name` (a golden may hold several pages).
fn declare_case_of(name: &str, page: &str) -> Value {
    golden::load(name)
        .cases
        .into_iter()
        .find(|c| c["kind"] == "declare" && c["page"] == page)
        .unwrap_or_else(|| panic!("{name} has no declare case for {page}"))
}

fn ids(value: &Value, key: &str) -> Vec<i64> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("golden has no array '{key}'"))
        .iter()
        .map(|v| v.as_i64().unwrap_or_else(|| panic!("'{key}' holds a non-integer: {v}")))
        .collect()
}

fn as_vec(set: &BTreeSet<i32>) -> Vec<i64> {
    set.iter().map(|v| i64::from(*v)).collect()
}

/// Declares `page` with the golden's `id_base` and compares everything the golden records, collecting every difference before failing so one run shows them all.
/// `compare_texts` is false only to isolate the documented text deviation of a page (see the Shortcuts tests).
fn check_declaration(page: &mut dyn SettingsPageView, case: &Value, compare_texts: bool) {
    let page_name = golden::str_of(case, "page");
    let id_base = i32::try_from(golden::i64_of(case, "id_base")).expect("id_base");
    let mut builder = SettingsPageBuilder::new(id_base);
    page.declare(&mut builder);
    let mut diffs: Vec<String> = Vec::new();
    let mut note = |what: &str, ok: bool, detail: String| {
        if !ok {
            diffs.push(format!("{page_name}: {what}: {detail}"));
        }
    };

    note(
        "key",
        page.key() == golden::str_of(case, "key"),
        format!("{} vs {}", page.key(), golden::str_of(case, "key")),
    );
    note(
        "no builder error",
        builder.errors().is_empty(),
        format!("{:?}", builder.errors()),
    );
    note(
        "id_base",
        builder.id_base() == id_base,
        format!("{}", builder.id_base()),
    );

    // items: class, id, style, ex_style (and text when the page texts are compared)
    let items = case["items"].as_array().expect("items");
    note(
        "item count",
        builder.items().len() == items.len(),
        format!("{} vs {}", builder.items().len(), items.len()),
    );
    for (index, (rust, golden_item)) in builder.items().iter().zip(items).enumerate() {
        let rust_class = format!("{:?}", rust.class);
        note(
            &format!("item {index} class"),
            rust_class == golden_item["class"].as_str().unwrap_or("?"),
            format!("{rust_class} vs {}", golden_item["class"]),
        );
        note(
            &format!("item {index} id"),
            i64::from(rust.id) == golden_item["id"].as_i64().unwrap_or(-1),
            format!("{} vs {}", rust.id, golden_item["id"]),
        );
        note(
            &format!("item {index} (id {}) style", rust.id),
            i64::from(rust.style) == golden_item["style"].as_i64().unwrap_or(-1),
            format!("{:#x} vs {}", rust.style, golden_item["style"]),
        );
        note(
            &format!("item {index} (id {}) ex_style", rust.id),
            i64::from(rust.ex_style) == golden_item["ex_style"].as_i64().unwrap_or(-1),
            format!("{:#x} vs {}", rust.ex_style, golden_item["ex_style"]),
        );
        if compare_texts {
            note(
                &format!("item {index} (id {}) text", rust.id),
                Some(rust.text.as_str()) == golden_item["text"].as_str(),
                format!("{:?} vs {}", rust.text, golden_item["text"]),
            );
        }
    }

    // text providers (id -> text)
    let texts = case["texts"].as_array().expect("texts");
    note(
        "text provider count",
        builder.texts().len() == texts.len(),
        format!("{} vs {}", builder.texts().len(), texts.len()),
    );
    for entry in texts {
        let id = i32::try_from(entry["id"].as_i64().expect("text id")).expect("text id range");
        match builder.texts().get(&id) {
            None => note(&format!("text {id}"), false, "missing in Rust".into()),
            Some(provider) if compare_texts => {
                let rust = provider();
                note(
                    &format!("text {id}"),
                    Some(rust.as_str()) == entry["text"].as_str(),
                    format!("{rust:?} vs {}", entry["text"]),
                );
            }
            Some(_) => {}
        }
    }

    // id sets and counts
    note(
        "notes",
        as_vec(builder.notes()) == ids(case, "notes"),
        format!("{:?}", builder.notes()),
    );
    note(
        "key_boxes",
        as_vec(builder.key_boxes()) == ids(case, "key_boxes"),
        format!("{:?}", builder.key_boxes()),
    );
    note(
        "warnings",
        as_vec(builder.warning_texts()) == ids(case, "warnings"),
        format!("{:?}", builder.warning_texts()),
    );
    let golden_len = |key: &str| case[key].as_array().map_or(0, Vec::len);
    note(
        "combo list count",
        builder.combo_items().len() == golden_len("combo_items"),
        format!("{} vs {}", builder.combo_items().len(), golden_len("combo_items")),
    );
    note(
        "slider count",
        builder.sliders().len() == golden_len("sliders"),
        format!("{} vs {}", builder.sliders().len(), golden_len("sliders")),
    );
    note(
        "element tree top level",
        builder.elements().len() == golden_len("elements"),
        format!("{} vs {}", builder.elements().len(), golden_len("elements")),
    );

    assert!(
        diffs.is_empty(),
        "{} differences from the C# golden:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

#[test]
fn the_diagnostics_page_matches_the_golden_settings_pages_diag_perf() {
    let case = declare_case_of("settings-pages-diag-perf", "Diagnostics");
    let mut page = DiagnosticsSettingsPage::new();
    check_declaration(&mut page, &case, true);
    assert_eq!(i64::from(page.ids().logging), case["ids"]["logging"]);
    assert_eq!(i64::from(page.ids().open_log), case["ids"]["open_log"]);
}

#[test]
fn the_performance_page_matches_the_golden_settings_pages_diag_perf() {
    let case = declare_case_of("settings-pages-diag-perf", "Performance");
    let mut page = PerformanceSettingsPage::new();
    check_declaration(&mut page, &case, true);
}

#[test]
fn the_display_page_matches_the_golden_settings_page_display() {
    let case = declare_case("settings-page-display");
    let mut page = DisplaySettingsPage::new();
    check_declaration(&mut page, &case, true);
}

#[test]
fn the_files_page_matches_the_golden_settings_page_files() {
    let case = declare_case("settings-page-files");
    let mut page = FilesSettingsPage::new();
    check_declaration(&mut page, &case, true);
}

#[test]
fn the_general_page_matches_the_golden_settings_page_general() {
    let case = declare_case("settings-page-general");
    let mut page = GeneralSettingsPage::new();
    check_declaration(&mut page, &case, true);
}

#[test]
fn the_mouse_page_matches_the_golden_settings_page_mouse() {
    let case = declare_case("settings-page-mouse");
    let mut page = MouseSettingsPage::new();
    check_declaration(&mut page, &case, true);
}

#[test]
fn the_shortcuts_page_structure_matches_the_golden_settings_page_shortcuts() {
    // Structure only (classes, ids, styles, id sets, element tree), texts excluded, so a structural drift is reported apart from a text drift (PARITY
    // X-settings-ui-pages notes English action names until `settings_texts()` exists, D-W8B5-806; the texts test below compares them against the golden).
    let case = declare_case("settings-page-shortcuts");
    let mut page = ShortcutsSettingsPage::new();
    check_declaration(&mut page, &case, false);
}

#[test]
fn the_shortcuts_page_texts_match_the_golden_settings_page_shortcuts() {
    let case = declare_case("settings-page-shortcuts");
    let mut page = ShortcutsSettingsPage::new();
    check_declaration(&mut page, &case, true);
}
