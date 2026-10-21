//! Amendment A11 of the Wave 8 B4 reconciliation (D-W8B4X-13): `MainViewModelSinkCallbacks`, `SetCurrentImageCallback` and
//! `PresentationSinkFactory` live in `pr_app::coordinators::sink_callbacks`. Port of the sink part of
//! `PhotoReview.App.Shared/Composition/SharedServiceContracts.cs`. Every test is marked `// New test` (the C# record has no test).

use std::cell::RefCell;
use std::rc::Rc;

use pr_app::coordinators::presenter::state::ImageChange;
use pr_app::coordinators::sink_callbacks::{
    MainViewModelSinkCallbacks, PresentationSinkFactory, SetCurrentImageCallback,
};
use pr_app::coordinators::traits::IPresentationSink;
use pr_imaging::decode_types::PlatformImage;

/// A sink that forwards every call to the callbacks it was created with, in the order a host sink does (view model first).
struct ForwardingSink {
    callbacks: MainViewModelSinkCallbacks,
}

impl IPresentationSink for ForwardingSink {
    fn set_current_image(&mut self, image: Option<PlatformImage>, is_file_change: bool, change: &ImageChange<'_>) {
        (self.callbacks.set_current_image)(image, is_file_change, change);
    }

    fn take_viewer_zoom_change(&mut self) -> Option<Option<f64>> {
        (self.callbacks.take_viewer_zoom_change)()
    }

    fn set_status_text(&mut self, status: &str) {
        (self.callbacks.set_status_text)(status);
    }

    fn apply_initial_view_mode(&mut self) {
        (self.callbacks.apply_initial_view_mode)();
    }

    fn on_presented(&mut self, path: &str) {
        (self.callbacks.on_presented)(path);
    }

    fn trace_presented(&mut self, _token: i64, _kind: &str, _assigned_timestamp: i64) {}
}

struct ForwardingFactory;

impl PresentationSinkFactory for ForwardingFactory {
    fn create(&self, callbacks: MainViewModelSinkCallbacks) -> Box<dyn IPresentationSink> {
        Box::new(ForwardingSink { callbacks })
    }
}

fn recording_callbacks(log: &Rc<RefCell<Vec<String>>>) -> MainViewModelSinkCallbacks {
    let set_image_log = Rc::clone(log);
    let set_image: SetCurrentImageCallback = Box::new(move |image, is_file_change, change| {
        set_image_log.borrow_mut().push(format!(
            "image {} {is_file_change} {:?} {}x{}",
            image.is_some(),
            change.presented_path,
            change.original_width,
            change.original_height
        ));
    });
    let status_log = Rc::clone(log);
    let init_log = Rc::clone(log);
    let presented_log = Rc::clone(log);
    let zoom_log = Rc::clone(log);
    MainViewModelSinkCallbacks {
        set_current_image: set_image,
        set_status_text: Box::new(move |text| status_log.borrow_mut().push(format!("status {text}"))),
        apply_initial_view_mode: Box::new(move || init_log.borrow_mut().push("init".to_owned())),
        on_presented: Box::new(move |path| presented_log.borrow_mut().push(format!("presented {path}"))),
        take_viewer_zoom_change: Box::new(move || {
            zoom_log.borrow_mut().push("zoom".to_owned());
            Some(None)
        }),
    }
}

#[test]
fn factory_create_hands_every_callback_to_the_sink_including_the_zoom_pull() {
    // New test (amendment A11: the fifth closure `take_viewer_zoom_change` reaches the sink)
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut sink = ForwardingFactory.create(recording_callbacks(&log));
    let change = ImageChange {
        presented_path: Some("a.jpg"),
        original_width: 4,
        original_height: 3,
        is_same_source_swap: false,
    };

    sink.set_current_image(None, true, &change);
    let zoom = sink.take_viewer_zoom_change();
    sink.set_status_text("ready");
    sink.apply_initial_view_mode();
    sink.on_presented("a.jpg");

    assert_eq!(
        zoom,
        Some(None),
        "the pull returns what the closure returned (a Fit request)"
    );
    assert_eq!(
        *log.borrow(),
        vec![
            "image false true Some(\"a.jpg\") 4x3".to_owned(),
            "zoom".to_owned(),
            "status ready".to_owned(),
            "init".to_owned(),
            "presented a.jpg".to_owned(),
        ]
    );
}

#[test]
fn callbacks_debug_output_names_the_type_without_the_closures() {
    // New test (amendment A11: the moved `Debug` impl)
    let log = Rc::new(RefCell::new(Vec::new()));

    let text = format!("{:?}", recording_callbacks(&log));

    assert!(text.starts_with("MainViewModelSinkCallbacks"), "{text}");
}
