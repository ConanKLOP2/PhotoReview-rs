//! Port of `PhotoReview.App.Shared/Composition/ServiceFactories.cs` (C# baseline `d92d6aae`, 80 lines).
//! Contract: `contracts/W8-09a.md`.
//!
//! Testable seams for four wirings of the service graph that no other test observes: the Recovery retry's live permanent-delete
//! setting, the RAW decoder's LibRaw last-resort wiring, the WebP/HEIC decoder chain and the preload's source-bytes prefetch.
//! `SharedServiceRegistration` (W8-09b) calls these. No DI container: plain functions with explicit arguments.
//!
//! C# overload pairs (public production overload + internal seam overload) become two functions each where a seam exists.
//! Everything here is synchronous (D-W8-2): the prefetch closure BLOCKS on the worker thread that calls it.

use std::io;
use std::path::Path;
use std::sync::Arc;

use pr_core::abstractions::{Clock, FileSystem, Log, NullLog, RecycleBin, SourceReader};
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::recovery_retry::{RecoveryRetryService, RecoveryRetryTexts};
use pr_core::file_actions::undo::FileActionGate;
use pr_core::model::enums::DecoderBackend;
use pr_core::settings::store::SettingsStore;
use pr_imaging::cache::source_bytes::SourceBytesCache;
use pr_imaging::cache::source_bytes::SourceBytesError;
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::fallback::FallbackImageDecoder;
use pr_imaging::preload::scheduler::PrefetchSourceBytesFn;
use pr_imaging::webp_heic::{WebpHeicRoutingDecoder, WicCodecSupport};
use pr_imaging_raw::RawPreviewFallback;
use pr_imaging_raw::decoder::{RawDecoder, RawDecoderOptions};

use pr_libraw::decoder::LibRawDecoder;
use pr_platform_win::wic::decoder::WicDirectDecoder;

use super::decoders::{DecoderDeps, LibRawPreviewFallback, ProbeFn};

/// C# `CreateRecoveryRetryService(journal, fileSystem, clock, recycleBin, settingsStore, fileActionGate = null)`.
/// The retry re-reads `AppSettings::allow_permanent_delete_without_recycle_bin` from `settings_store.current()` on EVERY call
/// (a setting changed after construction applies; Q-R8): the closure given to `with_allow_permanent_delete` captures the store,
/// never a copy of the value. `file_action_gate` = the app's single `FileActionService` (INV-4 gate, RV-C08). `texts` = the UI
/// sentences C# takes from `Tr` (production: the `pr_l10n` adapter, tests: `EnglishRecoveryRetryTexts`).
pub fn create_recovery_retry_service(
    journal: Arc<OperationJournal>,
    file_system: Arc<dyn FileSystem>,
    clock: Arc<dyn Clock>,
    recycle_bin: Arc<dyn RecycleBin>,
    settings_store: Arc<SettingsStore>,
    texts: Arc<dyn RecoveryRetryTexts>,
    file_action_gate: Option<Arc<dyn FileActionGate>>,
) -> RecoveryRetryService {
    let service = RecoveryRetryService::new(journal, file_system, clock, texts)
        .with_recycle_bin(recycle_bin)
        // Q-R8: read the live setting at every retry, never a copy taken now.
        .with_allow_permanent_delete(Arc::new(move || {
            settings_store.current().allow_permanent_delete_without_recycle_bin
        }));
    match file_action_gate {
        Some(gate) => service.with_file_action_gate(gate),
        None => service,
    }
}

/// The RAW wiring of C# `CreateRawDecoder` as plain data, so a test can inspect which seams were set (the C# test reads the
/// private fields `_previewFallback` and `_noPreviewDecoder` by reflection; `RawDecoder` has no accessors, `RawDecoderOptions`
/// has public fields). `libraw_probe` is called exactly ONCE. Available => `create_preview_fallback()` is called once and
/// stored in `preview_fallback`, and `create_libraw_decoder()` is called once and stored in `no_preview_decoder` (a RAW with
/// no embedded JPEG, e.g. a Leica M8 DNG, is then decoded by LibRaw instead of failing). Unavailable => NEITHER closure is
/// called and both options stay `None`. `source_reader` and `source_bytes_cache` are always passed through; `registry` and
/// `texts` stay `None` (defaults).
pub fn raw_decoder_options(
    source_reader: Arc<dyn SourceReader>,
    source_bytes_cache: Option<Arc<SourceBytesCache>>,
    libraw_probe: &ProbeFn,
    create_preview_fallback: &dyn Fn() -> Arc<dyn RawPreviewFallback>,
    create_libraw_decoder: &dyn Fn() -> Arc<dyn ImageDecoder>,
) -> RawDecoderOptions {
    // T2: the probe runs exactly once; T1: the creators run only when it says available.
    let available = (libraw_probe)().available;
    let preview_fallback = available.then(create_preview_fallback);
    // Both or neither: a RAW with no embedded JPEG (Leica M8 DNG, some phone DNGs) is decoded by LibRaw instead of failing.
    let no_preview_decoder = preview_fallback.as_ref().map(|_| create_libraw_decoder());
    RawDecoderOptions {
        source_reader: Some(source_reader),
        registry: None,
        source_bytes_cache,
        preview_fallback,
        no_preview_decoder,
        texts: None,
    }
}

/// C# internal `CreateRawDecoder(standardDecoder, sourceReader, cache, libRawProbe, createPreviewFallback, createLibRawDecoder)`:
/// `RawDecoder::with_options(standard_decoder, raw_decoder_options(..))`.
pub fn create_raw_decoder_with(
    standard_decoder: Arc<dyn ImageDecoder>,
    source_reader: Arc<dyn SourceReader>,
    source_bytes_cache: Option<Arc<SourceBytesCache>>,
    libraw_probe: &ProbeFn,
    create_preview_fallback: &dyn Fn() -> Arc<dyn RawPreviewFallback>,
    create_libraw_decoder: &dyn Fn() -> Arc<dyn ImageDecoder>,
) -> RawDecoder {
    RawDecoder::with_options(
        standard_decoder,
        raw_decoder_options(
            source_reader,
            source_bytes_cache,
            libraw_probe,
            create_preview_fallback,
            create_libraw_decoder,
        ),
    )
}

/// C# public `CreateRawDecoder(standardDecoder, sourceReader, cache, codec)`: the real LibRaw probe, `LibRawPreviewFallback`
/// over `deps.libraw_api` and a `LibRawDecoder` over `deps.codec`/`deps.libraw_api`/`deps.memory_info`.
pub fn create_raw_decoder(
    standard_decoder: Arc<dyn ImageDecoder>,
    source_bytes_cache: Option<Arc<SourceBytesCache>>,
    deps: &DecoderDeps,
) -> RawDecoder {
    let fallback_api = Arc::clone(&deps.libraw_api);
    let decoder_codec = Arc::clone(&deps.codec);
    let decoder_api = Arc::clone(&deps.libraw_api);
    let decoder_memory = Arc::clone(&deps.memory_info);
    create_raw_decoder_with(
        standard_decoder,
        Arc::clone(&deps.source_reader),
        source_bytes_cache,
        &super::decoders::libraw_probe(),
        &move || -> Arc<dyn RawPreviewFallback> { Arc::new(LibRawPreviewFallback::new(Arc::clone(&fallback_api))) },
        &move || -> Arc<dyn ImageDecoder> {
            Arc::new(LibRawDecoder::new(
                Arc::clone(&decoder_codec),
                Arc::clone(&decoder_api),
                Arc::clone(&decoder_memory),
            ))
        },
    )
}

/// C# `CreateWebpHeicDecoder(sourceReader, codec, wpfFallback, log, metrics)` (Q-FMT-WEBP-HEIC): the decoder chain every
/// WebP/HEIC path takes whatever backend the user picked. `WicDirectDecoder::new(deps.codec, Some(deps.source_reader),
/// deps.memory_info)` alone when `wpf_fallback` is `None` (Win32 host, NE-8), else
/// `FallbackImageDecoder::new(wic, DecoderBackend::WicDirect, wpf_fallback, log or NullLog, metrics)`.
pub fn create_webp_heic_decoder(
    deps: &DecoderDeps,
    wpf_fallback: Option<Arc<dyn ImageDecoder>>,
    log: Option<Arc<dyn Log>>,
    metrics: Option<Arc<ReviewMetrics>>,
) -> Arc<dyn ImageDecoder> {
    let wic: Arc<dyn ImageDecoder> = Arc::new(WicDirectDecoder::new(
        Arc::clone(&deps.codec),
        Some(Arc::clone(&deps.source_reader)),
        Arc::clone(&deps.memory_info),
    ));
    match wpf_fallback {
        None => wic,
        Some(wpf) => Arc::new(FallbackImageDecoder::new(
            wic,
            DecoderBackend::WicDirect,
            wpf,
            log.unwrap_or_else(|| Arc::new(NullLog)),
            metrics,
        )),
    }
}

/// C# `CreateSourcePrefetch(cache, isWebpHeicEnabled, codecs)`: the preload's source-bytes prefetch (the `PrefetchSourceBytesFn` of
/// `PreloadSchedulerDeps`). The returned closure, per call `(path, cancel)`:
/// 1. `WebpHeicRoutingDecoder::will_refuse(Some(path), &*is_webp_heic_enabled, &*codecs)` is true (switch off, or the Windows
///    codec of a WebP/HEIC file is missing) => `Ok(())` WITHOUT touching the cache or the disk (AGENTS.md priority 1);
/// 2. `cancel` already cancelled => `Err(DecodeError::Canceled)` (C# `Task.Run(..., token)` on a cancelled token) with no read;
/// 3. else `cache.try_prefetch(Path::new(path))` (its `Ok(false)` "too big to cache" is a success; its `SourceBytesError` maps
///    to `DecodeError::Io` / `InvalidArgument`).
///
/// `is_webp_heic_enabled` and `codecs` are read per call (the setting can change at runtime), never captured as values.
pub fn create_source_prefetch(
    cache: Arc<SourceBytesCache>,
    is_webp_heic_enabled: Arc<dyn Fn() -> bool + Send + Sync>,
    codecs: Arc<dyn Fn() -> WicCodecSupport + Send + Sync>,
) -> PrefetchSourceBytesFn {
    Arc::new(move |path, cancel| {
        // T6/T7: the refusal test comes first (INV-8: no read for a path no decode could use), then the cancel check.
        if WebpHeicRoutingDecoder::will_refuse(Some(path), &*is_webp_heic_enabled, &*codecs) {
            return Ok(());
        }
        if cancel.is_canceled() {
            return Err(DecodeError::Canceled);
        }
        match cache.try_prefetch(Path::new(path)) {
            // `Ok(false)` = too big to cache: nothing was opened, still a success (C# drops the bool).
            Ok(_) => Ok(()),
            Err(error) => Err(decode_error_of(&error)),
        }
    })
}

/// T8: `SourceBytesError::Io` holds an `Arc<io::Error>` (not `Clone`), so rebuild an `io::Error` with the same kind and text.
fn decode_error_of(error: &SourceBytesError) -> DecodeError {
    match error {
        SourceBytesError::Io(io_error) => DecodeError::Io(io::Error::new(io_error.kind(), io_error.to_string())),
        SourceBytesError::Argument { .. } | SourceBytesError::ArgumentOutOfRange { .. } => {
            DecodeError::InvalidArgument {
                message: error.to_string(),
            }
        }
    }
}
