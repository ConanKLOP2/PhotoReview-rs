//! Shared helpers of the shell-input tests (W8-14a creates it, W8-14b extends it): the Rust twin of the message senders of
//! `tests/PhotoReview.Shell.Tests/Input/InputTestSupport.cs` (baseline `d92d6aae`), reduced to what the three message handlers need.
//!
//! [`RecordingInput`] implements the three port traits (`KeyInputPort`, `PointerInputPort`, `WheelInputPort`) and records every call, so the
//! DECODING of the handlers (virtual key, scan code, signed words, `MK_*` flags, screen to client) is tested without a `ShellInput`.
//! [`send`] is the C# `Send(...)` helper: it builds a `WindowMessage` and asks one handler.
#![allow(dead_code)] // shared by several test crates, each of which uses a different part

use std::cell::{Cell, RefCell};

use pr_app::input::key_id::KeyId;
use pr_app::input::primitives::{PointD, PointerButton};
use pr_platform_win::host::contracts::{WindowMessage, WindowMessageHandler};
use pr_platform_win::shell_input::handlers::{KeyInputPort, PointerInputPort, WheelInputPort};
use pr_platform_win::win32::structs::{Lparam, Lresult};

/// One call a handler made on a port (`PointD` has no `PartialEq`, so points are stored as tuples).
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    KeyDown {
        key: KeyId,
        is_system_key: bool,
        is_repeat: bool,
        message_time: i32,
    },
    PointerDown(PointerButton, i32, i32),
    PointerMove(i32, i32, bool),
    LeftButtonUp(i32, i32),
    CaptureLost,
    ScreenToClient(f64, f64),
    VerticalWheel {
        delta: i32,
        ctrl: bool,
        client: (f64, f64),
    },
    HorizontalWheel {
        delta: i32,
        ctrl: bool,
        client: (f64, f64),
    },
}

/// A recording fake of `ShellInput` for the three ports.
pub struct RecordingInput {
    calls: RefCell<Vec<Call>>,
    /// Keys `handle_key_down` answers `true` for.
    pub handled_keys: RefCell<Vec<KeyId>>,
    /// The answer of `handle_pointer_down`.
    pub pointer_down_result: Cell<bool>,
    /// The answer of the two wheel calls.
    pub wheel_result: Cell<bool>,
    /// The `Environment.MessageTime()` the key handler passes on.
    pub message_time: Cell<i32>,
    /// `screen_to_client_dip(p) = ((p.x - origin.0) / scale, (p.y - origin.1) / scale)`.
    pub origin: Cell<(f64, f64)>,
    pub scale: Cell<f64>,
}

impl RecordingInput {
    pub fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            handled_keys: RefCell::new(Vec::new()),
            pointer_down_result: Cell::new(false),
            wheel_result: Cell::new(false),
            message_time: Cell::new(0),
            origin: Cell::new((0.0, 0.0)),
            scale: Cell::new(1.0),
        }
    }

    /// The recorded calls, oldest first.
    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }

    /// Forgets the recorded calls.
    pub fn clear(&self) {
        self.calls.borrow_mut().clear();
    }

    fn record(&self, call: Call) {
        self.calls.borrow_mut().push(call);
    }
}

impl KeyInputPort for RecordingInput {
    fn handle_key_down(&self, key: KeyId, is_system_key: bool, is_repeat: bool, message_time: i32) -> bool {
        self.record(Call::KeyDown {
            key,
            is_system_key,
            is_repeat,
            message_time,
        });
        self.handled_keys.borrow().contains(&key)
    }

    fn message_time(&self) -> i32 {
        self.message_time.get()
    }
}

impl PointerInputPort for RecordingInput {
    fn handle_pointer_down(&self, button: PointerButton, pixel_x: i32, pixel_y: i32) -> bool {
        self.record(Call::PointerDown(button, pixel_x, pixel_y));
        self.pointer_down_result.get()
    }

    fn handle_pointer_move(&self, pixel_x: i32, pixel_y: i32, left_button_pressed: bool) {
        self.record(Call::PointerMove(pixel_x, pixel_y, left_button_pressed));
    }

    fn handle_left_button_up(&self, pixel_x: i32, pixel_y: i32) {
        self.record(Call::LeftButtonUp(pixel_x, pixel_y));
    }

    fn handle_capture_lost(&self) {
        self.record(Call::CaptureLost);
    }
}

impl WheelInputPort for RecordingInput {
    fn screen_to_client_dip(&self, screen_pixel: PointD) -> PointD {
        self.record(Call::ScreenToClient(screen_pixel.x, screen_pixel.y));
        let (origin_x, origin_y) = self.origin.get();
        let scale = self.scale.get();
        PointD {
            x: (screen_pixel.x - origin_x) / scale,
            y: (screen_pixel.y - origin_y) / scale,
        }
    }

    fn handle_vertical_wheel(&self, delta: i32, ctrl: bool, client_dip: PointD) -> bool {
        self.record(Call::VerticalWheel {
            delta,
            ctrl,
            client: (client_dip.x, client_dip.y),
        });
        self.wheel_result.get()
    }

    fn handle_horizontal_wheel(&self, delta: i32, ctrl: bool, client_dip: PointD) -> bool {
        self.record(Call::HorizontalWheel {
            delta,
            ctrl,
            client: (client_dip.x, client_dip.y),
        });
        self.wheel_result.get()
    }
}

/// C# `Send(...)`: asks one handler about one message (`hwnd` is irrelevant to the decoding).
pub fn send(handler: &dyn WindowMessageHandler, msg: u32, wparam: usize, lparam: Lparam) -> Option<Lresult> {
    handler.try_handle(&WindowMessage {
        hwnd: 0,
        msg,
        wparam,
        lparam,
    })
}

/// `MAKELPARAM(x, y)` of a mouse message: two signed 16-bit client or screen coordinates, zero-extended as Windows delivers them on 64 bit.
pub fn point_lparam(x: i32, y: i32) -> Lparam {
    ((((y as u16) as usize) << 16) | ((x as u16) as usize)) as Lparam
}

/// The `lParam` of a `WM_KEYDOWN`: scan code in bits 16..23, bit 24 extended, bit 30 previous key state.
pub fn key_lparam(scan_code: u32, extended: bool, repeat: bool) -> Lparam {
    let mut value = (scan_code & 0xFF) << 16;
    if extended {
        value |= 0x0100_0000;
    }
    if repeat {
        value |= 0x4000_0000;
    }
    value as Lparam
}

/// The `wParam` of a wheel or `XBUTTON` message: `delta_or_button` in the high word, `flags` (`MK_*`) in the low word.
pub fn high_low_wparam(delta_or_button: i32, flags: u32) -> usize {
    (((delta_or_button as u16) as usize) << 16) | (flags as usize & 0xFFFF)
}
