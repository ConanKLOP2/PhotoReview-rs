//! New (no 1:1 C# twin; the C# `ViewModelOverlayCommands` forwarded to the view model directly): the PRODUCTION `OverlayViewModel` of W8-15j over the main window. Unit W8-17e. Contract:
//! `contracts/W8-17e.md`.
//!
//! The batch-B4 reconciliation (D-W8B4X-23, "15j open question: the 17e contract must name the owner of these seven calls") leaves the owner of `show_settings`, `show_skipped_files`,
//! `show_recovery`, `show_diagnostics`, `show_benchmark`, `clear_cache` and `remove_duplicates` open. DECISION D-W8B6-27: THIS file. The merged 15j has the trait and the test fixture only; the
//! view model already owns the dialogs (`MainViewModel` holds the `Arc<dyn DialogService>` of the shell and runs `show_*` itself), so each command is one `MainWindow::update_view_model` call, exactly the
//! way the context menu (14f) and the input (14c) run theirs.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pr_app::overlay::model::OverlayModel;
use pr_core::abstractions::Log;
use pr_core::settings::app_settings::AppSettings;
use pr_platform_win::host::main_window::MainWindow;
use pr_platform_win::host::main_window::core::SharedViewModel;
use pr_platform_win::shell_overlay::main_window::OverlayViewModel;

/// The slot that holds the main window (`StartupState::main`): the view model holds a `Weak` of it so the window can be closed and dropped while a command is running.
pub type MainWindowSlot = Rc<RefCell<Option<Rc<MainWindow>>>>;

/// `OverlayViewModel` over the shared view model and the window slot. All methods run on the UI thread, take `&self`, and call the window through a CLONED `Rc<MainWindow>` (never with the slot
/// borrowed): a modal dialog (Settings, Recovery, Benchmark) runs a nested message loop inside the call, during which the window may be closed.
pub struct MainWindowOverlayViewModel {
    view_model: SharedViewModel,
    window: Weak<RefCell<Option<Rc<MainWindow>>>>,
    log: Arc<dyn Log>,
    /// The last model read while the view model was free. A modal command (Settings, the folder pickers) runs inside `update_view_model` with the view model
    /// borrowed mutably, and its nested message loop runs the overlay's posted refresh: that refresh reads the model and must not panic on `RefCell`.
    last_model: RefCell<Option<OverlayModel>>,
    /// Same for `settings()` (the overlay reads it on hover and paint).
    last_settings: RefCell<Option<Arc<AppSettings>>>,
    /// Writes that arrived while a modal command held the view model (Apply in Settings changes the language and the settings of the main window behind it).
    deferred: RefCell<Deferred>,
}

/// What the overlay asked for while the view model was borrowed by a modal command; applied by [`MainWindowOverlayViewModel::apply_deferred`].
#[derive(Default)]
struct Deferred {
    settings: Option<Arc<AppSettings>>,
    refresh_localized_text: bool,
}

impl std::fmt::Debug for MainWindowOverlayViewModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MainWindowOverlayViewModel").finish_non_exhaustive()
    }
}

impl MainWindowOverlayViewModel {
    fn window(&self) -> Option<Rc<MainWindow>> {
        self.window.upgrade().and_then(|slot| slot.borrow().clone())
    }

    /// The view model of `window` (a `Weak` of the slot) over the shared view model.
    pub fn new(view_model: SharedViewModel, window: &MainWindowSlot, log: Arc<dyn Log>) -> Self {
        Self {
            view_model,
            window: Rc::downgrade(window),
            log,
            last_model: RefCell::new(None),
            last_settings: RefCell::new(None),
            deferred: RefCell::new(Deferred::default()),
        }
    }

    /// True while a modal command (`show_settings`, a folder picker) holds the view model mutably inside its nested message loop.
    fn view_model_busy(&self) -> bool {
        self.view_model.try_borrow_mut().is_err()
    }

    /// Applies the settings and the language refresh that were asked for while the view model was busy (the last settings win). Called after every modal
    /// command and before the next write; a no-op when nothing is pending or the view model is still busy. Found by the exe walkthrough 2 2026-10-29: Apply in
    /// Settings panicked here with "already borrowed" and the main window kept its old language until the next change.
    pub fn apply_deferred(&self) {
        if self.view_model_busy() {
            return;
        }
        let pending = std::mem::take(&mut *self.deferred.borrow_mut());
        if let Some(settings) = pending.settings {
            self.set_settings(settings);
        }
        if pending.refresh_localized_text {
            self.refresh_localized_text();
        }
    }
}

impl OverlayViewModel for MainWindowOverlayViewModel {
    /// `view_model.borrow().overlay_model()` (a short shared borrow). While a modal command holds the view model mutably (nested message loop inside
    /// `update_view_model`), the last model read is returned instead: nothing can change in the view model meanwhile, and the changes of the command are
    /// applied (and the overlay refreshed) when it returns. Found by the exe walkthrough 2026-10-29: the gear click panicked here and never opened Settings.
    fn overlay_model(&self) -> OverlayModel {
        if let Ok(view_model) = self.view_model.try_borrow() {
            let model = view_model.overlay_model();
            *self.last_model.borrow_mut() = Some(model.clone());
            return model;
        }
        self.last_model.borrow().clone().unwrap_or_default()
    }

    /// `view_model.borrow().settings()`; the last settings read while the view model is borrowed mutably (see [`Self::overlay_model`]).
    fn settings(&self) -> Arc<AppSettings> {
        if let Ok(view_model) = self.view_model.try_borrow() {
            let settings = view_model.settings();
            *self.last_settings.borrow_mut() = Some(Arc::clone(&settings));
            return settings;
        }
        self.last_settings.borrow().clone().unwrap_or_default()
    }

    /// `window.update_view_model(|vm| vm.set_settings(settings))` (C# `_viewModel.Settings = settings`, the change set is applied by the window); no window: `view_model.borrow_mut().set_settings(..)` and
    /// the change set dropped (shutdown).
    fn set_settings(&self, settings: Arc<AppSettings>) {
        if self.view_model_busy() {
            self.deferred.borrow_mut().settings = Some(settings);
            return;
        }
        if let Some(window) = self.window() {
            window.update_view_model(|vm| vm.set_settings(settings));
        } else {
            // Not busy (checked above) and on the UI thread: this borrow cannot fail.
            if let Ok(mut view_model) = self.view_model.try_borrow_mut() {
                view_model.set_settings(settings);
            }
        }
    }

    /// `window.update_view_model(|vm| vm.refresh_localized_text())`.
    fn refresh_localized_text(&self) {
        if self.view_model_busy() {
            self.deferred.borrow_mut().refresh_localized_text = true;
            return;
        }
        if let Some(window) = self.window() {
            window.update_view_model(|vm| vm.refresh_localized_text());
        }
    }

    /// C# `OnOverlayCommandFailed` (without the thread post: the overlay calls it on the UI thread): `log.error(&format!("{action_name} failed"), None)` and
    /// `window.update_view_model(|vm| vm.set_status_text(&pr_l10n::tr::status_action_failed(action_name, message.to_owned())))`.
    fn report_action_failed(&self, action_name: &str, message: &str) {
        if let Some(window) = self.window() {
            self.log.error(&format!("{action_name} failed"), None);
            window.update_view_model(|vm| {
                vm.set_status_text(&pr_l10n::tr::status_action_failed(action_name, message.to_owned()))
            });
        }
    }

    /// `window.update_view_model(|vm| vm.pick_and_open_folder())`.
    fn pick_and_open_folder(&self) {
        if let Some(window) = self.window() {
            window.update_view_model(|vm| vm.pick_and_open_folder());
            self.apply_deferred();
        }
    }

    /// `window.update_view_model(|vm| vm.show_settings())`: modal; the view model applies the new settings itself.
    fn show_settings(&self) {
        if let Some(window) = self.window() {
            window.update_view_model(|vm| vm.show_settings());
            self.apply_deferred();
        }
    }

    /// `view_model.borrow().show_skipped_files()` (`&self` on the view model: a SHARED borrow, so the nested loop of the dialog can still paint).
    fn show_skipped_files(&self) {
        if let Some(_window) = self.window() {
            self.view_model.borrow().show_skipped_files();
        }
    }

    /// `view_model.borrow().show_recovery()` (shared borrow, as above).
    fn show_recovery(&self) {
        if let Some(_window) = self.window() {
            self.view_model.borrow().show_recovery();
        }
    }

    /// `view_model.borrow().show_diagnostics()` (shared borrow).
    fn show_diagnostics(&self) {
        if let Some(_window) = self.window() {
            self.view_model.borrow().show_diagnostics();
        }
    }

    /// `view_model.borrow().show_benchmark()` (shared borrow).
    fn show_benchmark(&self) {
        if let Some(_window) = self.window() {
            self.view_model.borrow().show_benchmark();
        }
    }

    /// `window.update_view_model(|vm| vm.clear_cache())`.
    fn clear_cache(&self) {
        if let Some(window) = self.window() {
            window.update_view_model(|vm| vm.clear_cache());
            self.apply_deferred();
        }
    }

    /// `window.update_view_model(|vm| vm.remove_duplicates(numbered))`.
    fn remove_duplicates(&self, numbered: bool) {
        if let Some(window) = self.window() {
            window.update_view_model(|vm| vm.remove_duplicates(numbered));
            self.apply_deferred();
        }
    }
}
