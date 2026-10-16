//! Port tests for `pr_imaging::factory` (C# baseline d92d6aae, `PhotoReview.Imaging/Decoding/ImageDecoderFactory.cs`),
//! contract `contracts/W4-12a.md`. RED: every test fails through the stub's `todo!()` until the porter implements the bodies.
//!
//! Fakes replace the WPF/WIC/TurboJPEG decoders (no WIC in these tests). A [`Marker`] is a named decoder: `decode` and
//! `read_info` succeed with a 64 x 48 image whose platform image carries the marker name (golden `served_by`), or fail with
//! `DecodeError::NotSupported` (C# `MockFailingDecoder(new NotSupportedException())`). Its image reports
//! `actual_backend() == WicDirect`, as the golden markers do.
//!
//! The golden `decoder-factory` (tests/golden/decoder-factory.json, 21 cases) is driven by the inputs in
//! `tests/golden-inputs/decoder-factory.json`. Golden fields compared: `ctor_error`, `is_registered`, `provider_runs`,
//! `decorator_calls[].backend`, each create's `decorated`, `same_instance_as_previous`, `decode.ok/served_by/actual_backend`,
//! `read_info.ok` and `warnings`. Not compared: the `.NET` type names (`inner_type`, `error_type`, the `ArgumentException`
//! type) and the `(Parameter 'providers')` suffix of the constructor message, which is removed before comparing.
//! `wrapped_in_fallback` is checked through behaviour: a copy of the case whose backend provider fails (and whose Wpf slot is
//! healthy) must be served by the Wpf marker when the golden says wrapped, and must fail when it says not wrapped (for a
//! registered, non-Wpf backend). Rust has no downcast on `Arc<dyn ImageDecoder>`, so `Assert.IsType<FallbackImageDecoder>`
//! has no twin.
//!
//! Documented skips (not ported here, owned by the units named or with no Rust twin):
//! - `FactoryTests.PrimarySucceedsReturnsPrimaryBackend`, `FallbackOccursOnSupportedExceptions`, `MissingFileDoesNotFallback`,
//!   `ReadInfoFallsBack`: W4-12b (`FallbackImageDecoder`), `crates/pr-imaging/tests/fallback.rs`.
//! - `FactoryTests.ImageCacheKeyDistinguishesBackends`: W4-01a (`cache_key`).
//! - `FactoryTests.PreviewImageServiceIntegratesDecoderFactory`: W4-14 (`PreviewImageService`).
//! - `Wp06WpfFreeExceptionMappingTests`: W4-12b / W4-09 (exception mapping and cache-entry failure).
//! - `DecodingHelpersMutationTests` other tests (FallbackImageDecoder, MemoryHeadroom): W4-12b and W4-03a/b.
//! - `DecoderRegistrationTests.Release_RegistersEveryBackendOfferedInSettings`: App composition, W8 (needs the real provider list).
//! - `DecoderRegistrationTests.ImageDecoderFactory_HasNoReflectionLoading`: source scan of C# `Type.GetType`/`Activator`;
//!   Rust has no dynamic loading to scan (the factory only calls the supplied closures).
//! - `WebpHeicNativeTests`, `DecoderQualityGateTests`, `WicColorProfileTests`, `PreviewBackendIdentityTests`, `SoakLeakTests`:
//!   they only USE the factory (W4-12c, W4-11c, W4-16b, W4-14, W8).

use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, OnceLock, PoisonError, Weak, mpsc};
use std::thread;
use std::time::Duration;

use pr_core::abstractions::Log;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::model::enums::DecoderBackend;
use pr_core::model::serde_ext::LenientEnum;
use pr_imaging::decode_types::{
    DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo, PlatformImage,
};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, ImageDecoderFactory};
use pr_imaging::factory::{
    DecoderDecorator, DecoderFactoryError, DecoderFactoryOptions, DecoderProvider, DefaultImageDecoderFactory,
};
use pr_imaging::fallback::FALLBACK_BACKEND;
use pr_test_support::golden;
use pr_test_support::{LogLevel, RecordingLog};
use serde_json::Value;

/// Path every decode and read in this file uses (also the label of the golden warnings).
const PATH: &str = "x.jpg";
/// Size of every fake decoded image (the C# tests assert the 64 x 48 fixture).
const IMAGE_WIDTH: i32 = 64;
const IMAGE_HEIGHT: i32 = 48;
/// The .NET suffix of `ArgumentException.Message` for the `providers` parameter (not part of the Rust `Display`).
const CTOR_PARAM_SUFFIX: &str = " (Parameter 'providers')";
/// The spin of the C# concurrency test (`Thread.SpinWait(2_000_000)`).
const SPIN_ITERATIONS: usize = 2_000_000;
/// The C# concurrency test runs 16 threads.
const THREADS: usize = 16;

/// Run counts per provider name (golden `provider_runs`; names without a run are absent).
type Runs = Arc<Mutex<BTreeMap<String, u64>>>;

/// A named fake decoder. `fails`: `decode` and `read_info` return `NotSupported` with the message `marker <name> fails`.
#[derive(Debug)]
struct Marker {
    name: String,
    fails: bool,
}

impl Marker {
    fn ok(name: &str) -> Arc<dyn ImageDecoder> {
        Arc::new(Self {
            name: name.to_owned(),
            fails: false,
        })
    }

    fn failing(name: &str) -> Arc<dyn ImageDecoder> {
        Arc::new(Self {
            name: name.to_owned(),
            fails: true,
        })
    }

    fn refusal(&self) -> DecodeError {
        DecodeError::NotSupported {
            message: format!("marker {} fails", self.name),
            user_text: None,
        }
    }
}

impl ImageDecoder for Marker {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        if self.fails {
            return Err(self.refusal());
        }
        let options = DecodedImageOptions {
            actual_backend: DecoderBackend::WicDirect,
            ..DecodedImageOptions::default()
        };
        let image = DecodedImage::new(
            PlatformImage::Opaque(Arc::new(self.name.clone())),
            IMAGE_WIDTH,
            IMAGE_HEIGHT,
            0,
            options,
        )
        .unwrap_or_else(|error| panic!("fake image is valid: {error}"));
        let image: Arc<dyn DecodedImageView> = Arc::new(image);
        Ok(image)
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        if self.fails {
            return Err(self.refusal());
        }
        Ok(ImageInfo::new(IMAGE_WIDTH, IMAGE_HEIGHT))
    }
}

/// Forwards every call to `inner`: the decorator of the golden cases (C# `W412aDecorated`, which records and forwards).
struct Forward(Arc<dyn ImageDecoder>);

impl ImageDecoder for Forward {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.0.decode(request)
    }

    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        self.0.read_info(path)
    }
}

/// What the decorator of a test saw: the requested backend of each call (golden `decorator_calls`, in call order) and the
/// address of each decorator output (golden `decorated`: a create result is decorated when its address is one of these).
#[derive(Debug, Default)]
struct DecoratorRecorder {
    calls: Mutex<Vec<DecoderBackend>>,
    outputs: Mutex<Vec<usize>>,
}

impl DecoratorRecorder {
    fn decorator(self: &Arc<Self>) -> DecoderDecorator {
        let recorder = Arc::clone(self);
        Arc::new(
            move |backend: DecoderBackend, inner: Arc<dyn ImageDecoder>| -> Arc<dyn ImageDecoder> {
                recorder
                    .calls
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(backend);
                let wrapper: Arc<dyn ImageDecoder> = Arc::new(Forward(inner));
                recorder
                    .outputs
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(address(&wrapper));
                wrapper
            },
        )
    }
}

/// The address of the allocation behind a decoder handle (identity of a shared instance).
fn address(decoder: &Arc<dyn ImageDecoder>) -> usize {
    Arc::as_ptr(decoder).cast::<u8>() as usize
}

/// The marker name that produced `image` (golden `served_by`).
fn served_by(image: &dyn DecodedImageView) -> String {
    match image.platform_image() {
        PlatformImage::Opaque(any) => any
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| panic!("platform image is not a marker name")),
        PlatformImage::Pixels(_) => panic!("platform image is pixels, not a marker name"),
    }
}

/// A provider that always hands out `decoder` (the same instance on every build, as C# tests with a captured instance).
fn returning(decoder: &Arc<dyn ImageDecoder>) -> DecoderProvider {
    let decoder = Arc::clone(decoder);
    Arc::new(move || Arc::clone(&decoder))
}

/// A provider from a closure.
fn provider(build: impl Fn() -> Arc<dyn ImageDecoder> + Send + Sync + 'static) -> DecoderProvider {
    Arc::new(build)
}

fn build_default(
    providers: Vec<(DecoderBackend, DecoderProvider)>,
    options: DecoderFactoryOptions,
) -> DefaultImageDecoderFactory {
    DefaultImageDecoderFactory::new(providers, options)
        .unwrap_or_else(|error| panic!("factory construction failed: {error}"))
}

// ---- factory-only tests (C# FactoryTests.cs) ----

// C#: FactoryTests.FactoryReturnsWpfDecoderDirectly
#[test]
fn factory_returns_wpf_decoder_directly() {
    let wpf = Marker::ok("wpf");
    let factory = build_default(
        Vec::new(),
        DecoderFactoryOptions {
            wpf_decoder_factory: Some(returning(&wpf)),
            ..DecoderFactoryOptions::default()
        },
    );

    assert!(
        Arc::ptr_eq(&factory.create(DecoderBackend::Wpf), &wpf),
        "Create(Wpf) is the provider's instance, not wrapped"
    );
}

/// C# `FactoryWrapsRegisteredBackendInFallbackDecoder`, theory row `WicDirect`.
// C#: FactoryTests.FactoryWrapsRegisteredBackendInFallbackDecoder
#[test]
fn factory_wraps_registered_backend_in_fallback_decoder_wic_direct() {
    assert_wraps_registered_backend_in_fallback(DecoderBackend::WicDirect);
}

/// C# `FactoryWrapsRegisteredBackendInFallbackDecoder`, theory row `TurboJpeg`.
// C#: FactoryTests.FactoryWrapsRegisteredBackendInFallbackDecoder
#[test]
fn factory_wraps_registered_backend_in_fallback_decoder_turbo_jpeg() {
    assert_wraps_registered_backend_in_fallback(DecoderBackend::TurboJpeg);
}

fn assert_wraps_registered_backend_in_fallback(backend: DecoderBackend) {
    assert_eq!(
        FALLBACK_BACKEND,
        DecoderBackend::Wpf,
        "the fallback is always the Wpf slot"
    );
    let factory = build_default(
        vec![
            (backend, returning(&Marker::failing("primary"))),
            (DecoderBackend::Wpf, returning(&Marker::ok("wpf"))),
        ],
        DecoderFactoryOptions::default(),
    );

    // The primary throws NotSupportedException, so the decode is served by the Wpf fallback (the wrapper is observable
    // through this behaviour only). The fallback decoder's image is 64 x 48, as the C# fixture asserts.
    let decoded = factory
        .create(backend)
        .decode(&DecodeRequest::new(PATH, 0))
        .unwrap_or_else(|error| panic!("{backend:?}: the Wpf fallback must serve the decode: {error}"));
    assert_eq!(served_by(&*decoded), "wpf", "{backend:?}: served by the Wpf fallback");
    assert_eq!(decoded.pixel_width(), IMAGE_WIDTH);
    assert_eq!(decoded.pixel_height(), IMAGE_HEIGHT);
}

// C#: FactoryTests.Create_UnregisteredBackend_ReturnsWpfDecoder
#[test]
fn create_unregistered_backend_returns_wpf_decoder() {
    let wpf = Marker::ok("wpf");
    let factory = build_default(
        Vec::new(),
        DecoderFactoryOptions {
            wpf_decoder_factory: Some(returning(&wpf)),
            ..DecoderFactoryOptions::default()
        },
    );

    let decoder = factory.create(DecoderBackend::TurboJpeg);

    assert!(
        Arc::ptr_eq(&decoder, &wpf),
        "an unregistered backend is the Wpf decoder itself, not wrapped"
    );
}

// C#: FactoryTests.Create_SameBackendTwice_ReturnsSameInstanceAndBuildsOnce
#[test]
fn create_same_backend_twice_returns_same_instance_and_builds_once() {
    let wic_builds = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wic_builds);
    let factory = build_default(
        vec![
            (DecoderBackend::Wpf, returning(&Marker::ok("wpf"))),
            (
                DecoderBackend::WicDirect,
                provider(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Marker::ok("wic")
                }),
            ),
        ],
        DecoderFactoryOptions::default(),
    );

    let wic = factory.create(DecoderBackend::WicDirect);
    assert!(
        Arc::ptr_eq(&wic, &factory.create(DecoderBackend::WicDirect)),
        "WicDirect is one shared instance"
    );
    let wpf = factory.create(DecoderBackend::Wpf);
    assert!(
        Arc::ptr_eq(&wpf, &factory.create(DecoderBackend::Wpf)),
        "Wpf is one shared instance"
    );
    assert!(!Arc::ptr_eq(&wpf, &wic), "Wpf and WicDirect are different instances");
    assert_eq!(wic_builds.load(Ordering::SeqCst), 1, "the WicDirect provider runs once");
}

// C#: FactoryTests.Create_Concurrent_BuildsAndDecoratesOnce
#[test]
fn create_concurrent_builds_and_decorates_once() {
    let provider_runs = Arc::new(AtomicUsize::new(0));
    let decorator_runs = Arc::new(AtomicUsize::new(0));
    let slow_runs = Arc::clone(&provider_runs);
    let decorated_runs = Arc::clone(&decorator_runs);
    let factory = build_default(
        vec![
            (DecoderBackend::Wpf, returning(&Marker::ok("wpf"))),
            (
                DecoderBackend::WicDirect,
                provider(move || {
                    slow_runs.fetch_add(1, Ordering::SeqCst);
                    for _ in 0..SPIN_ITERATIONS {
                        std::hint::spin_loop();
                    }
                    Marker::ok("wic")
                }),
            ),
        ],
        DecoderFactoryOptions {
            decoder_decorator: Some(Arc::new(
                move |_backend: DecoderBackend, decoder: Arc<dyn ImageDecoder>| -> Arc<dyn ImageDecoder> {
                    decorated_runs.fetch_add(1, Ordering::SeqCst);
                    decoder
                },
            )),
            ..DecoderFactoryOptions::default()
        },
    );

    let barrier = Barrier::new(THREADS);
    let results: Vec<Arc<dyn ImageDecoder>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    factory.create(DecoderBackend::WicDirect)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or_else(|_| panic!("a worker thread panicked")))
            .collect()
    });

    assert_eq!(
        provider_runs.load(Ordering::SeqCst),
        1,
        "the provider runs exactly once"
    );
    assert_eq!(
        decorator_runs.load(Ordering::SeqCst),
        1,
        "the decorator runs exactly once"
    );
    for result in &results {
        assert!(Arc::ptr_eq(result, &results[0]), "all 16 callers share one instance");
    }
}

// C#: FactoryTests.IsRegistered_ReflectsProviders
#[test]
fn is_registered_reflects_providers() {
    let factory = build_default(
        vec![
            (DecoderBackend::Wpf, returning(&Marker::ok("wpf"))),
            (DecoderBackend::WicDirect, returning(&Marker::ok("wic"))),
        ],
        DecoderFactoryOptions::default(),
    );

    assert!(factory.is_registered(DecoderBackend::Wpf));
    assert!(factory.is_registered(DecoderBackend::WicDirect));
    assert!(!factory.is_registered(DecoderBackend::TurboJpeg));
}

// ---- Wp06 Wpf-slot tests (C# Wp06DecoderFactoryWpfSlotTests in Pixels/Wp06BridgeSplitTests.cs) ----

// C#: Wp06DecoderFactoryWpfSlotTests.WithoutWpf_MapsWpfToWicDirect
#[test]
fn without_wpf_maps_wpf_to_wic_direct() {
    let wic = Marker::ok("wic");
    let factory = build_default(
        vec![(DecoderBackend::WicDirect, returning(&wic))],
        DecoderFactoryOptions::default(),
    );

    // The persisted "Wpf" setting is only asked for, never rewritten: the slot is served by WIC Direct.
    assert!(
        Arc::ptr_eq(&factory.create(DecoderBackend::Wpf), &wic),
        "Create(Wpf) is the WicDirect provider's instance"
    );
    assert!(factory.is_registered(DecoderBackend::WicDirect));
    assert!(factory.is_registered(DecoderBackend::Wpf), "the slot is always served");
}

// C#: Wp06DecoderFactoryWpfSlotTests.WithoutWpf_WicDirectIsNotWrapped
#[test]
fn without_wpf_wic_direct_is_not_wrapped() {
    let wic = Marker::ok("wic");
    let factory = build_default(
        vec![(DecoderBackend::WicDirect, returning(&wic))],
        DecoderFactoryOptions::default(),
    );

    assert!(
        Arc::ptr_eq(&factory.create(DecoderBackend::WicDirect), &wic),
        "no fallback to itself: the WicDirect instance is handed out unwrapped"
    );
}

// C#: Wp06DecoderFactoryWpfSlotTests.WithoutWpf_OtherBackendFallsBackToWicDirect
#[test]
fn without_wpf_other_backend_falls_back_to_wic_direct() {
    let factory = build_default(
        vec![
            (DecoderBackend::WicDirect, returning(&Marker::ok("wic"))),
            (DecoderBackend::TurboJpeg, returning(&Marker::failing("turbo"))),
        ],
        DecoderFactoryOptions::default(),
    );

    // The TurboJpeg marker fails, so the decode is served by the WicDirect marker (the fallback wrapper is observable
    // through this behaviour only; C# `Assert.IsType<FallbackImageDecoder>`).
    let decoded = factory
        .create(DecoderBackend::TurboJpeg)
        .decode(&DecodeRequest::new(PATH, 0))
        .unwrap_or_else(|error| panic!("the WicDirect fallback must serve the decode: {error}"));
    assert_eq!(served_by(&*decoded), "wic");
}

// C#: Wp06DecoderFactoryWpfSlotTests.WpfDecoderFactory_FillsTheSlot_ExplicitProviderWins
#[test]
fn wpf_decoder_factory_fills_the_slot_explicit_provider_wins() {
    let from_factory = Marker::ok("factory");
    let explicit_wpf = Marker::ok("explicit");

    let via_factory = build_default(
        vec![(DecoderBackend::WicDirect, returning(&Marker::ok("wic")))],
        DecoderFactoryOptions {
            wpf_decoder_factory: Some(returning(&from_factory)),
            ..DecoderFactoryOptions::default()
        },
    );
    let both = build_default(
        vec![(DecoderBackend::Wpf, returning(&explicit_wpf))],
        DecoderFactoryOptions {
            wpf_decoder_factory: Some(returning(&from_factory)),
            ..DecoderFactoryOptions::default()
        },
    );

    assert!(
        Arc::ptr_eq(&via_factory.create(DecoderBackend::Wpf), &from_factory),
        "wpfDecoderFactory fills the Wpf slot"
    );
    assert!(
        Arc::ptr_eq(&both.create(DecoderBackend::Wpf), &explicit_wpf),
        "an explicit Wpf provider wins over wpfDecoderFactory"
    );
}

// C#: Wp06DecoderFactoryWpfSlotTests.NoWpfAndNoWicDirect_Throws
#[test]
fn no_wpf_and_no_wic_direct_throws() {
    let error = DefaultImageDecoderFactory::new(
        vec![(DecoderBackend::TurboJpeg, returning(&Marker::ok("turbo")))],
        DecoderFactoryOptions::default(),
    )
    .err()
    .unwrap_or_else(|| panic!("a factory without Wpf, wpfDecoderFactory or WicDirect is a composition error"));

    assert_eq!(error, DecoderFactoryError::NoWpfDecoder);
    assert_eq!(error.to_string(), DecoderFactoryError::NO_WPF_DECODER_MESSAGE);
}

// ---- DecodingHelpersMutationTests (C# Decoding/DecodingHelpersMutationTests.cs, factory part) ----

// C#: DecodingHelpersMutationTests.Factory_CustomProvider_IsTheBackendThatGetsBuilt
#[test]
fn factory_custom_provider_is_the_backend_that_gets_built() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let factory = build_default(
        vec![(
            DecoderBackend::WicDirect,
            provider(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                Marker::ok("wic")
            }),
        )],
        DecoderFactoryOptions::default(),
    );

    let decoded = factory
        .create(DecoderBackend::WicDirect)
        .decode(&DecodeRequest::new(PATH, 0))
        .unwrap_or_else(|error| panic!("the registered WicDirect provider decodes: {error}"));

    assert!(factory.is_registered(DecoderBackend::WicDirect));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the custom provider is called exactly once"
    );
    assert_eq!(decoded.actual_backend(), DecoderBackend::WicDirect);
}

// C#: DecodingHelpersMutationTests.Factory_UnregisteredBackend_WarnsThroughTheInjectedLogAndGivesWpf
#[test]
fn factory_unregistered_backend_warns_through_the_injected_log_and_gives_wpf() {
    let log = Arc::new(RecordingLog::new());
    let injected: Arc<dyn Log> = log.clone();
    let factory = build_default(
        Vec::new(),
        DecoderFactoryOptions {
            log: Some(injected),
            wpf_decoder_factory: Some(returning(&Marker::ok("wpf"))),
            ..DecoderFactoryOptions::default()
        },
    );

    let decoded = factory
        .create(DecoderBackend::TurboJpeg)
        .decode(&DecodeRequest::new(PATH, 0))
        .unwrap_or_else(|error| panic!("the Wpf slot serves an unregistered backend: {error}"));

    assert_eq!(
        served_by(&*decoded),
        "wpf",
        "an unregistered backend gives the Wpf decoder"
    );
    assert!(
        log.contains(LogLevel::Warn, "not registered"),
        "the warning goes through the injected log: {:?}",
        log.entries()
    );
}

// Rust-only (no C# twin): the contract passes `metrics` to every fallback wrapper (`new FallbackImageDecoder(..., _metrics)`).
#[test]
fn factory_passes_metrics_to_the_fallback_wrapper() {
    let metrics = Arc::new(ReviewMetrics::new());
    let factory = build_default(
        vec![
            (DecoderBackend::Wpf, returning(&Marker::ok("wpf"))),
            (DecoderBackend::TurboJpeg, returning(&Marker::failing("turbo"))),
        ],
        DecoderFactoryOptions {
            metrics: Some(Arc::clone(&metrics)),
            ..DecoderFactoryOptions::default()
        },
    );

    let decoded = factory
        .create(DecoderBackend::TurboJpeg)
        .decode(&DecodeRequest::new(PATH, 0))
        .unwrap_or_else(|error| panic!("the Wpf slot serves the failed primary: {error}"));

    assert_eq!(served_by(&*decoded), "wpf");
    assert_eq!(
        metrics.snapshot().decoder_fallback_count(),
        1,
        "the fallback is counted in the injected metrics"
    );
}

// Rust-only: contract note 4. The map lock is not held while a provider or decorator runs: a decorator that creates ANOTHER
// backend (as the App's routing decorator may) must not deadlock (C# `ConcurrentDictionary` + `Lazy` allows it).
#[test]
fn decorator_may_create_another_backend_without_deadlock() {
    let slot: Arc<OnceLock<Weak<DefaultImageDecoderFactory>>> = Arc::new(OnceLock::new());
    let seen_wpf: Arc<Mutex<Option<Arc<dyn ImageDecoder>>>> = Arc::new(Mutex::new(None));
    let (slot_in, seen_in) = (Arc::clone(&slot), Arc::clone(&seen_wpf));
    let decorator: DecoderDecorator = Arc::new(
        move |backend: DecoderBackend, decoder: Arc<dyn ImageDecoder>| -> Arc<dyn ImageDecoder> {
            if backend == DecoderBackend::WicDirect {
                let factory = slot_in.get().and_then(Weak::upgrade).expect("factory is alive");
                *seen_in.lock().unwrap_or_else(PoisonError::into_inner) = Some(factory.create(DecoderBackend::Wpf));
            }
            decoder
        },
    );
    let factory = Arc::new(build_default(
        vec![
            (DecoderBackend::Wpf, returning(&Marker::ok("wpf"))),
            (DecoderBackend::WicDirect, returning(&Marker::ok("wic"))),
        ],
        DecoderFactoryOptions {
            decoder_decorator: Some(decorator),
            ..DecoderFactoryOptions::default()
        },
    ));
    assert!(slot.set(Arc::downgrade(&factory)).is_ok());

    let (sender, receiver) = mpsc::channel();
    let worker = Arc::clone(&factory);
    thread::spawn(move || {
        let created = worker.create(DecoderBackend::WicDirect);
        let _ = sender.send(created);
    });
    let wic = receiver
        .recv_timeout(Duration::from_secs(20))
        .unwrap_or_else(|_| panic!("create(WicDirect) deadlocked: the map lock is held during the build"));

    let inner_wpf = seen_wpf
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .expect("the decorator created the Wpf backend");
    assert!(
        Arc::ptr_eq(&inner_wpf, &factory.create(DecoderBackend::Wpf)),
        "the nested create is cached like any other"
    );
    assert!(Arc::ptr_eq(&wic, &factory.create(DecoderBackend::WicDirect)));
}

// ---- golden decoder-factory (21 cases) ----

/// One golden input case (tests/golden-inputs/decoder-factory.json).
#[derive(Debug, Clone)]
struct CaseInput {
    name: String,
    providers: Vec<ProviderInput>,
    wpf_factory: Option<ProviderInput>,
    decorator: bool,
    create: Vec<DecoderBackend>,
}

/// One provider of a golden case: `fails` makes the decoder it builds fail with `NotSupported`.
#[derive(Debug, Clone)]
struct ProviderInput {
    backend: DecoderBackend,
    name: String,
    fails: bool,
}

fn backend_of(name: &str) -> DecoderBackend {
    match name {
        "Wpf" => DecoderBackend::Wpf,
        "WicDirect" => DecoderBackend::WicDirect,
        "TurboJpeg" => DecoderBackend::TurboJpeg,
        "LibRaw" => DecoderBackend::LibRaw,
        other => panic!("unknown backend {other}"),
    }
}

fn text_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("missing string '{key}' in {value}"))
}

fn provider_input(value: &Value, backend: DecoderBackend) -> ProviderInput {
    ProviderInput {
        backend,
        name: text_of(value, "name").to_owned(),
        fails: value["fails"].as_bool().unwrap_or(false),
    }
}

fn parse_case(value: &Value) -> CaseInput {
    let providers = value["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|provider| provider_input(provider, backend_of(text_of(provider, "backend"))))
        .collect();
    let wpf_factory = value
        .get("wpf_factory")
        .map(|factory| provider_input(factory, DecoderBackend::Wpf));
    let create = value["create"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|backend| backend_of(backend.as_str().unwrap_or_default()))
        .collect();
    CaseInput {
        name: text_of(value, "name").to_owned(),
        providers,
        wpf_factory,
        decorator: value["decorator"].as_bool().unwrap_or(false),
        create,
    }
}

/// The golden inputs, by case name.
fn load_inputs() -> BTreeMap<String, CaseInput> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("decoder-factory.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("no 'cases' in {}", path.display()))
        .iter()
        .map(parse_case)
        .map(|case| (case.name.clone(), case))
        .collect()
}

/// Builds the provider for one input entry; every run is counted under the entry's name.
fn provider_for(spec: &ProviderInput, runs: &Runs) -> DecoderProvider {
    let spec = spec.clone();
    let runs = Arc::clone(runs);
    provider(move || {
        *runs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(spec.name.clone())
            .or_insert(0) += 1;
        if spec.fails {
            Marker::failing(&spec.name)
        } else {
            Marker::ok(&spec.name)
        }
    })
}

/// The constructor under test, built from one input case.
fn build(
    case: &CaseInput,
    runs: &Runs,
    log: Arc<dyn Log>,
    decorator: Option<DecoderDecorator>,
) -> Result<DefaultImageDecoderFactory, DecoderFactoryError> {
    let providers: Vec<(DecoderBackend, DecoderProvider)> = case
        .providers
        .iter()
        .map(|spec| (spec.backend, provider_for(spec, runs)))
        .collect();
    let options = DecoderFactoryOptions {
        log: Some(log),
        metrics: None,
        decoder_decorator: decorator,
        wpf_decoder_factory: case.wpf_factory.as_ref().map(|spec| provider_for(spec, runs)),
    };
    DefaultImageDecoderFactory::new(providers, options)
}

/// The name of the provider that serves the Wpf slot: an explicit `Wpf` provider wins over `wpfDecoderFactory`, and with
/// neither the slot is mapped to the last `WicDirect` provider.
fn wpf_slot_name(case: &CaseInput) -> Option<String> {
    case.providers
        .iter()
        .rev()
        .find(|spec| spec.backend == DecoderBackend::Wpf)
        .map(|spec| spec.name.clone())
        .or_else(|| case.wpf_factory.as_ref().map(|spec| spec.name.clone()))
        .or_else(|| {
            case.providers
                .iter()
                .rev()
                .find(|spec| spec.backend == DecoderBackend::WicDirect)
                .map(|spec| spec.name.clone())
        })
}

/// The copy of `case` used to observe a fallback wrapper: the backend's provider fails, the Wpf slot is healthy, no decorator.
fn probe_case(case: &CaseInput, backend: DecoderBackend) -> CaseInput {
    let mut probe = case.clone();
    for spec in &mut probe.providers {
        if spec.backend == backend {
            spec.fails = true;
        }
        if spec.backend == DecoderBackend::Wpf {
            spec.fails = false;
        }
    }
    if let Some(spec) = probe.wpf_factory.as_mut() {
        spec.fails = false;
    }
    probe.decorator = false;
    probe
}

/// `wrapped_in_fallback` through behaviour: a registered, non-Wpf backend whose primary fails is served by the Wpf marker
/// when the golden says wrapped, and fails when it says not wrapped.
fn check_wrapping(input: &CaseInput, backend: DecoderBackend, wrapped: bool, registered: bool, label: &str) {
    if backend == DecoderBackend::Wpf || !registered {
        return;
    }
    let probe = probe_case(input, backend);
    let probe_runs = Runs::default();
    let probe_log: Arc<dyn Log> = Arc::new(RecordingLog::new());
    let factory = build(&probe, &probe_runs, probe_log, None)
        .unwrap_or_else(|error| panic!("{label}: probe constructor failed: {error}"));
    let result = factory.create(backend).decode(&DecodeRequest::new(PATH, 0));
    if wrapped {
        let image = result.unwrap_or_else(|error| panic!("{label}: a failing primary must fall back to Wpf: {error}"));
        let slot = wpf_slot_name(input).unwrap_or_else(|| panic!("{label}: wrapped without a Wpf slot"));
        assert_eq!(
            served_by(&*image),
            slot,
            "{label}: the Wpf slot serves the failing primary"
        );
    } else {
        assert!(result.is_err(), "{label}: an unwrapped primary must not fall back");
    }
}

fn text_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("not a string: {item}"))
                .to_owned()
        })
        .collect()
}

/// Checks one golden case against the factory built from its input.
fn check_case(input: &CaseInput, expected: &Value) {
    let name = input.name.as_str();
    let runs = Runs::default();
    let log = Arc::new(RecordingLog::new());
    let recorder = Arc::new(DecoratorRecorder::default());
    let decorator = input.decorator.then(|| recorder.decorator());

    let log_dyn: Arc<dyn Log> = log.clone();
    match (
        build(input, &runs, log_dyn, decorator),
        expected["ctor_error"].is_null(),
    ) {
        (Err(error), false) => {
            let message = expected["ctor_error"]["message"]
                .as_str()
                .unwrap_or_else(|| panic!("case {name}: ctor_error has no message"));
            let display = message.strip_suffix(CTOR_PARAM_SUFFIX).unwrap_or(message);
            assert_eq!(
                error,
                DecoderFactoryError::NoWpfDecoder,
                "case {name}: constructor error"
            );
            assert_eq!(error.to_string(), display, "case {name}: constructor error message");
        }
        (Err(error), true) => panic!("case {name}: unexpected constructor error: {error}"),
        (Ok(_), false) => panic!("case {name}: golden expects a constructor error"),
        (Ok(factory), true) => check_built(input, expected, &factory, &runs, &log, &recorder),
    }
}

fn check_built(
    input: &CaseInput,
    expected: &Value,
    factory: &DefaultImageDecoderFactory,
    runs: &Runs,
    log: &RecordingLog,
    recorder: &DecoratorRecorder,
) {
    let name = input.name.as_str();

    for (backend, key) in [
        (DecoderBackend::Wpf, "Wpf"),
        (DecoderBackend::WicDirect, "WicDirect"),
        (DecoderBackend::TurboJpeg, "TurboJpeg"),
        (DecoderBackend::LibRaw, "LibRaw"),
    ] {
        let want = expected["is_registered"][key]
            .as_bool()
            .unwrap_or_else(|| panic!("case {name}: no is_registered.{key}"));
        assert_eq!(
            factory.is_registered(backend),
            want,
            "case {name}: is_registered({key})"
        );
    }

    let creates = expected["create"]
        .as_array()
        .unwrap_or_else(|| panic!("case {name}: no create list"));
    assert_eq!(creates.len(), input.create.len(), "case {name}: create count");

    let mut results: Vec<Arc<dyn ImageDecoder>> = Vec::new();
    for (index, (backend, want)) in input.create.iter().zip(creates).enumerate() {
        let label = format!("case {name}, create #{index} ({})", backend.name());
        let decoder = factory.create(*backend);

        let decorated = recorder
            .outputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&address(&decoder));
        assert_eq!(Some(decorated), want["decorated"].as_bool(), "{label}: decorated");

        let previous = input.create[..index]
            .iter()
            .rposition(|earlier| earlier == backend)
            .map(|earlier| Arc::ptr_eq(&decoder, &results[earlier]));
        assert_eq!(
            previous,
            want["same_instance_as_previous"].as_bool(),
            "{label}: same_instance_as_previous"
        );

        let decode_want = &want["decode"];
        match decoder.decode(&DecodeRequest::new(PATH, 0)) {
            Ok(image) => {
                assert_eq!(decode_want["ok"].as_bool(), Some(true), "{label}: decode ok");
                assert_eq!(
                    decode_want["served_by"].as_str(),
                    Some(served_by(&*image).as_str()),
                    "{label}: served_by"
                );
                assert_eq!(
                    decode_want["actual_backend"].as_str(),
                    Some(image.actual_backend().name()),
                    "{label}: actual_backend"
                );
            }
            Err(error) => assert_eq!(
                decode_want["ok"].as_bool(),
                Some(false),
                "{label}: decode failed with {error}"
            ),
        }

        let read_info_ok = decoder.read_info(PATH).is_ok();
        assert_eq!(
            Some(read_info_ok),
            want["read_info"]["ok"].as_bool(),
            "{label}: read_info ok"
        );

        let wrapped = want["wrapped_in_fallback"]
            .as_bool()
            .unwrap_or_else(|| panic!("{label}: no wrapped_in_fallback"));
        let registered = factory.is_registered(*backend);
        check_wrapping(input, *backend, wrapped, registered, &label);

        results.push(decoder);
    }

    let actual_runs: BTreeMap<String, u64> = runs.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let want_runs: BTreeMap<String, u64> = expected["provider_runs"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key, count)| {
            (
                key.clone(),
                count
                    .as_u64()
                    .unwrap_or_else(|| panic!("case {name}: provider_runs.{key} is not a count")),
            )
        })
        .collect();
    assert_eq!(actual_runs, want_runs, "case {name}: provider_runs");

    let got_calls: Vec<String> = recorder
        .calls
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(|backend| backend.name().to_owned())
        .collect();
    let want_calls: Vec<String> = expected["decorator_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|call| text_of(call, "backend").to_owned())
        .collect();
    assert_eq!(
        got_calls, want_calls,
        "case {name}: decorator_calls (backends, in order)"
    );

    let got_warnings: Vec<String> = log
        .entries()
        .into_iter()
        .filter(|(level, _)| *level == LogLevel::Warn)
        .map(|(_, message)| message)
        .collect();
    assert_eq!(got_warnings, text_list(&expected["warnings"]), "case {name}: warnings");
}

#[test]
fn golden_decoder_factory_matches_csharp() {
    let golden_doc = golden::load("decoder-factory");
    let inputs = load_inputs();
    assert_eq!(golden_doc.cases.len(), 21, "the golden has 21 cases");
    for expected in &golden_doc.cases {
        let name = expected["name"]
            .as_str()
            .unwrap_or_else(|| panic!("golden case without a name: {expected}"));
        let input = inputs
            .get(name)
            .unwrap_or_else(|| panic!("no golden input for case {name}"));
        check_case(input, expected);
    }
}
