//! The native seams of the shared service graph, as ONE struct (W8-09b slice S2/S3; `contracts/W8-09b.md`, decision D-W8B6-152).
//!
//! C# registered each OS service as its own DI singleton (`WindowsRecycleBin.Instance`, `WindowsLiveOperationRegistry`, `WindowsMemoryProbe`, `WindowsNaturalComparer.Instance`,
//! `WindowsDisplayClock.Instance`, `ExplorerOrderService`, and the static helpers `RamBudgetPolicy`, `MemoryHeadroom`, `WicImageEncoder`, `WicCacheImageReader`, `EmbeddedThumbnailReader`,
//! `WicPixelScaler`, `WindowsShellIntegration`, `CultureInfo`) and a test replaced any of them with `services.AddSingleton<..>(fake)`. Rust has no container, so the OS seams are the
//! fields of [`PlatformServices`]: [`PlatformServices::production`] fills them with the real adapters, a test builds the struct field by field from fakes (no `Default`: a missing seam is a
//! compile error, the Rust form of C# "fails loudly when missing"). `registry::SharedServices::build` takes one by value and never builds an OS object itself.
//!
//! Everything here is `Send + Sync` (the preview service, the decoders and the journal run these on workers).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use pr_app::localization::service::CultureLookup;
use pr_core::abstractions::{
    DisplayClock, ExplorerOrderProvider, LiveOperationRegistry, Log, MemoryProbe, NaturalComparer, RecycleBin,
    ShellIntegration,
};
use pr_imaging::cache::preview_file::PreviewPayloadCodec;
use pr_imaging::cache::thumbnails::CacheImageReader;
use pr_imaging::decoder_traits::PlatformImageCodec;
use pr_imaging::embedded_thumbnail::{EmbeddedThumbnailBackend, EmbeddedThumbnailFrame, ThumbnailBackendError};
use pr_imaging::memory_headroom::MemoryInfoSource;
use pr_imaging::pixels::PixelBuffer;
use pr_imaging::ram_budget::PhysicalMemorySource;
use pr_l10n::loader::UiCulture;
use pr_libraw::decoder::LibRawApi;
use pr_turbojpeg::decoder::{FineScaleError, FineScaler};

/// The OS seams. Field names are the Rust names of the C# registrations (golden `shell-composition-registrations`, indices 28 to 33, plus the static helpers listed in the module doc).
#[derive(Clone)]
pub struct PlatformServices {
    /// C# `IRecycleBin` = `WindowsRecycleBin.Instance`. Production: `WindowsRecycleBin::new(Arc::new(WindowsRecycleBinRestoreBackend::new(log)))`. Tests (and the `GraphOverrides::recycle_bin`
    /// of the 20b smoke tests): a fake or `TempFolderRecycleBin`; NEVER the real bin (AGENTS.md).
    pub recycle_bin: Arc<dyn RecycleBin>,
    /// C# `ILiveOperationRegistry` = `WindowsLiveOperationRegistry(log)`. Production: `WindowsLiveOperationRegistry::new(log, pr_core::identity::INSTANCE_PREFIX)` (`PhotoReviewRs`, R0002:
    /// the C# default prefix `PhotoReview` would make this app's markers collide with the C# app's).
    pub live_operations: Arc<dyn LiveOperationRegistry>,
    /// C# `IMemoryProbe` = `WindowsMemoryProbe(log)`.
    pub memory_probe: Arc<dyn MemoryProbe>,
    /// C# `INaturalComparer` = `WindowsNaturalComparer.Instance`.
    pub natural_comparer: Arc<dyn NaturalComparer>,
    /// C# `IDisplayClock` = `WindowsDisplayClock.Instance` (production: `Arc::new(WindowsDisplayClock::new())`).
    pub display_clock: Arc<dyn DisplayClock>,
    /// C# `IExplorerOrderProvider` = `ExplorerOrderService`. Production: `pr_platform_win::explorer_order_provider::production_explorer_order(log)` (starts the STA pump thread).
    pub explorer_order: Arc<dyn ExplorerOrderProvider>,
    /// C# `RamBudgetPolicy.GetPhysicalMemoryBytes()`. Production: `pr_platform_win::physical_memory::WindowsPhysicalMemory`.
    pub physical_memory: Arc<dyn PhysicalMemorySource>,
    /// C# `MemoryHeadroom.ReadGcMemoryInfo` (the `MemoryInfo` property of every decoder). Production: the same `WindowsPhysicalMemory`.
    pub memory_info: Arc<dyn MemoryInfoSource>,
    /// C# `PhotoReview.Platform.Windows.WindowsShellIntegration` (`SettingsPresenter.ShellIntegration` default). Production: `pr_platform_win::shell_integration::WindowsShellIntegration::new()`.
    pub shell_integration: Arc<dyn ShellIntegration>,
    /// C# static `WicImageEncoder.EncodeJpeg` / `WicCacheImageReader.Decode` for the `.pv4` payloads. Production: `pr_platform_win::wic::cache_adapters::WicPreviewPayloadCodec`.
    pub payload_codec: Arc<dyn PreviewPayloadCodec>,
    /// C# `WicCacheImageReader` for the PNG thumbnail entries. Production: `pr_platform_win::wic::cache_adapters::WicCacheImageReader`.
    pub cache_image_reader: Arc<dyn CacheImageReader>,
    /// C# `EmbeddedThumbnailReader`'s WIC container (`WicDirectDecoder.CreateFactory`...). Production TODAY: [`UnavailableEmbeddedThumbnails`] (open question O1 of the contract: no merged
    /// unit implements `EmbeddedThumbnailBackend` over WIC; a `Different` row until one does).
    pub embedded_thumbnails: Arc<dyn EmbeddedThumbnailBackend>,
    /// C# `WicPixelScaler.Resize` for the TurboJPEG fine-scale step. Production: [`WicFineScaler`].
    pub fine_scaler: Arc<dyn FineScaler>,
    /// LibRaw library. Production: `Arc::new(pr_libraw::decoder::NativeLibRaw)`.
    pub libraw_api: Arc<dyn LibRawApi>,
    /// C# `CultureInfo.GetCultureInfo`. Production: `pr_platform_win::culture::WindowsCultureLookup`.
    pub cultures: Arc<dyn CultureLookup>,
    /// C# `CultureInfo.InstalledUICulture`. Production: `pr_platform_win::culture::installed_ui_culture()`.
    pub installed_ui_culture: UiCulture,
    /// C# `PixelBufferImageCodec.Instance` (`SharedServiceOptions.Codec` of the Win32 shell). Production: `PixelBufferImageCodec::shared()`.
    pub codec: Arc<dyn PlatformImageCodec>,
}

impl fmt::Debug for PlatformServices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlatformServices")
            .field("codec", &self.codec.name())
            .finish_non_exhaustive()
    }
}

/// A production adapter could not be created (today only the Explorer pump thread can fail).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformError {
    /// Which seam failed (the field name of [`PlatformServices`]).
    pub service: &'static str,
    /// The text of the underlying error.
    pub message: String,
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} could not be created: {}", self.service, self.message)
    }
}

impl Error for PlatformError {}

/// The two production adapters a test must be able to replace BEFORE they are created (`GraphOverrides::recycle_bin`, `GraphOverrides::explorer_order`): creating the real ones and
/// discarding them would start the Explorer pump thread and build the real Recycle Bin facade for nothing. `None` = the production adapter.
#[derive(Default)]
pub struct PlatformOverrides {
    /// Replaces `recycle_bin`; the production `WindowsRecycleBin` is NOT created when this is `Some`.
    pub recycle_bin: Option<Arc<dyn RecycleBin>>,
    /// Replaces `explorer_order`; the production Explorer service (and its STA pump thread) is NOT created when this is `Some`.
    pub explorer_order: Option<Arc<dyn ExplorerOrderProvider>>,
}

impl fmt::Debug for PlatformOverrides {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlatformOverrides")
            .field("recycle_bin", &self.recycle_bin.is_some())
            .field("explorer_order", &self.explorer_order.is_some())
            .finish()
    }
}

impl PlatformServices {
    /// Every field from its "Production" line above, in field order, except the ones `overrides` replaces. `log` is the log the OS adapters report to (the process log). Creates no
    /// window, reads no file, touches no user data; the one side effect is the Explorer STA pump thread (only when not overridden). `Err` names the field that failed.
    pub fn production(log: Arc<dyn Log>, overrides: PlatformOverrides) -> Result<PlatformServices, PlatformError> {
        todo!()
    }
}

/// Stand-in for the missing WIC `EmbeddedThumbnailBackend` (open question O1): `open` answers `Ok(None)` ("the container has no frame"), which `embedded_thumbnail::try_read` maps to
/// "no embedded thumbnail", so the thumbnail cache falls back to decoding the file (slower first paint of the filmstrip, never wrong pixels). Logged nowhere. Replace by the real
/// backend the day it exists; the registry code does not change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnavailableEmbeddedThumbnails;

impl EmbeddedThumbnailBackend for UnavailableEmbeddedThumbnails {
    fn open(
        &self,
        stream: Box<dyn pr_core::abstractions::ReadSeek>,
    ) -> Result<Option<Box<dyn EmbeddedThumbnailFrame>>, ThumbnailBackendError> {
        todo!()
    }
}

/// The adapter C# `WicPixelScaler.Resize(current, w, h)` -> `pr_turbojpeg::decoder::FineScaler`: `pr_platform_win::wic::scaler::resize(source, width, height)`;
/// `ScaleError` WIC `E_OUTOFMEMORY` / failed destination allocation (the variants the doc of `ScaleError` names) -> `FineScaleError::OutOfMemory`, every other `ScaleError` ->
/// `FineScaleError::Failed(DecodeError)` carrying its `Display` text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WicFineScaler;

impl FineScaler for WicFineScaler {
    fn resize(&self, source: &PixelBuffer, width: i32, height: i32) -> Result<PixelBuffer, FineScaleError> {
        todo!()
    }
}
