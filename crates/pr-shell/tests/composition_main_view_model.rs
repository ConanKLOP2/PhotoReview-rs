use std::sync::Arc;

use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::model::RawPairMode;
use pr_shell::composition::main_view_model::CatalogFeed;

// Shared registry fixture has accessors for other composition test binaries as well.
#[allow(dead_code)]
#[path = "common/composition_fixture.rs"]
mod composition_fixture;

use pr_app::{
    coordinators::{
        presenter::ImageChange,
        sink_callbacks::{MainViewModelSinkCallbacks, PresentationSinkFactory},
        traits::{IPreloadController, IPresentationSink},
        zoom_detail::ThreadRunner,
    },
    services::contracts::{ClipboardService, PresentationObserver},
};
use pr_core::abstractions::{BatchReviewItem, DialogService, SkippedEntry};
use pr_imaging::{cache_key::ImageCacheKey, decode_types::PlatformImage, preload::scheduler::RunHandle};
use pr_shell::composition::main_view_model::{MainViewModelHost, create_main_view_model};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

struct Dialogs;
impl DialogService for Dialogs {
    fn show_confirmation(&self, _: &str, _: &str) -> bool {
        false
    }
    fn show_message(&self, _: &str, _: &str) {}
    fn show_error(&self, _: &str, _: &str) {}
    fn pick_folder(&self, _: Option<&Path>) -> Option<PathBuf> {
        None
    }
    fn show_batch_review(&self, _: &[BatchReviewItem]) -> bool {
        false
    }
    fn show_recovery(&self) {}
    fn show_diagnostics(&self) {}
    fn show_settings(&self) -> bool {
        false
    }
    fn show_benchmark(&self, _: Option<&Path>) {}
    fn show_skipped_files(&self, _: &[SkippedEntry]) {}
}
#[derive(Default)]
struct Clipboard(Rc<RefCell<Vec<String>>>);
impl ClipboardService for Clipboard {
    fn try_set_text(&self, text: &str) -> bool {
        self.0.borrow_mut().push(text.to_owned());
        true
    }
}
struct Observer(Rc<RefCell<Vec<String>>>);
impl PresentationObserver for Observer {
    fn on_presented(&self, path: &str) {
        self.0.borrow_mut().push(path.to_owned());
    }
}
struct Sink;
impl IPresentationSink for Sink {
    fn set_current_image(&mut self, _: Option<PlatformImage>, _: bool, _: &ImageChange<'_>) {}
    fn set_status_text(&mut self, _: &str) {}
    fn apply_initial_view_mode(&mut self) {}
    fn on_presented(&mut self, _: &str) {}
    fn trace_presented(&mut self, _: i64, _: &str, _: i64) {}
}
struct SinkFactory(Rc<RefCell<Option<MainViewModelSinkCallbacks>>>);
impl PresentationSinkFactory for SinkFactory {
    fn create(&self, callbacks: MainViewModelSinkCallbacks) -> Box<dyn IPresentationSink> {
        assert!(self.0.borrow().is_none(), "factory called more than once");
        *self.0.borrow_mut() = Some(callbacks);
        Box::new(Sink)
    }
}
struct NoPreload;
impl IPreloadController for NoPreload {
    fn preload_around(&self, _: i32) -> RunHandle {
        panic!("construction must not preload")
    }
    fn try_consume_preloaded_key(&self, _: &ImageCacheKey) -> bool {
        false
    }
    fn cancel(&self) {}
    fn remove_preloaded_keys_for_path(&self, _: &str) {}
    fn clear_preloaded_keys(&self) {}
}
fn host(
    callbacks: Rc<RefCell<Option<MainViewModelSinkCallbacks>>>,
    observer: Rc<RefCell<Vec<String>>>,
) -> MainViewModelHost {
    MainViewModelHost {
        ui_scheduler: Arc::new(pr_test_support::QueuedUiScheduler::new()),
        dialog_service: Arc::new(Dialogs),
        folder_picker: None,
        clipboard: Box::new(Clipboard::default()),
        sink_factory: Box::new(SinkFactory(callbacks)),
        observer: Rc::new(Observer(observer)),
        wake: Arc::new(|| {}),
        preload_controller: Some(Arc::new(NoPreload)),
        runner: Arc::new(ThreadRunner),
    }
}

#[test]
fn main_view_model_uses_the_host_clipboard_and_the_host_sink_factory() {
    // C#: SharedServiceRegistrationTests.MainViewModel_UsesTheHostClipboardAndTheHostSinkFactory
    let (services, _root, _) = composition_fixture::shared_services();
    let mut settings = (*services.settings_store().current()).clone();
    settings.initial_view_mode = pr_core::model::InitialViewMode::ClickZoomLevel;
    settings.click_zoom_percent = 300;
    services.settings_store().save(settings).unwrap();
    let callbacks = Rc::new(RefCell::new(None));
    let observed = Rc::new(RefCell::new(Vec::new()));
    let mut vm = create_main_view_model(&services, host(callbacks.clone(), observed.clone())).unwrap();
    let mut callbacks = callbacks.borrow_mut().take().unwrap();
    (callbacks.set_status_text)("status");
    assert!(!vm.pump().is_empty());
    (callbacks.set_current_image)(
        None,
        true,
        &ImageChange {
            presented_path: Some("one.jpg"),
            original_width: 400,
            original_height: 300,
            is_same_source_swap: false,
        },
    );
    assert!(vm.pump().image_changing.unwrap().is_file_change);
    (callbacks.on_presented)("one.jpg");
    assert_eq!(&*observed.borrow(), &["one.jpg"]);
    (callbacks.apply_initial_view_mode)();
    assert_eq!(vm.viewer().borrow().effective_zoom(), Some(3.0));
    assert_eq!((callbacks.take_viewer_zoom_change)(), Some(Some(3.0)));
    assert_eq!((callbacks.take_viewer_zoom_change)(), None);
}

#[test]
fn sink_callbacks_set_current_image_reports_image_changing_only_for_a_file_change() {
    // C#: SharedServiceRegistrationTests.SinkCallbacks_SetCurrentImage_RaisesImageChangingOnlyForAFileChange
    let (services, _root, _) = composition_fixture::shared_services();
    let callbacks = Rc::new(RefCell::new(None));
    let mut vm = create_main_view_model(&services, host(callbacks.clone(), Rc::default())).unwrap();
    let mut callbacks = callbacks.borrow_mut().take().unwrap();
    let change = ImageChange {
        presented_path: Some("one.jpg"),
        original_width: 400,
        original_height: 300,
        is_same_source_swap: false,
    };
    (callbacks.set_current_image)(None, false, &change);
    assert!(vm.pump().image_changing.is_none());
    (callbacks.set_current_image)(None, true, &change);
    assert!(vm.pump().image_changing.unwrap().is_file_change);
}

#[test]
fn viewer_state_is_transient_so_every_main_view_model_owns_its_own_viewer() {
    // C#: SharedServiceRegistrationTests.ViewerState_IsTransient_SoEveryMainViewModelOwnsItsOwnViewer
    let (services, _root, _) = composition_fixture::shared_services();
    let first_callbacks = Rc::new(RefCell::new(None));
    let first = create_main_view_model(&services, host(first_callbacks.clone(), Rc::default())).unwrap();
    let second = create_main_view_model(&services, host(Rc::default(), Rc::default())).unwrap();
    let mut callbacks = first_callbacks.borrow_mut().take().unwrap();
    (callbacks.set_current_image)(
        None,
        true,
        &ImageChange {
            presented_path: Some("one.jpg"),
            original_width: 400,
            original_height: 300,
            is_same_source_swap: false,
        },
    );
    assert_eq!(first.viewer().borrow().source_pixel_width(), 400);
    assert_eq!(second.viewer().borrow().source_pixel_width(), 0);
    assert!(!std::ptr::eq(first.catalog(), second.catalog()));
}

#[test]
fn create_main_view_model_does_not_touch_the_disk_or_start_a_decode() {
    // new: composition initializes services without running review work.
    let (services, root, _) = composition_fixture::shared_services();
    let vm = create_main_view_model(&services, host(Rc::default(), Rc::default())).unwrap();
    let snapshot = services.metrics().snapshot();
    assert_eq!(snapshot.source_reads, 0);
    assert_eq!(snapshot.source_open_count, 0);
    assert_eq!(snapshot.stat_count, 0);
    assert_eq!(snapshot.session_write_count, 0);
    assert_eq!(snapshot.decode_milliseconds, 0);
    assert_eq!(snapshot.cache_misses, 0);
    assert!(vm.is_idle());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn host_clipboard_is_used_after_the_first_catalog_publication() {
    // C#: SharedServiceRegistrationTests.MainViewModel_UsesTheHostClipboardAndTheHostSinkFactory
    use pr_app::coordinators::zoom_detail::{BackgroundJob, BackgroundRunner};
    use std::{collections::VecDeque, sync::Mutex};
    #[derive(Default)]
    struct Jobs(Mutex<VecDeque<BackgroundJob>>);
    impl BackgroundRunner for Jobs {
        fn run(&self, _: &'static str, job: BackgroundJob) {
            self.0.lock().unwrap().push_back(job);
        }
    }
    let (services, root, _) = composition_fixture::shared_services();
    let folder = root.path().join("photos");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("one.jpg"), []).unwrap();
    let copied = Rc::new(RefCell::new(Vec::new()));
    let jobs = Arc::new(Jobs::default());
    let mut inputs = host(Rc::default(), Rc::default());
    inputs.clipboard = Box::new(Clipboard(copied.clone()));
    inputs.runner = jobs.clone();
    let mut vm = create_main_view_model(&services, inputs).unwrap();
    vm.open_folder(&folder.to_string_lossy(), None).unwrap();
    // Execute only the initial folder/Explorer jobs; image work remains queued.
    let initial: Vec<_> = jobs.0.lock().unwrap().drain(..).collect();
    assert!(!initial.is_empty());
    for job in initial {
        job();
    }
    vm.pump();
    assert_eq!(vm.catalog().count(), 1);
    vm.copy_file_name();
    assert_eq!(&*copied.borrow(), &["one.jpg"]);
    vm.close_session();
}

#[test]
fn feed_preload_controller_forwards_every_method() {
    // new: exercise the adapter over a real scheduler with deterministic fake cache/clock/I/O.
    use pr_core::{abstractions::FileSystem, diagnostics::metrics::ReviewMetrics, sync::CancelToken};
    use pr_imaging::{
        cache_key::KeyOptions,
        decode_types::DecodeBox,
        decoder_traits::DecodeError,
        navigation_pace::NavigationPace,
        preload::{
            scheduler::{PreloadScheduler, PreloadSchedulerDeps},
            target::{KeyError, PreloadTarget},
        },
        preload_order::PreloadOptions,
    };
    use pr_shell::composition::main_view_model::FeedPreloadController;
    use std::{
        collections::HashSet,
        sync::{
            Mutex,
            atomic::{AtomicI64, Ordering},
        },
        time::Duration,
    };
    struct Target {
        fs: pr_test_support::FakeFileSystem,
        cached: Mutex<HashSet<ImageCacheKey>>,
        gate: Mutex<Option<DecodeGate>>,
    }
    #[derive(Clone)]
    struct DecodeGate {
        entered: std::sync::mpsc::Sender<CancelToken>,
        release: Arc<(Mutex<bool>, std::sync::Condvar)>,
    }
    struct Headroom;
    impl pr_core::abstractions::MemoryProbe for Headroom {
        fn has_headroom(&self, _: f64, _: i64) -> bool {
            true
        }
        fn get_snapshot(&self) -> Option<pr_core::abstractions::MemorySnapshot> {
            None
        }
    }
    impl Target {
        fn key(&self, path: &str) -> ImageCacheKey {
            ImageCacheKey::create(&self.fs, path, false, DecodeBox::UNBOUNDED, KeyOptions::default()).unwrap()
        }
    }
    impl PreloadTarget for Target {
        fn try_get_cached_preview_for_path(&self, path: &str) -> bool {
            self.try_get_cached_preview(&self.key(path))
        }
        fn try_get_cached_preview(&self, key: &ImageCacheKey) -> bool {
            self.cached.lock().unwrap().contains(key)
        }
        fn preload(&self, path: &str, cancel: &CancelToken) -> Result<(), DecodeError> {
            let gate = self.gate.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.entered.send(cancel.clone()).unwrap();
                let (released, changed) = &*gate.release;
                let (released, timeout) = changed
                    .wait_timeout_while(released.lock().unwrap(), Duration::from_secs(10), |released| !*released)
                    .unwrap();
                assert!(*released && !timeout.timed_out(), "bounded fake decode gate");
            }
            if cancel.is_canceled() {
                return Err(DecodeError::Canceled);
            }
            assert!(!cancel.is_canceled());
            self.cached.lock().unwrap().insert(self.key(path));
            Ok(())
        }
        fn get_current_cache_key_for_path(&self, path: &str) -> Result<ImageCacheKey, KeyError> {
            Ok(self.key(path))
        }
        fn get_current_cache_key(
            &self,
            entry: &pr_core::catalog::types::CatalogEntry,
        ) -> Result<ImageCacheKey, KeyError> {
            Ok(self.key(entry.path()))
        }
        fn cache_count(&self) -> i32 {
            i32::try_from(self.cached.lock().unwrap().len()).unwrap()
        }
        fn cache_bytes(&self) -> i64 {
            0
        }
    }
    let root = pr_test_support::TempRoot::new("composition-feed");
    let fs = pr_test_support::FakeFileSystem::new();
    let paths: Vec<_> = (0..2)
        .map(|i| root.path().join(format!("{i}.jpg")).to_string_lossy().into_owned())
        .collect();
    for path in &paths {
        fs.add_file(path, &[1]);
    }
    let entries = paths
        .iter()
        .map(|path| pr_core::catalog::types::CatalogEntry::new(path).unwrap())
        .collect();
    let target = Arc::new(Target {
        fs,
        cached: Mutex::new(HashSet::new()),
        gate: Mutex::new(None),
    });
    let feed = CatalogFeed::new();
    let mut catalog = ReviewCatalog::new();
    catalog.reset(entries, RawPairMode::default(), None);
    let clock = Arc::new(AtomicI64::new(0));
    let ticks = clock.clone();
    let pace = Arc::new(NavigationPace::with_timestamp(Box::new(move || {
        ticks.load(Ordering::SeqCst)
    })));
    let metrics = Arc::new(ReviewMetrics::new());
    metrics.record_source_read(1, 500, true);
    let scheduler = Arc::new(PreloadScheduler::new(
        target.clone(),
        metrics,
        feed.entries_fn(),
        Arc::new(Headroom),
        PreloadSchedulerDeps {
            options: Some(PreloadOptions {
                worker_count: 1,
                ..Default::default()
            }),
            pace: Some(pace.clone()),
            snapshot_version: Some(feed.version_fn()),
            env_lookup: Some(Arc::new(|_| None)),
            ..Default::default()
        },
    ));
    let controller = FeedPreloadController::new(scheduler.clone(), feed.clone());
    controller.catalog_changed(&catalog);
    assert_eq!(feed.version_fn()().unwrap(), catalog.structural_version());
    let key = target.key(&paths[1]);
    let warm = || {
        target.cached.lock().unwrap().clear();
        let run = controller.preload_around(0);
        assert!(run.wait_timeout(Duration::from_secs(10)));
        assert!(controller.is_idle());
        assert!(
            scheduler.try_consume_preloaded_key(&key),
            "cached={} warm={} workers={}",
            target.cached.lock().unwrap().len(),
            scheduler.preloaded_key_count_for_tests(),
            scheduler.worker_count()
        );
        // Reinsert by a fresh run for adapter key-consumption/removal checks.
        target.cached.lock().unwrap().clear();
        assert!(controller.preload_around(0).wait_timeout(Duration::from_secs(10)));
    };
    warm();
    assert!(controller.try_consume_preloaded_key(&key));
    assert!(!controller.try_consume_preloaded_key(&key));
    warm();
    controller.remove_preloaded_keys_for_path(key.path());
    assert!(!scheduler.try_consume_preloaded_key(&key));
    warm();
    controller.clear_preloaded_keys();
    assert!(!scheduler.try_consume_preloaded_key(&key));
    let block_decode = || {
        let (sent, received) = std::sync::mpsc::channel();
        let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        *target.gate.lock().unwrap() = Some(DecodeGate {
            entered: sent,
            release: release.clone(),
        });
        target.cached.lock().unwrap().clear();
        let run = controller.preload_around(0);
        let token = received.recv_timeout(Duration::from_secs(10)).unwrap();
        (run, token, release)
    };
    let (cancelled_run, token, release) = block_decode();
    let was_running = !controller.is_idle();
    controller.cancel();
    let cancelled = token.is_canceled();
    *release.0.lock().unwrap() = true;
    release.1.notify_all();
    assert!(cancelled_run.wait_timeout(Duration::from_secs(10)));
    assert!(was_running);
    assert!(cancelled, "cancel reached the active scheduler token");
    // Empty catalog keeps pace checks independent of background decode timing.
    catalog.reset(Vec::new(), RawPairMode::default(), None);
    controller.catalog_changed(&catalog);
    for index in 0..6 {
        clock.store(i64::from(index) * 1_000_000, Ordering::SeqCst);
        controller.notify_navigation(index);
    }
    assert!(pace.get_lead(500.0) > 0, "navigation reached the scheduler's pace");
    assert!(controller.get_viewer_decode_delay() > Duration::ZERO);
    assert_eq!(
        controller.get_viewer_decode_delay(),
        scheduler.get_viewer_decode_delay()
    );
    // A blocked decode makes dispose observable: it must cancel the active lifetime.
    catalog.reset(
        paths
            .iter()
            .map(|path| pr_core::catalog::types::CatalogEntry::new(path).unwrap())
            .collect(),
        RawPairMode::default(),
        None,
    );
    controller.catalog_changed(&catalog);
    let (disposed_run, token, release) = block_decode();
    scheduler.set_dispose_drain_timeout(Duration::ZERO);
    controller.dispose();
    let cancelled = token.is_canceled();
    *release.0.lock().unwrap() = true;
    release.1.notify_all();
    assert!(disposed_run.wait_timeout(Duration::from_secs(10)));
    assert!(cancelled, "dispose reached the active scheduler lifetime");
    assert!(scheduler.preload_around(0).is_complete());
    assert!(controller.preload_around(0).is_complete());
    assert!(target.fs.file_exists(Path::new(&paths[0])));
}

#[test]
fn catalog_feed_publishes_only_when_the_structural_version_changes() {
    // new: A12 keeps preload snapshot identity stable until a structural catalog change.
    let feed = CatalogFeed::new();
    let mut catalog = ReviewCatalog::new();
    let entries = feed.entries_fn();
    let version = feed.version_fn();
    assert_eq!(version().unwrap(), -1);
    feed.publish_if_changed(&catalog);
    let first = entries().unwrap();
    assert_eq!(version().unwrap(), catalog.structural_version());
    feed.publish_if_changed(&catalog);
    assert!(Arc::ptr_eq(&first, &entries().unwrap()));
    catalog.reset(Vec::new(), RawPairMode::default(), None);
    feed.publish_if_changed(&catalog);
    assert_eq!(version().unwrap(), catalog.structural_version());
    assert!(!Arc::ptr_eq(&first, &entries().unwrap()));
}

#[test]
fn catalog_feed_entries_and_version_fns_read_the_published_state() {
    // new: closures from cloned feeds observe publications across threads.
    let feed = CatalogFeed::new();
    let clone = feed.clone();
    let entries = clone.entries_fn();
    let version = clone.version_fn();
    let mut catalog = ReviewCatalog::new();
    catalog.reset(
        vec![
            pr_core::catalog::types::CatalogEntry::new("first.jpg").unwrap(),
            pr_core::catalog::types::CatalogEntry::new("second.jpg").unwrap(),
        ],
        RawPairMode::default(),
        None,
    );
    feed.publish_if_changed(&catalog);
    let expected = catalog.structural_version();
    let worker = std::thread::spawn(move || (entries().unwrap(), version().unwrap()));
    let (snapshot, observed) = worker.join().unwrap();
    assert_eq!(
        snapshot.iter().map(|entry| entry.path()).collect::<Vec<_>>(),
        ["first.jpg", "second.jpg"]
    );
    assert_eq!(observed, expected);
}
