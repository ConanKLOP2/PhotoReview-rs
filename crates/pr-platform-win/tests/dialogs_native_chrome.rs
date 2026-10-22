//! Tests of `dialogs::native::chrome` (W8-18d): the theme decisions, the colour conversion and the order of the chrome calls (pure, with fakes), plus the
//! oracle golden `dialog-template` (kinds `theme_for`, `color_ref`, `from_key`, `named_colors`, `constants`) and one native test on real controls.
//! Contract: `contracts/W8-18d.md`, "Test plan".
mod common {
    pub mod dialogs_chrome_fixture;
}
use common::dialogs_chrome_fixture::{Event, Events, RecordingOs, RecordingPainter, theme_recorder};
use pr_platform_win::dialogs::native::{
    chrome::{
        CLASSIC_THEME, DarkDialogChrome, DialogChromeControl, DialogColors, NativeDialogChrome, theme_for,
        theme_for_control,
    },
    template::DialogControlClass as C,
};
use pr_test_support::golden::{self, str_of};
use std::{cell::RefCell, rc::Rc};
type Hwnd = isize;

const ALL_CLASSES: [C; 9] = [
    C::Button,
    C::Edit,
    C::Static,
    C::ListBox,
    C::ScrollBar,
    C::ComboBox,
    C::ListView,
    C::Progress,
    C::Trackbar,
];

fn class_of(name: &str) -> C {
    ALL_CLASSES
        .into_iter()
        .find(|c| format!("{c:?}") == name)
        .unwrap_or_else(|| panic!("unknown class {name}"))
}

fn chrome(events: &Events, combo_list: Hwnd, list_header: Hwnd) -> DarkDialogChrome {
    DarkDialogChrome::with_seams(
        Rc::new(RecordingPainter(events.clone())),
        theme_recorder(events),
        Rc::new(RecordingOs {
            events: events.clone(),
            combo_list,
            list_header,
        }),
    )
}

fn new_events() -> Events {
    Rc::new(RefCell::new(Vec::new()))
}

fn theme(hwnd: Hwnd) -> Event {
    Event::Theme(hwnd, String::new(), Some(String::new()))
}

#[test]
fn theme_for_every_class() {
    // C#: NativeControlsTests.EveryThemedControlClass_DropsTheSystemTheme_NotADarkModeTheme (8 InlineData) and Static_KeepsItsTheme...
    for c in [
        C::Button,
        C::Edit,
        C::ListBox,
        C::ScrollBar,
        C::ComboBox,
        C::ListView,
        C::Progress,
        C::Trackbar,
    ] {
        assert_eq!(theme_for(c), Some(CLASSIC_THEME), "{c:?}");
    }
    assert_eq!(theme_for(C::Static), None);
    assert_eq!(CLASSIC_THEME, "");
}

#[test]
fn theme_for_control_ignores_the_handle() {
    for c in ALL_CLASSES {
        for hwnd in [0, 1, 0x7fff_ffff] {
            assert_eq!(theme_for_control(hwnd, c), theme_for(c));
        }
    }
}

#[test]
fn golden_theme_for_rows() {
    let golden = golden::load("dialog-template");
    let mut compared = 0;
    for case in golden.cases.iter().filter(|c| c["kind"] == "theme_for") {
        let class = class_of(str_of(case, "class"));
        assert_eq!(theme_for(class), case["theme"].as_str(), "{}", case["name"]);
        compared += 1;
    }
    assert_eq!(compared, 9);
}

#[test]
fn golden_constants_classic_theme() {
    let golden = golden::load("dialog-template");
    let case = golden
        .cases
        .iter()
        .find(|c| c["kind"] == "constants")
        .expect("the constants row");
    assert_eq!(CLASSIC_THEME, str_of(case, "classic_theme"));
}

#[test]
fn to_color_ref_matches_the_golden() {
    // Includes the .5 boundary (0.5 * 255 = 127.5 -> 128, ties to even) and the out-of-range rows.
    let golden = golden::load("dialog-template");
    let mut compared = 0;
    for case in golden.cases.iter().filter(|c| c["kind"] == "color_ref") {
        let channel = |key: &str| case[key].as_f64().unwrap_or_else(|| panic!("{key}")) as f32;
        assert_eq!(
            u64::from(DialogColors::to_color_ref(channel("r"), channel("g"), channel("b"))),
            case["value"].as_u64().expect("value"),
            "{}",
            case["name"]
        );
        compared += 1;
    }
    assert_eq!(compared, 11);
}

#[test]
fn dialog_colors_from_key_matches_the_golden() {
    // 35 found keys and 3 misses (C# KeyNotFoundException -> None; the lookup is case sensitive).
    let golden = golden::load("dialog-template");
    let (mut found, mut missed) = (0, 0);
    for case in golden.cases.iter().filter(|c| c["kind"] == "from_key") {
        let key = str_of(case, "key");
        if case["found"].as_bool().expect("found") {
            assert_eq!(
                DialogColors::from_key(key).map(u64::from),
                Some(case["value"].as_u64().expect("value")),
                "{key}"
            );
            found += 1;
        } else {
            assert_eq!(DialogColors::from_key(key), None, "{key}");
            missed += 1;
        }
    }
    assert_eq!((found, missed), (35, 3));
}

#[test]
fn named_colors_use_the_documented_keys() {
    let golden = golden::load("dialog-template");
    let case = golden
        .cases
        .iter()
        .find(|c| c["kind"] == "named_colors")
        .expect("the named colours row");
    let expected = |key: &str| case[key].as_u64().expect("value") as u32;
    assert_eq!(DialogColors::window(), expected("window"));
    assert_eq!(DialogColors::input(), expected("input"));
    assert_eq!(DialogColors::text(), expected("text"));
    assert_eq!(DialogColors::text_strong(), expected("text_strong"));
    assert_eq!(DialogColors::text_secondary(), expected("text_secondary"));
    assert_eq!(DialogColors::warning(), expected("warning"));
    for (key, color) in [
        ("Dark.Window", DialogColors::window()),
        ("Dark.Input", DialogColors::input()),
        ("Dark.WindowText", DialogColors::text()),
        ("Dark.TextStrong", DialogColors::text_strong()),
        ("Dark.TextSecondary", DialogColors::text_secondary()),
        ("Dark.Warning", DialogColors::warning()),
    ] {
        assert_eq!(DialogColors::from_key(key), Some(color), "{key}");
        assert_ne!(color, 0, "{key} must exist (a missing key would answer 0)");
    }
}

#[test]
fn apply_runs_title_bar_then_dialog_painter_then_the_controls() {
    let events = new_events();
    let chrome = chrome(&events, 0, 0);
    chrome.apply(
        7,
        &[DialogChromeControl {
            hwnd: 11,
            class: C::Button,
        }],
    );
    assert_eq!(
        *events.borrow(),
        [
            Event::Title(7),
            Event::Dialog(7),
            theme(11),
            Event::Control(11, C::Button)
        ]
    );
}

#[test]
fn apply_controls_skips_the_title_bar_and_the_dialog_painter() {
    // C#: INativeDialogChrome.ApplyControls (WP-35 GD3a): only the control theme and painter, never the title bar.
    let events = new_events();
    let chrome = chrome(&events, 0, 0);
    chrome.apply_controls(
        7,
        &[DialogChromeControl {
            hwnd: 11,
            class: C::Edit,
        }],
    );
    assert_eq!(*events.borrow(), [theme(11), Event::Control(11, C::Edit)]);
    events.borrow_mut().clear();
    chrome.apply_controls(7, &[]);
    assert!(events.borrow().is_empty());
}

#[test]
fn apply_themes_each_control_then_attaches_its_painter_in_order() {
    let events = new_events();
    let chrome = chrome(&events, 0, 0);
    let controls = [
        DialogChromeControl {
            hwnd: 21,
            class: C::Button,
        },
        DialogChromeControl {
            hwnd: 22,
            class: C::Edit,
        },
        DialogChromeControl {
            hwnd: 23,
            class: C::Progress,
        },
        DialogChromeControl {
            hwnd: 24,
            class: C::Trackbar,
        },
    ];
    chrome.apply_controls(1, &controls);
    assert_eq!(
        *events.borrow(),
        [
            theme(21),
            Event::Control(21, C::Button),
            theme(22),
            Event::Control(22, C::Edit),
            theme(23),
            Event::Control(23, C::Progress),
            theme(24),
            Event::Control(24, C::Trackbar),
        ]
    );
}

#[test]
fn static_control_gets_a_painter_but_no_theme() {
    // C#: NativeControlsTests.Static_KeepsItsTheme_AndEveryOtherControlIsClassic
    let events = new_events();
    chrome(&events, 0, 0).apply_controls(
        1,
        &[DialogChromeControl {
            hwnd: 5,
            class: C::Static,
        }],
    );
    assert_eq!(*events.borrow(), [Event::Control(5, C::Static)]);
}

#[test]
fn combo_drop_down_list_is_themed_only_when_non_zero() {
    let combo = [DialogChromeControl {
        hwnd: 31,
        class: C::ComboBox,
    }];
    let events = new_events();
    chrome(&events, 0, 0).apply_controls(1, &combo);
    assert_eq!(
        *events.borrow(),
        [theme(31), Event::Control(31, C::ComboBox), Event::ComboList(31)],
        "a combo whose list window cannot be read gets no second theme call"
    );
    let events = new_events();
    chrome(&events, 77, 0).apply_controls(1, &combo);
    assert_eq!(
        *events.borrow(),
        [
            theme(31),
            Event::Control(31, C::ComboBox),
            Event::ComboList(31),
            theme(77)
        ]
    );
}

#[test]
fn list_view_header_is_themed_and_gets_the_filler_painter() {
    let list = [DialogChromeControl {
        hwnd: 41,
        class: C::ListView,
    }];
    let events = new_events();
    chrome(&events, 0, 0).apply_controls(1, &list);
    assert_eq!(
        *events.borrow(),
        [theme(41), Event::Control(41, C::ListView), Event::ListHeader(41)],
        "no header, no theme and no filler"
    );
    let events = new_events();
    chrome(&events, 0, 88).apply_controls(1, &list);
    assert_eq!(
        *events.borrow(),
        [
            theme(41),
            Event::Control(41, C::ListView),
            Event::ListHeader(41),
            theme(88),
            Event::Header(88)
        ]
    );
}

#[test]
fn apply_does_not_query_the_parts_of_other_classes() {
    let events = new_events();
    let controls: Vec<DialogChromeControl> = ALL_CLASSES
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c, C::ComboBox | C::ListView))
        .map(|(i, c)| DialogChromeControl {
            hwnd: 100 + i as isize,
            class: *c,
        })
        .collect();
    chrome(&events, 9, 9).apply_controls(1, &controls);
    assert!(
        events
            .borrow()
            .iter()
            .all(|e| !matches!(e, Event::ComboList(_) | Event::ListHeader(_) | Event::Header(_))),
        "{:?}",
        events.borrow()
    );
}

#[test]
fn default_apply_controls_calls_apply() {
    // C#: the default interface method `ApplyControls(dialog, controls) => Apply(dialog, controls)`
    struct ApplyOnly(Rc<RefCell<Vec<(Hwnd, usize)>>>);
    impl NativeDialogChrome for ApplyOnly {
        fn apply(&self, dialog: Hwnd, controls: &[DialogChromeControl]) {
            self.0.borrow_mut().push((dialog, controls.len()));
        }
    }
    let calls = Rc::new(RefCell::new(Vec::new()));
    ApplyOnly(calls.clone()).apply_controls(
        3,
        &[DialogChromeControl {
            hwnd: 4,
            class: C::Button,
        }],
    );
    assert_eq!(*calls.borrow(), [(3, 1)]);
}

#[test]
fn real_theme_closure_ignores_errors() {
    // `DarkDialogChrome::new` installs the real `SetWindowTheme` closure and the real OS helpers: a handle of 0 fails inside the OS, which must not panic
    // (the C# `catch (DllNotFoundException | EntryPointNotFoundException)`; the real calls only see window handle 0).
    struct Quiet;
    impl pr_platform_win::dialogs::native::chrome::ChromePainter for Quiet {
        fn attach_dialog(&self, _: Hwnd) -> bool {
            false
        }
        fn attach_control(&self, _: Hwnd, _: C) -> bool {
            false
        }
        fn attach_header_filler(&self, _: Hwnd) -> bool {
            false
        }
    }
    let chrome = DarkDialogChrome::new(Rc::new(Quiet));
    chrome.apply_controls(
        0,
        &[
            DialogChromeControl {
                hwnd: 0,
                class: C::Button,
            },
            DialogChromeControl {
                hwnd: 0,
                class: C::ComboBox,
            },
            DialogChromeControl {
                hwnd: 0,
                class: C::ListView,
            },
        ],
    );
}

#[test]
fn chrome_calls_report_a_zero_os_answer_without_panicking() {
    // Every OS answer is 0 / false: nothing is themed twice and nothing panics.
    let events = new_events();
    let controls: Vec<DialogChromeControl> = ALL_CLASSES
        .iter()
        .map(|c| DialogChromeControl { hwnd: 1, class: *c })
        .collect();
    chrome(&events, 0, 0).apply(1, &controls);
    assert_eq!(events.borrow().first(), Some(&Event::Title(1)));
}

mod native {
    use super::*;
    use pr_platform_win::dialogs::native::chrome::{ChromeOs, Win32ChromeOs};
    use pr_platform_win::dialogs::native::template::window_class_name;
    use pr_platform_win::win32::api::{kernel32, user32};
    use pr_platform_win::win32::comctl::common_controls;
    use pr_platform_win::win32::control_constants::{
        BS_AUTOCHECKBOX, BS_AUTORADIOBUTTON, BS_GROUPBOX, CBS_DROPDOWNLIST, ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES,
        ICC_PROGRESS_CLASS, LVS_REPORT,
    };
    use pr_platform_win::win32::dialog_native::user32 as dn_user32;
    use pr_platform_win::win32::messages::{WS_CHILD, WS_POPUP, WS_VISIBLE};

    struct Window(Hwnd);
    impl Drop for Window {
        fn drop(&mut self) {
            if dn_user32::is_window(self.0) {
                let _ = user32::destroy_window(self.0);
            }
        }
    }

    fn create(class: &str, style: u32, parent: Hwnd) -> Window {
        let instance = kernel32::get_module_handle().expect("module handle");
        // SAFETY: built-in and common control classes with no creation parameter (0); the window belongs to this test thread.
        let hwnd = unsafe { user32::create_window_ex(0, class, "x", style, 0, 0, 100, 40, parent, 0, instance, 0) }
            .expect("create_window_ex");
        assert_ne!(hwnd, 0);
        Window(hwnd)
    }

    #[test]
    #[ignore = "native: needs desktop"]
    fn real_chrome_clears_the_theme_of_each_control_including_the_combo_drop_down_and_the_list_view_header_and_never_uses_dark_mode_themes()
     {
        // C#: NativeControlsTests.RealChrome_ClearsTheThemeOfEachControlIncludingTheComboDropDownAndTheListViewHeader_AndNeverUsesDarkModeThemes
        // (a hidden owner with real children replaces the gallery dialog; the recorder replaces SetWindowTheme, the OS helpers are the real ones).
        // ICC_STANDARD_CLASSES needs a comctl32 v6 manifest, which a test exe has not: the three common classes are enough.
        assert!(common_controls::init_common_controls_ex(
            ICC_LISTVIEW_CLASSES | ICC_BAR_CLASSES | ICC_PROGRESS_CLASS
        ));
        let owner = create("STATIC", WS_POPUP, 0);
        let child = WS_CHILD | WS_VISIBLE;
        let combo = create(window_class_name(C::ComboBox), child | CBS_DROPDOWNLIST, owner.0);
        let list_view = create(window_class_name(C::ListView), child | LVS_REPORT, owner.0);
        let radio = create("Button", child | BS_AUTORADIOBUTTON, owner.0);
        let group = create("Button", child | BS_GROUPBOX, owner.0);
        let check = create("Button", child | BS_AUTOCHECKBOX, owner.0);
        let progress = create(window_class_name(C::Progress), child, owner.0);
        let trackbar = create(window_class_name(C::Trackbar), child, owner.0);
        let edit = create("Edit", child, owner.0);
        let controls = [
            DialogChromeControl {
                hwnd: combo.0,
                class: C::ComboBox,
            },
            DialogChromeControl {
                hwnd: list_view.0,
                class: C::ListView,
            },
            DialogChromeControl {
                hwnd: radio.0,
                class: C::Button,
            },
            DialogChromeControl {
                hwnd: group.0,
                class: C::Button,
            },
            DialogChromeControl {
                hwnd: check.0,
                class: C::Button,
            },
            DialogChromeControl {
                hwnd: progress.0,
                class: C::Progress,
            },
            DialogChromeControl {
                hwnd: trackbar.0,
                class: C::Trackbar,
            },
            DialogChromeControl {
                hwnd: edit.0,
                class: C::Edit,
            },
        ];
        let events = new_events();
        let chrome = DarkDialogChrome::with_seams(
            Rc::new(RecordingPainter(events.clone())),
            theme_recorder(&events),
            Rc::new(Win32ChromeOs),
        );
        chrome.apply(owner.0, &controls);
        let themes: Vec<(Hwnd, String, Option<String>)> = events
            .borrow()
            .iter()
            .filter_map(|e| match e {
                Event::Theme(hwnd, app, ids) => Some((*hwnd, app.clone(), ids.clone())),
                _ => None,
            })
            .collect();
        let list_window = pr_platform_win::dialogs::native::chrome::combo_list_window(combo.0);
        let header = Win32ChromeOs.list_view_header(list_view.0);
        assert_ne!(list_window, 0, "the combo has a drop-down list window");
        assert_ne!(header, 0, "the report view has a header");
        for (name, hwnd) in [
            ("combo", combo.0),
            ("combo list", list_window),
            ("list view", list_view.0),
            ("list view header", header),
            ("radio", radio.0),
            ("group", group.0),
            ("check", check.0),
            ("progress", progress.0),
            ("trackbar", trackbar.0),
            ("edit", edit.0),
        ] {
            let last = themes.iter().rev().find(|(h, _, _)| *h == hwnd);
            assert_eq!(
                last,
                Some(&(hwnd, String::new(), Some(String::new()))),
                "{name}: the last theme call clears the theme"
            );
        }
        assert!(
            themes
                .iter()
                .all(|(_, app, ids)| !app.starts_with("DarkMode_")
                    && !ids.as_deref().unwrap_or("").starts_with("DarkMode_")),
            "{themes:?}"
        );
        assert!(events.borrow().contains(&Event::Dialog(owner.0)));
        assert!(events.borrow().contains(&Event::Header(header)));
    }
}
