//! Inline, pure SettingsDialog contracts; no golden files, windows or user configuration.
use pr_platform_win::dialogs::settings::dialog::frame::{page_id_base, viewport_id};
use pr_platform_win::dialogs::settings::dialog::host::is_web_url;
use pr_platform_win::dialogs::settings::dialog::surface::{
    StaticTextKind, combo_drop_height, placement_change, static_text_kind,
};

#[test]
fn page_id_base_negative_and_wrapping_indices_preserve_contract() {
    // frame.rs:98:5 replace page_id_base -> i32 with -1 (unfinished prior lane).
    // C# SettingsDialog.PageIdBase / ViewportId, unchecked Int32 arithmetic.
    for (index, base, viewport) in [
        (-2, -1000, 108),
        (-1, 0, 109),
        (0, 1000, 110),
        (6, 7000, 116),
        (i32::MAX, 0, -2_147_483_539),
        (i32::MIN, 1000, -2_147_483_538),
    ] {
        assert_eq!(page_id_base(index), base, "index {index}");
        assert_eq!(viewport_id(index), viewport, "index {index}");
    }
}

#[test]
fn combo_drop_height_count_bounds_and_fallback_are_independent() {
    // surface.rs:36,38: return 0/1/-1, <= -> ==/</>/!=/>=.
    // C# SettingsDialog.PageSurface.ComboDropHeight.
    for (count, expected) in [
        (i32::MIN, 25),
        (0, 25),
        (1, 25),
        (2, 42),
        (12, 212),
        (13, 212),
        (i32::MAX, 212),
    ] {
        assert_eq!(combo_drop_height(count, 17, 23, 5, 8), expected);
    }
    assert_eq!(combo_drop_height(3, -1, 23, 5, 8), 92);
    assert_eq!(combo_drop_height(3, 0, 23, 5, 8), 92);
    assert_eq!(combo_drop_height(3, 1, 23, 5, 8), 11);
    assert_eq!(combo_drop_height(12, i32::MAX, 23, 5, 8), i32::MAX);
    assert_eq!(combo_drop_height(2, 0, i32::MAX, 5, 8), i32::MAX);
}

#[test]
fn placement_change_preserves_coordinates_and_clamps_each_size() {
    // surface.rs:54,55: return None/default rectangle; != -> ==.
    // C# SettingsDialog.PageSurface.Place cache and rectangle contract.
    let old = (7, -13, 31, 47);
    assert_eq!(placement_change(None, 7, -13, 31, 47), Some(old));
    assert_eq!(placement_change(Some(old), 7, -13, 31, 47), None);
    for next in [(8, -13, 31, 47), (7, -12, 31, 47), (7, -13, 32, 47), (7, -13, 31, 48)] {
        assert_eq!(placement_change(Some(old), next.0, next.1, next.2, next.3), Some(next));
    }
    assert_eq!(placement_change(None, -5, -9, -1, 47), Some((-5, -9, 0, 47)));
    assert_eq!(placement_change(None, -5, -9, 31, -1), Some((-5, -9, 31, 0)));
    assert_eq!(placement_change(Some((-5, -9, 0, 0)), -5, -9, -1, -2), None);
}

#[test]
fn static_text_kind_note_precedes_warning_and_normal_remains_distinct() {
    // surface.rs:25: replace static_text_kind with Default::default().
    // C# SettingsDialog.StaticTextColor.
    assert_eq!(static_text_kind(false, false), StaticTextKind::Normal);
    assert_eq!(static_text_kind(false, true), StaticTextKind::Warning);
    assert_eq!(static_text_kind(true, false), StaticTextKind::Muted);
    assert_eq!(static_text_kind(true, true), StaticTextKind::Muted);
}

#[test]
fn is_web_url_requires_nonempty_http_authority_and_rejects_injection() {
    // host.rs:61-68: return bool, ||/&& substitutions and length comparisons.
    // Rust-only URL hardening seam; no C# twin.
    for url in ["http://a", "https://b", " HTTPS://example.test/x ", "http://é"] {
        assert!(is_web_url(url), "{url:?}");
    }
    for url in [
        "",
        "http://",
        "https://",
        "file://a",
        "http://a b",
        "http://a\"b",
        "http://a\nb",
        "http://a\tb",
        "http://a\0b",
        "éééé",
        "https:/é",
        "http:/é",
    ] {
        assert!(!is_web_url(url), "{url:?}");
    }
}
