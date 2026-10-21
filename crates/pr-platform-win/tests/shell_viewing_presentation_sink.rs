//! W8-16i: `Win32PresentationSink` + `ShellPresentationHub` (C# `Win32PresentationSinkTests`, `ShellPresentationHubTests`). Test kind F: recording
//! callbacks and a fake perf log; no native window.

#[path = "common/viewing_guard_fixture.rs"]
mod viewing_guard_fixture;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use pr_app::coordinators::presenter::state::ImageChange;
use pr_app::coordinators::traits::IPresentationSink;
use pr_core::diagnostics::perf::{PerfEvent, PerfListener, PerfValue, PhotoReviewPerf};
use pr_platform_win::shell_viewing::presentation_sink::{
    ImageChangedHandler, MAX_PENDING_TRACES, RealPerfMarks, SetCurrentImageCallback, ShellPresentationHub,
    SinkCallbacks, Win32PresentationSink, Win32PresentationSinkFactory,
};
use viewing_guard_fixture::{FakePerfMarks, tag_of, test_image};

type Log = Rc<RefCell<Vec<String>>>;
type ChangeFacts = (Option<String>, i32, i32, bool);
type Raised = (Option<i32>, bool);

fn change() -> ImageChange<'static> {
    ImageChange {
        presented_path: Some("p"),
        original_width: 640,
        original_height: 480,
        is_same_source_swap: false,
    }
}

fn cap(b: bool) -> &'static str {
    if b { "True" } else { "False" }
}

fn callbacks(log: &Log) -> SinkCallbacks {
    let l = Rc::clone(log);
    let set_current_image: SetCurrentImageCallback = Box::new(move |_image, is_file_change, _change| {
        l.borrow_mut().push(format!("viewmodel:{}", cap(is_file_change)))
    });
    let l = Rc::clone(log);
    let set_status_text: Box<dyn FnMut(&str)> = Box::new(move |s| l.borrow_mut().push(format!("status:{s}")));
    let l = Rc::clone(log);
    let apply_initial_view_mode: Box<dyn FnMut()> = Box::new(move || l.borrow_mut().push("initialView".to_owned()));
    let l = Rc::clone(log);
    let on_presented: Box<dyn FnMut(&str)> = Box::new(move |p| l.borrow_mut().push(format!("presented:{p}")));
    SinkCallbacks {
        set_current_image,
        set_status_text,
        apply_initial_view_mode,
        on_presented,
        take_viewer_zoom_change: Box::new(|| None),
    }
}

fn hub_with_log(log: &Log) -> Rc<ShellPresentationHub> {
    let hub = Rc::new(ShellPresentationHub::with_perf(FakePerfMarks::new(true)));
    let l = Rc::clone(log);
    hub.subscribe_image_changed(Rc::new(move |_image, is_file_change| {
        l.borrow_mut().push(format!("surface:{}", cap(is_file_change)))
    }));
    hub
}

// C#: Win32PresentationSinkTests.Sink_UpdatesTheViewModelBeforeTheSurfaceSeesTheNewImage
#[test]
fn sink_updates_the_view_model_before_the_surface_sees_the_new_image() {
    let log: Log = Rc::default();
    let hub = hub_with_log(&log);
    let mut sink = Win32PresentationSink::new(hub, callbacks(&log));
    sink.set_current_image(Some(test_image(1)), true, &change());
    sink.set_status_text("s");
    sink.apply_initial_view_mode();
    sink.on_presented("p");
    assert_eq!(
        *log.borrow(),
        [
            "viewmodel:True",
            "surface:True",
            "status:s",
            "initialView",
            "presented:p"
        ]
    );
}

// C#: Win32PresentationSinkTests.Factory_BuildsASinkOverTheSharedHub
#[test]
fn factory_builds_a_sink_over_the_shared_hub() {
    let log: Log = Rc::default();
    let hub = hub_with_log(&log);
    let factory = Win32PresentationSinkFactory::new(Rc::clone(&hub));
    let mut sink = factory.create(callbacks(&log));
    sink.set_current_image(Some(test_image(7)), false, &change());
    assert_eq!(hub.current_image().as_ref().and_then(tag_of), Some(7));
    assert_eq!(*log.borrow(), ["viewmodel:False", "surface:False"]);
}

#[test]
fn sink_forwards_status_initial_view_and_presented_without_touching_the_hub() {
    let log: Log = Rc::default();
    let hub = hub_with_log(&log);
    let mut sink = Win32PresentationSink::new(Rc::clone(&hub), callbacks(&log));
    sink.set_status_text("a");
    sink.apply_initial_view_mode();
    sink.on_presented("x");
    assert_eq!(hub.image_change_count(), 0);
    assert!(hub.current_image().is_none());
    assert_eq!(*log.borrow(), ["status:a", "initialView", "presented:x"]);
}

#[test]
fn take_viewer_zoom_change_is_forwarded_once_per_callback_call() {
    let log: Log = Rc::default();
    let hub = hub_with_log(&log);
    let mut cb = callbacks(&log);
    let queue = Rc::new(RefCell::new(vec![Some(Some(2.0)), Some(None)]));
    let q = Rc::clone(&queue);
    cb.take_viewer_zoom_change = Box::new(move || {
        let mut q = q.borrow_mut();
        if q.is_empty() { None } else { q.remove(0) }
    });
    let mut sink = Win32PresentationSink::new(hub, cb);
    assert_eq!(sink.take_viewer_zoom_change(), Some(Some(2.0)));
    assert_eq!(sink.take_viewer_zoom_change(), Some(None));
    assert_eq!(sink.take_viewer_zoom_change(), None);
}

#[test]
fn set_current_image_passes_the_change_to_the_view_model() {
    let hub = Rc::new(ShellPresentationHub::with_perf(FakePerfMarks::new(true)));
    let seen: Rc<RefCell<Vec<ChangeFacts>>> = Rc::default();
    let s = Rc::clone(&seen);
    let log: Log = Rc::default();
    let mut cb = callbacks(&log);
    cb.set_current_image = Box::new(move |_i, _f, c| {
        s.borrow_mut().push((
            c.presented_path.map(str::to_owned),
            c.original_width,
            c.original_height,
            c.is_same_source_swap,
        ));
    });
    let mut sink = Win32PresentationSink::new(hub, cb);
    let c = ImageChange {
        presented_path: Some("a.jpg"),
        original_width: 10,
        original_height: 20,
        is_same_source_swap: true,
    };
    sink.set_current_image(None, false, &c);
    assert_eq!(*seen.borrow(), [(Some("a.jpg".to_owned()), 10, 20, true)]);
}

// C#: ShellPresentationHubTests.Trace_AddedBeforeTheFrame_IsEmittedWhenAFrameWithTheImageIsPresented
#[test]
fn trace_added_before_the_frame_is_emitted_when_a_frame_with_the_image_is_presented() {
    let perf = FakePerfMarks::new(true);
    let hub = ShellPresentationHub::with_perf(perf.clone());
    hub.set_current_image(Some(test_image(1)), true);
    hub.add_trace(5, "full", 100);
    hub.note_frame_presented(false);
    assert!(perf.events().is_empty());
    assert_eq!(hub.pending_trace_count(), 1);
    hub.note_frame_presented(true);
    assert_eq!(perf.events().len(), 3);
    assert_eq!(hub.pending_trace_count(), 0);
    hub.note_frame_presented(true);
    assert_eq!(perf.events().len(), 3);
}

// C#: ShellPresentationHubTests.Trace_AddedAfterTheImageWasAlreadyPresented_IsEmittedImmediately
#[test]
fn trace_added_after_the_image_was_already_presented_is_emitted_immediately() {
    let perf = FakePerfMarks::new(true);
    let hub = ShellPresentationHub::with_perf(perf.clone());
    hub.set_current_image(Some(test_image(1)), true);
    hub.note_frame_presented(true);
    hub.add_trace(9, "full", 1);
    assert_eq!(perf.events().len(), 3);
    assert_eq!(hub.pending_trace_count(), 0);
}

// C#: ShellPresentationHubTests.NewImage_ResetsThePresentedFlag_SoItsTraceWaitsForItsOwnFrame
#[test]
fn new_image_resets_the_presented_flag_so_its_trace_waits_for_its_own_frame() {
    let perf = FakePerfMarks::new(true);
    let hub = ShellPresentationHub::with_perf(perf.clone());
    hub.set_current_image(Some(test_image(1)), true);
    hub.note_frame_presented(true);
    hub.set_current_image(Some(test_image(2)), true);
    hub.add_trace(2, "full", 1);
    assert!(perf.events().is_empty());
    assert_eq!(hub.pending_trace_count(), 1);
    hub.note_frame_presented(true);
    assert_eq!(perf.events().len(), 3);
}

// C#: ShellPresentationHubTests.Win32Sink_TracePresented_ReachesTheHubAndIsEmittedWithTheFrameThatHasTheImage
#[test]
fn win32_sink_trace_presented_reaches_the_hub_and_is_emitted_with_the_frame_that_has_the_image() {
    let perf = FakePerfMarks::new(true);
    let hub = Rc::new(ShellPresentationHub::with_perf(perf.clone()));
    let log: Log = Rc::default();
    let mut sink = Win32PresentationSink::new(Rc::clone(&hub), callbacks(&log));
    sink.set_current_image(Some(test_image(1)), true, &change());
    sink.trace_presented(42, "full", 7);
    assert!(perf.events().is_empty());
    hub.note_frame_presented(true);
    assert_eq!(
        perf.events(),
        [
            ("Rendered".to_owned(), 42),
            ("Presented".to_owned(), 42),
            ("RenderedFrame".to_owned(), 42)
        ]
    );
    assert_eq!(*perf.kinds.borrow(), ["full"]);
}

// C#: ShellPresentationHubTests.ImageChanged_IsRaisedWithTheNewImageAndTheFileChangeFlag
#[test]
fn image_changed_is_raised_with_the_new_image_and_the_file_change_flag() {
    let hub = ShellPresentationHub::with_perf(FakePerfMarks::new(true));
    let seen: Rc<RefCell<Vec<Raised>>> = Rc::default();
    let s = Rc::clone(&seen);
    hub.subscribe_image_changed(Rc::new(move |i, f| s.borrow_mut().push((i.and_then(tag_of), f))));
    hub.set_current_image(Some(test_image(3)), true);
    hub.set_current_image(None, false);
    assert_eq!(*seen.borrow(), [(Some(3), true), (None, false)]);
    assert_eq!(hub.image_change_count(), 2);
}

#[test]
fn trace_cap_drops_the_oldest() {
    let perf = FakePerfMarks::new(true);
    let hub = ShellPresentationHub::with_perf(perf.clone());
    hub.set_current_image(Some(test_image(1)), false);
    for token in 0..100 {
        hub.add_trace(token, "k", 0);
        assert!(hub.pending_trace_count() <= MAX_PENDING_TRACES);
    }
    assert_eq!(hub.pending_trace_count(), 16);
    hub.note_frame_presented(true);
    let rendered: Vec<i64> = perf
        .events()
        .iter()
        .filter(|(n, _)| n == "Rendered")
        .map(|(_, t)| *t)
        .collect();
    assert_eq!(rendered, (84..100).collect::<Vec<i64>>());
}

#[test]
fn disabled_perf_log_consumes_the_queue_without_emitting() {
    let perf = FakePerfMarks::new(false);
    let hub = ShellPresentationHub::with_perf(perf.clone());
    hub.add_trace(1, "k", 0);
    hub.add_trace(2, "k", 0);
    hub.note_frame_presented(true);
    assert_eq!(hub.pending_trace_count(), 0);
    assert!(perf.events().is_empty());
    perf.enabled.set(true);
    hub.note_frame_presented(true);
    assert!(perf.events().is_empty());
}

#[test]
fn emit_order_is_rendered_presented_rendered_frame() {
    let perf = FakePerfMarks::new(true);
    let hub = ShellPresentationHub::with_perf(perf.clone());
    hub.add_trace(1, "a", 0);
    hub.add_trace(2, "b", 0);
    hub.note_frame_presented(true);
    let expected: Vec<(&str, i64)> = vec![
        ("Rendered", 1),
        ("Presented", 1),
        ("RenderedFrame", 1),
        ("Rendered", 2),
        ("Presented", 2),
        ("RenderedFrame", 2),
    ];
    let got = perf.events();
    let got: Vec<(&str, i64)> = got.iter().map(|(n, t)| (n.as_str(), *t)).collect();
    assert_eq!(got, expected);
    assert_eq!(*perf.kinds.borrow(), ["a", "b"]);
}

#[test]
fn image_changed_handlers_run_in_subscription_order_and_may_unsubscribe() {
    let hub = Rc::new(ShellPresentationHub::with_perf(FakePerfMarks::new(true)));
    let order: Rc<RefCell<Vec<&'static str>>> = Rc::default();
    let first_id = Rc::new(Cell::new(0u64));
    let (o, h, id) = (Rc::clone(&order), Rc::clone(&hub), Rc::clone(&first_id));
    let first: ImageChangedHandler = Rc::new(move |_, _| {
        o.borrow_mut().push("first");
        h.unsubscribe_image_changed(id.get());
        // A handler may read the hub.
        assert!(h.current_image().is_some());
    });
    first_id.set(hub.subscribe_image_changed(first));
    let o = Rc::clone(&order);
    hub.subscribe_image_changed(Rc::new(move |_, _| o.borrow_mut().push("second")));
    hub.set_current_image(Some(test_image(1)), false);
    hub.set_current_image(Some(test_image(2)), false);
    assert_eq!(*order.borrow(), ["first", "second", "second"]);
    hub.unsubscribe_image_changed(9999);
}

#[test]
fn hub_and_sink_are_not_send() {
    trait AmbiguousIfSend<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
    // Compiles only when the type is NOT `Send` (a `Send` type makes `some_item` ambiguous).
    <ShellPresentationHub as AmbiguousIfSend<_>>::some_item();
    <Win32PresentationSink as AmbiguousIfSend<_>>::some_item();
}

struct Collect(Mutex<Vec<&'static str>>);

impl PerfListener for Collect {
    fn on_event_written(&self, event: &PerfEvent) {
        if let Some(PerfValue::Int(token)) = event.payload.first()
            && *token == 987_654_321
        {
            self.0.lock().unwrap().push(event.name);
        }
    }
}

#[test]
fn real_perf_marks_reach_the_perf_log() {
    let collect = Arc::new(Collect(Mutex::new(Vec::new())));
    let _subscription = PhotoReviewPerf::log().subscribe(collect.clone());
    let hub = ShellPresentationHub::with_perf(Rc::new(RealPerfMarks));
    hub.set_current_image(Some(test_image(1)), false);
    hub.add_trace(987_654_321, "full", 1);
    hub.note_frame_presented(true);
    assert_eq!(*collect.0.lock().unwrap(), ["Rendered", "Presented", "RenderedFrame"]);
}
