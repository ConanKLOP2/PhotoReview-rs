#![cfg(windows)]

// The fixture also supplies seams used only by registry test binaries.
#[allow(dead_code)]
#[path = "common/composition_fixture.rs"]
mod composition_fixture;

use pr_app::services::prewarm::PrewarmPreviews;
use pr_core::settings::app_settings::SettingsTexts;
use pr_shell::composition::{adapters::LazyPrewarmPreviews, graph::dialog_services_of};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn dialog_services_fills_the_fourteen_fields_as_documented() {
    // new: D-W8B6-153 maps each registered service into the secondary-dialog graph.
    let (services, _root, _) = composition_fixture::shared_services();
    let dialog = dialog_services_of(&services);
    assert!(Arc::ptr_eq(
        dialog.settings_store.as_ref().unwrap(),
        &services.settings_store()
    ));
    assert!(Arc::ptr_eq(
        dialog.localization.as_ref().unwrap(),
        &services.localization()
    ));
    assert!(dialog.update_checker.is_none());
    assert!(dialog.decoder_factory.is_some());
    assert!(Arc::ptr_eq(dialog.journal.as_ref().unwrap(), &services.journal()));
    assert!(Arc::ptr_eq(
        dialog.recovery_retry.as_ref().unwrap(),
        &services.recovery_retry()
    ));
    assert!(Arc::ptr_eq(&dialog.file_system, &services.file_system()));
    assert!(Arc::ptr_eq(dialog.metrics.as_ref().unwrap(), &services.metrics()));
    assert!(Arc::ptr_eq(
        dialog.explorer_snapshot.as_ref().unwrap(),
        &services.explorer_snapshot()
    ));
    assert!(Arc::ptr_eq(
        &dialog.shell_integration,
        &services.platform().shell_integration
    ));
    assert_eq!(dialog.physical_memory_bytes, 16_i64 << 30);
    assert_eq!(
        dialog.settings_texts.action_default_name(7),
        pr_shell::composition::texts::LocalizedTexts.action_default_name(7)
    );
    // runner and benchmark_sessions are callable seams; neither may run during graph construction.
    assert_eq!(Arc::strong_count(&dialog.runner), 1);
    assert_eq!(Arc::strong_count(&dialog.benchmark_sessions), 1);
}

#[test]
fn dialog_services_decoder_factory_is_lazy() {
    // new: the settings worker must run before the decoder singleton captures cache policy.
    let resolutions = Arc::new(AtomicUsize::new(0));
    let recorded = resolutions.clone();
    let (services, _root, _) = composition_fixture::shared_services_with(
        None,
        Some(Arc::new(move |factory| {
            recorded.fetch_add(1, Ordering::SeqCst);
            factory
        })),
        None,
    );
    let dialog = dialog_services_of(&services);
    assert_eq!(resolutions.load(Ordering::SeqCst), 0);
    assert!(
        dialog
            .decoder_factory
            .as_ref()
            .unwrap()
            .is_registered(pr_core::model::DecoderBackend::WicDirect)
    );
    assert_eq!(resolutions.load(Ordering::SeqCst), 1);
}

#[test]
fn graph_previews_are_lazy_until_the_early_decode_asks() {
    // new: graph wrappers may be built before settings, but resolve previews only on the first request.
    let resolutions = Arc::new(AtomicUsize::new(0));
    let recorded = resolutions.clone();
    let (services, root, _) = composition_fixture::shared_services_with(
        None,
        Some(Arc::new(move |factory| {
            recorded.fetch_add(1, Ordering::SeqCst);
            factory
        })),
        None,
    );
    let previews = LazyPrewarmPreviews::new(services.clone());
    let _dialog = dialog_services_of(&services);
    assert_eq!(resolutions.load(Ordering::SeqCst), 0);
    assert!(
        previews
            .current_cache_key(&root.path().join("missing.jpg").to_string_lossy(), None)
            .is_err()
    );
    assert_eq!(resolutions.load(Ordering::SeqCst), 1);
}

#[test]
#[ignore = "native: creates a dispatcher and shell message loop on an isolated UI thread"]
fn production_graph_defers_the_view_model_until_loaded_settings_are_available() {
    // new: D-W8B6-160 closes the eager-view-model/cache-policy ordering regression.
    use std::{rc::Rc, sync::mpsc, time::Duration};
    let (sent, received) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| {
            use pr_core::{abstractions::NullLog, app_paths::AppPaths};
            use pr_platform_win::{
                host::{message_loop::MessageLoop, ui_dispatcher::Win32UiDispatcher},
                shell_viewing::presentation_sink::ShellPresentationHub,
            };
            use pr_shell::startup::{
                composition_root::{ShellHostServices, ShellServices, ShellServicesOptions},
                graph::{GraphInputs, GraphOverrides, production_graph_builder_with},
            };
            let (fake_services, root, _) = composition_fixture::shared_services();
            let paths = AppPaths::new(root.path(), None, true);
            let log = Arc::new(NullLog);
            let dispatcher = Win32UiDispatcher::new(log.clone()).unwrap();
            let message_loop = Rc::new(MessageLoop::new(Rc::new(dispatcher.clone())).unwrap());
            let hub = Rc::new(ShellPresentationHub::new());
            let services = Rc::new(ShellServices::build(
                ShellHostServices {
                    dispatcher: dispatcher.clone(),
                    owner_window: Rc::new(|| 0),
                    message_loop: Some(message_loop.clone()),
                },
                hub.clone(),
                log.clone(),
                ShellServicesOptions::default(),
            ));
            let calls = Arc::new(AtomicUsize::new(0));
            let recorded = calls.clone();
            let builder = production_graph_builder_with(GraphOverrides {
                paths: Some(paths),
                log: Some(log.clone()),
                recycle_bin: Some(fake_services.recycle_bin()),
                explorer_order: Some(fake_services.explorer_order()),
                clock: Some(fake_services.clock()),
                decoder_wrapper: Some(Arc::new(move |factory| {
                    recorded.fetch_add(1, Ordering::SeqCst);
                    factory
                })),
            });
            let graph = builder(GraphInputs {
                services,
                dispatcher: dispatcher.clone(),
                message_loop,
                hub,
                log,
                mark: Arc::new(|_| {}),
            })
            .unwrap();
            assert_eq!(
                calls.load(Ordering::SeqCst),
                0,
                "graph must not eagerly resolve the decoder"
            );
            let mut settings = (*graph.settings_store.current()).clone();
            settings.image_cache_capacity_bytes = 8_i64 << 30;
            graph.settings_store.save(settings).unwrap();
            let vm = (graph.view_model)().unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(vm.borrow().settings().image_cache_capacity_bytes, 8_i64 << 30);
            vm.borrow_mut().close_session();
            drop(vm);
            dispatcher.shutdown();
        });
        sent.send(result).unwrap();
    });
    let result = received
        .recv_timeout(Duration::from_secs(60))
        .expect("bounded graph UI thread");
    worker.join().unwrap();
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}
