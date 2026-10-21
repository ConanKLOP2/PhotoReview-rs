//! `ShellScrollBars` part 1 (W8-16f, contract `contracts/W8-16f.md`): `ScrollBars.cs` lines 11-142 (colours, options, the class declaration with ALL its
//! fields, constructor, state accessors). The geometry queries are `layout.rs` (W8-16f too); the message handling is `input.rs` and the painting is
//! `paint.rs` (W8-16g). Baseline `d92d6aae`.
//!
//! WP-26 (NE-3 a): the vertical and horizontal scroll bars of the main window, drawn by the shell instead of the WPF `ScrollBar` of `DarkScrollBars.xaml`.
//! They take space like WPF (the viewport engine already subtracts the thickness from the viewport) and handle the mouse: thumb drag, track paging with
//! repeat, and they SWALLOW every left press on a bar or the corner (never a pan of the image underneath). Registered BEFORE `ShellInput` in the window's
//! handler chain (the earlier handler sees the mouse first; the zoomed image's box covers the bars and would swallow the press).
//!
//! UI thread only. The window's handler list owns this object, so it holds the window by `Weak` (no reference cycle, D-W8B4V-7).

use std::cell::Cell;
use std::rc::{Rc, Weak};

use pr_app::input::primitives::PointD;
use pr_app::input::surface::RenderHookId;
use pr_app::overlay::palette;
use pr_app::viewing::scrollbar_geometry::{ScrollAxis, ScrollBarMetrics};
use pr_app::viewport::render_geometry::ColorF;

use crate::host::contracts::{ShellWindowApi, WindowMessageHandler};
use crate::host::frame_clock::base::qpc_frequency;
use crate::shell_viewing::scrollbar_native;
use crate::shell_viewing::viewport_controller::ViewportController;

/// Colours of the bar and the thumb per state (C# `ScrollBarColors`, from `DarkScrollBars.xaml`).
#[derive(Debug, Clone, Copy)]
pub struct ScrollBarColors;

impl ScrollBarColors {
    /// `Dark.ScrollTrack` `#161616`: the bar background and the corner square. Equal to `palette::get("Dark.ScrollTrack")`.
    pub const TRACK: ColorF = palette::from_argb(0xFF16_1616);
    /// `Border Background="#4A4A4A"`.
    pub const THUMB: ColorF = palette::from_argb(0xFF4A_4A4A);
    /// `IsMouseOver`: `#6A6A6A`.
    pub const THUMB_HOVER: ColorF = palette::from_argb(0xFF6A_6A6A);
    /// `IsDragging`: `#7A7A7A`.
    pub const THUMB_PRESSED: ColorF = palette::from_argb(0xFF7A_7A7A);
}

/// C# `ScrollBarOptions` (default = the real run; tests replace single members).
#[derive(Clone)]
pub struct ScrollBarOptions {
    /// Metrics in DIP; default = `DarkScrollBars.xaml` = `ViewportControllerOptions::scroll_bar_thickness` 10, margins 2/1, minimum thumb 8, radius 4.
    pub metrics: ScrollBarMetrics,
    /// `(delay_ms, interval_ms)` of press-and-hold on the track (WPF `RepeatButton`); default `scrollbar_native::repeat_timing` (keyboard settings).
    pub repeat_timing: Rc<dyn Fn() -> (i32, i32)>,
    /// Asks for `WM_MOUSELEAVE` for the window handle so the hover state clears when the pointer leaves; default `scrollbar_native::track_mouse_leave`.
    pub track_mouse_leave: Rc<dyn Fn(isize)>,
    /// Ticks per second of `ViewportController::timestamp`; default `host::frame_clock::base::qpc_frequency()`.
    pub ticks_per_second: i64,
}

impl Default for ScrollBarOptions {
    fn default() -> Self {
        Self {
            metrics: ScrollBarMetrics::default(),
            repeat_timing: Rc::new(scrollbar_native::repeat_timing),
            track_mouse_leave: Rc::new(scrollbar_native::track_mouse_leave),
            ticks_per_second: qpc_frequency(),
        }
    }
}

impl std::fmt::Debug for ScrollBarOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollBarOptions")
            .field("metrics", &self.metrics)
            .field("ticks_per_second", &self.ticks_per_second)
            .finish_non_exhaustive()
    }
}

/// C# `ShellScrollBars`. Share it as `Rc<ShellScrollBars>`; it implements `WindowMessageHandler` (in `input.rs`). Every field is interior-mutable
/// (a handler can be re-entered by a nested `SendMessage`, D-W8B3H-6) and `pub(super)` so `layout.rs`, `input.rs` and `paint.rs` can use them.
// Most fields are read only by `input.rs` and `paint.rs` (W8-16g); that unit removes this allow.
#[allow(dead_code)]
pub struct ShellScrollBars {
    pub(super) window: Weak<dyn ShellWindowApi>,
    pub(super) viewport: Rc<ViewportController>,
    pub(super) is_compare_visible: Rc<dyn Fn() -> bool>,
    pub(super) on_press: Option<Rc<dyn Fn()>>,
    pub(super) options: ScrollBarOptions,
    /// `Weak` to itself (`Rc::new_cyclic`): the frame-hook closure must not keep the bars alive.
    pub(super) this: Weak<ShellScrollBars>,
    pub(super) pointer_inside: Cell<bool>,
    pub(super) tracking: Cell<bool>,
    pub(super) pointer: Cell<PointD>,
    pub(super) dragging: Cell<bool>,
    pub(super) drag_axis: Cell<ScrollAxis>,
    pub(super) drag_grab: Cell<f64>,
    pub(super) paging: Cell<bool>,
    pub(super) page_axis: Cell<ScrollAxis>,
    pub(super) page_direction: Cell<i32>,
    pub(super) page_region_active: Cell<bool>,
    pub(super) next_repeat_ticks: Cell<i64>,
    /// C# `_frameHooked`: the id of the render-frame hook on the viewport controller while paging.
    pub(super) frame_hook: Cell<Option<RenderHookId>>,
    pub(super) drawn_frames: Cell<i64>,
    pub(super) disposed: Cell<bool>,
}

impl std::fmt::Debug for ShellScrollBars {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellScrollBars")
            .field("dragging", &self.dragging.get())
            .field("paging", &self.paging.get())
            .field("drawn_frames", &self.drawn_frames.get())
            .finish_non_exhaustive()
    }
}

impl ShellScrollBars {
    /// C# constructor `(window, viewport, isCompareVisible, onPress, options)`: builds the object with `Rc::new_cyclic` (all state false / zero) and
    /// registers it with `window.add_message_handler` (appended, so register BEFORE `ShellInput`). `on_press` runs for every press taken by a bar
    /// (C# `Window_PreviewMouseDown`: stops a kinetic glide). `options` `None` = `ScrollBarOptions::default()`.
    pub fn new(
        window: &Rc<dyn ShellWindowApi>,
        viewport: Rc<ViewportController>,
        is_compare_visible: Rc<dyn Fn() -> bool>,
        on_press: Option<Rc<dyn Fn()>>,
        options: Option<ScrollBarOptions>,
    ) -> Rc<ShellScrollBars> {
        // T6: the closure only stores `this`; it does not call the window or the viewport (no upgrade yet).
        let bars = Rc::new_cyclic(|this| ShellScrollBars {
            window: Rc::downgrade(window),
            viewport,
            is_compare_visible,
            on_press,
            options: options.unwrap_or_default(),
            this: this.clone(),
            pointer_inside: Cell::new(false),
            tracking: Cell::new(false),
            pointer: Cell::new(PointD::default()),
            dragging: Cell::new(false),
            drag_axis: Cell::new(ScrollAxis::Vertical),
            drag_grab: Cell::new(0.0),
            paging: Cell::new(false),
            page_axis: Cell::new(ScrollAxis::Vertical),
            page_direction: Cell::new(0),
            page_region_active: Cell::new(false),
            next_repeat_ticks: Cell::new(0),
            frame_hook: Cell::new(None),
            drawn_frames: Cell::new(0),
            disposed: Cell::new(false),
        });
        window.add_message_handler(Rc::clone(&bars) as Rc<dyn WindowMessageHandler>);
        bars
    }

    /// C# `IsDragging`: the thumb is being dragged.
    pub fn is_dragging(&self) -> bool {
        self.dragging.get()
    }

    /// C# `IsPaging`: the mouse is held on a track (page repeat).
    pub fn is_paging(&self) -> bool {
        self.paging.get()
    }

    /// C# `DrawnFrames`: frames in which at least one bar was drawn (diagnostics, tests).
    pub fn drawn_frames(&self) -> i64 {
        self.drawn_frames.get()
    }
}
