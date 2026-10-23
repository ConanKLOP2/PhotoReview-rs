//! W8-17d early-decode prediction with fake monitor and decoder, no image reads.
use pr_app::services::initial_viewport::{MonitorId, MonitorProbe, PixelRect};
use pr_app::services::prewarm::PrewarmPreviews;
use pr_app::services::viewport_size_source::ViewportSizeSource;
use pr_app::startup::early_decode::*;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_imaging::cache_key::{CacheKeyError, ImageCacheKey};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::service::handle::PreviewHandle;
use pr_test_support::{LogLevel, RecordingLog, TempRoot};
use std::sync::{Arc, Mutex};

struct Probe(bool);
impl MonitorProbe for Probe {
    fn monitor_nearest_rect(&self, _: PixelRect) -> Option<MonitorId> {
        Some(MonitorId(1))
    }
    fn monitor_primary(&self) -> Option<MonitorId> {
        assert!(!self.0, "probe fault");
        Some(MonitorId(1))
    }
    fn monitor_work_area(&self, _: MonitorId) -> Option<PixelRect> {
        Some(PixelRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        })
    }
    fn dpi_for(&self, _: MonitorId) -> u32 {
        96
    }
    fn frame_insets(&self, _: u32) -> Option<PixelRect> {
        Some(PixelRect {
            left: -8,
            top: -31,
            right: 8,
            bottom: 8,
        })
    }
    fn work_areas(&self) -> Vec<PixelRect> {
        vec![self.monitor_work_area(MonitorId(1)).unwrap()]
    }
}
struct Previews(Arc<ViewportSizeSource>, Mutex<Vec<DecodeBox>>);
impl PrewarmPreviews for Previews {
    fn current_cache_key(&self, path: &str, target: Option<DecodeBox>) -> Result<ImageCacheKey, CacheKeyError> {
        assert_eq!(self.0.startup_prediction(), target);
        self.1.lock().unwrap().push(target.unwrap());
        Err(CacheKeyError::SourceNoLongerExists { full_name: path.into() })
    }
    fn start_viewer_preview(&self, _: &str, _: &ImageCacheKey) -> PreviewHandle {
        panic!("key failed; decode must not run")
    }
}

#[test]
fn prediction_is_prepared_without_starting_a_decode() {
    // C#: ShellStartupTests.Startup_EarlyDecode_StartsOnlyAfterTheInstanceLockIsHeld (preparation phase).
    let previews = Arc::new(Previews(Arc::new(ViewportSizeSource::new()), Mutex::new(vec![])));
    let log = RecordingLog::new();
    let plan = prepare_early_decode(None, previews.clone(), &Probe(false), 1200.0, 800.0, 1.15, &log).unwrap();
    assert!(plan.decode_box.width > 0 && plan.decode_box.height > 0);
    assert!(previews.1.lock().unwrap().is_empty());
    assert_eq!(previews.0.startup_prediction(), None);
    assert!(log.entries().is_empty());
}

#[test]
fn invalid_prediction_and_a_panicking_probe_do_not_break_startup() {
    // New: C# ShellStartup.PrepareEarlyDecodeAsync catches non-OOM failures.
    let previews = Arc::new(Previews(Arc::new(ViewportSizeSource::new()), Mutex::new(vec![])));
    let log = RecordingLog::new();
    assert!(prepare_early_decode(None, previews.clone(), &Probe(false), 1200.0, 800.0, 0.0, &log).is_none());
    assert!(prepare_early_decode(None, previews, &Probe(true), 1200.0, 800.0, 1.15, &log).is_none());
    assert!(log.contains(LogLevel::Error, "Early decode preparation failed"));
}

#[test]
fn start_sets_prediction_before_the_worker_and_marks_only_a_started_task() {
    // C#: ShellStartup.StartEarlyDecode; new deterministic seam check of its ordering.
    let root = TempRoot::new("startup-prewarm");
    let viewport = Arc::new(ViewportSizeSource::new());
    let previews = Arc::new(Previews(viewport.clone(), Mutex::new(vec![])));
    let decode_box = DecodeBox {
        width: 1024,
        height: 768,
    };
    let plan = EarlyDecodePlan {
        previews: previews.clone(),
        decode_box,
    };
    let marks = Mutex::new(Vec::new());
    let settings = AppSettings::new(&EnglishSettingsTexts);
    assert!(start_early_decode(None, &viewport, &settings, "a.jpg", &|_| panic!("no plan")).is_none());
    let task = start_early_decode(
        Some(plan.clone()),
        &viewport,
        &settings,
        &root.join("a.jpg").to_string_lossy(),
        &|m| marks.lock().unwrap().push(m.to_owned()),
    )
    .unwrap();
    task.wait().unwrap();
    assert_eq!(*previews.1.lock().unwrap(), vec![decode_box]);
    assert_eq!(*marks.lock().unwrap(), vec!["earlyDecodeStarted"]);
    assert!(
        start_early_decode(Some(plan), &viewport, &settings, "a.nef", &|_| panic!(
            "RAW is not prewarmed"
        ))
        .is_none()
    );
}
