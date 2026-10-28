//! Slice S4 of unit W8-20b (`contracts/W8-20b.md`): the six whole-shell tests of C# `ShellOverlayIntegrationTests` that `W8-15j` defers ("Deferred to W8-20b: the other six"). Baseline `d92d6aae`,
//! `tests/PhotoReview.Shell.Integration.Tests/Startup/ShellOverlayIntegrationTests.cs` (the two others, the real click on Fit and the Tools anchor, are ported by 15j).
//!
//! Native (`#[ignore = "native: needs desktop"]`). The tests that switch the language or the settings serialise on one `static Mutex` and restore `Localizer::set_current` in a guard (the C#
//! "GlobalState" collection). Remaining unported cases retain explicit pending/blocked ignore reasons.
#![cfg(windows)]
use pr_app::overlay::element::ElementNode;
use pr_l10n::localizer::{Localizer, create_pseudo};
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::shell_overlay::main_window::MainWindowOverlay;
use std::sync::{Arc, Mutex, mpsc};
static SERIAL: Mutex<()> = Mutex::new(());

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{BOUND, SmokeConfig, UiProbe, UiSmokeRun, isolated_test};

/// C# `ToolbarIsDrawnFromTheFirstFrame_AtTheTopLeft_AndGoneAfterClose` (line 62).
#[test]
#[ignore = "native: needs desktop"]
fn toolbar_is_drawn_from_the_first_frame_at_the_top_left_and_gone_after_close() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "toolbar_is_drawn_from_the_first_frame_at_the_top_left_and_gone_after_close",
        || {
            let run = started();
            let (tx, rx) = mpsc::channel();
            let (opacity, bounds) = run
                .on_ui(move |p| {
                    let overlay = overlay(p);
                    let bounds = overlay.with_toolbar(|t| t.base().bounds());
                    // C# `Stop(run); Assert.Null(run.Main!.Overlay)`: the overlay is disposed by `MainWindow::dispose` at shutdown, so it is read once `ShellStartup::run` returned.
                    ui_smoke_fixture::after_exit(move || {
                        let _ = tx.send((overlay.toolbar_opacity(), overlay.toolbar_hit_test_visible()));
                    });
                    (overlay_opacity(p), bounds)
                })
                .unwrap();
            assert_eq!(opacity, 1.0);
            // C#: X = Y = 8 (3 decimals), height in 38..42 (32 + padding 4 + 4; golden 40), width > 100.
            assert!(
                (bounds.x - 8.0).abs() < 0.001 && (bounds.y - 8.0).abs() < 0.001,
                "{bounds:?}"
            );
            assert!(
                (38.0..=42.0).contains(&bounds.height) && bounds.width > 100.0,
                "{bounds:?}"
            );
            assert_eq!(run.close_and_wait(), Ok(0));
            assert_eq!(rx.recv_timeout(BOUND).unwrap(), (0.0, false));
        },
    );
}

/// C# `SettingsSave_ReevaluatesAutoHide_AndTheToolbarFollowsTheMouse` (line 109).
#[test]
#[ignore = "native: needs desktop"]
fn settings_save_reevaluates_auto_hide_and_the_toolbar_follows_the_mouse() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "settings_save_reevaluates_auto_hide_and_the_toolbar_follows_the_mouse",
        || {
            let run = started();
            run.on_ui(|p| {
                let mut settings = (*p.settings_store.current()).clone();
                settings.toolbar_auto_hide = true;
                settings.toolbar_auto_hide_delay_ms = 0;
                p.settings_store.save(settings).unwrap();
            })
            .unwrap();
            let (w, h) = run.on_ui(|p| p.main_window.window().client_size_pixels()).unwrap();
            run.mouse_move(w / 2, h / 2).unwrap();
            run.wait_until("toolbar hidden", |s| {
                s.toolbar_opacity <= 0.01 && !s.toolbar_hit_test_visible
            })
            .unwrap();
            run.mouse_move(10, 10).unwrap();
            run.wait_until("toolbar hot zone", |s| {
                s.toolbar_opacity > 0.9 && s.toolbar_hit_test_visible
            })
            .unwrap();
            run.on_ui(|p| {
                let mut settings = (*p.settings_store.current()).clone();
                settings.toolbar_auto_hide = false;
                p.settings_store.save(settings).unwrap();
            })
            .unwrap();
            run.mouse_move(w / 2, h / 2).unwrap();
            run.wait_until("auto hide disabled", |s| {
                s.toolbar_opacity > 0.9 && s.toolbar_hit_test_visible
            })
            .unwrap();
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `LanguageSwitch_UpdatesToolbarTextOnTheUiThread` (line 133).
#[test]
#[ignore = "native: needs desktop"]
fn language_switch_updates_toolbar_text_on_the_ui_thread() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test("language_switch_updates_toolbar_text_on_the_ui_thread", || {
        let run = started();
        let before = run
            .on_ui(|p| overlay(p).with_toolbar(|t| t.fit().content().to_owned()))
            .unwrap();
        let restore = LanguageRestore::new(&run);
        run.on_ui(|_| Localizer::set_current(Arc::new(create_pseudo(&Localizer::current()))))
            .unwrap();
        // Language notifications are queued; a rendered frame observes their completed application.
        let frame = run.snapshot().frames;
        run.on_ui(|p| p.main_window.window().invalidate()).unwrap();
        run.wait_until("language frame", |s| s.frames > frame).unwrap();
        let after = run
            .on_ui(|p| overlay(p).with_toolbar(|t| t.fit().content().to_owned()))
            .unwrap();
        assert_ne!(before, after);
        drop(restore);
        assert_eq!(run.close_and_wait(), Ok(0));
    });
}

/// C# `Close_UnsubscribesTheOverlayFromTheStaticLocalizerEvent` (line 159): the C# test counts `Localizer.CurrentChanged` by reflection; the twin reads `current_changed_handler_count()`.
#[test]
#[ignore = "native: needs desktop"]
fn close_unsubscribes_the_overlay_from_the_static_localizer_event() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test("close_unsubscribes_the_overlay_from_the_static_localizer_event", || {
        let before = pr_l10n::localizer::current_changed_handler_count();
        let run = started();
        assert!(
            pr_l10n::localizer::current_changed_handler_count() > before,
            "the overlay does not listen to Localizer.CurrentChanged"
        );
        assert_eq!(run.close_and_wait(), Ok(0));
        assert_eq!(pr_l10n::localizer::current_changed_handler_count(), before);
    });
}

/// C# `Close_UnsubscribesFromTheSettingsStoreAndTheViewModel` (line 188). The settings half is ported with `SettingsStore::changed_handler_count()`: the production graph subscribes the log
/// switch for the whole process (`phase_one`), the overlay one more handler, so closing leaves exactly one. The view-model half has no Rust twin: the Rust view model has no listener list (the
/// window passes every `ChangeSet` to its features, nothing subscribes to it), so there is nothing to leak.
#[test]
#[ignore = "native: needs desktop"]
fn close_unsubscribes_from_the_settings_store_and_the_view_model() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test("close_unsubscribes_from_the_settings_store_and_the_view_model", || {
        let run = started();
        let store = run.on_ui(|p| p.settings_store.clone()).unwrap();
        let before = store.changed_handler_count();
        assert!(
            before >= 2,
            "the overlay does not listen to SettingsStore.Changed ({before} handlers)"
        );
        assert_eq!(run.close_and_wait(), Ok(0));
        assert_eq!(store.changed_handler_count(), before - 1);
    });
}

/// C# `LanguageSwitch_RefreshesTheViewModelTextTheOverlayShows` (line 202): the text the view model builds in code (`FolderText`, "N photos") changes only when `refresh_localized_text` runs.
/// (`status_text` is event text that keeps its language until the next update, so it cannot show a language switch.)
#[test]
#[ignore = "native: needs desktop"]
fn language_switch_refreshes_the_view_model_text_the_overlay_shows() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "language_switch_refreshes_the_view_model_text_the_overlay_shows",
        || {
            let run = started();
            let fit_before = run
                .on_ui(|p| overlay(p).with_toolbar(|t| t.fit().content().to_owned()))
                .unwrap();
            let english = run.on_ui(|p| p.view_model.borrow().folder_text().to_owned()).unwrap();
            let restore = LanguageRestore::new(&run);
            run.on_ui(|_| Localizer::set_current(Arc::new(create_pseudo(&Localizer::current()))))
                .unwrap();
            run.on_ui(|p| p.main_window.window().invalidate()).unwrap();
            // C#: wait for the toolbar to show the new language, then read the view model text.
            run.wait_ui_condition("toolbar in the new language", move |p| {
                overlay(p).with_toolbar(|t| t.fit().content() != fit_before)
            })
            .unwrap();
            let switched = run.on_ui(|p| p.view_model.borrow().folder_text().to_owned()).unwrap();
            assert_ne!(english, String::new());
            assert_ne!(english, switched); // built in code: changes only when refresh_localized_text ran
            drop(restore);
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

fn started() -> UiSmokeRun {
    let run = UiSmokeRun::start(SmokeConfig::default()).unwrap();
    run.wait_first_image().unwrap();
    run
}
fn overlay_opacity(p: &UiProbe) -> f32 {
    overlay(p).toolbar_opacity()
}
fn overlay(p: &UiProbe) -> std::rc::Rc<pr_platform_win::shell_overlay::overlay::state::ShellOverlay> {
    p.main_window
        .with_feature::<MainWindowOverlay, _>(|o| o.overlay().clone())
        .expect("production overlay")
}
struct LanguageRestore<'a> {
    run: &'a UiSmokeRun,
    previous: Arc<Localizer>,
}
impl<'a> LanguageRestore<'a> {
    fn new(run: &'a UiSmokeRun) -> Self {
        let previous = run.on_ui(|_| Localizer::current()).unwrap();
        Self { run, previous }
    }
}
impl Drop for LanguageRestore<'_> {
    fn drop(&mut self) {
        let previous = self.previous.clone();
        let _ = self.run.on_ui(move |_| Localizer::set_current(previous));
    }
}
