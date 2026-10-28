//! W8-17e part A: a hidden native window with all data in MainEnv's TempRoot.
use crate::support::main_env::MainEnv;
use pr_app::{
    services::viewport_size_source::ViewportSizeSource, windowing::placement_store::JsonWindowPlacementStore,
};
use pr_core::{abstractions::NullLog, io::physical::PhysicalFileSystem};
use pr_platform_win::{
    host::{
        frame_clock::TimerFrameClock,
        main_window::{
            MainWindow,
            core::{MainWindowDeps, MainWindowOptions},
        },
        shell_window::{ShellWindow, ShellWindowOptions},
        ui_dispatcher::Win32UiDispatcher,
    },
    shell_startup::{
        cloak::{StartupCloak, StartupCloakOptions},
        placement::{NativeWindowPlacement, ShellPlacementSaver},
    },
    shell_viewing::presentation_sink::ShellPresentationHub,
};
use pr_shell::startup::{overlay_vm::MainWindowSlot, window::build_viewport_factory};
use std::{cell::RefCell, rc::Rc, sync::Arc};

pub fn with_window(body: impl FnOnce(&Rc<MainWindow>, &MainWindowSlot, &Arc<crate::support::main_env::TestDialog>)) {
    let env = MainEnv::new();
    let vm = Rc::new(RefCell::new(env.vm));
    let log = Arc::new(NullLog);
    let dispatcher = Win32UiDispatcher::new(log.clone()).unwrap();
    let clock = Rc::new(TimerFrameClock::create(None, 0).unwrap());
    let shell = Rc::new(ShellWindow::create(ShellWindowOptions::default(), clock.clone()).unwrap());
    let store = Arc::new(JsonWindowPlacementStore::new(
        Arc::new(PhysicalFileSystem),
        Arc::new(pr_platform_win::host::main_window::windowing::Win32MonitorLayout),
    ));
    let main = Rc::new(MainWindow::new(MainWindowDeps {
        window: shell,
        dispatcher: dispatcher.clone(),
        frame_clock: clock,
        view_model: vm,
        size_source: Arc::new(ViewportSizeSource::new()),
        hub: Rc::new(ShellPresentationHub::new()),
        cloak: Rc::new(StartupCloak::new(
            Arc::new(dispatcher.clone()),
            StartupCloakOptions::default(),
        )),
        placement: Rc::new(ShellPlacementSaver::new(
            store,
            Rc::new(NativeWindowPlacement),
            log.clone(),
        )),
        log,
        mark: None,
        viewport_factory: build_viewport_factory(Arc::new(pr_platform_win::display_clock::WindowsDisplayClock::new())),
        options: MainWindowOptions {
            activate: false,
            prefer_warp: true,
            placement_file: None,
            ..Default::default()
        },
    }));
    let slot = Rc::new(RefCell::new(Some(main.clone())));
    body(&main, &slot, &env.dialog);
    slot.borrow_mut().take();
    main.dispose();
    drop(main);
    dispatcher.shutdown();
}
