//! W9-R11 (FEAT-RENAME, end to end): "Rename..." in a REAL `MainWindow` (hidden shell window, WARP surface, real `MainViewModel`, real `Win32DialogService` over the real
//! `Win32SecondaryWindowPort`), plus the painted context-menu entry. Ports of `ShellRenameEndToEndTests.cs` (1 test) and of
//! `RenamePromptDialogTests.ContextMenu_RenameEntry_IsPaintedAndTakesARow` (4 rows), C# `7eaee2a2`. Contract: `contracts/W9-R11.md`.
//!
//! Differences from the C# harness (`ShellRun` starts the real process through `ShellStartup`; `pr-shell`'s own `UiSmokeRun` is still a stub): the window is the
//! `main_window_fixture` window (no `ShellStartup`), the user is a scripted `ready` hook of the port that runs INSIDE the modal loop of the box (the hook plays the
//! user with `SetWindowText` and `WM_COMMAND`, exactly the messages of the C# test), and F2 / the menu id are sent to the window. The file on disk is a real file of the
//! test's own `TempRoot`; nothing touches the real Recycle Bin or any user data. Nothing is shown or activated; every wait is bounded, no sleeps.
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
mod capture;
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use main_window_fixture::{BuildOptions, Built, WarpHook, build_with_env, built, finish, run, start_ui, wait_until};
use pr_app::input::key_id::KeyId;
use pr_app::input::key_mapping;
use pr_core::abstractions::{BatchReviewItem, DialogService, NullLog, SettingsTarget, SkippedEntry};
use pr_platform_win::dialogs::dialog_service::{LazySecondaryPort, Win32DialogService};
use pr_platform_win::dialogs::folder_dialog::{FolderPicker, ShellFolderDialog};
use pr_platform_win::dialogs::native::dialog::{ID_CANCEL, ID_OK, NativeDialog};
use pr_platform_win::dialogs::rename_prompt as rename;
use pr_platform_win::dialogs::secondary_window_port::{
    ActionProfilesOpener, PortOptions, PreparedDialog, SecondaryWindowDialogs, Win32SecondaryWindowPort,
};
use pr_platform_win::dialogs::task_dialog::{TaskDialogKind, TaskDialogPort};
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::shell_input::main_window::DialogServiceOption;
use pr_platform_win::shell_menus::main_window::MainWindowMenuFeature;
use pr_platform_win::win32::dialog_native;
use pr_platform_win::win32::messages::WM_KEYDOWN;
use pr_platform_win::win32::structs::{Hresult, Hwnd};
use shell_window_fixture::send;

// ---- shared helpers -----------------------------------------------------------------------------------------------------------------------------

/// C# TestJpeg counterpart: a real encoded JPEG owned by this test's `TempRoot`.
fn write_jpeg(path: &Path) {
    use pr_imaging::pixels::{PixelBuffer, PixelLayout};
    use pr_platform_win::wic::cache_image::encode_jpeg;
    let mut pixels = PixelBuffer::allocate(64, 48, PixelLayout::Pbgra32).expect("allocate test pixels");
    for pixel in pixels.try_get_span_mut().unwrap().unwrap().chunks_exact_mut(4) {
        pixel.copy_from_slice(&[80, 120, 160, 255]);
    }
    let mut file = std::fs::File::create(path).expect("create image");
    encode_jpeg(&pixels, &mut file, 90).expect("encode JPEG");
}

/// Runs the queued work of the view model once: the worker jobs, the scheduler, the completions.
fn settle_step() {
    let b = built();
    b.runner.run_all();
    b.scheduler.run_pending();
    b.window.pump_view_model();
}

/// Hands the current image of the view model to the window, like the production presentation hub does.
fn present_current() {
    let b = built();
    let image = b.window.view_model().borrow().current_image().cloned();
    if let Some(image) = image {
        b.hub.raise(Some(&image), false);
    }
}

/// Waits (bounded) for `condition`, settling the view model on every probe.
fn wait(ui: &ui_thread::UiThread, what: &str, condition: fn(&Built) -> bool) {
    wait_until(ui, what, move || {
        settle_step();
        present_current();
        condition(&built())
    });
}

fn current_path() -> Option<String> {
    built()
        .window
        .view_model()
        .borrow()
        .catalog()
        .current()
        .map(|entry| entry.path().to_owned())
}

// ---- the dialog service over the real port -------------------------------------------------------------------------------------------------------

struct FakeTasks;
impl TaskDialogPort for FakeTasks {
    fn show(&self, _: Hwnd, _: &str, _: &str, _: TaskDialogKind) -> i32 {
        0
    }
}
struct FakeFolder;
impl ShellFolderDialog for FakeFolder {
    fn show(&self, _: Hwnd, _: &str, _: Option<&str>) -> Result<Option<String>, Hresult> {
        Ok(None)
    }
}

/// The dialogs of the port: only the rename box is real (the trait's default), every other window is absent.
struct OnlyRename;
impl SecondaryWindowDialogs for OnlyRename {
    fn settings(&self, _: SettingsTarget, _: ActionProfilesOpener) -> Option<PreparedDialog<bool>> {
        None
    }
    fn recovery(&self, _: Hwnd) -> Option<PreparedDialog<()>> {
        None
    }
    fn benchmark(&self, _: Option<&Path>) -> Option<NativeDialog> {
        None
    }
    fn action_profiles(
        &self,
        _: &[pr_core::settings::shortcuts::ReviewAction],
    ) -> Option<PreparedDialog<Option<Vec<pr_core::settings::shortcuts::ReviewAction>>>> {
        None
    }
    fn diagnostics(&self) -> Option<PreparedDialog<()>> {
        None
    }
    fn skipped_files(&self, _: &[SkippedEntry]) -> Option<PreparedDialog<()>> {
        None
    }
    fn batch_review(&self, _: &[BatchReviewItem]) -> Option<PreparedDialog<bool>> {
        None
    }
    fn custom_zoom(&self, _: i32) -> Option<PreparedDialog<Option<i32>>> {
        None
    }
}

/// What a scripted "user" saw inside one rename box.
#[derive(Default, Debug)]
struct Seen {
    owner_enabled: Option<bool>,
    edit_text: Option<String>,
    selection: Option<(i32, i32)>,
    ok_enabled_for_invalid: Option<bool>,
    error_text_for_invalid: Option<String>,
    file_untouched_for_invalid: Option<bool>,
    ok_enabled_for_valid: Option<bool>,
}

type Script = Box<dyn Fn(&NativeDialog, &mut Seen)>;

thread_local! {
    /// The scripted users, one per box that opens (UI thread only).
    static SCRIPTS: RefCell<VecDeque<Script>> = const { RefCell::new(VecDeque::new()) };
    /// What each of them saw, in order.
    static SEEN: RefCell<Vec<Seen>> = const { RefCell::new(Vec::new()) };
}

fn dialog_service() -> Rc<dyn DialogService> {
    let port = Win32SecondaryWindowPort::new(
        Rc::new(OnlyRename),
        Arc::new(NullLog),
        PortOptions {
            ready: Some(Rc::new(|dialog| {
                // The user of C# `WaitOwned` / `SetWindowText` / `WM_COMMAND`: the box is modal, so this runs inside its loop.
                let Some(script) = SCRIPTS.with(|scripts| scripts.borrow_mut().pop_front()) else {
                    // Nobody to type: close the unexpected box instead of hanging the test.
                    send_command(dialog, ID_CANCEL);
                    return;
                };
                let mut seen = Seen::default();
                script(dialog, &mut seen);
                SEEN.with(|all| all.borrow_mut().push(seen));
            })),
            ..Default::default()
        },
    );
    let secondary = Rc::new(LazySecondaryPort::from_value(Some(port)));
    Rc::new(Win32DialogService::with_task_dialogs(
        Box::new(|| built().shell.hwnd()),
        Box::new(FakeTasks),
        FolderPicker::new(Rc::new(FakeFolder), Box::new(|| built().shell.hwnd())),
        secondary,
    ))
}

fn send_command(dialog: &NativeDialog, id: i32) {
    // SAFETY: WM_COMMAND carries integers here, with a null control handle.
    unsafe {
        dialog_native::user32::send_message(
            dialog.hwnd(),
            pr_platform_win::win32::messages::WM_COMMAND,
            (id as usize) & 0xFFFF,
            0,
        );
    }
}

fn edit_of(dialog: &NativeDialog) -> Hwnd {
    dialog_native::user32::get_dlg_item(dialog.hwnd(), rename::EDIT_ID)
}

fn set_name(dialog: &NativeDialog, text: &str) {
    let _ = dialog_native::user32::set_window_text(edit_of(dialog), text);
}

fn enabled(dialog: &NativeDialog, id: i32) -> bool {
    dialog_native::user32::is_window_enabled(dialog_native::user32::get_dlg_item(dialog.hwnd(), id))
}

/// Starts a hidden main window with the real dialog service and opens `files[0]` of `photos`.
fn start(ui: &ui_thread::UiThread, names: &'static [&'static str]) -> PathBuf {
    let folder = run(ui, move || {
        use pr_imaging::memory_headroom::MemoryInfo;
        use pr_imaging::pixel_memory::PixelBufferImageCodec;
        use pr_platform_win::wic::decoder::WicDirectDecoder;
        use support::main_env::{EnvOptions, MainEnv};
        let env = MainEnv::with_options(EnvOptions {
            decoder: Arc::new(WicDirectDecoder::new(
                Arc::new(PixelBufferImageCodec::INSTANCE),
                None,
                Arc::new(|| MemoryInfo {
                    total_available: 32 * 1024 * 1024 * 1024,
                    load: 0,
                }),
            )),
            initial_view: Some(Box::new(|bridge, settings| {
                bridge.apply_initial_view(built().window.viewport().viewport_size(), settings);
            })),
            ..EnvOptions::default()
        });
        let mut options = BuildOptions::default();
        options.options.extensions.insert(DialogServiceOption(dialog_service()));
        let b = build_with_env(options, env);
        let (surface_width, surface_height) = b.shell.client_size_pixels();
        b.window
            .attach_renderer(Some(&WarpHook::new(surface_width, surface_height)))
            .expect("attach WARP renderer");
        let folder = b.photos.join("photos");
        std::fs::create_dir_all(&folder).expect("create the folder");
        for name in names {
            write_jpeg(&folder.join(name));
        }
        b.window.open_path(&folder.join(names[0]).to_string_lossy());
        folder
    });
    wait(ui, "the first image", |b| {
        b.window.view_model().borrow().has_images()
            && b.window.view_model().borrow().current_image().is_some()
            && b.window.first_image_presented()
    });
    folder
}

// ---- the end-to-end test ------------------------------------------------------------------------------------------------------------------------

// C#: ShellRenameEndToEndTests.F2AndTheMenuItem_RenameTheRealFile_TheViewFollows_UndoRestores_InvalidAndCancelChangeNothing
#[test]
#[ignore = "native: needs desktop"]
fn f2_and_the_menu_item_rename_the_real_file_the_view_follows_undo_restores_invalid_and_cancel_change_nothing() {
    let ui = start_ui();
    run(&ui, || {});
    let folder = start(&ui, &["a.jpg", "b.jpg"]);
    let a = folder.join("a.jpg");
    let b = folder.join("b.jpg");
    let renamed = folder.join("holiday 01.jpg");
    {
        let expected = a.to_string_lossy().into_owned();
        run(&ui, move || assert_eq!(current_path(), Some(expected)));
    }

    // 1.-3. F2 opens the box (name WITHOUT extension, whole name selected, the extension fixed beside it, the shell window disabled while it is modal);
    // an invalid name is refused in place (error shown, OK disabled, the file untouched); a valid name + OK renames the real file in place.
    let (a_check, b_check) = (a.clone(), b.clone());
    run(&ui, move || {
        SCRIPTS.with(|scripts| {
            scripts.borrow_mut().push_back(Box::new(move |dialog, seen| {
                seen.owner_enabled = Some(dialog_native::user32::is_window_enabled(built().shell.hwnd()));
                seen.edit_text = Some(dialog_native::user32::get_window_text(edit_of(dialog)));
                // SAFETY: EM_GETSEL with null out-pointers returns the packed selection; the handle is a live control of this dialog.
                let packed = unsafe { dialog_native::user32::send_message(edit_of(dialog), 0x00B0, 0, 0) } as u32;
                seen.selection = Some(((packed & 0xFFFF) as i32, (packed >> 16) as i32));

                set_name(dialog, "a?b");
                seen.ok_enabled_for_invalid = Some(enabled(dialog, ID_OK));
                seen.error_text_for_invalid = Some(dialog_native::user32::get_window_text(
                    dialog_native::user32::get_dlg_item(dialog.hwnd(), rename::ERROR_ID),
                ));
                seen.file_untouched_for_invalid = Some(a_check.exists());

                set_name(dialog, "holiday 01");
                seen.ok_enabled_for_valid = Some(enabled(dialog, ID_OK));
                send_command(dialog, ID_OK);
            }));
        });
        send(
            built().shell.hwnd(),
            WM_KEYDOWN,
            key_mapping::to_virtual_key(KeyId::F2) as usize,
            1,
        );
    });
    let renamed_path = renamed.to_string_lossy().into_owned();
    {
        let (a, b, renamed) = (a.clone(), b.clone(), renamed.clone());
        let expected = renamed_path.clone();
        wait_until(&ui, "the file renamed on disk and the catalog following", move || {
            settle_step();
            present_current();
            renamed.exists() && current_path().as_deref() == Some(expected.as_str()) && !a.exists() && b.exists()
        });
    }
    let (a_after, b_after, renamed_after, expected) = (a.clone(), b_check, renamed.clone(), renamed_path.clone());
    run(&ui, move || {
        let seen = SEEN.with(|all| std::mem::take(&mut *all.borrow_mut()));
        assert_eq!(seen.len(), 1, "exactly one box opened: {seen:?}");
        let seen = &seen[0];
        assert_eq!(
            seen.owner_enabled,
            Some(false),
            "the box is modal: the shell window is disabled"
        );
        assert_eq!(seen.edit_text.as_deref(), Some("a"), "the name without the extension");
        assert_eq!(seen.selection, Some((0, 1)), "the whole name part is selected");
        assert_eq!(
            seen.ok_enabled_for_invalid,
            Some(false),
            "OK must be disabled for an invalid name"
        );
        assert_eq!(
            seen.error_text_for_invalid.as_deref(),
            Some(
                pr_l10n::localizer::Localizer::current()
                    .get("dialog.rename.error.invalidCharacter")
                    .as_str()
            )
        );
        assert_eq!(seen.file_untouched_for_invalid, Some(true));
        assert_eq!(seen.ok_enabled_for_valid, Some(true));
        // In place, same index; the presenter and the status bar follow; the shell window is enabled again.
        assert!(!a_after.exists());
        assert!(b_after.exists());
        assert!(renamed_after.exists());
        let b = built();
        assert_eq!(current_path().as_deref(), Some(expected.as_str()));
        assert_eq!(b.window.view_model().borrow().current_index(), 0);
        assert_eq!(
            b.window
                .view_model()
                .borrow()
                .presenter()
                .current_photo_info()
                .map(|info| info.file_name.clone())
                .as_deref(),
            Some("holiday 01.jpg")
        );
        assert!(b.window.view_model().borrow().status_text().contains("holiday 01.jpg"));
        assert!(dialog_native::user32::is_window_enabled(b.shell.hwnd()));
    });

    // 4. Undo puts the old name back.
    run(&ui, || {
        let b = built();
        let changes = b.window.view_model().borrow_mut().undo();
        b.window.apply_changes(&changes);
    });
    {
        let (a, renamed) = (a.clone(), renamed.clone());
        let expected = a.to_string_lossy().into_owned();
        wait_until(&ui, "Undo restored the name", move || {
            settle_step();
            present_current();
            a.exists() && !renamed.exists() && current_path().as_deref() == Some(expected.as_str())
        });
    }

    // 5. The menu item (controller dispatch) opens the same box; Cancel changes nothing.
    let (a_menu, never) = (a.clone(), folder.join("never.jpg"));
    run(&ui, move || {
        SCRIPTS.with(|scripts| {
            scripts.borrow_mut().push_back(Box::new(|dialog, seen| {
                seen.edit_text = Some(dialog_native::user32::get_window_text(edit_of(dialog)));
                set_name(dialog, "never");
                send_command(dialog, ID_CANCEL);
            }));
        });
        let controller = built()
            .window
            .with_feature::<MainWindowMenuFeature, _>(|feature| Rc::clone(feature.menus().context_menu()))
            .expect("the context-menu feature is registered");
        assert!(
            controller.execute("RenameFile").is_ok(),
            "the menu command did not finish after Cancel"
        );
        let seen = SEEN.with(|all| std::mem::take(&mut *all.borrow_mut()));
        assert_eq!(seen.len(), 1, "exactly one box opened via the menu: {seen:?}");
        assert_eq!(seen[0].edit_text.as_deref(), Some("a"));
    });
    {
        let a = a_menu.to_string_lossy().into_owned();
        wait_until(&ui, "nothing changed after Cancel", move || {
            settle_step();
            current_path().as_deref() == Some(a.as_str())
        });
    }
    run(&ui, move || {
        assert!(a_menu.exists());
        assert!(!never.exists());
        assert!(
            SCRIPTS.with(|scripts| scripts.borrow().is_empty()),
            "every scripted user was used"
        );
        assert!(dialog_native::user32::is_window_enabled(built().shell.hwnd()));
    });
    finish(ui);
}

// ---- the painted menu entry -----------------------------------------------------------------------------------------------------------------------

mod menu_paint {
    use super::*;
    use pr_app::menus::builder;
    use pr_app::menus::model::{ContextMenuBuildContext, MenuItemKind, MenuItemModel};
    use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
    use pr_core::settings::context_menu::{ContextMenuItemId, DEFAULT_HIDDEN, apply_hidden};
    use pr_l10n::catalog::LanguageCatalog;
    use pr_l10n::localizer::Localizer;
    use pr_platform_win::host::contracts::{WindowMessage, WindowMessageHandler};
    use pr_platform_win::shell_menus::{
        mapping,
        owner_draw::{OwnerDrawItem, OwnerDrawMenuSession},
    };
    use pr_platform_win::win32::{api, messages, structs::Hmenu};
    use std::cell::Cell;
    use std::sync::mpsc;
    use std::time::Duration;
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, WPARAM},
            Graphics::Gdi::HBRUSH,
            UI::WindowsAndMessaging::{
                AppendMenuW, HMENU, MENU_ITEM_FLAGS, MENUINFO, MENUINFO_MASK, PostMessageW, SetMenuInfo,
            },
        },
        core::PCWSTR,
    };

    const VK_ESCAPE: usize = 0x1B;
    const VK_DOWN: usize = 0x28;

    struct Menu(Hmenu);
    impl Drop for Menu {
        fn drop(&mut self) {
            let _ = api::user32::destroy_menu(self.0);
        }
    }
    struct RestoreLocalizer(Arc<Localizer>);
    impl Drop for RestoreLocalizer {
        fn drop(&mut self) {
            Localizer::set_current(Arc::clone(&self.0));
        }
    }

    fn post(owner: isize, message: u32, key: usize) {
        // SAFETY: posts integer-only messages to the live test-owned owner; no borrowed pointer crosses threads.
        unsafe {
            PostMessageW(Some(HWND(owner as *mut _)), message, WPARAM(key), LPARAM(0)).expect("post");
        }
    }

    // SAFETY: `DwmFlush` is a plain Win32 call with no arguments or pointers; it only blocks until the next DWM composition pass.
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmFlush() -> i32;
    }
    fn flush_compositor(hwnd: isize) {
        use windows::Win32::Graphics::Gdi::{RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow};
        // SAFETY: both flush calls take no pointers; they synchronize completed GDI draws and the compositor before the real window DC is read.
        unsafe {
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

    /// What the captured popup looked like.
    #[derive(Default)]
    struct Probe {
        /// False once its popup is gone: the handler cannot be unregistered, so a finished probe lets the message through to the next one.
        active: Cell<bool>,
        root: Cell<isize>,
        downs: usize,
        label: String,
        painted: Cell<u32>,
        size: Cell<(i32, i32)>,
        failure: RefCell<Option<Box<dyn std::any::Any + Send>>>,
    }
    impl Probe {
        fn idle(&self, message: &WindowMessage) {
            let first = self.root.get() == 0;
            if first {
                self.root.set(message.lparam);
            } else if message.lparam != self.root.get() || self.painted.get() >= 2 {
                return;
            }
            flush_compositor(message.lparam);
            let shot = capture::Shot::capture(message.lparam);
            shot.assert_not_flat(message.lparam, &self.label);
            if first {
                let rect = api::user32::get_window_rect(message.lparam).expect("popup rect");
                self.size.set((rect.right - rect.left, rect.bottom - rect.top));
                self.painted.set(1);
                if self.downs > 0 {
                    // The user presses Down until the entry is highlighted; the next idle paints the highlighted row.
                    for _ in 0..self.downs {
                        post(message.hwnd, WM_KEYDOWN, VK_DOWN);
                    }
                    return;
                }
            } else {
                self.painted.set(2);
            }
            // One Escape closes the one-level menu; a second would stay queued and close the next popup before it is painted.
            post(message.hwnd, WM_KEYDOWN, VK_ESCAPE);
        }
    }
    impl WindowMessageHandler for Probe {
        fn try_handle(&self, message: &WindowMessage) -> Option<isize> {
            if !self.active.get() || message.msg != messages::WM_ENTERIDLE || message.wparam != 2 || message.lparam == 0
            {
                return None;
            }
            if let Err(error) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.idle(message))) {
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
            let child = submenu.then(|| Menu(api::user32::create_popup_menu().expect("popup menu")));
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
            // SAFETY: MF_OWNERDRAW treats data as an integer token; the session owns its item until the menu is destroyed.
            unsafe {
                AppendMenuW(
                    HMENU(menu as *mut _),
                    MENU_ITEM_FLAGS(flags),
                    id,
                    PCWSTR(data as *const u16),
                )
                .expect("append");
            }
            if let Some(child) = child {
                std::mem::forget(child); // the root now owns the appended child
            }
        }
    }

    fn model(hide_rename: bool) -> (Vec<MenuItemModel>, AppSettings) {
        // The default hidden set (no zoom cluster / recycle): a short menu fits any screen at 144 DPI.
        let mut settings = AppSettings::new(&EnglishSettingsTexts);
        let mut hidden = DEFAULT_HIDDEN.to_vec();
        if hide_rename {
            hidden.push(ContextMenuItemId::RenameFile);
        }
        apply_hidden(&mut settings, hidden);
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
        .expect("the menu model");
        (items, settings)
    }

    /// C# `ShowMenuAndCapture`: shows the popup on the (hidden) shell window at `dpi`, captures it from the window DC when it is idle, returns its size.
    fn show_and_capture(items: &[MenuItemModel], dpi: u32, downs: usize, label: &str) -> (i32, i32) {
        let owner = built().shell.hwnd();
        let session = OwnerDrawMenuSession::new(owner, dpi, None);
        assert!(session.attach());
        let root = Menu(api::user32::create_popup_menu().expect("popup menu"));
        populate(root.0, items, &session, &mut 0);
        let info = MENUINFO {
            cbSize: std::mem::size_of::<MENUINFO>() as u32,
            fMask: MENUINFO_MASK(0x8000_0002),
            hbrBack: HBRUSH(session.background_brush() as *mut _),
            ..Default::default()
        };
        // SAFETY: root and the session's brush are live until after TrackPopupMenuEx and the menu's destruction.
        unsafe {
            SetMenuInfo(HMENU(root.0 as *mut _), &info).expect("menu info");
        }
        let probe = Rc::new(Probe {
            active: Cell::new(true),
            downs,
            label: label.to_owned(),
            ..Default::default()
        });
        built().shell.insert_message_handler_first(probe.clone());
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
        // Test-only: bypass the popup animation so the completed GDI/DWM paint is the visible frame.
        api::user32::track_popup_menu_ex(
            root.0,
            messages::TPM_RETURNCMD | windows::Win32::UI::WindowsAndMessaging::TPM_NOANIMATION.0,
            150,
            150,
            owner,
        );
        let _ = done.send(());
        probe.active.set(false);
        let timed_out = watchdog.join().expect("watchdog");
        drop(root);
        session.dispose();
        if let Some(error) = probe.failure.borrow_mut().take() {
            std::panic::resume_unwind(error);
        }
        assert!(!timed_out, "menu idle/capture timed out: {label}");
        assert!(probe.painted.get() >= 1, "the popup was never captured: {label}");
        probe.size.get()
    }

    fn one_row(dpi: u32, language: &'static str) {
        let ui = start_ui();
        run(&ui, move || {
            let _restore = RestoreLocalizer(Localizer::current());
            let mut warnings = Vec::new();
            let english = LanguageCatalog::try_parse(
                include_str!("../../../assets/languages/en.json"),
                "en.json",
                &mut warnings,
            )
            .expect("en catalog");
            let overlays = if language == "vi" {
                vec![
                    LanguageCatalog::try_parse(
                        include_str!("../../../assets/languages/vi.json"),
                        "vi.json",
                        &mut warnings,
                    )
                    .expect("vi catalog"),
                ]
            } else {
                Vec::new()
            };
            Localizer::set_current(Arc::new(Localizer::create(&english, &overlays, &warnings)));
            let _window = main_window_fixture::build(BuildOptions::default());

            let (with_items, _with_settings) = model(false);
            let (without_items, _without_settings) = model(true);
            let top: Vec<&MenuItemModel> = with_items
                .iter()
                .filter(|item| item.kind != MenuItemKind::Separator)
                .collect();
            let index = top
                .iter()
                .position(|item| item.id == "RenameFile")
                .expect("the menu model has no Rename entry");
            assert_eq!(top[index].text, pr_l10n::tr::main_menu_rename_file());
            assert_eq!(top[index].shortcut_text.as_deref(), Some("F2"));
            assert!(
                without_items.iter().all(|item| item.id != "RenameFile"),
                "hiding the entry removes it from the model"
            );

            // The user presses Down until Rename is highlighted: the popup with the entry, then the popup without it.
            let with_size = show_and_capture(
                &with_items,
                dpi,
                index + 1,
                &format!("menu with Rename {language} {dpi}"),
            );
            let without_size =
                show_and_capture(&without_items, dpi, 1, &format!("menu without Rename {language} {dpi}"));
            // One more row: the entry really takes room in the painted menu (not just in the model).
            let row_min = (14 * dpi / 96) as i32;
            assert!(
                with_size.1 >= without_size.1 + row_min,
                "menu with Rename is {}px high, without it {}px ({language} {dpi})",
                with_size.1,
                without_size.1
            );
        });
        finish(ui);
    }

    // C#: RenamePromptDialogTests.ContextMenu_RenameEntry_IsPaintedAndTakesARow, four rows (96 / 144 DPI x en / vi). Window-DC BitBlt of the real popup.
    #[test]
    #[ignore = "native: needs interactive desktop for real popup BitBlt"]
    fn context_menu_rename_entry_is_painted_and_takes_a_row() {
        let started = Instant::now();
        for language in ["en", "vi"] {
            for dpi in [96, 144] {
                one_row(dpi, language);
            }
        }
        eprintln!("rename menu paint: 4 rows in {:?}", started.elapsed());
    }
}
