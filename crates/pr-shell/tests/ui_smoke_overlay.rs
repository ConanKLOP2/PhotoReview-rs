//! Slice S4 of unit W8-20b (`contracts/W8-20b.md`): the six whole-shell tests of C# `ShellOverlayIntegrationTests` that `W8-15j` defers ("Deferred to W8-20b: the other six"). Baseline `d92d6aae`,
//! `tests/PhotoReview.Shell.Integration.Tests/Startup/ShellOverlayIntegrationTests.cs` (the two others, the real click on Fit and the Tools anchor, are ported by 15j).
//!
//! Native (`#[ignore = "native: needs desktop"]`). The tests that switch the language or the settings serialise on one `static Mutex` and restore `Localizer::set_current` in a guard (the C#
//! "GlobalState" collection). Remaining unported cases retain explicit pending/blocked ignore reasons.
#![cfg(windows)]
use pr_app::overlay::element::ElementNode;
use pr_l10n::localizer::{Localizer, create_pseudo};
use pr_platform_win::host::contracts::{ShellWindowApi, ShellWindowEvent};
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
            let bounds = run
                .on_ui(move |p| {
                    let overlay = overlay(p);
                    let bounds = overlay.with_toolbar(|t| t.base().bounds());
                    let closing = overlay.clone();
                    p.main_window.window().subscribe(
                        ShellWindowEvent::Closed,
                        Box::new(move || {
                            let _ = tx.send((closing.toolbar_opacity(), closing.toolbar_hit_test_visible()));
                        }),
                    );
                    assert!(overlay.toolbar_opacity() > 0.0);
                    bounds
                })
                .unwrap();
            assert!(
                bounds.x >= 0.0
                    && bounds.y >= 0.0
                    && bounds.x <= 16.0
                    && bounds.y <= 16.0
                    && bounds.width > 0.0
                    && bounds.height > 0.0,
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

/// C# `Close_UnsubscribesTheOverlayFromTheStaticLocalizerEvent` (line 159).
#[test]
#[ignore = "native: blocked on public Localizer listener counter (W8-20b S4)"]
fn close_unsubscribes_the_overlay_from_the_static_localizer_event() {
    todo!("W8-20b S4: contracts/W8-20b.md")
}

/// C# `Close_UnsubscribesFromTheSettingsStoreAndTheViewModel` (line 188).
#[test]
#[ignore = "native: blocked on public settings/view-model listener counters (W8-20b S4)"]
fn close_unsubscribes_from_the_settings_store_and_the_view_model() {
    todo!("W8-20b S4: contracts/W8-20b.md")
}

/// C# `LanguageSwitch_RefreshesTheViewModelTextTheOverlayShows` (line 202).
#[test]
#[ignore = "native: needs desktop"]
fn language_switch_refreshes_the_view_model_text_the_overlay_shows() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "language_switch_refreshes_the_view_model_text_the_overlay_shows",
        || {
            let run = started();
            let before = run.on_ui(|p| p.view_model.borrow().status_text()).unwrap();
            let restore = LanguageRestore::new(&run);
            run.on_ui(|_| Localizer::set_current(Arc::new(create_pseudo(&Localizer::current()))))
                .unwrap();
            run.on_ui(|p| p.main_window.window().invalidate()).unwrap();
            run.wait_until("localized status", |s| s.status_text != before).unwrap();
            let (vm, shown) = run
                .on_ui(|p| (p.view_model.borrow().status_text(), overlay(p).model().status_text))
                .unwrap();
            assert_eq!(vm, shown);
            assert_ne!(vm, before);
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
