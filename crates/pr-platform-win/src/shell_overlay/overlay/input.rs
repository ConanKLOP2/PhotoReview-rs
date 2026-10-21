//! `ShellOverlay` part 5, press/release, wheel, keyboard, button clicks and the Tools menu. Port of `ShellOverlay.cs` lines 304 to 425 (`PointerDown`,
//! `PointerUp`, `PointerCancel`, `IsPointerDown`, `PressedButton`, `Wheel`, `KeyDown`), 516 to 584 (`ShowToolsMenu`, `ClearKeyboardFocus`) and 629 to 658
//! (`RunGuarded`, `ObserveFailure`), baseline `d92d6aae`. Unit W8-15i (contract `contracts/W8-15i.md`). Pure over the injected seams
//! (`OverlayCommands`, `OverlayMenuHost`); the native Tools menu and the real window messages are other units.
//!
//! Cut note (D-W8B4O-3): the plan cuts the C# file at line 338, inside the doc comment of `PointerUp`, which would leave `PointerDown` in 15h and
//! `PointerUp` in 15i although they share `_pointerDown` and `_pressed`. The press/release state machine is therefore all here.
//!
//! Commands (D-W8B4O-9): `pr_app::overlay::tools_menu::OverlayCommands` is synchronous (contract W8-15e O3). C# `RunGuarded` and `ObserveFailure` (a command
//! that throws or whose task faults is reported with its name, cancellation is not a failure) have no Rust twin here: a command that can fail reports through its
//! own implementation (15j `ViewModelOverlayCommands`: log + status line), so the overlay only dispatches.

use pr_app::input::primitives::PointD;
use pr_app::overlay::element::ElementNode;
use pr_app::overlay::tools_menu;

use super::state::{OverlayButton, ShellOverlay};
use crate::shell_overlay::messages::OverlayInput;
use crate::shell_overlay::native::{VK_RETURN, VK_SPACE, VK_TAB};

/// The toolbar buttons in Tab order (`ToolbarPanel::buttons`).
const TOOLBAR_ORDER: [OverlayButton; 4] = [
    OverlayButton::OpenFolder,
    OverlayButton::Settings,
    OverlayButton::Fit,
    OverlayButton::Tools,
];

impl ShellOverlay {
    /// C# `PointerDown(point)` (lines 308 to 332): `false` after dispose. No hit: `clear_keyboard_focus()` and `false` (the image gets the click). A hit (button
    /// OR panel): `clear_keyboard_focus()`, `pointer_down = true`; when the hit is an ENABLED button: it becomes `pressed` and `is_pressed = true`, `request_render`.
    /// Returns `true` (swallowed), also for a panel or a disabled button.
    pub fn pointer_down(&self, point: PointD) -> bool {
        if self.is_disposed() {
            return false;
        }
        if !self.hit_test(point) {
            self.clear_keyboard_focus();
            return false;
        }
        self.clear_keyboard_focus();
        let hit_button = self.hit_button(point);
        let pressed = {
            let mut state = self.state.borrow_mut();
            state.pointer_down = true;
            match hit_button {
                Some(button) => match state.button_mut(button) {
                    Some(element) if element.is_enabled => {
                        element.is_pressed = true;
                        state.pressed = Some(button);
                        true
                    }
                    _ => false,
                },
                None => false,
            }
        };
        if pressed {
            (self.request_render)();
        }
        true
    }

    /// C# `PointerUp(point)` (lines 337 to 357): `false` after dispose or when no press is held. Else `pointer_down = false`; a pressed button is released
    /// (`is_pressed = false`, `request_render`) and, when the topmost hit at `point` is still THAT button, its click is dispatched (`dispatch_click`); returns `true`
    /// (the release belongs to the overlay, also after a panel press or a release outside the button).
    pub fn pointer_up(&self, point: PointD) -> bool {
        let pressed = {
            let mut state = self.state.borrow_mut();
            if state.disposed || !state.pointer_down {
                return false;
            }
            state.pointer_down = false;
            let pressed = state.pressed.take();
            if let Some(button) = pressed
                && let Some(element) = state.button_mut(button)
            {
                element.is_pressed = false;
            }
            pressed
        };
        if let Some(button) = pressed {
            (self.request_render)();
            if self.hit_button(point) == Some(button) {
                self.click_button(button);
            }
        }
        true
    }

    /// C# `PointerCancel()` (lines 360 to 369): capture was lost. Clears `pointer_down` and the pressed button (`is_pressed = false`, `request_render`); no click.
    /// Not guarded by `disposed` (C# did not).
    pub fn pointer_cancel(&self) {
        let had_pressed = {
            let mut state = self.state.borrow_mut();
            state.pointer_down = false;
            match state.pressed.take() {
                Some(button) => {
                    if let Some(element) = state.button_mut(button) {
                        element.is_pressed = false;
                    }
                    true
                }
                None => false,
            }
        };
        if had_pressed {
            (self.request_render)();
        }
    }

    /// C# `IsPointerDown`: the left press is held by the overlay (button or panel).
    pub fn is_pointer_down(&self) -> bool {
        self.state.borrow().pointer_down
    }

    /// C# `PressedButton`: the pressed button; `None` when the press landed on a panel.
    pub fn pressed_button(&self) -> Option<OverlayButton> {
        self.state.borrow().pressed
    }

    /// C# `Wheel(point)` (lines 381 to 385): `note_activity()` (even when the wheel is not over the overlay), then `hit_test(point)`: true = an overlay panel takes
    /// the wheel (it does not reach the image).
    pub fn wheel(&self, point: PointD) -> bool {
        self.note_activity();
        self.hit_test(point)
    }

    /// C# `KeyDown(virtualKey, shift, isAutoRepeat)` (lines 393 to 425): `false` after dispose. `note_info_activity()` first (every key is activity). `VK_TAB`:
    /// auto-repeat returns `toolbar.has_keyboard_focus()` without moving; otherwise `toolbar.move_focus(!shift)`, `apply_toolbar_visibility()`, `request_render`,
    /// `true`. `VK_SPACE` or `VK_RETURN` with a focused toolbar button: click it unless auto-repeat, `true`. Anything else `false` (the image handles it).
    pub fn key_down(&self, virtual_key: i32, shift: bool, is_auto_repeat: bool) -> bool {
        if self.is_disposed() {
            return false;
        }
        self.note_info_activity();
        if virtual_key == VK_TAB {
            if is_auto_repeat {
                return self.has_keyboard_focus();
            }
            {
                let mut state = self.state.borrow_mut();
                if let Some(toolbar) = state.toolbar_mut() {
                    // The result is ignored: the focus may leave the toolbar, and Tab is swallowed all the same (C# quirk, contract Q4).
                    toolbar.move_focus(!shift);
                }
            }
            self.apply_toolbar_visibility();
            (self.request_render)();
            return true;
        }
        if virtual_key == VK_SPACE || virtual_key == VK_RETURN {
            let focused = {
                let state = self.state.borrow();
                state
                    .toolbar()
                    .and_then(|toolbar| toolbar.focused_index())
                    .and_then(|index| TOOLBAR_ORDER.get(index).copied())
            };
            if let Some(button) = focused {
                if !is_auto_repeat {
                    self.click_button(button);
                }
                return true;
            }
        }
        false
    }

    /// C# `ButtonElement.Click()` followed by the subscription of the constructor: the element gate (`IsEnabled && IsVisible && Opacity > 0`) is
    /// `ButtonElement::click`; the click is dispatched after the state borrow is released.
    fn click_button(&self, button: OverlayButton) {
        let mut clicked = false;
        {
            let mut state = self.state.borrow_mut();
            if let Some(element) = state.button_mut(button) {
                element.click(&mut |_id| clicked = true);
            }
        }
        if clicked {
            self.dispatch_click(button);
        }
    }

    /// The five `Clicked` subscriptions of the C# constructor, by id: `OpenFolder` -> `commands.open_folder()`, `Settings` -> `show_settings()`,
    /// `Fit` -> `fit()`, `Tools` -> `show_tools_menu()`, `ViewList` -> `show_skipped_files()`. A command is called with NO `RefCell` borrow of the state held; the
    /// commands cell is taken with `try_borrow_mut` and a re-entrant call (a command that triggers another click) is dropped.
    pub(super) fn dispatch_click(&self, button: OverlayButton) {
        if button == OverlayButton::Tools {
            self.show_tools_menu();
            return;
        }
        let Ok(mut commands) = self.commands.try_borrow_mut() else {
            return;
        };
        match button {
            OverlayButton::OpenFolder => commands.open_folder(),
            OverlayButton::Settings => commands.show_settings(),
            OverlayButton::Fit => commands.fit(),
            OverlayButton::ViewList => commands.show_skipped_files(),
            OverlayButton::Tools => {}
        }
    }

    /// C# `ShowToolsMenu()` (lines 518 to 572). Nothing when the menu is already open or there is no menu host (a re-entrant click while open is ignored).
    /// Anchor = `client_origin_on_screen_dip()` (default `(0, 0)`) plus `(tools.x, tools.y + tools.height)` of the Tools button. Then: `tools_open = true`, the
    /// button `is_checked = true`, `apply_toolbar_visibility()` (the toolbar must not hide); `menus.show(tools_menu::build(has_images), anchor)` with NO borrow held
    /// (the call is modal and messages are processed inside it); whatever happens, afterwards `tools_open = false`, `is_checked = false`,
    /// `apply_toolbar_visibility()`, `request_render`. An `Err` of `show` goes to `on_menu_failed(&*error)` (else `on_command_failed("Tools menu", &error.to_string())`) and counts as
    /// "nothing chosen". A chosen id (not after dispose) runs `tools_menu::execute(id, commands, has_images)`.
    pub(super) fn show_tools_menu(&self) {
        let Some(menus) = self.menus.clone() else {
            return;
        };
        let (bar, has_images) = {
            let mut state = self.state.borrow_mut();
            if state.tools_open {
                return;
            }
            let Some(bar) = state.toolbar().map(|toolbar| toolbar.tools().base().bounds()) else {
                return;
            };
            state.tools_open = true;
            if let Some(tools) = state.button_mut(OverlayButton::Tools) {
                tools.is_checked = true;
            }
            (bar, state.model.has_images)
        };
        let origin = self
            .client_origin_on_screen_dip
            .as_ref()
            .map_or_else(PointD::default, |origin| origin());
        let anchor = PointD {
            x: origin.x + bar.x,
            y: origin.y + bar.y + bar.height,
        };
        self.apply_toolbar_visibility();

        let chosen = match menus.show(&tools_menu::build(has_images), anchor) {
            Ok(chosen) => chosen,
            Err(error) => {
                if let Some(on_menu_failed) = &self.on_menu_failed {
                    on_menu_failed(&*error);
                } else if let Some(on_command_failed) = &self.on_command_failed {
                    on_command_failed("Tools menu", &error.to_string());
                }
                None
            }
        };

        {
            let mut state = self.state.borrow_mut();
            state.tools_open = false;
            if let Some(tools) = state.button_mut(OverlayButton::Tools) {
                tools.is_checked = false;
            }
        }
        self.apply_toolbar_visibility();
        (self.request_render)();

        if self.is_disposed() {
            return;
        }
        if let Some(id) = chosen
            && let Ok(mut commands) = self.commands.try_borrow_mut()
        {
            tools_menu::execute(Some(&id), &mut **commands, has_images);
        }
    }

    /// C# `ClearKeyboardFocus()` (lines 576 to 584): when the toolbar holds the keyboard: `clear_focus()`, `apply_toolbar_visibility()`, `request_render`.
    pub(super) fn clear_keyboard_focus(&self) {
        let cleared = {
            let mut state = self.state.borrow_mut();
            match state.toolbar_mut() {
                Some(toolbar) if toolbar.has_keyboard_focus() => {
                    toolbar.clear_focus();
                    true
                }
                _ => false,
            }
        };
        if cleared {
            self.apply_toolbar_visibility();
            (self.request_render)();
        }
    }
}

/// The message handler's view of the overlay (W8-15f `OverlayInput`): each method forwards to the inherent method of the same name.
impl OverlayInput for ShellOverlay {
    fn pointer_moved(&self, point: PointD) {
        ShellOverlay::pointer_moved(self, point);
    }

    fn pointer_left(&self) {
        ShellOverlay::pointer_left(self);
    }

    fn pointer_down(&self, point: PointD) -> bool {
        ShellOverlay::pointer_down(self, point)
    }

    fn pointer_up(&self, point: PointD) -> bool {
        ShellOverlay::pointer_up(self, point)
    }

    fn pointer_cancel(&self) {
        ShellOverlay::pointer_cancel(self);
    }

    fn is_pointer_down(&self) -> bool {
        ShellOverlay::is_pointer_down(self)
    }

    fn wheel(&self, point: PointD) -> bool {
        ShellOverlay::wheel(self, point)
    }

    fn key_down(&self, virtual_key: i32, shift: bool, is_auto_repeat: bool) -> bool {
        ShellOverlay::key_down(self, virtual_key, shift, is_auto_repeat)
    }
}
