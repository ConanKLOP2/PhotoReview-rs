//! W8-17d: environment rules without changing the process environment.
use pr_app::startup::options::*;
use serde_json::Value;

struct Env(Value);
impl EnvSource for Env {
    fn var(&self, name: &str) -> Option<String> {
        self.0
            .get(name.replace("PHOTOREVIEW_RS_", "PHOTOREVIEW_SHELL_"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    }
}

#[test]
fn options_match_csharp_golden() {
    // C#: ShellStartupOptions property initializers; shell-startup-options oracle.
    let golden = pr_test_support::golden::load("shell-startup-options");
    let mut count = 0;
    for case in golden.cases.iter().filter(|c| c["kind"] == "options") {
        let actual = EnvOptions::from_env(&Env(case["env"].clone()));
        let expected = case["instance_name_prefix"].as_str().unwrap();
        assert_eq!(
            actual.instance_name_prefix,
            if expected == "PhotoReviewShell" {
                "PhotoReviewRs"
            } else {
                expected
            },
            "{}",
            case["id"]
        );
        assert_eq!(actual.prefer_warp, case["prefer_warp"].as_bool().unwrap());
        assert_eq!(actual.warm_graphics, case["warm_graphics"].as_bool().unwrap());
        count += 1;
    }
    assert_eq!(count, 14);
}

#[test]
fn constants_match_the_startup_contract() {
    // C#: ShellStartup constants (ShellStartup.cs).
    assert_eq!((EXIT_STARTUP_FAILED, EXIT_SMOKE_TIMEOUT, MAX_LOOP_FAULTS), (1, 7, 50));
    assert_eq!(EXPLORER_PREFETCH_TIMEOUT.as_millis(), 3000);
    assert_eq!(DEFAULT_SMOKE_TIMEOUT.as_millis(), 30000);
}

#[test]
fn process_env_reads_an_unset_variable_as_none() {
    // New: read-only check of ProcessEnv, never mutates the environment.
    assert_eq!(ProcessEnv.var("PHOTOREVIEW_RS_TEST_UNSET_W8_17D_6E99431B"), None);
}
