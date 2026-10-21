//! Port of the sink part of `PhotoReview.App.Shared/Composition/SharedServiceContracts.cs` (C# baseline `d92d6aae`):
//! `MainViewModelSinkCallbacks` and `IPresentationSinkFactory`. Moved here from `pr_shell::composition::contracts` (reconciliation
//! decision D-W8B4X-13, amendment A11) so that `pr-platform-win` (W8-16i) can implement the factory: `pr-shell` depends on
//! `pr-platform-win`, not the other way round. `pr_shell::composition::contracts` re-exports these names.
//!
//! Threading: the callbacks and the sink run on the UI thread only (no `Send`/`Sync` bounds), like every sink of
//! `coordinators::traits`.

use std::fmt;

use pr_imaging::decode_types::PlatformImage;

use crate::coordinators::presenter::state::ImageChange;
use crate::coordinators::traits::IPresentationSink;

/// C# `Action<object?, bool>` extended with the facts the callback reads from the presenter (contract W8-07h).
pub type SetCurrentImageCallback = Box<dyn FnMut(Option<PlatformImage>, bool, &ImageChange<'_>)>;

/// C# `MainViewModelSinkCallbacks(SetCurrentImage, SetStatusText, ApplyInitialViewMode, OnPresented)` PLUS the pull of the viewer zoom
/// (`take_viewer_zoom_change`, W8-08a O1; `PresentationBridge::take_viewer_zoom_change`): the view-model side of the presentation
/// sink. The host sink calls them on the UI thread, in the order its contract states (view model first).
pub struct MainViewModelSinkCallbacks {
    /// C# `Action<object?, bool>`: the image (`None` in Compare mode or an empty folder) and `is_file_change`.
    pub set_current_image: SetCurrentImageCallback,
    /// C# `Action<string>`.
    pub set_status_text: Box<dyn FnMut(&str)>,
    /// C# `Action`.
    pub apply_initial_view_mode: Box<dyn FnMut()>,
    /// C# `Action<string>`: the presented path.
    pub on_presented: Box<dyn FnMut(&str)>,
    /// The zoom (`None` = Fit) the view model asked for during the last call, once; `None` when it asked for nothing.
    pub take_viewer_zoom_change: Box<dyn FnMut() -> Option<Option<f64>>>,
}

impl fmt::Debug for MainViewModelSinkCallbacks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainViewModelSinkCallbacks").finish_non_exhaustive()
    }
}

/// C# `IPresentationSinkFactory` (WPF: `WpfPresentationSink`; Win32: `Win32PresentationSink`, W8-16i).
pub trait PresentationSinkFactory {
    /// C# `Create(callbacks)`: takes ownership of the callbacks.
    fn create(&self, callbacks: MainViewModelSinkCallbacks) -> Box<dyn IPresentationSink>;
}
