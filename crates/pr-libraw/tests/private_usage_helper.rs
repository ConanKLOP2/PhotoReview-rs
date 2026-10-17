#![cfg(windows)]

#[test]
fn process_private_bytes_reports_nonzero_private_usage() {
    let private_bytes = pr_platform_win::test_support::process_private_bytes()
        .expect("GetProcessMemoryInfo should report process private usage");

    assert!(private_bytes > 0);
}
