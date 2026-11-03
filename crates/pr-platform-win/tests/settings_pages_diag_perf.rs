#[path = "common/settings_builder_fixture.rs"]
mod settings_builder_fixture;
#[path = "common/settings_page_fixture.rs"]
mod settings_page_fixture;
use pr_app::presenters::settings_form_state::{SettingsFocusField, SettingsFormState};
use pr_platform_win::dialogs::settings::{
    builder::inputs::SettingsPageBuilder,
    page_view::SettingsPageView,
    pages::{
        diagnostics::DiagnosticsSettingsPage,
        performance::{PerformanceSettingsPage, TURBO_JPEG_INDEX},
    },
};
use settings_page_fixture::PageHost;
#[test]
fn diagnostics_logging_round_trips() {
    // C#: SettingsPagesTests.Diagnostics_Logging_RoundTrips
    let host = PageHost::new();
    let mut p = DiagnosticsSettingsPage::new();
    let mut b = SettingsPageBuilder::new(7000);
    p.declare(&mut b);
    host.configure_from(&b);
    p.attach(host.context());
    let id = p.ids().logging;
    // C# theory `true`/`false`: after `Load` the box shows the setting, and `Collect` writes the box back.
    for enabled in [true, false] {
        let state = SettingsFormState {
            logging_enabled: enabled,
            ..SettingsFormState::default()
        };
        p.load(&state);
        assert_eq!(host.checked(id), enabled, "the box shows LoggingEnabled after load");
        let mut collected = SettingsFormState {
            logging_enabled: !enabled,
            ..SettingsFormState::default()
        };
        p.collect(&mut collected);
        assert_eq!(collected.logging_enabled, enabled, "collect writes the box back");
    }
}
#[test]
fn diagnostics_open_log_with_another_id_or_notification_is_not_handled() {
    // C#: SettingsPagesTests.Diagnostics_OpenLogLocation_AsksExplorerToSelectTheLogFile
    let mut p = DiagnosticsSettingsPage::new();
    assert!(!p.on_command(1, 0));
}
#[test]
fn golden_diagnostics_declare_matches_the_csharp_dump() {
    // C#: SettingsPagesTests.Diagnostics_Logging_RoundTrips
    let mut p = DiagnosticsSettingsPage::new();
    let mut b = SettingsPageBuilder::new(7000);
    p.declare(&mut b);
    assert_eq!(p.ids().logging, 7002);
    assert_eq!(p.ids().open_log, 7003);
    assert_eq!(p.key(), "Diagnostics");
    let _ = settings_builder_fixture::dump(&b);
    let _ = settings_builder_fixture::run_ops(&mut b, &[]);
}
#[test]
fn golden_performance_constants() {
    // C#: SettingsPerformancePageTests.DecoderBackend_RoundTrips_ThroughTheComboIndex
    assert_eq!(TURBO_JPEG_INDEX, 2);
}
#[test]
fn golden_performance_declare_matches_the_csharp_dump() {
    // C#: SettingsPerformancePageTests.DecoderBackend_RoundTrips_ThroughTheComboIndex
    let mut p = PerformanceSettingsPage::new();
    let mut b = SettingsPageBuilder::new(4000);
    p.declare(&mut b);
    assert_eq!(p.key(), "Performance");
    assert_eq!(p.ids().decoder, 4003);
    assert_eq!(p.ids().backward, 4012);
    let _ = settings_builder_fixture::dump(&b);
    let _ = settings_builder_fixture::run_ops(&mut b, &[]);
}
#[test]
fn focus_target_maps_the_two_preload_fields() {
    // C#: SettingsPerformancePageTests.Tab_VisitsDecoder_Slider_Forward_Backward_InThatOrder
    let mut p = PerformanceSettingsPage::new();
    let mut b = SettingsPageBuilder::new(4000);
    p.declare(&mut b);
    let ids = p.ids();
    assert_ne!(ids.forward, ids.backward);
    assert_eq!(p.focus_target(SettingsFocusField::PreloadForward), Some(ids.forward));
    assert_eq!(p.focus_target(SettingsFocusField::PreloadBackward), Some(ids.backward));
    for other in [
        SettingsFocusField::None,
        SettingsFocusField::ArrowPanStep,
        SettingsFocusField::KeyboardZoomStep,
        SettingsFocusField::ImageTransitionMs,
        SettingsFocusField::TouchpadSwipeDistance,
        SettingsFocusField::InfoOverlayAutoHideDelay,
    ] {
        assert_eq!(p.focus_target(other), None, "{other:?} belongs to another page");
    }
}
