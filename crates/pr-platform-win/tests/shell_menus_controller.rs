//! Tests of W8-14e `shell_menus::{controller, message_handler}` (C# `ContextMenuControllerTests`, `ContextMenuMessageHandlerTests`, baseline
//! `d92d6aae`). Pure: fake actions, fake popup host, fake shell window. No native call, no real menu.
//!
//! Rust notes: no `Task`/cancellation (`ACancelledCommand_IsNotAnError` is retired, D-W8B4I-8); the G-MENU golden of the C# repo
//! (`context-menu.v1.json`) is not synced, so the golden-driven test replays the W8-02a golden `menu-model` through the controller (contract Q2) and the
//! C# 12-configuration test skips with a message until the file exists.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::error::Error;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_app::input::primitives::{PointD, SizeD};
use pr_app::input::review_command::ReviewCommand;
use pr_app::menus::model::{MenuItemKind, MenuItemModel};
use pr_core::abstractions::Log;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::context_menu::{ContextMenuItemId, apply_hidden};
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_platform_win::host::contracts::{
    ClosingHandler, EventHandlerId, ShellWindowApi, ShellWindowEvent, WindowEventHandler, WindowMessage,
    WindowMessageHandler,
};
use pr_platform_win::host::error::HostError;
use pr_platform_win::shell_menus::controller::{
    ActionResult, ContextMenuActions, ContextMenuController, PopupMenuHost, ZOOM_PRESET_PREFIX, try_parse_preset,
};
use pr_platform_win::shell_menus::message_handler::ContextMenuMessageHandler;
use pr_platform_win::win32::messages::{WM_CONTEXTMENU, WM_KEYDOWN, WM_LBUTTONDOWN, WM_RBUTTONDOWN, WM_RBUTTONUP};
use pr_platform_win::win32::structs::{Hwnd, Lparam};
use pr_test_support::{LogLevel, RecordingLog, golden};
use serde_json::{Value, json};

use ContextMenuItemId as Id;

// ---------------------------------------------------------------------------------------------------------------------
// English pin (the menu texts come from the ambient localizer)
// ---------------------------------------------------------------------------------------------------------------------

static GLOBAL: Mutex<()> = Mutex::new(());

thread_local! {
    /// Nesting depth of `English` guards on this thread (a test may hold several environments at once; the lock is taken once).
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct English {
    previous: Option<Arc<Localizer>>,
    lock: Option<MutexGuard<'static, ()>>,
}

fn english() -> English {
    let first = DEPTH.with(|d| {
        d.set(d.get() + 1);
        d.get() == 1
    });
    if first {
        let lock = GLOBAL.lock().unwrap_or_else(PoisonError::into_inner);
        let previous = Localizer::current();
        Localizer::set_current(built_in_english_localizer());
        English {
            previous: Some(previous),
            lock: Some(lock),
        }
    } else {
        English {
            previous: None,
            lock: None,
        }
    }
}

impl Drop for English {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            Localizer::set_current(previous);
        }
        self.lock.take();
        DEPTH.with(|d| d.set(d.get() - 1));
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------------------------------------------------

fn boom(label: &str) -> Box<dyn Error> {
    Box::new(io::Error::other(format!("boom {label}")))
}

/// C# `FakeContextMenuActions`: every call is one line of `log`.
struct FakeActions {
    settings: RefCell<Arc<AppSettings>>,
    has_image: Cell<bool>,
    can_copy_path: Cell<bool>,
    can_undo: Cell<bool>,
    can_open_in_external_editor: Cell<bool>,
    is_compare_visible: Cell<bool>,
    effective_zoom: Cell<Option<f64>>,
    log: RefCell<Vec<String>>,
    /// `controller.is_open()` at the moment each command ran.
    open_at_command: RefCell<Vec<bool>>,
    controller: RefCell<Weak<ContextMenuController>>,
    /// Every command fails (C# `Failure`).
    failing: Cell<bool>,
}

impl FakeActions {
    fn new(settings: AppSettings) -> Self {
        Self {
            settings: RefCell::new(Arc::new(settings)),
            has_image: Cell::new(true),
            can_copy_path: Cell::new(true),
            can_undo: Cell::new(true),
            can_open_in_external_editor: Cell::new(true),
            is_compare_visible: Cell::new(false),
            effective_zoom: Cell::new(Some(1.0)),
            log: RefCell::new(Vec::new()),
            open_at_command: RefCell::new(Vec::new()),
            controller: RefCell::new(Weak::new()),
            failing: Cell::new(false),
        }
    }

    fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn set_settings(&self, settings: AppSettings) {
        *self.settings.borrow_mut() = Arc::new(settings);
    }

    fn record(&self, entry: &str) -> ActionResult {
        self.log.borrow_mut().push(entry.to_owned());
        let open = self.controller.borrow().upgrade().map(|c| c.is_open());
        if let Some(open) = open {
            self.open_at_command.borrow_mut().push(open);
        }
        if self.failing.get() { Err(boom(entry)) } else { Ok(()) }
    }
}

impl ContextMenuActions for FakeActions {
    fn settings(&self) -> Arc<AppSettings> {
        Arc::clone(&self.settings.borrow())
    }
    fn has_image(&self) -> bool {
        self.has_image.get()
    }
    fn can_copy_path(&self) -> bool {
        self.can_copy_path.get()
    }
    fn can_undo(&self) -> bool {
        self.can_undo.get()
    }
    fn can_open_in_external_editor(&self) -> bool {
        self.can_open_in_external_editor.get()
    }
    fn is_compare_visible(&self) -> bool {
        self.is_compare_visible.get()
    }
    fn effective_zoom(&self) -> Option<f64> {
        self.effective_zoom.get()
    }
    fn execute(&self, command: ReviewCommand) -> ActionResult {
        self.record(&format!("Execute:{:?}", command.kind))
    }
    fn fit(&self) -> ActionResult {
        self.record("Fit")
    }
    fn zoom_to_configured_level(&self) -> ActionResult {
        self.record("ZoomToConfiguredLevel")
    }
    fn apply_zoom_preset(&self, percent: i32) -> ActionResult {
        self.record(&format!("Preset:{percent}"))
    }
    fn custom_zoom(&self) -> ActionResult {
        self.record("CustomZoom")
    }
    fn set_zoom_also_sets_click_level(&self, value: bool) {
        self.log.borrow_mut().push(format!("AlsoSet:{value}"));
    }
    fn set_click_zoom_percent(&self, percent: i32) {
        self.log.borrow_mut().push(format!("SetClickLevel:{percent}"));
    }
    fn open_in_external_editor(&self) -> ActionResult {
        self.record("ExternalEditor")
    }
    fn open_in_external_editor_by_id(&self, editor_id: &str) -> ActionResult {
        self.record(&format!("ExternalEditor:{editor_id}"))
    }
    fn add_external_editor(&self) -> ActionResult {
        self.record("AddEditor")
    }
    fn choose_default_external_editor(&self) -> ActionResult {
        self.record("SetDefaultEditor")
    }
    fn copy_file_name(&self) -> ActionResult {
        self.record("CopyFileName")
    }
    fn copy_full_pathname(&self) -> ActionResult {
        self.record("CopyFullPathname")
    }
    fn show_settings(&self) -> ActionResult {
        self.record("ShowSettings")
    }
    fn report_action_failed(&self, action_name: &str, error: &dyn Error) {
        self.log.borrow_mut().push(format!("failed:{action_name}:{error}"));
    }
    // `report_menu_failed` is NOT overridden: the trait default (`report_action_failed("Menu", ..)`) is part of the behaviour under test.
    fn zoom_action_name(&self) -> String {
        "Zoom-localized".to_owned()
    }
}

enum Behavior {
    Choose(Option<String>),
    Fail(&'static str),
    Panic,
    /// Calls `controller.show` from inside `show` (the nested `WM_CONTEXTMENU` of the modal loop), then chooses.
    Nested(Option<String>),
    /// Replaces the host of the controller from inside `show` (a `RefCell` borrow held across the call would panic).
    ReplaceHost(Option<String>),
}

/// C# `FakePopupMenuHost`.
struct FakeHost {
    behavior: RefCell<Behavior>,
    show_count: Cell<usize>,
    last_items: RefCell<Option<Vec<MenuItemModel>>>,
    last_point: Cell<Option<PointD>>,
    controller: RefCell<Weak<ContextMenuController>>,
    open_during: Cell<Option<bool>>,
    shows_after_nested_call: Cell<usize>,
    /// Filled by `Nested`: the log of the actions when the host returns (the command must not have run yet).
    actions: RefCell<Weak<FakeActions>>,
    log_when_returning: RefCell<Vec<String>>,
}

impl FakeHost {
    fn new() -> Self {
        Self {
            behavior: RefCell::new(Behavior::Choose(None)),
            show_count: Cell::new(0),
            last_items: RefCell::new(None),
            last_point: Cell::new(None),
            controller: RefCell::new(Weak::new()),
            open_during: Cell::new(None),
            shows_after_nested_call: Cell::new(0),
            actions: RefCell::new(Weak::new()),
            log_when_returning: RefCell::new(Vec::new()),
        }
    }

    fn choose(&self, id: &str) {
        *self.behavior.borrow_mut() = Behavior::Choose(Some(id.to_owned()));
    }

    fn set(&self, behavior: Behavior) {
        *self.behavior.borrow_mut() = behavior;
    }

    fn last_items(&self) -> Vec<MenuItemModel> {
        self.last_items.borrow().clone().expect("the host was shown")
    }

    fn ids(&self) -> Vec<String> {
        self.last_items().iter().map(|i| i.id.clone()).collect()
    }
}

impl PopupMenuHost for FakeHost {
    fn show(&self, items: &[MenuItemModel], screen_point_dip: PointD) -> Result<Option<String>, Box<dyn Error>> {
        self.show_count.set(self.show_count.get() + 1);
        *self.last_items.borrow_mut() = Some(items.to_vec());
        self.last_point.set(Some(screen_point_dip));
        let controller = self.controller.borrow().upgrade();
        let take = |b: &Option<String>| b.clone();
        let result = match &*self.behavior.borrow() {
            Behavior::Choose(id) => Ok(take(id)),
            Behavior::Fail(text) => Err(boom(text)),
            Behavior::Panic => panic!("host panicked"),
            Behavior::Nested(id) => {
                let controller = controller.expect("controller");
                self.open_during.set(Some(controller.is_open()));
                controller.show(PointD::default());
                self.shows_after_nested_call.set(self.show_count.get());
                Ok(take(id))
            }
            Behavior::ReplaceHost(id) => {
                let controller = controller.expect("controller");
                controller.set_menu_host(Rc::new(FakeHost::new()));
                Ok(take(id))
            }
        };
        if let Some(actions) = self.actions.borrow().upgrade() {
            *self.log_when_returning.borrow_mut() = actions.log();
        }
        result
    }
}

struct Env {
    controller: Rc<ContextMenuController>,
    actions: Rc<FakeActions>,
    host: Rc<FakeHost>,
    _english: English,
}

fn default_settings() -> AppSettings {
    AppSettings::new(&EnglishSettingsTexts)
}

fn nothing_hidden() -> AppSettings {
    let mut settings = default_settings();
    apply_hidden(&mut settings, std::iter::empty());
    settings
}

fn create_with(settings: AppSettings, log: Option<Arc<dyn Log>>) -> Env {
    let english = english();
    let actions = Rc::new(FakeActions::new(settings));
    let host = Rc::new(FakeHost::new());
    let controller = Rc::new(ContextMenuController::new(
        Rc::clone(&actions) as Rc<dyn ContextMenuActions>,
        Rc::clone(&host) as Rc<dyn PopupMenuHost>,
        log,
    ));
    *actions.controller.borrow_mut() = Rc::downgrade(&controller);
    *host.controller.borrow_mut() = Rc::downgrade(&controller);
    *host.actions.borrow_mut() = Rc::downgrade(&actions);
    Env {
        controller,
        actions,
        host,
        _english: english,
    }
}

fn create(settings: AppSettings) -> Env {
    create_with(settings, None)
}

fn pt(x: f64, y: f64) -> PointD {
    PointD { x, y }
}

fn find<'a>(items: &'a [MenuItemModel], id: &str) -> &'a MenuItemModel {
    items
        .iter()
        .find(|i| i.id == id)
        .unwrap_or_else(|| panic!("no item {id}"))
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// Controller: ports of ContextMenuControllerTests
// ---------------------------------------------------------------------------------------------------------------------

// C#: Show_WhenTheHostThrows_ReportsAMenuFailureInsteadOfThrowing_AndCanReopen
#[test]
fn show_when_the_host_fails_reports_a_menu_failure_instead_of_failing_and_can_reopen() {
    let env = create(default_settings());
    env.host.set(Behavior::Fail("host"));

    env.controller.show(pt(1.0, 1.0));

    assert_eq!(env.actions.log(), strs(&["failed:Menu:boom host"]));
    assert!(!env.controller.is_open());
    env.host.set(Behavior::Choose(None));
    env.controller.show(pt(1.0, 1.0));
    assert_eq!(env.host.show_count.get(), 2);
}

// C#: ChoosingAnItem_RunsExactlyTheMatchingCommand (theory over every id)
#[test]
fn choosing_an_item_runs_exactly_the_matching_command() {
    let cases = [
        ("Undo", "Execute:Undo"),
        ("MoveToRecycleBin", "Execute:Recycle"),
        ("Fit", "Fit"),
        ("ZoomToLevel", "ZoomToConfiguredLevel"),
        ("ZoomFitWidth", "Execute:FitWidth"),
        ("ZoomFitWidth2", "Execute:FitWidth2"),
        ("ZoomFitHeight", "Execute:FitHeight"),
        ("Refresh", "Execute:Refresh"),
        ("OpenFolder", "Execute:OpenFolder"),
        ("NextFolder", "Execute:NextFolder"),
        ("PreviousFolder", "Execute:PreviousFolder"),
        ("ExternalEditor", "ExternalEditor"),
        ("CopyFileName", "CopyFileName"),
        ("CopyFullPath", "CopyFullPathname"),
        ("Settings", "ShowSettings"),
        ("Zoom.Custom", "CustomZoom"),
        ("Zoom.Preset.50", "Preset:50"),
        ("Zoom.Preset.70", "Preset:70"),
        ("Zoom.Preset.100", "Preset:100"),
        ("Zoom.Preset.150", "Preset:150"),
        ("Zoom.Preset.200", "Preset:200"),
        ("Zoom.Preset.300", "Preset:300"),
        ("Zoom.Preset.400", "Preset:400"),
    ];
    for (id, expected) in cases {
        let env = create(nothing_hidden());
        env.host.choose(id);

        env.controller.show(pt(10.0, 20.0));

        assert_eq!(env.actions.log(), strs(&[expected]), "{id}");
    }
}

// C#: BuildModel_AllItemsVisible_CostIsReported_NotAsserted (smoke build, no timing assertion)
#[test]
fn build_model_with_all_items_visible_has_the_top_level_items() {
    let env = create(nothing_hidden());
    let count = env.controller.build_model().expect("build").len();
    assert!(count >= 15, "count {count}");
}

// C#: Cancelling_RunsNothing
#[test]
fn cancelling_runs_nothing() {
    let env = create(default_settings());
    env.host.set(Behavior::Choose(None));

    env.controller.show(pt(1.0, 1.0));

    assert_eq!(env.host.show_count.get(), 1);
    assert!(env.actions.log().is_empty());
}

// C#: Show_PassesTheAnchorPointToTheHostUnchanged
#[test]
fn show_passes_the_anchor_point_to_the_host_unchanged() {
    let env = create(default_settings());

    env.controller.show(pt(123.5, 456.25));

    assert_eq!(env.host.last_point.get(), Some(pt(123.5, 456.25)));
}

// C#: Show_BuildsTheModelFromTheSettingsAtOpenTime_NotAtConstruction
#[test]
fn show_builds_the_model_from_the_settings_at_open_time_not_at_construction() {
    let env = create(default_settings());

    env.controller.show(PointD::default());
    let first = env.host.ids();
    env.actions.set_settings(nothing_hidden());
    env.controller.show(PointD::default());
    let second = env.host.ids();

    assert!(!first.contains(&"MoveToRecycleBin".to_owned()));
    assert!(second.contains(&"MoveToRecycleBin".to_owned()));
    assert!(second.contains(&"ZoomSubmenu".to_owned()));
}

// C#: Show_ReadsClickZoomPercentLive_ForTheZoomToLevelTextAndThePresetCheck
#[test]
fn show_reads_click_zoom_percent_live_for_the_preset_check() {
    let mut settings = nothing_hidden();
    settings.click_zoom_percent = 200;
    let env = create(settings.clone());

    env.controller.show(PointD::default());
    let items = env.host.last_items();
    let zoom = find(&items, "ZoomSubmenu");
    assert!(find(&zoom.children, "Zoom.Preset.200").is_checked);
    assert!(!find(&zoom.children, "Zoom.Preset.100").is_checked);

    settings.click_zoom_percent = 70;
    env.actions.set_settings(settings);
    env.controller.show(PointD::default());
    let items = env.host.last_items();
    let zoom = find(&items, "ZoomSubmenu");
    assert!(find(&zoom.children, "Zoom.Preset.70").is_checked);
    assert!(!find(&zoom.children, "Zoom.Preset.200").is_checked);
}

// C#: Show_FeedsTheViewState_IntoTheModel_SetCurrentIsDisabledInFit_CopyNeedsAPhoto
#[test]
fn show_feeds_the_view_state_into_the_model() {
    let env = create(nothing_hidden());
    env.actions.effective_zoom.set(None); // Fit
    env.actions.has_image.set(false);
    env.actions.can_copy_path.set(false);

    env.controller.show(PointD::default());

    let items = env.host.last_items();
    assert!(!find(&find(&items, "ZoomSubmenu").children, "Zoom.SetCurrentAsClickLevel").is_enabled);
    assert!(!items.iter().any(|i| i.id == "CopyFileName" || i.id == "CopyFullPath"));

    env.actions.effective_zoom.set(Some(1.5));
    env.actions.has_image.set(true);
    env.actions.can_copy_path.set(true);
    env.controller.show(PointD::default());

    let items = env.host.last_items();
    assert!(find(&find(&items, "ZoomSubmenu").children, "Zoom.SetCurrentAsClickLevel").is_enabled);
    assert!(items.iter().any(|i| i.id == "CopyFileName"));
}

// C#: Show_HasImageButNoPath_HidesCopy
#[test]
fn show_has_image_but_no_path_hides_copy() {
    let env = create(nothing_hidden());
    env.actions.has_image.set(true);
    env.actions.can_copy_path.set(false);

    env.controller.show(PointD::default());

    assert!(!env.host.ids().contains(&"CopyFileName".to_owned()));
}

// new: the other half of the copy rule (a path without an image also hides Copy)
#[test]
fn show_path_but_no_image_hides_copy() {
    let env = create(nothing_hidden());
    env.actions.has_image.set(false);
    env.actions.can_copy_path.set(true);

    env.controller.show(PointD::default());

    assert!(!env.host.ids().contains(&"CopyFileName".to_owned()));
}

// C#: Show_UndoAndExternalEditorStayEnabled_EvenWhenTheStateSaysNo
#[test]
fn show_undo_and_external_editor_stay_enabled_even_when_the_state_says_no() {
    let env = create(default_settings());
    env.actions.can_undo.set(false);
    env.actions.can_open_in_external_editor.set(false);

    env.controller.show(PointD::default());

    let items = env.host.last_items();
    assert!(find(&items, "Undo").is_enabled);
    assert!(find(&items, "ExternalEditor").is_enabled);
}

// C#: AlsoSetClickLevel_FlipsTheCurrentValue_BothWays
#[test]
fn also_set_click_level_flips_the_current_value_both_ways() {
    let mut settings = nothing_hidden();
    settings.set_zoom_also_sets_click_level = true;
    let env = create(settings.clone());
    env.host.choose("Zoom.AlsoSetClickLevel");

    env.controller.show(PointD::default());
    settings.set_zoom_also_sets_click_level = false;
    env.actions.set_settings(settings);
    env.controller.show(PointD::default());

    assert_eq!(env.actions.log(), strs(&["AlsoSet:false", "AlsoSet:true"]));
}

// C#: SetCurrentAsClickLevel_RoundsAndClampsTheEffectiveZoom
#[test]
fn set_current_as_click_level_rounds_and_clamps_the_effective_zoom() {
    for (zoom, expected) in [(1.0, 100), (1.5, 150), (0.04, 10), (100.0, 800)] {
        let env = create(nothing_hidden());
        env.host.choose("Zoom.SetCurrentAsClickLevel");
        env.actions.effective_zoom.set(Some(zoom));

        env.controller.show(PointD::default());

        assert_eq!(
            env.actions.log(),
            vec![format!("SetClickLevel:{expected}")],
            "zoom {zoom}"
        );
    }
}

// C#: SetCurrentAsClickLevel_InFit_DoesNothing
#[test]
fn set_current_as_click_level_in_fit_does_nothing() {
    let env = create(nothing_hidden());
    env.host.choose("Zoom.SetCurrentAsClickLevel");
    env.actions.effective_zoom.set(None);

    env.controller.show(PointD::default());

    assert!(env.actions.log().is_empty());
}

// C#: SeparatorsAndUnknownIds_RunNothing
#[test]
fn separators_and_unknown_ids_run_nothing() {
    for id in [
        "Separator.2",
        "Separator.Zoom.2",
        "Zoom.Preset.abc",
        "Zoom.Preset.",
        "NoSuchItem",
        "ZoomSubmenu",
        "",
    ] {
        let env = create(default_settings());

        let result = env.controller.execute(id);

        assert!(result.is_ok(), "{id}");
        assert!(env.actions.log().is_empty(), "{id}");
    }
}

// C#: ACommandThatThrows_IsReportedWithItsName_AndNeverEscapes
#[test]
fn a_command_that_fails_is_reported_with_its_name_and_never_escapes() {
    let env = create(nothing_hidden());
    env.host.choose("Zoom.Preset.150");
    env.actions.failing.set(true);

    env.controller.show(PointD::default()); // does not panic

    assert_eq!(
        env.actions.log(),
        strs(&["Preset:150", "failed:Zoom-localized:boom Preset:150"])
    );
}

// C#: ASynchronousFailure_OfAnItemWithoutAnAsyncBody_IsAlsoReported (reported with the EMPTY name)
#[test]
fn a_failure_of_an_item_without_a_zoom_name_is_reported_with_an_empty_name() {
    let env = create(default_settings());
    env.host.choose("CopyFileName");
    env.actions.failing.set(true);

    env.controller.show(PointD::default());

    assert_eq!(env.actions.log(), strs(&["CopyFileName", "failed::boom CopyFileName"]));
}

// C#: OpeningWhileOpen_IsIgnored_AndIsOpenFollowsTheModalMenu
#[test]
fn opening_while_open_is_ignored_and_is_open_follows_the_modal_menu() {
    let env = create(default_settings());
    env.host.set(Behavior::Nested(None));

    assert!(!env.controller.is_open());
    env.controller.show(PointD::default());

    assert_eq!(env.host.open_during.get(), Some(true));
    assert_eq!(env.host.shows_after_nested_call.get(), 1);
    assert_eq!(env.host.show_count.get(), 1);
    assert!(!env.controller.is_open());
    assert!(env.actions.log().is_empty());
}

// C#: IsOpen_IsClearedWhenTheHostThrows
#[test]
fn is_open_is_cleared_when_the_host_fails() {
    let env = create(default_settings());
    env.host.set(Behavior::Fail("no menu"));

    env.controller.show(PointD::default());

    assert!(!env.controller.is_open());
    assert_eq!(env.actions.log(), strs(&["failed:Menu:boom no menu"]));
}

// C#: TheCommand_RunsAfterTheMenuHasClosed
#[test]
fn the_command_runs_after_the_menu_has_closed() {
    let env = create(default_settings());
    env.host.choose("Refresh");

    env.controller.show(PointD::default());

    assert_eq!(*env.actions.open_at_command.borrow(), vec![false]);
    // and the host had returned before the command ran
    assert!(env.host.log_when_returning.borrow().is_empty());
    assert_eq!(env.actions.log(), strs(&["Execute:Refresh"]));
}

// ---------------------------------------------------------------------------------------------------------------------
// Controller: new tests
// ---------------------------------------------------------------------------------------------------------------------

// new
#[test]
fn try_parse_preset_table() {
    assert_eq!(ZOOM_PRESET_PREFIX, "Zoom.Preset.");
    for (id, expected) in [
        ("Zoom.Preset.50", Some(50)),
        ("Zoom.Preset.100", Some(100)),
        ("Zoom.Preset.400", Some(400)),
        ("Zoom.Preset.0", Some(0)),
        ("Zoom.Preset.007", Some(7)),
        ("Zoom.Preset.2147483647", Some(i32::MAX)),
        ("Zoom.Preset.", None),
        ("Zoom.Preset.+5", None),
        ("Zoom.Preset.-5", None),
        ("Zoom.Preset. 5", None),
        ("Zoom.Preset.5 ", None),
        ("Zoom.Preset.1,000", None),
        ("Zoom.Preset.5.0", None),
        ("Zoom.Preset.abc", None),
        ("Zoom.Preset.99999999999", None),
        ("Zoom.Preset.2147483648", None),
        ("zoom.preset.5", None),
        ("Zoom.Preset", None),
        ("Zoom.Custom", None),
        ("XZoom.Preset.5", None),
        ("", None),
        // `NumberStyles.None` accepts ASCII digits only: Arabic-Indic digits are not a preset.
        ("Zoom.Preset.\u{0665}", None),
    ] {
        assert_eq!(try_parse_preset(id), expected, "{id:?}");
    }
}

// new
#[test]
fn a_nested_show_from_inside_the_host_is_ignored() {
    let env = create(default_settings());
    env.host.set(Behavior::Nested(Some("Refresh".to_owned())));

    env.controller.show(PointD::default());

    // The nested call returned at once; the outer menu then chose Refresh exactly once.
    assert_eq!(env.host.show_count.get(), 1);
    assert_eq!(env.actions.log(), strs(&["Execute:Refresh"]));
}

// new
#[test]
fn the_host_can_be_replaced_from_inside_a_show() {
    // No `RefCell` borrow of the host may be held across `host.show` (D-W8B4I-1): a borrow_mut inside would panic.
    let env = create(default_settings());
    env.host.set(Behavior::ReplaceHost(Some("Refresh".to_owned())));

    env.controller.show(PointD::default());

    assert_eq!(env.actions.log(), strs(&["Execute:Refresh"]));
    // The replacement is the host of the next call (it chooses nothing).
    env.controller.show(PointD::default());
    assert_eq!(env.host.show_count.get(), 1);
    assert_eq!(env.actions.log().len(), 1);
}

// new
#[test]
fn set_menu_host_and_menu_host_round_trip() {
    let env = create(default_settings());
    let replacement: Rc<dyn PopupMenuHost> = Rc::new(FakeHost::new());
    assert!(!Rc::ptr_eq(&env.controller.menu_host(), &replacement));

    env.controller.set_menu_host(Rc::clone(&replacement));

    assert!(Rc::ptr_eq(&env.controller.menu_host(), &replacement));
    env.controller.show(PointD::default());
    assert_eq!(env.host.show_count.get(), 0);
}

// new
#[test]
fn is_open_is_cleared_when_the_host_panics() {
    let env = create(default_settings());
    env.host.set(Behavior::Panic);

    let outcome = catch_unwind(AssertUnwindSafe(|| env.controller.show(PointD::default())));

    assert!(outcome.is_err());
    assert!(!env.controller.is_open());
}

// new: `build_model` reads the state again at every call (no cached model)
#[test]
fn build_model_is_built_fresh_at_every_call() {
    let env = create(nothing_hidden());
    let first = env.controller.build_model().expect("build");
    env.actions.has_image.set(false);
    env.actions.can_copy_path.set(false);
    let second = env.controller.build_model().expect("build");
    assert!(first.iter().any(|i| i.id == "CopyFileName"));
    assert!(!second.iter().any(|i| i.id == "CopyFileName"));
    assert!(!env.controller.is_open());
}

// new
#[test]
fn guarded_commands_use_the_zoom_name_and_unguarded_ones_the_id() {
    let cases = [
        // guarded with the localized zoom name
        ("ZoomToLevel", "failed:Zoom-localized:boom ZoomToConfiguredLevel"),
        ("ZoomFitWidth", "failed:Zoom-localized:boom Execute:FitWidth"),
        ("ZoomFitWidth2", "failed:Zoom-localized:boom Execute:FitWidth2"),
        ("ZoomFitHeight", "failed:Zoom-localized:boom Execute:FitHeight"),
        ("Zoom.Custom", "failed:Zoom-localized:boom CustomZoom"),
        ("Zoom.Preset.50", "failed:Zoom-localized:boom Preset:50"),
        // guarded with the empty name
        ("ExternalEditor", "failed::boom ExternalEditor"),
        ("CopyFileName", "failed::boom CopyFileName"),
        ("CopyFullPath", "failed::boom CopyFullPathname"),
        ("Settings", "failed::boom ShowSettings"),
        // unguarded in `execute`; `show` reports them with the id as the name
        ("Undo", "failed:Undo:boom Execute:Undo"),
        ("MoveToRecycleBin", "failed:MoveToRecycleBin:boom Execute:Recycle"),
        ("Fit", "failed:Fit:boom Fit"),
        ("Refresh", "failed:Refresh:boom Execute:Refresh"),
        ("OpenFolder", "failed:OpenFolder:boom Execute:OpenFolder"),
        ("NextFolder", "failed:NextFolder:boom Execute:NextFolder"),
        ("PreviousFolder", "failed:PreviousFolder:boom Execute:PreviousFolder"),
    ];
    for (id, report) in cases {
        let env = create(nothing_hidden());
        env.host.choose(id);
        env.actions.failing.set(true);

        env.controller.show(PointD::default());

        let log = env.actions.log();
        assert_eq!(log.len(), 2, "{id}: {log:?}");
        assert_eq!(log[1], report, "{id}");
    }
}

// new: `execute` itself returns the unguarded `Err` and swallows the guarded ones (they report themselves)
#[test]
fn execute_returns_the_error_of_unguarded_commands_only() {
    let env = create(nothing_hidden());
    env.actions.failing.set(true);

    assert!(env.controller.execute("Undo").is_err());
    assert!(env.controller.execute("Fit").is_err());
    assert!(env.controller.execute("ZoomToLevel").is_ok());
    assert!(env.controller.execute("Zoom.Preset.100").is_ok());
    assert!(env.controller.execute("CopyFileName").is_ok());
    // the guarded ones reported with their names
    let failed: Vec<String> = env
        .actions
        .log()
        .into_iter()
        .filter(|l| l.starts_with("failed:"))
        .collect();
    assert_eq!(
        failed,
        strs(&[
            "failed:Zoom-localized:boom ZoomToConfiguredLevel",
            "failed:Zoom-localized:boom Preset:100",
            "failed::boom CopyFileName",
        ])
    );
}

// C#: ExternalEditorsControllerTests.Controller_DispatchesEditorEntries_AddAndSetDefault (7eaee2a2).
#[test]
fn controller_dispatches_editor_entries_add_and_set_default() {
    let env = create(nothing_hidden());
    for id in [
        "ExternalEditor.Open.dxo",
        "ExternalEditor.Add",
        "ExternalEditor.SetDefault",
        "ExternalEditor",
    ] {
        env.controller.execute(id).unwrap();
    }
    assert_eq!(
        env.actions.log(),
        strs(&["ExternalEditor:dxo", "AddEditor", "SetDefaultEditor", "ExternalEditor"])
    );
}

#[test]
fn editor_suffix_is_forwarded_verbatim_and_prefix_is_ordinal() {
    let env = create(nothing_hidden());
    for id in [
        "ExternalEditor.Open.",
        "ExternalEditor.Open.a.b c",
        "externaleditor.Open.x",
    ] {
        env.controller.execute(id).unwrap();
    }
    assert_eq!(env.actions.log(), strs(&["ExternalEditor:", "ExternalEditor:a.b c"]));
}

#[test]
fn external_editor_dispatch_constants_match_model_builder() {
    use pr_app::menus::external_editors as model;
    use pr_platform_win::shell_menus::controller as shell;
    assert_eq!(shell::EXTERNAL_EDITOR_OPEN_PREFIX, model::EXTERNAL_EDITOR_OPEN_PREFIX);
    assert_eq!(shell::EXTERNAL_EDITOR_ADD_ID, model::EXTERNAL_EDITOR_ADD_ID);
    assert_eq!(
        shell::EXTERNAL_EDITOR_SET_DEFAULT_ID,
        model::EXTERNAL_EDITOR_SET_DEFAULT_ID
    );
}

/// A pre-W9 implementer: deliberately leaves all three new interface members at defaults.
struct LegacyActions(FakeActions);
macro_rules! legacy_query {
    ($name:ident, $result:ty) => {
        fn $name(&self) -> $result {
            self.0.$name()
        }
    };
}
macro_rules! legacy_action {
    ($name:ident) => {
        fn $name(&self) -> ActionResult {
            self.0.$name()
        }
    };
}
impl ContextMenuActions for LegacyActions {
    legacy_query!(settings, Arc<AppSettings>);
    legacy_query!(has_image, bool);
    legacy_query!(can_copy_path, bool);
    legacy_query!(can_undo, bool);
    legacy_query!(can_open_in_external_editor, bool);
    legacy_query!(is_compare_visible, bool);
    legacy_query!(effective_zoom, Option<f64>);
    legacy_query!(zoom_action_name, String);
    legacy_action!(fit);
    legacy_action!(zoom_to_configured_level);
    legacy_action!(custom_zoom);
    legacy_action!(open_in_external_editor);
    legacy_action!(copy_file_name);
    legacy_action!(copy_full_pathname);
    legacy_action!(show_settings);
    fn execute(&self, command: ReviewCommand) -> ActionResult {
        self.0.execute(command)
    }
    fn apply_zoom_preset(&self, percent: i32) -> ActionResult {
        self.0.apply_zoom_preset(percent)
    }
    fn set_zoom_also_sets_click_level(&self, value: bool) {
        self.0.set_zoom_also_sets_click_level(value);
    }
    fn set_click_zoom_percent(&self, percent: i32) {
        self.0.set_click_zoom_percent(percent);
    }
    fn report_action_failed(&self, name: &str, error: &dyn Error) {
        self.0.report_action_failed(name, error);
    }
}

#[test]
fn old_actions_by_id_default_calls_default_editor() {
    let env = create(nothing_hidden());
    let legacy = LegacyActions(FakeActions::new(env.actions.settings().as_ref().clone()));
    legacy.open_in_external_editor_by_id("a.b c").unwrap();
    assert_eq!(legacy.0.log(), strs(&["ExternalEditor"]));
}

#[test]
fn old_actions_add_editor_default_is_noop() {
    let env = create(nothing_hidden());
    let legacy = LegacyActions(FakeActions::new(env.actions.settings().as_ref().clone()));
    legacy.add_external_editor().unwrap();
    assert!(legacy.0.log().is_empty());
}

#[test]
fn old_actions_choose_default_editor_default_is_noop() {
    let env = create(nothing_hidden());
    let legacy = LegacyActions(FakeActions::new(env.actions.settings().as_ref().clone()));
    legacy.choose_default_external_editor().unwrap();
    assert!(legacy.0.log().is_empty());
}

#[test]
fn editor_dispatch_failures_are_reported_through_guarded_run() {
    let env = create(nothing_hidden());
    env.actions.failing.set(true);
    for id in [
        "ExternalEditor.Open.dxo",
        "ExternalEditor.Add",
        "ExternalEditor.SetDefault",
    ] {
        assert!(env.controller.execute(id).is_ok());
    }
    assert_eq!(
        env.actions
            .log()
            .iter()
            .filter(|entry| entry.starts_with("failed:"))
            .count(),
        3
    );
}

// new: the failure is also written to the log, text `Context menu command {name} failed`
#[test]
fn a_failed_command_is_written_to_the_log() {
    let log = Arc::new(RecordingLog::new());
    let env = create_with(nothing_hidden(), Some(Arc::clone(&log) as Arc<dyn Log>));
    env.host.choose("Zoom.Preset.150");
    env.actions.failing.set(true);

    env.controller.show(PointD::default());

    assert!(log.contains(LogLevel::Error, "Context menu command Zoom-localized failed"));
    assert!(log.contains(LogLevel::Error, "boom Preset:150"));
    assert_eq!(log.entries().len(), 1);

    // a menu (host) failure is reported but not written by the controller
    let log2 = Arc::new(RecordingLog::new());
    let env2 = create_with(default_settings(), Some(Arc::clone(&log2) as Arc<dyn Log>));
    env2.host.set(Behavior::Fail("x"));
    env2.controller.show(PointD::default());
    assert!(log2.entries().is_empty());
}

// new: C# `Run(() => SetZoomAlsoSetsClickLevel(!_actions.Settings.SetZoomAlsoSetsClickLevel))` reads the settings at the moment of the command
#[test]
fn also_set_click_level_reads_the_settings_when_the_command_runs() {
    let mut settings = nothing_hidden();
    settings.set_zoom_also_sets_click_level = false;
    let env = create(settings);

    env.controller.execute("Zoom.AlsoSetClickLevel").expect("ok");

    assert_eq!(env.actions.log(), strs(&["AlsoSet:true"]));
}

// new: a successful command writes nothing to the failure report
#[test]
fn a_successful_command_reports_nothing() {
    let env = create(nothing_hidden());
    env.host.choose("Zoom.Preset.100");
    env.controller.show(PointD::default());
    assert_eq!(env.actions.log(), strs(&["Preset:100"]));
}

// ---------------------------------------------------------------------------------------------------------------------
// Message handler: FakeShellWindow and ports of ContextMenuMessageHandlerTests
// ---------------------------------------------------------------------------------------------------------------------

/// C# `FakeShellWindow`: client origin in screen pixels, client size in DIP, DPI scale. `screen_to_client_dip(p) = (p - origin) / convert_scale`.
struct FakeShellWindow {
    dpi_scale: Cell<f64>,
    /// The scale `screen_to_client_dip` divides by (differs from `dpi_scale` only when a test reports a non-positive scale).
    convert_scale: Cell<f64>,
    client_size: Cell<SizeD>,
    origin_pixel: Cell<(f64, f64)>,
}

impl FakeShellWindow {
    fn new(dpi: f64, origin: (f64, f64)) -> Self {
        Self {
            dpi_scale: Cell::new(dpi),
            convert_scale: Cell::new(dpi),
            client_size: Cell::new(SizeD {
                width: 1000.0,
                height: 600.0,
            }),
            origin_pixel: Cell::new(origin),
        }
    }
}

impl ShellWindowApi for FakeShellWindow {
    fn hwnd(&self) -> Hwnd {
        0x1234
    }
    fn dpi_scale(&self) -> f64 {
        self.dpi_scale.get()
    }
    fn client_size_dip(&self) -> SizeD {
        self.client_size.get()
    }
    fn is_active(&self) -> bool {
        true
    }
    fn is_loaded(&self) -> bool {
        true
    }
    fn subscribe(&self, _event: ShellWindowEvent, _handler: WindowEventHandler) -> EventHandlerId {
        0
    }
    fn subscribe_closing(&self, _handler: ClosingHandler) -> EventHandlerId {
        0
    }
    fn unsubscribe(&self, _id: EventHandlerId) {}
    fn add_message_handler(&self, _handler: Rc<dyn WindowMessageHandler>) {}
    fn insert_message_handler_first(&self, _handler: Rc<dyn WindowMessageHandler>) {}
    fn is_destroyed(&self) -> bool {
        false
    }
    fn invalidate(&self) {}
    fn set_title(&self, _title: &str) -> Result<(), HostError> {
        Ok(())
    }
    fn set_cursor(&self, _cursor: pr_platform_win::host::contracts::ShellCursor) {}
    fn capture_pointer(&self) {}
    fn release_pointer(&self) {}
    fn has_pointer_capture(&self) -> bool {
        false
    }
    fn screen_to_client_dip(&self, screen_pixel: PointD) -> PointD {
        let (ox, oy) = self.origin_pixel.get();
        let scale = self.convert_scale.get();
        PointD {
            x: (screen_pixel.x - ox) / scale,
            y: (screen_pixel.y - oy) / scale,
        }
    }
    fn close(&self) {}
}

fn pack(x: i32, y: i32) -> Lparam {
    let low = u64::from(x as i16 as u16);
    let high = u64::from(y as i16 as u16);
    ((high << 16) | low) as Lparam
}

struct HandlerEnv {
    env: Env,
    window: Rc<FakeShellWindow>,
    handler: ContextMenuMessageHandler,
}

fn handler_env(dpi: f64, origin: (f64, f64)) -> HandlerEnv {
    let env = create(default_settings());
    let window = Rc::new(FakeShellWindow::new(dpi, origin));
    let handler =
        ContextMenuMessageHandler::new(Rc::clone(&window) as Rc<dyn ShellWindowApi>, Rc::clone(&env.controller));
    HandlerEnv { env, window, handler }
}

impl HandlerEnv {
    fn dispatch(&self, msg: u32, lparam: Lparam) -> Option<isize> {
        self.handler.try_handle(&WindowMessage {
            hwnd: self.window.hwnd(),
            msg,
            wparam: 0,
            lparam,
        })
    }

    fn last_point(&self) -> PointD {
        self.env.host.last_point.get().expect("the menu was shown")
    }
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!((actual - expected).abs() < 1e-6, "{what}: {actual} vs {expected}");
}

// C#: RightClickInsideTheClient_OpensTheMenu_AtTheScreenPointInDip (theory over the DPI)
#[test]
fn right_click_inside_the_client_opens_the_menu_at_the_screen_point_in_dip() {
    for dpi in [1.0, 1.25, 1.5, 2.0] {
        let h = handler_env(dpi, (100.0, 50.0));

        let result = h.dispatch(WM_CONTEXTMENU, pack(400, 300));

        assert_eq!(result, Some(0), "dpi {dpi}");
        assert_eq!(h.env.host.show_count.get(), 1);
        assert_close(h.last_point().x, 400.0 / dpi, "x");
        assert_close(h.last_point().y, 300.0 / dpi, "y");
    }
}

// C#: RightClickBeyondTheClientEdges_IsLeftToTheSystemMenu
#[test]
fn right_click_beyond_the_client_edges_is_left_to_the_system_menu() {
    let h = handler_env(1.0, (100.0, 50.0));

    // above the client origin (title bar) / left / below the bottom (600 DIP high) / right
    for (x, y) in [(400, 20), (90, 300), (400, 50 + 600), (100 + 1000, 300)] {
        assert_eq!(h.dispatch(WM_CONTEXTMENU, pack(x, y)), None, "({x},{y}) is non-client");
    }

    assert_eq!(h.env.host.show_count.get(), 0);
}

// C#: TheClientEdges_AreInside_TopLeftInclusiveBottomRightExclusive
#[test]
fn the_client_edges_are_inside_top_left_inclusive_bottom_right_exclusive() {
    let h = handler_env(1.0, (100.0, 50.0));

    assert!(h.dispatch(WM_CONTEXTMENU, pack(100, 50)).is_some());
    assert!(h.dispatch(WM_CONTEXTMENU, pack(100 + 999, 50 + 599)).is_some());
    assert!(h.dispatch(WM_CONTEXTMENU, pack(100 + 1000, 50 + 599)).is_none());
    assert!(h.dispatch(WM_CONTEXTMENU, pack(100 + 999, 50 + 600)).is_none());
    // one pixel outside on the top and the left
    assert!(h.dispatch(WM_CONTEXTMENU, pack(99, 50)).is_none());
    assert!(h.dispatch(WM_CONTEXTMENU, pack(100, 49)).is_none());
    assert_eq!(h.env.host.show_count.get(), 2);
}

// C#: ScreenCoordinatesLeftOfThePrimaryMonitor_AreSignedNotWrapped
#[test]
fn screen_coordinates_left_of_the_primary_monitor_are_signed_not_wrapped() {
    let h = handler_env(1.0, (-1500.0, 20.0));

    let result = h.dispatch(WM_CONTEXTMENU, pack(-1200, 200));

    assert_eq!(result, Some(0));
    assert_eq!(h.env.host.last_point.get(), Some(pt(-1200.0, 200.0)));
}

// new: T3, the words of a 64-bit lparam with high bits are masked first
#[test]
fn the_words_of_a_64_bit_lparam_are_masked_first() {
    let h = handler_env(1.0, (0.0, 0.0));
    let with_high_bits = (pack(400, 300) as u64 | (0x7FFF_u64 << 32)) as Lparam;

    assert_eq!(h.dispatch(WM_CONTEXTMENU, with_high_bits), Some(0));
    assert_eq!(h.env.host.last_point.get(), Some(pt(400.0, 300.0)));

    // a sign-extended negative pair (-1200, -5) is not mistaken for the keyboard case
    let h = handler_env(1.0, (-1500.0, -100.0));
    assert_eq!(h.dispatch(WM_CONTEXTMENU, pack(-1200, -5)), Some(0));
    assert_eq!(h.env.host.last_point.get(), Some(pt(-1200.0, -5.0)));
}

// C#: KeyboardInvocation_OpensAtTheCentreOfTheClient
#[test]
fn keyboard_invocation_opens_at_the_centre_of_the_client() {
    for dpi in [1.0, 1.5] {
        let h = handler_env(dpi, (100.0, 50.0));

        let result = h.dispatch(WM_CONTEXTMENU, pack(-1, -1));

        assert_eq!(result, Some(0));
        // client origin = (100, 50) px; centre = origin + 1000x600 DIP / 2 scaled by the DPI; the DIP point divides by the DPI
        assert_close(h.last_point().x, 100.0 / dpi + 500.0, "x");
        assert_close(h.last_point().y, 50.0 / dpi + 300.0, "y");
    }
}

// new: only the pair (-1, -1) is the keyboard case
#[test]
fn only_minus_one_minus_one_is_the_keyboard_case() {
    let h = handler_env(1.0, (100.0, 50.0));
    // (-1, 300) and (400, -1) are ordinary points (outside this client: left of / above the origin), never the centre
    assert_eq!(h.dispatch(WM_CONTEXTMENU, pack(-1, 300)), None);
    assert_eq!(h.dispatch(WM_CONTEXTMENU, pack(400, -1)), None);
    assert_eq!(h.env.host.show_count.get(), 0);
}

// C#: OtherMessages_NeverOpenTheMenu
#[test]
fn other_messages_never_open_the_menu() {
    for message in [
        WM_RBUTTONDOWN,
        WM_RBUTTONUP,
        WM_LBUTTONDOWN,
        WM_KEYDOWN,
        0,
        WM_CONTEXTMENU + 1,
    ] {
        let h = handler_env(1.0, (0.0, 0.0));

        assert_eq!(h.dispatch(message, pack(400, 300)), None, "message {message:#x}");
        assert_eq!(h.env.host.show_count.get(), 0);
    }
}

// C#: ChoosingAnItem_FromTheWindowMessage_RunsItsCommand
#[test]
fn choosing_an_item_from_the_window_message_runs_its_command() {
    let h = handler_env(1.0, (0.0, 0.0));
    h.env.host.choose("Refresh");

    h.dispatch(WM_CONTEXTMENU, pack(10, 10));

    assert_eq!(h.env.actions.log(), strs(&["Execute:Refresh"]));
}

// new
#[test]
fn the_handler_scales_by_the_dpi_and_a_non_positive_scale_counts_as_one() {
    for reported in [0.0, -2.0, f64::NAN] {
        let h = handler_env(2.0, (0.0, 0.0));
        h.window.dpi_scale.set(reported);
        h.window.convert_scale.set(1.0);

        assert_eq!(h.dispatch(WM_CONTEXTMENU, pack(400, 300)), Some(0), "scale {reported}");
        assert_eq!(h.env.host.last_point.get(), Some(pt(400.0, 300.0)), "scale {reported}");

        // keyboard case: centre = origin + size / 2 with a scale of one
        assert_eq!(h.dispatch(WM_CONTEXTMENU, pack(-1, -1)), Some(0));
        assert_eq!(h.env.host.last_point.get(), Some(pt(500.0, 300.0)), "scale {reported}");
    }
}

// new
#[test]
fn the_handler_ignores_messages_other_than_wm_contextmenu() {
    let h = handler_env(1.0, (0.0, 0.0));
    h.env.host.choose("Refresh");
    // the same lparam that would open the menu, with every neighbour message id
    for message in [WM_CONTEXTMENU - 1, WM_CONTEXTMENU + 1, WM_RBUTTONUP] {
        assert_eq!(h.dispatch(message, pack(5, 5)), None);
    }
    assert_eq!(h.env.host.show_count.get(), 0);
    assert!(h.env.actions.log().is_empty());
}

// new: the wm handled by the handler is `Some(0)`, and the menu is open only while the host runs
#[test]
fn the_controller_is_closed_again_after_the_handler_returns() {
    let h = handler_env(1.0, (0.0, 0.0));
    assert_eq!(h.dispatch(WM_CONTEXTMENU, pack(5, 5)), Some(0));
    assert!(!h.env.controller.is_open());
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden `menu-model` through the controller (contract Q2) and the C# G-MENU golden (skipped until synced)
// ---------------------------------------------------------------------------------------------------------------------

fn id_by_name(name: &str) -> Id {
    Id::ALL
        .iter()
        .copied()
        .find(|i| i.name() == name)
        .unwrap_or_else(|| panic!("unknown id {name}"))
}

fn kind_name(kind: MenuItemKind) -> &'static str {
    match kind {
        MenuItemKind::Command => "Command",
        MenuItemKind::Toggle => "Toggle",
        MenuItemKind::Submenu => "Submenu",
        MenuItemKind::Separator => "Separator",
    }
}

fn tree_json(items: &[MenuItemModel]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|i| {
                json!({
                    "id": i.id,
                    "kind": kind_name(i.kind),
                    "text": i.text,
                    "shortcut": i.shortcut_text,
                    "enabled": i.is_enabled,
                    "checked": i.is_checked,
                    "automation": i.automation_name,
                    "children": tree_json(&i.children),
                })
            })
            .collect(),
    )
}

fn render(items: &[MenuItemModel]) -> String {
    items
        .iter()
        .map(|i| {
            if i.kind == MenuItemKind::Separator {
                "|"
            } else {
                i.id.as_str()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn golden_settings(spec: &Value) -> AppSettings {
    let mut settings = default_settings();
    match &spec["hidden"] {
        Value::Array(names) => {
            let hidden: Vec<Id> = names.iter().map(|n| id_by_name(n.as_str().expect("name"))).collect();
            apply_hidden(&mut settings, hidden);
        }
        Value::Null => {
            settings.hidden_context_menu_items = None;
            settings.show_zoom_menu_items = spec["show_zoom"].as_bool().expect("show_zoom");
            settings.show_folder_menu_items = spec["show_folder"].as_bool().expect("show_folder");
            settings.show_recycle_menu_item = spec["show_recycle"].as_bool().expect("show_recycle");
        }
        _ => {} // "default"
    }
    settings.click_zoom_percent = i32::try_from(spec["click_zoom_percent"].as_i64().expect("percent")).expect("i32");
    settings.set_zoom_also_sets_click_level = spec["set_zoom_also_sets_click_level"].as_bool().expect("also");
    if let Some(shortcuts) = spec["shortcuts"].as_object() {
        for (name, value) in shortcuts {
            let slot = settings
                .shortcuts
                .get_mut(name)
                .unwrap_or_else(|| panic!("no shortcut {name}"));
            *slot = value.as_str().expect("shortcut text").to_owned();
        }
    }
    settings
}

fn load_inputs() -> Value {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("menu-model.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).expect("parse inputs")
}

// C# GMenu_EveryGoldenCase_MatchesTheWpfMenu_ThroughTheController, over the W8-02a golden (contract Q2): every `build` case, driven through the
// controller (actions -> `build_model` -> host), must show exactly the oracle's tree.
#[test]
fn golden_menu_model_every_case_matches_through_the_controller() {
    let golden = golden::load("menu-model");
    let inputs = load_inputs();
    let mut replayed = 0;
    let mut names = Vec::new();
    for case in golden.cases.iter().filter(|c| c["kind"] == "build") {
        let name = case["name"].as_str().expect("name");
        let spec = inputs["build"]
            .as_array()
            .expect("build")
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("no input case {name}"));
        let env = create(golden_settings(&spec["settings"]));
        env.actions
            .has_image
            .set(spec["has_image"].as_bool().expect("has_image"));
        env.actions
            .can_copy_path
            .set(spec["can_copy_path"].as_bool().expect("can_copy_path"));
        env.actions.effective_zoom.set(spec["effective_zoom"].as_f64());

        env.controller.show(PointD::default());

        let items = env.host.last_items();
        assert_eq!(
            case["render"].as_str().expect("render"),
            render(&items),
            "{name}: render"
        );
        assert_eq!(case["tree"], tree_json(&items), "{name}: tree");
        assert!(env.actions.log().is_empty(), "{name}: nothing chosen, nothing run");
        names.push(name.to_owned());
        replayed += 1;
    }
    assert_eq!(replayed, inputs["build"].as_array().expect("build").len());
    for expected in ["default", "no_photo", "nothing_hidden", "legacy_flags_all_off"] {
        assert!(
            names.iter().any(|n| n == expected),
            "the golden lacks the case {expected}"
        );
    }
}

// C# GMenu_TheGoldenCoversAllTwelveConfigurations: the 12-configuration golden of the C# repo (`context-menu.v1.json`) is not synced into
// `tests/golden` (lead action: tools/sync-from-csharp.ps1). Until it exists this test says so and passes; when it appears it FAILS so that the
// comparison gets ported (the C# format is `GoldenDocument<GoldenMenuCase>` with `Name`, `SettingsOverridesJson`, `VisibleIdsInOrder`).
#[test]
fn gmenu_csharp_golden_is_skipped_until_it_is_synced() {
    let path = golden::repo_root()
        .join("tests")
        .join("golden")
        .join("context-menu.v1.json");
    if !path.exists() {
        eprintln!(
            "SKIPPED: tests/golden/context-menu.v1.json (C# G-MENU, 12 configurations) is not in the repository; run tools/sync-from-csharp.ps1 (lead action)"
        );
        return;
    }
    panic!(
        "{} now exists: port GMenu_EveryGoldenCase_MatchesTheWpfMenu_ThroughTheController over it (12 configurations)",
        path.display()
    );
}

// new: the set of ids used by the id switch equals the ids the builder produces (no id of the model is silently ignored)
#[test]
fn every_executable_model_id_is_handled_by_the_controller() {
    let env = create(nothing_hidden());
    env.controller.show(PointD::default());
    let mut ids = HashSet::new();
    fn collect(items: &[MenuItemModel], ids: &mut HashSet<String>) {
        for item in items {
            if item.kind != MenuItemKind::Separator && item.kind != MenuItemKind::Submenu {
                ids.insert(item.id.clone());
            }
            collect(&item.children, ids);
        }
    }
    collect(&env.host.last_items(), &mut ids);
    assert!(ids.len() >= 15);
    for id in ids {
        let env = create(nothing_hidden());
        env.controller.execute(&id).expect("ok");
        let log = env.actions.log();
        // `Zoom.SetCurrentAsClickLevel` and `Zoom.AlsoSetClickLevel` write a log line too; every other id runs exactly one action
        assert_eq!(log.len(), 1, "{id} ran no action: {log:?}");
    }
}
