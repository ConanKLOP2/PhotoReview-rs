//! Fixture of the frame-loop tests (W8-13i): a window surface on the hidden test window (WARP, a real swap chain, so the present seam is reachable), a
//! feature whose `draw_overlay` runs a closure, and the fault seams of the surface. Built ON TOP of `main_window_fixture.rs` (13g); nothing there changes.
#![allow(dead_code)] // each test crate of the frame tests uses a different part

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pr_app::services::render::DrawContext;
use pr_platform_win::gfx::com::{D2DERR_RECREATE_TARGET_HRESULT, DXGI_STATUS_OCCLUDED_HRESULT};
use pr_platform_win::host::main_window::core::{MainWindow, MainWindowFeature, RenderSurfaceFactoryHook};
use pr_platform_win::render::device::RenderFault;
use pr_platform_win::render::surface::{D2dRenderSurface, SurfaceOptions};
use pr_platform_win::win32::structs::Hwnd;

use crate::main_window_fixture::EventLog;

/// A surface hook that makes a real WARP window surface (swap chain) on the window handle.
pub struct WindowSurfaceHook;

impl RenderSurfaceFactoryHook for WindowSurfaceHook {
    fn create(&self, hwnd: Hwnd, options: SurfaceOptions) -> Result<D2dRenderSurface, RenderFault> {
        D2dRenderSurface::create_window_surface(
            hwnd,
            SurfaceOptions {
                prefer_warp: true,
                ..options
            },
        )
    }
}

/// A closure run by the overlay hook of [`HookFeature`].
pub type OverlayHook = Box<dyn Fn()>;

/// A feature whose `draw_overlay` appends `name` to the shared log and runs an optional hook (a test looks at the window from inside a frame).
pub struct HookFeature {
    pub name: &'static str,
    pub log: EventLog,
    pub overlay_calls: Cell<u32>,
    pub hook: RefCell<Option<OverlayHook>>,
    pub presses: Cell<u32>,
}

impl HookFeature {
    pub fn new(name: &'static str, log: EventLog) -> Rc<Self> {
        Rc::new(Self {
            name,
            log,
            overlay_calls: Cell::new(0),
            hook: RefCell::new(None),
            presses: Cell::new(0),
        })
    }
}

impl std::fmt::Debug for HookFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookFeature")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl MainWindowFeature for HookFeature {
    fn name(&self) -> &'static str {
        self.name
    }

    fn draw_overlay(&self, _context: &mut dyn DrawContext) {
        self.overlay_calls.set(self.overlay_calls.get() + 1);
        self.log.borrow_mut().push(format!("{}:overlay", self.name));
        if let Some(hook) = self.hook.borrow().as_ref() {
            hook();
        }
    }

    fn on_scroll_bar_press(&self) {
        self.presses.set(self.presses.get() + 1);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Makes the next `EndDraw` report a lost render target (once): the surface recreates its device and answers `DeviceRecreated`.
pub fn inject_device_loss(window: &MainWindow) {
    window
        .with_surface(|surface| {
            let mut injected = false;
            surface.resources().set_end_draw_fault(Some(Box::new(move |hr| {
                if injected {
                    return hr;
                }
                injected = true;
                D2DERR_RECREATE_TARGET_HRESULT
            })));
        })
        .expect("a surface that is not being drawn");
}

/// Makes every `EndDraw` fail with `E_FAIL` (the surface answers `Failed`).
pub fn inject_end_draw_failure(window: &MainWindow) {
    window
        .with_surface(|surface| {
            surface
                .resources()
                .set_end_draw_fault(Some(Box::new(|_| 0x8000_4005_u32 as i32)));
        })
        .expect("a surface that is not being drawn");
}

/// Makes every `Present` of the swap chain report "window occluded" (a success code).
pub fn inject_occlusion(window: &MainWindow) {
    window
        .with_surface(|surface| {
            surface
                .resources()
                .set_present_fault(Some(Box::new(|_| DXGI_STATUS_OCCLUDED_HRESULT)));
        })
        .expect("a surface that is not being drawn");
}

/// Calls `step` until `done` is true, at most `bound` times (the C# `ShellFrames.RenderUntil`: `FramePacer` may defer a frame; no sleep).
pub fn render_until(mut step: impl FnMut(), done: impl Fn() -> bool, bound: u32) -> bool {
    for _ in 0..bound {
        if done() {
            return true;
        }
        step();
    }
    done()
}
