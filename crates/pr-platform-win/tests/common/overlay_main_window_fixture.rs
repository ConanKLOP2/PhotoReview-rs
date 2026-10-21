//! Test rig of W8-15i (`ShellOverlay` input and paint): the Rust twin of the click, key and menu parts of the C# `OverlayRig`.
//!
//! Built on the 15h fixture (`overlay_layout_fixture`: fake frame clock and tooltips, 1 tick = 1 ms). This one adds a `FakeMenuHost`, a command log that can
//! run a hook while a command is executing, and failure logs. Nothing sleeps and nothing touches the OS.

#![allow(dead_code)] // shared by two test files; each uses a subset

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::menus::model::MenuItemModel;
use pr_app::overlay::model::OverlayModel;
use pr_app::overlay::tools_menu::OverlayCommands;
use pr_app::services::render::TextRenderer;
use pr_core::settings::app_settings::AppSettings;
use pr_l10n::localizer::built_in_english_localizer;
use pr_platform_win::shell_overlay::overlay::state::{
    OverlayButton, OverlayMenuHost, ShellOverlay, ShellOverlayOptions,
};

use crate::overlay_fakes::FakeTextRenderer;
use crate::overlay_layout_fixture::{FakeFrameClock, FakeTooltips, center_of};

/// One `show` call: the item ids with their enabled flag, and the anchor.
pub type ShownMenu = (Vec<(String, bool)>, PointD);

/// What `FakeMenuHost::show` answers.
pub enum MenuAnswer {
    /// The user chose this item id.
    Chosen(&'static str),
    /// The user dismissed the menu.
    Dismissed,
    /// The menu could not be shown.
    Fails(&'static str),
}

/// C# `FakeMenuHost`: records the items and the anchor, runs `while_open` (the window keeps pumping messages inside a modal menu), then answers.
pub struct FakeMenuHost {
    /// `(item ids with their enabled flag, anchor)` of every `show`.
    pub shown: RefCell<Vec<ShownMenu>>,
    pub answer: RefCell<MenuAnswer>,
    pub while_open: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Default for FakeMenuHost {
    fn default() -> Self {
        Self {
            shown: RefCell::default(),
            answer: RefCell::new(MenuAnswer::Dismissed),
            while_open: RefCell::new(None),
        }
    }
}

impl OverlayMenuHost for FakeMenuHost {
    fn show(&self, items: &[MenuItemModel], screen_point_dip: PointD) -> Result<Option<String>, Box<dyn Error>> {
        self.shown.borrow_mut().push((
            items.iter().map(|item| (item.id.clone(), item.is_enabled)).collect(),
            screen_point_dip,
        ));
        let hook = self.while_open.borrow().clone();
        if let Some(hook) = hook {
            hook();
        }
        match &*self.answer.borrow() {
            MenuAnswer::Chosen(id) => Ok(Some((*id).to_owned())),
            MenuAnswer::Dismissed => Ok(None),
            MenuAnswer::Fails(text) => Err((*text).into()),
        }
    }
}

/// Called with the command name at the start of every command.
pub type CommandHook = Option<Rc<dyn Fn(&str)>>;

/// C# `FakeOverlayCommands`: a call log and a hook that runs inside each command.
pub struct LoggingCommands {
    pub calls: Rc<RefCell<Vec<String>>>,
    pub hook: Rc<RefCell<CommandHook>>,
}

impl LoggingCommands {
    fn record(&self, name: &str) {
        self.calls.borrow_mut().push(name.to_owned());
        let hook = self.hook.borrow().clone();
        if let Some(hook) = hook {
            hook(name);
        }
    }
}

impl OverlayCommands for LoggingCommands {
    fn open_folder(&mut self) {
        self.record("OpenFolder");
    }
    fn show_settings(&mut self) {
        self.record("Settings");
    }
    fn fit(&mut self) {
        self.record("Fit");
    }
    fn show_skipped_files(&mut self) {
        self.record("Skipped");
    }
    fn show_recovery(&mut self) {
        self.record("Recovery");
    }
    fn show_diagnostics(&mut self) {
        self.record("Diagnostics");
    }
    fn show_benchmark(&mut self) {
        self.record("Benchmark");
    }
    fn clear_cache(&mut self) {
        self.record("ClearCache");
    }
    fn remove_duplicates(&mut self, numbered: bool) {
        self.record(if numbered { "RemoveNumbered" } else { "RemoveOriginal" });
    }
}

/// C# `OverlayRig` with menus.
pub struct InputRig {
    pub overlay: Rc<ShellOverlay>,
    pub settings: Rc<RefCell<AppSettings>>,
    pub clock: Rc<FakeFrameClock>,
    pub tooltips: Rc<FakeTooltips>,
    pub menus: Rc<FakeMenuHost>,
    pub calls: Rc<RefCell<Vec<String>>>,
    pub command_hook: Rc<RefCell<CommandHook>>,
    pub now: Rc<Cell<i64>>,
    pub render_requests: Rc<Cell<usize>>,
    /// `(name, message)` of `on_command_failed`.
    pub command_failures: Rc<RefCell<Vec<(String, String)>>>,
    /// Messages of `on_menu_failed` (only when `with_menu_failed`).
    pub menu_failures: Rc<RefCell<Vec<String>>>,
}

/// A point far from the toolbar (outside its hot zone) and from every panel.
pub const FAR_AWAY: PointD = PointD { x: 600.0, y: 400.0 };

impl InputRig {
    /// `images`: `has_images = true`. `with_menu_failed`: install `on_menu_failed` (else the menu error goes to `on_command_failed`).
    pub fn new(images: bool, with_menu_failed: bool) -> InputRig {
        InputRig::with_text(images, with_menu_failed, Rc::new(FakeTextRenderer::new()), 1)
    }

    /// Same as `new` with a given text renderer (the native draw test passes the DirectWrite one).
    pub fn with_text(
        images: bool,
        with_menu_failed: bool,
        text: Rc<dyn TextRenderer>,
        text_renderer_id: u64,
    ) -> InputRig {
        let settings = Rc::new(RefCell::new(AppSettings {
            toolbar_auto_hide: true,
            toolbar_auto_hide_delay_ms: 1500,
            info_overlay_auto_hide: true,
            info_overlay_auto_hide_delay_ms: 3000,
            toolbar_opacity_percent: 100,
            ..AppSettings::default()
        }));
        let clock = Rc::new(FakeFrameClock::default());
        let tooltips = Rc::new(FakeTooltips::default());
        let menus = Rc::new(FakeMenuHost::default());
        let calls = Rc::new(RefCell::new(Vec::new()));
        let command_hook: Rc<RefCell<CommandHook>> = Rc::new(RefCell::new(None));
        let now = Rc::new(Cell::new(10_000));
        let render_requests = Rc::new(Cell::new(0usize));
        let command_failures = Rc::new(RefCell::new(Vec::new()));
        let menu_failures = Rc::new(RefCell::new(Vec::new()));

        let overlay = ShellOverlay::new(ShellOverlayOptions {
            text,
            text_renderer_id,
            texts: built_in_english_localizer(),
            clock: clock.clone(),
            settings: {
                let settings = Rc::clone(&settings);
                Box::new(move || Arc::new(settings.borrow().clone()))
            },
            commands: Box::new(LoggingCommands {
                calls: Rc::clone(&calls),
                hook: Rc::clone(&command_hook),
            }),
            request_render: {
                let counter = Rc::clone(&render_requests);
                Box::new(move || counter.set(counter.get() + 1))
            },
            is_window_active: Box::new(|| true),
            menus: Some(menus.clone()),
            tooltips: Some(tooltips.clone()),
            client_origin_on_screen_dip: Some(Box::new(|| PointD { x: 100.0, y: 50.0 })),
            on_command_failed: {
                let log = Rc::clone(&command_failures);
                Some(Box::new(move |name, message| {
                    log.borrow_mut().push((name.to_owned(), message.to_owned()));
                }))
            },
            on_menu_failed: if with_menu_failed {
                let log = Rc::clone(&menu_failures);
                Some(Box::new(move |error| log.borrow_mut().push(error.to_string())))
            } else {
                None
            },
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
        let rig = InputRig {
            overlay,
            settings,
            clock,
            tooltips,
            menus,
            calls,
            command_hook,
            now,
            render_requests,
            command_failures,
            menu_failures,
        };
        if images {
            rig.update(|model| model.has_images = true);
        }
        rig
    }

    /// A weak handle for hooks that must call back into the overlay without a reference cycle.
    pub fn weak_overlay(&self) -> Weak<ShellOverlay> {
        Rc::downgrade(&self.overlay)
    }

    /// C# `Advance(ms)`: `ms` frames, one per millisecond.
    pub fn advance(&self, ms: i64) {
        for _ in 0..ms {
            self.now.set(self.now.get() + 1);
            self.clock.tick(self.now.get());
        }
    }

    /// C# `Update(edit)`.
    pub fn update(&self, edit: impl FnOnce(&mut OverlayModel)) {
        let mut model = self.overlay.model();
        edit(&mut model);
        self.overlay.update(model);
    }

    /// The box of an element after the last arrange.
    pub fn bounds_of(&self, id: &str) -> RectD {
        self.overlay.element_bounds(id).expect("element exists")
    }

    /// C# `OverlayRig.Center(button)`.
    pub fn center(&self, button: OverlayButton) -> PointD {
        center_of(self.bounds_of(button.element_id()))
    }

    /// The calls logged so far.
    pub fn log(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// Press and release at the centre of `button`.
    pub fn click(&self, button: OverlayButton) {
        let point = self.center(button);
        assert!(self.overlay.pointer_down(point), "press must be swallowed");
        assert!(self.overlay.pointer_up(point), "release must be swallowed");
    }

    /// A model with everything visible (status, EXIF, skipped row, folder, zoom, badge).
    pub fn full_model(&self) {
        self.update(|m| {
            m.has_images = true;
            m.status_panel_visible = true;
            m.file_info_visible = true;
            m.status_text = "3 / 10".to_owned();
            m.font_size = 14.0;
            m.exif_font_size = 12.0;
            m.exif_visible = true;
            m.exif_text = "ISO 100".to_owned();
            m.has_skipped_entries = true;
            m.skipped_warning_text = "2 skipped".to_owned();
            m.folder_info_visible = true;
            m.folder_info_text = "C:\\Photos".to_owned();
            m.zoom_indicator_visible = true;
            m.zoom_indicator_text = "150%".to_owned();
            m.capture_pair_visible = true;
            m.capture_pair_text = "JPEG+RAW".to_owned();
            m.capture_pair_tool_tip = "Capture pair".to_owned();
        });
    }
}
