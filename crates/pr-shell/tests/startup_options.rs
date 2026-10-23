//! W8-17d shell defaults and panic boundary; no window, GPU, or process-environment mutation.
#![cfg(windows)]
use pr_app::startup::options::EnvSource;
use pr_shell::startup::{
    error::{StartupError, catch_step},
    options::ShellStartupOptions,
};
struct Env;
impl EnvSource for Env {
    fn var(&self, name: &str) -> Option<String> {
        match name {
            "PHOTOREVIEW_RS_INSTANCE_PREFIX" => Some("  test-prefix  ".into()),
            "PHOTOREVIEW_RS_WARP" | "PHOTOREVIEW_RS_NOWARM" => Some("1".into()),
            _ => None,
        }
    }
}
#[test]
fn shell_startup_options_use_environment_and_leave_every_optional_seam_empty() {
    // C#: ShellStartupOptions initializers, shell-startup-options oracle.
    let options = ShellStartupOptions::from_env(&Env);
    assert_eq!(options.instance_name_prefix, "  test-prefix  ");
    assert!(options.prefer_warp);
    assert!(!options.warm_graphics);
    assert!(options.activate && options.install_process_hooks && options.start_perf_listener);
    assert_eq!(options.smoke_timeout.as_secs(), 30);
    assert!(options.graph_builder.is_none() && options.mark.is_none() && options.journal_recovery.is_none());
    assert!(options.on_main_window_ready.is_none() && options.set_cloaked.is_none());
    assert!(options.windowing.is_none() && options.keep_warm.is_none() && options.frame_log_path.is_none());
    assert!(options.keep_warm_work.is_none() && options.keep_warm_timestamp.is_none());
    assert!(options.services.secondary_port.is_none() && options.services.dialogs.is_none());
    assert!(options.services.file_dialog.is_none() && options.services.folder_dialog.is_none());
}
#[test]
fn catch_step_preserves_success_and_errors_and_describes_string_and_unknown_panics() {
    // New: Rust replacement for ShellStartup.RunStartupGuardedAsync's exception boundary.
    assert_eq!(catch_step(|| Ok(7)).unwrap(), 7);
    assert!(
        matches!(catch_step::<()>(|| Err(StartupError::Graph("graph failed".into()))), Err(StartupError::Graph(m)) if m == "graph failed")
    );
    for (result, expected) in [
        (catch_step::<()>(|| panic!("borrow fault")), "borrow fault"),
        (
            catch_step::<()>(|| std::panic::panic_any(String::from("owned"))),
            "owned",
        ),
        (
            catch_step::<()>(|| std::panic::panic_any(42)),
            "unknown exception object",
        ),
    ] {
        assert!(matches!(result, Err(StartupError::Panicked(m)) if m == expected));
    }
}
