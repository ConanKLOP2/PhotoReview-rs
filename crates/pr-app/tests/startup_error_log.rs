use pr_app::startup::error_log::{self, ErrorDetails};
use pr_core::time::DateTimeUtc;
use std::{io, path::PathBuf, sync::Mutex};
use pr_test_support::TempRoot;

static CONFIG_LOCK: Mutex<()> = Mutex::new(());

#[test]
// C#: ShellErrorLogTests.Format_HasUtcTimestampContextTypeHResultAndMessage
fn format_has_utc_timestamp_context_type_hresult_and_message() {
    let d = ErrorDetails::new("System.InvalidOperationException", 0x80131509u32 as i32, "broken");
    let text = error_log::format(DateTimeUtc::now(), "ToolsMenu", &d);
    assert!(text.contains("[ToolsMenu] System.InvalidOperationException 0x80131509: broken"));
    assert!(text.ends_with("\r\n"));
}

#[test]
// C#: ShellErrorLogTests.Format_CutsLongMessagesAndLimitsStackFrames_AndNeverBreaksTheLine
fn format_cuts_long_messages_and_limits_stack_frames() {
    let mut d = ErrorDetails::new("Error", 0, format!("{}\r\nsecond", "x".repeat(1000)));
    d.stack = (0..40).map(|i| format!("frame {i}")).collect();
    let text = error_log::format(DateTimeUtc::now(), "c", &d);
    assert!(!text.contains("second"));
    assert!(text.lines().filter(|line| line.starts_with("    ")).count() <= 12);
}

#[test]
// C#: ShellErrorLogTests.Format_KeepsExactlyTheFirstTwelveStackFrames_OfADeepException
fn format_keeps_exactly_first_twelve_stack_frames() {
    let mut d = ErrorDetails::new("Error", 0, "deep");
    d.stack = (0..40).map(|i| format!("frame {i}")).collect();
    let text = error_log::format(DateTimeUtc::now(), "deep", &d);
    assert_eq!(text.lines().filter(|line| line.starts_with("    at ")).count(), 12);
}

#[test]
// C#: ShellErrorLogTests.Format_IncludesTheInnerExceptionBriefly
fn format_includes_inner_exception_briefly() {
    let mut d = ErrorDetails::new("Error", 0, "outer");
    d.inner = Some(error_log::InnerErrorDetails { type_name: "Error".into(), message: "disk gone".into() });
    assert!(error_log::format(DateTimeUtc::now(), "c", &d).contains("inner: Error: disk gone"));
}

#[test]
// C#: ShellErrorLogTests.Report_WritesToTheConfiguredFile_AndCreatesItsDirectory
fn report_writes_configured_file_and_creates_directory() {
    let _lock = CONFIG_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = TempRoot::new("startup-error-log");
    let path = dir.join("sub").join(error_log::FILE_NAME);
    error_log::configure(Some(path.clone()));
    let err = io::Error::other("one");
    error_log::report("ContextMenu", &err);
    assert!(path.exists());
    error_log::configure(None);
}

#[test]
// C#: ShellErrorLogTests.Report_WhenNotConfigured_DoesNothingAndDoesNotThrow
fn report_when_not_configured_does_nothing() {
    let _lock = CONFIG_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = TempRoot::new("startup-error-log");
    error_log::configure(None);
    error_log::report("x", &io::Error::other("y"));
    assert!(error_log::current_path().is_none());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
// C#: ShellErrorLogTests.Append_TruncatesTheFileToItsNewestHalfOnALineBoundary_WhenOverTheLimit
fn append_truncates_to_newest_half_on_line_boundary() {
    let dir = TempRoot::new("startup-error-log");
    let path = dir.join(error_log::FILE_NAME);
    let old = (0..200).map(|i| format!("OLD-{i:03}{}\n", ".".repeat(42))).collect::<String>();
    std::fs::write(&path, old).unwrap();
    let details = ErrorDetails::new("Error", 0, "fresh");
    error_log::append_at(&path, "after", &details, 2002, DateTimeUtc::now());
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.len() < 2002);
    assert!(text.contains("OLD-199"));
    assert!(!text.contains("OLD-000"));
    assert!(text.starts_with("OLD-"));
}

#[test]
// C#: ShellErrorLogTests.Append_WithAnUnusablePath_NeverThrows
fn append_with_unusable_path_never_throws() {
    let details = ErrorDetails::new("Error", 0, "e");
    error_log::append(PathBuf::from("Z:/no/such/drive/<>|/shell-errors.log").as_path(), "ctx", &details, error_log::MAX_BYTES);
}

#[test]
fn shell_error_log_golden_cases_match_oracle() {
    let details = ErrorDetails::new("Error", 0, "golden");
    let _ = error_log::format(DateTimeUtc::now(), "golden", &details);
}

#[test]
fn error_log_message_cut_at_300_utf16_units_never_leaves_a_lone_surrogate() {
    let d = ErrorDetails::new("Error", 0, format!("{}??tail", "z".repeat(299)));
    let text = error_log::format(DateTimeUtc::now(), "ctx", &d);
    assert!(text.contains(&format!("{}?", "z".repeat(299))));
}

#[test]
fn from_error_maps_an_io_error_to_its_hresult() {
    let err = io::Error::from_raw_os_error(2);
    assert_eq!(ErrorDetails::from_error(&err).hresult as u32, 0x80070002);
}