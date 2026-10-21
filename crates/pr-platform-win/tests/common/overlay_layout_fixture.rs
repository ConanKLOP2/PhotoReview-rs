//! Test rig of W8-15h (`ShellOverlay` part 1): the Rust twin of the C# `OverlayRig` without the click, key and menu parts (those belong to W8-15i).
//!
//! The clock is deterministic: 1 tick = 1 ms (`ticks_per_second = 1000`), `Rig::advance(ms)` bumps the fake timestamp by one and raises one frame per
//! millisecond (the C# `Advance` idiom). Nothing sleeps and nothing touches the OS.

#![allow(dead_code)] // shared by several test files; each uses a subset

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use pr_app::input::frame_clock::{FrameHandler, FrameHandlerId, FrameTick};
use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::overlay::model::OverlayModel;
use pr_app::overlay::tools_menu::OverlayCommands;
use pr_core::settings::app_settings::AppSettings;
use pr_l10n::localizer::built_in_english_localizer;
use pr_platform_win::shell_overlay::overlay::state::{OverlayFrameClock, ShellOverlay, ShellOverlayOptions};
use pr_platform_win::shell_overlay::tooltips::{OverlayTooltip, OverlayTooltips};
use pr_platform_win::win32::structs::{Lparam, Wparam};

use crate::overlay_fakes::FakeTextRenderer;

/// C# `FakeFrameClock`: records subscribers, `tick(now)` raises one frame. Subscribing and unsubscribing from inside a handler is allowed.
#[derive(Default)]
pub struct FakeFrameClock {
    next_id: Cell<u64>,
    handlers: RefCell<Vec<(FrameHandlerId, Rc<RefCell<FrameHandler>>)>>,
    pub frame_requests: Cell<usize>,
    pub subscribe_calls: Cell<usize>,
    pub unsubscribe_calls: Cell<usize>,
}

impl FakeFrameClock {
    /// C# `SubscriberCount`.
    pub fn subscriber_count(&self) -> usize {
        self.handlers.borrow().len()
    }

    /// C# `Tick(timestamp)`: one frame for every handler that is still subscribed when its turn comes.
    pub fn tick(&self, now: i64) {
        let tick = FrameTick {
            timestamp: now,
            rendering_time_ms: 0.0,
            timing: None,
        };
        let snapshot: Vec<_> = self
            .handlers
            .borrow()
            .iter()
            .map(|(id, handler)| (*id, Rc::clone(handler)))
            .collect();
        for (id, handler) in snapshot {
            let still = self.handlers.borrow().iter().any(|(other, _)| *other == id);
            if still {
                (handler.borrow_mut())(&tick);
            }
        }
    }
}

impl OverlayFrameClock for FakeFrameClock {
    fn subscribe(&self, handler: FrameHandler) -> FrameHandlerId {
        let id = self.next_id.get() + 1;
        self.next_id.set(id);
        self.subscribe_calls.set(self.subscribe_calls.get() + 1);
        self.handlers.borrow_mut().push((id, Rc::new(RefCell::new(handler))));
        id
    }

    fn unsubscribe(&self, id: FrameHandlerId) {
        self.unsubscribe_calls.set(self.unsubscribe_calls.get() + 1);
        self.handlers.borrow_mut().retain(|(other, _)| *other != id);
    }

    fn request_frame(&self) {
        self.frame_requests.set(self.frame_requests.get() + 1);
    }
}

/// C# `FakeOverlayCommands` (names only: the command parts are W8-15i's).
pub struct FakeCommands {
    pub calls: Rc<RefCell<Vec<&'static str>>>,
}

impl OverlayCommands for FakeCommands {
    fn open_folder(&mut self) {
        self.calls.borrow_mut().push("OpenFolder");
    }
    fn show_settings(&mut self) {
        self.calls.borrow_mut().push("Settings");
    }
    fn fit(&mut self) {
        self.calls.borrow_mut().push("Fit");
    }
    fn show_skipped_files(&mut self) {
        self.calls.borrow_mut().push("Skipped");
    }
    fn show_recovery(&mut self) {
        self.calls.borrow_mut().push("Recovery");
    }
    fn show_diagnostics(&mut self) {
        self.calls.borrow_mut().push("Diagnostics");
    }
    fn show_benchmark(&mut self) {
        self.calls.borrow_mut().push("Benchmark");
    }
    fn clear_cache(&mut self) {
        self.calls.borrow_mut().push("ClearCache");
    }
    fn remove_duplicates(&mut self, numbered: bool) {
        self.calls
            .borrow_mut()
            .push(if numbered { "RemoveNumbered" } else { "RemoveOriginal" });
    }
}

/// C# `FakeTooltips`: counts and remembers; `dispose` is recorded.
#[derive(Default)]
pub struct FakeTooltips {
    pub tools: RefCell<Vec<OverlayTooltip>>,
    pub dpi: Cell<f64>,
    pub sync_count: Cell<usize>,
    pub disposed: Cell<bool>,
}

impl OverlayTooltips for FakeTooltips {
    fn sync(&self, tools: &[OverlayTooltip], dpi_scale: f64) {
        *self.tools.borrow_mut() = tools.to_vec();
        self.dpi.set(dpi_scale);
        self.sync_count.set(self.sync_count.get() + 1);
    }

    fn relay_mouse(&self, _message: u32, _wparam: Wparam, _lparam: Lparam) {}

    fn dispose(&self) {
        self.disposed.set(true);
    }
}

/// Callback run by every `request_render` (re-entrancy tests).
pub type RenderHook = Option<Box<dyn Fn()>>;

/// C# `OverlayRig`.
pub struct Rig {
    pub overlay: Rc<ShellOverlay>,
    pub settings: Rc<RefCell<AppSettings>>,
    pub text: Rc<FakeTextRenderer>,
    pub clock: Rc<FakeFrameClock>,
    pub tooltips: Rc<FakeTooltips>,
    pub commands: Rc<RefCell<Vec<&'static str>>>,
    pub now: Rc<Cell<i64>>,
    pub window_active: Rc<Cell<bool>>,
    pub render_requests: Rc<Cell<usize>>,
    /// Called by every `request_render` (re-entrancy tests).
    pub on_render: Rc<RefCell<RenderHook>>,
}

/// The pointer far from the toolbar (outside its hot zone), over the middle of the window.
pub const FAR_AWAY: PointD = PointD { x: 600.0, y: 400.0 };

/// C# `OverlayRig.Center(element)`.
pub fn center_of(rect: RectD) -> PointD {
    PointD {
        x: rect.x + rect.width / 2.0,
        y: rect.y + rect.height / 2.0,
    }
}

impl Rig {
    /// C# `NewRig(tweak, images)`: the default settings of the C# rig (toolbar 1500 ms, info 3000 ms, opacity 100), a 1000x700 client at 1.0, and
    /// `has_images = true` when `images`.
    pub fn new(tweak: impl FnOnce(&mut AppSettings), images: bool) -> Rig {
        let rig = Rig::build(tweak);
        if images {
            rig.update(|model| model.has_images = true);
        }
        rig
    }

    /// C# `new OverlayRig(...)` with a given text renderer.
    pub fn build(tweak: impl FnOnce(&mut AppSettings)) -> Rig {
        let mut initial = AppSettings {
            toolbar_auto_hide: true,
            toolbar_auto_hide_delay_ms: 1500,
            info_overlay_auto_hide: true,
            info_overlay_auto_hide_delay_ms: 3000,
            toolbar_opacity_percent: 100,
            ..AppSettings::default()
        };
        tweak(&mut initial);
        let settings = Rc::new(RefCell::new(initial));
        let text = Rc::new(FakeTextRenderer::new());
        let clock = Rc::new(FakeFrameClock::default());
        let tooltips = Rc::new(FakeTooltips::default());
        let commands = Rc::new(RefCell::new(Vec::new()));
        let now = Rc::new(Cell::new(10_000));
        let window_active = Rc::new(Cell::new(true));
        let render_requests = Rc::new(Cell::new(0usize));
        let on_render: Rc<RefCell<RenderHook>> = Rc::new(RefCell::new(None));

        let overlay = ShellOverlay::new(ShellOverlayOptions {
            text: text.clone(),
            text_renderer_id: 1,
            texts: built_in_english_localizer(),
            clock: clock.clone(),
            settings: {
                let settings = Rc::clone(&settings);
                Box::new(move || Arc::new(settings.borrow().clone()))
            },
            commands: Box::new(FakeCommands {
                calls: Rc::clone(&commands),
            }),
            request_render: {
                let counter = Rc::clone(&render_requests);
                let hook = Rc::clone(&on_render);
                Box::new(move || {
                    counter.set(counter.get() + 1);
                    if let Some(callback) = hook.borrow().as_ref() {
                        callback();
                    }
                })
            },
            is_window_active: {
                let active = Rc::clone(&window_active);
                Box::new(move || active.get())
            },
            menus: None,
            tooltips: Some(tooltips.clone()),
            client_origin_on_screen_dip: Some(Box::new(|| PointD { x: 100.0, y: 50.0 })),
            on_command_failed: None,
            on_menu_failed: None,
            timestamp: {
                let now = Rc::clone(&now);
                Some(Box::new(move || now.get()))
            },
            ticks_per_second: Some(1000),
            animator_ticks_per_second: Some(1000),
        });
        overlay.arrange(
            SizeD {
                width: 1000.0,
                height: 700.0,
            },
            1.0,
        );
        Rig {
            overlay,
            settings,
            text,
            clock,
            tooltips,
            commands,
            now,
            window_active,
            render_requests,
            on_render,
        }
    }

    /// C# `Advance(ms)`: `ms` frames, one per millisecond.
    pub fn advance(&self, ms: i64) {
        for _ in 0..ms {
            self.now.set(self.now.get() + 1);
            self.clock.tick(self.now.get());
        }
    }

    /// C# `Update(edit)`: edit the current model and push it.
    pub fn update(&self, edit: impl FnOnce(&mut OverlayModel)) {
        let mut model = self.overlay.model();
        edit(&mut model);
        self.overlay.update(model);
    }

    /// C# `MoveTo(point)`.
    pub fn move_to(&self, point: PointD) {
        self.overlay.pointer_moved(point);
    }

    /// C# `ToolbarBounds`.
    pub fn toolbar_bounds(&self) -> RectD {
        self.bounds_of("ToolbarPanel")
    }

    /// The box of an element after the last arrange.
    pub fn bounds_of(&self, id: &str) -> RectD {
        self.overlay.element_bounds(id).expect("element exists")
    }

    /// Mutate the live settings (the overlay reads them at every evaluation).
    pub fn set(&self, edit: impl FnOnce(&mut AppSettings)) {
        edit(&mut self.settings.borrow_mut());
    }
}
