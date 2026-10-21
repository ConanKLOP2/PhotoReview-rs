//! Port of `PhotoReview.App.Shared/Composition/SharedServiceContracts.cs` (C# baseline `d92d6aae`, 23 lines).
//! Contract: `contracts/W8-09a.md`.
//!
//! C-16: composition shared by the old WPF app and the Win32 shell. The C# `IServiceProvider` is gone (no DI container), so
//! `Func<IServiceProvider, IImageDecoder>` becomes a plain [`DecoderProvider`] closure. `IPresentationSinkFactory` and
//! `MainViewModelSinkCallbacks` keep their C# shape (the host builds the sink from five callbacks into the view model) and now live in
//! `pr_app::coordinators::sink_callbacks`; they are re-exported below.
//!
//! Threading: the callbacks and the sink run on the UI thread only (no `Send`/`Sync` bounds), like every sink of
//! `coordinators::traits`.
use std::fmt;
use std::sync::Arc;

use pr_imaging::decoder_traits::PlatformImageCodec;
use pr_imaging::factory::DecoderProvider;

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

// Moved to `pr_app::coordinators::sink_callbacks` (reconciliation D-W8B4X-13, amendment A11) so `pr-platform-win` can implement the
// factory; re-exported so the 09a/09b names stay valid.
pub use pr_app::coordinators::sink_callbacks::{
    MainViewModelSinkCallbacks, PresentationSinkFactory, SetCurrentImageCallback,
};
