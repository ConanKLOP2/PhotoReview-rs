//! Port of `PhotoReview.App.Shared/Composition/SharedServiceContracts.cs` (C# baseline `d92d6aae`, 23 lines).
//! Contract: `contracts/W8-09a.md`.
//!
//! C-16: composition shared by the old WPF app and the Win32 shell. The C# `IServiceProvider` is gone (no DI container), so
//! `Func<IServiceProvider, IImageDecoder>` becomes a plain [`DecoderProvider`] closure. `IPresentationSinkFactory` and
//! `MainViewModelSinkCallbacks` keep their C# shape: the host builds the sink from five callbacks into the view model.
//!
//! Threading: the callbacks and the sink run on the UI thread only (no `Send`/`Sync` bounds), like every sink of
//! `coordinators::traits`.
use std::fmt;
use std::sync::Arc;

use pr_imaging::decode_types::PlatformImage;
use pr_imaging::decoder_traits::PlatformImageCodec;
use pr_imaging::factory::DecoderProvider;

use pr_app::coordinators::presenter::state::ImageChange;
use pr_app::coordinators::traits::IPresentationSink;

/// C# `SharedServiceOptions(Codec, WpfFallbackDecoder, ThumbnailDecoder)`: what a host (old WPF app or Win32 shell) supplies.
#[derive(Clone)]
pub struct SharedServiceOptions {
    /// Platform codec (C-02). Win32 shell: `pr_imaging::pixel_memory::PixelBufferImageCodec`.
    pub codec: Arc<dyn PlatformImageCodec>,
    /// WPF fallback decoder (INV-12) of the WPF host. `None` on the Win32 host (NE-8): the saved "Wpf" setting is then
    /// served by `WicDirect` (`DefaultImageDecoderFactory` does the mapping). The Rust app is always the Win32 host, so
    /// production passes `None`.
    pub wpf_fallback_decoder: Option<DecoderProvider>,
    /// Decoder for the thumbnail cache. Reserved: C# only checks it for null (NOWPF-WP09).
    pub thumbnail_decoder: DecoderProvider,
}

impl fmt::Debug for SharedServiceOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedServiceOptions")
            .field("codec", &self.codec.name())
            .field("has_wpf_fallback_decoder", &self.wpf_fallback_decoder.is_some())
            .finish_non_exhaustive()
    }
}

/// C# `Action<object?, bool>` extended with the facts the callback reads from the presenter (contract W8-07h).
pub type SetCurrentImageCallback = Box<dyn FnMut(Option<PlatformImage>, bool, &ImageChange<'_>)>;

/// C# `MainViewModelSinkCallbacks(SetCurrentImage, SetStatusText, ApplyInitialViewMode, OnPresented)`: the view-model side of the
/// presentation sink. The host sink calls them on the UI thread, in the order its contract states (view model first).
pub struct MainViewModelSinkCallbacks {
    /// C# `Action<object?, bool>`: the image (`None` in Compare mode or an empty folder) and `is_file_change`.
    pub set_current_image: SetCurrentImageCallback,
    /// C# `Action<string>`.
    pub set_status_text: Box<dyn FnMut(&str)>,
    /// C# `Action`.
    pub apply_initial_view_mode: Box<dyn FnMut()>,
    /// C# `Action<string>`: the presented path.
    pub on_presented: Box<dyn FnMut(&str)>,
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
