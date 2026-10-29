//! Tests of W8-14b (F): `pr_platform_win::shell_input::dispatch::ShellInput` (C# `ShellInput`, baseline `d92d6aae`).
//!
//! The pure controllers of `pr_app` run for real; the window, the host, the frame clock and the dispatcher are fakes (`common/shell_input_dispatch_fixture.rs`,
//! the twin of `InputTestSupport.cs`), the viewport is the REAL `ViewportController`. The 21 ports of `KeyboardInputTests` are in `shell_input_dispatch_keys.rs`, the 30 ports
//! of `PointerInputTests` in `shell_input_dispatch_pointer.rs`; this file has `lifecycle` (the tests new to Rust) and `native` (ignored: a real window; the G-INPUT replay).
#![cfg(windows)]

#[path = "common/shell_input_dispatch_fixture.rs"]
mod fixture;
#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
#[path = "common/ui_thread.rs"]
mod ui_thread;
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

#[path = "shell_input_dispatch/g_input.rs"]
mod g_input;
#[path = "shell_input_dispatch/lifecycle.rs"]
mod lifecycle;
#[path = "shell_input_dispatch/native.rs"]
mod native;
