//! D-10 mutant killers for `dialogs/settings/dialog/surface.rs` on a real Settings window (the real `PageSurface` over a real dialog).
#![cfg(windows)]
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;
use pr_platform_win::dialogs::native::chrome::DialogColors;
use pr_platform_win::dialogs::native::controls::basic::ComboBoxControl;
use pr_platform_win::dialogs::native::dialog::core::DialogContent;
use pr_platform_win::dialogs::native::template::DialogControlClass;
use pr_platform_win::dialogs::settings::builder::rows::SettingsLayoutSurface;
use pr_platform_win::dialogs::settings::dialog::{SettingsDialog, surface::PageSurface};
use pr_platform_win::win32::structs::{Hwnd, Point};
use pr_platform_win::win32::{api, comctl, dialog_native};
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());
const WS_VISIBLE: isize = 0x1000_0000;
const GWL_STYLE: i32 = -16;

fn modal(page: i32, body: fn(&SettingsDialog, &mut PageSurface<'_>)) {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(move || {
        let _styles = desktop::activate_visual_styles();
        let dialog = fixture::dialog();
        let ready = |d: &SettingsDialog| {
            d.activate_page(page, false, true);
            let mut surface = PageSurface::new(d.content(), d.native());
            body(d, &mut surface);
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
    });
}

fn items_of(d: &SettingsDialog, class: DialogControlClass) -> Vec<i32> {
    d.page_items(d.active_page_index() as usize)
        .iter()
        .filter(|i| i.class == class && d.native().item(i.id) != 0)
        .map(|i| i.id)
        .collect()
}

fn style(control: Hwnd) -> isize {
    api::user32::get_window_long_ptr(control, GWL_STYLE)
}

fn children(parent: Hwnd) -> Vec<Hwnd> {
    let mut out = Vec::new();
    let mut c = dialog_native::user32::get_window(parent, 5);
    while c != 0 {
        out.push(c);
        c = dialog_native::user32::get_window(c, 2);
    }
    out
}

fn rect_in_parent(control: Hwnd) -> (i32, i32, i32, i32) {
    let r = api::user32::get_window_rect(control).unwrap();
    let mut p = Point { x: r.left, y: r.top };
    assert!(dialog_native::user32::screen_to_client(
        comctl::user32::get_parent(control),
        &mut p
    ));
    (p.x, p.y, r.right - r.left, r.bottom - r.top)
}

// surface.rs:61:9 static_text_color -> 0 / -> 1: a note gets the muted colour, an undeclared id the text colour (both are neither 0 nor 1).
#[test]
#[ignore = "native: needs desktop"]
fn static_text_color_returns_the_declared_kind_colour() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let keys = [
            "General",
            "Display",
            "Mouse",
            "Performance",
            "Shortcuts",
            "Files",
            "Diagnostics",
        ];
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut args = fixture::args();
        args.options.pages = Some(
            keys.iter()
                .map(|&key| {
                    Box::new(fixture::ProbePage {
                        key,
                        events: events.clone(),
                    }) as Box<dyn pr_platform_win::dialogs::settings::page_view::SettingsPageView>
                })
                .collect(),
        );
        let dialog = SettingsDialog::new(args).unwrap();
        let ready = |d: &SettingsDialog| {
            d.activate_page(0, false, true);
            let note = items_of(d, DialogControlClass::Static)[0];
            let content = d.content().as_ref();
            let muted = DialogColors::from_key("Dark.TextMuted").unwrap_or_else(DialogColors::text_secondary);
            assert_eq!(DialogContent::static_text_color(content, d.native(), note), muted);
            assert_eq!(
                DialogContent::static_text_color(content, d.native(), 0x7FFF0),
                DialogColors::text()
            );
            assert_ne!(muted, DialogColors::text());
            assert!(muted > 1 && DialogColors::text() > 1);
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
    });
}

// surface.rs:85:9 Debug fmt -> Ok(Default::default()) writes nothing.
#[test]
#[ignore = "native: needs desktop"]
fn page_surface_debug_names_the_type() {
    modal(1, |_, s| assert_eq!(format!("{s:?}"), "PageSurface { .. }"));
}

// surface.rs:103:9 dx, 108:9 dy, 113:9 line_height, 118:9 field_height -> 0 / 1 / -1.
#[test]
#[ignore = "native: needs desktop"]
fn metric_queries_forward_the_dialog_values() {
    modal(1, |d, s| {
        let n = d.native();
        for u in [4, 7, 100, 250] {
            assert_eq!(s.dx(u), n.dx(u));
            assert_eq!(s.dy(u), n.dy(u));
            assert!(s.dx(u) > 1 && s.dy(u) > 1, "dialog units {u} must scale to > 1 px");
        }
        assert!(s.dx(100) > s.dx(7) && s.dy(250) > s.dy(100));
        assert_eq!(s.line_height(), n.line_height());
        assert!(s.line_height() > 1);
        // Independent formula: dy(8) is one line (the C# dialog-unit definition) and field_height = max(dy(14), line + dy(6)).
        assert_eq!(s.dy(8), s.line_height());
        let expected = s.dy(14).max(s.line_height() + s.dy(6));
        assert_eq!(s.field_height(), expected);
        assert!(s.field_height() > 1);
    });
}

// surface.rs:123:9 text_of -> String::new() / "xyzzy".
#[test]
#[ignore = "native: needs desktop"]
fn text_of_reads_the_live_control_text() {
    modal(1, |d, s| {
        let statics = items_of(d, DialogControlClass::Static);
        assert!(statics.len() >= 2);
        d.set_control_text(statics[0], "alpha text");
        d.set_control_text(statics[1], "beta");
        assert_eq!(s.text_of(statics[0]), "alpha text");
        assert_eq!(s.text_of(statics[1]), "beta");
    });
}

// surface.rs:128:9 text_height, 133:9 text_width -> 0 / 1 / -1.
#[test]
#[ignore = "native: needs desktop"]
fn text_measures_forward_the_dialog_measures() {
    modal(1, |d, s| {
        let id = items_of(d, DialogControlClass::Static)[0];
        d.set_control_text(
            id,
            "A fairly long sentence that has to wrap when the width is very small indeed",
        );
        let narrow = s.text_height(id, d.native().dx(40));
        let wide = s.text_height(id, d.native().dx(4000));
        assert_eq!(narrow, d.native().measure_item_height(id, d.native().dx(40)));
        assert_eq!(wide, d.native().measure_item_height(id, d.native().dx(4000)));
        assert!(wide >= s.line_height() && s.line_height() > 1);
        assert!(
            narrow > wide,
            "narrow {narrow} must wrap to more lines than wide {wide}"
        );
        let long = s.text_width("WWWWWWWWWWWWWWWWWWWW");
        let short = s.text_width("W");
        assert_eq!(long, d.native().measure_text_width("WWWWWWWWWWWWWWWWWWWW", 0));
        assert!(short > 1 && long > short * 10, "long {long} short {short}");
    });
}

// surface.rs:138:9 is_hidden -> false (the `true` variant is already caught); also keeps place() from showing a hidden id.
#[test]
#[ignore = "native: needs desktop"]
fn is_hidden_follows_the_page_visibility_and_place_leaves_hidden_controls_hidden() {
    modal(1, |d, s| {
        let id = items_of(d, DialogControlClass::Static)[0];
        let control = d.native().item(id);
        assert!(!s.is_hidden(id));
        d.set_control_visible(id, false);
        assert!(s.is_hidden(id));
        assert_eq!(style(control) & WS_VISIBLE, 0);
        s.place(id, 5, 6, 70, 20); // hidden id -> hide(), no move, no show
        assert_eq!(style(control) & WS_VISIBLE, 0);
        d.set_control_visible(id, true);
        assert!(!s.is_hidden(id));
        assert_ne!(style(control) & WS_VISIBLE, 0);
    });
}

// surface.rs:144:9 combo_drop_height -> 0 / 1 / -1 and 145:18 `==` -> `!=` (absent id must give 0, a real combo its drop height).
#[test]
#[ignore = "native: needs desktop"]
fn combo_drop_height_uses_the_live_combo_count_and_item_height() {
    modal(1, |d, s| {
        let id = items_of(d, DialogControlClass::ComboBox)[0];
        let combo = d.native().item(id);
        assert_eq!(s.combo_drop_height(0x7FFF0), 0);
        // SAFETY: CB_GETITEMHEIGHT / CB_GETCOUNT take integer arguments only.
        let h = unsafe { dialog_native::user32::send_message(combo, 0x0154, 0, 0) } as i32;
        assert!(h > 1);
        for (count, rows) in [(3usize, 3), (20, 12)] {
            let items: Vec<String> = (0..count).map(|i| format!("item {i}")).collect();
            d.set_combo_items(id, &items, 0);
            // SAFETY: as above.
            let live = unsafe { dialog_native::user32::send_message(combo, 0x0146, 0, 0) } as usize;
            assert_eq!(live, count);
            assert_eq!(ComboBoxControl::count(combo), count as i32);
            assert_eq!(s.combo_drop_height(id), rows * h + s.dy(4));
        }
    });
}

// surface.rs:163:9 place -> (), 164:20 `==` -> `!=`, 182:29 / 182:20 `|` -> `&` (SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW lost).
#[test]
#[ignore = "native: needs desktop"]
fn place_moves_the_control_keeps_the_z_order_and_shows_it() {
    modal(1, |d, s| {
        let ids = items_of(d, DialogControlClass::Static);
        let top = |c: Hwnd| children(comctl::user32::get_parent(c)).first() == Some(&c);
        let id = ids
            .into_iter()
            .find(|&i| !top(d.native().item(i)))
            .expect("a control that is not top");
        let control = d.native().item(id);
        let parent = comctl::user32::get_parent(control);
        let order = children(parent);
        // Hide it behind the surface's back so only place() can show it again.
        api::user32::show_window(control, 0);
        assert_eq!(style(control) & WS_VISIBLE, 0);
        let rect = (13, 29, 77, 31);
        s.place(id, rect.0, rect.1, rect.2, rect.3);
        assert_eq!(
            rect_in_parent(control),
            rect,
            "place must move and size the real control"
        );
        assert_ne!(style(control) & WS_VISIBLE, 0, "SWP_SHOWWINDOW lost");
        assert_eq!(
            children(parent),
            order,
            "SWP_NOZORDER lost: the control jumped to the top"
        );
        // A second, different rectangle moves again; the same one is a cache hit and leaves the window alone.
        s.place(id, 14, 30, 78, 32);
        assert_eq!(rect_in_parent(control), (14, 30, 78, 32));
        // A negative size is clamped to zero (the cache contract) on the real window.
        s.place(id, 14, 30, -5, -7);
        let (_, _, w, h) = rect_in_parent(control);
        assert_eq!((w, h), (0, 0));
    });
}

// surface.rs:194:9 hide -> (), 195:20 `==` -> `!=`, 199:105 `!=` -> `==`, 199:91 `&` -> `|`/`^`, 199:79 delete `-`, 200:19 `||` -> `&&`.
// Equivalent, no test: 182:29 / 182:20 `|` -> `^` (disjoint bits) and 199:91 `&` -> `|` / `^` (hide() of an already hidden control is a no-op).
#[test]
#[ignore = "native: needs desktop"]
fn hide_hides_placed_controls_and_never_placed_visible_controls() {
    modal(1, |d, s| {
        let id = items_of(d, DialogControlClass::Static)[0];
        let control = d.native().item(id);
        // Placed by the layout and visible: hide() hides it.
        s.place(id, 11, 12, 60, 20);
        assert_ne!(style(control) & WS_VISIBLE, 0);
        s.hide(id);
        assert_eq!(style(control) & WS_VISIBLE, 0, "hide() must hide a placed control");
        // Now never placed (hide() dropped the cache entry) but WS_VISIBLE again, as a freshly created control is.
        api::user32::show_window(control, 8);
        assert_ne!(style(control) & WS_VISIBLE, 0);
        s.hide(id);
        assert_eq!(
            style(control) & WS_VISIBLE,
            0,
            "a never-placed WS_VISIBLE control must still be hidden"
        );
        // Absent id: nothing to do and no panic.
        s.hide(0x7FFF0);
        // Hiding an already hidden, unplaced control is a no-op.
        s.hide(id);
        assert_eq!(style(control) & WS_VISIBLE, 0);
    });
}
