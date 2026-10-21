//! Port of `PhotoReview.Shell.Win32/Overlay/ShellMainWindow.Overlay.cs` (284 lines, baseline `d92d6aae`). Unit W8-15j.
//!
//! The overlay's attachment to the main window: building [`ShellOverlay`] from the view model and the window, drawing it after the image layer in the
//! same frame, feeding it model, settings and language changes, and tearing it down. The C# partial class `ShellMainWindow` becomes [`MainWindowOverlay`],
//! a field of the main window (W8-13g), which holds `Option<Rc<MainWindowOverlay>>` (C# `_overlay`) and forwards the frame, the view-model change sets
//! and `dispose` to it.
//!
//! The main window itself (W8-13g/13h) is NOT named here: everything the overlay needs from it is a trait or a plain parameter ([`OverlayWindowPort`],
//! [`OverlayViewModel`], [`UiWake`], [`LocalPoster`]); the main window implements or supplies them (cross-unit assumptions, `contracts/W8-15j.md` Q1 to Q4).
//!
//! Threads: `Localizer::CurrentChanged` and `SettingsStore.Changed` may be raised on any thread (`Send + Sync` handlers); everything else is the UI thread.
//! The cross-thread handlers only record the event in [`OverlayPending`] and call [`UiWake::wake`]; the owner then runs [`MainWindowOverlay::run_pending`] on
//! the UI thread (D-W8B4O-10).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pr_app::input::primitives::PointD;
use pr_app::overlay::model::OverlayModel;
use pr_app::overlay::tools_menu::OverlayCommands;
use pr_app::services::render::{DrawContext, TextRenderer};
use pr_app::viewmodels::main::state::{ChangeSet, MainProperty};
use pr_core::abstractions::{Log, UiPriority};
use pr_core::settings::app_settings::AppSettings;
use pr_core::settings::store::SettingsStore;
use pr_l10n::localizer::{CurrentChangedSubscription, Localizer};

use crate::host::contracts::{EventHandlerId, ShellWindowApi, ShellWindowEvent, WindowMessageHandler};
use crate::host::frame_clock::HostFrameClock;
use crate::host::main_window::core::{MainWindowFeature, MainWindowId, coalesced_wake};
use crate::host::shell_window::ShellWindow;
use crate::host::ui_dispatcher::Win32UiDispatcher;
use crate::shell_overlay::messages::{OverlayInput, OverlayMessageHandler};
use crate::shell_overlay::native::OverlayOs;
use crate::shell_overlay::overlay::state::{OverlayMenuHost, ShellOverlay, ShellOverlayOptions, overlay_clock};
use crate::shell_overlay::tooltips::{
    DeferredTooltips, LocalPoster, NativeTooltipHost, OverlayTooltips, TooltipFactory,
};

/// What the overlay needs from the shell window beyond [`ShellWindowApi`]: C# calls `ShellWindow.InsertMessageHandlerFirst`, which is on the concrete
/// class and not in `IShellWindow` (so not in `ShellWindowApi`). Implemented for [`ShellWindow`] below; tests implement it on a fake window.
pub trait OverlayWindowPort: ShellWindowApi {
    /// C# `InsertMessageHandlerFirst(handler)`: the handler goes to the FRONT of the chain.
    fn insert_message_handler_first(&self, handler: Rc<dyn WindowMessageHandler>);
}

impl OverlayWindowPort for ShellWindow {
    fn insert_message_handler_first(&self, handler: Rc<dyn WindowMessageHandler>) {
        ShellWindow::insert_message_handler_first(self, handler);
    }
}

/// What the overlay reads from and calls on `MainViewModel` (C# `_viewModel` uses in `ShellMainWindow.Overlay.cs` plus `ViewModelOverlayCommands`). The Rust
/// view model returns a `ChangeSet` from each mutating method and is not shared, so the main window (which owns it) implements this trait over its own
/// handle and applies the returned change sets itself (interior mutability, D-W8-1). All methods run on the UI thread.
pub trait OverlayViewModel {
    /// C# `OverlayModel.From(viewModel)`. The Rust owner of this mapping is `MainViewModel` (cross-unit question Q2 of `contracts/W8-15j.md`).
    fn overlay_model(&self) -> OverlayModel;
    /// C# `_viewModel.Settings`.
    fn settings(&self) -> Arc<AppSettings>;
    /// C# `_viewModel.Settings = settings`.
    fn set_settings(&self, settings: Arc<AppSettings>);
    /// C# `_viewModel.RefreshLocalizedText()`.
    fn refresh_localized_text(&self);
    /// C# `OnOverlayCommandFailed` body: `_log.Error($"{actionName} failed", exception)` and `StatusText = StatusFormatter.ActionFailed(actionName, describe(error))`.
    fn report_action_failed(&self, action_name: &str, message: &str);
    /// C# `PickAndOpenFolderAsync()`.
    fn pick_and_open_folder(&self);
    /// C# `ShowSettings()`.
    fn show_settings(&self);
    /// C# `ShowSkippedFiles()`.
    fn show_skipped_files(&self);
    /// C# `ShowRecovery()`.
    fn show_recovery(&self);
    /// C# `ShowDiagnostics()`.
    fn show_diagnostics(&self);
    /// C# `ShowBenchmark()`.
    fn show_benchmark(&self);
    /// C# `ClearCacheAsync()`.
    fn clear_cache(&self);
    /// C# `RemoveDuplicatesAsync(numbered)`.
    fn remove_duplicates(&self, numbered: bool);
}

/// A thread-safe "something is pending for the overlay" signal (cross-unit assumption Q1). `wake` may be called from any thread and must be cheap and
/// coalescing; the implementation (main window, W8-13g) posts ONE `Send` job to the UI dispatcher whose body ends in [`MainWindowOverlay::run_pending`].
pub trait UiWake: Send + Sync {
    /// Asks the UI thread to run `run_pending` soon.
    fn wake(&self);
}

/// Cross-thread mailbox between the `Send + Sync` handlers (`Localizer`, `SettingsStore`) and the UI thread.
#[derive(Debug, Default)]
pub struct OverlayPending {
    /// A language change was signalled.
    pub language_changed: AtomicBool,
    /// The latest settings of a `SettingsStore.Changed` (older ones are dropped: the overlay re-evaluates against the latest anyway).
    pub settings: Mutex<Option<Arc<AppSettings>>>,
}

/// C# `ViewModelOverlayCommands(MainViewModel viewModel, Func<Task> fit)`: the toolbar and Tools-menu commands on the view model.
pub struct ViewModelOverlayCommands {
    view_model: Rc<dyn OverlayViewModel>,
    /// The Fit command shared with the input group (the same operation counter as `PointerInputController`, cross-unit question Q3).
    fit: Box<dyn FnMut()>,
}

impl fmt::Debug for ViewModelOverlayCommands {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewModelOverlayCommands").finish_non_exhaustive()
    }
}

impl ViewModelOverlayCommands {
    /// C# primary constructor.
    pub fn new(view_model: Rc<dyn OverlayViewModel>, fit: Box<dyn FnMut()>) -> Self {
        Self { view_model, fit }
    }
}

impl OverlayCommands for ViewModelOverlayCommands {
    /// `open_folder` -> `pick_and_open_folder`; `show_settings`, `show_skipped_files`, `show_recovery`, `show_diagnostics`, `show_benchmark`,
    /// `clear_cache` map to the view-model method of the same name; `fit` -> the `fit` closure; `remove_duplicates(numbered)` -> `remove_duplicates(numbered)`.
    fn open_folder(&mut self) {
        self.view_model.pick_and_open_folder();
    }

    fn show_settings(&mut self) {
        self.view_model.show_settings();
    }

    fn fit(&mut self) {
        (self.fit)();
    }

    fn show_skipped_files(&mut self) {
        self.view_model.show_skipped_files();
    }

    fn show_recovery(&mut self) {
        self.view_model.show_recovery();
    }

    fn show_diagnostics(&mut self) {
        self.view_model.show_diagnostics();
    }

    fn show_benchmark(&mut self) {
        self.view_model.show_benchmark();
    }

    fn clear_cache(&mut self) {
        self.view_model.clear_cache();
    }

    fn remove_duplicates(&mut self, numbered: bool) {
        self.view_model.remove_duplicates(numbered);
    }
}

/// Everything `attach` needs (C# `AttachOverlay(IServiceProvider services, IFrameClock frameClock)` read these from the service provider and the window's
/// fields). No hidden defaults.
pub struct OverlayDeps {
    /// The shell window (C# `_window`); the real one is `Rc<ShellWindow>` through [`OverlayWindowPort`].
    pub window: Rc<dyn OverlayWindowPort>,
    /// The window's frame clock (C# `frameClock`).
    pub clock: Rc<dyn HostFrameClock>,
    /// The view model (C# `_viewModel`).
    pub view_model: Rc<dyn OverlayViewModel>,
    /// The DirectWrite renderer (C# `new DWriteTextRenderer()`), owned by the caller's composition so its lifetime and `text_renderer_id` are explicit.
    pub text: Rc<dyn TextRenderer>,
    /// Identity of `text` for the layout caches (a fresh id per renderer).
    pub text_renderer_id: u64,
    /// The current localizer (C# static `Localizer.Current`).
    pub texts: Arc<Localizer>,
    /// C# `services.GetService<IPopupMenuHost>() ?? new NativePopupMenuHost(() => Hwnd)`: the menu service, adapted (`None`: the Tools button does nothing).
    pub menus: Option<Rc<dyn OverlayMenuHost>>,
    /// C# `services.GetService<SettingsStore>()`.
    pub settings_store: Option<Arc<SettingsStore>>,
    /// Posts the deferred tooltip creation (C# `_dispatcher.Post(action, UiPriority.Background)`) and the model refresh (`Render`).
    pub poster: Rc<dyn LocalPoster>,
    /// The cross-thread wake (see [`UiWake`]).
    pub wake: Arc<dyn UiWake>,
    /// The Fit command (C# `FitHandler ?? own FitViewController`; the own-controller fallback is not ported, `contracts/W8-15j.md` Q3).
    pub fit: Box<dyn FnMut()>,
    /// Factory of the real tooltip control; `None` = `NativeTooltipHost::new(window.hwnd())`. Tests pass a fake control.
    pub tooltip_factory: Option<TooltipFactory>,
    /// The OS seam of the message handler and of the Tools-menu anchor.
    pub os: Rc<dyn OverlayOs>,
    /// C# `_log`.
    pub log: Arc<dyn Log>,
}

impl fmt::Debug for OverlayDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverlayDeps")
            .field("text_renderer_id", &self.text_renderer_id)
            .field("menus", &self.menus.is_some())
            .field("settings_store", &self.settings_store.is_some())
            .field("tooltip_factory", &self.tooltip_factory.is_some())
            .finish_non_exhaustive()
    }
}

/// The overlay of one main window (the C# fields `_overlay`, `_overlayText`, `_overlayHandler`, `_overlaySettings`, `_onOverlaySettingsChanged`,
/// `_onLanguageChanged`, `_onOverlaySourceChanged`, `_overlayRefreshPosted`). UI thread only.
pub struct MainWindowOverlay {
    overlay: Rc<ShellOverlay>,
    #[allow(dead_code, reason = "owned so the tooltips live as long as the overlay")]
    tooltips: Rc<DeferredTooltips>,
    #[allow(
        dead_code,
        reason = "owned: the window's chain holds it too; kept for the lifetime of the overlay"
    )]
    handler: Rc<OverlayMessageHandler>,
    window: Rc<dyn OverlayWindowPort>,
    view_model: Rc<dyn OverlayViewModel>,
    poster: Rc<dyn LocalPoster>,
    os: Rc<dyn OverlayOs>,
    pending: Arc<OverlayPending>,
    settings_store: Option<Arc<SettingsStore>>,
    /// Id from `SettingsStore::subscribe_changed`.
    settings_subscription: Cell<Option<usize>>,
    /// Dropping it unsubscribes from `Localizer` (`pr_l10n::localizer::on_current_changed`).
    language_subscription: RefCell<Option<CurrentChangedSubscription>>,
    /// The `Activated` and `Deactivated` subscriptions on the window.
    window_subscriptions: RefCell<Vec<EventHandlerId>>,
    /// C# `_overlayRefreshPosted`.
    refresh_posted: Cell<bool>,
    disposed: Cell<bool>,
    weak_self: Weak<MainWindowOverlay>,
}

impl fmt::Debug for MainWindowOverlay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWindowOverlay")
            .field("refresh_posted", &self.refresh_posted.get())
            .field("disposed", &self.disposed.get())
            .finish_non_exhaustive()
    }
}

impl MainWindowOverlay {
    /// C# `AttachOverlay` (lines 52 to 108), in C# order: the deferred tooltips (factory posted at `Background`); `ShellOverlay::new` with the options built from
    /// `deps` (settings = the view model's, `request_render` = `window.invalidate()`, `is_window_active` = `window.is_active()`,
    /// `client_origin_on_screen_dip` = [`MainWindowOverlay::client_origin_on_screen_dip`], `on_command_failed` = [`MainWindowOverlay::on_command_failed`],
    /// `on_menu_failed` = report as `"ToolsMenu"`, commands = [`ViewModelOverlayCommands`]); `overlay.update(view_model.overlay_model())`; the
    /// [`OverlayMessageHandler`] is inserted FIRST in the window's chain; subscribe `Activated`/`Deactivated` of the window; subscribe the settings store (its
    /// handler records the settings in [`OverlayPending`] and calls `wake`); subscribe the localizer (records the flag and calls `wake`). The C#
    /// `InvalidOperationException("The overlay is already attached.")` is the owner's `Option` field (a second `attach` is a programming error of the owner).
    pub fn attach(deps: OverlayDeps) -> Rc<MainWindowOverlay> {
        let OverlayDeps {
            window,
            clock,
            view_model,
            text,
            text_renderer_id,
            texts,
            menus,
            settings_store,
            poster,
            wake,
            fit,
            tooltip_factory,
            os,
            log: _,
        } = deps;
        let pending = Arc::new(OverlayPending::default());

        Rc::new_cyclic(|weak: &Weak<MainWindowOverlay>| {
            // Deferred tooltips: the control is created by a Background job (C# `_dispatcher.Post(action, UiPriority.Background)`).
            let factory: TooltipFactory = tooltip_factory.unwrap_or_else(|| {
                let window = Rc::clone(&window);
                Box::new(move || {
                    NativeTooltipHost::new(window.hwnd()).map(|host| Rc::new(host) as Rc<dyn OverlayTooltips>)
                })
            });
            let tooltips = Rc::new(DeferredTooltips::new(factory, poster.as_ref()));

            let overlay = ShellOverlay::new(ShellOverlayOptions {
                text,
                text_renderer_id,
                texts,
                clock: overlay_clock(clock),
                settings: {
                    let view_model = Rc::clone(&view_model);
                    Box::new(move || view_model.settings())
                },
                commands: Box::new(ViewModelOverlayCommands::new(Rc::clone(&view_model), fit)),
                request_render: {
                    let window = Rc::clone(&window);
                    Box::new(move || window.invalidate())
                },
                is_window_active: {
                    let window = Rc::clone(&window);
                    Box::new(move || window.is_active())
                },
                menus,
                tooltips: Some(Rc::clone(&tooltips) as Rc<dyn OverlayTooltips>),
                client_origin_on_screen_dip: {
                    let weak = weak.clone();
                    Some(Box::new(move || {
                        weak.upgrade()
                            .map_or(PointD { x: 0.0, y: 0.0 }, |this| this.client_origin_on_screen_dip())
                    }))
                },
                on_command_failed: {
                    let weak = weak.clone();
                    Some(Box::new(move |name, message| {
                        if let Some(this) = weak.upgrade() {
                            this.on_command_failed(name, message);
                        }
                    }))
                },
                on_menu_failed: {
                    let weak = weak.clone();
                    Some(Box::new(move |error| {
                        if let Some(this) = weak.upgrade() {
                            this.on_command_failed("ToolsMenu", &error.to_string());
                        }
                    }))
                },
                timestamp: None,
                ticks_per_second: None,
                animator_ticks_per_second: None,
            });
            overlay.update(view_model.overlay_model());

            // The handler goes to the FRONT of the chain; it relays to the same deferred tooltips as the overlay.
            let handler = Rc::new(OverlayMessageHandler::new(
                Rc::clone(&overlay) as Rc<dyn OverlayInput>,
                Rc::clone(&window) as Rc<dyn ShellWindowApi>,
                Some(Rc::clone(&tooltips) as Rc<dyn OverlayTooltips>),
                Rc::clone(&os),
            ));
            OverlayWindowPort::insert_message_handler_first(
                window.as_ref(),
                Rc::clone(&handler) as Rc<dyn WindowMessageHandler>,
            );

            let activated = {
                let overlay = Rc::downgrade(&overlay);
                ShellWindowApi::subscribe(
                    window.as_ref(),
                    ShellWindowEvent::Activated,
                    Box::new(move || {
                        if let Some(overlay) = overlay.upgrade() {
                            overlay.window_activated();
                        }
                    }),
                )
            };
            let deactivated = {
                let overlay = Rc::downgrade(&overlay);
                ShellWindowApi::subscribe(
                    window.as_ref(),
                    ShellWindowEvent::Deactivated,
                    Box::new(move || {
                        if let Some(overlay) = overlay.upgrade() {
                            overlay.window_deactivated();
                        }
                    }),
                )
            };

            // Cross-thread handlers: only `Arc<OverlayPending>` and `Arc<dyn UiWake>` are captured; the result is always `Ok(())`.
            let settings_subscription = settings_store.as_ref().map(|store| {
                let pending = Arc::clone(&pending);
                let wake = Arc::clone(&wake);
                store.subscribe_changed(Box::new(move |settings| {
                    *pending.settings.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(settings));
                    wake.wake();
                    Ok(())
                }))
            });
            let language_subscription = {
                let pending = Arc::clone(&pending);
                let wake = Arc::clone(&wake);
                pr_l10n::localizer::on_current_changed(move || {
                    pending.language_changed.store(true, Ordering::SeqCst);
                    wake.wake();
                })
            };

            MainWindowOverlay {
                overlay,
                tooltips,
                handler,
                window,
                view_model,
                poster,
                os,
                pending,
                settings_store,
                settings_subscription: Cell::new(settings_subscription),
                language_subscription: RefCell::new(Some(language_subscription)),
                window_subscriptions: RefCell::new(vec![activated, deactivated]),
                refresh_posted: Cell::new(false),
                disposed: Cell::new(false),
                weak_self: weak.clone(),
            }
        })
    }

    /// C# `Overlay` (null before attach and after dispose: the owner's `Option`).
    pub fn overlay(&self) -> &Rc<ShellOverlay> {
        &self.overlay
    }

    /// C# `DrawOverlay(context)`: `overlay.render(dc, window.client_size_dip(), window.dpi_scale())`; the host-twin `SizeD` is converted to the
    /// `pr_app` one. Called from the frame callback of the main window between `BeginDraw` and `EndDraw`, after the image layer.
    pub fn draw_overlay(&self, dc: &mut dyn DrawContext) {
        self.overlay
            .render(dc, self.window.client_size_dip(), self.window.dpi_scale());
    }

    /// C# `OnOverlaySourceChanged` for one change set of the view model: nothing when disposed; when `changes.main` holds `CurrentIndex` or `CurrentImage`
    /// (image navigation) `overlay.note_activity()`; when `main`, `overlay` (info overlay) or `compare` changed, `mark_model_dirty()`.
    pub fn on_view_model_changed(&self, changes: &ChangeSet) {
        if self.disposed.get() {
            return;
        }
        if changes.main.contains(MainProperty::CurrentIndex) || changes.main.contains(MainProperty::CurrentImage) {
            self.overlay.note_activity();
        }
        if !changes.main.is_empty() || !changes.overlay.is_empty() || !changes.compare.is_empty() {
            self.mark_model_dirty();
        }
    }

    /// C# `MarkOverlayModelDirty()`: posts ONE `refresh_model` at `UiPriority::Render` (coalesced by `refresh_posted`); nothing when disposed.
    pub fn mark_model_dirty(&self) {
        if self.disposed.get() || self.refresh_posted.replace(true) {
            return;
        }
        let weak = self.weak_self.clone();
        self.poster.post(
            UiPriority::Render,
            Box::new(move || {
                if let Some(this) = weak.upgrade() {
                    this.refresh_model();
                }
            }),
        );
    }

    /// C# `RefreshOverlayModel()` (the posted job): `refresh_posted = false`; when not disposed, `overlay.update(view_model.overlay_model())`.
    pub fn refresh_model(&self) {
        // Cleared FIRST: a change made inside `update` (a render request) can post again.
        self.refresh_posted.set(false);
        if !self.disposed.get() {
            self.overlay.update(self.view_model.overlay_model());
        }
    }

    /// The UI-thread end of [`UiWake::wake`]: takes the pending settings (C# `_onOverlaySettingsChanged`: `view_model.set_settings(settings)`, then
    /// `overlay.settings_changed()`, then `mark_model_dirty()`; the view model must see the new settings before the overlay re-evaluates) and the pending
    /// language flag (`on_language_changed`).
    pub fn run_pending(&self) {
        if self.disposed.get() {
            return;
        }
        let settings = self
            .pending
            .settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(settings) = settings {
            self.view_model.set_settings(settings);
            self.overlay.settings_changed();
            self.mark_model_dirty();
        }
        if self.pending.language_changed.swap(false, Ordering::SeqCst) {
            self.on_language_changed();
        }
    }

    /// C# `OnLanguageChangedOnUi()`: nothing when disposed; `view_model.refresh_localized_text()`, `overlay.apply_localization(&Localizer::current())`,
    /// `overlay.update(view_model.overlay_model())` (in that order).
    pub fn on_language_changed(&self) {
        if self.disposed.get() {
            return;
        }
        self.view_model.refresh_localized_text();
        self.overlay.apply_localization(&Localizer::current());
        self.overlay.update(self.view_model.overlay_model());
    }

    /// C# `ClientOriginOnScreenDip()`: [`client_origin_dip`] of `os.client_origin_on_screen(window.hwnd())` and `window.dpi_scale()`.
    pub fn client_origin_on_screen_dip(&self) -> PointD {
        client_origin_dip(
            self.os.client_origin_on_screen(self.window.hwnd()),
            self.window.dpi_scale(),
        )
    }

    /// C# `OnOverlayCommandFailed(actionName, exception)`: `view_model.report_action_failed(action_name, message)`; nothing when disposed.
    pub fn on_command_failed(&self, action_name: &str, message: &str) {
        if self.disposed.get() {
            return;
        }
        self.view_model.report_action_failed(action_name, message);
    }

    /// C# `DisposeOverlay()`: idempotent. Unsubscribes the localizer, the settings store, the window events (the view-model subscription of C# does not exist:
    /// the owner stops calling `on_view_model_changed`), then disposes the overlay (timers, animator, tooltips, text layouts). The message handler stays in the
    /// window's chain (C# never removed it) but its overlay is disposed, so it only answers `None`.
    pub fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        drop(self.language_subscription.borrow_mut().take());
        if let (Some(store), Some(id)) = (&self.settings_store, self.settings_subscription.take()) {
            store.unsubscribe_changed(id);
        }
        let subscriptions = std::mem::take(&mut *self.window_subscriptions.borrow_mut());
        for id in subscriptions {
            ShellWindowApi::unsubscribe(self.window.as_ref(), id);
        }
        self.overlay.dispose();
    }
}

/// C# `ClientOriginOnScreenDip` arithmetic: `(x / dpi, y / dpi)` with `dpi = dpi_scale <= 0 ? 1 : dpi_scale`. Pure.
pub fn client_origin_dip(origin_px: (i32, i32), dpi_scale: f64) -> PointD {
    let dpi = if dpi_scale <= 0.0 { 1.0 } else { dpi_scale };
    PointD {
        x: f64::from(origin_px.0) / dpi,
        y: f64::from(origin_px.1) / dpi,
    }
}

impl MainWindowFeature for MainWindowOverlay {
    fn name(&self) -> &'static str {
        "overlay"
    }

    /// At the place of C# `DisposeOverlay()` in `Dispose()`.
    fn dispose(&self) {
        MainWindowOverlay::dispose(self);
    }

    /// After the scroll bars, in the same frame as the image.
    fn draw_overlay(&self, context: &mut dyn DrawContext) {
        MainWindowOverlay::draw_overlay(self, context);
    }

    /// The single entry that replaces the three C# `PropertyChanged` subscriptions.
    fn on_changes(&self, changes: &ChangeSet) {
        self.on_view_model_changed(changes);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The production [`UiWake`] (contract Q1): `wake()` posts ONE `Send` job at `Render` priority (coalesced while it is queued) whose body finds the main
/// window on the UI thread, takes its registered [`MainWindowOverlay`] feature and runs [`MainWindowOverlay::run_pending`]. A wake that finds no live
/// window, or a window without the feature, is dropped.
pub struct WindowUiWake {
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl WindowUiWake {
    /// `window`: the id of the main window, or `None` for "the main window of the thread the job runs on".
    pub fn new(dispatcher: Win32UiDispatcher, window: Option<MainWindowId>) -> Self {
        let wake = coalesced_wake(dispatcher, window, UiPriority::Render, |inner| {
            inner.with_feature::<MainWindowOverlay, ()>(MainWindowOverlay::run_pending);
        });
        Self { wake }
    }
}

impl fmt::Debug for WindowUiWake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowUiWake").finish_non_exhaustive()
    }
}

impl UiWake for WindowUiWake {
    fn wake(&self) {
        (self.wake)();
    }
}
