//! Tests of W8-13g (N): `MainWindow` core (options, fields, constructor, `register_features`, `attach_renderer`, `open_initial`, the UI-thread registry
//! and the local poster). No C# test targets lines 1-226 of `ShellMainWindow.cs` directly (the window is only built by the startup harness, W8-17e), so
//! these are hand-written on `common/main_window_fixture.rs`; each native test names the `ShellStartupTests` assertion it is the window-level twin of.
//!
//! The pure tests need no window. The native tests are `#[ignore = "native: needs desktop"]`: a real UI thread with a `Win32UiDispatcher` and a hidden
//! (never shown, never activated) shell window with a WARP surface. Every wait is bounded; no sleeps.
//!
//! NOTE for the lead: the native tests build a whole `MainWindow`, whose constructor also runs the stubs of 13i (`FrameState`, scroll bars), 13j
//! (`ViewingState`, `initialize_viewing`) and 13k (`WindowingState`, `register_windowing`). They run as soon as those units are merged.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/main_window_fixture.rs"]
mod main_window_fixture;
#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::channel;

use main_window_fixture::{
    BuildOptions, FlakyHook, RecordingFeature, WarpHook, build, dispatcher, finish, new_order, run, start_ui,
    take_built, wait_until,
};
use pr_app::services::render::GpuImageCache;
use pr_app::services::render::gpu_image::PrefetchStatus;
use pr_core::abstractions::UiPriority;
use pr_imaging::adaptive_preview::calculate_target_decode_box;
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::host::main_window::core::{
    DEFAULT_HEIGHT_DIP, DEFAULT_WIDTH_DIP, FeatureOptions, LocalPoster, MainWindow, MainWindowError, MainWindowOptions,
    PREVIEW_QUALITY_MULTIPLIER, UiLocalPoster, canvas_color,
};
use pr_platform_win::host::main_window::lifecycle::view_model_wake;
use pr_platform_win::render::device::RenderFault;

// ---- pure ---------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn feature_options_store_one_value_per_type() {
    #[derive(Debug, PartialEq)]
    struct A(i32);
    #[derive(Debug, PartialEq)]
    struct B(&'static str);

    let mut options = FeatureOptions::default();
    assert!(options.get::<A>().is_none());
    options.insert(A(1));
    options.insert(B("b"));
    assert_eq!(*options.get::<A>().unwrap(), A(1));
    assert_eq!(*options.get::<B>().unwrap(), B("b"));
    // The same type again replaces the value.
    options.insert(A(2));
    assert_eq!(*options.get::<A>().unwrap(), A(2));
}

#[test]
fn feature_options_take_hands_a_value_out_once_and_refuses_a_shared_one() {
    struct OneShot(Box<dyn FnOnce() -> i32>);

    let mut options = FeatureOptions::default();
    options.insert(OneShot(Box::new(|| 7)));
    let value = options.take::<OneShot>().expect("the value is there");
    assert_eq!((value.0)(), 7);
    assert!(options.take::<OneShot>().is_none(), "taken once");
    assert!(options.get::<OneShot>().is_none());

    // A value that `get` shared stays in the bag until the shared handle is gone.
    options.insert(OneShot(Box::new(|| 8)));
    let shared = options.get::<OneShot>().unwrap();
    assert!(options.take::<OneShot>().is_none());
    drop(shared);
    assert!(options.take::<OneShot>().is_some());
}

// C#: ShellMainWindowOptions defaults (PreferWarp = false, PlacementFile = null, Activate = true)
#[test]
fn main_window_options_default_matches_csharp() {
    let options = MainWindowOptions::default();
    assert!(!options.prefer_warp);
    assert!(options.placement_file.is_none());
    assert!(options.activate);
    assert!(options.windowing.is_none());
    assert!(options.extensions.get::<u8>().is_none());
}

// C#: CanvasColor = new ColorF(0x10 / 255f, 0x10 / 255f, 0x10 / 255f, 1f)
#[test]
fn canvas_color_is_dark_canvas() {
    let color = canvas_color();
    let sixteenth = 16.0_f32 / 255.0_f32;
    assert_eq!(
        (color.r, color.g, color.b, color.a),
        (sixteenth, sixteenth, sixteenth, 1.0)
    );
}

// C#: DefaultWidthDip = 1200, DefaultHeightDip = 800, PreviewQualityMultiplier = 1.15
#[test]
fn constants_match_the_csharp_values() {
    assert_eq!(DEFAULT_WIDTH_DIP, 1200.0);
    assert_eq!(DEFAULT_HEIGHT_DIP, 800.0);
    assert_eq!(PREVIEW_QUALITY_MULTIPLIER, 1.15);
}

#[test]
fn main_window_error_texts() {
    assert_eq!(
        MainWindowError::Disposed.to_string(),
        "Cannot access a disposed object.\r\nObject name: 'PhotoReview.Shell.Win32.ShellMainWindow'."
    );
    assert_eq!(
        MainWindowError::RendererAlreadyAttached.to_string(),
        "The renderer is already attached."
    );
    assert_eq!(
        MainWindowError::WindowDestroyed.to_string(),
        "The main window is already destroyed."
    );
    assert_eq!(
        MainWindowError::FeatureNotRegistered { feature: "Scroll bars" }.to_string(),
        "Scroll bars are not registered."
    );
    assert_eq!(
        MainWindowError::FeatureNotRegistered { feature: "Input" }.to_string(),
        "Input is not registered."
    );
    assert_eq!(
        MainWindowError::FeatureNotRegistered {
            feature: "The context menu"
        }
        .to_string(),
        "The context menu is not registered."
    );
    let fault = RenderFault::InvalidOperation { message: "boom" };
    assert_eq!(MainWindowError::Render(fault).to_string(), fault.to_string());
}

// T2: the window is `!Send` (compile-time): `Send` would make the trait selection below ambiguous and the file would not compile.
#[test]
fn main_window_is_not_send() {
    trait AmbiguousIfSend<A> {
        fn probe() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
    <MainWindow as AmbiguousIfSend<_>>::probe();
}

// The poster is `!Send` (compile-time), as its doc says: it uses the thread-local queues of the thread that created it.
#[test]
fn ui_local_poster_is_not_send() {
    trait AmbiguousIfSend<A> {
        fn probe() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
    <UiLocalPoster as AmbiguousIfSend<_>>::probe();
}

// ---- native: the local poster and the wake -----------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs desktop"]
fn local_poster_runs_in_priority_then_fifo_order() {
    let ui = start_ui();
    let (tx, rx) = channel::<&'static str>();
    run(&ui, move || {
        let poster = UiLocalPoster::new(dispatcher());
        let job = |name: &'static str| -> Box<dyn FnOnce()> {
            let tx = tx.clone();
            // The job reaches `Rc` state in real use; the order is all this test looks at.
            let shared = Rc::new(name);
            Box::new(move || tx.send(*shared).unwrap())
        };
        poster.post(UiPriority::Background, job("background"));
        poster.post(UiPriority::Render, job("render-1"));
        poster.post(UiPriority::Normal, job("normal"));
        poster.post(UiPriority::Render, job("render-2"));
    });
    let seen: Vec<&str> = (0..4).map(|_| rx.recv_timeout(ui_thread::BOUND).unwrap()).collect();
    assert_eq!(seen, ["normal", "render-1", "render-2", "background"]);
    finish(ui);
}

#[test]
#[ignore = "native: needs desktop"]
fn local_poster_never_runs_a_job_inline() {
    let ui = start_ui();
    let ran_inline = run(&ui, || {
        let poster = UiLocalPoster::new(dispatcher());
        let ran = Rc::new(std::cell::Cell::new(false));
        let flag = Rc::clone(&ran);
        poster.post(UiPriority::Send, Box::new(move || flag.set(true)));
        ran.get()
    });
    assert!(!ran_inline);
    finish(ui);
}

#[test]
#[ignore = "native: needs desktop"]
fn post_after_runs_the_job_on_the_ui_thread_after_the_delay() {
    let ui = start_ui();
    let (tx, rx) = channel::<std::thread::ThreadId>();
    let ui_id = ui.thread_id();
    let started = std::time::Instant::now();
    run(&ui, move || {
        let poster = UiLocalPoster::new(dispatcher());
        let shared = Rc::new(());
        poster.post_after(
            std::time::Duration::from_millis(30),
            UiPriority::Normal,
            Box::new(move || {
                let _keep = shared;
                tx.send(std::thread::current().id()).unwrap();
            }),
        );
    });
    let ran_on = rx.recv_timeout(ui_thread::BOUND).unwrap();
    assert_eq!(ran_on, ui_id);
    assert!(started.elapsed() >= std::time::Duration::from_millis(30));
    finish(ui);
}

#[test]
#[ignore = "native: needs desktop"]
fn post_after_orders_by_due_time_then_fifo_for_equal_delays() {
    let ui = start_ui();
    let (tx, rx) = channel::<&'static str>();
    run(&ui, move || {
        let poster = UiLocalPoster::new(dispatcher());
        // Equal delays are scheduled consecutively; timer ordering uses due, then sequence.
        poster.post_after(
            std::time::Duration::from_millis(250),
            UiPriority::Normal,
            Box::new({
                let tx = tx.clone();
                move || tx.send("long").unwrap()
            }),
        );
        poster.post_after(
            std::time::Duration::from_millis(20),
            UiPriority::Normal,
            Box::new({
                let tx = tx.clone();
                move || tx.send("short").unwrap()
            }),
        );
        poster.post_after(
            std::time::Duration::from_millis(40),
            UiPriority::Normal,
            Box::new({
                let tx = tx.clone();
                move || tx.send("equal-1").unwrap()
            }),
        );
        poster.post_after(
            std::time::Duration::from_millis(40),
            UiPriority::Normal,
            Box::new(move || tx.send("equal-2").unwrap()),
        );
    });
    let deadline = std::time::Instant::now() + ui_thread::BOUND;
    let seen: Vec<_> = (0..4)
        .map(|_| {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            rx.recv_timeout(remaining)
                .expect("all delayed jobs run before deadline")
        })
        .collect();
    assert_eq!(seen, ["short", "equal-1", "equal-2", "long"]);
    finish(ui);
}

// "The wake is dropped while no window is registered, and the flag is cleared when the queued job ran."
#[test]
#[ignore = "native: needs desktop"]
fn coalesced_wake_posts_one_job_while_one_is_queued_and_drops_without_a_window() {
    let ui = start_ui();
    let queued_after_three = run(&ui, || {
        let wake = view_model_wake(dispatcher());
        // Three wakes: one queued job. The dispatcher counts OS wake messages, not items, so look at the queue state instead.
        wake();
        wake();
        wake();
        dispatcher().has_high_priority_work()
    });
    assert!(queued_after_three);
    // The queued job runs (no window: dropped) and the flag is clear again, so the next wake queues a new job.
    wait_until(&ui, "the queue to drain", || !dispatcher().has_high_priority_work());
    let queued_again = run(&ui, || {
        let wake = view_model_wake(dispatcher());
        wake();
        dispatcher().has_high_priority_work()
    });
    assert!(queued_again);
    finish(ui);
}

// ---- native: construction ---------------------------------------------------------------------------------------------------------------------

// C#: ShellMainWindow constructor order (lines 84-123): viewport, size accessor, viewer dpi, features, window events, hub, decode box, title.
#[test]
#[ignore = "native: needs desktop"]
fn constructor_wires_the_window_in_the_documented_order() {
    let ui = start_ui();
    type Seen = (Option<(f64, f64)>, Option<f64>, Option<DecodeBox>);
    let (title, order, size_at_hub, dpi_at_hub, box_at_hub, now) = run(&ui, move || {
        let seen: Rc<RefCell<Seen>> = Rc::new(RefCell::new((None, None, None)));
        let hook_seen = Rc::clone(&seen);
        let b = build(BuildOptions {
            on_hub_subscribe: Some(Box::new(move |probe| {
                *hook_seen.borrow_mut() = (
                    Some(probe.size_source.viewport_size()),
                    Some(probe.viewer.borrow().dpi_scale()),
                    Some(probe.size_source.target_decode_box()),
                );
            })),
            ..BuildOptions::default()
        });
        let state = *seen.borrow();
        (
            (b.title(), b.window.view_model().borrow().folder_title().to_owned()),
            b.order.borrow().clone(),
            state.0.unwrap(),
            state.1.unwrap(),
            state.2.unwrap(),
            (
                b.size_source.viewport_size(),
                b.size_source.target_decode_box(),
                b.shell.dpi_scale(),
            ),
        )
    });
    // 1. the viewport is built first, then (7.) the hub is subscribed.
    assert_eq!(&order[..2], ["viewport", "hub.subscribe"]);
    // 3. the size accessor was installed (and the initial size published) before the hub subscription ...
    assert_ne!(size_at_hub, (0.0, 0.0));
    assert_eq!(size_at_hub, now.0);
    // 4. ... the viewer had the window's dpi scale ...
    assert_eq!(dpi_at_hub, now.2);
    // 9. ... but the decode box (step 9) was not computed yet, and is afterwards.
    assert_eq!(box_at_hub, DecodeBox::UNBOUNDED);
    assert_ne!(now.1, DecodeBox::UNBOUNDED);
    // 10. the title is the view model's folder title.
    assert_eq!(title.0, title.1);
    finish(ui);
}

// D-W8B4M-12: the viewport is `!Send`; worker threads read the published size.
#[test]
#[ignore = "native: needs desktop"]
fn constructor_publishes_the_viewport_size_for_worker_threads() {
    let ui = start_ui();
    let (from_worker, from_ui) = run(&ui, || {
        let b = build(BuildOptions::default());
        let source = Arc::clone(&b.size_source);
        let from_worker = std::thread::spawn(move || source.viewport_size()).join().unwrap();
        (from_worker, b.window.viewport().viewport_size())
    });
    assert_ne!(from_worker, (0.0, 0.0));
    assert_eq!(from_worker, from_ui);
    finish(ui);
}

// T1: no strong `Rc` of the window state inside anything the window stores or registers elsewhere.
#[test]
#[ignore = "native: needs desktop"]
fn window_drop_releases_every_subscription() {
    let ui = start_ui();
    let (counts, order, hub_subscribers_after, shell_destroyed) = run(&ui, || {
        let b = build(BuildOptions::default());
        let feature_log = new_order();
        b.window
            .add_feature(RecordingFeature::new("f", Rc::clone(&feature_log)));
        let counts_alive = (b.window.strong_count(), b.hub.subscriber_count());
        b.window
            .attach_renderer(Some(&WarpHook::new(10, 10)))
            .expect("attach (WARP)");
        let count_after_attach = b.window.strong_count();
        let hub = Rc::clone(&b.hub);
        let shell = Rc::clone(&b.shell);
        drop(b); // the thread-local still holds one `Rc<Built>`
        let built = take_built();
        drop(built);
        (
            (counts_alive, count_after_attach),
            feature_log.borrow().clone(),
            hub.subscriber_count(),
            shell.is_destroyed(),
        )
    });
    assert_eq!(counts.0, (1, 1), "one owner, one hub subscription");
    assert_eq!(counts.1, 1, "attach_renderer adds no strong reference");
    assert_eq!(order, ["f:dispose", "f:release_renderer", "f:drop"]);
    assert_eq!(hub_subscribers_after, 0);
    assert!(shell_destroyed);
    finish(ui);
}

// ---- native: attach_renderer ------------------------------------------------------------------------------------------------------------------

// C#: ShellStartupTests.Startup_WithJpeg_OpensWindowThenDrawsFirstImageThenClosesCleanly (the window-level half: the surface and the cache exist)
#[test]
#[ignore = "native: needs desktop"]
fn attach_renderer_creates_surface_cache_and_marks() {
    let ui = start_ui();
    let (marks, has_surface, has_cache, loaded_after) = run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        (
            b.marks.borrow().clone(),
            b.window.has_surface(),
            b.window.image_cache().is_some(),
            b.shell.is_loaded(),
        )
    });
    assert_eq!(marks, ["d3dDeviceReady", "swapChainReady"]);
    assert!(has_surface);
    assert!(has_cache);
    // C#: `_surfaceReady` starts true, so `IsLoaded` is true as soon as the window exists; `SetSurfaceReady(true)` keeps it so.
    assert!(loaded_after);
    finish(ui);
}

#[test]
#[ignore = "native: needs desktop"]
fn attach_renderer_resizes_a_surface_whose_size_differs() {
    let ui = start_ui();
    let (client, surface_size) = run(&ui, || {
        let b = build(BuildOptions::default());
        b.window.attach_renderer(Some(&WarpHook::new(10, 10))).expect("attach");
        let size = b
            .window
            .with_surface(|surface| (surface.pixel_width(), surface.pixel_height()))
            .expect("a surface");
        (b.shell.client_size_pixels(), size)
    });
    assert!(client.0 > 10 && client.1 > 10);
    assert_eq!(surface_size, client);
    finish(ui);
}

#[test]
#[ignore = "native: needs desktop"]
fn attach_renderer_forwards_prefer_warp_to_surface_factory() {
    for prefer_warp in [true, false] {
        let ui = start_ui();
        let observed = run(&ui, move || {
            let mut options = BuildOptions::default();
            options.options.prefer_warp = prefer_warp;
            let b = build(options);
            let hook = WarpHook::new(10, 10);
            b.window.attach_renderer(Some(&hook)).expect("attach");
            hook.options.borrow()[0].prefer_warp
        });
        assert_eq!(observed, prefer_warp);
        finish(ui);
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn attach_renderer_resizes_for_each_dimension_mismatch_and_skips_equal_or_zero_client() {
    #[derive(Clone, Copy)]
    enum Case {
        WidthDiffers,
        HeightDiffers,
        BothDiffer,
        EqualAtChangedDpi,
    }

    for case in [
        Case::WidthDiffers,
        Case::HeightDiffers,
        Case::BothDiffer,
        Case::EqualAtChangedDpi,
    ] {
        let ui = start_ui();
        let (client, surface, window_dpi, factory_options) = run(&ui, move || {
            let build_options = BuildOptions::default();
            let b = build(build_options);
            if matches!(case, Case::EqualAtChangedDpi) {
                crate::shell_window_fixture::send_dpi_changed(
                    b.shell.hwnd(),
                    192,
                    crate::shell_window_fixture::window_rect(b.shell.hwnd()),
                );
            }
            let client = b.shell.client_size_pixels();
            let surface_size = match case {
                Case::WidthDiffers => (client.0 - 1, client.1),
                Case::HeightDiffers => (client.0, client.1 - 1),
                Case::BothDiffer => (client.0 - 1, client.1 - 1),
                Case::EqualAtChangedDpi => client,
            };
            let hook = WarpHook::new(surface_size.0, surface_size.1);
            b.window.attach_renderer(Some(&hook)).expect("attach");
            let surface = b
                .window
                .with_surface(|surface| (surface.pixel_width(), surface.pixel_height(), surface.dpi_scale()))
                .expect("surface stored");
            (
                client,
                surface,
                b.shell.dpi_scale(),
                hook.options.borrow()[0].prefer_warp,
            )
        });

        match case {
            Case::WidthDiffers => {
                assert_eq!(client.0, surface.0);
                assert_eq!(client.1, surface.1);
                assert_eq!(surface.2, window_dpi);
            }
            Case::HeightDiffers => {
                assert_eq!(client.0, surface.0);
                assert_eq!(client.1, surface.1);
                assert_eq!(surface.2, window_dpi);
            }
            Case::BothDiffer => {
                assert_eq!(client.0, surface.0);
                assert_eq!(client.1, surface.1);
                assert_eq!(surface.2, window_dpi);
            }
            Case::EqualAtChangedDpi => {
                assert!(client.0 > 0 && client.1 > 0);
                assert_eq!((surface.0, surface.1), client);
                assert_eq!(window_dpi, 2.0);
                assert_eq!(surface.2, 1.0, "equal dimensions must not trigger resize");
            }
        }
        assert!(factory_options, "fixture defaults to prefer_warp=true");
        finish(ui);
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn attach_renderer_twice_is_an_error_and_keeps_the_first() {
    let ui = start_ui();
    let (second, calls, still_attached) = run(&ui, || {
        let b = build(BuildOptions::default());
        let hook = WarpHook::new(10, 10);
        b.window.attach_renderer(Some(&hook)).expect("attach");
        let cache = b.window.image_cache().expect("cache");
        let second = b.window.attach_renderer(Some(&hook));
        let same = b.window.image_cache().is_some_and(|again| Rc::ptr_eq(&again, &cache));
        (second, hook.calls.get(), same && b.window.has_surface())
    });
    assert_eq!(second, Err(MainWindowError::RendererAlreadyAttached));
    assert_eq!(calls, 1, "the hook is not asked again");
    assert!(still_attached);
    finish(ui);
}

#[test]
#[ignore = "native: needs desktop"]
fn attach_renderer_after_dispose_is_an_error() {
    let ui = start_ui();
    let (result, calls) = run(&ui, || {
        let b = build(BuildOptions::default());
        b.window.dispose();
        let hook = WarpHook::new(10, 10);
        (b.window.attach_renderer(Some(&hook)), hook.calls.get())
    });
    assert_eq!(result, Err(MainWindowError::Disposed));
    assert_eq!(calls, 0);
    finish(ui);
}

#[test]
#[ignore = "native: needs desktop"]
fn attach_renderer_with_a_failing_hook_stores_nothing_and_allows_a_retry() {
    let ui = start_ui();
    let (first, state_after_failure, second, state_after_retry) = run(&ui, || {
        let b = build(BuildOptions::default());
        let hook = FlakyHook {
            failures: std::cell::Cell::new(1),
            width: 10,
            height: 10,
        };
        let first = b.window.attach_renderer(Some(&hook));
        let state_after_failure = (
            b.window.has_surface(),
            b.window.image_cache().is_some(),
            b.marks.borrow().len(),
        );
        let second = b.window.attach_renderer(Some(&hook));
        let state_after_retry = (b.window.has_surface(), b.window.image_cache().is_some());
        (first, state_after_failure, second, state_after_retry)
    });
    assert_eq!(
        first,
        Err(MainWindowError::Render(RenderFault::InvalidOperation {
            message: "injected surface failure"
        }))
    );
    assert_eq!(state_after_failure, (false, false, 0), "nothing stored, no marks");
    assert_eq!(second, Ok(()));
    assert_eq!(state_after_retry, (true, true));
    finish(ui);
}

// D-W8B3S-2: the stripe scheduler posts a Background job; it never uploads inline.
#[test]
#[ignore = "native: needs desktop"]
fn stripe_scheduler_never_runs_inline_and_runs_on_the_ui_thread_at_background() {
    let ui = start_ui();
    let ui_id = ui.thread_id();
    let (done_tx, done_rx) = channel::<(std::thread::ThreadId, Vec<&'static str>)>();
    let (inline, id) = run(&ui, move || {
        let b = build(BuildOptions::default());
        b.window.attach_renderer(Some(&WarpHook::new(64, 64))).expect("attach");
        let cache = b.window.image_cache().expect("cache");
        let order: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&order);
        let done = Rc::clone(&order);
        let _subscription = cache.subscribe_upload_completed(Rc::new(move |_id| {
            done.borrow_mut().push("stripe");
            let _ = done_tx.send((std::thread::current().id(), done.borrow().clone()));
        }));
        // One stripe covers a 64x64 image: inline execution would complete the upload inside `prefetch`.
        let pixels = Arc::new(PixelBuffer::allocate(64, 64, PixelLayout::Pbgra32).unwrap());
        cache.prefetch(&pixels).expect("prefetch");
        let inline = cache.is_fully_uploaded(pixels.id());
        // A Render job posted AFTER the prefetch still runs before the Background stripe.
        let poster = UiLocalPoster::new(dispatcher());
        poster.post(
            UiPriority::Render,
            Box::new(move || recorded.borrow_mut().push("render")),
        );
        // Keep the buffer alive until the stripe ran (a sole owner would drop the entry).
        std::mem::forget(Arc::clone(&pixels));
        (inline, pixels.id())
    });
    let (stripe_thread, order) = done_rx.recv_timeout(ui_thread::BOUND).unwrap();
    assert!(!inline, "the first stripe must not run inside prefetch");
    assert_eq!(stripe_thread, ui_id);
    assert_eq!(order, ["render", "stripe"]);
    assert!(id > 0);
    finish(ui);
}

// ---- native: open_initial -------------------------------------------------------------------------------------------------------------------------

fn expected_box_for_client(client: (i32, i32), dpi: f64) -> DecodeBox {
    let (w, h) = (f64::from(client.0) / dpi, f64::from(client.1) / dpi);
    calculate_target_decode_box(
        if w > 1.0 { w } else { 2200.0 },
        if h > 1.0 { h } else { 1400.0 },
        dpi,
        1.15,
    )
    .unwrap()
}

// C#: OpenInitialAsync lines 193-198 (the decode box comes from the REAL client area)
#[test]
#[ignore = "native: needs desktop"]
fn open_initial_sets_the_decode_box_from_the_real_client_area() {
    let ui = start_ui();
    let (after, expected, marks) = run(&ui, || {
        let b = build(BuildOptions::default());
        b.size_source.set_target_decode_box(DecodeBox::new(1, 1));
        b.window.open_initial(None, false);
        (
            b.size_source.target_decode_box(),
            expected_box_for_client(b.shell.client_size_pixels(), b.shell.dpi_scale()),
            b.marks.borrow().clone(),
        )
    });
    assert_eq!(after, expected);
    assert!(marks.is_empty(), "a missing path stops after the decode box");
    finish(ui);
}

// C#: OpenInitialAsync lines 207-213 (earlyDecodeBoxMatched / earlyDecodeBoxMismatch)
#[test]
#[ignore = "native: needs desktop"]
fn open_initial_marks_match_and_mismatch_of_the_startup_prediction() {
    let ui = start_ui();
    let (matched_marks, mismatched_marks) = run(&ui, || {
        let b = build(BuildOptions::default());
        let file = b.photos.join("a.jpg");
        std::fs::create_dir_all(&b.photos).unwrap();
        std::fs::write(&file, b"x").unwrap();
        let path = file.to_string_lossy().into_owned();
        let expected = expected_box_for_client(b.shell.client_size_pixels(), b.shell.dpi_scale());

        b.size_source.set_startup_prediction(Some(expected));
        b.window.open_initial(Some(&path), true);
        let matched = b.marks.borrow().clone();
        b.marks.borrow_mut().clear();

        b.size_source
            .set_startup_prediction(Some(DecodeBox::new(expected.width + 1, expected.height)));
        b.window.open_initial(Some(&path), true);
        (matched, b.marks.borrow().clone())
    });
    assert_eq!(&matched_marks[..2], ["openPathBegin", "earlyDecodeBoxMatched"]);
    assert_eq!(&mismatched_marks[..2], ["openPathBegin", "earlyDecodeBoxMismatch"]);
    finish(ui);
}

// C#: `string.IsNullOrWhiteSpace(initialPath)` returns before any mark
#[test]
#[ignore = "native: needs desktop"]
fn open_initial_with_a_blank_path_does_nothing_more() {
    let ui = start_ui();
    let (marks, deferred) = run(&ui, || {
        let b = build(BuildOptions::default());
        for blank in [None, Some(""), Some("   "), Some("\t\r\n")] {
            b.window.open_initial(blank, true);
        }
        let deferred = b.window.view_model().borrow().presenter().defer_next_preload_kick();
        (b.marks.borrow().clone(), deferred)
    });
    assert!(marks.is_empty());
    assert!(!deferred);
    finish(ui);
}

// C#: `Presenter.DeferNextPreloadKick = true` before `OpenPathAsync` (P-1)
#[test]
#[ignore = "native: needs desktop"]
fn open_initial_defers_the_preload_kick_before_opening() {
    let ui = start_ui();
    let (before, after, marks) = run(&ui, || {
        let b = build(BuildOptions::default());
        let file = b.photos.join("b.jpg");
        std::fs::create_dir_all(&b.photos).unwrap();
        std::fs::write(&file, b"x").unwrap();
        let before = b.window.view_model().borrow().presenter().defer_next_preload_kick();
        b.window.open_initial(Some(&file.to_string_lossy()), false);
        let after = b.window.view_model().borrow().presenter().defer_next_preload_kick();
        (before, after, b.marks.borrow().clone())
    });
    assert!(!before);
    assert!(after, "set before the open; the first frame clears it");
    assert_eq!(
        marks,
        ["openPathBegin"],
        "a folder open (is_file = false) has no prediction marks"
    );
    finish(ui);
}

// Features: `with_feature` finds the registered feature by its concrete type (D-W8B4X-9).
#[test]
#[ignore = "native: needs desktop"]
fn with_feature_finds_a_feature_by_its_concrete_type() {
    let ui = start_ui();
    let (found, other) = run(&ui, || {
        let b = build(BuildOptions::default());
        b.window.add_feature(RecordingFeature::new("one", new_order()));
        let found = b.window.with_feature::<RecordingFeature, _>(|feature| feature.name);
        let other = b.window.with_feature::<String, _>(|_| ());
        (found, other)
    });
    assert_eq!(found, Some("one"));
    assert_eq!(other, None);
    finish(ui);
}
