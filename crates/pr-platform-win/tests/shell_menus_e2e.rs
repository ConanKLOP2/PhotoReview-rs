//! End-to-end tests of W8-14f (C# `ShellContextMenuEndToEndTests`, 18 tests, baseline `d92d6aae`): a REAL `MainWindow` (hidden, never shown or activated)
//! with its real context-menu feature, on a real UI thread. `WM_CONTEXTMENU` / `WM_RBUTTONUP` / keys are POSTED to the window (the menu is modal, so no
//! `SendMessage`); the keys of the native menu are posted BEFORE it opens (the modal loop of `TrackPopupMenuEx` reads them). A group of tests uses a
//! scripted host (an id picked without showing a menu) to run every command through the real window.
//!
//! Legacy cases and the W9-E9 popup paint are ignored native tests; `editor_command_wiring` has four pure fake-window cases.
//! Native evidence is recorded separately from default CI. Differences from the C# harness:
//! the window is the `main_window_fixture` window (WARP-free: no renderer is attached, the viewport has no image content, the decoder is a fake), the
//! zoom prompt comes through `FeatureOptions`, and the settings are written through `SettingsSource` (`begin_edit`/`commit_edit`).
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/main_window_fixture.rs"]
mod main_window_fixture;
#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}
#[path = "common/w9_e2_capture.rs"]
mod editor_menu_capture;
#[path = "common/shell_input_dispatch_fixture.rs"]
mod fixture;
#[path = "common/input_feature_fixture.rs"]
mod input_feature_fixture;
#[path = "common/ui_thread.rs"]
mod ui_thread;
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::cell::RefCell;
use std::error::Error;
use std::rc::Rc;
use std::time::{Duration, Instant};

use main_window_fixture::{BuildOptions, build, built, finish, run, start_ui, wait_until};
use pr_app::input::primitives::PointD;
use pr_app::menus::model::MenuItemModel;
use pr_app::services::contracts::ZoomPromptService;
use pr_core::settings::app_settings::AppSettings;
use pr_core::settings::context_menu::apply_hidden;
use pr_platform_win::host::contracts::{ShellWindowApi, WindowMessage, WindowMessageHandler};
use pr_platform_win::host::main_window::core::MainWindowOptions;
use pr_platform_win::shell_input::main_window::ZoomPromptOption;
use pr_platform_win::shell_menus::controller::{ContextMenuController, PopupMenuHost};
use pr_platform_win::shell_menus::main_window::MainWindowMenuFeature;
use pr_platform_win::shell_menus::message_handler::ContextMenuMessageHandler;
use pr_platform_win::win32::messages::{WM_CONTEXTMENU, WM_KEYDOWN, WM_RBUTTONDOWN, WM_RBUTTONUP};
use ui_thread::{BOUND, UiThread};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

const VK_ESCAPE: usize = 0x1B;
const VK_RETURN: usize = 0x0D;
const VK_DOWN: usize = 0x28;
const VK_RIGHT: usize = 0x27;

/// W9-E9 wiring over a real view model and fake window/services. Never executes a process.
mod editor_command_wiring {
    use super::*;
    use input_feature_fixture::{FakeServices, FeatureRig};
    use pr_app::{
        input::review_command::ReviewCommand,
        viewmodels::main::{ChangeSet, ExternalEditorFailure, ExternalEditorLauncher, MainViewModel, SettingsSource},
    };
    use pr_core::{abstractions::Log, settings::external_editors::ExternalEditor};
    use pr_platform_win::{
        shell_input::main_window::MainWindowInputServices,
        shell_menus::{
            controller::ContextMenuActions,
            main_window::{MainWindowMenuActions, MainWindowMenuServices},
        },
    };
    use std::sync::{Arc, Mutex};
    use support::main_env::MainEnv;

    struct MenuServices(Rc<FakeServices>);
    impl MainWindowMenuServices for MenuServices {
        fn window(&self) -> Rc<dyn ShellWindowApi> {
            self.0.window.clone()
        }
        fn owner_hwnd(&self) -> isize {
            0
        }
        fn log(&self) -> Arc<dyn Log> {
            self.0.log.clone()
        }
        fn settings(&self) -> SettingsSource {
            self.0.env.borrow().settings.clone()
        }
        fn with_view_model(&self, f: &mut dyn FnMut(&mut MainViewModel) -> ChangeSet) {
            MainWindowInputServices::with_view_model(self.0.as_ref(), f);
        }
        fn report_shell_error(&self, _: &str, _: &dyn Error) {}
        fn show_menu_failed_status(&self, _: &dyn Error) {}
        fn zoom_action_name(&self) -> String {
            "Zoom".into()
        }
    }
    fn actions(rig: &FeatureRig) -> MainWindowMenuActions {
        MainWindowMenuActions::new(Rc::new(MenuServices(rig.services.clone())), rig.input.clone())
    }
    type Launches = Arc<Mutex<Vec<(String, String, String)>>>;
    struct Launcher {
        calls: Launches,
        unexpected: bool,
    }
    impl ExternalEditorLauncher for Launcher {
        fn launch(&mut self, exe: &str, path: &str, arguments: &str) -> Result<(), ExternalEditorFailure> {
            self.calls
                .lock()
                .unwrap()
                .push((exe.into(), path.into(), arguments.into()));
            if self.unexpected {
                Err(ExternalEditorFailure::Unexpected {
                    message: "editor wiring bug".into(),
                })
            } else {
                Ok(())
            }
        }
    }
    fn configured(unexpected: bool) -> (FeatureRig, Launches, String, String) {
        let mut env = MainEnv::new();
        let exe = env.root.join("declared-editor.exe").to_string_lossy().into_owned();
        std::fs::write(&exe, b"placeholder; fake launcher only").unwrap();
        env.update_settings(|settings| {
            settings.external_editors = vec![ExternalEditor {
                id: "chosen.id with spaces".into(),
                name: "Chosen".into(),
                path: exe.clone(),
                arguments: "--chosen {file}".into(),
            }];
            settings.default_external_editor_id = "chosen.id with spaces".into();
            settings.external_editor_path = "stale-mirror.exe".into();
        });
        let (folder, files) = env.images("editor-shell", &["photo.jpg"]);
        env.open(&folder);
        let calls = Arc::new(Mutex::new(Vec::new()));
        env.vm.set_external_editor_launcher(Box::new(Launcher {
            calls: calls.clone(),
            unexpected,
        }));
        (FeatureRig::with_env(env), calls, exe, files[0].clone())
    }

    #[test]
    fn input_default_editor_and_menu_by_id_reach_real_view_model_with_template() {
        let (rig, calls, exe, photo) = configured(false);
        rig.input
            .input_host()
            .execute(ReviewCommand::OPEN_IN_DEFAULT_EDITOR)
            .unwrap();
        actions(&rig)
            .open_in_external_editor_by_id("chosen.id with spaces")
            .unwrap();
        let launched = calls.lock().unwrap();
        assert_eq!(launched.len(), 2);
        let expected_photo = std::fs::canonicalize(photo).unwrap();
        for (program, path, arguments) in launched.iter() {
            assert_eq!(program, &exe); // The configured string is forwarded verbatim.
            // CI may expand a RUNNER~1 temp-root prefix. Check the stable tail and exact file identity.
            assert!(pr_core::text::eq_ignore_case(
                pr_core::text::dotnet_path::get_file_name(path),
                "photo.jpg"
            ));
            assert_eq!(std::fs::canonicalize(path).unwrap(), expected_photo);
            assert_eq!(arguments, &format!("--chosen \"{path}\""));
        }
        assert_eq!(rig.vm_calls(), 2);
    }

    #[test]
    fn input_default_editor_without_configuration_opens_editor_settings() {
        let rig = FeatureRig::new();
        rig.input
            .input_host()
            .execute(ReviewCommand::OPEN_IN_DEFAULT_EDITOR)
            .unwrap();
        let env = rig.env.borrow();
        assert_eq!(
            *env.dialog.settings_targets.lock().unwrap(),
            vec![pr_core::abstractions::SettingsTarget::ExternalEditor]
        );
        assert!(env.dialog.errors.lock().unwrap().is_empty());
    }

    #[test]
    fn menu_and_input_return_unexpected_editor_error_to_caller() {
        let (rig, _, _, _) = configured(true);
        let error = rig
            .input
            .input_host()
            .execute(ReviewCommand::OPEN_IN_DEFAULT_EDITOR)
            .unwrap_err();
        assert_eq!(error.to_string(), "editor wiring bug");
        let error = actions(&rig)
            .open_in_external_editor_by_id("chosen.id with spaces")
            .unwrap_err();
        assert_eq!(error.to_string(), "editor wiring bug");
        assert!(rig.env.borrow().dialog.errors.lock().unwrap().is_empty());
    }

    #[test]
    fn menu_add_and_choose_default_reach_dialog_and_persist_selection() {
        let rig = FeatureRig::new();
        *rig.env.borrow().dialog.added_editor.lock().unwrap() = Some(ExternalEditor {
            id: "dialog-id".into(),
            name: "Added".into(),
            path: r"C:\fake-editor.exe".into(),
            arguments: ExternalEditor::DEFAULT_ARGUMENTS.into(),
        });
        let actions = actions(&rig);
        actions.add_external_editor().unwrap();
        let added = rig.env.borrow().settings.current().external_editors[0].clone();
        assert_ne!(added.id, "dialog-id");
        *rig.env.borrow().dialog.chosen_editor.lock().unwrap() = Some(added.id.clone());
        actions.choose_default_external_editor().unwrap();
        let env = rig.env.borrow();
        assert_eq!(env.settings.current().default_external_editor_id, added.id);
        assert_eq!(env.dialog.editor_initials.lock().unwrap().len(), 1);
        assert_eq!(env.dialog.editor_choices.lock().unwrap().len(), 1);
    }
}

/// W9-E9: event-driven twin of ExternalEditorsUiTests.Menu_EditorSubmenu_PaintsItsEntries.
/// WM_ENTERIDLE marks a painted menu with an empty input queue; no fixed paint delays.
mod editor_popup_paint {
    use super::*;
    use pr_app::menus::{
        builder,
        model::{ContextMenuBuildContext, MenuItemKind},
    };
    use pr_core::settings::{
        app_settings::EnglishSettingsTexts, context_menu::ContextMenuItemId, external_editors::ExternalEditor,
    };
    use pr_l10n::{catalog::LanguageCatalog, localizer::Localizer};
    use pr_platform_win::{
        shell_menus::{
            mapping,
            owner_draw::{OwnerDrawItem, OwnerDrawMenuSession},
        },
        win32::{api, messages, structs::Hmenu},
    };
    use std::any::Any;
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
    use std::sync::{Arc, mpsc};
    use windows::{
        Win32::{
            Graphics::Gdi::HBRUSH,
            UI::WindowsAndMessaging::{AppendMenuW, HMENU, MENU_ITEM_FLAGS, MENUINFO, MENUINFO_MASK, SetMenuInfo},
        },
        core::PCWSTR,
    };

    struct Menu(Hmenu);
    impl Drop for Menu {
        fn drop(&mut self) {
            let _ = api::user32::destroy_menu(self.0);
        }
    }
    struct RestoreLocalizer(Arc<Localizer>);
    impl Drop for RestoreLocalizer {
        fn drop(&mut self) {
            Localizer::set_current(self.0.clone());
        }
    }
    fn post(owner: isize, message: u32, key: usize) {
        // SAFETY: posts integer-only messages to the live test-owned owner; no borrowed pointer crosses threads.
        unsafe {
            PostMessageW(Some(HWND(owner as *mut _)), message, WPARAM(key), LPARAM(0)).unwrap();
        }
    }

    struct IdleCapture {
        dpi: u32,
        editor_index: usize,
        root: Cell<isize>,
        completed: Cell<bool>,
        failure: RefCell<Option<Box<dyn Any + Send>>>,
    }
    // SAFETY: `DwmFlush` is a plain Win32 call with no arguments or pointers; it only blocks until the next DWM composition pass.
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmFlush() -> i32;
    }
    fn flush_compositor(hwnd: isize) {
        use windows::Win32::Graphics::Gdi::{RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow};
        // SAFETY: both flush calls take no pointers; synchronize completed GDI draws and the compositor before reading the real window DC.
        unsafe {
            // SAFETY: the live popup belongs to this UI thread; synchronous paint completes before its real DC is captured.
            assert!(
                RedrawWindow(
                    Some(HWND(hwnd as *mut _)),
                    None,
                    None,
                    RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN
                )
                .as_bool()
            );
            assert!(windows::Win32::Graphics::Gdi::GdiFlush().as_bool());
            assert!(DwmFlush() >= 0, "DWM flush failed");
        }
    }
    impl IdleCapture {
        fn idle(&self, message: &WindowMessage) {
            if self.root.get() == 0 {
                self.root.set(message.lparam);
                flush_compositor(message.lparam);
                let shot = editor_menu_capture::Shot::capture(message.lparam);
                shot.assert_not_flat(message.lparam, "editor root menu");
                for _ in 0..=self.editor_index {
                    post(message.hwnd, WM_KEYDOWN, VK_DOWN);
                }
                post(message.hwnd, WM_KEYDOWN, VK_RIGHT);
                post(message.hwnd, WM_KEYDOWN, VK_DOWN);
            } else if message.lparam != self.root.get() && !self.completed.get() {
                flush_compositor(message.lparam);
                let shot = editor_menu_capture::Shot::capture(message.lparam);
                shot.assert_not_flat(message.lparam, "editor submenu");
                let rect = api::user32::get_window_rect(message.lparam).unwrap();
                assert!(
                    rect.bottom - rect.top > 4 * (16 * self.dpi / 96) as i32,
                    "submenu height at DPI {}",
                    self.dpi
                );
                self.completed.set(true);
                post(message.hwnd, WM_KEYDOWN, VK_ESCAPE);
                post(message.hwnd, WM_KEYDOWN, VK_ESCAPE);
            }
        }
    }
    impl WindowMessageHandler for IdleCapture {
        fn try_handle(&self, message: &WindowMessage) -> Option<isize> {
            if message.msg != messages::WM_ENTERIDLE || message.wparam != 2 || message.lparam == 0 {
                return None;
            }
            if let Err(error) = catch_unwind(AssertUnwindSafe(|| self.idle(message))) {
                *self.failure.borrow_mut() = Some(error);
                post(message.hwnd, windows::Win32::UI::WindowsAndMessaging::WM_CANCELMODE, 0);
            }
            Some(0)
        }
    }

    // Test-side C# Populate: real HMENUs and the production OwnerDrawMenuSession, at an explicit test DPI.
    fn populate(menu: Hmenu, items: &[MenuItemModel], session: &OwnerDrawMenuSession, next_id: &mut usize) {
        for item in items {
            let separator = item.kind == MenuItemKind::Separator;
            let submenu = item.kind == MenuItemKind::Submenu && !item.children.is_empty();
            let mut child = if submenu {
                Some(Menu(api::user32::create_popup_menu().unwrap()))
            } else {
                None
            };
            if let Some(child) = &child {
                populate(child.0, &item.children, session, next_id);
            }
            let id = if let Some(child) = &child {
                child.0 as usize
            } else if separator {
                0
            } else {
                *next_id += 1;
                *next_id
            };
            let data = session.register(OwnerDrawItem {
                text: item.text.clone(),
                shortcut: item.shortcut_text.clone(),
                is_separator: separator,
                is_submenu: submenu,
            });
            let flags = mapping::item_flags(item)
                | 0x100
                | if separator {
                    messages::MF_SEPARATOR
                } else if submenu {
                    messages::MF_POPUP
                } else {
                    0
                };
            // SAFETY: MF_OWNERDRAW treats data as an integer token; session owns its item until the menu is destroyed.
            unsafe {
                AppendMenuW(
                    HMENU(menu as *mut _),
                    MENU_ITEM_FLAGS(flags),
                    id,
                    PCWSTR(data as *const u16),
                )
                .unwrap();
            }
            if let Some(child) = child.take() {
                std::mem::forget(child);
            } // Root now owns the appended child.
        }
    }

    fn paint(dpi: u32, language: &'static str) {
        let rig = start(&[], None);
        run(&rig.ui, move || {
            let _restore = RestoreLocalizer(Localizer::current());
            let mut warnings = Vec::new();
            let english = LanguageCatalog::try_parse(
                include_str!("../../../assets/languages/en.json"),
                "en.json",
                &mut warnings,
            )
            .unwrap();
            let overlays = if language == "vi" {
                vec![
                    LanguageCatalog::try_parse(
                        include_str!("../../../assets/languages/vi.json"),
                        "vi.json",
                        &mut warnings,
                    )
                    .unwrap(),
                ]
            } else {
                Vec::new()
            };
            Localizer::set_current(Arc::new(Localizer::create(&english, &overlays, &warnings)));
            let mut settings = AppSettings::new(&EnglishSettingsTexts);
            settings.external_editors = ["First editor", "Second editor", "Third editor"]
                .into_iter()
                .enumerate()
                .map(|(index, name)| ExternalEditor {
                    id: format!("e{index}"),
                    name: name.into(),
                    path: format!(r"C:\test-editor{index}.exe"),
                    arguments: ExternalEditor::DEFAULT_ARGUMENTS.into(),
                })
                .collect();
            settings.default_external_editor_id = "e0".into();
            apply_hidden(&mut settings, [ContextMenuItemId::RenameFile]);
            let items = builder::build(&ContextMenuBuildContext {
                settings: &settings,
                has_image: true,
                can_undo: true,
                is_fit: true,
                effective_zoom: None,
                can_open_in_external_editor: true,
                can_copy_path: true,
                is_compare_visible: false,
            })
            .unwrap();
            let top = items
                .iter()
                .filter(|item| item.kind != MenuItemKind::Separator)
                .collect::<Vec<_>>();
            let editor_index = top.iter().position(|item| item.id == "ExternalEditor").unwrap();
            assert_eq!(top[editor_index].kind, MenuItemKind::Submenu);
            assert_eq!(top[editor_index].children.len(), 6);
            let owner = built().shell.hwnd();
            let session = OwnerDrawMenuSession::new(owner, dpi, None);
            assert!(session.attach());
            let root = Menu(api::user32::create_popup_menu().unwrap());
            populate(root.0, &items, &session, &mut 0);
            let info = MENUINFO {
                cbSize: std::mem::size_of::<MENUINFO>() as u32,
                fMask: MENUINFO_MASK(0x8000_0002),
                hbrBack: HBRUSH(session.background_brush() as *mut _),
                ..Default::default()
            };
            // SAFETY: root and session's brush are live until after TrackPopupMenuEx and menu destruction.
            unsafe {
                SetMenuInfo(HMENU(root.0 as *mut _), &info).unwrap();
            }
            let capture = Rc::new(IdleCapture {
                dpi,
                editor_index,
                root: Cell::new(0),
                completed: Cell::new(false),
                failure: RefCell::new(None),
            });
            built().shell.insert_message_handler_first(capture.clone());
            let (done, receiver) = mpsc::channel();
            let watchdog = std::thread::spawn(move || {
                if receiver.recv_timeout(Duration::from_secs(15)).is_err() {
                    post(owner, windows::Win32::UI::WindowsAndMessaging::WM_CANCELMODE, 0);
                    post(owner, WM_KEYDOWN, VK_ESCAPE);
                    true
                } else {
                    false
                }
            });
            // Test-only: bypass popup animation so completed GDI/DWM paint is the visible frame.
            api::user32::track_popup_menu_ex(
                root.0,
                messages::TPM_RETURNCMD | windows::Win32::UI::WindowsAndMessaging::TPM_NOANIMATION.0,
                150,
                150,
                owner,
            );
            let _ = done.send(());
            let timed_out = watchdog.join().unwrap();
            drop(root);
            session.dispose();
            if let Some(error) = capture.failure.borrow_mut().take() {
                resume_unwind(error);
            }
            assert!(!timed_out, "menu idle/capture timed out: {language} {dpi}");
            assert!(
                capture.completed.get(),
                "root and submenu were not both captured: {language} {dpi}"
            );
            eprintln!("BitBlt root+submenu PASS: {language} {dpi}");
        });
        rig.finish();
    }

    // C#: ExternalEditorsUiTests.Menu_EditorSubmenu_PaintsItsEntries, all four theory rows.
    #[test]
    #[ignore = "native: needs interactive desktop for real popup BitBlt"]
    fn menu_editor_submenu_paints_its_entries() {
        for language in ["en", "vi"] {
            for dpi in [96, 144] {
                paint(dpi, language);
            }
        }
    }
}

// ---- the observing host ---------------------------------------------------------------------------------------------------------------------------

/// C# `SpyMenuHost`: records the model, the anchor point and the result; answers the scripted id (no menu) or asks the inner host.
struct Spy {
    inner: Rc<dyn PopupMenuHost>,
    scripted: RefCell<Option<String>>,
    shown: RefCell<Vec<Vec<MenuItemModel>>>,
    points: RefCell<Vec<PointD>>,
    results: RefCell<Vec<Option<String>>>,
}

impl PopupMenuHost for Spy {
    fn show(&self, items: &[MenuItemModel], screen_point_dip: PointD) -> Result<Option<String>, Box<dyn Error>> {
        self.shown.borrow_mut().push(items.to_vec());
        self.points.borrow_mut().push(screen_point_dip);
        let scripted = self.scripted.borrow().clone();
        // No borrow is held while the (modal) inner host runs.
        let result = match scripted {
            Some(id) => Some(id),
            None => self.inner.show(items, screen_point_dip)?,
        };
        self.results.borrow_mut().push(result.clone());
        Ok(result)
    }
}

/// C# `NullHost`: never shows anything (answers `None`).
struct NullHost;

impl PopupMenuHost for NullHost {
    fn show(&self, _items: &[MenuItemModel], _point: PointD) -> Result<Option<String>, Box<dyn Error>> {
        Ok(None)
    }
}

/// C# `FakeZoomPrompt`.
struct FakePrompt {
    answer: Option<i32>,
    asked: RefCell<Vec<i32>>,
}

impl ZoomPromptService for FakePrompt {
    fn prompt_custom_zoom(&self, current_percent: i32) -> Option<i32> {
        self.asked.borrow_mut().push(current_percent);
        self.answer
    }
}

thread_local! {
    static SPY: RefCell<Option<Rc<Spy>>> = const { RefCell::new(None) };
    static PROMPT: RefCell<Option<Rc<FakePrompt>>> = const { RefCell::new(None) };
}

fn spy() -> Rc<Spy> {
    SPY.with(|slot| slot.borrow().clone()).expect("install_spy() first")
}

// ---- UI-thread helpers (call inside `run`) ---------------------------------------------------------------------------------------------------------

/// Runs the queued jobs, pumps the view model and applies its changes, until nothing is outstanding (bounded).
fn settle() {
    let b = built();
    let deadline = Instant::now() + BOUND;
    loop {
        let mut progressed = false;
        while b.runner.position_not_in(&[]).is_some() {
            if let Some(index) = b.runner.position_not_in(&[]) {
                progressed |= b.runner.run_at(index);
            }
        }
        if b.scheduler.run_pending() > 0 {
            progressed = true;
        }
        let changes = b.window.view_model().borrow_mut().pump();
        if !changes.is_empty() {
            b.window.apply_changes(&changes);
        }
        let idle = {
            let vm = b.window.view_model().borrow();
            vm.is_idle() && b.runner.pending() == 0 && b.scheduler.pending() == 0
        };
        if idle && !progressed {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the view model to settle"
        );
        if !progressed {
            b.window
                .view_model()
                .borrow()
                .presenter()
                .wait_for_event(Duration::from_millis(20));
        }
    }
}

/// Writes `folders` under the window's photo root (one fake image each) and opens the first one.
fn open_folders(folders: &[&str]) {
    let b = built();
    let lib = b.photos.join("lib");
    for folder in folders {
        let dir = lib.join(folder);
        std::fs::create_dir_all(&dir).expect("create the folder");
        std::fs::write(dir.join("image.jpg"), [0xFF, 0xD8, 0xFF, 0xD9]).expect("write the fake image");
    }
    if let Some(first) = folders.first() {
        let path = lib.join(first).to_string_lossy().into_owned();
        let changes = b
            .window
            .view_model()
            .borrow_mut()
            .open_folder(&path, None)
            .expect("open the folder");
        b.window.apply_changes(&changes);
        settle();
    }
}

fn install_spy(scripted: Option<&str>) {
    let spy = built()
        .window
        .with_feature::<MainWindowMenuFeature, _>(|feature| {
            let controller = feature.menus().context_menu();
            let spy = Rc::new(Spy {
                inner: controller.menu_host(),
                scripted: RefCell::new(scripted.map(str::to_owned)),
                shown: RefCell::new(Vec::new()),
                points: RefCell::new(Vec::new()),
                results: RefCell::new(Vec::new()),
            });
            controller.set_menu_host(Rc::clone(&spy) as Rc<dyn PopupMenuHost>);
            spy
        })
        .expect("the context-menu feature is registered");
    SPY.with(|slot| *slot.borrow_mut() = Some(spy));
}

fn controller() -> Rc<ContextMenuController> {
    built()
        .window
        .with_feature::<MainWindowMenuFeature, _>(|feature| Rc::clone(feature.menus().context_menu()))
        .expect("the context-menu feature is registered")
}

fn settings() -> Rc<AppSettings> {
    let current = built().window.view_model().borrow().settings_source().current();
    Rc::new((*current).clone())
}

fn edit(mutate: impl FnOnce(&mut AppSettings)) {
    let source = built().window.view_model().borrow().settings_source();
    let edit = source.begin_edit(mutate);
    source.commit_edit(edit).expect("save the setting");
}

fn enable_every_menu_item() {
    edit(|s| apply_hidden(s, std::iter::empty()));
}

fn zoom_percent() -> i32 {
    built()
        .window
        .view_model()
        .borrow()
        .viewer()
        .borrow()
        .display_zoom_percent()
}

fn is_fit() -> bool {
    built().window.view_model().borrow().viewer().borrow().is_fit()
}

// ---- the harness ------------------------------------------------------------------------------------------------------------------------------

/// A running window: the UI thread plus the handle the tests post to.
struct Rig {
    ui: UiThread,
    hwnd: isize,
}

/// C# `Start`: a hidden main window; `folders` get one image each and the first is opened. `prompt`: `Some(answer)` installs a zoom prompt.
fn start(folders: &[&'static str], prompt: Option<Option<i32>>) -> Rig {
    let ui = start_ui();
    let folders: Vec<&'static str> = folders.to_vec();
    let hwnd = run(&ui, move || {
        let mut options = BuildOptions::default();
        let main_options: &mut MainWindowOptions = &mut options.options;
        if let Some(answer) = prompt {
            let fake = Rc::new(FakePrompt {
                answer,
                asked: RefCell::new(Vec::new()),
            });
            main_options
                .extensions
                .insert(ZoomPromptOption(Rc::clone(&fake) as Rc<dyn ZoomPromptService>));
            PROMPT.with(|slot| *slot.borrow_mut() = Some(fake));
        }
        let b = build(options);
        open_folders(&folders);
        b.shell.hwnd()
    });
    Rig { ui, hwnd }
}

impl Rig {
    fn post(&self, message: u32, wparam: usize, lparam: isize) {
        // SAFETY: a plain message post to a window of this process; the OS validates the handle.
        unsafe {
            PostMessageW(
                Some(HWND(self.hwnd as *mut core::ffi::c_void)),
                message,
                WPARAM(wparam),
                LPARAM(lparam),
            )
        }
        .expect("post a message to the main window");
    }

    fn post_keys(&self, keys: &[usize]) {
        for key in keys {
            self.post(WM_KEYDOWN, *key, 1);
        }
    }

    fn client_center_pixels(&self) -> (i32, i32) {
        run(&self.ui, || {
            let (w, h) = built().shell.client_size_pixels();
            (w / 2, h / 2)
        })
    }

    fn dpi(&self) -> f64 {
        run(&self.ui, || built().shell.dpi_scale())
    }

    fn screen_of(&self, client_x: i32, client_y: i32) -> (i32, i32) {
        let mut point = POINT {
            x: client_x,
            y: client_y,
        };
        // SAFETY: `point` is a live local the OS updates in place; the window handle is validated by the OS.
        assert!(unsafe { ClientToScreen(HWND(self.hwnd as *mut core::ffi::c_void), &mut point) }.as_bool());
        (point.x, point.y)
    }

    fn post_context_menu(&self, client_x: i32, client_y: i32) {
        let (sx, sy) = self.screen_of(client_x, client_y);
        self.post(WM_CONTEXTMENU, self.hwnd as usize, pack(sx, sy));
    }

    fn results(&self) -> usize {
        run(&self.ui, || spy().results.borrow().len())
    }

    fn shown(&self) -> usize {
        run(&self.ui, || spy().shown.borrow().len())
    }

    /// C# `Choose`: scripts `id`, opens the menu at the client centre and waits for the result.
    fn choose(&self, id: &'static str) {
        let before = self.results();
        run(&self.ui, move || *spy().scripted.borrow_mut() = Some(id.to_owned()));
        let (cx, cy) = self.client_center_pixels();
        self.post_context_menu(cx, cy);
        wait_until(&self.ui, id, move || spy().results.borrow().len() == before + 1);
    }

    fn finish(self) {
        finish(self.ui);
    }
}

fn pack(x: i32, y: i32) -> isize {
    (((y as i16 as u16 as usize) << 16) | (x as i16 as u16 as usize)) as isize
}

fn ids_of(items: &[MenuItemModel]) -> Vec<String> {
    items.iter().map(|item| item.id.clone()).collect()
}

fn first_shown_ids() -> Vec<String> {
    ids_of(&spy().shown.borrow()[0])
}

// C#7eaee2a2 ShellContextMenuEndToEndTests includes RenameFile after Separator.5.
const DEFAULT_IMAGE_MENU: [&str; 15] = [
    "Undo",
    "Separator.2",
    "Refresh",
    "Separator.3",
    "OpenFolder",
    "NextFolder",
    "PreviousFolder",
    "Separator.4",
    "ExternalEditor",
    "Separator.5",
    "RenameFile",
    "CopyFileName",
    "CopyFullPath",
    "Separator.6",
    "Settings",
];

// ---- showing the menu --------------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs desktop"]
fn wm_context_menu_opens_the_menu_built_from_the_live_settings_at_the_click_point_in_dip() {
    let rig = start(&["a-photos"], None);
    run(&rig.ui, null_spy);
    let (cx, cy) = rig.client_center_pixels();
    let (sx, sy) = rig.screen_of(cx, cy);
    let dpi = rig.dpi();
    rig.post_context_menu(cx, cy);
    wait_until(&rig.ui, "the menu opens", || spy().shown.borrow().len() == 1);

    let (point, ids) = run(&rig.ui, || (spy().points.borrow()[0], first_shown_ids()));
    assert!(
        (point.x - f64::from(sx) / dpi).abs() < 0.001,
        "x {} vs {}",
        point.x,
        f64::from(sx) / dpi
    );
    assert!(
        (point.y - f64::from(sy) / dpi).abs() < 0.001,
        "y {} vs {}",
        point.y,
        f64::from(sy) / dpi
    );
    assert_eq!(ids, DEFAULT_IMAGE_MENU);
    rig.finish();
}

fn null_spy() {
    let again = Rc::new(Spy {
        inner: Rc::new(NullHost) as Rc<dyn PopupMenuHost>,
        scripted: RefCell::new(None),
        shown: RefCell::new(Vec::new()),
        points: RefCell::new(Vec::new()),
        results: RefCell::new(Vec::new()),
    });
    controller().set_menu_host(Rc::clone(&again) as Rc<dyn PopupMenuHost>);
    SPY.with(|slot| *slot.borrow_mut() = Some(again));
}

#[test]
#[ignore = "native: needs desktop"]
fn wm_rbutton_up_is_turned_into_the_menu_by_def_window_proc() {
    let rig = start(&["a-photos"], None);
    run(&rig.ui, null_spy);
    let (cx, cy) = rig.client_center_pixels();
    rig.post(WM_RBUTTONDOWN, 0x0002, pack(cx, cy));
    rig.post(WM_RBUTTONUP, 0, pack(cx, cy));
    wait_until(&rig.ui, "releasing the right button opens the menu", || {
        spy().shown.borrow().len() == 1
    });
    rig.finish();
}

#[test]
#[ignore = "native: needs desktop"]
fn keyboard_invocation_lparam_minus_one_opens_at_the_client_centre() {
    let rig = start(&["a-photos"], None);
    run(&rig.ui, null_spy);
    let (cx, cy) = rig.client_center_pixels();
    let (sx, sy) = rig.screen_of(cx, cy);
    let dpi = rig.dpi();
    rig.post(WM_CONTEXTMENU, rig.hwnd as usize, pack(-1, -1));
    wait_until(&rig.ui, "the keyboard invocation opens the menu", || {
        spy().shown.borrow().len() == 1
    });
    let point = run(&rig.ui, || spy().points.borrow()[0]);
    // The centre in whole pixels: at most one pixel of rounding of the halving.
    assert!(
        (point.x * dpi - f64::from(sx)).abs() <= 1.5,
        "x {} vs {sx}",
        point.x * dpi
    );
    assert!(
        (point.y * dpi - f64::from(sy)).abs() <= 1.5,
        "y {} vs {sy}",
        point.y * dpi
    );
    rig.finish();
}

#[test]
#[ignore = "native: needs desktop"]
fn right_click_outside_the_client_area_does_not_open_the_app_menu() {
    let rig = start(&["a-photos"], None);
    run(&rig.ui, null_spy);
    let (ox, oy) = rig.screen_of(0, 0);
    let hwnd = rig.hwnd;
    let handled = run(&rig.ui, move || {
        let window = Rc::clone(&built().shell) as Rc<dyn ShellWindowApi>;
        let handler = ContextMenuMessageHandler::new(window, controller());
        // 40 px above the client origin (the title bar) is not client.
        handler
            .try_handle(&WindowMessage {
                hwnd,
                msg: WM_CONTEXTMENU,
                wparam: hwnd as usize,
                lparam: pack(ox + 50, oy - 40),
            })
            .is_some()
    });
    assert!(!handled);
    assert_eq!(rig.shown(), 0);
    rig.finish();
}

// ---- the real native menu ----------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs desktop"]
fn native_menu_escape_closes_the_menu_runs_nothing_and_keeps_the_window_alive() {
    let rig = start(&["a-photos"], None);
    run(&rig.ui, || install_spy(None));
    let (cx, cy) = rig.client_center_pixels();
    rig.post_context_menu(cx, cy);
    rig.post_keys(&[VK_ESCAPE]);
    wait_until(&rig.ui, "the menu closes", || spy().results.borrow().len() == 1);
    assert_eq!(run(&rig.ui, || spy().results.borrow()[0].clone()), None);
    assert!(!run(&rig.ui, || controller().is_open()));
    assert!(
        run(&rig.ui, || !built().shell.is_destroyed()),
        "Esc of the menu must not close the window"
    );
    rig.finish();
}

#[test]
#[ignore = "native: needs desktop"]
fn native_menu_arrows_and_enter_run_the_chosen_command_next_folder() {
    let rig = start(&["a-photos", "b-photos"], None);
    run(&rig.ui, || install_spy(None));
    let (cx, cy) = rig.client_center_pixels();
    assert!(run(&rig.ui, || built()
        .window
        .view_model()
        .borrow()
        .folder_text()
        .contains("a-photos")));
    rig.post_context_menu(cx, cy);
    // Default: Undo, Refresh, Open folder, Next folder -> four Down presses.
    rig.post_keys(&[VK_DOWN, VK_DOWN, VK_DOWN, VK_DOWN, VK_RETURN]);
    wait_until(&rig.ui, "the menu returns", || spy().results.borrow().len() == 1);
    assert_eq!(
        run(&rig.ui, || spy().results.borrow()[0].clone()).as_deref(),
        Some("NextFolder")
    );
    wait_until(&rig.ui, "Next folder moved to the next folder", || {
        settle();
        built().window.view_model().borrow().folder_text().contains("b-photos")
    });
    rig.finish();
}

#[test]
#[ignore = "native: needs desktop"]
fn native_menu_zoom_submenu_preset_from_the_submenu_zooms_and_saves_the_click_level() {
    let rig = start(&["a-photos"], None);
    run(&rig.ui, || {
        enable_every_menu_item();
        install_spy(None);
    });
    let (cx, cy) = rig.client_center_pixels();
    rig.post_context_menu(cx, cy);
    // Undo, Recycle, Fit, Zoom to N %, Zoom > : five Down presses to Zoom, Right opens the submenu (first item selected); in it
    // Fit width, Fit width 2, Fit height, 50, 70, 100, 150, 200: seven more Down presses reach the 200 preset.
    rig.post_keys(&[
        VK_DOWN, VK_DOWN, VK_DOWN, VK_DOWN, VK_DOWN, VK_RIGHT, VK_DOWN, VK_DOWN, VK_DOWN, VK_DOWN, VK_DOWN, VK_DOWN,
        VK_DOWN, VK_RETURN,
    ]);
    wait_until(&rig.ui, "the menu returns", || spy().results.borrow().len() == 1);
    assert_eq!(
        run(&rig.ui, || spy().results.borrow()[0].clone()).as_deref(),
        Some("Zoom.Preset.200")
    );
    wait_until(&rig.ui, "preset 200 zooms", || {
        settle();
        zoom_percent() == 200
    });
    assert_eq!(run(&rig.ui, || settings().click_zoom_percent), 200);
    rig.finish();
}

// ---- commands by id, through the real window -----------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs desktop"]
fn zoom_to_level_zooms_to_the_configured_level_without_changing_it() {
    let rig = start(&["a-photos"], None);
    run(&rig.ui, || {
        enable_every_menu_item();
        edit(|s| s.click_zoom_percent = 150);
        install_spy(Some("ZoomToLevel"));
    });
    rig.choose("ZoomToLevel");
    wait_until(&rig.ui, "zoomed to 150", || {
        settle();
        zoom_percent() == 150
    });
    assert_eq!(run(&rig.ui, || settings().click_zoom_percent), 150);
    rig.finish();
}

fn zoom_preset_case(also_set: bool, expected_click_level: i32) {
    let rig = start(&["a-photos"], None);
    run(&rig.ui, move || {
        enable_every_menu_item();
        edit(|s| s.set_zoom_also_sets_click_level = also_set);
        install_spy(Some("Zoom.Preset.300"));
    });
    rig.choose("Zoom.Preset.300");
    wait_until(&rig.ui, "zoomed to 300", || {
        settle();
        zoom_percent() == 300
    });
    assert_eq!(run(&rig.ui, || settings().click_zoom_percent), expected_click_level);
    rig.finish();
}

#[test]
#[ignore = "native: needs desktop"]
fn zoom_preset_saves_the_click_level_when_also_set_is_on() {
    zoom_preset_case(true, 300);
}

#[test]
#[ignore = "native: needs desktop"]
fn zoom_preset_does_not_save_the_click_level_when_also_set_is_off() {
    zoom_preset_case(false, 100);
}

#[test]
#[ignore = "native: needs desktop"]
fn custom_zoom_asks_the_prompt_with_the_current_level_and_applies_the_answer() {
    let rig = start(&["a-photos"], Some(Some(250)));
    run(&rig.ui, || {
        enable_every_menu_item();
        install_spy(Some("Zoom.Custom"));
    });
    rig.choose("Zoom.Custom");
    wait_until(&rig.ui, "the custom level is applied", || {
        settle();
        zoom_percent() == 250
    });
    assert_eq!(
        run(&rig.ui, || PROMPT
            .with(|p| p.borrow().as_ref().map(|p| p.asked.borrow().clone()))),
        Some(vec![100])
    );
    assert_eq!(run(&rig.ui, || settings().click_zoom_percent), 250);
    rig.finish();
}

#[test]
#[ignore = "native: needs desktop"]
fn custom_zoom_cancelled_prompt_changes_nothing() {
    let rig = start(&["a-photos"], Some(None));
    run(&rig.ui, || {
        enable_every_menu_item();
        install_spy(Some("Zoom.Custom"));
    });
    rig.choose("Zoom.Custom");
    wait_until(&rig.ui, "the prompt was asked", || {
        PROMPT.with(|p| p.borrow().as_ref().is_some_and(|p| p.asked.borrow().len() == 1))
    });
    assert!(run(&rig.ui, is_fit));
    assert_eq!(run(&rig.ui, || settings().click_zoom_percent), 100);
    rig.finish();
}

#[test]
#[ignore = "native: needs desktop"]
fn set_current_as_click_level_saves_the_current_zoom_and_is_disabled_in_fit() {
    let rig = start(&["a-photos"], None);
    run(&rig.ui, || {
        enable_every_menu_item();
        install_spy(Some("Zoom.SetCurrentAsClickLevel"));
    });
    let (cx, cy) = rig.client_center_pixels();
    // In Fit the item is disabled in the model; choosing it (scripted) changes nothing either.
    rig.post_context_menu(cx, cy);
    wait_until(&rig.ui, "the menu returns", || spy().results.borrow().len() == 1);
    let enabled = run(&rig.ui, || {
        let shown = spy().shown.borrow()[0].clone();
        shown
            .iter()
            .find(|item| item.id == "ZoomSubmenu")
            .and_then(|zoom| {
                zoom.children
                    .iter()
                    .find(|child| child.id == "Zoom.SetCurrentAsClickLevel")
            })
            .map(|child| child.is_enabled)
    });
    assert_eq!(enabled, Some(false));
    assert_eq!(run(&rig.ui, || settings().click_zoom_percent), 100);

    run(&rig.ui, || {
        let _ = built()
            .window
            .view_model()
            .borrow()
            .viewer()
            .borrow_mut()
            .set_zoom(1.75);
    });
    rig.choose("Zoom.SetCurrentAsClickLevel");
    wait_until(&rig.ui, "the click level is 175", || {
        settings().click_zoom_percent == 175
    });
    let zoom = run(&rig.ui, || {
        built().window.view_model().borrow().viewer().borrow().zoom()
    });
    assert!((zoom - 1.75).abs() < 1e-6, "no zoom again: {zoom}");
    rig.finish();
}

#[test]
#[ignore = "native: needs desktop"]
fn also_set_click_level_flips_and_persists_the_setting() {
    let rig = start(&["a-photos"], None);
    run(&rig.ui, || {
        enable_every_menu_item();
        install_spy(Some("Zoom.AlsoSetClickLevel"));
    });
    assert!(run(&rig.ui, || settings().set_zoom_also_sets_click_level));
    rig.choose("Zoom.AlsoSetClickLevel");
    wait_until(&rig.ui, "flipped off", || !settings().set_zoom_also_sets_click_level);
    rig.choose("Zoom.AlsoSetClickLevel");
    wait_until(&rig.ui, "flipped on again", || {
        settings().set_zoom_also_sets_click_level
    });
    rig.finish();
}

#[test]
#[ignore = "native: needs desktop"]
fn fit_returns_to_fit_from_a_zoomed_view() {
    let rig = start(&["a-photos"], None);
    run(&rig.ui, || {
        enable_every_menu_item();
        let _ = built().window.view_model().borrow().viewer().borrow_mut().set_zoom(2.0);
        install_spy(Some("Fit"));
    });
    assert!(!run(&rig.ui, is_fit));
    rig.choose("Fit");
    wait_until(&rig.ui, "back to Fit", || {
        settle();
        is_fit()
    });
    rig.finish();
}

#[test]
#[ignore = "native: needs desktop"]
fn no_folder_open_the_menu_still_opens_without_the_copy_items() {
    let rig = start(&[], None);
    run(&rig.ui, null_spy);
    let (cx, cy) = rig.client_center_pixels();
    rig.post_context_menu(cx, cy);
    wait_until(&rig.ui, "the menu opens without a folder", || {
        spy().shown.borrow().len() == 1
    });
    let ids = run(&rig.ui, first_shown_ids);
    assert_eq!(
        ids,
        [
            "Undo",
            "Separator.2",
            "Refresh",
            "Separator.3",
            "OpenFolder",
            "NextFolder",
            "PreviousFolder",
            "Separator.4",
            "ExternalEditor",
            "Separator.6",
            "Settings"
        ]
    );
    rig.finish();
}

#[test]
#[ignore = "native: needs desktop"]
fn menu_reflects_live_settings_change_between_two_openings() {
    let rig = start(&["a-photos"], None);
    run(&rig.ui, || install_spy(Some("Refresh")));
    rig.choose("Refresh");
    run(&rig.ui, enable_every_menu_item);
    rig.choose("Refresh");
    let (first, second) = run(&rig.ui, || {
        let shown = spy().shown.borrow().clone();
        (ids_of(&shown[0]), ids_of(&shown[1]))
    });
    assert!(!first.contains(&"MoveToRecycleBin".to_owned()), "{first:?}");
    assert!(second.contains(&"MoveToRecycleBin".to_owned()), "{second:?}");
    rig.finish();
}
