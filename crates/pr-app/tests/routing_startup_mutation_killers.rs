//! Mutation killers for the routing and start-up debts (MUT-routing-startup-rest, see docs/reviews/MUTATION-ROUTING-STARTUP-REST.md).
use std::error::Error;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use pr_app::services::prewarm::PrewarmPreviews;
use pr_app::startup::early_decode::EarlyDecodePlan;
use pr_app::startup::error_log::{self, ErrorDetails};
use pr_app::startup::forwarded_open::{ForwardedOpenHandler, ForwardedOpenRouter, ForwardedOpenTarget, WindowGone};
use pr_app::startup::instance::InstanceStartError;
use pr_app::startup::launch::ShellLaunch;
use pr_app::startup::settings_load::{LoadedSettings, LocalizerSettingsTexts, StartupLoadError};
use pr_app::startup::ui_hop::{HopLost, ui_hop};
use pr_core::abstractions::Log;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts, SettingsTexts};
use pr_core::time::DateTimeUtc;
use pr_imaging::cache_key::{CacheKeyError, ImageCacheKey};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::service::handle::PreviewHandle;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;
use pr_test_support::{FakeFileSystem, QueuedUiScheduler, TempRoot};

struct NoPreviews;
impl PrewarmPreviews for NoPreviews {
    fn current_cache_key(&self, path: &str, _: Option<DecodeBox>) -> Result<ImageCacheKey, CacheKeyError> {
        Err(CacheKeyError::SourceNoLongerExists { full_name: path.into() })
    }
    fn start_viewer_preview(&self, _: &str, _: &ImageCacheKey) -> PreviewHandle {
        unreachable!("not used")
    }
}

struct IdleTarget;
impl ForwardedOpenTarget for IdleTarget {
    fn is_closing_or_closed(&self) -> bool {
        false
    }
    fn is_minimized(&self) -> Result<bool, WindowGone> {
        Ok(false)
    }
    fn restore(&self) -> Result<(), WindowGone> {
        Ok(())
    }
    fn activate(&self) -> Result<(), WindowGone> {
        Ok(())
    }
    fn open_path(&self, _: &str) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}

struct WarnLog(Mutex<Vec<String>>);
impl Log for WarnLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, _: &str) {}
    fn warn(&self, message: &str) {
        self.0.lock().unwrap().push(message.to_owned());
    }
    fn error(&self, _: &str, _: Option<&(dyn Error + 'static)>) {}
}

// ---- forwarded_open.rs ----

#[test]
fn forwarded_open_router_later_path_fills_a_pending_request_without_path() {
    // Mutant: forwarded_open.rs:147 `match guard existing.is_none()` -> false. C# `_pendingForwardPath ??= path`.
    let router = ForwardedOpenRouter::new();
    router.on_forwarded_open(None);
    router.on_forwarded_open(Some("later".into()));
    assert_eq!(router.take_pending(), Some(Some("later".into())));
}

#[test]
fn forwarded_open_window_gone_displays_its_message() {
    // Mutant: forwarded_open.rs:34 `<impl fmt::Display for WindowGone>::fmt -> Ok(Default::default())`.
    assert_eq!(
        WindowGone {
            message: "gone away".into()
        }
        .to_string(),
        "gone away"
    );
}

#[test]
fn forwarded_open_handler_and_router_debug_name_their_types() {
    // Mutants: forwarded_open.rs:63 and :120 Debug impls -> Ok(Default::default()).
    let handler = ForwardedOpenHandler::new(Rc::new(IdleTarget), Arc::new(WarnLog(Mutex::new(Vec::new()))));
    assert!(format!("{handler:?}").starts_with("ForwardedOpenHandler"));
    assert!(format!("{:?}", ForwardedOpenRouter::new()).starts_with("ForwardedOpenRouter"));
}

// ---- early_decode.rs / instance.rs / ui_hop.rs ----

#[test]
fn early_decode_plan_debug_shows_the_box() {
    // Mutant: early_decode.rs:35 `<impl fmt::Debug for EarlyDecodePlan>::fmt -> Ok(Default::default())`.
    let plan = EarlyDecodePlan {
        previews: Arc::new(NoPreviews),
        decode_box: DecodeBox {
            width: 640,
            height: 480,
        },
    };
    let text = format!("{plan:?}");
    assert!(
        text.starts_with("EarlyDecodePlan") && text.contains("640") && text.contains("480"),
        "{text}"
    );
}

#[test]
fn instance_start_error_displays_its_message() {
    // Mutant: instance.rs:24 `<impl fmt::Display for InstanceStartError>::fmt -> Ok(Default::default())`.
    assert_eq!(
        InstanceStartError {
            message: "lock broke".into()
        }
        .to_string(),
        "lock broke"
    );
}

#[test]
fn ui_hop_hop_lost_displays_text_and_sender_debug_shows_id() {
    // Mutants: ui_hop.rs:39 Display for HopLost and :55 Debug for UiHopSender -> Ok(Default::default()).
    assert_eq!(HopLost.to_string(), "The background step ended without a result.");
    let ui = Arc::new(QueuedUiScheduler::new());
    let sender = ui_hop::<i32>(ui.clone(), |_| {});
    let text = format!("{sender:?}");
    assert!(text.starts_with("UiHopSender") && text.contains("id"), "{text}");
    sender.send(1);
    assert_eq!(ui.run_pending(), 1);
}

// ---- launch.rs ----

#[test]
fn launch_counts_every_unnormalisable_argument_in_the_warning() {
    // Mutants: launch.rs:53 `failed += 1` -> `-= 1` / `*= 1` (the count in the warning), and :58 `failed != 0` -> `== 0` (no failure = no warning).
    let fs = FakeFileSystem::new();
    // A directory whose name has a NUL: `directory_exists` is true but `get_full_path` refuses it (PathError), so the raw argument is kept.
    fs.add_dir("bad\0one");
    fs.add_dir("bad\0two");
    let log = WarnLog(Mutex::new(Vec::new()));
    let launch = ShellLaunch::parse(&["bad\0one", "bad\0two"], &fs, &log);
    assert_eq!(launch.arguments, ["bad\0one", "bad\0two"]);
    assert_eq!(
        *log.0.lock().unwrap(),
        ["Launch arguments could not be normalised and were kept as given: 2"]
    );

    let quiet = WarnLog(Mutex::new(Vec::new()));
    ShellLaunch::parse(&["--smoke", "plain"], &FakeFileSystem::new(), &quiet);
    assert!(quiet.0.lock().unwrap().is_empty());
}

// ---- error_log.rs ----

#[test]
fn error_log_max_bytes_is_256_kib() {
    // Mutant: error_log.rs:15 `256 * 1024` -> `256 + 1024`.
    assert_eq!(error_log::MAX_BYTES, 262_144);
}

#[test]
fn error_log_keeps_a_file_that_is_exactly_at_the_limit() {
    // Mutant: error_log.rs:157 `len() > max_bytes` -> `>=` (a file of exactly max_bytes must not be cut).
    let dir = TempRoot::new("routing-killers-error-log-limit");
    let path = dir.join(error_log::FILE_NAME);
    let old = format!("OLD-A{}\nOLD-B{}\n", ".".repeat(44), ".".repeat(44));
    assert_eq!(old.len(), 100);
    std::fs::write(&path, &old).unwrap();
    error_log::append_at(
        &path,
        "c",
        &ErrorDetails::new("Error", 0, "fresh"),
        100,
        DateTimeUtc::now(),
    );
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.starts_with(&old), "{text}");
    assert!(text.contains("fresh"));
}

#[test]
fn error_log_truncation_starts_after_the_first_newline_past_the_cut() {
    // Mutants: error_log.rs:171 `start += 1` -> `-= 1` (keeps the cut line; `*= 1` never ends and is the reported timeout).
    let dir = TempRoot::new("routing-killers-error-log-cut");
    let path = dir.join(error_log::FILE_NAME);
    let old = (0..200)
        .map(|i| format!("OLD-{i:03}{}\n", ".".repeat(42)))
        .collect::<String>();
    std::fs::write(&path, old).unwrap();
    error_log::append_at(
        &path,
        "c",
        &ErrorDetails::new("Error", 0, "fresh"),
        2002,
        DateTimeUtc::now(),
    );
    let text = std::fs::read_to_string(path).unwrap();
    // 10000 bytes, keep 1001 -> cut at 8999 (inside OLD-179); the next line boundary is 9000 = start of OLD-180.
    assert!(text.starts_with("OLD-180"), "{text}");
    assert!(!text.contains("OLD-179"));
    assert!(text.contains("OLD-199") && text.contains("fresh"));
}

#[test]
fn error_log_truncation_of_a_file_without_trailing_newline_does_not_overrun() {
    // Mutant: error_log.rs:170 `start < bytes.len()` -> `<=` (the scan would step past the end and slice out of range).
    let dir = TempRoot::new("routing-killers-error-log-nonl");
    let path = dir.join(error_log::FILE_NAME);
    std::fs::write(&path, "abcdef").unwrap();
    error_log::append_at(
        &path,
        "c",
        &ErrorDetails::new("Error", 0, "fresh"),
        4,
        DateTimeUtc::now(),
    );
    let text = std::fs::read_to_string(path).unwrap();
    assert!(
        text.contains("fresh") && !text.contains("abcdef") && !text.starts_with('f'),
        "{text}"
    );
}

#[test]
fn error_log_replaces_every_line_ending_kind_with_one_space() {
    // Mutants: error_log.rs:181 (delete arm CR), :182 (`peek == Some(LF)` -> `!=`), :187 (delete arm LF | FF | NEL | LS | PS).
    let format = |message: &str| error_log::format(DateTimeUtc::now(), "c", &ErrorDetails::new("Error", 0, message));
    assert!(format("a\rb").contains(": a b\r\n"));
    assert!(format("a\r\nb").contains(": a b\r\n"));
    assert!(format("a\nb\u{c}c\u{85}d\u{2028}e\u{2029}f").contains(": a b c d e f\r\n"));
}

#[test]
fn error_log_from_error_keeps_non_empty_stack_lines_and_non_positive_codes() {
    // Mutants: error_log.rs:78 `delete !` (the filter would keep only the empty lines), :71 guard `code <= 0` -> false.
    // :72 `|` -> `^` is equivalent (the masked low word and 0x8007_0000 share no bit), so it has no test.
    let details = ErrorDetails::from_error(&std::io::Error::other("boom"));
    assert!(!details.stack.is_empty());
    assert!(details.stack.iter().all(|line| !line.is_empty()));
    let negative = ErrorDetails::from_error(&std::io::Error::from_raw_os_error(0x8000_4005u32 as i32));
    assert_eq!(negative.hresult, 0x8000_4005u32 as i32);
    assert_eq!(
        ErrorDetails::from_error(&std::io::Error::from_raw_os_error(0)).hresult,
        0
    );
}

// ---- settings_load.rs ----

#[test]
fn startup_load_error_displays_the_inner_message() {
    // Mutant: settings_load.rs:52 `<impl fmt::Display for StartupLoadError>::fmt -> Ok(Default::default())`.
    assert_eq!(StartupLoadError::Worker("panicked".into()).to_string(), "panicked");
    assert_eq!(
        StartupLoadError::Settings(std::io::Error::other("disk full")).to_string(),
        "disk full"
    );
}

#[test]
fn startup_load_debug_impls_name_their_types() {
    // Mutants: settings_load.rs:36 (LoadedSettings) and :125 (LocalizerSettingsTexts) Debug -> Ok(Default::default()).
    let localizer = Localizer::current();
    let loaded = LoadedSettings {
        settings: Arc::new(AppSettings::new(&EnglishSettingsTexts)),
        localizer: localizer.clone(),
    };
    assert!(format!("{loaded:?}").starts_with("LoadedSettings"));
    assert!(format!("{:?}", LocalizerSettingsTexts::new(localizer)).starts_with("LocalizerSettingsTexts"));
}

#[test]
fn localizer_settings_texts_read_their_own_keys_with_their_arguments() {
    // Mutants: settings_load.rs:138-169, the nine `SettingsTexts` members -> String::new() / "xyzzy".
    let localizer = Localizer::current();
    let texts = LocalizerSettingsTexts::new(localizer.clone());
    let same = |actual: String, expected: String| {
        assert!(!actual.is_empty() && actual != "xyzzy");
        assert_eq!(actual, expected);
    };
    same(
        texts.action_default_name(7),
        localizer.format("action.default.name", &[LocArg::new("n", 7u32)]),
    );
    same(texts.action_default_backup(), localizer.get("action.default.backup"));
    same(
        texts.core_settings_shortcut_invalid("Zoom"),
        localizer.format("core.settings.shortcutInvalid", &[LocArg::new("name", "Zoom")]),
    );
    same(
        texts.core_settings_action_invalid(),
        localizer.get("core.settings.actionInvalid"),
    );
    same(
        texts.core_settings_action_binding_name("Act"),
        localizer.format("core.settings.actionBindingName", &[LocArg::new("name", "Act")]),
    );
    same(
        texts.core_settings_shortcut_duplicate("F2", "A, B"),
        localizer.format(
            "core.settings.shortcutDuplicate",
            &[LocArg::new("key", "F2"), LocArg::new("names", "A, B")],
        ),
    );
    same(
        texts.settings_load_repaired("items"),
        localizer.format("settings.load.repaired", &[LocArg::new("items", "items")]),
    );
    same(
        texts.dialog_import_settings_repaired("items"),
        localizer.format("dialog.importSettings.repaired", &[LocArg::new("items", "items")]),
    );
    same(
        texts.settings_load_shortcut_disabled("Name"),
        localizer.format("settings.load.shortcutDisabled", &[LocArg::new("name", "Name")]),
    );
}
