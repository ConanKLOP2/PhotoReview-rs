//! Port of `PhotoReview.Imaging/Decoding/ImageDecoderFactory.cs` (C# baseline d92d6aae, 99 lines). Contract:
//! `contracts/W4-12a.md`. Target path: `crates/pr-imaging/src/factory.rs` (lead adds `pub mod factory;` to `lib.rs`).
//!
//! INV-12: a non-Wpf backend is wrapped in the WPF fallback; the `Wpf` slot is always served; an unregistered backend
//! still decodes (warning + Wpf slot). Each backend's decoder is built ONCE (providers and decorator run once, even under
//! 16 racing threads).
//!
//! Stub: the bodies are `todo!()`, signatures and data types are final. Remove the `#![allow(...)]` line when implemented.
//! The interface trait (`ImageDecoderFactory`) is W4-01c; the concrete C# class `ImageDecoderFactory` is
//! [`DefaultImageDecoderFactory`] here (the trait took the unprefixed name).

#![allow(unused_imports, unused_variables, dead_code, clippy::unused_self)]

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

use pr_core::abstractions::Log;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::model::enums::DecoderBackend;

use crate::decoder_traits::{ImageDecoder, ImageDecoderFactory};
use crate::fallback::FallbackImageDecoder; // W4-12b

/// A decoder provider (C# `Func<IImageDecoder>`). The Wpf provider is called once per non-Wpf backend built, as in C#
/// (see the contract). Must be cheap and must not call back into the factory.
pub type DecoderProvider = Arc<dyn Fn() -> Arc<dyn ImageDecoder> + Send + Sync>;

/// C# `Func<DecoderBackend, IImageDecoder, IImageDecoder>? decoderDecorator`. Receives the REQUESTED backend (also for an
/// unregistered one) and the finished decoder; returns the decoder to hand out.
pub type DecoderDecorator = Arc<dyn Fn(DecoderBackend, Arc<dyn ImageDecoder>) -> Arc<dyn ImageDecoder> + Send + Sync>;

/// The build-once cell of one backend (shared so the map lock is not held while the provider runs).
type DecoderCell = Arc<OnceLock<Arc<dyn ImageDecoder>>>;

/// The optional constructor arguments (C# `log`, `metrics`, `decoderDecorator`, `wpfDecoderFactory`). `Default` = all `None`.
#[derive(Default, Clone)]
pub struct DecoderFactoryOptions {
    /// `None` = `NullLog` (C# `NullLog.Instance`).
    pub log: Option<Arc<dyn Log>>,
    /// Forwarded to each `FallbackImageDecoder` (counts fallbacks); `None` = not counted.
    pub metrics: Option<Arc<ReviewMetrics>>,
    /// Applied to every decoder `create` returns, after the fallback wrapping.
    pub decoder_decorator: Option<DecoderDecorator>,
    /// Fills the Wpf slot when no explicit `Wpf` provider is given (C# `wpfDecoderFactory`).
    pub wpf_decoder_factory: Option<DecoderProvider>,
}

impl std::fmt::Debug for DecoderFactoryOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecoderFactoryOptions").finish_non_exhaustive()
    }
}

/// Construction failure (C# `ArgumentException(paramName: "providers")`): a composition error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecoderFactoryError {
    /// No `Wpf` provider, no `wpf_decoder_factory`, and no `WicDirect` provider to map the Wpf slot to.
    NoWpfDecoder,
}

impl DecoderFactoryError {
    /// The C# exception message, byte-identical.
    pub const NO_WPF_DECODER_MESSAGE: &'static str =
        "No decoder can serve the Wpf backend: register a Wpf provider, pass wpfDecoderFactory, or register WicDirect.";
}

impl fmt::Display for DecoderFactoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoWpfDecoder => f.write_str(Self::NO_WPF_DECODER_MESSAGE),
        }
    }
}

impl std::error::Error for DecoderFactoryError {}

/// C# `ImageDecoderFactory` (the default `IImageDecoderFactory`).
pub struct DefaultImageDecoderFactory {
    /// Provider per backend; the `Wpf` slot is always present after construction.
    registry: HashMap<DecoderBackend, DecoderProvider>,
    /// One cell per requested backend; the cell, not the map lock, serialises the build (C# `Lazy`
    /// `ExecutionAndPublication`): a decoder is built and decorated exactly once, all callers share it.
    created: Mutex<HashMap<DecoderBackend, DecoderCell>>,
    log: Arc<dyn Log>,
    metrics: Option<Arc<ReviewMetrics>>,
    decoder_decorator: Option<DecoderDecorator>,
    /// True when the Wpf slot is served by the `WicDirect` provider (no WPF decoder): a `WicDirect` request then needs no
    /// second, identical fallback attempt.
    wpf_mapped_to_wic_direct: bool,
}

impl std::fmt::Debug for DefaultImageDecoderFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultImageDecoderFactory").finish_non_exhaustive()
    }
}

impl DefaultImageDecoderFactory {
    /// C# constructor. `providers` is applied in order, a later entry for the same backend replaces an earlier one. The Wpf
    /// slot, when no `Wpf` provider is present, is filled by `options.wpf_decoder_factory`, else by the `WicDirect`
    /// provider (`wpf_mapped_to_wic_direct = true`), else `Err(DecoderFactoryError::NoWpfDecoder)`. An explicit `Wpf`
    /// provider wins over `wpf_decoder_factory`.
    pub fn new(
        providers: Vec<(DecoderBackend, DecoderProvider)>,
        options: DecoderFactoryOptions,
    ) -> Result<Self, DecoderFactoryError> {
        todo!()
    }

    /// Builds (provider + fallback wrapping + decorator) the decoder for `backend`; called once per backend through the
    /// `OnceLock`. `Wpf`: `decorate(Wpf, registry[Wpf]())`. Unregistered: `log.warn("Decoder backend {backend} is not
    /// registered, falling back to Wpf.")` then `decorate(backend, registry[Wpf]())` (no fallback wrapper). Mapped Wpf slot
    /// and `backend == WicDirect`: `decorate(backend, provider())` unwrapped. Otherwise
    /// `decorate(backend, Arc::new(FallbackImageDecoder::new(provider(), backend, registry[Wpf](), log, metrics)))`.
    fn build_decoder(&self, backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        todo!()
    }

    /// `_decoderDecorator?.Invoke(backend, decoder) ?? decoder`.
    fn decorate(&self, backend: DecoderBackend, decoder: Arc<dyn ImageDecoder>) -> Arc<dyn ImageDecoder> {
        todo!()
    }
}

impl ImageDecoderFactory for DefaultImageDecoderFactory {
    /// `Create`: the shared instance for `backend`; the first call builds it, concurrent first calls build it once.
    fn create(&self, backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        todo!()
    }

    /// `IsRegistered`: `registry.contains_key(backend)`. `Wpf` is always true (the slot is always served).
    fn is_registered(&self, backend: DecoderBackend) -> bool {
        todo!()
    }
}
