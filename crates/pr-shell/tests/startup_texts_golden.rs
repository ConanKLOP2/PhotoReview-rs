//! ST-STARTUP-GOLDEN: the golden `shell-startup-texts` (W8-17e) and the texts/constants of `shell-startup-options` (W8-17d) loaded by name, plus the option/env
//! matrix of `ShellStartupOptions::from_env`. No window, GPU or process-environment mutation: the environment is a fake `EnvSource`.
#![cfg(windows)]
use pr_app::startup::forwarded_open::FORWARD_COALESCE_WINDOW;
use pr_app::startup::options::*;
use pr_app::startup::recovery::recovery_prompt_texts;
use pr_app::startup::shutdown::close_secondary_windows_unless_cancelled;
use pr_l10n::localizer::built_in_english_localizer;
use pr_l10n::template::LocArg;
use pr_shell::startup::options::ShellStartupOptions;
use pr_test_support::golden;
use std::collections::HashMap;

struct MapEnv(HashMap<String, String>);
impl EnvSource for MapEnv {
    fn var(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

const PREFIX: &str = "PHOTOREVIEW_RS_INSTANCE_PREFIX";
const WARP: &str = "PHOTOREVIEW_RS_WARP";
const NOWARM: &str = "PHOTOREVIEW_RS_NOWARM";

fn env_of(pairs: &[(&str, &str)]) -> MapEnv {
    MapEnv(pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect())
}

#[test]
fn startup_failed_texts_match_csharp_golden() {
    // C#: Tr.AppStartupFailed(message) in ShellStartup; golden shell-startup-texts, kind startup_failed.
    let localizer = built_in_english_localizer();
    let golden = golden::load("shell-startup-texts");
    let mut count = 0;
    for case in golden.cases.iter().filter(|c| c["kind"] == "startup_failed") {
        let message = golden::str_of(case, "message");
        let actual = localizer.format("app.startupFailed", &[LocArg::new("message", message)]);
        assert_eq!(actual, golden::str_of(case, "text"), "{}", case["id"]);
        count += 1;
    }
    assert_eq!(count, 5);
}

#[test]
fn recovery_prompt_texts_match_csharp_golden() {
    // C#: Tr.DialogStartupRecoveryFailedTitle / Tr.DialogStartupRecoveryFailedMessage(count); golden kind recovery_prompt.
    let localizer = built_in_english_localizer();
    let golden = golden::load("shell-startup-texts");
    let mut count = 0;
    for case in golden.cases.iter().filter(|c| c["kind"] == "recovery_prompt") {
        let failed = usize::try_from(golden::i64_of(case, "count")).unwrap();
        let (title, message) = recovery_prompt_texts(&localizer, failed);
        assert_eq!(title, golden::str_of(case, "title"), "{}", case["id"]);
        assert_eq!(message, golden::str_of(case, "message"), "{}", case["id"]);
        count += 1;
    }
    assert_eq!(count, 7);
}

#[test]
fn closing_rows_match_csharp_golden() {
    // C#: ShellStartup.CloseSecondaryWindowsUnlessCancelled(e, close); golden kind closing.
    let golden = golden::load("shell-startup-texts");
    let mut count = 0;
    for case in golden.cases.iter().filter(|c| c["kind"] == "closing") {
        let mut closed = false;
        close_secondary_windows_unless_cancelled(golden::bool_of(case, "cancel"), &mut || closed = true);
        assert_eq!(closed, golden::bool_of(case, "closed"), "{}", case["id"]);
        count += 1;
    }
    assert_eq!(count, 2);
}

#[test]
fn golden_has_only_the_three_known_case_kinds() {
    // Guards against a regenerated golden that adds a kind this file would silently skip.
    let golden = golden::load("shell-startup-texts");
    assert_eq!(golden.cases.len(), 14);
    for case in &golden.cases {
        let kind = golden::str_of(case, "kind");
        assert!(
            ["startup_failed", "recovery_prompt", "closing"].contains(&kind),
            "{case}"
        );
    }
}

#[test]
fn second_instance_texts_and_constants_match_options_golden() {
    // C#: Tr.FolderAlreadyOpenInOtherInstance, Tr.AppAlreadyRunningNoResponse, Tr.AppTitle and the ShellStartup constants; golden shell-startup-options kinds text/constants.
    let localizer = built_in_english_localizer();
    let golden = golden::load("shell-startup-options");
    let texts: HashMap<&str, String> = [
        (
            "folder-already-open",
            localizer.get("folder.alreadyOpenInOtherInstance"),
        ),
        ("app-already-running", localizer.get("app.alreadyRunningNoResponse")),
        ("app-title", localizer.get("app.title")),
    ]
    .into_iter()
    .collect();
    let mut text_rows = 0;
    for case in golden.cases.iter().filter(|c| c["kind"] == "text") {
        assert_eq!(
            texts[golden::str_of(case, "id")],
            golden::str_of(case, "text"),
            "{}",
            case["id"]
        );
        text_rows += 1;
    }
    assert_eq!(text_rows, 3);
    let constants = golden
        .cases
        .iter()
        .find(|c| c["kind"] == "constants")
        .expect("constants row");
    assert_eq!(
        i64::from(EXIT_STARTUP_FAILED),
        golden::i64_of(constants, "exit_startup_failed")
    );
    assert_eq!(
        i64::from(EXIT_SMOKE_TIMEOUT),
        golden::i64_of(constants, "exit_smoke_timeout")
    );
    assert_eq!(i64::from(MAX_LOOP_FAULTS), golden::i64_of(constants, "max_loop_faults"));
    assert_eq!(
        EXPLORER_PREFETCH_TIMEOUT.as_millis(),
        u128::try_from(golden::i64_of(constants, "explorer_prefetch_timeout_ms")).unwrap()
    );
    assert_eq!(
        FORWARD_COALESCE_WINDOW.as_millis(),
        u128::try_from(golden::i64_of(constants, "forward_coalesce_window_ms")).unwrap()
    );
    // R0002: the Rust prefix is PhotoReviewRs where C# had PhotoReviewShell.
    assert_eq!(golden::str_of(constants, "instance_prefix"), "PhotoReviewShell");
    assert_eq!(pr_app::startup::launch::INSTANCE_NAME_PREFIX, "PhotoReviewRs");
}

#[test]
fn shell_options_match_csharp_golden_including_the_non_env_defaults() {
    // The full shell record (not just EnvOptions): activate, install_process_hooks, start_perf_listener and smoke_timeout come from the same 14 oracle rows.
    let golden = golden::load("shell-startup-options");
    let mut count = 0;
    for case in golden.cases.iter().filter(|c| c["kind"] == "options") {
        let pairs: HashMap<String, String> = case["env"]
            .as_object()
            .expect("env object")
            .iter()
            .map(|(k, v)| {
                (
                    k.replace("PHOTOREVIEW_SHELL_", "PHOTOREVIEW_RS_"),
                    v.as_str().unwrap().to_owned(),
                )
            })
            .collect();
        let options = ShellStartupOptions::from_env(&MapEnv(pairs));
        let id = &case["id"];
        let expected_prefix = golden::str_of(case, "instance_name_prefix");
        assert_eq!(
            options.instance_name_prefix,
            if expected_prefix == "PhotoReviewShell" {
                "PhotoReviewRs"
            } else {
                expected_prefix
            },
            "{id}"
        );
        assert_eq!(options.prefer_warp, golden::bool_of(case, "prefer_warp"), "{id}");
        assert_eq!(options.warm_graphics, golden::bool_of(case, "warm_graphics"), "{id}");
        assert_eq!(options.activate, golden::bool_of(case, "activate"), "{id}");
        assert_eq!(
            options.install_process_hooks,
            golden::bool_of(case, "install_process_hooks"),
            "{id}"
        );
        assert_eq!(
            options.start_perf_listener,
            golden::bool_of(case, "start_perf_listener"),
            "{id}"
        );
        assert_eq!(
            options.smoke_timeout.as_millis(),
            u128::try_from(golden::i64_of(case, "smoke_timeout_ms")).unwrap(),
            "{id}"
        );
        count += 1;
    }
    assert_eq!(count, 14);
}

#[test]
fn option_env_matrix_follows_the_csharp_rules_for_every_combination() {
    // Full cross product of the three variables (the oracle only has single-axis rows). Rules from ShellStartup.cs: prefix = value when non-empty (whitespace kept),
    // WARP true only for exactly "1", warm-up off only for NOWARM exactly "1"; the other members never depend on the environment.
    let prefixes: [Option<&str>; 6] = [
        None,
        Some(""),
        Some(" "),
        Some("P"),
        Some("  test-prefix  "),
        Some("PhotoReviewRs"),
    ];
    let flags: [Option<&str>; 8] = [
        None,
        Some(""),
        Some("0"),
        Some("1"),
        Some(" 1"),
        Some("1 "),
        Some("true"),
        Some("11"),
    ];
    let mut combos = 0;
    for prefix in prefixes {
        for warp in flags {
            for nowarm in flags {
                let mut pairs = Vec::new();
                for (name, value) in [(PREFIX, prefix), (WARP, warp), (NOWARM, nowarm)] {
                    if let Some(value) = value {
                        pairs.push((name, value));
                    }
                }
                let env = env_of(&pairs);
                let options = ShellStartupOptions::from_env(&env);
                let label = format!("{prefix:?}/{warp:?}/{nowarm:?}");
                let expected_prefix = match prefix {
                    Some(p) if !p.is_empty() => p,
                    _ => "PhotoReviewRs",
                };
                assert_eq!(options.instance_name_prefix, expected_prefix, "{label}");
                assert_eq!(options.prefer_warp, warp == Some("1"), "{label}");
                assert_eq!(options.warm_graphics, nowarm != Some("1"), "{label}");
                assert!(
                    options.activate && options.install_process_hooks && options.start_perf_listener,
                    "{label}"
                );
                assert_eq!(options.smoke_timeout, DEFAULT_SMOKE_TIMEOUT, "{label}");
                assert!(
                    options.graph_builder.is_none() && options.journal_recovery.is_none(),
                    "{label}"
                );
                // The pure-half struct agrees with the shell record.
                let pure = EnvOptions::from_env(&env);
                assert_eq!(
                    (pure.instance_name_prefix.as_str(), pure.prefer_warp, pure.warm_graphics),
                    (
                        options.instance_name_prefix.as_str(),
                        options.prefer_warp,
                        options.warm_graphics
                    ),
                    "{label}"
                );
                combos += 1;
            }
        }
    }
    assert_eq!(combos, 6 * 8 * 8);
}

#[test]
fn variables_are_read_by_their_exact_rust_names_only() {
    // The C# names (PHOTOREVIEW_SHELL_*) are renamed by R0002; a set C# name must NOT leak into the options.
    let env = env_of(&[
        ("PHOTOREVIEW_SHELL_INSTANCE_PREFIX", "Old"),
        ("PHOTOREVIEW_SHELL_WARP", "1"),
        ("PHOTOREVIEW_SHELL_NOWARM", "1"),
        ("photoreview_rs_warp", "1"),
    ]);
    let options = ShellStartupOptions::from_env(&env);
    assert_eq!(options.instance_name_prefix, "PhotoReviewRs");
    assert!(!options.prefer_warp);
    assert!(options.warm_graphics);
    assert_eq!(
        (INSTANCE_PREFIX_VAR, PREFER_WARP_VAR, NO_WARM_VAR),
        (PREFIX, WARP, NOWARM)
    );
}
