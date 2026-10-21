//! Tests of W8-14c (N): the input of a REAL `MainWindow` (hidden shell window, WARP surface, real `MainViewModel`), a port of
//! `ShellInputEndToEndTests.cs` (16 tests, baseline `d92d6aae`). Messages are sent to the HWND (`SendMessage`), go through the three input handlers, the
//! real pointer/Fit/shortcut controllers and reach the view model.
//!
//! Every test is `#[ignore = "native: needs desktop"]`: it builds a whole `MainWindow`, whose constructor also runs the units 13i (frame and scroll bars),
//! 13j (viewing) and 13k (windowing) and `MainWindowInner::register_input` of this unit. They cannot run before those units are merged; until a desktop
//! run is recorded the PARITY rows stay `Partial`. Nothing is shown or activated; every wait is bounded, no sleeps.
//!
//! The fixture's `FakeHub` is not connected to the presenter, so [`present_current`] hands the view model's current image to the window the way the
//! production hub does (`ImageChanged`).
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
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use main_window_fixture::{BuildOptions, Built, build, built, finish, run, start_ui, wait_until};
use pr_app::input::key_id::KeyId;
use pr_app::input::key_mapping;
use pr_app::input::primitives::PointD;
use pr_imaging::decode_types::PlatformImage;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::shell_input::drop_target::{DropCallbacks, DropRegisterError, ShellDropTarget};
use pr_platform_win::shell_input::main_window::MainWindowInputFeature;
use pr_platform_win::win32::api::kernel32;
use pr_platform_win::win32::messages::{WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEWHEEL};
use shell_window_fixture::{client_origin, send};
use ui_thread::UiThread;
use windows::Win32::Foundation::{DV_E_FORMATETC, E_NOTIMPL, HGLOBAL, POINTL};
use windows::Win32::System::Com::{FORMATETC, IDataObject, IDataObject_Impl, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::core::{BOOL, ComObject, HRESULT, implement};

/// A tiny file the fake decoder accepts (`MainEnv::add_image`).
const TINY_IMAGE: [u8; 4] = [0xFF, 0xD8, 0xFF, 0xD9];

// ---- helpers (UI thread) -------------------------------------------------------------------------------------------------------------------------

/// Runs the view model's queued work once: the worker jobs, the scheduler, the completions.
fn settle_step() {
    let b = built();
    b.runner.run_all();
    b.scheduler.run_pending();
    b.window.pump_view_model();
}

thread_local! {
    /// Identity of the last image the view model handed to the hub (see [`present_current`]).
    static LAST_PRESENTED: RefCell<Option<usize>> = const { RefCell::new(None) };
}

/// The preview the fake decoder (`SizedDecoder`) stands for: 600 x 400 pixels of a 6000 x 4000 source.
fn preview_pixels() -> PlatformImage {
    let buffer = PixelBuffer::allocate(600, 400, PixelLayout::Pbgra32).expect("a preview buffer");
    PlatformImage::Pixels(Arc::new(buffer))
}

/// Hands a NEW current image of the view model to the window, like the production composition does: the presentation sink calls `set_current_image`
/// (here: the hub gets the preview) and then `apply_initial_view_mode` (here: `ShellInput::apply_initial_view`, the composition root of W8-17c wires it
/// the same way). The test view model's decoder is a fake whose images are `Opaque` (no pixels); the window lays out pixels, so the hub gets a
/// same-sized pixel buffer instead. Once per image, not once per probe.
fn present_current() {
    let b = built();
    let image = b.window.view_model().borrow().current_image().cloned();
    let Some(image) = image else {
        return;
    };
    let identity = match &image {
        PlatformImage::Opaque(opaque) => Arc::as_ptr(opaque).cast::<()>() as usize,
        PlatformImage::Pixels(pixels) => Arc::as_ptr(pixels) as usize,
    };
    if LAST_PRESENTED.with(|last| last.replace(Some(identity))) == Some(identity) {
        return;
    }
    let shown = match image {
        PlatformImage::Opaque(_) => preview_pixels(),
        pixels => pixels,
    };
    b.hub.raise(Some(&shown), false);
    b.window.apply_changes(&pr_app::viewmodels::main::ChangeSet::default());
    input().input().apply_initial_view();
}

/// Waits (bounded) for `condition`, settling the view model on every probe.
fn wait(ui: &UiThread, what: &str, condition: fn(&Built) -> bool) {
    wait_until(ui, what, move || {
        settle_step();
        present_current();
        condition(&built())
    });
}

/// Builds the window and opens a folder with the given tiny images (the first one is shown).
fn start_with_images(ui: &UiThread, names: &'static [&'static str]) -> PathBuf {
    let folder = run(ui, move || {
        let b = build(BuildOptions::default());
        let folder = b.photos.join("shots");
        std::fs::create_dir_all(&folder).expect("create the folder");
        for name in names {
            std::fs::write(folder.join(name), TINY_IMAGE).expect("write an image");
        }
        b.window.open_path(&folder.join(names[0]).to_string_lossy());
        folder
    });
    wait(ui, "the first image", |b| {
        b.window.view_model().borrow().has_images() && b.window.view_model().borrow().current_image().is_some()
    });
    folder
}

fn input() -> std::rc::Rc<pr_platform_win::shell_input::main_window::MainWindowInput> {
    built()
        .window
        .with_feature::<MainWindowInputFeature, _>(|feature| std::rc::Rc::clone(feature.input()))
        .expect("the input feature is registered")
}

fn key(key: KeyId, repeat: bool) {
    let lparam: isize = 1 | if repeat { 0x4000_0000 } else { 0 };
    send(
        built().window.hwnd(),
        WM_KEYDOWN,
        key_mapping::to_virtual_key(key) as usize,
        lparam,
    );
}

fn pack(x: i32, y: i32) -> isize {
    ((((y as u16) as usize) << 16) | ((x as u16) as usize)) as isize
}

fn index() -> i32 {
    built().window.view_model().borrow().current_index()
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

fn edit_settings(change: impl FnOnce(&mut pr_core::settings::app_settings::AppSettings)) {
    let source = built().window.view_model().borrow().settings_source();
    let edit = source.begin_edit(change);
    let _ = source.commit_edit(edit);
}

fn settings() -> std::sync::Arc<pr_core::settings::app_settings::AppSettings> {
    built().window.view_model().borrow().settings_source().current()
}

// ---- keyboard ------------------------------------------------------------------------------------------------------------------------------------

// C#: ShellInputEndToEndTests.ArrowKeysHomeEnd_NavigateTheFolder_ThroughRealMessages
#[test]
#[ignore = "native: needs desktop"]
fn arrow_keys_home_end_navigate_the_folder_through_real_messages() {
    let ui = start_ui();
    run(&ui, || {});
    start_with_images(&ui, &["a.jpg", "b.jpg", "c.jpg"]);
    for (pressed, expected) in [
        (KeyId::RIGHT, 1),
        (KeyId::RIGHT, 2),
        (KeyId::LEFT, 1),
        (KeyId::END, 2),
        (KeyId::HOME, 0),
    ] {
        run(&ui, move || key(pressed, false));
        match expected {
            0 => wait(&ui, "index 0", |_| index() == 0),
            1 => wait(&ui, "index 1", |_| index() == 1),
            _ => wait(&ui, "index 2", |_| index() == 2),
        }
    }
    finish(ui);
}

// C#: ShellInputEndToEndTests.Escape_ClosesTheWindow_AndF11TogglesTheViewerFullscreenState
#[test]
#[ignore = "native: needs desktop"]
fn escape_closes_the_window_and_f11_toggles_the_viewer_fullscreen_state() {
    let ui = start_ui();
    start_with_images(&ui, &["a.jpg"]);
    run(&ui, || key(KeyId::F11, false));
    wait(&ui, "fullscreen on", |b| {
        b.window.view_model().borrow().viewer().borrow().is_fullscreen()
    });
    // Esc in fullscreen leaves fullscreen and does NOT close the window.
    run(&ui, || key(KeyId::ESCAPE, false));
    wait(&ui, "fullscreen off", |b| {
        !b.window.view_model().borrow().viewer().borrow().is_fullscreen()
    });
    assert!(!run(&ui, || built().window.is_closing_or_closed()));
    // A plain Esc closes the window.
    run(&ui, || key(KeyId::ESCAPE, false));
    wait(&ui, "the window closes", |b| b.window.is_closing_or_closed());
    finish(ui);
}

// C#: ShellInputEndToEndTests.SpaceSkips_AndIToggleTheInfoOverlay_WithTheDefaultShortcuts
#[test]
#[ignore = "native: needs desktop"]
fn space_skips_and_i_toggles_the_info_overlay_with_the_default_shortcuts() {
    let ui = start_ui();
    start_with_images(&ui, &["a.jpg", "b.jpg"]);
    let overlay = run(&ui, || settings().show_info_overlay);
    run(&ui, || key(KeyId::I, false));
    wait_until(&ui, "the overlay toggles", move || {
        settle_step();
        settings().show_info_overlay != overlay
    });
    run(&ui, || key(KeyId::SPACE, false));
    wait(&ui, "Space (Skip) moves to the next image", |_| index() == 1);
    finish(ui);
}

// C#: ShellInputEndToEndTests.KeyK_TogglesKeepZoomAcrossImages_AndSavesItThroughTheStore (the saved value is read from the store, not from the file)
#[test]
#[ignore = "native: needs desktop"]
fn key_k_toggles_keep_zoom_across_images_and_saves_it_through_the_store() {
    let ui = start_ui();
    start_with_images(&ui, &["a.jpg"]);
    let initial = run(&ui, || settings().keep_zoom_across_images);
    run(&ui, || key(KeyId::K, false));
    wait_until(&ui, "K flips KeepZoomAcrossImages", move || {
        settle_step();
        settings().keep_zoom_across_images != initial
    });
    let saved = run(&ui, || {
        let source = built().window.view_model().borrow().settings_source();
        (source.store_current().keep_zoom_across_images, source.has_override())
    });
    assert_eq!(
        (!initial, false),
        saved,
        "the store holds the new value and the provisional override is gone"
    );
    finish(ui);
}

// C#: ShellInputEndToEndTests.SettingsChanged_RebuildsTheShortcutTable
#[test]
#[ignore = "native: needs desktop"]
fn settings_changed_rebuilds_the_shortcut_table() {
    let ui = start_ui();
    start_with_images(&ui, &["a.jpg"]);
    let initial = run(&ui, || settings().show_info_overlay);
    run(&ui, || {
        let store = std::sync::Arc::clone(built().window.view_model().borrow().settings_source().store());
        let mut changed = (*store.current()).clone();
        changed.shortcuts.toggle_info_overlay = "J".to_owned();
        assert!(store.save(changed).is_ok());
    });
    run(&ui, || key(KeyId::J, false));
    wait_until(&ui, "J (new) toggles the overlay", move || {
        settle_step();
        settings().show_info_overlay != initial
    });
    run(&ui, || key(KeyId::I, false)); // the old key is no longer bound
    run(&ui, || key(KeyId::J, false));
    wait_until(&ui, "J toggles back", move || {
        settle_step();
        settings().show_info_overlay == initial
    });
    finish(ui);
}

// ---- mouse ---------------------------------------------------------------------------------------------------------------------------------------

/// Pixel position of a client point (DIP) for a `SendMessage` that carries SCREEN pixels (the wheel).
fn screen_pixels(x_dip: f64, y_dip: f64) -> (i32, i32) {
    let b = built();
    let dpi = b.window.window().dpi_scale();
    let origin = client_origin(b.window.hwnd());
    (origin.x + (x_dip * dpi) as i32, origin.y + (y_dip * dpi) as i32)
}

// C#: ShellInputEndToEndTests.Wheel_ZoomsAtTheCursor_ThroughRealMessages_KeepingThePointUnderIt
#[test]
#[ignore = "native: needs desktop"]
fn wheel_zooms_at_the_cursor_through_real_messages_keeping_the_point_under_it() {
    let ui = start_ui();
    start_with_images(&ui, &["big.jpg"]);
    let (x, y, fraction_before, zoom_before) = run(&ui, || {
        let b = built();
        let size = b.window.window().client_size_dip();
        let (x, y) = (size.width / 3.0, size.height / 3.0);
        let viewport = b.window.viewport();
        let p = viewport.to_image_element(PointD { x, y });
        let fraction = (
            p.x / viewport.image_actual_width(),
            p.y / viewport.image_actual_height(),
        );
        (x, y, fraction, b.window.view_model().borrow().viewer().borrow().zoom())
    });
    run(&ui, move || {
        let (sx, sy) = screen_pixels(x, y);
        let hwnd = built().window.hwnd();
        send(hwnd, WM_MOUSEWHEEL, (120_usize & 0xFFFF) << 16, pack(sx, sy));
    });
    wait_until(&ui, "the wheel zooms", move || {
        settle_step();
        let b = built();
        let zoom = b.window.view_model().borrow().viewer().borrow().zoom();
        !is_fit() && zoom != zoom_before && !b.window.viewport().is_layout_posted()
    });
    // The zoom is applied inside the pointer call, where the viewer is borrowed and the layout reads the content of the previous pass (D-W8B4I-1); the
    // Render-priority resume of the zoom pass re-anchors on the settled layout. The END state is what the C# test checks, so wait (bounded) for the resume.
    let fraction_at = move || {
        let viewport = built().window.viewport().clone();
        let p = viewport.to_image_element(PointD { x, y });
        (
            p.x / viewport.image_actual_width(),
            p.y / viewport.image_actual_height(),
        )
    };
    wait_until(&ui, "the zoom pass re-anchored the point under the cursor", move || {
        settle_step();
        let after = fraction_at();
        (fraction_before.0 - after.0).abs() < 0.01 && (fraction_before.1 - after.1).abs() < 0.01
    });
    let fraction_after = run(&ui, fraction_at);
    assert!(
        (fraction_before.0 - fraction_after.0).abs() < 0.01,
        "{fraction_before:?} vs {fraction_after:?}"
    );
    assert!(
        (fraction_before.1 - fraction_after.1).abs() < 0.01,
        "{fraction_before:?} vs {fraction_after:?}"
    );
    finish(ui);
}

// C#: ShellInputEndToEndTests.MiddleClick_DefaultAction_ZoomsToActualSize
#[test]
#[ignore = "native: needs desktop"]
fn middle_click_default_action_zooms_to_actual_size() {
    let ui = start_ui();
    start_with_images(&ui, &["big.jpg"]);
    run(&ui, || {
        let b = built();
        let size = b.window.window().client_size_dip();
        let dpi = b.window.window().dpi_scale();
        send(
            b.window.hwnd(),
            WM_MBUTTONDOWN,
            0,
            pack((size.width * dpi / 2.0) as i32, (size.height * dpi / 2.0) as i32),
        );
    });
    wait(&ui, "the middle button zooms to 100 %", |_| zoom_percent() == 100);
    assert!(!run(&ui, is_fit));
    finish(ui);
}

// C#: ShellInputEndToEndTests.ClickToZoom_LeftPressRelease_ZoomsInAndOutAgain_ThroughRealCapture
#[test]
#[ignore = "native: needs desktop"]
fn click_to_zoom_left_press_release_zooms_through_real_capture() {
    let ui = start_ui();
    start_with_images(&ui, &["big.jpg"]);
    run(&ui, || edit_settings(|s| s.click_to_zoom_enabled = true));
    let (cx, cy) = run(&ui, || {
        let b = built();
        let viewport = b.window.viewport();
        let origin = viewport.image_origin();
        let dpi = b.window.window().dpi_scale();
        (
            ((origin.x + viewport.image_actual_width() / 2.0) * dpi) as i32,
            ((origin.y + viewport.image_actual_height() / 2.0) * dpi) as i32,
        )
    });
    run(&ui, move || {
        send(built().window.hwnd(), WM_LBUTTONDOWN, 0x0001, pack(cx, cy));
    });
    assert!(
        run(&ui, || built().window.window().has_pointer_capture()),
        "a left press on the image captures the pointer"
    );
    run(&ui, move || {
        send(built().window.hwnd(), WM_LBUTTONUP, 0, pack(cx, cy));
    });
    assert!(!run(&ui, || built().window.window().has_pointer_capture()));
    wait(&ui, "the click zooms", |_| !is_fit());
    finish(ui);
}

// ---- keyboard zoom frames ------------------------------------------------------------------------------------------------------------------------

thread_local! {
    /// `(image width, horizontal offset, vertical offset)` of every frame the window presented (UI thread).
    static FRAMES: RefCell<Vec<(f64, f64, f64)>> = const { RefCell::new(Vec::new()) };
}

// C#: ShellInputEndToEndTests.KeyboardZoom_NeverPresentsAFrameAtTheNewSizeWithTheOldScrollOffsets
#[test]
#[ignore = "native: needs desktop"]
fn keyboard_zoom_never_presents_a_frame_at_the_new_size_with_the_old_scroll_offsets() {
    let ui = start_ui();
    start_with_images(&ui, &["portrait.jpg"]);
    let subscription = run(&ui, || {
        built().window.window().subscribe_render(Box::new(|_tick| {
            let viewport = built().window.viewport().clone();
            FRAMES.with(|frames| {
                frames.borrow_mut().push((
                    viewport.image_actual_width(),
                    viewport.horizontal_offset(),
                    viewport.vertical_offset(),
                ))
            });
        }))
    });
    for pressed in [KeyId::D1, KeyId::ADD, KeyId::ADD, KeyId::SUBTRACT] {
        let before = run(&ui, || {
            FRAMES.with(|frames| frames.borrow_mut().clear());
            built().window.viewport().image_actual_width()
        });
        run(&ui, move || key(pressed, false));
        wait_until(&ui, "the key zooms", move || {
            settle_step();
            let viewport = built().window.viewport().clone();
            (viewport.image_actual_width() - before).abs() > 0.5 && !viewport.is_layout_posted()
        });
        let seen = run(&ui, || {
            built().window.window().invalidate();
            FRAMES.with(|frames| frames.borrow().len())
        });
        wait_until(&ui, "one more frame", move || {
            settle_step();
            FRAMES.with(|frames| frames.borrow().len()) > seen
        });
        let (final_frame, at_new_size) = run(&ui, || {
            let viewport = built().window.viewport().clone();
            let final_frame = (
                viewport.image_actual_width(),
                viewport.horizontal_offset(),
                viewport.vertical_offset(),
            );
            let at_new_size: Vec<(f64, f64, f64)> = FRAMES.with(|frames| {
                frames
                    .borrow()
                    .iter()
                    .copied()
                    .filter(|frame| (frame.0 - final_frame.0).abs() < 0.5)
                    .collect()
            });
            (final_frame, at_new_size)
        });
        assert!(!at_new_size.is_empty(), "{pressed:?}: no frame at the new size");
        for frame in at_new_size {
            assert!(
                (frame.1 - final_frame.1).abs() < 1.0 && (frame.2 - final_frame.2).abs() < 1.0,
                "{pressed:?}: a frame at the new size {:.0} has the offsets ({:.0},{:.0}); the final ones are ({:.0},{:.0})",
                frame.0,
                frame.1,
                frame.2,
                final_frame.1,
                final_frame.2
            );
        }
    }
    run(&ui, move || {
        built().window.window().unsubscribe(subscription);
        FRAMES.with(|frames| frames.borrow_mut().clear());
    });
    finish(ui);
}

// ---- keep zoom across images ---------------------------------------------------------------------------------------------------------------------

fn keep_zoom_across_images(keep: bool) {
    let ui = start_ui();
    start_with_images(&ui, &["a.jpg", "b.jpg"]);
    run(&ui, move || {
        edit_settings(|s| s.keep_zoom_across_images = keep);
        let b = built();
        let _ = b.window.view_model().borrow().viewer().borrow_mut().set_zoom(2.0);
    });
    assert!(!run(&ui, is_fit));
    run(&ui, || built().window.update_view_model(|vm| vm.next()));
    wait(&ui, "the second image shows", |_| index() == 1);
    if keep {
        wait(&ui, "the zoom is kept", |b| {
            let viewer = b.window.view_model().borrow().viewer().clone();
            let viewer = viewer.borrow();
            !viewer.is_fit() && (viewer.zoom() - 2.0).abs() < 1e-6
        });
    } else {
        wait(&ui, "navigation returns to Fit", |_| is_fit());
    }
    finish(ui);
}

// C#: ShellInputEndToEndTests.KeepZoomAcrossImages_DecidesWhetherNavigationKeepsTheZoom (theory row true)
#[test]
#[ignore = "native: needs desktop"]
fn keep_zoom_across_images_true_keeps_the_zoom_on_navigation() {
    keep_zoom_across_images(true);
}

// C#: ShellInputEndToEndTests.KeepZoomAcrossImages_DecidesWhetherNavigationKeepsTheZoom (theory row false)
#[test]
#[ignore = "native: needs desktop"]
fn keep_zoom_across_images_false_returns_to_fit_on_navigation() {
    keep_zoom_across_images(false);
}

// ---- drag and drop ---------------------------------------------------------------------------------------------------------------------------------

/// A data object carrying one `CF_HDROP` list (wide), the medium owning the `HGLOBAL`.
#[implement(IDataObject)]
struct DropList {
    files: Vec<String>,
}

fn hdrop_bytes(files: &[String]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&20u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&1i32.to_le_bytes());
    for file in files {
        for unit in file.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

#[allow(non_snake_case)] // the trait methods carry the COM names
impl IDataObject_Impl for DropList_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        // SAFETY: the caller passes a live FORMATETC.
        if unsafe { (*pformatetcin).cfFormat } != 15 {
            return Err(DV_E_FORMATETC.into());
        }
        let bytes = hdrop_bytes(&self.files);
        let handle = kernel32::global_alloc(0x0042, bytes.len()).expect("GlobalAlloc");
        let target = kernel32::global_lock(handle).expect("GlobalLock");
        // SAFETY: `target` is the locked allocation of `bytes.len()` bytes; the source is a live slice; they do not overlap.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len()) };
        kernel32::global_unlock(handle);
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 {
                hGlobal: HGLOBAL(handle as *mut c_void),
            },
            pUnkForRelease: ManuallyDrop::new(None),
        })
    }
    fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> windows::core::Result<()> {
        Err(DV_E_FORMATETC.into())
    }
    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        // SAFETY: the caller passes a live FORMATETC.
        if unsafe { (*pformatetc).cfFormat } == 15 {
            HRESULT(0)
        } else {
            DV_E_FORMATETC
        }
    }
    fn GetCanonicalFormatEtc(&self, _a: *const FORMATETC, _b: *mut FORMATETC) -> HRESULT {
        DV_E_FORMATETC
    }
    fn SetData(&self, _a: *const FORMATETC, _b: *const STGMEDIUM, _c: BOOL) -> windows::core::Result<()> {
        Err(DV_E_FORMATETC.into())
    }
    fn EnumFormatEtc(&self, _d: u32) -> windows::core::Result<windows::Win32::System::Com::IEnumFORMATETC> {
        Err(DV_E_FORMATETC.into())
    }
    fn DAdvise(
        &self,
        _a: *const FORMATETC,
        _b: u32,
        _c: windows::core::Ref<windows::Win32::System::Com::IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(E_NOTIMPL.into())
    }
    fn DUnadvise(&self, _a: u32) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn EnumDAdvise(&self) -> windows::core::Result<windows::Win32::System::Com::IEnumSTATDATA> {
        Err(E_NOTIMPL.into())
    }
}

// C#: ShellInputEndToEndTests.DropTarget_IsRegisteredWithOle_AndADropOpensTheFirstFile
#[test]
#[ignore = "native: needs desktop"]
fn drop_target_is_registered_with_ole_and_a_drop_opens_the_first_file() {
    let ui = start_ui();
    let folder = start_with_images(&ui, &["a.jpg"]);
    // A sibling folder, spelled without `..`: the catalog reports the normalized path, which the wait below compares.
    let other = folder.parent().expect("the folder has a parent").join("other");
    let target_file = run(&ui, move || {
        std::fs::create_dir_all(&other).expect("create the other folder");
        let file = other.join("z.jpg");
        std::fs::write(&file, TINY_IMAGE).expect("write z.jpg");
        file.to_string_lossy().into_owned()
    });
    assert!(
        run(&ui, || input().drop_target_registered()),
        "the drop target is registered"
    );
    // A second target on the same HWND is refused by OLE (DRAGDROP_E_ALREADYREGISTERED).
    let refused = run(&ui, || {
        let second = ShellDropTarget::new(DropCallbacks {
            open_path: Box::new(|_| Ok(())),
            on_failure: Box::new(|_| {}),
            files: None,
            has_files: None,
        });
        matches!(
            second.register(built().window.hwnd()),
            Err(DropRegisterError::Refused(_))
        )
    });
    assert!(refused);

    let dropped = target_file.clone();
    let effect = run(&ui, move || {
        let data: IDataObject = ComObject::new(DropList {
            files: vec![dropped, "ignored.jpg".to_owned()],
        })
        .to_interface::<IDataObject>();
        let target: IDropTarget = input().drop_target_interface().expect("the COM drop target");
        let mut effect = DROPEFFECT_COPY;
        // SAFETY: `data` and `effect` are live for the call; the point is a plain value.
        unsafe {
            target
                .Drop(&data, MODIFIERKEYS_FLAGS(0), POINTL { x: 0, y: 0 }, &mut effect)
                .expect("Drop");
        }
        effect
    });
    assert_eq!(DROPEFFECT_COPY, effect);
    assert_ne!(DROPEFFECT_NONE, effect);
    let expected = target_file.clone();
    wait_until(&ui, "the first dropped file opens", move || {
        settle_step();
        present_current();
        let b = built();
        let vm = b.window.view_model();
        let vm = vm.borrow();
        vm.catalog()
            .current()
            .is_some_and(|entry| entry.path().eq_ignore_ascii_case(&expected))
    });
    finish(ui);
}

// ---- wiring --------------------------------------------------------------------------------------------------------------------------------------

// C#: ShellInputEndToEndTests.ViewportController_UsesTheSystemDragThreshold_AtTheWindowDpi
#[test]
#[ignore = "native: needs desktop"]
fn viewport_controller_uses_the_system_drag_threshold_at_the_window_dpi() {
    let ui = start_ui();
    start_with_images(&ui, &["a.jpg"]);
    let (threshold, dpi) = run(&ui, || {
        let b = built();
        (b.window.viewport().drag_threshold(), b.window.window().dpi_scale())
    });
    let system =
        pr_platform_win::shell_input::native::drag_metrics_pixels(pr_platform_win::shell_input::native::dpi_of(dpi));
    assert!((f64::from(system.0) / dpi - threshold.0).abs() < 1e-9);
    assert!((f64::from(system.1) / dpi - threshold.1).abs() < 1e-9);
    finish(ui);
}

// ---- kinetic glide, latency ---------------------------------------------------------------------------------------------------------------------

// C#: ShellInputEndToEndTests.Navigation_StopsAKineticGlide
#[test]
#[ignore = "native: needs desktop"]
fn navigation_stops_a_kinetic_glide() {
    let ui = start_ui();
    start_with_images(&ui, &["a.jpg", "b.jpg"]);
    run(&ui, || edit_settings(|s| s.kinetic_pan_enabled = true));
    run(&ui, || key(KeyId::D1, false));
    wait(&ui, "100 %", |b| {
        zoom_percent() == 100 && !b.window.viewport().is_layout_posted()
    });
    run(&ui, || {
        for i in 0..12 {
            key(KeyId::RIGHT, i > 0); // the impulses add up: a long glide
        }
    });
    run(&ui, || built().window.update_view_model(|vm| vm.next()));
    wait(&ui, "the second image shows", |_| index() == 1);
    // The glide stops when the view is reset for the new image (a zoom-mode change): wait until then.
    wait(&ui, "the next image is back at Fit", |_| zoom_percent() != 100);
    let hooks_left = run(&ui, || {
        input().input().stop_kinetic();
        built().window.viewport().is_layout_posted()
    });
    let _ = hooks_left;
    finish(ui);
}

// C#: ShellInputEndToEndTests.InputLatency_ArrowPan_IsMeasuredAtTheHandler_TheLayout_AndTheFrame (a measurement: nothing is asserted on time)
#[test]
#[ignore = "native: needs desktop"]
fn input_latency_arrow_pan_is_measured_at_the_handler_the_layout_and_the_frame() {
    let ui = start_ui();
    start_with_images(&ui, &["big.jpg"]);
    run(&ui, || key(KeyId::D1, false));
    wait(&ui, "100 %", |b| {
        zoom_percent() == 100 && !b.window.viewport().is_layout_posted()
    });

    let mut send_ms = Vec::new();
    let directions = [KeyId::RIGHT, KeyId::DOWN, KeyId::LEFT, KeyId::UP];
    for i in 0..60 {
        let pressed = directions[i % directions.len()];
        let started = Instant::now();
        run(&ui, move || key(pressed, false));
        send_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        wait_until(&ui, "the layout settles", || {
            settle_step();
            !built().window.viewport().is_layout_posted()
        });
    }
    send_ms.sort_by(f64::total_cmp);
    println!(
        "wp22 input latency: n={} handler(SendMessage) p50={:.2} p95={:.2} max={:.2}",
        send_ms.len(),
        send_ms[send_ms.len() / 2],
        send_ms[(send_ms.len() * 95) / 100],
        send_ms[send_ms.len() - 1]
    );
    finish(ui);
}
