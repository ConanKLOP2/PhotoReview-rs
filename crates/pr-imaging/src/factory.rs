//! Port of `PhotoReview.Imaging/Decoding/ImageDecoderFactory.cs` (C# baseline d92d6aae, 99 lines). Contract:
//! `contracts/W4-12a.md`. Target path: `crates/pr-imaging/src/factory.rs` (lead adds `pub mod factory;` to `lib.rs`).
//!
//! INV-12: a non-Wpf backend is wrapped in the WPF fallback; the `Wpf` slot is always served; an unregistered backend
//! still decodes (warning + Wpf slot). Each backend's decoder is built ONCE (providers and decorator run once, even under
//! 16 racing threads).
//!
//! The interface trait (`ImageDecoderFactory`) is W4-01c; the concrete C# class `ImageDecoderFactory` is
//! [`DefaultImageDecoderFactory`] here (the trait took the unprefixed name).

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use pr_core::abstractions::{Log, NullLog};
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::model::enums::DecoderBackend;
use pr_core::model::serde_ext::LenientEnum;

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
        let mut registry: HashMap<DecoderBackend, DecoderProvider> = HashMap::new();
        for (backend, provider) in providers {
            registry.insert(backend, provider);
        }

        let mut wpf_mapped_to_wic_direct = false;
        if !registry.contains_key(&DecoderBackend::Wpf) {
            if let Some(wpf) = options.wpf_decoder_factory.clone() {
                registry.insert(DecoderBackend::Wpf, wpf);
            } else if let Some(wic) = registry.get(&DecoderBackend::WicDirect).cloned() {
                registry.insert(DecoderBackend::Wpf, wic);
                wpf_mapped_to_wic_direct = true;
            } else {
                return Err(DecoderFactoryError::NoWpfDecoder);
            }
        }

        let log: Arc<dyn Log> = options.log.unwrap_or_else(|| Arc::new(NullLog));
        Ok(Self {
            registry,
            created: Mutex::new(HashMap::new()),
            log,
            metrics: options.metrics,
            decoder_decorator: options.decoder_decorator,
            wpf_mapped_to_wic_direct,
        })
    }

    /// The Wpf provider; the slot is always filled by the constructor.
    fn wpf_decoder(&self) -> Option<Arc<dyn ImageDecoder>> {
        self.registry.get(&DecoderBackend::Wpf).map(|provider| provider())
    }

    /// Builds (provider + fallback wrapping + decorator) the decoder for `backend`; called once per backend through the
    /// `OnceLock`. `Wpf`: `decorate(Wpf, registry[Wpf]())`. Unregistered: `log.warn("Decoder backend {backend} is not
    /// registered, falling back to Wpf.")` then `decorate(backend, registry[Wpf]())` (no fallback wrapper). Mapped Wpf slot
    /// and `backend == WicDirect`: `decorate(backend, provider())` unwrapped. Otherwise
    /// `decorate(backend, Arc::new(FallbackImageDecoder::new(provider(), backend, registry[Wpf](), log, metrics)))`.
    fn build_decoder(&self, backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        // The Wpf slot is guaranteed by the constructor, so the lookups below cannot miss.
        let decoder = if backend == DecoderBackend::Wpf {
            self.wpf_decoder()
        } else if let Some(provider) = self.registry.get(&backend) {
            if backend == DecoderBackend::WicDirect && self.wpf_mapped_to_wic_direct {
                Some(provider())
            } else {
                let primary = provider();
                self.wpf_decoder().map(|fallback| {
                    Arc::new(FallbackImageDecoder::new(
                        primary,
                        backend,
                        fallback,
                        Arc::clone(&self.log),
                        self.metrics.clone(),
                    )) as Arc<dyn ImageDecoder>
                })
            }
        } else {
            self.log.warn(&format!(
                "Decoder backend {} is not registered, falling back to Wpf.",
                backend.name()
            ));
            self.wpf_decoder()
        };
        match decoder {
            Some(decoder) => self.decorate(backend, decoder),
            None => unreachable!("the constructor always fills the Wpf slot"),
        }
    }

    /// `_decoderDecorator?.Invoke(backend, decoder) ?? decoder`.
    fn decorate(&self, backend: DecoderBackend, decoder: Arc<dyn ImageDecoder>) -> Arc<dyn ImageDecoder> {
        match &self.decoder_decorator {
            Some(decorator) => decorator(backend, decoder),
            None => decoder,
        }
    }
}

impl ImageDecoderFactory for DefaultImageDecoderFactory {
    /// `Create`: the shared instance for `backend`; the first call builds it, concurrent first calls build it once.
    fn create(&self, backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        // Hold the map lock only to fetch-or-insert the cell; the build runs outside it.
        let cell: DecoderCell = {
            let mut created = self.created.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(created.entry(backend).or_default())
        };
        Arc::clone(cell.get_or_init(|| self.build_decoder(backend)))
    }

    /// `IsRegistered`: `registry.contains_key(backend)`. `Wpf` is always true (the slot is always served).
    fn is_registered(&self, backend: DecoderBackend) -> bool {
        self.registry.contains_key(&backend)
    }
}
