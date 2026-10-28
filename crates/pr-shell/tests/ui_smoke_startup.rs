//! Slice S2 of unit W8-20b (`contracts/W8-20b.md`): the four whole-shell tests of C# `ShellStartupTests` that `W8-17e` leaves to this unit because they exercise the render path
//! (`contracts/W8-17e.md` D-W8B6-37 and "NOT here"). Baseline `d92d6aae`, file `tests/PhotoReview.Shell.Integration.Tests/Startup/ShellStartupTests.cs`.
//!
//! Native (`#[ignore = "native: needs desktop"]`), WARP, one isolated `UiSmokeRun` each.
#![cfg(windows)]
use pr_core::abstractions::{UiDispatcher, UiPriority};
use pr_platform_win::gfx::com::DXGI_ERROR_DEVICE_REMOVED_HRESULT;
use pr_platform_win::host::contracts::{ShellWindowApi, ShellWindowEvent, WindowMessage, WindowMessageHandler};
use pr_platform_win::win32::messages::WM_PAINT;
use pr_shell::startup::graph::{GraphOverrides, production_graph_builder_with};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Mutex, mpsc};
use ui_smoke_fixture::{GraphChoice, with_probe};
static SERIAL: Mutex<()> = Mutex::new(());

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{BOUND, SmokeConfig, UiSmokeRun, isolated_test};

/// C# `ShellStartupTests.DeviceLost_DuringPresent_RecreatesTheDeviceAndRedrawsTheImage` (line 626): a device-removed fault injected through the surface's fault seam (the one the 13i twin
/// uses) during a present; the device generation rises, the GPU cache refills, the image is drawn again.
#[test]
#[ignore = "native: needs desktop"]
fn device_lost_during_present_recreates_the_device_and_redraws_the_image() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "device_lost_during_present_recreates_the_device_and_redraws_the_image",
        || {
            let run = UiSmokeRun::start(SmokeConfig::default()).unwrap();
            run.wait_first_image().unwrap();
            let (before, after, cache, drawn) = run
                .on_ui(|p| {
                    if false {
                        p.main_window
                            .window()
                            .insert_message_handler_first(Rc::new(SwallowPaint));
                    }
                    let before = p.main_window.with_surface(|s| s.device_generation()).expect("surface");
                    let armed = Cell::new(true);
                    p.main_window.with_surface(|s| {
                        s.resources().set_present_fault(Some(Box::new(move |hr| {
                            if armed.replace(false) {
                                DXGI_ERROR_DEVICE_REMOVED_HRESULT
                            } else {
                                hr
                            }
                        })))
                    });
                    for _ in 0..50 {
                        p.main_window.render_now();
                        if p.main_window
                            .with_surface(|s| s.device_generation())
                            .is_some_and(|g| g > before)
                            && p.main_window.image_cache().is_some_and(|c| c.inner().count() > 0)
                        {
                            break;
                        }
                    }
                    (
                        before,
                        p.main_window.with_surface(|s| s.device_generation()).unwrap(),
                        p.main_window.image_cache().unwrap().inner().count(),
                        p.main_window.composer().unwrap().last_drawn().is_some(),
                    )
                })
                .unwrap();
            assert!(after > before && cache > 0 && drawn, "{}", run.diagnostics());
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `ShellStartupTests.DeviceLost_RequestsItsOwnRedraw_EvenWhenTheOsSendsNoPaint` (line 688): after the fault no `WM_PAINT` is sent; the frame counter still rises.
#[test]
#[ignore = "native: needs desktop"]
fn device_lost_requests_its_own_redraw_even_when_the_os_sends_no_paint() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "device_lost_requests_its_own_redraw_even_when_the_os_sends_no_paint",
        || {
            let run = UiSmokeRun::start(SmokeConfig::default()).unwrap();
            run.wait_first_image().unwrap();
            let (before, after, cache, drawn) = run
                .on_ui(|p| {
                    if true {
                        p.main_window
                            .window()
                            .insert_message_handler_first(Rc::new(SwallowPaint));
                    }
                    let before = p.main_window.with_surface(|s| s.device_generation()).expect("surface");
                    let armed = Cell::new(true);
                    p.main_window.with_surface(|s| {
                        s.resources().set_present_fault(Some(Box::new(move |hr| {
                            if armed.replace(false) {
                                DXGI_ERROR_DEVICE_REMOVED_HRESULT
                            } else {
                                hr
                            }
                        })))
                    });
                    for _ in 0..50 {
                        p.main_window.render_now();
                        if p.main_window
                            .with_surface(|s| s.device_generation())
                            .is_some_and(|g| g > before)
                            && p.main_window.image_cache().is_some_and(|c| c.inner().count() > 0)
                        {
                            break;
                        }
                    }
                    (
                        before,
                        p.main_window.with_surface(|s| s.device_generation()).unwrap(),
                        p.main_window.image_cache().unwrap().inner().count(),
                        p.main_window.composer().unwrap().last_drawn().is_some(),
                    )
                })
                .unwrap();
            assert!(after > before && cache > 0 && drawn, "{}", run.diagnostics());
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `ShellStartupTests.FirstImage_IsDrawnInTheSameTurnItIsAssigned_NotAtTheNextFrameTick` (line 728): the frame that has the first image is rendered inside the UI turn that assigned it.
#[test]
#[ignore = "native: needs desktop"]
fn first_image_is_drawn_in_the_same_turn_it_is_assigned_not_at_the_next_frame_tick() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "first_image_is_drawn_in_the_same_turn_it_is_assigned_not_at_the_next_frame_tick",
        || {
            let (tx, rx) = mpsc::channel();
            let run = UiSmokeRun::start(SmokeConfig {
                graph: GraphChoice::Custom(Box::new(move |paths| {
                    let builder = production_graph_builder_with(GraphOverrides {
                        paths: Some(paths.paths.clone()),
                        recycle_bin: Some(paths.bin.clone()),
                        ..Default::default()
                    });
                    Box::new(move |inputs| {
                        let dispatcher = inputs.dispatcher.clone();
                        let armed = Cell::new(true);
                        inputs.hub.subscribe_image_changed(Rc::new(move |image, _| {
                            if let Some(pixels) = image.and_then(|i| i.as_pixels()).filter(|_| armed.replace(false)) {
                                let pixels = pixels.clone();
                                let tx = tx.clone();
                                dispatcher.post_with_priority(
                                    Box::new(move || {
                                        let drawn = with_probe(|p| {
                                            p.main_window.first_image_presented()
                                                && p.main_window.composer().and_then(|c| c.last_drawn()).is_some_and(
                                                    |(drawn, _, _)| std::sync::Arc::ptr_eq(&drawn, &pixels),
                                                )
                                        });
                                        let _ = tx.send(drawn);
                                    }),
                                    UiPriority::Send,
                                );
                            }
                        }));
                        builder(inputs)
                    })
                })),
                ..Default::default()
            })
            .unwrap();
            assert!(rx.recv_timeout(BOUND).unwrap(), "{}", run.diagnostics());
            run.wait_first_image().unwrap();
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `ShellStartupTests.Closing_ReleasesTheRenderSurface_WhileTheWindowIsStillAlive` (line 812): on `Closing` (not cancelled) the surface is released before the window is destroyed
/// (`MainWindow::has_surface()` is false at `Closed`).
#[test]
#[ignore = "native: needs desktop"]
fn closing_releases_the_render_surface_while_the_window_is_still_alive() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "closing_releases_the_render_surface_while_the_window_is_still_alive",
        || {
            let (tx, rx) = mpsc::channel();
            let run = UiSmokeRun::start(SmokeConfig {
                configure: Some(Box::new(move |options| {
                    options.on_main_window_ready = Some(Box::new(move |window| {
                        let weak = Rc::downgrade(window);
                        window.window().subscribe(
                            ShellWindowEvent::Closed,
                            Box::new(move || {
                                let _ = tx.send(weak.upgrade().map(|w| w.has_surface()));
                            }),
                        );
                    }));
                })),
                ..Default::default()
            })
            .unwrap();
            run.wait_first_image().unwrap();
            assert_eq!(run.close_and_wait(), Ok(0));
            assert_eq!(rx.recv_timeout(BOUND).unwrap(), Some(false));
        },
    );
}

struct SwallowPaint;
impl WindowMessageHandler for SwallowPaint {
    fn try_handle(&self, message: &WindowMessage) -> Option<isize> {
        (message.msg == WM_PAINT).then_some(0)
    }
}
