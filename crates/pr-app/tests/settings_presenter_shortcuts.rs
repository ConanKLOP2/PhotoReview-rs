//! W8-10e tests of `presenters::settings::shortcuts` (zoom summary and shortcut capture). Kind F: hand-written
//! expectations from the C# tests (baseline d92d6aae). Each test names its C# twin in a `// C#:` comment; tests without a
//! twin say `// New test`.

use std::collections::BTreeMap;

use pr_app::input::key_id::KeyId;
use pr_app::presenters::settings::SettingsPresenter;
use pr_app::presenters::settings_field_map::FieldContext;
use pr_core::settings::shortcuts::canonicalize;
use pr_l10n::localizer::built_in_english_localizer;
use pr_l10n::template::NumberCulture;

fn summary_of(texts: &BTreeMap<String, String>) -> Vec<String> {
    let localizer = built_in_english_localizer();
    let numbers = NumberCulture::invariant();
    let ctx = FieldContext {
        texts: &localizer,
        numbers: &numbers,
    };
    SettingsPresenter::zoom_shortcut_summary(texts, &ctx)
        .split("\r\n")
        .map(str::to_owned)
        .collect()
}

#[test]
fn capture_shortcut_reserved_keys_pass_through() {
    // C#: SettingsPresenterTests.CaptureShortcut_ReservedKeysPassThrough
    assert_eq!(SettingsPresenter::capture_shortcut(KeyId::TAB, "Tab"), None);
    assert_eq!(SettingsPresenter::capture_shortcut(KeyId::ESCAPE, "Escape"), None);
    assert_eq!(SettingsPresenter::capture_shortcut(KeyId::LEFT_CTRL, "LeftCtrl"), None);
    assert_eq!(
        SettingsPresenter::capture_shortcut(KeyId::RETURN, "Return"),
        Some(canonicalize(Some("Return")))
    );
}

#[test]
fn zoom_summary_lists_typed_keys() {
    // C#: SettingsPresenterTests.ZoomSummary_ListsTypedKeys
    let texts = BTreeMap::from([
        ("ClickZoom".to_owned(), "  ".to_owned()),
        ("ToggleFit".to_owned(), " F ".to_owned()),
    ]);
    let lines = summary_of(&texts);
    assert_eq!(lines.len(), 8);
    assert_eq!(lines[0], "Click zoom toggle: not set");
    assert_eq!(lines[1], "Fit image: F");
}

#[test]
fn zoom_summary_has_eight_crlf_joined_lines_in_the_documented_order() {
    // New test
    let texts = BTreeMap::from([
        ("ClickZoom".to_owned(), "A".to_owned()),
        ("ToggleFit".to_owned(), "B".to_owned()),
        ("ZoomActualSize".to_owned(), "C".to_owned()),
        ("ZoomIn".to_owned(), "D".to_owned()),
        ("ZoomOut".to_owned(), "E".to_owned()),
        ("FitWidth".to_owned(), "F".to_owned()),
        ("FitWidth2".to_owned(), "G".to_owned()),
        ("FitHeight".to_owned(), "H".to_owned()),
    ]);
    assert_eq!(
        summary_of(&texts),
        [
            "Click zoom toggle: A",
            "Fit image: B",
            "Zoom 100%: C",
            "Zoom in: D",
            "Zoom out: E",
            "Fit width: F",
            "Fit width 2: G",
            "Fit height: H",
        ]
    );
    // Missing entries read "not set"; nothing but CRLF separates the lines.
    let empty = summary_of(&BTreeMap::new());
    assert_eq!(empty.len(), 8);
    assert!(empty.iter().all(|line| line.ends_with(": not set")), "{empty:?}");
}

#[test]
fn capture_shortcut_canonicalizes_aliases() {
    // New test
    assert_eq!(
        SettingsPresenter::capture_shortcut(KeyId::A, "A"),
        Some(canonicalize(Some("A")))
    );
    assert_eq!(
        SettingsPresenter::capture_shortcut(KeyId::RETURN, "enter"),
        Some(canonicalize(Some("enter")))
    );
    assert_eq!(canonicalize(Some("enter")), "Enter");
    assert_eq!(
        SettingsPresenter::capture_shortcut(KeyId::RETURN, "enter").as_deref(),
        Some("Enter")
    );
}
