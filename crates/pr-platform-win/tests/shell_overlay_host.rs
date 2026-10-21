//! Pure tests for W8-15f overlay host and message handling.

#[path = "common/overlay_fakes.rs"]
mod overlay_fakes;

use std::cell::RefCell;
use std::rc::Rc;

use overlay_fakes::{
    FakeOverlayOs, FakeShellWindow, FakeTextRenderer, RecordingDrawContext, RecordingInput, RecordingTooltips,
};
use pr_app::input::frame_clock::FrameTick;
use pr_app::input::primitives::{PointD, SizeD};
use pr_app::overlay::animator::{Animator, OpacityStore};
use pr_app::overlay::button::ButtonElement;
use pr_app::overlay::contracts::{Easing, OverlayLayoutContext};
use pr_app::overlay::element::{ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment};
use pr_app::overlay::panel::PanelElement;
use pr_app::overlay::text::TextElement;
use pr_platform_win::host::contracts::{WindowMessage, WindowMessageHandler};
use pr_platform_win::shell_overlay::host::{OverlayHost, OverlayHostError};
use pr_platform_win::shell_overlay::messages::{OverlayMessageHandler, high_word, low_word};
use pr_platform_win::shell_overlay::native::{VK_SHIFT, WM_MOUSELEAVE, key_state_is_down};
use pr_platform_win::win32::messages::*;

fn corner(id: &str, h: OverlayHorizontalAlignment, v: OverlayVerticalAlignment, caption: &str) -> PanelElement {
    let mut panel = PanelElement::new(id);
    panel.base_mut().horizontal_alignment = h;
    panel.base_mut().vertical_alignment = v;
    panel.base_mut().margin = OverlayThickness::uniform(8.0);
    panel.padding = OverlayThickness::symmetric(6.0, 4.0);
    let mut label = TextElement::new(&format!("{id}.Text")).unwrap();
    label.set_text(caption);
    panel.add(Box::new(label));
    panel
}

fn context(renderer: &FakeTextRenderer) -> OverlayLayoutContext<'_> {
    OverlayLayoutContext::with_renderer_id(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        1.0,
        renderer,
        1,
    )
}

fn inside(node: &dyn ElementNode) -> PointD {
    let bounds = node.base().bounds();
    PointD {
        x: bounds.x + 1.0,
        y: bounds.y + 1.0,
    }
}

// C#: OverlayHostTests.Add_RejectsDuplicateIds_AcrossTrees
#[test]
fn add_rejects_duplicate_ids_across_trees() {
    let mut host = OverlayHost::new();
    host.add(Box::new(corner(
        "StatusPanel",
        OverlayHorizontalAlignment::Left,
        OverlayVerticalAlignment::Bottom,
        "a",
    )))
    .unwrap();
    assert_eq!(
        host.add(Box::new(PanelElement::new("StatusPanel"))),
        Err(OverlayHostError::DuplicateId("StatusPanel".into()))
    );
    assert_eq!(
        host.add(Box::new(PanelElement::new("StatusPanel.Text"))),
        Err(OverlayHostError::DuplicateId("StatusPanel.Text".into()))
    );
    let mut duplicate_child = PanelElement::new("NewRoot");
    duplicate_child.add(Box::new(PanelElement::new("StatusPanel")));
    assert!(host.add(Box::new(duplicate_child)).is_ok());
}

// C#: OverlayHostTests.HitTest_ReturnsTopmostDeepestElement_AndNullOverImage
#[test]
fn hit_test_returns_topmost_deepest_element_and_none_over_image() {
    let renderer = FakeTextRenderer::new();
    let mut host = OverlayHost::new();
    let status = corner(
        "StatusPanel",
        OverlayHorizontalAlignment::Left,
        OverlayVerticalAlignment::Bottom,
        "status",
    );
    let mut toolbar = PanelElement::new("ToolbarPanel");
    toolbar.base_mut().margin = OverlayThickness::uniform(8.0);
    toolbar.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    toolbar.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    let mut button = ButtonElement::new("Fit").unwrap();
    button.set_content("Fit");
    toolbar.add(Box::new(button));
    host.add(Box::new(status)).unwrap();
    host.add(Box::new(toolbar)).unwrap();
    host.arrange(&context(&renderer));
    let button_point = inside(host.find("Fit").unwrap());
    assert_eq!(host.hit_test(button_point).unwrap().base().id(), "Fit");
    let status_point = inside(host.find("StatusPanel").unwrap());
    assert_eq!(host.hit_test(status_point).unwrap().base().id(), "StatusPanel");
    assert!(host.hit_test(PointD { x: 400.0, y: 300.0 }).is_none());
}

// C#: OverlayHostTests.HitTest_SkipsHiddenAndZeroOpacityElements_SoClicksFallThrough
#[test]
fn hit_test_skips_hidden_and_zero_opacity_elements_so_clicks_fall_through() {
    let renderer = FakeTextRenderer::new();
    let mut host = OverlayHost::new();
    host.add(Box::new(corner(
        "ZoomIndicatorPanel",
        OverlayHorizontalAlignment::Right,
        OverlayVerticalAlignment::Top,
        "100 %",
    )))
    .unwrap();
    host.arrange(&context(&renderer));
    let point = inside(host.find("ZoomIndicatorPanel").unwrap());
    assert!(host.hit_test(point).is_some());
    host.root_mut::<PanelElement>("ZoomIndicatorPanel")
        .unwrap()
        .base_mut()
        .set_opacity(0.0);
    assert!(host.hit_test(point).is_none());
    let panel = host.root_mut::<PanelElement>("ZoomIndicatorPanel").unwrap();
    panel.base_mut().set_opacity(1.0);
    panel.base_mut().is_visible = false;
    assert!(host.hit_test(point).is_none());
    let panel = host.root_mut::<PanelElement>("ZoomIndicatorPanel").unwrap();
    panel.base_mut().is_visible = true;
    panel.base_mut().is_hit_test_visible = false;
    assert!(host.hit_test(point).is_none());
}

// C#: OverlayHostTests.Render_DrawsRootsInInsertionOrder
#[test]
fn render_draws_roots_in_insertion_order() {
    let renderer = FakeTextRenderer::new();
    let mut host = OverlayHost::new();
    host.add(Box::new(corner(
        "First",
        OverlayHorizontalAlignment::Left,
        OverlayVerticalAlignment::Top,
        "one",
    )))
    .unwrap();
    host.add(Box::new(corner(
        "Second",
        OverlayHorizontalAlignment::Right,
        OverlayVerticalAlignment::Top,
        "two",
    )))
    .unwrap();
    host.arrange(&context(&renderer));
    let mut dc = RecordingDrawContext::new();
    host.render(&mut dc);
    assert_eq!(
        dc.texts.iter().map(|entry| entry.0.as_str()).collect::<Vec<_>>(),
        ["one", "two"]
    );
}

// C#: OverlayHostTests.Find_And_Resolve_WalkTheWholeTree
#[test]
fn find_and_opacity_store_walk_the_tree() {
    let mut host = OverlayHost::new();
    host.add(Box::new(corner(
        "FolderInfoPanel",
        OverlayHorizontalAlignment::Right,
        OverlayVerticalAlignment::Bottom,
        "x",
    )))
    .unwrap();
    assert!(host.find("FolderInfoPanel").is_some());
    assert!(host.find("FolderInfoPanel.Text").is_some());
    assert!(host.find("Missing").is_none());
    assert_eq!(host.opacity_store().opacity("FolderInfoPanel"), Some(1.0));
    assert_eq!(host.opacity_store().opacity("FolderInfoPanel.Text"), None);
}

// C#: OverlayHostTests.AnimatorOnHost_FadesPanelAndStopsHitTestingAtZero
#[test]
fn animator_on_host_fades_panel_and_stops_hit_testing_at_zero() {
    let renderer = FakeTextRenderer::new();
    let mut host = OverlayHost::new();
    host.add(Box::new(corner(
        "ToolbarPanel",
        OverlayHorizontalAlignment::Left,
        OverlayVerticalAlignment::Top,
        "tools",
    )))
    .unwrap();
    host.arrange(&context(&renderer));
    let point = inside(host.find("ToolbarPanel").unwrap());
    let mut animator = Animator::new(1000).unwrap();
    animator
        .fade_to(
            &mut host.opacity_store(),
            "ToolbarPanel",
            0.0,
            2_000_000,
            Easing::Linear,
        )
        .unwrap();
    for timestamp in [0, 100] {
        animator.on_frame(
            &mut host.opacity_store(),
            &FrameTick {
                timestamp,
                rendering_time_ms: 0.0,
                timing: None,
            },
        );
    }
    assert!((host.root::<PanelElement>("ToolbarPanel").unwrap().base().opacity() - 0.5).abs() < 0.001);
    assert!(host.hit_test(point).is_some());
    animator.on_frame(
        &mut host.opacity_store(),
        &FrameTick {
            timestamp: 200,
            rendering_time_ms: 0.0,
            timing: None,
        },
    );
    assert_eq!(host.root::<PanelElement>("ToolbarPanel").unwrap().base().opacity(), 0.0);
    assert!(host.hit_test(point).is_none());
}

// C#: OverlayHostTests.Dispose_ReleasesEveryTextLayout
#[test]
fn dispose_releases_every_text_layout() {
    let renderer = FakeTextRenderer::new();
    let mut host = OverlayHost::new();
    host.add(Box::new(corner(
        "A",
        OverlayHorizontalAlignment::Left,
        OverlayVerticalAlignment::Top,
        "one",
    )))
    .unwrap();
    host.add(Box::new(corner(
        "B",
        OverlayHorizontalAlignment::Right,
        OverlayVerticalAlignment::Top,
        "two",
    )))
    .unwrap();
    host.arrange(&context(&renderer));
    assert_eq!(renderer.live(), 2);
    host.dispose();
    host.dispose();
    assert_eq!(renderer.live(), 0);
    assert!(host.is_disposed());
    assert_eq!(
        host.add(Box::new(PanelElement::new("C"))),
        Err(OverlayHostError::Disposed)
    );
}

// C#: OverlayHostTests.HitTest_PrefersTheLaterRoot_WhereRootsOverlap
#[test]
fn hit_test_prefers_the_later_root_where_roots_overlap() {
    let renderer = FakeTextRenderer::new();
    let mut host = OverlayHost::new();
    host.add(Box::new(PanelElement::new("Under"))).unwrap();
    host.add(Box::new(PanelElement::new("Over"))).unwrap();
    host.arrange(&context(&renderer));
    assert_eq!(
        host.hit_test(PointD { x: 100.0, y: 100.0 }).unwrap().base().id(),
        "Over"
    );
}

// new: root-only opacity mutation and typed lookup
#[test]
fn host_opacity_store_reads_and_writes_roots_only() {
    let mut host = OverlayHost::new();
    host.add(Box::new(corner(
        "root",
        OverlayHorizontalAlignment::Left,
        OverlayVerticalAlignment::Top,
        "text",
    )))
    .unwrap();
    host.opacity_store().set_opacity("root.Text", 0.0);
    assert_eq!(host.find("root.Text").unwrap().base().opacity(), 1.0);
    host.opacity_store().set_opacity("root", 0.25);
    assert_eq!(host.root::<PanelElement>("root").unwrap().base().opacity(), 0.25);
    assert!(host.root::<ButtonElement>("root").is_none());
}

// new: remove by root id and disposed add
#[test]
fn host_add_after_dispose_fails_and_remove_still_works() {
    let mut host = OverlayHost::new();
    host.add(Box::new(PanelElement::new("root"))).unwrap();
    assert!(host.remove("missing").is_none());
    assert_eq!(host.remove("root").unwrap().base().id(), "root");
    host.dispose();
    assert_eq!(
        host.add(Box::new(PanelElement::new("later"))),
        Err(OverlayHostError::Disposed)
    );
    assert!(host.remove("later").is_none());
}

struct Rig {
    handler: Rc<OverlayMessageHandler>,
    input: Rc<RecordingInput>,
    window: Rc<FakeShellWindow>,
    os: Rc<FakeOverlayOs>,
    tips: Rc<RecordingTooltips>,
}

fn rig() -> Rig {
    let input = Rc::new(RecordingInput::default());
    let window = Rc::new(FakeShellWindow::default());
    let os = Rc::new(FakeOverlayOs::default());
    let tips = Rc::new(RecordingTooltips::default());
    let handler = Rc::new(OverlayMessageHandler::new(
        input.clone(),
        window.clone(),
        Some(tips.clone()),
        os.clone(),
    ));
    Rig {
        handler,
        input,
        window,
        os,
        tips,
    }
}

fn message(msg: u32, lparam: isize) -> WindowMessage {
    WindowMessage {
        hwnd: 7,
        msg,
        wparam: 0,
        lparam,
    }
}

fn xy(x: i16, y: i16) -> isize {
    ((y as u16 as isize) << 16) | (x as u16 as isize)
}

// C#: OverlayPartsTests.MessageHandler_ConvertsPhysicalPixelsToDipsWithTheWindowDpi
#[test]
fn message_handler_converts_physical_pixels_to_dips_with_the_window_dpi() {
    let rig = rig();
    rig.window.dpi.set(2.0);
    assert_eq!(rig.handler.try_handle(&message(WM_MOUSEMOVE, xy(40, 60))), None);
    assert_eq!(rig.input.points.borrow().as_slice(), &[PointD { x: 20.0, y: 30.0 }]);
}

// new: TrackMouseEvent request is once per leave cycle
#[test]
fn mouse_move_requests_leave_once_until_the_leave_message() {
    let rig = rig();
    rig.handler.try_handle(&message(WM_MOUSEMOVE, 0));
    rig.handler.try_handle(&message(WM_MOUSEMOVE, 0));
    assert_eq!(rig.os.leave_requests.borrow().as_slice(), &[7]);
    rig.handler.try_handle(&message(WM_MOUSELEAVE, 0));
    rig.handler.try_handle(&message(WM_MOUSEMOVE, 0));
    assert_eq!(rig.os.leave_requests.borrow().as_slice(), &[7, 7]);
    assert!(rig.input.calls.borrow().contains(&"leave".into()));
}

// new: accepted press owns capture and the message
#[test]
fn left_down_on_overlay_is_swallowed_and_captures() {
    let rig = rig();
    rig.input.accept_down.set(true);
    assert_eq!(rig.handler.try_handle(&message(WM_LBUTTONDOWN, 0)), Some(0));
    assert_eq!(rig.window.capture_calls.get(), 1);
    assert!(rig.window.captured.get());
}

// new: image press remains available to later handlers
#[test]
fn left_down_outside_is_not_swallowed_and_does_not_capture() {
    let rig = rig();
    assert_eq!(rig.handler.try_handle(&message(WM_LBUTTONDBLCLK, 0)), None);
    assert_eq!(rig.window.capture_calls.get(), 0);
}

// new: a stray release does not reach the tooltip
#[test]
fn left_up_without_a_press_returns_none_before_relaying() {
    let rig = rig();
    assert_eq!(rig.handler.try_handle(&message(WM_LBUTTONUP, 0)), None);
    assert!(rig.tips.relays.borrow().is_empty());
}

// new: move, down, up each relay with the original Win32 arguments
#[test]
fn tooltip_relay_order_for_move_down_up() {
    let rig = rig();
    let timeline = Rc::new(RefCell::new(Vec::new()));
    *rig.input.timeline.borrow_mut() = Some(timeline.clone());
    *rig.tips.timeline.borrow_mut() = Some(timeline.clone());
    *rig.window.timeline.borrow_mut() = Some(timeline.clone());
    rig.input.accept_down.set(true);
    rig.input.accept_up.set(true);
    for msg in [WM_MOUSEMOVE, WM_LBUTTONDOWN, WM_LBUTTONUP] {
        rig.handler.try_handle(&WindowMessage {
            wparam: 3,
            ..message(msg, 4)
        });
    }
    assert_eq!(
        rig.tips.relays.borrow().as_slice(),
        &[(WM_MOUSEMOVE, 3, 4), (WM_LBUTTONDOWN, 3, 4), (WM_LBUTTONUP, 3, 4)]
    );
    assert_eq!(rig.input.calls.borrow().as_slice(), &["move", "down", "up"]);
    assert_eq!(
        timeline.borrow().as_slice(),
        &["move", "relay", "relay", "down", "relay", "release", "up"]
    );
}

// new: ReleaseCapture synchronously re-enters with WM_CAPTURECHANGED
#[test]
fn release_inside_up_does_not_cancel_the_press() {
    let rig = rig();
    rig.input.accept_down.set(true);
    rig.input.accept_up.set(true);
    rig.handler.try_handle(&message(WM_LBUTTONDOWN, 0));
    let handler = rig.handler.clone();
    *rig.window.on_release.borrow_mut() = Some(Box::new(move || {
        handler.try_handle(&message(WM_CAPTURECHANGED, 99));
    }));
    assert_eq!(rig.handler.try_handle(&message(WM_LBUTTONUP, 0)), Some(0));
    assert_eq!(rig.input.calls.borrow().as_slice(), &["down", "up"]);
    assert_eq!(rig.window.release_calls.get(), 1);
}

// new: a different window gaining capture cancels the overlay press
#[test]
fn capture_stolen_by_another_window_cancels_the_press() {
    let rig = rig();
    rig.input.accept_down.set(true);
    rig.handler.try_handle(&message(WM_LBUTTONDOWN, 0));
    assert_eq!(rig.handler.try_handle(&message(WM_CAPTURECHANGED, 99)), None);
    assert!(!rig.input.down.get());
    assert!(rig.input.calls.borrow().contains(&"cancel".into()));
}

// new: capture changed to the same HWND is not theft
#[test]
fn capture_changed_to_the_window_itself_does_not_cancel() {
    let rig = rig();
    rig.input.accept_down.set(true);
    rig.handler.try_handle(&message(WM_LBUTTONDOWN, 0));
    rig.handler.try_handle(&message(WM_CAPTURECHANGED, 7));
    assert!(rig.input.down.get());
}

// new: wheel lParam is screen pixels, including negative monitor coordinates
#[test]
fn wheel_converts_screen_to_client_and_swallows_only_over_a_panel() {
    let rig = rig();
    rig.window.dpi.set(2.0);
    rig.window.screen_offset.set(PointD { x: -100.0, y: -200.0 });
    assert_eq!(rig.handler.try_handle(&message(WM_MOUSEWHEEL, xy(-80, -180))), None);
    rig.input.accept_wheel.set(true);
    assert_eq!(rig.handler.try_handle(&message(WM_MOUSEHWHEEL, xy(-80, -180))), Some(0));
    assert_eq!(rig.input.points.borrow().as_slice(), &[PointD { x: 10.0, y: 10.0 }; 2]);
}

// new: previous key state bit and Shift are passed independently
#[test]
fn keys_report_shift_and_repeat_bit() {
    let rig = rig();
    rig.os.shift.set(true);
    rig.input.accept_key.set(true);
    for (msg, lp) in [(WM_KEYDOWN, 0), (WM_SYSKEYDOWN, 1 << 30)] {
        assert_eq!(
            rig.handler.try_handle(&WindowMessage {
                wparam: 0x10009,
                ..message(msg, lp)
            }),
            Some(0)
        );
    }
    assert_eq!(rig.input.keys.borrow().as_slice(), &[(9, true, false), (9, true, true)]);
    assert_eq!(rig.os.queried_keys.borrow().as_slice(), &[VK_SHIFT, VK_SHIFT]);
}

// new: C# casts each word through signed short
#[test]
fn low_word_high_word_are_signed() {
    assert_eq!(low_word(0xFFFF), -1);
    assert_eq!(high_word(0xFFFF0000_u32 as isize), -1);
    assert_eq!((low_word(-1), high_word(-1)), (-1, -1));
}

// new: C# uses one only for dpi <= 0
#[test]
fn dpi_zero_is_treated_as_one() {
    let rig = rig();
    rig.window.dpi.set(0.0);
    rig.handler.try_handle(&message(WM_MOUSEMOVE, xy(10, 20)));
    assert_eq!(rig.input.points.borrow().as_slice(), &[PointD { x: 10.0, y: 20.0 }]);
}

// new: unrelated messages remain available to the rest of the chain
#[test]
fn handler_ignores_other_messages() {
    let rig = rig();
    assert_eq!(rig.handler.try_handle(&message(WM_PAINT, 0)), None);
    assert!(rig.input.calls.borrow().is_empty());
}

// new: GetKeyState uses the high bit, not the toggle bit
#[test]
fn key_state_is_down_table() {
    for state in [i16::MIN, -1] {
        assert!(key_state_is_down(state));
    }
    for state in [0, 1, i16::MAX] {
        assert!(!key_state_is_down(state));
    }
}
