//! Port of `Services/PreviewStateContext.cs` (16 lines). Contract: `contracts/W8-04c.md`, rules R-PS-*.
//!
//! C# bridges UI state to the singleton `PreviewImageService` (three `Func` properties, default-initialised). Rust keeps the three
//! closures as shared `Arc<dyn Fn>` fields so the composition root can clone the context into the service's own closures.

use std::fmt;
use std::sync::Arc;

use pr_core::model::enums::DecoderBackend;
use pr_imaging::decode_types::DecodeBox;

/// C# `PreviewStateContext` (16 lines). Public closure fields mirror the C# settable properties (object-initializer syntax).
#[derive(Clone)]
pub struct PreviewStateContext {
    /// C# `IsOriginalLoadingMode`; default `() => false`.
    pub is_original_loading_mode: Arc<dyn Fn() -> bool + Send + Sync>,
    /// C# `TargetDecodeBox` (device pixels); default `DecodeBox.Unbounded` (= full size). Read by the service on every key build.
    pub target_decode_box: Arc<dyn Fn() -> DecodeBox + Send + Sync>,
    /// C# `CurrentBackend`; default `DecoderBackend.Wpf` (R-PS-2: the C# default, not a Rust backend choice).
    pub current_backend: Arc<dyn Fn() -> DecoderBackend + Send + Sync>,
}

impl PreviewStateContext {
    /// The three C# initialisers: not Original, `DecodeBox::UNBOUNDED`, `DecoderBackend::Wpf` (R-PS-1).
    pub fn new() -> Self {
        Self {
            is_original_loading_mode: Arc::new(|| false),
            target_decode_box: Arc::new(|| DecodeBox::UNBOUNDED),
            current_backend: Arc::new(|| DecoderBackend::Wpf),
        }
    }
}

impl Default for PreviewStateContext {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for PreviewStateContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreviewStateContext")
            .field("is_original_loading_mode", &(self.is_original_loading_mode)())
            .field("target_decode_box", &(self.target_decode_box)())
            .field("current_backend", &(self.current_backend)())
            .finish()
    }
}
