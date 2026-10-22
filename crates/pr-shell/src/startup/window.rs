//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 394-461 (`CreateMainWindow`, baseline `d92d6aae`) plus the `ViewportFactory` that the C# `ShellMainWindow` constructor built
//! itself (D-W8B4M-4, D-W8B4X-6) and the `AttachOverlay` call (`ShellMainWindow.Overlay.cs` 52-108 reads the services; the Rust `OverlayDeps` takes them, D-W8B4X-17). Unit W8-17e.
//! Contract: `contracts/W8-17e.md`.
//!
//! Order is behaviour (perf/startup-first-image): the surface is never seen unpainted (white) and the window is never seen at 1200x800 and then jumping to its placement: cloak from the moment
//! the HWND exists, placement applied while the window is hidden, THEN `ShowWindow`, then the view model and the Direct2D surface, then the first frame NOW so the cloak is lifted after the first
//! present instead of waiting for an image.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::rc::Rc;
use std::sync::Arc;

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
    todo!()
}

/// A fresh identity for a `DWriteTextRenderer` (W8-15c O2: the layout caches key on it): a process-wide `AtomicU64` counter starting at 1.
pub fn next_text_renderer_id() -> u64 {
    todo!()
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
    pub(super) fn create_main_window(self: &Rc<Self>) -> Result<(), StartupError> {
        todo!()
    }

    /// C# `AttachOverlay(services, frameClock)` as the caller of `MainWindowOverlay::attach` (D-W8B4X-17): builds the `OverlayDeps` and registers the result with `main.add_feature`:
    /// `text` = `Rc::new(DWriteTextRenderer::new(None, &pr_l10n::loader::current_ui_culture().name, 256))` (`Err(TextError)` is `StartupError::Overlay(text)`: the C# constructor threw and the start-up failed),
    /// `text_renderer_id = next_text_renderer_id()`, `texts = Localizer::current()`, `window = Rc<ShellWindow>` as `Rc<dyn OverlayWindowPort>`, `clock = frame_clock`, `view_model =
    /// Rc::new(MainWindowOverlayViewModel::new(..))`, `menus = Some(Rc::new(NativePopupMenuHost::new(owner closure).with_log(log)))` (the SECOND popup host, D-W8B4X-9 Q4: its own owner closure), `settings_store =
    /// Some(graph.settings_store.clone())`, `poster = Rc::new(UiLocalPoster::new(dispatcher))`, `wake = Arc::new(WindowUiWake::new(dispatcher, None))`, `fit` = `move || { if let Some(window) = slot
    /// .upgrade().and_then(..) { window.with_feature::<MainWindowInputFeature, _>(|f| f.input().input().apply_fit()); } }` (the Fit closure is mandatory: without it the Fit button does nothing),
    /// `tooltip_factory: None`, `os = Rc::new(Win32OverlayOs)`, `log`.
    pub(super) fn attach_overlay(self: &Rc<Self>, main: &Rc<MainWindow>) -> Result<(), StartupError> {
        todo!()
    }
}
