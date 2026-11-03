//! G2 / b4 H3 evidence (docs/reviews/G2-2026-11-03.md): the part of the view model that the shell input reaches after it changed the shared viewer
//! directly (`MainViewModel::on_viewer_zoom_mode_changed`). The presenter / zoom-detail half is pinned by `viewmodels_main_navigation.rs`
//! (`input_zoom_*`); this file pins that the overlay MODEL (`overlay_model()`) always reads the live viewer zoom, which is what any refresh of the HUD shows.
//! No C# twin: the C# viewer raises `ZoomModeChanged` / `PropertyChanged` itself (`MainViewModel.cs` lines 136 to 143).

mod file_action_fakes;
mod support {
    pub mod main_env;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use support::main_env::{EnvOptions, MainEnv};

fn hud_env() -> MainEnv {
    let mut env = MainEnv::with_options(EnvOptions {
        settings: Box::new(|settings| settings.show_zoom_indicator = true),
        ..EnvOptions::default()
    });
    let (folder, _) = env.images("zoom_events", &["a.png"]);
    env.open(&folder);
    env
}

#[test]
fn overlay_model_reads_the_live_viewer_zoom_after_an_input_zoom() {
    let mut env = hud_env();
    env.viewer.borrow_mut().set_zoom(2.0);
    env.vm.on_viewer_zoom_mode_changed();

    let model = env.vm.overlay_model();

    assert_eq!(env.viewer.borrow().display_zoom_percent(), 200);
    assert_eq!(model.zoom_indicator_text, "200%");
    assert!(model.zoom_indicator_visible);
}

#[test]
fn overlay_model_follows_each_further_input_zoom_step() {
    let mut env = hud_env();
    env.viewer.borrow_mut().set_zoom(2.0);
    env.vm.on_viewer_zoom_mode_changed();
    env.viewer.borrow_mut().set_zoom(0.5);
    env.vm.on_viewer_zoom_mode_changed();

    assert_eq!(env.vm.overlay_model().zoom_indicator_text, "50%");
}

// P-310 (C# `MainViewModel.cs` lines 140 to 143: `DisplayZoomPercent` -> `InfoOverlay.Refresh()`): the returned change set must tell the shell that the HUD zoom text changed.
#[test]
fn input_zoom_reports_the_zoom_indicator_text_as_changed() {
    use pr_app::viewmodels::info_overlay::InfoOverlayProperty;

    let mut env = hud_env();
    env.viewer.borrow_mut().set_zoom(2.0);

    let changes = env.vm.on_viewer_zoom_mode_changed();

    assert!(changes.overlay.contains(InfoOverlayProperty::ZoomIndicatorText));
}
