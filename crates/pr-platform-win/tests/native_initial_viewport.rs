#![cfg(windows)]
use pr_app::services::initial_viewport::{MonitorId, MonitorProbe, PixelRect, predict_decode_box};
use pr_imaging::adaptive_preview::calculate_target_decode_box;
use pr_platform_win::host::main_window::core::{DEFAULT_HEIGHT_DIP, DEFAULT_WIDTH_DIP, PREVIEW_QUALITY_MULTIPLIER};
use pr_platform_win::shell_startup::monitor_probe::Win32MonitorProbe;
use pr_platform_win::win32::{
    api::{shcore, user32},
    structs::{Point, Rect, new_monitor_info},
};

#[test]
#[ignore = "native: needs desktop"]
fn probe_matches_c_default_size() {
    // C#: InitialViewportPredictorTests.PredictDecodeBox_DefaultSize_IsQuantisedBox (native twin).
    on_desktop(check_default_size);
}

fn check_default_size() {
    let probe = Win32MonitorProbe;
    // Independent OS facts: using probe.dpi_for/frame_insets for the expectation would mask adapter bugs.
    let monitor = user32::monitor_from_point(Point { x: 0, y: 0 }, 1);
    assert_ne!(monitor, 0, "desktop primary monitor");
    assert_eq!(probe.monitor_primary(), Some(MonitorId(monitor)));
    let awareness = user32::get_awareness_from_dpi_awareness_context(user32::get_thread_dpi_awareness_context());
    let dpi = match awareness {
        0 => 96,
        2 => shcore::get_dpi_for_monitor(monitor, 0)
            .map(|(x, _)| x)
            .unwrap_or_else(|_| user32::get_dpi_for_system()),
        _ => user32::get_dpi_for_system(),
    };
    assert_eq!(probe.dpi_for(MonitorId(monitor)), dpi);
    let mut insets = Rect::default();
    user32::adjust_window_rect_ex_for_dpi(&mut insets, 0x00CF_0000, false, 0, dpi).unwrap();
    assert_eq!(
        probe.frame_insets(dpi),
        Some(PixelRect {
            left: insets.left,
            top: insets.top,
            right: insets.right,
            bottom: insets.bottom
        })
    );
    let mut info = new_monitor_info();
    user32::get_monitor_info(monitor, &mut info).unwrap();
    assert_eq!(
        probe.monitor_work_area(MonitorId(monitor)),
        Some(PixelRect {
            left: info.rcWork.left,
            top: info.rcWork.top,
            right: info.rcWork.right,
            bottom: info.rcWork.bottom
        })
    );
    let scale = f64::from(dpi) / 96.0;
    let width = (DEFAULT_WIDTH_DIP * scale).round_ties_even() as i32 - (insets.right - insets.left);
    let height = (DEFAULT_HEIGHT_DIP * scale).round_ties_even() as i32 - (insets.bottom - insets.top);
    let expected = calculate_target_decode_box(
        f64::from(width) / scale,
        f64::from(height) / scale,
        scale,
        PREVIEW_QUALITY_MULTIPLIER,
    )
    .unwrap();
    assert_eq!(
        predict_decode_box(
            None,
            DEFAULT_WIDTH_DIP,
            DEFAULT_HEIGHT_DIP,
            PREVIEW_QUALITY_MULTIPLIER,
            &probe
        ),
        Some(expected)
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn probe_work_areas_are_not_empty_on_a_desktop() {
    // New: the native implementation of the W8-04c MonitorProbe seam.
    on_desktop(|| {
        let areas = Win32MonitorProbe.work_areas();
        assert!(!areas.is_empty());
        assert!(areas.iter().all(|a| a.right > a.left && a.bottom > a.top));
    });
}

fn on_desktop(run: impl FnOnce() + Send + 'static) {
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));
        tx.send(result).unwrap();
    });
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("bounded monitor probe");
    thread.join().unwrap();
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}
