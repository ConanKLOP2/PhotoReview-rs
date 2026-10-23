//! Port of `ControlGalleryDialog.cs` at baseline d92d6aae for native integration tests.
#![allow(dead_code)] // Individual gallery tests exercise different controls and logs.

use std::cell::{Cell, RefCell};

use pr_platform_win::dialogs::native::chrome::DialogColors;
use pr_platform_win::dialogs::native::controls::basic::{ComboBoxControl, ListBoxControl};
use pr_platform_win::dialogs::native::controls::custom::{ButtonStateControl, ProgressControl, TrackbarControl};
use pr_platform_win::dialogs::native::controls::list_tree::ListViewControl;
use pr_platform_win::dialogs::native::dialog::{DialogContent, ListItemText, NativeDialog};
use pr_platform_win::dialogs::native::template::{DialogControlClass as Class, DialogItem};
use pr_platform_win::win32::structs::{Lparam, Lresult};
use pr_platform_win::win32::{control_constants, dialog_native};
use windows::Win32::UI::Controls::{NMLISTVIEW, NMLVCUSTOMDRAW};

pub const NAME_LABEL: i32 = 100;
pub const COMBO: i32 = 101;
pub const BADGE_LIST: i32 = 102;
pub const REPORT: i32 = 103;
pub const PROGRESS: i32 = 104;
pub const SLIDER: i32 = 105;
pub const NOTES: i32 = 106;
pub const COUNT: i32 = 107;
pub const CHECK: i32 = 108;
pub const RADIO_A: i32 = 109;
pub const RADIO_B: i32 = 110;
pub const GROUP: i32 = 111;
pub const TIP_BUTTON: i32 = 112;
pub const SLIDER_VALUE: i32 = 113;

const CHILD: u32 = 0x5000_0000;
const TAB: u32 = 0x0001_0000;

pub struct ControlGalleryContent {
    pub notifications: RefCell<Vec<(i32, i32)>>,
    pub scrolls: RefCell<Vec<(i32, i32)>>,
    pub column_clicks: RefCell<Vec<i32>>,
    pub last_command_id: Cell<i32>,
    rows: RefCell<Vec<Vec<String>>>,
    sort_column: Cell<i32>,
    sort_direction: Cell<i32>,
}

impl Default for ControlGalleryContent {
    fn default() -> Self {
        Self {
            notifications: RefCell::new(Vec::new()),
            scrolls: RefCell::new(Vec::new()),
            column_clicks: RefCell::new(Vec::new()),
            last_command_id: Cell::new(0),
            rows: RefCell::new(vec![
                [
                    "",
                    "Fast Sequential",
                    "Fast",
                    "Sequential",
                    "32",
                    "12 ms",
                    "18 ms",
                    "22 ms",
                    "31 ms",
                    "Pass",
                ]
                .map(str::to_owned)
                .to_vec(),
                [
                    "🏆",
                    "Preview Light",
                    "Preview",
                    "Sequential",
                    "32",
                    "9 ms",
                    "11 ms",
                    "14 ms",
                    "15 ms",
                    "Pass",
                ]
                .map(str::to_owned)
                .to_vec(),
                [
                    "",
                    "Original Correctness",
                    "Original",
                    "Correctness",
                    "3",
                    "120 ms",
                    "140 ms",
                    "150 ms",
                    "151 ms",
                    "Fail",
                ]
                .map(str::to_owned)
                .to_vec(),
                [
                    "",
                    "Rapid Key Press",
                    "Preview",
                    "WarmNext",
                    "16",
                    "20 ms",
                    "26 ms",
                    "30 ms",
                    "33 ms",
                    "Warn",
                ]
                .map(str::to_owned)
                .to_vec(),
            ]),
            sort_column: Cell::new(-1),
            sort_direction: Cell::new(0),
        }
    }
}

impl ControlGalleryContent {
    pub fn sort_by(&self, dialog: &NativeDialog, column: i32) {
        let direction = if self.sort_column.get() == column && self.sort_direction.get() == 1 {
            2
        } else {
            1
        };
        self.sort_column.set(column);
        self.sort_direction.set(direction);
        let index = column as usize;
        self.rows.borrow_mut().sort_by(|a, b| {
            let order = a.get(index).cmp(&b.get(index));
            if direction == 1 { order } else { order.reverse() }
        });
        self.fill_rows(dialog);
        dialog.set_list_view_sort(REPORT, column, direction);
    }

    fn fill_rows(&self, dialog: &NativeDialog) {
        let report = dialog.item(REPORT);
        ListViewControl::clear(report);
        for (index, row) in self.rows.borrow().iter().enumerate() {
            ListViewControl::add_row(report, row, index as isize);
        }
    }

    pub fn build_items() -> Vec<DialogItem> {
        vec![
            DialogItem::new(
                Class::Static,
                NAME_LABEL,
                "Operation",
                CHILD | dialog_native::SS_NOPREFIX,
            ),
            DialogItem::new(Class::ComboBox, COMBO, "", CHILD | ComboBoxControl::DEFAULT_STYLE),
            DialogItem::new(
                Class::ListBox,
                BADGE_LIST,
                "",
                CHILD | TAB | 0x0020_0000 | 0x0001 | 0x0100 | 0x0008 | 0x0010 | 0x0040,
            )
            .with_ex_style(dialog_native::WS_EX_CLIENTEDGE),
            DialogItem::new(Class::ListView, REPORT, "", CHILD | ListViewControl::DEFAULT_STYLE),
            DialogItem::new(Class::Progress, PROGRESS, "", CHILD | ProgressControl::DEFAULT_STYLE),
            DialogItem::new(Class::Trackbar, SLIDER, "", CHILD | TrackbarControl::DEFAULT_STYLE),
            DialogItem::new(Class::Static, SLIDER_VALUE, "0", CHILD | dialog_native::SS_NOPREFIX),
            DialogItem::new(
                Class::Edit,
                NOTES,
                "Line one\r\nLine two",
                CHILD | TAB | 0x0020_0000 | 0x0004 | 0x0040 | 0x1000,
            )
            .with_ex_style(dialog_native::WS_EX_CLIENTEDGE),
            DialogItem::new(Class::Edit, COUNT, "64", CHILD | TAB | 0x0080 | 0x2000)
                .with_ex_style(dialog_native::WS_EX_CLIENTEDGE),
            DialogItem::new(Class::Button, GROUP, "Options", CHILD | 0x0007),
            DialogItem::new(Class::Button, CHECK, "Confirm before running", CHILD | TAB | 0x0003),
            DialogItem::new(Class::Button, RADIO_A, "Move", CHILD | TAB | 0x0002_0000 | 0x0009),
            DialogItem::new(Class::Button, RADIO_B, "Copy", CHILD | 0x0009),
            DialogItem::new(Class::Button, TIP_BUTTON, "Hover me", CHILD | TAB),
            DialogItem::new(Class::Button, 2, "Cancel", CHILD | TAB),
            DialogItem::new(Class::Button, 1, "OK", CHILD | TAB | 0x0001),
        ]
    }
}

impl DialogContent for ControlGalleryContent {
    fn title(&self) -> String {
        "Control gallery".into()
    }
    fn items(&self) -> Vec<DialogItem> {
        Self::build_items()
    }
    fn resizable(&self) -> bool {
        true
    }
    fn preferred_client_size(&self, d: &NativeDialog) -> (i32, i32) {
        (d.dx(420), d.dy(270))
    }
    fn minimum_client_size(&self, d: &NativeDialog) -> (i32, i32) {
        (d.dx(300), d.dy(200))
    }
    fn on_initialized(&self, d: &NativeDialog) {
        for label in ["Move", "Copy", "Recycle"] {
            ComboBoxControl::add(d.item(COMBO), label);
        }
        ComboBoxControl::select(d.item(COMBO), 0);
        for label in [
            "Fast Sequential",
            "Preview Light",
            "Original Correctness",
            "Rapid Key Press",
        ] {
            ListBoxControl::add(d.item(BADGE_LIST), label);
        }
        ListBoxControl::set_selected(d.item(BADGE_LIST), 1, true);
        let report = d.item(REPORT);
        ListViewControl::configure(
            report,
            DialogColors::from_key("Dark.Window").unwrap(),
            DialogColors::text(),
        );
        for (index, header) in [
            "🏆", "Profile", "Loading", "Workload", "N", "P50", "P95", "P99", "Max", "Status",
        ]
        .iter()
        .enumerate()
        {
            ListViewControl::add_column(report, index as i32, header, d.dx(40), (4..=8).contains(&index));
        }
        self.fill_rows(d);
        ProgressControl::configure(d.item(PROGRESS));
        ProgressControl::set_fraction(d.item(PROGRESS), 0.4);
        TrackbarControl::configure(d.item(SLIDER), 0, 100, 10, 1, 10);
        TrackbarControl::set_position(d.item(SLIDER), 30);
        d.set_item_text(SLIDER_VALUE, "30");
        ButtonStateControl::set_checked(d.item(CHECK), true);
        ButtonStateControl::set_checked(d.item(RADIO_A), true);
        d.set_tooltip(TIP_BUTTON, "A dark tooltip for a plain button");
        d.set_item_tooltips(
            BADGE_LIST,
            std::rc::Rc::new(|index| {
                ["Fast", "Preview", "Original", "Preview"]
                    .get(index as usize)
                    .map(|badge| format!("Mode: {badge}"))
            }),
        );
    }
    fn list_item_text(&self, d: &NativeDialog, id: i32, index: i32) -> ListItemText {
        if id == BADGE_LIST {
            let names = [
                "Fast Sequential",
                "Preview Light",
                "Original Correctness",
                "Rapid Key Press",
            ];
            let badges = ["Fast", "Preview", "Original", "Preview"];
            if let (Some(name), Some(badge)) = (names.get(index as usize), badges.get(index as usize)) {
                return ListItemText {
                    text: (*name).into(),
                    badge: Some((*badge).into()),
                };
            }
        }
        ListItemText {
            text: ListBoxControl::text_at(d.item(id), index),
            badge: None,
        }
    }
    fn on_notify(&self, dialog: &NativeDialog, id: i32, code: i32, header: Lparam) -> Option<Lresult> {
        self.notifications.borrow_mut().push((id, code));
        if id != REPORT || header == 0 {
            return None;
        }
        if code == control_constants::NM_CUSTOMDRAW {
            // SAFETY: WM_NOTIFY supplies a live NMLVCUSTOMDRAW for NM_CUSTOMDRAW, valid during the hook.
            let custom = unsafe { &mut *(header as *mut NMLVCUSTOMDRAW) };
            return Some(ListViewControl::custom_draw(custom, &mut |row, param| {
                let rows = self.rows.borrow();
                let data = rows.get(param as usize);
                let background = if data.and_then(|r| r.first()).is_some_and(|value| !value.is_empty()) {
                    "Dark.SuccessBackground"
                } else if data.and_then(|r| r.get(9)).is_some_and(|value| value == "Fail") {
                    "Dark.ErrorBackground"
                } else if row % 2 == 0 {
                    "Dark.Panel"
                } else {
                    "Dark.RowAlt"
                };
                (DialogColors::from_key(background).unwrap(), DialogColors::text())
            }));
        }
        if code == control_constants::LVN_COLUMNCLICK {
            // SAFETY: WM_NOTIFY supplies NMLISTVIEW for LVN_COLUMNCLICK, valid during the hook.
            let column = unsafe { (*(header as *const NMLISTVIEW)).iSubItem };
            self.column_clicks.borrow_mut().push(column);
            self.sort_by(dialog, column);
            return Some(0);
        }
        None
    }
    fn on_scroll(&self, dialog: &NativeDialog, id: i32, code: i32) {
        self.scrolls.borrow_mut().push((id, code));
        if id == SLIDER {
            dialog.set_item_text(
                SLIDER_VALUE,
                &TrackbarControl::position(dialog.item(SLIDER)).to_string(),
            );
        }
    }
    fn on_command(&self, _: &NativeDialog, id: i32, _: i32) -> bool {
        self.last_command_id.set(id);
        false
    }
    fn layout_controls(&self, d: &NativeDialog, client_width: i32, client_height: i32) {
        let m = d.dx(8);
        let left = d.dx(120).max(client_width / 3);
        let right = m + left + m;
        let right_width = (client_width - right - m).max(d.dx(60));
        let line = d.dy(12).max(d.line_height() + d.dy(4));
        let button_height = d.dy(14).max(d.line_height() + d.dy(6));
        let mut y = m;
        d.place(NAME_LABEL, m, y, left, d.line_height());
        y += d.line_height() + d.dy(2);
        d.place(COMBO, m, y, left, line + 4 * (d.line_height() + d.dy(2)));
        y += line + d.dy(6);
        d.set_list_item_height(BADGE_LIST, d.line_height() + d.dy(5));
        d.place(BADGE_LIST, m, y, left, d.dy(60));
        y += d.dy(66);
        d.place(SLIDER, m, y, left - d.dx(20), d.dy(18));
        d.place(
            SLIDER_VALUE,
            m + left - d.dx(18),
            y + d.dy(3),
            d.dx(18),
            d.line_height(),
        );
        y += d.dy(22);
        d.place(PROGRESS, m, y, left, d.dy(6));
        y += d.dy(12);
        d.place(COUNT, m, y, d.dx(50), line);
        y += line + d.dy(4);
        d.place(NOTES, m, y, left, d.dy(40));
        d.place(REPORT, right, m, right_width, d.dy(90));
        for (index, units) in [14, 52, 28, 34, 14, 22, 22, 22, 22, 30].iter().enumerate() {
            ListViewControl::set_column_width(d.item(REPORT), index as i32, d.dx(*units));
        }
        let group_top = m + d.dy(96);
        d.place(GROUP, right, group_top, right_width, d.dy(60));
        d.place(
            CHECK,
            right + d.dx(8),
            group_top + d.dy(14),
            right_width - d.dx(16),
            line,
        );
        d.place(RADIO_A, right + d.dx(8), group_top + d.dy(14) + line, d.dx(60), line);
        d.place(RADIO_B, right + d.dx(72), group_top + d.dy(14) + line, d.dx(60), line);
        d.place(TIP_BUTTON, right, group_top + d.dy(66), d.dx(70), button_height);
        d.place_button_row(
            client_width,
            m,
            client_height - m - button_height,
            button_height,
            &[2, 1],
        );
    }
}
