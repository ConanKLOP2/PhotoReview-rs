//! W8-15i drawing and tooltip regions of `ShellOverlay`: `render`, `collect_tooltips`, `sync_tooltips`.
//! Pure tests use the `RecordingDrawContext` and `FakeTooltips`; the native tests (`#[ignore = "native: needs desktop"]`) draw on a WARP offscreen surface
//! and feed a real tooltip control owned by a hidden `STATIC` popup that is never shown. The manual perf report prints numbers and asserts nothing.
#![cfg(windows)]

#[path = "common/overlay_fakes.rs"]
mod overlay_fakes;
#[path = "common/overlay_layout_fixture.rs"]
mod overlay_layout_fixture;
#[path = "common/overlay_main_window_fixture.rs"]
mod overlay_main_window_fixture;

use std::rc::Rc;
use std::sync::Mutex;

use overlay_fakes::RecordingDrawContext;
use overlay_main_window_fixture::{FAR_AWAY, InputRig};
use pr_app::input::primitives::SizeD;
use pr_app::overlay::capture_badge;
use pr_app::overlay::status;
use pr_app::viewport::render_geometry::color_from_rgb;
use pr_platform_win::render::surface::D2dRenderSurface;
use pr_platform_win::render::text::renderer::DWriteTextRenderer;
use pr_platform_win::shell_overlay::overlay::state::OverlayButton;
use pr_platform_win::shell_overlay::tooltips::{NativeTooltipHost, OverlayTooltips, TTM_GETTOOLINFO, tool_rect_pixels};
use pr_platform_win::win32::activation_context::ActivationScope;
use pr_platform_win::win32::api::{kernel32, user32};
use pr_platform_win::win32::comctl::SHELL_MANIFEST;
use pr_platform_win::win32::dialog_native::user32 as dialog_user32;
use pr_platform_win::win32::messages::WS_POPUP;
use pr_platform_win::win32::structs::{Hwnd, Lparam};
use windows::Win32::UI::Controls as wc;

const CLIENT: SizeD = SizeD {
    width: 1000.0,
    height: 700.0,
};

fn full_rig() -> InputRig {
    let rig = InputRig::new(true, false);
    rig.full_model();
    rig
}

fn draw(rig: &InputRig) -> RecordingDrawContext {
    let mut dc = RecordingDrawContext::new();
    rig.overlay.render(&mut dc, CLIENT, 1.0);
    dc
}

fn text_index(dc: &RecordingDrawContext, text: &str) -> usize {
    dc.texts
        .iter()
        .position(|(drawn, ..)| drawn == text)
        .unwrap_or_else(|| {
            panic!(
                "{text:?} was not drawn; drawn: {:?}",
                dc.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
            )
        })
}

// Render ---------------------------------------------------------------------------------------------------------------

// C#: ShellOverlayBehaviorTests.Render_DrawsInfoLayerBeforeTheToolbar_SoTheToolbarIsOnTop
#[test]
fn render_draws_info_layer_before_the_toolbar_so_the_toolbar_is_on_top() {
    let rig = full_rig();
    let dc = draw(&rig);
    let fit = rig.overlay.with_toolbar(|t| t.fit().content().to_owned());
    assert!(text_index(&dc, "150%") < text_index(&dc, &fit));
    assert_eq!(dc.clip_depth(), 0);
}

// C#: ShellOverlayBehaviorTests.Render_FadedOutToolbarIsNotDrawn
#[test]
fn render_faded_out_toolbar_is_not_drawn() {
    let rig = full_rig();
    rig.overlay.pointer_moved(FAR_AWAY);
    rig.advance(1500 + 1 + 200);
    assert_eq!(rig.overlay.toolbar_opacity(), 0.0);
    let dc = draw(&rig);
    let fit = rig.overlay.with_toolbar(|t| t.fit().content().to_owned());
    assert!(dc.texts.iter().all(|(drawn, ..)| *drawn != fit));
    // The info layer is still on screen (its delay is longer).
    text_index(&dc, "150%");
}

// new: render arranges at the given size and DPI and syncs the tooltips once per frame
#[test]
fn render_arranges_and_syncs_the_tooltips_every_frame() {
    let rig = full_rig();
    let before = rig.tooltips.sync_count.get();
    let mut dc = RecordingDrawContext::new();
    rig.overlay.render(
        &mut dc,
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        1.5,
    );
    assert_eq!(rig.overlay.last_client().width, 800.0);
    assert_eq!(rig.overlay.last_dpi(), 1.5);
    assert_eq!(rig.tooltips.sync_count.get(), before + 1);
    assert_eq!(rig.tooltips.dpi.get(), 1.5);
    rig.overlay.render(
        &mut dc,
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        1.5,
    );
    assert_eq!(rig.tooltips.sync_count.get(), before + 2); // T5: unconditional, the control decides it is unchanged
}

// new: render_after_dispose_draws_nothing_and_does_not_sync
#[test]
fn render_after_dispose_draws_nothing_and_does_not_sync() {
    let rig = full_rig();
    rig.overlay.dispose();
    let before = rig.tooltips.sync_count.get();
    let dc = draw(&rig);
    assert!(dc.calls.is_empty());
    assert_eq!(rig.tooltips.sync_count.get(), before);
}

// Tooltips -------------------------------------------------------------------------------------------------------------

// C#: ShellOverlayBehaviorTests.Tooltips_ToolbarButtonsGetTheirLocalizedTextAtTheirBoxes
#[test]
fn tooltips_toolbar_buttons_get_their_localized_text_at_their_boxes() {
    let rig = InputRig::new(true, false);
    draw(&rig);
    let tools = rig.tooltips.tools.borrow();
    let expected = [
        OverlayButton::OpenFolder,
        OverlayButton::Settings,
        OverlayButton::Fit,
        OverlayButton::Tools,
    ];
    assert_eq!(tools.len(), 4);
    for (tool, button) in tools.iter().zip(expected) {
        assert_eq!(tool.id, button.element_id());
        assert_eq!(tool.bounds, rig.bounds_of(button.element_id()));
        assert!(!tool.text.is_empty());
    }
    let open_folder_text = rig.overlay.with_toolbar(|t| t.open_folder().tool_tip.clone());
    assert_eq!(tools[0].text, open_folder_text);
}

// C#: ShellOverlayBehaviorTests.Tooltips_HiddenOverlaysHaveNone_ExifAndBadgeComeFromTheModel
#[test]
fn tooltips_hidden_overlays_have_none_exif_and_badge_come_from_the_model() {
    let rig = full_rig();
    draw(&rig);
    {
        let tools = rig.tooltips.tools.borrow();
        let ids: Vec<&str> = tools.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids.len(), 6);
        assert_eq!(&ids[4..], [status::EXIF_TEXT_ID, capture_badge::PANEL_ID]);
        assert_eq!(tools[4].text, "ISO 100");
        assert_eq!(tools[5].text, "Capture pair");
    }
    rig.overlay.pointer_moved(FAR_AWAY);
    rig.advance(1500 + 3000 + 600);
    draw(&rig);
    assert!(rig.tooltips.tools.borrow().is_empty());
}

// C#: OverlayPartsTests.Tooltips_ToolbarOffersNoneAsSoonAsItStopsReceivingTheMouse_EvenWhileStillFading
#[test]
fn tooltips_toolbar_offers_none_as_soon_as_it_stops_receiving_the_mouse_even_while_still_fading() {
    let rig = full_rig();
    rig.overlay.pointer_moved(FAR_AWAY);
    rig.advance(1500); // the fade-out starts: click-through at once, opacity still 1
    assert!(!rig.overlay.toolbar_hit_test_visible());
    assert!(rig.overlay.toolbar_opacity() > 0.0);
    let tools = rig.overlay.collect_tooltips();
    assert!(
        tools.iter().all(|t| OverlayButton::from_element_id(&t.id).is_none()),
        "{tools:?}"
    );
}

// new: collect_tooltips_order_is_toolbar_exif_badge
#[test]
fn collect_tooltips_order_is_toolbar_exif_badge() {
    let rig = full_rig();
    rig.overlay.arrange(CLIENT, 1.0);
    let ids: Vec<String> = rig.overlay.collect_tooltips().into_iter().map(|t| t.id).collect();
    assert_eq!(ids.len(), 6);
    assert!(OverlayButton::from_element_id(&ids[3]).is_some());
    assert_eq!(ids[4], status::EXIF_TEXT_ID);
    assert_eq!(ids[5], capture_badge::PANEL_ID);
}

// new: collect_tooltips_skips_empty_tooltips
#[test]
fn collect_tooltips_skips_empty_tooltips() {
    let rig = full_rig();
    rig.update(|m| {
        m.exif_text = String::new();
        m.capture_pair_tool_tip = String::new();
    });
    rig.overlay.arrange(CLIENT, 1.0);
    assert_eq!(rig.overlay.collect_tooltips().len(), 4);
    rig.update(|m| {
        m.exif_text = "ISO 200".to_owned();
        m.exif_visible = false;
        m.capture_pair_visible = false;
        m.capture_pair_tool_tip = "x".to_owned();
    });
    rig.overlay.arrange(CLIENT, 1.0);
    assert_eq!(rig.overlay.collect_tooltips().len(), 4); // invisible EXIF line and badge offer none
}

// new: collect_tooltips after dispose is empty
#[test]
fn collect_tooltips_after_dispose_is_empty() {
    let rig = full_rig();
    rig.overlay.dispose();
    assert!(rig.overlay.collect_tooltips().is_empty());
}

// C#: ShellOverlayBehaviorTests.StatusPanel_SkippedRowAndWarningFollowTheModel_ViewListButtonClicks (draw half)
#[test]
fn status_panel_skipped_warning_is_drawn_from_the_model() {
    let rig = full_rig();
    let dc = draw(&rig);
    text_index(&dc, "2 skipped");
    text_index(&dc, "3 / 10");
}

// Native ---------------------------------------------------------------------------------------------------------------

static NATIVE_LOCK: Mutex<()> = Mutex::new(());

/// Comctl32 v6 active on this thread plus a hidden `STATIC` popup owner, destroyed on drop.
struct Env {
    owner: Hwnd,
    _scope: ActivationScope,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl Env {
    fn new() -> Self {
        let guard = NATIVE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let scope = ActivationScope::from_manifest_text(SHELL_MANIFEST).expect("activation scope");
        let module = kernel32::get_module_handle().expect("module");
        // SAFETY: the system `STATIC` class has no creation parameter (`param` 0); the window is a hidden popup that is never shown.
        let owner =
            unsafe { user32::create_window_ex(0, "STATIC", "owner", WS_POPUP, 0, 0, 200, 100, 0, 0, module, 0) }
                .expect("owner window");
        Self {
            owner,
            _scope: scope,
            _guard: guard,
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _destroyed = user32::destroy_window(self.owner);
    }
}

fn dwrite_rig() -> InputRig {
    let text = Rc::new(DWriteTextRenderer::new(Some("en-US"), "", 256).expect("DirectWrite"));
    let rig = InputRig::with_text(true, false, text, 7);
    rig.full_model();
    rig
}

#[test]
#[ignore = "native: needs desktop"]
fn hud_renders_on_a_warp_offscreen_surface_and_the_toolbar_pixels_differ_from_the_clear_colour() {
    let rig = dwrite_rig();
    let dpi = 1.5;
    let (width, height) = (1200, 800);
    let mut surface = D2dRenderSurface::create_offscreen_surface(width, height, dpi, false).expect("WARP surface");
    let context = surface.begin_draw_context().expect("begin draw");
    context.clear(color_from_rgb(255, 255, 255, 255));
    rig.overlay.render(
        context,
        SizeD {
            width: f64::from(width) / dpi,
            height: f64::from(height) / dpi,
        },
        dpi,
    );
    let pixels = context.try_read_pixels().expect("read").expect("pixels");
    let bytes = pixels.bytes().expect("bytes");
    let stride = pixels.stride() as usize;
    surface.end_draw_and_present().expect("end draw");

    let bar = rig.bounds_of("ToolbarPanel");
    let (left, top, right, bottom) = tool_rect_pixels(bar, dpi);
    let inside = |x: i32, y: i32| {
        let offset = y as usize * stride + x as usize * 4;
        [bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]
    };
    // Just inside the corner of the panel (rounded corners and the border are outside this probe).
    let probe = inside(left + 4, top + (bottom - top) / 2);
    assert_ne!(probe, [255, 255, 255, 255], "toolbar pixel is still the clear colour");
    // A point far from every overlay element keeps the clear colour.
    assert_eq!(inside(right + 200, bottom + 200), [255, 255, 255, 255]);
    assert_eq!(rig.tooltips.tools.borrow().len(), 6);
}

#[test]
#[ignore = "native: needs desktop"]
fn tooltip_regions_reach_a_native_control_in_physical_pixels() {
    let env = Env::new();
    let host = Rc::new(NativeTooltipHost::new(env.owner).expect("tooltip host"));
    // An overlay whose tooltip service is the real control: same options as the fixture rig, built by hand through the rig then re-synced by hand.
    let rig = full_rig();
    let mut dc = RecordingDrawContext::new();
    rig.overlay.render(
        &mut dc,
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        1.5,
    );
    let tools = rig.tooltips.tools.borrow().clone();
    assert_eq!(tools.len(), 6);
    host.sync(&tools, rig.tooltips.dpi.get());
    assert_eq!(host.tool_count(), tools.len());
    for (index, tool) in tools.iter().enumerate() {
        assert_eq!(host.text_of(index), tool.text);
        let mut buffer = [0u16; 512];
        let mut info = wc::TTTOOLINFOW {
            cbSize: size_of::<wc::TTTOOLINFOW>() as u32,
            hwnd: windows::Win32::Foundation::HWND(env.owner as *mut core::ffi::c_void),
            uId: index + 1,
            lpszText: windows::core::PWSTR(buffer.as_mut_ptr()),
            ..Default::default()
        };
        // SAFETY: TTM_GETTOOLINFOW takes a `TTTOOLINFOW` in lParam; `info` is a live local with `cbSize` set and `lpszText` pointing at a 512-unit buffer.
        let _found = unsafe {
            dialog_user32::send_message(
                host.handle(),
                TTM_GETTOOLINFO,
                0,
                &mut info as *mut wc::TTTOOLINFOW as Lparam,
            )
        };
        let expected = tool_rect_pixels(tool.bounds, 1.5);
        assert_eq!(
            (info.rect.left, info.rect.top, info.rect.right, info.rect.bottom),
            expected,
            "{}",
            tool.id
        );
    }
    host.dispose();
}

#[test]
#[ignore = "manual: perf report"]
fn hud_frame_full_overlay_1920x1080_dpi_1_5() {
    let rig = dwrite_rig();
    let dpi = 1.5;
    let mut surface = D2dRenderSurface::create_offscreen_surface(1920, 1080, dpi, false).expect("WARP surface");
    let client = SizeD {
        width: 1920.0 / dpi,
        height: 1080.0 / dpi,
    };
    let mut samples = Vec::new();
    for _ in 0..50 {
        let started = std::time::Instant::now();
        let context = surface.begin_draw_context().expect("begin draw");
        context.clear(color_from_rgb(0, 0, 0, 255));
        rig.overlay.render(context, client, dpi);
        surface.end_draw_and_present().expect("end draw");
        samples.push(started.elapsed());
    }
    samples.sort();
    println!(
        "hud frame: min {:?}, median {:?}, p95 {:?}",
        samples[0],
        samples[samples.len() / 2],
        samples[samples.len() * 95 / 100]
    );
}
