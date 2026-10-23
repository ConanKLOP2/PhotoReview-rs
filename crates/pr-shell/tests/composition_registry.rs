#[path = "common/composition_fixture.rs"]
mod composition_fixture;

use std::sync::Arc;

use pr_core::abstractions::Clock;
use pr_core::model::enums::{DecoderBackend, LoadingMode};
use pr_imaging::decode_types::DecodeRequest;
use pr_imaging::decoder_traits::ImageDecoderFactory;

use pr_shell::composition::{
    platform::{PlatformError, UnavailableEmbeddedThumbnails, WicFineScaler},
    registry::RegistryError,
};

#[test]
fn registry_error_names_the_accessor() {
    // new: RegistryError keeps the accessor name and underlying message visible.
    let error = RegistryError {
        service: "preview_service",
        message: "invalid capacity".to_owned(),
    };
    assert_eq!(
        error.to_string(),
        "preview_service could not be created: invalid capacity"
    );
}

#[test]
fn platform_error_names_the_failed_adapter() {
    // new: PlatformError preserves the failed platform seam.
    let error = PlatformError {
        service: "explorer_order",
        message: "thread unavailable".to_owned(),
    };
    assert_eq!(
        error.to_string(),
        "explorer_order could not be created: thread unavailable"
    );
}

#[test]
fn unavailable_embedded_thumbnails_returns_no_frame() {
    // new: the O1 backend reports no embedded frame so callers continue to normal decoding.
    let reader: Box<dyn pr_core::abstractions::ReadSeek> = Box::new(std::io::Cursor::new(Vec::<u8>::new()));
    let result = pr_imaging::embedded_thumbnail::EmbeddedThumbnailBackend::open(&UnavailableEmbeddedThumbnails, reader);
    assert!(matches!(result, Ok(None)));
}

#[test]
fn composition_fixture_reads_the_registration_golden() {
    // new: the fixture shares the C# registration golden reader used by registration parity checks.
    let root = composition_fixture::temp_root();
    assert!(root.path().exists());
    assert!(!composition_fixture::registrations_golden().cases.is_empty());
}

#[test]
fn shared_registrations_are_all_mapped() {
    // new: bind the C# shared registration block to its Rust accessor/lifetime decision.
    let golden = composition_fixture::registrations_golden();
    let descriptors = golden.cases[0]["descriptors"].as_array().expect("descriptors array");
    let expected = [
        ("PhotoReview.Core.Abstractions.IAppPaths", "paths", "Singleton"),
        ("PhotoReview.Core.Abstractions.IFileSystem", "file_system", "Singleton"),
        (
            "PhotoReview.Core.Abstractions.ISourceReader",
            "source_reader",
            "Singleton",
        ),
        ("PhotoReview.Core.Abstractions.IClock", "clock", "Singleton"),
        ("PhotoReview.Core.Abstractions.ILog", "log", "Singleton"),
        ("PhotoReview.Core.Settings.SettingsStore", "settings_store", "Singleton"),
        ("PhotoReview.Core.Session.SessionStore", "session_store", "Singleton"),
        ("PhotoReview.Core.Session.SessionWriter", "session_writer", "Singleton"),
        (
            "PhotoReview.App.Localization.LocalizationService",
            "localization",
            "Singleton",
        ),
        ("PhotoReview.Core.FileActions.OperationJournal", "journal", "Singleton"),
        (
            "PhotoReview.Core.FileActions.RecoveryRetryService",
            "recovery_retry",
            "Singleton",
        ),
        ("PhotoReview.App.FileHashService", "file_hash_service", "Singleton"),
        (
            "PhotoReview.Core.FileActions.FileActionService",
            "file_action_service",
            "Singleton",
        ),
        ("PhotoReview.Core.FileActions.UndoService", "undo_service", "Singleton"),
        ("PhotoReview.Core.Diagnostics.ReviewMetrics", "metrics", "Singleton"),
        (
            "PhotoReview.Core.Catalog.LatestExplorerSnapshot",
            "explorer_snapshot",
            "Singleton",
        ),
        (
            "PhotoReview.Core.Abstractions.IExplorerOrderProvider",
            "explorer_order",
            "Singleton",
        ),
        ("PhotoReview.Core.Abstractions.IRecycleBin", "recycle_bin", "Singleton"),
        (
            "PhotoReview.Core.Abstractions.ILiveOperationRegistry",
            "live_operations",
            "Singleton",
        ),
        (
            "PhotoReview.Core.Abstractions.IMemoryProbe",
            "memory_probe",
            "Singleton",
        ),
        (
            "PhotoReview.Core.Abstractions.INaturalComparer",
            "natural_comparer",
            "Singleton",
        ),
        (
            "PhotoReview.Core.Abstractions.IDisplayClock",
            "display_clock",
            "Singleton",
        ),
        (
            "PhotoReview.Core.Abstractions.IKeyNameValidator",
            "key_names",
            "Singleton",
        ),
        (
            "PhotoReview.App.Services.ViewportSizeSource",
            "viewport_size_source",
            "Singleton",
        ),
        (
            "PhotoReview.App.Services.IPresentationObserver",
            "host: observer",
            "Singleton",
        ),
        (
            "PhotoReview.Imaging.Decoding.IImageDecoderFactory",
            "decoder_factory",
            "Singleton",
        ),
        (
            "PhotoReview.Imaging.Caching.ThumbnailCache",
            "thumbnail_cache",
            "Singleton",
        ),
        (
            "PhotoReview.Imaging.Caching.SourceBytesCachePolicy",
            "source_bytes_policy",
            "Singleton",
        ),
        (
            "PhotoReview.App.Services.PreviewStateContext",
            "preview_state",
            "Singleton",
        ),
        (
            "PhotoReview.Imaging.Caching.PreviewImageService",
            "preview_service",
            "Singleton",
        ),
        ("System.Func", "preload_scheduler_factory", "Singleton"),
        (
            "PhotoReview.App.ViewModels.ViewerState",
            "transient: create_main_view_model",
            "Transient",
        ),
        (
            "PhotoReview.App.ViewModels.CompareViewModel",
            "transient: create_main_view_model",
            "Transient",
        ),
        (
            "PhotoReview.Core.Catalog.ReviewCatalog",
            "transient: create_main_view_model",
            "Transient",
        ),
        (
            "PhotoReview.Core.Catalog.GenerationClock",
            "transient: create_main_view_model",
            "Transient",
        ),
        (
            "PhotoReview.App.ViewModels.MainViewModel",
            "transient: create_main_view_model",
            "Transient",
        ),
    ];
    let actual = &descriptors[12..48];
    assert_eq!(actual.len(), expected.len());
    for (descriptor, (service_type, _binding, lifetime)) in actual.iter().zip(expected) {
        assert_eq!(descriptor["service_type"].as_str(), Some(service_type));
        assert_eq!(descriptor["lifetime"].as_str(), Some(lifetime));
    }
}

#[test]
fn host_without_wpf_resolves_the_shared_graph_with_the_production_defaults() {
    // C#: SharedServiceRegistrationTests.HostWithoutWpf_ResolvesTheSharedGraph_WithTheProductionDefaults
    let (services, _root, log) = composition_fixture::shared_services();
    let settings = services.settings_store();
    assert!(Arc::ptr_eq(&settings, &services.settings_store()));
    assert!(Arc::ptr_eq(
        &services.log(),
        &(log.clone() as Arc<dyn pr_core::abstractions::Log>)
    ));
    assert!(services.key_names().is_valid_key_name("F5"));
    assert!(!services.key_names().is_valid_key_name("NotAKey"));
    let clock = services.clock();
    assert_eq!(clock.utc_now(), pr_test_support::FakeClock::new().utc_now());
    assert!(!services.file_system().file_exists(&services.paths().config_file));
    assert!(services.session_writer().is_ok());
    let _ = services.session_store();
    let _ = services.localization();
    let _ = services.journal();
    let _ = services.recovery_retry();
    assert!(services.file_hash_service().is_ok());
    let _ = services.file_action_service();
    let _ = services.undo_service();
    let _ = services.metrics();
    let _ = services.explorer_snapshot();
    let _ = services.explorer_order();
    let _ = services.recycle_bin();
    let _ = services.live_operations();
    let _ = services.memory_probe();
    let _ = services.natural_comparer();
    let _ = services.display_clock();
    let _ = services.viewport_size_source();
    assert!(services.decoder_factory().is_ok());
    assert!(services.thumbnail_cache().is_ok());
    assert!(services.source_bytes_policy().is_ok());
    let _ = services.preview_state();
    assert!(services.preview_service().is_ok());
    let _ = services.preload_scheduler_factory();
}

#[test]
fn thumbnail_cache_and_preview_image_service_use_app_paths_disk_directories() {
    // C#: SharedServiceRegistrationTests.ThumbnailCacheAndPreviewImageService_UseAppPathsDiskDirectories
    let (services, _root, _log) = composition_fixture::shared_services();
    let thumbnail = services.thumbnail_cache().expect("thumbnail cache");
    let preview = services.preview_service().expect("preview service");
    assert_eq!(thumbnail.disk_directory(), services.paths().thumbnail_cache_dir);
    assert_eq!(preview.disk_directory(), services.paths().preview_cache_dir);
}

#[test]
fn cache_directories_follow_app_paths_under_a_data_root_override() {
    // C#: SharedServiceRegistrationTests.CacheDirectoriesFollowAppPathsUnderDataRootOverride
    let root = composition_fixture::temp_root();
    let data = root.path().join("custom-data");
    let (services, _fixture_root, _log) =
        composition_fixture::shared_services_with(None, None, Some(data.to_string_lossy().into_owned()));
    assert_eq!(services.paths().journal_file.parent(), Some(data.as_path()));
    assert_eq!(
        services.thumbnail_cache().expect("thumbnail").disk_directory(),
        services.paths().thumbnail_cache_dir
    );
    assert_eq!(
        services.preview_service().expect("preview").disk_directory(),
        services.paths().preview_cache_dir
    );
}

#[test]
fn viewport_size_source_defaults_to_zero_zero() {
    // C#: SharedServiceRegistrationTests.ViewportSizeSource_DefaultsToZeroZero
    let (services, _root, _log) = composition_fixture::shared_services();
    assert_eq!(services.viewport_size_source().viewport_size(), (0.0, 0.0));
}

#[test]
fn source_bytes_policy_is_disabled_when_the_setting_is_off() {
    // C#: SharedServiceRegistrationTests.SourceBytesCachePolicy_DisabledWhenSettingOff
    let (services, _root, _log) = composition_fixture::shared_services();
    let store = services.settings_store();
    let mut settings = (*store.current()).clone();
    settings.use_source_bytes_cache = false;
    store.save(settings).expect("save settings");
    assert!(services.source_bytes_policy().expect("policy").cache().is_none());
}

#[test]
fn settings_dependent_services_read_the_settings_when_first_resolved_not_at_build() {
    // new: settings read order is deferred until first service resolution.
    let (services, _root, _log) = composition_fixture::shared_services();
    let store = services.settings_store();
    let mut settings = (*store.current()).clone();
    settings.image_cache_capacity_bytes = 8 * 1024 * 1024 * 1024;
    store.save(settings).expect("save before first resolution");
    assert_eq!(
        services.preview_service().expect("preview").capacity_bytes(),
        8 * 1024 * 1024 * 1024
    );
}

#[test]
fn source_bytes_policy_enabled_shares_the_same_cache_across_consumers() {
    // C#: SharedServiceRegistrationTests.SourceBytesCachePolicy_EnabledSharesSameCacheAcrossConsumers
    let (services, _root, _log) = composition_fixture::shared_services();
    let store = services.settings_store();
    let mut settings = (*store.current()).clone();
    settings.use_source_bytes_cache = true;
    settings.source_bytes_capacity_bytes = 1024 * 1024;
    store.save(settings).expect("save enabled settings");
    let policy = services.source_bytes_policy().expect("policy");
    assert!(policy.cache().is_some());
    assert!(services.preview_service().expect("preview").capacity_bytes() > 0);
}

#[test]
fn recovery_retry_takes_the_file_action_gate() {
    // C#: SharedServiceRegistrationTests.RecoveryRetryTakesTheFileActionGate
    let (services, _root, _log) = composition_fixture::shared_services();
    let gate = services.file_action_service();
    assert!(gate.try_begin());
    let fs = services.file_system();
    let result = services
        .recovery_retry()
        .retry_move_or_copy(
            &pr_core::file_actions::journal_line::JournalEntry {
                id: "entry".to_owned(),
                op_type: pr_core::model::FileOperationType::Move,
                state: pr_core::model::JournalState::Failed,
                source: "missing.jpg".to_owned(),
                destination: Some("destination.jpg".to_owned()),
                size: 1,
                last_write_utc: pr_core::file_actions::journal_line::JournalDateTime::utc(
                    pr_core::time::DateTimeUtc::now(),
                ),
                timestamp_utc: pr_core::file_actions::journal_line::JournalDateTime::utc(
                    pr_core::time::DateTimeUtc::now(),
                ),
                error: Some("failure".to_owned()),
                error_code: None,
                permanent: None,
                undo: None,
                group_id: None,
                group_members: None,
            },
            false,
            &pr_core::sync::CancelToken::new(),
        )
        .expect("busy result");
    assert!(!result.succeeded);
    assert_eq!(
        result.message,
        "A file action is still running. Wait for it to finish, then retry."
    );
    assert!(!fs.file_exists(std::path::Path::new("missing.jpg")));
    gate.end();
}

#[test]
fn preview_state_is_original_loading_mode_follows_settings() {
    // C#: SharedServiceRegistrationTests.PreviewStateContext_IsOriginalLoadingMode_FollowsSettings
    let (services, _root, _log) = composition_fixture::shared_services();
    let preview = services.preview_state();
    assert!(!(preview.is_original_loading_mode)());
    let store = services.settings_store();
    let mut settings = (*store.current()).clone();
    settings.loading_mode = Some(LoadingMode::Original);
    store.save(settings).expect("save settings");
    let _ = services.preview_service().expect("preview service");
    assert!((services.preview_state().is_original_loading_mode)());
}

#[test]
fn journal_uses_the_platform_live_operation_registry() {
    // C#: SharedServiceRegistrationTests.JournalUsesWindowsLiveOperationRegistry
    let (services, _root, _log) = composition_fixture::shared_services();
    assert!(Arc::ptr_eq(
        services.journal().live_operations(),
        &services.platform().live_operations
    ));
}

#[test]
fn preload_scheduler_factory_produces_a_valid_scheduler() {
    // C#: SharedServiceRegistrationTests.PreloadSchedulerFactory_ProducesValidScheduler
    let probe = Arc::new(composition_fixture::CountingProbe::default());
    let (services, _root, _log) = composition_fixture::shared_services_with_probe(probe.clone());
    let entries = Arc::new(|| {
        Ok(Arc::<[pr_core::catalog::types::CatalogEntry]>::from(vec![
            pr_core::catalog::types::CatalogEntry::new("item.jpg")?,
        ]))
    });
    let version = Arc::new(|| Ok(0));
    let scheduler = (services.preload_scheduler_factory())(entries, version).expect("scheduler");
    let run = scheduler.preload_around(0);
    assert!(run.wait_timeout(std::time::Duration::from_secs(3)));
    assert!(probe.calls.load(std::sync::atomic::Ordering::SeqCst) > 0);
    scheduler.dispose();
}

#[test]
fn app_decoder_factory_every_backend_goes_through_the_webp_heic_switch_wpf() {
    // C#: ServiceFactoriesTests.AppDecoderFactory_EveryBackendGoesThroughTheWebpHeicSwitch(Wpf)
    app_decoder_factory_webp_switch(DecoderBackend::Wpf);
}

#[test]
fn app_decoder_factory_every_backend_goes_through_the_webp_heic_switch_wic_direct() {
    // C#: ServiceFactoriesTests.AppDecoderFactory_EveryBackendGoesThroughTheWebpHeicSwitch(WicDirect)
    app_decoder_factory_webp_switch(DecoderBackend::WicDirect);
}

fn app_decoder_factory_webp_switch(backend: DecoderBackend) {
    let (services, _root, _log) = composition_fixture::shared_services();
    let store = services.settings_store();
    let mut settings = (*store.current()).clone();
    settings.webp_heic_support_enabled = false;
    store.save(settings).expect("save");
    let decoder = services.decoder_factory().expect("factory").create(backend);
    let error = match decoder.decode(&DecodeRequest::new("nope.webp", 1)) {
        Ok(_) => panic!("disabled WebP unexpectedly decoded"),
        Err(error) => error,
    };
    let user_text = error.user_text().expect("localized disabled text");
    assert!(user_text().contains("turned off"));
}

#[test]
fn source_reader_override_is_used_by_the_caches() {
    // new: the supplied source reader is threaded to the source-bytes cache and preview service.
    let reader = Arc::new(composition_fixture::CountingSourceReader::default());
    let (services, _root, _log) = composition_fixture::shared_services_with_move_override_and_reader(
        Arc::new(NoopMoveOverride),
        Arc::new(composition_fixture::ReaderOverride(Arc::clone(&reader))),
    );
    let store = services.settings_store();
    let mut settings = (*store.current()).clone();
    settings.use_source_bytes_cache = true;
    store.save(settings).expect("enable source cache");
    let policy = services.source_bytes_policy().expect("policy");
    assert!(policy.cache().is_some());
    let cache = policy.cache().expect("cache");
    let source = _root.path().join("cached.bin");
    std::fs::write(&source, [1, 2, 3, 4]).expect("write temporary source");
    cache.get_or_read(&source).expect("read source");
    cache.get_or_read(&source).expect("cached source");
    assert_eq!(reader.reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(services.preview_service().expect("preview").capacity_bytes() > 0);
}

struct NoopMoveOverride;
impl pr_app::coordinators::traits::IMoveOverride for NoopMoveOverride {
    fn move_file(&self, _source: &std::path::Path, _destination: &std::path::Path) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn decoder_wrapper_is_applied() {
    // new: the optional factory wrapper decorates the cached decoder factory.
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let wrapper = Arc::new(move |factory: Arc<dyn ImageDecoderFactory>| {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        factory
    });
    let (services, _root, _log) = composition_fixture::shared_services_with(None, Some(wrapper), None);
    let _ = services.decoder_factory().expect("factory");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn decoder_factory_routes_every_backend_through_raw_support_gate() {
    // C#: SharedServiceRegistrationTests.DecoderFactoryRoutesEveryBackendThroughRawSupportGate
    let (services, _root, _log) = composition_fixture::shared_services();
    let store = services.settings_store();
    let mut settings = (*store.current()).clone();
    settings.raw_support_enabled = false;
    store.save(settings).expect("disable RAW");
    let factory = services.decoder_factory().expect("factory");
    for backend in [DecoderBackend::Wpf, DecoderBackend::WicDirect] {
        let error = factory
            .create(backend)
            .read_info("missing.cr2")
            .expect_err("RAW disabled");
        assert!(error.user_text().is_some());
        assert!(error.describe().contains("RAW support is turned off"));
    }
    let mut settings = (*store.current()).clone();
    settings.raw_support_enabled = true;
    store.save(settings).expect("enable RAW");
    let _ = factory.create(DecoderBackend::WicDirect).read_info("missing.cr2");
}

#[test]
fn with_no_move_override_registered_file_action_service_and_undo_service_move_for_real() {
    // C#: SharedServiceRegistrationTests.WithNoMoveOverrideRegistered_FileActionServiceAndUndoServiceMoveForReal
    let root = composition_fixture::temp_root();
    let source_dir = root.path().join("source");
    let target_dir = root.path().join("target");
    let from = source_dir.join("from.jpg");
    let to = target_dir.join("from.jpg");
    std::fs::create_dir_all(&source_dir).expect("source dir");
    std::fs::write(&from, b"x").expect("source file");
    let (services, _fixture_root, _log) = composition_fixture::shared_services();
    let request = pr_core::file_actions::types::FileActionRequest {
        destination: Some(target_dir.to_string_lossy().into_owned()),
        ..pr_core::file_actions::types::FileActionRequest::new(
            from.to_string_lossy(),
            pr_core::model::FileOperationType::Move,
        )
    };
    let result = services
        .file_action_service()
        .execute(&request, &pr_core::sync::CancelToken::new());
    assert!(result.succeeded, "{result:?}");
    assert!(to.exists());
    let undo = services.undo_service();
    undo.lock().unwrap().register(&result);
    assert!(undo.lock().unwrap().undo_move().succeeded);
    assert!(from.exists());
}

#[test]
fn with_a_move_override_registered_file_action_service_and_undo_service_use_it() {
    // C#: SharedServiceRegistrationTests.WithMoveOverrideRegistered_FileActionServiceAndUndoServiceUseIt
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MoveRecorder(Arc<AtomicUsize>);
    impl pr_app::coordinators::traits::IMoveOverride for MoveRecorder {
        fn move_file(&self, _source: &Path, _destination: &Path) -> std::io::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let root = composition_fixture::temp_root();
    let source_dir = root.path().join("source");
    let target_dir = root.path().join("target");
    let source = source_dir.join("source.jpg");
    let dest = target_dir.join("source.jpg");
    std::fs::create_dir_all(&source_dir).expect("source dir");
    std::fs::create_dir_all(&target_dir).expect("target dir");
    std::fs::write(&source, b"data").expect("source file");
    let (services, _fixture_root, _log) = composition_fixture::shared_services_with_move_override_and_fs(
        Some(Arc::new(MoveRecorder(Arc::clone(&calls)))),
        Arc::new(pr_test_support::FakeFileSystem::new()),
    );
    let request = pr_core::file_actions::types::FileActionRequest {
        destination: Some(target_dir.to_string_lossy().into_owned()),
        ..pr_core::file_actions::types::FileActionRequest::new(
            source.to_string_lossy(),
            pr_core::model::FileOperationType::Move,
        )
    };
    let result = services
        .file_action_service()
        .execute(&request, &pr_core::sync::CancelToken::new());
    assert!(
        !result.succeeded,
        "the recording override intentionally does not move the file"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let undo = services.undo_service();
    std::fs::remove_file(&source).expect("clear source for undo preflight");
    std::fs::write(&dest, b"data").expect("prepare destination for undo preflight");
    let now = pr_core::time::DateTimeUtc::now();
    let undo_result = pr_core::file_actions::types::FileActionResult::new(
        true,
        pr_core::model::FileOperationType::Move,
        source.to_string_lossy(),
        Some(dest.to_string_lossy().into_owned()),
        4,
        now,
        None,
    );
    undo.lock().unwrap().register(&undo_result);
    let _ = undo.lock().unwrap().undo_move();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn host_without_wpf_registers_wpf_backend_through_wic_direct() {
    // C#: SharedServiceRegistrationTests.Options_WithoutWpfFallbackDecoder_MapsTheWpfSettingOntoWicDirect
    let (services, _root, _log) = composition_fixture::shared_services();
    let factory = services.decoder_factory().expect("factory");
    assert!(factory.is_registered(DecoderBackend::Wpf));
    let _ = factory.create(DecoderBackend::Wpf);
    let _ = factory.create(DecoderBackend::WicDirect);
}

#[test]
fn shared_services_resolve_all_expected_core_services() {
    // C#: SharedServiceRegistrationTests.RegistersAllExpectedCoreServices
    let (services, _root, _log) = composition_fixture::shared_services();
    let _ = services.settings_store().load(None).expect("load settings");
    assert!(services.session_writer().is_ok());
    assert!(services.file_hash_service().is_ok());
    assert!(services.decoder_factory().is_ok());
    assert!(services.thumbnail_cache().is_ok());
    assert!(services.source_bytes_policy().is_ok());
    assert!(services.preview_service().is_ok());
}

#[test]
fn singletons_return_the_same_instance() {
    // C#: SharedServiceRegistrationTests.SingletonsReturnSameInstance
    let (services, _root, _log) = composition_fixture::shared_services();
    assert!(Arc::ptr_eq(&services.settings_store(), &services.settings_store()));
    assert!(Arc::ptr_eq(
        &services.preview_service().expect("preview"),
        &services.preview_service().expect("preview")
    ));
    assert!(Arc::ptr_eq(&services.metrics(), &services.metrics()));
}

#[test]
fn options_wpf_fallback_decoder_fills_the_wpf_slot_and_the_webp_fallback_when_supplied() {
    // C#: SharedServiceRegistrationTests.Options_WpfFallbackDecoder_FillsTheWpfSlotAndTheWebpFallback_WhenSupplied
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let provider: pr_imaging::factory::DecoderProvider = Arc::new(move || {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Arc::new(composition_fixture::fixture_decoder())
    });
    let (services, _root, _log) = composition_fixture::shared_services_with(Some(provider), None, None);
    let factory = services.decoder_factory().expect("factory");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(factory.is_registered(DecoderBackend::Wpf));
    let _ = factory.create(DecoderBackend::Wpf);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn build_creates_nothing_and_singleton_cells_share_instances() {
    // new: build stores inputs only; the first cached accessor owns the singleton instance.
    let (services, root, log) = composition_fixture::shared_services();
    assert!(!services.paths().config_file.exists());
    assert!(Arc::ptr_eq(
        &services.log(),
        &(log.clone() as Arc<dyn pr_core::abstractions::Log>)
    ));
    assert!(Arc::ptr_eq(&services.metrics(), &services.metrics()));
    assert!(Arc::ptr_eq(&services.settings_store(), &services.settings_store()));
    assert!(root.path().exists());
}

#[test]
#[ignore = "native: requires Windows Imaging Component"]
fn wic_fine_scaler_resizes_a_pixel_buffer() {
    // new: native WIC adapter smoke test.
    let source = pr_imaging::pixels::PixelBuffer::allocate(2, 2, pr_imaging::pixels::PixelLayout::Bgr32).unwrap();
    let scaled = pr_turbojpeg::decoder::FineScaler::resize(&WicFineScaler, &source, 1, 1).unwrap();
    assert_eq!((scaled.width(), scaled.height()), (1, 1));
}
