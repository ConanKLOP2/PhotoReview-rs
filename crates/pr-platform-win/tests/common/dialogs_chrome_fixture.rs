use pr_platform_win::dialogs::native::{
    chrome::{ChromeOs, ChromePainter},
    template::DialogControlClass,
};
use std::{cell::RefCell, rc::Rc};
type Hwnd = isize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Title(Hwnd),
    Dialog(Hwnd),
    Control(Hwnd, DialogControlClass),
    Header(Hwnd),
    ComboList(Hwnd),
    ListHeader(Hwnd),
}

pub struct RecordingPainter(pub Rc<RefCell<Vec<Event>>>);
impl ChromePainter for RecordingPainter {
    #[rustfmt::skip]
    fn attach_dialog(&self, hwnd: Hwnd) -> bool { self.0.borrow_mut().push(Event::Dialog(hwnd)); true }
    #[rustfmt::skip]
    fn attach_control(&self, hwnd: Hwnd, class: DialogControlClass) -> bool { self.0.borrow_mut().push(Event::Control(hwnd, class)); true }
    #[rustfmt::skip]
    fn attach_header_filler(&self, hwnd: Hwnd) -> bool { self.0.borrow_mut().push(Event::Header(hwnd)); true }
}

pub struct RecordingOs {
    pub events: Rc<RefCell<Vec<Event>>>,
    pub combo_list: Hwnd,
    pub list_header: Hwnd,
}
impl ChromeOs for RecordingOs {
    #[rustfmt::skip]
    fn apply_title_bar(&self, hwnd: Hwnd) -> bool { self.events.borrow_mut().push(Event::Title(hwnd)); true }
    #[rustfmt::skip]
    fn combo_list_window(&self, hwnd: Hwnd) -> Hwnd { self.events.borrow_mut().push(Event::ComboList(hwnd)); self.combo_list }
    #[rustfmt::skip]
    fn list_view_header(&self, hwnd: Hwnd) -> Hwnd { self.events.borrow_mut().push(Event::ListHeader(hwnd)); self.list_header }
}
