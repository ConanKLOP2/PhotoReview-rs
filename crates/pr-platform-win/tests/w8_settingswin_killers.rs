//! Deterministic settings-window logic killers; no window or real user configuration.
use pr_platform_win::dialogs::settings::dialog::frame::{page_id_base, viewport_id};
use pr_platform_win::dialogs::settings::dialog::surface::{
    StaticTextKind, combo_drop_height, placement_change, static_text_kind,
};

#[path = "settings_dialog_frame.rs"]
mod established_frame;

#[test]
fn web_url_rejects_empty_schemes_injection_and_unicode_boundary() {
    // Mutant host.rs:61-68: bool constants, || -> &&, && -> ||, > -> >=/==/<.
    // Rust hardening seam (host.rs documents that it has no C# twin).
    use pr_platform_win::dialogs::settings::dialog::host::is_web_url;
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
        "éééé",
        "https:/é",
        "http:/é",
    ] {
        assert!(!is_web_url(url), "{url:?}");
    }
}

#[test]
fn static_text_color_note_takes_precedence_over_warning() {
    // Mutant surface.rs:25: replace static_text_kind with Default::default().
    // C# SettingsDialog.StaticTextColor: notes are checked before warnings.
    assert_eq!(static_text_kind(false, false), StaticTextKind::Normal);
    assert_eq!(static_text_kind(false, true), StaticTextKind::Warning);
    assert_eq!(static_text_kind(true, false), StaticTextKind::Muted);
    assert_eq!(static_text_kind(true, true), StaticTextKind::Muted);
}

#[test]
fn combo_drop_height_clamps_count_and_uses_fallback_at_zero() {
    // Mutant surface.rs:36,38: replace return with 0/1/-1; <= -> >.
    // C# SettingsDialog.PageSurface.ComboDropHeight: clamp 1..12, fallback at <= 0.
    for (count, expected) in [(-9, 25), (0, 25), (1, 25), (2, 42), (12, 212), (13, 212)] {
        assert_eq!(combo_drop_height(count, 17, 23, 5, 8), expected);
    }
    assert_eq!(combo_drop_height(3, 0, 23, 5, 8), 92);
    assert_eq!(combo_drop_height(3, -1, 23, 5, 8), 92);
    assert_eq!(combo_drop_height(3, 1, 23, 5, 8), 11);
    assert_eq!(combo_drop_height(12, i32::MAX, 23, 5, 8), i32::MAX);
    assert_eq!(combo_drop_height(2, 0, i32::MAX, 5, 8), i32::MAX);
}

#[test]
fn placement_cache_compares_every_coordinate_after_clamping_sizes() {
    // Mutant surface.rs:54,55: replace return with None/default rectangle; != -> ==.
    // C# SettingsDialog.PageSurface.Place: skip only an identical clamped rectangle.
    let rect = (7, -13, 31, 47);
    assert_eq!(placement_change(None, 7, -13, 31, 47), Some(rect));
    assert_eq!(placement_change(Some(rect), 7, -13, 31, 47), None);
    for changed in [(8, -13, 31, 47), (7, -12, 31, 47), (7, -13, 32, 47), (7, -13, 31, 48)] {
        assert_eq!(
            placement_change(Some(rect), changed.0, changed.1, changed.2, changed.3),
            Some(changed)
        );
    }
    assert_eq!(placement_change(None, -5, -9, -1, -2), Some((-5, -9, 0, 0)));
    assert_eq!(placement_change(Some((-5, -9, 0, 0)), -5, -9, -1, -2), None);
    assert_eq!(placement_change(None, 7, -13, -1, 47), Some((7, -13, 0, 47)));
    assert_eq!(placement_change(None, 7, -13, 31, -1), Some((7, -13, 31, 0)));
}

#[test]
fn page_ids_preserve_wrapping_at_integer_extremes() {
    // Mutant frame.rs:93,98: replace viewport_id/page_id_base return with 0/1/-1.
    // C# SettingsDialog.ViewportId/PageIdBase: unchecked Int32 arithmetic.
    assert_eq!(viewport_id(i32::MAX), -2_147_483_539);
    assert_eq!(viewport_id(i32::MIN), -2_147_483_538);
    assert_eq!(page_id_base(i32::MAX), 0);
    assert_eq!(page_id_base(i32::MIN), 1000);
    assert_eq!(page_id_base(-2), -1000);
}
