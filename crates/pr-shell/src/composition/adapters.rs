//! Small adapters between the shell host services (W8-17c, `Rc`, UI thread) and the traits the view model and the graph take (W8-09b slice S1;
//! `contracts/W8-09b.md`, decision D-W8B6-153). No behaviour of their own: every method forwards.
//!
//! Why they exist: `ShellServices` holds `Rc<Win32DialogService>`, `Rc<Win32ClipboardService>` and `Rc<Win32PresentationSinkFactory>` (UI-thread objects), while
//! `MainViewModelDeps` takes `Arc<dyn DialogService>`, `Box<dyn ClipboardService>` and the composition contract `PresentationSinkFactory`. Rust's orphan rule forbids
//! `impl DialogService for Rc<Win32DialogService>` (neither the trait nor `Rc` is local), so each gets a one-field newtype in this crate. `Arc::new(SharedDialogService(..))`
//! holds an `Rc`, which `clippy::arc_with_non_send_sync` reports: the construction site carries `#[allow(clippy::arc_with_non_send_sync)]` with the justification "the view model
//! and every coordinator that holds the `Arc` live on the UI thread only (`MainViewModel` is `!Send`)".
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use pr_app::coordinators::sink_callbacks::{MainViewModelSinkCallbacks, PresentationSinkFactory};
use pr_app::coordinators::traits::IPresentationSink;
use pr_app::services::contracts::ClipboardService;
use pr_app::services::prewarm::PrewarmPreviews;
use pr_app::viewmodels::format::context::FormatContext;
use pr_app::viewmodels::main::FormatContextSource;
use pr_core::abstractions::{BatchReviewItem, DialogService, SettingsTarget, SkippedEntry};
use pr_core::model::enums::DecoderBackend;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache_key::{CacheKeyError, ImageCacheKey};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::{ImageDecoder, ImageDecoderFactory};
use pr_imaging::service::handle::PreviewHandle;
use pr_platform_win::dialogs::dialog_service::Win32DialogService;
use pr_platform_win::shell_viewing::presentation_sink::Win32PresentationSinkFactory;

use super::registry::SharedServices;

/// `IDialogService` of the shell as the `Arc<dyn DialogService>` the view model and its coordinators take. Every method forwards to the `Win32DialogService`.
pub struct SharedDialogService(pub Rc<Win32DialogService>);

impl std::fmt::Debug for SharedDialogService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedDialogService").finish_non_exhaustive()
    }
}

impl DialogService for SharedDialogService {
    fn show_confirmation(&self, title: &str, message: &str) -> bool {
        todo!()
    }

    fn show_message(&self, title: &str, message: &str) {
        todo!()
    }

    fn show_error(&self, title: &str, message: &str) {
        todo!()
    }

    fn pick_folder(&self, initial_folder: Option<&Path>) -> Option<PathBuf> {
        todo!()
    }

    fn show_batch_review(&self, items: &[BatchReviewItem]) -> bool {
        todo!()
    }

    fn show_recovery(&self) {
        todo!()
    }

    fn show_diagnostics(&self) {
        todo!()
    }

    fn show_settings(&self) -> bool {
        todo!()
    }

    /// MUST forward (the trait default would drop `target`, e.g. `ExternalEditor`).
    fn show_settings_at(&self, target: SettingsTarget) -> bool {
        todo!()
    }

    fn show_benchmark(&self, folder: Option<&Path>) {
        todo!()
    }

    fn show_skipped_files(&self, entries: &[SkippedEntry]) {
        todo!()
    }
}

/// `IClipboardService` of the shell as a `Box<dyn ClipboardService>` (`MainViewModelDeps::clipboard`). The shared `Rc` stays with `ShellServices` (the Recovery and Diagnostics
/// dialogs use the same object).
pub struct SharedClipboard(pub Rc<dyn ClipboardService>);

impl std::fmt::Debug for SharedClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedClipboard").finish_non_exhaustive()
    }
}

impl ClipboardService for SharedClipboard {
    fn try_set_text(&self, text: &str) -> bool {
        todo!()
    }
}

/// `Win32PresentationSinkFactory` as the composition contract `PresentationSinkFactory`. Until amendment A11 is finished in `pr-platform-win` (16i keeps the twin struct
/// `shell_viewing::presentation_sink::SinkCallbacks`, D-W8B4V-11) this is the "one-line adapter" its doc promises: `create(callbacks)` moves the five closures of
/// `MainViewModelSinkCallbacks` field by field into `SinkCallbacks { set_current_image, set_status_text, apply_initial_view_mode, on_presented, take_viewer_zoom_change }` and calls
/// `Win32PresentationSinkFactory::create`. When 16i implements the trait itself, this type is deleted and `ShellServices::sink_factory` is used directly.
pub struct Win32SinkFactoryAdapter(pub Rc<Win32PresentationSinkFactory>);

impl std::fmt::Debug for Win32SinkFactoryAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Win32SinkFactoryAdapter").finish_non_exhaustive()
    }
}

impl PresentationSinkFactory for Win32SinkFactoryAdapter {
    fn create(&self, callbacks: MainViewModelSinkCallbacks) -> Box<dyn IPresentationSink> {
        todo!()
    }
}

/// The preview service as the prewarm seam, resolved on first use (D-W8B6-154). The C# early decode resolved `PreviewImageService` lazily AFTER `store.Load()`; the Rust graph is built
/// BEFORE the settings worker runs, and the service captures `ImageCacheCapacityBytes`, the cache percent and the preload window from the settings at construction. Each method calls
/// `services.preview_service()` (built once, cached by the registry) and forwards to the `PrewarmPreviews` impl of `PreviewImageService`. A registry `Err` is a composition defect
/// (every input of the service is a constant of the registry): it is logged through `services.log()` and then `current_cache_key` answers
/// `Err(CacheKeyError::SourceNoLongerExists { full_name: full_path })` (the early decode then ends silently, R-PW-5); `start_viewer_preview` is only called with a key this wrapper gave
/// out, so its `Err` branch is unreachable and panics with the error text (caught by the `catch_unwind` of the worker, R0003).
pub struct LazyPrewarmPreviews {
    services: SharedServices,
}

impl std::fmt::Debug for LazyPrewarmPreviews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazyPrewarmPreviews").finish_non_exhaustive()
    }
}

impl LazyPrewarmPreviews {
    /// A lazy view over `services`; builds nothing.
    pub fn new(services: SharedServices) -> Self {
        Self { services }
    }
}

impl PrewarmPreviews for LazyPrewarmPreviews {
    fn current_cache_key(
        &self,
        full_path: &str,
        target_box: Option<DecodeBox>,
    ) -> Result<ImageCacheKey, CacheKeyError> {
        todo!()
    }

    fn start_viewer_preview(&self, full_path: &str, key: &ImageCacheKey) -> PreviewHandle {
        todo!()
    }
}

/// The decoder factory as the `Option<Arc<dyn ImageDecoderFactory>>` of `DialogServices::decoder_factory` (C# `GetService<IImageDecoderFactory>()` ran when the Settings window
/// opened, not at start-up). Resolving the real factory builds `SourceBytesCachePolicy`, which reads the settings: it must not happen before they are loaded (D-W8B6-154). `create` and
/// `is_registered` call `services.decoder_factory()` (cached) and forward. A registry `Err` cannot happen (`WicDirect` is always registered, so `DefaultImageDecoderFactory::new` has no
/// failing input): it is logged and then panics with the error text, like [`LazyPrewarmPreviews`].
pub struct LazyDecoderFactory {
    services: SharedServices,
}

impl std::fmt::Debug for LazyDecoderFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazyDecoderFactory").finish_non_exhaustive()
    }
}

impl LazyDecoderFactory {
    /// A lazy view over `services`; builds nothing.
    pub fn new(services: SharedServices) -> Self {
        Self { services }
    }
}

impl ImageDecoderFactory for LazyDecoderFactory {
    fn create(&self, backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        todo!()
    }

    fn is_registered(&self, backend: DecoderBackend) -> bool {
        todo!()
    }
}

/// Production `FormatContextSource` of `MainViewModelDeps::format` (08a O3): the display texts of the CURRENT language, evaluated on every call (a language switch applies at once,
/// D-W8-4). `with_context(run)` builds `FormatContext { texts: &Localizer::current(), numbers: &number_culture(), group_separator: &self.group_separator, dates: &WindowsDateText }` and
/// calls `run` with it; `to_local_time(utc)` = `pr_platform_win::culture::utc_to_local(utc)`. `group_separator` is `shell_startup::locale::group_separator()` read ONCE by `new`
/// (the user locale cannot change under a running process in a way the C# `CurrentCulture` would follow either).
pub struct LocalizedFormat {
    group_separator: String,
}

impl std::fmt::Debug for LocalizedFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalizedFormat").finish_non_exhaustive()
    }
}

impl LocalizedFormat {
    /// Reads the group separator of the user locale.
    pub fn new() -> Self {
        todo!()
    }

    /// For tests: a fixed separator (no Win32 call).
    pub fn with_group_separator(group_separator: String) -> Self {
        Self { group_separator }
    }
}

impl Default for LocalizedFormat {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatContextSource for LocalizedFormat {
    fn with_context(&self, run: &mut dyn FnMut(&FormatContext<'_>)) {
        todo!()
    }

    fn to_local_time(&self, utc: DateTimeUtc) -> DateTimeUtc {
        todo!()
    }
}
