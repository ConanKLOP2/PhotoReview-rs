//! Port of `PhotoReview.Shell.Win32/ShellMainWindow.cs` lines 226-452 (baseline `d92d6aae`): the `IForwardedOpenTarget` members, the layout and
//! decode-box helpers, the view-model/viewer change handlers, the window event handlers, closing, closed, `ReleaseRenderer` and `Dispose`. Unit W8-13h.
//! Contract: `contracts/W8-13h.md`.
//!
//! The C# window subscribed to `PropertyChanged` of the view model and the viewer. The Rust view models return a `ChangeSet` instead (D-W8-1), so the
//! subscriptions become ONE entry point, [`MainWindow::apply_changes`], that every caller of a view-model method feeds (D-W8B4M-2). The fan-out order of
//! one `ChangeSet` is fixed here (see `apply_changes`) and lives in [`fan_out_changes`], a function over the small [`ChangeSteps`] trait, so that the order
//! can be pinned without a window.

use std::rc::Rc;
use std::sync::Arc;

use pr_app::coordinators::traits::IPreloadController;
use pr_app::coordinators::zoom_detail::WakeFn;
use pr_app::viewmodels::main::{ChangeSet, MainProperty, MainViewModel};
use pr_app::viewmodels::viewer_state::{ViewerChanges, ViewerProperty};
use pr_core::abstractions::{UiAction, UiPriority};
use pr_imaging::adaptive_preview::{InvalidPreviewArgument, calculate_target_decode_box};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::pixels::PixelBuffer;

use super::core::{
    FALLBACK_VIEWPORT_HEIGHT, FALLBACK_VIEWPORT_WIDTH, MainWindow, MainWindowError, MainWindowFeature, MainWindowInner,
    PREVIEW_QUALITY_MULTIPLIER, ViewportContent, coalesced_wake, post_to_window, unregister_window,
};
use crate::host::contracts::{CancelEventArgs, ShellWindowApi};
use crate::host::ui_dispatcher::Win32UiDispatcher;
use crate::win32::api::user32::show_window;
use crate::win32::messages::SW_RESTORE;
use crate::win32::startup::{is_iconic, set_foreground_window};

/// The wake function for the view model's `CompletionInbox` (C# resumed `await`s on the UI synchronisation context): `coalesced_wake(dispatcher, None,
/// UiPriority::Normal, pump_view_model)` (D-W8B4M-6): ONE `Send` action while one is queued, found by "the current main window of the UI thread". The
/// composition root builds the inbox with it BEFORE the window exists, which is why the window is looked up at run time. A wake that finds no window
/// (closed or not yet built) is dropped; the first `pump_view_model` after the window exists drains whatever arrived.
pub fn view_model_wake(dispatcher: Win32UiDispatcher) -> WakeFn {
    coalesced_wake(dispatcher, None, UiPriority::Normal, MainWindowInner::pump_view_model)
}

// ---- the change fan-out ---------------------------------------------------------------------------------------------------------------------------

/// The steps of one change fan-out, as the window performs them (the window implements it over its own handlers; a test implements it with a recorder).
/// Every method runs with NO `RefCell` borrow of the view model or the viewer held by the caller.
pub trait ChangeSteps {
    /// C# `_disposed`: a disposed window ignores the whole fan-out.
    fn is_disposed(&self) -> bool;
    /// 13j `on_image_changing(is_file_change)`.
    fn on_image_changing(&self, is_file_change: bool);
    /// 13j `drain_pending_images()`.
    fn drain_pending_images(&self);
    /// `viewer.is_fit()` (a short borrow inside).
    fn viewer_is_fit(&self) -> bool;
    /// C# `_dispatcher.Post(UpdateFitSizeIfAlive, Render)`.
    fn post_update_fit_size(&self);
    /// C# `_window.SetTitle(_viewModel.FolderTitle)`.
    fn refresh_title(&self);
    /// 13j `on_navigation_changed()`.
    fn on_navigation_changed(&self);
    /// The features' `on_current_index_changed(index)` with the current index.
    fn notify_current_index(&self);
    /// 13k `on_viewer_fullscreen_changed()`.
    fn on_viewer_fullscreen_changed(&self);
    /// `on_viewer_changed()` (invalidate layout and frame).
    fn on_viewer_changed(&self);
    /// 13j `on_compare_changed()`.
    fn on_compare_changed(&self);
    /// Every feature's `on_changes(changes)`.
    fn notify_features(&self, changes: &ChangeSet);
}

/// The fan-out of one `ChangeSet`, in the C# raise order of one view-model call; does nothing when the window is disposed.
///
/// 1. `image_changing` before any image is handled; 2. the images the hub reported during the call (and, for `CurrentImage` in Fit, the posted fit update);
/// 3. `FolderTitle`; 4. `CurrentIndex`; 5. the viewer (see [`fan_out_viewer`]); 6. the compare view model; 7. the features.
pub fn fan_out_changes(steps: &dyn ChangeSteps, changes: &ChangeSet) {
    if steps.is_disposed() {
        return;
    }
    if let Some(args) = changes.image_changing {
        steps.on_image_changing(args.is_file_change);
    }
    steps.drain_pending_images();
    if changes.main.contains(MainProperty::CurrentImage) && steps.viewer_is_fit() {
        steps.post_update_fit_size();
    }
    if changes.main.contains(MainProperty::FolderTitle) {
        steps.refresh_title();
    }
    if changes.main.contains(MainProperty::CurrentIndex) {
        steps.on_navigation_changed();
        steps.notify_current_index();
    }
    fan_out_viewer(steps, changes.viewer);
    if !changes.compare.is_empty() {
        steps.on_compare_changed();
    }
    if !steps.is_disposed() {
        steps.notify_features(changes);
    }
}

/// The viewer part of the fan-out. C# order: the windowing handler of `IsFullscreen` (subscribed by `RegisterWindowing`, inside `RegisterFeatures`) runs BEFORE
/// the core's `OnViewerChanged` (subscribed after `RegisterFeatures`), so fullscreen first, then the generic invalidation.
pub fn fan_out_viewer(steps: &dyn ChangeSteps, viewer: ViewerChanges) {
    if viewer.is_empty() || steps.is_disposed() {
        return;
    }
    if viewer.contains(ViewerProperty::IsFullscreen) {
        steps.on_viewer_fullscreen_changed();
    }
    steps.on_viewer_changed();
}

/// [`ChangeSteps`] over the real window.
struct InnerSteps<'a>(&'a MainWindowInner);

impl ChangeSteps for InnerSteps<'_> {
    fn is_disposed(&self) -> bool {
        self.0.is_disposed()
    }

    fn on_image_changing(&self, is_file_change: bool) {
        self.0.on_image_changing(is_file_change);
    }

    fn drain_pending_images(&self) {
        self.0.drain_pending_images();
    }

    fn viewer_is_fit(&self) -> bool {
        self.0.viewer.borrow().is_fit()
    }

    fn post_update_fit_size(&self) {
        post_to_window(
            &self.0.dispatcher,
            self.0.id,
            UiPriority::Render,
            MainWindowInner::update_fit_size_if_alive,
        );
    }

    fn refresh_title(&self) {
        // The borrow ends before `set_title` (it sends `WM_SETTEXT` and re-enters the window procedure).
        let title = self.0.view_model.borrow().folder_title().to_owned();
        if let Err(error) = self.0.window.set_title(&title) {
            self.0.log.error("SetWindowText failed", Some(&error));
        }
    }

    fn on_navigation_changed(&self) {
        self.0.on_navigation_changed();
    }

    fn notify_current_index(&self) {
        let index = self.0.view_model.borrow().current_index();
        for feature in self.0.feature_snapshot() {
            feature.on_current_index_changed(index);
        }
    }

    fn on_viewer_fullscreen_changed(&self) {
        self.0.on_viewer_fullscreen_changed();
    }

    fn on_viewer_changed(&self) {
        self.0.on_viewer_changed();
    }

    fn on_compare_changed(&self) {
        self.0.on_compare_changed();
    }

    fn notify_features(&self, changes: &ChangeSet) {
        for feature in self.0.feature_snapshot() {
            feature.on_changes(changes);
        }
    }
}

/// Stops the preload controller when dropped (C# `finally { (PreloadController as IDisposable)?.Dispose(); }`), also while a panic unwinds.
struct StopPreload(Option<Arc<dyn IPreloadController>>);

impl Drop for StopPreload {
    fn drop(&mut self) {
        if let Some(preload) = self.0.take() {
            preload.cancel();
            preload.dispose();
        }
    }
}

/// Clears a flag when dropped (the pump guard).
struct ClearOnDrop<'a>(&'a std::cell::Cell<bool>);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

// ---- pure helpers -------------------------------------------------------------------------------------------------------------------------------

/// C# `UpdateTargetDecodeBox`'s calculation: the viewport size in DIP, where a side of 1 DIP or less (not measured yet) is replaced by the fallback (2200 x
/// 1400), at `dpi` and with the 15 % quality margin. `Err` only for an invalid `dpi` (C# throws; the caller logs and keeps the old box).
pub fn target_decode_box_for(
    viewport_width: f64,
    viewport_height: f64,
    dpi: f64,
) -> Result<DecodeBox, InvalidPreviewArgument> {
    let width = if viewport_width > 1.0 {
        viewport_width
    } else {
        FALLBACK_VIEWPORT_WIDTH
    };
    let height = if viewport_height > 1.0 {
        viewport_height
    } else {
        FALLBACK_VIEWPORT_HEIGHT
    };
    calculate_target_decode_box(width, height, dpi, PREVIEW_QUALITY_MULTIPLIER)
}

/// C# `ContentFromViewer`'s bitmap size: the width and height of the image when there is one that is not disposed, else `(0, 0)`.
pub fn bitmap_size(image: Option<&PixelBuffer>) -> (f64, f64) {
    match image {
        Some(image) if !image.is_disposed() => (f64::from(image.width()), f64::from(image.height())),
        _ => (0.0, 0.0),
    }
}

// ---- the public window API --------------------------------------------------------------------------------------------------------------------------

impl MainWindow {
    /// C# `IForwardedOpenTarget.IsMinimized`: `hwnd != 0 && IsIconic(hwnd)`.
    pub fn is_minimized(&self) -> bool {
        let hwnd = self.inner.window.hwnd();
        hwnd != 0 && is_iconic(hwnd)
    }

    /// C# `IForwardedOpenTarget.Restore()`: `hwnd != 0` -> `ShowWindow(hwnd, SW_RESTORE)` (back to the pre-minimise state, never forced to Normal, R7-5).
    pub fn restore(&self) {
        let hwnd = self.inner.window.hwnd();
        if hwnd != 0 {
            let _ = show_window(hwnd, SW_RESTORE);
        }
    }

    /// C# `IForwardedOpenTarget.Activate()`: a destroyed window is `Err(WindowDestroyed)` ("The main window is already destroyed."); else
    /// `SetForegroundWindow(hwnd)` (the OS may refuse; ignored).
    pub fn activate(&self) -> Result<(), MainWindowError> {
        if self.inner.window.is_destroyed() {
            return Err(MainWindowError::WindowDestroyed);
        }
        let _ = set_foreground_window(self.inner.window.hwnd());
        Ok(())
    }

    /// C# `OpenPathAsync(path)` (also the forwarded-open entry): `vm.open_path(path)` and the returned `ChangeSet` through `apply_changes`. The C# `Task` is the
    /// view model's step machine, finished by `pump_view_model`.
    pub fn open_path(&self, path: &str) {
        self.update_view_model(|vm| vm.open_path(path));
    }

    /// THE replacement of the C# `PropertyChanged`/`ImageChanging`/`CatalogChanged` subscriptions (D-W8B4M-2). Does nothing when disposed. Fan-out, in this
    /// order (the C# raise order for one view-model call), see [`fan_out_changes`]:
    /// 1. `image_changing` -> `on_image_changing` (13j; BEFORE the image itself, C# raised it before the hub told the window about the image);
    /// 2. `drain_pending_images()` (13j): every image the hub reported during the call, FIFO, each through `handle_image_changed` (this renders the first
    ///    image in this very call). When `main` contains `CurrentImage` and the viewer is in Fit: `post_to_window(.., UiPriority::Render,
    ///    update_fit_size_if_alive)` (C# `BeginInvoke(UpdateFitSize, Render)`);
    /// 3. `main` contains `FolderTitle` -> `window.set_title(vm.folder_title())` (an error is logged);
    /// 4. `main` contains `CurrentIndex` -> `on_navigation_changed()` (13j) and the features' `on_current_index_changed(index)` (14c);
    /// 5. `viewer` is not empty -> `IsFullscreen` -> `on_viewer_fullscreen_changed()` (13k), then `on_viewer_changed()` (any viewer property: invalidate layout
    ///    and frame; the C# windowing handler was subscribed before the core's);
    /// 6. `compare` is not empty -> `on_compare_changed()` (13j);
    /// 7. every feature's `on_changes(&ChangeSet)` (overlay refresh, input, menus).
    ///
    /// The view-model borrow is NEVER held while a handler runs.
    pub fn apply_changes(&self, changes: &ChangeSet) {
        self.inner.apply_changes(changes);
    }

    /// Changes made on the viewer alone (zoom by wheel, pan reset): the viewer part of `apply_changes` (step 5) for callers that do not hold a `ChangeSet`.
    pub fn apply_viewer_changes(&self, changes: ViewerChanges) {
        self.inner.apply_viewer_changes(changes);
    }

    /// Runs `f` with the view model borrowed mutably, RELEASES the borrow, then `apply_changes(&result)`. The way features run commands (D-W8B4M-2).
    pub fn update_view_model(&self, f: impl FnOnce(&mut MainViewModel) -> ChangeSet) {
        self.inner.update_view_model(f);
    }

    /// Drains the view model's completions (`vm.pump()`, UI thread; the borrow ends before the changes are applied) and applies their union. Called by the
    /// wake action of [`view_model_wake`] and by tests; a second call while one is running (re-entry through a nested message loop) returns at once.
    pub fn pump_view_model(&self) {
        self.inner.pump_view_model();
    }

    /// C# `Dispose()` (lines 421-445); see [`MainWindowInner::dispose_inner`]. Idempotent. Safe to call on the UI thread after `WM_CLOSE` or without it.
    pub fn dispose(&self) {
        self.inner.dispose_inner(true);
    }
}

// ---- the inner state ------------------------------------------------------------------------------------------------------------------------------

impl MainWindowInner {
    /// The features as they are now, in registration order; a copy, so that a hook may register features or call back into the window.
    pub(super) fn feature_snapshot(&self) -> Vec<Rc<dyn MainWindowFeature>> {
        self.features.borrow().clone()
    }

    /// The body of [`MainWindow::apply_changes`].
    pub(crate) fn apply_changes(&self, changes: &ChangeSet) {
        fan_out_changes(&InnerSteps(self), changes);
    }

    /// The body of [`MainWindow::apply_viewer_changes`].
    pub(crate) fn apply_viewer_changes(&self, changes: ViewerChanges) {
        fan_out_viewer(&InnerSteps(self), changes);
    }

    /// The body of [`MainWindow::update_view_model`]: `f` runs with the view model borrowed, the borrow ends, THEN the fan-out runs.
    pub(crate) fn update_view_model(&self, f: impl FnOnce(&mut MainViewModel) -> ChangeSet) {
        let changes = {
            let mut view_model = self.view_model.borrow_mut();
            f(&mut view_model)
        };
        self.apply_changes(&changes);
        // A wake that arrived inside a modal command (nested loop, borrow held) was deferred: drain it now that the borrow is gone.
        if self.pump_deferred.take() {
            self.pump_view_model();
        }
    }

    /// The body of [`MainWindow::pump_view_model`] and the wake action. A call while a pump runs (a nested message loop inside a handler) returns at once but
    /// makes the running pump go round once more, so a completion that arrived meanwhile is not left waiting for the next wake.
    pub(crate) fn pump_view_model(&self) {
        if self.disposed.get() {
            return;
        }
        if self.pumping.replace(true) {
            self.pump_again.set(true);
            return;
        }
        let _guard = ClearOnDrop(&self.pumping);
        loop {
            self.pump_again.set(false);
            let changes = {
                // A modal command (`show_settings`) holds the mutable borrow for its nested loop: defer to `update_view_model`, never panic.
                let Ok(mut view_model) = self.view_model.try_borrow_mut() else {
                    self.pump_deferred.set(true);
                    break;
                };
                view_model.pump()
            };
            self.apply_changes(&changes);
            if self.disposed.get() || !self.pump_again.get() {
                break;
            }
        }
    }

    /// C# `ContentFromViewer()` (lines 250-258): `ViewportContent { stretch, image_width, image_height, max_image_width, max_image_height, bitmap_width,
    /// bitmap_height, is_fit }` from `viewer` and `image` (width/height of the bitmap only when it is not disposed, else 0).
    pub(super) fn content_from_viewer(&self) -> ViewportContent {
        let viewer = self.viewer.borrow();
        let image = self.image.borrow();
        let (bitmap_width, bitmap_height) = bitmap_size(image.as_deref());
        ViewportContent {
            stretch: viewer.stretch(),
            image_width: viewer.image_width(),
            image_height: viewer.image_height(),
            max_image_width: viewer.max_image_width(),
            max_image_height: viewer.max_image_height(),
            bitmap_width,
            bitmap_height,
            is_fit: viewer.is_fit(),
        }
    }

    /// C# `UpdateFitSize()` (261-266): `(w, h) = viewport.viewport_size()`; `viewer.update_viewport(w, h, force = false)` (the returned update goes through
    /// `apply_viewer_changes`); `update_target_decode_box(None)`; publish the size for `size_source`.
    pub(super) fn update_fit_size(&self) {
        let (width, height) = self.viewport.viewport_size();
        let update = {
            let mut viewer = self.viewer.borrow_mut();
            viewer.update_viewport(width, height, false)
        };
        self.apply_viewer_changes(update.changes);
        self.update_target_decode_box(None);
        self.publish_viewport_size();
    }

    /// C# `UpdateTargetDecodeBox(viewportSize?)` (269-276): `viewer.set_dpi_scale(window.dpi_scale())`; `size_source.set_target_decode_box(
    /// calculate_target_decode_box(w > 1 ? w : 2200, h > 1 ? h : 1400, dpi, 1.15))` with `(w, h)` = the argument or `viewport.viewport_size()`. The scale and the
    /// box change in the same turn (never through a post).
    pub(super) fn update_target_decode_box(&self, viewport_size: Option<(f64, f64)>) {
        let (width, height) = viewport_size.unwrap_or_else(|| self.viewport.viewport_size());
        let dpi = self.window.dpi_scale();
        let changes = {
            let mut viewer = self.viewer.borrow_mut();
            viewer.set_dpi_scale(dpi)
        };
        self.apply_viewer_changes(changes);
        // C# throws only for an invalid argument (NaN, infinite or non-positive) and the fallbacks make a bad size unreachable; a bad dpi keeps the old box.
        match target_decode_box_for(width, height, dpi) {
            Ok(decode_box) => self.size_source.set_target_decode_box(decode_box),
            Err(error) => self
                .log
                .error("The target decode box could not be calculated", Some(&error)),
        }
    }

    /// C# `UpdateFitSizeIfAlive()` (306-312): `update_fit_size()` unless disposed.
    pub(super) fn update_fit_size_if_alive(&self) {
        if !self.disposed.get() {
            self.update_fit_size();
        }
    }

    /// C# `OnViewerChanged()` (278-287): unless disposed, `viewport.invalidate_layout()` and `window.invalidate()`.
    pub(super) fn on_viewer_changed(&self) {
        if self.disposed.get() {
            return;
        }
        self.invalidate_layout_and_window();
    }

    /// C# `OnClientSizeChanged` (316-325): unless disposed, `viewport.invalidate_layout()` and `window.invalidate()`.
    pub(super) fn on_client_size_changed(&self) {
        if self.disposed.get() {
            return;
        }
        self.invalidate_layout_and_window();
    }

    /// C# `OnDpiChanged` (327-337): unless disposed, `update_target_decode_box(None)`, `viewport.invalidate_layout()`, `window.invalidate()`. Runs AFTER the
    /// windowing handler (`on_windowing_dpi_changed`, 13k): it was subscribed first (D-W8B4M-3), so the decode box reads the final client size.
    pub(super) fn on_dpi_changed(&self) {
        if self.disposed.get() {
            return;
        }
        self.update_target_decode_box(None);
        self.invalidate_layout_and_window();
    }

    /// C# `OnClosing(sender, CancelEventArgs)` (341-362). `vm.defer_close_for_file_action()` true (a file action holds the gate, R7-7): `args.cancel = true`;
    /// when `close_when_file_action_done` is not yet set, set it and ask the gate (`vm.when_file_action_idle(waiter)`) whose `Send` waiter posts
    /// `close_when_file_action_done_waiter` to the window; return. Otherwise: `is_closing_or_closed = true`; `save_placement()`; `release_renderer()`
    /// WHILE THE HWND IS ALIVE (a flip-model swap chain with a queued Present released after the window is gone can hang the flush for ever).
    pub(super) fn on_closing(&self, args: &mut CancelEventArgs) {
        let defer = self.view_model.borrow().defer_close_for_file_action();
        if defer {
            args.cancel = true;
            if !self.close_when_file_action_done.get() {
                self.close_when_file_action_done.set(true);
                let dispatcher = self.dispatcher.clone();
                let id = self.id;
                // The gate runs the waiter inline when it became free meanwhile, and that happens under the borrow below: the waiter therefore only POSTS the
                // close (`window.close()` raises `Closed`, whose handler borrows the view model mutably).
                let waiter: UiAction = Box::new(move || {
                    post_to_window(
                        &dispatcher,
                        id,
                        UiPriority::Normal,
                        MainWindowInner::close_when_file_action_done_waiter,
                    );
                });
                let _ = self.view_model.borrow().when_file_action_idle(waiter);
            }
            return;
        }

        self.is_closing_or_closed.set(true);
        self.save_placement();
        self.release_renderer();
    }

    /// The waiter of C# `CloseWhenFileActionDoneAsync` (364-379): `close_when_file_action_done = false`; when not disposed and the window is not destroyed,
    /// `window.close()` (which raises `Closing` again, now with the gate free). Never throws; a panic is not caught (R0003).
    pub(super) fn close_when_file_action_done_waiter(&self) {
        self.close_when_file_action_done.set(false);
        if !self.disposed.get() && !self.window.is_destroyed() {
            self.window.close();
        }
    }

    /// C# `SavePlacement()` (381-388): `placement_file` set -> `placement.save(hwnd, file, windowing.restore_state(), windowing.restore_bounds())` (R7-10:
    /// closing during F11 saves the state and the Normal rect from before F11). The result is ignored; failures are logged by the port.
    pub(super) fn save_placement(&self) {
        let Some(file) = self.options.placement_file.as_deref() else {
            return;
        };
        let fullscreen = self.windowing.fullscreen.borrow().clone();
        let (state, bounds) = match fullscreen {
            Some(fullscreen) => (fullscreen.restore_state(), fullscreen.restore_bounds()),
            None => (None, None),
        };
        let _ = self.placement.save(self.window.hwnd(), file, state, bounds);
    }

    /// C# `OnClosed` (390-403): `is_closing_or_closed = true`; `cloak.stop()`; `vm.close_session()` (its `ChangeSet` is dropped: the window is gone) and, even if
    /// that panics, the preload controller is stopped (`try`/`finally` -> a drop guard).
    pub(super) fn on_closed(&self) {
        self.is_closing_or_closed.set(true);
        self.cloak.stop();
        let preload = self.view_model.borrow().preload_controller().cloned();
        let _stop_preload = StopPreload(preload);
        let _ = self.view_model.borrow_mut().close_session();
    }

    /// C# `ReleaseRenderer()` (409-415), idempotent: the keep-warm setup first (its thread owns a device of its own and stops BEFORE the window's device goes
    /// away), then the features' `release_renderer` (reverse order), `dispose_viewing()` (13j), `cache.take()` (drops the bitmaps through the device), then
    /// `surface.take()`. Reverse of creation. Called from `on_closing` (window alive) and again from `dispose_inner`.
    pub(super) fn release_renderer(&self) {
        if let Ok(mut keep_warm) = self.keep_warm.try_borrow_mut() {
            if let Some(setup) = keep_warm.as_mut() {
                setup.dispose();
            }
        }
        for feature in self.feature_snapshot().into_iter().rev() {
            feature.release_renderer();
        }
        self.dispose_viewing();
        // The values are dropped AFTER the borrows end (a drop may run device code).
        let cache = match self.cache.try_borrow_mut() {
            Ok(mut cache) => cache.take(),
            Err(_) => {
                self.log
                    .warn("release_renderer: the image cache is in use; dispose will retry");
                None
            }
        };
        drop(cache);
        let surface = match self.surface.try_borrow_mut() {
            Ok(mut surface) => surface.take(),
            Err(_) => {
                self.log
                    .warn("release_renderer: the surface is in use; dispose will retry");
                None
            }
        };
        drop(surface);
    }

    /// C# `Dispose()` body (421-445); `rethrow` = whether a captured panic may be resumed (false while panicking). Idempotent (`disposed`). Order:
    /// `disposed = true`; unsubscribe the hub, the window events (`window_subscriptions`) and the layout subscription; features' `dispose` (overlay, ...,
    /// reverse order); `size_source.set_viewport_size_fn(|| (0.0, 0.0))`; `cloak.stop()`; `image = None`; `dispose_windowing()` (13k); `release_renderer()`;
    /// `viewport.dispose()`; unregister from the thread-local registry; `window.dispose()` (destroys the native window if it still exists) only
    /// when `rethrow` (while panicking the `ShellWindow` drop does it without resuming a captured panic).
    pub(super) fn dispose_inner(&self, rethrow: bool) {
        if self.disposed.replace(true) {
            return;
        }
        if let Some(id) = self.hub_subscription.take() {
            self.hub.unsubscribe_image_changed(id);
        }
        let subscriptions: Vec<_> = self.window_subscriptions.borrow_mut().drain(..).collect();
        for id in subscriptions {
            self.window.unsubscribe(id);
        }
        if let Some(id) = self.layout_subscription.take() {
            self.viewport.unsubscribe_layout_updated(id);
        }
        for feature in self.feature_snapshot().into_iter().rev() {
            feature.dispose();
        }
        self.size_source.set_viewport_size_fn(Arc::new(|| (0.0, 0.0)));
        self.cloak.stop();
        let image = self.image.try_borrow_mut().ok().and_then(|mut image| image.take());
        drop(image);
        self.dispose_windowing();
        self.release_renderer();
        self.viewport.dispose();
        unregister_window(self.id);
        if rethrow {
            self.window.dispose();
        }
    }
}
