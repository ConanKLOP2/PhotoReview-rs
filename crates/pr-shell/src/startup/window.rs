//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 394-461 (`CreateMainWindow`, baseline `d92d6aae`) plus the `ViewportFactory` that the C# `ShellMainWindow` constructor built
//! itself (D-W8B4M-4, D-W8B4X-6) and the `AttachOverlay` call (`ShellMainWindow.Overlay.cs` 52-108 reads the services; the Rust `OverlayDeps` takes them, D-W8B4X-17). Unit W8-17e.
//! Contract: `contracts/W8-17e.md`.
//!
//! Order is behaviour (perf/startup-first-image): the surface is never seen unpainted (white) and the window is never seen at 1200x800 and then jumping to its placement: cloak from the moment
//! the HWND exists, placement applied while the window is hidden, THEN `ShowWindow`, then the view model and the Direct2D surface, then the first frame NOW so the cloak is lifted after the first
//! present instead of waiting for an image.

use super::forward_target::MainWindowForwardTarget;
use super::overlay_vm::MainWindowOverlayViewModel;
use pr_app::startup::forwarded_open::ForwardedOpenHandler;
use pr_core::instance::ownership::FolderOwnership;
use pr_core::io::physical::PhysicalFileSystem;
use pr_l10n::localizer::Localizer;
use pr_platform_win::host::contracts::{ShellWindowApi, ShellWindowEvent};
use pr_platform_win::host::frame_clock::TimerFrameClock;
use pr_platform_win::host::main_window::core::{
    DEFAULT_HEIGHT_DIP, DEFAULT_WIDTH_DIP, MainWindowDeps, MainWindowOptions, UiLocalPoster,
};
use pr_platform_win::host::message_loop::MessageLoop;
use pr_platform_win::host::shell_window::{ShellWindow, ShellWindowOptions};
use pr_platform_win::render::text::renderer::DWriteTextRenderer;
use pr_platform_win::shell_input::main_window::{MainWindowInputFeature, create_viewport_input_hooks};
use pr_platform_win::shell_menus::popup_host::NativePopupMenuHost;
use pr_platform_win::shell_overlay::main_window::{MainWindowOverlay, OverlayDeps, WindowUiWake};
use pr_platform_win::shell_overlay::native::Win32OverlayOs;
use pr_platform_win::shell_startup::cloak::{StartupCloak, StartupCloakOptions};
use pr_platform_win::shell_startup::placement::{
    NativeWindowPlacement, ShellPlacementSaver, apply_before_show, show_command_for,
};
use pr_platform_win::shell_viewing::keep_warm::KeepWarmRegistration;
use pr_platform_win::shell_viewing::viewport_controller::{ViewportController, ViewportControllerOptions};
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use pr_core::abstractions::DisplayClock;
use pr_platform_win::host::main_window::MainWindow;
use pr_platform_win::host::main_window::core::ViewportFactory;

use super::error::StartupError;
use super::state::StartupState;

/// The `ViewportFactory` of the C# constructor (`CreateViewportOptions(window, displayClock)` of `ShellMainWindow.Input.cs` + `new ViewportController(..)`): `move |cb| { let hooks =
/// create_viewport_input_hooks(cb.window.clone()); let options = ViewportControllerOptions { drag_metrics_pixels: hooks.drag_metrics_pixels, cursor_screen_pixel: hooks.cursor_screen_pixel,
/// display_clock: Some(display_clock), ..Default::default() }; ViewportController::new(cb.window, Rc::new(cb.poster), cb.frame_clock, cb.content, cb.update_fit_size, options) }`.
/// `scroll_bar_thickness` MUST stay at its default (D-W8B4X-7).
pub fn build_viewport_factory(display_clock: Arc<dyn DisplayClock>) -> ViewportFactory {
    Box::new(move |cb| {
        let hooks = create_viewport_input_hooks(cb.window.clone());
        let options = ViewportControllerOptions {
            drag_metrics_pixels: hooks.drag_metrics_pixels,
            cursor_screen_pixel: hooks.cursor_screen_pixel,
            display_clock: Some(display_clock),
            ..Default::default()
        };
        ViewportController::new(
            cb.window,
            Rc::new(cb.poster),
            cb.frame_clock,
            cb.content,
            cb.update_fit_size,
            options,
        )
    })
}

/// A fresh identity for a `DWriteTextRenderer` (W8-15c O2: the layout caches key on it): a process-wide `AtomicU64` counter starting at 1.
pub fn next_text_renderer_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl StartupState {
    /// C# `CreateMainWindow(services, hub, paths, placementFile, store, log)` (lines 394-461) on the UI thread, in this order; any `Err` is a start-up failure:
    ///
    /// 1. `activate = options.activate && !launch.smoke`.
    /// 2. **Frame clock** (400-402): `TimerFrameClock::create(Some(graph.display_clock.clone()), 0)?` (window 0: the C# `new TimerFrameClock(displayClock)`), stored in `self.frame_clock`;
    ///    `message_loop.add_frame_clock(clock as Rc<dyn HostFrameClock>)`.
    /// 3. **Window** (403-405): `ShellWindow::create(ShellWindowOptions { title: Localizer::current().get("app.title"), width_dip: DEFAULT_WIDTH_DIP, height_dip: DEFAULT_HEIGHT_DIP, size_is_outer: true,
    ///    ..Default::default() }, clock)?`, `Rc`'d.
    /// 4. **Closed** (406-410): `window.subscribe(ShellWindowEvent::Closed, ..)`: `close_secondary_windows()` (idempotent fallback for a forced destroy that skipped `Closing`) then
    ///    `MessageLoop::quit(0)`. The closure holds a `Weak<StartupState>`.
    /// 5. **Cloak** (414-415): `StartupCloak::new(ui, StartupCloakOptions { on_revealed: Some(mark "windowRevealed"), set_cloaked: options.set_cloaked.take(), ..Default })` stored in `self.cloak`;
    ///    `cloak.hide(window.hwnd())`.
    /// 6. **Placement before show** (416-425): smoke -> `None`; else `placement_read::read_saved_placement(&*store, &PhysicalFileSystem, file, &*log)` then
    ///    `shell_startup::placement::apply_before_show(hwnd, data.as_ref(), &|d| store.is_visible(d), &NativeWindowPlacement, Some(&*log))`; `Some(state)` -> `mark("placementRestoredBeforeShow")`.
    /// 7. **Show** (427-428): `window.show_command(show_command_for(state, activate))`; `mark("windowShown")`.
    /// 8. **View model** (430-437): `vm.set_folder_ownership(instance.clone().map(|scope| scope as Arc<dyn FolderOwnership>))` (smoke: `None`); `DiagOptions::instance_label` -> `vm.set_instance_label(Some(label))`
    ///    (the `ChangeSet` is dropped: nothing is subscribed yet, the window reads the title when it is built). `ShellNoticeSink.Attach` is NOT ported (D-W8B6-8).
    /// 9. **Options** (439-444): `MainWindowOptions { prefer_warp, placement_file: (!smoke).then(|| graph.window_placement_file.clone()), activate, windowing: options.windowing.take(),
    ///    extensions }` where `extensions.insert(ZoomPromptOption(services.zoom_prompt.clone()))` and `extensions.insert(KeepWarmRegistration { frame_log_path, keep_warm, prefer_warp, work, timestamp })`
    ///    from the options (the window overwrites `prefer_warp` of the registration, D-W8B4X-5). `ShellInputEnvironment` is NOT inserted (the input falls back to the Win32 default).
    /// 10. **Main window** (439): `MainWindow::new(MainWindowDeps { window, dispatcher, frame_clock, view_model: graph.view_model.clone(), size_source, hub: hub as Rc<dyn PresentationHubPort>,
    ///     cloak: cloak as Rc<dyn StartupCloakPort>, placement: Rc::new(ShellPlacementSaver::new(store, Rc::new(NativeWindowPlacement), log)), log, mark: Some(boxed mark), viewport_factory:
    ///     build_viewport_factory(display_clock), options })`, `Rc`'d and stored in the SLOT `self.main`.
    /// 11. **Closing** (445-448): `window.subscribe_closing(..)` registered AFTER `MainWindow::new` (so it runs after the window's own closing handler): `close_secondary_windows_unless_cancelled(
    ///     e.cancel, || self.close_secondary_windows())`. Secondary windows must close while the shell HWND is alive: destroying the owner first would stop the modal dialog's loop from ever seeing its
    ///     `WM_CLOSE` and swallow `WM_QUIT`.
    /// 12. **Forwarded open** (449): `router.set_handler(Rc::new(ForwardedOpenHandler::new(Rc::new(MainWindowForwardTarget::new(Rc::downgrade(&self.main))), log)))`.
    /// 13. **Renderer** (450): `main.attach_renderer(None)?`.
    /// 14. **Overlay** (451): [`attach_overlay`](Self::attach_overlay) (the HUD is there from the first frame).
    /// 15. `mark("mainWindowConstructed")`; **first frame NOW** (455): `main.render_now()`; smoke -> `arm_smoke_exit` (`smoke.rs`).
    #[allow(dead_code)] // W8-17e part B wires the caller in run.rs; keep its binding visibility.
    pub(super) fn create_main_window(self: &Rc<Self>) -> Result<(), StartupError> {
        let smoke = self.launch.smoke;
        let activate = self.options.borrow().activate && !smoke;
        let (display_clock, placement_file, size_source, log) = {
            let graph = self.graph.borrow();
            let graph = graph
                .as_ref()
                .ok_or_else(|| StartupError::Graph("Startup graph is unavailable.".into()))?;
            (
                graph.display_clock.clone(),
                graph.window_placement_file.clone(),
                graph.size_source.clone(),
                graph.log.clone(),
            )
        };
        let services = self
            .services
            .borrow()
            .clone()
            .ok_or_else(|| StartupError::Graph("Shell services are unavailable.".into()))?;
        let clock = Rc::new(TimerFrameClock::create(Some(display_clock.clone()), 0)?);
        *self.frame_clock.borrow_mut() = Some(clock.clone());
        self.message_loop.add_frame_clock(clock.clone());
        let window = Rc::new(ShellWindow::create(
            ShellWindowOptions {
                title: Localizer::current().get("app.title"),
                width_dip: DEFAULT_WIDTH_DIP,
                height_dip: DEFAULT_HEIGHT_DIP,
                size_is_outer: true,
                ..Default::default()
            },
            clock.clone(),
        )?);
        let weak = Rc::downgrade(self);
        window.subscribe(
            ShellWindowEvent::Closed,
            Box::new(move || {
                if let Some(state) = weak.upgrade() {
                    state.close_secondary_windows();
                }
                MessageLoop::quit(0);
            }),
        );
        let mark = self.mark.clone();
        let set_cloaked = self.options.borrow_mut().set_cloaked.take();
        let cloak = Rc::new(StartupCloak::new(
            self.ui.clone(),
            StartupCloakOptions {
                on_revealed: Some(Arc::new(move || mark("windowRevealed"))),
                set_cloaked,
                ..Default::default()
            },
        ));
        *self.cloak.borrow_mut() = Some(cloak.clone());
        cloak.hide(window.hwnd());
        let state = if smoke {
            None
        } else {
            let data = pr_app::startup::placement_read::read_saved_placement(
                &*self.placement_store,
                &PhysicalFileSystem,
                &placement_file.to_string_lossy(),
                &*log,
            );
            let state = apply_before_show(
                window.hwnd(),
                data.as_ref(),
                &|data| self.placement_store.is_visible(data),
                &NativeWindowPlacement,
                Some(&*log),
            );
            if state.is_some() {
                (self.mark)("placementRestoredBeforeShow");
            }
            state
        };
        window.show_command(show_command_for(state, activate));
        (self.mark)("windowShown");
        // D-W8B6-160: resolve the one-shot factory only after settings and ShowWindow.
        // Extract it before calling user/composition code: no state borrow spans construction.
        let factory = {
            let mut graph = self.graph.borrow_mut();
            let graph = graph
                .as_mut()
                .ok_or_else(|| StartupError::Graph("Startup graph is unavailable.".into()))?;
            std::mem::replace(
                &mut graph.view_model,
                Box::new(|| {
                    Err(StartupError::Graph(
                        "The view model factory was already consumed.".into(),
                    ))
                }),
            )
        };
        let view_model = factory()?;
        let instance = if smoke {
            None
        } else {
            self.instance
                .borrow()
                .clone()
                .map(|scope| scope as Arc<dyn FolderOwnership>)
        };
        view_model.borrow_mut().set_folder_ownership(instance);
        if let Some(label) = super::graph::diag_options().instance_label.clone() {
            view_model.borrow_mut().set_instance_label(Some(label));
        }
        let mut extensions = services.window_service_extensions();
        let options = {
            let mut options = self.options.borrow_mut();
            extensions.insert(KeepWarmRegistration {
                frame_log_path: options.frame_log_path.take(),
                keep_warm: options.keep_warm.take(),
                prefer_warp: options.prefer_warp,
                work: options.keep_warm_work.take(),
                timestamp: options.keep_warm_timestamp.take(),
            });
            MainWindowOptions {
                prefer_warp: options.prefer_warp,
                placement_file: (!smoke).then_some(placement_file),
                activate,
                windowing: options.windowing.take(),
                extensions,
            }
        };
        let mark = self.mark.clone();
        let main = Rc::new(MainWindow::new(MainWindowDeps {
            window: window.clone(),
            dispatcher: self.dispatcher.clone(),
            frame_clock: clock,
            view_model,
            size_source,
            hub: self.hub.clone(),
            cloak,
            placement: Rc::new(ShellPlacementSaver::new(
                self.placement_store.clone(),
                Rc::new(NativeWindowPlacement),
                log.clone(),
            )),
            log: log.clone(),
            mark: Some(Box::new(move |name| mark(name))),
            viewport_factory: build_viewport_factory(display_clock),
            options,
        }));
        *self.main.borrow_mut() = Some(main.clone());
        let weak = Rc::downgrade(self);
        window.subscribe_closing(Box::new(move |event| {
            if !event.cancel {
                if let Some(state) = weak.upgrade() {
                    state.close_secondary_windows();
                }
            }
        }));
        self.router.set_handler(Rc::new(ForwardedOpenHandler::new(
            Rc::new(MainWindowForwardTarget::new(Rc::downgrade(&self.main))),
            log,
        )));
        main.attach_renderer(None)?;
        self.attach_overlay(&main)?;
        (self.mark)("mainWindowConstructed");
        main.render_now();
        if smoke {
            self.arm_smoke_exit(&main);
        }
        Ok(())
    }

    /// C# `AttachOverlay(services, frameClock)` as the caller of `MainWindowOverlay::attach` (D-W8B4X-17): builds the `OverlayDeps` and registers the result with `main.add_feature`:
    /// `text` = `Rc::new(DWriteTextRenderer::new(None, &pr_l10n::loader::current_ui_culture().name, 256))` (`Err(TextError)` is `StartupError::Overlay(text)`: the C# constructor threw and the start-up failed),
    /// `text_renderer_id = next_text_renderer_id()`, `texts = Localizer::current()`, `window = Rc<ShellWindow>` as `Rc<dyn OverlayWindowPort>`, `clock = frame_clock`, `view_model =
    /// Rc::new(MainWindowOverlayViewModel::new(..))`, `menus = Some(Rc::new(NativePopupMenuHost::new(owner closure).with_log(log)))` (the SECOND popup host, D-W8B4X-9 Q4: its own owner closure), `settings_store =
    /// Some(graph.settings_store.clone())`, `poster = Rc::new(UiLocalPoster::new(dispatcher))`, `wake = Arc::new(WindowUiWake::new(dispatcher, None))`, `fit` = `move || { if let Some(window) = slot
    /// .upgrade().and_then(..) { window.with_feature::<MainWindowInputFeature, _>(|f| f.input().input().apply_fit()); } }` (the Fit closure is mandatory: without it the Fit button does nothing),
    /// `tooltip_factory: None`, `os = Rc::new(Win32OverlayOs)`, `log`.
    #[allow(dead_code)] // Reachable through create_main_window once part B wires run.rs.
    pub(super) fn attach_overlay(self: &Rc<Self>, main: &Rc<MainWindow>) -> Result<(), StartupError> {
        let (settings_store, log) = {
            let graph = self.graph.borrow();
            let graph = graph
                .as_ref()
                .ok_or_else(|| StartupError::Graph("Startup graph is unavailable.".into()))?;
            (graph.settings_store.clone(), graph.log.clone())
        };
        let clock = self
            .frame_clock
            .borrow()
            .clone()
            .ok_or_else(|| StartupError::Overlay("The frame clock is unavailable.".into()))?;
        let text = Rc::new(
            DWriteTextRenderer::new(None, &pr_l10n::loader::current_ui_culture().name, 256)
                .map_err(|error| StartupError::Overlay(error.to_string()))?,
        );
        let owner_slot = Rc::downgrade(&self.main);
        let menus = NativePopupMenuHost::new(Box::new(move || {
            let window = owner_slot.upgrade().and_then(|slot| slot.borrow().clone());
            window.map_or(0, |window| window.hwnd())
        }))
        .with_log(log.clone());
        let fit_slot = Rc::downgrade(&self.main);
        let overlay = MainWindowOverlay::attach(OverlayDeps {
            window: main.window().clone(),
            clock,
            view_model: Rc::new(MainWindowOverlayViewModel::new(
                main.view_model().clone(),
                &self.main,
                log.clone(),
            )),
            text,
            text_renderer_id: next_text_renderer_id(),
            texts: Localizer::current(),
            menus: Some(Rc::new(menus)),
            settings_store: Some(settings_store),
            poster: Rc::new(UiLocalPoster::new(self.dispatcher.clone())),
            wake: Arc::new(WindowUiWake::new(self.dispatcher.clone(), None)),
            fit: Box::new(move || {
                let window = fit_slot.upgrade().and_then(|slot| slot.borrow().clone());
                if let Some(window) = window {
                    window.with_feature::<MainWindowInputFeature, _>(|feature| feature.input().input().apply_fit());
                }
            }),
            tooltip_factory: None,
            os: Rc::new(Win32OverlayOs),
            log,
        });
        main.add_feature(overlay);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::{
        composition_root::{ShellHostServices, ShellServices, ShellServicesOptions},
        graph::{GraphInputs, GraphOverrides, production_graph_builder_with},
        options::ShellStartupOptions,
    };
    use pr_app::startup::{forwarded_open::ForwardedOpenRouter, launch::ShellLaunch};
    use pr_app::windowing::contracts::{WindowPlacementData, WindowPlacementStore};
    use pr_core::{abstractions::NullLog, app_paths::AppPaths};
    use pr_platform_win::host::ui_dispatcher::Win32UiDispatcher;
    use pr_platform_win::shell_startup::placement::WindowPlacementNative;
    use pr_platform_win::shell_viewing::presentation_sink::ShellPresentationHub;
    use pr_test_support::{FakeFileSystem, FakeRecycleBin, TempRoot};
    use std::{
        sync::{Mutex, mpsc},
        time::{Duration, Instant},
    };

    fn on_ui(body: impl FnOnce() + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
            tx.send(result).unwrap();
        });
        let result = rx
            .recv_timeout(Duration::from_secs(60))
            .expect("bounded window construction");
        thread.join().unwrap();
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }

    fn with_startup(saved: Option<WindowPlacementData>, body: impl FnOnce(&Rc<MainWindow>, &[String])) {
        let root = TempRoot::new("startup-window-part-a");
        let log = Arc::new(NullLog);
        let dispatcher = Win32UiDispatcher::new(log.clone()).unwrap();
        let message_loop = Rc::new(MessageLoop::new(Rc::new(dispatcher.clone())).unwrap());
        let hub = Rc::new(ShellPresentationHub::new());
        let marks = Arc::new(Mutex::new(Vec::<String>::new()));
        let recorded = marks.clone();
        let mark: pr_app::startup::early_decode::MarkFn =
            Arc::new(move |name| recorded.lock().unwrap().push(name.into()));
        let cloak_marks = marks.clone();
        let options = ShellStartupOptions {
            prefer_warp: true,
            activate: false,
            warm_graphics: false,
            install_process_hooks: false,
            start_perf_listener: false,
            set_cloaked: Some(Arc::new(move |hwnd, cloaked| {
                if cloaked {
                    assert!(!pr_platform_win::win32::api::user32::is_window_visible(hwnd));
                }
                cloak_marks
                    .lock()
                    .unwrap()
                    .push(if cloaked { "cloakHidden" } else { "cloakRevealed" }.into());
                true
            })),
            ..Default::default()
        };
        let state = StartupState::new(
            vec![],
            ShellLaunch::parse::<&str>(&[], &FakeFileSystem::new(), &NullLog),
            options,
            mark.clone(),
            Instant::now(),
            dispatcher.clone(),
            message_loop.clone(),
            hub.clone(),
            ForwardedOpenRouter::new(),
        );
        let services = Rc::new(ShellServices::build(
            ShellHostServices {
                dispatcher: dispatcher.clone(),
                owner_window: Rc::new(|| 0),
                message_loop: Some(message_loop.clone()),
            },
            hub.clone(),
            log.clone(),
            ShellServicesOptions::default(),
        ));
        let paths = AppPaths::new(root.path(), Some(&root.path().to_string_lossy()), true);
        let builder = production_graph_builder_with(GraphOverrides {
            paths: Some(paths),
            recycle_bin: Some(Arc::new(FakeRecycleBin::new(Arc::new(FakeFileSystem::new())))),
            ..Default::default()
        });
        let graph = builder(GraphInputs {
            services: services.clone(),
            dispatcher: dispatcher.clone(),
            message_loop,
            hub,
            log,
            mark,
        })
        .unwrap();
        if let Some(saved) = saved {
            state
                .placement_store
                .save(&graph.window_placement_file.to_string_lossy(), &saved)
                .unwrap();
        }
        services.bind_dialog_services(graph.dialog_services.clone());
        *state.services.borrow_mut() = Some(services);
        *state.graph.borrow_mut() = Some(graph);
        state.create_main_window().unwrap();
        let main = state.main.borrow().clone().unwrap();
        let snapshot = marks.lock().unwrap().clone();
        body(&main, &snapshot);
        state.main.borrow_mut().take();
        // The remaining shutdown implementation is part B. Expire its weak callbacks first.
        drop(state);
        main.dispose();
        drop(main);
        dispatcher.shutdown();
    }

    #[test]
    fn text_renderer_ids_are_nonzero_and_unique_across_threads() {
        // new: W8-15c cache identities must never alias between renderer instances.
        let threads: Vec<_> = (0..4)
            .map(|_| std::thread::spawn(|| (0..64).map(|_| next_text_renderer_id()).collect::<Vec<_>>()))
            .collect();
        let ids: Vec<_> = threads.into_iter().flat_map(|thread| thread.join().unwrap()).collect();
        assert!(ids.iter().all(|id| *id != 0));
        assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(), ids.len());
    }

    fn placement(show_command: i32) -> WindowPlacementData {
        WindowPlacementData {
            length: 44,
            flags: 0,
            show_command,
            min_x: -1,
            min_y: -1,
            max_x: -1,
            max_y: -1,
            normal_left: 120,
            normal_top: 90,
            normal_right: 920,
            normal_bottom: 590,
        }
    }

    #[test]
    #[ignore = "native: needs desktop and WARP; startup window creation"]
    fn startup_cloak_is_requested_while_the_window_is_not_yet_visible() {
        // C# ShellStartupTests.Startup_CloakIsRequestedWhileTheWindowIsNotYetVisible.
        on_ui(|| {
            with_startup(None, |main, marks| {
                let index = |name: &str| marks.iter().position(|mark| mark == name).unwrap();
                assert!(index("cloakHidden") < index("windowShown"));
                assert!(index("windowShown") < index("mainWindowConstructed"));
                assert!(index("mainWindowConstructed") < index("windowRevealed"));
                assert!(main.has_surface());
                assert!(main.with_feature::<MainWindowOverlay, _>(|_| ()).is_some());
            })
        });
    }

    #[test]
    #[ignore = "native: needs desktop and WARP; saved placement"]
    fn startup_saved_placement_is_applied_before_the_window_is_shown() {
        // C# ShellStartupTests.Startup_SavedPlacement_IsAppliedBeforeTheWindowIsShown.
        on_ui(|| {
            with_startup(Some(placement(1)), |main, marks| {
                let before = marks
                    .iter()
                    .position(|mark| mark == "placementRestoredBeforeShow")
                    .unwrap();
                let shown = marks.iter().position(|mark| mark == "windowShown").unwrap();
                assert!(before < shown);
                let data = NativeWindowPlacement.try_get(main.hwnd()).unwrap();
                assert_eq!(
                    (data.normal_left, data.normal_top, data.normal_right, data.normal_bottom),
                    (120, 90, 920, 590)
                );
            })
        });
    }

    #[test]
    #[ignore = "native: needs desktop and WARP; maximized placement"]
    fn startup_saved_maximized_placement_reopens_maximized() {
        // C# ShellStartupTests.Startup_SavedMaximizedPlacement_ReopensMaximized.
        on_ui(|| {
            with_startup(Some(placement(3)), |main, _| {
                assert_eq!(NativeWindowPlacement.try_get(main.hwnd()).unwrap().show_command, 3);
            })
        });
    }
    #[test]
    #[ignore = "native: needs desktop and WARP; default outer size"]
    fn startup_without_saved_placement_opens_at_the_wpf_outer_size() {
        // C# ShellStartupTests.Startup_WithoutSavedPlacement_OpensAtTheWpfOuterSize.
        on_ui(|| {
            with_startup(None, |main, _| {
                let rect = pr_platform_win::win32::api::user32::get_window_rect(main.hwnd()).unwrap();
                let scale = main.window().dpi_scale();
                assert!((f64::from(rect.right - rect.left) - DEFAULT_WIDTH_DIP * scale).abs() <= 1.0);
                assert!((f64::from(rect.bottom - rect.top) - DEFAULT_HEIGHT_DIP * scale).abs() <= 1.0);
            })
        });
    }
}
