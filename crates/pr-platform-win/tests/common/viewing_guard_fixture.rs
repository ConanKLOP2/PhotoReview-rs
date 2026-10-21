//! Fixture of the W8-16i tests (`shell_viewing_presentation_sink.rs`): a recording fake perf log and an opaque test image.
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use pr_imaging::decode_types::PlatformImage;
use pr_platform_win::shell_viewing::presentation_sink::PerfMarks;

/// Recording perf log: `(event name, token)` in call order; `ms_since` returns a fixed number.
pub struct FakePerfMarks {
    pub enabled: Cell<bool>,
    pub events: RefCell<Vec<(String, i64)>>,
    pub kinds: RefCell<Vec<String>>,
    pub ms: Cell<f64>,
}

impl FakePerfMarks {
    pub fn new(enabled: bool) -> Rc<Self> {
        Rc::new(Self {
            enabled: Cell::new(enabled),
            events: RefCell::new(Vec::new()),
            kinds: RefCell::new(Vec::new()),
            ms: Cell::new(1.5),
        })
    }

    pub fn events(&self) -> Vec<(String, i64)> {
        self.events.borrow().clone()
    }
}

impl PerfMarks for FakePerfMarks {
    fn is_enabled(&self) -> bool {
        self.enabled.get()
    }
    fn ms_since(&self, _assigned_timestamp: i64) -> f64 {
        self.ms.get()
    }
    fn rendered(&self, token: i64, _ms: f64) {
        self.events.borrow_mut().push(("Rendered".to_owned(), token));
    }
    fn presented(&self, token: i64, kind: &str) {
        self.events.borrow_mut().push(("Presented".to_owned(), token));
        self.kinds.borrow_mut().push(kind.to_owned());
    }
    fn rendered_frame(&self, token: i64, _ms: f64) {
        self.events.borrow_mut().push(("RenderedFrame".to_owned(), token));
    }
}

/// A distinguishable image without pixels.
pub fn test_image(tag: i32) -> PlatformImage {
    PlatformImage::Opaque(Arc::new(tag))
}

/// The tag of [`test_image`], `None` for another image.
pub fn tag_of(image: &PlatformImage) -> Option<i32> {
    match image {
        PlatformImage::Opaque(any) => any.downcast_ref::<i32>().copied(),
        PlatformImage::Pixels(_) => None,
    }
}
