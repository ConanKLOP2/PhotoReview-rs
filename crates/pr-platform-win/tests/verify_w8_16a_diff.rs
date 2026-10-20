//! Adversarial verification tests for W8-16a (NOT committed). Each asserts the C# d92d6aae behaviour (oracle-confirmed).
use pr_platform_win::shell_viewing::frame_log::FrameLog;
use pr_platform_win::shell_viewing::frame_stats::FrameStats;

// C# MaxPresentMs starts at 0.0 and uses Math.Max(max, sample): Math.Max(0.0, -5.0) == 0.0.
#[test]
fn max_present_ms_floors_at_zero_with_negative_samples() {
    let mut s = FrameStats::new(1000);
    s.record(0, -1000); // present = -1000 ms
    assert_eq!(s.max_present_ms(), 0.0);
}

// C# `new StreamWriter(path, append: true)` opens with FileShare.Read: a second writer on the same file throws IOException,
// so FromEnvironment gives null and the constructor throws.
#[test]
fn second_open_of_same_file_fails_like_file_share_read() {
    let p = std::env::temp_dir().join(format!("pr-w8-16a-verify-{}.log", std::process::id()));
    let first = FrameLog::open(&p, 10_000_000).unwrap();
    let second = FrameLog::open(&p, 10_000_000);
    let ok = second.is_err();
    drop(second);
    drop(first);
    let _ = std::fs::remove_file(&p);
    assert!(ok, "C# FileShare.Read forbids a second writer");
}

// C#: while the log is open, File.Delete throws IOException (sharing violation).
#[test]
fn delete_while_open_is_refused_like_file_share_read() {
    let p = std::env::temp_dir().join(format!("pr-w8-16a-verify-del-{}.log", std::process::id()));
    let first = FrameLog::open(&p, 10_000_000).unwrap();
    let del = std::fs::remove_file(&p);
    drop(first);
    let _ = std::fs::remove_file(&p);
    assert!(del.is_err(), "C# FileShare.Read (no FileShare.Delete) refuses delete");
}
