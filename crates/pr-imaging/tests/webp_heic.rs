//! Port tests for `pr_imaging::webp_heic` (C# baseline d92d6aae, `PhotoReview.Imaging/Decoding/WebpHeicRoutingDecoder.cs`):
//! the Q-FMT-WEBP-HEIC router. WebP/HEIC/HEIF paths go to the WIC chain, every other path to the inner decoder; a
//! refusal (switch off, codec missing) happens before any decoder runs, and a missing codec is logged once per format.
//! Golden-driven: `tests/golden/webp-heic-routing.json` (oracle-cs `webp-heic-routing`), inputs in
//! `tests/golden-inputs/webp-heic-routing.json`.
//!
//! Red by design: the router bodies are `todo!()`, so every test here fails until the porter lands W4-12c.
//!
//! Documented skips (not ported here, owned by the units named):
//! - `WebpHeicNativeTests.cs` (real WIC codecs, `[Trait("Category","Native")]`): W4-11b/c. Its line 163 composes the router
//!   over real decoders, which is the W4-11c composition.
//! - `ServiceFactoriesTests.cs` line 182 `CreateSourcePrefetch`: App composition (W8); it only calls `will_refuse`.
//! - `FormatRoutingDecoderTests` (RAW router): W5.
//! - `Constructor_RejectsNullDependencies`: no Rust twin, `Arc` is non-null by type (see the comment at the end).
//! - `InEnglish` / `InVietnamese` (C# `TestLocalization`): no Rust twin here. The `CatalogTexts` double renders the
//!   `en.json` / `vi.json` templates; the real `Localizer`-backed `WebpHeicTexts` is the App wave (W8).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pr_core::abstractions::Log;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::webp_heic::{WebpHeicRoutingDecoder, WebpHeicTexts, WicCodecSupport};
use pr_test_support::golden;
use pr_test_support::{LogLevel, RecordingLog};
use serde_json::{Value, json};

/// C# `AllCodecs`: the WebP container, the HEIF container and the HEVC decoder are all present.
fn all_codecs() -> WicCodecSupport {
    WicCodecSupport {
        webp: true,
        heif_container: true,
        hevc_decoder: true,
        detail: "test".to_owned(),
    }
}

/// A 1x1 decoded image. C# `RecordingDecoder.Decode` returns `null!`; the Rust double needs a real view.
struct StubView {
    platform: PlatformImage,
}

impl DecodedImageView for StubView {
    fn pixel_width(&self) -> i32 {
        1
    }

    fn pixel_height(&self) -> i32 {
        1
    }

    fn downscaled(&self) -> bool {
        false
    }

    fn orientation(&self) -> i32 {
        1
    }

    fn estimated_bytes(&self) -> i64 {
        4
    }

    fn platform_image(&self) -> &PlatformImage {
        &self.platform
    }
}

/// C# `RecordingDecoder`: counts the calls and records the paths it was given (the test only counts and records).
#[derive(Default)]
struct RecordingDecoder {
    paths: Mutex<Vec<String>>,
}

impl RecordingDecoder {
    fn record(&self, path: &str) {
        self.paths
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(path.to_owned());
    }

    fn paths(&self) -> Vec<String> {
        self.paths.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn calls(&self) -> usize {
        self.paths.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
}

impl ImageDecoder for RecordingDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.record(&request.path.to_string_lossy());
        Ok(Arc::new(StubView {
            platform: PlatformImage::Opaque(Arc::new(())),
        }))
    }

    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        self.record(path);
        Ok(ImageInfo::new(1, 1))
    }
}

/// Reads `assets/languages/<code>.json` (the same catalog the app ships; never hand-copied).
fn load_language(code: &str) -> Value {
    let path = golden::repo_root()
        .join("assets")
        .join("languages")
        .join(format!("{code}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// The three router sentences rendered from `en.json` / `vi.json` with `{fileName}` substituted. Switching the language
/// changes what every already-built `UserText` shows (the sentence is evaluated at display time, as in the app).
struct CatalogTexts {
    english: Value,
    vietnamese: Value,
    use_vietnamese: AtomicBool,
}

impl CatalogTexts {
    fn load() -> Self {
        Self {
            english: load_language("en"),
            vietnamese: load_language("vi"),
            use_vietnamese: AtomicBool::new(false),
        }
    }

    fn set_vietnamese(&self, on: bool) {
        self.use_vietnamese.store(on, Ordering::SeqCst);
    }

    fn render(&self, key: &str, file_name: &str) -> String {
        let table = if self.use_vietnamese.load(Ordering::SeqCst) {
            &self.vietnamese
        } else {
            &self.english
        };
        let template = table[key]
            .as_str()
            .unwrap_or_else(|| panic!("language table has no string '{key}'"));
        template.replace("{fileName}", file_name)
    }
}

impl WebpHeicTexts for CatalogTexts {
    fn webp_heic_disabled(&self, file_name: &str) -> String {
        self.render("image.error.webpHeicDisabled", file_name)
    }

    fn webp_codec_missing(&self, file_name: &str) -> String {
        self.render("image.error.webpCodecMissing", file_name)
    }

    fn heif_codec_missing(&self, file_name: &str) -> String {
        self.render("image.error.heifCodecMissing", file_name)
    }
}

/// One router over fakes: the inner and WIC recorders, a switch, a counting codec probe, the log and the texts.
struct Harness {
    inner: Arc<RecordingDecoder>,
    wic: Arc<RecordingDecoder>,
    log: Arc<RecordingLog>,
    texts: Arc<CatalogTexts>,
    enabled: Arc<AtomicBool>,
    probe_calls: Arc<AtomicUsize>,
    codecs: WicCodecSupport,
    router: WebpHeicRoutingDecoder,
}

impl Harness {
    fn new(enabled: bool, codecs: WicCodecSupport) -> Self {
        let inner = Arc::new(RecordingDecoder::default());
        let wic = Arc::new(RecordingDecoder::default());
        let log = Arc::new(RecordingLog::new());
        let texts = Arc::new(CatalogTexts::load());
        let enabled_flag = Arc::new(AtomicBool::new(enabled));
        let probe_calls = Arc::new(AtomicUsize::new(0));

        let enabled_source = {
            let enabled_flag = Arc::clone(&enabled_flag);
            Arc::new(move || enabled_flag.load(Ordering::SeqCst))
        };
        let codecs_source = {
            let probe_calls = Arc::clone(&probe_calls);
            let codecs = codecs.clone();
            Arc::new(move || {
                probe_calls.fetch_add(1, Ordering::SeqCst);
                codecs.clone()
            })
        };

        let router = WebpHeicRoutingDecoder::new(
            Arc::clone(&inner) as Arc<dyn ImageDecoder>,
            Arc::clone(&wic) as Arc<dyn ImageDecoder>,
            enabled_source,
            codecs_source,
            Arc::clone(&texts) as Arc<dyn WebpHeicTexts>,
            Some(Arc::clone(&log) as Arc<dyn Log>),
        );

        Self {
            inner,
            wic,
            log,
            texts,
            enabled: enabled_flag,
            probe_calls,
            codecs,
            router,
        }
    }

    fn probes(&self) -> usize {
        self.probe_calls.load(Ordering::SeqCst)
    }

    fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::SeqCst);
    }

    /// `(inner calls, wic calls)`: tells which decoder a successful call was routed to.
    fn counts(&self) -> (usize, usize) {
        (self.inner.calls(), self.wic.calls())
    }

    /// The `Warn` lines of the log, in order (C# `RecordingLog.Warnings`).
    fn warnings(&self) -> Vec<String> {
        self.log
            .entries()
            .into_iter()
            .filter(|(level, _)| *level == LogLevel::Warn)
            .map(|(_, message)| message)
            .collect()
    }

    /// The static `WillRefuse`, fed by this harness's switch and probe (C# `WebpHeicRoutingDecoder.WillRefuse`).
    fn will_refuse(&self, path: Option<&str>) -> bool {
        let enabled_flag = Arc::clone(&self.enabled);
        let probe_calls = Arc::clone(&self.probe_calls);
        let codecs = self.codecs.clone();
        WebpHeicRoutingDecoder::will_refuse(path, &move || enabled_flag.load(Ordering::SeqCst), &move || {
            probe_calls.fetch_add(1, Ordering::SeqCst);
            codecs.clone()
        })
    }
}

/// Unwraps the refusal of a decode or read; a successful call fails the test (the decoded value is not `Debug`).
fn expect_refusal<T>(result: Result<T, DecodeError>, what: &str) -> DecodeError {
    match result {
        Ok(_) => panic!("{what}: expected a refusal, got a decoded image"),
        Err(error) => error,
    }
}

// C#: WebpHeicRoutingDecoderTests.WebpAndHeic_GoToTheWicChain_ForDecodeAndReadInfo
#[test]
fn webp_and_heic_go_to_the_wic_chain_for_decode_and_read_info() {
    for path in [r"C:\p\a.webp", r"C:\p\a.WEBP", r"C:\p\IMG_0001.HEIC", r"C:\p\a.heif"] {
        let harness = Harness::new(true, all_codecs());

        harness
            .router
            .decode(&DecodeRequest::new(path, 0))
            .unwrap_or_else(|e| panic!("decode {path}: {e}"));
        harness
            .router
            .read_info(path)
            .unwrap_or_else(|e| panic!("read_info {path}: {e}"));

        assert_eq!(
            harness.wic.paths(),
            vec![path.to_owned(), path.to_owned()],
            "WebP/HEIC {path} goes to the WIC chain for decode and read_info"
        );
        assert!(
            harness.inner.paths().is_empty(),
            "WebP/HEIC {path} never reaches the inner chain"
        );
    }
}

// C#: WebpHeicRoutingDecoderTests.OtherFormats_GoToTheInnerChain_WithoutRunningTheCodecProbe
#[test]
fn other_formats_go_to_the_inner_chain_without_running_the_codec_probe() {
    for path in [r"C:\p\a.jpg", r"C:\p\a.png", r"C:\p\a.cr2", r"C:\p\a.avif"] {
        // The switch only concerns WebP/HEIC; with it off and no codec at all, other formats must still route inward.
        let harness = Harness::new(false, WicCodecSupport::none("none"));

        harness
            .router
            .decode(&DecodeRequest::new(path, 0))
            .unwrap_or_else(|e| panic!("decode {path}: {e}"));
        harness
            .router
            .read_info(path)
            .unwrap_or_else(|e| panic!("read_info {path}: {e}"));

        assert_eq!(
            harness.inner.paths(),
            vec![path.to_owned(), path.to_owned()],
            "{path} goes inward"
        );
        assert!(harness.wic.paths().is_empty(), "{path} never reaches the WIC chain");
        assert_eq!(
            harness.probes(),
            0,
            "{path}: a JPEG-only session never pays for the WIC/MF enumeration"
        );
    }
}

// C#: WebpHeicRoutingDecoderTests.SwitchOff_RefusesWithALocalizedNotSupportedException_BeforeAnyDecoderRuns
#[test]
fn switch_off_refuses_with_a_localized_not_supported_error_before_any_decoder_runs() {
    let harness = Harness::new(false, all_codecs());

    let decode_error = expect_refusal(
        harness.router.decode(&DecodeRequest::new(r"C:\p\a.webp", 0)),
        "decode with the switch off",
    );
    let info_error = expect_refusal(
        harness.router.read_info(r"C:\p\b.heic"),
        "read_info with the switch off",
    );

    assert!(
        matches!(decode_error.kind(), DecodeError::NotSupported { .. }),
        "switch-off refusal is a NotSupportedException, not a MissingImageCodecException"
    );
    assert!(
        decode_error.is_localized(),
        "switch-off refusal carries the localized sentence"
    );
    assert!(
        decode_error.describe().contains("a.webp"),
        "English sentence names the file: {}",
        decode_error.describe()
    );
    assert!(
        info_error.describe().contains("b.heic"),
        "English sentence names the file: {}",
        info_error.describe()
    );
    assert!(harness.wic.paths().is_empty(), "no WIC decoder ran");
    assert!(harness.inner.paths().is_empty(), "no inner decoder ran");
    assert_eq!(harness.probes(), 0, "the codec probe did not run");
}

// C#: WebpHeicRoutingDecoderTests.HeifWithoutHevc_RefusesWithInstallGuidance_AndLogsOnceForManyFiles
#[test]
fn heif_without_hevc_refuses_with_install_guidance_and_logs_once_for_many_files() {
    let harness = Harness::new(
        true,
        WicCodecSupport {
            webp: true,
            heif_container: true,
            hevc_decoder: false,
            detail: "probe-detail".to_owned(),
        },
    );

    let errors: Vec<DecodeError> = (1..=3)
        .map(|index| {
            expect_refusal(
                harness
                    .router
                    .decode(&DecodeRequest::new(format!(r"C:\p\IMG_{index}.HEIC"), 0)),
                "HEIC without HEVC",
            )
        })
        .collect();
    let info_error = expect_refusal(
        harness.router.read_info(r"C:\p\IMG_9.heif"),
        "HEIF read_info without HEVC",
    );

    assert!(
        matches!(info_error.kind(), DecodeError::MissingCodec { .. }),
        "HEIF without HEVC is a MissingImageCodecException"
    );

    // English UI text: both Store component names and the file name.
    let english = errors[0].describe();
    assert!(english.contains("IMG_1.HEIC"), "English text names the file: {english}");
    assert!(
        english.contains("HEIF Image Extensions"),
        "English text names the HEIF extension: {english}"
    );
    assert!(
        english.contains("HEVC Video Extensions"),
        "English text names the HEVC extension: {english}"
    );

    // The same error, rendered after the language switch: the sentence is evaluated at display time.
    harness.texts.set_vietnamese(true);
    let vietnamese = errors[0].describe();
    harness.texts.set_vietnamese(false);
    assert!(
        vietnamese.contains("Microsoft Store"),
        "Vietnamese text names the Store: {vietnamese}"
    );
    assert_ne!(english, vietnamese, "translated, not the English fallback");

    // English log text names the file too.
    assert!(
        errors[0].message().contains("IMG_1.HEIC"),
        "log text: {}",
        errors[0].message()
    );
    assert!(
        harness.wic.paths().is_empty(),
        "no WIC decoder ran for the refused HEIC files"
    );

    let warnings = harness.warnings();
    assert_eq!(warnings.len(), 1, "one warning for four refused calls: {warnings:?}");
    assert!(
        warnings[0].contains("probe-detail"),
        "the warning carries the probe detail: {}",
        warnings[0]
    );

    // WebP is unaffected by the missing HEVC decoder.
    harness
        .router
        .decode(&DecodeRequest::new(r"C:\p\still.webp", 0))
        .unwrap_or_else(|e| panic!("WebP after a HEIF refusal: {e}"));
    assert_eq!(harness.wic.paths(), vec![r"C:\p\still.webp".to_owned()]);
}

// C#: WebpHeicRoutingDecoderTests.WebpCodecMissing_RefusesWithWebpGuidance_LoggedSeparatelyFromHeif
#[test]
fn webp_codec_missing_refuses_with_webp_guidance_logged_separately_from_heif() {
    let harness = Harness::new(true, WicCodecSupport::none("nothing"));

    let webp_error = expect_refusal(
        harness.router.decode(&DecodeRequest::new(r"C:\p\a.webp", 0)),
        "WebP without a codec",
    );
    expect_refusal(
        harness.router.decode(&DecodeRequest::new(r"C:\p\b.webp", 0)),
        "second WebP without a codec",
    );
    expect_refusal(
        harness.router.decode(&DecodeRequest::new(r"C:\p\c.heic", 0)),
        "HEIC without a codec",
    );

    let text = webp_error.describe();
    assert!(
        text.contains("WebP Image Extensions"),
        "WebP guidance names the extension: {text}"
    );
    assert!(!text.contains("HEVC"), "WebP guidance does not mention HEVC: {text}");

    let warnings = harness.warnings();
    assert_eq!(warnings.len(), 2, "one warning per format: {warnings:?}");
    assert!(
        warnings.iter().any(|w| w.contains("WebP")),
        "a WebP warning: {warnings:?}"
    );
    assert!(
        warnings.iter().any(|w| w.contains("HEIC")),
        "a HEIC warning: {warnings:?}"
    );
}

// C#: WebpHeicRoutingDecoderTests.SwitchIsReadOnEveryCall_SoASettingsChangeAppliesAtOnce
#[test]
fn switch_is_read_on_every_call_so_a_settings_change_applies_at_once() {
    let harness = Harness::new(true, all_codecs());
    harness
        .router
        .decode(&DecodeRequest::new(r"C:\p\a.webp", 0))
        .unwrap_or_else(|e| panic!("first decode with the switch on: {e}"));

    harness.set_enabled(false);

    expect_refusal(
        harness.router.decode(&DecodeRequest::new(r"C:\p\a.webp", 0)),
        "decode after the switch went off",
    );
    assert_eq!(harness.wic.calls(), 1, "only the first decode reached the WIC chain");
}

// C#: WebpHeicRoutingDecoderTests.WillRefuse_MatchesWhatDecodeWouldDo
#[test]
fn will_refuse_matches_what_decode_would_do() {
    // (path, switch on, WebP codec present, expected refusal). The `.heic` row has the WebP codec only, so HEIF is
    // still missing: `WicCodecSupport::heif` needs the container AND the HEVC decoder.
    let rows: [(Option<&str>, bool, bool, bool); 6] = [
        (Some(r"C:\p\a.jpg"), false, false, false),
        (Some(r"C:\p\a.webp"), true, true, false),
        (Some(r"C:\p\a.webp"), false, true, true),
        (Some(r"C:\p\a.webp"), true, false, true),
        (Some(r"C:\p\a.heic"), true, true, true),
        (None, false, false, false),
    ];

    for (path, enabled, webp_codec, expected) in rows {
        let codecs = WicCodecSupport {
            webp: webp_codec,
            heif_container: false,
            hevc_decoder: false,
            detail: "t".to_owned(),
        };
        let actual = WebpHeicRoutingDecoder::will_refuse(path, &move || enabled, &move || codecs.clone());
        assert_eq!(
            actual, expected,
            "will_refuse({path:?}, enabled={enabled}, webp={webp_codec})"
        );
    }
}

// Golden: tests/golden/webp-heic-routing.json (oracle-cs `webp-heic-routing`, csharp_commit d92d6aae). Every case runs
// its ops in order on a fresh router; the actual per-op and per-case JSON must equal the golden JSON exactly.

/// `tests/golden-inputs/webp-heic-routing.json`: the switch, the codec probe result and the ops of each case.
fn load_golden_inputs() -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("webp-heic-routing.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
}

/// C# `throws` name for the variant the router produces.
fn throws_name(error: &DecodeError) -> &'static str {
    match error.kind() {
        DecodeError::NotSupported { .. } => "NotSupportedException",
        DecodeError::MissingCodec { .. } => "MissingImageCodecException",
        DecodeError::InvalidArgument { .. } => "ArgumentException",
        _ => "other",
    }
}

/// Runs one golden op and returns the JSON with exactly the keys the golden op has.
fn run_op(harness: &Harness, op: &Value) -> Value {
    let kind = op["op"].as_str().expect("golden op has a kind");
    let path_value = op.get("path").cloned().unwrap_or(Value::Null);

    match kind {
        "decode" | "read_info" => {
            let path = path_value
                .as_str()
                .unwrap_or_else(|| panic!("{kind} needs a string path: {op}"))
                .to_owned();
            let before = harness.counts();
            let outcome = if kind == "decode" {
                harness.router.decode(&DecodeRequest::new(path, 0)).map(|_| ())
            } else {
                harness.router.read_info(&path).map(|_| ())
            };
            match outcome {
                Ok(()) => {
                    let after = harness.counts();
                    let routed = if after.0 > before.0 {
                        "inner"
                    } else if after.1 > before.1 {
                        "wic"
                    } else {
                        panic!("{kind} {path_value} succeeded without reaching a decoder")
                    };
                    json!({
                        "op": kind,
                        "path": path_value,
                        "routed": routed,
                        "probe_calls": harness.probes(),
                    })
                }
                Err(error) => json!({
                    "op": kind,
                    "path": path_value,
                    "throws": throws_name(&error),
                    "is_missing_codec": matches!(error.kind(), DecodeError::MissingCodec { .. }),
                    "is_localized": error.is_localized(),
                    "message": error.message(),
                    "describe": error.describe(),
                    "probe_calls": harness.probes(),
                }),
            }
        }
        "will_refuse" => {
            let value = harness.will_refuse(path_value.as_str());
            json!({
                "op": kind,
                "path": path_value,
                "value": value,
                "probe_calls": harness.probes(),
            })
        }
        "set_enabled" => {
            let value = op["value"].as_bool().expect("set_enabled has a bool value");
            harness.set_enabled(value);
            json!({
                "op": kind,
                "path": path_value,
                "value": value,
                "probe_calls": harness.probes(),
            })
        }
        other => panic!("unknown golden op '{other}'"),
    }
}

/// Runs one golden input case on a fresh router and returns `{name, ops, warnings}` in the golden's shape.
fn run_case(input: &Value) -> Value {
    let name = input["name"].as_str().expect("case has a name");
    let codecs_in = &input["codecs"];
    let codecs = WicCodecSupport {
        webp: codecs_in["webp"].as_bool().expect("codecs.webp is a bool"),
        heif_container: codecs_in["heif_container"]
            .as_bool()
            .expect("codecs.heif_container is a bool"),
        hevc_decoder: codecs_in["hevc"].as_bool().expect("codecs.hevc is a bool"),
        detail: codecs_in["detail"]
            .as_str()
            .expect("codecs.detail is a string")
            .to_owned(),
    };
    let enabled = input["enabled"]
        .as_bool()
        .unwrap_or_else(|| panic!("{name}: enabled is a bool"));
    let harness = Harness::new(enabled, codecs);

    let ops: Vec<Value> = input["ops"]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: ops is an array"))
        .iter()
        .map(|op| run_op(&harness, op))
        .collect();

    json!({
        "name": name,
        "ops": ops,
        "warnings": harness.warnings(),
    })
}

// C#: WebpHeicRoutingDecoderTests (golden-driven: all 71 cases of tests/golden/webp-heic-routing.json, covering the
// 20 path spellings x {enabled+all codecs, disabled+all, enabled+none}, log-once for HEIF and WebP, the HEIF container
// without HEVC, WebP-only codec sets, a switch toggled between calls, probe counts, `will_refuse` with null and empty
// paths, and `read_info` with "" and "   ").
#[test]
fn golden_webp_heic_routing_matches_csharp_for_every_case() {
    let expected = golden::load("webp-heic-routing");
    let inputs = load_golden_inputs();
    assert_eq!(
        inputs.len(),
        expected.cases.len(),
        "golden inputs and golden outputs must have the same number of cases"
    );

    let mut mismatches = Vec::new();
    for (input, expected_case) in inputs.iter().zip(&expected.cases) {
        assert_eq!(
            input["name"], expected_case["name"],
            "inputs and outputs are in the same order"
        );
        let actual = run_case(input);
        if actual != *expected_case {
            mismatches.push(format!(
                "case {}:\n  expected {expected_case}\n  actual   {actual}",
                expected_case["name"]
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} of {} golden cases differ from C#:\n{}",
        mismatches.len(),
        expected.cases.len(),
        mismatches.join("\n")
    );
}

// C#: WebpHeicRoutingDecoderTests.Constructor_RejectsNullDependencies
// No Rust twin (non-null by type): `WebpHeicRoutingDecoder::new` takes `Arc` and `Option<Arc<dyn Log>>` only, so the C#
// `ArgumentNullException` checks cannot be written as a failing call.
