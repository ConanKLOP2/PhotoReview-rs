//! Port of the loader-level tests of `PhotoReview.App.Tests/Coordinators` for `ZoomDetailLoader` (C# baseline d92d6aae):
//! `ZoomDetailTests.cs`, `ZoomDetailLoaderGapTests.cs`, `ZoomDetailLoaderGapTests.Refresh.cs` and
//! `ZoomDetailLoaderGapTests.MutationGaps.cs` (PARITY I-15, I-16, INV-1). Contract: `contracts/W8-07g.md`.
//!
//! The Rust twins drive the LOADER directly (no presenter, no viewer): the viewer / layout half of each C# test belongs to
//! W8-06a, the presenter half to W8-07h..k. Decodes run on real worker threads behind gates the tests open themselves; every
//! wait is a condition wait bounded by a hang guard (`wait_for_event`, `Gate::wait`), never a sleep.

mod support {
    pub mod preview_fakes;
}

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::time::Duration;

use pr_app::coordinators::zoom_detail::{
    CompletionInbox, IndicatorDelay, ThreadRunner, ZoomDetailEffect, ZoomDetailLoader, ZoomDetailRefreshResult,
    default_indicator_delay, same_platform_image,
};
use pr_core::catalog::generation::GenerationClock;
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::PlatformImage;
use pr_imaging::decoder_traits::ImageDecoder;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_imaging::service::{PreviewImage, PreviewImageService};
use pr_test_support::TempRoot;
use support::preview_fakes::{
    EmbeddedRawDecoder, IndicatorGates, ServiceOptions, SizedDecoder, TIMEOUT, build_service, sized_image,
};

// ---- fixture --------------------------------------------------------------------------------------------------------

/// Counts the loader's wake calls and lets a test wait for a count. The wake runs AFTER the completion was queued, so the
/// UI thread can already have drained that completion while the count still lags; waiting on the queue for the count to
/// catch up would then hang. This waits on the count itself (a condition wait bounded by `TIMEOUT`).
#[derive(Default)]
struct WakeCounter {
    count: Mutex<usize>,
    changed: Condvar,
}

impl WakeCounter {
    fn wake(&self) {
        *self.count.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        self.changed.notify_all();
    }

    /// Waits until at least `count` wakes happened; false when `TIMEOUT` passes first.
    fn wait_for(&self, count: usize) -> bool {
        let guard = self.count.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, TIMEOUT, |wakes| *wakes < count)
            .unwrap_or_else(PoisonError::into_inner);
        *guard >= count
    }
}

/// One loader over a real `PreviewImageService` with fake decoders, a navigation clock and a wake counter. Every effect any
/// call returned is kept in `effects` (what the presenter would have applied, in order).
struct Fixture {
    root: TempRoot,
    clock: Arc<GenerationClock>,
    decoder: Arc<SizedDecoder>,
    service: Arc<PreviewImageService>,
    loader: ZoomDetailLoader,
    wakes: Arc<WakeCounter>,
    effects: Vec<ZoomDetailEffect>,
}

impl Fixture {
    /// The `ZoomDetailLoaderGapTests` fixture: a 6000 x 4000 source, previews 600 px wide, ordinary decoder only.
    fn new() -> Self {
        Self::build(SizedDecoder::new(), ServiceOptions::default(), None)
    }

    fn build(decoder: Arc<SizedDecoder>, options: ServiceOptions, delay: Option<IndicatorDelay>) -> Self {
        Self::build_over(decoder.clone(), decoder, options, delay)
    }

    /// `decoder` is the one the assertions read; `service_decoder` is the ordinary decoder of the service.
    fn build_over(
        decoder: Arc<SizedDecoder>,
        service_decoder: Arc<dyn ImageDecoder>,
        options: ServiceOptions,
        delay: Option<IndicatorDelay>,
    ) -> Self {
        let root = TempRoot::new("zoom-detail");
        let service = build_service(&root.join("cache"), service_decoder, options);
        let clock = Arc::new(GenerationClock::default());
        let wakes = Arc::new(WakeCounter::default());
        let counter = Arc::clone(&wakes);
        let mut loader = ZoomDetailLoader::new(
            Arc::clone(&service),
            Arc::clone(&clock),
            Arc::new(ThreadRunner),
            Arc::new(move || counter.wake()),
        );
        if let Some(delay) = delay {
            loader = loader.with_indicator_delay(delay);
        }
        Self {
            root,
            clock,
            decoder,
            service,
            loader,
            wakes,
            effects: Vec::new(),
        }
    }

    /// Writes the (tiny) source file `name` once and returns its path.
    fn file(&self, name: &str) -> String {
        let path = self.root.join(name);
        if !path.exists() {
            std::fs::write(&path, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap_or_else(|error| panic!("write {name}: {error}"));
        }
        path.to_string_lossy().into_owned()
    }

    /// A downscaled preview 600 px wide of a 6000 x 4000 original.
    fn default_preview() -> PreviewImage {
        sized_image(600, 400, 6000, 4000, true)
    }

    /// Starts a new navigation and presents `name` as its main image (C# `Present`).
    fn present(&mut self, name: &str) -> (i64, String) {
        self.present_preview(name, &Self::default_preview())
    }

    fn present_preview(&mut self, name: &str, preview: &PreviewImage) -> (i64, String) {
        let path = self.file(name);
        let token = self.clock.next_navigation();
        self.present_with(token, &path, preview);
        (token, path)
    }

    /// Presents `path` for an existing navigation `token`.
    fn present_with(&mut self, token: i64, path: &str, preview: &PreviewImage) {
        let key = self
            .service
            .get_current_cache_key(path)
            .unwrap_or_else(|error| panic!("cache key of {path}: {error:?}"));
        let effects = self.loader.on_preview_presented(token, path, &key, preview);
        self.effects.extend(effects);
    }

    fn set_zoom(&mut self, zoom: Option<f64>) -> usize {
        let effects = self.loader.set_zoom(zoom);
        let count = effects.len();
        self.effects.extend(effects);
        count
    }

    fn refresh(&mut self, zoom: f64) -> ZoomDetailRefreshResult {
        let (result, effects) = self.loader.refresh(zoom);
        self.effects.extend(effects);
        result
    }

    /// Applies the completions that are queued now.
    fn pump(&mut self) {
        let effects = self.loader.pump();
        self.effects.extend(effects);
    }

    /// Waits for ONE worker completion and applies it.
    fn pump_next(&mut self) {
        assert!(
            self.loader.wait_for_event(TIMEOUT),
            "no worker completion arrived within {TIMEOUT:?}"
        );
        self.pump();
    }

    /// Applies completions until `done` holds.
    fn pump_until(&mut self, done: impl Fn(&ZoomDetailLoader) -> bool) {
        while !done(&self.loader) {
            self.pump_next();
        }
    }

    /// Applies completions until the in-flight load (if any) is done (C# `await load`).
    fn finish_load(&mut self) {
        self.pump_until(|loader| !loader.pending_load());
    }

    /// Waits until `count` worker messages were sent in all (decodes and indicator delays), then applies what is queued.
    fn settle(&mut self, count: usize) {
        assert!(
            self.wakes.wait_for(count),
            "fewer than {count} worker messages arrived within {TIMEOUT:?}"
        );
        self.pump();
    }

    /// Opens a gate so the decode of `name` finishes, applies its completion.
    fn run_gated(&mut self, name: &str, start: impl FnOnce(&mut Self)) {
        let gate = self.decoder.gate(name);
        start(self);
        assert!(self.loader.pending_load(), "the decode of {name} did not start");
        gate.open();
        self.finish_load();
    }

    /// Every `ShowImage` effect so far: (platform image, original width, original height).
    fn shown(&self) -> Vec<(&PlatformImage, i32, i32)> {
        self.effects
            .iter()
            .filter_map(|effect| match effect {
                ZoomDetailEffect::ShowImage {
                    image,
                    original_width,
                    original_height,
                } => Some((image, *original_width, *original_height)),
                _ => None,
            })
            .collect()
    }

    fn originals_shown_events(&self) -> Vec<&str> {
        self.effects
            .iter()
            .filter_map(|effect| match effect {
                ZoomDetailEffect::OriginalShown { path } => Some(path.as_str()),
                _ => None,
            })
            .collect()
    }

    fn indicator_changes(&self) -> Vec<bool> {
        self.effects
            .iter()
            .filter_map(|effect| match effect {
                ZoomDetailEffect::RawDecodeIndicatorChanged { visible } => Some(*visible),
                _ => None,
            })
            .collect()
    }

    fn held_platform_image(&self) -> PlatformImage {
        self.loader
            .held_original()
            .map(|held| held.platform_image().clone())
            .unwrap_or_else(|| panic!("the loader holds no original"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // No worker stays blocked on a gate when a test fails half way.
        self.decoder.open_all_gates();
    }
}

fn same_image(a: &PlatformImage, b: &PlatformImage) -> bool {
    same_platform_image(a, b)
}

// ---- ZoomDetailTests ------------------------------------------------------------------------------------------------

// C#: ZoomDetailTests.Fit_NeverDecodesTheOriginal
#[test]
fn fit_never_decodes_the_original() {
    let mut fixture = Fixture::new();
    fixture.present("a.jpg");
    fixture.set_zoom(None);

    assert!(!fixture.loader.pending_load());
    assert_eq!(fixture.decoder.total_original_decodes(), 0);
    assert!(fixture.loader.held_original().is_none());
    assert!(fixture.effects.is_empty());
}

// C#: ZoomDetailTests.LeavingFit_ShowsPreviewAtOriginalSize_ThenSwapsInOriginalWithoutLayoutChange (loader half)
#[test]
fn leaving_fit_keeps_preview_then_shows_decoded_size() {
    let mut fixture = Fixture::new();
    let gate = fixture.decoder.gate("a.jpg");
    let (_, path) = fixture.present("a.jpg");

    let immediate = fixture.set_zoom(Some(1.0));

    // Preview still on screen: no ShowImage before the decode.
    assert_eq!(immediate, 0);
    assert!(fixture.shown().is_empty());
    assert!(fixture.loader.pending_load());
    gate.open();
    fixture.finish_load();

    assert!(fixture.loader.is_showing_original());
    let shown = fixture.shown();
    assert_eq!(shown.len(), 1);
    assert!(same_image(shown[0].0, &fixture.held_platform_image()));
    // The effect carries the DECODED size, not the preview's.
    let held = fixture
        .loader
        .held_original()
        .map(|held| (held.pixel_width(), held.pixel_height()));
    assert_eq!(held, Some((6000, 4000)));
    assert_eq!((shown[0].1, shown[0].2), (6000, 4000));
    assert_eq!(fixture.originals_shown_events(), vec![path.as_str()]);
    assert!(matches!(fixture.effects[0], ZoomDetailEffect::ShowImage { .. }));
    assert!(matches!(fixture.effects[1], ZoomDetailEffect::OriginalShown { .. }));
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 1);
}

// C#: ZoomDetailTests.FailedOriginalDecode_IsNotRetriedOnNextZoomStep
#[test]
fn failed_original_decode_is_not_retried_on_next_zoom_step() {
    let mut fixture = Fixture::new();
    fixture.decoder.fail("a.jpg");
    fixture.present("a.jpg");
    fixture.run_gated("a.jpg", |f| {
        f.set_zoom(Some(1.0));
    });
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 1);

    fixture.set_zoom(Some(2.0));
    fixture.set_zoom(Some(3.0));

    assert!(!fixture.loader.pending_load());
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 1);
    assert!(!fixture.loader.is_showing_original());
}

// C#: ZoomDetailTests.OriginalDecode_IsNotAddedToThePreviewRamCache
#[test]
fn original_decode_is_not_added_to_the_preview_ram_cache() {
    let mut fixture = Fixture::new();
    fixture.present("a.jpg");
    let cached_bytes = fixture.service.cache_bytes();
    let cached_count = fixture.service.cache_count();

    fixture.set_zoom(Some(2.0));
    fixture.finish_load();

    assert!(fixture.loader.is_showing_original());
    assert_eq!(fixture.service.cache_bytes(), cached_bytes);
    assert_eq!(fixture.service.cache_count(), cached_count);
}

// C#: ZoomDetailTests.BackToFit_ShowsPreview_AndZoomingAgainReusesTheHeldOriginal
#[test]
fn back_to_fit_shows_preview_and_zooming_again_reuses_the_held_original() {
    let mut fixture = Fixture::new();
    let preview = Fixture::default_preview();
    fixture.present_preview("a.jpg", &preview);
    fixture.set_zoom(Some(2.0));
    fixture.finish_load();
    let shown_before = fixture.shown().len();

    let back = fixture.loader.set_zoom(None);

    assert!(!fixture.loader.is_showing_original());
    assert!(matches!(
        back.as_slice(),
        [ZoomDetailEffect::ShowImage { image, original_width: 6000, original_height: 4000 }]
            if same_image(image, preview.platform_image())
    ));

    let again = fixture.loader.set_zoom(Some(4.0));

    assert!(!fixture.loader.pending_load());
    assert!(matches!(
        again.as_slice(),
        [ZoomDetailEffect::ShowImage { image, .. }, ZoomDetailEffect::OriginalShown { .. }]
            if same_image(image, &fixture.held_platform_image())
    ));
    assert_eq!(fixture.shown().len(), shown_before);
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 1);
}

// C#: ZoomDetailTests.ZoomBelowPreviewResolution_DoesNotDecodeTheOriginal
#[test]
fn zoom_below_preview_resolution_does_not_decode() {
    let mut fixture = Fixture::new();
    // Preview is 1600 px wide for a 6000 px original: 25 % (1500 px on screen) needs no more pixels.
    fixture.present_preview("a.jpg", &sized_image(1600, 1067, 6000, 4000, true));

    fixture.set_zoom(Some(0.25));

    assert!(!fixture.loader.pending_load());
    assert_eq!(fixture.decoder.total_original_decodes(), 0);
    assert!(fixture.effects.is_empty());
}

// C#: ZoomDetailTests.OriginalLoadingMode_NeverDecodesAgain
#[test]
fn original_loading_mode_never_decodes_again() {
    let options = ServiceOptions {
        original_loading_mode: true,
        ..ServiceOptions::default()
    };
    let mut fixture = Fixture::build(SizedDecoder::new(), options, None);
    // The key of an Original-mode load is an original key: even a (hypothetically) downscaled preview arms nothing.
    fixture.present("a.jpg");

    fixture.set_zoom(Some(2.0));

    assert!(!fixture.loader.pending_load());
    assert_eq!(fixture.decoder.total_original_decodes(), 0);
    assert!(fixture.effects.is_empty());
}

// ---- RAW (ZoomDetailTests) ------------------------------------------------------------------------------------------

fn raw_options(decoder: &Arc<SizedDecoder>, enabled: bool, original_mode: bool) -> ServiceOptions {
    let raw: Arc<dyn ImageDecoder> = decoder.clone();
    ServiceOptions {
        original_loading_mode: original_mode,
        raw_full_decoder: Some(raw),
        raw_full_decode_enabled: enabled,
    }
}

// C#: ZoomDetailTests.RawOnZoom_ShowsDelayedIndicatorAndUsesRawFullDecoder
#[test]
fn raw_on_zoom_shows_delayed_indicator_and_uses_raw_full_decoder() {
    let raw_decoder = SizedDecoder::new();
    let ordinary = SizedDecoder::new();
    let delays = IndicatorGates::new();
    let options = raw_options(&raw_decoder, true, false);
    let mut fixture = Fixture::build_over(raw_decoder.clone(), ordinary.clone(), options, Some(delays.delay()));
    let gate = raw_decoder.gate("zoom.cr2");
    fixture.present("zoom.cr2");

    fixture.set_zoom(Some(1.0));

    assert!(fixture.loader.pending_load());
    assert!(delays.wait_calls(1));
    assert!(!fixture.loader.is_raw_decode_indicator_visible());
    delays.open(0); // the 300 ms have "elapsed"
    fixture.pump_until(|loader| loader.is_raw_decode_indicator_visible());
    assert_eq!(fixture.indicator_changes(), vec![true]);

    gate.open();
    fixture.finish_load();

    assert!(!fixture.loader.is_raw_decode_indicator_visible());
    assert_eq!(fixture.indicator_changes(), vec![true, false]);
    assert_eq!(raw_decoder.original_decodes("zoom.cr2"), 1);
    assert_eq!(ordinary.total_original_decodes(), 0);
    assert!(fixture.loader.is_showing_original());
}

/// The RAW original-mode fixtures: the service is in Original mode, the shown image is the embedded JPEG of a 6000 x 4000 sensor.
fn raw_original_mode(enabled: bool, downscaled: bool) -> (Fixture, Arc<SizedDecoder>, String) {
    let raw_decoder = SizedDecoder::new();
    let options = raw_options(&raw_decoder, enabled, true);
    let embedded: Arc<dyn ImageDecoder> = Arc::new(EmbeddedRawDecoder { downscaled });
    let mut fixture = Fixture::build_over(
        raw_decoder.clone(),
        embedded,
        options,
        Some(IndicatorGates::new().delay()),
    );
    let preview = if downscaled {
        sized_image(2000, 1333, 6000, 4000, true)
    } else {
        sized_image(6000, 4000, 6000, 4000, false)
    };
    let (_, path) = fixture.present_preview("original-mode.cr2", &preview);
    (fixture, raw_decoder, path)
}

// C#: ZoomDetailTests.RawOriginalMode_EmbeddedJpegBelowSensorSize_OffersTheFullDecodeOnZoom
#[test]
fn raw_original_mode_embedded_jpeg_below_sensor_size_offers_the_full_decode_on_zoom() {
    let (mut fixture, raw_decoder, _) = raw_original_mode(true, true);

    fixture.set_zoom(Some(1.0));
    fixture.finish_load();

    assert_eq!(raw_decoder.original_decodes("original-mode.cr2"), 1);
    assert_eq!(
        fixture.loader.held_original().map(|held| held.pixel_width()),
        Some(6000)
    );
    assert!(fixture.loader.is_showing_original());
}

// C#: ZoomDetailTests.RawOriginalMode_FullDecodeDisabled_NeverDecodesAgain
#[test]
fn raw_original_mode_full_decode_disabled_never_decodes_again() {
    let (mut fixture, raw_decoder, _) = raw_original_mode(false, true);

    fixture.set_zoom(Some(1.0));

    assert!(!fixture.loader.pending_load());
    assert_eq!(raw_decoder.total_original_decodes(), 0);
    assert!(fixture.effects.is_empty());
}

// C#: ZoomDetailTests.RawOriginalMode_ImageAlreadyAtSensorSize_NeverDecodesAgain
#[test]
fn raw_original_mode_image_already_at_sensor_size_never_decodes_again() {
    let (mut fixture, raw_decoder, _) = raw_original_mode(true, false);

    fixture.set_zoom(Some(1.0));

    assert!(!fixture.loader.pending_load());
    assert_eq!(raw_decoder.total_original_decodes(), 0);
}

// New test: no C# twin, derived from ZoomDetailLoader.OnPreviewPresented line 142 (a RAW path arms nothing in ANY loading
// mode when the RAW full decoder is not in use, even with a standard-mode key).
#[test]
fn raw_preview_with_the_full_decoder_off_arms_nothing_in_preview_mode() {
    let raw_decoder = SizedDecoder::new();
    let options = raw_options(&raw_decoder, false, false);
    let mut fixture = Fixture::build_over(raw_decoder.clone(), SizedDecoder::new(), options, None);
    fixture.present("plain.cr2");

    fixture.set_zoom(Some(1.0));

    assert!(!fixture.loader.pending_load());
    assert_eq!(raw_decoder.total_original_decodes(), 0);
    assert_eq!(fixture.refresh(1.0), ZoomDetailRefreshResult::AlreadyFullResolution);
}

// C#: ZoomDetailTests.RawOnZoom_SupersededLoadFinishingLate_DoesNotClearTheNewerLoadsIndicator
#[test]
fn raw_on_zoom_superseded_load_finishing_late_does_not_clear_the_newer_loads_indicator() {
    let raw_decoder = SizedDecoder::new();
    let gate_a = raw_decoder.gate("a.cr2");
    let gate_b = raw_decoder.gate("b.cr2");
    let delays = IndicatorGates::new();
    let options = raw_options(&raw_decoder, true, false);
    let mut fixture = Fixture::build_over(raw_decoder.clone(), SizedDecoder::new(), options, Some(delays.delay()));
    fixture.set_zoom(Some(1.0));
    fixture.present("a.cr2");
    assert!(fixture.loader.pending_load());
    assert!(
        raw_decoder.wait_started("a.cr2"),
        "A must be inside the decoder before B supersedes it"
    );
    // The delays run on unordered worker threads: B may only start once A's delay is registered, so that call 0 is A's
    // and call 1 is B's.
    assert!(
        delays.wait_calls(1),
        "A's indicator delay must have started before B supersedes it"
    );

    fixture.present("b.cr2"); // supersedes A while its RAW decode is still running

    assert!(fixture.loader.pending_load());
    assert!(delays.wait_calls(2));
    assert!(delays.is_cancelled(0), "A's indicator was stopped when B superseded it");
    assert_eq!(delays.wait_returned(0), Some(false));
    delays.open(1); // B's indicator delay elapses while A's decode is still blocked on its gate
    fixture.pump_until(|loader| loader.is_raw_decode_indicator_visible());

    gate_a.open(); // the superseded load finishes late
    fixture.pump_next();

    assert!(fixture.loader.is_raw_decode_indicator_visible());
    assert!(fixture.loader.pending_load(), "A's completion must not clear B's slot");

    gate_b.open();
    fixture.finish_load();
    assert!(!fixture.loader.is_raw_decode_indicator_visible());
    assert_eq!(fixture.indicator_changes(), vec![true, false]);
}

// W8-07g mutation survivor (`zoom_detail.rs` `on_indicator_elapsed`, `&&` -> `||`): a stale "delay elapsed" event of a
// superseded indicator must not show the indicator of the newer, still running slot. Rust-only guard; the C# loader
// compares `ReferenceEquals(_indicatorCts, cts)` in `ShowIfCurrent`.
#[test]
fn raw_on_zoom_stale_indicator_elapsed_event_of_a_superseded_slot_does_not_show_the_newer_indicator() {
    let raw_decoder = SizedDecoder::new();
    let _gate_a = raw_decoder.gate("a.cr2");
    let _gate_b = raw_decoder.gate("b.cr2");
    let delays = IndicatorGates::new();
    let options = raw_options(&raw_decoder, true, false);
    let mut fixture = Fixture::build_over(raw_decoder.clone(), SizedDecoder::new(), options, Some(delays.delay()));
    fixture.set_zoom(Some(1.0));
    fixture.present("a.cr2");
    assert!(delays.wait_calls(1), "A's indicator delay must have started");
    delays.open(0); // A's delay elapses and queues its event; nothing is pumped yet
    assert!(
        fixture.loader.wait_for_event(TIMEOUT),
        "A's elapsed event must be queued"
    );

    fixture.present("b.cr2"); // B's slot replaces A's before A's event is applied
    assert!(delays.wait_calls(2));
    fixture.pump(); // applies only A's stale event (both decodes are gated)

    assert!(!fixture.loader.is_raw_decode_indicator_visible());
    assert!(fixture.indicator_changes().is_empty());
}

// ---- navigation -----------------------------------------------------------------------------------------------------

// C#: ZoomDetailTests.NavigatingAway_IgnoresTheInFlightOriginal_AndFetchesTheNextImagesOriginal
#[test]
fn navigating_away_ignores_the_in_flight_original_and_fetches_the_next_images_original() {
    let mut fixture = Fixture::new();
    let gate_a = fixture.decoder.gate("a.jpg");
    let gate_b = fixture.decoder.gate("b.jpg");
    fixture.set_zoom(Some(2.0));
    fixture.present("a.jpg");
    assert!(fixture.loader.pending_load());
    assert!(
        fixture.decoder.wait_started("a.jpg"),
        "A must be inside the decoder before B supersedes it"
    );

    fixture.present("b.jpg");
    assert!(fixture.loader.pending_load());
    gate_a.open(); // a's original finishes after the navigation
    fixture.pump_next();

    assert!(fixture.shown().is_empty());
    assert!(fixture.loader.held_original().is_none());
    assert!(fixture.loader.pending_load(), "b's load is still running");

    gate_b.open();
    fixture.finish_load();

    assert!(fixture.loader.is_showing_original());
    assert_eq!(fixture.shown().len(), 1);
    assert!(same_image(fixture.shown()[0].0, &fixture.held_platform_image()));
    assert_eq!(fixture.decoder.total_original_decodes(), 2);
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 1);
    assert_eq!(fixture.decoder.original_decodes("b.jpg"), 1);
}

// C#: ZoomDetailTests.Navigation_ReleasesTheOriginal_ForGarbageCollection
#[test]
fn navigation_releases_the_original_for_garbage_collection() {
    let mut fixture = Fixture::new();
    fixture.present("a.jpg");
    fixture.set_zoom(Some(1.0));
    fixture.finish_load();
    assert!(fixture.loader.is_showing_original());
    let weak = match fixture.loader.held_original() {
        Some(held) => Arc::downgrade(held),
        None => panic!("the loader holds no original"),
    };
    fixture.set_zoom(None);

    fixture.present("b.jpg"); // b stays in Fit: nothing new decoded

    assert!(!fixture.loader.pending_load());
    assert!(fixture.loader.held_original().is_none());
    assert!(
        weak.upgrade().is_none(),
        "The previous image's full-resolution decode is still reachable after navigation."
    );
}

// C#: ZoomDetailTests.SupersededOriginalFinishingLate_DoesNotChangeTheCurrentImagesSize
#[test]
fn superseded_original_finishing_late_does_not_change_the_current_images_size() {
    let decoder = SizedDecoder::new();
    decoder.set_decoded(6032, 4032);
    let mut fixture = Fixture::build(decoder, ServiceOptions::default(), None);
    let gate_a = fixture.decoder.gate("a.jpg");
    fixture.present("a.jpg");
    fixture.set_zoom(Some(1.0));
    assert!(fixture.loader.pending_load());
    assert!(
        fixture.decoder.wait_started("a.jpg"),
        "A must be inside the decoder before the navigation"
    );

    fixture.set_zoom(None); // back to Fit, then on to the next image while a's decode is still running
    fixture.present("b.jpg");
    let effects_before = fixture.effects.len();
    gate_a.open();
    fixture.pump_next();

    assert_eq!(fixture.effects.len(), effects_before);
    assert!(fixture.shown().is_empty());
    assert!(fixture.loader.held_original().is_none());
    assert!(!fixture.loader.is_showing_original());
}

// ---- sizes ----------------------------------------------------------------------------------------------------------

/// Preview of a 6720 x 4480 original (600 px wide).
fn preview_6720() -> PreviewImage {
    sized_image(600, 400, 6720, 4480, true)
}

fn check_original_swap_shows_the_decoded_size(decoded_width: i32, decoded_height: i32, zoom: f64) {
    let decoder = SizedDecoder::new();
    decoder.set_original(6720, 4480);
    decoder.set_decoded(decoded_width, decoded_height);
    let mut fixture = Fixture::build(decoder, ServiceOptions::default(), None);
    let gate = fixture.decoder.gate("a.jpg");
    fixture.present_preview("a.jpg", &preview_6720());

    fixture.set_zoom(Some(zoom));
    assert!(
        fixture.shown().is_empty(),
        "the preview stays on screen until the decode is done"
    );
    gate.open();
    fixture.finish_load();

    let shown = fixture.shown();
    assert_eq!(shown.len(), 1);
    assert_eq!((shown[0].1, shown[0].2), (decoded_width, decoded_height));
    let held = fixture
        .loader
        .held_original()
        .map(|held| (held.pixel_width(), held.pixel_height()));
    assert_eq!(held, Some((decoded_width, decoded_height)));
}

// C#: ZoomDetailTests.OriginalSwap_ShowsDecodedSizeTimesZoomOverDpi_NotThePreviewsSize (loader half; theory row 6744 x 4502)
#[test]
fn original_swap_shows_decoded_size_not_the_previews_size_6744_by_4502() {
    check_original_swap_shows_the_decoded_size(6744, 4502, 1.0);
    check_original_swap_shows_the_decoded_size(6744, 4502, 2.0);
}

// C#: ZoomDetailTests.OriginalSwap_ShowsDecodedSizeTimesZoomOverDpi_NotThePreviewsSize (loader half; theory row 6719 x 4479)
#[test]
fn original_swap_shows_decoded_size_not_the_previews_size_6719_by_4479() {
    check_original_swap_shows_the_decoded_size(6719, 4479, 1.0);
    check_original_swap_shows_the_decoded_size(6719, 4479, 3.0);
}

// C#: ZoomDetailTests.BackToFit_RevertsToThePreviewsOriginalSize_AndZoomingAgainReusesTheDecodedSize (effect sizes only)
#[test]
fn back_to_fit_reverts_to_the_previews_original_size_and_zooming_again_reuses_the_decoded_size() {
    let decoder = SizedDecoder::new();
    decoder.set_original(6720, 4480);
    decoder.set_decoded(6744, 4502);
    let mut fixture = Fixture::build(decoder, ServiceOptions::default(), None);
    let preview = preview_6720();
    fixture.present_preview("a.jpg", &preview);
    fixture.set_zoom(Some(1.0));
    fixture.finish_load();
    assert_eq!(
        fixture.shown().last().map(|shown| (shown.1, shown.2)),
        Some((6744, 4502))
    );

    fixture.set_zoom(None);

    let shown = fixture.shown();
    let last = shown.last().map(|shown| (shown.0, shown.1, shown.2));
    assert!(matches!(last, Some((image, 6720, 4480)) if same_image(image, preview.platform_image())));

    fixture.set_zoom(Some(1.0));

    assert!(fixture.loader.is_showing_original());
    assert_eq!(
        fixture.shown().last().map(|shown| (shown.1, shown.2)),
        Some((6744, 4502))
    );
    assert_eq!(
        fixture.decoder.original_decodes("a.jpg"),
        1,
        "held original re-shown, no second decode"
    );
}

// C#: ZoomDetailTests.RawFullDecode_NeverChangesThePreviewsRecordedDimensions
#[test]
fn raw_full_decode_never_changes_the_previews_recorded_dimensions() {
    let raw_decoder = SizedDecoder::new();
    raw_decoder.set_original(6720, 4480);
    raw_decoder.set_decoded(6744, 4502);
    let ordinary = SizedDecoder::new();
    ordinary.set_original(6720, 4480);
    let options = raw_options(&raw_decoder, true, false);
    let mut fixture = Fixture::build_over(
        raw_decoder.clone(),
        ordinary,
        options,
        Some(IndicatorGates::new().delay()),
    );
    let preview = preview_6720();
    let (_, path) = fixture.present_preview("dims.cr2", &preview);
    let key = fixture
        .service
        .get_current_cache_key(&path)
        .unwrap_or_else(|error| panic!("cache key: {error:?}"));
    // The preview decode seeds the known size of the file (C#: the service did it while decoding the preview).
    assert_eq!(
        fixture.service.get_original_dimensions(&path, &key).ok(),
        Some((6720, 4480))
    );

    fixture.set_zoom(Some(1.0));
    fixture.finish_load();

    assert_eq!(
        fixture.shown().last().map(|shown| (shown.1, shown.2)),
        Some((6744, 4502))
    );
    assert_eq!(
        fixture.service.try_get_known_original_dimensions(&key),
        Some((6720, 4480))
    );
    assert_eq!(
        fixture.service.get_original_dimensions(&path, &key).ok(),
        Some((6720, 4480))
    );
}

// ---- ZoomDetailLoaderGapTests ---------------------------------------------------------------------------------------

// C#: ZoomDetailLoaderGapTests.Reset_DuringInFlightDecodeThenNewTarget_OldLoadFinishingLateDoesNotClearTheNewLoadsSlot
#[test]
fn reset_during_in_flight_decode_then_new_target_old_load_finishing_late_does_not_clear_the_new_loads_slot() {
    let mut fixture = Fixture::new();
    let gate_a = fixture.decoder.gate("a.jpg");
    let gate_b = fixture.decoder.gate("b.jpg");
    fixture.set_zoom(Some(1.0));
    fixture.present("a.jpg");
    assert!(fixture.loader.pending_load());
    assert!(
        fixture.decoder.wait_started("a.jpg"),
        "A must be inside the decoder before B supersedes it"
    );

    fixture.present("b.jpg"); // reset + a new target while A's decode is still running; the zoom is still 1.0, so B starts at once
    assert!(fixture.loader.pending_load());

    gate_a.open(); // the superseded load ends now, before B has finished
    fixture.pump_next();

    assert!(
        fixture.loader.pending_load(),
        "A's completion must not have dropped B's slot"
    );
    assert_eq!(fixture.set_zoom(Some(2.0)), 0); // must not start a second decode of B (its slot is still owned)
    assert!(fixture.loader.pending_load());

    gate_b.open();
    fixture.finish_load();
    assert!(fixture.loader.is_showing_original());
    assert!(!fixture.loader.pending_load());
    assert_eq!(fixture.decoder.original_decodes("b.jpg"), 1);
}

// C#: ZoomDetailLoaderGapTests.FailedDecode_BlocksRetriesOnTheSameNavigationButTheNextNavigationDecodesAgain
#[test]
fn failed_decode_blocks_retries_on_the_same_navigation_but_the_next_navigation_decodes_again() {
    let mut fixture = Fixture::new();
    fixture.decoder.fail("a.jpg");
    fixture.set_zoom(Some(1.0));
    fixture.run_gated("a.jpg", |f| {
        f.present("a.jpg");
    });
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 1);

    fixture.set_zoom(Some(2.0));
    fixture.set_zoom(Some(3.0));
    assert!(!fixture.loader.pending_load());
    assert_eq!(
        fixture.decoder.original_decodes("a.jpg"),
        1,
        "no retry on this navigation"
    );

    fixture.present("c.jpg"); // next navigation (new token): the failure latch no longer applies
    assert!(fixture.loader.pending_load());
    fixture.finish_load();
    assert_eq!(fixture.decoder.original_decodes("c.jpg"), 1);
    assert!(fixture.loader.is_showing_original());
}

// ---- ZoomDetailLoaderGapTests.MutationGaps --------------------------------------------------------------------------

// C#: ZoomDetailLoaderGapTests.NavigationDuringInFlightDecode_WithoutAReset_DropsTheOriginalWhenItFinishesLate
#[test]
fn navigation_during_in_flight_decode_without_a_reset_drops_the_original_when_it_finishes_late() {
    let mut fixture = Fixture::new();
    let gate = fixture.decoder.gate("a.jpg");
    fixture.set_zoom(Some(1.0));
    fixture.present("a.jpg");
    assert!(fixture.loader.pending_load());

    fixture.clock.next_navigation(); // the user navigated on; the loader's target is still the old one
    gate.open();
    fixture.finish_load();

    assert!(!fixture.loader.is_showing_original());
    assert!(fixture.loader.held_original().is_none());
    assert!(fixture.shown().is_empty());
}

// C#: ZoomDetailLoaderGapTests.CancelledDecode_IsNotAFailureAndDoesNotRetryOrShowAnything
#[test]
fn cancelled_decode_is_not_a_failure_and_does_not_retry_or_show_anything() {
    let mut fixture = Fixture::new();
    fixture.decoder.cancel("a.jpg");
    fixture.set_zoom(Some(1.0));
    fixture.run_gated("a.jpg", |f| {
        f.present("a.jpg");
    });

    assert_eq!(
        fixture.decoder.original_decodes("a.jpg"),
        1,
        "observed as a cancel: no update that starts the load again"
    );
    assert!(!fixture.loader.pending_load());
    assert!(!fixture.loader.is_showing_original());
    assert!(fixture.effects.is_empty());
    // A cancel sets no failure latch: a Refresh reports a first load, not a retry.
    assert_eq!(fixture.refresh(2.0), ZoomDetailRefreshResult::LoadingOriginal);
    fixture.finish_load();
}

// C#: ZoomDetailLoaderGapTests.Reset_CancelsAQueuedDecodeSoItNeverStartsOnceTheSlotFrees
#[test]
fn reset_cancels_a_queued_decode_so_it_never_starts_once_the_slot_frees() {
    // The original decodes run one at a time: a.jpg holds the slot (blocked in the decoder), b.jpg's load waits for it.
    let mut fixture = Fixture::new();
    let gate_a = fixture.decoder.gate("a.jpg");
    let gate_c = fixture.decoder.gate("c.jpg");
    fixture.set_zoom(Some(1.0));
    fixture.present("a.jpg");
    assert!(fixture.decoder.wait_started("a.jpg"), "A must be inside the decoder");
    fixture.present("b.jpg"); // reset cancels A's token (A is already inside the decoder), then B queues behind A
    assert!(fixture.loader.pending_load());
    fixture.present("c.jpg"); // reset cancels B's token while B is still queued
    assert!(fixture.loader.pending_load());

    gate_a.open(); // the slot frees: a cancelled B must be dropped before it reaches the decoder
    gate_c.open();
    fixture.pump_until(|loader| loader.is_showing_original());
    fixture.settle(3); // A's, B's and C's completions were all sent and applied

    assert_eq!(fixture.decoder.original_decodes("b.jpg"), 0);
    assert_eq!(fixture.decoder.original_decodes("c.jpg"), 1);
    assert!(fixture.loader.is_showing_original());
    assert!(!fixture.loader.pending_load());
}

// C#: ZoomDetailLoaderGapTests.ASupersededTarget_OnTheSameNavigation_DropsItsOriginalWhenItFinishesLate
#[test]
fn a_superseded_target_on_the_same_navigation_drops_its_original_when_it_finishes_late() {
    let mut fixture = Fixture::new();
    let gate_a = fixture.decoder.gate("a.jpg");
    let gate_b = fixture.decoder.gate("b.jpg");
    fixture.set_zoom(Some(1.0));
    let token = fixture.clock.next_navigation();
    let path_a = fixture.file("a.jpg");
    let path_b = fixture.file("b.jpg");
    let preview = Fixture::default_preview();
    fixture.present_with(token, &path_a, &preview);
    assert!(fixture.loader.pending_load());
    assert!(
        fixture.decoder.wait_started("a.jpg"),
        "A must be inside the decoder before B supersedes it"
    );
    // A new target for the SAME navigation token: the navigation is still current.
    fixture.present_with(token, &path_b, &preview);
    assert!(fixture.loader.pending_load());

    gate_a.open();
    fixture.pump_next();

    assert!(
        !fixture.loader.is_showing_original(),
        "a.jpg's pixels must never be shown for b.jpg"
    );
    assert!(fixture.shown().is_empty());
    assert!(fixture.loader.held_original().is_none());
    gate_b.open();
    fixture.finish_load();
    assert!(fixture.loader.is_showing_original());
}

// C#: ZoomDetailLoaderGapTests.WhenShowingTheOriginalThrows_TheFailureIsLoggedAndTheLoadStillCompletes
// (Rust has no exception across an effect: the caller ignoring the ShowImage effect is the closest case)
#[test]
fn completion_keeps_original_held_and_clears_the_load_slot_when_the_show_effect_is_not_applied() {
    let mut fixture = Fixture::new();
    let gate = fixture.decoder.gate("a.jpg");
    fixture.set_zoom(Some(1.0));
    fixture.present("a.jpg");
    gate.open();
    assert!(fixture.loader.wait_for_event(TIMEOUT));

    let dropped = fixture.loader.pump(); // the caller does not apply the effects

    assert!(matches!(dropped.first(), Some(ZoomDetailEffect::ShowImage { .. })));
    assert!(fixture.loader.held_original().is_some());
    assert!(!fixture.loader.pending_load());
    assert_eq!(fixture.set_zoom(Some(2.0)), 0, "the finished load is not started again");
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 1);
}

// ---- ZoomDetailLoaderGapTests.Refresh -------------------------------------------------------------------------------

// C#: ZoomDetailLoaderGapTests.Refresh_AtFit_PreviewAlreadyLargeEnough_LoadsNothing
#[test]
fn refresh_at_fit_preview_already_large_enough_loads_nothing() {
    let mut fixture = Fixture::new();
    fixture.set_zoom(None);
    fixture.present("a.jpg");

    let result = fixture.refresh(0.1); // 600 px on screen = the preview's 600

    assert_eq!(result, ZoomDetailRefreshResult::PreviewSufficient);
    assert!(!fixture.loader.pending_load());
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 0);
    assert!(fixture.effects.is_empty());
}

// C#: ZoomDetailLoaderGapTests.Refresh_AtFit_ScreenShowsMorePixelsThanThePreview_DecodesAndKeepsTheOriginalAtFit
#[test]
fn refresh_at_fit_screen_shows_more_pixels_than_the_preview_decodes_and_keeps_the_original_at_fit() {
    let mut fixture = Fixture::new();
    let gate = fixture.decoder.gate("a.jpg");
    fixture.set_zoom(None);
    fixture.present("a.jpg");

    let result = fixture.refresh(0.2); // 1200 px on screen > 600

    assert_eq!(result, ZoomDetailRefreshResult::LoadingOriginal);
    assert!(fixture.loader.pending_load());
    gate.open();
    fixture.finish_load();
    assert!(fixture.loader.is_showing_original());
    let shown = fixture.shown();
    assert!(matches!(shown.last(), Some((image, ..)) if same_image(image, &fixture.held_platform_image())));

    assert_eq!(fixture.set_zoom(None), 0); // a repeated Fit (no zoom change) keeps the refreshed original
    assert!(fixture.loader.is_showing_original());
    assert_eq!(fixture.shown().len(), 1);
}

// C#: ZoomDetailLoaderGapTests.Refresh_HoldEndsWhenTheZoomChanges_FitThenShowsThePreviewAgain
#[test]
fn refresh_hold_ends_when_the_zoom_changes_fit_then_shows_the_preview_again() {
    let mut fixture = Fixture::new();
    fixture.set_zoom(None);
    let preview = Fixture::default_preview();
    fixture.present_preview("a.jpg", &preview);
    fixture.run_gated("a.jpg", |f| {
        assert_eq!(f.refresh(0.2), ZoomDetailRefreshResult::LoadingOriginal);
    });
    assert!(fixture.loader.is_showing_original());

    fixture.set_zoom(Some(1.0));
    fixture.set_zoom(None);

    assert!(!fixture.loader.is_showing_original()); // normal Fit behaviour again: the preview
    let shown = fixture.shown();
    assert!(matches!(shown.last(), Some((image, 6000, 4000)) if same_image(image, preview.platform_image())));
}

// C#: ZoomDetailLoaderGapTests.Refresh_AtFit_WithAHeldOriginal_ShowsItAtOnceWithoutDecoding
#[test]
fn refresh_at_fit_with_a_held_original_shows_it_at_once_without_decoding() {
    let mut fixture = Fixture::new();
    fixture.set_zoom(Some(1.0));
    fixture.run_gated("a.jpg", |f| {
        f.present("a.jpg");
    });
    fixture.set_zoom(None); // back to Fit: the preview again, the original stays held
    assert!(!fixture.loader.is_showing_original());
    let shown_before = fixture.shown().len();

    let result = fixture.refresh(0.2);

    assert_eq!(result, ZoomDetailRefreshResult::ShowedOriginal);
    assert!(fixture.loader.is_showing_original());
    assert_eq!(fixture.shown().len(), shown_before + 1);
    assert!(!fixture.loader.pending_load());
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 1);
}

// C#: ZoomDetailLoaderGapTests.Refresh_AfterAFailedDecode_RetriesIt
#[test]
fn refresh_after_a_failed_decode_retries_it() {
    let mut fixture = Fixture::new();
    fixture.decoder.fail("a.jpg");
    fixture.set_zoom(Some(1.0));
    fixture.run_gated("a.jpg", |f| {
        f.present("a.jpg");
    });
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 1);
    fixture.set_zoom(Some(2.0));
    assert_eq!(
        fixture.decoder.original_decodes("a.jpg"),
        1,
        "the failure latch blocks a zoom-triggered retry"
    );

    fixture.decoder.stop_failing("a.jpg");
    let mut result = None;
    fixture.run_gated("a.jpg", |f| result = Some(f.refresh(2.0)));

    assert_eq!(result, Some(ZoomDetailRefreshResult::RetryingOriginal));
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 2);
    assert!(fixture.loader.is_showing_original());
}

// C#: ZoomDetailLoaderGapTests.Refresh_WhileTheOriginalIsShown_DoesNothing
#[test]
fn refresh_while_the_original_is_shown_does_nothing() {
    let mut fixture = Fixture::new();
    fixture.set_zoom(Some(1.0));
    fixture.run_gated("a.jpg", |f| {
        f.present("a.jpg");
    });
    let shown_before = fixture.effects.len();

    assert_eq!(fixture.refresh(1.0), ZoomDetailRefreshResult::AlreadyFullResolution);
    assert_eq!(fixture.effects.len(), shown_before);
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 1);
}

// C#: ZoomDetailLoaderGapTests.Refresh_WithoutATarget_ReportsFullResolution
#[test]
fn refresh_without_a_target_reports_full_resolution() {
    let mut fixture = Fixture::new();

    assert_eq!(fixture.refresh(1.0), ZoomDetailRefreshResult::AlreadyFullResolution);
    assert!(fixture.effects.is_empty());
}

// C#: ZoomDetailLoaderGapTests.Refresh_ZoomedPreviewLargeEnough_LoadsNothing
#[test]
fn refresh_zoomed_preview_large_enough_loads_nothing() {
    let mut fixture = Fixture::new();
    fixture.set_zoom(Some(0.05)); // 300 px on screen: no zoom-triggered decode either
    fixture.present("a.jpg");

    assert_eq!(fixture.refresh(0.05), ZoomDetailRefreshResult::PreviewSufficient);
    assert_eq!(fixture.decoder.original_decodes("a.jpg"), 0);
}

// C#: ZoomDetailLoaderGapTests.Refresh_HoldDoesNotSurviveNavigation
#[test]
fn refresh_hold_does_not_survive_navigation() {
    let mut fixture = Fixture::new();
    fixture.set_zoom(None);
    fixture.present("a.jpg");
    fixture.run_gated("a.jpg", |f| {
        f.refresh(0.2);
    });

    fixture.present("b.jpg"); // the next image at Fit: back to the preview-only behaviour

    assert!(!fixture.loader.pending_load());
    assert_eq!(fixture.decoder.original_decodes("b.jpg"), 0);
    assert!(!fixture.loader.is_showing_original());
    assert!(fixture.loader.held_original().is_none());
}

// New test: no C# twin, derived from Update line 183 (`!_refreshHold &&`: a Refresh hold skips the "preview is enough for the
// stored zoom" check, because Refresh measured the opposite at its own effective zoom).
#[test]
fn refresh_hold_decodes_even_when_the_stored_zoom_needs_no_more_pixels() {
    let mut fixture = Fixture::new();
    fixture.set_zoom(Some(0.05)); // 300 px on screen: the zoom alone would not decode
    fixture.present("a.jpg");
    assert!(!fixture.loader.pending_load());

    let result = fixture.refresh(0.2); // 1200 px on screen > the preview's 600

    assert_eq!(result, ZoomDetailRefreshResult::LoadingOriginal);
    assert!(fixture.loader.pending_load());
    fixture.finish_load();
    assert!(fixture.loader.is_showing_original());
}

// New test: no C# twin, derived from Refresh lines 117-118 (the failure latch is cleared BEFORE the early returns).
#[test]
fn refresh_clears_the_failed_latch_before_an_early_return() {
    let mut fixture = Fixture::new();
    fixture.decoder.fail("a.jpg");
    fixture.set_zoom(Some(1.0));
    fixture.run_gated("a.jpg", |f| {
        f.present("a.jpg");
    });
    fixture.decoder.stop_failing("a.jpg");
    fixture.set_zoom(Some(2.0));
    assert!(!fixture.loader.pending_load(), "latched");

    // 0.05 * 6000 = 300 px <= 600: PreviewSufficient, an early return that still clears the latch.
    assert_eq!(fixture.refresh(0.05), ZoomDetailRefreshResult::PreviewSufficient);
    fixture.set_zoom(Some(3.0));

    assert!(
        fixture.loader.pending_load(),
        "the latch was cleared, the zoom step decodes again"
    );
    fixture.finish_load();
    assert!(fixture.loader.is_showing_original());
}

// New test: no C# twin, derived from Refresh line 120 (`!(effectiveZoom > 0)` is true for NaN and non-positive zooms).
#[test]
fn refresh_with_nan_or_non_positive_zoom_reports_preview_sufficient() {
    let mut fixture = Fixture::new();
    fixture.set_zoom(None);
    fixture.present("a.jpg");

    for zoom in [f64::NAN, 0.0, -0.0, -1.0, f64::NEG_INFINITY] {
        assert_eq!(
            fixture.refresh(zoom),
            ZoomDetailRefreshResult::PreviewSufficient,
            "zoom {zoom}"
        );
    }
    assert!(!fixture.loader.pending_load());
    assert_eq!(fixture.decoder.total_original_decodes(), 0);
    assert!(fixture.effects.is_empty());
}

// ---- seams ----------------------------------------------------------------------------------------------------------

// New test: the shared seam (the stub implements CompletionInbox fully; the tests pin it).
#[test]
fn completion_inbox_delivers_in_arrival_order_and_calls_wake_once_per_send() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let inbox = CompletionInbox::new(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    let sender = inbox.clone();
    assert!(inbox.is_empty());
    assert!(!inbox.wait_ready(Duration::ZERO));

    sender.send(1);
    sender.send(2);
    sender.send(3);

    assert_eq!(wakes.load(Ordering::SeqCst), 3);
    assert_eq!(inbox.len(), 3);
    assert!(inbox.wait_ready(TIMEOUT));
    assert_eq!(inbox.drain(), vec![1, 2, 3]);
    assert!(inbox.is_empty());
    assert_eq!(wakes.load(Ordering::SeqCst), 3);
}

// New test: C# `ReferenceEquals(a.PlatformImage, b.PlatformImage)`.
#[test]
fn same_platform_image_is_pointer_identity_not_content_equality() {
    let make =
        || PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).unwrap_or_else(|error| panic!("pixel buffer: {error:?}"));
    let first = PlatformImage::Pixels(Arc::new(make()));
    let second = PlatformImage::Pixels(Arc::new(make()));
    assert!(same_platform_image(&first, &first.clone()));
    assert!(
        !same_platform_image(&first, &second),
        "equal contents are not the same image"
    );

    let opaque_a = PlatformImage::Opaque(Arc::new(7_u8));
    let opaque_b = PlatformImage::Opaque(Arc::new(7_u8));
    assert!(same_platform_image(&opaque_a, &opaque_a.clone()));
    assert!(!same_platform_image(&opaque_a, &opaque_b));
    assert!(!same_platform_image(&first, &opaque_a));
    assert!(!same_platform_image(&opaque_a, &first));
}

// New test: the production runner starts a named worker thread.
#[test]
fn thread_runner_runs_the_job_on_a_named_worker_thread() {
    use pr_app::coordinators::zoom_detail::BackgroundRunner;
    let (sender, receiver) = mpsc::channel();
    ThreadRunner.run(
        "zoom-detail-test",
        Box::new(move || {
            let current = std::thread::current();
            let _ = sender.send((current.name().map(str::to_owned), current.id()));
        }),
    );

    let received = receiver.recv_timeout(TIMEOUT);
    let Ok((name, id)) = received else {
        panic!("the job did not run within {TIMEOUT:?}");
    };
    assert_eq!(name.as_deref(), Some("zoom-detail-test"));
    assert_ne!(id, std::thread::current().id());
}

// New test: the production indicator delay honours its token (no wall-clock wait in the test).
#[test]
fn default_indicator_delay_reports_cancel_and_elapsed() {
    let delay = default_indicator_delay();
    let cancelled = CancelToken::new();
    cancelled.cancel();
    assert!(
        !delay(Duration::from_secs(3600), &cancelled),
        "a cancelled token ends the wait at once"
    );
    assert!(delay(Duration::ZERO, &CancelToken::new()), "a zero delay has elapsed");
}

// New test (FIX-PANIC-REST M3): `catch_job` hands a worker's panic to the job's own failure result and leaves the normal path alone.
#[test]
fn catch_job_turns_a_panic_into_the_failure_result_and_keeps_the_normal_result() {
    use pr_app::coordinators::zoom_detail::catch_job;

    assert_eq!(catch_job(|| 7, |_| -1), 7);
    let failed: Result<i32, String> = catch_job(|| panic!("job blew up"), Err);
    assert_eq!(failed, Err("job blew up".to_owned()));
    let formatted: Result<i32, String> = catch_job(|| panic!("code {}", 5), Err);
    assert_eq!(formatted, Err("code 5".to_owned()));
}
