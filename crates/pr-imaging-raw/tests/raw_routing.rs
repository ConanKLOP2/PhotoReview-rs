//! C# parity tests for `FormatRoutingDecoder` (W5-12c), including the 116-case routing golden.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pr_core::text::dotnet_path::get_file_name;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging_raw::routing::{FormatRoutingDecoder, RawRoutingTexts};
use pr_test_support::golden;
use serde_json::{Value, json};

struct View(PlatformImage);

impl DecodedImageView for View {
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
        &self.0
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<String>>);

impl Recorder {
    fn paths(&self) -> Vec<String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
    fn record(&self, path: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(path.to_owned());
    }
}

impl ImageDecoder for Recorder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.record(&request.path.to_string_lossy());
        Ok(Arc::new(View(PlatformImage::Opaque(Arc::new(())))))
    }
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        self.record(path);
        Ok(ImageInfo::new(1, 1))
    }
}

struct Texts;
impl RawRoutingTexts for Texts {
    fn raw_disabled(&self, file_name: &str) -> String {
        format!("RAW support is turned off in Settings, so {file_name} cannot be shown.")
    }
}

fn harness(enabled: Arc<AtomicBool>, reads: Arc<AtomicUsize>) -> (FormatRoutingDecoder, Arc<Recorder>, Arc<Recorder>) {
    let standard = Arc::new(Recorder::default());
    let raw = Arc::new(Recorder::default());
    let flag = Arc::clone(&enabled);
    let count = Arc::clone(&reads);
    let router = FormatRoutingDecoder::new(
        standard.clone(),
        raw.clone(),
        Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            flag.load(Ordering::SeqCst)
        }),
        Arc::new(Texts),
    );
    (router, standard, raw)
}

fn error_name(error: &DecodeError) -> Option<&'static str> {
    match error {
        DecodeError::NotSupported { .. } => Some("NotSupportedException"),
        DecodeError::InvalidArgument { .. } => Some("ArgumentException"),
        _ => None,
    }
}

// C#: FormatRoutingDecoderTests (all 116 `raw-routing` golden cases).
#[test]
fn golden_raw_routing() {
    let input = golden::load("raw-routing");
    let expected = golden::load("raw-routing");
    let root = golden::repo_root();
    let input_text = std::fs::read_to_string(root.join("tests/golden-inputs/raw-routing.json")).unwrap();
    let input_doc: Value = serde_json::from_str(&input_text).unwrap();
    let input_cases = input_doc["cases"].as_array().unwrap();
    assert_eq!(input.cases.len(), 116);
    assert_eq!(expected.cases.len(), 116);
    for (source, golden_case) in input_cases.iter().zip(&expected.cases) {
        let name = source["name"].as_str().unwrap();
        assert_eq!(golden_case["name"], name);
        let enabled = Arc::new(AtomicBool::new(source["raw_enabled"].as_bool().unwrap()));
        let reads = Arc::new(AtomicUsize::new(0));
        let (router, standard, raw) = harness(Arc::clone(&enabled), Arc::clone(&reads));
        for (op, want) in source["ops"]
            .as_array()
            .unwrap()
            .iter()
            .zip(golden_case["ops"].as_array().unwrap())
        {
            let operation = op["op"].as_str().unwrap();
            if operation == "set_enabled" {
                enabled.store(op["value"].as_bool().unwrap(), Ordering::SeqCst);
                continue;
            }
            let path = op["path"].as_str().unwrap();
            let before_standard = standard.paths().len();
            let before_raw = raw.paths().len();
            let result = match operation {
                "decode" => router.decode(&DecodeRequest::new(path, 0)).map(|_| ()),
                "read_info" => router.read_info(path).map(|_| ()),
                other => panic!("{name}: unknown operation {other}"),
            };
            let count = reads.load(Ordering::SeqCst);
            assert_eq!(
                want["enabled_calls"].as_u64().unwrap() as usize,
                count,
                "{name}: setting reads"
            );
            match result {
                Ok(()) => {
                    assert_eq!(
                        want["routed"].as_str(),
                        if raw.paths().len() > before_raw {
                            Some("raw")
                        } else {
                            Some("standard")
                        },
                        "{name}: route"
                    );
                    assert!(want.get("throws").is_none(), "{name}: unexpected error");
                }
                Err(error) => {
                    assert_eq!(want["throws"].as_str(), error_name(&error), "{name}: error type");
                    assert_eq!(
                        want["is_localized"].as_bool().unwrap(),
                        error.is_localized(),
                        "{name}: localized"
                    );
                    if let Some(message) = want.get("message").and_then(Value::as_str) {
                        assert_eq!(error.message(), message, "{name}: message");
                    }
                    if let Some(description) = want.get("describe").and_then(Value::as_str) {
                        assert_eq!(error.describe(), description, "{name}: description");
                    }
                }
            }
            let standard_paths = standard.paths();
            let raw_paths = raw.paths();
            if standard_paths.len() > before_standard {
                assert_eq!(standard_paths.last().unwrap(), path, "{name}: forwarded standard path");
            }
            if raw_paths.len() > before_raw {
                assert_eq!(raw_paths.last().unwrap(), path, "{name}: forwarded RAW path");
            }
        }
        assert_eq!(
            json!(standard.paths()),
            golden_case["standard_paths"],
            "{name}: standard paths"
        );
        assert_eq!(json!(raw.paths()), golden_case["raw_paths"], "{name}: RAW paths");
    }
}

#[test]
fn router_is_not_cancellable_and_routes_non_raw_without_reading_setting() {
    let enabled = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(AtomicUsize::new(0));
    let (router, standard, raw) = harness(enabled, Arc::clone(&reads));
    assert!(router.as_cancellable().is_none());
    router.read_info("photo.jpg").unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(standard.paths(), ["photo.jpg"]);
    assert!(raw.paths().is_empty());
}

#[test]
fn disabled_raw_error_keeps_log_text_and_localized_description() {
    let (router, standard, raw) = harness(Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)));
    let error = router.read_info(r"C:\raw dir\photo.NEF").unwrap_err();
    assert!(matches!(error, DecodeError::NotSupported { .. }));
    assert_eq!(error.message(), "RAW support is disabled; cannot decode 'photo.NEF'.");
    assert_eq!(
        error.describe(),
        "RAW support is turned off in Settings, so photo.NEF cannot be shown."
    );
    assert!(standard.paths().is_empty() && raw.paths().is_empty());
}

#[test]
fn raw_path_is_not_retried_by_standard_decoder() {
    let (router, standard, raw) = harness(Arc::new(AtomicBool::new(true)), Arc::new(AtomicUsize::new(0)));
    let error = router.read_info("photo.dng").unwrap();
    assert_eq!(error, ImageInfo::new(1, 1));
    assert!(standard.paths().is_empty());
    assert_eq!(raw.paths(), ["photo.dng"]);
}

#[test]
fn dotnet_file_name_extracts_raw_error_name() {
    assert_eq!(get_file_name(r"C:\raw dir\photo.NEF"), "photo.NEF");
}
