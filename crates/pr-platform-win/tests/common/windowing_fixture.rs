//! Fakes of the W8-13k tests (`host_main_window_windowing.rs`): a recording `WindowOps` (user32 of `FullscreenController`) and a recording
//! `FullscreenNative`. Pure: no window, no Win32 call.
#![allow(dead_code)] // each test crate uses a different part

use std::cell::{Cell, RefCell};

use pr_app::windowing::contracts::WindowShowState;
use pr_app::windowing::fullscreen::{MonitorSnapshot, WindowOps};
use pr_app::windowing::monitor_layout::ScreenRect;
use pr_platform_win::host::main_window::windowing::{DwmWindowAttributes, FullscreenNative};
use pr_platform_win::win32::structs::Hwnd;

/// `SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED`.
pub const MOVE: u32 = 4 | 0x10 | 0x20;
/// Any non-zero handle.
pub const HWND: isize = 42;
pub const WINDOW: ScreenRect = ScreenRect::new(100, 100, 700, 500);
pub const MONITOR: ScreenRect = ScreenRect::new(0, 0, 1920, 1080);
pub const WORK: ScreenRect = ScreenRect::new(0, 0, 1920, 1040);
pub const STYLE: isize = 0x16CF_0000;

/// One mutating call of the fake user32.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    ShowNormal,
    ShowMaximized,
    SetStyle(isize),
    SetPos(ScreenRect, u32),
}

/// Fake user32: reads return the fields, mutating calls are logged. `on_set_pos` runs inside `set_window_pos` (the re-entrancy of `SetWindowPos`).
pub struct FakeOps {
    pub log: RefCell<Vec<Op>>,
    pub iconic: bool,
    pub normal: Option<ScreenRect>,
    pub window: Option<ScreenRect>,
    pub monitor: Option<MonitorSnapshot>,
    pub style: isize,
    pub on_set_pos: RefCell<Option<Box<dyn Fn()>>>,
}

impl FakeOps {
    pub fn baseline() -> Self {
        Self {
            log: RefCell::new(Vec::new()),
            iconic: false,
            normal: Some(WINDOW),
            window: Some(WINDOW),
            monitor: Some(MonitorSnapshot {
                handle: 7,
                monitor: MONITOR,
                work: WORK,
            }),
            style: STYLE,
            on_set_pos: RefCell::new(None),
        }
    }

    pub fn ops(&self) -> Vec<Op> {
        self.log.borrow().clone()
    }
}

impl WindowOps for FakeOps {
    fn is_iconic(&self, _hwnd: isize) -> bool {
        self.iconic
    }
    fn show_normal(&self, _hwnd: isize) {
        self.log.borrow_mut().push(Op::ShowNormal);
    }
    fn show_maximized(&self, _hwnd: isize) {
        self.log.borrow_mut().push(Op::ShowMaximized);
    }
    fn normal_position(&self, _hwnd: isize) -> Option<ScreenRect> {
        self.normal
    }
    fn window_rect(&self, _hwnd: isize) -> Option<ScreenRect> {
        self.window
    }
    fn nearest_monitor(&self, _hwnd: isize) -> Option<MonitorSnapshot> {
        self.monitor
    }
    fn window_style(&self, _hwnd: isize) -> isize {
        self.style
    }
    fn set_window_style(&self, _hwnd: isize, style: isize) {
        self.log.borrow_mut().push(Op::SetStyle(style));
    }
    fn set_window_pos(&self, _hwnd: isize, rect: ScreenRect, flags: u32) {
        self.log.borrow_mut().push(Op::SetPos(rect, flags));
        if let Some(hook) = self.on_set_pos.borrow().as_ref() {
            hook();
        }
    }
}

/// One mutating call of the fake native seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCall {
    MoveTo(ScreenRect),
    Show(WindowShowState),
}

/// Fake `FullscreenNative`.
pub struct FakeNative {
    pub state: Cell<WindowShowState>,
    pub monitor: Cell<Option<ScreenRect>>,
    pub window: Cell<Option<ScreenRect>>,
    pub calls: RefCell<Vec<NativeCall>>,
}

impl FakeNative {
    pub fn new(state: WindowShowState) -> Self {
        Self {
            state: Cell::new(state),
            monitor: Cell::new(Some(MONITOR)),
            window: Cell::new(Some(MONITOR)),
            calls: RefCell::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<NativeCall> {
        self.calls.borrow().clone()
    }
}

impl FullscreenNative for FakeNative {
    fn current_state(&self, _hwnd: Hwnd) -> WindowShowState {
        self.state.get()
    }
    fn try_get_monitor_rect(&self, _hwnd: Hwnd) -> Option<ScreenRect> {
        self.monitor.get()
    }
    fn try_get_window_rect(&self, _hwnd: Hwnd) -> Option<ScreenRect> {
        self.window.get()
    }
    fn move_to(&self, _hwnd: Hwnd, rect: ScreenRect) {
        self.calls.borrow_mut().push(NativeCall::MoveTo(rect));
    }
    fn show(&self, _hwnd: Hwnd, state: WindowShowState) {
        self.calls.borrow_mut().push(NativeCall::Show(state));
    }
}

/// Recording `DwmWindowAttributes`: `accept` decides per attribute whether DWM "accepts" it.
pub struct FakeDwm {
    pub calls: RefCell<Vec<(Hwnd, u32, u32)>>,
    pub accept: Box<dyn Fn(u32) -> bool>,
}

impl FakeDwm {
    pub fn new(accept: impl Fn(u32) -> bool + 'static) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            accept: Box::new(accept),
        }
    }

    pub fn calls(&self) -> Vec<(Hwnd, u32, u32)> {
        self.calls.borrow().clone()
    }
}

impl DwmWindowAttributes for FakeDwm {
    fn set(&self, hwnd: Hwnd, attribute: u32, value: u32) -> bool {
        self.calls.borrow_mut().push((hwnd, attribute, value));
        (self.accept)(attribute)
    }
}
