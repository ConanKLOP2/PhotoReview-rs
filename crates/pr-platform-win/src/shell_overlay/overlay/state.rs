//! `ShellOverlay` part 1: options, seams, state, constructor, accessors, data entry points and `dispose`. Port of `ShellOverlay.cs` lines 12 to 229 and 586
//! to 592 (`ApplyModel`) and 660 to 674 (`Dispose`), baseline `d92d6aae`. Unit W8-15h (contract `contracts/W8-15h.md`).
//!
//! This file declares the struct with ALL fields (the W8-13e `create.rs` precedent); the other files of this module add `impl ShellOverlay` blocks.
//!
//! Ownership (D-W8B4O-4): C# kept `Toolbar`, `Status`, ... as properties AND inside the host's roots. Rust owns the elements ONCE, inside the
//! [`OverlayHost`]: roots `InfoOverlayHost` (a `LayerElement` holding `StatusPanel`, `FolderInfoPanel`, `ZoomIndicatorPanel`, `CapturePairBadge` in that
//! order) and `ToolbarPanel` (added last = on top). The typed properties become the closure accessors `with_toolbar`, `with_status`, ... (the state is in
//! a `RefCell`, so a plain `&ToolbarPanel` cannot be returned). Mutation goes through `OverlayHost::root_mut` and `LayerElement::child_mut`.
//!
//! No borrow across a callback (D-W8B4O-7): every method reads the settings and the window flag BEFORE it borrows the state, mutates under a short
//! `borrow_mut`, releases it, and only then calls `request_render`, the clock or any other injected function.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pr_app::coordinators::fade_gate::FadeTargetGate;
use pr_app::input::primitives::{PointD, SizeD};
use pr_app::overlay::animator::Animator;
use pr_app::overlay::button::ButtonElement;
use pr_app::overlay::capture_badge::CapturePairBadge;
use pr_app::overlay::element::ElementNode;
use pr_app::overlay::folder_info::FolderInfoPanel;
use pr_app::overlay::frame_delay::FrameDelayTimer;
use pr_app::overlay::layer::LayerElement;
use pr_app::overlay::model::OverlayModel;
use pr_app::overlay::status::{self, StatusPanel};
use pr_app::overlay::toolbar::{self, ToolbarPanel};
use pr_app::overlay::tools_menu::OverlayCommands;
use pr_app::overlay::zoom_indicator::ZoomIndicatorPanel;
use pr_app::services::render::TextRenderer;
use pr_core::settings::app_settings::AppSettings;
use pr_l10n::localizer::Localizer;

use crate::host::frame_clock::{FrameHandler, FrameHandlerId, HostFrameClock, qpc_frequency, qpc_now};
use crate::shell_overlay::host::OverlayHost;
use crate::shell_overlay::tooltips::OverlayTooltips;

/// C# `ShellOverlay.HotZoneMargin`: the toolbar's hot zone is its box inflated by this many DIP on every side.
pub const HOT_ZONE_MARGIN: f64 = 24.0;
/// C# `ShellOverlay.FadeInMs`.
pub const FADE_IN_MS: i64 = 150;
/// C# `ShellOverlay.FadeOutMs`.
pub const FADE_OUT_MS: i64 = 200;
/// Id of the info overlay layer (C# `new LayerElement("InfoOverlayHost")`).
pub const INFO_LAYER_ID: &str = "InfoOverlayHost";

/// The frame clock as the overlay uses it: three members of `HostFrameClock`. `overlay_clock` adapts the window's `Rc<dyn HostFrameClock>` for production and
/// tests use a 20-line fake (D-W8B4O-5). `FrameHandler`/`FrameTick` are the host twins of `pr_app::input::frame_clock`.
pub trait OverlayFrameClock {
    /// C# `Frame +=`.
    fn subscribe(&self, handler: FrameHandler) -> FrameHandlerId;
    /// C# `Frame -=`; an unknown id is a no-op; safe from inside a handler.
    fn unsubscribe(&self, id: FrameHandlerId);
    /// C# `RequestFrame()`.
    fn request_frame(&self);
}

impl OverlayFrameClock for Rc<dyn HostFrameClock> {
    fn subscribe(&self, handler: FrameHandler) -> FrameHandlerId {
        HostFrameClock::subscribe(self.as_ref(), handler)
    }

    fn unsubscribe(&self, id: FrameHandlerId) {
        HostFrameClock::unsubscribe(self.as_ref(), id);
    }

    fn request_frame(&self) {
        HostFrameClock::request_frame(self.as_ref());
    }
}

/// The production adapter: the window's shared clock as the overlay's clock (`Rc<Rc<dyn HostFrameClock>>` coerces; `dyn` to `dyn` upcasting between
/// unrelated traits does not exist).
pub fn overlay_clock(clock: Rc<dyn HostFrameClock>) -> Rc<dyn OverlayFrameClock> {
    Rc::new(clock)
}

/// What `OverlayMenuHost::show` ends with: the chosen item id, `None` when the user dismissed the menu, `Err` when the menu could not be built or shown
/// (C# any exception except `OutOfMemoryException`). Same shape as `shell_menus::controller::PopupMenuHost::show` of W8-14e (D-W8B4O-6).
pub type MenuShowResult = Result<Option<String>, Box<dyn std::error::Error>>;

/// The popup menu service the Tools button uses (C# `IPopupMenuHost`): the trait is OWNED by W8-14e (`shell_menus::controller::PopupMenuHost`, same
/// `show(&self, &[MenuItemModel], PointD) -> Result<Option<String>, Box<dyn Error>>`) and renamed on import (D-W8B4X-3); there is no twin. `show` is MODAL: it
/// returns when the menu closes. `&self`: the window keeps receiving messages while it is open. `screen_point_dip` is in SCREEN DIP of the window's DPI (the menu's
/// top-left anchor).
pub use crate::shell_menus::controller::PopupMenuHost as OverlayMenuHost;

/// C# `OnCommandFailed(name, exception)` as `(name, message)`.
pub type CommandFailedHandler = Box<dyn Fn(&str, &str)>;

/// C# `OnMenuFailed(exception)`.
pub type MenuFailedHandler = Box<dyn Fn(&(dyn std::error::Error + 'static))>;

/// The five buttons of the overlay (C# `_hot`/`_pressed` held `ButtonElement` references; Rust holds this id, the element lives in the host).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OverlayButton {
    /// The open-folder button of the toolbar.
    OpenFolder,
    /// The settings button of the toolbar.
    Settings,
    /// The fit button of the toolbar.
    Fit,
    /// The tools (three dots) button of the toolbar.
    Tools,
    /// The "view list" button of the skipped-files row (`StatusPanel.ViewList`).
    ViewList,
}

impl OverlayButton {
    /// The element id (`OpenFolderButton`, `SettingsButton`, `FitButton`, `ToolsButton`, `SkippedViewListButton`).
    pub fn element_id(self) -> &'static str {
        match self {
            Self::OpenFolder => toolbar::OPEN_FOLDER_ID,
            Self::Settings => toolbar::SETTINGS_ID,
            Self::Fit => toolbar::FIT_ID,
            Self::Tools => toolbar::TOOLS_ID,
            Self::ViewList => status::SKIPPED_VIEW_LIST_ID,
        }
    }

    /// The inverse of [`OverlayButton::element_id`]; `None` for any other id.
    pub fn from_element_id(id: &str) -> Option<OverlayButton> {
        [Self::OpenFolder, Self::Settings, Self::Fit, Self::Tools, Self::ViewList]
            .into_iter()
            .find(|button| button.element_id() == id)
    }
}

/// C# `ShellOverlayOptions` (no hidden defaults: every dependency is given). Command failures: see `contracts/W8-15i.md` (the command methods are
/// synchronous and report through their implementation; `on_command_failed` is kept for the Tools menu).
pub struct ShellOverlayOptions {
    /// C# `Text`.
    pub text: Rc<dyn TextRenderer>,
    /// Identity of `text` for the layout caches (`OverlayLayoutContext::text_renderer_id`, contract W8-15c O2): a fresh id per renderer instance.
    pub text_renderer_id: u64,
    /// The localizer used to build the panels (C# read the static `Tr`/`Localizer.Current`); `apply_localization` takes the current one later.
    pub texts: Arc<Localizer>,
    /// C# `Clock`.
    pub clock: Rc<dyn OverlayFrameClock>,
    /// C# `Settings`: read at every evaluation, never cached.
    pub settings: Box<dyn Fn() -> Arc<AppSettings>>,
    /// C# `Commands`.
    pub commands: Box<dyn OverlayCommands>,
    /// C# `RequestRender`.
    pub request_render: Box<dyn Fn()>,
    /// C# `IsWindowActive`.
    pub is_window_active: Box<dyn Fn() -> bool>,
    /// C# `Menus` (`None`: the Tools button does nothing).
    pub menus: Option<Rc<dyn OverlayMenuHost>>,
    /// C# `Tooltips`.
    pub tooltips: Option<Rc<dyn OverlayTooltips>>,
    /// C# `ClientOriginOnScreenDip` (default `(0, 0)`).
    pub client_origin_on_screen_dip: Option<Box<dyn Fn() -> PointD>>,
    /// C# `OnCommandFailed(name, exception)`: `(name, message)`.
    pub on_command_failed: Option<CommandFailedHandler>,
    /// C# `OnMenuFailed(exception)`; `None` falls back to `on_command_failed("Tools menu", ..)`.
    pub on_menu_failed: Option<MenuFailedHandler>,
    /// C# `Timestamp` (default: the QPC timestamp, `host::frame_clock::qpc_now`).
    pub timestamp: Option<Box<dyn Fn() -> i64>>,
    /// C# `TicksPerSecond` (default `qpc_frequency()`).
    pub ticks_per_second: Option<i64>,
    /// C# `AnimatorTicksPerSecond` (default `qpc_frequency()`).
    pub animator_ticks_per_second: Option<i64>,
}

impl fmt::Debug for ShellOverlayOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellOverlayOptions")
            .field("text_renderer_id", &self.text_renderer_id)
            .field("menus", &self.menus.is_some())
            .field("tooltips", &self.tooltips.is_some())
            .field("ticks_per_second", &self.ticks_per_second)
            .field("animator_ticks_per_second", &self.animator_ticks_per_second)
            .finish_non_exhaustive()
    }
}

/// The mutable state (C# private fields). One `RefCell`; no borrow is held across a call to an injected callback, a seam or the OS (D-W8B4O-7).
pub(super) struct OverlayState {
    /// C# `_host`: roots `InfoOverlayHost` then `ToolbarPanel`.
    pub(super) host: OverlayHost,
    /// C# `_animator`.
    pub(super) animator: Animator,
    /// C# `_toolbarTimer`.
    pub(super) toolbar_timer: FrameDelayTimer,
    /// C# `_infoTimer`.
    pub(super) info_timer: FrameDelayTimer,
    /// C# `_toolbarGate`.
    pub(super) toolbar_gate: FadeTargetGate,
    /// C# `_infoGate`.
    pub(super) info_gate: FadeTargetGate,
    /// C# `_tooltipScratch` (reused list).
    pub(super) tooltip_scratch: Vec<crate::shell_overlay::tooltips::OverlayTooltip>,
    /// C# `_model`.
    pub(super) model: OverlayModel,
    /// C# `_hot`.
    pub(super) hot: Option<OverlayButton>,
    /// C# `_pressed`.
    pub(super) pressed: Option<OverlayButton>,
    /// C# `_pointerDown`.
    pub(super) pointer_down: bool,
    /// C# `_toolbarMouseInsideHotZone` (initially `true`: "inside" until the mouse says otherwise, as WPF).
    pub(super) toolbar_mouse_inside_hot_zone: bool,
    /// C# `_toolbarWasKeptVisible` (initially `true`).
    pub(super) toolbar_was_kept_visible: bool,
    /// C# `_toolsOpen`.
    pub(super) tools_open: bool,
    /// C# `_infoIdleElapsed`.
    pub(super) info_idle_elapsed: bool,
    /// C# `_infoWasKeptVisible` (initially `true`).
    pub(super) info_was_kept_visible: bool,
    /// C# `_lastClient`.
    pub(super) last_client: SizeD,
    /// C# `_lastDpi` (initially `1`).
    pub(super) last_dpi: f64,
    /// C# `_disposed`.
    pub(super) disposed: bool,
    /// The single frame-clock subscription (D-W8B4O-8): present while the animator runs or either timer runs.
    pub(super) frame_subscription: Option<FrameHandlerId>,
}

impl OverlayState {
    /// The toolbar root (`None` after dispose).
    pub(super) fn toolbar(&self) -> Option<&ToolbarPanel> {
        self.host.root::<ToolbarPanel>(toolbar::PANEL_ID)
    }

    /// Mutable twin of [`OverlayState::toolbar`].
    pub(super) fn toolbar_mut(&mut self) -> Option<&mut ToolbarPanel> {
        self.host.root_mut::<ToolbarPanel>(toolbar::PANEL_ID)
    }

    /// The info layer root (`None` after dispose).
    pub(super) fn info_layer(&self) -> Option<&LayerElement> {
        self.host.root::<LayerElement>(INFO_LAYER_ID)
    }

    /// Mutable twin of [`OverlayState::info_layer`].
    pub(super) fn info_layer_mut(&mut self) -> Option<&mut LayerElement> {
        self.host.root_mut::<LayerElement>(INFO_LAYER_ID)
    }

    /// The `ButtonElement` behind an [`OverlayButton`] (`None` after dispose): the C# `_hot`/`_pressed` references.
    pub(super) fn button_mut(&mut self, button: OverlayButton) -> Option<&mut ButtonElement> {
        match button {
            OverlayButton::ViewList => Some(self.info_layer_mut()?.child_mut::<StatusPanel>(0)?.view_list_mut()),
            toolbar_button => {
                let [open_folder, settings, fit, tools] = self.toolbar_mut()?.buttons_mut();
                Some(match toolbar_button {
                    OverlayButton::OpenFolder => open_folder,
                    OverlayButton::Settings => settings,
                    OverlayButton::Fit => fit,
                    _ => tools,
                })
            }
        }
    }
}

/// C# `ShellOverlay`. Created with [`ShellOverlay::new`] as an `Rc` (the frame handler holds a `Weak`). UI thread only.
pub struct ShellOverlay {
    pub(super) state: RefCell<OverlayState>,
    pub(super) commands: RefCell<Box<dyn OverlayCommands>>,
    pub(super) text: Rc<dyn TextRenderer>,
    pub(super) text_renderer_id: u64,
    pub(super) clock: Rc<dyn OverlayFrameClock>,
    pub(super) settings: Box<dyn Fn() -> Arc<AppSettings>>,
    pub(super) request_render: Box<dyn Fn()>,
    pub(super) is_window_active: Box<dyn Fn() -> bool>,
    pub(super) menus: Option<Rc<dyn OverlayMenuHost>>,
    pub(super) tooltips: Option<Rc<dyn OverlayTooltips>>,
    pub(super) client_origin_on_screen_dip: Option<Box<dyn Fn() -> PointD>>,
    pub(super) on_command_failed: Option<CommandFailedHandler>,
    pub(super) on_menu_failed: Option<MenuFailedHandler>,
    pub(super) timestamp: Box<dyn Fn() -> i64>,
    #[allow(dead_code)] // kept for symmetry with the timers' own copy; the C# options carried it
    pub(super) ticks_per_second: i64,
    /// `Rc::new_cyclic` handle for the frame handler.
    pub(super) weak_self: Weak<ShellOverlay>,
    /// Re-entrancy guard of `dispose` (C# `_disposed` is checked first in every entry point; this one makes `dispose` and `Drop` idempotent).
    pub(super) disposing: Cell<bool>,
}

impl fmt::Debug for ShellOverlay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = f.debug_struct("ShellOverlay");
        out.field("text_renderer_id", &self.text_renderer_id);
        match self.state.try_borrow() {
            Ok(state) => out
                .field("has_images", &state.model.has_images)
                .field("tools_open", &state.tools_open)
                .field("disposed", &state.disposed),
            Err(_) => out.field("state", &"<borrowed>"),
        };
        out.finish_non_exhaustive()
    }
}

impl ShellOverlay {
    /// C# constructor (lines 88 to 119): builds the toolbar (visible), status, folder info, zoom indicator and capture badge from `options.texts`; the info
    /// layer `InfoOverlayHost` holds the four info panels in that order; the host gets the layer, then the toolbar (on top); `Animator::new` with
    /// `animator_ticks_per_second` (default QPC frequency), two `FrameDelayTimer::new(ticks_per_second)`; then `apply_model`,
    /// `apply_toolbar_visibility`, `apply_info_overlay_visibility`. The click subscriptions of the C# constructor (`Toolbar.OpenFolder.Clicked += ...`)
    /// are the id dispatch of `input::dispatch_click`. Invalid ticks-per-second (`<= 0`) is a programming error: panic with the `AnimatorError` text.
    pub fn new(options: ShellOverlayOptions) -> Rc<ShellOverlay> {
        let ShellOverlayOptions {
            text,
            text_renderer_id,
            texts,
            clock,
            settings,
            commands,
            request_render,
            is_window_active,
            menus,
            tooltips,
            client_origin_on_screen_dip,
            on_command_failed,
            on_menu_failed,
            timestamp,
            ticks_per_second,
            animator_ticks_per_second,
        } = options;
        let ticks_per_second = ticks_per_second.unwrap_or_else(qpc_frequency);
        let animator_ticks_per_second = animator_ticks_per_second.unwrap_or_else(qpc_frequency);

        let mut toolbar_panel = ToolbarPanel::new(&texts);
        toolbar_panel.base_mut().is_visible = true;
        let mut info_layer = LayerElement::new(INFO_LAYER_ID);
        info_layer
            .add(Box::new(StatusPanel::new(&texts)))
            .add(Box::new(FolderInfoPanel::new(&texts)))
            .add(Box::new(ZoomIndicatorPanel::new(&texts)))
            .add(Box::new(CapturePairBadge::new()));
        let mut host = OverlayHost::new();
        // Fresh host, distinct constant ids: `add` cannot fail here.
        host.add(Box::new(info_layer))
            .unwrap_or_else(|error| panic!("overlay host rejected the info layer: {error}"));
        host.add(Box::new(toolbar_panel))
            .unwrap_or_else(|error| panic!("overlay host rejected the toolbar: {error}"));

        let animator = Animator::new(animator_ticks_per_second).unwrap_or_else(|error| panic!("{error}"));
        let toolbar_timer = FrameDelayTimer::new(ticks_per_second).unwrap_or_else(|error| panic!("{error}"));
        let info_timer = FrameDelayTimer::new(ticks_per_second).unwrap_or_else(|error| panic!("{error}"));

        let state = OverlayState {
            host,
            animator,
            toolbar_timer,
            info_timer,
            toolbar_gate: FadeTargetGate::new(),
            info_gate: FadeTargetGate::new(),
            tooltip_scratch: Vec::new(),
            model: OverlayModel::default(),
            hot: None,
            pressed: None,
            pointer_down: false,
            toolbar_mouse_inside_hot_zone: true,
            toolbar_was_kept_visible: true,
            tools_open: false,
            info_idle_elapsed: false,
            info_was_kept_visible: true,
            last_client: SizeD::default(),
            last_dpi: 1.0,
            disposed: false,
            frame_subscription: None,
        };
        let overlay = Rc::new_cyclic(|weak| ShellOverlay {
            state: RefCell::new(state),
            commands: RefCell::new(commands),
            text,
            text_renderer_id,
            clock,
            settings,
            request_render,
            is_window_active,
            menus,
            tooltips,
            client_origin_on_screen_dip,
            on_command_failed,
            on_menu_failed,
            timestamp: timestamp.unwrap_or_else(|| Box::new(qpc_now)),
            ticks_per_second,
            weak_self: weak.clone(),
            disposing: Cell::new(false),
        });
        overlay.apply_model();
        overlay.apply_toolbar_visibility();
        overlay.apply_info_overlay_visibility();
        overlay
    }

    /// C# `Model`: a clone of the current model.
    pub fn model(&self) -> OverlayModel {
        self.state.borrow().model.clone()
    }

    /// C# `IsToolsMenuOpen`.
    pub fn is_tools_menu_open(&self) -> bool {
        self.state.borrow().tools_open
    }

    /// C# `HasKeyboardFocus` (`Toolbar.HasKeyboardFocus`); `false` after dispose.
    pub fn has_keyboard_focus(&self) -> bool {
        self.state
            .borrow()
            .toolbar()
            .is_some_and(ToolbarPanel::has_keyboard_focus)
    }

    /// C# `Toolbar` as a closure accessor (the state is in a `RefCell`). Panics after `dispose` (the host released its roots); `read` must not call back into
    /// the overlay's mutating methods.
    pub fn with_toolbar<R>(&self, read: impl FnOnce(&ToolbarPanel) -> R) -> R {
        let state = self.state.borrow();
        read(state.toolbar().expect("ShellOverlay is disposed"))
    }

    /// C# `Status` (same rules as [`ShellOverlay::with_toolbar`]).
    pub fn with_status<R>(&self, read: impl FnOnce(&StatusPanel) -> R) -> R {
        let state = self.state.borrow();
        let panel = state
            .info_layer()
            .and_then(|layer| layer.child::<StatusPanel>(0))
            .expect("ShellOverlay is disposed");
        read(panel)
    }

    /// C# `FolderInfo` (same rules as [`ShellOverlay::with_toolbar`]).
    pub fn with_folder_info<R>(&self, read: impl FnOnce(&FolderInfoPanel) -> R) -> R {
        let state = self.state.borrow();
        let panel = state
            .info_layer()
            .and_then(|layer| layer.child::<FolderInfoPanel>(1))
            .expect("ShellOverlay is disposed");
        read(panel)
    }

    /// C# `ZoomIndicator` (same rules as [`ShellOverlay::with_toolbar`]).
    pub fn with_zoom_indicator<R>(&self, read: impl FnOnce(&ZoomIndicatorPanel) -> R) -> R {
        let state = self.state.borrow();
        let panel = state
            .info_layer()
            .and_then(|layer| layer.child::<ZoomIndicatorPanel>(2))
            .expect("ShellOverlay is disposed");
        read(panel)
    }

    /// C# `CapturePair` (same rules as [`ShellOverlay::with_toolbar`]).
    pub fn with_capture_pair<R>(&self, read: impl FnOnce(&CapturePairBadge) -> R) -> R {
        let state = self.state.borrow();
        let panel = state
            .info_layer()
            .and_then(|layer| layer.child::<CapturePairBadge>(3))
            .expect("ShellOverlay is disposed");
        read(panel)
    }

    /// C# `InfoLayer` (same rules as [`ShellOverlay::with_toolbar`]).
    pub fn with_info_layer<R>(&self, read: impl FnOnce(&LayerElement) -> R) -> R {
        let state = self.state.borrow();
        read(state.info_layer().expect("ShellOverlay is disposed"))
    }

    /// Opacity of the toolbar root (C# `Toolbar.Opacity`); `0` after dispose.
    pub fn toolbar_opacity(&self) -> f32 {
        self.state.borrow().toolbar().map_or(0.0, |t| t.base().opacity())
    }

    /// `Toolbar.IsHitTestVisible`; `false` after dispose.
    pub fn toolbar_hit_test_visible(&self) -> bool {
        self.state
            .borrow()
            .toolbar()
            .is_some_and(|t| t.base().is_hit_test_visible)
    }

    /// Opacity of the info layer (`InfoLayer.Opacity`); `0` after dispose.
    pub fn info_opacity(&self) -> f32 {
        self.state
            .borrow()
            .info_layer()
            .map_or(0.0, |layer| layer.base().opacity())
    }

    /// `InfoLayer.IsHitTestVisible`; `false` after dispose.
    pub fn info_hit_test_visible(&self) -> bool {
        self.state
            .borrow()
            .info_layer()
            .is_some_and(|layer| layer.base().is_hit_test_visible)
    }

    /// True while the single frame subscription exists (the test twin of C# `Clock.SubscriberCount > 0`).
    pub fn is_frame_subscribed(&self) -> bool {
        self.state.borrow().frame_subscription.is_some()
    }

    /// The test twin of C# `Animator.IsAnimating`: at least one fade is running.
    pub fn is_animating(&self) -> bool {
        self.state.borrow().animator.is_animating()
    }

    /// True after `dispose` (C# `_disposed`).
    pub(super) fn is_disposed(&self) -> bool {
        self.state.borrow().disposed
    }

    /// C# `Update(model)` (lines 153 to 180): ignored after dispose. Stores the model, `apply_model`; `has_images` changed: `apply_toolbar_visibility`;
    /// `compare_visible` changed: `note_info_activity`; else when `has_images`, `status_text`, `has_skipped_entries` or `status_needs_attention` changed:
    /// `apply_info_overlay_visibility`; always `request_render`.
    pub fn update(&self, model: OverlayModel) {
        let old = {
            let mut state = self.state.borrow_mut();
            if state.disposed {
                return;
            }
            std::mem::replace(&mut state.model, model)
        };
        self.apply_model();
        let new = self.model();
        if old.has_images != new.has_images {
            self.apply_toolbar_visibility();
        }
        if old.compare_visible != new.compare_visible {
            self.note_info_activity();
        } else if old.has_images != new.has_images
            || old.status_text != new.status_text
            || old.has_skipped_entries != new.has_skipped_entries
            || old.status_needs_attention != new.status_needs_attention
        {
            self.apply_info_overlay_visibility();
        }
        (self.request_render)();
    }

    /// C# `SettingsChanged()` (lines 183 to 193): `apply_toolbar_visibility`, `note_info_activity`, `request_render`.
    pub fn settings_changed(&self) {
        if self.is_disposed() {
            return;
        }
        self.apply_toolbar_visibility();
        self.note_info_activity();
        (self.request_render)();
    }

    /// C# `ApplyLocalization()` (lines 196 to 208): `ToolbarPanel`, `StatusPanel`, `FolderInfoPanel`, `ZoomIndicatorPanel` re-read their texts from `texts`
    /// (the capture badge has none); `request_render`.
    pub fn apply_localization(&self, texts: &Localizer) {
        {
            let mut state = self.state.borrow_mut();
            if state.disposed {
                return;
            }
            if let Some(toolbar_panel) = state.toolbar_mut() {
                toolbar_panel.apply_localization(texts);
            }
            if let Some(layer) = state.info_layer_mut() {
                if let Some(panel) = layer.child_mut::<StatusPanel>(0) {
                    panel.apply_localization(texts);
                }
                if let Some(panel) = layer.child_mut::<FolderInfoPanel>(1) {
                    panel.apply_localization(texts);
                }
                if let Some(panel) = layer.child_mut::<ZoomIndicatorPanel>(2) {
                    panel.apply_localization(texts);
                }
            }
        }
        (self.request_render)();
    }

    /// C# `NoteActivity()` (lines 211 to 217): non-mouse user activity (image change, scroll, key).
    pub fn note_activity(&self) {
        if !self.is_disposed() {
            self.note_info_activity();
        }
    }

    /// C# `WindowActivated()`: activity.
    pub fn window_activated(&self) {
        self.note_activity();
    }

    /// C# `WindowDeactivated()` (lines 223 to 229): `apply_info_overlay_visibility` (the info overlay must show while another window has the focus).
    pub fn window_deactivated(&self) {
        if !self.is_disposed() {
            self.apply_info_overlay_visibility();
        }
    }

    /// C# `ApplyModel()` (lines 586 to 592): `Status.Apply`, `FolderInfo.Apply`, `ZoomIndicator.Apply`, `CapturePair.Apply`.
    pub(super) fn apply_model(&self) {
        let mut guard = self.state.borrow_mut();
        let state = &mut *guard;
        let Some(layer) = state.host.root_mut::<LayerElement>(INFO_LAYER_ID) else {
            return;
        };
        let model = &state.model;
        if let Some(panel) = layer.child_mut::<StatusPanel>(0) {
            panel.apply(model);
        }
        if let Some(panel) = layer.child_mut::<FolderInfoPanel>(1) {
            panel.apply(model);
        }
        if let Some(panel) = layer.child_mut::<ZoomIndicatorPanel>(2) {
            panel.apply(model);
        }
        if let Some(panel) = layer.child_mut::<CapturePairBadge>(3) {
            panel.apply(model);
        }
    }

    /// C# `Dispose()` (lines 660 to 674): idempotent; unsubscribes the frame handler, disposes both timers and the animator, disposes the tooltips, releases the
    /// host (text layout leases). After it every entry point is a no-op (`false` for the boolean ones).
    pub fn dispose(&self) {
        if self.disposing.replace(true) {
            return;
        }
        let subscription = {
            let mut state = self.state.borrow_mut();
            state.disposed = true;
            state.animator.dispose();
            state.toolbar_timer.dispose();
            state.info_timer.dispose();
            state.frame_subscription.take()
        };
        if let Some(id) = subscription {
            self.clock.unsubscribe(id);
        }
        if let Some(tooltips) = &self.tooltips {
            tooltips.dispose();
        }
        self.state.borrow_mut().host.dispose();
    }
}

impl Drop for ShellOverlay {
    /// Same as `dispose` (idempotent).
    fn drop(&mut self) {
        self.dispose();
    }
}
