//! Port of `SettingsDialog.cs` lines 780 to 793 (`OnScroll`) and 1281 to 1410 (`LayoutPage`, `UpdateScrollBar`, `SetScroll`, `ScrollByBar`, `ScrollByWheel`, `EnsureVisible`),
//! baseline `d92d6aae`. Unit W8-19m. Contract: `contracts/W8-19m.md`.
//!
//! The page area scrolls vertically: each page is laid out at `y = -scroll` by its layout elements, the container clips; the scroll bar mirrors the offset. The offsets,
//! targets and ranges are pure functions (hand-checked against the C# arithmetic and, for the clamp, the golden `settings-dialog-layout`).
//!
//! `unsafe` here: none of its own (`set_scroll_info` / `get_scroll_info` are the safe wrappers of `win32::settings_native`).

use super::frame::SettingsContent;
use crate::dialogs::native::dialog::NativeDialog;

/// C# `SbLineUp` .. `SbBottom` as used by `ScrollByBar` (mirrors `win32::settings_native::SB_*`).
pub const SB_LINEUP: i32 = 0;
/// C# `SbLineDown`.
pub const SB_LINEDOWN: i32 = 1;
/// C# `SbPageUp`.
pub const SB_PAGEUP: i32 = 2;
/// C# `SbPageDown`.
pub const SB_PAGEDOWN: i32 = 3;
/// C# `SbThumbPosition`.
pub const SB_THUMBPOSITION: i32 = 4;
/// C# `SbThumbTrack`.
pub const SB_THUMBTRACK: i32 = 5;
/// C# `SbTop`.
pub const SB_TOP: i32 = 6;
/// C# `SbBottom`.
pub const SB_BOTTOM: i32 = 7;

/// C# `SetScroll` clamp: `clamp(value, 0, max(0, content_height - viewport_height))`, written as `max` then `min` so it cannot panic. Pure.
pub fn clamp_scroll(value: i32, content_height: i32, viewport_height: i32) -> i32 {
    value.max(0).min(content_height.saturating_sub(viewport_height).max(0))
}

/// C# `ScrollByBar` step: `line = max(1, line_height * 2)`. Pure.
pub fn bar_line(line_height: i32) -> i32 {
    line_height.saturating_mul(2).max(1)
}

/// C# `ScrollByBar` target before the clamp: `SB_LINEUP` -> `current - line`; `SB_LINEDOWN` -> `current + line`; `SB_PAGEUP` -> `current - max(line, viewport_height - line)`;
/// `SB_PAGEDOWN` -> `current + max(line, viewport_height - line)`; `SB_TOP` -> `0`; `SB_BOTTOM` -> `i32::MAX`; `SB_THUMBTRACK` / `SB_THUMBPOSITION` -> `track_pos`; any other
/// code (e.g. `SB_ENDSCROLL`) -> `current`. Saturating arithmetic. Pure.
pub fn bar_target(code: i32, current: i32, line: i32, viewport_height: i32, track_pos: i32) -> i32 {
    let page = line.max(viewport_height.saturating_sub(line));
    match code {
        SB_LINEUP => current.saturating_sub(line),
        SB_LINEDOWN => current.saturating_add(line),
        SB_PAGEUP => current.saturating_sub(page),
        SB_PAGEDOWN => current.saturating_add(page),
        SB_TOP => 0,
        SB_BOTTOM => i32::MAX,
        SB_THUMBTRACK | SB_THUMBPOSITION => track_pos,
        _ => current,
    }
}

/// C# `ScrollByWheel` target before the clamp: `current - (int)Math.Round(delta / 120.0 * 3 * line_height)` (one notch = three lines; `Math.Round` rounds half to EVEN;
/// the `(int)` cast saturates). Pure.
pub fn wheel_target(current: i32, delta: i32, line_height: i32) -> i32 {
    current.saturating_sub((f64::from(delta) / 120.0 * 3.0 * f64::from(line_height)).round_ties_even() as i32)
}

/// The `SCROLLINFO` values of `UpdateScrollBar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollBarInfo {
    /// `0`.
    pub min: i32,
    /// `content_height - 1`.
    pub max: i32,
    /// `max(1, viewport_height)`.
    pub page: u32,
    /// The scroll offset.
    pub pos: i32,
}

/// C# `UpdateScrollBar` decision: `None` when `content_height <= viewport_height` (the bar is hidden); else the range. Pure.
pub fn scroll_bar_info(content_height: i32, viewport_height: i32, scroll: i32) -> Option<ScrollBarInfo> {
    (content_height > viewport_height).then(|| ScrollBarInfo {
        min: 0,
        max: content_height.saturating_sub(1),
        page: viewport_height.max(1) as u32,
        pos: scroll,
    })
}

/// C# `EnsureVisible` target before the clamp: `top < 0` -> `Some(scroll + top - dy4)`; else `bottom > viewport_height` -> `Some(scroll + (bottom - viewport_height) + dy4)`;
/// else `None` (already visible). `top` / `bottom` are the control's edges in the page container's client coordinates; `dy4 = dy(4)`. Pure.
pub fn ensure_visible_target(top: i32, bottom: i32, scroll: i32, viewport_height: i32, dy4: i32) -> Option<i32> {
    if top < 0 {
        Some(scroll.saturating_add(top).saturating_sub(dy4))
    } else if bottom > viewport_height {
        Some(
            scroll
                .saturating_add(bottom.saturating_sub(viewport_height))
                .saturating_add(dy4),
        )
    } else {
        None
    }
}

impl SettingsContent {
    /// C# `LayoutPage(index)` (lines 1281 to 1311): `attached == false` or `viewport_width <= 0` -> nothing. Else `surface = PageSurface::new(self, dialog)`; `width = max(dx(100),
    /// viewport_width - dx(2))`; up to TWO passes: `surface.origin = -scroll[index]`; `cursor = 0`; every root element of the page's builder `cursor += element.place(&mut surface, 0,
    /// origin + cursor, width)`; `content_height[index] = cursor`; `max = max(0, cursor - viewport_height)`; when `scroll[index] <= max` stop, else `scroll[index] = max` and
    /// run the second pass (after the second pass an offset that is still above `max` is clamped to `max` and the loop ends: no third pass, as the C# `for (pass < 2)`). Then `update_scroll_bar(index)`. The builder is borrowed (shared) for the loop only; nothing inside calls a page.
    pub(super) fn layout_page(&self, dialog: &NativeDialog, index: i32) {
        if !self.view.attached.get() || self.view.viewport_width.get() <= 0 {
            return;
        }
        let Ok(i) = usize::try_from(index) else {
            return;
        };
        let Some(builder) = self.builders.get(i) else {
            return;
        };
        let Ok(builder) = builder.try_borrow() else {
            return;
        };
        let width = dialog
            .dx(100)
            .max(self.view.viewport_width.get().saturating_sub(dialog.dx(2)));
        let mut surface = super::surface::PageSurface::new(self, dialog);
        for _ in 0..2 {
            let Some(scroll) = self.view.pages.borrow().get(i).map(|p| p.scroll) else {
                return;
            };
            surface.origin = scroll.saturating_neg();
            let mut cursor = 0_i32;
            for element in builder.elements() {
                cursor = cursor.saturating_add(element.place(
                    &mut surface,
                    0,
                    scroll.saturating_neg().saturating_add(cursor),
                    width,
                ));
            }
            let max = cursor.saturating_sub(self.view.viewport_height.get()).max(0);
            let mut views = self.view.pages.borrow_mut();
            let Some(page) = views.get_mut(i) else {
                return;
            };
            page.content_height = cursor;
            if page.scroll <= max {
                break;
            }
            page.scroll = max;
        }
        drop(builder);
        self.update_scroll_bar(dialog, index);
    }

    /// C# `UpdateScrollBar(index)`: `scroll_bar_info(content_height[index], viewport_height, scroll[index])`; `None` -> `show_window(bar, SW_HIDE)`; `Some(info)` ->
    /// `show_window(bar, SW_SHOWNA)` and `set_scroll_info(bar, SB_CTL, &SCROLLINFO { cbSize, fMask = SIF_RANGE | SIF_PAGE | SIF_POS, .. }, true)`.
    pub(super) fn update_scroll_bar(&self, dialog: &NativeDialog, index: i32) {
        use crate::win32::settings_native as n;
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let info = self
            .view
            .pages
            .borrow()
            .get(index)
            .and_then(|p| scroll_bar_info(p.content_height, self.view.viewport_height.get(), p.scroll));
        let bar = dialog.item(super::frame::SCROLL_ID);
        crate::win32::api::user32::show_window(bar, if info.is_some() { 8 } else { 0 });
        if let Some(info) = info {
            let native = crate::win32::structs::ScrollInfo {
                fMask: windows::Win32::UI::WindowsAndMessaging::SCROLLINFO_MASK(
                    n::SIF_RANGE | n::SIF_PAGE | n::SIF_POS,
                ),
                nMin: info.min,
                nMax: info.max,
                nPage: info.page,
                nPos: info.pos,
                ..Default::default()
            };
            n::set_scroll_info(bar, n::SB_CTL, &native, true);
        }
    }

    /// C# `SetScroll(value)`: `active < 0` -> nothing; `clamped = clamp_scroll(value, content_height[active], viewport_height)`; equal to the current offset -> nothing; else store
    /// and `layout_page(active)`.
    pub(super) fn set_scroll(&self, dialog: &NativeDialog, value: i32) {
        let active = self.view.active.get();
        let Ok(i) = usize::try_from(active) else {
            return;
        };
        {
            let mut views = self.view.pages.borrow_mut();
            let Some(page) = views.get_mut(i) else {
                return;
            };
            let clamped = clamp_scroll(value, page.content_height, self.view.viewport_height.get());
            if clamped == page.scroll {
                return;
            }
            page.scroll = clamped;
        }
        self.layout_page(dialog, active);
    }

    /// C# `ScrollByBar(code)`: `active < 0` -> nothing. `get_scroll_info(item(SCROLL_ID), SB_CTL, mask = SIF_POS | SIF_TRACKPOS)` for `nTrackPos` (`0` when it fails), `line =
    /// bar_line(line_height)`, `set_scroll(bar_target(code, scroll[active], line, viewport_height, track_pos))`.
    pub(super) fn scroll_by_bar(&self, dialog: &NativeDialog, code: i32) {
        use crate::win32::settings_native as n;
        let Ok(i) = usize::try_from(self.view.active.get()) else {
            return;
        };
        let Some(current) = self.view.pages.borrow().get(i).map(|p| p.scroll) else {
            return;
        };
        let mut info = crate::win32::structs::ScrollInfo {
            fMask: windows::Win32::UI::WindowsAndMessaging::SCROLLINFO_MASK(n::SIF_POS | n::SIF_TRACKPOS),
            ..Default::default()
        };
        n::get_scroll_info(dialog.item(super::frame::SCROLL_ID), n::SB_CTL, &mut info);
        self.set_scroll(
            dialog,
            bar_target(
                code,
                current,
                bar_line(dialog.line_height()),
                self.view.viewport_height.get(),
                info.nTrackPos,
            ),
        );
    }

    /// C# `ScrollByWheel(delta)`: `set_scroll(wheel_target(scroll[max(0, active)], delta, line_height))`.
    pub(super) fn scroll_by_wheel(&self, dialog: &NativeDialog, delta: i32) {
        let i = self.view.active.get().max(0) as usize;
        let Some(current) = self.view.pages.borrow().get(i).map(|p| p.scroll) else {
            return;
        };
        self.set_scroll(dialog, wheel_target(current, delta, dialog.line_height()));
    }

    /// C# `EnsureVisible(id)`: `control = item(id)`; `control == 0`, no showing page or `nav::page_of(id) != active` -> nothing. Else the control's `get_window_rect` is mapped
    /// into the page container (`screen_to_client(item(viewport_id(active)), top-left)`; a `get_window_rect` `Err` -> nothing), `top`, `bottom = top + height`, `ensure_visible_target(.., dy(4))`; `Some(v)` -> `set_scroll(v)`.
    pub fn ensure_visible(&self, dialog: &NativeDialog, id: i32) {
        let control = dialog.item(id);
        let active = self.view.active.get();
        if control == 0 || active < 0 || super::nav::page_of(id) != active {
            return;
        }
        let Ok(rect) = crate::win32::api::user32::get_window_rect(control) else {
            return;
        };
        let mut point = crate::win32::structs::Point {
            x: rect.left,
            y: rect.top,
        };
        crate::win32::dialog_native::user32::screen_to_client(
            dialog.item(super::frame::viewport_id(active)),
            &mut point,
        );
        let Some(scroll) = self.view.pages.borrow().get(active as usize).map(|p| p.scroll) else {
            return;
        };
        if let Some(target) = ensure_visible_target(
            point.y,
            point.y.saturating_add(rect.bottom.saturating_sub(rect.top)),
            scroll,
            self.view.viewport_height.get(),
            dialog.dy(4),
        ) {
            self.set_scroll(dialog, target);
        }
    }

    /// C# `OnScroll(id, code)` (lines 780 to 793): `id == SCROLL_ID` -> `scroll_by_bar(code)`; else `page = nav::page_of(id)` in range -> `pages[page].on_scroll(id)` (skipped when
    /// borrowed); anything else nothing.
    pub(super) fn handle_scroll(&self, dialog: &NativeDialog, id: i32, code: i32) {
        if id == super::frame::SCROLL_ID {
            self.scroll_by_bar(dialog, code);
        } else if let Some(page) = self.page(super::nav::page_of(id)) {
            if let Ok(mut page) = page.try_borrow_mut() {
                page.on_scroll(id);
            }
        }
    }
}
