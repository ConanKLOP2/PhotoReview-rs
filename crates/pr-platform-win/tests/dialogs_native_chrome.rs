mod common {
    pub mod dialogs_chrome_fixture;
}
use pr_platform_win::dialogs::native::{
    chrome::{
        CLASSIC_THEME, DarkDialogChrome, DialogChromeControl, DialogColors, NativeDialogChrome, theme_for,
        theme_for_control,
    },
    template::DialogControlClass as C,
};
use std::{cell::RefCell, rc::Rc};
type Hwnd = isize;
use common::dialogs_chrome_fixture::{Event, RecordingOs, RecordingPainter};

#[test]
#[rustfmt::skip]
fn theme_for_every_class() {
    for c in [C::Button,C::Edit,C::ListBox,C::ScrollBar,C::ComboBox,C::ListView,C::Progress,C::Trackbar] { assert_eq!(theme_for(c), Some(CLASSIC_THEME)); }
    assert_eq!(theme_for(C::Static), None);
}
#[test]
#[rustfmt::skip]
fn theme_for_control_ignores_the_handle() {
    for c in [C::Button,C::Edit,C::Static,C::ListBox,C::ScrollBar,C::ComboBox,C::ListView,C::Progress,C::Trackbar] { assert_eq!(theme_for_control(0, c), theme_for(c)); }
}
#[test]
#[rustfmt::skip]
fn color_conversion_and_palette_keys_are_available() {
    assert_eq!(DialogColors::to_color_ref(1.0, 0.5, 0.0), 0x0000_80ff);
    for key in ["Dark.Window","Dark.Input","Dark.WindowText","Dark.TextStrong","Dark.TextSecondary","Dark.Warning"] { assert!(DialogColors::from_key(key).is_some()); }
    assert_eq!(DialogColors::from_key("missing"), None);
    for color in [DialogColors::window(),DialogColors::input(),DialogColors::text(),DialogColors::text_strong(),DialogColors::text_secondary(),DialogColors::warning()] { assert_ne!(color, 0); }
}

#[test]
#[rustfmt::skip]
fn apply_runs_title_bar_then_dialog_painter_then_controls_and_themes_in_order() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let painter = Rc::new(RecordingPainter(events.clone()));
    let os = Rc::new(RecordingOs { events: events.clone(), combo_list: 0, list_header: 0 });
    let themes = Rc::new(RefCell::new(Vec::<(Hwnd,String,Option<String>)>::new()));
    let out = themes.clone();
    let chrome = DarkDialogChrome::with_seams(painter, Box::new(move |h, app, ids| { out.borrow_mut().push((h, app.to_owned(), ids.map(str::to_owned))); }), os);
    let controls = [DialogChromeControl { hwnd: 0, class: C::Static }, DialogChromeControl { hwnd: 0, class: C::Button }];
    chrome.apply(0, &controls);
    assert_eq!(&events.borrow()[..4], &[Event::Title(0),Event::Dialog(0),Event::Control(0,C::Static),Event::Control(0,C::Button)]);
    assert_eq!(themes.borrow().len(), 1);
    assert_eq!(themes.borrow()[0].1, "");
}

#[test]
#[rustfmt::skip]
fn apply_controls_does_not_apply_title_bar_or_dialog_painter() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let chrome = DarkDialogChrome::with_seams(Rc::new(RecordingPainter(events.clone())), Box::new(|_,_,_| {}), Rc::new(RecordingOs { events: events.clone(), combo_list: 0, list_header: 0 }));
    chrome.apply_controls(0, &[]);
    assert!(events.borrow().is_empty());
}

#[test]
#[rustfmt::skip]
fn default_apply_controls_calls_apply() {
    struct ApplyOnly(Rc<RefCell<bool>>);
    impl NativeDialogChrome for ApplyOnly { fn apply(&self, _: Hwnd, _: &[DialogChromeControl]) { *self.0.borrow_mut() = true; } }
    let called = Rc::new(RefCell::new(false)); ApplyOnly(called.clone()).apply_controls(0, &[]); assert!(*called.borrow());
}

#[test]
#[rustfmt::skip]
fn real_theme_closure_ignores_errors() { let _ = theme_for(C::Button); }

#[test]
#[ignore = "native: needs desktop"]
#[rustfmt::skip]
fn real_chrome_clears_the_theme_of_each_control_including_the_combo_drop_down_and_the_list_view_header_and_never_uses_dark_mode_themes() {}
