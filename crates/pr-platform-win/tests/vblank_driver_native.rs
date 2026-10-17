#![cfg(windows)]

#[allow(dead_code)] // This source inclusion exposes only ABI helpers and the ignored DWM smoke to this test crate.
#[path = "../src/vblank_driver.rs"]
mod vblank_driver;

use vblank_driver::{IVBlankDriver, NativeVBlankDriver};

#[test]
fn monitor_info_ex_w_has_windows_abi_size() {
    assert_eq!(NativeVBlankDriver::monitor_info_ex_size(), 104);
}

#[test]
fn dwm_timing_info_has_packed_windows_abi_size() {
    assert_eq!(NativeVBlankDriver::timing_info_size(), 292);
}

#[test]
fn device_name_conversion_stops_at_nul_and_limits_32_units() {
    assert_eq!(
        NativeVBlankDriver::device_name_from(&[b'D' as u16, b'I' as u16, 0, b'X' as u16]),
        "DI"
    );

    let full = vec![b'X' as u16; 40];
    assert_eq!(NativeVBlankDriver::device_name_from(&full), "X".repeat(32));

    let mut nul_at_limit = vec![b'Y' as u16; 40];
    nul_at_limit[31] = 0;
    assert_eq!(NativeVBlankDriver::device_name_from(&nul_at_limit), "Y".repeat(31));
}

#[test]
fn monitor_lookup_uses_nearest_monitor_flag() {
    assert_eq!(NativeVBlankDriver::monitor_default_to_nearest_flag(), 2);
}

#[test]
fn dwm_timing_maps_only_successful_nonzero_qpc_values() {
    assert!(NativeVBlankDriver::map_dwm_timing(None, 10, 5).is_none());
    assert!(NativeVBlankDriver::map_dwm_timing(Some(-1), 10, 5).is_none());
    assert!(NativeVBlankDriver::map_dwm_timing(Some(0), 0, 5).is_none());
    assert!(NativeVBlankDriver::map_dwm_timing(Some(0), 10, 0).is_none());
    assert_eq!(
        NativeVBlankDriver::map_dwm_timing(Some(0), 12_345, 167_000)
            .map(|grid| (grid.last_vblank, grid.refresh_period)),
        Some((12_345, 167_000))
    );
}

#[test]
#[ignore = "native: requires Windows DWM"]
// C#: DwmTiming_Static_ReturnsNullOrAPositiveGrid
fn dwm_timing_static_returns_null_or_a_positive_grid() {
    let timing = NativeVBlankDriver::INSTANCE.dwm_timing();
    assert!(timing.is_none_or(|grid| grid.refresh_period > 0));
}
