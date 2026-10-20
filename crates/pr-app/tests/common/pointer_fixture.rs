use pr_app::input::operation_version::ViewportOperationVersion;
use pr_app::input::pointer::{PointerCommands, PointerContext, PointerInputController, PointerStep};
use pr_app::input::primitives::{PointD, SizeD};
use pr_app::input::surface::{ImageSurface, RenderFrameHandler, RenderHookId};
use pr_app::viewmodels::viewer_state::ViewerState;
use pr_core::abstractions::{DisplayTiming, NullLog};
use pr_core::model::enums::{KeyboardZoomAnchor, MouseWheelAction};
use pr_core::settings::app_settings::AppSettings;

#[derive(Default)]
pub struct FakeCommands {
    pub has_images: bool,
    pub next: usize,
    pub previous: usize,
    pub fits: usize,
}

impl PointerCommands for FakeCommands {
    fn has_images(&self) -> bool {
        self.has_images
    }
    fn next(&mut self) {
        self.next += 1;
    }
    fn previous(&mut self) {
        self.previous += 1;
    }
    fn apply_fit(&mut self) {
        self.fits += 1;
    }
}

pub struct FakeSurface {
    pub loaded: bool,
    pub horizontal: f64,
    pub vertical: f64,
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub extent_width: f64,
    pub extent_height: f64,
    pub scrolls: Vec<(f64, f64)>,
    pub origins_follow_scroll: bool,
    pub pointer: Option<PointD>,
    pub to_image_element_calls: std::cell::RefCell<Vec<PointD>>,
    pub source: Option<SizeD>,
    pub captures: usize,
    pub releases: usize,
    pub pan_cursor: bool,
    pub hooks: usize,
    pub unhooks: usize,
    pub layout_updates: usize,
    pub render_yields: usize,
}

impl Default for FakeSurface {
    fn default() -> Self {
        Self {
            loaded: true,
            horizontal: 0.0,
            vertical: 0.0,
            viewport_width: 800.0,
            viewport_height: 600.0,
            extent_width: 800.0,
            extent_height: 600.0,
            scrolls: Vec::new(),
            origins_follow_scroll: false,
            pointer: None,
            to_image_element_calls: std::cell::RefCell::new(Vec::new()),
            source: Some(SizeD {
                width: 800.0,
                height: 600.0,
            }),
            captures: 0,
            releases: 0,
            pan_cursor: false,
            hooks: 0,
            unhooks: 0,
            layout_updates: 0,
            render_yields: 0,
        }
    }
}

impl ImageSurface for FakeSurface {
    fn is_loaded(&self) -> bool {
        self.loaded
    }
    fn horizontal_offset(&self) -> f64 {
        self.horizontal
    }
    fn vertical_offset(&self) -> f64 {
        self.vertical
    }
    fn viewport_width(&self) -> f64 {
        self.viewport_width
    }
    fn viewport_height(&self) -> f64 {
        self.viewport_height
    }
    fn extent_width(&self) -> f64 {
        self.extent_width
    }
    fn extent_height(&self) -> f64 {
        self.extent_height
    }
    fn drag_threshold(&self) -> (f64, f64) {
        (4.0, 4.0)
    }
    fn scroll_to(&mut self, h: f64, v: f64) {
        self.horizontal = h;
        self.vertical = v;
        self.scrolls.push((h, v));
    }
    fn update_layout(&mut self) {
        self.layout_updates += 1;
    }
    fn yield_to_render(&mut self, continuation: Box<dyn FnOnce()>) {
        self.render_yields += 1;
        continuation();
    }
    fn image_origin(&self) -> PointD {
        if self.origins_follow_scroll {
            PointD {
                x: -self.horizontal,
                y: -self.vertical,
            }
        } else {
            PointD::default()
        }
    }
    fn to_image_element(&self, p: PointD) -> PointD {
        self.to_image_element_calls.borrow_mut().push(p);
        PointD {
            x: p.x + self.horizontal,
            y: p.y + self.vertical,
        }
    }
    fn image_actual_width(&self) -> f64 {
        self.extent_width
    }
    fn image_actual_height(&self) -> f64 {
        self.extent_height
    }
    fn source_size(&self) -> Option<SizeD> {
        self.source
    }
    fn capture_mouse(&mut self) {
        self.captures += 1;
    }
    fn release_mouse_capture(&mut self) {
        self.releases += 1;
    }
    fn set_pan_cursor(&mut self, value: bool) {
        self.pan_cursor = value;
    }
    fn hook_render_frame(&mut self, _handler: RenderFrameHandler) -> RenderHookId {
        self.hooks += 1;
        RenderHookId(self.hooks as u64)
    }
    fn unhook_render_frame(&mut self, _id: RenderHookId) {
        self.unhooks += 1;
    }
    fn timestamp(&self) -> i64 {
        0
    }
    fn display_timing(&mut self) -> Option<DisplayTiming> {
        None
    }
    fn pointer_position(&self) -> Option<PointD> {
        self.pointer
    }
}

pub struct Fixture {
    pub controller: PointerInputController,
    pub surface: FakeSurface,
    pub viewer: ViewerState,
    pub settings: AppSettings,
    pub versions: ViewportOperationVersion,
    pub commands: FakeCommands,
    pub yields: usize,
}

impl Fixture {
    // Preserve the red-tests fixture setup; its explicit assignments mirror the C# fixture.
    #[expect(
        clippy::field_reassign_with_default,
        reason = "red-tests fixture initialization must remain unchanged"
    )]
    pub fn new() -> Self {
        let mut viewer = ViewerState::new();
        viewer.reset_fit(800.0, 600.0);
        let mut settings = AppSettings::default();
        settings.mouse_wheel_action = MouseWheelAction::Zoom;
        settings.keyboard_zoom_anchor = Some(KeyboardZoomAnchor::Pointer);
        Self {
            controller: PointerInputController::new(10_000.0),
            surface: FakeSurface::default(),
            viewer,
            settings,
            versions: ViewportOperationVersion::new(),
            commands: FakeCommands {
                has_images: true,
                ..Default::default()
            },
            yields: 0,
        }
    }

    pub fn call(
        &mut self,
        action: impl FnOnce(&mut PointerInputController, &mut PointerContext<'_>) -> PointerStep,
    ) -> PointerStep {
        let mut ctx = PointerContext {
            surface: &mut self.surface,
            viewer: &mut self.viewer,
            settings: &self.settings,
            versions: &mut self.versions,
            commands: &mut self.commands,
            log: &NullLog,
        };
        action(&mut self.controller, &mut ctx)
    }

    pub fn drive(&mut self, mut step: PointerStep) {
        while let PointerStep::AwaitRender(next) = step {
            self.yields += 1;
            step = self.call(|controller, ctx| controller.resume(ctx, next));
        }
    }

    pub fn run(&mut self, action: impl FnOnce(&mut PointerInputController, &mut PointerContext<'_>) -> PointerStep) {
        let step = self.call(action);
        self.drive(step);
    }

    pub fn zoomed_with_source(&mut self, width: i32, height: i32) {
        self.viewer.set_source_size(width, height, false);
        self.viewer.set_zoom(1.0);
        self.surface.extent_width = f64::from(width);
        self.surface.extent_height = f64::from(height);
        self.surface.horizontal = 500.0;
        self.surface.vertical = 200.0;
        self.surface.origins_follow_scroll = true;
    }
}

pub fn point(x: f64, y: f64) -> PointD {
    PointD { x, y }
}
