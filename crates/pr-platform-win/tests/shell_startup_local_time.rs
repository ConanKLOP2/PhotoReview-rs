#![cfg(windows)]
use pr_platform_win::shell_startup::local_time::local_time_parts;

#[test]
fn local_time_parts_are_plausible() {
    // New: GetLocalTime adapter used by the C# PerfCsvListener timestamp filename.
    let now = local_time_parts();
    assert!((2000..=9999).contains(&now.year));
    assert!((1..=12).contains(&now.month));
    assert!((1..=31).contains(&now.day));
    assert!(now.hour < 24 && now.minute < 60 && now.second < 60);
}
