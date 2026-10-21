//! Fixture of the W8-14f tests (`tests/shell_menus_main_window.rs`): fake `MainWindowMenuServices` over the REAL `MainViewModel` and the REAL
//! `MainWindowInput` of the W8-14c fixture (`common/input_feature_fixture.rs`), plus a scripted `PopupMenuHost`.
//!
//! The including test crate declares at its root everything `input_feature_fixture.rs` needs (`file_action_fakes`, `support::{main_env, presenter_fakes,
//! preview_fakes}`, `fixture` = `common/shell_input_dispatch_fixture.rs`, `viewing_fakes`) and `input_feature_fixture` itself.
#![allow(dead_code)] // shared by the tests of one crate, each of which uses a different part

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::input::primitives::PointD;
use pr_app::menus::model::MenuItemModel;
use pr_app::viewmodels::main::{ChangeSet, MainViewModel, SettingsSource};
use pr_core::abstractions::Log;
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::shell_menus::controller::PopupMenuHost;
use pr_platform_win::shell_menus::main_window::{MainWindowMenuServices, MainWindowMenus};
use pr_platform_win::win32::structs::Hwnd;
use pr_test_support::RecordingLog;

use crate::input_feature_fixture::FeatureRig;
use crate::support::main_env::MainEnv;

/// An error with a fixed text (the `ToString` is what the log and the status line show).
#[derive(Debug)]
pub struct Boom(pub &'static str);

impl fmt::Display for Boom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl Error for Boom {}

/// `MainWindowMenuServices` over a [`FeatureRig`]: the window, the settings and the view model are the rig's; the log, the shell-errors sink and the
/// status line are recorded.
pub struct MenuServices {
    pub rig: Rc<FeatureRig>,
    pub owner: Cell<Hwnd>,
    pub log: Arc<RecordingLog>,
    /// `(context, error text)` of every `report_shell_error`.
    pub shell_errors: RefCell<Vec<(String, String)>>,
    /// The text of every `show_menu_failed_status`.
    pub statuses: RefCell<Vec<String>>,
    /// Calls of `with_view_model` made through this services object.
    pub vm_calls: Cell<u32>,
    pub zoom_name: RefCell<String>,
}

impl MainWindowMenuServices for MenuServices {
    fn window(&self) -> Rc<dyn ShellWindowApi> {
        Rc::clone(&self.rig.window) as Rc<dyn ShellWindowApi>
    }
    fn owner_hwnd(&self) -> Hwnd {
        self.owner.get()
    }
    fn log(&self) -> Arc<dyn Log> {
        Arc::clone(&self.log) as Arc<dyn Log>
    }
    fn settings(&self) -> SettingsSource {
        self.rig.env.borrow().settings.clone()
    }
    fn with_view_model(&self, f: &mut dyn FnMut(&mut MainViewModel) -> ChangeSet) {
        self.vm_calls.set(self.vm_calls.get() + 1);
        let mut env = self.rig.env.borrow_mut();
        let _ = f(&mut env.vm);
    }
    fn report_shell_error(&self, context: &str, error: &dyn Error) {
        self.shell_errors
            .borrow_mut()
            .push((context.to_owned(), error.to_string()));
    }
    fn show_menu_failed_status(&self, error: &dyn Error) {
        let text = format!("Menu error: {error}");
        self.statuses.borrow_mut().push(text.clone());
        let mut env = self.rig.env.borrow_mut();
        let _ = env.vm.set_status_text(&text);
    }
    fn zoom_action_name(&self) -> String {
        self.zoom_name.borrow().clone()
    }
}

/// A scripted `PopupMenuHost`: records what it was asked to show and answers `choice` (or fails with `failure`).
pub struct ScriptedHost {
    pub choice: RefCell<Option<String>>,
    pub failure: Cell<Option<&'static str>>,
    pub shown: RefCell<Vec<Vec<MenuItemModel>>>,
    pub points: RefCell<Vec<PointD>>,
}

impl ScriptedHost {
    pub fn new(choice: Option<&str>) -> Rc<ScriptedHost> {
        Rc::new(ScriptedHost {
            choice: RefCell::new(choice.map(str::to_owned)),
            failure: Cell::new(None),
            shown: RefCell::new(Vec::new()),
            points: RefCell::new(Vec::new()),
        })
    }

    pub fn ids_of_last(&self) -> Vec<String> {
        fn flatten(items: &[MenuItemModel], out: &mut Vec<String>) {
            for item in items {
                out.push(item.id.clone());
                flatten(&item.children, out);
            }
        }
        let mut out = Vec::new();
        if let Some(last) = self.shown.borrow().last() {
            flatten(last, &mut out);
        }
        out
    }
}

impl PopupMenuHost for ScriptedHost {
    fn show(&self, items: &[MenuItemModel], screen_point_dip: PointD) -> Result<Option<String>, Box<dyn Error>> {
        self.shown.borrow_mut().push(items.to_vec());
        self.points.borrow_mut().push(screen_point_dip);
        if let Some(message) = self.failure.get() {
            return Err(Box::new(Boom(message)));
        }
        Ok(self.choice.borrow().clone())
    }
}

/// The input wiring of a rig plus the menu wiring registered on top of it, like `register_features` does (input, then the context menu).
pub struct MenuRig {
    pub rig: Rc<FeatureRig>,
    pub services: Rc<MenuServices>,
    pub menus: MainWindowMenus,
    /// Message handlers the window had before the menus were registered.
    pub handlers_before: usize,
}

impl MenuRig {
    pub fn new() -> MenuRig {
        MenuRig::over(FeatureRig::new())
    }

    /// A rig whose view model has opened a folder with `names`.
    pub fn with_images(names: &[&str]) -> MenuRig {
        let mut env = MainEnv::new();
        let (folder, _paths) = env.images("shots", names);
        env.open(&folder);
        MenuRig::over(FeatureRig::with_env(env))
    }

    pub fn over(rig: FeatureRig) -> MenuRig {
        let rig = Rc::new(rig);
        let services = Rc::new(MenuServices {
            rig: Rc::clone(&rig),
            owner: Cell::new(0),
            log: Arc::new(RecordingLog::new()),
            shell_errors: RefCell::new(Vec::new()),
            statuses: RefCell::new(Vec::new()),
            vm_calls: Cell::new(0),
            zoom_name: RefCell::new("Zoom".to_owned()),
        });
        let handlers_before = rig.window.handler_count();
        let menus = MainWindowMenus::register(
            Rc::clone(&services) as Rc<dyn MainWindowMenuServices>,
            Rc::clone(&rig.input),
        );
        MenuRig {
            rig,
            services,
            menus,
            handlers_before,
        }
    }

    /// Replaces the host of the controller by a scripted one.
    pub fn script(&self, choice: Option<&str>) -> Rc<ScriptedHost> {
        let host = ScriptedHost::new(choice);
        self.menus
            .context_menu()
            .set_menu_host(Rc::clone(&host) as Rc<dyn PopupMenuHost>);
        host
    }

    pub fn click_zoom_percent(&self) -> i32 {
        self.rig.env.borrow().settings.current().click_zoom_percent
    }

    pub fn status_text(&self) -> String {
        self.rig.env.borrow().vm.status_text()
    }
}
