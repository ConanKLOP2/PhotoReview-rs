//! `ShellOverlay` part 1: options, seams, state, constructor, accessors, data entry points and `dispose`. Port of `ShellOverlay.cs` lines 12 to 229 and 586
//! to 592 (`ApplyModel`) and 660 to 674 (`Dispose`), baseline `d92d6aae`. Unit W8-15h (contract `contracts/W8-15h.md`).
//!
//! This file declares the struct with ALL fields (the W8-13e `create.rs` precedent); the other files of this module add `impl ShellOverlay` blocks.
//!
//! Ownership (D-W8B4O-4): C# kept `Toolbar`, `Status`, ... as properties AND inside the host's roots. Rust owns the elements ONCE, inside the
//! [`OverlayHost`]: roots `InfoOverlayHost` (a `LayerElement` holding `StatusPanel`, `FolderInfoPanel`, `ZoomIndicatorPanel`, `CapturePairBadge` in that
//! order) and `ToolbarPanel` (added last = on top). The typed properties become the closure accessors `with_toolbar`, `with_status`, ... (the state is in
//! a `RefCell`, so a plain `&ToolbarPanel` cannot be returned). Mutation goes through `OverlayHost::root_mut` and `LayerElement::child_mut` (a `pr_app`
//! amendment, see the contract).

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pr_app::coordinators::fade_gate::FadeTargetGate;
use pr_app::input::primitives::{PointD, SizeD};
use pr_app::overlay::animator::Animator;
use pr_app::overlay::capture_badge::CapturePairBadge;
use pr_app::overlay::folder_info::FolderInfoPanel;
use pr_app::overlay::frame_delay::FrameDelayTimer;
use pr_app::overlay::layer::LayerElement;
use pr_app::overlay::model::OverlayModel;
use pr_app::overlay::status::StatusPanel;
use pr_app::overlay::toolbar::ToolbarPanel;
use pr_app::overlay::tools_menu::OverlayCommands;
use pr_app::services::render::TextRenderer;
use pr_core::settings::app_settings::AppSettings;
use pr_l10n::localizer::Localizer;

use crate::host::frame_clock::{FrameHandler, FrameHandlerId, HostFrameClock};
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
    OpenFolder,
    Settings,
    Fit,
    Tools,
    /// The "view list" button of the skipped-files row (`StatusPanel.ViewList`).
    ViewList,
}

impl OverlayButton {
    /// The element id (`OpenFolderButton`, `SettingsButton`, `FitButton`, `ToolsButton`, `SkippedViewListButton`).
    pub fn element_id(self) -> &'static str {
        todo!()
    }

    /// The inverse of [`OverlayButton::element_id`]; `None` for any other id.
    pub fn from_element_id(id: &str) -> Option<OverlayButton> {
        todo!()
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
        todo!()
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
    pub(super) ticks_per_second: i64,
    /// `Rc::new_cyclic` handle for the frame handler.
    pub(super) weak_self: Weak<ShellOverlay>,
    /// Re-entrancy guard of `dispose` (C# `_disposed` is checked first in every entry point; this one is only for the `Drop` path).
    pub(super) disposing: Cell<bool>,
}

impl fmt::Debug for ShellOverlay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl ShellOverlay {
    /// C# constructor (lines 88 to 119): builds the toolbar (visible), status, folder info, zoom indicator and capture badge from `options.texts`; the info
    /// layer `InfoOverlayHost` holds the four info panels in that order; the host gets the layer, then the toolbar (on top); `Animator::new` with
    /// `animator_ticks_per_second` (default QPC frequency), two `FrameDelayTimer::new(ticks_per_second)`; then `apply_model`,
    /// `apply_toolbar_visibility`, `apply_info_overlay_visibility`. The click subscriptions of the C# constructor (`Toolbar.OpenFolder.Clicked += ...`)
    /// are the id dispatch of `input::dispatch_click`. Invalid ticks-per-second (`<= 0`) is a programming error: panic with the `AnimatorError` text.
    pub fn new(options: ShellOverlayOptions) -> Rc<ShellOverlay> {
        todo!()
    }

    /// C# `Model`: a clone of the current model.
    pub fn model(&self) -> OverlayModel {
        todo!()
    }

    /// C# `IsToolsMenuOpen`.
    pub fn is_tools_menu_open(&self) -> bool {
        todo!()
    }

    /// C# `HasKeyboardFocus` (`Toolbar.HasKeyboardFocus`).
    pub fn has_keyboard_focus(&self) -> bool {
        todo!()
    }

    /// C# `Toolbar` as a closure accessor (the state is in a `RefCell`).
    pub fn with_toolbar<R>(&self, read: impl FnOnce(&ToolbarPanel) -> R) -> R {
        todo!()
    }

    /// C# `Status`.
    pub fn with_status<R>(&self, read: impl FnOnce(&StatusPanel) -> R) -> R {
        todo!()
    }

    /// C# `FolderInfo`.
    pub fn with_folder_info<R>(&self, read: impl FnOnce(&FolderInfoPanel) -> R) -> R {
        todo!()
    }

    /// C# `ZoomIndicator`.
    pub fn with_zoom_indicator<R>(
        &self,
        read: impl FnOnce(&pr_app::overlay::zoom_indicator::ZoomIndicatorPanel) -> R,
    ) -> R {
        todo!()
    }

    /// C# `CapturePair`.
    pub fn with_capture_pair<R>(&self, read: impl FnOnce(&CapturePairBadge) -> R) -> R {
        todo!()
    }

    /// C# `InfoLayer`.
    pub fn with_info_layer<R>(&self, read: impl FnOnce(&LayerElement) -> R) -> R {
        todo!()
    }

    /// Opacity of the toolbar root (C# `Toolbar.Opacity`).
    pub fn toolbar_opacity(&self) -> f32 {
        todo!()
    }

    /// `Toolbar.IsHitTestVisible`.
    pub fn toolbar_hit_test_visible(&self) -> bool {
        todo!()
    }

    /// Opacity of the info layer (`InfoLayer.Opacity`).
    pub fn info_opacity(&self) -> f32 {
        todo!()
    }

    /// `InfoLayer.IsHitTestVisible`.
    pub fn info_hit_test_visible(&self) -> bool {
        todo!()
    }

    /// True while the single frame subscription exists (the test twin of C# `Clock.SubscriberCount > 0`).
    pub fn is_frame_subscribed(&self) -> bool {
        todo!()
    }

    /// C# `Update(model)` (lines 153 to 180): ignored after dispose. Stores the model, `apply_model`; `has_images` changed: `apply_toolbar_visibility`;
    /// `compare_visible` changed: `note_info_activity`; else when `has_images`, `status_text`, `has_skipped_entries` or `status_needs_attention` changed:
    /// `apply_info_overlay_visibility`; always `request_render`.
    pub fn update(&self, model: OverlayModel) {
        todo!()
    }

    /// C# `SettingsChanged()` (lines 183 to 193): `apply_toolbar_visibility`, `note_info_activity`, `request_render`.
    pub fn settings_changed(&self) {
        todo!()
    }

    /// C# `ApplyLocalization()` (lines 196 to 208): `ToolbarPanel`, `StatusPanel`, `FolderInfoPanel`, `ZoomIndicatorPanel` re-read their texts from `texts`
    /// (the capture badge has none); `request_render`.
    pub fn apply_localization(&self, texts: &Localizer) {
        todo!()
    }

    /// C# `NoteActivity()` (lines 211 to 217): non-mouse user activity (image change, scroll, key).
    pub fn note_activity(&self) {
        todo!()
    }

    /// C# `WindowActivated()`: activity.
    pub fn window_activated(&self) {
        todo!()
    }

    /// C# `WindowDeactivated()` (lines 223 to 229): `apply_info_overlay_visibility` (the info overlay must show while another window has the focus).
    pub fn window_deactivated(&self) {
        todo!()
    }

    /// C# `ApplyModel()` (lines 586 to 592): `Status.Apply`, `FolderInfo.Apply`, `ZoomIndicator.Apply`, `CapturePair.Apply`.
    pub(super) fn apply_model(&self) {
        todo!()
    }

    /// C# `Dispose()` (lines 660 to 674): idempotent; unsubscribes the frame handler, disposes both timers and the animator, disposes the tooltips, releases the
    /// host (text layout leases). After it every entry point is a no-op (`false` for the boolean ones).
    pub fn dispose(&self) {
        todo!()
    }
}

impl Drop for ShellOverlay {
    /// Same as `dispose` (idempotent).
    fn drop(&mut self) {
        todo!()
    }
}
